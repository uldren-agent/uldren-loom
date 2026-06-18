# Slackish - Organization Chat

**Status:** Target design. **Version:** 0.1.0-target.
**Capability:** `chat`.

This document defines a Slack-like collaboration layer for principals on top of Loom. A principal may
be a human, service, or agent identity, but the primary product expectation is organization chat among
authorized participants, not a private inference session with an agent. It is a Studio application
profile, not a replacement for the core storage, sync, access-control, or MCP binding specs.

The design targets long-lived enterprise organizations: durable history, high fanout, attachments,
principal callbacks, auditability, retention, legal hold, end-to-end encrypted sync, and server-side
compute where a deployment intentionally grants the server keys.

**Design priority (owner correction, 2026-07-10):** Slackish is **principal collaboration first**.
Agent participation is an optional group-chat capability because agents can be principals, but
agent inference is not the default mental model and does not define the chat profile. Importing
*from* Slack is a distant optional nicety, not a requirement (see §19 and ADOPTION §1.3). Where a
Slack-like collaboration contract and an agent/inference feature conflict, the collaboration contract
wins; the agent feature must move to shared agent metering, compute, inference, or surfaces work.

## 1. Contract Boundaries

The design builds on these current and target contracts:

- `0021-append-log-layer.md` states that the current queue substrate stores each named stream as one
  canonical blob, and that the enterprise target is a structured stream value with an entry log keyed
  by sequence number.
- `0006-synchronization.md` defines sync as movement of immutable content-addressed objects plus mutable
  workspace refs. Object transfer verifies content by address, and refs advance only after reachable
  objects are present.
- `0031-end-to-end-encrypted-sync.md` defines the recommended blind-replica topology: a local client has
  the key and full access, while the remote stores ciphertext frames by keyed opaque labels.
- `P9-0016-mcp-server.md` defines `loom mcp` as the agent-facing projection over tools and resources,
  with HTTP/SSE allowed when it reuses the served authentication and authorization path.
- `0061.md` defines the shared operation substrate: envelope, sequencer, durable cursors, order
  tokens, conflict records, annotations, entity versioning, projections/views, cross-facet search,
  and agent budgets/metering (§20). The local envelope and sequencer text was rewritten 2026-07-05
  to reference 0061 §2/§3 directly (see §3 mapping note and §5.1).
- `SURFACES.md` defines the human experience layer (MCP Apps, elicitation flows, visualizations)
  rendered over this profile's projections.

This document depends on those boundaries. It does not make a keyless remote capable of reading,
indexing, summarizing, or policy-classifying content.

## 2. Cloud and Encryption Model

A local Loom can see the full organization while Loom Cloud stores the replicated state without plaintext.
That is the recommended enterprise default for private organizations.

The rule is simple:

- A **key-holding local replica** can read, write, search locally, run agent logic, and render the full
  organization.
- A **blind Loom Cloud replica** stores sealed objects and refs by opaque labels, relays sync, enforces
  account-level authorization by label, and sends wakeups. It cannot read messages, inspect attachments,
  compute search indexes, run SQL, summarize channels, classify content, or verify plaintext integrity.
- A **keyed Loom Cloud replica** is an intentional deployment mode where the tenant grants server-side
  keys to enable hosted search, hosted policy scanning, server-side agents, data loss prevention,
  previews, and analytics. It is not zero-knowledge.

The product should support all three relationships, but must not blur them. A feature that needs content
access must declare that it requires either a key-holding client or a keyed remote.

### 2.1 The Blind/Keyed Boundary

Decision (owner, 2026-07-04; resolves §15.8): the boundary is pinned as an **envelope field
visibility rule** plus a feature ledger. A blind host sees operation envelope metadata -
organization/scope/operation ids, `operation_kind`, sequence, actor principal and kind, timestamps,
idempotency key, `base_root`/`base_entity_version`, payload digest and size, policy labels, and
block-id-count metadata - and never the payload (bodies, the references block, attachment plaintext,
agent trace content). `operation_kind` is deliberately visible: it lets a blind host shape-validate,
rate-limit per kind (agent metering, 0061 §20), route notification wakeups per kind (0061 §16
decision 7), and enforce scope operating modes (0061 §7.1). The cost is a documented metadata leak - a blind host
learns activity *type* and volume, never content. Recorded in 0061 §3 as the substrate-wide rule.

Feature ledger:

- **Blind host may provide:** sequencing, storage, sync, wakeups, per-kind and per-principal rate
  limits and quotas, operating-mode enforcement, replay by sequence.
- **Keyed worker or key-holding client required for:** search indexing, DLP/scanning, previews and
  text extraction, mention badges (references are payload-resident, 0061 §19.3), digest and
  notification *content*, summarization, and any projection over plaintext.

Recommended deployment:

```text
Private organization:
  Local Loom replica holds keys and full organization.
  Loom Cloud is blind storage, sync, and wakeup relay.
  Search and agent execution happen locally or in a customer-controlled keyed worker.

Managed enterprise organization:
  Tenant-controlled keyed service holds keys inside the tenant trust boundary.
  Loom Cloud may host search, retention processing, DLP, and agent workers.
  Audit records mark every server-side content access.

Hybrid organization:
  Blind Loom Cloud stores canonical encrypted history.
  Separate keyed compute replicas receive selected workspaces or channels for approved workloads.
```

