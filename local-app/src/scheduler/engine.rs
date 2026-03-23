//! 调度引擎：sleep-to-earliest + Notify + catch-up + stuck 检测

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;

use super::{store, planner, types::*};
use super::runner::{JobRunner, ExecResult};
use tauri::Manager;

const HEALTH_CHECK_INTERVAL_SECS: u64 = 300; // 5 分钟

pub struct SchedulerEngine {
    pool: sqlx::SqlitePool,
    notify: Arc<Notify>,
    runner: Arc<JobRunner>,
    semaphore: Arc<Semaphore>,
    shutdown: CancellationToken,
    app_handle: tauri::AppHandle,
    orchestrator: Option<Arc<crate::agent::Orchestrator>>,
}

impl SchedulerEngine {
    pub fn new(
        pool: sqlx::SqlitePool,
        notify: Arc<Notify>,
        runner: Arc<JobRunner>,
        shutdown: CancellationToken,
        app_handle: tauri::AppHandle,
    ) -> Self {
        Self {
            pool,
            notify,
            runner,
            semaphore: Arc::new(Semaphore::new(3)),
            shutdown,
            app_handle,
            orchestrator: None,
        }
    }

    /// 注入 Orchestrator（在 engine 启动后调用）
    pub fn set_orchestrator(&mut self, orch: Arc<crate::agent::Orchestrator>) {
        self.orchestrator = Some(orch);
    }

    /// 启动调度循环
    pub async fn run(&self) {
        log::info!("调度引擎启动");

        // 启动时：取消上次未完成的 run
        if let Err(e) = store::cancel_running_runs(&self.pool).await {
            log::error!("取消残留 run 失败: {}", e);
        }
        self.recovery_scan().await;

        let mut last_tick = Instant::now();
        let mut last_health_check = Instant::now();

        loop {
            // 1. 休眠唤醒检测
            if last_tick.elapsed() > Duration::from_secs(RECOVERY_THRESHOLD_SECS) {
                log::info!("检测到系统休眠唤醒，执行 recovery scan");
                self.recovery_scan().await;
            }

            // 2. 计算 sleep 时长
            let now_ts = chrono::Utc::now().timestamp();
            let delay = match store::earliest_next_run(&self.pool).await {
                Ok(Some(ts)) => {
                    let diff = (ts - now_ts).max(0) as u64;
                    Duration::from_secs(diff.min(MAX_TIMER_DELAY_SECS))
                }
                _ => Duration::from_secs(MAX_TIMER_DELAY_SECS),
            };

            // 3. sleep 或被唤醒
            tokio::select! {
                _ = self.notify.notified() => {
                    log::debug!("调度引擎被唤醒");
                }
                _ = tokio::time::sleep(delay) => {}
                _ = self.shutdown.cancelled() => {
                    log::info!("调度引擎收到关闭信号");
                    break;
                }
            }

            last_tick = Instant::now();

            // 4. 批量取到期任务
            let now_ts = chrono::Utc::now().timestamp();
            let due = match store::due_jobs(&self.pool, now_ts).await {
                Ok(jobs) => jobs,
                Err(e) => {
                    log::error!("查询到期任务失败: {}", e);
                    continue;
                }
            };

            // 5. 逐个检查 Guardrails → spawn 执行
            for job in due {
                if !self.check_guardrails(&job).await {
                    self.reschedule_job(&job).await;
                    continue;
                }

                // Poll 类型：检查 URL 内容变化，无变化则跳过
                if let Schedule::Poll { ref url, ref json_path, ref last_hash, .. } = job.schedule {
                    match self.check_poll_change(url, json_path.as_deref(), last_hash.as_deref(), &job.id).await {
                        Ok(true) => { /* 内容有变化，继续执行 */ }
                        Ok(false) => {
                            // 无变化，只重新调度
                            self.reschedule_job(&job).await;
                            continue;
                        }
                        Err(e) => {
                            log::warn!("Poll 检查失败 ({}): {}", job.name, e);
                            self.reschedule_job(&job).await;
                            continue;
                        }
                    }
                }

                self.spawn_job(job).await;
            }

            // 6. 定期健康检查 + LLM 心跳
            if last_health_check.elapsed() > Duration::from_secs(HEALTH_CHECK_INTERVAL_SECS) {
                self.health_check().await;

                // LLM 智能心跳（如果 orchestrator 已注入且 heartbeat 已启用）
                if let Some(ref orch) = self.orchestrator {
                    self.run_heartbeat_for_agents(orch).await;
                }

                last_health_check = Instant::now();
            }

            // 7. stuck run 检测
            if let Err(e) = store::timeout_stuck_runs(&self.pool, STUCK_RUN_THRESHOLD_SECS).await {
                log::error!("stuck run 检测失败: {}", e);
            }

            // 8. anti-spin
            tokio::time::sleep(Duration::from_millis(MIN_REFIRE_GAP_MS)).await;
        }

        // 优雅退出
        log::info!("等待执行中的任务完成...");
        let _ = tokio::time::timeout(
            Duration::from_secs(10),
            self.semaphore.acquire_many(3),
        ).await;
        if let Err(e) = store::cancel_running_runs(&self.pool).await {
            log::error!("退出时取消 run 失败: {}", e);
        }
        log::info!("调度引擎已停止");
    }

