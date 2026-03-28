//! 频道通用工具 — Token 缓存、重连策略、HTTP 回复等

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};

/// 指数退避重连延迟（秒）
///
/// 根据连续失败次数计算等待时间：
/// - attempt 0: 1s, 1: 2s, 2: 4s, 3: 8s, 4: 16s, 5+: 30s
/// - 超过 MAX_BACKOFF_ATTEMPTS 次连续失败后，降级为探测模式（60s）
pub fn reconnect_delay(attempt: u32) -> u64 {
    if attempt >= 10 {
        // 超过 10 次连续失败，降级为 60 秒探测
        60
    } else {
        // 1s, 2s, 4s, 8s, 16s, 30s (最大)
        (2u64.pow(attempt.min(4))).min(30)
    }
}

/// 通用 access_token 缓存
///
/// 各频道（飞书/钉钉/企业微信）均需定期刷新 access_token，
/// 本结构统一封装"读缓存 → 过期则刷新"的逻辑。
pub struct TokenCache {
    token: RwLock<Option<CachedToken>>,
}

/// 缓存条目
struct CachedToken {
    value: String,
    expires_at: Instant,
}

impl TokenCache {
    /// 创建空缓存（Arc 包装，方便跨 task 共享）
    pub fn new() -> Arc<Self> {
        Arc::new(Self { token: RwLock::new(None) })
    }

    /// 使用已有 token 值初始化缓存
    ///
    /// `ttl_secs` 为 token 的原始有效期（秒），内部自动提前 300 秒刷新。
    pub fn with_initial(value: String, ttl_secs: u64) -> Arc<Self> {
        let expires_at = Instant::now() + Duration::from_secs(ttl_secs.saturating_sub(300));
        Arc::new(Self {
            token: RwLock::new(Some(CachedToken { value, expires_at })),
        })
    }

    /// 获取 token，过期或不存在则调用 `fetch_fn` 刷新
    ///
    /// `fetch_fn` 返回 `(token_string, ttl_seconds)`。
    /// 内部会自动提前 300 秒（5 分钟）触发刷新，避免边界过期。
    pub async fn get_or_refresh<F, Fut>(&self, fetch_fn: F) -> Result<String, String>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(String, u64), String>>,
    {
        // 先尝试读缓存
        {
            let guard = self.token.read().await;
            if let Some(cached) = guard.as_ref() {
                if Instant::now() < cached.expires_at {
                    return Ok(cached.value.clone());
                }
            }
        }
        // 缓存不存在或已过期，刷新
        let (new_token, ttl_secs) = fetch_fn().await?;
        let expires_at = Instant::now() + Duration::from_secs(ttl_secs.saturating_sub(300));
        *self.token.write().await = Some(CachedToken {
            value: new_token.clone(),
            expires_at,
        });
        Ok(new_token)
    }
}
