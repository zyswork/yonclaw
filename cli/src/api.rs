//! XianZhu API 客户端
//!
//! 连接运行中的 XianZhu 桌面端 HTTP API Gateway。

use serde_json::Value;

pub struct ApiClient {
    base_url: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl ApiClient {
    pub fn new(base_url: &str, api_key: Option<&str>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.map(String::from),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    /// GET 请求
    pub async fn get(&self, path: &str) -> Result<Value, String> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.get(&url);
        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
        let resp = req.send().await.map_err(|e| {
            if e.is_connect() {
                "无法连接到 XianZhu（请确保桌面端正在运行，且 Gateway 端口已开启）".to_string()
            } else {
                format!("请求失败: {}", e)
            }
        })?;
        let status = resp.status();
        let body: Value = resp.json().await.map_err(|e| format!("解析响应失败: {}", e))?;
        if !status.is_success() {
            return Err(body["error"].as_str().unwrap_or("未知错误").to_string());
        }
        Ok(body)
    }

    /// POST 请求
    pub async fn post(&self, path: &str, body: &Value) -> Result<Value, String> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.post(&url).json(body);
        if let Some(ref key) = self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
        let resp = req.send().await.map_err(|e| {
            if e.is_connect() {
                "无法连接到 XianZhu".to_string()
            } else {
                format!("请求失败: {}", e)
            }
        })?;
        let status = resp.status();
        let body: Value = resp.json().await.map_err(|e| format!("解析响应失败: {}", e))?;
        if !status.is_success() {
            return Err(body["error"].as_str().unwrap_or("未知错误").to_string());
        }
        Ok(body)
    }

    /// 健康检查
    pub async fn health(&self) -> Result<Value, String> {
        self.get("/api/v1/health").await
    }

    /// 发送消息（流式）
    pub async fn send_message(&self, agent_id: &str, session_id: &str, message: &str) -> Result<Value, String> {
        self.post("/api/v1/message", &serde_json::json!({
            "agentId": agent_id,
            "sessionId": session_id,
            "message": message,
        })).await
    }

    /// 获取 Agent 列表
    pub async fn list_agents(&self) -> Result<Value, String> {
        self.get("/api/v1/agents").await
    }

    /// Token 统计
    pub async fn token_stats(&self, agent_id: &str) -> Result<Value, String> {
        self.get(&format!("/api/v1/token-stats/{}", agent_id)).await
    }
}
