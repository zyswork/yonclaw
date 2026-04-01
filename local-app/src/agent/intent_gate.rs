//! IntentGate — 用户意图分类
//!
//! 在 agent_loop 前对用户消息做轻量分类，不同意图给不同工具集。
//! 按 Codex 审查建议：
//! - 支持标签组合（不是互斥单类）
//! - 默认保守，允许升级
//! - 不依赖 LLM，纯关键词+规则

use std::collections::HashSet;

/// 意图标签
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Intent {
    /// 纯问答（不需要工具，或只需只读工具）
    Question,
    /// 代码修改（需要文件读写工具）
    CodeChange,
    /// 调研/搜索（需要搜索工具）
    Research,
    /// 危险操作（需要确认，如删除文件、执行命令）
    Dangerous,
}

/// 意图分类结果
#[derive(Debug, Clone)]
pub struct IntentResult {
    /// 识别到的意图标签（可多个）
    pub intents: HashSet<Intent>,
    /// 置信度（0.0-1.0）
    pub confidence: f64,
    /// 推荐的工具过滤策略
    pub tool_filter: ToolFilter,
}

/// 工具过滤策略
#[derive(Debug, Clone)]
pub enum ToolFilter {
    /// 所有工具可用
    All,
    /// 只读工具（file_read, file_list, code_search, web_search, web_fetch）
    ReadOnly,
    /// 读写工具（排除危险工具 bash_exec）
    ReadWrite,
    /// 需要用户确认后才启用完整工具集
    RequireConfirm,
}

/// 对用户消息进行意图分类
pub fn classify(message: &str) -> IntentResult {
    let msg = message.to_lowercase();
    let mut intents = HashSet::new();
    let mut danger_score = 0.0f64;

    // ── 问答检测 ─────────────────────────────
    let question_markers = ["是什么", "什么是", "怎么", "如何", "为什么", "why", "what", "how", "explain", "介绍", "解释", "区别"];
    let is_question = question_markers.iter().any(|m| msg.contains(m))
        || (msg.ends_with('?') || msg.ends_with('？'))
        || msg.chars().count() < 20; // 短消息多为问答
    if is_question { intents.insert(Intent::Question); }

    // ── 代码修改检测 ─────────────────────────
    let code_markers = ["修改", "修复", "添加", "删除代码", "重构", "实现", "创建文件", "写入",
        "fix", "add", "modify", "refactor", "implement", "create", "write", "update", "change",
        "编辑", "替换", "新建", "生成"];
    if code_markers.iter().any(|m| msg.contains(m)) {
        intents.insert(Intent::CodeChange);
    }

    // ── 调研检测 ──────────────────────────────
    let research_markers = ["搜索", "查找", "查看", "分析", "调研", "对比", "比较",
        "search", "find", "look", "research", "compare", "analyze", "read"];
    if research_markers.iter().any(|m| msg.contains(m)) {
        intents.insert(Intent::Research);
    }

    // ── 危险操作检测 ──────────────────────────
    let danger_markers = [
        ("删除", 0.7), ("remove", 0.5), ("delete", 0.7), ("rm ", 0.9), ("rm -", 0.95),
        ("drop", 0.6), ("清空", 0.7), ("格式化", 0.9), ("format", 0.5),
        ("sudo", 0.8), ("chmod", 0.6), ("kill", 0.5),
        ("数据库", 0.3), ("database", 0.3), ("deploy", 0.4), ("部署", 0.4),
        ("发布", 0.5), ("publish", 0.5), ("push", 0.3),
    ];
    for (marker, score) in &danger_markers {
        if msg.contains(marker) { danger_score += score; }
    }
    if danger_score > 0.5 {
        intents.insert(Intent::Dangerous);
    }

    // ── 如果没有匹配到任何意图，默认为代码修改 ─────
    if intents.is_empty() {
        intents.insert(Intent::CodeChange);
    }

    // ── 确定工具过滤策略 ─────────────────────
    // 参照 OpenClaw：默认给全部工具，让模型自己判断用哪个
    // 只有明确的危险操作才限制（如 "rm -rf /"）
    let tool_filter = if intents.contains(&Intent::Dangerous) && !intents.contains(&Intent::CodeChange) {
        ToolFilter::RequireConfirm
    } else {
        // Question/Research/CodeChange 都给全部工具
        // 用户说"下载并安装"时可能被分类为 Research，但实际需要写入权限
        ToolFilter::All
    };

    let confidence = if intents.len() == 1 && !is_question { 0.8 } else if intents.len() == 1 { 0.9 } else { 0.6 };

    IntentResult { intents, confidence, tool_filter }
}

/// 根据工具过滤策略过滤工具定义列表
pub fn filter_tools(tools: Vec<super::tools::ToolDefinition>, filter: &ToolFilter) -> Vec<super::tools::ToolDefinition> {
    match filter {
        ToolFilter::All => tools,
        ToolFilter::ReadOnly => {
            let readonly_tools: HashSet<&str> = [
                "file_read", "file_list", "code_search", "web_search", "web_fetch",
                "calculator", "datetime", "sessions_list", "sessions_history",
                "collaborate", "agent_chat",
            ].into_iter().collect();
            tools.into_iter().filter(|t| readonly_tools.contains(t.name.as_str())).collect()
        }
        ToolFilter::ReadWrite => {
            let denied: HashSet<&str> = ["bash_exec"].into_iter().collect();
            tools.into_iter().filter(|t| !denied.contains(t.name.as_str())).collect()
        }
        ToolFilter::RequireConfirm => {
            // 返回所有工具但加注释（实际确认由 ApprovalManager 处理）
            tools
        }
    }
}
