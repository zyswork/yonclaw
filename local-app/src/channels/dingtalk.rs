//! 钉钉机器人渠道
//!
//! 通过 HTTP 回调模式接收消息，REST API 发送回复。
//! 钉钉在用户发消息给机器人时，会 POST 到配置的回调 URL。
//! 桌面端启动一个小型 HTTP 服务器接收回调。
//!
//! 流程：
//! 1. 用 app_key + app_secret 获取 access_token
//! 2. 启动 HTTP 回调服务器（端口 7800+）
//! 3. 接收钉钉推送的消息事件
//! 4. 调用 orchestrator 处理消息
//! 5. 通过 REST API 回复消息

use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use crate::agent::Orchestrator;
use super::common::TokenCache;

/// 钉钉 API 基地址
const DINGTALK_API: &str = "https://oapi.dingtalk.com";
/// 钉钉新版 API 基地址
const DINGTALK_API_NEW: &str = "https://api.dingtalk.com";

/// 钉钉机器人配置
pub struct DingTalkConfig {
    /// 钉钉应用 AppKey
    pub app_key: String,
    /// 钉钉应用 AppSecret
    pub app_secret: String,
    /// 我们系统中的 Agent ID
    pub agent_id: String,
}

/// 运行时共享状态
struct AppState {
    app_key: String,
    app_secret: String,
    agent_id: String,
    pool: sqlx::SqlitePool,
    orchestrator: Arc<Orchestrator>,
    app_handle: tauri::AppHandle,
    /// 缓存的 access_token（通用 TokenCache）
    token_cache: Arc<TokenCache>,
}

/// 启动钉钉机器人
///
/// 由 ChannelManager 调用，通过 CancellationToken 控制生命周期。
/// 启动一个 HTTP 回调服务器，接收钉钉推送的消息。
pub async fn start_dingtalk(
    config: DingTalkConfig,
    pool: sqlx::SqlitePool,
    orchestrator: Arc<Orchestrator>,
    app_handle: tauri::AppHandle,
    cancel: CancellationToken,
) -> Result<(), String> {
    let app_key = config.app_key.clone();
    let agent_id = config.agent_id.clone();
    log::info!("钉钉: 启动回调服务器 (app_key: {}..., agent={})",
        &app_key[..app_key.len().min(10)], agent_id);

    // 初始获取 access_token 验证配置是否正确
    let client = reqwest::Client::new();
    let initial_token = get_access_token(&client, &config.app_key, &config.app_secret).await?;
    log::info!("钉钉: access_token 获取成功");

    // 钉钉 access_token 有效期 7200 秒
    let token_cache = TokenCache::with_initial(initial_token, 7200);

    let state = Arc::new(AppState {
        app_key: config.app_key,
        app_secret: config.app_secret,
        agent_id: config.agent_id,
        pool,
        orchestrator,
        app_handle,
        token_cache,
    });

    // 选择端口：7800 + 随机偏移（0-99），避免冲突
    let base_port: u16 = 7800;
    let mut port = base_port;
    let listener = loop {
        match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
            Ok(l) => break l,
            Err(_) if port < base_port + 100 => {
                port += 1;
                continue;
            }
            Err(e) => return Err(format!("无法绑定端口 {}-{}: {}", base_port, port, e)),
        }
    };

    log::info!("钉钉: HTTP 回调服务器监听 0.0.0.0:{}", port);
    log::info!("钉钉: 请在钉钉开发者后台配置回调 URL 为 http://<公网IP>:{}/callback", port);

    // 主循环：接受连接并处理
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                log::info!("钉钉: 收到取消信号，关闭回调服务器");
                return Ok(());
            }
            accept = listener.accept() => {
                match accept {
                    Ok((stream, addr)) => {
                        let state = state.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, addr, state).await {
                                log::warn!("钉钉: 处理连接失败 ({}): {}", addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        log::warn!("钉钉: 接受连接失败: {}", e);
                    }
                }
            }
        }
    }
}

