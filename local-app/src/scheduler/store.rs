//! 定时任务数据库 CRUD 操作

use sqlx::SqlitePool;
use sqlx::Row;

use super::types::*;

/// 添加定时任务
pub async fn add_job(pool: &SqlitePool, req: &CreateJobRequest) -> Result<CronJob, String> {
    // 校验 schedule 合法性
    super::planner::validate_schedule(&req.schedule)?;

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let job_type_str = req.job_type.to_string();
    let payload_json = serde_json::to_string(&req.action_payload)
        .map_err(|e| format!("序列化 payload 失败: {}", e))?;

    // 解构 schedule，提取扩展字段
    let (schedule_kind, cron_expr, every_secs, at_ts, timezone, webhook_secret, poll_json_path) = match &req.schedule {
        Schedule::Cron { expr, tz } => ("cron", Some(expr.clone()), None, None, tz.clone(), None, None),
        Schedule::Every { secs } => ("every", None, Some(*secs as i64), None, "UTC".to_string(), None, None),
        Schedule::At { ts } => ("at", None, None, Some(*ts), "UTC".to_string(), None, None),
        Schedule::Webhook { token, secret } => ("webhook", Some(token.clone()), None, None, "UTC".to_string(), secret.clone(), None),
        Schedule::Poll { url, interval_secs, json_path, .. } => ("poll", Some(url.clone()), Some(*interval_secs as i64), None, "UTC".to_string(), None, json_path.clone()),
        Schedule::OnMessage { channel, keyword_pattern, .. } => ("on_message", Some(channel.clone()), None, None, "UTC".to_string(), None, keyword_pattern.clone()),
        Schedule::OnAgentEvent { source_agent, event_type } => ("on_agent_event", Some(format!("{}:{}", source_agent, event_type)), None, None, "UTC".to_string(), None, None),
    };

    // 计算首次执行时间
    let next_run = super::planner::next_run_after(&req.schedule, now)
        .map_err(|e| format!("计算首次执行时间失败: {}", e))?;

    sqlx::query(
        "INSERT INTO cron_jobs (id, name, agent_id, job_type, schedule_kind, cron_expr,
         every_secs, at_ts, timezone, action_payload, timeout_secs, max_concurrent,
         cooldown_secs, max_daily_runs, max_consecutive_failures, retry_max,
         retry_base_delay_ms, retry_backoff_factor, misfire_policy, catch_up_limit,
         delete_after_run, created_at, updated_at, next_run_at, webhook_secret, poll_json_path)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)"
    )
    .bind(&id).bind(&req.name).bind(&req.agent_id).bind(&job_type_str)
    .bind(schedule_kind).bind(&cron_expr).bind(every_secs).bind(at_ts)
    .bind(&timezone).bind(&payload_json).bind(req.timeout_secs as i64)
    .bind(req.guardrails.max_concurrent as i64).bind(req.guardrails.cooldown_secs as i64)
    .bind(req.guardrails.max_daily_runs.map(|v| v as i64))
    .bind(req.guardrails.max_consecutive_failures as i64)
    .bind(req.retry.max_attempts as i64).bind(req.retry.base_delay_ms as i64)
    .bind(req.retry.backoff_factor).bind(&req.misfire_policy)
    .bind(req.catch_up_limit as i64).bind(req.delete_after_run as i64)
    .bind(now).bind(now).bind(next_run)
    .bind(&webhook_secret).bind(&poll_json_path)
    .execute(pool).await.map_err(|e| format!("插入任务失败: {}", e))?;

    get_job(pool, &id).await
}

