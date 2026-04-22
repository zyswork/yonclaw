//! 默认种子任务
//!
//! 首次启动时注入预设的定时任务，仅在 cron_jobs 表为空时执行

use sqlx::SqlitePool;
use super::types::*;
use super::store;

/// 注入默认种子任务（仅在无任务时执行）
pub async fn seed_default_jobs(pool: &SqlitePool) -> Result<(), String> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cron_jobs")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("查询任务数量失败: {}", e))?;

    if count > 0 {
        log::info!("已有 {} 个定时任务，跳过种子注入", count);
        return Ok(());
    }

    // Agent 类型任务需要 agent_id，查找默认 agent
    let default_agent_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM agents ORDER BY created_at ASC LIMIT 1"
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("查询默认 Agent 失败: {}", e))?;

    if default_agent_id.is_none() {
        log::info!("尚无 Agent，跳过种子任务注入（创建 Agent 后重新注入）");
        return Ok(());
    }

    log::info!("首次启动，注入默认种子任务...");

    let jobs = vec![
        // 每日记忆整理：凌晨 2 点
        CreateJobRequest {
            name: "每日记忆整理".into(),
            agent_id: default_agent_id.clone(),
            job_type: JobType::Agent,
            schedule: Schedule::Cron {
                expr: "0 2 * * *".into(),
                tz: "Asia/Shanghai".into(),
            },
            action_payload: ActionPayload::Agent {
                prompt: "请回顾过去24小时的对话记录，提取关键信息和重要决策，整理成结构化的长期记忆摘要。重点关注：用户偏好变化、新学到的知识、待跟进的事项。".into(),
                session_strategy: "new".into(), model: None, thinking: None,
            },
            timeout_secs: 300,
            guardrails: Guardrails::default(),
            retry: RetryConfig::default(),
            misfire_policy: "skip".into(),
            catch_up_limit: 1,
            delete_after_run: false,
        },
        // 系统健康巡检：每 30 分钟
        CreateJobRequest {
            name: "系统健康巡检".into(),
            agent_id: default_agent_id.clone(),
            job_type: JobType::Agent,
            schedule: Schedule::Every { secs: 1800 },
            action_payload: ActionPayload::Agent {
                prompt: "执行系统健康检查：检查内存使用、数据库连接状态、最近任务执行情况。如发现异常，输出简要报告。".into(),
                session_strategy: "new".into(), model: None, thinking: None,
            },
            timeout_secs: 120,
            guardrails: Guardrails {
                max_concurrent: 1,
                cooldown_secs: 600,
                max_daily_runs: Some(48),
                max_consecutive_failures: 10,
            },
            retry: RetryConfig::default(),
            misfire_policy: "skip".into(),
            catch_up_limit: 1,
            delete_after_run: false,
        },
        // 每周自我复盘：周一凌晨 3 点
        CreateJobRequest {
            name: "每周自我复盘".into(),
            agent_id: default_agent_id.clone(),
            job_type: JobType::Agent,
            schedule: Schedule::Cron {
                expr: "0 3 * * 1".into(),
                tz: "Asia/Shanghai".into(),
            },
            action_payload: ActionPayload::Agent {
                prompt: "复盘过去一周的任务执行情况：统计成功/失败次数，分析失败原因，提出改进建议。输出结构化的周报摘要。".into(),
                session_strategy: "new".into(), model: None, thinking: None,
            },
            timeout_secs: 600,
            guardrails: Guardrails::default(),
            retry: RetryConfig::default(),
            misfire_policy: "skip".into(),
            catch_up_limit: 1,
            delete_after_run: false,
        },
        // Character eval：每周日凌晨 4 点（Hermes character eval）
        CreateJobRequest {
            name: "周日人格一致性评估".into(),
            agent_id: default_agent_id.clone(),
            job_type: JobType::Agent,
            schedule: Schedule::Cron {
                expr: "0 4 * * 0".into(),
                tz: "Asia/Shanghai".into(),
            },
            action_payload: ActionPayload::Agent {
                // 注意：背景 session 不走前端 slash parser，直接用工具描述
                prompt: "请使用 memory_write 工具做以下事：\n\
                    1. 分析你最近一周的对话风格、是否忠实于你的人格设定、是否有 persona drift；\n\
                    2. 若发现偏离，写一条 category=core 的记忆，标题'persona drift 观察 {YYYY-MM-DD}'；\n\
                    3. 若无明显偏离，只回复'本周人格稳定，无需调整'，不要调用工具。".into(),
                session_strategy: "new".into(), model: None, thinking: None,
            },
            timeout_secs: 300,
            guardrails: Guardrails::default(),
            retry: RetryConfig::default(),
            misfire_policy: "skip".into(),
            catch_up_limit: 1,
            delete_after_run: false,
        },
        // Dreaming Light Sleep：每日凌晨 3 点，直接调 run_dream_phase
        CreateJobRequest {
            name: "每日 Light Sleep 记忆整理".into(),
            agent_id: default_agent_id.clone(),
            job_type: JobType::Agent,
            schedule: Schedule::Cron {
                expr: "0 3 * * *".into(),
                tz: "Asia/Shanghai".into(),
            },
            action_payload: ActionPayload::Dreaming { phase: "light".into() },
            timeout_secs: 120,
            guardrails: Guardrails::default(),
            retry: RetryConfig::default(),
            misfire_policy: "skip".into(),
            catch_up_limit: 1,
            delete_after_run: false,
        },
        // Dreaming REM Sleep：每周日凌晨 3:30，深度模式提炼
        CreateJobRequest {
            name: "每周 REM Sleep 深度记忆整理".into(),
            agent_id: default_agent_id.clone(),
            job_type: JobType::Agent,
            schedule: Schedule::Cron {
                expr: "30 3 * * 0".into(),
                tz: "Asia/Shanghai".into(),
            },
            action_payload: ActionPayload::Dreaming { phase: "rem".into() },
            timeout_secs: 180,
            guardrails: Guardrails::default(),
            retry: RetryConfig::default(),
            misfire_policy: "skip".into(),
            catch_up_limit: 1,
            delete_after_run: false,
        },
    ];

    for req in &jobs {
        store::add_job(pool, req).await?;
        log::info!("  ✓ 种子任务已创建: {}", req.name);
    }

    log::info!("✓ {} 个种子任务注入完成", jobs.len());
    Ok(())
}
