# XianZhu 插件系统设计方案

## 背景

OpenClaw 有 47 个插件，覆盖渠道（Telegram/Discord/Slack）、模型提供商（Ollama/vLLM）、功能扩展（语音/记忆增强）三大类。XianZhu 当前这些能力全部硬编码，无法扩展。

**参考架构**：
- **IronClaw**：最成熟，WASM + MCP + Native 三种插件格式，有在线注册表和热加载
- **ZeroClaw**：纯 Rust trait，15+ provider 实现，20+ channel 实现
- **OpenCrust**：WASM 沙箱 + plugin.toml manifest

**XianZhu 现状**：
- `channel.rs` 已有 Channel trait（80% 就绪）
- `memory/mod.rs` 已有 Memory trait（85% 就绪）
- `plugin_sdk.rs` 有插件清单骨架（60% 就绪）
- `llm.rs` 完全硬编码（20% 就绪）— 最大痛点

## 设计原则

1. **渐进式** — 先 trait 抽象，再动态加载，最后 WASM 沙箱
2. **不过度设计** — Phase 1 只做内置插件注册，不做 WASM
3. **兼容现有代码** — 把硬编码改成"内置插件"，不破坏现有功能
4. **多 Agent 隔离** — 每个 Agent 可以有不同的插件配置

## 架构总览

```
┌─────────────────────────────────────────────┐
│                  XianZhu App                │
│                                              │
│  ┌──────────────────────────────────────┐   │
│  │         Plugin Registry              │   │
│  │                                       │   │
│  │  ┌───────────┐  ┌──────────────┐     │   │
│  │  │ Channels   │  │ Providers    │     │   │
│  │  │ ─telegram  │  │ ─openai      │     │   │
│  │  │ ─discord   │  │ ─anthropic   │     │   │
│  │  │ ─slack     │  │ ─ollama      │     │   │
│  │  │ ─feishu    │  │ ─deepseek    │     │   │
│  │  └───────────┘  └──────────────┘     │   │
│  │                                       │   │
│  │  ┌───────────┐  ┌──────────────┐     │   │
│  │  │ Memory     │  │ Features     │     │   │
│  │  │ ─sqlite    │  │ ─tts         │     │   │
│  │  │ ─lancedb   │  │ ─voice       │     │   │
│  │  └───────────┘  └──────────────┘     │   │
│  └──────────────────────────────────────┘   │
│                                              │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  │
│  │ Agent A  │  │ Agent B  │  │ Agent C  │  │
│  │ plugins: │  │ plugins: │  │ plugins: │  │
│  │ -telegram│  │ -discord │  │ -slack   │  │
│  │ -openai  │  │ -ollama  │  │ -openai  │  │
│  └──────────┘  └──────────┘  └──────────┘  │
└─────────────────────────────────────────────┘
```

## 插件类型

```rust
pub enum PluginType {
    Channel,         // 消息渠道（Telegram/Discord/Slack/飞书）
    ModelProvider,   // LLM 提供商（OpenAI/Anthropic/Ollama/DeepSeek）
    MemoryBackend,   // 记忆存储（SQLite/LanceDB）
    Embedding,       // 嵌入模型（OpenAI/Aliyun/Local）
    Feature,         // 功能扩展（TTS/语音/设备配对）
}
```

## Phase 1：Trait 抽象 + 内置插件注册

> 目标：把硬编码改成 trait + registry，所有现有功能变成"内置插件"

### 1.1 ModelProvider trait（最高优先级）

**现状**：`llm.rs` 里 `match config.provider.as_str() { "openai" => ..., "anthropic" => ... }` 硬编码

**目标**：

```rust
// src/agent/provider_trait.rs

#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// 提供商 ID（"openai", "anthropic", "ollama"）
    fn id(&self) -> &str;

    /// 显示名称
    fn display_name(&self) -> &str;

    /// 支持的模型列表
    fn models(&self) -> Vec<String>;

    /// 流式调用
    async fn call_stream(
        &self,
        config: &CallConfig,
        messages: &[serde_json::Value],
        system_prompt: Option<&str>,
        tools: Option<&[ToolDefinition]>,
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<LlmResponse, String>;

    /// 健康检查
    async fn health_check(&self) -> Result<bool, String>;
}

pub struct CallConfig {
    pub model: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
}
```

**内置实现**：

