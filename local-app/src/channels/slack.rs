//! Slack Socket Mode 接入
//!
//! 通过 Slack Socket Mode WebSocket 接收消息，本地处理后回复。
//! Socket Mode 不需要公网 URL，完美适配桌面端。

use std::sync::Arc;
use crate::agent::Orchestrator;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use futures_util::{SinkExt, StreamExt};

/// Slack Bot 配置
pub struct SlackConfig {
    pub bot_token: String,   // xoxb-...
    pub app_token: String,   // xapp-...
}

/// 防止重复启动
static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 启动 Slack Socket Mode（后台 tokio task，单例）
pub async fn start_socket_mode(
    config: SlackConfig,
    pool: sqlx::SqlitePool,
    orchestrator: Arc<Orchestrator>,
    app_handle: tauri::AppHandle,
) {
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        log::info!("Slack: Socket Mode 已在运行，跳过重复启动");
        return;
    }

    log::info!("Slack: 启动 Socket Mode");

    tokio::spawn(async move {
        loop {
            if let Err(e) = run_socket_mode(&config, &pool, &orchestrator, &app_handle).await {
                log::warn!("Slack: Socket Mode 断开: {}，10秒后重连", e);
            }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });
}

/// 运行 Socket Mode 主循环
async fn run_socket_mode(
    config: &SlackConfig,
    pool: &sqlx::SqlitePool,
    orchestrator: &Arc<Orchestrator>,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    // 1. 获取 WebSocket URL
    let client = reqwest::Client::new();
    let resp = client.post("https://slack.com/api/apps.connections.open")
        .header("Authorization", format!("Bearer {}", config.app_token))
        .send().await.map_err(|e| format!("获取 Socket Mode URL 失败: {}", e))?;

    let data: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析响应失败: {}", e))?;
    if data["ok"].as_bool() != Some(true) {
        return Err(format!("apps.connections.open 失败: {}", data));
    }
    let ws_url = data["url"].as_str().ok_or("WebSocket URL 为空")?;
    log::info!("Slack: 连接 Socket Mode: {}...", &ws_url[..ws_url.len().min(60)]);

    // 2. 连接 WebSocket
    let (ws_stream, _) = connect_async(ws_url).await
        .map_err(|e| format!("WebSocket 连接失败: {}", e))?;
    let (write, mut read) = ws_stream.split();
    let write = Arc::new(tokio::sync::Mutex::new(write));

    // 获取 bot user ID
    let auth_resp = client.post("https://slack.com/api/auth.test")
        .header("Authorization", format!("Bearer {}", config.bot_token))
        .send().await;
    let bot_user_id = match auth_resp {
        Ok(r) => {
            let data: serde_json::Value = r.json().await.unwrap_or_default();
            data["user_id"].as_str().unwrap_or("").to_string()
        }
        Err(_) => String::new(),
    };
    log::info!("Slack: bot_user_id={}", bot_user_id);

    // 3. 事件循环
    while let Some(msg) = read.next().await {
        let msg = match msg {
            Ok(WsMessage::Text(t)) => t,
            Ok(WsMessage::Close(_)) => {
                log::info!("Slack: Socket 关闭");
                break;
            }
            Err(e) => {
                log::warn!("Slack: WebSocket 错误: {}", e);
                break;
            }
            _ => continue,
        };

        let envelope: serde_json::Value = match serde_json::from_str(&msg) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let envelope_id = envelope["envelope_id"].as_str().unwrap_or("").to_string();
        let event_type = envelope["type"].as_str().unwrap_or("");

        // ACK：必须在 3 秒内响应
        if !envelope_id.is_empty() {
            let ack = serde_json::json!({"envelope_id": envelope_id});
            let mut w = write.lock().await;
            let _ = w.send(WsMessage::Text(ack.to_string())).await;
        }

        match event_type {
            "events_api" => {
                let event = &envelope["payload"]["event"];
                let event_subtype = event["type"].as_str().unwrap_or("");

                if event_subtype != "message" { continue; }
                // 忽略 bot 消息、编辑、删除等
                if event["subtype"].as_str().is_some() { continue; }
                // 忽略自己的消息
                let user = event["user"].as_str().unwrap_or("");
                if user == bot_user_id { continue; }

                let text = event["text"].as_str().unwrap_or("").to_string();
                if text.is_empty() { continue; }

                let channel = event["channel"].as_str().unwrap_or("").to_string();
                let channel_type = event["channel_type"].as_str().unwrap_or("");
                let thread_ts = event["thread_ts"].as_str()
                    .or(event["ts"].as_str())
                    .unwrap_or("")
                    .to_string();

                // 频道消息需要 @Bot 触发，DM/im 直接回复
                let is_dm = channel_type == "im";
                let mentions_bot = text.contains(&format!("<@{}>", bot_user_id));
                if !is_dm && !mentions_bot { continue; }

                // 去掉 @Bot
                let clean_text = text
                    .replace(&format!("<@{}>", bot_user_id), "")
                    .trim()
                    .to_string();
                if clean_text.is_empty() { continue; }

                // 获取用户名
                let user_name = get_user_name(&config.bot_token, user).await
                    .unwrap_or_else(|| user.to_string());

                let bt = config.bot_token.clone();
                let p = pool.clone();
                let o = orchestrator.clone();
                let h = app_handle.clone();
                tokio::spawn(async move {
                    handle_message(
                        &bt, &channel, &thread_ts, &user_name, &clean_text,
                        &p, &o, &h,
                    ).await;
                });
            }
            "disconnect" => {
                let reason = envelope["reason"].as_str().unwrap_or("unknown");
                log::info!("Slack: 收到 disconnect: {}", reason);
                break;
            }
            "hello" => {
                log::info!("Slack: Socket Mode 连接就绪");
            }
            _ => {}
        }
    }

    Err("Socket Mode 循环结束".into())
}

