use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use rusqlite::{Connection, OpenFlags};
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
    pub tool_allowlist: Vec<String>,
    pub tool_denylist: Vec<String>,
    /// ZCode 3.5.3 Browser Use. The fallback TUI routes these prompts through
    /// the official classic CLI because the app-server strict schemas do not
    /// expose a browser runtime switch.
    pub browser_use: Option<String>,
    pub browser_executable: Option<String>,
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
    /// Copy the last assistant reply to the system clipboard via OSC52.
    CopyLastReply,
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
    /// `~/.zcode/cli/config.json` exists — the kernel can actually run.
    /// If an API key env var is also set it is carried along (masked).
    Configured {
        config_path: PathBuf,
        env_key: Option<(String, String)>,
    },
    /// Auth-ish evidence exists (env key or credential file) but the kernel
    /// hard-requires the model config file, so prompts will still fail.
    Partial {
        evidence: String,
    },
    None,
}

impl AuthStatus {
    pub fn short_label(&self) -> String {
        match self {
            Self::Configured { .. } => "config.json".to_string(),
            Self::Partial { .. } => "partial".to_string(),
            Self::None => "none".to_string(),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::Configured {
                config_path,
                env_key,
            } => match env_key {
                Some((variable, masked)) => format!(
                    "configured via {} (plus ${variable} {masked})",
                    config_path.display()
                ),
                Option::None => format!("configured via {}", config_path.display()),
            },
            Self::Partial { evidence } => format!(
                "partially configured: {evidence} found, but the kernel still needs \
                 ~/.zcode/cli/config.json — run `zcode login bigmodel-coding-plan-api-key <key>` \
                 (or zai-coding-plan-api-key) to finish"
            ),
            Self::None => {
                "not configured: no model config, API key env var, or credential file found; run /login"
                    .to_string()
            }
        }
    }

    pub fn is_configured(&self) -> bool {
        matches!(self, Self::Configured { .. })
    }
}

/// Environment variables checked, in priority order, for an API key.
pub const AUTH_ENV_VARS: &[&str] = &["ZCODE_API_KEY", "ZHIPUAI_API_KEY", "ZAI_API_KEY"];

