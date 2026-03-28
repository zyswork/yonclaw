//! Doctor 诊断工具
//!
//! 自检系统状态，发现问题并自动修复。
//! 参考 OpenClaw `openclaw doctor` 命令。
//!
//! 检查项：
//! 1. 数据库完整性（表结构、孤立数据）
//! 2. Provider 可用性（API Key 有效性）
//! 3. 渠道连接状态（Telegram/飞书/Discord/Slack/微信）
//! 4. Agent 工作区完整性
//! 5. 磁盘空间
//! 6. MCP Server 状态

use sqlx::SqlitePool;
use serde::Serialize;

/// 诊断结果
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticResult {
    pub category: String,
    pub check: String,
    pub status: DiagStatus,
    pub message: String,
    pub auto_fix: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum DiagStatus {
    Ok,
    Warning,
    Error,
    Fixed,
}

/// 运行全部诊断
pub async fn run_diagnostics(pool: &SqlitePool) -> Vec<DiagnosticResult> {
    let mut results = Vec::new();

    // 1. 数据库完整性
    results.extend(check_database(pool).await);

    // 2. Provider 可用性
    results.extend(check_providers(pool).await);

    // 3. Agent 工作区
    results.extend(check_agent_workspaces(pool).await);

    // 4. 磁盘空间
    results.extend(check_disk_space().await);

    // 5. MCP Server 状态
    results.extend(check_mcp_servers(pool).await);

    // 6. 渠道配置
    results.extend(check_channels(pool).await);

    results
}

/// 自动修复可修复的问题
pub async fn auto_fix(pool: &SqlitePool) -> Vec<DiagnosticResult> {
    let mut fixes = Vec::new();

    // 修复孤立 session（无 agent 关联）
    let orphaned = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM chat_sessions WHERE agent_id NOT IN (SELECT id FROM agents)"
    ).fetch_one(pool).await.unwrap_or(0);

    if orphaned > 0 {
        let _ = sqlx::query(
            "DELETE FROM chat_sessions WHERE agent_id NOT IN (SELECT id FROM agents)"
        ).execute(pool).await;
        fixes.push(DiagnosticResult {
            category: "数据库".into(),
            check: "孤立 Session".into(),
            status: DiagStatus::Fixed,
            message: format!("已清理 {} 个孤立 session", orphaned),
            auto_fix: Some("DELETE orphaned sessions".into()),
        });
    }

    // 修复孤立对话记录
    let orphaned_convs = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM conversations WHERE session_id NOT IN (SELECT id FROM chat_sessions)"
    ).fetch_one(pool).await.unwrap_or(0);

    if orphaned_convs > 0 {
        let _ = sqlx::query(
            "DELETE FROM conversations WHERE session_id NOT IN (SELECT id FROM chat_sessions)"
        ).execute(pool).await;
        fixes.push(DiagnosticResult {
            category: "数据库".into(),
            check: "孤立对话".into(),
            status: DiagStatus::Fixed,
            message: format!("已清理 {} 条孤立对话记录", orphaned_convs),
            auto_fix: Some("DELETE orphaned conversations".into()),
        });
    }

    // 修复缺失的 Agent 工作区
    let agents: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT id, workspace_path FROM agents"
    ).fetch_all(pool).await.unwrap_or_default();

    for (agent_id, workspace_path) in &agents {
        if let Some(wp) = workspace_path {
            let path = std::path::Path::new(wp);
            if !path.exists() {
                let _ = std::fs::create_dir_all(path);
                fixes.push(DiagnosticResult {
                    category: "工作区".into(),
                    check: format!("Agent {} 工作区", &agent_id[..8]),
                    status: DiagStatus::Fixed,
                    message: format!("已重建工作区: {}", wp),
                    auto_fix: Some("mkdir -p workspace".into()),
                });
            }
        }
    }

    // 清理过期的审计日志（>30天）
    let cutoff = chrono::Utc::now().timestamp_millis() - (30 * 86_400_000);
    let old_logs = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM tool_audit_log WHERE timestamp < ?"
    ).bind(cutoff).fetch_one(pool).await.unwrap_or(0);

    if old_logs > 100 {
        let _ = sqlx::query("DELETE FROM tool_audit_log WHERE timestamp < ?")
            .bind(cutoff).execute(pool).await;
        fixes.push(DiagnosticResult {
            category: "数据库".into(),
            check: "审计日志".into(),
            status: DiagStatus::Fixed,
            message: format!("已清理 {} 条过期审计日志（>30天）", old_logs),
            auto_fix: Some("DELETE old audit logs".into()),
        });
    }

    fixes
}

