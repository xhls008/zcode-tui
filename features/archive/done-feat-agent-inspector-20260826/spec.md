# Feature: feat-agent-inspector Read-only Agent Inspector

## Basic information
- ID: feat-agent-inspector
- Priority: 70
- Workflow mode: deep
- Risk signals: multi_module, compatibility, parallel_split
- Dependencies: feat-subagent-state-sync
- Plan phase: PR 4A

## User outcome
Users can inspect parent, Subagent, and Bash/background-work status and available details in a dedicated interface while the composer unambiguously continues to target the parent Agent.

## Scope and constraints

- Replace the current lifecycle-only `/agents` overlay with Agents and Background views backed by the reconciled model.
- Support list/detail navigation, refresh, stable selection, and detail scrolling.
- Show status, title/task, summary, identifiers, linked background work, and output tail when officially available.
- Always display `viewing: … · read-only` and `input target: parent` in detail mode.
- Do not implement directed child messages or silently resume child sessions.
- Preserve terminal-native selection and copy shortcuts.

## Acceptance scenarios

1. `/agents` distinguishes the parent, Subagents, and Bash/background work.
2. Live updates preserve the selected record and detail scroll position whenever that record still exists.
3. Refresh uses official state sources and presents an actionable error without closing the parent session.
4. The UI never suggests that typed input targets the selected child.
5. System copy and terminal selection continue to work while the Inspector is open.

## Technical notes

Keep rendering read-only and side-effect free. Navigation state belongs to the Inspector model; protocol requests remain in the app/update boundary.
