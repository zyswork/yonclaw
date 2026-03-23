//! 频道系统 — Telegram/飞书/钉钉等外部渠道
//!
//! Telegram 轮询在桌面端本地执行（不走云端中转），延迟最低。
//! 桌面端离线时，云端自动接管。

pub mod telegram;
pub mod feishu;
pub mod weixin;
pub mod discord;
pub mod slack;

/// 从 settings 表的 providers JSON 中查找可用的 API 配置
///
/// 返回 (api_type, api_key, base_url)
/// 优先匹配 agent 使用的模型，找不到则用第一个有 key 的 provider
pub async fn find_provider(
    pool: &sqlx::SqlitePool,
    preferred_model: &str,
) -> Option<(String, String, String)> {
    let providers_json: String = sqlx::query_scalar(
        "SELECT value FROM settings WHERE key = 'providers'"
    ).fetch_optional(pool).await.ok().flatten()?;

    let providers: Vec<serde_json::Value> = serde_json::from_str(&providers_json).ok()?;

    // 第一轮：按模型精确匹配
    for p in &providers {
        if p["enabled"].as_bool() != Some(true) { continue; }
        let key = p["apiKey"].as_str().unwrap_or("");
        if key.is_empty() { continue; }
        if let Some(models) = p["models"].as_array() {
            for m in models {
                if m["id"].as_str() == Some(preferred_model) {
                    let api_type = p["apiType"].as_str().unwrap_or("openai").to_string();
                    let base_url = p["baseUrl"].as_str().unwrap_or("").to_string();
                    log::info!("频道 Provider: 匹配模型 {} → {} ({})", preferred_model, p["name"], api_type);
                    return Some((api_type, key.to_string(), base_url));
                }
            }
        }
    }

    // 第二轮：回退到第一个有 key 的 provider
    for p in &providers {
        if p["enabled"].as_bool() != Some(true) { continue; }
        let key = p["apiKey"].as_str().unwrap_or("");
        if key.is_empty() { continue; }
        let api_type = p["apiType"].as_str().unwrap_or("openai").to_string();
        let base_url = p["baseUrl"].as_str().unwrap_or("").to_string();
        log::warn!("频道 Provider: 模型 {} 无精确匹配，回退到 {} ({}, key={}...)",
            preferred_model, p["name"], api_type, &key[..key.len().min(8)]);
        return Some((api_type, key.to_string(), base_url));
    }

    log::error!("频道 Provider: 无任何可用 provider（模型: {}）", preferred_model);
    None
}
