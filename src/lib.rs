use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpScope {
    Project,
    User,
}

impl McpScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpConfig {
    #[serde(rename = "mcpServers", default)]
    pub servers: BTreeMap<String, McpServer>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServer {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl McpServer {
    pub fn transport_label(&self) -> &str {
        match self.transport.as_deref() {
            Some(explicit) => explicit,
            None if self.url.is_some() => "http",
            None => "stdio",
        }
    }

    fn one_line(&self) -> String {
        let target = if let Some(url) = &self.url {
            url.clone()
        } else if self.args.is_empty() {
            self.command.clone()
        } else {
            format!("{} {}", self.command, self.args.join(" "))
        };
        let suffix = if self.disabled { " (disabled)" } else { "" };
        format!("[{}] {target}{suffix}", self.transport_label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthStatus {
    EnvKey { variable: String, masked: String },
    CredentialFile(PathBuf),
    None,
}

impl AuthStatus {
    pub fn short_label(&self) -> String {
        match self {
            Self::EnvKey { variable, .. } => format!("env:{variable}"),
            Self::CredentialFile(_) => "credentials".to_string(),
            Self::None => "none".to_string(),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::EnvKey { variable, masked } => {
                format!("authenticated via ${variable} ({masked})")
            }
            Self::CredentialFile(path) => {
                format!("authenticated via credential file {}", path.display())
            }
            Self::None => {
                "not authenticated: no API key env var or credential file found; run /login"
                    .to_string()
            }
        }
    }
}

/// Environment variables checked, in priority order, for an API key.
pub const AUTH_ENV_VARS: &[&str] = &["ZCODE_API_KEY", "ZHIPUAI_API_KEY", "ZAI_API_KEY"];

pub fn auth_credential_candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        // Path used by the ZCode CLI kernel shipped in the Linux desktop package.
        home.join(".zcode").join("v2").join("credentials.json"),
        home.join(".zcode").join("credentials.json"),
        home.join(".config").join("zcode").join("credentials.json"),
        home.join(".config").join("zcode").join("auth.json"),
    ]
}

pub fn detect_auth_status_with<F>(env_lookup: F, home: Option<&Path>) -> AuthStatus
where
    F: Fn(&str) -> Option<String>,
{
    for variable in AUTH_ENV_VARS {
        if let Some(value) = env_lookup(variable) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return AuthStatus::EnvKey {
                    variable: variable.to_string(),
                    masked: mask_secret(trimmed),
                };
            }
        }
    }
    if let Some(home) = home {
        for path in auth_credential_candidates(home) {
            if path.exists() {
                return AuthStatus::CredentialFile(path);
            }
        }
    }
    AuthStatus::None
}

pub fn detect_auth_status() -> AuthStatus {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    detect_auth_status_with(|key| std::env::var(key).ok(), home.as_deref())
}

pub fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 8 {
        return "****".to_string();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

pub fn login_command(zcode_bin: &str, override_command: Option<&str>) -> Result<Vec<String>> {
    build_auth_command(zcode_bin, "login", override_command)
}

pub fn logout_command(zcode_bin: &str, override_command: Option<&str>) -> Result<Vec<String>> {
    build_auth_command(zcode_bin, "logout", override_command)
}

fn build_auth_command(
    zcode_bin: &str,
    action: &str,
    override_command: Option<&str>,
) -> Result<Vec<String>> {
    if let Some(raw) = override_command {
        let parts = shell_words::split(raw)
            .with_context(|| format!("failed to parse auth command override: {raw}"))?;
        if parts.is_empty() {
            return Err(anyhow!("auth command override is empty"));
        }
        return Ok(parts);
    }
    // The real ZCode CLI exposes `zcode login` / `zcode logout` (Z.AI OAuth).
    Ok(vec![zcode_bin.to_string(), action.to_string()])
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
    build_prompt_command_with_attachments(zcode_bin, config, prompt, &[])
}

pub fn build_prompt_command_with_attachments(
    zcode_bin: &str,
    config: &AppConfig,
    prompt: &str,
    attachments: &[String],
) -> Vec<String> {
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
    for attach in attachments {
        command.extend(["--attach".to_string(), attach.clone()]);
    }
    command.extend(config.passthrough.iter().cloned());
    command.extend(["--prompt".to_string(), prompt.to_string()]);
    command
}

/// Extract `@path` mentions that resolve to regular files inside `cwd`.
/// Symlinks are resolved first, so `../`, absolute paths, and links pointing
/// outside the project are all rejected rather than silently attached.
pub fn extract_file_mentions(prompt: &str, cwd: &Path) -> Vec<String> {
    let Ok(canonical_cwd) = cwd.canonicalize() else {
        return Vec::new();
    };
    let mut mentions = Vec::new();
    for token in prompt.split_whitespace() {
        let Some(raw) = token.strip_prefix('@') else {
            continue;
        };
        let cleaned = raw.trim_matches(|c: char| matches!(c, ',' | ';' | ':' | '"' | '\'' | ')'));
        if cleaned.is_empty() || mentions.iter().any(|seen| seen == cleaned) {
            continue;
        }
        let Ok(resolved) = cwd.join(cleaned).canonicalize() else {
            continue;
        };
        if resolved.is_file() && resolved.starts_with(&canonical_cwd) {
            mentions.push(cleaned.to_string());
        }
    }
    mentions
}

pub fn prompt_command_for(
    zcode_bin: &str,
    config: &AppConfig,
    prompt: &str,
) -> Result<Vec<String>> {
    let cwd = resolve_cwd(config)?;
    let mentions = extract_file_mentions(prompt, &cwd);
    Ok(build_prompt_command_with_attachments(
        zcode_bin, config, prompt, &mentions,
    ))
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
        "help" | "clear" | "editor" | "login" | "logout" | "auth" | "status" | "diff" | "ide"
        | "mode" | "resume" | "new" => Ok(InputAction::Local(parts)),
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
            summary: "list project and user MCP servers",
            route: "local",
        },
        CommandSpec {
            command: "/mcp config",
            summary: "print the active MCP config paths",
            route: "local",
        },
        CommandSpec {
            command: "/mcp add",
            summary: "add stdio server; --transport http|sse for remote",
            route: "local",
        },
        CommandSpec {
            command: "/mcp add-json",
            summary: "add a server from raw JSON",
            route: "local",
        },
        CommandSpec {
            command: "/mcp get",
            summary: "show one MCP server as JSON",
            route: "local",
        },
        CommandSpec {
            command: "/mcp enable",
            summary: "re-enable a disabled MCP server",
            route: "local",
        },
        CommandSpec {
            command: "/mcp disable",
            summary: "disable an MCP server without removing it",
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
            command: "/login",
            summary: "sign in through the zcode CLI (interactive)",
            route: "local",
        },
        CommandSpec {
            command: "/logout",
            summary: "sign out through the zcode CLI",
            route: "local",
        },
        CommandSpec {
            command: "/auth",
            summary: "show local auth status (env key / credentials)",
            route: "local",
        },
        CommandSpec {
            command: "/status",
            summary: "show session, auth, and MCP overview",
            route: "local",
        },
        CommandSpec {
            command: "/model",
            summary: "forward model selection to ZCode",
            route: "zcode",
        },
        CommandSpec {
            command: "/mode",
            summary: "show or switch permission mode; Shift+Tab cycles",
            route: "local",
        },
        CommandSpec {
            command: "/resume",
            summary: "resume the latest session or one by sess_ id",
            route: "local",
        },
        CommandSpec {
            command: "/new",
            summary: "start a fresh session; context resets",
            route: "local",
        },
        CommandSpec {
            command: "/compact",
            summary: "forward context compaction to ZCode",
            route: "zcode",
        },
        CommandSpec {
            command: "/diff",
            summary: "show git diff with syntax coloring",
            route: "local",
        },
        CommandSpec {
            command: "/ide",
            summary: "open the working directory in your IDE",
            route: "local",
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
            command: "@<path>",
            summary: "mention a file; existing paths are auto-attached",
            route: "zcode",
        },
        CommandSpec {
            command: "! <cmd>",
            summary: "run a local shell command with sh -lc",
            route: "shell",
        },
    ]
}

