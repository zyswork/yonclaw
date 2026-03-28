//! 企业微信（WeCom）频道
//!
//! 通过 HTTP 回调服务器接收企业微信推送的消息，REST API 发送回复。
//! 桌面端启动一个本地 HTTP 服务器，配合内网穿透或公网 IP 使用。
//!
//! 流程：
//! 1. 用 corp_id + secret 获取 access_token
//! 2. 启动 HTTP 回调服务器（hyper）
//! 3. 处理企业微信 URL 验证（GET 请求）
//! 4. 接收消息推送（POST 请求），解密消息体
//! 5. 调用 orchestrator 处理消息
//! 6. 通过企业微信 API 发送回复

use std::sync::Arc;
use std::convert::Infallible;
use std::net::SocketAddr;
use tokio_util::sync::CancellationToken;
use crate::agent::Orchestrator;
use super::common::TokenCache;

/// 企业微信 API 基地址
const WECOM_API_BASE: &str = "https://qyapi.weixin.qq.com/cgi-bin";

/// 企业微信 Bot 配置
pub struct WeComConfig {
    /// 企业 ID
    pub corp_id: String,
    /// 企业微信应用的 AgentId（数字）
    pub agent_id_wecom: i64,
    /// 应用 Secret
    pub secret: String,
    /// 回调 Token（用于签名验证）
    pub token: String,
    /// 回调加密 Key（AES 解密用，Base64 编码的 43 字符）
    pub encoding_aes_key: String,
    /// 我们系统中的 Agent ID
    pub agent_id: String,
    /// 回调服务器监听端口（默认 9876）
    pub callback_port: u16,
}

/// 共享状态，在 HTTP handler 之间传递
struct SharedState {
    config: WeComConfig,
    pool: sqlx::SqlitePool,
    orchestrator: Arc<Orchestrator>,
    app_handle: tauri::AppHandle,
    /// access_token 缓存（通用 TokenCache）
    token_cache: Arc<TokenCache>,
    /// 事件去重
    seen_ids: std::sync::Mutex<std::collections::HashSet<String>>,
}

/// 启动企业微信频道
///
/// 由 ChannelManager 调用，通过 CancellationToken 控制生命周期。
pub async fn start_wecom(
    config: WeComConfig,
    pool: sqlx::SqlitePool,
    orchestrator: Arc<Orchestrator>,
    app_handle: tauri::AppHandle,
    cancel: CancellationToken,
) -> Result<(), String> {
    let port = config.callback_port;
    let corp_id = config.corp_id.clone();
    let agent_id = config.agent_id.clone();
    log::info!(
        "企业微信: 启动回调服务器 (corp_id: {}..., wecom_agent={}, agent={}, port={})",
        &corp_id[..corp_id.len().min(10)], config.agent_id_wecom, agent_id, port
    );

    // 先验证 access_token 可获取
    let client = reqwest::Client::new();
    let initial_token = get_access_token(&client, &config.corp_id, &config.secret).await?;
    log::info!("企业微信: access_token 获取成功");

    // 企业微信 access_token 有效期 7200 秒
    let token_cache = TokenCache::with_initial(initial_token, 7200);

    let state = Arc::new(SharedState {
        config,
        pool,
        orchestrator,
        app_handle,
        token_cache,
        seen_ids: std::sync::Mutex::new(std::collections::HashSet::new()),
    });

    // 启动 HTTP 回调服务器
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let make_svc = hyper::service::make_service_fn(move |_conn| {
        let state = state.clone();
        async move {
            Ok::<_, Infallible>(hyper::service::service_fn(move |req| {
                handle_http_request(req, state.clone())
            }))
        }
    });

    let server = hyper::Server::bind(&addr).serve(make_svc);
    log::info!("企业微信: HTTP 回调服务器已启动，监听 0.0.0.0:{}", port);

    // 使用 graceful shutdown
    let graceful = server.with_graceful_shutdown(async move {
        cancel.cancelled().await;
        log::info!("企业微信: 收到取消信号，关闭回调服务器");
    });

    graceful.await.map_err(|e| format!("企业微信 HTTP 服务器错误: {}", e))?;

    log::info!("企业微信: 回调服务器已关闭");
    Ok(())
}

