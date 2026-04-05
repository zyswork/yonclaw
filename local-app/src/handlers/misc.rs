//! 杂项命令 — 通知、备份、诊断、SOP、运行时、Token 统计等

use std::sync::Arc;
use tauri::State;

use crate::agent;
use crate::memory;
use crate::runtime;
use crate::sop;
use crate::AppState;
use super::helpers::{load_providers, find_provider_for_model};

/// 保存聊天中粘贴的图片到磁盘
#[tauri::command]
pub async fn save_chat_image(
    agent_id: String,
    base64_data: String,
) -> Result<String, String> {
    let image_dir = dirs::home_dir()
        .ok_or("无法获取 home 目录")?
        .join(".xianzhu/images")
        .join(&agent_id);
    let _ = std::fs::create_dir_all(&image_dir);

    let filename = format!("img_{}.jpg", chrono::Utc::now().timestamp_millis());
    let path = image_dir.join(&filename);

    fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
        let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut lookup = [255u8; 256];
        for (i, &b) in table.iter().enumerate() { lookup[b as usize] = i as u8; }
        let input = input.trim_end_matches('=');
        let mut out = Vec::with_capacity(input.len() * 3 / 4);
        let bytes = input.as_bytes();
        for chunk in bytes.chunks(4) {
            let mut buf = [0u8; 4];
            for (i, &b) in chunk.iter().enumerate() {
                let v = lookup[b as usize];
                if v == 255 { return Err(format!("无效 base64 字符: {}", b as char)); }
                buf[i] = v;
            }
            out.push((buf[0] << 2) | (buf[1] >> 4));
            if chunk.len() > 2 { out.push((buf[1] << 4) | (buf[2] >> 2)); }
            if chunk.len() > 3 { out.push((buf[2] << 6) | buf[3]); }
        }
        Ok(out)
    }

    let bytes = decode_base64(&base64_data)?;

    std::fs::write(&path, &bytes)
        .map_err(|e| format!("保存图片失败: {}", e))?;

    log::info!("聊天图片已保存: {} ({} 字节)", path.display(), bytes.len());
    Ok(path.to_string_lossy().to_string())
}

/// 发送系统原生通知
#[tauri::command]
pub fn send_notification(app: tauri::AppHandle, title: String, body: String) -> Result<(), String> {
    use tauri::api::notification::Notification;
    Notification::new(&app.config().tauri.bundle.identifier)
        .title(&title)
        .body(&body)
        .show()
        .map_err(|e| format!("通知发送失败: {}", e))
}

/// 备份数据库到文件
#[tauri::command]
pub async fn backup_database(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let db_path = dirs::data_dir()
        .unwrap_or_default()
        .join("com.xianzhu.app/xianzhu.db");

    if !db_path.exists() {
        return Err("数据库文件不存在".into());
    }

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let backup_dir = dirs::data_dir()
        .unwrap_or_default()
        .join("com.xianzhu.app/backups");
    let _ = std::fs::create_dir_all(&backup_dir);

    let backup_path = backup_dir.join(format!("xianzhu-{}.db", timestamp));

    sqlx::query(&format!("VACUUM INTO '{}'", backup_path.display()))
        .execute(state.db.pool()).await
        .map_err(|e| format!("备份失败: {}", e))?;

    let size = std::fs::metadata(&backup_path).map(|m| m.len()).unwrap_or(0);
    log::info!("数据库已备份: {} ({} bytes)", backup_path.display(), size);

    Ok(serde_json::json!({
        "path": backup_path.display().to_string(),
        "size_bytes": size,
        "timestamp": timestamp.to_string(),
    }).to_string())
}

/// 恢复数据库（从备份文件）
#[tauri::command]
pub async fn restore_database(backup_path: String) -> Result<String, String> {
    let src = std::path::Path::new(&backup_path);
    if !src.exists() {
        return Err(format!("备份文件不存在: {}", backup_path));
    }

    let db_path = dirs::data_dir()
        .unwrap_or_default()
        .join("com.xianzhu.app/xianzhu.db");

    let pre_restore = db_path.with_extension("db.pre-restore");
    if db_path.exists() {
        std::fs::copy(&db_path, &pre_restore)
            .map_err(|e| format!("备份当前数据库失败: {}", e))?;
    }

    std::fs::copy(src, &db_path)
        .map_err(|e| format!("恢复失败: {}", e))?;

    log::info!("数据库已恢复: {} -> {}", backup_path, db_path.display());
    Ok("数据库已恢复，请重启应用生效。".into())
}

