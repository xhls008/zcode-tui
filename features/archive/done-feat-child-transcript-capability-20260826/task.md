# Tasks: feat-child-transcript-capability

- [x] Record the target kernel version and official candidate request shapes.
- [x] Exercise active, inactive, and ended child-session read paths with protocol evidence.
- [x] Measure whether resume mutates parent relation, state, or input routing.
- [x] Write a supported/unsupported decision and compatibility behavior.
- [x] If supported, create a separately scoped follow-up Feature before adding transcript detail (not applicable: stateful-resume-only path is unsupported).

## Progress log

- 2026-08-26: Initialized from the plan's deferred child-session transcript verification section.
- 2026-08-26: Probed disposable running and ended Explore children against ZCode 0.16.3 using only public app-server RPCs.
- 2026-08-26: Direct messages/events reads returned `-32004 Session is not active`; resume enabled reads but activated an input-capable child (`startNow`).
- 2026-08-26: Documented the unsupported decision, pinned sanitized fixtures, corrected the official 0.16.3 subagent parser, and kept summary-only Inspector behavior.
- 2026-08-26: Passed strict linting, 128 tests, and release build.
