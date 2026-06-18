# Queue N - Title

This is the active working queue for a bounded delivery track. It is separate from
`specs/IMPLEMENTATION-PLAN.md`; do not use this file as a historical changelog.

## Goal

State the concrete outcome this queue owns. Keep the boundary narrow enough that another queue can own
adjacent work without overlap.

Queue type: Implementation | Research | Spec Review | Publication | Cleanup | Release

## Definition Of Done

State the closure test for this queue. The queue is complete only when the goal is achieved and the
Queue Closure Rules are satisfied.

## Completion State

Current state: Not Started | In Progress | Blocked | Waiting On Decision | Done | Cut

Current cursor: 10
Next task: First task

Decision Points: none.

If a decision is needed, replace the line above with numbered decision points using this format:

1. *Question*: State the owner decision in one sentence.
   *Context*: Name the source-backed facts that make the decision necessary.
   *Options* with Examples: List the viable choices and the tradeoff for each. Give concrete cases or paths affected by the choice.
   *Recommendation*: State the recommended choice and why.

## No Buried Work Rule

Before every status update, pause point, handoff, or final control-return message, audit the response for
future-tense work, prevention work, risks, blockers, follow-ups, or "should do next" statements.

If the response mentions work that is not already represented in the queue, do one of these before
handing off:

- Add it to the Active Focus Window and Ordered Task List when it is in current scope.
- Add it to Missed Or Hidden Work Found when it needs user vetting before promotion.
- Add it to Decision Points when user choice blocks the next action.
- Move it to another queue or planning document when it belongs outside this queue.

Do not describe future work only in chat. If it matters enough to mention, it must be represented in the
queue before control returns to the user.

## Decision Log

Record resolved decisions that affect this queue. This is not a changelog; keep only decisions needed
for future readers to understand why the queue is shaped this way.

| Date | Decision | Rationale | Source |
| --- | --- | --- | --- |
| YYYY-MM-DD | Decision made. | Why this was selected. | Thread, file, issue, or user instruction. |

## Source Authority Order

When sources disagree, resolve them in this order unless the user says otherwise:

1. Current repo source.
2. User decisions in the active thread.
3. Linked source-design docs.
4. Generated artifacts.
5. Agent inference.

## Assumptions

Keep assumptions separate from source-backed facts.

| Assumption | Why acceptable | Revisit trigger |
| --- | --- | --- |
| Assumption about target work. | Why it is safe to proceed. | What would make this assumption unsafe. |

## Current Source-Backed State

Summarize what exists in the tree today. Separate source-backed facts from target design, and cite the
authoritative file and line for every cross-boundary behavior this queue relies on.

Source evidence:

| Claim | Source |
| --- | --- |
| Claim about existing behavior. | `path/to/file.rs:1` |

## Scope Boundary

Queue N includes:

- Work item owned by this queue.

Queue N does not own:

- Adjacent work owned elsewhere.

Cut work must be moved to Missed Or Hidden Work Found or a follow-up queue before this queue closes.

## Priority Definitions

- P0: Blocks the queue goal.
- P1: Required for a correct, durable result.
- P2: Valuable follow-up that must be completed or explicitly re-homed before this queue closes.
- P3: Long-tail, low-to-medium ROI, or distant feature work that can be deferred with little to no consequence.

## Lift Scale

- 1: Trivial.
- 2-3: Small and clear.
- 4-5: Moderate and bounded.
- 6-8: Large or ambiguous; try to split before starting.
- 9+: Too large for one task; split before starting.

Try to break down tasks with a lift higher than 5 into smaller well-known tasks to reduce ambiguity.

## Research Notes

Record external specs, upstream examples, and repo-local design notes that shaped this queue. Keep this
section factual and short; execution details belong in the task table.

| Topic | Finding | Source |
| --- | --- | --- |
| Topic name | Short finding. | Link or local file path |

## Completion Evidence