pub fn slash_suggestions(input: &str, limit: usize) -> Vec<CommandSpec> {
    let query = input.trim();
    if query.is_empty() || !query.starts_with('/') || limit == 0 {
        return Vec::new();
    }
    let bare = query.trim_start_matches('/');
    let mut scored: Vec<(u8, usize, CommandSpec)> = Vec::new();
    for (index, item) in command_catalog().iter().enumerate() {
        if !item.command.starts_with('/') {
            continue;
        }
        let rank = if item.command.starts_with(query) {
            0
        } else if !bare.is_empty() && item.command.contains(bare) {
            1
        } else if is_subsequence(query, item.command) {
            2
        } else {
            continue;
        };
        scored.push((rank, index, *item));
    }
    scored.sort_by_key(|(rank, index, _)| (*rank, *index));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, _, item)| item)
        .collect()
}

fn is_subsequence(query: &str, candidate: &str) -> bool {
    let mut candidate_chars = candidate.chars().flat_map(char::to_lowercase);
    query
        .chars()
        .flat_map(char::to_lowercase)
        .all(|needle| candidate_chars.any(|hay| hay == needle))
}

const FILE_SCAN_SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    "dist",
    "build",
    "__pycache__",
    "venv",
];
const FILE_SCAN_MAX_DEPTH: usize = 4;
const FILE_SCAN_MAX_ENTRIES: usize = 4000;

/// Suggest paths relative to `root` matching `query`, for `@file` completion.
pub fn file_suggestions(root: &Path, query: &str, limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let mut entries = Vec::new();
    let mut scanned = 0usize;
    collect_paths(root, root, 0, &mut entries, &mut scanned);

    let needle = query.to_lowercase();
    let mut scored: Vec<(u8, usize, String)> = entries
        .into_iter()
        .filter_map(|path| {
            let hay = path.to_lowercase();
            let rank = if needle.is_empty() || hay.starts_with(&needle) {
                0
            } else if hay.contains(&needle) {
                1
            } else if is_subsequence(&needle, &hay) {
                2
            } else {
                return None;
            };
            Some((rank, path.len(), path))
        })
        .collect();
    scored.sort();
    scored
        .into_iter()
        .take(limit)
        .map(|(_, _, path)| path)
        .collect()
}

