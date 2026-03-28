//! SOP（Standard Operating Procedure）系统
//!
//! 多步骤自动化工作流引擎。参考 ZeroClaw SOP 设计。
//!
//! 特性：
//! - 多种触发方式（cron/webhook/消息/手动）
//! - 多步骤流水线（上一步输出 → 下一步输入）
//! - 执行模式（auto/supervised/step_by_step/deterministic）
//! - checkpoint 暂停等人工审批
//! - SOP.toml 声明式定义

pub mod types;
pub mod engine;

pub use engine::SopEngine;
