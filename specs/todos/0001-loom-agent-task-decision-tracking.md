# Loom MCP task and decision tracking

## Idea

Explore Loom as a native task and decision tracking layer for agent harnesses.

The core idea is to expose a Loom MCP server that can act as a drop-in replacement for common agent task tracking tools while extending them with versioned decision state.

## Motivation

Agent systems often track tasks as a flat checklist and keep decision rationale in the chat transcript. That makes it hard to inspect why a task exists, what evidence shaped it, which decision superseded it, or how to revert only one branch of work.

Loom could provide a durable task and decision tree:

- Tasks can have child tasks.
- Decisions can create or update tasks.
- Evidence can attach to tasks, decisions, or verification results.
- Human approvals can be stored as decision nodes.
- Reverts can create new decision nodes that point to prior accepted state.

## Candidate MCP surface

The MCP server should provide operations equivalent to:

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

## Agent integration

The integration should support:

- AGENTS.md guidance for portable fallback behavior.
- Skills for reusable decisioning workflows.
- Plugins for installable distribution.
- Hooks where a host supports automatic task and decision capture.
- Subagent role mapping for Model Relay workflows.

## Open questions

- What is the minimal task and decision schema?
- Which task-tracking tool shapes should Loom mimic first?
- How should revert behave when external side effects have already happened?
- What evidence should be mandatory before a decision can be accepted?
- How should users inspect and approve a revert?
- How should this map to Claude, Codex, Hermes, OpenClaw, and other harnesses without coupling to one host?