## 3. Organization Data Model

A Slackish organization is a Loom workspace group with Studio-specific resources and policies.

```text
organization
  channels
  threads
  direct-messages
  users
  agents
  memberships
  messages
  attachments
  reactions
  read-cursors
  presence
  tasks
  audit
  retention
```

The source of truth is an event log. User-facing views are derived projections.

```text
channel event log:
  message.created
  message.edited
  message.redacted
  message.restored
  reaction.added
  reaction.removed
  attachment.added
  thread.created
  task.created
  task.claimed
  task.completed
  handoff.requested

optional agent participant hooks:
  agent.invoked
  agent.replied
```

**Envelope: superseded by 0061 §2** (supersession rewrite, 2026-07-05). Chat events are operations
in the one canonical 0061 envelope; this profile defines only the operation kinds above and their
payload schemas. The local field vocabulary of earlier drafts maps as: event -> operation,
`channel_id` -> `scope_id`, `event_kind` -> `operation_kind`, `sender_principal`/`sender_kind` ->
`actor_principal`/`actor_kind`, `previous_channel_root` -> `base_root`, `event_id` ->
`operation_id`. Optional agent-authored messages carry the 0061 §2 agent identity block (§13).
Envelope canonical bytes and vectors are 0061 §16 decision 1, not owned here.

The user-visible message history is a projection over events, not the canonical log itself.

Current source backs the reusable chat service boundary in `loom-chat`, built on the chat model layer
in `loom-substrate::chat`: canonical chat operation payloads, channel operation logs, replay into
message/thread projections, reactions through the shared 0061 annotation store, task
create/claim/complete replay, optional agent invocation and reply linking, optional handoff request
projection, and operation-change cursor batches over channel records. `loom-hosted` re-exports this
service boundary for hosted REST and JSON-RPC adapters. `loom-mcp` source-backs the first public
ordinary chat tool vertical. The `loom` CLI source-backs the same reusable service boundary for
channel listing, channel create/rename, message projection, operation events, durable cursor
read/update, message create/edit/redact, thread creation, task create/claim/complete, optional agent
invocation/reply/handoff, reaction add/remove, and emoji registry list/register/unregister:
`chat_post_message`, `chat_edit_message`, `chat_redact_message`, `chat_add_reaction`,
`chat_remove_reaction`, `chat_create_thread`, `chat_messages`, `chat_fetch_events`, `chat_cursor`,
`chat_update_cursor`, `chat_presence`, and `chat_set_presence`, with protocol conformance coverage
for message, thread, reaction, cursor, presence, and sequenced event projection. The same source also
backs task tools and optional agent-participant tools: `chat_create_task`, `chat_claim_task`,
`chat_complete_task`, `chat_invoke_agent`, `chat_agent_reply`, and `chat_request_handoff`. Those
tools are not the definition of chat; they are extensions that use the same channel log when a
deployment enables task coordination or agent principals in a channel. Channel operation logs persist
as 0021a structured streams at
`profile/chat/v1/{workspace_id}/channels/{channel_id}/operations`, with one canonical
`ChatOperationRecord` per stream entry. Durable per-principal read cursors use the same 0021a consumer
offset substrate and expose head, next sequence, and unread count. Presence is an in-process,
permission-gated, TTL-bound ephemeral beacon with no durable storage. Hosted REST and JSON-RPC
adapter methods source-back the same post, edit, redact, reaction, thread, message, event, cursor,
presence, task, and optional agent-participant projection boundary. `loom serve` source-backs durable
`chat <workspace> <channel_id> --transport rest|json-rpc` listener admission and
daemon-opened REST/JSON-RPC route mounting for the current hosted chat adapter surface, including
workspace-scoped emoji registry list/register/unregister routes. Attachments, imports, broader
protocol conformance, and optional agent-participant policy integration remain target work. MCP,
hosted REST, and hosted JSON-RPC message create/edit/redact routes maintain shared substrate revision
rows for `chat:{channel_id}:message:{message_id}` through the generic 0061 profile transaction
helper. Agent budget enforcement and anomaly elicitation are shared 0061 §20 concerns; chat is only
one possible consumer when a deployment allows agent principals to participate in channels.

The first Chat MCP app bundles are source-backed in `loom-mcp`: Chat Channel, Chat Thread, Chat
Tasks, Chat Presence, and Chat Handoffs. They are binary-sourced MCP Apps, render through
`loom.chat`, support channel and thread deep links, read channel messages, threads, tasks, handoffs,
presence, cursors, and event summaries from the same Chat tool surface, and prepare app-only
`chat_post_message` and `chat_set_presence` calls through `apps_call_tool`. Browser visual
verification is covered by `just verify-apps`. Attachment handling UI is still target work because
the current promoted Chat MCP surface has no attachment byte upload/fetch tools.

### 3.1 Ephemeral State: Presence, Typing, Unread

Decision (owner, 2026-07-04): presence and typing are **ephemeral transport-level beacons** - TTL
expiry, never sequenced, never persisted, never part of any projection's source. Beacons are sealed
like payloads; a blind relay sees only the scope label and expiry (a documented metadata leak;
deployments may disable blind-mode presence relay entirely). Losing a beacon loses nothing durable.