// ─── HTTP 请求处理 ─────────────────────────────────────

/// 处理来自企业微信的 HTTP 请求
async fn handle_http_request(
    req: hyper::Request<hyper::Body>,
    state: Arc<SharedState>,
) -> Result<hyper::Response<hyper::Body>, Infallible> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let query = uri.query().unwrap_or("");

    log::info!("企业微信: {} {} query={}", method, uri.path(), &query[..query.len().min(100)]);

    let response = match method {
        hyper::Method::GET => {
            // URL 验证请求
            handle_url_verification(query, &state).await
        }
        hyper::Method::POST => {
            // 消息推送
            let body_bytes = match hyper::body::to_bytes(req.into_body()).await {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("企业微信: 读取请求体失败: {}", e);
                    return Ok(hyper::Response::builder()
                        .status(400)
                        .body(hyper::Body::from("bad request"))
                        .unwrap_or_else(|_| hyper::Response::new(hyper::Body::from("internal error"))));
                }
            };
            handle_message_callback(query, &body_bytes, &state).await
        }
        _ => {
            Ok(hyper::Response::builder()
                .status(405)
                .body(hyper::Body::from("method not allowed"))
                .unwrap_or_else(|_| hyper::Response::new(hyper::Body::from("internal error"))))
        }
    };

    match response {
        Ok(resp) => Ok(resp),
        Err(e) => {
            log::warn!("企业微信: 处理请求失败: {}", e);
            Ok(hyper::Response::builder()
                .status(500)
                .body(hyper::Body::from(format!("error: {}", e)))
                .unwrap_or_else(|_| hyper::Response::new(hyper::Body::from("internal error"))))
        }
    }
}

/// 处理 URL 验证（GET 请求）
///
/// 企业微信在配置回调地址时会发送验证请求：
/// GET /callback?msg_signature=xxx&timestamp=xxx&nonce=xxx&echostr=xxx
/// 需要验证签名并解密 echostr，返回明文
async fn handle_url_verification(
    query: &str,
    state: &SharedState,
) -> Result<hyper::Response<hyper::Body>, String> {
    let params = parse_query_params(query);
    let msg_signature = params.get("msg_signature").map(|s| s.as_str()).unwrap_or("");
    let timestamp = params.get("timestamp").map(|s| s.as_str()).unwrap_or("");
    let nonce = params.get("nonce").map(|s| s.as_str()).unwrap_or("");
    let echostr = params.get("echostr").map(|s| s.as_str()).unwrap_or("");

    if echostr.is_empty() {
        return Ok(hyper::Response::builder()
            .status(200)
            .body(hyper::Body::from("ok"))
            .unwrap_or_else(|_| hyper::Response::new(hyper::Body::from("internal error"))));
    }

    // 验证签名
    let computed_sig = compute_signature(&state.config.token, timestamp, nonce, echostr);
    if computed_sig != msg_signature {
        log::warn!("企业微信: URL 验证签名不匹配: computed={}, expected={}", computed_sig, msg_signature);
        return Err("签名验证失败".to_string());
    }

    // 解密 echostr
    let decrypted = decrypt_message(&state.config.encoding_aes_key, echostr)?;

    log::info!("企业微信: URL 验证成功");
    Ok(hyper::Response::builder()
        .status(200)
        .body(hyper::Body::from(decrypted))
        .unwrap_or_else(|_| hyper::Response::new(hyper::Body::from("internal error"))))
}

