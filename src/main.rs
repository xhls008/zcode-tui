use std::env;
use std::io::{self, Write};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::style::{Attribute, Print, SetAttribute};
use crossterm::terminal::{
    self, disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use crossterm::{execute, queue};
use zcode_tui::{
    classify_input, handle_local_command, help_text, parse_cli_args, run_prompt, AppConfig,
    InputAction,
};

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
    state.push_system("ZCode fallback TUI (Rust). Type /help for commands, /exit to quit.");

    let initial_prompts = state.config.initial_prompts.clone();
    for prompt in initial_prompts {
        state.submit_prompt(&prompt)?;
    }

    loop {
        terminal.draw(&state)?;
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Key(key) if state.handle_key(key)? => break,
                _ => {}
            }
        }
    }

    Ok(())
}

struct UiState {
    config: AppConfig,
    zcode_bin: String,
    log: Vec<LogLine>,
    input: String,
    history: Vec<String>,
    history_index: Option<usize>,
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
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
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
                if self.handle_submit(&input)? {
                    return Ok(true);
                }
            }
            KeyCode::Esc => return Ok(true),
            KeyCode::Up => self.recall_history(-1),
            KeyCode::Down => self.recall_history(1),
            _ => {}
        }
        Ok(false)
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

    fn handle_submit(&mut self, input: &str) -> Result<bool> {
        if input.is_empty() {
            return Ok(false);
        }
        self.history.push(input.to_string());
        self.push_user(input);

        match classify_input(input)? {
            InputAction::Prompt(prompt) => self.submit_prompt(&prompt)?,
            InputAction::Local(command) => {
                let output = handle_local_command(&command, &self.config, &self.zcode_bin)?;
                if output == "__CLEAR__" {
                    self.log.clear();
                } else {
                    self.push_system(output.trim_end());
                }
            }
            InputAction::Quit => {
                self.push_system("bye");
                return Ok(true);
            }
            InputAction::Empty => {}
        }
        Ok(false)
    }

    fn submit_prompt(&mut self, prompt: &str) -> Result<()> {
        self.push_system("running zcode --prompt ...");
        let result = run_prompt(&self.zcode_bin, &self.config, prompt);
        match result {
            Ok(output) if output.trim().is_empty() => self.push_assistant("(no output)"),
            Ok(output) => self.push_assistant(output.trim_end()),
            Err(error) => self.push_error(&format!("{error:#}")),
        }
        Ok(())
    }

    fn push_user(&mut self, text: &str) {
        self.log.push(LogLine::new("you", text));
    }

    fn push_assistant(&mut self, text: &str) {
        self.log.push(LogLine::new("zcode", text));
    }

    fn push_system(&mut self, text: &str) {
        self.log.push(LogLine::new("system", text));
    }

    fn push_error(&mut self, text: &str) {
        self.log.push(LogLine::new("error", text));
    }
}

struct LogLine {
    prefix: String,
    text: String,
}

impl LogLine {
    fn new(prefix: &str, text: &str) -> Self {
        Self {
            prefix: prefix.to_string(),
            text: text.to_string(),
        }
    }
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode().context("failed to enable raw mode")?;
        execute!(io::stdout(), EnterAlternateScreen, Hide)?;
        Ok(Self)
    }

    fn draw(&mut self, state: &UiState) -> Result<()> {
        let mut stdout = io::stdout();
        let (width, height) = terminal::size()?;
        let usable_height = height.saturating_sub(4) as usize;
        let rendered = render_lines(&state.log, width as usize);
        let start = rendered.len().saturating_sub(usable_height);

        queue!(
            stdout,
            MoveTo(0, 0),
            Clear(ClearType::All),
            SetAttribute(Attribute::Bold),
            Print("zcode-tui fallback"),
            SetAttribute(Attribute::Reset),
            Print("  /help  /goal  /skill  /skills  /mcp  /exit\n")
        )?;

        for (row, line) in rendered[start..].iter().enumerate() {
            queue!(
                stdout,
                MoveTo(0, (row + 2) as u16),
                Print(truncate(line, width as usize))
            )?;
        }

        let prompt_row = height.saturating_sub(1);
        queue!(
            stdout,
            MoveTo(0, prompt_row),
            Clear(ClearType::CurrentLine),
            SetAttribute(Attribute::Bold),
            Print("> "),
            SetAttribute(Attribute::Reset),
            Print(truncate(&state.input, width.saturating_sub(2) as usize))
        )?;
        stdout.flush()?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
    }
}

fn render_lines(log: &[LogLine], width: usize) -> Vec<String> {
    let content_width = width.saturating_sub(2).max(20);
    let mut lines = Vec::new();

    for entry in log {
        let prefix = format!("[{}] ", entry.prefix);
        for (index, raw_line) in entry.text.lines().enumerate() {
            let line_prefix = if index == 0 {
                prefix.clone()
            } else {
                " ".repeat(prefix.len())
            };
            wrap_line(
                raw_line,
                content_width.saturating_sub(line_prefix.len()),
                &mut |chunk| {
                    lines.push(format!("{line_prefix}{chunk}"));
                },
            );
        }
        if entry.text.is_empty() {
            lines.push(prefix);
        }
    }

    lines
}

fn wrap_line<F>(line: &str, width: usize, emit: &mut F)
where
    F: FnMut(&str),
{
    if width == 0 || line.chars().count() <= width {
        emit(line);
        return;
    }

    let chars: Vec<char> = line.chars().collect();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + width).min(chars.len());
        let chunk: String = chars[start..end].iter().collect();
        emit(&chunk);
        start = end;
    }
}

fn truncate(text: &str, width: usize) -> String {
    let mut result = String::new();
    for ch in text.chars().take(width) {
        result.push(ch);
    }
    result
}
