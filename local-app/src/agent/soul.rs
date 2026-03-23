//! 灵魂引擎 — 模块化 System Prompt 组装
//!
//! SoulEngine 通过 PromptSection trait 将系统提示词拆分为独立模块，
//! 各 Section 从 Agent 工作区文件读取内容并渲染为 prompt 片段。
//! 稳定的 Section 排在前面，最大化 Anthropic prompt cache 命中率。

use super::workspace::AgentWorkspace;
use super::token_counter::TokenCounter;
use std::collections::HashMap;

/// Prompt Section Trait
///
/// 每个 Section 负责渲染 system prompt 的一个片段。
/// 返回 None 表示该 Section 无内容，跳过。
pub trait PromptSection: Send + Sync {
    /// Section 名称（用于日志和调试）
    fn name(&self) -> &str;

    /// 渲染 Section 内容
    fn render(&self, workspace: &AgentWorkspace) -> Option<String>;
}

/// 灵魂引擎
///
/// 持有所有 PromptSection，按顺序组装完整的 system prompt
pub struct SoulEngine {
    sections: Vec<Box<dyn PromptSection>>,
}

impl SoulEngine {
    /// 创建空的灵魂引擎
    pub fn new() -> Self {
        Self {
            sections: Vec::new(),
        }
    }

    /// 创建带默认 Section 的灵魂引擎
    pub fn with_defaults() -> Self {
        let mut engine = Self::new();
        engine.add_section(Box::new(IdentitySection));
        engine.add_section(Box::new(SoulSection));
        engine.add_section(Box::new(SafetySection));
        engine.add_section(Box::new(ToolsSection));
        engine.add_section(Box::new(MemorySection));
        engine.add_section(Box::new(UserSection));
        engine.add_section(Box::new(ReflectionsSection));
        engine.add_section(Box::new(FocusSection));
        engine.add_section(Box::new(DateTimeSection));
        engine
    }

    /// 添加 Section
    pub fn add_section(&mut self, section: Box<dyn PromptSection>) {
        self.sections.push(section);
    }

    /// 组装完整的 system prompt
    ///
    /// 按 Section 顺序渲染，每个有内容的 Section 用分隔符拼接。
    /// 稳定部分（Identity/Soul/Safety）在前，动态部分（Memory/DateTime）在后，
    /// 有利于 Anthropic prompt cache 命中。
    pub fn build_system_prompt(&self, workspace: &AgentWorkspace) -> String {
        let mut parts: Vec<String> = Vec::new();

        for section in &self.sections {
            if let Some(content) = section.render(workspace) {
                if !content.trim().is_empty() {
                    parts.push(content);
                }
            }
        }

        parts.join("\n\n---\n\n")
    }

    /// 获取所有 Section 名称
    pub fn section_names(&self) -> Vec<&str> {
        self.sections.iter().map(|s| s.name()).collect()
    }

    /// 带预算的 system prompt 组装
    ///
    /// 每个 Section 按独立预算截断，总量不超过 total budget。
    /// 超出总预算时，按比例缩减非保护 section（identity/safety/datetime 受保护）。
    pub fn build_system_prompt_with_budget(
        &self,
        workspace: &AgentWorkspace,
        budget: &SectionBudget,
    ) -> String {
        let protected = ["identity", "safety", "datetime"];
        let mut parts: Vec<(String, String)> = Vec::new(); // (name, content)

        for section in &self.sections {
            if let Some(content) = section.render(workspace) {
                if content.trim().is_empty() {
                    continue;
                }
                let max = budget.limits.get(section.name()).copied().unwrap_or(3_000);
                let truncated = TokenCounter::truncate_to_budget(&content, max);
                parts.push((section.name().to_string(), truncated));
            }
        }

        // 总预算检查
        let total: usize = parts.iter().map(|(_, c)| TokenCounter::count(c)).sum();
        if total > budget.total {
            log::warn!(
                "System prompt {} tokens 超出预算 {}，缩减非保护 section",
                total,
                budget.total
            );
            let protected_tokens: usize = parts
                .iter()
                .filter(|(name, _)| protected.contains(&name.as_str()))
                .map(|(_, c)| TokenCounter::count(c))
                .sum();
            let remaining_budget = budget.total.saturating_sub(protected_tokens);
            let shrinkable_tokens: usize = parts
                .iter()
                .filter(|(name, _)| !protected.contains(&name.as_str()))
                .map(|(_, c)| TokenCounter::count(c))
                .sum();

            if shrinkable_tokens > 0 {
                let ratio = remaining_budget as f64 / shrinkable_tokens as f64;
                let ratio = ratio.min(1.0);
                for (name, content) in &mut parts {
                    if !protected.contains(&name.as_str()) {
                        let current = TokenCounter::count(content);
                        let new_max = (current as f64 * ratio) as usize;
                        if new_max < current {
                            *content = TokenCounter::truncate_to_budget(content, new_max);
                        }
                    }
                }
            }
        }

        parts
            .into_iter()
            .map(|(_, c)| c)
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    }
}

