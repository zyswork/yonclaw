//! 守护进程管理
//!
//! 参考 IronClaw/OpenCrust 的 service install 模式。
//! macOS: launchd (LaunchAgent)
//! Linux: systemd (user service)
//!
//! 让 XianZhu 在后台运行，无需打开桌面窗口。

use std::path::PathBuf;

/// 服务管理器
pub struct ServiceManager {
    /// 服务名称
    service_name: String,
    /// 可执行文件路径
    executable: PathBuf,
}

impl ServiceManager {
    pub fn new(executable: PathBuf) -> Self {
        Self {
            service_name: "com.xianzhu.agent".to_string(),
            executable,
        }
    }

    /// 安装为系统服务
    pub fn install(&self) -> Result<String, String> {
        #[cfg(target_os = "macos")]
        return self.install_launchd();

        #[cfg(target_os = "linux")]
        return self.install_systemd();

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        Err("当前平台不支持守护进程安装".to_string())
    }

    /// 卸载系统服务
    pub fn uninstall(&self) -> Result<String, String> {
        #[cfg(target_os = "macos")]
        return self.uninstall_launchd();

        #[cfg(target_os = "linux")]
        return self.uninstall_systemd();

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        Err("当前平台不支持".to_string())
    }

    /// 检查服务是否已安装
    pub fn is_installed(&self) -> bool {
        #[cfg(target_os = "macos")]
        return self.launchd_plist_path().exists();

        #[cfg(target_os = "linux")]
        return self.systemd_unit_path().exists();

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        false
    }

    // ── macOS launchd ──

    #[cfg(target_os = "macos")]
    fn launchd_plist_path(&self) -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/LaunchAgents")
            .join(format!("{}.plist", self.service_name))
    }

    #[cfg(target_os = "macos")]
    fn install_launchd(&self) -> Result<String, String> {
        let plist_path = self.launchd_plist_path();
        let log_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Logs/XianZhu");
        let _ = std::fs::create_dir_all(&log_dir);

        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>--daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{log_dir}/daemon.log</string>
    <key>StandardErrorPath</key>
    <string>{log_dir}/daemon.err</string>
</dict>
</plist>"#,
            label = self.service_name,
            exe = self.executable.display(),
            log_dir = log_dir.display(),
        );

        if let Some(parent) = plist_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&plist_path, plist)
            .map_err(|e| format!("写入 plist 失败: {}", e))?;

        // 加载服务
        let output = std::process::Command::new("launchctl")
            .args(["load", "-w", plist_path.to_str().unwrap_or("")])
            .output()
            .map_err(|e| format!("launchctl load 失败: {}", e))?;

        if output.status.success() {
            Ok(format!("服务已安装: {}", plist_path.display()))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("launchctl load 失败: {}", stderr))
        }
    }

    #[cfg(target_os = "macos")]
    fn uninstall_launchd(&self) -> Result<String, String> {
        let plist_path = self.launchd_plist_path();
        if !plist_path.exists() {
            return Err("服务未安装".to_string());
        }

        let _ = std::process::Command::new("launchctl")
            .args(["unload", plist_path.to_str().unwrap_or("")])
            .output();

        std::fs::remove_file(&plist_path)
            .map_err(|e| format!("删除 plist 失败: {}", e))?;

        Ok("服务已卸载".to_string())
    }

    // ── Linux systemd ──

    #[cfg(target_os = "linux")]
    fn systemd_unit_path(&self) -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config/systemd/user")
            .join(format!("{}.service", self.service_name))
    }

    #[cfg(target_os = "linux")]
    fn install_systemd(&self) -> Result<String, String> {
        let unit_path = self.systemd_unit_path();
        let unit = format!(
            r#"[Unit]
Description=XianZhu Agent Daemon
After=network.target

[Service]
ExecStart={exe} --daemon
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
            exe = self.executable.display(),
        );

        if let Some(parent) = unit_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&unit_path, unit)
            .map_err(|e| format!("写入 unit 失败: {}", e))?;

        let _ = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "enable", &self.service_name])
            .output();
        let output = std::process::Command::new("systemctl")
            .args(["--user", "start", &self.service_name])
            .output()
            .map_err(|e| format!("systemctl start 失败: {}", e))?;

        if output.status.success() {
            Ok(format!("服务已安装: {}", unit_path.display()))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("systemctl start 失败: {}", stderr))
        }
    }

    #[cfg(target_os = "linux")]
    fn uninstall_systemd(&self) -> Result<String, String> {
        let unit_path = self.systemd_unit_path();
        if !unit_path.exists() {
            return Err("服务未安装".to_string());
        }

        let _ = std::process::Command::new("systemctl")
            .args(["--user", "stop", &self.service_name])
            .output();
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "disable", &self.service_name])
            .output();

        std::fs::remove_file(&unit_path)
            .map_err(|e| format!("删除 unit 失败: {}", e))?;

        let _ = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();

        Ok("服务已卸载".to_string())
    }
}
