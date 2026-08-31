# Tasks: feat-zcode-cli-0165-compat

- [x] Verify and extract the official ZCode 3.9.1 / CLI 0.16.5 package
- [x] Compare 0.16.3 and 0.16.5 command and app-server protocol inventories
- [x] Execute the 0.16.5 app-server initialization path used by zcode-tui
- [x] Add focused 0.16.5 protocol fixtures and only necessary decoder changes
- [x] Update both READMEs, design baseline, and changelog with proven results
- [x] Run full Rust quality gates and save verification evidence

## Progress log

- 2026-08-31: Routed Deep for external dependency and protocol compatibility.
  Chose a temporary, checksum-verified vendor inspection plus minimal existing
  Rust fixture coverage; no bundled vendor code or new dependency will be kept.
- 2026-08-31: Verified official ZCode 3.9.1-5853 against the feed SHA-512;
  its bundled CLI reports 0.16.5. Compared it with the verified 3.7.7 / 0.16.3
  baseline: the public CLI adds only `--surface`; app-server removes no methods
  and adds official MCP auth headers, workspace hook trust, model-I/O retention,
  and generate-text cancellation. The runtime-preferences strict schema is
  unchanged. A live 0.16.5 `session/create` completed with the existing reply.
  The unversioned feed briefly returned 3.7.7 from one CDN edge during analysis,
  while the versioned package URL and subsequent plain/cache-busted feed reads
  consistently returned 3.9.1.
- 2026-08-31: Passed formatting, 160/160 Rust tests, zero-warning Clippy,
  release build, and diff whitespace checks. Saved verification evidence.
