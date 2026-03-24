//! 数据库模型定义

use chrono::Utc;
use serde::{Deserialize, Serialize};

/// 对话记录
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Conversation {
    pub id: String,
    pub agent_id: String,
    pub user_id: String,
    pub user_message: String,
    pub agent_response: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub metadata: Option<String>,
}

impl Conversation {
    /// 创建新的对话记录
    pub fn new(
        agent_id: String,
        user_id: String,
        user_message: String,
        agent_response: String,
    ) -> Self {
        let now = Utc::now().timestamp_millis();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id,
            user_id,
            user_message,
            agent_response,
            created_at: now,
            updated_at: now,
            metadata: None,
        }
    }
}

/// Agent 配置
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub system_prompt: String,
    pub model: String,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i32>,
    pub created_at: i64,
    pub updated_at: i64,
    pub config: Option<String>,
    pub workspace_path: Option<String>,
    pub config_version: Option<i64>,
    /// 绑定的供应商 ID（解决同名模型串供应商）
    pub provider_id: Option<String>,
}

impl Agent {
    /// 创建新的 Agent 配置
    pub fn new(name: String, system_prompt: String, model: String) -> Self {
        let now = Utc::now().timestamp_millis();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            system_prompt,
            model,
            temperature: Some(0.7),
            max_tokens: Some(2048),
            created_at: now,
            updated_at: now,
            config: None,
            workspace_path: None,
            config_version: Some(1),
            provider_id: None,
        }
    }
}

/// 响应缓存条目
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ResponseCacheEntry {
    pub cache_key: String,
    pub model: String,
    pub response: String,
    pub created_at: i64,
    pub last_used_at: i64,
    pub use_count: i64,
}

/// 记忆体记录
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Memory {
    pub id: String,
    pub agent_id: String,
    pub memory_type: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Memory {
    /// 创建新的记忆体记录
    pub fn new(agent_id: String, memory_type: String, content: String) -> Self {
        let now = Utc::now().timestamp_millis();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id,
            memory_type,
            content,
            created_at: now,
            updated_at: now,
        }
    }
}

/// 会话记录
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ChatSession {
    pub id: String,
    pub agent_id: String,
    pub title: String,
    pub created_at: i64,
    pub last_message_at: Option<i64>,
    pub summary: Option<String>,
}

impl ChatSession {
    /// 创建新的会话记录
    pub fn new(agent_id: String, title: String) -> Self {
        let now = Utc::now().timestamp_millis();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id,
            title,
            created_at: now,
            last_message_at: None,
            summary: None,
        }
    }
}

/// 向量数据
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Vector {
    pub id: String,
    pub agent_id: String,
    pub content: String,
    pub embedding: Vec<u8>,
    pub created_at: i64,
}

impl Vector {
    /// 创建新的向量数据
    pub fn new(agent_id: String, content: String, embedding: Vec<u8>) -> Self {
        let now = Utc::now().timestamp_millis();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id,
            content,
            embedding,
            created_at: now,
        }
    }
}
