//! 内嵌 HTTP API 网关
//!
//! 基于 hyper 0.14（reqwest 已依赖）实现轻量 REST API。
//! 用途：外部工具调用、webhook、健康检查、跨应用集成。

use std::sync::Arc;
use std::convert::Infallible;
use std::collections::HashMap;
use std::sync::Mutex;

/// API 网关配置
#[derive(Debug, Clone)]
pub struct ApiGatewayConfig {
    pub port: u16,
    pub bind_address: String,
    pub api_key: Option<String>,
}

impl Default for ApiGatewayConfig {
    fn default() -> Self {
        Self {
            port: 0,
            bind_address: "127.0.0.1".to_string(),
            api_key: None,
        }
    }
}

// ─── Webhook 预认证安全常量 ───────────────────────────────
/// 预认证阶段最大请求体（64KB）
const WEBHOOK_PRE_AUTH_MAX_BYTES: usize = 64 * 1024;
/// 预认证阶段读取超时（5 秒）
const WEBHOOK_PRE_AUTH_TIMEOUT_MS: u64 = 5_000;
/// Webhook 速率限制：每个 token 每分钟最多 30 次
const WEBHOOK_RATE_LIMIT_PER_MIN: u32 = 30;
/// HMAC 签名时间窗口（10 分钟）
const WEBHOOK_SIGNATURE_WINDOW_SECS: i64 = 600;

/// 网关共享状态
pub struct GatewayState {
    pub config: ApiGatewayConfig,
    pub pool: sqlx::SqlitePool,
    /// Agent 编排器（可选，用于 /message 端点）
    pub orchestrator: Option<std::sync::Arc<crate::agent::Orchestrator>>,
    /// 调度器唤醒信号（用于 webhook 触发）
    pub scheduler_notify: Option<std::sync::Arc<tokio::sync::Notify>>,
    /// Webhook 速率限制器（token → (count, window_start)）
    pub webhook_rate_limiter: Mutex<HashMap<String, (u32, i64)>>,
}

/// 启动 API 网关
pub async fn start_api_gateway(state: Arc<GatewayState>) -> Result<(), String> {
    use hyper::service::{make_service_fn, service_fn};
    use hyper::Server;

    if state.config.port == 0 {
        return Ok(());
    }

    let addr = format!("{}:{}", state.config.bind_address, state.config.port)
        .parse::<std::net::SocketAddr>()
        .map_err(|e| format!("无效地址: {}", e))?;

    let state_clone = state.clone();
    let make_svc = make_service_fn(move |_| {
        let st = state_clone.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                handle_request(req, st.clone())
            }))
        }
    });

    log::info!("API 网关启动: http://{}", addr);

    Server::bind(&addr)
        .serve(make_svc)
        .await
        .map_err(|e| format!("API 网关错误: {}", e))
}

