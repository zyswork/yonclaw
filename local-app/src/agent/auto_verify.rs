//! 自动验证执行器
//!
//! 两层验证策略（按 Codex 审查建议）：
//! - 轻量层：单文件语法检查（JSON parse、TOML parse、单文件编译提示）
//! - 完整层：项目级验证（cargo check / tsc / pytest），在 loop 结束后统一执行
//!
//! 验证结果结构化返回，供 verify-fix 编排器消费。

use std::path::Path;
use std::process::Command;

/// 验证结果
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub passed: bool,
    /// 错误摘要（人类可读，注入 LLM messages）
    pub summary: String,
    /// 详细诊断信息
    pub diagnostics: Vec<Diagnostic>,
}

/// 单条诊断
#[derive(Debug, Clone, serde::Serialize)]
pub struct Diagnostic {
    pub file: String,
    pub line: Option<u32>,
    pub severity: DiagSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub enum DiagSeverity {
    Error,
    Warning,
}

impl VerifyResult {
    pub fn ok() -> Self {
        Self { passed: true, summary: String::new(), diagnostics: vec![] }
    }
    pub fn fail(summary: String, diagnostics: Vec<Diagnostic>) -> Self {
        Self { passed: false, summary, diagnostics }
    }
}

// ─── 轻量验证（单文件，每次写入后可选调用）──────────────────

/// 对单个文件做轻量语法检查
pub fn check_file_syntax(path: &str) -> Option<VerifyResult> {
    let ext = Path::new(path).extension()?.to_str()?;
    match ext {
        "json" => check_json(path),
        "toml" => check_toml(path),
        "yaml" | "yml" => check_yaml(path),
        _ => None, // 不支持的类型跳过
    }
}

fn check_json(path: &str) -> Option<VerifyResult> {
    let content = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str::<serde_json::Value>(&content) {
        Ok(_) => Some(VerifyResult::ok()),
        Err(e) => Some(VerifyResult::fail(
            format!("JSON 语法错误: {}", e),
            vec![Diagnostic { file: path.to_string(), line: Some(e.line() as u32), severity: DiagSeverity::Error, message: e.to_string() }],
        )),
    }
}

fn check_toml(path: &str) -> Option<VerifyResult> {
    let content = std::fs::read_to_string(path).ok()?;
    match content.parse::<toml::Value>() {
        Ok(_) => Some(VerifyResult::ok()),
        Err(e) => Some(VerifyResult::fail(
            format!("TOML 语法错误: {}", e),
            vec![Diagnostic { file: path.to_string(), line: None, severity: DiagSeverity::Error, message: e.to_string() }],
        )),
    }
}

fn check_yaml(path: &str) -> Option<VerifyResult> {
    let content = std::fs::read_to_string(path).ok()?;
    match serde_yaml::from_str::<serde_yaml::Value>(&content) {
        Ok(_) => Some(VerifyResult::ok()),
        Err(e) => Some(VerifyResult::fail(
            format!("YAML 语法错误: {}", e),
            vec![Diagnostic { file: path.to_string(), line: None, severity: DiagSeverity::Error, message: e.to_string() }],
        )),
    }
}

// ─── 完整验证（项目级，loop 结束后调用）───────────────────

/// 根据改动文件列表，自动检测项目类型并运行验证
///
/// 返回 None 如果无法检测项目类型或无需验证
pub fn run_project_verify(changed_files: &[String], workspace_dir: Option<&str>) -> Option<VerifyResult> {
    if changed_files.is_empty() { return None; }

    // 检测改动文件的类型
    let has_rust = changed_files.iter().any(|f| f.ends_with(".rs"));
    let has_ts = changed_files.iter().any(|f| f.ends_with(".ts") || f.ends_with(".tsx"));
    let has_py = changed_files.iter().any(|f| f.ends_with(".py"));

    // 寻找项目根目录
    let project_dir = workspace_dir
        .or_else(|| changed_files.first().and_then(|f| Path::new(f).parent()?.parent()).map(|p| p.to_str().unwrap_or(".")))
        .unwrap_or(".");

    let mut all_diags = Vec::new();
    let mut all_errors = Vec::new();

    if has_rust {
        if let Some(result) = run_cargo_check(project_dir) {
            if !result.passed {
                all_errors.push(format!("Rust: {}", result.summary));
                all_diags.extend(result.diagnostics);
            }
        }
    }

    if has_ts {
        if let Some(result) = run_tsc_check(project_dir) {
            if !result.passed {
                all_errors.push(format!("TypeScript: {}", result.summary));
                all_diags.extend(result.diagnostics);
            }
        }
    }

    if has_py {
        for f in changed_files.iter().filter(|f| f.ends_with(".py")) {
            if let Some(result) = run_python_check(f) {
                if !result.passed {
                    all_errors.push(format!("Python: {}", result.summary));
                    all_diags.extend(result.diagnostics);
                }
            }
        }
    }

    if all_errors.is_empty() && (has_rust || has_ts || has_py) {
        Some(VerifyResult::ok())
    } else if all_errors.is_empty() {
        None // 无可验证的文件类型
    } else {
        Some(VerifyResult::fail(all_errors.join("\n"), all_diags))
    }
}

fn run_cargo_check(dir: &str) -> Option<VerifyResult> {
    // 找 Cargo.toml 所在目录
    let mut check_dir = Path::new(dir);
    for _ in 0..5 {
        if check_dir.join("Cargo.toml").exists() { break; }
        check_dir = check_dir.parent()?;
    }
    if !check_dir.join("Cargo.toml").exists() { return None; }

    log::info!("auto_verify: 运行 cargo check in {}", check_dir.display());
    let output = Command::new("cargo")
        .args(["check", "--message-format=short"])
        .current_dir(check_dir)
        .output()
        .ok()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        Some(VerifyResult::ok())
    } else {
        let diags = parse_compiler_output(&stderr, "error");
        let error_count = diags.len();
        Some(VerifyResult::fail(
            format!("{} 个编译错误", error_count),
            diags,
        ))
    }
}

