//! MCP Server 管理命令

use std::sync::Arc;
use tauri::State;

use crate::AppState;

/// MCP Server 配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpServerInfo {
    pub id: String,
    pub agent_id: String,
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub url: Option<String>,
    pub env: Option<serde_json::Value>,
    pub enabled: bool,
    pub status: String,
    pub created_at: i64,
}

/// 列出 Agent 的 MCP Server
#[tauri::command]
pub async fn list_mcp_servers(
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
pub async fn add_mcp_server(
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

    state.orchestrator.mcp_manager().invalidate_cache().await;

    Ok(McpServerInfo {
        id, agent_id, name, transport, command, args, url, env,
        enabled: true, status: "configured".to_string(), created_at: now,
    })
}

/// 删除 MCP Server
#[tauri::command]
pub async fn remove_mcp_server(
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
    state.orchestrator.mcp_manager().invalidate_cache().await;
    Ok(())
}

/// 更新 MCP Server 启用状态
#[tauri::command]
pub async fn toggle_mcp_server(
    state: State<'_, Arc<AppState>>,
    server_id: String,
    enabled: bool,
) -> Result<(), String> {
    sqlx::query("UPDATE mcp_servers SET enabled = ? WHERE id = ?")
        .bind(enabled as i32).bind(&server_id)
        .execute(state.orchestrator.pool()).await
        .map_err(|e| format!("更新 MCP Server 失败: {}", e))?;
    state.orchestrator.mcp_manager().invalidate_cache().await;
    Ok(())
}

/// 导入 Claude Desktop MCP 配置
#[tauri::command]
pub async fn import_claude_mcp_config(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<Vec<McpServerInfo>, String> {
    let home = dirs::home_dir().ok_or("无法获取 home 目录")?;
    let candidates = [
        home.join(".claude/config-templates/.mcp.json"),
        home.join(".claude/.mcp.json"),
        home.join("Library/Application Support/Claude/claude_desktop_config.json"),
        home.join(".config/Claude/claude_desktop_config.json"),
    ];

    let mut content = String::new();
    let mut found_path = String::new();
    for path in &candidates {
        if let Ok(c) = tokio::fs::read_to_string(path).await {
            content = c;
            found_path = path.display().to_string();
            break;
        }
    }
    if content.is_empty() {
        return Err(format!("未找到 Claude MCP 配置。已搜索:\n{}", candidates.iter().map(|p| format!("  - {}", p.display())).collect::<Vec<_>>().join("\n")));
    }

    let config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("解析 {} 失败: {}", found_path, e))?;

    let mcp_servers = config.get("mcpServers")
        .and_then(|v| v.as_object())
        .ok_or_else(|| format!("配置 {} 中未找到 mcpServers 字段。如果是新安装，请先在 Claude 中配置 MCP Server。", found_path))?;

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
pub async fn test_mcp_connection(
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
