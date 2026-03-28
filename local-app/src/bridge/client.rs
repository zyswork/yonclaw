//! Bridge WebSocket 客户端
//!
//! 连接 Cloud Gateway，注册能力，心跳保活，接收转发消息。

use std::sync::Arc;
use tokio::sync::mpsc;
use futures_util::{SinkExt, StreamExt};

/// Bridge 配置
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// Cloud Gateway WebSocket URL (如 wss://zys-openclaw.com/ws/bridge)
    pub gateway_url: String,
    /// API Key
    pub api_key: String,
    /// 设备 ID
    pub device_id: String,
    /// 心跳间隔（秒）
    pub heartbeat_secs: u64,
}

/// Bridge 客户端
pub struct BridgeClient {
    config: BridgeConfig,
    /// Agent IDs（注册时告知云端）
    agent_ids: Vec<String>,
    /// 工具能力列表
    capabilities: Vec<String>,
}

/// 从云端转发来的消息
#[derive(Debug, Clone)]
pub struct ForwardedMessage {
    pub request_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub message: String,
    pub sender_channel: String,
}

impl BridgeClient {
    pub fn new(config: BridgeConfig) -> Self {
        Self {
            config,
            agent_ids: Vec::new(),
            capabilities: Vec::new(),
        }
    }

    pub fn with_agents(mut self, agents: Vec<String>) -> Self {
        self.agent_ids = agents;
        self
    }

    pub fn with_capabilities(mut self, caps: Vec<String>) -> Self {
        self.capabilities = caps;
        self
    }

    /// 启动 Bridge 连接（后台运行，自动重连）
    pub async fn start(
        self,
        pool: sqlx::SqlitePool,
        orchestrator: Arc<crate::agent::Orchestrator>,
        message_tx: mpsc::UnboundedSender<ForwardedMessage>,
    ) {
        let config = self.config.clone();
        let agent_ids = self.agent_ids.clone();
        let capabilities = self.capabilities.clone();

        tokio::spawn(async move {
            let mut retry_delay = 1u64;

            loop {
                log::info!("Bridge: 连接 {}", config.gateway_url);

                match Self::connect_and_run(
                    &config, &agent_ids, &capabilities, &pool, &orchestrator, &message_tx,
                ).await {
                    Ok(()) => {
                        log::info!("Bridge: 连接正常关闭");
                        retry_delay = 1;
                    }
                    Err(e) => {
                        log::warn!("Bridge: 连接断开: {}，{}秒后重连", e, retry_delay);
                    }
                }

                tokio::time::sleep(std::time::Duration::from_secs(retry_delay)).await;
                retry_delay = (retry_delay * 2).min(60); // 指数退避，最大 60s
            }
        });
    }

