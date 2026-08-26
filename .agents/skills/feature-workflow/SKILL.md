---
name: feature-workflow
description: Manage feature-driven software delivery with persistent queue state, Lite/Deep risk routing, isolated Git worktrees, gated verification, resumable execution, and optional parallel agents. Use when creating, scheduling, implementing, verifying, completing, blocking, resuming, or auditing repository features. Do not use for a one-off code edit that does not need persistent feature state.
---

# Feature Workflow

Use Feature as the unit of user value, context, recovery, and delivery. Keep the
default path light; make state transitions and irreversible effects explicit.

## First use

If `feature-workflow/config.json` is absent, run the bundled script from this
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
