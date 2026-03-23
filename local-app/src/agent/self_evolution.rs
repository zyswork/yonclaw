//! 自我进化引擎
//!
//! 参考 Hermes Agent 的学习循环：
//! - 每 N 轮工具调用 → 触发技能 review（自动创建/改进技能）
//! - 每 N 次用户消息 → 触发记忆 review（更新用户画像/偏好）
//! - review 在后台异步执行，不阻塞用户对话
//!
//! 核心思路：不是每轮都学习，而是积累到阈值后批量反思。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// 进化引擎配置
pub struct EvolutionConfig {
    /// 多少次工具调用后触发技能 review
    pub skill_nudge_interval: u32,
    /// 多少次用户消息后触发记忆 review
    pub memory_nudge_interval: u32,
    /// 是否启用自我进化
    pub enabled: bool,
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self {
            skill_nudge_interval: 8,
            memory_nudge_interval: 6,
            enabled: true,
        }
    }
}

/// 进化引擎状态（线程安全）
pub struct EvolutionState {
    /// 自上次技能 review 以来的工具调用次数
    pub tool_calls_since_skill_review: AtomicU32,
    /// 自上次记忆 review 以来的用户消息次数
    pub user_msgs_since_memory_review: AtomicU32,
    /// 是否有正在运行的 review 任务
    pub review_in_progress: AtomicU32, // 0=空闲, 1=运行中
}

impl EvolutionState {
    pub fn new() -> Self {
        Self {
            tool_calls_since_skill_review: AtomicU32::new(0),
            user_msgs_since_memory_review: AtomicU32::new(0),
            review_in_progress: AtomicU32::new(0),
        }
    }

    /// 记录一次工具调用
    pub fn on_tool_call(&self) {
        self.tool_calls_since_skill_review.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录一次用户消息
    pub fn on_user_message(&self) {
        self.user_msgs_since_memory_review.fetch_add(1, Ordering::Relaxed);
    }

    /// 检查是否应该触发技能 review
    pub fn should_review_skills(&self, config: &EvolutionConfig) -> bool {
        if !config.enabled || config.skill_nudge_interval == 0 { return false; }
        self.tool_calls_since_skill_review.load(Ordering::Relaxed) >= config.skill_nudge_interval
    }

    /// 检查是否应该触发记忆 review
    pub fn should_review_memory(&self, config: &EvolutionConfig) -> bool {
        if !config.enabled || config.memory_nudge_interval == 0 { return false; }
        self.user_msgs_since_memory_review.load(Ordering::Relaxed) >= config.memory_nudge_interval
    }

    /// 重置技能计数器
    pub fn reset_skill_counter(&self) {
        self.tool_calls_since_skill_review.store(0, Ordering::Relaxed);
    }

    /// 重置记忆计数器
    pub fn reset_memory_counter(&self) {
        self.user_msgs_since_memory_review.store(0, Ordering::Relaxed);
    }
}

/// 技能 review 提示词
const SKILL_REVIEW_PROMPT: &str = r#"你是一个后台反思代理。请审查刚才的对话，判断是否有值得提取为可复用技能的模式。

审查标准：
- 用户是否要求了一个非平凡的操作（不是简单问答）？
- 是否经过了试错、调整方案、多步工具调用？
- 这个操作模式是否可能再次出现？

如果发现可复用模式，请使用 skill_manage 工具创建或更新技能。
如果没有值得提取的模式，直接回复"无需创建技能"。

注意：
- 优先更新已有技能（patch），而不是创建新的
- 技能应该是具体的操作流程，不是泛泛的描述
- 包含实际的命令、参数、注意事项
"#;

/// 记忆 review 提示词
const MEMORY_REVIEW_PROMPT: &str = r#"你是一个后台反思代理。请审查刚才的对话，判断是否有值得记住的用户信息。

审查标准：
- 用户是否透露了个人偏好（沟通风格、工具偏好、工作习惯）？
- 用户是否提到了项目背景、技术栈、团队信息？
- 是否有重要的决策或结论需要长期记住？
- 用户是否纠正了你的行为（说明偏好）？

如果发现值得记住的信息，请使用 memory_write 工具保存。
如果对话中没有新的用户信息，直接回复"无需更新记忆"。

注意：
- 不要重复保存已知信息
- 先用 memory_read 检查是否已有相关记忆
- 记忆应该具体、可操作，不是泛泛的描述
"#;

/// 在后台执行进化 review
///
/// 不阻塞主对话，异步分析刚才的对话并提取技能/记忆。
pub async fn spawn_background_review(
    pool: sqlx::SqlitePool,
    agent_id: String,
    _session_id: String,
    api_key: String,
    api_type: String,
    base_url: Option<String>,
    model: String,
    review_type: ReviewType,
    recent_messages: Vec<serde_json::Value>,
    evolution_state: Arc<EvolutionState>,
) {
    // 防止并发 review
    if evolution_state.review_in_progress.compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        log::info!("进化引擎: 已有 review 在运行，跳过");
        return;
    }

