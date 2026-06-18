# 0025 - Behavioral Conformance

**Status:** Complete for the current source-backed behavior boundary; runner expansion is split.
**Version:** 0.1.0. **Normative.**

This document defines behavioral conformance for Loom facades. Source is authoritative: the current
behavior catalog lives in `crates/loom-conformance::behavior`. Scenario tables describe expected
behavior; only executable runners certify an implementation.

## Current implementation

The current conformance crate provides:

- `Scenario`, a Given/When/Then scenario record;
- `BEHAVIOR_SUITES`, a catalog keyed by capability name;
- `EXECUTABLE_BEHAVIOR_SUITES`, the source-backed runnable subset;
- canonical vector suites for blob, object-model, templates, `exec-manifest`, interchange,
  substrate-model, drive-profile, meetings-profile, ledger-head, columnar-manifest, and
  table-identity. The
  substrate-model runner
  includes references, alias bindings, reference indexes, source-backed body kinds, opaque body
  preservation, pinned canonical body bytes for a page body containing `block_ref`, body epoch render
  gating, emoji registry canonical vectors, view
  source-facet/source-digest normalization, revision lookup variants, durable operation-log records,
  ticket/page/lifecycle profile operation-log records, lifecycle snapshot records, and Webish
  listener/route-table canonical round-trips plus route resolution semantics. The drive-profile
  runner includes Drive folder
  indexes, chunk manifests, file version indexes, profile snapshots, fold keys, snapshot control
  keys, upload-session control keys, operation-log control keys, conflict-index control keys, upload
  sessions, operation logs, conflict indexes, dehydrated-file marker bytes, conflict-copy naming, and
  current merge-matrix outcomes.
  The meetings-profile runner
  includes Meeting Memory snapshots, required projection-effect and projection-output coverage,
  redaction invalidation, extraction-review buckets, review transitions, and annotation evidence
  validation;
- `uldren-loom-conformance::studio_imports` executes the broad Studio importer fixture set through
  the reusable `ImportExecutionBatch` path for Redmine, Asana, Jira, Confluence, Slack, Drive,
  Markdown, Notion, and Granola Meetings. The runner checks report source scope and row counts,
  validates that each fixture's unsupported-field matrix is emitted as fidelity issues, and verifies
  representative profile-lowered state in Tickets, Pages, Chat, Drive, and Meetings;
- `uldren-loom-conformance::studio_imports` also exposes a Meetings import execution runner over
  the dedicated `execution-fidelity.json` vector set. It executes `granola-app`, `granola-api`,
  `granola-mcp`, and `csv` fixtures through the committed Meetings importer and verifies retry
  idempotence, retained source payloads, import checkpoints, explicit source states, profile
  selectors, snapshot counts, and invalid `source_state` rejection;
- executable runners for CAS, CAS facade, workspace, sync, queue, queue consumer offsets, delivery, VCS diff,
  locks, identity, ACL, KV, ephemeral KV, document, time-series, ledger, graph, vector, columnar,
  search, calendar, contacts, mail, merge-conflict, staging, file operations, file handles, symlinks,
  tags, restore, replay, squash, dataframe, exec, SQL state access, and the PIM trigger bridge;
- conformance-report inventory rows for local coordination evidence, including embedded lock
  coordinator behavior, CLI daemon runtime, transport degradation, host-native lock clients, MCP
  attached-session liveness, and unsupported mobile/browser or hosted lock surfaces;
- `uldren-loom-protocol-conformance`, a protocol-level runner crate for promoted MCP/hosted surfaces.
  Its focused network-access matrix certifies direct-peer allow and deny, trusted-proxy handling,
  missing-mTLS rejection, and denied-audit output. It records gRPC proxy and mTLS input as unsupported
  at the current gRPC acceptor boundary. Its CAS auth and ACL matrix certifies REST authentication
  failure and permission denial, JSON-RPC canonical denial, gRPC permission denial, persisted grant
  success, and immediate revoke denial across the source-backed CAS adapters.