fn collect_paths(
    root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<String>,
    scanned: &mut usize,
) {
    if depth > FILE_SCAN_MAX_DEPTH || *scanned > FILE_SCAN_MAX_ENTRIES {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<_> = read.flatten().collect();
    children.sort_by_key(|entry| entry.file_name());
    for entry in children {
        *scanned += 1;
        if *scanned > FILE_SCAN_MAX_ENTRIES {
            return;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || FILE_SCAN_SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        if path.is_dir() {
            out.push(format!("{relative}/"));
            collect_paths(root, &path, depth + 1, out, scanned);
        } else {
            out.push(relative);
        }
    }
}

pub fn command_palette_rows() -> Vec<String> {
    command_catalog()
        .iter()
        .map(|item| {
            format!(
                "{:<18} {:<7} {}",
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
    match command {
        None => true,
        Some(sub) if sub.starts_with("--") => true,
        Some(
            "list" | "config" | "add" | "add-json" | "get" | "enable" | "disable" | "remove" | "rm",
        ) => true,
        _ => false,
    }
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

fn resolve_cwd(config: &AppConfig) -> Result<PathBuf> {
    match &config.cwd {
        Some(path) => Ok(PathBuf::from(path)),
        None => std::env::current_dir().context("failed to resolve current directory"),
    }
}

pub fn mcp_config_path(config: &AppConfig) -> Result<PathBuf> {
    Ok(resolve_cwd(config)?.join(".mcp.json"))
}

pub fn user_mcp_config_path_from(
    xdg_config_home: Option<&str>,
    home: Option<&str>,
) -> Result<PathBuf> {
    if let Some(xdg) = xdg_config_home.filter(|value| !value.trim().is_empty()) {
        return Ok(PathBuf::from(xdg).join("zcode").join("mcp.json"));
    }
    let home = home
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("cannot resolve home directory for user MCP config"))?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("zcode")
        .join("mcp.json"))
}

pub fn user_mcp_config_path() -> Result<PathBuf> {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let home = std::env::var("HOME").ok();
    user_mcp_config_path_from(xdg.as_deref(), home.as_deref())
}

pub fn run_prompt(zcode_bin: &str, config: &AppConfig, prompt: &str) -> Result<String> {
    let command = prompt_command_for(zcode_bin, config, prompt)?;
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

#[derive(Debug)]
pub enum JobEvent {
    Line(String),
    /// One output stream (stdout or stderr) reached end-of-file; all of its
    /// lines were sent before this event.
    Eof,
    Finished {
        success: bool,
        detail: String,
    },
}

/// A child process streaming its merged stdout/stderr line by line.
pub struct StreamingJob {
    pub receiver: Receiver<JobEvent>,
    /// Number of output streams; the job has drained fully once this many
    /// `Eof` events arrived.
    pub streams: usize,
    child: Arc<Mutex<Child>>,
}

impl StreamingJob {
    /// Kill the whole process group so grandchildren (e.g. a shell's
    /// subprocesses holding the output pipes) die too.
    pub fn cancel(&self) {
        if let Ok(mut child) = self.child.lock() {
            #[cfg(unix)]
            unsafe {
                libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
            }
            let _ = child.kill();
        }
    }
}

pub fn spawn_streaming_command(command: &[String]) -> Result<StreamingJob> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow!("empty command"))?;
    let mut process = Command::new(program);
    process
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }
    let mut child = process
        .spawn()
        .with_context(|| format!("failed to run {program}"))?;

    let (sender, receiver) = channel();
    let mut streams = 0;
    if let Some(stdout) = child.stdout.take() {
        spawn_line_reader(stdout, sender.clone());
        streams += 1;
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_line_reader(stderr, sender.clone());
        streams += 1;
    }

    let child = Arc::new(Mutex::new(child));
    let waiter_child = Arc::clone(&child);
    thread::spawn(move || loop {
        let finished = {
            let Ok(mut guard) = waiter_child.lock() else {
                let _ = sender.send(JobEvent::Finished {
                    success: false,
                    detail: "job state poisoned".to_string(),
                });
                return;
            };
            match guard.try_wait() {
                Ok(Some(status)) => Some(JobEvent::Finished {
                    success: status.success(),
                    detail: status.to_string(),
                }),
                Ok(None) => None,
                Err(error) => Some(JobEvent::Finished {
                    success: false,
                    detail: format!("failed to wait for child: {error}"),
                }),
            }
        };
        if let Some(event) = finished {
            let _ = sender.send(event);
            return;
        }
        thread::sleep(Duration::from_millis(60));
    });

    Ok(StreamingJob {
        receiver,
        streams,
        child,
    })
}

fn spawn_line_reader<R>(reader: R, sender: Sender<JobEvent>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let buffered = BufReader::new(reader);
        for line in buffered.lines() {
            let Ok(line) = line else {
                break;
            };
            if sender.send(JobEvent::Line(strip_ansi(&line))).is_err() {
                return;
            }
        }
        let _ = sender.send(JobEvent::Eof);
    });
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
        Some("auth") => Ok(detect_auth_status().describe()),
        Some("status") => status_summary(config, zcode_bin),
        Some("diff") => {
            let cwd = resolve_cwd(config)?;
            let output = run_command(&git_diff_command(&cwd, &command[1..]))?;
            if output.trim().is_empty() {
                Ok("working tree clean".to_string())
            } else {
                Ok(output)
            }
        }
        Some("logout") => {
            let override_command = std::env::var("ZCODE_TUI_LOGOUT_CMD").ok();
            let logout = logout_command(zcode_bin, override_command.as_deref())?;
            let output = run_command(&logout)?;
            if output.trim().is_empty() {
                Ok("logged out".to_string())
            } else {
                Ok(output)
            }
        }
        Some("login") => Err(anyhow!(
            "login is interactive; run it from the TUI or run `{zcode_bin} login` directly"
        )),
        Some("ide") => {
            let cwd = resolve_cwd(config)?;
            let target = command
                .get(1)
                .cloned()
                .unwrap_or_else(|| cwd.display().to_string());
            let override_command = std::env::var("ZCODE_TUI_IDE_CMD").ok();
            let launch = ide_command(override_command.as_deref(), &target)?;
            Ok(format!("__IDE__{}", shell_words::join(&launch)))
        }
        Some("mode") | Some("resume") | Some("new") => {
            Err(anyhow!("session commands are handled inside the TUI"))
        }
        Some(other) => Err(anyhow!("unknown local command: /{other}")),
        None => Ok(String::new()),
    }
}

pub fn status_summary(config: &AppConfig, zcode_bin: &str) -> Result<String> {
    let cwd = resolve_cwd(config)?;
    let auth = detect_auth_status();
    let mut lines = vec![
        format!("zcode-tui {}", env!("CARGO_PKG_VERSION")),
        format!("bin: {zcode_bin}"),
        format!("cwd: {}", cwd.display()),
        format!("mode: {}", config.mode.as_deref().unwrap_or("default")),
        format!("auth: {}", auth.describe()),
    ];
    for (scope, path) in [
        (McpScope::Project, mcp_config_path(config)),
        (McpScope::User, user_mcp_config_path()),
    ] {
        match path {
            Ok(path) => {
                let servers = load_mcp_config(&path).map(|c| c.servers.len()).unwrap_or(0);
                lines.push(format!(
                    "mcp [{}]: {} ({} server{})",
                    scope.label(),
                    path.display(),
                    servers,
                    if servers == 1 { "" } else { "s" }
                ));
            }
            Err(error) => lines.push(format!("mcp [{}]: {error}", scope.label())),
        }
    }
    Ok(lines.join("\n"))
}

