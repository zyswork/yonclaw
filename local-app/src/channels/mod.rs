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
/// 查找 Provider（支持 provider_id 精确匹配，解决同名模型串供应商）
pub async fn find_provider(
    pool: &sqlx::SqlitePool,
    preferred_model: &str,
) -> Option<(String, String, String)> {
    find_provider_with_id(pool, preferred_model, None).await
}

/// 带 provider_id 的精确查找
pub async fn find_provider_with_id(
    pool: &sqlx::SqlitePool,
    preferred_model: &str,
    provider_id: Option<&str>,
) -> Option<(String, String, String)> {
    let providers_json: String = sqlx::query_scalar(
        "SELECT value FROM settings WHERE key = 'providers'"
    ).fetch_optional(pool).await.ok().flatten()?;

    let providers: Vec<serde_json::Value> = serde_json::from_str(&providers_json).ok()?;

    // 第 0 轮：按 provider_id 精确匹配（最优先）
    if let Some(pid) = provider_id {
        for p in &providers {
            if p["enabled"].as_bool() != Some(true) { continue; }
            let key = p["apiKey"].as_str().unwrap_or("");
            if key.is_empty() { continue; }
            if p["id"].as_str() == Some(pid) {
                let api_type = p["apiType"].as_str().unwrap_or("openai").to_string();
                let base_url = p["baseUrl"].as_str().unwrap_or("").to_string();
                log::info!("Provider: provider_id={} 精确匹配 → {} ({})", pid, p["name"], api_type);
                return Some((api_type, key.to_string(), base_url));
            }
        }
        log::warn!("Provider: provider_id={} 未找到，回退到模型匹配", pid);
    }

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
                    log::info!("Provider: 模型 {} 匹配 → {} ({})", preferred_model, p["name"], api_type);
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
        log::warn!("Provider: 模型 {} 无精确匹配，回退到 {} ({}, key={}...)",
            preferred_model, p["name"], api_type, &key[..key.len().min(8)]);
        return Some((api_type, key.to_string(), base_url));
    }

    log::error!("Provider: 无任何可用 provider（模型: {}）", preferred_model);
    None
}
