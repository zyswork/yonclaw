//! 工具审批系统
//!
//! 当工具安全级别为 Approval 时，暂停执行等待用户确认。
//! 通过 Tauri event + oneshot channel 实现前后端通信。

use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::oneshot;

/// 审批请求
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub request_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub safety_level: String,
    pub timestamp: i64,
}

/// 审批结果
#[derive(Debug, Clone)]
pub enum ApprovalResult {
    Approved,
    Denied(String),
}

/// 审批管理器（全局单例）
pub struct ApprovalManager {
    /// 等待审批的请求：request_id → oneshot sender
    pending: Mutex<HashMap<String, oneshot::Sender<ApprovalResult>>>,
}

impl ApprovalManager {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// 创建审批请求，返回 receiver（调用方 await 等待结果）
    pub fn request(&self, req_id: &str) -> oneshot::Receiver<ApprovalResult> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(req_id.to_string(), tx);
        }
        rx
    }

    /// 用户批准
    pub fn approve(&self, req_id: &str) -> Result<(), String> {
        if let Ok(mut pending) = self.pending.lock() {
            if let Some(tx) = pending.remove(req_id) {
                let _ = tx.send(ApprovalResult::Approved);
                return Ok(());
            }
        }
        Err(format!("审批请求 {} 不存在或已过期", req_id))
    }

    /// 用户拒绝
    pub fn deny(&self, req_id: &str, reason: &str) -> Result<(), String> {
        if let Ok(mut pending) = self.pending.lock() {
            if let Some(tx) = pending.remove(req_id) {
                let _ = tx.send(ApprovalResult::Denied(reason.to_string()));
                return Ok(());
            }
        }
        Err(format!("审批请求 {} 不存在或已过期", req_id))
    }

    /// 清理所有待审批请求（拒绝全部）
    #[allow(dead_code)]
    pub fn cleanup_all(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            for (_, tx) in pending.drain() {
                let _ = tx.send(ApprovalResult::Denied("系统清理".to_string()));
            }
        }
    }

    /// 当前待审批数
    pub fn pending_count(&self) -> usize {
        self.pending.lock().map(|p| p.len()).unwrap_or(0)
    }
}
