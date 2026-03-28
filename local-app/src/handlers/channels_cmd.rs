//! 频道管理相关命令

use std::sync::Arc;
use tauri::State;

use crate::channels;
use crate::AppState;

/// 创建 Agent 频道连接
#[tauri::command]
pub async fn create_agent_channel(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    channel_type: String,
    credentials: serde_json::Value,
    display_name: Option<String>,
) -> Result<serde_json::Value, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let creds_str = serde_json::to_string(&credentials).map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO agent_channels (id, agent_id, channel_type, credentials_json, display_name, enabled, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, 1, 'configured', ?, ?)"
    )
    .bind(&id).bind(&agent_id).bind(&channel_type).bind(&creds_str)
    .bind(&display_name).bind(now).bind(now)
    .execute(state.orchestrator.pool()).await
    .map_err(|e| format!("创建失败: {}", e))?;

    // 自动启动
    if let Some(mgr) = state.channel_manager.get() {
        if let Err(e) = mgr.start_instance(&id, &agent_id, &channel_type, &creds_str).await {
            log::warn!("频道自动启动失败: {}", e);
        }
    }

    Ok(serde_json::json!({ "id": id }))
}

/// 列出 Agent 的频道连接
#[tauri::command]
pub async fn list_agent_channels(
    state: State<'_, Arc<AppState>>,
    agent_id: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let rows: Vec<(String, String, String, String, Option<String>, bool, String, Option<String>, i64)> = if let Some(aid) = agent_id {
        sqlx::query_as(
            "SELECT id, agent_id, channel_type, credentials_json, display_name, enabled, status, status_message, created_at FROM agent_channels WHERE agent_id = ? ORDER BY created_at"
        ).bind(aid).fetch_all(state.orchestrator.pool()).await
    } else {
        sqlx::query_as(
            "SELECT id, agent_id, channel_type, credentials_json, display_name, enabled, status, status_message, created_at FROM agent_channels ORDER BY created_at"
        ).fetch_all(state.orchestrator.pool()).await
    }.map_err(|e| format!("查询失败: {}", e))?;

    Ok(rows.iter().map(|(id, aid, ct, creds, dn, en, st, sm, ca)| {
        // 脱敏 credentials
        let creds_val: serde_json::Value = serde_json::from_str(creds).unwrap_or_default();
        let mut masked = serde_json::Map::new();
        if let Some(obj) = creds_val.as_object() {
            for (k, v) in obj {
                if let Some(s) = v.as_str() {
                    if s.len() > 8 { masked.insert(k.clone(), serde_json::json!(format!("{}...{}", &s[..4], &s[s.len()-4..]))); }
                    else { masked.insert(k.clone(), serde_json::json!("****")); }
                } else { masked.insert(k.clone(), v.clone()); }
            }
        }
        serde_json::json!({
            "id": id, "agentId": aid, "channelType": ct,
            "credentials": masked, "displayName": dn,
            "enabled": en, "status": st, "statusMessage": sm, "createdAt": ca,
        })
    }).collect())
}

/// 删除 Agent 频道连接
#[tauri::command]
pub async fn delete_agent_channel(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    // 先停止
    if let Some(mgr) = state.channel_manager.get() {
        mgr.stop_instance(&id).await;
    }
    sqlx::query("DELETE FROM agent_channels WHERE id = ?")
        .bind(&id).execute(state.orchestrator.pool()).await
        .map_err(|e| format!("删除失败: {}", e))?;
    Ok(())
}

/// 启用/禁用 Agent 频道连接
#[tauri::command]
pub async fn toggle_agent_channel(
    state: State<'_, Arc<AppState>>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    sqlx::query("UPDATE agent_channels SET enabled = ?, updated_at = strftime('%s','now') WHERE id = ?")
        .bind(enabled).bind(&id).execute(state.orchestrator.pool()).await
        .map_err(|e| format!("更新失败: {}", e))?;

    if let Some(mgr) = state.channel_manager.get() {
        if enabled {
            // 重新启动
            let row: Option<(String, String, String)> = sqlx::query_as(
                "SELECT agent_id, channel_type, credentials_json FROM agent_channels WHERE id = ?"
            ).bind(&id).fetch_optional(state.orchestrator.pool()).await.map_err(|e| e.to_string())?;
            if let Some((aid, ct, creds)) = row {
                let _ = mgr.start_instance(&id, &aid, &ct, &creds).await;
            }
        } else {
            mgr.stop_instance(&id).await;
        }
    }
    Ok(())
}

/// 获取微信登录二维码
#[tauri::command]
pub async fn weixin_get_qrcode() -> Result<serde_json::Value, String> {
    channels::weixin::get_login_qrcode().await
}

/// 轮询微信扫码状态
#[tauri::command]
pub async fn weixin_poll_status(qrcode: String) -> Result<serde_json::Value, String> {
    channels::weixin::poll_qrcode_status(&qrcode).await
}

