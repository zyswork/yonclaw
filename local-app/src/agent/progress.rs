//! 会话进度持久化 + 跨会话恢复
//!
//! - agent_loop 关键状态变更时写入 workspace/PROGRESS.md
//! - 新会话开始时检查 PROGRESS.md，提供恢复上下文
//! - 节流写入（最快 5s 一次），避免频繁磁盘 I/O

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

/// 进度追踪器（每个 agent_loop 实例一个）
pub struct ProgressTracker {
    file_path: PathBuf,
    entries: Vec<ProgressEntry>,
    last_write: Mutex<Instant>,
    /// 节流间隔（秒）
    throttle_secs: u64,
}

#[derive(Debug, Clone)]
struct ProgressEntry {
    round: usize,
    tool_name: String,
    success: bool,
    summary: String,
    timestamp: String,
}

impl ProgressTracker {
    /// 从 workspace 路径创建
    pub fn new(workspace_path: &str) -> Self {
        Self {
            file_path: Path::new(workspace_path).join("PROGRESS.md"),
            entries: Vec::new(),
            last_write: Mutex::new(Instant::now()),
            throttle_secs: 5,
        }
    }

    /// 记录一次工具调用结果
    pub fn record(&mut self, round: usize, tool_name: &str, success: bool, summary: &str) {
        let timestamp = chrono::Utc::now().format("%H:%M:%S").to_string();
        let short_summary: String = summary.chars().take(80).collect();
        self.entries.push(ProgressEntry {
            round,
            tool_name: tool_name.to_string(),
            success,
            summary: short_summary,
            timestamp,
        });

        // 节流写入
        if let Ok(mut last) = self.last_write.lock() {
            if last.elapsed().as_secs() >= self.throttle_secs {
                self.flush();
                *last = Instant::now();
            }
        }
    }

    /// 标记任务完成
    pub fn mark_complete(&mut self) {
        let timestamp = chrono::Utc::now().format("%H:%M:%S").to_string();
        self.entries.push(ProgressEntry {
            round: 0,
            tool_name: "_complete".to_string(),
            success: true,
            summary: "任务执行完成".to_string(),
            timestamp,
        });
        self.flush();
    }

    /// 写入 PROGRESS.md
    fn flush(&self) {
        let mut md = String::from("# Progress\n\n");
        md.push_str(&format!("> Last updated: {}\n\n", chrono::Utc::now().to_rfc3339()));

        for entry in &self.entries {
            let icon = if entry.tool_name == "_complete" {
                "🏁"
            } else if entry.success {
                "✓"
            } else {
                "✗"
            };
            md.push_str(&format!(
                "- [{}] [{}] `{}` — {}\n",
                icon, entry.timestamp, entry.tool_name, entry.summary
            ));
        }

        if let Err(e) = std::fs::write(&self.file_path, &md) {
            log::warn!("progress: 写入失败: {}", e);
        }
    }

    /// 清除进度文件（任务完成后）
    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.file_path);
    }
}

/// 检查是否有未完成的进度（用于跨会话恢复）
///
/// 返回 Some(恢复上下文) 如果 PROGRESS.md 存在且有未完成项
pub fn check_pending_progress(workspace_path: &str) -> Option<String> {
    let path = Path::new(workspace_path).join("PROGRESS.md");
    let content = std::fs::read_to_string(&path).ok()?;

    // 如果包含完成标记，说明上次任务已完成
    if content.contains("_complete") {
        return None;
    }

    // 有内容但没有完成标记 → 上次任务中断
    if content.lines().any(|l| l.starts_with("- [")) {
        let recent: String = content.lines()
            .filter(|l| l.starts_with("- ["))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .take(5)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");

        Some(format!(
            "[Session Recovery] 检测到上次任务未完成。最近进度：\n{}\n\n如果当前任务与上次相关，请继续完成；否则请忽略。",
            recent
        ))
    } else {
        None
    }
}
