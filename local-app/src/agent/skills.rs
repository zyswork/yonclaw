//! 技能发现、安装与动态工具注册
//!
//! 扫描 skills/ 目录，读取 frontmatter 构建索引表，按需加载完整技能内容。
//! 支持扩展 frontmatter 格式：权限声明、工具定义、依赖要求。
//!
//! 技能文件格式（扩展版）：
//! ```yaml
//! ---
//! name: shell_exec
//! version: 0.1.0
//! description: 执行终端命令
//! trigger_keywords: [执行, 运行, shell]
//! permissions:
//!   read_paths: ["~/.xianzhu"]
//!   write_paths: ["~/.xianzhu/agents"]
//!   exec_commands: [ls, cat, grep]
//!   network: false
//! tools:
//!   - name: run_command
//!     description: 执行系统命令
//!     parameters: { "type": "object", "properties": { "command": { "type": "string" } }, "required": ["command"] }
//!     safety_level: sandboxed
//!     executor: { "type": "command", "command": "{command}", "args_template": ["{args}"] }
//! requires:
//!   bins: []
//!   env: []
//! ---
//! ```

use std::path::{Path, PathBuf};
use std::fs;
use serde::{Deserialize, Serialize};

// ─── 数据结构 ──────────────────────────────────────────────────

/// 技能索引条目（仅 frontmatter 基础字段，向后兼容）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillIndex {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub trigger_keywords: Vec<String>,
    /// 目录名（用于安装/卸载，区别于显示名 name）
    #[serde(default)]
    pub dir_name: String,
}

/// 完整技能清单（含权限/工具/依赖）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub trigger_keywords: Vec<String>,
    #[serde(default)]
    pub permissions: SkillPermissions,
    #[serde(default)]
    pub tools: Vec<SkillToolDecl>,
    #[serde(default)]
    pub requires: SkillRequirements,
}

/// 技能权限声明
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillPermissions {
    #[serde(default)]
    pub read_paths: Vec<String>,
    #[serde(default)]
    pub write_paths: Vec<String>,
    #[serde(default)]
    pub exec_commands: Vec<String>,
    #[serde(default)]
    pub network: bool,
}

/// 技能工具声明
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillToolDecl {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_parameters")]
    pub parameters: serde_json::Value,
    #[serde(default = "default_safety_level")]
    pub safety_level: String,
    #[serde(default)]
    pub executor: SkillToolExecutor,
}

fn default_parameters() -> serde_json::Value {
    serde_json::json!({"type": "object", "properties": {}})
}

fn default_safety_level() -> String {
    "sandboxed".to_string()
}

/// 工具执行器定义
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SkillToolExecutor {
    /// 命令执行器
    Command {
        command: String,
        #[serde(default)]
        args_template: Vec<String>,
    },
    /// 脚本执行器
    Script {
        path: String,
        #[serde(default = "default_interpreter")]
        interpreter: String,
    },
}

fn default_interpreter() -> String {
    "python3".to_string()
}

impl Default for SkillToolExecutor {
    fn default() -> Self {
        SkillToolExecutor::Command {
            command: String::new(),
            args_template: Vec::new(),
        }
    }
}

/// 技能依赖要求
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillRequirements {
    /// 需要的可执行文件
    #[serde(default)]
    pub bins: Vec<String>,
    /// 需要的环境变量
    #[serde(default)]
    pub env: Vec<String>,
}

impl SkillManifest {
    /// 从 SkillManifest 提取 SkillIndex（向后兼容）
    pub fn to_index(&self) -> SkillIndex {
        SkillIndex {
            name: self.name.clone(),
            description: self.description.clone(),
            trigger_keywords: self.trigger_keywords.clone(),
            dir_name: String::new(), // 由 scan() 填充
        }
    }

    /// 判断是否有工具声明（区分 prompt-only 和 tool-enabled 技能）
    pub fn has_tools(&self) -> bool {
        !self.tools.is_empty()
    }
}

// ─── SkillManager ──────────────────────────────────────────────

