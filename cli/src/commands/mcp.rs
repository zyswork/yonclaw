use crate::api::ApiClient;
use crate::McpCmd;
use colored::Colorize;

pub async fn run(client: &ApiClient, cmd: McpCmd) -> Result<(), String> {
    match cmd {
        McpCmd::List { agent } => {
            println!("{}", "MCP Servers (requires extended API)".yellow());
            Ok(())
        }
        McpCmd::Test { id } => {
            println!("Testing MCP connection: {}", id);
            println!("{}", "Use desktop app for MCP testing".yellow());
            Ok(())
        }
    }
}
