use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppConfig {
    pub cwd: Option<String>,
    pub mode: Option<String>,
    pub resume: Option<String>,
    pub locale: Option<String>,
    pub continue_session: bool,
    pub no_color: bool,
    pub attach: Vec<String>,
    pub passthrough: Vec<String>,
    pub initial_prompts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    Prompt(String),
    Local(Vec<String>),
    Shell(String),
    Quit,
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    pub command: &'static str,
    pub summary: &'static str,
    pub route: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderAction {
    CommandPalette,
    Help,
    Editor,
    ClearConversation,
    ClearInput,
    Quit,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpConfig {
    #[serde(rename = "mcpServers", default)]
    pub servers: BTreeMap<String, McpServer>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServer {
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

pub fn parse_cli_args<I, S>(args: I) -> Result<AppConfig>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut config = AppConfig::default();
    let mut iter = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .peekable();
    let mut target: Option<String> = None;
    let mut target_replace = false;

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "tui" => {}
            "-c" | "--continue" => config.continue_session = true,
            "--target-replace" => target_replace = true,
            "--no-color" => config.no_color = true,
            "--cwd" => config.cwd = Some(next_value(&mut iter, "--cwd")?),
            "--mode" => config.mode = Some(next_value(&mut iter, "--mode")?),
            "--resume" => config.resume = Some(next_value(&mut iter, "--resume")?),
            "--locale" => config.locale = Some(next_value(&mut iter, "--locale")?),
            "--attach" => config.attach.push(next_value(&mut iter, "--attach")?),
            "--target" => target = Some(next_value(&mut iter, "--target")?),
            _ if arg.starts_with("--cwd=") => config.cwd = Some(split_equals(&arg)),
            _ if arg.starts_with("--mode=") => config.mode = Some(split_equals(&arg)),
            _ if arg.starts_with("--resume=") => config.resume = Some(split_equals(&arg)),
            _ if arg.starts_with("--locale=") => config.locale = Some(split_equals(&arg)),
            _ if arg.starts_with("--attach=") => config.attach.push(split_equals(&arg)),
            _ if arg.starts_with("--target=") => target = Some(split_equals(&arg)),
            _ => config.passthrough.push(arg),
        }
    }

    if let Some(goal) = target {
        let trimmed = goal.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("--target requires non-empty text"));
        }
        let prefix = if target_replace {
            "/goal replace"
        } else {
            "/goal"
        };
        config.initial_prompts.push(format!("{prefix} {trimmed}"));
    }

    Ok(config)
}

fn next_value<I>(iter: &mut std::iter::Peekable<I>, option: &str) -> Result<String>
where
    I: Iterator<Item = String>,
{
    iter.next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("{option} requires a value"))
}

fn split_equals(arg: &str) -> String {
    arg.split_once('=')
        .map(|(_, value)| value.to_string())
        .unwrap_or_default()
}

pub fn build_prompt_command(zcode_bin: &str, config: &AppConfig, prompt: &str) -> Vec<String> {
    let mut command = vec![zcode_bin.to_string()];

    if let Some(cwd) = &config.cwd {
        command.extend(["--cwd".to_string(), cwd.clone()]);
    }
    if let Some(mode) = &config.mode {
        command.extend(["--mode".to_string(), mode.clone()]);
    }
    if let Some(locale) = &config.locale {
        command.extend(["--locale".to_string(), locale.clone()]);
    }
    if config.continue_session {
        command.push("--continue".to_string());
    }
    if let Some(resume) = &config.resume {
        command.extend(["--resume".to_string(), resume.clone()]);
    }
    if config.no_color {
        command.push("--no-color".to_string());
    }
    for attach in &config.attach {
        command.extend(["--attach".to_string(), attach.clone()]);
    }
    command.extend(config.passthrough.iter().cloned());
    command.extend(["--prompt".to_string(), prompt.to_string()]);
    command
}

pub fn classify_input(input: &str) -> Result<InputAction> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(InputAction::Empty);
    }
    if let Some(command) = trimmed.strip_prefix('!') {
        let command = command.trim();
        if command.is_empty() {
            return Ok(InputAction::Empty);
        }
        return Ok(InputAction::Shell(command.to_string()));
    }
    if !trimmed.starts_with('/') {
        return Ok(InputAction::Prompt(trimmed.to_string()));
    }

    let without_slash = trimmed.trim_start_matches('/');
    let parts = shell_words::split(without_slash)
        .with_context(|| format!("failed to parse slash command: {trimmed}"))?;
    if parts.is_empty() {
        return Ok(InputAction::Empty);
    }

    match parts[0].as_str() {
        "exit" | "quit" => Ok(InputAction::Quit),
        "help" | "clear" | "editor" => Ok(InputAction::Local(parts)),
        "skills" => {
            let mut local = parts;
            if local.len() == 1 {
                local.push("list".to_string());
            }
            Ok(InputAction::Local(local))
        }
        "mcp" if is_local_mcp_command(parts.get(1).map(String::as_str)) => {
            Ok(InputAction::Local(parts))
        }
        "goal" | "skill" | "login" | "logout" | "compact" | "expert" | "fork" | "mode"
        | "model" | "new" | "resume" | "rewind" | "mcp" => {
            Ok(InputAction::Prompt(trimmed.to_string()))
        }
        _ => Ok(InputAction::Prompt(trimmed.to_string())),
    }
}

