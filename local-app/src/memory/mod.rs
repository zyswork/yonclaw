//! 记忆体系统模块
//!
//! 三层记忆架构：
//! 1. 对话记忆（短期）— conversations 表
//! 2. 长期记忆（策展）— MEMORY.md + memory/YYYY-MM-DD.md
//! 3. 向量记忆（语义）— vectors 表 + FTS5 全文搜索

pub mod chunker;
pub mod conversation;
pub mod embedding;
pub mod factory;
#[cfg(feature = "lancedb")]
pub mod lance;
pub mod loader;
pub mod long_term;
pub mod vector;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

/// 记忆优先级（决定淘汰顺序）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MemoryPriority {
    /// 低优先级 — 优先淘汰
    Low = 0,
    /// 普通优先级
    Normal = 1,
    /// 高优先级 — 重要任务、决策
    High = 2,
    /// 关键 — 永不自动淘汰（核心规则、用户偏好）
    Critical = 3,
}

impl MemoryPriority {
    pub fn as_i32(&self) -> i32 { *self as i32 }
    pub fn from_i32(v: i32) -> Self {
        match v {
            0 => Self::Low,
            2 => Self::High,
            3 => Self::Critical,
            _ => Self::Normal,
        }
    }
}

/// 记忆分类
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryCategory {
    /// 核心灵魂文件（SOUL.md, IDENTITY.md 等）
    Core,
    /// 每日记忆日志（memory/YYYY-MM-DD.md）
    Daily,
    /// 对话历史
    Conversation,
    /// RAG 知识库内容
    Knowledge,
    /// 自定义分类
    Custom(String),
}

impl MemoryCategory {
    /// 转换为数据库存储的字符串
    pub fn as_str(&self) -> &str {
        match self {
            MemoryCategory::Core => "core",
            MemoryCategory::Daily => "daily",
            MemoryCategory::Conversation => "conversation",
            MemoryCategory::Knowledge => "knowledge",
            MemoryCategory::Custom(s) => s.as_str(),
        }
    }

    /// 从字符串解析
    pub fn from_str(s: &str) -> Self {
        match s {
            "core" => MemoryCategory::Core,
            "daily" => MemoryCategory::Daily,
            "conversation" => MemoryCategory::Conversation,
            "knowledge" => MemoryCategory::Knowledge,
            other => MemoryCategory::Custom(other.to_string()),
        }
    }
}

/// 记忆条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// 记忆 ID
    pub id: String,
    /// 记忆键名
    pub key: String,
    /// 记忆内容
    pub content: String,
    /// 记忆分类
    pub category: MemoryCategory,
    /// 优先级（决定淘汰顺序）
    pub priority: MemoryPriority,
    /// 创建时间戳（毫秒）
    pub timestamp: i64,
    /// 语义检索相关性分数（0.0 ~ 1.0）
    pub score: Option<f64>,
}

/// Memory Trait — 记忆体抽象接口
#[async_trait]
pub trait Memory: Send + Sync {
    /// 存储记忆（默认 Normal 优先级）
    async fn store(&self, agent_id: &str, key: &str, content: &str, category: MemoryCategory) -> Result<String, String>;

    /// 存储记忆（指定优先级）
    async fn store_with_priority(&self, agent_id: &str, key: &str, content: &str, category: MemoryCategory, _priority: MemoryPriority) -> Result<String, String> {
        // 默认实现：忽略优先级，调用 store
        self.store(agent_id, key, content, category).await
    }

    /// 语义检索记忆（返回按相关性排序的结果）
    async fn recall(&self, agent_id: &str, query: &str, limit: usize) -> Result<Vec<MemoryEntry>, String>;

    /// 按 key 获取单条记忆
    async fn get(&self, agent_id: &str, key: &str) -> Result<Option<MemoryEntry>, String>;

    /// 列出指定分类的记忆
    async fn list(&self, agent_id: &str, category: MemoryCategory) -> Result<Vec<MemoryEntry>, String>;

    /// 删除记忆
    async fn forget(&self, agent_id: &str, key: &str) -> Result<(), String>;

    /// 按优先级淘汰旧记忆（从 Low 开始淘汰，Critical 不淘汰）
    async fn evict_by_priority(&self, agent_id: &str, max_entries: usize) -> Result<u64, String> {
        let _ = (agent_id, max_entries);
        Ok(0) // 默认不淘汰
    }
}

/// 基于 SQLite 的记忆实现（可选 LanceDB 向量存储）
pub struct SqliteMemory {
    pool: SqlitePool,
    /// 嵌入客户端（可选，配置了 API key 才启用向量检索）
    embedder: Option<embedding::EmbeddingClient>,
    /// LanceDB 向量存储（可选，需启用 lancedb feature）
    #[cfg(feature = "lancedb")]
    lance_store: Option<Arc<lance::LanceVectorStore>>,
}

