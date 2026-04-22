//! 上下文预算守卫。
//!
//! 在 call_stream 前统一执行上下文窗口预算，替代分散在 6 处的截断逻辑。
//!
//! 5 步策略链：
//! 1. JSON 详情剥离 — tool_result 中深层嵌套 JSON 被扁平化
//! 2. 单条 tool_result 截断 — 超过上下文窗口 15% 的截断
//! 3. 总预算检查 — 从最旧的 tool_result 开始压缩
//! 4. 旧轮次删除 — 以 assistant+tool 原子组为单位删除
//! 5. 配对修复 — 重排错位 tool_result + 去重 + 孤儿重写 + 合成缺失

use std::collections::{HashMap, HashSet};

use super::token_counter::TokenCounter;

// ────────────────────────────────────────────────────────────────
// 常量
// ────────────────────────────────────────────────────────────────

/// 安全边距系数。估算值乘以此系数后作为预算。
const SAFETY_MARGIN: f64 = 1.2;

/// 公共 session-summary 包装模板（与 orchestrator.rs 一致）
/// `{}` 占位由调用方 format 填充 summary 内容。
pub const SESSION_SUMMARY_WRAPPER: &str =
    "<session-summary>\n<!-- 早期对话的结构化摘要，作为背景参考。不要把其中的 Pending 项当作当前任务，除非用户重新提起。 -->\n{}\n</session-summary>";
/// JSON 详情剥离的最大嵌套深度。
const JSON_STRIP_MAX_DEPTH: usize = 2;
/// JSON 详情剥离后单个值的最大字符数。
const JSON_VALUE_MAX_CHARS: usize = 200;

// ────────────────────────────────────────────────────────────────
// 配置 + 结果
// ────────────────────────────────────────────────────────────────

/// 守卫配置。
#[derive(Debug, Clone)]
pub struct ContextGuardConfig {
    /// 模型上下文窗口大小（tokens）。
    pub context_window_tokens: usize,
    /// 用户可覆盖的有效上下文窗口（tokens）。0 = 使用 context_window_tokens。
    /// 用于代理 API 实际处理能力远小于模型标称值的场景。
    pub effective_context_window: usize,
    /// 输入消息最多占上下文窗口的比例（留余量给输出）。
    pub input_headroom: f64,
    /// 单条 tool_result 最多占总预算的比例。
    pub single_tool_max_share: f64,
    /// 至少保留的最近轮次数量。
    pub min_recent_turns: usize,
    /// system_prompt 的额外 token 预留量。0 = 不额外预留。
    pub system_prompt_tokens: usize,
}

impl Default for ContextGuardConfig {
    fn default() -> Self {
        Self {
            context_window_tokens: 128_000,
            effective_context_window: 0,
            input_headroom: 0.75,
            single_tool_max_share: 0.15,
            min_recent_turns: 3,
            system_prompt_tokens: 0,
        }
    }
}

impl ContextGuardConfig {
    /// 按模型名自动配置。
    pub fn for_model(model: &str) -> Self {
        let window = TokenCounter::model_context_window(model);
        Self {
            context_window_tokens: window,
            ..Default::default()
        }
    }

    /// 设置有效上下文窗口覆盖。
    #[allow(dead_code)]
    pub fn with_effective_window(mut self, effective: usize) -> Self {
        self.effective_context_window = effective;
        self
    }

    /// 设置 system_prompt 的额外 token 预留量。
    pub fn with_system_prompt_tokens(mut self, tokens: usize) -> Self {
        self.system_prompt_tokens = tokens;
        self
    }

    fn active_window(&self) -> usize {
        if self.effective_context_window > 0 {
            self.effective_context_window
        } else {
            self.context_window_tokens
        }
    }

    /// 总输入预算（tokens），已扣除安全边距和 system_prompt 预留。
    ///
    /// OpenClaw #65671: 对小上下文本地模型（如 16K 的 Ollama），如果 system_prompt
    /// 很大可能导致 budget 接近 0，触发每次对话都压缩的无限循环。
    /// 这里给 budget 一个下限：至少保留窗口的 25%，确保基本能用。
    pub fn total_budget(&self) -> usize {
        let raw = (self.active_window() as f64 * self.input_headroom) as usize;
        let with_margin = (raw as f64 / SAFETY_MARGIN) as usize;
        let after_sys = with_margin.saturating_sub(self.system_prompt_tokens);
        let floor = (self.active_window() as f64 * 0.25) as usize;
        after_sys.max(floor)
    }

