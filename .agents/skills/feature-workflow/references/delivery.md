# Feature delivery

Use this reference to start, implement, verify, resume, or complete one Feature.

## Start

1. Inspect state with `workflow.py list --json`.
2. Confirm dependencies and the Deep review gate.
3. Create the configured branch and isolated worktree with Git.
4. Only after Git succeeds, transition business state:

```bash
python3 feature-workflow/scripts/workflow.py start-state \
  --id feat-example \
  --branch feature/feat-example \
  --worktree ../project-feat-example
```

If state transition fails, remove only the newly created worktree/branch after
resolving their exact paths.

## Implement

Read the active spec, task record, repository instructions, and relevant code.
Search and plan naturally inside the Feature boundary. Implement the smallest
complete vertical slice, add proportionate tests, and maintain `task.md` as a
recovery record. Do not touch other Feature documents.

## Verify

Run configured lint, type checks, unit/integration tests, and every acceptance
scenario. UI scenarios require real browser evidence when the project supports
it. Save a human report plus this canonical artifact:

`features/active-{id}/evidence/verification-status.json`

```json
{
  "schema": "feature-verification/v1",
  "feature_id": "feat-example",
  "status": "passed",
  "verified_at": "2026-08-25T10:30:00Z",
  "tasks": {"total": 4, "completed": 4, "incomplete": 0},
  "quality": {"status": "passed", "passed": 2, "failed": 0},
  "tests": {"status": "passed", "passed": 12, "failed": 0, "skipped": 0},
  "scenarios": {"total": 3, "passed": 3, "failed": 0},
  "blocking_failures": [],
  "warnings": [],
  "report": "features/active-feat-example/evidence/verification-report.md"
}
```

On blocking failure, persist diagnostics and run:

```bash
python3 feature-workflow/scripts/workflow.py needs-attention \
  --id feat-example --reason "verification failed"
```

Fix and retry only within the requested scope. Do not complete.

## Complete

1. Run the hard gate:

```bash
python3 feature-workflow/scripts/workflow.py validate-completion --id feat-example
```

2. Commit in the feature worktree.
3. Fetch/pull only when authorized and configured. Rebase onto the local main
   branch. If conflict resolution changes code, verify again.
4. Run the hard gate again immediately before merge.
5. Merge locally, create the configured local tag, and copy Feature documents
   plus evidence to `features/archive/done-{id}-{date}`. The archive must
   contain both `evidence/verification-status.json` and
   `evidence/verification-report.md`.
6. Record completion state before cleanup. `complete-state` must normalize the
   copied verification status so its `report` points inside `archive_path`, use
   that normalized status in both queue and archive indexes, and fail if the
   referenced archived report does not exist:

```bash
python3 feature-workflow/scripts/workflow.py complete-state \
  --id feat-example \
  --archive-path features/archive/done-feat-example-20260825 \
  --tag feat-example-20260825 \
  --merge-commit <hash>
```

7. Re-read the completed entries in `feature-workflow/queue.json` and
   `features/archive/archive.json`. Confirm their verification report paths are
   identical, contain no `active-*` segment, and resolve to files before
   removing the active Feature documents.
8. Remove only the resolved Feature worktree and branch when requested by config
   and authorized by the task. A cleanup failure is recoverable and must not
   invalidate the archive record.
9. Push main or tags only with explicit authorization.

`--skip-checklist` is an explicit manual override for checklist completeness;
it never bypasses verification.
