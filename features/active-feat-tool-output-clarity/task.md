# Tasks: feat-tool-output-clarity

- [x] Inventory and remove all folding state, rendering, input handling, docs, and tests.
- [x] Introduce explicit output-source classification and structured summary projection.
- [x] Add bounded failure diagnostics for unsuccessful tools.
- [x] Preserve and test user-requested full-output paths and terminal behavior.
- [x] Run formatting, Clippy, tests, release build, and terminal-behavior regression checks.

## Progress log

- 2026-08-26: Initialized from PR 1 of the Subagent TUI refactor plan.
- 2026-08-26: Removed Ctrl+O folding, added deterministic tool summaries and bounded failure tails, and updated current documentation and PTY expectations.