    let review_prompt = match review_type {
        ReviewType::Skill => SKILL_REVIEW_PROMPT,
        ReviewType::Memory => MEMORY_REVIEW_PROMPT,
        ReviewType::Both => SKILL_REVIEW_PROMPT, // 先做技能，记忆在第二轮
    };

    tokio::spawn(async move {
        log::info!("进化引擎: 开始后台 {:?} review（{}条消息）", review_type, recent_messages.len());

        // 构建 review 消息：摘要最近的对话 + review 提示
        let conversation_summary = summarize_recent_messages(&recent_messages);
        let review_message = format!(
            "以下是刚才的对话摘要：\n\n{}\n\n---\n\n{}",
            conversation_summary, review_prompt
        );

        // 用同一个模型做 review（轻量调用，不带工具）
        let config = super::llm::LlmConfig {
            provider: api_type,
            model,
            api_key,
            base_url,
            temperature: Some(0.3), // 低温度，更确定性
            max_tokens: Some(2000),
            thinking_level: None,
        };

        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let client = super::llm::LlmClient::new(config);

        let messages = vec![
            serde_json::json!({"role": "user", "content": review_message}),
        ];

        match client.call_stream(&messages, None, None, tx).await {
            Ok(response) => {
                let content = response.content.trim().to_string();
                if content.contains("无需") || content.is_empty() {
                    log::info!("进化引擎: {:?} review 完成，无需更新", review_type);
                } else {
                    log::info!("进化引擎: {:?} review 产出 {}字符", review_type, content.len());
                    // 保存 review 结果到 agent 的 reflections
                    save_reflection(&pool, &agent_id, &review_type, &content).await;
                }
            }
            Err(e) => {
                log::warn!("进化引擎: {:?} review 失败: {}", review_type, e);
            }
        }

        // 清空接收端
        while rx.try_recv().is_ok() {}

        // 重置计数器和状态
        match review_type {
            ReviewType::Skill => evolution_state.reset_skill_counter(),
            ReviewType::Memory => evolution_state.reset_memory_counter(),
            ReviewType::Both => {
                evolution_state.reset_skill_counter();
                evolution_state.reset_memory_counter();
            }
        }
        evolution_state.review_in_progress.store(0, Ordering::SeqCst);
        log::info!("进化引擎: {:?} review 完成", review_type);
    });
}

/// 将最近消息压缩为摘要（不用 LLM，纯文本截取）
fn summarize_recent_messages(messages: &[serde_json::Value]) -> String {
    let mut summary = String::new();
    // 只取最近 20 条消息
    let start = if messages.len() > 20 { messages.len() - 20 } else { 0 };
    for msg in &messages[start..] {
        let role = msg["role"].as_str().unwrap_or("?");
        let content = msg["content"].as_str().unwrap_or("");
        // 截取每条消息前 200 字符
        let truncated: String = content.chars().take(200).collect();
        let suffix = if content.len() > 200 { "..." } else { "" };

        match role {
            "user" => summary.push_str(&format!("用户: {}{}\n", truncated, suffix)),
            "assistant" => summary.push_str(&format!("助手: {}{}\n", truncated, suffix)),
            "tool" => {
                let tool_name = msg["name"].as_str().unwrap_or("tool");
                summary.push_str(&format!("[工具调用: {}] {}{}\n", tool_name, truncated.chars().take(100).collect::<String>(), suffix));
            }
            _ => {}
        }
    }
    summary
}

/// 保存反思结果到 agent workspace 的 reflections.md
async fn save_reflection(
    pool: &sqlx::SqlitePool,
    agent_id: &str,
    review_type: &ReviewType,
    content: &str,
) {
    // 查找 agent workspace 路径
    let workspace_path: Option<String> = sqlx::query_scalar(
        "SELECT workspace_path FROM agents WHERE id = ?"
    ).bind(agent_id).fetch_optional(pool).await.ok().flatten();

    if let Some(wp) = workspace_path {
        let reflections_path = std::path::Path::new(&wp).join("reflections.md");
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M");
        let entry = format!(
            "\n## [{:?}] {}\n\n{}\n",
            review_type, now, content
        );

        // 追加到 reflections.md
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true).append(true).open(&reflections_path)
        {
            use std::io::Write;
            let _ = file.write_all(entry.as_bytes());
            log::info!("进化引擎: 反思已保存到 {}", reflections_path.display());
        }
    }
}

/// Review 类型
#[derive(Debug, Clone)]
pub enum ReviewType {
    Skill,
    Memory,
    Both,
}
