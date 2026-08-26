# Tasks: feat-background-task-cancel

- [ ] Pin request/response shapes for `session/cancelBackgroundTask`.
- [ ] Add cancellability and exact task-ID eligibility to Inspector actions.
- [ ] Correlate in-flight cancellation responses without optimistic terminal-state mutation.
- [ ] Handle completion races, duplicate keys, method-not-found, and request errors safely.
- [ ] Verify cancellation isolation from the parent turn and other tasks.

## Progress log

- 2026-08-26: Initialized as the independently deliverable cancellation slice of PR 4.
