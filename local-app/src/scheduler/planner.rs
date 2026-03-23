//! 调度计划器：计算下次执行时间

use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use cron::Schedule as CronSchedule;
use std::str::FromStr;

use super::types::Schedule;

/// 计算下次执行时间（返回 unix 时间戳）
pub fn next_run_after(schedule: &Schedule, after: i64) -> Result<Option<i64>, String> {
    match schedule {
        Schedule::Cron { expr, tz } => {
            let timezone: Tz = tz.parse()
                .map_err(|_| format!("无效时区: {}", tz))?;
            // cron crate 需要 6/7 字段格式（秒 分 时 日 月 周）
            let cron_expr = normalize_cron_expr(expr)?;
            let cron_schedule = CronSchedule::from_str(&cron_expr)
                .map_err(|e| format!("无效 cron 表达式 '{}': {}", expr, e))?;
            let after_dt = Utc.timestamp_opt(after, 0)
                .single()
                .ok_or("无效时间戳")?;
            // 在指定时区中计算下次执行
            let after_tz = after_dt.with_timezone(&timezone);
            let next = cron_schedule.after(&after_tz).next();
            Ok(next.map(|dt| dt.with_timezone(&Utc).timestamp()))
        }
        Schedule::Every { secs } => {
            Ok(Some(after + *secs as i64))
        }
        Schedule::At { ts } => {
            if *ts > after {
                Ok(Some(*ts))
            } else {
                Ok(None)
            }
        }
        Schedule::Webhook { .. } => {
            // Webhook 不需要定时调度，由外部 HTTP 触发
            Ok(None)
        }
        Schedule::Poll { interval_secs, .. } => {
            // 按间隔轮询
            Ok(Some(after + *interval_secs as i64))
        }
    }
}

/// 标准化 cron 表达式：5 字段 → 6 字段（补秒）
fn normalize_cron_expr(expr: &str) -> Result<String, String> {
    let parts: Vec<&str> = expr.trim().split_whitespace().collect();
    match parts.len() {
        5 => Ok(format!("0 {}", expr)),  // 补秒=0
        6 => Ok(expr.to_string()),        // 已有秒
        7 => Ok(expr.to_string()),        // 已有秒和年
        _ => Err(format!("cron 表达式字段数错误({}): {}", parts.len(), expr)),
    }
}

/// 验证调度配置
pub fn validate_schedule(schedule: &Schedule) -> Result<(), String> {
    match schedule {
        Schedule::Cron { expr, tz } => {
            let _: Tz = tz.parse()
                .map_err(|_| format!("无效时区: {}", tz))?;
            let normalized = normalize_cron_expr(expr)?;
            CronSchedule::from_str(&normalized)
                .map_err(|e| format!("无效 cron 表达式: {}", e))?;
            Ok(())
        }
        Schedule::Every { secs } => {
            if *secs < 60 {
                Err("间隔不能小于 60 秒".to_string())
            } else {
                Ok(())
            }
        }
        Schedule::At { ts } => {
            if *ts <= Utc::now().timestamp() {
                Err("一次性定时不能是过去的时间".to_string())
            } else {
                Ok(())
            }
        }
        Schedule::Webhook { token, .. } => {
            if token.is_empty() {
                Err("Webhook token 不能为空".to_string())
            } else {
                Ok(())
            }
        }
        Schedule::Poll { url, interval_secs, .. } => {
            if url.is_empty() {
                return Err("Poll URL 不能为空".to_string());
            }
            if *interval_secs < 60 {
                return Err("Poll 间隔不能小于 60 秒".to_string());
            }
            // SSRF 防护：禁止内网地址
            let url_lower = url.to_lowercase();
            if url_lower.contains("localhost") || url_lower.contains("127.0.0.1")
                || url_lower.contains("0.0.0.0") || url_lower.contains("[::1]")
                || url_lower.starts_with("http://10.") || url_lower.starts_with("http://172.")
                || url_lower.starts_with("http://192.168.")
            {
                return Err("安全限制：Poll URL 不能指向内网地址".to_string());
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cron_next_run() {
        let schedule = Schedule::Cron {
            expr: "0 9 * * *".to_string(),
            tz: "Asia/Shanghai".to_string(),
        };
        let now = Utc::now().timestamp();
        let next = next_run_after(&schedule, now).unwrap();
        assert!(next.is_some());
        assert!(next.unwrap() > now);
    }

    #[test]
    fn test_every_next_run() {
        let schedule = Schedule::Every { secs: 3600 };
        let now = 1000;
        let next = next_run_after(&schedule, now).unwrap();
        assert_eq!(next, Some(4600));
    }

    #[test]
    fn test_at_future() {
        let future_ts = Utc::now().timestamp() + 3600;
        let schedule = Schedule::At { ts: future_ts };
        let now = Utc::now().timestamp();
        let next = next_run_after(&schedule, now).unwrap();
        assert_eq!(next, Some(future_ts));
    }

    #[test]
    fn test_at_past() {
        let past_ts = Utc::now().timestamp() - 3600;
        let schedule = Schedule::At { ts: past_ts };
        let now = Utc::now().timestamp();
        let next = next_run_after(&schedule, now).unwrap();
        assert_eq!(next, None);
    }

    #[test]
    fn test_normalize_5_field() {
        let result = normalize_cron_expr("0 9 * * *").unwrap();
        assert_eq!(result, "0 0 9 * * *");
    }

    #[test]
    fn test_validate_every_too_short() {
        let schedule = Schedule::Every { secs: 10 };
        assert!(validate_schedule(&schedule).is_err());
    }
}
