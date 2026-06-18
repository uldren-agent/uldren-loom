# FP-CONF-001 Conformance Strategy

## Packet Metadata

| Field | Value |
| --- | --- |
| Packet ID | FP-CONF-001 |
| Lane | Conformance |
| Status | Ready |
| ROI | 9 |
| Lift | 4 |
| Risk | 5 |
| Source dependency | Full `specs/_FACET_PRIMITIVES.md` |
| Packet dependencies | none |
| Allowed files | `specs/_FACET_PRIMITIVES.md`, relevant conformance specs only if needed for source anchors, and this packet file |
| Blocked files | Code, queues outside `specs/matrix2/`, implementation plans, lockfiles |

## Source Anchors

Read these before acting:

| Source | Required section |
| --- | --- |
| `specs/_FACET_PRIMITIVES.md` | Whole file for dependency scope; Appendix F and Appendix E for the first deep pass |
| `specs/matrix2/MATRIX.md` | Active Matrix and Review Rule |

The appendices listed above are not the full dependency boundary. Check every facet and facade
section for proof needs before converting the conformance strategy into packets.

## Task

Convert the conformance strategy matrix into actionable conformance work. Do not implement tests.
Identify which proof types should become reusable conformance categories, which facets or facades
need them first, and which packets should be created for later implementation sessions.

The output should distinguish:

| Proof family | Example |
| --- | --- |
| Canonical format proof | Canonical byte vectors and negative decode vectors. |
| Operation proof | Native operation tests and product protocol transcripts. |
| Capability proof | Supported, unsupported, degraded, denied, compile-missing, runtime-missing, and unavailable states. |
| Policy proof | Auth, PEP, API keys, app passwords, audit, and redaction. |
| Resource proof | Bounds on rows, bytes, time, regex cost, memory, fanout, and cardinality. |
| Client proof | Real client or differential behavior for mature ecosystems. |
| Recovery proof | Migration, rebuild, stale state, and retention behavior. |

## Stop Conditions

Stop and record a decision point if:

| Condition | Why |
| --- | --- |
| A conformance category would require a public compatibility claim. | Compatibility language needs owner review. |
| A test would fake the real behavior it is meant to prove. | The repo requires proof against real contracts. |
| The packet would require implementation edits. | This packet is design and handoff only. |

## Required Output

Update the `Results` section with:

1. Files changed.
2. Source anchors checked.
3. Proposed conformance categories.
4. First facets or facades to target.
5. Follow-on test implementation packets.
6. Checks run.
7. Blockers or decision points.

## Results

Status: Ready.

No worker results yet.
