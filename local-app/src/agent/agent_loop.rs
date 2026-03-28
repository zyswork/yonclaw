//! Agent 工具调用循环
//!
//! 从 orchestrator.rs 提取的多轮工具执行核心循环。
//! 职责单一：接收消息列表 → 调 LLM → 执行工具 → 追加结果 → 重复。

use std::collections::{HashMap, HashSet};

use super::dispatcher::ToolDispatcher;
use super::llm::{LlmClient, LlmConfig, LlmResponse};
use super::tools::{Tool, ToolDefinition, ToolManager, ToolSafetyLevel};
use super::agent_store::estimate_cost;
use crate::memory;
use tauri::Manager;
use tokio::sync::mpsc;
use sqlx::SqlitePool;

/// 多轮工具调用最大轮数
pub const MAX_TOOL_ROUNDS: usize = 10;

/// 单次请求默认 token 预算上限
pub const DEFAULT_TOKEN_BUDGET: u64 = 100_000;

/// Agent loop 错误，携带已生成的部分回复
#[derive(Debug)]
pub struct AgentLoopError {
    pub message: String,
    pub partial_content: String,
}

/// Agent loop 返回结果（扩展 yield 状态）
#[derive(Debug)]
pub enum AgentLoopResult {
    /// 正常完成
    Done(String),
    /// Yield — Agent 暂停，等待子代理完成后重新注入结果
    Yielded {
        content: String,
        /// 等待的子代理 run_id
        waiting_for: Option<String>,
        /// yield 消息
        yield_message: String,
    },
}

/// Agent loop 运行所需的依赖（避免传整个 Orchestrator）
pub struct AgentLoopDeps<'a> {
    pub pool: &'a SqlitePool,
    pub tool_manager: &'a ToolManager,
    pub mcp_manager: &'a super::mcp_manager::McpManager,
    pub policy_engine: &'a std::sync::Mutex<super::tool_policy::ToolPolicyEngine>,
    pub event_broadcaster: &'a super::observer::EventBroadcaster,
    pub hook_runner: &'a std::sync::Mutex<super::hooks::HookRunner>,
    pub lifecycle: &'a super::lifecycle::LifecycleManager,
    pub agent_config: Option<String>,
    /// 模型提供商注册表（Phase 1: 可选，为空则用传统 LlmClient）
    pub provider_registry: Option<&'a crate::plugin_system::ProviderRegistry>,
    /// 进化引擎状态（跟踪工具调用次数）
    pub evolution_state: Option<&'a std::sync::Arc<super::self_evolution::EvolutionState>>,
    /// 工具审批管理器
    pub approval_manager: Option<&'a super::approval::ApprovalManager>,
    /// Tauri app handle（用于发送事件到前端）
    pub app_handle: Option<&'a tauri::AppHandle>,
}

