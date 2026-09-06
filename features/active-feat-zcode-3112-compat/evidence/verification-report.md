# Verification: ZCode 3.11.2 CLI compatibility

Verified 2026-09-05; release verification repeated 2026-09-06 in
`feature/feat-zcode-3112-compat`. The official website and versioned feed still
identify 3.11.2 as latest on 2026-09-06.

## Official package and TUI

- Website: https://zcode.z.ai/en/changelog
- Versioned feed: https://cdn-zcode.z.ai/zcode/electron/releases/3.11.2/linux-x64/latest.yml
- Official deb: `ZCode-3.11.2-linux-x64.deb`, 146866560 bytes;
  desktop build `3.11.2-6792`. Size and feed SHA-512 both matched:
  `4UAMZSP9ZWBrnb8wGaP39rb5dB5ikPx4aYPXKXGea1Vmd6L6vGma3rXwRxKbhBxgsCIE9jv6tmrdSL60RrzGcA==`.
- CLI reports **0.16.5**, same as 3.9.1, but the 12615227-byte bundle SHA-256 is
  `e9f1868c0fdb863537ed910ee3828b9be96b8c2fd805473f63b439e1113266b8`.
- Installed 3.9.1 comparison bundle remains unchanged at
  `883c12ab99b790fadc5f3ec2f229acd269d8c5460654b4c279c1e18368c436d8`.
- Official `node .../zcode.cjs tui` exits 1:
  `Error: Cannot find package '@zcode/tui' imported from .../zcode.cjs`.
- Isolated installer/wrapper checks passed: direct CLI version, absent-TUI
  fallback, and official-first selection with a synthetic resolvable TUI module.
  The synthetic module checks routing, not a working official TUI release.
- Package downloads/extraction stayed in `/tmp`. No installed package, login
  state, system DNS, credentials or Git remotes changed.

## Confirmed behavioral incompatibilities

1. Public help and runtime-preferences schema are unchanged; the method
   inventory adds only `v4/attachment/previewSource`, with no removals.
2. New internal `sendInput` returns admission plus a completion Promise, but
   legacy `session/send` does not await that completion. It emits
   `prompt_completed` before the actual streamed response. The pre-fix
   test passed with 3.9.1 but ended with empty text on 3.11.2.
3. The fix uses `turn.completed` / `turn.failed`, retaining explicit terminal
   state support. It does not guess completion from text length, delay, or the
   unchanged CLI version. The reduced observed ordering is pinned in
   `tests/fixtures/zcode-3.11.2-early-completion.jsonl`.
4. V4 foreground `stop` reaches the live runtime after legacy admission has
   returned. Its acknowledgement is also early: the client waits for a real
   terminal event before reusing the session. Failed-stop timeout kills the
   owned transport and retains the session ID for resume.
5. Switching draft-session sends wholesale to V4 is NOT a workaround: 3.11.2
   returned a foreign-key error for V4 sendText before the first persisted turn.
   Regular sends therefore retain the existing legacy attachment path.

## Executed checks

| Check | Result |
|---|---|
| `cargo fmt --all --check` | passed |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | passed |
| `cargo test --locked` | 162 passed; opt-in live test ignored in this default run |
| `cargo build --release --locked` and binary version | passed, zcode-tui 0.6.8 |
| `bash -n install.sh`, `git diff --check` | passed |
| `tests/official_kernel.rs`, explicitly run with 3.11.2 | passed |
| Same live test with 3.9.1 and 3.8.1 | both passed |
| `python3 tests/agents_pty_smoke.py` | 11/11 passed |
| Real 3.11.2 + release TUI, loopback provider, 120×35 PTY | first turn, second turn, cancel and continuation passed |

Live tests use fresh HOME/config/workspace and a loopback Anthropic-compatible
fake model with dummy credentials. They exercise production request encoders,
runtime-preferences replies, event decoding, turn reducer, V4 subscription,
successive turns, foreground stop/settlement, post-cancel continuation,
subagents/usage queries, close and resume. No paid model was called.

Real PTY evidence is retained as `real-tui.trace` (structural debug log, no
request parameters). It confirms V4 stop, cancelled turn settlement, a
subsequent successful send on the same session, and graceful session/close.
The terminal displayed first/second/continued answers, no successful empty
reply, and no cancelled response tail.

## Acceptance and delivery boundary

All six acceptance scenarios passed. Authenticated official MCP/OAuth, desktop
UI/installer operation, remote providers, attachment preview UI and non-Linux
runtime behavior are not claimed. Historical older-version evidence remains
historical; this run explicitly rechecks 3.8.1, 3.9.1 and 3.11.2.

The unversioned Linux feed still returned 3.9.1; no checksum verification was
weakened and no automated feed-scraping update mechanism was added.

On 2026-09-06 the user authorized merging and publishing Release v0.6.8 after
repeat verification. Release delivery uses the existing four-platform GitHub
Actions workflow. Local installed packages and binaries are not changed.
