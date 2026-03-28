//! TUI 终端仪表盘
//!
//! 基于 ratatui 的交互式终端界面。
//! 显示：Agent 列表、系统状态、对话窗口。

use crate::api::ApiClient;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;

struct App {
    agents: Vec<(String, String, String)>, // (id, name, model)
    selected_agent: usize,
    messages: Vec<(String, String)>, // (role, content)
    input: String,
    status: String,
    mode: Mode,
    should_quit: bool,
}

enum Mode {
    Normal,
    Input,
}

impl App {
    fn new() -> Self {
        Self {
            agents: Vec::new(),
            selected_agent: 0,
            messages: Vec::new(),
            input: String::new(),
            status: "Connecting...".into(),
            mode: Mode::Normal,
            should_quit: false,
        }
    }
}

pub async fn run(client: &ApiClient) -> Result<(), String> {
    // 初始化终端
    enable_raw_mode().map_err(|e| e.to_string())?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| e.to_string())?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;

    let mut app = App::new();

    // 加载数据
    match client.health().await {
        Ok(h) => app.status = format!("Connected v{}", h["version"].as_str().unwrap_or("?")),
        Err(e) => app.status = format!("Disconnected: {}", e),
    }

    if let Ok(data) = client.list_agents().await {
        if let Some(agents) = data["agents"].as_array() {
            app.agents = agents.iter().map(|a| (
                a["id"].as_str().unwrap_or("").to_string(),
                a["name"].as_str().unwrap_or("?").to_string(),
                a["model"].as_str().unwrap_or("?").to_string(),
            )).collect();
        }
    }

    // 主循环
    loop {
        terminal.draw(|f| draw_ui(f, &app)).map_err(|e| e.to_string())?;

        if event::poll(std::time::Duration::from_millis(100)).map_err(|e| e.to_string())? {
            if let Event::Key(key) = event::read().map_err(|e| e.to_string())? {
                match app.mode {
                    Mode::Normal => match key.code {
                        KeyCode::Char('q') => app.should_quit = true,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => app.should_quit = true,
                        KeyCode::Char('i') | KeyCode::Enter => app.mode = Mode::Input,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.selected_agent > 0 { app.selected_agent -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.selected_agent + 1 < app.agents.len() { app.selected_agent += 1; }
                        }
                        _ => {}
                    }
                    Mode::Input => match key.code {
                        KeyCode::Esc => app.mode = Mode::Normal,
                        KeyCode::Enter => {
                            if !app.input.is_empty() {
                                let msg = app.input.clone();
                                app.input.clear();
                                app.messages.push(("user".into(), msg.clone()));
                                app.messages.push(("assistant".into(), "...".into()));

                                // 发消息
                                if let Some((id, _, _)) = app.agents.get(app.selected_agent) {
                                    let sid = format!("tui-{}", app.selected_agent);
                                    match client.send_message(id, &sid, &msg).await {
                                        Ok(resp) => {
                                            let reply = resp["response"].as_str().unwrap_or("(empty)");
                                            if let Some(last) = app.messages.last_mut() {
                                                last.1 = reply.to_string();
                                            }
                                        }
                                        Err(e) => {
                                            if let Some(last) = app.messages.last_mut() {
                                                last.1 = format!("Error: {}", e);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        KeyCode::Backspace => { app.input.pop(); }
                        KeyCode::Char(c) => app.input.push(c),
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit { break; }
    }

    // 恢复终端
    disable_raw_mode().map_err(|e| e.to_string())?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).map_err(|e| e.to_string())?;
    terminal.show_cursor().map_err(|e| e.to_string())?;

    Ok(())
}

fn draw_ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(28), Constraint::Min(40)])
        .split(f.area());

    draw_sidebar(f, app, chunks[0]);
    draw_main(f, app, chunks[1]);
}

fn draw_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(area);

    // Agent 列表
    let items: Vec<ListItem> = app.agents.iter().enumerate().map(|(i, (_, name, model))| {
        let style = if i == app.selected_agent {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let prefix = if i == app.selected_agent { "▸ " } else { "  " };
        ListItem::new(vec![
            Line::from(Span::styled(format!("{}{}", prefix, name), style)),
            Line::from(Span::styled(format!("  {}", model), Style::default().fg(Color::DarkGray))),
        ])
    }).collect();

    let agent_list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" 🐾 Agents "));
    f.render_widget(agent_list, chunks[0]);

    // 状态栏
    let status = Paragraph::new(Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(&app.status, Style::default().fg(
            if app.status.starts_with("Connected") { Color::Green } else { Color::Red }
        )),
    ]))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(status, chunks[1]);
}

fn draw_main(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(area);

    // 消息区
    let msg_items: Vec<ListItem> = app.messages.iter().map(|(role, content)| {
        let (prefix, color) = match role.as_str() {
            "user" => ("You> ", Color::Green),
            "assistant" => ("AI>  ", Color::Blue),
            _ => ("SYS> ", Color::Yellow),
        };
        let truncated: String = content.chars().take(200).collect();
        ListItem::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Span::raw(truncated),
        ]))
    }).collect();

    let title = match app.agents.get(app.selected_agent) {
        Some((_, name, model)) => format!(" Chat — {} ({}) ", name, model),
        None => " Chat ".to_string(),
    };

    let messages_widget = List::new(msg_items)
        .block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(messages_widget, chunks[0]);

    // 输入区
    let input_style = match app.mode {
        Mode::Input => Style::default().fg(Color::Cyan),
        Mode::Normal => Style::default().fg(Color::DarkGray),
    };
    let hint = match app.mode {
        Mode::Input => "Type message, Enter to send, Esc to cancel",
        Mode::Normal => "Press 'i' to chat, ↑↓ to select agent, 'q' to quit",
    };
    let input = Paragraph::new(Line::from(vec![
        Span::styled("> ", input_style.add_modifier(Modifier::BOLD)),
        Span::styled(
            if app.input.is_empty() { hint } else { &app.input },
            if app.input.is_empty() { Style::default().fg(Color::DarkGray) } else { input_style },
        ),
    ]))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(input, chunks[1]);
}