async fn handle_request(
    req: hyper::Request<hyper::Body>,
    state: Arc<GatewayState>,
) -> Result<hyper::Response<hyper::Body>, Infallible> {
    use hyper::{StatusCode, Method};

    // API Key 认证（静态文件不需要认证）
    let is_api_request = req.uri().path().starts_with("/api/") || req.uri().path().starts_with("/webhook/");
    if is_api_request {
        if let Some(ref expected_key) = state.config.api_key {
            let auth = req.headers().get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "));
            if auth != Some(expected_key.as_str()) {
                return Ok(json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Unauthorized"})));
            }
        }
    }

    let path = req.uri().path().to_string();
    let method = req.method().clone();

    match (method, path.as_str()) {
        (Method::GET, "/api/v1/health") => {
            Ok(json_response(StatusCode::OK, serde_json::json!({
                "status": "ok",
                "version": env!("CARGO_PKG_VERSION"),
                "timestamp": chrono::Utc::now().to_rfc3339(),
            })))
        }

        (Method::GET, "/api/v1/agents") => {
            match list_agents_handler(&state).await {
                Ok(data) => Ok(json_response(StatusCode::OK, data)),
                Err(e) => Ok(json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e}))),
            }
        }

        (Method::POST, "/api/v1/message") => {
            let body_bytes = hyper::body::to_bytes(req.into_body()).await.unwrap_or_default();
            match send_message_handler(&state, &body_bytes).await {
                Ok(data) => Ok(json_response(StatusCode::OK, data)),
                Err(e) => Ok(json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": e}))),
            }
        }

        (Method::GET, p) if p.starts_with("/api/v1/token-stats/") => {
            let agent_id = p.strip_prefix("/api/v1/token-stats/").unwrap_or("");
            match token_stats_handler(&state, agent_id).await {
                Ok(data) => Ok(json_response(StatusCode::OK, data)),
                Err(e) => Ok(json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e}))),
            }
        }

        // ── 扩展 API（CLI/Web/TUI 用）──────────────────────────

        // 会话列表
        (Method::GET, p) if p.starts_with("/api/v1/sessions/") => {
            let agent_id = p.strip_prefix("/api/v1/sessions/").unwrap_or("");
            match list_sessions_handler(&state, agent_id).await {
                Ok(data) => Ok(json_response(StatusCode::OK, data)),
                Err(e) => Ok(json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e}))),
            }
        }

        // 会话消息
        (Method::GET, p) if p.starts_with("/api/v1/messages/") => {
            let session_id = p.strip_prefix("/api/v1/messages/").unwrap_or("");
            match list_messages_handler(&state, session_id).await {
                Ok(data) => Ok(json_response(StatusCode::OK, data)),
                Err(e) => Ok(json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e}))),
            }
        }

        // 诊断
        (Method::GET, "/api/v1/doctor") => {
            let results = crate::agent::doctor::run_diagnostics(&state.pool).await;
            Ok(json_response(StatusCode::OK, serde_json::json!({"results": results})))
        }

        // 诊断自动修复
        (Method::POST, "/api/v1/doctor/fix") => {
            let fixes = crate::agent::doctor::auto_fix(&state.pool).await;
            Ok(json_response(StatusCode::OK, serde_json::json!({"fixes": fixes})))
        }

        // 设置读取
        (Method::GET, p) if p.starts_with("/api/v1/settings/") => {
            let key = p.strip_prefix("/api/v1/settings/").unwrap_or("");
            let value: Option<String> = sqlx::query_scalar(
                "SELECT value FROM settings WHERE key = ?"
            ).bind(key).fetch_optional(&state.pool).await.ok().flatten();
            Ok(json_response(StatusCode::OK, serde_json::json!({"key": key, "value": value})))
        }

        // 设置写入
        (Method::POST, "/api/v1/settings") => {
            let body_bytes = hyper::body::to_bytes(req.into_body()).await.unwrap_or_default();
            let payload: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap_or_default();
            let key = payload["key"].as_str().unwrap_or("");
            let value = payload["value"].as_str().unwrap_or("");
            if !key.is_empty() {
                let _ = sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)")
                    .bind(key).bind(value).execute(&state.pool).await;
            }
            Ok(json_response(StatusCode::OK, serde_json::json!({"ok": true})))
        }

        // 搜索消息
        (Method::GET, p) if p.starts_with("/api/v1/search/") => {
            let rest = p.strip_prefix("/api/v1/search/").unwrap_or("");
            let parts: Vec<&str> = rest.splitn(2, '/').collect();
            let (agent_id, query) = if parts.len() == 2 { (parts[0], parts[1]) } else { ("", rest) };
            let query = urlencoding::decode(query).unwrap_or_default().to_string();
            match search_messages_handler(&state, agent_id, &query).await {
                Ok(data) => Ok(json_response(StatusCode::OK, data)),
                Err(e) => Ok(json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e}))),
            }
        }

        // 压缩会话
        (Method::POST, p) if p.starts_with("/api/v1/compact/") => {
            let rest = p.strip_prefix("/api/v1/compact/").unwrap_or("");
            let parts: Vec<&str> = rest.splitn(2, '/').collect();
            if parts.len() == 2 {
                let (agent_id, session_id) = (parts[0], parts[1]);
                match compact_handler(&state, agent_id, session_id).await {
                    Ok(data) => Ok(json_response(StatusCode::OK, serde_json::json!({"result": data}))),
                    Err(e) => Ok(json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": e}))),
                }
            } else {
                Ok(json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": "需要 /api/v1/compact/{agentId}/{sessionId}"})))
            }
        }

        // Agent 创建
        (Method::POST, "/api/v1/agents") => {
            let body_bytes = hyper::body::to_bytes(req.into_body()).await.unwrap_or_default();
            let payload: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap_or_default();
            let name = payload["name"].as_str().unwrap_or("New Agent");
            let model = payload["model"].as_str().unwrap_or("gpt-4o");
            let prompt = payload["systemPrompt"].as_str().unwrap_or("你是一个有用的AI助手。");
            if let Some(ref orch) = state.orchestrator {
                match orch.register_agent(name, prompt, model).await {
                    Ok(agent) => Ok(json_response(StatusCode::OK, serde_json::json!({"id": agent.id, "name": agent.name}))),
                    Err(e) => Ok(json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": e}))),
                }
            } else {
                Ok(json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "orchestrator not available"})))
            }
        }

        // Agent 删除
        (Method::DELETE, p) if p.starts_with("/api/v1/agents/") => {
            let agent_id = p.strip_prefix("/api/v1/agents/").unwrap_or("");
            if let Some(ref orch) = state.orchestrator {
                match orch.delete_agent(agent_id).await {
                    Ok(_) => Ok(json_response(StatusCode::OK, serde_json::json!({"ok": true}))),
                    Err(e) => Ok(json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": e}))),
                }
            } else {
                Ok(json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "orchestrator not available"})))
            }
        }

        // 备份
        (Method::POST, "/api/v1/backup") => {
            match backup_handler(&state).await {
                Ok(data) => Ok(json_response(StatusCode::OK, data)),
                Err(e) => Ok(json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e}))),
            }
        }

        // 上下文使用情况
        (Method::GET, p) if p.starts_with("/api/v1/context/") => {
            let rest = p.strip_prefix("/api/v1/context/").unwrap_or("");
            let parts: Vec<&str> = rest.splitn(2, '/').collect();
            if parts.len() == 2 {
                let (agent_id, session_id) = (parts[0], parts[1]);
                match context_usage_handler(&state, agent_id, session_id).await {
                    Ok(data) => Ok(json_response(StatusCode::OK, data)),
                    Err(e) => Ok(json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e}))),
                }
            } else {
                Ok(json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": "需要 /api/v1/context/{agentId}/{sessionId}"})))
            }
        }

        // Webhook 触发端点：POST /webhook/{token}（预认证加固）
        (Method::POST, p) if p.starts_with("/webhook/") => {
            let token = p.strip_prefix("/webhook/").unwrap_or("").to_string();

            // 提取签名相关 header（在 body 消费前）
            let req_sig_header = req.headers().get("x-webhook-signature")
                .and_then(|v| v.to_str().ok()).map(String::from);
            let req_ts_header = req.headers().get("x-webhook-timestamp")
                .and_then(|v| v.to_str().ok()).map(String::from);

            // ── 预认证阶段 1: Token 格式校验（在读取 body 之前）──
            if token.is_empty() || token.len() > 128 || !token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                return Ok(json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": "无效的 webhook token 格式"})));
            }

            // ── 预认证阶段 2: 速率限制（在读取 body 之前）──
            {
                let now = chrono::Utc::now().timestamp();
                let mut limiter = state.webhook_rate_limiter.lock().unwrap_or_else(|e| e.into_inner());
                let entry = limiter.entry(token.clone()).or_insert((0, now));
                if now - entry.1 > 60 {
                    // 重置窗口
                    *entry = (1, now);
                } else {
                    entry.0 += 1;
                    if entry.0 > WEBHOOK_RATE_LIMIT_PER_MIN {
                        log::warn!("Webhook 速率限制: token={} count={}", token, entry.0);
                        return Ok(json_response(StatusCode::TOO_MANY_REQUESTS, serde_json::json!({"error": "速率限制：请求过于频繁"})));
                    }
                }
                // 清理过期条目（防止内存泄漏）
                if limiter.len() > 4096 {
                    limiter.retain(|_, (_, ts)| now - *ts < 120);
                }
            }

            // ── 预认证阶段 3: 限制 body 大小（防止大请求体 DoS）──
            let content_length = req.headers().get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<usize>().ok());
            if let Some(len) = content_length {
                if len > WEBHOOK_PRE_AUTH_MAX_BYTES {
                    return Ok(json_response(StatusCode::PAYLOAD_TOO_LARGE, serde_json::json!({"error": "请求体过大"})));
                }
            }

            // 带超时读取 body
            let body_result = tokio::time::timeout(
                std::time::Duration::from_millis(WEBHOOK_PRE_AUTH_TIMEOUT_MS),
                hyper::body::to_bytes(req.into_body())
            ).await;

            let body_bytes = match body_result {
                Ok(Ok(bytes)) => {
                    if bytes.len() > WEBHOOK_PRE_AUTH_MAX_BYTES {
                        return Ok(json_response(StatusCode::PAYLOAD_TOO_LARGE, serde_json::json!({"error": "请求体过大"})));
                    }
                    bytes
                }
                Ok(Err(_)) => return Ok(json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": "读取请求体失败"}))),
                Err(_) => return Ok(json_response(StatusCode::REQUEST_TIMEOUT, serde_json::json!({"error": "读取请求体超时"}))),
            };

            // ── 预认证阶段 4: HMAC 签名验证（如配置了 secret）──
            // 先查询任务的 secret，如果有则验证签名
            let secret_row: Option<(Option<String>,)> = sqlx::query_as(
                "SELECT webhook_secret FROM cron_jobs WHERE schedule_kind = 'webhook' AND cron_expr = ? AND enabled = 1"
            )
            .bind(&token)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();

            if let Some((Some(ref secret),)) = secret_row {
                if !secret.is_empty() {
                    let sig_header = req_sig_header.as_deref();
                    let ts_header = req_ts_header.as_deref();
                    if let Err(e) = verify_webhook_signature(secret, &body_bytes, sig_header, ts_header) {
                        log::warn!("Webhook 签名验证失败: token={} err={}", token, e);
                        return Ok(json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": format!("签名验证失败: {}", e)})));
                    }
                }
            }

            match webhook_trigger_handler(&state, &token, &body_bytes).await {
                Ok(data) => Ok(json_response(StatusCode::OK, data)),
                Err(e) => Ok(json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": e}))),
            }
        }

        // CORS preflight
        (Method::OPTIONS, _) => {
            Ok(hyper::Response::builder()
                .status(StatusCode::NO_CONTENT)
                .header("Access-Control-Allow-Origin", "*")
                .header("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS")
                .header("Access-Control-Allow-Headers", "Content-Type, Authorization")
                .header("Access-Control-Max-Age", "86400")
                .body(hyper::Body::empty())
                .unwrap())
        }

        // Web UI 静态文件服务
        (Method::GET, p) if !p.starts_with("/api/") && !p.starts_with("/webhook/") => {
            serve_web_ui(p).await
        }

        _ => {
            Ok(json_response(StatusCode::NOT_FOUND, serde_json::json!({
                "error": "Not Found",
                "path": path,
                "available_endpoints": [
                    "GET  /              — Web UI",
                    "GET  /api/v1/health",
                    "GET  /api/v1/agents",
                    "POST /api/v1/message",
                    "GET  /api/v1/token-stats/:agentId",
                    "POST /webhook/:token",
                ]
            })))
        }
    }
}

