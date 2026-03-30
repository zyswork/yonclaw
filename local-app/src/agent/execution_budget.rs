//! 统一执行预算管理
//!
//! 防止多个闭环（agent_loop、verify-fix、反思重试）互相打架。
//! 所有消耗操作都从同一个 budget 扣减。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

/// 执行预算（每次 send_message_stream 创建一份）
pub struct ExecutionBudget {
    /// 最大 LLM 调用次数
    pub max_llm_calls: u32,
    /// 最大工具调用次数
    pub max_tool_calls: u32,
    /// 最大验证循环次数（verify-fix）
    pub max_verify_cycles: u32,
    /// 单个工具最大重试次数
    pub max_retry_per_tool: u32,
    /// Token 预算上限
    pub max_tokens: u64,
    /// 费用预算上限（美元）
    pub max_cost_usd: f64,

    // 当前计数器
    llm_calls: AtomicU32,
    tool_calls: AtomicU32,
    verify_cycles: AtomicU32,
    /// 每个工具的重试计数（tool_call_id → 重试次数）
    retry_counts: std::sync::Mutex<HashMap<String, u32>>,
    accumulated_tokens: std::sync::atomic::AtomicU64,
    accumulated_cost: std::sync::Mutex<f64>,
}

impl ExecutionBudget {
    /// 使用默认限制创建
    pub fn default_budget() -> Self {
        Self {
            max_llm_calls: 15,
            max_tool_calls: 50,
            max_verify_cycles: 3,
            max_retry_per_tool: 2,
            max_tokens: 200_000,
            max_cost_usd: 1.0,
            llm_calls: AtomicU32::new(0),
            tool_calls: AtomicU32::new(0),
            verify_cycles: AtomicU32::new(0),
            retry_counts: std::sync::Mutex::new(HashMap::new()),
            accumulated_tokens: std::sync::atomic::AtomicU64::new(0),
            accumulated_cost: std::sync::Mutex::new(0.0),
        }
    }

    /// 从 Agent config 创建（可自定义限制）
    pub fn from_config(config: Option<&str>) -> Self {
        let mut budget = Self::default_budget();
        if let Some(json_str) = config {
            if let Ok(cfg) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Some(b) = cfg.get("budget") {
                    if let Some(v) = b.get("maxLlmCalls").and_then(|v| v.as_u64()) { budget.max_llm_calls = v as u32; }
                    if let Some(v) = b.get("maxToolCalls").and_then(|v| v.as_u64()) { budget.max_tool_calls = v as u32; }
                    if let Some(v) = b.get("maxVerifyCycles").and_then(|v| v.as_u64()) { budget.max_verify_cycles = v as u32; }
                    if let Some(v) = b.get("maxRetryPerTool").and_then(|v| v.as_u64()) { budget.max_retry_per_tool = v as u32; }
                    if let Some(v) = b.get("maxTokens").and_then(|v| v.as_u64()) { budget.max_tokens = v; }
                    if let Some(v) = b.get("maxCostUsd").and_then(|v| v.as_f64()) { budget.max_cost_usd = v; }
                }
            }
        }
        budget
    }

    /// 尝试消费一次 LLM 调用
    pub fn try_llm_call(&self) -> Result<u32, String> {
        let prev = self.llm_calls.fetch_add(1, Ordering::SeqCst);
        if prev >= self.max_llm_calls {
            self.llm_calls.fetch_sub(1, Ordering::SeqCst);
            Err(format!("LLM 调用次数已达上限（{}/{}）", prev, self.max_llm_calls))
        } else {
            Ok(prev + 1)
        }
    }

    /// 尝试消费一次工具调用
    pub fn try_tool_call(&self) -> Result<u32, String> {
        let prev = self.tool_calls.fetch_add(1, Ordering::SeqCst);
        if prev >= self.max_tool_calls {
            self.tool_calls.fetch_sub(1, Ordering::SeqCst);
            Err(format!("工具调用次数已达上限（{}/{}）", prev, self.max_tool_calls))
        } else {
            Ok(prev + 1)
        }
    }

    /// 尝试消费一次验证循环
    pub fn try_verify_cycle(&self) -> Result<u32, String> {
        let prev = self.verify_cycles.fetch_add(1, Ordering::SeqCst);
        if prev >= self.max_verify_cycles {
            self.verify_cycles.fetch_sub(1, Ordering::SeqCst);
            Err(format!("验证循环已达上限（{}/{}）", prev, self.max_verify_cycles))
        } else {
            Ok(prev + 1)
        }
    }

    /// 检查某个工具调用是否还能重试
    pub fn can_retry(&self, tool_call_id: &str) -> bool {
        let counts = self.retry_counts.lock().unwrap_or_else(|e| e.into_inner());
        let count = counts.get(tool_call_id).copied().unwrap_or(0);
        count < self.max_retry_per_tool
    }

    /// 记录一次工具重试
    pub fn record_retry(&self, tool_call_id: &str) {
        let mut counts = self.retry_counts.lock().unwrap_or_else(|e| e.into_inner());
        *counts.entry(tool_call_id.to_string()).or_insert(0) += 1;
    }

    /// 累加 token 消耗
    pub fn add_tokens(&self, tokens: u64) {
        self.accumulated_tokens.fetch_add(tokens, std::sync::atomic::Ordering::SeqCst);
    }

    /// 累加费用
    pub fn add_cost(&self, cost: f64) {
        if let Ok(mut c) = self.accumulated_cost.lock() {
            *c += cost;
        }
    }

    /// 检查 token 是否超预算
    pub fn is_token_exceeded(&self) -> bool {
        self.accumulated_tokens.load(std::sync::atomic::Ordering::SeqCst) >= self.max_tokens
    }

    /// 检查费用是否超预算
    pub fn is_cost_exceeded(&self) -> bool {
        self.accumulated_cost.lock().map(|c| *c >= self.max_cost_usd).unwrap_or(false)
    }

    /// 获取当前状态快照
    pub fn snapshot(&self) -> BudgetSnapshot {
        BudgetSnapshot {
            llm_calls: self.llm_calls.load(Ordering::SeqCst),
            max_llm_calls: self.max_llm_calls,
            tool_calls: self.tool_calls.load(Ordering::SeqCst),
            max_tool_calls: self.max_tool_calls,
            verify_cycles: self.verify_cycles.load(Ordering::SeqCst),
            max_verify_cycles: self.max_verify_cycles,
            tokens: self.accumulated_tokens.load(std::sync::atomic::Ordering::SeqCst),
            max_tokens: self.max_tokens,
            cost: self.accumulated_cost.lock().map(|c| *c).unwrap_or(0.0),
            max_cost: self.max_cost_usd,
        }
    }
}

/// 预算快照（用于 HUD 显示和日志）
#[derive(Debug, Clone, serde::Serialize)]
pub struct BudgetSnapshot {
    pub llm_calls: u32,
    pub max_llm_calls: u32,
    pub tool_calls: u32,
    pub max_tool_calls: u32,
    pub verify_cycles: u32,
    pub max_verify_cycles: u32,
    pub tokens: u64,
    pub max_tokens: u64,
    pub cost: f64,
    pub max_cost: f64,
}
