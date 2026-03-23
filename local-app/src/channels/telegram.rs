//! Telegram Bot 本地轮询
//!
//! 在桌面端直接轮询 Telegram API，消息本地处理（零延迟）。
//! 参考 OpenClaw：Telegram 轮询始终在能力最强的端执行。

use std::sync::Arc;
use crate::agent::Orchestrator;

/// Telegram Bot 配置
pub struct TelegramConfig {
    pub bot_token: String,
}

static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 启动 Telegram 长轮询（后台 tokio task，单例）
pub async fn start_polling(
    config: TelegramConfig,
    pool: sqlx::SqlitePool,
    orchestrator: Arc<Orchestrator>,
    app_handle: tauri::AppHandle,
) {
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        log::info!("Telegram: 轮询已在运行，跳过");
        return;
    }
    let token = config.bot_token.clone();
    log::info!("Telegram: 启动本地轮询 (token: {}...)", &token[..token.len().min(15)]);

    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let mut offset: i64 = 0;

        log::info!("Telegram: 轮询 loop 已进入");

        loop {
            let url = format!(
                "https://api.telegram.org/bot{}/getUpdates?offset={}&timeout=30",
                token, offset
            );

            match client.get(&url).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    match resp.json::<serde_json::Value>().await {
                        Ok(data) => {
                            if data["ok"].as_bool() != Some(true) {
                                log::warn!("Telegram: API 返回错误: {}", data);
                                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                continue;
                            }
                            if let Some(updates) = data["result"].as_array() {
                                if !updates.is_empty() {
                                    log::info!("Telegram: 收到 {} 条更新", updates.len());
                                }
                                for update in updates {
                                    offset = update["update_id"].as_i64().unwrap_or(0) + 1;
                                    // 并发处理：不阻塞轮询 loop
                                    let t = token.clone();
                                    let p = pool.clone();
                                    let o = orchestrator.clone();
                                    let h = app_handle.clone();
                                    let u = update.clone();
                                    tokio::spawn(async move {
                                        handle_update(&t, &u, &p, &o, &h).await;
                                    });
                                }
                            } else {
                                log::warn!("Telegram: 响应缺少 result 字段: {}", &data.to_string()[..data.to_string().len().min(200)]);
                            }
                        }
                        Err(e) => {
                            log::warn!("Telegram: JSON 解析失败 (status={}): {}", status, e);
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Telegram: 轮询请求失败: {}，10秒后重试", e);
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                }
            }
        }
    });
}

/// 处理单条 Telegram 消息
async fn handle_update(
    token: &str,
    update: &serde_json::Value,
    pool: &sqlx::SqlitePool,
    orchestrator: &Arc<Orchestrator>,
    app_handle: &tauri::AppHandle,
) {
    let msg = match update.get("message") {
        Some(m) => m,
        None => return,
    };
    let text = match msg["text"].as_str() {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => return,
    };

    let chat_id = msg["chat"]["id"].as_i64().unwrap_or(0);
    let user_name = msg["from"]["first_name"].as_str().unwrap_or("User");

    log::info!("Telegram: [{}] {}: {}", chat_id, user_name, &text[..text.len().min(50)]);

    // 获取本地 Agent
    let agent = match orchestrator.list_agents().await {
        Ok(agents) => match agents.into_iter().next() {
            Some(a) => a,
            None => { log::warn!("Telegram: 无可用 Agent"); return; }
        },
        Err(_) => return,
    };

    // 获取或创建 session
    let session_title = format!("[Telegram] {}", user_name);
    let session_id = get_or_create_session(pool, &agent.id, chat_id, &session_title).await;

    // 发送 typing 状态
    send_typing(token, chat_id).await;

    // 查找 Provider
    let (api_type, api_key, base_url) = match super::find_provider(pool, &agent.model).await {
        Some(info) => info,
        None => {
            send_message(token, chat_id, "未配置 LLM Provider，请在桌面端设置中添加。").await;
            return;
        }
    };

    use tauri::Manager;

    // 直接推送用户消息到前端（不经过 DB 读取，像 OpenClaw 一样）
    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "message",
        "sessionId": session_id,
        "role": "user",
        "content": text,
        "source": "telegram",
    }));

    // 推送"思考中"状态
    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "thinking",
        "sessionId": session_id,
        "source": "telegram",
    }));

    // 调用本地 orchestrator
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // 收集输出 + 推送流式 token（带 session 标识，不会污染其他会话）
    let app_for_stream = app_handle.clone();
    let sid_for_stream = session_id.clone();
    let output_handle = tokio::spawn(async move {
        let mut output = String::new();
        while let Some(token) = rx.recv().await {
            output.push_str(&token);
            // 带 sessionId 的流式 token（前端只处理匹配的 session）
            let _ = app_for_stream.emit_all("chat-event", serde_json::json!({
                "type": "token",
                "sessionId": sid_for_stream,
                "content": output.clone(),
                "source": "telegram",
            }));
        }
        output
    });

    let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };

    let result = orchestrator.send_message_stream(
        &agent.id, &session_id, &text,
        &api_key, &api_type, base_url_opt, tx, None,
    ).await;

    let response = output_handle.await.unwrap_or_default();

    let reply = match result {
        Ok(resp) => {
            let r = if resp.is_empty() { response.clone() } else { resp };
            if !r.is_empty() {
                send_message(token, chat_id, &r).await;
                log::info!("Telegram: 回复 [{}] {}字符", chat_id, r.len());
            }
            r
        }
        Err(e) => {
            log::error!("Telegram: 处理失败: {}", e);
            let err_msg = format!("处理出错: {}", &e[..e.len().min(100)]);
            send_message(token, chat_id, &err_msg).await;
            err_msg
        }
    };

    // 推送完整回复到前端（直接携带内容，不用读 DB）
    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "done",
        "sessionId": session_id,
        "role": "assistant",
        "content": reply,
        "source": "telegram",
    }));
}