/// 处理消息回调（POST 请求）
///
/// 企业微信推送消息到回调地址：
/// POST /callback?msg_signature=xxx&timestamp=xxx&nonce=xxx
/// Body 为 XML 格式，包含加密后的消息
async fn handle_message_callback(
    query: &str,
    body: &[u8],
    state: &SharedState,
) -> Result<hyper::Response<hyper::Body>, String> {
    let params = parse_query_params(query);
    let msg_signature = params.get("msg_signature").map(|s| s.as_str()).unwrap_or("");
    let timestamp = params.get("timestamp").map(|s| s.as_str()).unwrap_or("");
    let nonce = params.get("nonce").map(|s| s.as_str()).unwrap_or("");

    let body_str = String::from_utf8_lossy(body);
    log::debug!("企业微信: 收到回调 body: {}", &body_str[..body_str.len().min(500)]);

    // 从 XML 中提取加密内容
    let encrypt = extract_xml_field(&body_str, "Encrypt")
        .ok_or("XML 中缺少 Encrypt 字段")?;

    // 验证签名
    let computed_sig = compute_signature(&state.config.token, timestamp, nonce, &encrypt);
    if computed_sig != msg_signature {
        log::warn!("企业微信: 消息签名不匹配");
        return Err("签名验证失败".to_string());
    }

    // 解密消息
    let decrypted_xml = decrypt_message(&state.config.encoding_aes_key, &encrypt)?;
    log::debug!("企业微信: 解密后消息: {}", &decrypted_xml[..decrypted_xml.len().min(500)]);

    // 解析消息
    let msg_type = extract_xml_field(&decrypted_xml, "MsgType").unwrap_or_default();
    let msg_id = extract_xml_field(&decrypted_xml, "MsgId").unwrap_or_default();
    let from_user = extract_xml_field(&decrypted_xml, "FromUserName").unwrap_or_default();
    let content = extract_xml_field(&decrypted_xml, "Content").unwrap_or_default();
    let agent_id_str = extract_xml_field(&decrypted_xml, "AgentID").unwrap_or_default();

    log::info!(
        "企业微信: 收到消息 type={} from={} agent={} content={}",
        msg_type, from_user, agent_id_str, &content[..content.len().min(50)]
    );

    // 只处理文本消息
    if msg_type == "text" && !content.trim().is_empty() {
        // 去重
        let is_dup = if !msg_id.is_empty() {
            if let Ok(mut ids) = state.seen_ids.lock() {
                if ids.contains(&msg_id) {
                    true
                } else {
                    ids.insert(msg_id.clone());
                    if ids.len() > 1000 { ids.clear(); }
                    false
                }
            } else { false }
        } else { false };

        if !is_dup {
            // 异步处理消息，不阻塞回调响应
            let msg_ctx = Arc::new(WeComMessageContext {
                corp_id: state.config.corp_id.clone(),
                secret: state.config.secret.clone(),
                agent_id_wecom: state.config.agent_id_wecom,
                config_agent_id: state.config.agent_id.clone(),
                pool: state.pool.clone(),
                orchestrator: state.orchestrator.clone(),
                app_handle: state.app_handle.clone(),
                token_cache: state.token_cache.clone(),
            });
            let from = from_user.clone();
            let text = content.clone();
            tokio::spawn(async move {
                handle_wecom_message(&msg_ctx, &from, &text).await;
            });
        } else {
            log::info!("企业微信: 跳过重复消息: {}", msg_id);
        }
    } else if msg_type == "event" {
        let event = extract_xml_field(&decrypted_xml, "Event").unwrap_or_default();
        log::info!("企业微信: 收到事件: {}", event);
        // 可扩展：关注/取消关注、菜单点击等
    }

    // 企业微信要求在 5 秒内返回 "success" 或空字符串
    Ok(hyper::Response::builder()
        .status(200)
        .body(hyper::Body::from("success"))
        .unwrap_or_else(|_| hyper::Response::new(hyper::Body::from("internal error"))))
}

// ─── 消息处理 ──────────────────────────────────────────

/// 消息处理上下文
struct WeComMessageContext {
    corp_id: String,
    secret: String,
    agent_id_wecom: i64,
    config_agent_id: String,
    pool: sqlx::SqlitePool,
    orchestrator: Arc<Orchestrator>,
    app_handle: tauri::AppHandle,
    /// 共享 token 缓存
    token_cache: Arc<TokenCache>,
}

