//! 交互式对话命令

use crate::api::ApiClient;
use colored::Colorize;
use std::io::{self, Write};

pub async fn run(
    client: &ApiClient,
    agent_id: Option<&str>,
    session_id: Option<&str>,
    message: Option<&str>,
) -> Result<(), String> {
    // 获取 Agent
    let agents = client.list_agents().await?;
    let agent_list = agents["agents"].as_array().ok_or("无法获取 Agent 列表")?;
    if agent_list.is_empty() {
        return Err("没有 Agent。请先在桌面端创建一个。".into());
    }

    let agent = if let Some(id) = agent_id {
        agent_list.iter().find(|a| a["id"].as_str() == Some(id))
            .ok_or(format!("Agent {} 不存在", id))?
    } else {
        &agent_list[0]
    };

    let aid = agent["id"].as_str().unwrap_or("");
    let agent_name = agent["name"].as_str().unwrap_or("Agent");
    let model = agent["model"].as_str().unwrap_or("?");

    println!("{}", format!("XianZhu Chat — {} ({})", agent_name, model).cyan().bold());
    println!("{}", "Type your message, /help for commands, Ctrl+C to exit".dimmed());
    println!();

    // 使用指定 session 或创建新的
    let sid = session_id.unwrap_or("cli-session").to_string();

    // 单消息模式
    if let Some(msg) = message {
        let resp = client.send_message(aid, &sid, msg).await?;
        let reply = resp["response"].as_str().unwrap_or("");
        println!("{}", reply);
        return Ok(());
    }

    // 交互模式
    loop {
        print!("{} ", "You>".green().bold());
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            break;
        }
        let input = input.trim();
        if input.is_empty() { continue; }

        // 内置命令
        match input {
            "/exit" | "/quit" | "/q" => {
                println!("{}", "Bye!".dimmed());
                break;
            }
            "/help" => {
                println!("{}", "Commands:".yellow().bold());
                println!("  /exit     Exit chat");
                println!("  /status   Show agent status");
                println!("  /clear    Clear screen");
                println!("  /model    Show current model");
                continue;
            }
            "/clear" => {
                print!("\x1B[2J\x1B[1;1H");
                continue;
            }
            "/status" | "/model" => {
                println!("{}: {} ({})", "Agent".cyan(), agent_name, model);
                println!("{}: {}", "Session".cyan(), sid);
                continue;
            }
            _ => {}
        }

        // 发送消息
        print!("{} ", "AI>".blue().bold());
        io::stdout().flush().unwrap();

        match client.send_message(aid, &sid, input).await {
            Ok(resp) => {
                let reply = resp["response"].as_str().unwrap_or("(empty)");
                println!("{}", reply);
            }
            Err(e) => {
                println!("{} {}", "Error:".red(), e);
            }
        }
        println!();
    }

    Ok(())
}
