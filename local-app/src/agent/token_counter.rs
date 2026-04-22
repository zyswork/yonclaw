//! Token 计数器
//!
//! 封装 tiktoken-rs 提供 token 计数能力，支持不同模型的 tokenizer。
//! 使用 OnceLock 缓存 BPE 实例，避免重复初始化。

use std::sync::OnceLock;
use tiktoken_rs::CoreBPE;

/// 全局缓存的 BPE tokenizer（仅初始化一次）
static BPE_CACHE: OnceLock<Option<CoreBPE>> = OnceLock::new();

/// 获取缓存的 BPE tokenizer
///
/// 首次调用时初始化，后续直接复用。
/// 如果初始化失败返回 None，调用方应使用降级估算。
fn get_bpe() -> Option<&'static CoreBPE> {
    BPE_CACHE.get_or_init(|| {
        log::info!("初始化 tiktoken BPE tokenizer...");
        match tiktoken_rs::cl100k_base() {
            Ok(bpe) => {
                log::info!("tiktoken BPE tokenizer 初始化成功");
                Some(bpe)
            }
            Err(e) => {
                log::warn!("tiktoken BPE tokenizer 初始化失败: {}，将使用字符估算", e);
                None
            }
        }
    }).as_ref()
}

/// Token 计数器
pub struct TokenCounter;

impl TokenCounter {
    /// 估算文本的 token 数
    ///
    /// 使用 cl100k_base（GPT-4/Claude 系列）tokenizer。
    /// 如果 tiktoken 初始化失败，使用粗略估算（字符数 / 4）。
    pub fn count(text: &str) -> usize {
        if let Some(bpe) = get_bpe() {
            bpe.encode_with_special_tokens(text).len()
        } else {
            // 降级：粗略估算
            text.len() / 4
        }
    }

    /// 估算消息列表的总 token 数
    ///
    /// 每条消息额外计入 4 token 的消息开销（role 标记等）
    pub fn count_messages(messages: &[serde_json::Value]) -> usize {
        let mut total = 0;
        for msg in messages {
            // 每条消息有约 4 token 的开销
            total += 4;
            if let Some(content) = msg["content"].as_str() {
                total += Self::count(content);
            }
            if let Some(role) = msg["role"].as_str() {
                total += Self::count(role);
            }
        }
        // 回复开头的 assistant 标记
        total += 3;
        total
    }

    /// 按 token 预算截断文本，保留头部 70% + 尾部 20%，在换行边界切割
    pub fn truncate_to_budget(text: &str, max_tokens: usize) -> String {
        let current = Self::count(text);
        if current <= max_tokens {
            return text.to_string();
        }

        let lines: Vec<&str> = text.lines().collect();
        let head_budget = (max_tokens as f64 * 0.7) as usize;
        let tail_budget = (max_tokens as f64 * 0.2) as usize;

        let mut head_lines = Vec::new();
        let mut head_used = 0;
        for line in &lines {
            let lt = Self::count(line) + 1; // +1 换行符
            if head_used + lt > head_budget {
                break;
            }
            head_used += lt;
            head_lines.push(*line);
        }

        let mut tail_lines = Vec::new();
        let mut tail_used = 0;
        for line in lines.iter().rev() {
            let lt = Self::count(line) + 1;
            if tail_used + lt > tail_budget {
                break;
            }
            tail_used += lt;
            tail_lines.push(*line);
        }
        tail_lines.reverse();

        format!(
            "{}\n\n[... 已截断 ...]\n\n{}",
            head_lines.join("\n"),
            tail_lines.join("\n")
        )
    }

    /// 获取模型的上下文窗口大小
    pub fn model_context_window(model: &str) -> usize {
        let m = model.to_lowercase();
        match &*m {
            // OpenClaw #66453: gpt-5.4-pro 前向兼容，256K 窗口
            _ if m.starts_with("gpt-5.4-pro") || m.starts_with("gpt-5-pro") => 256_000,
            _ if m.starts_with("gpt-5") => 128_000,
            _ if m.starts_with("gpt-4o") => 128_000,
            _ if m.starts_with("gpt-4-turbo") => 128_000,
            _ if m.starts_with("gpt-4") => 8_192,
            _ if m.starts_with("gpt-3.5") => 16_385,
            _ if m.contains("claude") => 200_000,
            _ if m.starts_with("deepseek") => 64_000,
            _ if m.starts_with("qwen-long") => 128_000,
            _ if m.starts_with("qwen") => 32_768,
            _ if m.starts_with("glm") => 128_000,
            _ if m.starts_with("abab") || m.starts_with("minimax") => 128_000,
            _ if m.starts_with("gemini") => 128_000,
            _ if m.starts_with("o1") || m.starts_with("o3") || m.starts_with("o4") => 128_000,
            // 对代理 API 的自定义模型名，假设大窗口
            _ if m.contains("gpt") || m.contains("turbo") => 128_000,
            _ => 64_000, // 提高默认值，现代模型至少 64K
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_tokens() {
        let text = "Hello, world! This is a test.";
        let count = TokenCounter::count(text);
        // cl100k_base 对这段文本应该返回合理的 token 数
        assert!(count > 0);
        assert!(count < 20); // 不应超过 20 token
    }

    #[test]
    fn test_count_messages() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": "Hello"}),
            serde_json::json!({"role": "assistant", "content": "Hi there!"}),
        ];
        let count = TokenCounter::count_messages(&messages);
        assert!(count > 5);
    }

    #[test]
    fn test_model_context_window() {
        assert_eq!(TokenCounter::model_context_window("gpt-4o-mini"), 128_000);
        assert_eq!(TokenCounter::model_context_window("gpt-5.4"), 128_000);
        assert_eq!(TokenCounter::model_context_window("claude-sonnet-4-20250514"), 200_000);
        assert_eq!(TokenCounter::model_context_window("deepseek-chat"), 64_000);
        assert_eq!(TokenCounter::model_context_window("unknown-model"), 64_000);
    }

    #[test]
    fn test_empty_text() {
        assert_eq!(TokenCounter::count(""), 0);
    }

    #[test]
    fn test_chinese_text() {
        let text = "你好世界，这是一个测试。";
        let count = TokenCounter::count(text);
        assert!(count > 0);
    }
}
