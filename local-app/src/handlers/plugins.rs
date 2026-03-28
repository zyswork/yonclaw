//! 插件管理命令

use std::sync::Arc;
use tauri::State;

use crate::agent;
use crate::plugin_system;
use crate::AppState;

/// 列出已安装的插件
#[tauri::command]
pub async fn list_plugins(
    _state: State<'_, Arc<AppState>>,
) -> Result<Vec<serde_json::Value>, String> {
    let plugins_dir = dirs::home_dir()
        .ok_or("无法获取 home 目录")?
        .join(".xianzhu")
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

/// 查询 Plugin API 能力列表
#[tauri::command]
pub async fn list_plugin_capabilities(
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
pub async fn list_system_plugins(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<serde_json::Value>, String> {
    let registry = plugin_system::PluginRegistry::with_builtins();
    let mut plugins = registry.to_json();

    let rows: Vec<(String, i32)> = sqlx::query_as(
        "SELECT plugin_id, enabled FROM plugin_configs"
    ).fetch_all(state.orchestrator.pool()).await.unwrap_or_default();
    let db_state: std::collections::HashMap<String, bool> = rows.into_iter()
        .map(|(id, en)| (id, en == 1)).collect();

    let tg_token: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'telegram_bot_token'")
        .fetch_optional(state.orchestrator.pool()).await.ok().flatten();
    let feishu_id: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'feishu_app_id'")
        .fetch_optional(state.orchestrator.pool()).await.ok().flatten();

    for p in &mut plugins {
        if let Some(id) = p.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()) {
            if let Some(&enabled) = db_state.get(&id) {
                p["enabled"] = serde_json::Value::Bool(enabled);
            } else {
                p["enabled"] = p.get("defaultEnabled").cloned().unwrap_or(serde_json::Value::Bool(true));
            }

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
pub async fn toggle_system_plugin(
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
pub async fn save_plugin_config(
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

    if let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&config_json) {
        match plugin_id.as_str() {
            "telegram-channel" => {
                if let Some(token) = cfg["bot_token"].as_str() {
                    let _ = sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('telegram_bot_token', ?)")
                        .bind(token).execute(state.orchestrator.pool()).await;
                    log::info!("插件配置同步: telegram_bot_token -> settings");
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
                log::info!("插件配置同步: feishu_app_id/secret -> settings");
            }
            _ => {}
        }
    }

    Ok(())
}

/// 获取插件配置
#[tauri::command]
pub async fn get_plugin_config(
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
pub async fn get_agent_plugin_states(
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
pub async fn set_agent_plugin(
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

/// 导入外部插件（Claude/Cursor/Codex/MCP 格式）
#[tauri::command]
pub async fn import_external_plugin(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    path: String,
) -> Result<String, String> {
    let dir = std::path::PathBuf::from(&path);
    if !dir.exists() {
        return Err(format!("路径不存在: {}", path));
    }

    let bundle = plugin_system::bundle_compat::parse_bundle(&dir)?;
    log::info!("检测到 {:?} 插件: {} ({})", bundle.bundle_type, bundle.name, bundle.description);

    let result = plugin_system::bundle_compat::install_bundle(
        state.orchestrator.pool(), &agent_id, &bundle, &dir,
    ).await?;

    Ok(result)
}
