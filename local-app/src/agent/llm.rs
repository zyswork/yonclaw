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
fn sanitize_messages_for_anthropic(messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut result = Vec::with_capacity(messages.len());

    // 第一步：转换消息格式
    for msg in messages {
        let role = msg["role"].as_str().unwrap_or("");
        match role {
            "system" => continue, // Anthropic 用顶层 system 字段
            "tool" => {
                // OpenAI tool result → Anthropic user + tool_result
                let raw_id = msg["tool_call_id"].as_str().unwrap_or("unknown");
                let id = sanitize_tool_id(raw_id);
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
                                // 清理 tool_use id
                                if let Some(id) = b["id"].as_str() {
                                    block["id"] = serde_json::Value::String(sanitize_tool_id(id));
                                }
                                Some(block)
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
                    // 纯文本 assistant — 保持字符串
                    result.push(msg.clone());
                }
            }
            _ => {
                // user 消息
                if let Some(arr) = msg["content"].as_array() {
                    // 已是数组（可能含 tool_result）
                    result.push(msg.clone());
                } else {
                    // 纯文本 user — 保持字符串
                    result.push(msg.clone());
                }
            }
        }
    }

    // 第二步：确保每个 tool_use 都有对应的 tool_result（参考 OpenClaw transformMessages）
    ensure_tool_use_result_pairing(&mut result);

    // 第三步：合并连续同 role 消息
    merge_consecutive_roles(&mut result);

    // 第四步：确保第一条消息是 user（Anthropic 要求）
    if let Some(first) = result.first() {
        if first["role"].as_str() != Some("user") {
            result.insert(0, serde_json::json!({"role": "user", "content": "Continue."}));
        }
    }

    result
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

/// 清理消息给 OpenAI 用：把 Anthropic 格式的 content 数组转回字符串
fn sanitize_messages_for_openai(messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    messages.iter().map(|msg| {
        let mut m = msg.clone();
        // 如果 content 是数组（Anthropic 格式），提取文本拼为字符串
        if let Some(arr) = msg["content"].as_array() {
            let texts: Vec<String> = arr.iter().filter_map(|block| {
                match block["type"].as_str() {
                    Some("text") => block["text"].as_str().map(|s| s.to_string()),
                    Some("tool_use") => {
                        let name = block["name"].as_str().unwrap_or("tool");
                        Some(format!("[Called tool: {}]", name))
                    }
                    Some("tool_result") => {
                        let content = block["content"].as_str().unwrap_or("");
                        if content.is_empty() || content == "No result provided" { None }
                        else { Some(content.to_string()) }
                    }
                    _ => None,
                }
            }).collect();
            m["content"] = serde_json::Value::String(texts.join("\n"));
            // 移除 Anthropic 字段
            m.as_object_mut().map(|o| { o.remove("tool_calls"); });
        }
        m
    }).filter(|m| {
        // 过滤空内容消息
        let content = m["content"].as_str().unwrap_or("");
        !content.is_empty()
    }).collect()
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

/// LLM 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i32>,
}

