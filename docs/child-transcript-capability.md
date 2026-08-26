# Child-session transcript capability decision

Status: **not supported in the Inspector on ZCode 0.16.3**.

The public protocol exposes child identity, lifecycle state, title, and final summary through `session/subagents`, but it does not provide a read-only transcript path for a child that is merely running under its parent or has ended. The Inspector therefore keeps the existing summary/output-tail contract and does not read SQLite or automatically resume children.

## Reproduction boundary

The probe was run locally on 2026-08-26 with `zcode 0.16.3` against disposable parent/Explore-child sessions in this workspace. Requests used newline-delimited app-server envelopes (`{id, method, params}`) and contained no API keys. Identifiers and transcript content in the committed fixture are sanitized.

The structural fixture is [zcode-0.16.3-child-transcript.json](../tests/fixtures/zcode-0.16.3-child-transcript.json).

## Observations

`session/subagents {sessionId: parent}` returned the public 0.16.3 shape:

- top-level `revision` and `childSessionIds`;
- `running[]` records while the child was executing;
- `ended.items[]` with `status: success` and a final `summary` after completion.

For the child ID, both candidate reads returned error `-32004` (`Session is not active`) while `session/subagents` reported it as running. The same requests returned the same error after the child ended:

```text
session/messages {sessionId: child}
session/events   {sessionId: child}
```

Adding `afterSeq` and `limit` to `session/events` did not change the result.

## Resume side-effect probe

A single explicit `session/resume {sessionId: endedChild}` probe succeeded and returned four messages. After resume, `session/messages` and `session/events` also became readable. The response identified the session as `sessionKind: subagent_child`, retained its `parentSessionId`, and the parent's `session/subagents` relation and ended status were unchanged before/after the probe.

However, resume is a stateful activation operation, not a read API. It materialized the child as an idle active session and its V4 snapshot advertised `inputRouting.mode: startNow`. A production Inspector would therefore have to activate a child with an input-capable control plane merely to display history. The public protocol provides no read-only flag, no stated guarantee that activation is side-effect free across versions, and no way to express “inspect without changing session/runtime routing.”

## Product decision

The supported display contract remains:

- child status, type, title, identifiers, and final summary from `session/subagents`;
- official lifecycle/V4 output tail when supplied;
- explicit `viewing: read-only` and `input target: parent` labels.

Full child transcript is excluded until ZCode exposes a documented read-only operation that works for inactive child sessions. The TUI must not call `session/resume` from the Inspector, must not send directed child input, and must not fall back to kernel SQLite.
