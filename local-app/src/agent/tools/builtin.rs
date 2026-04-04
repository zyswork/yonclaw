// 内置工具实现

use super::*;

/// 计算工具 — 支持基本四则运算
pub struct CalculatorTool;

#[async_trait]
impl Tool for CalculatorTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "calculator".to_string(),
            description: "执行数学计算，支持加减乘除和括号".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "expression": {
                        "type": "string",
                        "description": "数学表达式，如 (1+2)*3"
                    }
                },
                "required": ["expression"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Safe
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let expression = arguments
            .get("expression")
            .and_then(|e| e.as_str())
            .ok_or("缺少 expression 参数")?;

        log::info!("执行计算: {}", expression);
        let result = eval_math(expression)?;
        Ok(result.to_string())
    }
}

/// 日期时间工具
pub struct DateTimeTool;

#[async_trait]
impl Tool for DateTimeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "datetime".to_string(),
            description: "获取当前日期和时间".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "timezone": {
                        "type": "string",
                        "description": "时区偏移，如 +8 或 -5，默认 UTC"
                    }
                },
                "required": []
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Safe
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let tz_offset = arguments
            .get("timezone")
            .and_then(|t| t.as_str())
            .unwrap_or("+0");

        let offset_hours: i32 = tz_offset
            .trim_start_matches('+')
            .parse()
            .unwrap_or(0);

        let now = chrono::Utc::now();
        let offset = chrono::FixedOffset::east_opt(offset_hours * 3600)
            .ok_or_else(|| format!("无效时区偏移: {}", tz_offset))?;
        let local = now.with_timezone(&offset);

        Ok(serde_json::json!({
            "datetime": local.format("%Y-%m-%d %H:%M:%S").to_string(),
            "timezone": format!("UTC{}", tz_offset),
            "timestamp": now.timestamp()
        }).to_string())
    }
}

/// 记忆读取工具 — 通过 SqliteMemory 管线检索（支持 FTS5 + 向量混合搜索）
pub struct MemoryReadTool {
    pool: sqlx::SqlitePool,
}

impl MemoryReadTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Tool for MemoryReadTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "memory_read".to_string(),
            description: "检索 Agent 的长期记忆。适用于：回忆用户偏好、查找历史对话中的信息、获取之前保存的知识。支持语义搜索（向量）和关键词搜索（FTS5）混合检索。当用户提到「之前说过」「上次」「记得吗」时应主动使用。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "Agent ID"
                    },
                    "query": {
                        "type": "string",
                        "description": "搜索关键词或语义查询（可选，为空则返回全部）"
                    },
                    "memory_type": {
                        "type": "string",
                        "description": "记忆类型过滤（可选）：core, episodic, semantic, procedural",
                        "enum": ["core", "episodic", "semantic", "procedural"]
                    }
                },
                "required": ["agent_id"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Guarded
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let agent_id = arguments.get("agent_id").and_then(|a| a.as_str())
            .ok_or("缺少 agent_id 参数")?;
        let query = arguments.get("query").and_then(|q| q.as_str()).unwrap_or("");
        let memory_type = arguments.get("memory_type").and_then(|t| t.as_str());

        log::info!("读取记忆: agent_id={}, query={}, type={:?}", agent_id, query, memory_type);

        // 通过 SqliteMemory 管线检索（自动走 RRF 混合搜索）
        use crate::memory::{SqliteMemory, Memory, MemoryCategory};
        let mem = if let Some(emb_config) = SqliteMemory::try_load_embedding_config(&self.pool).await {
            SqliteMemory::with_embedding(self.pool.clone(), emb_config).await
        } else {
            SqliteMemory::new(self.pool.clone())
        };

        if query.is_empty() {
            // 无查询：列出全部（按类型过滤）
            let category = memory_type.map(|t| MemoryCategory::from_str(t));
            let entries = if let Some(cat) = category {
                mem.list(agent_id, cat).await?
            } else {
                // 无类型过滤：列出全部（用 recall 空查询）
                mem.recall(agent_id, "", 30).await.unwrap_or_default()
            };
            if entries.is_empty() {
                return Ok("暂无记忆".to_string());
            }
            let result: Vec<serde_json::Value> = entries.iter().map(|e| serde_json::json!({
                "id": e.id, "type": e.category.as_str(), "content": e.content,
                "priority": e.priority.as_i32(), "score": e.score,
            })).collect();
            return Ok(serde_json::to_string_pretty(&result).unwrap_or_default());
        }

        // 有查询：走 recall（RRF 混合搜索）
        let entries = mem.recall(agent_id, query, 10).await?;

        // 按 memory_type 过滤
        let filtered: Vec<_> = entries.into_iter()
            .filter(|e| memory_type.is_none() || e.category.as_str() == memory_type.unwrap())
            .collect();

        if filtered.is_empty() {
            return Ok(format!("没有匹配 \"{}\" 的记忆", query));
        }

        let result: Vec<serde_json::Value> = filtered.iter().map(|e| serde_json::json!({
            "id": e.id, "type": e.category.as_str(), "content": e.content,
            "priority": e.priority.as_i32(),
            "relevance": format!("{:.1}%", e.score.unwrap_or(0.0) * 100.0),
        })).collect();

        Ok(serde_json::to_string_pretty(&result).unwrap_or_default())
    }
}

/// 记忆写入工具 — 通过 SqliteMemory 管线存储（自动写 FTS5 + 生成向量嵌入）
pub struct MemoryWriteTool {
    pool: sqlx::SqlitePool,
}

impl MemoryWriteTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Tool for MemoryWriteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "memory_write".to_string(),
            description: "将重要信息保存为 Agent 的长期记忆。适用于：记住用户偏好、保存重要事实、存储学到的知识。记忆会跨会话持久保存，并自动建立全文索引和语义向量。当用户说「记住」「以后都这样」或透露重要偏好时应主动使用。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "Agent ID"
                    },
                    "memory_type": {
                        "type": "string",
                        "description": "记忆类型：core（用户核心信息）、episodic（事件记忆）、semantic（知识信息）、procedural（操作流程）",
                        "enum": ["core", "episodic", "semantic", "procedural"]
                    },
                    "content": {
                        "type": "string",
                        "description": "记忆内容（清晰、具体的文本描述）"
                    },
                    "priority": {
                        "type": "integer",
                        "description": "优先级 1-10（10=用户明确要求记住，7-9=重要偏好，4-6=项目信息，1-3=一般信息）",
                        "minimum": 1,
                        "maximum": 10
                    }
                },
                "required": ["agent_id", "memory_type", "content"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Guarded
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let agent_id = arguments.get("agent_id").and_then(|a| a.as_str())
            .ok_or("缺少 agent_id 参数")?;
        let memory_type = arguments.get("memory_type").and_then(|t| t.as_str())
            .ok_or("缺少 memory_type 参数")?;
        let content = arguments.get("content").and_then(|c| c.as_str())
            .ok_or("缺少 content 参数")?;
        let priority = arguments.get("priority").and_then(|p| p.as_i64()).unwrap_or(5) as i32;

        log::info!("写入记忆: agent_id={}, type={}, priority={}", agent_id, memory_type, priority);

        // 通过 SqliteMemory 管线存储（自动写 FTS5 + 嵌入向量）
        use crate::memory::{SqliteMemory, Memory, MemoryCategory, MemoryPriority};
        let mem = if let Some(emb_config) = SqliteMemory::try_load_embedding_config(&self.pool).await {
            SqliteMemory::with_embedding(self.pool.clone(), emb_config).await
        } else {
            SqliteMemory::new(self.pool.clone())
        };

        let category = MemoryCategory::from_str(memory_type);
        let mem_priority = MemoryPriority::from_i32(priority.min(3));
        let key = format!("{}-{}", memory_type, chrono::Utc::now().timestamp_millis());

        let _id = mem.store_with_priority(agent_id, &key, content, category, mem_priority).await?;

        let has_vector = mem.has_embedding();
        Ok(format!("记忆已保存 [{}] (优先级 {}, FTS5 ✓, 向量 {}): {}",
            memory_type, priority,
            if has_vector { "✓" } else { "未配置" },
            content
        ))
    }
}

/// 文件读取工具
pub struct FileReadTool;

#[async_trait]
impl Tool for FileReadTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_read".to_string(),
            description: "读取指定文件的内容。适用于：查看代码、配置文件、日志。对于大文件，先用 file_list 确认文件存在和大小，再有针对性地读取。可以指定 offset 和 limit 只读取文件的一部分。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "文件路径"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Guarded
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let path = arguments
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or("缺少 path 参数")?;

        // 统一路径安全校验
        validate_path_safety(path)?;

        log::info!("读取文件: {}", path);

        // 如果是目录，列出目录内容
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| format!("访问路径失败: {}", e))?;

        if metadata.is_dir() {
            let mut entries = tokio::fs::read_dir(path)
                .await
                .map_err(|e| format!("读取目录失败: {}", e))?;
            let mut listing = format!("目录 {} 的内容:\n", path);
            while let Some(entry) = entries.next_entry().await.map_err(|e| format!("读取目录项失败: {}", e))? {
                let ft = entry.file_type().await.ok();
                let marker = if ft.map_or(false, |t| t.is_dir()) { "/" } else { "" };
                listing.push_str(&format!("  {}{}\n", entry.file_name().to_string_lossy(), marker));
            }
            return Ok(listing);
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| format!("读取文件失败: {}", e))?;

        // Harness: 注册文件内容 hash（用于后续编辑时校验）
        super::super::file_harness::register_read(path, &content);

        Ok(content)
    }
}

/// 网络搜索工具（6 纯搜索 API）
///
/// 只包含纯搜索 API（直接返回网页结果），不混入 LLM 搜索能力：
/// - Brave Search（api.search.brave.com）
/// - Exa（api.exa.ai，神经搜索 + 内容提取）
/// - Serper（google.serper.dev，Google 搜索代理）
/// - Tavily（api.tavily.com，AI 搜索 + 摘要）
/// - Firecrawl（api.firecrawl.dev，搜索 + 网页抓取）
/// - DuckDuckGo（免费，无需 API Key，兜底）
///
/// 注意：Perplexity/Grok/Kimi 是 LLM 模型的内置搜索能力，
/// 不属于纯搜索 API，应在对话层面通过 function calling 使用。
///
/// 自动检测优先级：Brave → Serper → Exa → Tavily → Firecrawl → DuckDuckGo
pub struct WebSearchTool {
    pool: sqlx::SqlitePool,
}

impl WebSearchTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }

    /// 获取 API Key（优先 DB settings → 环境变量）
    async fn get_api_key(pool: &sqlx::SqlitePool, env_var: &str) -> Option<String> {
        let db_key: Option<String> = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = ?"
        ).bind(format!("plugin_key_{}", env_var))
        .fetch_optional(pool).await.ok().flatten();

        if let Some(key) = db_key {
            if !key.is_empty() { return Some(key); }
        }

        std::env::var(env_var).ok().filter(|k| !k.is_empty())
    }

    /// 从 DB 读取用户配置的搜索引擎偏好
    async fn get_preferred_provider(pool: &sqlx::SqlitePool) -> String {
        let result: Option<String> = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = 'web_search_provider'"
        ).fetch_optional(pool).await.ok().flatten();
        result.unwrap_or_else(|| "auto".to_string())
    }
}

/// 纯搜索 API 自动检测优先级（有 key 的优先，DuckDuckGo 兜底）
const AUTO_DETECT_CHAIN: &[(&str, &[&str])] = &[
    ("brave",      &["BRAVE_API_KEY"]),
    ("serper",     &["SERPER_API_KEY"]),
    ("exa",        &["EXA_API_KEY"]),
    ("tavily",     &["TAVILY_API_KEY"]),
    ("firecrawl",  &["FIRECRAWL_API_KEY"]),
    // DuckDuckGo 无需 key，作为最终兜底
];

#[async_trait]
impl Tool for WebSearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "web_search".to_string(),
            description: "搜索互联网获取最新信息。适用于：查找技术文档、了解最新动态、搜索错误解决方案。搜索词建议精简且具体（如 'Rust tokio async runtime 2025' 而不是 '帮我查一下 Rust'）。搜索结果只有摘要，需要详细内容请用 web_fetch 打开链接。支持 Brave/Exa/Serper/Tavily/Firecrawl/DuckDuckGo 引擎。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索查询"
                    },
                    "provider": {
                        "type": "string",
                        "description": "搜索引擎（可选）：brave/exa/serper/tavily/firecrawl/duckduckgo/auto"
                    },
                    "count": {
                        "type": "integer",
                        "description": "返回结果数量（1-10，默认 5）"
                    }
                },
                "required": ["query"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Guarded
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let query = arguments.get("query").and_then(|q| q.as_str())
            .ok_or("缺少 query 参数")?;
        let count = arguments.get("count").and_then(|c| c.as_u64()).unwrap_or(5).min(10).max(1) as usize;

        let explicit_provider = arguments.get("provider").and_then(|p| p.as_str()).unwrap_or("");
        let preferred = if explicit_provider.is_empty() {
            Self::get_preferred_provider(&self.pool).await
        } else {
            explicit_provider.to_string()
        };

        log::info!("网络搜索: query={} provider={} count={}", query, preferred, count);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        match preferred.as_str() {
            "brave" => {
                let key = Self::get_api_key(&self.pool, "BRAVE_API_KEY").await
                    .ok_or("BRAVE_API_KEY 未配置。获取: https://brave.com/search/api/")?;
                search_brave(&client, &key, query, count).await
            }
            "exa" => {
                let key = Self::get_api_key(&self.pool, "EXA_API_KEY").await
                    .ok_or("EXA_API_KEY 未配置。获取: https://exa.ai/")?;
                search_exa(&client, &key, query, count).await
            }
            "serper" => {
                let key = Self::get_api_key(&self.pool, "SERPER_API_KEY").await
                    .ok_or("SERPER_API_KEY 未配置。获取: https://serper.dev/")?;
                search_serper(&client, &key, query).await
            }
            "tavily" => {
                let key = Self::get_api_key(&self.pool, "TAVILY_API_KEY").await
                    .ok_or("TAVILY_API_KEY 未配置。获取: https://tavily.com/")?;
                search_tavily(&client, &key, query).await
            }
            "firecrawl" => {
                let key = Self::get_api_key(&self.pool, "FIRECRAWL_API_KEY").await
                    .ok_or("FIRECRAWL_API_KEY 未配置。获取: https://www.firecrawl.dev/")?;
                search_firecrawl(&client, &key, query, count).await
            }
            "duckduckgo" => {
                search_duckduckgo(&client, query).await
            }
            _ => {
                // auto: 按优先级检测有 key 的引擎，最后 fallback DuckDuckGo
                for (provider, env_vars) in AUTO_DETECT_CHAIN {
                    for env_var in *env_vars {
                        if let Some(key) = Self::get_api_key(&self.pool, env_var).await {
                            let result = match *provider {
                                "brave" => search_brave(&client, &key, query, count).await,
                                "exa" => search_exa(&client, &key, query, count).await,
                                "serper" => search_serper(&client, &key, query).await,
                                "tavily" => search_tavily(&client, &key, query).await,
                                "firecrawl" => search_firecrawl(&client, &key, query, count).await,
                                _ => continue,
                            };
                            if let Ok(r) = result {
                                log::info!("auto-detect 使用 {} 搜索成功", provider);
                                return Ok(r);
                            }
                            break; // 该引擎有 key 但失败，尝试下一个
                        }
                    }
                }
                // 所有付费引擎都不可用，fallback DuckDuckGo
                match search_duckduckgo(&client, query).await {
                    Ok(r) => Ok(r),
                    Err(e) => {
                        log::warn!("DuckDuckGo 也失败: {}", e);
                        Ok(format!(
                            "搜索工具暂不可用（免费引擎被限制）。\n\
                            请在设置中配置搜索 API Key：\n\
                            - Serper (Google): https://serper.dev （每月 2500 次免费）\n\
                            - Tavily: https://tavily.com （每月 1000 次免费）\n\
                            - Brave: https://brave.com/search/api/\n\n\
                            可用 web_fetch 工具直接访问特定网页获取信息。"
                        ))
                    }
                }
            }
        }
    }
}

/// Serper.dev Google 搜索 API
pub async fn search_serper_public(client: &reqwest::Client, api_key: &str, query: &str) -> Result<String, String> {
    search_serper(client, api_key, query).await
}
async fn search_serper(client: &reqwest::Client, api_key: &str, query: &str) -> Result<String, String> {
    let resp = client.post("https://google.serper.dev/search")
        .header("X-API-KEY", api_key)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({"q": query, "num": 5}))
        .send().await.map_err(|e| format!("请求失败: {}", e))?;

    let data: serde_json::Value = resp.json().await.map_err(|e| format!("解析失败: {}", e))?;

    let mut results = Vec::new();

    // 精选摘要
    if let Some(answer) = data["answerBox"]["answer"].as_str() {
        results.push(format!("**精选答案:** {}", answer));
    } else if let Some(snippet) = data["answerBox"]["snippet"].as_str() {
        results.push(format!("**精选摘要:** {}", snippet));
    }

    // 知识面板
    if let Some(kg) = data["knowledgeGraph"].as_object() {
        if let (Some(title), Some(desc)) = (kg.get("title").and_then(|t| t.as_str()), kg.get("description").and_then(|d| d.as_str())) {
            results.push(format!("**{}:** {}", title, desc));
        }
    }

    // 搜索结果
    if let Some(organic) = data["organic"].as_array() {
        for (i, item) in organic.iter().take(5).enumerate() {
            let title = item["title"].as_str().unwrap_or("");
            let snippet = item["snippet"].as_str().unwrap_or("");
            let link = item["link"].as_str().unwrap_or("");
            results.push(format!("{}. **{}**\n   {}\n   {}", i + 1, title, snippet, link));
        }
    }

    if results.is_empty() {
        Ok(format!("搜索 \"{}\" 无结果。", query))
    } else {
        Ok(format!("搜索 \"{}\" 的结果:\n\n{}", query, results.join("\n\n")))
    }
}

/// DuckDuckGo Instant Answer API（免费）
pub async fn search_duckduckgo_public(client: &reqwest::Client, query: &str) -> Result<String, String> {
    search_duckduckgo(client, query).await
}
async fn search_duckduckgo(client: &reqwest::Client, query: &str) -> Result<String, String> {
    // DuckDuckGo Instant Answer API
    let url = format!("https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1", urlencoding::encode(query));
    let resp = client.get(&url)
        .header("User-Agent", "XianZhu/0.2 (AI Assistant)")
        .send().await.map_err(|e| format!("请求失败: {}", e))?;

    let data: serde_json::Value = resp.json().await.map_err(|e| format!("解析失败: {}", e))?;

    let mut results = Vec::new();

    // 摘要答案
    if let Some(abstract_text) = data["AbstractText"].as_str() {
        if !abstract_text.is_empty() {
            let source = data["AbstractSource"].as_str().unwrap_or("");
            let url = data["AbstractURL"].as_str().unwrap_or("");
            results.push(format!("**{}:** {}\n{}", source, abstract_text, url));
        }
    }

    // 相关话题
    if let Some(topics) = data["RelatedTopics"].as_array() {
        for (i, topic) in topics.iter().take(5).enumerate() {
            if let Some(text) = topic["Text"].as_str() {
                let url = topic["FirstURL"].as_str().unwrap_or("");
                results.push(format!("{}. {}\n   {}", i + 1, text, url));
            }
        }
    }

    // Infobox
    if let Some(answer) = data["Answer"].as_str() {
        if !answer.is_empty() {
            results.insert(0, format!("**答案:** {}", answer));
        }
    }

    if results.is_empty() {
        // DuckDuckGo Instant Answer 可能无结果，尝试 DuckDuckGo Lite HTML
        search_duckduckgo_lite(client, query).await
    } else {
        Ok(format!("搜索 \"{}\" 的结果:\n\n{}", query, results.join("\n\n")))
    }
}

/// DuckDuckGo Lite HTML 爬取（备用方案）
/// Tavily AI 搜索 API
pub async fn search_tavily_public(client: &reqwest::Client, api_key: &str, query: &str) -> Result<String, String> {
    search_tavily(client, api_key, query).await
}
async fn search_tavily(client: &reqwest::Client, api_key: &str, query: &str) -> Result<String, String> {
    let resp = client
        .post("https://api.tavily.com/search")
        .json(&serde_json::json!({
            "api_key": api_key,
            "query": query,
            "max_results": 5,
            "include_answer": true,
        }))
        .send()
        .await
        .map_err(|e| format!("Tavily 请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Tavily 返回 {}", resp.status()));
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| format!("解析失败: {}", e))?;

    let mut results = Vec::new();

    // AI 回答摘要
    if let Some(answer) = data["answer"].as_str() {
        if !answer.is_empty() {
            results.push(format!("**AI 摘要:** {}", answer));
        }
    }

    // 搜索结果
    if let Some(items) = data["results"].as_array() {
        for item in items.iter().take(5) {
            let title = item["title"].as_str().unwrap_or("");
            let url = item["url"].as_str().unwrap_or("");
            let content = item["content"].as_str().unwrap_or("");
            results.push(format!("**{}**\n{}\n{}", title, url, content));
        }
    }

    if results.is_empty() {
        Err("Tavily 无结果".to_string())
    } else {
        Ok(results.join("\n\n"))
    }
}

/// Brave Search API（https://brave.com/search/api/）
async fn search_brave(client: &reqwest::Client, api_key: &str, query: &str, count: usize) -> Result<String, String> {
    let resp = client.get("https://api.search.brave.com/res/v1/web/search")
        .header("X-Subscription-Token", api_key)
        .header("Accept", "application/json")
        .query(&[("q", query), ("count", &count.to_string())])
        .send().await.map_err(|e| format!("Brave 请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Brave 返回 {}", resp.status()));
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| format!("解析失败: {}", e))?;
    let mut results = Vec::new();

    if let Some(web) = data["web"]["results"].as_array() {
        for (i, item) in web.iter().take(count).enumerate() {
            let title = item["title"].as_str().unwrap_or("");
            let desc = item["description"].as_str().unwrap_or("");
            let url = item["url"].as_str().unwrap_or("");
            results.push(format!("{}. **{}**\n   {}\n   {}", i + 1, title, desc, url));
        }
    }

    if results.is_empty() {
        Err("Brave 无结果".into())
    } else {
        Ok(format!("[Brave] 搜索 \"{}\" 的结果:\n\n{}", query, results.join("\n\n")))
    }
}

