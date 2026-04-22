//! XianZhu CLI
//!
//! 命令行界面，连接运行中的 XianZhu (衔烛) 桌面端 HTTP API Gateway。
//! 参考 OpenClaw CLI 设计。

use clap::{Parser, Subcommand};
use colored::Colorize;

mod api;
mod commands;

/// XianZhu (衔烛) - AI Agent Desktop Assistant
#[derive(Parser)]
#[command(name = "xianzhu", version, about = "XianZhu CLI - AI Agent Desktop Assistant")]
struct Cli {
    /// API Gateway 地址
    #[arg(long, default_value = "http://127.0.0.1:9800", global = true, env = "XIANZHU_API")]
    api: String,

    /// API Key（可选）
    #[arg(long, global = true, env = "XIANZHU_API_KEY")]
    api_key: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 交互式对话
    Chat {
        /// Session ID（不填则创建新会话）
        #[arg(short, long)]
        session: Option<String>,
        /// Agent ID（不填用第一个 Agent）
        #[arg(short, long)]
        agent: Option<String>,
        /// 直接发送消息（非交互模式）
        #[arg(short, long)]
        message: Option<String>,
    },

    /// 一次性推理（脚本化用途，无需会话）
    Infer {
        /// 要提问的内容
        prompt: String,
        /// 指定模型（可选）
        #[arg(short, long)]
        model: Option<String>,
        /// 系统指令前缀（可选）
        #[arg(short, long)]
        system: Option<String>,
        /// 以 JSON 输出结果
        #[arg(long)]
        json: bool,
    },

    /// Agent 管理
    #[command(subcommand)]
    Agents(AgentsCmd),

    /// 会话管理
    #[command(subcommand)]
    Sessions(SessionsCmd),

    /// 配置管理
    #[command(subcommand)]
    Config(ConfigCmd),

    /// 系统健康检查
    Doctor,

    /// 系统状态
    Status,

    /// 渠道管理
    #[command(subcommand)]
    Channels(ChannelsCmd),

    /// 定时任务
    #[command(subcommand)]
    Cron(CronCmd),

    /// 浏览器控制
    #[command(subcommand)]
    Browser(BrowserCmd),

    /// 搜索消息
    Search {
        /// 搜索关键词
        query: String,
        /// Agent ID
        #[arg(short, long)]
        agent: Option<String>,
    },

    /// 数据备份
    Backup,

    /// 模型管理
    #[command(subcommand)]
    Models(ModelsCmd),

    /// 插件管理
    #[command(subcommand)]
    Plugins(PluginsCmd),

    /// 技能管理
    #[command(subcommand)]
    Skills(SkillsCmd),

    /// 记忆搜索
    #[command(subcommand)]
    Memory(MemoryCmd),

    /// MCP Server 管理
    #[command(subcommand)]
    Mcp(McpCmd),

    /// 发消息到渠道
    Message {
        /// 渠道 (telegram/discord/slack/feishu/weixin)
        channel: String,
        /// 目标 chat_id
        target: String,
        /// 消息内容
        content: String,
    },

    /// 终端仪表盘 (TUI)
    Tui,

    /// 生成 shell 补全脚本
    Completion {
        /// Shell 类型 (bash/zsh/fish)
        shell: String,
    },
}

#[derive(Subcommand)]
enum AgentsCmd {
    /// 列出所有 Agent
    List,
    /// 创建 Agent
    Create {
        /// Agent 名称
        name: String,
        /// 模型
        #[arg(short, long, default_value = "gpt-4o")]
        model: String,
        /// 系统提示词
        #[arg(short, long)]
        prompt: Option<String>,
    },
    /// 删除 Agent
    Delete {
        /// Agent ID
        id: String,
    },
    /// 导出 Agent
    Export {
        /// Agent ID
        id: String,
    },
    /// 导入 Agent
    Import {
        /// JSON 文件路径
        file: String,
    },
}

