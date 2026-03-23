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
            description: "检索 Agent 的长期记忆。支持语义搜索（向量）和关键词搜索（FTS5）。".to_string(),
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
            description: "将重要信息保存为 Agent 的长期记忆。记忆会跨会话持久保存，并自动建立全文索引和语义向量。".to_string(),
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
            description: "读取文件内容".to_string(),
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

        tokio::fs::read_to_string(path)
            .await
            .map_err(|e| format!("读取文件失败: {}", e))
    }
}

/// 网络搜索工具（占位）
pub struct WebSearchTool {
    #[allow(dead_code)]
    api_key: String,
}

impl WebSearchTool {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "web_search".to_string(),
            description: "在网络上搜索信息".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "搜索查询"
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
        let query = arguments
            .get("query")
            .and_then(|q| q.as_str())
            .ok_or("缺少 query 参数")?;

        log::info!("执行网络搜索: {}", query);

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        // 策略 1：Serper API（如果配置了 key）
        if let Ok(serper_key) = std::env::var("SERPER_API_KEY") {
            if !serper_key.is_empty() {
                match search_serper(&client, &serper_key, query).await {
                    Ok(results) => return Ok(results),
                    Err(e) => log::warn!("Serper 搜索失败，尝试备用: {}", e),
                }
            }
        }

        // 策略 2：DuckDuckGo HTML 搜索（免费，无需 API key）
        match search_duckduckgo(&client, query).await {
            Ok(results) => Ok(results),
            Err(e) => {
                log::warn!("DuckDuckGo 搜索失败: {}", e);
                // 策略 3：最终回退 — 提示用户使用 web_fetch 手动搜索
                Ok(format!("搜索暂不可用（{}）。你可以用 web_fetch 工具访问特定网页获取信息。", e))
            }
        }
    }
}

