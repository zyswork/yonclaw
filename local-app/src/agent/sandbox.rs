//! 沙箱执行器
//!
//! 在受限环境中执行命令，提供超时、路径白名单、命令白名单等安全控制

use std::path::{Path, PathBuf};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

/// 沙箱配置
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// 超时时间（秒）
    pub timeout_secs: u64,
    /// 最大内存（MB，仅用于日志警告，macOS 不支持 cgroup）
    pub max_memory_mb: u64,
    /// 允许访问的路径列表
    pub allowed_paths: Vec<PathBuf>,
    /// 允许执行的命令列表（空 = 允许所有）
    pub allowed_commands: Vec<String>,
    /// 是否允许网络访问
    pub network_allowed: bool,
    /// 额外环境变量（注入子进程）
    pub env: std::collections::HashMap<String, String>,
    /// 工作目录（为空则继承当前进程的工作目录）
    pub working_dir: Option<PathBuf>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            max_memory_mb: 256,
            allowed_paths: Vec::new(),
            allowed_commands: Vec::new(),
            network_allowed: false,
            env: std::collections::HashMap::new(),
            working_dir: None,
        }
    }
}

impl SandboxConfig {
    /// 创建仅允许指定工作区路径的配置
    pub fn for_workspace(workspace_path: PathBuf) -> Self {
        Self {
            allowed_paths: vec![workspace_path],
            ..Default::default()
        }
    }

    /// 注入 Node.js 运行时 PATH
    ///
    /// 将 node bin 目录加入 env["PATH"]，让子进程能找到 node/npm
    pub fn inject_node_path(&mut self, node_bin_dir: &Path) {
        let bin_str = node_bin_dir.to_string_lossy().to_string();
        self.env
            .entry("PATH".to_string())
            .and_modify(|existing| {
                // 如已有 PATH，前置 node 目录
                *existing = format!("{}:{}", bin_str, existing);
            })
            .or_insert(bin_str);
    }
}

/// 沙箱执行器
pub struct SandboxExecutor;

impl SandboxExecutor {
    /// 在沙箱中执行命令
    ///
    /// 检查命令白名单和路径白名单后，使用 tokio 子进程执行
    pub async fn execute(
        command: &str,
        args: &[&str],
        config: &SandboxConfig,
    ) -> Result<String, String> {
        // 1. 命令白名单检查
        if !config.allowed_commands.is_empty()
            && !config.allowed_commands.iter().any(|c| c == command)
        {
            return Err(format!("命令不在白名单中: {}", command));
        }

        // 2. 路径白名单检查（resolve symlink 后检查）
        if !config.allowed_paths.is_empty() {
            for arg in args {
                let path = PathBuf::from(arg);
                if path.is_absolute() {
                    // 安全: resolve symlinks 再检查边界
                    let resolved = if path.exists() {
                        path.canonicalize().unwrap_or_else(|_| path.clone())
                    } else {
                        path.clone()
                    };

                    // 安全: 逐段检查 symlink（防止中间段逃逸）
                    if path.exists() {
                        let mut cursor = PathBuf::new();
                        for component in path.components() {
                            cursor.push(component);
                            if cursor.exists() {
                                if let Ok(meta) = std::fs::symlink_metadata(&cursor) {
                                    if meta.is_symlink() {
                                        let target = std::fs::read_link(&cursor).unwrap_or_default();
                                        log::warn!("沙箱: 路径段 {} 是 symlink → {:?}", cursor.display(), target);
                                    }
                                }
                            }
                        }
                    }

                    let allowed = config.allowed_paths.iter().any(|ap| {
                        let ap_resolved = ap.canonicalize().unwrap_or_else(|_| ap.clone());
                        resolved.starts_with(&ap_resolved)
                    });
                    if !allowed {
                        return Err(format!("路径不在白名单中（解析后: {}）: {}", resolved.display(), arg));
                    }
                }
            }
        }

        // 3. 构建并执行命令
        log::info!("沙箱执行: {} {:?}", command, args);

        let mut cmd = Command::new(command);
        cmd.args(args);

        // 设置工作目录
        if let Some(ref cwd) = config.working_dir {
            cmd.current_dir(cwd);
        }

        // 注入额外环境变量
        for (key, value) in &config.env {
            if key == "PATH" {
                // PATH 特殊处理：前置到系统 PATH 前面
                let sys_path = std::env::var("PATH").unwrap_or_default();
                cmd.env("PATH", format!("{}:{}", value, sys_path));
            } else {
                cmd.env(key, value);
            }
        }

        let output = timeout(
            Duration::from_secs(config.timeout_secs),
            cmd.output(),
        )
        .await
        .map_err(|_| format!("命令执行超时（{}秒）", config.timeout_secs))?
        .map_err(|e| format!("命令执行失败: {}", e))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("命令返回错误 ({}): {}", output.status, stderr))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sandbox_execute_basic() {
        let config = SandboxConfig::default();
        let result = SandboxExecutor::execute("echo", &["hello"], &config).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().trim(), "hello");
    }

    #[tokio::test]
    async fn test_sandbox_timeout() {
        let config = SandboxConfig {
            timeout_secs: 1,
            ..Default::default()
        };
        let result = SandboxExecutor::execute("sleep", &["10"], &config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("超时"));
    }

    #[tokio::test]
    async fn test_sandbox_command_whitelist_reject() {
        let config = SandboxConfig {
            allowed_commands: vec!["echo".to_string()],
            ..Default::default()
        };
        let result = SandboxExecutor::execute("rm", &["-rf", "/"], &config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("白名单"));
    }

    #[tokio::test]
    async fn test_sandbox_command_whitelist_allow() {
        let config = SandboxConfig {
            allowed_commands: vec!["echo".to_string()],
            ..Default::default()
        };
        let result = SandboxExecutor::execute("echo", &["ok"], &config).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_sandbox_path_whitelist_reject() {
        let config = SandboxConfig {
            allowed_paths: vec![PathBuf::from("/tmp/safe")],
            ..Default::default()
        };
        let result = SandboxExecutor::execute("cat", &["/etc/passwd"], &config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("白名单"));
    }

    #[tokio::test]
    async fn test_sandbox_path_whitelist_allow() {
        let config = SandboxConfig {
            allowed_paths: vec![PathBuf::from("/tmp")],
            ..Default::default()
        };
        // 相对路径不受白名单限制
        let result = SandboxExecutor::execute("echo", &["test"], &config).await;
        assert!(result.is_ok());
    }
}

