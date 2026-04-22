//! 任务执行引擎：Shell / Agent / MCP，超时 + 重试 + 输出截断

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use super::types::*;

/// 执行结果
pub enum ExecResult {
    Success { output: String },
    Failed { error: String },
    Timeout,
}

pub struct JobRunner {
    pool: sqlx::SqlitePool,
    orchestrator: Arc<crate::agent::Orchestrator>,
}

impl JobRunner {
    pub fn new(
        pool: sqlx::SqlitePool,
        orchestrator: Arc<crate::agent::Orchestrator>,
    ) -> Self {
        Self { pool, orchestrator }
    }

    /// 执行任务（带超时）
    pub async fn execute(&self, job: &CronJob) -> ExecResult {
        match tokio::time::timeout(
            Duration::from_secs(job.timeout_secs as u64),
            self.execute_inner(job),
        ).await {
            Ok(result) => result,
            Err(_) => ExecResult::Timeout,
        }
    }

    async fn execute_inner(&self, job: &CronJob) -> ExecResult {
        match &job.action_payload {
            ActionPayload::Agent { prompt, session_strategy, model, thinking } => {
                self.execute_agent(job, prompt, session_strategy, model.as_deref(), thinking.as_deref()).await
            }
            ActionPayload::Shell { command } => {
                self.execute_shell(command).await
            }
            ActionPayload::McpTool { server_name, tool_name, args } => {
                self.execute_mcp(job, server_name, tool_name, args).await
            }
            ActionPayload::Dreaming { phase } => {
                self.execute_dreaming(job, phase).await
            }
        }
    }

    /// Dreaming 记忆整理：直接调 agent::dreaming::run_dream_phase
    async fn execute_dreaming(&self, job: &CronJob, phase_str: &str) -> ExecResult {
        let agent_id = match &job.agent_id {
            Some(id) => id.clone(),
            None => return ExecResult::Failed { error: "Dreaming 任务缺少 agent_id".to_string() },
        };

        let phase = match crate::agent::dreaming::DreamPhase::from_str(phase_str) {
            Ok(p) => p,
            Err(e) => return ExecResult::Failed { error: e },
        };

        // 查 agent 的 workspace 和 model（DB 错误必须暴露）
        let row: Option<(String, Option<String>)> = match sqlx::query_as(
            "SELECT model, workspace_path FROM agents WHERE id = ?"
        )
        .bind(&agent_id)
        .fetch_optional(&self.pool)
        .await {
            Ok(r) => r,
            Err(e) => return ExecResult::Failed { error: format!("查询 Agent 失败: {}", e) },
        };
        let (agent_model, workspace) = match row {
            Some((m, Some(wp))) if !wp.is_empty() => (m, wp),
            Some(_) => return ExecResult::Failed { error: format!("Agent {} 无 workspace", agent_id) },
            None => return ExecResult::Failed { error: format!("Agent {} 不存在", agent_id) },
        };

        // 加载 provider 配置
        let (api_type, api_key, base_url) = match self.load_provider(&agent_model).await {
            Ok(p) => p,
            Err(e) => return ExecResult::Failed { error: e },
        };

        let llm_config = crate::agent::llm::LlmConfig {
            provider: api_type,
            api_key,
            model: agent_model.clone(),
            base_url: if base_url.is_empty() { None } else { Some(base_url) },
            temperature: None,
            max_tokens: None,
            thinking_level: None,
        };

        match crate::agent::dreaming::run_dream_phase(
            &self.pool, &agent_id, &workspace, phase, &llm_config,
        ).await {
            Ok((phase_name, path, summary)) => {
                let output = format!(
                    "Dreaming {} 完成\n路径: {}\n\n摘要:\n{}",
                    phase_name, path.display(), summary
                );
                let (truncated, _) = truncate_output(&output);
                ExecResult::Success { output: truncated }
            }
            // 正常跳过（无对话/无新观察）记为 Success 避免污染失败率
            Err(e) if e.contains("最近无对话") || e.contains("本次无新观察") => {
                ExecResult::Success { output: format!("跳过: {}", e) }
            }
            Err(e) => ExecResult::Failed { error: e },
        }
    }

