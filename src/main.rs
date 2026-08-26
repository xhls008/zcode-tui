use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::fs;
use std::io::{self, Stdout, Write as _};
use std::path::{Path, PathBuf};
use std::process::{self, Command, ExitStatus};
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear as TerminalClear, ClearType};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Widget, Wrap,
};
use ratatui::{Frame, Terminal, TerminalOptions, Viewport};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use zcode_tui::{
    app_close_params, app_compact_params, app_create_params, app_file_rewind_params,
    app_resume_params, app_rewind_params, app_send_params_with_attachments, app_server_enabled,
    app_session_controls, app_session_id_from_result, app_set_mode_params, app_set_model_params,
    app_set_thought_params, app_state_controls, app_state_is_turn_end, app_state_turn_error,
    app_state_watermark, app_steer_params, app_stop_params, app_subscribe_params, app_usage_params,
    app_workspace_model_controls, app_workspace_read_params, build_runtime_model,
    build_send_attachments, checkpoint_short_id, classify_input, command_palette_rows,
    context_watermark_warn, conversation_target, db_baseline, db_schema_supported,
    detect_auth_status, diff_line_role, discover_zcode_app_dir, encode_interaction_reply,
    encode_runtime_preferences_reply, env_is_headless, extract_file_mentions, file_suggestions,
    fold_preview, format_context_watermark, git_diff_command, handle_local_command, help_text,
    history_search, is_newer_version, kernel_config_path_from, kernel_db_path_from,
    latest_assistant_text, latest_reasoning, latest_session_for_dir, leader_action_for_key,
    list_recent_sessions, live_tool_chips, load_mcp_config, load_ui_config, login_command,
    markdown_lines, mcp_config_path, mcp_servers_param, open_kernel_db_ro, osc52_copy_sequence,
    parse_apply_file_rewind, parse_cli_args, parse_interaction_request,
    parse_kernel_slash_commands, parse_prompt_summary, parse_resume_messages, parse_rewind_preview,
    parse_session_list, parse_steer_result, parse_stream_event, parse_todos, parse_update_feed,
    parse_v4_command_ack, prompt_command_for, recent_input_history, relative_age,
    resolve_update_download_url, rewind_failure, run_command, select_update_feed_url, shorten_home,
    skyline_mode, slash_suggestions_merged, spawn_streaming_command, tool_input_summary,
    usage_stats_params, user_mcp_config_path, v4_command_params, v4_conversation_subscribe_params,
    v4_file_rewind_preview_params, v4_rewind_target, with_mcp_servers, with_tool_policy,
    wrap_display, zcode_app_version_from_path, AppConfig, AppServerConn, AppServerEvent,
    AppServerMessage, AppServerTurn, AppServerUnavailable, AuthStatus, CheckpointEntry, DbBaseline,
    DebugLog, DiffRole, InputAction, InteractionRequest, JobEvent, KernelCommand, LeaderAction,
    LiveToolChip, MdLineKind, ModelChoice, RewindPreview, RewindTarget, SessionControls,
    SessionRow, SkylineMode, SpanRole, SteerOutcome, StreamEvent, StreamingJob, TodoItem,
    ToolChipStatus, TurnDelta, UiConfig, UpdateFeed, V4CommandBase, V4ConversationState,
};

mod agents;

use agents::AgentInspectorState;

type Tui = Terminal<CrosstermBackend<Stdout>>;

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SUGGESTION_LIMIT: usize = 8;
/// Foldable cells longer than this render as a head preview by default.
const FOLD_THRESHOLD: usize = 24;
const FOLD_HEAD: usize = 8;
const HISTORY_SEARCH_LIMIT: usize = 8;
/// OSC52 clipboard payload cap (base64 bytes) — larger sequences get
/// truncated or rejected by terminals.
const OSC52_MAX_B64: usize = 100_000;
/// Turns/jobs longer than this ring the terminal bell at finalize
/// (`notify = off` in the config file opts out).
const NOTIFY_AFTER_SECS: f32 = 30.0;
/// Resume history replay: how many messages, and the per-message char cap.
const REPLAY_LIMIT: usize = 6;
const REPLAY_CAP: usize = 400;
/// Avoid the terminal's last column: ambiguous-width glyphs and autowrap can
/// otherwise leak one or two cells onto the next physical row at column zero.
const TRANSCRIPT_RIGHT_GUTTER: usize = 2;

fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// Selectable ASCII reconstruction of the ZCODE block wordmark.
const ZCODE_WORDMARK: &str = r#"███████╗  ██████╗  ██████╗  ██████╗  ███████╗
╚══███╔╝ ██╔════╝ ██╔═══██╗ ██╔══██╗ ██╔════╝
  ███╔╝  ██║      ██║   ██║ ██║  ██║ █████╗
 ███╔╝   ██║      ██║   ██║ ██║  ██║ ██╔══╝
███████╗ ╚██████╗ ╚██████╔╝ ██████╔╝ ███████╗
╚══════╝  ╚═════╝  ╚═════╝  ╚═════╝  ╚══════╝"#;

const LOGO_ROWS: u16 = 6;
const MINI_Z_ICON: [&str; 8] = [
    "╭──────────────╮",
    "│███████████ ██│",
    "│         ██   │",
    "│       ██     │",
    "│     ██       │",
    "│   ██         │",
    "│██ ███████████│",
    "╰──────────────╯",
];
/// Bottom live viewport; completed transcript lives above in scrollback.
const INLINE_VIEWPORT_ROWS: u16 = 10;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{}", help_text());
        return Ok(());
    }
    if args.iter().any(|arg| arg == "-V" || arg == "--version") {
        println!("zcode-tui {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let config = parse_cli_args(args)?;
    let zcode_bin = env::var("ZCODE_TUI_ZCODE_BIN").unwrap_or_else(|_| "zcode".to_string());
    run_tui(config, &zcode_bin)
}

fn run_tui(config: AppConfig, zcode_bin: &str) -> Result<()> {
    let mut state = UiState::new(config, zcode_bin.to_string());
    let mut terminal = TerminalGuard::enter()?;
    state.push_startup_frame();
    let probe = spawn_startup_probe(zcode_bin.to_string(), state.app_workspace());

    for prompt in state.config.initial_prompts.clone() {
        state.queued.push_back(prompt);
    }

    loop {
        state.tick = state.tick.wrapping_add(1);
        if let Ok(report) = probe.try_recv() {
            state.apply_startup_report(report);
        }
        state.pump_job();
        state.pump_app_connect();
        state.pump_app_turn();
        state.pump_app_idle();
        state.poll_live_progress();
        state.drain_queue();
        terminal.flush_transcript(&mut state)?;
        terminal.draw(&mut state)?;

        if !event::poll(Duration::from_millis(80))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => match state.handle_key(key) {
                Some(UiEffect::Quit) => break,
                Some(UiEffect::Editor) => run_editor_effect(&mut terminal, &mut state),
                Some(UiEffect::Login) => run_login_effect(&mut terminal, &mut state),
                None => {}
            },
            Event::Paste(text) => {
                state.insert_text(&text.replace("\r\n", "\n").replace('\r', "\n"));
            }
            Event::Resize(width, height) => terminal.resize(width, height)?,
            _ => {}
        }
    }

    // Clean exit with a live streaming session: tell the kernel it is done
    // with (best-effort fire-and-forget; the process-group kill in Drop
    // remains the backstop for the connection itself).
    state.close_app_session();

    Ok(())
}

fn run_editor_effect(terminal: &mut TerminalGuard, state: &mut UiState) {
    let current = state.input.clone();
    let edited = terminal.suspend(|| edit_input_in_editor(&current));
    match edited {
        Ok(updated) => {
            state.set_input(updated.trim_end_matches('\n'));
            state.status = "editor returned".to_string();
        }
        Err(error) => {
            state.push_error(&format!("{error:#}"));
            state.status = "editor error".to_string();
        }
    }
}

fn run_login_effect(terminal: &mut TerminalGuard, state: &mut UiState) {
    if state.is_busy() {
        state.status = "busy: wait for the running job before /login".to_string();
        return;
    }
    let override_command = env::var("ZCODE_TUI_LOGIN_CMD").ok();
    let headless = env_is_headless(|key| env::var(key).ok());
    let command = match login_command(&state.zcode_bin, override_command.as_deref(), headless) {
        Ok(command) => command,
        Err(error) => {
            state.push_error(&format!("{error:#}"));
            return;
        }
    };
    state.push_system(&format!("interactive login: {}", command.join(" ")));
    let result = terminal.suspend(|| run_interactive_command(&command));
    let succeeded = result.as_ref().is_ok_and(|status| status.success());
    match result {
        Ok(status) if status.success() => state.push_system("login command finished"),
        Ok(status) => state.push_error(&format!("login command exited with {status}")),
        Err(error) => state.push_error(&format!("{error:#}")),
    }
    state.refresh_auth();
    if succeeded {
        state.reload_model_catalog();
    }
    state.status = format!("auth: {}", state.auth_label);
}

fn run_interactive_command(command: &[String]) -> Result<ExitStatus> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty command"))?;
    Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to run {program}"))
}

enum DbProbe {
    Missing,
    Unsupported,
    Supported(PathBuf),
}

struct StartupReport {
    kernel: Option<String>,
    installed: Option<String>,
    feed: Option<UpdateFeed>,
    feed_base: Option<String>,
    db: DbProbe,
    model_catalog: Option<ModelCatalogReport>,
}

struct ModelCatalogReport {
    provider_id: String,
    controls: SessionControls,
}

fn active_zcode_app_dir() -> Option<PathBuf> {
    let explicit = env::var_os("ZCODE_APP").map(PathBuf::from);
    let home = env::var_os("HOME").map(PathBuf::from);
    discover_zcode_app_dir(
        explicit.as_deref(),
        Some(Path::new("/opt/ZCode")),
        home.as_deref(),
    )
}

fn installed_zcode_version(app_dir: Option<&Path>) -> Option<String> {
    app_dir.and_then(zcode_app_version_from_path).or_else(|| {
        run_command(&[
            "dpkg-query".to_string(),
            "-W".to_string(),
            "-f=${Version}".to_string(),
            "zcode".to_string(),
        ])
        .ok()
        .map(|version| {
            version
                .trim()
                .split('-')
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .filter(|version| !version.is_empty())
    })
}

fn update_feed_url_for(app_dir: Option<&Path>) -> Option<String> {
    let app_update = app_dir
        .and_then(|app_dir| fs::read_to_string(app_dir.join("resources/app-update.yml")).ok());
    let explicit = env::var("ZCODE_TUI_UPDATE_FEED").ok();
    select_update_feed_url(app_update.as_deref(), explicit.as_deref())
}

/// Read-only schema check for the kernel db. Missing file is the normal
/// fresh-install state (silent); an unrecognized schema disables every
/// db-derived feature for this run.
fn probe_kernel_db() -> DbProbe {
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else {
        return DbProbe::Missing;
    };
    let path = kernel_db_path_from(&home);
    if !path.is_file() {
        return DbProbe::Missing;
    }
    match open_kernel_db_ro(&path) {
        Ok(conn) if db_schema_supported(&conn) => DbProbe::Supported(path),
        _ => DbProbe::Unsupported,
    }
}

fn model_cache_path() -> Option<PathBuf> {
    if let Some(root) = env::var_os("XDG_CACHE_HOME").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(root).join("zcode-tui").join("models.json"));
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".cache").join("zcode-tui").join("models.json"))
}

fn cached_model_catalog() -> Option<ModelCatalogReport> {
    let cache: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(model_cache_path()?).ok()?).ok()?;
    if cache.get("version")?.as_u64()? != 1 {
        return None;
    }
    let provider_id = cache.get("providerId")?.as_str()?.to_string();
    let models = cache
        .get("models")?
        .as_array()?
        .iter()
        .filter_map(|model| {
            let reference = model.get("reference")?.clone();
            if reference.get("providerId")?.as_str()? != provider_id {
                return None;
            }
            Some(ModelChoice {
                label: model.get("label")?.as_str()?.to_string(),
                provider: model
                    .get("provider")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                reference,
            })
        })
        .collect::<Vec<_>>();
    if models.is_empty() {
        return None;
    }
    Some(ModelCatalogReport {
        provider_id: provider_id.clone(),
        controls: SessionControls {
            models,
            model_provider: Some(provider_id),
            model_current: cache
                .get("modelCurrent")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            ..SessionControls::default()
        },
    })
}

fn cache_model_catalog(report: &ModelCatalogReport) -> Result<()> {
    let models = report
        .controls
        .models
        .iter()
        .map(|model| {
            serde_json::json!({
                "label": model.label,
                "provider": model.provider,
                "reference": model.reference,
            })
        })
        .collect::<Vec<_>>();
    let cache = serde_json::json!({
        "version": 1,
        "providerId": report.provider_id,
        "fetchedAt": unix_time_ms(),
        "modelCurrent": report.controls.model_current,
        "models": models,
    });
    let path = model_cache_path().ok_or_else(|| anyhow::anyhow!("cache directory unavailable"))?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)?;
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(&cache)?))?;
    Ok(())
}

fn refresh_model_catalog(zcode_bin: &str, workspace: &str) -> Option<ModelCatalogReport> {
    let live = AppServerConn::spawn(zcode_bin).ok().and_then(|mut conn| {
        let result = conn
            .request_blocking(
                "workspace/readState",
                app_workspace_read_params(workspace),
                Duration::from_secs(5),
            )
            .ok()?;
        let (provider_id, controls) = app_workspace_model_controls(&result)?;
        Some(ModelCatalogReport {
            provider_id,
            controls,
        })
    });
    if let Some(report) = live {
        let _ = cache_model_catalog(&report);
        Some(report)
    } else {
        cached_model_catalog()
    }
}

/// Probe, off the UI thread: the CLI kernel version, the installed desktop
/// package version, and the official electron-updater feed (the same
/// latest-linux.yml the ZCode desktop app polls, so the notice matches the
/// official release channel). ZCODE_TUI_NO_UPDATE_CHECK=1 skips the network.
fn spawn_startup_probe(
    zcode_bin: String,
    workspace: String,
) -> std::sync::mpsc::Receiver<StartupReport> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let model_catalog = refresh_model_catalog(&zcode_bin, &workspace);
        let kernel = run_command(&[zcode_bin, "version".to_string()])
            .ok()
            .and_then(|output| {
                output
                    .lines()
                    .map(str::trim)
                    .find(|line| line.chars().next().is_some_and(|c| c.is_ascii_digit()))
                    .map(str::to_string)
            });
        let app_dir = active_zcode_app_dir();
        let installed = installed_zcode_version(app_dir.as_deref());

        let mut feed = None;
        let mut feed_base = None;
        if env::var_os("ZCODE_TUI_NO_UPDATE_CHECK").is_none() {
            if let Some(url) = update_feed_url_for(app_dir.as_deref()) {
                feed_base = Some(url.trim_end_matches("latest-linux.yml").to_string());
                feed = run_command(&[
                    "curl".to_string(),
                    "-fsSL".to_string(),
                    "--max-time".to_string(),
                    "5".to_string(),
                    url,
                ])
                .ok()
                .and_then(|body| parse_update_feed(&body));
            }
        }
        let _ = sender.send(StartupReport {
            kernel,
            installed,
            feed,
            feed_base,
            db: probe_kernel_db(),
            model_catalog,
        });
    });
    receiver
}

fn build_update_tip(installed: &str, feed: &UpdateFeed, feed_base: Option<&str>) -> String {
    let mut lines = vec![format!(
        "Tip: 官方 ZCode {} 已发布，本机 {installed}。输入 /update 一步升级（下载+sha512 校验+安装）。",
        feed.version
    )];
    lines.push("更新说明: https://zcode.z.ai/en/changelog".to_string());
    match (feed_base, &feed.deb_file) {
        (Some(base), Some(file)) => match resolve_update_download_url(base, file) {
            Some(url) => lines.push(format!("手动下载: {url}")),
            None => lines.push("手动下载: https://zcode.z.ai".to_string()),
        },
        _ => lines.push("手动下载: https://zcode.z.ai".to_string()),
    }
    lines.join("\n")
}

enum UiEffect {
    Quit,
    Editor,
    Login,
}

/// 智谱-flavored theme in a Codex-like shell: one GLM-blue accent, cool
/// neutrals, elevated background bands instead of borders, semantic
/// green/red for state. `plain` honors --no-color/NO_COLOR.
#[derive(Clone, Copy)]
struct Theme {
    plain: bool,
    accent: Color,
    accent_dim: Color,
    text: Color,
    dim: Color,
    good: Color,
    bad: Color,
    frame: Color,
    code_bg: Color,
    band_bg: Color,
}

impl Theme {
    fn zhipu(plain: bool) -> Self {
        Self {
            plain,
            accent: Color::Rgb(96, 136, 255),
            accent_dim: Color::Rgb(64, 88, 168),
            text: Color::Rgb(222, 226, 234),
            dim: Color::Rgb(122, 130, 146),
            good: Color::Rgb(126, 200, 154),
            bad: Color::Rgb(232, 116, 116),
            frame: Color::Rgb(56, 62, 78),
            code_bg: Color::Rgb(33, 38, 51),
            band_bg: Color::Rgb(48, 52, 63),
        }
    }

    fn styled(&self, color: Color) -> Style {
        if self.plain {
            Style::default()
        } else {
            Style::default().fg(color)
        }
    }

    fn accent(&self) -> Style {
        self.styled(self.accent)
    }

    fn accent_dim(&self) -> Style {
        self.styled(self.accent_dim)
    }

    fn text(&self) -> Style {
        self.styled(self.text)
    }

    fn dim(&self) -> Style {
        self.styled(self.dim)
    }

    fn good(&self) -> Style {
        self.styled(self.good)
    }

    fn bad(&self) -> Style {
        self.styled(self.bad)
    }

    fn frame(&self) -> Style {
        self.styled(self.frame)
    }

    fn code(&self) -> Style {
        if self.plain {
            Style::default()
        } else {
            Style::default().fg(self.text).bg(self.code_bg)
        }
    }

    /// Elevated background band, Codex-style, for user messages and the
    /// composer instead of drawn borders.
    fn band(&self) -> Style {
        if self.plain {
            Style::default()
        } else {
            Style::default().bg(self.band_bg)
        }
    }

    fn selection(&self) -> Style {
        if self.plain {
            Style::default().reversed()
        } else {
            Style::default().fg(Color::Rgb(14, 18, 30)).bg(self.accent)
        }
    }

    /// Apply user config color overrides; NO_COLOR (plain) still wins
    /// because every accessor checks `plain` first.
    fn with_overrides(mut self, config: &UiConfig) -> Self {
        for (key, (r, g, b)) in &config.colors {
            let color = Color::Rgb(*r, *g, *b);
            match key.as_str() {
                "accent" => self.accent = color,
                "accent_dim" => self.accent_dim = color,
                "text" => self.text = color,
                "dim" => self.dim = color,
                "good" => self.good = color,
                "bad" => self.bad = color,
                "frame" => self.frame = color,
                "code_bg" => self.code_bg = color,
                "band_bg" => self.band_bg = color,
                _ => {}
            }
        }
        self
    }
}

#[derive(Clone)]
struct Suggestion {
    insert: String,
    display: String,
    /// Char index where the replaced region starts; None replaces the whole input.
    token_start: Option<usize>,
}

/// Per-run live-progress state fed by polling the kernel db. Purely
/// cosmetic: it renders in the work panel while the job runs and vanishes
/// at finalize, never entering the transcript.
struct LiveProgress {
    directory: String,
    session_id: Option<String>,
    /// Latest session for the directory at spawn time. A fresh (non-continue)
    /// prompt creates a NEW session, so polling must skip this stale id when
    /// resolving — latching onto it would filter every row away.
    prior_session: Option<String>,
    baseline: DbBaseline,
    chips: Vec<LiveToolChip>,
    reasoning: Option<String>,
    /// Latest assistant text as it forms (progressive in multi-step turns).
    text: Option<String>,
}

struct ActiveJob {
    job: StreamingJob,
    log_index: usize,
    kind: LogKind,
    label: String,
    finished: Option<(bool, String)>,
    finished_at: Option<Instant>,
    eofs: usize,
    entry_started: bool,
    any_output: bool,
    cancel_requested: bool,
    started: Instant,
    /// Assistant jobs buffer stdout here: with --json the whole output is
    /// one end-of-run summary object, parsed at finalize. stderr is kept
    /// apart in `errs` — a stray kernel warning interleaved mid-JSON would
    /// otherwise break the summary parse (raw leak + lost watermark).
    raw: Vec<String>,
    /// Assistant jobs: kernel stderr lines, surfaced as a dim note at
    /// finalize instead of polluting the summary buffer.
    errs: Vec<String>,
    live: Option<LiveProgress>,
}

impl ActiveJob {
    /// The child exited and every output stream reported EOF, so no more
    /// lines can arrive: safe to finalize without losing tail output.
    fn drained(&self) -> bool {
        self.finished.is_some() && self.eofs >= self.job.streams
    }
}

const PERMISSION_MODES: [&str; 4] = ["build", "edit", "plan", "yolo"];

/// State of the app-server streaming path for this process.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AppMode {
    /// Explicit opt-out: always use the classic `--prompt` path.
    Off,
    /// Healthy: prompts stream through the app-server.
    Ready,
    /// A failure permanently downgraded this run to `--prompt`.
    Downgraded,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum V4Mode {
    Unknown,
    Available,
    Unavailable,
}

/// A single in-flight app-server turn. The open text entry remains mutable;
/// completed text phases and tool results advance `committable_end` and append
/// to terminal scrollback without rewriting earlier rows.
struct AppTurn {
    turn: AppServerTurn,
    /// Exclusive end of the immutable prefix that may enter scrollback.
    committable_end: usize,
    /// The open assistant text entry, or None between text runs (e.g. right
    /// after a tool landed, before the next text token). Created lazily.
    text_index: Option<usize>,
    /// Bytes of `turn.text` already flushed into transcript entries; the rest
    /// is the unwritten suffix appended on the next Text delta.
    written: usize,
    started: Instant,
    cancel_requested: bool,
    /// At least one visible text token has landed — distinguishes "connection
    /// died before any answer" (retry via --prompt) from "died mid-answer"
    /// (keep the partial, just downgrade).
    got_text: bool,
    /// Request id of this turn's `session/send`. Only an error Response for
    /// *this* id aborts the turn — a stray error (e.g. a prior cancel's
    /// `session/stop`) arriving mid-turn must not down a healthy turn.
    send_id: u64,
}

/// A non-blocking `session/create` → `session/subscribe` handshake in flight.
/// Driven off the main loop so the UI keeps rendering (and Esc keeps working)
/// instead of freezing on two synchronous 20s blocking calls.
struct AppConnect {
    phase: ConnectPhase,
    /// The prompt to send once the session is subscribed.
    prompt: String,
    started: Instant,
}

/// Which handshake response the connect state is waiting on (matched by id).
enum ConnectPhase {
    Create(u64),
    /// `session/resume` of a picked/`--resume` session; an error falls back
    /// to Create (fresh session) instead of downgrading.
    Resume(u64),
    Subscribe(u64),
    V4Subscribe(u64),
}

/// Stage tag copied out of `ConnectPhase` so a poll loop can mutate `self`
/// without holding a borrow of `app_connect` across the arms.
#[derive(Clone, Copy)]
enum ConnectStage {
    Create,
    Resume,
    Subscribe,
    V4Subscribe,
}

/// Availability of the kernel's sqlite database for live progress.
/// Resolved once by the startup probe; anything but Enabled degrades every
/// db-derived feature to the pre-db behaviour.
#[derive(Clone, PartialEq, Eq)]
enum DbState {
    Unknown,
    Enabled(PathBuf),
    Disabled,
}

