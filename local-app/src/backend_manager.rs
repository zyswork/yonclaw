//! Node.js 后端管理模块
//!
//! 用于管理 Node.js 后端进程的启动、健康检查和关闭。
//! 提供统一的接口用于 Tauri 应用与后端进程的通信。
//! 优先使用沙箱自带的 NodeRuntime（~/.xianzhu/runtime/node/）。

use anyhow::{anyhow, Result};
use log::{info, warn};
use reqwest::Client;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::runtime::NodeRuntime;

const DEFAULT_BACKEND_PORT: u16 = 3000;
const MAX_PORT_SCAN: u16 = 3010;
const BACKEND_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_millis(500);
const HEALTH_CHECK_MAX_RETRIES: u32 = 20;
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// 选择可用的后端端口
///
/// 优先级：
/// 1. 环境变量 XIANZHU_BACKEND_PORT
/// 2. 从 DEFAULT_BACKEND_PORT 开始扫描到 MAX_PORT_SCAN，返回第一个可用端口
fn select_backend_port() -> u16 {
    // 1. 环境变量优先
    if let Ok(port_str) = std::env::var("XIANZHU_BACKEND_PORT") {
        if let Ok(port) = port_str.parse::<u16>() {
            info!("使用环境变量指定的后端端口: {}", port);
            return port;
        } else {
            warn!("XIANZHU_BACKEND_PORT 值无效: {}", port_str);
        }
    }

    // 2. 扫描可用端口
    for port in DEFAULT_BACKEND_PORT..=MAX_PORT_SCAN {
        match std::net::TcpListener::bind(("127.0.0.1", port)) {
            Ok(_listener) => {
                // listener 在此 drop，释放端口
                if port != DEFAULT_BACKEND_PORT {
                    info!("端口 {} 被占用，自动选择端口: {}", DEFAULT_BACKEND_PORT, port);
                }
                return port;
            }
            Err(_) => continue,
        }
    }

    // 所有端口都不可用，返回默认值（start 中会处理占用情况）
    warn!("端口 {}-{} 均被占用，回退到默认端口 {}", DEFAULT_BACKEND_PORT, MAX_PORT_SCAN, DEFAULT_BACKEND_PORT);
    DEFAULT_BACKEND_PORT
}

/// 后端进程管理器
///
/// 负责 Node.js 后端进程的生命周期管理：
/// - 启动后端进程
/// - 执行健康检查
/// - 优雅关闭进程
///
/// 为有状态的进程管理器，在单个实例中维护进程状态。
pub struct BackendManager {
    /// 运行中的后端进程句柄
    process: Option<Child>,
    /// HTTP 客户端（缓存以避免重复创建）
    http_client: Client,
    /// 后端服务 URL
    backend_url: String,
    /// 实际使用的端口
    port: u16,
}

impl BackendManager {
    /// 创建新的后端管理器实例
    ///
    /// 自动选择可用端口（优先环境变量，然后扫描 3000-3010）
    pub fn new() -> Self {
        let port = select_backend_port();
        Self {
            process: None,
            http_client: Client::new(),
            backend_url: format!("http://localhost:{}", port),
            port,
        }
    }

    /// 启动 Node.js 后端进程
    ///
    /// # 步骤
    /// 1. 检查端口可用性
    /// 2. 通过 NodeRuntime 确保沙箱 Node.js 已安装
    /// 3. 定位后端入口文件
    /// 4. 启动进程并设置环境变量
    /// 5. 等待健康检查通过
    pub async fn start(&mut self) -> Result<()> {
        info!("启动 Node.js 后端...");

        // 1. 检查端口是否可用
        if !self.is_port_available().await {
            warn!("端口 {} 被占用，尝试杀死前一个进程", self.port);
            self.kill_existing_process().await?;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // 2. 确保沙箱 Node.js 已安装，获取可执行文件路径
        let node_exe = self.ensure_node_runtime().await?;
        let backend_entry = self.get_backend_entry()?;

        info!(
            "启动命令: {} {} (端口: {})",
            node_exe.display(),
            backend_entry.display(),
            self.port,
        );

        // 3. 启动 Node.js 进程
        let child = Command::new(&node_exe)
            .arg(&backend_entry)
            .env("PORT", self.port.to_string())
            .env("NODE_ENV", "production")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("启动 Node.js 进程失败: {}", e))?;

        info!("Node.js 进程已启动 (PID: {})", child.id());

        // 保存进程句柄到管理器
        self.process = Some(child);

        // 4. 等待健康检查通过
        self.wait_for_backend_ready().await?;

        info!("Node.js 后端已就绪");

        Ok(())
    }

