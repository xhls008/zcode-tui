# adapt-zcode-3-5-3 · tasks

## 1. Compatibility facts and requirements

- [x] 1.1 Install and SHA-512 verify official Linux desktop 3.5.3 rootlessly;
  confirm wrapper selects it — verification: feed version `3.5.3`, deb metadata
  `3.5.3-3911`, installed path `~/.local/opt/zcode/3.5.3`, real wrapper probes.
- [x] 1.2 Compare 3.3.6/3.5.3 help, bundle methods, strict schemas, and live
  handler responses — verification: legacy steer/rewind reaches validation on
  3.3.6 and returns Method not found on 3.5.3; V4 subscribe returns a snapshot.
- [x] 1.3 Run non-mutating and existing-flow smoke checks — verification:
  `session/list`, a basic streamed turn, `/sessions`, and resume pass on 3.5.3.
- [x] 1.4 Add proposal, design, tasks, and capability deltas — verification:
  OpenSpec validation and review against `openspec/config.yaml` plus design
  roadmap §9.

## 2. Hybrid V4 capability layer

- [x] 2.1 Add pure encoders/decoders for V4 subscribe, frame snapshots, command
  acknowledgements, and CAS command envelopes — verification: exact JSON and
  malformed/unknown-field unit tests in `tests/core.rs`.
- [x] 2.2 Add optional per-session V4 state and negotiate after legacy session
  creation/resume without breaking the text stream — verification: simulated
  V4 available, Method not found, protocol error, session switch, and disconnect
  tests.
- [x] 2.3 Extend protocol debug logging with V4 method, command type, revision,
  and semantic ack status while redacting prompt content — verification: log
  formatter unit tests plus an isolated debug-log PTY check.

## 3. Turn steering migration

- [x] 3.1 Implement V4 `setFollowupMode:guide` then `sendText`; declare success
  only for acknowledgement delivery `guide` — verification: unit tests for
  guide/queue/startNow/rejected/stale outcomes.
- [x] 3.2 Retain `session/steer` only for negotiated legacy kernels and preserve
  error-to-queue behavior — verification: fake 3.3.6 and 3.5.3 transport tests.
- [x] 3.3 Replace PTY scenario 15's optimistic-marker assertion with a real
  3.5.3 semantic-ack assertion and an error-not-present assertion —
  verification: scenario fails on commit `5bde35c` and passes after migration.

## 4. Rewind migration

- [x] 4.1 In a temporary workspace, create a fresh 3.5.3 session that writes a
  disposable file; capture actionable V4 rows and pin exact preview/apply
  request and acknowledgement shapes — verification: recorded probe transcript
  and disk-content assertions; no existing user session is touched.
- [x] 4.2 Build row/entity rewind candidates from V4 snapshots and keep legacy
  checkpoint candidates only for non-V4 kernels — verification: snapshot row
  normalization and session-reset unit tests.
- [x] 4.3 Implement V4 preview-first apply with unsafe-file refusal and CAS
  result handling; never call removed legacy methods on V4 kernels —
  verification: fake protocol tests plus real temporary-workspace PTY rewind.
- [x] 4.4 Preserve 3.3.6 legacy rewind behavior — verification: existing rewind
  unit suite plus a 3.3.6 compatibility smoke when that kernel is available.

## 5. Browser Use routing

- [x] 5.1 Parse/normalize `--browser-use` and `--browser-executable` as explicit
  configuration instead of opaque passthrough — verification: CLI parser and
  prompt command-vector unit tests.
- [x] 5.2 Force Browser Use turns onto the official classic prompt path with a
  visible streaming-controls limitation notice — verification: fake binary
  test proves both flags reach `zcode --prompt` and no app-server request is
  sent.
- [x] 5.3 Pin official validation behavior for invalid mode and executable
  without headless mode — verification: command-level smoke against 3.5.3.

## 6. Documentation and release verification

- [x] 6.1 Update README, design environment facts, and CHANGELOG with hybrid
  protocol behavior and Browser Use limitation — verification: documentation
  diff review.
- [x] 6.2 Run `cargo fmt --check`, all-target tests, Clippy with warnings denied,
  release build, shell syntax check, and OpenSpec validation.
- [x] 6.3 Run the full model-backed PTY suite against 3.5.3; require every
  protocol-control scenario to assert semantic success rather than optimistic
  UI text before committing or pushing.