struct UiState {
    config: AppConfig,
    zcode_bin: String,
    theme: Theme,
    kernel_version: Option<String>,
    /// A prompt has completed in this run, so later prompts auto --continue.
    session_active: bool,
    auth_status: AuthStatus,
    db_state: DbState,
    context_watermark: Option<(u64, u64)>,
    log: Vec<LogLine>,
    input: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    status: String,
    auth_label: String,
    show_help: bool,
    show_palette: bool,
    leader_pending: bool,
    suggestions: Vec<Suggestion>,
    suggestion_index: usize,
    suggestion_nav: bool,
    suggestions_dismissed: bool,
    job: Option<ActiveJob>,
    queued: VecDeque<String>,
    tick: usize,
    /// /sessions picker overlay: rows + selected index.
    session_picker: Option<(Vec<SessionRow>, usize)>,
    /// Background tasks observed in the current session + /agents selection.
    agents: AgentInspectorState,
    /// /model picker overlay: selected index into `controls.models`.
    model_picker: Option<usize>,
    /// Model selected before the first app-server session exists. Applied
    /// after create/resume and before the queued first prompt is sent.
    pending_model: Option<ModelChoice>,
    /// Provider selected by `model.main`; model catalogs and kernel pushes
    /// from other configured providers are excluded from this TUI.
    model_provider: Option<String>,
    /// Ctrl+R reverse search overlay: query + selected index.
    history_query: Option<(String, usize)>,
    /// Log indices the user expanded with Ctrl+O (folding is the default
    /// for long foldable cells).
    unfolded: HashSet<usize>,
    /// A completed, scrollback-owned long entry shown in a read-only overlay.
    /// Terminal scrollback is immutable, so Ctrl+O cannot redraw it in place.
    expanded_log: Option<(usize, u16)>,
    /// Number of log entries already committed to terminal scrollback.
    flushed_log: usize,
    /// Incremented by /clear so the inline terminal purges its scrollback.
    clear_generation: u64,
    /// App-server streaming path (default-on, seamless fallback).
    app_mode: AppMode,
    app_conn: Option<AppServerConn>,
    /// Kernel session reused across prompts once created (session continuity).
    app_session: Option<String>,
    /// Optional ZCode 3.5.3 V4 control plane layered over the legacy body
    /// stream. Method-not-found marks old kernels unavailable without
    /// downgrading their otherwise healthy app-server connection.
    v4_mode: V4Mode,
    v4_state: V4ConversationState,
    v4_client_id: String,
    v4_command_seq: u64,
    pending_v4_steers: HashMap<String, String>,
    app_turn: Option<AppTurn>,
    /// Welcome wordmark visibility; `off` suppresses the ASCII logo.
    skyline_mode: SkylineMode,
    /// Non-blocking create+subscribe handshake in flight (first prompt of a run).
    app_connect: Option<AppConnect>,
    /// After an Esc-cancel the reused session keeps emitting the stopped turn's
    /// tail (its own `prompt_completed` included). While `Some`, those events
    /// are swallowed until the terminator lands, so they cannot bleed into —
    /// and prematurely finalize — the next prompt. Carries the start time for a
    /// timeout that forces a fresh session if the kernel never terminates it.
    app_draining: Option<Instant>,
    /// Pending kernel interaction (permission) request; renders the approval
    /// overlay. The kernel re-sends the same requestId under fresh envelope
    /// ids until answered — re-sends only refresh `envelope_id` here.
    interaction: Option<PendingInteraction>,
    /// requestIds already answered: late re-sends are dropped silently.
    interaction_done: HashSet<String>,
    /// Latest control surface (mode/model/thought level, current + choices)
    /// pushed by the kernel via `state.updated` — authoritative echo for
    /// /model, /think and Shift+Tab on the app-server path.
    controls: SessionControls,
    /// Kernel-reported slash commands (create/resume result), merged into
    /// `/` completion after the local catalog.
    kernel_commands: Vec<KernelCommand>,
    /// Kernel TODO list (create/resume result + state pushes); rendered in
    /// the work panel while non-empty.
    todos: Vec<TodoItem>,
    /// In-flight fire-and-forget control requests (setMode/setModel/…), by
    /// request id, so an error response can name the command it failed.
    control_requests: std::collections::HashMap<u64, ControlReq>,
    /// `checkpoint.created` events captured this session (in capture order) —
    /// the /rewind targets. Cleared whenever the session changes.
    checkpoints: Vec<CheckpointEntry>,
    /// /rewind overlay (target picker → preview/scope → apply).
    rewind: Option<RewindOverlay>,
    /// Latest `rewind.triggered` event (strategy, reason): the ONLY reliable
    /// rewind outcome signal — a failed rewind still gets a success envelope.
    /// The event precedes the response on the same stream; taken when the
    /// session/rewind response lands.
    rewind_trigger: Option<(String, String)>,
    /// ZCODE_TUI_LOG state-transition log (protocol traffic is logged by the
    /// connection itself); None = disabled, zero overhead.
    debug_log: Option<DebugLog>,
    /// Turn-complete bell (>30s turns); `notify = off` in the config disables.
    notify_enabled: bool,
    /// Browser Use is global CLI configuration; explain its classic routing
    /// once per run instead of repeating the limitation before every turn.
    browser_route_noted: bool,
}

/// The approval overlay's state: the parsed request, the freshest envelope id
/// to reply on, and the selected option of the (first) question.
struct PendingInteraction {
    request: InteractionRequest,
    envelope_id: serde_json::Value,
    selected: usize,
}