- `uldren-loom-store` pins the search-specific derived-Tantivy artifact lifecycle through missing,
  rebuilding, ready, stale, failed, and unsupported statuses, including source-digest and
  engine-version stamps.
  Its current MCP runner calls the in-process MCP host and certifies `substrate_transact`
  bound-scope success, transaction rollback on stale document replacement, and `substrate_search`
  degraded lexical response shape, plus ticket `project_create`/`create`/`update_fields`/
  `project_rekey`, derived active and retired ticket-key alias resolution,
  and page
  `substrate_changes` operation-log cursors, structure operation-log cursors, `substrate_refs`
  document-write projection bootstrap, published page-body reference indexing, generic substrate
  alias bind/rebind/resolve/list/release behavior, page `block_ref` rendered read-through in
  `pages_get`, the first public chat MCP vertical for post/thread/reaction/message/event/cursor/
  presence/task/agent/handoff projection, MCP custom emoji registry registration before custom
  reaction use, read-only Meetings projection-output and extraction-review tools over stored profile
  snapshots, Meetings app rendering over imported profile data, and the Studio status resource's
  ticket-backed assigned-item plus markdown sections, plus the `view:planning.markdown`
  `substrate_view_get` executor. The page
  profile path also certifies guarded `pages_create` and `structures_create` stale-root rejection;
- `uldren-loom-hosted` unit tests certify Meetings REST and JSON-RPC adapter methods and served
  routes for projection-output, apply-projection-output, and extraction-review behavior over stored
  profile snapshots. Apply-route checks verify physical document, files, graph, search,
  SQL/dataframe, and ledger writes plus retry-safe ledger append behavior;
- `uldren-loom-hosted` unit tests certify served REST and JSON-RPC ticket listener routes for project
  create, project re-key, retired-prefix release, ticket create, ticket field update, ticket get, and
  ticket history over a real store-backed ticket workspace. The route test verifies derived keys,
  `expected_root`, re-key, release, get, update, and history. Hosted and MCP ticket writes share the
  `loom-tickets` reference helper for text-field backlinks and unresolved ticket-key candidates;
- `uldren-loom-tickets` unit tests certify the reusable Ticket lifecycle authorization foundation,
  including deterministic migration of existing ticket projects to the `write_access` policy and
  ownership-governed assignee and acceptance-authority checks;
- `uldren-loom-hosted` unit tests certify served REST and JSON-RPC Pages profile routes over real
  store-backed workspaces: `spaces` create/list/get, `pages` create/update/publish/get/history, and
  `structures` create/add-node/link-node/get. The shared implementation lives in `loom-pages`, so MCP
  and hosted routes use the same space, page, publish, reference-index, revision-index, and structure
  graph projection behavior;
- `uldren-loom-lifecycle` unit tests certify the store-backed Lifecycle service boundary for standard
  definition creation, instance creation, transition validation, snapshot reads, current-stage
  surface reads, and operation-log reads. `uldren-loom-mcp` server-feature tests certify the public
  Lifecycle MCP facade and structured output schema registration for definitions, instances,
  transitions, snapshot plans, current surfaces, snapshots, and operation logs. `uldren-loom-hosted`
  tests certify matching served REST and JSON-RPC lifecycle routes over a real store-backed workspace;
- `uldren-loom-protocol-conformance` certifies the served Meetings REST and JSON-RPC
  projection-output, apply-projection-output, and extraction-review routes
  in-process against a real store-backed snapshot, including post-apply readback of materialized
  document/file/graph/FTS/SQL-dataframe/ledger artifacts, vector job records, and Meetings FTS
  projection documents through the unified MCP `search` tool. The same crate also
  certifies served REST and JSON-RPC typed Chat and Drive write routes against real store-backed
  snapshots, including Chat emoji registry management and the reserved shared revision rows for
  `chat:{channel_id}:message:{message_id}` and `drive:file:{file_id}` through the generic 0061
  profile transaction helper;
- `uldren-loom-protocol-conformance::client_parity` provides the local-vs-remote `LoomClient` parity
  harness (Queue 11 task 420): a driver-agnostic runner `run_client_parity_suite(driver)` over a small
  `ParityDriver` adapter trait that both a local and a remote client satisfy, recording each observable
  into a `ParityReport` (fixed byte encoding for text, bytes, `u64`, optional bytes, and optional
  time-series points) so parity is proven by comparing outputs byte-for-byte, not merely by both sides
  succeeding. Setup is deterministic (fixed workspace/collection names, fixed commit timestamps) so
  content-addressed digests are stable across runs and stores. The in-process `LocalClientDriver` over
  `LocalLoomClient` runs in-sandbox (`local_driver_runs_the_full_parity_suite`); the socket-backed
  `RemoteClientDriver` (in the loom-cli live-test path) runs the same suite against a `RemoteLoomClient`
  over a live `loom serve remote` endpoint and asserts equal reports (`client_parity_local_matches_remote`,
  owner-run). The 420 slice covers Store/version, KV, CAS, Queue, Document read, TimeSeries latest, and a
  timestamped VCS commit; `document_query`, `sql_exec`/`sql_query`, and `Watch` are carried to task 430;