/// 从 DB row 构建 CronJob
fn row_to_job(row: &sqlx::sqlite::SqliteRow) -> Result<CronJob, String> {
    let schedule_kind: String = row.try_get("schedule_kind").map_err(|e| e.to_string())?;
    let schedule = match schedule_kind.as_str() {
        "cron" => Schedule::Cron {
            expr: row.try_get("cron_expr").map_err(|e| e.to_string())?,
            tz: row.try_get("timezone").map_err(|e| e.to_string())?,
        },
        "every" => Schedule::Every {
            secs: row.try_get::<i64, _>("every_secs").map_err(|e| e.to_string())? as u64,
        },
        "at" => Schedule::At {
            ts: row.try_get("at_ts").map_err(|e| e.to_string())?,
        },
        "webhook" => {
            let token: String = row.try_get("cron_expr").map_err(|e| e.to_string())?;
            let secret: Option<String> = row.try_get("webhook_secret").unwrap_or(None);
            Schedule::Webhook { token, secret }
        },
        "poll" => {
            let url: String = row.try_get("cron_expr").map_err(|e| e.to_string())?;
            let interval_secs = row.try_get::<i64, _>("every_secs").map_err(|e| e.to_string())? as u64;
            let json_path: Option<String> = row.try_get("poll_json_path").unwrap_or(None);
            let last_hash: Option<String> = row.try_get("poll_last_hash").unwrap_or(None);
            Schedule::Poll { url, interval_secs, json_path, last_hash }
        },
        _ => return Err(format!("未知 schedule_kind: {}", schedule_kind)),
    };

    let job_type_str: String = row.try_get("job_type").map_err(|e| e.to_string())?;
    let job_type: JobType = job_type_str.parse()?;

    let payload_json: String = row.try_get("action_payload").map_err(|e| e.to_string())?;
    let action_payload: ActionPayload = serde_json::from_str(&payload_json)
        .map_err(|e| format!("反序列化 payload 失败: {}", e))?;

    Ok(CronJob {
        id: row.try_get("id").map_err(|e| e.to_string())?,
        name: row.try_get("name").map_err(|e| e.to_string())?,
        agent_id: row.try_get("agent_id").map_err(|e| e.to_string())?,
        job_type,
        schedule,
        action_payload,
        timeout_secs: row.try_get::<i64, _>("timeout_secs").map_err(|e| e.to_string())? as u32,
        guardrails: Guardrails {
            max_concurrent: row.try_get::<i64, _>("max_concurrent").map_err(|e| e.to_string())? as u32,
            cooldown_secs: row.try_get::<i64, _>("cooldown_secs").map_err(|e| e.to_string())? as u32,
            max_daily_runs: row.try_get::<Option<i64>, _>("max_daily_runs")
                .map_err(|e| e.to_string())?.map(|v| v as u32),
            max_consecutive_failures: row.try_get::<i64, _>("max_consecutive_failures")
                .map_err(|e| e.to_string())? as u32,
        },
        retry: RetryConfig {
            max_attempts: row.try_get::<i64, _>("retry_max").map_err(|e| e.to_string())? as u32,
            base_delay_ms: row.try_get::<i64, _>("retry_base_delay_ms").map_err(|e| e.to_string())? as u64,
            backoff_factor: row.try_get("retry_backoff_factor").map_err(|e| e.to_string())?,
        },
        misfire_policy: row.try_get("misfire_policy").map_err(|e| e.to_string())?,
        catch_up_limit: row.try_get::<i64, _>("catch_up_limit").map_err(|e| e.to_string())? as u32,
        enabled: row.try_get::<i64, _>("enabled").map_err(|e| e.to_string())? != 0,
        fail_streak: row.try_get::<i64, _>("fail_streak").map_err(|e| e.to_string())? as u32,
        runs_today: row.try_get::<i64, _>("runs_today").map_err(|e| e.to_string())? as u32,
        next_run_at: row.try_get("next_run_at").map_err(|e| e.to_string())?,
        last_run_at: row.try_get("last_run_at").map_err(|e| e.to_string())?,
        delete_after_run: row.try_get::<i64, _>("delete_after_run").map_err(|e| e.to_string())? != 0,
        created_at: row.try_get("created_at").map_err(|e| e.to_string())?,
        updated_at: row.try_get("updated_at").map_err(|e| e.to_string())?,
    })
}

/// 获取单个任务
pub async fn get_job(pool: &SqlitePool, job_id: &str) -> Result<CronJob, String> {
    let row = sqlx::query("SELECT * FROM cron_jobs WHERE id = ?")
        .bind(job_id)
        .fetch_one(pool).await
        .map_err(|e| format!("查询任务失败: {}", e))?;
    row_to_job(&row)
}

