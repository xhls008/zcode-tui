use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::ffi::OsStr;
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

mod protocol;

pub use protocol::*;

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
    let home = user_home_dir();
    detect_auth_status_with(|key| std::env::var(key).ok(), home.as_deref())
}

/// Resolve a user home on Unix and Windows without adding a platform-specific
/// dependency. PowerShell and cmd commonly expose USERPROFILE but not HOME.
pub fn user_home_dir_from(home: Option<&OsStr>, user_profile: Option<&OsStr>) -> Option<PathBuf> {
    home.filter(|value| !value.is_empty())
        .or_else(|| user_profile.filter(|value| !value.is_empty()))
        .map(PathBuf::from)
}

pub fn user_home_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME");
    let user_profile = std::env::var_os("USERPROFILE");
    user_home_dir_from(home.as_deref(), user_profile.as_deref())
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
        | "sessions" | "mode" | "theme" | "resume" | "new" | "model" | "think" | "compact"
        | "usage" | "update" | "copy" | "rewind" | "agents" => Ok(InputAction::Local(parts)),
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
            command: "/theme",
            summary: "list or switch built-in theme",
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
            command: "/agents",
            summary: "inspect parent, Subagents, and cancellable background work",
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
    r#"zcode-tui help

keyboard shortcuts:
  Enter                        accept a suggestion or send the prompt
                               while thinking: plain text steers the answer
  Ctrl+J                       insert a newline without sending
  Ctrl+P                       open the command palette
  Tab / Up / Down              navigate and accept suggestions
  Shift+Tab                    cycle permission mode
  Left/Right Home/End          move the input cursor
  Ctrl+A / Ctrl+E              jump to start / end of input
  Ctrl+G                       edit the prompt in $VISUAL or $EDITOR
  Ctrl+R                       reverse-search input history
  Ctrl+X, then p               command palette
  Ctrl+X, then h               help
  Ctrl+X, then e               external editor
  Ctrl+X, then x               clear conversation
  Ctrl+X, then u               clear input
  Ctrl+X, then y               copy last assistant reply
  Ctrl+X, then q               quit
  Mouse drag                   terminal-native text selection
  Mouse wheel                  scroll terminal history
  Cmd+C / Ctrl+Shift+C         system terminal copy
  Esc                          close popup or cancel running job
  ?                            open/close this help when input is empty
  Help: ↑/↓, j/k             scroll one line
        PgUp/PgDn, Home/End    scroll by page or jump to edge

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
  /theme [list|dark|light|tsinghua|pku]
                               list or persistently switch the color theme
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
  /agents                      inspect parent/Subagents/Background (read-only;
                               Tab switches, Enter details, r refreshes,
                               x cancels eligible official taskIds)
  /new                         start a fresh session; context resets
  /editor                      edit current prompt in $VISUAL or $EDITOR
  /clear                       clear this screen
  /exit                        quit

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

    fn separate(out: &mut Vec<StyledLine>) {
        if out.last().is_some_and(|line| !line.spans.is_empty()) {
            out.push(StyledLine {
                spans: Vec::new(),
                kind: MdLineKind::Text,
            });
        }
    }

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
                separate(&mut out);
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
                separate(&mut out);
            }
            Event::Start(Tag::Strong) => strong += 1,
            Event::End(TagEnd::Strong) => strong = strong.saturating_sub(1),
            Event::Start(Tag::Emphasis) => emph += 1,
            Event::End(TagEnd::Emphasis) => emph = emph.saturating_sub(1),
            Event::Start(Tag::Link { .. }) => link += 1,
            Event::End(TagEnd::Link) => link = link.saturating_sub(1),
            Event::Start(Tag::BlockQuote(_)) => quote += 1,
            Event::End(TagEnd::BlockQuote(_)) => {
                quote = quote.saturating_sub(1);
                separate(&mut out);
            }
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
                separate(&mut out);
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
            Event::Start(Tag::Paragraph) => {
                flush(&mut out, &mut current, kind, quote, width);
            }
            Event::End(TagEnd::Paragraph) => {
                flush(&mut out, &mut current, kind, quote, width);
                separate(&mut out);
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
                separate(&mut out);
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
    while out.last().is_some_and(|line| line.spans.is_empty()) {
        out.pop();
    }
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
pub fn pad_display(text: &str, target: usize) -> String {
    if target == 0 {
        return String::new();
    }
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

pub fn shorten_home(path: &str, home: Option<&str>) -> String {
    if let Some(home) = home.filter(|value| !value.is_empty()) {
        if let Some(rest) = path.strip_prefix(home) {
            return format!("~{rest}");
        }
    }
    path.to_string()
}

/// Keep kernel-provided session metadata on one terminal row. This strips
/// CR/LF pairs defensively while preserving ordinary spaces.
pub fn single_line(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

/// Last non-empty component of either a Unix or Windows path.
pub fn path_tail(path: &str) -> Option<&str> {
    path.rsplit(['/', '\\']).find(|piece| !piece.is_empty())
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

// ---- session picker / history / ui config ---------------------------------

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
                path_tail(&directory).unwrap_or(&id).to_string()
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

/// User config: theme token overrides plus the notify switch.
/// Parsing never fails — bad lines fall back to defaults so startup cannot
/// break.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiConfig {
    /// Built-in palette name. Unknown values are ignored while parsing.
    pub theme: Option<String>,
    pub colors: BTreeMap<String, (u8, u8, u8)>,
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
        if key == "theme" {
            let value = value.to_ascii_lowercase();
            if matches!(value.as_str(), "dark" | "light" | "tsinghua" | "pku") {
                config.theme = Some(value);
            }
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

pub fn ui_config_path() -> Option<PathBuf> {
    std::env::var_os("ZCODE_TUI_CONFIG")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| user_home_dir().map(|home| ui_config_path_from(&home)))
}

/// Persist one built-in theme while preserving the rest of the line-based
/// config, including its newline convention.
pub fn save_ui_theme_to(path: &Path, theme: &str) -> Result<()> {
    if !matches!(theme, "dark" | "light" | "tsinghua" | "pku") {
        return Err(anyhow!(
            "unknown theme {theme}; available: dark, light, tsinghua, pku"
        ));
    }
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()))
        }
    };
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut replaced = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        let is_theme = !line.trim_start().starts_with('#')
            && line
                .split_once('=')
                .is_some_and(|(key, _)| key.trim() == "theme");
        if is_theme {
            if !replaced {
                lines.push(format!("theme = {theme}"));
                replaced = true;
            }
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced {
        lines.push(format!("theme = {theme}"));
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{}{newline}", lines.join(newline)))?;
    Ok(())
}

pub fn save_ui_theme(theme: &str) -> Result<()> {
    let path = ui_config_path().ok_or_else(|| anyhow!("cannot resolve UI config path"))?;
    save_ui_theme_to(&path, theme)
}

/// Resolve and parse the user config; every failure path yields defaults.
pub fn load_ui_config() -> UiConfig {
    ui_config_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|content| parse_ui_config(&content))
        .unwrap_or_default()
}
