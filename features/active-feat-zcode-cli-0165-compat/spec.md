# Feature: feat-zcode-cli-0165-compat ZCode CLI 0.16.5 compatibility evidence

## Basic information
- ID: feat-zcode-cli-0165-compat
- Priority: 90
- Workflow mode: deep
- Risk signals: external_dependency, compatibility
- Dependencies: none
- Created: 2026-08-31T06:48:36Z

## User outcome
The repository records the official CLI 0.16.5 protocol delta and has an executable regression test for every newly relevant wire shape.

## Scope and constraints

- Obtain ZCode 3.9.1 only from the official Linux x64 update feed, verify the
  feed SHA-512, and inspect the bundled CLI 0.16.5 in a temporary directory.
  Do not install the desktop package, replace `/opt/ZCode`, or commit vendor
  binaries/minified bundles.
- Compare CLI 0.16.5 with the installed CLI 0.16.3 at three useful boundaries:
  public command surface, app-server method/event string inventory, and the
  live initialization/session protocol exercised by zcode-tui.
- Add the smallest deterministic Rust regression fixture for each newly
  relevant wire shape. Reuse the existing protocol decoders and integration
  test file; add production code only when a real 0.16.5 shape is rejected.
- Record confirmed upgrades separately from bundle-internal churn or guesses.
  Do not claim authenticated provider/model behavior without executing it.
- Update the Chinese/English compatibility baseline and design note only after
  executable evidence passes. Do not bump zcode-tui's version or publish.
- Preserve and never stage `.claude/`, `package.json`, or `tests/test.py`.

## Acceptance scenarios

1. The official ZCode 3.9.1 deb validates against its feed SHA-512 and reports
   CLI `0.16.5`; all downloaded/extracted files remain outside the repository.
2. A reproducible comparison identifies user-visible CLI option changes and
   app-server method/event additions/removals between 0.16.3 and 0.16.5.
3. The actual CLI 0.16.5 app-server completes the initialization/runtime
   preferences path used by zcode-tui without exposing credentials in logs.
4. Focused Rust fixture tests pin every newly relevant 0.16.5 protocol shape
   and fail if the existing decoder regresses.
5. Existing protocol compatibility tests and the full Rust suite continue to
   pass with formatting and Clippy warnings denied.
6. README Chinese/English and the design baseline state exactly what was
   verified for ZCode 3.9.1 / CLI 0.16.5 and what remains unverified.

## Technical notes

- Current verified baseline: ZCode 3.8.1 / CLI 0.16.3; the locally installed
  ZCode 3.7.7 package also reports CLI 0.16.3.
- Official 3.9.1 package discovery already confirmed build `3.9.1-5853` and
  CLI `0.16.5`; this Feature must repeat checksum verification as evidence.
- Use stdlib/shell extraction and existing Rust tests. No new dependency or
  general-purpose schema-diff framework is justified.