/// 技能管理器
#[derive(Clone)]
pub struct SkillManager {
    /// 技能索引列表
    index: Vec<SkillIndex>,
    /// 完整清单列表（含工具声明的技能）
    manifests: Vec<SkillManifest>,
    /// skills/ 目录路径
    skills_dir: PathBuf,
}

impl SkillManager {
    /// 扫描 skills/ 目录，读取所有技能的 frontmatter
    pub fn scan(skills_dir: &Path) -> Self {
        let mut index = Vec::new();
        let mut manifests = Vec::new();

        if !skills_dir.exists() {
            log::warn!("技能目录不存在: {:?}", skills_dir);
            return Self { index, manifests, skills_dir: skills_dir.to_path_buf() };
        }

        let entries = match fs::read_dir(skills_dir) {
            Ok(e) => e,
            Err(err) => {
                log::error!("读取技能目录失败: {}", err);
                return Self { index, manifests, skills_dir: skills_dir.to_path_buf() };
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();

            // 支持两种结构：
            // 1. skills/skill-name.md（单文件）
            // 2. skills/skill-name/SKILL.md（目录）
            let skill_md_path = if path.is_dir() {
                let md = path.join("SKILL.md");
                if md.exists() { md } else { continue; }
            } else if path.extension().map_or(true, |ext| ext != "md") {
                continue;
            } else {
                path.clone()
            };

            if let Some(manifest) = Self::parse_manifest(&skill_md_path) {
                // 获取目录名（文件系统名，非 SKILL.md 中的显示名）
                let dir_name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&manifest.name)
                    .to_string();
                log::info!("发现技能: {} (dir={}) - {} (工具: {})", manifest.name, dir_name, manifest.description, manifest.tools.len());
                let mut idx = manifest.to_index();
                idx.dir_name = dir_name;
                index.push(idx);
                manifests.push(manifest);
            }
        }

        index.sort_by(|a, b| a.name.cmp(&b.name));
        manifests.sort_by(|a, b| a.name.cmp(&b.name));
        log::info!("技能索引加载完成: {} 个技能", index.len());

        Self { index, manifests, skills_dir: skills_dir.to_path_buf() }
    }

    /// 获取技能索引
    pub fn index(&self) -> &[SkillIndex] {
        &self.index
    }

    /// 获取所有清单
    pub fn manifests(&self) -> &[SkillManifest] {
        &self.manifests
    }

