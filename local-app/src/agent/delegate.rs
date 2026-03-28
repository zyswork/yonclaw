//! 子代理委托执行（v3）
//!
//! 核心改进：
//! - 子代理走完整 agent_loop（通过 Orchestrator.send_message_stream）
//! - 有独立 session，工具实际可执行
//! - allowed_tools 通过 Agent 的 TOOLS.md profile 控制
//! - 执行结果持久化到 subagent_runs 表
//! - 支持异步模式
//!
//! 作为内置工具 `delegate_task` 注册到 ToolManager。

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::tools::{Tool, ToolDefinition, ToolSafetyLevel};

/// 默认最大并发子代理数
const DEFAULT_MAX_CONCURRENT: usize = 3;

/// 默认最大嵌套深度
const DEFAULT_MAX_DEPTH: u32 = 3;

/// 默认子代理超时（秒）
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// 运行时注入的 Orchestrator（解决循环依赖）
static ORCHESTRATOR: tokio::sync::OnceCell<Arc<super::orchestrator::Orchestrator>> =
    tokio::sync::OnceCell::const_new();

/// 注入 Orchestrator 引用（在 Orchestrator::new 完成后调用）
pub fn inject_orchestrator(orch: Arc<super::orchestrator::Orchestrator>) {
    let _ = ORCHESTRATOR.set(orch);
}

/// 获取 Orchestrator 引用
fn get_orchestrator() -> Result<&'static Arc<super::orchestrator::Orchestrator>, String> {
    ORCHESTRATOR.get().ok_or_else(|| "Orchestrator 未初始化".to_string())
}

/// 委托执行工具
pub struct DelegateTaskTool {
    pool: sqlx::SqlitePool,
    /// 当前嵌套深度（0=顶层）
    depth: u32,
    /// 事件广播器
    event_broadcaster: Arc<super::observer::EventBroadcaster>,
}

impl DelegateTaskTool {
    pub fn new(pool: sqlx::SqlitePool, broadcaster: Arc<super::observer::EventBroadcaster>) -> Self {
        Self { pool, depth: 0, event_broadcaster: broadcaster }
    }

    /// 持久化子代理运行记录
    async fn save_run(
        pool: &sqlx::SqlitePool,
        run_id: &str,
        parent_agent_id: &str,
        parent_session_id: Option<&str>,
        task_index: usize,
        goal: &str,
        context: &str,
        model: &str,
        allowed_tools: Option<&[String]>,
        depth: u32,
    ) {
        let now = chrono::Utc::now().timestamp_millis();
        let tools_json = allowed_tools.map(|t| serde_json::to_string(t).unwrap_or_default());
        if let Err(e) = sqlx::query(
            "INSERT INTO subagent_runs (id, parent_agent_id, parent_session_id, task_index, goal, context, model, status, depth, allowed_tools, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'running', ?, ?, ?)"
        )
        .bind(run_id)
        .bind(parent_agent_id)
        .bind(parent_session_id)
        .bind(task_index as i64)
        .bind(goal)
        .bind(if context.is_empty() { None } else { Some(context) })
        .bind(model)
        .bind(depth as i64)
        .bind(&tools_json)
        .bind(now)
        .execute(pool)
        .await {
            log::error!("save_run 失败: {}", e);
        }
    }

    /// 更新子代理运行结果
    async fn finish_run(
        pool: &sqlx::SqlitePool,
        run_id: &str,
        status: &str,
        result: Option<&str>,
        error: Option<&str>,
        duration_ms: i64,
    ) {
        let now = chrono::Utc::now().timestamp_millis();
        if let Err(e) = sqlx::query(
            "UPDATE subagent_runs SET status = ?, result = ?, error = ?, duration_ms = ?, finished_at = ? WHERE id = ?"
        )
        .bind(status)
        .bind(result)
        .bind(error)
        .bind(duration_ms)
        .bind(now)
        .bind(run_id)
        .execute(pool)
        .await {
            log::error!("finish_run 失败: {}", e);
        }
    }