/// Section 级别的 token 预算配置
pub struct SectionBudget {
    /// 每个 section 的最大 token 数
    pub limits: HashMap<String, usize>,
    /// 所有 section 的总预算
    pub total: usize,
}

impl Default for SectionBudget {
    fn default() -> Self {
        let limits = HashMap::from([
            ("identity".into(), 1_500),
            ("soul".into(), 3_000),
            ("safety".into(), 2_000),
            ("tools".into(), 4_000),
            ("memory".into(), 3_000),
            ("user".into(), 1_500),
            ("datetime".into(), 200),
            ("focus".into(), 1_500),
            ("bootstrap".into(), 1_000),
        ]);
        Self {
            limits,
            total: 20_000,
        }
    }
}

impl Default for SoulEngine {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// 会话类型
///
/// 不同会话类型加载不同的 Section 集合，减少不必要的 token 消耗。
/// - Full: 完整会话，加载所有 Section
/// - Light: 轻量会话，跳过 memory/user/safety
/// - SubAgent: 子代理，仅 identity + tools + datetime
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SessionType {
    /// 完整会话（1对1），加载所有 Section
    Full,
    /// 轻量会话，跳过 memory/user/safety
    Light,
    /// 子代理，仅 identity + tools + datetime
    SubAgent,
    /// 群聊会话 — 排除 MEMORY.md 和 USER.md（防止泄露私人记忆）
    Group,
}

impl SoulEngine {
    /// 根据会话类型创建灵魂引擎
    pub fn for_session(session_type: SessionType) -> Self {
        let mut engine = Self::new();
        match session_type {
            SessionType::Full => {
                engine.add_section(Box::new(IdentitySection));
                engine.add_section(Box::new(SoulSection));
                engine.add_section(Box::new(SafetySection));
                engine.add_section(Box::new(ToolsSection));
                engine.add_section(Box::new(MemorySection));
                engine.add_section(Box::new(UserSection));
                engine.add_section(Box::new(ReflectionsSection));
                engine.add_section(Box::new(DateTimeSection));
            }
            SessionType::Light => {
                engine.add_section(Box::new(IdentitySection));
                engine.add_section(Box::new(SoulSection));
                engine.add_section(Box::new(ToolsSection));
                engine.add_section(Box::new(DateTimeSection));
            }
            SessionType::SubAgent => {
                engine.add_section(Box::new(IdentitySection));
                engine.add_section(Box::new(ToolsSection));
                engine.add_section(Box::new(DateTimeSection));
            }
            SessionType::Group => {
                // 群聊：排除 MEMORY.md 和 USER.md，防止泄露私人记忆
                engine.add_section(Box::new(IdentitySection));
                engine.add_section(Box::new(SoulSection));
                engine.add_section(Box::new(SafetySection));
                engine.add_section(Box::new(ToolsSection));
                engine.add_section(Box::new(DateTimeSection));
            }
        }
        engine
    }
}

// ─── 内置 Sections ────────────────────────────────────────────

/// 身份 Section — 从 IDENTITY.md 读取
pub struct IdentitySection;

impl PromptSection for IdentitySection {
    fn name(&self) -> &str {
        "identity"
    }

    fn render(&self, workspace: &AgentWorkspace) -> Option<String> {
        workspace.read("IDENTITY.md")
    }
}

/// 灵魂 Section — 从 SOUL.md 读取
///
/// 注入人格体现指令，确保 SOUL.md 定义的人格优先于 IDENTITY.md
pub struct SoulSection;

impl PromptSection for SoulSection {
    fn name(&self) -> &str {
        "soul"
    }

