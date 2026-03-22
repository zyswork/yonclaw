//! 微信渠道（iLinkai 协议）
//!
//! 使用腾讯 iLinkai 平台 API 接入个人微信。
//! 流程：扫码登录 → 获取 bot_token → 长轮询 getUpdates → 发送回复
//! 参考：@tencent-weixin/openclaw-weixin 插件实现

use std::sync::Arc;
use crate::agent::Orchestrator;

const WEIXIN_BASE: &str = "https://ilinkai.weixin.qq.com";

/// 微信配置
pub struct WeixinConfig {
    /// bot_token（扫码登录后获得）
    pub bot_token: String,
}

/// 启动微信长轮询
pub async fn start_weixin(
    config: WeixinConfig,
    pool: sqlx::SqlitePool,
    orchestrator: Arc<Orchestrator>,
    app_handle: tauri::AppHandle,
) {
    let token = config.bot_token.clone();
    log::info!("微信: 启动长轮询 (token: {}...)", &token[..token.len().min(10)]);

    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(45))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let mut get_updates_buf = String::new();

        // 从本地缓存恢复 buf
        if let Some(buf) = load_sync_buf(&pool).await {
            get_updates_buf = buf;
        }

        loop {
            match get_updates(&client, &token, &get_updates_buf).await {
                Ok(resp) => {
                    // 更新 buf
                    if let Some(buf) = resp.get("get_updates_buf").and_then(|b| b.as_str()) {
                        if !buf.is_empty() {
                            get_updates_buf = buf.to_string();
                            save_sync_buf(&pool, &get_updates_buf).await;
                        }
                    }

                    // 处理消息
                    if let Some(msgs) = resp.get("msgs").and_then(|m| m.as_array()) {
                        if !msgs.is_empty() {
                            log::info!("微信: 收到 {} 条消息", msgs.len());
                        }
                        for msg in msgs {
                            let token = token.clone();
                            let pool = pool.clone();
                            let orch = orchestrator.clone();
                            let handle = app_handle.clone();
                            let msg = msg.clone();
                            tokio::spawn(async move {
                                handle_weixin_message(&token, &msg, &pool, &orch, &handle).await;
                            });
                        }
                    }
                }
                Err(e) => {
                    log::warn!("微信: getUpdates 失败: {}，5秒后重试", e);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    });
}

/// 长轮询获取消息
async fn get_updates(
    client: &reqwest::Client,
    token: &str,
    get_updates_buf: &str,
) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({
        "get_updates_buf": get_updates_buf,
        "base_info": { "channel_version": "yonclaw-1.0.0" },
    });

    let resp = client.post(format!("{}/ilink/bot/getupdates", WEIXIN_BASE))
        .header("Content-Type", "application/json")
        .header("AuthorizationType", "ilink_bot_token")
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send().await
        .map_err(|e| {
            // 长轮询超时是正常的
            if e.is_timeout() {
                return "timeout".to_string();
            }
            format!("{}", e)
        })?;

    let data: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析响应失败: {}", e))?;

    let ret = data["ret"].as_i64().unwrap_or(-1);
    if ret != 0 {
        let errcode = data["errcode"].as_i64().unwrap_or(0);
        let errmsg = data["errmsg"].as_str().unwrap_or("unknown");
        // -14 = session 过期
        if errcode == -14 {
            return Err("session 过期，需要重新扫码登录".to_string());
        }
        return Err(format!("getUpdates ret={} errcode={} errmsg={}", ret, errcode, errmsg));
    }

    Ok(data)
}