/// 列出任务
pub async fn list_jobs(pool: &SqlitePool, filter: Option<&JobFilter>) -> Result<Vec<CronJob>, String> {
    let mut sql = String::from("SELECT * FROM cron_jobs WHERE 1=1");
    let mut agent_id_bind: Option<&str> = None;
    let mut enabled_bind: Option<i64> = None;
    let mut job_type_bind: Option<String> = None;
    if let Some(f) = filter {
        if let Some(ref agent_id) = f.agent_id {
            sql.push_str(" AND agent_id = ?");
            agent_id_bind = Some(agent_id.as_str());
        }
        if let Some(enabled) = f.enabled {
            sql.push_str(" AND enabled = ?");
            enabled_bind = Some(enabled as i64);
        }
        if let Some(ref jt) = f.job_type {
            sql.push_str(" AND job_type = ?");
            job_type_bind = Some(jt.to_string());
        }
    }
    sql.push_str(" ORDER BY created_at DESC");

    let mut q = sqlx::query(&sql);
    if let Some(a) = agent_id_bind { q = q.bind(a); }
    if let Some(e) = enabled_bind { q = q.bind(e); }
    if let Some(ref j) = job_type_bind { q = q.bind(j); }

    let rows = q.fetch_all(pool).await
        .map_err(|e| format!("查询任务列表失败: {}", e))?;
    rows.iter().map(row_to_job).collect()
}

/// 更新任务
pub async fn update_job(pool: &SqlitePool, job_id: &str, patch: &UpdateJobRequest) -> Result<CronJob, String> {
    let now = chrono::Utc::now().timestamp();
    let mut sets = vec!["updated_at = ?".to_string()];
    let mut need_reschedule = false;

    // 动态构建 SET 子句
    if patch.name.is_some() { sets.push("name = ?".to_string()); }
    if patch.timeout_secs.is_some() { sets.push("timeout_secs = ?".to_string()); }
    if patch.misfire_policy.is_some() { sets.push("misfire_policy = ?".to_string()); }
    if patch.catch_up_limit.is_some() { sets.push("catch_up_limit = ?".to_string()); }
    if patch.enabled.is_some() { sets.push("enabled = ?".to_string()); }

    // 简化：直接用完整 UPDATE 语句
    let existing = get_job(pool, job_id).await?;
    let name = patch.name.as_deref().unwrap_or(&existing.name);
    let timeout = patch.timeout_secs.unwrap_or(existing.timeout_secs);
    let misfire = patch.misfire_policy.as_deref().unwrap_or(&existing.misfire_policy);
    let catch_up = patch.catch_up_limit.unwrap_or(existing.catch_up_limit);
    let enabled = patch.enabled.unwrap_or(existing.enabled);

    let schedule = patch.schedule.as_ref().unwrap_or(&existing.schedule);
    let (schedule_kind, cron_expr, every_secs, at_ts, timezone, webhook_secret, poll_json_path) = match schedule {
        Schedule::Cron { expr, tz } => ("cron", Some(expr.clone()), None, None, tz.clone(), None, None),
        Schedule::Every { secs } => ("every", None, Some(*secs as i64), None, "UTC".to_string(), None, None),
        Schedule::At { ts } => ("at", None, None, Some(*ts), "UTC".to_string(), None, None),
        Schedule::Webhook { token, secret } => ("webhook", Some(token.clone()), None, None, "UTC".to_string(), secret.clone(), None),
        Schedule::Poll { url, interval_secs, json_path, .. } => ("poll", Some(url.clone()), Some(*interval_secs as i64), None, "UTC".to_string(), None, json_path.clone()),
        Schedule::OnMessage { channel, keyword_pattern, .. } => ("on_message", Some(channel.clone()), None, None, "UTC".to_string(), None, keyword_pattern.clone()),
        Schedule::OnAgentEvent { source_agent, event_type } => ("on_agent_event", Some(format!("{}:{}", source_agent, event_type)), None, None, "UTC".to_string(), None, None),
    };

    if patch.schedule.is_some() { need_reschedule = true; }

    let payload = patch.action_payload.as_ref().unwrap_or(&existing.action_payload);
    let payload_json = serde_json::to_string(payload).map_err(|e| e.to_string())?;

    let guardrails = patch.guardrails.as_ref().unwrap_or(&existing.guardrails);
    let retry = patch.retry.as_ref().unwrap_or(&existing.retry);

    let next_run = if need_reschedule {
        super::planner::next_run_after(schedule, now).unwrap_or(None)
    } else {
        existing.next_run_at
    };

    sqlx::query(
        "UPDATE cron_jobs SET name=?, schedule_kind=?, cron_expr=?, every_secs=?, at_ts=?,
         timezone=?, action_payload=?, timeout_secs=?, max_concurrent=?, cooldown_secs=?,
         max_daily_runs=?, max_consecutive_failures=?, retry_max=?, retry_base_delay_ms=?,
         retry_backoff_factor=?, misfire_policy=?, catch_up_limit=?, enabled=?,
         next_run_at=?, updated_at=?, webhook_secret=?, poll_json_path=? WHERE id=?"
    )
    .bind(name).bind(schedule_kind).bind(&cron_expr).bind(every_secs).bind(at_ts)
    .bind(&timezone).bind(&payload_json).bind(timeout as i64)
    .bind(guardrails.max_concurrent as i64).bind(guardrails.cooldown_secs as i64)
    .bind(guardrails.max_daily_runs.map(|v| v as i64))
    .bind(guardrails.max_consecutive_failures as i64)
    .bind(retry.max_attempts as i64).bind(retry.base_delay_ms as i64)
    .bind(retry.backoff_factor).bind(misfire).bind(catch_up as i64)
    .bind(enabled as i64).bind(next_run).bind(now)
    .bind(&webhook_secret).bind(&poll_json_path).bind(job_id)
    .execute(pool).await.map_err(|e| format!("更新任务失败: {}", e))?;

    get_job(pool, job_id).await
}

