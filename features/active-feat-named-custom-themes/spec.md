# Feature: feat-named-custom-themes Named custom themes

## Basic information
- ID: feat-named-custom-themes
- Priority: 90
- Workflow mode: deep
- Risk signals: multi_module, compatibility, data_consistency
- Dependencies: none
- Created: 2026-08-29T11:27:04Z

## User outcome
Users can define, switch, persist, and restore validated named custom color themes through the existing theme registry.

## Scope and constraints

- Extend the v0.6.5 theme registry at runtime with zero or more validated
  `[[custom_themes]]` config sections. Each custom theme has a unique name, an
  optional built-in base (default `dark`), and any subset of the existing ten
  color tokens including `selection_fg`.
- Keep built-in definitions static and represent valid custom themes in the
  same registry abstraction used for name validation, palette resolution,
  display lists, command help, persistence, and restart restoration. Do not add
  a second allowlist.
- Preserve the existing top-level `theme`, `notify`, and color override format.
  `/theme <custom-name>` must only update the top-level `theme` line and must
  leave every custom theme section byte-for-byte equivalent apart from newline
  normalization already performed by the existing saver.
- Accept lowercase ASCII theme names of 1-32 characters: alphanumeric segments
  separated by single hyphens. Reject empty, malformed, duplicate, or built-in
  conflicting names; reject unknown/non-built-in bases and malformed color
  values with clear diagnostics while retaining every other valid config value
  and custom theme.
- Keep exactly the existing eleven built-in themes; do not add new university
  themes. Add no dependency.
- Update Chinese/English documentation, changelog, Cargo metadata, and lock
  metadata for v0.6.6.
- Never stage the user-owned `.claude/`, `package.json`, or `tests/test.py`.
- Publish only after fmt, tests, strict Clippy, build, focused acceptance checks,
  and workflow completion validation pass; then push `main` and `v0.6.6` and
  create the requested GitHub Release.

## Acceptance scenarios

1. A config with one custom theme resolves unspecified tokens and light/dark
   behavior from its built-in base; absent `base` resolves from `dark`.
2. Multiple valid custom themes extend the registry in declaration order and
   `/theme list` marks each with `(custom)` alongside all eleven built-ins.
3. Built-in conflicts, duplicate names, empty/invalid/overlong names, unknown
   bases, and invalid custom color values produce specific diagnostics; only
   the invalid custom entry is unavailable and the configuration file remains
   unchanged.
4. `/theme <custom-name>` switches its resolved palette immediately, persists
   only the top-level selection, and a fresh state restores the same custom
   theme and palette.
5. Config theme parsing, persistence validation, command validation/listing,
   help choices, and palette resolution all query the dynamic registry; tests
   demonstrate that no independent custom-theme allowlist exists.
6. Existing top-level token overrides continue to apply after either a built-in
   or custom palette is selected, including `selection_fg`.
7. README.md and README.en.md document eleven built-ins plus named custom
   themes with a complete config example and `/theme` usage; CHANGELOG.md and
   Cargo metadata report v0.6.6.
8. `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets -- -D warnings`,
   `cargo build`, workflow hard gates, scoped commit checks, push, tag, and
   GitHub Release all succeed.

## Technical notes

- Add a small owned custom-theme record and dynamic registry beside
  `BuiltInTheme`; palette resolution can copy `ThemePalette` and overlay the
  section's validated RGB tokens.
- Keep the tolerant startup contract by storing config diagnostics on
  `UiConfig`: invalid custom sections are ignored, valid settings survive, and
  the TUI surfaces diagnostics without refusing to start.
- Retain `help_text()` for built-in-only/library callers and add a registry-fed
  variant for the live TUI/CLI so both paths use the same renderer.
