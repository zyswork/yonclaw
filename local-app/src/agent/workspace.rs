//! Agent 工作区管理
//!
//! 负责在文件系统中创建和管理 Agent 的灵魂文件工作区。
//! 每个 Agent 拥有独立的工作区目录，包含人格、记忆、技能等配置文件。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
use std::collections::HashMap;

/// 文件缓存条目
struct CacheEntry {
    content: String,
    mtime: SystemTime,
}

/// 全局文件缓存：PathBuf -> (content, mtime)
///
/// 通过 mtime 判断文件是否变更，避免每次请求重复读取灵魂文件。
fn file_cache() -> &'static Mutex<HashMap<PathBuf, CacheEntry>> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<PathBuf, CacheEntry>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Agent 工作区
///
/// 管理 `~/.xianzhu/agents/{agent_id}/` 下的灵魂文件体系
pub struct AgentWorkspace {
    /// 工作区根目录
    root: PathBuf,
    /// Agent ID
    agent_id: String,
}

/// 灵魂文件类型
#[derive(Clone)]
pub enum SoulFile {
    Soul,
    Identity,
    Agents,
    User,
    Tools,
    Memory,
    Bootstrap,
    Heartbeat,
}

impl SoulFile {
    /// 获取文件名
    pub fn filename(&self) -> &str {
        match self {
            SoulFile::Soul => "SOUL.md",
            SoulFile::Identity => "IDENTITY.md",
            SoulFile::Agents => "AGENTS.md",
            SoulFile::User => "USER.md",
            SoulFile::Tools => "TOOLS.md",
            SoulFile::Memory => "MEMORY.md",
            SoulFile::Bootstrap => "BOOTSTRAP.md",
            SoulFile::Heartbeat => "HEARTBEAT.md",
        }
    }

    /// 从文件名解析
    pub fn from_str(name: &str) -> Option<Self> {
        match name {
            "SOUL.md" => Some(SoulFile::Soul),
            "IDENTITY.md" => Some(SoulFile::Identity),
            "AGENTS.md" => Some(SoulFile::Agents),
            "USER.md" => Some(SoulFile::User),
            "TOOLS.md" => Some(SoulFile::Tools),
            "MEMORY.md" => Some(SoulFile::Memory),
            "BOOTSTRAP.md" => Some(SoulFile::Bootstrap),
            "HEARTBEAT.md" => Some(SoulFile::Heartbeat),
            _ => None,
        }
    }

    /// 返回所有灵魂文件类型
    pub fn all() -> Vec<SoulFile> {
        vec![
            SoulFile::Soul,
            SoulFile::Identity,
            SoulFile::Agents,
            SoulFile::User,
            SoulFile::Tools,
            SoulFile::Memory,
            SoulFile::Bootstrap,
            SoulFile::Heartbeat,
        ]
    }
}

