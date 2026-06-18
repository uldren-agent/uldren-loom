# FP-SUB-001 Capability Reporting

## Packet Metadata

| Field | Value |
| --- | --- |
| Packet ID | FP-SUB-001 |
| Lane | Shared substrate |
| Status | Ready |
| ROI | 10 |
| Lift | 4 |
| Risk | 6 |
| Source dependency | Full `specs/_FACET_PRIMITIVES.md` |
| Packet dependencies | none |
| Allowed files | `specs/_FACET_PRIMITIVES.md`, relevant numbered specs only if needed for source anchors, and this packet file |
| Blocked files | Code, queues outside `specs/matrix2/`, implementation plans, lockfiles |

## Source Anchors

Read these before acting:

| Source | Required section |
| --- | --- |
| `specs/_FACET_PRIMITIVES.md` | Whole file for dependency scope; Appendix E, Appendix G, and Appendix H for the first deep pass |
| `specs/matrix2/MATRIX.md` | Active Matrix and Review Rule |

The appendices listed above are not the full dependency boundary. Check the facet inventory,
planned facets, and per-facet sections for consumers and naming conflicts before writing results.

## Task

Design the shared capability-reporting primitive as an owning-spec handoff packet. Do not implement
code. Identify the stable capability states, consumers, required source anchors, and follow-on spec
updates needed so later implementation sessions can build one capability model instead of per-facet
status flags.

The design must preserve distinctions between:

| State | Meaning |
| --- | --- |
| Supported | The feature is implemented, enabled, and allowed. |
| Unsupported | The product or native profile does not support the requested behavior. |
| Degraded | The feature runs in a reduced mode and must report what is missing. |
| Denied | Auth or policy rejected the operation. |
| Disabled | Configuration turns the feature off. |
| Compile-missing | The binary was not built with the required feature. |
| Runtime-missing | The binary supports the feature but a runtime dependency is absent. |
| Unavailable | The feature is temporarily unavailable because of state, maintenance, or runtime failure. |

## Stop Conditions

Stop and record a decision point if:

| Condition | Why |
| --- | --- |
| A public naming choice is needed for capability states. | State names become contract language across CLI, bindings, MCP, and hosted routes. |
| The packet would require code edits. | This packet is design and handoff only. |
| The worker cannot identify owning specs. | Guessing would scatter the primitive. |

## Required Output

Update the `Results` section with:

1. Files changed.
2. Source anchors checked.
3. Proposed owning specs.
4. Capability states and definitions.
5. Consumers and required projections.
6. Follow-on implementation packets.
7. Checks run.
8. Blockers or decision points.

## Results

Status: Ready.

No worker results yet.