- `uldren-loom-mcp` server-feature tests certify the public Drive MCP read/write vertical for list,
  stat, read, list versions, create folder, upload session/chunk/commit, stale-base new-file
  conflict materialization, rename, move, delete, list conflicts, resolve conflict, and attached
  daemon-backed lease acquire/refresh/release/break lifecycle, durable `lock.acquired`,
  `lock.refreshed`, `lock.released`, and `lock.broken` Drive operation-log records, plus
  structured `write_admission` fence application, generic substrate write-admission policy
  management, and missing-admission rejection when a Drive surface scope is mandatory. MCP tests also certify
  scoped share-to-ACL projection plus admin-gated share-expiry and retention management over the
  canonical Drive indexes, including manual `share.expired` and `retention.applied` publication.
  These tests also certify stale
  file deletes as held-delete conflict records, `keep_conflict` delete application, and stale folder
  deletes as descendant survivor
  conflict records that leave the ancestor chain visible until every survivor conflict is resolved as
  `keep_conflict`;
- `uldren-loom-conformance` source-backs Drive share-index and retention-index canonical round trips,
  including direct grant records, expiring share grants, live-root pins, trash-subtree pins, and
  non-expiring legal-hold pin validation;
- `uldren-loom-hosted` tests certify equivalent in-process REST and JSON-RPC Drive adapter behavior
  for the same read/write vertical and conflict-resolution path, including stale file deletes held as
  conflicts until resolved, completed held folder deletes pruning the deleted folder root, and
  hosted share-to-ACL projection and retention management. `loom-hosted` also certifies Drive local
  OS projection primitives for canonical marker rendering, hydrate-on-read from marker bytes, and
  marker-byte rejection on local writes. The hosted served-router tests also certify
  daemon-opened REST and JSON-RPC Drive listener routing for
  list, create-folder, upload-session, upload-chunk, commit-upload, read, rename, delete, and
  list-conflicts paths, plus hosted share, share-expiry, and retention routes, over the same hosted adapter path.
  They also certify daemon-opened REST and JSON-RPC chat listener routing for
  message creation and emoji registry management over the current hosted chat adapter path;
- `uldren-loom-cli` tests certify that `loom serve drive` registers a durable Drive policy target
  and that the daemon applies scheduled Drive policy from the registry without requiring a served
  listener. In authenticated mode the daemon provisions an audited service principal, grants it
  policy-worker access, and applies due share-expiry and retention pins through the same hosted Drive
  adapter functions used by MCP, REST, and JSON-RPC;
- `uldren-loom-cli` tests certify daemon-opened `web/rest` static-file serving over namespace files,
  including root index resolution, `.html` fallback, directory index resolution, content type
  projection, hidden `.loom` path rejection, and daemon startup from a persisted static route table.
  `uldren-loom-cli` also tests `loom serve route` static route set/remove persistence. `uldren-loom-hosted`
  tests certify listener-backed
  Webish route-table dispatch for static routes, including host-specific longest-prefix routing to a
  different workspace/root, directory index resolution, `.html` fallback, `HEAD`, hidden `.loom`
  path rejection, fail-closed rejection of unsupported route modes, and hosted admin JSON-RPC plus
  REST static route management over stored `web/rest` listeners;
- scenario tables for workspace, files, VCS, KV, document, graph, vector, ledger, queue,
  queue-consumer, time-series, columnar, search, CAS, locks, control-plane helpers, and sync.

Executable certification currently exists for the suites listed in `EXECUTABLE_BEHAVIOR_SUITES`.
Canonical vector certification also includes the `lock-fence` suite for structured embedded and
external-authority fence packing. All other scenario tables are target certification inputs until
their public facades and runners exist.