/// 删除任务
pub async fn delete_job(pool: &SqlitePool, job_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM cron_jobs WHERE id = ?")
        .bind(job_id)
        .execute(pool).await
        .map_err(|e| format!("删除任务失败: {}", e))?;
    Ok(())
}

// ─── 调度相关 ─────────────────────────────────────────────────

/// 查询最早到期时间
pub async fn earliest_next_run(pool: &SqlitePool) -> Result<Option<i64>, String> {
    let row = sqlx::query("SELECT MIN(next_run_at) as min_ts FROM cron_jobs WHERE enabled = 1 AND next_run_at IS NOT NULL")
        .fetch_one(pool).await
        .map_err(|e| format!("查询最早到期时间失败: {}", e))?;
    Ok(row.try_get("min_ts").unwrap_or(None))
}

/// 查询到期任务
pub async fn due_jobs(pool: &SqlitePool, now: i64) -> Result<Vec<CronJob>, String> {
    let rows = sqlx::query("SELECT * FROM cron_jobs WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ?")
        .bind(now)
        .fetch_all(pool).await
        .map_err(|e| format!("查询到期任务失败: {}", e))?;
    rows.iter().map(row_to_job).collect()
}

/// 更新下次执行时间
pub async fn update_next_run(pool: &SqlitePool, job_id: &str, next_run: i64, last_run: i64) -> Result<(), String> {
    sqlx::query("UPDATE cron_jobs SET next_run_at = ?, last_run_at = ?, updated_at = ? WHERE id = ?")
        .bind(next_run).bind(last_run).bind(chrono::Utc::now().timestamp()).bind(job_id)
        .execute(pool).await
        .map_err(|e| format!("更新 next_run 失败: {}", e))?;
    Ok(())
}