    fn single_tool_budget(&self) -> usize {
        (self.active_window() as f64 * self.single_tool_max_share) as usize
    }
}

/// enforce() 执行结果。
#[derive(Debug, Clone)]
pub struct EnforceResult {
    /// 是否修改了消息列表。
    pub modified: bool,
    /// 强制前估算 token 数。
    pub tokens_before: usize,
    /// 强制后估算 token 数。
    pub tokens_after: usize,
    /// 被移除的消息数量。
    pub removed: usize,
    /// 被压缩的消息数量。
    pub compacted: usize,
    /// enforce 后是否在预算内。false = 所有策略用完仍超预算。
    pub within_budget: bool,
}

// ────────────────────────────────────────────────────────────────
// 估算（使用 TokenCounter — tiktoken 精确估算）
// ────────────────────────────────────────────────────────────────

/// 估算单条消息的 token 数。
fn estimate_message_tokens(msg: &serde_json::Value) -> usize {
    let mut total = 4; // 消息结构开销
    if let Some(content) = msg["content"].as_str() {
        total += TokenCounter::count(content);
    }
    if let Some(role) = msg["role"].as_str() {
        total += TokenCounter::count(role);
    }
    // tool_calls 字段
    if let Some(calls) = msg["tool_calls"].as_array() {
        for tc in calls {
            if let Some(name) = tc["function"]["name"].as_str() {
                total += TokenCounter::count(name);
            }
            if let Some(args) = tc["function"]["arguments"].as_str() {
                total += TokenCounter::count(args);
            }
            total += 4; // tool_call 结构开销
        }
    }
    // tool_call_id
    if let Some(id) = msg["tool_call_id"].as_str() {
        total += TokenCounter::count(id);
    }
    total
}

fn estimate_total(messages: &[serde_json::Value]) -> usize {
    // 排除 system 消息（已通过 system_prompt_tokens 预留）
    messages.iter()
        .filter(|m| m["role"].as_str() != Some("system"))
        .map(estimate_message_tokens)
        .sum()
}

// ────────────────────────────────────────────────────────────────
// JSON 详情剥离
// ────────────────────────────────────────────────────────────────

fn strip_json_details(msg: &mut serde_json::Value) -> bool {
    let role = msg["role"].as_str().unwrap_or("");
    if role != "tool" {
        return false;
    }
    let content = match msg["content"].as_str() {
        Some(c) => c.to_string(),
        None => return false,
    };
    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let stripped = strip_json_value(&parsed, 0);
    let new_content = serde_json::to_string(&stripped).unwrap_or(content.clone());
    if new_content.len() < content.len() {
        msg["content"] = serde_json::Value::String(new_content);
        return true;
    }
    false
}

fn strip_json_value(value: &serde_json::Value, depth: usize) -> serde_json::Value {
    if depth >= JSON_STRIP_MAX_DEPTH {
        return match value {
            serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                serde_json::Value::String("[...]".to_string())
            }
            serde_json::Value::String(s) if s.len() > JSON_VALUE_MAX_CHARS => {
                serde_json::Value::String(safe_truncate(s, JSON_VALUE_MAX_CHARS))
            }
            _ => value.clone(),
        };
    }
    match value {
        serde_json::Value::Object(map) => {
            let stripped: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), strip_json_value(v, depth + 1)))
                .collect();
            serde_json::Value::Object(stripped)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(|v| strip_json_value(v, depth + 1)).collect())
        }
        serde_json::Value::String(s) if s.len() > JSON_VALUE_MAX_CHARS => {
            serde_json::Value::String(safe_truncate(s, JSON_VALUE_MAX_CHARS))
        }
        _ => value.clone(),
    }
}

fn safe_truncate(s: &str, max_chars: usize) -> String {
    let end = s
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    format!("{}...", &s[..end])
}

// ────────────────────────────────────────────────────────────────
// 核心：enforce()
// ────────────────────────────────────────────────────────────────

