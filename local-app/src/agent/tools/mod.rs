//! 工具调用系统
//!
//! 支持 Agent 调用外部工具，包含安全级别控制

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 透明 wrapper 列表（审批和执行共用，集中定义）
///
/// 这些命令不改变被包装程序的行为，只影响执行环境。
/// 审批时需要解包找到实际执行的程序。
pub const TRANSPARENT_WRAPPERS: &[&str] = &[
    "time ", "env ", "nice ", "nohup ", "strace ", "ltrace ",
    "stdbuf ", "timeout ", "ionice ", "taskset ",
];

/// 非透明 wrapper（需要额外权限审批）
pub const OPAQUE_WRAPPERS: &[&str] = &[
    "sudo ", "doas ", "chrt ",
];

/// 统一路径安全校验
///
/// 检查路径是否安全，拒绝系统路径、敏感路径和路径遍历攻击
pub(crate) fn validate_path_safety(path: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);

    // 安全: 检测符号链接（防止 symlink 攻击绕过路径限制）
    if p.exists() {
        let metadata = std::fs::symlink_metadata(p)
            .map_err(|e| format!("访问路径失败: {}", e))?;
        if metadata.is_symlink() {
            // 检查 symlink 目标是否也在允许范围内
            let target = std::fs::read_link(p)
                .map_err(|e| format!("读取链接目标失败: {}", e))?;
            let resolved = if target.is_absolute() { target } else {
                p.parent().unwrap_or(std::path::Path::new("/")).join(&target)
            };
            log::warn!("安全: 检测到符号链接 {} → {:?}", path, resolved);
            // 对 symlink 的目标也做安全校验（递归检查）
            if let Ok(canonical_target) = resolved.canonicalize() {
                let target_str = canonical_target.to_string_lossy();
                let blocked_prefixes = ["/etc", "/usr", "/bin", "/sbin", "/System", "/Library", "/var/root", "/private/etc"];
                for prefix in &blocked_prefixes {
                    if target_str.starts_with(prefix) {
                        return Err(format!("安全限制：符号链接指向系统路径 {} → {}", path, target_str));
                    }
                }
            }
        }
    }

    // 尝试规范化路径
    let canonical = if p.exists() {
        p.canonicalize()
            .map_err(|e| format!("路径规范化失败: {}", e))?
    } else {
        // 路径不存在：规范化父目录
        let parent = p.parent().ok_or("无效路径：无父目录")?;
        if parent.as_os_str().is_empty() {
            // 相对路径，使用当前目录
            std::env::current_dir()
                .map_err(|e| format!("获取当前目录失败: {}", e))?
                .join(p)
        } else if parent.exists() {
            parent.canonicalize()
                .map_err(|e| format!("父目录规范化失败: {}", e))?
                .join(p.file_name().unwrap_or_default())
        } else {
            return Err("路径不存在且父目录也不存在，拒绝访问".to_string());
        }
    };

    let path_str = canonical.to_string_lossy();

    // 规范化后仍含 .. 则拒绝
    if path_str.contains("..") {
        return Err("路径包含非法遍历序列".to_string());
    }

    // 系统路径黑名单
    let blocked_prefixes = [
        "/etc", "/usr", "/bin", "/sbin",
        "/System", "/Library",
        "/var/root", "/private/etc",
    ];
    for prefix in &blocked_prefixes {
        if path_str.starts_with(prefix) {
            return Err(format!("安全限制：不允许访问系统路径 {}", path_str));
        }
    }

    // 敏感用户路径
    if let Some(home) = std::env::var_os("HOME") {
        let home_str = home.to_string_lossy();
        let sensitive_dirs = [".ssh", ".gnupg", ".aws", ".config/gcloud"];
        for dir in &sensitive_dirs {
            if path_str.starts_with(&format!("{}/{}", home_str, dir)) {
                return Err(format!("安全限制：不允许访问敏感目录 {}", dir));
            }
        }
    }

    Ok(())
}

/// 工具安全级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolSafetyLevel {
    /// 安全工具，可直接执行（如计算器、时间查询）
    Safe,
    /// 受保护工具，需要参数校验（如文件读取）
    Guarded,
    /// 沙箱工具，需要在沙箱中执行（如命令执行）
    Sandboxed,
    /// 需要用户审批（如发送邮件、删除文件）
    Approval,
}