/// The kernel refuses to run without this file (verified 0.15.0), no matter
/// which env vars are set — it is the source of truth for "configured".
pub fn kernel_config_path_from(home: &Path) -> PathBuf {
    home.join(".zcode").join("cli").join("config.json")
}

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
    let env_key = AUTH_ENV_VARS.iter().find_map(|variable| {
        env_lookup(variable).and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| (variable.to_string(), mask_secret(trimmed)))
        })
    });
    if let Some(home) = home {
        let config_path = kernel_config_path_from(home);
        if config_path.is_file() {
            return AuthStatus::Configured {
                config_path,
                env_key,
            };
        }
    }
    if let Some((variable, masked)) = env_key {
        return AuthStatus::Partial {
            evidence: format!("${variable} ({masked})"),
        };
    }
    if let Some(home) = home {
        for path in auth_credential_candidates(home) {
            if path.exists() {
                return AuthStatus::Partial {
                    evidence: format!("credential file {}", path.display()),
                };
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

/// True when no graphical session is reachable (`DISPLAY` and
/// `WAYLAND_DISPLAY` both unset/empty) — `zcode login` would try to open a
/// browser and fail, so /login appends `--no-browser` to print the OAuth URL.
pub fn env_is_headless<F>(env_lookup: F) -> bool
where
    F: Fn(&str) -> Option<String>,
{
    ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .all(|variable| env_lookup(variable).is_none_or(|value| value.trim().is_empty()))
}

pub fn login_command(
    zcode_bin: &str,
    override_command: Option<&str>,
    headless: bool,
) -> Result<Vec<String>> {
    let mut command = build_auth_command(zcode_bin, "login", override_command)?;
    // Only inject into the default command; an explicit override is verbatim.
    if override_command.is_none() && headless {
        command.push("--no-browser".to_string());
    }
    Ok(command)
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
            "--permission-mode" => {
                config.mode = Some(normalize_permission_mode(&next_value(
                    &mut iter,
                    "--permission-mode",
                )?)?)
            }
            "--resume" => config.resume = Some(next_value(&mut iter, "--resume")?),
            "--locale" => config.locale = Some(next_value(&mut iter, "--locale")?),
            "--attach" => config.attach.push(next_value(&mut iter, "--attach")?),
            "--allowed-tools" => append_tool_values(
                &mut config.tool_allowlist,
                next_tool_values(&mut iter, "--allowed-tools")?,
            ),
            "--disallowed-tools" | "--disallowedTools" => append_tool_values(
                &mut config.tool_denylist,
                next_tool_values(&mut iter, arg.as_str())?,
            ),
            "--browser-use" => {
                config.browser_use = Some(normalize_browser_use(&next_value(
                    &mut iter,
                    "--browser-use",
                )?)?)
            }
            "--browser-executable" => {
                config.browser_executable = Some(next_value(&mut iter, "--browser-executable")?)
            }
            "--target" => target = Some(next_value(&mut iter, "--target")?),
            _ if arg.starts_with("--cwd=") => config.cwd = Some(split_equals(&arg)),
            _ if arg.starts_with("--mode=") => config.mode = Some(split_equals(&arg)),
            _ if arg.starts_with("--permission-mode=") => {
                config.mode = Some(normalize_permission_mode(&split_equals(&arg))?)
            }
            _ if arg.starts_with("--resume=") => config.resume = Some(split_equals(&arg)),
            _ if arg.starts_with("--locale=") => config.locale = Some(split_equals(&arg)),
            _ if arg.starts_with("--attach=") => config.attach.push(split_equals(&arg)),
            _ if arg.starts_with("--allowed-tools=") => append_tool_values(
                &mut config.tool_allowlist,
                parse_tool_values(&split_equals(&arg), "--allowed-tools")?,
            ),
            _ if arg.starts_with("--disallowed-tools=")
                || arg.starts_with("--disallowedTools=") =>
            {
                append_tool_values(
                    &mut config.tool_denylist,
                    parse_tool_values(&split_equals(&arg), "--disallowed-tools")?,
                )
            }
            _ if arg.starts_with("--browser-use=") => {
                config.browser_use = Some(normalize_browser_use(&split_equals(&arg))?)
            }
            _ if arg.starts_with("--browser-executable=") => {
                let value = split_equals(&arg);
                if value.is_empty() {
                    return Err(anyhow!("--browser-executable requires a value"));
                }
                config.browser_executable = Some(value);
            }
            _ if arg.starts_with("--target=") => target = Some(split_equals(&arg)),
            _ => config.passthrough.push(arg),
        }
    }

    if config.browser_executable.is_some() && config.browser_use.as_deref() != Some("headless") {
        return Err(anyhow!(
            "--browser-executable requires --browser-use headless"
        ));
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

fn normalize_permission_mode(value: &str) -> Result<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "default" => Ok("build".to_string()),
        "build" | "edit" | "plan" | "yolo" | "auto" => Ok(value.trim().to_ascii_lowercase()),
        _ => Err(anyhow!(
            "--permission-mode expects default, build, edit, plan, yolo, or auto"
        )),
    }
}

fn normalize_browser_use(value: &str) -> Result<String> {
    match value.trim() {
        "headless" => Ok("headless".to_string()),
        "" => Err(anyhow!("--browser-use requires a value")),
        other => Err(anyhow!(
            "unsupported --browser-use mode: {other} (ZCode 3.5.3 supports headless)"
        )),
    }
}

fn parse_tool_values(value: &str, option: &str) -> Result<Vec<String>> {
    let values: Vec<String> = value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    if values.is_empty() {
        Err(anyhow!("{option} requires at least one tool rule"))
    } else {
        Ok(values)
    }
}

fn next_tool_values<I>(iter: &mut std::iter::Peekable<I>, option: &str) -> Result<Vec<String>>
where
    I: Iterator<Item = String>,
{
    let mut values = Vec::new();
    while iter.peek().is_some_and(|value| !value.starts_with('-')) {
        if let Some(value) = iter.next() {
            values.extend(parse_tool_values(&value, option)?);
        }
    }
    if values.is_empty() {
        Err(anyhow!("{option} requires at least one tool rule"))
    } else {
        Ok(values)
    }
}

fn append_tool_values(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target.contains(&value) {
            target.push(value);
        }
    }
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
    if !config.tool_allowlist.is_empty() {
        command.push("--allowed-tools".to_string());
        command.extend(config.tool_allowlist.iter().cloned());
    }
    if !config.tool_denylist.is_empty() {
        command.push("--disallowed-tools".to_string());
        command.extend(config.tool_denylist.iter().cloned());
    }
    if let Some(mode) = &config.browser_use {
        command.extend(["--browser-use".to_string(), mode.clone()]);
    }
    if let Some(executable) = &config.browser_executable {
        command.extend(["--browser-executable".to_string(), executable.clone()]);
    }
    command.extend(config.passthrough.iter().cloned());
    // The end-of-run summary object (response/sessionId/usage/contextUsed)
    // is the authoritative result; parse failures fall back to plain text.
    if !command.iter().any(|arg| arg == "--json") {
        command.push("--json".to_string());
    }
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
        | "sessions" | "mode" | "resume" | "new" | "model" | "think" | "compact" | "usage"
        | "update" | "copy" | "rewind" => Ok(InputAction::Local(parts)),
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
            command: "/sessions",
            summary: "pick a recent kernel session to resume",
            route: "local",
        },
        CommandSpec {
            command: "/new",
            summary: "start a fresh session; context resets",
            route: "local",
        },
        CommandSpec {
            command: "/compact",
            summary: "compact the session context in place (keeps the session)",
            route: "local",
        },
        CommandSpec {
            command: "/model",
            summary: "switch the session model (app-server streaming path)",
            route: "local",
        },
        CommandSpec {
            command: "/think",
            summary: "cycle the thought level (app-server streaming path)",
            route: "local",
        },
        CommandSpec {
            command: "/usage",
            summary: "session + 7d/30d token usage (app-server streaming path)",
            route: "local",
        },
        CommandSpec {
            command: "/rewind",
            summary: "rewind files/conversation to a checkpoint (app-server streaming path)",
            route: "local",
        },
        CommandSpec {
            command: "/update",
            summary: "update the ZCode kernel from the official feed",
            route: "local",
        },
        CommandSpec {
            command: "/copy",
            summary: "copy the last assistant reply via OSC52",
            route: "local",
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
        'y' => Some(LeaderAction::CopyLastReply),
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
    /// An output line tagged with its origin stream. stdout and stderr are
    /// read by independent threads, so their lines interleave arbitrarily —
    /// consumers that parse structured stdout (the `--prompt --json` summary)
    /// MUST filter on `stderr`, or a stray kernel warning lands mid-JSON and
    /// breaks the parse (seen as raw-summary leak + lost watermark).
    Line {
        text: String,
        stderr: bool,
    },
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
        spawn_line_reader(stdout, sender.clone(), false);
        streams += 1;
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_line_reader(stderr, sender.clone(), true);
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

fn spawn_line_reader<R>(reader: R, sender: Sender<JobEvent>, stderr: bool)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let buffered = BufReader::new(reader);
        for line in buffered.lines() {
            let Ok(line) = line else {
                break;
            };
            let event = JobEvent::Line {
                text: strip_ansi(&line),
                stderr,
            };
            if sender.send(event).is_err() {
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
launch options:
  --allowed-tools <tools...>   allow only these tools for the session
  --disallowed-tools <tools...>
                               deny these tools for the session
  --permission-mode <mode>     legacy alias for --mode (default = build)
  --browser-use headless       Browser Use via official classic --prompt path
  --browser-executable <path>  browser binary (requires --browser-use headless)

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
  /mode [build|edit|plan|yolo] show or switch permission mode; applies live
                               on the app-server streaming path
  /model                       switch the session model (app-server path)
  /think                       cycle the thought level (app-server path)
  /compact                     compact the session context in place
  /usage [7d|30d]              show session and period token usage
  /rewind                      rewind files/conversation to a checkpoint
                               (app-server path; Enter previews then applies)
  /update                      update the ZCode kernel from the official feed
  /copy                        copy the last assistant reply to the system
                               clipboard (OSC52; tmux needs set-clipboard on)
  /resume [sess_id]            resume latest (bare) or a specific session
  /sessions                    pick a recent session from a list
  /new                         start a fresh session; context resets
  /editor                      edit current prompt in $VISUAL or $EDITOR
  /clear                       clear this screen
  /exit                        quit

keys:
  Ctrl+P                       command palette
  Ctrl+X then p/h/e/x/u/y/q    leader shortcuts (y copies the last reply)
  Tab / Up / Down              navigate and accept suggestions
  Shift+Tab                    cycle permission mode
  Enter                        accept selected suggestion or send; plain text
                               sent mid-turn steers the running answer
                               (app-server path; commands still queue)
  Left/Right Home/End          move the input cursor
  Ctrl+A / Ctrl+E              jump to start / end of input
  Ctrl+G                       edit prompt externally
  Ctrl+J                       insert newline
  Ctrl+R                       reverse-search input history
  Ctrl+O                       expand / fold the last long output
  Mouse wheel                  scroll the transcript (hold Shift to select
                               text; ZCODE_TUI_NO_MOUSE=1 disables capture)
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

/// A dim, right-aligned line-number gutter span, like a code editor's.
fn gutter_span(line_no: usize, width: usize) -> StyledSpan {
    StyledSpan {
        text: format!("{line_no:>width$} "),
        role: SpanRole::Marker,
        color: None,
    }
}

/// Render one fenced code block: `diff` fences get DiffBlock lines (kept raw
/// for their +/- coloring), everything else gets a line-number gutter plus
/// syntect highlighting when the language is known.
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

    // Gutter width tracks the largest line number in this block.
    let total = code.lines().count().max(1);
    let gutter_width = total.to_string().len();

    let set = syntax_set();
    let syntax = if lang.is_empty() {
        None
    } else {
        set.find_syntax_by_token(lang)
    };
    let Some(syntax) = syntax else {
        for (index, line) in code.lines().enumerate() {
            out.push(StyledLine {
                spans: vec![
                    gutter_span(index + 1, gutter_width),
                    StyledSpan::new(line.to_string(), SpanRole::Code),
                ],
                kind: MdLineKind::CodeBlock,
            });
        }
        return;
    };

    let mut highlighter = syntect::easy::HighlightLines::new(syntax, code_theme());
    for (index, line) in code.lines().enumerate() {
        let with_newline = format!("{line}\n");
        let mut spans = vec![gutter_span(index + 1, gutter_width)];
        match highlighter.highlight_line(&with_newline, set) {
            Ok(ranges) => spans.extend(ranges.into_iter().filter_map(|(style, piece)| {
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
            })),
            Err(_) => spans.push(StyledSpan::new(line.to_string(), SpanRole::Code)),
        }
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
                // Language tag as a header row of the panel; the accent left
                // rule (drawn by the renderer) is the block marker now, so the
                // old `· ` prefix is dropped. Diff fences color their own lines,
                // so they get no banded header.
                if !code_lang.is_empty() && !code_lang.eq_ignore_ascii_case("diff") {
                    out.push(StyledLine {
                        spans: vec![StyledSpan::new(code_lang.clone(), SpanRole::Marker)],
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

pub const OFFICIAL_ZCODE_LINUX_UPDATE_FEED: &str =
    "https://cdn-zcode.z.ai/zcode/electron/releases/update/linux/x64/latest-linux.yml";

fn zcode_app_has_kernel(app_dir: &Path) -> bool {
    app_dir.join("resources/glm/zcode.cjs").is_file()
}

/// Extract `<version>` from the documented rootless layout
/// `~/.local/opt/zcode/<version>/opt/ZCode`.
pub fn zcode_app_version_from_path(app_dir: &Path) -> Option<String> {
    if app_dir.file_name()?.to_str()? != "ZCode" {
        return None;
    }
    let opt = app_dir.parent()?;
    if opt.file_name()?.to_str()? != "opt" {
        return None;
    }
    let version = opt.parent()?.file_name()?.to_str()?.trim();
    version
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit())
        .then(|| version.to_string())
}

/// Numeric segment comparison used for package directories and update feeds.
/// Non-numeric separators/suffixes are ignored consistently with the original
/// `is_newer_version` implementation.
pub fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    fn segments(version: &str) -> Vec<u64> {
        version
            .split(|c: char| !c.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    }
    let left = segments(left);
    let right = segments(right);
    for index in 0..left.len().max(right.len()) {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        match a.cmp(&b) {
            std::cmp::Ordering::Equal => {}
            ordering => return ordering,
        }
    }
    std::cmp::Ordering::Equal
}

/// Resolve the application directory used by the wrapper/TUI. The system
/// directory is an argument so the precedence can be tested without depending
/// on the host's real `/opt/ZCode`.
pub fn discover_zcode_app_dir(
    explicit: Option<&Path>,
    system_dir: Option<&Path>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(app_dir) = explicit.filter(|path| zcode_app_has_kernel(path)) {
        return Some(app_dir.to_path_buf());
    }
    if let Some(app_dir) = system_dir.filter(|path| zcode_app_has_kernel(path)) {
        return Some(app_dir.to_path_buf());
    }
    let versions_root = home?.join(".local/opt/zcode");
    let entries = fs::read_dir(versions_root).ok()?;
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("opt/ZCode"))
        .filter(|path| zcode_app_has_kernel(path))
        .max_by(|left, right| {
            let left_version = zcode_app_version_from_path(left).unwrap_or_default();
            let right_version = zcode_app_version_from_path(right).unwrap_or_default();
            compare_versions(&left_version, &right_version)
                .then_with(|| left.as_os_str().cmp(right.as_os_str()))
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateFeed {
    pub version: String,
    pub deb_file: Option<String>,
    /// The deb entry's sha512, base64-encoded as published in the feed —
    /// verified before /update ever hands the file to dpkg.
    pub deb_sha512: Option<String>,
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

fn normalize_update_feed_url(value: &str) -> Option<String> {
    let value = value.trim().trim_matches(|c| c == '\'' || c == '"');
    if !(value.starts_with("http://") || value.starts_with("https://")) {
        return None;
    }
    if value.ends_with(".yml") || value.ends_with(".yaml") {
        Some(value.to_string())
    } else {
        Some(format!("{}/latest-linux.yml", value.trim_end_matches('/')))
    }
}

fn update_feed_is_loopback(url: &str) -> bool {
    let Some((_, authority_and_path)) = url.split_once("://") else {
        return false;
    };
    let authority = authority_and_path
        .split('/')
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default();
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    host.eq_ignore_ascii_case("localhost")
        || host == "::1"
        || host.starts_with("127.")
        || host == "0.0.0.0"
}

/// Resolve an update feed while distinguishing package metadata from an
/// explicit test/operator override. The observed 3.3.6 package carries a
/// localhost placeholder; implicit loopback metadata falls back to the
/// documented official Linux channel, while an explicit loopback override is
/// preserved for deterministic PTY tests.
pub fn select_update_feed_url(
    app_update_yml: Option<&str>,
    explicit_override: Option<&str>,
) -> Option<String> {
    if let Some(value) = explicit_override {
        return normalize_update_feed_url(value);
    }
    let url = parse_update_feed_url(app_update_yml?)?;
    if update_feed_is_loopback(&url) {
        Some(OFFICIAL_ZCODE_LINUX_UPDATE_FEED.to_string())
    } else {
        Some(url)
    }
}

pub fn parse_update_feed(yaml: &str) -> Option<UpdateFeed> {
    let mut version = None;
    let mut deb_file = None;
    let mut deb_sha512 = None;
    // Armed after the deb's `- url:` line: the NEXT `sha512:` belongs to that
    // files[] entry (electron-updater lists url/sha512/size per file).
    let mut in_deb_entry = false;
    let mut release_name = None;
    for line in yaml.lines() {
        let trimmed = line.trim();
        if version.is_none() {
            if let Some(value) = trimmed.strip_prefix("version:") {
                version = Some(value.trim().to_string());
            }
        }
        if let Some(value) = trimmed
            .strip_prefix("- url:")
            .or_else(|| trimmed.strip_prefix("url:"))
        {
            let value = value.trim();
            if deb_file.is_none() && value.ends_with(".deb") {
                deb_file = Some(value.to_string());
                in_deb_entry = true;
            } else {
                in_deb_entry = false;
            }
        }
        if in_deb_entry && deb_sha512.is_none() {
            if let Some(value) = trimmed.strip_prefix("sha512:") {
                deb_sha512 = Some(value.trim().to_string());
                in_deb_entry = false;
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
        deb_sha512,
        release_name,
    })
}

/// Turn a feed's deb entry into the URL curl should fetch. Current official
/// feeds use an absolute URL; older/test feeds use a bare filename. Keep an
/// absolute HTTP(S) URL intact, while relative entries are basenamed before
/// joining so they cannot escape the feed directory.
pub fn resolve_update_download_url(feed_base: &str, deb_entry: &str) -> Option<String> {
    let entry = deb_entry.trim();
    if entry.starts_with("https://") || entry.starts_with("http://") {
        return Some(entry.to_string());
    }
    if entry.contains("://") {
        return None;
    }
    let filename = entry.rsplit('/').next()?;
    if filename.is_empty() || filename == "." || filename == ".." {
        return None;
    }
    Some(format!("{}/{}", feed_base.trim_end_matches('/'), filename))
}

/// Numeric segment-wise version comparison: `3.2.5` > `3.2.3`, `3.10` > `3.9`.
pub fn is_newer_version(latest: &str, installed: &str) -> bool {
    compare_versions(latest, installed) == std::cmp::Ordering::Greater
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

// ---- kernel db (read-only consumer) --------------------------------------
//
// The kernel live-writes ~/.zcode/cli/db/db.sqlite during a turn (verified
// 2026-07-04: first part row lands ~1.6s in). Everything here is read-only
// and failure-tolerant: any error means "skip this tick", an unknown schema
// means every db-derived feature degrades to the pre-db behaviour.

/// Migration ids known at the time this consumer was written. The db stays
/// enabled while every id below exists (the kernel may append new ones);
/// a missing id means the schema moved under us.
pub const KNOWN_DB_MIGRATIONS: &[&str] = &[
    "0001_base_session_store",
    "0002_local_setting",
    "0003_backfill_permission_local_setting",
    "0004_session_target",
    "0005_session_target_accounting",
    "0006_input_history_attachments",
    "0007_workflow_script_runtime",
    "0008_workflow_definition_scope",
    "0009_session_title_metadata",
    "0010_usage_observability",
    "0011_session_target_summary_title",
    "0012_session_trace_id",
    "0013_session_target_active_run_accounting",
];

pub fn kernel_db_path_from(home: &Path) -> PathBuf {
    home.join(".zcode").join("cli").join("db").join("db.sqlite")
}

pub fn open_kernel_db_ro(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_millis(100))?;
    Ok(conn)
}

/// Known ids must be a subset of the actual ids; extra (newer) migrations
/// are fine. Any read error counts as unsupported.
pub fn db_schema_supported(conn: &Connection) -> bool {
    let Ok(mut stmt) = conn.prepare("SELECT id FROM schema_migration") else {
        return false;
    };
    let ids: std::collections::HashSet<String> = match stmt
        .query_map([], |row| row.get::<_, String>(0))
        .and_then(Iterator::collect)
    {
        Ok(ids) => ids,
        Err(_) => return false,
    };
    KNOWN_DB_MIGRATIONS
        .iter()
        .all(|migration| ids.contains(*migration))
}

/// Sessions are keyed by working directory; the row for a fresh prompt
/// appears at turn start (~1.6s), so polling can resolve it early.
pub fn latest_session_for_dir(conn: &Connection, directory: &str) -> Option<String> {
    conn.query_row(
        "SELECT id FROM session WHERE directory = ?1 ORDER BY time_updated DESC LIMIT 1",
        [directory],
        |row| row.get(0),
    )
    .ok()
}

/// Rowid snapshot taken before spawning a prompt job so polling only ever
/// attributes rows created by this run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DbBaseline {
    pub part_rowid: i64,
    pub tool_rowid: i64,
}

pub fn db_baseline(conn: &Connection) -> DbBaseline {
    let max_rowid = |table: &str| -> i64 {
        conn.query_row(
            &format!("SELECT COALESCE(MAX(rowid), 0) FROM {table}"),
            [],
            |row| row.get(0),
        )
        .unwrap_or(0)
    };
    DbBaseline {
        part_rowid: max_rowid("part"),
        tool_rowid: max_rowid("tool_usage"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolChipStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveToolChip {
    pub tool: String,
    pub status: ToolChipStatus,
    pub duration_ms: Option<i64>,
}

fn chip_status(status: &str, cancelled: bool) -> ToolChipStatus {
    let status = status.to_ascii_lowercase();
    if cancelled || status.contains("err") || status.contains("fail") {
        ToolChipStatus::Failed
    } else if matches!(
        status.as_str(),
        "completed" | "complete" | "done" | "success"
    ) {
        ToolChipStatus::Completed
    } else {
        ToolChipStatus::Running
    }
}

/// tool_usage rows are inserted once and updated in place, so re-reading
/// the window past the baseline each tick picks up status transitions.
pub fn live_tool_chips(
    conn: &Connection,
    session_id: &str,
    baseline: DbBaseline,
) -> Result<Vec<LiveToolChip>> {
    let mut stmt = conn.prepare(
        "SELECT tool_name, status, duration_ms, COALESCE(cancelled_by_user, 0) \
         FROM tool_usage WHERE session_id = ?1 AND rowid > ?2 ORDER BY rowid LIMIT 64",
    )?;
    let chips = stmt
        .query_map(rusqlite::params![session_id, baseline.tool_rowid], |row| {
            Ok(LiveToolChip {
                tool: row.get::<_, String>(0)?,
                status: chip_status(&row.get::<_, String>(1)?, row.get::<_, i64>(3)? != 0),
                duration_ms: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(chips)
}

/// Typed view of a `part.data` JSON blob. Unknown types map to `None` and
/// are skipped without failing the batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartEvent {
    Text(String),
    Reasoning(String),
    Tool {
        call_id: String,
        tool: String,
        status: ToolChipStatus,
    },
    StepStart,
    StepFinish,
}

pub fn parse_part_data(data: &str) -> Option<PartEvent> {
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    match value.get("type")?.as_str()? {
        "text" => Some(PartEvent::Text(value.get("text")?.as_str()?.to_string())),
        "reasoning" => Some(PartEvent::Reasoning(
            value.get("text")?.as_str()?.to_string(),
        )),
        "tool" => Some(PartEvent::Tool {
            call_id: value
                .get("callID")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            tool: value.get("tool")?.as_str()?.to_string(),
            status: chip_status(
                value
                    .pointer("/state/status")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
                false,
            ),
        }),
        "step-start" => Some(PartEvent::StepStart),
        "step-finish" => Some(PartEvent::StepFinish),
        _ => None,
    }
}

/// First line of the newest reasoning part past the baseline, for the dim
/// working-line shown while a prompt runs. Run-only: never enters the
/// transcript.
pub fn latest_reasoning(
    conn: &Connection,
    session_id: &str,
    baseline: DbBaseline,
) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT data FROM part WHERE session_id = ?1 AND rowid > ?2 \
         ORDER BY rowid DESC LIMIT 32",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![session_id, baseline.part_rowid], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for data in rows {
        if let Some(PartEvent::Reasoning(text)) = parse_part_data(&data) {
            if let Some(line) = text.lines().find(|line| !line.trim().is_empty()) {
                return Ok(Some(line.trim().to_string()));
            }
        }
    }
    Ok(None)
}

/// The newest ASSISTANT text part past the baseline — the answer forming.
/// Joins to `message` so the user's own prompt echo (a `user` text part) is
/// excluded. In multi-step turns each step's text lands progressively; a
/// single-step pure-text turn only appears at the very end (kernel writes
/// body text whole, not per token). Run-only: never enters the transcript.
pub fn latest_assistant_text(
    conn: &Connection,
    session_id: &str,
    baseline: DbBaseline,
) -> Result<Option<String>> {
    let text: Option<String> = conn
        .query_row(
            "SELECT p.data FROM part p JOIN message m ON m.id = p.message_id \
             WHERE p.session_id = ?1 AND p.rowid > ?2 \
             AND json_extract(m.data, '$.role') = 'assistant' \
             AND json_extract(p.data, '$.type') = 'text' \
             ORDER BY p.rowid DESC LIMIT 1",
            rusqlite::params![session_id, baseline.part_rowid],
            |row| row.get(0),
        )
        .ok();
    Ok(text.and_then(|data| match parse_part_data(&data) {
        Some(PartEvent::Text(body)) if !body.trim().is_empty() => Some(body),
        _ => None,
    }))
}

// ---- prompt --json summary ------------------------------------------------

/// End-of-run summary object printed by `zcode --prompt --json` (one block,
/// not JSONL — verified 0.15.0). `response` is the authoritative reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSummary {
    pub response: String,
    pub session_id: Option<String>,
    pub context_used: Option<u64>,
    pub context_window: Option<u64>,
    pub total_tokens: Option<u64>,
}

pub fn parse_prompt_summary(output: &str) -> Option<PromptSummary> {
    fn build(value: &serde_json::Value) -> Option<PromptSummary> {
        Some(PromptSummary {
            response: value.get("response")?.as_str()?.to_string(),
            session_id: value
                .get("sessionId")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            context_used: value
                .pointer("/projection/contextUsed")
                .and_then(serde_json::Value::as_u64),
            context_window: value
                .pointer("/projection/contextWindow")
                .and_then(serde_json::Value::as_u64),
            total_tokens: value
                .pointer("/usage/totalTokens")
                .and_then(serde_json::Value::as_u64),
        })
    }
    // The common case: the whole output is one summary object.
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(output.trim()) {
        if let Some(summary) = build(&value) {
            return Some(summary);
        }
    }
    // Tool-using turns stream NDJSON (one object per line); the authoritative
    // summary is the last line that parses to an object carrying `response`.
    output.lines().rev().find_map(|line| {
        let value = serde_json::from_str::<serde_json::Value>(line.trim()).ok()?;
        build(&value)
    })
}

/// Compact context watermark for the status line, e.g. `ctx 9k/200k (4%)`.
pub fn format_context_watermark(used: u64, window: u64) -> String {
    let kilo = |n: u64| -> String {
        if n >= 1000 {
            format!("{}k", n / 1000)
        } else {
            n.to_string()
        }
    };
    if window == 0 {
        return format!("ctx {}", kilo(used));
    }
    format!(
        "ctx {}/{} ({}%)",
        kilo(used),
        kilo(window),
        used * 100 / window
    )
}

/// High-watermark hint threshold: suggest /new at >= 80% usage.
pub fn context_watermark_warn(used: u64, window: u64) -> bool {
    window > 0 && used * 100 / window >= 80
}

/// A Beijing-skyline wireframe that stretches to fill `width`: four landmark
/// motifs — 天坛 (Temple of Heaven), 鸟巢 (Bird's Nest), 长城 (Great Wall),
/// 清华校门 (Tsinghua gate) — spread evenly over one continuous horizon with
/// `ZhiPU` resting on it, mirroring the single-line brand mark. Pure and
/// width-exact (every returned row has display width == `width`, all glyphs are
/// single columns) so it can be unit-tested and re-fitted on terminal resize.
/// Returns empty when `width` is too small to lay the motifs out without
/// overflow — the caller then shows the wordmark alone.
pub fn skyline_lines(width: usize) -> Vec<String> {
    // Four Beijing landmarks in fine wireframe line-art, 8 silhouette rows each,
    // every glyph a single column and each row left-aligned within its declared
    // width (the layout draws onto a space-filled line, so short rows need no
    // padding). Widths differ so the Bird's Nest and Wall read wider than the
    // pagoda and gate, matching the real skyline's proportions.
    #[rustfmt::skip]
    const TIANTAN: [&str; 8] = [ // 天坛 · tiered pagoda + stepped base (13 cols)
        "      ╷",
        "     ╱╲",
        "   ╭─┴─╮",
        "   ╰┬─┬╯",
        "  ╭┴───┴╮",
        "  ╰─┬─┬─╯",
        " ╭┴─────┴╮",
        " ╘═══════╛",
    ];
    #[rustfmt::skip]
    const NIAOCHAO: [&str; 8] = [ // 鸟巢 · flat woven-mesh ellipse (17 cols)
        "",
        "",
        "   ╭─────────╮",
        "  ╱╳╳╳╳╳╳╳╳╳╳╲",
        " ╱╳╳╳╳╳╳╳╳╳╳╳╳╲",
        " ╲╳╳╳╳╳╳╳╳╳╳╳╳╱",
        "  ╲╳╳╳╳╳╳╳╳╳╳╱",
        "   ╰─────────╯",
    ];
    #[rustfmt::skip]
    const CHANGCHENG: [&str; 8] = [ // 长城 · watchtower + wall winding to the horizon (17 cols)
        "  ╷ ╷ ╷",
        " ╭┴─┴─┴╮",
        " │ ╭─╮ │",
        " │ │ │ │╷╷╷",
        " │ │ │ ├┴┴┴╮",
        " │ │ │ │   ╰─╮",
        " │ │ │ │     ╰─╮",
        " ╰─┴─┴─╯       ╰─",
    ];
    #[rustfmt::skip]
    const XIAOMEN: [&str; 8] = [ // 清华二校门 · triple arch, tall centre, finial (13 cols)
        "      ╷",
        "    ╭─┴─╮",
        "╭─────┴─────╮",
        "│   ╭───╮   │",
        "│╭─╮│   │╭─╮│",
        "│││ │   │ │││",
        "│││ │   │ │││",
        "┴┴┴─┴───┴─┴┴┴",
    ];
    const MOTIFS: [&[&str]; 4] = [&TIANTAN, &NIAOCHAO, &CHANGCHENG, &XIAOMEN];
    const WIDTHS: [usize; 4] = [13, 17, 17, 13];
    const BRAND: &str = "ZhiPU";
    const SILH: usize = 8; // silhouette rows above the horizon
    let n = MOTIFS.len();
    let motif_total: usize = WIDTHS[0] + WIDTHS[1] + WIDTHS[2] + WIDTHS[3];
    let min_width = motif_total + (n + 1) * 2; // motifs + min 2-col gaps
    if width < min_width {
        return Vec::new();
    }
    let slack = width - motif_total;
    let base = slack / (n + 1);
    let extra = slack % (n + 1); // remainder spread onto the leftmost gaps
    let gap = |i: usize| base + usize::from(i < extra);
    // Start column of each motif (each ends at start + its own width).
    let mut xs = Vec::with_capacity(n);
    let mut cur = 0usize;
    for (i, &motif_width) in WIDTHS.iter().enumerate() {
        cur += gap(i);
        xs.push(cur);
        cur += motif_width;
    }
    let mut rows = Vec::with_capacity(SILH + 1);
    for r in 0..SILH {
        let mut line = vec![' '; width];
        for (i, motif) in MOTIFS.iter().enumerate() {
            for (j, ch) in motif[r].chars().enumerate() {
                line[xs[i] + j] = ch;
            }
        }
        rows.push(line.into_iter().collect());
    }
    // Continuous horizon with the brand mark resting at the centre (which, with
    // four evenly-spread motifs, always lands in the middle gap).
    let mut horizon = vec!['─'; width];
    let brand_len = BRAND.chars().count();
    let at = width / 2 - brand_len / 2;
    for (k, ch) in BRAND.chars().enumerate() {
        horizon[at + k] = ch;
    }
    rows.push(horizon.into_iter().collect());
    rows
}

/// Fixed display width of the braille skyline and the wordmark it sits under —
/// they render as one centred logo block (matching the reference art).
pub const SKYLINE_LOGO_W: usize = 45;

/// A higher-fidelity skyline drawn in Braille dots (2×4 sub-cells per glyph),
/// pre-rendered at the fixed logo width so it centres under the ZCODE wordmark.
/// Braille resolves smooth curves the box-drawing wireframe can't (the pagoda
/// domes, the nest ellipse, the wall's winding ridge). Returns 7 silhouette rows
/// + 1 horizon row, every row exactly `SKYLINE_LOGO_W` display columns.
pub fn skyline_braille() -> Vec<String> {
    // Pre-rendered (see the design prototype); every row is 45 columns wide, all
    // glyphs single-column (braille U+28xx + box-drawing horizon).
    const ROWS: [&str; 8] = [
        "⠀⠀⠀⠀⠀⠀⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀",
        "⠀⠀⠀⠀⠰⠟⠿⠻⠆⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⣶⣄⠀⠀⠀⠀",
        "⠀⠀⠀⣴⠞⠋⠉⠙⠳⣦⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠉⠀⠀⠀⠀⠀",
        "⠀⠀⢀⣀⣤⠤⠤⠤⣤⣀⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣤⠀⣤⢠⡄⠀⠀⠀⠀⠀⠀⣿⠉⠉⢉⣽⠛⣯⡉⠉⠉⣿",
        "⠀⠐⠛⠁⠀⠀⠀⠀⠀⠈⠛⠂⢀⣤⣴⠖⣶⠒⣶⠲⣦⣤⡀⣿⠉⠉⢹⣇⣒⣂⡤⠄⠀⠀⣿⣀⣀⣾⠃⠀⠘⣷⣀⣀⣿",
        "⠀⠀⢀⣀⣀⣀⣀⣀⣀⣀⡀⢰⡏⠤⣿⠤⣿⠤⣿⠤⣿⠤⢹⣿⠀⣶⢸⡇⠀⠈⠉⠛⠿⣅⣿⡏⢹⣿⠀⠀⠀⣿⡏⢹⣿",
        "⠀⠠⠤⠤⠤⠤⠤⠤⠤⠤⠤⠄⠙⠶⢿⣄⣿⣀⣿⣠⡿⠶⠋⣿⠀⣿⢸⡇⠀⠀⠀⠀⠀⢈⣿⣂⣀⣿⣀⣀⣀⣿⣀⣀⣿",
        "────────────────────ZhiPU────────────────────",
    ];
    ROWS.iter().map(|s| (*s).to_string()).collect()
}

/// How to render the welcome skyline. `Graphics` (Sixel/Kitty true image) is a
/// planned stage-two enhancement and not produced yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkylineMode {
    /// Braille-dot art — smoother curves; the default on capable terminals.
    Braille,
    /// Box-drawing wireframe — the widest-compatible fallback.
    Wire,
    /// Skyline suppressed (wordmark shows alone).
    None,
}

/// Pick the skyline renderer. `ZCODE_TUI_SKYLINE=braille|wire|off` forces it;
/// otherwise `auto` prefers the smoother Braille dots when the locale is UTF-8
/// (they need a Unicode font) and falls back to the wireframe when it clearly
/// is not. Set `wire` if your font renders the dots as tofu/blur.
pub fn skyline_mode<F>(env_lookup: F) -> SkylineMode
where
    F: Fn(&str) -> Option<String>,
{
    match env_lookup("ZCODE_TUI_SKYLINE").as_deref().map(str::trim) {
        Some("wire") => SkylineMode::Wire,
        Some("braille") => SkylineMode::Braille,
        Some("off") | Some("none") | Some("0") => SkylineMode::None,
        _ => {
            let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"]
                .iter()
                .find_map(|key| env_lookup(key))
                .map(|value| value.to_lowercase().contains("utf"))
                .unwrap_or(false);
            if utf8 {
                SkylineMode::Braille
            } else {
                SkylineMode::Wire
            }
        }
    }
}

/// Whether to attempt the true graphics-protocol logo (Sixel/Kitty/iTerm2). It
/// is the default; a terminal-capability probe decides if it actually renders,
/// falling back to the text skyline ([`skyline_mode`]) otherwise. Forcing any
/// text mode (`wire`/`braille`/`off`) opts out of the probe entirely.
pub fn skyline_graphics_wanted<F>(env_lookup: F) -> bool
where
    F: Fn(&str) -> Option<String>,
{
    !matches!(
        env_lookup("ZCODE_TUI_SKYLINE").as_deref().map(str::trim),
        Some("wire") | Some("braille") | Some("off") | Some("none") | Some("0")
    )
}

// ---- session picker / history / folding / ui config -----------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub id: String,
    pub title: String,
    pub directory: String,
    pub time_updated: i64,
}

/// Recent kernel sessions for the picker: current-directory sessions first,
/// then by recency. Title falls back to the directory tail.
pub fn list_recent_sessions(
    conn: &Connection,
    current_dir: &str,
    limit: usize,
) -> Result<Vec<SessionRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, COALESCE(title, ''), COALESCE(directory, ''), COALESCE(time_updated, 0) \
         FROM session ORDER BY (directory = ?1) DESC, time_updated DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![current_dir, limit as i64], |row| {
            let id: String = row.get(0)?;
            let title: String = row.get(1)?;
            let directory: String = row.get(2)?;
            let title = if title.trim().is_empty() {
                directory
                    .rsplit('/')
                    .find(|piece| !piece.is_empty())
                    .unwrap_or(&id)
                    .to_string()
            } else {
                title
            };
            Ok(SessionRow {
                id,
                title,
                directory,
                time_updated: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Compact relative age for the picker, from millisecond timestamps.
pub fn relative_age(now_ms: i64, then_ms: i64) -> String {
    let seconds = (now_ms - then_ms).max(0) / 1000;
    match seconds {
        0..=59 => "now".to_string(),
        60..=3599 => format!("{}m", seconds / 60),
        3600..=86_399 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86_400),
    }
}

/// The kernel persists every --prompt input; read it (oldest→newest) as the
/// base of the Up/Down history. Read-only, adjacent duplicates collapsed.
pub fn recent_input_history(conn: &Connection, limit: usize) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT COALESCE(text, '') FROM input_history ORDER BY rowid DESC LIMIT ?1")?;
    let mut rows = stmt
        .query_map([limit as i64], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.reverse();
    rows.retain(|text| !text.trim().is_empty());
    rows.dedup();
    Ok(rows)
}

/// Ctrl+R matcher: case-insensitive substring over the merged history,
/// newest first, de-duplicated.
pub fn history_search(history: &[String], query: &str, limit: usize) -> Vec<String> {
    let needle = query.to_lowercase();
    let mut seen = std::collections::HashSet::new();
    history
        .iter()
        .rev()
        .filter(|entry| needle.is_empty() || entry.to_lowercase().contains(&needle))
        .filter(|entry| seen.insert(entry.as_str()))
        .take(limit)
        .cloned()
        .collect()
}

/// Folding decision for long transcript cells: Some((visible_head_lines,
/// hidden_count)) when the text exceeds the threshold. Render-time only.
pub fn fold_preview(text: &str, threshold: usize, head: usize) -> Option<(usize, usize)> {
    let total = text.lines().count();
    (total > threshold && head < total).then(|| (head, total - head))
}

/// User config: theme token overrides plus the mouse and notify switches.
/// Parsing never fails — bad lines fall back to defaults so startup cannot
/// break.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiConfig {
    pub colors: BTreeMap<String, (u8, u8, u8)>,
    pub mouse: Option<bool>,
    /// `notify = off` silences the >30s turn-complete terminal bell.
    pub notify: Option<bool>,
}

pub const UI_CONFIG_COLOR_KEYS: &[&str] = &[
    "accent",
    "accent_dim",
    "text",
    "dim",
    "good",
    "bad",
    "frame",
    "code_bg",
    "band_bg",
    "brand",
    "brand_dim",
];

pub fn parse_hex_color(value: &str) -> Option<(u8, u8, u8)> {
    let hex = value.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).ok();
    Some((channel(0..2)?, channel(2..4)?, channel(4..6)?))
}

pub fn parse_ui_config(content: &str) -> UiConfig {
    let mut config = UiConfig::default();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key == "mouse" {
            config.mouse = match value.to_ascii_lowercase().as_str() {
                "on" | "true" | "1" => Some(true),
                "off" | "false" | "0" => Some(false),
                _ => config.mouse,
            };
        } else if key == "notify" {
            config.notify = match value.to_ascii_lowercase().as_str() {
                "on" | "true" | "1" => Some(true),
                "off" | "false" | "0" => Some(false),
                _ => config.notify,
            };
        } else if UI_CONFIG_COLOR_KEYS.contains(&key) {
            if let Some(rgb) = parse_hex_color(value) {
                config.colors.insert(key.to_string(), rgb);
            }
        }
    }
    config
}

pub fn ui_config_path_from(home: &Path) -> PathBuf {
    home.join(".config").join("zcode-tui").join("config")
}

/// Resolve and parse the user config; every failure path yields defaults.
pub fn load_ui_config() -> UiConfig {
    let path = std::env::var_os("ZCODE_TUI_CONFIG")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| ui_config_path_from(Path::new(&home))));
    path.and_then(|path| fs::read_to_string(path).ok())
        .map(|content| parse_ui_config(&content))
        .unwrap_or_default()
}

// ---- app-server protocol client (default-on, ZCODE_TUI_APP_SERVER=0 opts out) ----
//
// The kernel's `app-server` is a newline-delimited JSON stdio protocol
// (envelope {id, method, params}, NOT JSON-RPC — a `jsonrpc` key is
// rejected). It is the only path to true token streaming: `--prompt`
// buffers the kernel's internal delta stream, but app-server re-exposes it.
//
// Verified sequence (2026-07-06):
//   session/create {workspace:{workspaceKey, workspacePath}}  -> session.sessionId
//   session/subscribe {sessionId, deliveryKind:"desktop-continuous"}
//   session/send {sessionId, content}                         -> {accepted:true}
//   <- session/event notifications: params.payload.{kind, delta, done}
//      kind text_delta carries the streamed body token by token.

/// deliveryKind that streams events continuously (vs web-remote-replayable).
pub const APP_SERVER_DELIVERY_KIND: &str = "desktop-continuous";

/// Encode one request as a single compact JSON line (no jsonrpc field, no
/// trailing newline — the caller frames with `\n`).
pub fn encode_app_request(id: u64, method: &str, params: serde_json::Value) -> String {
    serde_json::json!({ "id": id, "method": method, "params": params }).to_string()
}

pub fn app_create_params(workspace_path: &str) -> serde_json::Value {
    serde_json::json!({
        "workspace": { "workspaceKey": workspace_path, "workspacePath": workspace_path }
    })
}

pub fn app_subscribe_params(session_id: &str) -> serde_json::Value {
    serde_json::json!({
        "sessionId": session_id,
        "deliveryKind": APP_SERVER_DELIVERY_KIND,
        "includeSnapshot": false
    })
}

pub fn app_send_params(session_id: &str, content: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id, "content": content })
}

/// `session/send` with `attachments[]`. Empty attachments MUST yield the
/// exact same shape as [`app_send_params`] (no `attachments` key), so the
/// no-mention path stays byte-identical to the pre-attachment behaviour.
pub fn app_send_params_with_attachments(
    session_id: &str,
    content: &str,
    attachments: &[serde_json::Value],
) -> serde_json::Value {
    if attachments.is_empty() {
        return app_send_params(session_id, content);
    }
    serde_json::json!({
        "sessionId": session_id,
        "content": content,
        "attachments": attachments,
    })
}

/// Extensions the kernel treats as images (attachment `kind:"image"`).
fn image_mime_for(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

/// Best-effort mimeType for `kind:"file"` attachments; unknown extensions
/// fall back to text/plain (the kernel only needs a plausible type).
fn file_mime_for(ext: &str) -> &'static str {
    match ext {
        "md" | "markdown" => "text/markdown",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "svg" => "image/svg+xml",
        "js" | "mjs" => "text/javascript",
        _ => "text/plain",
    }
}

/// Map `@file` mentions (already vetted by [`extract_file_mentions`]) to
/// `session/send` attachment objects — kernel bundle schema `Pwt`, a strict
/// union discriminated on `kind`, pinned live 2026-07-07 on kernel 0.15.0:
/// image `{kind, filename, mimeType, sizeBytes?, dataBase64?, localPath?}`,
/// file `{kind, filename, mimeType, sizeBytes(REQUIRED), dataBase64?,
/// textContent?, localPath?}`. `localPath` alone is sufficient (verified:
/// the model reads the referenced file's content); `sizeBytes` is mandatory
/// for `kind:"file"`, so a mention whose metadata cannot be read is skipped
/// rather than sent half-formed (a strict-schema ZodError would kill the
/// whole send).
pub fn build_send_attachments(mentions: &[String], cwd: &Path) -> Vec<serde_json::Value> {
    let mut attachments = Vec::new();
    for mention in mentions {
        let Ok(resolved) = cwd.join(mention).canonicalize() else {
            continue;
        };
        let Ok(meta) = fs::metadata(&resolved) else {
            continue;
        };
        let filename = resolved
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| mention.clone());
        let ext = resolved
            .extension()
            .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let local_path = resolved.to_string_lossy().into_owned();
        let attachment = match image_mime_for(&ext) {
            Some(mime) => serde_json::json!({
                "kind": "image",
                "filename": filename,
                "mimeType": mime,
                "sizeBytes": meta.len(),
                "localPath": local_path,
            }),
            None => serde_json::json!({
                "kind": "file",
                "filename": filename,
                "mimeType": file_mime_for(&ext),
                "sizeBytes": meta.len(),
                "localPath": local_path,
            }),
        };
        attachments.push(attachment);
    }
    attachments
}

pub fn app_stop_params(session_id: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id })
}

// Session-control params (schemas pinned 2026-07-07 via zod-error probing on
// kernel 0.15.0; setMode verified live).

/// `session/setMode` — mode ∈ plan|build|edit|yolo|auto (kernel-enforced enum).
pub fn app_set_mode_params(session_id: &str, mode: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id, "mode": mode })
}