/// A fire-and-forget control request in flight, kept by request id so an
/// error response can be attributed to the command that sent it. Steer
/// carries its input so a failure can requeue it instead of losing it.
enum ControlReq {
    Command(&'static str),
    Steer(String),
    /// V4 steer is two commands: first switch followup mode, then send text.
    V4SetGuide {
        content: String,
        command_id: String,
    },
    /// Accepted sendText still needs a semantic delivery frame (guide/queue).
    V4SteerText {
        content: String,
        command_id: String,
    },
    /// A /usage sub-request; the tag ("session" | "stats") picks the
    /// formatter when the result arrives.
    Usage(&'static str),
    /// /rewind: previewFileRewind in flight; the result feeds the preview
    /// stage of the overlay (dropped if the overlay was closed meanwhile).
    RewindPreview(RewindTarget),
    /// /rewind: applyFileRewind in flight (file scope — the safe apply that
    /// refuses externally-modified files). `then_conversation` carries the
    /// pre-translated MESSAGE-kind target of the conversation leg chained
    /// after a successful file restore (scope "both").
    RewindApplyFiles {
        target: RewindTarget,
        then_conversation: Option<RewindTarget>,
    },
    /// /rewind: conversation-scope session/rewind in flight; judged by the
    /// preceding rewind.triggered event, never the envelope.
    RewindConversation(RewindTarget),
    /// ZCode 3.5.3 V4 applyFileRewind command.
    V4RewindApply(RewindTarget),
}

/// The /rewind overlay. Stage 1 (`preview: None`): pick a target from the
/// session's captured checkpoints (+ latestCheckpoint). Stage 2: the
/// previewFileRewind result arrived — show it, pick a scope, Enter applies.
struct RewindOverlay {
    /// (label, target), latestCheckpoint first, then checkpoints new→old.
    targets: Vec<(String, RewindTarget)>,
    selected: usize,
    /// Set once the preview response lands (stage 2).
    preview: Option<(RewindTarget, RewindPreview)>,
    /// Index into REWIND_SCOPES (stage 2).
    scope: usize,
    /// A preview/apply request is in flight — Enter is debounced.
    busy: bool,
}

/// Scope choices of the preview stage. Default workspace (files only): the
/// primary use case is "the model broke my file". conversation/both rewrite
/// the kernel conversation and are explicit choices.
const REWIND_SCOPES: [&str; 3] = ["workspace", "conversation", "both"];

impl UiState {
    fn new(config: AppConfig, zcode_bin: String) -> Self {
        let auth_status = detect_auth_status();
        let auth_label = auth_status.short_label();
        let plain = config.no_color || env::var_os("NO_COLOR").is_some();
        let ui_config = load_ui_config();
        let notify_enabled = ui_config.notify != Some(false);
        let app_mode = if app_server_enabled(|key| env::var(key).ok()) {
            AppMode::Ready
        } else {
            AppMode::Off
        };
        Self {
            config,
            zcode_bin,
            theme: Theme::zhipu(plain).with_overrides(&ui_config),
            kernel_version: None,
            session_active: false,
            auth_status,
            db_state: DbState::Unknown,
            context_watermark: None,
            log: Vec::new(),
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
            status: "ready".to_string(),
            auth_label,
            show_help: false,
            show_palette: false,
            leader_pending: false,
            suggestions: Vec::new(),
            suggestion_index: 0,
            suggestion_nav: false,
            suggestions_dismissed: false,
            job: None,
            queued: VecDeque::new(),
            tick: 0,
            session_picker: None,
            agents: AgentInspectorState::default(),
            model_picker: None,
            pending_model: None,
            model_provider: None,
            history_query: None,
            unfolded: HashSet::new(),
            expanded_log: None,
            flushed_log: 0,
            clear_generation: 0,
            app_mode,
            app_conn: None,
            app_session: None,
            v4_mode: V4Mode::Unknown,
            v4_state: V4ConversationState::default(),
            v4_client_id: format!("zcode-tui-{}", process::id()),
            v4_command_seq: 0,
            pending_v4_steers: HashMap::new(),
            app_turn: None,
            skyline_mode: skyline_mode(|key| env::var(key).ok()),
            app_connect: None,
            app_draining: None,
            interaction: None,
            interaction_done: HashSet::new(),
            controls: SessionControls::default(),
            kernel_commands: Vec::new(),
            todos: Vec::new(),
            control_requests: std::collections::HashMap::new(),
            checkpoints: Vec::new(),
            rewind: None,
            rewind_trigger: None,
            debug_log: DebugLog::from_env(),
            notify_enabled,
            browser_route_noted: false,
        }
    }

    /// State-transition line into the ZCODE_TUI_LOG debug log (no-op when
    /// the env var is unset). Protocol traffic is logged by AppServerConn.
    fn log_debug(&self, text: &str) {
        if let Some(log) = &self.debug_log {
            log.line(text);
        }
    }

    /// Prefix of immutable transcript entries that can move into scrollback.
    fn committable_log_end(&self) -> usize {
        let mut end = self.log.len();
        if let Some(active) = &self.job {
            end = end.min(active.log_index);
        }
        if let Some(turn) = &self.app_turn {
            end = end.min(turn.committable_end);
        }
        end.max(self.flushed_log)
    }

    fn next_v4_command_id(&mut self, kind: &str) -> String {
        self.v4_command_seq = self.v4_command_seq.saturating_add(1);
        format!("{}-{kind}-{}", self.v4_client_id, self.v4_command_seq)
    }

    fn v4_cas_base(&self) -> Option<(u64, String)> {
        Some((self.v4_state.revision?, self.v4_state.log_epoch.clone()?))
    }

    /// Merge a V4 snapshot/delta and settle any sendText whose semantic
    /// delivery has become visible. A command response alone is not enough:
    /// live 3.5.3 reports guide/queue in the subsequent queue frame.
    fn apply_v4_frame(&mut self, params: serde_json::Value) {
        let previous_revision = self.v4_state.revision;
        let effect = self.v4_state.apply_frame(&params);
        if self.v4_state.revision != previous_revision {
            let revision = self.v4_state.revision.unwrap_or(0);
            self.log_debug(&format!("v4: frame revision={revision}"));
        }
        for (command_id, delivery) in effect.deliveries {
            if let Some(content) = self.pending_v4_steers.remove(&command_id) {
                self.settle_v4_steer(&command_id, &content, &delivery);
            }
        }
    }

    fn settle_v4_steer(&mut self, command_id: &str, content: &str, delivery: &str) {
        self.push_user(content);
        match delivery {
            "guide" => {
                self.push_system("↪ steered the running turn (V4 guide)");
                self.status = "steered (V4 guide)".to_string();
            }
            "queue" => {
                self.push_system("↪ follow-up admitted to the kernel queue (not steered)");
                self.status = "queued by kernel (V4)".to_string();
            }
            "startNow" => {
                self.push_system("↪ follow-up started as a new kernel turn");
                self.status = "started by kernel (V4)".to_string();
            }
            other => {
                self.push_system(&format!("↪ kernel accepted follow-up delivery={other}"));
                self.status = format!("follow-up {other}");
            }
        }
        self.log_debug(&format!(
            "v4 steer command={command_id} delivery={delivery}"
        ));
    }

    /// Whether a job or an app-server turn (including the handshake before it,
    /// and the drain after a cancel) is currently occupying the UI.
    fn is_busy(&self) -> bool {
        self.job.is_some()
            || self.app_turn.is_some()
            || self.app_connect.is_some()
            || self.app_draining.is_some()
    }

    fn refresh_auth(&mut self) {
        self.auth_status = detect_auth_status();
        self.auth_label = self.auth_status.short_label();
    }

    /// Startup skeleton: the official logo followed by a compact welcome card.
    fn push_startup_frame(&mut self) {
        self.push_banner();
        if !self.auth_status.is_configured() {
            self.push_unauth_screen_if_needed();
        }
    }

    /// Browser-free sign-in guidance appended to the welcome frame.
    fn push_unauth_screen_if_needed(&mut self) {
        if self.auth_status.is_configured() {
            return;
        }
        let headline = match &self.auth_status {
            AuthStatus::Partial { evidence } => format!(
                "partially configured: {evidence} found, but the kernel still needs \
                 ~/.zcode/cli/config.json"
            ),
            _ => "not configured — the kernel needs a model config before prompts can run"
                .to_string(),
        };
        self.push_system(&headline);
        self.log.push(LogLine::new(
            LogKind::Tip,
            "Tip: three ways to sign in without a browser on this machine:\n\
             › /login                                            OAuth (auto --no-browser when headless)\n\
             › zcode login bigmodel-coding-plan-api-key <key>    智谱国内 coding plan\n\
             › zcode login zai-coding-plan-api-key <key>         Z.AI international",
        ));
    }

    fn resolve_cwd(&self) -> PathBuf {
        self.config
            .cwd
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    // ---- input editing -------------------------------------------------

    fn char_count(&self) -> usize {
        self.input.chars().count()
    }

    fn byte_index(&self, char_index: usize) -> usize {
        self.input
            .char_indices()
            .nth(char_index)
            .map(|(index, _)| index)
            .unwrap_or(self.input.len())
    }

    fn set_input(&mut self, text: &str) {
        self.input = text.to_string();
        self.cursor = self.char_count();
        self.after_input_change();
    }

    fn insert_char(&mut self, ch: char) {
        let index = self.byte_index(self.cursor);
        self.input.insert(index, ch);
        self.cursor += 1;
        self.after_input_change();
    }

    fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let index = self.byte_index(self.cursor);
        self.input.insert_str(index, text);
        self.cursor += text.chars().count();
        self.after_input_change();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let index = self.byte_index(self.cursor - 1);
        self.input.remove(index);
        self.cursor -= 1;
        self.after_input_change();
    }

    fn delete_forward(&mut self) {
        if self.cursor >= self.char_count() {
            return;
        }
        let index = self.byte_index(self.cursor);
        self.input.remove(index);
        self.after_input_change();
    }

    fn delete_word_back(&mut self) {
        let chars: Vec<char> = self.input.chars().collect();
        let mut start = self.cursor;
        while start > 0 && chars[start - 1].is_whitespace() {
            start -= 1;
        }
        while start > 0 && !chars[start - 1].is_whitespace() {
            start -= 1;
        }
        if start == self.cursor {
            return;
        }
        let from = self.byte_index(start);
        let to = self.byte_index(self.cursor);
        self.input.replace_range(from..to, "");
        self.cursor = start;
        self.after_input_change();
    }

    fn clear_input(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.after_input_change();
    }

    fn after_input_change(&mut self) {
        self.history_index = None;
        self.suggestions_dismissed = false;
        self.refresh_suggestions();
    }

    // ---- suggestions ---------------------------------------------------

    fn refresh_suggestions(&mut self) {
        self.suggestions.clear();
        self.suggestion_index = 0;
        self.suggestion_nav = false;
        if self.suggestions_dismissed {
            return;
        }

        if self.input.starts_with('/') && !self.input.contains('\n') {
            self.suggestions =
                slash_suggestions_merged(&self.input, SUGGESTION_LIMIT, &self.kernel_commands)
                    .into_iter()
                    .map(|item| Suggestion {
                        insert: format!("{} ", item.command),
                        display: format!(
                            "{:<18} {:<7} {}",
                            item.command,
                            format!("[{}]", item.route),
                            item.summary
                        ),
                        token_start: None,
                    })
                    .collect();
            return;
        }

        if let Some((start, query)) = self.at_token_before_cursor() {
            let root = self.resolve_cwd();
            self.suggestions = file_suggestions(&root, &query, SUGGESTION_LIMIT)
                .into_iter()
                .map(|path| Suggestion {
                    insert: format!("@{path}"),
                    display: path,
                    token_start: Some(start),
                })
                .collect();
        }
    }

    /// If the token immediately before the cursor starts with `@`, return
    /// (token start char index, query without the `@`).
    fn at_token_before_cursor(&self) -> Option<(usize, String)> {
        let chars: Vec<char> = self.input.chars().collect();
        let cursor = self.cursor.min(chars.len());
        let mut start = cursor;
        while start > 0 && !chars[start - 1].is_whitespace() {
            start -= 1;
        }
        if start >= cursor || chars[start] != '@' {
            return None;
        }
        let query: String = chars[start + 1..cursor].iter().collect();
        Some((start, query))
    }

    fn accept_suggestion(&mut self) {
        let Some(suggestion) = self.suggestions.get(self.suggestion_index).cloned() else {
            return;
        };
        match suggestion.token_start {
            None => {
                self.input = suggestion.insert;
                self.cursor = self.char_count();
            }
            Some(start) => {
                let from = self.byte_index(start);
                let to = self.byte_index(self.cursor);
                self.input.replace_range(from..to, &suggestion.insert);
                self.cursor = start + suggestion.insert.chars().count();
            }
        }
        self.status = format!("completed {}", suggestion.display.trim_end());
        self.after_input_change();
    }

    // ---- key handling --------------------------------------------------

    fn handle_key(&mut self, key: KeyEvent) -> Option<UiEffect> {
        if self.leader_pending {
            self.leader_pending = false;
            return self.handle_leader_key(key);
        }
        // The kernel is waiting on an answer: the approval overlay owns the
        // keys (streaming continues rendering behind it).
        if self.interaction.is_some() {
            return self.handle_interaction_key(key);
        }
        if self.agents.is_open() {
            self.handle_background_task_key(key);
            return None;
        }
        if self.expanded_log.is_some() {
            self.handle_expanded_log_key(key);
            return None;
        }
        if self.model_picker.is_some() {
            return self.handle_model_picker_key(key);
        }
        if self.rewind.is_some() {
            return self.handle_rewind_key(key);
        }
        if self.session_picker.is_some() {
            return self.handle_session_picker_key(key);
        }
        if self.history_query.is_some() {
            return self.handle_history_search_key(key);
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('r') if ctrl => {
                self.history_query = Some((String::new(), 0));
                self.status = "reverse search: type to filter, Enter recalls".to_string();
            }
            KeyCode::Char('o') if ctrl => self.toggle_fold(),
            KeyCode::Char('q') if ctrl => return Some(UiEffect::Quit),
            KeyCode::Char('c') if ctrl => {
                if self.is_busy() {
                    self.cancel_current();
                } else if self.input.is_empty() {
                    return Some(UiEffect::Quit);
                } else {
                    self.clear_input();
                    self.status = "input cleared".to_string();
                }
            }
            KeyCode::Char('d') if ctrl => return Some(UiEffect::Quit),
            KeyCode::Char('p') if ctrl => {
                self.show_palette = !self.show_palette;
                self.show_help = false;
                self.status = "command palette".to_string();
            }
            KeyCode::Char('x') if ctrl => {
                self.leader_pending = true;
                self.status =
                    "leader: p palette | h help | e editor | x clear | u input | y copy | q quit"
                        .to_string();
            }
            KeyCode::Char('g') if ctrl => return Some(UiEffect::Editor),
            KeyCode::Char('j') if ctrl => self.insert_char('\n'),
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = self.char_count(),
            KeyCode::Char('w') if ctrl => self.delete_word_back(),
            KeyCode::Char('l') if ctrl => {
                self.clear_log();
                self.status = "cleared".to_string();
            }
            KeyCode::Char('u') if ctrl => {
                self.clear_input();
                self.status = "input cleared".to_string();
            }
            KeyCode::Char('?') if self.input.is_empty() => {
                self.show_help = !self.show_help;
                self.show_palette = false;
                self.status = "help toggled".to_string();
            }
            KeyCode::Char(ch) if !ctrl => self.insert_char(ch),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete_forward(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.char_count()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.char_count(),
            KeyCode::Tab => {
                if !self.suggestions.is_empty() {
                    self.accept_suggestion();
                } else if self.input.starts_with('/') || self.at_token_before_cursor().is_some() {
                    self.status = "no matches".to_string();
                } else {
                    self.status = "tab completes / commands and @ paths".to_string();
                }
            }
            KeyCode::BackTab => self.cycle_mode(),
            KeyCode::Enter => {
                if !self.suggestions.is_empty() && self.input.starts_with('/') {
                    self.accept_suggestion();
                    let input = self.input.trim().to_string();
                    if let Some(effect) = self.handle_submit(&input) {
                        return Some(effect);
                    }
                } else if !self.suggestions.is_empty() && self.suggestion_nav {
                    self.accept_suggestion();
                } else {
                    let input = self.input.trim().to_string();
                    if let Some(effect) = self.handle_submit(&input) {
                        return Some(effect);
                    }
                }
            }
            KeyCode::Esc => {
                if !self.suggestions.is_empty() {
                    self.suggestions_dismissed = true;
                    self.refresh_suggestions();
                } else if self.show_help {
                    self.show_help = false;
                } else if self.show_palette {
                    self.show_palette = false;
                } else if self.is_busy() {
                    self.cancel_current();
                } else {
                    return Some(UiEffect::Quit);
                }
            }
            KeyCode::Up => {
                if !self.suggestions.is_empty() {
                    self.suggestion_index = self.suggestion_index.saturating_sub(1);
                    self.suggestion_nav = true;
                } else {
                    self.recall_history(-1);
                }
            }
            KeyCode::Down => {
                if !self.suggestions.is_empty() {
                    self.suggestion_index =
                        (self.suggestion_index + 1).min(self.suggestions.len() - 1);
                    self.suggestion_nav = true;
                } else {
                    self.recall_history(1);
                }
            }
            _ => {}
        }
        None
    }

    fn handle_leader_key(&mut self, key: KeyEvent) -> Option<UiEffect> {
        let action = match key.code {
            KeyCode::Esc => {
                self.status = "leader cancelled".to_string();
                return None;
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
            Some(LeaderAction::Editor) => return Some(UiEffect::Editor),
            Some(LeaderAction::ClearConversation) => {
                self.clear_log();
                self.status = "cleared".to_string();
            }
            Some(LeaderAction::ClearInput) => {
                self.clear_input();
                self.status = "input cleared".to_string();
            }
            Some(LeaderAction::CopyLastReply) => self.copy_last_reply(),
            Some(LeaderAction::Quit) => return Some(UiEffect::Quit),
            None => self.status = "unknown leader key".to_string(),
        }
        None
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
        let recalled = (next < self.history.len()).then_some(next);
        self.input = recalled
            .map(|index| self.history[index].clone())
            .unwrap_or_default();
        self.cursor = self.char_count();
        self.suggestions.clear();
        self.history_index = recalled;
    }

    fn clear_log(&mut self) {
        if self.is_busy() {
            self.status = "busy: cannot clear while a job streams output".to_string();
            return;
        }
        self.log.clear();
        self.unfolded.clear();
        self.expanded_log = None;
        self.flushed_log = 0;
        self.clear_generation = self.clear_generation.wrapping_add(1);
        self.push_startup_frame();
    }

    // ---- submit + jobs ---------------------------------------------------

    fn handle_submit(&mut self, input: &str) -> Option<UiEffect> {
        if input.is_empty() {
            return None;
        }
        self.show_palette = false;

        let classified = classify_input(input);
        if let Ok(InputAction::Quit) = classified {
            self.status = "bye".to_string();
            return Some(UiEffect::Quit);
        }

        if self.is_busy() {
            // Plain text during a live app-server turn steers it instead of
            // queueing (session/steer). Slash/shell commands, and inputs
            // during the handshake or drain phases, still queue.
            if self.app_turn.is_some() && matches!(&classified, Ok(InputAction::Prompt(_))) {
                self.history.push(input.to_string());
                self.clear_input();
                self.steer_turn(input);
                return None;
            }
            self.queued.push_back(input.to_string());
            self.history.push(input.to_string());
            self.clear_input();
            self.status = format!("queued ({} waiting)", self.queued.len());
            return None;
        }

        self.history.push(input.to_string());
        self.clear_input();
        self.submit_now(input)
    }

    fn submit_now(&mut self, input: &str) -> Option<UiEffect> {
        self.push_user(input);

        match classify_input(input) {
            Ok(InputAction::Prompt(prompt)) => self.start_prompt_job(&prompt),
            Ok(InputAction::Local(command)) => return self.handle_local(&command),
            Ok(InputAction::Shell(command)) => self.start_shell_job(&command),
            Ok(InputAction::Quit) => {
                self.status = "bye".to_string();
                return Some(UiEffect::Quit);
            }
            Ok(InputAction::Empty) => {}
            Err(error) => self.push_error(&format!("{error:#}")),
        }
        None
    }

    fn drain_queue(&mut self) {
        if self.is_busy() {
            return;
        }
        if let Some(next) = self.queued.pop_front() {
            let _ = self.submit_now(&next);
        }
    }

    fn handle_local(&mut self, command: &[String]) -> Option<UiEffect> {
        match command.first().map(String::as_str) {
            Some("help") => {
                self.show_help = !self.show_help;
                self.show_palette = false;
                self.status = "help toggled".to_string();
                return None;
            }
            Some("editor") => return Some(UiEffect::Editor),
            Some("login") => return Some(UiEffect::Login),
            Some("sessions") => {
                self.open_session_picker();
                return None;
            }
            Some("agents") => {
                self.open_background_tasks();
                return None;
            }
            Some("diff") => {
                let cwd = self.resolve_cwd();
                let diff_command = git_diff_command(&cwd, &command[1..]);
                self.start_job(diff_command, LogKind::Diff, "git diff");
                return None;
            }
            Some("mode") => {
                self.set_mode(command.get(1).map(String::as_str));
                return None;
            }
            Some("model") => {
                self.open_model_picker();
                return None;
            }
            Some("think") => {
                self.toggle_thought();
                return None;
            }
            Some("compact") => {
                self.compact_session();
                return None;
            }
            Some("usage") => {
                self.show_usage(command.get(1).map(String::as_str));
                return None;
            }
            Some("rewind") => {
                self.open_rewind_picker();
                return None;
            }
            Some("update") => {
                self.update_kernel();
                return None;
            }
            Some("copy") => {
                self.copy_last_reply();
                return None;
            }
            Some("resume") => {
                self.set_resume(command.get(1).map(String::as_str));
                return None;
            }
            Some("new") => {
                self.new_session();
                return None;
            }
            _ => {}
        }

        match handle_local_command(command, &self.config, &self.zcode_bin) {
            Ok(output) if output == "__CLEAR__" => {
                self.clear_log();
                self.status = "cleared".to_string();
            }
            Ok(output) if output.starts_with("__IDE__") => {
                self.launch_ide(output.trim_start_matches("__IDE__"));
            }
            Ok(output) => {
                // Direct answer to a user command (/skills list, /mcp list,
                // /status, …): show it whole, never folded.
                self.log
                    .push(LogLine::unfolded(LogKind::System, output.trim_end()));
                self.status = "ok".to_string();
            }
            Err(error) => self.push_error(&format!("{error:#}")),
        }
        if command.first().map(String::as_str) == Some("logout") {
            self.refresh_auth();
            self.model_provider = None;
            self.controls.models.clear();
            self.controls.model_current = None;
            self.pending_model = None;
        }
        None
    }

    /// Launch the IDE detached so it outlives the TUI and never blocks it.
    fn launch_ide(&mut self, raw: &str) {
        let parts = match shell_words::split(raw) {
            Ok(parts) if !parts.is_empty() => parts,
            _ => {
                self.push_error("invalid IDE command");
                return;
            }
        };
        let spawned = Command::new(&parts[0])
            .args(&parts[1..])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match spawned {
            Ok(_) => {
                self.push_system(&format!("opened in IDE: {}", parts.join(" ")));
                self.status = "ide launched".to_string();
            }
            Err(error) => self.push_error(&format!("failed to launch {}: {error}", parts[0])),
        }
    }

    fn start_prompt_job(&mut self, prompt: &str) {
        // ZCode 3.5.3 exposes Browser Use only on the official CLI surface;
        // strict session/create+send reject a guessed browserUse field. Route
        // these turns explicitly to --prompt so the flags are never ignored.
        if self.config.browser_use.is_some() {
            if !self.browser_route_noted {
                self.push_system(
                    "Browser Use is running through the classic ZCode CLI; token streaming, \
                     in-turn steer, and app-server controls are unavailable for these turns",
                );
                self.browser_route_noted = true;
            }
            self.start_prompt_job_via_cli(prompt);
            return;
        }
        // Default streaming path through the long-lived app-server. Any
        // failure downgrades this process
        // permanently and falls through to the classic --prompt path so the
        // user is never stuck (design D4).
        if self.app_mode == AppMode::Ready {
            match self.start_app_prompt(prompt) {
                Ok(()) => return,
                Err(reason) => self.downgrade_app_server(reason),
            }
        }
        self.start_prompt_job_via_cli(prompt);
    }

    fn start_prompt_job_via_cli(&mut self, prompt: &str) {
        match prompt_command_for(&self.zcode_bin, &self.config, prompt) {
            Ok(command) => {
                let live = self.prepare_live_progress();
                self.start_job(command, LogKind::Assistant, "zcode --prompt");
                if let Some(active) = &mut self.job {
                    active.live = live;
                }
            }
            Err(error) => self.push_error(&format!("{error:#}")),
        }
    }

    /// Drive one prompt through the app-server: spawn the connection lazily,
    /// create+subscribe a session on the first prompt (reused after), send the
    /// content, and open an assistant transcript entry the token stream grows
    /// into. Any protocol/IO failure returns an `AppServerUnavailable` for the
    /// caller to downgrade on.
    fn start_app_prompt(&mut self, prompt: &str) -> Result<(), AppServerUnavailable> {
        // A connection that died between turns is not reusable.
        if self.app_conn.as_ref().is_some_and(|conn| !conn.is_alive()) {
            return Err(AppServerUnavailable::Disconnected);
        }
        if self.app_conn.is_none() {
            self.app_conn = Some(AppServerConn::spawn(&self.zcode_bin)?);
            // A fresh connection has no session yet.
            self.app_session = None;
            self.reset_rewind_state();
        }
        match self.app_session.clone() {
            // Session already open: send immediately (the fast path for every
            // prompt after the first).
            Some(session_id) => {
                // Drop any stray tail from a previously cancelled turn.
                self.drain_app_events();
                // @file mentions ride along as attachments (streaming path
                // equivalent of the classic --attach translation).
                let attachments = self.app_attachments_for(prompt);
                let conn = self.app_conn.as_mut().expect("app_conn set above");
                let send_id = conn.send(
                    "session/send",
                    app_send_params_with_attachments(&session_id, prompt, &attachments),
                )?;
                self.begin_app_turn(send_id);
            }
            // First prompt of the run: kick off the (resume|create)+subscribe
            // handshake WITHOUT blocking the UI thread. The prompt is sent
            // once the handshake completes (driven by `pump_app_connect`), so
            // a slow or hung app-server can never freeze the terminal — Esc
            // still cancels. A pending `--resume`//sessions selection resumes
            // that session instead of silently opening a fresh one.
            None => {
                let workspace = self.app_workspace();
                let resume = self.config.resume.clone();
                // Resume restores the conversation but NOT the model runtime:
                // without this, the first send fails with
                // ZCODE_RUNTIME_MODEL_UNAVAILABLE (pinned live 2026-07-07).
                // Built from the kernel's own config.json; None falls back to
                // a bare resume + the create-fallback path.
                let runtime = resume.as_ref().and_then(|_| load_runtime_model());
                // Project + user MCP config rides along on create AND resume
                // (the kernel never reads project .mcp.json on its own).
                let mcp_servers = self.mcp_servers_for_session();
                let resume_params = resume.as_ref().map(|session_id| {
                    self.session_handshake_params(
                        app_resume_params(session_id, runtime.as_ref()),
                        mcp_servers.clone(),
                    )
                });
                let create_params =
                    self.session_handshake_params(app_create_params(&workspace), mcp_servers);
                let conn = self.app_conn.as_mut().expect("app_conn set above");
                let phase = match resume {
                    Some(_) => ConnectPhase::Resume(conn.send(
                        "session/resume",
                        resume_params.expect("resume params built for resume branch"),
                    )?),
                    None => ConnectPhase::Create(conn.send("session/create", create_params)?),
                };
                self.app_connect = Some(AppConnect {
                    phase,
                    prompt: prompt.to_string(),
                    started: Instant::now(),
                });
                self.status = "connecting (app-server)…".to_string();
            }
        }
        Ok(())
    }

    /// Absorb the extras a `session/create`/`resume` result carries beyond
    /// the sessionId: the kernel's slash-command list (into completion) and
    /// the TODO state (into the work panel).
    fn absorb_session_result(&mut self, result: &serde_json::Value) {
        let commands = parse_kernel_slash_commands(result);
        if !commands.is_empty() {
            self.kernel_commands = commands;
        }
        if let Some(todos) = parse_todos(result) {
            self.todos = todos;
        }
        if let Some(controls) = app_session_controls(result) {
            self.merge_controls(controls);
        }
    }

    /// The canonical workspace path handed to `session/create`.
    fn app_workspace(&self) -> String {
        self.resolve_cwd()
            .canonicalize()
            .unwrap_or_else(|_| self.resolve_cwd())
            .to_string_lossy()
            .into_owned()
    }

    /// `@file` mentions of a streaming prompt as `session/send` attachments —
    /// same extraction + traversal vetting as the classic `--attach` path.
    fn app_attachments_for(&self, prompt: &str) -> Vec<serde_json::Value> {
        let cwd = self.resolve_cwd();
        let mentions = extract_file_mentions(prompt, &cwd);
        build_send_attachments(&mentions, &cwd)
    }

    /// `mcpServers[]` for `session/create`/`resume`, re-read from the project
    /// `.mcp.json` + user config at handshake time (the kernel itself never
    /// loads project MCP config — pinned live 2026-07-07). None when empty.
    fn mcp_servers_for_session(&self) -> Option<serde_json::Value> {
        let project = mcp_config_path(&self.config)
            .ok()
            .and_then(|path| load_mcp_config(&path).ok())
            .unwrap_or_default();
        let user = user_mcp_config_path()
            .ok()
            .and_then(|path| load_mcp_config(&path).ok())
            .unwrap_or_default();
        mcp_servers_param(&project, &user)
    }

    fn session_handshake_params(
        &self,
        params: serde_json::Value,
        mcp_servers: Option<serde_json::Value>,
    ) -> serde_json::Value {
        with_tool_policy(
            with_mcp_servers(params, mcp_servers),
            &self.config.tool_allowlist,
            &self.config.tool_denylist,
        )
    }

    /// Best-effort `session/close` for a live session being discarded
    /// (/new, clean exit). Fire-and-forget: errors and the response are
    /// ignored; a dead connection skips silently.
    fn close_app_session(&mut self) {
        if let (Some(conn), Some(session_id)) = (self.app_conn.as_mut(), self.app_session.clone()) {
            if conn.is_alive() {
                let _ = conn.send("session/close", app_close_params(&session_id));
                self.log_debug("session/close sent (discarding live session)");
            }
        }
    }

    /// Copy the last assistant reply to the system clipboard via OSC52
    /// (Ctrl+X y or /copy). Written straight to the raw stdout the TUI owns;
    /// terminals consume the sequence without displaying anything. tmux
    /// needs `set -g set-clipboard on` (documented in the README).
    fn copy_last_reply(&mut self) {
        let text = self
            .log
            .iter()
            .rev()
            .find(|entry| entry.kind == LogKind::Assistant && !entry.text.trim().is_empty())
            .map(|entry| entry.text.clone());
        let Some(text) = text else {
            self.status = "nothing to copy yet".to_string();
            return;
        };
        match osc52_copy_sequence(&text, OSC52_MAX_B64) {
            Some(sequence) => {
                let mut out = io::stdout();
                let _ = out.write_all(sequence.as_bytes());
                let _ = out.flush();
                self.status = "copied last reply (OSC52)".to_string();
            }
            None => self.status = "nothing to copy yet".to_string(),
        }
    }

    /// Terminal bell when a turn/job that ran past the threshold finalizes
    /// (opt-out via `notify = off`); cancelled turns stay silent.
    fn ring_bell_if_long(&self, elapsed_secs: f32, cancelled: bool) {
        if cancelled || !self.notify_enabled || elapsed_secs <= NOTIFY_AFTER_SECS {
            return;
        }
        let mut out = io::stdout();
        let _ = out.write_all(b"\x07");
        let _ = out.flush();
    }

    /// Open the assistant turn: text and tool entries are created lazily as
    /// their events arrive so they land in transcript order.
    fn begin_app_turn(&mut self, send_id: u64) {
        self.app_turn = Some(AppTurn {
            turn: AppServerTurn::default(),
            committable_end: self.log.len(),
            text_index: None,
            written: 0,
            started: Instant::now(),
            cancel_requested: false,
            got_text: false,
            send_id,
        });
        self.status = "streaming (app-server)".to_string();
    }

    /// Drive the non-blocking create→subscribe handshake. Matches each response
    /// by id, then sends the queued prompt and opens the turn. Any failure or a
    /// 40s timeout downgrades and retries the prompt once via --prompt.
    fn pump_app_connect(&mut self) {
        if self.app_connect.is_none() {
            return;
        }
        if !self.app_conn.as_ref().is_some_and(AppServerConn::is_alive) {
            self.app_connect_failed(AppServerUnavailable::Disconnected);
            return;
        }
        while let Some(message) = self.app_conn.as_mut().and_then(AppServerConn::poll) {
            if let AppServerMessage::V4Frame(params) = message {
                self.apply_v4_frame(params);
                continue;
            }
            if let AppServerMessage::ServerRequest { id, method, params } = message {
                self.on_server_request(id, &method, &params);
                continue;
            }
            let AppServerMessage::Response { id, result, error } = message else {
                // Stray events/state before the turn starts: nothing to render.
                continue;
            };
            // Copy the awaited stage+id out so the match arms can mutate `self`.
            let waiting = match &self.app_connect {
                Some(connect) => match &connect.phase {
                    ConnectPhase::Create(want) => (ConnectStage::Create, *want),
                    ConnectPhase::Resume(want) => (ConnectStage::Resume, *want),
                    ConnectPhase::Subscribe(want) => (ConnectStage::Subscribe, *want),
                    ConnectPhase::V4Subscribe(want) => (ConnectStage::V4Subscribe, *want),
                },
                None => return,
            };
            let (stage, want) = waiting;
            if id != want {
                continue; // stale/unmatched response id
            }
            if let Some(why) = error {
                // V4 is an optional control-plane capability. Old kernels
                // return Method not found; other V4 failures are reported but
                // must not tear down the healthy legacy text stream.
                if matches!(stage, ConnectStage::V4Subscribe) {
                    self.v4_mode = V4Mode::Unavailable;
                    if why.contains("Method not found") {
                        self.log_debug("v4: unavailable (legacy kernel)");
                    } else {
                        self.push_system(&format!(
                            "V4 session controls unavailable ({why}); using legacy controls"
                        ));
                        self.log_debug(&format!("v4: subscribe failed ({why})"));
                    }
                    self.finish_app_connect();
                    return;
                }
                // A failed resume (session gone/foreign) is not fatal: note it
                // and redo the handshake with a fresh session. Anything else
                // downgrades as before.
                if matches!(stage, ConnectStage::Resume) {
                    self.config.resume = None;
                    self.push_system(&format!("resume failed ({why}); starting a fresh session"));
                    self.log_debug("handshake: resume failed, falling back to create");
                    let workspace = self.app_workspace();
                    let mcp_servers = self.mcp_servers_for_session();
                    let create_params =
                        self.session_handshake_params(app_create_params(&workspace), mcp_servers);
                    let conn = self.app_conn.as_mut().expect("alive checked above");
                    match conn.send("session/create", create_params) {
                        Ok(create_id) => {
                            if let Some(connect) = &mut self.app_connect {
                                connect.phase = ConnectPhase::Create(create_id);
                            }
                            continue;
                        }
                        Err(err) => {
                            self.app_connect_failed(err);
                            return;
                        }
                    }
                }
                self.app_connect_failed(AppServerUnavailable::Protocol(why));
                return;
            }
            match stage {
                ConnectStage::Create | ConnectStage::Resume => {
                    let result = result.unwrap_or(serde_json::Value::Null);
                    let Some(session_id) = app_session_id_from_result(&result) else {
                        self.app_connect_failed(AppServerUnavailable::Protocol(
                            "session create/resume missing session.sessionId".to_string(),
                        ));
                        return;
                    };
                    // The result also carries the kernel's command list and
                    // TODO state (same shape for create and resume).
                    self.absorb_session_result(&result);
                    if matches!(stage, ConnectStage::Resume) {
                        // One-shot: the picked session is now live; later
                        // prompts reuse app_session, /new clears both.
                        self.config.resume = None;
                        self.session_active = true;
                        let count = result
                            .get("messages")
                            .and_then(|m| m.as_array())
                            .map(|m| m.len())
                            .unwrap_or(0);
                        self.push_system(&format!(
                            "resumed {session_id} ({count} messages of history)"
                        ));
                        // Compact history replay: the last few exchanges as
                        // dim one-liners under the header, so the resumed
                        // context is visible without re-rendering the turn.
                        for message in parse_resume_messages(&result, REPLAY_LIMIT, REPLAY_CAP) {
                            let prefix = if message.role == "user" {
                                "› "
                            } else {
                                "· "
                            };
                            let flat = message.preview.replace('\n', " ");
                            self.push_system(&format!("{prefix}{flat}"));
                        }
                        self.refresh_banner();
                    }
                    let conn = self.app_conn.as_mut().expect("alive checked above");
                    match conn.send("session/subscribe", app_subscribe_params(&session_id)) {
                        Ok(sub_id) => {
                            self.app_session = Some(session_id);
                            self.reset_rewind_state();
                            if let Some(connect) = &mut self.app_connect {
                                connect.phase = ConnectPhase::Subscribe(sub_id);
                            }
                        }
                        Err(err) => {
                            self.app_connect_failed(err);
                            return;
                        }
                    }
                }
                ConnectStage::Subscribe => {
                    // Layer the optional V4 control subscription over the
                    // working legacy body stream before the first prompt.
                    let session_id = self.app_session.clone().expect("set on create");
                    let conn = self.app_conn.as_mut().expect("alive checked above");
                    match conn.send(
                        "v4/conversation/subscribe",
                        v4_conversation_subscribe_params(&session_id, &self.v4_client_id),
                    ) {
                        Ok(v4_id) => {
                            if let Some(connect) = &mut self.app_connect {
                                connect.phase = ConnectPhase::V4Subscribe(v4_id);
                            }
                        }
                        Err(err) => {
                            self.app_connect_failed(err);
                        }
                    }
                }
                ConnectStage::V4Subscribe => {
                    self.v4_mode = V4Mode::Available;
                    self.log_debug("v4: conversation controls available");
                    self.finish_app_connect();
                    return;
                }
            }
        }
        // Handshake watchdog: never let a hung app-server strand the prompt.
        if self
            .app_connect
            .as_ref()
            .is_some_and(|c| c.started.elapsed() > Duration::from_secs(40))
        {
            self.app_connect_failed(AppServerUnavailable::Handshake(
                "session handshake timed out".to_string(),
            ));
        }
    }

    /// Finish the hybrid handshake and send its queued first prompt. The V4
    /// initial frame is emitted immediately after the subscribe response and
    /// will be consumed by `pump_app_turn` on the next tick.
    fn finish_app_connect(&mut self) {
        let Some(connect) = self.app_connect.take() else {
            return;
        };
        let Some(session_id) = self.app_session.clone() else {
            self.downgrade_app_server(AppServerUnavailable::Protocol(
                "session missing after subscribe".to_string(),
            ));
            self.start_prompt_job_via_cli(&connect.prompt);
            return;
        };
        if let Some(mode) = self.config.mode.clone() {
            self.send_control(
                "session/setMode",
                app_set_mode_params(&session_id, &mode),
                ControlReq::Command("/mode"),
            );
        }
        if let Some(choice) = self.pending_model.take() {
            self.send_control(
                "session/setModel",
                app_set_model_params(&session_id, &choice.reference),
                ControlReq::Command("/model"),
            );
        }
        let attachments = self.app_attachments_for(&connect.prompt);
        self.log_debug("handshake: legacy+v4 negotiation complete, sending first prompt");
        let result =
            self.app_conn
                .as_mut()
                .map_or(Err(AppServerUnavailable::Disconnected), |conn| {
                    conn.send(
                        "session/send",
                        app_send_params_with_attachments(
                            &session_id,
                            &connect.prompt,
                            &attachments,
                        ),
                    )
                });
        match result {
            Ok(send_id) => self.begin_app_turn(send_id),
            Err(err) => {
                self.downgrade_app_server(err);
                self.start_prompt_job_via_cli(&connect.prompt);
            }
        }
    }

    /// Handshake failed before any turn started: downgrade and retry the
    /// prompt once via the classic --prompt path (nothing was shown yet).
    fn app_connect_failed(&mut self, reason: AppServerUnavailable) {
        let prompt = self.app_connect.take().map(|c| c.prompt);
        self.downgrade_app_server(reason);
        if let Some(prompt) = prompt {
            self.start_prompt_job_via_cli(&prompt);
        }
    }

    /// Discard whatever is currently buffered on the connection without
    /// applying it (used to clear a cancelled turn's tail before a new turn).
    fn drain_app_events(&mut self) {
        while let Some(message) = self.app_conn.as_mut().and_then(AppServerConn::poll) {
            if let AppServerMessage::V4Frame(params) = message {
                self.apply_v4_frame(params);
            }
        }
    }

    /// Retire the app-server path for the rest of this run and note it once.
    fn downgrade_app_server(&mut self, reason: AppServerUnavailable) {
        self.log_debug(&format!("downgrade: {reason}"));
        self.app_mode = AppMode::Downgraded;
        self.app_turn = None;
        self.app_connect = None;
        self.app_draining = None;
        self.app_session = None;
        self.reset_rewind_state();
        // Tear the connection down for good (process-group kill via Drop).
        self.app_conn = None;
        self.push_system(&format!(
            "{reason}; falling back to --prompt for this session"
        ));
    }

    /// Snapshot the db just before spawning so polling only ever attributes
    /// rows created by this run. Any failure means no live progress — the
    /// job itself is unaffected.
    fn prepare_live_progress(&self) -> Option<LiveProgress> {
        let DbState::Enabled(path) = &self.db_state else {
            return None;
        };
        let conn = open_kernel_db_ro(path).ok()?;
        let directory = self
            .resolve_cwd()
            .canonicalize()
            .unwrap_or_else(|_| self.resolve_cwd())
            .to_string_lossy()
            .into_owned();
        let latest = latest_session_for_dir(&conn, &directory);
        let (session_id, prior_session) = if let Some(resume) = &self.config.resume {
            (Some(resume.clone()), None)
        } else if self.config.continue_session || self.session_active {
            (latest, None)
        } else {
            // Fresh run: the kernel will create a new session row (~1.6s in);
            // remember the current latest so polling can tell them apart.
            (None, latest)
        };
        Some(LiveProgress {
            directory,
            session_id,
            prior_session,
            baseline: db_baseline(&conn),
            chips: Vec::new(),
            reasoning: None,
            text: None,
        })
    }

    /// Poll the kernel db (~every 5th 80ms tick) while an assistant job
    /// runs. Every failure path is "skip this tick".
    fn poll_live_progress(&mut self) {
        if !self.tick.is_multiple_of(5) {
            return;
        }
        let DbState::Enabled(path) = &self.db_state else {
            return;
        };
        let path = path.clone();
        let Some(active) = &mut self.job else { return };
        if active.kind != LogKind::Assistant {
            return;
        }
        let Some(live) = &mut active.live else { return };
        let Ok(conn) = open_kernel_db_ro(&path) else {
            return;
        };
        if live.session_id.is_none() {
            live.session_id = latest_session_for_dir(&conn, &live.directory)
                .filter(|candidate| Some(candidate) != live.prior_session.as_ref());
        }
        let Some(session_id) = live.session_id.clone() else {
            return;
        };
        if let Ok(chips) = live_tool_chips(&conn, &session_id, live.baseline) {
            live.chips = chips;
        }
        if let Ok(Some(reasoning)) = latest_reasoning(&conn, &session_id, live.baseline) {
            live.reasoning = Some(reasoning);
        }
        if let Ok(text) = latest_assistant_text(&conn, &session_id, live.baseline) {
            live.text = text;
        }
    }

    fn start_shell_job(&mut self, command: &str) {
        self.push_system(&format!("$ {command}"));
        let full = vec!["sh".to_string(), "-lc".to_string(), command.to_string()];
        self.start_job(full, LogKind::System, "! shell");
    }

    /// /update — self-update the ZCode kernel from the official feed: fetch
    /// latest-linux.yml, compare versions (dpkg semantics), download the deb,
    /// verify its sha512 against the feed (base64), then install via
    /// passwordless sudo or print the no-root unpack path. Runs as a shell
    /// job: streaming output in the transcript, Esc cancels the download.
    fn update_kernel(&mut self) {
        let app_dir = active_zcode_app_dir();
        let feed_url = update_feed_url_for(app_dir.as_deref());
        let Some(feed_url) = feed_url else {
            self.push_error(
                "no update feed found (is the ZCode desktop package installed?); \
                 set ZCODE_APP or ZCODE_TUI_UPDATE_FEED",
            );
            return;
        };
        let installed = installed_zcode_version(app_dir.as_deref()).unwrap_or_else(|| "0".into());
        let feed_arg = shell_words::join([feed_url.as_str()]);
        let installed_arg = shell_words::join([installed.as_str()]);
        // Everything network/versioned happens inside the job so the UI never
        // blocks: yml fetch, version compare (dpkg --compare-versions), deb
        // download, sha512 check, install. The awk mirrors parse_update_feed:
        // the sha512 belongs to the files[] entry whose url is the .deb.
        let script = format!(
            r#"set -eu
FEED={feed_arg}
echo "feed: $FEED"
YML=$(curl -fsSL --max-time 15 "$FEED")
VER=$(printf '%s' "$YML" | sed -n 's/^version:[[:space:]]*//p' | head -1)
DEB_ENTRY=$(printf '%s' "$YML" | sed -n 's/^[[:space:]]*-*[[:space:]]*url:[[:space:]]*//p' | grep '\.deb$' | head -1)
DEB=$(basename "$DEB_ENTRY")
SHA=$(printf '%s' "$YML" | awk '/url:.*\.deb$/{{f=1;next}} f&&/sha512:/{{sub(/^[[:space:]]*sha512:[[:space:]]*/,"");print;exit}}')
INSTALLED={installed_arg}
echo "installed: $INSTALLED   latest: $VER"
if ! dpkg --compare-versions "$VER" gt "$INSTALLED"; then
  echo "already up to date"
  exit 0
fi
[ -n "$DEB" ] && [ -n "$SHA" ] || {{ echo "feed carries no deb entry/sha512 - aborting"; exit 1; }}
BASE=${{FEED%latest-linux.yml}}
case "$DEB_ENTRY" in
  http://*|https://*) DOWNLOAD=$DEB_ENTRY ;;
  *://*) echo "unsupported deb URL scheme - aborting"; exit 1 ;;
  *) DOWNLOAD=$BASE$DEB ;;
esac
TMP=$(mktemp -d /tmp/zcode-update.XXXXXX)
echo "downloading $DOWNLOAD"
curl -fSL --retry 3 --retry-delay 2 -o "$TMP/$DEB" "$DOWNLOAD"
echo "verifying sha512"
ACTUAL=$(openssl dgst -sha512 -binary "$TMP/$DEB" | base64 -w0)
if [ "$ACTUAL" != "$SHA" ]; then
  echo "sha512 MISMATCH - download corrupt, aborting (file removed)"
  rm -f "$TMP/$DEB"
  exit 1
fi
echo "sha512 ok"
if sudo -n true 2>/dev/null; then
  echo "installing $VER (do not cancel)"
  sudo -n dpkg -i "$TMP/$DEB"
  rm -f "$TMP/$DEB"
  echo "installed $VER - restart zcode-tui to use the new kernel"
else
  echo "no passwordless sudo; finish manually:"
  echo "  sudo dpkg -i $TMP/$DEB"
  echo "or unpack without root:"
  echo "  dpkg-deb -x $TMP/$DEB ~/.local/opt/zcode/$VER/"
fi"#
        );
        self.push_system("checking the official update feed…");
        let full = vec!["sh".to_string(), "-c".to_string(), script];
        self.start_job(full, LogKind::System, "/update");
    }

    fn start_job(&mut self, command: Vec<String>, kind: LogKind, label: &str) {
        match spawn_streaming_command(&command) {
            Ok(job) => {
                self.log.push(LogLine::new(kind, ""));
                self.job = Some(ActiveJob {
                    job,
                    log_index: self.log.len() - 1,
                    kind,
                    label: label.to_string(),
                    finished: None,
                    finished_at: None,
                    eofs: 0,
                    entry_started: false,
                    any_output: false,
                    cancel_requested: false,
                    started: Instant::now(),
                    raw: Vec::new(),
                    errs: Vec::new(),
                    live: None,
                });
                self.status = format!("running {label}");
            }
            Err(error) => self.push_error(&format!("{error:#}")),
        }
    }

    fn cancel_job(&mut self) {
        if let Some(active) = &mut self.job {
            active.cancel_requested = true;
            active.job.cancel();
            self.status = "cancelling...".to_string();
        }
    }

    /// Cancel whichever path is running: the app-server turn (graceful
    /// `session/stop`, keeping the connection for the next prompt) or the
    /// classic child job.
    fn cancel_current(&mut self) {
        // Cancelled before the first turn even started (still handshaking):
        // drop the half-open connection so a clean one is spawned next time.
        if self.app_connect.is_some() {
            self.app_connect = None;
            self.app_conn = None;
            self.app_session = None;
            self.reset_rewind_state();
            self.status = "cancelled".to_string();
            return;
        }
        if self.app_turn.is_some() {
            if let (Some(conn), Some(session_id)) =
                (self.app_conn.as_mut(), self.app_session.clone())
            {
                let _ = conn.send("session/stop", app_stop_params(&session_id));
            }
            if let Some(turn) = &mut self.app_turn {
                turn.cancel_requested = true;
            }
            self.finalize_app_turn();
            // The kernel keeps emitting the stopped turn's tail (its own
            // `prompt_completed` included). Swallow it before the next prompt
            // reuses the session, or it would bleed into — and prematurely
            // finalize — the next turn.
            if self.app_conn.is_some() {
                self.app_draining = Some(Instant::now());
            }
        } else {
            self.cancel_job();
        }
    }

    /// Drain app-server events into the streaming turn each loop tick. Text
    /// deltas grow the transcript entry live; a `finish` event finalizes; a
    /// dead connection downgrades and either retries or keeps the partial.
    fn pump_app_turn(&mut self) {
        // A cancelled turn's tail is being swallowed before the next prompt.
        if self.app_draining.is_some() {
            self.drain_cancelled_turn();
            return;
        }
        if self.app_turn.is_none() {
            return;
        }
        // The connection dying mid-turn is a hard failure.
        if !self.app_conn.as_ref().is_some_and(AppServerConn::is_alive) {
            self.app_turn_connection_lost();
            return;
        }
        while let Some(message) = self.app_conn.as_mut().and_then(AppServerConn::poll) {
            match message {
                AppServerMessage::Event(event) => {
                    // Retain checkpoint ids as /rewind targets (the turn
                    // itself only counts them for the files-changed note).
                    self.capture_rewind_event(&event);
                    // ZCode 3.3.4 background tasks (subagent/bash backgrounding).
                    // Surface lifecycle events as a safe system line when an
                    // app-server version emits them.
                    if matches!(
                        event.kind.as_str(),
                        "background_task_started"
                            | "background_task_updated"
                            | "background_task_completed"
                    ) {
                        self.capture_background_task_event(&event);
                        self.log.push(LogLine::new(
                            LogKind::System,
                            &format_background_task(&event),
                        ));
                        self.app_commit_phase();
                        return;
                    }
                    let Some(turn) = &mut self.app_turn else {
                        return;
                    };
                    match turn.turn.apply(&event) {
                        TurnDelta::Text => self.app_append_text(),
                        TurnDelta::ToolFinished(idx) => self.app_push_tool_entry(idx),
                        TurnDelta::Done => {
                            self.finalize_app_turn();
                            return;
                        }
                        TurnDelta::ToolStarted(_) => self.app_commit_phase(),
                        // Reasoning/None need no transcript change.
                        TurnDelta::Reasoning | TurnDelta::None => {}
                    }
                }
                AppServerMessage::StateUpdated(params) => {
                    if let Some(watermark) = app_state_watermark(&params) {
                        self.context_watermark = Some(watermark);
                    }
                    if let Some(controls) = app_state_controls(&params) {
                        self.merge_controls(controls);
                    }
                    if let Some(todos) = parse_todos(&params) {
                        self.todos = todos;
                    }
                    // The kernel ends a turn with a `prompt_completed` state
                    // update, not a session/event — this is the terminator.
                    if app_state_is_turn_end(&params) {
                        self.finalize_app_turn();
                        return;
                    }
                    // Abnormal end (error/aborted/…): close the turn with a note
                    // instead of hanging until the 600s backstop.
                    if let Some(why) = app_state_turn_error(&params) {
                        self.end_app_turn_abnormally(&why);
                        return;
                    }
                }
                AppServerMessage::V4Frame(params) => self.apply_v4_frame(params),
                // The kernel asking *us* something (permission approval).
                AppServerMessage::ServerRequest { id, method, params } => {
                    self.on_server_request(id, &method, &params);
                }
                AppServerMessage::Response {
                    id,
                    error: Some(message),
                    ..
                } => {
                    // A failed control command (setMode/steer/…) only concerns
                    // that command — report it, keep the turn.
                    if self.on_control_error(id, &message) {
                        continue;
                    }
                    // Only *this* turn's own send failing is fatal. A stray
                    // error Response (e.g. a prior cancel's session/stop) must
                    // not down the healthy turn now streaming.
                    if self.app_turn.as_ref().is_some_and(|t| t.send_id == id) {
                        self.abort_app_turn(AppServerUnavailable::Protocol(message));
                        return;
                    }
                }
                AppServerMessage::Response { id, result, .. } => {
                    // Successful control command; a steer's result still
                    // needs its queued/rejected union inspected.
                    self.on_control_ok(id, result.as_ref());
                }
                AppServerMessage::Other => {}
            }
        }
        // Backstop: a turn that never gets a finish event and never errors
        // would otherwise hang forever. Give up after a generous ceiling.
        if self
            .app_turn
            .as_ref()
            .is_some_and(|t| t.started.elapsed() > Duration::from_secs(600))
        {
            self.abort_app_turn(AppServerUnavailable::Handshake(
                "turn produced no completion".to_string(),
            ));
        }
    }

    /// Flush newly-arrived answer text into the open assistant entry, opening a
    /// fresh one if the previous run was closed by a tool landing.
    fn app_append_text(&mut self) {
        let (full_len, written, existing) = match &self.app_turn {
            Some(turn) => (turn.turn.text.len(), turn.written, turn.text_index),
            None => return,
        };
        if full_len <= written {
            return;
        }
        let suffix = self.app_turn.as_ref().unwrap().turn.text[written..].to_string();
        let idx = existing.unwrap_or_else(|| {
            self.log.push(LogLine::new(LogKind::Assistant, ""));
            self.log.len() - 1
        });
        self.log[idx].text.push_str(&suffix);
        let non_empty = !self.log[idx].text.is_empty();
        if let Some(turn) = &mut self.app_turn {
            turn.text_index = Some(idx);
            turn.written = full_len;
            turn.got_text |= non_empty;
        }
    }

    /// Freeze the current phase so its entries can append to scrollback. Any
    /// later text delta opens a new entry and never mutates a flushed row.
    fn app_commit_phase(&mut self) {
        let end = self.log.len();
        if let Some(turn) = &mut self.app_turn {
            turn.text_index = None;
            turn.committable_end = end;
        }
    }

    /// Persist a finished tool call into the transcript as a foldable `Tool`
    /// entry (header + output), then close the current text run so following
    /// answer text opens a new entry — tools and text stay in turn order.
    fn app_push_tool_entry(&mut self, idx: usize) {
        let text = {
            let Some(turn) = &self.app_turn else { return };
            let Some(tool) = turn.turn.tools.get(idx) else {
                return;
            };
            let mut header = if tool.name.is_empty() {
                "tool".to_string()
            } else {
                tool.name.clone()
            };
            let summary = tool_input_summary(&tool.input);
            if !summary.is_empty() {
                header.push_str(&format!("  {summary}"));
            }
            if let Some(ms) = tool.duration_ms {
                if ms >= 1000 {
                    header.push_str(&format!("  · {:.1}s", ms as f32 / 1000.0));
                } else {
                    header.push_str(&format!("  · {ms}ms"));
                }
            }
            if !tool.success {
                header.push_str("  · failed");
            }
            let output = tool.output.trim_end();
            if output.is_empty() {
                header
            } else {
                format!("{header}\n{output}")
            }
        };
        self.log.push(LogLine::new(LogKind::Tool, &text));
        self.app_commit_phase();
    }

    /// The connection dropped while a turn was streaming. If nothing reached the
    /// transcript yet, retry this prompt on --prompt; otherwise keep the partial.
    fn app_turn_connection_lost(&mut self) {
        let Some(turn) = self.app_turn.take() else {
            return;
        };
        let prompt = self
            .log
            .iter()
            .rev()
            .find(|entry| entry.kind == LogKind::User)
            .map(|entry| entry.text.clone());
        self.downgrade_app_server(AppServerUnavailable::Disconnected);
        if turn.got_text || !turn.turn.tools.is_empty() {
            // Text and/or tool output already landed; keep it, just downgrade.
            self.status = "app-server dropped; kept partial reply".to_string();
        } else if let Some(prompt) = prompt {
            // Nothing shown yet: retry the whole prompt via --prompt.
            self.start_prompt_job_via_cli(&prompt);
        }
    }

    /// Abort the current turn with a reason and permanently downgrade. Any
    /// content already streamed into the transcript stays put.
    fn abort_app_turn(&mut self, reason: AppServerUnavailable) {
        self.downgrade_app_server(reason);
    }

    /// End the current turn abnormally (kernel signalled error/aborted). Keep
    /// whatever streamed, note it, but stay on the app-server path — this is a
    /// turn-level end, not a connection failure, so the session lives on.
    fn end_app_turn_abnormally(&mut self, why: &str) {
        if self.app_turn.is_none() {
            return;
        }
        self.push_system(&format!("app-server ended the turn: {why}"));
        self.finalize_app_turn();
        self.status = format!("ended ({why})");
    }

    /// Swallow a cancelled turn's trailing events until its terminator lands, so
    /// nothing bleeds into the next prompt on the reused session. Context
    /// watermarks are still worth keeping; everything else is discarded. If the
    /// kernel never sends a clean terminator, give up after a ceiling and force
    /// a fresh session next prompt so no straggler can leak in.
    fn drain_cancelled_turn(&mut self) {
        if !self.app_conn.as_ref().is_some_and(AppServerConn::is_alive) {
            self.app_draining = None;
            return;
        }
        while let Some(message) = self.app_conn.as_mut().and_then(AppServerConn::poll) {
            match message {
                AppServerMessage::StateUpdated(params) => {
                    if let Some(watermark) = app_state_watermark(&params) {
                        self.context_watermark = Some(watermark);
                    }
                    if app_state_is_turn_end(&params) || app_state_turn_error(&params).is_some() {
                        self.app_draining = None;
                        return;
                    }
                }
                AppServerMessage::V4Frame(params) => self.apply_v4_frame(params),
                _ => {}
            }
        }
        if self
            .app_draining
            .is_some_and(|started| started.elapsed() > Duration::from_secs(10))
        {
            self.app_draining = None;
            self.app_session = None; // recreate → guaranteed clean next prompt
            self.reset_rewind_state();
        }
    }

    /// Finalize the current app-server turn: mark the session live for
    /// continuity and update status. Text/tool entries are already in place.
    fn finalize_app_turn(&mut self) {
        let Some(turn) = self.app_turn.take() else {
            return;
        };
        let elapsed = turn.started.elapsed().as_secs_f32();
        if !turn.got_text && turn.turn.tools.is_empty() {
            self.log
                .push(LogLine::new(LogKind::Assistant, "(no output)"));
        }
        // Files-changed turn summary from checkpoint.created events (one per
        // gated write; fileCount summed). Shown even on cancel — the files
        // DID change and /diff is exactly what the user wants next.
        if turn.turn.files_changed > 0 {
            self.push_system(&format!(
                "{} file(s) changed · /diff to review",
                turn.turn.files_changed
            ));
        }
        if turn.cancel_requested {
            self.status = "cancelled".to_string();
            self.push_system("app-server turn cancelled");
        } else {
            self.status = format!("done ({elapsed:.1}s)");
        }
        self.log_debug(&format!(
            "turn finalized ({elapsed:.1}s, cancelled={})",
            turn.cancel_requested
        ));
        self.ring_bell_if_long(elapsed, turn.cancel_requested);
        // A turn landed in a live kernel session: keep continuity by reusing
        // the same sessionId for later prompts (already stored in app_session).
        self.session_active = true;
    }

    /// Consume connection messages BETWEEN turns. Control echoes
    /// (`mode_changed` after /mode·/model·/think), watermark refreshes after
    /// /compact, and control-command responses would otherwise sit unread in
    /// the channel until the next turn happens to poll.
    fn pump_app_idle(&mut self) {
        if self.app_turn.is_some() || self.app_connect.is_some() || self.app_draining.is_some() {
            return;
        }
        while let Some(message) = self.app_conn.as_mut().and_then(AppServerConn::poll) {
            match message {
                AppServerMessage::StateUpdated(params) => {
                    if let Some(watermark) = app_state_watermark(&params) {
                        self.context_watermark = Some(watermark);
                    }
                    if let Some(controls) = app_state_controls(&params) {
                        self.merge_controls(controls);
                    }
                    if let Some(todos) = parse_todos(&params) {
                        self.todos = todos;
                    }
                }
                AppServerMessage::V4Frame(params) => self.apply_v4_frame(params),
                AppServerMessage::ServerRequest { id, method, params } => {
                    self.on_server_request(id, &method, &params);
                }
                AppServerMessage::Response {
                    id,
                    error: Some(message),
                    ..
                } => {
                    self.on_control_error(id, &message);
                }
                AppServerMessage::Response { id, result, .. } => {
                    self.on_control_ok(id, result.as_ref())
                }
                // Idle events matter to /rewind: the kernel runs a rewind as
                // a synthetic turn (turn.started → rewind.triggered →
                // turn.completed) outside any prompt of ours.
                AppServerMessage::Event(event) => {
                    self.capture_rewind_event(&event);
                    self.capture_background_task_event(&event);
                }
                AppServerMessage::Other => {}
            }
        }
    }

    /// Session-level events the /rewind flow feeds on: `checkpoint.created`
    /// (target accumulation) and `rewind.triggered` (the only reliable
    /// rewind outcome — the envelope lies on failure).
    fn capture_rewind_event(&mut self, event: &AppServerEvent) {
        match event.kind.as_str() {
            "checkpoint.created" => {
                if let Some(id) = &event.checkpoint_id {
                    self.log_debug(&format!(
                        "rewind: checkpoint captured {} ({} file(s))",
                        id,
                        event.file_count.unwrap_or(0)
                    ));
                    self.checkpoints.push(CheckpointEntry {
                        id: id.clone(),
                        files: event.file_count.unwrap_or(0),
                        message_id: event.target_message_id.clone(),
                    });
                }
            }
            "rewind.triggered" => {
                let strategy = event.strategy.clone().unwrap_or_default();
                let reason = event.reason.clone().unwrap_or_default();
                self.log_debug(&format!(
                    "rewind: triggered strategy={strategy} reason={reason}"
                ));
                self.rewind_trigger = Some((strategy, reason));
            }
            _ => {}
        }
    }

    /// Cache the latest lifecycle state for the read-only /agents overlay.
    fn capture_background_task_event(&mut self, event: &AppServerEvent) {
        self.agents.ingest(event);
    }

    /// Merge a control-surface push into the cache, echoing actual changes in
    /// the status line (the push is the authoritative confirmation — control
    /// commands never update state optimistically).
    fn merge_controls(&mut self, update: SessionControls) {
        if let Some(mode) = update.mode {
            if self.controls.mode.as_deref() != Some(mode.as_str()) && self.controls.mode.is_some()
            {
                self.status = format!("mode {mode}");
                self.config.mode = Some(mode.clone());
                self.refresh_banner();
            }
            self.controls.mode = Some(mode);
        }
        if let Some(provider) = update.model_provider {
            if self.model_provider.as_deref() != Some(provider.as_str()) {
                self.controls.models.clear();
            }
            self.model_provider = Some(provider);
        }
        let model_provider = self.model_provider.clone();
        for model in update.models.into_iter().filter(|model| {
            model_provider.as_deref().is_none_or(|provider_id| {
                model
                    .reference
                    .get("providerId")
                    .and_then(serde_json::Value::as_str)
                    == Some(provider_id)
            })
        }) {
            if let Some(index) = self
                .controls
                .models
                .iter()
                .position(|existing| existing.reference == model.reference)
            {
                self.controls.models[index] = model;
            } else {
                self.controls.models.push(model);
            }
        }
        if let Some(current) = update.model_current {
            if self.controls.model_current.as_deref() != Some(current.as_str())
                && self.controls.model_current.is_some()
            {
                self.status = format!("model {current}");
            }
            self.controls.model_current = Some(current);
        }
        if !update.thought_levels.is_empty() {
            self.controls.thought_levels = update.thought_levels;
        }
        if let Some(current) = update.thought_current {
            if self.controls.thought_current.as_deref() != Some(current.as_str())
                && self.controls.thought_current.is_some()
            {
                self.status = format!("thinking {current}");
            }
            self.controls.thought_current = Some(current);
        }
    }

    /// Dispatch a server→client request. Both interaction methods (user
    /// input / tool permission) are understood; anything else stays
    /// unanswered — the kernel's retry keeps it alive for a future, more
    /// capable client.
    fn on_server_request(
        &mut self,
        id: serde_json::Value,
        method: &str,
        params: &serde_json::Value,
    ) {
        if let Some(line) = encode_runtime_preferences_reply(&id, method) {
            match self.app_conn.as_mut().map(|conn| conn.reply(&line)) {
                Some(Ok(())) => self.log_debug("runtime preferences replied"),
                Some(Err(reason)) => {
                    self.push_error(&format!("runtime preferences reply failed: {reason}"))
                }
                None => {
                    self.push_error("runtime preferences reply failed: app-server disconnected")
                }
            }
            return;
        }
        let Some(request) = parse_interaction_request(method, params) else {
            return;
        };
        if self.interaction_done.contains(&request.request_id) {
            return; // late re-send of an already-answered request
        }
        match &mut self.interaction {
            // Re-send of the open request: refresh the envelope id to reply on.
            Some(pending) if pending.request.request_id == request.request_id => {
                pending.envelope_id = id;
            }
            // A different request while one is open: the kernel's retry will
            // re-deliver it once the current one is answered.
            Some(_) => {}
            None => {
                self.status = format!("approval required: {}", request.tool_name);
                self.interaction = Some(PendingInteraction {
                    request,
                    envelope_id: id,
                    selected: 0,
                });
            }
        }
    }

    /// Send a fire-and-forget control request, remembering its id so a later
    /// error response can name the command. A steer that cannot be sent falls
    /// back to the queue (spec: steer failure must not lose the input).
    fn send_control(&mut self, method: &str, params: serde_json::Value, req: ControlReq) {
        let result = match self.app_conn.as_mut() {
            Some(conn) => conn.send(method, params),
            None => Err(AppServerUnavailable::Disconnected),
        };
        match result {
            Ok(id) => {
                self.control_requests.insert(id, req);
            }
            Err(reason) => {
                self.push_error(&format!("{method} failed: {reason}"));
                match req {
                    ControlReq::Steer(content) => self.queued.push_back(content),
                    ControlReq::V4SetGuide { content, .. }
                    | ControlReq::V4SteerText { content, .. } => self.queued.push_back(content),
                    // A rewind leg that never left must not wedge the overlay.
                    ControlReq::RewindPreview(_) => {
                        if let Some(overlay) = &mut self.rewind {
                            overlay.busy = false;
                        }
                    }
                    ControlReq::RewindApplyFiles { .. }
                    | ControlReq::RewindConversation(_)
                    | ControlReq::V4RewindApply(_) => {
                        self.close_rewind_overlay();
                    }
                    _ => {}
                }
            }
        }
    }

    /// A control command's error response: report it against the command and
    /// swallow it (never fatal to the turn/session). True when `id` was ours.
    fn on_control_error(&mut self, id: u64, message: &str) -> bool {
        match self.control_requests.remove(&id) {
            Some(ControlReq::Command(name)) => {
                self.push_error(&format!("{name} failed: {message}"));
                true
            }
            Some(ControlReq::Steer(content)) => {
                self.push_error(&format!("steer failed: {message} (input queued)"));
                self.queued.push_back(content);
                true
            }
            Some(ControlReq::V4SetGuide {
                content,
                command_id,
            })
            | Some(ControlReq::V4SteerText {
                content,
                command_id,
            }) => {
                self.push_error(&format!("V4 steer failed: {message} (input queued)"));
                self.log_debug(&format!("v4 steer command={command_id} transport-error"));
                self.queued.push_back(content);
                true
            }
            Some(ControlReq::Usage(tag)) => {
                self.push_error(&format!("usage {tag} failed: {message}"));
                true
            }
            // A rewind leg erroring never harms the session — report, close
            // the overlay so the user is not stuck on a dead preview.
            Some(ControlReq::RewindPreview(target)) => {
                self.push_error(&format!("rewind preview failed: {message}"));
                self.log_debug(&format!(
                    "rewind: preview error for {} ({message})",
                    target.label()
                ));
                if let Some(overlay) = &mut self.rewind {
                    overlay.busy = false;
                }
                true
            }
            Some(ControlReq::RewindApplyFiles { target, .. })
            | Some(ControlReq::RewindConversation(target))
            | Some(ControlReq::V4RewindApply(target)) => {
                self.push_error(&format!("rewind failed: {message}"));
                self.log_debug(&format!(
                    "rewind: apply error for {} ({message})",
                    target.label()
                ));
                self.close_rewind_overlay();
                true
            }
            None => false,
        }
    }

    /// A control command succeeded at the envelope level. /compact gets a
    /// direct status (no push marks completion). A steer's OK envelope still
    /// carries a union result — `kind:"rejected"` means the input did NOT
    /// enter the turn and must be requeued, not silently lost.
    fn on_control_ok(&mut self, id: u64, result: Option<&serde_json::Value>) {
        match self.control_requests.remove(&id) {
            Some(ControlReq::Command("/compact")) => {
                self.status = "compacted".to_string();
            }
            Some(ControlReq::Steer(content)) => {
                if let Some(SteerOutcome::Rejected(reason)) = result.map(parse_steer_result) {
                    self.push_error(&format!("steer rejected: {reason} (input queued)"));
                    self.queued.push_back(content);
                }
            }
            Some(ControlReq::V4SetGuide {
                content,
                command_id,
            }) => self.on_v4_set_guide_ok(content, command_id, result),
            Some(ControlReq::V4SteerText {
                content,
                command_id,
            }) => self.on_v4_steer_text_ok(content, command_id, result),
            Some(ControlReq::Usage(tag)) => {
                if let Some(result) = result {
                    let text = match tag {
                        "session" => format_session_usage(result),
                        _ => format_usage_stats(result),
                    };
                    // /usage is a user-requested report: never fold.
                    self.log.push(LogLine::unfolded(LogKind::System, &text));
                    self.status = "usage".to_string();
                }
            }
            Some(ControlReq::RewindPreview(target)) => self.on_rewind_preview(target, result),
            Some(ControlReq::RewindApplyFiles {
                target,
                then_conversation,
            }) => self.on_apply_file_rewind(target, then_conversation, result),
            Some(ControlReq::RewindConversation(target)) => {
                self.on_conversation_rewind(target, result)
            }
            Some(ControlReq::V4RewindApply(target)) => self.on_v4_rewind_apply(target, result),
            _ => {}
        }
    }

    fn v4_ack_failure(ack: &zcode_tui::V4CommandAck) -> String {
        ack.message
            .clone()
            .or_else(|| ack.reason_code.clone())
            .unwrap_or_else(|| ack.status.clone())
    }

    fn on_v4_set_guide_ok(
        &mut self,
        content: String,
        expected_command_id: String,
        result: Option<&serde_json::Value>,
    ) {
        let Some(ack) = result.and_then(parse_v4_command_ack) else {
            self.push_error(
                "V4 steer: unrecognized setFollowupMode acknowledgement (input queued)",
            );
            self.queued.push_back(content);
            return;
        };
        if !ack.accepted() {
            let why = Self::v4_ack_failure(&ack);
            self.push_error(&format!("V4 steer mode rejected: {why} (input queued)"));
            self.log_debug(&format!(
                "v4 steer command={expected_command_id} status={}",
                ack.status
            ));
            self.queued.push_back(content);
            return;
        }
        let Some(session_id) = self.app_session.clone() else {
            self.queued.push_back(content);
            return;
        };
        let command_id = self.next_v4_command_id("steer-text");
        let params = v4_command_params(
            &command_id,
            &self.v4_client_id,
            &session_id,
            "sendText",
            serde_json::json!({ "text": content }),
            V4CommandBase::None,
            unix_time_ms(),
        );
        self.log_debug(&format!(
            "v4 steer guide accepted command={expected_command_id}; sending {command_id}"
        ));
        self.send_control(
            "v4/command",
            params,
            ControlReq::V4SteerText {
                content,
                command_id,
            },
        );
    }

    fn on_v4_steer_text_ok(
        &mut self,
        content: String,
        expected_command_id: String,
        result: Option<&serde_json::Value>,
    ) {
        let Some(ack) = result.and_then(parse_v4_command_ack) else {
            self.push_error("V4 steer: unrecognized sendText acknowledgement (input queued)");
            self.queued.push_back(content);
            return;
        };
        if !ack.accepted() {
            let why = Self::v4_ack_failure(&ack);
            self.push_error(&format!("V4 steer rejected: {why} (input queued)"));
            self.log_debug(&format!(
                "v4 steer command={expected_command_id} status={}",
                ack.status
            ));
            self.queued.push_back(content);
            return;
        }
        let delivery = ack.input_delivery().map(str::to_string).or_else(|| {
            self.v4_state
                .delivery_for(&expected_command_id)
                .map(str::to_string)
        });
        if let Some(delivery) = delivery {
            self.settle_v4_steer(&expected_command_id, &content, &delivery);
        } else {
            self.pending_v4_steers
                .insert(expected_command_id.clone(), content);
            self.status = "steer accepted; awaiting V4 delivery".to_string();
            self.log_debug(&format!(
                "v4 steer command={expected_command_id} accepted awaiting-delivery"
            ));
        }
    }

    /// /usage [7d|30d] — session token breakdown + period aggregate, both
    /// fetched fire-and-forget and rendered as they arrive.
    fn show_usage(&mut self, range: Option<&str>) {
        let Some(session_id) = self.app_session.clone() else {
            self.push_system(
                "/usage needs an active app-server session (send a prompt first; \
                 classic --prompt path shows ctx in the footer instead)",
            );
            return;
        };
        let range = match range {
            None | Some("7d") => "7d",
            Some("30d") => "30d",
            Some(other) => {
                self.push_error(&format!("unknown range: {other} (use 7d or 30d)"));
                return;
            }
        };
        self.send_control(
            "session/usage",
            app_usage_params(&session_id),
            ControlReq::Usage("session"),
        );
        self.send_control(
            "usage/stats",
            usage_stats_params(range),
            ControlReq::Usage("stats"),
        );
        self.status = "fetching usage…".to_string();
    }

    fn pump_job(&mut self) {
        loop {
            let (event, kind) = {
                let Some(active) = &mut self.job else { return };
                (active.job.receiver.try_recv(), active.kind)
            };
            match event {
                Ok(JobEvent::Line { text, stderr }) => {
                    if kind == LogKind::Assistant {
                        // --json prints one end-of-run summary object on
                        // stdout; buffer and parse at finalize (fallback
                        // replays these lines). stderr goes to its own buffer
                        // so an interleaved warning can't corrupt the parse.
                        if let Some(active) = &mut self.job {
                            if stderr {
                                active.errs.push(text);
                            } else {
                                active.raw.push(text);
                            }
                            active.any_output = true;
                        }
                    } else {
                        // Shell/diff jobs keep the merged live view.
                        self.append_job_text(&text);
                    }
                }
                Ok(JobEvent::Eof) => {
                    if let Some(active) = &mut self.job {
                        active.eofs += 1;
                    }
                }
                Ok(JobEvent::Finished { success, detail }) => {
                    if let Some(active) = &mut self.job {
                        active.finished = Some((success, detail));
                        active.finished_at = Some(Instant::now());
                    }
                }
                Err(TryRecvError::Empty) => {
                    let Some(active) = &self.job else { return };
                    // Finalize once the child exited and both streams hit EOF
                    // (nothing left to lose). The timeout only covers a
                    // grandchild that inherited the pipes and keeps them open.
                    let drained = active.drained();
                    let stuck = active
                        .finished_at
                        .is_some_and(|at| at.elapsed() > Duration::from_millis(1500));
                    if drained || stuck {
                        self.finalize_job();
                    }
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    self.finalize_job();
                    return;
                }
            }
        }
    }

    fn append_job_text(&mut self, text: &str) {
        let last_index = self.log.len() - 1;
        let Some(active) = &mut self.job else { return };
        if active.log_index != last_index {
            self.log.push(LogLine::new(active.kind, ""));
            active.log_index = self.log.len() - 1;
            active.entry_started = false;
        }
        let entry = &mut self.log[active.log_index];
        if active.entry_started {
            entry.text.push('\n');
        }
        entry.text.push_str(text);
        active.entry_started = true;
        active.any_output = true;
    }

    /// Keys while the kernel awaits an interaction answer: ↑↓ select an
    /// option, Enter answers, Esc declines.
    fn handle_interaction_key(&mut self, key: KeyEvent) -> Option<UiEffect> {
        match key.code {
            KeyCode::Up => {
                if let Some(pending) = &mut self.interaction {
                    pending.selected = pending.selected.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(pending) = &mut self.interaction {
                    let max = pending
                        .request
                        .questions
                        .first()
                        .map(|q| q.options.len().saturating_sub(1))
                        .unwrap_or(0);
                    pending.selected = (pending.selected + 1).min(max);
                }
            }
            KeyCode::Enter => self.answer_interaction(),
            KeyCode::Esc => self.decline_interaction(),
            // Ctrl+C must never be swallowed by an overlay: treat as decline
            // (it cancels the turn, same as the non-overlay cancel path).
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.decline_interaction()
            }
            _ => {}
        }
        None
    }

    /// Answer the pending interaction with the option at `selected`.
    /// Replying stops the kernel's re-send loop; a permission approval lets
    /// the gated tool run within the SAME turn, a plan approval ends the
    /// turn (continuation handled below).
    fn answer_interaction_with(&mut self, selected: usize) {
        let Some(pending) = self.interaction.take() else {
            return;
        };
        let request = pending.request;
        let Some(line) = encode_interaction_reply(&pending.envelope_id, &request, selected) else {
            // Malformed selection/payload: leave open (kernel keeps retrying).
            self.interaction = Some(PendingInteraction {
                request,
                envelope_id: pending.envelope_id,
                selected: 0,
            });
            return;
        };
        let sent = self
            .app_conn
            .as_mut()
            .map(|conn| conn.reply(&line))
            .unwrap_or(Err(AppServerUnavailable::Disconnected));
        match sent {
            Ok(()) => {
                self.interaction_done.insert(request.request_id.clone());
                let value = request
                    .questions
                    .first()
                    .and_then(|q| q.options.get(selected))
                    .map(|o| o.value.clone())
                    .unwrap_or_default();
                self.push_system(&format!("{}: {}", request.tool_name, value));
                self.status = format!("answered ({value})");
                // plan_approval + approve: the kernel neither flips the mode
                // nor continues on its own (pinned 2026-07-07: mode stays
                // plan, a follow-up Write still doesn't land) — so do the
                // Claude-Code dance ourselves: switch to build and queue a
                // continuation prompt for when this turn finalizes.
                if request.interaction == "plan_approval" && value == "approve" {
                    if let Some(session_id) = self.app_session.clone() {
                        self.config.mode = Some("build".to_string());
                        self.refresh_banner();
                        self.send_control(
                            "session/setMode",
                            app_set_mode_params(&session_id, "build"),
                            ControlReq::Command("/mode"),
                        );
                    }
                    self.queued
                        .push_back("Proceed with the approved plan.".to_string());
                }
            }
            Err(reason) => self.push_error(&format!("interaction reply failed: {reason}")),
        }
    }

    fn answer_interaction(&mut self) {
        if let Some(pending) = &self.interaction {
            self.answer_interaction_with(pending.selected);
        }
    }

    /// Esc on the approval overlay. Permission requests carry a protocol-
    /// level `deny` option — answer it and let the turn continue (the model
    /// reacts to the denial). Plan approvals offer no decline option, so
    /// declining stops the turn (session/stop + drain).
    fn decline_interaction(&mut self) {
        if let Some(deny) = self.interaction.as_ref().and_then(|p| p.request.deny_index) {
            self.answer_interaction_with(deny);
            return;
        }
        let Some(pending) = self.interaction.take() else {
            return;
        };
        self.interaction_done
            .insert(pending.request.request_id.clone());
        self.push_system(&format!("{}: declined", pending.request.tool_name));
        self.cancel_current();
    }

    fn handle_model_picker_key(&mut self, key: KeyEvent) -> Option<UiEffect> {
        match key.code {
            KeyCode::Esc => {
                self.model_picker = None;
                self.status = "model picker closed".to_string();
            }
            KeyCode::Up => {
                if let Some(index) = &mut self.model_picker {
                    *index = index.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(index) = &mut self.model_picker {
                    *index = (*index + 1).min(self.controls.models.len().saturating_sub(1));
                }
            }
            KeyCode::Enter => self.pick_model(),
            _ => {}
        }
        None
    }

    /// /model — list models reported by the ZCode app-server.
    fn open_model_picker(&mut self) {
        if self.controls.models.is_empty() {
            self.push_system("model catalog is not available from the ZCode app-server yet");
            return;
        }
        let current =
            self.controls
                .model_current
                .as_deref()
                .and_then(|id| {
                    self.controls.models.iter().position(|m| {
                        m.reference.get("modelId").and_then(|v| v.as_str()) == Some(id)
                    })
                })
                .unwrap_or(0);
        self.model_picker = Some(current);
    }

    fn pick_model(&mut self) {
        let Some(index) = self.model_picker.take() else {
            return;
        };
        if let Some(choice) = self.controls.models.get(index).cloned() {
            self.controls.model_current = choice
                .reference
                .get("modelId")
                .and_then(|value| value.as_str())
                .map(str::to_string);
            match self.app_session.clone() {
                Some(session_id) => {
                    self.push_system(&format!("model → {} ({})", choice.label, choice.provider));
                    self.send_control(
                        "session/setModel",
                        app_set_model_params(&session_id, &choice.reference),
                        ControlReq::Command("/model"),
                    );
                }
                None => {
                    self.push_system(&format!(
                        "model → {} ({}) for the next session",
                        choice.label, choice.provider
                    ));
                    self.pending_model = Some(choice);
                }
            }
        }
    }

    /// /think — cycle to the next kernel-reported thought level.
    fn toggle_thought(&mut self) {
        let Some(session_id) = self.app_session.clone() else {
            self.push_system(
                "/think needs an active app-server session \
                 (complete one streaming prompt first; ZCODE_TUI_APP_SERVER=0 disables it)",
            );
            return;
        };
        if self.controls.thought_levels.is_empty() {
            self.push_system("no thought levels reported by the kernel yet");
            return;
        }
        let levels = &self.controls.thought_levels;
        let next = self
            .controls
            .thought_current
            .as_deref()
            .and_then(|current| levels.iter().position(|level| level == current))
            .map(|index| levels[(index + 1) % levels.len()].clone())
            .unwrap_or_else(|| levels[0].clone());
        self.push_system(&format!("thought level → {next}"));
        self.send_control(
            "session/setThoughtLevel",
            app_set_thought_params(&session_id, &next),
            ControlReq::Command("/think"),
        );
    }

    /// /compact — compact the live session's context in place (keeps the
    /// session, unlike /new). Completion shows as a watermark refresh. With
    /// no app-server session it forwards to the kernel CLI as before.
    fn compact_session(&mut self) {
        let Some(session_id) = self.app_session.clone() else {
            self.start_prompt_job("/compact");
            return;
        };
        self.push_system("compacting session context…");
        self.status = "compacting (app-server)".to_string();
        self.send_control(
            "session/compact",
            app_compact_params(&session_id),
            ControlReq::Command("/compact"),
        );
    }

    /// Steer the RUNNING app-server turn with fresh input (Codex-style: just
    /// type while it streams). ZCode 3.5.3 uses V4 guide delivery; older
    /// kernels retain session/steer. V4 UI is not optimistic: the user entry
    /// lands only after a semantic guide/queue delivery frame.
    fn steer_turn(&mut self, content: &str) {
        let Some(session_id) = self.app_session.clone() else {
            self.queued.push_back(content.to_string());
            return;
        };
        if self.v4_mode == V4Mode::Available {
            let Some(revision) = self.v4_state.revision else {
                self.push_error("V4 steer state is not ready yet (input queued)");
                self.queued.push_back(content.to_string());
                return;
            };
            if self.v4_state.set_followup_allowed == Some(false) {
                self.push_error("V4 guide mode is unavailable right now (input queued)");
                self.queued.push_back(content.to_string());
                return;
            }
            let command_id = self.next_v4_command_id("guide");
            let params = v4_command_params(
                &command_id,
                &self.v4_client_id,
                &session_id,
                "setFollowupMode",
                serde_json::json!({ "mode": "guide" }),
                V4CommandBase::Revision(revision),
                unix_time_ms(),
            );
            self.status = "steering via V4 guide…".to_string();
            self.send_control(
                "v4/command",
                params,
                ControlReq::V4SetGuide {
                    content: content.to_string(),
                    command_id,
                },
            );
            return;
        }
        if self.v4_mode == V4Mode::Unknown {
            self.push_error("V4 capability negotiation is incomplete (input queued)");
            self.queued.push_back(content.to_string());
            return;
        }
        self.push_user(content);
        self.push_system("↪ steering the running turn");
        self.status = "steering (app-server)".to_string();
        self.send_control(
            "session/steer",
            app_steer_params(&session_id, content),
            ControlReq::Steer(content.to_string()),
        );
    }

    /// /rewind — pick a captured checkpoint (or latestCheckpoint), preview
    /// the file restore, choose a scope, apply. App-server path only; idle
    /// only (a rewind mid-turn would race the streaming turn's events).
    fn open_rewind_picker(&mut self) {
        if self.app_session.is_none() {
            self.push_system(
                "/rewind needs an active app-server session \
                 (complete one streaming prompt first; ZCODE_TUI_APP_SERVER=0 disables it)",
            );
            return;
        }
        if self.app_turn.is_some() || self.app_connect.is_some() || self.app_draining.is_some() {
            self.push_system("/rewind: wait for the running turn to finish (Esc cancels it)");
            return;
        }
        let targets: Vec<(String, RewindTarget)> = match self.v4_mode {
            V4Mode::Available => {
                let rows = self.v4_state.rewind_rows();
                if rows.is_empty() {
                    self.push_system(
                        "no V4 file-rewind targets in this session yet \
                         (a completed tool-writing turn creates one)",
                    );
                    return;
                }
                rows.into_iter()
                    .map(|row| {
                        (
                            format!(
                                "turn row {} · {} file(s) · +{} -{} · restores BEFORE this turn",
                                row.row_id, row.files, row.additions, row.deletions
                            ),
                            RewindTarget::V4Row {
                                row_id: row.row_id,
                                entity_id: row.entity_id.clone(),
                            },
                        )
                    })
                    .collect()
            }
            V4Mode::Unavailable => {
                if self.checkpoints.is_empty() {
                    self.push_system(
                        "no checkpoints in this session yet \
                         (approved tool writes create them; nothing to rewind to)",
                    );
                    return;
                }
                let mut targets: Vec<(String, RewindTarget)> = vec![(
                    "latest checkpoint (undo the most recent write)".to_string(),
                    RewindTarget::LatestCheckpoint,
                )];
                for (index, entry) in self.checkpoints.iter().enumerate().rev() {
                    targets.push((
                        format!(
                            "checkpoint {} · #{} · {} file(s) · restores the state BEFORE this write",
                            checkpoint_short_id(&entry.id),
                            index + 1,
                            entry.files,
                        ),
                        RewindTarget::Checkpoint(entry.id.clone()),
                    ));
                }
                targets
            }
            V4Mode::Unknown => {
                self.push_system("/rewind: V4 capability state is not ready yet");
                return;
            }
        };
        self.log_debug(&format!(
            "rewind: picker opened ({} targets)",
            targets.len()
        ));
        self.rewind = Some(RewindOverlay {
            targets,
            selected: 0,
            preview: None,
            scope: 0,
            busy: false,
        });
        self.status = "rewind: pick a target".to_string();
    }

    fn handle_rewind_key(&mut self, key: KeyEvent) -> Option<UiEffect> {
        let previewing = self
            .rewind
            .as_ref()
            .is_some_and(|overlay| overlay.preview.is_some());
        let v4_preview = self
            .rewind
            .as_ref()
            .and_then(|overlay| overlay.preview.as_ref())
            .is_some_and(|(target, _)| target.is_v4());
        match key.code {
            KeyCode::Esc if previewing => {
                // Back to the target list.
                if let Some(overlay) = &mut self.rewind {
                    overlay.preview = None;
                    overlay.busy = false;
                }
                self.status = "rewind: pick a target".to_string();
            }
            KeyCode::Esc => {
                self.rewind = None;
                self.status = "rewind closed".to_string();
            }
            // Preview stage: ←/→ (and ↑/↓) cycle the scope.
            KeyCode::Left | KeyCode::Up if previewing && !v4_preview => {
                if let Some(overlay) = &mut self.rewind {
                    overlay.scope = (overlay.scope + REWIND_SCOPES.len() - 1) % REWIND_SCOPES.len();
                }
            }
            KeyCode::Right | KeyCode::Down if previewing && !v4_preview => {
                if let Some(overlay) = &mut self.rewind {
                    overlay.scope = (overlay.scope + 1) % REWIND_SCOPES.len();
                }
            }
            KeyCode::Up => {
                if let Some(overlay) = &mut self.rewind {
                    overlay.selected = overlay.selected.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(overlay) = &mut self.rewind {
                    overlay.selected =
                        (overlay.selected + 1).min(overlay.targets.len().saturating_sub(1));
                }
            }
            KeyCode::Enter if previewing => self.apply_rewind(),
            KeyCode::Enter => self.request_rewind_preview(),
            _ => {}
        }
        None
    }

    /// Stage 1 Enter: ask the kernel what the selected target would restore.
    fn request_rewind_preview(&mut self) {
        let Some(session_id) = self.app_session.clone() else {
            return;
        };
        let Some(overlay) = &mut self.rewind else {
            return;
        };
        if overlay.busy {
            return; // a preview/apply is already in flight
        }
        let Some((_, target)) = overlay.targets.get(overlay.selected) else {
            return;
        };
        let target = target.clone();
        overlay.busy = true;
        self.log_debug(&format!("rewind: preview requested {}", target.label()));
        self.status = format!("rewind: previewing {}…", target.label());
        match &target {
            RewindTarget::V4Row { row_id, entity_id } => {
                let Some((revision, epoch)) = self.v4_cas_base() else {
                    if let Some(overlay) = &mut self.rewind {
                        overlay.busy = false;
                    }
                    self.push_error("rewind preview: V4 revision state is unavailable");
                    return;
                };
                self.send_control(
                    "v4/conversation/fileRewindPreview",
                    v4_file_rewind_preview_params(
                        &session_id,
                        *row_id,
                        entity_id,
                        revision,
                        &epoch,
                    ),
                    ControlReq::RewindPreview(target),
                );
            }
            _ => self.send_control(
                "session/previewFileRewind",
                app_file_rewind_params(&session_id, &target),
                ControlReq::RewindPreview(target),
            ),
        }
    }

    /// Stage 2 Enter: apply with the chosen scope. File scopes go through
    /// applyFileRewind (refuses unsafe files); the conversation scope uses
    /// session/rewind with a MESSAGE-kind target — both pinned live:
    /// session/rewind force-applies file restores over external
    /// modifications, and it coerces checkpoint-kind targets to a workspace
    /// rewind even under scope:"conversation" (2026-07-09: the file got
    /// deleted). Only `{kind:"message"}` targets honor the conversation scope.
    fn apply_rewind(&mut self) {
        let Some(session_id) = self.app_session.clone() else {
            return;
        };
        let (target, preview, scope) = {
            let Some(overlay) = &self.rewind else {
                return;
            };
            if overlay.busy {
                return;
            }
            let Some((target, preview)) = overlay.preview.clone() else {
                return;
            };
            (target, preview, REWIND_SCOPES[overlay.scope])
        };
        // Local gate mirroring applyFileRewind's own refusal: file scopes
        // cannot apply over externally-modified files (and we never force —
        // that would need the unchecked session/rewind).
        if scope != "conversation" && !preview.can_apply {
            let files = preview
                .unsafe_files
                .iter()
                .map(|f| format!("{} ({})", f.path, f.note))
                .collect::<Vec<_>>()
                .join(", ");
            self.push_error(&format!(
                "rewind blocked: unsafe file(s) changed outside the session: {files} — \
                 resolve them or use the conversation scope"
            ));
            self.log_debug("rewind: apply blocked (canApply=false)");
            return;
        }
        if let RewindTarget::V4Row { row_id, entity_id } = &target {
            let Some((revision, epoch)) = self.v4_cas_base() else {
                self.push_error("rewind apply: V4 revision state is unavailable");
                return;
            };
            if let Some(overlay) = &mut self.rewind {
                overlay.busy = true;
                overlay.scope = 0;
            }
            let command_id = self.next_v4_command_id("rewind");
            let params = v4_command_params(
                &command_id,
                &self.v4_client_id,
                &session_id,
                "applyFileRewind",
                serde_json::json!({ "target": v4_rewind_target(*row_id, entity_id) }),
                V4CommandBase::RevisionAndEpoch {
                    revision,
                    log_epoch: &epoch,
                },
                unix_time_ms(),
            );
            self.log_debug(&format!(
                "rewind: V4 apply {} revision={revision}",
                target.label()
            ));
            self.status = "rewinding files (V4)…".to_string();
            self.send_control("v4/command", params, ControlReq::V4RewindApply(target));
            return;
        }
        // Conversation legs need the checkpoint's targetMessageId; without it
        // the only wire form left would be the checkpoint target, which the
        // kernel turns into a forced file rewind — refuse instead.
        let conversation = if scope == "conversation" || scope == "both" {
            match conversation_target(&target, &self.checkpoints) {
                Some(message_target) => Some(message_target),
                None => {
                    self.push_error(
                        "conversation rewind unavailable for this target \
                         (no message id captured) — workspace scope still works",
                    );
                    self.log_debug("rewind: conversation leg refused (no message id)");
                    return;
                }
            }
        } else {
            None
        };
        if let Some(overlay) = &mut self.rewind {
            overlay.busy = true;
        }
        self.rewind_trigger = None;
        self.log_debug(&format!("rewind: apply {} scope={scope}", target.label()));
        self.status = format!("rewinding ({scope})…");
        match scope {
            "conversation" => {
                let wire = conversation.expect("checked above");
                self.send_control(
                    "session/rewind",
                    app_rewind_params(&session_id, &wire, "conversation"),
                    ControlReq::RewindConversation(target),
                );
            }
            both_or_workspace => self.send_control(
                "session/applyFileRewind",
                app_file_rewind_params(&session_id, &target),
                ControlReq::RewindApplyFiles {
                    target,
                    then_conversation: if both_or_workspace == "both" {
                        conversation
                    } else {
                        None
                    },
                },
            ),
        }
    }

    /// previewFileRewind result → stage 2. Dropped unless an overlay is open
    /// AND awaiting (`busy`) — a stale response from a closed-and-reopened
    /// picker must not surface a preview the user never asked for.
    fn on_rewind_preview(&mut self, target: RewindTarget, result: Option<&serde_json::Value>) {
        if !self.rewind.as_ref().is_some_and(|overlay| overlay.busy) {
            return;
        }
        let preview = result.and_then(parse_rewind_preview);
        let Some(preview) = preview else {
            if let Some(overlay) = &mut self.rewind {
                overlay.busy = false;
            }
            self.push_error("rewind preview: unrecognized result shape");
            return;
        };
        self.log_debug(&format!(
            "rewind: preview canApply={} safe={} unsafe={}",
            preview.can_apply,
            preview.safe.len(),
            preview.unsafe_files.len()
        ));
        self.status = if preview.can_apply {
            if target.is_v4() {
                "rewind: Enter applies workspace · Esc back".to_string()
            } else {
                "rewind: Enter applies · ←/→ scope · Esc back".to_string()
            }
        } else if target.is_v4() {
            "rewind: unsafe files — V4 apply blocked".to_string()
        } else {
            "rewind: unsafe files — conversation scope only".to_string()
        };
        if let Some(overlay) = &mut self.rewind {
            overlay.busy = false;
            overlay.scope = 0;
            overlay.preview = Some((target, preview));
        }
    }

    /// applyFileRewind result: `applied` is authoritative (a refusal is a
    /// success envelope with applied:false + the unsafe files).
    fn on_apply_file_rewind(
        &mut self,
        target: RewindTarget,
        then_conversation: Option<RewindTarget>,
        result: Option<&serde_json::Value>,
    ) {
        let outcome = result.map(parse_apply_file_rewind);
        let Some(outcome) = outcome else {
            self.push_error("rewind: applyFileRewind returned no result");
            self.close_rewind_overlay();
            return;
        };
        if !outcome.applied {
            let files = outcome
                .unsafe_files
                .iter()
                .map(|f| format!("{} ({})", f.path, f.note))
                .collect::<Vec<_>>()
                .join(", ");
            let detail = if files.is_empty() {
                outcome.response.clone()
            } else {
                format!("{} — {files}", outcome.response)
            };
            self.push_error(&format!("rewind not applied: {detail}"));
            self.log_debug("rewind: applyFileRewind refused");
            self.close_rewind_overlay();
            return;
        }
        self.push_system(&format!("⟲ {}", outcome.response));
        self.log_debug(&format!("rewind: files restored ({})", target.label()));
        self.status = "rewound (files)".to_string();
        if let Some(wire) = then_conversation {
            // scope "both": chain the conversation rewind (message-kind
            // target — checkpoint kinds would force ANOTHER file rewind)
            // now that the file restore landed safely.
            if let Some(session_id) = self.app_session.clone() {
                self.rewind_trigger = None;
                self.send_control(
                    "session/rewind",
                    app_rewind_params(&session_id, &wire, "conversation"),
                    ControlReq::RewindConversation(target),
                );
                return; // overlay closes when the conversation leg reports
            }
        }
        self.close_rewind_overlay();
    }

    fn on_v4_rewind_apply(&mut self, target: RewindTarget, result: Option<&serde_json::Value>) {
        let Some(ack) = result.and_then(parse_v4_command_ack) else {
            self.push_error("rewind: unrecognized V4 apply acknowledgement");
            self.close_rewind_overlay();
            return;
        };
        if !ack.accepted() {
            let why = Self::v4_ack_failure(&ack);
            self.push_error(&format!("rewind not applied: {why}"));
            self.log_debug(&format!("rewind: V4 apply status={}", ack.status));
            self.close_rewind_overlay();
            return;
        }
        let nested = ack.result.as_ref().filter(|value| {
            value.get("type").and_then(|field| field.as_str()) == Some("applyFileRewind")
        });
        if nested.is_none() {
            self.push_error("rewind: V4 apply acknowledgement carried no result");
            self.close_rewind_overlay();
            return;
        }
        self.on_apply_file_rewind(target, None, nested);
    }

    /// session/rewind (conversation scope) result: judged by the preceding
    /// rewind.triggered event — the envelope is a success even when the
    /// kernel did nothing ("Checkpoint … was not found.").
    fn on_conversation_rewind(&mut self, target: RewindTarget, result: Option<&serde_json::Value>) {
        let response = result
            .and_then(|r| r.get("response"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let trigger = self.rewind_trigger.take();
        let (strategy, reason) = match &trigger {
            Some((strategy, reason)) => (Some(strategy.as_str()), Some(reason.as_str())),
            None => (None, None),
        };
        match rewind_failure(strategy, reason, &response) {
            Some(failure) => {
                self.push_error(&format!("rewind failed: {failure}"));
                self.log_debug(&format!("rewind: conversation leg failed ({failure})"));
            }
            None => {
                self.push_system(&format!("⟲ {response}"));
                self.push_system(
                    "conversation rewound — messages after the target are gone kernel-side \
                     (transcript above is history, not context)",
                );
                self.prune_checkpoints(&target);
                self.log_debug(&format!(
                    "rewind: conversation rewound ({})",
                    target.label()
                ));
                self.status = "rewound (conversation)".to_string();
            }
        }
        self.close_rewind_overlay();
    }

    /// After a conversation rewind the checkpoints at/after the target no
    /// longer exist on the kernel's active chain — drop them locally too.
    fn prune_checkpoints(&mut self, target: &RewindTarget) {
        match target {
            RewindTarget::Checkpoint(id) => {
                if let Some(pos) = self.checkpoints.iter().position(|c| &c.id == id) {
                    self.checkpoints.truncate(pos);
                }
            }
            RewindTarget::LatestCheckpoint => {
                self.checkpoints.pop();
            }
            RewindTarget::Message(_) | RewindTarget::Turn(_) => self.checkpoints.clear(),
            RewindTarget::V4Row { .. } => {}
        }
    }

    fn close_rewind_overlay(&mut self) {
        self.rewind = None;
    }

    /// The session is gone (new/resume/downgrade/cancel): its checkpoints are
    /// not valid targets anymore, and any open /rewind flow is moot.
    fn reset_rewind_state(&mut self) {
        self.checkpoints.clear();
        self.rewind = None;
        self.rewind_trigger = None;
        self.v4_mode = V4Mode::Unknown;
        self.v4_state = V4ConversationState::default();
        self.pending_v4_steers.clear();
        self.agents.reset();
    }

    fn open_background_tasks(&mut self) {
        if !self.agents.open() {
            self.push_system("no background tasks observed in this session");
            return;
        }
        self.show_palette = false;
        self.show_help = false;
        self.status = "background tasks: ↑↓ select · Esc close (read-only)".to_string();
    }

    fn handle_background_task_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.agents.close();
                self.status = "background task list closed".to_string();
            }
            KeyCode::Up => self.agents.select_previous(),
            KeyCode::Down => self.agents.select_next(),
            KeyCode::Home => self.agents.select_first(),
            KeyCode::End => self.agents.select_last(),
            _ => {}
        }
    }

    fn handle_session_picker_key(&mut self, key: KeyEvent) -> Option<UiEffect> {
        match key.code {
            KeyCode::Esc => {
                self.session_picker = None;
                self.status = "session picker closed".to_string();
            }
            KeyCode::Up => {
                if let Some((_, index)) = &mut self.session_picker {
                    *index = index.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some((rows, index)) = &mut self.session_picker {
                    *index = (*index + 1).min(rows.len().saturating_sub(1));
                }
            }
            KeyCode::Enter => {
                if let Some((rows, index)) = self.session_picker.take() {
                    if let Some(row) = rows.get(index) {
                        let id = row.id.clone();
                        self.set_resume(Some(&id));
                    }
                }
            }
            _ => {}
        }
        None
    }

    fn handle_history_search_key(&mut self, key: KeyEvent) -> Option<UiEffect> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.history_query = None;
                self.status = "search cancelled".to_string();
            }
            KeyCode::Enter => {
                if let Some((query, index)) = self.history_query.take() {
                    let matches = history_search(&self.history, &query, HISTORY_SEARCH_LIMIT);
                    match matches.get(index) {
                        Some(entry) => {
                            let entry = entry.clone();
                            self.set_input(&entry);
                            self.status = "recalled from history".to_string();
                        }
                        None => self.status = "no history match".to_string(),
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some((query, index)) = &mut self.history_query {
                    query.pop();
                    *index = 0;
                }
            }
            KeyCode::Up => {
                if let Some((query, index)) = &mut self.history_query {
                    let count = history_search(&self.history, query, HISTORY_SEARCH_LIMIT).len();
                    *index = (*index + 1).min(count.saturating_sub(1));
                }
            }
            KeyCode::Down => {
                if let Some((_, index)) = &mut self.history_query {
                    *index = index.saturating_sub(1);
                }
            }
            KeyCode::Char(ch) if !ctrl => {
                if let Some((query, index)) = &mut self.history_query {
                    query.push(ch);
                    *index = 0;
                }
            }
            _ => {}
        }
        None
    }

    fn open_session_picker(&mut self) {
        // A live app-server connection is the authoritative source
        // (session/list, verified live); the kernel db stays the fallback for
        // the classic path or when the protocol call fails.
        if self.app_conn.as_ref().is_some_and(AppServerConn::is_alive) && !self.is_busy() {
            let cwd = self.app_workspace();
            let listed = self
                .app_conn
                .as_mut()
                .expect("alive checked above")
                .request_blocking(
                    "session/list",
                    serde_json::json!({}),
                    Duration::from_secs(3),
                );
            if let Ok(result) = listed {
                let rows = parse_session_list(&result, &cwd);
                if !rows.is_empty() {
                    self.session_picker = Some((rows, 0));
                    self.show_palette = false;
                    self.show_help = false;
                    self.status =
                        "pick a session: ↑↓ select · Enter resume · Esc close".to_string();
                    return;
                }
            }
            // Fall through to the db source on error/empty.
        }
        let DbState::Enabled(path) = &self.db_state else {
            self.push_system(
                "session list unavailable: kernel db not readable (missing or schema changed)",
            );
            return;
        };
        let Ok(conn) = open_kernel_db_ro(path) else {
            self.push_system("session list unavailable: kernel db busy, try again");
            return;
        };
        let directory = self
            .resolve_cwd()
            .canonicalize()
            .unwrap_or_else(|_| self.resolve_cwd())
            .to_string_lossy()
            .into_owned();
        match list_recent_sessions(&conn, &directory, 20) {
            Ok(rows) if rows.is_empty() => self.push_system("no sessions recorded yet"),
            Ok(rows) => {
                self.session_picker = Some((rows, 0));
                self.show_palette = false;
                self.show_help = false;
                self.status = "pick a session: ↑↓ select · Enter resume · Esc close".to_string();
            }
            Err(error) => self.push_system(&format!("session list unavailable: {error:#}")),
        }
    }

    /// Toggle the most recent foldable over-threshold cell between the
    /// folded preview and the full text.
    fn toggle_fold(&mut self) {
        if self.expanded_log.take().is_some() {
            self.status = "expanded output closed".to_string();
            return;
        }
        let target = self
            .log
            .iter()
            .enumerate()
            .rev()
            .find(|(_, entry)| {
                foldable_kind(entry.kind)
                    && !entry.no_fold
                    && fold_preview(&entry.text, FOLD_THRESHOLD, FOLD_HEAD).is_some()
            })
            .map(|(index, _)| index);
        match target {
            Some(index) if index < self.flushed_log => {
                self.expanded_log = Some((index, 0));
                self.status = "expanded output: ↑↓/PageUp/PageDown scroll · Esc closes".to_string();
            }
            Some(index) => {
                let expanded = if self.unfolded.remove(&index) {
                    false
                } else {
                    self.unfolded.insert(index);
                    true
                };
                self.status = if expanded {
                    "expanded (Ctrl+O folds back)".to_string()
                } else {
                    "folded".to_string()
                };
            }
            None => self.status = "no long output to fold".to_string(),
        }
    }

    /// Navigate a completed transcript entry without taking ownership of
    /// terminal mouse selection or clipboard shortcuts.
    fn handle_expanded_log_key(&mut self, key: KeyEvent) {
        let Some((_, scroll)) = self.expanded_log.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.expanded_log = None;
                self.status = "expanded output closed".to_string();
            }
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.expanded_log = None;
                self.status = "expanded output closed".to_string();
            }
            KeyCode::Up => *scroll = scroll.saturating_sub(1),
            KeyCode::Down => *scroll = scroll.saturating_add(1),
            KeyCode::PageUp => *scroll = scroll.saturating_sub(8),
            KeyCode::PageDown => *scroll = scroll.saturating_add(8),
            KeyCode::Home => *scroll = 0,
            KeyCode::End => *scroll = u16::MAX,
            _ => {}
        }
    }

    /// Fallback when stdout isn't the --json summary (older kernel or plain
    /// output): replay the buffered lines through the streamed-event
    /// interpretation the pump used to apply live.
    fn render_assistant_fallback(&mut self, log_index: usize, raw: &str) {
        let mut text = String::new();
        let mut sides: Vec<(LogKind, String)> = Vec::new();
        let append = |acc: &mut String, piece: &str| {
            if !acc.is_empty() {
                acc.push('\n');
            }
            acc.push_str(piece);
        };
        for line in raw.lines() {
            match parse_stream_event(line) {
                Some(StreamEvent::Text(t)) => append(&mut text, &t),
                Some(StreamEvent::ToolUse { name, detail }) => {
                    sides.push((LogKind::Tool, format!("⚙ {name} {detail}")));
                }
                Some(StreamEvent::ToolResult { detail }) => {
                    sides.push((LogKind::Tool, format!("↳ {detail}")));
                }
                Some(StreamEvent::Meta(meta)) => {
                    sides.push((LogKind::System, format!("· {meta}")));
                }
                None => append(&mut text, line),
            }
        }
        self.log[log_index].text = text;
        for (kind, line) in sides {
            self.log.push(LogLine::new(kind, &line));
        }
    }

    fn finalize_job(&mut self) {
        let Some(active) = self.job.take() else {
            return;
        };
        if active.kind == LogKind::Assistant && !active.raw.is_empty() {
            let raw = active.raw.join("\n");
            match parse_prompt_summary(&raw) {
                Some(summary) => {
                    let response = summary.response.trim_end();
                    self.log[active.log_index].text = if response.is_empty() {
                        "(no output)".to_string()
                    } else {
                        response.to_string()
                    };
                    if let (Some(used), Some(window)) =
                        (summary.context_used, summary.context_window)
                    {
                        self.context_watermark = Some((used, window));
                    }
                }
                None => self.render_assistant_fallback(active.log_index, &raw),
            }
        }
        // Kernel stderr (warnings etc.) surfaces as its own dim note instead
        // of polluting the summary parse; skip it on cancel (kill noise).
        if active.kind == LogKind::Assistant && !active.errs.is_empty() && !active.cancel_requested
        {
            let note = active.errs.join("\n");
            self.log.push(LogLine::new(LogKind::System, &note));
        }
        if !active.any_output {
            self.log[active.log_index].text = match active.kind {
                LogKind::Diff => "working tree clean".to_string(),
                _ => "(no output)".to_string(),
            };
        } else if self.log[active.log_index].text.is_empty() {
            self.log.remove(active.log_index);
            // Fold state is keyed by log index; shift entries past the hole.
            self.unfolded = self
                .unfolded
                .iter()
                .map(|&index| {
                    if index > active.log_index {
                        index - 1
                    } else {
                        index
                    }
                })
                .collect();
            if let Some((index, _)) = &mut self.expanded_log {
                if *index > active.log_index {
                    *index -= 1;
                } else if *index == active.log_index {
                    self.expanded_log = None;
                }
            }
        }
        let elapsed = active.started.elapsed().as_secs_f32();
        let (success, detail) = active
            .finished
            .unwrap_or((false, "job ended unexpectedly".to_string()));
        // Classic-path prompt jobs get the same >30s completion bell as
        // streaming turns (shell/diff jobs stay silent).
        if active.kind == LogKind::Assistant {
            self.ring_bell_if_long(elapsed, active.cancel_requested);
        }
        if active.cancel_requested {
            self.status = "cancelled".to_string();
            self.push_system(&format!("{} cancelled", active.label));
        } else if success {
            self.status = format!("done ({elapsed:.1}s)");
            // A prompt landed in a kernel session: keep the conversation
            // going by resuming that session on subsequent prompts.
            if active.kind == LogKind::Assistant && !self.session_active {
                self.session_active = true;
                if self.config.resume.is_none() {
                    self.config.continue_session = true;
                }
                self.refresh_banner();
            }
        } else {
            self.status = "error".to_string();
            self.push_error(&format!("{} failed: {detail}", active.label));
        }
    }

    // ---- log helpers -----------------------------------------------------

    fn banner_text(&self) -> String {
        let home = env::var("HOME").ok();
        let cwd = shorten_home(&display_cwd(&self.config), home.as_deref());
        let version = match &self.kernel_version {
            Some(kernel) => format!("kernel {kernel} · tui {}", env!("CARGO_PKG_VERSION")),
            None => format!("tui {}", env!("CARGO_PKG_VERSION")),
        };
        let session = if let Some(id) = &self.config.resume {
            format!("resume {id}")
        } else if self.config.continue_session || self.session_active {
            "continuing latest".to_string()
        } else {
            "fresh".to_string()
        };
        format!(
            "Welcome to ZCODE! ({version})\nZhiPU terminal TUI   /help for shortcuts\n\ndirectory: {cwd}\nmode: {}   /mode to change\nsession: {session}   /new to reset\nauth: {}   /login to sign in",
            display_mode(&self.config),
            self.auth_label
        )
    }

    fn push_banner(&mut self) {
        let text = self.banner_text();
        self.log.push(LogLine::new(LogKind::Banner, &text));
    }

    fn refresh_banner(&mut self) {
        if let Some(pos) = self
            .log
            .iter()
            .enumerate()
            .skip(self.flushed_log)
            .find_map(|(index, entry)| matches!(entry.kind, LogKind::Banner).then_some(index))
        {
            self.log[pos].text = self.banner_text();
        }
    }

    // ---- session state ---------------------------------------------------

    fn set_mode(&mut self, mode: Option<&str>) {
        match mode {
            None => self.push_system(&format!(
                "mode: {} (available: {})",
                display_mode(&self.config),
                PERMISSION_MODES.join(", ")
            )),
            Some(mode) if PERMISSION_MODES.contains(&mode) => {
                self.config.mode = Some(mode.to_string());
                self.refresh_banner();
                // A live app-server session takes the mode immediately
                // (session/setMode); otherwise it applies on the next spawn.
                if let Some(session_id) = self.app_session.clone() {
                    self.send_control(
                        "session/setMode",
                        app_set_mode_params(&session_id, mode),
                        ControlReq::Command("/mode"),
                    );
                    self.push_system(&format!("mode set to {mode} (applied to live session)"));
                } else {
                    self.push_system(&format!("mode set to {mode}"));
                }
                self.status = format!("mode {mode}");
            }
            Some(other) => self.push_error(&format!(
                "unknown mode: {other} (use {})",
                PERMISSION_MODES.join(", ")
            )),
        }
    }

    fn cycle_mode(&mut self) {
        let current = display_mode(&self.config);
        let next = PERMISSION_MODES
            .iter()
            .position(|mode| *mode == current)
            .map(|index| PERMISSION_MODES[(index + 1) % PERMISSION_MODES.len()])
            .unwrap_or(PERMISSION_MODES[0]);
        self.config.mode = Some(next.to_string());
        self.refresh_banner();
        // A live app-server session takes the mode immediately.
        if let Some(session_id) = self.app_session.clone() {
            self.send_control(
                "session/setMode",
                app_set_mode_params(&session_id, next),
                ControlReq::Command("/mode"),
            );
        }
        self.status = format!("mode {next} (Shift+Tab cycles)");
    }

    fn set_resume(&mut self, id: Option<&str>) {
        match id {
            Some(id) if id.starts_with("sess_") => {
                self.config.resume = Some(id.to_string());
                self.config.continue_session = false;
                self.session_active = false;
                // Drop any live streaming session: the next prompt must
                // handshake anew via session/resume, not reuse the old one.
                self.app_session = None;
                self.reset_rewind_state();
                self.push_system(&format!("resuming {id} on the next prompt"));
            }
            Some(other) => {
                self.push_error(&format!("session ids look like sess_...: got {other}"));
                return;
            }
            None => {
                self.config.resume = None;
                self.config.continue_session = true;
                self.push_system("continuing the latest session for this directory");
            }
        }
        self.refresh_banner();
    }

    fn new_session(&mut self) {
        self.config.resume = None;
        self.config.continue_session = false;
        self.session_active = false;
        // Tell the kernel the discarded session is done with (best-effort),
        // then drop it so the next prompt creates a fresh one; the
        // connection itself is reused.
        self.close_app_session();
        self.app_session = None;
        self.reset_rewind_state();
        self.clear_log();
        self.push_system("fresh session: context resets on the next prompt");
        self.status = "new session".to_string();
    }

    fn apply_startup_report(&mut self, report: StartupReport) {
        self.kernel_version = report.kernel;
        if let Some(catalog) = report.model_catalog {
            self.apply_model_catalog(catalog);
        }
        self.db_state = match report.db {
            DbProbe::Supported(path) => {
                // The kernel already persists every prompt input; merge it in
                // as the base of Up/Down history (this process's inputs stay
                // on top). Read-only, failures leave the in-process history.
                if let Ok(conn) = open_kernel_db_ro(&path) {
                    if let Ok(persisted) = recent_input_history(&conn, 200) {
                        if !persisted.is_empty() {
                            let mut merged = persisted;
                            merged.append(&mut self.history);
                            merged.dedup();
                            self.history = merged;
                        }
                    }
                }
                DbState::Enabled(path)
            }
            DbProbe::Unsupported => {
                // The one allowed dim notice; everything else degrades silently.
                self.push_system(
                    "kernel db schema not recognized; live tool progress disabled \
                     (a newer zcode-tui may support it)",
                );
                DbState::Disabled
            }
            DbProbe::Missing => DbState::Disabled,
        };
        let banner_pos = self
            .log
            .iter()
            .position(|entry| matches!(entry.kind, LogKind::Banner));
        self.refresh_banner();
        let (Some(installed), Some(feed)) = (&report.installed, &report.feed) else {
            return;
        };
        if !is_newer_version(&feed.version, installed) {
            return;
        }
        let tip = build_update_tip(installed, feed, report.feed_base.as_deref());
        if banner_pos.is_some_and(|pos| pos < self.flushed_log) {
            self.log.push(LogLine::new(LogKind::Tip, &tip));
            self.status = format!("update available: ZCode {}", feed.version);
            return;
        }
        let logo_at = banner_pos.map(|pos| pos + 1).unwrap_or(self.log.len());
        // Unauthenticated startup already shows a Brand logo after the banner.
        // Reuse that slot for the update Logo, then insert only the Tip. Normal
        // configured startup has only the compact avatar in the banner, so the
        // update case inserts the big Logo + Tip here.
        let (shift_at, inserted) = if matches!(
            self.log.get(logo_at).map(|entry| entry.kind),
            Some(LogKind::Logo)
        ) {
            self.log[logo_at] = LogLine::new(LogKind::Logo, ZCODE_WORDMARK);
            self.log
                .insert(logo_at + 1, LogLine::new(LogKind::Tip, &tip));
            (logo_at + 1, 1)
        } else {
            self.log
                .insert(logo_at, LogLine::new(LogKind::Logo, ZCODE_WORDMARK));
            self.log
                .insert(logo_at + 1, LogLine::new(LogKind::Tip, &tip));
            (logo_at, 2)
        };
        // The probe can land mid-turn: entries were just inserted near the top,
        // so every live index that pointed at or past the insertion point must
        // shift — otherwise streamed text lands in the wrong entry (and a later
        // remove() could delete the wrong one).
        if let Some(turn) = &mut self.app_turn {
            if turn.committable_end >= shift_at {
                turn.committable_end += inserted;
            }
            if let Some(ti) = turn.text_index {
                if ti >= shift_at {
                    turn.text_index = Some(ti + inserted);
                }
            }
        }
        if let Some(active) = &mut self.job {
            if active.log_index >= shift_at {
                active.log_index += inserted;
            }
        }
        if !self.unfolded.is_empty() {
            self.unfolded = self
                .unfolded
                .iter()
                .map(|&i| if i >= shift_at { i + inserted } else { i })
                .collect();
        }
        if let Some((index, _)) = &mut self.expanded_log {
            if *index >= shift_at {
                *index += inserted;
            }
        }
        self.status = format!("update available: ZCode {}", feed.version);
    }

    fn apply_model_catalog(&mut self, catalog: ModelCatalogReport) {
        self.model_provider = Some(catalog.provider_id);
        self.controls.models.clear();
        self.merge_controls(catalog.controls);
    }

    fn reload_model_catalog(&mut self) {
        if let Some(catalog) = refresh_model_catalog(&self.zcode_bin, &self.app_workspace()) {
            self.apply_model_catalog(catalog);
        }
    }

    fn push_user(&mut self, text: &str) {
        self.log.push(LogLine::new(LogKind::User, text));
    }

    fn push_system(&mut self, text: &str) {
        self.log.push(LogLine::new(LogKind::System, text));
    }

    fn push_error(&mut self, text: &str) {
        self.log.push(LogLine::new(LogKind::Error, text));
        self.status = "error".to_string();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogKind {
    Banner,
    Logo,
    Tip,
    User,
    Assistant,
    System,
    Error,
    Diff,
    Tool,
}

/// Long assistant replies stay full; everything mechanical can fold.
fn foldable_kind(kind: LogKind) -> bool {
    matches!(
        kind,
        LogKind::Tool | LogKind::System | LogKind::Diff | LogKind::Error
    )
}

#[derive(Debug)]
struct LogLine {
    kind: LogKind,
    text: String,
    /// Exempt from long-output folding. Set for DIRECT ANSWERS the user
    /// asked to read (/skills list, /mcp list, /status, /usage): folding
    /// exists to keep mechanical tool/shell output from flooding the
    /// transcript, not to hide a listing the user explicitly requested.
    no_fold: bool,
}

impl LogLine {
    fn new(kind: LogKind, text: &str) -> Self {
        Self {
            kind,
            text: text.to_string(),
            no_fold: false,
        }
    }

    /// A user-requested listing: renders like its kind but never folds.
    fn unfolded(kind: LogKind, text: &str) -> Self {
        Self {
            kind,
            text: text.to_string(),
            no_fold: true,
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

    let status = Command::new(&program)
        .args(&args)
        .status()
        .with_context(|| format!("failed to run editor: {program}"))?;
    if !status.success() {
        return Err(anyhow::anyhow!("editor exited with {status}"));
    }

    let updated =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let _ = fs::remove_file(&path);
    Ok(updated)
}

struct TerminalGuard {
    terminal: Tui,
    clear_generation: u64,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode().context("failed to enable raw mode")?;
        execute!(io::stdout(), EnableBracketedPaste, Hide)?;
        let terminal = (|| -> Result<Tui> {
            let (_, rows) = crossterm::terminal::size().context("failed to read terminal size")?;
            let backend = CrosstermBackend::new(io::stdout());
            Ok(Terminal::with_options(
                backend,
                TerminalOptions {
                    viewport: Viewport::Inline(
                        rows.saturating_sub(1).clamp(1, INLINE_VIEWPORT_ROWS),
                    ),
                },
            )?)
        })();
        match terminal {
            Ok(terminal) => Ok(Self {
                terminal,
                clear_generation: 0,
            }),
            Err(error) => {
                let _ = disable_raw_mode();
                let _ = execute!(io::stdout(), Show, DisableBracketedPaste);
                Err(error)
            }
        }
    }

    fn flush_transcript(&mut self, state: &mut UiState) -> Result<()> {
        if self.clear_generation != state.clear_generation {
            execute!(self.terminal.backend_mut(), TerminalClear(ClearType::Purge))?;
            self.terminal.clear()?;
            self.clear_generation = state.clear_generation;
        }
        let end = state.committable_log_end();
        while state.flushed_log < end {
            let index = state.flushed_log;
            let area = self.terminal.size()?;
            let width = area.width.saturating_sub(2).max(1) as usize;
            if state.log[index].kind == LogKind::Logo
                && !ascii_logo_fits(width, area.height, state.skyline_mode)
            {
                state.flushed_log += 1;
                continue;
            }
            let mut items = Vec::new();
            if log_entry_needs_separator(&state.log, index) {
                items.push(ListItem::new(Line::default()));
            }
            items.extend(rendered_log_entry(state, index, width));
            self.insert_transcript_items(items)?;
            state.flushed_log += 1;
        }
        Ok(())
    }

    fn insert_transcript_items(&mut self, items: Vec<ListItem<'static>>) -> Result<()> {
        let height = u16::try_from(items.len()).unwrap_or(u16::MAX).max(1);
        self.terminal.insert_before(height, move |buffer| {
            let area = Rect {
                x: buffer.area.x.saturating_add(1),
                y: buffer.area.y,
                width: buffer.area.width.saturating_sub(2),
                height: buffer.area.height,
            };
            Widget::render(List::new(items), area, buffer);
        })?;
        Ok(())
    }

    fn draw(&mut self, state: &mut UiState) -> Result<()> {
        self.terminal.draw(|frame| render(frame, state))?;
        Ok(())
    }

    /// Apply the terminal's resize event immediately and invalidate the prior
    /// frame. Inline autoresize can otherwise consume the new dimensions
    /// before the queued event arrives, leaving unchanged footer cells absent
    /// after the viewport clear.
    fn resize(&mut self, width: u16, height: u16) -> Result<()> {
        self.terminal.resize(Rect::new(0, 0, width, height))?;
        self.terminal.clear()?;
        Ok(())
    }

    /// Leave the TUI, run `f` with the normal terminal, then restore.
    fn suspend<T>(&mut self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        disable_raw_mode().context("failed to disable raw mode")?;
        execute!(io::stdout(), DisableBracketedPaste, Show)
            .context("failed to suspend inline terminal")?;
        let result = f();
        execute!(io::stdout(), EnableBracketedPaste, Hide)
            .context("failed to resume inline terminal")?;
        enable_raw_mode().context("failed to re-enable raw mode")?;
        self.terminal
            .clear()
            .context("failed to redraw after suspend")?;
        result
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), Show, DisableBracketedPaste);
    }
}

fn render(frame: &mut Frame<'_>, state: &mut UiState) {
    let root = frame.area();
    let composer_width = root.width.saturating_sub(3).max(1) as usize;
    let input_lines = composer_layout(&state.input, state.cursor, composer_width)
        .lines
        .len() as u16;
    let composer_height = input_lines.clamp(1, 5) + 2;
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(composer_height),
            Constraint::Length(1),
        ])
        .split(root);

