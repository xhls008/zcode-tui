use std::collections::{HashSet, VecDeque};
use std::env;
use std::fs;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::process::{self, Command, ExitStatus};
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};
use ratatui::{Frame, Terminal};
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::StatefulImage;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use zcode_tui::{
    app_compact_params, app_create_params, app_send_params, app_server_enabled,
    app_session_id_from_result, app_set_mode_params, app_set_model_params, app_set_thought_params,
    app_state_controls, app_state_is_turn_end, app_state_turn_error, app_state_watermark,
    app_steer_params, app_stop_params, app_subscribe_params, classify_input, command_palette_rows,
    context_watermark_warn, db_baseline, db_schema_supported, detect_auth_status, diff_line_role,
    encode_interaction_reply, env_is_headless, file_suggestions, fold_preview,
    format_context_watermark, git_diff_command, handle_local_command, help_text, history_search,
    is_newer_version, kernel_db_path_from, latest_assistant_text, latest_reasoning,
    latest_session_for_dir, leader_action_for_key, list_recent_sessions, live_tool_chips,
    load_ui_config, login_command, markdown_lines, open_kernel_db_ro, parse_cli_args,
    parse_interaction_request, parse_prompt_summary, parse_stream_event, parse_update_feed,
    parse_update_feed_url, prompt_command_for, recent_input_history, relative_age, run_command,
    shorten_home, skyline_braille, skyline_graphics_wanted, skyline_lines, skyline_mode,
    slash_suggestions, spawn_streaming_command, tool_input_summary, wrap_display, AppConfig,
    AppServerConn, AppServerMessage, AppServerTurn, AppServerUnavailable, AuthStatus, DbBaseline,
    DiffRole, InputAction, InteractionRequest, JobEvent, LeaderAction, LiveToolChip, MdLineKind,
    SessionControls, SessionRow, SkylineMode, SpanRole, StreamEvent, StreamingJob, ToolChipStatus,
    TurnDelta, UiConfig, UpdateFeed, SKYLINE_LOGO_W,
};

type Tui = Terminal<CrosstermBackend<Stdout>>;

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SUGGESTION_LIMIT: usize = 8;
/// Foldable cells longer than this render as a head preview by default.
const FOLD_THRESHOLD: usize = 24;
const FOLD_HEAD: usize = 8;
const HISTORY_SEARCH_LIMIT: usize = 8;

/// The ZCODE block wordmark. Rendered bright (`█` blocks) over a dim/shadow
/// secondary layer; the responsive Beijing-skyline wireframe (`skyline_lines`)
/// is drawn beneath it at render time so it stretches to the terminal width.
/// Used for both the not-configured welcome (Brand → 清华紫) and the update
/// notice (Logo → GLM 蓝).
const ZCODE_WORDMARK: &str = r#"███████╗  ██████╗  ██████╗  ██████╗  ███████╗
╚══███╔╝ ██╔════╝ ██╔═══██╗ ██╔══██╗ ██╔════╝
  ███╔╝  ██║      ██║   ██║ ██║  ██║ █████╗
 ███╔╝   ██║      ██║   ██║ ██║  ██║ ██╔══╝
███████╗ ╚██████╗ ╚██████╔╝ ██████╔╝ ███████╗
╚══════╝  ╚═════╝  ╚═════╝  ╚═════╝  ╚══════╝"#;

/// The true ZCODE logo (清华紫 purple block letters + Beijing-landmark line art
/// on the ZhiPU horizon), rendered via a terminal graphics protocol when
/// available. 480×231 RGBA, transparent background, duotoned onto the brand
/// purple so it matches the text wordmark. Falls back to the text skyline.
const LOGO_PNG: &[u8] = include_bytes!("../assets/zcode-logo.png");
/// Cell footprint reserved for the graphics logo. The 2.04:1 source is fit
/// (aspect-preserving) into this box; ~2:1 cell aspect keeps margins small.
const LOGO_IMG_ROWS: u16 = 14;
const LOGO_IMG_COLS: u16 = 58;

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
    let mut terminal = TerminalGuard::enter(state.mouse_enabled)?;
    // Probe for a graphics protocol now that the alt-screen is active and
    // before the event loop reads stdin (ratatui-image queries via stdio).
    state.init_graphics_logo();
    state.push_startup_frame();
    let probe = spawn_startup_probe(zcode_bin.to_string());

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
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => {
                    state.scroll = state.scroll.saturating_add(3);
                }
                MouseEventKind::ScrollDown => {
                    state.scroll = state.scroll.saturating_sub(3);
                }
                _ => {}
            },
            _ => {}
        }
    }

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
    match result {
        Ok(status) if status.success() => state.push_system("login command finished"),
        Ok(status) => state.push_error(&format!("login command exited with {status}")),
        Err(error) => state.push_error(&format!("{error:#}")),
    }
    state.refresh_auth();
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