/// 获取用户名
async fn get_user_name(bot_token: &str, user_id: &str) -> Option<String> {
    let client = reqwest::Client::new();
    let resp = client.get("https://slack.com/api/users.info")
        .header("Authorization", format!("Bearer {}", bot_token))
        .query(&[("user", user_id)])
        .send().await.ok()?;
    let data: serde_json::Value = resp.json().await.ok()?;
    data["user"]["real_name"].as_str()
        .or(data["user"]["name"].as_str())
        .map(|s| s.to_string())
}

/// 处理消息
async fn handle_message(
    bot_token: &str,
    channel: &str,
    thread_ts: &str,
    user_name: &str,
    text: &str,
    pool: &sqlx::SqlitePool,
    orchestrator: &Arc<Orchestrator>,
    app_handle: &tauri::AppHandle,
) {
    log::info!("Slack: [{}] {}: {}", channel, user_name, &text[..text.len().min(80)]);

    let agent = match orchestrator.list_agents().await {
        Ok(agents) => match agents.into_iter().next() {
            Some(a) => a,
            None => {
                log::warn!("Slack: 无可用 Agent");
                return;
            }
        },
        Err(_) => return,
    };

    let session_title = format!("[Slack] {}", user_name);
    let session_id = get_or_create_session(pool, &agent.id, channel, &session_title).await;

    // Provider 查找
    let providers_json: Option<String> = sqlx::query_scalar(
        "SELECT value FROM settings WHERE key = 'providers'"
    ).fetch_optional(pool).await.ok().flatten();

    let provider_info = providers_json.and_then(|pj| {
        let providers: Vec<serde_json::Value> = serde_json::from_str(&pj).ok()?;
        for p in &providers {
            if p["enabled"].as_bool() != Some(true) { continue; }
            let key = p["apiKey"].as_str().unwrap_or("");
            if key.is_empty() { continue; }
            let api_type = p["apiType"].as_str().unwrap_or("openai").to_string();
            let base_url = p["baseUrl"].as_str().unwrap_or("").to_string();
            return Some((api_type, key.to_string(), base_url));
        }
        None
    });

    let (api_type, api_key, base_url) = match provider_info {
        Some(info) => info,
        None => {
            slack_post_message(bot_token, channel, thread_ts, "⚠️ No LLM provider configured.").await;
            return;
        }
    };

    use tauri::Manager;
    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "message",
        "sessionId": session_id,
        "role": "user",
        "content": text,
        "source": "slack",
    }));
    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "thinking",
        "sessionId": session_id,
        "source": "slack",
    }));

    // 先发一条"思考中"消息，拿到 ts 用于后续更新
    let initial_ts = slack_post_message(bot_token, channel, thread_ts, "💭 Thinking...").await;
    if initial_ts.is_none() {
        log::warn!("Slack: 发送初始消息失败，流式更新将不可用");
    }

    // 流式调用
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let app_for_stream = app_handle.clone();
    let sid_for_stream = session_id.clone();

    // 流式更新 Slack 消息（节流 1.5s）
    let bt_stream = bot_token.to_string();
    let ch_stream = channel.to_string();
    let ts_stream = initial_ts.clone();
    let output_handle = tokio::spawn(async move {
        let mut output = String::new();
        let mut last_update = std::time::Instant::now();
        while let Some(tok) = rx.recv().await {
            output.push_str(&tok);
            let _ = app_for_stream.emit_all("chat-event", serde_json::json!({
                "type": "token",
                "sessionId": sid_for_stream,
                "content": output.clone(),
                "source": "slack",
            }));
            // 节流：每 1.5 秒更新一次 Slack 消息
            if last_update.elapsed() > std::time::Duration::from_millis(1500) {
                if let Some(ref ts) = ts_stream {
                    slack_update_message(
                        &bt_stream, &ch_stream, ts,
                        &format!("{}▌", output),
                    ).await;
                }
                last_update = std::time::Instant::now();
            }
        }
        output
    });

    let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };
    let result = orchestrator.send_message_stream(
        &agent.id, &session_id, text, &api_key, &api_type,
        base_url_opt, tx, None,
    ).await;

    let response = output_handle.await.unwrap_or_default();
    let reply = match result {
        Ok(resp) => {
            let r = if resp.is_empty() { response } else { resp };
            // 更新最终消息（移除光标）
            if let Some(ref ts) = initial_ts {
                slack_update_message(bot_token, channel, ts, &r).await;
            } else if !r.is_empty() {
                slack_post_message(bot_token, channel, thread_ts, &r).await;
            }
            r
        }
        Err(e) => {
            let err_msg = format!("⚠️ Error: {}", &e[..e.len().min(200)]);
            if let Some(ref ts) = initial_ts {
                slack_update_message(bot_token, channel, ts, &err_msg).await;
            } else {
                slack_post_message(bot_token, channel, thread_ts, &err_msg).await;
            }
            err_msg
        }
    };

    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "done",
        "sessionId": session_id,
        "role": "assistant",
        "content": reply,
        "source": "slack",
    }));
}