```rust
// src/agent/providers/openai.rs
pub struct OpenAiProvider;

impl ModelProvider for OpenAiProvider {
    fn id(&self) -> &str { "openai" }
    fn display_name(&self) -> &str { "OpenAI" }
    fn models(&self) -> Vec<String> {
        vec!["gpt-4o", "gpt-4-turbo", "gpt-3.5-turbo", "o1", "o3"]
    }
    // ... 从 llm.rs 提取的 OpenAI 逻辑
}

// src/agent/providers/anthropic.rs
pub struct AnthropicProvider;
// ... 从 llm.rs 提取的 Anthropic 逻辑

// src/agent/providers/ollama.rs（新增）
pub struct OllamaProvider;
// ... 本地模型支持
```

**Provider Registry**：

```rust
pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn ModelProvider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        let mut reg = Self { providers: HashMap::new() };
        // 内置 provider 自动注册
        reg.register(Box::new(OpenAiProvider));
        reg.register(Box::new(AnthropicProvider));
        reg
    }

    pub fn get(&self, id: &str) -> Option<&dyn ModelProvider> { ... }
    pub fn register(&mut self, provider: Box<dyn ModelProvider>) { ... }
    pub fn list(&self) -> Vec<&str> { ... }
}
```

### 1.2 Channel trait（已有，需完善）

**现状**：`src/channel.rs` 已有 `Channel` trait，但 Telegram 没有通过它注册

**改动**：
- Telegram 改为实现 Channel trait 注册到 ChannelRegistry
- 新增渠道只需实现 trait + 注册

```rust
// 已有，略微扩展
#[async_trait]
pub trait Channel: Send + Sync {
    fn id(&self) -> &str;
    fn display_name(&self) -> &str;
    fn channel_type(&self) -> ChannelType; // 新增：Telegram/Discord/Slack/...
    async fn start(&self, ctx: ChannelContext) -> Result<(), String>;
    async fn stop(&self) -> Result<(), String>;
    async fn send(&self, msg: OutgoingMessage) -> Result<(), String>;
    fn is_ready(&self) -> bool;
    fn config_fields(&self) -> Vec<ConfigField>; // 新增：配置项声明
}
```

### 1.3 MemoryBackend trait（已有，微调）

**现状**：已有完善的 `Memory` trait，只需注册

### 1.4 Plugin Manifest

```rust
// src/plugin_system/manifest.rs

#[derive(Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,           // "openai-provider"
    pub name: String,         // "OpenAI"
    pub version: String,      // "1.0.0"
    pub description: String,
    pub plugin_type: PluginType,
    pub builtin: bool,        // 内置 vs 第三方
    pub config_schema: Vec<ConfigField>,
    pub dependencies: Vec<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    pub field_type: String,   // "text" | "password" | "select" | "boolean"
    pub required: bool,
    pub default: Option<String>,
    pub placeholder: Option<String>,
}
```

### 1.5 PluginManager（核心）

```rust
pub struct PluginManager {
    /// 已注册的插件清单
    manifests: Vec<PluginManifest>,
    /// 渠道注册表
    channels: ChannelRegistry,
    /// 模型提供商注册表
    providers: ProviderRegistry,
    /// 记忆后端注册表
    memory_backends: MemoryRegistry,
}

impl PluginManager {
    pub fn new() -> Self {
        let mut mgr = Self::default();
        // 注册所有内置插件
        mgr.register_builtin_channels();
        mgr.register_builtin_providers();
        mgr.register_builtin_memory();
        mgr
    }

    /// 列出所有插件（前端展示）
    pub fn list_plugins(&self) -> Vec<&PluginManifest> { ... }

    /// 按类型列出
    pub fn list_by_type(&self, t: PluginType) -> Vec<&PluginManifest> { ... }

    /// 获取 Agent 已启用的插件
    pub async fn get_agent_plugins(&self, agent_id: &str, pool: &SqlitePool) -> Vec<String> { ... }

    /// 为 Agent 启用/禁用插件
    pub async fn toggle_plugin(&self, agent_id: &str, plugin_id: &str, enabled: bool, pool: &SqlitePool) { ... }
}
```

## Phase 2：动态加载 + 配置界面

> 目标：支持从文件系统加载第三方插件，前端有完整的插件管理界面

### 2.1 插件目录结构

```
~/.xianzhu/plugins/
├── builtin/                    ← 内置插件（随 app 安装）
│   ├── openai-provider/
│   │   └── manifest.json
│   ├── anthropic-provider/
│   │   └── manifest.json
│   └── telegram-channel/
│       └── manifest.json
├── installed/                  ← 第三方安装的插件
│   ├── ollama-provider/
│   │   ├── manifest.json
│   │   └── plugin.rs / plugin.wasm
│   └── discord-channel/
│       ├── manifest.json
│       └── plugin.rs / plugin.wasm
```

