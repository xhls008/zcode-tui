# adapt-zcode-3-7-6 · tasks

## 1. Compatibility facts

- [x] 1.1 Fetch and verify the official 3.7.6 deb — verification: package
  `3.7.6-4691`, CLI `0.16.3`, feed SHA-512 match, `@zcode/tui` absent.
- [x] 1.1a Install and verify official 3.7.7 — verification: package
  `3.7.7-4926`, CLI `0.16.3`, feed SHA-512 match, same protocol methods,
  `@zcode/tui` absent.
- [x] 1.2 Pin the breaking handshake and strict response schema — verification:
  unanswered create times out after 15s; the four-field reply completes create.
- [x] 1.3 Add proposal, design, tasks, and app-server spec delta.

## 2. Runtime preferences compatibility

- [x] 2.1 Add a pure exact reply encoder — verification: string/numeric id,
  method mismatch, and exact result-object unit tests.
- [x] 2.2 Dispatch runtime preferences during create/resume handshake and reuse
  the existing turn/idle server-request path — verification: fake transport
  handshake test and real 3.7.6/3.7.7 create/subscribe/V4 probes.
- [x] 2.3 Keep interaction requests unchanged — verification: existing plan and
  permission reply tests remain green.

## 3. Verification and documentation

- [x] 3.1 Run fmt, all-target tests, Clippy with warnings denied, and release
  build.
- [x] 3.2 Run real 3.7.6/3.7.7 TUI PTY prompts and require app-server streaming rather
  than the downgrade notice.
- [x] 3.3 Update compatibility baseline and changelog with measured results.
