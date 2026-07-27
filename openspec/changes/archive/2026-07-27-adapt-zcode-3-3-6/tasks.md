# adapt-zcode-3-3-6 · tasks

## 1. Compatibility facts and requirements

- [x] 1.1 Compare 3.3.4/3.3.6 CLI help, bundle method set, create/resume schemas,
  app-update metadata, and a read-only `session/list` probe — verification:
  captured local commands; both kernels return the same list envelope, 3.3.6
  adds `toolAllowlist/toolDenylist` schema fields.
- [x] 1.2 Add proposal, design, tasks, and capability deltas — verification:
  artifact review against `openspec/config.yaml` and design roadmap §9.

## 2. Kernel discovery and update channel

- [x] 2.1 Add pure helpers for numeric version ordering, rootless application
  discovery, version extraction, and update-feed selection — verification:
  unit tests with temporary `3.9.9`/`3.10.0` layouts and loopback/override feeds.
- [x] 2.2 Update both install-time and generated wrapper discovery to numeric
  ordering and export the resolved `ZCODE_APP` to fallback TUI processes —
  verification: shell syntax check plus a temporary fake-kernel wrapper probe.
- [x] 2.3 Reuse discovery in startup probe and `/update`, including rootless
  installed-version comparison and shell-safe interpolation — verification:
  unit tests plus an isolated PTY run using `ZCODE_TUI_UPDATE_FEED`: screen
  state showed installed 3.3.6/latest 99.99.99 and sha512 mismatch abort.

## 3. App-server tool policy

- [x] 3.1 Parse tool allow/deny flags and `--permission-mode`, preserving
  internal spaces and canonicalizing comma-separated entries — verification:
  `parse_cli_args` unit tests.
- [x] 3.2 Add `with_tool_policy` protocol params and wire create, resume, and
  resume-fallback create — verification: exact JSON shape unit tests and code
  review of all handshake branches.
- [x] 3.3 Keep classic `--prompt` policy flags equivalent — verification:
  prompt command vector unit test.

## 4. Documentation and verification

- [x] 4.1 Update README/help/environment variables and CHANGELOG Unreleased —
  verification: documentation diff review.
- [x] 4.2 Run `cargo fmt --check`, all-target tests, Clippy with warnings denied,
  release build, shell syntax check, and read-only 3.3.6 app-server probe.
- [x] 4.3 Run the model-backed PTY suite against 3.3.6 when quota/network are
  intentionally available — verification: `uv run --with pyte python
  tests/pty_smoke.py` completed with 72/72 checks passed.