/// Web UI 静态文件服务
///
/// 从前端 build 产物目录提供文件服务。
/// 支持 SPA 路由（非文件路径都返回 index.html）。
async fn serve_web_ui(path: &str) -> Result<hyper::Response<hyper::Body>, Infallible> {
    use hyper::StatusCode;

    // 查找前端 build 目录
    let web_dir = find_web_ui_dir();
    let web_dir = match web_dir {
        Some(d) => d,
        None => {
            // 无前端 dist 目录时，返回内嵌的轻量 Web UI
            return Ok(hyper::Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/html; charset=utf-8")
                .header("Access-Control-Allow-Origin", "*")
                .body(hyper::Body::from(EMBEDDED_WEB_UI))
                .unwrap());
        }
    };

    // 映射路径
    let file_path = if path == "/" || path.is_empty() {
        web_dir.join("index.html")
    } else {
        let clean = path.trim_start_matches('/');
        // 安全：禁止路径遍历
        if clean.contains("..") {
            return Ok(hyper::Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(hyper::Body::from("Forbidden")).unwrap());
        }
        web_dir.join(clean)
    };

    // 文件存在 → 返回
    if file_path.is_file() {
        match tokio::fs::read(&file_path).await {
            Ok(bytes) => {
                let mime = guess_mime(&file_path);
                return Ok(hyper::Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", mime)
                    .header("Cache-Control", if mime.starts_with("text/html") { "no-cache" } else { "public, max-age=31536000" })
                    .body(hyper::Body::from(bytes))
                    .unwrap());
            }
            Err(_) => {}
        }
    }

    // SPA fallback：非 API、非静态文件 → 返回 index.html
    let index = web_dir.join("index.html");
    if index.is_file() {
        if let Ok(bytes) = tokio::fs::read(&index).await {
            return Ok(hyper::Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/html; charset=utf-8")
                .header("Cache-Control", "no-cache")
                .body(hyper::Body::from(bytes))
                .unwrap());
        }
    }

    Ok(hyper::Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(hyper::Body::from("Not Found")).unwrap())
}

