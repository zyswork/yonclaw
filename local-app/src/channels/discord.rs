//! Discord Bot Gateway 接入
//!
//! 通过 Discord Gateway WebSocket 接收消息，本地处理后回复。
//! 支持 DM 和频道消息（需要 @Bot 或配置前缀触发）。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use crate::agent::Orchestrator;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use futures_util::{SinkExt, StreamExt};

/// Discord Bot 配置
pub struct DiscordConfig {
    pub bot_token: String,
}

/// 防止重复启动
static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 启动 Discord Gateway（后台 tokio task，单例）
pub async fn start_gateway(
    config: DiscordConfig,
    pool: sqlx::SqlitePool,
    orchestrator: Arc<Orchestrator>,
    app_handle: tauri::AppHandle,
) {
    // 防止重复启动（已有 Gateway 在运行则跳过）
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        log::info!("Discord: Gateway 已在运行，跳过重复启动");
        return;
    }

    let token = config.bot_token.clone();
    log::info!("Discord: 启动 Gateway 连接 (token: {}...)", &token[..token.len().min(20)]);

    tokio::spawn(async move {
        loop {
            if let Err(e) = run_gateway(&token, &pool, &orchestrator, &app_handle).await {
                log::warn!("Discord: Gateway 断开: {}，10秒后重连", e);
            }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });
}