impl SqliteMemory {
    /// 创建 SQLite 记忆实例（纯 FTS5，无向量）
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool, embedder: None,
            #[cfg(feature = "lancedb")]
            lance_store: None,
        }
    }

    /// 创建带嵌入能力的记忆实例（FTS5 关键词 + 嵌入缓存 + 可选 LanceDB 向量）
    pub async fn with_embedding(pool: SqlitePool, config: embedding::EmbeddingConfig) -> Self {
        if config.api_key.is_empty() {
            log::info!("嵌入 API 未配置，使用纯 FTS5 检索");
            return Self {
                pool, embedder: None,
                #[cfg(feature = "lancedb")]
                lance_store: None,
            };
        }
        let _dims = config.dimensions;

        #[cfg(feature = "lancedb")]
        let lance = {
            log::info!("嵌入已启用: model={}, dimensions={}, 缓存=SQLite, 向量=LanceDB", config.model, _dims);
            match lance::LanceVectorStore::new(&lance::LanceVectorStore::default_path(), _dims).await {
                Ok(store) => Some(Arc::new(store)),
                Err(e) => {
                    log::warn!("LanceDB 初始化失败，回退到 SQLite 向量: {}", e);
                    None
                }
            }
        };

        #[cfg(not(feature = "lancedb"))]
        log::info!("嵌入已启用: model={}, dimensions={}, 缓存=SQLite, 向量=SQLite", config.model, _dims);

        Self {
            embedder: Some(embedding::EmbeddingClient::with_cache(config, pool.clone())),
            pool,
            #[cfg(feature = "lancedb")]
            lance_store: lance,
        }
    }

    /// 尝试从 settings 表加载嵌入配置
    pub async fn try_load_embedding_config(pool: &SqlitePool) -> Option<embedding::EmbeddingConfig> {
        // 从 settings 表读取嵌入配置
        let api_key: Option<String> = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = 'embedding_api_key'"
        ).fetch_optional(pool).await.ok().flatten();

        let api_key = api_key.filter(|k| !k.trim().is_empty())?;

        let api_url: String = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = 'embedding_api_url'"
        ).fetch_optional(pool).await.ok().flatten()
            .unwrap_or_else(|| "https://api.openai.com/v1/embeddings".to_string());

        let model: String = sqlx::query_scalar(
            "SELECT value FROM settings WHERE key = 'embedding_model'"
        ).fetch_optional(pool).await.ok().flatten()
            .unwrap_or_else(|| "text-embedding-3-small".to_string());

        let dimensions: usize = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = 'embedding_dimensions'"
        ).fetch_optional(pool).await.ok().flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1024);

        Some(embedding::EmbeddingConfig {
            api_url,
            api_key,
            model,
            dimensions,
        })
    }

    /// 获取连接池引用
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// 是否启用了向量嵌入
    pub fn has_embedding(&self) -> bool {
        self.embedder.is_some()
    }
}

#[async_trait]
impl Memory for SqliteMemory {
    async fn store(&self, agent_id: &str, key: &str, content: &str, category: MemoryCategory) -> Result<String, String> {
        self.store_with_priority(agent_id, key, content, category, MemoryPriority::Normal).await
    }

    async fn store_with_priority(&self, agent_id: &str, key: &str, content: &str, category: MemoryCategory, priority: MemoryPriority) -> Result<String, String> {
        // 如果提供了 key，用作 id，使 get(key) 和 forget(key) 能正确匹配
        let id = if key.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            key.to_string()
        };
        let now = chrono::Utc::now().timestamp_millis();
        let cat_str = category.as_str().to_string();

