//! Agent CRUD + 成本/限额
//!
//! 从 orchestrator.rs 提取的 Agent 数据管理职责。

use super::workspace::AgentWorkspace;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

/// Agent 状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub system_prompt: String,
    pub model: String,
    pub temperature: f64,
    pub max_tokens: i32,
    pub created_at: i64,
    pub updated_at: i64,
    pub config_version: Option<i64>,
    pub config: Option<String>,
}

/// 估算 LLM 调用成本（美元）
pub fn estimate_cost(model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
    let m = model.to_lowercase();
    let (input_price, output_price) = if m.contains("claude-3-5-sonnet") || m.contains("claude-sonnet-4") {
        (3.0, 15.0)
    } else if m.contains("claude-3-5-haiku") || m.contains("claude-haiku-4") {
        (0.8, 4.0)
    } else if m.contains("claude-3-opus") || m.contains("claude-opus-4") {
        (15.0, 75.0)
    } else if m.starts_with("gpt-4o-mini") {
        (0.15, 0.6)
    } else if m.starts_with("gpt-4o") || m.starts_with("gpt-4-turbo") {
        (2.5, 10.0)
    } else if m.contains("gpt-5") && m.contains("mini") {
        (0.30, 1.20)
    } else if m.starts_with("gpt-5") || m.starts_with("gpt-4.5") {
        (5.0, 20.0)
    } else if m.starts_with("deepseek") {
        (0.14, 0.28)
    } else if m.starts_with("qwen") {
        (0.5, 2.0)
    } else if m.starts_with("gemini-2.5-pro") {
        (1.25, 10.0)
    } else if m.starts_with("gemini") {
        (0.075, 0.3)
    } else if m.starts_with("llama") || m.starts_with("mixtral") {
        (0.05, 0.1) // Groq 免费/极低价
    } else if m.starts_with("mistral-large") {
        (2.0, 6.0)
    } else if m.starts_with("mistral") || m.starts_with("codestral") {
        (0.2, 0.6)
    } else if m.starts_with("grok") {
        (3.0, 15.0)
    } else if m.starts_with("glm-5") {
        (2.0, 8.0)
    } else if m.starts_with("glm") {
        (0.5, 2.0)
    } else if m.starts_with("minimax") {
        (1.0, 4.0)
    } else if m.starts_with("kimi") || m.starts_with("moonshot") {
        (0.5, 2.0)
    } else if m.starts_with("baichuan") {
        (0.5, 2.0)
    } else if m.starts_with("step") {
        (0.5, 2.0)
    } else if m.starts_with("doubao") {
        (0.3, 1.0)
    } else if m.starts_with("o3") || m.starts_with("o1") {
        (5.0, 20.0) // reasoning models 较贵
    } else {
        (1.0, 3.0)
    };
    (input_tokens as f64 * input_price + output_tokens as f64 * output_price) / 1_000_000.0
}

/// Agent CRUD 操作
pub struct AgentStore {
    pool: SqlitePool,
    /// Agent 元数据缓存：agent_id → (Agent, fetch_time)
    cache: std::sync::Mutex<std::collections::HashMap<String, (crate::db::models::Agent, std::time::Instant)>>,
}