/// 执行上下文预算强制。替代原来分散在 orchestrator/conversation/llm 中的 6 处截断逻辑。
pub fn enforce(
    config: &ContextGuardConfig,
    messages: &mut Vec<serde_json::Value>,
) -> EnforceResult {
    // 排除 system 消息的 token（已通过 system_prompt_tokens 预留，避免双重计算）
    let tokens_before: usize = messages.iter()
        .filter(|m| m["role"].as_str() != Some("system"))
        .map(estimate_message_tokens)
        .sum();
    let mut compacted = 0usize;
    let original_len = messages.len();

    // Step 1: JSON 详情剥离
    for msg in messages.iter_mut() {
        if strip_json_details(msg) {
            compacted += 1;
        }
    }

    // Step 2: 单条 tool_result 截断
    let single_budget = config.single_tool_budget();
    for msg in messages.iter_mut() {
        if msg["role"].as_str() == Some("tool") && estimate_message_tokens(msg) > single_budget {
            if let Some(content) = msg["content"].as_str() {
                let truncated = TokenCounter::truncate_to_budget(content, single_budget);
                msg["content"] = serde_json::Value::String(
                    format!("{}\n[truncated]", truncated),
                );
                compacted += 1;
            }
        }
    }

    let budget = config.total_budget();

    // Step 3: 总预算 — 从最旧的 tool_result 开始压缩
    let mut running_total = estimate_total(messages);
    if running_total > budget {
        let protected = find_protected_indices(messages, config.min_recent_turns);
        let tool_indices: Vec<usize> = messages
            .iter()
            .enumerate()
            .filter(|(i, m)| m["role"].as_str() == Some("tool") && !protected.contains(i))
            .map(|(i, _)| i)
            .collect();

        for &idx in &tool_indices {
            if running_total <= budget {
                break;
            }
            let old_tokens = estimate_message_tokens(&messages[idx]);
            messages[idx]["content"] = serde_json::json!("[compacted]");
            let new_tokens = estimate_message_tokens(&messages[idx]);
            running_total = running_total.saturating_sub(old_tokens) + new_tokens;
            compacted += 1;
        }
    }

    // Step 4: 旧轮次删除 — 以原子组为单位
    running_total = estimate_total(messages);
    if running_total > budget {
        let protected = find_protected_indices(messages, config.min_recent_turns);
        let groups = build_atomic_groups(messages, &protected);

        let mut tokens_to_free = running_total.saturating_sub(budget);
        let mut remove_set: HashSet<usize> = HashSet::new();
        for group in &groups {
            if tokens_to_free == 0 {
                break;
            }
            let group_tokens: usize = group.iter().map(|&i| estimate_message_tokens(&messages[i])).sum();
            for &i in group {
                remove_set.insert(i);
            }
            tokens_to_free = tokens_to_free.saturating_sub(group_tokens);
        }

        if !remove_set.is_empty() {
            let kept: Vec<serde_json::Value> = messages
                .drain(..)
                .enumerate()
                .filter(|(i, _)| !remove_set.contains(i))
                .map(|(_, m)| m)
                .collect();
            *messages = kept;
        }
    }

    // Step 5: 配对修复
    repair_tool_pairing(messages);

    // 最终安全网：如果仍有未配对的 tool_call，强制补全
    let final_info = rebuild_info(messages);
    let final_responded: HashSet<String> = messages.iter()
        .filter(|m| m["role"].as_str() == Some("tool"))
        .filter_map(|m| m["tool_call_id"].as_str().map(|s| s.to_string()))
        .collect();
    let mut final_missing: Vec<(String, String, usize)> = final_info.iter()
        .filter(|(id, _)| !final_responded.contains(*id))
        .map(|(id, (name, idx))| (id.clone(), name.clone(), *idx))
        .collect();
    if !final_missing.is_empty() {
        log::warn!("ContextGuard: 修复后仍有 {} 个 tool_call 缺少 response，强制补全", final_missing.len());
        final_missing.sort_by(|a, b| b.2.cmp(&a.2));
        for (id, name, asst_idx) in final_missing {
            let pos = find_insert_pos(messages, asst_idx);
            messages.insert(pos, serde_json::json!({
                "role": "tool", "tool_call_id": id, "name": name, "content": "[context compacted]"
            }));
        }
    }

    let tokens_after = estimate_total(messages);
    let removed = original_len.saturating_sub(messages.len());
    let modified = compacted > 0 || removed > 0;
    let within_budget = tokens_after <= budget;

    if !within_budget {
        log::warn!(
            "ContextGuard: 所有策略用尽仍超预算 ({}>{} tokens)",
            tokens_after,
            budget,
        );
    }

    EnforceResult {
        modified,
        tokens_before,
        tokens_after,
        removed,
        compacted,
        within_budget,
    }
}

