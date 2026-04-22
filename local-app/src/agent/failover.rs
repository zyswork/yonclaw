//! 模型链 + Failover
//!
//! 支持 primary + fallback 模型配置，错误分类，重试+指数退避+切换

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Failover 错误分类
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FailoverError {
    /// 计费问题（余额不足）
    Billing,
    /// 速率限制
    RateLimit,
    /// 认证失败
    Auth,
    /// 超时
    Timeout,
    /// 服务过载
    Overloaded,
    /// 其他错误
    Other(String),
}

impl FailoverError {
    /// 从错误消息分类
    pub fn classify(error: &str) -> Self {
        let lower = error.to_lowercase();
        if lower.contains("insufficient") || lower.contains("quota") || lower.contains("billing") || lower.contains("balance") {
            Self::Billing
        } else if lower.contains("rate limit") || lower.contains("429") || lower.contains("too many") {
            Self::RateLimit
        } else if lower.contains("unauthorized") || lower.contains("401") || lower.contains("invalid api key") || lower.contains("authentication") {
            Self::Auth
        } else if lower.contains("timeout") || lower.contains("timed out") {
            Self::Timeout
        } else if lower.contains("overloaded") || lower.contains("503") || lower.contains("529") || lower.contains("capacity") {
            Self::Overloaded
        } else {
            Self::Other(error.to_string())
        }
    }

    /// 是否应该重试（同一模型）
    pub fn should_retry(&self) -> bool {
        matches!(self, Self::RateLimit | Self::Timeout | Self::Overloaded)
    }

    /// 是否应该切换到 fallback 模型
    pub fn should_fallback(&self) -> bool {
        matches!(self, Self::Billing | Self::Auth | Self::RateLimit | Self::Overloaded)
    }
}

/// 模型链配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelChainConfig {
    /// 主模型
    pub primary: String,
    /// 备用模型列表（按优先级排序）
    #[serde(default)]
    pub fallbacks: Vec<String>,
    /// 最大重试次数（单个模型）
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// 初始退避时间（毫秒）
    #[serde(default = "default_initial_backoff_ms")]
    pub initial_backoff_ms: u64,
}

fn default_max_retries() -> u32 { 2 }
fn default_initial_backoff_ms() -> u64 { 1000 }

impl Default for ModelChainConfig {
    fn default() -> Self {
        Self {
            primary: String::new(),
            fallbacks: Vec::new(),
            max_retries: 2,
            initial_backoff_ms: 1000,
        }
    }
}

/// Failover 执行结果
#[derive(Debug, Clone, Serialize)]
pub struct FailoverResult<T> {
    /// 实际使用的模型
    pub model_used: String,
    /// 是否使用了 fallback
    pub used_fallback: bool,
    /// 重试次数
    pub retry_count: u32,
    /// 结果
    pub result: T,
}

/// 熔断器状态
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    /// 正常通行
    Closed,
    /// 熔断打开（拒绝请求）
    Open,
    /// 半开（允许探测）
    HalfOpen,
}

/// 熔断器
pub struct CircuitBreaker {
    /// 当前状态
    state: std::sync::Mutex<CircuitState>,
    /// 连续失败次数
    failure_count: std::sync::atomic::AtomicU32,
    /// 打开阈值（连续失败多少次触发 Open）
    threshold: u32,
    /// Open 状态持续时间
    cooldown: Duration,
    /// 上次进入 Open 的时间
    opened_at: std::sync::Mutex<Option<std::time::Instant>>,
}

impl CircuitBreaker {
    /// 创建熔断器（默认 5 次失败 → Open，30 秒冷却）
    pub fn new(threshold: u32, cooldown: Duration) -> Self {
        Self {
            state: std::sync::Mutex::new(CircuitState::Closed),
            failure_count: std::sync::atomic::AtomicU32::new(0),
            threshold,
            cooldown,
            opened_at: std::sync::Mutex::new(None),
        }
    }

