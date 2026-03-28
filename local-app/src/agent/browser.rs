//! 浏览器检测与控制
//!
//! 支持检测系统安装的 Chromium 系浏览器并打开 URL。
//! 参考 OpenClaw chrome.executables.ts 实现。
//!
//! 支持的浏览器：Chrome、Brave、Edge、Chromium
//! 支持的平台：macOS、Linux、Windows


/// 支持的浏览器类型
#[derive(Debug, Clone, serde::Serialize)]
pub enum BrowserKind {
    Chrome,
    Brave,
    Edge,
    Chromium,
    Default, // 系统默认浏览器
}

impl BrowserKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Chrome => "Google Chrome",
            Self::Brave => "Brave Browser",
            Self::Edge => "Microsoft Edge",
            Self::Chromium => "Chromium",
            Self::Default => "Default Browser",
        }
    }
}

/// 检测到的浏览器
#[derive(Debug, Clone, serde::Serialize)]
pub struct DetectedBrowser {
    pub kind: String,
    pub name: String,
    pub path: String,
}

/// 检测系统安装的浏览器
pub fn detect_browsers() -> Vec<DetectedBrowser> {
    let mut browsers = Vec::new();

    #[cfg(target_os = "macos")]
    {
        let candidates = [
            ("Chrome", "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            ("Brave", "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"),
            ("Edge", "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
            ("Chromium", "/Applications/Chromium.app/Contents/MacOS/Chromium"),
        ];
        for (name, path) in &candidates {
            if std::path::Path::new(path).exists() {
                browsers.push(DetectedBrowser {
                    kind: name.to_lowercase().to_string(),
                    name: name.to_string(),
                    path: path.to_string(),
                });
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let candidates = [
            ("Chrome", &["google-chrome", "google-chrome-stable"][..]),
            ("Brave", &["brave-browser", "brave-browser-stable"]),
            ("Edge", &["microsoft-edge", "microsoft-edge-stable"]),
            ("Chromium", &["chromium", "chromium-browser"]),
        ];
        for (name, cmds) in &candidates {
            for cmd in *cmds {
                if let Ok(output) = std::process::Command::new("which").arg(cmd).output() {
                    if output.status.success() {
                        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        if !path.is_empty() {
                            browsers.push(DetectedBrowser {
                                kind: name.to_lowercase().to_string(),
                                name: name.to_string(),
                                path,
                            });
                            break;
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let candidates = [
            ("Chrome", r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            ("Chrome", r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"),
            ("Brave", r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe"),
            ("Edge", r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
            ("Chromium", r"C:\Program Files\Chromium\Application\chrome.exe"),
        ];
        for (name, path) in &candidates {
            if std::path::Path::new(path).exists() {
                browsers.push(DetectedBrowser {
                    kind: name.to_lowercase().to_string(),
                    name: name.to_string(),
                    path: path.to_string(),
                });
            }
        }
    }

    // 始终添加"系统默认"选项
    browsers.push(DetectedBrowser {
        kind: "default".to_string(),
        name: "System Default".to_string(),
        path: String::new(),
    });

    browsers
}

/// 用指定浏览器打开 URL
pub fn open_url(url: &str, browser_kind: Option<&str>) -> Result<(), String> {
    // URL 安全校验
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("安全限制：只能打开 http/https URL".to_string());
    }

    let browsers = detect_browsers();

    // 找到指定浏览器
    if let Some(kind) = browser_kind {
        if kind != "default" {
            if let Some(browser) = browsers.iter().find(|b| b.kind == kind) {
                return launch_browser(&browser.path, url);
            }
            return Err(format!("未找到浏览器: {}", kind));
        }
    }

    // 系统默认浏览器
    open_url_default(url)
}

/// 用系统默认浏览器打开 URL
fn open_url_default(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("打开浏览器失败: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("打开浏览器失败: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(&["/c", "start", url])
            .spawn()
            .map_err(|e| format!("打开浏览器失败: {}", e))?;
    }

    Ok(())
}

/// 启动指定浏览器可执行文件
fn launch_browser(executable: &str, url: &str) -> Result<(), String> {
    std::process::Command::new(executable)
        .arg(url)
        .spawn()
        .map_err(|e| format!("启动 {} 失败: {}", executable, e))?;
    Ok(())
}
