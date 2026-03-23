//! 观察者模式 — Agent 事件广播
//!
//! 在 LLM 调用、工具执行等关键环节发布事件，
//! 前端通过 Tauri event 监听实时状态。
//! 借鉴 ZeroClaw 的 BroadcastObserver。

use serde::Serialize;

/// Agent 事件类型
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    /// LLM 请求开始
    #[serde(rename = "llm_start")]
    LlmStart {
        model: String,
        message_count: usize,
        round: usize,
    },
    /// LLM 请求完成
    #[serde(rename = "llm_done")]
    LlmDone {
        model: String,
        content_len: usize,
        tool_call_count: usize,
        input_tokens: u64,
        output_tokens: u64,
        duration_ms: u64,
    },
    /// 工具调用开始
    #[serde(rename = "tool_start")]
    ToolStart {
        tool_name: String,
        round: usize,
    },
    /// 工具调用完成
    #[serde(rename = "tool_done")]
    ToolDone {
        tool_name: String,
        success: bool,
        duration_ms: u64,
    },
    /// 上下文压缩
    #[serde(rename = "context_compact")]
    ContextCompact {
        original_count: usize,
        compacted_count: usize,
        tier: String,
    },
    /// Token 预算警告
    #[serde(rename = "token_warning")]
    TokenWarning {
        accumulated: u64,
        budget: u64,
    },
    /// 工具被策略拒绝
    #[serde(rename = "tool_blocked")]
    ToolBlocked {
        tool_name: String,
        reason: String,
        agent_id: String,
    },
    /// 子代理已派发
    #[serde(rename = "subagent_spawned")]
    SubagentSpawned {
        batch_id: String,
        parent_agent_id: String,
        task_count: usize,
        model: String,
    },
    /// 子代理完成通知（异步模式）
    #[serde(rename = "subagent_complete")]
    SubagentComplete {
        batch_id: String,
        parent_agent_id: String,
        parent_session_id: Option<String>,
        success_count: usize,
        fail_count: usize,
        summary: String,
    },
    /// Webhook 触发
    #[serde(rename = "webhook_received")]
    WebhookReceived {
        job_id: String,
        job_name: String,
        payload_bytes: usize,
    },
    /// Poll 检测到变化
    #[serde(rename = "poll_changed")]
    PollChanged {
        job_id: String,
        job_name: String,
    },
    /// 错误
    #[serde(rename = "error")]
    Error {
        message: String,
    },
}

/// 事件广播器
///
/// 通过 channel 将事件推送给所有订阅者。
/// Orchestrator 持有 sender，前端通过 Tauri event 接收。
pub struct EventBroadcaster {
    tx: tokio::sync::broadcast::Sender<AgentEvent>,
}

impl EventBroadcaster {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(capacity);
        Self { tx }
    }

    /// 发布事件
    pub fn emit(&self, event: AgentEvent) {
        let _ = self.tx.send(event);
    }

    /// 获取订阅 receiver
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<AgentEvent> {
        self.tx.subscribe()
    }

    /// 当前订阅者数
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for EventBroadcaster {
    fn default() -> Self {
        Self::new(100)
    }
}