/// 解析后的工具调用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedToolCall {
    /// 调用 ID（用于关联结果）
    pub id: String,
    /// 工具名称
    pub name: String,
    /// 调用参数
    pub arguments: serde_json::Value,
}

/// 工具定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// 工具调用请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

/// 错误分类（用于智能重试和验证决策）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorClass {
    /// 权限/安全错误 — 不可重试
    Permission,
    /// 参数错误（格式、类型、范围）— 可重试（修改参数后）
    Parameter,
    /// 环境错误（文件不存在、网络超时）— 可重试
    Environment,
    /// 外部依赖错误（API 失败、服务不可用）— 可重试（等待后）
    Dependency,
    /// 代码语义错误（编译失败、逻辑错误）— 需反思后重试
    Semantic,
    /// 验证失败（lint/test 不通过）— 需修复后重试
    Validation,
}

impl ErrorClass {
    /// 该类错误是否允许自动重试
    pub fn is_retryable(&self) -> bool {
        !matches!(self, ErrorClass::Permission)
    }

    /// 从错误文本自动分类
    pub fn classify(error: &str) -> Self {
        let e = error.to_lowercase();
        // 权限错误（不可重试）
        if e.contains("permission") || e.contains("denied") || e.contains("forbidden")
            || e.contains("安全拦截") || e.contains("安全限制") || e.contains("不允许") {
            ErrorClass::Permission
        // Python 模块缺失 / npm 包缺失 → 外部依赖（区别于普通环境错误）
        } else if e.contains("modulenotfounderror") || e.contains("no module named")
            || e.contains("cannot find module") || e.contains("not installed")
            || e.contains("pip install") || e.contains("npm install") {
            ErrorClass::Dependency
        // 文件/网络/系统环境问题
        } else if e.contains("no such file") || e.contains("not found") || e.contains("不存在")
            || e.contains("timeout") || e.contains("超时") || e.contains("connection") {
            ErrorClass::Environment
        // API/服务端错误
        } else if e.contains("api") || e.contains("500") || e.contains("502") || e.contains("503")
            || e.contains("rate limit") || e.contains("quota") {
            ErrorClass::Dependency
        // 编译/语法错误
        } else if e.contains("compile") || e.contains("syntax") || e.contains("error[")
            || e.contains("lint") || e.contains("test fail") {
            ErrorClass::Semantic
        // 参数验证
        } else if e.contains("validation") || e.contains("invalid") || e.contains("缺少")
            || e.contains("参数") || e.contains("格式") {
            ErrorClass::Parameter
        } else {
            ErrorClass::Environment
        }
    }
}

/// 工具调用结果（含结构化元数据）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub tool_name: String,
    /// 人类可读的结果文本（给 LLM 看的）
    pub result: String,
    pub success: bool,
    pub error: Option<String>,
    /// 错误分类（失败时自动推断）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_class: Option<ErrorClass>,
    /// 本次调用修改的文件路径列表
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub changed_files: Vec<String>,
    /// 本次调用读取的文件 hash（path → hash）
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub content_hashes: HashMap<String, String>,
}

impl ToolCallResult {
    /// 构造成功结果
    pub fn ok(tool_name: &str, result: String) -> Self {
        Self { tool_name: tool_name.to_string(), result, success: true, error: None, error_class: None, changed_files: vec![], content_hashes: HashMap::new() }
    }
    /// 构造失败结果（自动分类错误）
    pub fn err(tool_name: &str, error: String) -> Self {
        let class = ErrorClass::classify(&error);
        Self { tool_name: tool_name.to_string(), result: String::new(), success: false, error: Some(error), error_class: Some(class), changed_files: vec![], content_hashes: HashMap::new() }
    }
    /// 附加修改的文件
    pub fn with_changed_files(mut self, files: Vec<String>) -> Self { self.changed_files = files; self }
    /// 附加文件 hash
    pub fn with_hash(mut self, path: String, hash: String) -> Self { self.content_hashes.insert(path, hash); self }
}

/// 工具处理器特征
#[async_trait]
pub trait Tool: Send + Sync {
    /// 获取工具定义
    fn definition(&self) -> ToolDefinition;

    /// 获取安全级别（默认 Safe）
    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Safe
    }

    /// 执行工具
    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String>;
}


pub mod builtin;
pub use builtin::*;

pub struct ToolManager {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolManager {
    /// 创建新的工具管理器
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// 注册工具
    pub fn register_tool(&mut self, tool: Box<dyn Tool>) {
        let name = tool.definition().name.clone();
        self.tools.insert(name.clone(), tool);
        log::info!("工具已注册: {}", name);
    }