/// 获取或创建 session
async fn get_or_create_session(
    pool: &sqlx::SqlitePool,
    agent_id: &str,
    channel_id: &str,
    title: &str,
) -> String {
    let tag = format!("slack-{}", channel_id);
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM chat_sessions WHERE title LIKE '%' || ? || '%' OR title = ? LIMIT 1"
    ).bind(&tag).bind(title).fetch_optional(pool).await.ok().flatten();

    if let Some((id,)) = existing {
        return id;
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let full_title = format!("{} slack-{}", title, channel_id);
    let _ = sqlx::query(
        "INSERT INTO chat_sessions (id, agent_id, title, created_at) VALUES (?, ?, ?, ?)"
    ).bind(&id).bind(agent_id).bind(&full_title).bind(now)
        .execute(pool).await;
    id
}

/// 发送消息到 Slack（返回消息 ts 用于后续更新）
async fn slack_post_message(
    bot_token: &str,
    channel: &str,
    thread_ts: &str,
    text: &str,
) -> Option<String> {
    let client = reqwest::Client::new();
    let mut body = serde_json::json!({
        "channel": channel,
        "text": text,
    });
    // 在线程中回复
    if !thread_ts.is_empty() {
        body["thread_ts"] = serde_json::Value::String(thread_ts.to_string());
    }
    let resp = client.post("https://slack.com/api/chat.postMessage")
        .header("Authorization", format!("Bearer {}", bot_token))
        .json(&body)
        .send().await.ok()?;
    let data: serde_json::Value = resp.json().await.ok()?;
    data["ts"].as_str().map(|s| s.to_string())
}

/// 更新已发送的消息（用于流式输出）
async fn slack_update_message(bot_token: &str, channel: &str, ts: &str, text: &str) {
    let client = reqwest::Client::new();
    let _ = client.post("https://slack.com/api/chat.update")
        .header("Authorization", format!("Bearer {}", bot_token))
        .json(&serde_json::json!({
            "channel": channel,
            "ts": ts,
            "text": text,
        }))
        .send().await;
}
