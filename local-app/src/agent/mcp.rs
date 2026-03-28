//! MCP Client 核心模块
//!
//! 实现 JSON-RPC 2.0 客户端，支持 stdio (子进程) 和 HTTP 传输
//! 用于连接 MCP Server 并获取/调用其工具

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::time::{timeout, Duration};

/// JSON-RPC 2.0 请求
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    method: String,
    params: serde_json::Value,
}

/// JSON-RPC 2.0 响应
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: Option<u64>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 错误
#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[allow(dead_code)]
    data: Option<serde_json::Value>,
}

/// MCP 工具定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// MCP 连接状态
#[derive(Debug, Clone, PartialEq)]
pub enum McpStatus {
    Configured,
    Connected,
    Failed(String),
}

/// IO 超时时间（秒）
const IO_TIMEOUT_SECS: u64 = 30;

/// MCP 传输层
enum McpTransport {
    Stdio {
        child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
    },
    Http {
        client: reqwest::Client,
        url: String,
    },
}

/// MCP Client
///
/// 管理与单个 MCP Server 的连接和通信
pub struct McpClient {
    name: String,
    transport: McpTransport,
    status: McpStatus,
    tools: Vec<McpToolDef>,
    next_id: u64,
}

impl McpClient {
    /// 通过 stdio 连接 MCP Server（启动子进程）
    pub async fn new_stdio(
        name: &str,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self, String> {
        log::info!("启动 MCP Server (stdio): {} — {} {:?}", name, command, args);

        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .envs(env);

        let mut child = cmd.spawn().map_err(|e| {
            format!("启动 MCP Server '{}' 失败: {} (命令: {} {:?})", name, e, command, args)
        })?;

        let stdin = child.stdin.take()
            .ok_or_else(|| format!("MCP Server '{}' stdin 不可用", name))?;
        let stdout = child.stdout.take()
            .ok_or_else(|| format!("MCP Server '{}' stdout 不可用", name))?;
        let stdout = BufReader::new(stdout);

        let mut client = Self {
            name: name.to_string(),
            transport: McpTransport::Stdio { child, stdin, stdout },
            status: McpStatus::Configured,
            tools: Vec::new(),
            next_id: 1,
        };

        // 执行 MCP 握手
        client.initialize().await?;
        client.fetch_tools().await?;
        client.status = McpStatus::Connected;

        log::info!("MCP Server '{}' 已连接，发现 {} 个工具", name, client.tools.len());
        Ok(client)
    }

    /// 通过 HTTP 连接 MCP Server
    pub async fn new_http(name: &str, url: &str) -> Result<Self, String> {
        log::info!("连接 MCP Server (HTTP): {} — {}", name, url);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(IO_TIMEOUT_SECS))
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

        let mut mcp_client = Self {
            name: name.to_string(),
            transport: McpTransport::Http { client, url: url.to_string() },
            status: McpStatus::Configured,
            tools: Vec::new(),
            next_id: 1,
        };

        mcp_client.initialize().await?;
        mcp_client.fetch_tools().await?;
        mcp_client.status = McpStatus::Connected;

        log::info!("MCP Server '{}' (HTTP) 已连接，发现 {} 个工具", name, mcp_client.tools.len());
        Ok(mcp_client)
    }

