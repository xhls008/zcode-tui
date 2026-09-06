# Tasks: feat-zcode-3112-compat

- [x] Define scope and review compatibility risks
- [x] Verify official package, CLI changes and TUI availability
- [x] Implement minimal confirmed compatibility fixes and regression coverage
- [x] Run real isolated protocol probes and deterministic PTY tests
- [x] Update compatibility documentation and run all quality gates
- [x] Persist verification evidence and validate the completion gate

## Progress log
- 2026-09-05: Created isolated feature worktree; existing main worktree's
  untracked user files are untouched. Versioned official feed obtained.
- 2026-09-05: Verified desktop 3.11.2-6792 / CLI 0.16.5. Actual `tui` invocation
  fails because `@zcode/tui` is absent. Help is unchanged; one V4 method added.
- 2026-09-05: Live old/new comparison exposed early `prompt_completed` and
  stop-acceptance races. Use real turn lifecycle events and V4 foreground stop;
  keep the cancellation barrier until its terminal event, not the stop ack.
- 2026-09-05: 162 offline Rust tests, three explicit live kernel runs (3.11.2,
  3.9.1, 3.8.1), 11 deterministic PTY checks and real 3.11.2 PTY first/second/
  cancel/continue checks passed. All quality gates passed.
- 2026-09-06: User authorized latest-kernel adaptation and Release publication.
  Rechecked the official website and versioned feed: latest remains 3.11.2.
  Preparing v0.6.8 with repeat verification, merge, archive and tag publication;
  installed ZCode and zcode-tui binaries remain untouched.
- 2026-09-06: Rebased onto remote main `90f27722`, preserving its three
  unpublished changes and authoritative response recovery. Resolved overlap in
  admission handling without a duplicate terminal-event branch. Repeated all
  checks: 172 offline tests, three live-kernel runs, 11 PTY checks, real-kernel
  PTY cancellation/continuation, v0.6.8 wrapper routing and quality gates passed.