/// 处理企业微信文本消息
async fn handle_wecom_message(
    ctx: &WeComMessageContext,
    from_user: &str,
    text: &str,
) {
    let clean_text = text.trim().to_string();
    if clean_text.is_empty() { return; }

    log::info!("企业微信: [{}] {}", from_user, &clean_text[..clean_text.len().min(50)]);

    // 获取 access_token（走缓存）
    let corp_id = ctx.corp_id.clone();
    let secret = ctx.secret.clone();
    let access_token = match ctx.token_cache.get_or_refresh(|| async {
        let client = reqwest::Client::new();
        let token = get_access_token(&client, &corp_id, &secret).await?;
        log::info!("企业微信: access_token 已刷新");
        // 企业微信 access_token 有效期 7200 秒
        Ok((token, 7200))
    }).await {
        Ok(t) => t,
        Err(e) => {
            log::error!("企业微信: 获取 access_token 失败: {}", e);
            return;
        }
    };

    // 优先使用 config 中指定的 agent_id，fallback 到 Router
    let agent_id = if !ctx.config_agent_id.is_empty() {
        ctx.config_agent_id.clone()
    } else {
        let router = crate::routing::Router::new(ctx.orchestrator.pool().clone());
        let route = router.resolve("wecom", Some(from_user)).await;
        match route {
            Ok(r) => r.agent_id,
            Err(_) => {
                let agents = ctx.orchestrator.list_agents().await.unwrap_or_default();
                match agents.into_iter().next() {
                    Some(a) => a.id,
                    None => { log::warn!("企业微信: 无可用 Agent"); return; }
                }
            }
        }
    };

    let agent = match ctx.orchestrator.get_agent_cached(&agent_id).await {
        Ok(a) => a,
        Err(e) => { log::warn!("企业微信: 获取 Agent 失败: {}", e); return; }
    };

    // 获取或创建 session
    let session_title = format!("[企业微信] {}", from_user);
    let session_id = get_or_create_session(&ctx.pool, &agent.id, from_user, &session_title).await;

    // 查找 Provider
    let (api_type, api_key, base_url) = match super::find_provider(&ctx.pool, &agent.model).await {
        Some(info) => info,
        None => {
            send_text_message(&access_token, from_user, ctx.agent_id_wecom, "未配置 LLM Provider，请在桌面端设置中添加。").await;
            return;
        }
    };

    use tauri::Manager;

    // 推送用户消息到前端
    let _ = ctx.app_handle.emit_all("chat-event", serde_json::json!({
        "type": "message", "sessionId": session_id,
        "role": "user", "content": clean_text, "source": "wecom",
    }));

    // 推送"思考中"
    let _ = ctx.app_handle.emit_all("chat-event", serde_json::json!({
        "type": "thinking", "sessionId": session_id, "source": "wecom",
    }));

    // 流式调用 orchestrator
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };

    // 后台收集 token 并推送流式到桌面端
    let app_for_stream = ctx.app_handle.clone();
    let sid_for_stream = session_id.clone();
    let output_handle = tokio::spawn(async move {
        let mut output = String::new();
        while let Some(token) = rx.recv().await {
            output.push_str(&token);
            let _ = app_for_stream.emit_all("chat-event", serde_json::json!({
                "type": "token", "sessionId": sid_for_stream,
                "content": output.clone(), "source": "wecom",
            }));
        }
        output
    });

    let result = ctx.orchestrator.send_message_stream(
        &agent.id, &session_id, &clean_text,
        &api_key, &api_type, base_url_opt, tx, None,
    ).await;

    let streamed_output = output_handle.await.unwrap_or_default();

    let reply = match result {
        Ok(resp) => if resp.is_empty() { streamed_output } else { resp },
        Err(e) => format!("处理出错: {}", &e[..e.len().min(100)]),
    };

    // 发送回复到企业微信
    if !reply.is_empty() {
        send_text_message(&access_token, from_user, ctx.agent_id_wecom, &reply).await;
        log::info!("企业微信: 回复 [{}] {}字符", from_user, reply.len());
    }

    // 推送完成到前端
    let _ = ctx.app_handle.emit_all("chat-event", serde_json::json!({
        "type": "done", "sessionId": session_id,
        "role": "assistant", "content": reply, "source": "wecom",
    }));

    // Session 自动命名
    crate::memory::conversation::auto_name_session(
        &ctx.pool, &session_id, &clean_text, &api_key, &api_type, base_url_opt,
    ).await;
}

