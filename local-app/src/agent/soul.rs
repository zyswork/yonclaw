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
        engine.add_section(Box::new(StrategySection));
        engine.add_section(Box::new(SafetySection));
        engine.add_section(Box::new(ToolsSection));
        engine.add_section(Box::new(MemorySection));
        engine.add_section(Box::new(UserSection));
        engine.add_section(Box::new(ReflectionsSection));
        engine.add_section(Box::new(FocusSection));
        engine.add_section(Box::new(StandingOrdersSection));
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
        let protected = ["identity", "strategy", "safety", "datetime"];
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
            ("strategy".into(), 2_000),
            ("safety".into(), 2_000),
            ("tools".into(), 4_000),
            ("memory".into(), 3_000),
            ("user".into(), 1_500),
            ("datetime".into(), 200),
            ("focus".into(), 1_500),
            ("standing_orders".into(), 2_000),
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
                engine.add_section(Box::new(StrategySection));
                engine.add_section(Box::new(SafetySection));
                engine.add_section(Box::new(ToolsSection));
                engine.add_section(Box::new(MemorySection));
                engine.add_section(Box::new(UserSection));
                engine.add_section(Box::new(ReflectionsSection));
                engine.add_section(Box::new(StandingOrdersSection));
                engine.add_section(Box::new(DateTimeSection));
            }
            SessionType::Light => {
                engine.add_section(Box::new(IdentitySection));
                engine.add_section(Box::new(SoulSection));
                engine.add_section(Box::new(StrategySection));
                engine.add_section(Box::new(ToolsSection));
                engine.add_section(Box::new(StandingOrdersSection));
                engine.add_section(Box::new(DateTimeSection));
            }
            SessionType::SubAgent => {
                engine.add_section(Box::new(IdentitySection));
                engine.add_section(Box::new(StrategySection));
                engine.add_section(Box::new(ToolsSection));
                engine.add_section(Box::new(DateTimeSection));
            }
            SessionType::Group => {
                // 群聊：排除 MEMORY.md 和 USER.md，防止泄露私人记忆
                engine.add_section(Box::new(IdentitySection));
                engine.add_section(Box::new(SoulSection));
                engine.add_section(Box::new(StrategySection));
                engine.add_section(Box::new(SafetySection));
                engine.add_section(Box::new(ToolsSection));
                engine.add_section(Box::new(StandingOrdersSection));
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
        let raw = workspace.read("IDENTITY.md")?;
        // Hermes: 注入前威胁扫描
        let scan = super::threat_scan::scan("IDENTITY.md", &raw);
        Some(scan.cleaned)
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
        let raw = workspace.read("SOUL.md")?;
        if raw.trim().is_empty() {
            return None;
        }
        // Hermes: 注入前威胁扫描
        let scan = super::threat_scan::scan("SOUL.md", &raw);
        let directive = "## 人格体现\n\n\
            以下 SOUL.md 定义了你的核心人格、语气和行为方式。\
            当与其他文件（如 IDENTITY.md）冲突时，以 SOUL.md 为准。\n\n";
        Some(format!("{directive}{}", scan.cleaned))
    }
}

/// 策略指导 Section — 教 Agent 如何思考和使用工具
///
/// 高优先级 Section，始终注入，不依赖工作区文件。
/// 包含：思考方式、工具使用策略、技能选择、错误处理。
pub struct StrategySection;

impl PromptSection for StrategySection {
    fn name(&self) -> &str {
        "strategy"
    }

    fn render(&self, _workspace: &AgentWorkspace) -> Option<String> {
        Some(STRATEGY_CONTENT.to_string())
    }
}

const STRATEGY_CONTENT: &str = r#"## 核心原则：聚焦当前请求

**最重要的规则：永远优先关注用户最新的一条消息。**

- 用户的最新消息是你当前的唯一任务。历史上下文只作为参考背景，不要被它主导。
- 如果用户的新请求与之前的对话话题无关，忽略之前的话题，直接处理新请求。
- 不要重复回答已经回答过的问题。不要总结之前做过的事情，除非用户明确要求。
- 如果不确定用户想要什么，优先按字面意思执行，而不是去翻历史猜测。
- 短消息（如"看看"、"帮我查一下"、"继续"）要结合紧接着的上一轮对话理解，不要去关联更早的对话。

