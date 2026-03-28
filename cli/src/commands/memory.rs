use crate::api::ApiClient;
use crate::MemoryCmd;
use colored::Colorize;

pub async fn run(client: &ApiClient, cmd: MemoryCmd) -> Result<(), String> {
    match cmd {
        MemoryCmd::Search { query } => {
            println!("{} \"{}\"", "Memory search:".cyan(), query);
            println!("{}", "Memory search requires extended API".yellow());
            Ok(())
        }
    }
}
