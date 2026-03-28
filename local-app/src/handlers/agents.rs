//! Agent 管理相关命令

use std::sync::Arc;
use tauri::State;

use crate::agent;
use crate::AppState;
use super::helpers::{load_providers, ensure_agent_workspace};

/// 创建新 Agent
#[tauri::command]
pub async fn create_agent(
    state: State<'_, Arc<AppState>>,
    name: String,
    system_prompt: String,
    model: String,
) -> Result<serde_json::Value, String> {
    let agent = state
        .orchestrator
        .register_agent(&name, &system_prompt, &model)
        .await?;

    Ok(serde_json::json!({
        "id": agent.id,
        "name": agent.name,
        "model": agent.model,
        "systemPrompt": agent.system_prompt,
        "createdAt": agent.created_at,
    }))
}

/// 列出所有 Agent
#[tauri::command]
pub async fn list_agents(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<serde_json::Value>, String> {
    let agents = state.orchestrator.list_agents().await?;

    Ok(agents
        .into_iter()
        .map(|a| {
            serde_json::json!({
                "id": a.id,
                "name": a.name,
                "model": a.model,
                "systemPrompt": a.system_prompt,
                "temperature": a.temperature,
                "maxTokens": a.max_tokens,
                "configVersion": a.config_version,
                "createdAt": a.created_at,
                "updatedAt": a.updated_at,
            })
        })
        .collect())
}

/// 删除 Agent
#[tauri::command]
pub async fn delete_agent(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<(), String> {
    state.orchestrator.delete_agent(&agent_id).await
}

/// 更新 Agent 配置（name / model / temperature / max_tokens）
///
/// 仅更新提供的字段，未提供的字段保持不变
#[tauri::command]
pub async fn update_agent(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    name: Option<String>,
    model: Option<String>,
    temperature: Option<f64>,
    max_tokens: Option<i32>,
) -> Result<(), String> {
    // 至少需要提供一个字段
    if name.is_none() && model.is_none() && temperature.is_none() && max_tokens.is_none() {
        return Err("至少需要提供一个要更新的字段".to_string());
    }

    // 动态构建 UPDATE SQL
    let mut set_clauses = Vec::new();
    if name.is_some() { set_clauses.push("name = ?"); }
    if model.is_some() { set_clauses.push("model = ?"); }
    if temperature.is_some() { set_clauses.push("temperature = ?"); }
    if max_tokens.is_some() { set_clauses.push("max_tokens = ?"); }

    let now = chrono::Utc::now().timestamp_millis();
    set_clauses.push("updated_at = ?");

    let sql = format!("UPDATE agents SET {} WHERE id = ?", set_clauses.join(", "));

    // 使用 sqlx::query 动态绑定参数
    let mut query = sqlx::query(&sql);
    if let Some(ref v) = name { query = query.bind(v); }
    if let Some(ref v) = model { query = query.bind(v); }
    if let Some(v) = temperature { query = query.bind(v); }
    if let Some(v) = max_tokens { query = query.bind(v); }
    query = query.bind(now);
    query = query.bind(&agent_id);

    let result = query
        .execute(state.orchestrator.pool())
        .await
        .map_err(|e| format!("更新 Agent 失败: {}", e))?;

    if result.rows_affected() == 0 {
        return Err("Agent 不存在".to_string());
    }

    log::info!("Agent 已更新: {}", agent_id);
    // 清除 agent 缓存
    state.orchestrator.invalidate_agent_cache(&agent_id);
    Ok(())
}

/// AI 生成 Agent 配置
///
/// 用户输入自然语言描述，调用 LLM 生成完整 Agent 配置 JSON
#[tauri::command]
pub async fn ai_generate_agent_config(
    state: State<'_, Arc<AppState>>,
    description: String,
) -> Result<serde_json::Value, String> {
    // 获取可用的 provider 和模型列表
    let providers = load_providers(&state.db).await?;
    let mut available_models = Vec::new();
    for p in &providers {
        if p["enabled"].as_bool() != Some(true) { continue; }
        let has_key = p["apiKey"].as_str().map_or(false, |k| !k.is_empty());
        if !has_key { continue; }
        if let Some(models) = p["models"].as_array() {
            for m in models {
                if let Some(id) = m["id"].as_str() {
                    available_models.push(id.to_string());
                }
            }
        }
    }

    if available_models.is_empty() {
        return Err("没有可用的模型，请先在设置中配置 API Key".to_string());
    }

    // 获取可用工具列表
    let tool_names: Vec<String> = state.orchestrator.tool_manager()
        .get_tool_definitions()
        .iter()
        .map(|t| t.name.clone())
        .collect();

    // 找一个可用的 provider 来调用 LLM
    let (api_type, api_key, base_url) = providers.iter()
        .filter(|p| p["enabled"].as_bool() == Some(true))
        .filter(|p| p["apiKey"].as_str().map_or(false, |k| !k.is_empty()))
        .filter_map(|p| {
            let api_type = p["apiType"].as_str().unwrap_or("openai").to_string();
            let api_key = p["apiKey"].as_str().unwrap_or("").to_string();
            let base_url = p["baseUrl"].as_str().unwrap_or("").to_string();
            if !api_key.is_empty() { Some((api_type, api_key, base_url)) } else { None }
        })
        .next()
        .ok_or("没有可用的 LLM 供应商")?;

    // 选择第一个可用模型
    let model = available_models.first().cloned().unwrap_or_default();

    let system_prompt = format!(
        r#"你是一个 Agent 配置生成器。根据用户的描述，生成一个完整的 Agent 配置。

可用模型列表: {}
可用工具列表: {}
工具 Profile 选项: basic（基础工具）, coding（编程工具）, full（全部工具）

请严格按以下 JSON 格式返回，不要包含任何其他文字：
{{
  "name": "Agent 名称（简短有意义）",
  "systemPrompt": "系统提示词（详细描述 Agent 的角色、能力和行为准则）",
  "model": "推荐的模型 ID（从可用模型中选择最合适的）",
  "temperature": 0.7,
  "maxTokens": 4096,
  "toolProfile": "推荐的工具 profile",
  "soulIdentity": "Agent 的身份描述（用于 IDENTITY.md）",
  "soulPersonality": "Agent 的性格特征（用于 SOUL.md）"
}}"#,
        available_models.join(", "),
        tool_names.join(", "),
    );

    let llm_config = agent::LlmConfig {
        provider: api_type,
        api_key,
        model,
        base_url: if base_url.is_empty() { None } else { Some(base_url) },
        temperature: Some(0.3),
        max_tokens: Some(2048),
        thinking_level: None,
    };

    let client = agent::LlmClient::new(llm_config);
    let messages = vec![("user".to_string(), description)];
    let response = client.call(messages, system_prompt, 0.3, 2048)
        .await
        .map_err(|e| format!("LLM 调用失败: {}", e))?;

    // 解析 JSON（容忍 markdown 代码块包裹）
    let json_str = response.trim();
    let json_str = json_str
        .strip_prefix("```json").or_else(|| json_str.strip_prefix("```"))
        .unwrap_or(json_str);
    let json_str = json_str.strip_suffix("```").unwrap_or(json_str).trim();

    let config: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("解析 AI 生成的配置失败: {}。原始响应: {}", e, response))?;

    Ok(config)
}

/// 获取 Agent 详细信息（聚合 Soul + 工具 + MCP + Skills）
#[tauri::command]
pub async fn get_agent_detail(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<serde_json::Value, String> {
    // 获取 Agent 基本信息
    let agents = state.orchestrator.list_agents().await?;
    let agent = agents
        .into_iter()
        .find(|a| a.id == agent_id)
        .ok_or("Agent 不存在")?;

    // 获取工作区
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;

    // 获取 Soul 文件列表
    let soul_files: Vec<serde_json::Value> = agent::workspace::SoulFile::all()
        .iter()
        .filter_map(|f| {
            let content = workspace.read_file(f);
            if content.is_some() {
                Some(serde_json::json!({
                    "name": f.filename(),
                    "size": content.map(|c| c.len()).unwrap_or(0),
                }))
            } else {
                None
            }
        })
        .collect();

    // 获取工具数量
    let tool_count = state.orchestrator.tool_manager().get_tool_definitions().len();

    // 获取 MCP 服务器列表
    let mcp_servers: Vec<serde_json::Value> = sqlx::query_as::<_, (String, String, String, i32)>(
        "SELECT id, name, transport, enabled FROM mcp_servers WHERE agent_id = ? ORDER BY created_at"
    )
    .bind(&agent_id)
    .fetch_all(state.orchestrator.pool())
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(id, name, transport, enabled)| {
        serde_json::json!({
            "id": id,
            "name": name,
            "transport": transport,
            "enabled": enabled != 0,
        })
    })
    .collect();

    // 获取 Skills 列表
    let skills = agent::SkillManager::list_installed(&agent_id, state.orchestrator.pool()).await
        .unwrap_or_default();

    // 获取会话数量
    let session_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_sessions WHERE agent_id = ?"
    )
    .bind(&agent_id)
    .fetch_one(state.orchestrator.pool())
    .await
    .unwrap_or(0);

    // 获取记忆体列表
    let memories: Vec<serde_json::Value> = sqlx::query_as::<_, (String, String, String, i64, i64, i64)>(
        "SELECT id, memory_type, content, priority, created_at, updated_at FROM memories WHERE agent_id = ? ORDER BY updated_at DESC"
    )
    .bind(&agent_id)
    .fetch_all(state.orchestrator.pool())
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(id, memory_type, content, priority, created_at, updated_at)| {
        serde_json::json!({
            "id": id,
            "memory_type": memory_type,
            "content": content,
            "priority": priority,
            "created_at": created_at,
            "updated_at": updated_at,
        })
    })
    .collect();

    // 获取对话统计
    let conversation_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM conversations WHERE agent_id = ?"
    )
    .bind(&agent_id)
    .fetch_one(state.orchestrator.pool())
    .await
    .unwrap_or(0);

    let message_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_messages WHERE agent_id = ?"
    )
    .bind(&agent_id)
    .fetch_one(state.orchestrator.pool())
    .await
    .unwrap_or(0);

    Ok(serde_json::json!({
        "id": agent.id,
        "name": agent.name,
        "model": agent.model,
        "systemPrompt": agent.system_prompt,
        "temperature": agent.temperature,
        "maxTokens": agent.max_tokens,
        "configVersion": agent.config_version,
        "createdAt": agent.created_at,
        "updatedAt": agent.updated_at,
        "soulFiles": soul_files,
        "toolCount": tool_count,
        "mcpServers": mcp_servers,
        "skillCount": skills.len(),
        "sessionCount": session_count,
        "memories": memories,
        "conversationCount": conversation_count,
        "messageCount": message_count,
        "vectorCount": sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM vectors WHERE agent_id = ?")
            .bind(&agent_id).fetch_one(state.orchestrator.pool()).await.unwrap_or(0),
        "embeddingCacheCount": sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM embedding_cache")
            .fetch_one(state.orchestrator.pool()).await.unwrap_or(0),
    }))
}

