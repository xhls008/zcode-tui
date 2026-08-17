# adapt-zcode-3-7-6

## Why

ZCode desktop `3.7.6` introduced CLI kernel `0.16.3`, retained by the current
`3.7.7` feed. The Linux package still omits `@zcode/tui`, but the app-server
handshake gained a required server-to-client request:
`session/requestRuntimePreferences`.

The current TUI decodes the request but leaves unknown server requests
unanswered. `session/create` therefore waits 15 seconds, returns
`Client request timed out`, and forces every prompt onto the classic CLI path.

## What Changes

- Reply to `session/requestRuntimePreferences` during create/resume and normal
  connection pumping.
- Use the 0.16.3 compatibility defaults that the kernel itself applies for
  clients returning Method not found: memory off, native search enhancements
  on, automatic user-question resolution on, and `preflight-v1` context budget.
- Preserve the existing interaction approval path and all 0.15.x behavior.
- Revalidate legacy streaming plus V4 subscribe against the official 3.7.6 and
  installed 3.7.7 packages.

## Impact

- Affected modules: `src/lib.rs`, `src/main.rs`, `tests/core.rs`, protocol docs,
  README, and changelog.
- Affected users: anyone updating the official Linux package to ZCode 3.7.6+.
- Subprocess ownership and process-group cancellation are unchanged.

## Out Of Scope

- Implementing desktop memory settings, integrated-terminal selection, or a
  settings UI.
- Adding plugin management, automation, `/expert`, or `/fork` merely because
  0.16.3 exposes them.
- Mapping `--settings` or `--max-turns` into strict app-server schemas without
  a verified protocol field; classic CLI passthrough remains available.
