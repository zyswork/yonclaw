//! LanceDB 向量存储
//!
//! 替代 SQLite BLOB 存储向量，提供原生 ANN 索引搜索。
//! 数据存储在 ~/.xianzhu/lance/ 目录下。

use std::sync::Arc;
use arrow_array::{RecordBatch, RecordBatchIterator, StringArray, Float32Array, Int64Array, FixedSizeListArray, ArrayRef};
use arrow_schema::{Schema, Field, DataType};
use futures_util::TryStreamExt;

/// LanceDB 向量存储
pub struct LanceVectorStore {
    db: lancedb::Connection,
    table_name: String,
    dimensions: usize,
}

/// 搜索结果
#[derive(Debug, Clone)]
pub struct VectorSearchResult {
    pub id: String,
    pub content: String,
    pub score: f32,
}

impl LanceVectorStore {
    /// 创建或打开 LanceDB 存储
    pub async fn new(lance_dir: &str, dimensions: usize) -> Result<Self, String> {
        let db = lancedb::connect(lance_dir)
            .execute()
            .await
            .map_err(|e| format!("LanceDB 连接失败: {}", e))?;

        let store = Self {
            db,
            table_name: "memory_vectors".to_string(),
            dimensions,
        };

        store.ensure_table().await?;

        log::info!("LanceDB: 已连接 (path={}, dims={})", lance_dir, dimensions);
        Ok(store)
    }

    /// 获取 LanceDB 存储路径
    pub fn default_path() -> String {
        let home = dirs::home_dir().unwrap_or_default();
        home.join(".xianzhu").join("lance").to_string_lossy().to_string()
    }

    /// 确保表存在
    async fn ensure_table(&self) -> Result<(), String> {
        let tables = self.db.table_names()
            .execute()
            .await
            .map_err(|e| format!("列出表失败: {}", e))?;

        if !tables.contains(&self.table_name) {
            let schema = self.build_schema();
            let batch = self.empty_batch(&schema);
            let batches = RecordBatchIterator::new(
                vec![Ok(batch)],
                Arc::new(schema),
            );
            self.db.create_table(&self.table_name, Box::new(batches))
                .execute()
                .await
                .map_err(|e| format!("创建表失败: {}", e))?;
            log::info!("LanceDB: 创建表 {}", self.table_name);
        }

        Ok(())
    }

