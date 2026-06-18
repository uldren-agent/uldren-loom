---
title: "Working Title"
slug: "working-title"
date: "YYYY-MM-DD"
author: "Uldren"
summary: "One plain sentence that says what the reader will understand after reading."
status: "draft"
---

# Working Title

Start with the thing the reader has already felt.

Do not begin with a market map, a product pitch, or a definition. Start with the practical tension. The first paragraph should make the reader think, "yes, that is the problem."

Example:

> You ask one model to write the plan. Then you ask another model to review it. Then you paste the revision back into the first model. The interesting part is not the copy and paste. The interesting part is the shape hiding underneath it.

## The Short Version

Name the pattern in one paragraph.

Use this section to give the reader a mental handle before adding nuance. Keep it concrete. If the post is about a pattern, include the pattern as a short sequence.

Example:

```text
Proposer -> Critic -> Reconciler -> Executor -> Verifier
```

## The Problem

Explain why the old framing is too small.

Good posts in this series should avoid "AI will change everything" language. The useful question is narrower: what workflow are people already doing manually, and what shape does it have?

Cover:

- What people do today
- Where the workflow gets awkward
- What breaks when the task gets larger
- What the pattern clarifies

## The Pattern

Define the pattern as if the reader might implement it in any tool.

Include:

- Roles
- Inputs and outputs
- Where human approval belongs
- What gets logged
- What makes the pattern fail

## Where It Fits

Place the pattern relative to adjacent patterns.

For agent posts, use this stack:

```text
Loop Engineering
  -> Model Relay
    -> ReAct Agent
```

Loop engineering decides when the workflow runs. Model Relay decides which role gets the work next. ReAct is one way a single model performs a role by reasoning, acting, observing, and repeating.

## What Not To Do

Every pattern post needs a section that reduces hype.

Examples:

- Do not use five models when one model and a checklist is enough.
- Do not call something verified because another model said it looked good.
- Do not hide the handoff state inside a chat transcript.
- Do not automate a workflow before you can run it manually.

## Sources

Prefer primary or near-primary sources.

- Paper, official project page, or official docs
- A strong practitioner post when the term is still emerging
- A note on what the source does and does not prove

## Closing

End with a useful distinction, not a call to action.

The reader should leave with a cleaner vocabulary and a slightly sharper way to evaluate their own agent workflow.