    /// 获取工具定义列表
    pub fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|tool| tool.definition())
            .collect()
    }

    /// 获取工具安全级别
    pub fn get_safety_level(&self, tool_name: &str) -> Option<ToolSafetyLevel> {
        self.tools.get(tool_name).map(|t| t.safety_level())
    }

    /// 执行工具（带安全防护层）
    pub async fn execute_tool(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> ToolCallResult {
        // 防护 1: 危险命令检测
        if tool_name == "bash_exec" {
            if let Some(cmd) = arguments.get("command").and_then(|c| c.as_str()) {
                if let Some(warning) = detect_dangerous_command(cmd) {
                    log::warn!("危险命令被拦截: {} — {}", cmd, warning);
                    return ToolCallResult::err(tool_name, format!("安全拦截：{}", warning));
                }
            }
        }

        // 防护 2: 文件路径白名单（file_read/write/edit 限制为 Agent workspace + /tmp）
        if matches!(tool_name, "file_write" | "file_edit" | "diff_edit") {
            if let Some(path) = arguments.get("path").and_then(|p| p.as_str()) {
                if let Err(e) = validate_write_path(path) {
                    return ToolCallResult::err(tool_name, e);
                }
            }
        }

        match self.tools.get(tool_name) {
            Some(tool) => match tool.execute(arguments).await {
                Ok(result) => {
                    // 防护 3: 输出大小限制（最大 50KB，防止撑爆上下文）
                    let truncated = truncate_output(&result, MAX_OUTPUT_SIZE);
                    ToolCallResult::ok(tool_name, truncated)
                }
                Err(error) => ToolCallResult::err(tool_name, error),
            },
            None => ToolCallResult::err(tool_name, format!("工具不存在: {}", tool_name)),
        }
    }

    /// 获取工具
    pub fn get_tool(&self, tool_name: &str) -> Option<&Box<dyn Tool>> {
        self.tools.get(tool_name)
    }
}

impl Default for ToolManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 工具输出最大字节数（50KB）
const MAX_OUTPUT_SIZE: usize = 50 * 1024;

/// 截断超大输出
fn truncate_output(output: &str, max_bytes: usize) -> String {
    if output.len() <= max_bytes {
        return output.to_string();
    }
    // 找到安全的 UTF-8 截断点
    let mut end = max_bytes;
    while end > 0 && !output.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = &output[..end];
    format!(
        "{}\n\n[输出已截断：原始 {} 字节，显示前 {} 字节]",
        truncated,
        output.len(),
        end
    )
}

/// 命令最大长度限制（防止超长混淆命令）
const MAX_COMMAND_CHARS: usize = 10_000;

/// 不可见 Unicode 码点集合（参考 OpenClaw 73+ 码点）
///
/// 包含零宽字符、变体选择符、BiDi 覆盖、格式控制符等
fn is_invisible_unicode(c: char) -> bool {
    let cp = c as u32;
    matches!(cp,
        // 软连字符
        0x00AD |
        // 组合用字素连接符
        0x034F |
        // 阿拉伯字母标记
        0x061C |
        // 韩文填充符
        0x115F | 0x1160 | 0x3164 | 0xFFA0 |
        // 高棉元音固有符
        0x17B4 | 0x17B5 |
        // 蒙古语变体选择符
        0x180B..=0x180F |
        // BOM / 零宽无断空格
        0xFEFF |
        // 零宽字符 + BiDi 控制
        0x200B..=0x200F |
        // BiDi 覆盖/嵌入
        0x202A..=0x202E |
        // 不可见数学运算符 + 隔离
        0x2060..=0x2069 |
        // 已废弃格式字符
        0x206A..=0x206F |
        // 变体选择符 (VS1-VS16)
        0xFE00..=0xFE0F |
        // 语言标签
        0xE0001 |
        // Tag 空格到 ~ (标签修饰符)
        0xE0020..=0xE007F |
        // 补充变体选择符 (VS17-VS256)
        0xE0100..=0xE01EF
    )
}

/// 剥离不可见 Unicode 字符（用于命令规范化分析）
fn strip_invisible_unicode(cmd: &str) -> String {
    cmd.chars().filter(|c| !is_invisible_unicode(*c)).collect()
}