    render_conversation(frame, vertical[0], state);
    render_composer(frame, vertical[2], state);
    render_footer(frame, vertical[3], state);

    if !state.suggestions.is_empty() {
        render_suggestions(frame, vertical[2], state);
    }
    if state.show_help {
        render_help_modal(frame, centered_rect(74, 70, root), &state.theme);
    }
    if state.show_palette {
        render_command_palette(frame, centered_rect(82, 68, root), state);
    }
    if state.session_picker.is_some() {
        render_session_picker(frame, centered_rect(84, 60, root), state);
    }
    if state.agents.is_open() {
        render_background_tasks(frame, centered_rect(92, 90, root), state);
    }
    if state.model_picker.is_some() {
        render_model_picker(frame, centered_rect(64, 46, root), state);
    }
    if state.rewind.is_some() {
        render_rewind(frame, centered_rect(78, 58, root), state);
    }
    if state.history_query.is_some() {
        render_history_search(frame, centered_rect(72, 50, root), state);
    }
    if state.expanded_log.is_some() {
        render_expanded_log(frame, centered_rect(88, 82, root), state);
    }
    // Topmost: the kernel is blocked on this answer.
    if state.interaction.is_some() {
        render_interaction(frame, centered_rect(74, 62, root), state);
    }
}

/// Max lines of forming assistant text shown with the current turn.
const LIVE_TEXT_TAIL: usize = 4;

