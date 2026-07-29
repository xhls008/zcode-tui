# adapt-zcode-3-5-3

## Why

ZCode desktop 3.5.3 still reports CLI version `0.15.2`, so the TUI cannot
infer protocol compatibility from `zcode version`. Live comparison with 3.3.6
shows a breaking app-server transition:

- 3.5.3 removes the legacy `session/steer`, `session/previewFileRewind`,
  `session/applyFileRewind`, `session/rewind`, and `session/rewindCascade`
  handlers.
- replacement control semantics are exposed through V4 conversation snapshots
  and CAS-style `v4/command` envelopes.
- the existing steer PTY test is a false positive: it sees the optimistic
  "steering" marker, then the request fails with Method not found and the input
  is silently returned to the normal queue.
- the CLI adds `--browser-use headless` and `--browser-executable`, but the
  fallback TUI's app-server path currently ignores those passthrough flags.

The existing package discovery and official-feed fallback already install and
select 3.5.3 correctly. The remaining work is protocol and routing adaptation,
not another kernel installer rewrite.

## What Changes

- Keep the proven legacy `session/create|resume|subscribe|send|event` stream,
  then negotiate an optional V4 conversation subscription for control-plane
  state.
- Cache V4 `revision`, `logEpoch`, rows, input routing, and availability for
  version-safe command construction.
- Use `setFollowupMode: guide` plus V4 `sendText` for mid-turn steering on
  3.5.3; retain `session/steer` only when V4 is unavailable on an older kernel.
- Move 3.5.3 rewind to V4 row/entity targets, preview, and
  `applyFileRewind` command semantics while preserving the current safe legacy
  rewind path for 3.3.6.
- Route Browser Use prompts through the official classic CLI path until a
  verified app-server browser runtime contract exists; never silently discard
  browser flags.
- Tighten real-kernel PTY assertions so a displayed optimistic marker cannot
  pass without the corresponding protocol acknowledgement.

## Impact

- Affected modules: `src/lib.rs`, `src/main.rs`, `tests/core.rs`,
  `tests/pty_smoke.py`, README, changelog, and protocol debug logging.
- Affected users: users running the rootless or system ZCode 3.5.3 package,
  especially those using in-turn steering, `/rewind`, or Browser Use flags.
- Subprocess cleanup is unchanged: app-server remains process-group owned and
  killed on disconnect/drop; Browser Use classic jobs use the existing
  streaming job process-group cancellation path.

## Out Of Scope

- Reimplementing Browser Use, automation, or desktop browser interaction in
  the TUI.
- Adopting every V4 conversation/attachment/usage method without a TUI feature
  that needs it; this follows the design document's "加法导向" boundary.
- Sending guessed `browserUse` fields to strict `session/create` or
  `session/send` schemas (live 3.5.3 rejects them).
- Claiming V4 rewind support before a fresh 3.5.3 write-session probe verifies
  preview and apply against an actionable row target.
- Implementing `--settings` or `--max-turns`; 3.5.3 still advertises but rejects
  both options.