Hosted protocol conformance currently includes source-backed CAS REST/JSON-RPC/gRPC operation
coverage for put, get, missing get, has, list, delete, invalid digest handling, and post-delete
absence; Queue REST/JSON-RPC/gRPC operation coverage for append, get, range, and len; and
Time-series REST/JSON-RPC/gRPC operation coverage for put, get, latest, and range; and Ledger
REST/JSON-RPC operation coverage for append, get, head, len, and verify; and native FTS
REST/JSON-RPC operation coverage for create, index, get, query, ids, remap, and delete; and Graph
REST/JSON-RPC operation coverage for the bounded native graph route set; and Vector REST/JSON-RPC
operation coverage for create, upsert, get, and search; and Columnar REST/JSON-RPC operation coverage
for create, append, scan, columns, rows, compact, inspect, source-digest, select, and aggregate; and
KV REST/JSON-RPC operation coverage for put, get, delete, list, and range. These are protocol
conformance slices, not a promotion of consumer-offset protocol conformance, Ledger range or proof
parity, OpenSearch expansion, full Graph CRUD/list parity, Vector delete/ids/metadata-index/source/model
routes, Arrow Flight or Flight SQL data-plane transfer, KV management projection, Redis or Memcached
compatibility, structured KV storage, generated clients, hosted lock protocol routes, or full hosted
listener parity.

## Studio Protocol Conformance Matrix

This matrix records the promoted Studio profile surface by profile, transport, current status, and
source proof. It is a conformance map, not a product scope promise. A row marked source-backed means a
test or vector currently exercises the named boundary. A row marked target is still specification
work or implementation work. A row marked shared-target belongs to a shared substrate before any one
profile can claim it.

