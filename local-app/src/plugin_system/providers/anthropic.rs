//! Anthropic ModelProvider
//!
//! Claude 系列模型，支持 prompt caching。
//! Phase 1: 包装现有 LlmClient 实现。

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::agent::llm::{LlmClient, LlmConfig, LlmResponse};
use crate::agent::tools::ToolDefinition;
use super::super::provider_trait::{ModelProvider, CallConfig};

/// Anthropic 提供商
pub struct AnthropicProvider;

impl AnthropicProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn display_name(&self) -> &str {
        "Anthropic"
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "claude-3-opus".into(), "claude-3-sonnet".into(), "claude-3-haiku".into(),
            "claude-3.5-sonnet".into(), "claude-3.5-haiku".into(),
            "claude-4-sonnet".into(), "claude-4-opus".into(),
        ]
    }

    fn supports_model(&self, model: &str) -> bool {
        model.starts_with("claude")
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
            provider: "anthropic".to_string(),
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
