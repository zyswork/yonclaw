//! 飞书 Bot 渠道
//!
//! 通过飞书 WebSocket 长连接接收消息，REST API 发送回复。
//! 桌面端无需公网 IP，适合 Tauri 应用。
//!
//! 流程：
//! 1. 用 app_id + app_secret 获取 tenant_access_token
//! 2. 用 token 获取 WebSocket endpoint
//! 3. 连接 WebSocket，接收事件
//! 4. 处理 im.message.receive_v1 事件
//! 5. 调用 orchestrator 处理消息
//! 6. 通过 REST API 发送回复

use std::sync::Arc;
use prost::Message as ProstMessage;
use tokio_util::sync::CancellationToken;
use crate::agent::Orchestrator;
use super::common::TokenCache;

// ─── 飞书 WebSocket Protobuf 帧定义 ────────────────────
#[derive(Clone, PartialEq, prost::Message)]
struct PbHeader {
    #[prost(string, tag = "1")]
    pub key: String,
    #[prost(string, tag = "2")]
    pub value: String,
}

#[derive(Clone, PartialEq, prost::Message)]
struct PbFrame {
    #[prost(uint64, tag = "1")]
    pub seq_id: u64,
    #[prost(uint64, tag = "2")]
    pub log_id: u64,
    #[prost(int32, tag = "3")]
    pub service: i32,
    #[prost(int32, tag = "4")]
    pub method: i32, // 0=CONTROL(ping/pong) 1=DATA(events)
    #[prost(message, repeated, tag = "5")]
    pub headers: Vec<PbHeader>,
    #[prost(bytes = "vec", optional, tag = "8")]
    pub payload: Option<Vec<u8>>,
}

impl PbFrame {
    fn header_value(&self, key: &str) -> &str {
        self.headers.iter()
            .find(|h| h.key == key)
            .map(|h| h.value.as_str())
            .unwrap_or("")
    }
}

/// 飞书 Bot 配置
pub struct FeishuConfig {
    pub app_id: String,
    pub app_secret: String,
    pub agent_id: String,
}

/// 飞书 API 基地址
const FEISHU_BASE: &str = "https://open.feishu.cn/open-apis";

/// 启动飞书长连接
///
/// 由 ChannelManager 调用，不再自行 spawn，通过 CancellationToken 控制生命周期。
pub async fn start_feishu(
    config: FeishuConfig,
    pool: sqlx::SqlitePool,
    orchestrator: Arc<Orchestrator>,
    app_handle: tauri::AppHandle,
    cancel: CancellationToken,
) -> Result<(), String> {
    let app_id = config.app_id.clone();
    let app_secret = config.app_secret.clone();
    let agent_id = config.agent_id.clone();
    log::info!("飞书: 启动连接 (app_id: {}..., agent={})", &app_id[..app_id.len().min(10)], agent_id);

    let mut attempt: u32 = 0;

    loop {
        if cancel.is_cancelled() {
            log::info!("飞书: 收到取消信号，退出");
            return Ok(());
        }
        match run_feishu_loop(&app_id, &app_secret, &agent_id, &pool, &orchestrator, &app_handle, &cancel).await {
            Ok(_) => {
                // 连接曾成功建立后断开，重置计数
                attempt = 0;
                log::info!("飞书: 连接正常关闭，1s 后重连");
                tokio::select! {
                    _ = cancel.cancelled() => return Ok(()),
                    _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {},
                }
            }
            Err(e) => {
                attempt += 1;
                let delay = super::common::reconnect_delay(attempt);
                if attempt >= 10 {
                    log::error!("飞书: 连接异常: {}，连续失败 {} 次，降级为 {}s 探测模式", e, attempt, delay);
                } else {
                    log::warn!("飞书: 连接异常: {}，第 {} 次重连，{}s 后重试", e, attempt, delay);
                }
                tokio::select! {
                    _ = cancel.cancelled() => return Ok(()),
                    _ = tokio::time::sleep(std::time::Duration::from_secs(delay)) => {},
                }
            }
        }
    }
}

