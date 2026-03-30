//! OAuth 认证相关命令
//!
//! 支持 Google (Gemini) 和 OpenAI 的 OAuth PKCE 认证流程。
//! 包含启动授权、交换令牌、刷新令牌三个 Tauri 命令。

use std::sync::Arc;
use std::collections::HashMap;
use tauri::State;

use crate::AppState;
use crate::agent::llm::build_proxied_client;
use super::helpers::{load_providers, save_providers};

// ─── 全局待处理 OAuth 流程存储 ────────────────────────────────

static PENDING_FLOWS: std::sync::OnceLock<std::sync::Mutex<HashMap<String, PendingOAuth>>> =
    std::sync::OnceLock::new();

struct PendingOAuth {
    provider: String,
    code_verifier: String,
    redirect_uri: String,
    #[allow(dead_code)]
    created_at: i64,
}

fn pending_flows() -> &'static std::sync::Mutex<HashMap<String, PendingOAuth>> {
    PENDING_FLOWS.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

// ─── OAuth Provider 预设配置 ──────────────────────────────────

struct OAuthPreset {
    name: &'static str,
    api_type: &'static str,
    authorize_url: &'static str,
    token_url: &'static str,
    client_id: &'static str,
    client_secret: &'static str,
    base_url: &'static str,
    scopes: &'static str,
    models: Vec<(&'static str, &'static str)>,
    // 运行时动态填充（覆盖 client_id/client_secret）
    client_id_owned: String,
    client_secret_owned: String,
}

impl OAuthPreset {
    fn effective_client_id(&self) -> &str {
        if !self.client_id_owned.is_empty() { &self.client_id_owned } else { self.client_id }
    }
    fn effective_client_secret(&self) -> &str {
        if !self.client_secret_owned.is_empty() { &self.client_secret_owned } else { self.client_secret }
    }
}

fn get_oauth_presets() -> Vec<OAuthPreset> {
    vec![
        OAuthPreset {
            name: "Google (Gemini)",
            api_type: "openai",
            authorize_url: "https://accounts.google.com/o/oauth2/v2/auth",
            token_url: "https://oauth2.googleapis.com/token",
            // 运行时从本地 Gemini CLI 提取，或从环境变量读取
            client_id: "", // 动态填充，见 resolve_google_credentials()
            client_secret: "", // 动态填充
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
            scopes: "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile",
            models: vec![
                ("gemini-3.1-pro-preview", "Gemini 3.1 Pro"),
                ("gemini-3-flash-preview", "Gemini 3 Flash"),
                ("gemini-2.5-pro", "Gemini 2.5 Pro"),
                ("gemini-2.5-flash", "Gemini 2.5 Flash"),
                ("gemini-2.5-flash-lite", "Gemini 2.5 Flash Lite"),
                ("gemini-2.0-flash", "Gemini 2.0 Flash"),
            ],
            client_id_owned: String::new(),
            client_secret_owned: String::new(),
        },
        OAuthPreset {
            name: "OpenAI",
            api_type: "openai",
            authorize_url: "https://auth.openai.com/oauth/authorize",
            token_url: "https://auth.openai.com/oauth/token",
            client_id: "app_EMoamEEZ73f0CkXaXp7hrann",
            client_secret: "",
            base_url: "",
            scopes: "openid profile email offline_access",
            models: vec![
                ("gpt-5.2", "GPT-5.2"),
                ("gpt-5.2-pro", "GPT-5.2 Pro"),
                ("gpt-5.1", "GPT-5.1"),
                ("o4-mini", "o4-mini"),
                ("o3", "o3"),
                ("o3-pro", "o3 Pro"),
                ("gpt-4.1", "GPT-4.1"),
                ("gpt-4.1-mini", "GPT-4.1 Mini"),
                ("gpt-4o", "GPT-4o"),
                ("gpt-4o-mini", "GPT-4o Mini"),
            ],
            client_id_owned: String::new(),
            client_secret_owned: String::new(),
        },
    ]
}