impl LlmConfig {
    pub fn openai(api_key: String, model: String) -> Self {
        Self {
            provider: "openai".to_string(),
            api_key, model,
            base_url: Some("https://api.openai.com/v1".to_string()),
            temperature: None, max_tokens: None,
        }
    }
    pub fn anthropic(api_key: String, model: String) -> Self {
        Self {
            provider: "anthropic".to_string(),
            api_key, model,
            base_url: Some("https://api.anthropic.com/v1".to_string()),
            temperature: None, max_tokens: None,
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
            .timeout(std::time::Duration::from_secs(120))
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
        let (url, body) = match config.provider.as_str() {
            "openai" => {
                let url = format!("{}/chat/completions", config.base_url.as_deref().unwrap_or("https://api.openai.com/v1"));
                let resolved_max_tokens = config.max_tokens.filter(|&t| t > 0).unwrap_or_else(|| default_max_tokens(&config.provider, &config.model));
                // 清理历史中可能混入的 Anthropic 格式消息
                let clean_messages = sanitize_messages_for_openai(messages);
                let mut body = serde_json::json!({
                    "model": config.model, "messages": clean_messages, "stream": true,
                    "temperature": config.temperature.unwrap_or(0.7),
                    "max_tokens": resolved_max_tokens,
                    "stream_options": {"include_usage": true},
                });
                if let Some(t) = tools { if !t.is_empty() {
                    body["tools"] = Self::build_openai_tools(t);
                    body["tool_choice"] = serde_json::json!("auto");
                    body["parallel_tool_calls"] = serde_json::json!(true);
                } }
                (url, body)
            }
            "anthropic" => {
                let url = build_anthropic_url(config.base_url.as_deref());
                let resolved_max_tokens = config.max_tokens.filter(|&t| t > 0).unwrap_or_else(|| default_max_tokens(&config.provider, &config.model));
                // 转换 OpenAI 格式的 tool 消息为 Anthropic 格式
                let clean_messages = sanitize_messages_for_anthropic(messages);
                log::info!("sanitize 完成: {} 条消息", clean_messages.len());
                // 调试：打印每条消息的结构
                for (i, m) in clean_messages.iter().enumerate() {
                    let role = m["role"].as_str().unwrap_or("?");
                    let content_type = if m["content"].is_array() { "array" } else if m["content"].is_string() { "STRING(!)" } else { "other(!)" };
                    let has_tool_calls = m.get("tool_calls").is_some();
                    let preview = &m["content"].to_string()[..m["content"].to_string().len().min(80)];
                    log::info!("Anthropic msg[{}]: role={}, content={}, tool_calls={}, preview={}", i, role, content_type, has_tool_calls, preview);
                }
                let mut body = serde_json::json!({
                    "model": config.model, "messages": clean_messages, "stream": true,
                    "temperature": config.temperature.unwrap_or(1.0),
                    "max_tokens": resolved_max_tokens,
                });
                log::info!("body 构建完成，添加 system+tools...");
                if let Some(sp) = system_prompt { body["system"] = Self::build_anthropic_system_blocks(sp); }
                if let Some(t) = tools { if !t.is_empty() { body["tools"] = Self::build_anthropic_tools(t); } }
                // 调试：保存请求 body 到文件（临时）
                let _ = std::fs::write("/tmp/anthropic-debug.json", serde_json::to_string_pretty(&body).unwrap_or_default());
                log::info!("Anthropic 请求 body 已保存到 /tmp/anthropic-debug.json (size={})", body.to_string().len());
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
                "openai" => { req = req.header("Authorization", format!("Bearer {}", config.api_key)); }
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
                    return Err(format!("LLM 连接失败（重试 {} 次后）: {}", MAX_RETRIES, e));
                }
            };
            if !response.status().is_success() {
                let status = response.status();
                let body_text = response.text().await.unwrap_or_default();
                log::error!("LLM API 错误 {}: {}", status, body_text.chars().take(500).collect::<String>());

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
                return Err(format!("LLM API 错误 {}: {}", status, body_text));
            }
            log::info!("LLM API 响应开始: status=200, 开始读取 SSE 流");

            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut chunk_debug_count = 0usize;
            let mut current_sse_event: Option<String> = None;

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
                            let evt = line.splitn(2, ':').nth(1).unwrap_or("").trim().to_string();
                            // 检测错误事件
                            if evt == "error" || evt == "response.failed" {
                                current_sse_event = Some(evt);
                                continue;
                            }
                            current_sse_event = Some(evt);
                            continue;
                        }

                        if !line.starts_with("data: ") && !line.starts_with("data:") { continue; }
                        let data = if line.starts_with("data: ") { &line[6..] } else { &line[5..] };
                        let data = data.trim();
                        if data == "[DONE]" { break; }

                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                            log::debug!("SSE event={:?} data: {}", current_sse_event, json.to_string().chars().take(300).collect::<String>());
                            // 检测错误事件
                            if let Some(err_msg) = Self::extract_sse_error(&json) {
                                log::error!("LLM API 返回错误事件: {}", err_msg);
                                return Err(format!("LLM 服务端错误: {}", err_msg));
                            }
                            // 提取 usage 统计
                            Self::extract_usage(&json, &mut result);
                            // 处理正常事件
                            self.process_sse_event(&json, &mut result, &mut oa_tool_calls, &mut anth_current_tool, &tx, &mut in_think, &mut think_buffer);
                        } else {
                            log::warn!("SSE JSON 解析失败: {:?}", data.chars().take(200).collect::<String>());
                        }
                        current_sse_event = None;
                    }
                }

                // 残留缓冲区
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
                    return Err(e);
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
}
