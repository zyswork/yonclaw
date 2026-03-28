//! 定时任务相关命令

use std::sync::Arc;
use tauri::State;

use crate::scheduler;
use crate::AppState;

#[tauri::command]
pub async fn create_cron_job(
    state: State<'_, Arc<AppState>>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let request: scheduler::CreateJobRequest = serde_json::from_value(payload)
        .map_err(|e| format!("参数错误: {}", e))?;
    let sched = state.scheduler.get().ok_or("调度器未初始化")?;
    let job = scheduler::store::add_job(sched.pool(), &request).await?;
    sched.wake();
    serde_json::to_value(&job).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_cron_job(
    state: State<'_, Arc<AppState>>,
    job_id: String,
    patch: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let request: scheduler::UpdateJobRequest = serde_json::from_value(patch)
        .map_err(|e| format!("参数错误: {}", e))?;
    let sched = state.scheduler.get().ok_or("调度器未初始化")?;
    let job = scheduler::store::update_job(sched.pool(), &job_id, &request).await?;
    sched.wake();
    serde_json::to_value(&job).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_cron_job(
    state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<(), String> {
    let sched = state.scheduler.get().ok_or("调度器未初始化")?;
    scheduler::store::delete_job(sched.pool(), &job_id).await?;
    sched.wake();
    Ok(())
}

#[tauri::command]
pub async fn list_cron_jobs(
    state: State<'_, Arc<AppState>>,
    agent_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let pool = state.scheduler.get().ok_or("调度器未初始化")?.pool();
    let filter = agent_id.map(|id| scheduler::JobFilter {
        agent_id: Some(id),
        ..Default::default()
    });
    let jobs = scheduler::store::list_jobs(pool, filter.as_ref()).await?;
    serde_json::to_value(&jobs).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_cron_job(
    state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<serde_json::Value, String> {
    let pool = state.scheduler.get().ok_or("调度器未初始化")?.pool();
    let job = scheduler::store::get_job(pool, &job_id).await?;
    serde_json::to_value(&job).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn trigger_cron_job(
    state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<(), String> {
    let sched = state.scheduler.get().ok_or("调度器未初始化")?;
    let job = scheduler::store::get_job(sched.pool(), &job_id).await?;
    let now = chrono::Utc::now().timestamp();
    scheduler::store::update_next_run(sched.pool(), &job_id, now, job.last_run_at.unwrap_or(0)).await?;
    sched.wake();
    Ok(())
}

#[tauri::command]
pub async fn pause_cron_job(
    state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<(), String> {
    let sched = state.scheduler.get().ok_or("调度器未初始化")?;
    scheduler::store::disable_job(sched.pool(), &job_id).await?;
    sched.wake();
    Ok(())
}

#[tauri::command]
pub async fn resume_cron_job(
    state: State<'_, Arc<AppState>>,
    job_id: String,
) -> Result<(), String> {
    let sched = state.scheduler.get().ok_or("调度器未初始化")?;
    let patch = scheduler::UpdateJobRequest {
        name: None, schedule: None, action_payload: None,
        timeout_secs: None, guardrails: None, retry: None,
        misfire_policy: None, catch_up_limit: None,
        enabled: Some(true),
    };
    scheduler::store::update_job(sched.pool(), &job_id, &patch).await?;
    sched.wake();
    Ok(())
}

#[tauri::command]
pub async fn list_cron_runs(
    state: State<'_, Arc<AppState>>,
    job_id: String,
    limit: Option<u32>,
) -> Result<serde_json::Value, String> {
    let pool = state.scheduler.get().ok_or("调度器未初始化")?.pool();
    let runs = scheduler::store::list_runs(pool, &job_id, limit.unwrap_or(20)).await?;
    serde_json::to_value(&runs).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_scheduler_status(
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    let pool = state.scheduler.get().ok_or("调度器未初始化")?.pool();
    let jobs = scheduler::store::list_jobs(pool, None).await.unwrap_or_default();
    let failure_rate = scheduler::store::recent_failure_rate(pool, 3600).await.unwrap_or(0.0);
    let running = jobs.iter().filter(|j| j.enabled).count() as u32;

    let status = scheduler::SchedulerStatus {
        running: state.scheduler.get().is_some(),
        total_jobs: jobs.len() as u32,
        enabled_jobs: running,
        running_runs: 0,
        recent_failure_rate: failure_rate,
        last_tick_at: None,
    };
    serde_json::to_value(&status).map_err(|e| e.to_string())
}
