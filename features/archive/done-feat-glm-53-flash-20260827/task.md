# Tasks: feat-glm-53-flash

- [x] Add a Desktop-aware runtime model builder with safe CLI fallback
- [x] Attach enriched runtime models to create and resume requests
- [x] Merge enriched choices into the startup and live `/model` catalog
- [x] Add regression tests for metadata, fallback, and request wiring
- [x] Validate against a real local app-server without exposing credentials
- [x] Run required quality gates and produce archived verification evidence

## Progress log

- 2026-08-27: Confirmed CLI `workspace/readState` omits Flash while Desktop v2 provider registry contains it; root cause is missing dynamic registry synchronization in standalone TUI sessions.
- 2026-08-27: Added Desktop metadata merging while retaining CLI credentials and endpoints.
- 2026-08-27: Confirmed `runtimeModel` alone is insufficient; added the Desktop-equivalent `workspace/updateProviderRegistry` phase before create/resume.
- 2026-08-27: A real local app-server exposed and accepted `glm-5.3-flash` after registration.
- 2026-08-27: Rust tests, Clippy, release build, and focused PTY checks passed. The user elected to perform the broad interactive regression manually.
