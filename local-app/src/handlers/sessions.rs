//! 会话/消息相关命令

use std::sync::Arc;
use tauri::Manager;
use tauri::State;

use crate::agent;
use crate::memory;
use crate::AppState;
use super::helpers::{load_providers, find_provider_for_model, resolve_model_context_window, rotate_api_key};

/// 发送消息并通过事件流推送 token（支持 Failover）
#[tauri::command]
pub async fn send_message(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    session_id: String,
    message: String,
    _attachments: Option<Vec<serde_json::Value>>,
) -> Result<String, String> {
    // 读取 Agent 信息（含 config 用于 failover）
    let agent = {
        let agents = state.orchestrator.list_agents().await?;
        agents
            .into_iter()
            .find(|a| a.id == agent_id)
            .ok_or("Agent 不存在")?
    };

    // 构建 Failover 执行器
    let failover = agent::FailoverExecutor::from_agent_config(
        &agent.model,
        agent.config.as_deref(),
    );

    // 从 providers 配置中查找可用的模型
    let providers = load_providers(&state.db).await?;

    // 找到第一个有可用 provider 的模型（支持 provider_id/model 限定格式）
    let mut selected_model = None;
    for model in failover.all_models() {
        if let Some(provider_info) = find_provider_for_model(&providers, model) {
            if !provider_info.1.is_empty() {
                selected_model = Some((model.to_string(), provider_info));
                break;
            }
        }
    }

    let (model_used, (api_type, api_key, base_url)) = selected_model
        .ok_or_else(|| format!(
            "未找到可用的模型供应商配置（尝试了: {}），请在设置中添加",
            failover.all_models().join(", ")
        ))?;

    if model_used != agent.model {
        log::info!("Failover: 主模型 {} 不可用，使用备用模型 {}", agent.model, model_used);
    }

    // 创建 token 推送通道
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // 后台任务：将流式 token 推送到前端
    let app_clone = app.clone();
    tokio::spawn(async move {
        while let Some(token) = rx.recv().await {
            let _ = app_clone.emit_all("llm-token", &token);
        }
        let _ = app_clone.emit_all("llm-done", "");
    });

    // 调用编排器执行流式对话
    let base_url_opt = if base_url.is_empty() {
        None
    } else {
        Some(base_url.as_str())
    };
    let result = state
        .orchestrator
        .send_message_stream(
            &agent_id,
            &session_id,
            &message,
            &api_key,
            &api_type,
            base_url_opt,
            tx,
            None, // cancel_token（未来可从前端传入）
        )
        .await;

    // 对话后自动处理（后台异步，不阻塞返回）
    if result.is_ok() {
        let pool = state.orchestrator.pool().clone();
        let sid = session_id.clone();
        let aid = agent_id.clone();
        let msg = message.clone();
        let _db_ref = &state.db; // 不需要 clone，pool 已有
        let api_key_clone = api_key.clone();
        let api_type_clone = api_type.clone();
        let base_url_clone = base_url.clone();
        tokio::spawn(async move {
            // 1. LLM 自动生成会话标题（第一条消息时，fire-and-forget）
            let bu = if base_url_clone.is_empty() { None } else { Some(base_url_clone.as_str()) };
            crate::memory::conversation::auto_name_session(
                &pool, &sid, &msg, &api_key_clone, &api_type_clone, bu,
            ).await;

            // 2. 自动更新会话摘要（每 5 轮更新一次）
            let msg_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM chat_messages WHERE session_id = ?"
            ).bind(&sid).fetch_one(&pool).await.unwrap_or(0);

            if msg_count > 0 && msg_count % 10 == 0 {
                // 取最近 10 条消息生成摘要
                let recent: Vec<(String, String)> = sqlx::query_as(
                    "SELECT role, COALESCE(content, '') FROM chat_messages WHERE session_id = ? ORDER BY seq DESC LIMIT 10"
                ).bind(&sid).fetch_all(&pool).await.unwrap_or_default();

                if !recent.is_empty() {
                    let summary_text: String = recent.iter().rev()
                        .map(|(role, content)| {
                            let preview: String = content.chars().take(100).collect();
                            format!("{}: {}", role, preview)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let _ = sqlx::query("UPDATE chat_sessions SET summary = ? WHERE id = ?")
                        .bind(&summary_text).bind(&sid).execute(&pool).await;
                    log::info!("自动更新会话摘要: session={}, messages={}", &sid[..8], msg_count);
                }
            }

            // 3. 自动学习用户偏好 → 更新 USER.md（每 20 轮检查一次）
            if msg_count > 0 && msg_count % 20 == 0 {
                if let Ok(Some(wp)) = sqlx::query_scalar::<_, String>(
                    "SELECT workspace_path FROM agents WHERE id = ?"
                ).bind(&aid).fetch_optional(&pool).await {
                    let user_file = std::path::PathBuf::from(&wp).join("USER.md");
                    let existing = std::fs::read_to_string(&user_file).unwrap_or_default();

                    // 用简单规则提取用户偏好（不调 LLM，保持轻量）
                    let mut new_facts = Vec::new();
                    // 从最近消息中提取偏好关键词
                    let recent_user: Vec<String> = sqlx::query_scalar(
                        "SELECT COALESCE(content, '') FROM chat_messages WHERE session_id = ? AND role = 'user' ORDER BY seq DESC LIMIT 10"
                    ).bind(&sid).fetch_all(&pool).await.unwrap_or_default();

                    for content in &recent_user {
                        // 提取"我喜欢/我习惯/我是/我在"等自我描述
                        for pattern in &["我喜欢", "我习惯", "我是", "我在", "我的", "我常用", "我偏好"] {
                            if let Some(pos) = content.find(pattern) {
                                let fact: String = content[pos..].chars().take(50).collect();
                                let fact = fact.split(&['。', '，', '！', '？', '\n'][..]).next().unwrap_or(&fact);
                                if !fact.is_empty() && !existing.contains(fact) {
                                    new_facts.push(fact.to_string());
                                }
                            }
                        }
                    }

                    if !new_facts.is_empty() {
                        let append = format!(
                            "\n\n## 自动学习 ({})\n{}\n",
                            chrono::Local::now().format("%Y-%m-%d"),
                            new_facts.iter().map(|f| format!("- {}", f)).collect::<Vec<_>>().join("\n"),
                        );
                        let updated = format!("{}{}", existing, append);
                        let _ = std::fs::write(&user_file, &updated);
                        log::info!("USER.md 自动学习: 新增 {} 条偏好", new_facts.len());
                    }
                }
            }
        });
    }

    result
}

/// 群聊轻量对话 — 不带 tools/skills/memory，纯 LLM 文本对话，速度快
#[tauri::command]
pub async fn send_chat_only(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    message: String,
) -> Result<String, String> {
    // 读取 Agent 信息
    let agent = {
        let agents = state.orchestrator.list_agents().await?;
        agents.into_iter().find(|a| a.id == agent_id).ok_or("Agent 不存在")?
    };

    let failover = agent::FailoverExecutor::from_agent_config(&agent.model, agent.config.as_deref());
    let providers = load_providers(&state.db).await?;
    let mut selected_model = None;
    for model in failover.all_models() {
        if let Some(provider_info) = find_provider_for_model(&providers, model) {
            if !provider_info.1.is_empty() {
                selected_model = Some((model.to_string(), provider_info));
                break;
            }
        }
    }
    let (model_used, (api_type, api_key, base_url)) = selected_model
        .ok_or("未找到可用的模型供应商配置")?;

    log::info!("send_chat_only: agent={}, model={}, provider={}", agent.name, model_used, api_type);

    let system_prompt = agent.system_prompt.clone();

    // 流式 token 推送
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let app_clone = app.clone();
    tokio::spawn(async move {
        while let Some(token) = rx.recv().await {
            let _ = app_clone.emit_all("llm-token", &token);
        }
        let _ = app_clone.emit_all("llm-done", "");
    });

    let messages = vec![serde_json::json!({ "role": "user", "content": message })];

    let llm_config = agent::llm::LlmConfig {
        provider: api_type.clone(),
        model: model_used.clone(),
        api_key: api_key.clone(),
        base_url: if base_url.is_empty() { None } else { Some(base_url.clone()) },
        temperature: Some(agent.temperature),
        max_tokens: Some(agent.max_tokens),
        thinking_level: None,
    };

    let client = agent::llm::LlmClient::new(llm_config);
    let sp = if system_prompt.is_empty() { None } else { Some(system_prompt.as_str()) };

    // 流式对话，60 秒超时
    match tokio::time::timeout(
        std::time::Duration::from_secs(60),
        client.call_stream(&messages, sp, None, tx),
    ).await {
        Ok(Ok(resp)) => Ok(resp.content),
        Ok(Err(e)) => Err(format!("LLM 调用失败: {}", e)),
        Err(_) => Err("LLM 响应超时 (60s)".to_string()),
    }
}

/// 获取对话历史（按 session）
#[tauri::command]
pub async fn get_conversations(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    session_id: String,
    limit: i64,
) -> Result<Vec<serde_json::Value>, String> {
    let conversations = state
        .orchestrator
        .get_conversations(&agent_id, &session_id, limit)
        .await?;

    Ok(conversations
        .into_iter()
        .map(|(user_msg, agent_resp)| {
            serde_json::json!({
                "userMessage": user_msg,
                "agentResponse": agent_resp,
            })
        })
        .collect())
}

/// 获取会话消息（返回 Message[] 格式，供 AgentDetailPage 使用）
#[tauri::command]
pub async fn get_session_messages(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    session_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let conversations = state
        .orchestrator
        .get_conversations(&agent_id, &session_id, 100)
        .await?;

    let mut msgs = Vec::new();
    // get_conversations 返回 DESC 顺序，需要反转为时间正序
    for (user_msg, agent_resp) in conversations.into_iter().rev() {
        msgs.push(serde_json::json!({ "role": "user", "content": user_msg }));
        if !agent_resp.is_empty() {
            msgs.push(serde_json::json!({ "role": "assistant", "content": agent_resp }));
        }
    }
    Ok(msgs)
}

/// 加载结构化消息历史（含完整的 tool_calls、tool_result）
#[tauri::command]
pub async fn load_structured_messages(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    limit: Option<i64>,
) -> Result<Vec<serde_json::Value>, String> {
    crate::memory::conversation::load_chat_messages(
        state.orchestrator.pool(), &session_id, limit.unwrap_or(30),
    ).await.map_err(|e| format!("加载消息失败: {}", e))
}

/// 清除会话的对话历史
#[tauri::command]
pub async fn clear_history(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    state.orchestrator.clear_history(&session_id).await
}

/// 创建会话
#[tauri::command]
pub async fn create_session(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    title: Option<String>,
) -> Result<serde_json::Value, String> {
    let title = title.unwrap_or_else(|| "New Session".to_string());
    let session = memory::conversation::create_session(state.orchestrator.pool(), &agent_id, &title)
        .await
        .map_err(|e| format!("创建会话失败: {}", e))?;
    Ok(serde_json::json!({
        "id": session.id,
        "agentId": session.agent_id,
        "title": session.title,
        "createdAt": session.created_at,
        "lastMessageAt": session.last_message_at,
        "summary": session.summary,
    }))
}

/// 清理旧的 cron/heartbeat 会话及其消息
#[tauri::command]
pub async fn cleanup_system_sessions(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    keep_days: Option<i64>,
) -> Result<serde_json::Value, String> {
    let pool = state.orchestrator.pool();
    let days = keep_days.unwrap_or(7);
    let cutoff = chrono::Utc::now().timestamp_millis() - (days * 86_400_000);

    // 查找旧的系统会话（cron-/heartbeat-/[cron]/[heartbeat] 开头）
    let old_sessions: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM chat_sessions WHERE agent_id = ? AND (title LIKE 'cron-%' OR title LIKE '[cron]%' OR title LIKE 'heartbeat-%' OR title LIKE '[heartbeat]%') AND created_at < ?"
    )
    .bind(&agent_id).bind(cutoff)
    .fetch_all(pool).await
    .map_err(|e| format!("查询失败: {}", e))?;

    let mut deleted_sessions = 0;
    let mut deleted_messages = 0;

    for (sid,) in &old_sessions {
        // 删除消息
        let msg_result = sqlx::query("DELETE FROM chat_messages WHERE session_id = ?")
            .bind(sid).execute(pool).await;
        if let Ok(r) = msg_result { deleted_messages += r.rows_affected(); }

        let conv_result = sqlx::query("DELETE FROM conversations WHERE session_id = ?")
            .bind(sid).execute(pool).await;
        if let Ok(r) = conv_result { deleted_messages += r.rows_affected(); }

        // 删除会话
        let _ = sqlx::query("DELETE FROM chat_sessions WHERE id = ?")
            .bind(sid).execute(pool).await;
        deleted_sessions += 1;
    }

    log::info!("清理系统会话: 删除 {} 个会话, {} 条消息 (保留 {} 天内)", deleted_sessions, deleted_messages, days);

    Ok(serde_json::json!({
        "deletedSessions": deleted_sessions,
        "deletedMessages": deleted_messages,
        "keepDays": days,
    }))
}

