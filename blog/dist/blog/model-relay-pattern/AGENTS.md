# Model Relay Pattern

Use Model Relay when a task should move through accountable roles before the result is trusted.

The relay has five roles:

- Proposer: turns intent into a plan.
- Critic: challenges the plan before execution.
- Reconciler: turns proposal plus critique into the accepted execution plan.
- Executor: performs the work from the reconciled plan.
- Verifier: checks the result against the accepted plan and evidence.

## Operating Rules

- Keep each role separate. A role may be a different subagent, model, prompt profile, or human gate.
- Pass a durable handoff artifact between roles.
- Do not let the Executor work from the original user prompt when a reconciled plan exists.
- Do not let the Verifier accept work without evidence.
- If Critic rejects the plan, return to Proposer.
- If Verifier finds an execution issue, return to Executor.
- If Verifier finds a plan issue, return to Reconciler or Proposer.

## Role Files

- [Proposer](./proposer/AGENTS.md)
- [Critic](./critic/AGENTS.md)
- [Reconciler](./reconciler/AGENTS.md)
- [Executor](./executor/AGENTS.md)
- [Verifier](./verifier/AGENTS.md)

## Handoff Packet

Every handoff should include:

- Goal
- Source request
- Assumptions
- Accepted constraints
- Open questions
- Current plan or result
- Evidence gathered so far
- Risks and unresolved concerns
- Human approvals or overrides

## Model Mapping

An agent harness can map roles to different models or settings.

- Use a strong planning model for Proposer when scope is unclear.
- Use a skeptical model or stricter prompt for Critic.
- Use a high-context model for Reconciler when critique is large.
- Use a tool-capable coding or execution agent for Executor.
- Use a separate model, deterministic checks, or a human for Verifier.

Claude, Codex, Hermes, OpenClaw, and similar systems can implement the same relay with different model choices, tool access, and approval gates.
