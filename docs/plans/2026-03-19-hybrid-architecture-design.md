# XianZhu 混合架构设计方案

> 日期：2026-03-19
> 状态：设计阶段
> 目标：桌面端 + 移动端 + 云端的统一对话体验

---

## 1. 背景与目标

### 当前状态
- **桌面端（Tauri）**：全功能，本地 SQLite，工具调用 / MCP / 技能 / 本地文件
- **云端（admin-backend）**：Node.js API 服务（39.102.55.3:3000），PostgreSQL，nginx 反代
- **移动端**：无

### 目标
用户通过移动端也能对话、操作。根据桌面端在线状态自动切换：

| 场景 | 路径 | 能力 |
|------|------|------|
| 桌面在线 | 移动端 → Cloud → WebSocket → 桌面本地执行 | 全能力（工具、MCP、技能、文件） |
| 桌面离线 | 移动端 → Cloud → Cloud LLM Fallback | 基础对话 + 云端安全工具 |
| 桌面本地 | Tauri UI → 本地 orchestrator | 全能力（现有） |

---

## 2. 整体架构

```
                    ┌──────────────┐
                    │   移动端/Web  │
                    └──────┬───────┘
                           │ HTTPS + SSE
                    ┌──────▼───────┐
                    │  Cloud Gateway│  ← 永远在线（39.102.55.3）
                    │  (Node.js)   │
                    │              │
                    │  · 消息路由    │
                    │  · 消息队列    │
                    │  · 事件广播    │
                    │  · Postgres   │
                    └──┬───────┬───┘
           桌面在线？   │       │
              ┌────────▼┐   ┌──▼──────────┐
              │ WebSocket│   │ Cloud Fallback│
              │  Bridge  │   │ (基础 LLM)   │
              └────┬─────┘   └─────────────┘
              ┌────▼─────┐
              │  Desktop  │  ← Tauri 桌面端
              │ (全能力)   │
              └──────────┘
```

---

## 3. 核心模块设计

### 3.1 Cloud Gateway（升级 admin-backend）

```
admin-backend/src/
├── gateway/
│   ├── router.ts       // 消息路由：桌面在线→转发，离线→fallback
│   ├── presence.ts     // 桌面在线状态管理（心跳检测）
│   ├── ws-bridge.ts    // WebSocket 服务端，与桌面双向通信
│   ├── message-queue.ts// 每个 session 的消息队列（顺序处理）
│   ├── event-bus.ts    // SSE 事件广播（推送到所有订阅设备）
│   └── fallback.ts     // 离线时的 LLM fallback 调用
├── routes/
│   ├── chat.ts         // POST /api/v1/chat/send（移动端发消息）
│   ├── chat-subscribe.ts // GET /api/v1/chat/subscribe（SSE 事件流）
│   └── sync.ts         // 状态同步 API
```

**消息队列**（每个 session 维护一个有序队列）：
```typescript
// session_queues: Map<sessionId, Queue<PendingMessage>>

// 移动端发 msg1 → 转发桌面处理
// 还没处理完，移动端又发 msg2 → 入队等待
// msg1 完成 → 自动取 msg2 处理
// 移动端不阻塞，始终可以继续发送
```

**事件广播**（实时推送到所有订阅设备）：
```typescript
// 一条消息产生后，广播到该 session 的所有订阅者：
//   - Desktop WebSocket
//   - Mobile SSE (可能多个设备)
//
// 事件类型：
//   new_message  — 新消息（user/assistant/tool）
//   status       — 状态变更（thinking/tool_call/done）
//   stream_token — 流式 token
//   stream_done  — 流式完成
```

### 3.2 Desktop Bridge（Tauri 端新增）

```
local-app/src/bridge/
├── mod.rs          // 模块入口，启动/停止 bridge
├── client.rs       // WebSocket 客户端，连接 Cloud Gateway
├── presence.rs     // 心跳 + 能力注册
├── handler.rs      // 接收转发消息 → 调用本地 orchestrator → 流式返回
└── sync.rs         // 启动时增量拉取 Cloud 数据补入本地 SQLite
```

**连接生命周期**：
```
启动 Tauri → 读取云端配置（URL + API Key）
           → WebSocket 连接 Cloud Gateway
           → 发送 register 消息（能力注册）
           → 增量同步（拉取离线期间的消息）
           → 进入心跳循环（30s 间隔）

断线 → 指数退避重连（1s, 2s, 4s, 8s... 最大 60s）
关闭 Tauri → 发送 disconnect → Cloud 标记桌面离线
```