## 思考方式
收到任务后，先在内部思考（不要输出给用户），然后行动：
1. 理解：用户**这次**到底要什么？（不是上次，不是上上次）
2. 规划：需要哪些步骤？用哪些工具？什么顺序？
3. 执行：一步步执行，每步检查结果
4. 验证：结果是否符合预期？需要调整吗？

## 行动原则
- 先动手，后汇报。不要问"要不要帮你执行"，直接执行
- 遇到错误时，先自己分析原因并重试，不要立刻报错给用户
- 一次性完成整个任务，不要做一半就停下来汇报进度
- 如果需要多个工具配合，按逻辑顺序依次调用，不要等用户确认每一步
- 只读操作（读文件、搜索、查看内容）不需要任何确认，直接执行

## 工具使用策略
- 能用工具解决的，不要口头回答（比如用户问文件内容，直接 file_read，不要猜）
- 能并行的工具调用尽量并行（比如同时搜索+读文件）
- **严格遵守：内置工具优先，禁止绕道 Python**：
  | 任务 | 必须用的工具 | 禁止的方式 |
  |------|------------|-----------|
  | 读 Excel/XLS/XLSX | `doc_parse` | ❌ python pandas/openpyxl |
  | 写 Excel/CSV | `doc_write` | ❌ python openpyxl/xlsxwriter |
  | 读 PDF | `doc_parse` | ❌ python PyPDF2/pdfplumber |
  | 读 DOCX | `doc_parse` | ❌ python python-docx |
  | 数学计算 | `calculator` | ❌ python -c "print(...)" |
  | 搜索文件 | `code_search` | ❌ grep/rg via bash_exec |
  | 读文件 | `file_read` | ❌ cat/head via bash_exec |
  | 编辑文件 | `file_edit` | ❌ sed/awk via bash_exec |
  | 获取网页 | `web_fetch` | ❌ curl via bash_exec |
  | 获取时间 | `datetime` | ❌ python datetime |
  | 读/写剪贴板 | `clipboard` (action=read/write) | ❌ pbcopy/xclip via bash |
  | 截屏 | `screenshot` | ❌ screencapture via bash |
- **修改 Excel/CSV 的标准流程**（必须遵守）：
  1. `doc_parse` 读取 → 获得 Markdown 表格
  2. 在你脑中处理数据（增删改列/行）
  3. `doc_write` 写入完整数据（headers + rows）
  只需 2 次工具调用。不需要 Python，不需要 pip，不需要 bash_exec。
- bash_exec **只在以下场景使用**：安装软件、运行项目命令、执行用户明确要求的 shell 命令、系统管理、UI 自动化
- **UI 自动化**（用 bash_exec）：
  - macOS: `osascript -e 'tell application "xxx" to ...'`（AppleScript 控制任意应用）
  - macOS 按键: `osascript -e 'tell application "System Events" to keystroke "c" using command down'`
  - macOS 打开应用: `open -a "Safari" "https://..."`
  - Windows: `powershell -Command 'Start-Process "notepad"'`
- **如果 bash_exec 报 ModuleNotFoundError，立即改用内置工具，禁止重试**
- web_fetch 抓网页内容时，如果是 GitHub 仓库，优先用 raw.githubusercontent.com 获取原始文件
- 遇到大文件，先用 file_list 了解结构，再有针对性地 file_read
- 安装软件/包：直接用 bash_exec 执行安装命令，不要只告诉用户怎么装
- 不要重复调用同一个工具获取相同的信息
- 不要过度准备：简单任务不需要先 memory_read、skill_manage、多次 file_list。直接用最少的工具完成任务
- **工具调用失败 2 次后，必须换一种方式或停止重试并告知用户**
- **不要做完分析后停下来问用户"要我继续吗"——直接完成整个任务**

