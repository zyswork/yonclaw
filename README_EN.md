<p align="center">
  <img src="docs/assets/logo.png" alt="XianZhu Logo" width="120" />
</p>

<h1 align="center">XianZhuClaw 衔烛Claw</h1>

<p align="center">
  <strong>AI-native desktop assistant with multi-agent orchestration</strong>
</p>

<p align="center">
  <a href="#features">Features</a> &middot;
  <a href="#installation">Installation</a> &middot;
  <a href="#quick-start">Quick Start</a> &middot;
  <a href="#architecture">Architecture</a> &middot;
  <a href="#contributing">Contributing</a>
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

## What is XianZhu?

XianZhu (衔烛, "bearing the torch") is an open-source, cross-platform AI desktop assistant built with Rust + Tauri. It runs locally on your machine, supports multiple LLM providers, and features multi-agent orchestration, persistent memory, and a skill marketplace.

## Features

### Multi-Provider LLM Support
- **OpenAI** (GPT-4o, GPT-5, o3/o4)
- **Anthropic** (Claude Opus, Sonnet, Haiku)
- **Google Gemini** (2.5 Pro/Flash, 3.1 Pro via OAuth)
- **Moonshot** (Kimi K2)
- **MiniMax**, **ZhiPu GLM**, **DeepSeek**
- **Ollama** local models
- Custom OpenAI-compatible endpoints

### Multi-Agent System
- Create specialized agents with distinct personalities (SOUL.md)
- **Agent-to-Agent chat** (A2aTool) — full agentic loop invocation
- **Task delegation** with parallel execution and smart model routing
- **Collaborate tool** — inter-agent messaging with permission-based access
- **Yield/Resume** — pause parent agent while subagent works
- Relation management (Collaborator, Supervisor, Delegate)

### Memory & Learning
- **5-stage context compression** (SoulEngine + ContextGuard)
- **Tiered memory** — Hot (LRU) / Warm (SQLite) / Cold (archive files)
- **Learner system** — LLM-driven experience extraction from conversations
- **Memory eviction** — multi-dimensional scoring (priority 40% + freshness 30% + access 30%)
- **Knowledge distillation** to STANDING_ORDERS.md
- Hybrid search (FTS5 + vector embeddings)

### Harness Engineering
- **ExecutionBudget** — unified resource limits (LLM calls, tool calls, verify cycles)
- **IntentGate** — intent classification (Question/CodeChange/Research/Dangerous)
- **FileHarness** — hash-based file integrity verification + rollback
- **AutoVerify** — syntax checking for JSON/TOML/YAML + project verification
- **ProgressTracker** — throttled progress reporting with session recovery

### Channels
- Telegram
- Discord
- Feishu
- WeChat
- WeCom

### Skills Marketplace
- **60 bundled skills** with bilingual trigger keywords
- Cloud marketplace (search, download, publish)
- Sandboxed execution with per-skill permissions
- Auto-activation by intent keyword matching

### Desktop Features
- Cross-platform (macOS, Windows, Linux)
- Local-first — data stays on your machine (SQLite)
- OAuth provider authentication (Google Gemini, OpenAI)
- Voice input (Whisper)
- Dark/Light theme with glassmorphism UI
- Profile management (nickname, avatar, bio)

## Installation

### Pre-built Binaries

Download the latest release for your platform:

| Platform | Download |
|----------|----------|
| macOS (Apple Silicon) | `XianZhu_x.x.x_aarch64.dmg` |
| macOS (Intel) | `XianZhu_x.x.x_x64.dmg` |
| Windows | `XianZhu_x.x.x_x64_en-US.msi` |
| Linux (AppImage) | `xian-zhu_x.x.x_amd64.AppImage` |
| Linux (Debian) | `xian-zhu_x.x.x_amd64.deb` |

### Build from Source

**Prerequisites:**
- Rust 1.75+
- Node.js 22+
- Platform-specific dependencies (see below)

```bash
# Clone
git clone https://github.com/zyswork/xianzhu-claw.git
cd xianzhu

# Install frontend dependencies
cd frontend && npm install && cd ..

# Build
cd local-app && cargo tauri build
```

<details>
<summary>Linux dependencies</summary>

```bash
sudo apt-get install -y \
  libgtk-3-dev libwebkit2gtk-4.0-dev libwebkit2gtk-4.1-dev \
  libappindicator3-dev librsvg2-dev patchelf \
  libjavascriptcoregtk-4.0-dev libjavascriptcoregtk-4.1-dev \
  libsoup-3.0-dev
```
</details>

## Quick Start

1. **Launch XianZhu** and create an account (or login)
2. **Add a provider** in Settings — paste your API key or use OAuth for Google Gemini (free)
3. **Create an agent** — give it a name, choose a model, customize its personality
4. **Start chatting** — the agent can use tools, search the web, manage files, and more
5. **Install skills** from the marketplace to extend capabilities

## Architecture

```
xianzhu/
├── local-app/           # Tauri desktop app (Rust backend)
│   ├── src/
│   │   ├── agent/       # Core agent system
│   │   │   ├── orchestrator.rs   # Main orchestration loop
│   │   │   ├── agent_loop.rs     # Tool execution cycle
│   │   │   ├── llm.rs            # Multi-provider LLM client
│   │   │   ├── tools/            # 30+ built-in tools
│   │   │   ├── skills.rs         # Skill system
│   │   │   ├── delegate.rs       # Sub-agent task delegation
│   │   │   ├── learner.rs        # Experience extraction
│   │   │   └── memory_eviction.rs
│   │   ├── memory/      # Tiered memory system
│   │   ├── channels/    # Telegram/Discord/Feishu/WeChat
│   │   ├── handlers/    # Tauri command handlers
│   │   └── bridge/      # Cloud sync bridge
│   └── bundled-skills/  # 60 bundled skills
├── frontend/            # React + Vite + TypeScript
├── admin-backend/       # Enterprise backend (Node.js)
└── docs/                # Documentation
```

### Tech Stack

| Layer | Technology |
|-------|-----------|
| Desktop framework | Tauri 1.x |
| Backend | Rust (tokio, sqlx, reqwest) |
| Frontend | React 18 + TypeScript + Vite |
| Database | SQLite (local) |
| Vector search | FTS5 + embedding similarity |
| LLM protocols | OpenAI, Anthropic, Google Gemini (native + Cloud Code) |

## Configuration

### Data Locations (macOS)

| Item | Path |
|------|------|
| Database | `~/Library/Application Support/com.xianzhu.app/xianzhu.db` |
| Logs | `~/Library/Logs/XianZhu/xianzhu.log` |
| Agent workspaces | `~/.xianzhu/agents/{uuid}/` |
| Profile | `~/.xianzhu/profile/` |
| Skills marketplace | `~/.xianzhu/marketplace/` |
| OAuth credentials | `~/.xianzhu/oauth_credentials.json` |

### Environment Variables

| Variable | Description |
|----------|-------------|
| `XIANZHU_BACKEND_PORT` | Override backend port (default: 3000-3010 auto) |
| `HTTPS_PROXY` / `ALL_PROXY` | HTTP proxy for LLM API calls |

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes
4. Push to the branch
5. Open a Pull Request

## License

MIT License. See [LICENSE](LICENSE) for details.

---

<p align="center">
  Built with Rust, React, and Tauri<br/>
  <sub>XianZhuClaw 衔烛Claw — bearing the torch of AI assistance</sub>
</p>