/// Exa AI 搜索（https://exa.ai/ — 神经搜索 + 内容提取）
async fn search_exa(client: &reqwest::Client, api_key: &str, query: &str, count: usize) -> Result<String, String> {
    let resp = client.post("https://api.exa.ai/search")
        .header("x-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "query": query,
            "numResults": count,
            "type": "auto",
            "contents": {
                "text": { "maxCharacters": 500 },
                "highlights": true,
            }
        }))
        .send().await.map_err(|e| format!("Exa 请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Exa 返回 {}", resp.status()));
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| format!("解析失败: {}", e))?;
    let mut results = Vec::new();

    if let Some(items) = data["results"].as_array() {
        for (i, item) in items.iter().take(count).enumerate() {
            let title = item["title"].as_str().unwrap_or("");
            let url = item["url"].as_str().unwrap_or("");
            // Exa 返回 text 或 highlights
            let text = item["text"].as_str().unwrap_or("");
            let highlight = item["highlights"].as_array()
                .and_then(|h| h.first())
                .and_then(|h| h.as_str())
                .unwrap_or("");
            let snippet = if !highlight.is_empty() { highlight } else { text };
            results.push(format!("{}. **{}**\n   {}\n   {}", i + 1, title, snippet, url));
        }
    }

    if results.is_empty() {
        Err("Exa 无结果".into())
    } else {
        Ok(format!("[Exa] 搜索 \"{}\" 的结果:\n\n{}", query, results.join("\n\n")))
    }
}

/// Firecrawl 搜索（https://www.firecrawl.dev/ — 搜索 + 网页抓取）
async fn search_firecrawl(client: &reqwest::Client, api_key: &str, query: &str, count: usize) -> Result<String, String> {
    let resp = client.post("https://api.firecrawl.dev/v1/search")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "query": query,
            "limit": count,
        }))
        .send().await.map_err(|e| format!("Firecrawl 请求失败: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Firecrawl 返回 {}", resp.status()));
    }

    let data: serde_json::Value = resp.json().await.map_err(|e| format!("解析失败: {}", e))?;
    let mut results = Vec::new();

    if let Some(items) = data["data"].as_array() {
        for (i, item) in items.iter().take(count).enumerate() {
            let title = item["metadata"]["title"].as_str()
                .or(item["title"].as_str()).unwrap_or("");
            let url = item["url"].as_str().unwrap_or("");
            let desc = item["metadata"]["description"].as_str()
                .or(item["description"].as_str()).unwrap_or("");
            results.push(format!("{}. **{}**\n   {}\n   {}", i + 1, title, desc, url));
        }
    }

    if results.is_empty() {
        Err("Firecrawl 无结果".into())
    } else {
        Ok(format!("[Firecrawl] 搜索 \"{}\" 的结果:\n\n{}", query, results.join("\n\n")))
    }
}

async fn search_duckduckgo_lite(client: &reqwest::Client, query: &str) -> Result<String, String> {
    // 使用 DuckDuckGo HTML 搜索（POST 方式，模拟浏览器，避免 bot 检测）
    let resp = client.post("https://html.duckduckgo.com/html/")
        .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
        .header("Referer", "https://html.duckduckgo.com/")
        .header("Accept", "text/html,application/xhtml+xml")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("q={}&b=&kl=", urlencoding::encode(query)))
        .send().await.map_err(|e| format!("请求失败: {}", e))?;

    let html = resp.text().await.map_err(|e| format!("读取失败: {}", e))?;

    // 检查是否被 bot 检测拦截
    if html.contains("anomaly-modal") || html.contains("botnet") {
        return Err("DuckDuckGo bot 检测拦截，请配置搜索 API Key（推荐 Serper/Tavily）".into());
    }

    let mut results = Vec::new();
    let mut count = 0;

    // 提取 result__a 链接和 result__snippet 摘要
    // DuckDuckGo HTML 格式：<a class="result__a" href="...">title</a> ... <a class="result__snippet">snippet</a>
    let mut i = 0;
    let chars: Vec<char> = html.chars().collect();
    let html_len = chars.len();

    while i < html_len && count < 5 {
        // 查找 result__a
        if let Some(pos) = html[i..].find("result__a") {
            let abs_pos = i + pos;
            // 找 href
            if let Some(href_start) = html[abs_pos..].find("href=\"") {
                let href_begin = abs_pos + href_start + 6;
                if let Some(href_end) = html[href_begin..].find('"') {
                    let url = &html[href_begin..href_begin + href_end];
                    // 找标题（> 和 </a> 之间）
                    if let Some(gt) = html[href_begin..].find('>') {
                        let title_begin = href_begin + gt + 1;
                        if let Some(end_a) = html[title_begin..].find("</a>") {
                            let title = html[title_begin..title_begin + end_a]
                                .replace("<b>", "").replace("</b>", "").trim().to_string();
                            // 找摘要 result__snippet
                            let search_from = title_begin + end_a;
                            let snippet = if let Some(snip_pos) = html[search_from..].find("result__snippet") {
                                let snip_abs = search_from + snip_pos;
                                if let Some(snip_gt) = html[snip_abs..].find('>') {
                                    let snip_begin = snip_abs + snip_gt + 1;
                                    if let Some(snip_end) = html[snip_begin..].find("</a>").or_else(|| html[snip_begin..].find("</td>")) {
                                        html[snip_begin..snip_begin + snip_end]
                                            .replace("<b>", "").replace("</b>", "").trim().to_string()
                                    } else { String::new() }
                                } else { String::new() }
                            } else { String::new() };

                            if !title.is_empty() {
                                count += 1;
                                let clean_url = if url.starts_with("//duckduckgo.com/l/?uddg=") {
                                    urlencoding::decode(url.trim_start_matches("//duckduckgo.com/l/?uddg=").split('&').next().unwrap_or(""))
                                        .unwrap_or_default().to_string()
                                } else { url.to_string() };
                                results.push(format!("{}. **{}**\n   {}\n   {}", count, title, snippet, clean_url));
                            }
                            i = title_begin + end_a;
                            continue;
                        }
                    }
                }
            }
            i = abs_pos + 10;
        } else {
            break;
        }
    }

    if results.is_empty() {
        Err("DuckDuckGo 无搜索结果，请配置搜索 API Key（推荐 Serper: https://serper.dev 或 Tavily: https://tavily.com）".into())
    } else {
        Ok(format!("搜索 \"{}\" 的结果:\n\n{}", query, results.join("\n\n")))
    }
}

/// Bash 命令执行工具
///
/// 在沙箱中执行 shell 命令，支持超时、命令白名单等安全控制。
/// 自动注入 Node.js 运行时 PATH（如已安装）。
pub struct BashExecTool;

#[async_trait]
impl Tool for BashExecTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "bash_exec".to_string(),
            description: "在沙箱环境中执行 Shell 命令。适用于：安装软件包（npm/pip/brew）、git 操作、文件批量处理、运行脚本、系统管理。当其他专用工具无法满足需求时，bash_exec 是万能后备。注意：长时间运行的命令请设置合理的 timeout。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "要执行的完整命令，如 'ls -la /tmp' 或 'node -e \"console.log(1+1)\"'"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Sandboxed
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let raw_command = arguments
            .get("command")
            .and_then(|c| c.as_str())
            .ok_or("缺少 command 参数")?;

        // Python 沙箱：将 python3/python/pip 命令重写为沙箱路径
        let command = crate::agent::python_sandbox::rewrite_python_command(raw_command);
        let command = command.as_str();

        // Shell 安全守卫
        crate::agent::sandbox::ShellGuard::validate_command(command)?;

        // 环境变量清洗
        let mut safe_env = crate::agent::sandbox::EnvSanitizer::sanitized_env();

        log::info!("执行 bash 命令: {}", command);

        // 构建 PATH：注入 Node.js 运行时 + brew + bun + 用户本地
        let mut env_path = safe_env.get("PATH").cloned().unwrap_or_default();
        let node_rt = crate::runtime::NodeRuntime::new();
        if node_rt.is_installed().await {
            let bin_dir = node_rt.bin_dir();
            env_path = format!("{}:{}", bin_dir.to_string_lossy(), env_path);
        }
        // 补充常见工具路径（brew/bun/npm 全局/用户本地）
        if let Some(home) = dirs::home_dir() {
            let extra = format!(
                "{}:{}:{}:/opt/homebrew/bin:/usr/local/bin",
                home.join(".bun/bin").to_string_lossy(),
                home.join(".local/bin").to_string_lossy(),
                home.join(".npm-global/bin").to_string_lossy(),
            );
            env_path = format!("{}:{}", extra, env_path);
        }
        // Python 沙箱：将 venv/bin 注入 PATH 最前面，使 python3/pip3 自动走沙箱
        for (key, val) in crate::agent::python_sandbox::sandbox_env() {
            if key == "_XIANZHU_PYTHON_BIN" {
                let sep = if cfg!(target_os = "windows") { ";" } else { ":" };
                env_path = format!("{}{}{}", val, sep, env_path);
            } else {
                safe_env.insert(key, val);
            }
        }

        // 动态超时：pip/npm/cargo 等长命令给更多时间
        let timeout_secs = if command.contains("pip install") || command.contains("npm install")
            || command.contains("cargo build") || command.contains("cargo install")
            || command.contains("brew install") || command.contains("apt install")
        { 300 } else { 120 };

        // 跨平台 Shell 执行（支持管道、重定向等）
        #[cfg(windows)]
        let mut shell_cmd = {
            let mut cmd = tokio::process::Command::new("cmd");
            cmd.args(&["/C", command]);
            cmd
        };
        #[cfg(not(windows))]
        let mut shell_cmd = {
            let mut cmd = tokio::process::Command::new("sh");
            cmd.args(&["-c", command]);
            cmd
        };

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            shell_cmd
                .env_clear()
                .envs(&safe_env)
                .env("PATH", &env_path)
                .output(),
        )
        .await
        .map_err(|_| format!("命令执行超时（{}秒）", timeout_secs))?
        .map_err(|e| format!("命令执行失败: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            if stdout.is_empty() && !stderr.is_empty() {
                Ok(format!("[stderr]\n{}", stderr))
            } else {
                Ok(stdout)
            }
        } else {
            if !stderr.is_empty() {
                Err(format!("命令返回错误 (exit {}):\n{}", output.status.code().unwrap_or(-1), stderr))
            } else {
                Err(format!("命令返回错误 (exit {})\n{}", output.status.code().unwrap_or(-1), stdout))
            }
        }
    }
}

/// 文件写入工具
pub struct FileWriteTool;

#[async_trait]
impl Tool for FileWriteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_write".to_string(),
            description: "创建新文件或覆盖已有文件。适用于：创建配置文件、写入代码、保存下载内容。注意：会覆盖已有内容！如需修改已有文件的部分内容，优先使用 file_edit。自动创建父目录。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "文件路径"
                    },
                    "content": {
                        "type": "string",
                        "description": "要写入的内容"
                    }
                },
                "required": ["path", "content"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Guarded
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let path = arguments
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or("缺少 path 参数")?;
        let content = arguments
            .get("content")
            .and_then(|c| c.as_str())
            .ok_or("缺少 content 参数")?;

        // 统一路径安全校验
        validate_path_safety(path)?;

        log::info!("写入文件: {} ({} 字节)", path, content.len());

        // Harness: 编辑前备份（如果文件已存在）
        super::super::file_harness::backup_before_edit(path);

        // 自动创建父目录
        let file_path = std::path::Path::new(path);
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("创建目录失败: {}", e))?;
        }

        tokio::fs::write(path, content)
            .await
            .map_err(|e| format!("写入文件失败: {}", e))?;

        // Harness: 注册新内容 hash
        super::super::file_harness::update_hash(path, content);

        Ok(format!("文件已写入: {} ({} 字节)", path, content.len()))
    }
}

/// 目录列表工具
pub struct FileListTool;

#[async_trait]
impl Tool for FileListTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_list".to_string(),
            description: "列出目录内容。适用于：了解项目结构、查找文件位置、确认文件是否存在。建议在 file_read 之前先用 file_list 确认路径。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "目录路径"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Guarded
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let path = arguments
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or("缺少 path 参数")?;

        log::info!("列出目录: {}", path);
        validate_path_safety(path)?;

        let mut entries = tokio::fs::read_dir(path)
            .await
            .map_err(|e| format!("读取目录失败: {}", e))?;

        let mut items: Vec<String> = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
            let file_type = entry.file_type().await.map_err(|e| e.to_string())?;
            // 安全: 标记符号链接（不隐藏，但提示用户）
            let name = entry.file_name().to_string_lossy().to_string();
            let marker = if file_type.is_symlink() {
                " -> [symlink]"
            } else if file_type.is_dir() {
                "/"
            } else {
                ""
            };
            items.push(format!("{}{}", name, marker));
        }

        items.sort();
        Ok(items.join("\n"))
    }
}

/// 文件编辑工具 — 精准替换文件中的文本片段
pub struct FileEditTool;

#[async_trait]
impl Tool for FileEditTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_edit".to_string(),
            description: "修改已有文件的部分内容（查找替换）。适用于：修改配置项、修复代码 bug、更新文本。需要提供 old_text（要替换的原文）和 new_text（替换后的内容）。old_text 必须能在文件中精确匹配。支持多行文本。如果 old_text 为空则在 insert_line 位置插入 new_text。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "文件路径"
                    },
                    "old_text": {
                        "type": "string",
                        "description": "要替换的原始文本（精确匹配）。为空时使用 insert_line 插入模式"
                    },
                    "new_text": {
                        "type": "string",
                        "description": "替换后的新文本"
                    },
                    "insert_line": {
                        "type": "integer",
                        "description": "插入模式：在指定行号之后插入 new_text（仅当 old_text 为空时使用，0 表示文件开头）"
                    }
                },
                "required": ["path", "new_text"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Guarded
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let path = arguments.get("path").and_then(|p| p.as_str())
            .ok_or("缺少 path 参数")?;
        let new_text = arguments.get("new_text").and_then(|n| n.as_str())
            .ok_or("缺少 new_text 参数")?;
        let old_text = arguments.get("old_text").and_then(|o| o.as_str()).unwrap_or("");

        // 统一路径安全校验
        validate_path_safety(path)?;

        let content = tokio::fs::read_to_string(path).await
            .map_err(|e| format!("读取文件失败: {}", e))?;

        // Harness: 校验文件是否被外部修改（自上次 file_read 后）
        super::super::file_harness::verify_before_edit(path, &content)?;

        // Harness: 编辑前自动备份
        super::super::file_harness::backup_before_edit(path);

        let new_content = if old_text.is_empty() {
            // 插入模式
            let insert_line = arguments.get("insert_line")
                .and_then(|l| l.as_i64()).unwrap_or(0) as usize;
            let lines: Vec<&str> = content.lines().collect();
            let insert_at = insert_line.min(lines.len());
            let mut result_lines: Vec<&str> = Vec::with_capacity(lines.len() + 1);
            result_lines.extend_from_slice(&lines[..insert_at]);
            // 收集 new_text 的行
            let new_lines: Vec<&str> = new_text.lines().collect();
            result_lines.extend(new_lines.iter());
            result_lines.extend_from_slice(&lines[insert_at..]);
            result_lines.join("\n") + if content.ends_with('\n') { "\n" } else { "" }
        } else {
            // 替换模式
            let count = content.matches(old_text).count();
            if count == 0 {
                return Err(format!("未找到匹配文本，文件未修改。搜索文本前50字符: '{}'", &old_text[..old_text.len().min(50)]));
            }
            if count > 1 {
                return Err(format!("找到 {} 处匹配，请提供更精确的文本以避免歧义", count));
            }
            content.replacen(old_text, new_text, 1)
        };

        tokio::fs::write(path, &new_content).await
            .map_err(|e| format!("写入文件失败: {}", e))?;

        // 安全: 写后验证（确认写入内容与预期一致）
        let verify = tokio::fs::read_to_string(path).await
            .map_err(|e| format!("写后验证读取失败: {}", e))?;
        if !old_text.is_empty() && verify.contains(old_text) && !new_text.contains(old_text) {
            log::warn!("file_edit 写后验证: old_text 仍存在于文件中（可能写入失败）");
        }
        if !new_text.is_empty() && !verify.contains(new_text) {
            log::warn!("file_edit 写后验证: new_text 未出现在文件中（写入可能异常）");
            return Err(format!("写后验证失败：new_text 未出现在文件中。当前文件前 500 字符:\n{}", &verify[..verify.len().min(500)]));
        }

        // Harness: 更新 hash
        super::super::file_harness::update_hash(path, &new_content);

        log::info!("文件已编辑: {}", path);
        Ok(format!("文件已编辑: {} (新大小: {} 字节)", path, new_content.len()))
    }
}

/// Diff-based 文件���辑工具
///
/// 接受 unified diff 格式，应用到目标文件
pub struct DiffEditTool;

#[async_trait]
impl Tool for DiffEditTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "diff_edit".to_string(),
            description: "使用 unified diff 格式编辑文件。输入 diff 内容和目标文件路径。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "目标文件路径"
                    },
                    "diff": {
                        "type": "string",
                        "description": "unified diff 格式的变更内容"
                    }
                },
                "required": ["file_path", "diff"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Guarded
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let file_path = arguments["file_path"].as_str()
            .ok_or("缺少 file_path 参数")?;
        let diff = arguments["diff"].as_str()
            .ok_or("缺少 diff 参数")?;

        // 统一路径安全校验
        validate_path_safety(file_path)?;

        // 读取原文件
        let original = tokio::fs::read_to_string(file_path).await
            .map_err(|e| format!("读取文件失败: {}", e))?;

        // 应用 diff
        let patched = apply_unified_diff(&original, diff)?;

        // 写回文件
        tokio::fs::write(file_path, &patched).await
            .map_err(|e| format!("写入文件失败: {}", e))?;

        let lines_changed = diff.lines()
            .filter(|l| l.starts_with('+') || l.starts_with('-'))
            .filter(|l| !l.starts_with("+++") && !l.starts_with("---"))
            .count();

        Ok(format!("文件 {} 已更新，变更 {} 行", file_path, lines_changed))
    }
}

/// 文件回滚工具 — 从备份恢复文件
pub struct FileRollbackTool;

#[async_trait]
impl Tool for FileRollbackTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_rollback".to_string(),
            description: "从备份恢复文件。可查看备份列表或将指定备份恢复到原路径。每次 file_edit/file_write 会自动创建备份。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "操作：list（查看备份列表）或 restore（恢复备份）",
                        "enum": ["list", "restore"]
                    },
                    "backup_path": {
                        "type": "string",
                        "description": "备份文件路径（restore 时必填，从 list 结果中获取）"
                    },
                    "target_path": {
                        "type": "string",
                        "description": "恢复目标路径（restore 时必填）"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Guarded }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let action = arguments["action"].as_str().unwrap_or("list");
        match action {
            "list" => {
                let backups = super::super::file_harness::list_backups();
                if backups.is_empty() { return Ok("没有可用的备份。".into()); }
                let lines: Vec<String> = backups.iter().take(20)
                    .map(|(path, name, size)| format!("- `{}` ({:.1}KB)\n  路径: {}", name, *size as f64 / 1024.0, path))
                    .collect();
                Ok(format!("最近 {} 个备份：\n{}", lines.len(), lines.join("\n")))
            }
            "restore" => {
                let backup = arguments["backup_path"].as_str().ok_or("缺少 backup_path")?;
                let target = arguments["target_path"].as_str().ok_or("缺少 target_path")?;
                validate_path_safety(target)?;
                super::super::file_harness::rollback(backup, target)?;
                Ok(format!("已恢复: {} → {}", backup, target))
            }
            _ => Err(format!("未知操作: {}。支持: list/restore", action)),
        }
    }
}

/// 应用 unified diff 到原始文本
///
/// 简化版：解析 @@ 行获取位置，应用增删
fn apply_unified_diff(original: &str, diff: &str) -> Result<String, String> {
    let original_lines: Vec<&str> = original.lines().collect();
    let mut result_lines: Vec<String> = original_lines.iter().map(|s| s.to_string()).collect();

    let mut offset: i64 = 0; // 累计偏移量

    for hunk in parse_hunks(diff) {
        let start = ((hunk.old_start as i64 - 1) + offset) as usize;

        // 移除旧行
        let end = (start + hunk.old_count).min(result_lines.len());
        result_lines.drain(start..end);

        // 插入新行
        for (i, line) in hunk.new_lines.iter().enumerate() {
            result_lines.insert(start + i, line.clone());
        }

        offset += hunk.new_count as i64 - hunk.old_count as i64;
    }

    Ok(result_lines.join("\n"))
}

struct DiffHunk {
    old_start: usize,
    old_count: usize,
    new_count: usize,
    new_lines: Vec<String>,
}

fn parse_hunks(diff: &str) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut current_hunk: Option<DiffHunk> = None;

    for line in diff.lines() {
        if line.starts_with("@@") {
            // 保存前一个 hunk
            if let Some(h) = current_hunk.take() {
                hunks.push(h);
            }

            // 解析 @@ -old_start,old_count +new_start,new_count @@
            if let Some((old_start, old_count, new_count)) = parse_hunk_header(line) {
                current_hunk = Some(DiffHunk {
                    old_start,
                    old_count,
                    new_count,
                    new_lines: Vec::new(),
                });
            }
        } else if let Some(ref mut hunk) = current_hunk {
            if line.starts_with('+') && !line.starts_with("+++") {
                hunk.new_lines.push(line[1..].to_string());
            } else if line.starts_with('-') && !line.starts_with("---") {
                // 删除行，不加入 new_lines
            } else if line.starts_with(' ') {
                // 上下文行
                hunk.new_lines.push(line[1..].to_string());
            } else if !line.starts_with("---") && !line.starts_with("+++") {
                // 无前缀的上下文行
                hunk.new_lines.push(line.to_string());
            }
        }
    }

    if let Some(h) = current_hunk {
        hunks.push(h);
    }

    hunks
}

