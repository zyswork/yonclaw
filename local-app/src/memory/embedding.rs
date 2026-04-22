//! 向量嵌入模块
//!
//! 支持通过 API 生成文本嵌入向量，以及余弦相似度计算。
//! 内置 SHA256 → SQLite 嵌入缓存，避免重复调用 API。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 嵌入配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// API 端点
    pub api_url: String,
    /// API Key
    pub api_key: String,
    /// 模型名称
    pub model: String,
    /// 向量维度
    pub dimensions: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            api_url: "https://api.openai.com/v1/embeddings".to_string(),
            api_key: String::new(),
            model: "text-embedding-3-small".to_string(),
            dimensions: 1536,
        }
    }
}

/// 嵌入缓存最大条目数
const EMBEDDING_CACHE_MAX: i64 = 10_000;

/// 嵌入客户端（带 SQLite 缓存）
pub struct EmbeddingClient {
    config: EmbeddingConfig,
    /// 备选 provider 配置（主 API 失败时尝试）
    fallback_configs: Vec<EmbeddingConfig>,
    client: reqwest::Client,
    /// 可选的 SQLite 缓存池（无池时不缓存）
    cache_pool: Option<sqlx::SqlitePool>,
    /// 缓存统计
    cache_hits: std::sync::atomic::AtomicU64,
    cache_misses: std::sync::atomic::AtomicU64,
}

