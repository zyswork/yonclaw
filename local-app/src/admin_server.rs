//! 本地只读状态 HTTP 服务
//!
//! 参照 OpenClaw Web Control UI 简化版：
//! - 监听 127.0.0.1 随机端口
//! - 提供 `/status` JSON 端点（只读）
//! - 用于外部脚本/CLI 查询当前 XianZhu 状态
//!
//! 安全：只监听 127.0.0.1，不绑定公网；无写操作。

use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct AdminServer {
    pub port: u16,
    db: sqlx::SqlitePool,
    /// 用于优雅关闭 accept 循环
    pub shutdown: Arc<tokio::sync::Notify>,
}

impl AdminServer {
    /// 启动服务，返回实际监听端口
    pub async fn start(db: sqlx::SqlitePool) -> Result<Arc<Self>, String> {
        // 绑定 127.0.0.1:0（随机端口）
        let listener = TcpListener::bind("127.0.0.1:0").await
            .map_err(|e| format!("Admin server 绑定失败: {}", e))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        log::info!("Admin server 监听: http://127.0.0.1:{}/status", port);

        let shutdown = Arc::new(tokio::sync::Notify::new());
        let server = Arc::new(Self { port, db: db.clone(), shutdown: shutdown.clone() });
        let server_clone = server.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.notified() => {
                        log::info!("Admin server 收到关闭信号，退出 accept 循环");
                        break;
                    }
                    accept = listener.accept() => {
                        match accept {
                            Ok((mut socket, _)) => {
                                let srv = server_clone.clone();
                                tokio::spawn(async move {
                                    let _ = srv.handle_conn(&mut socket).await;
                                });
                            }
                            Err(e) => {
                                log::warn!("Admin server accept 失败: {}", e);
                                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            }
                        }
                    }
                }
            }
        });
        Ok(server)
    }

    /// 通知 accept 循环退出
    pub fn shutdown_signal(&self) {
        self.shutdown.notify_one();
    }

    async fn handle_conn(&self, socket: &mut tokio::net::TcpStream) -> std::io::Result<()> {
        let mut buf = [0u8; 4096];
        let n = socket.read(&mut buf).await?;
        let req = String::from_utf8_lossy(&buf[..n]);
        let first_line = req.lines().next().unwrap_or("");
        let parts: Vec<&str> = first_line.split_whitespace().collect();
        let path = parts.get(1).copied().unwrap_or("/");

        // 先做 Origin 拦截（浏览器跨域探测），避免恶意网页触发任何 DB 查询
        if req.lines().any(|line| line.to_ascii_lowercase().starts_with("origin:")) {
            let deny = r#"{"error":"cross-origin denied"}"#;
            let response = format!(
                "HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                deny.len(), deny
            );
            socket.write_all(response.as_bytes()).await?;
            socket.shutdown().await?;
            return Ok(());
        }

        let (status, body) = match path {
            "/" | "/status" => ("200 OK", self.build_status().await),
            "/agents" => ("200 OK", self.build_agents().await),
            _ => ("404 Not Found", r#"{"error":"not found"}"#.to_string()),
        };

        let response = format!(
            "HTTP/1.1 {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status, body.len(), body
        );
        socket.write_all(response.as_bytes()).await?;
        socket.shutdown().await?;
        Ok(())
    }

    async fn build_status(&self) -> String {
        let agent_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agents")
            .fetch_one(&self.db).await.unwrap_or(0);
        let session_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_sessions")
            .fetch_one(&self.db).await.unwrap_or(0);
        let memory_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memories")
            .fetch_one(&self.db).await.unwrap_or(0);

        serde_json::json!({
            "app": "xianzhu",
            "status": "ok",
            "agents": agent_count,
            "sessions": session_count,
            "memories": memory_count,
            "pid": std::process::id(),
        }).to_string()
    }

    async fn build_agents(&self) -> String {
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT id, name, model FROM agents ORDER BY updated_at DESC LIMIT 30"
        )
        .fetch_all(&self.db).await.unwrap_or_default();

        let arr: Vec<serde_json::Value> = rows.into_iter().map(|(id, name, model)| {
            serde_json::json!({ "id": id, "name": name, "model": model })
        }).collect();
        serde_json::json!({ "agents": arr }).to_string()
    }
}