fn build_zcode_passthrough_command(zcode_bin: &str, command: &[String]) -> Vec<String> {
    let mut result = vec![zcode_bin.to_string()];
    result.extend(command.iter().cloned());
    result
}

struct McpInvocation {
    scope: Option<McpScope>,
    transport: Option<String>,
    positional: Vec<String>,
}

/// Wrapper flags (--scope/--transport/...) are only interpreted before the
/// first three positionals (`add <name> <command>`), or before a literal
/// `--`; everything after belongs to the MCP server verbatim, so commands
/// like `/mcp add fs npx --user x` or `/mcp add fs -- npx --scope y` keep
/// the server's own arguments intact.
fn parse_mcp_invocation(args: &[String]) -> Result<McpInvocation> {
    let mut scope = None;
    let mut transport = None;
    let mut positional = Vec::new();
    let mut verbatim = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if verbatim {
            positional.push(args[index].clone());
            index += 1;
            continue;
        }
        match arg {
            "--" => verbatim = true,
            "--user" => scope = Some(McpScope::User),
            "--project" => scope = Some(McpScope::Project),
            "--scope" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("--scope requires a value: user or project"))?;
                scope = Some(parse_mcp_scope(value)?);
            }
            "--transport" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("--transport requires a value: stdio, sse, or http"))?;
                transport = Some(parse_mcp_transport(value)?);
            }
            _ if arg.starts_with("--scope=") => {
                scope = Some(parse_mcp_scope(&split_equals(arg))?);
            }
            _ if arg.starts_with("--transport=") => {
                transport = Some(parse_mcp_transport(&split_equals(arg))?);
            }
            _ => {
                positional.push(args[index].clone());
                if positional.len() >= 3 {
                    verbatim = true;
                }
            }
        }
        index += 1;
    }
    Ok(McpInvocation {
        scope,
        transport,
        positional,
    })
}

fn parse_mcp_scope(value: &str) -> Result<McpScope> {
    match value {
        "user" | "global" => Ok(McpScope::User),
        "project" | "local" => Ok(McpScope::Project),
        other => Err(anyhow!("unknown MCP scope: {other} (use user or project)")),
    }
}

fn parse_mcp_transport(value: &str) -> Result<String> {
    match value {
        "stdio" | "sse" | "http" => Ok(value.to_string()),
        other => Err(anyhow!(
            "unknown MCP transport: {other} (use stdio, sse, or http)"
        )),
    }
}

fn mcp_scope_path(scope: McpScope, config: &AppConfig) -> Result<PathBuf> {
    match scope {
        McpScope::Project => mcp_config_path(config),
        McpScope::User => user_mcp_config_path(),
    }
}

fn locate_mcp_server(
    name: &str,
    scope: Option<McpScope>,
    config: &AppConfig,
) -> Result<Option<(McpScope, PathBuf, McpConfig)>> {
    let scopes = match scope {
        Some(scope) => vec![scope],
        None => vec![McpScope::Project, McpScope::User],
    };
    for scope in scopes {
        let path = mcp_scope_path(scope, config)?;
        let file = load_mcp_config(&path)?;
        if file.servers.contains_key(name) {
            return Ok(Some((scope, path, file)));
        }
    }
    Ok(None)
}

