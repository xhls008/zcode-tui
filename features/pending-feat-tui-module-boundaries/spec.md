# Feature: feat-tui-module-boundaries Stable TUI module boundaries

## Basic information
- ID: feat-tui-module-boundaries
- Priority: 90
- Workflow mode: deep
- Risk signals: multi_module, compatibility, parallel_split
- Dependencies: feat-tool-output-clarity
- Plan phase: PR 2

## User outcome
Existing session, streaming, rewind, model, interaction, transcript, and rendering behavior remains stable while future Subagent work can be implemented in bounded, testable modules.

## Scope and constraints

- Continue the incremental extraction started by `src/agents.rs`.
- Establish clear app, protocol, transcript, agents, and UI ownership without an all-at-once rewrite.
- Introduce stable transcript entry IDs and remove long-lived state tied to `Vec` indices.
- Move protocol parsing and presentation projections toward independently testable pure functions.
- Preserve classic fallback, app-server streaming, create/resume, rewind, model, interaction, and inline viewport behavior.
- Do not create empty module trees before responsibilities are ready to move.
- New cross-domain logic must land in its owning module; `main.rs` remains an entry point/event-loop shell and `lib.rs` exposes only deliberate reusable APIs.

## Acceptance scenarios

1. Existing interaction flows behave identically before and after module extraction.
2. Transcript insert, delete, rewind, and resume paths no longer require persistent UI state to repair vector indices.
3. Protocol parsing, agents reduction, and transcript projection have focused unit tests outside the main event loop.
4. Rendering remains stable at 80 and 120 columns and after resize.
5. Formatting, Clippy, tests, and release build pass.
6. A checked-in responsibility map documents what moved out of `main.rs` and `lib.rs` and where future app, protocol, transcript, agents, and UI changes belong.

## Technical notes

`src/agents.rs` already owns background-task records, lifecycle reduction, and inspector selection state. Treat that as the first completed slice, then extract one coherent responsibility at a time.