// ─── 企业微信 API ──────────────────────────────────────

/// 获取企业微信 access_token
///
/// access_token 有效期 7200 秒，建议缓存。
/// 此处每次调用均重新获取（简化实现），生产环境应配合 TokenCache。
async fn get_access_token(
    client: &reqwest::Client,
    corp_id: &str,
    secret: &str,
) -> Result<String, String> {
    let url = format!(
        "{}/gettoken?corpid={}&corpsecret={}",
        WECOM_API_BASE, corp_id, secret
    );
    let resp: serde_json::Value = client.get(&url).send().await
        .map_err(|e| format!("请求失败: {}", e))?
        .json().await
        .map_err(|e| format!("解析失败: {}", e))?;

    if resp["errcode"].as_i64() != Some(0) {
        return Err(format!(
            "WeCom API 错误: errcode={} errmsg={}",
            resp["errcode"], resp["errmsg"].as_str().unwrap_or("")
        ));
    }

    resp["access_token"].as_str()
        .map(String::from)
        .ok_or("access_token 为空".into())
}

/// 发送文本消息到企业微信用户
async fn send_text_message(
    access_token: &str,
    to_user: &str,
    agent_id_wecom: i64,
    content: &str,
) {
    let client = reqwest::Client::new();

    // 企业微信单条消息最大 2048 字节，超长需要分段
    let chunks = split_message(content, 2000);

    for chunk in chunks {
        let body = serde_json::json!({
            "touser": to_user,
            "msgtype": "text",
            "agentid": agent_id_wecom,
            "text": { "content": chunk },
        });

        match client
            .post(format!("{}/message/send?access_token={}", WECOM_API_BASE, access_token))
            .json(&body)
            .send().await
        {
            Ok(resp) => {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    if data["errcode"].as_i64() != Some(0) {
                        log::warn!(
                            "企业微信: 发送消息失败: errcode={} errmsg={}",
                            data["errcode"], data["errmsg"].as_str().unwrap_or("?")
                        );
                    }
                }
            }
            Err(e) => log::warn!("企业微信: 发送消息请求失败: {}", e),
        }
    }
}

/// 发送 Markdown 消息（企业微信应用支持）
#[allow(dead_code)]
async fn send_markdown_message(
    access_token: &str,
    to_user: &str,
    agent_id_wecom: i64,
    content: &str,
) {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "touser": to_user,
        "msgtype": "markdown",
        "agentid": agent_id_wecom,
        "markdown": { "content": content },
    });

    match client
        .post(format!("{}/message/send?access_token={}", WECOM_API_BASE, access_token))
        .json(&body)
        .send().await
    {
        Ok(resp) => {
            if let Ok(data) = resp.json::<serde_json::Value>().await {
                if data["errcode"].as_i64() != Some(0) {
                    log::warn!(
                        "企业微信: 发送 Markdown 失败: errcode={} errmsg={}",
                        data["errcode"], data["errmsg"].as_str().unwrap_or("?")
                    );
                }
            }
        }
        Err(e) => log::warn!("企业微信: 发送 Markdown 请求失败: {}", e),
    }
}

// ─── 消息加解密 ────────────────────────────────────────

