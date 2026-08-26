# ZCode TUI module ownership

This map is the routing rule for new code. `main.rs` and `lib.rs` are entry
surfaces, not default destinations for unrelated feature logic.

## Current responsibility map

| Concern | Owning module | Migrated from | Rule for new work |
| --- | --- | --- | --- |
| Binary startup, terminal lifecycle, top-level event loop | `src/main.rs` | existing binary | Keep orchestration only; delegate domain state and pure transformations. |
| Composer wrapping and popup geometry | `src/app/input.rs` | `src/main.rs` | Input-to-layout or input-to-action logic belongs under `app`. |
| App-server/V4/connection/database availability state primitives | `src/app/state.rs` | `src/main.rs` | Cross-loop application state types belong under `app`; orchestration remains at the root until a behavior-preserving slice can move with its tests. |
| Background task/Subagent model and inspector selection | `src/agents.rs` | `src/main.rs` | Agent identity, lifecycle reduction, and selection state land here. |
| Stable transcript identity and entry kinds | `src/transcript/model.rs` | `src/main.rs` | Persistent transcript state must use `EntryId`, never a long-lived vector index. |
| Conversation, composer, and Agent Inspector rendering | `src/ui/conversation.rs`, `src/ui/composer.rs`, `src/ui/agents.rs` | `src/main.rs` | Renderers read state only and never send protocol requests. |
| Theme tokens and style construction | `src/ui/theme.rs` | `src/main.rs` | Rendering style belongs under `ui`. |
| Public app-server/V4 shapes, decoding, reducers, and stdio client | `src/protocol.rs` | `src/lib.rs` | Official wire changes and compatibility parsing land here. |
| Reusable CLI/config/history/markdown/database helpers | `src/lib.rs` | existing library | Keep only deliberate reusable APIs; new protocol implementation does not return here. |

## Dependency direction

```text
main orchestration
  ├─ app input/layout
  ├─ transcript model
  ├─ agents state
  ├─ ui theme/rendering
  └─ zcode_tui public library
       └─ protocol
```

- `ui` reads state and renders; it does not issue app-server requests.
- `agents` and `transcript` own stable domain state; `main` coordinates them.
- `protocol` owns wire compatibility and exposes typed/public operations to the binary.
- Cross-domain additions require an explicit owner in this table before implementation.

## Incremental migration policy

Large functions move only with their tests and without behavior changes. A module may
remain a single file until it has at least two independently meaningful responsibilities;
empty directory scaffolding is not a deliverable. Future extraction candidates are app
state/update transitions and the remaining transcript presentation helpers and overlays.
