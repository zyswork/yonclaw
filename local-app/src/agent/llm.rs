//! LLM 调用模块
//!
//! 支持 OpenAI 和 Anthropic API，包含流式输出和工具调用

use super::tools::{ParsedToolCall, ToolDefinition};
use futures::StreamExt;
use log;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

/// 构建 Anthropic API URL
/// - 无 base_url → 官方 https://api.anthropic.com/v1/messages
/// - base_url 含 /v1 → 拼 /messages（如 https://api.anthropic.com/v1）
/// - base_url 是第三方完整路径 → 直接用，不拼后缀（如 https://api.aicodewith.com）
fn build_anthropic_url(base_url: Option<&str>) -> String {
    match base_url {
        None => "https://api.anthropic.com/v1/messages".to_string(),
        Some(url) => {
            let url = url.trim_end_matches('/');
            if url.ends_with("/v1") {
                format!("{}/messages", url)
            } else if url.contains("/v1/") || url.ends_with("/messages") {
                url.to_string()
            } else {
                // 第三方代理：直接用完整 URL，不拼后缀
                url.to_string()
            }
        }
    }
}

/// 将 OpenAI 格式的 tool 消息转为 Anthropic 兼容格式
/// - role: "tool" → role: "user" + tool_result content block
/// - role: "system" → 移除（Anthropic 用顶层 system 字段）
/// 清理 Anthropic 消息：确保 tool_use/tool_result 配对，格式正确
/// 参考 OpenClaw 的 transformMessages + convertMessages
/// current_provider/current_model: 当前请求使用的 provider 和 model（用于模型感知转换）
fn sanitize_messages_for_anthropic(messages: &[serde_json::Value], current_provider: &str, current_model: &str) -> Vec<serde_json::Value> {
    let mut result = Vec::with_capacity(messages.len());

    // 预构建 tool ID 映射（确保全局一致性，参考 OpenClaw transformMessages）
    let mut tool_id_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for msg in messages {
        if let Some(arr) = msg["content"].as_array() {
            for block in arr {
                if block["type"].as_str() == Some("tool_use") {
                    if let Some(raw_id) = block["id"].as_str() {
                        let clean = sanitize_tool_id(raw_id);
                        if clean != raw_id { tool_id_map.insert(raw_id.to_string(), clean); }
                    }
                }
            }
        }
        if let Some(calls) = msg["tool_calls"].as_array() {
            for tc in calls {
                if let Some(raw_id) = tc["id"].as_str() {
                    let clean = sanitize_tool_id(raw_id);
                    if clean != raw_id { tool_id_map.insert(raw_id.to_string(), clean); }
                }
            }
        }
    }

    // 第一步：转换消息格式
    for msg in messages {
        let role = msg["role"].as_str().unwrap_or("");

        // 跳过空 content 的 assistant 消息（null / 空字符串）
        if role == "assistant" {
            if msg["content"].is_null() || (msg["content"].as_str() == Some("")) {
                log::debug!("跳过空 content 的 assistant 消息");
                continue;
            }
            if let Some(stop) = msg["stop_reason"].as_str() {
                if stop == "error" || stop == "aborted" {
                    log::debug!("跳过 error/aborted assistant 消息");
                    continue;
                }
            }
        }

        match role {
            "system" => continue,
            "tool" => {
                // OpenAI tool result → Anthropic user + tool_result
                let raw_id = msg["tool_call_id"].as_str().unwrap_or("unknown");
                let id = tool_id_map.get(raw_id).cloned().unwrap_or_else(|| sanitize_tool_id(raw_id));
                let content = msg["content"].as_str().unwrap_or("");
                result.push(serde_json::json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": id,
                        "content": content
                    }]
                }));
            }
            "assistant" => {
                if let Some(arr) = msg["content"].as_array() {
                    // 已是 Anthropic 数组格式
                    let blocks: Vec<serde_json::Value> = arr.iter().filter_map(|b| {
                        match b["type"].as_str() {
                            Some("text") => {
                                let text = b["text"].as_str().unwrap_or("");
                                if text.is_empty() { None } else { Some(b.clone()) }
                            }
                            Some("tool_use") => {
                                let mut block = b.clone();
                                if let Some(id) = b["id"].as_str() {
                                    let clean = tool_id_map.get(id).cloned().unwrap_or_else(|| sanitize_tool_id(id));
                                    block["id"] = serde_json::Value::String(clean);
                                }
                                Some(block)
                            }
                            Some("thinking") => {
                                // 模型感知转换（参考 OpenClaw transformMessages:30-43）
                                let msg_provider = msg.get("provider").and_then(|v| v.as_str()).unwrap_or("");
                                let msg_model = msg.get("model").and_then(|v| v.as_str()).unwrap_or("");
                                let is_same_model = msg_provider == current_provider && msg_model == current_model;

                                if is_same_model {
                                    // 同模型：保留带签名的 thinking（可 replay）
                                    let has_signature = b.get("signature").and_then(|s| s.as_str()).map_or(false, |s| !s.is_empty());
                                    if has_signature {
                                        Some(b.clone())
                                    } else {
                                        // 无签名的 thinking：转为文本或跳过
                                        let thinking_text = b["thinking"].as_str().unwrap_or("");
                                        if thinking_text.is_empty() { None }
                                        else { Some(serde_json::json!({"type": "text", "text": thinking_text})) }
                                    }
                                } else {
                                    // 异模型：转为文本（不能跨模型 replay thinking 签名）
                                    let thinking_text = b["thinking"].as_str().unwrap_or("");
                                    if thinking_text.is_empty() { None }
                                    else { Some(serde_json::json!({"type": "text", "text": thinking_text})) }
                                }
                            }
                            _ => Some(b.clone()),
                        }
                    }).collect();
                    if !blocks.is_empty() {
                        result.push(serde_json::json!({"role": "assistant", "content": blocks}));
                    }
                } else if let Some(tool_calls) = msg["tool_calls"].as_array() {
                    // OpenAI tool_calls → Anthropic content 数组
                    let mut blocks = Vec::new();
                    if let Some(text) = msg["content"].as_str() {
                        if !text.is_empty() {
                            blocks.push(serde_json::json!({"type": "text", "text": text}));
                        }
                    }
                    for tc in tool_calls {
                        let name = tc["function"]["name"].as_str().unwrap_or("");
                        let id = sanitize_tool_id(tc["id"].as_str().unwrap_or("unknown"));
                        let input: serde_json::Value = if let Some(obj) = tc["function"]["arguments"].as_object() {
                            serde_json::Value::Object(obj.clone())
                        } else {
                            tc["function"]["arguments"].as_str()
                                .and_then(|s| serde_json::from_str(s).ok())
                                .unwrap_or(serde_json::json!({}))
                        };
                        blocks.push(serde_json::json!({
                            "type": "tool_use", "id": id, "name": name, "input": input
                        }));
                    }
                    if !blocks.is_empty() {
                        result.push(serde_json::json!({"role": "assistant", "content": blocks}));
                    }
                } else {
                    // 纯文本 assistant — 转为 Anthropic 数组格式
                    if let Some(text) = msg["content"].as_str() {
                        if !text.is_empty() {
                            result.push(serde_json::json!({
                                "role": "assistant",
                                "content": [{"type": "text", "text": text}]
                            }));
                        }
                    } else {
                        result.push(msg.clone());
                    }
                }
            }
            _ => {
                // user 消息（无论内容是数组还是字符串，直接透传）
                result.push(msg.clone());
            }
        }
    }

    // 第二步：去掉重复/空的 tool_result
    deduplicate_tool_results(&mut result);

    // 第三步：确保每个 tool_use 都有对应的 tool_result
    ensure_tool_use_result_pairing(&mut result);

    // 第四步：合并连续同 role 消息
    merge_consecutive_roles(&mut result);

    // 第四步：确保第一条消息是 user（Anthropic 要求）
    if let Some(first) = result.first() {
        if first["role"].as_str() != Some("user") {
            result.insert(0, serde_json::json!({"role": "user", "content": [{"type": "text", "text": "Continue."}]}));
        }
    }

    // 第五步：确保所有消息的 content 都是数组格式（部分严格代理要求）
    for msg in result.iter_mut() {
        if let Some(text) = msg["content"].as_str().map(|s| s.to_string()) {
            msg["content"] = serde_json::json!([{"type": "text", "text": text}]);
        }
    }

    result
}

/// 去掉重复和空的 tool_result（同一 tool_use_id 只保留第一个有内容的）
fn deduplicate_tool_results(messages: &mut Vec<serde_json::Value>) {
    for msg in messages.iter_mut() {
        if let Some(arr) = msg["content"].as_array_mut() {
            let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
            arr.retain(|block| {
                if block["type"].as_str() == Some("tool_result") {
                    let tid = block["tool_use_id"].as_str().unwrap_or("").to_string();
                    let content = block["content"].as_str().unwrap_or("");
                    // 跳过空内容或 [context compacted]
                    if content.is_empty() || content == "[context compacted]" {
                        // 如果还没见过这个 id，保留（但标记为已见）
                        if !seen_ids.contains(&tid) {
                            // 不保留空的，等下面有内容的来
                            return false;
                        }
                        return false;
                    }
                    // 有内容的：去重
                    if !seen_ids.insert(tid) { return false; }
                }
                true
            });
            // 如果去重后数组为空，需要特殊处理
        }
    }
    // 移除 content 数组为空的消息
    messages.retain(|msg| {
        if let Some(arr) = msg["content"].as_array() {
            !arr.is_empty()
        } else {
            true
        }
    });
}