/// 获取或创建 Telegram session
async fn get_or_create_session(pool: &sqlx::SqlitePool, agent_id: &str, chat_id: i64, title: &str) -> String {
    let tag = format!("tg-{}", chat_id);

    // 先查有没有已存在的
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM chat_sessions WHERE title LIKE '%' || ? || '%' OR title = ? LIMIT 1"
    ).bind(&tag).bind(title).fetch_optional(pool).await.ok().flatten();

    if let Some((id,)) = existing {
        return id;
    }

    // 创建新 session
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let _ = sqlx::query(
        "INSERT INTO chat_sessions (id, agent_id, title, created_at) VALUES (?, ?, ?, ?)"
    ).bind(&id).bind(agent_id).bind(title).bind(now).execute(pool).await;

    id
}

/// 发送 typing 状态
async fn send_typing(token: &str, chat_id: i64) {
    let client = reqwest::Client::new();
    let _ = client.post(format!("https://api.telegram.org/bot{}/sendChatAction", token))
        .json(&serde_json::json!({"chat_id": chat_id, "action": "typing"}))
        .send().await;
}

/// 发送消息到 Telegram（Markdown → HTML 渲染）
async fn send_message(token: &str, chat_id: i64, text: &str) {
    let client = reqwest::Client::new();
    let html = markdown_to_telegram_html(text);

    // 先尝试 HTML 格式（比 Markdown 更宽容）
    let resp = client.post(format!("https://api.telegram.org/bot{}/sendMessage", token))
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "text": html,
            "parse_mode": "HTML",
            "disable_web_page_preview": true,
        }))
        .send().await;

    // HTML 失败则降级纯文本
    if let Ok(r) = resp {
        if let Ok(body) = r.json::<serde_json::Value>().await {
            if body["ok"].as_bool() != Some(true) {
                let _ = client.post(format!("https://api.telegram.org/bot{}/sendMessage", token))
                    .json(&serde_json::json!({"chat_id": chat_id, "text": text}))
                    .send().await;
            }
        }
    }
}

