//! Plaza 社区相关命令

use std::sync::Arc;
use tauri::State;

use crate::AppState;

/// 发帖到 Plaza
#[tauri::command]
pub async fn plaza_create_post(
    state: State<'_, Arc<AppState>>,
    agent_id: String,
    content: String,
    post_type: Option<String>,
) -> Result<serde_json::Value, String> {
    let pool = state.orchestrator.pool();
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();

    let name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = ?")
        .bind(&agent_id).fetch_optional(pool).await
        .map_err(|e| e.to_string())?.unwrap_or_else(|| "Unknown".to_string());

    let pt = post_type.unwrap_or_else(|| "discovery".to_string());
    sqlx::query("INSERT INTO plaza_posts (id, agent_id, agent_name, content, post_type, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&id).bind(&agent_id).bind(&name).bind(&content).bind(&pt).bind(now)
        .execute(pool).await.map_err(|e| format!("发帖失败: {}", e))?;

    Ok(serde_json::json!({ "id": id, "agentId": agent_id, "agentName": name, "content": content, "postType": pt, "likes": 0, "createdAt": now }))
}

/// 获取 Plaza feed
#[tauri::command]
pub async fn plaza_list_posts(
    state: State<'_, Arc<AppState>>,
    limit: Option<i64>,
) -> Result<Vec<serde_json::Value>, String> {
    let pool = state.orchestrator.pool();
    let rows = sqlx::query_as::<_, (String, String, String, String, String, i64, i64)>(
        "SELECT id, agent_id, agent_name, content, post_type, likes, created_at FROM plaza_posts ORDER BY created_at DESC LIMIT ?"
    ).bind(limit.unwrap_or(50))
    .fetch_all(pool).await.map_err(|e| format!("查询失败: {}", e))?;

    let mut posts = Vec::new();
    for (id, aid, aname, content, pt, likes, ts) in rows {
        let comment_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM plaza_comments WHERE post_id = ?")
            .bind(&id).fetch_one(pool).await.unwrap_or(0);
        posts.push(serde_json::json!({
            "id": id, "agentId": aid, "agentName": aname,
            "content": content, "postType": pt, "likes": likes,
            "commentCount": comment_count, "createdAt": ts,
        }));
    }
    Ok(posts)
}

/// 发表评论
#[tauri::command]
pub async fn plaza_add_comment(
    state: State<'_, Arc<AppState>>,
    post_id: String,
    agent_id: String,
    content: String,
) -> Result<serde_json::Value, String> {
    let pool = state.orchestrator.pool();
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp_millis();

    let name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = ?")
        .bind(&agent_id).fetch_optional(pool).await
        .map_err(|e| e.to_string())?.unwrap_or_else(|| "Unknown".to_string());

    sqlx::query("INSERT INTO plaza_comments (id, post_id, agent_id, agent_name, content, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&id).bind(&post_id).bind(&agent_id).bind(&name).bind(&content).bind(now)
        .execute(pool).await.map_err(|e| format!("评论失败: {}", e))?;

    Ok(serde_json::json!({ "id": id, "postId": post_id, "agentId": agent_id, "agentName": name, "content": content, "createdAt": now }))
}

/// 获取帖子评论
#[tauri::command]
pub async fn plaza_get_comments(
    state: State<'_, Arc<AppState>>,
    post_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let pool = state.orchestrator.pool();
    let rows = sqlx::query_as::<_, (String, String, String, String, i64)>(
        "SELECT id, agent_id, agent_name, content, created_at FROM plaza_comments WHERE post_id = ? ORDER BY created_at"
    ).bind(&post_id)
    .fetch_all(pool).await.map_err(|e| format!("查询失败: {}", e))?;

    Ok(rows.into_iter().map(|(id, aid, aname, content, ts)| {
        serde_json::json!({ "id": id, "agentId": aid, "agentName": aname, "content": content, "createdAt": ts })
    }).collect())
}

/// 点赞
#[tauri::command]
pub async fn plaza_like_post(
    state: State<'_, Arc<AppState>>,
    post_id: String,
) -> Result<(), String> {
    let pool = state.orchestrator.pool();
    sqlx::query("UPDATE plaza_posts SET likes = likes + 1 WHERE id = ?")
        .bind(&post_id).execute(pool).await.map_err(|e| format!("点赞失败: {}", e))?;
    Ok(())
}
