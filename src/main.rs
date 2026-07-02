use std::env;
use std::io::{self, Stdout};

use anyhow::{Context, Result};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use zcode_tui::{
    classify_input, handle_local_command, help_text, parse_cli_args, run_prompt, AppConfig,
    InputAction,
};

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{}", help_text());
        return Ok(());
    }

    let config = parse_cli_args(args)?;
    let zcode_bin = env::var("ZCODE_TUI_ZCODE_BIN").unwrap_or_else(|_| "zcode".to_string());
    run_tui(config, &zcode_bin)
}

fn run_tui(config: AppConfig, zcode_bin: &str) -> Result<()> {
    let mut terminal = TerminalGuard::enter()?;
    let mut state = UiState::new(config, zcode_bin.to_string());
    state.push_system("Rust fallback started. Type /help for commands, Ctrl+Q to quit.");

    let initial_prompts = state.config.initial_prompts.clone();
    for prompt in initial_prompts {
        state.submit_prompt(&prompt);
    }

    loop {
        terminal.draw(&mut state)?;
        if let Event::Key(key) = event::read()? {
            if state.handle_key(key) {
                break;
            }
        }
    }

    Ok(())
}

#[derive(Debug)]
struct UiState {
    config: AppConfig,
    zcode_bin: String,
    log: Vec<LogLine>,
    input: String,
    history: Vec<String>,
    history_index: Option<usize>,
    scroll: u16,
    status: String,
    show_help: bool,
}

impl UiState {
    fn new(config: AppConfig, zcode_bin: String) -> Self {
        Self {
            config,
            zcode_bin,
            log: Vec::new(),
            input: String::new(),
            history: Vec::new(),
            history_index: None,
            scroll: 0,
            status: "ready".to_string(),
            show_help: false,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.log.clear();
                self.status = "cleared".to_string();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.clear();
                self.history_index = None;
            }
            KeyCode::Char(ch) => {
                self.input.push(ch);
                self.history_index = None;
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.history_index = None;
            }
            KeyCode::Enter => {
                let input = self.input.trim().to_string();
                self.input.clear();
                self.history_index = None;
                if self.handle_submit(&input) {
                    return true;
                }
            }
            KeyCode::Esc => {
                if self.show_help {
                    self.show_help = false;
                } else {
                    return true;
                }
            }
            KeyCode::Up => self.recall_history(-1),
            KeyCode::Down => self.recall_history(1),
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_add(6);
                self.status = format!("scroll +{}", self.scroll);
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_sub(6);
                self.status = format!("scroll +{}", self.scroll);
            }
            KeyCode::Home => self.scroll = u16::MAX / 2,
            KeyCode::End => self.scroll = 0,
            _ => {}
        }
        false
    }

    fn recall_history(&mut self, direction: isize) {
        if self.history.is_empty() {
            return;
        }
        let current = self.history_index.unwrap_or(self.history.len());
        let next = if direction < 0 {
            current.saturating_sub(1)
        } else {
            (current + 1).min(self.history.len())
        };
        self.history_index = (next < self.history.len()).then_some(next);
        self.input = self
            .history_index
            .map(|index| self.history[index].clone())
            .unwrap_or_default();
    }

    fn handle_submit(&mut self, input: &str) -> bool {
        if input.is_empty() {
            return false;
        }

        self.history.push(input.to_string());
        self.push_user(input);
        self.scroll = 0;

        match classify_input(input) {
            Ok(InputAction::Prompt(prompt)) => self.submit_prompt(&prompt),
            Ok(InputAction::Local(command)) => self.handle_local(&command),
            Ok(InputAction::Quit) => {
                self.status = "bye".to_string();
                return true;
            }
            Ok(InputAction::Empty) => {}
            Err(error) => self.push_error(&format!("{error:#}")),
        }
        false
    }

    fn handle_local(&mut self, command: &[String]) {
        if command.first().map(String::as_str) == Some("help") {
            self.show_help = !self.show_help;
            self.status = "help toggled".to_string();
            return;
        }

        match handle_local_command(command, &self.config, &self.zcode_bin) {
            Ok(output) if output == "__CLEAR__" => {
                self.log.clear();
                self.status = "cleared".to_string();
            }
            Ok(output) => {
                self.push_system(output.trim_end());
                self.status = "ok".to_string();
            }
            Err(error) => self.push_error(&format!("{error:#}")),
        }
    }

    fn submit_prompt(&mut self, prompt: &str) {
        self.status = "running zcode --prompt ...".to_string();
        self.push_system("running zcode --prompt ...");
        match run_prompt(&self.zcode_bin, &self.config, prompt) {
            Ok(output) if output.trim().is_empty() => {
                self.push_assistant("(no output)");
                self.status = "done".to_string();
            }
            Ok(output) => {
                self.push_assistant(output.trim_end());
                self.status = "done".to_string();
            }
            Err(error) => {
                self.push_error(&format!("{error:#}"));
                self.status = "error".to_string();
            }
        }
    }

    fn push_user(&mut self, text: &str) {
        self.log.push(LogLine::new(LogKind::User, text));
    }

    fn push_assistant(&mut self, text: &str) {
        self.log.push(LogLine::new(LogKind::Assistant, text));
    }

    fn push_system(&mut self, text: &str) {
        self.log.push(LogLine::new(LogKind::System, text));
    }

    fn push_error(&mut self, text: &str) {
        self.log.push(LogLine::new(LogKind::Error, text));
    }
}