/// 列出 Agent 的所有会话
#[tauri::command]
pub async fn list_sessions(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let sessions = memory::conversation::list_sessions(state.orchestrator.pool(), &agent_id)
        .await
        .map_err(|e| format!("获取会话列表失败: {}", e))?;
    Ok(sessions
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "agentId": s.agent_id,
                "title": s.title,
                "createdAt": s.created_at,
                "lastMessageAt": s.last_message_at,
                "summary": s.summary,
            })
        })
        .collect())
}

/// 重命名会话
#[tauri::command]
pub async fn rename_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    title: String,
) -> Result<(), String> {
    memory::conversation::rename_session(state.orchestrator.pool(), &session_id, &title)
        .await
        .map_err(|e| format!("重命名会话失败: {}", e))
}

/// 删除会话
#[tauri::command]
pub async fn delete_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    memory::conversation::delete_session(state.orchestrator.pool(), &session_id)
        .await
        .map_err(|e| format!("删除会话失败: {}", e))
}

/// 压缩会话上下文
#[tauri::command]
pub async fn compact_session(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    session_id: String,
) -> Result<String, String> {
    log::info!("compact_session: agent_id={}, session_id={}", agent_id, session_id);
    // 查找 agent 模型对应的 provider
    let agent_model = {
        let agents = state.orchestrator.list_agents().await?;
        agents
            .into_iter()
            .find(|a| a.id == agent_id)
            .map(|a| a.model)
            .ok_or("Agent 不存在")?
    };
    let providers = load_providers(&state.db).await?;
    log::info!("compact_session: agent_model={}, providers_count={}", agent_model, providers.len());
    let (api_type, api_key, base_url) =
        find_provider_for_model(&providers, &agent_model)
        .ok_or_else(|| {
            log::error!("compact_session: 未找到模型 {} 的供应商配置！", agent_model);
            format!("未找到模型 {} 的供应商配置", agent_model)
        })?;
    log::info!("compact_session: 使用 provider={} base_url={}", api_type, if base_url.is_empty() { "default" } else { &base_url });
    let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };

    state
        .orchestrator
        .compact_session(&agent_id, &session_id, &api_key, &api_type, base_url_opt)
        .await
}