/// In-progress reasoning and tool state inserted immediately below the current
/// user message. These rows disappear when the authoritative result lands.
fn live_panel_lines(state: &UiState) -> Vec<Line<'static>> {
    let t = &state.theme;
    // The kernel's TODO list persists across turns while non-empty (state
    // pushes update or clear it) — always the panel's top section.
    let mut todo_lines: Vec<Line<'static>> = Vec::new();
    if !state.todos.is_empty() {
        let done = state.todos.iter().filter(|item| item.done).count();
        todo_lines.push(Line::from(Span::styled(
            format!(" todos {done}/{}", state.todos.len()),
            t.dim(),
        )));
        for item in state.todos.iter().take(6) {
            let (mark, style) = if item.done {
                ("✓", t.dim())
            } else {
                ("·", t.text())
            };
            let clipped: String = item.text.chars().take(110).collect();
            todo_lines.push(Line::from(Span::styled(
                format!("  {mark} {clipped}"),
                style,
            )));
        }
        if state.todos.len() > 6 {
            todo_lines.push(Line::from(Span::styled(
                format!("  … +{} more", state.todos.len() - 6),
                t.dim(),
            )));
        }
    }
    // App-server streaming turn: answer text already streams into the current
    // transcript; these rows explain the work occurring before and around it.
    if let Some(turn) = &state.app_turn {
        let mut lines = todo_lines;
        let symbol = SPINNER_FRAMES[state.tick % SPINNER_FRAMES.len()];
        lines.push(Line::from(Span::styled(
            format!(" {symbol} thinking"),
            t.accent(),
        )));
        // Tools still running show as spinner chips; finished ones have already
        // dropped into the transcript (foldable), so they leave the panel.
        let running: Vec<&zcode_tui::AppToolCall> = turn
            .turn
            .tools
            .iter()
            .filter(|tool| !tool.finished)
            .collect();
        if !running.is_empty() {
            let mut spans = vec![Span::raw(" ".to_string())];
            for (index, tool) in running.iter().rev().take(6).rev().enumerate() {
                if index > 0 {
                    spans.push(Span::raw("   ".to_string()));
                }
                spans.push(Span::styled(format!("{symbol} "), t.accent()));
                let name = if tool.name.is_empty() {
                    "tool"
                } else {
                    tool.name.as_str()
                };
                spans.push(Span::styled(name.to_string(), t.text()));
            }
            lines.push(Line::from(spans));
        }
        let reasoning = turn.turn.reasoning.trim();
        if !reasoning.is_empty() {
            let tail: Vec<&str> = reasoning
                .lines()
                .filter(|line| !line.trim().is_empty())
                .rev()
                .take(LIVE_TEXT_TAIL)
                .collect();
            for line in tail.into_iter().rev() {
                let clipped: String = line.chars().take(120).collect();
                lines.push(Line::from(Span::styled(format!("    {clipped}"), t.dim())));
            }
        }
        return lines;
    }
    let Some(active) = &state.job else {
        return todo_lines;
    };
    let Some(live) = &active.live else {
        return todo_lines;
    };
    let mut lines = todo_lines;
    if !live.chips.is_empty() {
        let mut spans = vec![Span::raw(" ".to_string())];
        // Keep the newest chips in view when a turn runs many tools.
        let visible: Vec<&LiveToolChip> = live.chips.iter().rev().take(6).collect();
        for (index, chip) in visible.into_iter().rev().enumerate() {
            if index > 0 {
                spans.push(Span::raw("   ".to_string()));
            }
            match chip.status {
                ToolChipStatus::Running => {
                    let symbol = SPINNER_FRAMES[state.tick % SPINNER_FRAMES.len()];
                    spans.push(Span::styled(format!("{symbol} "), t.accent()));
                    spans.push(Span::styled(chip.tool.clone(), t.text()));
                }
                ToolChipStatus::Completed => {
                    spans.push(Span::styled("✓ ".to_string(), t.good()));
                    spans.push(Span::styled(chip.tool.clone(), t.dim()));
                    if let Some(ms) = chip.duration_ms {
                        spans.push(Span::styled(
                            format!(" {:.1}s", ms as f32 / 1000.0),
                            t.dim(),
                        ));
                    }
                }
                ToolChipStatus::Failed => {
                    spans.push(Span::styled("✗ ".to_string(), t.bad()));
                    spans.push(Span::styled(chip.tool.clone(), t.dim()));
                }
            }
        }
        lines.push(Line::from(spans));
    }
    if let Some(reasoning) = &live.reasoning {
        lines.push(Line::from(Span::styled(
            format!(" ⠿ thinking  {reasoning}"),
            t.dim(),
        )));
    }
    // The answer forming, tail-first so the newest content stays in view.
    if let Some(text) = &live.text {
        let tail: Vec<&str> = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .rev()
            .take(LIVE_TEXT_TAIL)
            .collect();
        for (index, line) in tail.into_iter().rev().enumerate() {
            let clipped: String = line.chars().take(120).collect();
            let prefix = if index == 0 { " •  " } else { "    " };
            lines.push(Line::from(Span::styled(
                format!("{prefix}{clipped}"),
                t.text(),
            )));
        }
    }
    lines
}