/// 确保每个 tool_use 都有对应的 tool_result
/// 参考 OpenClaw: transformMessages 行 78-147
fn ensure_tool_use_result_pairing(messages: &mut Vec<serde_json::Value>) {
    let mut i = 0;
    while i < messages.len() {
        if messages[i]["role"].as_str() == Some("assistant") {
            // 收集这条 assistant 消息里的所有 tool_use id
            let tool_use_ids: Vec<String> = messages[i]["content"].as_array()
                .map(|arr| arr.iter()
                    .filter(|b| b["type"].as_str() == Some("tool_use"))
                    .filter_map(|b| b["id"].as_str().map(|s| s.to_string()))
                    .collect()
                ).unwrap_or_default();

            if !tool_use_ids.is_empty() {
                // 检查后续消息是否有对应的 tool_result
                let mut found_results: std::collections::HashSet<String> = std::collections::HashSet::new();
                let mut j = i + 1;
                while j < messages.len() {
                    let next_role = messages[j]["role"].as_str().unwrap_or("");
                    if next_role == "assistant" { break; } // 下一个 assistant 消息，停止搜索
                    if let Some(arr) = messages[j]["content"].as_array() {
                        for block in arr {
                            if block["type"].as_str() == Some("tool_result") {
                                if let Some(tid) = block["tool_use_id"].as_str() {
                                    found_results.insert(tid.to_string());
                                }
                            }
                        }
                    }
                    j += 1;
                }

                // 为缺失的 tool_result 插入合成结果
                let missing: Vec<String> = tool_use_ids.into_iter()
                    .filter(|id| !found_results.contains(id))
                    .collect();

                if !missing.is_empty() {
                    let synthetic_blocks: Vec<serde_json::Value> = missing.iter().map(|id| {
                        serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": id,
                            "content": "No result provided",
                            "is_error": true
                        })
                    }).collect();

                    // 在 assistant 消息后面插入合成的 user+tool_result
                    let insert_pos = i + 1;
                    messages.insert(insert_pos, serde_json::json!({
                        "role": "user",
                        "content": synthetic_blocks
                    }));
                    log::info!("Anthropic: 插入 {} 个合成 tool_result（孤儿 tool_use 修复）", missing.len());
                }
            }
        }
        i += 1;
    }
}

/// 清理 tool ID（Anthropic 要求 ^[a-zA-Z0-9_-]{1,64}$）
fn sanitize_tool_id(id: &str) -> String {
    let clean: String = id.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .take(64)
        .collect();
    if clean.is_empty() { "unknown".to_string() } else { clean }
}

/// 清理消息给 OpenAI 用：
/// - 保留原生 OpenAI 格式的 tool/tool_calls 消息
/// - Anthropic 格式的 tool_use/tool_result → 提取文本，剥离工具调用
/// - 确保 tool role 消息有对应的 assistant tool_calls
fn sanitize_messages_for_openai(messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut result = Vec::new();

    for msg in messages {
        let role = msg["role"].as_str().unwrap_or("");

        // 跳过 OpenAI tool role 如果对应的 assistant tool_calls 不存在
        // （可能是从 Anthropic 会话带过来的）
        if role == "tool" {
            // 检查是否有对应的 assistant tool_calls（往前找）
            let call_id = msg["tool_call_id"].as_str().unwrap_or("");
            let has_matching_call = result.iter().rev().any(|prev: &serde_json::Value| {
                if let Some(calls) = prev["tool_calls"].as_array() {
                    calls.iter().any(|tc: &serde_json::Value| tc["id"].as_str() == Some(call_id))
                } else { false }
            });
            if !has_matching_call {
                // 没有匹配的 tool_call，转为普通 user 消息
                let content = msg["content"].as_str().unwrap_or("");
                if !content.is_empty() && content != "[context compacted]" {
                    result.push(serde_json::json!({"role": "user", "content": content}));
                }
                continue;
            }
            result.push(msg.clone());
            continue;
        }

        // assistant 消息
        if role == "assistant" {
            // Anthropic 数组 content（含 tool_use）→ 提取纯文本
            if let Some(arr) = msg["content"].as_array() {
                let has_tool_use = arr.iter().any(|b| b["type"].as_str() == Some("tool_use"));
                if has_tool_use {
                    // 只提取文本部分，丢弃 tool_use
                    let text: String = arr.iter().filter_map(|b| {
                        if b["type"].as_str() == Some("text") { b["text"].as_str().map(|s| s.to_string()) }
                        else { None }
                    }).collect::<Vec<_>>().join("\n");
                    if !text.is_empty() {
                        result.push(serde_json::json!({"role": "assistant", "content": text}));
                    }
                    continue;
                }
                // 纯文本数组 → 拼字符串
                let text: String = arr.iter()
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>().join("\n");
                if !text.is_empty() {
                    result.push(serde_json::json!({"role": "assistant", "content": text}));
                }
                continue;
            }
            // 有 tool_calls 的 assistant 消息 → 保留（OpenAI 原生格式）
            let mut m = msg.clone();
            // 确保 content 不为 null（Kimi 等不接受空 assistant 消息）
            if m["content"].is_null() || (m["content"].as_str().map_or(false, |s| s.is_empty()) && m.get("tool_calls").is_none()) {
                // 无 tool_calls 且 content 为空 → 跳过
                if m.get("tool_calls").is_none() { continue; }
                // 有 tool_calls 但 content 为 null → 设为空字符串
                m["content"] = serde_json::json!("");
            }
            result.push(m);
            continue;
        }

        // user 消息
        if let Some(arr) = msg["content"].as_array() {
            // Anthropic 格式 tool_result 数组 → 跳过（对应的 tool_use 已被剥离）
            if arr.iter().any(|b| b["type"].as_str() == Some("tool_result")) {
                continue;
            }
            // 其他数组 → 提取文本
            let text: String = arr.iter()
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>().join("\n");
            if !text.is_empty() {
                result.push(serde_json::json!({"role": "user", "content": text}));
            }
            continue;
        }

        // 纯字符串消息 → 直接保留
        let content = msg["content"].as_str().unwrap_or("");
        if !content.is_empty() {
            result.push(msg.clone());
        }
    }

    result
}

/// 合并连续同 role 的消息（Anthropic 要求 user/assistant 严格交替）
fn merge_consecutive_roles(messages: &mut Vec<serde_json::Value>) {
    if messages.len() < 2 { return; }
    let mut i = 0;
    while i + 1 < messages.len() {
        let role_a = messages[i]["role"].as_str().unwrap_or("").to_string();
        let role_b = messages[i + 1]["role"].as_str().unwrap_or("").to_string();
        if role_a == role_b {
            let content_b = messages[i + 1]["content"].clone();
            let content_a = &mut messages[i]["content"];
            // 两个都是数组：合并
            if let (Some(arr_a), Some(arr_b)) = (content_a.as_array_mut(), content_b.as_array()) {
                arr_a.extend(arr_b.iter().cloned());
            }
            // 数组 + 字符串
            else if let Some(arr_a) = content_a.as_array_mut() {
                if let Some(text) = content_b.as_str() {
                    if !text.is_empty() {
                        arr_a.push(serde_json::json!({"type": "text", "text": text}));
                    }
                }
            }
            // 两个都是字符串
            else if let (Some(text_a), Some(text_b)) = (content_a.as_str().map(|s| s.to_string()), content_b.as_str()) {
                *content_a = serde_json::Value::String(format!("{}\n{}", text_a, text_b));
            }
            // 字符串 + 数组：转为数组
            else if let Some(text_a) = content_a.as_str().map(|s| s.to_string()) {
                if let Some(arr_b) = content_b.as_array() {
                    let mut new_arr = vec![serde_json::json!({"type": "text", "text": text_a})];
                    new_arr.extend(arr_b.iter().cloned());
                    *content_a = serde_json::Value::Array(new_arr);
                }
            }
            messages.remove(i + 1);
        } else {
            i += 1;
        }
    }
}
/// LLM 提供商
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProvider {
    OpenAI,
    Anthropic,
}

/// Thinking 级别（扩展推理）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ThinkingLevel {
    Off,
    Minimal,  // budget: 1024
    Low,      // budget: 2048
    Medium,   // budget: 8192
    High,     // budget: 16384
}

impl ThinkingLevel {
    pub fn budget_tokens(&self) -> Option<i32> {
        match self {
            Self::Off => None,
            Self::Minimal => Some(1024),
            Self::Low => Some(2048),
            Self::Medium => Some(8192),
            Self::High => Some(16384),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "minimal" => Self::Minimal,
            "low" => Self::Low,
            "medium" => Self::Medium,
            "high" => Self::High,
            _ => Self::Off,
        }
    }

    pub fn is_enabled(&self) -> bool { *self != Self::Off }
}

/// LLM 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i32>,
    /// 扩展推理级别（Anthropic thinking / OpenAI reasoning）
    #[serde(default)]
    pub thinking_level: Option<ThinkingLevel>,
}