## 技能使用
收到请求时，先扫描可用技能列表的描述。
- 如果恰好有一个技能匹配：用 file_read 读取该技能的 SKILL.md，然后按照指引执行
- 如果多个技能可能相关：选最具体的那个
- 如果没有匹配的技能：用基础工具直接完成任务
- 不要一次读取多个技能文档，只读最相关的一个

## 错误处理
- 工具调用失败时：分析错误信息，调整参数重试（最多 2 次）
- 网络请求失败时：尝试替代 URL 或不同的搜索词
- 文件不存在时：用 file_list 查看目录确认路径，然后重试
- 权限不足时：尝试 sudo 或告知用户需要权限
- 不要因为一次失败就放弃整个任务"#;

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
            以下工具可以直接使用，**不需要**询问用户确认：\n\
            - 所有只读工具：`file_read`, `file_list`, `code_search`, `web_search`, `web_fetch`, `calculator`, `datetime`, `memory_read`, `doc_parse`\n\
            - 读取和分析类操作：查看文件、搜索内容、获取网页、查看目录\n\n\
            以下工具需要**简要说明后执行**（不需要等用户回复确认）：\n\
            - `file_write` / `file_edit` / `diff_edit`: 简述修改内容后直接执行\n\
            - `bash_exec`: 简述命令目的后直接执行（危险命令如 rm -rf、sudo 除外）\n\n\
            以下工具**必须先获得用户确认**才能执行：\n\
            - 删除文件、格式化磁盘等不可逆操作\n\
            - `bash_exec` 中的危险命令（rm -rf, sudo, kill, chmod 等）\n\
            - `cron_manage` (create/delete): 确认计划任务内容\n\
            - `skill_manage` (install/uninstall): 确认技能名称\n";
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
/// Hermes: agent 观察类记忆上限 2200 字符，超出截断 tail。
pub struct MemorySection;

const MEMORY_MD_MAX_CHARS: usize = 2200;

impl PromptSection for MemorySection {
    fn name(&self) -> &str {
        "memory"
    }

    fn render(&self, workspace: &AgentWorkspace) -> Option<String> {
        let raw = workspace.read("MEMORY.md").filter(|c| !c.trim().is_empty())?;
        let scan = super::threat_scan::scan("MEMORY.md", &raw);
        let content = &scan.cleaned;
        // 字符数（非字节）上限，超出保留尾部（最新追加通常在尾部）
        let trimmed = if content.chars().count() > MEMORY_MD_MAX_CHARS {
            let skip = content.chars().count() - MEMORY_MD_MAX_CHARS;
            let out: String = content.chars().skip(skip).collect();
            format!("[... {} 字符已截断 ...]\n{}", skip, out)
        } else {
            content.to_string()
        };
        Some(format!("# Long-Term Memory\n\n{}", trimmed))
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

/// 动态 Section — 从指定文件加载内容（插件/配置可注册）
///
/// 用于实现 Memory Plugin 可插拔的 system prompt section。
/// 通过 `DynamicSection::new("name", "FILE.md")` 创建。
pub struct DynamicSection {
    section_name: String,
    file_name: String,
    prefix: Option<String>,
}

impl DynamicSection {
    pub fn new(name: &str, file_name: &str) -> Self {
        Self {
            section_name: name.to_string(),
            file_name: file_name.to_string(),
            prefix: None,
        }
    }

    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.prefix = Some(prefix.to_string());
        self
    }
}

impl PromptSection for DynamicSection {
    fn name(&self) -> &str {
        &self.section_name
    }

    fn render(&self, workspace: &AgentWorkspace) -> Option<String> {
        let content = workspace.read(&self.file_name).filter(|c| !c.trim().is_empty())?;
        if let Some(ref prefix) = self.prefix {
            Some(format!("{}\n\n{}", prefix, content.trim()))
        } else {
            Some(content)
        }
    }
}

/// 内联 Section — 直接持有内容（不从文件读取）
///
/// 用于 BeforePromptBuild hook 注入的动态内容。
pub struct InlineSection {
    section_name: String,
    content: String,
}

impl InlineSection {
    pub fn new(name: &str, content: String) -> Self {
        Self { section_name: name.to_string(), content }
    }
}

impl PromptSection for InlineSection {
    fn name(&self) -> &str {
        &self.section_name
    }

