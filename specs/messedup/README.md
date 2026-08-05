# Recovery Evidence Layout

`/_MESSEDUP.md` is the canonical active control file. It owns task status, dependencies,
assignments, blockers, and the current review queue.

`evidence/<task-id>.md` is the canonical submission and review record for one active task. Workers
replace the `Current Submission` section when remediation is requested. Arbiters update `Arbiter
Review`.

`archive/_MESSEDUP_FULL_2026-07-30.md` preserves the complete pre-split control and evidence record.
It is historical and must not receive new agent submissions.
