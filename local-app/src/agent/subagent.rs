//! 子 Agent 系统
//!
//! 支持主 Agent 派生子 Agent 执行子任务。
//! SubagentRegistry 统一管理内存状态（邮箱/等待者）+ DB 持久化（subagent_runs 表）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};

/// 子 Agent 状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SubagentStatus {
    Running,
    Completed,
    Failed(String),
    Timeout,
    Cancelled,
}

impl SubagentStatus {
    /// 转为 DB 状态字符串
    pub fn to_db_str(&self) -> &str {
        match self {
            SubagentStatus::Running => "running",
            SubagentStatus::Completed => "success",
            SubagentStatus::Failed(_) => "failed",
            SubagentStatus::Timeout => "timeout",
            SubagentStatus::Cancelled => "cancelled",
        }
    }
}

/// 子 Agent 记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentRecord {
    pub id: String,
    pub parent_id: String,
    pub name: String,
    pub task: String,
    pub status: SubagentStatus,
    pub result: Option<String>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub timeout_secs: u64,
}

/// 消息信封
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub from: String,
    pub to: String,
    pub content: String,
    pub timestamp: i64,
}

/// 子 Agent 注册表
///
/// 内存状态（邮箱/等待者）+ DB 持久化（subagent_runs 表）双写。
pub struct SubagentRegistry {
    /// 活跃的子 Agent 记录（内存缓存，DB 为主）
    records: Arc<Mutex<HashMap<String, SubagentRecord>>>,
    /// 消息邮箱
    mailboxes: Arc<Mutex<HashMap<String, Vec<AgentMessage>>>>,
    /// 等待回复的 channel
    waiters: Arc<Mutex<HashMap<String, oneshot::Sender<AgentMessage>>>>,
    /// DB 连接池（用于持久化）
    pool: Option<sqlx::SqlitePool>,
}

impl SubagentRegistry {
    pub fn new() -> Self {
        Self {
            records: Arc::new(Mutex::new(HashMap::new())),
            mailboxes: Arc::new(Mutex::new(HashMap::new())),
            waiters: Arc::new(Mutex::new(HashMap::new())),
            pool: None,
        }
    }

    /// 带 DB 连接的构造（推荐）
    pub fn with_pool(pool: sqlx::SqlitePool) -> Self {
        Self {
            records: Arc::new(Mutex::new(HashMap::new())),
            mailboxes: Arc::new(Mutex::new(HashMap::new())),
            waiters: Arc::new(Mutex::new(HashMap::new())),
            pool: Some(pool),
        }
    }

    /// 注册新的子 Agent（双写内存 + DB）
    ///
    /// DB 写入优先：如果 DB 写入失败，不更新内存，保证一致性。
    /// 如果内存更新失败（理论上不会），DB 已写入，记录错误日志。
    pub async fn register(&self, record: SubagentRecord) {
        let id = record.id.clone();

        // 先写 DB（INSERT OR IGNORE 避免与 delegate.rs 的 save_run 冲突）
        if let Some(ref pool) = self.pool {
            let now = chrono::Utc::now().timestamp_millis();
            if let Err(e) = sqlx::query(
                "INSERT OR IGNORE INTO subagent_runs (id, parent_agent_id, parent_session_id, task_index, goal, model, status, depth, created_at) VALUES (?, ?, NULL, 0, ?, 'default', 'running', 0, ?)"
            )
            .bind(&record.id)
            .bind(&record.parent_id)
            .bind(&record.task)
            .bind(now)
            .execute(pool)
            .await {
                log::warn!("SubagentRegistry DB 写入失败，跳过内存更新以保持一致性: {}", e);
                return;
            }
        }

        // DB 写入成功后，更新内存
        let should_cleanup = {
            let mut records = self.records.lock().await;
            records.insert(id.clone(), record);
            records.len() > 100
        };
        self.mailboxes.lock().await.insert(id, Vec::new());

        if should_cleanup {
            self.cleanup().await;
        }
    }

    /// 更新子 Agent 状态（双写内存 + DB）
    pub async fn update_status(&self, id: &str, status: SubagentStatus, result: Option<String>) {
        let now = chrono::Utc::now().timestamp_millis();

        // 更新内存
        if let Some(record) = self.records.lock().await.get_mut(id) {
            record.status = status.clone();
            record.result = result.clone();
            record.finished_at = Some(now);
        }

        // 更新 DB
        if let Some(ref pool) = self.pool {
            let error_msg = match &status {
                SubagentStatus::Failed(e) => Some(e.as_str()),
                _ => None,
            };
            if let Err(e) = sqlx::query(
                "UPDATE subagent_runs SET status = ?, result = ?, error = ?, finished_at = ? WHERE id = ?"
            )
            .bind(status.to_db_str())
            .bind(&result)
            .bind(error_msg)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await {
                log::warn!("SubagentRegistry DB 更新失败: {}", e);
            }
        }
    }

    /// 获取子 Agent 记录
    pub async fn get(&self, id: &str) -> Option<SubagentRecord> {
        self.records.lock().await.get(id).cloned()
    }

