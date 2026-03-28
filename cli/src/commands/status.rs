use crate::api::ApiClient;
use colored::Colorize;

pub async fn run(client: &ApiClient) -> Result<(), String> {
    let health = client.health().await?;
    println!("{}", "XianZhu Status".cyan().bold());
    println!("  {} Version: {}", "✓".green(), health["version"].as_str().unwrap_or("?"));
    println!("  {} Status:  {}", "✓".green(), health["status"].as_str().unwrap_or("?"));
    println!("  {} Time:    {}", "✓".green(), health["timestamp"].as_str().unwrap_or("?"));

    // Agent 列表
    let agents = client.list_agents().await?;
    let count = agents["count"].as_u64().unwrap_or(0);
    println!("  {} Agents:  {}", "✓".green(), count);

    if let Some(list) = agents["agents"].as_array() {
        for a in list {
            let name = a["name"].as_str().unwrap_or("?");
            let model = a["model"].as_str().unwrap_or("?");
            let id = a["id"].as_str().unwrap_or("?");
            println!("    {} {} ({}) [{}]", "•".dimmed(), name.bold(), model, &id[..8.min(id.len())]);

            // Token 统计
            if let Ok(stats) = client.token_stats(id).await {
                if let Some(models) = stats["models"].as_array() {
                    for m in models {
                        let model_name = m["model"].as_str().unwrap_or("?");
                        let calls = m["calls"].as_u64().unwrap_or(0);
                        let input = m["input"].as_u64().unwrap_or(0);
                        let output = m["output"].as_u64().unwrap_or(0);
                        if calls > 0 {
                            println!("      {} {} calls, {}+{} tokens",
                                model_name.dimmed(), calls, input, output);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
