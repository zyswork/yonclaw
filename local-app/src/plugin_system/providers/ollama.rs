//! Ollama 本地模型 Provider
//!
//! 通过 Ollama API（兼容 OpenAI 格式）调用本地运行的模型。
//! 无需 API Key，支持 Llama、Mistral、Qwen、Gemma 等模型。

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::agent::llm::{LlmClient, LlmConfig, LlmResponse};
use crate::agent::tools::ToolDefinition;
use super::super::provider_trait::{ModelProvider, CallConfig};

/// Ollama 本地模型提供商
pub struct OllamaProvider;

impl OllamaProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    fn id(&self) -> &str {
        "ollama"
    }

    fn display_name(&self) -> &str {
        "Ollama (本地模型)"
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "llama3".into(), "llama3.1".into(), "llama3.2".into(),
            "mistral".into(), "mixtral".into(),
            "qwen2".into(), "qwen2.5".into(),
            "gemma2".into(), "gemma3".into(),
            "phi3".into(), "phi4".into(),
            "codellama".into(), "deepseek-coder".into(),
        ]
    }

    fn supports_model(&self, model: &str) -> bool {
        // Ollama 模型通常以这些前缀开头
        let prefixes = ["llama", "mistral", "mixtral", "qwen2", "gemma", "phi", "codellama", "deepseek-coder", "ollama/"];
        prefixes.iter().any(|p| model.to_lowercase().starts_with(p))
    }

    async fn call_stream(
        &self,
        config: &CallConfig,
        messages: &[serde_json::Value],
        system_prompt: Option<&str>,
        tools: Option<&[ToolDefinition]>,
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<LlmResponse, String> {
        // Ollama 使用 OpenAI 兼容格式，base_url 默认指向本地
        let base_url = config.base_url.clone()
            .unwrap_or_else(|| "http://localhost:11434/v1".to_string());

        let llm_config = LlmConfig {
            provider: "openai".to_string(), // Ollama 的 /v1 端点兼容 OpenAI 格式
            model: config.model.clone(),
            api_key: config.api_key.clone(), // Ollama 不需要 key，但字段必填
            base_url: Some(base_url),
            temperature: config.temperature,
            max_tokens: config.max_tokens.map(|m| m as i32),
            thinking_level: None,
        };
        let client = LlmClient::new(llm_config);
        client.call_stream(messages, system_prompt, tools, tx).await
    }

    async fn health_check(&self, config: &CallConfig) -> Result<bool, String> {
        let base_url = config.base_url.clone()
            .unwrap_or_else(|| "http://localhost:11434".to_string());
        let url = format!("{}/api/tags", base_url.trim_end_matches("/v1"));

        match reqwest::Client::new().get(&url).timeout(std::time::Duration::from_secs(3)).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}
