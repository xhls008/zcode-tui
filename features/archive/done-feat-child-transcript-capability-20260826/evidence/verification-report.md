# Verification report: feat-child-transcript-capability

Verified 2026-08-26 against local ZCode 0.16.3 and in the isolated Feature worktree.

## Capability result

- Public `session/subagents` exposed both running children and ended children with final summaries.
- `session/messages` and `session/events` returned `-32004 Session is not active` for a child while it was reported running under its parent.
- The same candidate reads failed for an ended child; `afterSeq`/`limit` did not change the result.
- Explicit bare `session/resume` made an ended child's four messages and two events readable.
- The resume result retained `sessionKind: subagent_child` and `parentSessionId`; parent `childSessionIds` and ended status were unchanged in the before/after observation.
- Resume also materialized the child as an idle active session whose V4 input routing was `startNow`; it is therefore not a read-only inspection operation.

Decision: full child transcript is unsupported in the Inspector until an official read-only inactive-child API exists. Keep public summary/output-tail display, never auto-resume, never direct child input, and never read SQLite.

Sanitized evidence is committed in `tests/fixtures/zcode-0.16.3-child-transcript.json`; the decision record is `docs/child-transcript-capability.md`.

## Executable verification

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo test`: passed; 16 binary tests and 112 integration tests (128 total).
- `cargo build --release`: passed.
- The pinned 0.16.3 `running[]` and `ended.items[]` fixture is parsed by an automated regression test.

## Acceptance scenarios

All five Feature scenarios passed. The live probe used disposable sessions and public RPCs only, did not read API keys or database tables, and left no running child work.