fn parse_hunk_header(line: &str) -> Option<(usize, usize, usize)> {
    // @@ -1,5 +1,7 @@
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 4 { return None; }

    let old_part = parts[1].trim_start_matches('-');
    let new_part = parts[2].trim_start_matches('+');

    let parse_range = |s: &str| -> (usize, usize) {
        if let Some((start, count)) = s.split_once(',') {
            (start.parse().unwrap_or(1), count.parse().unwrap_or(1))
        } else {
            (s.parse().unwrap_or(1), 1)
        }
    };

    let (old_start, old_count) = parse_range(old_part);
    let (_, new_count) = parse_range(new_part);

    Some((old_start, old_count, new_count))
}

/// 代码搜索工具 — 在目录中搜索匹配文本
pub struct CodeSearchTool;

#[async_trait]
impl Tool for CodeSearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "code_search".to_string(),
            description: "在指定目录中搜索包含关键词的代码和文本。适用于：查找函数定义、定位 bug、追踪引用。支持正则表达式和文件类型过滤。返回匹配行及其文件路径和行号。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "搜索关键词或正则表达式"
                    },
                    "path": {
                        "type": "string",
                        "description": "搜索目录路径，默认当前目录"
                    },
                    "file_pattern": {
                        "type": "string",
                        "description": "文件名过滤（如 '*.rs', '*.py'），默认搜索所有文本文件"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "最大返回结果数，默认 50"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Guarded
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let pattern = arguments.get("pattern").and_then(|p| p.as_str())
            .ok_or("缺少 pattern 参数")?;
        let path = arguments.get("path").and_then(|p| p.as_str()).unwrap_or(".");
        validate_path_safety(path)?;
        let file_pattern = arguments.get("file_pattern").and_then(|f| f.as_str()).unwrap_or("");
        let max_results = arguments.get("max_results").and_then(|m| m.as_i64()).unwrap_or(50) as usize;

        log::info!("代码搜索: pattern='{}', path='{}', file_pattern='{}'", pattern, path, file_pattern);

        // 构建 grep 命令
        let mut cmd = tokio::process::Command::new("grep");
        cmd.arg("-rn")        // 递归 + 行号
            .arg("--color=never")
            .arg("-I");        // 跳过二进制文件

        if !file_pattern.is_empty() {
            cmd.arg("--include").arg(file_pattern);
        }

        // 排除常见非源码目录
        for exclude in &["node_modules", ".git", "target", "__pycache__", "dist", "build"] {
            cmd.arg("--exclude-dir").arg(exclude);
        }

        cmd.arg(pattern).arg(path);

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            cmd.output(),
        )
        .await
        .map_err(|_| "搜索超时（15秒）".to_string())?
        .map_err(|e| format!("搜索执行失败: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.is_empty() {
            return Ok("未找到匹配结果".to_string());
        }

        // 截取前 max_results 行
        let lines: Vec<&str> = stdout.lines().take(max_results).collect();
        let total = stdout.lines().count();
        let mut result = lines.join("\n");
        if total > max_results {
            result.push_str(&format!("\n\n... 共 {} 处匹配，已显示前 {}", total, max_results));
        }

        Ok(result)
    }
}

/// 网页获取工具 — HTTP GET 读取网页内容
pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "web_fetch".to_string(),
            description: "获取指定 URL 的网页内容并提取正文。适用于：抓取网页信息、读取 GitHub 文件（优先使用 raw.githubusercontent.com）、获取 API 文档。对于 GitHub 仓库页面，建议获取 raw 文件而不是 HTML 页面。返回内容可能较长，注意提取关键信息。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "要获取的 URL"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "超时秒数，默认 15"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Guarded
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let url = arguments.get("url").and_then(|u| u.as_str())
            .ok_or("缺少 url 参数")?;
        // SSRF 防护：拒绝私有 IP、内网地址、非 HTTP 协议
        if let Ok(parsed) = url::Url::parse(url) {
            // 协议白名单
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(format!("安全限制：只允许 http/https 协议，不允许 {}", parsed.scheme()));
            }
            if let Some(host) = parsed.host_str() {
                let host_lower = host.to_lowercase();
                let is_private = host_lower == "localhost"
                    || host_lower == "127.0.0.1"
                    || host_lower == "0.0.0.0"
                    || host_lower == "::1"
                    || host_lower == "[::1]"
                    || host_lower.starts_with("10.")
                    || host_lower.starts_with("192.168.")
                    || host_lower.starts_with("169.254.")
                    || host_lower.starts_with("fe80:")  // IPv6 链路本地
                    || host_lower.starts_with("fd")     // IPv6 唯一本地
                    || host_lower.starts_with("fc")     // IPv6 唯一本地
                    || host_lower.ends_with(".local")   // mDNS
                    || host_lower.ends_with(".internal")
                    || (host_lower.starts_with("172.") && {
                        host_lower.split('.').nth(1)
                            .and_then(|s| s.parse::<u8>().ok())
                            .map_or(false, |n| (16..=31).contains(&n))
                    });
                if is_private {
                    return Err(format!("安全限制：不允许访问内网地址 {}", host));
                }
            }
        }
        let timeout_secs = arguments.get("timeout_secs").and_then(|t| t.as_i64()).unwrap_or(15) as u64;

        log::info!("获取网页: {} (timeout={}s)", url, timeout_secs);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .user_agent("XianZhu-Agent/0.1")
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

        let resp = client.get(url).send().await
            .map_err(|e| format!("HTTP 请求失败: {}", e))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(format!("HTTP {} {}", status.as_u16(), status.canonical_reason().unwrap_or("")));
        }

        let content_type = resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body = resp.text().await
            .map_err(|e| format!("读取响应体失败: {}", e))?;

        // 截断过长内容
        const MAX_LEN: usize = 100_000;
        let truncated = if body.len() > MAX_LEN {
            format!("{}...\n\n[内容已截断，总长 {} 字节]", &body[..MAX_LEN], body.len())
        } else {
            body
        };

        Ok(format!("[Content-Type: {}]\n\n{}", content_type, truncated))
    }
}

// ─── 数学表达式求值 ─────────────────────────────────────────

/// 简单数学表达式求值（支持 +, -, *, /, 括号）
pub(crate) fn eval_math(expr: &str) -> Result<f64, String> {
    let tokens = tokenize(expr)?;
    let mut pos = 0;
    let result = parse_expr(&tokens, &mut pos)?;
    if pos != tokens.len() {
        return Err(format!("表达式解析未完成，剩余 token: {:?}", &tokens[pos..]));
    }
    Ok(result)
}

#[derive(Debug, Clone)]
enum MathToken {
    Num(f64),
    Op(char),
    LParen,
    RParen,
}

fn tokenize(expr: &str) -> Result<Vec<MathToken>, String> {
    let mut tokens = Vec::new();
    let mut chars = expr.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' => { chars.next(); }
            '0'..='9' | '.' => {
                let mut num_str = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() || d == '.' {
                        num_str.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let n: f64 = num_str.parse().map_err(|_| format!("无效数字: {}", num_str))?;
                tokens.push(MathToken::Num(n));
            }
            '+' | '-' => {
                // 处理一元负号：表达式开头、左括号后、运算符后
                let is_unary = tokens.is_empty()
                    || matches!(tokens.last(), Some(MathToken::LParen) | Some(MathToken::Op(_)));
                if is_unary && c == '-' {
                    chars.next();
                    // 读取后续数字
                    let mut num_str = String::from("-");
                    while let Some(&d) = chars.peek() {
                        if d.is_ascii_digit() || d == '.' {
                            num_str.push(d);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    if num_str == "-" {
                        return Err("无效的一元负号".to_string());
                    }
                    let n: f64 = num_str.parse().map_err(|_| format!("无效数字: {}", num_str))?;
                    tokens.push(MathToken::Num(n));
                } else if is_unary && c == '+' {
                    chars.next(); // 一元正号，跳过
                } else {
                    tokens.push(MathToken::Op(c));
                    chars.next();
                }
            }
            '*' | '/' => {
                tokens.push(MathToken::Op(c));
                chars.next();
            }
            '(' => { tokens.push(MathToken::LParen); chars.next(); }
            ')' => { tokens.push(MathToken::RParen); chars.next(); }
            _ => return Err(format!("无效字符: {}", c)),
        }
    }
    Ok(tokens)
}

/// expr = term (('+' | '-') term)*
fn parse_expr(tokens: &[MathToken], pos: &mut usize) -> Result<f64, String> {
    let mut left = parse_term(tokens, pos)?;
    while *pos < tokens.len() {
        match &tokens[*pos] {
            MathToken::Op('+') => { *pos += 1; left += parse_term(tokens, pos)?; }
            MathToken::Op('-') => { *pos += 1; left -= parse_term(tokens, pos)?; }
            _ => break,
        }
    }
    Ok(left)
}

/// term = factor (('*' | '/') factor)*
fn parse_term(tokens: &[MathToken], pos: &mut usize) -> Result<f64, String> {
    let mut left = parse_factor(tokens, pos)?;
    while *pos < tokens.len() {
        match &tokens[*pos] {
            MathToken::Op('*') => { *pos += 1; left *= parse_factor(tokens, pos)?; }
            MathToken::Op('/') => {
                *pos += 1;
                let right = parse_factor(tokens, pos)?;
                if right == 0.0 { return Err("除以零".to_string()); }
                left /= right;
            }
            _ => break,
        }
    }
    Ok(left)
}

/// factor = Num | '(' expr ')'
fn parse_factor(tokens: &[MathToken], pos: &mut usize) -> Result<f64, String> {
    if *pos >= tokens.len() {
        return Err("表达式不完整".to_string());
    }
    match &tokens[*pos] {
        MathToken::Num(n) => { let v = *n; *pos += 1; Ok(v) }
        MathToken::LParen => {
            *pos += 1;
            let v = parse_expr(tokens, pos)?;
            if *pos >= tokens.len() || !matches!(&tokens[*pos], MathToken::RParen) {
                return Err("缺少右括号".to_string());
            }
            *pos += 1;
            Ok(v)
        }
        _ => Err(format!("意外的 token: {:?}", tokens[*pos])),
    }
}

// ─── 自管理工具 ─────────────────────────────────────────────

/// 设置读写工具 — 让 Agent 能查看和修改系统设置
pub struct SettingsTool {
    pool: sqlx::SqlitePool,
}

impl SettingsTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl Tool for SettingsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "settings_manage".to_string(),
            description: "查看或修改系统设置。可以读取、写入配置项（如嵌入模型、Token 限额等）。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "操作类型",
                        "enum": ["get", "set", "list"]
                    },
                    "key": {
                        "type": "string",
                        "description": "设置项名称（get/set 时必填）"
                    },
                    "value": {
                        "type": "string",
                        "description": "设置值（set 时必填）"
                    },
                    "prefix": {
                        "type": "string",
                        "description": "前缀过滤（list 时可选，如 'embedding_'）"
                    }
                },
                "required": ["action"]
            }),
        }
    }
    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Guarded }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let action = args["action"].as_str().ok_or("缺少 action")?;
        match action {
            "get" => {
                let key = args["key"].as_str().ok_or("缺少 key")?;
                let val: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
                    .bind(key).fetch_optional(&self.pool).await.map_err(|e| e.to_string())?;
                Ok(val.unwrap_or_else(|| format!("设置项 '{}' 不存在", key)))
            }
            "set" => {
                let key = args["key"].as_str().ok_or("缺少 key")?;
                let value = args["value"].as_str().ok_or("缺少 value")?;
                let now = chrono::Utc::now().timestamp_millis();
                sqlx::query("INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at")
                    .bind(key).bind(value).bind(now)
                    .execute(&self.pool).await.map_err(|e| e.to_string())?;
                Ok(format!("已设置 {} = {}", key, value))
            }
            "list" => {
                let prefix = args["prefix"].as_str().unwrap_or("");
                let pattern = format!("{}%", prefix);
                let rows = sqlx::query_as::<_, (String, String)>("SELECT key, value FROM settings WHERE key LIKE ?")
                    .bind(&pattern).fetch_all(&self.pool).await.map_err(|e| e.to_string())?;
                if rows.is_empty() { return Ok("没有匹配的设置项".to_string()); }
                Ok(rows.iter().map(|(k, v)| format!("{} = {}", k, v)).collect::<Vec<_>>().join("\n"))
            }
            _ => Err(format!("未知操作: {}", action)),
        }
    }
}

/// Provider 管理工具 — 让 Agent 能查看和添加 LLM 供应商
pub struct ProviderTool {
    pool: sqlx::SqlitePool,
}

impl ProviderTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl Tool for ProviderTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "provider_manage".to_string(),
            description: "管理 LLM 供应商配置。可以列出、添加、更新供应商（包括 API Key、Base URL、模型列表）。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "操作类型",
                        "enum": ["list", "add", "update"]
                    },
                    "provider": {
                        "type": "object",
                        "description": "供应商配置（add/update 时必填）",
                        "properties": {
                            "id": { "type": "string", "description": "供应商 ID" },
                            "name": { "type": "string", "description": "显示名称" },
                            "apiType": { "type": "string", "description": "API 类型：openai 或 anthropic" },
                            "baseUrl": { "type": "string", "description": "API Base URL" },
                            "apiKey": { "type": "string", "description": "API Key" },
                            "models": {
                                "type": "array",
                                "items": { "type": "object", "properties": { "id": {"type":"string"}, "name": {"type":"string"} } }
                            }
                        }
                    }
                },
                "required": ["action"]
            }),
        }
    }
    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Guarded }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let action = args["action"].as_str().ok_or("缺少 action")?;
        match action {
            "list" => {
                let val: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'providers'")
                    .fetch_optional(&self.pool).await.map_err(|e| e.to_string())?;
                let providers: Vec<serde_json::Value> = val
                    .and_then(|v| serde_json::from_str(&v).ok())
                    .unwrap_or_default();
                let summary: Vec<String> = providers.iter().map(|p| {
                    let name = p["name"].as_str().unwrap_or("?");
                    let enabled = p["enabled"].as_bool().unwrap_or(false);
                    let has_key = p["apiKey"].as_str().map(|k| !k.is_empty()).unwrap_or(false);
                    let models: Vec<&str> = p["models"].as_array()
                        .map(|m| m.iter().filter_map(|x| x["id"].as_str()).collect())
                        .unwrap_or_default();
                    format!("- {} (enabled={}, key={}, models=[{}])", name, enabled, if has_key {"有"} else {"无"}, models.join(", "))
                }).collect();
                Ok(format!("已配置的供应商:\n{}", summary.join("\n")))
            }
            "add" | "update" => {
                let provider = &args["provider"];
                if provider.is_null() { return Err("缺少 provider 配置".to_string()); }

                // 读取现有 providers
                let val: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'providers'")
                    .fetch_optional(&self.pool).await.map_err(|e| e.to_string())?;
                let mut providers: Vec<serde_json::Value> = val
                    .and_then(|v| serde_json::from_str(&v).ok())
                    .unwrap_or_default();

                let id = provider["id"].as_str().unwrap_or(&uuid::Uuid::new_v4().to_string()).to_string();

                // 查找是否已存在
                let existing_idx = providers.iter().position(|p| p["id"].as_str() == Some(&id));

                let mut new_provider = if let Some(idx) = existing_idx {
                    providers[idx].clone()
                } else {
                    serde_json::json!({"id": id, "enabled": true, "models": []})
                };

                // 合并字段
                if let Some(v) = provider["name"].as_str() { new_provider["name"] = serde_json::json!(v); }
                if let Some(v) = provider["apiType"].as_str() { new_provider["apiType"] = serde_json::json!(v); }
                if let Some(v) = provider["baseUrl"].as_str() { new_provider["baseUrl"] = serde_json::json!(v); }
                if let Some(v) = provider["apiKey"].as_str() { if !v.is_empty() { new_provider["apiKey"] = serde_json::json!(v); } }
                if provider["models"].is_array() { new_provider["models"] = provider["models"].clone(); }
                if !new_provider.get("enabled").is_some() { new_provider["enabled"] = serde_json::json!(true); }

                if let Some(idx) = existing_idx {
                    providers[idx] = new_provider;
                } else {
                    providers.push(new_provider);
                }

                // 写回
                let json = serde_json::to_string(&providers).map_err(|e| e.to_string())?;
                let now = chrono::Utc::now().timestamp_millis();
                sqlx::query("INSERT INTO settings (key, value, updated_at) VALUES ('providers', ?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at")
                    .bind(&json).bind(now)
                    .execute(&self.pool).await.map_err(|e| e.to_string())?;

                Ok(format!("供应商 '{}' 已{}", id, if existing_idx.is_some() { "更新" } else { "添加" }))
            }
            _ => Err(format!("未知操作: {}", action)),
        }
    }
}

/// Agent 自身配置工具 — 让 Agent 能修改自己的模型、温度等参数
pub struct AgentSelfConfigTool {
    pool: sqlx::SqlitePool,
}

impl AgentSelfConfigTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl Tool for AgentSelfConfigTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "agent_self_config".to_string(),
            description: "查看或修改当前 Agent 的配置（模型、温度、最大 Token、名称）。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "操作类型",
                        "enum": ["get", "update"]
                    },
                    "agent_id": {
                        "type": "string",
                        "description": "Agent ID"
                    },
                    "model": {
                        "type": "string",
                        "description": "新模型名称（update 时可选）"
                    },
                    "temperature": {
                        "type": "number",
                        "description": "新温度值 0-2（update 时可选）"
                    },
                    "max_tokens": {
                        "type": "integer",
                        "description": "新最大 Token 数（update 时可选）"
                    },
                    "name": {
                        "type": "string",
                        "description": "新名称（update 时可选）"
                    }
                },
                "required": ["action", "agent_id"]
            }),
        }
    }
    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Guarded }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let action = args["action"].as_str().ok_or("缺少 action")?;
        let agent_id = args["agent_id"].as_str().ok_or("缺少 agent_id")?;

        match action {
            "get" => {
                let row = sqlx::query_as::<_, (String, String, Option<f64>, Option<i64>)>(
                    "SELECT name, model, temperature, max_tokens FROM agents WHERE id = ?"
                ).bind(agent_id).fetch_optional(&self.pool).await.map_err(|e| e.to_string())?;

                match row {
                    Some((name, model, temp, max_t)) => Ok(format!(
                        "Agent 配置:\n- 名称: {}\n- 模型: {}\n- 温度: {}\n- 最大Token: {}",
                        name, model, temp.map(|t| format!("{:.1}", t)).unwrap_or("默认".into()),
                        max_t.map(|t| t.to_string()).unwrap_or("默认".into())
                    )),
                    None => Err("Agent 不存在".to_string()),
                }
            }
            "update" => {
                let now = chrono::Utc::now().timestamp_millis();
                let mut updates = Vec::new();

                if let Some(model) = args["model"].as_str() {
                    sqlx::query("UPDATE agents SET model = ?, updated_at = ? WHERE id = ?")
                        .bind(model).bind(now).bind(agent_id)
                        .execute(&self.pool).await.map_err(|e| e.to_string())?;
                    updates.push(format!("模型 → {}", model));
                }
                if let Some(temp) = args["temperature"].as_f64() {
                    sqlx::query("UPDATE agents SET temperature = ?, updated_at = ? WHERE id = ?")
                        .bind(temp).bind(now).bind(agent_id)
                        .execute(&self.pool).await.map_err(|e| e.to_string())?;
                    updates.push(format!("温度 → {:.1}", temp));
                }
                if let Some(max_t) = args["max_tokens"].as_i64() {
                    sqlx::query("UPDATE agents SET max_tokens = ?, updated_at = ? WHERE id = ?")
                        .bind(max_t).bind(now).bind(agent_id)
                        .execute(&self.pool).await.map_err(|e| e.to_string())?;
                    updates.push(format!("最大Token → {}", max_t));
                }
                if let Some(name) = args["name"].as_str() {
                    sqlx::query("UPDATE agents SET name = ?, updated_at = ? WHERE id = ?")
                        .bind(name).bind(now).bind(agent_id)
                        .execute(&self.pool).await.map_err(|e| e.to_string())?;
                    updates.push(format!("名称 → {}", name));
                }

                if updates.is_empty() {
                    Ok("没有需要更新的字段".to_string())
                } else {
                    Ok(format!("Agent 配置已更新:\n{}", updates.join("\n")))
                }
            }
            _ => Err(format!("未知操作: {}", action)),
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// skill_manage — 对话中管理技能（安装/卸载/搜索）
// ═══════════════════════════════════════════════════════════════

pub struct SkillManageTool {
    pool: sqlx::SqlitePool,
}

impl SkillManageTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl Tool for SkillManageTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "skill_manage".to_string(),
            description: "管理 Agent 的技能。支持：列出已安装技能、安装新技能（从市场或 URL）、卸载技能、搜索在线技能市场。安装技能后会自动激活，无需重启。用户说「帮我装个邮件技能」时使用此工具。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "操作类型",
                        "enum": ["list_installed", "list_marketplace", "install", "uninstall", "search_online"]
                    },
                    "agent_id": { "type": "string", "description": "Agent ID" },
                    "skill_name": { "type": "string", "description": "技能名称（install/uninstall 时必填）" },
                    "query": { "type": "string", "description": "搜索关键词（search_online 时使用）" }
                },
                "required": ["action", "agent_id"]
            }),
        }
    }
    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Guarded }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let action = args["action"].as_str().ok_or("缺少 action")?;
        let agent_id = args["agent_id"].as_str().ok_or("缺少 agent_id")?;
        let home = dirs::home_dir().unwrap_or_default();

        match action {
            "list_installed" => {
                let workspace = home.join(".xianzhu").join("agents").join(agent_id).join("skills");
                if !workspace.exists() {
                    return Ok("当前 Agent 暂无已安装技能。可用 action=list_marketplace 查看可安装技能。".into());
                }
                let mut skills = Vec::new();
                if let Ok(entries) = std::fs::read_dir(&workspace) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() {
                            skills.push(entry.file_name().to_string_lossy().to_string());
                        }
                    }
                }
                if skills.is_empty() {
                    Ok("当前 Agent 暂无已安装技能。".into())
                } else {
                    Ok(format!("已安装技能 ({} 个): {}", skills.len(), skills.join(", ")))
                }
            }
            "list_marketplace" => {
                let mp_dir = home.join(".xianzhu").join("marketplace");
                if !mp_dir.exists() { return Ok("本地技能市场为空。用 action=search_online 从在线市场搜索。".into()); }
                let mut skills = Vec::new();
                if let Ok(entries) = std::fs::read_dir(&mp_dir) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() {
                            skills.push(entry.file_name().to_string_lossy().to_string());
                        }
                    }
                }
                if skills.is_empty() { Ok("本地市场为空。".into()) }
                else { Ok(format!("可安装技能 ({} 个): {}\n\n用 action=install, skill_name=<名称> 安装。", skills.len(), skills.join(", "))) }
            }
            "install" => {
                let skill_name = args["skill_name"].as_str().ok_or("缺少 skill_name")?;
                let src = home.join(".xianzhu").join("marketplace").join(skill_name);
                if !src.exists() { return Err(format!("技能 {} 不在本地市场。先用 search_online 下载。", skill_name)); }
                let dst = home.join(".xianzhu").join("agents").join(agent_id).join("skills").join(skill_name);
                if dst.exists() { return Ok(format!("技能 {} 已安装。", skill_name)); }
                let _ = std::fs::create_dir_all(dst.parent().unwrap());
                copy_dir_recursive(&src, &dst).map_err(|e| format!("安装失败: {}", e))?;
                Ok(format!("✅ 技能 {} 已安装！后续对话中会自动使用。", skill_name))
            }
            "uninstall" => {
                let skill_name = args["skill_name"].as_str().ok_or("缺少 skill_name")?;
                let target = home.join(".xianzhu").join("agents").join(agent_id).join("skills").join(skill_name);
                if !target.exists() { return Err(format!("技能 {} 未安装。", skill_name)); }
                std::fs::remove_dir_all(&target).map_err(|e| format!("卸载失败: {}", e))?;
                Ok(format!("✅ 技能 {} 已卸载。", skill_name))
            }
            "search_online" => {
                let query = args["query"].as_str().unwrap_or("");
                let url = if query.is_empty() {
                    "https://zys-openclaw.com/api/v1/skill-hub/search".to_string()
                } else {
                    format!("https://zys-openclaw.com/api/v1/skill-hub/search?q={}", urlencoding::encode(query))
                };
                let resp = reqwest::Client::new().get(&url).send().await.map_err(|e| format!("搜索失败: {}", e))?;
                let data: serde_json::Value = resp.json().await.map_err(|e| format!("解析失败: {}", e))?;
                match data["skills"].as_array() {
                    Some(arr) if !arr.is_empty() => {
                        let list: Vec<String> = arr.iter().take(10).map(|s| {
                            format!("- {} (v{}) — {}", s["name"].as_str().unwrap_or(""), s["version"].as_str().unwrap_or(""), s["description"].as_str().unwrap_or(""))
                        }).collect();
                        Ok(format!("在线技能 ({} 个):\n{}", arr.len(), list.join("\n")))
                    }
                    _ => Ok("在线市场没有找到匹配的技能。".into()),
                }
            }
            _ => Err(format!("未知操作: {}", action)),
        }
    }
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if s.is_dir() { copy_dir_recursive(&s, &d)?; } else { std::fs::copy(&s, &d)?; }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════
// cron_manage — 对话中管理定时任务
// ═══════════════════════════════════════════════════════════════