    /// 优雅关闭后端进程
    pub async fn stop(&mut self) {
        if let Some(mut process) = self.process.take() {
            info!("停止 Node.js 后端 (PID: {})...", process.id());

            #[cfg(unix)]
            {
                use nix::sys::signal::{kill, Signal};
                use nix::unistd::Pid;

                let pid = process.id() as i32;
                if let Err(e) = kill(Pid::from_raw(pid), Signal::SIGTERM) {
                    warn!("发送 SIGTERM 失败: {}", e);
                }
            }

            #[cfg(windows)]
            {
                let _ = process.kill();
            }

            match tokio::time::timeout(Duration::from_secs(3), async {
                let _ = process.wait();
            })
            .await
            {
                Ok(_) => {
                    info!("Node.js 后端已优雅关闭");
                }
                Err(_) => {
                    warn!("Node.js 后端未在规定时间内关闭，强制终止");
                    let _ = process.kill();
                }
            }
        }
    }

    /// 等待后端进程就绪
    async fn wait_for_backend_ready(&self) -> Result<()> {
        info!("等待 Node.js 后端就绪...");

        let start = Instant::now();
        let mut retry_count = 0;

        loop {
            if self.health_check().await {
                info!("后端健康检查通过");
                return Ok(());
            }

            retry_count += 1;
            if retry_count > HEALTH_CHECK_MAX_RETRIES {
                return Err(anyhow!(
                    "Node.js 后端在 {} 秒内未就绪",
                    BACKEND_STARTUP_TIMEOUT.as_secs()
                ));
            }

            if start.elapsed() > BACKEND_STARTUP_TIMEOUT {
                return Err(anyhow!(
                    "等待后端超时 ({} 秒)",
                    BACKEND_STARTUP_TIMEOUT.as_secs()
                ));
            }

            tokio::time::sleep(HEALTH_CHECK_INTERVAL).await;
        }
    }

    /// 对后端执行健康检查
    async fn health_check(&self) -> bool {
        let health_url = format!("{}/health", self.backend_url);

        match tokio::time::timeout(HEALTH_CHECK_TIMEOUT, self.http_client.get(&health_url).send())
            .await
        {
            Ok(Ok(response)) => response.status().is_success(),
            _ => false,
        }
    }

    /// 检查指定端口是否可用
    async fn is_port_available(&self) -> bool {
        match std::net::TcpListener::bind(("127.0.0.1", self.port)) {
            Ok(_listener) => true,
            Err(_) => false,
        }
    }