### 2.2 插件配置存储

```sql
-- 全局插件配置
CREATE TABLE plugin_configs (
    plugin_id TEXT PRIMARY KEY,
    config_json TEXT NOT NULL,  -- 加密存储敏感字段
    enabled BOOLEAN DEFAULT 1,
    updated_at INTEGER
);

-- Agent 级别的插件启用状态
CREATE TABLE agent_plugins (
    agent_id TEXT,
    plugin_id TEXT,
    enabled BOOLEAN DEFAULT 1,
    config_override TEXT,       -- Agent 级别的配置覆盖
    PRIMARY KEY (agent_id, plugin_id)
);
```

### 2.3 前端插件管理页面

```
插件市场
├── 已安装 (8)
│   ├── [内置] OpenAI Provider ✅
│   ├── [内置] Anthropic Provider ✅
│   ├── [内置] Telegram Channel ✅
│   ├── [内置] SQLite Memory ✅
│   └── [安装] Ollama Provider ✅ [配置] [卸载]
├── 渠道类
│   ├── Discord Channel [安装]
│   ├── Slack Channel [安装]
│   ├── 飞书 Channel [安装]
│   └── 微信 Channel [安装]
├── 模型提供商
│   ├── Ollama (本地模型) [安装]
│   ├── DeepSeek [安装]
│   └── vLLM [安装]
└── 功能扩展
    ├── TTS 语音 [安装]
    └── 设备配对 [安装]
```

## Phase 3：WASM 沙箱（可选）

> 目标：支持不受信任的第三方插件安全运行

- 使用 `wasmtime` + WASI 运行第三方插件
- 参考 IronClaw 的 `plugin.toml` 权限声明
- 网络白名单、文件系统隔离、内存限制
- 仅在需要社区插件生态时实施

## 实施路线

### Phase 1（1-2 周）— Trait 抽象

| 步骤 | 文件 | 改动 |
|------|------|------|
| 1 | `src/agent/provider_trait.rs` | 新建 ModelProvider trait |
| 2 | `src/agent/providers/openai.rs` | 从 llm.rs 提取 OpenAI 逻辑 |
| 3 | `src/agent/providers/anthropic.rs` | 从 llm.rs 提取 Anthropic 逻辑 |
| 4 | `src/agent/providers/registry.rs` | Provider 注册表 |
| 5 | `src/agent/llm.rs` | 重构：通过 registry 调用 provider |
| 6 | `src/plugin_system/mod.rs` | PluginManager + PluginManifest |
| 7 | `src/plugin_system/manifest.rs` | 插件清单定义 |
| 8 | `src/channels/mod.rs` | Channel registry 整合 |
| 9 | `src/main.rs` | 启动时注册所有内置插件 |

### Phase 2（1-2 周）— 前端 + 动态加载

| 步骤 | 文件 | 改动 |
|------|------|------|
| 1 | DB schema | 新增 plugin_configs + agent_plugins 表 |
| 2 | `src/main.rs` | 新增插件管理 Tauri 命令 |
| 3 | `frontend/src/pages/PluginsPage.tsx` | 插件管理界面 |
| 4 | `src/plugin_system/loader.rs` | 从文件系统扫描加载插件 |
| 5 | `src/agent/providers/ollama.rs` | 新增 Ollama provider（首个可选插件） |

### Phase 3（按需）— WASM 沙箱

| 步骤 | 改动 |
|------|------|
| 1 | 引入 wasmtime 依赖 |
| 2 | WASM 插件加载器 |
| 3 | 安全沙箱（网络/文件/内存限制）|
| 4 | 插件打包工具 |

## 与现有系统的关系

| 现有系统 | 改造方式 |
|----------|----------|
| **技能 (Skills)** | 保持不变。技能是 prompt 层面的扩展，插件是运行时层面的扩展 |
| **MCP Servers** | 保持不变。MCP 是标准协议，插件是内部扩展机制 |
| **Telegram 轮询** | Phase 1 改造为 Channel 插件 |
| **LLM 调用** | Phase 1 改造为 Provider 插件 |
| **SQLite 记忆** | Phase 1 改造为 Memory 插件 |

## 总结

**核心思路**：把硬编码变成 trait + registry，渐进式支持动态加载。

**最大收益**：
1. 新增渠道/模型只需实现 trait，不改核心代码
2. 不同 Agent 可以用不同的插件组合
3. 用户可以通过界面管理插件，不需要改代码
4. 为未来社区生态打基础
