//! Agent 生命周期事件系统
//!
//! 参考 IronClaw 的 6 点钩子设计。
//! 每个钩子点支持注册多个 handler，按优先级顺序执行。

use async_trait::async_trait;
use serde::Serialize;

/// 生命周期钩子点（参考 OpenClaw 25+ hooks 精简版）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookPoint {
    // ─── 消息流 ───
    /// 用户消息进入系统前（可修改/拒绝）
    BeforeInbound,
    /// 回复发出前（可修改内容）
    BeforeOutbound,

    // ─── Prompt 构建 ───
    /// System prompt 构建前（可注入额外上下文）
    BeforePromptBuild,

    // ─── LLM 调用 ───
    /// LLM 调用前（可修改 messages/tools）
    BeforeLlmCall,
    /// LLM 调用后（可观察 response）
    AfterLlmCall,

    // ─── 工具执行 ───
    /// 工具执行前（可修改参数/拒绝）
    BeforeToolCall,
    /// 工具执行后（可观察结果）
    AfterToolCall,

    // ─── 会话 ───
    /// 会话开始
    SessionStart,
    /// 会话结束
    SessionEnd,

    // ─── 上下文管理 ───
    /// 上下文压缩前
    BeforeCompaction,
    /// 上下文压缩后
    AfterCompaction,

    // ─── 子代理 ───
    /// 子代理派发
    SubagentSpawned,
    /// 子代理完成
    SubagentCompleted,
}

/// 钩子事件数据
#[derive(Debug, Clone, Serialize)]
pub struct HookEvent {
    /// 钩子点
    pub point: String,
    /// Agent ID
    pub agent_id: String,
    /// 会话 ID
    pub session_id: String,
    /// 事件载荷（JSON）
    pub payload: serde_json::Value,
}

/// 钩子处理器 trait
#[async_trait]
pub trait HookHandler: Send + Sync {
    /// 处理器名称
    fn name(&self) -> &str;

    /// 关注的钩子点
    fn points(&self) -> Vec<HookPoint>;

    /// 优先级（越小越先执行，默认 100）
    fn priority(&self) -> u32 { 100 }

    /// 处理事件。返回 Ok(None) 继续链路，Ok(Some(modified)) 修改载荷，Err 中断链路
    async fn handle(&self, event: &HookEvent) -> Result<Option<serde_json::Value>, String>;
}

/// 生命周期管理器
pub struct LifecycleManager {
    handlers: Vec<Box<dyn HookHandler>>,
}

impl LifecycleManager {
    pub fn new() -> Self {
        Self { handlers: Vec::new() }
    }

    /// 注册处理器
    pub fn register(&mut self, handler: Box<dyn HookHandler>) {
        log::info!("注册生命周期处理器: {} (points={:?})", handler.name(), handler.points());
        self.handlers.push(handler);
        // 按优先级排序
        self.handlers.sort_by_key(|h| h.priority());
    }

    /// 触发钩子点，返回可能被修改的载荷
    pub async fn emit(&self, point: HookPoint, event: &HookEvent) -> Result<Option<serde_json::Value>, String> {
        let mut modified_payload = None;

        for handler in &self.handlers {
            if !handler.points().contains(&point) {
                continue;
            }
            match handler.handle(event).await {
                Ok(None) => {} // 继续
                Ok(Some(new_payload)) => {
                    modified_payload = Some(new_payload);
                }
                Err(e) => {
                    log::warn!("钩子 {} 在 {:?} 返回错误: {}", handler.name(), point, e);
                    return Err(e);
                }
            }
        }

        Ok(modified_payload)
    }

    /// 触发只读钩子（忽略返回值）
    pub async fn notify(&self, point: HookPoint, event: &HookEvent) {
        for handler in &self.handlers {
            if !handler.points().contains(&point) {
                continue;
            }
            if let Err(e) = handler.handle(event).await {
                log::warn!("钩子 {} 通知失败: {}", handler.name(), e);
            }
        }
    }
}

/// 内置：日志记录钩子
pub struct LoggingHandler;

#[async_trait]
impl HookHandler for LoggingHandler {
    fn name(&self) -> &str { "logging" }

    fn points(&self) -> Vec<HookPoint> {
        vec![HookPoint::BeforeLlmCall, HookPoint::AfterLlmCall, HookPoint::BeforeToolCall, HookPoint::AfterToolCall]
    }

    fn priority(&self) -> u32 { 200 } // 低优先级，最后执行

    async fn handle(&self, event: &HookEvent) -> Result<Option<serde_json::Value>, String> {
        log::debug!("[Lifecycle] {} agent={} session={}", event.point, event.agent_id, event.session_id);
        Ok(None)
    }
}

/// 内置：Token 统计钩子
pub struct TokenTrackingHandler;

#[async_trait]
impl HookHandler for TokenTrackingHandler {
    fn name(&self) -> &str { "token_tracking" }

    fn points(&self) -> Vec<HookPoint> {
        vec![HookPoint::AfterLlmCall]
    }

    async fn handle(&self, event: &HookEvent) -> Result<Option<serde_json::Value>, String> {
        if let Some(usage) = event.payload.get("usage") {
            let input = usage["input_tokens"].as_u64().unwrap_or(0);
            let output = usage["output_tokens"].as_u64().unwrap_or(0);
            log::info!("[TokenTrack] agent={} input={} output={}", event.agent_id, input, output);
        }
        Ok(None)
    }
}