| Profile | Surface | Transports | Status | Source proof | Remaining target work |
| --- | --- | --- | --- | --- | --- |
| Tickets | Project create, project re-key, retired-prefix release, ticket create/update/get/history, alias resolution, reference indexing, reusable lifecycle policy model | MCP, REST, JSON-RPC, Rust service | source-backed | `uldren-loom-protocol-conformance` MCP ticket suite; `uldren-loom-hosted` served ticket route tests; `uldren-loom-tickets` lifecycle policy unit tests; `uldren-loom-conformance::studio_imports` Redmine/Asana/Jira fixture execution | Comments, attachments, links, rank/watch tools, richer workflow transport conformance |
| Pages / Spaces / Structures | Space create/list/get, page create/update/publish/get/list/history, rendered block refs, structure create/list/add/link/get, Pages app selected page/backlink and structure rendering, app-only selected-page publish, app-only structure add/move/link actions | MCP, REST, JSON-RPC, MCP Apps | source-backed | `loom-pages` shared service tests through MCP and hosted route tests; protocol conformance for page refs and stale-root guards; MCP Pages app instance tests; `uldren-loom-conformance::studio_imports` Confluence/Markdown/Notion fixture execution | Structure decomposition and richer profile conformance |
| Chat | Message create/edit/redact, thread, reaction, cursor, presence, task, agent, handoff, emoji registry management, message revision rows | CLI, MCP, REST, JSON-RPC | source-backed | `uldren-loom-cli` chat parser coverage; `uldren-loom-mcp` chat suite; `uldren-loom-hosted` served Chat tests; `uldren-loom-protocol-conformance` hosted Chat post/edit/emoji checks; `uldren-loom-conformance::studio_imports` Slack fixture execution | Shared attachment byte handling through 0061, shared notification delivery through 0035, and broader profile conformance |
| Drive | Read/write/upload/conflict/share/retention/listener routing, CLI Drive service parity, local OS projection primitives, committed-upload revision rows, Drive Browser/Preview/Sharing/Conflicts/Retention app bundles | CLI, MCP, REST, JSON-RPC, MCP Apps | source-backed | `uldren-loom-cli` Drive parse coverage; `uldren-loom-mcp` Drive suite and Drive app render tests; `just verify-apps` Drive visual fixtures; `uldren-loom-hosted` Drive adapter and served route tests; `uldren-loom-protocol-conformance` hosted Drive upload checks; `uldren-loom-conformance::studio_imports` Drive fixture execution | OS-native placeholder hooks, background hydration/eviction workers, app-callable hydrate/dehydrate/worker-plan tools, and lease expiration sweeps |
| Drive leases | Acquire, refresh, release, break, structured fence tokens, write admission, lock operation-log records | MCP attached daemon | source-backed | `uldren-loom-mcp` attached-daemon lease tests; `uldren-loom-conformance` `lock-fence` vectors | REST/JSON-RPC listener routes are intentionally not claimed; background expiration sweeps remain target |
| Meetings | List/get/search, projection outputs, apply projection outputs, materialized output readback, extraction review, annotation/vocabulary/entity-merge/promotion writes, normalized import execution, hosted vector runtime execution when embedding is configured, Meeting Details/Memory Graph/Extraction Review/Meeting Search/Import Coverage/Access Audit app bundles | MCP, REST, JSON-RPC, local importer, MCP Apps | source-backed | `uldren-loom-protocol-conformance` MCP and hosted Meetings suites; `uldren-loom-hosted` Meetings route tests; `uldren-loom-interchange-io` Meetings import execution tests; `uldren-loom-mcp` Meetings app render test; `just verify-apps` Meetings visual fixtures | Raw source extraction, dedicated import-run read projection, export workflow tools, Meetings-specific audit-log projection, end-to-end target-profile promotion operations |
| Lifecycle | Define, define standard, instantiate, transition, definition/instance reads, snapshot plan, current surface, snapshots, operation log, registered prompts, session-bound active lifecycle surfacing | MCP, REST, JSON-RPC | source-backed | `uldren-loom-lifecycle`, `uldren-loom-mcp`, and `uldren-loom-hosted` lifecycle tests | Durable trigger keeper/public facade conformance remains with 0029; richer lifecycle app visualization remains with Surfaces |
| Surfaces | App definitions, elicitation records, prompt handoffs, render frames, resource catalog, IDL/C ABI/C++/Node/Python/iOS/JVM/Android/React Native/WASM catalog JSON helper, MCP app resources, template rendering, launcher metadata, app-only write dispatch, app-only visible-tool dispatch, subscriptions, built-in Directed Graph app foundation, catalog-derived profile graph data, built-in Chat app bundle foundation with channel/thread deep links and app-only message/presence actions, built-in Tickets app bundle foundation with selected-ticket deep links and app-only ticket update actions, built-in Pages app bundle foundation with selected page/structure deep links and app-only write actions, built-in Drive app bundle foundation with folder/file deep links and app-only folder/upload actions, built-in Meetings app bundle foundation with path-shaped Meeting Details links and app-only review actions, shared built-in app shell CSS, Playwright-backed VCS/Decisions/Directed Graph/Chat/Tickets/Pages/Drive/Meetings visual verification | CLI, IDL, C ABI, C++, Node, Python, iOS, JVM, Android, React Native, WASM, MCP resources/tools, and browser harness | source-backed | `uldren-loom-conformance` substrate-model vectors, FFI catalog helper test, Node/Python/iOS/JVM/Android/React Native catalog checks, MCP app/resource tests, `apps_call_tool` dispatch tests, and `just verify-apps` | Browser-host/iframe certification, expanded visual render fixtures, broader profile public-surface bindings |
| Attachments | Shared content-addressed profile attachments | Shared 0061 substrate | shared-target | No promoted executable runner yet | Implement shared attachment APIs before profile-specific attachment routes claim conformance |
| Notifications | Durable wakeups, resource updates, delivery cursors | Shared 0035/MCP delivery | shared-target | MCP app delivery tests cover app resource delivery only | Profile notification projection policy and cross-profile delivery conformance |
| Imports | Planned import records plus executable normalized fixture imports for Jira, Confluence, Slack, Drive, Meetings, Markdown, Notion, Asana, and Redmine | Local/importer paths, MCP | source-backed for normalized fixture boundary; partial for full importer targets | `uldren-loom-conformance` import-plan vectors and `studio_imports` execution runner; `uldren-loom-cli` Redmine, Markdown, Notion, and Meetings import tests; `uldren-loom-interchange-io` profile import execution tests; `uldren-loom-mcp` Redmine and Meetings import execution tests; `specs/studio/fixtures/*/expected/comparison.json` | Raw vendor/API extraction, MCP-assisted import ergonomics, full identity mapping, richer profile-specific lowerings, and live coexistence bridges |

## 1. Certification Rule

A backend claiming a behavioral capability must pass that capability's executable runner. Scenario text
alone is not certification.

Each behavioral runner must:

- use the public facade or provider surface that users call;
- assert stable `Code` values where an error is part of the contract;
- be deterministic across platforms and bindings;
- leave no hidden state dependency between scenarios;
- be callable from binding or protocol conformance once that surface is promoted.

## 2. Current Behavior Suites