/// Token 费用估算
#[tauri::command]
pub async fn estimate_token_cost(
    state: State<'_, Arc<AppState>>,
    agent_id: Option<String>,
    days: Option<i64>,
) -> Result<serde_json::Value, String> {
    let days = days.unwrap_or(7);
    let since = chrono::Utc::now().timestamp_millis() - (days * 86_400_000);

    let query = if let Some(ref aid) = agent_id {
        sqlx::query_as::<_, (String, i64, i64, i64)>(
            "SELECT model, SUM(input_tokens), SUM(output_tokens), COUNT(*) FROM token_usage WHERE agent_id = ? AND created_at >= ? GROUP BY model"
        ).bind(aid).bind(since).fetch_all(state.db.pool()).await
    } else {
        sqlx::query_as::<_, (String, i64, i64, i64)>(
            "SELECT model, SUM(input_tokens), SUM(output_tokens), COUNT(*) FROM token_usage WHERE created_at >= ? GROUP BY model"
        ).bind(since).fetch_all(state.db.pool()).await
    }.map_err(|e| e.to_string())?;

    // 费率表（每 1M token 的美元价格，input/output）
    let price_per_million = |model: &str| -> (f64, f64) {
        match model {
            m if m.contains("gpt-5") && m.contains("mini") => (0.30, 1.20),
            m if m.contains("gpt-5") => (5.0, 20.0),
            m if m.contains("gpt-4o-mini") || m.contains("gpt-4.1-mini") => (0.15, 0.60),
            m if m.contains("gpt-4o") || m.contains("gpt-4.1") => (2.50, 10.0),
            m if m.contains("gpt-4.5") => (10.0, 30.0),
            m if m.contains("o4-mini") || m.contains("o3-mini") => (1.10, 4.40),
            m if m.contains("o3") || m.contains("o4") => (10.0, 40.0),
            m if m.contains("claude-opus-4") => (15.0, 75.0),
            m if m.contains("claude-sonnet-4") => (3.0, 15.0),
            m if m.contains("claude-haiku-4") => (0.80, 4.0),
            m if m.contains("claude-3") && m.contains("opus") => (15.0, 75.0),
            m if m.contains("claude-3") && m.contains("sonnet") => (3.0, 15.0),
            m if m.contains("claude") && m.contains("haiku") => (0.25, 1.25),
            m if m.contains("claude") => (3.0, 15.0),
            m if m.contains("gemini") && m.contains("flash") => (0.075, 0.30),
            m if m.contains("gemini") && m.contains("pro") => (1.25, 5.0),
            m if m.contains("gemini") => (0.50, 1.50),
            m if m.contains("deepseek-r1") => (0.55, 2.19),
            m if m.contains("deepseek") => (0.27, 1.10),
            m if m.contains("grok-3-mini") => (0.30, 0.50),
            m if m.contains("grok") => (3.0, 15.0),
            m if m.contains("qwen") && m.contains("turbo") => (0.30, 0.60),
            m if m.contains("qwen") => (0.80, 2.0),
            m if m.contains("moonshot") || m.contains("kimi") => (1.0, 1.0),
            m if m.contains("glm") && m.contains("flash") => (0.10, 0.10),
            m if m.contains("glm") => (1.0, 1.0),
            m if m.contains("minimax") || m.contains("abab") => (1.0, 1.0),
            m if m.contains("mistral") && m.contains("large") => (2.0, 6.0),
            m if m.contains("mistral") => (0.25, 0.25),
            m if m.contains("llama") => (0.20, 0.20),
            _ => (1.0, 3.0),
        }
    };

    let mut total_cost = 0.0f64;
    let models: Vec<serde_json::Value> = query.iter().map(|(model, inp, out, calls)| {
        let (in_price, out_price) = price_per_million(model);
        let cost = (*inp as f64 / 1_000_000.0) * in_price + (*out as f64 / 1_000_000.0) * out_price;
        total_cost += cost;
        serde_json::json!({
            "model": model,
            "input_tokens": inp,
            "output_tokens": out,
            "calls": calls,
            "estimated_cost_usd": format!("{:.4}", cost),
        })
    }).collect();

    Ok(serde_json::json!({
        "days": days,
        "agent_id": agent_id,
        "models": models,
        "total_estimated_cost_usd": format!("{:.4}", total_cost),
    }))
}