/// 计算企业微信消息签名
///
/// 签名算法：SHA1(sort([token, timestamp, nonce, encrypt_str]))
fn compute_signature(token: &str, timestamp: &str, nonce: &str, encrypt_str: &str) -> String {
    use sha1::Digest;

    let mut params = vec![token, timestamp, nonce, encrypt_str];
    params.sort();
    let joined = params.join("");

    let mut hasher = sha1::Sha1::new();
    hasher.update(joined.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

/// 解密企业微信消息
///
/// 使用 AES-256-CBC 解密：
/// 1. encoding_aes_key（Base64 编码的 43 字符） + "=" → Base64 解码得到 32 字节 AES Key
/// 2. AES Key 的前 16 字节作为 IV
/// 3. 解密后格式：random(16B) + msg_len(4B, big-endian) + msg + corp_id
fn decrypt_message(encoding_aes_key: &str, encrypted: &str) -> Result<String, String> {
    use aes::cipher::{BlockDecryptMut, KeyIvInit};
    use base64::Engine;

    // 1. 解码 AES Key
    let aes_key_b64 = format!("{}=", encoding_aes_key);
    let aes_key = base64::engine::general_purpose::STANDARD.decode(&aes_key_b64)
        .map_err(|e| format!("encoding_aes_key Base64 解码失败: {}", e))?;

    if aes_key.len() != 32 {
        return Err(format!("AES Key 长度错误: {} (应为 32)", aes_key.len()));
    }

    // 2. IV = AES Key 的前 16 字节
    let iv = &aes_key[..16];

    // 3. Base64 解码密文
    let ciphertext = base64::engine::general_purpose::STANDARD.decode(encrypted)
        .map_err(|e| format!("密文 Base64 解码失败: {}", e))?;

    // 4. AES-256-CBC 解密
    type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;
    let mut buf = ciphertext.clone();
    let decrypted = Aes256CbcDec::new_from_slices(&aes_key, iv)
        .map_err(|e| format!("AES 初始化失败: {}", e))?
        .decrypt_padded_mut::<cbc::cipher::block_padding::Pkcs7>(&mut buf)
        .map_err(|e| format!("AES 解密失败: {}", e))?;

    // 5. 解析明文：random(16B) + msg_len(4B) + msg + corp_id
    if decrypted.len() < 20 {
        return Err("解密后数据太短".to_string());
    }

    let msg_len = u32::from_be_bytes([
        decrypted[16], decrypted[17], decrypted[18], decrypted[19],
    ]) as usize;

    if 20 + msg_len > decrypted.len() {
        return Err(format!(
            "消息长度错误: msg_len={}, available={}",
            msg_len, decrypted.len() - 20
        ));
    }

    let msg = &decrypted[20..20 + msg_len];
    String::from_utf8(msg.to_vec())
        .map_err(|e| format!("消息 UTF-8 解码失败: {}", e))
}

/// 加密消息（用于被动回复，暂未使用，预留）
#[allow(dead_code)]
fn encrypt_message(encoding_aes_key: &str, msg: &str, corp_id: &str) -> Result<String, String> {
    use aes::cipher::{BlockEncryptMut, KeyIvInit};
    use base64::Engine;

    let aes_key_b64 = format!("{}=", encoding_aes_key);
    let aes_key = base64::engine::general_purpose::STANDARD.decode(&aes_key_b64)
        .map_err(|e| format!("encoding_aes_key Base64 解码失败: {}", e))?;

    if aes_key.len() != 32 {
        return Err(format!("AES Key 长度错误: {}", aes_key.len()));
    }

    let iv = &aes_key[..16];

    // 构造明文：random(16B) + msg_len(4B) + msg + corp_id
    let msg_bytes = msg.as_bytes();
    let corp_bytes = corp_id.as_bytes();
    let msg_len = (msg_bytes.len() as u32).to_be_bytes();

    let mut plaintext = Vec::new();
    // 16 字节随机数
    for _ in 0..16 {
        plaintext.push(rand_byte());
    }
    plaintext.extend_from_slice(&msg_len);
    plaintext.extend_from_slice(msg_bytes);
    plaintext.extend_from_slice(corp_bytes);

    // PKCS7 padding
    let block_size = 16;
    let pad_len = block_size - (plaintext.len() % block_size);
    for _ in 0..pad_len {
        plaintext.push(pad_len as u8);
    }

    // AES-256-CBC 加密
    type Aes256CbcEnc = cbc::Encryptor<aes::Aes256>;
    let encryptor = Aes256CbcEnc::new_from_slices(&aes_key, iv)
        .map_err(|e| format!("AES 初始化失败: {}", e))?;

    let mut buf = plaintext;
    // encrypt_padded_mut 需要已 padded 的数据，使用 NoPadding
    let ct_len = buf.len();
    encryptor.encrypt_padded_mut::<cbc::cipher::block_padding::NoPadding>(&mut buf, ct_len)
        .map_err(|e| format!("AES 加密失败: {}", e))?;

    Ok(base64::engine::general_purpose::STANDARD.encode(&buf))
}

/// 简单的伪随机字节（非安全场景用）
#[allow(dead_code)]
fn rand_byte() -> u8 {
    use std::time::SystemTime;
    let d = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    ((d.subsec_nanos() ^ d.as_secs() as u32) & 0xFF) as u8
}

// ─── XML 解析（简易实现，避免引入 xml 库）──────────────

/// 从 XML 中提取指定标签的内容
///
/// 支持 `<Tag>content</Tag>` 和 `<Tag><![CDATA[content]]></Tag>` 两种格式
fn extract_xml_field(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);

    let start = xml.find(&open)? + open.len();
    let end = xml.find(&close)?;

    if start >= end { return None; }

    let content = &xml[start..end];

    // 处理 CDATA
    if content.starts_with("<![CDATA[") && content.ends_with("]]>") {
        Some(content[9..content.len() - 3].to_string())
    } else {
        Some(content.to_string())
    }
}

// ─── 工具函数 ──────────────────────────────────────────

/// 解析 URL 查询参数
fn parse_query_params(query: &str) -> std::collections::HashMap<String, String> {
    let mut params = std::collections::HashMap::new();
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            params.insert(
                urlencoding::decode(k).unwrap_or_default().to_string(),
                urlencoding::decode(v).unwrap_or_default().to_string(),
            );
        }
    }
    params
}

