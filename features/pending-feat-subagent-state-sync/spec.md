# Feature: feat-subagent-state-sync Reliable official Subagent state

## Basic information
- ID: feat-subagent-state-sync
- Priority: 80
- Workflow mode: deep
- Risk signals: public_api, external_dependency, data_consistency, compatibility, parallel_split
- Dependencies: feat-tui-module-boundaries
- Plan phase: PR 3

## User outcome
Users see a reliable Subagent and background-work list after fresh sessions, resume, picker resume, and live updates, without Bash work being mislabeled as a Subagent.

## Scope and constraints

- Query `session/subagents` after create/resume and on explicit refresh/open.
- Parse V4 `subagents` and `backgroundWorks` snapshots and incremental state.
- Complete lifecycle decoding for `background_task_*` and `subagent_*` events.
- Model `taskId`, `childSessionId`, `agentId`, and `toolCallId` as distinct identifiers.
- Reconcile sources with revision-aware ordering and terminal-state precedence.
- Separate Bash background work from Subagent records.
- Degrade safely when official methods or V4 fields are unavailable; do not read SQLite as a fallback.

## Acceptance scenarios

1. Fresh, direct resume, and session-picker resume initialize the same authoritative Subagent list.
2. Bash and Subagent background work remain distinct even when identifiers arrive incrementally.
3. Out-of-order running updates cannot overwrite success, failed, or cancelled terminal states.
4. Opening `/agents` refreshes state without high-frequency polling.
5. Method-not-found and older-kernel payloads retain existing TUI functionality with a clear unavailable state.

## Technical notes

Use `session/subagents` as the persistent source, V4 as the current-session snapshot, and lifecycle events for low-latency updates. Record enough provenance to make merge decisions deterministic.