// ─── 具体检查函数 ──────────────────────────────────

async fn check_database(pool: &SqlitePool) -> Vec<DiagnosticResult> {
    let mut results = Vec::new();

    // 检查表是否存在
    let tables = ["agents", "chat_sessions", "conversations", "chat_messages",
        "memories", "cron_jobs", "settings", "mcp_servers", "token_usage"];

    for table in &tables {
        let exists: bool = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?"
        ).bind(table).fetch_one(pool).await.unwrap_or(0) > 0;

        if !exists {
            results.push(DiagnosticResult {
                category: "数据库".into(),
                check: format!("表 {}", table),
                status: DiagStatus::Error,
                message: format!("表 {} 不存在", table),
                auto_fix: None,
            });
        }
    }

    // SQLite integrity check
    let integrity: Option<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_optional(pool).await.ok().flatten();
    let ok = integrity.as_deref() == Some("ok");
    results.push(DiagnosticResult {
        category: "数据库".into(),
        check: "完整性检查".into(),
        status: if ok { DiagStatus::Ok } else { DiagStatus::Error },
        message: if ok { "SQLite 完整性正常".into() } else {
            format!("完整性异常: {}", integrity.unwrap_or_default())
        },
        auto_fix: None,
    });

    // 数据库大小
    let db_path = dirs::data_dir()
        .unwrap_or_default()
        .join("com.xianzhu.app/xianzhu.db");
    if db_path.exists() {
        let size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
        let size_mb = size as f64 / 1_048_576.0;
        let status = if size_mb > 500.0 { DiagStatus::Warning } else { DiagStatus::Ok };
        results.push(DiagnosticResult {
            category: "数据库".into(),
            check: "数据库大小".into(),
            status,
            message: format!("{:.1} MB", size_mb),
            auto_fix: if size_mb > 500.0 { Some("VACUUM".into()) } else { None },
        });
    }

    // 检查孤立数据
    let orphaned_sessions = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM chat_sessions WHERE agent_id NOT IN (SELECT id FROM agents)"
    ).fetch_one(pool).await.unwrap_or(0);
    if orphaned_sessions > 0 {
        results.push(DiagnosticResult {
            category: "数据库".into(),
            check: "孤立 Session".into(),
            status: DiagStatus::Warning,
            message: format!("{} 个 session 无关联 Agent（可自动修复）", orphaned_sessions),
            auto_fix: Some("auto_fix".into()),
        });
    } else {
        results.push(DiagnosticResult {
            category: "数据库".into(),
            check: "数据引用完整性".into(),
            status: DiagStatus::Ok,
            message: "无孤立数据".into(),
            auto_fix: None,
        });
    }

    results
}

async fn check_providers(pool: &SqlitePool) -> Vec<DiagnosticResult> {
    let mut results = Vec::new();

    let json_str: Option<String> = sqlx::query_scalar(
        "SELECT value FROM settings WHERE key = 'providers'"
    ).fetch_optional(pool).await.ok().flatten();

    let providers: Vec<serde_json::Value> = json_str
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    if providers.is_empty() {
        results.push(DiagnosticResult {
            category: "Provider".into(),
            check: "LLM 供应商".into(),
            status: DiagStatus::Error,
            message: "未配置任何 LLM 供应商。请在设置中添加。".into(),
            auto_fix: None,
        });
        return results;
    }

    let mut has_enabled = false;
    for p in &providers {
        let name = p["name"].as_str().unwrap_or("未知");
        let enabled = p["enabled"].as_bool().unwrap_or(false);
        let has_key = p["apiKey"].as_str().map(|k| !k.is_empty()).unwrap_or(false);

        if enabled && has_key {
            has_enabled = true;
            results.push(DiagnosticResult {
                category: "Provider".into(),
                check: format!("{}", name),
                status: DiagStatus::Ok,
                message: "已启用，API Key 已配置".into(),
                auto_fix: None,
            });
        } else if enabled && !has_key {
            results.push(DiagnosticResult {
                category: "Provider".into(),
                check: format!("{}", name),
                status: DiagStatus::Error,
                message: "已启用但缺少 API Key".into(),
                auto_fix: None,
            });
        }
    }

    if !has_enabled {
        results.push(DiagnosticResult {
            category: "Provider".into(),
            check: "可用供应商".into(),
            status: DiagStatus::Error,
            message: "没有可用的 LLM 供应商（需要至少一个启用且有 API Key）".into(),
            auto_fix: None,
        });
    }

    results
}

