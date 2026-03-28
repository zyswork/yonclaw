//! SOP 类型定义

use serde::{Deserialize, Serialize};

/// SOP 优先级
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SopPriority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}

/// SOP 执行模式
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SopExecutionMode {
    /// 全自动执行，无需人工干预
    Auto,
    /// 启动前需审批，之后自动执行
    #[default]
    Supervised,
    /// 每步都需审批
    StepByStep,
    /// 确定性执行（无 LLM 调用，步骤间管道传递）
    Deterministic,
}

/// SOP 触发器
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SopTrigger {
    /// 手动触发
    Manual,
    /// Cron 定时触发
    Cron { expression: String },
    /// Webhook 触发
    Webhook { path: String },
    /// 消息匹配触发
    Message {
        #[serde(default)]
        channel: String,
        #[serde(default)]
        pattern: Option<String>,
    },
}

/// SOP 步骤类型
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SopStepKind {
    /// 正常执行步骤
    #[default]
    Execute,
    /// 检查点 — 暂停等人工审批
    Checkpoint,
}

/// SOP 单步定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SopStep {
    pub number: u32,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub suggested_tools: Vec<String>,
    #[serde(default)]
    pub kind: SopStepKind,
}

/// SOP 完整定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sop {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub priority: SopPriority,
    #[serde(default)]
    pub execution_mode: SopExecutionMode,
    pub triggers: Vec<SopTrigger>,
    pub steps: Vec<SopStep>,
    #[serde(default = "default_cooldown")]
    pub cooldown_secs: u64,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
}

fn default_cooldown() -> u64 { 0 }
fn default_max_concurrent() -> u32 { 1 }

/// SOP 运行状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SopRunStatus {
    Pending,
    Running,
    WaitingApproval,
    PausedCheckpoint,
    Completed,
    Failed,
    Cancelled,
}

/// 步骤执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SopStepResult {
    pub step_number: u32,
    pub status: String,
    pub output: String,
    pub started_at: String,
    pub completed_at: Option<String>,
}

/// SOP 运行实例
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SopRun {
    pub run_id: String,
    pub sop_name: String,
    pub status: SopRunStatus,
    pub current_step: u32,
    pub total_steps: u32,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub step_results: Vec<SopStepResult>,
}
