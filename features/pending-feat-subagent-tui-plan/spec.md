# Feature: feat-subagent-tui-plan Complete Subagent TUI experience

## Basic information
- ID: feat-subagent-tui-plan
- Priority: 50
- Workflow mode: deep
- Risk signals: multi_module, public_api, external_dependency, data_consistency, compatibility, parallel_split
- Dependencies: feat-background-task-cancel, feat-child-transcript-capability
- Plan phase: integration and complete-plan acceptance

## User outcome
The complete Markdown plan is delivered as one coherent experience: concise tool output, stable architecture, reliable official Subagent state, honest read-only inspection, and safe cancellation.

## Scope and constraints

- Reconcile the delivered child Features against every requirement and non-goal in the source plan.
- Run cross-feature compatibility, fallback, terminal interaction, and release verification.
- Resolve only integration defects; new user outcomes must become separate Features.
- Update README/help and the source plan to describe final supported behavior and explicit capability exclusions.
- Do not bypass child Feature verification or reinterpret a failed acceptance check as a warning.

## Acceptance scenarios

1. The parent transcript is concise while failures and user-requested output remain complete.
2. Fresh/resume/picker paths restore accurate Subagent state and live terminal states never regress.
3. `/agents` clearly separates Agents and Background work, remains read-only, and keeps input targeting the parent.
4. Only officially cancellable tasks with real task IDs can be cancelled, without stopping the parent.
5. Old-kernel and unavailable-method paths degrade safely.
6. Child transcript detail follows the capability decision and never falls back to private storage or fake functionality.
7. Formatting, Clippy, complete tests, release build, and interactive 80/120-column terminal checks pass.

## Technical notes

This is the logical root and final integration node of the Feature Map. Transitive prerequisites are intentionally represented by the DAG rather than duplicated as direct dependencies.
