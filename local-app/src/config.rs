//! 统一配置系统
//!
//! 参考 IronClaw 的 builder 模式：从文件 + 环境变量 + DB 加载配置。
//! 优先级：环境变量 > config.json > DB settings > 默认值

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 应用配置（所有子系统汇聚于此）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 数据目录（默认 ~/Library/Application Support/com.xianzhu.app/）
    pub data_dir: PathBuf,
    /// Agent 工作区根目录（默认 ~/.xianzhu/agents/）
    pub agents_dir: PathBuf,
    /// LLM 配置
    pub llm: LlmDefaults,
    /// 调度器配置
    pub scheduler: SchedulerDefaults,
    /// 记忆系统配置
    pub memory: MemoryDefaults,
}

/// LLM 默认配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmDefaults {
    /// 默认 provider（"openai" / "anthropic"）
    pub default_provider: String,
    /// 默认模型
    pub default_model: String,
    /// 默认温度
    pub default_temperature: f64,
    /// 默认最大 token
    pub default_max_tokens: i32,
    /// 每日 token 限额（0 = 不限）
    pub daily_token_limit: u64,
}

/// 调度器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerDefaults {
    /// 是否启用
    pub enabled: bool,
    /// 最大并发任务数
    pub max_concurrent: usize,
}

/// 记忆系统配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryDefaults {
    /// 是否启用向量搜索
    pub vector_enabled: bool,
    /// Embedding API URL
    pub embedding_api_url: String,
    /// Embedding 模型
    pub embedding_model: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| home.clone())
            .join("com.xianzhu.app");
        Self {
            data_dir,
            agents_dir: home.join(".xianzhu").join("agents"),
            llm: LlmDefaults::default(),
            scheduler: SchedulerDefaults::default(),
            memory: MemoryDefaults::default(),
        }
    }
}

impl Default for LlmDefaults {
    fn default() -> Self {
        Self {
            default_provider: "openai".to_string(),
            default_model: "gpt-4o".to_string(),
            default_temperature: 0.7,
            default_max_tokens: 2048,
            daily_token_limit: 0,
        }
    }
}

impl Default for SchedulerDefaults {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent: 3,
        }
    }
}

impl Default for MemoryDefaults {
    fn default() -> Self {
        Self {
            vector_enabled: false,
            embedding_api_url: "https://api.openai.com/v1/embeddings".to_string(),
            embedding_model: "text-embedding-3-small".to_string(),
        }
    }
}

impl AppConfig {
    /// 从文件加载配置，合并环境变量覆盖
    pub fn load(config_path: &Path) -> Self {
        let mut config = if config_path.exists() {
            match std::fs::read_to_string(config_path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(e) => {
                    log::warn!("读取配置文件失败: {}, 使用默认配置", e);
                    Self::default()
                }
            }
        } else {
            Self::default()
        };

        // 环境变量覆盖
        if let Ok(v) = std::env::var("XIANZHU_DEFAULT_MODEL") {
            config.llm.default_model = v;
        }
        if let Ok(v) = std::env::var("XIANZHU_DAILY_TOKEN_LIMIT") {
            if let Ok(n) = v.parse() { config.llm.daily_token_limit = n; }
        }
        if let Ok(v) = std::env::var("XIANZHU_AGENTS_DIR") {
            config.agents_dir = PathBuf::from(v);
        }

        config
    }

    /// 保存配置到文件
    pub fn save(&self, config_path: &Path) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("序列化配置失败: {}", e))?;
        if let Some(parent) = config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(config_path, content)
            .map_err(|e| format!("写入配置失败: {}", e))
    }

    /// 配置文件默认路径
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".xianzhu")
            .join("config.json")
    }
}