    /// 发送 JSON-RPC 请求并接收响应
    async fn send_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let id = self.next_id;
        self.next_id += 1;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: method.to_string(),
            params,
        };

        let request_json = serde_json::to_string(&request)
            .map_err(|e| format!("序列化请求失败: {}", e))?;

        log::debug!("MCP '{}' 发送: {}", self.name, request_json);

        match &mut self.transport {
            McpTransport::Stdio { stdin, stdout, .. } => {
                // 写请求到 stdin
                let write_data = format!("{}\n", request_json);
                timeout(
                    Duration::from_secs(IO_TIMEOUT_SECS),
                    stdin.write_all(write_data.as_bytes()),
                )
                .await
                .map_err(|_| format!("MCP '{}' 写入超时", self.name))?
                .map_err(|e| format!("MCP '{}' 写入失败: {}", self.name, e))?;

                timeout(
                    Duration::from_secs(IO_TIMEOUT_SECS),
                    stdin.flush(),
                )
                .await
                .map_err(|_| format!("MCP '{}' flush 超时", self.name))?
                .map_err(|e| format!("MCP '{}' flush 失败: {}", self.name, e))?;

                // 从 stdout 读取响应（跳过非 JSON 行和通知）
                loop {
                    let mut line = String::new();
                    let bytes_read = timeout(
                        Duration::from_secs(IO_TIMEOUT_SECS),
                        stdout.read_line(&mut line),
                    )
                    .await
                    .map_err(|_| format!("MCP '{}' 读取超时", self.name))?
                    .map_err(|e| format!("MCP '{}' 读取失败: {}", self.name, e))?;

                    if bytes_read == 0 {
                        return Err(format!("MCP '{}' 连接已关闭", self.name));
                    }

                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }

                    log::debug!("MCP '{}' 收到: {}", self.name, line);

                    // 尝试解析为 JSON-RPC 响应
                    if let Ok(response) = serde_json::from_str::<JsonRpcResponse>(line) {
                        // 跳过通知（无 id）
                        if response.id.is_none() {
                            continue;
                        }
                        if let Some(error) = response.error {
                            return Err(format!(
                                "MCP '{}' 错误 ({}): {}",
                                self.name, error.code, error.message
                            ));
                        }
                        return Ok(response.result.unwrap_or(serde_json::Value::Null));
                    }
                    // 非 JSON 行，跳过（可能是 stderr 输出混入）
                }
            }
            McpTransport::Http { client, url } => {
                let response = client
                    .post(url.as_str())
                    .json(&request)
                    .send()
                    .await
                    .map_err(|e| format!("MCP '{}' HTTP 请求失败: {}", self.name, e))?;

                let body = response
                    .text()
                    .await
                    .map_err(|e| format!("MCP '{}' 读取响应体失败: {}", self.name, e))?;

                log::debug!("MCP '{}' HTTP 收到: {}", self.name, body);

                let rpc_response: JsonRpcResponse = serde_json::from_str(&body)
                    .map_err(|e| format!("MCP '{}' 解析响应失败: {}", self.name, e))?;

                if let Some(error) = rpc_response.error {
                    return Err(format!(
                        "MCP '{}' 错误 ({}): {}",
                        self.name, error.code, error.message
                    ));
                }
                Ok(rpc_response.result.unwrap_or(serde_json::Value::Null))
            }
        }
    }

    /// 发送通知（无 id，不期望响应）
    async fn send_notification(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), String> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: method.to_string(),
            params,
        };

        let request_json = serde_json::to_string(&request)
            .map_err(|e| format!("序列化通知失败: {}", e))?;

        match &mut self.transport {
            McpTransport::Stdio { stdin, .. } => {
                let write_data = format!("{}\n", request_json);
                timeout(
                    Duration::from_secs(IO_TIMEOUT_SECS),
                    stdin.write_all(write_data.as_bytes()),
                )
                .await
                .map_err(|_| format!("MCP '{}' 写入超时", self.name))?
                .map_err(|e| format!("MCP '{}' 写入失败: {}", self.name, e))?;

                timeout(
                    Duration::from_secs(IO_TIMEOUT_SECS),
                    stdin.flush(),
                )
                .await
                .map_err(|_| format!("MCP '{}' flush 超时", self.name))?
                .map_err(|e| format!("MCP '{}' flush 失败: {}", self.name, e))?;
            }
            McpTransport::Http { client, url } => {
                let _ = client
                    .post(url.as_str())
                    .json(&request)
                    .send()
                    .await;
            }
        }
        Ok(())
    }

    /// MCP 初始化握手
    async fn initialize(&mut self) -> Result<(), String> {
        let params = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "xianzhu",
                "version": "0.1.0"
            }
        });

        let result = self.send_request("initialize", params).await?;
        log::info!(
            "MCP '{}' 初始化成功: protocol={}",
            self.name,
            result.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or("unknown")
        );

        // 发送 initialized 通知
        self.send_notification("notifications/initialized", serde_json::json!({})).await?;

        Ok(())
    }

    /// 获取 MCP Server 的工具列表
    async fn fetch_tools(&mut self) -> Result<(), String> {
        let result = self.send_request("tools/list", serde_json::json!({})).await?;

        let tools_array = result
            .get("tools")
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("MCP '{}' tools/list 响应格式错误", self.name))?;

        self.tools = tools_array
            .iter()
            .filter_map(|t| {
                let name = t.get("name")?.as_str()?.to_string();
                let description = t
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let input_schema = t
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or(serde_json::json!({"type": "object"}));
                Some(McpToolDef {
                    name,
                    description,
                    input_schema,
                })
            })
            .collect();

        Ok(())
    }

    /// 调用 MCP 工具
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<String, String> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments,
        });

        let result = self.send_request("tools/call", params).await?;

        // MCP 返回 content 数组，提取第一个 text 类型的内容
        if let Some(content) = result.get("content").and_then(|v| v.as_array()) {
            let texts: Vec<&str> = content
                .iter()
                .filter_map(|c| {
                    if c.get("type").and_then(|t| t.as_str()) == Some("text") {
                        c.get("text").and_then(|t| t.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            if !texts.is_empty() {
                return Ok(texts.join("\n"));
            }
        }

        // 降级：直接返回 result 的 JSON 字符串
        Ok(serde_json::to_string_pretty(&result).unwrap_or_default())
    }

    /// 关闭连接
    pub async fn shutdown(&mut self) {
        match &mut self.transport {
            McpTransport::Stdio { child, .. } => {
                let _ = child.kill().await;
                log::info!("MCP Server '{}' 子进程已终止", self.name);
            }
            McpTransport::Http { .. } => {
                log::info!("MCP Server '{}' (HTTP) 连接已关闭", self.name);
            }
        }
        self.status = McpStatus::Failed("已关闭".to_string());
    }

    /// 获取连接状态
    pub fn status(&self) -> &McpStatus {
        &self.status
    }

    /// 获取工具列表
    pub fn tools(&self) -> &[McpToolDef] {
        &self.tools
    }

    /// 获取 Server 名称
    pub fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_status_eq() {
        assert_eq!(McpStatus::Connected, McpStatus::Connected);
        assert_eq!(McpStatus::Configured, McpStatus::Configured);
        assert_ne!(McpStatus::Connected, McpStatus::Configured);
        assert_ne!(
            McpStatus::Failed("a".to_string()),
            McpStatus::Failed("b".to_string())
        );
    }

    #[test]
    fn test_mcp_tool_def_clone() {
        let tool = McpToolDef {
            name: "test".to_string(),
            description: "test tool".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        };
        let cloned = tool.clone();
        assert_eq!(cloned.name, "test");
        assert_eq!(cloned.description, "test tool");
    }

    #[test]
    fn test_json_rpc_request_serialize() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(1),
            method: "initialize".to_string(),
            params: serde_json::json!({}),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"method\":\"initialize\""));
    }

    #[test]
    fn test_json_rpc_notification_no_id() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notifications/initialized".to_string(),
            params: serde_json::json!({}),
        };
        let json = serde_json::to_string(&req).unwrap();
        // id 为 None 时应该不出现在 JSON 中
        assert!(!json.contains("\"id\""));
    }

    #[test]
    fn test_json_rpc_response_parse() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.id, Some(1));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_json_rpc_error_parse() {
        let json = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Method not found");
    }
}
