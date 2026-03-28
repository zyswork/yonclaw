use crate::api::ApiClient;
use crate::SessionsCmd;
use colored::Colorize;

pub async fn run(client: &ApiClient, cmd: SessionsCmd) -> Result<(), String> {
    match cmd {
        SessionsCmd::List { agent } => {
            let agent_id = resolve_agent(client, agent.as_deref()).await?;
            let data = client.get(&format!("/api/v1/sessions/{}", agent_id)).await?;
            let sessions = data["sessions"].as_array().ok_or("无法获取会话")?;
            println!("{}", format!("{} Sessions:", sessions.len()).cyan().bold());
            for s in sessions {
                let title = s["title"].as_str().unwrap_or("?");
                let id = s["id"].as_str().unwrap_or("?");
                println!("  {} {} [{}]", "•".green(), title.bold(), &id[..id.len().min(8)].dimmed());
            }
            Ok(())
        }
        SessionsCmd::History { id } => {
            let data = client.get(&format!("/api/v1/messages/{}", id)).await?;
            let messages = data["messages"].as_array().ok_or("无法获取消息")?;
            println!("{}", format!("{} messages:", messages.len()).cyan().bold());
            for m in messages {
                let role = m["role"].as_str().unwrap_or("?");
                let content = m["content"].as_str().unwrap_or("");
                let preview: String = content.chars().take(120).collect();
                let prefix = match role { "user" => "You> ".green().bold(), "assistant" => "AI>  ".blue().bold(), _ => "SYS> ".yellow().bold() };
                println!("  {}{}", prefix, preview);
            }
            Ok(())
        }
        SessionsCmd::Export { id, format } => {
            let data = client.get(&format!("/api/v1/messages/{}", id)).await?;
            let messages = data["messages"].as_array().ok_or("无消息")?;
            if format == "json" {
                println!("{}", serde_json::to_string_pretty(&data).unwrap_or_default());
            } else {
                for m in messages {
                    let role = m["role"].as_str().unwrap_or("?");
                    let content = m["content"].as_str().unwrap_or("");
                    println!("**{}**: {}\n", role, content);
                }
            }
            Ok(())
        }
        SessionsCmd::Compact { id } => {
            let agent_id = resolve_agent(client, None).await?;
            println!("{}", "Compacting...".cyan());
            let data = client.post(&format!("/api/v1/compact/{}/{}", agent_id, id), &serde_json::json!({})).await?;
            println!("{}", data["result"].as_str().unwrap_or("Done").green());
            Ok(())
        }
    }
}

async fn resolve_agent(client: &ApiClient, explicit: Option<&str>) -> Result<String, String> {
    if let Some(id) = explicit { return Ok(id.to_string()); }
    let agents = client.list_agents().await?;
    agents["agents"].as_array().and_then(|a| a.first()).and_then(|a| a["id"].as_str())
        .map(String::from).ok_or("没有 Agent".into())
}