Read state, by contrast, **is durable**: the per-principal read cursor is a 0035 consumer cursor
(0061 §12), advanced by `cursors.update`. Unread count is a pure function - scope head sequence
minus cursor - computable by any key-holding client from state it already replicates; no server-side
counting is required. Mention badges require payload access (references are payload-resident, 0061
§19.3): computed client-side in blind deployments, server-side only in keyed mode, per §2.1.

### 3.2 Tasks and Exclusive Claims

Decision (owner, 2026-07-04): **sequenced first-claim-wins, with an optional 0036 lease for
liveness.** `task.claimed` is a guarded operation validated post-sequence per the JIRAISH §7.3
pattern (blind and keyed topologies compute identical outcomes): the first claim in sequence order
wins; later claims reject with `task_already_claimed` - an extension of the shared JIRAISH §7.3
rejection enum, not a parallel enum. Claims end by explicit release, by `task.completed`, or by
expiry: a claimant may hold a 0036 fenced lease and carries its fencing token in claim and complete
operations; on lease expiry any worker may sequence `task.claim_expired`, making the task claimable
again. `task.completed` must cite the live claim, so a stale claimant's complete rejects with
`claim_expired`. The claim history is fully in the operation log; the lease is a liveness mechanism,
never the record.

### 3.3 Message Bodies

Decision (owner, 2026-07-04): **chat message bodies use the 0061 §9.1 canonical block content type.**
The §9.1 block model is the superset for all chat platforms: Slackish registers chat-specific block
kinds and marks (`mention` inline nodes bound through the 0061 §19 reference grammar, `emoji(name)`
against the organization emoji registry (§3.4), `context`/`section` presentation blocks, and opaque
interactive nodes), and unknown kinds round-trip untouched per §9.1 unknown-node preservation.
Platform imports (Slack mrkdwn/Block Kit, and later Teams/Discord/Matrix-class sources) **convert to
canonical blocks at import time**; unmappable constructs are preserved as opaque nodes and
fidelity-reported. This deliberately deviates from the JIRAISH store-native precedent (§20.2 typed
bodies): chat messages are small, write-once-heavy, and agent-read-heavy, so one canonical format for
AI consumption outweighs byte-native fidelity. The 0061 §9 `content_type` mechanism remains the
escape hatch for bodies that must be stored native.

### 3.4 Pins and the Emoji Registry

Two model additions from the Slack-mapping stress test (owner, 2026-07-04; fix the model, not the
importer):

- **Pins.** `annotation.pinned` / `annotation.unpinned` are 0061 §7 annotation kinds: scope-scoped,
  sequenced, auditable. Pinning generalizes cross-profile (starred/favorited entities in other
  profiles are the same kind).
- **Emoji registry.** Reaction kinds are arbitrary strings validated against an organization **emoji
  registry** - scope-level data like label taxonomies (0061 §7.1), auditable and importable. Unicode
  emoji are implicitly registered; custom names resolve to attachment-backed images.

Current source backs the canonical emoji registry model, MCP registry storage tools, hosted
REST/JSON-RPC registry management routes, and custom reaction validation in MCP plus hosted Chat
reaction-add paths. The stored registry is scoped by the Chat workspace/profile id.
Attachment-backed custom emoji images, import hooks, and profile UX remain target work.

## 4. Storage Layer

