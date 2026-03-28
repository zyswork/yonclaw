use crate::api::ApiClient;
use crate::ModelsCmd;
use colored::Colorize;

pub async fn run(client: &ApiClient, cmd: ModelsCmd) -> Result<(), String> {
    match cmd {
        ModelsCmd::List => {
            let agents = client.list_agents().await?;
            println!("{}", "Models in use:".cyan().bold());
            if let Some(list) = agents["agents"].as_array() {
                for a in list {
                    println!("  {} {} → {}",
                        "•".green(),
                        a["name"].as_str().unwrap_or("?"),
                        a["model"].as_str().unwrap_or("?").bold()
                    );
                }
            }
            Ok(())
        }
    }
}
