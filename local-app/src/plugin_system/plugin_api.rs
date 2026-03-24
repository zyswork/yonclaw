//! 统一插件 API（参考 OpenClaw definePluginEntry 模式）
//!
//! 每个插件通过 PluginEntry 注册自己的能力：
//! - register_tool: 注册自定义工具
//! - register_provider: 注册 LLM 提供商
//! - register_web_search: 注册搜索引擎
//! - register_image_gen: 注册图片生成
//! - register_tts: 注册语音合成
//! - on_hook: 注册生命周期 hook

use async_trait::async_trait;
use std::collections::HashMap;

use crate::agent::tools::{Tool, ToolDefinition};

/// 插件能力类型
#[derive(Debug, Clone, PartialEq)]
pub enum PluginCapability {
    Tool(String),
    Provider(String),
    WebSearch(String),
    ImageGeneration(String),
    Tts(String),
    Channel(String),
    Hook(String),
}

/// 搜索 Provider 定义
pub struct WebSearchProvider {
    pub id: String,
    pub label: String,
    pub hint: String,
    pub requires_credential: bool,
    pub env_vars: Vec<String>,
    pub execute: Box<dyn Fn(&str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>> + Send + Sync>,
}

/// 图片生成 Provider 定义
pub struct ImageGenProvider {
    pub id: String,
    pub label: String,
    pub models: Vec<String>,
    pub generate: Box<dyn Fn(&str, &str, &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>> + Send + Sync>,
}

/// TTS Provider 定义
pub struct TtsProvider {
    pub id: String,
    pub label: String,
    pub voices: Vec<String>,
    pub synthesize: Box<dyn Fn(&str, &str, f64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>> + Send + Sync>,
}

/// 插件注册 API（传给每个插件的 register 回调）
pub struct PluginApi {
    pub plugin_id: String,
    pub tools: Vec<Box<dyn Tool>>,
    pub web_search_providers: Vec<WebSearchProvider>,
    pub image_gen_providers: Vec<ImageGenProvider>,
    pub tts_providers: Vec<TtsProvider>,
    pub capabilities: Vec<PluginCapability>,
}

impl PluginApi {
    pub fn new(plugin_id: &str) -> Self {
        Self {
            plugin_id: plugin_id.to_string(),
            tools: Vec::new(),
            web_search_providers: Vec::new(),
            image_gen_providers: Vec::new(),
            tts_providers: Vec::new(),
            capabilities: Vec::new(),
        }
    }

    /// 注册自定义工具
    pub fn register_tool(&mut self, tool: Box<dyn Tool>) {
        let name = tool.definition().name.clone();
        log::info!("[Plugin {}] 注册工具: {}", self.plugin_id, name);
        self.capabilities.push(PluginCapability::Tool(name));
        self.tools.push(tool);
    }

    /// 注册搜索引擎
    pub fn register_web_search(&mut self, provider: WebSearchProvider) {
        log::info!("[Plugin {}] 注册搜索引擎: {}", self.plugin_id, provider.id);
        self.capabilities.push(PluginCapability::WebSearch(provider.id.clone()));
        self.web_search_providers.push(provider);
    }

    /// 注册图片生成
    pub fn register_image_gen(&mut self, provider: ImageGenProvider) {
        log::info!("[Plugin {}] 注册图片生成: {}", self.plugin_id, provider.id);
        self.capabilities.push(PluginCapability::ImageGeneration(provider.id.clone()));
        self.image_gen_providers.push(provider);
    }

    /// 注册 TTS
    pub fn register_tts(&mut self, provider: TtsProvider) {
        log::info!("[Plugin {}] 注册 TTS: {}", self.plugin_id, provider.id);
        self.capabilities.push(PluginCapability::Tts(provider.id.clone()));
        self.tts_providers.push(provider);
    }
}

/// 插件入口定义（参考 OpenClaw definePluginEntry）
pub struct PluginEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// 注册回调（插件在这里注册自己的能力）
    pub register: Box<dyn FnOnce(&mut PluginApi) + Send>,
}

/// 已加载的插件
pub struct LoadedPlugin {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: Vec<PluginCapability>,
    pub enabled: bool,
}

/// 统一插件管理器（管理所有插件能力的注册和查询）
pub struct PluginManager {
    /// 已加载的插件列表
    plugins: Vec<LoadedPlugin>,
    /// 所有注册的工具（plugin_id → tools）
    tools: HashMap<String, Vec<Box<dyn Tool>>>,
    /// 搜索引擎 providers
    web_search_providers: Vec<WebSearchProvider>,
    /// 图片生成 providers
    image_gen_providers: Vec<ImageGenProvider>,
    /// TTS providers
    tts_providers: Vec<TtsProvider>,
    /// LLM Model providers
    model_providers: Vec<Box<dyn super::provider_trait::ModelProvider>>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            tools: HashMap::new(),
            web_search_providers: Vec::new(),
            image_gen_providers: Vec::new(),
            tts_providers: Vec::new(),
            model_providers: Vec::new(),
        }
    }