pub struct CronManageTool {
    pool: sqlx::SqlitePool,
}
impl CronManageTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl Tool for CronManageTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cron_manage".to_string(),
            description: "管理定时任务：创建、列出、暂停、恢复、删除。用户说「每天早上9点帮我查邮件」时使用。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "create", "pause", "resume", "delete", "trigger"] },
                    "agent_id": { "type": "string", "description": "Agent ID" },
                    "job_id": { "type": "string", "description": "任务 ID（pause/resume/delete/trigger 用，支持前缀匹配）" },
                    "name": { "type": "string", "description": "任务名称（create 必填）" },
                    "cron_expr": { "type": "string", "description": "Cron 表达式（create 必填），如 '0 9 * * *'" },
                    "prompt": { "type": "string", "description": "AI 执行指令（create 必填）" },
                    "timezone": { "type": "string", "description": "时区，默认 Asia/Shanghai" },
                    "model": { "type": "string", "description": "指定模型（可选，如 gpt-4o / claude-sonnet-4-6），不填则用 Agent 默认模型" },
                    "thinking": { "type": "string", "description": "推理级别（可选）：off/minimal/low/medium/high", "enum": ["off", "minimal", "low", "medium", "high"] }
                },
                "required": ["action", "agent_id"]
            }),
        }
    }
    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Guarded }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let action = args["action"].as_str().ok_or("缺少 action")?;
        let agent_id = args["agent_id"].as_str().ok_or("缺少 agent_id")?;

        match action {
            "list" => {
                let rows = sqlx::query_as::<_, (String, String, String, bool)>(
                    "SELECT id, name, schedule, enabled FROM cron_jobs WHERE agent_id = ? OR agent_id IS NULL ORDER BY created_at DESC"
                ).bind(agent_id).fetch_all(&self.pool).await.map_err(|e| e.to_string())?;
                if rows.is_empty() { return Ok("暂无定时任务。用 action=create 创建。".into()); }
                let list: Vec<String> = rows.iter().map(|(id, name, sched, enabled)| {
                    format!("{} {} | {} | id:{}", if *enabled {"▶️"} else {"⏸️"}, name, sched, &id[..id.len().min(8)])
                }).collect();
                Ok(format!("定时任务 ({} 个):\n{}", rows.len(), list.join("\n")))
            }
            "create" => {
                let name = args["name"].as_str().ok_or("缺少 name")?;
                let cron_expr = args["cron_expr"].as_str().ok_or("缺少 cron_expr")?;
                let prompt = args["prompt"].as_str().ok_or("缺少 prompt")?;
                let tz = args["timezone"].as_str().unwrap_or("Asia/Shanghai");
                let model = args["model"].as_str();
                let thinking = args["thinking"].as_str();
                let id = uuid::Uuid::new_v4().to_string();
                let now = chrono::Utc::now().timestamp_millis();
                let schedule = serde_json::json!({"kind":"cron","expr":cron_expr,"tz":tz}).to_string();
                let mut payload_obj = serde_json::json!({"type":"agent","prompt":prompt,"sessionStrategy":"new"});
                if let Some(m) = model { payload_obj["model"] = serde_json::json!(m); }
                if let Some(t) = thinking { payload_obj["thinking"] = serde_json::json!(t); }
                let payload = payload_obj.to_string();
                sqlx::query("INSERT INTO cron_jobs (id, agent_id, name, job_type, schedule, action_payload, enabled, timeout_secs, created_at, updated_at) VALUES (?,?,?,'agent',?,?,1,300,?,?)")
                    .bind(&id).bind(agent_id).bind(name).bind(&schedule).bind(&payload).bind(now).bind(now)
                    .execute(&self.pool).await.map_err(|e| format!("创建失败: {}", e))?;
                let model_info = model.map(|m| format!(" | model: {}", m)).unwrap_or_default();
                let thinking_info = thinking.map(|t| format!(" | thinking: {}", t)).unwrap_or_default();
                Ok(format!("✅ 定时任务已创建: {} | {} ({}){}{} | {}", name, cron_expr, tz, model_info, thinking_info, &id[..8]))
            }
            "pause" => {
                let jid = args["job_id"].as_str().ok_or("缺少 job_id")?;
                sqlx::query("UPDATE cron_jobs SET enabled=0,updated_at=? WHERE id LIKE ?||'%'")
                    .bind(chrono::Utc::now().timestamp_millis()).bind(jid)
                    .execute(&self.pool).await.map_err(|e| e.to_string())?;
                Ok(format!("⏸️ 任务 {} 已暂停", jid))
            }
            "resume" => {
                let jid = args["job_id"].as_str().ok_or("缺少 job_id")?;
                sqlx::query("UPDATE cron_jobs SET enabled=1,updated_at=? WHERE id LIKE ?||'%'")
                    .bind(chrono::Utc::now().timestamp_millis()).bind(jid)
                    .execute(&self.pool).await.map_err(|e| e.to_string())?;
                Ok(format!("▶️ 任务 {} 已恢复", jid))
            }
            "delete" => {
                let jid = args["job_id"].as_str().ok_or("缺少 job_id")?;
                sqlx::query("DELETE FROM cron_jobs WHERE id LIKE ?||'%'").bind(jid)
                    .execute(&self.pool).await.map_err(|e| e.to_string())?;
                Ok(format!("🗑️ 任务 {} 已删除", jid))
            }
            "trigger" => {
                let jid = args["job_id"].as_str().ok_or("缺少 job_id")?;
                sqlx::query("UPDATE cron_jobs SET fail_streak=-1,updated_at=? WHERE id LIKE ?||'%'")
                    .bind(chrono::Utc::now().timestamp_millis()).bind(jid)
                    .execute(&self.pool).await.map_err(|e| e.to_string())?;
                Ok(format!("⚡ 任务 {} 已触发", jid))
            }
            _ => Err(format!("未知操作: {}", action)),
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// plugin_manage — 对话中管理插件
// ═══════════════════════════════════════════════════════════════

pub struct PluginManageTool {
    pool: sqlx::SqlitePool,
}
impl PluginManageTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl Tool for PluginManageTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "plugin_manage".to_string(),
            description: "管理系统插件：列出、启用、禁用。用户说「启用 Anthropic」或「看看有哪些插件」时使用。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "enable", "disable"] },
                    "plugin_id": { "type": "string", "description": "插件 ID（enable/disable 时必填）" }
                },
                "required": ["action"]
            }),
        }
    }
    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Guarded }

    async fn execute(&self, args: serde_json::Value) -> Result<String, String> {
        let action = args["action"].as_str().ok_or("缺少 action")?;
        match action {
            "list" => {
                Ok("系统插件:\n\
                    \n**模型提供商:** openai, anthropic, deepseek, qwen, zhipu, moonshot, ollama, vllm\
                    \n**渠道:** telegram-channel, feishu-channel, weixin-channel, discord-channel, slack-channel\
                    \n**记忆:** sqlite-memory, lancedb-vector\
                    \n**嵌入:** openai-embedding\
                    \n\n使用 action=enable/disable, plugin_id=<id> 来管理。\
                    \n模型供应商的详细配置请使用 provider_manage 工具。".to_string())
            }
            "enable" => {
                let pid = args["plugin_id"].as_str().ok_or("缺少 plugin_id")?;
                let now = chrono::Utc::now().timestamp_millis();
                let _ = sqlx::query("INSERT OR REPLACE INTO plugin_states (plugin_id, enabled, updated_at) VALUES (?, 1, ?)")
                    .bind(pid).bind(now).execute(&self.pool).await;
                Ok(format!("✅ 插件 {} 已启用", pid))
            }
            "disable" => {
                let pid = args["plugin_id"].as_str().ok_or("缺少 plugin_id")?;
                let now = chrono::Utc::now().timestamp_millis();
                let _ = sqlx::query("INSERT OR REPLACE INTO plugin_states (plugin_id, enabled, updated_at) VALUES (?, 0, ?)")
                    .bind(pid).bind(now).execute(&self.pool).await;
                Ok(format!("❌ 插件 {} 已禁用", pid))
            }
            _ => Err(format!("未知操作: {}", action)),
        }
    }
}

/// 图片生成工具 — 支持 DALL-E 3 和 OpenAI 兼容接口
pub struct ImageGenerateTool {
    pool: sqlx::SqlitePool,
}

impl ImageGenerateTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Tool for ImageGenerateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "image_generate".to_string(),
            description: "生成图片。根据文字描述生成图片，支持 DALL-E 3。返回图片 URL。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "图片描述（英文效果更好）"
                    },
                    "size": {
                        "type": "string",
                        "description": "图片尺寸（可选）：1024x1024 / 1792x1024 / 1024x1792",
                        "default": "1024x1024"
                    },
                    "quality": {
                        "type": "string",
                        "description": "质量（可选）：standard / hd",
                        "default": "standard"
                    }
                },
                "required": ["prompt"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Guarded
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let prompt = arguments.get("prompt")
            .and_then(|p| p.as_str())
            .ok_or("缺少 prompt 参数")?;
        let size = arguments.get("size").and_then(|s| s.as_str()).unwrap_or("1024x1024");
        let quality = arguments.get("quality").and_then(|q| q.as_str()).unwrap_or("standard");

        log::info!("生成图片: {} (size={}, quality={})", &prompt[..prompt.len().min(50)], size, quality);

        // 从 provider 配置中查找 OpenAI 兼容的图片生成端点
        let (api_key, base_url) = self.find_image_provider().await?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| format!("创建客户端失败: {}", e))?;

        let url = format!("{}/images/generations", base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": "dall-e-3",
            "prompt": prompt,
            "n": 1,
            "size": size,
            "quality": quality,
        });

        let resp = client.post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("请求失败: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("图片生成失败 (HTTP {}): {}", status, &text[..text.len().min(200)]));
        }

        let data: serde_json::Value = resp.json().await.map_err(|e| format!("解析失败: {}", e))?;

        if let Some(url) = data["data"][0]["url"].as_str() {
            let revised_prompt = data["data"][0]["revised_prompt"].as_str().unwrap_or("");
            let mut result = format!("![Generated Image]({})", url);
            if !revised_prompt.is_empty() {
                result.push_str(&format!("\n\n*Revised prompt: {}*", revised_prompt));
            }
            Ok(result)
        } else {
            Err("图片生成返回格式异常".to_string())
        }
    }
}

impl ImageGenerateTool {
    /// 从 DB 查找支持图片生成的 provider（优先 OpenAI）
    async fn find_image_provider(&self) -> Result<(String, String), String> {
        let json_str: Option<String> = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = 'providers'"
        ).fetch_optional(&self.pool).await.ok().flatten();

        let providers: Vec<serde_json::Value> = json_str
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        // 优先找 OpenAI（原生支持 DALL-E）
        for p in &providers {
            if p["enabled"].as_bool() != Some(true) { continue; }
            let key = p["apiKey"].as_str().unwrap_or("");
            if key.is_empty() { continue; }
            let api_type = p["apiType"].as_str().unwrap_or("");
            let base_url = p["baseUrl"].as_str().unwrap_or("");

            // OpenAI 原生或兼容端点
            if api_type == "openai" && (base_url.contains("openai.com") || base_url.is_empty()) {
                return Ok((
                    key.to_string(),
                    if base_url.is_empty() { "https://api.openai.com/v1".to_string() } else { base_url.to_string() },
                ));
            }
        }

        // 回退：任何有 apiKey 的 OpenAI 兼容 provider
        for p in &providers {
            if p["enabled"].as_bool() != Some(true) { continue; }
            let key = p["apiKey"].as_str().unwrap_or("");
            if key.is_empty() { continue; }
            if p["apiType"].as_str() == Some("openai") {
                let base_url = p["baseUrl"].as_str().unwrap_or("https://api.openai.com/v1");
                return Ok((key.to_string(), base_url.to_string()));
            }
        }

        Err("未找到支持图片生成的 Provider（需要 OpenAI 或兼容 API）".to_string())
    }
}

/// TTS 配置（从 settings 表读取 tts.* 键）
struct TtsConfig {
    provider: String,      // "local" | "mimo" | "openai"
    api_key: String,
    base_url: String,
    model: String,
    default_voice: String,
    default_style: String,
}

/// TTS 语音合成工具 — 支持本地/小米 MiMo/OpenAI 三模式
pub struct TtsTool {
    pool: sqlx::SqlitePool,
}

impl TtsTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Tool for TtsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "tts".to_string(),
            description: "文字转语音。支持 list_voices（列出可用声音）和 synthesize（合成语音）两种操作。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "操作类型：synthesize（合成语音，默认）/ list_voices（列出可用声音）"
                    },
                    "text": {
                        "type": "string",
                        "description": "要转换为语音的文本（synthesize 时必填）"
                    },
                    "mode": {
                        "type": "string",
                        "description": "TTS 模式（可选）：local（系统 TTS，免费）/ mimo（小米 MiMo-V2-TTS，高质量免费）/ api（OpenAI TTS）",
                        "default": "local"
                    },
                    "voice": {
                        "type": "string",
                        "description": "声音（可选）。local: 系统语音名（如 Ting-Ting/Samantha）；mimo: 自然人声；api: alloy/echo/fable/onyx/nova/shimmer"
                    },
                    "style": {
                        "type": "string",
                        "description": "语音风格描述（仅 mimo 模式）。如：'温柔的女声'、'东北口音'、'四川方言'、'开心的语气'、'唱歌'"
                    },
                    "speed": {
                        "type": "number",
                        "description": "语速（可选）。local: 100-300（默认200）；api: 0.25-4.0（默认1.0）"
                    }
                },
                "required": ["text"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Guarded
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let action = arguments.get("action").and_then(|a| a.as_str()).unwrap_or("synthesize");

        // 列出可用声音
        if action == "list_voices" {
            return self.list_voices().await;
        }

        let text = arguments.get("text")
            .and_then(|t| t.as_str())
            .ok_or("缺少 text 参数")?;
        // mode 优先用参数指定，否则读 tts.provider 配置，默认 local
        let config = self.load_tts_config().await;
        // 配置的 provider 优先（用户在设置中选的），LLM 参数次之，默认 local
        let arg_mode = arguments.get("mode").and_then(|m| m.as_str()).unwrap_or("");
        let mode_str = if !config.provider.is_empty() {
            config.provider.clone()
        } else if !arg_mode.is_empty() {
            arg_mode.to_string()
        } else {
            "local".to_string()
        };
        let mode = mode_str.as_str();
        log::info!("TTS mode={} (config.provider={}, arg_mode={})", mode, config.provider, arg_mode);

        if text.len() > 4096 {
            return Err("文本过长（最多 4096 字符）".to_string());
        }

        match mode {
            "mimo" => self.tts_mimo(text, &arguments).await,
            "api" | "openai" => self.tts_api(text, &arguments).await,
            _ => self.tts_local(text, &arguments).await,
        }
    }
}

impl TtsTool {
    /// 列出可用声音
    async fn list_voices(&self) -> Result<String, String> {
        let mut voices = Vec::new();

        // 本地声音
        #[cfg(target_os = "macos")]
        {
            let output = tokio::process::Command::new("say")
                .arg("-v").arg("?")
                .output().await
                .map_err(|e| format!("获取声音列表失败: {}", e))?;
            let list = String::from_utf8_lossy(&output.stdout);
            voices.push("## 本地声音 (macOS say)\n".to_string());
            for line in list.lines().take(20) {
                let parts: Vec<&str> = line.splitn(3, char::is_whitespace).collect();
                if let Some(name) = parts.first() {
                    voices.push(format!("- **{}** {}", name.trim(), parts.get(1).unwrap_or(&"")));
                }
            }
        }
        #[cfg(target_os = "linux")]
        {
            voices.push("## 本地声音 (espeak)\n".to_string());
            voices.push("- zh (中文)\n- en (英文)\n- de (德文)\n- fr (法文)".to_string());
        }
        #[cfg(target_os = "windows")]
        {
            voices.push("## 本地声音 (Windows SAPI)\n".to_string());
            voices.push("- 系统默认声音".to_string());
        }

        // OpenAI 声音
        voices.push("\n## 小米 MiMo-V2-TTS (mode=mimo)\n".to_string());
        voices.push("- 支持自然语言风格控制（通过 style 参数）".to_string());
        voices.push("- 示例风格：'温柔的女声'、'东北口音'、'四川方言'、'开心的'、'悲伤的'、'唱歌'".to_string());
        voices.push("- 支持方言：东北话、四川话、河南话、粤语、台湾腔".to_string());
        voices.push("- 当前免费".to_string());

        voices.push("\n## OpenAI TTS (mode=api)\n".to_string());
        voices.push("- **alloy** — 中性、平衡\n- **echo** — 低沉、稳重\n- **fable** — 温暖、叙事\n- **onyx** — 深沉、权威\n- **nova** — 明亮、活力\n- **shimmer** — 柔和、友好".to_string());

        Ok(voices.join("\n"))
    }

    /// 本地 TTS — 使用系统命令（macOS: say, Linux: espeak, Windows: PowerShell）
    async fn tts_local(&self, text: &str, args: &serde_json::Value) -> Result<String, String> {
        let output_dir = dirs::home_dir()
            .ok_or("无法获取 home 目录")?
            .join(".xianzhu/tts");
        let _ = std::fs::create_dir_all(&output_dir);
        #[cfg(target_os = "macos")]
        let filename = format!("tts_{}.m4a", chrono::Utc::now().timestamp_millis());
        #[cfg(not(target_os = "macos"))]
        let filename = format!("tts_{}.wav", chrono::Utc::now().timestamp_millis());
        let output_path = output_dir.join(&filename);

        #[cfg(target_os = "macos")]
        {
            let voice = args.get("voice").and_then(|v| v.as_str()).unwrap_or("");
            let rate = args.get("speed").and_then(|s| s.as_u64()).unwrap_or(200);

            // 先生成 AIFF 临时文件
            let tmp_aiff = output_dir.join(format!("_tmp_{}.aiff", chrono::Utc::now().timestamp_millis()));
            let mut cmd = tokio::process::Command::new("say");
            if !voice.is_empty() {
                cmd.arg("-v").arg(voice);
            }
            cmd.arg("-r").arg(rate.to_string());
            cmd.arg("-o").arg(tmp_aiff.to_str().unwrap_or(""));
            cmd.arg(text);

            let result = cmd.output().await
                .map_err(|e| format!("say 命令执行失败: {}", e))?;

            if !result.status.success() {
                let stderr = String::from_utf8_lossy(&result.stderr);
                return Err(format!("say 失败: {}", stderr));
            }

            // 转换为 m4a（浏览器可播放）
            let convert = tokio::process::Command::new("afconvert")
                .args(["-f", "m4af", "-d", "aac"])
                .arg(tmp_aiff.to_str().unwrap_or(""))
                .arg(output_path.to_str().unwrap_or(""))
                .output().await;

            // 清理临时文件
            let _ = std::fs::remove_file(&tmp_aiff);

            if let Ok(r) = convert {
                if !r.status.success() {
                    return Err("音频格式转换失败（afconvert）".to_string());
                }
            }

            let size = std::fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0);
            Ok(format!("语音已生成（本地 TTS）: {} ({} 字节)\n文件: {}", filename, size, output_path.display()))
        }