/// The `runtimeModel` to attach to `session/resume`, built from the kernel's
/// own `~/.zcode/cli/config.json` (the file session/create seeds fresh
/// sessions from). None → resume bare; the create-fallback still saves us.
fn load_runtime_model() -> Option<serde_json::Value> {
    let home = env::var("HOME").ok()?;
    let config = fs::read_to_string(kernel_config_path_from(std::path::Path::new(&home))).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    build_runtime_model(&config, now)
}

/// `session/usage` result → readable summary (shape pinned live 2026-07-07).
fn format_session_usage(result: &serde_json::Value) -> String {
    let n = |key: &str| {
        result
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    format!(
        "session usage\n  total {}  ·  in {}  ·  out {}  ·  reasoning {}\n  \
         model requests {}  ·  cache read {}",
        n("totalTokens"),
        n("inputTokens"),
        n("outputTokens"),
        n("reasoningTokens"),
        n("modelRequestCount"),
        n("cacheReadTokens"),
    )
}

/// `usage/stats` result → readable summary.
fn format_usage_stats(result: &serde_json::Value) -> String {
    let range = result.get("range").and_then(|v| v.as_str()).unwrap_or("7d");
    let s = |key: &str| {
        result
            .pointer(&format!("/summary/{key}"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    let hit_rate = result
        .pointer("/summary/cacheHitRate")
        .and_then(serde_json::Value::as_f64)
        .map(|rate| format!("{:.0}%", rate * 100.0))
        .unwrap_or_else(|| "-".to_string());
    format!(
        "usage over {range}\n  total {}  ·  in {}  ·  out {}\n  \
         sessions {}  ·  cache hit {}",
        s("totalTokens"),
        s("inputTokens"),
        s("outputTokens"),
        s("totalSessions"),
        hit_rate,
    )
}

fn render_conversation(frame: &mut Frame<'_>, area: Rect, state: &mut UiState) {
    let inner = Rect {
        x: area.x.saturating_add(1),
        y: area.y,
        width: area.width.saturating_sub(2),
        height: area.height,
    };
    let live_lines = live_panel_lines(state);
    if live_lines.is_empty() {
        return;
    }
    frame.render_widget(
        Paragraph::new(live_lines)
            .wrap(Wrap { trim: false })
            .scroll((0, 0)),
        inner,
    );
}

fn rendered_log_entry(state: &UiState, index: usize, width: usize) -> Vec<ListItem<'static>> {
    let entry = &state.log[index];
    if foldable_kind(entry.kind) && !entry.no_fold && !state.unfolded.contains(&index) {
        if let Some((head, hidden)) = fold_preview(&entry.text, FOLD_THRESHOLD, FOLD_HEAD) {
            let preview_text = entry.text.lines().take(head).collect::<Vec<_>>().join("\n");
            let preview = LogLine::new(entry.kind, &preview_text);
            let mut items = log_to_items(&preview, width, &state.theme, state.skyline_mode);
            items.push(ListItem::new(Line::from(Span::styled(
                format!("  … (+{hidden} lines)"),
                state.theme.dim(),
            ))));
            return items;
        }
    }
    log_to_items(entry, width, &state.theme, state.skyline_mode)
}

fn log_entry_needs_separator(log: &[LogLine], index: usize) -> bool {
    index > 0 && log[index].kind != LogKind::User && log[index - 1].kind != LogKind::Assistant
}

/// Split a line into (matches, chunk) runs so adjacent chars of the same
/// class share one span.
fn chunk_by(text: &str, pred: impl Fn(char) -> bool) -> Vec<(bool, String)> {
    let mut runs: Vec<(bool, String)> = Vec::new();
    for c in text.chars() {
        let class = pred(c);
        match runs.last_mut() {
            Some((last, chunk)) if *last == class => chunk.push(c),
            _ => runs.push((class, c.to_string())),
        }
    }
    runs
}

fn wrap_words_display(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rest = text.trim_end();
    let mut lines = Vec::new();
    while rest.width() > width {
        let mut used = 0usize;
        let mut hard_break = rest.len();
        let mut word_break = None;
        for (byte, ch) in rest.char_indices() {
            let cell_width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + cell_width > width {
                hard_break = byte;
                break;
            }
            used += cell_width;
            if ch.is_whitespace() {
                word_break = Some(byte);
            }
        }
        let split = word_break.filter(|index| *index > 0).unwrap_or(hard_break);
        let split = if split == 0 {
            rest.char_indices()
                .nth(1)
                .map(|(byte, _)| byte)
                .unwrap_or(rest.len())
        } else {
            split
        };
        lines.push(rest[..split].trim_end().to_string());
        rest = rest[split..].trim_start();
    }
    lines.push(rest.to_string());
    lines
}

fn ascii_logo_width() -> usize {
    ZCODE_WORDMARK
        .lines()
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0)
}

fn ascii_logo_fits(width: usize, height: u16, mode: SkylineMode) -> bool {
    mode != SkylineMode::None && width >= ascii_logo_width() && height >= LOGO_ROWS
}

fn log_to_items(
    entry: &LogLine,
    width: usize,
    theme: &Theme,
    _mode: SkylineMode,
) -> Vec<ListItem<'static>> {
    let mut items: Vec<ListItem<'static>> = Vec::new();
    match entry.kind {
        LogKind::Banner => {
            let max_inner = width.saturating_sub(4).max(1);
            let source = entry.text.lines().collect::<Vec<_>>();
            let title = source.first().copied().unwrap_or_default();
            let subtitle = source.get(1).copied().unwrap_or_default();
            let icon_width = MINI_Z_ICON[0].width();
            let right_header_width = title.width().max(subtitle.width());
            let show_icon = max_inner >= icon_width + 3 + right_header_width;
            let show_wordmark = show_icon && max_inner >= icon_width + 3 + ascii_logo_width();
            let wordmark = ZCODE_WORDMARK.lines().collect::<Vec<_>>();
            let mut content: Vec<(Option<&str>, String)> = Vec::new();
            if show_icon {
                for (row, icon) in MINI_Z_ICON.iter().enumerate() {
                    let right = match row {
                        0 => title,
                        1 => subtitle,
                        2..=7 if show_wordmark => wordmark[row - 2],
                        _ => "",
                    };
                    content.push((Some(*icon), right.to_string()));
                }
                for line in source.iter().skip(2) {
                    if line.is_empty() {
                        content.push((None, String::new()));
                    } else {
                        content.extend(
                            wrap_words_display(line, max_inner)
                                .into_iter()
                                .map(|line| (None, line)),
                        );
                    }
                }
            } else {
                for line in source {
                    if line.is_empty() {
                        content.push((None, String::new()));
                    } else {
                        content.extend(
                            wrap_words_display(line, max_inner)
                                .into_iter()
                                .map(|line| (None, line)),
                        );
                    }
                }
            }
            let inner = content
                .iter()
                .map(|(icon, line)| icon.map_or(0, |icon| icon.width() + 3) + line.width())
                .max()
                .unwrap_or(0)
                .min(max_inner);
            items.push(ListItem::new(Line::from(Span::styled(
                format!("╭{}╮", "─".repeat(inner + 2)),
                theme.frame(),
            ))));
            for (icon, raw) in &content {
                let mut spans = vec![Span::styled("│ ".to_string(), theme.frame())];
                let mut used = raw.width();
                if let Some(icon) = icon {
                    spans.extend(official_icon_spans(icon, theme));
                    spans.push(Span::raw("   ".to_string()));
                    used += icon.width() + 3;
                }
                spans.extend(banner_spans(raw, theme));
                let pad = inner.saturating_sub(used);
                spans.push(Span::raw(" ".repeat(pad)));
                spans.push(Span::styled(" │".to_string(), theme.frame()));
                items.push(ListItem::new(Line::from(spans)));
            }
            items.push(ListItem::new(Line::from(Span::styled(
                format!("╰{}╯", "─".repeat(inner + 2)),
                theme.frame(),
            ))));
        }
        LogKind::Logo => {
            // Text reconstruction of the official SVG wordmark. It remains
            // selectable and reflow-safe because it uses ordinary cells only.
            for raw in entry.text.lines() {
                items.push(ListItem::new(Line::from(Span::styled(
                    format!(" {raw}"),
                    theme.accent().bold(),
                ))));
            }
        }
        LogKind::Tip => {
            for (index, raw) in entry.text.lines().enumerate() {
                let mut spans = Vec::new();
                let mut rest = raw;
                if index == 0 {
                    if let Some(body) = raw.strip_prefix("Tip:") {
                        spans.push(Span::styled("Tip:".to_string(), theme.text().bold()));
                        rest = body;
                    }
                }
                spans.extend(spans_with_links(
                    rest,
                    theme.text(),
                    theme.accent().underlined(),
                ));
                items.push(ListItem::new(Line::from(spans)));
            }
        }
        LogKind::User => {
            // Restore the compact background band without a textual label.
            // Every span inherits the same background so spaces do not look
            // like isolated highlighted cells.
            items.push(ListItem::new(Line::default()).style(theme.band()));
            let content_width = width.saturating_sub(3 + TRANSCRIPT_RIGHT_GUTTER).max(1);
            for (index, piece) in wrap_display(&entry.text, content_width)
                .into_iter()
                .enumerate()
            {
                let prefix = if index == 0 {
                    Span::styled(" › ".to_string(), theme.accent().bold())
                } else {
                    Span::raw("   ".to_string())
                };
                items.push(
                    ListItem::new(Line::from(vec![prefix, Span::styled(piece, theme.text())]))
                        .style(theme.band()),
                );
            }
            items.push(ListItem::new(Line::default()).style(theme.band()));
        }
        LogKind::Assistant => {
            let content_width = width.saturating_sub(3 + TRANSCRIPT_RIGHT_GUTTER).max(1);
            for (index, styled) in markdown_lines(&entry.text, content_width)
                .into_iter()
                .enumerate()
            {
                let mut spans = Vec::new();
                if index == 0 {
                    spans.push(Span::styled("•  ".to_string(), theme.dim()));
                } else if styled.kind == MdLineKind::Quote {
                    spans.push(Span::styled(">  ".to_string(), theme.good()));
                } else {
                    spans.push(Span::raw("   ".to_string()));
                }
                if styled.kind == MdLineKind::DiffBlock {
                    // Colored ```diff fences, like /diff output.
                    let raw: String = styled.spans.iter().map(|span| span.text.as_str()).collect();
                    let style = match diff_line_role(&raw) {
                        DiffRole::Add => theme.good(),
                        DiffRole::Remove => theme.bad(),
                        DiffRole::Hunk => theme.accent(),
                        DiffRole::Meta => theme.dim().bold(),
                        DiffRole::Context => theme.text(),
                    };
                    spans.push(Span::styled(raw, style));
                    items.push(ListItem::new(Line::from(spans)));
                    continue;
                }
                if styled.kind == MdLineKind::CodeBlock {
                    // Editor-style panel: a 2-col inset to align under prose, an
                    // accent left rule, then a uniform code_bg band padded flush
                    // to the right edge — so the block reads as one container
                    // instead of ragged, character-hugging highlight.
                    let bar_style = if theme.plain {
                        Style::default()
                    } else {
                        theme.accent_dim().bg(theme.code_bg)
                    };
                    let mut row = vec![
                        Span::raw("  ".to_string()),
                        Span::styled("▎".to_string(), bar_style),
                        Span::styled(" ".to_string(), theme.code()),
                    ];
                    let mut used = 2usize; // left rule + lead space, inside the band
                    for span in styled.spans {
                        let mut style = md_style(theme, styled.kind, span.role);
                        if let Some((r, g, b)) = span.color {
                            if !theme.plain {
                                style = style.fg(Color::Rgb(r, g, b));
                            }
                        }
                        used += span.text.as_str().width();
                        row.push(Span::styled(span.text, style));
                    }
                    let pad = content_width.saturating_sub(used);
                    if pad > 0 && !theme.plain {
                        row.push(Span::styled(" ".repeat(pad), theme.code()));
                    }
                    items.push(ListItem::new(Line::from(row)));
                    continue;
                }
                for span in styled.spans {
                    let mut style = md_style(theme, styled.kind, span.role);
                    if let Some((r, g, b)) = span.color {
                        if !theme.plain {
                            style = style.fg(Color::Rgb(r, g, b));
                        }
                    }
                    spans.push(Span::styled(span.text, style));
                }
                items.push(ListItem::new(Line::from(spans)));
            }
            items.push(ListItem::new(Line::default()));
        }
        LogKind::Diff => {
            let content_width = width.saturating_sub(2 + TRANSCRIPT_RIGHT_GUTTER).max(1);
            for raw in entry.text.lines() {
                let style = match diff_line_role(raw) {
                    DiffRole::Add => theme.good(),
                    DiffRole::Remove => theme.bad(),
                    DiffRole::Hunk => theme.accent(),
                    DiffRole::Meta => theme.dim().bold(),
                    DiffRole::Context => theme.text(),
                };
                for piece in wrap_display(raw, content_width) {
                    items.push(ListItem::new(Line::from(vec![
                        Span::raw("  ".to_string()),
                        Span::styled(piece, style),
                    ])));
                }
            }
        }
        LogKind::Tool | LogKind::System | LogKind::Error => {
            let (marker, style) = match entry.kind {
                LogKind::Tool => ("•  ", theme.accent_dim()),
                LogKind::Error => ("✗  ", theme.bad()),
                _ => ("•  ", theme.dim()),
            };
            let content_width = width.saturating_sub(3 + TRANSCRIPT_RIGHT_GUTTER).max(1);
            for (index, piece) in wrap_display(&entry.text, content_width)
                .into_iter()
                .enumerate()
            {
                let prefix = if index == 0 {
                    Span::styled(marker.to_string(), style)
                } else {
                    Span::raw("   ".to_string())
                };
                items.push(ListItem::new(Line::from(vec![
                    prefix,
                    Span::styled(piece, style),
                ])));
            }
        }
    }
    if items.is_empty() {
        items.push(ListItem::new(Line::default()));
    }
    items
}

/// One-line summary of a `background_task_*` event for a system transcript line.
fn format_background_task(event: &AppServerEvent) -> String {
    let verb = match event.kind.as_str() {
        "background_task_started" => "started",
        "background_task_completed" => event.status.as_deref().unwrap_or("completed"),
        _ => "updated",
    };
    let tool = event.tool_name.as_deref().unwrap_or("task");
    let mut line = format!("background {tool} {verb}");
    if let Some(task_id) = &event.task_id {
        let short: String = task_id.chars().take(17).collect();
        if task_id.chars().count() > 20 {
            line.push_str(&format!(" · {short}…"));
        } else {
            line.push_str(&format!(" · {task_id}"));
        }
    }
    if let Some(pid) = event.pid {
        line.push_str(&format!(" (pid {pid})"));
    }
    line
}

fn md_style(theme: &Theme, kind: MdLineKind, role: SpanRole) -> Style {
    match kind {
        MdLineKind::Heading => theme.accent().bold(),
        MdLineKind::CodeBlock => {
            // Gutter numbers and the language tag (Marker) are dim, but every
            // cell of a code line sits on the code band so the panel stays
            // seamless — no default-bg holes punched through the fill.
            let base = if role == SpanRole::Marker {
                theme.dim()
            } else {
                theme.code()
            };
            if theme.plain {
                base
            } else {
                base.bg(theme.code_bg)
            }
        }
        MdLineKind::DiffBlock => theme.text(),
        MdLineKind::Quote => theme.good(),
        MdLineKind::Rule => theme.frame(),
        MdLineKind::Text => match role {
            SpanRole::Normal => theme.text(),
            SpanRole::Strong => theme.text().bold(),
            SpanRole::Emph => theme.text().italic(),
            // Codex renders inline code as colored text, no background box.
            SpanRole::Code => theme.accent(),
            SpanRole::Link => theme.accent().underlined(),
            SpanRole::Marker => theme.dim(),
        },
    }
}

/// Style spans for one banner-box line: compact logo/title rows first, then
/// `key: value   /hint` rows below.
fn banner_spans(line: &str, theme: &Theme) -> Vec<Span<'static>> {
    if line.is_empty() {
        return Vec::new();
    }
    if line.width() > 12 && line.contains('█') {
        return chunk_by(line, |ch| ch == '█')
            .into_iter()
            .map(|(block, text)| {
                Span::styled(
                    text,
                    if block {
                        theme.text().bold()
                    } else {
                        theme.dim()
                    },
                )
            })
            .collect();
    }
    if let Some(rest) = line.strip_prefix("╭──────╮  Welcome to ZCODE!") {
        let mut spans = official_icon_spans("╭──────╮", theme);
        spans.push(Span::raw("  ".to_string()));
        let rest = format!("Welcome to ZCODE!{rest}");
        match rest.split_once(" (") {
            Some((name, tail)) => {
                spans.push(Span::styled(name.to_string(), theme.accent().bold()));
                spans.push(Span::styled(format!(" ({tail}"), theme.dim()));
            }
            None => spans.push(Span::styled(rest.to_string(), theme.accent().bold())),
        }
        return spans;
    }
    if let Some(rest) = line.strip_prefix("│██████│  ") {
        let mut spans = official_icon_spans("│██████│", theme);
        spans.push(Span::raw("  ".to_string()));
        match rest.split_once("   /") {
            Some((name, hint)) => {
                spans.push(Span::styled(name.to_string(), theme.text()));
                spans.push(Span::styled(format!("   /{hint}"), theme.dim()));
            }
            None => spans.push(Span::styled(rest.to_string(), theme.text())),
        }
        return spans;
    }
    if matches!(
        line,
        "│   ██ │" | "│  ██  │" | "│ ██   │" | "│██████│" | "╰──────╯"
    ) {
        return official_icon_spans(line, theme);
    }
    if let Some(rest) = line.strip_prefix(">_ ") {
        let mut spans = vec![Span::styled(">_ ".to_string(), theme.accent().bold())];
        match rest.split_once(" (") {
            Some((name, tail)) => {
                spans.push(Span::styled(name.to_string(), theme.text().bold()));
                spans.push(Span::styled(format!(" ({tail}"), theme.dim()));
            }
            None => spans.push(Span::styled(rest.to_string(), theme.text().bold())),
        }
        return spans;
    }
    if let Some((key, value)) = line.split_once(": ") {
        let mut spans = vec![Span::styled(format!("{key}: "), theme.dim())];
        match value.split_once("   /") {
            Some((data, hint)) => {
                spans.push(Span::styled(data.to_string(), theme.text()));
                spans.push(Span::styled(format!("   /{hint}"), theme.dim()));
            }
            None => spans.push(Span::styled(value.to_string(), theme.text())),
        }
        return spans;
    }
    vec![Span::styled(line.to_string(), theme.text())]
}

