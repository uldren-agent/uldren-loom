# Matrix2 Active Task Matrix

This matrix is a dynamic pull registry for work derived from `specs/_FACET_PRIMITIVES.md`.
It is designed for 3-5 generic agent sessions that can claim ready work without forcing serial
completion of one lane.

Every packet depends on the whole `specs/_FACET_PRIMITIVES.md` document. Appendix references and
facet-section references are reading-order hints, not dependency boundaries.

## Pull Policy

| Rule | Instruction |
| --- | --- |
| Claim only ready packets | A packet is ready when dependencies are listed as `none` or already complete, and the allowed file scope is clear. |
| Prefer ROI over serial order | Do not finish a lane serially when another lane has a higher-value ready packet. |
| Respect source scope | Work only in the packet's allowed files unless the packet explicitly asks for an owner decision to expand scope. |
| Read the primitive file as a whole | Each packet must treat the full `_FACET_PRIMITIVES.md` file as the design dependency, then use targeted sections for depth. |
| Write results into the packet | The executing session updates the `Results` section in the prompt file. |
| Preserve decision visibility | If owner input is needed, route the question through the active ticket and decision resource before chat notification. |

## Lane Definitions

The Lane column below classifies packet work type for pull selection. It is not the managed Lane
object. Managed Lane objects are defined in `README.md` and coordinate assignment, active tickets,
status reports, reviewer feedback, and ordered ticket membership.

| Work type | Scope | Typical output |
| --- | --- | --- |
| Shared substrate | Cross-facet primitives that should be built once and reused. | Owning-spec requirements, shared crate candidates, implementation packets, conformance needs. |
| Conformance | Proof strategy, vectors, transcripts, negative tests, client matrices, and capability tests. | Test plans, matrix rows, source-backed proof requirements. |
| Packaging | Compile features, optional runtimes, platform profiles, binary composition, and capability reporting states. | Packaging rules, feature profiles, capability states, build-profile tasks. |
| Owning-spec propagation | Moves primitive decisions from `_FACET_PRIMITIVES.md` into numbered specs without changing implementation. | Spec updates, unresolved decisions, downstream task packets. |
| Design review | Clarifies ambiguous primitive or facade direction before implementation starts. | Decision records, revised packet scope, new matrix rows. |
| Implementation handoff | Converts source-backed design into scoped build work for a coding session. | Executable implementation prompt with checks and stop conditions. |

## Active Matrix

| Packet ID | Lane | Prompt file | ROI | Lift | Risk | Source dependency | Packet dependencies | Ready | Best worker fit | Output expected |
| --- | --- | --- | ---: | ---: | ---: | --- | --- | --- | --- | --- |
| FP-SUB-001 | Shared substrate | `prompts/FP-SUB-001-capability-reporting.md` | 10 | 4 | 6 | Full `_FACET_PRIMITIVES.md` | none | Yes | Spec architecture | Capability-reporting owning-spec packet and source anchors. |
| FP-SUB-002 | Shared substrate | `prompts/FP-SUB-002-conditional-mutation-entity-tags.md` | 8 | 5 | 6 | Full `_FACET_PRIMITIVES.md` | none | Yes | Spec architecture | Shared lost-update primitive requirements. |
| FP-SUB-003 | Shared substrate | `prompts/FP-SUB-003-derived-artifact-lifecycle.md` | 9 | 6 | 7 | Full `_FACET_PRIMITIVES.md` | FP-SUB-001 helpful but not required | Yes | Spec architecture | Derived-artifact lifecycle ownership and task split. |
| FP-CONF-001 | Conformance | `prompts/FP-CONF-001-conformance-strategy.md` | 9 | 4 | 5 | Full `_FACET_PRIMITIVES.md` | none | Yes | Test/spec design | Cross-facet proof matrix converted into actionable conformance work. |
| FP-PKG-001 | Packaging | `prompts/FP-PKG-001-packaging-profiles.md` | 8 | 4 | 5 | Full `_FACET_PRIMITIVES.md` | FP-SUB-001 helpful but not required | Yes | Build/release design | Packaging and feature-gating tasks with capability-state language. |
| FP-SPEC-001 | Owning-spec propagation | `prompts/FP-SPEC-001-owning-spec-propagation.md` | 9 | 5 | 5 | Full `_FACET_PRIMITIVES.md` | none | Yes | Spec propagation | Batch plan for moving primitive decisions into owning specs. |

## Suggested Pull Order For 3-5 Agents

| Worker slot | First packet to claim | Reason |
| --- | --- | --- |
| Agent 1 | FP-SUB-001 | Capability reporting improves every other packet's output. |
| Agent 2 | FP-CONF-001 | Conformance proof can progress independently and will improve implementation quality. |
| Agent 3 | FP-SPEC-001 | Owning-spec propagation prevents `_FACET_PRIMITIVES.md` from becoming the only source of unfinished work. |
| Agent 4 | FP-SUB-002 | Conditional mutation has high cross-facet value and a bounded conceptual surface. |
| Agent 5 | FP-PKG-001 | Packaging rules can progress while substrate design continues. |

FP-SUB-003 should be pulled as soon as one architecture-capable worker is free. It is slightly
larger, but it unlocks FTS, vector, graph, dataframe, columnar, PIM indexes, metrics rollups, and
IPFS cache correctness.

## Review Rule

After a worker updates a prompt packet, review the results before treating the matrix row as
complete. The review should check:

| Review item | Pass condition |
| --- | --- |
| Scope control | The worker stayed inside the allowed files or recorded why scope expansion is needed. |
| Source grounding | Claims cite source anchors or are explicitly marked as design assumptions. |
| Decision visibility | Owner questions are recorded in the required decision format. |
| Task conversion | Follow-on work is actionable, not vague. |
| Verification | Checks are named with outcomes, or not-run reasons are explicit. |
