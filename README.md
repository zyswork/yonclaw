<p align="center">
  <img src="docs/assets/logo.png" alt="XianZhu Logo" width="120" />
</p>

<h1 align="center">XianZhuClaw 衔烛Claw</h1>

<p align="center">
  <strong>AI-native desktop assistant with multi-agent orchestration</strong><br/>
  <strong>AI 原生桌面助手，支持多智能体协作</strong>
</p>

<p align="center">
  <a href="#features--功能特性">Features</a> &middot;
  <a href="#installation--安装">Installation</a> &middot;
  <a href="#quick-start--快速开始">Quick Start</a> &middot;
  <a href="#architecture--架构">Architecture</a> &middot;
  <a href="#contributing--贡献">Contributing</a>
</p>

<p align="center">
  <a href="./README_EN.md">English</a> | <a href="./README.md">中文</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-0.2.0-blue" alt="Version" />
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-brightgreen" alt="Platform" />
  <img src="https://img.shields.io/badge/license-MIT-green" alt="License" />
  <img src="https://github.com/zyswork/xianzhu-claw/actions/workflows/build.yml/badge.svg" alt="Build" />
</p>

---

## What is XianZhu? | 什么是衔烛？

XianZhu (衔烛, "bearing the torch") is an open-source, cross-platform AI desktop assistant built with Rust + Tauri. It runs locally on your machine, supports multiple LLM providers, and features multi-agent orchestration, persistent memory, and a skill marketplace.

衔烛是一款开源、跨平台的 AI 桌面助手，基于 Rust + Tauri 构建。所有数据存储在本地，支持多家大模型供应商，具备多智能体协作、持久化记忆和技能市场。

## Features | 功能特性

### Multi-Provider LLM Support | 多供应商大模型
- **OpenAI** (GPT-4o, GPT-5, o3/o4)
- **Anthropic** (Claude Opus, Sonnet, Haiku)
- **Google Gemini** (2.5 Pro/Flash, 3.1 Pro — OAuth 免费使用)
- **Moonshot** (Kimi K2)
- **MiniMax**、**智谱 GLM**、**DeepSeek**
- **Ollama** 本地模型
- 自定义 OpenAI 兼容端点

### Multi-Agent System | 多智能体系统
- 创建拥有独立人格的专业 Agent（SOUL.md 定义）
- **Agent 间对话**（A2aTool）— 完整的智能体循环调用
- **任务委派** — 并行执行 + 智能模型路由
- **协作工具** — Agent 间消息通信，基于关系权限控制
- **暂停/恢复** — 父 Agent 等待子 Agent 完成后继续
- 关系管理（协作者、主管、委托者）

### Memory & Learning | 记忆与学习
- **5 阶段上下文压缩**（SoulEngine + ContextGuard）
- **分层记忆** — 热层（LRU）/ 温层（SQLite）/ 冷层（归档文件）
- **经验学习** — LLM 驱动的对话经验自动提取
- **记忆淘汰** — 多维度评分（优先级 40% + 新鲜度 30% + 访问频率 30%）
- **知识蒸馏** — 高频经验沉淀为 STANDING_ORDERS.md
- 混合检索（FTS5 全文 + 向量嵌入）

### Harness Engineering | 工程约束
- **ExecutionBudget** — 统一资源限制（LLM 调用数、工具调用数、验证轮次）
- **IntentGate** — 意图分类（提问 / 代码修改 / 研究 / 危险操作）
- **FileHarness** — 基于哈希的文件完整性校验 + 回滚
- **AutoVerify** — JSON/TOML/YAML 语法检查 + 项目级验证
- **ProgressTracker** — 节流进度报告，支持会话恢复

### Channels | 多渠道接入
- Telegram
- Discord
- 飞书（Feishu）
- 微信（WeChat）
- 企业微信（WeCom）

### Skills Marketplace | 技能市场
- **60 个内置技能**，支持中英文触发关键词
- 云端技能市场（搜索、下载、发布）
- 沙箱化执行，每个技能独立权限控制
- 基于意图关键词自动激活

### Desktop Features | 桌面特性
- 跨平台（macOS、Windows、Linux）
- 本地优先 — 数据存储在你的设备上（SQLite）
- OAuth 供应商认证（Google Gemini、OpenAI）
- 语音输入（Whisper）
- 深色/浅色主题，毛玻璃风格 UI
- 个人资料管理（昵称、头像、简介）

## Installation | 安装

### Pre-built Binaries | 预编译安装包

下载对应平台的最新版本：

