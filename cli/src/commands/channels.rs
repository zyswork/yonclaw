use crate::api::ApiClient;
use crate::ChannelsCmd;
use colored::Colorize;

pub async fn run(client: &ApiClient, cmd: ChannelsCmd) -> Result<(), String> {
    match cmd {
        ChannelsCmd::List => {
            println!("{}", "Channels:".cyan().bold());
            println!("  {} Telegram", "•".green());
            println!("  {} Discord", "•".green());
            println!("  {} Slack", "•".green());
            println!("  {} Feishu", "•".green());
            println!("  {} WeChat", "•".green());
            println!("{}", "Use desktop app for detailed status".dimmed());
            Ok(())
        }
        ChannelsCmd::Status { channel } => {
            println!("Channel status: {}", channel);
            println!("{}", "Use desktop app for channel management".dimmed());
            Ok(())
        }
    }
}