/// 处理单条微信消息
async fn handle_weixin_message(
    token: &str,
    msg: &serde_json::Value,
    pool: &sqlx::SqlitePool,
    orchestrator: &Arc<Orchestrator>,
    app_handle: &tauri::AppHandle,
) {
    // 只处理用户消息（message_type=1）
    let message_type = msg["message_type"].as_i64().unwrap_or(0);
    if message_type != 1 { return; }

    // 只处理新消息（message_state=0）
    let message_state = msg["message_state"].as_i64().unwrap_or(-1);
    if message_state != 0 { return; }

    let from_user = msg["from_user_id"].as_str().unwrap_or("");
    let to_user = msg["to_user_id"].as_str().unwrap_or("");
    let context_token = msg["context_token"].as_str().unwrap_or("");

    // 提取文本内容
    let text = extract_text(msg);
    if text.is_empty() { return; }

    log::info!("微信: [{}] {}", from_user, &text[..text.len().min(50)]);

    // 获取 Agent
    let agent = match orchestrator.list_agents().await {
        Ok(agents) => match agents.into_iter().next() {
            Some(a) => a,
            None => { log::warn!("微信: 无可用 Agent"); return; }
        },
        Err(_) => return,
    };

    // session
    let session_title = format!("[微信] {}", &from_user[..from_user.len().min(10)]);
    let session_id = get_or_create_session(pool, &agent.id, from_user, &session_title).await;

    // Provider
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
            send_weixin_text(token, from_user, to_user, context_token, "未配置 LLM Provider").await;
            return;
        }
    };

    // 推送到前端
    use tauri::Manager;
    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "message", "sessionId": session_id,
        "role": "user", "content": text, "source": "weixin",
    }));

    // 调用 LLM
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };

    let result = orchestrator.send_message_stream(
        &agent.id, &session_id, &text,
        &api_key, &api_type, base_url_opt, tx, None,
    ).await;

    let mut output = String::new();
    while let Ok(t) = rx.try_recv() { output.push_str(&t); }

    let reply = match result {
        Ok(resp) => if resp.is_empty() { output } else { resp },
        Err(e) => format!("处理出错: {}", &e[..e.len().min(100)]),
    };

    if !reply.is_empty() {
        send_weixin_text(token, to_user, from_user, context_token, &reply).await;
        log::info!("微信: 回复 [{}] {}字符", from_user, reply.len());
    }

    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "done", "sessionId": session_id,
        "role": "assistant", "content": reply, "source": "weixin",
    }));
}

/// 提取消息文本
fn extract_text(msg: &serde_json::Value) -> String {
    if let Some(items) = msg["item_list"].as_array() {
        for item in items {
            let item_type = item["type"].as_i64().unwrap_or(0);
            if item_type == 1 { // TEXT
                if let Some(text) = item["text_item"]["text"].as_str() {
                    return text.to_string();
                }
            }
        }
    }
    String::new()
}

/// 发送文本消息
async fn send_weixin_text(token: &str, from: &str, to: &str, context_token: &str, text: &str) {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "msg": {
            "from_user_id": from,
            "to_user_id": to,
            "context_token": context_token,
            "message_type": 2, // BOT
            "item_list": [{
                "type": 1,
                "text_item": { "text": text },
            }],
        },
        "base_info": { "channel_version": "yonclaw-1.0.0" },
    });

    let resp = client.post(format!("{}/ilink/bot/sendmessage", WEIXIN_BASE))
        .header("Content-Type", "application/json")
        .header("AuthorizationType", "ilink_bot_token")
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send().await;

    if let Err(e) = resp {
        log::warn!("微信: 发送消息失败: {}", e);
    }
}

/// 保存 sync buf 到数据库
async fn save_sync_buf(pool: &sqlx::SqlitePool, buf: &str) {
    let _ = sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('weixin_sync_buf', ?)")
        .bind(buf).execute(pool).await;
}

/// 加载 sync buf
async fn load_sync_buf(pool: &sqlx::SqlitePool) -> Option<String> {
    sqlx::query_scalar("SELECT value FROM settings WHERE key = 'weixin_sync_buf'")
        .fetch_optional(pool).await.ok().flatten()
}

/// 获取或创建微信 session
async fn get_or_create_session(pool: &sqlx::SqlitePool, agent_id: &str, user_id: &str, title: &str) -> String {
    let tag = format!("wx-{}", &user_id[..user_id.len().min(16)]);
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM chat_sessions WHERE title LIKE '%' || ? || '%' LIMIT 1"
    ).bind(&tag).fetch_optional(pool).await.ok().flatten();

    if let Some((id,)) = existing { return id; }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let full_title = format!("{} {}", title, tag);
    let _ = sqlx::query("INSERT INTO chat_sessions (id, agent_id, title, created_at) VALUES (?, ?, ?, ?)")
        .bind(&id).bind(agent_id).bind(&full_title).bind(now).execute(pool).await;
    id
}

// ─── 扫码登录 API（供前端 Tauri 命令调用）─────────────

/// 获取微信登录二维码
pub async fn get_login_qrcode() -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let resp = client.get(format!("{}/ilink/bot/get_bot_qrcode?bot_type=3", WEIXIN_BASE))
        .send().await
        .map_err(|e| format!("获取二维码失败: {}", e))?;

    let data: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析失败: {}", e))?;

    Ok(data)
}

/// 轮询扫码状态
pub async fn poll_qrcode_status(qrcode: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(40))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let resp = client.get(format!(
        "{}/ilink/bot/get_qrcode_status?qrcode={}",
        WEIXIN_BASE, urlencoding::encode(qrcode)
    ))
    .header("iLink-App-ClientVersion", "1")
    .send().await
    .map_err(|e| format!("轮询状态失败: {}", e))?;

    let data: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析失败: {}", e))?;

    Ok(data)
}