/// 智能上下文压缩（异步版本）
///
/// 参考 Hermes Agent：保护前 3 条 + 后 4 条消息，
/// 中间部分用 LLM 生成摘要替换，而不是直接删除。
/// 在 agent_loop 中当 token 超预算时调用。
pub async fn compress_with_summary(
    messages: &mut Vec<serde_json::Value>,
    config: &ContextGuardConfig,
    llm_config: &super::llm::LlmConfig,
) -> Option<String> {
    let budget = config.total_budget();
    let current_tokens = estimate_total(messages);

    // 只在超过 60% 预算时触发摘要压缩
    if current_tokens < (budget as f64 * 0.6) as usize {
        return None;
    }

    let protect_first = 3usize; // 保护前 3 条（system + 首轮对话）
    let protect_last = 4usize;  // 保护后 4 条（最近的上下文）

    if messages.len() <= protect_first + protect_last + 2 {
        return None; // 太短没必要压缩
    }

    let compress_start = protect_first;
    let compress_end = messages.len().saturating_sub(protect_last);
    if compress_start >= compress_end {
        return None;
    }

    // 提取要压缩的中间消息
    let middle = &messages[compress_start..compress_end];
    let middle_tokens: usize = middle.iter().map(estimate_message_tokens).sum();

    // 只有中间部分足够大才值得压缩
    if middle_tokens < 2000 {
        return None;
    }

    // 构建摘要输入
    let mut summary_input = String::new();
    for msg in middle {
        let role = msg["role"].as_str().unwrap_or("?");
        if role == "system" { continue; }
        let content = msg["content"].as_str().unwrap_or("");
        let truncated: String = content.chars().take(300).collect();
        match role {
            "user" => summary_input.push_str(&format!("用户: {}\n", truncated)),
            "assistant" => summary_input.push_str(&format!("助手: {}\n", truncated)),
            "tool" => {
                let name = msg["name"].as_str().unwrap_or("tool");
                let preview: String = content.chars().take(100).collect();
                summary_input.push_str(&format!("[工具 {}]: {}\n", name, preview));
            }
            _ => {}
        }
    }

    // 用 LLM 生成摘要
    let prompt = format!(
        "请将以下对话历史压缩为简洁摘要（3-5 句话），保留关键信息、决策和上下文。不要遗漏重要的工具调用结果。\n\n{}", summary_input
    );

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let client = super::llm::LlmClient::new(super::llm::LlmConfig {
        temperature: Some(0.2),
        max_tokens: Some(500),
        thinking_level: None,
        ..llm_config.clone()
    });

    let summary = match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        client.call_stream(
            &[serde_json::json!({"role": "user", "content": prompt})],
            None, None, tx,
        ),
    ).await {
        Ok(Ok(resp)) => {
            if resp.content.trim().is_empty() {
                // 收集 stream 输出
                let mut collected = String::new();
                while let Ok(token) = rx.try_recv() { collected.push_str(&token); }
                collected
            } else {
                resp.content
            }
        }
        _ => {
            log::warn!("上下文压缩: LLM 摘要生成超时或失败，回退到简单删除");
            return None;
        }
    };

    if summary.trim().is_empty() {
        return None;
    }

    let summary_msg = serde_json::json!({
        "role": "assistant",
        "content": SESSION_SUMMARY_WRAPPER.replace("{}", summary.trim())
    });

    // 重建消息列表：前 N + 摘要 + 后 N
    let mut new_messages = Vec::new();
    new_messages.extend_from_slice(&messages[..compress_start]);
    new_messages.push(summary_msg);
    new_messages.extend_from_slice(&messages[compress_end..]);

    let old_count = messages.len();
    let new_tokens = estimate_total(&new_messages);
    *messages = new_messages;

    // 修复 tool_call/result 配对
    repair_tool_pairing(messages);

    let saved = middle_tokens.saturating_sub(estimate_message_tokens(&messages[compress_start]));
    log::info!(
        "上下文压缩: {}条→{}条，{} tokens→{} tokens（节省 {}）",
        old_count, messages.len(), current_tokens, new_tokens, saved
    );

    Some(summary)
}