/// 增加连续失败计数
pub async fn increment_fail_streak(pool: &SqlitePool, job_id: &str) -> Result<u32, String> {
    sqlx::query("UPDATE cron_jobs SET fail_streak = fail_streak + 1, updated_at = ? WHERE id = ?")
        .bind(chrono::Utc::now().timestamp()).bind(job_id)
        .execute(pool).await
        .map_err(|e| format!("更新 fail_streak 失败: {}", e))?;
    let row = sqlx::query("SELECT fail_streak FROM cron_jobs WHERE id = ?")
        .bind(job_id)
        .fetch_one(pool).await
        .map_err(|e| e.to_string())?;
    Ok(row.try_get::<i64, _>("fail_streak").map_err(|e| e.to_string())? as u32)
}

/// 重置连续失败计数
pub async fn reset_fail_streak(pool: &SqlitePool, job_id: &str) -> Result<(), String> {
    sqlx::query("UPDATE cron_jobs SET fail_streak = 0, updated_at = ? WHERE id = ?")
        .bind(chrono::Utc::now().timestamp()).bind(job_id)
        .execute(pool).await
        .map_err(|e| format!("重置 fail_streak 失败: {}", e))?;
    Ok(())
}

/// 禁用任务
pub async fn disable_job(pool: &SqlitePool, job_id: &str) -> Result<(), String> {
    sqlx::query("UPDATE cron_jobs SET enabled = 0, updated_at = ? WHERE id = ?")
        .bind(chrono::Utc::now().timestamp()).bind(job_id)
        .execute(pool).await
        .map_err(|e| format!("禁用任务失败: {}", e))?;
    Ok(())
}

/// 查询正在运行的 run 数量
pub async fn count_running(pool: &SqlitePool, job_id: &str) -> Result<u32, String> {
    let row = sqlx::query("SELECT COUNT(*) as cnt FROM cron_runs WHERE job_id = ? AND status = 'running'")
        .bind(job_id)
        .fetch_one(pool).await
        .map_err(|e| e.to_string())?;
    Ok(row.try_get::<i64, _>("cnt").map_err(|e| e.to_string())? as u32)
}

/// 重置每日计数器
pub async fn reset_daily_counter(pool: &SqlitePool, job_id: &str, today: &str) -> Result<(), String> {
    sqlx::query("UPDATE cron_jobs SET runs_today = 0, runs_today_date = ?, updated_at = ? WHERE id = ?")
        .bind(today).bind(chrono::Utc::now().timestamp()).bind(job_id)
        .execute(pool).await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 增加每日计数
pub async fn increment_daily_counter(pool: &SqlitePool, job_id: &str) -> Result<(), String> {
    sqlx::query("UPDATE cron_jobs SET runs_today = runs_today + 1, updated_at = ? WHERE id = ?")
        .bind(chrono::Utc::now().timestamp()).bind(job_id)
        .execute(pool).await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ─── 运行记录 ─────────────────────────────────────────────────

/// 记录一次运行
pub async fn record_run(pool: &SqlitePool, run: &CronRun) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO cron_runs (id, job_id, scheduled_at, started_at, finished_at, status, trigger_source, attempt, output, error)
         VALUES (?,?,?,?,?,?,?,?,?,?)"
    )
    .bind(&run.id).bind(&run.job_id).bind(run.scheduled_at)
    .bind(run.started_at).bind(run.finished_at)
    .bind(run.status.to_string()).bind(run.trigger_source.to_string())
    .bind(run.attempt as i64).bind(&run.output).bind(&run.error)
    .execute(pool).await
    .map_err(|e| format!("记录 run 失败: {}", e))?;
    Ok(())
}

/// 更新运行状态
pub async fn update_run_status(
    pool: &SqlitePool, run_id: &str, status: RunStatus,
    output: Option<&str>, error: Option<&str>,
) -> Result<(), String> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query("UPDATE cron_runs SET status = ?, finished_at = ?, output = ?, error = ? WHERE id = ?")
        .bind(status.to_string()).bind(now).bind(output).bind(error).bind(run_id)
        .execute(pool).await
        .map_err(|e| format!("更新 run 状态失败: {}", e))?;
    Ok(())
}

