//! 微信渠道（iLinkai 协议）
//!
//! 使用腾讯 iLinkai 平台 API 接入个人微信。
//! 流程：扫码登录 → 获取 bot_token → 长轮询 getUpdates → 发送回复
//! 参考：@tencent-weixin/openclaw-weixin 插件实现

use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use crate::agent::Orchestrator;

const WEIXIN_BASE: &str = "https://ilinkai.weixin.qq.com";

/// 微信配置
pub struct WeixinConfig {
    /// bot_token（扫码登录后获得）
    pub bot_token: String,
    pub agent_id: String,
}

/// 启动微信长轮询
///
/// 由 ChannelManager 调用，不再自行 spawn，通过 CancellationToken 控制生命周期。
pub async fn start_weixin(
    config: WeixinConfig,
    pool: sqlx::SqlitePool,
    orchestrator: Arc<Orchestrator>,
    app_handle: tauri::AppHandle,
    cancel: CancellationToken,
) -> Result<(), String> {
    let token = config.bot_token.clone();
    let agent_id = config.agent_id.clone();
    log::info!("微信: 启动长轮询 (token: {}..., agent={})", &token[..token.len().min(15)], agent_id);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(45))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let mut get_updates_buf = String::new();
    let mut attempt: u32 = 0; // 连续失败计数，用于指数退避

    // 从 settings 读取 base_url（扫码时可能分配了不同的端点）
    let base_url: String = sqlx::query_scalar(
        "SELECT value FROM settings WHERE key = 'weixin_base_url'"
    ).fetch_optional(&pool).await.ok().flatten()
        .unwrap_or_else(|| WEIXIN_BASE.to_string());
    log::info!("微信: 使用 API 端点: {}", base_url);

    // 从本地缓存恢复 buf
    if let Some(buf) = load_sync_buf(&pool).await {
        get_updates_buf = buf;
    }

    loop {
        if cancel.is_cancelled() {
            log::info!("微信: 收到取消信号，退出轮询");
            return Ok(());
        }

        let result = tokio::select! {
            _ = cancel.cancelled() => {
                log::info!("微信: 轮询等待中收到取消信号");
                return Ok(());
            }
            r = get_updates(&client, &token, &get_updates_buf, &base_url) => r,
        };

        match result {
            Ok(resp) => {
                // 成功响应，重置失败计数
                attempt = 0;

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
                        let aid = agent_id.clone();
                        tokio::spawn(async move {
                            handle_weixin_message(&token, &msg, &pool, &orch, &handle, &aid).await;
                        });
                    }
                }
            }
            Err(e) => {
                if e.contains("session 过期") || e.contains("session timeout") {
                    log::error!("微信: Session 已过期，需要重新扫码登录");
                    let _ = sqlx::query("DELETE FROM settings WHERE key IN ('weixin_bot_token', 'weixin_sync_buf')")
                        .execute(&pool).await;
                    return Err("session 过期".to_string());
                }
                if e == "timeout" {
                    // 长轮询超时是正常的，不算失败
                    continue;
                }
                attempt += 1;
                let delay = super::common::reconnect_delay(attempt);
                if attempt >= 10 {
                    log::error!("微信: getUpdates 失败: {}，连续失败 {} 次，降级为 {}s 探测模式", e, attempt, delay);
                } else {
                    log::warn!("微信: getUpdates 失败: {}，第 {} 次重试，{}s 后重试", e, attempt, delay);
                }
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            }
        }
    }
}