/// 获取 Agent 工具调用审计日志
#[tauri::command]
pub async fn get_audit_log(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<serde_json::Value>, String> {
    let entries = crate::db::audit::query_audit_log(
        state.orchestrator.pool(),
        &agent_id,
        limit.unwrap_or(50),
        offset.unwrap_or(0),
    ).await?;

    Ok(entries.iter().map(|e| {
        serde_json::json!({
            "id": e.id,
            "agentId": e.agent_id,
            "sessionId": e.session_id,
            "toolName": e.tool_name,
            "arguments": e.arguments,
            "result": e.result,
            "success": e.success,
            "policyDecision": e.policy_decision,
            "policySource": e.policy_source,
            "durationMs": e.duration_ms,
            "createdAt": e.created_at,
        })
    }).collect())
}

/// 获取 Agent 自治配置
#[tauri::command]
pub async fn get_autonomy_config(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<serde_json::Value, String> {
    let agents = state.orchestrator.list_agents().await?;
    let agent = agents.into_iter().find(|a| a.id == agent_id)
        .ok_or("Agent 不存在")?;
    let config = agent::autonomy::load_autonomy_config(agent.config.as_deref());
    serde_json::to_value(&config).map_err(|e| e.to_string())
}

/// 更新 Agent 自治配置
#[tauri::command]
pub async fn update_autonomy_config(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    autonomy_config: serde_json::Value,
) -> Result<(), String> {
    // 读取现有 config
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT config FROM agents WHERE id = ?"
    )
    .bind(&agent_id)
    .fetch_optional(state.orchestrator.pool())
    .await
    .map_err(|e| format!("查询失败: {}", e))?;

    let row = row.ok_or("Agent 不存在")?;

    let mut config: serde_json::Value = row.0
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));

    config["autonomy"] = autonomy_config;

    let config_str = serde_json::to_string(&config).map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp_millis();

    sqlx::query("UPDATE agents SET config = ?, updated_at = ? WHERE id = ?")
        .bind(&config_str)
        .bind(now)
        .bind(&agent_id)
        .execute(state.orchestrator.pool())
        .await
        .map_err(|e| format!("更新失败: {}", e))?;

    Ok(())
}