/// `session/setModel` — `model` is sent back verbatim from the state push's
/// `model.available[].ref` (shape `{modelId, providerId}`).
pub fn app_set_model_params(session_id: &str, model_ref: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id, "model": model_ref })
}

/// `session/setThoughtLevel` — level values per the state push's
/// `thoughtLevel.available[].value` (observed: enabled/disabled).
pub fn app_set_thought_params(session_id: &str, level: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id, "thoughtLevel": level })
}

/// `session/compact` — compacts the session context in place.
pub fn app_compact_params(session_id: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id })
}

/// `session/steer` — inject input into the RUNNING turn (same shape as send).
pub fn app_steer_params(session_id: &str, content: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id, "content": content })
}

/// Subscribe to the ZCode 3.5.3 conversation control plane while keeping the
/// legacy session subscription for token/body events.
pub fn v4_conversation_subscribe_params(
    session_id: &str,
    connection_id: &str,
) -> serde_json::Value {
    serde_json::json!({
        "topic": format!("conversation/{session_id}"),
        "connectionId": connection_id,
        "clientMode": APP_SERVER_DELIVERY_KIND,
        "visibility": "foreground",
    })
}

/// Generic V4 command envelope. Only commands whose bundle schema requires a
/// CAS field receive it: setFollowupMode needs baseRevision, while
/// applyFileRewind needs both revision and log epoch. sendText deliberately
/// works without a CAS base and is judged by the semantic delivery frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V4CommandBase<'a> {
    None,
    Revision(u64),
    RevisionAndEpoch { revision: u64, log_epoch: &'a str },
}

