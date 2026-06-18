# GW-0000 — Gateways: Index & Concept

**Series:** Gateways (exploratory project sub-series; Draft)
**Version:** 0.1.0-draft · **Status:** Draft — exploratory
**Last updated:** 2026-06-18
**Not linked to the main `0000-index.md` (intentional, per owner).**

A **gateway** is Loom sitting **on a standard port, in the network path** of an existing protocol — either
*terminating* it (Loom is the server) or *tapping* it (Loom records, optionally filters, and forwards
upstream). This is distinct from [`../facet-bindings/`](../facet-bindings/), which *exposes Loom's facets
as* protocols; a gateway *consumes a foreign protocol into* Loom workspaces.

## Two modes

- **Terminate / back (B).** Loom is the endpoint; data lands in a workspace. Overlaps the P9 Tier-2
  adapters (pg-wire, S3, …) — a gateway in B-mode *is* a facet binding's foreign adapter, deployed as a
  standalone listener.
- **Tap / relay (T).** Loom intercepts in the path, **records into a workspace**, optionally **filters or
  transforms** (LLM via `ai/`, or heuristics), and **forwards upstream**. This is the novel capability.
  **Flagship:** SMTP-465 — intercept mail → record → LLM/heuristic filter → forward to the final mail
  server (versioned, queryable, auditable email).

## Source-backed status (reconciliation, 2026-07-18)

B-mode gateways for the highest-value data/store protocols are **already source-backed** as standalone
`loom-hosted` / `loom-hosted-pim` wire adapters (bounded profiles), registered in
[`../0008-wire-protocols.md`](../0008-wire-protocols.md) and conformance-gated in `loom-conformance`:
**OpenSearch, S3, PostgreSQL, MySQL, Redis, Memcached, IMAP, and JMAP**. The authoritative per-facade
support state (Supported / Target / Unsupported, source anchors, conformance) is the compatibility
matrix on ticket **MX-124** (FP-COMPAT-MATRICES-001) and the owning facet specs — **not** this
exploratory GW series. A B-mode gateway *is* a facet binding's foreign adapter deployed as a listener,
so those eight are realized already.

This sub-series remains the landscape/planning view for the *remaining* gateway work, which is
primarily **T-mode taps** (record → filter → forward) — e.g. the SMTP-465 flagship (only setup exists
today in `crates/loom-hosted-pim/src/smtp.rs`; no SMTP in the base mail facet, 0039 RD3/RD6) — plus the
B-mode adapters not yet built (FTP, WebDAV, git mirror, DNS, LDAP, syslog/OTLP/MQTT, and so on).

## How gateways compose with the rest of the spec

- **`facet-bindings/`** — B-mode gateways reuse the Tier-2 adapter mappings (S3↔`files`/`cas`,
  pg-wire↔`sql`, …).
- **`control-plane-bindings/`** — telemetry taps (syslog, OTLP, SNMP, MQTT) land in `ledger`/`queue`/
  `time-series`; `watch` (CP-0003) lets agents react to what a gateway records; `exec` (CP-0002) and
  `trigger` (CP-0006) run the filter/transform logic.
- **`ai/`** — the LLM/embedding providers power the "filter with an LLM" step (e.g. the 465 case).

## Files

| Doc | Scope |
| --- | --- |
| `GW-0000-index.md` | this concept/index |
| [`GW-0001-port-landscape.md`](./GW-0001-port-landscape.md) | curated ports with a data-/file-store aspect: protocol, store aspect, mode (B/T), target facet |
| [`GW-0002-implementation-lift.md`](./GW-0002-implementation-lift.md) | per-protocol implementation **lift** (S/M/L) and **Rust libraries** (or custom) |

## Folder-name rationale

`gateways` was chosen over `taps` / `interceptors` / `proxies` (each captures only the T-mode) and over
`ports` / `apps` (mechanism-named / too vague). "Gateway" covers both B and T modes and reads as a
purpose, not a mechanism.

## Suggested first verticals

1. **Telemetry** (syslog 514, StatsD/Graphite, OTLP, MQTT, SNMP) — append-only, maps almost 1:1 to
   `ledger`/`queue`/`time-series`, mostly **S/M** lift. The cleanest start.
2. **Mail** (SMTP 465) — the marquee filter-and-forward demo (**M** lift), exercises `ai/` filtering.
3. **DNS** (53) — near-**S** with `hickory-server`; a versioned, filterable DNS sinkhole/audit.

(Status/next-steps to be filled in as the owner directs; this sub-series is exploratory and not yet
sequenced into the build phases.)