/// 获取 Agent 关系列表
#[tauri::command]
pub async fn get_agent_relations(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let relations = agent::relations::RelationManager::get_relations(
        state.orchestrator.pool(), &agent_id
    ).await?;

    Ok(relations.iter().map(|r| {
        serde_json::json!({
            "id": r.id,
            "fromId": r.from_id,
            "toId": r.to_id,
            "relationType": r.relation_type,
            "metadata": r.metadata,
            "createdAt": r.created_at,
        })
    }).collect())
}

/// 创建 Agent 关系
#[tauri::command]
pub async fn create_agent_relation(
    state: State<'_, Arc<AppState>>,
    from_id: String,
    to_id: String,
    relation_type: String,
) -> Result<String, String> {
    let rt = agent::RelationType::from_str(&relation_type)
        .ok_or(format!("无效的关系类型: {}", relation_type))?;
    agent::RelationManager::create(
        state.orchestrator.pool(), &from_id, &to_id, &rt, None
    ).await
}

/// 删除 Agent 关系
#[tauri::command]
pub async fn delete_agent_relation(
    state: State<'_, Arc<AppState>>,
    relation_id: String,
) -> Result<(), String> {
    agent::RelationManager::delete(state.orchestrator.pool(), &relation_id).await
}

/// 列出 Agent 的子 Agent
#[tauri::command]
pub async fn list_subagents(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let children = state.orchestrator.subagent_registry().list_children(&agent_id).await;
    Ok(children.iter().map(|r| {
        serde_json::json!({
            "id": r.id,
            "parentId": r.parent_id,
            "name": r.name,
            "task": r.task,
            "status": format!("{:?}", r.status),
            "result": r.result,
            "createdAt": r.created_at,
            "finishedAt": r.finished_at,
            "timeoutSecs": r.timeout_secs,
        })
    }).collect())
}

