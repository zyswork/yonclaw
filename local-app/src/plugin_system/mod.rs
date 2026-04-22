//! 插件系统
//!
//! 统一管理渠道、模型提供商、记忆后端等可扩展组件。
//! Phase 2: Plugin API 框架 + 内置插件 + 能力注册

pub mod manifest;
pub mod registry;
pub mod provider_trait;
pub mod providers;
pub mod plugin_api;
pub mod builtin_plugins;
pub mod bundle_compat;
pub mod text_transforms;

pub use registry::PluginRegistry;
pub use provider_trait::{ProviderRegistry, CallConfig};
pub use providers::create_default_registry;
pub use plugin_api::PluginManager;
pub use builtin_plugins::register_builtin_plugins;
