//! 子代理委托执行
//!
//! 参考 Hermes Agent 的 delegate_task：
//! - 最多 3 个并发子代理
//! - 最大嵌套深度 2 层
//! - 子代理有工具限制（不能递归委托、不能操作记忆）
//! - 结果汇总给父代理
//!
//! 作为内置工具 `delegate_task` 注册到 ToolManager。

use async_trait::async_trait;
use tokio::sync::mpsc;

use super::llm::{LlmClient, LlmConfig};
use super::tools::{Tool, ToolDefinition, ToolSafetyLevel};

/// 最大并发子代理数
const MAX_CONCURRENT: usize = 3;

/// 最大嵌套深度
const MAX_DEPTH: usize = 2;

/// 子代理禁用的工具
const BLOCKED_TOOLS: &[&str] = &[
    "delegate_task",  // 不能递归委托
    "memory_write",   // 不能修改记忆
    "memory_read",    // 不需要读记忆
    "skill_manage",   // 不能修改技能
];

/// 委托执行工具
pub struct DelegateTaskTool {
    pool: sqlx::SqlitePool,
    /// 当前嵌套深度（0=顶层）
    depth: u32,
}

impl DelegateTaskTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool, depth: 0 }
    }
}

#[async_trait]
impl Tool for DelegateTaskTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "delegate_task".to_string(),
            description: "委托子任务给并行子代理执行。可以同时派发多个独立任务，每个子代理有独立上下文。适合需要同时查邮件、查日程、查任务等并行操作的场景。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "array",
                        "description": "任务列表，每个任务包含 goal（目标描述）和可选的 context（上下文信息）",
                        "items": {
                            "type": "object",
                            "properties": {
                                "goal": { "type": "string", "description": "任务目标" },
                                "context": { "type": "string", "description": "可选的上下文信息" }
                            },
                            "required": ["goal"]
                        }
                    }
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

        if tasks.len() > MAX_CONCURRENT * 2 {
            return Err(format!("最多同时 {} 个任务", MAX_CONCURRENT * 2));
        }

        // 获取 LLM 配置
        let providers_json: Option<String> = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = 'providers'"
        ).fetch_optional(&self.pool).await.ok().flatten();

        let (api_type, api_key, base_url, model) = match providers_json.and_then(|pj| {
            let providers: Vec<serde_json::Value> = serde_json::from_str(&pj).ok()?;
            for p in &providers {
                if p["enabled"].as_bool() != Some(true) { continue; }
                let key = p["apiKey"].as_str().unwrap_or("").to_string();
                if key.is_empty() { continue; }
                let api_type = p["apiType"].as_str().unwrap_or("openai").to_string();
                let base_url = p["baseUrl"].as_str().unwrap_or("").to_string();
                let model = p["models"].as_array()
                    .and_then(|models| models.first())
                    .and_then(|m| m["id"].as_str())
                    .unwrap_or("gpt-4o-mini")
                    .to_string();
                return Some((api_type, key, base_url, model));
            }
            None
        }) {
            Some(info) => info,
            None => return Err("未配置 LLM Provider，无法创建子代理".to_string()),
        };

        log::info!("delegate_task: 收到 {} 个子任务，开始并行执行", tasks.len());

        // 并行执行（限制并发数）
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT));
        let mut handles = Vec::new();

        for (i, task) in tasks.iter().enumerate() {
            let goal = task["goal"].as_str().unwrap_or("").to_string();
            let context = task["context"].as_str().unwrap_or("").to_string();

            if goal.is_empty() { continue; }

            let sem = semaphore.clone();
            let api_type = api_type.clone();
            let api_key = api_key.clone();
            let base_url = base_url.clone();
            let model = model.clone();

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();

                log::info!("子代理 #{}: 开始执行「{}」", i + 1, &goal[..goal.len().min(50)]);

                let system_prompt = format!(
                    "你是一个专注执行单一任务的子代理。\n\n你的任务：{}\n{}\n\n要求：\n- 直接执行，不要问问题\n- 完成后给出简洁的结果摘要\n- 如果无法完成，说明原因",
                    goal,
                    if context.is_empty() { String::new() } else { format!("\n背景：{}", context) }
                );

                let config = LlmConfig {
                    provider: api_type,
                    model,
                    api_key,
                    base_url: if base_url.is_empty() { None } else { Some(base_url) },
                    temperature: Some(0.3),
                    max_tokens: Some(2000),
                    thinking_level: None,
                };

                let (tx, mut rx) = mpsc::unbounded_channel::<String>();
                let client = LlmClient::new(config);

                let messages = vec![
                    serde_json::json!({"role": "system", "content": system_prompt}),
                    serde_json::json!({"role": "user", "content": goal}),
                ];

                match tokio::time::timeout(
                    std::time::Duration::from_secs(60),
                    client.call_stream(&messages, None, None, tx),
                ).await {
                    Ok(Ok(resp)) => {
                        let content = if resp.content.is_empty() {
                            let mut collected = String::new();
                            while let Ok(token) = rx.try_recv() { collected.push_str(&token); }
                            collected
                        } else {
                            resp.content
                        };
                        log::info!("子代理 #{}: 完成（{}字符）", i + 1, content.len());
                        (i, goal, Ok(content))
                    }
                    Ok(Err(e)) => {
                        log::warn!("子代理 #{}: 失败: {}", i + 1, e);
                        (i, goal, Err(e))
                    }
                    Err(_) => {
                        log::warn!("子代理 #{}: 超时", i + 1);
                        (i, goal, Err("执行超时（60秒）".to_string()))
                    }
                }
            });

            handles.push(handle);
        }

        // 收集结果
        let mut results = Vec::new();
        for handle in handles {
            if let Ok((i, goal, result)) = handle.await {
                let goal_preview: String = goal.chars().take(30).collect();
                match result {
                    Ok(content) => results.push(format!("### 任务 {}：{}\n\n{}", i + 1, goal_preview, content)),
                    Err(e) => results.push(format!("### 任务 {}：{}\n\n❌ 失败：{}", i + 1, goal_preview, e)),
                }
            }
        }

        Ok(format!("# 子代理执行结果\n\n共 {} 个任务\n\n{}", results.len(), results.join("\n\n---\n\n")))
    }
}