| Suite key | Anchor | Current proof status |
| --- | --- | --- |
| `workspace` | workspace-as-bucket lifecycle | executable |
| `files` | POSIX-like local filesystem | scenario |
| `vcs` | git-like version control assurances | scenario |
| `kv` | key-value store | executable |
| `kv-ephemeral` | runtime-only KV cache tier | executable |
| `document` | document store by id | executable |
| `graph` | property graph | executable |
| `vector` | exact nearest-neighbor search | executable |
| `ledger` | append-only audit log and hash chain | executable |
| `queue` | append-only FIFO log | executable |
| `queue-consumer` | authority-local queue consumer offsets | executable |
| `delivery` | durable at-least-once delivery over append-log streams | executable |
| `vcs-diff` | cross-facet structural diff envelope | executable |
| `time-series` | time-series database range semantics | executable |
| `columnar` | columnar scan engine | executable |
| `search` | full-text search facade | executable |
| `dataframe` | dataframe facade | executable |
| `cas` | content-addressed store | executable |
| `cas-facade` | workspace-scoped CAS facade | executable |
| `exec` | program execution facade | executable |
| `sql-state-access` | SQL execution through `StateAccess` | executable |
| `pim-trigger` | PIM trigger execution bridge | executable |
| `sync` | clone, push, and bundle behavior | executable |
| `lock` | leased fenced lock coordinator | executable |
| `identity` | principal bootstrap and session authentication | executable |
| `acl` | authorization evaluator, role expansion, revocation, scopes, and selected engine PEP hooks | executable |
| `calendar` | calendar collection and entry facade | executable |
| `contacts` | contact book and entry facade | executable |
| `mail` | mailbox and message facade | executable |
| `merge-conflict` | in-progress merge lifecycle | executable |
| `staging` | workspace staging index | executable |
| `file-ops` | whole-file operations | executable |
| `file-handle` | byte-range file handles | executable |
| `symlink` | symlink creation and read-link | executable |
| `tags` | lightweight and annotated tags | executable |
| `restore` | path restore behavior | executable |
| `replay` | commit replay behavior | executable |
| `squash` | squash behavior | executable |

The `exec` runner covers the public gated/direct/batched execution facade, manifest identity, denied
operation rollback, metering, and promoted multi-facet `StateAccess` operations for files, CAS,
document, queue, time-series, ledger, graph, columnar, search, vector, and dataframe. SQL and PIM
execution have dedicated executable runners.

## 3. Executable Suites

### 3.1 Workspace

`run_workspace_behavior` certifies:

- a fresh Loom has zero workspaces;
- reading the default workspace does not create it;
- writing through the default selector creates exactly one `Default` workspace;
- multiple facets can coexist in the same workspace;
- deleting a workspace removes the id and frees the name;
- recreating a deleted workspace uses a new caller-provided id;
- bundle import preserves workspace id, name, refs, and facet set;
- cross-workspace operations are rejected with `CrossWorkspace`.

This runner also exercises current bundle behavior because workspace preservation across export/import
is part of the workspace lifecycle contract.

### 3.2 CAS

`run_cas_behavior` certifies:

- `put` returns the content digest;
- `get` round-trips stored canonical bytes;
- repeated `put` of the same bytes is idempotent;
- `get` of an unknown digest returns absence.

The current CAS runner maps directly to `ObjectStore`.

### 3.3 Queue

`run_queue_behavior` certifies:

- append assigns sequence `0`, then `1`;
- length reflects appends;
- get returns a payload or absence;
- range is half-open and ordered by sequence;
- commit and checkout restore prior stream state;
- clone preserves queue payloads.

The current queue runner maps to the source-backed core queue operations. Hosted gRPC, REST, and
JSON-RPC protocol conformance cover append, get, range, and len. Binding conformance,
consumer-offset protocol conformance, and broader queue-compatible protocols remain separate
projection work.

### 3.4 Sync

`run_sync_behavior` certifies:

- clone copies workspace identity, refs, facets, and object closure;
- checkout after clone materializes files;
- fast-forward push transfers only new objects and advances the destination branch;
- non-fast-forward push is rejected;
- authenticated push fails without source read;
- authenticated push fails without destination advance;
- authenticated push succeeds after source read plus destination write and advance grants exist;
- bundle export and import preserve workspace metadata, refs, and object closure;
- bundle encode and decode round-trip;
- non-bundle bytes are rejected;
- identity-profile mismatch is rejected with `Conflict`.

### 3.5 Queue Consumer

`run_consumer_behavior` certifies:

- a missing consumer offset reads as sequence `0`;
- consumer reads do not advance progress;
- read without advance redelivers the same entries;
- explicit advance moves the stored position;
- backward advance is rejected;
- reset may move the offset backward for replay;
- checkout does not mutate consumer progress;
- clone does not transfer consumer progress;
- invalid consumer ids and stream names are rejected.

### 3.6 Delivery

`run_delivery_behavior` certifies:

- delivery envelopes are appended in sequence;
- payload bytes, payload digest, optional expiry, and source cursor round-trip;
- unacked replay redelivers messages with the same message id;
- ack advancement resumes at the next sequence;
- authenticated delivery produce, replay, and ack fail closed without matching queue grants.

### 3.7 ACL

`run_acl_behavior` certifies:

- authenticated mode defaults to deny;
- an explicit deny beats a matching allow;
- selected file and configured KV engine operations fail closed until matching grants exist;
- a role grant authorizes through the engine PEP;
- revoking that role removes access on the next operation;
- `ref_glob` plus prefix scopes authorize only the matching ref and resource prefix;
- a prefix-scoped grant does not authorize a broad resource.

## 4. Scenario Suites

The following suites are present as checked-in scenario data. Scenario text is not executable
certification by itself; suites marked executable in section 2 are certified by source-backed runners.

### 4.1 Files

The files suite anchors to POSIX-like local filesystem behavior:

- create then read;
- exclusive create rejects an existing file;
- write truncates;
- same-operation visibility;
- reading a directory as a file is rejected;
- missing paths return `NotFound`;
- cross-workspace move or copy is rejected.

The current source-backed files APIs cover some of this behavior, but there is no shared executable
behavior runner in `uldren-loom-conformance` yet.

### 4.2 VCS

The VCS suite anchors to git-like version-control assurances:

- explicit staging;
- implicit staging;
- commit then log;
- empty commit refusal;
- file identity ignores mtime;
- empty tree is a diff base, not a checkout target;
- dirty checkout refusal;
- merge conflict behavior;
- mutating operations serialize.

`loom_core` has source-backed branch, commit, checkout, and merge behavior, but the shared behavior
runner is not implemented in the conformance crate.

### 4.3 Data Facets

Scenario data exists for:

- KV;
- document;
- graph;
- vector;
- ledger;
- time-series;
- search;
- columnar.

Scenario-only suites must not be used as certification until each owning facet exposes a stable public
facade and a runner exercises it.

## 5. Relationship to Conflict Policy

Behavioral conformance does not choose winners for distributed conflicts. Sync divergence and merge
policy follow `CONFLICT-RESOLUTION-MATRIX.md`.

The default sync baseline is:

- object transfer does not conflict;
- branch or ref divergence is detected deterministically;
- sync direction, peer order, platform, binding, and transport do not select a winner;
- explicit merge or resolution APIs handle facet-specific reconciliation.

Facet behavior runners may test merge helpers only after the owning facet spec defines the merge
contract and the source implements it.

## 6. Promotion Requirements

To promote a scenario suite to executable certification:

1. Define the public facade or provider surface.
2. Map every expected failure to a stable `Code`.
3. Implement `run_<suite>_behavior`.
4. Run it against `MemoryStore` or another source-backed reference.
5. Run it against `FileStore` where persistence is part of the claim.
6. Wire it into binding or protocol conformance only after that surface is public.
7. Update 0010 capability proof status from `scenario` to `executable`.

## 7. Target Work

0025a owns future runner expansion:

- `run_files_behavior`;
- `run_vcs_behavior`;
- behavior runners for SQL, public identity/ACL management facades, and additional PEP hooks as
  surfaces promote;
- provider-backed variants where persistence, crash recovery, or store profile affects behavior;
- binding and protocol behavior runners after 0007 and 0008 surfaces are reconciled;
- scenario table and 0010 capability proof-status synchronization for new runners.

## 8. Resolved Decisions

1. **Scenario text is not certification.** Only executable runners certify behavior.
2. **Executable suite names are source-backed.** `EXECUTABLE_BEHAVIOR_SUITES` is the authoritative
   runnable list; every other suite remains scenario data.
3. **Anchors remain useful.** Each scenario suite names the established tool or model it mimics.
4. **Conflict policy is shared.** Distributed conflict behavior is governed by the conflict matrix,
   not by per-facet ad hoc sync rules.
5. **Promotion updates 0010.** When a behavior runner lands, the capability registry proof status must
   be updated in 0010.