        #[cfg(target_os = "linux")]
        {
            // 检测 espeak 是否安装
            let check = tokio::process::Command::new("which").arg("espeak").output().await;
            if check.is_err() || !check.unwrap().status.success() {
                // 尝试 espeak-ng（更新的分支）
                let check_ng = tokio::process::Command::new("which").arg("espeak-ng").output().await;
                if check_ng.is_err() || !check_ng.unwrap().status.success() {
                    return Err("本地 TTS 需要安装 espeak：\n  Ubuntu/Debian: sudo apt install espeak-ng\n  Fedora: sudo dnf install espeak-ng\n  Arch: sudo pacman -S espeak-ng\n\n或使用 mode=api 调用 OpenAI TTS".to_string());
                }
                // 用 espeak-ng
                let voice = args.get("voice").and_then(|v| v.as_str()).unwrap_or("zh");
                let speed = args.get("speed").and_then(|s| s.as_u64()).unwrap_or(175);
                let result = tokio::process::Command::new("espeak-ng")
                    .arg("-v").arg(voice)
                    .arg("-s").arg(speed.to_string())
                    .arg("-w").arg(output_path.to_str().unwrap_or(""))
                    .arg(text)
                    .output().await
                    .map_err(|e| format!("espeak-ng 执行失败: {}", e))?;
                if !result.status.success() {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    return Err(format!("espeak-ng 失败: {}", stderr));
                }
            } else {
                let voice = args.get("voice").and_then(|v| v.as_str()).unwrap_or("zh");
                let speed = args.get("speed").and_then(|s| s.as_u64()).unwrap_or(175);
                let result = tokio::process::Command::new("espeak")
                    .arg("-v").arg(voice)
                    .arg("-s").arg(speed.to_string())
                    .arg("-w").arg(output_path.to_str().unwrap_or(""))
                    .arg(text)
                    .output().await
                    .map_err(|e| format!("espeak 执行失败: {}", e))?;
                if !result.status.success() {
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    return Err(format!("espeak 失败: {}", stderr));
                }
            }
            let size = std::fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0);
            Ok(format!("语音已生成（Linux TTS）: {} ({} 字节)\n文件: {}", filename, size, output_path.display()))
        }

        #[cfg(target_os = "windows")]
        {
            // Windows 自带 System.Speech（.NET Framework），大部分系统可用
            let output_wav = output_path.with_extension("wav");
            let ps_script = format!(
                "Add-Type -AssemblyName System.Speech; \
                 $synth = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
                 $synth.SetOutputToWaveFile('{}'); \
                 $synth.Speak('{}'); \
                 $synth.Dispose()",
                output_wav.to_str().unwrap_or("").replace("'", "''"),
                text.replace("'", "''").replace("\n", " ")
            );
            let result = tokio::process::Command::new("powershell")
                .arg("-NoProfile").arg("-Command").arg(&ps_script)
                .output().await
                .map_err(|e| format!("PowerShell TTS 失败: {}。\n如果 System.Speech 不可用，请使用 mode=api", e))?;

            if !result.status.success() {
                let stderr = String::from_utf8_lossy(&result.stderr);
                return Err(format!("Windows TTS 失败: {}\n\n建议使用 mode=api 调用 OpenAI TTS", stderr));
            }
            let size = std::fs::metadata(&output_wav).map(|m| m.len()).unwrap_or(0);
            Ok(format!("语音已生成（Windows TTS）: {} ({} 字节)\n文件: {}", output_wav.file_name().unwrap_or_default().to_string_lossy(), size, output_wav.display()))
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Err("当前系统不支持本地 TTS，请使用 mode=api".to_string())
        }
    }

    /// 小米 MiMo-V2-TTS — 通过 chat/completions 端点，返回 base64 WAV 音频
    ///
    /// 调用方式：POST /v1/chat/completions
    ///   model: "mimo-v2-tts"
    ///   messages: [{"role": "assistant", "content": "要朗读的文本"}]
    /// 响应：choices[0].message.audio.data → base64 编码的 WAV 音频
    async fn tts_mimo(&self, text: &str, args: &serde_json::Value) -> Result<String, String> {
        let style = args.get("style").and_then(|s| s.as_str()).unwrap_or("");

        // 风格控制：拼接到文本前面
        let content = if style.is_empty() {
            text.to_string()
        } else {
            format!("[{}]{}", style, text)
        };

        log::info!("MiMo TTS: {} 字符, style={}", text.len(), style);

        let (api_key, base_url) = self.find_mimo_provider().await?;
        let tts_config = self.load_tts_config().await;
        let model = if tts_config.model.is_empty() { "mimo-v2-tts".to_string() } else { tts_config.model };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| format!("创建客户端失败: {}", e))?;

        // MiMo TTS 使用 chat/completions 端点，要求 assistant 角色
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let resp = client.post(&url)
            .header("api-key", &api_key)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "messages": [{"role": "assistant", "content": content}],
                "max_completion_tokens": 8192,
            }))
            .send().await
            .map_err(|e| format!("MiMo TTS 请求失败: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("MiMo TTS 失败 (HTTP {}): {}", status, &body[..body.len().min(200)]));
        }

        // 解析 JSON 响应，提取 audio.data (base64)
        let json: serde_json::Value = resp.json().await
            .map_err(|e| format!("解析响应失败: {}", e))?;

        let audio_b64 = json["choices"][0]["message"]["audio"]["data"]
            .as_str()
            .ok_or("响应中没有音频数据（choices[0].message.audio.data）")?;

        // base64 解码
        let audio_bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, audio_b64)
            .map_err(|e| format!("音频 base64 解码失败: {}", e))?;

        // 保存为 WAV 文件
        let output_dir = dirs::home_dir().ok_or("无法获取 home 目录")?.join(".xianzhu/tts");
        let _ = std::fs::create_dir_all(&output_dir);
        let filename = format!("mimo_tts_{}.wav", chrono::Utc::now().timestamp_millis());
        let output_path = output_dir.join(&filename);
        std::fs::write(&output_path, &audio_bytes).map_err(|e| format!("保存失败: {}", e))?;

        let style_info = if style.is_empty() { String::new() } else { format!(", 风格: {}", style) };
        Ok(format!("语音已生成（MiMo-V2-TTS{}）: {} ({:.1}KB)\n文件: {}",
            style_info, filename, audio_bytes.len() as f64 / 1024.0, output_path.display()))
    }

    /// 读取 TTS 专用配置（settings 表中的 tts.* 键）
    async fn load_tts_config(&self) -> TtsConfig {
        let get = |key: &str| -> Option<String> {
            let full_key = format!("tts.{}", key);
            let rt = tokio::runtime::Handle::current();
            std::thread::spawn(move || {
                rt.block_on(async {
                    // 这里不能 async，用同步方式
                    None::<String>
                })
            }).join().ok().flatten()
        };
        // 用简单的同步查询
        let provider = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'tts.provider'")
            .fetch_optional(&self.pool).await.ok().flatten().unwrap_or_else(|| "local".into());
        let api_key = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'tts.api_key'")
            .fetch_optional(&self.pool).await.ok().flatten().unwrap_or_default();
        let base_url = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'tts.base_url'")
            .fetch_optional(&self.pool).await.ok().flatten().unwrap_or_default();
        let model = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'tts.model'")
            .fetch_optional(&self.pool).await.ok().flatten().unwrap_or_default();
        let default_voice = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'tts.default_voice'")
            .fetch_optional(&self.pool).await.ok().flatten().unwrap_or_default();
        let default_style = sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'tts.default_style'")
            .fetch_optional(&self.pool).await.ok().flatten().unwrap_or_default();

        TtsConfig { provider, api_key, base_url, model, default_voice, default_style }
    }

    /// 从 TTS 配置或 providers 列表查找 MiMo 凭据
    async fn find_mimo_provider(&self) -> Result<(String, String), String> {
        // 优先从 TTS 专用配置读取
        let config = self.load_tts_config().await;
        if config.provider == "mimo" && !config.api_key.is_empty() && !config.base_url.is_empty() {
            return Ok((config.api_key, config.base_url));
        }

        // fallback: 从 providers 列表查找
        let json_str: Option<String> = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = 'providers'"
        ).fetch_optional(&self.pool).await.ok().flatten();

        let providers: Vec<serde_json::Value> = json_str
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        for p in &providers {
            let base = p["base_url"].as_str().unwrap_or("");
            let id = p["id"].as_str().unwrap_or("");
            if base.contains("xiaomimimo") || base.contains("mimo") || id.contains("mimo") {
                let key = p["api_key"].as_str().unwrap_or("").to_string();
                if !key.is_empty() && !base.is_empty() {
                    return Ok((key, base.to_string()));
                }
            }
        }

        Err("未配置 TTS 语音合成。请在设置 → 语音合成中配置 API Key 和服务地址。".into())
    }

    /// OpenAI TTS API
    async fn tts_api(&self, text: &str, args: &serde_json::Value) -> Result<String, String> {
        let voice = args.get("voice").and_then(|v| v.as_str()).unwrap_or("alloy");
        let speed = args.get("speed").and_then(|s| s.as_f64()).unwrap_or(1.0);

        log::info!("TTS API: {} 字符, voice={}, speed={}", text.len(), voice, speed);

        let (api_key, base_url) = self.find_openai_provider().await?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("创建客户端失败: {}", e))?;

        let url = format!("{}/audio/speech", base_url.trim_end_matches('/'));
        let resp = client.post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&serde_json::json!({
                "model": "tts-1",
                "input": text,
                "voice": voice,
                "speed": speed,
            }))
            .send().await
            .map_err(|e| format!("TTS 请求失败: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("TTS 失败 (HTTP {}): {}", status, &body[..body.len().min(200)]));
        }

        let bytes = resp.bytes().await.map_err(|e| format!("读取音频失败: {}", e))?;
        let output_dir = dirs::home_dir().ok_or("无法获取 home 目录")?.join(".xianzhu/tts");
        let _ = std::fs::create_dir_all(&output_dir);
        let filename = format!("tts_{}.mp3", chrono::Utc::now().timestamp_millis());
        let output_path = output_dir.join(&filename);
        std::fs::write(&output_path, &bytes).map_err(|e| format!("保存失败: {}", e))?;

        Ok(format!("语音已生成（OpenAI TTS）: {} ({} 字节)\n文件: {}", filename, bytes.len(), output_path.display()))
    }

    async fn find_openai_provider(&self) -> Result<(String, String), String> {
        let json_str: Option<String> = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = 'providers'"
        ).fetch_optional(&self.pool).await.ok().flatten();

        let providers: Vec<serde_json::Value> = json_str
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        for p in &providers {
            if p["enabled"].as_bool() != Some(true) { continue; }
            let key = p["apiKey"].as_str().unwrap_or("");
            if key.is_empty() { continue; }
            if p["apiType"].as_str() == Some("openai") {
                let base_url = p["baseUrl"].as_str().unwrap_or("https://api.openai.com/v1");
                return Ok((key.to_string(), base_url.to_string()));
            }
        }
        Err("未找到 OpenAI Provider，请先配置。或使用 mode=local（免费）".to_string())
    }
}

// ─── Patch 应用工具 ──────────────────────────────────────────

/// 多文件 Patch 应用工具
///
/// 接收 unified diff 格式的 patch，应用到工作目录。
/// 支持 dry-run 预检、备份原文件、回滚。
pub struct ApplyPatchTool;

#[async_trait]
impl Tool for ApplyPatchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "apply_patch".to_string(),
            description: "应用 unified diff patch 到文件。支持多文件 patch、dry-run 预检、自动备份。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "patch": {
                        "type": "string",
                        "description": "unified diff 格式的 patch 内容"
                    },
                    "working_dir": {
                        "type": "string",
                        "description": "工作目录（patch 中的文件路径相对于此目录）"
                    },
                    "dry_run": {
                        "type": "boolean",
                        "description": "仅检查是否可以应用，不实际修改文件（默认 false）"
                    }
                },
                "required": ["patch"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Approval }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let patch = arguments["patch"].as_str().ok_or("缺少 patch")?;
        let working_dir = arguments["working_dir"].as_str().unwrap_or(".");
        let dry_run = arguments["dry_run"].as_bool().unwrap_or(false);

        let flag = if dry_run { "--dry-run" } else { "--backup" };

        let _output = tokio::process::Command::new("patch")
            .args(&["-p1", flag, "--verbose"])
            .current_dir(working_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("启动 patch 命令失败: {}。请确保已安装 patch 工具。", e))?
            .wait_with_output().await
            .map_err(|e| format!("patch 执行失败: {}", e))?;

        // 如果 spawn 后需要写 stdin，重新执行
        let mut child = tokio::process::Command::new("patch")
            .args(&["-p1", flag, "--verbose"])
            .current_dir(working_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("启动 patch 失败: {}", e))?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let _ = stdin.write_all(patch.as_bytes()).await;
            drop(stdin);
        }

        let output = child.wait_with_output().await
            .map_err(|e| format!("patch 执行失败: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            if dry_run {
                Ok(format!("Patch 预检通过（dry-run）:\n{}", stdout))
            } else {
                Ok(format!("Patch 已成功应用:\n{}", stdout))
            }
        } else {
            Err(format!("Patch 应用失败:\n{}\n{}", stdout, stderr))
        }
    }
}

// ─── 浏览器工具（CDP 完整版）─────────────────────────────────

/// 浏览器自动化工具
///
/// 支持两种模式：
/// - **简单模式**：打开 URL、列出浏览器（无需 CDP）
/// - **CDP 模式**：启动受管 Chrome，支持截图、页面快照、导航、
///   JS 执行、点击、输入等自动化操作
///
/// CDP 模式需要系统安装 Chrome/Brave/Edge/Chromium。
/// 首次调用 CDP 操作时自动启动隔离 Chrome 实例（端口 9222）。
pub struct BrowserTool;

/// CDP 默认端口
const CDP_PORT: u16 = 9222;

#[async_trait]
impl Tool for BrowserTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "browser".to_string(),
            description: "浏览器自动化工具。支持两种模式：隔离模式（新 Chrome）和用户模式（连接已登录的 Chrome）。操作：导航、截图、ARIA 快照、JS 执行、点击、输入、悬停、拖拽、表单填写、文件上传、对话框处理、滚动等。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "操作类型",
                        "enum": ["open", "list_browsers", "navigate", "tabs", "screenshot", "screenshot_labels", "snapshot", "evaluate", "click", "double_click", "type", "press_key", "hover", "drag", "scroll", "fill_form", "upload_file", "handle_dialog", "resize", "wait_for", "page_info", "close_tab", "connect_user"]
                    },
                    "url": { "type": "string", "description": "URL（open/navigate 时需要）" },
                    "browser": { "type": "string", "description": "指定浏览器: chrome/brave/edge/chromium/default" },
                    "full_page": { "type": "boolean", "description": "全页截图（默认 false）" },
                    "expression": { "type": "string", "description": "JavaScript 表达式（evaluate 时需要）" },
                    "ref": { "type": "string", "description": "ARIA ref（如 ax15）— 从 snapshot 获取，可替代 x/y 坐标和 selector" },
                    "x": { "type": "number", "description": "X 坐标（无 ref 时使用）" },
                    "y": { "type": "number", "description": "Y 坐标（无 ref 时使用）" },
                    "to_x": { "type": "number", "description": "拖拽目标 X（drag 时需要）" },
                    "to_y": { "type": "number", "description": "拖拽目标 Y（drag 时需要）" },
                    "delta_x": { "type": "number", "description": "滚动 X 偏移（scroll 时）" },
                    "delta_y": { "type": "number", "description": "滚动 Y 偏移（scroll 时，负=向上）" },
                    "text": { "type": "string", "description": "输入文本（type）/ 按键名（press_key）" },
                    "key": { "type": "string", "description": "按键：Enter/Tab/Escape/Backspace/ArrowUp 等" },
                    "selector": { "type": "string", "description": "CSS 选择器（fill_form/upload_file/wait_for 时）" },
                    "fields": { "type": "array", "description": "表单字段（fill_form 时）", "items": { "type": "object", "properties": { "selector": { "type": "string" }, "value": { "type": "string" } } } },
                    "file_paths": { "type": "array", "description": "文件路径列表（upload_file 时）", "items": { "type": "string" } },
                    "accept": { "type": "boolean", "description": "接受/拒绝对话框（handle_dialog 时）" },
                    "prompt_text": { "type": "string", "description": "对话框输入文本（handle_dialog + prompt 时）" },
                    "width": { "type": "integer", "description": "视口宽度（resize 时）" },
                    "height": { "type": "integer", "description": "视口高度（resize 时）" },
                    "timeout_ms": { "type": "integer", "description": "超时毫秒（wait_for 时，默认 5000）" },
                    "target_id": { "type": "string", "description": "目标 Tab ID（可选）" },
                    "limit": { "type": "integer", "description": "快照节点上限（默认 500）" },
                    "headless": { "type": "boolean", "description": "无头模式（默认 false）" }
                },
                "required": ["action"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel {
        ToolSafetyLevel::Guarded
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let action = arguments["action"].as_str().unwrap_or("open");

        match action {
            // ── 简单模式（无需 CDP）──
            "open" => {
                let url = arguments["url"].as_str().ok_or("缺少 url")?;
                let browser = arguments["browser"].as_str();
                crate::agent::browser::open_url(url, browser)?;
                Ok(format!("已在浏览器中打开: {}", url))
            }
            "list_browsers" => {
                let browsers = crate::agent::browser::detect_browsers();
                let list: Vec<String> = browsers.iter()
                    .map(|b| format!("- **{}** (`{}`): {}", b.name, b.kind,
                        if b.path.is_empty() { "系统默认".into() } else { b.path.clone() }))
                    .collect();
                Ok(format!("已检测到 {} 个浏览器:\n{}", browsers.len(), list.join("\n")))
            }

            // ── CDP 模式 ──
            "tabs" => {
                let tabs = crate::agent::cdp::list_tabs(CDP_PORT).await?;
                if tabs.is_empty() {
                    Ok("无打开的 Tab（Chrome CDP 可能未运行，先执行 action=navigate 启动）".into())
                } else {
                    let list: Vec<String> = tabs.iter()
                        .map(|t| format!("- [{}] **{}**\n  {}", &t.id[..8.min(t.id.len())], t.title, t.url))
                        .collect();
                    Ok(format!("{} 个 Tab:\n{}", tabs.len(), list.join("\n")))
                }
            }
            "navigate" => {
                let url = arguments["url"].as_str().ok_or("缺少 url")?;
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return Err("安全限制：只能导航到 http/https URL".into());
                }
                let ws_url = get_or_launch_cdp(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                client.navigate(url).await?;
                Ok(format!("已导航到: {}", url))
            }
            "screenshot" => {
                let full_page = arguments["full_page"].as_bool().unwrap_or(false);
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                let base64 = client.screenshot(full_page).await?;

                // 保存到临时文件
                let path = std::env::temp_dir().join(format!("xianzhu-screenshot-{}.png", chrono::Utc::now().timestamp()));
                let bytes = base64_decode(&base64)?;
                tokio::fs::write(&path, &bytes).await.map_err(|e| e.to_string())?;

                Ok(format!("截图已保存: {} ({} bytes)\n[base64 数据长度: {}]", path.display(), bytes.len(), base64.len()))
            }
            "screenshot_labels" => {
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                let max_labels = arguments["limit"].as_u64().unwrap_or(50) as usize;
                let nodes = client.aria_snapshot(500).await?;
                let base64 = client.screenshot_with_labels(&nodes, max_labels).await?;

                let path = std::env::temp_dir().join(format!("xianzhu-labeled-{}.png", chrono::Utc::now().timestamp()));
                let bytes = base64_decode(&base64)?;
                tokio::fs::write(&path, &bytes).await.map_err(|e| e.to_string())?;

                Ok(format!("标注截图已保存: {} ({} bytes, {} labels)", path.display(), bytes.len(), nodes.len().min(max_labels)))
            }
            "snapshot" => {
                let limit = arguments["limit"].as_u64().unwrap_or(500) as usize;
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                let nodes = client.aria_snapshot(limit).await?;
                let formatted = crate::agent::cdp::format_aria_snapshot(&nodes);
                Ok(format!("页面 ARIA 快照（{} 节点）:\n\n{}", nodes.len(), formatted))
            }
            "evaluate" => {
                let expr = arguments["expression"].as_str().ok_or("缺少 expression")?;
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                let result = client.evaluate(expr).await?;
                Ok(format!("JS 执行结果:\n{}", serde_json::to_string_pretty(&result).unwrap_or_default()))
            }
            "click" => {
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                if let Some(ref_id) = arguments["ref"].as_str() {
                    let nodes = client.aria_snapshot(1000).await?;
                    client.click_ref(&nodes, ref_id).await?;
                    Ok(format!("已点击 [ref={}]", ref_id))
                } else {
                    let x = arguments["x"].as_f64().ok_or("缺少 x 坐标或 ref")?;
                    let y = arguments["y"].as_f64().ok_or("缺少 y")?;
                    client.click(x, y).await?;
                    Ok(format!("已点击 ({}, {})", x, y))
                }
            }
            "type" => {
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                if let Some(ref_id) = arguments["ref"].as_str() {
                    let value = arguments["text"].as_str().ok_or("缺少 text")?;
                    let nodes = client.aria_snapshot(1000).await?;
                    client.fill_ref(&nodes, ref_id, value).await?;
                    Ok(format!("已填入 [ref={}]: {}", ref_id, value))
                } else {
                    let text = arguments["text"].as_str().ok_or("缺少 text")?;
                    client.type_text(text).await?;
                    Ok(format!("已输入: {}", text))
                }
            }
            "double_click" => {
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                if let Some(ref_id) = arguments["ref"].as_str() {
                    let nodes = client.aria_snapshot(1000).await?;
                    let (x, y) = client.resolve_ref_coordinates(&nodes, ref_id).await?;
                    client.double_click(x, y).await?;
                    Ok(format!("已双击 [ref={}]", ref_id))
                } else {
                    let x = arguments["x"].as_f64().ok_or("缺少 x 或 ref")?;
                    let y = arguments["y"].as_f64().ok_or("缺少 y")?;
                    client.double_click(x, y).await?;
                    Ok(format!("已双击 ({}, {})", x, y))
                }
            }
            "hover" => {
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                if let Some(ref_id) = arguments["ref"].as_str() {
                    let nodes = client.aria_snapshot(1000).await?;
                    client.hover_ref(&nodes, ref_id).await?;
                    Ok(format!("已悬停 [ref={}]", ref_id))
                } else {
                    let x = arguments["x"].as_f64().ok_or("缺少 x 或 ref")?;
                    let y = arguments["y"].as_f64().ok_or("缺少 y")?;
                    client.hover(x, y).await?;
                    Ok(format!("已悬停 ({}, {})", x, y))
                }
            }
            "drag" => {
                let x = arguments["x"].as_f64().ok_or("缺少 x")?;
                let y = arguments["y"].as_f64().ok_or("缺少 y")?;
                let to_x = arguments["to_x"].as_f64().ok_or("缺少 to_x")?;
                let to_y = arguments["to_y"].as_f64().ok_or("缺少 to_y")?;
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                client.drag(x, y, to_x, to_y).await?;
                Ok(format!("已拖拽 ({},{}) → ({},{})", x, y, to_x, to_y))
            }
            "press_key" => {
                let key = arguments["key"].as_str().or(arguments["text"].as_str()).ok_or("缺少 key")?;
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                client.press_key(key).await?;
                Ok(format!("已按键: {}", key))
            }
            "scroll" => {
                let x = arguments["x"].as_f64().unwrap_or(400.0);
                let y = arguments["y"].as_f64().unwrap_or(300.0);
                let dx = arguments["delta_x"].as_f64().unwrap_or(0.0);
                let dy = arguments["delta_y"].as_f64().unwrap_or(-300.0);
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                client.scroll(x, y, dx, dy).await?;
                Ok(format!("已滚动 dx={} dy={}", dx, dy))
            }
            "fill_form" => {
                let fields_val = arguments.get("fields").ok_or("缺少 fields")?;
                let fields_arr = fields_val.as_array().ok_or("fields 需要是数组")?;
                let fields: Vec<(String, String)> = fields_arr.iter().filter_map(|f| {
                    let sel = f["selector"].as_str()?.to_string();
                    let val = f["value"].as_str()?.to_string();
                    Some((sel, val))
                }).collect();
                if fields.is_empty() { return Err("fields 为空".into()); }
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                client.fill_form(&fields).await
            }
            "upload_file" => {
                let paths: Vec<String> = arguments["file_paths"].as_array()
                    .ok_or("缺少 file_paths")?
                    .iter().filter_map(|p| p.as_str().map(String::from)).collect();
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                if let Some(ref_id) = arguments["ref"].as_str() {
                    let nodes = client.aria_snapshot(1000).await?;
                    client.upload_ref(&nodes, ref_id, &paths).await?;
                    Ok(format!("已上传 {} 个文件到 [ref={}]", paths.len(), ref_id))
                } else {
                    let selector = arguments["selector"].as_str().ok_or("缺少 selector 或 ref")?;
                    client.set_file_input(selector, &paths).await?;
                    Ok(format!("已上传 {} 个文件到 {}", paths.len(), selector))
                }
            }
            "handle_dialog" => {
                let accept = arguments["accept"].as_bool().unwrap_or(true);
                let prompt_text = arguments["prompt_text"].as_str();
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                client.handle_dialog(accept, prompt_text).await?;
                Ok(format!("对话框已{}", if accept { "接受" } else { "拒绝" }))
            }
            "resize" => {
                let w = arguments["width"].as_u64().ok_or("缺少 width")? as u32;
                let h = arguments["height"].as_u64().ok_or("缺少 height")? as u32;
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                client.resize(w, h).await?;
                Ok(format!("视口已调整: {}x{}", w, h))
            }
            "wait_for" => {
                let selector = arguments["selector"].as_str().ok_or("缺少 selector")?;
                let timeout = arguments["timeout_ms"].as_u64().unwrap_or(5000);
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                client.wait_for_selector(selector, timeout).await?;
                Ok(format!("元素 {} 已出现", selector))
            }
            "page_info" => {
                let ws_url = get_tab_ws_url(&arguments).await?;
                let client = crate::agent::cdp::CdpClient::connect(&ws_url).await?;
                let (url, title) = client.get_page_info().await?;
                Ok(format!("**{}**\n{}", title, url))
            }
            "connect_user" => {
                // 连接用户已运行的 Chrome（existing-session 模式）
                match crate::agent::cdp::connect_user_chrome().await {
                    Ok((port, _ws)) => {
                        let tabs = crate::agent::cdp::list_tabs(port).await?;
                        let tab_list: Vec<String> = tabs.iter().take(10)
                            .map(|t| format!("- **{}** — {}", t.title, t.url))
                            .collect();
                        Ok(format!("已连接用户 Chrome（端口 {}）\n{} 个 Tab:\n{}",
                            port, tabs.len(), tab_list.join("\n")))
                    }
                    Err(e) => Err(e),
                }
            }
            "close_tab" => {
                let target_id = arguments["target_id"].as_str().ok_or("缺少 target_id")?;
                let tabs = crate::agent::cdp::list_tabs(CDP_PORT).await?;
                if let Some(tab) = tabs.iter().find(|t| t.id == target_id || t.id.starts_with(target_id)) {
                    if let Some(ref ws) = tab.ws_url {
                        let client = crate::agent::cdp::CdpClient::connect(ws).await?;
                        client.send("Target.closeTarget", Some(serde_json::json!({"targetId": tab.id}))).await?;
                        return Ok(format!("已关闭 Tab: {}", tab.title));
                    }
                }
                Err("未找到指定 Tab".into())
            }
            _ => Err(format!("未知操作: {}。支持: open/list_browsers/navigate/tabs/screenshot/snapshot/evaluate/click/double_click/type/press_key/hover/drag/scroll/fill_form/upload_file/handle_dialog/resize/wait_for/page_info/connect_user/close_tab", action)),
        }
    }
}

