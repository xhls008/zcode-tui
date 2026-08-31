---
name: feature-workflow
description: Manage explicitly requested or existing repository Features with persistent queue state, Lite/Deep risk routing, isolated worktrees, gated verification, and resumable delivery. If considered implicitly for an ordinary change, route it to Direct delivery and never create Feature state.
---

# Feature Workflow

Use the smallest delivery boundary that preserves the user's intent. Direct
changes are the default for ordinary work; Feature is the unit of persistent
planning, recovery, scheduling, and delivery only when that lifecycle is
actually wanted.

## Activation gate

Skill selection and Feature creation are separate decisions. Implicit selection
must never create queue state, Feature documents, branches, worktrees, tags, or
archives by itself.

Use the **Direct path** when the user asks for a bounded implementation, fix,
refactor, documentation update, build adjustment, or other coherent change and
does not explicitly ask for a Feature/workflow lifecycle. Direct remains valid
when the change touches several related files, needs tests, or is followed by a
commit/build/install. Multiple small fixes in the same working context may also
stay Direct.

Use a **Managed Feature** only when at least one is true:

- the user explicitly asks to create/use/start a Feature or the Feature
  Workflow;
- the request names or continues an existing queue Feature;
- the user asks for persistent scheduling, a Feature Map/DAG, resumable staged
  delivery, independent worktrees, Feature archives, or Feature-level parallel
  delegation;
- the task grows beyond a direct recovery boundary and the user agrees to
  promote it.

Merely calling product behavior a "feature" or "feature request" is not an
explicit workflow request. The user must ask to create, initialize, schedule,
start, or manage it as a Feature, or refer to existing Feature state.

Complexity, risk signals, or the mere presence of `feature-workflow/` do not
authorize silent promotion. If a request no longer fits Direct, explain the
specific reason and ask before creating Feature state.

### Direct path

For Direct work:

1. Inspect the relevant repository state and preserve unrelated user changes.
2. Implement the smallest complete change in the current worktree.
3. Verify proportionally to risk.
4. Commit, build, install, push, or otherwise mutate external state only when
   requested or already authorized by the task.

Do not initialize the workflow, call `workflow.py`, mutate `queue.json`, create
Feature documents, open a Feature worktree, run Feature review gates, create a
Feature tag, or archive delivery evidence. Ordinary test output and a concise
handoff are sufficient.

## First use

This section applies only after the activation gate selects Managed Feature. If
`feature-workflow/config.json` is absent, run the bundled script from this
plugin's `scripts/workflow.py`:

```bash
python3 <plugin-root>/scripts/workflow.py --root "$(git rev-parse --show-toplevel)" init
```

Initialization copies the runtime script into the repository. Afterwards use:

```bash
python3 feature-workflow/scripts/workflow.py <command>
```

Do not overwrite an initialized workflow unless the user explicitly requests
reinitialization.

## Route by intent

- Direct implementation: use the Direct path above; do not read managed
  lifecycle references.
- Create, split, enrich, or review a feature: read
  [references/planning.md](references/planning.md).
- Start, implement, verify, resume, or complete one feature: read
  [references/delivery.md](references/delivery.md).
- Schedule a DAG or use parallel agents: read
  [references/scheduling.md](references/scheduling.md).
- List, inspect, block, unblock, or query archives: read
  [references/state-schema.md](references/state-schema.md).

Read only the references needed for the current request.

## Invariants

- Direct work has no Feature business state and must not appear in the queue or
  archive.
- Never silently promote Direct work into a Managed Feature.
- `feature-workflow/queue.json` is the business-state source of truth.
- Git is the code-state source of truth.
- `features/archive/archive.json` and Git tags are persistent delivery records.
- Legacy queue entries without `workflow_mode` are treated as `lite`.
- Lite uses `start → implement → verify → complete`.
- Deep adds only the planning/review capabilities justified by recorded risk
  signals, then converges on the same lifecycle.
- Never complete a feature unless the executable verification gate passes.
- Completed verification records must reference evidence inside their
  `archive_path`; no completed record may retain an `active-*` report path, and
  every referenced report must exist.
- Never turn positive test, type, quality, task, or acceptance failures into a
  warning.
- Never merge, push, delete a worktree, or dispatch agents merely because this
  skill was loaded. Those actions require the user's task to include them.
- Remote pushes default off. Enabling a config flag does not itself authorize a
  push in the current task.

## Codex-native behavior

Use Codex's own repository search, planning, and code-editing abilities inside a
Feature boundary. Do not recreate them as mandatory micro-stages. Use the plan
tool for meaningful multi-step work; use collaboration agents only when the user
authorized delegation and independent Feature nodes can safely run in parallel.

Prefer the deterministic runtime script for queue mutations and completion
validation. Use `apply_patch` for project documents and ordinary source edits.
