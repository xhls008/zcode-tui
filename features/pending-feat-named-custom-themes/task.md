# Tasks: feat-named-custom-themes

- [ ] Add dynamic custom-theme registry records, validation, and palette overlay
- [ ] Parse one or more `[[custom_themes]]` sections with tolerant diagnostics
- [ ] Route startup, help, `/theme` listing/switching, and persistence through the dynamic registry
- [ ] Add config, registry, command, persistence, and restart-restoration tests
- [ ] Update both READMEs, changelog, and v0.6.6 Cargo metadata
- [ ] Run every requested quality/build gate and save verification evidence
- [ ] Commit, merge, tag, push, and publish GitHub Release v0.6.6

## Progress log

- 2026-08-29: Reviewed the v0.6.5 registry archive and traced theme parsing,
  persistence, startup, TUI dispatch, dynamic help, and existing regression
  tests. Chose an owned runtime extension of the existing registry with no new
  dependency and tolerant per-section diagnostics.