    /// 检查是否允许请求
    pub fn allow_request(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match *state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // 检查冷却是否到期
                let opened = self.opened_at.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(t) = *opened {
                    if t.elapsed() >= self.cooldown {
                        *state = CircuitState::HalfOpen;
                        log::info!("熔断器: Open → HalfOpen（冷却到期）");
                        return true; // 允许探测
                    }
                }
                false
            }
            CircuitState::HalfOpen => true, // 允许一个探测请求
        }
    }

    /// 记录成功
    pub fn record_success(&self) {
        self.failure_count.store(0, std::sync::atomic::Ordering::Relaxed);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if *state == CircuitState::HalfOpen {
            *state = CircuitState::Closed;
            log::info!("熔断器: HalfOpen → Closed（探测成功）");
        }
    }

    /// 记录失败
    pub fn record_failure(&self) {
        let count = self.failure_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if count >= self.threshold {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if *state != CircuitState::Open {
                *state = CircuitState::Open;
                *self.opened_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
                log::warn!("熔断器: → Open（连续失败 {} 次）", count);
            }
        }
    }

    /// 当前状态
    pub fn state(&self) -> CircuitState {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Failover 执行器（含熔断器）
///
/// TODO（未接入主路径）：orchestrator 当前直接用 LlmClient::call_stream，
/// 没走 FailoverExecutor。接入需要：
/// 1. 在 orchestrator 构造 FailoverExecutor 实例（per agent 或 per session）
/// 2. `execute(|model| client.call_stream(...))` 包装调用
/// 3. 把失败 classify 映射到 FailoverError 触发 failover
/// 现阶段标记为可扩展扩展点。
#[allow(dead_code)]
pub struct FailoverExecutor {
    config: ModelChainConfig,
    /// 每个模型的熔断器
    breakers: std::collections::HashMap<String, CircuitBreaker>,
}

impl FailoverExecutor {
    pub fn new(config: ModelChainConfig) -> Self {
        let mut breakers = std::collections::HashMap::new();
        // 为每个模型创建熔断器（5次失败→Open，30秒冷却）
        breakers.insert(config.primary.clone(), CircuitBreaker::new(5, Duration::from_secs(30)));
        for f in &config.fallbacks {
            breakers.insert(f.clone(), CircuitBreaker::new(5, Duration::from_secs(30)));
        }
        Self { config, breakers }
    }

    /// 从 Agent 配置 JSON 解析模型链
    pub fn from_agent_config(model: &str, config_json: Option<&str>) -> Self {
        let mut chain = ModelChainConfig {
            primary: model.to_string(),
            ..Default::default()
        };

        if let Some(json_str) = config_json {
            if let Ok(config) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Some(mc) = config.get("modelChain") {
                    if let Some(fallbacks) = mc.get("fallbacks").and_then(|f| f.as_array()) {
                        chain.fallbacks = fallbacks
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                    }
                    if let Some(retries) = mc.get("maxRetries").and_then(|v| v.as_u64()) {
                        chain.max_retries = retries as u32;
                    }
                }
            }
        }

        Self::new(chain)
    }

    /// 获取所有模型（primary + fallbacks）
    pub fn all_models(&self) -> Vec<&str> {
        let mut models = vec![self.config.primary.as_str()];
        for f in &self.config.fallbacks {
            models.push(f.as_str());
        }
        models
    }

    /// 执行带 failover 的异步操作
    ///
    /// `call_fn` 接收模型名，返回 Result
    pub async fn execute<F, Fut, T, E>(
        &self,
        mut call_fn: F,
    ) -> Result<FailoverResult<T>, String>
    where
        F: FnMut(&str) -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        E: std::fmt::Display,
    {
        let models = self.all_models();
        let mut last_error = String::new();

        for (model_idx, model) in models.iter().enumerate() {
            let is_fallback = model_idx > 0;

            // 熔断器检查：如果该模型被熔断，直接跳过
            if let Some(breaker) = self.breakers.get(*model) {
                if !breaker.allow_request() {
                    log::info!("模型 {} 被熔断，跳过", model);
                    continue;
                }
            }

            for retry in 0..=self.config.max_retries {
                if retry > 0 {
                    let backoff = Duration::from_millis(
                        self.config.initial_backoff_ms * 2u64.pow(retry - 1)
                    );
                    log::info!(
                        "模型 {} 重试 {}/{}，退避 {}ms",
                        model, retry, self.config.max_retries, backoff.as_millis()
                    );
                    tokio::time::sleep(backoff).await;
                }

                match call_fn(model).await {
                    Ok(result) => {
                        // 成功：重置熔断器
                        if let Some(breaker) = self.breakers.get(*model) {
                            breaker.record_success();
                        }
                        if is_fallback {
                            log::info!("Failover 成功：使用备用模型 {}", model);
                        }
                        return Ok(FailoverResult {
                            model_used: model.to_string(),
                            used_fallback: is_fallback,
                            retry_count: retry,
                            result,
                        });
                    }
                    Err(e) => {
                        let error_str = e.to_string();
                        let classified = FailoverError::classify(&error_str);
                        log::warn!(
                            "模型 {} 调用失败 (retry {}/{}): {:?} - {}",
                            model, retry, self.config.max_retries, classified, error_str
                        );
                        last_error = error_str;

                        // 记录失败到熔断器
                        if let Some(breaker) = self.breakers.get(*model) {
                            breaker.record_failure();
                        }

                        if !classified.should_retry() {
                            if classified.should_fallback() {
                                break;
                            } else {
                                return Err(format!("模型 {} 调用失败: {}", model, last_error));
                            }
                        }
                    }
                }
            }

            if model_idx < models.len() - 1 {
                log::info!("切换到备用模型: {} → {}", model, models[model_idx + 1]);
            }
        }

        Err(format!("所有模型均失败。最后错误: {}", last_error))
    }
}
