# Admin HTTP Server

## 能力
系统 SHALL 暴露本地 127.0.0.1 随机端口的只读状态 HTTP 端点。

## 需求

### Requirement: 仅本地绑定
AdminServer SHALL 只监听 `127.0.0.1`，随机端口（`:0`）。

### Requirement: 拒绝跨域
系统 SHALL 拒绝任何携带 `Origin` 请求头的连接，返回 403。

#### Scenario: 浏览器跨域攻击
- **WHEN** 恶意网页 fetch 本地端口
- **THEN** 在执行任何 DB 查询之前返回 `403 Forbidden`
- **AND** 响应体为 `{"error":"cross-origin denied"}`

### Requirement: 仅只读端点
AdminServer SHALL 只提供只读 GET 接口：`/status`（简略状态）、`/agents`（Agent 列表）。

### Requirement: 端口查询
系统 SHALL 通过 Tauri 命令 `get_admin_port` 返回当前端口（或 `None`）。

### Requirement: 优雅关闭
AdminServer SHALL 支持通过 `shutdown_signal()` 触发 `tokio::select!` 退出 accept 循环。
