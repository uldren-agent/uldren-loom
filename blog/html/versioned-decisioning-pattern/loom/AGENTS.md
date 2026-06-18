# Loom Versioned Decisioning

This file assumes Loom is available as a finished decision and task tracking system through a Loom MCP server.

Use Loom as the durable state layer for agent tasks and decisions. Treat Loom as the source of truth for task status, decision history, evidence, and rollback points.

## Required Behavior

When a user gives you work:

1. Create or find a Loom task.
2. Record the user request as the root decision input.
3. Break the work into task nodes when useful.
4. Record every meaningful decision as a decision node.
5. Attach evidence to the decision or task that used it.
6. Update task state as work proceeds.
7. Preserve rejected options and superseded decisions.

## MCP Expectations

The Loom MCP should expose drop-in replacements for common agent task tracking tools.

At minimum, it should support operations equivalent to:

- Create task
- Update task
- List tasks
- Mark task blocked
- Mark task complete
- Create decision
- Attach evidence
- Link decision to task
- Supersede decision
- Inspect task tree
- Inspect decision tree
- Revert task
- Revert decision

## Decision Tree

Store decisions as a tree or graph:

- A task can have child tasks.
- A decision can create tasks.
- A decision can supersede another decision.
- Evidence can attach to a task, a decision, or a verification result.
- Human approvals are decision nodes.
- Reverts create new decision nodes that point to the reverted state.

## Revert Behavior

If the user asks to revert a task or decision:

1. Ask Loom to inspect the relevant task or decision tree.
2. Identify the last accepted state before the target decision.
3. Explain what will be reverted.
4. Ask for confirmation when the revert is destructive or affects external systems.
5. Use Loom to create a revert decision.
6. Restore or instruct the relevant tool to restore the prior state.
7. Attach evidence that the revert completed.

## User-Facing Guarantees

When using Loom, tell the user:

- Which task is active.
- Which decision is being made.
- Which evidence supports the decision.
- Which prior decision can be reverted if needed.
- Which decisions still need human review.

This gives the agent introspectability, traceability, and versioning over tasks and decisions.