// ---------------------------------------------------------------------------
// 安全守卫：PathGuard / ShellGuard / EnvSanitizer
// ---------------------------------------------------------------------------

/// 路径边界守卫
///
/// 验证路径是否在允许的工作区范围内
pub struct PathGuard {
    /// 允许的根路径列表
    allowed_roots: Vec<PathBuf>,
}

impl PathGuard {
    pub fn new(allowed_roots: Vec<PathBuf>) -> Self {
        Self { allowed_roots }
    }

    /// 从 Agent 工作区路径创建
    pub fn for_agent(workspace_path: &str) -> Self {
        Self {
            allowed_roots: vec![PathBuf::from(workspace_path)],
        }
    }

    /// 验证路径是否安全
    ///
    /// 检查：
    /// 1. 规范化路径（解析 .. 和 symlink）
    /// 2. 确保在允许的根路径下
    /// 3. 拒绝 symlink 逃逸
    pub fn validate_path(&self, path: &str) -> Result<PathBuf, String> {
        let path = Path::new(path);

        // 规范化为绝对路径
        let canonical = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| format!("获取当前目录失败: {}", e))?
                .join(path)
        };

        // 尝试解析 symlink（如果路径存在）
        let resolved = if canonical.exists() {
            canonical
                .canonicalize()
                .map_err(|e| format!("路径解析失败: {}", e))?
        } else {
            // 路径不存在时，检查父目录
            if let Some(parent) = canonical.parent() {
                if parent.exists() {
                    let resolved_parent = parent
                        .canonicalize()
                        .map_err(|e| format!("父目录解析失败: {}", e))?;
                    resolved_parent.join(canonical.file_name().unwrap_or_default())
                } else {
                    canonical.clone()
                }
            } else {
                canonical.clone()
            }
        };

        // 如果没有设置允许的根路径，允许所有
        if self.allowed_roots.is_empty() {
            return Ok(resolved);
        }

        // 检查是否在允许的根路径下
        for root in &self.allowed_roots {
            if resolved.starts_with(root) {
                return Ok(resolved);
            }
        }

        Err(format!(
            "路径 {} 不在允许的工作区范围内（允许: {:?}）",
            resolved.display(),
            self.allowed_roots
        ))
    }
}

/// 危险 token 列表
///
/// 策略：shell 语法本身不是威胁，高危命令才是（见 DANGEROUS_COMMANDS）。
/// LLM 生成的多行脚本、变量引用、命令替换都是正常的。
/// 仅拦截明确的注入模式（当前为空，依赖 DANGEROUS_COMMANDS 黑名单）。
const DANGEROUS_TOKENS: &[&str] = &[
    // 故意留空：shell 语法由 DANGEROUS_COMMANDS 黑名单保护
    // 如需更严格的策略，可添加回 "`", "$(" 等
];

/// 高危命令列表
const DANGEROUS_COMMANDS: &[&str] = &[
    "rm -rf /", "rm -rf ~", "rm -rf *",
    "mkfs", "dd if=", "format ",
    "> /dev/sd", "chmod -R 777",
    "eval ", "exec ", "source ", ". ",
    "sudo ", "su ", "doas ", "pkexec ",
    "nc ", "ncat ", "socat ",
    "base64 -d", "base64 --decode",
];