/// 从 DB settings 或本地 Gemini CLI 获取 Google OAuth credentials
fn resolve_google_credentials() -> Option<(String, String)> {
    // 优先从环境变量
    if let (Ok(id), Ok(secret)) = (std::env::var("GEMINI_CLI_OAUTH_CLIENT_ID"), std::env::var("GEMINI_CLI_OAUTH_CLIENT_SECRET")) {
        return Some((id, secret));
    }

    // 从 settings DB 读取（首次运行时由 seed_oauth_credentials 写入）
    // 这里用同步文件读取作为 fallback（DB 需要 async，但 preset 构建是 sync）
    let home = dirs::home_dir()?;
    let creds_file = home.join(".xianzhu/oauth_credentials.json");
    if let Ok(content) = std::fs::read_to_string(&creds_file) {
        if let Ok(creds) = serde_json::from_str::<serde_json::Value>(&content) {
            let id = creds["google_client_id"].as_str()?.to_string();
            let secret = creds["google_client_secret"].as_str()?.to_string();
            if !id.is_empty() && !secret.is_empty() {
                return Some((id, secret));
            }
        }
    }

    // Fallback: 从本地 Gemini CLI 提取
    let search_paths = [
        "/opt/homebrew/lib/node_modules/@google/gemini-cli/node_modules/@google/gemini-cli-core/dist/src/code_assist/oauth2.js",
        "/usr/local/lib/node_modules/@google/gemini-cli/node_modules/@google/gemini-cli-core/dist/src/code_assist/oauth2.js",
    ];
    for path in &search_paths {
        if let Ok(content) = std::fs::read_to_string(path) {
            let id = extract_pattern(&content, r"\d+-[a-z0-9]+\.apps\.googleusercontent\.com");
            let secret = extract_pattern(&content, r"GOCSPX-[A-Za-z0-9_-]+");
            if let (Some(id), Some(secret)) = (id, secret) {
                // 提取成功后缓存到本地文件
                let _ = save_credentials_cache(&id, &secret);
                return Some((id, secret));
            }
        }
    }

    None
}

/// 缓存 credentials 到本地文件（避免每次都扫描 Gemini CLI）
fn save_credentials_cache(google_id: &str, google_secret: &str) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("无法获取 home 目录")?;
    let dir = home.join(".xianzhu");
    let _ = std::fs::create_dir_all(&dir);
    let creds = serde_json::json!({
        "google_client_id": google_id,
        "google_client_secret": google_secret,
    });
    std::fs::write(dir.join("oauth_credentials.json"), serde_json::to_string_pretty(&creds).unwrap_or_default())
        .map_err(|e| format!("保存 credentials 缓存失败: {}", e))
}

fn extract_pattern(content: &str, pattern: &str) -> Option<String> {
    regex::Regex::new(pattern).ok()?.find(content).map(|m| m.as_str().to_string())
}

/// 首次运行时从 Gemini CLI 提取 credentials 并缓存（由 main.rs 调用）
pub async fn seed_oauth_credentials() {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return,
    };
    let creds_file = home.join(".xianzhu/oauth_credentials.json");
    if creds_file.exists() { return; } // 已有缓存

    // 尝试从 Gemini CLI 提取
    let search_paths = [
        "/opt/homebrew/lib/node_modules/@google/gemini-cli/node_modules/@google/gemini-cli-core/dist/src/code_assist/oauth2.js",
        "/usr/local/lib/node_modules/@google/gemini-cli/node_modules/@google/gemini-cli-core/dist/src/code_assist/oauth2.js",
    ];
    for path in &search_paths {
        if let Ok(content) = std::fs::read_to_string(path) {
            let id = extract_pattern(&content, r"\d+-[a-z0-9]+\.apps\.googleusercontent\.com");
            let secret = extract_pattern(&content, r"GOCSPX-[A-Za-z0-9_-]+");
            if let (Some(id), Some(secret)) = (id, secret) {
                let _ = save_credentials_cache(&id, &secret);
                log::info!("OAuth: 已从 Gemini CLI 提取并缓存 Google credentials");
                return;
            }
        }
    }
    log::info!("OAuth: 未找到 Gemini CLI，Google OAuth 需要安装 gemini-cli 或手动配置");
}

fn find_preset(provider: &str) -> Option<OAuthPreset> {
    get_oauth_presets()
        .into_iter()
        .find(|p| p.name.to_lowercase().contains(&provider.to_lowercase()))
}

// ─── PKCE 辅助函数 ───────────────────────────────────────────

/// Base64 URL 安全编码（无填充）
fn base64_url_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 生成 PKCE code_verifier（32 字节随机数据 → base64url）
fn generate_code_verifier() -> String {
    // 使用 uuid 生成随机字节（两个 UUID = 32 字节）
    let u1 = uuid::Uuid::new_v4();
    let u2 = uuid::Uuid::new_v4();
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(u1.as_bytes());
    bytes.extend_from_slice(u2.as_bytes());
    base64_url_encode(&bytes)
}

