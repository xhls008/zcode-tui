# Tasks: feat-subagent-state-sync

- [x] Pin and test official request/response and V4 payload shapes.
- [x] Expand agents/background-work models with distinct protocol identifiers and provenance.
- [x] Implement create/resume/open refresh requests and backward-compatible fallback.
- [x] Implement revision-aware, terminal-state-first reconciliation across all three sources.
- [x] Test fresh, resume, picker, out-of-order, Bash/Subagent separation, and old-kernel scenarios.

## Progress log

- 2026-08-26: Initialized from PR 3 of the Subagent TUI refactor plan.
- 2026-08-26: Added optional `session/subagents` refresh after create/resume and on `/agents` open.
- 2026-08-26: Unified official snapshots, V4 state, and lifecycle events with distinct identity fields and terminal-state precedence.
- 2026-08-26: Passed fmt, clippy, 125 tests, and release build locally.