#[derive(Debug, Clone, Copy)]
enum LogKind {
    User,
    Assistant,
    System,
    Error,
}

impl LogKind {
    fn label(self) -> &'static str {
        match self {
            Self::User => "you",
            Self::Assistant => "zcode",
            Self::System => "system",
            Self::Error => "error",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::User => Color::Cyan,
            Self::Assistant => Color::Green,
            Self::System => Color::Yellow,
            Self::Error => Color::Red,
        }
    }
}

#[derive(Debug)]
struct LogLine {
    kind: LogKind,
    text: String,
}

impl LogLine {
    fn new(kind: LogKind, text: &str) -> Self {
        Self {
            kind,
            text: text.to_string(),
        }
    }
}

struct TerminalGuard {
    terminal: Tui,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode().context("failed to enable raw mode")?;
        execute!(io::stdout(), EnterAlternateScreen, Hide)?;
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    fn draw(&mut self, state: &mut UiState) -> Result<()> {
        self.terminal.draw(|frame| render(frame, state))?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen);
    }
}

fn render(frame: &mut Frame<'_>, state: &mut UiState) {
    let root = frame.area();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Min(8),
            Constraint::Length(4),
            Constraint::Length(1),
        ])
        .split(root);

    render_brand(frame, vertical[0], state);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(50), Constraint::Length(34)])
        .split(vertical[1]);
    render_conversation(frame, body[0], state);
    render_sidebar(frame, body[1], state);
    render_input(frame, vertical[2], state);
    render_status(frame, vertical[3], state);

    if state.show_help {
        render_help_modal(frame, centered_rect(74, 70, root));
    }
}