/// 列出已注册的 Hooks
#[tauri::command]
pub fn list_hooks() -> Vec<serde_json::Value> {
    let hook_points = [
        ("BeforeInbound", "用户消息进入前", "可修改/拒绝消息"),
        ("BeforeOutbound", "回复发出前", "可修改回复内容"),
        ("BeforePromptBuild", "System prompt 构建前", "可注入额外上下文"),
        ("BeforeLlmCall", "LLM 调用前", "可修改 messages/tools"),
        ("AfterLlmCall", "LLM 调用后", "可观察 response"),
        ("BeforeToolCall", "工具执行前", "可修改参数/拒绝"),
        ("AfterToolCall", "工具执行后", "可观察结果"),
        ("SessionStart", "会话开始", "初始化"),
        ("SessionEnd", "会话结束", "清理"),
        ("BeforeCompaction", "上下文压缩前", ""),
        ("AfterCompaction", "上下文压缩后", ""),
        ("SubagentSpawned", "子代理派发", ""),
        ("SubagentCompleted", "子代理完成", ""),
    ];

    hook_points.iter().map(|(point, desc, note)| {
        serde_json::json!({
            "point": point,
            "description": desc,
            "note": note,
            "builtinHandlers": ["logging"]
        })
    }).collect()
}

/// SOP: 列出所有工作流
#[tauri::command]
pub fn sop_list() -> Vec<serde_json::Value> {
    let mut engine = sop::SopEngine::new();
    let sop_dir = dirs::home_dir().unwrap_or_default().join(".xianzhu/sops");
    let _ = engine.load_from_dir(&sop_dir);
    engine.list().iter().map(|s| serde_json::json!({
        "name": s.name, "description": s.description,
        "priority": format!("{:?}", s.priority),
        "mode": format!("{:?}", s.execution_mode),
        "steps": s.steps.len(),
        "triggers": s.triggers.len(),
    })).collect()
}

/// SOP: 触发执行
#[tauri::command]
pub fn sop_trigger(name: String) -> Result<serde_json::Value, String> {
    let mut engine = sop::SopEngine::new();
    let sop_dir = dirs::home_dir().unwrap_or_default().join(".xianzhu/sops");
    let _ = engine.load_from_dir(&sop_dir);
    let run = engine.trigger(&name)?;
    Ok(serde_json::json!({
        "runId": run.run_id, "sopName": run.sop_name,
        "status": format!("{:?}", run.status),
        "totalSteps": run.total_steps,
    }))
}

/// SOP: 查看运行历史
#[tauri::command]
pub fn sop_runs() -> Vec<serde_json::Value> {
    let engine = sop::SopEngine::new();
    engine.all_runs().iter().map(|r| serde_json::json!({
        "runId": r.run_id, "sopName": r.sop_name,
        "status": format!("{:?}", r.status),
        "currentStep": r.current_step, "totalSteps": r.total_steps,
    })).collect()
}

/// Doctor 诊断：运行全部检查
#[tauri::command]
pub async fn run_doctor(state: State<'_, Arc<AppState>>) -> Result<Vec<agent::doctor::DiagnosticResult>, String> {
    Ok(agent::doctor::run_diagnostics(state.db.pool()).await)
}

/// Doctor 自动修复
#[tauri::command]
pub async fn doctor_auto_fix(state: State<'_, Arc<AppState>>) -> Result<Vec<agent::doctor::DiagnosticResult>, String> {
    Ok(agent::doctor::auto_fix(state.db.pool()).await)
}

/// 检测系统安装的浏览器
#[tauri::command]
pub fn detect_browsers() -> Vec<agent::browser::DetectedBrowser> {
    agent::browser::detect_browsers()
}

/// 用浏览器打开 URL
#[tauri::command]
pub fn open_in_browser(url: String, browser: Option<String>) -> Result<String, String> {
    agent::browser::open_url(&url, browser.as_deref())?;
    Ok(format!("已打开: {}", url))
}

/// 检查 Node.js 运行时状态
#[tauri::command]
pub async fn check_runtime() -> Result<serde_json::Value, String> {
    let rt = runtime::NodeRuntime::new();
    let status = rt.status().await;
    serde_json::to_value(&status).map_err(|e| e.to_string())
}

/// 安装 Node.js 运行时（自动下载）
#[tauri::command]
pub async fn setup_runtime() -> Result<serde_json::Value, String> {
    let rt = runtime::NodeRuntime::new();
    rt.ensure_installed().await?;
    let status = rt.status().await;
    serde_json::to_value(&status).map_err(|e| e.to_string())
}

