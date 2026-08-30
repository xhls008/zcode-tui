# Feature: feat-cross-platform-ci Cross-platform CI and release validation

## Basic information
- ID: feat-cross-platform-ci
- Priority: 90
- Workflow mode: deep
- Risk signals: multi_module, external_dependency, compatibility
- Dependencies: none
- Created: 2026-08-30T13:02:23Z

## User outcome
Every pull request is quality-gated, Windows and macOS run native tests, and each release binary is executed on its native runner before publication.

## Scope and constraints

- Add a read-only GitHub Actions CI workflow for pull requests and pushes to
  `main`: Ubuntu runs formatting, Clippy, and the full Rust suite; native
  Windows x64, macOS Apple Silicon, and macOS Intel runners execute the Rust
  tests on their host platforms.
- Keep tag publication in the existing release workflow. Each release runner
  must run native tests, build its target on matching hardware, execute the
  produced binary, and require exact `zcode-tui <tag-version>` output before
  uploading the artifact.
- Use current official Node 24 action majors and explicit runner labels where
  architecture matters. Grant CI read-only repository permissions; retain
  release `contents: write` only where publication needs it.
- Do not inject ZCode credentials, API keys, or a real account into hosted
  runners. Protocol/auth/session behavior remains covered by deterministic
  fake app-server and fixture tests. Account-backed end-to-end checks remain a
  documented optional manual or private self-hosted-runner gate.
- Document the hosted-runner validation model in both READMEs and align the
  design document's verified ZCode baseline with the existing README baseline.
- Do not refactor `main.rs`, add `/doctor`, change runtime behavior, remove the
  initialized feature-workflow runtime copy, bump the product version, publish,
  or touch/stage `.claude/`, `package.json`, or `tests/test.py`.

## Acceptance scenarios

1. Opening or updating a pull request, or pushing to `main`, schedules Ubuntu
   quality gates plus native Windows x64, macOS arm64, and macOS x64 tests.
2. CI jobs have `contents: read`, use Node 24 action releases, and can be
   cancelled when a newer run supersedes the same branch or pull request.
3. A tag build runs tests on every native build runner, builds Linux musl,
   Windows x64, macOS Intel, and macOS Apple Silicon artifacts, and refuses to
   upload a binary whose `--version` output differs from the tag.
4. Intel and Apple Silicon macOS artifacts are built and executed on matching
   GitHub-hosted architectures rather than relying on cross-compilation alone.
5. CI contains no product credentials and the documentation states what hosted
   tests prove and what still requires a real ZCode installation/account.
6. README Chinese/English guidance and the design environment baseline agree
   on the validation approach and ZCode 3.8.1 / CLI 0.16.3 baseline.
7. Workflow files pass structural review; `cargo fmt --all --check`,
   `cargo test --locked`, `cargo clippy --locked --all-targets --all-features
   -- -D warnings`, `cargo build --locked`, and `bash -n install.sh` pass.

## Technical notes

- Official `actions/runner-images` labels checked on 2026-08-30:
  `windows-latest` = Windows Server 2025 x64, `macos-latest` = macOS 26 arm64,
  and `macos-15-intel` = macOS 15 x64.
- Official action releases checked on 2026-08-30: checkout v7, upload-artifact
  v7, and download-artifact v8 use `node24`.
- Prefer two small workflows over a reusable-workflow abstraction: CI validates
  proposed code continuously; release independently rechecks a tag and publishes.
