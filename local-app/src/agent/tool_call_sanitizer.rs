//! 工具调用 ID/Name 清洗 + 消息配对修复。
//!
//! 参照 OpenClaw 的 sanitizeSessionHistory 防御策略：
//! 1. 空 ID 自动生成、无效字符清理
//! 2. 工具名验证
//! 3. tool_use/tool_result 配对修复（重排 + 合成缺失 + 去重 + 删孤儿）
//! 4. 严格 provider 的轮次顺序校验

use std::collections::{HashMap, HashSet};

// ────────────────────────────────────────────────────────────────
// 1. ID / Name 清洗
// ────────────────────────────────────────────────────────────────

/// 清洗 tool_call ID — 空 ID 自动生成，无效字符清理
pub fn sanitize_tool_call_id(id: &str, counter: &mut usize) -> String {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        *counter += 1;
        return format!("call_auto_{}", counter);
    }
    // 只保留字母数字、下划线、连字符、冒号、点
    let clean: String = trimmed
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ':' || *c == '.')
        .collect();
    if clean.is_empty() {
        *counter += 1;
        format!("call_auto_{}", counter)
    } else {
        clean
    }
}

/// 清洗 tool name — trim + 验证格式
pub fn sanitize_tool_name(name: &str, fallback: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return fallback.to_string();
    }
    // 合法字符：字母数字、下划线、连字符、点
    if trimmed
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        trimmed.to_string()
    } else {
        trimmed
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_')
            .collect::<String>()
    }
}

// ────────────────────────────────────────────────────────────────
// 2. 完整清洗管道
// ────────────────────────────────────────────────────────────────

/// 完整清洗消息列表 — 参照 OpenClaw 的 sanitizeSessionHistory
pub fn sanitize_messages_for_llm(messages: &mut Vec<serde_json::Value>, provider: &str) {
    let mut id_counter = 0usize;

    // Step 1: 清洗所有 tool_call ID 和 name
    sanitize_tool_call_ids(messages, &mut id_counter);

    // Step 2: 修复 tool_use/tool_result 配对（重排 + 合成缺失）
    repair_tool_pairing(messages);

    // Step 3: 去重 tool results
    dedup_tool_results(messages);

    // Step 4: 严格 provider 的轮次顺序校验
    if provider == "anthropic" || is_strict_provider(provider) {
        validate_turn_ordering(messages);
    }

    // Step 5: 剥离内部标记字段（LLM API 可能拒绝未知字段）
    for msg in messages.iter_mut() {
        if let Some(obj) = msg.as_object_mut() {
            obj.remove("_internal");
            obj.remove("seq"); // 也清理 DB 序号字段
        }
    }
}

/// 是否为对消息顺序严格的 provider
fn is_strict_provider(provider: &str) -> bool {
    matches!(
        provider,
        "anthropic" | "moonshot" | "kimi" | "deepseek"
    )
}

// ────────────────────────────────────────────────────────────────
// Step 1: 清洗 tool_call ID 和 name
// ────────────────────────────────────────────────────────────────

