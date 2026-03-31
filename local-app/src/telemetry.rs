//! 遥测模块 — 错误上报与心跳
//!
//! 所有操作均为后台异步执行，永远不会阻塞调用方，
//! 遥测失败只记录日志，不影响应用正常功能。

use serde_json::json;
use sqlx::SqlitePool;
use std::sync::OnceLock;

/// 遥测服务端基地址
const TELEMETRY_BASE: &str = "https://zys-openclaw.com/api/v1/telemetry";

/// 全局数据库连接池（由 init 设置，供无 pool 参数的场景使用）
static GLOBAL_POOL: OnceLock<SqlitePool> = OnceLock::new();

/// 初始化全局连接池（在 main 中调用一次）
pub fn init(pool: SqlitePool) {
    let _ = GLOBAL_POOL.set(pool);
}

/// 获取全局连接池（供其他模块使用）
pub fn get_global_pool() -> Option<&'static SqlitePool> {
    GLOBAL_POOL.get()
}

fn global_pool() -> Option<&'static SqlitePool> {
    GLOBAL_POOL.get()
}

/// 心跳间隔（秒）
const HEARTBEAT_INTERVAL_SECS: u64 = 300; // 5 分钟

/// 应用版本（从 Cargo.toml 编译时注入）
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// 当前平台
fn platform() -> &'static str {
    std::env::consts::OS
}

// ─── Device ID ───────────────────────────────────────────────

/// 获取或创建设备 ID（持久化到 settings 表）
/// 公开的持久化 device_id 获取（供 Bridge 等模块共享）
pub async fn get_or_create_device_id_public(pool: &SqlitePool) -> String {
    get_or_create_device_id(pool).await
}

async fn get_or_create_device_id(pool: &SqlitePool) -> String {
    // 读取已有的 device_id
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT value FROM settings WHERE key = 'device_id'"
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    if let Some(id) = existing {
        if !id.is_empty() {
            return id;
        }
    }

    // 生成新的 UUID 并保存
    let new_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let _ = sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES ('device_id', ?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"
    )
    .bind(&new_id)
    .bind(now)
    .execute(pool)
    .await;

    new_id
}

/// 获取 userId（从 settings 表的 profile.nickname 或 user_id）
async fn get_user_id(pool: &SqlitePool) -> String {
    // 按优先级查找用户标识：user_id > user_email > user_name > profile.nickname
    for key in &["user_id", "user_email", "user_name", "profile.nickname"] {
        if let Ok(Some(val)) = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = ?"
        ).bind(key).fetch_optional(pool).await {
            if !val.is_empty() {
                return val;
            }
        }
    }
    "anonymous".to_string()
}

// ─── Error Reporter ──────────────────────────────────────────

/// 上报错误（使用全局连接池，无需传 pool）
///
/// 适用于没有 pool 引用的调用点（如 LlmClient::call_stream）。
/// 如果全局 pool 尚未初始化则静默跳过。
pub fn report_error_global(
    error_type: &str,
    error_code: &str,
    message: &str,
    context: serde_json::Value,
) {
    if let Some(pool) = global_pool() {
        report_error(error_type, error_code, message, context, pool.clone());
    }
}

/// 上报错误到遥测服务
///
/// Fire-and-forget：在后台 tokio 任务中发送，调用方不阻塞。
/// 参数：
/// - `error_type`: "llm_error", "tool_error", "network", "auth", "crash"
/// - `error_code`: "401", "429", "timeout" 等
/// - `message`: 错误描述
/// - `context`: 附加上下文（provider, model, sessionId 等）
/// - `pool`: 数据库连接池
pub fn report_error(
    error_type: &str,
    error_code: &str,
    message: &str,
    context: serde_json::Value,
    pool: SqlitePool,
) {
    let error_type = error_type.to_string();
    let error_code = error_code.to_string();
    let message = message.to_string();

    tokio::spawn(async move {
        if let Err(e) = report_error_inner(&error_type, &error_code, &message, &context, &pool).await {
            log::debug!("遥测上报失败（忽略）: {}", e);
        }
    });
}