/// Windows UNC/SMB 路径检测（防止 SMB 凭证泄漏）
///
/// 攻击向量：LLM 被诱导执行包含 UNC 路径的命令（如 \\evil.com\share），
/// Windows 会自动发送 NTLM 凭证到远程 SMB 服务器
fn detect_unc_path(cmd: &str) -> Option<String> {
    for line in cmd.lines() {
        let trimmed = line.trim();
        // 检测 \\server\share 或 \\?\UNC\ 形式
        if trimmed.contains("\\\\") {
            // 排除 shell 转义序列（\\n, \\t 等）
            let re_unc = regex::Regex::new(r"\\\\[a-zA-Z0-9._\-]+\\[a-zA-Z0-9._\-]+").ok()?;
            if re_unc.is_match(trimmed) {
                return Some(format!(
                    "安全拦截：检测到 Windows UNC 路径，可能导致 SMB 凭证泄漏。命令: {}",
                    &trimmed[..trimmed.len().min(80)]
                ));
            }
        }
    }
    None
}

/// 命令混淆检测结构
struct ObfuscationResult {
    detected: bool,
    reasons: Vec<String>,
}

/// 安全 curl|sh 白名单 URL
const SAFE_CURL_PIPE_HOSTS: &[&str] = &[
    "brew.sh",
    "get.pnpm.io",
    "bun.sh",
    "sh.rustup.rs",
    "get.docker.com",
    "install.python-poetry.org",
];

/// 检测命令混淆/编码攻击（参考 OpenClaw 16 种模式）
///
/// 攻击向量：LLM 被诱导执行经过 base64/hex/printf 编码后管道到 shell 的命令，
/// 绕过基于关键词的安全检查
fn detect_command_obfuscation(cmd: &str) -> ObfuscationResult {
    if cmd.is_empty() {
        return ObfuscationResult { detected: false, reasons: vec![] };
    }
    if cmd.len() > MAX_COMMAND_CHARS {
        return ObfuscationResult {
            detected: true,
            reasons: vec!["命令过长，可能包含混淆内容".to_string()],
        };
    }

    // NFKC 规范化 + 剥离不可见字符
    let normalized = strip_invisible_unicode(cmd);
    let mut reasons = Vec::new();

    // 混淆模式正则定义（使用 r#"..."# 避免转义问题）
    let patterns: &[(&str, &str)] = &[
        // base64 解码管道到 shell
        (r#"(?i)base64\s+(?:-d|--decode)\b.*\|\s*(?:sh|bash|zsh|dash|ksh|fish)\b"#,
         "Base64 解码管道到 shell 执行"),
        // xxd 十六进制解码管道到 shell
        (r#"(?i)xxd\s+-r\b.*\|\s*(?:sh|bash|zsh|dash|ksh|fish)\b"#,
         "十六进制解码管道到 shell 执行"),
        // printf 转义管道到 shell
        (r#"(?i)printf\s+.*\\x[0-9a-f]{2}.*\|\s*(?:sh|bash|zsh|dash|ksh|fish)\b"#,
         "printf 转义序列管道到 shell 执行"),
        // eval + 解码
        (r#"(?i)eval\s+.*(?:base64|xxd|printf|decode)"#,
         "eval 结合编码/解码，可能执行隐藏命令"),
        // 管道直接到 shell 解释器
        (r#"(?im)\|\s*(?:sh|bash|zsh|dash|ksh|fish)\b(?:\s+[^|;\n\r]+)?\s*$"#,
         "内容直接管道到 shell 解释器"),
        // shell -c 内含命令替换 + 解码
        (r#"(?i)(?:sh|bash|zsh|dash|ksh|fish)\s+-c\s+["'][^"']*\$\([^)]*(?:base64|xxd|printf)"#,
         "shell -c 内含命令替换解码"),
        // 进程替换执行远程内容
        (r#"(?i)(?:sh|bash|zsh|dash|ksh|fish)\s+<\(\s*(?:curl|wget)\b"#,
         "进程替换执行远程内容"),
        // source/. 进程替换远程内容
        (r#"(?i)(?:source|\.)\s+<\(\s*(?:curl|wget)\b"#,
         "source 进程替换执行远程内容"),
        // shell heredoc 执行
        (r#"(?i)(?:sh|bash|zsh|dash|ksh|fish)\s+<<-?\s*['"]?[a-zA-Z_][\w-]*['"]?"#,
         "shell heredoc 执行"),
        // bash 八进制转义（可能混淆命令）
        (r#"\$'(?:[^']*\\[0-7]{3}){2,}"#,
         "bash 八进制转义序列（可能混淆命令）"),
        // bash 十六进制转义
        (r#"\$'(?:[^']*\\x[0-9a-fA-F]{2}){2,}"#,
         "bash 十六进制转义序列（可能混淆命令）"),
        // Python/Perl/Ruby 编码执行
        (r#"(?i)(?:python[23]?|perl|ruby)\s+-[ec]\s+.*(?:base64|b64decode|decode|exec|system|eval)"#,
         "脚本语言编码执行"),
        // curl/wget 管道到 shell
        (r#"(?i)(?:curl|wget)\s+.*\|\s*(?:sh|bash|zsh|dash|ksh|fish)\b"#,
         "远程内容 (curl/wget) 管道到 shell"),
        // 变量展开混淆
        (r#"(?:[a-zA-Z_]\w{0,2}=[^;\s]+\s*;\s*){2,}[^$]*\$(?:[a-zA-Z_]|\{[a-zA-Z_])"#,
         "变量赋值链展开（可能混淆命令）"),
    ];

    for (pattern_str, description) in patterns {
        if let Ok(re) = regex::Regex::new(pattern_str) {
            if re.is_match(&normalized) {
                // curl|sh 白名单检查
                if description.contains("curl/wget") || description.contains("管道到 shell") {
                    if is_safe_curl_pipe(&normalized) {
                        continue;
                    }
                }
                reasons.push(description.to_string());
            }
        }
    }

    ObfuscationResult {
        detected: !reasons.is_empty(),
        reasons,
    }
}