fn official_icon_spans(icon: &str, theme: &Theme) -> Vec<Span<'static>> {
    icon.chars()
        .map(|ch| {
            let style = match ch {
                '█' => official_icon_mark(theme).bold(),
                ' ' => official_icon_fill(theme),
                _ => official_icon_frame(theme),
            };
            Span::styled(ch.to_string(), style)
        })
        .collect()
}

fn official_icon_frame(theme: &Theme) -> Style {
    if theme.plain {
        Style::default()
    } else {
        Style::default()
            .fg(Color::Rgb(88, 94, 108))
            .bg(Color::Rgb(7, 9, 12))
    }
}

fn official_icon_fill(theme: &Theme) -> Style {
    if theme.plain {
        Style::default()
    } else {
        Style::default().bg(Color::Rgb(7, 9, 12))
    }
}

fn official_icon_mark(theme: &Theme) -> Style {
    if theme.plain {
        Style::default()
    } else {
        Style::default()
            .fg(Color::Rgb(170, 176, 188))
            .bg(Color::Rgb(7, 9, 12))
    }
}

/// Split a line into plain spans and accent-styled URL spans.
fn spans_with_links(text: &str, base: Style, link: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("https://").or_else(|| rest.find("http://")) {
        if pos > 0 {
            spans.push(Span::styled(rest[..pos].to_string(), base));
        }
        let tail = &rest[pos..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '（' | '）' | '('))
            .unwrap_or(tail.len());
        spans.push(Span::styled(tail[..end].to_string(), link));
        rest = &tail[end..];
    }
    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_string(), base));
    }
    spans
}

