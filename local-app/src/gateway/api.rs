//! 内嵌 HTTP API 网关
//!
//! 基于 hyper 0.14（reqwest 已依赖）实现轻量 REST API。
//! 用途：外部工具调用、webhook、健康检查、跨应用集成。

use std::sync::Arc;
use std::convert::Infallible;

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

/// 网关共享状态
pub struct GatewayState {
    pub config: ApiGatewayConfig,
    pub pool: sqlx::SqlitePool,
    /// Agent 编排器（可选，用于 /message 端点）
    pub orchestrator: Option<std::sync::Arc<crate::agent::Orchestrator>>,
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

    // API Key 认证
    if let Some(ref expected_key) = state.config.api_key {
        let auth = req.headers().get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        if auth != Some(expected_key.as_str()) {
            return Ok(json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Unauthorized"})));
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

        _ => {
            Ok(json_response(StatusCode::NOT_FOUND, serde_json::json!({
                "error": "Not Found",
                "path": path,
                "available_endpoints": [
                    "GET  /api/v1/health",
                    "GET  /api/v1/agents",
                    "POST /api/v1/message",
                    "GET  /api/v1/token-stats/:agentId",
                ]
            })))
        }
    }
}

fn json_response(status: hyper::StatusCode, body: serde_json::Value) -> hyper::Response<hyper::Body> {
    hyper::Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .header("Access-Control-Allow-Origin", "*")
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
