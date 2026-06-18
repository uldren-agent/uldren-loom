# FP-PKG-001 Packaging Profiles

## Packet Metadata

| Field | Value |
| --- | --- |
| Packet ID | FP-PKG-001 |
| Lane | Packaging |
| Status | Ready |
| ROI | 8 |
| Lift | 4 |
| Risk | 5 |
| Source dependency | Full `specs/_FACET_PRIMITIVES.md` |
| Packet dependencies | FP-SUB-001 helpful but not required |
| Allowed files | `specs/_FACET_PRIMITIVES.md`, relevant packaging or build specs only if needed for source anchors, and this packet file |
| Blocked files | Code, queues outside `specs/matrix2/`, implementation plans, lockfiles |

## Source Anchors

Read these before acting:

| Source | Required section |
| --- | --- |
| `specs/_FACET_PRIMITIVES.md` | Whole file for dependency scope; Appendix H, Appendix A, and Appendix G for the first deep pass |
| `specs/matrix2/MATRIX.md` | Active Matrix and Review Rule |

The appendices listed above are not the full dependency boundary. Check every optional engine,
facade, hosted surface, mobile/WASM concern, and runtime dependency mentioned elsewhere in the file
before writing results.

## Task

Convert the packaging and deployment profile matrix into actionable packaging work. Do not change
build files. Identify profile requirements for normal distributions, server builds, data-heavy
builds, privacy-network builds, IPFS-capable builds, mount-enabled builds, mobile bindings,
WASM/browser builds, and CI/conformance builds.

The design must preserve:

| Rule | Reason |
| --- | --- |
| Config-only support in normal binaries | Stores should preserve optional IPFS, Tor, mount, and engine configuration even when runtimes are not compiled in. |
| No startup dynamic-link failures | Optional runtimes must not crash the base binary at startup. |
| Explicit activation | Optional runtimes require compiled support and operator configuration. |
| Source identity independence | Optional engines cannot define canonical Loom data. |
| Capability clarity | Compile-disabled, runtime-missing, configured-disabled, denied, unsupported, degraded, and unavailable states must stay distinct. |

## Stop Conditions

Stop and record a decision point if:

| Condition | Why |
| --- | --- |
| A packaging profile changes the single-binary strategy. | This is a public distribution decision. |
| A runtime should become default-on. | Default runtime activation affects security, footprint, and user expectations. |
| The packet would require implementation edits. | This packet is design and handoff only. |

## Required Output

Update the `Results` section with:

1. Files changed.
2. Source anchors checked.
3. Proposed owning specs.
4. Packaging profiles and capability states.
5. Follow-on implementation packets.
6. Checks run.
7. Blockers or decision points.

## Results

Status: Ready.

No worker results yet.
