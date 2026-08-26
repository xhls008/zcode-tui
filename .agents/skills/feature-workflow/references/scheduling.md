# DAG scheduling and parallel execution

Use this reference only for batch delivery, resume, or parallel Feature work.

## Scheduling

Read `queue.json`, select pending nodes whose dependencies are completed, sort by
priority, and respect `parallelism.max_concurrent`. Deep nodes must pass review
before dispatch. Never treat a blocked node as runnable.

## Codex collaboration

Parallel agents are optional, not a required lifecycle stage. Use them only when:

- the user authorized delegation or parallel agent work;
- at least two ready Features have non-overlapping worktrees and effects;
- each agent owns exactly one Feature lifecycle;
- the primary agent remains responsible for queue reconciliation and final
  reporting.

Do not have agents write `queue.json` directly at the same time. Each agent may
run runtime-script state commands, which serialize mutations with a file lock.

## Resume

For active entries, derive the resume point from durable evidence:

- incomplete tasks or uncommitted code: implement;
- tasks complete but verification absent/failed: verify;
- verification clean but not archived: complete;
- missing worktree: reconcile Git before changing queue state.

An interrupted scheduler does not require a second persistent scheduler state.
The queue, Git state, active documents, and verification artifact are sufficient
to reconstruct progress.

Stop batch execution when the queue is empty, every remaining node is blocked,
or a quality/authorization boundary requires user action. Do not convert the
latter into an infinite retry loop.
