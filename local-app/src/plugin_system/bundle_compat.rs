//! 跨生态插件兼容
//!
//! 支持解析 Claude / Cursor / Codex 生态的插件格式，
//! 映射为 XianZhu 的 Skill 或 MCP Server。
//!
//! 格式检测：
//! - .claude-plugin/marketplace.json → Claude 插件
//! - .cursor-plugin/plugin.json → Cursor 插件
//! - .codex-plugin/plugin.json → Codex 插件
//! - 含 mcp.json / mcp-servers.json → MCP 配置

use std::path::Path;

/// 检测到的外部插件类型
#[derive(Debug, Clone)]
pub enum BundleType {
    Claude,
    Cursor,
    Codex,
    Mcp,
    Unknown,
}

/// 解析后的外部插件信息
#[derive(Debug, Clone)]
pub struct BundleInfo {
    pub bundle_type: BundleType,
    pub name: String,
    pub description: String,
    pub version: String,
    /// MCP server 配置（如果有）
    pub mcp_servers: Vec<McpServerConfig>,
    /// 工具定义（如果有）
    pub tools: Vec<String>,
}

/// MCP Server 配置
#[derive(Debug, Clone, serde::Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

/// 检测目录中的外部插件类型
pub fn detect_bundle_type(dir: &Path) -> BundleType {
    if dir.join(".claude-plugin/marketplace.json").exists()
        || dir.join(".claude/marketplace.json").exists() {
        return BundleType::Claude;
    }
    if dir.join(".cursor-plugin/plugin.json").exists() {
        return BundleType::Cursor;
    }
    if dir.join(".codex-plugin/plugin.json").exists() {
        return BundleType::Codex;
    }
    if dir.join("mcp.json").exists() || dir.join("mcp-servers.json").exists() {
        return BundleType::Mcp;
    }
    BundleType::Unknown
}

/// 解析外部插件目录
pub fn parse_bundle(dir: &Path) -> Result<BundleInfo, String> {
    let bundle_type = detect_bundle_type(dir);

    match bundle_type {
        BundleType::Claude => parse_claude_bundle(dir),
        BundleType::Cursor => parse_cursor_bundle(dir),
        BundleType::Codex => parse_codex_bundle(dir),
        BundleType::Mcp => parse_mcp_bundle(dir),
        BundleType::Unknown => Err("未识别的插件格式".to_string()),
    }
}

/// 解析 Claude 插件
fn parse_claude_bundle(dir: &Path) -> Result<BundleInfo, String> {
    let manifest_path = if dir.join(".claude-plugin/marketplace.json").exists() {
        dir.join(".claude-plugin/marketplace.json")
    } else {
        dir.join(".claude/marketplace.json")
    };

    let content = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读取 Claude manifest 失败: {}", e))?;
    let manifest: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("解析 JSON 失败: {}", e))?;

    let name = manifest["name"].as_str().unwrap_or("claude-plugin").to_string();
    let description = manifest["description"].as_str().unwrap_or("").to_string();
    let version = manifest["version"].as_str().unwrap_or("1.0.0").to_string();

    // 提取 MCP server 配置
    let mcp_servers = extract_mcp_from_manifest(&manifest, dir);

    Ok(BundleInfo {
        bundle_type: BundleType::Claude,
        name, description, version, mcp_servers,
        tools: Vec::new(),
    })
}

/// 解析 Cursor 插件
fn parse_cursor_bundle(dir: &Path) -> Result<BundleInfo, String> {
    let manifest_path = dir.join(".cursor-plugin/plugin.json");
    let content = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读取 Cursor manifest 失败: {}", e))?;
    let manifest: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("解析 JSON 失败: {}", e))?;

    let name = manifest["name"].as_str().unwrap_or("cursor-plugin").to_string();
    let description = manifest["description"].as_str().unwrap_or("").to_string();
    let version = manifest["version"].as_str().unwrap_or("1.0.0").to_string();

    let mcp_servers = extract_mcp_from_manifest(&manifest, dir);

    Ok(BundleInfo {
        bundle_type: BundleType::Cursor,
        name, description, version, mcp_servers,
        tools: Vec::new(),
    })
}

