# zcode-tui

[Chinese](README.md) | [English](README.en.md) | [Releases](https://github.com/xhls008/zcode-tui/releases) | [Design](docs/2026-07-04-design.md)

![Current zcode-tui interface with ASCII branding, phased output, and the Agent Inspector](assets/zcode-tui-auenger.png)

> The v0.6.0 interface captured with ZCode CLI kernel 0.16.3 on macOS. It shows
> the adaptive ASCII brand panel, native terminal scrollback, phased app-server
> output, and the read-only parent Agent / Subagent / Background Inspector. The
> current source is also verified against ZCode Linux 3.8.1.

> **Unofficial notice**: `zcode-tui` is not an official ZCode / Zhipu project
> and is not endorsed by ZCode or Zhipu. It is a community/personal Linux
> terminal fallback for the TUI experience currently missing from the official
> package.

> **Usage benefit**: zcode-tui continues to use your official ZCode account and
> kernel rather than a separate billing path, so it inherits ZCode's **1.5x
> (150%) usage benefit**. Eligibility and exact terms remain subject to ZCode's
> official account policy.

`zcode-tui` is a Rust terminal-first fallback TUI for ZCode on Linux. It exists
for the gap where the official Linux package exposes a `tui` command, but does
not ship the terminal UI runtime (`@zcode/tui`).

It does not pretend to be an official implementation. It is a practical
terminal shell around the official ZCode CLI path: normal prompts prefer
`zcode app-server` and automatically fall back to `zcode --prompt`, while slash
commands, MCP config, shell escapes, session selection, phased output, command
palette, and editor workflows are handled locally.

## Current compatibility baseline

| Component | Current version / status |
|---|---|
| ZCode Linux x64 desktop | **3.8.1** (official feed) |
| Official CLI kernel | **0.16.3** (bundled with ZCode 3.8.1) |
| zcode-tui | **0.6.3** |
| Protocol compatibility | 3.8.1/3.7.7/3.7.6: runtime-preferences handshake + legacy body stream + V4 controls; 3.5.3: legacy + V4; 3.3.6: legacy controls |
| Current source verification | 144/144 Rust tests; zero-warning Clippy; native build; verified 3.8.1 app-server handshake and TUI lifecycle |

When the official x64 feed changes, startup update detection and `/update`
continue to use SHA-512 verification. Protocol compatibility is revalidated
against the real app-server/V4 behavior instead of being inferred from the CLI
version alone.

## Theme

The premise is simple: if the official Linux package does not provide a usable
terminal interface, build one.

The project is designed for SSH, tmux, headless servers, and keyboard-first
workflows. It does not try to recreate the desktop application. It focuses on
the part Linux terminal users need immediately: a responsive TUI that starts
fast, streams output, keeps session state visible, and can be installed without
a desktop environment.

## Features

The interaction model is influenced by Claude Code, Codex CLI, Gemini CLI,
OpenCode, and Crush. See [the research notes](docs/references/tui-research-2026-07.md)
for details.

**Visuals and rendering**

- Codex-like transcript layout: borderless scrollback, user message bands,
  assistant output as flat markdown, and a compact footer.
- Built-in `dark`, `light`, `tsinghua` (Tsinghua Purple), and `pku` (PKU Red)
  themes; `/theme` lists them and `/theme <name>` switches immediately and
  persists the choice. Dark keeps the Zhipu-inspired cool gray palette with
  GLM-blue accents.
- Markdown rendering via `pulldown-cmark`: headings, emphasis, inline code,
  fenced code blocks, lists, quotes, rules, and display-width aligned tables.
- Syntax highlighting for fenced code blocks via `syntect`; `diff` fences and
  `/diff` output are colored by line role.
- Startup banner with kernel/TUI versions, cwd, mode, session, and auth status.
- Optional official update check using the same Linux update feed as the
  desktop app.

**Phased streaming (default on)**

- Prompts run through the kernel's `zcode app-server` protocol and the answer
  accumulates from text deltas. A tool start commits the preceding text phase,
  a tool result appends when complete, and turn completion appends the final
  text phase. Rows already in terminal scrollback are never rewritten or
  reordered.
- On ZCode 3.5.3, the proven legacy create/resume/subscribe/send/event body
  stream is retained while `v4/conversation/subscribe` provides the new
  control plane. Older kernels that do not expose V4 keep their legacy
  controls without losing streaming.
- ZCode 3.7.6+ / CLI 0.16.3 adds a server-to-client
  `session/requestRuntimePreferences` request during session startup. The TUI
  answers it automatically instead of timing out after 15 seconds and falling
  back to the classic path.
- Any failure (spawn, handshake timeout, schema mismatch, disconnect)
  permanently and seamlessly downgrades the process to the classic
  `--prompt` path; set `ZCODE_TUI_APP_SERVER=0` to force the classic path.
- Tool permission approval: gated side-effect tools (Write in build mode,
  plan approval in plan mode) raise an approval overlay (arrows + Enter to
  answer, Esc to decline) instead of hanging; approved tools continue within
  the same turn.
- Session controls on the live session: `/model` picker, `/think` thought
  level, `/compact` in-place context compaction; `/mode` and Shift+Tab apply
  immediately via `session/setMode`.
- Dynamic model catalog: at startup, the TUI reads ZCode's resolved catalog
  through `workspace/readState`, merges matching Desktop metadata from
  `~/.zcode/v2/config.json` into the active CLI provider, and registers it with
  `workspace/updateProviderRegistry`. This exposes Desktop-provided entries such
  as `GLM-5.3-Flash` even when the standalone CLI catalog has not listed them.
  Authentication and endpoints still come from `~/.zcode/cli/config.json`;
  credentials are handed only in memory to the same local app-server, never
  logged, persisted, or used by the TUI to contact the provider directly.
  Public metadata is cached for outages. `/model` works before the first prompt,
  excludes inactive providers, and stores only `{providerId, modelId}` in
  `~/.config/zcode-tui/model.json` so the selection survives a restart and is
  applied to created or resumed sessions.
- Steering: on ZCode 3.5.3, plain text typed during a turn uses V4
  `setFollowupMode:guide` followed by `sendText`; success is shown only after
  the subsequent frame admits `delivery=guide`. Older kernels retain
  `session/steer`.
- `/rewind`: ZCode 3.5.3 lists V4 turn rows, previews a stable
  `{rowId,entityId}` target, and applies safe workspace file rewind through
  `v4/command applyFileRewind`. Older kernels retain the checkpoint-based
  legacy path; neither path overwrites externally modified unsafe files.
- Browser Use: `--browser-use headless` and optional `--browser-executable`
  are parsed explicitly and routed to the official classic `zcode --prompt`
  path, because the strict app-server schema does not accept these fields.
- The composer footer always shows parent-session `ctx used/window (%)` and
  cumulative `tok`. Context follows `state.updated`, cumulative tokens refresh
  silently after every turn, and `/usage [7d|30d]` remains immediately
  available while a response streams instead of entering the input queue.
- `/update` self-updates the kernel from the official feed (sha512-verified).
- ZCode 3.3.6 tool-policy flags (`--allowed-tools`, `--disallowed-tools`, and
  `--disallowedTools`) apply to both classic prompts and app-server
  create/resume sessions; `--permission-mode` is accepted as a legacy alias
  for `--mode` (`default` maps to `build`).
- `@file` mentions become `session/send` attachments (image/file kinds,
  `localPath`-based), so the model reads them on the streaming path too.
- Project `.mcp.json` and user-level MCP config are passed to
  `session/create`/`resume` as `mcpServers` — the kernel does not read
  project `.mcp.json` on its own, so this is what makes MCP servers work in
  streaming sessions.
- Turn comfort: a terminal bell after >30s turns (`notify = off` disables),
  a `N file(s) changed · /diff to review` note when a turn wrote files, and
  the current model + mode in the footer (e.g. `glm-5.1 · build`).

**Sessions**

- Multi-turn continuity: after the first successful prompt, later prompts
  reuse the live streaming session (classic path: `--continue`).
- `/new`, `/resume [sess_id]`, `/mode`, and Shift+Tab mode cycling.
- `/sessions` picker for recent kernel sessions, sorted with current-directory
  sessions first; streaming resumes go through `session/resume` with the
  runtime-model fix (bare resumes used to fail their first send).
- After a resume, the last few exchanges replay as dim compact lines so the
  restored context is visible.
- Discarded live sessions (`/new`, clean exit) get a best-effort
  `session/close`.

**Interaction**

- Rust + Ratatui + Crossterm, shipped as a single static Linux binary in
  GitHub Releases.
- CJK-aware transcript and composer wrapping; long input grows to five rows,
  then keeps the cursor visible in a scrolling composer viewport.
- Non-blocking streaming jobs for prompts, shell commands, and diffs.
- Esc/Ctrl+C cancellation with process-group kill on Unix.
- Busy input queueing.
- Live slash-command suggestions and `@file` completion.
- File mentions are translated into `--attach` after canonicalization, rejecting
  path traversal and symlink escapes.
- Persistent prompt history from the ZCode kernel database, plus Ctrl+R reverse
  search.
- `/agents` is a read-only Agent Inspector backed by official
  `session/subagents`, V4 state, and lifecycle events. It separates the parent,
  Subagents, and Background work; supports tabs, details, refresh, and stable
  selection; and always labels the composer target as the parent. Press `x`
  only on Background records that the kernel marks `cancellable=true` and gives
  a real `taskId`. Inactive child transcripts on ZCode 0.16.3 require a stateful
  resume, so the TUI deliberately shows official summaries/output tails only,
  never auto-resumes a child, and never falls back to SQLite.
- The TUI uses a normal-screen Ratatui inline viewport. Completed phases append
  to terminal scrollback in chronological order; only unfinished thinking
  state and the composer remain in the viewport. Mouse capture is
  never enabled, so wheel scrolling,
  ordinary drag selection, and system Cmd+C/Ctrl+Shift+C work natively. Sparse
  scrolling-region writes keep trailing spaces out of history so terminal
  reflow remains stable after a window resize.
- Structured summaries for internal tool calls; failures retain a bounded
  diagnostic tail, while explicit user-requested output remains complete.
- Bracketed paste support.
- Ctrl+P command palette, Ctrl+X leader shortcuts, Ctrl+G external editor.
- `/copy` (or Ctrl+X then y) copies the last assistant reply to the system
  clipboard via OSC52 — works over SSH; in tmux enable
  `set -g set-clipboard on`.

**Live progress**

- While a prompt is running, `zcode-tui` can read the ZCode kernel SQLite
  database in a read-only way and render live tool progress as compact status
  chips.
- If the database schema is missing or unsupported, this feature degrades
  silently and normal streaming still works.
- JSON prompt results are parsed when available; plain text remains supported.

**Auth**

- `/auth` distinguishes complete kernel configuration from partial env-key or
  credential-file setups.
- The unauthenticated startup screen gives browserless login paths for headless
  machines.
- `/login` runs `zcode login`; on headless machines it adds `--no-browser` so
  the OAuth URL is visible in the terminal.
- `/logout` runs `zcode logout`.

**MCP config**

- Project scope `.mcp.json` and user scope `~/.config/zcode/mcp.json`.
- stdio, http, and sse transports.
- `/mcp add`, `/mcp add-json`, `/mcp get`, `/mcp enable`, `/mcp disable`,
  `/mcp remove`.
- Runtime commands such as `/mcp status` are forwarded to ZCode.

## Install

### Option 1: Download the release binary

Recommended for SSH servers and machines without a Rust toolchain.

Each [GitHub Release](https://github.com/xhls008/zcode-tui/releases) contains:

| Platform | Release asset |
|---|---|
| Linux x86_64 | `zcode-tui-x86_64-unknown-linux-musl` (statically linked) |
| Windows x86_64 | `zcode-tui-x86_64-pc-windows-msvc.exe` |
| macOS Intel | `zcode-tui-x86_64-apple-darwin` |
| macOS Apple Silicon | `zcode-tui-aarch64-apple-darwin` |

Linux installation:

```bash
mkdir -p ~/.local/bin
curl -fL -o ~/.local/bin/zcode-tui \
  https://github.com/xhls008/zcode-tui/releases/latest/download/zcode-tui-x86_64-unknown-linux-musl
chmod +x ~/.local/bin/zcode-tui
```

If you also want the `zcode` wrapper:

```bash
curl -fLO https://github.com/xhls008/zcode-tui/releases/latest/download/install.sh
bash install.sh --no-build
```

On macOS, download the asset matching the machine architecture, save it as
`zcode-tui`, and run `chmod +x zcode-tui`. On Windows, download the `.exe`.
Both platforms need a `zcode` command that can invoke the official kernel on
`PATH`, or an explicit `ZCODE_TUI_ZCODE_BIN`. `install.sh` and the built-in
`/update` command remain Linux-only.

> **macOS auth and model configuration:** with ZCode 3.8.1 / CLI 0.16.3,
> upstream `zcode login` OAuth may still fail with
> `OAuth response is not valid JSON`; see the
> [upstream issue](https://github.com/zai-org/feedback/issues/51). Prefer
> `zcode login zai-coding-plan-api-key <key>` (global) or
> `zcode login bigmodel-coding-plan-api-key <key>` (China). Desktop sign-in is
> stored under `~/.zcode/v2/`, while the CLI still requires
> `~/.zcode/cli/config.json` with an explicit `provider/model`; desktop sign-in
> alone does not create that CLI model configuration.

### Option 2: Build from source

```bash
./install.sh
```

The installer builds the release binary, installs it to
`~/.local/bin/zcode-tui`, and installs a managed `~/.local/bin/zcode` wrapper.
The managed wrapper enables app-server true streaming for the fallback TUI by
default; use `ZCODE_TUI_APP_SERVER=0 zcode` to force the classic `--prompt`
path.

Manual build:

```bash
cargo build --release
./target/release/zcode-tui
```

Or point an existing wrapper at a custom fallback binary:

```bash
export ZCODE_FALLBACK_TUI="$PWD/target/release/zcode-tui"
zcode tui
```

## SSH and Headless Use

This is the primary use case. The TUI itself is a pure terminal application and
works over SSH and tmux. The only tricky part is bootstrapping the official
ZCode kernel.

1. Get the kernel without installing system-wide:

   ```bash
   dpkg-deb -x ZCode-<version>.deb ~/.local/opt/zcode/<version>/
   ```

2. Run the kernel. The wrapper probes `$ZCODE_APP`, `/opt/ZCode`, and
   `~/.local/opt/zcode/*/opt/ZCode`. Multiple rootless versions are compared
   numerically (`3.10` is newer than `3.9`), and the selected directory is
   exported to the fallback TUI as `ZCODE_APP`. It prefers Electron's embedded
   Node when possible, then falls back to system Node. The kernel requires
   Node >= 22.5 because it uses `node:sqlite`.

3. Log in. In headless environments, use one of:

   ```bash
   zcode login bigmodel-coding-plan-api-key <key>
   zcode login zai-coding-plan-api-key <key>
   zcode login --no-browser
   ```

   Or copy both files from a logged-in machine:

   ```text
   ~/.zcode/cli/config.json
   ~/.zcode/v2/credentials.json
   ```

## Common Commands

```text
text                         send through app-server (fallback: --prompt)
@<path>                      mention a file and auto-attach it
! <cmd>                      run a local shell command
/goal <text>                 forward goal handling to ZCode
/skill <name> <task>         force a ZCode skill
/skills [list]               run zcode skills list
/login                       interactive login
/logout                      logout
/auth                        show local auth status
/status                      show session, auth, and MCP overview
/sessions                    open recent session picker
/theme [list|dark|light|tsinghua|pku]
                             list or persistently switch built-in themes
/agents                      inspect parent, Subagents, and Background work
                             (read-only; Tab/Enter/r; x cancels eligible work)
/mcp list                    list project and user MCP servers
/mcp add <name> <cmd> [args] add stdio MCP server
/mcp add --transport http|sse <name> <url>
/mcp add-json <name> <json>
/mcp get <name>
/mcp enable|disable <name>
/mcp remove <name>
/mode [build|edit|plan|yolo]
/model
/think
/compact
/usage [7d|30d]
/update
/copy
/resume [sess_id]
/new
/diff [args]
/ide [path]
/editor
/clear
/exit
```

## Configuration

Config file:

```text
~/.config/zcode-tui/config
```

Line format:

```text
# Built-in palette and optional token overrides.
theme = dark
# Tokens: accent accent_dim text dim good bad frame code_bg band_bg
accent = #6088ff

# Disable the >30s turn-complete terminal bell.
notify = off
```

`NO_COLOR` and `--no-color` take precedence over theme colors.
You can also run `/theme dark|light|tsinghua|pku`; the command updates only the
`theme` line and preserves the rest of the file.

Useful environment variables:

```text
ZCODE_TUI_ZCODE_BIN
ZCODE_TUI_LOGIN_CMD
ZCODE_TUI_LOGOUT_CMD
ZCODE_TUI_IDE_CMD
ZCODE_TUI_NO_UPDATE_CHECK
ZCODE_TUI_UPDATE_FEED       (explicit latest-linux.yml URL or base; explicit
                             loopback URLs are supported for smoke tests)
ZCODE_TUI_APP_SERVER        (set 0/off/false to force the classic --prompt path)
ZCODE_TUI_LOG               (file path: append-only protocol debug log;
                             outbound entries are method names only — request
                             params, runtimeModel, and apiKey never touch disk)
ZCODE_TUI_SKYLINE           (ASCII ZCODE logo; off/none/0 disables it; hidden
                             automatically when the terminal is too small)
ZCODE_TUI_CONFIG
ZCODE_APP
ZCODE_FALLBACK_TUI
ZCODE_FORCE_SYSTEM_NODE
```

## Design and References

- [Design document](docs/2026-07-04-design.md)
- [TUI research notes](docs/references/tui-research-2026-07.md)
- [Agent TUI habits](docs/references/agent-tui-habits.md)
- [GitHub Releases](https://github.com/xhls008/zcode-tui/releases)

## Limitations

This fallback does not recreate the missing official `@zcode/tui` package, and
it does not have access to any private desktop-app runtime model. It does one
plain useful job: read terminal input, route local slash commands, call the
official CLI path, and render output.

## Background

The official Linux package currently makes terminal users do extra work: the CLI
help lists `tui`, but `zcode tui` can fail because the package does not include
`@zcode/tui`.

That gap is the reason this project exists.

The Chinese README keeps the sharper roast images and wording:
[README.md](README.md#背景与吐槽).

## Development

```bash
cargo fmt
cargo test
cargo clippy --all-targets --all-features
```

Thanks to [@tastypear](https://github.com/tastypear) for the resumed-session
model-config compatibility fix
([PR #1](https://github.com/xhls008/zcode-tui/pull/1)) and slash-command Enter
completion behavior ([PR #2](https://github.com/xhls008/zcode-tui/pull/2)); and
to [@auenger](https://github.com/auenger) for dynamic model discovery and
pre-session model selection
([PR #3](https://github.com/xhls008/zcode-tui/pull/3)), plus the native
scrollback, phased output, and layout refactor
([PR #4](https://github.com/xhls008/zcode-tui/pull/4)), and for continuing as
a project collaborator in v0.6.1 with the Agent Inspector, background-task
cancellation, GLM-5.3-Flash support, context status, and startup-layout work.
Thank you for the continued support of the project.

## License

MIT