pub fn command_catalog() -> &'static [CommandSpec] {
    &[
        CommandSpec {
            command: "/goal",
            summary: "forward a goal to ZCode",
            route: "zcode",
        },
        CommandSpec {
            command: "/goal replace",
            summary: "replace the current ZCode goal",
            route: "zcode",
        },
        CommandSpec {
            command: "/skill",
            summary: "force a named ZCode skill for the prompt",
            route: "zcode",
        },
        CommandSpec {
            command: "/skills list",
            summary: "list available ZCode skills",
            route: "local",
        },
        CommandSpec {
            command: "/mcp list",
            summary: "list local .mcp.json servers",
            route: "local",
        },
        CommandSpec {
            command: "/mcp config",
            summary: "print the active .mcp.json path",
            route: "local",
        },
        CommandSpec {
            command: "/mcp add",
            summary: "add or update an MCP server",
            route: "local",
        },
        CommandSpec {
            command: "/mcp remove",
            summary: "remove an MCP server",
            route: "local",
        },
        CommandSpec {
            command: "/mcp status",
            summary: "ask ZCode for runtime MCP status",
            route: "zcode",
        },
        CommandSpec {
            command: "/model",
            summary: "forward model selection to ZCode",
            route: "zcode",
        },
        CommandSpec {
            command: "/mode",
            summary: "forward mode selection to ZCode",
            route: "zcode",
        },
        CommandSpec {
            command: "/resume",
            summary: "forward session resume to ZCode",
            route: "zcode",
        },
        CommandSpec {
            command: "/new",
            summary: "forward new-session request to ZCode",
            route: "zcode",
        },
        CommandSpec {
            command: "/compact",
            summary: "forward context compaction to ZCode",
            route: "zcode",
        },
        CommandSpec {
            command: "/editor",
            summary: "edit the prompt in $VISUAL or $EDITOR",
            route: "local",
        },
        CommandSpec {
            command: "/clear",
            summary: "clear conversation scrollback",
            route: "local",
        },
        CommandSpec {
            command: "/help",
            summary: "toggle the help overlay",
            route: "local",
        },
        CommandSpec {
            command: "/exit",
            summary: "quit the fallback TUI",
            route: "local",
        },
        CommandSpec {
            command: "! <cmd>",
            summary: "run a local shell command with sh -lc",
            route: "shell",
        },
    ]
}

pub fn slash_suggestions(input: &str, limit: usize) -> Vec<CommandSpec> {
    let prefix = input.trim();
    if prefix.is_empty() || !prefix.starts_with('/') || limit == 0 {
        return Vec::new();
    }
    command_catalog()
        .iter()
        .copied()
        .filter(|item| item.command.starts_with(prefix))
        .take(limit)
        .collect()
}

pub fn command_palette_rows() -> Vec<String> {
    command_catalog()
        .iter()
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

pub fn leader_action_for_key(key: char) -> Option<LeaderAction> {
    match key.to_ascii_lowercase() {
        'p' => Some(LeaderAction::CommandPalette),
        'h' | '?' => Some(LeaderAction::Help),
        'e' => Some(LeaderAction::Editor),
        'x' => Some(LeaderAction::ClearConversation),
        'u' => Some(LeaderAction::ClearInput),
        'q' => Some(LeaderAction::Quit),
        _ => None,
    }
}

fn is_local_mcp_command(command: Option<&str>) -> bool {
    matches!(
        command,
        None | Some("list" | "config" | "add" | "remove" | "rm")
    )
}

pub fn load_mcp_config(path: &Path) -> Result<McpConfig> {
    if !path.exists() {
        return Ok(McpConfig::default());
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read MCP config {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(McpConfig::default());
    }
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse MCP config {}", path.display()))
}

pub fn save_mcp_config(path: &Path, config: &McpConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(config)?;
    fs::write(path, format!("{content}\n"))
        .with_context(|| format!("failed to write MCP config {}", path.display()))
}

pub fn mcp_config_path(config: &AppConfig) -> Result<PathBuf> {
    let cwd = match &config.cwd {
        Some(path) => PathBuf::from(path),
        None => std::env::current_dir().context("failed to resolve current directory")?,
    };
    Ok(cwd.join(".mcp.json"))
}

pub fn run_prompt(zcode_bin: &str, config: &AppConfig, prompt: &str) -> Result<String> {
    let command = build_prompt_command(zcode_bin, config, prompt);
    run_command(&command)
}

pub fn run_shell_command(command: &str) -> Result<String> {
    run_command(&["sh".to_string(), "-lc".to_string(), command.to_string()])
}

pub fn run_command(command: &[String]) -> Result<String> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow!("empty command"))?;
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {program}"))?;
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let text = strip_ansi(&text);
    if output.status.success() {
        Ok(text)
    } else {
        Err(anyhow!(
            "command exited with {}:\n{}",
            output.status,
            text.trim_end()
        ))
    }
}