    fn render(&self, workspace: &AgentWorkspace) -> Option<String> {
        let content = workspace.read("SOUL.md")?;
        if content.trim().is_empty() {
            return None;
        }
        let directive = "## 人格体现\n\n\
            以下 SOUL.md 定义了你的核心人格、语气和行为方式。\
            当与其他文件（如 IDENTITY.md）冲突时，以 SOUL.md 为准。\n\n";
        Some(format!("{directive}{content}"))
    }
}

/// 安全红线 Section — 从 AGENTS.md 读取
pub struct SafetySection;

impl PromptSection for SafetySection {
    fn name(&self) -> &str {
        "safety"
    }

    fn render(&self, workspace: &AgentWorkspace) -> Option<String> {
        workspace.read("AGENTS.md")
    }
}

/// 工具 Section — 由外部注入可用工具列表
///
/// 当前为占位实现，Phase 9b 实装 Function Calling 时完善
pub struct ToolsSection;

/// 渐进式披露阈值：超过此字符数只注入摘要
const TOOLS_PROGRESSIVE_THRESHOLD: usize = 1000;

impl PromptSection for ToolsSection {
    fn name(&self) -> &str {
        "tools"
    }

    fn render(&self, workspace: &AgentWorkspace) -> Option<String> {
        let content = workspace.read("TOOLS.md").unwrap_or_default();

        // 动态追加已安装技能的索引（从 skills/ 目录扫描）
        let skills_dir = workspace.root().join("skills");
        let skill_index = if skills_dir.exists() {
            let mgr = super::skills::SkillManager::scan(&skills_dir);
            let index = mgr.index();
            if index.is_empty() {
                String::new()
            } else {
                let mut lines = vec!["\n## Installed Skills（按需激活）\n".to_string()];
                lines.push("| 技能 | 描述 | 触发词 |".to_string());
                lines.push("|------|------|--------|".to_string());
                for skill in index {
                    let keywords = if skill.trigger_keywords.is_empty() {
                        "(自动)".to_string()
                    } else {
                        skill.trigger_keywords.join("、")
                    };
                    lines.push(format!("| {} | {} | {} |", skill.name, skill.description.chars().take(50).collect::<String>(), keywords));
                }
                lines.push("\n技能在对话中根据关键词自动激活，无需手动调用。".to_string());
                lines.join("\n")
            }
        } else {
            String::new()
        };

        let full_content = if content.trim().is_empty() {
            if skill_index.is_empty() { return None; }
            format!("# Available Tools\n{}", skill_index)
        } else {
            // 如果 TOOLS.md 已经有 Installed Skills 部分，用动态版本替换
            let base = if let Some(pos) = content.find("## Installed Skills") {
                content[..pos].trim_end().to_string()
            } else {
                content
            };
            format!("{}{}", base, skill_index)
        };

        // 追加工具安全规则
        let safety_rules = "\n\n## Tool Safety Rules\n\n\
            Before executing the following actions, **always tell the user what you plan to do and ask for confirmation**:\n\
            - `bash_exec`: Describe the command and its effect\n\
            - `file_write` / `file_edit` / `diff_edit`: Describe what file will be changed and how\n\
            - `cron_manage` (create/delete): Confirm the schedule and action\n\
            - `skill_manage` (install/uninstall): Confirm the skill name\n\
            - Any tool that modifies system state\n\n\
            Safe tools that can be used without confirmation: `calculator`, `datetime`, `memory_read`, `file_read`, `file_list`, `code_search`, `web_search`, `web_fetch`.\n";
        let full_content = format!("{}{}", full_content, safety_rules);

        // 渐进式披露：大内容只注入摘要
        if full_content.len() <= TOOLS_PROGRESSIVE_THRESHOLD {
            return Some(full_content);
        }

        let mut summary_lines: Vec<&str> = Vec::new();
        for line in full_content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.starts_with('-') || trimmed.starts_with('|') {
                summary_lines.push(trimmed);
            }
        }

        if summary_lines.is_empty() {
            let preview: String = full_content.chars().take(800).collect();
            return Some(format!("# Tools Configuration (摘要)\n\n{}\n\n...", preview));
        }

        Some(format!(
            "# Tools & Skills (摘要)\n\n{}\n\n如需查看完整工具配置详情，请使用 file_read 工具读取工作区中的 TOOLS.md 文件。",
            summary_lines.join("\n")
        ))
    }
}