pub fn v4_command_params(
    command_id: &str,
    client_id: &str,
    session_id: &str,
    command_type: &str,
    payload: serde_json::Value,
    base: V4CommandBase<'_>,
    issued_at: u64,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "commandId": command_id,
        "clientId": client_id,
        "sessionId": session_id,
        "type": command_type,
        "payload": payload,
        "issuedAt": issued_at,
    });
    let map = value
        .as_object_mut()
        .expect("v4 command envelope is object");
    match base {
        V4CommandBase::None => {}
        V4CommandBase::Revision(revision) => {
            map.insert("baseRevision".to_string(), serde_json::json!(revision));
        }
        V4CommandBase::RevisionAndEpoch {
            revision,
            log_epoch,
        } => {
            map.insert("baseRevision".to_string(), serde_json::json!(revision));
            map.insert("baseLogEpoch".to_string(), serde_json::json!(log_epoch));
        }
    }
    value
}

/// V4 row identity used by preview/apply (strict `{rowId, entityId}`).
pub fn v4_rewind_target(row_id: u64, entity_id: &str) -> serde_json::Value {
    serde_json::json!({ "rowId": row_id, "entityId": entity_id })
}

pub fn v4_file_rewind_preview_params(
    session_id: &str,
    row_id: u64,
    entity_id: &str,
    base_revision: u64,
    base_log_epoch: &str,
) -> serde_json::Value {
    serde_json::json!({
        "sessionId": session_id,
        "target": v4_rewind_target(row_id, entity_id),
        "baseRevision": base_revision,
        "baseLogEpoch": base_log_epoch,
    })
}

/// A V4 conversation row reduced to the fields needed by `/rewind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V4ConversationRow {
    pub row_id: u64,
    pub entity_id: String,
    pub kind: String,
    pub state: String,
    pub files: u64,
    pub additions: u64,
    pub deletions: u64,
    pub file_state: Option<String>,
    pub can_rewind_files: bool,
}