    /// Agent 任务：调用 Orchestrator
    async fn execute_agent(&self, job: &CronJob, prompt: &str, _session_strategy: &str, model_override: Option<&str>, _thinking_override: Option<&str>) -> ExecResult {
        let agent_id = match &job.agent_id {
            Some(id) => id.clone(),
            None => return ExecResult::Failed { error: "Agent 任务缺少 agent_id".to_string() },
        };

        // 查找模型：优先用 payload 指定的 model，否则用 Agent 默认
        let agent_model = if let Some(m) = model_override {
            log::info!("Cron 任务 {} 使用覆盖模型: {}", job.name, m);
            m.to_string()
        } else {
            match self.orchestrator.list_agents().await {
                Ok(agents) => match agents.into_iter().find(|a| a.id == agent_id) {
                    Some(a) => a.model,
                    None => return ExecResult::Failed { error: format!("Agent {} 不存在", agent_id) },
                },
                Err(e) => return ExecResult::Failed { error: e },
            }
        };

        // 查找 provider 配置
        let (api_type, api_key, base_url) = match self.load_provider(&agent_model).await {
            Ok(p) => p,
            Err(e) => return ExecResult::Failed { error: e },
        };

        // 复用 cron session（每个 job 一个，避免会话列表膨胀）
        let cron_session_title = format!("[cron] {}", job.name);
        let session_id = {
            // 查找已有的 cron session
            let existing: Option<(String,)> = sqlx::query_as(
                "SELECT id FROM chat_sessions WHERE agent_id = ? AND title = ? LIMIT 1"
            )
            .bind(&agent_id).bind(&cron_session_title)
            .fetch_optional(&self.pool).await.ok().flatten();

            if let Some((id,)) = existing {
                // 更新 last_message_at
                let _ = sqlx::query("UPDATE chat_sessions SET last_message_at = ? WHERE id = ?")
                    .bind(chrono::Utc::now().timestamp_millis())
                    .bind(&id)
                    .execute(&self.pool).await;
                id
            } else {
                // 首次执行，创建新 session
                match crate::memory::conversation::create_session(
                    &self.pool, &agent_id, &cron_session_title,
                ).await {
                    Ok(s) => s.id,
                    Err(e) => return ExecResult::Failed { error: format!("创建 session 失败: {}", e) },
                }
            }
        };

        // 收集 LLM 输出
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let output_handle = tokio::spawn(async move {
            let mut output = String::new();
            while let Some(token) = rx.recv().await {
                output.push_str(&token);
            }
            output
        });

        let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };

        // 注入 Focus Items 上下文（如果 Agent 有 FOCUS.md）
        let enriched_prompt = {
            let wp: Option<String> = sqlx::query_scalar(
                "SELECT workspace_path FROM agents WHERE id = ?"
            ).bind(&agent_id).fetch_optional(&self.pool).await.ok().flatten();

            if let Some(wp) = wp {
                let focus_path = std::path::PathBuf::from(&wp).join("FOCUS.md");
                if focus_path.exists() {
                    if let Ok(focus_content) = tokio::fs::read_to_string(&focus_path).await {
                        let active: Vec<&str> = focus_content.lines()
                            .filter(|l| l.trim().starts_with("- [ ]") || l.trim().starts_with("- [/]"))
                            .collect();
                        if !active.is_empty() {
                            format!("[当前焦点]\n{}\n\n[任务]\n{}", active.join("\n"), prompt)
                        } else {
                            prompt.to_string()
                        }
                    } else {
                        prompt.to_string()
                    }
                } else {
                    prompt.to_string()
                }
            } else {
                prompt.to_string()
            }
        };