/// 记忆 Section — 从 MEMORY.md 读取策展记忆
///
/// Phase 9a 先读取静态文件，后续 MemoryLoader 注入语义检索结果
pub struct MemorySection;

impl PromptSection for MemorySection {
    fn name(&self) -> &str {
        "memory"
    }

    fn render(&self, workspace: &AgentWorkspace) -> Option<String> {
        let memory = workspace.read("MEMORY.md").filter(|c| !c.trim().is_empty());
        memory.map(|content| {
            format!("# Long-Term Memory\n\n{}", content)
        })
    }
}

/// 反思日志 Section — 从 reflections.md 读取（最近 500 字符）
pub struct ReflectionsSection;

impl PromptSection for ReflectionsSection {
    fn name(&self) -> &str {
        "reflections"
    }

    fn render(&self, workspace: &AgentWorkspace) -> Option<String> {
        let content = workspace.read("reflections.md").filter(|c| !c.trim().is_empty())?;
        let trimmed = if content.len() > 500 {
            let start = content.len() - 500;
            if let Some(pos) = content[start..].find("---") {
                &content[start + pos..]
            } else {
                &content[start..]
            }
        } else {
            &content
        };
        Some(format!("# Recent Reflections\n\n{}", trimmed.trim()))
    }
}

/// Focus Items Section — 从 FOCUS.md 加载结构化工作记忆
///
/// 格式：
/// - [ ] 待办项
/// - [/] 进行中
/// - [x] 已完成
///
/// 只注入未完成的 items（[ ] 和 [/]），跳过已完成的。
pub struct FocusSection;

impl PromptSection for FocusSection {
    fn name(&self) -> &str {
        "focus"
    }

    fn render(&self, workspace: &AgentWorkspace) -> Option<String> {
        let content = workspace.read("FOCUS.md").filter(|c| !c.trim().is_empty())?;

        // 提取未完成的 focus items
        let active_items: Vec<&str> = content.lines()
            .filter(|line| {
                let trimmed = line.trim();
                trimmed.starts_with("- [ ]") || trimmed.starts_with("- [/]")
            })
            .collect();

        if active_items.is_empty() {
            return None;
        }

        let mut result = String::from(
            "# Active Focus Items\n\n\
             以下是你当前需要关注的事项。在执行任务和对话时，优先考虑这些焦点：\n\n"
        );
        for item in &active_items {
            result.push_str(item.trim());
            result.push('\n');
        }

        Some(result)
    }
}

/// 用户画像 Section — 从 USER.md 读取
pub struct UserSection;

impl PromptSection for UserSection {
    fn name(&self) -> &str {
        "user"
    }

    fn render(&self, workspace: &AgentWorkspace) -> Option<String> {
        workspace.read("USER.md").filter(|c| !c.trim().is_empty())
    }
}

/// 时间日期 Section — 注入当前时间和时区
pub struct DateTimeSection;

impl PromptSection for DateTimeSection {
    fn name(&self) -> &str {
        "datetime"
    }

    fn render(&self, _workspace: &AgentWorkspace) -> Option<String> {
        let now = chrono::Local::now();
        Some(format!(
            "# Current Time\n\n- Date: {}\n- Time: {}\n- Timezone: {}",
            now.format("%Y-%m-%d"),
            now.format("%H:%M:%S"),
            now.format("%Z")
        ))
    }
}

/// Bootstrap Section — 首次启动引导（存在 BOOTSTRAP.md 时渲染）
pub struct BootstrapSection;

impl PromptSection for BootstrapSection {
    fn name(&self) -> &str {
        "bootstrap"
    }