fn handle_mcp_command(command: &[String], config: &AppConfig) -> Result<String> {
    let invocation = parse_mcp_invocation(&command[1..])?;
    let positional = &invocation.positional;
    let subcommand = positional.first().map(String::as_str).unwrap_or("list");
    let rest = if positional.is_empty() {
        &[] as &[String]
    } else {
        &positional[1..]
    };

    match subcommand {
        "list" => {
            let scopes = match invocation.scope {
                Some(scope) => vec![scope],
                None => vec![McpScope::Project, McpScope::User],
            };
            let mut lines = Vec::new();
            for scope in scopes {
                let path = mcp_scope_path(scope, config)?;
                let file = load_mcp_config(&path)?;
                if file.servers.is_empty() {
                    lines.push(format!(
                        "[{}] {} (no servers)",
                        scope.label(),
                        path.display()
                    ));
                    continue;
                }
                lines.push(format!("[{}] {}:", scope.label(), path.display()));
                for (name, server) in &file.servers {
                    lines.push(format!("- {name} {}", server.one_line()));
                }
            }
            Ok(lines.join("\n"))
        }
        "config" => {
            let project = mcp_config_path(config)?;
            let user = user_mcp_config_path()?;
            Ok(format!(
                "[project] {}\n[user] {}",
                project.display(),
                user.display()
            ))
        }
        "add" => {
            let name = rest
                .first()
                .ok_or_else(|| anyhow!("usage: /mcp add [--transport http|sse] [--scope user] <name> <command|url> [args...]"))?;
            let server = match invocation.transport.as_deref() {
                Some("http") | Some("sse") => {
                    let url = rest.get(1).ok_or_else(|| {
                        anyhow!("usage: /mcp add --transport http|sse <name> <url>")
                    })?;
                    McpServer {
                        transport: invocation.transport.clone(),
                        url: Some(url.clone()),
                        ..Default::default()
                    }
                }
                _ => {
                    let program = rest
                        .get(1)
                        .ok_or_else(|| anyhow!("usage: /mcp add <name> <command> [args...]"))?;
                    McpServer {
                        command: program.clone(),
                        args: rest[2..].to_vec(),
                        ..Default::default()
                    }
                }
            };
            let scope = invocation.scope.unwrap_or(McpScope::Project);
            let path = mcp_scope_path(scope, config)?;
            let mut file = load_mcp_config(&path)?;
            file.servers.insert(name.clone(), server);
            save_mcp_config(&path, &file)?;
            Ok(format!(
                "Added MCP server '{name}' to [{}] {}",
                scope.label(),
                path.display()
            ))
        }
        "add-json" => {
            let name = rest
                .first()
                .ok_or_else(|| anyhow!("usage: /mcp add-json <name> <json>"))?;
            let raw = rest[1..].join(" ");
            if raw.trim().is_empty() {
                return Err(anyhow!("usage: /mcp add-json <name> <json>"));
            }
            let server: McpServer = serde_json::from_str(&raw)
                .with_context(|| format!("invalid MCP server JSON: {raw}"))?;
            let scope = invocation.scope.unwrap_or(McpScope::Project);
            let path = mcp_scope_path(scope, config)?;
            let mut file = load_mcp_config(&path)?;
            file.servers.insert(name.clone(), server);
            save_mcp_config(&path, &file)?;
            Ok(format!(
                "Added MCP server '{name}' to [{}] {}",
                scope.label(),
                path.display()
            ))
        }
        "get" => {
            let name = rest
                .first()
                .ok_or_else(|| anyhow!("usage: /mcp get <name>"))?;
            match locate_mcp_server(name, invocation.scope, config)? {
                Some((scope, path, file)) => {
                    let server = &file.servers[name];
                    Ok(format!(
                        "{name} [{}] {}\n{}",
                        scope.label(),
                        path.display(),
                        serde_json::to_string_pretty(server)?
                    ))
                }
                None => Ok(format!("MCP server '{name}' is not configured")),
            }
        }
        "enable" | "disable" => {
            let name = rest
                .first()
                .ok_or_else(|| anyhow!("usage: /mcp {subcommand} <name>"))?;
            let disable = subcommand == "disable";
            match locate_mcp_server(name, invocation.scope, config)? {
                Some((scope, path, mut file)) => {
                    if let Some(server) = file.servers.get_mut(name) {
                        server.disabled = disable;
                    }
                    save_mcp_config(&path, &file)?;
                    Ok(format!(
                        "{} MCP server '{name}' in [{}] {}",
                        if disable { "Disabled" } else { "Enabled" },
                        scope.label(),
                        path.display()
                    ))
                }
                None => Ok(format!("MCP server '{name}' is not configured")),
            }
        }
        "remove" | "rm" => {
            let name = rest
                .first()
                .ok_or_else(|| anyhow!("usage: /mcp remove <name>"))?;
            match locate_mcp_server(name, invocation.scope, config)? {
                Some((scope, path, mut file)) => {
                    file.servers.remove(name);
                    save_mcp_config(&path, &file)?;
                    Ok(format!(
                        "Removed MCP server '{name}' from [{}] {}",
                        scope.label(),
                        path.display()
                    ))
                }
                None => Ok(format!("MCP server '{name}' was not configured")),
            }
        }
        _ => Err(anyhow!("unsupported local MCP command: /mcp {subcommand}")),
    }
}

pub fn help_text() -> &'static str {
    r#"zcode-tui fallback commands:
  text                         send a prompt with zcode --prompt
  @<path> in a prompt          auto-attach existing files via --attach
  ! <cmd>                      run a local shell command
  /goal <text>                 forward to ZCode goal handling
  /goal replace <text>         replace current goal through ZCode
  /skill <name> <task>         force a ZCode skill for a prompt
  /skills [list]               list ZCode skills through zcode skills list
  /login                       interactive sign-in through the zcode CLI
  /logout                      sign out through the zcode CLI
  /auth                        show local auth status (env key / credentials)
  /status                      session, auth, and MCP overview
  /mcp list                    list project and user MCP servers
  /mcp config                  print MCP config paths
  /mcp add <name> <cmd> [args] add a stdio MCP server
  /mcp add --transport http|sse <name> <url>
                               add a remote MCP server
  /mcp add-json <name> <json>  add a server from raw JSON
  /mcp get <name>              show one server as JSON
  /mcp enable|disable <name>   toggle a server without removing it
  /mcp remove <name>           remove an MCP server
  /mcp ... --scope user        target ~/.config/zcode/mcp.json instead
  /mcp status                  forward to ZCode as /mcp status
  /diff [args]                 show git diff with coloring (e.g. /diff --staged)
  /ide [path]                  open cwd (or path) in your IDE; override with
                               ZCODE_TUI_IDE_CMD
  /mode [build|edit|plan|yolo] show or switch permission mode
  /resume [sess_id]            resume latest (bare) or a specific session
  /new                         start a fresh session; context resets
  /editor                      edit current prompt in $VISUAL or $EDITOR
  /clear                       clear this screen
  /exit                        quit

keys:
  Ctrl+P                       command palette
  Ctrl+X then p/h/e/x/u/q      leader shortcuts
  Tab / Up / Down              navigate and accept suggestions
  Shift+Tab                    cycle permission mode
  Enter                        accept selected suggestion or send
  Left/Right Home/End          move the input cursor
  Ctrl+A / Ctrl+E              jump to start / end of input
  Ctrl+G                       edit prompt externally
  Ctrl+J                       insert newline
  Esc                          close popups / cancel running job
"#
}

// ---- markdown rendering (transcript) ----------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanRole {
    Normal,
    Strong,
    Emph,
    Code,
    Link,
    Marker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledSpan {
    pub text: String,
    pub role: SpanRole,
    /// Syntax-highlight color for code spans; None uses the theme default.
    pub color: Option<(u8, u8, u8)>,
}

