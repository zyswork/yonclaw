//! Python 沙箱管理器
//!
//! 为 Agent 提供完全独立的 Python 运行环境，用户无需安装 Python：
//! 1. 自动下载 standalone Python（python-build-standalone）到 ~/.xianzhu/python/runtime/
//! 2. 用内置 Python 创建 venv
//! 3. 预装基础库（pandas, openpyxl, requests, matplotlib 等）
//! 4. bash_exec 中 Python 命令自动走沙箱
//! 5. ModuleNotFoundError 自动 pip install 并重试

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::process::Command;

/// 沙箱根目录: ~/.xianzhu/python/
fn sandbox_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".xianzhu")
        .join("python")
}

/// 内置 Python runtime 目录
fn runtime_dir() -> PathBuf {
    sandbox_root().join("runtime")
}

/// 内置 Python 可执行文件路径
fn runtime_python() -> PathBuf {
    let rt = runtime_dir();
    if cfg!(target_os = "windows") {
        rt.join("python").join("python.exe")
    } else {
        rt.join("python").join("bin").join("python3")
    }
}

/// venv 目录
fn venv_dir() -> PathBuf {
    sandbox_root().join("venv")
}

/// 沙箱 Python 路径（venv 内）
pub fn python_path() -> PathBuf {
    let venv = venv_dir();
    if cfg!(target_os = "windows") {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python3")
    }
}

/// 沙箱 pip 路径（venv 内）
pub fn pip_path() -> PathBuf {
    let venv = venv_dir();
    if cfg!(target_os = "windows") {
        venv.join("Scripts").join("pip.exe")
    } else {
        venv.join("bin").join("pip3")
    }
}

/// 沙箱是否已初始化（venv 可用）
pub fn is_initialized() -> bool {
    python_path().exists()
}

/// 是否正在初始化中
static INITIALIZING: AtomicBool = AtomicBool::new(false);

/// 全局初始化状态
static INIT_STATUS: OnceLock<Result<(), String>> = OnceLock::new();

pub fn init_status() -> Option<&'static Result<(), String>> {
    INIT_STATUS.get()
}

/// 是否正在初始化
pub fn is_initializing() -> bool {
    INITIALIZING.load(Ordering::Relaxed)
}

/// 预装的基础库
const BASE_PACKAGES: &[&str] = &[
    "pandas",
    "openpyxl",
    "requests",
    "matplotlib",
    "Pillow",
    "numpy",
    "chardet",
    "python-docx",
    "PyPDF2",
    "beautifulsoup4",
];

/// Standalone Python 下载 URL（python-build-standalone 项目）
/// 版本: CPython 3.12, install_only_stripped（最小体积）
const PYTHON_VERSION: &str = "3.12.9";
const PYTHON_RELEASE: &str = "20250317";

fn standalone_python_url() -> String {
    let base = "https://github.com/indygreg/python-build-standalone/releases/download";
    let tag = format!("{}", PYTHON_RELEASE);

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    let filename = format!("cpython-{}+{}-aarch64-apple-darwin-install_only_stripped.tar.gz", PYTHON_VERSION, PYTHON_RELEASE);

    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    let filename = format!("cpython-{}+{}-x86_64-apple-darwin-install_only_stripped.tar.gz", PYTHON_VERSION, PYTHON_RELEASE);

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    let filename = format!("cpython-{}+{}-x86_64-pc-windows-msvc-install_only_stripped.tar.gz", PYTHON_VERSION, PYTHON_RELEASE);

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    let filename = format!("cpython-{}+{}-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz", PYTHON_VERSION, PYTHON_RELEASE);

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    let filename = format!("cpython-{}+{}-aarch64-unknown-linux-gnu-install_only_stripped.tar.gz", PYTHON_VERSION, PYTHON_RELEASE);

    format!("{}/{}/{}", base, tag, filename)
}

