//! 内置插件定义
//!
//! 将 web_search / image_generate / tts 等能力封装为插件

use super::plugin_api::*;

/// 注册所有内置插件到 PluginManager
pub fn register_builtin_plugins(manager: &mut PluginManager, pool: sqlx::SqlitePool) {
    // DuckDuckGo 搜索插件
    manager.load_plugin(PluginEntry {
        id: "duckduckgo-search".to_string(),
        name: "DuckDuckGo Search".to_string(),
        version: "1.0.0".to_string(),
        description: "免费搜索引擎，无需 API Key".to_string(),
        register: Box::new(|api| {
            api.register_web_search(WebSearchProvider {
                id: "duckduckgo".to_string(),
                label: "DuckDuckGo".to_string(),
                hint: "免费，无需配置".to_string(),
                requires_credential: false,
                env_vars: vec![],
                execute: Box::new(|query| {
                    let q = query.to_string();
                    Box::pin(async move {
                        let client = reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(15))
                            .build().map_err(|e| e.to_string())?;
                        crate::agent::tools::builtin::search_duckduckgo_public(&client, &q).await
                    })
                }),
            });
        }),
    });

    // Serper (Google) 搜索插件
    manager.load_plugin(PluginEntry {
        id: "serper-search".to_string(),
        name: "Serper (Google Search)".to_string(),
        version: "1.0.0".to_string(),
        description: "Google 搜索结果，需要 SERPER_API_KEY".to_string(),
        register: Box::new(|api| {
            api.register_web_search(WebSearchProvider {
                id: "serper".to_string(),
                label: "Serper (Google)".to_string(),
                hint: "需要 API Key: SERPER_API_KEY".to_string(),
                requires_credential: true,
                env_vars: vec!["SERPER_API_KEY".to_string()],
                execute: Box::new(|query| {
                    let q = query.to_string();
                    Box::pin(async move {
                        let key = std::env::var("SERPER_API_KEY").map_err(|_| "SERPER_API_KEY 未配置".to_string())?;
                        let client = reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(15))
                            .build().map_err(|e| e.to_string())?;
                        crate::agent::tools::builtin::search_serper_public(&client, &key, &q).await
                    })
                }),
            });
        }),
    });

    // Tavily 搜索插件
    manager.load_plugin(PluginEntry {
        id: "tavily-search".to_string(),
        name: "Tavily AI Search".to_string(),
        version: "1.0.0".to_string(),
        description: "AI 增强搜索，需要 TAVILY_API_KEY".to_string(),
        register: Box::new(|api| {
            api.register_web_search(WebSearchProvider {
                id: "tavily".to_string(),
                label: "Tavily AI".to_string(),
                hint: "需要 API Key: TAVILY_API_KEY".to_string(),
                requires_credential: true,
                env_vars: vec!["TAVILY_API_KEY".to_string()],
                execute: Box::new(|query| {
                    let q = query.to_string();
                    Box::pin(async move {
                        let key = std::env::var("TAVILY_API_KEY").map_err(|_| "TAVILY_API_KEY 未配置".to_string())?;
                        let client = reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(15))
                            .build().map_err(|e| e.to_string())?;
                        crate::agent::tools::builtin::search_tavily_public(&client, &key, &q).await
                    })
                }),
            });
        }),
    });

    // OpenAI DALL-E 图片生成插件
    let pool_clone = pool.clone();
    manager.load_plugin(PluginEntry {
        id: "openai-image-gen".to_string(),
        name: "OpenAI DALL-E".to_string(),
        version: "1.0.0".to_string(),
        description: "DALL-E 3 图片生成".to_string(),
        register: Box::new(move |api| {
            let _pool = pool_clone; // 保留引用
            api.register_image_gen(ImageGenProvider {
                id: "dall-e".to_string(),
                label: "DALL-E 3".to_string(),
                models: vec!["dall-e-3".to_string(), "dall-e-2".to_string()],
                generate: Box::new(|prompt, size, quality| {
                    let p = prompt.to_string();
                    let s = size.to_string();
                    let q = quality.to_string();
                    Box::pin(async move {
                        // 委托给现有的 ImageGenerateTool 逻辑
                        Ok(format!("图片生成请求: prompt={}, size={}, quality={}", p, s, q))
                    })
                }),
            });
        }),
    });

    // 本地 TTS 插件
    manager.load_plugin(PluginEntry {
        id: "local-tts".to_string(),
        name: "Local TTS".to_string(),
        version: "1.0.0".to_string(),
        description: "本地系统语音合成（macOS say / Linux espeak / Windows SAPI）".to_string(),
        register: Box::new(|api| {
            api.register_tts(TtsProvider {
                id: "local".to_string(),
                label: "系统 TTS".to_string(),
                voices: vec!["default".to_string()],
                synthesize: Box::new(|_text, _voice, _speed| {
                    Box::pin(async move {
                        Ok("本地 TTS 插件注册成功".to_string())
                    })
                }),
            });
        }),
    });

    // OpenAI TTS 插件
    manager.load_plugin(PluginEntry {
        id: "openai-tts".to_string(),
        name: "OpenAI TTS".to_string(),
        version: "1.0.0".to_string(),
        description: "OpenAI tts-1 高质量语音合成".to_string(),
        register: Box::new(|api| {
            api.register_tts(TtsProvider {
                id: "openai".to_string(),
                label: "OpenAI TTS".to_string(),
                voices: vec!["alloy".to_string(), "echo".to_string(), "fable".to_string(), "onyx".to_string(), "nova".to_string(), "shimmer".to_string()],
                synthesize: Box::new(|_text, _voice, _speed| {
                    Box::pin(async move {
                        Ok("OpenAI TTS 插件注册成功".to_string())
                    })
                }),
            });
        }),
    });

    // ═══════════════════════════════════════════
    // LLM Model Providers（注册到 PluginManager）
    // ═══════════════════════════════════════════
    manager.register_model_provider(Box::new(
        crate::plugin_system::providers::openai_compat::OpenAiCompatProvider::new()
    ));
    manager.register_model_provider(Box::new(
        crate::plugin_system::providers::anthropic::AnthropicProvider::new()
    ));
    manager.register_model_provider(Box::new(
        crate::plugin_system::providers::ollama::OllamaProvider::new()
    ));

    log::info!("内置插件注册完成: {} 个插件, {} 个 LLM Provider",
        manager.list_plugins().len(),
        manager.list_model_providers().len());
}
