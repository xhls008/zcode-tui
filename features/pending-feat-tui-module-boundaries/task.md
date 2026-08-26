# Tasks: feat-tui-module-boundaries

- [x] Extract background-task model, reducer, and inspector selection into `src/agents.rs`.
- [ ] Inventory every responsibility currently concentrated in `src/main.rs` and `src/lib.rs`, then record an explicit source-to-target migration map before moving code.
- [ ] Split application composition, state, input routing, and update transitions into the `app` boundary; keep `main.rs` focused on startup, terminal lifecycle, and the top-level event loop.
- [ ] Split app-server/classic/V4 protocol types and decoding into the `protocol` boundary; keep `lib.rs` focused on intentional reusable public APIs rather than unrelated feature logic.
- [ ] Introduce stable transcript entry IDs and migrate index-coupled state.
- [ ] Move transcript model/projection into `transcript`, and conversation/composer/agents/theme rendering into `ui`, in behavior-preserving slices.
- [ ] Add module ownership documentation and tests that make future features land in the owning module instead of growing `main.rs` or `lib.rs` again.
- [ ] Verify streaming, fallback, resume, rewind, model, interaction, width, and resize compatibility.

## Progress log

- 2026-08-26: First agents-domain extraction completed before Feature Map initialization; 119 tests, Clippy, formatting, and release build passed.
