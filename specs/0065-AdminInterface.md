# 0065 - Admin Interface

**Status:** Draft, deferred target. **Version:** 0.1.0.
**Capability:** `admin`.

**Depends on:** 0008 (wire protocols), 0009/0009a (security, audit, retention, governance), 0010a
(conformance reporting), 0026 (principals and identity), 0027/0028 (access control), 0029 (triggers),
0035 (durable delivery), 0039 (mail mutable state), 0041 (facet lifecycle hooks), 0060 (FIPS
distribution), and 0061 (operation substrate).

This spec defines the long-term administrative interface for Loom. It is the home for operator and
administrator controls that would otherwise be buried inside implementation queues, facet specs, hosted
protocol adapters, or conformance notes.

Implementation status: target design with source-backed subsets. Current source backs local CLI audit
configuration/read/prune, hosted Admin REST/JSON-RPC listener management, identity management, ACL
management, protected-ref management, static web route management, network-access policy management,
and audit read/export/config/prune. The full coherent admin control plane, shared admin facade,
canonical policy schemas, MCP/App projection, certification UI, and broad conformance suite remain
target work.

## 1. Purpose

The admin interface gives authorized administrators a durable way to inspect, configure, and prove
system policy. It is not ordinary data access, not a user productivity UI, and not a bypass around facet
authorization.

The interface owns administrative projection for:

- policy profiles and effective policy inspection;
- retention and compaction controls;
- audit configuration and redacted audit reads;
- hook and trigger administration;
- hosted listener and certificate posture;
- capability and conformance evidence;
- reference-client certification records;
- operator-safe diagnostics.

## 2. Non-Goals

- It does not define a new storage facet for user data.
- It does not replace 0026/0027 authorization.
- It does not let administrators read encrypted or access-controlled payloads without grants.
- It does not own the runtime semantics of mail, calendar, contacts, triggers, delivery, FIPS, or search.
- It does not turn every internal knob into a public API.

## 3. Source Checks

This spec exists because several target contracts need a shared administrative home:

- 0039 source-backs mail flag retention policy records, mutable-state version tokens, and detailed flag
  deltas, operation-style merge, observed-version replacement conflicts, redacted audit summaries,
  compaction, and retained-gap errors. This admin interface owns operator-visible retention editing,
  audit inspection, and compaction controls.
- 0035 requires retention-gap detection, backpressure policy, hosted transport projection, and replay
  authorization checks.
- 0041 source-backs hook registration, event emission, execution-policy planning, run-as validation,
  depth bounds, and loop refusal. This admin interface owns operator-visible policy editing, status
  inspection, fire-history inspection, and safe enable/disable controls.
- 0010a requires machine-readable conformance and certification reports.
- 0008 requires hosted listener capability rows and protocol conformance evidence.
- 0060 requires FIPS policy and hosted-listener rejection visibility.

## 4. Shape

The admin interface should be one control plane with multiple projections:

| Projection | Purpose |
| --- | --- |
| Rust/admin facade | Source-backed local API and conformance anchor. |
| CLI | Operator workflow for local stores and daemon-managed stores. |
| Hosted REST/JSON-RPC | Remote administrative automation through 0008 auth and PEP. |
| MCP/App surface | Human review of policy, evidence, and pending decisions. |
| Machine-readable reports | CI, release, compliance, and certification automation. |

All projections share one schema and one authorization model. Projection-specific formatting is allowed;
projection-specific policy semantics are not.

## 5. Authorization

Administrative operations require explicit admin rights. The PEP must distinguish at least:

- view effective policy;
- edit policy;
- view redacted audit;
- export compliance evidence;
- manage listeners and certificates;
- manage hooks and triggers;
- manage retention and compaction;
- view reference-client certification records.

Admin access is not payload access. A principal may be allowed to see that a mail flag changed without
being allowed to read the message body or full headers.

## 6. Policy Records

Administrative settings should be durable records with stable schema versions. The first target records
are:

| Record | Scope | Purpose |
| --- | --- | --- |
| `admin.policy.profile` | store or workspace | Named policy bundle selected by deployment. |
| `admin.retention.mail_flags` | principal, mailbox, workspace, or store | Detailed delta window, size caps, audit summary class, and retained-gap behavior. |
| `admin.audit.profile` | store or workspace | Redaction policy, audit class selection, export permissions, and legal hold hooks. |
| `admin.hooks.policy` | workspace or facet | Hook registration rights, max depth, priority classes, run-as constraints, and loop handling. |
| `admin.hosted.listener_policy` | listener or service class | Direct TLS requirement, app credential requirement, FIPS mode, and capability advertisement policy. |
| `admin.certification.profile` | release, store, or deployment | Required clients, transcript retention, conformance suites, and evidence export format. |

Policy records are ordinary versioned Loom content unless a specific record must be authority-local
operational state. Authority-local exceptions must be explicit.

## 7. Mail Mutable-State Policy

The admin interface owns configuration and inspection for mail mutable-state retention.

The enterprise target is:

- current flag state is retained indefinitely;
- detailed per-message flag deltas are source-backed and retained for a configurable sync window and
  size cap;
- expired detailed deltas compact into redacted audit summaries;
- stale incremental sync requests receive a stable retained-gap response requiring full resync;
- audit summaries record actor, source protocol, message identity, old/new flag digests or keyword sets,
  commit/state token, and timestamp;
- raw RFC 5322 bodies are not included in audit payloads.

## 8. Hook And Trigger Administration

The admin interface owns operator-visible hook and trigger management:

- list registrations, scopes, programs, run-as principals, priority, and status;
- enable, disable, unregister, and reassign run-as where authorized;
- inspect fire history and loop-protection refusals;
- inspect denied, skipped, queued, budget-exceeded, and error outcomes;
- configure max depth and priority classes;
- export redacted hook evidence for conformance.