Decision (owner, 2026-07-04; resolves §15.2): **the channel log is a 0021a structured stream.** The
canonical identity of a channel or DM log is exactly the implemented 0021a stream root - sequence-keyed
prolly map of entry records, metadata blob, consumer-offset map - with operation envelopes (0061 §2)
as entry payloads. No new Merkle format is introduced: 0021a already provides append cost proportional
to the touched index path, half-open range reads by sequence, structural sharing, deterministic object
identity, inclusion proofs via the prolly spine, and clone/bundle/GC reachability, all with existing
conformance vectors. MMR and segmented-Merkle designs were considered and rejected as duplicate
formats (survey retained in this section's history).

The sequence index supports, via 0021a directly:

- append in amortized logarithmic time without rewriting history;
- range read by sequence; point read by sequence;
- proof of inclusion for audit and replication (prolly spine);
- stable root identity for sync and subscriptions;
- compaction that never changes visible message identity (§4.3).

**Secondary indexes are projections, not identity.** The time, sender, thread, attachment, and unread
indexes sketched in earlier drafts are 0061 §8 derived, rebuildable projections over the stream - they
are never part of the canonical channel root, so index evolution is never identity-affecting. Point
read by `message_id` resolves through the alias/id index projection to a sequence, then reads by
sequence.

### 4.1 Channel Logs and Workspace Branches

Decision (owner, 2026-07-04; resolves §15.4): **one ref per scope.** Every channel and DM conversation
is its own sequencer-owned ref whose tip is the channel log root (wrapped in a commit for the ordinary
0003b audit chain). The organization workspace branch holds organization-level state - membership, channel
manifest, config, retention policy - and is advanced by its own (much lower-rate) operations. This
gives per-channel CAS with no cross-channel write contention, per-channel wakeup granularity, and
instantiates 0061 §3's "scope root" literally: scope = channel, scope root = channel ref tip. A
single-branch layout remains a permissible degenerate deployment for tiny organizations; it changes
operations, not contracts.

### 4.2 Remote-Tracking Refs

Decision (owner, 2026-07-04; resolves §15.5): **sequencer-owned upstream with git-style
remote-tracking refs.** For any sequenced scope the shared ref has exactly one writer - the sequencer.
Every replica (laptop, keyed worker, blind cloud peer) holds `remotes/<remote>/<scope-ref>` mirrors
that advance only through sync, never through local commits. There is no push for sequenced scopes:
a local replica's unacknowledged operations live in a 0035 outbox until the sequencer acks them with
an assigned sequence, so a local replica is never a divergent branch and 0006's divergent-tip
rejection is never triggered in normal operation. Non-sequenced facets keep ordinary 0006
push/fast-forward semantics unchanged.

### 4.3 Retention Live Roots and Chat GC

Decision (owner, 2026-07-04; resolves §15.6): **epoch keys plus a fact-preserving live set** - 0061
§9/§9.1's crypto-shred mechanism specialized to chat:

- Channel payloads are encrypted under rolling **retention epochs** (time- or count-windowed per
  channel retention class; the §9.1 snapshot-epoch mechanism applied to an append-only log). Hard
  deletion retires epoch keys; targeted single-message hard deletion retires a per-message key within
  the epoch envelope.
- The **live root set** is: {current ref tip of every scope} ∪ {roots pinned by legal hold or export
  hold} ∪ {source roots of registered projections/views}. Superseded channel roots leave the live set
  once past the audit window; they are never live merely by having existed.
- **Compaction** drops shredded payload objects and unreachable history while entry records -
  sequence, payload digest, payload length, envelope metadata - persist forever, so message ids,
  version facts, and inclusion proofs survive per §14 and 0061 §9. Redaction (§7) never touches
  storage; only retention policy reaches this layer.
- Legal hold pins roots and blocks key retirement; hold release is a separate audited event kind.

## 5. Multi-Replica Coordination

Five laptops, each with a local Loom and an AI assistant, cannot safely coordinate shared channel writes
by independently appending to the same channel branch and then relying on ordinary current sync. Current
sync moves objects and fast-forwards refs. It rejects divergent branch tips instead of choosing or
merging a winner. Current queue storage also does not define concurrent append replay or resequencing.

Slackish therefore needs an explicit multi-writer coordination design above raw branch sync.

### 5.1 Blind Central Sequencer

**The sequencer contract is 0061 §3** (supersession rewrite, 2026-07-05): authorization by label,
blind shape validation per the §2.1 visibility rule, sequence assignment, alias/order-token
allocation, opaque persistence, CAS root advance on the per-scope ref (§4.1), wakeup emission, and
replay by sequence range. Wire-protocol details are 0061 §16 decision 2. What follows is the chat
instantiation of that one contract, kept for intuition.

The recommended enterprise default is a central sequencer that can be blind to message plaintext.

Flow:

1. A local assistant creates an encrypted event envelope and stores the referenced objects in its local
   Loom.
2. The local Loom submits the opaque event envelope, object labels, idempotency key, and previous channel
   root to Loom Cloud.
3. Loom Cloud verifies authorization by label, assigns the next channel sequence, persists the opaque
   event, advances the channel root with compare-and-swap, and emits wakeups.
4. Other local Looms pull the new opaque event and objects, decrypt locally, verify digests, update
   projections, and advance their cursors.

This is a central mediator for ordering and delivery, but not a plaintext server. It can safely sequence
opaque encrypted event envelopes as long as it can authorize the principal and validate protocol-level
shape without reading message bodies.

Properties:

- total order per channel;
- simple user experience;
- durable replay by sequence;
- zero-knowledge compatible;
- no server-side content search, DLP, preview, or summarization unless a keyed worker is added.

### 5.2 Multi-Writer Actor Logs

An alternative is to avoid a central sequencer by giving every device or assistant its own actor log:

```text
channel/{channel_id}/actors/{actor_id}/events
```

Each local Loom appends only to its own actor log. Loom Cloud stores and relays actor log heads. Every
client pulls all authorized actor logs and deterministically derives the channel view.

The merge function must define:

- event identity and idempotency;
- causal parents or vector-clock metadata;
- deterministic tie-breaking for concurrent events;
- provisional ordering in the UI;
- conflict rules for edit, redaction, task claim, and reaction events;
- compaction rules that preserve audit proofs.

Properties:

- no central ordering authority;
- offline writes are natural;
- clients eventually converge;
- ordering can be provisional;
- implementation and user-facing semantics are materially harder than central sequencing.

Actor logs are attractive for peer-to-peer and intermittently connected organizations. They are not the
recommended first enterprise Slackish path because enterprise chat users expect simple, stable channel
ordering.

### 5.3 Keyed Central Loom Server

A keyed central Loom server is the operationally simplest option. Clients submit writes to the server,
the server reads and validates content, assigns sequence numbers, updates projections, indexes search,
runs DLP, invokes hosted agents, and broadcasts updates.

This is compatible with Loom, but it is not zero-knowledge. It is appropriate when the tenant explicitly
chooses hosted compute inside a trust boundary.

### 5.4 Decision

Slackish should support the blind central sequencer as the default shared-channel coordination model.
Actor logs remain a target topology for decentralized or offline-heavy deployments. A keyed central Loom
server is a deployment option, not a requirement. Per 0061 §3, all three are deployment modes of the
one sequencer contract, not separate protocols.

Raw current branch sync alone is not a sufficient coordination protocol for multiple local assistants
writing the same shared conversation.

## 6. Attachments

**The attachment model is 0061 §7.1** (shared facility; the schema below is the shared one). This
section keeps only the chat upload flow and blind-mode behavior.

Attachments are first-class Loom content. File bytes are stored as content-addressed blobs or chunk lists.
Message events reference attachments by digest and metadata.

```text
attachment:
  attachment_id
  digest
  name
  media_type
  size
  encryption_scope
  uploaded_by
  created_at_ms
  scan_status
  retention_class
```

Attachment upload flow:

1. Client chunks and stores file content in the local Loom.
2. Client creates an attachment metadata object.
3. Client appends `attachment.added` or `message.created` with attachment references.
4. Sync transfers reachable attachment objects to the configured remote.
5. Background workers derive previews, text extraction, embeddings, and scan results only where they hold
   the required keys.

Blind Loom Cloud can store and sync encrypted attachment objects. It cannot scan, preview, classify, or
extract text unless a keyed worker performs that work.

## 7. Message Removal and Redaction

**Messages, edits, redactions, and reactions are thread-anchored annotations per 0061 §7**; this
section keeps the chat-specific redaction policy surface. Hard-deletion mechanics are §4.3.

Conversation history is mutable as a view, not as an audit log.

Default deletion appends a redaction event:

```text
message.redacted:
  message_id
  redacted_by
  reason
  redaction_policy
  replacement_text optional
```

The visible message projection hides the original body after redaction. The audit log keeps enough
information to prove what happened, subject to retention and legal hold.

Hard deletion is a policy operation:

- remove live references from visible projections;
- remove or expire index entries;
- delete or retire content encryption keys when crypto-shredding is allowed;
- mark old roots outside the retention live set;
- let garbage collection reclaim unreachable objects after the policy window.

Legal hold overrides hard deletion. User delete, administrator redact, retention expiry, and legal hold
release are separate event kinds.

## 8. Background Workers

Slackish requires background workers. They are part of the product contract, not optional cleanup.

Required workers:

- **GC worker:** computes policy-approved live roots and removes unreachable content after retention
  windows.
- **Compaction worker:** seals old segments, removes tombstoned payloads where hard deletion allows it,
  and rewrites projection indexes.
- **Index worker:** maintains search, mentions, thread views, unread counts, task views, and vector
  indexes.
- **Attachment worker:** scans files, extracts text, generates previews, computes embeddings, and expires
  unreferenced uploads.
- **Retention worker:** applies tenant retention, legal hold, export hold, and deletion policy.
- **Sync worker:** repairs replicas, resumes interrupted transfers, verifies roots, and reconciles
  remote-tracking refs.
- **Notification worker:** converts durable log advancement into MCP and WebSocket wakeups.

Keyless workers can operate only on labels, sizes, and encrypted frames. Content-aware workers require
keys and must run as auditable principals.

## 9. MCP as the Primary Protocol

MCP is the agent-native control plane. WebSocket is a secondary low-latency transport for UI fanout and
custom clients.

Expose chat state as MCP resources:

```text
loom://{workspace}/chat
loom://{workspace}/chat/{channel_id}/events
loom://{workspace}/chat/{channel_id}/messages
loom://{workspace}/chat/thread/{thread_id}/events
loom://{workspace}/chat/message/{message_id}
loom://{workspace}/chat/attachment/{attachment_id}
loom://{workspace}/chat/task/{task_id}
loom://{workspace}/chat/audit/{range}
```

Expose mutation and query through MCP tools:

```text
chat_fetch_events
chat_post_message
chat_edit_message
chat_redact_message
chat_add_reaction
chat_remove_reaction
chat.upload_attachment
chat.fetch_attachment
chat_create_thread
chat_create_task
chat_claim_task
chat_complete_task
chat_update_cursor
chat_invoke_agent
chat_agent_reply
chat_request_handoff
chat.search
```

All write tools execute as the resolved principal. Tool visibility is ergonomic only; every write is
checked by the policy enforcement point.

### 9.1 Resource URI Stability

Decision (owner, 2026-07-04; resolves §15.7): **URIs are built from stable ids and are permanent.**

- Every `loom://` path segment is a stable id (0061 §4), never an alias: `{workspace}`,
  `{channel_id}`, `{message_id}` are ids. Renaming a channel changes its alias, never its URI.
- A URI, once served, never changes meaning. Ids are never reused, so a URI can dangle (deleted and
  GC'd) but never point at a different entity.
- Archived and deleted scopes keep their URIs valid for history reads, subject to grants and
  retention.
- Alias-form URIs (`…/channel/by-name/{alias}`) may be served as non-subscribable redirect
  conveniences that resolve to the id form; subscriptions bind to id-form URIs only, so they survive
  renames.
- Imported permalinks (ADOPTION §1.3 alias preservation) become aliases resolving to id-form URIs.

## 10. Principal Callbacks and Subscriptions

Participants, including optional agent principals, are triggered by subscriptions, not by polling
every channel.

A client opens an MCP session and subscribes to the resources it is authorized to observe:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "resources/subscribe",
  "params": {
    "uri": "loom://organization/acme/channel/incident-123/events"
  }
}
```

When a new event is committed to Loom and the channel root advances, the MCP server emits a resource
update notification:

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/resources/updated",
  "params": {
    "uri": "loom://organization/acme/channel/incident-123/events"
  }
}
```

The notification is a wakeup. The client then fetches from its durable application cursor:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "chat_fetch_events",
    "arguments": {
      "channel_id": "incident-123",
      "after_sequence": 48291
    }
  }
}
```

Transport-level resume ids are not the delivery contract. The durable delivery contract is the Loom
channel sequence. Participants store their cursor with `chat_update_cursor`.

## 11. Elicitation

MCP elicitation is used when a participant, app, or server needs structured input before an action
proceeds. It is not the event callback mechanism.

Use elicitation for:

- approval before posting to a restricted channel;
- choosing which channel or thread should receive a response;
- confirming redaction scope;
- selecting whether an optional agent participant may invite another agent participant;
- classifying an ambiguous attachment;
- confirming a handoff from automated to human handling;
- selecting a retention class when policy allows user choice.

Example:

```json
{
  "jsonrpc": "2.0",
  "id": 9,
  "method": "elicitation/create",
  "params": {
    "message": "Approve posting this incident summary to #incident-response?",
    "requestedSchema": {
      "type": "object",
      "properties": {
        "approve": { "type": "boolean" },
        "visibility": {
          "type": "string",
          "enum": ["channel", "thread-only", "do-not-post"]
        }
      },
      "required": ["approve", "visibility"]
    }
  }
}
```

Elicitation responses become durable events when they affect organization state.

## 12. WebSocket Secondary Transport

WebSocket can be offered as a secondary transport for high-frequency UI updates and custom clients. It
must preserve the same semantics as MCP:

- authenticated principal context;
- authorized subscriptions;
- durable channel sequence cursors;
- idempotent writes;
- replay after reconnect;
- no stronger write authority than MCP tools.

WebSocket frames should carry the same event envelopes as `chat_fetch_events`. A WebSocket broadcast is
never the source of truth.

## 13. Optional Agent Participant Semantics

Agents are first-class principals with scoped grants and durable identity. This section applies only
when a deployment enables agent principals in organization chat; it is not the core chat contract and
must not be interpreted as making chat a one-to-one inference interface.

Agent messages must identify:

```text
agent_id
model_or_runtime
operator_principal optional
tool_calls optional
source_messages
confidence optional
policy_labels
trace_digest optional
```

Optional agent participant workflows:

- subscribe to channels, threads, tasks, and mentions;
- fetch events since the last cursor;
- decide whether to act;
- use elicitation when policy or ambiguity requires it;
- claim tasks before performing exclusive work;
- post results as normal messages or structured task updates;
- write audit events for sensitive tool calls.

Agents must be rate-limited and permissioned independently from the user who installed or invoked
them. The mechanism is 0061 §20 (owner, 2026-07-05; resolves ADOPTION G7) and is substrate-owned
because it applies equally to chat messages, issue edits, page writes, file operations, lifecycle
triggers, and compute. Slackish adds only chat-specific operation hooks when agent principals are
enabled: message-post kinds can meter against per-kind budgets, channel-level mute can map to a
single chat scope, and `chat_invoke_agent` can count against inviter and invitee budgets. The budget
document, anomaly policy, kill switch, trace retention, and elicitation policy remain owned by 0061
§20 rather than by the chat profile.

## 14. Performance Requirements

The design must meet these storage and protocol requirements:

- append does not rewrite the whole channel history;
- range read by sequence is logarithmic plus output size;
- fanout notifications do not require scanning channels;
- search and projections are derived from durable events;
- attachment bytes deduplicate by content address where encryption policy allows it;
- sync transfers only missing objects;
- reconnect uses durable sequence cursors;
- compaction preserves externally visible message ids and audit proofs;
- blind cloud mode remains usable for sync and wakeup without content access.

## 15. Open Design Decisions

All items resolved or 0061-owned as of 2026-07-05 (numbers retained so external references stay
valid):

1. Canonical envelope and payload encoding - **0061-owned** (§2 envelope; canonical bytes are 0061
   §16 decision 1). See §3 mapping note.
2. Merkle log structure - **resolved**: the 0021a structured stream is the channel log; secondary
   indexes are projections (§4).
3. Blind sequencer protocol and replay - **0061-owned** (§3 contract; wire frames are 0061 §16
   decision 2). Chat instantiation in §5.1.
4. Channel logs vs workspace branches - **resolved**: one sequencer-owned ref per scope (§4.1).
5. Remote-tracking ref model - **resolved**: sequencer-owned upstream, remote-tracking mirrors, no
   push for sequenced scopes, 0035 outbox for pending operations (§4.2).
6. Retention live-root algorithm - **resolved**: epoch keys plus fact-preserving live set (§4.3).
7. MCP resource URI stability - **resolved**: stable-id permanent URIs, alias redirects
   non-subscribable (§9.1).
8. Blind-versus-keyed compute boundary - **resolved**: envelope visibility rule plus feature ledger
   (§2.1, lifted to 0061 §3).
9. Conformance vectors - **split**: cursor-replay vectors are 0061-owned; the chat-specific vector
   set (event-log identity, redaction/pin/reaction semantics, attachment references, import
   round-trip for §19, ephemeral non-persistence) remains open - see §18.

## 16. Long-Term Contract Shape

All contracts in this document are long-term decisions; sequencing below is implementation staging
only and never changes a contract (no-v1 principle):

- local-first Loom replicas hold organization keys and can see the full organization;
- Loom Cloud is a blind sync and notification replica by default;
- a separate keyed compute deployment is used when hosted search, DLP, previews, or server-side agents
  are required - the boundary is the §2.1 visibility rule;
- channel history is the 0021a structured stream per scope (§4), sequenced per 0061 §3;
- MCP resources and subscriptions are the primary agent callback mechanism, on permanent stable-id
  URIs (§9.1);
- WebSocket mirrors the same event and cursor contract for clients that need lower latency;
- deletion is redaction by default (0061 §7), with hard deletion by retention epochs, key
  retirement, and GC (§4.3);
- optional agent participants are budgeted, metered, pausable, and killable per 0061 §20 (§13);
  ordinary human-to-human chat does not depend on that machinery.

## 17. Example Tool Surface (illustrative only - not a design decision)

Names, grouping, and parameters are examples to make assistant ergonomics concrete, not designed
contracts. Underscore-flattened per MCP; capability `chat`.

| Category | Tool | Description |
| --- | --- | --- |
| Channels | `channels.create` | Create a channel |
| Channels | `channels.archive` | Archive a channel |
| Channels | `channels.list` | List channels visible to the principal |
| Channels | `dms.open` | Open or fetch a direct-message conversation |
| Messages | `chat_fetch_events` | Range-read channel events from a sequence (§10) |
| Messages | `chat_post_message` | Append a UTF-8 `body_text` message event |
| Messages | `chat_edit_message` | Edit with UTF-8 `body_text`; version history kept |
| Messages | `chat_redact_message` | Redact; audit fact persists (§7) |
| Messages | `chat_add_reaction` | Append a reaction annotation |
| Messages | `chat_remove_reaction` | Remove a reaction (sequenced) |
| Messages | `chat_create_thread` | Start a thread from a message |
| Attachments | `chat.upload_attachment` | Store content-addressed attachment (§6) |
| Attachments | `chat.fetch_attachment` | Fetch attachment bytes by digest |
| Discovery | `chat.search` | Domain search over messages where keys permit |
| Tasks | `chat_create_task` | Create a lightweight channel task |
| Tasks | `chat_claim_task` | Claim exclusively before working (§3.2: sequenced first-claim-wins + optional lease) |
| Tasks | `chat_complete_task` | Complete a claimed task |
| Agents | `chat_invoke_agent` | Invoke an agent principal into a conversation with UTF-8 `prompt_text` (§13) |
| Agents | `chat_agent_reply` | Link a message as an agent reply |
| Agents | `chat_request_handoff` | Hand off from automated to human handling |
| Membership | `memberships.update` | Add/remove members; sequenced event |
| Cursors | `chat_cursor` | Read the principal's durable cursor (§10) |
| Cursors | `chat_update_cursor` | Advance the principal's durable cursor (§10) |
| Presence | `chat_presence` | Read live ephemeral presence |
| Presence | `chat_set_presence` | Set the principal's ephemeral presence |

## 18. Unfinished Tasks (pushed back from Queue 8)

`specs/0061.md` owns the shared substrate: operation envelope, sequencer protocol, durable cursors,
conflict records, annotation subsystem (messages, edits, redactions, reactions, and pins are
thread-anchored annotations per 0061 §7), entity versioning, view/projection machinery, and agent
budgets/metering (0061 §20). The 2026-07-04/05 design session (prompt 6) resolved every §15 item - see
the §15 annotations. What remains uniquely Slackish is implementation and vectors, unowned by any
queue:

- The chat-specific conformance vector set (§15.9 remainder): event-log identity over 0021a,
  redaction/pin/reaction semantics, attachment references, §19 import round-trip, ephemeral
  non-persistence (§3.1).
- Registration of the chat block kinds and marks (§3.3) against the 0061 §9.1 registry, and the
  emoji-registry and pin projections (§3.4).
- Chat projections and workers (§8) beyond the current reusable service boundary: unread counts,
  mention badges, search collections - definitions over 0061 §8, implementation unowned.
- Presence/typing relay implementation (§3.1) on the notification transport.
- Chat-specific metering defaults (§13) as shipped policy data.
- **Slack importer and coexistence bridge (candidate, on demand):** mapping pinned in §19; build
  unowned by decision (see intro design priority).

## 19. Slack Import Mapping

**Status (owner, 2026-07-04): demoted from requirement to candidate.** Importing from Slack is a
distant optional nicety (see the intro design priority). The mapping table below is retained at spec
level because it is the schema stress test that surfaced the §3.3/§3.4 model fixes - the model is
fixed, the importer is unowned and built only on demand. JIRAISH §25 conventions apply when it is
built: backdated unconditional operations with `import_provenance`, sequences assigned in `ts` order
per channel, unmapped users -> inactive placeholder principals (SCIM-mergeable), per-run fidelity report.

Current source backs the reusable Slack importer in `loom-interchange-io`, the CLI
`loom interchange import-slack` path, and the generic 0012 import-execution batch path used by MCP
assisted imports. The broad fixture at `specs/studio/fixtures/slack/` is derived from Slack's
workspace export, conversation, message, formatting, Block Kit, user, and file object documentation:
Slack workspace exports (`https://slack.com/help/articles/201658943-Export-your-workspace-data`),
message formatting (`https://docs.slack.dev/messaging/formatting-message-text/`), Block Kit blocks
(`https://docs.slack.dev/reference/block-kit/blocks/`), message events
(`https://docs.slack.dev/reference/events/message/`), retrieving messages
(`https://docs.slack.dev/messaging/retrieving-messages/`), files
(`https://docs.slack.dev/messaging/working-with-files/`), conversation objects
(`https://docs.slack.dev/reference/objects/conversation-object/`), user objects
(`https://docs.slack.dev/reference/objects/user-object/`), and file objects
(`https://docs.slack.dev/reference/objects/file-object/`).