/// 从 app bundle 资源或网络获取 standalone Python
///
/// 优先级：
/// 1. 已存在 → 跳过
/// 2. App bundle 内置 resources/python.tar.gz → 解压（零网络，最快）
/// 3. 网络下载 fallback → 从 CDN/GitHub 下载
async fn setup_standalone_python() -> Result<(), String> {
    let rt_dir = runtime_dir();
    let rt_python = runtime_python();

    if rt_python.exists() {
        log::info!("内置 Python 已存在: {}", rt_python.display());
        return Ok(());
    }

    tokio::fs::create_dir_all(&rt_dir).await
        .map_err(|e| format!("创建 runtime 目录失败: {}", e))?;

    // 方案 1: 从 app bundle 资源解压（安装包自带，无需网络）
    if let Some(bundled) = find_bundled_python() {
        log::info!("从 app bundle 解压内置 Python: {}", bundled.display());
        let rt_dir_clone = rt_dir.clone();
        let bundled_clone = bundled.clone();
        let result = tokio::task::spawn_blocking(move || {
            let file = std::fs::File::open(&bundled_clone)
                .map_err(|e| format!("打开资源文件失败: {}", e))?;
            let decoder = flate2::read::GzDecoder::new(file);
            let mut archive = tar::Archive::new(decoder);
            archive.unpack(&rt_dir_clone)
                .map_err(|e| format!("解压失败: {}", e))
        }).await
            .map_err(|e| format!("解压任务失败: {}", e))?;

        if result.is_ok() {
            finalize_python(&rt_python)?;
            return Ok(());
        }
        log::warn!("bundle 解压失败，尝试网络下载: {:?}", result.err());
    }

    // 方案 2: 网络下载 fallback
    log::info!("从网络下载 standalone Python...");
    let url = standalone_python_url();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let response = client.get(&url).send().await
        .map_err(|e| format!("下载 Python 失败: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("下载失败 ({})，将使用系统 Python", response.status()));
    }

    let bytes = response.bytes().await
        .map_err(|e| format!("读取失败: {}", e))?;

    log::info!("下载完成: {:.1}MB，解压中...", bytes.len() as f64 / 1024.0 / 1024.0);

    let tar_gz = bytes.to_vec();
    let rt_dir_clone = rt_dir.clone();
    tokio::task::spawn_blocking(move || {
        let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(tar_gz));
        let mut archive = tar::Archive::new(decoder);
        archive.unpack(&rt_dir_clone)
            .map_err(|e| format!("解压失败: {}", e))
    }).await
        .map_err(|e| format!("解压任务失败: {}", e))?
        .map_err(|e: String| e)?;

    finalize_python(&rt_python)?;
    Ok(())
}

/// 查找 app bundle 中内置的 python.tar.gz
fn find_bundled_python() -> Option<PathBuf> {
    // Tauri bundle 资源路径：
    // macOS: App.app/Contents/Resources/python.tar.gz
    // Windows: app目录/resources/python.tar.gz
    // Linux: /usr/share/app/resources/python.tar.gz
    let candidates = vec![
        // macOS app bundle: App.app/Contents/Resources/resources/python.tar.gz
        std::env::current_exe().ok()
            .and_then(|p| p.parent()?.parent().map(|p| p.join("Resources/resources/python.tar.gz"))),
        // macOS app bundle (直接)
        std::env::current_exe().ok()
            .and_then(|p| p.parent()?.parent().map(|p| p.join("Resources/python.tar.gz"))),
        // Windows / Linux: 可执行文件旁边的 resources/
        std::env::current_exe().ok()
            .and_then(|p| p.parent().map(|p| p.join("resources/python.tar.gz"))),
        // 开发模式
        Some(PathBuf::from("resources/python.tar.gz")),
    ];

    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// 设置 Python 可执行权限
fn finalize_python(rt_python: &PathBuf) -> Result<(), String> {
    if rt_python.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // 递归设置 bin/ 下所有文件为可执行
            let bin_dir = rt_python.parent().unwrap();
            if let Ok(entries) = std::fs::read_dir(bin_dir) {
                for entry in entries.flatten() {
                    let _ = std::fs::set_permissions(entry.path(), std::fs::Permissions::from_mode(0o755));
                }
            }
        }
        log::info!("standalone Python 就绪: {}", rt_python.display());
        Ok(())
    } else {
        Err(format!("Python 不存在: {}", rt_python.display()))
    }
}