        // 写入 memories 表
        sqlx::query(
            "INSERT INTO memories (id, agent_id, memory_type, content, created_at, updated_at, priority) VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&id)
        .bind(agent_id)
        .bind(&cat_str)
        .bind(content)
        .bind(now)
        .bind(now)
        .bind(priority.as_i32())
        .execute(&self.pool)
        .await
        .map_err(|e| format!("存储记忆失败: {}", e))?;

        // 同步到 FTS5 索引
        let _ = sqlx::query(
            "INSERT INTO memories_fts (content, agent_id, memory_id) VALUES (?, ?, ?)"
        )
        .bind(content)
        .bind(agent_id)
        .bind(&id)
        .execute(&self.pool)
        .await;

        // 有嵌入配置时，分块后生成向量并存入 vectors 表
        if let Some(ref embedder) = self.embedder {
            let chunks = chunker::chunk_text(content, &chunker::ChunkConfig::default());
            // 短文档（未达分块阈值）作为整体处理
            let texts_to_embed: Vec<&str> = if chunks.is_empty() {
                vec![content]
            } else {
                chunks.iter().map(|c| c.content.as_str()).collect()
            };

            let mut success_count = 0usize;
            for chunk_text in &texts_to_embed {
                match embedder.embed(chunk_text).await {
                    Ok(emb) => {
                        let _chunk_id = uuid::Uuid::new_v4().to_string();
                        let mut _stored = false;

                        #[cfg(feature = "lancedb")]
                        if let Some(ref lance) = self.lance_store {
                            if let Err(e) = lance.insert(&_chunk_id, agent_id, chunk_text, &emb, &cat_str).await {
                                log::warn!("LanceDB 存储失败，回退 SQLite: {}", e);
                            } else {
                                _stored = true;
                            }
                        }

                        if !_stored {
                            let emb_bytes = embedding::embedding_to_bytes(&emb);
                            if let Err(e) = vector::save_vector(&self.pool, agent_id, chunk_text, emb_bytes).await {
                                log::warn!("向量存储失败（FTS5 已保存）: {}", e);
                            }
                        }
                        success_count += 1;
                    }
                    Err(e) => {
                        log::warn!("嵌入生成失败（FTS5 已保存）: {}", e);
                        break;
                    }
                }
            }
            #[cfg(feature = "lancedb")]
            let store_type = if self.lance_store.is_some() { "LanceDB" } else { "SQLite" };
            #[cfg(not(feature = "lancedb"))]
            let store_type = "SQLite";
            if success_count > 0 {
                log::debug!("向量已生成并存储({}): {} 块, dim={}", store_type, success_count, self.embedder.as_ref().map(|e| e.config().dimensions).unwrap_or(0));
            }
        }

        log::info!("记忆已存储: agent_id={}, key={}, category={}, vector={}", agent_id, key, cat_str, self.embedder.is_some());
        Ok(id)
    }

