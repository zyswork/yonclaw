//! Agent 可调用的定时任务管理工具

use async_trait::async_trait;
use serde_json::json;

use crate::agent::tools::{Tool, ToolDefinition, ToolSafetyLevel};
use super::{store, planner, types::*};

/// cron_add 工具
pub struct CronAddTool {
    pool: sqlx::SqlitePool,
    notify: std::sync::Arc<tokio::sync::Notify>,
}

impl CronAddTool {
    pub fn new(pool: sqlx::SqlitePool, notify: std::sync::Arc<tokio::sync::Notify>) -> Self {
        Self { pool, notify }
    }
}

#[async_trait]
impl Tool for CronAddTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cron_add".to_string(),
            description: "创建定时任务。支持 cron 表达式、固定间隔、一次性定时。".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "任务名称" },
                    "job_type": { "type": "string", "enum": ["agent", "shell", "mcp_tool"] },
                    "schedule": { "type": "object", "description": "{kind:'cron',expr,tz?} | {kind:'every',secs} | {kind:'at',ts}" },
                    "action": { "type": "object", "description": "执行配置" },
                    "agent_id": { "type": "string", "description": "Agent ID（agent/mcp_tool 类型必填）" }
                },
                "required": ["name", "job_type", "schedule", "action"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Approval }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let name = args["name"].as_str().ok_or("缺少 name")?;
        let job_type: JobType = args["job_type"].as_str().ok_or("缺少 job_type")?.parse()?;
        let schedule: Schedule = serde_json::from_value(args["schedule"].clone())
            .map_err(|e| format!("schedule 格式错误: {}", e))?;
        let action: ActionPayload = serde_json::from_value(args["action"].clone())
            .map_err(|e| format!("action 格式错误: {}", e))?;
        let agent_id = args["agent_id"].as_str().map(|s| s.to_string());

        planner::validate_schedule(&schedule)?;

        let req = CreateJobRequest {
            name: name.to_string(),
            agent_id,
            job_type,
            schedule,
            action_payload: action,
            timeout_secs: args["timeout_secs"].as_u64().unwrap_or(300) as u32,
            guardrails: Guardrails::default(),
            retry: RetryConfig::default(),
            misfire_policy: "catch_up".to_string(),
            catch_up_limit: 3,
            delete_after_run: false,
        };

        let job = store::add_job(&self.pool, &req).await?;
        self.notify.notify_one();
        Ok(format!("已创建定时任务: {} (ID: {})", job.name, job.id))
    }
}

/// cron_list 工具
pub struct CronListTool { pool: sqlx::SqlitePool }

impl CronListTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl Tool for CronListTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cron_list".to_string(),
            description: "列出所有定时任务".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        }
    }

    async fn execute(&self, _args: serde_json::Value) -> Result<String, String> {
        let jobs = store::list_jobs(&self.pool, None).await?;
        if jobs.is_empty() {
            return Ok("当前没有定时任务".to_string());
        }
        let mut output = String::from("定时任务列表:\n");
        for job in &jobs {
            let status = if job.enabled { "启用" } else { "暂停" };
            let schedule_desc = match &job.schedule {
                Schedule::Cron { expr, .. } => format!("cron: {}", expr),
                Schedule::Every { secs } => format!("每 {}s", secs),
                Schedule::At { ts } => format!("定时: {}", ts),
                Schedule::Webhook { token, .. } => format!("webhook: {}", &token[..token.len().min(8)]),
                Schedule::Poll { url, interval_secs, .. } => format!("poll: {} (每 {}s)", url, interval_secs),
            };
            output.push_str(&format!(
                "\n[{}] {} [{}] {} (ID: {})",
                status, job.name, job.job_type, schedule_desc, job.id
            ));
        }
        Ok(output)
    }
}

/// cron_remove 工具
pub struct CronRemoveTool {
    pool: sqlx::SqlitePool,
    notify: std::sync::Arc<tokio::sync::Notify>,
}

impl CronRemoveTool {
    pub fn new(pool: sqlx::SqlitePool, notify: std::sync::Arc<tokio::sync::Notify>) -> Self {
        Self { pool, notify }
    }
}

#[async_trait]
impl Tool for CronRemoveTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cron_remove".to_string(),
            description: "删除定时任务".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "job_id": { "type": "string", "description": "任务 ID" }
                },
                "required": ["job_id"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Approval }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let job_id = args["job_id"].as_str().ok_or("缺少 job_id")?;
        store::delete_job(&self.pool, job_id).await?;
        self.notify.notify_one();
        Ok(format!("已删除任务: {}", job_id))
    }
}

/// cron_update 工具
pub struct CronUpdateTool {
    pool: sqlx::SqlitePool,
    notify: std::sync::Arc<tokio::sync::Notify>,
}

impl CronUpdateTool {
    pub fn new(pool: sqlx::SqlitePool, notify: std::sync::Arc<tokio::sync::Notify>) -> Self {
        Self { pool, notify }
    }
}

#[async_trait]
impl Tool for CronUpdateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cron_update".to_string(),
            description: "修改定时任务（暂停/恢复/改调度/改执行配置）".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "job_id": { "type": "string", "description": "任务 ID" },
                    "enabled": { "type": "boolean", "description": "启用/禁用" },
                    "schedule": { "type": "object", "description": "新调度配置" },
                    "action": { "type": "object", "description": "新执行配置" }
                },
                "required": ["job_id"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Approval }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let job_id = args["job_id"].as_str().ok_or("缺少 job_id")?;
        let patch = UpdateJobRequest {
            name: args["name"].as_str().map(|s| s.to_string()),
            enabled: args["enabled"].as_bool(),
            schedule: args.get("schedule").and_then(|v| serde_json::from_value(v.clone()).ok()),
            action_payload: args.get("action").and_then(|v| serde_json::from_value(v.clone()).ok()),
            timeout_secs: args["timeout_secs"].as_u64().map(|v| v as u32),
            guardrails: None,
            retry: None,
            misfire_policy: None,
            catch_up_limit: None,
        };
        let job = store::update_job(&self.pool, job_id, &patch).await?;
        self.notify.notify_one();
        Ok(format!("已更新任务: {} ({})", job.name, job.id))
    }
}

/// cron_trigger 工具
pub struct CronTriggerTool {
    pool: sqlx::SqlitePool,
    notify: std::sync::Arc<tokio::sync::Notify>,
}

impl CronTriggerTool {
    pub fn new(pool: sqlx::SqlitePool, notify: std::sync::Arc<tokio::sync::Notify>) -> Self {
        Self { pool, notify }
    }
}

#[async_trait]
impl Tool for CronTriggerTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cron_trigger".to_string(),
            description: "手动触发一次定时任务执行".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "job_id": { "type": "string", "description": "任务 ID" }
                },
                "required": ["job_id"]
            }),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let job_id = args["job_id"].as_str().ok_or("缺少 job_id")?;
        let job = store::get_job(&self.pool, job_id).await?;
        let now = chrono::Utc::now().timestamp();
        store::update_next_run(&self.pool, job_id, now, job.last_run_at.unwrap_or(0)).await?;
        self.notify.notify_one();
        Ok(format!("已触发任务: {}", job.name))
    }
}