/// 列出运行记录
pub async fn list_runs(pool: &SqlitePool, job_id: &str, limit: u32) -> Result<Vec<CronRun>, String> {
    let rows = sqlx::query(
        "SELECT * FROM cron_runs WHERE job_id = ? ORDER BY started_at DESC LIMIT ?"
    )
    .bind(job_id).bind(limit as i64)
    .fetch_all(pool).await
    .map_err(|e| format!("查询 run 列表失败: {}", e))?;

    rows.iter().map(|row| {
        let status_str: String = row.try_get("status").map_err(|e| e.to_string())?;
        let trigger_str: String = row.try_get("trigger_source").map_err(|e| e.to_string())?;
        Ok(CronRun {
            id: row.try_get("id").map_err(|e| e.to_string())?,
            job_id: row.try_get("job_id").map_err(|e| e.to_string())?,
            scheduled_at: row.try_get("scheduled_at").map_err(|e| e.to_string())?,
            started_at: row.try_get("started_at").map_err(|e| e.to_string())?,
            finished_at: row.try_get("finished_at").map_err(|e| e.to_string())?,
            status: status_str.parse().map_err(|e: String| e)?,
            trigger_source: trigger_str.parse().map_err(|_| "未知触发来源".to_string())?,
            attempt: row.try_get::<i64, _>("attempt").map_err(|e| e.to_string())? as u32,
            output: row.try_get("output").map_err(|e| e.to_string())?,
            error: row.try_get("error").map_err(|e| e.to_string())?,
        })
    }).collect()
}

/// 超时 stuck runs
pub async fn timeout_stuck_runs(pool: &SqlitePool, threshold_secs: i64) -> Result<u32, String> {
    let cutoff = chrono::Utc::now().timestamp() - threshold_secs;
    let result = sqlx::query(
        "UPDATE cron_runs SET status = 'timeout', finished_at = ?, error = '执行超时（stuck 检测）'
         WHERE status = 'running' AND started_at < ?"
    )
    .bind(chrono::Utc::now().timestamp()).bind(cutoff)
    .execute(pool).await
    .map_err(|e| e.to_string())?;
    Ok(result.rows_affected() as u32)
}

/// 取消所有运行中的 run
pub async fn cancel_running_runs(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query("UPDATE cron_runs SET status = 'cancelled', finished_at = ? WHERE status = 'running'")
        .bind(chrono::Utc::now().timestamp())
        .execute(pool).await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ─── 健康统计 ─────────────────────────────────────────────────

/// 最近失败率
pub async fn recent_failure_rate(pool: &SqlitePool, window_secs: i64) -> Result<f64, String> {
    let cutoff = chrono::Utc::now().timestamp() - window_secs;
    let row = sqlx::query(
        "SELECT COUNT(*) as total,
         SUM(CASE WHEN status IN ('failed','timeout') THEN 1 ELSE 0 END) as failures
         FROM cron_runs WHERE started_at > ?"
    )
    .bind(cutoff)
    .fetch_one(pool).await
    .map_err(|e| e.to_string())?;

    let total: i64 = row.try_get("total").unwrap_or(0);
    let failures: i64 = row.try_get("failures").unwrap_or(0);
    if total == 0 { return Ok(0.0); }
    Ok(failures as f64 / total as f64)
}

/// 高失败率任务
pub async fn high_fail_jobs(pool: &SqlitePool, threshold: u32) -> Result<Vec<String>, String> {
    let rows = sqlx::query("SELECT id FROM cron_jobs WHERE fail_streak >= ? AND enabled = 1")
        .bind(threshold as i64)
        .fetch_all(pool).await
        .map_err(|e| e.to_string())?;
    Ok(rows.iter().map(|r| r.try_get::<String, _>("id").unwrap_or_default()).collect())
}

/// 自动禁用的任务
pub async fn auto_disabled_jobs(pool: &SqlitePool) -> Result<Vec<String>, String> {
    let rows = sqlx::query(
        "SELECT id FROM cron_jobs WHERE enabled = 0 AND fail_streak >= max_consecutive_failures"
    )
    .fetch_all(pool).await
    .map_err(|e| e.to_string())?;
    Ok(rows.iter().map(|r| r.try_get::<String, _>("id").unwrap_or_default()).collect())
}