    async fn recall(&self, agent_id: &str, query: &str, limit: usize) -> Result<Vec<MemoryEntry>, String> {
        // 查询扩展：提取关键词，扩展 FTS 搜索范围
        let expanded_query = expand_query(query);

        // 有嵌入配置时：RRF 混合搜索（Reciprocal Rank Fusion）
        if let Some(ref embedder) = self.embedder {
            if let Ok(query_emb) = embedder.embed(query).await {
                // 向量搜索
                let vector_results = {
                    #[cfg(feature = "lancedb")]
                    if let Some(ref lance) = self.lance_store {
                        match lance.search(agent_id, &query_emb, limit * 3, None).await {
                            Ok(results) => results.into_iter()
                                .map(|r| (r.id, r.content, r.score))
                                .collect::<Vec<_>>(),
                            Err(e) => {
                                log::warn!("LanceDB 搜索失败，回退 SQLite: {}", e);
                                vector::hybrid_search(&self.pool, agent_id, query, Some(&query_emb), (limit * 3) as i64)
                                    .await.unwrap_or_default()
                            }
                        }
                    } else {
                        vector::hybrid_search(&self.pool, agent_id, query, Some(&query_emb), (limit * 3) as i64)
                            .await.unwrap_or_default()
                    }

                    #[cfg(not(feature = "lancedb"))]
                    {
                        vector::hybrid_search(&self.pool, agent_id, query, Some(&query_emb), (limit * 3) as i64)
                            .await.unwrap_or_default()
                    }
                };

                // FTS5 关键词搜索（使用扩展查询，含 importance 置信度）
                let fts_rows = sqlx::query_as::<_, (String, String, String, i64, i32)>(
                    "SELECT m.id, m.memory_type, m.content, m.created_at, COALESCE(m.importance, 5) FROM memories_fts f JOIN memories m ON f.memory_id = m.id WHERE f.agent_id = ? AND memories_fts MATCH ? ORDER BY rank LIMIT ?"
                )
                .bind(agent_id).bind(&expanded_query).bind((limit * 3) as i64)
                .fetch_all(&self.pool).await.unwrap_or_default();

                // RRF 融合（参考 IronClaw 的 Reciprocal Rank Fusion）
                // score(d) = Σ 1/(k + rank)，k=60
                // 最终分数加入 importance 置信度因子：final_score = rrf_score * (1 + importance * 0.1)
                const RRF_K: f64 = 60.0;

                // content → (rrf_score, id, memory_type, created_at, importance)
                let mut score_map: std::collections::HashMap<String, (f64, String, String, i64, i32)> =
                    std::collections::HashMap::new();

                // 向量排名贡献
                for (rank, (_vid, content, _cosine)) in vector_results.iter().enumerate() {
                    let rrf = 1.0 / (RRF_K + rank as f64 + 1.0);
                    let e = score_map.entry(content.clone()).or_insert((0.0, String::new(), String::new(), 0, 5));
                    e.0 += rrf;
                }

                // FTS 排名贡献（含 importance）
                for (rank, (id, memory_type, content, created_at, importance)) in fts_rows.into_iter().enumerate() {
                    let rrf = 1.0 / (RRF_K + rank as f64 + 1.0);
                    let e = score_map.entry(content.clone()).or_insert((0.0, String::new(), String::new(), 0, 5));
                    e.0 += rrf;
                    if e.1.is_empty() { e.1 = id; }
                    if e.2.is_empty() { e.2 = memory_type; }
                    if e.3 == 0 { e.3 = created_at; }
                    // 取最高 importance（同一内容可能来自多个条目）
                    if importance > e.4 { e.4 = importance; }
                }

                // 应用 importance 置信度因子到最终分数
                let mut results: Vec<_> = score_map.into_iter()
                    .map(|(content, (rrf_score, id, cat, ts, importance))| {
                        let boosted = rrf_score * (1.0 + importance as f64 * 0.1);
                        (content, (boosted, id, cat, ts, importance))
                    })
                    .collect();
                results.sort_by(|a, b| b.1.0.partial_cmp(&a.1.0).unwrap_or(std::cmp::Ordering::Equal));
                results.truncate(limit);

                // 归一化分数到 [0, 1]
                let max_score = results.first().map(|(_, (s, ..))| *s).unwrap_or(1.0);
                let norm_factor = if max_score > 0.0 { 1.0 / max_score } else { 1.0 };

                log::info!("RRF 混合检索（含置信度）: agent={}, 向量={}, FTS 结果={}", agent_id, vector_results.len(), results.len());

                return Ok(results.into_iter().map(|(content, (score, id, cat, ts, _imp))| MemoryEntry {
                    id, key: String::new(), content,
                    category: MemoryCategory::from_str(&cat),
                    priority: MemoryPriority::Normal,
                    timestamp: ts, score: Some(score * norm_factor),
                }).collect());
            } else {
                log::warn!("查询嵌入失败，回退到 FTS5");
            }
        }

        // 纯 FTS5 全文搜索（使用扩展查询）
        let rows = sqlx::query_as::<_, (String, String, String, i64)>(
            "SELECT m.id, m.memory_type, m.content, m.created_at FROM memories_fts f JOIN memories m ON f.memory_id = m.id WHERE f.agent_id = ? AND memories_fts MATCH ? ORDER BY rank LIMIT ?"
        )
        .bind(agent_id).bind(&expanded_query).bind(limit as i64)
        .fetch_all(&self.pool).await
        .map_err(|e| format!("语义检索失败: {}", e))?;

        Ok(rows.into_iter().enumerate().map(|(i, (id, memory_type, content, created_at))| MemoryEntry {
            id, key: String::new(), content,
            category: MemoryCategory::from_str(&memory_type),
            timestamp: created_at,
            priority: MemoryPriority::Normal,
            score: Some(1.0 - (i as f64 * 0.1).min(0.9)),
        }).collect())
    }

    async fn get(&self, agent_id: &str, key: &str) -> Result<Option<MemoryEntry>, String> {
        let row = sqlx::query_as::<_, (String, String, String, i64)>(
            "SELECT id, memory_type, content, created_at FROM memories WHERE agent_id = ? AND id = ? LIMIT 1"
        )
        .bind(agent_id)
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| format!("获取记忆失败: {}", e))?;

