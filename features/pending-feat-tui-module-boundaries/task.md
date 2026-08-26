# Tasks: feat-tui-module-boundaries

- [x] Extract background-task model, reducer, and inspector selection into `src/agents.rs`.
- [ ] Introduce stable transcript entry IDs and migrate index-coupled state.
- [ ] Extract protocol types/parsing from general library utilities where boundaries are clear.
- [ ] Extract transcript presentation and UI rendering in behavior-preserving slices.
- [ ] Verify streaming, fallback, resume, rewind, model, interaction, width, and resize compatibility.

## Progress log

- 2026-08-26: First agents-domain extraction completed before Feature Map initialization; 119 tests, Clippy, formatting, and release build passed.
