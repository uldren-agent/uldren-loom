# Loom Platform

**Status:** Draft | **Version:** 0.1.0-draft | **Normative?** Informative product architecture

This document maps the Loom product surfaces to the technical trust and deployment models already
defined by the core specs. It is not a protocol contract. The numbered specs remain authoritative for
data model, sync, encryption, hosted protocols, authorization, and conformance behavior.

## 1. Product surfaces

| Surface | Distribution | Primary role | Data visibility | Compute |
| --- | --- | --- | --- | --- |
| `loom-cli` | Local binary and embeddable library | On-device Loom capability set | Local caller controls access to the opened Loom | Local and embedded compute only |
| `loom-server` | Closed-source paid product | Managed Loom host for teams and enterprises | Host can access managed data when configured as a keyed remote | Server-side analysis across hosted Loom data |
| `loom-cloud` | Closed-source Uldren-operated service | Zero-knowledge durable storage and sync relay | Uldren does not hold the data key | No server-side data processing |

`loom-cli` is the local capability surface. It exposes Loom to end users, applications, agents,
language bindings, and direct embedding in client software. It can operate without a hosted service,
and it can sync to a compatible remote.

`loom-server` is the trusted hosted surface. It can be installed on-premise by a customer or operated
as a cloud-hosted managed deployment. In both cases, the deployment is designed for a host that is
allowed to manage and analyze the data it stores. That makes it the right surface for enterprise
administration, query, indexing, policy enforcement, and cross-Loom analysis.

`loom-cloud` is the Uldren-operated backup and relay surface. It stores encrypted Loom data and sync
state, but it does not receive the unwrapping key and does not run SQL, vector search, programs,
indexing, or analytics over customer content. It can authorize and route sync by labels, but it is
not a compute surface.

## 2. Trust boundaries

The platform split follows the key-holding rule from end-to-end encrypted sync:

1. A remote that does not hold the key is a blind storage and sync host.
2. A remote that holds the key is a fully accessible replica and may compute on the data.

`loom-cloud` is always the first case. It is a zero-knowledge remote for backups, multi-device sync,
and enterprise disaster recovery. Its product promise depends on not receiving the key and not adding
data-processing features that require plaintext access.

`loom-server` is the second case when the customer configures it as a managed enterprise host. It may
hold the required key material through the approved unlock-provider path for that deployment, so it
can run queries, indexes, triggers, analysis jobs, and administrative workflows against hosted data.

`loom-cli` can participate in either relationship. It can sync directly to `loom-cloud` as an
end-to-end encrypted client, or it can sync to `loom-server` as a trusted enterprise host.

## 3. Deployment relationships

```text
+--------------------+        encrypted sync        +--------------------+
| End-user device   | ---------------------------> | loom-cloud         |
| loom-cli          |                              | storage + backup   |
+--------------------+                              +--------------------+

+--------------------+        trusted sync          +--------------------+
| Enterprise app     | ---------------------------> | loom-server        |
| embedded loom-cli  |                              | managed + compute  |
+--------------------+                              +--------------------+

+--------------------+        encrypted backup      +--------------------+
| Enterprise host    | ---------------------------> | loom-cloud         |
| loom-server        |                              | storage + backup   |
+--------------------+                              +--------------------+
```

## 4. Customer paths

### 4.1 End users

End-user path:

```text
loom-cli -> loom-cloud
```

This is the consumer paid-service path. The user keeps their local Loom on device and pays Uldren for
hosted sync, backup, and recovery storage. `loom-cloud` stores encrypted data and sync metadata but
does not process the user's content.

### 4.2 Enterprise users

Enterprise application path:

```text
loom-cli -> loom-server
```

The enterprise embeds `loom-cli` as a library or ships it with their software to enable Loom-backed
local functionality. Devices and applications sync to the enterprise's `loom-server`. The enterprise
can manage the data, enforce policy, run analysis, and integrate Loom capabilities into its own
software stack.

### 4.3 Enterprise businesses

Enterprise backup path:

```text
loom-server -> loom-cloud
```

An enterprise can back up its hosted Loom data to `loom-cloud`. In this relationship, `loom-cloud`
remains a zero-knowledge backup target. The enterprise keeps the operational and analytical surface in
`loom-server`, while Uldren provides durable encrypted storage for recovery.

## 5. Product boundaries

- `loom-cloud` does not depend on plaintext access to customer content.
- `loom-cloud` does not offer server-side data analysis, indexing, query execution, trigger
  execution, or program execution over customer content.
- `loom-cloud` stores encrypted frames, wrapped key metadata, account metadata, billing metadata,
  authorization labels, and sync state required to operate the backup and relay service.
- `loom-server` can perform data analysis across hosted Loom data when the deployment is configured as
  a trusted keyed remote.
- `loom-server` can be customer-operated on-premise or Uldren-operated as a cloud-hosted managed
  service.
- `loom-cli` remains usable without `loom-server` or `loom-cloud`.
- `loom-cli` is embeddable by enterprise software that wants Loom behavior in-process.

## 6. Implementation status

This document describes the intended platform architecture. The current repository has a CLI, C ABI,
language binding directories, sync primitives, encrypted storage, SQL support, compute support, and
facet substrates. Hosted wire protocols, live network sync, server packaging, and cloud operation are
target work owned by their specific specs and future implementation.

## 7. Source checks

- `specs/0001-overview-and-architecture.md` lines 11-19: the current source contains the engine,
  store, C ABI, CLI, bindings, sync pieces, SQL, compute, and facets, while hosted protocols and
  enterprise facades are target work.
- `specs/0001-overview-and-architecture.md` lines 23-35: Loom is a content-addressed, versioned
  filesystem with pluggable providers, sync, and optional hosted and encrypted extensions.
- `specs/0006-synchronization.md` lines 5-29: sync works between compatible local, browser, and remote
  Looms by transferring immutable objects and ref updates.
- `specs/0008-wire-protocols.md` lines 10-24: hosted REST, JSON-RPC, gRPC, MCP, and live network sync
  are target projection contracts, not current source-backed services.
- `specs/0031-end-to-end-encrypted-sync.md` lines 24-40 and 122-140: a keyless remote is storage and
  sync only, while a keyed remote can read, verify, and compute.