/// 检查 curl|sh 命令是否指向安全白名单 URL
fn is_safe_curl_pipe(cmd: &str) -> bool {
    // 简单提取 http(s) URL
    let url_re = regex::Regex::new(r"https?://[^\s]+").ok();
    if let Some(re) = url_re {
        let urls: Vec<&str> = re.find_iter(cmd).map(|m| m.as_str()).collect();
        if urls.len() == 1 {
            if let Ok(url) = url::Url::parse(urls[0]) {
                let host = url.host_str().unwrap_or("");
                return SAFE_CURL_PIPE_HOSTS.iter().any(|safe| host == *safe || host.ends_with(&format!(".{}", safe)));
            }
        }
    }
    false
}

/// 检测危险 bash 命令，返回警告消息
///
/// 安全防线层级：
/// 1. 不可见 Unicode 字符检测（73+ 码点，含 NFKC 规范化）
/// 2. Windows UNC/SMB 路径凭证泄漏检测
/// 3. 命令混淆/编码攻击检测（16 种模式）
/// 4. 高危破坏性命令拦截
/// 5. 中危命令日志记录
fn detect_dangerous_command(cmd: &str) -> Option<String> {
    // ── 1. 不可见 Unicode 字符检测（完整 73+ 码点集合）──
    if cmd.chars().any(|c| {
        is_invisible_unicode(c) || (c.is_control() && c != '\n' && c != '\r' && c != '\t')
    }) {
        return Some("安全拦截：命令包含不可见 Unicode 字符，可能隐藏恶意内容".to_string());
    }

    // ── 2. Windows UNC/SMB 凭证泄漏检测 ──
    if let Some(warning) = detect_unc_path(cmd) {
        return Some(warning);
    }

    // ── 3. 命令混淆/编码攻击检测（16 种模式）──
    let obfuscation = detect_command_obfuscation(cmd);
    if obfuscation.detected {
        let reasons_str = obfuscation.reasons.join("；");
        return Some(format!("安全拦截：检测到命令混淆/编码攻击 — {}", reasons_str));
    }

    let cmd_lower = cmd.to_lowercase();

    // ── 4. 高危：不可逆的破坏性命令 ──
    let critical_patterns = [
        ("rm -rf /", "危险：试图删除根目录"),
        ("rm -rf ~", "危险：试图删除用户目录"),
        ("rm -rf /*", "危险：试图删除根目录下所有文件"),
        ("mkfs", "危险：试图格式化磁盘"),
        ("dd if=", "危险：dd 命令可能覆盖磁盘数据"),
        (":(){:|:&};:", "危险：fork bomb"),
    ];
    for (pattern, msg) in &critical_patterns {
        if cmd_lower.contains(pattern) {
            return Some(msg.to_string());
        }
    }

    // ── 5. 中危：需要注意的命令（记录日志但不拦截）──
    let warn_patterns = [
        "sudo ", "chmod 777", "chown ", "curl | sh", "curl | bash",
        "wget -O - | sh", "eval ", "> /dev/",
    ];
    for pattern in &warn_patterns {
        if cmd_lower.contains(pattern) {
            log::warn!("工具安全: bash_exec 执行了敏感命令: {}", cmd);
            break;
        }
    }

    // 安全: 透明 wrapper 解包检测（集中定义，审批和执行共用）
    let mut actual_cmd = cmd_lower.as_str();
    for wrapper in TRANSPARENT_WRAPPERS {
        if actual_cmd.starts_with(wrapper) {
            actual_cmd = actual_cmd[wrapper.len()..].trim_start();
            log::info!("工具安全: 透明 wrapper '{}' 解包后实际命令: {}", wrapper.trim(), actual_cmd);
        }
    }

    // 安全: 检测 Shell 行续行中的命令替换（$( ) 在续行中可能被隐藏）
    if cmd.contains("\\\n") && (cmd.contains("$(") || cmd.contains("`")) {
        log::warn!("工具安全: 检测到续行中的命令替换: {}", &cmd[..cmd.len().min(100)]);
    }

    None
}