**能力注册协议**：
```json
{
  "type": "register",
  "deviceId": "macbook-uuid",
  "platform": "macos",
  "version": "0.1.0",
  "capabilities": [
    { "name": "bash_exec", "local": true },
    { "name": "file_read", "local": true },
    { "name": "file_write", "local": true },
    { "name": "mcp:github", "local": true },
    { "name": "web_search", "local": false }
  ],
  "agents": ["agent-uuid-1"],
  "skills": ["daily-report", "code-review"]
}
```

### 3.3 Mobile Client（新增）

轻量 PWA 或独立 Web App：

```
mobile-web/
├── 对话界面       // 精简版聊天 UI（SSE 流式显示）
├── Agent 选择     // 切换 Agent
├── 会话管理       // 创建/切换/删除会话
├── 状态指示       // 显示 "桌面在线" / "云端模式"
└── 基础设置       // 连接配置
```

---

## 4. 消息实时同步设计

### 4.1 核心原则

- **在线时实时双写，离线后上线补齐**
- 不需要 CRDT，不需要分布式锁
- 消息是 append-only 日志，天然无冲突

### 4.2 实时双写

**桌面在线 · 本地消息产生时**：
```
用户在桌面输入 → Desktop orchestrator 处理
  → 写入本地 SQLite
  → 同时通过 WS 推送到 Cloud → 写入 Postgres
  → Cloud 广播到 Mobile SSE（如果有手机在看）
```

**桌面在线 · 手机消息到达时**：
```
手机发消息 → Cloud 写入 Postgres
  → Cloud 通过 WS 转发到 Desktop
  → Desktop 处理 + 写入本地 SQLite
  → 结果通过 WS 回传 Cloud → 写入 Postgres
  → Cloud 通过 SSE 推送到手机（流式 token）
```

**桌面离线 · 手机消息**：
```
手机发消息 → Cloud 写入 Postgres
  → Cloud Fallback LLM 处理
  → 结果写入 Postgres
  → SSE 推送到手机
  → 桌面下次上线时拉取补齐
```

### 4.3 启动时增量同步

```
Desktop 上线 → 连接 Cloud WS
  → 发送: { type: "sync_pull", last_sync_at: 1710856800000 }
  → Cloud 返回: created_at > last_sync_at 的所有消息
  → Desktop 用 sync_id (UUID) 去重后写入 SQLite
  → 更新本地 last_sync_at 水位
```

### 4.4 数据库 Schema 变更

**Cloud Postgres — 新增字段**：
```sql
-- chat_messages 表
ALTER TABLE chat_messages ADD COLUMN device_id TEXT NOT NULL DEFAULT 'cloud';
ALTER TABLE chat_messages ADD COLUMN sync_id TEXT UNIQUE;

-- memories 表
ALTER TABLE memories ADD COLUMN device_id TEXT NOT NULL DEFAULT 'cloud';
ALTER TABLE memories ADD COLUMN sync_id TEXT UNIQUE;
ALTER TABLE memories ADD COLUMN version INTEGER NOT NULL DEFAULT 1;

-- 同步水位表（新建）
CREATE TABLE sync_log (
    device_id TEXT NOT NULL,
    table_name TEXT NOT NULL,
    last_sync_at BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (device_id, table_name)
);
```

**Desktop SQLite — 新增字段**：
```sql
ALTER TABLE chat_messages ADD COLUMN device_id TEXT NOT NULL DEFAULT 'desktop';
ALTER TABLE chat_messages ADD COLUMN sync_id TEXT UNIQUE;
ALTER TABLE memories ADD COLUMN device_id TEXT NOT NULL DEFAULT 'desktop';
ALTER TABLE memories ADD COLUMN sync_id TEXT UNIQUE;
ALTER TABLE memories ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
```

### 4.5 各类数据同步策略

| 数据 | 特性 | 同步方式 | 冲突处理 |
|------|------|----------|----------|
| **chat_messages** | 只追加、不修改 | 实时双写 + 启动补齐 | 无冲突（UUID 去重） |
| **conversations** | 只追加 | 同上 | 无冲突 |
| **chat_sessions** | 可改 title | updated_at 水位 | last-write-wins |
| **memories** | 可增可改 | 实时双写 + version 号 | version 大的覆盖小的 |
| **token_usage** | 只追加 | 启动时补齐 | 无冲突 |
| **agents 配置** | 低频修改 | 桌面上线时全量推送 | 桌面端为准 |
| **skills/MCP** | 仅桌面端 | 不同步 | 云端有独立安全子集 |

