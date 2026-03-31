//! XianZhu 本地应用主程序
//!
//! 基于 Tauri 框架的桌面应用入口
//! 提供 AI 代理的本地运行环境

#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]
#![allow(dead_code)]

mod agent;
mod backend_manager;
mod bridge;
mod channel;
mod channels;
mod config;
mod daemon;
mod db;
mod gateway;
mod handlers;
mod memory;
mod plugin_system;
mod routing;
mod runtime;
mod scheduler;
mod sop;
mod telemetry;

use std::sync::{Arc, Mutex};

// ─── 从 handlers 导入所有 tauri command 函数 ──────────────────
use handlers::providers::*;
use handlers::agents::*;
use handlers::sessions::*;
use handlers::channels_cmd::*;
use handlers::plaza::*;
use handlers::soul::*;
use handlers::mcp::*;
use handlers::skills::*;
use handlers::plugins::*;
use handlers::scheduler_cmd::*;
use handlers::misc::*;
use handlers::profile::*;
use handlers::oauth::*;

// ─── 从 helpers 导入 main() 使用的函数 ────────────────────────
use handlers::helpers::{load_providers, save_providers, seed_marketplace_skills};

/// 应用共享状态
///
/// 持有数据库连接和 Agent 编排器，通过 Tauri State 注入到 commands 中
pub(crate) struct AppState {
    pub db: db::Database,
    pub orchestrator: Arc<agent::Orchestrator>,
    pub scheduler: std::sync::OnceLock<scheduler::SchedulerManager>,
    pub channel_manager: std::sync::OnceLock<Arc<channels::manager::ChannelManager>>,
}

