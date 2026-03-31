//! 频道管理器 — 管理每个 Agent 的频道连接实例
//!
//! 每个 Agent 可以有自己的 Telegram bot、飞书应用等。
//! ChannelManager 负责启动、停止、重启这些连接。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use sqlx::SqlitePool;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::agent::orchestrator::Orchestrator;

/// 单个频道实例的运行状态
struct ChannelInstance {
    id: String,
    #[allow(dead_code)]
    agent_id: String,
    channel_type: String,
    handle: JoinHandle<()>,
    cancel: CancellationToken,
}

/// 频道管理器
pub struct ChannelManager {
    pool: SqlitePool,
    orchestrator: Arc<Orchestrator>,
    app_handle: tauri::AppHandle,
    instances: Mutex<HashMap<String, ChannelInstance>>,
}

impl ChannelManager {
    pub fn new(pool: SqlitePool, orchestrator: Arc<Orchestrator>, app_handle: tauri::AppHandle) -> Self {
        Self {
            pool,
            orchestrator,
            app_handle,
            instances: Mutex::new(HashMap::new()),
        }
    }

    /// 启动所有已启用的频道连接
    pub async fn start_all(&self) {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT id, agent_id, channel_type, credentials_json FROM agent_channels WHERE enabled = 1"
        ).fetch_all(&self.pool).await.unwrap_or_else(|e| {
            log::warn!("频道列表加载失败: {}", e);
            vec![]
        });

