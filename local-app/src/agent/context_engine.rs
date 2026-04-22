//! 可插拔 Context Engine
//!
//! 参照 OpenClaw #56224 / #62179 的 pluggable context engine 概念：
//! 允许在同一个 agent 上切换不同的消息组装策略。
//!
//! 默认 engine: `LegacyEngine` —— 和原来的 context_guard 行为一致
//! 可扩展: FileAwareEngine（为 file tool 调用保留更多上下文）、
//!        RagHeavyEngine（把记忆召回当作主要 context 源），等
//!
//! **当前状态**：留好 trait + 注册表 + 默认实现；具体替换留给后续迭代。

use std::collections::HashMap;
use std::sync::Arc;

/// Context Engine 输入
pub struct EngineInput<'a> {
    pub agent_id: &'a str,
    pub session_id: &'a str,
    pub system_prompt: &'a str,
    pub messages: &'a [serde_json::Value],
    pub model: &'a str,
}

/// Context Engine 输出（可能经过裁剪/重排/拼接的消息数组）
pub struct EngineOutput {
    pub messages: Vec<serde_json::Value>,
    /// 引擎生成的元数据（如 cache 提示、token 估算）
    pub metadata: serde_json::Value,
}

/// Context Engine trait
pub trait ContextEngine: Send + Sync {
    /// 引擎 ID
    fn id(&self) -> &str;

    /// 处理输入消息，返回可发送给 LLM 的消息序列
    fn process(&self, input: &EngineInput<'_>) -> EngineOutput;
}

/// 默认 engine：直接透传（保留既有 context_guard 在上游执行）
pub struct LegacyEngine;

impl ContextEngine for LegacyEngine {
    fn id(&self) -> &str { "legacy" }

    fn process(&self, input: &EngineInput<'_>) -> EngineOutput {
        EngineOutput {
            messages: input.messages.to_vec(),
            metadata: serde_json::json!({ "engine": "legacy" }),
        }
    }
}

/// 注册表
#[derive(Default)]
pub struct ContextEngineRegistry {
    engines: HashMap<String, Arc<dyn ContextEngine>>,
    default_id: String,
}

impl ContextEngineRegistry {
    pub fn new_with_legacy() -> Self {
        let mut r = Self::default();
        r.register(Arc::new(LegacyEngine));
        r.default_id = "legacy".to_string();
        r
    }

    pub fn register(&mut self, engine: Arc<dyn ContextEngine>) {
        self.engines.insert(engine.id().to_string(), engine);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn ContextEngine>> {
        self.engines.get(id).cloned()
    }

    pub fn default_engine(&self) -> Arc<dyn ContextEngine> {
        self.engines.get(&self.default_id)
            .cloned()
            .unwrap_or_else(|| Arc::new(LegacyEngine))
    }

    pub fn list_ids(&self) -> Vec<String> {
        self.engines.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legacy_passthrough() {
        let reg = ContextEngineRegistry::new_with_legacy();
        let engine = reg.default_engine();
        let msgs = vec![serde_json::json!({"role": "user", "content": "hi"})];
        let out = engine.process(&EngineInput {
            agent_id: "a", session_id: "s", system_prompt: "",
            messages: &msgs, model: "gpt-4o",
        });
        assert_eq!(out.messages.len(), 1);
    }
}
