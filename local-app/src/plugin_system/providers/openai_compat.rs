//! OpenAI 兼容 ModelProvider
//!
//! 支持 OpenAI API 格式的所有提供商：OpenAI、DeepSeek、Qwen、通义千问代理等。
//! Phase 1: 包装现有 LlmClient 实现。

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::agent::llm::{LlmClient, LlmConfig, LlmResponse};
use crate::agent::tools::ToolDefinition;
use super::super::provider_trait::{ModelProvider, CallConfig};

/// OpenAI 兼容提供商
pub struct OpenAiCompatProvider;

impl OpenAiCompatProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ModelProvider for OpenAiCompatProvider {
    fn id(&self) -> &str {
        "openai"
    }

    fn display_name(&self) -> &str {
        "OpenAI 兼容"
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "gpt-4o".into(), "gpt-4o-mini".into(), "gpt-4-turbo".into(),
            "gpt-3.5-turbo".into(), "gpt-5.4".into(),
            "o1".into(), "o3".into(), "o3-mini".into(),
            "deepseek".into(), "qwen".into(),
        ]
    }

    fn supports_model(&self, model: &str) -> bool {
        // OpenAI 兼容格式是默认 fallback：不是 claude 就用这个
        !model.starts_with("claude")
    }

    async fn call_stream(
        &self,
        config: &CallConfig,
        messages: &[serde_json::Value],
        system_prompt: Option<&str>,
        tools: Option<&[ToolDefinition]>,
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<LlmResponse, String> {
        let llm_config = LlmConfig {
            provider: "openai".to_string(),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
            base_url: config.base_url.clone(),
            temperature: config.temperature,
            max_tokens: config.max_tokens.map(|m| m as i32),
            thinking_level: None,
        };
        let client = LlmClient::new(llm_config);
        client.call_stream(messages, system_prompt, tools, tx).await
    }
}
