//! 定时任务类型定义

use serde::{Deserialize, Serialize};

/// 调度类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Schedule {
    /// cron 表达式（如 "0 9 * * *"）
    Cron { expr: String, tz: String },
    /// 固定间隔（秒）
    Every { secs: u64 },
    /// 一次性定时（unix 时间戳）
    At { ts: i64 },
    /// Webhook 触发（HTTP POST 到 /webhook/{token}）
    Webhook {
        /// 唯一 token（自动生成）
        token: String,
        /// 可选的 secret（用于验证签名）
        #[serde(default)]
        secret: Option<String>,
    },
    /// 轮询触发（定期检查 URL，内容变化时执行）
    Poll {
        /// 目标 URL
        url: String,
        /// 轮询间隔（秒）
        interval_secs: u64,
        /// JSON Path 提取（如 "$.data.status"），为空则比较完整 body
        #[serde(default)]
        json_path: Option<String>,
        /// 上次内容摘要（内部状态，用于变化检测）
        #[serde(default)]
        last_hash: Option<String>,
    },
}

/// 任务执行类型
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobType {
    Agent,
    Shell,
    McpTool,
}

impl std::fmt::Display for JobType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobType::Agent => write!(f, "agent"),
            JobType::Shell => write!(f, "shell"),
            JobType::McpTool => write!(f, "mcp_tool"),
        }
    }
}

impl std::str::FromStr for JobType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "agent" => Ok(JobType::Agent),
            "shell" => Ok(JobType::Shell),
            "mcp_tool" => Ok(JobType::McpTool),
            _ => Err(format!("未知任务类型: {}", s)),
        }
    }
}

/// 执行载荷
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionPayload {
    Agent {
        prompt: String,
        #[serde(default = "default_session_strategy")]
        session_strategy: String, // "new" | "reuse"
        /// 模型覆盖（可选，不填用 Agent 默认模型）
        #[serde(default)]
        model: Option<String>,
        /// 推理级别覆盖（可选）
        #[serde(default)]
        thinking: Option<String>,
    },
    Shell {
        command: String,
    },
    McpTool {
        server_name: String,
        tool_name: String,
        #[serde(default)]
        args: serde_json::Value,
    },
}

fn default_session_strategy() -> String {
    "new".to_string()
}

/// Guardrails 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guardrails {
    pub max_concurrent: u32,
    pub cooldown_secs: u32,
    pub max_daily_runs: Option<u32>,
    pub max_consecutive_failures: u32,
}

impl Default for Guardrails {
    fn default() -> Self {
        Self {
            max_concurrent: 1,
            cooldown_secs: 0,
            max_daily_runs: None,
            max_consecutive_failures: 5,
        }
    }
}

/// 重试配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub backoff_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 0,
            base_delay_ms: 2000,
            backoff_factor: 2.0,
        }
    }
}

/// 定时任务
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub agent_id: Option<String>,
    pub job_type: JobType,
    pub schedule: Schedule,
    pub action_payload: ActionPayload,
    pub timeout_secs: u32,
    pub guardrails: Guardrails,
    pub retry: RetryConfig,
    pub misfire_policy: String,
    pub catch_up_limit: u32,
    pub enabled: bool,
    pub fail_streak: u32,
    pub runs_today: u32,
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
    pub delete_after_run: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 运行记录
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronRun {
    pub id: String,
    pub job_id: String,
    pub scheduled_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub status: RunStatus,
    pub trigger_source: TriggerSource,
    pub attempt: u32,
    pub output: Option<String>,
    pub error: Option<String>,
}

/// 运行状态
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Success,
    Failed,
    Timeout,
    Cancelled,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunStatus::Queued => write!(f, "queued"),
            RunStatus::Running => write!(f, "running"),
            RunStatus::Success => write!(f, "success"),
            RunStatus::Failed => write!(f, "failed"),
            RunStatus::Timeout => write!(f, "timeout"),
            RunStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::str::FromStr for RunStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(RunStatus::Queued),
            "running" => Ok(RunStatus::Running),
            "success" => Ok(RunStatus::Success),
            "failed" => Ok(RunStatus::Failed),
            "timeout" => Ok(RunStatus::Timeout),
            "cancelled" => Ok(RunStatus::Cancelled),
            _ => Err(format!("未知状态: {}", s)),
        }
    }
}

/// 触发来源
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSource {
    Schedule,
    Manual,
    Retry,
    CatchUp,
    Heartbeat,
}