impl StyledSpan {
    fn new(text: String, role: SpanRole) -> Self {
        Self {
            text,
            role,
            color: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MdLineKind {
    Text,
    Heading,
    CodeBlock,
    /// Fenced code block with `diff` language: colored per diff_line_role.
    DiffBlock,
    Quote,
    Rule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StyledLine {
    pub spans: Vec<StyledSpan>,
    pub kind: MdLineKind,
}

fn syntax_set() -> &'static syntect::parsing::SyntaxSet {
    static SET: std::sync::OnceLock<syntect::parsing::SyntaxSet> = std::sync::OnceLock::new();
    SET.get_or_init(syntect::parsing::SyntaxSet::load_defaults_newlines)
}

fn code_theme() -> &'static syntect::highlighting::Theme {
    static THEME: std::sync::OnceLock<syntect::highlighting::Theme> = std::sync::OnceLock::new();
    THEME.get_or_init(|| {
        let mut themes = syntect::highlighting::ThemeSet::load_defaults();
        themes
            .themes
            .remove("base16-ocean.dark")
            .or_else(|| themes.themes.pop_first().map(|(_, theme)| theme))
            .unwrap_or_default()
    })
}

/// Render one fenced code block: `diff` fences get DiffBlock lines, known
/// languages are syntax-highlighted with syntect, everything else is plain.
fn render_code_block(lang: &str, code: &str, out: &mut Vec<StyledLine>) {
    if lang.eq_ignore_ascii_case("diff") {
        for line in code.lines() {
            out.push(StyledLine {
                spans: vec![StyledSpan::new(line.to_string(), SpanRole::Normal)],
                kind: MdLineKind::DiffBlock,
            });
        }
        return;
    }

    let set = syntax_set();
    let syntax = if lang.is_empty() {
        None
    } else {
        set.find_syntax_by_token(lang)
    };
    let Some(syntax) = syntax else {
        for line in code.lines() {
            out.push(StyledLine {
                spans: vec![StyledSpan::new(line.to_string(), SpanRole::Code)],
                kind: MdLineKind::CodeBlock,
            });
        }
        return;
    };

    let mut highlighter = syntect::easy::HighlightLines::new(syntax, code_theme());
    for line in code.lines() {
        let with_newline = format!("{line}\n");
        let spans = match highlighter.highlight_line(&with_newline, set) {
            Ok(ranges) => ranges
                .into_iter()
                .filter_map(|(style, piece)| {
                    let piece = piece.trim_end_matches('\n');
                    if piece.is_empty() {
                        return None;
                    }
                    let fg = style.foreground;
                    Some(StyledSpan {
                        text: piece.to_string(),
                        role: SpanRole::Code,
                        color: Some((fg.r, fg.g, fg.b)),
                    })
                })
                .collect(),
            Err(_) => vec![StyledSpan::new(line.to_string(), SpanRole::Code)],
        };
        out.push(StyledLine {
            spans,
            kind: MdLineKind::CodeBlock,
        });
    }
}

/// Render markdown into styled terminal lines, wrapped to `width` columns
/// (0 disables wrapping). Covers the subset coding agents emit: headings,
/// emphasis, inline code, fenced code blocks (with syntax highlighting and
/// colored `diff` fences), lists, quotes, tables, rules, links.
pub fn markdown_lines(input: &str, width: usize) -> Vec<StyledLine> {
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

    let mut out: Vec<StyledLine> = Vec::new();
    let mut current: Vec<StyledSpan> = Vec::new();
    let mut kind = MdLineKind::Text;
    let mut strong = 0usize;
    let mut emph = 0usize;
    let mut link = 0usize;
    let mut quote = 0usize;
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buffer = String::new();
    let mut in_table = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut table_row: Vec<String> = Vec::new();
    let mut table_cell = String::new();
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    let mut pending_marker: Option<String> = None;

    fn flush(
        out: &mut Vec<StyledLine>,
        current: &mut Vec<StyledSpan>,
        kind: MdLineKind,
        quote: usize,
        width: usize,
    ) {
        if current.is_empty() {
            return;
        }
        let spans = std::mem::take(current);
        let kind = if quote > 0 { MdLineKind::Quote } else { kind };
        for wrapped in wrap_spans(spans, width) {
            out.push(StyledLine {
                spans: wrapped,
                kind,
            });
        }
    }

    let role_for = |strong: usize, emph: usize, link: usize| -> SpanRole {
        if link > 0 {
            SpanRole::Link
        } else if strong > 0 {
            SpanRole::Strong
        } else if emph > 0 {
            SpanRole::Emph
        } else {
            SpanRole::Normal
        }
    };

    let parser = Parser::new_ext(input, Options::ENABLE_TABLES);
    for event in parser {
        match event {
            Event::Start(Tag::Table(_)) => {
                flush(&mut out, &mut current, kind, quote, width);
                in_table = true;
                table_rows.clear();
            }
            Event::End(TagEnd::Table) => {
                emit_table(&table_rows, &mut out);
                in_table = false;
            }
            Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => {
                table_row.clear();
            }
            Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => {
                table_rows.push(std::mem::take(&mut table_row));
            }
            Event::Start(Tag::TableCell) => table_cell.clear(),
            Event::End(TagEnd::TableCell) => table_row.push(std::mem::take(&mut table_cell)),
            Event::Start(Tag::Heading { .. }) => {
                flush(&mut out, &mut current, kind, quote, width);
                kind = MdLineKind::Heading;
            }
            Event::End(TagEnd::Heading(_)) => {
                flush(&mut out, &mut current, kind, quote, width);
                kind = MdLineKind::Text;
            }
            Event::Start(Tag::Strong) => strong += 1,
            Event::End(TagEnd::Strong) => strong = strong.saturating_sub(1),
            Event::Start(Tag::Emphasis) => emph += 1,
            Event::End(TagEnd::Emphasis) => emph = emph.saturating_sub(1),
            Event::Start(Tag::Link { .. }) => link += 1,
            Event::End(TagEnd::Link) => link = link.saturating_sub(1),
            Event::Start(Tag::BlockQuote(_)) => quote += 1,
            Event::End(TagEnd::BlockQuote(_)) => quote = quote.saturating_sub(1),
            Event::Start(Tag::CodeBlock(block)) => {
                flush(&mut out, &mut current, kind, quote, width);
                in_code_block = true;
                code_buffer.clear();
                code_lang = match block {
                    CodeBlockKind::Fenced(lang) => lang.trim().to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                if !code_lang.is_empty() {
                    out.push(StyledLine {
                        spans: vec![StyledSpan::new(format!("· {code_lang}"), SpanRole::Marker)],
                        kind: MdLineKind::CodeBlock,
                    });
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                render_code_block(&code_lang, &code_buffer, &mut out);
                in_code_block = false;
            }
            Event::Start(Tag::List(start)) => list_stack.push(start),
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
            }
            Event::Start(Tag::Item) => {
                flush(&mut out, &mut current, kind, quote, width);
                let marker = match list_stack.last_mut() {
                    Some(Some(number)) => {
                        let text = format!("{number}. ");
                        *number += 1;
                        text
                    }
                    _ => "• ".to_string(),
                };
                pending_marker = Some(marker);
            }
            Event::End(TagEnd::Item) => {
                flush(&mut out, &mut current, kind, quote, width);
            }
            Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => {
                flush(&mut out, &mut current, kind, quote, width);
            }
            Event::Rule => {
                flush(&mut out, &mut current, kind, quote, width);
                out.push(StyledLine {
                    spans: vec![StyledSpan::new(
                        "─".repeat(width.clamp(8, 40)),
                        SpanRole::Marker,
                    )],
                    kind: MdLineKind::Rule,
                });
            }
            Event::Text(text) => {
                if in_table {
                    table_cell.push_str(&text);
                } else if in_code_block {
                    code_buffer.push_str(&text);
                } else {
                    if let Some(marker) = pending_marker.take() {
                        current.push(StyledSpan::new(marker, SpanRole::Marker));
                    }
                    current.push(StyledSpan::new(
                        text.to_string(),
                        role_for(strong, emph, link),
                    ));
                }
            }
            Event::Code(text) => {
                if in_table {
                    table_cell.push_str(&text);
                    continue;
                }
                if let Some(marker) = pending_marker.take() {
                    current.push(StyledSpan::new(marker, SpanRole::Marker));
                }
                current.push(StyledSpan::new(text.to_string(), SpanRole::Code));
            }
            Event::SoftBreak | Event::HardBreak => {
                flush(&mut out, &mut current, kind, quote, width);
            }
            _ => {}
        }
    }
    flush(&mut out, &mut current, kind, quote, width);
    out
}

/// Render a parsed table as aligned columns: bold header, rule, plain rows.
fn emit_table(rows: &[Vec<String>], out: &mut Vec<StyledLine>) {
    if rows.is_empty() {
        return;
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0usize; columns];
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.as_str().width()).min(40);
        }
    }
    for (row_index, row) in rows.iter().enumerate() {
        let mut spans = Vec::new();
        for (col, target) in widths.iter().enumerate() {
            let cell = row.get(col).map(String::as_str).unwrap_or("");
            let role = if row_index == 0 {
                SpanRole::Strong
            } else {
                SpanRole::Normal
            };
            spans.push(StyledSpan::new(pad_display(cell, *target), role));
            if col + 1 < columns {
                spans.push(StyledSpan::new("  ".to_string(), SpanRole::Normal));
            }
        }
        out.push(StyledLine {
            spans,
            kind: MdLineKind::Text,
        });
        if row_index == 0 {
            let rule = widths
                .iter()
                .map(|target| "─".repeat(*target))
                .collect::<Vec<_>>()
                .join("  ");
            out.push(StyledLine {
                spans: vec![StyledSpan::new(rule, SpanRole::Marker)],
                kind: MdLineKind::Text,
            });
        }
    }
}

