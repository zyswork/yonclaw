//! Tauri command handlers — 按功能域拆分
//!
//! 每个子模块包含一组相关的 `#[tauri::command]` 函数。
//! main.rs 通过 `generate_handler![]` 引用这些函数。

pub mod providers;
pub mod agents;
pub mod sessions;
pub mod channels_cmd;
pub mod plaza;
pub mod soul;
pub mod mcp;
pub mod skills;
pub mod plugins;
pub mod scheduler_cmd;
pub mod misc;
pub mod helpers;