/// 全文搜索消息
#[tauri::command]
pub async fn search_messages(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<serde_json::Value>, String> {
    let limit = limit.unwrap_or(20);
    let like_query = format!("%{}%", query);

    let rows: Vec<(String, String, String, String, i64)> = sqlx::query_as(
        "SELECT m.session_id, s.title, m.role, m.content, m.seq \
         FROM chat_messages m JOIN chat_sessions s ON m.session_id = s.id \
         WHERE s.agent_id = ? AND m.content LIKE ? \
         ORDER BY m.seq DESC LIMIT ?"
    )
    .bind(&agent_id).bind(&like_query).bind(limit)
    .fetch_all(state.db.pool()).await.map_err(|e| e.to_string())?;

    Ok(rows.iter().map(|(sid, title, role, content, seq)| {
        // 高亮匹配片段（取匹配位置前后各 80 字符）
        let lower_content = content.to_lowercase();
        let lower_query = query.to_lowercase();
        let snippet = if let Some(pos) = lower_content.find(&lower_query) {
            let start = pos.saturating_sub(80);
            let end = (pos + query.len() + 80).min(content.len());
            let mut end_safe = end;
            while end_safe > 0 && !content.is_char_boundary(end_safe) { end_safe -= 1; }
            let mut start_safe = start;
            while start_safe < content.len() && !content.is_char_boundary(start_safe) { start_safe += 1; }
            format!("{}...{}...", if start > 0 { "..." } else { "" }, &content[start_safe..end_safe])
        } else {
            content.chars().take(160).collect::<String>()
        };

        serde_json::json!({
            "sessionId": sid, "sessionTitle": title,
            "role": role, "snippet": snippet, "seq": seq,
        })
    }).collect())
}

/// 导出 Session 对话历史为 JSON
#[tauri::command]
pub async fn export_session_history(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    format: Option<String>,
) -> Result<String, String> {
    let messages: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT role, COALESCE(content, ''), seq FROM chat_messages WHERE session_id = ? ORDER BY seq ASC"
    ).bind(&session_id).fetch_all(state.db.pool()).await.map_err(|e| e.to_string())?;

    let session_title: Option<String> = sqlx::query_scalar(
        "SELECT title FROM chat_sessions WHERE id = ?"
    ).bind(&session_id).fetch_optional(state.db.pool()).await.ok().flatten();

    let fmt = format.as_deref().unwrap_or("markdown");

    match fmt {
        "json" => {
            let export = serde_json::json!({
                "session_id": session_id,
                "title": session_title.unwrap_or_default(),
                "exported_at": chrono::Utc::now().to_rfc3339(),
                "messages": messages.iter().map(|(role, content, seq)| {
                    serde_json::json!({"role": role, "content": content, "seq": seq})
                }).collect::<Vec<_>>()
            });
            serde_json::to_string_pretty(&export).map_err(|e| e.to_string())
        }
        _ => {
            // Markdown 格式
            let mut md = format!("# {}\n\n", session_title.unwrap_or("对话记录".into()));
            md.push_str(&format!("导出时间: {}\n\n---\n\n", chrono::Utc::now().format("%Y-%m-%d %H:%M")));
            for (role, content, _) in &messages {
                let label = match role.as_str() {
                    "user" => "**用户**",
                    "assistant" => "**助手**",
                    "tool" => "**工具**",
                    _ => role.as_str(),
                };
                md.push_str(&format!("{}: {}\n\n", label, content));
            }
            Ok(md)
        }
    }
}