/// 飞书连接主循环
async fn run_feishu_loop(
    app_id: &str,
    app_secret: &str,
    agent_id: &str,
    pool: &sqlx::SqlitePool,
    orchestrator: &Arc<Orchestrator>,
    app_handle: &tauri::AppHandle,
    cancel: &CancellationToken,
) -> Result<(), String> {
    let client = reqwest::Client::new();

    // 1. 获取 tenant_access_token（飞书 token 有效期 7200 秒）
    let (initial_token, ttl) = fetch_tenant_token_raw(&client, app_id, app_secret).await?;
    let token_cache = TokenCache::with_initial(initial_token.clone(), ttl);
    log::info!("飞书: tenant_access_token 获取成功");

    // 2. 尝试 WebSocket 模式
    let ws_result = try_websocket_mode(&client, app_id, app_secret, &initial_token, agent_id, pool, orchestrator, app_handle, cancel, &token_cache).await;

    match ws_result {
        Ok(_) => Ok(()),
        Err(e) => {
            log::warn!("飞书: WebSocket 模式失败 ({}), 降级为轮询模式", e);
            // 降级：定时拉取消息（飞书不支持长轮询，但可以用定时检查）
            polling_fallback(&client, &initial_token, pool, orchestrator, app_handle).await
        }
    }
}

/// 获取 tenant_access_token（原始版本，返回 token + ttl 秒数）
async fn fetch_tenant_token_raw(
    client: &reqwest::Client,
    app_id: &str,
    app_secret: &str,
) -> Result<(String, u64), String> {
    let resp = client.post(format!("{}/auth/v3/tenant_access_token/internal", FEISHU_BASE))
        .json(&serde_json::json!({
            "app_id": app_id,
            "app_secret": app_secret
        }))
        .send().await
        .map_err(|e| format!("获取 token 失败: {}", e))?;

    let data: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析 token 响应失败: {}", e))?;

    if data["code"].as_i64() != Some(0) {
        return Err(format!("飞书 token 错误: {}", data["msg"].as_str().unwrap_or("unknown")));
    }

    let token = data["tenant_access_token"].as_str()
        .map(|s| s.to_string())
        .ok_or("token 字段缺失".to_string())?;
    // 飞书返回 expire 字段（秒），默认 7200
    let ttl = data["expire"].as_u64().unwrap_or(7200);
    Ok((token, ttl))
}

/// 通过 TokenCache 获取 tenant_access_token（自动缓存和刷新）
async fn get_tenant_token_cached(
    token_cache: &Arc<TokenCache>,
    app_id: &str,
    app_secret: &str,
) -> Result<String, String> {
    let aid = app_id.to_string();
    let asec = app_secret.to_string();
    token_cache.get_or_refresh(|| async {
        let client = reqwest::Client::new();
        fetch_tenant_token_raw(&client, &aid, &asec).await
    }).await
}

