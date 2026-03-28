//! Agent Soul 文件和工具配置命令

use std::sync::Arc;
use tauri::State;

use crate::agent;
use crate::AppState;
use super::helpers::ensure_agent_workspace;

/// 读取 Agent 灵魂文件
#[tauri::command]
pub async fn read_soul_file(
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
pub async fn write_soul_file(
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
pub async fn list_soul_files(
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

/// 读取 Agent 的 Standing Orders
#[tauri::command]
pub async fn read_standing_orders(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<String, String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;
    let path = workspace.root().join("STANDING_ORDERS.md");
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(content),
        Err(_) => Ok(String::new()), // 文件不存在返回空字符串
    }
}

/// 写入 Agent 的 Standing Orders
#[tauri::command]
pub async fn write_standing_orders(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    content: String,
) -> Result<(), String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;
    let path = workspace.root().join("STANDING_ORDERS.md");
    std::fs::write(&path, &content)
        .map_err(|e| format!("写入 STANDING_ORDERS.md 失败: {}", e))?;
    // 清除缓存，确保下次读取获取最新内容
    workspace.invalidate_cache();
    log::info!("Agent {} Standing Orders 已更新，长度: {} 字节", agent_id, content.len());
    Ok(())
}

/// 获取 Agent 的工具配置
#[tauri::command]
pub async fn get_agent_tools(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<serde_json::Value, String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;

    let tools_content = workspace.read_file(&agent::workspace::SoulFile::Tools)
        .unwrap_or_default();
    let (profile, overrides) = agent::parse_tools_config(&tools_content);

    let tool_defs = state.orchestrator.tool_manager().get_tool_definitions();

    let tools: Vec<serde_json::Value> = tool_defs.iter().map(|def| {
        let enabled = agent::is_tool_enabled(&def.name, &profile, &overrides);
        let safety = state.orchestrator.tool_manager()
            .get_safety_level(&def.name)
            .map(|s| format!("{:?}", s))
            .unwrap_or_else(|| "Safe".to_string());
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
#[tauri::command]
pub async fn set_agent_tool_profile(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    profile: String,
) -> Result<(), String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;

    let tools_content = workspace.read_file(&agent::workspace::SoulFile::Tools)
        .unwrap_or_default();
    let (_old_profile, overrides) = agent::parse_tools_config(&tools_content);

    let new_content = agent::format_tools_config(&profile, &overrides);
    workspace.write_file(&agent::workspace::SoulFile::Tools, &new_content)?;

    log::info!("Agent {} 工具 Profile 已更新为: {}", agent_id, profile);
    Ok(())
}

/// 设置 Agent 的单个工具覆盖
#[tauri::command]
pub async fn set_agent_tool_override(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    tool_name: String,
    enabled: Option<bool>,
) -> Result<(), String> {
    let workspace = ensure_agent_workspace(state.orchestrator.pool(), &agent_id).await?;

    let tools_content = workspace.read_file(&agent::workspace::SoulFile::Tools)
        .unwrap_or_default();
    let (profile, mut overrides) = agent::parse_tools_config(&tools_content);

    match enabled {
        Some(value) => {
            overrides.insert(tool_name.clone(), value);
        }
        None => {
            overrides.remove(&tool_name);
        }
    }

    let new_content = agent::format_tools_config(&profile, &overrides);
    workspace.write_file(&agent::workspace::SoulFile::Tools, &new_content)?;

    log::info!("Agent {} 工具覆盖已更新: {}", agent_id, tool_name);
    Ok(())
}
