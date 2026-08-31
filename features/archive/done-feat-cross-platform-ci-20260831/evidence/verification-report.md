# Verification report: feat-cross-platform-ci

Verified at 2026-08-30T14:55:12Z in the isolated feature worktree and on
[PR #6](https://github.com/xhls008/zcode-tui/pull/6).

## Hosted platform results

GitHub Actions run
[33318040354](https://github.com/xhls008/zcode-tui/actions/runs/33318040354)
passed all jobs:

- Ubuntu quality gates: passed in 48 seconds.
- Windows Server 2025 x64 native Rust tests: passed in 1 minute 27 seconds.
- macOS Apple Silicon native Rust tests: passed in 35 seconds.
- macOS 15 Intel native Rust tests: passed in 1 minute 56 seconds.

The first Windows run found that user configuration assumed Unix `HOME` and
one attachment assertion assumed `/` path separators. The rerun above verifies
the `USERPROFILE` fallback and component-aware path check on native Windows.

## Local quality and build

- `cargo fmt --all --check`: passed.
- `cargo test --locked`: passed; 159 tests, 0 failed.
- `cargo clippy --locked --all-targets --all-features -- -D warnings`: passed.
- `cargo build --release --locked`: passed.
- `bash -n install.sh`: passed.
- YAML structural parsing for CI and release workflows: passed.
- `target/release/zcode-tui --version`: exactly `zcode-tui 0.6.6`.
- `git diff --check`: passed.

## Acceptance scenarios

All seven scenarios in `spec.md` passed by hosted execution, local quality
gates, workflow review, and documentation review. The tag-only release matrix
is structurally verified here; its four native tests and exact-version binary
smoke checks will execute again when a maintainer creates the next release tag.

## Credential boundary

Hosted CI contains no ZCode account or API credentials. Fake app-server and
fixture tests cover protocol, authentication, model, and session behavior.
Real-account/provider-network checks remain restricted to maintainer machines
or private self-hosted runners that do not execute untrusted pull requests.