fn parse_v4_row(value: &serde_json::Value) -> Option<V4ConversationRow> {
    let changes = value.get("fileChanges");
    Some(V4ConversationRow {
        row_id: value.get("rowId")?.as_u64()?,
        entity_id: value.get("entityId")?.as_str()?.to_string(),
        kind: str_at(value, "kind"),
        state: str_at(value, "state"),
        files: changes
            .and_then(|v| v.get("files"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        additions: changes
            .and_then(|v| v.get("additions"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        deletions: changes
            .and_then(|v| v.get("deletions"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        file_state: changes
            .and_then(|v| v.get("state"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        can_rewind_files: value
            .pointer("/actions/canRewindFiles")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    })
}

/// Minimal snapshot/delta cache for the hybrid V4 control plane. The V4
/// frame's `toSeq` is an event sequence, not the command CAS revision; only
/// `snapshot.revision` and `state.updated.patch.revision` update `revision`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct V4ConversationState {
    pub revision: Option<u64>,
    pub log_epoch: Option<String>,
    pub input_routing: Option<String>,
    pub followup_mode: Option<String>,
    pub set_followup_allowed: Option<bool>,
    pub rows: Vec<V4ConversationRow>,
    /// Command id -> admitted delivery, retained so a frame that races ahead
    /// of its response can still settle the pending steer once the ack lands.
    pub input_deliveries: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct V4FrameEffect {
    pub deliveries: Vec<(String, String)>,
}

impl V4ConversationState {
    fn apply_queue(&mut self, value: &serde_json::Value, effect: &mut V4FrameEffect) {
        let Some(items) = value.get("items").and_then(|v| v.as_array()) else {
            return;
        };
        for item in items {
            let Some(command_id) = item.get("sourceCommandId").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(delivery) = item.pointer("/delivery/admitted").and_then(|v| v.as_str()) else {
                continue;
            };
            self.input_deliveries
                .insert(command_id.to_string(), delivery.to_string());
            effect
                .deliveries
                .push((command_id.to_string(), delivery.to_string()));
        }
    }

    fn apply_state_patch(&mut self, patch: &serde_json::Value, effect: &mut V4FrameEffect) {
        if let Some(revision) = patch.get("revision").and_then(serde_json::Value::as_u64) {
            self.revision = Some(revision);
        }
        if let Some(mode) = patch.pointer("/inputRouting/mode").and_then(|v| v.as_str()) {
            self.input_routing = Some(mode.to_string());
        }
        if let Some(mode) = patch
            .pointer("/config/followupMode")
            .and_then(|v| v.as_str())
        {
            self.followup_mode = Some(mode.to_string());
        }
        if let Some(allowed) = patch
            .pointer("/availability/setFollowupMode/allowed")
            .and_then(serde_json::Value::as_bool)
        {
            self.set_followup_allowed = Some(allowed);
        }
        if let Some(queue) = patch.get("queue") {
            self.apply_queue(queue, effect);
        }
    }

    fn upsert_row(&mut self, row: V4ConversationRow) {
        if let Some(existing) = self.rows.iter_mut().find(|old| old.row_id == row.row_id) {
            *existing = row;
        } else {
            self.rows.push(row);
            self.rows.sort_by_key(|row| row.row_id);
        }
    }

    /// Apply one complete `v4/conversation/frame` params object. Fragmented
    /// transport frames are ignored safely; the conversation window is bounded
    /// and normal CLI frames are complete in observed 3.5.3 sessions.
    pub fn apply_frame(&mut self, params: &serde_json::Value) -> V4FrameEffect {
        let mut effect = V4FrameEffect::default();
        let Some(payload) = params.pointer("/frame/payload") else {
            return effect;
        };
        match payload.get("kind").and_then(|v| v.as_str()) {
            Some("snapshot") => {
                let Some(snapshot) = payload.get("snapshot") else {
                    return effect;
                };
                self.revision = snapshot.get("revision").and_then(serde_json::Value::as_u64);
                self.log_epoch = snapshot
                    .get("logEpoch")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                self.input_routing = snapshot
                    .pointer("/inputRouting/mode")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                self.followup_mode = snapshot
                    .pointer("/config/followupMode")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                self.set_followup_allowed = snapshot
                    .pointer("/availability/setFollowupMode/allowed")
                    .and_then(serde_json::Value::as_bool);
                self.rows = snapshot
                    .pointer("/rows/window")
                    .and_then(|v| v.as_array())
                    .map(|rows| rows.iter().filter_map(parse_v4_row).collect())
                    .unwrap_or_default();
                self.input_deliveries.clear();
                if let Some(queue) = snapshot.get("queue") {
                    self.apply_queue(queue, &mut effect);
                }
            }
            Some("deltas") => {
                let Some(deltas) = payload.get("deltas").and_then(|v| v.as_array()) else {
                    return effect;
                };
                for delta in deltas {
                    match delta.get("op").and_then(|v| v.as_str()) {
                        Some("state.updated") => {
                            if let Some(patch) = delta.get("patch") {
                                self.apply_state_patch(patch, &mut effect);
                            }
                        }
                        Some("row.appended") | Some("row.upserted") => {
                            if let Some(row) = delta.get("row").and_then(parse_v4_row) {
                                self.upsert_row(row);
                            }
                        }
                        Some("row.removed") => {
                            if let Some(row_id) =
                                delta.get("rowId").and_then(serde_json::Value::as_u64)
                            {
                                self.rows.retain(|row| row.row_id != row_id);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        effect
    }

    pub fn rewind_rows(&self) -> Vec<&V4ConversationRow> {
        self.rows
            .iter()
            .rev()
            .filter(|row| {
                row.kind == "turnHeader"
                    && row.can_rewind_files
                    && row.files > 0
                    && row.file_state.as_deref() != Some("reverted")
            })
            .collect()
    }

    pub fn delivery_for(&self, command_id: &str) -> Option<&str> {
        self.input_deliveries.get(command_id).map(String::as_str)
    }
}

/// Semantic acknowledgement returned by `v4/command`. A successful response
/// envelope may still carry status stale/rejected/failed, so callers must
/// inspect this object before changing UI state.
#[derive(Debug, Clone, PartialEq)]
pub struct V4CommandAck {
    pub command_id: String,
    pub status: String,
    pub reason_code: Option<String>,
    pub message: Option<String>,
    pub revision_at_decision: u64,
    pub result: Option<serde_json::Value>,
}

impl V4CommandAck {
    pub fn accepted(&self) -> bool {
        matches!(self.status.as_str(), "accepted" | "duplicate" | "noop")
    }

    pub fn input_delivery(&self) -> Option<&str> {
        self.result
            .as_ref()
            .filter(|result| {
                result.get("type").and_then(|v| v.as_str()) == Some("inputDisposition")
            })
            .and_then(|result| result.get("delivery"))
            .and_then(|v| v.as_str())
    }
}

pub fn parse_v4_command_ack(result: &serde_json::Value) -> Option<V4CommandAck> {
    Some(V4CommandAck {
        command_id: result.get("commandId")?.as_str()?.to_string(),
        status: result.get("status")?.as_str()?.to_string(),
        reason_code: result
            .get("reasonCode")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        message: result
            .get("message")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        revision_at_decision: result
            .get("revisionAtDecision")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        result: result.get("result").cloned(),
    })
}

/// `session/resume` — reopen an existing session; the result is shaped like
/// `session/create`'s (verified live: messages/projection/session/todos).
/// `runtime_model` (from [`build_runtime_model`]) MUST accompany the resume:
/// resume restores the conversation but NOT the model runtime — without it
/// the first send fails with `ZCODE_RUNTIME_MODEL_UNAVAILABLE` ("历史任务
/// 使用的模型已不可用", pinned live 2026-07-07).
pub fn app_resume_params(
    session_id: &str,
    runtime_model: Option<&serde_json::Value>,
) -> serde_json::Value {
    match runtime_model {
        Some(runtime) => {
            serde_json::json!({ "sessionId": session_id, "runtimeModel": runtime })
        }
        None => serde_json::json!({ "sessionId": session_id }),
    }
}

/// Attach an `mcpServers` array to `session/create`/`session/resume` params
/// (both schemas carry the same optional field). `None` leaves the params
/// untouched so kernels predating the field never see an unknown key.
pub fn with_mcp_servers(
    mut params: serde_json::Value,
    servers: Option<serde_json::Value>,
) -> serde_json::Value {
    if let (Some(list), Some(map)) = (servers, params.as_object_mut()) {
        map.insert("mcpServers".to_string(), list);
    }
    params
}

/// Attach the ZCode 3.3.6 app-server session policy fields. Both create and
/// resume use the same strict optional keys. Empty lists deliberately leave
/// the request untouched so the no-policy path stays compatible with older
/// kernels and byte-identical to the pre-adaptation shape.
pub fn with_tool_policy(
    mut params: serde_json::Value,
    allowlist: &[String],
    denylist: &[String],
) -> serde_json::Value {
    let Some(map) = params.as_object_mut() else {
        return params;
    };
    if !allowlist.is_empty() {
        map.insert("toolAllowlist".to_string(), serde_json::json!(allowlist));
    }
    if !denylist.is_empty() {
        map.insert("toolDenylist".to_string(), serde_json::json!(denylist));
    }
    params
}

/// Build the `mcpServers[]` array for `session/create`/`resume` from the
/// project + user MCP configs. The kernel itself NEVER reads project
/// `.mcp.json` (bundle-verified 2026-07-07: it only appears in plugin
/// loading), so streaming sessions only get MCP servers the client passes
/// here — schema `$xe`, strict union pinned from the kernel bundle:
/// stdio `{name, command, args, env:[{name,value}], timeoutMs?}` (NO type
/// key), remote `{name, type:"http"|"sse", url, headers:[{name,value}],
/// timeoutMs?}`. Disabled servers are skipped; on a name collision the
/// project entry wins. Returns None when nothing survives, so the params
/// stay byte-identical to the pre-MCP shape.
pub fn mcp_servers_param(project: &McpConfig, user: &McpConfig) -> Option<serde_json::Value> {
    let mut merged: BTreeMap<&String, &McpServer> = BTreeMap::new();
    for (name, server) in user.servers.iter().chain(project.servers.iter()) {
        merged.insert(name, server); // later (project) insert wins
    }
    let kv_array = |map: &BTreeMap<String, String>| -> serde_json::Value {
        serde_json::Value::Array(
            map.iter()
                .map(|(name, value)| serde_json::json!({ "name": name, "value": value }))
                .collect(),
        )
    };
    let servers: Vec<serde_json::Value> = merged
        .into_iter()
        .filter(|(_, server)| !server.disabled)
        .filter_map(|(name, server)| {
            if let Some(url) = &server.url {
                // Remote shape; anything that isn't exactly http/sse is
                // normalized to http (the kernel enum allows only those two).
                let transport = match server.transport_label() {
                    "sse" => "sse",
                    _ => "http",
                };
                Some(serde_json::json!({
                    "name": name,
                    "type": transport,
                    "url": url,
                    "headers": kv_array(&server.headers),
                }))
            } else if !server.command.is_empty() {
                Some(serde_json::json!({
                    "name": name,
                    "command": server.command,
                    "args": server.args,
                    "env": kv_array(&server.env),
                }))
            } else {
                None
            }
        })
        .collect();
    if servers.is_empty() {
        None
    } else {
        Some(serde_json::Value::Array(servers))
    }
}

/// Build the `runtimeModel` object the kernel needs to revive a resumed
/// session, from the kernel's own `~/.zcode/cli/config.json` (the same file
/// session/create seeds a fresh session from). Shape pinned live 2026-07-07
/// against the kernel's strict zod schema (`p_` in the bundle):
/// `{revision, generatedAt, model:{providerId,modelId},
///   provider:{providerId, kind, label?, source, baseURL?,
///             apiKey:{source:"inline", value}?, models:[{modelId,label?}…]}}`.
/// Returns None when the config is missing or not in the known layout — the
/// caller resumes without it and relies on the create-fallback path.
pub fn build_runtime_model(config_json: &str, generated_at: u64) -> Option<serde_json::Value> {
    let config: serde_json::Value = serde_json::from_str(config_json).ok()?;
    // `model.main` is "provider/modelId".
    let main = config
        .pointer("/model/main")
        .and_then(|v| v.as_str())
        .or_else(|| config.get("model").and_then(|v| v.as_str()))?;
    let (provider_id, model_id) = main.split_once('/')?;
    let provider = config.pointer(&format!("/provider/{provider_id}"))?;
    let kind = provider.get("kind")?.as_str()?;
    let models: Vec<serde_json::Value> = provider
        .get("models")?
        .as_object()?
        .iter()
        .map(|(id, m)| {
            serde_json::json!({
                "modelId": id,
                "label": m.get("name").and_then(|v| v.as_str()).unwrap_or(id),
            })
        })
        .collect();
    if models.is_empty() {
        return None;
    }
    let mut provider_obj = serde_json::json!({
        "providerId": provider_id,
        "kind": kind,
        "label": provider.get("name").and_then(|v| v.as_str()).unwrap_or(provider_id),
        "source": "user",
        "models": models,
    });
    if let Some(base_url) = provider
        .pointer("/options/baseURL")
        .and_then(|v| v.as_str())
    {
        provider_obj["baseURL"] = serde_json::json!(base_url);
    }
    if let Some(api_key) = provider.pointer("/options/apiKey").and_then(|v| v.as_str()) {
        // The kernel's credential union; inline carries the key verbatim
        // (same trust domain: the kernel owns config.json to begin with).
        provider_obj["apiKey"] = serde_json::json!({ "source": "inline", "value": api_key });
    }
    Some(serde_json::json!({
        "revision": "zcode-tui-resume",
        "generatedAt": generated_at,
        "model": { "providerId": provider_id, "modelId": model_id },
        "provider": provider_obj,
    }))
}

/// `session/usage` — per-session token breakdown.
pub fn app_usage_params(session_id: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id })
}

/// `usage/stats` — period aggregate; kernel zod enum pins range to 7d|30d.
pub fn usage_stats_params(range: &str) -> serde_json::Value {
    serde_json::json!({ "range": range })
}

/// `session/close` — release a session the TUI is discarding (/new, clean
/// exit). Params pinned live 2026-07-07: `{sessionId}` strict (empty params
/// ZodError names sessionId), result `{}`. Fire-and-forget, best-effort.
pub fn app_close_params(session_id: &str) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id })
}

/// One captured `checkpoint.created` event — a rewind target. The kernel
/// emits one per gated tool write; the snapshot is the workspace state
/// **before** that write ran (pinned live 2026-07-07: rewinding to the first
/// checkpoint of a fresh file DELETES it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointEntry {
    pub id: String,
    /// `fileCount` — files captured by the snapshot.
    pub files: u64,
    /// `targetMessageId` — the turn's user message. The conversation-scope
    /// leg MUST target this via `{kind:"message"}`: pinned live 2026-07-09,
    /// `session/rewind` COERCES checkpoint-kind targets to a workspace (file)
    /// rewind no matter which scope was requested, while message-kind targets
    /// honor scope:"conversation" and leave files alone.
    pub message_id: Option<String>,
}

/// Short display form of a checkpoint id ("checkpoint_90c0d5df-…" → "90c0d5df").
pub fn checkpoint_short_id(id: &str) -> String {
    id.trim_start_matches("checkpoint_")
        .chars()
        .take(8)
        .collect()
}

/// A rewind target — the discriminated union of `session/rewind` and the
/// file-rewind pair. The UI picks checkpoint forms; conversation legs are
/// translated to `Message` (see `conversation_target`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewindTarget {
    /// `{kind:"latestCheckpoint"}` — kernel-tracked most recent checkpoint.
    LatestCheckpoint,
    /// `{kind:"checkpoint", checkpointId}`.
    Checkpoint(String),
    /// `{kind:"message", messageId}` — the only target kind whose
    /// scope:"conversation" is honored by session/rewind (pinned live
    /// 2026-07-09; checkpoint kinds get coerced to a forced file rewind).
    Message(String),
    /// `{kind:"turn", turnIndex}` — conversation rewind to before that turn.
    Turn(u64),
    /// ZCode 3.5.3 V4 stable row target `{rowId, entityId}`.
    V4Row { row_id: u64, entity_id: String },
}

impl RewindTarget {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Self::LatestCheckpoint => serde_json::json!({ "kind": "latestCheckpoint" }),
            Self::Checkpoint(id) => {
                serde_json::json!({ "kind": "checkpoint", "checkpointId": id })
            }
            Self::Message(id) => serde_json::json!({ "kind": "message", "messageId": id }),
            Self::Turn(index) => serde_json::json!({ "kind": "turn", "turnIndex": index }),
            Self::V4Row { row_id, entity_id } => v4_rewind_target(*row_id, entity_id),
        }
    }

    /// Picker/status label, e.g. "latest checkpoint" or "checkpoint 90c0d5df".
    pub fn label(&self) -> String {
        match self {
            Self::LatestCheckpoint => "latest checkpoint".to_string(),
            Self::Checkpoint(id) => format!("checkpoint {}", checkpoint_short_id(id)),
            Self::Message(id) => format!("message {}", &id[..id.len().min(16)]),
            Self::Turn(index) => format!("turn {index}"),
            Self::V4Row { row_id, .. } => format!("turn row {row_id}"),
        }
    }

    pub fn is_v4(&self) -> bool {
        matches!(self, Self::V4Row { .. })
    }
}

/// Translate a picker target (checkpoint form) into the message-kind target
/// its conversation-scope leg must use. None when the checkpoint (or its
/// `targetMessageId`) is unknown — the caller refuses the conversation leg
/// instead of sending a checkpoint target that would force-restore files.
pub fn conversation_target(
    picker: &RewindTarget,
    checkpoints: &[CheckpointEntry],
) -> Option<RewindTarget> {
    let entry = match picker {
        RewindTarget::Checkpoint(id) => checkpoints.iter().find(|c| &c.id == id),
        RewindTarget::LatestCheckpoint => checkpoints.last(),
        // Already conversation-shaped.
        RewindTarget::Message(_) | RewindTarget::Turn(_) => return Some(picker.clone()),
        RewindTarget::V4Row { .. } => return None,
    }?;
    entry.message_id.clone().map(RewindTarget::Message)
}

/// `session/previewFileRewind` / `session/applyFileRewind` — both take
/// `{sessionId, target}` (empty-params ZodError names exactly those two).
pub fn app_file_rewind_params(session_id: &str, target: &RewindTarget) -> serde_json::Value {
    serde_json::json!({ "sessionId": session_id, "target": target.to_json() })
}

/// `session/rewind {sessionId, target, scope}`. scope ∈ conversation|
/// workspace|both. The UI only sends scope:"conversation" and only with
/// message-kind targets — BOTH pinned live: session/rewind FORCE-applies
/// file restores over external modifications (2026-07-07, ignores
/// canApply:false), and it COERCES checkpoint-kind targets to a workspace
/// rewind even when scope:"conversation" was requested (2026-07-09,
/// rewind.triggered came back scope:"workspace" and deleted the file).
/// File restores must go through `session/applyFileRewind` instead.
pub fn app_rewind_params(
    session_id: &str,
    target: &RewindTarget,
    scope: &str,
) -> serde_json::Value {
    serde_json::json!({
        "sessionId": session_id,
        "target": target.to_json(),
        "scope": scope,
    })
}

/// One file row of a rewind preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewindFile {
    pub path: String,
    /// safeFiles: `action` ("restore"); unsafeFiles: `reason`
    /// ("external_modified", …).
    pub note: String,
    /// Joined `toolNames`.
    pub tools: String,
}

/// Parsed `session/previewFileRewind` result (shape pinned live 2026-07-07:
/// `{canApply, safeFiles[{action,operationCount,path,toolNames}],
/// unsafeFiles[{path,reason,expectedHash,currentHash,…}], ignoredFiles,…}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewindPreview {
    pub can_apply: bool,
    pub safe: Vec<RewindFile>,
    pub unsafe_files: Vec<RewindFile>,
    pub ignored: usize,
}

fn rewind_files(result: &serde_json::Value, key: &str, note_key: &str) -> Vec<RewindFile> {
    result
        .get(key)
        .and_then(|v| v.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|file| {
                    let path = file.get("path")?.as_str()?.to_string();
                    let tools = file
                        .get("toolNames")
                        .and_then(|v| v.as_array())
                        .map(|names| {
                            names
                                .iter()
                                .filter_map(|n| n.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_default();
                    Some(RewindFile {
                        path,
                        note: str_at(file, note_key),
                        tools,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a previewFileRewind result. Missing `canApply` -> None (not a
/// preview shape); empty file lists are valid (nothing to restore).
pub fn parse_rewind_preview(result: &serde_json::Value) -> Option<RewindPreview> {
    let can_apply = result.get("canApply")?.as_bool()?;
    Some(RewindPreview {
        can_apply,
        safe: rewind_files(result, "safeFiles", "action"),
        unsafe_files: rewind_files(result, "unsafeFiles", "reason"),
        ignored: result
            .get("ignoredFiles")
            .and_then(|v| v.as_array())
            .map_or(0, Vec::len),
    })
}

/// Outcome of `session/applyFileRewind` (shape pinned live 2026-07-07:
/// `{applied: bool, preview: {…}, response: string}`; refusals keep
/// `applied:false` with the unsafe files in the embedded preview).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRewindOutcome {
    pub applied: bool,
    pub response: String,
    /// Unsafe rows of the embedded preview ("reason path"), for the refusal
    /// report.
    pub unsafe_files: Vec<RewindFile>,
}

pub fn parse_apply_file_rewind(result: &serde_json::Value) -> FileRewindOutcome {
    FileRewindOutcome {
        applied: result
            .get("applied")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        response: str_at(result, "response"),
        unsafe_files: result
            .get("preview")
            .map(|preview| rewind_files(preview, "unsafeFiles", "reason"))
            .unwrap_or_default(),
    }
}

/// Judge a `session/rewind` outcome. NEVER trust the envelope: a rewind to a
/// nonexistent checkpoint returns a SUCCESS envelope whose `response` reads
/// "Checkpoint … was not found." (pinned live 2026-07-07). The only reliable
/// signal is the `rewind.triggered` session event: `strategy:"active_chain"`
/// = applied, `strategy:"unavailable"` (+ `reason`, e.g.
/// "target_checkpoint_not_found") = nothing happened. Returns the failure
/// text, or None on success.
pub fn rewind_failure(
    strategy: Option<&str>,
    reason: Option<&str>,
    response: &str,
) -> Option<String> {
    match strategy {
        Some("unavailable") => Some(if response.is_empty() {
            format!("rewind unavailable: {}", reason.unwrap_or("unknown reason"))
        } else {
            response.to_string()
        }),
        Some(_) => None,
        // No rewind.triggered observed at all — do not claim success.
        None => Some(format!(
            "no rewind.triggered event observed (kernel said: {})",
            if response.is_empty() {
                "nothing"
            } else {
                response
            }
        )),
    }
}

/// One replayable history message from a `session/resume` result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayMessage {
    /// "user" | "assistant".
    pub role: String,
    /// Concatenated text parts, truncated to the preview cap.
    pub preview: String,
}

/// Extract the LAST up-to-`limit` renderable messages from a resume result's
/// `messages[]` (shape pinned live 2026-07-07: `{info:{role,…},
/// parts:[{type:"text", text}|{type:"reasoning"|"file"|"step-*",…}…]}`).
/// Only user/assistant roles count; only `type:"text"` parts contribute;
/// empty texts are skipped; previews are char-truncated to `cap` with "…".
pub fn parse_resume_messages(
    result: &serde_json::Value,
    limit: usize,
    cap: usize,
) -> Vec<ReplayMessage> {
    let Some(messages) = result.get("messages").and_then(|m| m.as_array()) else {
        return Vec::new();
    };
    let mut replay: Vec<ReplayMessage> = messages
        .iter()
        .filter_map(|message| {
            let role = message.pointer("/info/role")?.as_str()?;
            if role != "user" && role != "assistant" {
                return None;
            }
            let text = message
                .get("parts")?
                .as_array()?
                .iter()
                .filter(|part| part.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return None;
            }
            let mut preview: String = trimmed.chars().take(cap).collect();
            if trimmed.chars().count() > cap {
                preview.push('…');
            }
            Some(ReplayMessage {
                role: role.to_string(),
                preview,
            })
        })
        .collect();
    if replay.len() > limit {
        replay.drain(..replay.len() - limit);
    }
    replay
}

/// Standard base64 (RFC 4648, with padding) — hand-rolled to keep the
/// dependency tree flat; only used for the OSC52 clipboard payload.
pub fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// OSC52 clipboard-set sequence (`ESC ] 52 ; c ; <base64> BEL`) for `text`,
/// or None when text is empty. `max_b64` caps the encoded payload (~100KB by
/// convention — terminals truncate or reject oversized sequences); the SOURCE
/// text is truncated on a char boundary first so the base64 is always valid.
pub fn osc52_copy_sequence(text: &str, max_b64: usize) -> Option<String> {
    if text.is_empty() {
        return None;
    }
    let max_src = max_b64 / 4 * 3;
    let mut source = text;
    if source.len() > max_src {
        // Truncate to the last char boundary at or below the byte cap.
        let mut cut = max_src;
        while cut > 0 && !source.is_char_boundary(cut) {
            cut -= 1;
        }
        source = &source[..cut];
        if source.is_empty() {
            return None;
        }
    }
    Some(format!(
        "\x1b]52;c;{}\x07",
        base64_encode(source.as_bytes())
    ))
}

/// Parse a `session/list` result (`sessions[]{sessionId,title,workspace,
/// updatedAt,status,…}`) into picker rows: current-`cwd` sessions first,
/// then by recency — mirroring the db-backed `list_recent_sessions` order.
/// Sessions still `running` get a marker suffix so the picker can show it.
pub fn parse_session_list(result: &serde_json::Value, cwd: &str) -> Vec<SessionRow> {
    let Some(sessions) = result.get("sessions").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut rows: Vec<(bool, SessionRow)> = sessions
        .iter()
        .filter_map(|s| {
            let id = s.get("sessionId")?.as_str()?.to_string();
            let directory = s
                .pointer("/workspace/workspacePath")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let mut title = s
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            if title.is_empty() {
                title = directory.rsplit('/').next().unwrap_or_default().to_string();
            }
            if s.get("status").and_then(|v| v.as_str()) == Some("running") {
                title.push_str("  · running");
            }
            let time_updated = s
                .get("updatedAt")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            Some((
                directory == cwd,
                SessionRow {
                    id,
                    title,
                    directory,
                    time_updated,
                },
            ))
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.time_updated.cmp(&a.1.time_updated)));
    rows.into_iter().map(|(_, row)| row).collect()
}

/// Outcome of a `session/steer` request. The SUCCESS envelope's result is a
/// discriminated union (kernel `FKr`): `{kind:"queued",…}` means the input
/// entered the running turn; `{kind:"rejected", reason}` means it did NOT —
/// treating an ok envelope as success silently loses rejected input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteerOutcome {
    Queued,
    Rejected(String),
    /// Unrecognized result shape (older/newer kernel): assume queued rather
    /// than double-submitting the input.
    Unknown,
}

pub fn parse_steer_result(result: &serde_json::Value) -> SteerOutcome {
    match result.get("kind").and_then(|v| v.as_str()) {
        Some("queued") => SteerOutcome::Queued,
        Some("rejected") => SteerOutcome::Rejected(
            result
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("rejected")
                .to_string(),
        ),
        _ => SteerOutcome::Unknown,
    }
}

/// A kernel-reported slash command (`session/create`/`resume` result's
/// `slashCommands[]`), merged into `/` completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelCommand {
    pub name: String,
    pub description: String,
    pub input_hint: String,
}

pub fn parse_kernel_slash_commands(result: &serde_json::Value) -> Vec<KernelCommand> {
    result
        .get("slashCommands")
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|c| {
                    let name = c.get("name")?.as_str()?.to_string();
                    Some(KernelCommand {
                        name,
                        description: c
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        input_hint: c
                            .get("inputHint")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// An owned suggestion row: the local catalog merged with kernel-reported
/// commands (local implementations win on name collisions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashEntry {
    pub command: String,
    pub summary: String,
    pub route: String,
}

/// `/` completion over local + kernel commands. Local entries keep their
/// catalog order and priority; kernel commands come after, deduped by base
/// name (`/goal …` vs local `/goal`), showing their inputHint as the command
/// when it adds information.
pub fn slash_suggestions_merged(
    input: &str,
    limit: usize,
    kernel: &[KernelCommand],
) -> Vec<SlashEntry> {
    let query = input.trim();
    if query.is_empty() || !query.starts_with('/') || limit == 0 {
        return Vec::new();
    }
    let bare = query.trim_start_matches('/');
    let local_names: std::collections::HashSet<&str> = command_catalog()
        .iter()
        .filter_map(|item| item.command.strip_prefix('/'))
        .map(|rest| rest.split_whitespace().next().unwrap_or(rest))
        .collect();
    let mut catalog: Vec<SlashEntry> = command_catalog()
        .iter()
        .map(|item| SlashEntry {
            command: item.command.to_string(),
            summary: item.summary.to_string(),
            route: item.route.to_string(),
        })
        .collect();
    for command in kernel {
        if local_names.contains(command.name.as_str()) {
            continue;
        }
        let display = if command.input_hint.starts_with('/') {
            command.input_hint.clone()
        } else {
            format!("/{}", command.name)
        };
        catalog.push(SlashEntry {
            command: display,
            summary: command.description.clone(),
            route: "zcode".to_string(),
        });
    }
    let mut scored: Vec<(u8, usize, SlashEntry)> = Vec::new();
    for (index, item) in catalog.into_iter().enumerate() {
        let rank = if item.command.starts_with(query) {
            0
        } else if !bare.is_empty() && item.command.contains(bare) {
            1
        } else if is_subsequence(query, &item.command) {
            2
        } else {
            continue;
        };
        scored.push((rank, index, item));
    }
    scored.sort_by_key(|a| (a.0, a.1));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, _, item)| item)
        .collect()
}

/// One kernel TODO item (create/resume result's `todos[]` or a state push).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItem {
    pub text: String,
    pub done: bool,
}

/// Extract todos from a create/resume result or a `state.updated` patch.
/// Tolerates both `{content|text|title, status|completed}` item shapes; an
/// absent list -> empty (caller keeps its previous list only on pushes that
/// carry no `todos` key at all — an empty array is an explicit clear).
pub fn parse_todos(value: &serde_json::Value) -> Option<Vec<TodoItem>> {
    let list = value
        .get("todos")
        .or_else(|| value.pointer("/patch/todos"))?
        .as_array()?;
    Some(
        list.iter()
            .filter_map(|t| {
                let text = t
                    .get("content")
                    .or_else(|| t.get("text"))
                    .or_else(|| t.get("title"))?
                    .as_str()?
                    .to_string();
                let done = t
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.eq_ignore_ascii_case("completed") || s.eq_ignore_ascii_case("done"))
                    .or_else(|| t.get("completed").and_then(serde_json::Value::as_bool))
                    .unwrap_or(false);
                Some(TodoItem { text, done })
            })
            .collect(),
    )
}

/// One selectable model from the state push's `model.available[]`.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelChoice {
    pub label: String,
    pub provider: String,
    /// `available[].ref`, echoed back verbatim in `session/setModel`.
    pub reference: serde_json::Value,
}

/// The session control surface carried by a `state.updated` patch (all fields
/// optional — pushes are partial; the consumer merges non-empty fields).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionControls {
    pub mode: Option<String>,
    pub models: Vec<ModelChoice>,
    /// Current model's `modelId`.
    pub model_current: Option<String>,
    pub thought_levels: Vec<String>,
    pub thought_current: Option<String>,
}

/// Extract whatever control-surface state a `state.updated` push carries
/// (`reason:"mode_changed"` carries the full set; others may carry parts).
/// None when the patch has none of the control keys.
pub fn app_state_controls(params: &serde_json::Value) -> Option<SessionControls> {
    let patch = params.get("patch")?;
    let controls = SessionControls {
        mode: patch
            .pointer("/mode/current")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        models: patch
            .pointer("/model/available")
            .and_then(|v| v.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|m| {
                        Some(ModelChoice {
                            label: m.get("label")?.as_str()?.to_string(),
                            provider: m
                                .get("providerLabel")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string(),
                            reference: m.get("ref")?.clone(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        model_current: patch
            .pointer("/model/current/modelId")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        thought_levels: patch
            .pointer("/thoughtLevel/available")
            .and_then(|v| v.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|l| l.get("value").and_then(|v| v.as_str()).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        thought_current: patch
            .pointer("/thoughtLevel/current")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    };
    let empty = controls.mode.is_none()
        && controls.models.is_empty()
        && controls.model_current.is_none()
        && controls.thought_levels.is_empty()
        && controls.thought_current.is_none();
    if empty {
        None
    } else {
        Some(controls)
    }
}

/// A decoded inbound line: a response to one of our requests, a session
/// event (the token stream), a session-level state update, a server→client
/// request (the kernel asking *us* something), or ignorable.
// The Event variant (AppServerEvent) is the largest, but every message is
// decoded and matched immediately (never stored in bulk), so boxing it would
// only add a heap allocation per streamed event on the hot path.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum AppServerMessage {
    /// Response to request `id`; `error` set means the request failed.
    Response {
        id: u64,
        result: Option<serde_json::Value>,
        error: Option<String>,
    },
    /// `session/event` payload — the streaming turn events.
    Event(AppServerEvent),
    /// `state.updated` — session status/mode/model/context watermark.
    StateUpdated(serde_json::Value),
    /// ZCode 3.5.3 V4 conversation snapshot/delta transport frame.
    V4Frame(serde_json::Value),
    /// Server→client request: carries `method` AND an envelope `id` we must
    /// echo back in the reply. The kernel uses STRING ids here (`"server-1"`,
    /// `"server-2"`, …) so the id is kept as raw JSON and returned verbatim
    /// (`interaction/requestUserInput` is the permission-approval channel).
    ServerRequest {
        id: serde_json::Value,
        method: String,
        params: serde_json::Value,
    },
    /// A recognized-but-uninteresting line; skipped without failing.
    Other,
}

/// One `session/event` payload. `kind` drives dispatch; the rest are set only
/// for the kinds that carry them (tool events, `result`). Defaulted so tests
/// and non-tool events can build it with just `kind`/`delta`/`done`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppServerEvent {
    pub kind: String,
    pub delta: String,
    pub done: bool,
    /// `toolName` (tool_input_start / tool_call / started).
    pub tool_name: Option<String>,
    /// `toolCallId` — correlates a tool across its start/input/result events.
    pub tool_call_id: Option<String>,
    /// `result.result.content` — the tool's output text (on `kind=result`).
    pub output: Option<String>,
    /// `result.result.success` — tool succeeded (on `kind=result`).
    pub success: Option<bool>,
    /// `result.duration` — tool wall time in ms (on `kind=result`).
    pub duration_ms: Option<u64>,
    /// `payload.fileCount` — files captured by a `checkpoint.created`
    /// session-level event (surfaced via `params.type` passthrough).
    pub file_count: Option<u64>,
    /// `payload.checkpointId` (`checkpoint.created`) — the rewind target id;
    /// captured per session for the /rewind picker.
    pub checkpoint_id: Option<String>,
    /// `payload.targetMessageId` (falling back to `payload.messageId`) of a
    /// `checkpoint.created` — the turn's user message, needed as the
    /// `{kind:"message"}` target of the conversation-scope leg.
    pub target_message_id: Option<String>,
    /// `payload.strategy` (`rewind.triggered`) — "active_chain" on a real
    /// rewind, "unavailable" when the kernel could NOT rewind. Pinned live
    /// 2026-07-07: a failed rewind still returns a SUCCESS envelope, so this
    /// event is the only trustworthy outcome signal.
    pub strategy: Option<String>,
    /// `payload.reason` (`rewind.triggered`) — e.g. "target_in_active_chain",
    /// "target_checkpoint_not_found".
    pub reason: Option<String>,
    /// `payload.taskId` — `background_task_*` events used by ZCode 3.3.4
    /// subagent/bash backgrounding. Decoded so future app-server deliveries are
    /// never silently dropped.
    pub task_id: Option<String>,
    /// `payload.command` — the backgrounded shell command.
    pub command: Option<String>,
    /// `payload.status` — background task status (running|completed|lost…).
    pub status: Option<String>,
    /// `payload.pid` — background task process id.
    pub pid: Option<u64>,
}

/// Decode a single inbound protocol line. Unparseable lines -> None (skip).
pub fn decode_app_message(line: &str) -> Option<AppServerMessage> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    // Server→client request: method AND id together (the kernel expects a
    // reply). Must be checked before the Response branch — its envelope id is
    // a string ("server-N"), and a u64-only path would drop it on the floor
    // (observed: ignored interaction requests hang plan-mode turns until the
    // 600s backstop).
    if let (Some(method), Some(id)) = (
        value.get("method").and_then(|m| m.as_str()),
        value.get("id"),
    ) {
        return Some(AppServerMessage::ServerRequest {
            id: id.clone(),
            method: method.to_string(),
            params: value
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        });
    }
    // Response: has an `id` and either result or error.
    if let Some(id) = value.get("id").and_then(serde_json::Value::as_u64) {
        let error = value.get("error").map(|err| {
            err.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("app-server error")
                .to_string()
        });
        let result = value.get("result").cloned();
        return Some(AppServerMessage::Response { id, result, error });
    }
    match value.get("method").and_then(|m| m.as_str()) {
        Some("session/event") => {
            let payload = value.pointer("/params/payload")?;
            let str_field = |key: &str| {
                payload
                    .get(key)
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            };
            // Streaming payloads (model.streaming) carry their own `kind`;
            // session-level events (checkpoint.created, turn.*, …) do NOT —
            // pass `params.type` through as the kind so they are consumable
            // instead of dropped as unparseable (neither present -> skip).
            let kind = payload
                .get("kind")
                .and_then(|k| k.as_str())
                .or_else(|| value.pointer("/params/type").and_then(|t| t.as_str()))?
                .to_string();
            Some(AppServerMessage::Event(AppServerEvent {
                kind,
                delta: payload
                    .get("delta")
                    .and_then(|d| d.as_str())
                    .unwrap_or_default()
                    .to_string(),
                done: payload
                    .get("done")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                tool_name: str_field("toolName"),
                tool_call_id: str_field("toolCallId"),
                // `result` events nest the tool output under /result/content.
                output: payload
                    .pointer("/result/content")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                success: payload
                    .pointer("/result/success")
                    .and_then(serde_json::Value::as_bool),
                duration_ms: payload.get("duration").and_then(serde_json::Value::as_u64),
                file_count: payload.get("fileCount").and_then(serde_json::Value::as_u64),
                checkpoint_id: str_field("checkpointId"),
                target_message_id: str_field("targetMessageId").or_else(|| str_field("messageId")),
                strategy: str_field("strategy"),
                reason: str_field("reason"),
                task_id: str_field("taskId"),
                command: str_field("command"),
                status: str_field("status"),
                pid: payload.get("pid").and_then(serde_json::Value::as_u64),
            }))
        }
        Some("state.updated") => Some(AppServerMessage::StateUpdated(
            value
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        )),
        Some("v4/conversation/frame") => Some(AppServerMessage::V4Frame(
            value
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        )),
        _ => Some(AppServerMessage::Other),
    }
}

/// The kernel's user-input interaction request (plan approval): questions
/// with options, answered as `{requestId, answers:{header:value}}`.
pub const INTERACTION_METHOD: &str = "interaction/requestUserInput";
/// The kernel's tool-permission request (gated side-effect tools, e.g. Write
/// in build mode): flat options each carrying a ready-made `response` object,
/// answered by echoing the chosen option's `response` VERBATIM (the kernel's
/// result schema is strict — adding any key, even requestId, is rejected).
pub const PERMISSION_METHOD: &str = "interaction/requestPermission";
/// ZCode 0.16.3 asks the client for runtime preferences while materializing a
/// create/resume session. Leaving this unanswered makes session/create time
/// out after 15 seconds and disables app-server streaming.
pub const RUNTIME_PREFERENCES_METHOD: &str = "session/requestRuntimePreferences";

/// Exact 0.16.3 runtime-preferences reply. These are the same compatibility
/// defaults the kernel uses when an older client returns Method not found.
/// The optional integratedTerminalShell is deliberately absent so the kernel
/// selects the host shell normally.
pub fn encode_runtime_preferences_reply(
    envelope_id: &serde_json::Value,
    method: &str,
) -> Option<String> {
    if method != RUNTIME_PREFERENCES_METHOD {
        return None;
    }
    Some(
        serde_json::json!({
            "id": envelope_id,
            "result": {
                "nativeSearchEnhancementsEnabled": true,
                "memoryEnabled": false,
                "askUserQuestionAutoResolutionEnabled": true,
                "modelContextBudgetStrategy": "preflight-v1",
            }
        })
        .to_string(),
    )
}

/// How the reply's `result` must be built — the two interaction methods use
/// incompatible result schemas (both pinned live 2026-07-07).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionReply {
    /// requestUserInput: `{"requestId":…, "answers":{<header>:<value>}}`.
    Answers,
    /// requestPermission: the chosen option's `response` object verbatim.
    Permission,
}

/// One selectable option of an interaction request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionOption {
    pub label: String,
    /// Answer value (requestUserInput) or optionId (requestPermission).
    pub value: String,
    pub description: String,
    /// requestPermission only: the pre-baked reply `result` for this option.
    pub response: Option<serde_json::Value>,
}

/// One question of an interaction request. `header` doubles as the answer key
/// in the reply (`answers: {<header>: <option value>}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionQuestion {
    pub header: String,
    pub question: String,
    pub options: Vec<InteractionOption>,
}

/// A parsed interaction request (kernel 0.15.0, pinned 2026-07-07): the
/// kernel re-sends the same `request_id` under fresh envelope ids with
/// backoff until answered, so consumers must dedupe on `request_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct InteractionRequest {
    pub request_id: String,
    /// Top line, e.g. "Tool ExitPlanMode requires user interaction" or the
    /// permission reason ("Tool has side effects and requires approval").
    pub prompt: String,
    /// `schema.interaction` (e.g. "plan_approval"), or "permission".
    pub interaction: String,
    pub tool_name: String,
    /// `input.plan` — the plan text under review (plan_approval).
    pub plan: Option<String>,
    pub questions: Vec<InteractionQuestion>,
    pub reply: InteractionReply,
    /// Index of a protocol-level decline option (permission `kind:"deny"`),
    /// answered on Esc. None -> declining falls back to stopping the turn.
    pub deny_index: Option<usize>,
}

fn str_at(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

/// Parse a server→client interaction request by method. Requires `requestId`
/// and at least one answerable option — anything less returns None and the
/// caller leaves the request unanswered (the kernel's retry keeps it alive
/// for a future, more capable client).
pub fn parse_interaction_request(
    method: &str,
    params: &serde_json::Value,
) -> Option<InteractionRequest> {
    match method {
        INTERACTION_METHOD => parse_user_input_request(params),
        PERMISSION_METHOD => parse_permission_request(params),
        _ => None,
    }
}

fn parse_user_input_request(params: &serde_json::Value) -> Option<InteractionRequest> {
    let request_id = params.get("requestId")?.as_str()?.to_string();
    let questions: Vec<InteractionQuestion> = params
        .get("questions")?
        .as_array()?
        .iter()
        .filter_map(|q| {
            let options: Vec<InteractionOption> = q
                .get("options")?
                .as_array()?
                .iter()
                .filter_map(|o| {
                    let value = o.get("value")?.as_str()?.to_string();
                    Some(InteractionOption {
                        label: {
                            let label = str_at(o, "label");
                            if label.is_empty() {
                                value.clone()
                            } else {
                                label
                            }
                        },
                        value,
                        description: str_at(o, "description"),
                        response: None,
                    })
                })
                .collect();
            if options.is_empty() {
                return None;
            }
            Some(InteractionQuestion {
                header: str_at(q, "header"),
                question: str_at(q, "question"),
                options,
            })
        })
        .collect();
    if questions.is_empty() {
        return None;
    }
    Some(InteractionRequest {
        request_id,
        prompt: str_at(params, "prompt"),
        interaction: params
            .pointer("/schema/interaction")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        tool_name: str_at(params, "toolName"),
        plan: params
            .pointer("/input/plan")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        questions,
        reply: InteractionReply::Answers,
        deny_index: None,
    })
}

fn parse_permission_request(params: &serde_json::Value) -> Option<InteractionRequest> {
    let request_id = params.get("requestId")?.as_str()?.to_string();
    let tool_name = str_at(params, "toolName");
    let options: Vec<InteractionOption> = params
        .get("options")?
        .as_array()?
        .iter()
        .filter_map(|o| {
            let option_id = o.get("optionId")?.as_str()?.to_string();
            Some(InteractionOption {
                label: {
                    let name = str_at(o, "name");
                    if name.is_empty() {
                        option_id.clone()
                    } else {
                        name
                    }
                },
                value: option_id,
                description: str_at(o, "description"),
                response: Some(o.get("response")?.clone()),
            })
        })
        .collect();
    if options.is_empty() {
        return None;
    }
    let deny_index = params
        .get("options")
        .and_then(|v| v.as_array())
        .and_then(|list| {
            list.iter()
                .position(|o| o.get("kind").and_then(|k| k.as_str()) == Some("deny"))
        });
    // What the tool wants to do, condensed: "Write  w.txt · hi (risk medium)".
    let summary = params
        .get("input")
        .map(|input| tool_input_summary(&input.to_string()))
        .unwrap_or_default();
    let risk = str_at(params, "riskLevel");
    let mut question = tool_name.clone();
    if !summary.is_empty() {
        question.push_str(&format!("  {summary}"));
    }
    if !risk.is_empty() {
        question.push_str(&format!("  (risk {risk})"));
    }
    Some(InteractionRequest {
        request_id,
        prompt: str_at(params, "reason"),
        interaction: "permission".to_string(),
        tool_name,
        plan: None,
        questions: vec![InteractionQuestion {
            header: String::new(),
            question,
            options,
        }],
        reply: InteractionReply::Permission,
        deny_index,
    })
}

/// Encode the reply for `selected` (an index into the first question's
/// options) as one compact JSON line; the envelope `id` is echoed back
/// verbatim (string or number). Returns None if `selected` is out of bounds
/// or the option lacks its reply payload.
pub fn encode_interaction_reply(
    envelope_id: &serde_json::Value,
    request: &InteractionRequest,
    selected: usize,
) -> Option<String> {
    let result = match request.reply {
        InteractionReply::Answers => {
            // The selection answers the first question; any further questions
            // get their first option (observed payloads carry exactly one).
            let mut answers = serde_json::Map::new();
            for (index, question) in request.questions.iter().enumerate() {
                let pick = if index == 0 { selected } else { 0 };
                let option = question.options.get(pick)?;
                answers.insert(
                    question.header.clone(),
                    serde_json::Value::String(option.value.clone()),
                );
            }
            serde_json::json!({
                "requestId": request.request_id,
                "answers": serde_json::Value::Object(answers),
            })
        }
        // Strict kernel schema: the option's response object and NOTHING else.
        InteractionReply::Permission => request
            .questions
            .first()?
            .options
            .get(selected)?
            .response
            .clone()?,
    };
    Some(serde_json::json!({ "id": envelope_id, "result": result }).to_string())
}

/// A compact one-line summary of a tool's JSON input, for the chip header:
/// `{"file_path":"/a/b/notes.txt"}` -> `notes.txt`. Joins the string values
/// (path-basenamed), collapses whitespace, and caps at ~48 chars. Falls back to
/// the trimmed raw input when it isn't a JSON object.
pub fn tool_input_summary(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let raw = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(serde_json::Value::Object(map)) => {
            let parts: Vec<&str> = map
                .values()
                .filter_map(|v| v.as_str())
                .map(|s| s.rsplit('/').next().unwrap_or(s))
                .filter(|s| !s.is_empty())
                .collect();
            if parts.is_empty() {
                trimmed.to_string()
            } else {
                parts.join(" ")
            }
        }
        _ => trimmed.to_string(),
    };
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 48 {
        let mut out: String = collapsed.chars().take(47).collect();
        out.push('…');
        out
    } else {
        collapsed
    }
}

/// Extract `session.sessionId` from a `session/create` result.
pub fn app_session_id_from_result(result: &serde_json::Value) -> Option<String> {
    result
        .pointer("/session/sessionId")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Whether a `state.updated` marks the running turn as finished. The kernel
/// signals turn completion with `reason == "prompt_completed"` — there is no
/// `finish` session/event — so this is the authoritative turn terminator. A
/// status patch to the unambiguous terminal `completed` is a version-tolerant
/// fallback. `idle`/`ready` are deliberately NOT treated as turn-end: the
/// kernel can emit them as a settling state on a reused session *before* tokens
/// flow, which would finalize the turn prematurely as "(no output)".
pub fn app_state_is_turn_end(params: &serde_json::Value) -> bool {
    if params.get("reason").and_then(|r| r.as_str()) == Some("prompt_completed") {
        return true;
    }
    params.pointer("/patch/status").and_then(|s| s.as_str()) == Some("completed")
}

/// Whether a `state.updated` marks the turn as ended *abnormally* (error /
/// failed / aborted / cancelled / interrupted), via `reason` or `patch/status`.
/// Returns the offending word so the turn can be closed with a note instead of
/// hanging on a false "streaming" spinner until the 600s backstop fires.
pub fn app_state_turn_error(params: &serde_json::Value) -> Option<String> {
    fn is_bad(s: &str) -> bool {
        const BAD: [&str; 6] = [
            "error",
            "failed",
            "aborted",
            "cancelled",
            "canceled",
            "interrupted",
        ];
        BAD.contains(&s.to_ascii_lowercase().as_str())
    }
    for candidate in [
        params.get("reason").and_then(|r| r.as_str()),
        params.pointer("/patch/status").and_then(|s| s.as_str()),
    ]
    .into_iter()
    .flatten()
    {
        if is_bad(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

/// Best-effort context watermark from a `state.updated` payload. The exact
/// JSON path is not contractual and shifts across kernel builds, so this
/// walks the tree for the first object carrying a used/window numeric pair
/// under any known key name. Missing/zero window -> None (no watermark, no
/// crash), which the caller treats as "leave the last value in place".
pub fn app_state_watermark(params: &serde_json::Value) -> Option<(u64, u64)> {
    const USED_KEYS: [&str; 4] = ["contextUsed", "used", "tokensUsed", "contextTokens"];
    const WINDOW_KEYS: [&str; 4] = ["contextWindow", "window", "total", "maxTokens"];
    fn walk(value: &serde_json::Value) -> Option<(u64, u64)> {
        match value {
            serde_json::Value::Object(map) => {
                let used = USED_KEYS
                    .iter()
                    .find_map(|key| map.get(*key).and_then(serde_json::Value::as_u64));
                let window = WINDOW_KEYS
                    .iter()
                    .find_map(|key| map.get(*key).and_then(serde_json::Value::as_u64));
                if let (Some(used), Some(window)) = (used, window) {
                    if window > 0 {
                        return Some((used, window));
                    }
                }
                map.values().find_map(walk)
            }
            serde_json::Value::Array(items) => items.iter().find_map(walk),
            _ => None,
        }
    }
    walk(params)
}

/// One tool invocation within a turn, correlated across its start/input/result
/// events by `call_id`. `finished` flips when the `result` event lands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppToolCall {
    pub call_id: String,
    pub name: String,
    /// Accumulated `tool_input_delta` JSON (the arguments).
    pub input: String,
    /// Tool output text (`result.result.content`).
    pub output: String,
    pub success: bool,
    pub duration_ms: Option<u64>,
    pub finished: bool,
}

/// What visibly changed when a turn applied one event — lets the UI know
/// exactly when to grow text, show a tool chip, or drop a finished tool into
/// the transcript, without re-diffing the whole turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnDelta {
    /// Nothing the UI needs to react to.
    None,
    /// Visible answer text grew.
    Text,
    /// Reasoning text grew (work-panel only).
    Reasoning,
    /// `tools[idx]` just began (show a running chip).
    ToolStarted(usize),
    /// `tools[idx]` just finished (persist it, foldable, to the transcript).
    ToolFinished(usize),
    /// The turn completed.
    Done,
}

/// Accumulates a streaming turn from session/event deltas. Body text arrives as
/// `text_delta` (like Anthropic content_block_delta); tool calls arrive as a
/// start/input/result sequence correlated by `toolCallId`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppServerTurn {
    pub text: String,
    pub reasoning: String,
    pub tools: Vec<AppToolCall>,
    pub done: bool,
    /// `checkpoint.created` events seen this turn (one per gated tool write).
    pub checkpoints: u64,
    /// Sum of those events' `fileCount` — the turn's files-changed total.
    pub files_changed: u64,
}

impl AppServerTurn {
    fn tool_index(&self, call_id: &str) -> Option<usize> {
        self.tools.iter().position(|t| t.call_id == call_id)
    }

    /// Apply one event, returning what changed so the caller can react.
    pub fn apply(&mut self, event: &AppServerEvent) -> TurnDelta {
        match event.kind.as_str() {
            "text_delta" => {
                self.text.push_str(&event.delta);
                TurnDelta::Text
            }
            "reasoning_delta" => {
                self.reasoning.push_str(&event.delta);
                TurnDelta::Reasoning
            }
            // First sighting of a tool (start marker or full call) registers it.
            "tool_input_start" | "tool_call" => {
                let Some(call_id) = event.tool_call_id.as_deref() else {
                    return TurnDelta::None;
                };
                if let Some(idx) = self.tool_index(call_id) {
                    if let Some(name) = &event.tool_name {
                        if !name.is_empty() {
                            self.tools[idx].name = name.clone();
                        }
                    }
                    TurnDelta::None
                } else {
                    self.tools.push(AppToolCall {
                        call_id: call_id.to_string(),
                        name: event.tool_name.clone().unwrap_or_default(),
                        ..Default::default()
                    });
                    TurnDelta::ToolStarted(self.tools.len() - 1)
                }
            }
            "tool_input_delta" => {
                if let Some(call_id) = event.tool_call_id.as_deref() {
                    if let Some(idx) = self.tool_index(call_id) {
                        self.tools[idx].input.push_str(&event.delta);
                    }
                }
                TurnDelta::None
            }
            "result" => {
                let Some(call_id) = event.tool_call_id.as_deref() else {
                    return TurnDelta::None;
                };
                let Some(idx) = self.tool_index(call_id) else {
                    return TurnDelta::None;
                };
                let tool = &mut self.tools[idx];
                if let Some(output) = &event.output {
                    tool.output = output.clone();
                }
                tool.success = event.success.unwrap_or(true);
                tool.duration_ms = event.duration_ms;
                tool.finished = true;
                TurnDelta::ToolFinished(idx)
            }
            "finish" => {
                self.done = true;
                TurnDelta::Done
            }
            "text_end" if event.done => {
                self.done = true;
                TurnDelta::Done
            }
            // Session-level checkpoint (params.type passthrough): one per
            // gated tool write; fileCount sums into the turn's change total
            // for the finalize-time "N file(s) changed" note.
            "checkpoint.created" => {
                self.checkpoints += 1;
                self.files_changed += event.file_count.unwrap_or(0);
                TurnDelta::None
            }
            // input_end, scheduled, started, batch, tool_result, unknown: no-op.
            _ => TurnDelta::None,
        }
    }
}

/// Why the app-server path was abandoned (all trigger a --prompt fallback).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppServerUnavailable {
    Spawn(String),
    Handshake(String),
    Protocol(String),
    Disconnected,
}

impl std::fmt::Display for AppServerUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(why) => write!(f, "app-server did not start: {why}"),
            Self::Handshake(why) => write!(f, "app-server handshake failed: {why}"),
            Self::Protocol(why) => write!(f, "app-server protocol error: {why}"),
            Self::Disconnected => write!(f, "app-server connection closed"),
        }
    }
}

/// Whether prompts take the app-server streaming path. ON by default since
/// the graduation (streaming-graduation change): the path is a functional
/// superset of `--prompt` (true streaming + permission approval + session
/// controls + steer) and seamlessly downgrades on any failure. Only an
/// explicit opt-out disables it; `=1/true/on` stays accepted for the scripts
/// and wrappers written while it was opt-in.
pub fn app_server_enabled<F>(env_lookup: F) -> bool
where
    F: Fn(&str) -> Option<String>,
{
    !env_lookup("ZCODE_TUI_APP_SERVER").is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "off" | "false" | "no"
        )
    })
}