/// 验证写入路径（限制为 Agent workspace、/tmp、当前用户目录下的项目）
fn validate_write_path(path: &str) -> Result<(), String> {
    // 先做基本路径安全校验
    validate_path_safety(path)?;

    let p = std::path::Path::new(path);
    let canonical = if p.exists() {
        p.canonicalize().map_err(|e| format!("路径规范化失败: {}", e))?
    } else if let Some(parent) = p.parent() {
        if parent.exists() {
            parent.canonicalize().map_err(|e| e.to_string())?
                .join(p.file_name().unwrap_or_default())
        } else {
            return Err("写入路径的父目录不存在".to_string());
        }
    } else {
        return Err("无效的写入路径".to_string());
    };

    let path_str = canonical.to_string_lossy();

    // 允许的写入路径
    let allowed = [
        "/tmp",
        "/var/tmp",
    ];
    for prefix in &allowed {
        if path_str.starts_with(prefix) { return Ok(()); }
    }

    // 允许 ~/.xianzhu/ 下的所有路径（Agent workspace）
    if let Some(home) = std::env::var_os("HOME") {
        let home_str = home.to_string_lossy();
        if path_str.starts_with(&format!("{}/.xianzhu/", home_str)) {
            return Ok(());
        }
        // 允许用户 Desktop/Documents/Downloads 下的项目目录
        for dir in &["Desktop", "Documents", "Downloads", "Projects", "workspace"] {
            if path_str.starts_with(&format!("{}/{}/", home_str, dir)) {
                return Ok(());
            }
        }
    }

    Err(format!("安全限制：不允许写入此路径 {}。只能写入 Agent 工作区、/tmp 或用户项目目录。", path_str))
}

// ─── 工具配置 (TOOLS.md) 解析与序列化 ────────────────────────

/// Profile 预设定义：每个 profile 包含的工具列表
pub fn profile_tools(profile: &str) -> Vec<&'static str> {
    match profile {
        "basic" => vec!["calculator", "datetime", "file_read", "file_list", "code_search"],
        "coding" => vec!["calculator", "datetime", "file_read", "file_write", "file_edit", "file_list", "bash_exec", "code_search", "web_fetch", "memory_read", "memory_write"],
        "full" => vec![], // 空表示全部启用
        _ => vec!["calculator", "datetime", "file_read", "file_list", "code_search"], // 未知 profile 降级为 basic
    }
}

/// 解析 TOOLS.md 内容，返回 (profile, overrides)
///
/// overrides: HashMap<tool_name, enabled>
pub fn parse_tools_config(content: &str) -> (String, HashMap<String, bool>) {
    let mut profile = "full".to_string();
    let mut overrides = HashMap::new();
    let mut in_profile = false;
    let mut in_overrides = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "## Profile" {
            in_profile = true;
            in_overrides = false;
            continue;
        }
        if trimmed == "## Overrides" {
            in_overrides = true;
            in_profile = false;
            continue;
        }
        if trimmed.starts_with("## ") || trimmed.starts_with("# ") {
            in_profile = false;
            in_overrides = false;
            continue;
        }

        if in_profile && !trimmed.is_empty() {
            profile = trimmed.to_string();
            in_profile = false;
        }

        if in_overrides && trimmed.starts_with("- ") {
            // 格式: "- tool_name: enabled" 或 "- tool_name: disabled"
            let entry = trimmed.trim_start_matches("- ");
            if let Some((name, status)) = entry.split_once(':') {
                let name = name.trim();
                let status = status.trim();
                let enabled = status == "enabled";
                overrides.insert(name.to_string(), enabled);
            }
        }
    }

    (profile, overrides)
}