#[tokio::main]
async fn main() {
    // 初始化日志（同时输出到文件和 stderr）
    {
        use std::io::Write;

        // 日志文件路径：~/Library/Logs/XianZhu/xianzhu.log (macOS)
        // 其他平台降级到 ~/.xianzhu/logs/xianzhu.log
        let log_dir = if cfg!(target_os = "macos") {
            dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("Library/Logs/XianZhu")
        } else {
            dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".xianzhu/logs")
        };
        let _ = std::fs::create_dir_all(&log_dir);
        let log_path = log_dir.join("xianzhu.log");

        // 打开日志文件（追加模式），超过 10MB 时截断
        if log_path.exists() {
            if let Ok(meta) = std::fs::metadata(&log_path) {
                if meta.len() > 10 * 1024 * 1024 {
                    // 截断并写入 UTF-8 BOM（Windows 兼容）
                    let _ = std::fs::write(&log_path, "\u{feff}");
                }
            }
        }
        // 新文件写入 UTF-8 BOM，确保 Windows 记事本/PowerShell 正确识别编码
        if !log_path.exists() {
            let _ = std::fs::write(&log_path, "\u{feff}");
        }
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .ok();

        let log_file = std::sync::Arc::new(std::sync::Mutex::new(log_file));

        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
            .format(move |buf, record| {
                let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                let line = format!(
                    "[{} {} {}] {}\n",
                    ts,
                    record.level(),
                    record.target(),
                    record.args()
                );
                // 写到 stderr（终端调试时可见）
                let _ = buf.write_all(line.as_bytes());
                // 同时写到文件
                if let Ok(mut guard) = log_file.lock() {
                    if let Some(ref mut f) = *guard {
                        let _ = f.write_all(line.as_bytes());
                        let _ = f.flush();
                    }
                }
                Ok(())
            })
            .init();

        eprintln!("\u{1f4dd} 日志文件: {}", log_path.display());
    }

    // CLI 参数处理（在 Tauri 启动前）
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--install-service") {
        let exe = std::env::current_exe().unwrap_or_default();
        let mgr = daemon::ServiceManager::new(exe);
        match mgr.install() {
            Ok(msg) => { eprintln!("\u{2705} {}", msg); std::process::exit(0); }
            Err(e) => { eprintln!("\u{274c} {}", e); std::process::exit(1); }
        }
    }
    if args.iter().any(|a| a == "--uninstall-service") {
        let exe = std::env::current_exe().unwrap_or_default();
        let mgr = daemon::ServiceManager::new(exe);
        match mgr.uninstall() {
            Ok(msg) => { eprintln!("\u{2705} {}", msg); std::process::exit(0); }
            Err(e) => { eprintln!("\u{274c} {}", e); std::process::exit(1); }
        }
    }
    if args.iter().any(|a| a == "--service-status") {
        let exe = std::env::current_exe().unwrap_or_default();
        let mgr = daemon::ServiceManager::new(exe);
        eprintln!("服务已安装: {}", mgr.is_installed());
        std::process::exit(0);
    }

    // 记录启动开始时间
    let app_start_time = std::time::Instant::now();
    log::info!("\u{23f1}\u{fe0f}  启动 XianZhu 本地应用");

    // 统一配置加载
    let app_config = config::AppConfig::load(&config::AppConfig::default_path());
    log::info!("配置加载完成: data_dir={}", app_config.data_dir.display());

    // 初始化数据库
    let data_dir = &app_config.data_dir;
    std::fs::create_dir_all(data_dir).expect("无法创建数据目录");
    let db_path = data_dir.join("xianzhu.db");
    let db = match db::Database::new(db_path.to_str().unwrap_or("xianzhu.db")).await {
        Ok(db) => {
            log::info!("数据库初始化成功");
            db
        }
        Err(e) => {
            log::error!("数据库初始化失败: {}", e);
            return;
        }
    };

    // 检查 Node.js 运行时状态（启动时异步检查，不阻塞启动流程）
    tokio::spawn(async {
        let node_rt = runtime::NodeRuntime::new();
        match node_rt.status().await {
            runtime::node::RuntimeStatus::Ready { version, path } => {
                log::info!("Node.js 运行时就绪: {} ({})", version, path);
            }
            runtime::node::RuntimeStatus::NotInstalled => {
                log::warn!("Node.js 运行时未安装，技能工具可能无法执行。请通过设置页面安装。");
            }
            _ => {}
        }
    });

    // 自动从环境变量导入 API Key 到 provider 配置
    // 支持 OPENAI_API_KEY、ANTHROPIC_API_KEY、DEEPSEEK_API_KEY
    {
        let mut providers = load_providers(&db).await.unwrap_or_default();

        // 定义环境变量 → provider 映射
        let env_mappings = vec![
            (
                "OPENAI_API_KEY",
                "openai",
                "OpenAI",
                "openai",
                "https://api.openai.com/v1",
                vec![
                    ("gpt-4o-mini", "GPT-4o Mini"),
                    ("gpt-4o", "GPT-4o"),
                    ("gpt-4-turbo", "GPT-4 Turbo"),
                ],
            ),
            (
                "ANTHROPIC_API_KEY",
                "anthropic",
                "Anthropic",
                "anthropic",
                "https://api.anthropic.com/v1",
                vec![
                    ("claude-sonnet-4-20250514", "Claude Sonnet 4"),
                    ("claude-haiku-4-20250414", "Claude Haiku 4"),
                    ("claude-opus-4-20250514", "Claude Opus 4"),
                ],
            ),
            (
                "DEEPSEEK_API_KEY",
                "deepseek",
                "DeepSeek",
                "openai",
                "https://api.deepseek.com",
                vec![
                    ("deepseek-chat", "DeepSeek Chat"),
                    ("deepseek-reasoner", "DeepSeek Reasoner"),
                ],
            ),
        ];

        for (env_var, id, name, api_type, base_url, models) in &env_mappings {
            if let Ok(key) = std::env::var(env_var) {
                if key.is_empty() {
                    continue;
                }
                if let Some(existing) = providers.iter_mut().find(|p| p["id"].as_str() == Some(id)) {
                    if existing["apiKey"].as_str().map_or(true, |k| k.is_empty()) {
                        existing["apiKey"] = serde_json::Value::String(key.clone());
                        log::info!("已从环境变量 {} 导入 API Key 到 provider {}", env_var, id);
                    }
                } else {
                    let model_array: Vec<serde_json::Value> = models
                        .iter()
                        .map(|(mid, mname)| {
                            serde_json::json!({"id": mid, "name": mname})
                        })
                        .collect();
                    providers.push(serde_json::json!({
                        "id": id,
                        "name": name,
                        "apiType": api_type,
                        "baseUrl": base_url,
                        "apiKey": key,
                        "models": model_array,
                        "enabled": true,
                    }));
                    log::info!("已从环境变量 {} 创建 provider {}", env_var, id);
                }
            }
        }

        // 如果没有任何 provider，初始化默认列表（无 key）
        if providers.is_empty() {
            providers = vec![
                serde_json::json!({
                    "id": "openai", "name": "OpenAI", "apiType": "openai",
                    "baseUrl": "https://api.openai.com/v1", "apiKey": "",
                    "models": [{"id": "gpt-4o-mini", "name": "GPT-4o Mini"}, {"id": "gpt-4o", "name": "GPT-4o"}, {"id": "gpt-4-turbo", "name": "GPT-4 Turbo"}],
                    "enabled": true,
                }),
                serde_json::json!({
                    "id": "anthropic", "name": "Anthropic", "apiType": "anthropic",
                    "baseUrl": "https://api.anthropic.com/v1", "apiKey": "",
                    "models": [{"id": "claude-sonnet-4-20250514", "name": "Claude Sonnet 4"}, {"id": "claude-haiku-4-20250414", "name": "Claude Haiku 4"}, {"id": "claude-opus-4-20250514", "name": "Claude Opus 4"}],
                    "enabled": true,
                }),
                serde_json::json!({
                    "id": "deepseek", "name": "DeepSeek", "apiType": "openai",
                    "baseUrl": "https://api.deepseek.com", "apiKey": "",
                    "models": [{"id": "deepseek-chat", "name": "DeepSeek Chat"}, {"id": "deepseek-reasoner", "name": "DeepSeek Reasoner"}],
                    "enabled": true,
                }),
                serde_json::json!({
                    "id": "zhipu", "name": "智谱 AI (GLM)", "apiType": "openai",
                    "baseUrl": "https://open.bigmodel.cn/api/paas/v4", "apiKey": "",
                    "models": [
                        {"id": "glm-5", "name": "GLM-5"},
                        {"id": "glm-5-turbo", "name": "GLM-5 Turbo"},
                        {"id": "glm-4.7", "name": "GLM-4.7"},
                        {"id": "glm-4.7-flash", "name": "GLM-4.7 Flash"},
                        {"id": "glm-4.7-flashx", "name": "GLM-4.7 FlashX"},
                        {"id": "glm-4.6", "name": "GLM-4.6"},
                        {"id": "glm-z1", "name": "GLM-Z1 (推理)"}
                    ],
                    "enabled": true,
                }),
                serde_json::json!({
                    "id": "zhipu-coding", "name": "智谱 AI (CodePlan)", "apiType": "openai",
                    "baseUrl": "https://open.bigmodel.cn/api/coding/paas/v4", "apiKey": "",
                    "models": [
                        {"id": "glm-5", "name": "GLM-5"},
                        {"id": "glm-5-turbo", "name": "GLM-5 Turbo"},
                        {"id": "glm-4.7", "name": "GLM-4.7"},
                        {"id": "glm-4.7-flash", "name": "GLM-4.7 Flash"},
                        {"id": "glm-4.6", "name": "GLM-4.6"}
                    ],
                    "enabled": true,
                }),
            ];
        }

        let _ = save_providers(&db, &providers).await;
    }

    // 初始化记忆体系统
    let _memory_system = memory::MemorySystem::new(db.pool().clone());
    log::info!("记忆体系统初始化成功");

    // 初始化消息网关
    let _gateway = gateway::MessageGateway::new();
    log::info!("消息网关初始化成功");

    // 在 move db 之前克隆连接池，用于创建编排器
    let pool_clone = db.pool().clone();

    // 创建编排器
    let mut orchestrator = agent::Orchestrator::new(pool_clone.clone());
    log::info!("Agent 编排器初始化成功");

    // 创建调度器共享的 Notify（cron 工具和调度引擎共用）
    let scheduler_notify = std::sync::Arc::new(tokio::sync::Notify::new());

    // 注册 cron 工具到编排器
    {
        use scheduler::tools::*;
        let pool = pool_clone.clone();
        let notify = scheduler_notify.clone();
        orchestrator.tool_manager_mut().register_tool(Box::new(CronAddTool::new(pool.clone(), notify.clone())));
        orchestrator.tool_manager_mut().register_tool(Box::new(CronListTool::new(pool.clone())));
        orchestrator.tool_manager_mut().register_tool(Box::new(CronRemoveTool::new(pool.clone(), notify.clone())));
        orchestrator.tool_manager_mut().register_tool(Box::new(CronUpdateTool::new(pool.clone(), notify.clone())));
        orchestrator.tool_manager_mut().register_tool(Box::new(CronTriggerTool::new(pool.clone(), notify.clone())));
        log::info!("Cron 工具已注册到编排器");
    }

    // 包装编排器为 Arc（工具注册已完成）
    let orchestrator = Arc::new(orchestrator);

    // 注入 Orchestrator 到 DelegateTaskTool（解决循环依赖）
    agent::delegate::inject_orchestrator(orchestrator.clone());

    // 注册内置插件到 PluginManager
    if let Ok(mut pm) = orchestrator.plugin_manager.lock() {
        plugin_system::register_builtin_plugins(&mut pm, pool_clone.clone());
        log::info!("PluginManager: {} 个插件已加载", pm.list_plugins().len());
    }

    // 构建应用共享状态
    let app_state = Arc::new(AppState { db, orchestrator: orchestrator.clone(), scheduler: std::sync::OnceLock::new(), channel_manager: std::sync::OnceLock::new() });

    // 创建 BackendManager 并包装为可共享的引用
    let backend_manager = Arc::new(Mutex::new(backend_manager::BackendManager::new()));

    // 尝试启动本地后端进程（可选，后端可能在远程服务器上）
    {
        let mut bm = backend_manager.lock().unwrap_or_else(|e| e.into_inner());
        match bm.start().await {
            Ok(_) => {
                log::info!("Node.js 后端启动成功");
            }
            Err(e) => {
                log::warn!("本地后端未启动: {}（将使用远程后端）", e);
            }
        }
    }

    // 启动 API 网关（如果环境变量 XIANZHU_API_PORT 配置了端口）
    if let Ok(port_str) = std::env::var("XIANZHU_API_PORT") {
        if let Ok(port) = port_str.parse::<u16>() {
            let gw_config = gateway::api::ApiGatewayConfig {
                port,
                bind_address: std::env::var("XIANZHU_API_BIND").unwrap_or_else(|_| "127.0.0.1".to_string()),
                api_key: std::env::var("XIANZHU_API_KEY").ok(),
            };
            let gw_state = std::sync::Arc::new(gateway::api::GatewayState {
                config: gw_config,
                pool: pool_clone.clone(),
                orchestrator: Some(orchestrator.clone()),
                scheduler_notify: Some(scheduler_notify.clone()),
                webhook_rate_limiter: std::sync::Mutex::new(std::collections::HashMap::new()),
            });
            tokio::spawn(async move {
                if let Err(e) = gateway::api::start_api_gateway(gw_state).await {
                    log::error!("API 网关启动失败: {}", e);
                }
            });
        }
    }

    // 启动 Desktop Bridge（如果配置了云端 URL）
    {
        let bridge_pool = pool_clone.clone();
        let bridge_orch = orchestrator.clone();
        tokio::spawn(async move {
            let gateway_url: Option<String> = sqlx::query_scalar(
                "SELECT value FROM settings WHERE key = 'cloud_gateway_url'"
            ).fetch_optional(&bridge_pool).await.ok().flatten();

            let api_key: Option<String> = sqlx::query_scalar(
                "SELECT value FROM settings WHERE key = 'cloud_api_key'"
            ).fetch_optional(&bridge_pool).await.ok().flatten();

            if let (Some(url), Some(key)) = (gateway_url, api_key) {
                let url = url.trim().to_string();
                let key = key.trim().to_string();
                if !url.is_empty() && !key.is_empty() {
                    log::info!("Bridge: 配置已找到，连接 {}", url);

                    let agents = bridge_orch.list_agents().await.unwrap_or_default()
                        .into_iter().map(|a| a.id).collect::<Vec<_>>();
                    let tools = bridge_orch.tool_manager().get_tool_definitions()
                        .into_iter().map(|t| t.name).collect::<Vec<_>>();

                    let device_id = format!("desktop-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("x"));

                    let config = bridge::BridgeConfig {
                        gateway_url: url,
                        api_key: key,
                        device_id,
                        heartbeat_secs: 30,
                    };

                    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel();

                    let client = bridge::BridgeClient::new(config)
                        .with_agents(agents)
                        .with_capabilities(tools);
                    client.start(bridge_pool.clone(), bridge_orch.clone(), msg_tx).await;

                    let orch = bridge_orch.clone();
                    let pool = bridge_pool.clone();
                    tokio::spawn(async move {
                        while let Some(fwd) = msg_rx.recv().await {
                            let actual_agent_id = if fwd.agent_id == "default" {
                                orch.list_agents().await.ok()
                                    .and_then(|a| a.first().map(|x| x.id.clone()))
                                    .unwrap_or(fwd.agent_id.clone())
                            } else {
                                fwd.agent_id.clone()
                            };
                            log::info!("Bridge: 处理转发消息 agent={} session={}", actual_agent_id, fwd.session_id);
                            let providers_json: Option<String> = sqlx::query_scalar(
                                "SELECT value FROM settings WHERE key = 'providers'"
                            ).fetch_optional(&pool).await.ok().flatten();

                            if let Some(pj) = providers_json {
                                let providers: Vec<serde_json::Value> = serde_json::from_str(&pj).unwrap_or_default();
                                for p in &providers {
                                    if p["enabled"].as_bool() != Some(true) { continue; }
                                    let api_key = p["apiKey"].as_str().unwrap_or("");
                                    if api_key.is_empty() { continue; }
                                    let api_type = p["apiType"].as_str().unwrap_or("openai");
                                    let base_url = p["baseUrl"].as_str().unwrap_or("");
                                    let base_url_opt = if base_url.is_empty() { None } else { Some(base_url) };

                                    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                                    let orch_clone = orch.clone();
                                    let fwd_clone = fwd.clone();

                                    let _ = sqlx::query(
                                        "INSERT OR IGNORE INTO chat_sessions (id, agent_id, title, created_at) VALUES (?, ?, ?, ?)"
                                    )
                                    .bind(&fwd_clone.session_id)
                                    .bind(&actual_agent_id)
                                    .bind(format!("[{}] 转发", fwd_clone.sender_channel))
                                    .bind(chrono::Utc::now().timestamp_millis())
                                    .execute(&pool).await;

                                    match orch_clone.send_message_stream(
                                        &actual_agent_id, &fwd_clone.session_id, &fwd_clone.message,
                                        api_key, api_type, base_url_opt, tx, None,
                                    ).await {
                                        Ok(response) => {
                                            log::info!("Bridge: 转发消息处理完成 len={}", response.len());
                                        }
                                        Err(e) => {
                                            log::error!("Bridge: 转发消息处理失败: {}", e);
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                    });
                } else {
                    log::info!("Bridge: 云端配置为空，跳过连接");
                }
            } else {
                log::info!("Bridge: 未配置云端连接（设置 cloud_gateway_url 和 cloud_api_key 启用）");
            }
        });
    }

    // 释放内置技能到 marketplace（首次安装或 marketplace 为空时）
    seed_marketplace_skills();

    // 首次运行时从 Gemini CLI 提取 OAuth credentials 并缓存
    tokio::spawn(async { handlers::oauth::seed_oauth_credentials().await });

    // 初始化遥测模块并启动心跳（后台任务，不阻塞启动）
    telemetry::init(pool_clone.clone());
    telemetry::start_heartbeat(pool_clone.clone());
    log::info!("遥测心跳已启动");

    // 记录初始化完成时间
    let init_elapsed = app_start_time.elapsed();
    log::info!(
        "\u{2713} 应用初始化完成，耗时: {:.2}s",
        init_elapsed.as_secs_f64()
    );

    // 构建 Tauri 应用，注册 commands 和共享状态
    let orchestrator_for_setup = orchestrator.clone();
    let pool_for_setup = pool_clone.clone();
    let notify_for_setup = scheduler_notify.clone();
    let app_state_for_setup = app_state.clone();

    let app = tauri::Builder::default()
        .manage(app_state.clone())
        .setup(move |app| {
            let handle = app.handle().clone();
            let sched = scheduler::SchedulerManager::start(
                pool_for_setup.clone(),
                notify_for_setup,
                orchestrator_for_setup,
                handle,
            );
            let _ = app_state_for_setup.scheduler.set(sched);
            log::info!("\u{2713} 调度引擎已启动");

            let pool_for_seed = pool_for_setup.clone();
            tokio::spawn(async move {
                if let Err(e) = scheduler::seed::seed_default_jobs(&pool_for_seed).await {
                    log::warn!("种子任务注入失败: {}", e);
                }
            });

            {
                let mgr = Arc::new(channels::manager::ChannelManager::new(
                    pool_for_setup.clone(),
                    app_state_for_setup.orchestrator.clone(),
                    app.handle().clone(),
                ));
                let _ = app_state_for_setup.channel_manager.set(mgr.clone());
                let mgr_clone = mgr.clone();
                let mgr_health = mgr.clone();
                tokio::spawn(async move {
                    mgr_clone.start_all().await;
                    log::info!("ChannelManager: {} 个频道实例已启动", mgr_clone.running_count());
                    // 启动定时健康检查（每 30 秒）
                    mgr_health.start_health_check();
                });
            }

            // 桥接 EventBroadcaster → Tauri 事件（让前端能收到 agent-event）
            {
                use tauri::Manager;
                let mut rx = app_state_for_setup.orchestrator.event_broadcaster.subscribe();
                let handle = app.handle().clone();
                tokio::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(event) => {
                                let _ = handle.emit_all("agent-event", &event);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                log::warn!("EventBroadcaster: 丢失 {} 个事件（通道积压）", n);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                log::warn!("EventBroadcaster: 广播通道已关闭");
                                break;
                            }
                        }
                    }
                });
                log::info!("\u{2713} EventBroadcaster → Tauri 事件桥已启动");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // providers
            save_config, get_config, get_providers, save_provider, delete_provider,
            get_api_status, test_provider_connection,
            // agents
            create_agent, list_agents, list_agents_with_stats, delete_agent, update_agent, get_agent_detail,
            ai_generate_agent_config, get_audit_log, get_autonomy_config, update_autonomy_config,
            get_agent_relations, create_agent_relation, delete_agent_relation,
            list_subagents, cancel_subagent, list_subagent_runs,
            approve_tool_call, deny_tool_call, send_agent_message, get_agent_mailbox,
            export_agent_bundle, import_agent_bundle, list_agent_templates,
            // sessions
            send_message, send_chat_only, stop_generation, get_conversations, get_session_messages,
            load_structured_messages, clear_history, create_session, list_sessions,
            rename_session, delete_session, compact_session, cleanup_system_sessions,
            search_messages, export_session_history, get_context_usage, submit_message_feedback,
            edit_message, regenerate_response, transcribe_audio, transcribe_audio_file,
            start_voice_recording, stop_voice_recording,
            // channels
            create_agent_channel, list_agent_channels, delete_agent_channel, toggle_agent_channel,
            weixin_get_qrcode, weixin_poll_status, weixin_save_token,
            verify_telegram_token, discord_connect, slack_connect, send_poll,
            // plaza
            plaza_create_post, plaza_list_posts, plaza_add_comment, plaza_get_comments, plaza_like_post,
            // soul & tools
            read_soul_file, write_soul_file, list_soul_files,
            read_standing_orders, write_standing_orders,
            get_agent_tools, set_agent_tool_profile, set_agent_tool_override,
            // mcp
            list_mcp_servers, add_mcp_server, remove_mcp_server, toggle_mcp_server,
            import_claude_mcp_config, test_mcp_connection,
            // skills
            install_skill, remove_skill, list_skills, toggle_skill,
            list_marketplace_skills, download_skill_from_hub, search_skill_hub,
            publish_skill_to_hub, install_skill_to_agent, uninstall_skill_from_agent,
            clawhub_featured, clawhub_categories, clawhub_install,
            // plugins
            list_plugins, list_plugin_capabilities, list_system_plugins,
            toggle_system_plugin, save_plugin_config, get_plugin_config,
            get_agent_plugin_states, set_agent_plugin, import_external_plugin,
            // scheduler
            create_cron_job, update_cron_job, delete_cron_job, list_cron_jobs,
            get_cron_job, trigger_cron_job, pause_cron_job, resume_cron_job,
            list_cron_runs, get_scheduler_status,
            // misc
            save_chat_image, send_notification, backup_database, restore_database,
            estimate_token_cost, list_hooks, sop_list, sop_trigger, sop_runs,
            run_doctor, doctor_auto_fix, detect_browsers, open_in_browser,
            check_runtime, setup_runtime, health_check, get_token_stats, get_token_daily_stats,
            run_memory_hygiene, get_cache_stats, get_setting, set_setting, get_settings_by_prefix,
            export_memory_snapshot, extract_memories_from_history, run_learner, cloud_api_proxy,
            // profile
            get_user_profile, save_user_profile, save_user_avatar, get_user_avatar,
            // oauth
            start_oauth_flow, exchange_oauth_code, refresh_oauth_token,
        ])
        .build(tauri::generate_context!())
        .expect("error building tauri application");

    // 在应用事件循环中处理退出
    let backend_manager_clone = backend_manager.clone();
    let app_state_clone = app_state.clone();
    app.run(move |_app_handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            api.prevent_exit();

            if let Some(sched) = app_state_clone.scheduler.get() {
                sched.shutdown();
                log::info!("\u{2713} 调度引擎已关闭");
            }

            // 清理编排器：取消所有活跃会话
            app_state_clone.orchestrator.cancel_all_sessions();
            log::info!("\u{2713} 编排器已清理");

            // 关闭 ChannelManager
            if let Some(cm) = app_state_clone.channel_manager.get() {
                cm.stop_all();
                log::info!("\u{2713} 频道管理器已关闭");
            }

            let backend_manager_clone = backend_manager_clone.clone();
            let db_pool = app_state_clone.db.pool().clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async {
                    if let Ok(mut bm) = backend_manager_clone.lock() {
                        log::info!("应用关闭，停止后端进程...");
                        bm.stop().await;
                        log::info!("\u{2713} 后端进程已停止");
                    }
                    // 优雅关闭数据库连接池
                    db_pool.close().await;
                    log::info!("\u{2713} 数据库连接池已关闭");
                    std::process::exit(0);
                });
            });
        }
    });
}