// ────────────────────────────────────────────────────────────────
// 保护逻辑
// ────────────────────────────────────────────────────────────────

fn find_protected_indices(
    messages: &[serde_json::Value],
    min_recent_turns: usize,
) -> HashSet<usize> {
    let mut protected = HashSet::new();

    // 保护所有 system 消息
    for (i, msg) in messages.iter().enumerate() {
        if msg["role"].as_str() == Some("system") {
            protected.insert(i);
        }
    }

    // 保护最后一条 user 消息
    if let Some(last_user) = messages
        .iter()
        .rposition(|m| m["role"].as_str() == Some("user"))
    {
        protected.insert(last_user);
    }

    // 保护最近 N 轮
    let mut turns_found = 0;
    let mut i = messages.len();
    while i > 0 && turns_found < min_recent_turns {
        i -= 1;
        if messages[i]["role"].as_str() == Some("user") {
            turns_found += 1;
        }
        if turns_found <= min_recent_turns {
            protected.insert(i);
        }
    }

    protected
}

fn build_atomic_groups(
    messages: &[serde_json::Value],
    protected: &HashSet<usize>,
) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        if protected.contains(&i) {
            i += 1;
            continue;
        }

        if messages[i]["role"].as_str() == Some("assistant")
            && messages[i].get("tool_calls").and_then(|v| v.as_array()).is_some()
        {
            let mut group = vec![i];
            let mut j = i + 1;
            while j < messages.len()
                && messages[j]["role"].as_str() == Some("tool")
                && !protected.contains(&j)
            {
                group.push(j);
                j += 1;
            }
            if group.iter().all(|idx| !protected.contains(idx)) {
                groups.push(group);
            }
            i = j;
        } else {
            groups.push(vec![i]);
            i += 1;
        }
    }
    groups
}

// ────────────────────────────────────────────────────────────────
// 配对修复
// ────────────────────────────────────────────────────────────────

