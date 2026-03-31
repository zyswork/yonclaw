//! SQLite 本地数据库模块
//!
//! 提供数据库连接池、迁移、查询等功能
//! 支持对话历史、Agent 配置、记忆体存储

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;

pub mod audit;
pub mod models;
pub mod queries;
pub mod schema;

/// 数据库连接管理器
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// 创建新的数据库连接
    ///
    /// # Arguments
    /// * `db_path` - SQLite 数据库文件路径
    ///
    /// # Returns
    /// 返回初始化后的 Database 实例或错误
    pub async fn new(db_path: &str) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path))?
            .create_if_missing(true)
            .pragma("foreign_keys", "on")
            .busy_timeout(std::time::Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        // 初始化数据库 schema
        schema::init_schema(&pool).await?;

        log::info!("数据库初始化成功: {}", db_path);

        Ok(Database { pool })
    }

    /// 获取连接池引用
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// 获取配置项
    pub async fn get_setting(&self, key: &str) -> Result<Option<String>, sqlx::Error> {
        let row = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row)
    }

    /// 设置配置项（插入或更新）
    pub async fn set_setting(&self, key: &str, value: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(key)
        .bind(value)
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 关闭数据库连接
    pub async fn close(&self) {
        self.pool.close().await;
        log::info!("数据库连接已关闭");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_database_creation() {
        let db = Database::new("sqlite::memory:").await;
        assert!(db.is_ok());
    }
}