/// 查找前端 build 目录
fn find_web_ui_dir() -> Option<std::path::PathBuf> {
    // 1. 环境变量指定
    if let Ok(dir) = std::env::var("XIANZHU_WEB_DIR") {
        let p = std::path::PathBuf::from(dir);
        if p.join("index.html").exists() { return Some(p); }
    }

    // 2. 相对于可执行文件的 ../web/dist
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let web = parent.join("../Resources/web/dist");
            if web.join("index.html").exists() { return Some(web); }
            // 开发模式
            let dev = parent.join("../../frontend/dist");
            if dev.join("index.html").exists() { return Some(dev); }
        }
    }

    // 3. 用户数据目录
    if let Some(data) = dirs::data_dir() {
        let p = data.join("com.xianzhu.app/web/dist");
        if p.join("index.html").exists() { return Some(p); }
    }

    // 4. 当前目录附近
    let candidates = [
        "../frontend/dist",
        "frontend/dist",
        "web/dist",
        "dist",
    ];
    for c in &candidates {
        let p = std::path::PathBuf::from(c);
        if p.join("index.html").exists() { return Some(p); }
    }

    None
}

/// 猜测 MIME 类型
fn guess_mime(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "webp" => "image/webp",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// 内嵌 Web UI（编译时嵌入）
const EMBEDDED_WEB_UI: &str = include_str!("web/index.html");

/// 无前端文件时的 fallback HTML
const WEB_UI_FALLBACK_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>XianZhu Web UI</title>
  <style>
    body { font-family: -apple-system, sans-serif; max-width: 600px; margin: 80px auto; text-align: center; color: #333; }
    h1 { font-size: 2em; }
    .hint { color: #888; margin-top: 2em; font-size: 0.9em; }
    code { background: #f0f0f0; padding: 2px 8px; border-radius: 4px; }
  </style>
</head>
<body>
  <h1>🐾 衔烛</h1>
  <p>API Gateway is running. Web UI files not found.</p>
  <div class="hint">
    <p>To enable Web UI, build the frontend and place it where the gateway can find it:</p>
    <p><code>cd frontend && npm run build</code></p>
    <p>Or set <code>XIANZHU_WEB_DIR=/path/to/dist</code></p>
  </div>
  <hr>
  <p><a href="/api/v1/health">API Health</a> · <a href="/api/v1/agents">Agents</a></p>
</body>
</html>"#;

fn json_response(status: hyper::StatusCode, body: serde_json::Value) -> hyper::Response<hyper::Body> {
    hyper::Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .header("Access-Control-Allow-Origin", "*")
        // 安全头
        .header("X-Content-Type-Options", "nosniff")
        .header("X-Frame-Options", "DENY")
        .header("Cache-Control", "no-store")
        .body(hyper::Body::from(serde_json::to_string(&body).unwrap_or_default()))
        .unwrap_or_else(|_| hyper::Response::new(hyper::Body::empty()))
}

async fn list_agents_handler(state: &GatewayState) -> Result<serde_json::Value, String> {
    let rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT id, name, model FROM agents ORDER BY created_at DESC"
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| format!("查询失败: {}", e))?;

    let agents: Vec<serde_json::Value> = rows.iter().map(|(id, name, model)| {
        serde_json::json!({"id": id, "name": name, "model": model})
    }).collect();

    Ok(serde_json::json!({"agents": agents, "count": agents.len()}))
}

/// POST /api/v1/message — 发送消息并获取回复
async fn send_message_handler(state: &GatewayState, body: &[u8]) -> Result<serde_json::Value, String> {
    let payload: serde_json::Value = serde_json::from_slice(body)
        .map_err(|e| format!("无效 JSON: {}", e))?;

    let message = payload["message"].as_str().ok_or("缺少 message")?;
    let session_id = payload["sessionId"].as_str().ok_or("缺少 sessionId")?;

    // agent_id 可选 — 如果不传，通过路由自动选择
    let agent_id = if let Some(id) = payload["agentId"].as_str() {
        id.to_string()
    } else {
        let sender = payload["senderId"].as_str().unwrap_or("api-anonymous");
        let channel = payload["channel"].as_str().unwrap_or("api");
        let router = crate::routing::Router::new(state.pool.clone());
        let route = router.resolve(channel, Some(sender)).await?;
        log::info!("API 路由: channel={} sender={} → agent={} ({})", channel, sender, route.agent_id, route.match_rule);
        route.agent_id
    };
    let agent_id = agent_id.as_str();

    let orchestrator = state.orchestrator.as_ref()
        .ok_or("编排器未初始化（API 网关需要传入 orchestrator）")?;

    // 查找 provider 配置（使用统一的 channels::find_provider）
    let agent_model = {
        let row: Option<(String,)> = sqlx::query_as("SELECT model FROM agents WHERE id = ?")
            .bind(agent_id).fetch_optional(&state.pool).await.ok().flatten();
        row.map(|r| r.0).unwrap_or_else(|| "gpt-4o".to_string())
    };
    let (api_type, api_key, base_url) = crate::channels::find_provider(&state.pool, &agent_model)
        .await
        .ok_or("没有可用的 LLM 供应商")?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let output_handle = tokio::spawn(async move {
        let mut output = String::new();
        while let Some(token) = rx.recv().await { output.push_str(&token); }
        output
    });

    let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };
    orchestrator.send_message_stream(
        agent_id, session_id, message,
        &api_key, &api_type, base_url_opt, tx, None,
    ).await.map_err(|e| format!("消息处理失败: {}", e))?;

    let response = output_handle.await.map_err(|e| format!("收集回复失败: {}", e))?;

    Ok(serde_json::json!({
        "agentId": agent_id,
        "sessionId": session_id,
        "response": response,
    }))
}

async fn token_stats_handler(state: &GatewayState, agent_id: &str) -> Result<serde_json::Value, String> {
    let since = chrono::Utc::now().timestamp_millis() - (7 * 86_400_000);
    let rows = sqlx::query_as::<_, (String, i64, i64, i64)>(
        "SELECT model, SUM(input_tokens), SUM(output_tokens), COUNT(*) FROM token_usage WHERE agent_id = ? AND created_at >= ? GROUP BY model"
    )
    .bind(agent_id).bind(since)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| format!("查询失败: {}", e))?;

    let models: Vec<serde_json::Value> = rows.iter().map(|(model, inp, out, calls)| {
        serde_json::json!({"model": model, "input": inp, "output": out, "calls": calls})
    }).collect();

    Ok(serde_json::json!({"agent_id": agent_id, "days": 7, "models": models}))
}

/// 时序安全的字符串比较（防止 timing attack）
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        // 即使长度不同，也做一次虚拟比较以保持时间恒定
        let _ = a.iter().zip(a.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y));
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// HMAC-SHA256 签名验证
///
/// 期望请求头: X-Webhook-Signature: sha256=<hex>
/// 签名 = HMAC-SHA256(secret, timestamp + "." + body)
/// 时间戳从 X-Webhook-Timestamp 头获取
fn verify_webhook_signature(secret: &str, body: &[u8], signature_header: Option<&str>, timestamp_header: Option<&str>) -> Result<(), String> {
    let sig_str = signature_header.ok_or("缺少 X-Webhook-Signature 头")?;
    let hex_sig = sig_str.strip_prefix("sha256=").ok_or("签名格式错误，需要 sha256=<hex>")?;

    // 验证时间窗口（防重放攻击）
    if let Some(ts_str) = timestamp_header {
        if let Ok(ts) = ts_str.parse::<i64>() {
            let now = chrono::Utc::now().timestamp();
            if (now - ts).abs() > WEBHOOK_SIGNATURE_WINDOW_SECS {
                return Err(format!("签名已过期（时间偏差 {} 秒）", (now - ts).abs()));
            }
        }
    }

    // 计算期望签名
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let signing_input = if let Some(ts) = timestamp_header {
        [ts.as_bytes(), b".", body].concat()
    } else {
        body.to_vec()
    };

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| "HMAC 密钥无效")?;
    mac.update(&signing_input);
    let expected = hex::encode(mac.finalize().into_bytes());

    // 时序安全比较
    if !constant_time_eq(expected.as_bytes(), hex_sig.as_bytes()) {
        return Err("签名验证失败".to_string());
    }

    Ok(())
}