/// 运行 Gateway 连接主循环
async fn run_gateway(
    token: &str,
    pool: &sqlx::SqlitePool,
    orchestrator: &Arc<Orchestrator>,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    // 1. 获取 Gateway URL
    let client = reqwest::Client::new();
    let gw_resp = client.get("https://discord.com/api/v10/gateway/bot")
        .header("Authorization", format!("Bot {}", token))
        .send().await.map_err(|e| format!("获取 Gateway URL 失败: {}", e))?;

    let gw_data: serde_json::Value = gw_resp.json().await
        .map_err(|e| format!("解析 Gateway 响应失败: {}", e))?;
    let gw_url = gw_data["url"].as_str().ok_or("Gateway URL 为空")?;
    let ws_url = format!("{}/?v=10&encoding=json", gw_url);

    log::info!("Discord: 连接 Gateway: {}", ws_url);

    // 2. 连接 WebSocket
    let (ws_stream, _) = connect_async(&ws_url).await
        .map_err(|e| format!("WebSocket 连接失败: {}", e))?;
    let (mut write, mut read) = ws_stream.split();

    // 3. 接收 Hello（opcode 10）获取 heartbeat_interval
    let hello = read.next().await
        .ok_or("未收到 Hello")?
        .map_err(|e| format!("读取 Hello 失败: {}", e))?;
    let hello_data: serde_json::Value = match hello {
        WsMessage::Text(t) => serde_json::from_str(&t)
            .map_err(|e| format!("解析 Hello 失败: {}", e))?,
        _ => return Err("Hello 不是文本帧".into()),
    };

    let heartbeat_interval = hello_data["d"]["heartbeat_interval"].as_u64().unwrap_or(41250);
    log::info!("Discord: Hello 收到, heartbeat_interval={}ms", heartbeat_interval);

    // 4. 发送 IDENTIFY（opcode 2）
    // Intents: GUILDS(1) + GUILD_MESSAGES(512) + DIRECT_MESSAGES(4096) + MESSAGE_CONTENT(32768) = 37377
    let identify = serde_json::json!({
        "op": 2,
        "d": {
            "token": token,
            "intents": 37377,
            "properties": {
                "os": "macos",
                "browser": "yonclaw",
                "device": "yonclaw"
            }
        }
    });
    write.send(WsMessage::Text(identify.to_string())).await
        .map_err(|e| format!("发送 IDENTIFY 失败: {}", e))?;

    // 5. 心跳任务
    let last_seq = Arc::new(AtomicI64::new(-1));
    let ack_received = Arc::new(AtomicBool::new(true));

    let last_seq_hb = last_seq.clone();
    let ack_hb = ack_received.clone();
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_hb = shutdown.clone();
    // write 仅由心跳任务使用（消息回复走 HTTP REST，不走 WebSocket）
    let write = Arc::new(tokio::sync::Mutex::new(write));
    let write_hb = write.clone();

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(heartbeat_interval)).await;
            if shutdown_hb.load(Ordering::Relaxed) { break; }
            if !ack_hb.load(Ordering::Relaxed) {
                log::warn!("Discord: 未收到心跳 ACK，断开");
                break;
            }
            ack_hb.store(false, Ordering::Relaxed);
            let seq = last_seq_hb.load(Ordering::Relaxed);
            let payload = if seq < 0 {
                serde_json::json!({"op": 1, "d": null})
            } else {
                serde_json::json!({"op": 1, "d": seq})
            };
            let mut w = write_hb.lock().await;
            if w.send(WsMessage::Text(payload.to_string())).await.is_err() {
                break;
            }
        }
    });

    // 获取 bot 自己的 user ID（从 READY 事件）
    let mut bot_user_id = String::new();

    // 6. 事件循环
    while let Some(msg) = read.next().await {
        let msg = match msg {
            Ok(WsMessage::Text(t)) => t,
            Ok(WsMessage::Close(_)) => {
                log::info!("Discord: Gateway 关闭");
                break;
            }
            Err(e) => {
                log::warn!("Discord: WebSocket 错误: {}", e);
                break;
            }
            _ => continue,
        };

        let data: serde_json::Value = match serde_json::from_str(&msg) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let op = data["op"].as_u64().unwrap_or(0);

        // 更新 sequence
        if let Some(s) = data["s"].as_i64() {
            last_seq.store(s, Ordering::Relaxed);
        }

        match op {
            // Dispatch（事件）
            0 => {
                let event_name = data["t"].as_str().unwrap_or("");
                match event_name {
                    "READY" => {
                        bot_user_id = data["d"]["user"]["id"].as_str().unwrap_or("").to_string();
                        log::info!("Discord: READY, bot_user_id={}", bot_user_id);
                    }
                    "MESSAGE_CREATE" => {
                        let d = &data["d"];
                        // 忽略 bot 自己的消息
                        let author_id = d["author"]["id"].as_str().unwrap_or("");
                        if author_id == bot_user_id { continue; }
                        // 忽略其他 bot
                        if d["author"]["bot"].as_bool() == Some(true) { continue; }

                        let content = d["content"].as_str().unwrap_or("").to_string();
                        if content.is_empty() { continue; }

                        let channel_id = d["channel_id"].as_str().unwrap_or("").to_string();
                        let author_name = d["author"]["username"].as_str().unwrap_or("User").to_string();
                        let guild_id = d["guild_id"].as_str().map(|s| s.to_string());

                        // 群聊中需要 @Bot 才触发（DM 直接回复）
                        let is_dm = guild_id.is_none();
                        let mentions_bot = d["mentions"].as_array()
                            .map(|arr| arr.iter().any(|m| m["id"].as_str() == Some(&bot_user_id)))
                            .unwrap_or(false);

                        if !is_dm && !mentions_bot { continue; }

                        // 去掉 @Bot mention 标记
                        let text = content
                            .replace(&format!("<@{}>", bot_user_id), "")
                            .trim()
                            .to_string();
                        if text.is_empty() { continue; }

                        let t = token.to_string();
                        let p = pool.clone();
                        let o = orchestrator.clone();
                        let h = app_handle.clone();
                        tokio::spawn(async move {
                            handle_message(&t, &channel_id, &author_name, &text, &p, &o, &h).await;
                        });
                    }
                    _ => {}
                }
            }
            // Heartbeat ACK
            11 => {
                ack_received.store(true, Ordering::Relaxed);
            }
            // Reconnect requested
            7 => {
                log::info!("Discord: 服务端请求重连");
                break;
            }
            // Invalid session
            9 => {
                log::warn!("Discord: 会话失效，重连");
                break;
            }
            _ => {}
        }
    }

    shutdown.store(true, Ordering::Relaxed);
    Err("Gateway 循环结束".into())
}

