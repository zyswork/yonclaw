use crate::api::ApiClient;
use colored::Colorize;

pub async fn run(client: &ApiClient, channel: &str, target: &str, content: &str) -> Result<(), String> {
    println!("{} {} → {} : {}", "Sending:".cyan(), channel, target, content);
    println!("{}", "Channel messaging requires extended API".yellow());
    Ok(())
}