async fn check_agent_workspaces(pool: &SqlitePool) -> Vec<DiagnosticResult> {
    let mut results = Vec::new();

    let agents: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT id, name, workspace_path FROM agents"
    ).fetch_all(pool).await.unwrap_or_default();

    if agents.is_empty() {
        results.push(DiagnosticResult {
            category: "Agent".into(),
            check: "Agent 列表".into(),
            status: DiagStatus::Warning,
            message: "无 Agent，请创建一个".into(),
            auto_fix: None,
        });
        return results;
    }

    for (_id, name, workspace_path) in &agents {
        if let Some(wp) = workspace_path {
            let exists = std::path::Path::new(wp).exists();
            results.push(DiagnosticResult {
                category: "Agent".into(),
                check: format!("{} 工作区", name),
                status: if exists { DiagStatus::Ok } else { DiagStatus::Warning },
                message: if exists { format!("{}", wp) } else { format!("工作区不存在: {}（可自动修复）", wp) },
                auto_fix: if exists { None } else { Some("auto_fix".into()) },
            });
        }
    }

    results
}

async fn check_disk_space() -> Vec<DiagnosticResult> {
    let mut results = Vec::new();

    // 检查 home 目录可用空间
    if let Some(home) = dirs::home_dir() {
        #[cfg(unix)]
        {
            let output = std::process::Command::new("df")
                .arg("-k")
                .arg(home.to_string_lossy().as_ref())
                .output();

            if let Ok(out) = output {
                let text = String::from_utf8_lossy(&out.stdout);
                if let Some(line) = text.lines().nth(1) {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 4 {
                        if let Ok(avail_kb) = parts[3].parse::<u64>() {
                            let avail_gb = avail_kb as f64 / 1_048_576.0;
                            let status = if avail_gb < 1.0 { DiagStatus::Error }
                                else if avail_gb < 5.0 { DiagStatus::Warning }
                                else { DiagStatus::Ok };
                            results.push(DiagnosticResult {
                                category: "系统".into(),
                                check: "磁盘空间".into(),
                                status,
                                message: format!("{:.1} GB 可用", avail_gb),
                                auto_fix: None,
                            });
                        }
                    }
                }
            }
        }
    }

    results
}

async fn check_mcp_servers(pool: &SqlitePool) -> Vec<DiagnosticResult> {
    let mut results = Vec::new();

    let servers: Vec<(String, String, bool, String)> = sqlx::query_as(
        "SELECT id, name, enabled, status FROM mcp_servers"
    ).fetch_all(pool).await.unwrap_or_default();

    if servers.is_empty() {
        results.push(DiagnosticResult {
            category: "MCP".into(),
            check: "MCP Server".into(),
            status: DiagStatus::Ok,
            message: "未配置 MCP Server（可选功能）".into(),
            auto_fix: None,
        });
        return results;
    }

    for (_, name, enabled, status) in &servers {
        let diag_status = match status.as_str() {
            "connected" => DiagStatus::Ok,
            "configured" => DiagStatus::Warning,
            "failed" => DiagStatus::Error,
            _ => DiagStatus::Warning,
        };
        results.push(DiagnosticResult {
            category: "MCP".into(),
            check: format!("{}", name),
            status: diag_status,
            message: format!("状态: {} | {}", status, if *enabled { "启用" } else { "禁用" }),
            auto_fix: None,
        });
    }

    results
}

async fn check_channels(pool: &SqlitePool) -> Vec<DiagnosticResult> {
    let mut results = Vec::new();

    let channel_keys = [
        ("Telegram", "telegram_bot_token"),
        ("飞书", "feishu_app_id"),
        ("Discord", "discord_bot_token"),
        ("Slack", "slack_bot_token"),
        ("微信", "weixin_token"),
    ];

    for (name, key) in &channel_keys {
        let value: Option<String> = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = ?"
        ).bind(key).fetch_optional(pool).await.ok().flatten();

        let configured = value.map(|v| !v.is_empty()).unwrap_or(false);
        results.push(DiagnosticResult {
            category: "渠道".into(),
            check: name.to_string(),
            status: if configured { DiagStatus::Ok } else { DiagStatus::Ok }, // 渠道是可选的
            message: if configured { "已配置".into() } else { "未配置（可选）".into() },
            auto_fix: None,
        });
    }

    results
}
