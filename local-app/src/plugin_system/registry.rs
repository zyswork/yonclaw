//! 插件注册表
//!
//! 管理所有已注册插件的清单，提供查询、启用/禁用等操作。
//! Phase 1: 内置插件静态注册；Phase 2: 动态加载。

use super::manifest::{PluginManifest, PluginType, ConfigField};

/// 插件注册表
pub struct PluginRegistry {
    manifests: Vec<PluginManifest>,
}

impl PluginRegistry {
    /// 创建空注册表
    pub fn new() -> Self {
        Self {
            manifests: Vec::new(),
        }
    }

    /// 创建带内置插件的注册表
    pub fn with_builtins() -> Self {
        let mut reg = Self::new();
        reg.register_builtins();
        reg
    }

    /// 注册一个插件
    pub fn register(&mut self, manifest: PluginManifest) {
        // 去重
        if self.manifests.iter().any(|m| m.id == manifest.id) {
            return;
        }
        log::info!("注册插件: {} ({}) [{}]",
            manifest.name, manifest.id, manifest.plugin_type);
        self.manifests.push(manifest);
    }

    /// 获取所有插件清单
    pub fn list(&self) -> &[PluginManifest] {
        &self.manifests
    }

    /// 按类型过滤
    pub fn list_by_type(&self, t: PluginType) -> Vec<&PluginManifest> {
        self.manifests.iter().filter(|m| m.plugin_type == t).collect()
    }

    /// 按 ID 查找
    pub fn get(&self, id: &str) -> Option<&PluginManifest> {
        self.manifests.iter().find(|m| m.id == id)
    }