/// 保存微信 token 并立即启动轮询
#[tauri::command]
pub async fn weixin_save_token(
    state: State<'_, Arc<AppState>>,
    app_handle: tauri::AppHandle,
    bot_token: String,
) -> Result<(), String> {
    let _ = sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('weixin_bot_token', ?)")
        .bind(&bot_token).execute(state.orchestrator.pool()).await;
    // 清空旧的 sync_buf（旧 buf 绑定旧 token，会导致 session timeout）
    let _ = sqlx::query("DELETE FROM settings WHERE key = 'weixin_sync_buf'")
        .execute(state.orchestrator.pool()).await;
    log::info!("微信: token 已保存（旧 sync_buf 已清除），立即启动轮询");

    // 立即启动微信轮询（不等重启）
    let pool = state.orchestrator.pool().clone();
    let orch = state.orchestrator.clone();
    let handle = app_handle.clone();
    let token = bot_token.clone();
    tokio::spawn(async move {
        let _ = channels::weixin::start_weixin(
            channels::weixin::WeixinConfig { bot_token: token, agent_id: String::new() },
            pool, orch, handle,
            tokio_util::sync::CancellationToken::new(),
        ).await;
    });

    Ok(())
}

/// 验证 Telegram Bot Token（桌面端能翻墙访问 api.telegram.org）
#[tauri::command]
pub async fn verify_telegram_token(bot_token: String) -> Result<serde_json::Value, String> {
    let url = format!("https://api.telegram.org/bot{}/getMe", bot_token.trim());
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await
        .map_err(|e| format!("请求失败: {}", e))?;

    let data: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析失败: {}", e))?;

    if data["ok"].as_bool() == Some(true) {
        let result = &data["result"];
        Ok(serde_json::json!({
            "ok": true,
            "username": result["username"].as_str().unwrap_or(""),
            "name": result["first_name"].as_str().unwrap_or(""),
            "id": result["id"],
        }))
    } else {
        Ok(serde_json::json!({
            "ok": false,
            "error": data["description"].as_str().unwrap_or("未知错误"),
        }))
    }
}

/// 验证 Discord Bot Token 并保存 + 启动 Gateway
#[tauri::command]
pub async fn discord_connect(
    state: State<'_, Arc<AppState>>,
    app_handle: tauri::AppHandle,
    bot_token: String,
) -> Result<serde_json::Value, String> {
    let token = bot_token.trim().to_string();
    // 验证 Token
    let client = reqwest::Client::new();
    let resp = client.get("https://discord.com/api/v10/users/@me")
        .header("Authorization", format!("Bot {}", token))
        .send().await.map_err(|e| format!("请求失败: {}", e))?;

    let data: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析失败: {}", e))?;

    if data["id"].as_str().is_none() {
        return Ok(serde_json::json!({
            "ok": false,
            "error": data["message"].as_str().unwrap_or("Invalid token"),
        }));
    }

    let username = data["username"].as_str().unwrap_or("");
    let discriminator = data["discriminator"].as_str().unwrap_or("0");

    // 保存 Token
    let _ = sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('discord_bot_token', ?)")
        .bind(&token).execute(state.orchestrator.pool()).await;

    // 立即启动 Gateway
    let pool = state.orchestrator.pool().clone();
    let orch = state.orchestrator.clone();
    let handle = app_handle.clone();
    let t = token.clone();
    tokio::spawn(async move {
        let _ = channels::discord::start_discord(
            channels::discord::DiscordConfig { bot_token: t, agent_id: String::new() },
            pool, orch, handle,
            tokio_util::sync::CancellationToken::new(),
        ).await;
    });

    log::info!("Discord: 已连接 Bot {}#{}", username, discriminator);
    Ok(serde_json::json!({
        "ok": true,
        "username": username,
        "discriminator": discriminator,
        "id": data["id"],
    }))
}

/// 验证 Slack Token 并保存 + 启动 Socket Mode
#[tauri::command]
pub async fn slack_connect(
    state: State<'_, Arc<AppState>>,
    app_handle: tauri::AppHandle,
    bot_token: String,
    app_token: String,
) -> Result<serde_json::Value, String> {
    let bt = bot_token.trim().to_string();
    let at = app_token.trim().to_string();

    // 验证 Bot Token
    let client = reqwest::Client::new();
    let resp = client.post("https://slack.com/api/auth.test")
        .header("Authorization", format!("Bearer {}", bt))
        .send().await.map_err(|e| format!("请求失败: {}", e))?;

    let data: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析失败: {}", e))?;

    if data["ok"].as_bool() != Some(true) {
        return Ok(serde_json::json!({
            "ok": false,
            "error": data["error"].as_str().unwrap_or("Invalid bot token"),
        }));
    }

    let team = data["team"].as_str().unwrap_or("");
    let user = data["user"].as_str().unwrap_or("");

    // 验证 App Token（尝试获取 WebSocket URL）
    let ws_resp = client.post("https://slack.com/api/apps.connections.open")
        .header("Authorization", format!("Bearer {}", at))
        .send().await.map_err(|e| format!("App Token 验证失败: {}", e))?;

    let ws_data: serde_json::Value = ws_resp.json().await
        .map_err(|e| format!("解析失败: {}", e))?;

    if ws_data["ok"].as_bool() != Some(true) {
        return Ok(serde_json::json!({
            "ok": false,
            "error": format!("App Token 无效: {}", ws_data["error"].as_str().unwrap_or("unknown")),
        }));
    }

    // 保存 Token
    let _ = sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('slack_bot_token', ?)")
        .bind(&bt).execute(state.orchestrator.pool()).await;
    let _ = sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('slack_app_token', ?)")
        .bind(&at).execute(state.orchestrator.pool()).await;

    // 立即启动 Socket Mode
    let pool = state.orchestrator.pool().clone();
    let orch = state.orchestrator.clone();
    let handle = app_handle.clone();
    tokio::spawn(async move {
        let _ = channels::slack::start_slack(
            channels::slack::SlackConfig { bot_token: bt, app_token: at, agent_id: String::new() },
            pool, orch, handle,
            tokio_util::sync::CancellationToken::new(),
        ).await;
    });

    log::info!("Slack: 已连接 team={}, bot={}", team, user);
    Ok(serde_json::json!({
        "ok": true,
        "team": team,
        "user": user,
    }))
}

