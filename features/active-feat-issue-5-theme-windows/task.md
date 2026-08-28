# Tasks: feat-issue-5-theme-windows

- [ ] Add tested home-directory, theme parsing, and theme persistence helpers
- [ ] Add dark/light palettes and wire `/theme` into local command handling
- [ ] Harden `/sessions` lookup and row rendering for Windows terminals
- [ ] Update README files, v0.6.2 metadata, and CHANGELOG
- [ ] Run formatting, Rust tests, Clippy, and build
- [ ] Save verification evidence and complete the delivery workflow

## Progress log

- 2026-08-28: Confirmed Issue #5 via GitHub. The attached Windows screenshot
  shows the DB-unavailable fallback rather than a malformed picker overlay.
- 2026-08-28: Traced `/sessions`, startup DB probing, `UiConfig`, `Theme`, slash
  classification/catalog, and terminal initialization. Chose the smallest
  shared fixes without new dependencies.