---

## 5. 消息队列与并发控制

### 5.1 消息不阻塞

移动端始终可以发送消息，不受桌面处理状态影响：

```
Cloud Gateway 内部：

session_queues: Map<sessionId, Queue<PendingMessage>>

流程：
1. 移动端 POST /chat/send → 消息入队
2. 如果队列空闲 → 立即处理（转发桌面或 fallback）
3. 如果正在处理上一条 → 排队等待
4. 前一条完成 → 自动取下一条处理
5. 移动端收到 "已接收" 确认，可继续输入
```

### 5.2 同一会话多设备操作

```
手机发了一条 + 电脑同时也发了一条（同一个 session）：

电脑的消息 → 直接进本地队列（本地 session_lock 控制）
手机的消息 → Cloud 转发到电脑队列

session_lock 保证同一时间只有一条在被 LLM 处理
两条按到达时间排序执行
结果都实时推送到两端
```

---

## 6. 实时事件协议

### 6.1 移动端订阅（SSE）

```
GET /api/v1/chat/subscribe?sessionId=xxx
Accept: text/event-stream
Authorization: Bearer xxx

event: message
data: {"type":"new_message","role":"user","content":"帮我查看 git status","deviceId":"mobile"}

event: status
data: {"type":"thinking","agentId":"agent-1"}

event: token
data: {"type":"stream_token","token":"项"}

event: token
data: {"type":"stream_token","token":"目"}

event: tool
data: {"type":"tool_call","tool":"bash_exec","args":{"command":"git status"}}

event: done
data: {"type":"stream_done","fullResponse":"项目状态正常...","tokenUsage":{"input":150,"output":80}}
```

### 6.2 桌面端通信（WebSocket）

```
// Cloud → Desktop: 转发消息
{
  "type": "forward_message",
  "requestId": "uuid",
  "agentId": "agent-1",
  "sessionId": "session-abc",
  "message": "帮我查看 git status",
  "sender": { "id": "mobile-user", "channel": "mobile", "deviceId": "iphone-1" }
}

// Desktop → Cloud: 流式 token
{ "type": "stream_token", "requestId": "uuid", "token": "项目" }

// Desktop → Cloud: 工具调用通知
{ "type": "tool_call", "requestId": "uuid", "tool": "bash_exec", "args": {"command": "git status"} }

// Desktop → Cloud: 完成
{
  "type": "stream_done",
  "requestId": "uuid",
  "fullResponse": "项目状态正常...",
  "messages": [
    { "role": "assistant", "content": "...", "sync_id": "uuid-1" },
    { "role": "tool", "content": "...", "tool_name": "bash_exec", "sync_id": "uuid-2" }
  ]
}

// Desktop → Cloud: 心跳
{ "type": "heartbeat", "timestamp": 1710856800000 }

// Desktop → Cloud: 本地消息同步
{
  "type": "sync_push",
  "messages": [ ... ],
  "memories": [ ... ]
}
```

---

## 7. 云端 Fallback 能力

桌面离线时，Cloud Gateway 自行处理对话：

### 7.1 能力对比

| 功能 | 桌面在线（转发） | 桌面离线（Fallback） |
|------|-----------------|---------------------|
| LLM 调用 | 本地 Provider | Cloud Provider（需配置） |
| bash_exec | ✓ 本地执行 | ✗ 不可用 |
| file_read/write | ✓ 本地文件 | ✗ 不可用 |
| MCP 工具 | ✓ 本地 MCP | ✗ 不可用 |
| memory_store | ✓ 写两端 | ✓ 写 Cloud |
| memory_query | ✓ 本地向量搜索 | ✓ Cloud FTS |
| web_search | ✓ | ✓ |
| 基础对话 | ✓ | ✓ |

### 7.2 System Prompt 动态调整

