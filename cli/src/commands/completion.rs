use colored::Colorize;

pub fn run(shell: &str) {
    match shell {
        "bash" => {
            println!(r#"# XianZhu bash completion
_xianzhu() {{
    local cur prev cmds
    cur="${{COMP_WORDS[COMP_CWORD]}}"
    prev="${{COMP_WORDS[COMP_CWORD-1]}}"
    cmds="chat agents sessions config doctor status channels cron browser search backup models plugins skills memory mcp message completion"
    COMPREPLY=($(compgen -W "$cmds" -- "$cur"))
}}
complete -F _xianzhu xianzhu"#);
        }
        "zsh" => {
            println!(r#"# XianZhu zsh completion
_xianzhu() {{
    local -a commands
    commands=(
        'chat:Interactive conversation'
        'agents:Agent management'
        'sessions:Session management'
        'config:Configuration'
        'doctor:Health check'
        'status:System status'
        'channels:Channel management'
        'cron:Cron jobs'
        'browser:Browser control'
        'search:Search messages'
        'backup:Database backup'
        'models:Model management'
        'plugins:Plugin management'
        'skills:Skill management'
        'memory:Memory search'
        'mcp:MCP server management'
        'message:Send channel message'
        'completion:Shell completion'
    )
    _describe 'command' commands
}}
compdef _xianzhu xianzhu"#);
        }
        "fish" => {
            println!("# XianZhu fish completion");
            for cmd in &["chat", "agents", "sessions", "config", "doctor", "status", "channels", "cron", "browser", "search", "backup", "models", "plugins", "skills", "memory", "mcp", "message"] {
                println!("complete -c xianzhu -n '__fish_use_subcommand' -a '{}' -d '{} management'", cmd, cmd);
            }
        }
        _ => {
            eprintln!("{} Unsupported shell: {}. Use bash, zsh, or fish.", "Error:".red(), shell);
        }
    }
}