/// 生成 PKCE code_challenge = SHA256(verifier) base64url
fn generate_code_challenge(verifier: &str) -> String {
    use sha2::{Sha256, Digest};
    let hash = Sha256::digest(verifier.as_bytes());
    base64_url_encode(&hash)
}

/// 生成随机 state 参数（32 hex 字符）
fn generate_state() -> String {
    let u = uuid::Uuid::new_v4();
    hex::encode(u.as_bytes())
}

// ─── Tauri Commands ──────────────────────────────────────────

/// 启动 OAuth 授权流程
///
/// 启动临时 HTTP server 接收回调，生成 PKCE 参数，打开浏览器
#[tauri::command]
pub async fn start_oauth_flow(
    app: tauri::AppHandle,
    state_arc: State<'_, Arc<AppState>>,
    provider: String,
) -> Result<serde_json::Value, String> {
    let mut preset = find_preset(&provider)
        .ok_or_else(|| format!("未知的 OAuth 提供商: {}", provider))?;

    // Google: 动态填充 credentials
    if provider.to_lowercase().contains("google") {
        let (id, secret) = resolve_google_credentials()
            .ok_or("未找到 Google OAuth credentials。请先安装 Gemini CLI: brew install gemini-cli（或 npm install -g @google/gemini-cli），或设置环境变量 GEMINI_CLI_OAUTH_CLIENT_ID")?;
        preset.client_id_owned = id;
        preset.client_secret_owned = secret;
    }

    // 生成 PKCE 参数
    let code_verifier = generate_code_verifier();
    let code_challenge = generate_code_challenge(&code_verifier);
    let oauth_state = generate_state();

    // 保存到全局待处理流程
    {
        let mut flows = pending_flows().lock().map_err(|e| format!("锁定失败: {}", e))?;
        flows.insert(oauth_state.clone(), PendingOAuth {
            provider: provider.clone(),
            code_verifier,
            redirect_uri: String::new(), // 先占位，下面设端口后更新
            created_at: chrono::Utc::now().timestamp(),
        });
        let cutoff = chrono::Utc::now().timestamp() - 600;
        flows.retain(|_, v| v.created_at > cutoff);
    }

    // 确定回调端口和路径（OpenAI 要求固定 1455 + /auth/callback）
    let is_openai = provider.to_lowercase().contains("openai");
    let (port, callback_path) = if is_openai {
        (1455u16, "/auth/callback")
    } else {
        // Google 等其他 provider 用随机端口
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|e| format!("绑定回调端口失败: {}", e))?;
        let p = listener.local_addr().map_err(|e| format!("获取端口失败: {}", e))?.port();
        drop(listener);
        (p, "/oauth/callback")
    };

    let redirect_uri = format!("http://localhost:{}{}", port, callback_path);

    // 更新 pending flow 的 redirect_uri
    {
        let mut flows = pending_flows().lock().map_err(|e| format!("锁定失败: {}", e))?;
        if let Some(flow) = flows.get_mut(&oauth_state) {
            flow.redirect_uri = redirect_uri.clone();
        }
    }

    // 构建授权 URL
    let mut params = vec![
        ("client_id", preset.effective_client_id().to_string()),
        ("redirect_uri", redirect_uri.clone()),
        ("response_type", "code".to_string()),
        ("scope", preset.scopes.to_string()),
        ("state", oauth_state.clone()),
        ("code_challenge", code_challenge),
        ("code_challenge_method", "S256".to_string()),
    ];

    if provider.to_lowercase().contains("google") {
        params.push(("access_type", "offline".to_string()));
        params.push(("prompt", "consent".to_string()));
    }
    if is_openai {
        params.push(("id_token_add_organizations", "true".to_string()));
        params.push(("codex_cli_simplified_flow", "true".to_string()));
    }

    let url = format!(
        "{}?{}",
        preset.authorize_url,
        params.iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&")
    );

    // 异步启动临时回调 server（2 分钟超时自动关闭）
    let pool = state_arc.db.pool().clone();
    let app_handle = app.clone();
    tokio::spawn(async move {
        if let Err(e) = run_oauth_callback_server(port, &pool, &app_handle).await {
            log::warn!("OAuth 回调 server 错误: {}", e);
        }
    });

    // 打开浏览器
    #[cfg(target_os = "macos")]
    { let _ = std::process::Command::new("open").arg(&url).spawn(); }
    #[cfg(target_os = "linux")]
    { let _ = std::process::Command::new("xdg-open").arg(&url).spawn(); }
    #[cfg(target_os = "windows")]
    { let _ = std::process::Command::new("cmd").args(["/C", "start", &url]).spawn(); }

    log::info!("OAuth 流程已启动: provider={}, callback=http://localhost:{}", provider, port);

    Ok(serde_json::json!({
        "state": oauth_state,
        "authorizeUrl": url,
        "callbackPort": port,
    }))
}