/// 健康检查
#[tauri::command]
pub async fn health_check(
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    let pool = state.db.pool();

    let db_ok = sqlx::query("SELECT 1").execute(pool).await.is_ok();
    let agent_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agents")
        .fetch_one(pool).await.unwrap_or(0);
    let memory_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memories")
        .fetch_one(pool).await.unwrap_or(0);

    let today_start = chrono::Local::now().date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().timestamp_millis())
        .unwrap_or(0);
    let today_tokens: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total_tokens), 0) FROM token_usage WHERE created_at >= ?"
    ).bind(today_start).fetch_one(pool).await.unwrap_or(0);

    let cache_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM response_cache")
        .fetch_one(pool).await.unwrap_or(0);

    Ok(serde_json::json!({
        "status": if db_ok { "healthy" } else { "degraded" },
        "db": db_ok,
        "agents": agent_count,
        "memories": memory_count,
        "today_tokens": today_tokens,
        "response_cache_entries": cache_count,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

/// Token 使用统计 — 按天/模型聚合
#[tauri::command]
pub async fn get_token_stats(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    days: Option<i64>,
) -> Result<serde_json::Value, String> {
    let pool = state.db.pool();
    let days = days.unwrap_or(7);
    let since = chrono::Utc::now().timestamp_millis() - (days * 86_400_000);

    let rows = sqlx::query_as::<_, (String, i64, i64, i64)>(
        r#"
        SELECT model,
               SUM(input_tokens) as total_input,
               SUM(output_tokens) as total_output,
               COUNT(*) as call_count
        FROM token_usage
        WHERE agent_id = ? AND created_at >= ?
        GROUP BY model
        ORDER BY total_input + total_output DESC
        "#
    )
    .bind(&agent_id).bind(since)
    .fetch_all(pool).await
    .map_err(|e| format!("查询 token 统计失败: {}", e))?;

    let models: Vec<serde_json::Value> = rows.iter().map(|(model, input, output, count)| {
        serde_json::json!({
            "model": model,
            "input_tokens": input,
            "output_tokens": output,
            "total_tokens": input + output,
            "calls": count,
        })
    }).collect();

    let total_input: i64 = rows.iter().map(|(_, i, _, _)| i).sum();
    let total_output: i64 = rows.iter().map(|(_, _, o, _)| o).sum();

    Ok(serde_json::json!({
        "agent_id": agent_id,
        "days": days,
        "total_input_tokens": total_input,
        "total_output_tokens": total_output,
        "total_tokens": total_input + total_output,
        "models": models,
    }))
}

/// Token 使用日统计（最近 N 天，每天一条）
#[tauri::command]
pub async fn get_token_daily_stats(
    state: State<'_, Arc<AppState>>,
    agent_id: Option<String>,
    days: Option<i64>,
) -> Result<Vec<serde_json::Value>, String> {
    let pool = state.db.pool();
    let days = days.unwrap_or(30);
    let since = chrono::Utc::now().timestamp_millis() - (days * 86_400_000);

    let rows = if let Some(ref aid) = agent_id {
        sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
            r#"
            SELECT DATE(created_at / 1000, 'unixepoch', 'localtime') as day,
                   SUM(input_tokens), SUM(output_tokens), SUM(total_tokens), COUNT(*)
            FROM token_usage
            WHERE agent_id = ? AND created_at >= ?
            GROUP BY day ORDER BY day
            "#
        ).bind(aid).bind(since).fetch_all(pool).await
    } else {
        sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
            r#"
            SELECT DATE(created_at / 1000, 'unixepoch', 'localtime') as day,
                   SUM(input_tokens), SUM(output_tokens), SUM(total_tokens), COUNT(*)
            FROM token_usage
            WHERE created_at >= ?
            GROUP BY day ORDER BY day
            "#
        ).bind(since).fetch_all(pool).await
    }.map_err(|e| format!("查询日统计失败: {}", e))?;

    Ok(rows.iter().map(|(day, input, output, total, calls)| {
        serde_json::json!({
            "date": day, "inputTokens": input, "outputTokens": output,
            "totalTokens": total, "calls": calls,
        })
    }).collect())
}

/// Memory Hygiene — 手动触发清理
#[tauri::command]
pub async fn run_memory_hygiene(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<String, String> {
    let agent = state.orchestrator.get_agent_cached(&agent_id).await?;
    let wp = agent.workspace_path.as_deref();
    state.orchestrator.run_memory_hygiene(&agent_id, wp).await
}

/// 响应缓存统计
#[tauri::command]
pub async fn get_cache_stats(
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    let pool = state.db.pool();

    let resp_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM response_cache")
        .fetch_one(pool).await.unwrap_or(0);
    let resp_hits: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(use_count), 0) FROM response_cache")
        .fetch_one(pool).await.unwrap_or(0);

    let emb_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM embedding_cache")
        .fetch_one(pool).await.unwrap_or(0);

    Ok(serde_json::json!({
        "response_cache": {
            "entries": resp_count,
            "total_hits": resp_hits,
        },
        "embedding_cache": {
            "entries": emb_count,
        },
    }))
}

/// 读取设置
#[tauri::command]
pub async fn get_setting(
    state: State<'_, Arc<AppState>>,
    key: String,
) -> Result<Option<String>, String> {
    let pool = state.db.pool();
    let val: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(&key)
        .fetch_optional(pool).await
        .map_err(|e| format!("读取设置失败: {}", e))?;
    Ok(val)
}

/// 写入设置
#[tauri::command]
pub async fn set_setting(
    state: State<'_, Arc<AppState>>,
    key: String,
    value: String,
) -> Result<(), String> {
    let pool = state.db.pool();
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"
    )
    .bind(&key).bind(&value).bind(now)
    .execute(pool).await
    .map_err(|e| format!("写入设置失败: {}", e))?;
    Ok(())
}

