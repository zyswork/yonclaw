//! Learner 系统 v2 — LLM 驱动的经验提取
//!
//! 核心改进（按 harness engineering 原则）：
//! - LLM 提取替代关键词匹配（准确度飞跃）
//! - 结构化记忆 schema（fact/source/confidence/verified）
//! - 矛盾检测（新记忆 vs 已有记忆）
//! - 使用反馈循环（记忆是否被引用 → boost/decay）

use sqlx::SqlitePool;

/// 经验类型
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum LessonCategory {
    ToolPattern,
    UserPreference,
    CodeConvention,
    FixPattern,
    ProjectKnowledge,
}

impl LessonCategory {
    pub fn as_str(&self) -> &str {
        match self {
            Self::ToolPattern => "tool_pattern",
            Self::UserPreference => "user_preference",
            Self::CodeConvention => "code_convention",
            Self::FixPattern => "fix_pattern",
            Self::ProjectKnowledge => "project_knowledge",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "user_preference" => Self::UserPreference,
            "code_convention" => Self::CodeConvention,
            "fix_pattern" => Self::FixPattern,
            "project_knowledge" => Self::ProjectKnowledge,
            _ => Self::ToolPattern,
        }
    }
}

/// 结构化经验
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Lesson {
    pub category: LessonCategory,
    pub content: String,
    pub confidence: f64,
}

/// 学习结果
#[derive(Debug, Clone)]
pub struct LearningOutcome {
    pub lessons: Vec<Lesson>,
    pub skipped_reason: Option<String>,
}

/// 用 LLM 从对话中提取经验
///
/// 比关键词匹配准确得多，成本 ~500 token（用 mini 模型）
pub async fn extract_lessons_with_llm(
    pool: &SqlitePool,
    agent_id: &str,
    session_id: &str,
    llm_config: &super::llm::LlmConfig,
) -> LearningOutcome {
    // 质量门控
    let messages: Vec<(String, String)> = match sqlx::query_as(
        "SELECT role, COALESCE(content, '') FROM chat_messages WHERE session_id = ? ORDER BY seq DESC LIMIT 20"
    ).bind(session_id).fetch_all(pool).await {
        Ok(msgs) => msgs,
        Err(_) => return LearningOutcome { lessons: vec![], skipped_reason: Some("加载消息失败".into()) },
    };

    let user_count = messages.iter().filter(|(r, _)| r == "user").count();
    if user_count < 2 {
        return LearningOutcome { lessons: vec![], skipped_reason: Some("对话轮次不足".into()) };
    }

    // 跳过系统会话
    let title: Option<String> = sqlx::query_scalar("SELECT title FROM chat_sessions WHERE id = ?")
        .bind(session_id).fetch_optional(pool).await.ok().flatten();
    if let Some(ref t) = title {
        if t.starts_with("[subagent]") || t.starts_with("[heartbeat]") || t.starts_with("[a2a]") {
            return LearningOutcome { lessons: vec![], skipped_reason: Some("系统会话".into()) };
        }
    }

    // 构建对话摘要（限制 token）
    let mut conversation = String::new();
    for (role, content) in messages.iter().rev().take(15) {
        let preview: String = content.chars().take(150).collect();
        conversation.push_str(&format!("{}: {}\n", role, preview));
    }

    // LLM 提取
    let prompt = format!(
        r#"分析以下对话，提取 0-3 条可复用的经验教训。

每条经验必须是具体的、可复用的事实或规则，不要泛泛而谈。

返回 JSON 数组，每个元素格式：
{{"category": "tool_pattern|user_preference|code_convention|fix_pattern|project_knowledge", "fact": "具体经验", "confidence": 0.5-1.0}}

category 说明：
- tool_pattern: 工具使用模式（如"编辑 Rust 文件后应跑 cargo check"）
- user_preference: 用户偏好（如"用户偏好中文注释"）
- code_convention: 代码规范（如"该项目使用 snake_case 命名"）
- fix_pattern: 错误修复经验（如"此 API 的 timeout 需要设为 30s"）
- project_knowledge: 项目知识（如"数据库 schema 在 db/schema.rs"）

如果没有值得记录的经验，返回空数组 []。

对话内容：
{}"#,
        conversation
    );

    // 直接使用传入的 llm_config（已由 orchestrator 通过 build_compact_llm_config 构建好）
    let client = super::llm::LlmClient::new(llm_config.clone());

    let msgs = vec![serde_json::json!({"role": "user", "content": prompt})];
    // _rx 必须持有直到 call_stream 完成，否则 tx.is_closed() 会立即取消流
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let resp = match client.call_stream(&msgs, None, None, tx).await {
        Ok(r) => r.content,
        Err(e) => {
            log::warn!("Learner LLM 调用失败: {}", e);
            return LearningOutcome { lessons: vec![], skipped_reason: Some(format!("LLM 失败: {}", e)) };
        }
    };

    // 解析 JSON 响应
    let lessons = parse_llm_lessons(&resp, pool, agent_id).await;

    if lessons.is_empty() {
        LearningOutcome { lessons: vec![], skipped_reason: Some("LLM 未提取到经验".into()) }
    } else {
        LearningOutcome { lessons, skipped_reason: None }
    }
}