/// Probe, off the UI thread: the CLI kernel version, the installed desktop
/// package version, and the official electron-updater feed (the same
/// latest-linux.yml the ZCode desktop app polls, so the notice matches the
/// official release channel). ZCODE_TUI_NO_UPDATE_CHECK=1 skips the network.
fn spawn_startup_probe(zcode_bin: String) -> std::sync::mpsc::Receiver<StartupReport> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let kernel = run_command(&[zcode_bin, "version".to_string()])
            .ok()
            .and_then(|output| {
                output
                    .lines()
                    .map(str::trim)
                    .find(|line| line.chars().next().is_some_and(|c| c.is_ascii_digit()))
                    .map(str::to_string)
            });
        let installed = run_command(&[
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
        .filter(|version| !version.is_empty());

        let mut feed = None;
        let mut feed_base = None;
        if env::var_os("ZCODE_TUI_NO_UPDATE_CHECK").is_none() {
            let candidates = [
                env::var("ZCODE_APP")
                    .ok()
                    .map(|app| format!("{app}/resources/app-update.yml")),
                Some("/opt/ZCode/resources/app-update.yml".to_string()),
            ];
            for path in candidates.into_iter().flatten() {
                let Ok(content) = fs::read_to_string(&path) else {
                    continue;
                };
                if let Some(url) = parse_update_feed_url(&content) {
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
                break;
            }
        }
        let _ = sender.send(StartupReport {
            kernel,
            installed,
            feed,
            feed_base,
            db: probe_kernel_db(),
        });
    });
    receiver
}

