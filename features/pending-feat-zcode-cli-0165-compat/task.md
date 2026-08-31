# Tasks: feat-zcode-cli-0165-compat

- [ ] Verify and extract the official ZCode 3.9.1 / CLI 0.16.5 package
- [ ] Compare 0.16.3 and 0.16.5 command and app-server protocol inventories
- [ ] Execute the 0.16.5 app-server initialization path used by zcode-tui
- [ ] Add focused 0.16.5 protocol fixtures and only necessary decoder changes
- [ ] Update both READMEs, design baseline, and changelog with proven results
- [ ] Run full Rust quality gates and save verification evidence

## Progress log

- 2026-08-31: Routed Deep for external dependency and protocol compatibility.
  Chose a temporary, checksum-verified vendor inspection plus minimal existing
  Rust fixture coverage; no bundled vendor code or new dependency will be kept.