/// 处理单个 HTTP 连接
async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: std::net::SocketAddr,
    state: Arc<AppState>,
) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = stream;
    let mut buf = vec![0u8; 65536];
    let n = stream.read(&mut buf).await
        .map_err(|e| format!("读取失败: {}", e))?;

    if n == 0 { return Ok(()); }

    let request = String::from_utf8_lossy(&buf[..n]);

    // 解析 HTTP 请求
    let (method, path, body) = parse_http_request(&request);

    log::debug!("钉钉: {} {} from {} (body: {}字节)", method, path, addr, body.len());

    let (status, response_body) = match (method.as_str(), path.as_str()) {
        ("POST", "/callback") => {
            // 钉钉消息回调
            match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(payload) => {
                    // 异步处理消息，立即返回 200
                    let s = state.clone();
                    tokio::spawn(async move {
                        handle_dingtalk_message(&payload, &s).await;
                    });
                    ("200 OK", r#"{"success":true}"#.to_string())
                }
                Err(e) => {
                    log::warn!("钉钉: 回调 JSON 解析失败: {}", e);
                    ("400 Bad Request", format!(r#"{{"error":"invalid json: {}"}}"#, e))
                }
            }
        }
        ("GET", "/health") => {
            ("200 OK", r#"{"status":"running","channel":"dingtalk"}"#.to_string())
        }
        _ => {
            ("404 Not Found", r#"{"error":"not found"}"#.to_string())
        }
    };

    // 发送 HTTP 响应
    let resp = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status, response_body.len(), response_body
    );
    stream.write_all(resp.as_bytes()).await
        .map_err(|e| format!("写响应失败: {}", e))?;

    Ok(())
}

/// 简单的 HTTP 请求解析
fn parse_http_request(raw: &str) -> (String, String, String) {
    let mut lines = raw.split("\r\n");
    let first_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();

    let method = parts.first().unwrap_or(&"GET").to_string();
    let path = parts.get(1).unwrap_or(&"/").to_string();

    // 找到空行后的 body
    let body = if let Some(pos) = raw.find("\r\n\r\n") {
        raw[pos + 4..].to_string()
    } else {
        String::new()
    };

    (method, path, body)
}

/// 处理钉钉消息事件
///
/// 钉钉机器人回调的 JSON 格式：
/// ```json
/// {
///   "conversationId": "xxx",
///   "atUsers": [{"dingtalkId": "xxx"}],
///   "chatbotCorpId": "xxx",
///   "chatbotUserId": "xxx",
///   "msgId": "xxx",
///   "senderNick": "用户昵称",
///   "isAdmin": false,
///   "senderStaffId": "xxx",
///   "sessionWebhookExpiredTime": 1234567890000,
///   "createAt": 1234567890000,
///   "senderCorpId": "xxx",
///   "conversationType": "1",  // 1=单聊 2=群聊
///   "senderId": "xxx",
///   "sessionWebhook": "https://oapi.dingtalk.com/robot/sendBySession?session=xxx",
///   "text": {"content": "消息内容"},
///   "msgtype": "text"
/// }
/// ```
async fn handle_dingtalk_message(
    payload: &serde_json::Value,
    state: &Arc<AppState>,
) {
    let msgtype = payload["msgtype"].as_str().unwrap_or("");

    // 目前只处理文本消息
    if msgtype != "text" {
        log::info!("钉钉: 暂不支持的消息类型: {}", msgtype);
        return;
    }

    let text = payload["text"]["content"].as_str().unwrap_or("").trim().to_string();
    if text.is_empty() {
        return;
    }

    let sender_nick = payload["senderNick"].as_str().unwrap_or("用户");
    let sender_id = payload["senderId"].as_str().unwrap_or("unknown");
    let conversation_id = payload["conversationId"].as_str().unwrap_or("");
    let conversation_type = payload["conversationType"].as_str().unwrap_or("1");
    let session_webhook = payload["sessionWebhook"].as_str().unwrap_or("");
    let msg_id = payload["msgId"].as_str().unwrap_or("");

    log::info!("钉钉: [{}] {}: {} (type={}, msgId={})",
        conversation_id, sender_nick,
        &text[..text.len().min(50)],
        conversation_type, msg_id);

    // 确定使用的 Agent
    let agent_id = if !state.agent_id.is_empty() {
        state.agent_id.clone()
    } else {
        let router = crate::routing::Router::new(state.orchestrator.pool().clone());
        let route = router.resolve("dingtalk", Some(sender_id)).await;
        match route {
            Ok(r) => r.agent_id,
            Err(_) => {
                let agents = state.orchestrator.list_agents().await.unwrap_or_default();
                match agents.into_iter().next() {
                    Some(a) => a.id,
                    None => {
                        log::warn!("钉钉: 无可用 Agent");
                        reply_via_webhook(session_webhook, "未配置 Agent，请在桌面端设置。").await;
                        return;
                    }
                }
            }
        }
    };

    let agent = match state.orchestrator.get_agent_cached(&agent_id).await {
        Ok(a) => a,
        Err(e) => {
            log::warn!("钉钉: 获取 Agent 失败: {}", e);
            reply_via_webhook(session_webhook, &format!("获取 Agent 失败: {}", e)).await;
            return;
        }
    };

    // 获取或创建 session
    let session_title = format!("[钉钉] {}", sender_nick);
    let session_id = get_or_create_session(
        &state.pool, &agent.id, conversation_id, &session_title
    ).await;

    // 查找 Provider
    let (api_type, api_key, base_url) = match super::find_provider(&state.pool, &agent.model).await {
        Some(info) => info,
        None => {
            reply_via_webhook(session_webhook, "未配置 LLM Provider，请在桌面端设置中添加。").await;
            return;
        }
    };

    use tauri::Manager;

    // 推送用户消息到前端
    let _ = state.app_handle.emit_all("chat-event", serde_json::json!({
        "type": "message",
        "sessionId": session_id,
        "role": "user",
        "content": text,
        "source": "dingtalk",
    }));

    // 推送"思考中"状态
    let _ = state.app_handle.emit_all("chat-event", serde_json::json!({
        "type": "thinking",
        "sessionId": session_id,
        "source": "dingtalk",
    }));

    // 流式调用 orchestrator
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let app_for_stream = state.app_handle.clone();
    let sid_for_stream = session_id.clone();
    let output_handle = tokio::spawn(async move {
        let mut output = String::new();
        while let Some(token) = rx.recv().await {
            output.push_str(&token);
            let _ = app_for_stream.emit_all("chat-event", serde_json::json!({
                "type": "token",
                "sessionId": sid_for_stream,
                "content": output.clone(),
                "source": "dingtalk",
            }));
        }
        output
    });

    let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };

    let result = state.orchestrator.send_message_stream(
        &agent.id, &session_id, &text,
        &api_key, &api_type, base_url_opt, tx, None,
    ).await;

    let streamed_output = output_handle.await.unwrap_or_default();

    let reply = match result {
        Ok(resp) => {
            let r = if resp.is_empty() { streamed_output } else { resp };
            if !r.is_empty() {
                // 优先通过 sessionWebhook 回复（无需 access_token，且自动回复到正确会话）
                if !session_webhook.is_empty() {
                    reply_via_webhook(session_webhook, &r).await;
                } else {
                    // 降级：通过 access_token + 群聊/单聊 API 回复
                    reply_via_api(state, conversation_id, conversation_type, &r).await;
                }
                log::info!("钉钉: 回复 [{}] {}字符", conversation_id, r.len());
            }
            r
        }
        Err(e) => {
            log::error!("钉钉: 处理失败: {}", e);
            let err_msg = format!("处理出错: {}", &e[..e.len().min(100)]);
            if !session_webhook.is_empty() {
                reply_via_webhook(session_webhook, &err_msg).await;
            }
            err_msg
        }
    };

    // 推送完成到前端
    let _ = state.app_handle.emit_all("chat-event", serde_json::json!({
        "type": "done",
        "sessionId": session_id,
        "role": "assistant",
        "content": reply,
        "source": "dingtalk",
    }));

    // Session 自动命名
    crate::memory::conversation::auto_name_session(
        &state.pool, &session_id, &text, &api_key, &api_type, base_url_opt,
    ).await;
}

/// 通过 sessionWebhook 回复消息（推荐方式）
///
/// 钉钉在消息回调中会携带 sessionWebhook，有效期内可直接回复到对应会话。
/// 无需 access_token，适合快速回复。
async fn reply_via_webhook(webhook_url: &str, text: &str) {
    if webhook_url.is_empty() {
        return;
    }

    let client = reqwest::Client::new();

    // 长消息分段发送（钉钉单条消息限制约 20000 字符）
    let max_len = 18000;
    let chunks: Vec<&str> = if text.len() > max_len {
        text.as_bytes()
            .chunks(max_len)
            .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
            .collect()
    } else {
        vec![text]
    };

    for chunk in chunks {
        if chunk.is_empty() { continue; }

        let body = serde_json::json!({
            "msgtype": "markdown",
            "markdown": {
                "title": "回复",
                "text": chunk,
            }
        });

        match client.post(webhook_url).json(&body).send().await {
            Ok(resp) => {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    if data["errcode"].as_i64() != Some(0) {
                        log::warn!("钉钉: webhook 回复失败: {} {}",
                            data["errcode"], data["errmsg"].as_str().unwrap_or(""));
                        // Markdown 失败则降级为纯文本
                        let fallback = serde_json::json!({
                            "msgtype": "text",
                            "text": { "content": chunk }
                        });
                        let _ = client.post(webhook_url).json(&fallback).send().await;
                    }
                }
            }
            Err(e) => log::warn!("钉钉: webhook 请求失败: {}", e),
        }
    }
}

