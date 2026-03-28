//! Node.js 运行时管理
//!
//! 自动下载、解压、缓存 Node.js 运行时到 ~/.xianzhu/runtime/node/，
//! 支持 macOS (x64/arm64)、Linux (x64/arm64)、Windows (x64)。

use std::path::{Path, PathBuf};
use tokio::fs;

/// Node.js LTS 版本（固定版本，确保一致性）
const NODE_VERSION: &str = "v22.16.0";

/// Node.js 运行时管理器
pub struct NodeRuntime {
    /// 运行时根目录 ~/.xianzhu/runtime/node/
    base_dir: PathBuf,
}

/// 运行时安装状态
#[derive(Debug, Clone, serde::Serialize)]
pub enum RuntimeStatus {
    /// 已安装可用
    Ready { version: String, path: String },
    /// 未安装
    NotInstalled,
    /// 正在下载
    Downloading { progress_pct: u8 },
    /// 安装失败
    Failed { error: String },
}

impl NodeRuntime {
    /// 创建运行时管理器
    ///
    /// 默认使用 ~/.xianzhu/runtime/node/ 作为安装目录
    pub fn new() -> Self {
        let base_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".xianzhu")
            .join("runtime")
            .join("node");
        Self { base_dir }
    }

    /// 使用自定义目录创建（测试用）
    pub fn with_dir(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// 获取 node 可执行文件路径
    pub fn node_bin_path(&self) -> PathBuf {
        let extracted_dir = self.extracted_dir();
        if cfg!(target_os = "windows") {
            extracted_dir.join("node.exe")
        } else {
            extracted_dir.join("bin").join("node")
        }
    }

    /// 获取 npm 可执行文件路径
    pub fn npm_bin_path(&self) -> PathBuf {
        let extracted_dir = self.extracted_dir();
        if cfg!(target_os = "windows") {
            extracted_dir.join("npm.cmd")
        } else {
            extracted_dir.join("bin").join("npm")
        }
    }

    /// 获取 bin 目录路径（用于注入 PATH）
    pub fn bin_dir(&self) -> PathBuf {
        let extracted_dir = self.extracted_dir();
        if cfg!(target_os = "windows") {
            extracted_dir.clone()
        } else {
            extracted_dir.join("bin")
        }
    }

    /// 检查 Node.js 是否已安装
    pub async fn is_installed(&self) -> bool {
        let node_bin = self.node_bin_path();
        fs::metadata(&node_bin).await.is_ok()
    }

    /// 获取当前状态
    pub async fn status(&self) -> RuntimeStatus {
        if self.is_installed().await {
            RuntimeStatus::Ready {
                version: NODE_VERSION.to_string(),
                path: self.node_bin_path().to_string_lossy().to_string(),
            }
        } else {
            RuntimeStatus::NotInstalled
        }
    }

    /// 确保 Node.js 已安装（未安装则自动下载）
    ///
    /// 返回 node 可执行文件路径
    pub async fn ensure_installed(&self) -> Result<PathBuf, String> {
        if self.is_installed().await {
            log::info!("Node.js 运行时已就绪: {:?}", self.node_bin_path());
            return Ok(self.node_bin_path());
        }

        log::info!("Node.js 运行时未安装，开始下载 {}", NODE_VERSION);
        self.download_and_extract().await?;
        Ok(self.node_bin_path())
    }

    /// 下载并解压 Node.js
    async fn download_and_extract(&self) -> Result<(), String> {
        // 确保目录存在
        fs::create_dir_all(&self.base_dir)
            .await
            .map_err(|e| format!("创建目录失败: {}", e))?;

        let url = self.download_url();
        log::info!("下载 Node.js: {}", url);

        // 下载
        let response = reqwest::get(&url)
            .await
            .map_err(|e| format!("下载失败: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("下载失败，HTTP 状态: {}", response.status()));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("读取下载内容失败: {}", e))?;

        let archive_path = self.base_dir.join(self.archive_filename());
        fs::write(&archive_path, &bytes)
            .await
            .map_err(|e| format!("保存文件失败: {}", e))?;

        log::info!("下载完成 ({} MB)，开始解压", bytes.len() / 1024 / 1024);

        // 解压
        self.extract(&archive_path).await?;

        // 清理压缩包
        let _ = fs::remove_file(&archive_path).await;

        // 验证安装
        if self.is_installed().await {
            log::info!("Node.js {} 安装成功", NODE_VERSION);
            Ok(())
        } else {
            Err("安装后验证失败：node 可执行文件不存在".to_string())
        }
    }

    /// 解压归档文件
    async fn extract(&self, archive_path: &Path) -> Result<(), String> {
        let archive_str = archive_path.to_string_lossy().to_string();
        let base_dir_str = self.base_dir.to_string_lossy().to_string();

        if cfg!(target_os = "windows") {
            // Windows: .zip 格式，使用 PowerShell 解压
            let status = tokio::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    &format!(
                        "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                        archive_str, base_dir_str
                    ),
                ])
                .status()
                .await
                .map_err(|e| format!("PowerShell 解压失败: {}", e))?;

            if !status.success() {
                return Err("PowerShell 解压返回错误".to_string());
            }
        } else {
            // macOS/Linux: .tar.xz 格式
            let status = tokio::process::Command::new("tar")
                .args(["xf", &archive_str, "-C", &base_dir_str])
                .status()
                .await
                .map_err(|e| format!("tar 解压失败: {}", e))?;

            if !status.success() {
                return Err("tar 解压返回错误".to_string());
            }
        }

        Ok(())
    }

    /// 卸载（删除整个运行时目录）
    pub async fn uninstall(&self) -> Result<(), String> {
        if self.base_dir.exists() {
            fs::remove_dir_all(&self.base_dir)
                .await
                .map_err(|e| format!("删除运行时目录失败: {}", e))?;
            log::info!("Node.js 运行时已卸载");
        }
        Ok(())
    }

    /// 获取解压后的目录名
    fn extracted_dir(&self) -> PathBuf {
        let dir_name = if cfg!(target_os = "windows") {
            format!("node-{}-win-x64", NODE_VERSION)
        } else if cfg!(target_os = "macos") {
            let arch = if cfg!(target_arch = "aarch64") {
                "arm64"
            } else {
                "x64"
            };
            format!("node-{}-darwin-{}", NODE_VERSION, arch)
        } else {
            // Linux
            let arch = if cfg!(target_arch = "aarch64") {
                "arm64"
            } else {
                "x64"
            };
            format!("node-{}-linux-{}", NODE_VERSION, arch)
        };
        self.base_dir.join(dir_name)
    }

    /// 构建下载 URL
    fn download_url(&self) -> String {
        let filename = self.archive_filename();
        format!(
            "https://nodejs.org/dist/{}/{}",
            NODE_VERSION, filename
        )
    }

    /// 获取归档文件名
    fn archive_filename(&self) -> String {
        if cfg!(target_os = "windows") {
            format!("node-{}-win-x64.zip", NODE_VERSION)
        } else if cfg!(target_os = "macos") {
            let arch = if cfg!(target_arch = "aarch64") {
                "arm64"
            } else {
                "x64"
            };
            format!("node-{}-darwin-{}.tar.xz", NODE_VERSION, arch)
        } else {
            let arch = if cfg!(target_arch = "aarch64") {
                "arm64"
            } else {
                "x64"
            };
            format!("node-{}-linux-{}.tar.xz", NODE_VERSION, arch)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_url_format() {
        let rt = NodeRuntime::new();
        let url = rt.download_url();
        assert!(url.starts_with(&format!("https://nodejs.org/dist/{}/node-{}-", NODE_VERSION, NODE_VERSION)));
        assert!(url.ends_with(".tar.xz") || url.ends_with(".zip"));
    }

    #[test]
    fn test_node_bin_path() {
        let rt = NodeRuntime::with_dir(PathBuf::from("/tmp/test-runtime"));
        let bin = rt.node_bin_path();
        // macOS/Linux 路径应包含 bin/node
        if !cfg!(target_os = "windows") {
            assert!(bin.to_string_lossy().contains("bin/node"));
        }
    }

    #[test]
    fn test_bin_dir() {
        let rt = NodeRuntime::with_dir(PathBuf::from("/tmp/test-runtime"));
        let dir = rt.bin_dir();
        if !cfg!(target_os = "windows") {
            assert!(dir.to_string_lossy().ends_with("bin"));
        }
    }

    #[test]
    fn test_archive_filename() {
        let rt = NodeRuntime::new();
        let name = rt.archive_filename();
        assert!(name.contains(NODE_VERSION));
        if cfg!(target_os = "macos") {
            assert!(name.contains("darwin"));
        } else if cfg!(target_os = "linux") {
            assert!(name.contains("linux"));
        }
    }

    #[tokio::test]
    async fn test_status_not_installed() {
        let rt = NodeRuntime::with_dir(PathBuf::from("/tmp/nonexistent-node-runtime-xyz"));
        let status = rt.status().await;
        assert!(matches!(status, RuntimeStatus::NotInstalled));
    }

    #[test]
    fn test_runtime_status_serialize() {
        let status = RuntimeStatus::Ready {
            version: "v20.11.1".to_string(),
            path: "/tmp/node".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("Ready"));
        assert!(json.contains("v20.11.1"));
    }
}