    /// Guardrails 检查
    async fn check_guardrails(&self, job: &CronJob) -> bool {
        // anti-spin
        if let Some(last) = job.last_run_at {
            let gap = chrono::Utc::now().timestamp() - last;
            if gap < (MIN_REFIRE_GAP_MS / 1000) as i64 {
                return false;
            }
        }

        // max_concurrent
        if let Ok(running) = store::count_running(&self.pool, &job.id).await {
            if running >= job.guardrails.max_concurrent {
                log::debug!("任务 {} 达到最大并发 {}", job.name, job.guardrails.max_concurrent);
                return false;
            }
        }

        // cooldown
        if job.guardrails.cooldown_secs > 0 {
            if let Some(last) = job.last_run_at {
                let elapsed = chrono::Utc::now().timestamp() - last;
                if elapsed < job.guardrails.cooldown_secs as i64 {
                    return false;
                }
            }
        }

        // max_daily_runs
        if let Some(max) = job.guardrails.max_daily_runs {
            if job.runs_today >= max {
                log::debug!("任务 {} 达到每日上限 {}", job.name, max);
                return false;
            }
        }

        // max_consecutive_failures → 自动 disable
        if job.fail_streak >= job.guardrails.max_consecutive_failures {
            log::warn!("任务 {} 连续失败 {} 次，自动禁用", job.name, job.fail_streak);
            let _ = store::disable_job(&self.pool, &job.id).await;
            return false;
        }

        true
    }

