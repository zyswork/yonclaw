//! SOP 执行引擎

use super::types::*;
use std::collections::HashMap;
use std::path::Path;

/// SOP 引擎 — 管理 SOP 定义、触发和执行
pub struct SopEngine {
    /// 已加载的 SOP 定义
    sops: HashMap<String, Sop>,
    /// 活跃的运行实例
    runs: HashMap<String, SopRun>,
    /// 上次执行时间（用于 cooldown）
    last_run: HashMap<String, i64>,
}

impl SopEngine {
    pub fn new() -> Self {
        Self {
            sops: HashMap::new(),
            runs: HashMap::new(),
            last_run: HashMap::new(),
        }
    }

    /// 从目录加载 SOP 定义（SOP.toml + SOP.md）
    pub fn load_from_dir(&mut self, dir: &Path) -> Result<usize, String> {
        let mut count = 0;
        if !dir.exists() { return Ok(0); }

        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                let toml_path = path.join("SOP.toml");
                if toml_path.exists() {
                    match self.load_sop(&toml_path) {
                        Ok(name) => {
                            log::info!("SOP 加载: {}", name);
                            count += 1;
                        }
                        Err(e) => log::warn!("SOP 加载失败 {:?}: {}", toml_path, e),
                    }
                }
            }
        }
        Ok(count)
    }

    /// 加载单个 SOP
    fn load_sop(&mut self, toml_path: &Path) -> Result<String, String> {
        let content = std::fs::read_to_string(toml_path).map_err(|e| e.to_string())?;
        let sop: Sop = toml::from_str(&content).map_err(|e| format!("SOP.toml 解析失败: {}", e))?;
        let name = sop.name.clone();
        self.sops.insert(name.clone(), sop);
        Ok(name)
    }

    /// 注册 SOP（编程方式）
    pub fn register(&mut self, sop: Sop) {
        self.sops.insert(sop.name.clone(), sop);
    }

    /// 列出所有 SOP
    pub fn list(&self) -> Vec<&Sop> {
        self.sops.values().collect()
    }

    /// 获取 SOP
    pub fn get(&self, name: &str) -> Option<&Sop> {
        self.sops.get(name)
    }

    /// 触发 SOP 执行
    pub fn trigger(&mut self, sop_name: &str) -> Result<SopRun, String> {
        let sop = self.sops.get(sop_name).ok_or(format!("SOP '{}' 不存在", sop_name))?;

        // Cooldown 检查
        let now = chrono::Utc::now().timestamp();
        if sop.cooldown_secs > 0 {
            if let Some(last) = self.last_run.get(sop_name) {
                if now - last < sop.cooldown_secs as i64 {
                    return Err(format!("SOP '{}' 在冷却中（{}s 后可再次执行）",
                        sop_name, sop.cooldown_secs as i64 - (now - last)));
                }
            }
        }

        // 并发检查
        let active_count = self.runs.values()
            .filter(|r| r.sop_name == sop_name && matches!(r.status, SopRunStatus::Running | SopRunStatus::WaitingApproval))
            .count();
        if active_count >= sop.max_concurrent as usize {
            return Err(format!("SOP '{}' 已达最大并发数 {}", sop_name, sop.max_concurrent));
        }

        let run = SopRun {
            run_id: format!("sop-{}-{}", sop_name, now),
            sop_name: sop_name.to_string(),
            status: SopRunStatus::Running,
            current_step: 1,
            total_steps: sop.steps.len() as u32,
            started_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
            step_results: Vec::new(),
        };

        self.last_run.insert(sop_name.to_string(), now);
        let run_id = run.run_id.clone();
        self.runs.insert(run_id.clone(), run.clone());

        Ok(run)
    }

    /// 获取当前步骤信息
    pub fn current_step(&self, run_id: &str) -> Option<&SopStep> {
        let run = self.runs.get(run_id)?;
        let sop = self.sops.get(&run.sop_name)?;
        sop.steps.iter().find(|s| s.number == run.current_step)
    }

    /// 提交步骤结果并推进
    pub fn advance(&mut self, run_id: &str, output: String) -> Result<SopRunStatus, String> {
        let run = self.runs.get_mut(run_id).ok_or("运行实例不存在")?;
        let sop = self.sops.get(&run.sop_name).ok_or("SOP 不存在")?;

        // 记录步骤结果
        run.step_results.push(SopStepResult {
            step_number: run.current_step,
            status: "completed".into(),
            output,
            started_at: chrono::Utc::now().to_rfc3339(),
            completed_at: Some(chrono::Utc::now().to_rfc3339()),
        });

        // 推进到下一步
        run.current_step += 1;
        if run.current_step > run.total_steps {
            run.status = SopRunStatus::Completed;
            run.completed_at = Some(chrono::Utc::now().to_rfc3339());
            return Ok(SopRunStatus::Completed);
        }

        // 检查下一步是否是 checkpoint
        if let Some(next_step) = sop.steps.iter().find(|s| s.number == run.current_step) {
            if next_step.kind == SopStepKind::Checkpoint {
                run.status = SopRunStatus::PausedCheckpoint;
                return Ok(SopRunStatus::PausedCheckpoint);
            }
        }

        Ok(SopRunStatus::Running)
    }

    /// 审批 checkpoint，继续执行
    pub fn approve(&mut self, run_id: &str) -> Result<(), String> {
        let run = self.runs.get_mut(run_id).ok_or("运行实例不存在")?;
        if run.status != SopRunStatus::PausedCheckpoint && run.status != SopRunStatus::WaitingApproval {
            return Err("当前状态不需要审批".into());
        }
        run.status = SopRunStatus::Running;
        Ok(())
    }

    /// 取消运行
    pub fn cancel(&mut self, run_id: &str) -> Result<(), String> {
        let run = self.runs.get_mut(run_id).ok_or("运行实例不存在")?;
        run.status = SopRunStatus::Cancelled;
        run.completed_at = Some(chrono::Utc::now().to_rfc3339());
        Ok(())
    }

    /// 列出活跃运行
    pub fn active_runs(&self) -> Vec<&SopRun> {
        self.runs.values()
            .filter(|r| matches!(r.status, SopRunStatus::Running | SopRunStatus::PausedCheckpoint | SopRunStatus::WaitingApproval))
            .collect()
    }

    /// 列出所有运行
    pub fn all_runs(&self) -> Vec<&SopRun> {
        self.runs.values().collect()
    }

    /// 构建步骤的 Agent prompt（包含上一步输出作为输入）
    pub fn build_step_prompt(&self, run_id: &str) -> Option<String> {
        let run = self.runs.get(run_id)?;
        let sop = self.sops.get(&run.sop_name)?;
        let step = sop.steps.iter().find(|s| s.number == run.current_step)?;

        let mut prompt = format!("## SOP: {} — Step {}/{}: {}\n\n{}\n",
            sop.name, step.number, sop.steps.len(), step.title, step.body);

        if !step.suggested_tools.is_empty() {
            prompt.push_str(&format!("\nSuggested tools: {}\n", step.suggested_tools.join(", ")));
        }

        // 注入上一步输出作为上下文
        if let Some(prev) = run.step_results.last() {
            prompt.push_str(&format!("\n### Previous step output:\n{}\n", prev.output));
        }

        Some(prompt)
    }
}

impl Default for SopEngine {
    fn default() -> Self { Self::new() }
}
