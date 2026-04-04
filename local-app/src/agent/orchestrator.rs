//! Agent 编排引擎
//!
//! 支持 LLM 调用、多轮工具调用、Agent 管理、对话历史

use super::llm::{LlmClient, LlmConfig, ThinkingLevel};
use super::mcp_manager::McpManager;
use super::skill_tool::SkillTool;
use super::media::MediaProvider; // 导入 trait 使 describe_image 可用
use super::skills::SkillManager;
use super::soul::{SoulEngine, SectionBudget};
use super::tools::{ToolManager, CalculatorTool, DateTimeTool, FileReadTool, FileWriteTool, FileListTool, FileEditTool, DiffEditTool, FileRollbackTool, BashExecTool, CodeSearchTool, WebFetchTool, WebSearchTool, ImageGenerateTool, TtsTool, SttTool, DocParseTool, DocWriteTool, ClipboardTool, ScreenshotTool, ApplyPatchTool, HttpRequestTool, SessionTool, FocusTool, ResearchTool, CollaborateTool, YieldTool, A2aTool, MemoryReadTool, MemoryWriteTool, SettingsTool, ProviderTool, AgentSelfConfigTool, SkillManageTool, CronManageTool, PluginManageTool, BrowserTool, Tool};
use super::tools::{parse_tools_config, is_tool_enabled};
use super::workspace::AgentWorkspace;
use super::subagent::SubagentRegistry;
use super::tool_policy::ToolPolicyEngine;

/// 估算文本的 token 数（中文 1 字 ≈ 1.5 token，英文 1 词 ≈ 1.3 token）
/// 公开版本供其他模块使用
pub fn estimate_tokens_pub(text: &str) -> usize { estimate_tokens(text) }

fn estimate_tokens(text: &str) -> usize {
    let mut tokens = 0usize;
    let mut in_ascii = false;
    let mut ascii_len = 0usize;
    for c in text.chars() {
        if c.is_ascii() {
            if !in_ascii { in_ascii = true; ascii_len = 0; }
            ascii_len += 1;
            if c == ' ' || c == '\n' || c == '\t' {
                tokens += (ascii_len as f64 / 4.0).ceil() as usize;
                ascii_len = 0;
            }
        } else {
            if in_ascii {
                tokens += (ascii_len as f64 / 4.0).ceil() as usize;
                ascii_len = 0;
                in_ascii = false;
            }
            // 中文/日文/韩文：每字约 1.5 token
            tokens += if c >= '\u{4E00}' && c <= '\u{9FFF}' { 2 } // CJK
                else if c >= '\u{3040}' && c <= '\u{30FF}' { 2 } // 假名
                else { 1 };
        }
    }
    if in_ascii { tokens += (ascii_len as f64 / 4.0).ceil() as usize; }
    tokens.max(1)
}

/// 格式化 token 数量（如 1234 → "1.2K"，123456 → "123.5K"）
fn format_token_count(tokens: usize) -> String {
    if tokens >= 1_000_000 { format!("{:.1}M", tokens as f64 / 1_000_000.0) }
    else if tokens >= 1_000 { format!("{:.1}K", tokens as f64 / 1_000.0) }
    else { format!("{}", tokens) }
}

/// 为后台任务（经验提取、压缩、cron）选择模型
/// 优先级：全局 background_model 设置 > 自动推断轻量模型 > agent 自身模型
/// 为后台任务构建完整的 LlmConfig（含正确的 provider/api_key/base_url）
/// 如果 background_model 来自不同 provider，会自动查找对应 provider 配置
pub async fn build_compact_llm_config(agent_config: &super::llm::LlmConfig, pool: &sqlx::SqlitePool) -> super::llm::LlmConfig {
    let compact_model = pick_compact_model(&agent_config.model);
    let (_, pure_model) = crate::channels::parse_qualified_model(&compact_model);

    // 如果 compact_model 带 provider 前缀（如 "moonshot/kimi-k2.5"），查找该 provider 的配置
    if compact_model.contains('/') {
        if let Ok(Some(providers_json)) = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = 'providers'"
        ).fetch_optional(pool).await {
            if let Ok(providers) = serde_json::from_str::<Vec<serde_json::Value>>(&providers_json) {
                for p in &providers {
                    if let Some(models) = p["models"].as_array() {
                        let has_model = models.iter().any(|m| {
                            let mid = m["id"].as_str().unwrap_or("");
                            mid.eq_ignore_ascii_case(pure_model) || mid.eq_ignore_ascii_case(&compact_model)
                        });
                        if has_model {
                            if let (Some(api_key), Some(base_url)) = (p["apiKey"].as_str(), p["baseUrl"].as_str()) {
                                let pid = p["id"].as_str().unwrap_or("");
                                let api_type = p["apiType"].as_str().unwrap_or("openai");
                                log::info!("compact LLM: 使用 provider={} model={}", pid, pure_model);
                                return super::llm::LlmConfig {
                                    provider: api_type.to_string(),
                                    api_key: api_key.to_string(),
                                    model: format!("{}/{}", pid, pure_model),
                                    base_url: Some(base_url.to_string()),
                                    temperature: Some(0.3),
                                    max_tokens: Some(1024),
                                    thinking_level: None,
                                };
                            }
                        }
                    }
                }
            }
        }
    }

    // Fallback：用 agent 自己的 provider + compact model
    super::llm::LlmConfig {
        provider: agent_config.provider.clone(),
        api_key: agent_config.api_key.clone(),
        model: compact_model,
        base_url: agent_config.base_url.clone(),
        temperature: Some(0.3),
        max_tokens: Some(1024),
        thinking_level: None,
    }
}

pub fn pick_compact_model(agent_model: &str) -> String {
    // 1. 检查全局 background_model 设置（由用户在设置界面配置）
    if let Some(pool) = crate::telemetry::get_global_pool() {
        if let Ok(Some(bg_model)) = futures::executor::block_on(
            sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'background_model'")
                .fetch_optional(pool)
        ) {
            if !bg_model.is_empty() {
                return bg_model;
            }
        }
    }

    // 2. 如果 agent 已经是轻量模型，直接用
    let m = agent_model.to_lowercase();
    if m.contains("mini") || m.contains("haiku") || m.contains("flash") || m.contains("turbo") {
        return agent_model.to_string();
    }

    // 3. 自动推断同系列的轻量模型（用户未配置 background_model 时的兜底）
    if m.contains("gpt") || m.contains("openai") { return "gpt-4o-mini".to_string(); }
    if m.contains("claude") { return "claude-haiku-4-5-20251001".to_string(); }
    if m.contains("deepseek") { return "deepseek-chat".to_string(); }
    if m.contains("qwen") { return "qwen-turbo".to_string(); }
    if m.contains("gemini") { return "gemini-3-flash-preview".to_string(); }
    if m.contains("grok") { return "grok-3-mini".to_string(); }
    if m.contains("kimi") { return "kimi-k2.5".to_string(); }
    if m.contains("moonshot") { return "moonshot-v1-8k".to_string(); }
    if m.contains("glm") { return "glm-4.7-flash".to_string(); }
    if m.contains("minimax") { return "MiniMax-M2.5".to_string(); }
    // 默认用 agent 自己的模型
    agent_model.to_string()
}
use crate::memory;
use crate::memory::loader::MemoryLoader;
use crate::memory::SqliteMemory;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::mpsc;

// Agent struct 已移到 agent_store.rs
pub use super::agent_store::Agent;

/// 多轮工具调用最大轮数
const MAX_TOOL_ROUNDS: usize = 10;

/// 单次请求默认 token 预算上限（防止失控）
const DEFAULT_TOKEN_BUDGET: u64 = 100_000;

/// Agent loop 错误，携带已生成的部分回复
#[derive(Debug)]
struct AgentLoopError {
    message: String,
    partial_content: String,
}

// estimate_cost 已移到 agent_store.rs

/// 会话消息 LRU 缓存最大条目数
const SESSION_CACHE_MAX: usize = 20;

/// Agent 编排器
pub struct Orchestrator {
    pool: SqlitePool,
    tool_manager: ToolManager,
    mcp_manager: McpManager,
    /// Agent CRUD + 缓存（从 orchestrator 提取的独立模块）
    agent_store: super::agent_store::AgentStore,
    /// 工具策略引擎
    policy_engine: std::sync::Mutex<ToolPolicyEngine>,
    /// SkillManager 缓存
    skill_cache: std::sync::Mutex<HashMap<PathBuf, (SkillManager, std::time::SystemTime)>>,
    /// 子 Agent 注册表
    subagent_registry: SubagentRegistry,
    /// 会话级别并发锁：同一 session 串行执行，防止并发 LLM 调用
    session_locks: std::sync::Mutex<HashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>>,
    /// 会话消息 LRU 缓存
    session_msg_cache: std::sync::Mutex<lru::LruCache<String, (Vec<serde_json::Value>, std::time::Instant)>>,
    /// 速率限制器
    rate_limiter: super::rate_limiter::RateLimiter,
    /// 事件广播器
    pub event_broadcaster: std::sync::Arc<super::observer::EventBroadcaster>,
    /// 工具钩子运行器
    hook_runner: std::sync::Mutex<super::hooks::HookRunner>,
    /// 生命周期事件管理器（替代 hooks/observer）
    lifecycle: super::lifecycle::LifecycleManager,
    /// 插件注册表（旧版 manifest，已废弃，保留向后兼容）
    #[allow(dead_code)]
    plugin_registry_legacy: crate::plugin_system::PluginRegistry,
    /// 插件管理器（新版 Plugin API）
    pub plugin_manager: std::sync::Mutex<crate::plugin_system::PluginManager>,
    /// 模型提供商注册表
    provider_registry: crate::plugin_system::ProviderRegistry,
    /// 自我进化状态
    pub evolution_state: std::sync::Arc<super::self_evolution::EvolutionState>,
    /// 自我进化配置
    evolution_config: super::self_evolution::EvolutionConfig,
    /// 工具审批管理器
    pub approval_manager: super::approval::ApprovalManager,
    /// 活跃会话的取消令牌：session_id → CancellationToken
    active_cancellations: std::sync::Mutex<HashMap<String, tokio_util::sync::CancellationToken>>,
}