fn sanitize_tool_call_ids(
    messages: &mut [serde_json::Value],
    counter: &mut usize,
) {
    // 构建 old_id → new_id 映射
    let mut id_map: HashMap<String, String> = HashMap::new();

    for msg in messages.iter_mut() {
        let role = msg["role"].as_str().unwrap_or("");

        if role == "assistant" {
            // OpenAI 格式: tool_calls 数组
            if let Some(calls) = msg.get_mut("tool_calls").and_then(|v| v.as_array_mut()) {
                for tc in calls.iter_mut() {
                    // 清洗 ID
                    let old_id = tc["id"].as_str().unwrap_or("").to_string();
                    let new_id = sanitize_tool_call_id(&old_id, counter);
                    if old_id != new_id {
                        id_map.insert(old_id, new_id.clone());
                    }
                    tc["id"] = serde_json::Value::String(new_id);

                    // 清洗 name
                    if let Some(func) = tc.get_mut("function") {
                        let old_name = func["name"].as_str().unwrap_or("").to_string();
                        let new_name = sanitize_tool_name(&old_name, "unknown_tool");
                        func["name"] = serde_json::Value::String(new_name);
                    }
                }
            }

            // Anthropic 格式: content 数组中的 tool_use 块
            if let Some(blocks) = msg.get_mut("content").and_then(|v| v.as_array_mut()) {
                for block in blocks.iter_mut() {
                    if block["type"].as_str() == Some("tool_use") {
                        let old_id = block["id"].as_str().unwrap_or("").to_string();
                        let new_id = sanitize_tool_call_id(&old_id, counter);
                        if old_id != new_id {
                            id_map.insert(old_id, new_id.clone());
                        }
                        block["id"] = serde_json::Value::String(new_id);

                        let old_name = block["name"].as_str().unwrap_or("").to_string();
                        let new_name = sanitize_tool_name(&old_name, "unknown_tool");
                        block["name"] = serde_json::Value::String(new_name);
                    }
                }
            }
        }
    }

    // 将映射应用到 tool/tool_result 消息
    if !id_map.is_empty() {
        for msg in messages.iter_mut() {
            let role = msg["role"].as_str().unwrap_or("").to_string();

            // OpenAI 格式: role=tool, tool_call_id
            if role == "tool" {
                if let Some(old) = msg["tool_call_id"].as_str().map(|s| s.to_string()) {
                    if let Some(new_id) = id_map.get(&old) {
                        msg["tool_call_id"] = serde_json::Value::String(new_id.clone());
                    }
                }
            }

            // Anthropic 格式: role=user, content 数组中的 tool_result 块
            if role == "user" {
                if let Some(blocks) = msg.get_mut("content").and_then(|v| v.as_array_mut()) {
                    for block in blocks.iter_mut() {
                        if block["type"].as_str() == Some("tool_result") {
                            if let Some(old) =
                                block["tool_use_id"].as_str().map(|s| s.to_string())
                            {
                                if let Some(new_id) = id_map.get(&old) {
                                    block["tool_use_id"] =
                                        serde_json::Value::String(new_id.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
        log::debug!(
            "tool_call_sanitizer: 清洗 {} 个 ID 映射",
            id_map.len()
        );
    }
}

// ────────────────────────────────────────────────────────────────
// Step 2: 配对修复
// ────────────────────────────────────────────────────────────────

/// 从 assistant 消息提取 tool_call ID 列表
fn extract_tool_calls(msg: &serde_json::Value) -> Vec<(String, String)> {
    let mut calls = Vec::new();
    // OpenAI 格式
    if let Some(tcs) = msg["tool_calls"].as_array() {
        for tc in tcs {
            if let Some(id) = tc["id"].as_str() {
                let name = tc["function"]["name"]
                    .as_str()
                    .or_else(|| tc["name"].as_str())
                    .unwrap_or("unknown");
                calls.push((id.to_string(), name.to_string()));
            }
        }
    }
    // Anthropic 格式
    if let Some(blocks) = msg["content"].as_array() {
        for block in blocks {
            if block["type"].as_str() == Some("tool_use") {
                if let Some(id) = block["id"].as_str() {
                    let name = block["name"].as_str().unwrap_or("unknown");
                    calls.push((id.to_string(), name.to_string()));
                }
            }
        }
    }
    calls
}

/// 获取 tool result 消息的 tool_call_id
fn get_tool_result_id(msg: &serde_json::Value) -> Option<String> {
    if msg["role"].as_str() == Some("tool") {
        return msg["tool_call_id"].as_str().map(|s| s.to_string());
    }
    None
}

/// 修复 tool_use/tool_result 配对（参照 OpenClaw 的 repairToolUseResultPairing）
///
/// 重建消息列表：
/// - assistant(tool_calls) 后面紧跟所有对应的 tool results
/// - 合成缺失的 tool results
/// - 丢弃孤儿 tool results（无对应 assistant）
fn repair_tool_pairing(messages: &mut Vec<serde_json::Value>) {
    // 收集所有 tool response 到 map（id → message），保留第一个
    let mut tool_results: HashMap<String, serde_json::Value> = HashMap::new();
    for msg in messages.iter() {
        if let Some(id) = get_tool_result_id(msg) {
            tool_results.entry(id).or_insert_with(|| msg.clone());
        }
    }

    // 重建消息列表
    let mut rebuilt: Vec<serde_json::Value> = Vec::with_capacity(messages.len());
    let mut used_result_ids: HashSet<String> = HashSet::new();
    let mut repaired = 0usize;
    let orphaned;

    for msg in messages.iter() {
        let role = msg["role"].as_str().unwrap_or("");

        // 跳过游离的 tool response（会在对应 assistant 后重新插入）
        if role == "tool" {
            continue;
        }

        rebuilt.push(msg.clone());

        // 如果是带 tool_calls 的 assistant，紧跟插入所有 tool response
        if role == "assistant" {
            let calls = extract_tool_calls(msg);
            if calls.is_empty() {
                continue;
            }

            for (id, name) in &calls {
                if let Some(result) = tool_results.get(id) {
                    rebuilt.push(result.clone());
                    used_result_ids.insert(id.clone());
                } else {
                    // 合成缺失的 tool response
                    rebuilt.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": id,
                        "name": name,
                        "content": "[error: result unavailable]"
                    }));
                    repaired += 1;
                }
            }
        }
    }

    // 统计被丢弃的孤儿 tool results
    orphaned = tool_results.len().saturating_sub(used_result_ids.len());

    if repaired > 0 || orphaned > 0 {
        log::warn!(
            "tool_call_sanitizer: repair_tool_pairing — 合成 {} 个缺失 result, 丢弃 {} 个孤儿 result",
            repaired, orphaned
        );
    }

    *messages = rebuilt;
}

// ────────────────────────────────────────────────────────────────
// Step 3: 去重 tool results
// ────────────────────────────────────────────────────────────────

fn dedup_tool_results(messages: &mut Vec<serde_json::Value>) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut dup_indices: Vec<usize> = Vec::new();

    for (i, msg) in messages.iter().enumerate() {
        if msg["role"].as_str() != Some("tool") {
            continue;
        }
        let tc_id = msg["tool_call_id"].as_str().unwrap_or("").to_string();
        if tc_id.is_empty() {
            continue;
        }
        if !seen.insert(tc_id) {
            dup_indices.push(i);
        }
    }

    if !dup_indices.is_empty() {
        log::debug!(
            "tool_call_sanitizer: 去除 {} 个重复 tool result",
            dup_indices.len()
        );
        for &idx in dup_indices.iter().rev() {
            messages.remove(idx);
        }
    }
}

// ────────────────────────────────────────────────────────────────
// Step 4: 轮次顺序校验
// ────────────────────────────────────────────────────────────────

fn validate_turn_ordering(messages: &mut Vec<serde_json::Value>) {
    if messages.is_empty() {
        return;
    }

    // 确保第一条非 system 消息是 user
    let first_non_system = messages
        .iter()
        .position(|m| m["role"].as_str() != Some("system"));
    if let Some(idx) = first_non_system {
        let role = messages[idx]["role"].as_str().unwrap_or("");
        if role != "user" {
            // 在非 system 起始位置前插入空 user 消息
            log::debug!("tool_call_sanitizer: 在位置 {} 插入空 user 消息（首条非 system 为 {}）", idx, role);
            messages.insert(
                idx,
                serde_json::json!({"role": "user", "content": ""}),
            );
        }
    }

    // 确保 assistant(tool_calls) 后紧跟所有 tool results
    // repair_tool_pairing 已经处理了这个，这里做最终校验
    let mut i = 0;
    while i < messages.len() {
        let role = messages[i]["role"].as_str().unwrap_or("");
        if role == "assistant" {
            let calls = extract_tool_calls(&messages[i]);
            if !calls.is_empty() {
                let expected_count = calls.len();
                let mut actual_count = 0;
                let mut j = i + 1;
                while j < messages.len() && messages[j]["role"].as_str() == Some("tool") {
                    actual_count += 1;
                    j += 1;
                }
                if actual_count < expected_count {
                    log::warn!(
                        "tool_call_sanitizer: validate_turn_ordering — assistant 在位置 {} 有 {} 个 tool_call 但只有 {} 个 tool result",
                        i, expected_count, actual_count
                    );
                    // 补充缺失的（正常不应走到这里，repair 已处理）
                    let responded: HashSet<String> = messages[i + 1..j]
                        .iter()
                        .filter_map(|m| m["tool_call_id"].as_str().map(|s| s.to_string()))
                        .collect();
                    for (id, name) in &calls {
                        if !responded.contains(id) {
                            messages.insert(
                                j,
                                serde_json::json!({
                                    "role": "tool",
                                    "tool_call_id": id,
                                    "name": name,
                                    "content": "[error: result unavailable]"
                                }),
                            );
                            j += 1;
                        }
                    }
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
}

// ────────────────────────────────────────────────────────────────
// 工具结果截断
// ────────────────────────────────────────────────────────────────

/// 工具结果最大字符数（100KB）
pub const MAX_TOOL_RESULT_CHARS: usize = 100_000;

/// 截断过大的工具结果，保留 70% 头部 + 30% 尾部
pub fn truncate_tool_result(result: &str) -> String {
    if result.len() <= MAX_TOOL_RESULT_CHARS {
        return result.to_string();
    }
    let original_len = result.len();
    let head_len = MAX_TOOL_RESULT_CHARS * 7 / 10;
    let tail_len = MAX_TOOL_RESULT_CHARS * 3 / 10;

    // 安全地在 char boundary 上截断
    let head_end = result
        .char_indices()
        .take_while(|(i, _)| *i < head_len)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(head_len.min(result.len()));

    let tail_start_candidate = original_len.saturating_sub(tail_len);
    let tail_start = result[tail_start_candidate..]
        .char_indices()
        .next()
        .map(|(i, _)| tail_start_candidate + i)
        .unwrap_or(tail_start_candidate);

    format!(
        "{}...\n[truncated: original {} chars]\n...{}",
        &result[..head_end],
        original_len,
        &result[tail_start..]
    )
}

// ────────────────────────────────────────────────────────────────
// 流式 tool_call 修复工具
// ────────────────────────────────────────────────────────────────

/// 检测并分割合并的 JSON 对象（如 `{"a":1}{"b":2}`）
pub fn split_merged_json(args: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in args.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 && i + 1 < args.len() {
                    results.push(args[start..=i].to_string());
                    start = i + 1;
                }
            }
            _ => {}
        }
    }
    if start < args.len() {
        results.push(args[start..].to_string());
    }
    // 只有检测到多个 JSON 对象时才返回分割结果
    if results.len() > 1 {
        results
    } else {
        vec![args.to_string()]
    }
}

// ────────────────────────────────────────────────────────────────
// 测试
// ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_tool_call_id_empty() {
        let mut counter = 0;
        assert_eq!(sanitize_tool_call_id("", &mut counter), "call_auto_1");
        assert_eq!(counter, 1);
    }

    #[test]
    fn test_sanitize_tool_call_id_whitespace() {
        let mut counter = 0;
        assert_eq!(sanitize_tool_call_id("  ", &mut counter), "call_auto_1");
    }

    #[test]
    fn test_sanitize_tool_call_id_invalid_chars() {
        let mut counter = 0;
        assert_eq!(
            sanitize_tool_call_id("call#$%123", &mut counter),
            "call123"
        );
        assert_eq!(counter, 0);
    }

    #[test]
    fn test_sanitize_tool_call_id_valid() {
        let mut counter = 0;
        assert_eq!(
            sanitize_tool_call_id("call_abc-123:def.ghi", &mut counter),
            "call_abc-123:def.ghi"
        );
        assert_eq!(counter, 0);
    }

    #[test]
    fn test_sanitize_tool_call_id_all_invalid() {
        let mut counter = 5;
        assert_eq!(sanitize_tool_call_id("###", &mut counter), "call_auto_6");
        assert_eq!(counter, 6);
    }

    #[test]
    fn test_sanitize_tool_name_empty() {
        assert_eq!(sanitize_tool_name("", "fallback"), "fallback");
    }

    #[test]
    fn test_sanitize_tool_name_valid() {
        assert_eq!(sanitize_tool_name("bash_exec", "fb"), "bash_exec");
        assert_eq!(sanitize_tool_name("mcp-tool.v2", "fb"), "mcp-tool.v2");
    }

    #[test]
    fn test_sanitize_tool_name_invalid() {
        assert_eq!(sanitize_tool_name("my tool!", "fb"), "mytool");
    }

    #[test]
    fn test_sanitize_messages_basic() {
        let mut messages = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            serde_json::json!({
                "role": "assistant", "content": "",
                "tool_calls": [{"id": "", "type": "function", "function": {"name": "search", "arguments": "{}"}}]
            }),
            serde_json::json!({"role": "tool", "tool_call_id": "", "name": "search", "content": "found"}),
        ];
        sanitize_messages_for_llm(&mut messages, "openai");
        // 空 ID 应该被自动生成
        let tc_id = messages[1]["tool_calls"][0]["id"].as_str().unwrap();
        assert!(!tc_id.is_empty());
        // tool result 的 ID 应该匹配
        let result_id = messages[2]["tool_call_id"].as_str().unwrap();
        assert_eq!(tc_id, result_id);
    }

    #[test]
    fn test_repair_missing_tool_result() {
        let mut messages = vec![
            serde_json::json!({"role": "user", "content": "q"}),
            serde_json::json!({
                "role": "assistant", "content": "",
                "tool_calls": [
                    {"id": "c1", "type": "function", "function": {"name": "search", "arguments": "{}"}},
                    {"id": "c2", "type": "function", "function": {"name": "http", "arguments": "{}"}}
                ]
            }),
            serde_json::json!({"role": "tool", "tool_call_id": "c1", "name": "search", "content": "found"}),
        ];
        sanitize_messages_for_llm(&mut messages, "openai");
        // c2 的合成 result 应存在
        assert_eq!(messages.len(), 4); // user + assistant + tool(c1) + tool(c2)
        assert_eq!(messages[3]["tool_call_id"].as_str(), Some("c2"));
        assert!(messages[3]["content"]
            .as_str()
            .unwrap()
            .contains("unavailable"));
    }

    #[test]
    fn test_repair_orphan_tool_result() {
        let mut messages = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            serde_json::json!({"role": "tool", "tool_call_id": "orphan_id", "name": "search", "content": "data"}),
            serde_json::json!({"role": "assistant", "content": "done"}),
        ];
        sanitize_messages_for_llm(&mut messages, "openai");
        // 孤儿 tool result 应被丢弃
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"].as_str(), Some("user"));
        assert_eq!(messages[1]["role"].as_str(), Some("assistant"));
    }

    #[test]
    fn test_dedup_tool_results() {
        let mut messages = vec![
            serde_json::json!({"role": "user", "content": "q"}),
            serde_json::json!({
                "role": "assistant", "content": "",
                "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "search", "arguments": "{}"}}]
            }),
            serde_json::json!({"role": "tool", "tool_call_id": "c1", "name": "search", "content": "first"}),
            serde_json::json!({"role": "tool", "tool_call_id": "c1", "name": "search", "content": "duplicate"}),
        ];
        sanitize_messages_for_llm(&mut messages, "openai");
        // 只保留第一个 tool result
        let tool_msgs: Vec<_> = messages
            .iter()
            .filter(|m| m["role"].as_str() == Some("tool"))
            .collect();
        assert_eq!(tool_msgs.len(), 1);
        assert_eq!(tool_msgs[0]["content"].as_str(), Some("first"));
    }

    #[test]
    fn test_validate_turn_ordering_first_message() {
        let mut messages = vec![
            serde_json::json!({"role": "system", "content": "you are helpful"}),
            serde_json::json!({"role": "assistant", "content": "hi"}),
        ];
        sanitize_messages_for_llm(&mut messages, "anthropic");
        // 应该在 system 和 assistant 之间插入 user
        assert_eq!(messages[1]["role"].as_str(), Some("user"));
    }

    #[test]
    fn test_truncate_tool_result_small() {
        let small = "hello world";
        assert_eq!(truncate_tool_result(small), small);
    }

    #[test]
    fn test_truncate_tool_result_large() {
        let large = "a".repeat(200_000);
        let result = truncate_tool_result(&large);
        assert!(result.len() < large.len());
        assert!(result.contains("[truncated: original 200000 chars]"));
    }

    #[test]
    fn test_split_merged_json_single() {
        let input = r#"{"url":"https://example.com"}"#;
        let parts = split_merged_json(input);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], input);
    }

    #[test]
    fn test_split_merged_json_double() {
        let input = r#"{"url":"a"}{"path":"b"}"#;
        let parts = split_merged_json(input);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], r#"{"url":"a"}"#);
        assert_eq!(parts[1], r#"{"path":"b"}"#);
    }

    #[test]
    fn test_split_merged_json_nested() {
        let input = r#"{"a":{"b":1}}{"c":2}"#;
        let parts = split_merged_json(input);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], r#"{"a":{"b":1}}"#);
        assert_eq!(parts[1], r#"{"c":2}"#);
    }

    #[test]
    fn test_misplaced_tool_result_reordered() {
        let mut messages = vec![
            serde_json::json!({"role": "user", "content": "q1"}),
            serde_json::json!({
                "role": "assistant", "content": "",
                "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "search", "arguments": "{}"}}]
            }),
            serde_json::json!({"role": "user", "content": "q2"}),
            serde_json::json!({"role": "tool", "tool_call_id": "c1", "name": "search", "content": "found"}),
        ];
        sanitize_messages_for_llm(&mut messages, "openai");
        // tool result 应紧跟在 assistant 之后
        assert_eq!(messages[2]["role"].as_str(), Some("tool"));
        assert_eq!(messages[2]["tool_call_id"].as_str(), Some("c1"));
    }
}
