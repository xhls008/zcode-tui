# Tasks: feat-theme-registry-accessible

- [x] Audit every theme-name, color, help, documentation, and version path
- [x] Add the shared theme registry and route parsing/saving/help/TUI through it
- [x] Add accessible and selection foreground palettes; repair themed rendering
- [x] Add registry, persistence, palette, selection, and light-contrast tests
- [x] Update both READMEs, changelog, and v0.6.5 Cargo metadata
- [x] Run all requested gates and save verification evidence
- [x] Prepare the verified commit, tag, push, and GitHub Release metadata

## Progress log

- 2026-08-29: Audited the v0.6.4 theme implementation and prior release archive.
  Confirmed four duplicated runtime allowlists, one static help list, a missing
  `selection_fg`, dark syntect colors rendered unchanged on light code panels,
  and three official-icon helpers with fixed dark RGB values.
- 2026-08-29: Added the shared eleven-entry registry and routed runtime help,
  parsing, persistence, command validation/listing, and palette dispatch through
  it. Added accessible, per-theme selection foregrounds, light syntax contrast,
  and theme-token icon rendering with focused regression coverage.
- 2026-08-29: Updated v0.6.5 metadata and both READMEs. Passed 152 Rust tests,
  fmt, strict Clippy, debug build, diff check, generated-help smoke check, and
  Cargo metadata version check.