    /// 列出某个父 Agent 的所有子 Agent（内存 + DB 合并）
    pub async fn list_children(&self, parent_id: &str) -> Vec<SubagentRecord> {
        // 优先从内存获取活跃记录
        let mut results: Vec<SubagentRecord> = self.records.lock().await.values()
            .filter(|r| r.parent_id == parent_id)
            .cloned()
            .collect();

        // 如果内存为空，从 DB 加载最近的
        if results.is_empty() {
            if let Some(ref pool) = self.pool {
                if let Ok(rows) = sqlx::query_as::<_, (String, String, String, String, Option<String>, i64, Option<i64>)>(
                    "SELECT id, parent_agent_id, goal, status, result, created_at, finished_at FROM subagent_runs WHERE parent_agent_id = ? ORDER BY created_at DESC LIMIT 50"
                )
                .bind(parent_id)
                .fetch_all(pool)
                .await {
                    results = rows.into_iter().map(|(id, parent, goal, status, result, created_at, finished_at)| {
                        SubagentRecord {
                            id,
                            parent_id: parent,
                            name: String::new(),
                            task: goal,
                            status: match status.as_str() {
                                "running" => SubagentStatus::Running,
                                "success" => SubagentStatus::Completed,
                                "failed" => SubagentStatus::Failed(String::new()),
                                "timeout" => SubagentStatus::Timeout,
                                "cancelled" => SubagentStatus::Cancelled,
                                _ => SubagentStatus::Failed(format!("unknown: {}", status)),
                            },
                            result,
                            created_at,
                            finished_at,
                            timeout_secs: 0,
                        }
                    }).collect();
                }
            }
        }

        results
    }

    /// 取消子 Agent（双写）
    pub async fn cancel(&self, id: &str) -> Result<(), String> {
        let mut records = self.records.lock().await;
        if let Some(record) = records.get_mut(id) {
            if record.status == SubagentStatus::Running {
                record.status = SubagentStatus::Cancelled;
                record.finished_at = Some(chrono::Utc::now().timestamp_millis());
                drop(records);

                // 同步到 DB
                if let Some(ref pool) = self.pool {
                    let now = chrono::Utc::now().timestamp_millis();
                    let _ = sqlx::query("UPDATE subagent_runs SET status = 'cancelled', finished_at = ? WHERE id = ?")
                        .bind(now).bind(id)
                        .execute(pool).await;
                }
                Ok(())
            } else {
                Err(format!("子 Agent {} 不在运行状态", id))
            }
        } else {
            // 尝试从 DB 取消
            if let Some(ref pool) = self.pool {
                let now = chrono::Utc::now().timestamp_millis();
                let result = sqlx::query("UPDATE subagent_runs SET status = 'cancelled', finished_at = ? WHERE id = ? AND status = 'running'")
                    .bind(now).bind(id)
                    .execute(pool).await;
                match result {
                    Ok(r) if r.rows_affected() > 0 => Ok(()),
                    Ok(_) => Err(format!("子 Agent {} 不存在或不在运行状态", id)),
                    Err(e) => Err(format!("取消失败: {}", e)),
                }
            } else {
                Err(format!("子 Agent {} 不存在", id))
            }
        }
    }

    /// 发送消息（带关系权限检查）
    pub async fn send_message_checked(
        &self,
        pool: &sqlx::SqlitePool,
        msg: AgentMessage,
    ) -> Result<(), String> {
        let can_comm = super::relations::RelationManager::can_communicate(pool, &msg.from, &msg.to).await?;
        if !can_comm {
            return Err(format!(
                "Agent {} 没有与 Agent {} 的通信权限，请先建立关系",
                msg.from, msg.to
            ));
        }
        self.send_message(msg).await
    }

    /// 发送消息到指定 Agent 的邮箱
    pub(crate) async fn send_message(&self, msg: AgentMessage) -> Result<(), String> {
        let to = msg.to.clone();

        {
            let mut waiters = self.waiters.lock().await;
            if let Some(waiter) = waiters.remove(&to) {
                let _ = waiter.send(msg);
                return Ok(());
            }
        }

        let mut mailboxes = self.mailboxes.lock().await;
        mailboxes.entry(to).or_default().push(msg);
        Ok(())
    }