List the commands, reviews, artifacts, user confirmations, or external checks required to prove this
queue is complete.

| Evidence | Required? | Result | Notes |
| --- | --- | --- | --- |
| Example verification command or artifact review. | Yes | Pending | Why it matters. |

## Ordered Task List

Current cursor: 10
Next task: First task

Status values: Not Started, In Progress, Blocked, Waiting On Decision, Done, Cut.
Evidence types: Source, Test, Review, Artifact, User Decision, External.

| Order | Status | Priority | Lift | Task | Owning specs | Depends on | Output | Verification | User input needed |
| --- | --- | --- | ---: | --- | --- | --- | --- | --- | --- |
| 10 | Not Started | P0 | 3 | First task | Spec or queue refs | None | Concrete source, spec, test, or artifact result. | Evidence type plus command, review, source citation, or artifact check. | No. |

## Missed Or Hidden Work Found

Record missed work, hidden target scope, stale claims, incomplete implementation, spec drift, or
surprising blockers here as soon as it is discovered.

Discovered work should not auto-expand scope unless it is a blocker. Do not promote discovered work into
the active queue without user approval or explicit queue-scope confirmation, unless it blocks completion
or verification of an existing active task.

Discovered work may be promoted without prior user approval only when it is:

- Blocking completion or verification of an active task.
- Required to repair safety, security, privacy, data-loss, or credential-exposure risk.
- Required to reconcile contradictory source-of-truth files.
- A mechanical consistency update caused by an approved rename, move, taxonomy change, or schema/vector
  change.
- Required to regenerate or sync artifacts derived from changed sources.
- A narrow dependency repair needed for an approved task.
- Explicitly covered by a user instruction such as "do whatever is needed to finish/post/release."

When using a carve-out, keep the promoted task as narrow as possible and record why it qualified.

Before final handoff, every item in this section must be promoted into the current queue, moved to
another queue or planning document, or cut/deferred with rationale.

Moved work format:

- Item:
- Moved to:
- Reason:
- Date:

## Risk Register

| Risk | Impact | Mitigation | Status |
| --- | --- | --- | --- |
| Risk that could affect queue completion. | Consequence if it happens. | How to reduce or handle it. | Open |

## Implementation Batch Map

| Batch | Tasks | Purpose |
| --- | --- | --- |
| Batch name | 10 | Why this batch exists. |

## Blocked Task Protocol

Blocked tasks must include:

- Blocking condition.
- Attempted resolution.
- Decision needed, if any.
- Next unblock action.

## Queue Closure Rules

Do not close this queue until:

- Every task is Done, Cut with rationale, or moved to another queue or planning document.
- Missed Or Hidden Work Found is empty, promoted, cut with rationale, or moved.
- Decision Points are resolved, cut with rationale, or moved.
- Completion Evidence is satisfied.
- Final Handoff is complete.

Do not reorder, reprioritize, or cut tasks without recording the reason. Ask the user before changing
P0/P1 priority unless it is a blocker carve-out.

## Final Handoff

Complete this section before closing the queue.

- Summary:
- Completed tasks:
- Cut or deferred tasks and where they moved:
- Decisions resolved:
- Completion evidence:
- Remaining risks:
- Files changed:

## Working Rules For This Queue

- Check the source before relying on a boundary.
- Keep source-backed state separate from target design.
- Update task status as work progresses; do not wait until the end to mark everything Done.
- Include the Active Focus Window and the single next action in every status update, pause point,
  handoff, or final control-return message.
- Apply the No Buried Work Rule before every status update, pause point, handoff, or final
  control-return message.
- Record discovered work immediately under Missed Or Hidden Work Found; promote it into the active queue
  only after user approval, explicit queue-scope confirmation, or a blocker carve-out.
- Try to break down tasks with a lift higher than 5 into smaller well-known tasks to reduce ambiguity.
- Do not use this file as a changelog.
- When a blocker appears, report it in chat with Question, Context, Examples, Options, and Recommendation.
