//! 插件系统
//!
//! 支持插件注册、生命周期管理、能力扩展
//! 插件结构：目录 + xianzhu.plugin.json 清单 + 入口文件

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 插件清单（xianzhu.plugin.json）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// 插件名称
    pub name: String,
    /// 版本号
    pub version: String,
    /// 描述
    pub description: String,
    /// 作者
    #[serde(default)]
    pub author: String,
    /// 入口文件（相对路径）
    #[serde(default = "default_entry")]
    pub entry: String,
    /// 插件能力声明
    #[serde(default)]
    pub capabilities: Vec<PluginCapability>,
    /// 生命周期钩子声明
    #[serde(default)]
    pub hooks: Vec<String>,
    /// 依赖的其他插件
    #[serde(default)]
    pub dependencies: Vec<String>,
}

fn default_entry() -> String { "index.js".to_string() }

/// 插件能力类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PluginCapability {
    /// 注册工具
    #[serde(rename = "tool")]
    Tool { name: String, description: String },
    /// 注册 channel
    #[serde(rename = "channel")]
    Channel { name: String },
    /// 注册 memory backend
    #[serde(rename = "memory_backend")]
    MemoryBackend { name: String },
    /// 注册 skill
    #[serde(rename = "skill")]
    Skill { name: String },
    /// 注册 service
    #[serde(rename = "service")]
    Service { name: String, port: Option<u16> },
}

/// 插件状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PluginStatus {
    Installed,
    Active,
    Disabled,
    Error(String),
}

/// 已加载的插件记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRecord {
    pub manifest: PluginManifest,
    pub path: PathBuf,
    pub status: PluginStatus,
    pub installed_at: i64,
}

/// 生命周期钩子事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookEvent {
    BeforeToolCall { tool_name: String, arguments: serde_json::Value },
    AfterToolCall { tool_name: String, result: String, success: bool },
    BeforeModelCall { model: String, messages_count: usize },
    AfterModelCall { model: String, tokens_used: usize },
    OnSessionStart { agent_id: String, session_id: String },
    OnSessionEnd { agent_id: String, session_id: String },
    OnMessage { agent_id: String, role: String, content: String },
}

/// 插件注册表
pub struct PluginRegistry {
    /// 已注册的插件
    plugins: HashMap<String, PluginRecord>,
    /// 插件根目录
    plugins_dir: PathBuf,
}

impl PluginRegistry {
    pub fn new(plugins_dir: PathBuf) -> Self {
        Self {
            plugins: HashMap::new(),
            plugins_dir,
        }
    }

    /// 扫描插件目录，加载所有插件清单
    pub async fn scan(&mut self) -> Result<usize, String> {
        if !self.plugins_dir.exists() {
            std::fs::create_dir_all(&self.plugins_dir)
                .map_err(|e| format!("创建插件目录失败: {}", e))?;
            return Ok(0);
        }

        let mut count = 0;
        let entries = std::fs::read_dir(&self.plugins_dir)
            .map_err(|e| format!("读取插件目录失败: {}", e))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() { continue; }

            let manifest_path = path.join("xianzhu.plugin.json");
            if !manifest_path.exists() { continue; }

            match self.load_manifest(&manifest_path) {
                Ok(manifest) => {
                    let name = manifest.name.clone();
                    self.plugins.insert(name, PluginRecord {
                        manifest,
                        path,
                        status: PluginStatus::Installed,
                        installed_at: chrono::Utc::now().timestamp_millis(),
                    });
                    count += 1;
                }
                Err(e) => {
                    log::warn!("加载插件清单失败 {:?}: {}", manifest_path, e);
                }
            }
        }

        log::info!("扫描到 {} 个插件", count);
        Ok(count)
    }

    /// 加载插件清单
    fn load_manifest(&self, path: &Path) -> Result<PluginManifest, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("读取清单失败: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("解析清单失败: {}", e))
    }

    /// 安装插件（从本地路径）
    pub async fn install_from_path(&mut self, source: &Path) -> Result<String, String> {
        let manifest_path = source.join("xianzhu.plugin.json");
        if !manifest_path.exists() {
            return Err("目录中未找到 xianzhu.plugin.json".to_string());
        }

        let manifest = self.load_manifest(&manifest_path)?;
        let name = manifest.name.clone();

        // 插件名称安全校验
        if name.is_empty() || name.len() > 64
            || !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(format!("插件名称不合法: {}（仅允许字母数字-_，最长64字符）", name));
        }

        let dest = self.plugins_dir.join(&name);

        // 确认目标路径在 plugins_dir 下（防止路径遍历）
        if !dest.starts_with(&self.plugins_dir) {
            return Err("插件安装路径异常：路径遍历检测".to_string());
        }

        // 复制插件目录
        if dest.exists() {
            std::fs::remove_dir_all(&dest)
                .map_err(|e| format!("删除旧版本失败: {}", e))?;
        }
        copy_dir_recursive(source, &dest)?;

        self.plugins.insert(name.clone(), PluginRecord {
            manifest,
            path: dest,
            status: PluginStatus::Installed,
            installed_at: chrono::Utc::now().timestamp_millis(),
        });

        log::info!("插件 {} 安装成功", name);
        Ok(name)
    }

    /// 卸载插件
    pub fn uninstall(&mut self, name: &str) -> Result<(), String> {
        if let Some(record) = self.plugins.remove(name) {
            if record.path.exists() {
                std::fs::remove_dir_all(&record.path)
                    .map_err(|e| format!("删除插件目录失败: {}", e))?;
            }
            log::info!("插件 {} 已卸载", name);
            Ok(())
        } else {
            Err(format!("插件 {} 不存在", name))
        }
    }

    /// 启用插件
    pub fn enable(&mut self, name: &str) -> Result<(), String> {
        if let Some(record) = self.plugins.get_mut(name) {
            record.status = PluginStatus::Active;
            Ok(())
        } else {
            Err(format!("插件 {} 不存在", name))
        }
    }

    /// 禁用插件
    pub fn disable(&mut self, name: &str) -> Result<(), String> {
        if let Some(record) = self.plugins.get_mut(name) {
            record.status = PluginStatus::Disabled;
            Ok(())
        } else {
            Err(format!("插件 {} 不存在", name))
        }
    }

    /// 列出所有插件
    pub fn list(&self) -> Vec<&PluginRecord> {
        self.plugins.values().collect()
    }

    /// 获取插件
    pub fn get(&self, name: &str) -> Option<&PluginRecord> {
        self.plugins.get(name)
    }

    /// 获取所有活跃插件声明的能力
    pub fn active_capabilities(&self) -> Vec<(&str, &PluginCapability)> {
        self.plugins.values()
            .filter(|r| r.status == PluginStatus::Active)
            .flat_map(|r| {
                r.manifest.capabilities.iter()
                    .map(move |c| (r.manifest.name.as_str(), c))
            })
            .collect()
    }
}

/// 递归复制目录
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("创建目录失败: {}", e))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("读取目录失败: {}", e))? {
        let entry = entry.map_err(|e| format!("读取条目失败: {}", e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)
                .map_err(|e| format!("复制文件失败: {}", e))?;
        }
    }
    Ok(())
}