/// 批量读取设置（前缀匹配）
#[tauri::command]
pub async fn get_settings_by_prefix(
    state: State<'_, Arc<AppState>>,
    prefix: String,
) -> Result<serde_json::Value, String> {
    let pool = state.db.pool();
    let pattern = format!("{}%", prefix);
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT key, value FROM settings WHERE key LIKE ?"
    )
    .bind(&pattern)
    .fetch_all(pool).await
    .map_err(|e| format!("查询设置失败: {}", e))?;

    let mut map = serde_json::Map::new();
    for (k, v) in rows {
        map.insert(k, serde_json::json!(v));
    }
    Ok(serde_json::Value::Object(map))
}

/// Memory Snapshot — 导出所有记忆
#[tauri::command]
pub async fn export_memory_snapshot(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<String, String> {
    let agent = state.orchestrator.get_agent_cached(&agent_id).await?;
    let wp = agent.workspace_path.as_deref().ok_or("Agent 没有工作区路径")?;
    let snapshot_dir = std::path::PathBuf::from(wp).join("memory");
    let _ = std::fs::create_dir_all(&snapshot_dir);
    let path = snapshot_dir.join(format!("snapshot-{}.md", chrono::Local::now().format("%Y%m%d-%H%M%S")));
    let count = memory::snapshot_memories(state.db.pool(), &agent_id, &path).await?;
    Ok(format!("已导出 {} 条记忆到 {}", count, path.display()))
}

/// 从历史对话中提取记忆
#[tauri::command]
pub async fn extract_memories_from_history(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<serde_json::Value, String> {
    let pool = state.orchestrator.pool();

    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT user_message, agent_response FROM conversations WHERE agent_id = ? ORDER BY created_at DESC LIMIT 100"
    )
    .bind(&agent_id)
    .fetch_all(pool).await
    .map_err(|e| format!("查询对话失败: {}", e))?;

    if rows.is_empty() {
        return Ok(serde_json::json!({"extracted": 0, "message": "没有可分析的对话历史"}));
    }

    let mut conversation_text = String::new();
    let max_chars = 8000;
    for (user_msg, agent_resp) in rows.iter().rev() {
        let entry = format!("用户: {}\n助手: {}\n\n", user_msg, agent_resp);
        if conversation_text.len() + entry.len() > max_chars {
            break;
        }
        conversation_text.push_str(&entry);
    }

    let providers = load_providers(&state.db).await?;
    let agent_model: String = sqlx::query_scalar("SELECT model FROM agents WHERE id = ?")
        .bind(&agent_id)
        .fetch_one(pool).await
        .map_err(|e| format!("查询 Agent 失败: {}", e))?;
    let (api_type, api_key, base_url) = find_provider_for_model(&providers, &agent_model)
        .ok_or("没有可用的 LLM 供应商，请先在设置中配置")?;
    let model = agent_model;

    let extraction_prompt = format!(
        r#"分析以下对话历史，提取值得长期记住的关键信息。

请以 JSON 数组格式返回，每条记忆包含：
- "type": 类型（core=用户核心信息, episodic=事件记忆, semantic=知识信息, procedural=操作流程）
- "content": 记忆内容（简洁、具体）
- "priority": 优先级 1-10

只提取重要信息，不要提取琐碎的对话内容。最多提取 10 条。

对话历史：
{}

请直接返回 JSON 数组，不要有其他文字："#,
        conversation_text
    );

    let llm_config = agent::llm::LlmConfig {
        provider: api_type.clone(),
        model: model.clone(),
        api_key: api_key.clone(),
        base_url: if base_url.is_empty() { None } else { Some(base_url.clone()) },
        temperature: Some(0.3),
        max_tokens: Some(2000),
        thinking_level: None,
    };
    let llm_client = agent::llm::LlmClient::new(llm_config);
    let messages = vec![
        agent::llm::OpenAiMessage { role: "user".to_string(), content: extraction_prompt },
    ];

    let response = llm_client.call_openai(messages, 0.3, 2000).await
        .map_err(|e| format!("LLM 调用失败: {}", e))?;

    let json_str = if let Some(start) = response.find('[') {
        if let Some(end) = response.rfind(']') {
            &response[start..=end]
        } else {
            &response
        }
    } else {
        &response
    };

    let items: Vec<serde_json::Value> = serde_json::from_str(json_str)
        .map_err(|e| format!("解析 LLM 返回的 JSON 失败: {}。原始响应: {}", e, &response[..response.len().min(200)]))?;

    use memory::Memory;
    let mem = if let Some(emb_config) = memory::SqliteMemory::try_load_embedding_config(pool).await {
        memory::SqliteMemory::with_embedding(pool.clone(), emb_config).await
    } else {
        memory::SqliteMemory::new(pool.clone())
    };

    let mut extracted = 0;
    for item in &items {
        let mem_type = item["type"].as_str().unwrap_or("semantic");
        let content = item["content"].as_str().unwrap_or("");
        let priority = item["priority"].as_i64().unwrap_or(5) as i32;

        if content.is_empty() { continue; }

        let category = memory::MemoryCategory::from_str(mem_type);
        let mem_priority = memory::MemoryPriority::from_i32(priority.min(3));
        let key = format!("extracted-{}-{}", mem_type, chrono::Utc::now().timestamp_millis());

        match mem.store_with_priority(&agent_id, &key, content, category, mem_priority).await {
            Ok(_) => { extracted += 1; }
            Err(e) => { log::warn!("写入提取记忆失败: {}", e); }
        }
    }

    log::info!("从对话历史提取了 {} 条记忆（共分析 {} 轮对话）", extracted, rows.len());

    Ok(serde_json::json!({
        "extracted": extracted,
        "analyzed": rows.len(),
        "message": format!("从 {} 轮对话中提取了 {} 条记忆", rows.len(), extracted),
    }))
}

/// 手动触发 Learner 从最近会话中提取经验教训
///
/// 与 extract_memories_from_history 不同：
/// - extract_memories_from_history: 提取通用记忆（core/episodic/semantic/procedural）
/// - run_learner: 提取可复用经验（tool_pattern/user_preference/code_convention/fix_pattern/project_knowledge）
#[tauri::command]
pub async fn run_learner(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
) -> Result<serde_json::Value, String> {
    let pool = state.orchestrator.pool();

    // 获取最近的会话
    let sessions: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM chat_sessions WHERE agent_id = ? ORDER BY created_at DESC LIMIT 5"
    )
    .bind(&agent_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询会话失败: {}", e))?;

    if sessions.is_empty() {
        return Ok(serde_json::json!({"extracted": 0, "message": "没有可分析的会话"}));
    }

    // 构建 LLM 配置
    let providers = load_providers(&state.db).await?;
    let agent_model: String = sqlx::query_scalar("SELECT model FROM agents WHERE id = ?")
        .bind(&agent_id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("查询 Agent 失败: {}", e))?;
    let (api_type, api_key, base_url) = find_provider_for_model(&providers, &agent_model)
        .ok_or("没有可用的 LLM 供应商，请先在设置中配置")?;

    let llm_config = agent::llm::LlmConfig {
        provider: api_type,
        model: agent_model,
        api_key,
        base_url: if base_url.is_empty() { None } else { Some(base_url) },
        temperature: Some(0.2),
        max_tokens: Some(500),
        thinking_level: None,
    };

    // 获取 workspace 路径
    let workspace_path: Option<String> = sqlx::query_scalar(
        "SELECT workspace_path FROM agents WHERE id = ?"
    ).bind(&agent_id).fetch_optional(pool).await.ok().flatten();

    let mut total_extracted = 0;
    let mut total_skipped = 0;

    for (session_id,) in &sessions {
        let outcome = agent::learner::extract_lessons_with_llm(pool, &agent_id, session_id, &llm_config).await;
        if !outcome.lessons.is_empty() {
            let count = outcome.lessons.len();
            agent::learner::persist_lessons(pool, &agent_id, workspace_path.as_deref(), &outcome.lessons).await;
            total_extracted += count;
        } else {
            total_skipped += 1;
        }
    }

    log::info!("手动 Learner: 从 {} 个会话中提取了 {} 条经验（跳过 {}）",
        sessions.len(), total_extracted, total_skipped);

    Ok(serde_json::json!({
        "extracted": total_extracted,
        "sessions": sessions.len(),
        "skipped": total_skipped,
        "message": format!("从 {} 个会话中提取了 {} 条经验教训", sessions.len(), total_extracted),
    }))
}

/// 云端 API 代理
#[tauri::command]
pub async fn cloud_api_proxy(
    state: State<'_, Arc<AppState>>,
    method: String,
    path: String,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let pool = state.db.pool();

    let gateway_url: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'cloud_gateway_url'")
        .fetch_optional(pool).await.map_err(|e| e.to_string())?
        .unwrap_or_default();

    if gateway_url.is_empty() {
        return Err("未配置云端连接（cloud_gateway_url）".to_string());
    }

    let base_url = gateway_url.trim()
        .replace("ws://", "http://").replace("wss://", "https://")
        .replace("/ws/bridge", "");

    let url = format!("{}{}", base_url, path);
    let client = reqwest::Client::new();

    let req = match method.to_uppercase().as_str() {
        "POST" => client.post(&url).json(&body),
        "PUT" => client.put(&url).json(&body),
        "DELETE" => client.delete(&url),
        _ => client.get(&url),
    };

    let resp = req.send().await.map_err(|e| format!("请求失败: {}", e))?;
    let status = resp.status();

    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, text.chars().take(200).collect::<String>()));
    }

    resp.json::<serde_json::Value>().await.map_err(|e| format!("解析响应失败: {}", e))
}