fn render_composer(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let t = &state.theme;
    // Elevated band instead of a border, like the Codex composer.
    frame.render_widget(Paragraph::new("").style(t.band()), area);

    let mut lines: Vec<Line> = vec![Line::default()];
    if state.input.is_empty() {
        let placeholder = if area.width >= 54 {
            "describe a task   /commands   @files   !shell"
        } else if area.width >= 28 {
            "describe a task   /commands"
        } else {
            "describe a task"
        };
        lines.push(Line::from(vec![
            Span::styled(" › ".to_string(), t.accent().bold()),
            Span::styled(placeholder.to_string(), t.dim()),
        ]));
    } else {
        let content_width = area.width.saturating_sub(3).max(1) as usize;
        let layout = composer_layout(&state.input, state.cursor, content_width);
        let visible_rows = area.height.saturating_sub(2).max(1) as usize;
        let first_row = layout.cursor_row.saturating_sub(visible_rows - 1);
        for (offset, raw) in layout
            .lines
            .iter()
            .skip(first_row)
            .take(visible_rows)
            .enumerate()
        {
            let index = first_row + offset;
            let prefix = if index == 0 {
                Span::styled(" › ".to_string(), t.accent().bold())
            } else {
                Span::raw("   ".to_string())
            };
            lines.push(Line::from(vec![
                prefix,
                Span::styled(raw.clone(), t.text()),
            ]));
        }
        frame.render_widget(Paragraph::new(Text::from(lines)).style(t.band()), area);
        let cursor_x = area
            .x
            .saturating_add(3)
            .saturating_add(layout.cursor_col.min(content_width) as u16);
        let cursor_y = area.y.saturating_add(1).saturating_add(
            layout
                .cursor_row
                .saturating_sub(first_row)
                .min(visible_rows - 1) as u16,
        );
        frame.set_cursor_position((cursor_x, cursor_y));
        return;
    }
    frame.render_widget(Paragraph::new(Text::from(lines)).style(t.band()), area);
    frame.set_cursor_position((area.x.saturating_add(3), area.y.saturating_add(1)));
}

struct ComposerLayout {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
}

/// Wrap composer text by terminal cell width and locate the character cursor.
fn composer_layout(input: &str, cursor: usize, width: usize) -> ComposerLayout {
    let width = width.max(1);
    let chars = input.chars().collect::<Vec<_>>();
    let mut lines = vec![String::new()];
    let mut line_width = 0usize;
    for ch in &chars {
        if *ch == '\n' {
            lines.push(String::new());
            line_width = 0;
            continue;
        }
        let char_width = UnicodeWidthChar::width(*ch).unwrap_or(0);
        if line_width > 0 && line_width.saturating_add(char_width) > width {
            lines.push(String::new());
            line_width = 0;
        }
        lines.last_mut().expect("composer has a line").push(*ch);
        line_width = line_width.saturating_add(char_width);
    }

    let cursor = cursor.min(chars.len());
    let mut cursor_row = 0usize;
    let mut cursor_col = 0usize;
    for ch in chars.iter().take(cursor) {
        if *ch == '\n' {
            cursor_row += 1;
            cursor_col = 0;
            continue;
        }
        let char_width = UnicodeWidthChar::width(*ch).unwrap_or(0);
        if cursor_col > 0 && cursor_col.saturating_add(char_width) > width {
            cursor_row += 1;
            cursor_col = 0;
        }
        cursor_col = cursor_col.saturating_add(char_width);
    }
    if cursor_col >= width {
        cursor_row += cursor_col / width;
        cursor_col %= width;
    } else if let Some(next) = chars.get(cursor).filter(|ch| **ch != '\n') {
        let next_width = UnicodeWidthChar::width(*next).unwrap_or(0);
        if cursor_col > 0 && cursor_col.saturating_add(next_width) > width {
            cursor_row += 1;
            cursor_col = 0;
        }
    }
    while lines.len() <= cursor_row {
        lines.push(String::new());
    }
    ComposerLayout {
        lines,
        cursor_row,
        cursor_col,
    }
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let t = &state.theme;
    let left = if let Some(job) = &state.job {
        let frame_symbol = SPINNER_FRAMES[state.tick % SPINNER_FRAMES.len()];
        let queued = if state.queued.is_empty() {
            String::new()
        } else {
            format!("   queued {}", state.queued.len())
        };
        Line::from(vec![
            Span::styled(format!(" {frame_symbol} "), t.accent()),
            Span::styled(
                format!(
                    "{} · {:.0}s   Esc to interrupt{queued}",
                    job.label,
                    job.started.elapsed().as_secs_f32()
                ),
                t.dim(),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(format!(" {} ", state.status), t.accent_dim()),
            Span::styled(
                "  ⏎ send   ^J newline   ^P commands   ? help   ^C quit".to_string(),
                t.dim(),
            ),
        ])
    };
    let mut right = String::new();
    if let Some((used, window)) = state.context_watermark {
        right.push_str(&format_context_watermark(used, window));
        if context_watermark_warn(used, window) {
            // /compact keeps the session (app-server), /new resets it.
            right.push_str(" · /compact or /new?");
        }
        right.push_str(" · ");
    }
    // Current model + permission mode: authoritative from the kernel's state
    // pushes (SessionControls cache); before the first push fall back to the
    // configured mode alone (never guess a model name).
    let mode = state
        .controls
        .mode
        .clone()
        .unwrap_or_else(|| display_mode(&state.config).to_string());
    match &state.controls.model_current {
        Some(model) => right.push_str(&format!("{model} · {mode} · ")),
        None => right.push_str(&format!("{mode} · ")),
    }
    right.push_str(&format!("auth {} ", state.auth_label));
    if area.width < 80 {
        right = state
            .controls
            .model_current
            .clone()
            .unwrap_or_else(|| mode.clone());
    }
    if area.width < 48 {
        right.clear();
    }
    let right_width = u16::try_from(right.width())
        .unwrap_or(u16::MAX)
        .min(area.width);
    let left_area = Rect {
        width: area.width.saturating_sub(right_width.saturating_add(1)),
        ..area
    };
    let right_area = Rect {
        x: area.right().saturating_sub(right_width),
        width: right_width,
        ..area
    };
    frame.render_widget(Paragraph::new(left), left_area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(right, t.dim()))).alignment(Alignment::Right),
        right_area,
    );
}

fn render_suggestions(frame: &mut Frame<'_>, input_area: Rect, state: &UiState) {
    let t = &state.theme;
    let Some(area) = suggestion_popup_area(frame.area(), input_area, state.suggestions.len())
    else {
        return;
    };
    let items = state
        .suggestions
        .iter()
        .map(|suggestion| {
            ListItem::new(Line::from(Span::styled(
                suggestion.display.clone(),
                t.text(),
            )))
        })
        .collect::<Vec<_>>();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(t.frame())
        .title(Line::from(Span::styled(
            " Tab accepts ".to_string(),
            t.dim(),
        )));
    let list = List::new(items)
        .block(block)
        .highlight_style(t.selection())
        .highlight_symbol("› ");
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(
        list,
        area,
        &mut ListState::default().with_selected(Some(state.suggestion_index)),
    );
}

fn suggestion_popup_area(viewport: Rect, input_area: Rect, item_count: usize) -> Option<Rect> {
    let requested_height = (item_count as u16).min(SUGGESTION_LIMIT as u16) + 2;
    let height = requested_height.min(input_area.y.saturating_sub(viewport.y));
    (height > 0).then_some(Rect {
        x: input_area.x,
        y: input_area.y - height,
        width: input_area.width,
        height,
    })
}

fn render_command_palette(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let t = &state.theme;
    let rows = if state.input.starts_with('/') {
        let suggestions = slash_suggestions_merged(&state.input, 18, &state.kernel_commands);
        if suggestions.is_empty() {
            command_palette_rows()
        } else {
            suggestions
                .into_iter()
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
    } else {
        command_palette_rows()
    };

    let items = rows
        .into_iter()
        .map(|row| ListItem::new(Line::from(Span::styled(row, t.text()))))
        .collect::<Vec<_>>();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(t.frame())
        .title(Line::from(Span::styled(
            " commands · Ctrl+P closes ".to_string(),
            t.dim(),
        )));
    frame.render_widget(Clear, area);
    frame.render_widget(List::new(items).block(block), area);
}

/// The kernel-awaits-approval overlay: prompt + plan under review + question,
/// then the options list (↑↓/Enter answers, Esc declines).
fn render_interaction(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let t = &state.theme;
    let Some(pending) = &state.interaction else {
        return;
    };
    let request = &pending.request;
    let Some(question) = request.questions.first() else {
        return;
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(t.accent())
        .title(Line::from(Span::styled(
            format!(" {} · Enter answers · Esc declines ", request.tool_name),
            t.dim(),
        )));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    // Body text above, options below (options own their exact height).
    let option_rows = question.options.len().min(6) as u16;
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(option_rows)])
        .split(inner);

    let mut body: Vec<Line> = Vec::new();
    if !request.prompt.is_empty() {
        body.push(Line::from(Span::styled(
            request.prompt.clone(),
            t.text().bold(),
        )));
        body.push(Line::default());
    }
    if let Some(plan) = &request.plan {
        for raw in plan.lines() {
            body.push(Line::from(Span::styled(raw.to_string(), t.text())));
        }
        body.push(Line::default());
    }
    if !question.question.is_empty() {
        body.push(Line::from(Span::styled(
            question.question.clone(),
            t.accent(),
        )));
    }
    frame.render_widget(
        Paragraph::new(Text::from(body)).wrap(Wrap { trim: false }),
        parts[0],
    );

    let items = question
        .options
        .iter()
        .map(|option| {
            let mut spans = vec![Span::styled(
                format!("{:<12}", option.label),
                t.text().bold(),
            )];
            if !option.description.is_empty() {
                spans.push(Span::styled(option.description.clone(), t.dim()));
            }
            ListItem::new(Line::from(spans))
        })
        .collect::<Vec<_>>();
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(t.selection())
            .highlight_symbol("› "),
        parts[1],
        &mut ListState::default().with_selected(Some(pending.selected)),
    );
}

/// The /model picker: kernel-reported models, current one preselected.
fn render_model_picker(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let t = &state.theme;
    let Some(index) = state.model_picker else {
        return;
    };
    let current = state.controls.model_current.as_deref();
    let items = state
        .controls
        .models
        .iter()
        .map(|choice| {
            let id = choice.reference.get("modelId").and_then(|v| v.as_str());
            let marker = if id.is_some() && id == current {
                "● "
            } else {
                "  "
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker}{:<24}", choice.label), t.text()),
                Span::styled(choice.provider.clone(), t.dim()),
            ]))
        })
        .collect::<Vec<_>>();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(t.frame())
        .title(Line::from(Span::styled(
            " model · Enter selects · Esc closes ".to_string(),
            t.dim(),
        )));
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(t.selection())
            .highlight_symbol("› "),
        area,
        &mut ListState::default().with_selected(Some(index)),
    );
}

/// The /rewind overlay: stage 1 lists the targets, stage 2 shows the
/// previewFileRewind result with the scope selector.
fn render_rewind(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let t = &state.theme;
    let Some(overlay) = &state.rewind else {
        return;
    };
    frame.render_widget(Clear, area);
    match &overlay.preview {
        // Stage 1: target picker.
        None => {
            let items = overlay
                .targets
                .iter()
                .map(|(label, _)| ListItem::new(Line::from(Span::styled(label.clone(), t.text()))))
                .collect::<Vec<_>>();
            let title = if overlay.busy {
                " rewind · previewing… ".to_string()
            } else {
                " rewind · Enter previews · Esc closes ".to_string()
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(t.frame())
                .title(Line::from(Span::styled(title, t.dim())));
            frame.render_stateful_widget(
                List::new(items)
                    .block(block)
                    .highlight_style(t.selection())
                    .highlight_symbol("› "),
                area,
                &mut ListState::default().with_selected(Some(overlay.selected)),
            );
        }
        // Stage 2: preview + scope.
        Some((target, preview)) => {
            let v4 = target.is_v4();
            let mut lines: Vec<Line<'static>> = Vec::new();
            lines.push(Line::from(Span::styled(
                format!("target: {}", target.label()),
                t.text(),
            )));
            lines.push(Line::from(Span::styled(
                "restores the workspace state captured BEFORE that turn/write ran",
                t.dim(),
            )));
            lines.push(Line::default());
            if preview.safe.is_empty() && preview.unsafe_files.is_empty() {
                lines.push(Line::from(Span::styled(
                    "no file changes to restore",
                    t.dim(),
                )));
            }
            for file in &preview.safe {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", file.note), t.good()),
                    Span::styled(file.path.clone(), t.text()),
                    Span::styled(format!("  [{}]", file.tools), t.dim()),
                ]));
            }
            for file in &preview.unsafe_files {
                lines.push(Line::from(vec![
                    Span::styled(format!("  ! {} ", file.note), t.bad()),
                    Span::styled(file.path.clone(), t.text()),
                    Span::styled(format!("  [{}]", file.tools), t.dim()),
                ]));
            }
            if preview.ignored > 0 {
                lines.push(Line::from(Span::styled(
                    format!("  {} ignored file(s)", preview.ignored),
                    t.dim(),
                )));
            }
            lines.push(Line::default());
            if !preview.can_apply {
                lines.push(Line::from(Span::styled(
                    if v4 {
                        "files changed outside the session — V4 file restore is blocked"
                    } else {
                        "files changed outside the session — file restore is blocked \
                         (conversation scope still works)"
                    },
                    t.bad(),
                )));
            }
            let mut scope_spans: Vec<Span<'static>> =
                vec![Span::styled("scope: ".to_string(), t.text())];
            if v4 {
                scope_spans.push(Span::styled("‹workspace›".to_string(), t.selection()));
            } else {
                for (index, scope) in REWIND_SCOPES.iter().enumerate() {
                    if index == overlay.scope {
                        scope_spans.push(Span::styled(format!("‹{scope}› "), t.selection()));
                    } else {
                        scope_spans.push(Span::styled(format!(" {scope}  "), t.dim()));
                    }
                }
            }
            lines.push(Line::from(scope_spans));
            lines.push(Line::from(Span::styled(
                if v4 {
                    "ZCode 3.5.3 V4 currently exposes verified file rewind only"
                } else {
                    "workspace restores files · conversation rewinds the kernel chat · both does each"
                },
                t.dim(),
            )));
            let title = if overlay.busy {
                " rewind preview · applying… ".to_string()
            } else if v4 {
                " rewind preview · Enter applies workspace · Esc back ".to_string()
            } else {
                " rewind preview · Enter applies · ←/→ scope · Esc back ".to_string()
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(t.frame())
                .title(Line::from(Span::styled(title, t.dim())));
            frame.render_widget(
                Paragraph::new(lines)
                    .block(block)
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
    }
}

fn render_session_picker(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let t = &state.theme;
    let Some((rows, index)) = &state.session_picker else {
        return;
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0);
    let home = env::var("HOME").ok();
    let items = rows
        .iter()
        .map(|row| {
            let title: String = row.title.chars().take(40).collect();
            let dir = shorten_home(&row.directory, home.as_deref());
            ListItem::new(Line::from(vec![
                Span::styled(format!("{title:<42}"), t.text()),
                Span::styled(
                    format!("{:>4}  ", relative_age(now_ms, row.time_updated)),
                    t.dim(),
                ),
                Span::styled(dir, t.dim()),
            ]))
        })
        .collect::<Vec<_>>();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(t.frame())
        .title(Line::from(Span::styled(
            " sessions · Enter resumes · Esc closes ".to_string(),
            t.dim(),
        )));
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(t.selection())
            .highlight_symbol("› "),
        area,
        &mut ListState::default().with_selected(Some(*index)),
    );
}

fn render_background_tasks(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let Some(selected) = state.agents.selected() else {
        return;
    };
    let t = &state.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(t.frame())
        .title(Line::from(Span::styled(
            " agents · observed lifecycle · ↑↓ selects · Esc closes ".to_string(),
            t.dim(),
        )));
    let inner = block.inner(area);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(inner);
    let items = state
        .agents
        .tasks()
        .iter()
        .map(|task| {
            let status = task.status.to_ascii_lowercase();
            let (symbol, style) = if status == "running" || status == "started" {
                ("●", t.accent())
            } else if status == "completed" || status == "success" {
                ("✓", t.good())
            } else if status.contains("fail") || status == "lost" {
                ("✗", t.bad())
            } else {
                ("·", t.dim())
            };
            let short_id: String = task.id.chars().take(18).collect();
            let pid = task.pid.map(|pid| format!("pid {pid}")).unwrap_or_default();
            ListItem::new(Line::from(vec![
                Span::styled(format!("{symbol} {:<10}", task.status), style),
                Span::styled(format!("{:<14}", task.tool), t.text()),
                Span::styled(format!("{short_id:<20}"), t.dim()),
                Span::styled(pid, t.dim()),
            ]))
        })
        .collect::<Vec<_>>();
    let selected = selected.min(state.agents.tasks().len().saturating_sub(1));
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(t.selection())
            .highlight_symbol("› "),
        parts[0],
        &mut ListState::default().with_selected(Some(selected)),
    );

    if let Some(task) = state.agents.tasks().get(selected) {
        let command = task
            .command
            .as_deref()
            .unwrap_or("(not provided by kernel)");
        let command = command.replace(['\r', '\n'], " ");
        let command: String = command.chars().take(120).collect();
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled("task: ".to_string(), t.dim()),
                    Span::styled(task.id.clone(), t.text()),
                ]),
                Line::from(vec![
                    Span::styled("command: ".to_string(), t.dim()),
                    Span::styled(command, t.text()),
                ]),
                Line::from(Span::styled(
                    "read-only: kernel exposes lifecycle events, not task logs or controls"
                        .to_string(),
                    t.dim(),
                )),
            ])
            .wrap(Wrap { trim: false }),
            parts[1],
        );
    }
}

fn render_history_search(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let t = &state.theme;
    let Some((query, index)) = &state.history_query else {
        return;
    };
    let matches = history_search(&state.history, query, HISTORY_SEARCH_LIMIT);
    let items = matches
        .iter()
        .map(|entry| {
            let line: String = entry.replace('\n', " ⏎ ").chars().take(96).collect();
            ListItem::new(Line::from(Span::styled(line, t.text())))
        })
        .collect::<Vec<_>>();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(t.frame())
        .title(Line::from(vec![
            Span::styled(" reverse search: ".to_string(), t.dim()),
            Span::styled(query.clone(), t.accent()),
            Span::styled(" · Enter recalls · Esc closes ".to_string(), t.dim()),
        ]));
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(t.selection())
            .highlight_symbol("› "),
        area,
        &mut ListState::default()
            .with_selected(Some((*index).min(matches.len().saturating_sub(1)))),
    );
}

fn render_help_modal(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.frame())
        .title(Line::from(Span::styled(" help ".to_string(), theme.dim())));
    let help = Paragraph::new(Text::styled(help_text(), theme.text()))
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(Clear, area);
    frame.render_widget(help, area);
}

fn render_expanded_log(frame: &mut Frame<'_>, area: Rect, state: &mut UiState) {
    let Some((index, requested_scroll)) = state.expanded_log else {
        return;
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(state.theme.frame())
        .title(Line::from(Span::styled(
            " expanded output · ↑↓/PageUp/PageDown · Esc closes ",
            state.theme.dim(),
        )));
    let inner = block.inner(area);
    let items = log_to_items(
        &state.log[index],
        inner.width.max(1) as usize,
        &state.theme,
        state.skyline_mode,
    );
    let max_scroll = (items.len() as u16).saturating_sub(inner.height);
    let scroll = requested_scroll.min(max_scroll);
    if let Some((_, current_scroll)) = state.expanded_log.as_mut() {
        *current_scroll = scroll;
    }
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(
        List::new(items).block(block),
        area,
        &mut ListState::default().with_offset(scroll as usize),
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composer_wraps_ascii_and_cjk_at_display_width() {
        let ascii = composer_layout("abcdef", 6, 5);
        assert_eq!(ascii.lines, vec!["abcde", "f"]);
        assert_eq!((ascii.cursor_row, ascii.cursor_col), (1, 1));

        let cjk = composer_layout("中文a", 3, 4);
        assert_eq!(cjk.lines, vec!["中文", "a"]);
        assert_eq!((cjk.cursor_row, cjk.cursor_col), (1, 1));

        let explicit = composer_layout("abcd\nef", 7, 4);
        assert_eq!(explicit.lines, vec!["abcd", "ef"]);
        assert_eq!((explicit.cursor_row, explicit.cursor_col), (1, 2));
    }

    #[test]
    fn transcript_does_not_wait_for_startup_probe() {
        let mut state = UiState::new(AppConfig::default(), "zcode".to_string());
        state.log.push(LogLine::new(LogKind::User, "hello"));
        assert_eq!(state.committable_log_end(), 1);
    }

    #[test]
    fn late_startup_probe_does_not_rewrite_flushed_banner() {
        let mut state = UiState::new(AppConfig::default(), "zcode".to_string());
        state.push_banner();
        let original = state.log[0].text.clone();
        state.flushed_log = 1;
        state.kernel_version = Some("9.9.9".to_string());

        state.refresh_banner();

        assert_eq!(state.log[0].text, original);
    }

    #[test]
    fn app_turn_commits_completed_phases_without_rewriting_open_text() {
        let mut state = UiState::new(AppConfig::default(), "zcode".to_string());
        state.db_state = DbState::Disabled;
        state.begin_app_turn(1);

        state.app_turn.as_mut().unwrap().turn.text = "before tool".to_string();
        state.app_append_text();
        assert_eq!(state.committable_log_end(), 0);

        state.app_commit_phase();
        assert_eq!(state.committable_log_end(), 1);
        assert_eq!(state.app_turn.as_ref().unwrap().text_index, None);

        state
            .app_turn
            .as_mut()
            .unwrap()
            .turn
            .tools
            .push(zcode_tui::AppToolCall {
                name: "Bash".to_string(),
                output: "done".to_string(),
                success: true,
                finished: true,
                ..Default::default()
            });
        state.app_push_tool_entry(0);
        assert_eq!(state.committable_log_end(), 2);

        state
            .app_turn
            .as_mut()
            .unwrap()
            .turn
            .text
            .push_str(" after tool");
        state.app_append_text();
        assert_eq!(state.committable_log_end(), 2);
        assert_eq!(state.log.len(), 3);

        state.finalize_app_turn();
        assert_eq!(state.committable_log_end(), 3);
    }

    #[test]
    fn background_task_events_update_the_read_only_task_list() {
        let mut state = UiState::new(AppConfig::default(), "zcode".to_string());
        state.capture_background_task_event(&AppServerEvent {
            kind: "background_task_started".to_string(),
            task_id: Some("bg-1".to_string()),
            tool_name: Some("Bash".to_string()),
            command: Some("sleep 12".to_string()),
            pid: Some(4242),
            ..Default::default()
        });
        state.capture_background_task_event(&AppServerEvent {
            kind: "background_task_completed".to_string(),
            task_id: Some("bg-1".to_string()),
            status: Some("completed".to_string()),
            ..Default::default()
        });

        assert_eq!(state.agents.tasks().len(), 1);
        assert_eq!(state.agents.tasks()[0].status, "completed");
        assert_eq!(state.agents.tasks()[0].pid, Some(4242));
        assert_eq!(state.agents.tasks()[0].command.as_deref(), Some("sleep 12"));

        state.open_background_tasks();
        assert_eq!(state.agents.selected(), Some(0));
        state.reset_rewind_state();
        assert!(state.agents.tasks().is_empty());
        assert_eq!(state.agents.selected(), None);
    }

    #[test]
    fn completed_long_output_expands_in_read_only_overlay() {
        let mut state = UiState::new(AppConfig::default(), "zcode".to_string());
        let output = (0..=FOLD_THRESHOLD)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        state.log.push(LogLine::new(LogKind::Tool, &output));
        state.flushed_log = 1;

        state.toggle_fold();
        assert_eq!(state.expanded_log, Some((0, 0)));

        state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(state.expanded_log, None);
    }

    #[test]
    fn user_message_spaces_share_the_message_band_background() {
        let theme = Theme::zhipu(false);
        let entry = LogLine::new(LogKind::User, "hello   world");
        let items = log_to_items(&entry, 40, &theme, SkylineMode::None);
        let area = Rect::new(0, 0, 40, items.len() as u16);
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        Widget::render(List::new(items), area, &mut buffer);

        assert_eq!(
            buffer.cell((8, 1)).expect("first space").bg,
            buffer.cell((3, 1)).expect("first letter").bg
        );
        assert_ne!(buffer.cell((8, 1)).expect("first space").bg, Color::Reset);
    }

    #[test]
    fn assistant_output_has_symmetric_vertical_padding() {
        let theme = Theme::zhipu(false);
        let entry = LogLine::new(LogKind::Assistant, "answer");
        let items = log_to_items(&entry, 40, &theme, SkylineMode::None);

        assert_eq!(items.len(), 2);
        let log = vec![entry, LogLine::new(LogKind::System, "next")];
        assert!(!log_entry_needs_separator(&log, 1));
    }

    #[test]
    fn suggestions_never_render_above_inline_viewport() {
        let viewport = Rect::new(0, 50, 106, 10);
        let composer = Rect::new(0, 55, 106, 3);
        let area = suggestion_popup_area(viewport, composer, SUGGESTION_LIMIT)
            .expect("suggestions fit above composer");

        assert_eq!(area.y, viewport.y);
        assert_eq!(area.height, 5);
        assert!(viewport.contains(area.as_position()));
    }

    #[test]
    fn enter_accepts_and_runs_partial_slash_command() {
        let mut state = UiState::new(AppConfig::default(), "zcode".to_string());
        state.set_input("/he");

        assert!(!state.suggestions.is_empty());
        assert!(!state.show_help);
        assert!(state
            .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .is_none());
        assert!(state.show_help);
        assert!(state.input.is_empty());
    }
}
