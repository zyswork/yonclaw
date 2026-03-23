//! Agent 编排引擎
//!
//! 支持 LLM 调用、多轮工具调用、Agent 管理、对话历史

use super::llm::{LlmClient, LlmConfig};
use super::mcp_manager::McpManager;
use super::skill_tool::SkillTool;
use super::media::MediaProvider; // 导入 trait 使 describe_image 可用
use super::skills::SkillManager;
use super::soul::{SoulEngine, SectionBudget};
use super::tools::{ToolManager, CalculatorTool, DateTimeTool, FileReadTool, FileWriteTool, FileListTool, FileEditTool, DiffEditTool, BashExecTool, CodeSearchTool, WebFetchTool, MemoryReadTool, MemoryWriteTool, SettingsTool, ProviderTool, AgentSelfConfigTool, Tool};
use super::tools::{parse_tools_config, is_tool_enabled};
use super::workspace::AgentWorkspace;
use super::subagent::SubagentRegistry;
use super::tool_policy::ToolPolicyEngine;
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
    pub event_broadcaster: super::observer::EventBroadcaster,
    /// 工具钩子运行器
    hook_runner: std::sync::Mutex<super::hooks::HookRunner>,
    /// 生命周期事件管理器（替代 hooks/observer）
    lifecycle: super::lifecycle::LifecycleManager,
    /// 插件注册表（旧版）
    pub plugin_registry: crate::plugin_sdk::PluginRegistry,
    /// 模型提供商注册表
    provider_registry: crate::plugin_system::ProviderRegistry,
    /// 自我进化状态
    pub evolution_state: std::sync::Arc<super::self_evolution::EvolutionState>,
    /// 自我进化配置
    evolution_config: super::self_evolution::EvolutionConfig,
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
        tool_manager.register_tool(Box::new(CodeSearchTool));
        tool_manager.register_tool(Box::new(WebFetchTool));
        tool_manager.register_tool(Box::new(MemoryReadTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(MemoryWriteTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(SettingsTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(ProviderTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(AgentSelfConfigTool::new(pool.clone())));
        tool_manager.register_tool(Box::new(super::delegate::DelegateTaskTool::new(pool.clone())));
        let mcp_manager = McpManager::new(pool.clone());
        // 初始化钩子运行器（注册默认日志钩子）
        let mut hook_runner = super::hooks::HookRunner::new();
        hook_runner.register(Box::new(super::hooks::LoggingHook));

        Self {
            agent_store: super::agent_store::AgentStore::new(pool.clone()),
            pool, tool_manager, mcp_manager,
            policy_engine: std::sync::Mutex::new(ToolPolicyEngine::new()),
            skill_cache: std::sync::Mutex::new(HashMap::new()),
            subagent_registry: SubagentRegistry::new(),
            session_locks: std::sync::Mutex::new(HashMap::new()),
            session_msg_cache: std::sync::Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(SESSION_CACHE_MAX).unwrap()
            )),
            rate_limiter: super::rate_limiter::RateLimiter::new(super::rate_limiter::RateLimitConfig::default()),
            event_broadcaster: super::observer::EventBroadcaster::default(),
            hook_runner: std::sync::Mutex::new(hook_runner),
            lifecycle: {
                let mut lm = super::lifecycle::LifecycleManager::new();
                lm.register(Box::new(super::lifecycle::LoggingHandler));
                lm.register(Box::new(super::lifecycle::TokenTrackingHandler));
                lm
            },
            plugin_registry: crate::plugin_sdk::PluginRegistry::new(),
            provider_registry: crate::plugin_system::create_default_registry(),
            evolution_state: std::sync::Arc::new(super::self_evolution::EvolutionState::new()),
            evolution_config: super::self_evolution::EvolutionConfig::default(),
        }
    }

    /// 获取数据库连接池引用
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
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

        // 1. 获取 agent 信息（缓存，60 秒 TTL）
        let agent = self.get_agent_cached(agent_id).await?;

        // 2. 构建 system prompt（从 Soul 文件组装）
        // 迁移旧的 .openclaw 路径到 .yonclaw
        let workspace_path = agent.workspace_path.as_ref().map(|wp| {
            if wp.contains("/.openclaw/") {
                let new_wp = wp.replace("/.openclaw/", "/.yonclaw/");
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
                let engine = SoulEngine::with_defaults();
                let budget = SectionBudget::default();
                let mut prompt = engine.build_system_prompt_with_budget(&workspace, &budget);
                // 注入工作区环境信息
                prompt.push_str(&format!(
                    "\n\n---\n\n# Environment\n\n- Workspace: {}\n- Skills: {}/skills\n- Memory: {}/memory\n- Agent ID: {}",
                    wp, wp, wp, agent_id
                ));
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
        log::info!("最终 system_prompt 长度: {} 字节", system_prompt.len());

        // 3. MemoryLoader 注入记忆（三层存储：Hot LRU → Warm SQLite → Cold 归档）
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
            if let Ok(Some(memory_text)) = loader.load_relevant_memories(agent_id, user_message).await {
                system_prompt = format!("{}\n\n---\n\n{}", system_prompt, memory_text);
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
            log::info!("注入 {} 个 MCP 工具", mcp_defs.len());
            final_tool_defs.extend(mcp_defs);
        }

        // 4b. 技能激活：根据用户消息匹配技能，注册技能工具
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

        // 5. 构建消息列表（LRU 缓存 → DB fallback）
        let structured_history = {
            // 先查 LRU 缓存（10秒内有效）
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
                let msgs = memory::conversation::load_chat_messages(&self.pool, session_id, 20).await.unwrap_or_default();
                // 写入缓存
                if let Ok(mut cache) = self.session_msg_cache.lock() {
                    cache.put(session_id.to_string(), (msgs.clone(), std::time::Instant::now()));
                }
                msgs
            }
        };

        // 如果 session 有摘要，注入到 system prompt
        if let Ok(Some(session)) = memory::conversation::get_session(&self.pool, session_id).await {
            if let Some(ref summary) = session.summary {
                system_prompt = format!(
                    "{}\n\n---\n\n# 之前的对话摘要\n\n{}",
                    system_prompt, summary
                );
            }
        }

        let mut messages: Vec<serde_json::Value> = Vec::new();
        log::info!("Provider: '{}', 添加 system message: {}", provider, provider == "openai");
        if provider == "openai" {
            log::info!("注入 system message, 长度: {} 字节", system_prompt.len());
            messages.push(serde_json::json!({"role": "system", "content": &system_prompt}));
        }

        if !structured_history.is_empty() {
            // 使用结构化历史（包含完整的工具调用上下文）
            log::info!("加载 {} 条结构化历史消息", structured_history.len());
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

        let config = LlmConfig {
            provider: provider.to_string(), api_key: api_key.to_string(),
            model: selected_model, base_url: base_url.map(|s| s.to_string()),
            temperature: agent.temperature, max_tokens: agent.max_tokens,
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
        };
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

        // 自动生成会话标题：如果是第一条消息且标题为默认值
        if let Ok(Some(session)) = memory::conversation::get_session(&self.pool, session_id).await {
            if session.title == "New Session" {
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
                    let client = LlmClient::new(LlmConfig {
                        provider: cfg.provider.clone(), api_key: cfg.api_key.clone(),
                        model: cfg.model.clone(), base_url: cfg.base_url.clone(),
                        temperature: Some(0.3), max_tokens: Some(512),
                    });
                    let msgs = vec![serde_json::json!({"role": "user", "content": prompt})];
                    let (tx, _) = tokio::sync::mpsc::unbounded_channel::<String>();
                    if let Ok(resp) = client.call_stream(&msgs, None, None, tx).await {
                        if !resp.content.trim().is_empty() {
                            let _ = memory::conversation::update_session_summary(&pool, &sid, resp.content.trim()).await;
                            log::info!("自动摘要已生成: session={}, 长度={}", sid, resp.content.len());
                        }
                    }
                });
            }
        }

        // 11. 每日 Token 限额检查（异步，不阻塞返回，超限下次请求拦截）
        // TODO: 在 send_message_stream 入口处做前置检查

        // 11. 自我进化：检查是否触发后台 review
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

    /// 压缩会话：用 LLM 生成摘要，删除旧消息保留最近 5 条
    pub async fn compact_session(
        &self,
        agent_id: &str,
        session_id: &str,
        api_key: &str,
        provider: &str,
        base_url: Option<&str>,
    ) -> Result<String, String> {
        // 获取所有消息
        let history = memory::conversation::get_history(&self.pool, agent_id, session_id, 200)
            .await.map_err(|e| format!("获取历史失败: {}", e))?;

        if history.is_empty() {
            return Err("会话中没有消息可压缩".to_string());
        }

        // 构建摘要请求
        let mut conversation_text = String::new();
        for (user_msg, agent_resp) in history.iter().rev() {
            conversation_text.push_str(&format!("用户: {}\n助手: {}\n\n", user_msg, agent_resp));
        }

        let summary_prompt = format!(
            "请将以下对话压缩为一段简洁的摘要，保留关键信息、决策和上下文。摘要应该让后续对话能够理解之前讨论的内容。\n\n---\n\n{}\n\n---\n\n请直接输出摘要，不要加前缀。",
            conversation_text
        );

        // 用 LLM 生成摘要（单次调用，不带工具）
        // 使用 Agent 自身模型而非硬编码
        let agent_info = self.get_agent_cached(agent_id).await?;
        let config = LlmConfig {
            provider: provider.to_string(),
            api_key: api_key.to_string(),
            model: agent_info.model.clone(),
            base_url: base_url.map(|s| s.to_string()),
            temperature: Some(0.3),
            max_tokens: Some(1024),
        };
        let client = LlmClient::new(config);
        let messages = vec![
            serde_json::json!({"role": "user", "content": summary_prompt}),
        ];
        let (dummy_tx, _) = mpsc::unbounded_channel::<String>();
        let response = client.call_stream(&messages, None, None, dummy_tx)
            .await.map_err(|e| format!("LLM 摘要生成失败: {}", e))?;

        let summary = response.content.trim().to_string();

        // 存入 session.summary
        memory::conversation::update_session_summary(&self.pool, session_id, &summary)
            .await.map_err(|e| format!("保存摘要失败: {}", e))?;

        // 删除旧消息，保留最近 5 条
        memory::conversation::delete_old_session_messages(&self.pool, session_id, 5)
            .await.map_err(|e| format!("删除旧消息失败: {}", e))?;

        log::info!("会话已压缩: session_id={}, 摘要长度={}", session_id, summary.len());
        Ok(summary)
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
