//! Agent 编排引擎
//!
//! ## 模块结构
//!
//! ### 核心管道（一条消息的处理路径）
//! ```text
//! orchestrator.send_message_stream()
//!   → soul.rs (构建 system prompt)
//!   → context_guard.rs (预算强制)
//!   → agent_loop.rs (多轮工具循环)
//!     → llm.rs (LLM 调用)
//!     → dispatcher.rs (工具结果格式化)
//! ```
//!
//! ### 工具系统
//! - `tools/` — Tool trait + 13 个内置工具 + ToolManager
//! - `skills.rs` — 技能扫描/激活/管理
//! - `skill_tool.rs` — 单个技能的 Tool 适配器
//! - `mcp.rs` + `mcp_manager.rs` — MCP 协议工具
//! - `sandbox.rs` — Shell 命令安全校验
//! - `tool_policy.rs` — 工具访问策略
//!
//! ### 数据/存储
//! - `agent_store.rs` — Agent CRUD + 缓存 + 成本限额
//! - `workspace.rs` — Agent 工作区文件系统
//!
//! ### 辅助
//! - `autonomy.rs` — 自治等级评估
//! - `failover.rs` — 多模型 failover
//! - `router.rs` — 智能模型路由
//! - `response_cache.rs` — LLM 响应缓存
//! - `observer.rs` — 事件广播（前端订阅）
//! - `hooks.rs` — LLM/工具调用钩子
//! - `rate_limiter.rs` — 请求频率控制
//! - `multimodal.rs` — 图片/vision 支持
//! - `token_counter.rs` — tiktoken 计数

// ── 核心管道 ──
pub mod agent_loop;
pub mod agent_store;
pub mod context_guard;
pub mod dispatcher;
pub mod llm;
pub mod orchestrator;
pub mod soul;
pub mod self_evolution;
pub mod delegate;
pub mod token_counter;
pub mod execution_budget;
pub mod file_harness;
pub mod auto_verify;
pub mod progress;
pub mod intent_gate;

// ── 工具系统 ──
pub mod tools;
pub mod skill_tool;
pub mod skills;
pub mod mcp;
pub mod mcp_manager;
pub mod sandbox;
pub mod tool_policy;

// ── 数据/存储 ──
pub mod workspace;

// ── 辅助 ──
pub mod autonomy;
pub mod lifecycle;
pub mod media;
pub mod failover;
pub mod hooks;
pub mod multimodal;
pub mod observer;
pub mod rate_limiter;
pub mod response_cache;
pub mod router;

// ── 扩展 ──
pub mod approval;
pub mod browser;
pub mod cdp;
pub mod content_security;
pub mod doctor;
pub mod plugin;
pub mod relations;
pub mod subagent;

// ── 公开接口 ──
pub use failover::FailoverExecutor;
pub use llm::{LlmClient, LlmConfig};
pub use orchestrator::Orchestrator;
pub use skills::SkillManager;
pub use tools::{parse_tools_config, is_tool_enabled, format_tools_config};
pub use workspace::AgentWorkspace;
pub use relations::{RelationManager, RelationType};
