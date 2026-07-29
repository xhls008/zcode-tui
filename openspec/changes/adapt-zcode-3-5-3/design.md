# adapt-zcode-3-5-3 · design

## Context

The official Linux feed currently resolves to desktop `3.5.3` (deb metadata
`3.5.3-3911`). The package is installed rootlessly at
`~/.local/opt/zcode/3.5.3`, its SHA-512 matches the feed, and the existing
wrapper selects it by numeric version ordering. `session/list`, a basic
app-server streaming turn, `/sessions`, and resume all pass against the real
kernel. `@zcode/tui` remains absent, so the Rust fallback is still active.

Static bundle inspection was cross-checked with live requests. On 3.5.3 the
five legacy steer/rewind methods return `-32601 Method not found`; on 3.3.6 the
same methods reach parameter validation, proving this is a handler removal and
not a malformed probe. Conversely, `v4/conversation/subscribe` returns an ack
and a `v4/conversation/frame` snapshot on 3.5.3.

## Architecture

Use a hybrid transport rather than replacing the working stream:

1. Establish the session through legacy `session/create` or `session/resume`,
   then legacy `session/subscribe` and `session/send` as today.
2. Once `sessionId` is known, try `v4/conversation/subscribe` with topic
   `conversation/<sessionId>`, a stable per-process `connectionId`,
   `clientMode:"desktop-continuous"`, and foreground visibility.
3. If the V4 method succeeds, consume `v4/conversation/frame` snapshots and
   mark V4 controls available. If it returns Method not found, keep the legacy
   3.3.6 control path. Other errors are surfaced and leave controls disabled;
   they do not terminate the working legacy text stream.
4. Route only features that require V4 through `v4/command`. Body deltas,
   permission/user-input server requests, attachments, and ordinary send remain
   on the already-tested legacy path.

This keeps the compatibility surface small and follows the project boundary:
consume protocol only for existing TUI additions, not for parity with the
desktop application.

## Interfaces

### V4 subscription

```json
{
  "method": "v4/conversation/subscribe",
  "params": {
    "topic": "conversation/<sessionId>",
    "connectionId": "zcode-tui-<process-id>",
    "clientMode": "desktop-continuous",
    "visibility": "foreground"
  }
}
```

The snapshot fields required by this change are:

- `revision` and `logEpoch` for compare-and-swap command bases;
- `rows.window[]` for stable rewind targets;
- `inputRouting.mode` and `config.followupMode` for steering state;
- `availability.*.allowed` for feature gates.

Unknown frame fields and row kinds remain forward-compatible and are ignored.

### V4 command envelope

```json
{
  "commandId": "cmd_<unique>",
  "clientId": "zcode-tui-<process-id>",
  "sessionId": "sess_...",
  "baseRevision": 42,
  "baseLogEpoch": "epoch_...",
  "type": "setFollowupMode",
  "payload": {"mode": "guide"},
  "issuedAt": 1780000000000
}
```

CAS requirements follow the verified 3.5.3 command schema rather than a blanket
rule: `setFollowupMode` requires `baseRevision`; `applyFileRewind` requires both
`baseRevision` and `baseLogEpoch`; `sendText` requires neither. A stale or
rejected acknowledgement reports the failure, and the client MUST NOT blindly
resend `sendText`, because doing so can duplicate user input.

### Steering

When V4 is active, mid-turn Enter is a two-command flow:

1. send `setFollowupMode {mode:"guide"}`;
2. after it is accepted, send `sendText` with the user's content.

The observed 3.5.3 send response reports command status but no
`inputDisposition`; the semantic result appears in the following V4 frame's
queue item as `delivery.admitted`. The steer is successful only when that value
is `guide` (a direct future `inputDisposition` result is accepted equivalently).
`startNow` or `queue` is not steer success and must be represented honestly. On
kernels without V4, retain the verified legacy `session/steer` request and
current error-to-queue discipline.

### Rewind

V4 rewind targets are row identities, not legacy checkpoint unions:

```json
{"rowId": 7, "entityId": "msg_..."}
```

The intended 3.5.3 flow is:

1. select an actionable row from the latest V4 snapshot;
2. call `v4/conversation/fileRewindPreview` with session, target, revision, and
   log epoch;
3. render safe/unsafe file information and require explicit confirmation;
4. issue `v4/command` type `applyFileRewind` using the same target and current
   CAS base;
5. accept success only from the command acknowledgement and subsequent frame.

A fresh disposable 3.5.3 session was verified with two writes: the latest
completed `turnHeader` exposed `actions.canRewindFiles:true`; V4 preview returned
`canApply:true`; apply returned `applied:true`; and the temporary file changed
from `two` back to `one`. No write probe targets an existing user session or
non-temporary workspace.

For V4 kernels, the client MUST NOT fall back to removed legacy methods. For
older kernels without V4, the current checkpoint preview and safe
`session/applyFileRewind` behavior remains intact.

### Browser Use

The shipped 3.5.3 CLI accepts only `--browser-use headless` and requires
`--browser-executable` to accompany that mode. Neither strict
`session/create` nor strict `session/send` accepts a `browserUse` field.

Until a verified app-server browser runtime contract exists, any prompt with
Browser Use options is routed to the existing classic `zcode --prompt` job,
where the official parser and runtime receive the flags. The TUI shows a short
notice that app-server streaming/control features are unavailable for that
turn. Invalid combinations remain the official CLI's responsibility and their
stderr is surfaced normally.

## Data Model

Add an optional per-session `V4ConversationState` containing:

- capability state: probing, available, or unavailable;
- `connection_id`, `revision`, and `log_epoch`;
- normalized rewind row candidates (`row_id`, `entity_id`, label metadata);
- current input routing/followup mode and relevant availability flags;
- pending command ids and their command kind.

The state is cleared on `/new`, session switch, app-server downgrade, or
disconnect. It is not persisted to disk because revision/log epoch are live
connection state.

## Risks

- **Mixed legacy/V4 events race**: serialize outbound commands through the
  existing connection writer and update CAS state from every V4 frame before
  accepting new control input.
- **Duplicate steer text after stale revision**: never automatically replay a
  rejected `sendText`; return the text to the local queue with an explicit
  diagnostic.
- **Optimistic UI masks failure**: mark steering/rewind as pending until a
  semantic ack is received; PTY tests assert ack-derived delivery/result, not
  just local transcript text.
- **Rewind damages user files**: live verification uses a fresh session and a
  temporary workspace; application remains preview-first and refuses unsafe
  files.
- **V4 absent on 3.3.6**: Method not found is a normal capability result and
  preserves the existing legacy implementation.
- **Browser flags silently disappear**: prompt routing is decided before
  app-server send; classic fallback receives the untouched passthrough flags.

## Alternatives

- **Convert all traffic to V4**: rejected because legacy streaming,
  interactions, attachments, and session lifecycle still work on 3.5.3; a
  wholesale rewrite adds risk without a TUI benefit.
- **Detect by `zcode version`**: rejected because 3.3.6 and 3.5.3 both report
  `0.15.2`.
- **Try legacy first and fall back after Method not found**: rejected for
  steering because it creates visible false-success and avoidable queueing;
  capability negotiation is deterministic.
- **Guess `browserUse` session fields**: rejected because live strict-schema
  probes reject them.
- **Disable Browser Use entirely**: rejected because the official classic CLI
  already supports it and the TUI can route to that path without reimplementing
  browser behavior.
