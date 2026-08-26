# Planning and risk routing

Use this reference for initialization, feature creation, splitting, enrichment,
or specification review.

## Create

1. Extract a short feature name, user-facing outcome, priority, dependencies,
   and testable acceptance scenarios from the request.
2. Identify concrete risk signals from the configured list. Do not infer Deep
   from vague importance.
3. Create state and baseline documents atomically:

```bash
python3 feature-workflow/scripts/workflow.py create \
  --id feat-example \
  --name "Example outcome" \
  --description "User-visible result" \
  --priority 50 \
  --mode auto \
  --signals public_api,compatibility \
  --dependencies feat-prerequisite
```

The script chooses Lite unless at least one configured Deep signal is present.
An explicit `--mode lite|deep` is allowed when the user selected it.

After creation, replace document placeholders with concrete boundaries,
acceptance scenarios, and only the task detail useful for recovery.

## Split

Split only when the request contains multiple independently deliverable user
outcomes or exceeds a single context/recovery boundary. Split vertically by user
value, not into database/API/UI layers. Record dependencies as a DAG and ensure
every child can be verified independently.

A split parent and its children are Deep because `parallel_split` is a risk
signal. Create each child with the script and keep all dependency IDs valid.
Do not perform ad-hoc concurrent JSON writes.

## Deep preparation

For Deep features:

1. Read only the archive index first.
2. Load full historical records only for a strong architecture, migration, or
   compatibility match.
3. Enrich the spec with missing boundaries and concrete risks, not prose volume.
4. Review clarity, completeness, consistency, feasibility, acceptance quality,
   and operational risk.
5. Save `features/pending-{id}/review-status.json`:

```json
{
  "schema": "feature-review/v1",
  "feature_id": "feat-example",
  "status": "passed",
  "score": 82,
  "reviewed_at": "2026-08-25T10:00:00Z",
  "blocking_issues": []
}
```

Deep start is blocked when this artifact is missing, failed, or below
`workflow.review.min_score`.

Lite features do not require enrich, review, or deep archive loading.
