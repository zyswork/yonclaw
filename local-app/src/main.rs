//! YonClaw 本地应用主程序
//!
//! 基于 Tauri 框架的桌面应用入口
//! 提供 AI 代理的本地运行环境

#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]
#![allow(dead_code)]

mod agent;
mod backend_manager;
mod channel;
mod config;
mod daemon;
mod db;
mod plugin_sdk;
mod plugin_system;
mod routing;
mod gateway;
mod memory;
mod bridge;
mod channels;
mod runtime;
mod scheduler;

use std::sync::{Arc, Mutex};
use tauri::Manager;
use tauri::State;

/// 应用共享状态
///
/// 持有数据库连接和 Agent 编排器，通过 Tauri State 注入到 commands 中
struct AppState {
    db: db::Database,
    orchestrator: Arc<agent::Orchestrator>,
    scheduler: std::sync::OnceLock<scheduler::SchedulerManager>,
}

// ─── 工具函数 ─────────────────────────────────────────────────

/// 从数据库加载所有 provider 配置
async fn load_providers(db: &db::Database) -> Result<Vec<serde_json::Value>, String> {
    let json_str = db
        .get_setting("providers")
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "[]".to_string());
    serde_json::from_str(&json_str).map_err(|e| format!("解析 providers 配置失败: {}", e))
}

/// 确保 Agent 工作区已初始化
///
/// 如果 workspace_path 为 NULL（旧版本创建的 Agent），自动创建工作区并更新数据库
async fn ensure_agent_workspace(
    pool: &sqlx::SqlitePool,
    agent_id: &str,
) -> Result<agent::AgentWorkspace, String> {
    let row = sqlx::query_as::<_, (Option<String>, String)>(
        "SELECT workspace_path, name FROM agents WHERE id = ?"
    )
    .bind(agent_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询失败: {}", e))?
    .ok_or("Agent 不存在")?;

    let (workspace_path, agent_name) = row;

    if let Some(wp) = workspace_path {
        // 检查是否为旧的 .openclaw 路径，自动迁移到 .yonclaw
        let wp = if wp.contains("/.openclaw/") {
            let new_wp = wp.replace("/.openclaw/", "/.yonclaw/");
            log::info!("迁移工作区路径: {} -> {}", wp, new_wp);
            // 如果旧目录存在，移动到新路径
            let old_path = std::path::PathBuf::from(&wp);
            let new_path = std::path::PathBuf::from(&new_wp);
            if old_path.exists() && !new_path.exists() {
                if let Some(parent) = new_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::rename(&old_path, &new_path) {
                    log::warn!("迁移工作区目录失败，将创建新目录: {}", e);
                }
            }
            // 更新数据库中的路径
            let _ = sqlx::query("UPDATE agents SET workspace_path = ? WHERE id = ?")
                .bind(&new_wp)
                .bind(agent_id)
                .execute(pool)
                .await;
            new_wp
        } else {
            wp
        };
        let ws = agent::AgentWorkspace::from_path(std::path::PathBuf::from(&wp), agent_id);
        // 确保目录也存在（可能被手动删除）
        if !ws.exists() {
            ws.initialize(&agent_name).await?;
        }
        Ok(ws)
    } else {
        // 旧 Agent，自动初始化工作区
        let ws = agent::AgentWorkspace::new(agent_id);
        ws.initialize(&agent_name).await?;
        let wp = ws.root().to_string_lossy().to_string();
        sqlx::query("UPDATE agents SET workspace_path = ? WHERE id = ?")
            .bind(&wp)
            .bind(agent_id)
            .execute(pool)
            .await
            .map_err(|e| format!("更新 workspace_path 失败: {}", e))?;
        log::info!("自动初始化 Agent {} 的工作区: {}", agent_id, wp);
        Ok(ws)
    }
}

/// 保存所有 provider 配置到数据库
async fn save_providers(db: &db::Database, providers: &[serde_json::Value]) -> Result<(), String> {
    let json_str = serde_json::to_string(providers).map_err(|e| e.to_string())?;
    db.set_setting("providers", &json_str)
        .await
        .map_err(|e| format!("保存 providers 失败: {}", e))
}

/// 根据模型 ID 从 providers 中查找匹配的 provider 配置
///
/// 返回 (api_type, api_key, base_url)
fn find_provider_for_model(
    providers: &[serde_json::Value],
    model: &str,
) -> Option<(String, String, String)> {
    find_provider_for_model_with_id(providers, model, None)
}

/// 带 provider_id 的精确查找（解决同名模型串供应商）
fn find_provider_for_model_with_id(
    providers: &[serde_json::Value],
    model: &str,
    provider_id: Option<&str>,
) -> Option<(String, String, String)> {
    // 第 0 轮：按 provider_id 精确匹配
    if let Some(pid) = provider_id {
        for p in providers {
            if p["enabled"].as_bool() != Some(true) { continue; }
            if p["id"].as_str() == Some(pid) {
                let api_type = p["apiType"].as_str().unwrap_or("openai").to_string();
                let api_key = p["apiKey"].as_str().unwrap_or("").to_string();
                let base_url = p["baseUrl"].as_str().unwrap_or("").to_string();
                if !api_key.is_empty() {
                    return Some((api_type, api_key, base_url));
                }
            }
        }
    }

    // 第一轮：按模型精确匹配
    for p in providers {
        if p["enabled"].as_bool() != Some(true) { continue; }
        let models = match p["models"].as_array() {
            Some(m) => m,
            None => continue,
        };
        for m in models {
            if m["id"].as_str() == Some(model) {
                let api_type = p["apiType"].as_str().unwrap_or("openai").to_string();
                let api_key = p["apiKey"].as_str().unwrap_or("").to_string();
                let base_url = p["baseUrl"].as_str().unwrap_or("").to_string();
                if !api_key.is_empty() {
                    return Some((api_type, api_key, base_url));
                }
            }
        }
    }
    None
}

// ─── Tauri Commands ────────────────────────────────────────────

/// 保存配置项到数据库
#[tauri::command]
async fn save_config(
    state: State<'_, Arc<AppState>>,
    key: String,
    value: String,
) -> Result<(), String> {
    // 配置 key 白名单：仅允许安全的配置项
    const ALLOWED_KEYS: &[&str] = &[
        "theme", "language", "sidebar_collapsed", "default_model",
        "font_size", "auto_save", "notification_enabled",
    ];
    if !ALLOWED_KEYS.contains(&key.as_str()) {
        return Err(format!("不允许修改配置项: {}（允许: {:?}）", key, ALLOWED_KEYS));
    }
    state
        .db
        .set_setting(&key, &value)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))
}

/// 获取配置项
#[tauri::command]
async fn get_config(
    state: State<'_, Arc<AppState>>,
    key: String,
) -> Result<Option<String>, String> {
    state
        .db
        .get_setting(&key)
        .await
        .map_err(|e| format!("读取配置失败: {}", e))
}