    /// 获取 skills 目录路径
    pub fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }

    /// 渲染技能索引表（注入 system prompt）
    pub fn render_index(&self) -> Option<String> {
        if self.index.is_empty() {
            return None;
        }
        let mut lines = vec!["# Available Skills".to_string(), String::new()];
        lines.push("| Skill | Description |".to_string());
        lines.push("|-------|-------------|".to_string());
        for skill in &self.index {
            lines.push(format!("| {} | {} |", skill.name, skill.description));
        }
        lines.push(String::new());
        lines.push("Use `read_skill(name)` to load full skill instructions.".to_string());
        Some(lines.join("\n"))
    }

    /// 加载完整技能内容（去除 frontmatter）
    pub fn load_full(&self, name: &str) -> Option<String> {
        let skill = self.index.iter().find(|s| s.name == name)?;
        let path = self.skills_dir.join(format!("{}.md", skill.name));
        let content = fs::read_to_string(&path).ok()?;
        Self::strip_frontmatter(&content)
    }

    /// 根据关键词匹配技能
    pub fn match_keywords(&self, text: &str) -> Vec<&SkillIndex> {
        let text_lower = text.to_lowercase();
        self.index
            .iter()
            .filter(|s| {
                s.trigger_keywords
                    .iter()
                    .any(|kw| text_lower.contains(&kw.to_lowercase()))
            })
            .collect()
    }

    /// 根据用户消息激活技能，返回有工具声明的匹配清单
    pub fn activate_for_message(&self, text: &str) -> Vec<&SkillManifest> {
        let text_lower = text.to_lowercase();
        self.manifests
            .iter()
            .filter(|m| {
                m.has_tools()
                    && m.trigger_keywords
                        .iter()
                        .any(|kw| text_lower.contains(&kw.to_lowercase()))
            })
            .collect()
    }

    /// 根据用户消息激活 prompt-only 技能（有关键词匹配但无工具声明）
    /// 返回 (技能名, 技能内容) 列表，用于注入 system prompt
    pub fn activate_prompt_skills(&self, text: &str) -> Vec<(String, String)> {
        let text_lower = text.to_lowercase();
        self.manifests
            .iter()
            .filter(|m| {
                !m.has_tools()
                    && !m.trigger_keywords.is_empty()
                    && m.trigger_keywords
                        .iter()
                        .any(|kw| text_lower.contains(&kw.to_lowercase()))
            })
            .filter_map(|m| {
                // 尝试两种路径：skill-name/SKILL.md 或 skill-name.md
                let dir_path = self.skills_dir.join(&m.name).join("SKILL.md");
                let file_path = self.skills_dir.join(format!("{}.md", m.name));
                let content = fs::read_to_string(&dir_path)
                    .or_else(|_| fs::read_to_string(&file_path))
                    .ok()?;
                let body = Self::strip_frontmatter(&content)?;
                if body.trim().is_empty() { return None; }
                log::info!("激活 prompt-only 技能: {} ({}字符)", m.name, body.len());
                Some((m.name.clone(), body))
            })
            .collect()
    }

    /// 按名称获取清单
    pub fn get_manifest(&self, name: &str) -> Option<&SkillManifest> {
        self.manifests.iter().find(|m| m.name == name)
    }

    // ─── 安装/移除 ──────────────────────────────────────────────

    /// 从文件安装技能
    pub async fn install_from_file(
        &mut self,
        path: &Path,
        agent_id: &str,
        pool: &sqlx::SqlitePool,
    ) -> Result<SkillManifest, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("读取技能文件失败: {}", e))?;
        self.install_from_content(&content, agent_id, pool).await
    }

    /// 从字符串内容安装技能
    pub async fn install_from_content(
        &mut self,
        content: &str,
        agent_id: &str,
        pool: &sqlx::SqlitePool,
    ) -> Result<SkillManifest, String> {
        let manifest = Self::parse_manifest_from_str(content)
            .ok_or("无法解析技能 frontmatter")?;

        Self::validate_manifest(&manifest)?;
        Self::check_requirements(&manifest.requires)?;

        // 写入 skills/ 目录
        let skill_path = self.skills_dir.join(format!("{}.md", manifest.name));
        fs::create_dir_all(&self.skills_dir)
            .map_err(|e| format!("创建技能目录失败: {}", e))?;
        fs::write(&skill_path, content)
            .map_err(|e| format!("写入技能文件失败: {}", e))?;

        // 写入数据库
        let manifest_json = serde_json::to_string(&manifest)
            .map_err(|e| format!("序列化 manifest 失败: {}", e))?;
        let id = uuid::Uuid::new_v4().to_string();

        sqlx::query(
            "INSERT OR REPLACE INTO installed_skills (id, agent_id, name, version, manifest_json, source, enabled)
             VALUES (?, ?, ?, ?, ?, ?, 1)"
        )
        .bind(&id)
        .bind(agent_id)
        .bind(&manifest.name)
        .bind(&manifest.version)
        .bind(&manifest_json)
        .bind(skill_path.to_string_lossy().as_ref())
        .execute(pool)
        .await
        .map_err(|e| format!("写入数据库失败: {}", e))?;

        // 更新内存索引
        self.manifests.retain(|m| m.name != manifest.name);
        self.index.retain(|i| i.name != manifest.name);
        self.index.push(manifest.to_index());
        self.manifests.push(manifest.clone());

        log::info!("技能已安装: {} v{}", manifest.name, manifest.version);
        Ok(manifest)
    }

    /// 移除已安装的技能
    pub async fn remove_skill(
        &mut self,
        name: &str,
        agent_id: &str,
        pool: &sqlx::SqlitePool,
    ) -> Result<(), String> {
        // 删除文件
        let skill_path = self.skills_dir.join(format!("{}.md", name));
        if skill_path.exists() {
            fs::remove_file(&skill_path)
                .map_err(|e| format!("删除技能文件失败: {}", e))?;
        }

        // 删除数据库记录
        sqlx::query("DELETE FROM installed_skills WHERE agent_id = ? AND name = ?")
            .bind(agent_id)
            .bind(name)
            .execute(pool)
            .await
            .map_err(|e| format!("删除数据库记录失败: {}", e))?;

        // 更新内存索引
        self.manifests.retain(|m| m.name != name);
        self.index.retain(|i| i.name != name);

        log::info!("技能已移除: {}", name);
        Ok(())
    }

    /// 从数据库恢复已安装的技能
    pub async fn load_installed(
        &mut self,
        agent_id: &str,
        pool: &sqlx::SqlitePool,
    ) -> Result<(), String> {
        let rows: Vec<(String, i32)> = sqlx::query_as(
            "SELECT manifest_json, enabled FROM installed_skills WHERE agent_id = ? AND enabled = 1"
        )
        .bind(agent_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("查询已安装技能失败: {}", e))?;

        for (json_str, _enabled) in rows {
            if let Ok(manifest) = serde_json::from_str::<SkillManifest>(&json_str) {
                if !self.manifests.iter().any(|m| m.name == manifest.name) {
                    self.index.push(manifest.to_index());
                    self.manifests.push(manifest);
                }
            }
        }

        Ok(())
    }

    /// 切换技能启用状态
    pub async fn toggle_skill(
        name: &str,
        agent_id: &str,
        enabled: bool,
        pool: &sqlx::SqlitePool,
    ) -> Result<(), String> {
        sqlx::query("UPDATE installed_skills SET enabled = ? WHERE agent_id = ? AND name = ?")
            .bind(enabled as i32)
            .bind(agent_id)
            .bind(name)
            .execute(pool)
            .await
            .map_err(|e| format!("更新技能状态失败: {}", e))?;
        Ok(())
    }

    /// 列出已安装的技能
    pub async fn list_installed(
        agent_id: &str,
        pool: &sqlx::SqlitePool,
    ) -> Result<Vec<serde_json::Value>, String> {
        let rows: Vec<(String, String, String, String, i32, String)> = sqlx::query_as(
            "SELECT id, name, version, manifest_json, enabled, installed_at FROM installed_skills WHERE agent_id = ?"
        )
        .bind(agent_id)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("查询已安装技能失败: {}", e))?;

        let mut result = Vec::new();
        for (id, name, version, manifest_json, enabled, installed_at) in rows {
            let manifest: serde_json::Value = serde_json::from_str(&manifest_json).unwrap_or_default();
            result.push(serde_json::json!({
                "id": id,
                "name": name,
                "version": version,
                "enabled": enabled == 1,
                "installed_at": installed_at,
                "tools_count": manifest.get("tools").and_then(|t| t.as_array()).map_or(0, |a| a.len()),
                "description": manifest.get("description").and_then(|d| d.as_str()).unwrap_or(""),
            }));
        }
        Ok(result)
    }

    // ─── 验证 ──────────────────────────────────────────────────

    /// 验证技能清单完整性
    pub fn validate_manifest(manifest: &SkillManifest) -> Result<(), String> {
        if manifest.name.is_empty() {
            return Err("技能名称不能为空".to_string());
        }
        // 名称只允许字母数字下划线连字符
        if !manifest.name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            return Err(format!("技能名称包含非法字符: {}", manifest.name));
        }
        // 验证工具声明
        for tool in &manifest.tools {
            if tool.name.is_empty() {
                return Err("工具名称不能为空".to_string());
            }
            if !tool.name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Err(format!("工具名称包含非法字符: {}", tool.name));
            }
        }
        Ok(())
    }

    /// 检查依赖要求是否满足
    pub fn check_requirements(requires: &SkillRequirements) -> Result<(), String> {
        // 检查可执行文件（用 `which` 命令代替 which crate）
        for bin in &requires.bins {
            let found = std::process::Command::new("which")
                .arg(bin)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !found {
                // 降级为警告而非错误，允许安装但运行时可能失败
                log::warn!("技能依赖的可执行文件未找到: {}", bin);
            }
        }
        // 检查环境变量
        for env_var in &requires.env {
            if std::env::var(env_var).is_err() {
                log::warn!("技能依赖的环境变量未设置: {}", env_var);
            }
        }
        Ok(())
    }

    // ─── 内部工具函数 ──────────────────────────────────────────

    /// 解析 frontmatter 为完整 SkillManifest（使用 serde_yaml）
    fn parse_manifest(path: &Path) -> Option<SkillManifest> {
        let content = fs::read_to_string(path).ok()?;
        Self::parse_manifest_from_str(&content)
    }

    /// 从字符串解析 SkillManifest
    ///
    /// 支持两种格式：
    /// 1. YAML frontmatter: `---\nname: ...\n---`
    /// 2. 纯 Markdown: 从 `# Title` 推断 name，第一段作为 description
    fn parse_manifest_from_str(content: &str) -> Option<SkillManifest> {
        let trimmed = content.trim();

        // 格式 1: YAML frontmatter
        if trimmed.starts_with("---") {
            let rest = &trimmed[3..];
            let end = rest.find("---")?;
            let yaml_str = rest[..end].trim();
            return serde_yaml::from_str::<SkillManifest>(yaml_str).ok();
        }

        // 格式 2: 纯 Markdown — 从标题和首段推断
        let mut name = String::new();
        let mut description = String::new();

        for line in trimmed.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }
            if name.is_empty() && line.starts_with("# ") {
                name = line.trim_start_matches("# ").trim().to_string();
                continue;
            }
            if !name.is_empty() && description.is_empty() && !line.starts_with('#') {
                description = line.to_string();
                break;
            }
        }

        if name.is_empty() {
            return None;
        }

        // 将标题转为 kebab-case 作为 name（中文保留原样）
        let slug = name.to_lowercase()
            .replace(|c: char| c.is_whitespace(), "-")
            .replace(|c: char| !c.is_alphanumeric() && c != '-', "");

        Some(SkillManifest {
            name: slug,
            version: String::new(),
            description,
            trigger_keywords: Vec::new(),
            tools: Vec::new(),
            permissions: SkillPermissions::default(),
            requires: SkillRequirements::default(),
        })
    }

    /// 去除 frontmatter，返回正文
    pub fn strip_frontmatter(content: &str) -> Option<String> {
        let trimmed = content.trim();
        if !trimmed.starts_with("---") {
            return Some(content.to_string());
        }
        let rest = &trimmed[3..];
        let end = rest.find("---")?;
        let body = &rest[end + 3..];
        Some(body.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter_legacy() {
        // 旧格式（3 字段）向后兼容
        let temp = std::env::temp_dir().join("xianzhu_skill_test_legacy");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        fs::write(
            temp.join("calculator.md"),
            "---\nname: calculator\ndescription: 数学计算\ntrigger_keywords: [计算, math, calculate]\n---\n\n# Calculator\n\n执行数学运算。\n",
        ).unwrap();

        let manager = SkillManager::scan(&temp);
        assert_eq!(manager.index().len(), 1);

        let calc = &manager.index()[0];
        assert_eq!(calc.name, "calculator");
        assert_eq!(calc.description, "数学计算");
        assert_eq!(calc.trigger_keywords, vec!["计算", "math", "calculate"]);

        // manifest 也应该被解析
        let manifest = manager.get_manifest("calculator").unwrap();
        assert!(manifest.tools.is_empty());
        assert!(!manifest.has_tools());

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_parse_extended_manifest() {
        // 扩展格式（含 tools/permissions）
        let temp = std::env::temp_dir().join("xianzhu_skill_test_ext");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let content = r#"---
name: shell_exec
version: 0.1.0
description: 执行终端命令
trigger_keywords: [执行, 运行, shell]
permissions:
  read_paths: ["~/.xianzhu"]
  write_paths: ["~/.xianzhu/agents"]
  exec_commands: [ls, cat, grep]
  network: false
tools:
  - name: run_command
    description: 执行系统命令
    parameters: {"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]}
    safety_level: sandboxed
    executor: {"type": "command", "command": "{command}", "args_template": ["{args}"]}
requires:
  bins: [ls]
  env: []
---

# Shell Executor

允许在沙箱中执行预定义的安全命令。
"#;
        fs::write(temp.join("shell_exec.md"), content).unwrap();

        let manager = SkillManager::scan(&temp);
        assert_eq!(manager.index().len(), 1);

        let manifest = manager.get_manifest("shell_exec").unwrap();
        assert_eq!(manifest.version, "0.1.0");
        assert!(manifest.has_tools());
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools[0].name, "run_command");
        assert_eq!(manifest.permissions.exec_commands, vec!["ls", "cat", "grep"]);
        assert!(!manifest.permissions.network);

        // activate_for_message 应该匹配
        let active = manager.activate_for_message("帮我执行一个命令");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "shell_exec");

        // 不匹配的消息
        let active = manager.activate_for_message("今天天气怎么样");
        assert!(active.is_empty());

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_match_keywords() {
        let temp = std::env::temp_dir().join("xianzhu_skill_kw");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        fs::write(
            temp.join("calculator.md"),
            "---\nname: calculator\ndescription: 数学计算\ntrigger_keywords: [计算, math]\n---\nBody\n",
        ).unwrap();

        let manager = SkillManager::scan(&temp);
        let matches = manager.match_keywords("帮我计算一下");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].name, "calculator");

        let matches = manager.match_keywords("no match here");
        assert!(matches.is_empty());

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_empty_skills_dir() {
        let temp = std::env::temp_dir().join("xianzhu_skill_empty2");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let manager = SkillManager::scan(&temp);
        assert!(manager.index().is_empty());
        assert!(manager.render_index().is_none());

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_nonexistent_skills_dir() {
        let manager = SkillManager::scan(Path::new("/tmp/nonexistent_skills_12345"));
        assert!(manager.index().is_empty());
    }

    #[test]
    fn test_strip_frontmatter() {
        let content = "---\nname: test\n---\n\nBody content here.";
        let body = SkillManager::strip_frontmatter(content).unwrap();
        assert_eq!(body, "Body content here.");

        let plain = "Just plain text.";
        let body = SkillManager::strip_frontmatter(plain).unwrap();
        assert_eq!(body, "Just plain text.");
    }

    #[test]
    fn test_validate_manifest() {
        // 有效清单
        let valid = SkillManifest {
            name: "test_skill".to_string(),
            version: "0.1.0".to_string(),
            description: "测试".to_string(),
            trigger_keywords: vec![],
            permissions: SkillPermissions::default(),
            tools: vec![SkillToolDecl {
                name: "my_tool".to_string(),
                description: "工具".to_string(),
                parameters: serde_json::json!({}),
                safety_level: "sandboxed".to_string(),
                executor: SkillToolExecutor::default(),
            }],
            requires: SkillRequirements::default(),
        };
        assert!(SkillManager::validate_manifest(&valid).is_ok());

        // 空名称
        let mut invalid = valid.clone();
        invalid.name = String::new();
        assert!(SkillManager::validate_manifest(&invalid).is_err());

        // 非法字符
        let mut invalid = valid.clone();
        invalid.name = "bad name!".to_string();
        assert!(SkillManager::validate_manifest(&invalid).is_err());

        // 工具名非法
        let mut invalid = valid;
        invalid.tools[0].name = "bad.tool".to_string();
        assert!(SkillManager::validate_manifest(&invalid).is_err());
    }

    #[test]
    fn test_render_index() {
        let temp = std::env::temp_dir().join("xianzhu_skill_render");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        fs::write(
            temp.join("calc.md"),
            "---\nname: calc\ndescription: 计算器\ntrigger_keywords: []\n---\nBody\n",
        ).unwrap();

        let manager = SkillManager::scan(&temp);
        let rendered = manager.render_index().unwrap();
        assert!(rendered.contains("calc"));
        assert!(rendered.contains("计算器"));
        assert!(rendered.contains("read_skill"));

        let _ = fs::remove_dir_all(&temp);
    }
}