        match self.orchestrator.send_message_stream(
            &agent_id, &session_id, &enriched_prompt,
            &api_key, &api_type, base_url_opt, tx, None,
        ).await {
            Ok(_) => {
                let output = output_handle.await.unwrap_or_default();
                let (truncated, _) = truncate_output(&output);
                ExecResult::Success { output: truncated }
            }
            Err(e) => ExecResult::Failed { error: e },
        }
    }

    /// Shell 命令执行
    async fn execute_shell(&self, command: &str) -> ExecResult {
        match tokio::process::Command::new("sh")
            .args(["-c", command])
            .output()
            .await
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if output.status.success() {
                    let combined = format!("{}{}", stdout, stderr);
                    let (truncated, _) = truncate_output(&combined);
                    ExecResult::Success { output: truncated }
                } else {
                    ExecResult::Failed {
                        error: format!("退出码 {}: {}", output.status.code().unwrap_or(-1), stderr)
                    }
                }
            }
            Err(e) => ExecResult::Failed { error: format!("执行失败: {}", e) },
        }
    }

    /// MCP 工具调用
    async fn execute_mcp(
        &self, job: &CronJob, server_name: &str, tool_name: &str, args: &serde_json::Value,
    ) -> ExecResult {
        let agent_id = job.agent_id.as_deref().unwrap_or("default");
        // 确保 MCP server 已启动
        if let Err(e) = self.orchestrator.mcp_manager().start_servers_for_agent(agent_id).await {
            return ExecResult::Failed { error: format!("启动 MCP 服务失败: {}", e) };
        }
        // MCP 工具名格式：server_name.tool_name
        let namespaced = format!("{}.{}", server_name, tool_name);
        match self.orchestrator.mcp_manager().call_tool(&namespaced, args.clone()).await {
            Ok(result) => {
                let (truncated, _) = truncate_output(&result);
                ExecResult::Success { output: truncated }
            }
            Err(e) => ExecResult::Failed { error: e },
        }
    }

    /// 带重试的执行
    pub async fn execute_with_retry(&self, job: &CronJob) -> (ExecResult, u32) {
        let max = job.retry.max_attempts;
        for attempt in 1..=(max + 1) {
            let result = self.execute(job).await;
            match &result {
                ExecResult::Success { .. } => return (result, attempt),
                _ if attempt <= max => {
                    let delay_ms = (job.retry.base_delay_ms as f64
                        * job.retry.backoff_factor.powi((attempt - 1) as i32)) as u64;
                    let delay_ms = delay_ms.min(600_000); // 上限 10 分钟
                    log::warn!("任务 {} 第 {} 次重试，等待 {}ms", job.name, attempt, delay_ms);
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
                _ => return (result, attempt),
            }
        }
        unreachable!()
    }

    /// 从 DB 加载 provider 配置
    async fn load_provider(&self, model: &str) -> Result<(String, String, String), String> {
        let json_str = sqlx::query_scalar::<_, Option<String>>(
            "SELECT value FROM settings WHERE key = 'providers'"
        )
        .fetch_one(&self.pool).await
        .map_err(|e| format!("查询 providers 失败: {}", e))?
        .unwrap_or_else(|| "[]".to_string());

        let providers: Vec<serde_json::Value> = serde_json::from_str(&json_str)
            .map_err(|e| format!("解析 providers 失败: {}", e))?;

        for p in &providers {
            if p["enabled"].as_bool() != Some(true) { continue; }
            if let Some(models) = p["models"].as_array() {
                for m in models {
                    if m["id"].as_str() == Some(model) {
                        let api_type = p["apiType"].as_str().unwrap_or("openai").to_string();
                        let api_key = p["apiKey"].as_str().unwrap_or("").to_string();
                        let base_url = p["baseUrl"].as_str().unwrap_or("").to_string();
                        return Ok((api_type, api_key, base_url));
                    }
                }
            }
        }
        Err(format!("未找到模型 {} 对应的供应商配置", model))
    }
}