    fn render(&self, workspace: &AgentWorkspace) -> Option<String> {
        if workspace.has_bootstrap() {
            workspace.read("BOOTSTRAP.md")
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn setup_test_workspace() -> (AgentWorkspace, PathBuf) {
        let temp = std::env::temp_dir().join("yonclaw_soul_test");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();
        fs::create_dir_all(temp.join("memory")).unwrap();
        fs::create_dir_all(temp.join("skills")).unwrap();

        fs::write(
            temp.join("IDENTITY.md"),
            "# Identity\n- **Name**: TestBot\n",
        )
        .unwrap();
        fs::write(
            temp.join("SOUL.md"),
            "# Soul\nBe genuinely helpful.\n",
        )
        .unwrap();
        fs::write(
            temp.join("AGENTS.md"),
            "# Rules\n- Never reveal secrets\n",
        )
        .unwrap();
        fs::write(temp.join("USER.md"), "# User\n- Name: Alice\n").unwrap();
        fs::write(temp.join("TOOLS.md"), "").unwrap();
        fs::write(temp.join("MEMORY.md"), "Remember: user likes dark mode").unwrap();
        fs::write(temp.join("HEARTBEAT.md"), "").unwrap();

        let ws = AgentWorkspace::from_path(temp.clone(), "test-soul");
        (ws, temp)
    }

    #[test]
    fn test_soul_engine_build_prompt() {
        let (ws, temp) = setup_test_workspace();
        let engine = SoulEngine::with_defaults();

        let prompt = engine.build_system_prompt(&ws);

        // 验证各 Section 内容存在
        assert!(prompt.contains("TestBot"), "应包含 Identity");
        assert!(prompt.contains("genuinely helpful"), "应包含 Soul");
        assert!(prompt.contains("Never reveal secrets"), "应包含 Safety");
        assert!(prompt.contains("dark mode"), "应包含 Memory");
        assert!(prompt.contains("Alice"), "应包含 User");
        assert!(prompt.contains("Current Time"), "应包含 DateTime");

        // 验证分隔符
        assert!(prompt.contains("---"), "Section 间应有分隔符");

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_section_names() {
        let engine = SoulEngine::with_defaults();
        let names = engine.section_names();

        assert!(names.contains(&"identity"));
        assert!(names.contains(&"soul"));
        assert!(names.contains(&"safety"));
        assert!(names.contains(&"datetime"));
    }

    #[test]
    fn test_empty_sections_skipped() {
        let temp = std::env::temp_dir().join("yonclaw_soul_empty");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        // 只写 IDENTITY.md，其他文件不创建
        fs::write(temp.join("IDENTITY.md"), "# Identity\n- Name: Solo\n").unwrap();

        let ws = AgentWorkspace::from_path(temp.clone(), "test-empty");
        let engine = SoulEngine::with_defaults();
        let prompt = engine.build_system_prompt(&ws);

        // 应包含 Identity 和 DateTime（始终有内容）
        assert!(prompt.contains("Solo"));
        assert!(prompt.contains("Current Time"));

        // 不应包含不存在的文件内容
        assert!(!prompt.contains("Long-Term Memory"));

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_custom_section() {
        struct CustomSection;
        impl PromptSection for CustomSection {
            fn name(&self) -> &str { "custom" }
            fn render(&self, _ws: &AgentWorkspace) -> Option<String> {
                Some("Custom instruction: always be concise".to_string())
            }
        }

        let temp = std::env::temp_dir().join("yonclaw_soul_custom");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let ws = AgentWorkspace::from_path(temp.clone(), "test-custom");
        let mut engine = SoulEngine::new();
        engine.add_section(Box::new(CustomSection));

        let prompt = engine.build_system_prompt(&ws);
        assert!(prompt.contains("always be concise"));

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_session_type_full() {
        let engine = SoulEngine::for_session(SessionType::Full);
        let names = engine.section_names();
        assert_eq!(names.len(), 8);
        assert!(names.contains(&"identity"));
        assert!(names.contains(&"soul"));
        assert!(names.contains(&"safety"));
        assert!(names.contains(&"memory"));
        assert!(names.contains(&"user"));
        assert!(names.contains(&"reflections"));
        assert!(names.contains(&"datetime"));
    }

    #[test]
    fn test_session_type_light() {
        let engine = SoulEngine::for_session(SessionType::Light);
        let names = engine.section_names();
        assert_eq!(names.len(), 4);
        assert!(names.contains(&"identity"));
        assert!(names.contains(&"soul"));
        assert!(names.contains(&"tools"));
        assert!(names.contains(&"datetime"));
        assert!(!names.contains(&"memory"));
        assert!(!names.contains(&"user"));
        assert!(!names.contains(&"safety"));
    }

    #[test]
    fn test_session_type_subagent() {
        let engine = SoulEngine::for_session(SessionType::SubAgent);
        let names = engine.section_names();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"identity"));
        assert!(names.contains(&"tools"));
        assert!(names.contains(&"datetime"));
        assert!(!names.contains(&"soul"));
        assert!(!names.contains(&"memory"));
    }
}
