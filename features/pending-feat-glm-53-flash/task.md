# Tasks: feat-glm-53-flash

- [ ] Add a Desktop-aware runtime model builder with safe CLI fallback
- [ ] Attach enriched runtime models to create and resume requests
- [ ] Merge enriched choices into the startup and live `/model` catalog
- [ ] Add regression tests for metadata, fallback, and request wiring
- [ ] Validate against a real local app-server without exposing credentials
- [ ] Run full quality gates and produce archived verification evidence

## Progress log

- 2026-08-27: Confirmed CLI `workspace/readState` omits Flash while Desktop v2 provider registry contains it; root cause is missing dynamic registry synchronization in standalone TUI sessions.
