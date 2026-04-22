//! Dreaming / REM 记忆整理
//!
//! 参照 OpenClaw #63273/#63297 的 Dreaming 设计：
//! - **Light Sleep**：从最近 24h 对话中提取"观察"（事实、偏好、结论）
//! - **REM Sleep**：对多天对话做深度分析（模式、关联、信念层）
//!
//! 运行方式：
//! - 手动：Tauri 命令 `run_dreaming`（/dream 斜杠命令）
//! - 自动：scheduler 种子任务 `ActionPayload::Dreaming { phase }`
//!   - Light Sleep：每日 03:00（浅睡观察）
//!   - REM Sleep：每周日 03:30（深度模式提炼）
//! 输出：`~/.xianzhu/agents/{agent_id}/memory/dreaming/{phase}/YYYY-MM-DD.md`
//!
//! 与 OpenClaw 的差异：
//! - 分离存储（storage.mode=separate）默认，避免污染日常记忆文件
//! - 按 agent 维度隔离，每个 agent 有独立的 dreaming 目录

use std::path::PathBuf;

/// Dreaming 阶段
#[derive(Debug, Clone, Copy)]
pub enum DreamPhase {
    /// 浅睡 — 快速观察
    LightSleep,
    /// 快速眼动睡眠 — 深度模式提炼
    RemSleep,
}

impl DreamPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LightSleep => "light",
            Self::RemSleep => "rem",
        }
    }

    /// 从字符串解析（与 `as_str` 对称，避免 runner/seed 硬编码漂移）
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "light" => Ok(Self::LightSleep),
            "rem" => Ok(Self::RemSleep),
            other => Err(format!("未知 DreamPhase: {}", other)),
        }
    }

    /// 对应阶段的分析 prompt
    pub fn prompt_template(&self) -> &'static str {
        match self {
            Self::LightSleep => "你正在进行 Light Sleep（浅睡）记忆整理。\n\
                从以下最近 24 小时的对话摘录中，提取最多 5 条值得长期记住的观察：\n\
                - 用户偏好（工作风格、技术栈倾向、回答语气）\n\
                - 关键事实（项目背景、时间节点、决策理由）\n\
                - 已确立的约定（命名规范、流程、禁忌）\n\n\
                每条观察用 markdown bullet 列出，简短明确。不要编造，只从对话中提取。\n\
                如果没有值得记忆的内容，只输出「本日无新观察」。",
            Self::RemSleep => "你正在进行 REM Sleep（深度睡眠）记忆整理。\n\
                从以下最近多天的对话摘录中，做深度分析：\n\
                - **模式**：用户重复出现的需求类型、常踩的坑、偏好的解决路径\n\
                - **关联**：不同项目/话题之间隐含的联系\n\
                - **信念层**：基于交互可以形成的对用户的「心智模型」\n\n\
                输出三个小节 `## Patterns` / `## Connections` / `## Beliefs`，\n\
                每节 3-5 条 bullet，简短明确。只从对话中提取，不编造。",
        }
    }
}

/// Dreaming 输出目录（每个 agent 独立）
pub fn dreaming_dir(agent_workspace: &str, phase: DreamPhase) -> PathBuf {
    PathBuf::from(agent_workspace).join("memory").join("dreaming").join(phase.as_str())
}

/// Dreaming 今日文件路径
pub fn dreaming_file_for_today(agent_workspace: &str, phase: DreamPhase) -> PathBuf {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    dreaming_dir(agent_workspace, phase).join(format!("{}.md", today))
}

/// 拉取最近 N 小时的对话（简化为最近 N 条消息）
pub async fn fetch_recent_conversations(
    pool: &sqlx::SqlitePool,
    agent_id: &str,
    hours: i64,
    limit: usize,
) -> Result<Vec<(String, String, String)>, String> {
    let since = chrono::Utc::now() - chrono::Duration::hours(hours);
    let since_ms = since.timestamp_millis();
    let rows: Vec<(String, String, String)> = sqlx::query_as::<_, (String, String, String)>(
        "SELECT m.role, m.content, s.id \
         FROM chat_messages m \
         JOIN chat_sessions s ON m.session_id = s.id \
         WHERE s.agent_id = ? AND m.created_at > ? \
         ORDER BY m.created_at DESC LIMIT ?"
    )
    .bind(agent_id)
    .bind(since_ms)
    .bind(limit as i64)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询对话失败: {}", e))?;
    Ok(rows)
}

/// 把对话序列化为简短 transcript（用于放入 prompt）
pub fn format_transcript(msgs: &[(String, String, String)], max_chars_per_msg: usize) -> String {
    let mut out = String::new();
    for (role, content, _session) in msgs.iter().rev() {  // 时间顺序
        if role == "tool" { continue; }  // 跳过工具调用
        let preview: String = content.chars().take(max_chars_per_msg).collect();
        let prefix = if role == "user" { "U" } else { "A" };
        out.push_str(&format!("[{}] {}\n", prefix, preview));
    }
    out
}