/// 解析 Codex 插件
fn parse_codex_bundle(dir: &Path) -> Result<BundleInfo, String> {
    let manifest_path = dir.join(".codex-plugin/plugin.json");
    let content = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读取 Codex manifest 失败: {}", e))?;
    let manifest: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("解析 JSON 失败: {}", e))?;

    let name = manifest["name"].as_str().unwrap_or("codex-plugin").to_string();
    let description = manifest["description"].as_str().unwrap_or("").to_string();
    let version = manifest["version"].as_str().unwrap_or("1.0.0").to_string();

    Ok(BundleInfo {
        bundle_type: BundleType::Codex,
        name, description, version,
        mcp_servers: Vec::new(),
        tools: Vec::new(),
    })
}

/// 解析纯 MCP 配置
fn parse_mcp_bundle(dir: &Path) -> Result<BundleInfo, String> {
    let mcp_path = if dir.join("mcp.json").exists() {
        dir.join("mcp.json")
    } else {
        dir.join("mcp-servers.json")
    };

    let content = std::fs::read_to_string(&mcp_path)
        .map_err(|e| format!("读取 MCP 配置失败: {}", e))?;
    let config: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("解析 JSON 失败: {}", e))?;

    let mut servers = Vec::new();
    if let Some(obj) = config["mcpServers"].as_object().or(config.as_object()) {
        for (name, cfg) in obj {
            if let Some(cmd) = cfg["command"].as_str() {
                let args: Vec<String> = cfg["args"].as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                servers.push(McpServerConfig {
                    name: name.clone(),
                    command: cmd.to_string(),
                    args,
                    env: Default::default(),
                });
            }
        }
    }

    let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("mcp-bundle");
    Ok(BundleInfo {
        bundle_type: BundleType::Mcp,
        name: dir_name.to_string(),
        description: format!("{} 个 MCP Server", servers.len()),
        version: "1.0.0".to_string(),
        mcp_servers: servers,
        tools: Vec::new(),
    })
}

/// 从 manifest 中提取 MCP server 配置
fn extract_mcp_from_manifest(manifest: &serde_json::Value, _dir: &Path) -> Vec<McpServerConfig> {
    let mut servers = Vec::new();

    // 检查 mcpServers 字段
    if let Some(mcp) = manifest.get("mcpServers").and_then(|v| v.as_object()) {
        for (name, cfg) in mcp {
            if let Some(cmd) = cfg["command"].as_str() {
                let args: Vec<String> = cfg["args"].as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                servers.push(McpServerConfig {
                    name: name.clone(),
                    command: cmd.to_string(),
                    args,
                    env: Default::default(),
                });
            }
        }
    }

    servers
}

/// 将外部插件安装到 XianZhu（转换为 Skill 或 MCP Server）
pub async fn install_bundle(
    pool: &sqlx::SqlitePool,
    agent_id: &str,
    bundle: &BundleInfo,
    _source_dir: &Path,
) -> Result<String, String> {
    let mut results = Vec::new();

    // 安装 MCP Servers
    for server in &bundle.mcp_servers {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let args_json = serde_json::to_string(&server.args).unwrap_or_default();

        let _ = sqlx::query(
            "INSERT INTO mcp_servers (id, agent_id, name, transport, command, args, enabled, status, created_at) VALUES (?, ?, ?, 'stdio', ?, ?, 1, 'configured', ?)"
        )
        .bind(&id).bind(agent_id).bind(&server.name)
        .bind(&server.command).bind(&args_json).bind(now)
        .execute(pool).await;

        results.push(format!("MCP Server: {}", server.name));
        log::info!("已导入 MCP Server: {} (from {:?} bundle)", server.name, bundle.bundle_type);
    }

    if results.is_empty() {
        Ok(format!("已解析 {:?} 插件 '{}'，但没有可导入的组件", bundle.bundle_type, bundle.name))
    } else {
        Ok(format!("已从 {:?} 插件 '{}' 导入: {}", bundle.bundle_type, bundle.name, results.join(", ")))
    }
}