impl EmbeddingClient {
    pub fn new(config: EmbeddingConfig) -> Self {
        Self {
            config, fallback_configs: Vec::new(),
            // OpenClaw #66418: embedding 请求显式超时，避免悬挂（本地 Ollama 也受尊重）
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build().unwrap_or_default(),
            cache_pool: None,
            cache_hits: std::sync::atomic::AtomicU64::new(0),
            cache_misses: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// 创建带缓存的嵌入客户端
    pub fn with_cache(config: EmbeddingConfig, pool: sqlx::SqlitePool) -> Self {
        Self {
            config, fallback_configs: Vec::new(),
            // OpenClaw #66418: embedding 请求显式超时，避免悬挂（本地 Ollama 也受尊重）
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build().unwrap_or_default(),
            cache_pool: Some(pool),
            cache_hits: std::sync::atomic::AtomicU64::new(0),
            cache_misses: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// 添加备选 provider（主 API 失败时按顺序尝试）
    pub fn with_fallback(mut self, config: EmbeddingConfig) -> Self {
        self.fallback_configs.push(config);
        self
    }

    /// 获取配置引用
    pub fn config(&self) -> &EmbeddingConfig {
        &self.config
    }

    /// 内容哈希（缓存键）
    fn content_hash(text: &str, model: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(model.as_bytes());
        hasher.update(b"|");
        hasher.update(text.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// 查缓存
    async fn cache_get(&self, hash: &str) -> Option<Vec<f32>> {
        let pool = self.cache_pool.as_ref()?;
        let row = sqlx::query_as::<_, (Vec<u8>,)>(
            "SELECT embedding FROM embedding_cache WHERE content_hash = ?"
        )
        .bind(hash)
        .fetch_optional(pool)
        .await.ok().flatten()?;

        // 更新访问时间
        let now = chrono::Utc::now().timestamp_millis();
        let _ = sqlx::query("UPDATE embedding_cache SET accessed_at = ? WHERE content_hash = ?")
            .bind(now).bind(hash).execute(pool).await;

        Some(bytes_to_embedding(&row.0))
    }

    /// 写缓存 + LRU 淘汰
    async fn cache_put(&self, hash: &str, embedding: &[f32]) {
        let pool = match self.cache_pool.as_ref() {
            Some(p) => p,
            None => return,
        };
        let now = chrono::Utc::now().timestamp_millis();
        let emb_bytes = embedding_to_bytes(embedding);

        let _ = sqlx::query(
            "INSERT OR REPLACE INTO embedding_cache (content_hash, embedding, model, accessed_at) VALUES (?, ?, ?, ?)"
        )
        .bind(hash).bind(&emb_bytes).bind(&self.config.model).bind(now)
        .execute(pool).await;

        // LRU 淘汰
        let count: Result<(i64,), _> = sqlx::query_as("SELECT COUNT(*) FROM embedding_cache")
            .fetch_one(pool).await;
        if let Ok((c,)) = count {
            if c > EMBEDDING_CACHE_MAX {
                let to_del = c - EMBEDDING_CACHE_MAX;
                let _ = sqlx::query(
                    "DELETE FROM embedding_cache WHERE content_hash IN (SELECT content_hash FROM embedding_cache ORDER BY accessed_at ASC LIMIT ?)"
                ).bind(to_del).execute(pool).await;
            }
        }
    }

    /// 缓存命中率
    pub fn cache_hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(std::sync::atomic::Ordering::Relaxed);
        let misses = self.cache_misses.load(std::sync::atomic::Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 { 0.0 } else { hits as f64 / total as f64 }
    }

    /// 生成单个文本的嵌入向量（带缓存 + fallback）
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let hash = Self::content_hash(text, &self.config.model);

        // 查缓存
        if let Some(cached) = self.cache_get(&hash).await {
            self.cache_hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Ok(cached);
        }
        self.cache_misses.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // 主 provider
        match self.embed_api(text).await {
            Ok(result) => {
                self.cache_put(&hash, &result).await;
                return Ok(result);
            }
            Err(primary_err) => {
                if self.fallback_configs.is_empty() {
                    return Err(primary_err);
                }
                log::warn!("主嵌入 API 失败: {}，尝试备选 provider", primary_err);

                // 依次尝试 fallback providers
                for (i, fb_config) in self.fallback_configs.iter().enumerate() {
                    match self.embed_api_with_config(text, fb_config).await {
                        Ok(result) => {
                            log::info!("备选 provider {} 成功 ({})", i + 1, fb_config.model);
                            self.cache_put(&hash, &result).await;
                            return Ok(result);
                        }
                        Err(fb_err) => {
                            log::warn!("备选 provider {} 失败: {}", i + 1, fb_err);
                        }
                    }
                }
                // 所有 provider 都失败
                Err(format!("所有嵌入 provider 均失败。主: {}", primary_err))
            }
        }
    }

    /// 使用指定配置调用嵌入 API
    async fn embed_api_with_config(&self, text: &str, config: &EmbeddingConfig) -> Result<Vec<f32>, String> {
        let body = serde_json::json!({
            "input": text,
            "model": config.model,
            "dimensions": config.dimensions,
        });

        let response = self.client
            .post(&config.api_url)
            .header("Authorization", format!("Bearer {}", config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("嵌入 API 请求失败: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            let sanitized: String = text.chars().take(200).collect();
            return Err(format!("嵌入 API 返回错误 {}: {}", status, sanitized));
        }

        let data: serde_json::Value = response.json().await
            .map_err(|e| format!("解析嵌入响应失败: {}", e))?;

        let embedding = data["data"][0]["embedding"]
            .as_array()
            .ok_or("嵌入响应格式错误")?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect::<Vec<f32>>();

        if embedding.is_empty() {
            return Err("嵌入向量为空".to_string());
        }

        Ok(embedding)
    }

    /// 调用嵌入 API（使用主配置）
    async fn embed_api(&self, text: &str) -> Result<Vec<f32>, String> {
        self.embed_api_with_config(text, &self.config).await
    }

    /// 批量生成嵌入向量
    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() { return Ok(Vec::new()); }

        let body = serde_json::json!({
            "input": texts,
            "model": self.config.model,
        });

        let response = self.client
            .post(&self.config.api_url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("批量嵌入 API 请求失败: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            // 脱敏：截断并过滤敏感前缀
            let sanitized: String = text.chars().take(200).collect();
            let sanitized = sanitized
                .replace(|c: char| c.is_control(), "")
                .split_whitespace()
                .filter(|w| !w.starts_with("Bearer") && !w.starts_with("sk-") && !w.starts_with("key-"))
                .collect::<Vec<_>>()
                .join(" ");
            return Err(format!("嵌入 API 返回错误 {}: {}", status, sanitized));
        }

        let data: serde_json::Value = response.json().await
            .map_err(|e| format!("解析批量嵌入响应失败: {}", e))?;

        let embeddings = data["data"]
            .as_array()
            .ok_or("批量��入响应格式错误")?
            .iter()
            .filter_map(|item| {
                item["embedding"].as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                        .collect::<Vec<f32>>()
                })
            })
            .collect();

        Ok(embeddings)
    }
}

/// 余弦相似度计算
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() { return 0.0; }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 { 0.0 } else { dot / denom }
}

/// 将 f32 向量序列化为字节（用于 SQLite BLOB 存储）
pub fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

/// 从字节反序列化为 f32 向量
pub fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4)
        .map(|chunk| {
            let arr: [u8; 4] = chunk.try_into().unwrap_or([0; 4]);
            f32::from_le_bytes(arr)
        })
        .collect()
}

/// 在候选向量中搜索最相似的 top-k
pub fn top_k_similar(
    query: &[f32],
    candidates: &[(String, Vec<f32>)],
    k: usize,
) -> Vec<(String, f32)> {
    let mut scored: Vec<(String, f32)> = candidates.iter()
        .map(|(id, emb)| (id.clone(), cosine_similarity(query, emb)))
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    scored
}
