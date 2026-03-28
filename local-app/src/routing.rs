//! 多 Agent 路由
//!
//! 参考 OpenClaw 的 routing 模块：根据渠道、发送者、@mention 选择 Agent。
//! 一个频道可以绑定多个 Agent，通过 @Agent名 指定谁回复。
//! 没有 @mention 时使用默认 Agent（priority 最小的那个）。

use sqlx::SqlitePool;

/// 路由绑定规则
#[derive(Debug, Clone)]
pub struct AgentBinding {
    /// 渠道 ID（"api", "telegram", "feishu", "*" = 所有渠道）
    pub channel: String,
    /// 发送者 ID（可选，None = 匹配所有发送者）
    pub sender_id: Option<String>,
    /// 绑定的 Agent ID
    pub agent_id: String,
    /// 优先级（越小越优先，最小的是默认 Agent）
    pub priority: i32,
}

/// 路由解析结果
#[derive(Debug, Clone)]
pub struct ResolvedRoute {
    pub agent_id: String,
    pub match_rule: String,
}

/// 路由器
pub struct Router {
    pool: SqlitePool,
    /// 默认 Agent ID（如果没有匹配到任何规则）
    default_agent_id: Option<String>,
}

impl Router {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool, default_agent_id: None }
    }

    pub fn with_default_agent(mut self, agent_id: &str) -> Self {
        self.default_agent_id = Some(agent_id.to_string());
        self
    }

    /// 解析路由：给定渠道、发送者和消息内容，返回应处理的 Agent ID
    /// message_text 用于检测 @mention（如 "@小爪 你好" → 路由到名为"小爪"的 Agent）
    pub async fn resolve(
        &self,
        channel: &str,
        sender_id: Option<&str>,
    ) -> Result<ResolvedRoute, String> {
        self.resolve_with_mention(channel, sender_id, None).await
    }

    /// 带 @mention 检测的路由解析
    pub async fn resolve_with_mention(
        &self,
        channel: &str,
        sender_id: Option<&str>,
        message_text: Option<&str>,
    ) -> Result<ResolvedRoute, String> {
        // 获取该频道绑定的所有 Agent
        let bindings = self.list_bindings(channel).await?;

        // 如果有消息内容，检测 @mention
        if let Some(text) = message_text {
            if text.contains('@') && !bindings.is_empty() {
                // 加载所有绑定的 Agent 名称
                let agent_names = self.load_agent_names(&bindings).await?;
                for (agent_id, name) in &agent_names {
                    // 检查 @Agent名 是否在消息中
                    let mention = format!("@{}", name);
                    if text.contains(&mention) {
                        return Ok(ResolvedRoute {
                            agent_id: agent_id.clone(),
                            match_rule: format!("mention={}", name),
                        });
                    }
                    // 也检查小写匹配
                    if text.to_lowercase().contains(&mention.to_lowercase()) {
                        return Ok(ResolvedRoute {
                            agent_id: agent_id.clone(),
                            match_rule: format!("mention={} (case-insensitive)", name),
                        });
                    }
                }
            }
        }

        // 1. 精确匹配：channel + sender_id
        if let Some(sid) = sender_id {
            if let Some(binding) = self.find_binding(channel, Some(sid)).await? {
                return Ok(ResolvedRoute {
                    agent_id: binding.agent_id,
                    match_rule: format!("channel={} sender={}", channel, sid),
                });
            }
        }

        // 2. 频道默认 Agent（priority 最小的）
        if !bindings.is_empty() {
            return Ok(ResolvedRoute {
                agent_id: bindings[0].agent_id.clone(),
                match_rule: format!("channel={} default", channel),
            });
        }

        // 3. 通配匹配：* + any sender
        if let Some(binding) = self.find_binding("*", None).await? {
            return Ok(ResolvedRoute {
                agent_id: binding.agent_id,
                match_rule: "wildcard".to_string(),
            });
        }

        // 4. 配置的默认 Agent
        if let Some(ref default_id) = self.default_agent_id {
            return Ok(ResolvedRoute {
                agent_id: default_id.clone(),
                match_rule: "default".to_string(),
            });
        }

        // 5. 查找第一个 Agent 作为兜底
        let first: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM agents ORDER BY created_at ASC LIMIT 1"
        ).fetch_optional(&self.pool).await.map_err(|e| format!("查询失败: {}", e))?;

        match first {
            Some((id,)) => Ok(ResolvedRoute {
                agent_id: id,
                match_rule: "first_agent_fallback".to_string(),
            }),
            None => Err("没有可用的 Agent".to_string()),
        }
    }

    /// 列出某个频道绑定的所有 Agent（按 priority 排序）
    pub async fn list_bindings(&self, channel: &str) -> Result<Vec<AgentBinding>, String> {
        let rows: Vec<(String, String, Option<String>, i32)> = sqlx::query_as(
            "SELECT channel, agent_id, sender_id, priority FROM agent_bindings WHERE channel = ? AND sender_id IS NULL ORDER BY priority ASC"
        ).bind(channel).fetch_all(&self.pool).await.map_err(|e| format!("查询路由失败: {}", e))?;

        Ok(rows.into_iter().map(|(channel, agent_id, sender_id, priority)| AgentBinding {
            channel, agent_id, sender_id, priority,
        }).collect())
    }

    /// 加载 Agent ID → 名称映射
    async fn load_agent_names(&self, bindings: &[AgentBinding]) -> Result<Vec<(String, String)>, String> {
        let mut result = Vec::new();
        for b in bindings {
            let name: Option<(String,)> = sqlx::query_as(
                "SELECT name FROM agents WHERE id = ?"
            ).bind(&b.agent_id).fetch_optional(&self.pool).await.map_err(|e| format!("{}", e))?;
            if let Some((name,)) = name {
                if !name.is_empty() {
                    result.push((b.agent_id.clone(), name));
                }
            }
        }
        Ok(result)
    }

    async fn find_binding(&self, channel: &str, sender_id: Option<&str>) -> Result<Option<AgentBinding>, String> {
        let row = if let Some(sid) = sender_id {
            sqlx::query_as::<_, (String, String, Option<String>, i32)>(
                "SELECT channel, agent_id, sender_id, priority FROM agent_bindings WHERE channel = ? AND sender_id = ? ORDER BY priority ASC LIMIT 1"
            ).bind(channel).bind(sid).fetch_optional(&self.pool).await
        } else {
            sqlx::query_as::<_, (String, String, Option<String>, i32)>(
                "SELECT channel, agent_id, sender_id, priority FROM agent_bindings WHERE channel = ? AND sender_id IS NULL ORDER BY priority ASC LIMIT 1"
            ).bind(channel).fetch_optional(&self.pool).await
        }.map_err(|e| format!("查询路由失败: {}", e))?;

        Ok(row.map(|(channel, agent_id, sender_id, priority)| AgentBinding {
            channel, agent_id, sender_id, priority,
        }))
    }
}