/// 临时 OAuth 回调 HTTP server
///
/// 只接受一次请求后自动关闭，最多等 2 分钟
async fn run_oauth_callback_server(
    port: u16,
    pool: &sqlx::SqlitePool,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    use tokio::net::TcpListener;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await
        .map_err(|e| format!("绑定回调端口失败: {}", e))?;

    log::info!("OAuth 回调 server 启动: http://localhost:{}", port);

    // 等待一个连接（2 分钟超时）
    let accept = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        listener.accept(),
    ).await;

    let (mut stream, _) = match accept {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(format!("接受连接失败: {}", e)),
        Err(_) => return Err("OAuth 回调等待超时（2分钟）".into()),
    };

    // 读取 HTTP 请求
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.map_err(|e| format!("读取请求失败: {}", e))?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // 解析请求行：GET /oauth/callback?code=xxx&state=yyy HTTP/1.1
    let path = request.lines().next().unwrap_or("")
        .split_whitespace().nth(1).unwrap_or("");

    let query = path.split('?').nth(1).unwrap_or("");
    let params: HashMap<String, String> = query.split('&')
        .filter_map(|p| {
            let mut kv = p.splitn(2, '=');
            Some((kv.next()?.to_string(), urlencoding::decode(kv.next().unwrap_or("")).unwrap_or_default().to_string()))
        })
        .collect();

    let code = params.get("code").cloned().unwrap_or_default();
    let state = params.get("state").cloned().unwrap_or_default();

    // HTML 转义：防止注入攻击
    fn html_escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#x27;")
    }

    // 处理回调
    let (html, success, provider_name) = if code.is_empty() || state.is_empty() {
        let error = params.get("error").cloned().unwrap_or_else(|| "缺少 code 或 state".into());
        (format!(
            "<html><body style='font-family:system-ui;text-align:center;padding:60px'><h2 style='color:#ef4444'>授权失败</h2><p>{}</p></body></html>",
            html_escape(&error)
        ), false, String::new())
    } else {
        match handle_oauth_callback(pool, &code, &state).await {
            Ok(name) => (
                format!(
                    "<html><body style='font-family:system-ui;text-align:center;padding:60px'><h2 style='color:#10b981'>授权成功！</h2><p>{} 已配置完成。</p><p style='color:#888'>可以关闭此窗口。</p><script>setTimeout(()=>window.close(),2000)</script></body></html>",
                    html_escape(&name)
                ),
                true, name
            ),
            Err(e) => (
                format!(
                    "<html><body style='font-family:system-ui;text-align:center;padding:60px'><h2 style='color:#ef4444'>授权失败</h2><p>{}</p></body></html>",
                    html_escape(&e)
                ),
                false, String::new()
            ),
        }
    };

    // 发送 HTTP 响应
    let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n{}", html);
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;

    // 通知前端
    use tauri::Manager;
    let _ = app_handle.emit_all("oauth-complete", serde_json::json!({
        "success": success,
        "provider": provider_name,
    }));

    log::info!("OAuth 回调处理完成: success={}, provider={}", success, provider_name);
    Ok(())
}

