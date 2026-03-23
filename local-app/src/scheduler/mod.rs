//! 定时任务调度器

pub mod types;
pub mod store;
pub mod planner;
pub mod runner;
pub mod engine;
pub mod heartbeat;
pub mod tools;
pub mod seed;

use std::sync::Arc;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use self::engine::SchedulerEngine;
use self::runner::JobRunner;

pub use types::*;

/// 调度管理器：持有引擎控制句柄
pub struct SchedulerManager {
    pool: sqlx::SqlitePool,
    notify: Arc<Notify>,
    shutdown: CancellationToken,
}

impl SchedulerManager {
    /// 创建并启动调度引擎
    ///
    /// `notify` 参数允许外部共享唤醒信号（如 cron 工具注册时使用同一个 Notify）
    pub fn start(
        pool: sqlx::SqlitePool,
        notify: Arc<Notify>,
        orchestrator: Arc<crate::agent::Orchestrator>,
        app_handle: tauri::AppHandle,
    ) -> Self {
        let shutdown = CancellationToken::new();

        let runner = Arc::new(JobRunner::new(
            pool.clone(),
            orchestrator.clone(),
        ));

        let mut engine = SchedulerEngine::new(
            pool.clone(),
            notify.clone(),
            runner,
            shutdown.clone(),
            app_handle,
        );
        engine.set_orchestrator(orchestrator);

        tokio::spawn(async move {
            engine.run().await;
        });

        Self { pool, notify, shutdown }
    }

    /// 唤醒调度循环
    pub fn wake(&self) {
        self.notify.notify_one();
    }

    /// 关闭调度引擎
    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        &self.pool
    }

    pub fn notify(&self) -> Arc<Notify> {
        self.notify.clone()
    }
}