/// WebSocket 模式（Protobuf 帧协议）
async fn try_websocket_mode(
    client: &reqwest::Client,
    app_id: &str,
    app_secret: &str,
    _token: &str,
    agent_id: &str,
    pool: &sqlx::SqlitePool,
    orchestrator: &Arc<Orchestrator>,
    app_handle: &tauri::AppHandle,
    cancel: &CancellationToken,
    token_cache: &Arc<TokenCache>,
) -> Result<(), String> {
    // 获取 WebSocket endpoint
    let resp = client.post("https://open.feishu.cn/callback/ws/endpoint")
        .json(&serde_json::json!({
            "AppID": app_id,
            "AppSecret": app_secret
        }))
        .send().await
        .map_err(|e| format!("获取 WS endpoint 失败: {}", e))?;

    let data: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析 WS endpoint 响应失败: {}", e))?;

    if data["code"].as_i64() != Some(0) {
        return Err(format!("WS endpoint 错误: {}", data));
    }

    let ws_url = data["data"]["URL"].as_str()
        .ok_or("WS URL 缺失")?;

    // 从 URL 提取 service_id（查询参数 fpid）
    let service_id: i32 = url::Url::parse(ws_url).ok()
        .and_then(|u| u.query_pairs().find(|(k, _)| k == "fpid").map(|(_, v)| v.to_string()))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    log::info!("飞书: WebSocket 连接 {} (service_id={})", &ws_url[..ws_url.len().min(60)], service_id);

    // 连接 WebSocket
    let (ws_stream, _) = tokio_tungstenite::connect_async(ws_url).await
        .map_err(|e| format!("WS 连接失败: {}", e))?;

    use tokio_tungstenite::tungstenite::Message as WsMsg;
    use futures_util::{StreamExt, SinkExt};

    let (mut write, mut read) = ws_stream.split();

    // 事件去重（Arc + Mutex 跨 spawn 共享）
    let seen_ids = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::<String>::new()));
    let mut seq: u64 = 0;
    let mut ping_secs: u64 = 120;
    let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(ping_secs));

    // 发送初始 ping（飞书 SDK 的做法）
    seq += 1;
    let initial_ping = PbFrame {
        seq_id: seq, log_id: 0, service: service_id, method: 0,
        headers: vec![PbHeader { key: "type".into(), value: "ping".into() }],
        payload: None,
    };
    let _ = write.send(WsMsg::Binary(initial_ping.encode_to_vec())).await;

    log::info!("飞书: WebSocket 已连接，已发送初始 ping，等待事件...");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                log::info!("飞书: 收到取消信号，关闭 WebSocket");
                break;
            }
            _ = ping_interval.tick() => {
                seq += 1;
                let ping = PbFrame {
                    seq_id: seq, log_id: 0, service: service_id, method: 0,
                    headers: vec![PbHeader { key: "type".into(), value: "ping".into() }],
                    payload: None,
                };
                if write.send(WsMsg::Binary(ping.encode_to_vec())).await.is_err() {
                    log::warn!("飞书: ping 发送失败，重连");
                    break;
                }
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(WsMsg::Binary(data))) => {
                        // 解析 Protobuf 帧
                        let frame = match PbFrame::decode(&data[..]) {
                            Ok(f) => f,
                            Err(e) => {
                                log::warn!("飞书: Protobuf 解码失败: {}", e);
                                continue;
                            }
                        };

                        // CONTROL 帧（ping/pong）
                        if frame.method == 0 {
                            if frame.header_value("type") == "pong" {
                                // 从 pong payload 更新 ping 间隔
                                if let Some(p) = &frame.payload {
                                    #[derive(serde::Deserialize)]
                                    struct WsCfg { #[serde(rename = "PingInterval")] ping_interval: Option<u64> }
                                    if let Ok(cfg) = serde_json::from_slice::<WsCfg>(p) {
                                        if let Some(secs) = cfg.ping_interval {
                                            let secs = secs.max(10);
                                            if secs != ping_secs {
                                                ping_secs = secs;
                                                ping_interval = tokio::time::interval(std::time::Duration::from_secs(ping_secs));
                                                log::info!("飞书: ping 间隔更新为 {}s", ping_secs);
                                            }
                                        }
                                    }
                                }
                            }
                            continue;
                        }

                        // DATA 帧（事件）— 必须 3 秒内 ACK，否则飞书重发
                        {
                            let mut ack = frame.clone();
                            ack.payload = Some(br#"{"code":200,"headers":{},"data":[]}"#.to_vec());
                            ack.headers.push(PbHeader { key: "biz_rt".into(), value: "0".into() });
                            let _ = write.send(WsMsg::Binary(ack.encode_to_vec())).await;
                        }

                        if let Some(payload) = &frame.payload {
                            if let Ok(event) = serde_json::from_slice::<serde_json::Value>(payload) {
                                log::info!("飞书: 收到事件: type={}", frame.header_value("type"));
                                // 并发处理
                                let aid = app_id.to_string();
                                let asec = app_secret.to_string();
                                let cfg_agent = agent_id.to_string();
                                let p = pool.clone();
                                let o = orchestrator.clone();
                                let h = app_handle.clone();
                                let sids = seen_ids.clone();
                                let tc = token_cache.clone();
                                tokio::spawn(async move {
                                    handle_feishu_event(&event, &sids, &aid, &asec, &cfg_agent, &p, &o, &h, &tc).await;
                                });
                            }
                        }
                    }
                    Some(Ok(WsMsg::Text(text))) => {
                        // 有些飞书版本用 JSON 文本
                        if let Ok(event) = serde_json::from_str::<serde_json::Value>(&text) {
                            let aid = app_id.to_string();
                            let asec = app_secret.to_string();
                            let cfg_agent = agent_id.to_string();
                            let p = pool.clone();
                            let o = orchestrator.clone();
                            let h = app_handle.clone();
                            let sids = seen_ids.clone();
                            let tc = token_cache.clone();
                            tokio::spawn(async move {
                                handle_feishu_event(&event, &sids, &aid, &asec, &cfg_agent, &p, &o, &h, &tc).await;
                            });
                        }
                    }
                    Some(Ok(WsMsg::Ping(d))) => { let _ = write.send(WsMsg::Pong(d)).await; }
                    Some(Ok(WsMsg::Close(_))) => { log::info!("飞书: WebSocket 关闭"); break; }
                    Some(Err(e)) => { log::warn!("飞书: WebSocket 错误: {}", e); break; }
                    None => break,
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

/// 处理飞书事件
async fn handle_feishu_event(
    event: &serde_json::Value,
    seen_ids: &std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    app_id: &str,
    app_secret: &str,
    config_agent_id: &str,
    pool: &sqlx::SqlitePool,
    orchestrator: &Arc<Orchestrator>,
    app_handle: &tauri::AppHandle,
    token_cache: &Arc<TokenCache>,
) {
    // URL 验证 challenge
    if let Some(challenge) = event["challenge"].as_str() {
        log::info!("飞书: 收到 challenge 验证");
        // WebSocket 模式不需要回复 challenge，只做日志
        let _ = challenge;
        return;
    }

    let event_type = event["header"]["event_type"].as_str().unwrap_or("");
    let event_id = event["header"]["event_id"].as_str().unwrap_or("");

    // 去重
    if !event_id.is_empty() {
        if let Ok(mut ids) = seen_ids.lock() {
            if ids.contains(event_id) {
                log::info!("飞书: 跳过重复事件: {}", event_id);
                return;
            }
            ids.insert(event_id.to_string());
            if ids.len() > 1000 { ids.clear(); }
        }
    }

    // 只处理消息事件
    if event_type != "im.message.receive_v1" {
        log::info!("飞书: 忽略事件类型: {}", event_type);
        return;
    }

    let msg = &event["event"]["message"];
    let sender = &event["event"]["sender"];

    // 忽略 bot 自己的消息
    if sender["sender_type"].as_str() == Some("bot") {
        return;
    }

    let message_type = msg["message_type"].as_str().unwrap_or("");
    let chat_id = msg["chat_id"].as_str().unwrap_or("");
    let chat_type = msg["chat_type"].as_str().unwrap_or("p2p");
    let sender_id = sender["sender_id"]["open_id"].as_str().unwrap_or("unknown");

    // 提取文本内容
    let text = match message_type {
        "text" => {
            let content_str = msg["content"].as_str().unwrap_or("{}");
            let content: serde_json::Value = serde_json::from_str(content_str).unwrap_or_default();
            content["text"].as_str().unwrap_or("").to_string()
        }
        _ => {
            log::info!("飞书: 暂不支持的消息类型: {}", message_type);
            return;
        }
    };

    if text.trim().is_empty() {
        return;
    }

    // 群聊中需要 @ 才回复
    if chat_type == "group" {
        // 简单检查：如果消息里没有 @ mention，跳过
        let mentions = msg["mentions"].as_array();
        if mentions.map_or(true, |m| m.is_empty()) {
            return;
        }
    }

    // 清理 @ mention 文本
    let clean_text = text.replace("@_user_1", "").trim().to_string();
    if clean_text.is_empty() { return; }

    log::info!("飞书: [{}] {}: {}", chat_id, sender_id, &clean_text[..clean_text.len().min(50)]);

    // 优先使用 config 中指定的 agent_id，fallback 到 Router
    let agent_id = if !config_agent_id.is_empty() {
        config_agent_id.to_string()
    } else {
        let router = crate::routing::Router::new(orchestrator.pool().clone());
        let route = router.resolve("feishu", Some(sender_id)).await;
        match route {
            Ok(r) => r.agent_id,
            Err(_) => {
                let agents = orchestrator.list_agents().await.unwrap_or_default();
                match agents.into_iter().next() {
                    Some(a) => a.id,
                    None => { log::warn!("飞书: 无可用 Agent"); return; }
                }
            }
        }
    };
    let agent = match orchestrator.get_agent_cached(&agent_id).await {
        Ok(a) => a,
        Err(e) => { log::warn!("飞书: 获取 Agent 失败: {}", e); return; }
    };

    // 获取或创建 session
    let session_title = format!("[飞书] {}", sender_id);
    let session_id = get_or_create_session(pool, &agent.id, chat_id, &session_title).await;

    // 查找 Provider
    let (api_type, api_key, base_url) = match super::find_provider(pool, &agent.model).await {
        Some(info) => info,
        None => {
            send_feishu_message(app_id, app_secret, chat_id, "未配置 LLM Provider，请在桌面端设置中添加。", token_cache).await;
            return;
        }
    };

    use tauri::Manager;
    // 推送用户消息到前端
    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "message", "sessionId": session_id,
        "role": "user", "content": clean_text, "source": "feishu",
    }));

    // 1. 先发一个"思考中"卡片
    let card_msg_id = send_feishu_card(app_id, app_secret, chat_id, "思考中...", true, token_cache).await;

    // 2. 流式调用 orchestrator
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };

    // 后台任务：收集 token 并定时更新卡片
    let card_id = card_msg_id.clone();
    let aid = app_id.to_string();
    let asec = app_secret.to_string();
    let app_for_stream = app_handle.clone();
    let sid_for_stream = session_id.clone();
    let tc_for_stream = token_cache.clone();

    let output_handle = tokio::spawn(async move {
        let mut accumulated = String::new();
        let mut last_update = std::time::Instant::now();
        let update_interval = std::time::Duration::from_millis(1000);

        while let Some(token) = rx.recv().await {
            accumulated.push_str(&token);

            // 推送流式 token 到前端
            let _ = app_for_stream.emit_all("chat-event", serde_json::json!({
                "type": "token", "sessionId": sid_for_stream,
                "content": accumulated.clone(), "source": "feishu",
            }));

            // 节流更新卡片（reasoning 双轨渲染）
            if last_update.elapsed() >= update_interval && !accumulated.is_empty() {
                if let Some(ref msg_id) = card_id {
                    let card_text = format_feishu_reasoning(&accumulated);
                    patch_feishu_card(&aid, &asec, msg_id, &format!("{}▌", card_text), &tc_for_stream).await;
                }
                last_update = std::time::Instant::now();
            }
        }
        accumulated
    });

    let result = orchestrator.send_message_stream(
        &agent.id, &session_id, &clean_text,
        &api_key, &api_type, base_url_opt, tx, None,
    ).await;

    let streamed_output = output_handle.await.unwrap_or_default();

    let reply = match result {
        Ok(resp) => if resp.is_empty() { streamed_output } else { resp },
        Err(e) => format!("处理出错: {}", &e[..e.len().min(100)]),
    };

    // 3. 最终更新卡片为完整回复（reasoning 双轨渲染，去掉光标）
    if let Some(ref msg_id) = card_msg_id {
        if !reply.is_empty() {
            let final_text = format_feishu_reasoning(&reply);
            patch_feishu_card(app_id, app_secret, msg_id, &final_text, token_cache).await;
        }
    } else if !reply.is_empty() {
        // 卡片发送失败的降级：发纯文本
        send_feishu_message(app_id, app_secret, chat_id, &reply, token_cache).await;
    }

    log::info!("飞书: 回复 [{}] {}字符", chat_id, reply.len());

    // 推送完成到前端
    let _ = app_handle.emit_all("chat-event", serde_json::json!({
        "type": "done", "sessionId": session_id,
        "role": "assistant", "content": reply, "source": "feishu",
    }));

    // Session 自动命名
    crate::memory::conversation::auto_name_session(
        pool, &session_id, &clean_text, &api_key, &api_type, base_url_opt,
    ).await;
}