    fn build_schema(&self) -> Schema {
        Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("agent_id", DataType::Utf8, false),
            Field::new("content", DataType::Utf8, false),
            Field::new("memory_type", DataType::Utf8, false),
            Field::new(
                "vector",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    self.dimensions as i32,
                ),
                false,
            ),
            Field::new("created_at", DataType::Int64, false),
        ])
    }

    fn empty_batch(&self, schema: &Schema) -> RecordBatch {
        let id: ArrayRef = Arc::new(StringArray::from(Vec::<String>::new()));
        let agent_id: ArrayRef = Arc::new(StringArray::from(Vec::<String>::new()));
        let content: ArrayRef = Arc::new(StringArray::from(Vec::<String>::new()));
        let memory_type: ArrayRef = Arc::new(StringArray::from(Vec::<String>::new()));
        let values = Float32Array::from(Vec::<f32>::new());
        let vector: ArrayRef = Arc::new(FixedSizeListArray::new(
            Arc::new(Field::new("item", DataType::Float32, true)),
            self.dimensions as i32,
            Arc::new(values),
            None,
        ));
        let created_at: ArrayRef = Arc::new(Int64Array::from(Vec::<i64>::new()));

        RecordBatch::try_new(
            Arc::new(schema.clone()),
            vec![id, agent_id, content, memory_type, vector, created_at],
        ).unwrap()
    }

    /// 插入向量
    pub async fn insert(
        &self,
        id: &str,
        agent_id: &str,
        content: &str,
        embedding: &[f32],
        memory_type: &str,
    ) -> Result<(), String> {
        if embedding.len() != self.dimensions {
            return Err(format!(
                "向量维度不匹配: 期望 {}, 实际 {}",
                self.dimensions, embedding.len()
            ));
        }

        let schema = self.build_schema();

        let id_arr: ArrayRef = Arc::new(StringArray::from(vec![id.to_string()]));
        let agent_arr: ArrayRef = Arc::new(StringArray::from(vec![agent_id.to_string()]));
        let content_arr: ArrayRef = Arc::new(StringArray::from(vec![content.to_string()]));
        let type_arr: ArrayRef = Arc::new(StringArray::from(vec![memory_type.to_string()]));
        let values = Float32Array::from(embedding.to_vec());
        let vector_arr: ArrayRef = Arc::new(FixedSizeListArray::new(
            Arc::new(Field::new("item", DataType::Float32, true)),
            self.dimensions as i32,
            Arc::new(values),
            None,
        ));
        let ts_arr: ArrayRef = Arc::new(Int64Array::from(vec![chrono::Utc::now().timestamp_millis()]));

        let batch = RecordBatch::try_new(
            Arc::new(schema.clone()),
            vec![id_arr, agent_arr, content_arr, type_arr, vector_arr, ts_arr],
        ).map_err(|e| format!("构建 RecordBatch 失败: {}", e))?;

        let batches = RecordBatchIterator::new(vec![Ok(batch)], Arc::new(schema));

        let table = self.db.open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| format!("打开表失败: {}", e))?;

        table.add(Box::new(batches))
            .execute()
            .await
            .map_err(|e| format!("插入向量失败: {}", e))?;

        log::debug!("LanceDB: 已插入向量 id={}, agent={}", id, agent_id);
        Ok(())
    }

    /// ANN 向量搜索 + metadata 过滤
    pub async fn search(
        &self,
        agent_id: &str,
        query_embedding: &[f32],
        top_k: usize,
        memory_type: Option<&str>,
    ) -> Result<Vec<VectorSearchResult>, String> {
        let table = self.db.open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| format!("打开表失败: {}", e))?;

        let filter = match memory_type {
            Some(mt) => format!("agent_id = '{}' AND memory_type = '{}'", agent_id, mt),
            None => format!("agent_id = '{}'", agent_id),
        };

        use lancedb::query::{QueryBase, ExecutableQuery};
        let mut stream = table.vector_search(query_embedding)
            .map_err(|e| format!("向量搜索构建失败: {}", e))?
            .only_if(filter)
            .limit(top_k)
            .execute()
            .await
            .map_err(|e| format!("向量搜索执行失败: {}", e))?;

        let mut output = Vec::new();
        while let Some(batch) = stream.try_next().await.map_err(|e| format!("读取结果失败: {}", e))? {
            let ids = batch.column_by_name("id")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let contents = batch.column_by_name("content")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>());
            let distances = batch.column_by_name("_distance")
                .and_then(|c| c.as_any().downcast_ref::<Float32Array>());

            if let (Some(ids), Some(contents), Some(distances)) = (ids, contents, distances) {
                for i in 0..batch.num_rows() {
                    let distance = distances.value(i);
                    let score = 1.0 / (1.0 + distance);
                    output.push(VectorSearchResult {
                        id: ids.value(i).to_string(),
                        content: contents.value(i).to_string(),
                        score,
                    });
                }
            }
        }

        log::debug!("LanceDB: 搜索完成 agent={}, 结果={}条", agent_id, output.len());
        Ok(output)
    }

    /// 删除 Agent 的所有向量
    pub async fn delete_agent_vectors(&self, agent_id: &str) -> Result<(), String> {
        let table = self.db.open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| format!("打开表失败: {}", e))?;

        table.delete(&format!("agent_id = '{}'", agent_id))
            .await
            .map_err(|e| format!("删除向量失败: {}", e))?;

        log::info!("LanceDB: 已清空 Agent {} 的向量", agent_id);
        Ok(())
    }

    /// 获取向量统计
    pub async fn count_vectors(&self, agent_id: &str) -> Result<usize, String> {
        let table = self.db.open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| format!("打开表失败: {}", e))?;

        use lancedb::query::{QueryBase, ExecutableQuery};
        let mut stream = table.query()
            .only_if(format!("agent_id = '{}'", agent_id))
            .select(lancedb::query::Select::Columns(vec!["id".to_string()]))
            .execute()
            .await
            .map_err(|e| format!("查询失败: {}", e))?;

        let mut count = 0;
        while let Some(batch) = stream.try_next().await.map_err(|e| format!("读取失败: {}", e))? {
            count += batch.num_rows();
        }
        Ok(count)
    }
}

/// 从 SQLite vectors 表迁移到 LanceDB（一次性）
pub async fn migrate_from_sqlite(
    pool: &sqlx::SqlitePool,
    store: &LanceVectorStore,
    agent_id: &str,
) -> Result<usize, String> {
    let rows = sqlx::query_as::<_, (String, String, Vec<u8>)>(
        "SELECT id, content, embedding FROM vectors WHERE agent_id = ?"
    )
    .bind(agent_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("读取 SQLite 向量失败: {}", e))?;

    let mut migrated = 0;
    for (id, content, emb_bytes) in &rows {
        let embedding = super::embedding::bytes_to_embedding(emb_bytes);
        if embedding.len() == store.dimensions {
            store.insert(id, agent_id, content, &embedding, "migrated").await?;
            migrated += 1;
        }
    }

    log::info!("LanceDB: 从 SQLite 迁移 {} 条向量 (agent={})", migrated, agent_id);
    Ok(migrated)
}