/// 获取当前会话的上下文 Token 使用情况
#[tauri::command]
pub async fn get_context_usage(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    session_id: String,
) -> Result<serde_json::Value, String> {
    // 获取 Agent 信息
    let agent = state.orchestrator.list_agents().await?
        .into_iter().find(|a| a.id == agent_id)
        .ok_or("Agent 不存在")?;

    // 估算各部分 Token（使用 orchestrator 的 estimate_tokens）
    let system_prompt_tokens = agent::orchestrator::estimate_tokens_pub(&agent.system_prompt);

    // 读取 Soul 文件
    let mut soul_tokens = 0usize;
    let ctx_workspace: Option<String> = sqlx::query_scalar(
        "SELECT workspace_path FROM agents WHERE id = ?"
    ).bind(&agent_id).fetch_optional(state.db.pool()).await.ok().flatten();
    if let Some(ref wp) = ctx_workspace {
        for name in &["SOUL.md", "PERSONA.md", "TOOLS.md", "FOCUS.md"] {
            let path = std::path::Path::new(wp.as_str()).join(name);
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    soul_tokens += agent::orchestrator::estimate_tokens_pub(&content);
                }
            }
        }
    }

    // 消息 Token — 只算 LLM 实际会看到的消息（compact boundary 之后的）
    let compact_boundary: i64 = {
        let key = format!("compact_boundary_{}", session_id);
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(&key)
            .fetch_optional(state.db.pool()).await.ok().flatten()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0)
    };

    let msg_contents: Vec<(String,)> = if compact_boundary > 0 {
        sqlx::query_as(
            "SELECT COALESCE(content, '') FROM chat_messages WHERE session_id = ? AND seq > ?"
        ).bind(&session_id).bind(compact_boundary).fetch_all(state.db.pool()).await.unwrap_or_default()
    } else {
        sqlx::query_as(
            "SELECT COALESCE(content, '') FROM chat_messages WHERE session_id = ?"
        ).bind(&session_id).fetch_all(state.db.pool()).await.unwrap_or_default()
    };
    let message_tokens: usize = msg_contents.iter()
        .map(|(c,)| agent::orchestrator::estimate_tokens_pub(c))
        .sum();
    let message_count = msg_contents.len();

    // 工具定义 Token
    let tool_defs = state.orchestrator.tool_manager().get_tool_definitions();
    let tool_tokens: usize = tool_defs.iter()
        .map(|t| agent::orchestrator::estimate_tokens_pub(&t.name)
            + agent::orchestrator::estimate_tokens_pub(&t.description) + 30)
        .sum();

    // 摘要 Token
    let summary: Option<String> = sqlx::query_scalar(
        "SELECT summary FROM chat_sessions WHERE id = ?"
    ).bind(&session_id).fetch_optional(state.db.pool()).await.ok().flatten();
    let summary_tokens = summary.as_ref()
        .map(|s| agent::orchestrator::estimate_tokens_pub(s))
        .unwrap_or(0);

    let total = system_prompt_tokens + soul_tokens + message_tokens + tool_tokens + summary_tokens;

    // 模型最大 Token 窗口（精确映射）
    let max_context = resolve_model_context_window(&agent.model);

    log::info!(
        "get_context_usage: session={} model={} msg_count={} sys={} soul={} msgs={} tools={} sum={} total={} max={} pct={:.1}%",
        &session_id[..session_id.len().min(8)], agent.model, message_count,
        system_prompt_tokens, soul_tokens, message_tokens, tool_tokens, summary_tokens,
        total, max_context, total as f64 / max_context as f64 * 100.0
    );

    Ok(serde_json::json!({
        "system_prompt": system_prompt_tokens,
        "soul_files": soul_tokens,
        "messages": message_tokens,
        "tools": tool_tokens,
        "summary": summary_tokens,
        "total": total,
        "message_count": message_count,
        "max_context": max_context,
        "usage_percent": format!("{:.1}", total as f64 / max_context as f64 * 100.0),
    }))
}

