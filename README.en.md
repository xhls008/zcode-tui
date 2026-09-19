# zcode-tui

[中文](README.md) · [Releases](https://github.com/xhls008/zcode-tui/releases) · [Design](docs/2026-07-04-design.md)

![zcode-tui](assets/zcode-tui-auenger.png)

> An unofficial community TUI for ZCode Linux installations that do not ship a
> usable `@zcode/tui`. It runs the official CLI kernel; it is not the Desktop app.

`zcode-tui` is a Rust/Ratatui terminal interface for SSH, tmux, headless servers,
and keyboard-first workflows. Prompts use the official `app-server` stream when
available and fall back to `zcode --prompt` when the protocol is unavailable.

## Quick start

### Download a Linux release

```bash
mkdir -p ~/.local/bin
curl -fL -o ~/.local/bin/zcode-tui \
  https://github.com/xhls008/zcode-tui/releases/latest/download/zcode-tui-x86_64-unknown-linux-musl
chmod +x ~/.local/bin/zcode-tui

# Optional: install the managed `zcode` wrapper too.
curl -fLO https://github.com/xhls008/zcode-tui/releases/latest/download/install.sh
bash install.sh --no-build
```

### Build from source

```bash
./install.sh                 # build and install TUI + wrapper
./install.sh --no-wrapper
./install.sh --uninstall
```

Log in with `zcode login`, or run `/login` inside the TUI. Headless machines need
Node ≥ 22.5 unless the official Electron runtime is usable. See
[Browser Use and runtime notes](docs/browser-use.md) for the longer SSH guide.

## Why this exists

The official Linux package once advertised a `tui` entry without shipping a usable
`@zcode/tui`. This project supplies a terminal shell for SSH/tmux/headless use; it
does not claim to reproduce every Desktop capability.

![ZCode launched. Where is the TUI?](assets/zcode-no-tui-satire.png)

*Product satire: the menu advertises a TUI, while the package is missing the entry point.*

![A menu promises beef noodles and serves hot water](assets/beef-noodle-hot-water-satire.png)

*Product satire: users should not have to finish the product integration themselves.*

## Privacy boundary

### What this project does

- The managed path runs `resources/glm/zcode.cjs`; it does not start the Desktop
  host at `app.asar/out/host/index.js`.
- The audited official CLI kernels (3.11.2 and 3.12.3) did not contain the
  Desktop repository-snapshot uploader.
- `check-zhipu-upload` is local and read-only: no kernel, network, credentials,
  decryption, extraction, deletion, or upload is triggered.

### What it does not guarantee

- Model requests, attachments, tools, MCP servers, plugins, and Browser Use may
  still send content over the network.
- This is not an OS-level filesystem/network sandbox and cannot control a
  separately running Desktop app.
- No local evidence does **not** mean nothing was ever uploaded; a client success
  state is not an independent server receipt.

### Privacy criticism of the official design

For the audited historical Desktop packages, the host triggered a repository
snapshot flow after accepted session input: it enumerated workspace files and
some `.git` metadata, archived and encrypted them, then uploaded the result to
a server-provided object-storage target. The data key was wrapped with a
server-held public-key workflow; the user did not hold the corresponding private
key. Therefore, “encrypted” did not mean “user-controlled” or “decryptable only
by the user.” The audited flow also did not present clear per-upload consent,
and similarly named indexing/experience settings were not reliable opt-out gates.

This criticism concerns transparency, defaults, and user control in those fixed
artifacts. It does not claim that every user uploaded data or make a legal
determination. The official team later apologized publicly and said it had fixed
the issue, would open-source the component, and add third-party review; the
scope of those changes and current server behavior still require version-specific
verification ([audit](docs/privacy-audit-2026-09-18.md)).

### New privacy-monitoring feature

This project adds `check-zhipu-upload` to inspect upload-related evidence left in
the official local data directories:

- Read-only and offline: it does not start the kernel, read workspace contents,
  obtain credentials, decrypt, or delete files.
- Checks accepted manifests, attempt/failure counters, staged or encrypted
  remnants, including the 3.12.3 upload-state fields.
- Emits five stable statuses—`client_reported_accepted`, `attempt_evidence`,
  `staged_evidence`, `no_local_evidence`, and `inconclusive`—with `--json` for
  automation.
- Reports read errors, limits, and incomplete coverage. `no_local_evidence` only
  means no evidence was found locally; it is not proof that no upload occurred or
  a server receipt. Stop the official Desktop app before checking when possible.

```bash
zcode check-zhipu-upload
zcode-tui check-zhipu-upload --json
# Inside the TUI: /check-zhipu-upload
```

See [the checker documentation](docs/check-zhipu-upload.md) for scope and status
semantics, and [the audit](docs/privacy-audit-2026-09-18.md) for fixed-package evidence.

**Position:** undisclosed, unauthorized background repository collection is not
acceptable; encryption alone does not give users control. This criticizes the
audited historical artifacts and does not claim that every user uploaded data or
that a legal violation occurred.

![Satirical comic: the cloud robot encrypts the user's Git history but keeps the key.](output/imagegen/privacy-comic.png)

*An original adaptation of [ferstar's public commentary](https://blog.ferstar.org/posts/zcode-silent-workspace-snapshot-upload/);
the dialogue is fictional and is not evidence of current server behavior.*

## Compatibility

| Component | Status |
|---|---|
| Official Desktop packages | 3.12.3-7463 and 3.11.2-6792 audited |
| 3.12.3 | Classic CLI create/resume; no app-server/V4/in-session model picker yet |
| 3.11.2 | app-server streaming, V4 controls, cancellation, and resume verified |
| Browser Use | Classic official CLI route on 3.11.2/3.12.3; no in-turn steer/interactive approval |
| Release | `zcode-tui 0.7.0` |

Equal CLI version strings do not imply equal protocols. Release assets include
Linux, Windows, macOS Intel, Apple Silicon binaries, and `SHA256SUMS`.

## Features

- Multi-turn sessions, `/new`, `/resume`, `/sessions`, permission modes, and
  context/token status.
- App-server streaming, tool approval, and automatic fallback to classic CLI.
- MCP at project/user scope (stdio/http/sse), `@file` attachments, shell escape,
  and syntax-highlighted git diff.
- Read-only `/agents` inspector, background-task cancellation, and `/rewind`.
- Markdown/code rendering, CJK-aware layout, themes/custom themes, OSC52 copy,
  and persistent input history.
- Headless Browser Use with a matching official Playwright runtime bridge; see
  [Browser Use docs](docs/browser-use.md).

## Common commands

| Command | Purpose |
|---|---|
| `text` | Send a prompt; stream first, then fall back if needed |
| `@path` | Attach a workspace file; traversal and symlink escapes are rejected |
| `! <cmd>` | Run a local shell command |
| `/login`, `/logout`, `/auth` | Authentication actions/status |
| `/status` | Session, auth, and MCP summary |
| `/mcp ...` | Manage MCP servers |
| `/mode [build\|edit\|plan\|yolo]` | View/change permission mode |
| `/model`, `/think`, `/compact` | Live session controls (not on 3.12.3 classic route) |
| `/usage [7d\|30d]` | Token usage |
| `/agents` | Read-only Agent/Background inspector |
| `/rewind` | Preview and apply a checkpoint rewind |
| `/diff [args]` | Highlight `git diff` |
| `/theme [name]` | List or switch themes |
| `/update` | SHA-512-verified official kernel update |
| `/check-zhipu-upload` | Local upload-evidence check |
| `/copy`, `/editor`, `/clear`, `/new`, `/exit` | Copy, edit, clear, reset, quit |

Run `zcode-tui --help` for the complete command and shortcut list.

## Configuration

File: `~/.config/zcode-tui/config` (`key = value`):

```text
theme = dark
notify = off
accent = #6088ff
```

| Variable | Purpose |
|---|---|
| `ZCODE_TUI_ZCODE_BIN` | Official `zcode` path |
| `ZCODE_APP` | Official Desktop package directory |
| `ZCODE_FALLBACK_TUI` | Fallback TUI binary |
| `ZCODE_TUI_APP_SERVER=0` | Force classic `--prompt` mode |
| `ZCODE_FORCE_SYSTEM_NODE=1` | Skip Electron's embedded Node |
| `ZCODE_TUI_NO_UPDATE_CHECK=1` | Disable startup update check |
| `ZCODE_TUI_CONFIG` | TUI config path |
| `ZCODE_TUI_LOG` | Structural protocol log; request params/credentials are omitted |

For theme syntax, wrapper behavior, and build/release gates see
[BUILDING.md](BUILDING.md) and the [design document](docs/2026-07-04-design.md).

## Limitations

This project is a terminal shell and compatibility layer, not a recreation of
the Desktop app or the missing official `@zcode/tui`. Tool approval is not an OS
sandbox; restrict workspace, network egress, MCP, and plugin access for sensitive projects.

## Development

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo build --release --locked
```

Real-kernel and Browser Use checks use temporary homes, synthetic data, and
loopback services; see `BUILDING.md`.

## License

MIT
