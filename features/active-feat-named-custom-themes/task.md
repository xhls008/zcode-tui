# Tasks: feat-named-custom-themes

- [x] Add dynamic custom-theme registry records, validation, and palette overlay
- [x] Parse one or more `[[custom_themes]]` sections with tolerant diagnostics
- [x] Route startup, help, `/theme` listing/switching, and persistence through the dynamic registry
- [x] Add config, registry, command, persistence, and restart-restoration tests
- [x] Update both READMEs, changelog, and v0.6.6 Cargo metadata
- [x] Run every requested quality/build gate and save verification evidence
- [x] Prepare the verified commit, tag, push, and GitHub Release metadata

## Progress log

- 2026-08-29: Reviewed the v0.6.5 registry archive and traced theme parsing,
  persistence, startup, TUI dispatch, dynamic help, and existing regression
  tests. Chose an owned runtime extension of the existing registry with no new
  dependency and tolerant per-section diagnostics.
- 2026-08-29: Added dynamic custom entries and tolerant `[[custom_themes]]`
  parsing, then routed startup, live help, command lists/validation, palette
  resolution, persistence, and restart restoration through the runtime
  registry. Invalid entries surface diagnostics without blocking valid config.
- 2026-08-29: Documented v0.6.6 in Chinese and English and passed 159 Rust
  tests, formatting, strict Clippy, debug build, version/help smoke checks,
  Cargo metadata validation, and diff hygiene checks.