/// 分割长消息为多段（企业微信单条限制约 2048 字节）
fn split_message(text: &str, max_len: usize) -> Vec<&str> {
    if text.len() <= max_len {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();

    while start < bytes.len() {
        let mut end = (start + max_len).min(bytes.len());

        // 确保不在 UTF-8 多字节字符中间截断
        while end < bytes.len() && !text.is_char_boundary(end) {
            end -= 1;
        }

        // 尝试在换行符处分割
        if end < bytes.len() {
            if let Some(nl) = text[start..end].rfind('\n') {
                if nl > 0 {
                    end = start + nl + 1;
                }
            }
        }

        chunks.push(&text[start..end]);
        start = end;
    }

    chunks
}

/// 获取或创建企业微信 session
async fn get_or_create_session(
    pool: &sqlx::SqlitePool,
    agent_id: &str,
    user_id: &str,
    title: &str,
) -> String {
    let tag = format!("wecom-{}", user_id);

    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM chat_sessions WHERE title LIKE '%' || ? || '%' OR title = ? LIMIT 1"
    ).bind(&tag).bind(title).fetch_optional(pool).await.ok().flatten();

    if let Some((id,)) = existing {
        return id;
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let full_title = format!("{} {}", title, tag);
    let _ = sqlx::query(
        "INSERT INTO chat_sessions (id, agent_id, title, created_at) VALUES (?, ?, ?, ?)"
    ).bind(&id).bind(agent_id).bind(&full_title).bind(now).execute(pool).await;

    id
}

// ─── 公开 API（供其他模块调用）────────────────────────

/// 主动发送企业微信消息（供 gateway 等模块调用）
pub async fn send_message(
    access_token: &str,
    user_id: &str,
    agent_id_num: i64,
    content: &str,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "touser": user_id,
        "msgtype": "text",
        "agentid": agent_id_num,
        "text": { "content": content },
    });

    let resp: serde_json::Value = client
        .post(format!("{}/message/send?access_token={}", WECOM_API_BASE, access_token))
        .json(&body)
        .send().await.map_err(|e| format!("发送失败: {}", e))?
        .json().await.map_err(|e| format!("解析失败: {}", e))?;

    if resp["errcode"].as_i64() != Some(0) {
        return Err(format!("发送失败: {}", resp["errmsg"].as_str().unwrap_or("")));
    }
    Ok(())
}