/// 取消子 Agent
#[tauri::command]
pub async fn cancel_subagent(
    state: State<'_, Arc<AppState>>,
    subagent_id: String,
) -> Result<(), String> {
    state.orchestrator.subagent_registry().cancel(&subagent_id).await
}

/// 批准工具执行
#[tauri::command]
pub async fn approve_tool_call(
    state: State<'_, Arc<AppState>>,
    request_id: String,
) -> Result<(), String> {
    state.orchestrator.approval_manager.approve(&request_id)
}

/// 拒绝工具执行
#[tauri::command]
pub async fn deny_tool_call(
    state: State<'_, Arc<AppState>>,
    request_id: String,
    reason: Option<String>,
) -> Result<(), String> {
    state.orchestrator.approval_manager.deny(&request_id, reason.as_deref().unwrap_or(""))
}

/// Agent 间发送消息
#[tauri::command]
pub async fn send_agent_message(
    state: State<'_, Arc<AppState>>,
    from_id: String,
    to_id: String,
    content: String,
) -> Result<(), String> {
    let msg = agent::subagent::AgentMessage {
        from: from_id,
        to: to_id,
        content,
        timestamp: chrono::Utc::now().timestamp_millis(),
    };
    state.orchestrator.subagent_registry()
        .send_message_checked(state.orchestrator.pool(), msg).await
}

/// 获取 Agent 邮箱消息（非阻塞，立即返回当前所有待读消息）
#[tauri::command]
pub async fn get_agent_mailbox(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    // 用极短超时拉取邮箱（0秒 = 只看邮箱不等待）
    let mut messages = Vec::new();
    loop {
        match state.orchestrator.subagent_registry()
            .receive_message(&agent_id, 0).await {
            Ok(msg) => messages.push(serde_json::json!({
                "from": msg.from,
                "to": msg.to,
                "content": msg.content,
                "timestamp": msg.timestamp,
            })),
            Err(_) => break,
        }
    }
    Ok(messages)
}