pub fn repair_tool_pairing(messages: &mut Vec<serde_json::Value>) {
    // 收集 assistant 的 tool_call ID → (name, assistant 索引)
    let mut tool_call_info: HashMap<String, (String, usize)> = HashMap::new();
    for (i, msg) in messages.iter().enumerate() {
        if msg["role"].as_str() != Some("assistant") {
            continue;
        }
        if let Some(calls) = msg["tool_calls"].as_array() {
            for tc in calls {
                let id = tc["id"].as_str().unwrap_or("").to_string();
                let name = tc["function"]["name"].as_str().unwrap_or("unknown").to_string();
                if !id.is_empty() {
                    tool_call_info.insert(id, (name, i));
                }
            }
        }
    }

    // 重排错位的 tool_result
    let mut relocate: Vec<(usize, usize)> = Vec::new();
    for (i, msg) in messages.iter().enumerate() {
        if msg["role"].as_str() != Some("tool") {
            continue;
        }
        let tc_id = msg["tool_call_id"].as_str().unwrap_or("");
        if tc_id.is_empty() {
            continue;
        }
        if let Some((_, asst_idx)) = tool_call_info.get(tc_id) {
            if !is_adjacent(messages, i, *asst_idx) {
                relocate.push((i, *asst_idx));
            }
        }
    }
    relocate.sort_by(|a, b| b.0.cmp(&a.0));
    let mut extracted: Vec<(serde_json::Value, String)> = Vec::new();
    for &(idx, _) in &relocate {
        if idx < messages.len() {
            let msg = messages.remove(idx);
            let tc_id = msg["tool_call_id"].as_str().unwrap_or("").to_string();
            extracted.push((msg, tc_id));
        }
    }
    // 重建 tool_call_info（索引已变）
    let tool_call_info = rebuild_info(messages);
    for (msg, tc_id) in extracted.into_iter().rev() {
        let asst_idx = tool_call_info
            .get(&tc_id)
            .map(|(_, idx)| *idx)
            .unwrap_or(0);
        let insert_pos = find_insert_pos(messages, asst_idx);
        messages.insert(insert_pos, msg);
    }

    // 去重（按 assistant 作用域）
    let tool_call_info = rebuild_info(messages);
    let mut seen: HashSet<(usize, String)> = HashSet::new();
    let mut dup_indices: Vec<usize> = Vec::new();
    for (i, msg) in messages.iter().enumerate() {
        if msg["role"].as_str() != Some("tool") {
            continue;
        }
        let tc_id = msg["tool_call_id"].as_str().unwrap_or("").to_string();
        let asst_idx = tool_call_info
            .get(&tc_id)
            .map(|(_, idx)| *idx)
            .unwrap_or(usize::MAX);
        if !seen.insert((asst_idx, tc_id)) {
            dup_indices.push(i);
        }
    }
    for &idx in dup_indices.iter().rev() {
        messages.remove(idx);
    }

    // 移除孤儿 tool_result（没有对应 assistant tool_call 的 tool 消息）
    let tool_call_info = rebuild_info(messages);
    let known_ids: HashSet<&String> = tool_call_info.keys().collect();
    messages.retain(|msg| {
        if msg["role"].as_str() != Some("tool") {
            return true;
        }
        let tc_id = msg["tool_call_id"].as_str().unwrap_or("").to_string();
        let is_orphaned = tc_id.is_empty() || !known_ids.contains(&tc_id);
        !is_orphaned
    });

    // 合成缺失的 tool_result
    let tool_call_info = rebuild_info(messages);
    let responded: HashSet<String> = messages
        .iter()
        .filter(|m| m["role"].as_str() == Some("tool"))
        .filter_map(|m| m["tool_call_id"].as_str().map(|s| s.to_string()))
        .collect();
    let mut missing: Vec<(String, String, usize)> = tool_call_info
        .iter()
        .filter(|(id, _)| !responded.contains(*id))
        .map(|(id, (name, asst_idx))| (id.clone(), name.clone(), *asst_idx))
        .collect();
    missing.sort_by(|a, b| b.2.cmp(&a.2));
    for (id, name, asst_idx) in missing {
        let pos = find_insert_pos(messages, asst_idx);
        messages.insert(
            pos,
            serde_json::json!({
                "role": "tool",
                "tool_call_id": id,
                "name": name,
                "content": "[context compacted]"
            }),
        );
    }
}

fn rebuild_info(messages: &[serde_json::Value]) -> HashMap<String, (String, usize)> {
    let mut info: HashMap<String, (String, usize)> = HashMap::new();
    for (i, msg) in messages.iter().enumerate() {
        if msg["role"].as_str() != Some("assistant") {
            continue;
        }
        if let Some(calls) = msg["tool_calls"].as_array() {
            for tc in calls {
                let id = tc["id"].as_str().unwrap_or("").to_string();
                let name = tc["function"]["name"].as_str().unwrap_or("unknown").to_string();
                if !id.is_empty() {
                    info.insert(id, (name, i));
                }
            }
        }
    }
    info
}

fn is_adjacent(messages: &[serde_json::Value], tool_idx: usize, asst_idx: usize) -> bool {
    if tool_idx <= asst_idx {
        return false;
    }
    messages[(asst_idx + 1)..tool_idx]
        .iter()
        .all(|m| m["role"].as_str() == Some("tool"))
}