    fn render(&self, _workspace: &AgentWorkspace) -> Option<String> {
        if self.content.trim().is_empty() {
            None
        } else {
            Some(self.content.clone())
        }
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
///
/// Hermes: 用户画像独立上限 1375 字符，与 MEMORY.md 分域。
pub struct UserSection;

const USER_MD_MAX_CHARS: usize = 1375;

impl PromptSection for UserSection {
    fn name(&self) -> &str {
        "user"
    }

    fn render(&self, workspace: &AgentWorkspace) -> Option<String> {
        let raw = workspace.read("USER.md").filter(|c| !c.trim().is_empty())?;
        let scan = super::threat_scan::scan("USER.md", &raw);
        let content = &scan.cleaned;
        // USER.md 是手编文件，重要内容通常在顶部（介绍/画像）。保留头部，截掉末尾。
        let trimmed = if content.chars().count() > USER_MD_MAX_CHARS {
            let head: String = content.chars().take(USER_MD_MAX_CHARS).collect();
            let cut = content.chars().count() - USER_MD_MAX_CHARS;
            format!("{}\n[... 末尾 {} 字符已截断 ...]", head, cut)
        } else {
            content.to_string()
        };
        Some(format!("# User Profile\n\n{}", trimmed))
    }
}

/// Standing Orders Section — 从 STANDING_ORDERS.md 读取常驻指令
///
/// 每次对话都会注入的规则/指令，独立于 SOUL.md。
/// 适合放置持久化的行为规则、格式要求等。
pub struct StandingOrdersSection;

impl PromptSection for StandingOrdersSection {
    fn name(&self) -> &str {
        "standing_orders"
    }

    fn render(&self, workspace: &AgentWorkspace) -> Option<String> {
        let content = workspace.read("STANDING_ORDERS.md").filter(|c| !c.trim().is_empty())?;
        Some(format!("## Standing Orders\n\n{}", content.trim()))
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
        // 只注入日期和时区（不注入具体时间），最大化 prompt cache 命中率
        // 参照 OpenClaw：时间通过 session_status 工具获取，不在 prompt 中频繁变化
        Some(format!(
            "# Current Date & Timezone\n\n- Date: {}\n- Timezone: {}\n- Use `datetime` tool to get precise current time if needed.",
            now.format("%Y-%m-%d"),
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
        let temp = std::env::temp_dir().join("xianzhu_soul_test");
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
        assert!(prompt.contains("Current Date"), "应包含 DateTime");

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
        let temp = std::env::temp_dir().join("xianzhu_soul_empty");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        // 只写 IDENTITY.md，其他文件不创建
        fs::write(temp.join("IDENTITY.md"), "# Identity\n- Name: Solo\n").unwrap();

        let ws = AgentWorkspace::from_path(temp.clone(), "test-empty");
        let engine = SoulEngine::with_defaults();
        let prompt = engine.build_system_prompt(&ws);

        // 应包含 Identity 和 DateTime（始终有内容）
        assert!(prompt.contains("Solo"));
        assert!(prompt.contains("Current Date"));

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

        let temp = std::env::temp_dir().join("xianzhu_soul_custom");
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
        assert_eq!(names.len(), 10);
        assert!(names.contains(&"identity"));
        assert!(names.contains(&"soul"));
        assert!(names.contains(&"strategy"));
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
        assert_eq!(names.len(), 6);
        assert!(names.contains(&"identity"));
        assert!(names.contains(&"soul"));
        assert!(names.contains(&"strategy"));
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
        assert_eq!(names.len(), 4);
        assert!(names.contains(&"identity"));
        assert!(names.contains(&"strategy"));
        assert!(names.contains(&"tools"));
        assert!(names.contains(&"datetime"));
        assert!(!names.contains(&"soul"));
        assert!(!names.contains(&"memory"));
    }
}
