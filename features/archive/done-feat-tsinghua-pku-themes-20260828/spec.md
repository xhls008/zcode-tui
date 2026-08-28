# Feature: feat-tsinghua-pku-themes Tsinghua purple and PKU red themes

## Basic information
- ID: feat-tsinghua-pku-themes
- Priority: 1
- Workflow mode: lite
- Risk signals: none
- Dependencies: none
- Created: 2026-08-28T15:24:06Z

## User outcome
Add selectable and persistent tsinghua and pku TUI themes, extend theme tests and documentation, and release v0.6.3.

## Scope and constraints

- Add `tsinghua` (清华紫) and `pku` (北大红) as built-in theme names alongside
  `dark` and `light`.
- Give each new theme a coherent full palette for accent, text, status, frame,
  code background, and band background colors.
- Reuse the existing `/theme` command and UI-config persistence path; preserve
  comments, unknown settings, line endings, and color-token overrides.
- Update Chinese and English README documentation, version metadata, and the
  changelog for v0.6.3.
- Do not touch or stage `.claude/`, `package.json`, or `tests/test.py`.

## Acceptance scenarios

1. `/theme` and `/theme list` show all four built-in themes, including Chinese
   aliases for `tsinghua` and `pku`.
2. `/theme tsinghua` and `/theme pku` apply distinct palettes immediately and
   persist through the existing config-file behavior.
3. The persistence API accepts all four built-in names and rejects unknown
   names without changing the file.
4. Existing `dark` and `light`, color overrides, and plain/no-color behavior
   remain unchanged.
5. README files and CHANGELOG document the new themes and v0.6.3.
6. `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets -- -D warnings`,
   and `cargo build` pass.

## Technical notes

- Extend the existing string matches and `Theme` constructors only; add no new
  dependency or theme registry abstraction.
- Cover public persistence behavior in `tests/core.rs` and palette dispatch in
  the existing unit test beside `Theme`.
