# Tasks: feat-theme-registry-accessible

- [ ] Audit every theme-name, color, help, documentation, and version path
- [ ] Add the shared theme registry and route parsing/saving/help/TUI through it
- [ ] Add accessible and selection foreground palettes; repair themed rendering
- [ ] Add registry, persistence, palette, selection, and light-contrast tests
- [ ] Update both READMEs, changelog, and v0.6.5 Cargo metadata
- [ ] Run all requested gates and save verification evidence
- [ ] Commit, merge, tag, push main/tag, and publish GitHub Release v0.6.5

## Progress log

- 2026-08-29: Audited the v0.6.4 theme implementation and prior release archive.
  Confirmed four duplicated runtime allowlists, one static help list, a missing
  `selection_fg`, dark syntect colors rendered unchanged on light code panels,
  and three official-icon helpers with fixed dark RGB values.