/// 通过 API 回复消息（降级方式，需要 access_token）
async fn reply_via_api(
    state: &Arc<AppState>,
    conversation_id: &str,
    conversation_type: &str,
    text: &str,
) {
    let token = match get_cached_token(state).await {
        Ok(t) => t,
        Err(e) => {
            log::warn!("钉钉: 获取 token 失败，无法回复: {}", e);
            return;
        }
    };

    let client = reqwest::Client::new();

    // 使用新版 API 发送消息到群聊
    if conversation_type == "2" {
        // 群聊：发送到群会话
        let _body = serde_json::json!({
            "msgKey": "sampleMarkdown",
            "msgParam": serde_json::json!({
                "title": "回复",
                "text": text,
            }).to_string(),
        });

        let url = format!(
            "{}/v1.0/robot/groupMessages/send",
            DINGTALK_API_NEW
        );

        match client.post(&url)
            .header("x-acs-dingtalk-access-token", &token)
            .json(&serde_json::json!({
                "msgKey": "sampleMarkdown",
                "msgParam": serde_json::json!({
                    "title": "回复",
                    "text": text,
                }).to_string(),
                "openConversationId": conversation_id,
                "robotCode": state.app_key,
            }))
            .send().await
        {
            Ok(resp) => {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    if data.get("processQueryKey").is_none() {
                        log::warn!("钉钉: API 群聊回复可能失败: {}", data);
                    }
                }
            }
            Err(e) => log::warn!("钉钉: API 群聊回复请求失败: {}", e),
        }
    } else {
        // 单聊：通过机器人发送单聊消息
        let url = format!(
            "{}/v1.0/robot/oToMessages/batchSend",
            DINGTALK_API_NEW
        );

        let _ = client.post(&url)
            .header("x-acs-dingtalk-access-token", &token)
            .json(&serde_json::json!({
                "msgKey": "sampleMarkdown",
                "msgParam": serde_json::json!({
                    "title": "回复",
                    "text": text,
                }).to_string(),
                "robotCode": state.app_key,
                "userIds": [],  // 需要实际的 userId，这里是降级路径
            }))
            .send().await;

        log::info!("钉钉: 通过 API 单聊发送（降级模式）");
    }
}