    /// spawn 执行任务
    async fn spawn_job(&self, job: CronJob) {
        let permit = match self.semaphore.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                log::warn!("全局并发已满，跳过任务 {}", job.name);
                return;
            }
        };

        let pool = self.pool.clone();
        let runner = self.runner.clone();
        let app_handle = self.app_handle.clone();
        let notify = self.notify.clone();

        tokio::spawn(async move {
            let now = chrono::Utc::now().timestamp();
            let run_id = uuid::Uuid::new_v4().to_string();

            // 记录 running
            let run = CronRun {
                id: run_id.clone(),
                job_id: job.id.clone(),
                scheduled_at: job.next_run_at.unwrap_or(now),
                started_at: Some(now),
                finished_at: None,
                status: RunStatus::Running,
                trigger_source: TriggerSource::Schedule,
                attempt: 1,
                output: None,
                error: None,
            };
            let _ = store::record_run(&pool, &run).await;

            // 执行（带重试）
            let (result, attempt) = runner.execute_with_retry(&job).await;

            // 更新 run 状态
            let (status, output, error) = match &result {
                ExecResult::Success { output } => (RunStatus::Success, Some(output.as_str()), None),
                ExecResult::Failed { error } => (RunStatus::Failed, None, Some(error.as_str())),
                ExecResult::Timeout => (RunStatus::Timeout, None, Some("执行超时")),
            };
            let _ = store::update_run_status(&pool, &run_id, status, output, error).await;

            // 更新 job 状态
            match &result {
                ExecResult::Success { .. } => {
                    let _ = store::reset_fail_streak(&pool, &job.id).await;
                }
                _ => {
                    let _ = store::increment_fail_streak(&pool, &job.id).await;
                }
            }
            let _ = store::increment_daily_counter(&pool, &job.id).await;

            // 计算下次执行时间
            let finished = chrono::Utc::now().timestamp();
            if let Ok(Some(next)) = planner::next_run_after(&job.schedule, finished) {
                let _ = store::update_next_run(&pool, &job.id, next, finished).await;
            } else if job.delete_after_run {
                let _ = store::delete_job(&pool, &job.id).await;
            }

            // 通知前端
            let _ = app_handle.emit_all("cron-run-complete", &serde_json::json!({
                "jobId": job.id,
                "runId": run_id,
                "status": status.to_string(),
                "attempt": attempt,
            }));

            notify.notify_one();
            drop(permit);
        });
    }

    /// 重新调度（跳过执行但更新 next_run_at）
    async fn reschedule_job(&self, job: &CronJob) {
        let now = chrono::Utc::now().timestamp();
        if let Ok(Some(next)) = planner::next_run_after(&job.schedule, now) {
            let _ = store::update_next_run(&self.pool, &job.id, next, now).await;
        }
    }

    /// Recovery scan：补执行错过的任务
    async fn recovery_scan(&self) {
        log::info!("执行 recovery scan...");
        let now = chrono::Utc::now().timestamp();
        let missed = match store::due_jobs(&self.pool, now).await {
            Ok(jobs) => jobs,
            Err(_) => return,
        };

        for job in missed {
            if job.misfire_policy == "skip" {
                if let Ok(Some(next)) = planner::next_run_after(&job.schedule, now) {
                    let _ = store::update_next_run(&self.pool, &job.id, next, now).await;
                }
                continue;
            }

            // catch_up: stagger 防雷群
            let stagger_ms = {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                job.id.hash(&mut hasher);
                (hasher.finish() % 5000) as u64
            };
            tokio::time::sleep(Duration::from_millis(stagger_ms)).await;

            self.spawn_job(job).await;
        }
    }

    /// 程序化健康检查
    async fn health_check(&self) {
        let report = self.build_health_report().await;
        if report.has_issues() {
            log::warn!("健康检查发现问题: stuck={}, high_fail={}, disabled={}",
                report.stuck_runs.len(), report.high_fail_jobs.len(), report.auto_disabled_jobs.len());
            let _ = self.app_handle.emit_all("heartbeat-alert", &report);
        }
    }

    /// Poll 变化检测：请求 URL，比较内容 hash
    async fn check_poll_change(
        &self,
        url: &str,
        json_path: Option<&str>,
        last_hash: Option<&str>,
        job_id: &str,
    ) -> Result<bool, String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

        let resp = client.get(url).send().await
            .map_err(|e| format!("请求失败: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }

        let body = resp.text().await
            .map_err(|e| format!("读取响应失败: {}", e))?;

        // 提取关注的内容
        let content = if let Some(path) = json_path {
            // 简单的 JSON path 支持：$.key.subkey 格式
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                let parts: Vec<&str> = path.trim_start_matches("$.").split('.').collect();
                let mut current = &json;
                for part in &parts {
                    if current.is_null() { break; }
                    current = &current[*part];
                }
                if current.is_null() {
                    log::warn!("Poll JSON path '{}' 未找到，使用完整 body", path);
                    body.clone()
                } else {
                    current.to_string()
                }
            } else {
                body.clone()
            }
        } else {
            body
        };

        // 计算 hash
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut hasher);
        let new_hash = format!("{:x}", hasher.finish());

        // 从 DB 读取上次 hash（优先使用 poll_last_hash 列）
        let db_last_hash: Option<String> = match sqlx::query_scalar::<_, Option<String>>(
            "SELECT poll_last_hash FROM cron_jobs WHERE id = ?"
        ).bind(job_id).fetch_optional(&self.pool).await {
            Ok(row) => row.flatten(),
            Err(e) => {
                log::warn!("Poll 读取 last_hash 失败: {}，视为首次检测", e);
                None
            }
        };

        let effective_last = db_last_hash.as_deref().or(last_hash);
        let changed = effective_last.map_or(true, |old| old != new_hash);

        // 始终更新 hash 到 DB（无论是否有变化，确保状态一致）
        if let Err(e) = sqlx::query("UPDATE cron_jobs SET poll_last_hash = ? WHERE id = ?")
            .bind(&new_hash).bind(job_id)
            .execute(&self.pool).await
        {
            log::warn!("Poll 更新 hash 失败: {}", e);
        }

        if changed {
            log::info!("Poll 检测到变化: job={}, hash {} → {}", job_id, effective_last.unwrap_or("(none)"), new_hash);
        }

        Ok(changed)
    }

    /// 为每个启用心跳的 Agent 运行 LLM 心跳
    async fn run_heartbeat_for_agents(&self, orchestrator: &Arc<crate::agent::Orchestrator>) {
        // 从 DB 读取全局心跳配置
        let config_json: Option<String> = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = 'heartbeat_config'"
        ).fetch_optional(&self.pool).await.ok().flatten();

        let config: HeartbeatConfig = config_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        if !config.enabled {
            return;
        }

        // 获取所有 Agent，对每个有 workspace 的 Agent 运行心跳
        let agents = orchestrator.list_agents().await.unwrap_or_default();
        for agent in &agents {
            // 从 DB 获取 workspace_path
            let wp: Option<String> = sqlx::query_scalar(
                "SELECT workspace_path FROM agents WHERE id = ?"
            ).bind(&agent.id).fetch_optional(&self.pool).await.ok().flatten();

            if let Some(wp) = wp {
                let workspace_dir = std::path::PathBuf::from(&wp);
                if workspace_dir.join("HEARTBEAT.md").exists() {
                    log::debug!("运行 Agent {} 的 LLM 心跳", agent.name);
                    if let Err(e) = super::heartbeat::llm_heartbeat(
                        &self.pool, orchestrator, &workspace_dir, &config, &self.app_handle,
                    ).await {
                        log::warn!("Agent {} LLM 心跳失败: {}", agent.name, e);
                    }
                }
            }
        }
    }

    /// 构建健康报告
    pub async fn build_health_report(&self) -> HealthReport {
        let stuck = store::timeout_stuck_runs(&self.pool, STUCK_RUN_THRESHOLD_SECS).await.unwrap_or(0);
        let high_fail = store::high_fail_jobs(&self.pool, 3).await.unwrap_or_default();
        let disabled = store::auto_disabled_jobs(&self.pool).await.unwrap_or_default();
        let failure_rate = store::recent_failure_rate(&self.pool, 3600).await.unwrap_or(0.0);

        HealthReport {
            scheduler_alive: true,
            stuck_runs: if stuck > 0 {
                vec![format!("{} 个 stuck run 已超时", stuck)]
            } else {
                vec![]
            },
            high_fail_jobs: high_fail,
            auto_disabled_jobs: disabled,
            recent_failure_rate: failure_rate,
        }
    }
}