/// Append-only debug log, enabled by `ZCODE_TUI_LOG=<file path>`. Zero
/// overhead when unset (`from_env` is checked once per owner; the disabled
/// path is a single `is_none()`). Write failures are silently ignored —
/// diagnostics must never break the TUI.
///
/// REDACTION DISCIPLINE: outbound entries carry METHOD NAMES ONLY — request
/// params are never serialized (session/create·resume params embed
/// `runtimeModel` with the provider apiKey). Inbound summaries are truncated
/// and structural (class/kind/reason/id), never raw lines.
#[derive(Clone)]
pub struct DebugLog {
    file: Arc<Mutex<fs::File>>,
}

impl DebugLog {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("ZCODE_TUI_LOG")?;
        if path.is_empty() {
            return None;
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        Some(Self {
            file: Arc::new(Mutex::new(file)),
        })
    }

    /// Append one timestamped line; failures are dropped on the floor.
    pub fn line(&self, text: &str) {
        if let Ok(mut file) = self.file.lock() {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let day_secs = (ms / 1000) % 86_400;
            let _ = writeln!(
                file,
                "{:02}:{:02}:{:02}.{:03} {text}",
                day_secs / 3600,
                (day_secs / 60) % 60,
                day_secs % 60,
                ms % 1000
            );
        }
    }
}