/// 获取缓存的 access_token（自动刷新）
async fn get_cached_token(state: &Arc<AppState>) -> Result<String, String> {
    let app_key = state.app_key.clone();
    let app_secret = state.app_secret.clone();
    state.token_cache.get_or_refresh(|| async {
        let client = reqwest::Client::new();
        let token = get_access_token(&client, &app_key, &app_secret).await?;
        log::info!("钉钉: access_token 已刷新");
        // 钉钉 access_token 有效期 7200 秒
        Ok((token, 7200))
    }).await
}

/// 获取钉钉 access_token
///
/// POST https://oapi.dingtalk.com/gettoken?appkey=KEY&appsecret=SECRET
async fn get_access_token(
    client: &reqwest::Client,
    app_key: &str,
    app_secret: &str,
) -> Result<String, String> {
    let url = format!(
        "{}/gettoken?appkey={}&appsecret={}",
        DINGTALK_API, app_key, app_secret
    );

    let resp: serde_json::Value = client.get(&url)
        .send().await
        .map_err(|e| format!("获取 access_token 请求失败: {}", e))?
        .json().await
        .map_err(|e| format!("解析 access_token 响应失败: {}", e))?;

    if resp["errcode"].as_i64() != Some(0) {
        return Err(format!("钉钉 API 错误: {} {}",
            resp["errcode"],
            resp["errmsg"].as_str().unwrap_or("unknown")));
    }

    resp["access_token"].as_str()
        .map(String::from)
        .ok_or("access_token 字段缺失".to_string())
}

/// 获取或创建钉钉 session
async fn get_or_create_session(
    pool: &sqlx::SqlitePool,
    agent_id: &str,
    conversation_id: &str,
    title: &str,
) -> String {
    let tag = format!("dingtalk-{}", conversation_id);

    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM chat_sessions WHERE title LIKE '%' || ? || '%' OR title = ? LIMIT 1"
    ).bind(&tag).bind(title).fetch_optional(pool).await.ok().flatten();

    if let Some((id,)) = existing {
        return id;
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let _ = sqlx::query(
        "INSERT INTO chat_sessions (id, agent_id, title, created_at) VALUES (?, ?, ?, ?)"
    ).bind(&id).bind(agent_id).bind(title).bind(now).execute(pool).await;

    id
}
