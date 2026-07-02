use std::env;
use std::fs;
use std::io::{self, Stdout};
use std::process::{self, Command};

use anyhow::{Context, Result};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use zcode_tui::{
    classify_input, command_catalog, command_palette_rows, handle_local_command, help_text,
    leader_action_for_key, parse_cli_args, run_prompt, run_shell_command, slash_suggestions,
    AppConfig, InputAction, LeaderAction,
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
    show_palette: bool,
    leader_pending: bool,
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
            show_palette: false,
            leader_pending: false,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.leader_pending {
            self.leader_pending = false;
            return self.handle_leader_key(key);
        }

        match key.code {
            KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.input.is_empty() {
                    return true;
                }
                self.input.clear();
                self.status = "input cleared".to_string();
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.show_palette = !self.show_palette;
                self.show_help = false;
                self.status = "command palette".to_string();
            }
            KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.leader_pending = true;
                self.status = "leader: p palette | h help | e editor | x clear | u input | q quit"
                    .to_string();
            }
            KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.open_external_editor();
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.push('\n');
                self.history_index = None;
            }
            KeyCode::Char('?') if self.input.is_empty() => {
                self.show_help = !self.show_help;
                self.show_palette = false;
                self.status = "help toggled".to_string();
            }
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
            KeyCode::Tab => self.complete_slash_command(),
            KeyCode::Enter => {
                let input = self.input.trim().to_string();
                self.input.clear();
                self.history_index = None;
                self.show_palette = false;
                if self.handle_submit(&input) {
                    return true;
                }
            }
            KeyCode::Esc => {
                if self.show_help {
                    self.show_help = false;
                } else if self.show_palette {
                    self.show_palette = false;
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

    fn handle_leader_key(&mut self, key: KeyEvent) -> bool {
        let action = match key.code {
            KeyCode::Esc => {
                self.status = "leader cancelled".to_string();
                return false;
            }
            KeyCode::Char(ch) => leader_action_for_key(ch),
            _ => None,
        };

        match action {
            Some(LeaderAction::CommandPalette) => {
                self.show_palette = !self.show_palette;
                self.show_help = false;
                self.status = "command palette".to_string();
            }
            Some(LeaderAction::Help) => {
                self.show_help = !self.show_help;
                self.show_palette = false;
                self.status = "help toggled".to_string();
            }
            Some(LeaderAction::Editor) => self.open_external_editor(),
            Some(LeaderAction::ClearConversation) => {
                self.log.clear();
                self.status = "cleared".to_string();
            }
            Some(LeaderAction::ClearInput) => {
                self.input.clear();
                self.status = "input cleared".to_string();
            }
            Some(LeaderAction::Quit) => return true,
            None => self.status = "unknown leader key".to_string(),
        }
        false
    }

    fn complete_slash_command(&mut self) {
        if !self.input.trim_start().starts_with('/') {
            self.status = "tab: slash completion only".to_string();
            return;
        }
        let suggestions = slash_suggestions(&self.input, 8);
        match suggestions.as_slice() {
            [] => self.status = "no slash matches".to_string(),
            [single] => {
                self.input = single.command.to_string();
                if !self.input.ends_with(' ') {
                    self.input.push(' ');
                }
                self.show_palette = false;
                self.status = format!("completed {}", single.command);
            }
            _ => {
                self.show_palette = true;
                self.show_help = false;
                self.status = format!("{} slash matches", suggestions.len());
            }
        }
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
            Ok(InputAction::Shell(command)) => self.submit_shell(&command),
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
            self.show_palette = false;
            self.status = "help toggled".to_string();
            return;
        }
        if command.first().map(String::as_str) == Some("editor") {
            self.open_external_editor();
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

    fn submit_shell(&mut self, command: &str) {
        self.status = format!("running ! {command}");
        self.push_system(&format!("$ {command}"));
        match run_shell_command(command) {
            Ok(output) if output.trim().is_empty() => {
                self.push_system("(no output)");
                self.status = "shell done".to_string();
            }
            Ok(output) => {
                self.push_system(output.trim_end());
                self.status = "shell done".to_string();
            }
            Err(error) => {
                self.push_error(&format!("{error:#}"));
                self.status = "shell error".to_string();
            }
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

    fn open_external_editor(&mut self) {
        match edit_input_in_editor(&self.input) {
            Ok(updated) => {
                self.input = updated.trim_end_matches('\n').to_string();
                self.status = "editor returned".to_string();
            }
            Err(error) => {
                self.push_error(&format!("{error:#}"));
                self.status = "editor error".to_string();
            }
        }
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

fn edit_input_in_editor(initial: &str) -> Result<String> {
    let editor = env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    let mut parts =
        shell_words::split(&editor).with_context(|| format!("failed to parse editor: {editor}"))?;
    if parts.is_empty() {
        parts.push("vi".to_string());
    }

    let path = env::temp_dir().join(format!("zcode-tui-{}-prompt.md", process::id()));
    fs::write(&path, initial).with_context(|| format!("failed to write {}", path.display()))?;

    let program = parts.remove(0);
    let mut args = parts;
    args.push(path.display().to_string());

    disable_raw_mode().context("failed to disable raw mode for editor")?;
    execute!(io::stdout(), LeaveAlternateScreen, Show)
        .context("failed to leave alternate screen for editor")?;
    let status = Command::new(&program)
        .args(&args)
        .status()
        .with_context(|| format!("failed to run editor: {program}"));
    let restore_screen = execute!(io::stdout(), EnterAlternateScreen, Hide)
        .context("failed to re-enter alternate screen after editor");
    let restore_raw = enable_raw_mode().context("failed to re-enable raw mode after editor");

    status?;
    restore_screen?;
    restore_raw?;

    let updated =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let _ = fs::remove_file(&path);
    Ok(updated)
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
            Constraint::Length(7),
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
    if state.show_palette {
        render_command_palette(frame, centered_rect(82, 68, root), state);
    }
}

fn render_brand(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let title = vec![
        Line::from(vec![
            Span::styled("智谱", Style::default().fg(Color::Cyan).bold()),
            Span::styled(" @zcode", Style::default().fg(Color::Magenta).bold()),
            Span::styled("  // RUST FALLBACK TUI", Style::default().fg(Color::Green)),
        ]),
        Line::from(Span::styled(
            "╾─ signal: online   route: prompt/local/shell/mcp   shell: ! <cmd>   palette: Ctrl+P ─╼",
            Style::default().fg(Color::Green),
        )),
        Line::from(Span::styled(
            "╾─ official Linux package missed @zcode/tui; this layer keeps the terminal alive ─╼",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(vec![
            Span::styled("mode ", Style::default().fg(Color::DarkGray)),
            Span::styled(display_mode(&state.config), Style::default().fg(Color::Yellow)),
            Span::styled("  cwd ", Style::default().fg(Color::DarkGray)),
            Span::styled(display_cwd(&state.config), Style::default().fg(Color::Yellow)),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(Line::from(" ZCODE // TERMINAL BUS ").green().bold());
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
        .border_style(Style::default().fg(Color::Green))
        .title(Line::from(" TRANSCRIPT ").green().bold());
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
    let mut commands = vec![Line::from(vec![Span::styled(
        "ROUTES",
        Style::default().fg(Color::Cyan).bold(),
    )])];
    for item in command_catalog().iter().take(12) {
        commands.push(Line::from(vec![
            Span::styled(
                format!("{:<14}", item.command),
                Style::default().fg(Color::Green),
            ),
            Span::styled(
                format!(" -> {}", item.route),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    commands.extend([
        Line::from(""),
        Line::from(vec![Span::styled(
            "HOTKEYS",
            Style::default().fg(Color::Magenta).bold(),
        )]),
        Line::from("Ctrl+P: palette"),
        Line::from("Ctrl+X: leader"),
        Line::from("Tab: slash complete"),
        Line::from("Ctrl+G: editor"),
        Line::from("Ctrl+J: newline"),
        Line::from("Up/Down: history"),
        Line::from("PgUp/PgDn: scroll"),
        Line::from("Ctrl+L: clear log"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "SESSION",
            Style::default().fg(Color::Green).bold(),
        )]),
        Line::from(format!("messages: {}", state.log.len())),
        Line::from(format!("history: {}", state.history.len())),
        Line::from(format!("status: {}", state.status)),
        Line::from(format!(
            "leader: {}",
            if state.leader_pending {
                "armed"
            } else {
                "idle"
            }
        )),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Line::from(" CONTROL MATRIX ").cyan().bold());
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
        .border_style(Style::default().fg(Color::Magenta))
        .title(
            Line::from(" PROMPT // Enter send // ! shell // Ctrl+J newline ")
                .magenta()
                .bold(),
        );
    let text = Paragraph::new(state.input.as_str())
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(text, area);

    let lines = state.input.lines().collect::<Vec<_>>();
    let last_line = lines.last().copied().unwrap_or("");
    let cursor_x = area.x.saturating_add(1).saturating_add(
        last_line
            .chars()
            .count()
            .min(area.width.saturating_sub(3) as usize) as u16,
    );
    let cursor_y = area.y.saturating_add(1).saturating_add(
        lines
            .len()
            .saturating_sub(1)
            .min(area.height.saturating_sub(3) as usize) as u16,
    );
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn render_status(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let text = format!(
        " {} | Ctrl+P palette | Ctrl+X leader | ? help | scroll:{} | bin:{} ",
        state.status, state.scroll, state.zcode_bin
    );
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().bg(Color::Black).fg(Color::Green))
            .alignment(Alignment::Left),
        area,
    );
}

fn render_command_palette(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let rows = if state.input.trim_start().starts_with('/') {
        let suggestions = slash_suggestions(&state.input, 18);
        if suggestions.is_empty() {
            command_palette_rows()
        } else {
            suggestions
                .into_iter()
                .map(|item| {
                    format!(
                        "{:<18} {:<5} {}",
                        item.command,
                        format!("[{}]", item.route),
                        item.summary
                    )
                })
                .collect()
        }
    } else {
        command_palette_rows()
    };

    let items = rows
        .into_iter()
        .map(|row| {
            ListItem::new(Line::from(Span::styled(
                row,
                Style::default().fg(Color::Green),
            )))
        })
        .collect::<Vec<_>>();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(
            Line::from(" COMMAND PALETTE // Ctrl+P close // Tab complete ")
                .magenta()
                .bold(),
        );
    frame.render_widget(Clear, area);
    frame.render_widget(List::new(items).block(block), area);
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