// ── 扩展 API handlers ──────────────────────────────────────

async fn list_sessions_handler(state: &GatewayState, agent_id: &str) -> Result<serde_json::Value, String> {
    let sessions: Vec<(String, String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT id, title, created_at, last_message_at FROM chat_sessions WHERE agent_id = ? ORDER BY COALESCE(last_message_at, created_at) DESC LIMIT 50"
    ).bind(agent_id).fetch_all(&state.pool).await.map_err(|e| e.to_string())?;

    let list: Vec<serde_json::Value> = sessions.iter().map(|(id, title, created, last)| {
        serde_json::json!({"id": id, "title": title, "createdAt": created, "lastMessageAt": last})
    }).collect();
    Ok(serde_json::json!({"sessions": list, "count": list.len()}))
}

async fn list_messages_handler(state: &GatewayState, session_id: &str) -> Result<serde_json::Value, String> {
    let messages: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT seq, role, COALESCE(content, '') FROM chat_messages WHERE session_id = ? ORDER BY seq DESC LIMIT 50"
    ).bind(session_id).fetch_all(&state.pool).await.map_err(|e| e.to_string())?;

    let list: Vec<serde_json::Value> = messages.iter().rev().map(|(seq, role, content)| {
        serde_json::json!({"seq": seq, "role": role, "content": content})
    }).collect();
    Ok(serde_json::json!({"messages": list, "count": list.len()}))
}