/// 长轮询获取消息
async fn get_updates(
    client: &reqwest::Client,
    token: &str,
    get_updates_buf: &str,
    base_url: &str,
) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({
        "get_updates_buf": get_updates_buf,
        "base_info": { "channel_version": "xianzhu-1.0.0" },
    });

    let resp = client.post(format!("{}/ilink/bot/getupdates", base_url))
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

    let raw_text = resp.text().await
        .map_err(|e| format!("读取响应失败: {}", e))?;

    // 只在有消息或错误时打日志（避免空轮询刷屏）
    if !raw_text.contains("\"msgs\":[]") || raw_text.contains("errcode") {
        log::info!("微信: getUpdates 响应: {}", &raw_text[..raw_text.len().min(300)]);
    }

    let data: serde_json::Value = serde_json::from_str(&raw_text)
        .map_err(|e| format!("解析响应失败: {}", e))?;

    // 检查错误：只有 errcode=-14 才是真正的 session 过期
    let errcode = data["errcode"].as_i64().unwrap_or(0);
    if errcode == -14 {
        return Err("session 过期，需要重新扫码登录".to_string());
    }

    let ret = data["ret"].as_i64().unwrap_or(0);
    if ret != 0 && errcode != 0 {
        let errmsg = data["errmsg"].as_str().unwrap_or("unknown");
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
    config_agent_id: &str,
) {
    let message_type = msg["message_type"].as_i64().unwrap_or(0);
    let message_state = msg["message_state"].as_i64().unwrap_or(-1);
    let from_user = msg["from_user_id"].as_str().unwrap_or("");
    let _to_user = msg["to_user_id"].as_str().unwrap_or("");
    let context_token = msg["context_token"].as_str().unwrap_or("");

    log::info!("微信: handle_message type={} state={} from={} items={}",
        message_type, message_state, &from_user[..from_user.len().min(20)],
        msg["item_list"].as_array().map_or(0, |a| a.len()));

    // 只处理用户消息（message_type=1），忽略 bot 自己的消息(2)
    if message_type != 1 { return; }

    // 处理新消息和生成中的消息
    if message_state != 0 && message_state != 2 { return; }

    // 提取文本内容
    let text = extract_text(msg);
    if text.is_empty() {
        log::info!("微信: 消息无文本内容，跳过");
        return;
    }

    log::info!("微信: [{}] {}", from_user, &text[..text.len().min(50)]);

    // 优先使用 config 中指定的 agent_id，fallback 到 Router
    let agent_id = if !config_agent_id.is_empty() {
        config_agent_id.to_string()
    } else {
        let router = crate::routing::Router::new(orchestrator.pool().clone());
        let route = router.resolve("weixin", Some(from_user)).await;
        match route {
            Ok(r) => r.agent_id,
            Err(_) => {
                let agents = orchestrator.list_agents().await.unwrap_or_default();
                match agents.into_iter().next() {
                    Some(a) => a.id,
                    None => { log::warn!("微信: 无可用 Agent"); return; }
                }
            }
        }
    };
    let agent = match orchestrator.get_agent_cached(&agent_id).await {
        Ok(a) => a,
        Err(e) => { log::warn!("微信: 获取 Agent 失败: {}", e); return; }
    };

    // session
    let session_title = format!("[微信] {}", &from_user[..from_user.len().min(10)]);
    let session_id = get_or_create_session(pool, &agent.id, from_user, &session_title).await;

    // Provider
    let (api_type, api_key, base_url) = match super::find_provider(pool, &agent.model).await {
        Some(info) => info,
        None => {
            send_weixin_text(token, from_user, context_token, "未配置 LLM Provider", pool).await;
            return;
        }
    };

    // 推送用户消息到前端
    use tauri::Manager;
    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "message", "sessionId": session_id,
        "role": "user", "content": text, "source": "weixin",
    }));

    // 推送"思考中"到前端
    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "thinking", "sessionId": session_id, "source": "weixin",
    }));

    // 流式调用 LLM
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };

    // 后台收集 token 并推送流式到桌面端
    let app_for_stream = app_handle.clone();
    let sid_for_stream = session_id.clone();
    let output_handle = tokio::spawn(async move {
        let mut output = String::new();
        while let Some(token) = rx.recv().await {
            output.push_str(&token);
            let _ = app_for_stream.emit_all("chat-event", serde_json::json!({
                "type": "token", "sessionId": sid_for_stream,
                "content": output.clone(), "source": "weixin",
            }));
        }
        output
    });

    let result = orchestrator.send_message_stream(
        &agent.id, &session_id, &text,
        &api_key, &api_type, base_url_opt, tx, None,
    ).await;

    let output = output_handle.await.unwrap_or_default();

    let reply = match result {
        Ok(resp) => if resp.is_empty() { output } else { resp },
        Err(e) => format!("处理出错: {}", &e[..e.len().min(100)]),
    };

    if !reply.is_empty() {
        send_weixin_text(token, from_user, context_token, &reply, pool).await;
        log::info!("微信: 回复 [{}] {}字符", from_user, reply.len());
    }

    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "done", "sessionId": session_id,
        "role": "assistant", "content": reply, "source": "weixin",
    }));

    // Session 自动命名
    crate::memory::conversation::auto_name_session(
        pool, &session_id, &text, &api_key, &api_type, base_url_opt,
    ).await;
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
async fn send_weixin_text(token: &str, to: &str, context_token: &str, text: &str, pool: &sqlx::SqlitePool) {
    let client = reqwest::Client::new();

    // 使用保存的 base_url
    let base_url: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'weixin_base_url'")
        .fetch_optional(pool).await.ok().flatten()
        .unwrap_or_else(|| WEIXIN_BASE.to_string());

    // 参考 OpenClaw: from_user_id 为空, to_user_id 为用户, 必须有 client_id 和 message_state
    let client_id = format!("xianzhu-{}", uuid::Uuid::new_v4());
    let body = serde_json::json!({
        "msg": {
            "from_user_id": "",
            "to_user_id": to,
            "client_id": client_id,
            "context_token": context_token,
            "message_type": 2, // BOT
            "message_state": 2, // FINISH
            "item_list": [{
                "type": 1,
                "text_item": { "text": text },
            }],
        },
        "base_info": { "channel_version": "xianzhu-1.0.0" },
    });

    log::info!("微信: sendMessage to={} base_url={}", &to[..to.len().min(20)], &base_url[..base_url.len().min(40)]);

    match client.post(format!("{}/ilink/bot/sendmessage", base_url))
        .header("Content-Type", "application/json")
        .header("AuthorizationType", "ilink_bot_token")
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send().await
    {
        Ok(resp) => {
            let status = resp.status();
            if let Ok(text) = resp.text().await {
                log::info!("微信: sendMessage 响应 status={} body={}", status, &text[..text.len().min(200)]);
            }
        }
        Err(e) => {
            log::warn!("微信: 发送消息失败: {}", e);
        }
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

    log::info!("微信: 二维码获取成功, qrcode={}, img_url={}",
        data["qrcode"].as_str().unwrap_or("?"),
        &data["qrcode_img_content"].as_str().unwrap_or("?")[..50.min(data["qrcode_img_content"].as_str().unwrap_or("").len())]);

    Ok(data)
}

/// 轮询扫码状态（长轮询，服务器 hold 到有结果或超时）
pub async fn poll_qrcode_status(qrcode: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(40))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let url = format!(
        "{}/ilink/bot/get_qrcode_status?qrcode={}",
        WEIXIN_BASE, urlencoding::encode(qrcode)
    );

    let resp = client.get(&url)
        .header("iLink-App-ClientVersion", "1")
        .send().await
        .map_err(|e| {
            // 长轮询超时是正常的
            if e.is_timeout() {
                return "timeout".to_string();
            }
            format!("轮询状态失败: {}", e)
        })?;

    let data: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析失败: {}", e))?;

    let status = data["status"].as_str().unwrap_or("unknown");
    log::info!("微信: 扫码状态={}, has_token={}, has_bot_id={}, baseurl={}, full_resp={}",
        status,
        data["bot_token"].is_string(),
        data["ilink_bot_id"].is_string(),
        data["baseurl"].as_str().unwrap_or("(none)"),
        &data.to_string()[..data.to_string().len().min(500)],
    );

    Ok(data)
}