/// 交换 OAuth 授权码获取令牌
///
/// 使用 PKCE code_verifier 交换 access_token，并保存为 provider
#[tauri::command]
pub async fn exchange_oauth_code(
    state: State<'_, Arc<AppState>>,
    code: String,
    oauth_state: String,
) -> Result<serde_json::Value, String> {
    // 查找并移除待处理流程
    let pending = {
        let mut flows = pending_flows().lock().map_err(|e| format!("锁定失败: {}", e))?;
        flows.remove(&oauth_state)
            .ok_or_else(|| "无效或过期的 OAuth state".to_string())?
    };

    let preset = find_preset(&pending.provider)
        .ok_or_else(|| format!("未知的 OAuth 提供商: {}", pending.provider))?;

    let redirect_uri = pending.redirect_uri;

    // 调用令牌端点
    let client = build_proxied_client(10, 30);
    let mut form_params: Vec<(&str, String)> = vec![
        ("grant_type", "authorization_code".into()),
        ("code", code.clone()),
        ("redirect_uri", redirect_uri.clone()),
        ("client_id", preset.effective_client_id().to_string()),
        ("code_verifier", pending.code_verifier.clone()),
    ];
    if !preset.effective_client_secret().is_empty() {
        form_params.push(("client_secret", preset.effective_client_secret().to_string()));
    }
    let token_response = client
        .post(preset.token_url)
        .form(&form_params)
        .send()
        .await
        .map_err(|e| format!("令牌请求失败: {}", e))?;

    let status = token_response.status();
    let body: serde_json::Value = token_response.json().await
        .map_err(|e| format!("解析令牌响应失败: {}", e))?;

    if !status.is_success() {
        let error_desc = body["error_description"].as_str()
            .or_else(|| body["error"].as_str())
            .unwrap_or("未知错误");
        return Err(format!("令牌交换失败 ({}): {}", status.as_u16(), error_desc));
    }

    let access_token = body["access_token"].as_str()
        .ok_or("响应中缺少 access_token")?;
    let refresh_token = body["refresh_token"].as_str().map(|s| s.to_string());
    let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
    let expires_at = chrono::Utc::now().timestamp() + expires_in;

    let provider_id = format!("oauth-{}", pending.provider.to_lowercase().replace(' ', "-"));

    // Google OAuth：发现 project ID（通过 Cloud Code API）
    // Gemini API 的 OAuth token 需要通过 Vertex AI 端点调用，需要 project ID
    let project_id = if pending.provider.to_lowercase().contains("google") {
        match discover_google_project(access_token, &client).await {
            Ok(pid) => {
                log::info!("Google Cloud project 发现成功: {}", pid);
                Some(pid)
            }
            Err(e) => {
                log::warn!("Google Cloud project 发现失败（将使用全局端点）: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Google OAuth 使用 Vertex AI 端点（generativelanguage.googleapis.com 不支持 OAuth token）
    // Google OAuth 使用 Cloud Code Assist API — project ID 编码在 base_url 中
    let effective_base_url = if pending.provider.to_lowercase().contains("google") {
        if let Some(ref pid) = project_id {
            // 把 project ID 编码在 URL fragment 中，llm.rs 会解析
            format!("https://cloudcode-pa.googleapis.com/v1internal#project={}", pid)
        } else {
            "https://cloudcode-pa.googleapis.com/v1internal".to_string()
        }
    } else {
        preset.base_url.to_string()
    };

    // 构建模型列表
    let model_array: Vec<serde_json::Value> = preset.models.iter()
        .map(|(mid, mname)| serde_json::json!({"id": mid, "name": mname}))
        .collect();

    // 加载现有 providers 并更新或添加
    let mut providers = load_providers(&state.db).await.unwrap_or_default();

    let mut oauth_info = serde_json::json!({
        "provider": pending.provider,
        "refreshToken": refresh_token,
        "expiresAt": expires_at,
        "tokenUrl": preset.token_url,
        "clientId": preset.effective_client_id(),
    });
    if let Some(ref pid) = project_id {
        oauth_info["projectId"] = serde_json::Value::String(pid.clone());
    }

    if let Some(existing) = providers.iter_mut().find(|p| p["id"].as_str() == Some(&provider_id)) {
        existing["apiKey"] = serde_json::Value::String(access_token.to_string());
        existing["baseUrl"] = serde_json::Value::String(effective_base_url.clone());
        existing["oauth"] = oauth_info;
    } else {
        let new_provider = serde_json::json!({
            "id": provider_id,
            "name": preset.name,
            "apiType": preset.api_type,
            "baseUrl": effective_base_url,
            "apiKey": access_token,
            "models": model_array,
            "enabled": true,
            "oauth": oauth_info,
        });
        providers.push(new_provider);
    }

    save_providers(&state.db, &providers).await
        .map_err(|e| format!("保存 provider 失败: {}", e))?;

    log::info!("OAuth 认证成功: provider={}", preset.name);

    Ok(serde_json::json!({
        "success": true,
        "provider": preset.name,
        "providerId": provider_id,
    }))
}

/// 刷新 OAuth 令牌
///
/// 使用 refresh_token 获取新的 access_token
#[tauri::command]
pub async fn refresh_oauth_token(
    state: State<'_, Arc<AppState>>,
    provider_id: String,
) -> Result<(), String> {
    let mut providers = load_providers(&state.db).await
        .map_err(|e| format!("加载 providers 失败: {}", e))?;

    let provider = providers.iter_mut()
        .find(|p| p["id"].as_str() == Some(&provider_id))
        .ok_or_else(|| format!("未找到 provider: {}", provider_id))?;

    let oauth = provider.get("oauth")
        .ok_or("该 provider 不是 OAuth 类型")?;

    let refresh_token = oauth["refreshToken"].as_str()
        .ok_or("缺少 refreshToken")?;
    let token_url = oauth["tokenUrl"].as_str()
        .ok_or("缺少 tokenUrl")?;
    let client_id = oauth["clientId"].as_str()
        .ok_or("缺少 clientId")?;

    // 调用令牌刷新端点
    let client = build_proxied_client(10, 30);
    let token_response = client
        .post(token_url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
        ])
        .send()
        .await
        .map_err(|e| format!("刷新令牌请求失败: {}", e))?;

    let status = token_response.status();
    let body: serde_json::Value = token_response.json().await
        .map_err(|e| format!("解析刷新响应失败: {}", e))?;

    if !status.is_success() {
        let error_desc = body["error_description"].as_str()
            .or_else(|| body["error"].as_str())
            .unwrap_or("未知错误");
        return Err(format!("刷新令牌失败: {}", error_desc));
    }

    let new_access_token = body["access_token"].as_str()
        .ok_or("刷新响应中缺少 access_token")?;
    let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
    let new_expires_at = chrono::Utc::now().timestamp() + expires_in;

    // 更新 provider
    provider["apiKey"] = serde_json::Value::String(new_access_token.to_string());
    if let Some(oauth_mut) = provider.get_mut("oauth") {
        oauth_mut["expiresAt"] = serde_json::json!(new_expires_at);
        // 如果响应中有新的 refresh_token，也更新
        if let Some(new_rt) = body["refresh_token"].as_str() {
            oauth_mut["refreshToken"] = serde_json::Value::String(new_rt.to_string());
        }
    }

    save_providers(&state.db, &providers).await
        .map_err(|e| format!("保存更新后的 provider 失败: {}", e))?;

    log::info!("OAuth 令牌刷新成功: provider_id={}", provider_id);

    Ok(())
}

// ─── 供网关回调使用的公共函数 ─────────────────────────────────

/// 内部刷新 OAuth token（供 send_message 自动刷新调用）
///
/// 返回新的 access_token
pub async fn refresh_oauth_token_internal(
    db: &crate::db::Database,
    provider_id: &str,
) -> Result<String, String> {
    let mut providers = load_providers(db).await
        .map_err(|e| format!("加载 providers 失败: {}", e))?;

    let provider = providers.iter_mut()
        .find(|p| p["id"].as_str() == Some(provider_id))
        .ok_or_else(|| format!("未找到 provider: {}", provider_id))?;

    let oauth = provider.get("oauth")
        .ok_or("该 provider 不是 OAuth 类型")?;

    let refresh_token = oauth["refreshToken"].as_str()
        .ok_or("缺少 refreshToken")?.to_string();
    let token_url = oauth["tokenUrl"].as_str()
        .ok_or("缺少 tokenUrl")?.to_string();
    let client_id_str = oauth["clientId"].as_str()
        .ok_or("缺少 clientId")?.to_string();

    // Google 需要 client_secret
    let client_secret_str = oauth.get("clientSecret")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // 如果 DB 没有 client_secret，从本地 credentials 缓存读取
    let effective_secret = if client_secret_str.is_empty() && provider_id.contains("google") {
        resolve_google_credentials().map(|(_, s)| s).unwrap_or_default()
    } else {
        client_secret_str
    };

    let client = build_proxied_client(10, 30);
    let mut form_params: Vec<(&str, &str)> = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", &refresh_token),
        ("client_id", &client_id_str),
    ];
    if !effective_secret.is_empty() {
        form_params.push(("client_secret", &effective_secret));
    }

    let token_response = client
        .post(&token_url)
        .form(&form_params)
        .send()
        .await
        .map_err(|e| format!("刷新令牌请求失败: {}", e))?;

    let status = token_response.status();
    let body: serde_json::Value = token_response.json().await
        .map_err(|e| format!("解析刷新响应失败: {}", e))?;

    if !status.is_success() {
        let error_desc = body["error_description"].as_str()
            .or_else(|| body["error"].as_str())
            .unwrap_or("未知错误");
        return Err(format!("刷新令牌失败 ({}): {}", status.as_u16(), error_desc));
    }

    let new_access_token = body["access_token"].as_str()
        .ok_or("刷新响应中缺少 access_token")?.to_string();
    let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
    let new_expires_at = chrono::Utc::now().timestamp() + expires_in;

    // 更新 provider
    provider["apiKey"] = serde_json::Value::String(new_access_token.clone());
    if let Some(oauth_mut) = provider.get_mut("oauth") {
        oauth_mut["expiresAt"] = serde_json::json!(new_expires_at);
        if let Some(new_rt) = body["refresh_token"].as_str() {
            oauth_mut["refreshToken"] = serde_json::Value::String(new_rt.to_string());
        }
    }

    save_providers(db, &providers).await
        .map_err(|e| format!("保存更新后的 provider 失败: {}", e))?;

    log::info!("OAuth token 自动刷新成功: provider={}, new_expires_at={}", provider_id, new_expires_at);

    Ok(new_access_token)
}

/// 处理 OAuth 回调（供 gateway/api.rs 调用）
///
/// 从查询参数中提取 code 和 state，执行令牌交换，
/// 返回 (success, provider_name) 或错误信息
pub async fn handle_oauth_callback(
    pool: &sqlx::SqlitePool,
    code: &str,
    oauth_state: &str,
) -> Result<String, String> {
    // 查找并移除待处理流程
    let pending = {
        let mut flows = pending_flows().lock().map_err(|e| format!("锁定失败: {}", e))?;
        flows.remove(oauth_state)
            .ok_or_else(|| "无效或过期的 OAuth state".to_string())?
    };

    let mut preset = find_preset(&pending.provider)
        .ok_or_else(|| format!("未知的 OAuth 提供商: {}", pending.provider))?;

    // Google: 动态解析 credentials（和 start_oauth_flow 相同）
    if pending.provider.to_lowercase().contains("google") {
        if let Some((id, secret)) = resolve_google_credentials() {
            preset.client_id_owned = id;
            preset.client_secret_owned = secret;
        }
    }

    let redirect_uri = pending.redirect_uri;

    // 调用令牌端点
    let client = build_proxied_client(10, 30);
    let mut form_params: Vec<(&str, String)> = vec![
        ("grant_type", "authorization_code".into()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.clone()),
        ("client_id", preset.effective_client_id().to_string()),
        ("code_verifier", pending.code_verifier.clone()),
    ];
    if !preset.effective_client_secret().is_empty() {
        form_params.push(("client_secret", preset.effective_client_secret().to_string()));
    }
    let token_response = client
        .post(preset.token_url)
        .form(&form_params)
        .send()
        .await
        .map_err(|e| format!("令牌请求失败: {}", e))?;

    let status = token_response.status();
    let body: serde_json::Value = token_response.json().await
        .map_err(|e| format!("解析令牌响应失败: {}", e))?;

    if !status.is_success() {
        let error_desc = body["error_description"].as_str()
            .or_else(|| body["error"].as_str())
            .unwrap_or("未知错误");
        return Err(format!("令牌交换失败: {}", error_desc));
    }

    let access_token = body["access_token"].as_str()
        .ok_or("响应中缺少 access_token")?;
    let refresh_token = body["refresh_token"].as_str().map(|s| s.to_string());
    let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
    let expires_at = chrono::Utc::now().timestamp() + expires_in;

    let provider_id = format!("oauth-{}", pending.provider.to_lowercase().replace(' ', "-"));

    // Google OAuth：发现 project ID
    let project_id = if pending.provider.to_lowercase().contains("google") {
        match discover_google_project(access_token, &client).await {
            Ok(pid) => {
                log::info!("OAuth callback: Google Cloud project 发现成功: {}", pid);
                Some(pid)
            }
            Err(e) => {
                log::warn!("OAuth callback: Google Cloud project 发现失败: {}", e);
                None
            }
        }
    } else {
        None
    };

    let effective_base_url = if pending.provider.to_lowercase().contains("google") {
        if let Some(ref pid) = project_id {
            format!("https://cloudcode-pa.googleapis.com/v1internal#project={}", pid)
        } else {
            "https://cloudcode-pa.googleapis.com/v1internal".to_string()
        }
    } else {
        preset.base_url.to_string()
    };

    let model_array: Vec<serde_json::Value> = preset.models.iter()
        .map(|(mid, mname)| serde_json::json!({"id": mid, "name": mname}))
        .collect();

    let providers_json_str: Option<String> = sqlx::query_scalar(
        "SELECT value FROM settings WHERE key = 'providers'"
    ).fetch_optional(pool).await.ok().flatten();
    let mut providers: Vec<serde_json::Value> = providers_json_str
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let mut oauth_info = serde_json::json!({
        "provider": pending.provider,
        "refreshToken": refresh_token,
        "expiresAt": expires_at,
        "tokenUrl": preset.token_url,
        "clientId": preset.effective_client_id(),
    });
    if let Some(ref pid) = project_id {
        oauth_info["projectId"] = serde_json::Value::String(pid.clone());
    }

    if let Some(existing) = providers.iter_mut().find(|p| p["id"].as_str() == Some(&provider_id)) {
        existing["apiKey"] = serde_json::Value::String(access_token.to_string());
        existing["baseUrl"] = serde_json::Value::String(effective_base_url.clone());
        existing["oauth"] = oauth_info;
    } else {
        providers.push(serde_json::json!({
            "id": provider_id,
            "name": preset.name,
            "apiType": preset.api_type,
            "baseUrl": effective_base_url,
            "apiKey": access_token,
            "models": model_array,
            "enabled": true,
            "oauth": oauth_info,
        }));
    }

    let providers_json = serde_json::to_string(&providers)
        .map_err(|e| format!("序列化 providers 失败: {}", e))?;
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('providers', ?)")
        .bind(&providers_json)
        .execute(pool)
        .await
        .map_err(|e| format!("保存 providers 失败: {}", e))?;

    log::info!("OAuth 回调处理成功: provider={}", preset.name);

    Ok(preset.name.to_string())
}

/// 通过 Cloud Code API 发现用户的 Google Cloud project ID
/// 参考 OpenClaw extensions/google/oauth.project.ts
async fn discover_google_project(access_token: &str, client: &reqwest::Client) -> Result<String, String> {
    let endpoints = [
        "https://cloudcode-pa.googleapis.com",
        "https://daily-cloudcode-pa.sandbox.googleapis.com",
    ];

    let headers_meta = serde_json::json!({
        "ideType": "ANTIGRAVITY",
        "pluginType": "GEMINI",
    });

    let load_body = serde_json::json!({
        "metadata": headers_meta,
    });

    for endpoint in &endpoints {
        let url = format!("{}/v1internal:loadCodeAssist", endpoint);
        let resp = match client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .header("User-Agent", "google-api-nodejs-client/9.15.1")
            .header("Client-Metadata", headers_meta.to_string())
            .json(&load_body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                log::debug!("Cloud Code endpoint {} 请求失败: {}", endpoint, e);
                continue;
            }
        };

        if !resp.status().is_success() {
            log::debug!("Cloud Code endpoint {} 返回 {}", endpoint, resp.status());
            continue;
        }

        let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

        // 尝试从 currentTier + cloudaicompanionProject 获取 project ID
        if let Some(project) = data.get("cloudaicompanionProject") {
            if let Some(pid) = project.as_str() {
                return Ok(pid.to_string());
            }
            if let Some(pid) = project["id"].as_str() {
                return Ok(pid.to_string());
            }
        }

        // 如果没有 project，尝试 onboard
        let tiers = data["allowedTiers"].as_array();
        let tier_id = tiers
            .and_then(|arr| arr.iter().find(|t| t["isDefault"].as_bool() == Some(true)))
            .and_then(|t| t["id"].as_str())
            .unwrap_or("free-tier");

        let onboard_body = serde_json::json!({
            "tierId": tier_id,
            "metadata": headers_meta,
        });

        let onboard_resp = client
            .post(&format!("{}/v1internal:onboardUser", endpoint))
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Content-Type", "application/json")
            .json(&onboard_body)
            .send()
            .await
            .map_err(|e| format!("onboardUser 失败: {}", e))?;

        if onboard_resp.status().is_success() {
            let lro: serde_json::Value = onboard_resp.json().await.map_err(|e| e.to_string())?;
            if let Some(pid) = lro.pointer("/response/cloudaicompanionProject/id").and_then(|v| v.as_str()) {
                return Ok(pid.to_string());
            }
        }
    }

    Err("无法发现 Google Cloud project ID".to_string())
}
