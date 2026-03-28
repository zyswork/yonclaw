//! 数据库 Schema 定义和初始化

use sqlx::SqlitePool;

/// 初始化数据库 schema
pub async fn init_schema(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    // 创建对话历史表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            user_id TEXT NOT NULL,
            user_message TEXT NOT NULL,
            agent_response TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            metadata TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 创建 Agent 配置表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS agents (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            system_prompt TEXT NOT NULL,
            model TEXT NOT NULL,
            temperature REAL,
            max_tokens INTEGER,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            config TEXT,
            workspace_path TEXT
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 兼容旧数据库：尝试添加 workspace_path 列（如已存在则忽略）
    let _ = sqlx::query("ALTER TABLE agents ADD COLUMN workspace_path TEXT")
        .execute(pool)
        .await;

    // 兼容旧数据库：尝试添加 config_version 列
    let _ = sqlx::query("ALTER TABLE agents ADD COLUMN config_version INTEGER DEFAULT 1")
        .execute(pool)
        .await;

    // 兼容：添加 provider_id 列（指定模型对应的供应商，解决同名模型串供应商问题）
    let _ = sqlx::query("ALTER TABLE agents ADD COLUMN provider_id TEXT")
        .execute(pool)
        .await;

    // 创建记忆体表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS memories (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            memory_type TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 兼容旧数据库：给 memories 表添加 priority 列
    let _ = sqlx::query("ALTER TABLE memories ADD COLUMN priority INTEGER NOT NULL DEFAULT 1")
        .execute(pool)
        .await;

    // 创建向量数据表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS vectors (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            content TEXT NOT NULL,
            embedding BLOB NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 创建响应缓存表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS response_cache (
            cache_key TEXT PRIMARY KEY,
            model TEXT NOT NULL,
            response TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            last_used_at INTEGER NOT NULL,
            use_count INTEGER NOT NULL DEFAULT 1
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 创建记忆体 FTS5 全文搜索虚拟表（用于语义检索降级方案）
    sqlx::query(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
            content,
            agent_id UNINDEXED,
            memory_id UNINDEXED
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 创建索引以提高查询性能
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_conversations_agent_id ON conversations(agent_id)")
        .execute(pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_conversations_user_id ON conversations(user_id)")
        .execute(pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_agent_id ON memories(agent_id)")
        .execute(pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_vectors_agent_id ON vectors(agent_id)")
        .execute(pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_response_cache_last_used ON response_cache(last_used_at)")
        .execute(pool)
        .await?;

    // 创建已安装技能表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS installed_skills (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            name TEXT NOT NULL,
            version TEXT NOT NULL DEFAULT '0.0.0',
            manifest_json TEXT NOT NULL,
            source TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            installed_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(agent_id, name)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_installed_skills_agent_id ON installed_skills(agent_id)")
        .execute(pool)
        .await?;

    // 创建设置表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now') * 1000)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // 创建 MCP Server 配置表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS mcp_servers (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            name TEXT NOT NULL,
            transport TEXT NOT NULL,
            command TEXT,
            args TEXT,
            url TEXT,
            env TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            status TEXT NOT NULL DEFAULT 'configured',
            created_at INTEGER NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_mcp_servers_agent_id ON mcp_servers(agent_id)")
        .execute(pool)
        .await?;

    // 创建会话表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chat_sessions (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT 'New Session',
            created_at INTEGER NOT NULL,
            last_message_at INTEGER,
            summary TEXT,
            FOREIGN KEY (agent_id) REFERENCES agents(id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_chat_sessions_agent_id ON chat_sessions(agent_id)")
        .execute(pool)
        .await?;

    // 兼容旧数据库：conversations 表添加 session_id 列
    let _ = sqlx::query("ALTER TABLE conversations ADD COLUMN session_id TEXT")
        .execute(pool)
        .await;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_conversations_session_id ON conversations(session_id)")
        .execute(pool)
        .await?;

    // 定时任务表
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS cron_jobs (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            agent_id TEXT,
            job_type TEXT NOT NULL CHECK(job_type IN ('agent','shell','mcp_tool')),
            schedule_kind TEXT NOT NULL CHECK(schedule_kind IN ('cron','every','at','webhook','poll')),
            cron_expr TEXT,
            every_secs INTEGER,
            at_ts INTEGER,
            timezone TEXT NOT NULL DEFAULT 'Asia/Shanghai',
            action_payload TEXT NOT NULL,
            timeout_secs INTEGER NOT NULL DEFAULT 300,
            max_concurrent INTEGER NOT NULL DEFAULT 1,
            cooldown_secs INTEGER NOT NULL DEFAULT 0,
            max_daily_runs INTEGER,
            max_consecutive_failures INTEGER NOT NULL DEFAULT 5,
            retry_max INTEGER NOT NULL DEFAULT 0,
            retry_base_delay_ms INTEGER NOT NULL DEFAULT 2000,
            retry_backoff_factor REAL NOT NULL DEFAULT 2.0,
            misfire_policy TEXT NOT NULL DEFAULT 'catch_up' CHECK(misfire_policy IN ('skip','catch_up')),
            catch_up_limit INTEGER NOT NULL DEFAULT 3,
            enabled INTEGER NOT NULL DEFAULT 1,
            fail_streak INTEGER NOT NULL DEFAULT 0,
            runs_today INTEGER NOT NULL DEFAULT 0,
            runs_today_date TEXT,
            next_run_at INTEGER,
            last_run_at INTEGER,
            delete_after_run INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )"
    ).execute(pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS cron_runs (
            id TEXT PRIMARY KEY,
            job_id TEXT NOT NULL,
            scheduled_at INTEGER NOT NULL,
            started_at INTEGER,
            finished_at INTEGER,
            status TEXT NOT NULL CHECK(status IN ('queued','running','success','failed','timeout','cancelled')),
            trigger_source TEXT NOT NULL CHECK(trigger_source IN ('schedule','manual','retry','catch_up','heartbeat')),
            attempt INTEGER NOT NULL DEFAULT 1,
            output TEXT,
            error TEXT,
            FOREIGN KEY(job_id) REFERENCES cron_jobs(id) ON DELETE CASCADE
        )"
    ).execute(pool).await?;

    // 兼容：webhook/poll 扩展列
    let _ = sqlx::query("ALTER TABLE cron_jobs ADD COLUMN poll_last_hash TEXT")
        .execute(pool).await;
    let _ = sqlx::query("ALTER TABLE cron_jobs ADD COLUMN webhook_secret TEXT")
        .execute(pool).await;
    let _ = sqlx::query("ALTER TABLE cron_jobs ADD COLUMN poll_json_path TEXT")
        .execute(pool).await;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_cron_jobs_due ON cron_jobs(enabled, next_run_at)")
        .execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_cron_runs_job ON cron_runs(job_id, started_at DESC)")
        .execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_cron_runs_status ON cron_runs(status)")
        .execute(pool).await?;

    // 创建工具调用审计日志表
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tool_audit_log (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            session_id TEXT NOT NULL,
            tool_name TEXT NOT NULL,
            arguments TEXT NOT NULL,
            result TEXT,
            success INTEGER NOT NULL DEFAULT 1,
            policy_decision TEXT NOT NULL,
            policy_source TEXT NOT NULL,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL
        )"
    ).execute(pool).await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_audit_agent ON tool_audit_log(agent_id, created_at DESC)")
        .execute(pool).await?;

    // 创建 Agent 关系表
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS agent_relations (
            id TEXT PRIMARY KEY,
            from_id TEXT NOT NULL,
            to_id TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            metadata TEXT,
            created_at INTEGER NOT NULL
        )"
    ).execute(pool).await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_relations_from ON agent_relations(from_id)")
        .execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_relations_to ON agent_relations(to_id)")
        .execute(pool).await?;

    // 创建 Token 使用统计表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS token_usage (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            session_id TEXT,
            model TEXT NOT NULL,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            total_tokens INTEGER NOT NULL DEFAULT 0,
            cached_tokens INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_token_usage_agent ON token_usage(agent_id, created_at)")
        .execute(pool)
        .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_token_usage_date ON token_usage(created_at)")
        .execute(pool)
        .await?;

    // 创建嵌入缓存表（避免重复调用嵌入 API）
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS embedding_cache (
            content_hash TEXT PRIMARY KEY,
            embedding BLOB NOT NULL,
            model TEXT NOT NULL,
            accessed_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_embedding_cache_accessed ON embedding_cache(accessed_at)")
        .execute(pool)
        .await?;

    // 创建结构化聊天消息表（存储完整消息序列，含工具调用上下文）
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chat_messages (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT,
            tool_calls_json TEXT,
            tool_call_id TEXT,
            tool_name TEXT,
            seq INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY (session_id) REFERENCES chat_sessions(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_chat_messages_session ON chat_messages(session_id, seq)")
        .execute(pool)
        .await?;

    // 多 Agent 路由绑定表
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS agent_bindings (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            channel TEXT NOT NULL,
            sender_id TEXT,
            agent_id TEXT NOT NULL,
            priority INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s','now') * 1000),
            UNIQUE(channel, sender_id)
        )
        "#,
    ).execute(pool).await?;

    // 频道连接表（每个 Agent 可以绑定自己的 Telegram bot / 飞书应用等）
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS agent_channels (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            channel_type TEXT NOT NULL,
            credentials_json TEXT NOT NULL,
            display_name TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            status TEXT NOT NULL DEFAULT 'configured',
            status_message TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (agent_id) REFERENCES agents(id) ON DELETE CASCADE
        )
        "#,
    ).execute(pool).await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_agent_channels_agent ON agent_channels(agent_id)")
        .execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_agent_channels_type ON agent_channels(channel_type, enabled)")
        .execute(pool).await?;

    // 迁移：把旧的全局频道 token（settings 表）转到 agent_channels 表
    {
        let existing_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agent_channels")
            .fetch_one(pool).await.unwrap_or((0,));
        if existing_count.0 == 0 {
            // 找第一个 agent 作为迁移目标
            let first_agent: Option<(String,)> = sqlx::query_as(
                "SELECT id FROM agents ORDER BY created_at ASC LIMIT 1"
            ).fetch_optional(pool).await.unwrap_or(None);

            if let Some((agent_id,)) = first_agent {
                let now = chrono::Utc::now().timestamp();
                // Telegram
                let tg_token: Option<(String,)> = sqlx::query_as(
                    "SELECT value FROM settings WHERE key = 'telegram_bot_token'"
                ).fetch_optional(pool).await.unwrap_or(None);
                if let Some((token,)) = tg_token {
                    if !token.is_empty() {
                        let id = uuid::Uuid::new_v4().to_string();
                        let creds = serde_json::json!({"bot_token": token}).to_string();
                        let _ = sqlx::query(
                            "INSERT OR IGNORE INTO agent_channels (id, agent_id, channel_type, credentials_json, enabled, status, created_at, updated_at) VALUES (?, ?, 'telegram', ?, 1, 'configured', ?, ?)"
                        ).bind(&id).bind(&agent_id).bind(&creds).bind(now).bind(now).execute(pool).await;
                        log::info!("迁移 Telegram 频道到 agent_channels: agent={}", agent_id);
                    }
                }
                // 飞书
                let fs_id: Option<(String,)> = sqlx::query_as(
                    "SELECT value FROM settings WHERE key = 'feishu_app_id'"
                ).fetch_optional(pool).await.unwrap_or(None);
                let fs_secret: Option<(String,)> = sqlx::query_as(
                    "SELECT value FROM settings WHERE key = 'feishu_app_secret'"
                ).fetch_optional(pool).await.unwrap_or(None);
                if let (Some((app_id,)), Some((app_secret,))) = (fs_id, fs_secret) {
                    if !app_id.is_empty() {
                        let id = uuid::Uuid::new_v4().to_string();
                        let creds = serde_json::json!({"app_id": app_id, "app_secret": app_secret}).to_string();
                        let _ = sqlx::query(
                            "INSERT OR IGNORE INTO agent_channels (id, agent_id, channel_type, credentials_json, enabled, status, created_at, updated_at) VALUES (?, ?, 'feishu', ?, 1, 'configured', ?, ?)"
                        ).bind(&id).bind(&agent_id).bind(&creds).bind(now).bind(now).execute(pool).await;
                        log::info!("迁移飞书频道到 agent_channels: agent={}", agent_id);
                    }
                }
                // Discord
                let dc_token: Option<(String,)> = sqlx::query_as(
                    "SELECT value FROM settings WHERE key = 'discord_bot_token'"
                ).fetch_optional(pool).await.unwrap_or(None);
                if let Some((token,)) = dc_token {
                    if !token.is_empty() {
                        let id = uuid::Uuid::new_v4().to_string();
                        let creds = serde_json::json!({"bot_token": token}).to_string();
                        let _ = sqlx::query(
                            "INSERT OR IGNORE INTO agent_channels (id, agent_id, channel_type, credentials_json, enabled, status, created_at, updated_at) VALUES (?, ?, 'discord', ?, 1, 'configured', ?, ?)"
                        ).bind(&id).bind(&agent_id).bind(&creds).bind(now).bind(now).execute(pool).await;
                        log::info!("迁移 Discord 频道到 agent_channels: agent={}", agent_id);
                    }
                }
                // Slack
                let sl_bot: Option<(String,)> = sqlx::query_as(
                    "SELECT value FROM settings WHERE key = 'slack_bot_token'"
                ).fetch_optional(pool).await.unwrap_or(None);
                let sl_app: Option<(String,)> = sqlx::query_as(
                    "SELECT value FROM settings WHERE key = 'slack_app_token'"
                ).fetch_optional(pool).await.unwrap_or(None);
                if let (Some((bot_token,)), Some((app_token,))) = (sl_bot, sl_app) {
                    if !bot_token.is_empty() {
                        let id = uuid::Uuid::new_v4().to_string();
                        let creds = serde_json::json!({"bot_token": bot_token, "app_token": app_token}).to_string();
                        let _ = sqlx::query(
                            "INSERT OR IGNORE INTO agent_channels (id, agent_id, channel_type, credentials_json, enabled, status, created_at, updated_at) VALUES (?, ?, 'slack', ?, 1, 'configured', ?, ?)"
                        ).bind(&id).bind(&agent_id).bind(&creds).bind(now).bind(now).execute(pool).await;
                        log::info!("迁移 Slack 频道到 agent_channels: agent={}", agent_id);
                    }
                }
                // 微信
                let wx_token: Option<(String,)> = sqlx::query_as(
                    "SELECT value FROM settings WHERE key = 'weixin_bot_token'"
                ).fetch_optional(pool).await.unwrap_or(None);
                if let Some((token,)) = wx_token {
                    if !token.is_empty() {
                        let id = uuid::Uuid::new_v4().to_string();
                        let creds = serde_json::json!({"bot_token": token}).to_string();
                        let _ = sqlx::query(
                            "INSERT OR IGNORE INTO agent_channels (id, agent_id, channel_type, credentials_json, enabled, status, created_at, updated_at) VALUES (?, ?, 'weixin', ?, 1, 'configured', ?, ?)"
                        ).bind(&id).bind(&agent_id).bind(&creds).bind(now).bind(now).execute(pool).await;
                        log::info!("迁移微信频道到 agent_channels: agent={}", agent_id);
                    }
                }
            }
        }
    }

    // 插件全局配置
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS plugin_configs (
            plugin_id TEXT PRIMARY KEY,
            config_json TEXT NOT NULL DEFAULT '{}',
            enabled INTEGER NOT NULL DEFAULT 1,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now') * 1000)
        )
        "#,
    ).execute(pool).await?;

    // Agent 级别的插件启用/配置
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS agent_plugins (
            agent_id TEXT NOT NULL,
            plugin_id TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            config_override TEXT DEFAULT NULL,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now') * 1000),
            PRIMARY KEY (agent_id, plugin_id)
        )
        "#,
    ).execute(pool).await?;

    // 子代理执行记录表（持久化 delegate_task 结果）
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS subagent_runs (
            id TEXT PRIMARY KEY,
            parent_agent_id TEXT NOT NULL,
            parent_session_id TEXT,
            task_index INTEGER NOT NULL DEFAULT 0,
            goal TEXT NOT NULL,
            context TEXT,
            model TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('running','success','failed','timeout','cancelled')),
            result TEXT,
            error TEXT,
            depth INTEGER NOT NULL DEFAULT 0,
            allowed_tools TEXT,
            duration_ms INTEGER,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            finished_at INTEGER
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_subagent_runs_parent ON subagent_runs(parent_agent_id, created_at DESC)")
        .execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_subagent_runs_session ON subagent_runs(parent_session_id, created_at DESC)")
        .execute(pool).await?;

    // Agent Plaza（社交 feed）
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS plaza_posts (
            id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            agent_name TEXT NOT NULL,
            content TEXT NOT NULL,
            post_type TEXT NOT NULL DEFAULT 'discovery',
            likes INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL
        )
        "#,
    ).execute(pool).await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS plaza_comments (
            id TEXT PRIMARY KEY,
            post_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            agent_name TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY (post_id) REFERENCES plaza_posts(id) ON DELETE CASCADE
        )
        "#,
    ).execute(pool).await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_plaza_posts_time ON plaza_posts(created_at DESC)")
        .execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_plaza_comments_post ON plaza_comments(post_id, created_at)")
        .execute(pool).await?;

    log::info!("数据库 schema 初始化完成");

    Ok(())
}