#[derive(Subcommand)]
enum SessionsCmd {
    /// 列出会话
    List {
        #[arg(short, long)]
        agent: Option<String>,
    },
    /// 查看会话历史
    History {
        /// Session ID
        id: String,
    },
    /// 导出会话
    Export {
        /// Session ID
        id: String,
        /// 格式 (markdown/json)
        #[arg(short, long, default_value = "markdown")]
        format: String,
    },
    /// 压缩会话上下文
    Compact {
        /// Session ID
        id: String,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// 获取配置
    Get { key: String },
    /// 设置配置
    Set { key: String, value: String },
    /// 列出所有配置
    List,
}

#[derive(Subcommand)]
enum ChannelsCmd {
    /// 列出渠道状态
    List,
    /// 渠道详情
    Status { channel: String },
}

#[derive(Subcommand)]
enum CronCmd {
    /// 列出任务
    List,
    /// 触发任务
    Trigger { id: String },
    /// 任务运行历史
    Runs { id: String },
}

#[derive(Subcommand)]
enum BrowserCmd {
    /// 列出已安装浏览器
    List,
    /// 打开 URL
    Open { url: String },
    /// 截图
    Screenshot {
        #[arg(long)]
        full_page: bool,
    },
    /// 页面快照 (ARIA)
    Snapshot {
        #[arg(long, default_value = "500")]
        limit: usize,
    },
}

#[derive(Subcommand)]
enum ModelsCmd {
    /// 列出可用模型
    List,
}

#[derive(Subcommand)]
enum PluginsCmd {
    /// 列出插件
    List,
}

#[derive(Subcommand)]
enum SkillsCmd {
    /// 列出技能
    List {
        #[arg(short, long)]
        agent: Option<String>,
    },
    /// 搜索技能
    Search { query: String },
    /// 安装技能
    Install { name: String },
}

#[derive(Subcommand)]
enum MemoryCmd {
    /// 搜索记忆
    Search { query: String },
}

#[derive(Subcommand)]
enum McpCmd {
    /// 列出 MCP 服务器
    List {
        #[arg(short, long)]
        agent: Option<String>,
    },
    /// 测试 MCP 连接
    Test { id: String },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let client = api::ApiClient::new(&cli.api, cli.api_key.as_deref());

    // 检查连接
    let result = match cli.command {
        Commands::Chat { session, agent, message } => {
            commands::chat::run(&client, agent.as_deref(), session.as_deref(), message.as_deref()).await
        }
        Commands::Infer { prompt, model, system, json } => {
            commands::infer::run(&client, model.as_deref(), &prompt, system.as_deref(), json).await
        }
        Commands::Agents(cmd) => commands::agents::run(&client, cmd).await,
        Commands::Sessions(cmd) => commands::sessions::run(&client, cmd).await,
        Commands::Config(cmd) => commands::config::run(&client, cmd).await,
        Commands::Doctor => commands::doctor::run(&client).await,
        Commands::Status => commands::status::run(&client).await,
        Commands::Channels(cmd) => commands::channels::run(&client, cmd).await,
        Commands::Cron(cmd) => commands::cron::run(&client, cmd).await,
        Commands::Browser(cmd) => commands::browser::run(&client, cmd).await,
        Commands::Search { query, agent } => commands::search::run(&client, &query, agent.as_deref()).await,
        Commands::Backup => commands::backup::run(&client).await,
        Commands::Models(cmd) => commands::models::run(&client, cmd).await,
        Commands::Plugins(cmd) => commands::plugins::run(&client, cmd).await,
        Commands::Skills(cmd) => commands::skills::run(&client, cmd).await,
        Commands::Memory(cmd) => commands::memory::run(&client, cmd).await,
        Commands::Mcp(cmd) => commands::mcp::run(&client, cmd).await,
        Commands::Message { channel, target, content } => {
            commands::message::run(&client, &channel, &target, &content).await
        }
        Commands::Tui => commands::tui::run(&client).await,
        Commands::Completion { shell } => {
            commands::completion::run(&shell);
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("{} {}", "Error:".red().bold(), e);
        std::process::exit(1);
    }
}