    /// 注册 LLM Model Provider
    pub fn register_model_provider(&mut self, provider: Box<dyn super::provider_trait::ModelProvider>) {
        log::info!("PluginManager: 注册 LLM Provider: {} ({})", provider.display_name(), provider.id());
        self.model_providers.push(provider);
    }

    /// 按 provider ID 查找 Model Provider
    pub fn get_model_provider(&self, id: &str) -> Option<&dyn super::provider_trait::ModelProvider> {
        self.model_providers.iter().find(|p| p.id() == id).map(|p| p.as_ref())
    }

    /// 按模型名查找 Model Provider
    pub fn find_model_provider_by_model(&self, model: &str) -> Option<&dyn super::provider_trait::ModelProvider> {
        self.model_providers.iter().find(|p| p.supports_model(model)).map(|p| p.as_ref())
    }

    /// 列出所有 Model Provider
    pub fn list_model_providers(&self) -> Vec<(&str, &str)> {
        self.model_providers.iter().map(|p| (p.id(), p.display_name())).collect()
    }

    /// 加载一个插件
    pub fn load_plugin(&mut self, entry: PluginEntry) {
        let mut api = PluginApi::new(&entry.id);
        (entry.register)(&mut api);

        let loaded = LoadedPlugin {
            id: entry.id.clone(),
            name: entry.name,
            version: entry.version,
            description: entry.description,
            capabilities: api.capabilities,
            enabled: true,
        };

        log::info!("插件已加载: {} ({}能力)", loaded.id, loaded.capabilities.len());
        self.plugins.push(loaded);

        // 收集注册的能力
        if !api.tools.is_empty() {
            self.tools.insert(entry.id.clone(), api.tools);
        }
        self.web_search_providers.extend(api.web_search_providers);
        self.image_gen_providers.extend(api.image_gen_providers);
        self.tts_providers.extend(api.tts_providers);
    }

    /// 获取所有插件注册的工具定义
    pub fn get_tool_definitions(&self) -> Vec<ToolDefinition> {
        let mut defs = Vec::new();
        for tools in self.tools.values() {
            for tool in tools {
                defs.push(tool.definition());
            }
        }
        defs
    }

    /// 执行插件工具
    pub async fn execute_tool(&self, name: &str, args: serde_json::Value) -> Option<Result<String, String>> {
        for tools in self.tools.values() {
            for tool in tools {
                if tool.definition().name == name {
                    return Some(tool.execute(args).await);
                }
            }
        }
        None
    }

    /// 获取搜索 provider
    pub fn get_web_search_provider(&self, id: &str) -> Option<&WebSearchProvider> {
        self.web_search_providers.iter().find(|p| p.id == id)
    }

    /// 列出所有搜索 provider
    pub fn list_web_search_providers(&self) -> Vec<(&str, &str)> {
        self.web_search_providers.iter().map(|p| (p.id.as_str(), p.label.as_str())).collect()
    }

    /// 获取图片生成 provider
    pub fn get_image_gen_provider(&self, id: &str) -> Option<&ImageGenProvider> {
        self.image_gen_providers.iter().find(|p| p.id == id)
    }

    /// 获取 TTS provider
    pub fn get_tts_provider(&self, id: &str) -> Option<&TtsProvider> {
        self.tts_providers.iter().find(|p| p.id == id)
    }

    /// 列出所有已加载插件
    pub fn list_plugins(&self) -> &[LoadedPlugin] {
        &self.plugins
    }

    /// 转为 JSON（前端展示）
    pub fn to_json(&self) -> Vec<serde_json::Value> {
        let mut result: Vec<serde_json::Value> = self.plugins.iter().map(|p| {
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "version": p.version,
                "description": p.description,
                "type": "plugin",
                "capabilities": p.capabilities.iter().map(|c| format!("{:?}", c)).collect::<Vec<_>>(),
                "enabled": p.enabled,
            })
        }).collect();

        // 追加 Model Provider 信息
        for p in &self.model_providers {
            result.push(serde_json::json!({
                "id": p.id(),
                "name": p.display_name(),
                "version": "1.0.0",
                "description": format!("LLM Provider: {}", p.display_name()),
                "type": "provider",
                "capabilities": [format!("Provider({})", p.id())],
                "models": p.supported_models(),
                "enabled": true,
            }));
        }

        result
    }
}
