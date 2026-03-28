use crate::api::ApiClient;
use colored::Colorize;

pub async fn run(client: &ApiClient) -> Result<(), String> {
    println!("{}", "XianZhu Doctor".cyan().bold());
    println!();

    // 1. 连接检查
    print!("  Checking connection... ");
    match client.health().await {
        Ok(h) => println!("{} v{}", "OK".green().bold(), h["version"].as_str().unwrap_or("?")),
        Err(e) => {
            println!("{}", "FAILED".red().bold());
            println!("    {}", e.red());
            println!();
            println!("{}", "Make sure XianZhu desktop app is running and Gateway is enabled.".yellow());
            println!("Settings → Gateway Port → set a port (e.g. 9800)");
            return Ok(());
        }
    }

    // 2. Agent 检查
    print!("  Checking agents... ");
    match client.list_agents().await {
        Ok(a) => {
            let count = a["count"].as_u64().unwrap_or(0);
            if count > 0 { println!("{} ({} agents)", "OK".green().bold(), count); }
            else { println!("{} No agents configured", "WARN".yellow().bold()); }
        }
        Err(e) => println!("{} {}", "ERROR".red().bold(), e),
    }

    // 3. 详细诊断
    print!("  Running diagnostics... ");
    match client.get("/api/v1/doctor").await {
        Ok(data) => {
            println!("{}", "Done".green());
            if let Some(results) = data["results"].as_array() {
                let mut ok = 0; let mut warn = 0; let mut err = 0;
                for r in results {
                    match r["status"].as_str().unwrap_or("") {
                        "Ok" => ok += 1,
                        "Warning" => warn += 1,
                        "Error" => err += 1,
                        _ => {}
                    }
                }
                println!("    {} passed, {} warnings, {} errors", ok.to_string().green(), warn.to_string().yellow(), err.to_string().red());
                // 显示非 OK 的
                for r in results {
                    let status = r["status"].as_str().unwrap_or("");
                    if status != "Ok" {
                        let icon = match status { "Warning" => "⚠".yellow(), "Error" => "✗".red(), _ => "?".dimmed() };
                        let cat = r["category"].as_str().unwrap_or("?");
                        let check = r["check"].as_str().unwrap_or("?");
                        let msg = r["message"].as_str().unwrap_or("");
                        println!("    {} [{}] {}: {}", icon, cat, check.bold(), msg);
                    }
                }
            }
        }
        Err(_) => {
            println!("{}", "API not available".yellow());
            println!("    {}", "Doctor API requires gateway v0.1.0+".dimmed());
        }
    }

    Ok(())
}