/// 将标准 Markdown 转为 Telegram HTML
///
/// Telegram HTML 支持: <b> <i> <code> <pre> <a> <s> <u> <blockquote>
fn markdown_to_telegram_html(md: &str) -> String {
    let mut html = String::with_capacity(md.len() * 2);
    let mut in_code_block = false;
    let mut _code_lang = String::new();

    for line in md.lines() {
        // 代码块
        if line.trim_start().starts_with("```") {
            if in_code_block {
                html.push_str("</code></pre>\n");
                in_code_block = false;
            } else {
                _code_lang = line.trim_start().trim_start_matches('`').to_string();
                if _code_lang.is_empty() {
                    html.push_str("<pre><code>");
                } else {
                    html.push_str(&format!("<pre><code class=\"language-{}\">", escape_html(&_code_lang)));
                }
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            html.push_str(&escape_html(line));
            html.push('\n');
            continue;
        }

        // 标题 → 加粗
        let trimmed = line.trim_start();
        if trimmed.starts_with("### ") {
            html.push_str(&format!("<b>{}</b>\n", escape_html(&trimmed[4..])));
            continue;
        }
        if trimmed.starts_with("## ") {
            html.push_str(&format!("\n<b>{}</b>\n", escape_html(&trimmed[3..])));
            continue;
        }
        if trimmed.starts_with("# ") {
            html.push_str(&format!("\n<b>{}</b>\n", escape_html(&trimmed[2..])));
            continue;
        }

        // 行内格式转换
        let processed = process_inline_markdown(line);
        html.push_str(&processed);
        html.push('\n');
    }

    if in_code_block {
        html.push_str("</code></pre>\n");
    }

    html.trim().to_string()
}

/// 处理行内 Markdown 格式
fn process_inline_markdown(line: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // 行内代码 `code`
        if chars[i] == '`' && !matches!(chars.get(i+1), Some(&'`')) {
            if let Some(end) = chars[i+1..].iter().position(|&c| c == '`') {
                let code: String = chars[i+1..i+1+end].iter().collect();
                result.push_str(&format!("<code>{}</code>", escape_html(&code)));
                i += end + 2;
                continue;
            }
        }

        // 加粗 **text**
        if i + 1 < len && chars[i] == '*' && chars[i+1] == '*' {
            if let Some(end) = find_closing(&chars, i+2, "**") {
                let inner: String = chars[i+2..end].iter().collect();
                result.push_str(&format!("<b>{}</b>", escape_html(&inner)));
                i = end + 2;
                continue;
            }
        }

        // 斜体 *text* 或 _text_
        if (chars[i] == '*' || chars[i] == '_') && i + 1 < len && chars[i+1] != ' ' {
            let marker = chars[i];
            if let Some(end) = chars[i+1..].iter().position(|&c| c == marker) {
                let inner: String = chars[i+1..i+1+end].iter().collect();
                if !inner.is_empty() && !inner.contains(' ') || marker == '*' {
                    result.push_str(&format!("<i>{}</i>", escape_html(&inner)));
                    i += end + 2;
                    continue;
                }
            }
        }

        // 链接 [text](url)
        if chars[i] == '[' {
            if let Some(close_bracket) = chars[i+1..].iter().position(|&c| c == ']') {
                let text_end = i + 1 + close_bracket;
                if text_end + 1 < len && chars[text_end + 1] == '(' {
                    if let Some(close_paren) = chars[text_end+2..].iter().position(|&c| c == ')') {
                        let link_text: String = chars[i+1..text_end].iter().collect();
                        let url: String = chars[text_end+2..text_end+2+close_paren].iter().collect();
                        result.push_str(&format!("<a href=\"{}\">{}</a>", escape_html(&url), escape_html(&link_text)));
                        i = text_end + 2 + close_paren + 1;
                        continue;
                    }
                }
            }
        }

        // 删除线 ~~text~~
        if i + 1 < len && chars[i] == '~' && chars[i+1] == '~' {
            if let Some(end) = find_closing(&chars, i+2, "~~") {
                let inner: String = chars[i+2..end].iter().collect();
                result.push_str(&format!("<s>{}</s>", escape_html(&inner)));
                i = end + 2;
                continue;
            }
        }

        // 普通字符（HTML 转义）
        match chars[i] {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' if trimmed_starts_with_quote(line) && i == 0 => {
                // blockquote
                result.push_str("<blockquote>");
                // 跳过 > 和空格
                i += 1;
                if i < len && chars[i] == ' ' { i += 1; }
                let rest: String = chars[i..].iter().collect();
                result.push_str(&escape_html(&rest));
                result.push_str("</blockquote>");
                return result;
            }
            '>' => result.push_str("&gt;"),
            _ => result.push(chars[i]),
        }
        i += 1;
    }

    result
}

fn trimmed_starts_with_quote(line: &str) -> bool {
    line.trim_start().starts_with('>')
}

fn find_closing(chars: &[char], start: usize, marker: &str) -> Option<usize> {
    let marker_chars: Vec<char> = marker.chars().collect();
    let mlen = marker_chars.len();
    for i in start..chars.len().saturating_sub(mlen - 1) {
        if chars[i..i+mlen] == marker_chars[..] {
            return Some(i);
        }
    }
    None
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