/// 获取或启动 CDP Chrome，返回第一个 page 的 WebSocket URL
async fn get_or_launch_cdp(args: &serde_json::Value) -> Result<String, String> {
    // 先尝试连接已有的
    if let Ok(tabs) = crate::agent::cdp::list_tabs(CDP_PORT).await {
        if let Some(tab) = tabs.first() {
            if let Some(ref ws) = tab.ws_url {
                return Ok(ws.clone());
            }
        }
    }

    // 启动新的 Chrome
    let browsers = crate::agent::browser::detect_browsers();
    let executable = browsers.iter()
        .find(|b| b.kind != "default" && !b.path.is_empty())
        .map(|b| b.path.clone())
        .ok_or("未检测到 Chromium 浏览器（Chrome/Brave/Edge），请先安装。")?;

    let headless = args["headless"].as_bool().unwrap_or(false);
    let chrome = crate::agent::cdp::launch_chrome(&executable, CDP_PORT, headless).await?;
    let ws_url = chrome.ws_url.clone();
    // 泄漏 chrome 实例使其保持运行（不触发 Drop）
    std::mem::forget(chrome);
    Ok(ws_url)
}

/// 获取指定 Tab 的 WebSocket URL（优先 target_id，否则第一个 Tab）
async fn get_tab_ws_url(args: &serde_json::Value) -> Result<String, String> {
    let target_id = args["target_id"].as_str();

    let tabs = crate::agent::cdp::list_tabs(CDP_PORT).await
        .map_err(|_| "Chrome CDP 未运行。请先执行 action=navigate 启动浏览器。".to_string())?;

    if tabs.is_empty() {
        return Err("无打开的 Tab。请先执行 action=navigate 打开页面。".into());
    }

    let tab = if let Some(tid) = target_id {
        tabs.iter().find(|t| t.id == tid || t.id.starts_with(tid))
            .ok_or(format!("未找到 Tab: {}", tid))?
    } else {
        tabs.first().unwrap()
    };

    tab.ws_url.clone().ok_or("Tab 无 WebSocket URL".into())
}

/// base64 解码
fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    // 简单的 base64 解码（不引入额外依赖）
    let chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = Vec::new();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for c in input.chars() {
        if c == '=' || c == '\n' || c == '\r' { continue; }
        let val = chars.find(c).ok_or(format!("非法 base64 字符: {}", c))? as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push(((buf >> bits) & 0xFF) as u8);
        }
    }

    Ok(output)
}

// ─── 语音转文字工具 (STT) ────────────────────────────────────

/// 语音转文字工具
///
/// 支持：
/// - OpenAI Whisper API（需要 API Key）
/// - macOS 原生 Speech Framework（免费）
/// - Linux: whisper.cpp 本地推理（如安装）
pub struct SttTool {
    pool: sqlx::SqlitePool,
}

impl SttTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl Tool for SttTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "speech_to_text".to_string(),
            description: "将音频文件转为文字。支持 OpenAI Whisper API 和本地转录。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "音频文件路径（支持 mp3/wav/m4a/ogg/webm）"
                    },
                    "mode": {
                        "type": "string",
                        "description": "模式：whisper（OpenAI API）/ local（本地转录）。默认 auto。"
                    },
                    "language": {
                        "type": "string",
                        "description": "语言代码（如 zh/en/ja）。不填自动检测。"
                    }
                },
                "required": ["file_path"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Guarded }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let file_path = arguments["file_path"].as_str().ok_or("缺少 file_path")?;
        let mode = arguments["mode"].as_str().unwrap_or("auto");
        let language = arguments["language"].as_str();

        // 校验文件存在
        let path = std::path::Path::new(file_path);
        if !path.exists() {
            return Err(format!("文件不存在: {}", file_path));
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if !["mp3", "wav", "m4a", "ogg", "webm", "flac", "mp4"].contains(&ext.as_str()) {
            return Err(format!("不支持的音频格式: .{}", ext));
        }

        match mode {
            "whisper" => self.whisper_api(file_path, language).await,
            "local" => stt_local(file_path, language).await,
            _ => {
                // auto: 有 OpenAI key 用 Whisper，否则本地
                if let Some(key) = self.get_openai_key().await {
                    match whisper_api_call(&key, file_path, language).await {
                        Ok(text) => Ok(text),
                        Err(_) => stt_local(file_path, language).await,
                    }
                } else {
                    stt_local(file_path, language).await
                }
            }
        }
    }
}

impl SttTool {
    async fn get_openai_key(&self) -> Option<String> {
        // 复用 TtsTool 的 key 查找逻辑
        let json_str: Option<String> = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = 'providers'"
        ).fetch_optional(&self.pool).await.ok().flatten();

        let providers: Vec<serde_json::Value> = json_str
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        for p in &providers {
            if p["enabled"].as_bool() != Some(true) { continue; }
            let key = p["apiKey"].as_str().unwrap_or("");
            if key.is_empty() { continue; }
            if p["apiType"].as_str() == Some("openai") {
                return Some(key.to_string());
            }
        }
        None
    }

    async fn whisper_api(&self, file_path: &str, language: Option<&str>) -> Result<String, String> {
        let key = self.get_openai_key().await
            .ok_or("未找到 OpenAI Provider，无法使用 Whisper API")?;
        whisper_api_call(&key, file_path, language).await
    }
}

/// OpenAI Whisper API 调用
async fn whisper_api_call(api_key: &str, file_path: &str, language: Option<&str>) -> Result<String, String> {
    let file_bytes = tokio::fs::read(file_path).await
        .map_err(|e| format!("读取文件失败: {}", e))?;

    let file_name = std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.mp3")
        .to_string();

    let file_part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(file_name)
        .mime_str("audio/mpeg").unwrap_or_else(|_| reqwest::multipart::Part::bytes(vec![]));

    let mut form = reqwest::multipart::Form::new()
        .text("model", "whisper-1")
        .part("file", file_part);

    if let Some(lang) = language {
        form = form.text("language", lang.to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build().map_err(|e| e.to_string())?;

    let resp = client.post("https://api.openai.com/v1/audio/transcriptions")
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send().await
        .map_err(|e| format!("Whisper API 请求失败: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Whisper API 错误 {}: {}", status, &body[..body.len().min(200)]));
    }

    let data: serde_json::Value = resp.json().await
        .map_err(|e| format!("解析失败: {}", e))?;

    data["text"].as_str()
        .map(|s| s.to_string())
        .ok_or("Whisper 返回空结果".into())
}

/// 本地语音转文字
async fn stt_local(file_path: &str, _language: Option<&str>) -> Result<String, String> {
    // macOS: 使用 say -i 的逆操作不可行，改用 afplay + Python speech_recognition
    // 简单方案: 检查是否安装了 whisper CLI
    #[cfg(target_os = "macos")]
    {
        // 尝试 macOS 内置 SFSpeechRecognizer (通过 swift 脚本)
        let output = tokio::process::Command::new("swift")
            .arg("-e")
            .arg(format!(r#"
import Speech
import Foundation
let sem = DispatchSemaphore(value: 0)
let recognizer = SFSpeechRecognizer(locale: Locale(identifier: "zh-Hans"))!
let request = SFSpeechURLRecognitionRequest(url: URL(fileURLWithPath: "{}"))
recognizer.recognitionTask(with: request) {{ result, error in
    if let r = result, r.isFinal {{ print(r.bestTranscription.formattedString); sem.signal() }}
    else if error != nil {{ print("ERROR: \(error!.localizedDescription)"); sem.signal() }}
}}
sem.wait()
"#, file_path))
            .output().await;

        if let Ok(out) = output {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !text.is_empty() && !text.starts_with("ERROR:") {
                    return Ok(text);
                }
            }
        }
    }

    // 通用 fallback: whisper CLI（如果安装了）
    let whisper_output = tokio::process::Command::new("whisper")
        .args(&[file_path, "--model", "base", "--output_format", "txt"])
        .output().await;

    if let Ok(out) = whisper_output {
        if out.status.success() {
            // whisper 输出到同目录的 .txt 文件
            let txt_path = format!("{}.txt", file_path.strip_suffix(&format!(".{}",
                std::path::Path::new(file_path).extension().and_then(|e| e.to_str()).unwrap_or("")
            )).unwrap_or(file_path));
            if let Ok(text) = tokio::fs::read_to_string(&txt_path).await {
                let _ = tokio::fs::remove_file(&txt_path).await;
                return Ok(text.trim().to_string());
            }
        }
    }

    Err("本地语音转录不可用。请安装 whisper CLI（pip install openai-whisper）或配置 OpenAI Provider 使用 Whisper API。".into())
}

// ─── PDF/文档解析工具 ────────────────────────────────────────

/// PDF/文档解析工具
///
/// 支持 PDF/DOCX/TXT/CSV 文件解析为纯文本。
/// PDF: 优先使用 pdftotext（poppler），macOS 也可用 mdimport。
/// DOCX: 使用 pandoc 或简单 XML 解压提取。
pub struct DocParseTool;

#[async_trait]
impl Tool for DocParseTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "doc_parse".to_string(),
            description: "解析文档文件为纯文本。支持 PDF、DOCX、XLSX、XLS、TXT、CSV、Markdown 等格式。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "文档文件路径"
                    },
                    "pages": {
                        "type": "string",
                        "description": "PDF 页码范围（如 1-5, 3）。不填提取全部。"
                    },
                    "max_chars": {
                        "type": "integer",
                        "description": "最大返回字符数（默认 50000）"
                    }
                },
                "required": ["file_path"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Guarded }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let file_path = arguments["file_path"].as_str().ok_or("缺少 file_path")?;
        let max_chars = arguments["max_chars"].as_u64().unwrap_or(50000) as usize;
        let pages = arguments["pages"].as_str();

        let path = std::path::Path::new(file_path);
        if !path.exists() {
            return Err(format!("文件不存在: {}", file_path));
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

        let text = match ext.as_str() {
            "pdf" => parse_pdf(file_path, pages).await?,
            "txt" | "md" | "csv" | "tsv" | "log" | "json" | "xml" | "yaml" | "yml" | "toml" => {
                tokio::fs::read_to_string(file_path).await
                    .map_err(|e| format!("读取文件失败: {}", e))?
            }
            "docx" => parse_docx(file_path).await?,
            "xlsx" | "xls" | "xlsm" => parse_excel(file_path)?,
            _ => return Err(format!("不支持的文档格式: .{}。支持: pdf/docx/xlsx/xls/txt/md/csv/json/xml/yaml", ext)),
        };

        // 截断
        if text.len() > max_chars {
            let truncated: String = text.chars().take(max_chars).collect();
            Ok(format!("{}\n\n[文档已截断：显示前 {} 字符，总 {} 字符]", truncated, max_chars, text.len()))
        } else {
            Ok(text)
        }
    }
}

/// PDF 解析（pdftotext 或 macOS textutil）
async fn parse_pdf(file_path: &str, pages: Option<&str>) -> Result<String, String> {
    // 方案 1: pdftotext (poppler-utils)
    let mut args = vec![file_path.to_string(), "-".to_string()];
    if let Some(p) = pages {
        // 解析 "1-5" 或 "3"
        if let Some((first, last)) = p.split_once('-') {
            args.insert(0, "-l".to_string());
            args.insert(1, last.to_string());
            args.insert(0, "-f".to_string());
            args.insert(1, first.to_string());
        } else {
            args.insert(0, "-f".to_string());
            args.insert(1, p.to_string());
            args.insert(0, "-l".to_string());
            args.insert(1, p.to_string());
        }
    }

    let output = tokio::process::Command::new("pdftotext")
        .args(&args)
        .output().await;

    if let Ok(out) = output {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout).to_string();
            if !text.trim().is_empty() {
                return Ok(text);
            }
        }
    }

    // 方案 2: macOS 的 mdimport + textutil
    #[cfg(target_os = "macos")]
    {
        let output = tokio::process::Command::new("textutil")
            .args(&["-convert", "txt", "-stdout", file_path])
            .output().await;
        if let Ok(out) = output {
            if out.status.success() {
                return Ok(String::from_utf8_lossy(&out.stdout).to_string());
            }
        }
    }

    // 方案 3: Python pdfminer
    let output = tokio::process::Command::new("python3")
        .args(&["-c", &format!(
            "from pdfminer.high_level import extract_text; print(extract_text('{}'))",
            file_path.replace('\'', "\\'")
        )])
        .output().await;

    if let Ok(out) = output {
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).to_string());
        }
    }

    Err("PDF 解析失败。请安装 pdftotext（brew install poppler / apt install poppler-utils）".into())
}

/// DOCX 解析
async fn parse_docx(file_path: &str) -> Result<String, String> {
    // 方案 1: pandoc
    let output = tokio::process::Command::new("pandoc")
        .args(&[file_path, "-t", "plain"])
        .output().await;

    if let Ok(out) = output {
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).to_string());
        }
    }

    // 方案 2: macOS textutil
    #[cfg(target_os = "macos")]
    {
        let output = tokio::process::Command::new("textutil")
            .args(&["-convert", "txt", "-stdout", file_path])
            .output().await;
        if let Ok(out) = output {
            if out.status.success() {
                return Ok(String::from_utf8_lossy(&out.stdout).to_string());
            }
        }
    }

    // 方案 3: Python python-docx
    let output = tokio::process::Command::new("python3")
        .args(&["-c", &format!(
            "from docx import Document; d=Document('{}'); print('\\n'.join(p.text for p in d.paragraphs))",
            file_path.replace('\'', "\\'")
        )])
        .output().await;

    if let Ok(out) = output {
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).to_string());
        }
    }

    Err("DOCX 解析失败。请安装 pandoc（brew install pandoc / apt install pandoc）".into())
}

/// Excel 解析（纯 Rust，calamine crate，零外部依赖）
fn parse_excel(file_path: &str) -> Result<String, String> {
    use calamine::{Reader, open_workbook_auto};

    let mut workbook = open_workbook_auto(file_path)
        .map_err(|e| format!("Excel 文件打开失败: {}", e))?;

    let mut result = String::new();
    let sheet_names = workbook.sheet_names().to_vec();

    for (sheet_idx, sheet_name) in sheet_names.iter().enumerate() {
        if let Ok(range) = workbook.worksheet_range(sheet_name) {
            if sheet_idx > 0 { result.push_str("\n\n"); }
            result.push_str(&format!("## Sheet: {}\n\n", sheet_name));

            // 转为 Markdown 表格格式
            let mut rows_iter = range.rows();
            if let Some(header) = rows_iter.next() {
                let header_cells: Vec<String> = header.iter().map(|c| format!("{}", c)).collect();
                result.push_str("| ");
                result.push_str(&header_cells.join(" | "));
                result.push_str(" |\n");
                result.push_str("| ");
                result.push_str(&header_cells.iter().map(|_| "---").collect::<Vec<_>>().join(" | "));
                result.push_str(" |\n");
            }
            let mut row_count = 0;
            for row in rows_iter {
                let cells: Vec<String> = row.iter().map(|c| format!("{}", c)).collect();
                result.push_str("| ");
                result.push_str(&cells.join(" | "));
                result.push_str(" |\n");
                row_count += 1;
                if row_count >= 500 {
                    result.push_str(&format!("\n[... 已截断，共 {} 行数据 ...]\n", range.height()));
                    break;
                }
            }
            result.push_str(&format!("\n共 {} 行 x {} 列\n", range.height(), range.width()));
        }
    }

    if result.is_empty() {
        Err("Excel 文件为空或无法读取".into())
    } else {
        Ok(result)
    }
}