async fn search_messages_handler(state: &GatewayState, agent_id: &str, query: &str) -> Result<serde_json::Value, String> {
    let like = format!("%{}%", query);
    let results: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT m.session_id, s.title, m.role, m.content FROM chat_messages m JOIN chat_sessions s ON m.session_id = s.id WHERE s.agent_id = ? AND m.content LIKE ? ORDER BY m.seq DESC LIMIT 20"
    ).bind(agent_id).bind(&like).fetch_all(&state.pool).await.map_err(|e| e.to_string())?;

    let list: Vec<serde_json::Value> = results.iter().map(|(sid, title, role, content)| {
        let snippet: String = content.chars().take(200).collect();
        serde_json::json!({"sessionId": sid, "sessionTitle": title, "role": role, "snippet": snippet})
    }).collect();
    Ok(serde_json::json!({"results": list, "count": list.len(), "query": query}))
}

async fn compact_handler(state: &GatewayState, agent_id: &str, session_id: &str) -> Result<String, String> {
    let orchestrator = state.orchestrator.as_ref().ok_or("编排器未初始化")?;
    let agent_model: String = sqlx::query_scalar("SELECT model FROM agents WHERE id = ?")
        .bind(agent_id).fetch_optional(&state.pool).await
        .map_err(|e| e.to_string())?
        .ok_or("Agent 不存在")?;

    let (api_type, api_key, base_url) = crate::channels::find_provider(&state.pool, &agent_model)
        .await.ok_or("无可用 Provider")?;
    let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };

    orchestrator.compact_session(agent_id, session_id, &api_key, &api_type, base_url_opt).await
}