        Ok(row.map(|(id, memory_type, content, created_at)| MemoryEntry {
            id,
            key: key.to_string(),
            content,
            category: MemoryCategory::from_str(&memory_type),
            priority: MemoryPriority::Normal,
            timestamp: created_at,
            score: None,
        }))
    }

    async fn list(&self, agent_id: &str, category: MemoryCategory) -> Result<Vec<MemoryEntry>, String> {
        let cat_str = category.as_str().to_string();
        let rows = sqlx::query_as::<_, (String, String, i64)>(
            "SELECT id, content, created_at FROM memories WHERE agent_id = ? AND memory_type = ? ORDER BY created_at DESC"
        )
        .bind(agent_id)
        .bind(&cat_str)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("列出记忆失败: {}", e))?;

        Ok(rows
            .into_iter()
            .map(|(id, content, created_at)| MemoryEntry {
                id,
                key: String::new(),
                content,
                category: category.clone(),
                priority: MemoryPriority::Normal,
                timestamp: created_at,
                score: None,
            })
            .collect())
    }

    async fn forget(&self, agent_id: &str, key: &str) -> Result<(), String> {
        // 从 FTS5 索引删除
        let _ = sqlx::query("DELETE FROM memories_fts WHERE memory_id = ?")
            .bind(key)
            .execute(&self.pool)
            .await;

        // 从 memories 表删除
        sqlx::query("DELETE FROM memories WHERE agent_id = ? AND id = ?")
            .bind(agent_id)
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(|e| format!("删除记忆失败: {}", e))?;

        log::info!("记忆已删除: agent_id={}, id={}", agent_id, key);
        Ok(())
    }

    async fn evict_by_priority(&self, agent_id: &str, max_entries: usize) -> Result<u64, String> {
        // 统计当前条目数
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM memories WHERE agent_id = ?")
            .bind(agent_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| format!("统计记忆失败: {}", e))?;

        if (count.0 as usize) <= max_entries {
            return Ok(0);
        }

        let to_delete = count.0 as usize - max_entries;

        // 按优先级升序（Low 先淘汰）、时间升序（旧的先淘汰）删除
        // Critical(3) 不在删除范围内
        let result = sqlx::query(
            r#"
            DELETE FROM memories WHERE id IN (
                SELECT id FROM memories
                WHERE agent_id = ? AND COALESCE(priority, 1) < 3
                ORDER BY COALESCE(priority, 1) ASC, created_at ASC
                LIMIT ?
            )
            "#,
        )
        .bind(agent_id)
        .bind(to_delete as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| format!("淘汰记忆失败: {}", e))?;

        let deleted = result.rows_affected();
        if deleted > 0 {
            log::info!("优先级淘汰: agent_id={}, 删除 {} 条记忆（保留 Critical）", agent_id, deleted);
        }
        Ok(deleted)
    }
}

// ─── 三层存储：Hot（内存 LRU）→ Warm（SQLite）→ Cold（归档文件） ───

/// Hot 层内存缓存容量
const HOT_CACHE_CAPACITY: usize = 500;
/// Hot → Warm 回写间隔（秒）
const HOT_TTL_SECS: u64 = 300; // 5 分钟

/// 三层记忆存储
///
/// - Hot: 内存 LRU 缓存（最近访问的记忆，0-5分钟）
/// - Warm: SQLite 数据库（主存储，所有记忆）
/// - Cold: 归档文件（超过 N 天的记忆导出到 workspace/memory/archive/）
pub struct TieredMemory {
    /// Hot 层：内存 LRU（key → MemoryEntry）
    hot: std::sync::Mutex<lru::LruCache<String, (MemoryEntry, std::time::Instant)>>,
    /// Warm 层：SQLite
    warm: SqliteMemory,
    /// Cold 归档目录（可选）
    cold_dir: Option<std::path::PathBuf>,
}

impl TieredMemory {
    /// 创建三层存储
    pub fn new(warm: SqliteMemory, cold_dir: Option<std::path::PathBuf>) -> Self {
        Self {
            hot: std::sync::Mutex::new(lru::LruCache::new(
                std::num::NonZeroUsize::new(HOT_CACHE_CAPACITY).unwrap()
            )),
            warm,
            cold_dir,
        }
    }