/// RAII 守卫：函数返回时自动从 active_cancellations 中移除会话的取消令牌
struct CancelGuard<'a> {
    cancellations: &'a std::sync::Mutex<HashMap<String, tokio_util::sync::CancellationToken>>,
    session_id: String,
}

impl<'a> Drop for CancelGuard<'a> {
    fn drop(&mut self) {
        let mut map = self.cancellations.lock().unwrap_or_else(|p| p.into_inner());
        map.remove(&self.session_id);
    }
}

impl Orchestrator {
    /// 创建编排器并注册默认内置工具
    pub fn new(pool: SqlitePool) -> Self {
        let mut tool_manager = ToolManager::new();
        tool_manager.register_tool(Box::new(CalculatorTool));
        tool_manager.register_tool(Box::new(DateTimeTool));
        tool_manager.register_tool(Box::new(FileReadTool));
        tool_manager.register_tool(Box::new(FileWriteTool));
        tool_manager.register_tool(Box::new(FileListTool));
        tool_manager.register_tool(Box::new(BashExecTool));
        tool_manager.register_tool(Box::new(FileEditTool));
        tool_manager.register_tool(Box::new(DiffEditTool));
        tool_manager.register_tool(Box::new(FileRollbackTool));
        tool_manager.register_tool(Box::new(CodeSearchTool));
        tool_manager.register_tool(Box::new(WebFetchTool));
        tool_manager.register_tool(Box::new(WebSearchTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(ImageGenerateTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(TtsTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(MemoryReadTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(MemoryWriteTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(SettingsTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(ProviderTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(AgentSelfConfigTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(SkillManageTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(CronManageTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(PluginManageTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(BrowserTool));
        tool_manager.register_tool(Box::new(SttTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(DocParseTool));
        tool_manager.register_tool(Box::new(DocWriteTool));
        tool_manager.register_tool(Box::new(ClipboardTool));
        tool_manager.register_tool(Box::new(ScreenshotTool));
        tool_manager.register_tool(Box::new(ApplyPatchTool));
        tool_manager.register_tool(Box::new(HttpRequestTool));
        tool_manager.register_tool(Box::new(SessionTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(FocusTool));
        tool_manager.register_tool(Box::new(ResearchTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(CollaborateTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(YieldTool));
        tool_manager.register_tool(Box::new(A2aTool::new(pool.clone())));
        let event_broadcaster = std::sync::Arc::new(super::observer::EventBroadcaster::default());
        tool_manager.register_tool(Box::new(super::delegate::DelegateTaskTool::new(pool.clone(), event_broadcaster.clone())));
        let mcp_manager = McpManager::new(pool.clone());
        // 初始化钩子运行器（注册默认日志钩子）
        let mut hook_runner = super::hooks::HookRunner::new();
        hook_runner.register(Box::new(super::hooks::LoggingHook));

        let subagent_registry = SubagentRegistry::with_pool(pool.clone());
        Self {
            agent_store: super::agent_store::AgentStore::new(pool.clone()),
            pool, tool_manager, mcp_manager,
            policy_engine: std::sync::Mutex::new(ToolPolicyEngine::new()),
            skill_cache: std::sync::Mutex::new(HashMap::new()),
            subagent_registry,
            session_locks: std::sync::Mutex::new(HashMap::new()),
            session_msg_cache: std::sync::Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(SESSION_CACHE_MAX).unwrap()
            )),
            rate_limiter: super::rate_limiter::RateLimiter::new(super::rate_limiter::RateLimitConfig::default()),
            event_broadcaster: event_broadcaster,
            hook_runner: std::sync::Mutex::new(hook_runner),
            lifecycle: {
                let mut lm = super::lifecycle::LifecycleManager::new();
                lm.register(Box::new(super::lifecycle::LoggingHandler));
                lm.register(Box::new(super::lifecycle::TokenTrackingHandler));
                lm
            },
            plugin_registry_legacy: crate::plugin_system::PluginRegistry::with_builtins(),
            plugin_manager: std::sync::Mutex::new(crate::plugin_system::PluginManager::new()),
            provider_registry: crate::plugin_system::create_default_registry(),
            evolution_state: std::sync::Arc::new(super::self_evolution::EvolutionState::new()),
            evolution_config: super::self_evolution::EvolutionConfig::default(),
            approval_manager: super::approval::ApprovalManager::new(),
            active_cancellations: std::sync::Mutex::new(HashMap::new()),
        }
    }

    /// 获取数据库连接池引用
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// 取消指定会话的活跃生成
    ///
    /// 查找 session_id 对应的 CancellationToken 并触发取消
    pub fn cancel_session(&self, session_id: &str) -> bool {
        let tokens = self.active_cancellations.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(token) = tokens.get(session_id) {
            token.cancel();
            log::info!("已取消会话 {} 的活跃生成", session_id);
            true
        } else {
            log::debug!("会话 {} 没有活跃的生成任务", session_id);
            false
        }
    }

    /// 取消所有活跃会话的生成（用于应用退出清理）
    pub fn cancel_all_sessions(&self) {
        let mut tokens = self.active_cancellations.lock().unwrap_or_else(|p| p.into_inner());
        for (session_id, token) in tokens.drain() {
            token.cancel();
            log::info!("已取消活跃会话: {}", session_id);
        }
    }

    /// 获取子 Agent 注册表引用
    pub fn subagent_registry(&self) -> &SubagentRegistry {
        &self.subagent_registry
    }

    /// 派生并执行子 Agent
    ///
    /// 创建子 Agent 记录，在后台执行任务，完成后更新状态
    pub async fn spawn_subagent(
        &self,
        parent_id: &str,
        config: super::subagent::SpawnConfig,
        api_key: &str,
        api_type: &str,
        base_url: Option<&str>,
    ) -> Result<String, String> {
        let sub_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();

        // 获取父 Agent 信息
        let parent = {
            let agents = self.list_agents().await?;
            agents.into_iter().find(|a| a.id == parent_id)
                .ok_or("父 Agent 不存在")?
        };

        // 创建子 Agent 记录
        let record = super::subagent::SubagentRecord {
            id: sub_id.clone(),
            parent_id: parent_id.to_string(),
            name: config.name.clone(),
            task: config.task.clone(),
            status: super::subagent::SubagentStatus::Running,
            result: None,
            created_at: now,
            finished_at: None,
            timeout_secs: config.timeout_secs.unwrap_or(300),
        };
        self.subagent_registry.register(record).await;

        // 创建临时 Agent 用于子任务
        let model = config.model.unwrap_or(parent.model.clone());
        let system_prompt = format!(
            "{}\n\n---\n你是由 Agent「{}」派生的子 Agent。你的任务是：\n{}",
            parent.system_prompt, parent.name, config.task
        );

        // 注册临时 Agent 到数据库
        let sub_agent = self.register_agent(&config.name, &system_prompt, &model).await?;
        let _sub_agent_id = sub_agent.id.clone();

        // 创建会话
        let _session = memory::conversation::create_session(&self.pool, &_sub_agent_id, &config.name)
            .await.map_err(|e| format!("创建子 Agent 会话失败: {}", e))?;

        // 在后台执行子任务
        let _timeout_secs = config.timeout_secs.unwrap_or(300);
        let _api_key = api_key.to_string();
        let _api_type = api_type.to_string();
        let _base_url = base_url.map(|s| s.to_string());

        // 注意：后台执行尚未实现，标记为 Failed 并记录日志
        log::warn!("子 Agent {} 后台执行尚未实现，标记为 Failed", sub_id);
        self.subagent_registry.update_status(
            &sub_id,
            super::subagent::SubagentStatus::Failed("后台执行尚未实现".to_string()),
            None,
        ).await;

        Ok(sub_id)
    }

    /// 获取工具管理器的可变引用（用于注册额外工具）
    pub fn tool_manager_mut(&mut self) -> &mut ToolManager {
        &mut self.tool_manager
    }

    /// 获取工具管理器的只读引用
    pub fn tool_manager(&self) -> &ToolManager {
        &self.tool_manager
    }

    /// 获取 MCP Manager 的引用
    pub fn mcp_manager(&self) -> &McpManager {
        &self.mcp_manager
    }

    /// 清除指定 agent 的元数据缓存
    pub fn invalidate_agent_cache(&self, agent_id: &str) {
        self.agent_store.invalidate_cache(agent_id);
    }

    /// 获取缓存的 SkillManager（基于技能文件最大 mtime 失效）
    fn get_skill_manager(&self, skills_dir: &std::path::Path) -> SkillManager {
        // 计算 skills 目录下所有 .md 文件的最大 mtime
        let max_mtime = Self::get_skills_max_mtime(skills_dir);

        if let Some(mtime) = max_mtime {
            if let Ok(cache) = self.skill_cache.lock() {
                if let Some((cached_mgr, cached_mtime)) = cache.get(skills_dir) {
                    if *cached_mtime == mtime {
                        return cached_mgr.clone();
                    }
                }
            }
        }

        // 缓存未命中，重新扫描
        let mgr = SkillManager::scan(skills_dir);

        if let Some(mtime) = max_mtime {
            if let Ok(mut cache) = self.skill_cache.lock() {
                cache.insert(skills_dir.to_path_buf(), (mgr.clone(), mtime));
            }
        }

        mgr
    }

    /// 清除技能缓存（安装/卸载技能后调用，让下次对话立即感知变化）
    pub fn invalidate_skill_cache(&self) {
        if let Ok(mut cache) = self.skill_cache.lock() {
            cache.clear();
            log::info!("技能缓存已清除");
        }
    }

    /// 计算 skills 目录下所有 .md 文件的最大 mtime
    fn get_skills_max_mtime(skills_dir: &std::path::Path) -> Option<std::time::SystemTime> {
        // 先取目录本身的 mtime（捕获文件新增/删除）
        let dir_mtime = std::fs::metadata(skills_dir).and_then(|m| m.modified()).ok();

        // 再遍历所有 .md 文件取最大 mtime（捕获文件内容修改）
        let file_max_mtime = std::fs::read_dir(skills_dir).ok().and_then(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "md"))
                .filter_map(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
                .max()
        });

        // 取两者中较大的
        match (dir_mtime, file_max_mtime) {
            (Some(d), Some(f)) => Some(d.max(f)),
            (Some(d), None) => Some(d),
            (None, Some(f)) => Some(f),
            (None, None) => None,
        }
    }

    /// 获取缓存的 Agent 元数据（委托给 AgentStore）
    pub async fn get_agent_cached(&self, agent_id: &str) -> Result<crate::db::models::Agent, String> {
        self.agent_store.get_cached(agent_id).await
    }

    /// 注册新 Agent（委托给 AgentStore）
    pub async fn register_agent(&self, name: &str, system_prompt: &str, model: &str) -> Result<Agent, String> {
        self.agent_store.register(name, system_prompt, model).await
    }

    /// 列出所有 Agent（委托给 AgentStore）
    pub async fn list_agents(&self) -> Result<Vec<Agent>, String> {
        self.agent_store.list().await
    }

    /// 删除 Agent（委托给 AgentStore）
    pub async fn delete_agent(&self, agent_id: &str) -> Result<(), String> {
        self.agent_store.delete(agent_id).await
    }

    /// 发送消息（流式），支持多轮工具调用
    ///
    /// Pipeline：
    /// 1. 获取 Agent 信息
    /// 2. SoulEngine 构建 system prompt
    /// 3. MemoryLoader 注入相关记忆
    /// 4. 解析工具定义（内置 + MCP）并注入 system prompt
    /// 5. 构建消息列表
    /// 6. ContextManager 分级上下文压缩
    /// 7. run_agent_loop 多轮工具调用
    /// 8. 保存对话
    pub async fn send_message_stream(
        &self,
        agent_id: &str,
        session_id: &str,
        user_message: &str,
        api_key: &str,
        provider: &str,
        base_url: Option<&str>,
        tx: mpsc::UnboundedSender<String>,
        cancel_token: Option<tokio_util::sync::CancellationToken>,
    ) -> Result<String, String> {
        // 0. 并发控制：同一 session 串行执行
        let session_lock = {
            let mut locks = self.session_locks.lock().unwrap_or_else(|p| p.into_inner());
            locks.entry(session_id.to_string())
                .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
                .clone()
        };
        let _guard = session_lock.lock().await;

        // 0-cancel. 创建或使用传入的取消令牌，并注册到 active_cancellations
        let cancel_token = {
            let token = cancel_token.unwrap_or_else(tokio_util::sync::CancellationToken::new);
            let mut cancellations = self.active_cancellations.lock().unwrap_or_else(|p| p.into_inner());
            cancellations.insert(session_id.to_string(), token.clone());
            Some(token)
        };
        // 使用 CancelGuard 确保函数返回时自动清理取消令牌
        let _cancel_guard = CancelGuard {
            cancellations: &self.active_cancellations,
            session_id: session_id.to_string(),
        };

        // 0a. 速率限制
        if let Err(wait_ms) = self.rate_limiter.check(agent_id) {
            return Err(format!("请求过于频繁，请等待 {} 毫秒后重试。", wait_ms));
        }

        // 0b. 每日 Token 限额检查
        {
            let daily_limit = self.get_daily_token_limit(agent_id).await;
            if daily_limit > 0 {
                let today_usage = self.get_today_token_usage(agent_id).await;
                if today_usage >= daily_limit {
                    return Err(format!(
                        "今日 Token 消耗已达上限（{}/{}），请明天再试或调整限额。",
                        today_usage, daily_limit
                    ));
                }
            }
        }

        // 1. 获取 agent 信息（每次重新读取，确保模型切换等配置立即生效）
        self.agent_store.invalidate_cache(agent_id);
        let agent = self.get_agent_cached(agent_id).await?;

        // 2. 构建 system prompt（从 Soul 文件组装）
        // 迁移旧的 .openclaw 路径到 .xianzhu
        let workspace_path = agent.workspace_path.as_ref().map(|wp| {
            if wp.contains("/.openclaw/") {
                let new_wp = wp.replace("/.openclaw/", "/.xianzhu/");
                log::info!("Orchestrator 迁移工作区路径: {} -> {}", wp, new_wp);
                let old_path = std::path::PathBuf::from(wp);
                let new_path = std::path::PathBuf::from(&new_wp);
                if old_path.exists() && !new_path.exists() {
                    if let Some(parent) = new_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::rename(&old_path, &new_path);
                }
                // 异步更新数据库
                let pool = self.pool.clone();
                let aid = agent_id.to_string();
                let nwp = new_wp.clone();
                tokio::spawn(async move {
                    let _ = sqlx::query("UPDATE agents SET workspace_path = ? WHERE id = ?")
                        .bind(&nwp).bind(&aid).execute(&pool).await;
                });
                new_wp
            } else {
                wp.clone()
            }
        });
        log::info!("Agent workspace_path: {:?}", workspace_path);
        let mut system_prompt = if let Some(ref wp) = workspace_path {
            let workspace = AgentWorkspace::from_path(std::path::PathBuf::from(wp), agent_id);
            log::info!("Workspace root: {:?}, exists: {}", workspace.root(), workspace.exists());
            if workspace.exists() {
                let mut engine = SoulEngine::with_defaults();

                // 从 Agent config 加载自定义 PromptSection
                if let Some(ref config_str) = agent.config {
                    if let Ok(config) = serde_json::from_str::<serde_json::Value>(config_str) {
                        if let Some(sections) = config.get("customSections").and_then(|s| s.as_array()) {
                            for s in sections {
                                let name = s["name"].as_str().unwrap_or("custom");
                                let file = s["file"].as_str().unwrap_or("");
                                let content = s["content"].as_str().unwrap_or("");
                                if !file.is_empty() {
                                    engine.add_section(Box::new(super::soul::DynamicSection::new(name, file)));
                                } else if !content.is_empty() {
                                    engine.add_section(Box::new(super::soul::InlineSection::new(name, content.to_string())));
                                }
                            }
                        }
                    }
                }

                // 动态记忆预算：根据用户意图调整 memory section 大小
                let mut budget = SectionBudget::default();
                let intent_preview = super::intent_gate::classify(user_message);
                let memory_budget = if intent_preview.intents.contains(&super::intent_gate::Intent::Question) {
                    500  // 简单问题不需要太多记忆
                } else if intent_preview.intents.contains(&super::intent_gate::Intent::Research) {
                    2_000 // 调研需要项目知识
                } else if intent_preview.intents.contains(&super::intent_gate::Intent::Dangerous) {
                    1_000 // 危险操作以安全规则为主
                } else {
                    3_000 // 代码修改需要全部上下文
                };
                budget.limits.insert("memory".into(), memory_budget);
                let mut prompt = engine.build_system_prompt_with_budget(&workspace, &budget);
                // 注入工作区环境信息
                prompt.push_str(&format!(
                    "\n\n---\n\n# Environment\n\n- Workspace: {}\n- Skills: {}/skills\n- Memory: {}/memory\n- Agent ID: {}",
                    wp, wp, wp, agent_id
                ));
                // 注入可协作 Agent 列表（Agent 发现机制）
                let peers = super::relations::RelationManager::get_relations(&self.pool, agent_id).await.unwrap_or_default();
                if !peers.is_empty() {
                    let mut peer_lines = Vec::new();
                    for r in &peers {
                        let peer_id = if r.from_id == agent_id { &r.to_id } else { &r.from_id };
                        let peer_info: Option<(String, String)> = sqlx::query_as(
                            "SELECT name, model FROM agents WHERE id = ?"
                        ).bind(peer_id).fetch_optional(&self.pool).await.ok().flatten();
                        if let Some((name, model)) = peer_info {
                            let direction = if r.from_id == agent_id { "→" } else { "←" };
                            peer_lines.push(format!("- {} **{}** ({}, {}) `{}`", direction, name, r.relation_type, model, peer_id));
                        }
                    }
                    if !peer_lines.is_empty() {
                        prompt.push_str(&format!(
                            "\n\n---\n\n# Collaborators\n\n你可以通过 `collaborate` 或 `agent_chat` 或 `delegate_task` 工具与以下 Agent 协作：\n\n{}\n\n使用 agent_id 指定目标。",
                            peer_lines.join("\n")
                        ));
                    }
                }

                log::info!("SoulEngine 构建的 system_prompt 长度: {} 字节, 前200字符: {}", prompt.len(), prompt.chars().take(200).collect::<String>());
                prompt
            } else {
                log::warn!("Workspace 不存在，使用 agent.system_prompt (长度: {})", agent.system_prompt.len());
                agent.system_prompt.clone()
            }
        } else {
            log::warn!("Agent 没有 workspace_path，使用 agent.system_prompt (长度: {})", agent.system_prompt.len());
            agent.system_prompt.clone()
        };
        // Hook: BeforePromptBuild — 允许插件注入额外上下文
        {
            let hook_event = super::lifecycle::HookEvent {
                point: "before_prompt_build".to_string(),
                agent_id: agent_id.to_string(),
                session_id: session_id.to_string(),
                payload: serde_json::json!({ "prompt_length": system_prompt.len() }),
            };
            if let Ok(Some(extra)) = self.lifecycle.emit(super::lifecycle::HookPoint::BeforePromptBuild, &hook_event).await {
                if let Some(append) = extra.get("append_context").and_then(|v| v.as_str()) {
                    system_prompt.push_str("\n\n---\n\n");
                    system_prompt.push_str(append);
                    log::info!("BeforePromptBuild hook 注入了 {} 字节上下文", append.len());
                }
            }
        }
        log::info!("最终 system_prompt 长度: {} 字节", system_prompt.len());

        // 3. MemoryLoader 注入记忆（三层存储：Hot LRU → Warm SQLite → Cold 归档）
        // 使用 with_ids 版本以追踪注入了哪些记忆，用于后续反馈循环
        let mut injected_memories: Vec<(String, String)> = Vec::new();
        {
            let sqlite_mem = if let Some(emb_config) = SqliteMemory::try_load_embedding_config(&self.pool).await {
                SqliteMemory::with_embedding(self.pool.clone(), emb_config).await
            } else {
                SqliteMemory::new(self.pool.clone())
            };
            // TieredMemory 包装：Hot 内存缓存 + Warm SQLite
            let cold_dir = workspace_path.as_ref().map(|wp| std::path::PathBuf::from(wp).join("memory"));
            let tiered_mem = memory::TieredMemory::new(sqlite_mem, cold_dir);
            let loader = MemoryLoader::new(&tiered_mem).with_top_k(5).with_threshold(0.3);
            if let Ok(Some((memory_text, ids))) = loader.load_relevant_memories_with_ids(agent_id, user_message).await {
                system_prompt = format!("{}\n\n---\n\n{}", system_prompt, memory_text);
                injected_memories = ids;
                log::info!("记忆注入: {} 条记忆已注入 system prompt", injected_memories.len());
            }
        }

        // 4. 解析工具定义（内置 + MCP），并注入 system prompt
        let all_tool_defs = self.tool_manager.get_tool_definitions();
        let filtered_tool_defs = if let Some(ref wp) = workspace_path {
            let workspace = AgentWorkspace::from_path(std::path::PathBuf::from(wp), agent_id);
            let tools_content = workspace.read("TOOLS.md").unwrap_or_default();
            if tools_content.trim().is_empty() {
                all_tool_defs
            } else {
                let (profile, overrides) = parse_tools_config(&tools_content);
                all_tool_defs.into_iter()
                    .filter(|def| is_tool_enabled(&def.name, &profile, &overrides))
                    .collect()
            }
        } else {
            all_tool_defs
        };

        let mut final_tool_defs = filtered_tool_defs;
        if let Err(e) = self.mcp_manager.start_servers_for_agent(agent_id).await {
            log::warn!("启动 MCP Server 失败: {}", e);
        }
        let mcp_defs = self.mcp_manager.get_tool_definitions().await;
        if !mcp_defs.is_empty() {
            // 安全: 拒绝与内置工具同名的 MCP 工具（防止工具名碰撞攻击）
            let builtin_names: std::collections::HashSet<String> = final_tool_defs.iter()
                .map(|d| d.name.clone())
                .collect();
            let mut injected = 0;
            for def in mcp_defs {
                if builtin_names.contains(&def.name) {
                    log::warn!("安全: MCP 工具 '{}' 与内置工具同名，已拒绝注册", def.name);
                    continue;
                }
                final_tool_defs.push(def);
                injected += 1;
            }
            if injected > 0 {
                log::info!("注入 {} 个 MCP 工具", injected);
            }
        }

        // 4b. IntentGate：根据用户意图过滤工具集
        let intent = super::intent_gate::classify(user_message);
        log::info!("IntentGate: {:?} (confidence={:.1}, filter={:?})", intent.intents, intent.confidence, intent.tool_filter);
        final_tool_defs = super::intent_gate::filter_tools(final_tool_defs, &intent.tool_filter);

        // 4b-2. 意图感知温度：当用户未显式设置温度时，根据意图推断
        // 优先级：用户显式设置 > 意图推断 > 模型默认值（llm::default_temperature）
        let intent_temperature: Option<f64> = if agent.temperature.is_none() {
            if intent.intents.contains(&super::intent_gate::Intent::CodeChange)
                || intent.intents.contains(&super::intent_gate::Intent::Dangerous) {
                log::info!("意图感知温度: 0.3 (意图={:?}, 用户未显式设置)", intent.intents);
                Some(0.3)  // 代码变更/危险操作：精确保守
            } else if intent.intents.contains(&super::intent_gate::Intent::Question)
                || intent.intents.contains(&super::intent_gate::Intent::Research) {
                log::info!("意图感知温度: 0.7 (意图={:?}, 用户未显式设置)", intent.intents);
                Some(0.7)  // 问答/调研：均衡
            } else {
                None  // 无明确意图，由 llm::default_temperature 兜底
            }
        } else {
            None  // 用户已显式设置，不覆盖
        };

        // 4c. 技能激活：根据用户消息匹配技能，注册技能工具
        let mut skill_tools: HashMap<String, Box<dyn Tool>> = HashMap::new();
        // 检查 Node.js 运行时，为技能工具注入 PATH
        let node_runtime = crate::runtime::NodeRuntime::new();
        let node_bin_dir = if node_runtime.is_installed().await {
            Some(node_runtime.bin_dir())
        } else {
            None
        };
        if let Some(ref wp) = workspace_path {
            let skills_dir = std::path::PathBuf::from(wp).join("skills");
            let skill_mgr = self.get_skill_manager(&skills_dir);
            let active = skill_mgr.activate_for_message(user_message);
            for manifest in &active {
                for tool_decl in &manifest.tools {
                    let mut st = SkillTool::new(
                        tool_decl.clone(),
                        &manifest.name,
                        skills_dir.join(&manifest.name),
                        &manifest.permissions,
                    );
                    // 注入 Node.js 运行时 PATH
                    if let Some(ref bin_dir) = node_bin_dir {
                        st.inject_node_path(bin_dir);
                    }
                    let def = st.definition();
                    final_tool_defs.push(def);
                    skill_tools.insert(st.full_name(), Box::new(st));
                }
            }
            if !active.is_empty() {
                log::info!("激活 {} 个技能，注入 {} 个技能工具", active.len(), skill_tools.len());
            }

            // 4c. Prompt-only 技能：关键词匹配但无工具声明的技能，注入内容到 system prompt
            let prompt_skills = skill_mgr.activate_prompt_skills(user_message);
            if !prompt_skills.is_empty() {
                let mut skill_prompt = String::new();
                for (name, body) in &prompt_skills {
                    skill_prompt.push_str(&format!("\n\n## Active Skill: {}\n\n{}", name, body));
                }
                // 追加到 system_prompt
                system_prompt.push_str(&skill_prompt);
                log::info!("注入 {} 个 prompt-only 技能到 system prompt", prompt_skills.len());
            }
        }

        // 工具定义通过 API tools 参数传递，不再重复注入 system prompt
        // 仅记录可用工具数量供调试
        if !final_tool_defs.is_empty() {
            log::info!("可用工具: {} 个（通过 API tools 参数传递）", final_tool_defs.len());
        }

        // 5. 构建消息列表 — 轮次感知加载（参照 OpenClaw limitHistoryTurns）
        //
        // 核心改进：按「用户轮次」而非「消息条数」加载历史。
        // 1 轮 = 1 条 user 消息 + 后续所有 assistant/tool 消息。
        // 避免 tool_call 噪声稀释新请求（250 条 session 中 tool 消息占 80%+）。
        const MAX_HISTORY_TURNS: usize = 10; // 保留最近 10 轮用户对话

        // 获取压缩边界和会话摘要
        let compact_boundary: i64 = {
            let key = format!("compact_boundary_{}", session_id);
            sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
                .bind(&key)
                .fetch_optional(&self.pool).await.ok().flatten()
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0)
        };

        let session_summary = if let Ok(Some(session)) = memory::conversation::get_session(&self.pool, session_id).await {
            session.summary.filter(|s| !s.is_empty())
        } else {
            None
        };

        let mut messages: Vec<serde_json::Value> = Vec::new();
        log::info!("Provider: '{}', 添加 system message: {}", provider, provider == "openai");
        if provider == "openai" {
            log::info!("注入 system message, 长度: {} 字节", system_prompt.len());
            messages.push(serde_json::json!({"role": "system", "content": &system_prompt}));
        }

        // 参照 OpenClaw：摘要作为 assistant 消息注入对话流（不是系统提示）
        // 这样 LLM 能看到"之前聊了什么 → 现在的消息"的自然衔接
        if let Some(ref summary) = session_summary {
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": format!("[对话摘要] 以下是之前对话的要点：\n\n{}\n\n---\n请基于以上背景继续对话。", summary)
            }));
            log::info!("compact: 摘要注入为 assistant 消息（{}字符），boundary_seq={}", summary.len(), compact_boundary);
        }

        if compact_boundary > 0 {
            // 有压缩边界：加载边界之后的结构化消息（保留 tool_calls 结构）
            let recent_msgs = memory::conversation::load_chat_messages_after_boundary(
                &self.pool, session_id, compact_boundary, 200
            ).await.unwrap_or_default();

            log::info!("compact mode: 加载 boundary_seq={} 之后的 {} 条结构化消息", compact_boundary, recent_msgs.len());
            messages.extend(recent_msgs);
        } else {
            // 无压缩：按轮次加载（而非固定 100 条）
            let structured_history = {
                let cached = {
                    match self.session_msg_cache.lock() {
                        Ok(mut cache) => {
                            cache.get(session_id).and_then(|(msgs, time)| {
                                if time.elapsed() < std::time::Duration::from_secs(10) {
                                    Some(msgs.clone())
                                } else {
                                    None
                                }
                            })
                        }
                        Err(_) => None,
                    }
                };
                if let Some(msgs) = cached {
                    log::debug!("会话消息缓存命中: session_id={}", session_id);
                    msgs
                } else {
                    // 按轮次加载：保留最近 N 轮完整对话
                    let msgs = memory::conversation::load_chat_messages_by_turns(
                        &self.pool, session_id, MAX_HISTORY_TURNS
                    ).await.unwrap_or_default();
                    // 写入缓存
                    if let Ok(mut cache) = self.session_msg_cache.lock() {
                        cache.put(session_id.to_string(), (msgs.clone(), std::time::Instant::now()));
                    }
                    msgs
                }
            };

            if !structured_history.is_empty() {
                log::info!("按轮次加载 {} 条结构化历史消息（最近{}轮）", structured_history.len(), MAX_HISTORY_TURNS);
                messages.extend(structured_history);
            } else {
                // Fallback: 旧的纯文本历史
                let history = memory::conversation::get_history(&self.pool, agent_id, session_id, 20).await.unwrap_or_default();
                for (user_msg, agent_resp) in history.into_iter().rev() {
                    messages.push(serde_json::json!({"role": "user", "content": user_msg}));
                    if !agent_resp.is_empty() {
                        messages.push(serde_json::json!({"role": "assistant", "content": agent_resp}));
                    }
                }
            }
        }
        // 5.1 Focus Directive — 长对话中引导 LLM 聚焦当前请求
        //
        // 当历史消息超过 6 条时，在用户消息前注入一个轻量提示，
        // 明确告知 LLM "以下是用户最新请求，请聚焦回答"。
        // 这是一个 system-level hint（以 user 消息注入，避免 Anthropic 不支持多 system）。
        {
            // 统计历史中的 user 消息数（不含即将添加的当前消息）
            let history_user_count = messages.iter()
                .filter(|m| m["role"].as_str() == Some("user"))
                .count();
            if history_user_count >= 3 {
                messages.push(serde_json::json!({
                    "role": "system",
                    "content": "[Focus] 以上是历史对话背景。请聚焦回答用户接下来的最新消息，不要被之前的话题带偏。"
                }));
                // 对于不支持 system 角色在中间的 provider，改为 user 角色
                if provider != "openai" {
                    if let Some(last) = messages.last_mut() {
                        last["role"] = serde_json::json!("user");
                    }
                }
            }
        }

        // 媒体理解：图片处理（提取 → 保存到磁盘 → 传给 LLM）
        let image_urls = super::multimodal::extract_image_urls(user_message);
        // 把 base64 图片保存到磁盘，DB 存路径引用
        let saved_paths = if !image_urls.is_empty() {
            super::multimodal::save_images_to_disk(&image_urls, agent_id)
        } else {
            Vec::new()
        };
        if !image_urls.is_empty() {
            if super::multimodal::supports_vision(&agent.model) {
                // 模型原生支持 vision — 直接传图片 URL
                log::info!("检测到 {} 张图片，转为 vision 格式", image_urls.len());
                messages.push(super::multimodal::to_vision_message("user", user_message, &image_urls));
            } else {
                // 模型不支持 vision — 尝试用 MediaProvider 描述图片后注入文本
                let mut descriptions = Vec::new();
                let describer = super::media::VisionDescriber::new(api_key, &agent.model);
                for url in &image_urls {
                    match describer.describe_image(url, None).await {
                        Ok(desc) => {
                            log::info!("图片描述成功: {} 字符", desc.len());
                            descriptions.push(format!("[图片描述: {}]", desc));
                        }
                        Err(e) => log::warn!("图片描述失败: {}", e),
                    }
                }
                let augmented = if descriptions.is_empty() {
                    user_message.to_string()
                } else {
                    format!("{}\n\n{}", user_message, descriptions.join("\n"))
                };
                messages.push(serde_json::json!({"role": "user", "content": augmented}));
            }
        } else {
            messages.push(serde_json::json!({"role": "user", "content": user_message}));
        }

        // 5a. 自动压缩 — 双触发：token 预算 80% 或消息数 > 40
        //
        // 改进点：
        // 1. 移除 compact_boundary==0 限制，允许重复压缩
        // 2. 增加消息数触发（>40条），不必等到 token 预算紧张
        // 3. 压缩后摘要统一注入为 assistant 消息（不是 system prompt）
        {
            let sys_tokens = super::token_counter::TokenCounter::count(&system_prompt);
            let pre_guard_config = super::context_guard::ContextGuardConfig::for_model(&agent.model)
                .with_system_prompt_tokens(sys_tokens);
            let budget = pre_guard_config.total_budget();
            let current_tokens = super::token_counter::TokenCounter::count_messages(&messages);
            let usage_percent = if budget > 0 { (current_tokens as f64 / budget as f64) * 100.0 } else { 0.0 };
            let msg_count = messages.len();

            // 双触发条件：token 超 80% 或 消息超 40 条（排除 system 消息）
            let need_compact = usage_percent > 80.0 || msg_count > 40;

            if need_compact {
                log::info!(
                    "自动压缩触发: 上下文 {:.1}% ({}/{} tokens), 消息 {} 条, 已有边界={}",
                    usage_percent, current_tokens, budget, msg_count, compact_boundary
                );
                // 通知用户正在压缩（参照 OpenClaw compaction.notifyUser）
                let _ = tx.send("\n⚙️ 对话历史较长，正在智能压缩...\n".to_string());
                match self.compact_session(agent_id, session_id, api_key, provider, base_url).await {
                    Ok(result) => {
                        let _ = tx.send("✅ 压缩完成，继续对话。\n".to_string());
                        log::info!("自动压缩完成: {}", result.chars().take(200).collect::<String>());
                        // 压缩后重新加载消息
                        let new_boundary: i64 = {
                            let key = format!("compact_boundary_{}", session_id);
                            sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
                                .bind(&key)
                                .fetch_optional(&self.pool).await.ok().flatten()
                                .and_then(|v| v.parse::<i64>().ok())
                                .unwrap_or(0)
                        };
                        if new_boundary > 0 {
                            // 重建消息列表（摘要作为 assistant 消息，统一路径）
                            messages.clear();
                            if provider == "openai" {
                                messages.push(serde_json::json!({"role": "system", "content": &system_prompt}));
                            }
                            // 注入新摘要为 assistant 消息
                            if let Ok(Some(session)) = memory::conversation::get_session(&self.pool, session_id).await {
                                if let Some(ref summary) = session.summary {
                                    if !summary.is_empty() {
                                        messages.push(serde_json::json!({
                                            "role": "assistant",
                                            "content": format!("[对话摘要] 以下是之前对话的要点：\n\n{}\n\n---\n请基于以上背景继续对话。", summary)
                                        }));
                                    }
                                }
                            }
                            // 加载结构化消息（保留 tool_calls）
                            let recent_msgs = memory::conversation::load_chat_messages_after_boundary(
                                &self.pool, session_id, new_boundary, 200
                            ).await.unwrap_or_default();
                            messages.extend(recent_msgs);

                            // 确保当前用户消息在末尾
                            let has_current_user_msg = messages.last()
                                .and_then(|m| m["content"].as_str())
                                .map(|c| c == user_message)
                                .unwrap_or(false);
                            if !has_current_user_msg {
                                messages.push(serde_json::json!({"role": "user", "content": user_message}));
                            }
                            let new_tokens = super::token_counter::TokenCounter::count_messages(&messages);
                            log::info!(
                                "自动压缩后重建消息: {} 条, {} tokens (原 {} 条 {} tokens)",
                                messages.len(), new_tokens, msg_count, current_tokens
                            );
                        }
                    }
                    Err(e) => {
                        log::warn!("自动压缩失败（将继续使用 ContextGuard 截断）: {}", e);
                    }
                }
            }
        }

        // 5.5 tool_call 清洗 — 在 ContextGuard 之前清洗 ID/配对
        super::tool_call_sanitizer::sanitize_messages_for_llm(&mut messages, provider);

        // 6. 上下文预算守卫（ContextGuard）— 唯一的预算强制点
        //
        // 统一管理所有上下文裁剪。5 步策略链：
        // JSON 剥离 → 单条截断 → 总预算压缩 → 旧轮次删除 → 配对修复。
        // 归档功能集成在守卫触发后。
        let mut final_messages = messages;
        {
            let sys_prompt_tokens = super::token_counter::TokenCounter::count(&system_prompt);
            let guard_config = super::context_guard::ContextGuardConfig::for_model(&agent.model)
                .with_system_prompt_tokens(sys_prompt_tokens);
            let guard_result = super::context_guard::enforce(&guard_config, &mut final_messages);
            if guard_result.modified {
                log::info!(
                    "ContextGuard: {}→{} tokens, removed={}, compacted={}, within_budget={}",
                    guard_result.tokens_before, guard_result.tokens_after,
                    guard_result.removed, guard_result.compacted, guard_result.within_budget,
                );
                // 被裁剪时归档到 workspace/daily/
                if guard_result.removed > 0 {
                    if let Some(ref wp) = workspace_path {
                        let daily_dir = std::path::PathBuf::from(wp).join("daily");
                        let _ = std::fs::create_dir_all(&daily_dir);
                        let date_str = chrono::Local::now().format("%Y-%m-%d").to_string();
                        let archive_path = daily_dir.join(format!("{}.md", date_str));
                        let note = format!(
                            "\n\n---\n\n## Session {} ({}) — ContextGuard 裁剪\n\n移除 {} 条消息, 压缩 {} 条 ({}→{} tokens)\n\n",
                            &session_id[..8.min(session_id.len())],
                            chrono::Local::now().format("%H:%M"),
                            guard_result.removed, guard_result.compacted,
                            guard_result.tokens_before, guard_result.tokens_after,
                        );
                        use std::io::Write;
                        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&archive_path) {
                            let _ = f.write_all(note.as_bytes());
                        }
                    }
                }
            }
            if !guard_result.within_budget {
                log::warn!(
                    "ContextGuard: 所有策略用尽仍超预算 ({}>{} tokens)，LLM 调用可能失败",
                    guard_result.tokens_after, guard_config.total_budget(),
                );
            }
        }

        // 7. 智能路由：根据消息复杂度选择模型
        let selected_model = {
            let router_config = super::router::RouterConfig::from_agent_config(
                &agent.model, agent.config.as_deref()
            );
            if router_config.is_enabled() {
                let complexity = super::router::score_complexity(
                    user_message, final_tool_defs.len(), final_messages.len()
                );
                let model = super::router::select_model(&router_config, &complexity);
                if model != agent.model {
                    log::info!("智能路由: 复杂度={:.2}, {} → {}", complexity.score, agent.model, model);
                }
                model
            } else {
                agent.model.clone()
            }
        };

        // 构建 Failover 执行器（从 agent config 读取 fallback 模型链）
        let failover = super::failover::FailoverExecutor::from_agent_config(
            &selected_model, agent.config.as_deref()
        );
        // 如果有 fallback 模型，日志记录
        if failover.all_models().len() > 1 {
            log::info!("Failover 模型链: {:?}", failover.all_models());
        }

        // 温度策略（参考 OpenClaw/IronClaw/Hermes）：
        // - 用户手动设了 → 用用户的
        // - 没设 → 不传（用模型默认值，对话更自然）
        // - 辅助任务（摘要/进化 review）在各自模块里固定 0.3

        // 从 Agent config JSON 读取 thinking level
        let thinking_level = agent.config.as_deref()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
            .and_then(|v| v["thinkingLevel"].as_str().map(|s| s.to_string()))
            .map(|s| ThinkingLevel::from_str(&s))
            .filter(|l| l.is_enabled());

        let config = LlmConfig {
            provider: provider.to_string(), api_key: api_key.to_string(),
            model: selected_model, base_url: base_url.map(|s| s.to_string()),
            temperature: agent.temperature.or(intent_temperature), max_tokens: agent.max_tokens,
            thinking_level,
        };
        let system_prompt_opt = if provider == "anthropic" { Some(system_prompt.as_str()) } else { None };

        // 7. 先保存用户消息（去除 base64，用磁盘路径引用替代）
        let user_msg_for_db = if !saved_paths.is_empty() {
            let text = super::multimodal::strip_image_markers(user_message);
            let refs = saved_paths.iter().map(|p| format!("![图片]({})", p)).collect::<Vec<_>>().join("\n");
            if text.is_empty() { refs } else { format!("{}\n{}", text, refs) }
        } else {
            super::multimodal::strip_image_markers(user_message)
        };
        let conv_id = memory::conversation::save_user_message(&self.pool, agent_id, session_id, &user_msg_for_db)
            .await.map_err(|e| format!("保存用户消息失败: {}", e))?;
        let user_msg_json = serde_json::json!({"role": "user", "content": &user_msg_for_db});
        let _ = memory::conversation::save_chat_message(&self.pool, session_id, agent_id, &user_msg_json).await;

        // 8. 执行 agent loop，捕获失败
        let dispatcher: Box<dyn super::dispatcher::ToolDispatcher> = if super::agent_loop::is_xml_model(&config.model) {
            Box::new(super::dispatcher::XmlDispatcher::new())
        } else {
            Box::new(super::dispatcher::NativeDispatcher::new(provider))
        };
        // Harness: 创建执行预算和进度追踪器
        let execution_budget = super::execution_budget::ExecutionBudget::from_config(agent.config.as_deref());
        let progress_tracker = workspace_path.as_ref().map(|wp| {
            std::sync::Mutex::new(super::progress::ProgressTracker::new(wp))
        });

        let loop_deps = super::agent_loop::AgentLoopDeps {
            pool: &self.pool,
            tool_manager: &self.tool_manager,
            mcp_manager: &self.mcp_manager,
            policy_engine: &self.policy_engine,
            event_broadcaster: &self.event_broadcaster,
            hook_runner: &self.hook_runner,
            lifecycle: &self.lifecycle,
            agent_config: agent.config.clone(),
            provider_registry: Some(&self.provider_registry),
            evolution_state: Some(&self.evolution_state),
            approval_manager: Some(&self.approval_manager),
            app_handle: None,
            budget: Some(&execution_budget),
            progress: progress_tracker.as_ref(),
        };

        // C4: 跨会话恢复 — 检查 PROGRESS.md 是否有未完成任务
        if let Some(ref wp) = workspace_path {
            if let Some(recovery_ctx) = super::progress::check_pending_progress(wp) {
                log::info!("Session Recovery: 注入恢复上下文");
                final_messages.push(serde_json::json!({"role": "user", "content": recovery_ctx}));
            }
        }

        let response = match super::agent_loop::run_agent_loop(&loop_deps, &config, final_messages, system_prompt_opt, provider, &tx, &final_tool_defs, &skill_tools, agent_id, session_id, &cancel_token, dispatcher.as_ref()).await {
            Ok(resp) => resp,
            Err(e) => {
                let persisted = if e.partial_content.trim().is_empty() {
                    format!("⚠️ 回复生成失败: {}", e.message)
                } else {
                    format!("{}\n\n⚠️ 回复生成失败: {}", e.partial_content, e.message)
                };
                let _ = memory::conversation::update_agent_response(&self.pool, &conv_id, &persisted).await;
                return Err(e.message);
            }
        };

        // 9. 成功时更新完整回复
        memory::conversation::update_agent_response(&self.pool, &conv_id, &response)
            .await.map_err(|e| format!("更新回复失败: {}", e))?;

        // 9.5 Verify-Fix 循环：agent_loop 结束后自动验证，失败则修复
        if let Some(ref wp) = workspace_path {
            // 从最近备份推断改动的文件
            let recent_backups = super::file_harness::list_backups();
            let changed: Vec<String> = recent_backups.iter()
                .take(20)
                .filter_map(|(_, name, _)| {
                    // 备份文件名格式: YYYYMMDD_HHMMSS.mmm_filename
                    name.splitn(2, '_').nth(1)
                        .and_then(|rest| rest.splitn(2, '_').nth(1))
                        .map(|f| f.to_string())
                })
                .collect();

            if !changed.is_empty() {
                if let Some(verify_result) = super::auto_verify::run_project_verify(&changed, Some(wp)) {
                    if !verify_result.passed {
                        log::warn!("Verify-Fix: 验证失败 — {}", verify_result.summary);
                        let _ = tx.send(format!("\n⚠️ 自动验证发现问题: {}\n", verify_result.summary));

                        // 尝试修复（最多 2 轮，消耗 ExecutionBudget）
                        let fix_prompt = format!(
                            "[Auto-Verify Failed]\n验证发现以下错误，请修复：\n\n{}\n\n请逐一修复这些问题。",
                            verify_result.summary
                        );
                        // 注入修复消息到新一轮 agent_loop
                        let fix_messages = vec![
                            serde_json::json!({"role": "user", "content": fix_prompt}),
                        ];
                        let fix_response = super::agent_loop::run_agent_loop(
                            &loop_deps, &config, fix_messages, system_prompt_opt,
                            provider, &tx, &final_tool_defs, &skill_tools,
                            agent_id, session_id, &cancel_token, dispatcher.as_ref(),
                        ).await;
                        match fix_response {
                            Ok(fix_text) => {
                                log::info!("Verify-Fix: 修复完成 ({}字符)", fix_text.len());
                                let _ = tx.send("\n✅ 自动修复完成\n".to_string());
                            }
                            Err(e) => {
                                log::warn!("Verify-Fix: 修复失败: {}", e.message);
                                let _ = tx.send(format!("\n❌ 自动修复失败: {}\n", e.message));
                            }
                        }
                    } else {
                        log::info!("Verify-Fix: 验证通过 ✓");
                    }
                }
            }
        }

        // 9.8 记忆使用反馈循环：检查 Agent 回复是否引用了注入的记忆
        if !injected_memories.is_empty() {
            let (used_ids, unused_ids) = super::learner::check_memory_usage(&response, &injected_memories);
            log::info!("记忆反馈: {} 条被引用, {} 条未引用", used_ids.len(), unused_ids.len());
            if !used_ids.is_empty() || !unused_ids.is_empty() {
                let pool = self.pool.clone();
                let used = used_ids.clone();
                let unused = unused_ids.clone();
                tokio::spawn(async move {
                    super::learner::update_memory_feedback(&pool, &used, &unused).await;
                });
            }
        }

        // 自动生成会话标题：如果是第一条消息且标题为默认值
        if let Ok(Some(session)) = memory::conversation::get_session(&self.pool, session_id).await {
            let is_default_title = session.title == "New Session"
                || session.title.starts_with("对话")
                || session.title.starts_with("Conversation");
            if is_default_title {
                let auto_title: String = user_message.chars().take(20).collect();
                let _ = memory::conversation::rename_session(&self.pool, session_id, &auto_title).await;
            }
        }

        // invalidate 会话消息缓存（本次对话已添加新消息）
        if let Ok(mut cache) = self.session_msg_cache.lock() {
            cache.pop(session_id);
        }

        // 10. 自动生成会话摘要（每 10 条消息自动压缩一次）
        {
            let msg_count = memory::conversation::get_chat_message_count(&self.pool, session_id).await.unwrap_or(0);
            if msg_count > 0 && msg_count % 10 == 0 {
                log::info!("触发自动会话摘要: session={}, messages={}", session_id, msg_count);
                // 异步执行，不阻塞返回
                let pool = self.pool.clone();
                let sid = session_id.to_string();
                let _aid = agent_id.to_string();
                let cfg = config.clone();
                tokio::spawn(async move {
                    let messages = memory::conversation::load_chat_messages(&pool, &sid, 50).await.unwrap_or_default();
                    if messages.len() < 6 { return; }
                    // 构建摘要文本
                    let mut text = String::new();
                    for msg in &messages {
                        let role = msg["role"].as_str().unwrap_or("?");
                        if role == "system" { continue; }
                        let content = msg["content"].as_str().unwrap_or("");
                        let preview: String = content.chars().take(300).collect();
                        text.push_str(&format!("{}: {}\n", role, preview));
                    }
                    let prompt = format!(
                        "请将以下对话压缩为简洁摘要（3-5 句话），保留关键决策、任务和上下文：\n\n{}", text
                    );
                    let compact_cfg = build_compact_llm_config(&cfg, &pool).await;
                    let client = LlmClient::new(compact_cfg);
                    let msgs = vec![serde_json::json!({"role": "user", "content": prompt})];
                    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                    if let Ok(resp) = client.call_stream(&msgs, None, None, tx).await {
                        if !resp.content.trim().is_empty() {
                            let _ = memory::conversation::update_session_summary(&pool, &sid, resp.content.trim()).await;
                            log::info!("自动摘要已生成: session={}, 长度={}", sid, resp.content.len());
                        }
                    }
                });
            }
        }

        // 11. Learner：从会话中提取经验（异步，fire-and-forget）
        // 仅当对话有 >3 条用户消息时才触发（避免从琐碎对话中学习）
        {
            let user_msg_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM chat_messages WHERE session_id = ? AND role = 'user'"
            )
            .bind(session_id)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

            if user_msg_count > 3 {
            let pool = self.pool.clone();
            let aid = agent_id.to_string();
            let sid = session_id.to_string();
            let wp = workspace_path.clone();
            let learner_llm_config = build_compact_llm_config(&config, &self.pool).await;
            tokio::spawn(async move {
                // 确保 DB schema 有 access_count / unused_recall_count 列
                super::memory_eviction::ensure_schema(&pool).await;
                let _ = sqlx::query("ALTER TABLE memories ADD COLUMN unused_recall_count INTEGER DEFAULT 0")
                    .execute(&pool).await;

                // 用 LLM 提取经验（v2，替代关键词匹配）
                let outcome = super::learner::extract_lessons_with_llm(&pool, &aid, &sid, &learner_llm_config).await;
                if !outcome.lessons.is_empty() {
                    log::info!("Learner: 从会话 {} 学到 {} 条经验", &sid[..8], outcome.lessons.len());
                    super::learner::persist_lessons(&pool, &aid, wp.as_deref(), &outcome.lessons).await;

                    // 每 10 次学习后执行一次蒸馏
                    let learned_count: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM memories WHERE agent_id = ? AND memory_type = 'learned'"
                    ).bind(&aid).fetch_one(&pool).await.unwrap_or(0);

                    if learned_count > 0 && learned_count % 10 == 0 {
                        let result = super::distillation::distill_rules(&pool, &aid).await;
                        if !result.rules.is_empty() {
                            if let Some(ref wp) = wp {
                                let _ = super::distillation::append_to_standing_orders(wp, &result.rules);
                            }
                        }
                    }

                    // 记忆淘汰
                    let config = super::memory_eviction::EvictionConfig::default();
                    let removed = super::memory_eviction::run_eviction(&pool, &aid, &config).await;
                    if removed > 0 {
                        log::info!("Learner: 淘汰了 {} 条旧记忆", removed);
                    }

                    // 文件记忆一致性验证（每次学习后检查）
                    let invalidated = super::learner::verify_file_memories(&pool, &aid).await;
                    if invalidated > 0 {
                        log::info!("Learner: {} 条文件记忆已失效（文件不存在）", invalidated);
                    }
                } else if let Some(reason) = outcome.skipped_reason {
                    log::debug!("Learner: 跳过学习 — {}", reason);
                }
            });
            } else {
                log::debug!("Learner: 跳过（用户消息数 {} <= 3）", user_msg_count);
            }
        }

        // 12. 每日 Token 限额检查（已在 send_message_stream 入口 0b 步骤实现）

        // 12. 自我进化：检查是否触发后台 review
        {
            self.evolution_state.on_user_message();
            let should_skill = self.evolution_state.should_review_skills(&self.evolution_config);
            let should_memory = self.evolution_state.should_review_memory(&self.evolution_config);

            if should_skill || should_memory {
                let review_type = if should_skill && should_memory {
                    super::self_evolution::ReviewType::Both
                } else if should_skill {
                    super::self_evolution::ReviewType::Skill
                } else {
                    super::self_evolution::ReviewType::Memory
                };

                log::info!("进化引擎: 触发 {:?} review（tool_calls={}, user_msgs={}）",
                    review_type,
                    self.evolution_state.tool_calls_since_skill_review.load(std::sync::atomic::Ordering::Relaxed),
                    self.evolution_state.user_msgs_since_memory_review.load(std::sync::atomic::Ordering::Relaxed),
                );

                // 获取最近的对话消息
                let recent = memory::conversation::load_chat_messages(&self.pool, session_id, 30).await.unwrap_or_default();
                if recent.len() >= 4 {
                    super::self_evolution::spawn_background_review(
                        self.pool.clone(),
                        agent_id.to_string(),
                        session_id.to_string(),
                        api_key.to_string(),
                        provider.to_string(),
                        base_url.map(|s| s.to_string()),
                        agent.model.clone(),
                        review_type,
                        recent,
                        self.evolution_state.clone(),
                    ).await;
                }
            }
        }

        Ok(response)
    }

    ///
    /// 建议每天或每小时调用一次
    pub async fn run_memory_hygiene(&self, agent_id: &str, workspace_path: Option<&str>) -> Result<String, String> {
        let mut report = Vec::new();

        // 1. 删除 30 天前的对话历史（旧表）
        let _deleted = memory::conversation::delete_old_conversations(&self.pool, agent_id, 30).await
            .map_err(|e| format!("清理旧对话失败: {}", e))?;
        report.push(format!("旧对话: 已清理30天前记录"));

        // 2. 优先级淘汰（保留最多 1000 条，Critical 不淘汰）
        let sqlite_mem = SqliteMemory::new(self.pool.clone());
        let evicted = <SqliteMemory as memory::Memory>::evict_by_priority(&sqlite_mem, agent_id, 1000).await?;
        if evicted > 0 {
            report.push(format!("优先级淘汰: {} 条低优先级记忆", evicted));
        }

        // 3. Cold 归档（超过 7 天的非 Critical 记忆）
        if let Some(wp) = workspace_path {
            let cold_dir = std::path::PathBuf::from(wp).join("memory");
            let tiered = super::super::memory::TieredMemory::new(
                SqliteMemory::new(self.pool.clone()),
                Some(cold_dir),
            );
            let archived = tiered.archive_to_cold(agent_id, 7).await?;
            if archived > 0 {
                report.push(format!("Cold 归档: {} 条记忆", archived));
            }
        }

        // 4. 清理过期响应缓存
        let cache = super::response_cache::ResponseCache::new(self.pool.clone());
        let expired = cache.cleanup_expired().await?;
        if expired > 0 {
            report.push(format!("响应缓存: 清理 {} 条过期", expired));
        }

        let summary = if report.is_empty() {
            "无需清理".to_string()
        } else {
            report.join("; ")
        };
        log::info!("Memory Hygiene 完成: agent={}, {}", agent_id, summary);
        Ok(summary)
    }

    /// 获取每日 Token 限额（委托给 AgentStore）
    async fn get_daily_token_limit(&self, agent_id: &str) -> u64 {
        self.agent_store.get_daily_token_limit(agent_id).await
    }

    /// 获取今日已消耗 token（委托给 AgentStore）
    async fn get_today_token_usage(&self, agent_id: &str) -> u64 {
        self.agent_store.get_today_token_usage(agent_id).await
    }

    // detect_tool_intent, scrub_credentials, is_xml_model, append_assistant_message
    // 已移到 agent_loop.rs

    /// 获取对话历史（按 session）
    pub async fn get_conversations(&self, agent_id: &str, session_id: &str, limit: i64) -> Result<Vec<(String, String)>, String> {
        memory::conversation::get_history(&self.pool, agent_id, session_id, limit)
            .await.map_err(|e| format!("获取对话历史失败: {}", e))
    }

    /// 清除会话的对话历史
    pub async fn clear_history(&self, session_id: &str) -> Result<(), String> {
        memory::conversation::clear_session_history(&self.pool, session_id)
            .await.map_err(|e| format!("清除对话历史失败: {}", e))
    }

    /// 压缩会话（参考 OpenClaw compact 机制）
    ///
    /// 流程：
    /// 1. 从 chat_messages 读取全部消息
    /// 2. 将要压缩的旧消息（保留最近 10 条）送给 LLM 生成摘要
    /// 3. 删除旧消息
    /// 4. 将摘要作为 system 消息插入，为后续对话提供上下文
    /// 5. 更新 session.summary
    pub async fn compact_session(
        &self,
        agent_id: &str,
        session_id: &str,
        api_key: &str,
        provider: &str,
        base_url: Option<&str>,
    ) -> Result<String, String> {
        // 1. 从 chat_messages 读取全部消息
        let all_messages: Vec<(i64, String, String)> = sqlx::query_as(
            "SELECT seq, role, COALESCE(content, '') FROM chat_messages WHERE session_id = ? ORDER BY seq ASC"
        ).bind(session_id).fetch_all(&self.pool).await
            .map_err(|e| format!("获取消息失败: {}", e))?;

        let total_count = all_messages.len();
        log::info!("compact: 读取到 {} 条消息", total_count);
        if total_count <= 10 {
            return Err(format!("消息数不多（{}条），无需压缩", total_count));
        }

        // 2. 保留最近 10 条，压缩其余的
        let keep_count = 10;
        let to_compact = &all_messages[..total_count - keep_count];
        let compact_count = to_compact.len();

        log::info!("compact: 将压缩 {} 条，保留 {} 条", compact_count, keep_count);

        // 构建要压缩的对话文本
        // 改进：增加字符预算到 16000，每条消息截断到 300 字符，保留更多语义
        let mut conversation_text = String::new();
        let char_budget = 16000usize;
        for (_, role, content) in to_compact.iter() {
            let truncated: String = content.chars().take(300).collect();
            let line = format!("{}: {}\n", role, truncated);
            if conversation_text.len() + line.len() > char_budget { break; }
            conversation_text.push_str(&line);
        }

        // 提取最后一条 user 消息（用于 "MUST PRESERVE" 指令）
        let last_user_request: String = to_compact.iter()
            .rev()
            .find(|(_, role, _)| role == "user")
            .map(|(_, _, content)| content.chars().take(200).collect())
            .unwrap_or_default();

        // 3. 用 Agent 自身模型生成摘要
        let agent_info = self.get_agent_cached(agent_id).await?;
        let compact_model = agent_info.model.clone();
        log::info!("compact: 使用模型 {} 生成摘要", compact_model);

        // 参照 OpenClaw compaction.ts 的保留指令
        let summary_prompt = format!(
            "请将以下对话压缩为简洁摘要（最多 800 字符）。使用与对话相同的语言。\n\
             \n\
             **必须保留：**\n\
             - 用户最后的请求是什么，以及当前的处理状态\n\
             - 活跃的任务及其进度（进行中、已阻塞、待处理）\n\
             - 已做出的决策及其理由\n\
             - 待办事项、开放问题和约束条件\n\
             - 所有文件路径、URL、ID 等标识符必须精确保留\n\
             \n\
             **可以省略：**\n\
             - 工具调用的详细参数和返回值\n\
             - 重复的对话内容\n\
             - 已完成且不再相关的中间步骤\n\
             \n\
             用户最后的请求: {}\n\
             \n\
             只输出摘要，不要其他内容。\n\n---\n{}\n---",
            last_user_request, conversation_text
        );

        let config = LlmConfig {
            provider: provider.to_string(),
            api_key: api_key.to_string(),
            model: compact_model,
            base_url: base_url.map(|s| s.to_string()),
            temperature: Some(0.2),
            max_tokens: Some(1024),
            thinking_level: None,
        };
        let client = LlmClient::new(config);
        let messages = vec![
            serde_json::json!({"role": "user", "content": summary_prompt}),
        ];
        let (dummy_tx, _rx) = mpsc::unbounded_channel::<String>();
        log::info!("compact: 开始调 LLM 生成摘要...");

        // 30 秒超时保护
        let llm_result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            client.call_stream(&messages, None, None, dummy_tx)
        ).await;

        let summary = match llm_result {
            Ok(Ok(response)) => {
                let s = response.content.trim().to_string();
                log::info!("compact: 摘要生成完成，{}字符", s.len());
                s
            }
            Ok(Err(e)) => {
                log::error!("compact: LLM 调用失败: {}", e);
                let fallback: String = to_compact.iter()
                    .rev().take(5)
                    .map(|(_, role, content)| {
                        let t: String = content.chars().take(100).collect();
                        format!("{}: {}", role, t)
                    })
                    .collect::<Vec<_>>().join("\n");
                format!("[自动摘要失败]\n{}", fallback)
            }
            Err(_) => {
                log::error!("compact: LLM 调用超时（30s）");
                let fallback: String = to_compact.iter()
                    .rev().take(5)
                    .map(|(_, role, content)| {
                        let t: String = content.chars().take(100).collect();
                        format!("{}: {}", role, t)
                    })
                    .collect::<Vec<_>>().join("\n");
                format!("[摘要生成超时]\n{}", fallback)
            }
        };

        // 4. 记录压缩边界点（不删除任何消息！用户仍能看到全部历史）
        //    只更新 session.summary 和 compacted_before_seq
        //    下次构建 LLM context 时，compacted_before_seq 之前的消息用 summary 替代
        let boundary_seq = to_compact.last().map(|(seq, _, _)| *seq).unwrap_or(0);

        // 事务性写入：summary + boundary 必须一起成功或一起失败
        let compact_key = format!("compact_boundary_{}", session_id);
        let mut tx = self.pool.begin().await.map_err(|e| format!("事务开始失败: {}", e))?;
        sqlx::query("UPDATE chat_sessions SET summary = ? WHERE id = ?")
            .bind(&summary).bind(session_id)
            .execute(&mut *tx).await.map_err(|e| format!("保存摘要失败: {}", e))?;
        sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)")
            .bind(&compact_key).bind(boundary_seq.to_string())
            .execute(&mut *tx).await.map_err(|e| format!("保存边界失败: {}", e))?;
        tx.commit().await.map_err(|e| format!("事务提交失败: {}", e))?;

        // 5. 计算压缩效果
        let tokens_before: usize = all_messages.iter().map(|(_, _, c)| estimate_tokens(c)).sum();
        let kept_messages = &all_messages[total_count - keep_count..];
        let tokens_after: usize = kept_messages.iter().map(|(_, _, c)| estimate_tokens(c)).sum::<usize>()
            + estimate_tokens(&summary);

        log::info!("compact 完成: session={}, boundary_seq={}, tokens {}→{} (LLM context only, 消息不删除)",
            &session_id[..8.min(session_id.len())], boundary_seq, tokens_before, tokens_after);

        // 失效 LRU 缓存（避免下次请求用到压缩前的旧消息）
        if let Ok(mut cache) = self.session_msg_cache.lock() {
            cache.pop(session_id);
            log::debug!("compact: 已失效 session 消息缓存");
        }

        Ok(format!(
            "Context compacted: {} → {} (LLM context)\n{} messages summarized, all history preserved.\n\n{}",
            format_token_count(tokens_before), format_token_count(tokens_after),
            compact_count,
            if summary.len() > 300 { format!("{}...", &summary.chars().take(300).collect::<String>()) } else { summary }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:").await.unwrap();
        crate::db::schema::init_schema(&pool).await.unwrap();
        pool
    }

    // Agent CRUD 测试已移到 agent_store.rs

    #[tokio::test]
    async fn test_default_tools_registered() {
        // 验证 Orchestrator 创建时默认注册了内置工具
        let pool = setup_pool().await;
        let mut tm = ToolManager::new();
        tm.register_tool(Box::new(CalculatorTool));
        tm.register_tool(Box::new(DateTimeTool));
        tm.register_tool(Box::new(FileReadTool));
        tm.register_tool(Box::new(FileWriteTool));
        tm.register_tool(Box::new(FileListTool));
        tm.register_tool(Box::new(FileEditTool));
        tm.register_tool(Box::new(BashExecTool));
        tm.register_tool(Box::new(CodeSearchTool));
        tm.register_tool(Box::new(WebFetchTool));
        tm.register_tool(Box::new(MemoryReadTool::new(pool.clone())));
        tm.register_tool(Box::new(MemoryWriteTool::new(pool.clone())));
        let defs = tm.get_tool_definitions();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"calculator"));
        assert!(names.contains(&"datetime"));
        assert!(names.contains(&"file_read"));
        assert!(names.contains(&"file_write"));
        assert!(names.contains(&"file_list"));
        assert!(names.contains(&"file_edit"));
        assert!(names.contains(&"bash_exec"));
        assert!(names.contains(&"code_search"));
        assert!(names.contains(&"web_fetch"));
        assert!(names.contains(&"memory_read"));
        assert!(names.contains(&"memory_write"));
    }

    #[test]
    fn test_is_xml_model() {
        // is_xml_model 已移到 agent_loop.rs
        assert!(!super::super::agent_loop::is_xml_model("gpt-4"));
        assert!(!super::super::agent_loop::is_xml_model("deepseek-chat"));
        assert!(!super::super::agent_loop::is_xml_model("qwen2.5-72b"));
        assert!(super::super::agent_loop::is_xml_model("qwen-turbo"));
    }
}