    /// 执行单个子任务（通过 Orchestrator 走完整 agent_loop）
    async fn execute_subtask(
        pool: sqlx::SqlitePool,
        parent_agent_id: String,
        run_id: String,
        goal: String,
        context: String,
        model: String,
        _allowed_tools: Option<Vec<String>>,
        timeout: u64,
        task_index: usize,
    ) -> (usize, String, Result<String, String>) {
        let start = std::time::Instant::now();
        let goal_preview: String = goal.chars().take(50).collect();
        log::info!("子代理 #{}: 开始执行「{}」", task_index + 1, goal_preview);

        // 获取 Orchestrator
        let orchestrator = match get_orchestrator() {
            Ok(o) => o,
            Err(e) => {
                Self::finish_run(&pool, &run_id, "failed", None, Some(&e), 0).await;
                return (task_index, goal, Err(e));
            }
        };

        // 查找 provider
        let (api_type, api_key, base_url) = match crate::channels::find_provider(&pool, &model).await {
            Some(p) => p,
            None => {
                let e = format!("未找到模型 {} 的 provider", model);
                Self::finish_run(&pool, &run_id, "failed", None, Some(&e), 0).await;
                return (task_index, goal, Err(e));
            }
        };

        // 创建/复用子代理 session
        let session_title = format!("[subagent] {}", &goal_preview);
        let session_id = match crate::memory::conversation::create_session(
            &pool, &parent_agent_id, &session_title,
        ).await {
            Ok(s) => s.id,
            Err(e) => {
                let e_str = format!("创建 session 失败: {}", e);
                Self::finish_run(&pool, &run_id, "failed", None, Some(&e_str), 0).await;
                return (task_index, goal, Err(e_str));
            }
        };

        // 构建 prompt
        let prompt = if context.is_empty() {
            goal.clone()
        } else {
            format!("背景：{}\n\n任务：{}", context, goal)
        };

        // 收集输出
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let output_handle = tokio::spawn(async move {
            let mut output = String::new();
            while let Some(token) = rx.recv().await {
                output.push_str(&token);
            }
            output
        });

        let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };

        // 带超时调用完整 agent_loop
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(timeout),
            orchestrator.send_message_stream(
                &parent_agent_id, &session_id, &prompt,
                &api_key, &api_type, base_url_opt, tx, None,
            ),
        ).await {
            Ok(Ok(_)) => {
                let output = output_handle.await.unwrap_or_default();
                let elapsed = start.elapsed().as_millis() as i64;
                log::info!("子代理 #{}: 完成（{}字符, {}ms）", task_index + 1, output.len(), elapsed);
                Self::finish_run(&pool, &run_id, "success", Some(&output), None, elapsed).await;
                Ok(output)
            }
            Ok(Err(e)) => {
                let elapsed = start.elapsed().as_millis() as i64;
                log::warn!("子代理 #{}: 失败: {}", task_index + 1, e);
                Self::finish_run(&pool, &run_id, "failed", None, Some(&e), elapsed).await;
                Err(e)
            }
            Err(_) => {
                let elapsed = start.elapsed().as_millis() as i64;
                log::warn!("子代理 #{}: 超时（{}秒）", task_index + 1, timeout);
                let e = format!("执行超时（{}秒）", timeout);
                Self::finish_run(&pool, &run_id, "timeout", None, Some(&e), elapsed).await;
                Err(e)
            }
        };

        (task_index, goal, result)
    }

    /// 收集所有子代理的执行结果
    async fn collect_results(
        handles: Vec<tokio::task::JoinHandle<(usize, String, Result<String, String>)>>,
    ) -> (usize, usize, String) {
        let mut results = Vec::new();
        let mut success_count = 0;
        let mut fail_count = 0;

        for handle in handles {
            if let Ok((i, goal, result)) = handle.await {
                let goal_preview: String = goal.chars().take(30).collect();
                match result {
                    Ok(content) => {
                        success_count += 1;
                        results.push(format!("### 任务 {}：{}\n\n{}", i + 1, goal_preview, content));
                    }
                    Err(e) => {
                        fail_count += 1;
                        results.push(format!("### 任务 {}：{}\n\n❌ 失败：{}", i + 1, goal_preview, e));
                    }
                }
            }
        }

        let status = if fail_count == 0 {
            format!("全部 {} 个任务成功", success_count)
        } else {
            format!("{} 成功，{} 失败", success_count, fail_count)
        };

        (success_count, fail_count, format!("# 子代理执行结果\n\n{}\n\n{}", status, results.join("\n\n---\n\n")))
    }
}

