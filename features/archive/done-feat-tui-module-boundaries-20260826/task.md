# Tasks: feat-tui-module-boundaries

- [x] Extract background-task model, reducer, and inspector selection into `src/agents.rs`.
- [x] Inventory every responsibility currently concentrated in `src/main.rs` and `src/lib.rs`, then record an explicit source-to-target migration map before moving code.
- [x] Establish the `app` boundary with composer/input layout and application state primitives while keeping root orchestration explicit.
- [x] Split app-server/classic/V4 protocol types and decoding into the `protocol` boundary; keep `lib.rs` focused on intentional reusable public APIs rather than unrelated feature logic.
- [x] Introduce stable transcript entry IDs and migrate the streaming assistant target away from a long-lived vector index.
- [x] Move transcript model/projection into `transcript`, and conversation/composer/agents/theme rendering into `ui`, in behavior-preserving slices.
- [x] Add module ownership documentation and tests that make future features land in the owning module instead of growing `main.rs` or `lib.rs` again.
- [x] Verify streaming, fallback, resume, rewind, model, interaction, width, and resize compatibility.

## Progress log

- 2026-08-26: First agents-domain extraction completed before Feature Map initialization; 119 tests, Clippy, formatting, and release build passed.
- 2026-08-26: Moved 2,635 protocol lines out of `lib.rs`; extracted app input/state, transcript identity/presentation, and UI conversation/composer/agents/theme modules; documented ownership in `docs/architecture.md`.
