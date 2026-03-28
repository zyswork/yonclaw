//! 频道系统 — Telegram/飞书/钉钉等外部渠道
//!
//! Telegram 轮询在桌面端本地执行（不走云端中转），延迟最低。
//! 桌面端离线时，云端自动接管。

pub mod common;
pub mod telegram;
pub mod feishu;
pub mod weixin;
pub mod wecom;
pub mod discord;
pub mod slack;
pub mod dingtalk;
pub mod manager;

/// 从 settings 表的 providers JSON 中查找可用的 API 配置
///
/// 返回 (api_type, api_key, base_url)
/// 优先匹配 agent 使用的模型，找不到则用第一个有 key 的 provider
/// 解析限定模型引用：`provider_id/model` → (provider_id, model)
/// 如果没有 `/`，返回 (None, 原始字符串)
pub fn parse_qualified_model(qualified: &str) -> (Option<&str>, &str) {
    if let Some(pos) = qualified.find('/') {
        let pid = &qualified[..pos];
        let model = &qualified[pos + 1..];
        if !pid.is_empty() && !model.is_empty() {
            return (Some(pid), model);
        }
    }
    (None, qualified)
}

/// 查找 Provider
///
/// 支持限定格式 `provider_id/model`（如 `openai/gpt-4o`）和纯模型名（如 `gpt-4o`）
pub async fn find_provider(
    pool: &sqlx::SqlitePool,
    preferred_model: &str,
) -> Option<(String, String, String)> {
    let providers_json: String = sqlx::query_scalar(
        "SELECT value FROM settings WHERE key = 'providers'"
    ).fetch_optional(pool).await.ok().flatten()?;

    let providers: Vec<serde_json::Value> = serde_json::from_str(&providers_json).ok()?;

    // 解析限定引用
    let (qualified_pid, model_id) = parse_qualified_model(preferred_model);

    // 第 0 轮：限定引用精确匹配（provider_id/model）
    if let Some(pid) = qualified_pid {
        for p in &providers {
            if p["enabled"].as_bool() != Some(true) { continue; }
            let key = p["apiKey"].as_str().unwrap_or("");
            if key.is_empty() { continue; }
            if p["id"].as_str() == Some(pid) {
                let api_type = p["apiType"].as_str().unwrap_or("openai").to_string();
                let base_url = p["baseUrl"].as_str().unwrap_or("").to_string();
                log::info!("Provider: 限定匹配 {}/{} → {} ({})", pid, model_id, p["name"], api_type);
                return Some((api_type, key.to_string(), base_url));
            }
        }
        log::warn!("Provider: 限定引用 {} 未找到供应商 {}，回退到模型匹配", preferred_model, pid);
    }

    // 第一轮：按模型名精确匹配
    for p in &providers {
        if p["enabled"].as_bool() != Some(true) { continue; }
        let key = p["apiKey"].as_str().unwrap_or("");
        if key.is_empty() { continue; }
        if let Some(models) = p["models"].as_array() {
            for m in models {
                if m["id"].as_str() == Some(model_id) {
                    let api_type = p["apiType"].as_str().unwrap_or("openai").to_string();
                    let base_url = p["baseUrl"].as_str().unwrap_or("").to_string();
                    log::info!("Provider: 模型 {} 匹配 → {} ({})", model_id, p["name"], api_type);
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
            model_id, p["name"], api_type, &key[..key.len().min(8)]);
        return Some((api_type, key.to_string(), base_url));
    }

    log::error!("Provider: 无任何可用 provider（模型: {}）", preferred_model);
    None
}