The admin interface does not change the execution semantics defined by 0015, 0029, or 0041.

## 9. Hosted And Certification Administration

The admin interface owns operator-visible hosted protocol posture:

- listener inventory and direct TLS status;
- certificate bundle status and FIPS rejection reasons;
- app credential requirements;
- advertised capability rows;
- conformance report links;
- reference-client transcript inventory;
- unsupported, degraded, target, and source-backed rows.

It should make false capability advertisement hard by comparing configured policy, source-backed
evidence, and advertised runtime rows.

The `admin.certification.profile` record owns the long-term reference-client gate. For PIM it records:

- required clients, versions, operating systems, and protocol families;
- TLS mode, auth mode, listener identity, and certificate posture;
- transcript retention and redaction policy;
- conformance suites and executable transcript names;
- RFC implementation-gate versions and pass, fail, degraded, unsupported, target, or deferred rows;
- pass, fail, degraded, unsupported, skipped, and target rows;
- links from failures to fixes, dialect decisions, or unsupported capability rows.

The PIM enterprise release gate is Apple plus cross-platform client coverage for CalDAV, CardDAV, and
IMAP: Apple Calendar or iOS Calendar, Apple Contacts or iOS Contacts, Apple Mail, Thunderbird, and DAVx5.
JMAP certification starts with executable RFC 8620/8621/9404 transcripts and later pins one JMAP-native
client or conformance tool after blob, identity, changes, and push are source-backed. Direct TLS is
already source-backed for the current hosted JMAP routes and remains part of the hosted posture
evidence.
The current source-backed report fixture is `pim-owner-only-enterprise-v1` in 0010a. It serializes the
`admin.certification.profile` key, required client targets, transcript inventory, and
`pim-owner-only-redacted-transcripts-v1` policy. The deferred admin UI must read those records instead
of maintaining a separate certification target list.

## 10. Active Coherent Admin Owner Gate

Completion state: active implementation owner. Current source backs selected local and hosted admin
slices, including capability matrix readout, audit configuration and read/export/prune, hosted
listener management, identity, ACL, protected-ref, static web route, and network-access policy
management. The full coherent admin control plane, shared facade, canonical policy schemas, MCP/App
projection, certification UI, hook/trigger administration, evidence reports, hosted posture reporting,
and authorization proof remain P0 implementation work.

Decision Points: none.

| Gate | Source-backed evidence | Remaining implementation work | Disposition |
| --- | --- | --- | --- |
| Shared admin facade | Existing local and hosted routes cover selected administrative slices. | Define one Rust admin facade and projection contract so CLI, hosted, MCP/App, and reports share policy semantics instead of duplicating admin logic per adapter. | Target P0. |
| Canonical policy schemas | This spec defines target records for policy profiles, retention, audit, hooks, hosted listener policy, and certification profiles. | Pin schema versions, canonical encodings, migrations, ownership rules, negative vectors, and report integration for each promoted record. | Target P0. |
| Authorization proof | Admin access is separate from payload access and requires distinct rights for policy, audit, evidence export, listeners, hooks, retention, and certification records. | Add denied-operation tests proving admin rights do not grant payload reads and that each projection enforces the correct right before policy edits, exports, listener changes, hook changes, and retention changes. | Target P0. |
| MCP/App projection | MCP and Apps provide source-backed app resources, Ask flows, and local tooling. | Define admin resources, tools, ACL-filtered visibility, pending-decision records, report-resource semantics, and host bridge expectations as a coherent admin surface. | Target P0. |
| Certification UI and reports | 0010a source-backs the `pim-owner-only-enterprise-v1` fixture and this spec owns the certification profile shape. | Build reviewed certification records, advertised capability rows, transcript evidence, unsupported/degraded/target/source-backed status rows, reference-client decisions, and report export into the admin plane. | Target P0. |
| Hosted posture reporting | Hosted admin routes cover capability matrix readout, listener inventory, and selected management actions. | Add direct TLS and certificate posture reporting, FIPS rejection evidence, app credential policy summaries, advertised capability reconciliation, and conformance links across hosted projections. | Target P0. |
| Hook and trigger administration | 0029 and 0041 source-back trigger and hook mechanics, execution handoff, event history, run-as presence, priority, depth, and loop refusal. | Add admin list, enable, disable, unregister, run-as reassignment, fire history, skipped/denied/queued/budget/error reporting, loop-protection evidence, priority/depth controls, redacted exports, and conformance. | Target P0. |
| Retention, audit, and compaction administration | Local and hosted audit list/export/config/prune slices are source-backed, and this spec defines target retention and audit policy. | Promote canonical retention and audit records, retained-gap responses, redacted summaries, compaction evidence, legal-hold behavior, and migrations across CLI, hosted, MCP/App, and reports. | Target P0. |
| Broad conformance | 0010a defines status vocabulary and certification tracks. | Add executable suites and report rows for admin policy CRUD, denied rights, audit redaction/export, retention gaps, hook/trigger admin, hosted posture, MCP/App resources, certification review, migrations, and false-advertisement guards. | Target P0. |

## 11. Deferred Promotion Path

0065 becomes implementation-gating only through a later queue that provides:

- a Rust admin facade;
- canonical policy schemas and vectors;
- authorization surface review;
- CLI and hosted projection design;
- report schema integration with 0010a;
- conformance scenarios for policy reads, policy writes, audit reads, retention-gap handling, and hook
  administration;
- migration rules for any policy records added before the admin facade is source-backed.

Until then, owning specs may implement local policy mechanics, but they should point administrator-facing
controls and evidence export back to this spec rather than adding bespoke admin surfaces.