/// 查询子代理执行历史（DB 持久化记录）
#[tauri::command]
pub async fn list_subagent_runs(
    state: State<'_, Arc<AppState>>,
    agent_id: Option<String>,
    session_id: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<serde_json::Value>, String> {
    crate::agent::subagent::list_subagent_runs(
        state.orchestrator.pool(),
        agent_id.as_deref(),
        session_id.as_deref(),
        limit.unwrap_or(50),
    ).await
}

/// 导出 Agent 为 JSON bundle（配置 + Soul + Skills）
#[tauri::command]
pub async fn export_agent_bundle(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<String, String> {
    // 读取 Agent 基本信息
    let agent = state.orchestrator.list_agents().await?
        .into_iter().find(|a| a.id == agent_id)
        .ok_or("Agent 不存在")?;

    // 读取 Soul 文件
    let mut soul_files = serde_json::Map::new();
    let workspace_path: Option<String> = sqlx::query_scalar(
        "SELECT workspace_path FROM agents WHERE id = ?"
    ).bind(&agent_id).fetch_optional(state.db.pool()).await.ok().flatten();
    if let Some(ref wp) = workspace_path {
        let soul_dir = std::path::Path::new(wp.as_str());
        for name in &["SOUL.md", "PERSONA.md", "TOOLS.md", "FOCUS.md"] {
            let path = soul_dir.join(name);
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    soul_files.insert(name.to_string(), serde_json::Value::String(content));
                }
            }
        }
    }

    // 读取已安装 Skills
    let skills: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, description FROM installed_skills WHERE agent_id = ?"
    ).bind(&agent_id).fetch_all(state.db.pool()).await.unwrap_or_default();

    // 读取 MCP Servers
    let mcp_servers: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT name, transport, command, url FROM mcp_servers WHERE agent_id = ?"
    ).bind(&agent_id).fetch_all(state.db.pool()).await.unwrap_or_default();

    let bundle = serde_json::json!({
        "version": "1.0",
        "type": "xianzhu-agent-bundle",
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "agent": {
            "name": agent.name,
            "model": agent.model,
            "system_prompt": agent.system_prompt,
            "config": agent.config,
        },
        "soul_files": soul_files,
        "skills": skills.iter().map(|(n, d)| serde_json::json!({"name": n, "description": d})).collect::<Vec<_>>(),
        "mcp_servers": mcp_servers.iter().map(|(n, t, c, u)| serde_json::json!({"name": n, "transport": t, "command": c, "url": u})).collect::<Vec<_>>(),
    });

    serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string())
}

/// 导入 Agent bundle
#[tauri::command]
pub async fn import_agent_bundle(
    state: State<'_, Arc<AppState>>,
    bundle_json: String,
) -> Result<String, String> {
    let bundle: serde_json::Value = serde_json::from_str(&bundle_json)
        .map_err(|e| format!("JSON 解析失败: {}", e))?;

    if bundle["type"].as_str() != Some("xianzhu-agent-bundle") {
        return Err("无效的 Agent bundle 格式".into());
    }

    let agent_data = &bundle["agent"];
    let name = agent_data["name"].as_str().unwrap_or("Imported Agent");
    let model = agent_data["model"].as_str().unwrap_or("gpt-4o");
    let system_prompt = agent_data["system_prompt"].as_str().unwrap_or("");

    // 创建 Agent
    let agent_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let workspace = dirs::home_dir()
        .unwrap_or_default()
        .join(format!(".xianzhu/agents/{}", agent_id));
    let _ = std::fs::create_dir_all(&workspace);

    sqlx::query(
        "INSERT INTO agents (id, name, model, system_prompt, workspace_path, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&agent_id).bind(name).bind(model).bind(system_prompt)
    .bind(workspace.to_string_lossy().as_ref()).bind(now).bind(now)
    .execute(state.db.pool()).await.map_err(|e| e.to_string())?;

    // 写入 Soul 文件
    if let Some(files) = bundle["soul_files"].as_object() {
        for (filename, content) in files {
            if let Some(text) = content.as_str() {
                let _ = std::fs::write(workspace.join(filename), text);
            }
        }
    }

    // 导入 MCP Servers
    if let Some(servers) = bundle["mcp_servers"].as_array() {
        for srv in servers {
            let srv_id = uuid::Uuid::new_v4().to_string();
            let _ = sqlx::query(
                "INSERT INTO mcp_servers (id, agent_id, name, transport, command, url, enabled, status, created_at) VALUES (?, ?, ?, ?, ?, ?, 1, 'configured', ?)"
            )
            .bind(&srv_id).bind(&agent_id)
            .bind(srv["name"].as_str().unwrap_or(""))
            .bind(srv["transport"].as_str().unwrap_or("stdio"))
            .bind(srv["command"].as_str())
            .bind(srv["url"].as_str())
            .bind(now)
            .execute(state.db.pool()).await;
        }
    }

    Ok(serde_json::json!({
        "agentId": agent_id,
        "name": name,
        "skills": bundle["skills"].as_array().map(|a| a.len()).unwrap_or(0),
        "mcpServers": bundle["mcp_servers"].as_array().map(|a| a.len()).unwrap_or(0),
    }).to_string())
}

/// Agent 模板列表
#[tauri::command]
pub fn list_agent_templates() -> Vec<serde_json::Value> {
    agent::tools::builtin::agent_templates()
}
