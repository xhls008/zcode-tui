# Geek TUI Habits Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `zcode-tui` feel closer to established coding-agent TUIs while keeping it a Rust fallback wrapper for ZCode Linux.

**Architecture:** Keep ZCode process execution in `src/lib.rs` pure enough to test. Keep terminal rendering and keyboard handling in `src/main.rs`. Store external product references under `docs/references/`.

**Tech Stack:** Rust 2021, Ratatui, Crossterm, Serde, shell-words, cargo test/clippy.

---

### Task 1: Local Reference Docs

**Files:**
- Create: `docs/references/agent-tui-habits.md`
- Create: `docs/references/raw/*.html`
- Create: `docs/references/raw/claude-code-llms-full.txt`

- [x] Download official Claude Code interactive mode, slash commands, settings, and LLM index.
- [x] Download official OpenCode TUI and keybind pages.
- [x] Summarize local design decisions derived from those docs.

### Task 2: Test New Interaction Logic

**Files:**
- Modify: `tests/core.rs`
- Modify: `src/lib.rs`

- [x] Add failing tests for `! <cmd>` shell classification.
- [x] Add failing tests for slash command suggestions.
- [x] Add failing tests for command palette rows.
- [x] Add failing tests for leader key action mapping.
- [x] Implement the minimal library code to pass these tests.
- [x] Run: `cargo test`

### Task 3: TUI Interaction Upgrade

**Files:**
- Modify: `src/main.rs`
- Modify: `src/lib.rs`

- [x] Add `Ctrl+P` command palette overlay.
- [x] Add `Ctrl+X` leader mode with `p/h/e/x/u/q` actions.
- [x] Add `Tab` slash completion and suggestions.
- [x] Add `! <cmd>` shell execution with output rendered as system text.
- [x] Add `Ctrl+G` and `/editor` editor handoff through `$VISUAL` or `$EDITOR`.
- [x] Add `Ctrl+J` multiline prompt insertion.
- [x] Run: `cargo test`

### Task 4: Geek Visual Pass

**Files:**
- Modify: `src/main.rs`
- Modify: `README.md`

- [x] Replace the soft boxed header with a dense terminal instrument panel.
- [x] Add route/status hints for prompt, local, shell, MCP, and leader modes.
- [x] Update README shortcuts and local reference notes.
- [x] Run: `cargo fmt --check`
- [x] Run: `cargo clippy --all-targets --all-features -- -D warnings`
- [x] Run: `cargo build --release`

### Task 5: Publish Public Code Page

**Files:**
- Git repository metadata only.

- [ ] Confirm no token or secret strings are tracked.
- [ ] Commit the changes.
- [ ] Push `main` to `https://github.com/xhls008/zcode-tui`.
- [ ] Verify public access with unauthenticated GitHub API or raw README fetch.
