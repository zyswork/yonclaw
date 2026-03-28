use crate::api::ApiClient;
use crate::BrowserCmd;
use colored::Colorize;

pub async fn run(client: &ApiClient, cmd: BrowserCmd) -> Result<(), String> {
    match cmd {
        BrowserCmd::List => {
            println!("{}", "Detected browsers:".cyan().bold());
            // 本地检测，不需要 API
            #[cfg(target_os = "macos")]
            {
                let browsers = [
                    ("Chrome", "/Applications/Google Chrome.app"),
                    ("Brave", "/Applications/Brave Browser.app"),
                    ("Edge", "/Applications/Microsoft Edge.app"),
                ];
                for (name, path) in &browsers {
                    if std::path::Path::new(path).exists() {
                        println!("  {} {}", "✓".green(), name);
                    } else {
                        println!("  {} {} (not installed)", "✗".dimmed(), name.dimmed());
                    }
                }
            }
            Ok(())
        }
        BrowserCmd::Open { url } => {
            println!("Opening: {}", url);
            #[cfg(target_os = "macos")]
            { let _ = std::process::Command::new("open").arg(&url).spawn(); }
            #[cfg(target_os = "linux")]
            { let _ = std::process::Command::new("xdg-open").arg(&url).spawn(); }
            Ok(())
        }
        BrowserCmd::Screenshot { full_page } => {
            println!("Screenshot (full_page={}) requires running CDP session", full_page);
            println!("{}", "Use desktop app's browser tool".yellow());
            Ok(())
        }
        BrowserCmd::Snapshot { limit } => {
            println!("Snapshot (limit={}) requires running CDP session", limit);
            println!("{}", "Use desktop app's browser tool".yellow());
            Ok(())
        }
    }
}
