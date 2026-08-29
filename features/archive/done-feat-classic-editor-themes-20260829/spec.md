# Feature: feat-classic-editor-themes Six classic editor themes

## Basic information
- ID: feat-classic-editor-themes
- Priority: 70
- Workflow mode: lite
- Risk signals: none
- Dependencies: none
- Created: 2026-08-29T00:45:44Z

## User outcome
Ship six built-in editor-inspired themes with switching, persistence, documentation, and v0.6.4 release metadata.

## Scope and constraints

- Add `solarized-dark`, `solarized-light`, `dracula`, `nord`, `gruvbox-dark`, and
  `tokyo-night` as built-in themes using the existing `Theme` palette model.
- Keep every theme-name allowlist, parser, command, help text, persistence path,
  README, and relevant test synchronized.
- Bump all project version metadata to 0.6.4 and document the release.
- Do not add dependencies or change the existing theme persistence format.
- Do not stage `.claude/`, `package.json`, or `tests/test.py`.

## Acceptance scenarios

1. `/theme list` displays all ten built-in themes.
2. `/theme <new-name>` accepts and switches to each new theme.
3. Every new theme resolves to its own readable palette.
4. Each new theme name survives config persistence and reload.
5. Chinese and English READMEs and command help list all themes.
6. `cargo fmt --check`, `cargo test`, Clippy with denied warnings, and `cargo build` pass.
7. Version 0.6.4 metadata and release notes are ready for commit, tag, push, and
   GitHub Release publication.

## Technical notes

Follow the established `tsinghua`/`pku` implementation and tests. Use official
palette values with small terminal-readability adjustments where necessary.