/// 发送飞书交互卡片（用于流式更新）
/// 返回 message_id（用于后续 PATCH 更新）
async fn send_feishu_card(app_id: &str, app_secret: &str, chat_id: &str, text: &str, thinking: bool, token_cache: &Arc<TokenCache>) -> Option<String> {
    let client = reqwest::Client::new();
    let token = get_tenant_token_cached(token_cache, app_id, app_secret).await.ok()?;

    let header_text = if thinking { "思考中..." } else { "小爪" };
    let card = serde_json::json!({
        "config": {"update_multi": true},
        "header": {
            "template": "blue",
            "title": {"content": header_text, "tag": "plain_text"}
        },
        "elements": [
            {"tag": "markdown", "content": text}
        ]
    });

    let body = serde_json::json!({
        "receive_id": chat_id,
        "msg_type": "interactive",
        "content": card.to_string(),
    });

    let resp = client.post(format!("{}/im/v1/messages?receive_id_type=chat_id", FEISHU_BASE))
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send().await.ok()?;

    let data: serde_json::Value = resp.json().await.ok()?;
    if data["code"].as_i64() != Some(0) {
        log::warn!("飞书: 卡片发送失败: {}", data["msg"].as_str().unwrap_or("?"));
        return None;
    }

    let msg_id = data["data"]["message_id"].as_str().map(|s| s.to_string());
    log::info!("飞书: 卡片已发送 msg_id={:?}", msg_id);
    msg_id
}