pub fn handle_local_command(
    command: &[String],
    config: &AppConfig,
    zcode_bin: &str,
) -> Result<String> {
    match command.first().map(String::as_str) {
        Some("help") => Ok(help_text().to_string()),
        Some("clear") => Ok("__CLEAR__".to_string()),
        Some("skills") => run_command(&build_zcode_passthrough_command(zcode_bin, command)),
        Some("mcp") => handle_mcp_command(command, config),
        Some(other) => Err(anyhow!("unknown local command: /{other}")),
        None => Ok(String::new()),
    }
}

fn build_zcode_passthrough_command(zcode_bin: &str, command: &[String]) -> Vec<String> {
    let mut result = vec![zcode_bin.to_string()];
    result.extend(command.iter().cloned());
    result
}

fn handle_mcp_command(command: &[String], config: &AppConfig) -> Result<String> {
    let path = mcp_config_path(config)?;
    let subcommand = command.get(1).map(String::as_str).unwrap_or("list");
    match subcommand {
        "list" => {
            let config = load_mcp_config(&path)?;
            if config.servers.is_empty() {
                return Ok(format!("No MCP servers configured in {}", path.display()));
            }
            let mut lines = vec![format!("MCP servers in {}:", path.display())];
            for (name, server) in config.servers {
                let args = if server.args.is_empty() {
                    String::new()
                } else {
                    format!(" {}", server.args.join(" "))
                };
                lines.push(format!("- {name}: {}{args}", server.command));
            }
            Ok(lines.join("\n"))
        }
        "config" => Ok(path.display().to_string()),
        "add" => {
            if command.len() < 4 {
                return Err(anyhow!("usage: /mcp add <name> <command> [args...]"));
            }
            let mut config_file = load_mcp_config(&path)?;
            config_file.servers.insert(
                command[2].clone(),
                McpServer {
                    command: command[3].clone(),
                    args: command[4..].to_vec(),
                    env: BTreeMap::new(),
                },
            );
            save_mcp_config(&path, &config_file)?;
            Ok(format!(
                "Added MCP server '{}' to {}",
                command[2],
                path.display()
            ))
        }
        "remove" | "rm" => {
            if command.len() != 3 {
                return Err(anyhow!("usage: /mcp remove <name>"));
            }
            let mut config_file = load_mcp_config(&path)?;
            let existed = config_file.servers.remove(&command[2]).is_some();
            save_mcp_config(&path, &config_file)?;
            if existed {
                Ok(format!(
                    "Removed MCP server '{}' from {}",
                    command[2],
                    path.display()
                ))
            } else {
                Ok(format!("MCP server '{}' was not configured", command[2]))
            }
        }
        _ => Err(anyhow!("unsupported local MCP command: /mcp {subcommand}")),
    }
}

pub fn help_text() -> &'static str {
    r#"zcode-tui fallback commands:
  text                         send a prompt with zcode --prompt
  ! <cmd>                      run a local shell command
  /goal <text>                 forward to ZCode goal handling
  /goal replace <text>         replace current goal through ZCode
  /skill <name> <task>         force a ZCode skill for a prompt
  /skills [list]               list ZCode skills through zcode skills list
  /mcp list                    list local .mcp.json servers
  /mcp config                  print local .mcp.json path
  /mcp add <name> <cmd> [args] add/update an MCP server in .mcp.json
  /mcp remove <name>           remove an MCP server from .mcp.json
  /mcp status                  forward to ZCode as /mcp status
  /editor                      edit current prompt in $VISUAL or $EDITOR
  /clear                       clear this screen
  /exit                        quit

keys:
  Ctrl+P                       command palette
  Ctrl+X then p/h/e/x/u/q      leader shortcuts
  Tab                          complete slash command suggestions
  Ctrl+G                       edit prompt externally
  Ctrl+J                       insert newline
"#
}

pub fn strip_ansi(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if ('@'..='~').contains(&code) {
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }

    output
}