/// 通过渠道发送投票/Poll
#[tauri::command]
pub async fn send_poll(
    state: State<'_, Arc<AppState>>,
    channel: String,
    chat_id: String,
    question: String,
    options: Vec<String>,
    is_anonymous: Option<bool>,
    allows_multiple: Option<bool>,
) -> Result<String, String> {
    let client = reqwest::Client::new();

    match channel.as_str() {
        "telegram" => {
            let token: String = sqlx::query_scalar(
                "SELECT value FROM settings WHERE key = 'telegram_bot_token'"
            ).fetch_optional(state.db.pool()).await.ok().flatten()
            .ok_or("Telegram token 未配置")?;

            let chat_id_num: i64 = chat_id.parse().map_err(|_| "无效的 chat_id")?;

            let resp = client.post(format!("https://api.telegram.org/bot{}/sendPoll", token))
                .json(&serde_json::json!({
                    "chat_id": chat_id_num,
                    "question": question,
                    "options": options,
                    "is_anonymous": is_anonymous.unwrap_or(true),
                    "allows_multiple_answers": allows_multiple.unwrap_or(false),
                }))
                .send().await.map_err(|e| format!("发送失败: {}", e))?;

            let data: serde_json::Value = resp.json().await.unwrap_or_default();
            if data["ok"].as_bool() == Some(true) {
                Ok("Telegram 投票已发送".into())
            } else {
                Err(format!("Telegram 投票失败: {}", data["description"].as_str().unwrap_or("未知错误")))
            }
        }
        "discord" => {
            let token: String = sqlx::query_scalar(
                "SELECT value FROM settings WHERE key = 'discord_bot_token'"
            ).fetch_optional(state.db.pool()).await.ok().flatten()
            .ok_or("Discord token 未配置")?;

            // Discord 没有原生 Poll API，用 reaction 模拟
            let emoji_list = ["1\u{fe0f}\u{20e3}", "2\u{fe0f}\u{20e3}", "3\u{fe0f}\u{20e3}", "4\u{fe0f}\u{20e3}", "5\u{fe0f}\u{20e3}", "6\u{fe0f}\u{20e3}", "7\u{fe0f}\u{20e3}", "8\u{fe0f}\u{20e3}", "9\u{fe0f}\u{20e3}", "\u{1f51f}"];
            let mut poll_text = format!("\u{1f4ca} **{}**\n\n", question);
            for (i, opt) in options.iter().enumerate().take(10) {
                poll_text.push_str(&format!("{} {}\n", emoji_list.get(i).unwrap_or(&"\u{25aa}\u{fe0f}"), opt));
            }

            let resp = client.post(format!("https://discord.com/api/v10/channels/{}/messages", chat_id))
                .header("Authorization", format!("Bot {}", token))
                .header("Content-Type", "application/json")
                .json(&serde_json::json!({"content": poll_text}))
                .send().await.map_err(|e| format!("发送失败: {}", e))?;

            let data: serde_json::Value = resp.json().await.unwrap_or_default();
            if let Some(msg_id) = data["id"].as_str() {
                // 自动添加 reaction
                for (i, _) in options.iter().enumerate().take(10) {
                    let emoji = emoji_list.get(i).unwrap_or(&"\u{25aa}\u{fe0f}");
                    let encoded = urlencoding::encode(emoji);
                    let _ = client.put(format!(
                        "https://discord.com/api/v10/channels/{}/messages/{}/reactions/{}/@me",
                        chat_id, msg_id, encoded
                    ))
                    .header("Authorization", format!("Bot {}", token))
                    .send().await;
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await; // Discord rate limit
                }
                Ok("Discord 投票已发送（reaction 模式）".into())
            } else {
                Err(format!("Discord 发送失败: {}", data))
            }
        }
        _ => Err(format!("渠道 {} 不支持投票功能。支持: telegram/discord", channel)),
    }
}
