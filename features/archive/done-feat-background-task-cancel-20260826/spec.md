# Feature: feat-background-task-cancel Safe background task cancellation

## Basic information
- ID: feat-background-task-cancel
- Priority: 60
- Workflow mode: deep
- Risk signals: public_api, external_dependency, compatibility, parallel_split
- Dependencies: feat-agent-inspector
- Plan phase: PR 4B

## User outcome
Users can cancel a selected officially cancellable background task without stopping the parent turn or accidentally sending the wrong identifier.

## Scope and constraints

- Offer cancellation only when `cancellable=true` and a real kernel `taskId` are present.
- Call `session/cancelBackgroundTask` with that exact `taskId`; never substitute child-session, agent, tool-call, PID, or display IDs.
- Show pending, success, already-finished, unavailable, and failed outcomes without closing the Inspector or parent session.
- Prevent duplicate cancel requests while one is in flight.
- Do not add generic process killing or direct Subagent stop mechanisms outside the public protocol.

## Acceptance scenarios

1. Non-cancellable or identifier-incomplete records show no enabled cancel action.
2. Cancel sends the exact official task ID for the selected record.
3. A failed or method-not-found response leaves the parent session usable and explains the outcome.
4. Live completion racing with cancellation converges on one terminal state without repeated requests.
5. Cancelling one background task never invokes parent `session/stop`.

## Technical notes

Treat cancellation as a protocol command with explicit in-flight correlation and authoritative state confirmation rather than an optimistic local terminal-state change.