        for (id, agent_id, channel_type, creds_json) in rows {
            if let Err(e) = self.start_instance(&id, &agent_id, &channel_type, &creds_json).await {
                log::error!("启动频道失败: {} ({}/{}): {}", id, channel_type, agent_id, e);
                // 更新状态为 error
                if let Err(db_err) = sqlx::query("UPDATE agent_channels SET status = 'error', status_message = ? WHERE id = ?")
                    .bind(&e).bind(&id).execute(&self.pool).await {
                    log::warn!("更新频道状态失败 (id={}): {}", id, db_err);
                }
            }
        }
    }

    /// 启动单个频道实例
    pub async fn start_instance(&self, id: &str, agent_id: &str, channel_type: &str, creds_json: &str) -> Result<(), String> {
        // 如果已经在运行，先停止
        self.stop_instance(id).await;

        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let pool = self.pool.clone();
        let orch = self.orchestrator.clone();
        let app = self.app_handle.clone();
        let id_owned = id.to_string();
        let agent_id_owned = agent_id.to_string();
        let channel_type_owned = channel_type.to_string();
        let creds: serde_json::Value = serde_json::from_str(creds_json).map_err(|e| format!("JSON 解析失败: {}", e))?;

        let handle = match channel_type {
            "telegram" => {
                let token = creds["bot_token"].as_str().ok_or("缺少 bot_token")?.to_string();
                let config = crate::channels::telegram::TelegramConfig {
                    bot_token: token,
                    agent_id: agent_id_owned.clone(),
                };
                tokio::spawn(async move {
                    if let Err(e) = crate::channels::telegram::start_polling(config, pool.clone(), orch, app, cancel_clone).await {
                        log::error!("Telegram 实例 {} 退出: {}", id_owned, e);
                    }
                    if let Err(e) = sqlx::query("UPDATE agent_channels SET status = 'stopped' WHERE id = ?")
                        .bind(&id_owned).execute(&pool).await {
                        log::warn!("更新频道状态失败 (id={}): {}", id_owned, e);
                    }
                })
            }
            "feishu" => {
                let app_id = creds["app_id"].as_str().ok_or("缺少 app_id")?.to_string();
                let app_secret = creds["app_secret"].as_str().ok_or("缺少 app_secret")?.to_string();
                let config = crate::channels::feishu::FeishuConfig {
                    app_id,
                    app_secret,
                    agent_id: agent_id_owned.clone(),
                };
                tokio::spawn(async move {
                    if let Err(e) = crate::channels::feishu::start_feishu(config, pool.clone(), orch, app, cancel_clone).await {
                        log::error!("Feishu 实例 {} 退出: {}", id_owned, e);
                    }
                    if let Err(e) = sqlx::query("UPDATE agent_channels SET status = 'stopped' WHERE id = ?")
                        .bind(&id_owned).execute(&pool).await {
                        log::warn!("更新频道状态失败 (id={}): {}", id_owned, e);
                    }
                })
            }
            "discord" => {
                let token = creds["bot_token"].as_str().ok_or("缺少 bot_token")?.to_string();
                let config = crate::channels::discord::DiscordConfig {
                    bot_token: token,
                    agent_id: agent_id_owned.clone(),
                };
                tokio::spawn(async move {
                    if let Err(e) = crate::channels::discord::start_discord(config, pool.clone(), orch, app, cancel_clone).await {
                        log::error!("Discord 实例 {} 退出: {}", id_owned, e);
                    }
                    if let Err(e) = sqlx::query("UPDATE agent_channels SET status = 'stopped' WHERE id = ?")
                        .bind(&id_owned).execute(&pool).await {
                        log::warn!("更新频道状态失败 (id={}): {}", id_owned, e);
                    }
                })
            }
            "slack" => {
                let bot_token = creds["bot_token"].as_str().ok_or("缺少 bot_token")?.to_string();
                let app_token = creds["app_token"].as_str().ok_or("缺少 app_token")?.to_string();
                let config = crate::channels::slack::SlackConfig {
                    bot_token,
                    app_token,
                    agent_id: agent_id_owned.clone(),
                };
                tokio::spawn(async move {
                    if let Err(e) = crate::channels::slack::start_slack(config, pool.clone(), orch, app, cancel_clone).await {
                        log::error!("Slack 实例 {} 退出: {}", id_owned, e);
                    }
                    if let Err(e) = sqlx::query("UPDATE agent_channels SET status = 'stopped' WHERE id = ?")
                        .bind(&id_owned).execute(&pool).await {
                        log::warn!("更新频道状态失败 (id={}): {}", id_owned, e);
                    }
                })
            }
            "weixin" => {
                let token = creds["bot_token"].as_str().ok_or("缺少 bot_token")?.to_string();
                let config = crate::channels::weixin::WeixinConfig {
                    bot_token: token,
                    agent_id: agent_id_owned.clone(),
                };
                tokio::spawn(async move {
                    if let Err(e) = crate::channels::weixin::start_weixin(config, pool.clone(), orch, app, cancel_clone).await {
                        log::error!("WeChat 实例 {} 退出: {}", id_owned, e);
                    }
                    if let Err(e) = sqlx::query("UPDATE agent_channels SET status = 'stopped' WHERE id = ?")
                        .bind(&id_owned).execute(&pool).await {
                        log::warn!("更新频道状态失败 (id={}): {}", id_owned, e);
                    }
                })
            }
            "wecom" => {
                let corp_id = creds["corp_id"].as_str().ok_or("缺少 corp_id")?.to_string();
                let secret = creds["secret"].as_str().ok_or("缺少 secret")?.to_string();
                let token = creds["token"].as_str().ok_or("缺少 token")?.to_string();
                let encoding_aes_key = creds["encoding_aes_key"].as_str().ok_or("缺少 encoding_aes_key")?.to_string();
                let agent_id_wecom = creds["agent_id_wecom"].as_i64().ok_or("缺少 agent_id_wecom")?;
                let callback_port = creds["callback_port"].as_u64().unwrap_or(9876) as u16;
                let config = crate::channels::wecom::WeComConfig {
                    corp_id,
                    agent_id_wecom,
                    secret,
                    token,
                    encoding_aes_key,
                    agent_id: agent_id_owned.clone(),
                    callback_port,
                };
                tokio::spawn(async move {
                    if let Err(e) = crate::channels::wecom::start_wecom(config, pool.clone(), orch, app, cancel_clone).await {
                        log::error!("WeCom 实例 {} 退出: {}", id_owned, e);
                    }
                    if let Err(e) = sqlx::query("UPDATE agent_channels SET status = 'stopped' WHERE id = ?")
                        .bind(&id_owned).execute(&pool).await {
                        log::warn!("更新频道状态失败 (id={}): {}", id_owned, e);
                    }
                })
            }
            "dingtalk" => {
                let app_key = creds["app_key"].as_str().ok_or("缺少 app_key")?.to_string();
                let app_secret = creds["app_secret"].as_str().ok_or("缺少 app_secret")?.to_string();
                let config = crate::channels::dingtalk::DingTalkConfig {
                    app_key,
                    app_secret,
                    agent_id: agent_id_owned.clone(),
                };
                tokio::spawn(async move {
                    if let Err(e) = crate::channels::dingtalk::start_dingtalk(config, pool.clone(), orch, app, cancel_clone).await {
                        log::error!("DingTalk 实例 {} 退出: {}", id_owned, e);
                    }
                    if let Err(e) = sqlx::query("UPDATE agent_channels SET status = 'stopped' WHERE id = ?")
                        .bind(&id_owned).execute(&pool).await {
                        log::warn!("更新频道状态失败 (id={}): {}", id_owned, e);
                    }
                })
            }
            _ => return Err(format!("不支持的频道类型: {}", channel_type)),
        };

        // 更新状态
        if let Err(e) = sqlx::query("UPDATE agent_channels SET status = 'running', status_message = NULL WHERE id = ?")
            .bind(id).execute(&self.pool).await {
            log::warn!("更新频道状态为 running 失败 (id={}): {}", id, e);
        }

        let mut instances = self.instances.lock().unwrap_or_else(|p| p.into_inner());
        instances.insert(id.to_string(), ChannelInstance {
            id: id.to_string(),
            agent_id: agent_id.to_string(),
            channel_type: channel_type_owned,
            handle,
            cancel,
        });

        log::info!("频道实例已启动: {} ({}/agent={})", id, channel_type, agent_id);
        Ok(())
    }

    /// 停止单个实例
    pub async fn stop_instance(&self, id: &str) {
        let instance = {
            let mut instances = self.instances.lock().unwrap_or_else(|p| p.into_inner());
            instances.remove(id)
        };
        if let Some(inst) = instance {
            inst.cancel.cancel();
            // 给 3 秒优雅关闭
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), inst.handle).await;
            log::info!("频道实例已停止: {} ({})", id, inst.channel_type);
        }
        if let Err(e) = sqlx::query("UPDATE agent_channels SET status = 'stopped' WHERE id = ?")
            .bind(id).execute(&self.pool).await {
            log::warn!("更新频道状态为 stopped 失败 (id={}): {}", id, e);
        }
    }

    /// 停止所有频道实例（用于应用退出清理）
    pub fn stop_all(&self) {
        let mut instances = self.instances.lock().unwrap_or_else(|p| p.into_inner());
        for (id, inst) in instances.drain() {
            inst.cancel.cancel();
            log::info!("频道实例已停止: {} ({})", id, inst.channel_type);
        }
    }

    /// 获取所有实例状态
    pub fn running_count(&self) -> usize {
        self.instances.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// 启动定时健康检查
    ///
    /// 每 30 秒检查所有已启用频道的连接状态：
    /// - 如果实例的 JoinHandle 已结束（task 退出），标记为断开并触发重连
    /// - 通过 Tauri event 通知前端渠道状态变化
    pub fn start_health_check(self: &Arc<Self>) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;

                // 收集已结束的实例 ID
                let finished_ids: Vec<(String, String, String)> = {
                    let instances = manager.instances.lock().unwrap_or_else(|p| p.into_inner());
                    instances.iter()
                        .filter(|(_, inst)| inst.handle.is_finished())
                        .map(|(id, inst)| (id.clone(), inst.channel_type.clone(), inst.agent_id.clone()))
                        .collect()
                };

                if !finished_ids.is_empty() {
                    log::warn!("健康检查: 发现 {} 个已断开的频道实例，尝试重连", finished_ids.len());
                }

                for (id, channel_type, _agent_id) in &finished_ids {
                    // 从实例列表中移除已结束的
                    {
                        let mut instances = manager.instances.lock().unwrap_or_else(|p| p.into_inner());
                        instances.remove(id.as_str());
                    }

                    // 通知前端状态变化
                    use tauri::Manager;
                    let _ = manager.app_handle.emit_all("channel-status", serde_json::json!({
                        "id": id,
                        "channelType": channel_type,
                        "status": "disconnected",
                        "message": "健康检查检测到连接断开，正在重连...",
                    }));

                    // 从数据库读取凭据并重启
                    let row: Option<(String, String, String)> = sqlx::query_as(
                        "SELECT agent_id, channel_type, credentials_json FROM agent_channels WHERE id = ? AND enabled = 1"
                    ).fetch_optional(&manager.pool).await.unwrap_or_else(|e| {
                        log::warn!("频道凭据查询失败: {}", e);
                        None
                    });

                    if let Some((agent_id, ch_type, creds_json)) = row {
                        log::info!("健康检查: 重启频道 {} ({})", id, ch_type);
                        if let Err(e) = manager.start_instance(id, &agent_id, &ch_type, &creds_json).await {
                            log::error!("健康检查: 重启频道 {} 失败: {}", id, e);
                            if let Err(db_err) = sqlx::query("UPDATE agent_channels SET status = 'error', status_message = ? WHERE id = ?")
                                .bind(&format!("健康检查重启失败: {}", e))
                                .bind(id)
                                .execute(&manager.pool).await {
                                log::warn!("更新频道状态失败 (id={}): {}", id, db_err);
                            }

                            // 通知前端重启失败
                            let _ = manager.app_handle.emit_all("channel-status", serde_json::json!({
                                "id": id,
                                "channelType": channel_type,
                                "status": "error",
                                "message": format!("重启失败: {}", e),
                            }));
                        } else {
                            // 通知前端重连成功
                            let _ = manager.app_handle.emit_all("channel-status", serde_json::json!({
                                "id": id,
                                "channelType": channel_type,
                                "status": "running",
                                "message": "健康检查重连成功",
                            }));
                        }
                    } else {
                        log::info!("健康检查: 频道 {} 已禁用或不存在，跳过重连", id);
                    }
                }
            }
        });
    }
}