/// 编辑用户消息（更新内容并删除该消息之后的所有消息）
#[tauri::command]
pub async fn edit_message(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    message_seq: i64,
    new_content: String,
) -> Result<(), String> {
    let pool = state.orchestrator.pool();

    // 1. 更新该消息的 content
    let result = sqlx::query("UPDATE chat_messages SET content = ? WHERE session_id = ? AND seq = ?")
        .bind(&new_content)
        .bind(&session_id)
        .bind(message_seq)
        .execute(pool)
        .await
        .map_err(|e| format!("更新消息失败: {}", e))?;

    if result.rows_affected() == 0 {
        return Err("消息不存在".to_string());
    }

    // 2. 删除该消息之后的所有消息（chat_messages 表）
    sqlx::query("DELETE FROM chat_messages WHERE session_id = ? AND seq > ?")
        .bind(&session_id)
        .bind(message_seq)
        .execute(pool)
        .await
        .map_err(|e| format!("删除后续消息失败: {}", e))?;

    // 3. 同步清理 conversations 表中该 session 的旧记录
    //    conversations 表没有 seq 字段，简单地删除所有记录后重建不太合适，
    //    所以只做 chat_messages 的操作即可（conversations 表是旧格式兼容）

    log::info!("编辑消息: session={} seq={} new_content_len={}", session_id, message_seq, new_content.len());
    Ok(())
}

