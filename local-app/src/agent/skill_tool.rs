//! 技能动态工具
//!
//! 将技能声明的工具转换为实现 Tool trait 的 SkillTool，
//! 通过 SandboxExecutor 在受限环境中执行。

use async_trait::async_trait;
use std::path::PathBuf;

use super::sandbox::{SandboxConfig, SandboxExecutor};
use super::skills::{SkillPermissions, SkillToolDecl, SkillToolExecutor};
use super::tools::{Tool, ToolDefinition, ToolSafetyLevel};

/// 技能动态工具
///
/// 桥接技能声明（SkillToolDecl）与沙箱执行（SandboxExecutor）
pub struct SkillTool {
    /// 工具声明
    decl: SkillToolDecl,
    /// 所属技能名称
    skill_name: String,
    /// 技能目录路径
    skill_dir: PathBuf,
    /// 沙箱配置（从技能权限构建）
    sandbox_config: SandboxConfig,
}

impl SkillTool {
    /// 创建技能工具
    ///
    /// 从技能权限声明构建沙箱配置
    pub fn new(
        decl: SkillToolDecl,
        skill_name: &str,
        skill_dir: PathBuf,
        permissions: &SkillPermissions,
    ) -> Self {
        let sandbox_config = Self::build_sandbox_config(permissions, &skill_dir);
        Self {
            decl,
            skill_name: skill_name.to_string(),
            skill_dir,
            sandbox_config,
        }
    }

    /// 获取完整工具名（skill_name-tool_name）
    /// 用连字符而非点号，兼容 OpenAI API 的 ^[a-zA-Z0-9_-]+$ 模式
    pub fn full_name(&self) -> String {
        format!("{}-{}", self.skill_name, self.decl.name)
    }

    /// 注入 Node.js 运行时 PATH 到沙箱配置
    pub fn inject_node_path(&mut self, node_bin_dir: &std::path::Path) {
        self.sandbox_config.inject_node_path(node_bin_dir);
    }

    /// 从技能权限构建沙箱配置
    fn build_sandbox_config(permissions: &SkillPermissions, skill_dir: &PathBuf) -> SandboxConfig {
        let mut allowed_paths: Vec<PathBuf> = permissions
            .read_paths
            .iter()
            .chain(permissions.write_paths.iter())
            .map(|p| {
                // 展开 ~ 为 home 目录
                if p.starts_with("~/") {
                    if let Some(home) = dirs::home_dir() {
                        return home.join(&p[2..]);
                    }
                }
                PathBuf::from(p)
            })
            .collect();

        // 始终允许访问技能自身目录
        allowed_paths.push(skill_dir.clone());

        // 构建默认 PATH：捆绑 Node + brew + 系统路径
        let mut env = std::collections::HashMap::new();
        let mut path_parts: Vec<String> = Vec::new();

        // 1. 捆绑的 Node 运行时
        if let Some(home) = dirs::home_dir() {
            let node_dir = home.join(".xianzhu/runtime/node");
            if node_dir.exists() {
                // 找最新版本的 Node
                if let Ok(entries) = std::fs::read_dir(&node_dir) {
                    let mut versions: Vec<_> = entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.path().is_dir() && e.file_name().to_string_lossy().starts_with("node-"))
                        .collect();
                    versions.sort_by(|a, b| b.file_name().cmp(&a.file_name())); // 最新版本在前
                    if let Some(latest) = versions.first() {
                        path_parts.push(latest.path().join("bin").to_string_lossy().to_string());
                    }
                }
            }
            // npm 全局安装路径
            path_parts.push(home.join(".npm-global/bin").to_string_lossy().to_string());
            // bun 路径
            path_parts.push(home.join(".bun/bin").to_string_lossy().to_string());
            path_parts.push(home.join(".local/bin").to_string_lossy().to_string());
        }

        // 2. brew 路径（macOS）
        path_parts.push("/opt/homebrew/bin".to_string());
        path_parts.push("/usr/local/bin".to_string());