async fn backup_handler(state: &GatewayState) -> Result<serde_json::Value, String> {
    let db_path = dirs::data_dir().unwrap_or_default().join("com.xianzhu.app/xianzhu.db");
    if !db_path.exists() { return Err("数据库不存在".into()); }

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let backup_dir = dirs::data_dir().unwrap_or_default().join("com.xianzhu.app/backups");
    let _ = std::fs::create_dir_all(&backup_dir);
    let backup_path = backup_dir.join(format!("xianzhu-{}.db", timestamp));

    sqlx::query(&format!("VACUUM INTO '{}'", backup_path.display()))
        .execute(&state.pool).await.map_err(|e| format!("备份失败: {}", e))?;

    let size = std::fs::metadata(&backup_path).map(|m| m.len()).unwrap_or(0);
    Ok(serde_json::json!({"path": backup_path.display().to_string(), "size_bytes": size}))
}

async fn context_usage_handler(state: &GatewayState, agent_id: &str, session_id: &str) -> Result<serde_json::Value, String> {
    let model: String = sqlx::query_scalar("SELECT model FROM agents WHERE id = ?")
        .bind(agent_id).fetch_optional(&state.pool).await
        .map_err(|e| e.to_string())?
        .ok_or("Agent 不存在")?;

    let msg_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_messages WHERE session_id = ?"
    ).bind(session_id).fetch_one(&state.pool).await.unwrap_or(0);

    let msg_chars: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM chat_messages WHERE session_id = ?"
    ).bind(session_id).fetch_one(&state.pool).await.unwrap_or(0);

    // 粗估 token（中英混合约 1.5 字符/token）
    let est_tokens = (msg_chars as f64 / 1.5) as usize;

    Ok(serde_json::json!({
        "model": model,
        "message_count": msg_count,
        "estimated_tokens": est_tokens,
    }))
}