impl AgentWorkspace {
    /// 创建工作区实例（不创建目录）
    pub fn new(agent_id: &str) -> Self {
        let root = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".xianzhu")
            .join("agents")
            .join(agent_id);
        Self {
            root,
            agent_id: agent_id.to_string(),
        }
    }

    /// 从指定路径创建工作区实例
    pub fn from_path(path: PathBuf, agent_id: &str) -> Self {
        Self {
            root: path,
            agent_id: agent_id.to_string(),
        }
    }

    /// 获取工作区根目录
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 获取 Agent ID
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// 初始化工作区：创建目录并生成模板文件
    pub async fn initialize(&self, agent_name: &str) -> Result<(), String> {
        // 创建目录结构
        std::fs::create_dir_all(&self.root)
            .map_err(|e| format!("创建工作区目录失败: {}", e))?;
        std::fs::create_dir_all(self.root.join("memory"))
            .map_err(|e| format!("创建 memory 目录失败: {}", e))?;
        std::fs::create_dir_all(self.root.join("skills"))
            .map_err(|e| format!("创建 skills 目录失败: {}", e))?;

        // 生成模板文件（仅在文件不存在时）
        self.write_if_absent("IDENTITY.md", &Self::identity_template(agent_name))?;
        self.write_if_absent("SOUL.md", &Self::soul_template(agent_name))?;
        self.write_if_absent("AGENTS.md", &Self::agents_template())?;
        self.write_if_absent("USER.md", &Self::user_template())?;
        self.write_if_absent("TOOLS.md", "")?;
        self.write_if_absent("MEMORY.md", "")?;
        self.write_if_absent("HEARTBEAT.md", "")?;
        self.write_if_absent("BOOTSTRAP.md", &Self::bootstrap_template(agent_name))?;

        log::info!("Agent 工作区已初始化: {}", self.root.display());
        Ok(())
    }

    /// 读取灵魂文件内容
    pub fn read_file(&self, soul_file: &SoulFile) -> Option<String> {
        let path = self.root.join(soul_file.filename());
        std::fs::read_to_string(&path).ok()
    }

    /// 读取指定文件名的内容（带 mtime 缓存）
    ///
    /// 先检查缓存中的 mtime，命中则返回缓存内容；
    /// 未命中或 mtime 变更则读取文件并更新缓存。
    pub fn read(&self, filename: &str) -> Option<String> {
        let path = self.root.join(filename);

        // 获取文件 metadata
        let metadata = std::fs::metadata(&path).ok()?;
        let mtime = metadata.modified().ok()?;

        // 检查缓存
        {
            let cache = file_cache().lock().ok()?;
            if let Some(entry) = cache.get(&path) {
                if entry.mtime == mtime {
                    return Some(entry.content.clone());
                }
            }
        }

        // 缓存未命中，读取文件
        let content = std::fs::read_to_string(&path).ok()?;

        // 更新缓存
        if let Ok(mut cache) = file_cache().lock() {
            cache.insert(
                path,
                CacheEntry {
                    content: content.clone(),
                    mtime,
                },
            );
        }

        Some(content)
    }

    /// 清除指定 agent 的文件缓存
    ///
    /// 在写入灵魂文件后调用，确保下次读取获取最新内容。
    pub fn invalidate_cache(&self) {
        if let Ok(mut cache) = file_cache().lock() {
            cache.retain(|path, _| !path.starts_with(&self.root));
        }
    }

    /// 写入灵魂文件（写入后自动清除缓存）
    pub fn write_file(&self, soul_file: &SoulFile, content: &str) -> Result<(), String> {
        let path = self.root.join(soul_file.filename());
        std::fs::write(&path, content)
            .map_err(|e| format!("写入 {} 失败: {}", soul_file.filename(), e))?;
        // 清除该文件的缓存
        if let Ok(mut cache) = file_cache().lock() {
            cache.remove(&path);
        }
        Ok(())
    }

    /// 检查工作区是否存在
    pub fn exists(&self) -> bool {
        self.root.exists()
    }

    /// 检查 BOOTSTRAP.md 是否存在（首次启动标识）
    pub fn has_bootstrap(&self) -> bool {
        let path = self.root.join("BOOTSTRAP.md");
        path.exists() && std::fs::read_to_string(&path).map_or(false, |c| !c.trim().is_empty())
    }

    /// 删除 BOOTSTRAP.md（首次引导完成后调用）
    pub fn remove_bootstrap(&self) -> Result<(), String> {
        let path = self.root.join("BOOTSTRAP.md");
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("删除 BOOTSTRAP.md 失败: {}", e))?;
        }
        Ok(())
    }

    /// 列出每日记忆文件
    pub fn list_daily_memories(&self) -> Vec<String> {
        let memory_dir = self.root.join("memory");
        if !memory_dir.exists() {
            return Vec::new();
        }
        let mut files: Vec<String> = std::fs::read_dir(&memory_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.path()
                            .extension()
                            .map_or(false, |ext| ext == "md")
                    })
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        files
    }

    // ─── 内部工具函数 ──────────────────────────────────────────

    /// 仅在文件不存在时写入
    fn write_if_absent(&self, filename: &str, content: &str) -> Result<(), String> {
        let path = self.root.join(filename);
        if !path.exists() {
            std::fs::write(&path, content)
                .map_err(|e| format!("写入 {} 失败: {}", filename, e))?;
        }
        Ok(())
    }

    // ─── 模板生成 ──────────────────────────────────────────────

    fn identity_template(name: &str) -> String {
        format!(
            r#"# Identity

- **Name**: {}
- **Creature**: AI Assistant
- **Vibe**: Helpful, thoughtful, precise
- **Emoji**: 🤖
"#,
            name
        )
    }

    fn soul_template(name: &str) -> String {
        format!(
            r#"# Soul

## Core Values
- Be genuinely helpful, not performatively helpful
- Prioritize accuracy over speed
- Respect user boundaries and privacy
- Be transparent about limitations

## Personality
{} is a thoughtful assistant that takes time to understand context before responding. Prefers clear, concise communication.

## Communication Style
- Use the language the user is speaking
- Be direct and specific
- Acknowledge uncertainty honestly
- Ask clarifying questions when needed
"#,
            name
        )
    }

    fn agents_template() -> String {
        r#"# Behavioral Guidelines

## Red Lines (Non-Negotiable)
- Never reveal API keys, passwords, or sensitive credentials
- Never pretend to be a human
- Never generate harmful, illegal, or deceptive content
- Never access systems without explicit authorization

## External Actions
- Always confirm before executing any action that affects external systems
- Log all tool usage for audit trail
- Prefer read-only operations unless write is explicitly requested

## Memory Maintenance
- Regularly curate MEMORY.md to keep it relevant
- Archive outdated information to daily memory logs
- Never store sensitive user data in plain text
"#
        .to_string()
    }

    fn user_template() -> String {
        r#"# User Profile

> This file is updated as the agent learns about the user through conversation.

- **Name**: (unknown)
- **Timezone**: (unknown)
- **Preferences**: (to be discovered)
"#
        .to_string()
    }

    fn bootstrap_template(name: &str) -> String {
        format!(
            r#"# Bootstrap Guide

> This file is used for the first conversation with a new agent.
> It will be deleted after the initial setup is complete.

## First Conversation Goals
1. Introduce {} to the user
2. Learn the user's name and preferences
3. Understand the primary use case
4. Set communication style preferences

## Suggested Opening
"Hi! I'm {}. I'd love to get to know you a bit so I can be more helpful. What should I call you, and what will we mainly be working on together?"
"#,
            name, name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn test_workspace_initialize() {
        let temp_dir = std::env::temp_dir().join("xianzhu_test_workspace");
        let _ = fs::remove_dir_all(&temp_dir);

        let ws = AgentWorkspace::from_path(temp_dir.clone(), "test-agent-1");
        ws.initialize("TestBot").await.unwrap();

        // 验证目录结构
        assert!(temp_dir.exists());
        assert!(temp_dir.join("memory").exists());
        assert!(temp_dir.join("skills").exists());

        // 验证灵魂文件
        assert!(temp_dir.join("SOUL.md").exists());
        assert!(temp_dir.join("IDENTITY.md").exists());
        assert!(temp_dir.join("AGENTS.md").exists());
        assert!(temp_dir.join("USER.md").exists());
        assert!(temp_dir.join("TOOLS.md").exists());
        assert!(temp_dir.join("MEMORY.md").exists());
        assert!(temp_dir.join("HEARTBEAT.md").exists());
        assert!(temp_dir.join("BOOTSTRAP.md").exists());

        // 验证内容
        let identity = fs::read_to_string(temp_dir.join("IDENTITY.md")).unwrap();
        assert!(identity.contains("TestBot"));

        let soul = fs::read_to_string(temp_dir.join("SOUL.md")).unwrap();
        assert!(soul.contains("genuinely helpful"));

        // 清理
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_read_write_files() {
        let temp_dir = std::env::temp_dir().join("xianzhu_test_rw");
        let _ = fs::remove_dir_all(&temp_dir);

        let ws = AgentWorkspace::from_path(temp_dir.clone(), "test-agent-2");
        ws.initialize("RWBot").await.unwrap();

        // 读取文件
        let content = ws.read_file(&SoulFile::Soul);
        assert!(content.is_some());
        assert!(content.unwrap().contains("RWBot"));

        // 写入文件
        ws.write_file(&SoulFile::User, "# Updated User\nName: Alice")
            .unwrap();
        let updated = ws.read_file(&SoulFile::User).unwrap();
        assert!(updated.contains("Alice"));

        // 清理
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_bootstrap_lifecycle() {
        let temp_dir = std::env::temp_dir().join("xianzhu_test_bootstrap");
        let _ = fs::remove_dir_all(&temp_dir);

        let ws = AgentWorkspace::from_path(temp_dir.clone(), "test-agent-3");
        ws.initialize("BootBot").await.unwrap();

        assert!(ws.has_bootstrap());

        ws.remove_bootstrap().unwrap();
        assert!(!ws.has_bootstrap());

        // 清理
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_workspace_not_exists() {
        let ws = AgentWorkspace::from_path(PathBuf::from("/tmp/nonexistent_ws_12345"), "fake");
        assert!(!ws.exists());
    }
}