/// 重新生成 AI 回复（删除指定 seq 之后的所有消息）
#[tauri::command]
pub async fn regenerate_response(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    after_seq: i64,
) -> Result<(), String> {
    let pool = state.orchestrator.pool();

    // 删除 after_seq 及之后的所有 assistant/tool 消息
    // 实际上删除 after_seq（含）之后的所有消息，因为重新生成需要从用户消息重新开始
    sqlx::query("DELETE FROM chat_messages WHERE session_id = ? AND seq >= ?")
        .bind(&session_id)
        .bind(after_seq)
        .execute(pool)
        .await
        .map_err(|e| format!("删除消息失败: {}", e))?;

    log::info!("重新生成回复: session={} 删除 seq >= {}", session_id, after_seq);
    Ok(())
}

/// 提交消息反馈（thumbs up/down）
#[tauri::command]
pub async fn submit_message_feedback(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    message_seq: i64,
    feedback: String,  // "up" | "down"
) -> Result<(), String> {
    let now = chrono::Utc::now().timestamp_millis();
    // 存入 settings 表（简单方案，无需新建表）
    let key = format!("feedback_{}_{}", session_id, message_seq);
    sqlx::query("INSERT OR REPLACE INTO settings (key, value, updated_at) VALUES (?, ?, ?)")
        .bind(&key).bind(&feedback).bind(now)
        .execute(state.db.pool()).await.map_err(|e| e.to_string())?;

    log::info!("消息反馈: session={} seq={} feedback={}", session_id, message_seq, feedback);
    Ok(())
}