/// 飞书 reasoning 双轨渲染
///
/// 将含 <think>...</think> 或 Reasoning:\n 的内容格式化为：
/// - thinking 部分 → blockquote（> 💭 **Thinking** ...）
/// - answer 部分 → 正常 markdown
/// - 两者之间用 --- 分隔
fn format_feishu_reasoning(text: &str) -> String {
    // 检测 <think>...</think> 格式
    if let Some(think_start) = text.find("<think>") {
        let after_tag = think_start + 7;
        let (thinking, answer) = if let Some(think_end) = text.find("</think>") {
            let thinking = text[after_tag..think_end].trim();
            let answer = text[think_end + 8..].trim();
            (thinking.to_string(), answer.to_string())
        } else {
            // <think> 还没结束（还在 thinking 中）
            let thinking = text[after_tag..].trim();
            (thinking.to_string(), String::new())
        };
        return build_reasoning_card(&thinking, &answer);
    }

    // 检测 Reasoning:\n 格式
    if text.starts_with("Reasoning:\n") || text.starts_with("Reasoning:\r\n") {
        let without_label = text.strip_prefix("Reasoning:\n")
            .or(text.strip_prefix("Reasoning:\r\n"))
            .unwrap_or(text);
        // 找到 reasoning 结束（通常以空行分隔后是 answer）
        if let Some(sep) = without_label.find("\n\n") {
            let thinking = without_label[..sep].trim();
            let answer = without_label[sep + 2..].trim();
            return build_reasoning_card(thinking, answer);
        }
        // 全部是 thinking
        return build_reasoning_card(without_label.trim(), "");
    }

    // 无 reasoning 标记，原样返回
    text.to_string()
}