fn render_brand(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let title = vec![
        Line::from(vec![
            Span::styled(
                "智谱",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled("@zcode", Style::default().fg(Color::Gray)),
        ]),
        Line::from(Span::styled(
            "Rust fallback TUI for Linux builds without @zcode/tui",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            format!(
                "mode: {}  cwd: {}",
                display_mode(&state.config),
                display_cwd(&state.config)
            ),
            Style::default().fg(Color::Yellow),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Line::from(" ZCode Linux TUI Patch ").cyan().bold());
    frame.render_widget(
        Paragraph::new(title)
            .block(block)
            .alignment(Alignment::Center),
        area,
    );
}

fn render_conversation(frame: &mut Frame<'_>, area: Rect, state: &mut UiState) {
    let items = state
        .log
        .iter()
        .flat_map(|entry| log_to_items(entry, area.width.saturating_sub(6) as usize))
        .collect::<Vec<_>>();
    let total = items.len() as u16;
    let height = area.height.saturating_sub(2);
    let max_scroll = total.saturating_sub(height);
    if state.scroll > max_scroll {
        state.scroll = max_scroll;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue))
        .title(Line::from(" Conversation ").blue().bold());
    let list = List::new(items).block(block).highlight_symbol(">> ");
    frame.render_stateful_widget(
        list,
        area,
        &mut ratatui::widgets::ListState::default().with_offset(state.scroll as usize),
    );
}

fn log_to_items(entry: &LogLine, width: usize) -> Vec<ListItem<'static>> {
    let mut items = Vec::new();
    let label = format!("[{}] ", entry.kind.label());
    let wrapped = wrap_text(&entry.text, width.saturating_sub(label.len()).max(10));

    for (index, line) in wrapped.into_iter().enumerate() {
        let prefix = if index == 0 {
            label.clone()
        } else {
            " ".repeat(label.len())
        };
        items.push(ListItem::new(Line::from(vec![
            Span::styled(prefix, Style::default().fg(entry.kind.color()).bold()),
            Span::raw(line),
        ])));
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            label,
            Style::default().fg(entry.kind.color()).bold(),
        ))));
    }

    items
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let commands = vec![
        Line::from(vec![Span::styled(
            "Commands",
            Style::default().fg(Color::Cyan).bold(),
        )]),
        Line::from("/goal <text>"),
        Line::from("/goal replace <text>"),
        Line::from("/skill <name> <task>"),
        Line::from("/skills list"),
        Line::from("/mcp list"),
        Line::from("/mcp add <name> <cmd>"),
        Line::from("/mcp remove <name>"),
        Line::from("/mcp status"),
        Line::from("/clear"),
        Line::from("/exit"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Keys",
            Style::default().fg(Color::Yellow).bold(),
        )]),
        Line::from("Up/Down: history"),
        Line::from("PgUp/PgDn: scroll"),
        Line::from("Ctrl+L: clear"),
        Line::from("Ctrl+U: clear input"),
        Line::from("Ctrl+Q/Esc: quit"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Session",
            Style::default().fg(Color::Green).bold(),
        )]),
        Line::from(format!("messages: {}", state.log.len())),
        Line::from(format!("history: {}", state.history.len())),
        Line::from(format!("status: {}", state.status)),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Line::from(" Control ").gray().bold());
    frame.render_widget(
        Paragraph::new(Text::from(commands))
            .block(block)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_input(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Line::from(" Prompt ").green().bold());
    let text = Paragraph::new(state.input.as_str())
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(text, area);

    let cursor_x = area.x.saturating_add(1).saturating_add(
        state
            .input
            .chars()
            .count()
            .min(area.width.saturating_sub(3) as usize) as u16,
    );
    let cursor_y = area.y.saturating_add(1);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn render_status(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let text = format!(
        " {} | Enter sends | /help toggles help | scroll:{} | {} ",
        state.status, state.scroll, state.zcode_bin
    );
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().bg(Color::Black).fg(Color::Gray))
            .alignment(Alignment::Left),
        area,
    );
}

fn render_help_modal(frame: &mut Frame<'_>, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(Line::from(" Help ").magenta().bold());
    let help = Paragraph::new(help_text())
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(help, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for raw_line in text.lines() {
        let chars = raw_line.chars().collect::<Vec<_>>();
        if chars.len() <= width {
            lines.push(raw_line.to_string());
            continue;
        }
        let mut start = 0;
        while start < chars.len() {
            let end = (start + width).min(chars.len());
            lines.push(chars[start..end].iter().collect());
            start = end;
        }
    }
    lines
}

fn display_mode(config: &AppConfig) -> &str {
    config.mode.as_deref().unwrap_or("default")
}

fn display_cwd(config: &AppConfig) -> String {
    config.cwd.clone().unwrap_or_else(|| {
        env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| ".".to_string())
    })
}
