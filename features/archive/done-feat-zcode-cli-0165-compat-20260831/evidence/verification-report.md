# Verification report: feat-zcode-cli-0165-compat

Verified at 2026-08-31T09:35:24Z in the isolated feature worktree.

## Official package evidence

- ZCode 3.9.1-5853 Linux x64 deb matched the SHA-512 published by the official
  update feed. Its bundled CLI reported `0.16.5`.
- The 3.9.1 CLI bundle SHA-256 was
  `883c12ab99b790fadc5f3ec2f229acd269d8c5460654b4c279c1e18368c436d8`
  (12,491,401 bytes).
- The comparison sample, ZCode 3.7.7-4926, reported CLI `0.16.3`; its bundle
  SHA-256 was
  `4130592942dcaa070f898c2c0152a8345dbfacbf6efb6422b2753c626e756bf5`
  (13,125,321 bytes).
- Both packages were downloaded and extracted only under a temporary `/tmp`
  directory. No official package or minified bundle is retained in the repo,
  and `/opt/ZCode` was not changed.

## Compatibility comparison

- Public CLI help adds one option: `--surface <surface>`, selecting `terminal`
  or `desktop` presentation for headless prompts/app-server.
- The app-server inventory grows from 50 to 54 methods, with no removals:
  `interaction/requestOfficialMcpAuthHeaders`,
  `workspace/cancelGenerateText`, `workspace/hooks/trustGrant`, and
  `workspace/updateModelIoPreferences`.
- The strict `session/requestRuntimePreferences` schema is unchanged.
- A live 0.16.5 app-server `session/create` sent the runtime-preferences reverse
  request and accepted zcode-tui's existing reply. The returned result included
  a session ID and the expected session/protocol/runtime projections.
- The newly relevant official-MCP-auth reverse request is pinned by a fixture.
  zcode-tui replies with the kernel's strict
  `{ "ok": false, "reason": "official_auth_unavailable" }` union, preserves
  the envelope ID, and does not echo request parameters or credentials.

## Local quality gates

- `cargo fmt --all --check`: passed.
- `cargo test --locked`: passed; 160 tests, 0 failed, 0 ignored.
- `cargo clippy --locked --all-targets --all-features -- -D warnings`: passed.
- `cargo build --release --locked`: passed.
- `bash -n install.sh`: passed.
- `target/release/zcode-tui --version`: exactly `zcode-tui 0.6.7`.
- `git diff --check`: passed.

## Acceptance scenarios and boundaries

All six scenarios in `spec.md` passed. The test proves protocol compatibility
for the initialization path and the new reverse-request fallback; it does not
claim desktop UI/installer coverage or successful authenticated official-MCP,
provider, or model-network behavior.

During analysis the unversioned update feed briefly returned 3.7.7 from one
CDN edge. The versioned package URL remained stable, and later plain and
cache-busted feed requests consistently returned 3.9.1.
