# Matrix2 Prompt Packets

Each file in this folder is an executable task packet. To use one, tell an agent session:

```text
Read and execute specs/matrix2/prompts/<packet-file>.md.
Put your results back into that file.
```

The packet should be small enough for a worker to execute without needing a long pasted prompt.
The worker updates the `Results` section in the same file. A later review session can inspect the
diff and decide whether the packet is complete, needs revision, or should produce follow-on packets.

Every packet depends on the full `specs/_FACET_PRIMITIVES.md` file. Targeted source anchors are
reading-order hints, not a narrowed scope. Workers must check that their output does not contradict
the facet inventory, planned facets, appendices, or shared-substrate analysis elsewhere in the file.

## Packet Contract

| Section | Required content |
| --- | --- |
| Packet metadata | ID, lane, status, ROI, lift, risk, dependencies, allowed files, blocked files. |
| Source anchors | Files and sections the worker must read before acting. |
| Task | Concrete action and expected level of depth. |
| Stop conditions | Conditions where the worker must stop and route an owner question through the active ticket instead of guessing. |
| Required output | The exact results the worker must write back into the packet. |
| Results | Worker-written output, including files changed, anchors checked, decisions, remaining work, checks, and blockers. |

## Result Status Values

| Status | Meaning |
| --- | --- |
| Ready | The packet can be claimed now. |
| Claimed | A worker is actively executing it. |
| Review | The worker wrote results and it needs review. |
| Complete | The packet is accepted and no remaining work is hidden. |
| Blocked | The packet needs an owner decision or external state change before meaningful work can continue. |

## Owner Questions

Prompt packets must not treat the packet file or chat as the only home for owner questions. If a
worker needs owner input, the worker creates or updates
`decisions/<active-ticket-id>/<short-decision-id>`, updates the active ticket to
`status: awaiting_decision` with `decision_id` and `decision_resource`, stops at the affected
boundary, and reports only the ticket key and decision resource in chat.

The decision document must use readable Markdown sections:

1. Question.
2. Context.
3. Examples.
4. Options.
5. Recommendation.
6. Consequence of Deferring.

If the active ticket or ticket update surface is unavailable, the worker records that missing surface
as the blocker. It must not ask an owner question only in chat.

## MCP Document Operations

Prompt packets, boards, decisions, and result closeouts are UTF-8 documents unless the packet says
otherwise. Workers should use `document_get_text` and `document_put_text` for those records when the
tools are available. If only binary document tools are exposed, workers may use
`document_get_binary` and `document_put_binary`, but they should generate byte arrays from local
UTF-8 text and record the fallback in the result.

Writing a result document is not the same as updating a ticket. If a ticket status change is required
and no ticket operation surface is available, the missing ticket operation is the blocker.