    /// 从 Cold 层（归档文件）中按关键词召回记忆
    ///
    /// 读取 `workspace/memory/archive/` 下的 markdown 文件，
    /// 按 `##` 标题分割条目，对每个条目进行关键词匹配。
    pub fn recall_from_archive(&self, query: &str, limit: usize) -> Vec<MemoryEntry> {
        let cold_dir = match &self.cold_dir {
            Some(d) => d,
            None => return Vec::new(),
        };
        let archive_dir = cold_dir.join("archive");
        if !archive_dir.exists() {
            return Vec::new();
        }

        // 提取查询关键词（小写化）
        let keywords: Vec<String> = query.split_whitespace()
            .map(|w| w.to_lowercase())
            .filter(|w| w.len() >= 2) // 跳过过短的词
            .collect();
        if keywords.is_empty() {
            return Vec::new();
        }

        let mut results: Vec<MemoryEntry> = Vec::new();

        // 遍历归档目录下的 .md 文件
        let entries = match std::fs::read_dir(&archive_dir) {
            Ok(e) => e,
            Err(e) => {
                log::warn!("读取归档目录失败: {}", e);
                return Vec::new();
            }
        };

        for dir_entry in entries.flatten() {
            let path = dir_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // 按 ## 标题分割条目
            for section in content.split("\n## ") {
                let section_trimmed = section.trim();
                if section_trimmed.is_empty() {
                    continue;
                }

                let lower = section_trimmed.to_lowercase();
                // 计算匹配的关键词数量作为分数
                let matched: usize = keywords.iter()
                    .filter(|kw| lower.contains(kw.as_str()))
                    .count();

                if matched == 0 {
                    continue;
                }

                // 解析标题行获取 memory_type 和 id（格式：[type] id）
                let first_line = section_trimmed.lines().next().unwrap_or("");
                let (memory_type, entry_id) = if first_line.starts_with('[') {
                    if let Some(end) = first_line.find(']') {
                        let mtype = first_line[1..end].to_string();
                        let eid = first_line[end+1..].trim().to_string();
                        (mtype, eid)
                    } else {
                        ("archive".to_string(), first_line.to_string())
                    }
                } else {
                    ("archive".to_string(), first_line.to_string())
                };

                // 条目内容（跳过标题行）
                let body: String = section_trimmed.lines().skip(1)
                    .collect::<Vec<_>>().join("\n").trim().to_string();

                let score = matched as f64 / keywords.len() as f64;

                results.push(MemoryEntry {
                    id: format!("cold:{}", entry_id),
                    key: format!("archive/{}", memory_type),
                    content: if body.is_empty() { first_line.to_string() } else { body },
                    category: MemoryCategory::Custom("archive".to_string()),
                    priority: MemoryPriority::Low,
                    timestamp: 0, // 归档条目不保留精确时间戳
                    score: Some(score),
                });
            }

            if results.len() >= limit * 3 {
                break; // 足够多了，提前退出
            }
        }

        // 按匹配分数降序排列
        results.sort_by(|a, b| {
            b.score.unwrap_or(0.0).partial_cmp(&a.score.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        results
    }

    /// 将超龄记忆归档到 Cold 层（文件系统）
    ///
    /// 将超过 `days` 天的记忆导出为 markdown 文件，然后从 Warm 层删除
    pub async fn archive_to_cold(&self, agent_id: &str, days: u32) -> Result<u64, String> {
        let cold_dir = match &self.cold_dir {
            Some(d) => d,
            None => return Ok(0),
        };

        let archive_dir = cold_dir.join("archive");
        std::fs::create_dir_all(&archive_dir).map_err(|e| format!("创建归档目录失败: {}", e))?;

        let cutoff = chrono::Utc::now().timestamp_millis() - (days as i64 * 86_400_000);

        // 查询超龄记忆
        let rows = sqlx::query_as::<_, (String, String, String, i64)>(
            "SELECT id, memory_type, content, created_at FROM memories WHERE agent_id = ? AND created_at < ? AND COALESCE(priority, 1) < 3"
        )
        .bind(agent_id).bind(cutoff)
        .fetch_all(self.warm.pool())
        .await
        .map_err(|e| format!("查询超龄记忆失败: {}", e))?;

        if rows.is_empty() { return Ok(0); }

        // 按月归档
        let mut archives: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for (id, memory_type, content, created_at) in &rows {
            let date = chrono::DateTime::from_timestamp_millis(*created_at)
                .map(|d| d.format("%Y-%m").to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let entry = format!("## [{}] {}\n\n{}\n", memory_type, id, content);
            archives.entry(date).or_default().push(entry);
        }

        // 写入归档文件
        for (month, entries) in &archives {
            let path = archive_dir.join(format!("{}.md", month));
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path)
                .map_err(|e| format!("打开归档文件失败: {}", e))?;
            for entry in entries {
                f.write_all(entry.as_bytes()).map_err(|e| format!("写入归档失败: {}", e))?;
            }
        }

        // 从 Warm 层删除
        let ids: Vec<&str> = rows.iter().map(|(id, _, _, _)| id.as_str()).collect();
        for id in &ids {
            let _ = sqlx::query("DELETE FROM memories WHERE id = ?").bind(id).execute(self.warm.pool()).await;
            let _ = sqlx::query("DELETE FROM memories_fts WHERE memory_id = ?").bind(id).execute(self.warm.pool()).await;
        }

        let count = rows.len() as u64;
        log::info!("Cold 归档: agent={}, 归档 {} 条记忆到 {}", agent_id, count, archive_dir.display());
        Ok(count)
    }
}

#[async_trait]
impl Memory for TieredMemory {
    async fn store(&self, agent_id: &str, key: &str, content: &str, category: MemoryCategory) -> Result<String, String> {
        // 写入 Warm 层
        let id = self.warm.store(agent_id, key, content, category.clone()).await?;

        // 同步写入 Hot 层
        if let Ok(mut hot) = self.hot.lock() {
            hot.put(id.clone(), (MemoryEntry {
                id: id.clone(), key: key.to_string(), content: content.to_string(),
                category, priority: MemoryPriority::Normal,
                timestamp: chrono::Utc::now().timestamp_millis(), score: None,
            }, std::time::Instant::now()));
        }

        Ok(id)
    }

    async fn recall(&self, agent_id: &str, query: &str, limit: usize) -> Result<Vec<MemoryEntry>, String> {
        // Hot 层快速匹配（简单包含检查）
        let mut hot_results: Vec<MemoryEntry> = Vec::new();
        if let Ok(hot) = self.hot.lock() {
            for (_, (entry, _time)) in hot.iter() {
                if entry.content.contains(query) || query.split_whitespace().any(|w| entry.content.contains(w)) {
                    hot_results.push(entry.clone());
                }
            }
        }

        // Warm 层完整检索
        let warm_results = self.warm.recall(agent_id, query, limit).await?;

        // 合并去重（Hot 优先）
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut merged: Vec<MemoryEntry> = Vec::new();
        for entry in hot_results.into_iter().chain(warm_results.into_iter()) {
            if seen.insert(entry.id.clone()) {
                merged.push(entry);
            }
        }

        // Cold 层回退：如果 Hot+Warm 结果不足 3 条，从归档中补充
        if merged.len() < 3 {
            let cold_needed = limit.saturating_sub(merged.len());
            let cold_results = self.recall_from_archive(query, cold_needed);
            for entry in cold_results {
                if seen.insert(entry.id.clone()) {
                    merged.push(entry);
                }
            }
            if merged.len() > limit {
                // Hot+Warm 条目中可能有 0 条但 cold 返回了很多
                // 无需截断以 limit 为准（cold_needed 已限制）
            }
        }

        merged.truncate(limit);
        Ok(merged)
    }

    async fn get(&self, agent_id: &str, key: &str) -> Result<Option<MemoryEntry>, String> {
        // Hot 层查找
        if let Ok(mut hot) = self.hot.lock() {
            if let Some((entry, _)) = hot.get(key) {
                return Ok(Some(entry.clone()));
            }
        }
        // Warm 层查找
        self.warm.get(agent_id, key).await
    }

    async fn list(&self, agent_id: &str, category: MemoryCategory) -> Result<Vec<MemoryEntry>, String> {
        self.warm.list(agent_id, category).await
    }

    async fn forget(&self, agent_id: &str, key: &str) -> Result<(), String> {
        // 从 Hot 移除
        if let Ok(mut hot) = self.hot.lock() {
            hot.pop(key);
        }
        // 从 Warm 移除
        self.warm.forget(agent_id, key).await
    }

    async fn evict_by_priority(&self, agent_id: &str, max_entries: usize) -> Result<u64, String> {
        self.warm.evict_by_priority(agent_id, max_entries).await
    }
}

/// 查询扩展：提取关键词，用 OR 连接以扩大 FTS5 检索范围。
///
/// 例："Rust 编程语言的性能优势" → "Rust OR 编程语言 OR 性能 OR 优势"
fn expand_query(query: &str) -> String {
    // 中文停用词
    const STOPWORDS: &[&str] = &[
        "的", "了", "在", "是", "我", "有", "和", "就", "不", "人",
        "都", "一", "一个", "上", "也", "很", "到", "说", "要", "去",
        "你", "会", "着", "没有", "看", "好", "自己", "这", "他", "她",
        "the", "a", "an", "is", "are", "was", "were", "in", "on", "at",
        "to", "for", "of", "with", "and", "or", "but", "not", "this", "that",
        "it", "be", "as", "by", "from", "do", "does", "did", "will", "would",
        "can", "could", "should", "have", "has", "had", "what", "how", "why",
    ];

    let words: Vec<&str> = query.split_whitespace()
        .filter(|w| w.len() > 1 && !STOPWORDS.contains(w))
        .collect();

    if words.is_empty() {
        return query.to_string();
    }

    // FTS5 OR 查询语法
    words.join(" OR ")
}

/// 导出所有记忆为 snapshot 文件（用于备份/恢复）
pub async fn snapshot_memories(
    pool: &SqlitePool,
    agent_id: &str,
    output_path: &std::path::Path,
) -> Result<usize, String> {
    let rows = sqlx::query_as::<_, (String, String, String, i64, i32)>(
        "SELECT id, memory_type, content, created_at, COALESCE(priority, 1) FROM memories WHERE agent_id = ? ORDER BY created_at"
    )
    .bind(agent_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("查询记忆失败: {}", e))?;

    let mut content = format!("# Memory Snapshot\n\n- Agent: {}\n- Date: {}\n- Count: {}\n\n---\n\n",
        agent_id, chrono::Local::now().format("%Y-%m-%d %H:%M"), rows.len());

    for (_id, memory_type, mem_content, created_at, priority) in &rows {
        let date = chrono::DateTime::from_timestamp_millis(*created_at)
            .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let pri = MemoryPriority::from_i32(*priority);
        content.push_str(&format!("## [{}] {:?} ({})\n\n{}\n\n---\n\n", memory_type, pri, date, mem_content));
    }

    std::fs::write(output_path, &content).map_err(|e| format!("写入快照失败: {}", e))?;
    log::info!("Memory snapshot: agent={}, {} 条记忆 → {}", agent_id, rows.len(), output_path.display());
    Ok(rows.len())
}

/// 记忆体系统（兼容旧接口）
pub struct MemorySystem {
    memory: SqliteMemory,
}

impl MemorySystem {
    /// 创建新的记忆体系统
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            memory: SqliteMemory::new(pool),
        }
    }

    /// 获取 Memory trait 引用
    pub fn memory(&self) -> &dyn Memory {
        &self.memory
    }

    /// 保存对话
    pub async fn save_conversation(
        &self,
        agent_id: &str,
        session_id: &str,
        user_message: &str,
        agent_response: &str,
    ) -> Result<(), sqlx::Error> {
        conversation::save_conversation(self.memory.pool(), agent_id, session_id, user_message, agent_response).await
    }

    /// 检索对话历史
    pub async fn retrieve_conversation_history(
        &self,
        agent_id: &str,
        session_id: &str,
        limit: i64,
    ) -> Result<Vec<(String, String)>, sqlx::Error> {
        conversation::get_history(self.memory.pool(), agent_id, session_id, limit).await
    }

    /// 保存长期记忆
    pub async fn save_long_term_memory(
        &self,
        agent_id: &str,
        memory_type: &str,
        content: &str,
    ) -> Result<(), sqlx::Error> {
        long_term::save_memory(self.memory.pool(), agent_id, memory_type, content).await
    }

    /// 检索长期记忆
    pub async fn retrieve_long_term_memory(
        &self,
        agent_id: &str,
        memory_type: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        long_term::get_memory(self.memory.pool(), agent_id, memory_type).await
    }

    /// 获取连接池
    pub fn pool(&self) -> &SqlitePool {
        self.memory.pool()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::db::schema::init_schema(&pool).await.unwrap();

        // 创建测试 Agent
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO agents (id, name, system_prompt, model, temperature, max_tokens, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind("test-agent")
        .bind("Test")
        .bind("prompt")
        .bind("gpt-4")
        .bind(0.7)
        .bind(2048)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    #[tokio::test]
    async fn test_memory_trait_store_and_list() {
        let pool = setup_pool().await;
        let memory = SqliteMemory::new(pool);

        // 存储
        let id = memory
            .store("test-agent", "fact-1", "用户喜欢深色模式", MemoryCategory::Knowledge)
            .await
            .unwrap();
        assert!(!id.is_empty());

        // 列出
        let entries = memory
            .list("test-agent", MemoryCategory::Knowledge)
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].content.contains("深色模式"));
    }

    #[tokio::test]
    async fn test_memory_trait_get_and_forget() {
        let pool = setup_pool().await;
        let memory = SqliteMemory::new(pool);

        let id = memory
            .store("test-agent", "temp", "临时记忆", MemoryCategory::Daily)
            .await
            .unwrap();

        // 获取
        let entry = memory.get("test-agent", &id).await.unwrap();
        assert!(entry.is_some());
        assert!(entry.unwrap().content.contains("临时记忆"));

        // 删除
        memory.forget("test-agent", &id).await.unwrap();
        let gone = memory.get("test-agent", &id).await.unwrap();
        assert!(gone.is_none());
    }

    #[tokio::test]
    async fn test_memory_trait_recall_fts() {
        let pool = setup_pool().await;
        let memory = SqliteMemory::new(pool);

        memory
            .store("test-agent", "m1", "Rust 编程语言很快", MemoryCategory::Knowledge)
            .await
            .unwrap();
        memory
            .store("test-agent", "m2", "Python 适合数据分析", MemoryCategory::Knowledge)
            .await
            .unwrap();
        memory
            .store("test-agent", "m3", "Rust 的所有权系统很强大", MemoryCategory::Knowledge)
            .await
            .unwrap();

        // FTS5 检索
        let results = memory.recall("test-agent", "Rust", 5).await.unwrap();
        assert!(!results.is_empty());
        // 至少应该找到包含 "Rust" 的记忆
        assert!(results.iter().any(|r| r.content.contains("Rust")));
    }

    #[tokio::test]
    async fn test_memory_category_roundtrip() {
        assert_eq!(MemoryCategory::from_str("core"), MemoryCategory::Core);
        assert_eq!(MemoryCategory::from_str("daily"), MemoryCategory::Daily);
        assert_eq!(MemoryCategory::from_str("custom_type"), MemoryCategory::Custom("custom_type".to_string()));
        assert_eq!(MemoryCategory::Core.as_str(), "core");
    }
}
