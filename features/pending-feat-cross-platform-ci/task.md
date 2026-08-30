# Tasks: feat-cross-platform-ci

- [ ] Add PR/main Ubuntu quality and native Windows/macOS test workflow
- [ ] Update release Actions and bind macOS builds to matching architectures
- [ ] Run native tests and exact-version binary smoke checks before upload
- [ ] Document hosted-runner and real-ZCode validation boundaries in both READMEs
- [ ] Align the design environment baseline with ZCode 3.8.1 / CLI 0.16.3
- [ ] Run workflow review and local Rust/shell quality gates
- [ ] Save verification evidence and complete the delivery workflow

## Progress log

- 2026-08-30: Confirmed official runner-image labels and Node 24 action majors.
  Chose GitHub-hosted Windows x64, macOS arm64, and macOS Intel runners; real
  account credentials remain outside hosted CI.