/// 内部实现：发送错误上报请求
async fn report_error_inner(
    error_type: &str,
    error_code: &str,
    message: &str,
    context: &serde_json::Value,
    pool: &SqlitePool,
) -> Result<(), String> {
    let device_id = get_or_create_device_id(pool).await;
    let user_id = get_user_id(pool).await;

    let payload = json!({
        "userId": user_id,
        "deviceId": device_id,
        "platform": platform(),
        "appVersion": app_version(),
        "errorType": error_type,
        "errorCode": error_code,
        "message": message,
        "context": context,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    let client = crate::agent::llm::build_proxied_client(5, 10);
    let url = format!("{}/report", TELEMETRY_BASE);
    let resp = client.post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("HTTP 发送失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("服务端返回 {}", resp.status()));
    }
    Ok(())
}

// ─── Heartbeat ───────────────────────────────────────────────

/// 启动心跳后台任务（每 5 分钟发送一次）
///
/// 在 tokio 后台任务中运行，优雅处理所有错误。
pub fn start_heartbeat(pool: SqlitePool) {
    tokio::spawn(async move {
        // 首次等待 10 秒再发第一次心跳
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

        loop {
            match send_heartbeat(&pool).await {
                Ok(_) => log::info!("遥测心跳发送成功"),
                Err(e) => log::warn!("遥测心跳发送失败: {}", e),
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(HEARTBEAT_INTERVAL_SECS)).await;
        }
    });
}

/// 内部实现：发送一次心跳
async fn send_heartbeat(pool: &SqlitePool) -> Result<(), String> {
    let device_id = get_or_create_device_id(pool).await;
    let user_id = get_user_id(pool).await;

    // 收集统计数据
    let agent_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agents")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    let session_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_sessions")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    let last_model: String = sqlx::query_scalar::<_, String>(
        "SELECT value FROM settings WHERE key = 'last_model'"
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or_default();

    let payload = json!({
        "userId": user_id,
        "deviceId": device_id,
        "platform": platform(),
        "appVersion": app_version(),
        "agentCount": agent_count,
        "sessionCount": session_count,
        "lastModel": last_model,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    let client = crate::agent::llm::build_proxied_client(5, 10);
    let url = format!("{}/heartbeat", TELEMETRY_BASE);
    let resp = client.post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("HTTP 发送失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("服务端返回 {}", resp.status()));
    }

    log::debug!("心跳已发送: device={}, agents={}, sessions={}", device_id, agent_count, session_count);
    Ok(())
}

// ─── 辅助函数：从错误文本提取错误代码 ─────────────────────────

/// 从 LLM 错误文本中提取 HTTP 状态码或错误类型标识
pub fn extract_error_code(error: &str) -> &str {
    let lower = error.to_lowercase();
    if lower.contains("401") || lower.contains("unauthorized") { return "401"; }
    if lower.contains("403") || lower.contains("forbidden") { return "403"; }
    if lower.contains("429") || lower.contains("rate limit") { return "429"; }
    if lower.contains("402") || lower.contains("payment") || lower.contains("quota") { return "402"; }
    if lower.contains("404") { return "404"; }
    if lower.contains("500") { return "500"; }
    if lower.contains("502") { return "502"; }
    if lower.contains("503") { return "503"; }
    if lower.contains("timeout") || lower.contains("超时") { return "timeout"; }
    if lower.contains("connection") || lower.contains("dns") || lower.contains("连接") { return "connection"; }
    "unknown"
}

/// 从 LLM 错误文本中提取错误类型
pub fn extract_error_type(error: &str) -> &str {
    let code = extract_error_code(error);
    match code {
        "401" | "403" => "auth",
        "429" | "402" => "llm_error",
        "timeout" | "connection" => "network",
        _ => "llm_error",
    }
}