// ─── Agent 模板 ──────────────────────────────────────────────

/// 预设 Agent 模板列表
pub fn agent_templates() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "id": "translator",
            "name": "翻译助手",
            "description": "专业中英文翻译，保持原文风格",
            "system_prompt": "你是一位专业翻译。用户发中文时翻译为英文，发英文时翻译为中文。保持原文的语气和风格。只输出翻译结果，不添加解释。",
            "model": "gpt-4o-mini",
            "icon": "🌐"
        }),
        serde_json::json!({
            "id": "coder",
            "name": "编程助手",
            "description": "全栈开发助手，擅长代码编写和调试",
            "system_prompt": "你是一位全栈开发专家。擅长 Python、JavaScript/TypeScript、Rust、Go 等语言。回答编程问题时给出完整代码示例，说明关键逻辑。遇到 bug 先分析原因再给修复方案。",
            "model": "gpt-4o",
            "icon": "💻"
        }),
        serde_json::json!({
            "id": "writer",
            "name": "写作助手",
            "description": "文案写作、文章润色、内容创作",
            "system_prompt": "你是一位优秀的写作助手。擅长各类文体创作、文章润色、内容优化。根据用户需求调整风格（正式/轻松/学术/营销等）。注重逻辑清晰、用词精准、表达流畅。",
            "model": "gpt-4o",
            "icon": "✍️"
        }),
        serde_json::json!({
            "id": "analyst",
            "name": "数据分析师",
            "description": "数据分析、报表解读、趋势预测",
            "system_prompt": "你是一位数据分析专家。擅长数据解读、统计分析、趋势预测。能够处理 CSV/Excel 数据，生成分析报告。使用图表描述和数字佐证来表达观点。",
            "model": "gpt-4o",
            "icon": "📊"
        }),
        serde_json::json!({
            "id": "teacher",
            "name": "学习导师",
            "description": "知识讲解、概念解析、学习指导",
            "system_prompt": "你是一位耐心的学习导师。用简单易懂的方式解释复杂概念，善于用类比和例子帮助理解。根据学生水平调整讲解深度，鼓励提问和思考。",
            "model": "gpt-4o-mini",
            "icon": "🎓"
        }),
        serde_json::json!({
            "id": "assistant",
            "name": "通用助理",
            "description": "日常问答、信息整理、任务规划",
            "system_prompt": "你是一位高效的个人助理。帮助用户处理日常问题、整理信息、规划任务。回答准确简洁，必要时提供多个方案供选择。",
            "model": "gpt-4o-mini",
            "icon": "🤖"
        }),
        serde_json::json!({
            "id": "creative",
            "name": "创意顾问",
            "description": "头脑风暴、创意方案、营销策划",
            "system_prompt": "你是一位创意顾问。擅长头脑风暴、创意方案设计、营销策划。善于跳出常规思维，提供新颖独特的视角和解决方案。",
            "model": "gpt-4o",
            "icon": "💡"
        }),
    ]
}

// ─── AutoResearch 自主实验工具 ────────────────────────────────

/// 自主实验工具（参考 NuClaw AutoResearch）
///
/// Agent 可以设计实验 → 执行 → 评估结果 → 迭代优化。
/// 用于 prompt 优化、参数调优、A/B 测试等。
pub struct ResearchTool {
    pool: sqlx::SqlitePool,
}

impl ResearchTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl Tool for ResearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "research".to_string(),
            description: "自主实验工具。设计实验、执行测试、记录结果、评估效果。用于 prompt 优化、方案对比等。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "操作：create/run/log/evaluate/list",
                        "enum": ["create", "run", "log", "evaluate", "list"]
                    },
                    "experiment_name": { "type": "string", "description": "实验名称" },
                    "hypothesis": { "type": "string", "description": "假设（create 时）" },
                    "test_cases": { "type": "array", "description": "测试用例", "items": { "type": "object", "properties": { "input": { "type": "string" }, "expected": { "type": "string" } } } },
                    "result": { "type": "string", "description": "执行结果（log 时）" },
                    "metric": { "type": "string", "description": "评估指标（evaluate 时）" },
                    "score": { "type": "number", "description": "评分 0-100（evaluate 时）" }
                },
                "required": ["action"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Safe }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let action = arguments["action"].as_str().unwrap_or("list");

        // 实验数据存在 settings 表（简单方案）
        let _experiments_key = "research_experiments";

        match action {
            "create" => {
                let name = arguments["experiment_name"].as_str().ok_or("缺少 experiment_name")?;
                let hypothesis = arguments["hypothesis"].as_str().unwrap_or("");
                let id = format!("exp_{}", chrono::Utc::now().timestamp_millis() % 100000);
                let exp = serde_json::json!({
                    "id": id, "name": name, "hypothesis": hypothesis,
                    "created_at": chrono::Utc::now().to_rfc3339(),
                    "status": "active", "runs": [], "score": null,
                });

                let mut experiments = self.load_experiments().await;
                experiments.push(exp);
                self.save_experiments(&experiments).await?;
                Ok(format!("Experiment created: {} [{}]\nHypothesis: {}", name, id, hypothesis))
            }
            "log" => {
                let name = arguments["experiment_name"].as_str().ok_or("缺少 experiment_name")?;
                let result = arguments["result"].as_str().ok_or("缺少 result")?;
                let mut experiments = self.load_experiments().await;
                let found = experiments.iter_mut().find(|e| e["name"].as_str() == Some(name));
                if let Some(exp) = found {
                    if let Some(runs) = exp["runs"].as_array_mut() {
                        runs.push(serde_json::json!({
                            "timestamp": chrono::Utc::now().to_rfc3339(),
                            "result": result,
                        }));
                    }
                    self.save_experiments(&experiments).await?;
                    Ok(format!("Logged result for experiment '{}': {}", name, &result[..result.len().min(100)]))
                } else {
                    Err(format!("Experiment '{}' not found", name))
                }
            }
            "evaluate" => {
                let name = arguments["experiment_name"].as_str().ok_or("缺少 experiment_name")?;
                let score = arguments["score"].as_f64().ok_or("缺少 score")?;
                let metric = arguments["metric"].as_str().unwrap_or("overall");
                let mut experiments = self.load_experiments().await;
                let idx = experiments.iter().position(|e| e["name"].as_str() == Some(name));
                if let Some(i) = idx {
                    let status_str = if score >= 80.0 { "passed" } else { "needs_improvement" };
                    experiments[i]["score"] = serde_json::json!(score);
                    experiments[i]["metric"] = serde_json::json!(metric);
                    experiments[i]["status"] = serde_json::json!(status_str);
                    self.save_experiments(&experiments).await?;
                    Ok(format!("Experiment '{}' evaluated: {} = {}/100 ({})", name, metric, score, status_str))
                } else {
                    Err(format!("Experiment '{}' not found", name))
                }
            }
            "list" => {
                let experiments = self.load_experiments().await;
                if experiments.is_empty() {
                    return Ok("No experiments. Use `research action=create experiment_name=\"...\" hypothesis=\"...\"`".into());
                }
                let list: Vec<String> = experiments.iter().map(|e| {
                    let name = e["name"].as_str().unwrap_or("?");
                    let status = e["status"].as_str().unwrap_or("?");
                    let runs = e["runs"].as_array().map(|r| r.len()).unwrap_or(0);
                    let score = e["score"].as_f64().map(|s| format!("{:.0}", s)).unwrap_or("-".into());
                    format!("- {} [{}] runs={} score={}", name, status, runs, score)
                }).collect();
                Ok(format!("{} experiments:\n{}", experiments.len(), list.join("\n")))
            }
            _ => Err(format!("Unknown action: {}", action)),
        }
    }
}

impl ResearchTool {
    async fn load_experiments(&self) -> Vec<serde_json::Value> {
        let data: Option<String> = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = 'research_experiments'"
        ).fetch_optional(&self.pool).await.ok().flatten();
        data.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    async fn save_experiments(&self, experiments: &[serde_json::Value]) -> Result<(), String> {
        let json = serde_json::to_string(experiments).map_err(|e| e.to_string())?;
        sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('research_experiments', ?)")
            .bind(&json).execute(&self.pool).await.map_err(|e| e.to_string())?;
        Ok(())
    }
}

// ─── Outbound Webhook 工具 ──────────────────────────────────

/// Outbound HTTP/Webhook 工具
///
/// Agent 可主动发 HTTP 请求到外部 API（POST/PUT/PATCH/DELETE）。
/// 用于集成 Slack Webhook、IFTTT、Zapier、自建服务等。
pub struct HttpRequestTool;

#[async_trait]
impl Tool for HttpRequestTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "http_request".to_string(),
            description: "发送 HTTP 请求到外部 API/Webhook。支持 GET/POST/PUT/PATCH/DELETE，可设置 headers 和 JSON body。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "请求 URL" },
                    "method": { "type": "string", "description": "HTTP 方法: GET/POST/PUT/PATCH/DELETE（默认 POST）", "enum": ["GET", "POST", "PUT", "PATCH", "DELETE"] },
                    "headers": { "type": "object", "description": "请求头（键值对）" },
                    "body": { "type": "object", "description": "JSON 请求体" },
                    "timeout_secs": { "type": "integer", "description": "超时秒数（默认 30）" }
                },
                "required": ["url"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Approval }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let url = arguments["url"].as_str().ok_or("缺少 url")?;

        // 安全：只允许 http/https
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err("安全限制：只允许 http/https URL".into());
        }

        // 安全：禁止内网地址
        if url.contains("127.0.0.1") || url.contains("localhost") || url.contains("0.0.0.0")
            || url.contains("[::1]") || url.contains("169.254.") || url.contains("10.0.")
            || url.contains("192.168.") || url.contains("172.16.") {
            return Err("安全限制：禁止访问内网地址".into());
        }

        let method = arguments["method"].as_str().unwrap_or("POST").to_uppercase();
        let timeout = arguments["timeout_secs"].as_u64().unwrap_or(30);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout))
            .build().map_err(|e| e.to_string())?;

        let mut req = match method.as_str() {
            "GET" => client.get(url),
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "PATCH" => client.patch(url),
            "DELETE" => client.delete(url),
            _ => return Err(format!("不支持的 HTTP 方法: {}", method)),
        };

        // 设置 headers
        if let Some(headers) = arguments["headers"].as_object() {
            for (k, v) in headers {
                if let Some(val) = v.as_str() {
                    req = req.header(k.as_str(), val);
                }
            }
        }

        // 设置 body（非 GET 方法）
        if method != "GET" {
            if let Some(body) = arguments.get("body") {
                req = req.header("Content-Type", "application/json")
                    .json(body);
            }
        }

        log::info!("HTTP 外发请求: {} {}", method, url);

        let resp = req.send().await.map_err(|e| format!("请求失败: {}", e))?;
        let status = resp.status().as_u16();
        let headers_str: Vec<String> = resp.headers().iter().take(10)
            .map(|(k, v)| format!("{}: {}", k, v.to_str().unwrap_or("?")))
            .collect();

        let body = resp.text().await.unwrap_or_default();
        let truncated = if body.len() > 5000 { format!("{}...[truncated]", &body[..5000]) } else { body };

        Ok(format!("HTTP {} {}\nStatus: {}\nHeaders: {}\n\n{}",
            method, url, status, headers_str.join(", "), truncated))
    }
}

// ─── Focus 管理工具（Agent 自治意识）────────────────────────

/// Focus 管理工具 — Agent 自主管理工作记忆
///
/// 参考 Clawith Aware System。Agent 可以：
/// - 添加/更新/完成 focus items（结构化工作记忆）
/// - 查看当前 focus 状态
/// - 创建关联 trigger（自动化任务）
pub struct FocusTool;

#[async_trait]
impl Tool for FocusTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "focus".to_string(),
            description: "管理 Agent 工作记忆（Focus Items）。可添加/更新/完成任务项，查看当前状态。Agent 的自治意识核心。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "操作：list/add/update/complete/remove",
                        "enum": ["list", "add", "update", "complete", "remove"]
                    },
                    "item": { "type": "string", "description": "Focus item 内容（add/update 时）" },
                    "id": { "type": "string", "description": "Item ID（update/complete/remove 时）" },
                    "status": { "type": "string", "description": "状态标记：pending/in_progress/done", "enum": ["pending", "in_progress", "done"] },
                    "priority": { "type": "string", "description": "优先级：high/medium/low", "enum": ["high", "medium", "low"] }
                },
                "required": ["action"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Safe }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let action = arguments["action"].as_str().unwrap_or("list");

        // Focus 存储在 Agent 工作区的 FOCUS.md
        let focus_dir = dirs::home_dir()
            .unwrap_or_default()
            .join(".xianzhu/focus");
        let _ = std::fs::create_dir_all(&focus_dir);
        let focus_file = focus_dir.join("FOCUS.md");

        match action {
            "list" => {
                let content = std::fs::read_to_string(&focus_file).unwrap_or_default();
                if content.trim().is_empty() {
                    Ok("No focus items. Use `focus action=add item=\"...\"` to add one.".into())
                } else {
                    Ok(format!("Current Focus:\n\n{}", content))
                }
            }
            "add" => {
                let item = arguments["item"].as_str().ok_or("缺少 item")?;
                let priority = arguments["priority"].as_str().unwrap_or("medium");
                let id = format!("f{}", chrono::Utc::now().timestamp_millis() % 10000);
                let marker = "[ ]";
                let line = format!("{} [{}] ({}) {}\n", marker, id, priority, item);

                let mut content = std::fs::read_to_string(&focus_file).unwrap_or_default();
                content.push_str(&line);
                std::fs::write(&focus_file, &content).map_err(|e| e.to_string())?;
                Ok(format!("Added focus item: {} — {}", id, item))
            }
            "update" => {
                let id = arguments["id"].as_str().ok_or("缺少 id")?;
                let new_item = arguments["item"].as_str();
                let new_status = arguments["status"].as_str();
                let content = std::fs::read_to_string(&focus_file).unwrap_or_default();
                let mut updated = false;
                let new_content: String = content.lines().map(|line| {
                    if line.contains(&format!("[{}]", id)) {
                        updated = true;
                        let mut l = line.to_string();
                        if let Some(status) = new_status {
                            let marker = match status { "in_progress" => "[/]", "done" => "[x]", _ => "[ ]" };
                            l = l.replacen("[ ]", marker, 1).replacen("[/]", marker, 1).replacen("[x]", marker, 1);
                        }
                        if let Some(item) = new_item {
                            // 替换内容部分（保留标记和 ID）
                            if let Some(pos) = l.rfind(')') {
                                l = format!("{}) {}", &l[..pos], item);
                            }
                        }
                        l
                    } else { line.to_string() }
                }).collect::<Vec<_>>().join("\n");
                if updated {
                    std::fs::write(&focus_file, format!("{}\n", new_content.trim())).map_err(|e| e.to_string())?;
                    Ok(format!("Updated focus item: {}", id))
                } else {
                    Err(format!("Focus item {} not found", id))
                }
            }
            "complete" => {
                let id = arguments["id"].as_str().ok_or("缺少 id")?;
                let content = std::fs::read_to_string(&focus_file).unwrap_or_default();
                let new_content: String = content.lines().map(|line| {
                    if line.contains(&format!("[{}]", id)) {
                        line.replacen("[ ]", "[x]", 1).replacen("[/]", "[x]", 1)
                    } else { line.to_string() }
                }).collect::<Vec<_>>().join("\n");
                std::fs::write(&focus_file, format!("{}\n", new_content.trim())).map_err(|e| e.to_string())?;
                Ok(format!("Completed focus item: {}", id))
            }
            "remove" => {
                let id = arguments["id"].as_str().ok_or("缺少 id")?;
                let content = std::fs::read_to_string(&focus_file).unwrap_or_default();
                let new_content: String = content.lines()
                    .filter(|line| !line.contains(&format!("[{}]", id)))
                    .collect::<Vec<_>>().join("\n");
                std::fs::write(&focus_file, format!("{}\n", new_content.trim())).map_err(|e| e.to_string())?;
                Ok(format!("Removed focus item: {}", id))
            }
            _ => Err(format!("Unknown action: {}", action)),
        }
    }
}

// ─── Session 管理工具 ────────────────────────────────────────

/// Session 管理工具（创建/列表/切换/历史）
pub struct SessionTool {
    pool: sqlx::SqlitePool,
}

impl SessionTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl Tool for SessionTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "session".to_string(),
            description: "管理对话会话：创建新会话、列出会话、查看历史、导出。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "操作：list/create/history/export/compact",
                        "enum": ["list", "create", "history", "export", "compact"]
                    },
                    "agent_id": { "type": "string", "description": "Agent ID（list/create 时需要）" },
                    "session_id": { "type": "string", "description": "Session ID（history/export/compact 时需要）" },
                    "title": { "type": "string", "description": "新会话标题（create 时可选）" }
                },
                "required": ["action"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Safe }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let action = arguments["action"].as_str().unwrap_or("list");

        match action {
            "list" => {
                let agent_id = arguments["agent_id"].as_str().unwrap_or("");
                let sessions: Vec<(String, String, i64)> = if agent_id.is_empty() {
                    sqlx::query_as("SELECT id, title, created_at FROM chat_sessions ORDER BY COALESCE(last_message_at, created_at) DESC LIMIT 20")
                        .fetch_all(&self.pool).await.unwrap_or_default()
                } else {
                    sqlx::query_as("SELECT id, title, created_at FROM chat_sessions WHERE agent_id = ? ORDER BY COALESCE(last_message_at, created_at) DESC LIMIT 20")
                        .bind(agent_id).fetch_all(&self.pool).await.unwrap_or_default()
                };
                let list: Vec<String> = sessions.iter()
                    .map(|(id, title, _)| format!("- {} [{}]", title, &id[..id.len().min(8)]))
                    .collect();
                Ok(format!("{} sessions:\n{}", sessions.len(), list.join("\n")))
            }
            "create" => {
                let agent_id = arguments["agent_id"].as_str().ok_or("缺少 agent_id")?;
                let title = arguments["title"].as_str().unwrap_or("New Session");
                let id = uuid::Uuid::new_v4().to_string();
                let now = chrono::Utc::now().timestamp_millis();
                sqlx::query("INSERT INTO chat_sessions (id, agent_id, title, created_at) VALUES (?, ?, ?, ?)")
                    .bind(&id).bind(agent_id).bind(title).bind(now)
                    .execute(&self.pool).await.map_err(|e| e.to_string())?;
                Ok(format!("Session created: {} [{}]", title, &id[..8]))
            }
            "history" => {
                let session_id = arguments["session_id"].as_str().ok_or("缺少 session_id")?;
                let messages: Vec<(String, String)> = sqlx::query_as(
                    "SELECT role, COALESCE(content, '') FROM chat_messages WHERE session_id = ? ORDER BY seq DESC LIMIT 10"
                ).bind(session_id).fetch_all(&self.pool).await.unwrap_or_default();
                let list: Vec<String> = messages.iter().rev()
                    .map(|(role, content)| {
                        let preview: String = content.chars().take(100).collect();
                        format!("{}: {}", role, preview)
                    }).collect();
                Ok(format!("Last {} messages:\n{}", messages.len(), list.join("\n")))
            }
            "export" => {
                let session_id = arguments["session_id"].as_str().ok_or("缺少 session_id")?;
                let messages: Vec<(String, String)> = sqlx::query_as(
                    "SELECT role, COALESCE(content, '') FROM chat_messages WHERE session_id = ? ORDER BY seq ASC"
                ).bind(session_id).fetch_all(&self.pool).await.unwrap_or_default();
                let mut output = String::new();
                for (role, content) in &messages {
                    output.push_str(&format!("**{}**: {}\n\n", role, content));
                }
                Ok(output)
            }
            _ => Err(format!("未知操作: {}", action)),
        }
    }
}

// ─── 多 Agent 协作工具 ──────────────────────────────────────

/// Agent 协作工具（v2）
///
/// 通过 SubagentRegistry 消息队列实现 Agent 间通信：
/// - 向其他 Agent 发消息（带权限检查）
/// - 查看邮箱收件
/// - 发现可协作 Agent 及其关系
pub struct CollaborateTool {
    pool: sqlx::SqlitePool,
}

