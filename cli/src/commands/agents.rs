use crate::api::ApiClient;
use crate::AgentsCmd;
use colored::Colorize;

pub async fn run(client: &ApiClient, cmd: AgentsCmd) -> Result<(), String> {
    match cmd {
        AgentsCmd::List => {
            let data = client.list_agents().await?;
            let agents = data["agents"].as_array().ok_or("无法获取")?;
            println!("{}", format!("{} Agents:", agents.len()).cyan().bold());
            for a in agents {
                println!("  {} {} ({})",
                    "•".green(),
                    a["name"].as_str().unwrap_or("?").bold(),
                    a["model"].as_str().unwrap_or("?").dimmed()
                );
                println!("    ID: {}", a["id"].as_str().unwrap_or("?").dimmed());
            }
            Ok(())
        }
        AgentsCmd::Create { name, model, prompt } => {
            let prompt = prompt.unwrap_or_else(|| "你是一个有用的AI助手。".into());
            println!("{} {} ({})", "Creating:".cyan(), name.bold(), model);
            let data = client.post("/api/v1/agents", &serde_json::json!({
                "name": name, "model": model, "systemPrompt": prompt,
            })).await?;
            let id = data["id"].as_str().unwrap_or("?");
            println!("  {} Created: {} [{}]", "✓".green().bold(), data["name"].as_str().unwrap_or("?"), id);
            Ok(())
        }
        AgentsCmd::Delete { id } => {
            println!("{} {}", "Deleting:".red(), &id[..id.len().min(8)]);
            client.get(&format!("/api/v1/agents/{}?_method=DELETE", id)).await
                .or_else(|_| {
                    // 简单方案：用自定义 DELETE（部分 gateway 不支持）
                    Err("Delete requires desktop app or extended gateway".to_string())
                })?;
            println!("  {} Deleted", "✓".green());
            Ok(())
        }
        AgentsCmd::Export { id } => {
            println!("{}", "Agent export (use desktop app for full bundle export)".yellow());
            // 列出 agent 基本信息
            let data = client.list_agents().await?;
            if let Some(agents) = data["agents"].as_array() {
                if let Some(a) = agents.iter().find(|a| a["id"].as_str() == Some(&id)) {
                    println!("{}", serde_json::to_string_pretty(a).unwrap_or_default());
                }
            }
            Ok(())
        }
        AgentsCmd::Import { file } => {
            println!("Importing from: {}", file);
            let content = std::fs::read_to_string(&file).map_err(|e| format!("读取文件失败: {}", e))?;
            let bundle: serde_json::Value = serde_json::from_str(&content).map_err(|e| format!("JSON 解析失败: {}", e))?;
            if let Some(agent) = bundle.get("agent") {
                let name = agent["name"].as_str().unwrap_or("Imported");
                let model = agent["model"].as_str().unwrap_or("gpt-4o");
                let prompt = agent["system_prompt"].as_str().unwrap_or("");
                let data = client.post("/api/v1/agents", &serde_json::json!({
                    "name": name, "model": model, "systemPrompt": prompt,
                })).await?;
                println!("  {} Imported: {} [{}]", "✓".green().bold(), name, data["id"].as_str().unwrap_or("?"));
            } else {
                return Err("Invalid bundle format".into());
            }
            Ok(())
        }
    }
}