impl std::fmt::Display for TriggerSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TriggerSource::Schedule => write!(f, "schedule"),
            TriggerSource::Manual => write!(f, "manual"),
            TriggerSource::Retry => write!(f, "retry"),
            TriggerSource::CatchUp => write!(f, "catch_up"),
            TriggerSource::Heartbeat => write!(f, "heartbeat"),
        }
    }
}

impl std::str::FromStr for TriggerSource {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "schedule" => Ok(TriggerSource::Schedule),
            "manual" => Ok(TriggerSource::Manual),
            "retry" => Ok(TriggerSource::Retry),
            "catch_up" => Ok(TriggerSource::CatchUp),
            "heartbeat" => Ok(TriggerSource::Heartbeat),
            _ => Err(format!("未知触发来源: {}", s)),
        }
    }
}

/// 调度器状态（前端展示用）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerStatus {
    pub running: bool,
    pub total_jobs: u32,
    pub enabled_jobs: u32,
    pub running_runs: u32,
    pub recent_failure_rate: f64,
    pub last_tick_at: Option<i64>,
}

/// 心跳配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatConfig {
    pub enabled: bool,
    pub interval_secs: u64,
    pub quiet_hours_start: Option<u8>,
    pub quiet_hours_end: Option<u8>,
    pub timezone: String,
    pub suppress_ok: bool,
    pub max_failures: u32,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: 1800, // 30 分钟
            quiet_hours_start: None,
            quiet_hours_end: None,
            timezone: "Asia/Shanghai".to_string(),
            suppress_ok: true,
            max_failures: 3,
        }
    }
}

/// 健康报告
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub scheduler_alive: bool,
    pub stuck_runs: Vec<String>,
    pub high_fail_jobs: Vec<String>,
    pub auto_disabled_jobs: Vec<String>,
    pub recent_failure_rate: f64,
}

impl HealthReport {
    pub fn has_issues(&self) -> bool {
        !self.stuck_runs.is_empty()
            || !self.high_fail_jobs.is_empty()
            || !self.auto_disabled_jobs.is_empty()
            || self.recent_failure_rate > 0.5
    }
}

/// 输出截断常量
pub const MAX_OUTPUT_BYTES: usize = 16 * 1024;
pub const TRUNCATED_MARKER: &str = "\n...[truncated]";

/// Anti-spin 常量
pub const MIN_REFIRE_GAP_MS: u64 = 2000;
pub const MAX_TIMER_DELAY_SECS: u64 = 60;
pub const STUCK_RUN_THRESHOLD_SECS: i64 = 7200; // 2 小时
pub const RECOVERY_THRESHOLD_SECS: u64 = 90;

/// 截断输出
pub fn truncate_output(output: &str) -> (String, bool) {
    if output.len() <= MAX_OUTPUT_BYTES {
        (output.to_string(), false)
    } else {
        let boundary = MAX_OUTPUT_BYTES - TRUNCATED_MARKER.len();
        // 在 UTF-8 字符边界截断
        let mut end = boundary;
        while end > 0 && !output.is_char_boundary(end) {
            end -= 1;
        }
        (format!("{}{}", &output[..end], TRUNCATED_MARKER), true)
    }
}

/// 创建任务请求
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateJobRequest {
    pub name: String,
    pub agent_id: Option<String>,
    pub job_type: JobType,
    pub schedule: Schedule,
    pub action_payload: ActionPayload,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u32,
    #[serde(default)]
    pub guardrails: Guardrails,
    #[serde(default)]
    pub retry: RetryConfig,
    #[serde(default = "default_misfire_policy")]
    pub misfire_policy: String,
    #[serde(default = "default_catch_up_limit")]
    pub catch_up_limit: u32,
    #[serde(default)]
    pub delete_after_run: bool,
}

fn default_timeout() -> u32 { 300 }
fn default_misfire_policy() -> String { "catch_up".to_string() }
fn default_catch_up_limit() -> u32 { 3 }

/// 更新任务请求
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateJobRequest {
    pub name: Option<String>,
    pub schedule: Option<Schedule>,
    pub action_payload: Option<ActionPayload>,
    pub timeout_secs: Option<u32>,
    pub guardrails: Option<Guardrails>,
    pub retry: Option<RetryConfig>,
    pub misfire_policy: Option<String>,
    pub catch_up_limit: Option<u32>,
    pub enabled: Option<bool>,
}

/// 任务过滤器
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JobFilter {
    pub agent_id: Option<String>,
    pub enabled: Option<bool>,
    pub job_type: Option<JobType>,
}