    /// 杀死已有的后端进程
    async fn kill_existing_process(&self) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            match Command::new("netstat")
                .args(&["-ano"])
                .output()
            {
                Ok(output) => {
                    let output_str = String::from_utf8_lossy(&output.stdout);
                    for line in output_str.lines() {
                        if line.contains(&format!(":{}", self.port)) && line.contains("LISTENING") {
                            if let Some(pid_str) = line.split_whitespace().last() {
                                if let Ok(pid) = pid_str.parse::<u32>() {
                                    match Command::new("taskkill")
                                        .args(&["/F", "/PID", &pid.to_string()])
                                        .output()
                                    {
                                        Ok(_) => info!("成功杀死占用端口 {} 的进程 (PID: {})", self.port, pid),
                                        Err(e) => warn!("杀死进程失败 (PID: {}): {}", pid, e),
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => warn!("无法执行 netstat 命令: {}", e),
            }
        }

        #[cfg(unix)]
        {
            Command::new("pkill")
                .args(&["-f", "admin-backend"])
                .output()
                .ok();
        }

        Ok(())
    }

    /// 确保 Node.js 运行时可用，返回可执行文件路径
    ///
    /// 查找顺序：
    /// 1. 沙箱 NodeRuntime（~/.xianzhu/runtime/node/，未安装则自动下载）
    /// 2. Bundled 版本（.app 资源目录中的 node）
    /// 3. 系统 PATH 中的 node（后备方案）
    async fn ensure_node_runtime(&self) -> Result<PathBuf> {
        // 1. 优先使用沙箱 NodeRuntime（自动下载安装）
        let node_rt = NodeRuntime::new();
        match node_rt.ensure_installed().await {
            Ok(node_bin) => {
                info!("使用沙箱 Node.js: {}", node_bin.display());
                return Ok(node_bin);
            }
            Err(e) => {
                warn!("沙箱 Node.js 安装失败: {}，尝试其他方式", e);
            }
        }

        // 2. 检查 bundled 版本（.app 资源目录）
        #[cfg(target_os = "windows")]
        let bundled_node = "node.exe";
        #[cfg(not(target_os = "windows"))]
        let bundled_node = "node";

        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(app_dir) = exe_path.parent() {
                let bundled_path = app_dir.join("resources").join(bundled_node);
                if bundled_path.exists() {
                    info!("使用 bundled Node.js: {}", bundled_path.display());
                    return Ok(bundled_path);
                }
            }
        }

        // 3. 系统 PATH（后备方案）
        #[cfg(target_os = "windows")]
        let node_cmd = "node.exe";
        #[cfg(not(target_os = "windows"))]
        let node_cmd = "node";

        if Command::new(node_cmd).arg("--version").output().is_ok() {
            info!("使用系统 Node.js: {}", node_cmd);
            return Ok(PathBuf::from(node_cmd));
        }

        Err(anyhow!(
            "找不到 Node.js 可执行文件（沙箱安装失败，无 bundled 版本，系统 PATH 中也没有）"
        ))
    }

    /// 获取后端入口文件路径
    ///
    /// 查找顺序：
    /// 1. Bundled 版本（资源目录中的 admin-backend/dist/index.js）
    /// 2. 环境变量 XIANZHU_BACKEND_PATH
    /// 3. 开发环境（当前目录下的 admin-backend/dist/index.js）
    /// 4. 开发环境（相对于可执行文件的项目根目录）
    fn get_backend_entry(&self) -> Result<PathBuf> {
        // 1. Bundled 版本
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(app_dir) = exe_path.parent() {
                let resources_dir = app_dir.join("resources");
                let backend_entry = resources_dir.join("admin-backend/dist/index.js");

                if backend_entry.exists() {
                    info!("使用 bundled 后端: {}", backend_entry.display());
                    return Ok(backend_entry);
                }
            }
        }

        // 2. 环境变量
        if let Ok(backend_path) = std::env::var("XIANZHU_BACKEND_PATH") {
            let path = PathBuf::from(&backend_path);
            if path.exists() && path.is_file() {
                if path.extension().map_or(false, |ext| ext == "js") {
                    info!("使用环境变量指定的后端: {}", backend_path);
                    return Ok(path);
                } else {
                    warn!("XIANZHU_BACKEND_PATH 指向非 JavaScript 文件: {}", path.display());
                }
            } else {
                warn!("XIANZHU_BACKEND_PATH 指向的文件不存在或不是文件: {}", backend_path);
            }
        }

        // 3. 开发环境（相对于当前目录）
        if let Ok(current_dir) = std::env::current_dir() {
            let dev_entry = current_dir.join("admin-backend/dist/index.js");
            if dev_entry.exists() {
                info!("使用开发环境后端: {}", dev_entry.display());
                return Ok(dev_entry);
            }
        }

        // 4. 开发环境（相对于可执行文件位置推断项目根目录）
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let mut search_dir = exe_dir.to_path_buf();
                for _ in 0..5 {
                    if let Some(parent) = search_dir.parent() {
                        let candidate = parent.join("admin-backend/dist/index.js");
                        if candidate.exists() {
                            info!("使用开发环境后端（相对于 exe）: {}", candidate.display());
                            return Ok(candidate);
                        }
                        search_dir = parent.to_path_buf();
                    } else {
                        break;
                    }
                }
            }
        }

        Err(anyhow!("找不到 Node.js 后端入口文件"))
    }

    /// 获取后端 URL
    pub fn backend_url(&self) -> &str {
        &self.backend_url
    }

    /// 获取后端端口
    pub fn backend_port(&self) -> u16 {
        self.port
    }

    /// 检查进程是否正在运行
    pub fn is_running(&self) -> bool {
        self.process.is_some()
    }
}

impl Default for BackendManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BackendManager {
    fn drop(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_manager_creation() {
        let manager = BackendManager::new();
        // 端口应在有效范围内（可能不是 3000 如果被占用）
        assert!(manager.backend_port() >= DEFAULT_BACKEND_PORT);
        assert!(manager.backend_port() <= MAX_PORT_SCAN);
        assert_eq!(
            manager.backend_url(),
            &format!("http://localhost:{}", manager.backend_port())
        );
        assert!(!manager.is_running());
    }

    #[test]
    fn test_backend_manager_default() {
        let manager = BackendManager::default();
        assert!(manager.backend_port() >= DEFAULT_BACKEND_PORT);
        assert!(!manager.is_running());
    }
}