/// 审计日志 — 记录敏感操作
pub async fn write_audit_log(pool: &sqlx::SqlitePool, action: &str, category: &str, detail: &str) {
    let _ = sqlx::query("INSERT INTO audit_log (action, category, detail) VALUES (?, ?, ?)")
        .bind(action).bind(category).bind(detail)
        .execute(pool).await;
}

/// 读取本地文件为 base64（用于前端播放音频/显示图片等）
#[tauri::command]
pub async fn read_file_base64(path: String) -> Result<String, String> {
    let bytes = tokio::fs::read(&path).await
        .map_err(|e| format!("读取文件失败: {}", e))?;
    Ok(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes))
}

/// Python 沙箱状态查询
#[tauri::command]
pub async fn get_python_sandbox_status() -> Result<serde_json::Value, String> {
    let initialized = crate::agent::python_sandbox::is_initialized();
    let initializing = crate::agent::python_sandbox::is_initializing();
    let python_path = crate::agent::python_sandbox::python_path();

    Ok(serde_json::json!({
        "initialized": initialized,
        "initializing": initializing,
        "pythonPath": python_path.to_string_lossy(),
        "status": if initialized { "ready" } else if initializing { "initializing" } else { "not_started" }
    }))
}

/// 导出应用数据（数据库 + Agent 工作区 → 单个 .zip 文件）
///
/// 安全措施：
/// - 导出前 checkpoint WAL（确保所有数据落盘）
/// - API Key 脱敏（providers 表的 api_key 字段替换为占位符）
#[tauri::command]
pub async fn export_app_data(
    state: State<'_, std::sync::Arc<crate::AppState>>,
    output_path: String,
) -> Result<String, String> {
    use std::io::Write;

    let config = crate::config::AppConfig::default();
    let db_path = config.data_dir.join("xianzhu.db");
    let agents_dir = &config.agents_dir;

    // 1. Checkpoint WAL — 确保所有数据从 WAL 刷入主数据库文件
    let pool = state.db.pool();
    let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(pool).await;
    log::info!("导出: WAL checkpoint 完成");

    // 2. 复制数据库到临时文件（避免导出期间被修改）
    let tmp_db = config.data_dir.join("xianzhu_export_tmp.db");
    std::fs::copy(&db_path, &tmp_db)
        .map_err(|e| format!("复制数据库失败: {}", e))?;

    // 3. 脱敏：清除临时数据库中的 API Key
    {
        let tmp_pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&format!("sqlite:{}", tmp_db.to_string_lossy()))
            .await
            .map_err(|e| format!("打开临时数据库失败: {}", e))?;
        // 清除 providers 表中的 api_key（保留其他配置）
        let _ = sqlx::query("UPDATE settings SET value = '[已脱敏]' WHERE key LIKE '%api_key%' OR key LIKE '%secret%' OR key LIKE '%token%'")
            .execute(&tmp_pool).await;
        // 清除 providers JSON 中的敏感字段
        let _ = sqlx::query("UPDATE settings SET value = '[已脱敏]' WHERE key LIKE 'provider_%' AND value LIKE '%api_key%'")
            .execute(&tmp_pool).await;
        let _ = sqlx::query("UPDATE settings SET value = '' WHERE key LIKE 'auth.%'")
            .execute(&tmp_pool).await;
        tmp_pool.close().await;
        log::info!("导出: API Key 已脱敏");
    }

    let file = std::fs::File::create(&output_path)
        .map_err(|e| format!("创建导出文件失败: {}", e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // 4. 写入脱敏后的数据库
    let db_bytes = std::fs::read(&tmp_db)
        .map_err(|e| format!("读取数据库失败: {}", e))?;
    zip.start_file("xianzhu.db", options).map_err(|e| e.to_string())?;
    zip.write_all(&db_bytes).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&tmp_db); // 清理临时文件
    log::info!("导出数据库: {:.1}MB", db_bytes.len() as f64 / 1024.0 / 1024.0);

    // 5. 导出 Agent 工作区
    if agents_dir.exists() {
        for entry in walkdir::WalkDir::new(agents_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            if let Ok(meta) = std::fs::metadata(path) {
                if meta.len() > 10 * 1024 * 1024 { continue; }
            }
            let relative = path.strip_prefix(agents_dir.parent().unwrap_or(agents_dir))
                .unwrap_or(path);
            let zip_path = format!("agents/{}", relative.to_string_lossy().replace('\\', "/"));
            if let Ok(content) = std::fs::read(path) {
                let _ = zip.start_file(&zip_path, options);
                let _ = zip.write_all(&content);
            }
        }
    }

    // 6. 导出 profile
    let profile_dir = dirs::home_dir().unwrap_or_default().join(".xianzhu").join("profile");
    if profile_dir.exists() {
        for entry in std::fs::read_dir(&profile_dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Ok(content) = std::fs::read(&path) {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    let _ = zip.start_file(format!("profile/{}", name), options);
                    let _ = zip.write_all(&content);
                }
            }
        }
    }

    zip.finish().map_err(|e| e.to_string())?;

    let size = std::fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0);
    write_audit_log(pool, "data_export", "security", &format!("导出 {:.1}MB", size as f64 / 1024.0 / 1024.0)).await;
    Ok(format!("数据已导出（{:.1}MB）。注意：API Key 已脱敏，导入后需重新配置。", size as f64 / 1024.0 / 1024.0))
}