/// 构建 thinking + answer 双轨卡片文本
fn build_reasoning_card(thinking: &str, answer: &str) -> String {
    let mut parts = Vec::new();
    if !thinking.is_empty() {
        // 去掉 italic markers（_text_）
        let plain = thinking.lines()
            .map(|line| {
                let l = line.trim();
                let l = l.strip_prefix('_').unwrap_or(l);
                let l = l.strip_suffix('_').unwrap_or(l);
                format!("> {}", l)
            })
            .collect::<Vec<_>>()
            .join("\n");
        parts.push(format!("> 💭 **Thinking**\n{}", plain));
    }
    if !thinking.is_empty() && !answer.is_empty() {
        parts.push("\n---\n".to_string());
    }
    if !answer.is_empty() {
        parts.push(answer.to_string());
    }
    if parts.is_empty() {
        "思考中...".to_string()
    } else {
        parts.join("")
    }
}

/// 更新飞书卡片内容（PATCH，用于流式输出）
async fn patch_feishu_card(app_id: &str, app_secret: &str, message_id: &str, text: &str, token_cache: &Arc<TokenCache>) {
    let token = match get_tenant_token_cached(token_cache, app_id, app_secret).await {
        Ok(t) => t,
        Err(_) => return,
    };
    let client = reqwest::Client::new();

    let card = serde_json::json!({
        "config": {"update_multi": true},
        "header": {
            "template": "blue",
            "title": {"content": "小爪", "tag": "plain_text"}
        },
        "elements": [
            {"tag": "markdown", "content": text}
        ]
    });

    let _ = client.patch(format!("{}/im/v1/messages/{}", FEISHU_BASE, message_id))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({"content": card.to_string()}))
        .send().await;
}