impl AgentStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// 清除指定 agent 的元数据缓存
    pub fn invalidate_cache(&self, agent_id: &str) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(agent_id);
        }
    }

    /// 获取缓存的 Agent 元数据（60 秒 TTL）
    pub async fn get_cached(&self, agent_id: &str) -> Result<crate::db::models::Agent, String> {
        {
            if let Ok(cache) = self.cache.lock() {
                if let Some((agent, fetched_at)) = cache.get(agent_id) {
                    if fetched_at.elapsed().as_secs() < 5 {
                        return Ok(agent.clone());
                    }
                }
            }
        }
        let agent = sqlx::query_as::<_, crate::db::models::Agent>("SELECT * FROM agents WHERE id = ?")
            .bind(agent_id).fetch_optional(&self.pool).await
            .map_err(|e| format!("查询 Agent 失败: {}", e))?
            .ok_or_else(|| "Agent 不存在".to_string())?;
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(agent_id.to_string(), (agent.clone(), std::time::Instant::now()));
        }
        Ok(agent)
    }

    /// 注册新 Agent
    pub async fn register(&self, name: &str, system_prompt: &str, model: &str) -> Result<Agent, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let workspace = AgentWorkspace::new(&id);
        workspace.initialize(name).await.map_err(|e| format!("初始化工作区失败: {}", e))?;
        let workspace_path = workspace.root().to_string_lossy().to_string();

        sqlx::query("INSERT INTO agents (id, name, system_prompt, model, temperature, max_tokens, workspace_path, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&id).bind(name).bind(system_prompt).bind(model)
            .bind(Option::<f64>::None).bind(2048i32).bind(&workspace_path).bind(now).bind(now)
            .execute(&self.pool).await.map_err(|e| format!("创建 Agent 失败: {}", e))?;

        log::info!("Agent 已注册: {} ({})", name, id);
        self.invalidate_cache(&id);
        Ok(Agent { id, name: name.to_string(), system_prompt: system_prompt.to_string(), model: model.to_string(), temperature: 0.0, max_tokens: 2048, created_at: now, updated_at: now, config_version: Some(1), config: None })
    }

    /// 列出所有 Agent
    pub async fn list(&self) -> Result<Vec<Agent>, String> {
        let rows = sqlx::query_as::<_, crate::db::models::Agent>("SELECT * FROM agents ORDER BY created_at DESC")
            .fetch_all(&self.pool).await.map_err(|e| format!("查询 Agent 列表失败: {}", e))?;
        Ok(rows.into_iter().map(|a| Agent {
            id: a.id, name: a.name, system_prompt: a.system_prompt, model: a.model,
            temperature: a.temperature.unwrap_or(0.7), max_tokens: a.max_tokens.unwrap_or(2048),  // 注意：此处 0.7 仅用于 AgentStore::Agent（非 LlmConfig），实际推理温度由 llm::default_temperature 决定
            created_at: a.created_at, updated_at: a.updated_at,
            config_version: a.config_version, config: a.config,
        }).collect())
    }

    /// 删除 Agent（同时清理 workspace 目录）
    pub async fn delete(&self, agent_id: &str) -> Result<(), String> {
        // 先获取 workspace_path 用于清理目录
        let workspace_path: Option<String> = sqlx::query_scalar(
            "SELECT workspace_path FROM agents WHERE id = ?"
        )
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("查询 Agent 失败: {}", e))?
        .flatten();

        let result = sqlx::query("DELETE FROM agents WHERE id = ?").bind(agent_id)
            .execute(&self.pool).await.map_err(|e| format!("删除 Agent 失败: {}", e))?;
        if result.rows_affected() == 0 { return Err("Agent 不存在".to_string()); }

        // 清理 workspace 目录
        if let Some(wp) = workspace_path {
            let path = std::path::PathBuf::from(&wp);
            if path.exists() {
                if let Err(e) = std::fs::remove_dir_all(&path) {
                    log::warn!("清理 Agent workspace 失败: {} ({})", wp, e);
                } else {
                    log::info!("Agent workspace 已清理: {}", wp);
                }
            }
        }

        self.invalidate_cache(agent_id);
        log::info!("Agent 已删除: {}", agent_id);
        Ok(())
    }

    /// 获取每日 Token 限额
    pub async fn get_daily_token_limit(&self, agent_id: &str) -> u64 {
        if let Ok(agent) = self.get_cached(agent_id).await {
            if let Some(ref config_str) = agent.config {
                if let Ok(config) = serde_json::from_str::<serde_json::Value>(config_str) {
                    if let Some(limit) = config.get("dailyTokenLimit").and_then(|v| v.as_u64()) {
                        return limit;
                    }
                }
            }
        }
        let result: Option<String> = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = 'daily_token_limit'"
        ).fetch_optional(&self.pool).await.ok().flatten();
        result.and_then(|v| v.parse::<u64>().ok()).unwrap_or(0)
    }

    /// 获取今日已消耗 token
    pub async fn get_today_token_usage(&self, agent_id: &str) -> u64 {
        let today_start = {
            let now = chrono::Local::now();
            now.date_naive().and_hms_opt(0, 0, 0)
                .map(|dt| dt.and_utc().timestamp_millis())
                .unwrap_or(0)
        };
        let result: Option<(i64,)> = sqlx::query_as(
            "SELECT COALESCE(SUM(total_tokens), 0) FROM token_usage WHERE agent_id = ? AND created_at >= ?"
        )
        .bind(agent_id).bind(today_start)
        .fetch_optional(&self.pool).await.ok().flatten();
        result.map(|(v,)| v as u64).unwrap_or(0)
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

    #[tokio::test]
    async fn test_register_agent() {
        let pool = setup_pool().await;
        let store = AgentStore::new(pool);
        let agent = store.register("Test", "You are helpful", "gpt-4").await.unwrap();
        assert_eq!(agent.name, "Test");
        assert!(!agent.id.is_empty());
    }

    #[tokio::test]
    async fn test_list_and_delete() {
        let pool = setup_pool().await;
        let store = AgentStore::new(pool);
        store.register("A1", "p1", "gpt-4").await.unwrap();
        store.register("A2", "p2", "gpt-4").await.unwrap();
        assert_eq!(store.list().await.unwrap().len(), 2);

        let agents = store.list().await.unwrap();
        store.delete(&agents[0].id).await.unwrap();
        assert_eq!(store.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_delete_nonexistent() {
        let pool = setup_pool().await;
        let store = AgentStore::new(pool);
        assert!(store.delete("nonexistent").await.is_err());
    }
}