impl LlmConfig {
    pub fn openai(api_key: String, model: String) -> Self {
        Self {
            provider: "openai".to_string(),
            api_key, model,
            base_url: Some("https://api.openai.com/v1".to_string()),
            temperature: None, max_tokens: None,
            thinking_level: None,
        }
    }
    pub fn anthropic(api_key: String, model: String) -> Self {
        Self {
            provider: "anthropic".to_string(),
            api_key, model,
            base_url: Some("https://api.anthropic.com/v1".to_string()),
            temperature: None, max_tokens: None,
            thinking_level: None,
        }
    }
}

/// LLM 流式响应结果
#[derive(Debug, Clone, Default)]
pub struct LlmResponse {
    pub content: String,
    pub tool_calls: Vec<ParsedToolCall>,
    pub stop_reason: String,
    /// API 返回的 token 使用统计
    pub usage: Option<LlmUsage>,
    /// Anthropic extended thinking 内容
    pub thinking_content: String,
}

/// Token 使用统计
#[derive(Debug, Clone, Default)]
pub struct LlmUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    /// Anthropic Prompt Cache 读取的 token 数（按 0.1x 计费）
    pub cache_read_tokens: u64,
    /// Anthropic Prompt Cache 写入的 token 数（按 1.25x 计费）
    pub cache_creation_tokens: u64,
}

