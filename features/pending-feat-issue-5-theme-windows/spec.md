# Feature: feat-issue-5-theme-windows Theme switching and Windows session compatibility

## Basic information
- ID: feat-issue-5-theme-windows
- Priority: 80
- Workflow mode: deep
- Risk signals: multi_module, compatibility
- Dependencies: none
- Created: 2026-08-28T14:09:44Z

## User outcome
Users can persistently switch dark/light themes, use a more defensive sessions picker on Windows terminals, and read v0.6.2 documentation for inherited 150% usage benefits.

## Scope and constraints

- Add a local `/theme [list|dark|light]` command using the existing slash
  command catalog and line-based UI config.
- Apply the selected palette immediately and persist only the `theme` setting,
  preserving color-token overrides, comments, unknown settings, and CRLF/LF
  style in the existing config file.
- Resolve the user home from `HOME`, falling back to Windows `USERPROFILE`, so
  config and kernel DB lookup work from cmd, PowerShell, and Windows Terminal.
- Make `/sessions` retry the read-only kernel DB probe when opened, normalize
  Windows path tails and embedded line breaks, and align/truncate rows by
  Unicode display width.
- Fall back to a safe terminal size if Crossterm cannot report one.
- Keep all Windows behavior best-effort: no claim that this removes upstream
  ZCode app-server constraints, and no Windows-only integration environment is
  available in this Linux workspace.
- Update Chinese and English README documentation, version metadata, and the
  changelog for v0.6.2. Do not touch or stage `.claude/`, `package.json`, or
  `tests/test.py`.

## Acceptance scenarios

1. `/theme` and `/theme list` show `dark` and `light` and identify the current
   choice without forwarding a prompt to ZCode.
2. `/theme light` and `/theme dark` update the live palette and persist across
   restart through the existing config file; invalid names do not change it.
3. Existing color overrides still apply on top of either built-in palette,
   and `NO_COLOR`/`--no-color` remain authoritative.
4. With `HOME` absent and `USERPROFILE` present, config and kernel DB paths
   resolve below the Windows profile.
5. Opening `/sessions` before the asynchronous startup probe completes retries
   the DB check; Windows-style directory fallbacks and CJK/CRLF titles render
   as bounded, single-line rows.
6. Failure to read terminal dimensions no longer aborts TUI initialization.
7. README files document `/theme` and inherited ZCode 150% usage benefits;
   CHANGELOG records Windows `/sessions` compatibility as best-effort.
8. `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets -- -D warnings`,
   and `cargo build` pass.

## Technical notes

- Reuse `UiConfig`, `Theme`, `command_catalog`, and `handle_local`; add no
  dependency or standalone theme subsystem.
- Keep the current Zhipu palette as `dark`; add one light palette.
- Use the existing read-only SQLite probe and app-server `session/list` path.
