use crate::api::ApiClient;
use crate::CronCmd;
use colored::Colorize;

pub async fn run(client: &ApiClient, cmd: CronCmd) -> Result<(), String> {
    match cmd {
        CronCmd::List => {
            println!("{}", "Cron jobs (requires extended API)".yellow());
            Ok(())
        }
        CronCmd::Trigger { id } => {
            // 可以通过 webhook 触发
            let resp = client.post(&format!("/webhook/{}", id), &serde_json::json!({})).await;
            match resp {
                Ok(data) => {
                    println!("{} Triggered: {}", "✓".green(), data["jobName"].as_str().unwrap_or("?"));
                    Ok(())
                }
                Err(e) => Err(format!("Trigger failed: {}", e)),
            }
        }
        CronCmd::Runs { id } => {
            println!("Cron runs for: {}", id);
            println!("{}", "Use desktop app for run history".yellow());
            Ok(())
        }
    }
}