/// Outbound request log line: the method name and id, NOTHING else — params
/// stay out of the log by construction (see [`DebugLog`] redaction notes).
pub fn log_line_outbound(method: &str, id: u64) -> String {
    format!("-> {method} (id {id})")
}

/// Structural outbound request log. V4 commands add type/revision only; the
/// payload is deliberately never serialized because sendText contains the
/// user's prompt and other commands may grow credential-bearing fields.
pub fn log_line_outbound_request(method: &str, id: u64, params: &serde_json::Value) -> String {
    if method != "v4/command" {
        return log_line_outbound(method, id);
    }
    let kind = params.get("type").and_then(|v| v.as_str()).unwrap_or("-");
    let revision = params
        .get("baseRevision")
        .and_then(serde_json::Value::as_u64)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string());
    format!("-> v4/command type={kind} rev={revision} (id {id})")
}

/// Inbound message summary: message class + structural fields, truncated.
/// Result/params bodies are never serialized.
pub fn log_line_inbound(message: &AppServerMessage) -> String {
    match message {
        AppServerMessage::Response {
            id,
            error: Some(error),
            ..
        } => format!("<- response id {id} ERR {}", truncate_chars(error, 160)),
        AppServerMessage::Response { id, .. } => format!("<- response id {id} ok"),
        AppServerMessage::Event(event) => {
            let mut line = format!("<- event {}", event.kind);
            if !event.delta.is_empty() {
                line.push_str(&format!(" +{}b", event.delta.len()));
            }
            if let Some(name) = event.tool_name.as_deref().filter(|name| !name.is_empty()) {
                line.push_str(&format!(" tool={}", truncate_chars(name, 40)));
            }
            if let Some(count) = event.file_count {
                line.push_str(&format!(" files={count}"));
            }
            line
        }
        AppServerMessage::StateUpdated(params) => format!(
            "<- state.updated reason={}",
            params.get("reason").and_then(|r| r.as_str()).unwrap_or("-")
        ),
        AppServerMessage::V4Frame(params) => {
            let payload = params.pointer("/frame/payload");
            let kind = payload
                .and_then(|value| value.get("kind"))
                .and_then(|value| value.as_str())
                .unwrap_or("-");
            let revision = payload
                .and_then(|value| value.pointer("/snapshot/revision"))
                .and_then(serde_json::Value::as_u64)
                .or_else(|| {
                    payload
                        .and_then(|value| value.get("deltas"))
                        .and_then(|value| value.as_array())
                        .and_then(|deltas| {
                            deltas.iter().rev().find_map(|delta| {
                                delta
                                    .pointer("/patch/revision")
                                    .and_then(serde_json::Value::as_u64)
                            })
                        })
                })
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string());
            format!("<- v4/conversation/frame kind={kind} rev={revision}")
        }
        AppServerMessage::ServerRequest { id, method, .. } => {
            format!(
                "<- server-request {method} id {}",
                truncate_chars(&id.to_string(), 40)
            )
        }
        AppServerMessage::Other => "<- other".to_string(),
    }
}

