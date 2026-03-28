use crate::api::ApiClient;
use colored::Colorize;

pub async fn run(client: &ApiClient) -> Result<(), String> {
    println!("{}", "Creating backup...".cyan());
    match client.post("/api/v1/backup", &serde_json::json!({})).await {
        Ok(data) => {
            let path = data["path"].as_str().unwrap_or("?");
            let size = data["size_bytes"].as_u64().unwrap_or(0);
            println!("  {} Backup saved: {}", "✓".green().bold(), path);
            println!("  Size: {:.1} MB", size as f64 / 1_048_576.0);
            Ok(())
        }
        Err(e) => {
            println!("  {} {}", "✗".red(), e);
            println!();
            println!("  Manual backup:");
            println!("  cp ~/Library/Application\\ Support/com.xianzhu.app/xianzhu.db xianzhu-backup-$(date +%Y%m%d).db");
            Err(e)
        }
    }
}