/// 导入应用数据（从 .zip 恢复数据库 + Agent 工作区）
///
/// 安全措施：
/// - 数据库写入到临时文件，不直接覆盖正在使用的 db
/// - Path traversal 防护（拒绝含 .. 的路径）
/// - 自动备份旧数据库
#[tauri::command]
pub async fn import_app_data(
    zip_path: String,
) -> Result<String, String> {
    let config = crate::config::AppConfig::default();
    let data_dir = &config.data_dir;
    let agents_parent = config.agents_dir.parent()
        .unwrap_or(&config.agents_dir).to_path_buf();

    let file = std::fs::File::open(&zip_path)
        .map_err(|e| format!("打开导入文件失败: {}", e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("解析 zip 失败: {}", e))?;

    let mut imported_count = 0;
    let mut db_imported = false;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();

        // Path traversal 防护
        if name.contains("..") {
            log::warn!("导入: 跳过可疑路径 {}", name);
            continue;
        }

        if name == "xianzhu.db" {
            // 导入数据库 → 写到临时文件（不直接覆盖正在使用的 db）
            let import_db = data_dir.join("xianzhu_import.db");
            let mut content = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut content).map_err(|e| e.to_string())?;
            std::fs::write(&import_db, &content)
                .map_err(|e| format!("写入临时数据库失败: {}", e))?;

            // 备份旧数据库
            let db_path = data_dir.join("xianzhu.db");
            if db_path.exists() {
                let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
                let backup = data_dir.join(format!("xianzhu_{}.db.backup", ts));
                let _ = std::fs::copy(&db_path, &backup);
                log::info!("旧数据库已备份: {}", backup.display());
            }

            // 重命名临时文件为正式文件（原子操作，重启后生效）
            let _ = std::fs::rename(&import_db, data_dir.join("xianzhu_pending_import.db"));
            db_imported = true;
            log::info!("导入数据库: {:.1}MB（重启后生效）", content.len() as f64 / 1024.0 / 1024.0);
            imported_count += 1;
        } else if name.starts_with("agents/") || name.starts_with("profile/") {
            let target = agents_parent.join(&name);
            // 二次 path traversal 检查
            if !target.starts_with(&agents_parent) {
                log::warn!("导入: 路径越界 {}", name);
                continue;
            }
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let mut content = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut content).map_err(|e| e.to_string())?;
            std::fs::write(&target, &content).map_err(|e| format!("写入失败: {}", e))?;
            imported_count += 1;
        }
    }

    let msg = if db_imported {
        format!("导入完成（{} 个文件）。请关闭并重新打开应用以加载新数据。", imported_count)
    } else {
        format!("导入完成（{} 个文件）。Agent 工作区已更新。", imported_count)
    };
    Ok(msg)
}