    /// 注册所有内置插件
    fn register_builtins(&mut self) {
        let cf = |key: &str, label: &str, ft: &str, req: bool, ph: &str| ConfigField {
            key: key.into(), label: label.into(), field_type: ft.into(),
            required: req, default: None, placeholder: Some(ph.into()), options: None,
        };

        // ═══════════════════════════════════════════
        // 模型提供商
        // ═══════════════════════════════════════════
        self.register(PluginManifest::builtin(
            "openai-compatible", "OpenAI 兼容",
            "支持 OpenAI API 格式的提供商（GPT、DeepSeek、Qwen 等）",
            PluginType::ModelProvider, "\u{1F4A1}",
        ).with_config(vec![
            cf("api_key", "API Key", "password", true, "sk-..."),
            ConfigField { key: "base_url".into(), label: "Base URL".into(), field_type: "text".into(),
                required: false, default: Some("https://api.openai.com/v1".into()),
                placeholder: Some("https://api.openai.com/v1".into()), options: None },
        ]));

        self.register(PluginManifest::builtin(
            "anthropic", "Anthropic",
            "Claude 系列模型（支持 prompt caching）",
            PluginType::ModelProvider, "\u{1F9E0}",
        ).with_config(vec![cf("api_key", "API Key", "password", true, "sk-ant-...")]));

        self.register(PluginManifest::builtin(
            "ollama", "Ollama",
            "本地模型运行（Llama、Mistral、Qwen 等），无需 API Key",
            PluginType::ModelProvider, "\u{1F999}",
        ).with_status("ready").with_config(vec![
            ConfigField { key: "base_url".into(), label: "Ollama 地址".into(), field_type: "text".into(),
                required: false, default: Some("http://localhost:11434".into()),
                placeholder: Some("http://localhost:11434".into()), options: None },
        ]));

        self.register(PluginManifest::builtin(
            "vllm", "vLLM",
            "自托管 vLLM 推理服务器（OpenAI 兼容格式）",
            PluginType::ModelProvider, "\u{1F680}",
        ).with_status("ready").with_config(vec![
            ConfigField { key: "base_url".into(), label: "vLLM 地址".into(), field_type: "text".into(),
                required: false, default: Some("http://localhost:8000/v1".into()),
                placeholder: Some("http://localhost:8000/v1".into()), options: None },
        ]));

        // OpenClaw #53248: LM Studio 本地模型（OpenAI 兼容）
        self.register(PluginManifest::builtin(
            "lmstudio", "LM Studio",
            "LM Studio 本地模型（OpenAI 兼容，默认端口 1234）",
            PluginType::ModelProvider, "\u{1F3AE}",
        ).with_status("ready").with_config(vec![
            ConfigField { key: "base_url".into(), label: "LM Studio 地址".into(), field_type: "text".into(),
                required: false, default: Some("http://localhost:1234/v1".into()),
                placeholder: Some("http://localhost:1234/v1".into()), options: None },
            ConfigField { key: "api_key".into(), label: "API Key (可选)".into(), field_type: "password".into(),
                required: false, default: Some("lm-studio".into()),
                placeholder: Some("lm-studio".into()), options: None },
        ]));

        // ─── 国产/热门模型快捷入口 ───
        self.register(PluginManifest::builtin(
            "deepseek", "DeepSeek",
            "DeepSeek-V3/R1 系列模型，性价比极高",
            PluginType::ModelProvider, "\u{1F52D}",
        ).with_config(vec![
            cf("api_key", "API Key", "password", true, "sk-..."),
            ConfigField { key: "base_url".into(), label: "Base URL".into(), field_type: "text".into(),
                required: false, default: Some("https://api.deepseek.com/v1".into()),
                placeholder: Some("https://api.deepseek.com/v1".into()), options: None },
        ]));

        self.register(PluginManifest::builtin(
            "stepfun", "StepFun \u{9636}\u{8DC3}\u{661F}\u{8FB0}",
            "Step-2/Step-1 系列模型",
            PluginType::ModelProvider, "\u{2B50}",
        ).with_config(vec![
            cf("api_key", "API Key", "password", true, ""),
            ConfigField { key: "base_url".into(), label: "Base URL".into(), field_type: "text".into(),
                required: false, default: Some("https://api.stepfun.com/v1".into()),
                placeholder: Some("https://api.stepfun.com/v1".into()), options: None },
        ]));

        self.register(PluginManifest::builtin(
            "minimax", "MiniMax",
            "MiniMax-M2.7/Omni \u{7CFB}\u{5217}\u{6A21}\u{578B}",
            PluginType::ModelProvider, "\u{1F4AB}",
        ).with_config(vec![
            cf("api_key", "API Key", "password", true, ""),
            ConfigField { key: "base_url".into(), label: "Base URL".into(), field_type: "text".into(),
                required: false, default: Some("https://api.minimaxi.com/v1".into()),
                placeholder: Some("https://api.minimaxi.com/v1".into()), options: None },
        ]));

        self.register(PluginManifest::builtin(
            "xiaomi-mimo", "\u{5C0F}\u{7C73} MiMo",
            "MiMo-V2-Pro/Flash/Omni/TTS \u{7CFB}\u{5217}",
            PluginType::ModelProvider, "\u{1F4F1}",
        ).with_config(vec![
            cf("api_key", "API Key", "password", true, "tp-..."),
            ConfigField { key: "base_url".into(), label: "Base URL".into(), field_type: "text".into(),
                required: false, default: Some("https://token-plan-cn.xiaomimimo.com/v1".into()),
                placeholder: Some("https://token-plan-cn.xiaomimimo.com/v1".into()), options: None },
        ]));

        self.register(PluginManifest::builtin(
            "siliconflow", "SiliconFlow \u{7845}\u{57FA}\u{6D41}\u{52A8}",
            "\u{805A}\u{5408}\u{591A}\u{79CD}\u{5F00}\u{6E90}\u{6A21}\u{578B}\u{FF08}Qwen/DeepSeek/Llama\u{FF09}",
            PluginType::ModelProvider, "\u{1F30A}",
        ).with_config(vec![
            cf("api_key", "API Key", "password", true, "sk-..."),
            ConfigField { key: "base_url".into(), label: "Base URL".into(), field_type: "text".into(),
                required: false, default: Some("https://api.siliconflow.cn/v1".into()),
                placeholder: Some("https://api.siliconflow.cn/v1".into()), options: None },
        ]));

        // ═══════════════════════════════════════════
        // 渠道（仅已实现的）
        // ═══════════════════════════════════════════
        self.register(PluginManifest::builtin(
            "telegram-channel", "Telegram",
            "通过 Bot API 接入 Telegram，本地轮询零延迟",
            PluginType::Channel, "\u{1F4E8}",
        ).with_config(vec![cf("bot_token", "Bot Token", "password", true, "123456:ABC-DEF...")]));

        self.register(PluginManifest::builtin(
            "feishu-channel", "飞书",
            "WebSocket 长连接接入飞书，支持流式卡片输出、Markdown 渲染",
            PluginType::Channel, "\u{1F426}",
        ).with_config(vec![
            cf("app_id", "App ID", "text", true, "cli_xxx"),
            cf("app_secret", "App Secret", "password", true, ""),
        ]));

        self.register(PluginManifest::builtin(
            "weixin-channel", "\u{5FAE}\u{4FE1}",
            "iLinkai \u{534F}\u{8BAE}\u{63A5}\u{5165}\u{4E2A}\u{4EBA}\u{5FAE}\u{4FE1}\u{FF0C}\u{626B}\u{7801}\u{767B}\u{5F55} + \u{957F}\u{8F6E}\u{8BE2}",
            PluginType::Channel, "\u{1F4F1}",
        ));

        self.register(PluginManifest::builtin(
            "discord-channel", "Discord",
            "通过 Gateway WebSocket 接入 Discord 服务器",
            PluginType::Channel, "\u{1F3AE}",
        ));

        self.register(PluginManifest::builtin(
            "slack-channel", "Slack",
            "Socket Mode 接入 Slack 工作区（无需公网）",
            PluginType::Channel, "\u{1F4BC}",
        ));

        // ═══════════════════════════════════════════
        // 记忆后端
        // ═══════════════════════════════════════════
        self.register(PluginManifest::builtin(
            "sqlite-memory", "SQLite 记忆",
            "三级记忆系统（FTS5 全文索引 + 向量搜索 + RRF 混合排序）",
            PluginType::MemoryBackend, "\u{1F4BE}",
        ));

        self.register(PluginManifest::builtin(
            "lancedb-memory", "LanceDB 向量记忆",
            "基于 LanceDB 的本地向量数据库，增强语义检索能力",
            PluginType::MemoryBackend, "\u{1F9EC}",
        ).with_status("planned"));

        // ═══════════════════════════════════════════
        // 嵌入模型
        // ═══════════════════════════════════════════
        self.register(PluginManifest::builtin(
            "aliyun-embedding", "阿里云嵌入",
            "通义千问 text-embedding-v3（1024维，中文优化）",
            PluginType::Embedding, "\u{1F50D}",
        ).with_config(vec![cf("api_key", "DashScope API Key", "password", true, "sk-...")]));

        self.register(PluginManifest::builtin(
            "openai-embedding", "OpenAI 嵌入",
            "text-embedding-3-small/large",
            PluginType::Embedding, "\u{1F50E}",
        ).with_status("planned"));

        // ═══════════════════════════════════════════
        // 功能扩展
        // ═══════════════════════════════════════════
        self.register(PluginManifest::builtin(
            "cloud-bridge", "云端桥接",
            "连接 Cloud Gateway，支持移动端访问和离线 Fallback",
            PluginType::Feature, "\u{2601}\u{FE0F}",
        ));

        self.register(PluginManifest::builtin(
            "scheduler", "定时任务",
            "Cron 调度引擎，支持定时执行 Agent 任务",
            PluginType::Feature, "\u{23F0}",
        ));

        self.register(PluginManifest::builtin(
            "diff-viewer", "Diff 查看器",
            "代码差异可视化，渲染 unified diff 为图片或 HTML",
            PluginType::Feature, "\u{1F4DD}",
        ).with_status("planned"));

        self.register(PluginManifest::builtin(
            "device-pair", "设备配对",
            "生成配对码，连接手机/平板等客户端设备",
            PluginType::Feature, "\u{1F4F1}",
        ).with_status("planned"));

        self.register(PluginManifest::builtin(
            "tts", "语音合成",
            "文字转语音（支持本地 sherpa-onnx 离线方案）",
            PluginType::Feature, "\u{1F50A}",
        ).with_status("planned"));
    }

    /// 转为 JSON（前端展示用）
    pub fn to_json(&self) -> Vec<serde_json::Value> {
        self.manifests.iter().map(|m| {
            serde_json::json!({
                "id": m.id,
                "name": m.name,
                "version": m.version,
                "description": m.description,
                "pluginType": format!("{}", m.plugin_type),
                "builtin": m.builtin,
                "icon": m.icon,
                "defaultEnabled": m.default_enabled,
                "configSchema": m.config_schema,
                "status": m.status,
            })
        }).collect()
    }
}