        // 3. 系统基础路径
        path_parts.push("/usr/bin".to_string());
        path_parts.push("/bin".to_string());

        if !path_parts.is_empty() {
            env.insert("PATH".to_string(), path_parts.join(":"));
        }

        SandboxConfig {
            timeout_secs: 30,
            max_memory_mb: 256,
            allowed_paths,
            allowed_commands: permissions.exec_commands.clone(),
            network_allowed: permissions.network,
            env,
            working_dir: None,
        }
    }

    /// 模板替换：将 {param} 替换为实际参数值
    fn render_template(template: &str, arguments: &serde_json::Value) -> String {
        let mut result = template.to_string();
        if let Some(obj) = arguments.as_object() {
            for (key, value) in obj {
                let placeholder = format!("{{{}}}", key);
                let replacement = match value {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                result = result.replace(&placeholder, &replacement);
            }
        }
        result
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.full_name(),
            description: self.decl.description.clone(),
            parameters: self.decl.parameters.clone(),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        match self.decl.safety_level.as_str() {
            "safe" => ToolSafetyLevel::Safe,
            "guarded" => ToolSafetyLevel::Guarded,
            "sandboxed" => ToolSafetyLevel::Sandboxed,
            "approval" => ToolSafetyLevel::Approval,
            _ => ToolSafetyLevel::Sandboxed, // 默认沙箱
        }
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        log::info!("执行技能工具: {} 参数: {}", self.full_name(), arguments);

        match &self.decl.executor {
            SkillToolExecutor::Command { command, args_template } => {
                let rendered_cmd = Self::render_template(command, &arguments);

                let rendered_args: Vec<String> = args_template
                    .iter()
                    .map(|t| Self::render_template(t, &arguments))
                    .collect();

                // 展开 {args} 数组参数
                let mut final_args: Vec<String> = Vec::new();
                for arg in &rendered_args {
                    if arg == "{args}" {
                        // 从 arguments 中提取 args 数组
                        if let Some(arr) = arguments.get("args").and_then(|a| a.as_array()) {
                            for item in arr {
                                final_args.push(
                                    item.as_str().map(|s| s.to_string()).unwrap_or_else(|| item.to_string())
                                );
                            }
                        }
                    } else {
                        final_args.push(arg.clone());
                    }
                }

                // 设置工作目录为技能目录（解决相对路径问题）
                let mut config = self.sandbox_config.clone();
                config.working_dir = Some(self.skill_dir.clone());

                let arg_refs: Vec<&str> = final_args.iter().map(|s| s.as_str()).collect();
                SandboxExecutor::execute(&rendered_cmd, &arg_refs, &config).await
            }
            SkillToolExecutor::Script { path, interpreter } => {
                let script_path = self.skill_dir.join(path);
                let script_str = script_path.to_string_lossy().to_string();
                // 注入 action 字段（工具名），让脚本知道调用哪个子命令
                let mut args_with_action = arguments.clone();
                if let Some(obj) = args_with_action.as_object_mut() {
                    obj.entry("action").or_insert_with(|| serde_json::Value::String(self.decl.name.clone()));
                }
                let args_json = serde_json::to_string(&args_with_action)
                    .unwrap_or_else(|_| "{}".to_string());

                SandboxExecutor::execute(
                    interpreter,
                    &[&script_str, &args_json],
                    &self.sandbox_config,
                )
                .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::skills::SkillPermissions;

    #[test]
    fn test_full_name() {
        let tool = SkillTool::new(
            SkillToolDecl {
                name: "run_command".to_string(),
                description: "执行命令".to_string(),
                parameters: serde_json::json!({}),
                safety_level: "sandboxed".to_string(),
                executor: SkillToolExecutor::Command {
                    command: "echo".to_string(),
                    args_template: vec![],
                },
            },
            "shell_exec",
            PathBuf::from("/tmp/skills/shell_exec"),
            &SkillPermissions::default(),
        );
        assert_eq!(tool.full_name(), "shell_exec-run_command");
    }

    #[test]
    fn test_render_template() {
        let args = serde_json::json!({"command": "ls", "path": "/tmp"});
        assert_eq!(SkillTool::render_template("{command}", &args), "ls");
        assert_eq!(SkillTool::render_template("{command} {path}", &args), "ls /tmp");
        assert_eq!(SkillTool::render_template("no-placeholder", &args), "no-placeholder");
    }

    #[test]
    fn test_safety_level_mapping() {
        let make_tool = |level: &str| {
            SkillTool::new(
                SkillToolDecl {
                    name: "t".to_string(),
                    description: String::new(),
                    parameters: serde_json::json!({}),
                    safety_level: level.to_string(),
                    executor: SkillToolExecutor::default(),
                },
                "s",
                PathBuf::from("/tmp"),
                &SkillPermissions::default(),
            )
        };

        assert_eq!(make_tool("safe").safety_level(), ToolSafetyLevel::Safe);
        assert_eq!(make_tool("guarded").safety_level(), ToolSafetyLevel::Guarded);
        assert_eq!(make_tool("sandboxed").safety_level(), ToolSafetyLevel::Sandboxed);
        assert_eq!(make_tool("approval").safety_level(), ToolSafetyLevel::Approval);
        assert_eq!(make_tool("unknown").safety_level(), ToolSafetyLevel::Sandboxed);
    }

    #[test]
    fn test_sandbox_config_from_permissions() {
        let perms = SkillPermissions {
            read_paths: vec!["/tmp/read".to_string()],
            write_paths: vec!["/tmp/write".to_string()],
            exec_commands: vec!["ls".to_string(), "cat".to_string()],
            network: true,
        };
        let skill_dir = PathBuf::from("/tmp/skills/test");
        let tool = SkillTool::new(
            SkillToolDecl {
                name: "t".to_string(),
                description: String::new(),
                parameters: serde_json::json!({}),
                safety_level: "sandboxed".to_string(),
                executor: SkillToolExecutor::default(),
            },
            "test",
            skill_dir.clone(),
            &perms,
        );

        assert!(tool.sandbox_config.network_allowed);
        assert_eq!(tool.sandbox_config.allowed_commands, vec!["ls", "cat"]);
        assert!(tool.sandbox_config.allowed_paths.contains(&PathBuf::from("/tmp/read")));
        assert!(tool.sandbox_config.allowed_paths.contains(&PathBuf::from("/tmp/write")));
        assert!(tool.sandbox_config.allowed_paths.contains(&skill_dir));
    }

    #[tokio::test]
    async fn test_execute_command() {
        let tool = SkillTool::new(
            SkillToolDecl {
                name: "echo_tool".to_string(),
                description: "echo".to_string(),
                parameters: serde_json::json!({}),
                safety_level: "sandboxed".to_string(),
                executor: SkillToolExecutor::Command {
                    command: "echo".to_string(),
                    args_template: vec!["{message}".to_string()],
                },
            },
            "test",
            PathBuf::from("/tmp"),
            &SkillPermissions::default(),
        );

        let result = tool.execute(serde_json::json!({"message": "hello world"})).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().trim(), "hello world");
    }

    #[test]
    fn test_definition() {
        let tool = SkillTool::new(
            SkillToolDecl {
                name: "my_tool".to_string(),
                description: "我的工具".to_string(),
                parameters: serde_json::json!({"type": "object"}),
                safety_level: "safe".to_string(),
                executor: SkillToolExecutor::default(),
            },
            "my_skill",
            PathBuf::from("/tmp"),
            &SkillPermissions::default(),
        );

        let def = tool.definition();
        assert_eq!(def.name, "my_skill-my_tool");
        assert_eq!(def.description, "我的工具");
    }
}