/// A long-lived connection to `zcode app-server`: one child process (own
/// process group), a reader thread decoding inbound lines, and a stdin
/// handle for requests. Requests get monotonic ids.
pub struct AppServerConn {
    child: Arc<Mutex<Child>>,
    stdin: std::process::ChildStdin,
    receiver: Receiver<AppServerMessage>,
    /// Messages read while blocking for a specific response id are stashed
    /// here so `poll` still delivers them afterwards (e.g. early events).
    pending: VecDeque<AppServerMessage>,
    next_id: u64,
    alive: bool,
    /// ZCODE_TUI_LOG debug log (None = disabled, zero overhead).
    log: Option<DebugLog>,
}

impl AppServerConn {
    pub fn spawn(zcode_bin: &str) -> std::result::Result<Self, AppServerUnavailable> {
        let mut process = Command::new(zcode_bin);
        process
            .arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            process.process_group(0);
        }
        let mut child = process
            .spawn()
            .map_err(|error| AppServerUnavailable::Spawn(error.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AppServerUnavailable::Spawn("no stdin pipe".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AppServerUnavailable::Spawn("no stdout pipe".to_string()))?;
        let log = DebugLog::from_env();
        let reader_log = log.clone();
        let (sender, receiver) = channel();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if let Some(message) = decode_app_message(&line) {
                    if let Some(log) = &reader_log {
                        log.line(&log_line_inbound(&message));
                    }
                    if sender.send(message).is_err() {
                        break;
                    }
                }
            }
        });
        if let Some(log) = &log {
            log.line("app-server spawned");
        }
        Ok(Self {
            child: Arc::new(Mutex::new(child)),
            stdin,
            receiver,
            pending: VecDeque::new(),
            next_id: 1,
            alive: true,
            log,
        })
    }

    fn write_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> std::result::Result<u64, AppServerUnavailable> {
        let id = self.next_id;
        self.next_id += 1;
        // Method name only — params may carry credentials (runtimeModel).
        if let Some(log) = &self.log {
            log.line(&log_line_outbound_request(method, id, &params));
        }
        let line = format!("{}\n", encode_app_request(id, method, params));
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|_| {
                self.alive = false;
                AppServerUnavailable::Disconnected
            })?;
        Ok(id)
    }

    /// Fire-and-forget request; the response arrives via `poll`.
    pub fn send(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> std::result::Result<u64, AppServerUnavailable> {
        self.write_request(method, params)
    }

    /// Write a pre-encoded line verbatim (plus framing newline). Replies to
    /// server→client requests echo the kernel's own envelope id — string ids
    /// like "server-1" — so they bypass the u64 request-id counter.
    pub fn reply(&mut self, line: &str) -> std::result::Result<(), AppServerUnavailable> {
        // Marker only — the reply body echoes kernel-provided payloads.
        if let Some(log) = &self.log {
            log.line("-> server-request reply");
        }
        self.stdin
            .write_all(format!("{line}\n").as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|_| {
                self.alive = false;
                AppServerUnavailable::Disconnected
            })
    }

    /// Send a request and block for its response, stashing any other
    /// messages that arrive meanwhile. Used for the fast create/subscribe
    /// handshake; the streaming turn uses `poll` instead.
    pub fn request_blocking(
        &mut self,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> std::result::Result<serde_json::Value, AppServerUnavailable> {
        let want = self.write_request(method, params)?;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(AppServerUnavailable::Handshake(format!(
                    "{method} timed out"
                )));
            }
            match self.receiver.recv_timeout(remaining) {
                Ok(AppServerMessage::Response { id, result, error }) if id == want => {
                    return match error {
                        Some(message) => Err(AppServerUnavailable::Protocol(message)),
                        None => Ok(result.unwrap_or(serde_json::Value::Null)),
                    };
                }
                Ok(other) => self.pending.push_back(other),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return Err(AppServerUnavailable::Handshake(format!(
                        "{method} timed out"
                    )));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    self.alive = false;
                    return Err(AppServerUnavailable::Disconnected);
                }
            }
        }
    }

    /// Non-blocking: next buffered or newly-arrived message, if any.
    pub fn poll(&mut self) -> Option<AppServerMessage> {
        if let Some(message) = self.pending.pop_front() {
            return Some(message);
        }
        match self.receiver.try_recv() {
            Ok(message) => Some(message),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.alive = false;
                None
            }
        }
    }

    pub fn is_alive(&self) -> bool {
        self.alive
    }

    pub fn cancel(&self) {
        if let Ok(mut child) = self.child.lock() {
            #[cfg(unix)]
            unsafe {
                libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
            }
            let _ = child.kill();
            // Reap the killed process so it does not linger as a <defunct>
            // zombie for the (possibly hours-long) rest of the TUI session.
            // After SIGKILL to the group this returns promptly.
            let _ = child.wait();
        }
    }
}

impl Drop for AppServerConn {
    fn drop(&mut self) {
        self.cancel();
    }
}