```
桌面在线时:
"你是 XianZhu 助手。你可以使用以下工具：
bash_exec, file_read, file_write, web_search, memory_store, memory_query,
mcp:github, ..."

桌面离线时:
"你是 XianZhu 助手（云端模式）。用户的桌面设备当前不在线，
你只能使用以下工具：web_search, memory_store, memory_query。
如果用户请求需要本地操作（文件、代码执行等），请告知用户需要打开桌面端。"
```

### 7.3 Cloud Fallback LLM 配置

```
Cloud Gateway 需要独立的 LLM Provider 配置：
- 从 settings 表读取，或环境变量
- 支持 OpenAI / DeepSeek / Qwen 等 API
- 与桌面端 Provider 配置独立（cloud 有自己的 API Key）
```

---

## 8. 安全设计

### 8.1 认证

```
移动端 → Cloud:  API Key (Bearer Token) 或 JWT
Desktop → Cloud: API Key + Device ID（首次需配对）
Cloud 内部:      服务间信任
```

### 8.2 桌面端配对流程

```
1. 桌面端设置页 → 输入 Cloud URL + API Key
2. 桌面端连接 Cloud → 发送 register（含 device_id）
3. Cloud 验证 API Key → 记录 device_id → 返回 OK
4. 后续连接用 device_id + API Key 双重验证
```

### 8.3 数据安全

- Cloud 到桌面的转发：通过 WSS（TLS 加密）
- 移动端到 Cloud：HTTPS
- API Key 不存储在移动端本地（每次需输入或用 Keychain/安全存储）
- 工具执行结果中的敏感信息（文件内容、环境变量）：脱敏后再同步

---

## 9. 实施计划

### Phase 1: Cloud Gateway 升级（admin-backend）
- [ ] Postgres 建表（chat_messages, memories, agents, sessions 等）
- [ ] 消息路由 + 队列模块
- [ ] WebSocket 服务端（/ws/bridge）
- [ ] SSE 事件广播（/api/v1/chat/subscribe）
- [ ] 对话 API（/api/v1/chat/send）
- [ ] 桌面在线状态管理（presence）

### Phase 2: Desktop Bridge
- [ ] bridge 模块（WS 客户端 + 心跳 + 能力注册）
- [ ] 消息转发处理（接收 → orchestrator → 流式返回）
- [ ] 启动时增量同步（拉取 Cloud 数据补入 SQLite）
- [ ] 实时双写（本地消息推送 Cloud）
- [ ] 设置页新增 Cloud 配置 UI

### Phase 3: Cloud Fallback
- [ ] Cloud LLM 调用模块（独立 Provider 配置）
- [ ] 上下文加载（从 Postgres 读历史消息）
- [ ] 安全工具子集（memory_store, memory_query, web_search）
- [ ] System Prompt 动态调整（根据桌面在线状态）

### Phase 4: Mobile Client
- [ ] PWA 项目搭建（React + Vite）
- [ ] 对话界面（SSE 流式显示）
- [ ] 会话管理（创建/切换/删除）
- [ ] Agent 选择器
- [ ] 桌面在线/离线状态指示
- [ ] 基础设置页

### Phase 5: 数据同步完善
- [ ] SQLite ↔ Postgres 增量同步协议
- [ ] 记忆体 version 冲突解决
- [ ] Agent 配置同步
- [ ] Token 使用统计合并
- [ ] 离线消息补齐测试

---

## 10. 服务器资源

| 现有资源 | 说明 |
|---------|------|
| 服务器 | 39.102.55.3（阿里云，Ubuntu 22.04） |
| SSH | `ssh -i ~/.ssh/google_compute_engine zys@39.102.55.3` |
| Nginx | 443 → admin-backend:3000，域名 zys-openclaw.com |
| PostgreSQL | 本地 5432 |
| Redis | 本地 6379 |
| 现有 admin-backend | ~/openclaw/admin-backend（Node.js，端口 3000） |
| 现有前端 | ~/openclaw/frontend-dist |

---

## 11. 技术选型

| 组件 | 技术 |
|------|------|
| Cloud Gateway | Node.js + TypeScript（升级现有 admin-backend） |
| WebSocket | ws 库（Node.js 端） + tokio-tungstenite（Rust 端） |
| SSE | 原生 HTTP Response streaming |
| 消息队列 | 内存队列（Map<sessionId, Queue>），后续可换 Redis |
| 数据库 | PostgreSQL（Cloud） + SQLite（Desktop） |
| Mobile | React PWA（复用桌面端组件风格） |
| 认证 | API Key（Bearer Token） |
