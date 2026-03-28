//! 对话历史存储和检索

use crate::db::models::ChatSession;
use sqlx::{SqlitePool, Row};

// ─── Session CRUD ─────────────────────────────────────────────

/// 创建会话
pub async fn create_session(
    pool: &SqlitePool,
    agent_id: &str,
    title: &str,
) -> Result<ChatSession, sqlx::Error> {
    let session = ChatSession::new(agent_id.to_string(), title.to_string());
    sqlx::query(
        r#"
        INSERT INTO chat_sessions (id, agent_id, title, created_at, last_message_at, summary)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&session.id)
    .bind(&session.agent_id)
    .bind(&session.title)
    .bind(session.created_at)
    .bind(session.last_message_at)
    .bind(&session.summary)
    .execute(pool)
    .await?;

    log::info!("会话已创建: agent_id={}, session_id={}", agent_id, session.id);
    Ok(session)
}

/// 获取单个会话
pub async fn get_session(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Option<ChatSession>, sqlx::Error> {
    sqlx::query_as::<_, ChatSession>("SELECT * FROM chat_sessions WHERE id = ?")
        .bind(session_id)
        .fetch_optional(pool)
        .await
}

/// 列出 Agent 的所有会话（按最后消息时间降序）
pub async fn list_sessions(
    pool: &SqlitePool,
    agent_id: &str,
) -> Result<Vec<ChatSession>, sqlx::Error> {
    sqlx::query_as::<_, ChatSession>(
        r#"
        SELECT * FROM chat_sessions
        WHERE agent_id = ?
        ORDER BY COALESCE(last_message_at, created_at) DESC
        "#,
    )
    .bind(agent_id)
    .fetch_all(pool)
    .await
}

/// 重命名会话
pub async fn rename_session(
    pool: &SqlitePool,
    session_id: &str,
    title: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE chat_sessions SET title = ? WHERE id = ?")
        .bind(title)
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 删除会话（事务化级联删除消息）
pub async fn delete_session(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM conversations WHERE session_id = ?")
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM chat_messages WHERE session_id = ?")
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM chat_sessions WHERE id = ?")
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    log::info!("会话已删除: session_id={}", session_id);
    Ok(())
}

/// 更新会话摘要
pub async fn update_session_summary(
    pool: &SqlitePool,
    session_id: &str,
    summary: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE chat_sessions SET summary = ? WHERE id = ?")
        .bind(summary)
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 更新会话的 last_message_at
pub async fn touch_session(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query("UPDATE chat_sessions SET last_message_at = ? WHERE id = ?")
        .bind(now)
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 自动为 Session 生成标题（LLM fire-and-forget）
///
/// 在第一轮对话完成后异步调用 LLM 生成 2-4 词标题。
/// 不阻塞对话流程，静默失败。
pub async fn auto_name_session(
    pool: &SqlitePool,
    session_id: &str,
    user_message: &str,
    api_key: &str,
    provider: &str,
    base_url: Option<&str>,
) {
    // 仅在第一轮对话时触发（消息数 <= 1）
    let msg_count = get_session_message_count(pool, session_id).await.unwrap_or(99);
    if msg_count > 1 {
        return;
    }

    // 检查当前标题是否是默认/通用标题（已手动命名的不覆盖）
    let current_title: Option<String> = sqlx::query_scalar(
        "SELECT title FROM chat_sessions WHERE id = ?"
    ).bind(session_id).fetch_optional(pool).await.ok().flatten();

    if let Some(ref title) = current_title {
        let is_default = title == "New Session"
            || title.starts_with("[Telegram]")
            || title.starts_with("[飞书]")
            || title.starts_with("[微信]")
            || title.starts_with("[Discord]")
            || title.starts_with("[Slack]")
            || title.starts_with("Conversation");
        if !is_default {
            return; // 用户已手动命名
        }
    }

    let pool = pool.clone();
    let session_id = session_id.to_string();
    let user_msg = user_message.chars().take(500).collect::<String>();
    let api_key = api_key.to_string();
    let provider = provider.to_string();
    let base_url = base_url.map(|s| s.to_string());
    let channel_prefix = current_title.as_deref()
        .and_then(|t| {
            if t.starts_with('[') {
                t.find(']').map(|i| &t[..=i])
            } else { None }
        })
        .map(|s| s.to_string());

    // fire-and-forget：不阻塞对话
    tokio::spawn(async move {
        match generate_session_title(&api_key, &provider, base_url.as_deref(), &user_msg).await {
            Ok(title) => {
                let final_title = if let Some(prefix) = channel_prefix {
                    format!("{} {}", prefix, title)
                } else {
                    title.clone()
                };
                if let Err(e) = rename_session(&pool, &session_id, &final_title).await {
                    log::warn!("Session 自动命名失败: {}", e);
                } else {
                    log::info!("Session 自动命名: {} → {}", session_id, final_title);
                }
            }
            Err(e) => log::debug!("Session 标题生成失败（静默）: {}", e),
        }
    });
}

/// 调 LLM 生成 session 标题（2-4 词，与消息同语言）
async fn generate_session_title(
    api_key: &str,
    provider: &str,
    base_url: Option<&str>,
    user_message: &str,
) -> Result<String, String> {
    let prompt = format!(
        "Generate a very short topic label (2-5 words, max 30 chars) for a chat based on the user's first message below. \
         No emoji. Use the same language as the message. Be concise and descriptive. Return ONLY the topic name, nothing else.\n\n{}",
        user_message
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let url = match base_url {
        Some(u) => format!("{}/chat/completions", u.trim_end_matches('/')),
        None => match provider {
            "anthropic" => "https://api.anthropic.com/v1/messages".to_string(),
            _ => "https://api.openai.com/v1/chat/completions".to_string(),
        }
    };

    // OpenAI-compatible API（大多数 provider 都支持）
    let body = serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": 50,
        "temperature": 0.3,
    });

    let resp = client.post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send().await
        .map_err(|e| format!("请求失败: {}", e))?;

    let data: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析失败: {}", e))?;

    let title = data["choices"][0]["message"]["content"].as_str()
        .or(data["content"][0]["text"].as_str()) // Anthropic 格式
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .chars().take(30).collect::<String>();

    if title.is_empty() {
        Err("LLM 返回空标题".into())
    } else {
        Ok(title)
    }
}

/// 获取会话消息数
pub async fn get_session_message_count(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query("SELECT COUNT(*) as cnt FROM conversations WHERE session_id = ?")
        .bind(session_id)
        .fetch_one(pool)
        .await?;
    Ok(row.get::<i64, _>("cnt"))
}

// ─── 对话 CRUD（按 session） ──────────────────────────────────

/// 保存对话
pub async fn save_conversation(
    pool: &SqlitePool,
    agent_id: &str,
    session_id: &str,
    user_message: &str,
    agent_response: &str,
) -> Result<(), sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();

    sqlx::query(
        r#"
        INSERT INTO conversations
        (id, agent_id, user_id, user_message, agent_response, created_at, updated_at, session_id)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(agent_id)
    .bind("system")
    .bind(user_message)
    .bind(agent_response)
    .bind(now)
    .bind(now)
    .bind(session_id)
    .execute(pool)
    .await?;

    // 更新会话的 last_message_at
    touch_session(pool, session_id).await?;

    log::info!("对话已保存: agent_id={}, session_id={}, conversation_id={}", agent_id, session_id, id);
    Ok(())
}

/// 保存用户消息（agent_response 暂为空），返回 conversation_id
pub async fn save_user_message(
    pool: &SqlitePool,
    agent_id: &str,
    session_id: &str,
    user_message: &str,
) -> Result<String, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();

    sqlx::query(
        r#"
        INSERT INTO conversations
        (id, agent_id, user_id, user_message, agent_response, created_at, updated_at, session_id)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(agent_id)
    .bind("system")
    .bind(user_message)
    .bind("")
    .bind(now)
    .bind(now)
    .bind(session_id)
    .execute(pool)
    .await?;

    touch_session(pool, session_id).await?;

    log::info!("用户消息已保存: agent_id={}, session_id={}, conversation_id={}", agent_id, session_id, id);
    Ok(id)
}

/// 更新 AI 回复
pub async fn update_agent_response(
    pool: &SqlitePool,
    conversation_id: &str,
    agent_response: &str,
) -> Result<(), sqlx::Error> {
    let now = chrono::Utc::now().timestamp_millis();
    let result = sqlx::query("UPDATE conversations SET agent_response = ?, updated_at = ? WHERE id = ?")
        .bind(agent_response)
        .bind(now)
        .bind(conversation_id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }

    log::info!("AI回复已更新: conversation_id={}", conversation_id);
    Ok(())
}

/// 获取对话历史（按 session）
pub async fn get_history(
    pool: &SqlitePool,
    agent_id: &str,
    session_id: &str,
    limit: i64,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT user_message, agent_response
        FROM conversations
        WHERE agent_id = ? AND session_id = ?
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )
    .bind(agent_id)
    .bind(session_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let history = rows
        .into_iter()
        .map(|row| {
            let user_msg: String = row.get(0);
            let agent_resp: String = row.get(1);
            (user_msg, agent_resp)
        })
        .collect();

    Ok(history)
}

/// 清除会话的对话历史
pub async fn clear_session_history(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM conversations WHERE session_id = ?")
        .bind(session_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM chat_messages WHERE session_id = ?")
        .bind(session_id)
        .execute(pool)
        .await?;
    log::info!("会话对话历史已清除: session_id={}", session_id);
    Ok(())
}

/// 删除会话中的旧消息，保留最近 N 条
pub async fn delete_old_session_messages(
    pool: &SqlitePool,
    session_id: &str,
    keep_recent: i64,
) -> Result<(), sqlx::Error> {
    // 清理 conversations 表
    sqlx::query(
        r#"
        DELETE FROM conversations WHERE session_id = ? AND id NOT IN (
            SELECT id FROM conversations WHERE session_id = ?
            ORDER BY created_at DESC LIMIT ?
        )
        "#,
    )
    .bind(session_id)
    .bind(session_id)
    .bind(keep_recent)
    .execute(pool)
    .await?;

    // 同时清理 chat_messages 表（保留最近 keep_recent * 10 条结构化消息）
    let keep_structured = keep_recent * 10;
    sqlx::query(
        r#"
        DELETE FROM chat_messages WHERE session_id = ? AND id NOT IN (
            SELECT id FROM chat_messages WHERE session_id = ?
            ORDER BY seq DESC LIMIT ?
        )
        "#,
    )
    .bind(session_id)
    .bind(session_id)
    .bind(keep_structured)
    .execute(pool)
    .await?;

    Ok(())
}

/// 清理所有会话中超出阈值的旧消息（定期调用，防止 DB 无限增长）
pub async fn truncate_all_sessions(
    pool: &SqlitePool,
    max_messages_per_session: i64,
) -> Result<u64, sqlx::Error> {
    // 找出消息数超标的 session
    let sessions: Vec<(String, i64)> = sqlx::query_as(
        "SELECT session_id, COUNT(*) as cnt FROM chat_messages GROUP BY session_id HAVING cnt > ?"
    )
    .bind(max_messages_per_session)
    .fetch_all(pool)
    .await?;

    let mut total_deleted = 0u64;
    for (sid, _count) in &sessions {
        let result = sqlx::query(
            "DELETE FROM chat_messages WHERE session_id = ? AND id NOT IN (SELECT id FROM chat_messages WHERE session_id = ? ORDER BY seq DESC LIMIT ?)"
        )
        .bind(sid).bind(sid).bind(max_messages_per_session)
        .execute(pool)
        .await?;
        total_deleted += result.rows_affected();
    }

    if total_deleted > 0 {
        log::info!("自动截断: {} 个会话共清理 {} 条旧消息", sessions.len(), total_deleted);
    }
    Ok(total_deleted)
}

/// 删除旧对话
pub async fn delete_old_conversations(
    pool: &SqlitePool,
    agent_id: &str,
    days: i64,
) -> Result<(), sqlx::Error> {
    let cutoff_time = chrono::Utc::now().timestamp_millis() - (days * 24 * 60 * 60 * 1000);

    sqlx::query(
        "DELETE FROM conversations WHERE agent_id = ? AND created_at < ?",
    )
    .bind(agent_id)
    .bind(cutoff_time)
    .execute(pool)
    .await?;

    log::info!("已删除 {} 天前的对话", days);
    Ok(())
}

// ─── 结构化消息存储（完整消息序列，含工具调用） ─────────────

/// 保存一条结构化消息到 chat_messages 表
pub async fn save_chat_message(
    pool: &SqlitePool,
    session_id: &str,
    agent_id: &str,
    message: &serde_json::Value,
) -> Result<String, sqlx::Error> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();

    let role = message["role"].as_str().unwrap_or("user").to_string();
    let content = message["content"].as_str().map(|s| s.to_string())
        .or_else(|| {
            // Anthropic content 数组格式：提取文本部分
            message["content"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|b| {
                        if b["type"].as_str() == Some("text") { b["text"].as_str().map(|s| s.to_string()) }
                        else { None }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
        });

    // 序列化 tool_calls（如果存在）
    let tool_calls_json = if message.get("tool_calls").is_some() && !message["tool_calls"].is_null() {
        Some(message["tool_calls"].to_string())
    } else if let Some(content_arr) = message["content"].as_array() {
        // Anthropic 格式：tool_use 在 content 数组中
        let tool_uses: Vec<&serde_json::Value> = content_arr.iter()
            .filter(|b| b["type"].as_str() == Some("tool_use"))
            .collect();
        if !tool_uses.is_empty() {
            Some(serde_json::to_string(&tool_uses).unwrap_or_default())
        } else {
            None
        }
    } else {
        None
    };

    let tool_call_id = message["tool_call_id"].as_str().map(|s| s.to_string())
        .or_else(|| {
            // Anthropic tool_result 格式
            message["content"].as_array().and_then(|arr| {
                arr.iter().find_map(|b| {
                    if b["type"].as_str() == Some("tool_result") {
                        b["tool_use_id"].as_str().map(|s| s.to_string())
                    } else { None }
                })
            })
        });

    let tool_name = message["name"].as_str().map(|s| s.to_string());

    // 获取下一个 seq
    let seq_row = sqlx::query("SELECT COALESCE(MAX(seq), 0) + 1 as next_seq FROM chat_messages WHERE session_id = ?")
        .bind(session_id)
        .fetch_one(pool)
        .await?;
    let seq: i64 = seq_row.get("next_seq");

    sqlx::query(
        r#"
        INSERT INTO chat_messages (id, session_id, agent_id, role, content, tool_calls_json, tool_call_id, tool_name, seq, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(session_id)
    .bind(agent_id)
    .bind(&role)
    .bind(&content)
    .bind(&tool_calls_json)
    .bind(&tool_call_id)
    .bind(&tool_name)
    .bind(seq)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(id)
}

/// 批量保存多条结构化消息
pub async fn save_chat_messages(
    pool: &SqlitePool,
    session_id: &str,
    agent_id: &str,
    messages: &[serde_json::Value],
) -> Result<(), sqlx::Error> {
    for msg in messages {
        save_chat_message(pool, session_id, agent_id, msg).await?;
    }
    touch_session(pool, session_id).await?;
    Ok(())
}

/// 加载结构化消息历史（返回完整 JSON 消息列表）
///
/// DB 加载只做数据读取，不截断。截断统一由 ContextGuard 在 call_stream 前处理。
/// limit 控制最多加载的消息条数（默认 30）。
pub async fn load_chat_messages(
    pool: &SqlitePool,
    session_id: &str,
    limit: i64,
) -> Result<Vec<serde_json::Value>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT role, content, tool_calls_json, tool_call_id, tool_name, seq
        FROM chat_messages
        WHERE session_id = ?
        ORDER BY seq DESC
        LIMIT ?
        "#,
    )
    .bind(session_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    // 反转为时间正序
    let all_rows: Vec<_> = rows.into_iter().rev().collect();
    let mut messages: Vec<serde_json::Value> = Vec::new();

    for row in all_rows {
        let role: String = row.get("role");
        let content: Option<String> = row.get("content");
        let tool_calls_json: Option<String> = row.get("tool_calls_json");
        let tool_call_id: Option<String> = row.get("tool_call_id");
        let tool_name: Option<String> = row.get("tool_name");

        // 跳过 system 消息（每次都重新构建）
        if role == "system" { continue; }

        let mut msg = serde_json::json!({"role": role});

        match role.as_str() {
            "tool" => {
                if let Some(ref tcid) = tool_call_id {
                    msg["tool_call_id"] = serde_json::json!(tcid);
                }
                if let Some(ref name) = tool_name {
                    msg["name"] = serde_json::json!(name);
                }
                // 完整内容，不截断（ContextGuard 会处理）
                msg["content"] = serde_json::json!(content.unwrap_or_default());
            }
            "assistant" => {
                if let Some(ref c) = content {
                    if !c.is_empty() {
                        msg["content"] = serde_json::json!(c);
                    } else {
                        msg["content"] = serde_json::Value::Null;
                    }
                } else {
                    msg["content"] = serde_json::Value::Null;
                }
                // 恢复 tool_calls
                if let Some(ref tc_json) = tool_calls_json {
                    if let Ok(tc) = serde_json::from_str::<serde_json::Value>(tc_json) {
                        msg["tool_calls"] = tc;
                    }
                }
            }
            "user" => {
                if let Some(ref tcid) = tool_call_id {
                    // Anthropic tool_result 格式
                    let raw = content.unwrap_or_default();
                    msg["content"] = serde_json::json!([{
                        "type": "tool_result",
                        "tool_use_id": tcid,
                        "content": raw
                    }]);
                } else {
                    msg["content"] = serde_json::json!(content.unwrap_or_default());
                }
            }
            _ => {
                msg["content"] = serde_json::json!(content.unwrap_or_default());
            }
        }

        messages.push(msg);
    }

    Ok(messages)
}

/// 获取 session 的结构化消息数量
pub async fn get_chat_message_count(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query("SELECT COUNT(*) as cnt FROM chat_messages WHERE session_id = ?")
        .bind(session_id)
        .fetch_one(pool)
        .await?;
    Ok(row.get::<i64, _>("cnt"))
}

/// 清除会话的结构化消息
pub async fn clear_chat_messages(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM chat_messages WHERE session_id = ?")
        .bind(session_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 删除旧的结构化消息，保留最近 N 条
pub async fn delete_old_chat_messages(
    pool: &SqlitePool,
    session_id: &str,
    keep_recent: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        DELETE FROM chat_messages WHERE session_id = ? AND id NOT IN (
            SELECT id FROM chat_messages WHERE session_id = ?
            ORDER BY seq DESC LIMIT ?
        )
        "#,
    )
    .bind(session_id)
    .bind(session_id)
    .bind(keep_recent)
    .execute(pool)
    .await?;
    Ok(())
}

/// 搜索对话
pub async fn search_conversations(
    pool: &SqlitePool,
    agent_id: &str,
    keyword: &str,
    limit: i64,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    let search_pattern = format!("%{}%", keyword);

    let rows = sqlx::query(
        r#"
        SELECT user_message, agent_response
        FROM conversations
        WHERE agent_id = ? AND (user_message LIKE ? OR agent_response LIKE ?)
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )
    .bind(agent_id)
    .bind(&search_pattern)
    .bind(&search_pattern)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let results = rows
        .into_iter()
        .map(|row| {
            let user_msg: String = row.get(0);
            let agent_resp: String = row.get(1);
            (user_msg, agent_resp)
        })
        .collect();

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::db::schema::init_schema(&pool).await.unwrap();
        // 插入测试用 agent（满足外键约束）
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query("INSERT INTO agents (id, name, system_prompt, model, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)")
            .bind("agent1").bind("Test").bind("prompt").bind("gpt-4").bind(now).bind(now)
            .execute(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn test_session_crud() {
        let pool = setup_pool().await;

        // 创建会话
        let session = create_session(&pool, "agent1", "Test Session").await.unwrap();
        assert_eq!(session.title, "Test Session");
        assert_eq!(session.agent_id, "agent1");

        // 列出会话
        let sessions = list_sessions(&pool, "agent1").await.unwrap();
        assert_eq!(sessions.len(), 1);

        // 重命名
        rename_session(&pool, &session.id, "Renamed").await.unwrap();
        let s = get_session(&pool, &session.id).await.unwrap().unwrap();
        assert_eq!(s.title, "Renamed");

        // 删除
        delete_session(&pool, &session.id).await.unwrap();
        let sessions = list_sessions(&pool, "agent1").await.unwrap();
        assert_eq!(sessions.len(), 0);
    }

    #[tokio::test]
    async fn test_conversation_with_session() {
        let pool = setup_pool().await;

        let session = create_session(&pool, "agent1", "Session 1").await.unwrap();

        // 保存对话
        save_conversation(&pool, "agent1", &session.id, "Hello", "Hi there!").await.unwrap();

        // 获取历史
        let history = get_history(&pool, "agent1", &session.id, 10).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].0, "Hello");

        // 消息数
        let count = get_session_message_count(&pool, &session.id).await.unwrap();
        assert_eq!(count, 1);

        // 清除
        clear_session_history(&pool, &session.id).await.unwrap();
        let history = get_history(&pool, "agent1", &session.id, 10).await.unwrap();
        assert_eq!(history.len(), 0);
    }

    #[tokio::test]
    async fn test_session_summary() {
        let pool = setup_pool().await;

        let session = create_session(&pool, "agent1", "Session").await.unwrap();
        update_session_summary(&pool, &session.id, "这是摘要").await.unwrap();

        let s = get_session(&pool, &session.id).await.unwrap().unwrap();
        assert_eq!(s.summary.as_deref(), Some("这是摘要"));
    }

    #[tokio::test]
    async fn test_save_user_message_and_update_response() {
        let pool = setup_pool().await;
        let session = create_session(&pool, "agent1", "Session").await.unwrap();

        // 先保存用户消息（agent_response 为空）
        let conv_id = save_user_message(&pool, "agent1", &session.id, "你好").await.unwrap();
        assert!(!conv_id.is_empty());

        // 验证历史中 agent_response 为空
        let history = get_history(&pool, "agent1", &session.id, 10).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].0, "你好");
        assert_eq!(history[0].1, "");

        // 更新 AI 回复
        update_agent_response(&pool, &conv_id, "你好！有什么可以帮你的？").await.unwrap();

        // 验证回复已更新
        let history = get_history(&pool, "agent1", &session.id, 10).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].1, "你好！有什么可以帮你的？");
    }

    #[tokio::test]
    async fn test_save_user_message_error_recovery() {
        let pool = setup_pool().await;
        let session = create_session(&pool, "agent1", "Session").await.unwrap();

        // 保存用户消息
        let conv_id = save_user_message(&pool, "agent1", &session.id, "测试问题").await.unwrap();

        // 模拟失败：保存错误信息
        let error_msg = "⚠️ 回复生成失败: 网络超时";
        update_agent_response(&pool, &conv_id, error_msg).await.unwrap();

        // 验证错误信息被保存
        let history = get_history(&pool, "agent1", &session.id, 10).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].0, "测试问题");
        assert!(history[0].1.contains("回复生成失败"));
    }
}
