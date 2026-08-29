# Feature: feat-theme-registry-accessible Registry-backed accessible themes

## Basic information
- ID: feat-theme-registry-accessible
- Priority: 90
- Workflow mode: deep
- Risk signals: multi_module, compatibility
- Dependencies: none
- Created: 2026-08-29T05:53:39Z

## User outcome
Ship v0.6.5 with one theme registry, an accessible palette, corrected themed colors, tests, docs, and release artifacts

## Scope and constraints

- Replace every runtime theme-name allowlist with one registry containing each
  built-in name, display label, and complete palette. Theme parsing, saving,
  switching, listing, errors, and help must derive from that registry.
- Preserve the existing ten names, unknown-name rejection semantics, dark
  fallback behavior in palette dispatch, config format, and `/theme` behavior.
- Add the `accessible` dark palette from Okabe-Ito colors and make it available
  through switching and persistence.
- Add the missing `selection_fg` theme/config token, give all eleven palettes
  readable selections, correct light-theme code contrast, and replace reusable
  hard-coded UI colors with existing theme tokens.
- Update the Chinese/English documentation, changelog, Cargo metadata, and lock
  metadata for v0.6.5 without adding dependencies.
- Do not stage the user-owned `.claude/`, `package.json`, or `tests/test.py`.
- Publish only after fmt, tests, strict Clippy, build, and workflow completion
  validation pass; then push `main` and `v0.6.5` and create the requested release.

## Acceptance scenarios

1. The registry exposes exactly eleven unique built-in names and every entry
   resolves to its registered palette; an unknown name is rejected by config,
   save, and command validation while palette dispatch retains dark fallback.
2. Runtime help, `/theme list`, command errors, config parsing, and persistence
   obtain their names from the registry and therefore cannot drift from it.
3. `/theme accessible` uses #0072B2 as its accent and Okabe-Ito semantic colors,
   persists to the current config format, and reloads successfully.
4. Every theme supplies `selection_fg`; overrides accept the token and selected
   rows use it instead of a hard-coded foreground.
5. `light` and `solarized-light` keep readable code text and syntax colors on
   their light code backgrounds; the official icon no longer forces dark colors.
6. README.md, README.en.md, Cargo.toml/Cargo.lock, and CHANGELOG.md describe the
   eleven themes and v0.6.5, including the one-place registry extension rule.
7. `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets -- -D warnings`,
   `cargo build`, focused release checks, and the workflow hard gate pass.
8. Only scoped files are committed; `main` and tag `v0.6.5` reach `origin`, and
   GitHub Release v0.6.5 is published with the requested notes.

## Technical notes

- Keep the registry in the library crate so both config/help code and the TUI
  binary consume the same data. The TUI `Theme` remains the Ratatui conversion
  layer and applies user overrides.
- Reuse the existing palette model and syntect output. For light code panels,
  adjust emitted syntax RGB values at render time to retain hue while restoring
  contrast, rather than introducing another parser or dependency.
- Reuse `frame`, `text`, and `code_bg` for the official icon instead of adding
  icon-only tokens.