impl LlmResponse {
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

/// 根据 provider + model 推断合理的 max_tokens 默认值
fn default_max_tokens(provider: &str, model: &str) -> i32 {
    // 按模型前缀匹配
    let m = model.to_lowercase();
    if m.contains("claude") { return 4096; }
    if m.starts_with("qwen-plus") || m.starts_with("qwen-long") { return 16384; }
    if m.starts_with("qwen") { return 8192; }
    if m.starts_with("minimax") || m.starts_with("abab") { return 16384; }
    if m.starts_with("deepseek") { return 8192; }
    if m.starts_with("glm") { return 16384; }
    if m.starts_with("gpt-4o") || m.starts_with("gpt-4-turbo") || m.starts_with("gpt-5") { return 16384; }
    if m.starts_with("gpt-4") { return 8192; }
    if m.starts_with("gpt-3.5") { return 4096; }
    // 按 provider 兜底
    match provider {
        "anthropic" => 4096,
        "qwen" => 8192,
        "minimax" => 16384,
        _ => 4096, // 保守默认
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiMessage { pub role: String, pub content: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiRequest {
    pub model: String, pub messages: Vec<OpenAiMessage>,
    pub temperature: f64, pub max_tokens: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiResponse { pub choices: Vec<OpenAiChoice> }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiChoice { pub message: OpenAiMessage, pub finish_reason: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessage { pub role: String, pub content: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicRequest {
    pub model: String, pub messages: Vec<AnthropicMessage>,
    pub system: String, pub temperature: f64, pub max_tokens: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicResponse { pub content: Vec<AnthropicContent>, pub stop_reason: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicContent { pub r#type: String, pub text: Option<String> }

struct OaToolCallAccum { id: String, name: String, arguments: String }
struct AnthToolAccum { id: String, name: String, input_json: String }

pub struct LlmClient {
    config: LlmConfig,
    client: reqwest::Client,
}
impl LlmClient {
    pub fn new(config: LlmConfig) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(120))
            .pool_max_idle_per_host(0) // 不复用连接，避免被上一个卡住的连接阻塞
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { config, client }
    }

    pub async fn call_openai(&self, messages: Vec<OpenAiMessage>, temperature: f64, max_tokens: i32) -> Result<String, Box<dyn std::error::Error>> {
        let url = format!("{}/chat/completions", self.config.base_url.as_deref().unwrap_or("https://api.openai.com/v1"));
        let request = OpenAiRequest { model: self.config.model.clone(), messages, temperature, max_tokens };
        let response = self.client.post(&url).header("Authorization", format!("Bearer {}", self.config.api_key)).json(&request).send().await?;
        let data: OpenAiResponse = response.json().await?;
        data.choices.first().map(|c| c.message.content.clone()).ok_or("OpenAI 响应为空".into())
    }

    pub async fn call_anthropic(&self, messages: Vec<AnthropicMessage>, system: String, temperature: f64, max_tokens: i32) -> Result<String, Box<dyn std::error::Error>> {
        let url = build_anthropic_url(self.config.base_url.as_deref());
        let request = AnthropicRequest { model: self.config.model.clone(), messages, system, temperature, max_tokens };
        let response = self.client.post(&url).header("x-api-key", &self.config.api_key).header("anthropic-version", "2023-06-01").json(&request).send().await?;
        let data: AnthropicResponse = response.json().await?;
        data.content.first().and_then(|c| c.text.clone()).ok_or("Anthropic 响应为空".into())
    }

    pub async fn call(&self, messages: Vec<(String, String)>, system_prompt: String, temperature: f64, max_tokens: i32) -> Result<String, Box<dyn std::error::Error>> {
        match self.config.provider.as_str() {
            "openai" => {
                let msgs = messages.into_iter().map(|(role, content)| OpenAiMessage { role, content }).collect();
                self.call_openai(msgs, temperature, max_tokens).await
            }
            "anthropic" => {
                let msgs = messages.into_iter().map(|(role, content)| AnthropicMessage { role, content }).collect();
                self.call_anthropic(msgs, system_prompt, temperature, max_tokens).await
            }
            _ => Err(format!("不支持的 LLM 提供商: {}", self.config.provider).into()),
        }
    }

    /// 构建 Anthropic system blocks，支持多断点 Prompt Cache
    ///
    /// Anthropic 的 prompt cache 按前缀匹配：
    /// - 稳定部分（Identity/Soul/Safety/Tools）放在前面，标记 cache_control
    /// - 动态部分（Memory/DateTime/摘要）放在后面
    /// - 最多标记 4 个 cache 断点（Anthropic 限制）
    fn build_anthropic_system_blocks(system_prompt: &str) -> serde_json::Value {
        let sections: Vec<&str> = system_prompt.split("\n\n---\n\n").filter(|s| !s.trim().is_empty()).collect();
        if sections.is_empty() { return serde_json::json!([]); }

        // 标记策略：稳定前缀的末尾 + 最后一个 block
        // SoulEngine 的 section 顺序：Identity(0) Soul(1) Safety(2) Tools(3) Memory(4) User(5) DateTime(6) ...
        // 前 4 个是稳定的（不随对话变化），适合缓存
        let stable_boundary = sections.len().min(4); // 最多前 4 个 section 视为稳定
        let last_idx = sections.len() - 1;

        let mut blocks = Vec::new();
        let mut cache_points_used = 0;
        const MAX_CACHE_POINTS: usize = 4; // Anthropic 最多 4 个 cache breakpoints

        for (i, section) in sections.iter().enumerate() {
            let should_cache =
                // 稳定前缀末尾（促进跨请求 cache 命中）
                (i + 1 == stable_boundary && cache_points_used < MAX_CACHE_POINTS)
                // 最后一个 block（整体 cache）
                || (i == last_idx && cache_points_used < MAX_CACHE_POINTS);

            if should_cache {
                blocks.push(serde_json::json!({"type": "text", "text": *section, "cache_control": {"type": "ephemeral"}}));
                cache_points_used += 1;
            } else {
                blocks.push(serde_json::json!({"type": "text", "text": *section}));
            }
        }
        serde_json::Value::Array(blocks)
    }

    fn build_openai_tools(tools: &[ToolDefinition]) -> serde_json::Value {
        serde_json::Value::Array(tools.iter().map(|t| serde_json::json!({
            "type": "function",
            "function": {"name": t.name, "description": t.description, "parameters": t.parameters}
        })).collect())
    }

    fn build_anthropic_tools(tools: &[ToolDefinition]) -> serde_json::Value {
        serde_json::Value::Array(tools.iter().map(|t| serde_json::json!({
            "name": t.name, "description": t.description, "input_schema": t.parameters
        })).collect())
    }
    /// 流式调用 LLM，通过 channel 逐 token 返回，支持工具调用
    pub async fn call_stream(
        &self,
        messages: &[serde_json::Value],
        system_prompt: Option<&str>,
        tools: Option<&[ToolDefinition]>,
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<LlmResponse, String> {
        let config = &self.config;
        // 从限定引用 "provider/model" 中提取纯模型名（API 请求只用模型名）
        let (_, pure_model) = crate::channels::parse_qualified_model(&config.model);
        let (url, body) = match config.provider.as_str() {
            "openai" => {
                let url = format!("{}/chat/completions", config.base_url.as_deref().unwrap_or("https://api.openai.com/v1"));
                let resolved_max_tokens = config.max_tokens.filter(|&t| t > 0).unwrap_or_else(|| default_max_tokens(&config.provider, &config.model));
                // 清理历史中可能混入的 Anthropic 格式消息
                let clean_messages = sanitize_messages_for_openai(messages);
                let mut body = serde_json::json!({
                    "model": pure_model, "messages": clean_messages, "stream": true,
                    "temperature": config.temperature.unwrap_or(0.7),
                    "max_tokens": resolved_max_tokens,
                    "stream_options": {"include_usage": true},
                });
                if let Some(t) = tools { if !t.is_empty() {
                    body["tools"] = Self::build_openai_tools(t);
                    body["tool_choice"] = serde_json::json!("auto");
                    body["parallel_tool_calls"] = serde_json::json!(true);
                } }
                // 特定模型兼容性处理
                let ml = pure_model.to_lowercase();
                if ml.contains("kimi-k2") {
                    // kimi-k2.5 不是推理模型，显式关闭 thinking（参考 OpenClaw）
                    // kimi-k2-thinking / kimi-k2-thinking-turbo 才需要启用
                    if !ml.contains("thinking") {
                        // thinking 禁用时 temperature 必须为 0.6
                        body["temperature"] = serde_json::json!(0.6);
                        body["thinking"] = serde_json::json!({"type": "disabled"});
                    } else {
                        // thinking 启用时 temperature 必须为 1
                        body["temperature"] = serde_json::json!(1);
                        // thinking 模型：为历史 assistant 消息补充空 reasoning_content
                        body["thinking"] = serde_json::json!({"type": "enabled"});
                        if let Some(msgs) = body["messages"].as_array_mut() {
                            for m in msgs.iter_mut() {
                                if m["role"].as_str() == Some("assistant") && m.get("reasoning_content").is_none() {
                                    m["reasoning_content"] = serde_json::json!("");
                                }
                            }
                        }
                    }
                    // Kimi thinking 模式下不支持 required/pinned tool_choice
                    if ml.contains("thinking") {
                        if let Some(tc) = body.get("tool_choice") {
                            if tc.as_str() == Some("required") {
                                body["tool_choice"] = serde_json::json!("auto");
                            }
                        }
                    }
                    // 移除 Kimi 不支持的参数
                    body.as_object_mut().map(|o| {
                        o.remove("parallel_tool_calls");
                        o.remove("stream_options");
                    });
                } else if ml.starts_with("glm") || ml.contains("glm-") {
                    // Z.AI GLM 系列：启用 tool_stream 支持实时工具调用流式传输（参考 OpenClaw zai-stream-wrappers.ts）
                    body["tool_stream"] = serde_json::json!(true);
                    // GLM 推理模型（glm-z1、glm-4.7、glm-5 等）处理
                    // Z.AI 不支持 parallel_tool_calls
                    body.as_object_mut().map(|o| { o.remove("parallel_tool_calls"); });
                } else if ml.contains("o1") || ml.contains("o3") || ml.contains("o4") {
                    // OpenAI o 系列: temperature 必须为 1
                    body["temperature"] = serde_json::json!(1);
                }
                (url, body)
            }
            "anthropic" => {
                let url = build_anthropic_url(config.base_url.as_deref());
                let resolved_max_tokens = config.max_tokens.filter(|&t| t > 0).unwrap_or_else(|| default_max_tokens(&config.provider, &config.model));
                // 转换 OpenAI 格式的 tool 消息为 Anthropic 格式
                let clean_messages = sanitize_messages_for_anthropic(messages, &config.provider, &config.model);
                log::info!("sanitize 完成: {} 条消息", clean_messages.len());
                // 调试：打印每条消息的结构
                for (i, m) in clean_messages.iter().enumerate() {
                    let role = m["role"].as_str().unwrap_or("?");
                    let content_type = if m["content"].is_array() { "array" } else if m["content"].is_string() { "STRING(!)" } else { "other(!)" };
                    let has_tool_calls = m.get("tool_calls").is_some();
                    let preview: String = m["content"].to_string().chars().take(80).collect();
                    log::info!("Anthropic msg[{}]: role={}, content={}, tool_calls={}, preview={}", i, role, content_type, has_tool_calls, preview);
                }
                let mut body = serde_json::json!({
                    "model": pure_model, "messages": clean_messages, "stream": true,
                    "temperature": config.temperature.unwrap_or(1.0),
                    "max_tokens": resolved_max_tokens,
                });
                log::info!("body 构建完成，添加 system+tools...");
                if let Some(sp) = system_prompt { body["system"] = Self::build_anthropic_system_blocks(sp); }
                if let Some(t) = tools { if !t.is_empty() { body["tools"] = Self::build_anthropic_tools(t); } }
                // Thinking（扩展推理）支持
                if let Some(ref level) = config.thinking_level {
                    if level.is_enabled() {
                        if let Some(budget) = level.budget_tokens() {
                            body["thinking"] = serde_json::json!({"type": "enabled", "budget_tokens": budget});
                            // thinking 模式下 temperature 必须为 1（Anthropic 要求）
                            body["temperature"] = serde_json::json!(1);
                            log::info!("Anthropic thinking 已启用: level={:?}, budget={}", level, budget);
                        }
                    }
                }
                (url, body)
            }
            _ => return Err("不支持的 LLM 提供商".to_string()),
        };

        // 记录请求信息
        {
            let msg_count = messages.len();
            let _has_tools = tools.map_or(false, |t| !t.is_empty());
            let tool_count = tools.map_or(0, |t| t.len());
            let body_size = body.to_string().len();
            // 检查是否包含工具结果消息
            let tool_result_count = messages.iter().filter(|m| {
                m["role"].as_str() == Some("tool") || // OpenAI 格式
                m.get("content").and_then(|c| c.as_array()).map_or(false, |arr| {
                    arr.iter().any(|b| b["type"].as_str() == Some("tool_result")) // Anthropic 格式
                })
            }).count();
            log::info!(
                "LLM 请求: provider={}, model={}, messages={}, tools={}, tool_results={}, body_size={}KB, url={}",
                config.provider, config.model, msg_count, tool_count, tool_result_count, body_size / 1024, url
            );
            // 如果包含工具结果，记录最后几条消息的角色和摘要
            if tool_result_count > 0 {
                for (i, msg) in messages.iter().enumerate().rev().take(4) {
                    let role = msg["role"].as_str().unwrap_or("?");
                    let content_preview = msg["content"].as_str()
                        .map(|s| s.chars().take(100).collect::<String>())
                        .unwrap_or_else(|| "[non-string]".to_string());
                    log::debug!("  msg[{}] role={} content_preview={}", i, role, content_preview);
                }
            }
        }

        // 带重试的 HTTP 请求 + SSE 流解析
        const MAX_RETRIES: usize = 3;
        let mut result = LlmResponse::default();
        let mut oa_tool_calls: Vec<OaToolCallAccum> = Vec::new();
        let mut anth_current_tool: Option<AnthToolAccum> = None;
        // think 标签过滤状态
        let mut in_think = false;
        let mut think_buffer = String::new();

        for attempt in 0..MAX_RETRIES {
            let mut req = self.client.post(&url).json(&body);
            match config.provider.as_str() {
                "openai" => {
                    log::info!("LLM auth: Bearer key_len={}, prefix={}", config.api_key.len(), &config.api_key[..config.api_key.len().min(15)]);
                    req = req.header("Authorization", format!("Bearer {}", config.api_key));
                }
                "anthropic" => {
                    req = req.header("x-api-key", &config.api_key);
                    req = req.header("anthropic-version", "2023-06-01");
                    req = req.header("anthropic-beta", "prompt-caching-2024-07-31,fine-grained-tool-streaming-2025-05-14,interleaved-thinking-2025-05-14");
                }
                _ => {}
            }

            let response = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    if attempt < MAX_RETRIES - 1 {
                        let wait = (attempt + 1) as u64;
                        log::warn!("LLM 请求失败（第 {} 次），{}秒后重试: {}", attempt + 1, wait, e);
                        tokio::time::sleep(Duration::from_secs(wait)).await;
                        // 重置累积状态
                        result = LlmResponse::default();
                        oa_tool_calls.clear();
                        anth_current_tool = None;
                        in_think = false;
                        think_buffer.clear();
                        continue;
                    }
                    log::error!("LLM HTTP 请求最终失败（{} 次尝试）: {}", MAX_RETRIES, e);
                    let raw = format!("LLM 连接失败（重试 {} 次后）: {}", MAX_RETRIES, e);
                    return Err(classify_llm_error(&raw));
                }
            };
            if !response.status().is_success() {
                let status = response.status();
                let body_text = response.text().await.unwrap_or_default();
                // 安全: 清洗错误响应中可能泄露的凭据
                let safe_body = body_text
                    .replace(&config.api_key, "***REDACTED***")
                    .chars().take(500).collect::<String>();
                log::error!("LLM API 错误 {}: {}", status, safe_body);

                // 5xx / 429 可重试；4xx（除 429）不可重试
                let is_retryable = status.as_u16() >= 500 || status.as_u16() == 429;
                if is_retryable && attempt < MAX_RETRIES - 1 {
                    let wait = (attempt + 1) as u64 * 2; // 递增等待
                    log::warn!("LLM API {} 可重试，{}秒后第 {} 次重试", status, wait, attempt + 2);
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                    result = LlmResponse::default();
                    oa_tool_calls.clear();
                    anth_current_tool = None;
                    in_think = false;
                    think_buffer.clear();
                    continue;
                }
                let raw = format!("LLM API 错误 {}: {}", status, safe_body);
                return Err(classify_llm_error(&raw));
            }
            log::info!("LLM API 响应开始: status=200, 开始读取 SSE 流");

            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut chunk_debug_count = 0usize;
            let mut current_sse_event: Option<String> = None;
            // SSE 多行 data 累积缓冲（Responses API 事件 JSON 可能跨行）
            let mut data_accum = String::new();

            let stream_result: Result<(), String> = async {
                loop {
                    let maybe_chunk = timeout(Duration::from_secs(60), stream.next())
                        .await.map_err(|_| "流式读取超时（60 秒无数据）".to_string())?;
                    let chunk = match maybe_chunk {
                        Some(Ok(bytes)) => bytes,
                        Some(Err(e)) => return Err(e.to_string()),
                        None => break,
                    };
                    let text = String::from_utf8_lossy(&chunk);
                    buffer.push_str(&text);
                    if chunk_debug_count < 3 {
                        log::info!("SSE raw chunk #{}: {:?}", chunk_debug_count, text.chars().take(500).collect::<String>());
                        chunk_debug_count += 1;
                    }

                    while let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer[..newline_pos].trim().to_string();
                        buffer.drain(..newline_pos + 1);

                        // 追踪 SSE event: 行
                        if line.starts_with("event:") || line.starts_with("event: ") {
                            // 在新 event 之前，尝试解析累积的 data
                            if !data_accum.is_empty() {
                                Self::try_parse_sse_data(&data_accum, &current_sse_event, &mut result, &mut oa_tool_calls, &mut anth_current_tool, &tx, &mut in_think, &mut think_buffer, &self);
                                data_accum.clear();
                            }
                            let evt = line.splitn(2, ':').nth(1).unwrap_or("").trim().to_string();
                            current_sse_event = Some(evt);
                            continue;
                        }

                        // 空行 = SSE 事件分隔符，触发 dispatch
                        if line.is_empty() {
                            if !data_accum.is_empty() {
                                Self::try_parse_sse_data(&data_accum, &current_sse_event, &mut result, &mut oa_tool_calls, &mut anth_current_tool, &tx, &mut in_think, &mut think_buffer, &self);
                                data_accum.clear();
                            }
                            current_sse_event = None;
                            continue;
                        }

                        if !line.starts_with("data: ") && !line.starts_with("data:") { continue; }
                        let data = if line.starts_with("data: ") { &line[6..] } else { &line[5..] };
                        let data = data.trim();
                        if data == "[DONE]" { break; }

                        // 累积 data 行（SSE 规范：多个 data: 行用换行连接）
                        if !data_accum.is_empty() {
                            data_accum.push('\n');
                        }
                        data_accum.push_str(data);

                        // 尝试立即解析（大多数情况是单行完整 JSON）
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data_accum) {
                            log::debug!("SSE event={:?} data: {}", current_sse_event, json.to_string().chars().take(300).collect::<String>());
                            if let Some(err_msg) = Self::extract_sse_error(&json) {
                                log::error!("LLM API 返回错误事件: {}", err_msg);
                                let raw = format!("LLM 服务端错误: {}", err_msg);
                                return Err(classify_llm_error(&raw));
                            }
                            Self::extract_usage(&json, &mut result);
                            self.process_sse_event(&json, &mut result, &mut oa_tool_calls, &mut anth_current_tool, &tx, &mut in_think, &mut think_buffer);
                            data_accum.clear();
                            current_sse_event = None;
                        }
                        // 解析失败 → 继续累积下一行 data（不报错，等空行或下一个 event 触发最终解析）
                    }
                }

                // 残留累积数据 + 缓冲区
                if !data_accum.is_empty() {
                    Self::try_parse_sse_data(&data_accum, &current_sse_event, &mut result, &mut oa_tool_calls, &mut anth_current_tool, &tx, &mut in_think, &mut think_buffer, &self);
                }
                for line in buffer.lines() {
                    let line = line.trim();
                    if line.starts_with("event:") || line.starts_with("event: ") { continue; }
                    let data = if line.starts_with("data: ") { &line[6..] }
                        else if line.starts_with("data:") { &line[5..] }
                        else { continue };
                    let data = data.trim();
                    if data == "[DONE]" { continue; }
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                        Self::extract_usage(&json, &mut result);
                        self.process_sse_event(&json, &mut result, &mut oa_tool_calls, &mut anth_current_tool, &tx, &mut in_think, &mut think_buffer);
                    }
                }
                Ok(())
            }.await;

            match stream_result {
                Ok(()) => {
                    // 空回复检测：SSE 成功但内容和 tool_calls 都为空（代理 API 偶发空响应）
                    let has_content = !result.content.trim().is_empty();
                    let has_tools = !oa_tool_calls.is_empty() || anth_current_tool.is_some();
                    if !has_content && !has_tools && attempt < MAX_RETRIES - 1 {
                        let wait = (attempt + 1) as u64;
                        log::warn!("LLM SSE 成功但返回空内容（第 {} 次），{}秒后重试", attempt + 1, wait);
                        tokio::time::sleep(Duration::from_secs(wait)).await;
                        result = LlmResponse::default();
                        oa_tool_calls.clear();
                        anth_current_tool = None;
                        in_think = false;
                        think_buffer.clear();
                        continue;
                    }
                    break; // 有内容或重试耗尽
                }
                Err(e) => {
                    if attempt < MAX_RETRIES - 1 && (e.contains("超时") || e.contains("timeout") || e.contains("connection")) {
                        let wait = (attempt + 1) as u64;
                        log::warn!("SSE 流读取失败（第 {} 次），{}秒后重试: {}", attempt + 1, wait, e);
                        tokio::time::sleep(Duration::from_secs(wait)).await;
                        result = LlmResponse::default();
                        oa_tool_calls.clear();
                        anth_current_tool = None;
                        in_think = false;
                        think_buffer.clear();
                        continue;
                    }
                    return Err(classify_llm_error(&e));
                }
            }
        }

        // 转换累积的 tool calls
        // 对缺少 name 的 tool call，尝试从 arguments 推断工具名
        // 对缺少 id 的 tool call，自动生成 ID
        let mut filtered_count = 0usize;
        for (i, tc) in oa_tool_calls.iter().enumerate() {
            let mut name = tc.name.clone();
            let mut id = tc.id.clone();

            // 如果 name 为空，尝试从 arguments 的 key 推断工具名
            if name.trim().is_empty() {
                if let Ok(args_val) = serde_json::from_str::<serde_json::Value>(&tc.arguments) {
                    name = Self::infer_tool_name(&args_val, tools);
                }
            }

            // 推断后仍为空，跳过
            if name.trim().is_empty() {
                log::warn!("跳过无法推断名称的 tool_call: id='{}', args='{}'", tc.id, tc.arguments.chars().take(200).collect::<String>());
                filtered_count += 1;
                continue;
            }

            // 如果 id 为空，自动生成
            if id.trim().is_empty() {
                id = format!("call_{}_{}", chrono::Utc::now().timestamp_millis(), i);
                log::info!("为 tool_call '{}' 生成 ID: {}", name, id);
            }

            let arguments = serde_json::from_str(&tc.arguments).unwrap_or(serde_json::Value::Object(Default::default()));
            log::info!("解析 OpenAI tool_call: id={}, name={}, args_len={}", id, name, tc.arguments.len());
            result.tool_calls.push(ParsedToolCall { id, name, arguments });
        }
        if let Some(tc) = anth_current_tool {
            if tc.name.trim().is_empty() {
                log::warn!("跳过无效 Anthropic tool_call: id='{}', name 为空", tc.id);
            } else {
                let arguments = serde_json::from_str(&tc.input_json).unwrap_or(serde_json::Value::Object(Default::default()));
                result.tool_calls.push(ParsedToolCall { id: tc.id, name: tc.name, arguments });
            }
        }
        // 如果所有 tool calls 都被过滤且没有文本内容，生成提示消息
        if filtered_count > 0 && result.tool_calls.is_empty() && result.content.is_empty() {
            let fallback = "抱歉，我尝试执行操作但遇到了格式问题（工具调用缺少名称）。请重新描述您的需求，我会直接回复。".to_string();
            result.content = fallback.clone();
            let _ = tx.send(fallback);
        }
        if result.stop_reason.is_empty() { result.stop_reason = "stop".to_string(); }
        // 清理残留的 think 内容
        if !in_think {
            // think 已关闭，think_buffer 中可能还有非 think 内容
            if !think_buffer.is_empty() {
                result.content.push_str(&think_buffer);
                let _ = tx.send(think_buffer.clone());
                think_buffer.clear();
            }
        }
        let usage_str = if let Some(ref u) = result.usage {
            format!("input={}+output={}={}tokens", u.input_tokens, u.output_tokens, u.total_tokens)
        } else {
            "no_usage".to_string()
        };
        log::info!(
            "LLM SSE 流结束: content_len={}, tool_calls={}, inferred={}, filtered_invalid={}, stop_reason='{}', usage={}",
            result.content.len(), result.tool_calls.len(),
            result.tool_calls.iter().filter(|tc| tc.id.starts_with("call_")).count(),
            filtered_count, result.stop_reason, usage_str
        );
        Ok(result)
    }

    /// 从 arguments 的 key 推断工具名
    ///
    /// 根据已知工具的参数签名匹配：
    /// - {"command": ...} → bash_exec
    /// - {"expression": ...} → calculator
    /// - {"path": ..., "content": ...} → file_write
    /// - {"path": ..., "old_text": ...} → file_edit
    /// - {"path": ...} → file_read 或 file_list
    /// - {"query": ..., "path": ...} → code_search
    /// - {"url": ...} → web_fetch
    /// - {"key": ..., "value": ...} → memory_write
    /// - {"key": ...} → memory_read
    fn infer_tool_name(args: &serde_json::Value, available_tools: Option<&[ToolDefinition]>) -> String {
        let obj = match args.as_object() {
            Some(o) => o,
            None => return String::new(),
        };
        let keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();

        if keys.contains(&"command") {
            return "bash_exec".to_string();
        }
        if keys.contains(&"expression") {
            return "calculator".to_string();
        }
        if keys.contains(&"old_text") && keys.contains(&"path") {
            return "file_edit".to_string();
        }
        if keys.contains(&"content") && keys.contains(&"path") {
            return "file_write".to_string();
        }
        if keys.contains(&"query") && keys.contains(&"path") {
            return "code_search".to_string();
        }
        if keys.contains(&"url") {
            return "web_fetch".to_string();
        }
        if keys.contains(&"key") && keys.contains(&"value") && !keys.contains(&"action") {
            return "memory_write".to_string();
        }
        if keys.contains(&"key") && !keys.contains(&"action") {
            return "memory_read".to_string();
        }
        if keys.contains(&"path") {
            if keys.contains(&"pattern") || keys.contains(&"recursive") {
                return "file_list".to_string();
            }
            return "file_read".to_string();
        }

        // 自管理工具推断（基于 action 参数 + 其他特征）
        if keys.contains(&"action") {
            if keys.contains(&"provider") {
                return "provider_manage".to_string();
            }
            if keys.contains(&"agent_id") && (keys.contains(&"model") || keys.contains(&"temperature") || keys.contains(&"max_tokens")) {
                return "agent_self_config".to_string();
            }
            if keys.contains(&"agent_id") && (keys.contains(&"memory_type") || keys.contains(&"content")) {
                return "memory_write".to_string();
            }
            if keys.contains(&"agent_id") && keys.contains(&"query") {
                return "memory_read".to_string();
            }
            if keys.contains(&"key") || keys.contains(&"prefix") {
                return "settings_manage".to_string();
            }
            // action 只有 list 且没有其他区分字段 → 猜 provider_manage
            if let Some(action) = args["action"].as_str() {
                if action == "list" && keys.len() <= 2 {
                    return "provider_manage".to_string();
                }
            }
        }

        // 动态匹配：根据参数签名匹配可用工具（覆盖 skill tools 等动态注册的工具）
        if let Some(tools) = available_tools {
            let mut best_match: Option<(&str, usize)> = None; // (name, matched_required_count)
            for tool in tools {
                // 提取工具的 required 参数
                let required = tool.parameters.get("required")
                    .and_then(|r| r.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();
                let properties = tool.parameters.get("properties")
                    .and_then(|p| p.as_object())
                    .map(|p| p.keys().map(|k| k.as_str()).collect::<Vec<_>>())
                    .unwrap_or_default();

                // 所有 required 参数都在 args 中
                let all_required_match = required.iter().all(|r| keys.contains(r));
                // args 的所有 key 都在工具的 properties 中
                let all_keys_valid = keys.iter().all(|k| properties.contains(k));

                if all_required_match && all_keys_valid && !properties.is_empty() {
                    let score = required.len() + if all_keys_valid { keys.len() } else { 0 };
                    if best_match.is_none() || score > best_match.unwrap().1 {
                        best_match = Some((&tool.name, score));
                    }
                }
            }
            if let Some((name, _)) = best_match {
                log::info!("通过参数签名匹配到动态工具: {}, keys={:?}", name, keys);
                return name.to_string();
            }
        }

        log::warn!("无法从参数推断工具名, keys={:?}", keys);
        String::new()
    }
    /// 从 SSE JSON 中提取错误信息
    ///
    /// 检测以下格式：
    /// - `{"type":"response.failed","response":{"error":{"message":"..."}}}`  (Responses API)
    /// - `{"error":{"message":"..."}}`  (通用错误)
    fn extract_sse_error(json: &serde_json::Value) -> Option<String> {
        // Responses API 格式: type=response.failed
        if json["type"].as_str() == Some("response.failed") {
            let msg = json["response"]["error"]["message"].as_str()
                .or_else(|| json["response"]["error"]["code"].as_str())
                .unwrap_or("未知服务端错误");
            return Some(msg.to_string());
        }
        // 通用错误格式
        if let Some(err) = json.get("error") {
            let msg = err["message"].as_str()
                .or_else(|| err["code"].as_str())
                .unwrap_or("未知错误");
            return Some(msg.to_string());
        }
        None
    }

    /// 过滤 <think>...</think> 标签，返回非思考内容
    ///
    /// 流式处理：跟踪 in_think 状态和未完成的标签 buffer
    fn filter_think_tags(text: &str, in_think: &mut bool, buffer: &mut String) -> String {
        buffer.push_str(text);
        let mut emit = String::new();
        let mut i = 0;
        let buf_bytes = buffer.as_bytes();
        let buf_len = buf_bytes.len();

        while i < buf_len {
            if !*in_think {
                if buf_bytes[i] == b'<' {
                    let remaining = &buffer[i..];
                    if remaining.starts_with("<think>") {
                        *in_think = true;
                        i += 7;
                        continue;
                    } else if "<think>".starts_with(remaining) {
                        // 部分匹配，保留在 buffer 等下次
                        break;
                    } else {
                        emit.push(buf_bytes[i] as char);
                        i += 1;
                    }
                } else {
                    // 对 UTF-8 多字节字符安全处理
                    let ch = buffer[i..].chars().next().unwrap_or('?');
                    emit.push(ch);
                    i += ch.len_utf8();
                }
            } else {
                // 在 think 块内，寻找 </think>
                if buf_bytes[i] == b'<' {
                    let remaining = &buffer[i..];
                    if remaining.starts_with("</think>") {
                        *in_think = false;
                        i += 8;
                        continue;
                    } else if "</think>".starts_with(remaining) {
                        break;
                    }
                }
                // 跳过 UTF-8 字符
                let ch = buffer[i..].chars().next().unwrap_or('?');
                i += ch.len_utf8();
            }
        }

        *buffer = buffer[i..].to_string();
        emit
    }

    /// 从 SSE JSON 中提取 usage 统计
    fn extract_usage(json: &serde_json::Value, result: &mut LlmResponse) {
        // OpenAI 格式: {"usage": {"prompt_tokens": N, "completion_tokens": N, "total_tokens": N}}
        if let Some(usage) = json.get("usage").and_then(|u| u.as_object()) {
            let input = usage.get("prompt_tokens").or_else(|| usage.get("input_tokens"))
                .and_then(|v| v.as_u64()).unwrap_or(0);
            let output = usage.get("completion_tokens").or_else(|| usage.get("output_tokens"))
                .and_then(|v| v.as_u64()).unwrap_or(0);
            let total = usage.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(input + output);
            if input > 0 || output > 0 {
                result.usage = Some(LlmUsage {
                    input_tokens: input, output_tokens: output, total_tokens: total,
                    cache_read_tokens: 0, cache_creation_tokens: 0,
                });
            }
        }
        // Anthropic 格式: message_start.message.usage 或 message_delta.usage
        let anth_usage = json.pointer("/message/usage")
            .or_else(|| json.get("usage"))
            .and_then(|u| u.as_object());
        if let Some(usage) = anth_usage {
            let input = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            let output = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            let cache_read = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            let cache_creation = usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            if input > 0 || output > 0 || cache_read > 0 {
                let u = result.usage.get_or_insert(LlmUsage::default());
                u.input_tokens = input;
                u.output_tokens = u.output_tokens.max(output); // message_delta 可能更新
                u.total_tokens = input + u.output_tokens;
                u.cache_read_tokens = cache_read;
                u.cache_creation_tokens = cache_creation;
                if cache_read > 0 {
                    log::info!("Prompt Cache 命中: read={} tokens (省 90% 费用), creation={}", cache_read, cache_creation);
                }
            }
        }
    }

    /// 处理单个 SSE 事件
    /// 尝试解析累积的 SSE data（多行 data 合并后重试）
    fn try_parse_sse_data(
        data: &str,
        event: &Option<String>,
        result: &mut LlmResponse,
        oa_tool_calls: &mut Vec<OaToolCallAccum>,
        anth_current_tool: &mut Option<AnthToolAccum>,
        tx: &mpsc::UnboundedSender<String>,
        in_think: &mut bool,
        think_buffer: &mut String,
        this: &Self,
    ) {
        let trimmed = data.trim();
        if trimmed.is_empty() || trimmed == "[DONE]" { return; }
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(json) => {
                log::debug!("SSE(accumulated) event={:?} data: {}", event, json.to_string().chars().take(300).collect::<String>());
                Self::extract_usage(&json, result);
                this.process_sse_event(&json, result, oa_tool_calls, anth_current_tool, tx, in_think, think_buffer);
            }
            Err(_) => {
                // 非关键事件（如 Responses API 的 response.created/completed），降级为 debug
                log::debug!("SSE 多行 data 解析跳过 (event={:?}): {}", event, trimmed.chars().take(120).collect::<String>());
            }
        }
    }

    fn process_sse_event(
        &self,
        json: &serde_json::Value,
        result: &mut LlmResponse,
        oa_tool_calls: &mut Vec<OaToolCallAccum>,
        anth_current_tool: &mut Option<AnthToolAccum>,
        tx: &mpsc::UnboundedSender<String>,
        in_think: &mut bool,
        think_buffer: &mut String,
    ) {
        match self.config.provider.as_str() {
            "openai" => self.process_openai_sse(json, result, oa_tool_calls, tx, in_think, think_buffer),
            "anthropic" => self.process_anthropic_sse(json, result, anth_current_tool, tx),
            _ => {}
        }
    }

    fn process_openai_sse(
        &self,
        json: &serde_json::Value,
        result: &mut LlmResponse,
        oa_tool_calls: &mut Vec<OaToolCallAccum>,
        tx: &mpsc::UnboundedSender<String>,
        in_think: &mut bool,
        think_buffer: &mut String,
    ) {
        let choice = &json["choices"][0];
        let delta = &choice["delta"];

        // DeepSeek R1 等模型的 reasoning_content（单独字段，不需要 think 标签过滤）
        if let Some(_reasoning) = delta["reasoning_content"].as_str() {
            // 暂不转发思考内容到前端，仅记录
        }

        // 文本内容 — 含 <think> 标签过滤
        if let Some(content) = delta["content"].as_str() {
            if !content.is_empty() {
                let filtered = Self::filter_think_tags(content, in_think, think_buffer);
                if !filtered.is_empty() {
                    result.content.push_str(&filtered);
                    let _ = tx.send(filtered);
                }
            }
        }

        // tool_calls 增量 — 兼容多种 provider 格式
        // 标准 OpenAI: choices[0].delta.tool_calls
        // 某些 provider: choices[0].tool_calls 或 choices[0].delta.tool_calls 但 function 结构不同
        let tool_calls_val = if delta["tool_calls"].is_array() {
            Some(&delta["tool_calls"])
        } else if choice["tool_calls"].is_array() {
            Some(&choice["tool_calls"])
        } else {
            None
        };

        if let Some(tool_calls) = tool_calls_val.and_then(|v| v.as_array()) {
            for tc in tool_calls {
                // 首次出现 tool_calls 时记录原始 JSON，帮助调试 provider 格式
                if oa_tool_calls.is_empty() {
                    log::info!("原始 SSE tool_call 数据: {}", tc);
                }
                let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                // 扩展累积器
                while oa_tool_calls.len() <= idx {
                    oa_tool_calls.push(OaToolCallAccum { id: String::new(), name: String::new(), arguments: String::new() });
                }
                // id: 标准路径 tc.id，备选 tc.function.id
                if let Some(id) = tc["id"].as_str().or_else(|| tc["function"]["id"].as_str()) {
                    if !id.is_empty() {
                        oa_tool_calls[idx].id = id.to_string();
                    }
                }
                // name: 标准路径 tc.function.name，备选 tc.name, tc.function_call.name
                if let Some(name) = tc["function"]["name"].as_str()
                    .or_else(|| tc["name"].as_str())
                    .or_else(|| tc["function_call"]["name"].as_str())
                {
                    if !name.is_empty() {
                        oa_tool_calls[idx].name.push_str(name);
                    }
                }
                // arguments: 标准路径 tc.function.arguments，备选 tc.function_call.arguments, tc.arguments
                if let Some(args) = tc["function"]["arguments"].as_str()
                    .or_else(|| tc["function_call"]["arguments"].as_str())
                    .or_else(|| tc["arguments"].as_str())
                {
                    oa_tool_calls[idx].arguments.push_str(args);
                }
            }
        }

        // 某些 provider 使用旧版 function_call 格式（非 tool_calls）
        if let Some(fc) = delta.get("function_call") {
            if oa_tool_calls.is_empty() {
                log::info!("原始 SSE function_call 数据: {}", fc);
                oa_tool_calls.push(OaToolCallAccum {
                    id: "fc_0".to_string(),
                    name: String::new(),
                    arguments: String::new(),
                });
            }
            if let Some(name) = fc["name"].as_str() {
                if !name.is_empty() {
                    oa_tool_calls[0].name.push_str(name);
                }
            }
            if let Some(args) = fc["arguments"].as_str() {
                oa_tool_calls[0].arguments.push_str(args);
            }
        }

        // finish_reason
        if let Some(reason) = json["choices"][0]["finish_reason"].as_str() {
            result.stop_reason = reason.to_string();
        }

        // ── OpenAI Responses API 兼容 ──
        // 某些代理 API 返回 Responses API 格式而非 Chat Completions 格式
        // 检测 "type" 字段存在且不在 choices 结构中
        if json.get("choices").is_none() {
            if let Some(event_type) = json["type"].as_str() {
                match event_type {
                    // 文本内容增量
                    "response.output_text.delta" | "response.content_part.delta" => {
                        if let Some(text) = json["delta"].as_str()
                            .or_else(|| json["delta"]["text"].as_str())
                            .or_else(|| json["text"].as_str())
                        {
                            if !text.is_empty() {
                                let filtered = Self::filter_think_tags(text, in_think, think_buffer);
                                if !filtered.is_empty() {
                                    result.content.push_str(&filtered);
                                    let _ = tx.send(filtered);
                                }
                            }
                        }
                    }
                    // 响应完成
                    "response.completed" | "response.done" => {
                        if let Some(text) = json["response"]["output_text"].as_str()
                            .or_else(|| json.pointer("/response/output/0/content/0/text").and_then(|v| v.as_str()))
                        {
                            if result.content.is_empty() && !text.is_empty() {
                                result.content = text.to_string();
                                let _ = tx.send(text.to_string());
                            }
                        }
                        result.stop_reason = "stop".to_string();
                    }
                    // 工具调用（Responses API 格式）
                    "response.function_call_arguments.delta" => {
                        // Responses API 的工具调用增量
                        if let Some(delta) = json["delta"].as_str() {
                            if oa_tool_calls.is_empty() {
                                let name = json["name"].as_str()
                                    .or_else(|| json["item"]["name"].as_str())
                                    .unwrap_or("").to_string();
                                let id = json["call_id"].as_str()
                                    .or_else(|| json["item"]["call_id"].as_str())
                                    .unwrap_or("").to_string();
                                oa_tool_calls.push(OaToolCallAccum { id, name, arguments: String::new() });
                            }
                            if let Some(tc) = oa_tool_calls.last_mut() {
                                tc.arguments.push_str(delta);
                            }
                        }
                    }
                    _ => {} // 忽略其他 Responses API 事件
                }
            }
        }
    }

    fn process_anthropic_sse(
        &self,
        json: &serde_json::Value,
        result: &mut LlmResponse,
        anth_current_tool: &mut Option<AnthToolAccum>,
        tx: &mpsc::UnboundedSender<String>,
    ) {
        let event_type = json["type"].as_str().unwrap_or("");
        match event_type {
            "content_block_start" => {
                let block = &json["content_block"];
                if block["type"].as_str() == Some("tool_use") {
                    // 先保存之前的 tool
                    if let Some(prev) = anth_current_tool.take() {
                        let arguments = serde_json::from_str(&prev.input_json)
                            .unwrap_or(serde_json::Value::Object(Default::default()));
                        result.tool_calls.push(ParsedToolCall { id: prev.id, name: prev.name, arguments });
                    }
                    *anth_current_tool = Some(AnthToolAccum {
                        id: block["id"].as_str().unwrap_or("").to_string(),
                        name: block["name"].as_str().unwrap_or("").to_string(),
                        input_json: String::new(),
                    });
                }
            }
            "content_block_delta" => {
                let delta = &json["delta"];
                if delta["type"].as_str() == Some("text_delta") {
                    if let Some(text) = delta["text"].as_str() {
                        if !text.is_empty() {
                            result.content.push_str(text);
                            let _ = tx.send(text.to_string());
                        }
                    }
                } else if delta["type"].as_str() == Some("thinking_delta") {
                    // Anthropic extended thinking：流式思维内容
                    if let Some(thinking) = delta["thinking"].as_str() {
                        if !thinking.is_empty() {
                            result.thinking_content.push_str(thinking);
                            // 用特殊前缀发送给前端，便于区分
                            let _ = tx.send(format!("\x01THINKING\x01{}", thinking));
                        }
                    }
                } else if delta["type"].as_str() == Some("input_json_delta") {
                    if let Some(partial) = delta["partial_json"].as_str() {
                        if let Some(ref mut tool) = anth_current_tool {
                            tool.input_json.push_str(partial);
                        }
                    }
                }
            }
            "message_delta" => {
                if let Some(reason) = json["delta"]["stop_reason"].as_str() {
                    result.stop_reason = reason.to_string();
                }
            }
            _ => {}
        }
    }
}

/// 对 LLM 原始错误信息进行分类，返回用户友好的提示
///
/// 保持原始错误在日志中，仅改善用户看到的提示文本
pub fn classify_llm_error(error: &str) -> String {
    let lower = error.to_lowercase();

    // 429 / rate limit
    if lower.contains("429") || lower.contains("rate limit") || lower.contains("too many requests") || lower.contains("请求过于频繁") {
        return "请求过于频繁，请稍后重试".to_string();
    }

    // 401 / unauthorized
    if lower.contains("401") || lower.contains("unauthorized") || lower.contains("invalid api key")
        || lower.contains("invalid x-api-key") || lower.contains("authentication") || lower.contains("api key") && lower.contains("invalid")
    {
        return "API Key 无效或已过期，请检查设置".to_string();
    }

    // 402 / payment / quota / 余额 / 积分
    if lower.contains("402") || lower.contains("payment required") || lower.contains("insufficient")
        || lower.contains("quota") || lower.contains("余额不足") || lower.contains("积分")
        || lower.contains("额度") || lower.contains("credit") || lower.contains("billing")
    {
        return "API 额度不足，请充值或更换供应商".to_string();
    }

    // 5xx server errors
    if lower.contains("500") || lower.contains("502") || lower.contains("503") || lower.contains("504")
        || lower.contains("internal server error") || lower.contains("bad gateway")
        || lower.contains("service unavailable") || lower.contains("服务端错误")
    {
        return "AI 服务暂时不可用，请稍后重试".to_string();
    }

    // timeout
    if lower.contains("timeout") || lower.contains("超时") || lower.contains("timed out") {
        return "请求超时，请检查网络或重试".to_string();
    }

    // connection errors
    if lower.contains("connection refused") || lower.contains("connection reset")
        || lower.contains("connect error") || lower.contains("dns") || lower.contains("无法连接")
        || lower.contains("连接失败") || lower.contains("network") && lower.contains("error")
    {
        return "无法连接到 AI 服务，请检查网络".to_string();
    }

    // 无法分类的错误，返回原始信息（截断过长文本）
    let trimmed: String = error.chars().take(200).collect();
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_config() {
        let c = LlmConfig::openai("key".into(), "gpt-4".into());
        assert_eq!(c.provider, "openai");
    }

    #[test]
    fn test_anthropic_config() {
        let c = LlmConfig::anthropic("key".into(), "claude-3".into());
        assert_eq!(c.provider, "anthropic");
    }

    #[test]
    fn test_system_blocks_single() {
        let b = LlmClient::build_anthropic_system_blocks("Hi.");
        assert_eq!(b.as_array().unwrap().len(), 1);
        assert_eq!(b[0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn test_system_blocks_multi() {
        let b = LlmClient::build_anthropic_system_blocks("A\n\n---\n\nB\n\n---\n\nC");
        let a = b.as_array().unwrap();
        assert_eq!(a.len(), 3);
        // stable_boundary=3, last_idx=2 → section 2 标记（既是 stable 末尾又是 last）
        assert!(a[0].get("cache_control").is_none());
        assert!(a[1].get("cache_control").is_none());
        assert_eq!(a[2]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn test_system_blocks_empty() {
        let b = LlmClient::build_anthropic_system_blocks("");
        assert!(b.as_array().unwrap().is_empty());
    }

    #[test]
    fn test_build_openai_tools() {
        let t = vec![ToolDefinition { name: "c".into(), description: "d".into(), parameters: serde_json::json!({}) }];
        let r = LlmClient::build_openai_tools(&t);
        assert_eq!(r[0]["type"], "function");
        assert_eq!(r[0]["function"]["name"], "c");
    }

    #[test]
    fn test_build_anthropic_tools() {
        let t = vec![ToolDefinition { name: "c".into(), description: "d".into(), parameters: serde_json::json!({}) }];
        let r = LlmClient::build_anthropic_tools(&t);
        assert_eq!(r[0]["name"], "c");
    }

    #[test]
    fn test_llm_response_default() {
        let r = LlmResponse::default();
        assert!(!r.has_tool_calls());
        assert!(r.content.is_empty());
    }

    // 辅助宏：创建 SSE 测试的 think 状态
    fn sse_test_event(c: &LlmClient, j: &serde_json::Value, r: &mut LlmResponse, oa: &mut Vec<OaToolCallAccum>, an: &mut Option<AnthToolAccum>, tx: &mpsc::UnboundedSender<String>) {
        let mut it = false;
        let mut tb = String::new();
        c.process_sse_event(j, r, oa, an, tx, &mut it, &mut tb);
    }

    #[test]
    fn test_openai_sse_text() {
        let c = LlmClient::new(LlmConfig::openai("k".into(), "m".into()));
        let j = serde_json::json!({"choices":[{"delta":{"content":"hi"},"finish_reason":null}]});
        let mut r = LlmResponse::default();
        let mut oa = Vec::new();
        let mut an = None;
        let (tx, _rx) = mpsc::unbounded_channel();
        sse_test_event(&c, &j, &mut r, &mut oa, &mut an, &tx);
        assert_eq!(r.content, "hi");
    }

    #[test]
    fn test_openai_sse_tool() {
        let c = LlmClient::new(LlmConfig::openai("k".into(), "m".into()));
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut r = LlmResponse::default();
        let mut oa = Vec::new();
        let mut an = None;
        let j = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"x","arguments":"{}"}}]},"finish_reason":null}]});
        sse_test_event(&c, &j, &mut r, &mut oa, &mut an, &tx);
        assert_eq!(oa.len(), 1);
        assert_eq!(oa[0].id, "c1");
        assert_eq!(oa[0].name, "x");
    }

    #[test]
    fn test_anthropic_sse_tool() {
        let c = LlmClient::new(LlmConfig::anthropic("k".into(), "m".into()));
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut r = LlmResponse::default();
        let mut oa = Vec::new();
        let mut an: Option<AnthToolAccum> = None;
        let j1 = serde_json::json!({"type":"content_block_start","content_block":{"type":"tool_use","id":"t1","name":"y"}});
        sse_test_event(&c, &j1, &mut r, &mut oa, &mut an, &tx);
        assert!(an.is_some());
        let j2 = serde_json::json!({"type":"content_block_delta","delta":{"type":"input_json_delta","partial_json":"{}"}});
        sse_test_event(&c, &j2, &mut r, &mut oa, &mut an, &tx);
        assert_eq!(an.unwrap().input_json, "{}");
    }

    #[test]
    fn test_anthropic_sse_text() {
        let c = LlmClient::new(LlmConfig::anthropic("k".into(), "m".into()));
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut r = LlmResponse::default();
        let mut oa = Vec::new();
        let mut an = None;
        let j = serde_json::json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"ok"}});
        sse_test_event(&c, &j, &mut r, &mut oa, &mut an, &tx);
        assert_eq!(r.content, "ok");
    }

    #[test]
    fn test_anthropic_sse_stop() {
        let c = LlmClient::new(LlmConfig::anthropic("k".into(), "m".into()));
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut r = LlmResponse::default();
        let mut oa = Vec::new();
        let mut an = None;
        let j = serde_json::json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}});
        sse_test_event(&c, &j, &mut r, &mut oa, &mut an, &tx);
        assert_eq!(r.stop_reason, "tool_use");
    }

    #[test]
    fn test_classify_llm_error_rate_limit() {
        assert_eq!(classify_llm_error("LLM API 错误 429: Too Many Requests"), "请求过于频繁，请稍后重试");
        assert_eq!(classify_llm_error("rate limit exceeded"), "请求过于频繁，请稍后重试");
    }

    #[test]
    fn test_classify_llm_error_unauthorized() {
        assert_eq!(classify_llm_error("LLM API 错误 401: Unauthorized"), "API Key 无效或已过期，请检查设置");
        assert_eq!(classify_llm_error("Invalid API key provided"), "API Key 无效或已过期，请检查设置");
    }

    #[test]
    fn test_classify_llm_error_payment() {
        assert_eq!(classify_llm_error("LLM API 错误 402: Payment Required"), "API 额度不足，请充值或更换供应商");
        assert_eq!(classify_llm_error("余额不足，请充值"), "API 额度不足，请充值或更换供应商");
        assert_eq!(classify_llm_error("insufficient quota"), "API 额度不足，请充值或更换供应商");
    }

    #[test]
    fn test_classify_llm_error_server() {
        assert_eq!(classify_llm_error("LLM API 错误 500: Internal Server Error"), "AI 服务暂时不可用，请稍后重试");
        assert_eq!(classify_llm_error("502 Bad Gateway"), "AI 服务暂时不可用，请稍后重试");
    }

    #[test]
    fn test_classify_llm_error_timeout() {
        assert_eq!(classify_llm_error("流式读取超时（60 秒无数据）"), "请求超时，请检查网络或重试");
        assert_eq!(classify_llm_error("request timed out"), "请求超时，请检查网络或重试");
    }

    #[test]
    fn test_classify_llm_error_connection() {
        assert_eq!(classify_llm_error("connection refused"), "无法连接到 AI 服务，请检查网络");
        assert_eq!(classify_llm_error("dns resolution failed"), "无法连接到 AI 服务，请检查网络");
    }

    #[test]
    fn test_classify_llm_error_unknown() {
        assert_eq!(classify_llm_error("some unknown error"), "some unknown error");
    }
}