impl CollaborateTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl Tool for CollaborateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "collaborate".to_string(),
            description: "多 Agent 协作。发消息给其他 Agent、查看邮箱、发现可协作的 Agent。需要先建立关系（Delegate/Collaborator/Supervisor）才能发消息。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "操作类型",
                        "enum": ["send_message", "check_mailbox", "list_agents", "list_peers"]
                    },
                    "target_agent_id": { "type": "string", "description": "目标 Agent ID（send_message 时必填）" },
                    "message": { "type": "string", "description": "消息内容（send_message 时必填）" }
                },
                "required": ["action"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Safe }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let action = arguments["action"].as_str().unwrap_or("list_agents");
        let my_agent_id = arguments["_parent_agent_id"].as_str().unwrap_or("");

        match action {
            "list_agents" => {
                // 列出所有 Agent
                let agents: Vec<(String, String, String)> = sqlx::query_as(
                    "SELECT id, name, model FROM agents ORDER BY name"
                ).fetch_all(&self.pool).await.unwrap_or_default();
                let list: Vec<String> = agents.iter()
                    .map(|(id, name, model)| format!("- **{}** (模型: {}) `{}`", name, model, &id[..id.len().min(8)]))
                    .collect();
                Ok(format!("共 {} 个 Agent：\n{}", agents.len(), list.join("\n")))
            }
            "list_peers" => {
                // 列出当前 Agent 的关系（可通信的 Agent）
                if my_agent_id.is_empty() {
                    return Err("无法获取当前 Agent ID".into());
                }
                let relations = super::super::relations::RelationManager::get_relations(&self.pool, my_agent_id).await?;
                if relations.is_empty() {
                    return Ok("当前没有与其他 Agent 建立关系。请在「关系」页面创建 Delegate 或 Collaborator 关系后才能通信。".into());
                }
                let mut lines = Vec::new();
                for r in &relations {
                    let peer_id = if r.from_id == my_agent_id { &r.to_id } else { &r.from_id };
                    let peer_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = ?")
                        .bind(peer_id)
                        .fetch_optional(&self.pool).await.ok().flatten()
                        .unwrap_or_else(|| peer_id[..peer_id.len().min(8)].to_string());
                    let direction = if r.from_id == my_agent_id { "→" } else { "←" };
                    lines.push(format!("- {} **{}** ({}) `{}`", direction, peer_name, r.relation_type, &peer_id[..peer_id.len().min(8)]));
                }
                Ok(format!("{} 个关系：\n{}", relations.len(), lines.join("\n")))
            }
            "send_message" => {
                let target = arguments["target_agent_id"].as_str().ok_or("缺少 target_agent_id")?;
                let message = arguments["message"].as_str().ok_or("缺少 message")?;
                if my_agent_id.is_empty() {
                    return Err("无法获取当前 Agent ID".into());
                }
                // 通过 SubagentRegistry 发送（带权限检查）
                let orchestrator = super::super::delegate::get_orchestrator()
                    .map_err(|e| format!("Orchestrator 未初始化: {}", e))?;
                let msg = super::super::subagent::AgentMessage {
                    from: my_agent_id.to_string(),
                    to: target.to_string(),
                    content: message.to_string(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                };
                orchestrator.subagent_registry()
                    .send_message_checked(&self.pool, msg).await?;
                // 获取目标 Agent 名称
                let name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = ?")
                    .bind(target).fetch_optional(&self.pool).await.ok().flatten()
                    .unwrap_or_else(|| target[..target.len().min(8)].to_string());
                Ok(format!("消息已发送给 **{}**", name))
            }
            "check_mailbox" => {
                if my_agent_id.is_empty() {
                    return Err("无法获取当前 Agent ID".into());
                }
                // 从 SubagentRegistry 读取邮箱（非阻塞）
                let orchestrator = super::super::delegate::get_orchestrator()
                    .map_err(|e| format!("Orchestrator 未初始化: {}", e))?;
                // 尝试接收，0 秒超时 = 非阻塞检查
                let mut messages = Vec::new();
                for _ in 0..20 {
                    match orchestrator.subagent_registry()
                        .receive_message(my_agent_id, 0).await {
                        Ok(msg) => {
                            let sender_name: String = sqlx::query_scalar("SELECT name FROM agents WHERE id = ?")
                                .bind(&msg.from).fetch_optional(&self.pool).await.ok().flatten()
                                .unwrap_or_else(|| msg.from[..msg.from.len().min(8)].to_string());
                            messages.push(format!("- **{}**: {}", sender_name, msg.content));
                        }
                        Err(_) => break,
                    }
                }
                if messages.is_empty() {
                    Ok("邮箱为空，没有新消息。".into())
                } else {
                    Ok(format!("{} 条新消息：\n{}", messages.len(), messages.join("\n")))
                }
            }
            _ => Err(format!("未知操作: {}。支持: send_message/check_mailbox/list_agents/list_peers", action)),
        }
    }
}

// ─── Yield 工具 ─────────────────────────────────────────────

/// Sessions Yield 工具
///
/// Agent 调用此工具暂停当前轮次，等待子代理完成后恢复。
/// 参考 OpenClaw sessions_yield。
///
/// 用法：
/// 1. Agent 调用 delegate_task 派发任务
/// 2. Agent 调用 sessions_yield 暂停自己
/// 3. 子代理完成后，结果自动注入父 session
/// 4. 父 Agent 恢复执行，看到子代理结果
pub struct YieldTool;

#[async_trait]
impl Tool for YieldTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "sessions_yield".to_string(),
            description: "暂停当前轮次，等待子代理完成。调用 delegate_task 后使用此工具等待结果。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "暂停时显示的消息"
                    },
                    "wait_run_id": {
                        "type": "string",
                        "description": "要等待的子代理 run_id（从 delegate_task 返回值获取）"
                    }
                },
                "required": []
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Safe }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let message = arguments["message"].as_str().unwrap_or("Turn yielded.");
        let wait_run_id = arguments["wait_run_id"].as_str();

        // 返回特殊前缀，agent_loop 检测到后暂停
        if let Some(rid) = wait_run_id {
            Ok(format!("YIELD:wait:{}", rid))
        } else {
            Ok(format!("YIELD:{}", message))
        }
    }
}

// ─── A2A 对话工具 ───────────────────────────────────────────

/// Agent-to-Agent 对话工具（v2）
///
/// 通过完整 agent_loop 与目标 Agent 对话：
/// - 走 Orchestrator.send_message_stream（使用目标 Agent 的工具/人格/记忆）
/// - 对话持久化到 DB
/// - 带权限检查
pub struct A2aTool {
    pool: sqlx::SqlitePool,
}

impl A2aTool {
    pub fn new(pool: sqlx::SqlitePool) -> Self { Self { pool } }
}

#[async_trait]
impl Tool for A2aTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "agent_chat".to_string(),
            description: "与另一个 Agent 对话。消息走完整 agent_loop（使用目标 Agent 的工具和人格），对话持久化。需要先建立关系。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "target_agent_id": { "type": "string", "description": "目标 Agent ID" },
                    "message": { "type": "string", "description": "要发送的消息" },
                    "timeout_secs": { "type": "integer", "description": "超时秒数（默认 60）" }
                },
                "required": ["target_agent_id", "message"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Guarded }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let target_id = arguments["target_agent_id"].as_str().ok_or("缺少 target_agent_id")?;
        let message = arguments["message"].as_str().ok_or("缺少 message")?;
        let timeout = arguments["timeout_secs"].as_u64().unwrap_or(60);
        let my_agent_id = arguments["_parent_agent_id"].as_str().unwrap_or("");

        // 权限检查
        if !my_agent_id.is_empty() {
            let can = super::super::relations::RelationManager::can_communicate(
                &self.pool, my_agent_id, target_id
            ).await?;
            if !can {
                return Err(format!(
                    "与目标 Agent 没有协作关系，无法对话。请先在「关系」页面建立 Delegate 或 Collaborator 关系。"
                ));
            }
        }

        // 验证目标 Agent 存在并获取模型
        let target: Option<(String, String)> = sqlx::query_as(
            "SELECT name, model FROM agents WHERE id = ?"
        ).bind(target_id).fetch_optional(&self.pool).await.map_err(|e| e.to_string())?;
        let (target_name, target_model) = target.ok_or("目标 Agent 不存在")?;

        // 查找 provider
        let (api_type, api_key, base_url) = crate::channels::find_provider(&self.pool, &target_model)
            .await
            .ok_or("目标 Agent 无可用 Provider")?;

        // 获取 Orchestrator
        let orchestrator = super::super::delegate::get_orchestrator()
            .map_err(|e| format!("Orchestrator 未初始化: {}", e))?;

        // 在目标 Agent 下创建 A2A session
        let session_title = format!("[a2a] 来自 {}", if my_agent_id.is_empty() { "unknown" } else { &my_agent_id[..my_agent_id.len().min(8)] });
        let session = crate::memory::conversation::create_session(
            &self.pool, target_id, &session_title,
        ).await.map_err(|e| format!("创建 A2A session 失败: {}", e))?;

        // 收集输出
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let output_handle = tokio::spawn(async move {
            let mut output = String::new();
            while let Some(token) = rx.recv().await {
                output.push_str(&token);
            }
            output
        });

        let base_url_opt = if base_url.is_empty() { None } else { Some(base_url.as_str()) };

        // 通过完整 agent_loop 调用目标 Agent（使用其 workspace/工具/人格/记忆）
        let result = match tokio::time::timeout(
            std::time::Duration::from_secs(timeout),
            orchestrator.send_message_stream(
                target_id, &session.id, message,
                &api_key, &api_type, base_url_opt, tx, None,
            ),
        ).await {
            Ok(Ok(_)) => {
                let output = output_handle.await.unwrap_or_default();
                log::info!("A2A 对话完成: {} → {} ({}字符)", my_agent_id, target_name, output.len());
                Ok(output)
            }
            Ok(Err(e)) => Err(format!("{} 回复失败: {}", target_name, e)),
            Err(_) => Err(format!("{} 回复超时（{}秒）", target_name, timeout)),
        };

        match result {
            Ok(reply) => Ok(format!(
                "**{}** 的回复：\n\n{}",
                target_name,
                if reply.is_empty() { "(无回复内容)".to_string() } else { reply }
            )),
            Err(e) => Err(e),
        }
    }
}

// ──────────────────────────────────────────────────────────
// Excel/CSV 写入工具（纯 Rust，不依赖 Python）
// ──────────────────────────────────────────────────────────

pub struct DocWriteTool;

#[async_trait]
impl Tool for DocWriteTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "doc_write".to_string(),
            description: "将数据写入 Excel(.xlsx) 或 CSV 文件。输入 JSON 格式数据，自动生成表格文件。不需要 Python 或 pip。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "输出文件路径（.xlsx 或 .csv）"
                    },
                    "headers": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "列标题数组，如 [\"姓名\", \"年龄\", \"邮箱\"]"
                    },
                    "rows": {
                        "type": "array",
                        "items": { "type": "array", "items": { "type": "string" } },
                        "description": "数据行数组，每行是字符串数组，如 [[\"张三\", \"25\", \"a@b.com\"]]"
                    },
                    "sheet_name": {
                        "type": "string",
                        "description": "Sheet 名称（仅 xlsx，默认 Sheet1）"
                    }
                },
                "required": ["file_path", "rows"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Guarded }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let file_path = arguments["file_path"].as_str().ok_or("缺少 file_path")?;
        let ext = std::path::Path::new(file_path)
            .extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

        let headers: Vec<String> = arguments.get("headers")
            .and_then(|h| serde_json::from_value(h.clone()).ok())
            .unwrap_or_default();

        let rows: Vec<Vec<String>> = arguments["rows"].as_array()
            .ok_or("rows 必须是二维数组")?
            .iter()
            .map(|row| {
                row.as_array()
                    .map(|cells| cells.iter().map(|c| {
                        c.as_str().map(|s| s.to_string()).unwrap_or_else(|| c.to_string())
                    }).collect())
                    .unwrap_or_default()
            })
            .collect();

        match ext.as_str() {
            "csv" | "tsv" => {
                let sep = if ext == "tsv" { "\t" } else { "," };
                let mut content = String::new();
                if !headers.is_empty() {
                    content.push_str(&headers.join(sep));
                    content.push('\n');
                }
                for row in &rows {
                    content.push_str(&row.join(sep));
                    content.push('\n');
                }
                tokio::fs::write(file_path, &content).await
                    .map_err(|e| format!("写入失败: {}", e))?;
                Ok(format!("CSV 文件已写入: {} ({} 行)", file_path, rows.len()))
            }
            "xlsx" | "xls" => {
                write_xlsx(file_path, &headers, &rows, arguments.get("sheet_name").and_then(|s| s.as_str()))?;
                Ok(format!("Excel 文件已写入: {} ({} 行 x {} 列)", file_path, rows.len(),
                    headers.len().max(rows.first().map(|r| r.len()).unwrap_or(0))))
            }
            _ => Err(format!("不支持的格式: .{}。支持: xlsx/csv/tsv", ext)),
        }
    }
}

/// 纯 Rust xlsx 写入（使用简单的 OpenXML 结构）
fn write_xlsx(file_path: &str, headers: &[String], rows: &[Vec<String>], sheet_name: Option<&str>) -> Result<(), String> {
    use std::io::Write;
    let sheet = sheet_name.unwrap_or("Sheet1");
    let file = std::fs::File::create(file_path).map_err(|e| format!("创建文件失败: {}", e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // [Content_Types].xml
    zip.start_file("[Content_Types].xml", options).map_err(|e| e.to_string())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>"#).map_err(|e| e.to_string())?;

    // _rels/.rels
    zip.start_file("_rels/.rels", options).map_err(|e| e.to_string())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#).map_err(|e| e.to_string())?;

    // xl/_rels/workbook.xml.rels
    zip.start_file("xl/_rels/workbook.xml.rels", options).map_err(|e| e.to_string())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#).map_err(|e| e.to_string())?;

    // xl/workbook.xml
    zip.start_file("xl/workbook.xml", options).map_err(|e| e.to_string())?;
    write!(zip, r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="{}" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#, sheet).map_err(|e| e.to_string())?;

    // xl/worksheets/sheet1.xml
    zip.start_file("xl/worksheets/sheet1.xml", options).map_err(|e| e.to_string())?;
    let mut sheet_xml = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#);

    let mut row_num = 1u32;
    // 写标题行
    if !headers.is_empty() {
        sheet_xml.push_str(&format!("<row r=\"{}\">", row_num));
        for (ci, h) in headers.iter().enumerate() {
            let col = (b'A' + ci as u8) as char;
            let escaped = h.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
            sheet_xml.push_str(&format!("<c r=\"{}{}\" t=\"inlineStr\"><is><t>{}</t></is></c>", col, row_num, escaped));
        }
        sheet_xml.push_str("</row>");
        row_num += 1;
    }
    // 写数据行
    for row in rows {
        sheet_xml.push_str(&format!("<row r=\"{}\">", row_num));
        for (ci, cell) in row.iter().enumerate() {
            let col = if ci < 26 { format!("{}", (b'A' + ci as u8) as char) }
                      else { format!("{}{}", (b'A' + (ci / 26 - 1) as u8) as char, (b'A' + (ci % 26) as u8) as char) };
            let escaped = cell.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
            // 尝试解析为数字
            if cell.parse::<f64>().is_ok() {
                sheet_xml.push_str(&format!("<c r=\"{}{}\"><v>{}</v></c>", col, row_num, cell));
            } else {
                sheet_xml.push_str(&format!("<c r=\"{}{}\" t=\"inlineStr\"><is><t>{}</t></is></c>", col, row_num, escaped));
            }
        }
        sheet_xml.push_str("</row>");
        row_num += 1;
    }
    sheet_xml.push_str("</sheetData></worksheet>");
    write!(zip, "{}", sheet_xml).map_err(|e| e.to_string())?;

    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

// ──────────────────────────────────────────────────────────
// 剪贴板工具（跨平台：macOS pbcopy/pbpaste, Windows clip/PowerShell, Linux xclip）
// ──────────────────────────────────────────────────────────

pub struct ClipboardTool;

#[async_trait]
impl Tool for ClipboardTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "clipboard".to_string(),
            description: "读取或写入系统剪贴板。action=read 读取剪贴板内容，action=write 写入文本到剪贴板。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["read", "write"],
                        "description": "操作类型：read（读取）或 write（写入）"
                    },
                    "text": {
                        "type": "string",
                        "description": "要写入剪贴板的文本（仅 write 时需要）"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Guarded }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let action = arguments["action"].as_str().ok_or("缺少 action 参数")?;

        match action {
            "read" => clipboard_read().await,
            "write" => {
                let text = arguments["text"].as_str().ok_or("write 操作需要 text 参数")?;
                clipboard_write(text).await
            }
            _ => Err(format!("不支持的操作: {}，请用 read 或 write", action)),
        }
    }
}

async fn clipboard_read() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let output = tokio::process::Command::new("pbpaste")
            .output().await.map_err(|e| format!("pbpaste 失败: {}", e))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).to_string());
        }
        return Err("剪贴板读取失败".into());
    }
    #[cfg(target_os = "windows")]
    {
        let output = tokio::process::Command::new("powershell")
            .args(&["-Command", "Get-Clipboard"])
            .output().await.map_err(|e| format!("PowerShell 失败: {}", e))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).to_string());
        }
        return Err("剪贴板读取失败".into());
    }
    #[cfg(target_os = "linux")]
    {
        // 优先 xclip，备选 xsel
        for cmd in &["xclip", "xsel"] {
            let args: Vec<&str> = if *cmd == "xclip" {
                vec!["-selection", "clipboard", "-o"]
            } else {
                vec!["--clipboard", "--output"]
            };
            if let Ok(output) = tokio::process::Command::new(cmd).args(&args).output().await {
                if output.status.success() {
                    return Ok(String::from_utf8_lossy(&output.stdout).to_string());
                }
            }
        }
        return Err("剪贴板读取失败，请安装 xclip 或 xsel".into());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    Err("当前平台不支持剪贴板操作".into())
}

async fn clipboard_write(text: &str) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let mut child = tokio::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn().map_err(|e| format!("pbcopy 失败: {}", e))?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(text.as_bytes()).await.map_err(|e| format!("写入失败: {}", e))?;
            drop(stdin);
        }
        let status = child.wait().await.map_err(|e| format!("等待失败: {}", e))?;
        if status.success() {
            return Ok(format!("已复制到剪贴板（{} 字符）", text.len()));
        }
        return Err("写入剪贴板失败".into());
    }
    #[cfg(target_os = "windows")]
    {
        let escaped = text.replace('\'', "''");
        let output = tokio::process::Command::new("powershell")
            .args(&["-Command", &format!("Set-Clipboard '{}'", escaped)])
            .output().await.map_err(|e| format!("PowerShell 失败: {}", e))?;
        if output.status.success() {
            return Ok(format!("已复制到剪贴板（{} 字符）", text.len()));
        }
        return Err("写入剪贴板失败".into());
    }
    #[cfg(target_os = "linux")]
    {
        for cmd in &["xclip", "xsel"] {
            let args: Vec<&str> = if *cmd == "xclip" {
                vec!["-selection", "clipboard"]
            } else {
                vec!["--clipboard", "--input"]
            };
            if let Ok(mut child) = tokio::process::Command::new(cmd)
                .args(&args)
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    use tokio::io::AsyncWriteExt;
                    let _ = stdin.write_all(text.as_bytes()).await;
                    drop(stdin);
                }
                if let Ok(status) = child.wait().await {
                    if status.success() {
                        return Ok(format!("已复制到剪贴板（{} 字符）", text.len()));
                    }
                }
            }
        }
        return Err("写入剪贴板失败，请安装 xclip 或 xsel".into());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    Err("当前平台不支持剪贴板操作".into())
}

// ──────────────────────────────────────────────────────────
// 截图工具（macOS screencapture, Windows PowerShell, Linux import/scrot）
// ──────────────────────────────────────────────────────────

pub struct ScreenshotTool;

#[async_trait]
impl Tool for ScreenshotTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "screenshot".to_string(),
            description: "截取屏幕截图并保存到文件。可截取全屏或指定区域。返回截图文件路径。".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "output_path": {
                        "type": "string",
                        "description": "截图保存路径（可选，默认保存到桌面）"
                    },
                    "region": {
                        "type": "string",
                        "enum": ["fullscreen", "window", "selection"],
                        "description": "截图范围：fullscreen（全屏）、window（当前窗口）、selection（用户选区）。默认 fullscreen"
                    }
                }
            }),
        }
    }

    fn safety_level(&self) -> ToolSafetyLevel { ToolSafetyLevel::Guarded }

    async fn execute(&self, arguments: serde_json::Value) -> Result<String, String> {
        let region = arguments.get("region").and_then(|r| r.as_str()).unwrap_or("fullscreen");

        // 生成默认输出路径
        let output_path = if let Some(p) = arguments.get("output_path").and_then(|p| p.as_str()) {
            p.to_string()
        } else {
            let desktop = dirs::desktop_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Desktop"));
            let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
            desktop.join(format!("screenshot_{}.png", ts)).to_string_lossy().to_string()
        };

        // 确保父目录存在
        if let Some(parent) = std::path::Path::new(&output_path).parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        take_screenshot(&output_path, region).await?;

        // 检查文件是否生成
        if tokio::fs::metadata(&output_path).await.is_ok() {
            let size = tokio::fs::metadata(&output_path).await
                .map(|m| m.len()).unwrap_or(0);
            Ok(format!("截图已保存: {} ({:.1}KB)", output_path, size as f64 / 1024.0))
        } else {
            Err("截图文件未生成".into())
        }
    }
}

async fn take_screenshot(output_path: &str, region: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let mut args = vec!["-x".to_string()]; // 静音
        match region {
            "window" => args.push("-w".to_string()),
            "selection" => args.push("-s".to_string()),
            _ => {} // fullscreen 不需要额外参数
        }
        args.push(output_path.to_string());

        let output = tokio::process::Command::new("screencapture")
            .args(&args)
            .output().await.map_err(|e| format!("screencapture 失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("截图失败: {}", stderr));
        }
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        // PowerShell 截图（全屏）
        let ps_script = format!(
            r#"Add-Type -AssemblyName System.Windows.Forms; $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds; $bmp = New-Object System.Drawing.Bitmap($b.Width, $b.Height); $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size); $bmp.Save('{}')"#,
            output_path.replace('\'', "''")
        );
        let output = tokio::process::Command::new("powershell")
            .args(&["-Command", &ps_script])
            .output().await.map_err(|e| format!("PowerShell 截图失败: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("截图失败: {}", stderr));
        }
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        // 优先 import (ImageMagick)，备选 scrot
        let (cmd, args) = match region {
            "window" => ("import", vec!["-window", "root", output_path]),
            "selection" => ("import", vec![output_path]),
            _ => ("scrot", vec![output_path.to_string()].iter().map(|s| s.as_str()).collect()),
        };
        let output = tokio::process::Command::new(cmd)
            .args(&args)
            .output().await;

        if let Ok(out) = output {
            if out.status.success() { return Ok(()); }
        }

        // fallback: scrot
        let output = tokio::process::Command::new("scrot")
            .arg(output_path)
            .output().await.map_err(|e| format!("scrot 失败: {}", e))?;

        if output.status.success() { return Ok(()); }
        return Err("截图失败，请安装 scrot 或 ImageMagick".into());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    Err("当前平台不支持截图".into())
}
