use crate::api::ApiClient;
use crate::SkillsCmd;
use colored::Colorize;

pub async fn run(client: &ApiClient, cmd: SkillsCmd) -> Result<(), String> {
    match cmd {
        SkillsCmd::List { agent } => {
            println!("{}", "Skills (requires extended API)".yellow());
            Ok(())
        }
        SkillsCmd::Search { query } => {
            println!("Searching skills: {}", query);
            println!("{}", "Use desktop app Skills page".yellow());
            Ok(())
        }
        SkillsCmd::Install { name } => {
            println!("Installing skill: {}", name);
            println!("{}", "Use desktop app to install skills".yellow());
            Ok(())
        }
    }
}