/// 执行一次 Dreaming 循环（某个 agent，某个 phase）
///
/// 返回：(phase_name, saved_path, content_summary) 或 Err
pub async fn run_dream_phase(
    pool: &sqlx::SqlitePool,
    agent_id: &str,
    agent_workspace: &str,
    phase: DreamPhase,
    llm_config: &super::llm::LlmConfig,
) -> Result<(String, PathBuf, String), String> {
    // 拉对话
    let (hours, limit) = match phase {
        DreamPhase::LightSleep => (24_i64, 200_usize),
        DreamPhase::RemSleep => (72, 500),
    };
    let msgs = fetch_recent_conversations(pool, agent_id, hours, limit).await?;
    if msgs.is_empty() {
        return Err("最近无对话，跳过".to_string());
    }
    let transcript = format_transcript(&msgs, 300);

    // 调 LLM
    let prompt = format!("{}\n\n---\n对话摘录：\n{}\n---", phase.prompt_template(), transcript);
    let client = super::llm::LlmClient::new(llm_config.clone());
    let messages = vec![serde_json::json!({"role": "user", "content": prompt})];
    let (dummy_tx, _rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let resp = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        client.call_stream(&messages, None, None, dummy_tx)
    ).await
    .map_err(|_| "Dreaming LLM 超时")?
    .map_err(|e| format!("Dreaming LLM 失败: {}", e))?;

    let content = resp.content.trim().to_string();
    // 严格匹配"本日无新观察"整行，避免误杀包含子串的合法输出
    let is_empty_result = content.is_empty()
        || content.lines().any(|l| l.trim() == "本日无新观察");
    if is_empty_result {
        return Err("本次无新观察".to_string());
    }

    // 写文件
    let out_path = dreaming_file_for_today(agent_workspace, phase);
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent).await
            .map_err(|e| format!("创建目录失败: {}", e))?;
    }
    let header = format!(
        "# {} — {}\n\n生成时间：{}\n\n",
        phase.as_str(),
        chrono::Local::now().format("%Y-%m-%d"),
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
    );
    let full = format!("{}{}\n", header, content);
    tokio::fs::write(&out_path, &full).await
        .map_err(|e| format!("写入失败: {}", e))?;

    log::info!("Dreaming {} 完成: agent={}, path={:?}, {} 字节",
        phase.as_str(),
        agent_id.chars().take(8).collect::<String>(),
        out_path, full.len());

    let summary: String = content.chars().take(200).collect();
    Ok((phase.as_str().to_string(), out_path, summary))
}

/// OpenClaw memory-wiki (#63332 系列): 信念层摘要编译
///
/// 聚合最近 N 天的 REM Sleep 输出为单个 WIKI.md，作为该 agent 的"长期信念库"。
/// 读取时优先查询 WIKI.md；单日详情仍在 rem/{date}.md 中。
pub async fn compile_wiki_digest(
    agent_workspace: &str,
    max_days: usize,
) -> Result<PathBuf, String> {
    let rem_dir = dreaming_dir(agent_workspace, DreamPhase::RemSleep);
    if !rem_dir.exists() {
        return Err("尚无 REM 记忆可编译".to_string());
    }

    // 列出 rem 目录下所有 YYYY-MM-DD.md
    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    let mut rd = tokio::fs::read_dir(&rem_dir).await
        .map_err(|e| format!("读取 rem 目录失败: {}", e))?;
    while let Some(e) = rd.next_entry().await.map_err(|e| format!("遍历失败: {}", e))? {
        let name = e.file_name().to_string_lossy().to_string();
        if name.ends_with(".md") && !name.eq_ignore_ascii_case("WIKI.md") {
            entries.push((name.trim_end_matches(".md").to_string(), e.path()));
        }
    }
    // 按日期降序
    entries.sort_by(|a, b| b.0.cmp(&a.0));
    let recent: Vec<_> = entries.into_iter().take(max_days).collect();

    if recent.is_empty() {
        return Err("无可编译的 REM 记忆".to_string());
    }

    let mut body = String::from("# Memory Wiki (belief-layer digests)\n\n");
    body.push_str(&format!("编译时间：{}  \n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
    body.push_str(&format!("合并天数：{}\n\n---\n\n", recent.len()));

    for (date, path) in &recent {
        let content = tokio::fs::read_to_string(path).await.unwrap_or_default();
        body.push_str(&format!("## {}\n\n{}\n\n---\n\n", date, content.trim()));
    }

    let wiki_path = rem_dir.join("WIKI.md");
    tokio::fs::write(&wiki_path, body).await
        .map_err(|e| format!("写入 WIKI.md 失败: {}", e))?;
    log::info!("Wiki 编译完成: {:?} ({} 天)", wiki_path, recent.len());
    Ok(wiki_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_str() {
        assert_eq!(DreamPhase::LightSleep.as_str(), "light");
        assert_eq!(DreamPhase::RemSleep.as_str(), "rem");
    }

    #[test]
    fn test_dreaming_dir() {
        let p = dreaming_dir("/tmp/ws", DreamPhase::LightSleep);
        assert!(p.to_string_lossy().ends_with("memory/dreaming/light"));
    }
}