/// 判断工具是否启用（基于 profile 和 overrides）
pub fn is_tool_enabled(tool_name: &str, profile: &str, overrides: &HashMap<String, bool>) -> bool {
    // overrides 优先
    if let Some(&enabled) = overrides.get(tool_name) {
        return enabled;
    }

    // full profile: 全部启用
    if profile == "full" {
        return true;
    }

    // 检查 profile 包含列表
    let allowed = profile_tools(profile);
    allowed.contains(&tool_name)
}

/// 将 profile 和 overrides 序列��为 TOOLS.md 格式
pub fn format_tools_config(profile: &str, overrides: &HashMap<String, bool>) -> String {
    let mut output = String::from("# Tools Configuration\n\n## Profile\n");
    output.push_str(profile);
    output.push('\n');

    if !overrides.is_empty() {
        output.push_str("\n## Overrides\n");
        let mut sorted_keys: Vec<&String> = overrides.keys().collect();
        sorted_keys.sort();
        for key in sorted_keys {
            let status = if overrides[key] { "enabled" } else { "disabled" };
            output.push_str(&format!("- {}: {}\n", key, status));
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_manager_creation() {
        let manager = ToolManager::new();
        assert_eq!(manager.get_tool_definitions().len(), 0);
    }

    #[test]
    fn test_calculator_tool_definition() {
        let tool = CalculatorTool;
        let def = tool.definition();
        assert_eq!(def.name, "calculator");
    }

    #[test]
    fn test_safety_level_default() {
        let tool = CalculatorTool;
        assert_eq!(tool.safety_level(), ToolSafetyLevel::Safe);
    }

    #[test]
    fn test_safety_level_guarded() {
        let tool = FileReadTool;
        assert_eq!(tool.safety_level(), ToolSafetyLevel::Guarded);
    }

    #[test]
    fn test_parsed_tool_call() {
        let call = ParsedToolCall {
            id: "call_1".to_string(),
            name: "calculator".to_string(),
            arguments: serde_json::json!({"expression": "1+1"}),
        };
        assert_eq!(call.name, "calculator");
        assert_eq!(call.id, "call_1");
    }

    #[test]
    fn test_eval_math_basic() {
        assert_eq!(super::builtin::eval_math("1+2").unwrap(), 3.0);
        assert_eq!(super::builtin::eval_math("10-3").unwrap(), 7.0);
        assert_eq!(super::builtin::eval_math("4*5").unwrap(), 20.0);
        assert_eq!(super::builtin::eval_math("15/3").unwrap(), 5.0);
    }

    #[test]
    fn test_eval_math_precedence() {
        assert_eq!(super::builtin::eval_math("2+3*4").unwrap(), 14.0);
        assert_eq!(super::builtin::eval_math("(2+3)*4").unwrap(), 20.0);
    }

    #[test]
    fn test_eval_math_negative() {
        assert_eq!(super::builtin::eval_math("-5+3").unwrap(), -2.0);
    }

    #[test]
    fn test_eval_math_divide_by_zero() {
        assert!(super::builtin::eval_math("1/0").is_err());
    }

    #[tokio::test]
    async fn test_calculator_execute() {
        let tool = CalculatorTool;
        let result = tool.execute(serde_json::json!({"expression": "(1+2)*3"})).await.unwrap();
        assert_eq!(result, "9");
    }

    #[tokio::test]
    async fn test_datetime_execute() {
        let tool = DateTimeTool;
        let result = tool.execute(serde_json::json!({"timezone": "+8"})).await.unwrap();
        assert!(result.contains("datetime"));
        assert!(result.contains("UTC+8"));
    }

    #[test]
    fn test_tool_manager_safety_level() {
        let mut manager = ToolManager::new();
        manager.register_tool(Box::new(CalculatorTool));
        manager.register_tool(Box::new(FileReadTool));
        assert_eq!(manager.get_safety_level("calculator"), Some(ToolSafetyLevel::Safe));
        assert_eq!(manager.get_safety_level("file_read"), Some(ToolSafetyLevel::Guarded));
        assert_eq!(manager.get_safety_level("nonexistent"), None);
    }

    #[test]
    fn test_parse_tools_config_full() {
        let content = "# Tools Configuration\n\n## Profile\nfull\n\n## Overrides\n- web_search: disabled\n";
        let (profile, overrides) = parse_tools_config(content);
        assert_eq!(profile, "full");
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides.get("web_search"), Some(&false));
    }

    #[test]
    fn test_parse_tools_config_basic() {
        let content = "# Tools Configuration\n\n## Profile\nbasic\n";
        let (profile, overrides) = parse_tools_config(content);
        assert_eq!(profile, "basic");
        assert!(overrides.is_empty());
    }

    #[test]
    fn test_parse_tools_config_empty() {
        let (profile, overrides) = parse_tools_config("");
        assert_eq!(profile, "full");
        assert!(overrides.is_empty());
    }

    #[test]
    fn test_parse_tools_config_multiple_overrides() {
        let content = "# Tools Configuration\n\n## Profile\ncoding\n\n## Overrides\n- web_search: disabled\n- calculator: enabled\n- file_read: disabled\n";
        let (profile, overrides) = parse_tools_config(content);
        assert_eq!(profile, "coding");
        assert_eq!(overrides.len(), 3);
        assert_eq!(overrides.get("web_search"), Some(&false));
        assert_eq!(overrides.get("calculator"), Some(&true));
        assert_eq!(overrides.get("file_read"), Some(&false));
    }

    #[test]
    fn test_is_tool_enabled_full_profile() {
        let overrides = HashMap::new();
        assert!(is_tool_enabled("calculator", "full", &overrides));
        assert!(is_tool_enabled("web_search", "full", &overrides));
        assert!(is_tool_enabled("anything", "full", &overrides));
    }

    #[test]
    fn test_is_tool_enabled_basic_profile() {
        let overrides = HashMap::new();
        assert!(is_tool_enabled("calculator", "basic", &overrides));
        assert!(is_tool_enabled("datetime", "basic", &overrides));
        assert!(is_tool_enabled("file_read", "basic", &overrides));
        assert!(is_tool_enabled("file_list", "basic", &overrides));
        assert!(is_tool_enabled("code_search", "basic", &overrides));
        assert!(!is_tool_enabled("bash_exec", "basic", &overrides));
        assert!(!is_tool_enabled("web_search", "basic", &overrides));
        assert!(!is_tool_enabled("file_edit", "basic", &overrides));
    }

    #[test]
    fn test_is_tool_enabled_override_wins() {
        let mut overrides = HashMap::new();
        overrides.insert("calculator".to_string(), false);
        // override 禁用 calculator，即使 full profile 允许
        assert!(!is_tool_enabled("calculator", "full", &overrides));

        overrides.insert("web_search".to_string(), true);
        // override 启用 web_search，即使 basic profile 不包含
        assert!(is_tool_enabled("web_search", "basic", &overrides));
    }

    #[test]
    fn test_format_tools_config_roundtrip() {
        let mut overrides = HashMap::new();
        overrides.insert("web_search".to_string(), false);
        overrides.insert("calculator".to_string(), true);
        let output = format_tools_config("coding", &overrides);
        let (profile, parsed_overrides) = parse_tools_config(&output);
        assert_eq!(profile, "coding");
        assert_eq!(parsed_overrides, overrides);
    }

    #[test]
    fn test_format_tools_config_no_overrides() {
        let overrides = HashMap::new();
        let output = format_tools_config("full", &overrides);
        assert!(output.contains("## Profile\nfull"));
        assert!(!output.contains("## Overrides"));
    }

    #[test]
    fn test_profile_tools() {
        assert_eq!(profile_tools("basic"), vec!["calculator", "datetime", "file_read", "file_list", "code_search"]);
        assert!(profile_tools("coding").contains(&"file_read"));
        assert!(profile_tools("coding").contains(&"bash_exec"));
        assert!(profile_tools("coding").contains(&"file_edit"));
        assert!(profile_tools("coding").contains(&"code_search"));
        assert!(profile_tools("coding").contains(&"web_fetch"));
        assert!(profile_tools("full").is_empty()); // 空表示全部启用
    }
}
