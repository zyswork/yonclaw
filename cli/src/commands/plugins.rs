use crate::api::ApiClient;
use crate::PluginsCmd;
use colored::Colorize;

pub async fn run(client: &ApiClient, cmd: PluginsCmd) -> Result<(), String> {
    match cmd {
        PluginsCmd::List => {
            println!("{}", "Built-in plugins:".cyan().bold());
            let plugins = [
                ("DuckDuckGo Search", "search", true),
                ("Serper (Google)", "search", true),
                ("Tavily AI", "search", true),
                ("OpenAI DALL-E", "image_gen", true),
                ("Local TTS", "tts", true),
                ("OpenAI TTS", "tts", true),
            ];
            for (name, cap, enabled) in &plugins {
                let status = if *enabled { "✓".green() } else { "✗".red() };
                println!("  {} {} [{}]", status, name, cap.dimmed());
            }
            Ok(())
        }
    }
}