/// 解析 LLM 返回的经验 JSON，并执行矛盾检测 + 去重
async fn parse_llm_lessons(response: &str, pool: &SqlitePool, agent_id: &str) -> Vec<Lesson> {
    // 提取 JSON 数组（LLM 可能返回 markdown 包裹的 JSON）
    let json_str = response.trim()
        .trim_start_matches("```json").trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let items: Vec<serde_json::Value> = match serde_json::from_str(json_str) {
        Ok(arr) => arr,
        Err(_) => {
            // 尝试从文本中提取 JSON 数组
            if let Some(start) = json_str.find('[') {
                if let Some(end) = json_str.rfind(']') {
                    serde_json::from_str(&json_str[start..=end]).unwrap_or_default()
                } else { vec![] }
            } else { vec![] }
        }
    };

    let mut lessons = Vec::new();
    for item in items.iter().take(3) {
        let category = item["category"].as_str().unwrap_or("tool_pattern");
        let fact = item["fact"].as_str().unwrap_or("");
        let confidence = item["confidence"].as_f64().unwrap_or(0.6);

        if fact.is_empty() || fact.len() < 10 { continue; }

        // 矛盾检测：搜索已有相似记忆
        let existing: Vec<String> = sqlx::query_scalar(
            "SELECT content FROM memories WHERE agent_id = ? AND content LIKE ? LIMIT 5"
        )
        .bind(agent_id)
        .bind(format!("%{}%", &fact[..fact.len().min(30)]))
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        // 如果已有高度相似记忆，跳过（去重）
        let is_dup = existing.iter().any(|e| {
            let overlap = fact.chars().take(50).collect::<String>();
            e.contains(&overlap)
        });
        if is_dup {
            log::debug!("Learner: 去重跳过: {}", &fact[..fact.len().min(50)]);
            continue;
        }

        lessons.push(Lesson {
            category: LessonCategory::from_str(category),
            content: fact.to_string(),
            confidence,
        });
    }

    lessons
}

/// 持久化经验到 DB + MEMORY.md
pub async fn persist_lessons(
    pool: &SqlitePool,
    agent_id: &str,
    workspace_path: Option<&str>,
    lessons: &[Lesson],
) {
    for lesson in lessons {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let priority = if lesson.confidence > 0.7 { 2 } else { 1 } as i64;

        let _ = sqlx::query(
            "INSERT INTO memories (id, agent_id, memory_type, key, content, priority, category, created_at, access_count) VALUES (?, ?, 'learned', ?, ?, ?, ?, ?, 0)"
        )
        .bind(&id).bind(agent_id).bind(lesson.category.as_str())
        .bind(&lesson.content).bind(priority).bind(lesson.category.as_str()).bind(now)
        .execute(pool).await;

        log::info!("Learner: [{}] {} (conf={:.1})", lesson.category.as_str(), &lesson.content[..lesson.content.len().min(60)], lesson.confidence);
    }

    // 追加到 MEMORY.md
    if let Some(wp) = workspace_path {
        let path = std::path::Path::new(wp).join("MEMORY.md");
        let mut content = std::fs::read_to_string(&path).unwrap_or_default();
        if !content.contains("## Learned") {
            content.push_str("\n\n## Learned\n\n");
        }
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        for l in lessons {
            content.push_str(&format!("- [{}] [{}] {}\n", date, l.category.as_str(), l.content));
        }
        let _ = std::fs::write(&path, &content);
    }
}