fn run_tsc_check(dir: &str) -> Option<VerifyResult> {
    // 找 tsconfig.json 所在目录
    let mut check_dir = Path::new(dir);
    for _ in 0..5 {
        if check_dir.join("tsconfig.json").exists() { break; }
        check_dir = check_dir.parent()?;
    }
    if !check_dir.join("tsconfig.json").exists() { return None; }

    log::info!("auto_verify: 运行 tsc --noEmit in {}", check_dir.display());
    let output = Command::new("npx")
        .args(["tsc", "--noEmit"])
        .current_dir(check_dir)
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if output.status.success() {
        Some(VerifyResult::ok())
    } else {
        let diags = parse_compiler_output(&stdout, "error");
        let error_count = diags.len().max(1);
        Some(VerifyResult::fail(
            format!("{} 个 TypeScript 错误", error_count),
            diags,
        ))
    }
}

fn run_python_check(file: &str) -> Option<VerifyResult> {
    log::info!("auto_verify: 运行 python -m py_compile {}", file);
    let output = Command::new("python3")
        .args(["-m", "py_compile", file])
        .output()
        .ok()?;

    if output.status.success() {
        Some(VerifyResult::ok())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Some(VerifyResult::fail(
            format!("Python 语法错误: {}", file),
            vec![Diagnostic { file: file.to_string(), line: None, severity: DiagSeverity::Error, message: stderr.trim().to_string() }],
        ))
    }
}

/// 从编译器输出中提取诊断信息
fn parse_compiler_output(output: &str, severity_keyword: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.contains(severity_keyword) && (line.contains(".rs") || line.contains(".ts") || line.contains(".tsx") || line.contains(".py")) {
            // 尝试提取 file:line 格式
            let parts: Vec<&str> = line.splitn(2, ':').collect();
            let file = parts.first().unwrap_or(&"").to_string();
            diags.push(Diagnostic {
                file,
                line: None,
                severity: DiagSeverity::Error,
                message: line.to_string(),
            });
        }
    }
    // 最多返回 10 条，避免 context 爆炸
    diags.truncate(10);
    diags
}