/// 获取所有 provider 配置（脱敏 API Key）
#[tauri::command]
async fn get_providers(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<serde_json::Value>, String> {
    let providers = load_providers(&state.db).await?;
    // 脱敏 API Key：只显示前 8 位 + ...
    Ok(providers
        .into_iter()
        .map(|mut p| {
            if let Some(key) = p["apiKey"].as_str() {
                if key.len() > 8 {
                    p["apiKeyMasked"] = serde_json::Value::String(format!("{}...", &key[..8]));
                } else if !key.is_empty() {
                    p["apiKeyMasked"] = serde_json::Value::String("****".to_string());
                } else {
                    p["apiKeyMasked"] = serde_json::Value::String("".to_string());
                }
            }
            // 不返回明文 key
            p.as_object_mut().map(|o| o.remove("apiKey"));
            p
        })
        .collect())
}

/// 保存单个 provider 配置（新增或更新）
#[tauri::command]
async fn save_provider(
    state: State<'_, Arc<AppState>>,
    provider: serde_json::Value,
) -> Result<(), String> {
    let id = provider["id"]
        .as_str()
        .ok_or("provider 缺少 id 字段")?
        .to_string();

    let mut providers = load_providers(&state.db).await?;

    // 如果已有同 id，合并更新；否则追加
    if let Some(existing) = providers.iter_mut().find(|p| p["id"].as_str() == Some(&id)) {
        // 更新字段，但如果前端没传 apiKey（脱敏了），保留旧值
        let old_key = existing["apiKey"].as_str().unwrap_or("").to_string();
        *existing = provider.clone();
        if existing["apiKey"].as_str().map_or(true, |k| k.is_empty()) {
            existing["apiKey"] = serde_json::Value::String(old_key);
        }
    } else {
        providers.push(provider);
    }

    save_providers(&state.db, &providers).await
}

/// 删除 provider
#[tauri::command]
async fn delete_provider(
    state: State<'_, Arc<AppState>>,
    provider_id: String,
) -> Result<(), String> {
    let mut providers = load_providers(&state.db).await?;
    providers.retain(|p| p["id"].as_str() != Some(&provider_id));
    save_providers(&state.db, &providers).await
}

/// 获取 API 配置状态（兼容旧前端，返回哪些 provider 已配置 key）
#[tauri::command]
async fn get_api_status(
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    let providers = load_providers(&state.db).await?;
    let mut status = serde_json::Map::new();
    for p in &providers {
        if let Some(id) = p["id"].as_str() {
            let has_key = p["apiKey"].as_str().map_or(false, |k| !k.is_empty());
            let enabled = p["enabled"].as_bool().unwrap_or(true);
            status.insert(id.to_string(), serde_json::Value::Bool(has_key && enabled));
        }
    }
    Ok(serde_json::Value::Object(status))
}

/// 创建新 Agent
#[tauri::command]
async fn create_agent(
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
async fn list_agents(
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
async fn delete_agent(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<(), String> {
    state.orchestrator.delete_agent(&agent_id).await
}

/// 更新 Agent 配置（name / model / temperature / max_tokens）
///
/// 仅更新提供的字段，未提供的字段保持不变
#[tauri::command]
async fn update_agent(
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
async fn ai_generate_agent_config(
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
async fn get_agent_detail(
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
async fn get_audit_log(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<serde_json::Value>, String> {
    let entries = db::audit::query_audit_log(
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

/// 列出已安装的插件
#[tauri::command]
async fn list_plugins(
    _state: State<'_, Arc<AppState>>,
) -> Result<Vec<serde_json::Value>, String> {
    // 获取插件目录
    let plugins_dir = dirs::home_dir()
        .ok_or("���法获取 home 目录")?
        .join(".yonclaw")
        .join("plugins");

    let mut registry = agent::plugin::PluginRegistry::new(plugins_dir);
    registry.scan().await?;

    Ok(registry.list().iter().map(|p| {
        serde_json::json!({
            "name": p.manifest.name,
            "version": p.manifest.version,
            "description": p.manifest.description,
            "author": p.manifest.author,
            "status": format!("{:?}", p.status),
            "capabilities": p.manifest.capabilities.len(),
            "installedAt": p.installed_at,
        })
    }).collect())
}

/// 获取 Agent 自治配置
#[tauri::command]
async fn get_autonomy_config(
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
async fn update_autonomy_config(
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
async fn get_agent_relations(
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
async fn create_agent_relation(
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
async fn delete_agent_relation(
    state: State<'_, Arc<AppState>>,
    relation_id: String,
) -> Result<(), String> {
    agent::RelationManager::delete(state.orchestrator.pool(), &relation_id).await
}

/// 列出 Agent 的子 Agent
#[tauri::command]
async fn list_subagents(
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
async fn cancel_subagent(
    state: State<'_, Arc<AppState>>,
    subagent_id: String,
) -> Result<(), String> {
    state.orchestrator.subagent_registry().cancel(&subagent_id).await
}

/// 批准工具执行
#[tauri::command]
async fn approve_tool_call(
    state: State<'_, Arc<AppState>>,
    request_id: String,
) -> Result<(), String> {
    state.orchestrator.approval_manager.approve(&request_id)
}

/// Agent 间发送消息
#[tauri::command]
async fn send_agent_message(
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
async fn get_agent_mailbox(
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

// ─── Plaza API ──────────────────────────────────────────────

/// 发帖到 Plaza
#[tauri::command]
async fn plaza_create_post(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    content: String,
    post_type: Option<String>,
) -> Result<serde_json::Value, String> {
    let pool = state.orchestrator.pool();
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();

    // 获取 agent 名称
    let name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = ?")
        .bind(&agent_id).fetch_optional(pool).await
        .map_err(|e| e.to_string())?.unwrap_or_else(|| "Unknown".to_string());

    let pt = post_type.unwrap_or_else(|| "discovery".to_string());
    sqlx::query("INSERT INTO plaza_posts (id, agent_id, agent_name, content, post_type, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&id).bind(&agent_id).bind(&name).bind(&content).bind(&pt).bind(now)
        .execute(pool).await.map_err(|e| format!("发帖失败: {}", e))?;

    Ok(serde_json::json!({ "id": id, "agentId": agent_id, "agentName": name, "content": content, "postType": pt, "likes": 0, "createdAt": now }))
}

/// 获取 Plaza feed
#[tauri::command]
async fn plaza_list_posts(
    state: State<'_, Arc<AppState>>,
    limit: Option<i64>,
) -> Result<Vec<serde_json::Value>, String> {
    let pool = state.orchestrator.pool();
    let rows = sqlx::query_as::<_, (String, String, String, String, String, i64, i64)>(
        "SELECT id, agent_id, agent_name, content, post_type, likes, created_at FROM plaza_posts ORDER BY created_at DESC LIMIT ?"
    ).bind(limit.unwrap_or(50))
    .fetch_all(pool).await.map_err(|e| format!("查询失败: {}", e))?;

    let mut posts = Vec::new();
    for (id, aid, aname, content, pt, likes, ts) in rows {
        // 获取评论数
        let comment_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM plaza_comments WHERE post_id = ?")
            .bind(&id).fetch_one(pool).await.unwrap_or(0);
        posts.push(serde_json::json!({
            "id": id, "agentId": aid, "agentName": aname,
            "content": content, "postType": pt, "likes": likes,
            "commentCount": comment_count, "createdAt": ts,
        }));
    }
    Ok(posts)
}

/// 发表评论
#[tauri::command]
async fn plaza_add_comment(
    state: State<'_, Arc<AppState>>,
    post_id: String,
    agent_id: String,
    content: String,
) -> Result<serde_json::Value, String> {
    let pool = state.orchestrator.pool();
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();

    let name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = ?")
        .bind(&agent_id).fetch_optional(pool).await
        .map_err(|e| e.to_string())?.unwrap_or_else(|| "Unknown".to_string());

    sqlx::query("INSERT INTO plaza_comments (id, post_id, agent_id, agent_name, content, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&id).bind(&post_id).bind(&agent_id).bind(&name).bind(&content).bind(now)
        .execute(pool).await.map_err(|e| format!("评论失败: {}", e))?;

    Ok(serde_json::json!({ "id": id, "postId": post_id, "agentId": agent_id, "agentName": name, "content": content, "createdAt": now }))
}

/// 获取帖子评论
#[tauri::command]
async fn plaza_get_comments(
    state: State<'_, Arc<AppState>>,
    post_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let pool = state.orchestrator.pool();
    let rows = sqlx::query_as::<_, (String, String, String, String, i64)>(
        "SELECT id, agent_id, agent_name, content, created_at FROM plaza_comments WHERE post_id = ? ORDER BY created_at"
    ).bind(&post_id)
    .fetch_all(pool).await.map_err(|e| format!("查询失败: {}", e))?;

    Ok(rows.into_iter().map(|(id, aid, aname, content, ts)| {
        serde_json::json!({ "id": id, "agentId": aid, "agentName": aname, "content": content, "createdAt": ts })
    }).collect())
}

/// 点赞
#[tauri::command]
async fn plaza_like_post(
    state: State<'_, Arc<AppState>>,
    post_id: String,
) -> Result<(), String> {
    let pool = state.orchestrator.pool();
    sqlx::query("UPDATE plaza_posts SET likes = likes + 1 WHERE id = ?")
        .bind(&post_id).execute(pool).await.map_err(|e| format!("点赞失败: {}", e))?;
    Ok(())
}

/// 拒绝工具执行
#[tauri::command]
async fn deny_tool_call(
    state: State<'_, Arc<AppState>>,
    request_id: String,
    reason: Option<String>,
) -> Result<(), String> {
    state.orchestrator.approval_manager.deny(&request_id, reason.as_deref().unwrap_or(""))
}

/// 查询子代理执行历史（DB 持久化记录）
#[tauri::command]
async fn list_subagent_runs(
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

/// 发送消息并通过事件流推送 token（支持 Failover）
///
/// 流程：
/// 1. 从 Agent 获取完整配置（含 fallback 模型）
/// 2. 构建 FailoverExecutor，按优先级尝试每个模型
/// 3. 从 providers 配置中查找第一个有可用 provider 的模型
/// 4. 创建 mpsc channel，spawn 后台任务将 token 通过 `emit_all` 推送到前端
/// 5. 调用 orchestrator.send_message_stream 完成流式对话
/// 6. 对话结束后发送 `llm-done` 事件
#[tauri::command]
async fn send_message(
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

    // 找到第一个有可用 provider 的模型（优先使用 Agent 绑定的 provider_id）
    let agent_provider_id = {
        let row: Option<Option<String>> = sqlx::query_scalar(
            "SELECT provider_id FROM agents WHERE id = ?"
        ).bind(&agent_id).fetch_optional(state.orchestrator.pool()).await.ok().flatten();
        row.flatten()
    };

    let mut selected_model = None;
    for model in failover.all_models() {
        if let Some(provider_info) = find_provider_for_model_with_id(
            &providers, model, agent_provider_id.as_deref()
        ) {
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
        tokio::spawn(async move {
            // 1. 自动生成会话标题（第一条消息时）
            let title: Option<String> = sqlx::query_scalar(
                "SELECT title FROM chat_sessions WHERE id = ?"
            ).bind(&sid).fetch_optional(&pool).await.ok().flatten();

            if let Some(t) = &title {
                let is_default = t.starts_with("对话") || t.starts_with("New") || t == "新对话";
                if is_default {
                    let auto_title: String = msg.chars().take(20).collect::<String>().trim().to_string();
                    let auto_title = if msg.chars().count() > 20 { format!("{}...", auto_title) } else { auto_title };
                    if !auto_title.is_empty() {
                        let _ = sqlx::query("UPDATE chat_sessions SET title = ? WHERE id = ?")
                            .bind(&auto_title).bind(&sid).execute(&pool).await;
                    }
                }
            }

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

/// 获取对话历史（按 session）
#[tauri::command]
async fn get_conversations(
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
async fn get_session_messages(
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
async fn load_structured_messages(
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
async fn clear_history(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    state.orchestrator.clear_history(&session_id).await
}

// ─── Session Commands ─────────────────────────────────────────

/// 创建会话
#[tauri::command]
async fn create_session(
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
async fn cleanup_system_sessions(
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
async fn list_sessions(
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
async fn rename_session(
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
async fn delete_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
) -> Result<(), String> {
    memory::conversation::delete_session(state.orchestrator.pool(), &session_id)
        .await
        .map_err(|e| format!("删除会话失败: {}", e))
}

/// 压缩会话上下文
#[tauri::command]
async fn compact_session(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    session_id: String,
) -> Result<String, String> {
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
    let (api_type, api_key, base_url) =
        find_provider_for_model(&providers, &agent_model).ok_or("未找到供应商配置")?;
    let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };

    state
        .orchestrator
        .compact_session(&agent_id, &session_id, &api_key, &api_type, base_url_opt)
        .await
}

/// 读取 Agent 灵魂文件
#[tauri::command]
async fn read_soul_file(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    file_name: String,
) -> Result<String, String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;
    let soul_file = agent::workspace::SoulFile::from_str(&file_name)
        .ok_or_else(|| format!("未知的灵魂文件: {}", file_name))?;
    workspace.read_file(&soul_file)
        .ok_or_else(|| format!("文件不存在: {}", file_name))
}

/// 写入 Agent 灵魂文件
#[tauri::command]
async fn write_soul_file(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    file_name: String,
    content: String,
) -> Result<(), String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;
    let soul_file = agent::workspace::SoulFile::from_str(&file_name)
        .ok_or_else(|| format!("未知的灵魂文件: {}", file_name))?;
    workspace.write_file(&soul_file, &content)
}

/// 列出 Agent 的所有灵魂文件
#[tauri::command]
async fn list_soul_files(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;

    let files = agent::workspace::SoulFile::all();
    let mut result = Vec::new();
    for f in &files {
        let content = workspace.read_file(f);
        let exists = content.is_some();
        let size = content.map(|c| c.len()).unwrap_or(0);
        result.push(serde_json::json!({
            "name": f.filename(),
            "exists": exists,
            "size": size,
        }));
    }
    Ok(result)
}

/// 获取 Agent 的工具配置
///
/// 读取 TOOLS.md，合并 ToolManager 中的工具定义，返回完整的工具列表
#[tauri::command]
async fn get_agent_tools(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<serde_json::Value, String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;

    // 读取 TOOLS.md
    let tools_content = workspace.read_file(&agent::workspace::SoulFile::Tools)
        .unwrap_or_default();
    let (profile, overrides) = agent::parse_tools_config(&tools_content);

    // 从 ToolManager 获取所有工具定义
    let tool_defs = state.orchestrator.tool_manager().get_tool_definitions();

    let tools: Vec<serde_json::Value> = tool_defs.iter().map(|def| {
        let enabled = agent::is_tool_enabled(&def.name, &profile, &overrides);
        let safety = state.orchestrator.tool_manager()
            .get_safety_level(&def.name)
            .map(|s| format!("{:?}", s))
            .unwrap_or_else(|| "Safe".to_string());
        // 判断来源：有 override 则标记 override，否则标记 profile
        let source = if overrides.contains_key(&def.name) { "override" } else { "profile" };
        serde_json::json!({
            "name": def.name,
            "description": def.description,
            "safety": safety,
            "enabled": enabled,
            "source": source,
        })
    }).collect();

    Ok(serde_json::json!({
        "profile": profile,
        "tools": tools,
    }))
}

/// 设置 Agent 的工具 Profile
///
/// 更新 TOOLS.md 中的 Profile 段，保留已有的 Overrides
#[tauri::command]
async fn set_agent_tool_profile(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    profile: String,
) -> Result<(), String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;

    // 读取现有配置，保留 overrides
    let tools_content = workspace.read_file(&agent::workspace::SoulFile::Tools)
        .unwrap_or_default();
    let (_old_profile, overrides) = agent::parse_tools_config(&tools_content);

    // 写回新 profile + 旧 overrides
    let new_content = agent::format_tools_config(&profile, &overrides);
    workspace.write_file(&agent::workspace::SoulFile::Tools, &new_content)?;

    log::info!("Agent {} 工具 Profile 已更新为: {}", agent_id, profile);
    Ok(())
}

/// 设置 Agent 的单个工具覆盖
///
/// 在 TOOLS.md 的 Overrides 段中添加或移除工具覆盖
#[tauri::command]
async fn set_agent_tool_override(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    tool_name: String,
    enabled: Option<bool>,
) -> Result<(), String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;

    // 读取现有配置
    let tools_content = workspace.read_file(&agent::workspace::SoulFile::Tools)
        .unwrap_or_default();
    let (profile, mut overrides) = agent::parse_tools_config(&tools_content);

    // 更新 override：Some(bool) = 设置，None = 移除
    match enabled {
        Some(value) => {
            overrides.insert(tool_name.clone(), value);
        }
        None => {
            overrides.remove(&tool_name);
        }
    }

    // 写回
    let new_content = agent::format_tools_config(&profile, &overrides);
    workspace.write_file(&agent::workspace::SoulFile::Tools, &new_content)?;

    log::info!("Agent {} 工具覆盖已更新: {}", agent_id, tool_name);
    Ok(())
}

/// MCP Server 配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct McpServerInfo {
    id: String,
    agent_id: String,
    name: String,
    transport: String,
    command: Option<String>,
    args: Option<Vec<String>>,
    url: Option<String>,
    env: Option<serde_json::Value>,
    enabled: bool,
    status: String,
    created_at: i64,
}

/// 列出 Agent 的 MCP Server
#[tauri::command]
async fn list_mcp_servers(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<Vec<McpServerInfo>, String> {
    let rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, Option<String>, Option<String>, Option<String>, i32, String, i64)>(
        "SELECT id, agent_id, name, transport, command, args, url, env, enabled, status, created_at FROM mcp_servers WHERE agent_id = ? ORDER BY created_at"
    )
    .bind(&agent_id)
    .fetch_all(state.orchestrator.pool())
    .await
    .map_err(|e| format!("查询 MCP Server 失败: {}", e))?;

    Ok(rows.into_iter().map(|r| McpServerInfo {
        id: r.0, agent_id: r.1, name: r.2, transport: r.3,
        command: r.4,
        args: r.5.and_then(|s| serde_json::from_str(&s).ok()),
        url: r.6,
        env: r.7.and_then(|s| serde_json::from_str(&s).ok()),
        enabled: r.8 != 0, status: r.9, created_at: r.10,
    }).collect())
}

/// 添加 MCP Server
#[tauri::command]
async fn add_mcp_server(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    name: String,
    transport: String,
    command: Option<String>,
    args: Option<Vec<String>>,
    url: Option<String>,
    env: Option<serde_json::Value>,
) -> Result<McpServerInfo, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let args_json = args.as_ref().map(|a| serde_json::to_string(a).unwrap_or_default());
    let env_json = env.as_ref().map(|e| serde_json::to_string(e).unwrap_or_default());

    sqlx::query("INSERT INTO mcp_servers (id, agent_id, name, transport, command, args, url, env, enabled, status, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, 'configured', ?)")
        .bind(&id).bind(&agent_id).bind(&name).bind(&transport)
        .bind(&command).bind(&args_json).bind(&url).bind(&env_json).bind(now)
        .execute(state.orchestrator.pool()).await
        .map_err(|e| format!("添加 MCP Server 失败: {}", e))?;

    // 使 MCP 缓存失效
    state.orchestrator.mcp_manager().invalidate_cache().await;

    Ok(McpServerInfo {
        id, agent_id, name, transport, command, args, url, env,
        enabled: true, status: "configured".to_string(), created_at: now,
    })
}

/// 删除 MCP Server
#[tauri::command]
async fn remove_mcp_server(
    state: State<'_, Arc<AppState>>,
    server_id: String,
) -> Result<(), String> {
    let result = sqlx::query("DELETE FROM mcp_servers WHERE id = ?")
        .bind(&server_id)
        .execute(state.orchestrator.pool()).await
        .map_err(|e| format!("删除 MCP Server 失败: {}", e))?;
    if result.rows_affected() == 0 {
        return Err("MCP Server 不存在".to_string());
    }
    // 使 MCP 缓存失效
    state.orchestrator.mcp_manager().invalidate_cache().await;
    Ok(())
}

/// 更新 MCP Server 启用状态
#[tauri::command]
async fn toggle_mcp_server(
    state: State<'_, Arc<AppState>>,
    server_id: String,
    enabled: bool,
) -> Result<(), String> {
    sqlx::query("UPDATE mcp_servers SET enabled = ? WHERE id = ?")
        .bind(enabled as i32).bind(&server_id)
        .execute(state.orchestrator.pool()).await
        .map_err(|e| format!("更新 MCP Server 失败: {}", e))?;
    // 使 MCP 缓存失效
    state.orchestrator.mcp_manager().invalidate_cache().await;
    Ok(())
}

/// 导入 Claude Desktop MCP 配置
#[tauri::command]
async fn import_claude_mcp_config(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<Vec<McpServerInfo>, String> {
    // 读取 Claude Desktop 配置文件
    let config_path = dirs::home_dir()
        .ok_or("无法获取 home 目录")?
        .join("Library/Application Support/Claude/claude_desktop_config.json");

    let content = tokio::fs::read_to_string(&config_path).await
        .map_err(|e| format!("读取 Claude Desktop 配置失败: {}。路径: {}", e, config_path.display()))?;

    let config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("解析配置 JSON 失败: {}", e))?;

    let mcp_servers = config.get("mcpServers")
        .and_then(|v| v.as_object())
        .ok_or("配置中未找到 mcpServers 字段")?;

    let mut imported = Vec::new();
    let now = chrono::Utc::now().timestamp_millis();

    for (name, server_config) in mcp_servers {
        let id = uuid::Uuid::new_v4().to_string();
        let command = server_config.get("command").and_then(|v| v.as_str()).map(|s| s.to_string());
        let args: Option<Vec<String>> = server_config.get("args")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect());
        let env = server_config.get("env").cloned();
        let args_json = args.as_ref().map(|a| serde_json::to_string(a).unwrap_or_default());
        let env_json = env.as_ref().map(|e| serde_json::to_string(e).unwrap_or_default());

        sqlx::query("INSERT INTO mcp_servers (id, agent_id, name, transport, command, args, url, env, enabled, status, created_at) VALUES (?, ?, ?, 'stdio', ?, ?, NULL, ?, 1, 'configured', ?)")
            .bind(&id).bind(&agent_id).bind(name)
            .bind(&command).bind(&args_json).bind(&env_json).bind(now)
            .execute(state.orchestrator.pool()).await
            .map_err(|e| format!("导入 {} 失败: {}", name, e))?;

        imported.push(McpServerInfo {
            id, agent_id: agent_id.clone(), name: name.clone(),
            transport: "stdio".to_string(), command, args, url: None, env,
            enabled: true, status: "configured".to_string(), created_at: now,
        });
    }

    log::info!("导入 {} 个 Claude Desktop MCP Server", imported.len());
    Ok(imported)
}

/// 测试 MCP Server 连接
#[tauri::command]
async fn test_mcp_connection(
    state: State<'_, Arc<AppState>>,
    server_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let tools = state.orchestrator.mcp_manager()
        .test_connection(&server_id).await?;
    Ok(tools.iter().map(|t| serde_json::json!({
        "name": t.name,
        "description": t.description,
    })).collect())
}

// ─── 技能管理命令 ──────────────────────────────────────────────

/// 安装技能
#[tauri::command]
async fn install_skill(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    file_path: String,
) -> Result<serde_json::Value, String> {
    // H9: 路径安全校验
    if file_path.contains("..") {
        return Err("路径包含非法遍历序列".to_string());
    }
    let src_path = std::path::Path::new(&file_path);
    let canonical = src_path.canonicalize()
        .map_err(|e| format!("路径规范化失败: {}", e))?;
    let path_str = canonical.to_string_lossy();
    if path_str.starts_with("/etc") || path_str.starts_with("/usr") || path_str.starts_with("/System") || path_str.starts_with("/bin") || path_str.starts_with("/sbin") {
        return Err("安全限制：不允许从系统路径安装技能".to_string());
    }

    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;
    let skills_dir = workspace.root().join("skills");
    let mut skill_mgr = agent::SkillManager::scan(&skills_dir);

    let manifest = skill_mgr.install_from_file(
        &canonical,
        &agent_id,
        state.orchestrator.pool(),
    ).await?;

    Ok(serde_json::json!({
        "name": manifest.name,
        "version": manifest.version,
        "description": manifest.description,
        "tools_count": manifest.tools.len(),
    }))
}

/// 移除技能
#[tauri::command]
async fn remove_skill(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    skill_name: String,
) -> Result<(), String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;
    let skills_dir = workspace.root().join("skills");
    let mut skill_mgr = agent::SkillManager::scan(&skills_dir);

    skill_mgr.remove_skill(&skill_name, &agent_id, state.orchestrator.pool()).await
}

/// 列出已安装的技能（合并数据库记录 + 文件系统扫描）
#[tauri::command]
async fn list_skills(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    // 从数据库获取已注册的技能
    let mut db_skills = agent::SkillManager::list_installed(&agent_id, state.orchestrator.pool()).await?;
    let db_names: std::collections::HashSet<String> = db_skills.iter()
        .filter_map(|s| s.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
        .collect();

    // 扫描文件系统，补充仅在磁盘上的技能（如通过 bash_exec 安装的）
    if let Ok(workspace) = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await {
        let skills_dir = workspace.root().join("skills");
        let fs_manager = agent::SkillManager::scan(&skills_dir);
        for skill in fs_manager.index() {
            if !db_names.contains(&skill.name) {
                db_skills.push(serde_json::json!({
                    "id": format!("fs-{}", skill.name),
                    "name": skill.name,
                    "version": "",
                    "enabled": true,
                    "installed_at": "",
                    "tools_count": 0,
                    "description": skill.description,
                    "source": "filesystem",
                }));
            }
        }
    }

    Ok(db_skills)
}

/// 切换技能启用状态
#[tauri::command]
async fn toggle_skill(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    skill_name: String,
    enabled: bool,
) -> Result<(), String> {
    agent::SkillManager::toggle_skill(&skill_name, &agent_id, enabled, state.orchestrator.pool()).await
}

/// 查询 Plugin API 能力列表
#[tauri::command]
async fn list_plugin_capabilities(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<serde_json::Value>, String> {
    if let Ok(pm) = state.orchestrator.plugin_manager.lock() {
        Ok(pm.to_json())
    } else {
        Ok(Vec::new())
    }
}

/// 列出所有已注册的系统插件（含 DB 里的启用状态）
#[tauri::command]
async fn list_system_plugins(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<serde_json::Value>, String> {
    let registry = plugin_system::PluginRegistry::with_builtins();
    let mut plugins = registry.to_json();

    // 从 DB 读取全局启用状态
    let rows: Vec<(String, i32)> = sqlx::query_as(
        "SELECT plugin_id, enabled FROM plugin_configs"
    ).fetch_all(state.orchestrator.pool()).await.unwrap_or_default();
    let db_state: std::collections::HashMap<String, bool> = rows.into_iter()
        .map(|(id, en)| (id, en == 1)).collect();

    // 从 settings 读取渠道配置状态
    let tg_token: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'telegram_bot_token'")
        .fetch_optional(state.orchestrator.pool()).await.ok().flatten();
    let feishu_id: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'feishu_app_id'")
        .fetch_optional(state.orchestrator.pool()).await.ok().flatten();

    for p in &mut plugins {
        if let Some(id) = p.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()) {
            // 全局启用状态
            if let Some(&enabled) = db_state.get(&id) {
                p["enabled"] = serde_json::Value::Bool(enabled);
            } else {
                p["enabled"] = p.get("defaultEnabled").cloned().unwrap_or(serde_json::Value::Bool(true));
            }

            // 渠道连接状态
            match id.as_str() {
                "telegram-channel" => {
                    let connected = tg_token.as_ref().map_or(false, |t| !t.trim().is_empty());
                    p["connected"] = serde_json::Value::Bool(connected);
                }
                "feishu-channel" => {
                    let connected = feishu_id.as_ref().map_or(false, |t| !t.trim().is_empty());
                    p["connected"] = serde_json::Value::Bool(connected);
                }
                _ => {}
            }
        }
    }
    Ok(plugins)
}

/// 切换插件全局启用状态
#[tauri::command]
async fn toggle_system_plugin(
    state: State<'_, Arc<AppState>>,
    plugin_id: String,
    enabled: bool,
) -> Result<(), String> {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT OR REPLACE INTO plugin_configs (plugin_id, enabled, updated_at) VALUES (?, ?, ?)"
    )
    .bind(&plugin_id).bind(enabled as i32).bind(now)
    .execute(state.orchestrator.pool()).await
    .map_err(|e| format!("保存失败: {}", e))?;
    log::info!("插件 {} 已{}", plugin_id, if enabled { "启用" } else { "禁用" });
    Ok(())
}

/// 保存插件配置
#[tauri::command]
async fn save_plugin_config(
    state: State<'_, Arc<AppState>>,
    plugin_id: String,
    config_json: String,
) -> Result<(), String> {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT INTO plugin_configs (plugin_id, config_json, enabled, updated_at) VALUES (?, ?, 1, ?) ON CONFLICT(plugin_id) DO UPDATE SET config_json = excluded.config_json, updated_at = excluded.updated_at"
    )
    .bind(&plugin_id).bind(&config_json).bind(now)
    .execute(state.orchestrator.pool()).await
    .map_err(|e| format!("保存失败: {}", e))?;

    // 渠道类插件：同步配置到 settings 表（让启动时的渠道初始化能读到）
    if let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&config_json) {
        match plugin_id.as_str() {
            "telegram-channel" => {
                if let Some(token) = cfg["bot_token"].as_str() {
                    let _ = sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('telegram_bot_token', ?)")
                        .bind(token).execute(state.orchestrator.pool()).await;
                    log::info!("插件配置同步: telegram_bot_token → settings");
                }
            }
            "feishu-channel" => {
                if let Some(id) = cfg["app_id"].as_str() {
                    let _ = sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('feishu_app_id', ?)")
                        .bind(id).execute(state.orchestrator.pool()).await;
                }
                if let Some(secret) = cfg["app_secret"].as_str() {
                    let _ = sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('feishu_app_secret', ?)")
                        .bind(secret).execute(state.orchestrator.pool()).await;
                }
                log::info!("插件配置同步: feishu_app_id/secret → settings");
            }
            _ => {}
        }
    }

    Ok(())
}

/// 获取插件配置
#[tauri::command]
async fn get_plugin_config(
    state: State<'_, Arc<AppState>>,
    plugin_id: String,
) -> Result<String, String> {
    let config: Option<String> = sqlx::query_scalar(
        "SELECT config_json FROM plugin_configs WHERE plugin_id = ?"
    )
    .bind(&plugin_id)
    .fetch_optional(state.orchestrator.pool()).await
    .map_err(|e| format!("查询失败: {}", e))?;
    Ok(config.unwrap_or_else(|| "{}".to_string()))
}

/// 获取 Agent 的插件启用状态
#[tauri::command]
async fn get_agent_plugin_states(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let rows: Vec<(String, i32, Option<String>)> = sqlx::query_as(
        "SELECT plugin_id, enabled, config_override FROM agent_plugins WHERE agent_id = ?"
    )
    .bind(&agent_id)
    .fetch_all(state.orchestrator.pool()).await
    .unwrap_or_default();

    Ok(rows.into_iter().map(|(id, en, cfg)| {
        serde_json::json!({ "pluginId": id, "enabled": en == 1, "configOverride": cfg })
    }).collect())
}

/// 设置 Agent 的插件启用状态
#[tauri::command]
async fn set_agent_plugin(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    plugin_id: String,
    enabled: bool,
) -> Result<(), String> {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT OR REPLACE INTO agent_plugins (agent_id, plugin_id, enabled, updated_at) VALUES (?, ?, ?, ?)"
    )
    .bind(&agent_id).bind(&plugin_id).bind(enabled as i32).bind(now)
    .execute(state.orchestrator.pool()).await
    .map_err(|e| format!("保存失败: {}", e))?;
    Ok(())
}

/// 列出技能市场中的所有可用技能
#[tauri::command]
async fn list_marketplace_skills() -> Result<Vec<serde_json::Value>, String> {
    let marketplace_dir = dirs::home_dir()
        .ok_or("无法获取 home 目录")?
        .join(".yonclaw/marketplace");

    if !marketplace_dir.exists() {
        return Ok(Vec::new());
    }

    let mgr = agent::SkillManager::scan(&marketplace_dir);
    let mut result = Vec::new();
    for skill in mgr.index() {
        let manifest = mgr.get_manifest(&skill.name);
        let tools_count = manifest.map_or(0, |m| m.tools.len());
        result.push(serde_json::json!({
            "name": skill.name,
            "dir_name": if skill.dir_name.is_empty() { &skill.name } else { &skill.dir_name },
            "description": skill.description,
            "tools_count": tools_count,
            "trigger_keywords": skill.trigger_keywords,
        }));
    }
    Ok(result)
}

/// 从云端下载技能到本地 marketplace（内部函数）
async fn download_skill_from_hub_inner(slug: &str) -> Result<String, String> {
    let marketplace_dir = dirs::home_dir()
        .ok_or("无法获取 home 目录")?
        .join(".yonclaw/marketplace");
    let dest = marketplace_dir.join(slug);

    if dest.exists() {
        return Ok(format!("技能 {} 已存在于本地 marketplace", slug));
    }

    let client = reqwest::Client::new();

    // 1. 先获取技能元数据
    let meta_url = format!("https://zys-openclaw.com/api/v1/skill-hub/{}", slug);
    let meta_resp = client.get(&meta_url).send().await
        .map_err(|e| format!("获取技能信息失败: {}", e))?;
    let meta: serde_json::Value = meta_resp.json().await
        .map_err(|e| format!("解析技能信息失败: {}", e))?;

    if meta.get("error").is_some() {
        return Err(format!("云端技能不存在: {}", slug));
    }

    // 2. 尝试下载技能包
    let download_url = format!("https://zys-openclaw.com/api/v1/skill-hub/{}/download", slug);
    let dl_resp = client.get(&download_url).send().await;

    let has_package = if let Ok(resp) = dl_resp {
        if resp.status().is_success() {
            let bytes = resp.bytes().await.map_err(|e| format!("下载失败: {}", e))?;
            let gz = flate2::read::GzDecoder::new(&bytes[..]);
            let mut archive = tar::Archive::new(gz);
            let _ = std::fs::create_dir_all(&dest);
            archive.unpack(&dest).map_err(|e| format!("解压失败: {}", e))?;
            true
        } else {
            false
        }
    } else {
        false
    };

    // 3. 如果没有包文件，从元数据生成一个基本的 SKILL.md
    if !has_package {
        let _ = std::fs::create_dir_all(&dest);
        let name = meta["name"].as_str().unwrap_or(slug);
        let desc = meta["description"].as_str().unwrap_or("");
        let _category = meta["category"].as_str().unwrap_or("general");
        let tags: Vec<String> = meta["tags"].as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let skill_md = format!(
            "---\nname: {}\ndescription: {}\ntrigger_keywords:\n{}\n---\n\n# {}\n\n{}\n",
            slug, desc,
            tags.iter().map(|t| format!("  - {}", t)).collect::<Vec<_>>().join("\n"),
            name, desc
        );
        std::fs::write(dest.join("SKILL.md"), skill_md)
            .map_err(|e| format!("写入 SKILL.md 失败: {}", e))?;
    }

    log::info!("云端技能下载完成: {} → {}", slug, dest.display());
    Ok(format!("技能 {} 已下载到本地", slug))
}

/// 从云端技能市场下载并安装到本地 marketplace（Tauri command）
#[tauri::command]
async fn download_skill_from_hub(slug: String) -> Result<String, String> {
    download_skill_from_hub_inner(&slug).await
}

/// 将本地技能发布到云端技能市场
#[tauri::command]
async fn publish_skill_to_hub(
    skill_name: String,
    author: String,
) -> Result<String, String> {
    let marketplace_dir = dirs::home_dir()
        .ok_or("无法获取 home 目录")?
        .join(".yonclaw/marketplace");
    let skill_dir = marketplace_dir.join(&skill_name);

    if !skill_dir.exists() {
        return Err(format!("技能不存在: {}", skill_name));
    }

    let skill_md_path = skill_dir.join("SKILL.md");
    if !skill_md_path.exists() {
        return Err("缺少 SKILL.md".to_string());
    }

    // 解析 SKILL.md 元数据
    let content = std::fs::read_to_string(&skill_md_path)
        .map_err(|e| format!("读取 SKILL.md 失败: {}", e))?;

    let (name, description, tags) = parse_skill_meta(&content, &skill_name);

    let client = reqwest::Client::new();

    // 1. 发布元数据
    let publish_url = "https://zys-openclaw.com/api/v1/skill-hub/publish";
    let resp = client.post(publish_url)
        .json(&serde_json::json!({
            "slug": skill_name,
            "name": name,
            "description": description,
            "author": if author.is_empty() { "community".to_string() } else { author },
            "version": "1.0.0",
            "category": "community",
            "tags": tags,
        }))
        .send().await
        .map_err(|e| format!("发布失败: {}", e))?;

    let result: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析响应失败: {}", e))?;

    if result.get("error").is_some() {
        return Err(format!("发布失败: {}", result["error"].as_str().unwrap_or("?")));
    }

    // 2. 打包并上传技能包（tar.gz）— 排除敏感文件
    let tar_path = std::env::temp_dir().join(format!("{}.tar.gz", skill_name));
    {
        let tar_file = std::fs::File::create(&tar_path)
            .map_err(|e| format!("创建打包文件失败: {}", e))?;
        let enc = flate2::write::GzEncoder::new(tar_file, flate2::Compression::default());
        let mut tar_builder = tar::Builder::new(enc);

        // 敏感文件排除列表
        let excluded = ["cookie.txt", "config.txt", ".env", "token.txt", "credentials.json"];

        fn add_dir_filtered(builder: &mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>, dir: &std::path::Path, prefix: &std::path::Path, excluded: &[&str]) -> Result<(), String> {
            for entry in std::fs::read_dir(dir).map_err(|e| format!("读取目录失败: {}", e))? {
                let entry = entry.map_err(|e| format!("读取条目失败: {}", e))?;
                let path = entry.path();
                let file_name = entry.file_name().to_string_lossy().to_string();

                // 跳过敏感文件
                if excluded.iter().any(|&e| file_name == e) {
                    log::info!("发布跳过敏感文件: {}", file_name);
                    continue;
                }

                let archive_name = prefix.join(&file_name);
                if path.is_dir() {
                    add_dir_filtered(builder, &path, &archive_name, excluded)?;
                } else {
                    builder.append_path_with_name(&path, &archive_name)
                        .map_err(|e| format!("添加文件失败: {}", e))?;
                }
            }
            Ok(())
        }

        add_dir_filtered(&mut tar_builder, &skill_dir, std::path::Path::new("."), &excluded)
            .map_err(|e| format!("打包失败: {}", e))?;
        tar_builder.finish().map_err(|e| format!("完成打包失败: {}", e))?;
    }

    let tar_bytes = std::fs::read(&tar_path)
        .map_err(|e| format!("读取打包文件失败: {}", e))?;
    let upload_url = format!("https://zys-openclaw.com/api/v1/skill-hub/{}/upload", skill_name);
    let _ = client.post(&upload_url)
        .header("Content-Type", "application/octet-stream")
        .body(tar_bytes)
        .send().await;

    let _ = std::fs::remove_file(&tar_path);

    log::info!("技能已发布到云端: {}", skill_name);
    Ok(format!("技能 {} 已发布", skill_name))
}

/// 从 SKILL.md 内容解析元数据
fn parse_skill_meta(content: &str, default_name: &str) -> (String, String, Vec<String>) {
    let trimmed = content.trim();
    if trimmed.starts_with("---") {
        let rest = &trimmed[3..];
        if let Some(end) = rest.find("---") {
            let yaml_str = &rest[..end];
            if let Ok(data) = serde_yaml::from_str::<serde_json::Value>(yaml_str) {
                let name = data["name"].as_str().unwrap_or(default_name).to_string();
                let desc = data["description"].as_str().unwrap_or("").to_string();
                let tags = data["trigger_keywords"].as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                return (name, desc, tags);
            }
        }
    }
    // 纯 Markdown：从标题和首段推断
    let mut name = default_name.to_string();
    let mut desc = String::new();
    for line in trimmed.lines() {
        let l = line.trim();
        if l.starts_with("# ") && name == default_name {
            name = l.trim_start_matches("# ").to_string();
        } else if !l.is_empty() && !l.starts_with('#') && desc.is_empty() {
            desc = l.to_string();
            break;
        }
    }
    (name, desc, vec![])
}

/// 安装技能到指定 Agent（从 marketplace 复制到 agent skills 目录）
#[tauri::command]
async fn install_skill_to_agent(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    skill_name: String,
) -> Result<String, String> {
    let marketplace_dir = dirs::home_dir()
        .ok_or("无法获取 home 目录")?
        .join(".yonclaw/marketplace");
    let src = marketplace_dir.join(&skill_name);

    // 如果本地 marketplace 没有该技能，自动从云端下载
    if !src.exists() {
        log::info!("技能 {} 不在本地，尝试从云端下载...", skill_name);
        match download_skill_from_hub_inner(&skill_name).await {
            Ok(msg) => log::info!("技能下载完成: {}", msg),
            Err(e) => return Err(format!("技能不存在或下载失败: {}", e)),
        }
        if !src.exists() {
            return Err(format!("技能下载后仍未找到: {}", skill_name));
        }
    }

    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;
    let dest = workspace.root().join("skills").join(&skill_name);

    if dest.exists() {
        return Err(format!("技能已安装: {}", skill_name));
    }

    // 创建 skills 目录
    let _ = std::fs::create_dir_all(workspace.root().join("skills"));

    // 复制整个技能目录
    copy_dir_recursive(&src, &dest).map_err(|e| format!("复制技能失败: {}", e))?;

    // 自动安装 CLI 依赖
    let skill_md = src.join("SKILL.md");
    if skill_md.exists() {
        if let Ok(content) = std::fs::read_to_string(&skill_md) {
            auto_install_skill_deps(&skill_name, &content).await;
        }
    }

    // 清除 SkillManager 缓存，让下次对话立即感知变化
    state.orchestrator.invalidate_skill_cache();

    log::info!("技能已安装: {} -> agent {}（缓存已失效，下次对话立即生效）", skill_name, agent_id);

    // 检测是否有 .example 配置文件需要用户手动配置
    let mut setup_hints = Vec::new();
    let example_files = ["cookie.txt.example", "config.txt.example", ".env.example", "token.txt.example"];
    for ef in &example_files {
        if dest.join(ef).exists() {
            let target_name = ef.trim_end_matches(".example");
            if !dest.join(target_name).exists() {
                setup_hints.push(format!("请配置 {}", target_name));
            }
        }
    }
    // 检查依赖技能（如 oa-common）
    if let Ok(content) = std::fs::read_to_string(dest.join("SKILL.md")) {
        if content.contains("oa-common") && skill_name != "oa-common" {
            let common_dir = workspace.root().join("skills/oa-common");
            if !common_dir.exists() {
                setup_hints.push("依赖技能 oa-公共层 未安装，请先安装".to_string());
            }
        }
    }

    if setup_hints.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("安装成功！配置提示：{}", setup_hints.join("；")))
    }
}

/// 从 Agent 卸载技能
#[tauri::command]
async fn uninstall_skill_from_agent(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    skill_name: String,
) -> Result<(), String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;
    let skill_dir = workspace.root().join("skills").join(&skill_name);

    if !skill_dir.exists() {
        return Err(format!("技能未安装: {}", skill_name));
    }

    std::fs::remove_dir_all(&skill_dir)
        .map_err(|e| format!("卸载技能失败: {}", e))?;

    // 清除 SkillManager 缓存，让下次对话立即感知变化
    state.orchestrator.invalidate_skill_cache();

    log::info!("技能已卸载: {} from agent {}（缓存已失效，下次对话立即生效）", skill_name, agent_id);
    Ok(())
}

/// 自动安装技能的 CLI 依赖
///
/// 从 SKILL.md 的 frontmatter 解析 openclaw.requires.bins 和 openclaw.install，
/// 检测缺失的 CLI 工具并自动安装（brew/npm/pip）。
async fn auto_install_skill_deps(skill_name: &str, skill_md_content: &str) {
    let trimmed = skill_md_content.trim();
    if !trimmed.starts_with("---") { return; }
    let rest = &trimmed[3..];
    let end = match rest.find("---") { Some(e) => e, None => return };
    let yaml_str = &rest[..end];

    // 解析 YAML
    let data: serde_json::Value = match serde_yaml::from_str(yaml_str) {
        Ok(d) => d,
        Err(_) => return,
    };

    let meta = &data["metadata"]["openclaw"];
    let bins: Vec<String> = meta["requires"]["bins"].as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let installs = meta["install"].as_array().cloned().unwrap_or_default();

    if bins.is_empty() { return; }

    // 构建完整 PATH（包含 brew/npm/bun 路径）
    let home = dirs::home_dir().unwrap_or_default();
    let extra_path = format!(
        "/opt/homebrew/bin:/usr/local/bin:{}:{}:{}:{}",
        home.join(".yonclaw/runtime/node").to_string_lossy(),
        home.join(".npm-global/bin").to_string_lossy(),
        home.join(".bun/bin").to_string_lossy(),
        home.join(".local/bin").to_string_lossy(),
    );
    let full_path = format!("{}:{}", extra_path, std::env::var("PATH").unwrap_or_default());

    // 检测哪些 bin 缺失
    let mut missing: Vec<String> = Vec::new();
    for bin in &bins {
        let status = tokio::process::Command::new("which")
            .arg(bin)
            .env("PATH", &full_path)
            .output().await;
        if status.map(|o| !o.status.success()).unwrap_or(true) {
            missing.push(bin.clone());
        }
    }

    if missing.is_empty() {
        log::info!("技能 {}: 所有依赖已安装 ({:?})", skill_name, bins);
        return;
    }

    log::info!("技能 {}: 缺失依赖 {:?}，尝试自动安装...", skill_name, missing);

    // 找到捆绑的 Node/npm 路径
    let bundled_npm = find_bundled_npm(&home);

    // 按优先级排序安装方式：npm > brew > pip > cargo
    // 优先用我们捆绑的 npm，不依赖用户装 brew
    let mut installed = false;
    for install_item in &installs {
        if installed { break; }
        let kind = install_item["kind"].as_str().unwrap_or("");
        let result = match kind {
            "node" => {
                let package = install_item["package"].as_str().unwrap_or("");
                if package.is_empty() { continue; }
                // 用捆绑的 npm 安装
                let npm = bundled_npm.as_deref().unwrap_or("npm");
                run_install_cmd(skill_name, npm, &["install", "-g", package], &full_path).await
            }
            "brew" => {
                let formula = install_item["formula"].as_str().unwrap_or("");
                if formula.is_empty() { continue; }
                // 先检查 brew 是否存在
                let brew_exists = check_cmd_exists("brew", &full_path).await;
                if brew_exists {
                    run_install_cmd(skill_name, "brew", &["install", formula], &full_path).await
                } else {
                    // brew 不存在，尝试 npm 替代（很多 CLI 工具同时发布在 npm）
                    log::info!("技能 {}: brew 不存在，尝试 npm 安装 {}", skill_name, formula);
                    let npm = bundled_npm.as_deref().unwrap_or("npm");
                    let npm_result = run_install_cmd(skill_name, npm, &["install", "-g", formula], &full_path).await;
                    if !npm_result {
                        log::warn!("技能 {}: {} 需要 brew 安装但 brew 不可用，npm 安装也失败", skill_name, formula);
                    }
                    npm_result
                }
            }
            "pip" | "uv" => {
                let package = install_item["package"].as_str()
                    .or_else(|| install_item["args"].as_str())
                    .unwrap_or("");
                if package.is_empty() { continue; }
                if kind == "uv" && check_cmd_exists("uv", &full_path).await {
                    run_install_cmd(skill_name, "uv", &["tool", "install", package], &full_path).await
                } else {
                    run_install_cmd(skill_name, "pip3", &["install", "--user", package], &full_path).await
                }
            }
            "cargo" => {
                let crate_name = install_item["crate"].as_str().unwrap_or("");
                if crate_name.is_empty() { continue; }
                if check_cmd_exists("cargo", &full_path).await {
                    run_install_cmd(skill_name, "cargo", &["install", crate_name], &full_path).await
                } else {
                    log::warn!("技能 {}: cargo 不存在，无法安装 {}", skill_name, crate_name);
                    false
                }
            }
            _ => continue,
        };
        installed = result;
    }

    if !installed && !missing.is_empty() {
        log::warn!("技能 {}: 依赖 {:?} 自动安装失败，技能可能无法正常工作", skill_name, missing);
    }
}

/// 找到捆绑的 npm 路径
fn find_bundled_npm(home: &std::path::Path) -> Option<String> {
    let node_dir = home.join(".yonclaw/runtime/node");
    if !node_dir.exists() { return None; }
    let mut versions: Vec<_> = std::fs::read_dir(&node_dir).ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.file_name().to_string_lossy().starts_with("node-"))
        .collect();
    versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    versions.first().map(|v| {
        v.path().join("bin/npm").to_string_lossy().to_string()
    })
}

/// 检查命令是否存在
async fn check_cmd_exists(cmd: &str, path: &str) -> bool {
    tokio::process::Command::new("which")
        .arg(cmd)
        .env("PATH", path)
        .output().await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 执行安装命令，返回是否成功
async fn run_install_cmd(skill_name: &str, cmd: &str, args: &[&str], path: &str) -> bool {
    if !check_cmd_exists(cmd.split('/').last().unwrap_or(cmd), path).await {
        // 如果 cmd 是绝对路径，直接检查文件是否存在
        if !cmd.starts_with('/') || !std::path::Path::new(cmd).exists() {
            log::warn!("技能 {}: {} 不存在", skill_name, cmd);
            return false;
        }
    }
    log::info!("技能 {}: 执行 {} {:?}", skill_name, cmd, args);
    match tokio::process::Command::new(cmd)
        .args(args)
        .env("PATH", path)
        .output().await
    {
        Ok(output) => {
            if output.status.success() {
                log::info!("技能 {}: 安装成功 ({} {:?})", skill_name, cmd, args);
                true
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                log::warn!("技能 {}: 安装失败: {}", skill_name, stderr.chars().take(200).collect::<String>());
                false
            }
        }
        Err(e) => {
            log::warn!("技能 {}: 执行失败: {}", skill_name, e);
            false
        }
    }
}

/// 从应用内置资源释放 marketplace 技能
///
/// 检查 ~/.yonclaw/marketplace/ 是否为空或缺少技能，
/// 从 bundled-skills/ 资源释放到 marketplace 目录。
fn seed_marketplace_skills() {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return,
    };
    let marketplace_dir = home.join(".yonclaw/marketplace");
    let _ = std::fs::create_dir_all(&marketplace_dir);

    // 获取应用的 resource 目录（Tauri 打包后在 .app/Contents/Resources/）
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };

    // Tauri 1.x 资源路径：
    // macOS: YonClaw.app/Contents/Resources/bundled-skills/
    // Windows: <exe_dir>/bundled-skills/
    // Linux: <exe_dir>/bundled-skills/ 或 /usr/share/yonclaw/bundled-skills/
    let possible_paths = vec![
        exe_path.parent().unwrap_or(std::path::Path::new(".")).join("../Resources/bundled-skills"),
        exe_path.parent().unwrap_or(std::path::Path::new(".")).join("bundled-skills"),
        std::path::PathBuf::from("bundled-skills"), // 开发模式
    ];

    let bundled_dir = match possible_paths.iter().find(|p| p.exists()) {
        Some(p) => p.clone(),
        None => {
            log::info!("Marketplace: 未找到内置技能资源目录（开发模式正常）");
            return;
        }
    };

    // 遍历内置技能，缺失的释放到 marketplace
    let entries = match std::fs::read_dir(&bundled_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut seeded = 0;
    for entry in entries.flatten() {
        if !entry.path().is_dir() { continue; }
        let name = entry.file_name();
        let dest = marketplace_dir.join(&name);
        if !dest.exists() {
            if let Ok(_) = copy_dir_recursive(&entry.path(), &dest) {
                seeded += 1;
            }
        }
    }

    if seeded > 0 {
        log::info!("Marketplace: 已释放 {} 个内置技能到 {}", seeded, marketplace_dir.display());
    } else {
        let count = std::fs::read_dir(&marketplace_dir).map(|e| e.count()).unwrap_or(0);
        log::info!("Marketplace: {} 个技能已就绪", count);
    }
}

/// 递归复制目录
fn copy_dir_recursive(src: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dest_path = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

// ─── 运行时管理命令 ──────────────────────────────────────────

/// 检查 Node.js 运行时状态
#[tauri::command]
async fn check_runtime() -> Result<serde_json::Value, String> {
    let rt = runtime::NodeRuntime::new();
    let status = rt.status().await;
    serde_json::to_value(&status).map_err(|e| e.to_string())
}

/// 安装 Node.js 运行时（自动下载）
#[tauri::command]
async fn setup_runtime() -> Result<serde_json::Value, String> {
    let rt = runtime::NodeRuntime::new();
    rt.ensure_installed().await?;
    let status = rt.status().await;
    serde_json::to_value(&status).map_err(|e| e.to_string())
}

// ─── 定时任务 Commands ─────────────────────────────────────────

#[tauri::command]
async fn create_cron_job(
    state: State<'_, Arc<AppState>>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let request: scheduler::CreateJobRequest = serde_json::from_value(payload)
        .map_err(|e| format!("参数错误: {}", e))?;
    let sched = state.scheduler.get().ok_or("调度器未初始化")?;
    let job = scheduler::store::add_job(sched.pool(), &request).await?;
    sched.wake();
    serde_json::to_value(&job).map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_cron_job(
    state: State<'_, Arc<AppState>>,
    job_id: String,
    patch: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let request: scheduler::UpdateJobRequest = serde_json::from_value(patch)
        .map_err(|e| format!("参数错误: {}", e))?;
    let sched = state.scheduler.get().ok_or("调度器未初始化")?;
    let job = scheduler::store::update_job(sched.pool(), &job_id, &request).await?;
    sched.wake();
    serde_json::to_value(&job).map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_cron_job(
    state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<(), String> {
    let sched = state.scheduler.get().ok_or("调度器未初始化")?;
    scheduler::store::delete_job(sched.pool(), &job_id).await?;
    sched.wake();
    Ok(())
}

#[tauri::command]
async fn list_cron_jobs(
    state: State<'_, Arc<AppState>>,
    agent_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let pool = state.scheduler.get().ok_or("调度器未初始化")?.pool();
    let filter = agent_id.map(|id| scheduler::JobFilter {
        agent_id: Some(id),
        ..Default::default()
    });
    let jobs = scheduler::store::list_jobs(pool, filter.as_ref()).await?;
    serde_json::to_value(&jobs).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_cron_job(
    state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<serde_json::Value, String> {
    let pool = state.scheduler.get().ok_or("调度器未初始化")?.pool();
    let job = scheduler::store::get_job(pool, &job_id).await?;
    serde_json::to_value(&job).map_err(|e| e.to_string())
}

#[tauri::command]
async fn trigger_cron_job(
    state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<(), String> {
    let sched = state.scheduler.get().ok_or("调度器未初始化")?;
    let job = scheduler::store::get_job(sched.pool(), &job_id).await?;
    let now = chrono::Utc::now().timestamp();
    scheduler::store::update_next_run(sched.pool(), &job_id, now, job.last_run_at.unwrap_or(0)).await?;
    sched.wake();
    Ok(())
}

#[tauri::command]
async fn pause_cron_job(
    state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<(), String> {
    let sched = state.scheduler.get().ok_or("调度器未初始化")?;
    scheduler::store::disable_job(sched.pool(), &job_id).await?;
    sched.wake();
    Ok(())
}

#[tauri::command]
async fn resume_cron_job(
    state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<(), String> {
    let sched = state.scheduler.get().ok_or("调度器未初始化")?;
    let patch = scheduler::UpdateJobRequest {
        name: None, schedule: None, action_payload: None,
        timeout_secs: None, guardrails: None, retry: None,
        misfire_policy: None, catch_up_limit: None,
        enabled: Some(true),
    };
    scheduler::store::update_job(sched.pool(), &job_id, &patch).await?;
    sched.wake();
    Ok(())
}

#[tauri::command]
async fn list_cron_runs(
    state: State<'_, Arc<AppState>>,
    job_id: String,
    limit: Option<u32>,
) -> Result<serde_json::Value, String> {
    let pool = state.scheduler.get().ok_or("调度器未初始化")?.pool();
    let runs = scheduler::store::list_runs(pool, &job_id, limit.unwrap_or(20)).await?;
    serde_json::to_value(&runs).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_scheduler_status(
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    let pool = state.scheduler.get().ok_or("调度器未初始化")?.pool();
    let jobs = scheduler::store::list_jobs(pool, None).await.unwrap_or_default();
    let failure_rate = scheduler::store::recent_failure_rate(pool, 3600).await.unwrap_or(0.0);
    let running = jobs.iter().filter(|j| j.enabled).count() as u32;

    let status = scheduler::SchedulerStatus {
        running: state.scheduler.get().is_some(),
        total_jobs: jobs.len() as u32,
        enabled_jobs: running,
        running_runs: 0, // 简化
        recent_failure_rate: failure_rate,
        last_tick_at: None,
    };
    serde_json::to_value(&status).map_err(|e| e.to_string())
}

/// 健康检查 — 检测 DB 连通性和系统状态
#[tauri::command]
async fn health_check(
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    let pool = state.db.pool();

    // DB 连通性
    let db_ok = sqlx::query("SELECT 1").execute(pool).await.is_ok();

    // Agent 数量
    let agent_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agents")
        .fetch_one(pool).await.unwrap_or(0);

    // 记忆数量
    let memory_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memories")
        .fetch_one(pool).await.unwrap_or(0);

    // 今日 token 消耗
    let today_start = chrono::Local::now().date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().timestamp_millis())
        .unwrap_or(0);
    let today_tokens: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total_tokens), 0) FROM token_usage WHERE created_at >= ?"
    ).bind(today_start).fetch_one(pool).await.unwrap_or(0);

    // 响应缓存统计
    let cache_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM response_cache")
        .fetch_one(pool).await.unwrap_or(0);

    Ok(serde_json::json!({
        "status": if db_ok { "healthy" } else { "degraded" },
        "db": db_ok,
        "agents": agent_count,
        "memories": memory_count,
        "today_tokens": today_tokens,
        "response_cache_entries": cache_count,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

/// 云端 API 代理 — 前端通过此命令调用云端 API（避免跨域问题）
#[tauri::command]
async fn cloud_api_proxy(
    state: State<'_, Arc<AppState>>,
    method: String,
    path: String,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let pool = state.db.pool();

    // 从 settings 读取云端 URL
    let gateway_url: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'cloud_gateway_url'")
        .fetch_optional(pool).await.map_err(|e| e.to_string())?
        .unwrap_or_default();

    if gateway_url.is_empty() {
        return Err("未配置云端连接（cloud_gateway_url）".to_string());
    }

    let base_url = gateway_url.trim()
        .replace("ws://", "http://").replace("wss://", "https://")
        .replace("/ws/bridge", "");

    let url = format!("{}{}", base_url, path);
    let client = reqwest::Client::new();

    let req = match method.to_uppercase().as_str() {
        "POST" => client.post(&url).json(&body),
        "PUT" => client.put(&url).json(&body),
        "DELETE" => client.delete(&url),
        _ => client.get(&url),
    };

    let resp = req.send().await.map_err(|e| format!("请求失败: {}", e))?;
    let status = resp.status();

    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, text.chars().take(200).collect::<String>()));
    }

    resp.json::<serde_json::Value>().await.map_err(|e| format!("解析响应失败: {}", e))
}

/// 获取微信登录二维码
#[tauri::command]
async fn weixin_get_qrcode() -> Result<serde_json::Value, String> {
    channels::weixin::get_login_qrcode().await
}

/// 轮询微信扫码状态
#[tauri::command]
async fn weixin_poll_status(qrcode: String) -> Result<serde_json::Value, String> {
    channels::weixin::poll_qrcode_status(&qrcode).await
}

/// 保存微信 token 并立即启动轮询
#[tauri::command]
async fn weixin_save_token(
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
        channels::weixin::start_weixin(
            channels::weixin::WeixinConfig { bot_token: token },
            pool, orch, handle,
        ).await;
    });

    Ok(())
}

/// 验证 Telegram Bot Token（桌面端能翻墙访问 api.telegram.org）
#[tauri::command]
async fn verify_telegram_token(bot_token: String) -> Result<serde_json::Value, String> {
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
async fn discord_connect(
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
        channels::discord::start_gateway(
            channels::discord::DiscordConfig { bot_token: t },
            pool, orch, handle,
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
async fn slack_connect(
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
        channels::slack::start_socket_mode(
            channels::slack::SlackConfig { bot_token: bt, app_token: at },
            pool, orch, handle,
        ).await;
    });

    log::info!("Slack: 已连接 team={}, bot={}", team, user);
    Ok(serde_json::json!({
        "ok": true,
        "team": team,
        "user": user,
    }))
}

/// Token 使用统计 — 按天/模型聚合
#[tauri::command]
async fn get_token_stats(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    days: Option<i64>,
) -> Result<serde_json::Value, String> {
    let pool = state.db.pool();
    let days = days.unwrap_or(7);
    let since = chrono::Utc::now().timestamp_millis() - (days * 86_400_000);

    let rows = sqlx::query_as::<_, (String, i64, i64, i64)>(
        r#"
        SELECT model,
               SUM(input_tokens) as total_input,
               SUM(output_tokens) as total_output,
               COUNT(*) as call_count
        FROM token_usage
        WHERE agent_id = ? AND created_at >= ?
        GROUP BY model
        ORDER BY total_input + total_output DESC
        "#
    )
    .bind(&agent_id).bind(since)
    .fetch_all(pool).await
    .map_err(|e| format!("查询 token 统计失败: {}", e))?;

    let models: Vec<serde_json::Value> = rows.iter().map(|(model, input, output, count)| {
        serde_json::json!({
            "model": model,
            "input_tokens": input,
            "output_tokens": output,
            "total_tokens": input + output,
            "calls": count,
        })
    }).collect();

    let total_input: i64 = rows.iter().map(|(_, i, _, _)| i).sum();
    let total_output: i64 = rows.iter().map(|(_, _, o, _)| o).sum();

    Ok(serde_json::json!({
        "agent_id": agent_id,
        "days": days,
        "total_input_tokens": total_input,
        "total_output_tokens": total_output,
        "total_tokens": total_input + total_output,
        "models": models,
    }))
}

/// Memory Hygiene — 手动触发清理
#[tauri::command]
async fn run_memory_hygiene(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<String, String> {
    // 获取 workspace path
    let agent = state.orchestrator.get_agent_cached(&agent_id).await?;
    let wp = agent.workspace_path.as_deref();
    state.orchestrator.run_memory_hygiene(&agent_id, wp).await
}

/// 响应缓存统计
#[tauri::command]
async fn get_cache_stats(
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    let pool = state.db.pool();

    // 响应缓存
    let resp_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM response_cache")
        .fetch_one(pool).await.unwrap_or(0);
    let resp_hits: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(use_count), 0) FROM response_cache")
        .fetch_one(pool).await.unwrap_or(0);

    // 嵌入缓存
    let emb_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM embedding_cache")
        .fetch_one(pool).await.unwrap_or(0);

    Ok(serde_json::json!({
        "response_cache": {
            "entries": resp_count,
            "total_hits": resp_hits,
        },
        "embedding_cache": {
            "entries": emb_count,
        },
    }))
}

/// 读取设置
#[tauri::command]
async fn get_setting(
    state: State<'_, Arc<AppState>>,
    key: String,
) -> Result<Option<String>, String> {
    let pool = state.db.pool();
    let val: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(&key)
        .fetch_optional(pool).await
        .map_err(|e| format!("读取设置失败: {}", e))?;
    Ok(val)
}

/// 写入设置
#[tauri::command]
async fn set_setting(
    state: State<'_, Arc<AppState>>,
    key: String,
    value: String,
) -> Result<(), String> {
    let pool = state.db.pool();
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"
    )
    .bind(&key).bind(&value).bind(now)
    .execute(pool).await
    .map_err(|e| format!("写入设置失败: {}", e))?;
    Ok(())
}

/// 批量读取设置（前缀匹配）
#[tauri::command]
async fn get_settings_by_prefix(
    state: State<'_, Arc<AppState>>,
    prefix: String,
) -> Result<serde_json::Value, String> {
    let pool = state.db.pool();
    let pattern = format!("{}%", prefix);
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT key, value FROM settings WHERE key LIKE ?"
    )
    .bind(&pattern)
    .fetch_all(pool).await
    .map_err(|e| format!("查询设置失败: {}", e))?;

    let mut map = serde_json::Map::new();
    for (k, v) in rows {
        map.insert(k, serde_json::json!(v));
    }
    Ok(serde_json::Value::Object(map))
}

/// Memory Snapshot — 导出所有记忆
#[tauri::command]
async fn export_memory_snapshot(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<String, String> {
    let agent = state.orchestrator.get_agent_cached(&agent_id).await?;
    let wp = agent.workspace_path.as_deref().ok_or("Agent 没有工作区路径")?;
    let snapshot_dir = std::path::PathBuf::from(wp).join("memory");
    let _ = std::fs::create_dir_all(&snapshot_dir);
    let path = snapshot_dir.join(format!("snapshot-{}.md", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    let count = memory::snapshot_memories(state.db.pool(), &agent_id, &path).await?;
    Ok(format!("已导出 {} 条记忆到 {}", count, path.display()))
}

/// 从历史对话中提取记忆 — 用 LLM 分析对话历史，自动生成记忆条目
#[tauri::command]
async fn extract_memories_from_history(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<serde_json::Value, String> {
    let pool = state.orchestrator.pool();

    // 1. 获取最近的对话内容（最多 100 轮）
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT user_message, agent_response FROM conversations WHERE agent_id = ? ORDER BY created_at DESC LIMIT 100"
    )
    .bind(&agent_id)
    .fetch_all(pool).await
    .map_err(|e| format!("查询对话失败: {}", e))?;

    if rows.is_empty() {
        return Ok(serde_json::json!({"extracted": 0, "message": "没有可分析的对话历史"}));
    }

    // 2. 拼接对话摘要（限制总长度，避免超 token）
    let mut conversation_text = String::new();
    let max_chars = 8000;
    for (user_msg, agent_resp) in rows.iter().rev() {
        let entry = format!("用户: {}\n助手: {}\n\n", user_msg, agent_resp);
        if conversation_text.len() + entry.len() > max_chars {
            break;
        }
        conversation_text.push_str(&entry);
    }

    // 3. 查找可用 LLM Provider（从 settings 表读取 JSON 配置）
    let providers = load_providers(&state.db).await?;
    let agent_model: String = sqlx::query_scalar("SELECT model FROM agents WHERE id = ?")
        .bind(&agent_id)
        .fetch_one(pool).await
        .map_err(|e| format!("查询 Agent 失败: {}", e))?;
    let (api_type, api_key, base_url) = find_provider_for_model(&providers, &agent_model)
        .ok_or("没有可用的 LLM 供应商，请先在设置中配置")?;
    let model = agent_model;

    // 5. 调用 LLM 提取记忆
    let extraction_prompt = format!(
        r#"分析以下对话历史，提取值得长期记住的关键信息。

请以 JSON 数组格式返回，每条记忆包含：
- "type": 类型（core=用户核心信息, episodic=事件记忆, semantic=知识信息, procedural=操作流程）
- "content": 记忆内容（简洁、具体）
- "priority": 优先级 1-10

只提取重要信息，不要提取琐碎的对话内容。最多提取 10 条。

对话历史：
{}

请直接返回 JSON 数组，不要有其他文字："#,
        conversation_text
    );

    let llm_config = agent::llm::LlmConfig {
        provider: api_type.clone(),
        model: model.clone(),
        api_key: api_key.clone(),
        base_url: if base_url.is_empty() { None } else { Some(base_url.clone()) },
        temperature: Some(0.3),
        max_tokens: Some(2000),
        thinking_level: None,
    };
    let llm_client = agent::llm::LlmClient::new(llm_config);
    let messages = vec![
        agent::llm::OpenAiMessage { role: "user".to_string(), content: extraction_prompt },
    ];

    let response = llm_client.call_openai(messages, 0.3, 2000).await
        .map_err(|e| format!("LLM 调用失败: {}", e))?;

    // 6. 解析 JSON 并写入记忆
    // 尝试从响应中提取 JSON 数组
    let json_str = if let Some(start) = response.find('[') {
        if let Some(end) = response.rfind(']') {
            &response[start..=end]
        } else {
            &response
        }
    } else {
        &response
    };

    let items: Vec<serde_json::Value> = serde_json::from_str(json_str)
        .map_err(|e| format!("解析 LLM 返回的 JSON 失败: {}。原始响应: {}", e, &response[..response.len().min(200)]))?;

    // 7. 通过 SqliteMemory 管线写入（自动 FTS + 向量）
    use memory::Memory;
    let mem = if let Some(emb_config) = memory::SqliteMemory::try_load_embedding_config(pool).await {
        memory::SqliteMemory::with_embedding(pool.clone(), emb_config).await
    } else {
        memory::SqliteMemory::new(pool.clone())
    };

    let mut extracted = 0;
    for item in &items {
        let mem_type = item["type"].as_str().unwrap_or("semantic");
        let content = item["content"].as_str().unwrap_or("");
        let priority = item["priority"].as_i64().unwrap_or(5) as i32;

        if content.is_empty() { continue; }

        let category = memory::MemoryCategory::from_str(mem_type);
        let mem_priority = memory::MemoryPriority::from_i32(priority.min(3));
        let key = format!("extracted-{}-{}", mem_type, chrono::Utc::now().timestamp_millis());

        match mem.store_with_priority(&agent_id, &key, content, category, mem_priority).await {
            Ok(_) => { extracted += 1; }
            Err(e) => { log::warn!("写入提取记忆失败: {}", e); }
        }
    }

    log::info!("从对话历史提取了 {} 条记忆（共分析 {} 轮对话）", extracted, rows.len());

    Ok(serde_json::json!({
        "extracted": extracted,
        "analyzed": rows.len(),
        "message": format!("从 {} 轮对话中提取了 {} 条记忆", rows.len(), extracted),
    }))
}

#[tokio::main]
async fn main() {
    // 初始化日志（同时输出到文件和 stderr）
    {
        use std::io::Write;

        // 日志文件路径：~/Library/Logs/YonClaw/yonclaw.log (macOS)
        // 其他平台降级到 ~/.yonclaw/logs/yonclaw.log
        let log_dir = if cfg!(target_os = "macos") {
            dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("Library/Logs/YonClaw")
        } else {
            dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".yonclaw/logs")
        };
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("yonclaw.log");

        // 打开日志文件（追加模式），超过 10MB 时截断
        if log_path.exists() {
            if let Ok(meta) = std::fs::metadata(&log_path) {
                if meta.len() > 10 * 1024 * 1024 {
                    let _ = std::fs::write(&log_path, ""); // 截断
                }
            }
        }
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .ok();

        let log_file = std::sync::Arc::new(std::sync::Mutex::new(log_file));

        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
            .format(move |buf, record| {
                let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                let line = format!(
                    "[{} {} {}] {}\n",
                    ts,
                    record.level(),
                    record.target(),
                    record.args()
                );
                // 写到 stderr（终端调试时可见）
                let _ = buf.write_all(line.as_bytes());
                // 同时写到文件
                if let Ok(mut guard) = log_file.lock() {
                    if let Some(ref mut f) = *guard {
                        let _ = f.write_all(line.as_bytes());
                        let _ = f.flush();
                    }
                }
                Ok(())
            })
            .init();

        eprintln!("📝 日志文件: {}", log_path.display());
    }

    // CLI 参数处理（在 Tauri 启动前）
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--install-service") {
        let exe = std::env::current_exe().unwrap_or_default();
        let mgr = daemon::ServiceManager::new(exe);
        match mgr.install() {
            Ok(msg) => { eprintln!("✅ {}", msg); std::process::exit(0); }
            Err(e) => { eprintln!("❌ {}", e); std::process::exit(1); }
        }
    }
    if args.iter().any(|a| a == "--uninstall-service") {
        let exe = std::env::current_exe().unwrap_or_default();
        let mgr = daemon::ServiceManager::new(exe);
        match mgr.uninstall() {
            Ok(msg) => { eprintln!("✅ {}", msg); std::process::exit(0); }
            Err(e) => { eprintln!("❌ {}", e); std::process::exit(1); }
        }
    }
    if args.iter().any(|a| a == "--service-status") {
        let exe = std::env::current_exe().unwrap_or_default();
        let mgr = daemon::ServiceManager::new(exe);
        eprintln!("服务已安装: {}", mgr.is_installed());
        std::process::exit(0);
    }

    // 记录启动开始时间
    let app_start_time = std::time::Instant::now();
    log::info!("⏱️  启动 YonClaw 本地应用");

    // 统一配置加载
    let app_config = config::AppConfig::load(&config::AppConfig::default_path());
    log::info!("配置加载完成: data_dir={}", app_config.data_dir.display());

    // 初始化数据库
    let data_dir = &app_config.data_dir;
    std::fs::create_dir_all(data_dir).expect("无法创建数据目录");
    let db_path = data_dir.join("yonclaw.db");
    let db = match db::Database::new(db_path.to_str().unwrap()).await {
        Ok(db) => {
            log::info!("数据库初始化成功");
            db
        }
        Err(e) => {
            log::error!("数据库初始化失败: {}", e);
            return;
        }
    };

    // 检查 Node.js 运行时状态（启动时异步检查，不阻塞启动流程）
    tokio::spawn(async {
        let node_rt = runtime::NodeRuntime::new();
        match node_rt.status().await {
            runtime::node::RuntimeStatus::Ready { version, path } => {
                log::info!("Node.js 运行时就绪: {} ({})", version, path);
            }
            runtime::node::RuntimeStatus::NotInstalled => {
                log::warn!("Node.js 运行时未安装，技能工具可能无法执行。请通过设置页面安装。");
            }
            _ => {}
        }
    });

    // 自动从环境变量导入 API Key 到 provider 配置
    // 支持 OPENAI_API_KEY、ANTHROPIC_API_KEY、DEEPSEEK_API_KEY
    {
        let mut providers = load_providers(&db).await.unwrap_or_default();

        // 定义环境变量 → provider 映射
        let env_mappings = vec![
            (
                "OPENAI_API_KEY",
                "openai",
                "OpenAI",
                "openai",
                "https://api.openai.com/v1",
                vec![
                    ("gpt-4o-mini", "GPT-4o Mini"),
                    ("gpt-4o", "GPT-4o"),
                    ("gpt-4-turbo", "GPT-4 Turbo"),
                ],
            ),
            (
                "ANTHROPIC_API_KEY",
                "anthropic",
                "Anthropic",
                "anthropic",
                "https://api.anthropic.com/v1",
                vec![
                    ("claude-sonnet-4-20250514", "Claude Sonnet 4"),
                    ("claude-haiku-4-20250414", "Claude Haiku 4"),
                    ("claude-opus-4-20250514", "Claude Opus 4"),
                ],
            ),
            (
                "DEEPSEEK_API_KEY",
                "deepseek",
                "DeepSeek",
                "openai",
                "https://api.deepseek.com",
                vec![
                    ("deepseek-chat", "DeepSeek Chat"),
                    ("deepseek-reasoner", "DeepSeek Reasoner"),
                ],
            ),
        ];

        for (env_var, id, name, api_type, base_url, models) in &env_mappings {
            if let Ok(key) = std::env::var(env_var) {
                if key.is_empty() {
                    continue;
                }
                // 检查是否已存在此 provider
                if let Some(existing) = providers.iter_mut().find(|p| p["id"].as_str() == Some(id)) {
                    // 仅在未配置 key 时导入
                    if existing["apiKey"].as_str().map_or(true, |k| k.is_empty()) {
                        existing["apiKey"] = serde_json::Value::String(key.clone());
                        log::info!("已从环境变量 {} 导入 API Key 到 provider {}", env_var, id);
                    }
                } else {
                    // 创建新 provider
                    let model_array: Vec<serde_json::Value> = models
                        .iter()
                        .map(|(mid, mname)| {
                            serde_json::json!({"id": mid, "name": mname})
                        })
                        .collect();
                    providers.push(serde_json::json!({
                        "id": id,
                        "name": name,
                        "apiType": api_type,
                        "baseUrl": base_url,
                        "apiKey": key,
                        "models": model_array,
                        "enabled": true,
                    }));
                    log::info!("已从环境变量 {} 创建 provider {}", env_var, id);
                }
            }
        }

        // 如果没有任何 provider，初始化默认列表（无 key）
        if providers.is_empty() {
            providers = vec![
                serde_json::json!({
                    "id": "openai",
                    "name": "OpenAI",
                    "apiType": "openai",
                    "baseUrl": "https://api.openai.com/v1",
                    "apiKey": "",
                    "models": [
                        {"id": "gpt-4o-mini", "name": "GPT-4o Mini"},
                        {"id": "gpt-4o", "name": "GPT-4o"},
                        {"id": "gpt-4-turbo", "name": "GPT-4 Turbo"},
                    ],
                    "enabled": true,
                }),
                serde_json::json!({
                    "id": "anthropic",
                    "name": "Anthropic",
                    "apiType": "anthropic",
                    "baseUrl": "https://api.anthropic.com/v1",
                    "apiKey": "",
                    "models": [
                        {"id": "claude-sonnet-4-20250514", "name": "Claude Sonnet 4"},
                        {"id": "claude-haiku-4-20250414", "name": "Claude Haiku 4"},
                        {"id": "claude-opus-4-20250514", "name": "Claude Opus 4"},
                    ],
                    "enabled": true,
                }),
                serde_json::json!({
                    "id": "deepseek",
                    "name": "DeepSeek",
                    "apiType": "openai",
                    "baseUrl": "https://api.deepseek.com",
                    "apiKey": "",
                    "models": [
                        {"id": "deepseek-chat", "name": "DeepSeek Chat"},
                        {"id": "deepseek-reasoner", "name": "DeepSeek Reasoner"},
                    ],
                    "enabled": true,
                }),
            ];
        }

        let _ = save_providers(&db, &providers).await;
    }

    // 初始化记忆体系统
    let _memory_system = memory::MemorySystem::new(db.pool().clone());
    log::info!("记忆体系统初始化成功");

    // 初始化消息网关
    let _gateway = gateway::MessageGateway::new();
    log::info!("消息网关初始化成功");

    // 在 move db 之前克隆连接池，用于创建编排器
    let pool_clone = db.pool().clone();

    // 创建编排器
    let mut orchestrator = agent::Orchestrator::new(pool_clone.clone());
    log::info!("Agent 编排器初始化成功");

    // 创建调度器共享的 Notify（cron 工具和调度引擎共用）
    let scheduler_notify = std::sync::Arc::new(tokio::sync::Notify::new());

    // 注册 cron 工具到编排器
    {
        use scheduler::tools::*;
        let pool = pool_clone.clone();
        let notify = scheduler_notify.clone();
        orchestrator.tool_manager_mut().register_tool(Box::new(CronAddTool::new(pool.clone(), notify.clone())));
        orchestrator.tool_manager_mut().register_tool(Box::new(CronListTool::new(pool.clone())));
        orchestrator.tool_manager_mut().register_tool(Box::new(CronRemoveTool::new(pool.clone(), notify.clone())));
        orchestrator.tool_manager_mut().register_tool(Box::new(CronUpdateTool::new(pool.clone(), notify.clone())));
        orchestrator.tool_manager_mut().register_tool(Box::new(CronTriggerTool::new(pool.clone(), notify.clone())));
        log::info!("Cron 工具已注册到编排器");
    }

    // 包装编排器为 Arc（工具注册已完成）
    let orchestrator = Arc::new(orchestrator);

    // 注入 Orchestrator 到 DelegateTaskTool（解决循环依赖）
    agent::delegate::inject_orchestrator(orchestrator.clone());

    // 注册内置插件到 PluginManager
    if let Ok(mut pm) = orchestrator.plugin_manager.lock() {
        plugin_system::register_builtin_plugins(&mut pm, pool_clone.clone());
        log::info!("PluginManager: {} 个插件已加载", pm.list_plugins().len());
    }

    // 构建应用共享状态
    let app_state = Arc::new(AppState { db, orchestrator: orchestrator.clone(), scheduler: std::sync::OnceLock::new() });

    // 创建 BackendManager 并包装为可共享的引用
    let backend_manager = Arc::new(Mutex::new(backend_manager::BackendManager::new()));

    // 尝试启动本地后端进程（可选，后端可能在远程服务器上）
    {
        let mut bm = backend_manager.lock().unwrap();
        match bm.start().await {
            Ok(_) => {
                log::info!("Node.js 后端启动成功");
            }
            Err(e) => {
                log::warn!("本地后端未启动: {}（将使用远程后端）", e);
            }
        }
    }

    // 启动 API 网关（如果环境变量 YONCLAW_API_PORT 配置了端口）
    if let Ok(port_str) = std::env::var("YONCLAW_API_PORT") {
        if let Ok(port) = port_str.parse::<u16>() {
            let gw_config = gateway::api::ApiGatewayConfig {
                port,
                bind_address: std::env::var("YONCLAW_API_BIND").unwrap_or_else(|_| "127.0.0.1".to_string()),
                api_key: std::env::var("YONCLAW_API_KEY").ok(),
            };
            let gw_state = std::sync::Arc::new(gateway::api::GatewayState {
                config: gw_config,
                pool: pool_clone.clone(),
                orchestrator: Some(orchestrator.clone()),
                scheduler_notify: Some(scheduler_notify.clone()),
            });
            tokio::spawn(async move {
                if let Err(e) = gateway::api::start_api_gateway(gw_state).await {
                    log::error!("API 网关启动失败: {}", e);
                }
            });
        }
    }

    // 启动 Desktop Bridge（如果配置了云端 URL）
    {
        let bridge_pool = pool_clone.clone();
        let bridge_orch = orchestrator.clone();
        tokio::spawn(async move {
            // 从 settings 读取云端配置
            let gateway_url: Option<String> = sqlx::query_scalar(
                "SELECT value FROM settings WHERE key = 'cloud_gateway_url'"
            ).fetch_optional(&bridge_pool).await.ok().flatten();

            let api_key: Option<String> = sqlx::query_scalar(
                "SELECT value FROM settings WHERE key = 'cloud_api_key'"
            ).fetch_optional(&bridge_pool).await.ok().flatten();

            if let (Some(url), Some(key)) = (gateway_url, api_key) {
                let url = url.trim().to_string();
                let key = key.trim().to_string();
                if !url.is_empty() && !key.is_empty() {
                    log::info!("Bridge: 配置已找到，连接 {}", url);

                    // 获取 Agent 列表和工具列表
                    let agents = bridge_orch.list_agents().await.unwrap_or_default()
                        .into_iter().map(|a| a.id).collect::<Vec<_>>();
                    let tools = bridge_orch.tool_manager().get_tool_definitions()
                        .into_iter().map(|t| t.name).collect::<Vec<_>>();

                    let device_id = format!("desktop-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("x"));

                    let config = bridge::BridgeConfig {
                        gateway_url: url,
                        api_key: key,
                        device_id,
                        heartbeat_secs: 30,
                    };

                    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel();

                    // 启动 Bridge 客户端
                    let client = bridge::BridgeClient::new(config)
                        .with_agents(agents)
                        .with_capabilities(tools);
                    client.start(bridge_pool.clone(), bridge_orch.clone(), msg_tx).await;

                    // 处理转发消息
                    let orch = bridge_orch.clone();
                    let pool = bridge_pool.clone();
                    tokio::spawn(async move {
                        while let Some(fwd) = msg_rx.recv().await {
                            // 映射 agent_id: "default" → 第一个本地 Agent
                            let actual_agent_id = if fwd.agent_id == "default" {
                                orch.list_agents().await.ok()
                                    .and_then(|a| a.first().map(|x| x.id.clone()))
                                    .unwrap_or(fwd.agent_id.clone())
                            } else {
                                fwd.agent_id.clone()
                            };
                            log::info!("Bridge: 处理转发消息 agent={} session={}", actual_agent_id, fwd.session_id);
                            // 查找 provider
                            let providers_json: Option<String> = sqlx::query_scalar(
                                "SELECT value FROM settings WHERE key = 'providers'"
                            ).fetch_optional(&pool).await.ok().flatten();

                            if let Some(pj) = providers_json {
                                let providers: Vec<serde_json::Value> = serde_json::from_str(&pj).unwrap_or_default();
                                // 找第一个有 key 的 provider
                                for p in &providers {
                                    if p["enabled"].as_bool() != Some(true) { continue; }
                                    let api_key = p["apiKey"].as_str().unwrap_or("");
                                    if api_key.is_empty() { continue; }
                                    let api_type = p["apiType"].as_str().unwrap_or("openai");
                                    let base_url = p["baseUrl"].as_str().unwrap_or("");
                                    let base_url_opt = if base_url.is_empty() { None } else { Some(base_url) };

                                    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                                    let orch_clone = orch.clone();
                                    let fwd_clone = fwd.clone();

                                    // 执行对话
                                    // 确保本地有这个 session（可能是从 Telegram 来的）
                                    let _ = sqlx::query(
                                        "INSERT OR IGNORE INTO chat_sessions (id, agent_id, title, created_at) VALUES (?, ?, ?, ?)"
                                    )
                                    .bind(&fwd_clone.session_id)
                                    .bind(&actual_agent_id)
                                    .bind(format!("[{}] 转发", fwd_clone.sender_channel))
                                    .bind(chrono::Utc::now().timestamp_millis())
                                    .execute(&pool).await;

                                    match orch_clone.send_message_stream(
                                        &actual_agent_id, &fwd_clone.session_id, &fwd_clone.message,
                                        api_key, api_type, base_url_opt, tx, None,
                                    ).await {
                                        Ok(response) => {
                                            log::info!("Bridge: 转发消息处理完成 len={}", response.len());
                                        }
                                        Err(e) => {
                                            log::error!("Bridge: 转发消息处理失败: {}", e);
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                    });
                } else {
                    log::info!("Bridge: 云端配置为空，跳过连接");
                }
            } else {
                log::info!("Bridge: 未配置云端连接（设置 cloud_gateway_url 和 cloud_api_key 启用）");
            }
        });
    }

    // 释放内置技能到 marketplace（首次安装或 marketplace 为空时）
    seed_marketplace_skills();

    // 记录初始化完成时间
    let init_elapsed = app_start_time.elapsed();
    log::info!(
        "✓ 应用初始化完成，耗时: {:.2}s",
        init_elapsed.as_secs_f64()
    );

    // 构建 Tauri 应用，注册 commands 和共享状态
    let orchestrator_for_setup = orchestrator.clone();
    let pool_for_setup = pool_clone.clone();
    let notify_for_setup = scheduler_notify.clone();
    let app_state_for_setup = app_state.clone();

    let app = tauri::Builder::default()
        .manage(app_state.clone())
        .setup(move |app| {
            // 在窗口创建前启动调度引擎
            let handle = app.handle().clone();
            let sched = scheduler::SchedulerManager::start(
                pool_for_setup.clone(),
                notify_for_setup,
                orchestrator_for_setup,
                handle,
            );
            let _ = app_state_for_setup.scheduler.set(sched);
            log::info!("✓ 调度引擎已启动");

            // 种子任务注入
            let pool_for_seed = pool_for_setup.clone();
            tokio::spawn(async move {
                if let Err(e) = scheduler::seed::seed_default_jobs(&pool_for_seed).await {
                    log::warn!("种子任务注入失败: {}", e);
                }
            });

            // 启动 Telegram 本地轮询（如果配置了 Bot Token）
            {
                let tg_pool = pool_for_setup.clone();
                let tg_orch = app_state_for_setup.orchestrator.clone();
                let tg_handle = app.handle().clone();
                tokio::spawn(async move {
                    let token: Option<String> = sqlx::query_scalar(
                        "SELECT value FROM settings WHERE key = 'telegram_bot_token'"
                    ).fetch_optional(&tg_pool).await.ok().flatten();

                    if let Some(token) = token {
                        if !token.trim().is_empty() {
                            channels::telegram::start_polling(
                                channels::telegram::TelegramConfig { bot_token: token.trim().to_string() },
                                tg_pool, tg_orch, tg_handle,
                            ).await;
                        }
                    } else {
                        log::info!("Telegram: 未配置 Bot Token，跳过本地轮询");
                    }
                });
            }

            // 启动飞书连接（如果配置了 App ID）
            {
                let fs_pool = pool_for_setup.clone();
                let fs_orch = app_state_for_setup.orchestrator.clone();
                let fs_handle = app.handle().clone();
                tokio::spawn(async move {
                    let app_id: Option<String> = sqlx::query_scalar(
                        "SELECT value FROM settings WHERE key = 'feishu_app_id'"
                    ).fetch_optional(&fs_pool).await.ok().flatten();
                    let app_secret: Option<String> = sqlx::query_scalar(
                        "SELECT value FROM settings WHERE key = 'feishu_app_secret'"
                    ).fetch_optional(&fs_pool).await.ok().flatten();

                    if let (Some(id), Some(secret)) = (app_id, app_secret) {
                        if !id.trim().is_empty() && !secret.trim().is_empty() {
                            channels::feishu::start_feishu(
                                channels::feishu::FeishuConfig {
                                    app_id: id.trim().to_string(),
                                    app_secret: secret.trim().to_string(),
                                },
                                fs_pool, fs_orch, fs_handle,
                            ).await;
                        }
                    } else {
                        log::info!("飞书: 未配置 App ID/Secret，跳过连接");
                    }
                });
            }

            // 启动微信长轮询（如果配置了 token）
            {
                let wx_pool = pool_for_setup.clone();
                let wx_orch = app_state_for_setup.orchestrator.clone();
                let wx_handle = app.handle().clone();
                tokio::spawn(async move {
                    let token: Option<String> = sqlx::query_scalar(
                        "SELECT value FROM settings WHERE key = 'weixin_bot_token'"
                    ).fetch_optional(&wx_pool).await.ok().flatten();

                    if let Some(token) = token {
                        if !token.trim().is_empty() {
                            channels::weixin::start_weixin(
                                channels::weixin::WeixinConfig { bot_token: token.trim().to_string() },
                                wx_pool, wx_orch, wx_handle,
                            ).await;
                        }
                    } else {
                        log::info!("微信: 未配置 bot token，跳过（需先扫码登录）");
                    }
                });
            }

            // Discord Bot
            {
                let dc_pool = pool_for_setup.clone();
                let dc_orch = app_state_for_setup.orchestrator.clone();
                let dc_handle = app.handle().clone();
                tokio::spawn(async move {
                    let token: Option<String> = sqlx::query_scalar(
                        "SELECT value FROM settings WHERE key = 'discord_bot_token'"
                    ).fetch_optional(&dc_pool).await.ok().flatten();

                    if let Some(token) = token {
                        if !token.trim().is_empty() {
                            channels::discord::start_gateway(
                                channels::discord::DiscordConfig { bot_token: token.trim().to_string() },
                                dc_pool, dc_orch, dc_handle,
                            ).await;
                        }
                    } else {
                        log::info!("Discord: 未配置 bot token，跳过");
                    }
                });
            }

            // Slack Socket Mode
            {
                let sk_pool = pool_for_setup.clone();
                let sk_orch = app_state_for_setup.orchestrator.clone();
                let sk_handle = app.handle().clone();
                tokio::spawn(async move {
                    let bot_token: Option<String> = sqlx::query_scalar(
                        "SELECT value FROM settings WHERE key = 'slack_bot_token'"
                    ).fetch_optional(&sk_pool).await.ok().flatten();
                    let app_token: Option<String> = sqlx::query_scalar(
                        "SELECT value FROM settings WHERE key = 'slack_app_token'"
                    ).fetch_optional(&sk_pool).await.ok().flatten();

                    if let (Some(bt), Some(at)) = (bot_token, app_token) {
                        if !bt.trim().is_empty() && !at.trim().is_empty() {
                            channels::slack::start_socket_mode(
                                channels::slack::SlackConfig {
                                    bot_token: bt.trim().to_string(),
                                    app_token: at.trim().to_string(),
                                },
                                sk_pool, sk_orch, sk_handle,
                            ).await;
                        }
                    } else {
                        log::info!("Slack: 未配置 token，跳过");
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            save_config,
            get_config,
            get_providers,
            save_provider,
            delete_provider,
            get_api_status,
            create_agent,
            list_agents,
            delete_agent,
            update_agent,
            get_agent_detail,
            ai_generate_agent_config,
            get_audit_log,
            list_plugins,
            get_autonomy_config,
            update_autonomy_config,
            get_agent_relations,
            create_agent_relation,
            delete_agent_relation,
            list_subagents,
            cancel_subagent,
            list_subagent_runs,
            approve_tool_call,
            deny_tool_call,
            send_agent_message,
            get_agent_mailbox,
            plaza_create_post,
            plaza_list_posts,
            plaza_add_comment,
            plaza_get_comments,
            plaza_like_post,
            send_message,
            get_conversations,
            get_session_messages,
            load_structured_messages,
            clear_history,
            create_session,
            list_sessions,
            rename_session,
            delete_session,
            compact_session,
            read_soul_file,
            write_soul_file,
            list_soul_files,
            get_agent_tools,
            set_agent_tool_profile,
            set_agent_tool_override,
            list_mcp_servers,
            add_mcp_server,
            remove_mcp_server,
            toggle_mcp_server,
            import_claude_mcp_config,
            test_mcp_connection,
            install_skill,
            remove_skill,
            list_skills,
            toggle_skill,
            list_system_plugins,
            list_plugin_capabilities,
            toggle_system_plugin,
            save_plugin_config,
            get_plugin_config,
            get_agent_plugin_states,
            set_agent_plugin,
            list_marketplace_skills,
            download_skill_from_hub,
            publish_skill_to_hub,
            install_skill_to_agent,
            uninstall_skill_from_agent,
            check_runtime,
            setup_runtime,
            // 定时任务
            create_cron_job,
            update_cron_job,
            delete_cron_job,
            list_cron_jobs,
            get_cron_job,
            trigger_cron_job,
            pause_cron_job,
            resume_cron_job,
            list_cron_runs,
            get_scheduler_status,
            health_check,
            get_token_stats,
            get_token_daily_stats,
            run_memory_hygiene,
            get_cache_stats,
            get_setting,
            set_setting,
            get_settings_by_prefix,
            export_memory_snapshot,
            extract_memories_from_history,
            cleanup_system_sessions,
            cloud_api_proxy,
            weixin_get_qrcode,
            weixin_poll_status,
            weixin_save_token,
            verify_telegram_token,
            discord_connect,
            slack_connect,
        ])
        .build(tauri::generate_context!())
        .expect("error building tauri application");

    // 在应用事件循环中处理退出
    let backend_manager_clone = backend_manager.clone();
    let app_state_clone = app_state.clone();
    app.run(move |_app_handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            api.prevent_exit();

            // 关闭调度引擎
            if let Some(sched) = app_state_clone.scheduler.get() {
                sched.shutdown();
                log::info!("✓ 调度引擎已关闭");
            }

            // 在单独的线程中执行异步清理
            let backend_manager_clone = backend_manager_clone.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    if let Ok(mut bm) = backend_manager_clone.lock() {
                        log::info!("应用关闭，停止后端进程...");
                        bm.stop().await;
                        log::info!("✓ 后端进程已停止");
                    }
                    std::process::exit(0);
                });
            });
        }
    });
}

/// Token 使用日统计（最近 N 天，每天一条）
#[tauri::command]
async fn get_token_daily_stats(
    state: State<'_, Arc<AppState>>,
    agent_id: Option<String>,
    days: Option<i64>,
) -> Result<Vec<serde_json::Value>, String> {
    let pool = state.db.pool();
    let days = days.unwrap_or(30);
    let since = chrono::Utc::now().timestamp_millis() - (days * 86_400_000);

    let rows = if let Some(ref aid) = agent_id {
        sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
            r#"
            SELECT DATE(created_at / 1000, 'unixepoch', 'localtime') as day,
                   SUM(input_tokens), SUM(output_tokens), SUM(total_tokens), COUNT(*)
            FROM token_usage
            WHERE agent_id = ? AND created_at >= ?
            GROUP BY day ORDER BY day
            "#
        ).bind(aid).bind(since).fetch_all(pool).await
    } else {
        sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
            r#"
            SELECT DATE(created_at / 1000, 'unixepoch', 'localtime') as day,
                   SUM(input_tokens), SUM(output_tokens), SUM(total_tokens), COUNT(*)
            FROM token_usage
            WHERE created_at >= ?
            GROUP BY day ORDER BY day
            "#
        ).bind(since).fetch_all(pool).await
    }.map_err(|e| format!("查询日统计失败: {}", e))?;

    Ok(rows.iter().map(|(day, input, output, total, calls)| {
        serde_json::json!({
            "date": day, "inputTokens": input, "outputTokens": output,
            "totalTokens": total, "calls": calls,
        })
    }).collect())
}
