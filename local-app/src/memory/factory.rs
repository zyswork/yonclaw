//! 存储后端工厂
//!
//! 根据配置创建不同的 Memory 实现。
//! 当前支持 SQLite，预留 PostgreSQL 和 Qdrant 扩展点。
//! 借鉴 ZeroClaw 的多后端设计。

use super::{SqliteMemory, Memory, embedding::EmbeddingConfig};
use sqlx::SqlitePool;

/// 存储后端类型
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryBackend {
    /// SQLite（默认，嵌入式）
    Sqlite,
    /// PostgreSQL（生产部署，预留）
    Postgres,
    /// Qdrant（向量专用，预留）
    Qdrant,
}

impl MemoryBackend {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "postgres" | "postgresql" | "pg" => Self::Postgres,
            "qdrant" => Self::Qdrant,
            _ => Self::Sqlite,
        }
    }
}

/// 存储工厂配置
#[derive(Debug, Clone)]
pub struct MemoryFactoryConfig {
    pub backend: MemoryBackend,
    pub embedding: Option<EmbeddingConfig>,
    // 预留 PostgreSQL 配置
    pub postgres_url: Option<String>,
    // 预留 Qdrant 配置
    pub qdrant_url: Option<String>,
    pub qdrant_collection: Option<String>,
}

impl Default for MemoryFactoryConfig {
    fn default() -> Self {
        Self {
            backend: MemoryBackend::Sqlite,
            embedding: None,
            postgres_url: None,
            qdrant_url: None,
            qdrant_collection: None,
        }
    }
}

/// 创建 Memory 实现
///
/// 当前仅支持 SQLite，其他后端返回 Err 提示未实现。
pub async fn create_memory(
    config: &MemoryFactoryConfig,
    sqlite_pool: SqlitePool,
) -> Result<Box<dyn Memory>, String> {
    match config.backend {
        MemoryBackend::Sqlite => {
            let mem = if let Some(ref emb) = config.embedding {
                SqliteMemory::with_embedding(sqlite_pool, emb.clone()).await
            } else {
                SqliteMemory::new(sqlite_pool)
            };
            Ok(Box::new(mem))
        }
        MemoryBackend::Postgres => {
            Err("PostgreSQL 后端尚未实现。请在 memory/ 目录下添加 postgres.rs 实现 Memory trait。".into())
        }
        MemoryBackend::Qdrant => {
            Err("Qdrant 后端尚未实现。请在 memory/ 目录下添加 qdrant.rs 实现 Memory trait。".into())
        }
    }
}

/// 从 settings 表自动检测配置
pub async fn auto_detect_config(pool: &SqlitePool) -> MemoryFactoryConfig {
    let mut config = MemoryFactoryConfig::default();

    // 检测后端类型
    if let Ok(Some(backend)) = sqlx::query_scalar::<_, String>(
        "SELECT value FROM settings WHERE key = 'memory_backend'"
    ).fetch_optional(pool).await {
        config.backend = MemoryBackend::from_str(&backend);
    }

    // 检测嵌入配置
    if let Some(emb_config) = SqliteMemory::try_load_embedding_config(pool).await {
        config.embedding = Some(emb_config);
    }

    // 检测 PostgreSQL URL
    if let Ok(Some(url)) = sqlx::query_scalar::<_, String>(
        "SELECT value FROM settings WHERE key = 'postgres_url'"
    ).fetch_optional(pool).await {
        config.postgres_url = Some(url);
    }

    config
}