The importer accepts normalized Slack snapshot JSON and Slack export zip files. Zip ingestion parses
`channels.json`, `users.json`, `usergroups.json`, and channel message JSON files; `users.json` and
`usergroups.json` are retained as unsupported workspace metadata for fidelity reporting rather than
misread as message files. The importer creates or reuses Chat channels, lowers plain text message
bodies through the reusable Chat service, creates thread records when the parent message is present,
registers reaction names, applies one reaction per imported reaction kind, skips duplicate channels
and messages idempotently, and emits the shared 0012 import report.

Source-backed coverage matrix:

| Slack field or source shape | Current source-backed handling |
| --- | --- |
| Normalized JSON snapshot | 1:1 accepted as the fixture and generic execution payload format. |
| Slack export zip `channels.json` | Parsed into channel records and folder-to-channel id mapping. |
| Slack export zip channel message JSON | Parsed into messages and normalized through the same importer path. |
| Slack export zip `users.json` / `usergroups.json` | Parsed and reported as unsupported workspace metadata. |
| Channel `id`, `name`, `handle` | Imported as Chat channel identity; duplicate channels are reused. |
| Channel flags, topic, purpose, properties, creator, timestamps, previous names, shared teams, members | Unsupported with fidelity issue. |
| Message `ts`, `text` / `body`, `thread_ts` | Imported as deterministic message identity, body bytes, and present-parent thread linkage. |
| Slack mrkdwn text | Retained as plain text. Canonical block conversion is target work. |
| Message `type`, `subtype`, author/user/team/channel type, edit/star/pin markers, Block Kit blocks, attachments, files, metadata, permalink, hidden/deleted/event markers | Unsupported with fidelity issue. |
| Reactions `name` | Reaction kind is registered and applied once for the message. |
| Reactions `users` and `count` | Unsupported with fidelity issue. |
| Workspace files, pins, custom emoji | Unsupported with fidelity issue. |
| Principal mapping, per-user reaction authorship, attachments, custom emoji assets, pin annotations, membership events, coexistence bridge | Target work. |