/// Shell 命令安全守卫
pub struct ShellGuard;

/// Safe-bin 白名单（参照 OpenClaw exec-safe-bin-semantics）
/// 这些命令仅读取/查看数据，不修改系统状态，无需用户审批
const SAFE_BINS: &[&str] = &[
    "ls", "cat", "head", "tail", "wc", "sort", "uniq", "grep", "rg",
    "find", "which", "whoami", "date", "echo", "printf", "env", "printenv",
    "pwd", "basename", "dirname", "realpath", "file", "stat", "du", "df",
    "uname", "hostname", "id", "uptime", "ps", "top",
    "jq", "xargs", "tr", "cut", "awk", "sed",  // sed 只读不写时是安全的
    "diff", "md5", "shasum", "sha256sum",
    "python3 --version", "python --version", "node --version", "npm --version",
    "cargo --version", "rustc --version", "git status", "git log", "git diff", "git branch",
];

impl ShellGuard {
    /// 判断命令是否为低风险 safe-bin（无需审批）
    pub fn is_safe_command(command: &str) -> bool {
        let trimmed = command.trim();
        let first_cmd = trimmed.split_whitespace().next().unwrap_or("");
        let first_cmd_base = first_cmd.rsplit('/').next().unwrap_or(first_cmd);

        // 检查完整命令前缀匹配
        for safe in SAFE_BINS {
            if trimmed.starts_with(safe) { return true; }
        }
        // 检查命令名匹配（忽略路径前缀）
        for safe in &SAFE_BINS[..29] { // 前 29 个是单命令名
            if first_cmd_base == *safe { return true; }
        }
        false
    }

    /// 检查命令是否安全
    ///
    /// 返回 Ok(()) 如果安全，Err(reason) 如果危险
    pub fn validate_command(command: &str) -> Result<(), String> {
        let lower = command.to_lowercase().trim().to_string();

        // 检查高危命令
        for dangerous in DANGEROUS_COMMANDS {
            if lower.contains(dangerous) {
                return Err(format!("检测到高危命令: {}", dangerous));
            }
        }

        // 检查危险 token（仅在非引号内检查）
        // 简化版：检查是否包含多命令链接
        let unquoted = Self::strip_quotes(&lower);
        for token in DANGEROUS_TOKENS {
            if *token == ">" || *token == "<" {
                // 重定向需要更精确的检查
                if unquoted.contains(" > ")
                    || unquoted.contains(" >> ")
                    || unquoted.contains(" < ")
                {
                    return Err(format!("检测到 I/O 重定向: {}", token));
                }
            } else if unquoted.contains(token) {
                return Err(format!("检测到危险 token: {}", token));
            }
        }

        Ok(())
    }

    fn strip_quotes(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    // 反斜杠转义：跳过下一字符，直接追加
                    if let Some(next) = chars.next() {
                        result.push(next);
                    }
                }
                '\'' => {
                    // 单引号内容：直到下一个未转义的单引号
                    while let Some(inner) = chars.next() {
                        if inner == '\'' { break; }
                        result.push(inner);
                    }
                }
                '"' => {
                    // 双引号内容：支持反斜杠转义
                    while let Some(inner) = chars.next() {
                        if inner == '\\' {
                            if let Some(escaped) = chars.next() {
                                result.push(escaped);
                            }
                        } else if inner == '"' {
                            break;
                        } else {
                            result.push(inner);
                        }
                    }
                }
                _ => result.push(c),
            }
        }
        result
    }
}

/// 敏感环境变量后缀
const SENSITIVE_SUFFIXES: &[&str] = &[
    "_KEY",
    "_SECRET",
    "_TOKEN",
    "_PASSWORD",
    "_PASSWD",
    "_CREDENTIAL",
    "_API_KEY",
    "_APIKEY",
    "_AUTH",
];

/// 敏感环境变量名
const SENSITIVE_NAMES: &[&str] = &[
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "GITHUB_TOKEN",
    "DATABASE_URL",
    "REDIS_URL",
    "MONGO_URI",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "SSH_AUTH_SOCK",
    "GPG_AGENT_INFO",
];

/// 环境变量清洗器
pub struct EnvSanitizer;

impl EnvSanitizer {
    /// 获取清洗后的环境变量
    ///
    /// 移除所有敏感变量，返回安全的环境变量 map
    pub fn sanitized_env() -> std::collections::HashMap<String, String> {
        std::env::vars()
            .filter(|(key, _)| !Self::is_sensitive(key))
            .collect()
    }

    /// 检查环境变量名是否敏感
    pub fn is_sensitive(key: &str) -> bool {
        let upper = key.to_uppercase();

        // 精确匹配
        if SENSITIVE_NAMES.contains(&upper.as_str()) {
            return true;
        }

        // 后缀匹配
        for suffix in SENSITIVE_SUFFIXES {
            if upper.ends_with(suffix) {
                return true;
            }
        }

        false
    }
}
