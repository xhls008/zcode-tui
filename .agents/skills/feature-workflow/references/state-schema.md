# State and archive operations

Use this reference to inspect, list, block, unblock, or query Feature state.

## Sources of truth

- `feature-workflow/config.json`: policy and project configuration.
- `feature-workflow/queue.json`: pending, active, blocked, completed business state.
- Git branches/worktrees/commits: code state.
- `features/archive/archive.json`: searchable completed-feature index.
- `features/archive/done-*`: persistent documents and evidence.

Do not introduce another durable workflow-state file. Runtime locks are
short-lived coordination artifacts, not business state.

## Commands

```bash
python3 feature-workflow/scripts/workflow.py list
python3 feature-workflow/scripts/workflow.py list --json
python3 feature-workflow/scripts/workflow.py block --id feat-example --reason "waiting for API contract"
python3 feature-workflow/scripts/workflow.py unblock --id feat-example
```

To query archives, read `archive.json` first and filter by ID, name, keywords,
category, related features, verification status, or date. Load a full archive
directory only when its index entry is a strong match for the question.

## Reconciliation

When queue, Git, and disk disagree, do not guess:

1. Resolve the exact Feature ID and all matching paths/refs.
2. Prefer queue for intended business state and Git for actual code state.
3. Repair the minimum inconsistent record without deleting recoverable data.
4. Preserve a diagnostic note when intervention was required.

Completion records are idempotent by Feature ID. Rerunning `complete-state`
updates the existing archive record instead of appending a duplicate.

For every completed Feature, reconcile all three copies of verification state:

- `feature-workflow/queue.json`
- `features/archive/archive.json`
- `{archive_path}/evidence/verification-status.json`

Their `report` values must match, must resolve to the archived verification
report, and must not reference an `active-*` directory. A completed count alone
is not a clean archive audit.