/// 多轮工具调用循环
///
/// loop (max MAX_TOOL_ROUNDS):
///   1. call_stream → LlmResponse
///   2. if no tool_calls → break
///   3. for each tool_call: 检查 safety, 执行, 推送状态
///   4. format_tool_result → 追加到 messages
///   5. 继续循环
pub async fn run_agent_loop(
    deps: &AgentLoopDeps<'_>,
    config: &LlmConfig,
    mut messages: Vec<serde_json::Value>,
    system_prompt: Option<&str>,
    provider: &str,
    tx: &mpsc::UnboundedSender<String>,
    tool_defs: &[ToolDefinition],
    skill_tools: &HashMap<String, Box<dyn Tool>>,
    agent_id: &str,
    session_id: &str,
    cancel_token: &Option<tokio_util::sync::CancellationToken>,
    dispatcher: &dyn ToolDispatcher,
) -> Result<String, AgentLoopError> {
    // 尝试从 provider_registry 查找 provider（Phase 1: 优先 registry，fallback 到传统 LlmClient）
    let registry_provider = deps.provider_registry
        .and_then(|reg| reg.get(provider).or_else(|| reg.find_by_model(&config.model)));
    if let Some(p) = registry_provider {
        log::info!("使用 ProviderRegistry: {} ({})", p.display_name(), p.id());
    }
    let client = LlmClient::new(config.clone());
    let tools_opt = if tool_defs.is_empty() { None } else { Some(tool_defs) };

    // 响应缓存
    let response_cache = super::response_cache::ResponseCache::new(deps.pool.clone());
    let sys_prompt_str = system_prompt.unwrap_or("");
    let cache_key = super::response_cache::ResponseCache::cache_key(&config.model, sys_prompt_str, &messages);

    if tool_defs.is_empty() {
        if let Ok(Some(cached)) = response_cache.get(&cache_key).await {
            log::info!("响应缓存命中: key={}..., 长度={}", &cache_key[..16], cached.len());
            let _ = tx.send(cached.clone());
            return Ok(cached);
        }
    }

    let mut full_content = String::new();
    let mut accumulated_tokens: u64 = 0;
    let mut _accumulated_cost: f64 = 0.0;
    let mut empty_retries: u32 = 0;

    for round in 0..MAX_TOOL_ROUNDS {
        // 取消检查
        if let Some(ref token) = cancel_token {
            if token.is_cancelled() {
                log::info!("Agent loop 被用户取消（第 {} 轮）", round + 1);
                let _ = tx.send("\n\n⚠️ 已取消\n".to_string());
                break;
            }
        }
        // 每轮执行 ContextGuard（防止工具调用累积导致上下文爆炸）
        if round > 0 {
            let guard_config = super::context_guard::ContextGuardConfig::for_model(&config.model);
            // Hook: BeforeCompaction
            deps.lifecycle.notify(super::lifecycle::HookPoint::BeforeCompaction, &super::lifecycle::HookEvent {
                point: "before_compaction".to_string(),
                agent_id: agent_id.to_string(), session_id: session_id.to_string(),
                payload: serde_json::json!({ "message_count": messages.len(), "round": round }),
            }).await;

            // 先尝试智能摘要压缩（用 LLM 生成中间对话摘要）
            let guard_config_clone = guard_config.clone();
            if let Some(summary) = super::context_guard::compress_with_summary(
                &mut messages, &guard_config_clone, config,
            ).await {
                log::info!("智能压缩(round {}): 已生成摘要（{}字符），消息数={}",
                    round + 1, summary.len(), messages.len());
                // 通知前端压缩正在进行
                let _ = tx.send("\n[上下文已压缩，对话继续...]\n".to_string());
            }
            // 摘要后仍超预算则用传统方式截断
            let guard_result = super::context_guard::enforce(&guard_config, &mut messages);
            if guard_result.modified {
                log::info!("ContextGuard(round {}): {}→{} tokens, removed={}, compacted={}",
                    round + 1, guard_result.tokens_before, guard_result.tokens_after,
                    guard_result.removed, guard_result.compacted);
                deps.event_broadcaster.emit(super::observer::AgentEvent::ContextCompact {
                    original_count: guard_result.tokens_before,
                    compacted_count: guard_result.tokens_after,
                    tier: format!("round_{}", round + 1),
                });
                // Hook: AfterCompaction
                deps.lifecycle.notify(super::lifecycle::HookPoint::AfterCompaction, &super::lifecycle::HookEvent {
                    point: "after_compaction".to_string(),
                    agent_id: agent_id.to_string(), session_id: session_id.to_string(),
                    payload: serde_json::json!({
                        "tokens_before": guard_result.tokens_before,
                        "tokens_after": guard_result.tokens_after,
                        "removed": guard_result.removed,
                    }),
                }).await;
            }
        }
        log::info!("Agent loop 第 {} 轮, messages 数量: {}", round + 1, messages.len());

        // token channel
        let (round_tx, mut round_rx) = mpsc::unbounded_channel::<String>();
        let round_buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let round_buf_clone = round_buf.clone();
        let tx_clone = tx.clone();
        let forward_handle = tokio::spawn(async move {
            while let Some(token) = round_rx.recv().await {
                if let Ok(mut buf) = round_buf_clone.lock() { buf.push_str(&token); }
                let _ = tx_clone.send(token);
            }
        });

        if round > 0 {
            let _ = tx.send("\n\n".to_string());
        }

        // 生命周期事件: BeforeLlmCall
        {
            let lc_event = super::lifecycle::HookEvent {
                point: "BeforeLlmCall".to_string(),
                agent_id: agent_id.to_string(),
                session_id: session_id.to_string(),
                payload: serde_json::json!({
                    "model": config.model, "message_count": messages.len(), "round": round,
                }),
            };
            deps.lifecycle.notify(super::lifecycle::HookPoint::BeforeLlmCall, &lc_event).await;
        }
        // 旧事件系统（兼容）
        deps.event_broadcaster.emit(super::observer::AgentEvent::LlmStart {
            model: config.model.clone(), message_count: messages.len(), round,
        });
        if let Ok(runner) = deps.hook_runner.lock() {
            let event = super::hooks::HookEvent::BeforeLlmCall {
                model: config.model.clone(), message_count: messages.len(), agent_id: agent_id.to_string(),
            };
            let _ = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(runner.emit(&event))
            });
        }

        let llm_start = std::time::Instant::now();
        // 通过 provider_registry 或 fallback 到传统 LlmClient
        let call_result = if let Some(p) = registry_provider {
            let call_config = crate::plugin_system::CallConfig {
                model: config.model.clone(),
                api_key: config.api_key.clone(),
                base_url: config.base_url.clone(),
                temperature: config.temperature.map(|t| t as f64),
                max_tokens: config.max_tokens.map(|m| m as u32),
            };
            p.call_stream(&call_config, &messages, system_prompt, tools_opt, round_tx).await
        } else {
            client.call_stream(&messages, system_prompt, tools_opt, round_tx).await
        };
        let llm_response = match call_result {
            Ok(resp) => resp,
            Err(e) => {
                // 检测是否是上下文溢出（自动 compact 并重试一次）
                let is_context_overflow = {
                    let el = e.to_lowercase();
                    el.contains("context_length") || el.contains("context length")
                        || el.contains("maximum context") || el.contains("too many tokens")
                        || el.contains("max_tokens") || el.contains("token limit")
                        || el.contains("reduce the length") || el.contains("请减少")
                };
                if is_context_overflow && round == 0 {
                    log::warn!("LLM 上下文溢出，尝试自动压缩...");
                    let _ = tx.send("\n⚙️ Context overflow — auto-compacting...\n".to_string());
                    // 触发自动压缩（设置 boundary 到当前消息数的一半）
                    let msg_count: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM chat_messages WHERE session_id = ?"
                    ).bind(session_id).fetch_one(deps.pool).await.unwrap_or(0);
                    if msg_count > 5 {
                        // 设置 boundary 为总消息数的 2/3 处
                        let boundary_target = msg_count * 2 / 3;
                        let boundary_seq: Option<i64> = sqlx::query_scalar(
                            "SELECT seq FROM chat_messages WHERE session_id = ? ORDER BY seq ASC LIMIT 1 OFFSET ?"
                        ).bind(session_id).bind(boundary_target).fetch_optional(deps.pool).await.ok().flatten();
                        if let Some(bseq) = boundary_seq {
                            // 快速摘要（不调 LLM，直接截断）
                            let compact_key = format!("compact_boundary_{}", session_id);
                            let _ = sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)")
                                .bind(&compact_key).bind(bseq.to_string())
                                .execute(deps.pool).await;
                            let _ = sqlx::query("UPDATE chat_sessions SET summary = ? WHERE id = ?")
                                .bind("[Auto-compacted due to context overflow]")
                                .bind(session_id).execute(deps.pool).await;
                            log::info!("自动压缩: boundary_seq={}, 继续重试", bseq);
                            let _ = tx.send("⚙️ Auto-compacted. Retrying...\n".to_string());
                            // 不直接重试（让循环继续会复杂），返回提示让用户重发
                        }
                    }
                }
                log::error!("LLM 调用失败（第 {} 轮）: {}", round + 1, e);
                let _ = tx.send(format!("\n\n⚠️ LLM 调用出错: {}\n", e));
                let _ = forward_handle.await;
                let mut partial = full_content.clone();
                if let Ok(buf) = round_buf.lock() { partial.push_str(&buf); }
                return Err(AgentLoopError { message: e, partial_content: partial });
            }
        };
        forward_handle.await.map_err(|e| AgentLoopError {
            message: e.to_string(), partial_content: full_content.clone(),
        })?;

        let llm_duration_ms = llm_start.elapsed().as_millis() as u64;
        log::info!("LLM 响应: content_len={}, tool_calls={}, stop_reason='{}', duration={}ms",
            llm_response.content.len(), llm_response.tool_calls.len(), llm_response.stop_reason, llm_duration_ms);

        // 生命周期事件: AfterLlmCall
        {
            let (input, output) = llm_response.usage.as_ref().map(|u| (u.input_tokens, u.output_tokens)).unwrap_or((0, 0));
            let lc_event = super::lifecycle::HookEvent {
                point: "AfterLlmCall".to_string(),
                agent_id: agent_id.to_string(),
                session_id: session_id.to_string(),
                payload: serde_json::json!({
                    "model": config.model, "content_len": llm_response.content.len(),
                    "tool_call_count": llm_response.tool_calls.len(),
                    "usage": {"input_tokens": input, "output_tokens": output},
                    "duration_ms": llm_duration_ms,
                }),
            };
            deps.lifecycle.notify(super::lifecycle::HookPoint::AfterLlmCall, &lc_event).await;
        }
        // 旧事件系统（兼容）
        {
            let (input, output) = llm_response.usage.as_ref().map(|u| (u.input_tokens, u.output_tokens)).unwrap_or((0, 0));
            deps.event_broadcaster.emit(super::observer::AgentEvent::LlmDone {
                model: config.model.clone(), content_len: llm_response.content.len(),
                tool_call_count: llm_response.tool_calls.len(),
                input_tokens: input, output_tokens: output, duration_ms: llm_duration_ms,
            });
            if let Ok(runner) = deps.hook_runner.lock() {
                let event = super::hooks::HookEvent::AfterLlmCall {
                    model: config.model.clone(), content_len: llm_response.content.len(),
                    tool_call_count: llm_response.tool_calls.len(),
                    usage: Some((input, output)), agent_id: agent_id.to_string(),
                };
                let _ = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(runner.emit(&event))
                });
            }
        }

        // Token 统计 + 成本
        if let Some(ref usage) = llm_response.usage {
            accumulated_tokens += usage.total_tokens;
            _accumulated_cost += estimate_cost(&config.model, usage.input_tokens, usage.output_tokens);
            log::info!("Token: input={}, output={}, 累积={}/{}", usage.input_tokens, usage.output_tokens, accumulated_tokens, DEFAULT_TOKEN_BUDGET);

            // 异步写入 token_usage
            let pool = deps.pool.clone();
            let (aid, sid, model) = (agent_id.to_string(), session_id.to_string(), config.model.clone());
            let (inp, out, total) = (usage.input_tokens as i64, usage.output_tokens as i64, usage.total_tokens as i64);
            tokio::spawn(async move {
                let id = uuid::Uuid::new_v4().to_string();
                let now = chrono::Utc::now().timestamp_millis();
                let _ = sqlx::query(
                    "INSERT INTO token_usage (id, agent_id, session_id, model, input_tokens, output_tokens, total_tokens, cached_tokens, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?)"
                ).bind(&id).bind(&aid).bind(&sid).bind(&model).bind(inp).bind(out).bind(total).bind(now)
                .execute(&pool).await;
            });

            if accumulated_tokens > DEFAULT_TOKEN_BUDGET {
                log::warn!("Token 预算超限: {} > {}", accumulated_tokens, DEFAULT_TOKEN_BUDGET);
                let _ = tx.send(format!("\n\n⚠️ Token 消耗已达上限（{}）\n", accumulated_tokens));
                break;
            }
        }

        full_content.push_str(&llm_response.content);

        if !llm_response.has_tool_calls() {
            // 工具意图检测
            if round == 0 && !tool_defs.is_empty() && detect_tool_intent(&llm_response.content) {
                log::info!("检测到工具意图但未调用，注入 nudge");
                messages.push(serde_json::json!({"role": "assistant", "content": &llm_response.content}));
                messages.push(serde_json::json!({"role": "user", "content": "请直接使用工具来执行操作，而不是描述你会做什么。你有可用的工具，请调用它们。"}));
                continue;
            }

            // 空回复检测：LLM 返回空内容，重试（最多重试 2 次）
            if llm_response.content.trim().is_empty() && empty_retries < 2 {
                empty_retries += 1;
                log::warn!("LLM 返回空内容（第 {} 轮，第 {} 次重试），重新请求", round + 1, empty_retries);
                if round > 0 {
                    // 工具调用后空回复：提示 LLM 根据工具结果回答
                    messages.push(serde_json::json!({"role": "assistant", "content": ""}));
                    messages.push(serde_json::json!({"role": "user", "content": "请根据上面工具的执行结果，给出完整的回复。"}));
                }
                // round == 0 时直接重试（代理 API 偶发空响应）
                continue;
            }

            let final_msg = serde_json::json!({
                "role": "assistant", "content": &llm_response.content,
                "provider": config.provider, "model": config.model,
                "stop_reason": &llm_response.stop_reason,
            });
            let _ = memory::conversation::save_chat_message(deps.pool, session_id, agent_id, &final_msg).await;
            break;
        }

        // 追加 assistant(tool_calls) 到 messages
        append_assistant_message(&mut messages, &llm_response, provider);
        // 持久化
        {
            let tool_calls_json: Vec<serde_json::Value> = llm_response.tool_calls.iter().map(|tc| {
                serde_json::json!({"id": tc.id, "type": "function", "function": {"name": tc.name, "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default()}})
            }).collect();
            let asst_msg = serde_json::json!({
                "role": "assistant",
                "content": if llm_response.content.is_empty() { serde_json::Value::Null } else { serde_json::json!(&llm_response.content) },
                "tool_calls": tool_calls_json,
                "provider": config.provider,
                "model": config.model,
                "stop_reason": &llm_response.stop_reason,
            });
            let _ = memory::conversation::save_chat_message(deps.pool, session_id, agent_id, &asst_msg).await;
        }

        // 执行工具（支持并行）
        let mut seen_sigs: HashSet<String> = HashSet::new();
        let total_tools = llm_response.tool_calls.len();

        // 第一遍：策略检查 + 去重，分为可并行和需串行两组
        let mut parallel_tasks: Vec<(usize, super::tools::ParsedToolCall)> = Vec::new();
        let mut denied_results: Vec<(String, String, String)> = Vec::new(); // (id, name, reason)

        for (i, tc) in llm_response.tool_calls.iter().enumerate() {
            let sig = format!("{}:{}", tc.name, tc.arguments);
            if !seen_sigs.insert(sig) {
                denied_results.push((tc.id.clone(), tc.name.clone(), format!("工具 {} 已在本轮执行过", tc.name)));
                continue;
            }

            let safety = if tc.name.contains('.') {
                ToolSafetyLevel::Approval
            } else {
                deps.tool_manager.get_safety_level(&tc.name).unwrap_or(ToolSafetyLevel::Safe)
            };
            let decision = match deps.policy_engine.lock() {
                Ok(engine) => engine.evaluate(agent_id, Some(session_id), &tc.name, &safety),
                Err(p) => p.into_inner().evaluate(agent_id, Some(session_id), &tc.name, &safety),
            };
            if !decision.allowed {
                log::warn!("策略拒绝工具 {}: {}", tc.name, decision.reason);
                deps.event_broadcaster.emit(super::observer::AgentEvent::ToolBlocked {
                    tool_name: tc.name.clone(),
                    reason: decision.reason.clone(),
                    agent_id: agent_id.to_string(),
                });
                let _ = crate::db::audit::log_tool_call(deps.pool, agent_id, session_id, &tc.name, &tc.arguments.to_string(), Some(&decision.reason), false, "denied", &format!("{:?}", decision.source), 0).await;
                denied_results.push((tc.id.clone(), tc.name.clone(), decision.reason));
                continue;
            }

            // 自治检查
            let autonomy_config = super::autonomy::load_autonomy_config(deps.agent_config.as_deref());
            let _auto_decision = super::autonomy::evaluate_autonomy(&autonomy_config, &tc.name);

            parallel_tasks.push((i, tc.clone()));
        }

        // 先追加被拒绝的结果
        for (id, name, reason) in &denied_results {
            messages.push(dispatcher.format_tool_result(id, name, reason));
        }

        // 判断是否可以并行执行
        let can_parallel = parallel_tasks.len() > 1
            && parallel_tasks.iter().all(|(_, tc)| {
                let safety = deps.tool_manager.get_safety_level(&tc.name).unwrap_or(ToolSafetyLevel::Safe);
                // 只有 Safe/Guarded 才能并行，Sandboxed/Approval 需串行
                matches!(safety, ToolSafetyLevel::Safe | ToolSafetyLevel::Guarded)
            });

        if can_parallel {
            log::info!("并行执行 {} 个工具", parallel_tasks.len());
            // 并行执行
            let mut futures = Vec::new();
            for (i, tc) in &parallel_tasks {
                let is_skill = skill_tools.contains_key(&tc.name);
                let is_builtin = deps.tool_manager.get_safety_level(&tc.name).is_some();
                let tc_name = tc.name.clone();
                let tc_args = tc.arguments.clone();
                let tc_id = tc.id.clone();
                let idx = *i;

                log::info!("执行工具 {}/{}: {} (id={}) [并行]", idx + 1, total_tools, tc_name, tc_id);

                futures.push(async move {
                    let (result_text, success, source, duration_ms) = if is_skill {
                        execute_external_tool(deps, &tc_name, &tc_args, skill_tools, tx).await
                    } else if is_builtin || !tc_name.contains('-') {
                        execute_builtin_tool(deps, &tc_name, &tc_args, tx, agent_id, session_id).await
                    } else {
                        execute_external_tool(deps, &tc_name, &tc_args, skill_tools, tx).await
                    };
                    (tc_id, tc_name, tc_args, result_text, success, source, duration_ms, is_builtin)
                });
            }

            // 等待所有完成
            let results = futures::future::join_all(futures).await;

            for (tc_id, tc_name, tc_args, result_text, success, source, duration_ms, is_builtin) in results {
                if let Some(es) = deps.evolution_state { es.on_tool_call(); }
                let _ = crate::db::audit::log_tool_call(deps.pool, agent_id, session_id, &tc_name, &tc_args.to_string(), Some(&result_text), success, "allowed", source, duration_ms).await;
                if is_builtin {
                    deps.event_broadcaster.emit(super::observer::AgentEvent::ToolStart { tool_name: tc_name.clone(), round });
                    deps.event_broadcaster.emit(super::observer::AgentEvent::ToolDone { tool_name: tc_name.clone(), success, duration_ms: duration_ms as u64 });
                }
                let scrubbed = scrub_credentials(&result_text);
                messages.push(dispatcher.format_tool_result(&tc_id, &tc_name, &scrubbed));
            }
        } else {
            // 串行执行
            for (i, tc) in &parallel_tasks {
                log::info!("执行工具 {}/{}: {} (id={})", i + 1, total_tools, tc.name, tc.id);

                // Hook: BeforeToolCall
                let before_tool_event = super::lifecycle::HookEvent {
                    point: "before_tool_call".to_string(),
                    agent_id: agent_id.to_string(),
                    session_id: session_id.to_string(),
                    payload: serde_json::json!({ "tool": tc.name, "arguments": tc.arguments }),
                };
                if let Err(e) = deps.lifecycle.emit(super::lifecycle::HookPoint::BeforeToolCall, &before_tool_event).await {
                    log::warn!("BeforeToolCall hook 拒绝工具 {}: {}", tc.name, e);
                    messages.push(dispatcher.format_tool_result(&tc.id, &tc.name, &format!("Hook 拒绝: {}", e)));
                    continue;
                }

                let is_skill = skill_tools.contains_key(&tc.name);
                let is_builtin = deps.tool_manager.get_safety_level(&tc.name).is_some();
                let (result_text, success, source, duration_ms) = if is_skill {
                    execute_external_tool(deps, &tc.name, &tc.arguments, skill_tools, tx).await
                } else if is_builtin || !tc.name.contains('-') {
                    execute_builtin_tool(deps, &tc.name, &tc.arguments, tx, agent_id, session_id).await
                } else {
                    execute_external_tool(deps, &tc.name, &tc.arguments, skill_tools, tx).await
                };

                // Hook: AfterToolCall
                deps.lifecycle.notify(super::lifecycle::HookPoint::AfterToolCall, &super::lifecycle::HookEvent {
                    point: "after_tool_call".to_string(),
                    agent_id: agent_id.to_string(),
                    session_id: session_id.to_string(),
                    payload: serde_json::json!({ "tool": tc.name, "success": success, "duration_ms": duration_ms }),
                }).await;

                if let Some(es) = deps.evolution_state { es.on_tool_call(); }
                let _ = crate::db::audit::log_tool_call(deps.pool, agent_id, session_id, &tc.name, &tc.arguments.to_string(), Some(&result_text), success, "allowed", source, duration_ms).await;
                if is_builtin {
                    deps.event_broadcaster.emit(super::observer::AgentEvent::ToolStart { tool_name: tc.name.clone(), round });
                    deps.event_broadcaster.emit(super::observer::AgentEvent::ToolDone { tool_name: tc.name.clone(), success, duration_ms: duration_ms as u64 });
                }

                // 刷新技能缓存检测
                if tc.name == "bash_exec" {
                    let cmd_str = tc.arguments.get("command").and_then(|c| c.as_str()).unwrap_or("");
                    if cmd_str.contains("clawhub install") || cmd_str.contains("clawhub update") || cmd_str.contains("skill") {
                        log::info!("检测到可能的技能安装操作，技能缓存将在下次对话自动刷新");
                    }
                }

                let scrubbed = scrub_credentials(&result_text);
                messages.push(dispatcher.format_tool_result(&tc.id, &tc.name, &scrubbed));
            }
        }

        // 检测 yield：如果本轮有 sessions_yield 工具调用，暂停 loop
        let mut yielded = false;
        let mut yield_message = String::new();
        let mut yield_waiting_for: Option<String> = None;
        for msg in messages.iter().rev().take(total_tools + denied_results.len()) {
            if let Some(content) = msg["content"].as_str() {
                if content.starts_with("YIELD:") {
                    yielded = true;
                    yield_message = content.strip_prefix("YIELD:").unwrap_or("").trim().to_string();
                    // 尝试提取等待的 run_id
                    if let Some(rid) = yield_message.strip_prefix("wait:") {
                        yield_waiting_for = Some(rid.trim().to_string());
                        yield_message = format!("Yielded, waiting for {}", rid.trim());
                    }
                    break;
                }
            }
        }

        // 持久化工具结果
        {
            let msg_count = messages.len();
            let mut save_start = msg_count;
            for idx in (0..msg_count).rev() {
                if messages[idx]["role"].as_str() == Some("assistant") { save_start = idx + 1; break; }
            }
            for idx in save_start..msg_count {
                let _ = memory::conversation::save_chat_message(deps.pool, session_id, agent_id, &messages[idx]).await;
            }
        }

        // 如果 yielded，提前退出 loop
        if yielded {
            log::info!("Agent loop YIELDED: {} (waiting_for={:?})", yield_message, yield_waiting_for);
            let _ = tx.send(format!("\n⏸️ {}\n", yield_message));

            // 如果有等待的 run_id，等待子代理完成，然后将结果作为新消息注入
            if let Some(ref run_id) = yield_waiting_for {
                let _ = tx.send("\n⏳ Waiting for subagent to complete...\n".to_string());
                // 使用 SubagentRegistry 的 yield_wait
                // 注意：这里需要从 deps 获取 registry，但当前 deps 没有
                // 简化方案：直接轮询 DB
                let timeout = std::time::Duration::from_secs(120);
                let start = std::time::Instant::now();
                #[allow(unused_assignments)]
                let mut subagent_result = String::new();
                loop {
                    let status: Option<(String, Option<String>)> = sqlx::query_as(
                        "SELECT status, output FROM subagent_runs WHERE id = ?"
                    ).bind(run_id).fetch_optional(deps.pool).await.ok().flatten();

                    if let Some((st, output)) = &status {
                        if st == "success" {
                            subagent_result = output.clone().unwrap_or_default();
                            let _ = tx.send(format!("\n✅ Subagent completed\n"));
                            break;
                        } else if st == "failed" || st == "timeout" || st == "cancelled" {
                            subagent_result = format!("Subagent {}: {}", st, output.as_deref().unwrap_or(""));
                            let _ = tx.send(format!("\n❌ Subagent {}\n", st));
                            break;
                        }
                    }

                    if start.elapsed() > timeout {
                        subagent_result = "Subagent wait timeout (120s)".into();
                        let _ = tx.send("\n⚠️ Subagent timeout\n".to_string());
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }

                // 将子代理结果注入为新的 user message，继续 agent loop
                if !subagent_result.is_empty() {
                    let inject_msg = format!("[Subagent Result for {}]\n\n{}", run_id, subagent_result);
                    messages.push(serde_json::json!({"role": "user", "content": inject_msg}));
                    let _ = memory::conversation::save_chat_message(
                        deps.pool, session_id, agent_id,
                        &serde_json::json!({"role": "user", "content": inject_msg}),
                    ).await;
                    // 不 break，继续 agent loop 的下一轮
                    continue;
                }
            }

            // 无等待目标的 yield，直接结束
            let mut partial = full_content.clone();
            partial.push_str(&yield_message);
            return Ok(partial);
        }

        if round == MAX_TOOL_ROUNDS - 1 {
            let _ = tx.send(format!("\n[警告: 工具调用轮数已达上限 {}]\n", MAX_TOOL_ROUNDS));
        }
    }

    // 缓存
    if tool_defs.is_empty() && !full_content.is_empty() {
        let _ = response_cache.put(&cache_key, &config.model, &full_content).await;
    }

    Ok(full_content)
}

/// 执行外部工具（技能 / MCP）
async fn execute_external_tool(
    deps: &AgentLoopDeps<'_>,
    name: &str,
    args: &serde_json::Value,
    skill_tools: &HashMap<String, Box<dyn Tool>>,
    tx: &mpsc::UnboundedSender<String>,
) -> (String, bool, &'static str, i64) {
    // 技能工具
    if let Some(skill_tool) = skill_tools.get(name) {
        let _ = tx.send(format!("\n[技能工具: {} 执行中...]\n", name));
        let start = std::time::Instant::now();
        let result = skill_tool.execute(args.clone()).await;
        let ms = start.elapsed().as_millis() as i64;
        return match result {
            Ok(text) => (text, true, "skill", ms),
            Err(e) => (format!("技能工具调用失败: {}", e), false, "skill", ms),
        };
    }

    // MCP 工具
    let _ = tx.send(format!("\n[MCP 工具: {} 执行中...]\n", name));
    let start = std::time::Instant::now();
    let result = deps.mcp_manager.call_tool(name, args.clone()).await;
    let ms = start.elapsed().as_millis() as i64;
    match result {
        Ok(text) => (text, true, "mcp", ms),
        Err(e) => (format!("MCP 工具调用失败: {}", e), false, "mcp", ms),
    }
}

/// 执行内置工具
async fn execute_builtin_tool(
    deps: &AgentLoopDeps<'_>,
    name: &str,
    args: &serde_json::Value,
    tx: &mpsc::UnboundedSender<String>,
    agent_id: &str,
    session_id: &str,
) -> (String, bool, &'static str, i64) {
    let safety = deps.tool_manager.get_safety_level(name);
    match safety {
        Some(ToolSafetyLevel::Approval) => {
            // 尝试通过审批管理器请求用户确认
            if let (Some(mgr), Some(handle)) = (deps.approval_manager, deps.app_handle) {
                let req_id = uuid::Uuid::new_v4().to_string();
                let request = super::approval::ApprovalRequest {
                    request_id: req_id.clone(),
                    agent_id: agent_id.to_string(),
                    session_id: session_id.to_string(),
                    tool_name: name.to_string(),
                    arguments: args.clone(),
                    safety_level: "approval".to_string(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                };

                // 发送审批事件到前端
                let _ = handle.emit_all("tool-approval-request", &request);
                let _ = tx.send(format!("\n[等待审批: {} ...]\n", name));

                let rx = mgr.request(&req_id);

                // 等待审批（最多 60 秒）
                match tokio::time::timeout(
                    std::time::Duration::from_secs(60),
                    rx,
                ).await {
                    Ok(Ok(super::approval::ApprovalResult::Approved)) => {
                        log::info!("工具 {} 已获批准", name);
                        let _ = tx.send(format!("\n[已批准: {}]\n", name));
                        // 继续执行（不 return）
                    }
                    Ok(Ok(super::approval::ApprovalResult::Denied(reason))) => {
                        let msg = format!("用户拒绝执行: {}", if reason.is_empty() { "无原因" } else { &reason });
                        return (msg, false, "user_denied", 0);
                    }
                    _ => {
                        // 审批超时 → 升级通知
                        log::warn!("工具 {} 审批超时，检查是否有上级 Agent 可升级", name);
                        deps.event_broadcaster.emit(super::observer::AgentEvent::ToolBlocked {
                            tool_name: name.to_string(),
                            reason: "审批超时，已自动拒绝".to_string(),
                            agent_id: agent_id.to_string(),
                        });
                        // 记录审计
                        let _ = crate::db::audit::log_tool_call(
                            deps.pool, agent_id, session_id, name,
                            &args.to_string(), Some("审批超时"), false,
                            "timeout_escalation", "approval", 0,
                        ).await;
                        return ("审批超时（60秒），已记录并通知。如需执行请重新发起。".to_string(), false, "approval_timeout", 0);
                    }
                }
            } else {
                // 无审批管理器，直接拒绝
                return (format!("工具 {} 需要用户审批，但审批系统未初始化", name), false, "safety_level", 0);
            }
        }
        None => {
            return (format!("工具不存在: {}", name), false, "not_found", 0);
        }
        _ => {}
    }

    // 为 delegate_task 注入父上下文
    let mut final_args = args.clone();
    if name == "delegate_task" {
        if let Some(obj) = final_args.as_object_mut() {
            obj.insert("_parent_agent_id".to_string(), serde_json::json!(agent_id));
            obj.insert("_parent_session_id".to_string(), serde_json::json!(session_id));
        }
    }

    let _ = tx.send(format!("\n[工具: {} 执行中...]\n", name));
    let timeout = match name {
        "bash_exec" => std::time::Duration::from_secs(120),
        "web_fetch" => std::time::Duration::from_secs(30),
        "delegate_task" => std::time::Duration::from_secs(300), // 子代理需要更长超时
        _ => std::time::Duration::from_secs(60),
    };
    let start = std::time::Instant::now();
    let result = match tokio::time::timeout(timeout, deps.tool_manager.execute_tool(name, final_args)).await {
        Ok(r) => r,
        Err(_) => super::tools::ToolCallResult {
            tool_name: name.to_string(), success: false, result: String::new(),
            error: Some(format!("工具执行超时（{}秒）", timeout.as_secs())),
        },
    };
    let ms = start.elapsed().as_millis() as i64;
    if result.success {
        (result.result, true, "builtin", ms)
    } else {
        (format!("错误: {}", result.error.unwrap_or_default()), false, "builtin", ms)
    }
}

// ────────────────────────────────────────────────────────────────
// 辅助函数（从 orchestrator 搬来）
// ────────────────────────────────────────────────────────────────

/// 检测 LLM 回复中的工具使用意图
pub fn detect_tool_intent(content: &str) -> bool {
    const CN_PATTERNS: &[&str] = &[
        "我来查看", "让我读取", "我需要执行", "我来运行", "让我搜索",
        "我会创建", "让我写入", "我来查找", "让我检查", "我来打开",
        "我将读取", "我会执行", "让我运行", "我来编辑", "让我修改",
    ];
    const EN_PATTERNS: &[&str] = &[
        "let me read", "i'll check", "i will look", "let me search",
        "i'll run", "let me execute", "i would need to", "i can check",
        "let me open", "i'll create", "let me write",
    ];
    let lower = content.to_lowercase();
    CN_PATTERNS.iter().any(|p| content.contains(p)) || EN_PATTERNS.iter().any(|p| lower.contains(p))
}

/// 清理凭证
pub fn scrub_credentials(input: &str) -> String {
    use once_cell::sync::Lazy;
    use regex::Regex;

    static PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| vec![
        Regex::new(r#"(?i)((?:api[_\-]?key|token|secret|password|passwd|auth|bearer|credential|access[_\-]?key)["'\s]*[:=]\s*["']?)([a-zA-Z0-9_\-./+]{8,})"#).unwrap(),
        Regex::new(r#"(?i)(Bearer\s+)([a-zA-Z0-9_\-./+]{8,})"#).unwrap(),
        Regex::new(r#"\b(sk-|ghp_|gho_|glpat-|xoxb-|xoxp-)([a-zA-Z0-9_\-]{8,})"#).unwrap(),
    ]);

    let mut result = input.to_string();
    for pattern in PATTERNS.iter() {
        result = pattern.replace_all(&result, |caps: &regex::Captures| {
            let prefix = &caps[1];
            let secret = &caps[2];
            let visible: String = secret.chars().take(4).collect();
            format!("{}{}...[REDACTED]", prefix, visible)
        }).to_string();
    }
    result
}

/// 判断是否为 XML 工具格式的模型
pub fn is_xml_model(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("qwen") && !m.contains("qwen2.5")
}

/// 将 assistant 的工具调用响应追加到消息列表
pub fn append_assistant_message(messages: &mut Vec<serde_json::Value>, response: &LlmResponse, provider: &str) {
    match provider {
        "anthropic" => {
            let mut content = Vec::new();
            if !response.content.is_empty() {
                content.push(serde_json::json!({"type": "text", "text": response.content}));
            }
            for tc in &response.tool_calls {
                content.push(serde_json::json!({"type": "tool_use", "id": tc.id, "name": tc.name, "input": tc.arguments}));
            }
            messages.push(serde_json::json!({"role": "assistant", "content": content}));
        }
        _ => {
            let mut msg = serde_json::json!({"role": "assistant"});
            if !response.content.is_empty() {
                msg["content"] = serde_json::Value::String(response.content.clone());
            } else {
                msg["content"] = serde_json::Value::Null;
            }
            if !response.tool_calls.is_empty() {
                let tool_calls: Vec<serde_json::Value> = response.tool_calls.iter().map(|tc| {
                    serde_json::json!({"id": tc.id, "type": "function", "function": {"name": tc.name, "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default()}})
                }).collect();
                msg["tool_calls"] = serde_json::Value::Array(tool_calls);
            }
            messages.push(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_tool_intent_cn() {
        assert!(detect_tool_intent("我来查看一下文件内容"));
        assert!(detect_tool_intent("让我搜索相关信息"));
        assert!(!detect_tool_intent("文件已经读取完毕"));
        assert!(!detect_tool_intent("Hello world"));
    }

    #[test]
    fn test_detect_tool_intent_en() {
        assert!(detect_tool_intent("Let me read the file"));
        assert!(detect_tool_intent("I'll check the directory"));
        assert!(!detect_tool_intent("The file contains important data"));
        assert!(!detect_tool_intent("Here are the results"));
    }

    #[test]
    fn test_scrub_credentials_api_key() {
        let input = r#"api_key: sk-proj-abcdef123456"#;
        let scrubbed = scrub_credentials(input);
        assert!(scrubbed.contains("[REDACTED]"));
        assert!(!scrubbed.contains("abcdef123456"));
    }

    #[test]
    fn test_scrub_credentials_bearer() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.test";
        let scrubbed = scrub_credentials(input);
        assert!(scrubbed.contains("[REDACTED]"));
    }

    #[test]
    fn test_scrub_credentials_no_secrets() {
        let input = "This is a normal response with no secrets";
        assert_eq!(scrub_credentials(input), input);
    }

    #[test]
    fn test_is_xml_model() {
        assert!(is_xml_model("qwen-turbo"));
        assert!(!is_xml_model("qwen2.5-72b"));
        assert!(!is_xml_model("gpt-4o"));
        assert!(!is_xml_model("deepseek-chat"));
    }

    #[test]
    fn test_append_assistant_message_openai() {
        let mut messages = Vec::new();
        let response = LlmResponse {
            content: "Hello".to_string(),
            tool_calls: vec![],
            stop_reason: "stop".to_string(),
            usage: None,
        };
        append_assistant_message(&mut messages, &response, "openai");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["content"], "Hello");
    }

    #[test]
    fn test_append_assistant_message_with_tools() {
        let mut messages = Vec::new();
        let response = LlmResponse {
            content: "".to_string(),
            tool_calls: vec![super::super::tools::ParsedToolCall {
                id: "c1".to_string(),
                name: "search".to_string(),
                arguments: serde_json::json!({"q": "test"}),
            }],
            stop_reason: "tool_use".to_string(),
            usage: None,
        };
        append_assistant_message(&mut messages, &response, "openai");
        assert_eq!(messages[0]["tool_calls"].as_array().unwrap().len(), 1);
    }
}