// ─── 记忆使用反馈循环 ───────────────────────────────────────

/// 检查 Agent 回复是否引用了注入的记忆（关键词匹配）
///
/// 返回 (引用的 memory IDs, 未引用的 memory IDs)
pub fn check_memory_usage(
    response: &str,
    injected_memories: &[(String, String)], // (memory_id, content_snippet)
) -> (Vec<String>, Vec<String>) {
    let resp_lower = response.to_lowercase();
    let mut used = Vec::new();
    let mut unused = Vec::new();

    for (id, snippet) in injected_memories {
        // 取记忆的前 30 个字作为关键词
        let keywords: Vec<String> = snippet.split_whitespace()
            .filter(|w| w.len() > 2)
            .take(5)
            .map(|w| w.to_lowercase())
            .collect();

        // 至少 2 个关键词出现在回复中 → 视为引用
        let matched = keywords.iter().filter(|k| resp_lower.contains(k.as_str())).count();
        if matched >= 2 || (keywords.len() <= 2 && matched >= 1) {
            used.push(id.clone());
        } else {
            unused.push(id.clone());
        }
    }

    (used, unused)
}

/// 更新记忆的使用反馈（boost 或 decay）
pub async fn update_memory_feedback(pool: &SqlitePool, used_ids: &[String], unused_ids: &[String]) {
    // Boost 被引用的记忆
    for id in used_ids {
        let _ = sqlx::query(
            "UPDATE memories SET access_count = COALESCE(access_count, 0) + 1, last_accessed = ?, priority = MIN(COALESCE(priority, 1) + 1, 3) WHERE id = ?"
        )
        .bind(chrono::Utc::now().timestamp_millis())
        .bind(id)
        .execute(pool).await;
    }

    // 记录未使用（连续 3 次未使用的记忆 decay priority）
    for id in unused_ids {
        let _ = sqlx::query(
            "UPDATE memories SET unused_recall_count = COALESCE(unused_recall_count, 0) + 1 WHERE id = ?"
        ).bind(id).execute(pool).await;

        // 检查是否需要 decay
        let count: Option<i64> = sqlx::query_scalar(
            "SELECT unused_recall_count FROM memories WHERE id = ?"
        ).bind(id).fetch_optional(pool).await.ok().flatten();

        if let Some(c) = count {
            if c >= 3 {
                let _ = sqlx::query(
                    "UPDATE memories SET priority = MAX(COALESCE(priority, 1) - 1, 0), unused_recall_count = 0 WHERE id = ?"
                ).bind(id).execute(pool).await;
                log::info!("Learner feedback: 记忆 {} priority decay（连续 {} 次未引用）", &id[..8], c);
            }
        }
    }
}

// ─── 记忆一致性验证 ──────────────────────────────────────────

/// 验证包含文件路径的记忆是否仍然有效
pub async fn verify_file_memories(pool: &SqlitePool, agent_id: &str) -> usize {
    let memories: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, content FROM memories WHERE agent_id = ? AND (content LIKE '%src/%' OR content LIKE '%lib/%' OR content LIKE '%.rs%' OR content LIKE '%.ts%') LIMIT 50"
    )
    .bind(agent_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    let mut invalidated = 0;
    for (id, content) in &memories {
        // 提取文件路径
        let paths: Vec<&str> = content.split_whitespace()
            .filter(|w| (w.contains('/') && w.contains('.')) || w.ends_with(".rs") || w.ends_with(".ts"))
            .take(3)
            .collect();

        for path in &paths {
            let clean = path.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '.' && c != '_' && c != '-');
            if !clean.is_empty() && !std::path::Path::new(clean).exists() {
                // 文件不存在 → 标记为低优先级
                let _ = sqlx::query(
                    "UPDATE memories SET priority = 0 WHERE id = ? AND COALESCE(priority, 1) > 0"
                ).bind(id).execute(pool).await;
                invalidated += 1;
                log::info!("Learner verify: 记忆 {} 引用的文件不存在: {}", &id[..8], clean);
                break;
            }
        }
    }

    invalidated
}
