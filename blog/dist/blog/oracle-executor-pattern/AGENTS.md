# Oracle Executor Pattern

Use the Oracle Executor Pattern when an AI system may propose actions, but a separate authority must
decide whether those actions can mutate state.

The pattern has two roles:

- Oracle: proposes typed actions, plans, scores, or evidence requests.
- Executor: validates the proposal, applies policy, records evidence, and performs or blocks the
  state change.

## Core Rule

The oracle is not the executor.

Do not let the same model response both propose an action and declare that it safely changed the
world.

## Operating Rules

- Keep proposal and mutation separate.
- Make oracle outputs typed and checkable.
- Validate every proposal against current state and policy before mutation.
- Ask for more evidence when behavior is hidden behind scripts, tools, links, or external resources.
- Record the proposal, evidence, decision, accepted action, fallback action, and resulting state.
- Treat human approval thresholds as configurable policy, not fixed truth.
- Prefer deterministic checks for known hazards before using a model judge.
- Escalate uncertainty instead of silently allowing it.

## Decision Outcomes

An executor can return:

- allow: proposal may proceed.
- review: human approval or more evidence is required.
- deny: proposal must not proceed.
- fallback: a safer deterministic action is applied instead.

## Metrics

Track:

- accepted oracle proposals
- rejected oracle proposals
- fallback actions
- human reviews
- false allows
- false alarms
- missing-evidence escalations
- policy overrides

The pattern makes model weakness visible before it changes state.

## Role Files

- [Oracle](./oracle/AGENTS.md)
- [Executor](./executor/AGENTS.md)