| Platform 平台 | Download 下载 |
|----------|----------|
| macOS (Apple Silicon) | `XianZhu_x.x.x_aarch64.dmg` |
| macOS (Intel) | `XianZhu_x.x.x_x64.dmg` |
| Windows | `XianZhu_x.x.x_x64_en-US.msi` |
| Linux (AppImage) | `xian-zhu_x.x.x_amd64.AppImage` |
| Linux (Debian) | `xian-zhu_x.x.x_amd64.deb` |

### Build from Source | 从源码构建

**Prerequisites | 前置条件：**
- Rust 1.75+
- Node.js 22+
- 平台相关依赖（见下方）

```bash
# 克隆仓库
git clone https://github.com/zyswork/xianzhu-claw.git
cd xianzhu

# 安装前端依赖
cd frontend && npm install && cd ..

# 构建
cd local-app && cargo tauri build
```

<details>
<summary>Linux 依赖</summary>

```bash
sudo apt-get install -y \
  libgtk-3-dev libwebkit2gtk-4.0-dev libwebkit2gtk-4.1-dev \
  libappindicator3-dev librsvg2-dev patchelf \
  libjavascriptcoregtk-4.0-dev libjavascriptcoregtk-4.1-dev \
  libsoup-3.0-dev
```
</details>

## Quick Start | 快速开始

1. **启动衔烛** — 创建账号或登录
2. **添加供应商** — 在设置中粘贴 API Key，或使用 OAuth 授权 Google Gemini（免费）
3. **创建 Agent** — 起名字、选模型、自定义人格
4. **开始对话** — Agent 可以使用工具、搜索网页、管理文件等
5. **安装技能** — 从技能市场安装扩展能力

## Architecture | 架构

```
xianzhu/
├── local-app/           # Tauri 桌面应用（Rust 后端）
│   ├── src/
│   │   ├── agent/       # 核心智能体系统
│   │   │   ├── orchestrator.rs   # 主编排循环
│   │   │   ├── agent_loop.rs     # 工具执行循环
│   │   │   ├── llm.rs            # 多供应商 LLM 客户端
│   │   │   ├── tools/            # 30+ 内置工具
│   │   │   ├── skills.rs         # 技能系统
│   │   │   ├── delegate.rs       # 子 Agent 任务委派
│   │   │   ├── learner.rs        # 经验提取
│   │   │   └── memory_eviction.rs
│   │   ├── memory/      # 分层记忆系统
│   │   ├── channels/    # Telegram/Discord/飞书/微信
│   │   ├── handlers/    # Tauri 命令处理器
│   │   └── bridge/      # 云端同步桥接
│   └── bundled-skills/  # 60 个内置技能
├── frontend/            # React + Vite + TypeScript
├── admin-backend/       # 企业后台（Node.js）
└── docs/                # 文档
```

### Tech Stack | 技术栈

| Layer 层级 | Technology 技术 |
|-------|-----------|
| 桌面框架 | Tauri 1.x |
| 后端 | Rust (tokio, sqlx, reqwest) |
| 前端 | React 18 + TypeScript + Vite |
| 数据库 | SQLite（本地） |
| 向量检索 | FTS5 + 嵌入相似度 |
| LLM 协议 | OpenAI, Anthropic, Google Gemini (原生 + Cloud Code) |

## Configuration | 配置

### Data Locations | 数据路径

| Item 项目 | macOS Path 路径 |
|------|------|
| 数据库 | `~/Library/Application Support/com.xianzhu.app/xianzhu.db` |
| 日志 | `~/Library/Logs/XianZhu/xianzhu.log` |
| Agent 工作区 | `~/.xianzhu/agents/{uuid}/` |
| 个人资料 | `~/.xianzhu/profile/` |
| 技能市场 | `~/.xianzhu/marketplace/` |
| OAuth 凭据 | `~/.xianzhu/oauth_credentials.json` |

### Environment Variables | 环境变量

| Variable 变量 | Description 说明 |
|----------|-------------|
| `XIANZHU_BACKEND_PORT` | 覆盖后端端口（默认 3000-3010 自动选择） |
| `HTTPS_PROXY` / `ALL_PROXY` | LLM API 请求代理 |

## Contributing | 贡献

欢迎贡献！请遵循以下流程：

1. Fork 本仓库
2. 创建功能分支（`git checkout -b feature/amazing-feature`）
3. 提交修改
4. 推送分支
5. 发起 Pull Request

## License | 许可证

MIT License. See [LICENSE](LICENSE) for details.

MIT 许可证，详见 [LICENSE](LICENSE)。

---

<p align="center">
  Built with Rust, React, and Tauri<br/>
  基于 Rust、React 和 Tauri 构建<br/>
  <sub>XianZhuClaw 衔烛Claw — 衔火而行，烛照前路</sub>
</p>