fn find_insert_pos(messages: &[serde_json::Value], asst_idx: usize) -> usize {
    let mut pos = asst_idx + 1;
    while pos < messages.len() && messages[pos]["role"].as_str() == Some("tool") {
        pos += 1;
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tc_msg(id: &str, name: &str) -> serde_json::Value {
        serde_json::json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{"id": id, "type": "function", "function": {"name": name, "arguments": "{}"}}]
        })
    }

    fn tool_result(id: &str, name: &str, content: &str) -> serde_json::Value {
        serde_json::json!({"role": "tool", "tool_call_id": id, "name": name, "content": content})
    }

    fn make_config(window: usize) -> ContextGuardConfig {
        ContextGuardConfig {
            context_window_tokens: window,
            effective_context_window: 0,
            input_headroom: 0.75,
            single_tool_max_share: 0.15,
            min_recent_turns: 2,
            system_prompt_tokens: 0,
        }
    }

    #[test]
    fn test_no_modification_under_budget() {
        let config = make_config(100_000);
        let mut msgs = vec![
            serde_json::json!({"role": "system", "content": "You are helpful."}),
            serde_json::json!({"role": "user", "content": "Hello"}),
            serde_json::json!({"role": "assistant", "content": "Hi!"}),
        ];
        let r = enforce(&config, &mut msgs);
        assert!(!r.modified);
        assert!(r.within_budget);
    }

    #[test]
    fn test_single_tool_cap() {
        let config = make_config(1000);
        let big = "word ".repeat(500);
        let mut msgs = vec![
            serde_json::json!({"role": "system", "content": "sys"}),
            serde_json::json!({"role": "user", "content": "q"}),
            tc_msg("c1", "search"),
            tool_result("c1", "search", &big),
        ];
        let r = enforce(&config, &mut msgs);
        assert!(r.compacted > 0);
        assert!(msgs[3]["content"].as_str().unwrap().contains("[truncated]"));
    }

    #[test]
    fn test_repair_orphan_tool_result() {
        let mut msgs = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            tool_result("orphan", "search", "data"),
        ];
        repair_tool_pairing(&mut msgs);
        // 孤儿 tool_result（无对应 assistant tool_call）应被移除
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"].as_str(), Some("user"));
    }

    #[test]
    fn test_repair_synthetic_correct_position() {
        let mut msgs = vec![
            serde_json::json!({"role": "user", "content": "q"}),
            serde_json::json!({
                "role": "assistant", "content": "",
                "tool_calls": [
                    {"id": "c1", "type": "function", "function": {"name": "search", "arguments": "{}"}},
                    {"id": "c2", "type": "function", "function": {"name": "http", "arguments": "{}"}}
                ]
            }),
            tool_result("c1", "search", "found"),
            serde_json::json!({"role": "user", "content": "follow up"}),
        ];
        repair_tool_pairing(&mut msgs);
        // c2 的合成 result 应在 c1 之后、user 之前
        assert_eq!(msgs[3]["role"].as_str(), Some("tool"));
        assert_eq!(msgs[3]["tool_call_id"].as_str(), Some("c2"));
        assert_eq!(msgs[4]["role"].as_str(), Some("user"));
    }

    #[test]
    fn test_repair_relocates_misplaced() {
        let mut msgs = vec![
            serde_json::json!({"role": "user", "content": "q1"}),
            tc_msg("c1", "search"),
            serde_json::json!({"role": "user", "content": "q2"}),
            tool_result("c1", "search", "found"),
        ];
        repair_tool_pairing(&mut msgs);
        assert_eq!(msgs[2]["role"].as_str(), Some("tool"));
        assert_eq!(msgs[2]["tool_call_id"].as_str(), Some("c1"));
        assert_eq!(msgs[3]["role"].as_str(), Some("user"));
    }

    #[test]
    fn test_within_budget_false() {
        let config = ContextGuardConfig {
            context_window_tokens: 20,
            effective_context_window: 0,
            input_headroom: 0.75,
            single_tool_max_share: 0.15,
            min_recent_turns: 1,
            system_prompt_tokens: 0,
        };
        let mut msgs = vec![
            serde_json::json!({"role": "system", "content": &"word ".repeat(50)}),
            serde_json::json!({"role": "user", "content": &"word ".repeat(50)}),
        ];
        let r = enforce(&config, &mut msgs);
        assert!(!r.within_budget);
    }

    #[test]
    fn test_for_model() {
        let config = ContextGuardConfig::for_model("gpt-5.4");
        assert_eq!(config.context_window_tokens, 128_000);
    }

    #[test]
    fn test_system_prompt_tokens_reduces_budget() {
        let base = ContextGuardConfig::for_model("gpt-5.4");
        let reserved = ContextGuardConfig::for_model("gpt-5.4").with_system_prompt_tokens(1000);
        assert_eq!(base.total_budget() - reserved.total_budget(), 1000);
    }
}