| Slack export | Slackish |
| --- | --- |
| organization | organization scope group |
| channels.json / groups / mpims / ims | scopes (channel, private channel, group DM, DM); membership synthesized from members lists |
| message (`ts`) | `message.created`, backdated; `ts` preserved as a msg alias; permalinks -> aliases -> id-form URIs (§9.1) |
| `thread_ts` replies | thread-anchored annotations (0061 §7), thread from parent `ts`; `reply_broadcast` -> thread reply + channel-visible flag |
| message body (mrkdwn / Block Kit) | converted to the 0061 §9.1 canonical block type per §3.3; unmappables preserved as opaque nodes + fidelity note |
| `edited` marker | `message.created` + `annotation.edited` with final text (full edit history is not exported -> fidelity note) |
| tombstoned deletions | `message.redacted` where the export shows one; silently absent messages -> fidelity note |
| reactions | `annotation.reaction_added` per reacting user, backdated; names validated against the imported emoji registry |
| custom emoji | organization emoji registry entries (§3.4), images as attachments |
| pins | `annotation.pinned` (§3.4) |
| files | 0061 §7.1 attachments, content-addressed; external/missing bytes -> metadata-only attachment + fidelity note |
| users.json | principals; unmapped -> inactive placeholder principals |
| bots / apps | inactive **agent-kind** placeholder principals with a minimal 0061 §2 agent identity block: `agent_id` from the app/bot id, `model_or_runtime = "slack_bot:<app_id>"`, `operator_principal` and `trace_digest` absent; mergeable onto real agent principals later |
| `channel_join` / `channel_leave` subtypes | synthesized `membership.changed` events |
| `channel_topic` / `channel_purpose` / `channel_name` | synthesized scope config operations (name changes preserve old aliases per 0061 §4) |
| huddles, calls, unmappable subtypes | plain messages + fidelity note |

**Coexistence bridge (if ever built):** per-channel `mirror(slack)` operating mode (0061 §7.1) - each
channel mirrors and cuts over independently; cutover is the audited mode-change operation recording
the last-synced baseline; DMs cut over per-conversation or at organization close-out. Export zips are
ingested by the import framework's archive-container support (ADOPTION §1.3).

**Fidelity report:** per-run, per-channel counts of mapped / degraded (opaque-preserved, metadata-only
attachments, edit-history loss) / dropped, plus the unresolved-principal and unresolved-reference
(0061 §19.3) lists.