fn build_update_tip(installed: &str, feed: &UpdateFeed, feed_base: Option<&str>) -> String {
    let mut lines = vec![format!(
        "Tip: 官方 ZCode {} 已发布，本机 {installed}。更新说明: https://zcode.z.ai/en/changelog",
        feed.version
    )];
    match (feed_base, &feed.deb_file) {
        (Some(base), Some(file)) => {
            lines.push(format!("下载: {base}{file}"));
            lines.push(format!("安装: sudo apt install ./{file} 后无需其他改动"));
        }
        _ => lines.push("下载: https://zcode.z.ai".to_string()),
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
    /// Tsinghua purple, logo-only: never used on interactive elements, so
    /// the GLM-blue single-accent discipline stays intact.
    brand: Color,
    brand_dim: Color,
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
            // 清华紫 #660874 as the shadow; a lightened variant carries the
            // wordmark so it stays readable on dark terminals.
            brand: Color::Rgb(178, 108, 196),
            brand_dim: Color::Rgb(122, 42, 134),
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

    fn brand(&self) -> Style {
        self.styled(self.brand)
    }

    fn brand_dim(&self) -> Style {
        self.styled(self.brand_dim)
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
                "brand" => self.brand = color,
                "brand_dim" => self.brand_dim = color,
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

/// State of the experimental app-server streaming path for this process.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AppMode {
    /// `ZCODE_TUI_APP_SERVER` not set: always the classic `--prompt` path.
    Off,
    /// Opted in and healthy: prompts stream through the app-server.
    Ready,
    /// Opted in but a failure permanently downgraded this run to `--prompt`.
    Downgraded,
}

/// A single in-flight app-server turn. Answer text streams into an assistant
/// transcript entry token by token; running tools show as live chips and drop
/// into the transcript (foldable) as they finish, so text and tools interleave
/// in chronological order — the way Codex / Claude Code render a turn.
struct AppTurn {
    turn: AppServerTurn,
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
    Subscribe(u64),
}

/// Stage tag copied out of `ConnectPhase` so a poll loop can mutate `self`
/// without holding a borrow of `app_connect` across the arms.
#[derive(Clone, Copy)]
enum ConnectStage {
    Create,
    Subscribe,
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
    scroll: u16,
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
    /// /model picker overlay: selected index into `controls.models`.
    model_picker: Option<usize>,
    /// Ctrl+R reverse search overlay: query + selected index.
    history_query: Option<(String, usize)>,
    /// Log indices the user expanded with Ctrl+O (folding is the default
    /// for long foldable cells).
    unfolded: HashSet<usize>,
    mouse_enabled: bool,
    /// Experimental app-server streaming path (opt-in, seamless fallback).
    app_mode: AppMode,
    app_conn: Option<AppServerConn>,
    /// Kernel session reused across prompts once created (session continuity).
    app_session: Option<String>,
    app_turn: Option<AppTurn>,
    /// Welcome-skyline renderer (braille / wireframe / off), resolved once.
    skyline_mode: SkylineMode,
    /// Whether to attempt the true graphics-protocol logo before falling back
    /// to `skyline_mode`. Set from env; the actual capability probe runs once
    /// after entering the alt-screen (`init_graphics_logo`).
    skyline_graphics: bool,
    /// The decoded logo bound to a live graphics protocol (Sixel/Kitty/iTerm2)
    /// once the probe confirms support; `None` keeps the text skyline.
    graphics_logo: Option<StatefulProtocol>,
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
    /// In-flight fire-and-forget control requests (setMode/setModel/…), by
    /// request id, so an error response can name the command it failed.
    control_requests: std::collections::HashMap<u64, ControlReq>,
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
}

impl UiState {
    fn new(config: AppConfig, zcode_bin: String) -> Self {
        let auth_status = detect_auth_status();
        let auth_label = auth_status.short_label();
        let plain = config.no_color || env::var_os("NO_COLOR").is_some();
        let ui_config = load_ui_config();
        let mouse_enabled =
            env::var_os("ZCODE_TUI_NO_MOUSE").is_none() && ui_config.mouse != Some(false);
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
            scroll: 0,
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
            model_picker: None,
            history_query: None,
            unfolded: HashSet::new(),
            mouse_enabled,
            app_mode,
            app_conn: None,
            app_session: None,
            app_turn: None,
            skyline_mode: skyline_mode(|key| env::var(key).ok()),
            skyline_graphics: skyline_graphics_wanted(|key| env::var(key).ok()),
            graphics_logo: None,
            app_connect: None,
            app_draining: None,
            interaction: None,
            interaction_done: HashSet::new(),
            controls: SessionControls::default(),
            control_requests: std::collections::HashMap::new(),
        }
    }

    /// Probe the terminal for a graphics protocol and, if one is available,
    /// decode the embedded logo into a resize protocol. Call once after the
    /// alt-screen is active and before reading events (per ratatui-image). Any
    /// failure — probe error, no real protocol, decode error — leaves
    /// `graphics_logo` as `None`, so the text skyline renders instead.
    fn init_graphics_logo(&mut self) {
        if !self.skyline_graphics {
            return;
        }
        let Ok(picker) = Picker::from_query_stdio() else {
            return;
        };
        // Halfblocks is the no-graphics-protocol fallback; prefer our own
        // braille/wire skyline over ratatui-image's half-block cells.
        if picker.protocol_type() == ProtocolType::Halfblocks {
            return;
        }
        if let Ok(img) = image::load_from_memory(LOGO_PNG) {
            self.graphics_logo = Some(picker.new_resize_protocol(img));
        }
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

    /// Startup skeleton: a Kimi/Claude-style welcome card. Configured launches
    /// keep the logo compact inside the card; unauthenticated launches append
    /// the larger login guidance below it.
    fn push_startup_frame(&mut self) {
        self.push_banner();
        if !self.auth_status.is_configured() {
            self.push_unauth_screen_if_needed();
        }
    }

    fn push_brand_logo_if_missing(&mut self) {
        if self
            .log
            .iter()
            .any(|entry| matches!(entry.kind, LogKind::Brand | LogKind::Logo))
        {
            return;
        }
        self.log.push(LogLine::new(LogKind::Brand, ZCODE_WORDMARK));
    }

    /// Codex-style unauthenticated welcome: purple wordmark over the
    /// skyline strip, then the three browser-free ways in.
    fn push_unauth_screen_if_needed(&mut self) {
        if self.auth_status.is_configured() {
            return;
        }
        self.push_brand_logo_if_missing();
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
            self.suggestions = slash_suggestions(&self.input, SUGGESTION_LIMIT)
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
        if self.model_picker.is_some() {
            return self.handle_model_picker_key(key);
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
                self.status = "leader: p palette | h help | e editor | x clear | u input | q quit"
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
                if !self.suggestions.is_empty() && self.suggestion_nav {
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
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_add(6);
                self.status = format!("scrollback +{}", self.scroll);
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_sub(6);
                self.status = if self.scroll == 0 {
                    "following tail".to_string()
                } else {
                    format!("scrollback +{}", self.scroll)
                };
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
        self.scroll = 0;
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
        self.scroll = 0;

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
                self.push_system(output.trim_end());
                self.status = "ok".to_string();
            }
            Err(error) => self.push_error(&format!("{error:#}")),
        }
        if command.first().map(String::as_str) == Some("logout") {
            self.refresh_auth();
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
        // Experimental streaming path: true token streaming through the
        // long-lived app-server. Any failure downgrades this process
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
        }
        match self.app_session.clone() {
            // Session already open: send immediately (the fast path for every
            // prompt after the first).
            Some(session_id) => {
                // Drop any stray tail from a previously cancelled turn.
                self.drain_app_events();
                let conn = self.app_conn.as_mut().expect("app_conn set above");
                let send_id = conn.send("session/send", app_send_params(&session_id, prompt))?;
                self.begin_app_turn(send_id);
            }
            // First prompt of the run: kick off the create+subscribe handshake
            // WITHOUT blocking the UI thread. The prompt is sent once the
            // handshake completes (driven by `pump_app_connect`), so a slow or
            // hung app-server can never freeze the terminal — Esc still cancels.
            None => {
                let workspace = self.app_workspace();
                let conn = self.app_conn.as_mut().expect("app_conn set above");
                let create_id = conn.send("session/create", app_create_params(&workspace))?;
                self.app_connect = Some(AppConnect {
                    phase: ConnectPhase::Create(create_id),
                    prompt: prompt.to_string(),
                    started: Instant::now(),
                });
                self.status = "connecting (app-server)…".to_string();
            }
        }
        Ok(())
    }

    /// The canonical workspace path handed to `session/create`.
    fn app_workspace(&self) -> String {
        self.resolve_cwd()
            .canonicalize()
            .unwrap_or_else(|_| self.resolve_cwd())
            .to_string_lossy()
            .into_owned()
    }

    /// Open the assistant turn: text and tool entries are created lazily as
    /// their events arrive so they land in transcript order.
    fn begin_app_turn(&mut self, send_id: u64) {
        self.app_turn = Some(AppTurn {
            turn: AppServerTurn::default(),
            text_index: None,
            written: 0,
            started: Instant::now(),
            cancel_requested: false,
            got_text: false,
            send_id,
        });
        self.scroll = 0;
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
            let AppServerMessage::Response { id, result, error } = message else {
                // Stray events/state before the turn starts: nothing to render.
                continue;
            };
            // Copy the awaited stage+id out so the match arms can mutate `self`.
            let waiting = match &self.app_connect {
                Some(connect) => match &connect.phase {
                    ConnectPhase::Create(want) => (ConnectStage::Create, *want),
                    ConnectPhase::Subscribe(want) => (ConnectStage::Subscribe, *want),
                },
                None => return,
            };
            let (stage, want) = waiting;
            if id != want {
                continue; // stale/unmatched response id
            }
            if let Some(why) = error {
                self.app_connect_failed(AppServerUnavailable::Protocol(why));
                return;
            }
            match stage {
                ConnectStage::Create => {
                    let result = result.unwrap_or(serde_json::Value::Null);
                    let Some(session_id) = app_session_id_from_result(&result) else {
                        self.app_connect_failed(AppServerUnavailable::Protocol(
                            "session/create missing session.sessionId".to_string(),
                        ));
                        return;
                    };
                    let conn = self.app_conn.as_mut().expect("alive checked above");
                    match conn.send("session/subscribe", app_subscribe_params(&session_id)) {
                        Ok(sub_id) => {
                            self.app_session = Some(session_id);
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
                    // Subscribed: send the queued prompt and open the turn.
                    let connect = self.app_connect.take().expect("connect present");
                    let session_id = self.app_session.clone().expect("set on create");
                    // Apply a pre-selected permission mode (--mode, or /mode
                    // before the first prompt) to the fresh session first —
                    // requests are processed in order, so the prompt below
                    // already runs under it. Fresh sessions default to build.
                    if let Some(mode) = self.config.mode.clone() {
                        self.send_control(
                            "session/setMode",
                            app_set_mode_params(&session_id, &mode),
                            ControlReq::Command("/mode"),
                        );
                    }
                    let conn = self.app_conn.as_mut().expect("alive checked above");
                    match conn.send(
                        "session/send",
                        app_send_params(&session_id, &connect.prompt),
                    ) {
                        Ok(send_id) => self.begin_app_turn(send_id),
                        Err(err) => {
                            self.downgrade_app_server(err);
                            self.start_prompt_job_via_cli(&connect.prompt);
                        }
                    }
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
        if let Some(conn) = self.app_conn.as_mut() {
            while conn.poll().is_some() {}
        }
    }

    /// Retire the app-server path for the rest of this run and note it once.
    fn downgrade_app_server(&mut self, reason: AppServerUnavailable) {
        self.app_mode = AppMode::Downgraded;
        self.app_turn = None;
        self.app_connect = None;
        self.app_draining = None;
        self.app_session = None;
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
                        // ToolStarted shows only as a live chip; Reasoning/None
                        // need no transcript change.
                        TurnDelta::ToolStarted(_) | TurnDelta::Reasoning | TurnDelta::None => {}
                    }
                }
                AppServerMessage::StateUpdated(params) => {
                    if let Some(watermark) = app_state_watermark(&params) {
                        self.context_watermark = Some(watermark);
                    }
                    if let Some(controls) = app_state_controls(&params) {
                        self.merge_controls(controls);
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
                AppServerMessage::Response { id, .. } => {
                    // Successful control command: echo comes via state push.
                    self.control_requests.remove(&id);
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
        self.scroll = 0;
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
        if let Some(turn) = &mut self.app_turn {
            turn.text_index = None;
        }
        self.scroll = 0;
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
            if let AppServerMessage::StateUpdated(params) = message {
                if let Some(watermark) = app_state_watermark(&params) {
                    self.context_watermark = Some(watermark);
                }
                if app_state_is_turn_end(&params) || app_state_turn_error(&params).is_some() {
                    self.app_draining = None;
                    return;
                }
            }
        }
        if self
            .app_draining
            .is_some_and(|started| started.elapsed() > Duration::from_secs(10))
        {
            self.app_draining = None;
            self.app_session = None; // recreate → guaranteed clean next prompt
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
        if turn.cancel_requested {
            self.status = "cancelled".to_string();
            self.push_system("app-server turn cancelled");
        } else {
            self.status = format!("done ({elapsed:.1}s)");
        }
        // A turn landed in a live kernel session: keep continuity by reusing
        // the same sessionId for later prompts (already stored in app_session).
        self.session_active = true;
        self.scroll = 0;
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
                }
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
                AppServerMessage::Response { id, .. } => self.on_control_ok(id),
                AppServerMessage::Event(_) | AppServerMessage::Other => {}
            }
        }
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
        if !update.models.is_empty() {
            self.controls.models = update.models;
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
                if let ControlReq::Steer(content) = req {
                    self.queued.push_back(content);
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
            None => false,
        }
    }

    /// A control command succeeded; the substantive echo arrives via the
    /// state push. /compact gets a direct status (no push marks completion).
    fn on_control_ok(&mut self, id: u64) {
        if let Some(ControlReq::Command("/compact")) = self.control_requests.remove(&id) {
            self.status = "compacted".to_string();
        }
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
                    self.scroll = 0;
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

    /// /model — list the kernel-reported models for selection. Needs a live
    /// app-server session: the choices come from its state pushes.
    fn open_model_picker(&mut self) {
        if self.app_session.is_none() {
            self.push_system(
                "/model needs an active app-server session \
                 (ZCODE_TUI_APP_SERVER=1, complete one prompt first)",
            );
            return;
        }
        if self.controls.models.is_empty() {
            self.push_system("no models reported by the kernel yet (complete one prompt first)");
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
        let Some(session_id) = self.app_session.clone() else {
            return;
        };
        if let Some(choice) = self.controls.models.get(index).cloned() {
            self.push_system(&format!("model → {} ({})", choice.label, choice.provider));
            self.send_control(
                "session/setModel",
                app_set_model_params(&session_id, &choice.reference),
                ControlReq::Command("/model"),
            );
        }
    }

    /// /think — cycle to the next kernel-reported thought level.
    fn toggle_thought(&mut self) {
        let Some(session_id) = self.app_session.clone() else {
            self.push_system(
                "/think needs an active app-server session \
                 (ZCODE_TUI_APP_SERVER=1, complete one prompt first)",
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
    /// type while it streams). The input lands in the transcript marked as a
    /// steer; a failure requeues it via the control-error path.
    fn steer_turn(&mut self, content: &str) {
        let Some(session_id) = self.app_session.clone() else {
            self.queued.push_back(content.to_string());
            return;
        };
        self.push_user(content);
        self.push_system("↪ steering the running turn");
        self.scroll = 0;
        self.status = "steering (app-server)".to_string();
        self.send_control(
            "session/steer",
            app_steer_params(&session_id, content),
            ControlReq::Steer(content.to_string()),
        );
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
        let target = self
            .log
            .iter()
            .enumerate()
            .rev()
            .find(|(_, entry)| {
                foldable_kind(entry.kind)
                    && fold_preview(&entry.text, FOLD_THRESHOLD, FOLD_HEAD).is_some()
            })
            .map(|(index, _)| index);
        match target {
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
                self.scroll = 0;
            }
            None => self.status = "no long output to fold".to_string(),
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
        }
        let elapsed = active.started.elapsed().as_secs_f32();
        let (success, detail) = active
            .finished
            .unwrap_or((false, "job ended unexpectedly".to_string()));
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
        self.scroll = 0;
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
            "╭──────╮  Welcome to ZCODE! ({version})\n│██████│  ZhiPU terminal TUI   /help for shortcuts\n│   ██ │\n│  ██  │\n│ ██   │\n│██████│\n╰──────╯\n\ndirectory: {cwd}\nmode: {}   /mode to change\nsession: {session}   /new to reset\nauth: {}   /login to sign in",
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
            .position(|entry| matches!(entry.kind, LogKind::Banner))
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
        // Drop the app-server session so the next prompt creates a fresh one;
        // the connection itself is reused.
        self.app_session = None;
        self.clear_log();
        self.push_system("fresh session: context resets on the next prompt");
        self.status = "new session".to_string();
    }

    fn apply_startup_report(&mut self, report: StartupReport) {
        self.kernel_version = report.kernel;
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
        let logo_at = banner_pos.map(|pos| pos + 1).unwrap_or(self.log.len());
        // Unauthenticated startup already shows a Brand logo after the banner.
        // Reuse that slot for the update Logo, then insert only the Tip. Normal
        // configured startup has only the compact avatar in the banner, so the
        // update case inserts the big Logo + Tip here.
        let (shift_at, inserted) = if matches!(
            self.log.get(logo_at).map(|entry| entry.kind),
            Some(LogKind::Brand | LogKind::Logo)
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
        self.status = format!("update available: ZCode {}", feed.version);
        self.scroll = 0;
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
    Brand,
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
    mouse: bool,
}

impl TerminalGuard {
    fn enter(mouse: bool) -> Result<Self> {
        enable_raw_mode().context("failed to enable raw mode")?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            Hide
        )?;
        if mouse {
            execute!(io::stdout(), EnableMouseCapture)?;
        }
        let backend = CrosstermBackend::new(io::stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal, mouse })
    }

    fn draw(&mut self, state: &mut UiState) -> Result<()> {
        self.terminal.draw(|frame| render(frame, state))?;
        Ok(())
    }

    /// Leave the TUI, run `f` with the normal terminal, then restore.
    fn suspend<T>(&mut self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        if self.mouse {
            let _ = execute!(io::stdout(), DisableMouseCapture);
        }
        disable_raw_mode().context("failed to disable raw mode")?;
        execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableBracketedPaste,
            Show
        )
        .context("failed to leave alternate screen")?;
        let result = f();
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            Hide
        )
        .context("failed to re-enter alternate screen")?;
        if self.mouse {
            let _ = execute!(io::stdout(), EnableMouseCapture);
        }
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
        if self.mouse {
            let _ = execute!(self.terminal.backend_mut(), DisableMouseCapture);
        }
        let _ = execute!(
            self.terminal.backend_mut(),
            Show,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
    }
}

fn render(frame: &mut Frame<'_>, state: &mut UiState) {
    let root = frame.area();
    let input_lines = state.input.split('\n').count() as u16;
    let composer_height = input_lines.clamp(1, 5) + 2;
    let live_lines = live_panel_lines(state);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(6),
            Constraint::Length(live_lines.len() as u16),
            Constraint::Length(composer_height),
            Constraint::Length(1),
        ])
        .split(root);

    render_conversation(frame, vertical[0], state);
    if !live_lines.is_empty() {
        frame.render_widget(Paragraph::new(live_lines), vertical[1]);
    }
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
    if state.model_picker.is_some() {
        render_model_picker(frame, centered_rect(64, 46, root), state);
    }
    if state.history_query.is_some() {
        render_history_search(frame, centered_rect(72, 50, root), state);
    }
    // Topmost: the kernel is blocked on this answer.
    if state.interaction.is_some() {
        render_interaction(frame, centered_rect(74, 62, root), state);
    }
}

/// Max lines of forming assistant text shown in the run-only work panel.
const LIVE_TEXT_TAIL: usize = 4;

/// Run-only work panel above the composer: tool chips, the newest reasoning
/// line, and the tail of the assistant text as it forms — all gone the
/// moment the job finalizes (the authoritative reply lands in the transcript).
fn live_panel_lines(state: &UiState) -> Vec<Line<'static>> {
    let t = &state.theme;
    // App-server streaming turn: the answer streams straight into the
    // transcript, so the work panel only carries a live marker and the
    // newest reasoning, both gone when the turn finalizes.
    if let Some(turn) = &state.app_turn {
        let mut lines = Vec::new();
        let symbol = SPINNER_FRAMES[state.tick % SPINNER_FRAMES.len()];
        lines.push(Line::from(Span::styled(
            format!(" {symbol} streaming"),
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
                lines.push(Line::from(Span::styled(format!(" {clipped}"), t.dim())));
            }
        }
        return lines;
    }
    let Some(active) = &state.job else {
        return Vec::new();
    };
    let Some(live) = &active.live else {
        return Vec::new();
    };
    let mut lines = Vec::new();
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
        lines.push(Line::from(Span::styled(format!(" {reasoning}"), t.dim())));
    }
    // The answer forming, tail-first so the newest content stays in view.
    if let Some(text) = &live.text {
        let tail: Vec<&str> = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .rev()
            .take(LIVE_TEXT_TAIL)
            .collect();
        for line in tail.into_iter().rev() {
            let clipped: String = line.chars().take(120).collect();
            lines.push(Line::from(Span::styled(format!(" {clipped}"), t.text())));
        }
    }
    lines
}

fn render_conversation(frame: &mut Frame<'_>, area: Rect, state: &mut UiState) {
    let inner = Rect {
        x: area.x.saturating_add(1),
        y: area.y,
        width: area.width.saturating_sub(2),
        height: area.height,
    };
    let width = inner.width as usize;
    let graphics_logo = state.graphics_logo.is_some();
    // Item index where the reserved graphics-logo block starts, once placed.
    let mut logo_span: Option<u16> = None;
    let mut items: Vec<ListItem<'static>> = Vec::new();
    for (index, entry) in state.log.iter().enumerate() {
        if index > 0 {
            items.push(ListItem::new(Line::default()));
        }
        // With a graphics protocol the wordmark+skyline block is drawn as a
        // real image overlay; reserve blank rows here and remember where the
        // first such block sits so the image can be painted over it below.
        if graphics_logo
            && logo_span.is_none()
            && matches!(entry.kind, LogKind::Logo | LogKind::Brand)
        {
            logo_span = Some(items.len() as u16);
            for _ in 0..LOGO_IMG_ROWS {
                items.push(ListItem::new(Line::default()));
            }
            continue;
        }
        // Long mechanical output folds to a head preview unless expanded.
        if foldable_kind(entry.kind) && !state.unfolded.contains(&index) {
            if let Some((head, hidden)) = fold_preview(&entry.text, FOLD_THRESHOLD, FOLD_HEAD) {
                let preview_text = entry.text.lines().take(head).collect::<Vec<_>>().join("\n");
                let preview = LogLine::new(entry.kind, &preview_text);
                items.extend(log_to_items(
                    &preview,
                    width,
                    &state.theme,
                    state.skyline_mode,
                ));
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("  … (+{hidden} lines · Ctrl+O)"),
                    state.theme.dim(),
                ))));
                continue;
            }
        }
        items.extend(log_to_items(entry, width, &state.theme, state.skyline_mode));
    }
    let total = items.len() as u16;
    let max_scroll = total.saturating_sub(inner.height);
    if state.scroll > max_scroll {
        state.scroll = max_scroll;
    }
    // scroll counts lines back from the bottom; 0 follows the tail.
    let offset = max_scroll.saturating_sub(state.scroll);

    frame.render_stateful_widget(
        List::new(items),
        inner,
        &mut ListState::default().with_offset(offset as usize),
    );

    // Paint the true logo over its reserved rows, but only when the whole
    // block is scrolled fully into view (partial draws corrupt the protocol).
    if let Some(start) = logo_span {
        let end = start.saturating_add(LOGO_IMG_ROWS);
        if start >= offset && end <= offset.saturating_add(inner.height) {
            let img_w = LOGO_IMG_COLS.min(inner.width);
            let rect = Rect {
                x: inner.x + (inner.width.saturating_sub(img_w)) / 2,
                y: inner.y + (start - offset),
                width: img_w,
                height: LOGO_IMG_ROWS,
            };
            if let Some(protocol) = state.graphics_logo.as_mut() {
                frame.render_stateful_widget(StatefulImage::new(), rect, protocol);
            }
        }
    }
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

/// The welcome skyline + the width it centres to. Braille is a fixed 45-col
/// block; the wireframe is capped to a logo width (not full-panel) so both
/// centre under the wordmark. `outer` centres the whole logo in the panel;
/// `inner` centres the 45-col wordmark within a wider skyline.
fn skyline_layout(mode: SkylineMode, width: usize) -> (Vec<String>, usize, usize) {
    let sky = match mode {
        SkylineMode::Braille => skyline_braille(),
        SkylineMode::Wire => skyline_lines(width.min(70)),
        SkylineMode::None => Vec::new(),
    };
    let sky_w = sky.first().map(|row| row.width()).unwrap_or(0);
    let logo_w = sky_w.max(SKYLINE_LOGO_W);
    let outer = width.saturating_sub(logo_w) / 2;
    let inner = logo_w.saturating_sub(SKYLINE_LOGO_W) / 2;
    (sky, outer, inner)
}

fn log_to_items(
    entry: &LogLine,
    width: usize,
    theme: &Theme,
    mode: SkylineMode,
) -> Vec<ListItem<'static>> {
    let mut items: Vec<ListItem<'static>> = Vec::new();
    match entry.kind {
        LogKind::Banner => {
            // Codex-style rounded welcome box.
            let content: Vec<&str> = entry.text.lines().collect();
            let inner = content
                .iter()
                .map(|line| line.width())
                .max()
                .unwrap_or(0)
                .min(width.saturating_sub(4));
            items.push(ListItem::new(Line::from(Span::styled(
                format!("╭{}╮", "─".repeat(inner + 2)),
                theme.frame(),
            ))));
            for raw in &content {
                let mut spans = vec![Span::styled("│ ".to_string(), theme.frame())];
                spans.extend(banner_spans(raw, theme));
                let pad = inner.saturating_sub(raw.width());
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
            // Wordmark bright (GLM 蓝) with the skyline dim beneath it, together
            // as one centred logo block (wordmark inset within a wider skyline).
            let (sky, outer, inner) = skyline_layout(mode, width);
            for raw in entry.text.lines() {
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("{}{raw}", " ".repeat(outer + inner)),
                    theme.accent().bold(),
                ))));
            }
            for line in sky {
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("{}{line}", " ".repeat(outer)),
                    theme.dim(),
                ))));
            }
        }
        LogKind::Brand => {
            // Solid blocks carry the bright purple; the skyline beneath is the
            // shadow layer (清华紫 dim). Both centre together as one logo block.
            let (sky, outer, inner) = skyline_layout(mode, width);
            for raw in entry.text.lines() {
                let padded = format!("{}{raw}", " ".repeat(outer + inner));
                let mut spans = Vec::new();
                for (is_block, chunk) in chunk_by(&padded, |c| c == '█') {
                    let style = if is_block {
                        theme.brand().bold()
                    } else {
                        theme.brand_dim()
                    };
                    spans.push(Span::styled(chunk, style));
                }
                items.push(ListItem::new(Line::from(spans)));
            }
            for line in sky {
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("{}{line}", " ".repeat(outer)),
                    theme.brand_dim(),
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
            // Codex-style elevated band with a `›` prompt marker.
            items.push(ListItem::new(Line::default()).style(theme.band()));
            let content_width = width.saturating_sub(3).max(10);
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
            let content_width = width.saturating_sub(2).max(10);
            for (index, styled) in markdown_lines(&entry.text, content_width)
                .into_iter()
                .enumerate()
            {
                let mut spans = Vec::new();
                if index == 0 {
                    spans.push(Span::styled("• ".to_string(), theme.dim()));
                } else if styled.kind == MdLineKind::Quote {
                    spans.push(Span::styled("> ".to_string(), theme.good()));
                } else {
                    spans.push(Span::raw("  ".to_string()));
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
        }
        LogKind::Diff => {
            let content_width = width.saturating_sub(2).max(10);
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
                LogKind::Tool => ("• ", theme.accent_dim()),
                LogKind::Error => ("✗ ", theme.bad()),
                _ => ("• ", theme.dim()),
            };
            let content_width = width.saturating_sub(2).max(10);
            for (index, piece) in wrap_display(&entry.text, content_width)
                .into_iter()
                .enumerate()
            {
                let prefix = if index == 0 {
                    Span::styled(marker.to_string(), style)
                } else {
                    Span::raw("  ".to_string())
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

fn md_style(theme: &Theme, kind: MdLineKind, role: SpanRole) -> Style {
    match kind {
        MdLineKind::Heading => theme.text().bold(),
        MdLineKind::CodeBlock => {
            if role == SpanRole::Marker {
                theme.dim()
            } else {
                theme.code()
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
        lines.push(Line::from(vec![
            Span::styled(" › ".to_string(), t.accent().bold()),
            Span::styled(
                "describe a task   /commands   @files   !shell".to_string(),
                t.dim(),
            ),
        ]));
    } else {
        for (index, raw) in state.input.split('\n').enumerate() {
            let prefix = if index == 0 {
                Span::styled(" › ".to_string(), t.accent().bold())
            } else {
                Span::raw("   ".to_string())
            };
            lines.push(Line::from(vec![
                prefix,
                Span::styled(raw.to_string(), t.text()),
            ]));
        }
    }
    frame.render_widget(Paragraph::new(Text::from(lines)).style(t.band()), area);

    let mut line = 0usize;
    let mut column = 0usize;
    for ch in state.input.chars().take(state.cursor) {
        if ch == '\n' {
            line += 1;
            column = 0;
        } else {
            // Display columns, not chars: CJK characters occupy two cells.
            column += UnicodeWidthChar::width(ch).unwrap_or(0);
        }
    }
    let cursor_x = area
        .x
        .saturating_add(3)
        .saturating_add(column.min(area.width.saturating_sub(4) as usize) as u16);
    let cursor_y = area
        .y
        .saturating_add(1)
        .saturating_add(line.min(area.height.saturating_sub(2) as usize) as u16);
    frame.set_cursor_position((cursor_x, cursor_y));
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
    frame.render_widget(Paragraph::new(left), area);
    let mut right = String::new();
    if let Some((used, window)) = state.context_watermark {
        right.push_str(&format_context_watermark(used, window));
        if context_watermark_warn(used, window) {
            // /compact keeps the session (app-server), /new resets it.
            right.push_str(" · /compact or /new?");
        }
        right.push_str(" · ");
    }
    right.push_str(&format!("auth {} ", state.auth_label));
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(right, t.dim()))).alignment(Alignment::Right),
        area,
    );
}

fn render_suggestions(frame: &mut Frame<'_>, input_area: Rect, state: &UiState) {
    let t = &state.theme;
    let height = (state.suggestions.len() as u16).min(SUGGESTION_LIMIT as u16) + 2;
    let area = Rect {
        x: input_area.x,
        y: input_area.y.saturating_sub(height),
        width: input_area.width,
        height,
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

fn render_command_palette(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let t = &state.theme;
    let rows = if state.input.starts_with('/') {
        let suggestions = slash_suggestions(&state.input, 18);
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