/// 发送飞书纯文本消息（降级用）
async fn send_feishu_message(app_id: &str, app_secret: &str, chat_id: &str, text: &str, token_cache: &Arc<TokenCache>) {
    // 获取 token（走缓存）
    let token = match get_tenant_token_cached(token_cache, app_id, app_secret).await {
        Ok(t) => t,
        Err(e) => { log::warn!("飞书: 发送消息失败（token）: {}", e); return; }
    };

    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "receive_id": chat_id,
        "msg_type": "text",
        "content": serde_json::json!({"text": text}).to_string(),
    });

    let resp = client.post(format!("{}/im/v1/messages?receive_id_type=chat_id", FEISHU_BASE))
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .send().await;

    match resp {
        Ok(r) => {
            if let Ok(data) = r.json::<serde_json::Value>().await {
                if data["code"].as_i64() != Some(0) {
                    log::warn!("飞书: 发送消息失败: {}", data["msg"].as_str().unwrap_or("?"));
                }
            }
        }
        Err(e) => log::warn!("飞书: 发送消息请求失败: {}", e),
    }
}

/// 轮询降级模式（WebSocket 不可用时）
async fn polling_fallback(
    _client: &reqwest::Client,
    _token: &str,
    _pool: &sqlx::SqlitePool,
    _orchestrator: &Arc<Orchestrator>,
    _app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    log::info!("飞书: 轮询模式暂未实现，请确保 WebSocket 可用");
    // 飞书不像 Telegram 有 getUpdates，需要 webhook 或 WebSocket
    tokio::time::sleep(std::time::Duration::from_secs(300)).await;
    Ok(())
}

/// 获取或创建飞书 session
async fn get_or_create_session(pool: &sqlx::SqlitePool, agent_id: &str, chat_id: &str, title: &str) -> String {
    let tag = format!("feishu-{}", chat_id);

    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM chat_sessions WHERE title LIKE '%' || ? || '%' OR title = ? LIMIT 1"
    ).bind(&tag).bind(title).fetch_optional(pool).await.ok().flatten();

    if let Some((id,)) = existing {
        return id;
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();
    let _ = sqlx::query(
        "INSERT INTO chat_sessions (id, agent_id, title, created_at) VALUES (?, ?, ?, ?)"
    ).bind(&id).bind(agent_id).bind(title).bind(now).execute(pool).await;

    id
}
