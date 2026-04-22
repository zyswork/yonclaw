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

/// OpenClaw #61077/#64790: 审批卡片渲染前 redact 敏感字段，避免密钥泄漏
///
/// 递归扫描 JSON value，对 key 名包含敏感关键字的字段将 value 替换为 "***REDACTED***"。
/// 对字符串值也扫描常见令牌 pattern（Bearer、sk-、ya29. 等）。
pub fn redact_secrets(value: &serde_json::Value) -> serde_json::Value {
    const SENSITIVE_KEYS: &[&str] = &[
        "api_key", "apikey", "api-key", "token", "access_token", "refresh_token",
        "password", "passwd", "secret", "credential", "bearer",
        "private_key", "client_secret",
        // 注：不把 session_id / auth 等高频合法字段列为敏感，避免审批卡不可读
    ];

    fn is_sensitive_key(k: &str) -> bool {
        let lower = k.to_lowercase();
        SENSITIVE_KEYS.iter().any(|sk| lower.contains(sk))
    }

    fn redact_string(s: &str) -> String {
        // 常见密钥前缀（含 OpenAI / Anthropic / Google / GitHub / Slack / AWS / Azure）
        let patterns = [
            ("sk-ant-", 20), ("sk-", 20), ("sk_", 20),
            ("ya29.", 30), ("AIza", 30),                     // Google OAuth / API key
            ("ghp_", 20), ("gho_", 20), ("ghs_", 20),        // GitHub classic
            ("github_pat_", 30), ("ghu_", 20), ("ghr_", 20), // GitHub fine-grained / user / refresh
            ("xoxb-", 20), ("xoxa-", 20), ("xoxp-", 20),     // Slack
            ("AKIA", 16),                                     // AWS access key
            ("ASIA", 16),                                     // AWS STS
            ("Bearer ", 20),
        ];
        let mut result = s.to_string();
        for (prefix, min_len) in &patterns {
            if let Some(pos) = result.find(prefix) {
                let remainder = &result[pos + prefix.len()..];
                if remainder.len() >= *min_len {
                    let end = pos + prefix.len() + remainder.chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
                        .map(|c| c.len_utf8()).sum::<usize>();
                    result = format!("{}{}***REDACTED***{}",
                        &result[..pos], prefix, &result[end..]);
                }
            }
        }
        result
    }

    match value {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                if is_sensitive_key(k) && v.is_string() {
                    out.insert(k.clone(), serde_json::Value::String("***REDACTED***".into()));
                } else {
                    out.insert(k.clone(), redact_secrets(v));
                }
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(redact_secrets).collect())
        }
        serde_json::Value::String(s) => {
            serde_json::Value::String(redact_string(s))
        }
        _ => value.clone(),
    }
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

    /// OpenClaw #66239: 超时过期某个 req（主动清理，避免后续用户点击误中）
    pub fn expire(&self, req_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(req_id);
        }
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