/// Serper.dev Google 搜索 API
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
async fn search_duckduckgo(client: &reqwest::Client, query: &str) -> Result<String, String> {
    // DuckDuckGo Instant Answer API
    let url = format!("https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1", urlencoding::encode(query));
    let resp = client.get(&url)
        .header("User-Agent", "YonClaw/0.2 (AI Assistant)")
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
async fn search_duckduckgo_lite(client: &reqwest::Client, query: &str) -> Result<String, String> {
    let url = format!("https://lite.duckduckgo.com/lite/?q={}", urlencoding::encode(query));
    let resp = client.get(&url)
        .header("User-Agent", "YonClaw/0.2 (AI Assistant)")
        .send().await.map_err(|e| format!("请求失败: {}", e))?;

    let html = resp.text().await.map_err(|e| format!("读取失败: {}", e))?;

    // 简单提取搜索结果（从 <a class="result-link"> 或 <td> 中）
    let mut results = Vec::new();
    let mut in_result = false;
    let mut current_title = String::new();
    let mut current_snippet = String::new();
    let mut count = 0;

    for line in html.lines() {
        let trimmed = line.trim();
        // 提取链接标题
        if trimmed.contains("result-link") || (trimmed.starts_with("<a") && trimmed.contains("rel=\"nofollow\"")) {
            // 提取 href 和文本
            if let Some(start) = trimmed.find('>') {
                if let Some(end) = trimmed[start..].find("</a>") {
                    current_title = trimmed[start + 1..start + end].to_string();
                    in_result = true;
                }
            }
        }
        // 提取摘要
        if in_result && trimmed.starts_with("<td>") && !trimmed.contains("<a") {
            current_snippet = trimmed.replace("<td>", "").replace("</td>", "").trim().to_string();
            if !current_title.is_empty() && !current_snippet.is_empty() {
                count += 1;
                results.push(format!("{}. **{}**\n   {}", count, current_title, current_snippet));
                current_title.clear();
                current_snippet.clear();
                in_result = false;
                if count >= 5 { break; }
            }
        }
    }

    if results.is_empty() {
        Err("无搜索结果".into())
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
            description: "执行终端命令。可以运行 shell 命令并返回输出结果。支持 ls、cat、grep、find、echo、node、npm、python3、git 等常用命令。".to_string(),
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
        let command = arguments
            .get("command")
            .and_then(|c| c.as_str())
            .ok_or("缺少 command 参数")?;

        // Shell 安全守卫
        crate::agent::sandbox::ShellGuard::validate_command(command)?;

        // 环境变量清洗
        let safe_env = crate::agent::sandbox::EnvSanitizer::sanitized_env();

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

        // 使用 sh -c 执行完整命令字符串（支持管道、重定向等）
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .env_clear()
                .envs(&safe_env)
                .env("PATH", &env_path)
                .output(),
        )
        .await
        .map_err(|_| "命令执行超时（30秒）".to_string())?
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
            description: "写入内容到文件。如果文件不存在则创建，如果存在则覆盖。自动创建父目录。".to_string(),
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
            description: "列出目录中的文件和子目录".to_string(),
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
            let name = entry.file_name().to_string_lossy().to_string();
            let marker = if file_type.is_dir() { "/" } else { "" };
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
            description: "精准编辑文件：在文件中查找 old_text 并替换为 new_text。支持多行文本。如果 old_text 为空则在 insert_line 位置插入 new_text。".to_string(),
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
            description: "在指定目录中搜索包含关键词的文件和行。支持递归搜索，返回匹配行及其文件路径和行号。".to_string(),
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
            description: "通过 HTTP GET 获取网页或 API 的文本内容。支持设置超时。返回响应体文本（最多 100KB）。".to_string(),
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
        // SSRF 防护：拒绝私有 IP 地址
        if let Ok(parsed) = url::Url::parse(url) {
            if let Some(host) = parsed.host_str() {
                // 检查是否为私有/保留 IP
                let is_private = host == "localhost"
                    || host == "127.0.0.1"
                    || host == "0.0.0.0"
                    || host == "::1"
                    || host.starts_with("10.")
                    || host.starts_with("192.168.")
                    || host.starts_with("169.254.")
                    || (host.starts_with("172.") && {
                        host.split('.').nth(1)
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
            .user_agent("YonClaw-Agent/0.1")
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
            description: "管理 Agent 技能：列出已安装技能、查看可安装技能、安装、卸载、搜索在线技能市场。用户说「帮我装个邮件技能」时使用此工具。".to_string(),
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
                let workspace = home.join(".yonclaw").join("agents").join(agent_id).join("skills");
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
                let mp_dir = home.join(".yonclaw").join("marketplace");
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
                let src = home.join(".yonclaw").join("marketplace").join(skill_name);
                if !src.exists() { return Err(format!("技能 {} 不在本地市场。先用 search_online 下载。", skill_name)); }
                let dst = home.join(".yonclaw").join("agents").join(agent_id).join("skills").join(skill_name);
                if dst.exists() { return Ok(format!("技能 {} 已安装。", skill_name)); }
                let _ = std::fs::create_dir_all(dst.parent().unwrap());
                copy_dir_recursive(&src, &dst).map_err(|e| format!("安装失败: {}", e))?;
                Ok(format!("✅ 技能 {} 已安装！后续对话中会自动使用。", skill_name))
            }
            "uninstall" => {
                let skill_name = args["skill_name"].as_str().ok_or("缺少 skill_name")?;
                let target = home.join(".yonclaw").join("agents").join(agent_id).join("skills").join(skill_name);
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
                    "timezone": { "type": "string", "description": "时区，默认 Asia/Shanghai" }
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
                let id = uuid::Uuid::new_v4().to_string();
                let now = chrono::Utc::now().timestamp_millis();
                let schedule = serde_json::json!({"kind":"cron","expr":cron_expr,"tz":tz}).to_string();
                let payload = serde_json::json!({"type":"agent","prompt":prompt,"sessionStrategy":"new"}).to_string();
                sqlx::query("INSERT INTO cron_jobs (id, agent_id, name, job_type, schedule, action_payload, enabled, timeout_secs, created_at, updated_at) VALUES (?,?,?,'agent',?,?,1,300,?,?)")
                    .bind(&id).bind(agent_id).bind(name).bind(&schedule).bind(&payload).bind(now).bind(now)
                    .execute(&self.pool).await.map_err(|e| format!("创建失败: {}", e))?;
                Ok(format!("✅ 定时任务已创建: {} | {} ({}) | {}", name, cron_expr, tz, &id[..8]))
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