#[async_trait]
impl Tool for DelegateTaskTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "delegate_task".to_string(),
            description: "委托子任务给并行子代理执行。每个子代理走完整工具循环，可使用父 Agent 的全部工具。支持独立模型、可配超时、异步模式。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "array",
                        "description": "任务列表",
                        "items": {
                            "type": "object",
                            "properties": {
                                "goal": { "type": "string", "description": "任务目标" },
                                "context": { "type": "string", "description": "上下文信息（可选）" },
                                "timeout_secs": { "type": "integer", "description": "单任务超时秒数（可选，默认120）" }
                            },
                            "required": ["goal"]
                        }
                    },
                    "model": { "type": "string", "description": "子代理使用的模型（可选），不填则用默认" },
                    "max_concurrent": { "type": "integer", "description": "最大并发数（可选，默认3，最大6）" },
                    "max_depth": { "type": "integer", "description": "最大嵌套深度（可选，默认3）" },
                    "async_mode": { "type": "boolean", "description": "异步模式（可选）。true 时立即返回，后台执行完成后通过事件通知" }
                },
                "required": ["tasks"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Guarded
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let tasks = arguments.get("tasks")
            .and_then(|t| t.as_array())
            .ok_or("缺少 tasks 数组")?;

        if tasks.is_empty() {
            return Err("任务列表不能为空".to_string());
        }

        // 深度检查
        let max_depth = arguments.get("max_depth")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(DEFAULT_MAX_DEPTH);

        if self.depth >= max_depth {
            return Err(format!(
                "委托深度已达上限（{}/{}），不能继续嵌套委托",
                self.depth, max_depth
            ));
        }

        let max_concurrent = arguments.get("max_concurrent")
            .and_then(|v| v.as_u64())
            .map(|v| (v as usize).min(6))
            .unwrap_or(DEFAULT_MAX_CONCURRENT);

        if tasks.len() > max_concurrent * 3 {
            return Err(format!("最多同时 {} 个任务", max_concurrent * 3));
        }

        let async_mode = arguments.get("async_mode").and_then(|v| v.as_bool()).unwrap_or(false);

        // 从注入的上下文中获取父 Agent 信息（由 agent_loop 注入到 arguments 中）
        let parent_agent_id = arguments.get("_parent_agent_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let parent_session_id = arguments.get("_parent_session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if parent_agent_id.is_empty() {
            log::warn!("delegate_task: 无法获取父 Agent 上下文（_parent_agent_id 为空）");
        }

        // 模型选择：优先使用调用方显式指定的，否则继承父 Agent 的模型配置
        let model_str = if let Some(m) = arguments["model"].as_str().filter(|s| !s.is_empty()) {
            m.to_string()
        } else if !parent_agent_id.is_empty() {
            match sqlx::query_scalar::<_, String>("SELECT model FROM agents WHERE id = ?")
                .bind(&parent_agent_id)
                .fetch_optional(&self.pool)
                .await {
                Ok(Some(m)) if !m.is_empty() => {
                    log::info!("delegate_task: 继承父 Agent 模型 {}", m);
                    m
                }
                _ => "gpt-4o-mini".to_string(),
            }
        } else {
            "gpt-4o-mini".to_string()
        };

        let batch_id = uuid::Uuid::new_v4().to_string();
        log::info!(
            "delegate_task: {} 个子任务，深度 {}/{}，并发 {}，模型 {}，异步={}，父agent={}",
            tasks.len(), self.depth, max_depth, max_concurrent, model_str, async_mode,
            if parent_agent_id.is_empty() { "(unknown)" } else { &parent_agent_id }
        );

        // 并行执行（限制并发数）
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
        let mut handles = Vec::new();

        for (i, task) in tasks.iter().enumerate() {
            let goal = task["goal"].as_str().unwrap_or("").to_string();
            let context = task["context"].as_str().unwrap_or("").to_string();
            if goal.is_empty() { continue; }

            let timeout = task.get("timeout_secs")
                .and_then(|v| v.as_u64())
                .unwrap_or(DEFAULT_TIMEOUT_SECS);

            let run_id = uuid::Uuid::new_v4().to_string();
            Self::save_run(
                &self.pool, &run_id, &parent_agent_id,
                Some(&parent_session_id), i,
                &goal, &context, &model_str, None, self.depth,
            ).await;

            let sem = semaphore.clone();
            let pool = self.pool.clone();
            let parent = parent_agent_id.clone();
            let model = model_str.clone();

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                Self::execute_subtask(
                    pool, parent, run_id, goal, context, model, None, timeout, i,
                ).await
            });

            handles.push(handle);
        }

        // 发出子代理派发事件
        self.event_broadcaster.emit(super::observer::AgentEvent::SubagentSpawned {
            batch_id: batch_id.clone(),
            parent_agent_id: parent_agent_id.clone(),
            task_count: handles.len(),
            model: model_str.clone(),
        });

        // 异步模式
        if async_mode {
            let broadcaster = self.event_broadcaster.clone();
            let batch_id_clone = batch_id.clone();
            let parent_agent = parent_agent_id.clone();
            let parent_session = parent_session_id.clone();
            tokio::spawn(async move {
                let (success_count, fail_count, summary) = Self::collect_results(handles).await;
                broadcaster.emit(super::observer::AgentEvent::SubagentComplete {
                    batch_id: batch_id_clone,
                    parent_agent_id: parent_agent,
                    parent_session_id: Some(parent_session),
                    success_count,
                    fail_count,
                    summary,
                });
            });
            return Ok(format!(
                "已异步派发 {} 个子任务（batch_id: {}）。完成后会通过事件通知。",
                tasks.len(), batch_id
            ));
        }

        // 同步模式
        let (_, _, summary) = Self::collect_results(handles).await;
        Ok(summary)
    }
}