/// 语音转文字
///
/// 接收前端录音的二进制数据，调用 Whisper API 或本地 STT 转为文字。
/// 优先使用用户配置的 OpenAI 兼容 Provider（支持自定义 base_url），
/// 无可用 API key 时回退到 macOS 本地 SFSpeechRecognizer。
#[tauri::command]
pub async fn transcribe_audio(
    state: State<'_, Arc<AppState>>,
    audio_data: Vec<u8>,
    format: String,
    language: Option<String>,
) -> Result<String, String> {
    // 1. 写临时文件
    let tmp_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".xianzhu/tts");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let ext = match format.as_str() {
        "webm" => "webm",
        "mp4" => "m4a",
        "wav" => "wav",
        "ogg" => "ogg",
        _ => "webm",
    };
    let tmp_file = tmp_dir.join(format!(
        "stt_input_{}.{}",
        chrono::Utc::now().timestamp_millis(),
        ext
    ));
    std::fs::write(&tmp_file, &audio_data)
        .map_err(|e| format!("写入音频失败: {}", e))?;

    // 2. 从 providers 中查找 OpenAI 兼容的 API key 和 base_url
    let (api_key, base_url) = find_whisper_provider(&state.db).await;

    // 3. 调用 Whisper API 或本地 STT
    let result = if let Some(key) = api_key {
        whisper_transcribe(
            &tmp_file,
            &key,
            base_url.as_deref(),
            language.as_deref(),
        )
        .await
    } else {
        local_stt_fallback(&tmp_file, language.as_deref()).await
    };

    // 4. 清理临时文件
    let _ = std::fs::remove_file(&tmp_file);

    result
}

/// 语音转文字（接收文件路径，录音后直接调用）
#[tauri::command]
pub async fn transcribe_audio_file(
    state: State<'_, Arc<AppState>>,
    file_path: String,
    language: Option<String>,
) -> Result<String, String> {
    let path = std::path::PathBuf::from(&file_path);
    if !path.exists() {
        return Err("录音文件不存在".to_string());
    }

    let (api_key, base_url) = find_whisper_provider(&state.db).await;

    let result = if let Some(key) = api_key {
        whisper_transcribe(&path, &key, base_url.as_deref(), language.as_deref()).await
    } else {
        local_stt_fallback(&path, language.as_deref()).await
    };

    // 清理录音文件
    let _ = std::fs::remove_file(&path);

    result
}

/// 从 providers 配置中查找可用于 Whisper API 的 key 和 base_url
async fn find_whisper_provider(db: &crate::db::Database) -> (Option<String>, Option<String>) {
    let providers = match load_providers(db).await {
        Ok(p) => p,
        Err(_) => return (None, None),
    };

    // 优先查找 apiType 为 "openai" 且 enabled 的 provider
    for p in &providers {
        if p["enabled"].as_bool() != Some(true) {
            continue;
        }
        let api_type = p["apiType"].as_str().unwrap_or("openai");
        if api_type == "openai" {
            if let Some(raw_key) = p["apiKey"].as_str() {
                if !raw_key.is_empty() {
                    let provider_id = p["id"].as_str().unwrap_or("whisper");
                    let key = rotate_api_key(provider_id, raw_key);
                    let base_url = p["baseUrl"]
                        .as_str()
                        .map(|s| s.to_string())
                        .filter(|s| !s.is_empty());
                    return (Some(key), base_url);
                }
            }
        }
    }

    (None, None)
}

/// 调用 Whisper API 进行语音转文字（支持自定义 base_url）
async fn whisper_transcribe(
    file_path: &std::path::Path,
    api_key: &str,
    base_url: Option<&str>,
    language: Option<&str>,
) -> Result<String, String> {
    let file_bytes = tokio::fs::read(file_path)
        .await
        .map_err(|e| format!("读取音频文件失败: {}", e))?;

    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.webm")
        .to_string();

    // 根据扩展名设置 MIME type
    let mime = match file_path.extension().and_then(|e| e.to_str()) {
        Some("webm") => "audio/webm",
        Some("m4a") | Some("mp4") => "audio/mp4",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        _ => "audio/webm",
    };

    let file_part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(file_name)
        .mime_str(mime)
        .unwrap_or_else(|_| reqwest::multipart::Part::bytes(vec![]));

    let mut form = reqwest::multipart::Form::new()
        .text("model", "whisper-1")
        .part("file", file_part);

    if let Some(lang) = language {
        form = form.text("language", lang.to_string());
    }

    // 构建 API URL：支持自定义 base_url
    let url = if let Some(base) = base_url {
        let base = base.trim_end_matches('/');
        // 如果 base_url 已包含 /v1，直接追加路径
        if base.ends_with("/v1") {
            format!("{}/audio/transcriptions", base)
        } else {
            format!("{}/v1/audio/transcriptions", base)
        }
    } else {
        "https://api.openai.com/v1/audio/transcriptions".to_string()
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Whisper API 请求失败: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Whisper API 错误 {}: {}",
            status,
            &body[..body.len().min(200)]
        ));
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 Whisper 响应失败: {}", e))?;

    data["text"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Whisper 返回空结果".to_string())
}

