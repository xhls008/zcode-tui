# Tasks: feat-background-task-cancel

- [x] Pin request/response shapes for `session/cancelBackgroundTask`.
- [x] Add cancellability and exact task-ID eligibility to Inspector actions.
- [x] Correlate in-flight cancellation responses without optimistic terminal-state mutation.
- [x] Handle completion races, duplicate keys, method-not-found, and request errors safely.
- [x] Verify cancellation isolation from the parent turn and other tasks.

## Progress log

- 2026-08-26: Initialized as the independently deliverable cancellation slice of PR 4.
- 2026-08-26: Live-probed ZCode 0.16.3 strict `{sessionId, taskId}` schema and `background_task_not_found` outcome.
- 2026-08-26: Added exact-ID eligibility, pending correlation, terminal-race convergence, safe response/error handling, and Inspector action hints.
- 2026-08-26: Passed strict linting, 130 tests, and release build; background cancellation never routes through `session/stop`.