    /// 等待接收消息（带超时）
    pub async fn receive_message(&self, agent_id: &str, timeout_secs: u64) -> Result<AgentMessage, String> {
        {
            let mut mailboxes = self.mailboxes.lock().await;
            if let Some(mailbox) = mailboxes.get_mut(agent_id) {
                if !mailbox.is_empty() {
                    return Ok(mailbox.remove(0));
                }
            }
        }

        let (tx, rx) = oneshot::channel();
        self.waiters.lock().await.insert(agent_id.to_string(), tx);

        match tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            rx,
        ).await {
            Ok(Ok(msg)) => Ok(msg),
            Ok(Err(_)) => Err("消息通道已关闭".to_string()),
            Err(_) => {
                self.waiters.lock().await.remove(agent_id);
                Err("等待消息超时".to_string())
            }
        }
    }

    /// Yield: 暂停当前 Agent，等待指定子代理完成
    ///
    /// 参考 OpenClaw sessions_yield。Agent A 可以：
    /// 1. 派发任务给 Agent B
    /// 2. yield 等待 B 完成
    /// 3. B 完成后 A 自动恢复，获取 B 的结果
    pub async fn yield_wait(&self, run_id: &str, timeout_secs: u64) -> Result<String, String> {
        log::info!("Agent yield: 等待子代理 {} 完成 (timeout={}s)", run_id, timeout_secs);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
        let poll_interval = std::time::Duration::from_millis(500);

        loop {
            // 检查子代理是否完成
            {
                let records = self.records.lock().await;
                if let Some(record) = records.get(run_id) {
                    match &record.status {
                        SubagentStatus::Completed => {
                            let output = record.result.clone().unwrap_or_default();
                            log::info!("Agent yield: {} 已完成，恢复执行", run_id);
                            return Ok(output);
                        }
                        SubagentStatus::Failed(err) => {
                            return Err(format!("子代理 {} 失败: {}", run_id, err));
                        }
                        SubagentStatus::Timeout => {
                            return Err(format!("子代理 {} 超时", run_id));
                        }
                        SubagentStatus::Cancelled => {
                            return Err(format!("子代理 {} 已取消", run_id));
                        }
                        SubagentStatus::Running => {
                            // 继续等待
                        }
                    }
                } else {
                    return Err(format!("子代理 {} 不存在", run_id));
                }
            }

            if std::time::Instant::now() >= deadline {
                return Err(format!("yield 超时（{}s），子代理 {} 仍在运行", timeout_secs, run_id));
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    /// 清理已完成的记录（内存，DB 保留）
    pub async fn cleanup(&self) {
        let mut records = self.records.lock().await;
        let mut finished: Vec<(String, i64)> = records.iter()
            .filter(|(_, r)| r.status != SubagentStatus::Running)
            .map(|(id, r)| (id.clone(), r.finished_at.unwrap_or(0)))
            .collect();
        finished.sort_by(|a, b| b.1.cmp(&a.1));

        let to_remove: Vec<String> = finished.iter().skip(100).map(|(id, _)| id.clone()).collect();
        for id in &to_remove {
            records.remove(id);
        }
        drop(records);
        let mut mailboxes = self.mailboxes.lock().await;
        for id in &to_remove {
            mailboxes.remove(id);
        }
    }
}

/// 从 DB 查询子代理运行记录
pub async fn list_subagent_runs(
    pool: &sqlx::SqlitePool,
    parent_agent_id: Option<&str>,
    session_id: Option<&str>,
    limit: i64,
) -> Result<Vec<serde_json::Value>, String> {
    let mut sql = String::from(
        "SELECT id, parent_agent_id, parent_session_id, task_index, goal, model, status, result, error, depth, allowed_tools, duration_ms, created_at, finished_at FROM subagent_runs WHERE 1=1"
    );
    let mut binds: Vec<String> = Vec::new();

    if let Some(aid) = parent_agent_id {
        sql.push_str(" AND parent_agent_id = ?");
        binds.push(aid.to_string());
    }
    if let Some(sid) = session_id {
        sql.push_str(" AND parent_session_id = ?");
        binds.push(sid.to_string());
    }
    sql.push_str(" ORDER BY created_at DESC LIMIT ?");

    let rows = if binds.is_empty() {
        sqlx::query_as::<_, (String, String, Option<String>, i64, String, String, String, Option<String>, Option<String>, i64, Option<String>, Option<i64>, i64, Option<i64>)>(&sql)
            .bind(limit)
            .fetch_all(pool).await
    } else if binds.len() == 1 {
        sqlx::query_as::<_, (String, String, Option<String>, i64, String, String, String, Option<String>, Option<String>, i64, Option<String>, Option<i64>, i64, Option<i64>)>(&sql)
            .bind(&binds[0])
            .bind(limit)
            .fetch_all(pool).await
    } else {
        sqlx::query_as::<_, (String, String, Option<String>, i64, String, String, String, Option<String>, Option<String>, i64, Option<String>, Option<i64>, i64, Option<i64>)>(&sql)
            .bind(&binds[0])
            .bind(&binds[1])
            .bind(limit)
            .fetch_all(pool).await
    };

    let rows = rows.map_err(|e| format!("查询子代理记录失败: {}", e))?;

    Ok(rows.into_iter().map(|(id, parent_agent_id, parent_session_id, task_index, goal, model, status, result, error, depth, allowed_tools, duration_ms, created_at, finished_at)| {
        serde_json::json!({
            "id": id,
            "parentAgentId": parent_agent_id,
            "parentSessionId": parent_session_id,
            "taskIndex": task_index,
            "goal": goal,
            "model": model,
            "status": status,
            "result": result,
            "error": error,
            "depth": depth,
            "allowedTools": allowed_tools.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok()),
            "durationMs": duration_ms,
            "createdAt": created_at,
            "finishedAt": finished_at,
        })
    }).collect())
}