/// 处理单条消息
async fn handle_message(
    token: &str,
    channel_id: &str,
    author_name: &str,
    text: &str,
    pool: &sqlx::SqlitePool,
    orchestrator: &Arc<Orchestrator>,
    app_handle: &tauri::AppHandle,
) {
    log::info!("Discord: [{}] {}: {}", channel_id, author_name, &text[..text.len().min(80)]);

    // 获取本地 Agent
    let agent = match orchestrator.list_agents().await {
        Ok(agents) => match agents.into_iter().next() {
            Some(a) => a,
            None => { log::warn!("Discord: 无可用 Agent"); return; }
        },
        Err(_) => return,
    };

    // 获取或创建 session
    let session_title = format!("[Discord] {}", author_name);
    let session_id = get_or_create_session(pool, &agent.id, channel_id, &session_title).await;

    // 发送 typing 状态
    send_typing(token, channel_id).await;

    // 查找 Provider（与 telegram 相同逻辑）
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
            // 检查模型是否在此 provider
            if let Some(models) = p["models"].as_array() {
                for m in models {
                    if m["id"].as_str() == Some(&agent.model) {
                        return Some((api_type, key.to_string(), base_url));
                    }
                }
            }
            // 有 key 就用第一个
            return Some((api_type, key.to_string(), base_url));
        }
        None
    });

    let (api_type, api_key, base_url) = match provider_info {
        Some(info) => info,
        None => {
            discord_send_message(token, channel_id, "未配置 LLM Provider，请在桌面端设置中添加。").await;
            return;
        }
    };

    use tauri::Manager;

    // 推送用户消息到前端
    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "message",
        "sessionId": session_id,
        "role": "user",
        "content": text,
        "source": "discord",
    }));

    // 推送"思考中"状态
    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "thinking",
        "sessionId": session_id,
        "source": "discord",
    }));

    // 流式调用 orchestrator
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // 收集输出 + 推送流式 token
    let app_for_stream = app_handle.clone();
    let sid_for_stream = session_id.clone();
    let output_handle = tokio::spawn(async move {
        let mut output = String::new();
        while let Some(tok) = rx.recv().await {
            output.push_str(&tok);
            let _ = app_for_stream.emit_all("chat-event", serde_json::json!({
                "type": "token",
                "sessionId": sid_for_stream,
                "content": output.clone(),
                "source": "discord",
            }));
        }
        output
    });

    let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };

    let result = orchestrator.send_message_stream(
        &agent.id, &session_id, text,
        &api_key, &api_type, base_url_opt, tx, None,
    ).await;

    let response = output_handle.await.unwrap_or_default();

    let reply = match result {
        Ok(resp) => {
            let r = if resp.is_empty() { response.clone() } else { resp };
            if !r.is_empty() {
                // Discord 消息上限 2000 字符，超过分段发送
                for chunk in split_message(&r, 2000) {
                    discord_send_message(token, channel_id, chunk).await;
                }
                log::info!("Discord: 回复 [{}] {}字符", channel_id, r.len());
            }
            r
        }
        Err(e) => {
            log::error!("Discord: 处理失败: {}", e);
            let err_msg = format!("处理出错: {}", &e[..e.len().min(200)]);
            discord_send_message(token, channel_id, &err_msg).await;
            err_msg
        }
    };

    // 推送完整回复到前端
    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "done",
        "sessionId": session_id,
        "role": "assistant",
        "content": reply,
        "source": "discord",
    }));
}

/// 获取或创建 Discord session
async fn get_or_create_session(pool: &sqlx::SqlitePool, agent_id: &str, channel_id: &str, title: &str) -> String {
    let tag = format!("discord-{}", channel_id);

    // 先查有没有已存在的
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM chat_sessions WHERE title LIKE '%' || ? || '%' OR title = ? LIMIT 1"
    ).bind(&tag).bind(title).fetch_optional(pool).await.ok().flatten();

    if let Some((id,)) = existing {
        return id;
    }

    // 创建新 session（title 包含 tag 方便后续查找）
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let full_title = format!("{} discord-{}", title, channel_id);
    let _ = sqlx::query(
        "INSERT INTO chat_sessions (id, agent_id, title, created_at) VALUES (?, ?, ?, ?)"
    ).bind(&id).bind(agent_id).bind(&full_title).bind(now).execute(pool).await;

    id
}

/// 发送 typing 状态（Discord Trigger Typing Indicator）
async fn send_typing(token: &str, channel_id: &str) {
    let client = reqwest::Client::new();
    let _ = client.post(format!("https://discord.com/api/v10/channels/{}/typing", channel_id))
        .header("Authorization", format!("Bot {}", token))
        .send().await;
}

/// 发送消息到 Discord 频道
async fn discord_send_message(token: &str, channel_id: &str, content: &str) {
    let client = reqwest::Client::new();
    let resp = client.post(format!("https://discord.com/api/v10/channels/{}/messages", channel_id))
        .header("Authorization", format!("Bot {}", token))
        .json(&serde_json::json!({"content": content}))
        .send().await;

    if let Err(e) = resp {
        log::warn!("Discord: 发送消息失败: {}", e);
    }
}

/// 分段发送（Discord 单条消息上限 2000 字符）
fn split_message(text: &str, max_len: usize) -> Vec<&str> {
    if text.len() <= max_len {
        return vec![text];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let end = (start + max_len).min(text.len());
        // 尝试在换行处断开，避免截断一行
        let actual_end = if end < text.len() {
            text[start..end].rfind('\n').map(|p| start + p + 1).unwrap_or(end)
        } else {
            end
        };
        chunks.push(&text[start..actual_end]);
        start = actual_end;
    }
    chunks
}
