# adapt-zcode-3-3-6

## Why

ZCode desktop 3.3.6 keeps CLI version `0.15.2`, but its bundle and packaging
changed in ways the TUI cannot identify from the CLI version alone:

- `session/create` and `session/resume` gained strict optional
  `toolAllowlist[]` / `toolDenylist[]` fields.
- the Linux package used for this compatibility run carries an
  `app-update.yml` pointing at `http://localhost:8081`, while the TUI promises
  the official update channel.
- rootless installs under `~/.local/opt/zcode/<version>/` are selected by the
  wrapper but are invisible to the TUI's startup probe and `/update`; the
  wrapper also compares directory names lexically rather than by version.

Without an adaptation, tool-policy flags are ignored on the default
app-server path and rootless users cannot reliably inspect or update the
kernel they are actually running.

## What Changes

- Discover the active ZCode application directory from `ZCODE_APP`,
  `/opt/ZCode`, or versioned rootless installs, using numeric version order.
- Export the wrapper's resolved application directory to the fallback TUI and
  use the same directory for installed-version and update-feed resolution.
- Add an explicit `ZCODE_TUI_UPDATE_FEED` override. Treat an implicit loopback
  package feed as a development placeholder and fall back to the documented
  official Linux feed; explicit overrides remain usable for local smoke tests.
- Parse `--allowed-tools`, `--disallowed-tools` / `--disallowedTools`, and the
  legacy `--permission-mode` alias. Carry tool policies through both classic
  `--prompt` arguments and app-server create/resume params.
- Record the 3.3.6 help/parser drift: `--settings` and `--max-turns` are
  advertised but rejected by the shipped CLI and have no app-server schema;
  this change does not invent semantics for them.

## Impact

- Affected modules: `install.sh`, `src/lib.rs`, `src/main.rs`, `tests/core.rs`,
  `tests/pty_smoke.py`, README and changelog.
- Affected users: Linux deb installs and rootless extracted-kernel users,
  especially callers using tool allow/deny policy flags.
- Subprocess lifecycle is unchanged: the wrapper still `exec`s the selected
  kernel/TUI; app-server remains process-group owned and killed on drop.

## Out Of Scope

- Implementing `--settings` or `--max-turns` before the kernel accepts them or
  exposes equivalent protocol fields.
- Replacing the wrapper with a cross-platform launcher.
- Adding new ZCode protocol methods or emulating kernel permissions locally.