/// Pad (or truncate with an ellipsis) to exactly `target` display columns.
fn pad_display(text: &str, target: usize) -> String {
    let text_width = text.width();
    if text_width <= target {
        return format!("{text}{}", " ".repeat(target - text_width));
    }
    let mut kept = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > target.saturating_sub(1) {
            break;
        }
        kept.push(ch);
        used += ch_width;
    }
    format!("{kept}…{}", " ".repeat(target.saturating_sub(used + 1)))
}

fn wrap_spans(spans: Vec<StyledSpan>, width: usize) -> Vec<Vec<StyledSpan>> {
    if width == 0 {
        return vec![spans];
    }
    let mut lines: Vec<Vec<StyledSpan>> = Vec::new();
    let mut line: Vec<StyledSpan> = Vec::new();
    let mut used = 0usize;
    for span in spans {
        let mut chunk = String::new();
        for ch in span.text.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + ch_width > width && used > 0 {
                if !chunk.is_empty() {
                    line.push(StyledSpan {
                        text: std::mem::take(&mut chunk),
                        role: span.role,
                        color: span.color,
                    });
                }
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            chunk.push(ch);
            used += ch_width;
        }
        if !chunk.is_empty() {
            line.push(StyledSpan {
                text: chunk,
                role: span.role,
                color: span.color,
            });
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

/// Wrap text to `width` display columns, CJK-aware (中文 counts as 2).
/// Preserves existing newlines; width 0 disables wrapping.
pub fn wrap_display(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    if width == 0 {
        return text.lines().map(str::to_string).collect();
    }
    let mut lines = Vec::new();
    for raw in text.lines() {
        let mut line = String::new();
        let mut used = 0usize;
        for ch in raw.chars() {
            let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + ch_width > width && used > 0 {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            line.push(ch);
            used += ch_width;
        }
        lines.push(line);
    }
    lines
}

// ---- git diff ----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffRole {
    Add,
    Remove,
    Hunk,
    Meta,
    Context,
}

pub fn diff_line_role(line: &str) -> DiffRole {
    if line.starts_with("diff --git")
        || line.starts_with("index ")
        || line.starts_with("--- ")
        || line.starts_with("+++ ")
        || line.starts_with("new file")
        || line.starts_with("deleted file")
        || line.starts_with("rename ")
        || line.starts_with("similarity ")
        || line.starts_with("Binary files")
    {
        DiffRole::Meta
    } else if line.starts_with("@@") {
        DiffRole::Hunk
    } else if line.starts_with('+') {
        DiffRole::Add
    } else if line.starts_with('-') {
        DiffRole::Remove
    } else {
        DiffRole::Context
    }
}

pub fn git_diff_command(cwd: &Path, args: &[String]) -> Vec<String> {
    let mut command = vec![
        "git".to_string(),
        "-C".to_string(),
        cwd.display().to_string(),
        "--no-pager".to_string(),
        "diff".to_string(),
        "--no-color".to_string(),
    ];
    command.extend(args.iter().cloned());
    command
}

// ---- streamed agent events (zcode --json) ------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    ToolUse { name: String, detail: String },
    ToolResult { detail: String },
    Text(String),
    Meta(String),
}

/// Recognize one line of machine-readable agent output (JSONL). Returns
/// None for plain text so callers fall back to raw rendering.
pub fn parse_stream_event(line: &str) -> Option<StreamEvent> {
    let trimmed = line.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    let object = value.as_object()?;
    let event_type = object.get("type").and_then(|t| t.as_str())?;

    let text_of = |keys: &[&str]| -> Option<String> {
        for key in keys {
            match object.get(*key) {
                Some(serde_json::Value::String(text)) => return Some(text.clone()),
                Some(serde_json::Value::Array(parts)) => {
                    let joined: String = parts
                        .iter()
                        .filter_map(|part| {
                            part.get("text")
                                .and_then(|t| t.as_str())
                                .or_else(|| part.as_str())
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    if !joined.is_empty() {
                        return Some(joined);
                    }
                }
                _ => {}
            }
        }
        None
    };

    if event_type.contains("tool_use") || event_type == "tool_call" {
        let name = text_of(&["name", "tool", "tool_name"]).unwrap_or_else(|| "tool".to_string());
        let detail = object
            .get("input")
            .or_else(|| object.get("arguments"))
            .or_else(|| object.get("args"))
            .map(|v| compact_json(v, 80))
            .unwrap_or_default();
        return Some(StreamEvent::ToolUse { name, detail });
    }
    if event_type.contains("tool_result") || event_type.contains("tool_output") {
        let detail = text_of(&["content", "output", "result"])
            .map(|text| truncate_chars(&text, 80))
            .unwrap_or_else(|| "done".to_string());
        return Some(StreamEvent::ToolResult { detail });
    }
    if matches!(event_type, "text" | "message" | "assistant" | "completion") {
        if let Some(text) = text_of(&["text", "content", "message"]) {
            return Some(StreamEvent::Text(text));
        }
    }
    Some(StreamEvent::Meta(event_type.to_string()))
}

fn compact_json(value: &serde_json::Value, max: usize) -> String {
    let rendered = match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    truncate_chars(&rendered.replace('\n', " "), max)
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max).collect();
    format!("{kept}…")
}

pub fn shorten_home(path: &str, home: Option<&str>) -> String {
    if let Some(home) = home.filter(|value| !value.is_empty()) {
        if let Some(rest) = path.strip_prefix(home) {
            return format!("~{rest}");
        }
    }
    path.to_string()
}

// ---- official update check ---------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateFeed {
    pub version: String,
    pub deb_file: Option<String>,
    pub release_name: Option<String>,
}

/// Extract the electron-updater feed URL from an `app-update.yml` and point
/// it at `latest-linux.yml`, the same file the official desktop app polls.
pub fn parse_update_feed_url(app_update_yml: &str) -> Option<String> {
    for line in app_update_yml.lines() {
        if let Some(rest) = line.trim().strip_prefix("url:") {
            let url = rest.trim().trim_matches(|c| c == '\'' || c == '"');
            if url.starts_with("http") {
                return Some(format!("{}/latest-linux.yml", url.trim_end_matches('/')));
            }
        }
    }
    None
}

pub fn parse_update_feed(yaml: &str) -> Option<UpdateFeed> {
    let mut version = None;
    let mut deb_file = None;
    let mut release_name = None;
    for line in yaml.lines() {
        let trimmed = line.trim();
        if version.is_none() {
            if let Some(value) = trimmed.strip_prefix("version:") {
                version = Some(value.trim().to_string());
            }
        }
        if deb_file.is_none() {
            let value = trimmed
                .strip_prefix("- url:")
                .or_else(|| trimmed.strip_prefix("url:"));
            if let Some(value) = value {
                let value = value.trim();
                if value.ends_with(".deb") {
                    deb_file = Some(value.to_string());
                }
            }
        }
        if release_name.is_none() {
            if let Some(value) = trimmed.strip_prefix("releaseName:") {
                release_name = Some(value.trim().to_string());
            }
        }
    }
    version.map(|version| UpdateFeed {
        version,
        deb_file,
        release_name,
    })
}

/// Numeric segment-wise version comparison: `3.2.5` > `3.2.3`, `3.10` > `3.9`.
pub fn is_newer_version(latest: &str, installed: &str) -> bool {
    fn segments(version: &str) -> Vec<u64> {
        version
            .split(|c: char| !c.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    }
    let latest = segments(latest);
    let installed = segments(installed);
    for index in 0..latest.len().max(installed.len()) {
        let a = latest.get(index).copied().unwrap_or(0);
        let b = installed.get(index).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

// ---- /ide ----------------------------------------------------------------

/// IDE launchers probed, in order, when ZCODE_TUI_IDE_CMD is not set.
pub const IDE_CANDIDATES: &[&str] = &["code", "cursor", "zed", "subl", "idea"];

pub fn ide_command(override_command: Option<&str>, path: &str) -> Result<Vec<String>> {
    if let Some(raw) = override_command {
        let mut parts = shell_words::split(raw)
            .with_context(|| format!("failed to parse IDE command override: {raw}"))?;
        if parts.is_empty() {
            return Err(anyhow!("IDE command override is empty"));
        }
        parts.push(path.to_string());
        return Ok(parts);
    }
    for candidate in IDE_CANDIDATES {
        if find_in_path(candidate) {
            return Ok(vec![candidate.to_string(), path.to_string()]);
        }
    }
    Err(anyhow!(
        "no IDE launcher found in PATH (tried {}); set ZCODE_TUI_IDE_CMD",
        IDE_CANDIDATES.join(", ")
    ))
}

fn find_in_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(name).is_file())
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