/// 检测可用的 Python（优先内置，fallback 系统）
async fn find_python() -> Result<String, String> {
    // 优先使用内置 standalone Python
    let rt = runtime_python();
    if rt.exists() {
        return Ok(rt.to_string_lossy().to_string());
    }

    // fallback: 系统 Python
    detect_system_python().await
        .ok_or("未找到可用的 Python 3.8+。请等待内置 Python 下载完成，或手动安装 Python。".into())
}

/// 检测系统 Python3
pub async fn detect_system_python() -> Option<String> {
    let candidates = if cfg!(target_os = "windows") {
        vec!["python", "python3", "py -3"]
    } else {
        vec!["python3", "python"]
    };

    for cmd in candidates {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let result = Command::new(parts[0])
            .args(&parts[1..])
            .arg("--version")
            .output()
            .await;

        if let Ok(output) = result {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if version.contains("Python 3.") {
                    let ver_str = version.replace("Python ", "");
                    let parts: Vec<&str> = ver_str.split('.').collect();
                    if let Some(minor) = parts.get(1).and_then(|m| m.parse::<u32>().ok()) {
                        if minor >= 8 {
                            log::info!("检测到系统 Python: {} ({})", cmd, version);
                            return Some(cmd.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// 初始化 Python 沙箱
///
/// 优先下载 standalone Python（用户无需安装），fallback 到系统 Python。
pub async fn initialize() -> Result<(), String> {
    if is_initialized() {
        log::info!("Python 沙箱已存在: {}", venv_dir().display());
        ensure_base_packages().await?;
        return Ok(());
    }

    INITIALIZING.store(true, Ordering::Relaxed);
    log::info!("开始初始化 Python 沙箱...");

    // 1. 从 bundle 解压或网络下载 standalone Python（失败则 fallback 到系统 Python）
    match setup_standalone_python().await {
        Ok(()) => log::info!("内置 Python 就绪"),
        Err(e) => log::warn!("内置 Python 设置失败（将使用系统 Python）: {}", e),
    }

    // 2. 找到可用的 Python
    let python_cmd = find_python().await?;
    log::info!("使用 Python: {}", python_cmd);

    // 3. 创建目录
    let root = sandbox_root();
    tokio::fs::create_dir_all(&root).await
        .map_err(|e| format!("创建沙箱目录失败: {}", e))?;

    // 4. 创建 venv
    log::info!("创建 venv: {}", venv_dir().display());
    let output = Command::new(&python_cmd)
        .args(&["-m", "venv", &venv_dir().to_string_lossy()])
        .output()
        .await
        .map_err(|e| format!("创建 venv 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        INITIALIZING.store(false, Ordering::Relaxed);
        return Err(format!("创建 venv 失败: {}", stderr));
    }

    // 5. 升级 pip
    let _ = Command::new(python_path())
        .args(&["-m", "pip", "install", "--upgrade", "pip"])
        .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
        .output()
        .await;

    // 6. 预装基础库
    ensure_base_packages().await?;

    INITIALIZING.store(false, Ordering::Relaxed);
    log::info!("Python 沙箱初始化完成: {}", venv_dir().display());
    Ok(())
}

/// 查找 app bundle 中内置的 wheels 目录
fn find_bundled_wheels() -> Option<PathBuf> {
    let candidates = vec![
        // macOS: App.app/Contents/Resources/resources/wheels/
        std::env::current_exe().ok()
            .and_then(|p| p.parent()?.parent().map(|p| p.join("Resources/resources/wheels"))),
        // Windows/Linux
        std::env::current_exe().ok()
            .and_then(|p| p.parent().map(|p| p.join("resources/wheels"))),
        // 开发模式
        Some(PathBuf::from("resources/wheels")),
    ];
    for c in candidates.into_iter().flatten() {
        if c.exists() && c.is_dir() {
            return Some(c);
        }
    }
    None
}

/// 确保基础包已安装
///
/// 优先从 app bundle 内置的 wheels 本地安装（零网络），fallback 到 pip install 联网。
async fn ensure_base_packages() -> Result<(), String> {
    let pip = pip_path();
    if !pip.exists() {
        return Err("pip 不存在，沙箱可能损坏".into());
    }

    // 检查已安装的包
    let output = Command::new(&pip)
        .args(&["list", "--format=columns"])
        .output()
        .await
        .map_err(|e| format!("pip list 失败: {}", e))?;

    let installed = String::from_utf8_lossy(&output.stdout).to_lowercase();

    let missing: Vec<&str> = BASE_PACKAGES.iter()
        .filter(|pkg| {
            let name = pkg.to_lowercase().replace('-', "_").replace("python_", "");
            !installed.contains(&name)
        })
        .copied()
        .collect();

    if missing.is_empty() {
        log::info!("基础库全部已安装");
        return Ok(());
    }

    log::info!("安装缺失的基础库: {:?}", missing);

    // 优先从内置 wheels 本地安装（零网络）
    if let Some(wheels_dir) = find_bundled_wheels() {
        log::info!("从内置 wheels 安装: {}", wheels_dir.display());
        let output = Command::new(&pip)
            .arg("install")
            .arg("--no-index")
            .arg("--find-links")
            .arg(wheels_dir.to_string_lossy().as_ref())
            .args(&missing)
            .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
            .output()
            .await
            .map_err(|e| format!("pip install (local) 失败: {}", e))?;

        if output.status.success() {
            log::info!("基础库从内置 wheels 安装完成（零网络）");
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!("内置 wheels 安装部分失败，尝试联网安装: {}", &stderr[..stderr.len().min(300)]);
    }

    // fallback: 联网安装（使用国内镜像加速）
    let output = Command::new(&pip)
        .arg("install")
        .args(&missing)
        .arg("-i")
        .arg("https://pypi.tuna.tsinghua.edu.cn/simple")
        .arg("--trusted-host")
        .arg("pypi.tuna.tsinghua.edu.cn")
        .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
        .output()
        .await
        .map_err(|e| format!("pip install 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::warn!("部分基础库安装失败（非致命）: {}", &stderr[..stderr.len().min(500)]);
    } else {
        log::info!("基础库安装完成");
    }

    Ok(())
}

/// 安装单个包到沙箱（超时 5 分钟）
pub async fn pip_install(package: &str) -> Result<String, String> {
    if !is_initialized() {
        return Err("Python 沙箱未初始化".into());
    }

    log::info!("pip install: {}", package);
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        Command::new(pip_path())
            .args(&["install", package])
            .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
            .output()
    ).await
        .map_err(|_| format!("pip install {} 超时（5分钟）", package))?
        .map_err(|e| format!("pip install 失败: {}", e))?;

    if output.status.success() {
        Ok(format!("已安装 {}", package))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("安装 {} 失败: {}", package, &stderr[..stderr.len().min(300)]))
    }
}

/// 从 ModuleNotFoundError 提取模块名
pub fn extract_missing_module(error_text: &str) -> Option<String> {
    if let Some(pos) = error_text.find("No module named '") {
        let rest = &error_text[pos + 17..];
        if let Some(end) = rest.find('\'') {
            return Some(module_to_package(&rest[..end]));
        }
    }
    if let Some(pos) = error_text.find("No module named \"") {
        let rest = &error_text[pos + 17..];
        if let Some(end) = rest.find('"') {
            return Some(module_to_package(&rest[..end]));
        }
    }
    None
}

/// 模块名 → pip 包名映射
fn module_to_package(module: &str) -> String {
    let top = module.split('.').next().unwrap_or(module);
    match top {
        "sklearn" => "scikit-learn".into(),
        "cv2" => "opencv-python".into(),
        "PIL" => "Pillow".into(),
        "bs4" => "beautifulsoup4".into(),
        "yaml" => "pyyaml".into(),
        "docx" => "python-docx".into(),
        "dotenv" => "python-dotenv".into(),
        "Crypto" => "pycryptodome".into(),
        "serial" => "pyserial".into(),
        "attr" => "attrs".into(),
        "jose" => "python-jose".into(),
        _ => top.to_string(),
    }
}

/// 重写 bash 命令中的 python/pip 为沙箱路径
///
/// 改进方案：不做脆弱的字符串替换，而是通过 PATH 环境变量让沙箱 Python 优先。
/// 返回 (rewritten_command, extra_env) 供 bash_exec 使用。
pub fn sandbox_env() -> Vec<(String, String)> {
    if !is_initialized() {
        return vec![];
    }

    let venv = venv_dir();
    let bin_dir = if cfg!(target_os = "windows") {
        venv.join("Scripts")
    } else {
        venv.join("bin")
    };

    vec![
        ("VIRTUAL_ENV".to_string(), venv.to_string_lossy().to_string()),
        ("XIANZHU_PYTHON".to_string(), python_path().to_string_lossy().to_string()),
        // PATH 前缀在 bash_exec 中注入
        ("_XIANZHU_PYTHON_BIN".to_string(), bin_dir.to_string_lossy().to_string()),
    ]
}

/// 旧接口兼容：重写 python/pip 命令为沙箱路径
pub fn rewrite_python_command(command: &str) -> String {
    if !is_initialized() {
        return command.to_string();
    }

    let py = python_path();
    let py_str = py.to_string_lossy();
    let pip = pip_path();
    let pip_str = pip.to_string_lossy();

    let mut result = command.to_string();

    // 替换命令开头的 python3/python/pip3/pip
    for (pattern, replacement) in &[
        ("python3 ", format!("{} ", py_str)),
        ("python ", format!("{} ", py_str)),
        ("pip3 ", format!("{} ", pip_str)),
        ("pip ", format!("{} ", pip_str)),
    ] {
        if result.starts_with(pattern) {
            result = format!("{}{}", replacement, &result[pattern.len()..]);
            break;
        }
    }

    // 命令中间的（&& / ; / | 后面的）
    for sep in &[" && ", " ; ", " | "] {
        for (pattern, replacement) in &[
            ("python3 ", format!("{} ", py_str)),
            ("python ", format!("{} ", py_str)),
            ("pip3 ", format!("{} ", pip_str)),
            ("pip ", format!("{} ", pip_str)),
        ] {
            let full = format!("{}{}", sep, pattern);
            if result.contains(&full) {
                result = result.replace(&full, &format!("{}{}", sep, replacement));
            }
        }
    }

    if result != command {
        log::info!("Python 命令重写: {} → {}", &command[..command.len().min(80)], &result[..result.len().min(80)]);
    }

    result
}

/// 后台初始化入口
pub fn spawn_background_init() {
    tokio::spawn(async {
        let result = initialize().await;
        match &result {
            Ok(()) => log::info!("Python 沙箱后台初始化成功"),
            Err(e) => log::warn!("Python 沙箱后台初始化失败（非致命）: {}", e),
        }
        let _ = INIT_STATUS.set(result);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_missing_module() {
        assert_eq!(
            extract_missing_module("ModuleNotFoundError: No module named 'pandas'"),
            Some("pandas".into())
        );
        assert_eq!(
            extract_missing_module("ModuleNotFoundError: No module named 'sklearn.model_selection'"),
            Some("scikit-learn".into())
        );
        assert_eq!(
            extract_missing_module("ModuleNotFoundError: No module named 'cv2'"),
            Some("opencv-python".into())
        );
        assert_eq!(extract_missing_module("some other error"), None);
    }

    #[test]
    fn test_module_to_package() {
        assert_eq!(module_to_package("PIL"), "Pillow");
        assert_eq!(module_to_package("bs4"), "beautifulsoup4");
        assert_eq!(module_to_package("pandas"), "pandas");
        assert_eq!(module_to_package("sklearn"), "scikit-learn");
    }
}