    async fn connect_and_run(
        config: &BridgeConfig,
        agent_ids: &[String],
        capabilities: &[String],
        _pool: &sqlx::SqlitePool,
        _orchestrator: &Arc<crate::agent::Orchestrator>,
        message_tx: &mpsc::UnboundedSender<ForwardedMessage>,
    ) -> Result<(), String> {
        // 连接 WebSocket
        let (ws_stream, _) = tokio_tungstenite::connect_async(&config.gateway_url)
            .await
            .map_err(|e| format!("WebSocket 连接失败: {}", e))?;

        log::info!("Bridge: WebSocket 已连接");

        let (mut write, mut read) = ws_stream.split();

        // 发送 register 消息
        let register_msg = serde_json::json!({
            "type": "register",
            "deviceId": config.device_id,
            "platform": std::env::consts::OS,
            "version": env!("CARGO_PKG_VERSION"),
            "capabilities": capabilities,
            "agents": agent_ids,
        });

        let register_json = serde_json::to_string(&register_msg)
            .map_err(|e| format!("register 序列化失败: {}", e))?;
        write.send(tokio_tungstenite::tungstenite::Message::Text(register_json))
            .await.map_err(|e| format!("发送 register 失败: {}", e))?;

        log::info!("Bridge: 已发送 register（{} capabilities, {} agents）", capabilities.len(), agent_ids.len());

        // 启动时同步：从云端拉取离线期间的数据
        {
            let sync_url = config.gateway_url
                .replace("ws://", "http://").replace("wss://", "https://")
                .replace("/ws/bridge", "/api/v1/sync/pull");
            let last_sync: i64 = sqlx::query_scalar::<_, String>(
                "SELECT value FROM settings WHERE key = 'cloud_last_sync_at'"
            ).fetch_optional(_pool).await.ok().flatten()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);

            log::info!("Bridge: 同步拉取 since={}", last_sync);
            let client = reqwest::Client::new();
            match client.post(&sync_url)
                .json(&serde_json::json!({
                    "deviceId": config.device_id,
                    "lastSyncAt": last_sync,
                }))
                .send().await
            {
                Ok(resp) => {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        let pulled = body["pulled"].as_i64().unwrap_or(0);
                        let synced_at = body["syncedAt"].as_i64().unwrap_or(0);

                        // 写入本地 SQLite
                        if let Some(messages) = body["data"]["chat_messages"].as_array() {
                            for m in messages {
                                let sync_id = m["sync_id"].as_str().unwrap_or("");
                                if sync_id.is_empty() { continue; }
                                let _ = sqlx::query(
                                    "INSERT OR IGNORE INTO chat_messages (id, session_id, agent_id, role, content, seq, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)"
                                )
                                .bind(sync_id)
                                .bind(m["session_id"].as_str().unwrap_or(""))
                                .bind(m["agent_id"].as_str().unwrap_or(""))
                                .bind(m["role"].as_str().unwrap_or(""))
                                .bind(m["content"].as_str().unwrap_or(""))
                                .bind(m["seq"].as_i64().unwrap_or(0))
                                .bind(m["created_at"].as_i64().unwrap_or(0))
                                .execute(_pool).await;
                            }
                        }

                        // 更新同步水位
                        if synced_at > 0 {
                            let _ = sqlx::query(
                                "INSERT OR REPLACE INTO settings (key, value, updated_at) VALUES ('cloud_last_sync_at', ?, ?)"
                            )
                            .bind(synced_at.to_string())
                            .bind(synced_at)
                            .execute(_pool).await;
                        }

                        log::info!("Bridge: 同步完成，拉取 {} 条数据", pulled);
                    }
                }
                Err(e) => log::warn!("Bridge: 同步拉取失败: {}", e),
            }
        }

        // 心跳 ticker
        let heartbeat_interval = std::time::Duration::from_secs(config.heartbeat_secs);
        let mut heartbeat = tokio::time::interval(heartbeat_interval);
        heartbeat.tick().await; // 跳过第一次立即触发

        loop {
            tokio::select! {
                // 接收云端消息
                msg = read.next() => {
                    match msg {
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                                let msg_type = json["type"].as_str().unwrap_or("");
                                match msg_type {
                                    "registered" => {
                                        log::info!("Bridge: 注册确认 deviceId={}", json["deviceId"]);
                                    }
                                    "heartbeat_ack" => {
                                        // 心跳确认，忽略
                                    }
                                    "new_message" => {
                                        // 云端实时推送新消息（Telegram/Mobile 产生的）
                                        let data = &json["data"];
                                        let session_id = data["sessionId"].as_str().unwrap_or("");
                                        let role = data["role"].as_str().unwrap_or("");
                                        let content = data["content"].as_str().unwrap_or("");
                                        let sync_id = data["syncId"].as_str().unwrap_or("");
                                        let device_id = data["deviceId"].as_str().unwrap_or("cloud");

                                        if !session_id.is_empty() && !sync_id.is_empty() {
                                            let _ = sqlx::query(
                                                "INSERT OR IGNORE INTO chat_messages (id, session_id, agent_id, role, content, seq, created_at) VALUES (?, ?, (SELECT agent_id FROM chat_sessions WHERE id = ?), ?, ?, (SELECT COALESCE(MAX(seq),0)+1 FROM chat_messages WHERE session_id = ?), ?)"
                                            )
                                            .bind(sync_id).bind(session_id).bind(session_id)
                                            .bind(role).bind(content)
                                            .bind(session_id)
                                            .bind(data["createdAt"].as_i64().unwrap_or(chrono::Utc::now().timestamp_millis()))
                                            .execute(_pool).await;
                                            log::info!("Bridge: 实时同步消息 [{}] {} from {}", role, &content.chars().take(30).collect::<String>(), device_id);
                                        }
                                    }
                                    "forward_message" => {
                                        // 云端转发来的消息（需要本地处理）
                                        let data = &json["data"];
                                        let fwd = ForwardedMessage {
                                            request_id: data["requestId"].as_str().unwrap_or("").to_string(),
                                            agent_id: data["agentId"].as_str().unwrap_or("default").to_string(),
                                            session_id: data["sessionId"].as_str().unwrap_or("").to_string(),
                                            message: data["message"].as_str().unwrap_or("").to_string(),
                                            sender_channel: data["sender"]["channel"].as_str().unwrap_or("mobile").to_string(),
                                        };
                                        log::info!("Bridge: 收到转发消息 requestId={}", fwd.request_id);
                                        let _ = message_tx.send(fwd);
                                    }
                                    "ping" => {
                                        // 回复 pong
                                        let pong = serde_json::json!({"type": "pong"});
                                        if let Ok(pong_str) = serde_json::to_string(&pong) {
                                            let _ = write.send(tokio_tungstenite::tungstenite::Message::Text(pong_str)).await;
                                        }
                                    }
                                    "sync_ack" => {
                                        log::info!("Bridge: 同步确认 pushed={}", json["pushed"]);
                                    }
                                    _ => {
                                        log::debug!("Bridge: 未知消息类型 {}", msg_type);
                                    }
                                }
                            }
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => {
                            log::info!("Bridge: 收到关闭帧");
                            return Ok(());
                        }
                        Some(Err(e)) => {
                            return Err(format!("WebSocket 读取错误: {}", e));
                        }
                        None => {
                            return Err("WebSocket 连接已关闭".to_string());
                        }
                        _ => {} // Binary, Ping, Pong 等
                    }
                }

                // 心跳
                _ = heartbeat.tick() => {
                    let hb = serde_json::json!({
                        "type": "heartbeat",
                        "timestamp": chrono::Utc::now().timestamp_millis(),
                    });
                    match serde_json::to_string(&hb) {
                        Ok(hb_str) => {
                            if let Err(e) = write.send(tokio_tungstenite::tungstenite::Message::Text(hb_str)).await {
                                return Err(format!("心跳发送失败: {}", e));
                            }
                        }
                        Err(e) => log::warn!("Bridge: 心跳序列化失败: {}", e),
                    }
                }
            }
        }
    }
}
