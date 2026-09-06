# Feature: feat-zcode-3112-compat ZCode 3.11.2 CLI compatibility and official TUI verification

## Basic information
- ID: feat-zcode-3112-compat
- Priority: 50
- Workflow mode: deep
- Risk signals: compatibility, external_dependency
- Dependencies: none
- Created: 2026-09-05T13:55:23Z

## User outcome
Verify the latest official Linux CLI/TUI and preserve zcode-tui session and streaming compatibility with minimal changes.

## Scope and constraints
- Inspect the SHA-512-verified official 3.11.2 Linux x64 package in `/tmp`.
- Compare public CLI help, app-server methods and relevant strict schemas with
  installed CLI 0.16.5. Change only confirmed incompatibilities.
- Verify official TUI by package resolution and an actual isolated invocation;
  a help entry alone is not support. Preserve existing official-first routing.
- Do not change `/opt/ZCode`, user credentials, or installed binaries. Do not
  push, merge, tag or delete the isolated feature worktree without authorization.
- Run live protocol tests with temporary HOME/config/workspace and no paid model
  calls; use a local fake provider if a generated turn is needed.

## Acceptance scenarios
1. Package size/SHA-512, desktop build and CLI version are recorded.
2. Official TUI availability is established with reproducible evidence.
3. Current and 0.16.5 kernels accept client runtime preferences, create/read/
   subscribe/close sessions, and relevant V4 requests without hanging.
4. Confirmed protocol changes have focused regression coverage; existing Rust
   and deterministic PTY tests pass, including legacy fallback.
5. Formatting, Clippy, release build, installer syntax and diff checks pass.
6. Documentation distinguishes desktop and CLI versions, compatibility scope,
   official TUI availability and the stale unversioned update channel.

## Technical notes
- Prior evidence: `done-feat-zcode-cli-0165-compat-20260831`.
- Website links to versioned `3.11.2/linux-x64/latest.yml`; the unversioned feed
  still returns 3.9.1. Do not weaken SHA-512 verification to work around it.
