# Versioned Decisioning Pattern

Use Versioned Decisioning when agent decisions and tasks need to be inspectable, reversible, and reusable.

Every meaningful decision should become a durable artifact:

- User request
- Task state
- Decision state
- Options considered
- Chosen path
- Rejected paths
- Evidence
- Tool outputs
- Human approvals
- Superseded decisions

## Rules

- Do not keep important decision state only in chat.
- Record why a decision was made, not only what changed.
- Link every task to the decision that created or changed it.
- Link every decision to evidence.
- When a decision is replaced, mark the old decision as superseded instead of deleting it.
- If the user asks to revert a task or decision, locate the relevant decision node and restore the prior accepted state.

## Agent Behavior

Before acting:

1. Record or find the active task.
2. Record the current decision point.
3. Attach relevant evidence.
4. Proceed only after the task and decision are traceable.

After acting:

1. Record the result.
2. Attach verification evidence.
3. Mark the decision accepted, rejected, superseded, or needing human review.

The goal is not bureaucracy. The goal is to make agent work auditable and reversible.
