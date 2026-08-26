# Feature: feat-child-transcript-capability Child session transcript capability decision

## Basic information
- ID: feat-child-transcript-capability
- Priority: 65
- Workflow mode: deep
- Risk signals: public_api, external_dependency, compatibility, parallel_split
- Dependencies: feat-subagent-state-sync
- Plan phase: post-PR 4 capability gate

## User outcome
The project has a reproducible, safe decision on whether inactive child-session transcript details can be shown, without risking parent/child relationships or secretly reading internal storage.

## Scope and constraints

- Test official `session/messages` and/or `session/events` behavior for active and inactive child sessions.
- Determine whether child resume is required and whether it changes parent linkage, runtime state, or input routing.
- Test ended child sessions separately from running children.
- Produce protocol fixtures/evidence and a clear supported/unsupported decision.
- If safe, define the smallest follow-up display contract; if unsafe or unavailable, explicitly retain summary/output-tail-only Inspector behavior.
- Never read kernel SQLite or add an undocumented directed-message path.

## Acceptance scenarios

1. The evidence identifies tested kernel version, request shapes, child state, and observed response/error.
2. Resume side effects on parent linkage and input routing are explicitly measured or ruled out.
3. Running and ended child sessions are both covered where the public protocol permits.
4. The decision is deterministic: safe supported path with constraints, or documented exclusion with graceful UI behavior.
5. No implementation depends on API keys, internal database tables, or private RPCs.

## Technical notes

This is a bounded capability Feature. A well-supported “not safely available” result satisfies the Feature and prevents unsafe scope creep; it does not block the summary-based Inspector.
