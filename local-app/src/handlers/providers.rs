//! Provider 配置相关命令

use std::sync::Arc;
use tauri::State;

use crate::AppState;
use super::helpers::{load_providers, save_providers};

/// 保存配置项到数据库
#[tauri::command]
pub async fn save_config(
    state: State<'_, Arc<AppState>>,
    key: String,
    value: String,
) -> Result<(), String> {
    // 配置 key 白名单：仅允许安全的配置项
    const ALLOWED_KEYS: &[&str] = &[
        "theme", "language", "sidebar_collapsed", "default_model",
        "font_size", "auto_save", "notification_enabled",
    ];
    if !ALLOWED_KEYS.contains(&key.as_str()) {
        return Err(format!("不允许修改配置项: {}（允许: {:?}）", key, ALLOWED_KEYS));
    }
    state
        .db
        .set_setting(&key, &value)
        .await
        .map_err(|e| format!("保存配置失败: {}", e))
}

/// 获取配置项
#[tauri::command]
pub async fn get_config(
    state: State<'_, Arc<AppState>>,
    key: String,
) -> Result<Option<String>, String> {
    state
        .db
        .get_setting(&key)
        .await
        .map_err(|e| format!("读取配置失败: {}", e))
}

/// 获取所有 provider 配置（脱敏 API Key）
#[tauri::command]
pub async fn get_providers(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<serde_json::Value>, String> {
    let providers = load_providers(&state.db).await?;
    // 脱敏 API Key：只显示前 8 位 + ...，多 Key 时显示数量
    Ok(providers
        .into_iter()
        .map(|mut p| {
            if let Some(key) = p["apiKey"].as_str() {
                let keys: Vec<&str> = key.split("|||").filter(|k| !k.trim().is_empty()).collect();
                let key_count = keys.len();
                let first_key = keys.first().unwrap_or(&"");
                if first_key.len() > 8 {
                    let suffix = if key_count > 1 { format!(" ({} keys)", key_count) } else { String::new() };
                    p["apiKeyMasked"] = serde_json::Value::String(format!("{}...{}", &first_key[..8], suffix));
                } else if !first_key.is_empty() {
                    let suffix = if key_count > 1 { format!(" ({} keys)", key_count) } else { String::new() };
                    p["apiKeyMasked"] = serde_json::Value::String(format!("****{}", suffix));
                } else {
                    p["apiKeyMasked"] = serde_json::Value::String("".to_string());
                }
                p["apiKeyCount"] = serde_json::Value::Number(serde_json::Number::from(key_count));
            }
            // 不返回明文 key
            p.as_object_mut().map(|o| o.remove("apiKey"));
            p
        })
        .collect())
}

/// 保存单个 provider 配置（新增或更新）
#[tauri::command]
pub async fn save_provider(
    state: State<'_, Arc<AppState>>,
    provider: serde_json::Value,
) -> Result<(), String> {
    let id = provider["id"]
        .as_str()
        .ok_or("provider 缺少 id 字段")?
        .to_string();

    let mut providers = load_providers(&state.db).await?;

    // 如果已有同 id，合并更新；否则追加
    if let Some(existing) = providers.iter_mut().find(|p| p["id"].as_str() == Some(&id)) {
        // 更新字段，但如果前端没传 apiKey（脱敏了），保留旧值
        let old_key = existing["apiKey"].as_str().unwrap_or("").to_string();
        *existing = provider.clone();
        if existing["apiKey"].as_str().map_or(true, |k| k.is_empty()) {
            existing["apiKey"] = serde_json::Value::String(old_key);
        }
    } else {
        providers.push(provider);
    }

    save_providers(&state.db, &providers).await
}

/// 删除 provider
#[tauri::command]
pub async fn delete_provider(
    state: State<'_, Arc<AppState>>,
    provider_id: String,
) -> Result<(), String> {
    let mut providers = load_providers(&state.db).await?;
    providers.retain(|p| p["id"].as_str() != Some(&provider_id));
    save_providers(&state.db, &providers).await
}

/// 获取 API 配置状态（兼容旧前端，返回哪些 provider 已配置 key）
#[tauri::command]
pub async fn get_api_status(
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    let providers = load_providers(&state.db).await?;
    let mut status = serde_json::Map::new();
    for p in &providers {
        if let Some(id) = p["id"].as_str() {
            let has_key = p["apiKey"].as_str().map_or(false, |k| !k.is_empty());
            let enabled = p["enabled"].as_bool().unwrap_or(true);
            status.insert(id.to_string(), serde_json::Value::Bool(has_key && enabled));
        }
    }
    Ok(serde_json::Value::Object(status))
}

/// 测试 Provider 连接（发送一个简单请求验证 API Key 有效）
#[tauri::command]
pub async fn test_provider_connection(
    api_type: String,
    api_key: String,
    base_url: Option<String>,
    _model: Option<String>,
) -> Result<serde_json::Value, String> {
    // 多 Key 时取第一个用于测试
    let test_key = api_key.split("|||").next().unwrap_or(&api_key).trim().to_string();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build().map_err(|e| e.to_string())?;

    let url = match base_url.as_deref() {
        Some(u) if !u.is_empty() => format!("{}/models", u.trim_end_matches('/')),
        _ => match api_type.as_str() {
            "anthropic" => "https://api.anthropic.com/v1/models".to_string(),
            _ => "https://api.openai.com/v1/models".to_string(),
        }
    };

    let mut req = client.get(&url);
    if api_type == "anthropic" {
        req = req.header("x-api-key", &test_key)
                 .header("anthropic-version", "2023-06-01");
    } else {
        req = req.header("Authorization", format!("Bearer {}", test_key));
    }

    let start = std::time::Instant::now();
    let resp = req.send().await.map_err(|e| format!("连接失败: {}", e))?;
    let latency_ms = start.elapsed().as_millis();
    let status = resp.status().as_u16();

    if status == 200 || status == 201 {
        let data: serde_json::Value = resp.json().await.unwrap_or_default();
        let model_count = data["data"].as_array().map(|a| a.len()).unwrap_or(0);
        Ok(serde_json::json!({
            "status": "ok",
            "latency_ms": latency_ms,
            "models_available": model_count,
        }))
    } else if status == 401 || status == 403 {
        Err("API Key 无效或已过期".into())
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(format!("HTTP {} — {}", status, &body[..body.len().min(200)]))
    }
}