/// macOS 本地 STT 回退（使用 SFSpeechRecognizer）
async fn local_stt_fallback(
    file_path: &std::path::Path,
    _language: Option<&str>,
) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let path_str = file_path.to_string_lossy().to_string();
        let output = tokio::process::Command::new("swift")
            .arg("-e")
            .arg(format!(
                r#"
import Speech
import Foundation
let sem = DispatchSemaphore(value: 0)
let recognizer = SFSpeechRecognizer(locale: Locale(identifier: "zh-Hans"))!
let request = SFSpeechURLRecognitionRequest(url: URL(fileURLWithPath: "{}"))
recognizer.recognitionTask(with: request) {{ result, error in
    if let r = result, r.isFinal {{ print(r.bestTranscription.formattedString); sem.signal() }}
    else if error != nil {{ print("ERROR: \(error!.localizedDescription)"); sem.signal() }}
}}
sem.wait()
"#,
                path_str
            ))
            .output()
            .await;

        if let Ok(out) = output {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !text.is_empty() && !text.starts_with("ERROR:") {
                    return Ok(text);
                }
            }
        }
    }

    Err("语音转文字不可用。请配置 OpenAI 兼容的 Provider（需要 Whisper API 支持），或在 macOS 上确保语音识别权限已授予。".to_string())
}

// ─── 录音控制 ──────────────────────────────────────────

#[cfg(target_os = "macos")]
mod voice_recording {
    use std::sync::Mutex as StdMutex;
    use once_cell::sync::Lazy;

    static RECORDING_PROCESS: Lazy<StdMutex<Option<std::process::Child>>> = Lazy::new(|| StdMutex::new(None));
    static RECORDING_PATH: Lazy<StdMutex<Option<String>>> = Lazy::new(|| StdMutex::new(None));

    pub async fn start() -> Result<String, String> {
        let audio_dir = dirs::home_dir()
            .ok_or("无法获取 home 目录")?
            .join(".xianzhu/tts");
        let _ = std::fs::create_dir_all(&audio_dir);
        let path = audio_dir.join(format!("recording_{}.wav", chrono::Utc::now().timestamp_millis()));
        let path_str = path.to_string_lossy().to_string();

        let child = std::process::Command::new("swift")
            .arg("-e")
            .arg(format!(
                r#"
import AVFoundation
import Foundation
let url = URL(fileURLWithPath: "{}")
let settings: [String: Any] = [
    AVFormatIDKey: Int(kAudioFormatLinearPCM),
    AVSampleRateKey: 16000,
    AVNumberOfChannelsKey: 1,
    AVLinearPCMBitDepthKey: 16,
    AVLinearPCMIsFloatKey: false
]
let recorder = try AVAudioRecorder(url: url, settings: settings)
recorder.record()
signal(SIGTERM, {{ _ in exit(0) }})
signal(SIGINT, {{ _ in exit(0) }})
RunLoop.current.run()
"#,
                path_str
            ))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("启动录音失败: {}", e))?;

        *RECORDING_PROCESS.lock().unwrap() = Some(child);
        *RECORDING_PATH.lock().unwrap() = Some(path_str.clone());
        log::info!("录音开始: {}", path_str);
        Ok(path_str)
    }

    pub async fn stop() -> Result<String, String> {
        let mut guard = RECORDING_PROCESS.lock().unwrap();
        if let Some(ref mut child) = *guard {
            unsafe { libc::kill(child.id() as i32, libc::SIGTERM); }
            let _ = child.wait();
        }
        *guard = None;
        std::thread::sleep(std::time::Duration::from_millis(200));

        let path = RECORDING_PATH.lock().unwrap().take()
            .ok_or("没有进行中的录音")?;
        let meta = std::fs::metadata(&path).map_err(|_| "录音文件不存在")?;
        if meta.len() < 100 { return Err("录音时间太短".to_string()); }
        log::info!("录音停止: {} ({}KB)", path, meta.len() / 1024);
        Ok(path)
    }
}

#[tauri::command]
pub async fn start_voice_recording() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    { return voice_recording::start().await; }
    #[cfg(not(target_os = "macos"))]
    { Err("语音录制目前仅支持 macOS".to_string()) }
}

#[tauri::command]
pub async fn stop_voice_recording() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    { return voice_recording::stop().await; }
    #[cfg(not(target_os = "macos"))]
    { Err("语音录制目前仅支持 macOS".to_string()) }
}
