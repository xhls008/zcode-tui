# Tasks: feat-cross-platform-ci

- [x] Add PR/main Ubuntu quality and native Windows/macOS test workflow
- [x] Update release Actions and bind macOS builds to matching architectures
- [x] Run native tests and exact-version binary smoke checks before upload
- [x] Document hosted-runner and real-ZCode validation boundaries in both READMEs
- [x] Align the design environment baseline with ZCode 3.8.1 / CLI 0.16.3
- [x] Run workflow review and local Rust/shell quality gates
- [x] Save verification evidence; keep PR unmerged pending maintainer approval

## Progress log

- 2026-08-30: Confirmed official runner-image labels and Node 24 action majors.
  Chose GitHub-hosted Windows x64, macOS arm64, and macOS Intel runners; real
  account credentials remain outside hosted CI.
- 2026-08-30: Added continuous cross-platform CI, native release tests and
  exact-version binary smoke checks; documented hosted versus real-ZCode gates.
- 2026-08-30: Local verification passed YAML parsing, 159/159 Rust tests, fmt,
  Clippy with warnings denied, release build, install.sh syntax, and exact
  `zcode-tui 0.6.6` binary-version smoke. Windows/macOS hosted jobs require an
  authorized branch push and remain the final executable acceptance gate.
- 2026-08-30: PR #6 first run passed Ubuntu and macOS arm64, then exposed two
  Windows assumptions: user config only checked `HOME`, and an attachment test
  assumed `/` separators. Added `USERPROFILE` fallback and component-aware path
  assertion; local fmt, Clippy, and 159/159 tests passed again before rerun.
- 2026-08-30: PR #6 run 33318040354 passed all four jobs: Ubuntu quality,
  Windows x64, macOS arm64, and macOS x64. Saved canonical verification
  evidence; the PR remains open and unmerged for maintainer approval.