/// POST /webhook/{token} — Webhook 触发器（含预认证加固）
///
/// 安全特性：
/// - 预认证阶段: token 格式校验、速率限制、body 大小限制
/// - HMAC-SHA256 签名验证（如配置了 webhook_secret）
/// - 时间窗口防重放（10 分钟）
/// - 时序安全比较（防 timing attack）
async fn webhook_trigger_handler(
    state: &GatewayState,
    token: &str,
    body: &[u8],
) -> Result<serde_json::Value, String> {
    if token.is_empty() {
        return Err("缺少 webhook token".to_string());
    }

    // 查找匹配的 webhook 任务（含 secret）
    let job_row: Option<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, name, webhook_secret FROM cron_jobs WHERE schedule_kind = 'webhook' AND cron_expr = ? AND enabled = 1"
    )
    .bind(token)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("查询 webhook 任务失败: {}", e))?;

    let (job_id, job_name, webhook_secret) = job_row.ok_or("未找到匹配的 webhook 任务或任务已禁用")?;

    // ── HMAC 签名验证（如果任务配置了 secret）──
    // 签名可选：有 secret 则强制验证，无 secret 则跳过（但记录警告）
    if let Some(ref secret) = webhook_secret {
        if !secret.is_empty() {
            // 从全局 body bytes 不可获取 header，此处由上层传递
            // 实际验证在 handle_request 层完成，这里做二次校验标记
            log::info!("Webhook {}: 已配置签名验证", job_name);
        }
    } else {
        log::warn!("Webhook {}: 未配置 webhook_secret，跳过签名验证（建议配置）", job_name);
    }

    // 解析 webhook payload
    let webhook_payload = if body.is_empty() {
        String::new()
    } else {
        String::from_utf8_lossy(body).to_string()
    };

    log::info!("Webhook 触发: {} ({}), payload {}字节", job_name, job_id, webhook_payload.len());

    // 创建手动触发 run 记录
    let run_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    if let Err(e) = sqlx::query(
        "INSERT INTO cron_runs (id, job_id, scheduled_at, started_at, status, trigger_source, attempt) VALUES (?, ?, ?, ?, 'queued', 'manual', 1)"
    )
    .bind(&run_id)
    .bind(&job_id)
    .bind(now)
    .bind(now)
    .execute(&state.pool)
    .await {
        log::error!("Webhook 创建 run 记录失败: {}", e);
    }

    // 更新 last_run_at + next_run_at（设为当前时间，让调度器立即执行）
    if let Err(e) = sqlx::query(
        "UPDATE cron_jobs SET last_run_at = ?, next_run_at = ?, updated_at = ? WHERE id = ?"
    )
    .bind(now).bind(now).bind(now).bind(&job_id)
    .execute(&state.pool)
    .await {
        log::error!("Webhook 更新任务状态失败: {}", e);
    }

    // 唤醒调度引擎执行
    if let Some(ref notify) = state.scheduler_notify {
        notify.notify_one();
        log::info!("Webhook 已唤醒调度引擎");
    } else {
        log::warn!("Webhook: 调度引擎未连接，任务已排队但可能延迟执行");
    }

    Ok(serde_json::json!({
        "status": "triggered",
        "jobId": job_id,
        "jobName": job_name,
        "runId": run_id,
        "webhookPayloadBytes": webhook_payload.len(),
    }))
}
