use crate::api::ApiClient;
use crate::ConfigCmd;
use colored::Colorize;

pub async fn run(client: &ApiClient, cmd: ConfigCmd) -> Result<(), String> {
    match cmd {
        ConfigCmd::Get { key } => {
            let data = client.get(&format!("/api/v1/settings/{}", key)).await?;
            let value = data["value"].as_str().unwrap_or("(not set)");
            println!("{} = {}", key.cyan(), value.bold());
            Ok(())
        }
        ConfigCmd::Set { key, value } => {
            client.post("/api/v1/settings", &serde_json::json!({"key": key, "value": value})).await?;
            println!("{} {} = {}", "✓".green(), key.cyan(), value.bold());
            Ok(())
        }
        ConfigCmd::List => {
            let common_keys = [
                "web_search_provider", "gateway_port", "gateway_api_key",
                "embedding_api_key", "embedding_api_url",
                "telegram_bot_token", "discord_bot_token", "slack_bot_token",
                "notification_enabled", "theme",
            ];
            println!("{}", "Settings:".cyan().bold());
            for key in &common_keys {
                match client.get(&format!("/api/v1/settings/{}", key)).await {
                    Ok(data) => {
                        let value = data["value"].as_str();
                        if let Some(v) = value {
                            let display = if key.contains("token") || key.contains("key") {
                                if v.len() > 8 { format!("{}...{}", &v[..4], &v[v.len()-4..]) } else { "****".into() }
                            } else { v.to_string() };
                            println!("  {} = {}", key, display.bold());
                        }
                    }
                    Err(_) => {}
                }
            }
            Ok(())
        }
    }
}
