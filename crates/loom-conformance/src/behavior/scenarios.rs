//! Behavioral conformance *scenario tables* (pure Given/When/Then data) for every facet, plus the
//! [`BEHAVIOR_SUITES`] registry and the [`EXECUTABLE_BEHAVIOR_SUITES`] marker.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::Scenario;

/// `fs` facade, anchored to a POSIX / local filesystem.
pub const FS_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "create-then-read",
        given: "an empty working tree",
        when: "create_file(\"/a\", \"hi\") then read_file(\"/a\")",
        then: "the bytes equal \"hi\"",
    },
    Scenario {
        name: "create-exclusive",
        given: "\"/a\" exists",
        when: "create_file(\"/a\", ..)",
        then: "ALREADY_EXISTS (like O_CREAT|O_EXCL)",
    },
    Scenario {
        name: "write-truncates",
        given: "\"/a\" = \"hello world\"",
        when: "write_file(\"/a\", \"hi\")",
        then: "read_file(\"/a\") == \"hi\" (truncating write)",
    },
    Scenario {
        name: "per-operation-visibility",
        given: "a WRITE handle that wrote but did not close",
        when: "read_file on the same path",
        then: "the read sees the write (POSIX; writes apply per op, not on close)",
    },
    Scenario {
        name: "read-dir-is-eisdir",
        given: "\"/d\" is a directory",
        when: "read_file(\"/d\")",
        then: "IS_A_DIRECTORY",
    },
    Scenario {
        name: "missing-is-enoent",
        given: "no \"/nope\"",
        when: "read_file(\"/nope\")",
        then: "NOT_FOUND",
    },
    Scenario {
        name: "cross-workspace-refused",
        given: "any move/copy across workspaces",
        when: "fs.move/copy",
        then: "CROSS_WORKSPACE",
    },
];

/// `vcs` facade, anchored to git.
pub const VCS_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "explicit-staging",
        given: "explicit mode, \"/a\" modified",
        when: "stage then unstage",
        then: "status reflects staged then unstaged (like git add/restore)",
    },
    Scenario {
        name: "implicit-staging",
        given: "implicit-staging mode",
        when: "stage / unstage",
        then: "stage is a no-op success; unstage is UNSUPPORTED",
    },
    Scenario {
        name: "commit-then-log",
        given: "\"/a\" staged",
        when: "commit then log",
        then: "newest commit has the message and the staged tree",
    },
    Scenario {
        name: "empty-commit-refused",
        given: "nothing staged",
        when: "commit(allow_empty: false)",
        then: "NOTHING_TO_COMMIT",
    },
    Scenario {
        name: "identity-ignores-mtime",
        given: "same bytes/path/mode committed on two machines at different times",
        when: "compare Tree digests",
        then: "they are equal (git trees carry no mtime)",
    },
    Scenario {
        name: "empty-tree-not-checkout",
        given: "the empty-tree sentinel",
        when: "checkout(EMPTY_TREE)",
        then: "INVALID_ARGUMENT (valid as a diff base, not a checkout target)",
    },
    Scenario {
        name: "checkout-dirty-refused",
        given: "a dirty working tree",
        when: "checkout(other, force: false)",
        then: "WORKING_TREE_DIRTY",
    },
    Scenario {
        name: "merge-conflict-left-in-tree",
        given: "diverged conflicting edits",
        when: "merge",
        then: "conflicts listed, no commit, markers in the working tree",
    },
    Scenario {
        name: "vcs-lock-serializes",
        given: "two concurrent mutating vcs ops on one workspace",
        when: "both run",
        then: "serialized; the loser is LOCKED / CAS_MISMATCH",
    },
];

pub const DIFF_COMMITS_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "cross-facet-envelope",
        given: "two workspace commits with files, sql, kv, document, queue, cas, pim, and coarse facet changes",
        when: "vcs diff from..to",
        then: "the LMDIFF envelope groups natural unit changes and marks whole-blob fallback sections coarse",
    },
    Scenario {
        name: "workspace-scoped",
        given: "a commit digest outside the workspace history",
        when: "vcs diff uses it",
        then: "the call is rejected before returning a diff",
    },
];

pub const WATCH_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "pull-history-order",
        given: "one workspace branch with three first-parent commits",
        when: "watch poll starts from an empty cursor",
        then: "events are returned in commit order with workspace-local sequence numbers",
    },
    Scenario {
        name: "cursor-resume",
        given: "a cursor pinned to an older commit",
        when: "watch poll is called with max one",
        then: "only later commits are returned and the next cursor advances to the returned event",
    },
    Scenario {
        name: "path-events",
        given: "added, modified, and deleted file paths across commits",
        when: "watch poll returns commit events",
        then: "each event includes sorted path changes derived from the VCS diff substrate",
    },
    Scenario {
        name: "invalid-cursor",
        given: "a cursor whose commit is not reachable from the watched branch",
        when: "watch poll is called",
        then: "the call is rejected with CURSOR_INVALID",
    },
    Scenario {
        name: "unsupported-domain-marker",
        given: "a workspace commit that includes non-file facet changes and no file changes",
        when: "watch poll includes unsupported facet domain",
        then: "the event has no stable DomainChange records and includes an UnsupportedDomainDetail marker with the required capability",
    },
    Scenario {
        name: "path-prefix-and-kind-narrowing",
        given: "a commit with several file changes and non-file-only facets",
        when: "watch poll is called with path-prefix and kind filters",
        then: "only authorized matching file changes remain while sequence order and cursor advances remain stable",
    },
    Scenario {
        name: "debounced-streaming-delivery",
        given: "multiple commits arrive inside a short poll window",
        when: "watch stream runs with debounce_ms and stream interval",
        then: "events are coalesced by committed window while cursor progression remains monotonic",
    },
    Scenario {
        name: "watch-materialize",
        given: "a poll cursor and a destination append-log stream name",
        when: "watch materialize runs from a cursor and advances the cursor",
        then: "the append-log receives canonical loom.watch.batch.v1 payloads and returns the advanced source cursor",
    },
    Scenario {
        name: "hosted-projection-parity",
        given: "the core watch source events above",
        when: "hosted REST, JSON-RPC, gRPC, and MCP consume the same cursor",
        then: "the same DataChange and DomainChange ordering is projected, with transport-specific envelope serialization",
    },
    Scenario {
        name: "abi-cbor-batch-projection",
        given: "a watch batch from the core source",
        when: "the C ABI and local bindings read the batch and cursor",
        then: "cursor is opaque UTF-8, and batch bytes round-trip as loom.watch.batch.v1",
    },
];

/// `kv` facade, anchored to a key-value store.
pub const KV_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "put-get",
        given: "-",
        when: "put(\"k\",\"v\") then get(\"k\")",
        then: "\"v\"",
    },
    Scenario {
        name: "absent-is-none",
        given: "-",
        when: "get(\"missing\")",
        then: "None (not an error)",
    },
    Scenario {
        name: "overwrite",
        given: "\"k\"=\"v1\"",
        when: "put(\"k\",\"v2\")",
        then: "get(\"k\") == \"v2\"",
    },
    Scenario {
        name: "delete-idempotent",
        given: "-",
        when: "delete(\"k\") twice",
        then: "neither errors; \"k\" absent",
    },
    Scenario {
        name: "scan-range",
        given: "\"a:1\",\"a:2\",\"b:1\"",
        when: "scan(\"a:\")",
        then: "[\"a:1\",\"a:2\"] in key order",
    },
    Scenario {
        name: "versions-like-files",
        given: "\"k\"=\"v1\" committed, then put \"v2\"",
        when: "diff HEAD vs working",
        then: "\"k\" shows MODIFIED v1 -> v2",
    },
];

/// `kv-ephemeral` tier, anchored to cache-shaped key-value storage.
pub const EPHEMERAL_KV_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "ttl-expires",
        given: "a cache entry with ttl",
        when: "read after expiry",
        then: "None; expired entries behave as absent",
    },
    Scenario {
        name: "idle-ttl-expires",
        given: "a cache entry with idle ttl",
        when: "read before idle expiry then after idle expiry",
        then: "first read refreshes idle time; later read is absent",
    },
    Scenario {
        name: "read-through",
        given: "a versioned backing map holds a key",
        when: "cache get misses",
        then: "value is loaded from backing and populated in cache",
    },
    Scenario {
        name: "write-through",
        given: "an empty cache and versioned backing map",
        when: "cache write-through put",
        then: "cache is updated and the backing map enters versioned history",
    },
    Scenario {
        name: "configured-routing",
        given: "a map configured as ephemeral with read-through and write-through",
        when: "put and get through the tier-aware facade",
        then: "runtime cache semantics and versioned backing semantics are both honored",
    },
    Scenario {
        name: "write-behind-buffers-then-flushes",
        given: "a write-behind ephemeral map",
        when: "a put buffers the backing write, then flush_pending drains it",
        then: "the backing map is untouched until flush, then holds the value; the buffer empties",
    },
    Scenario {
        name: "back-pressure-pressure-rejects",
        given: "a write-behind map at its high-water mark with back_pressure=pressure",
        when: "another put arrives while saturated",
        then: "LOCKED; the write is rejected so the caller backs off, leaving no trace",
    },
    Scenario {
        name: "write-around-skips-cache",
        given: "a write-around ephemeral map",
        when: "a put writes the backing map",
        then: "the backing map holds the value and the cache is not populated",
    },
    Scenario {
        name: "gc-sweep-reclaims",
        given: "a cache entry past its ttl",
        when: "sweep_expired runs",
        then: "the expired entry is reclaimed and the sweep is idempotent",
    },
    Scenario {
        name: "checkout-invalidates-cache",
        given: "a heated ephemeral cache over a backing map",
        when: "the working tree is replaced by a checkout",
        then: "the cache is dropped so a later read reflects the new tree",
    },
];

/// Embedded `lock` coordinator, anchored to leased fenced coordination.
pub const LOCK_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "exclusive-excludes",
        given: "one owner holds an exclusive lock",
        when: "another owner tries the same key",
        then: "LOCKED and no fence is issued to the contender",
    },
    Scenario {
        name: "shared-coexists",
        given: "one owner holds a shared lock",
        when: "another owner takes shared, then a third takes exclusive",
        then: "shared succeeds; exclusive returns LOCKED",
    },
    Scenario {
        name: "semaphore-capacity",
        given: "a semaphore lock with bounded capacity",
        when: "holders exhaust permits",
        then: "the next acquire returns LOCKED",
    },
    Scenario {
        name: "lease-expiry",
        given: "a lock lease reaches its deadline",
        when: "the holder releases and another owner acquires",
        then: "release returns LOCK_LEASE_EXPIRED and the next owner succeeds",
    },
    Scenario {
        name: "fencing-stale",
        given: "an applied fence high-water",
        when: "a lower fence is applied",
        then: "FENCING_STALE",
    },
];

/// `identity` core registry behavior.
pub const IDENTITY_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "bootstrap-root",
        given: "a new principal registry",
        when: "effective principal is requested without a session",
        then: "the initial root principal is used while unauthenticated mode is active",
    },
    Scenario {
        name: "passphrase-auth",
        given: "a root passphrase is set",
        when: "authenticate with wrong then correct passphrase",
        then: "wrong passphrase is AUTHENTICATION_FAILED and correct passphrase creates a session",
    },
    Scenario {
        name: "root-removal",
        given: "a credentialed replacement principal exists",
        when: "root is removed",
        then: "root is gone and the replacement can authenticate",
    },
];

/// `acl` core authorization behavior.
pub const ACL_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "default-deny",
        given: "authenticated mode and no matching grants",
        when: "authorize a read",
        then: "PERMISSION_DENIED",
    },
    Scenario {
        name: "deny-precedence",
        given: "a matching allow and matching deny",
        when: "authorize the denied right",
        then: "PERMISSION_DENIED",
    },
    Scenario {
        name: "engine-pep",
        given: "a Loom with identity and ACL installed",
        when: "file and KV facade calls are attempted",
        then: "calls fail closed until matching grants are present",
    },
    Scenario {
        name: "role-grant-and-revoke",
        given: "a principal inherits a matching role grant",
        when: "the role is revoked after a successful read",
        then: "future reads immediately fail with PERMISSION_DENIED",
    },
    Scenario {
        name: "ref-and-prefix-scope",
        given: "a grant scoped to branch/release-* and path prefix docs/",
        when: "authorize matching and non-matching resources",
        then: "only the matching ref and prefix are allowed",
    },
    Scenario {
        name: "sync-write-gates",
        given: "authenticated source and destination looms",
        when: "clone and push run without then with required grants",
        then: "sync refuses missing read/write/advance/admin rights and passes after grants exist",
    },
];

/// `document` facade, anchored to a document store.
pub const DOCUMENT_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "put-get-by-id",
        given: "-",
        when: "put(\"u1\", doc) then get(\"u1\")",
        then: "the document",
    },
    Scenario {
        name: "absent-is-none",
        given: "-",
        when: "get(\"nope\")",
        then: "None",
    },
    Scenario {
        name: "index-find",
        given: "an index on \"status\"",
        when: "find(\"status\",\"open\")",
        then: "the matching ids",
    },
    Scenario {
        name: "delete-updates-index",
        given: "u1 indexed",
        when: "delete(\"u1\") then find",
        then: "u1 no longer returned",
    },
    Scenario {
        name: "malformed-rejected",
        given: "-",
        when: "put(\"u1\", invalid)",
        then: "INVALID_ARGUMENT",
    },
];

/// `graph` facade, anchored to a property graph.
pub const GRAPH_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "add-and-traverse",
        given: "-",
        when: "add nodes a,b and edge a-knows->b",
        then: "neighbors(a) == [b]",
    },
    Scenario {
        name: "delete-removes-edges",
        given: "a-knows->b",
        when: "remove_node(a)",
        then: "the edge is gone (no dangling edges)",
    },
    Scenario {
        name: "edge-to-missing-node",
        given: "\"ghost\" absent",
        when: "add_edge(a, ghost, ..)",
        then: "NOT_FOUND",
    },
    Scenario {
        name: "deterministic-traversal",
        given: "-",
        when: "neighbors(a) twice",
        then: "same order both times",
    },
];

/// `vector` facade, anchored to a vector index.
pub const VECTOR_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "upsert-get-remove",
        given: "-",
        when: "upsert/get/remove a vector",
        then: "get round-trips, then None after remove",
    },
    Scenario {
        name: "knn-search",
        given: "vectors under one (dim, metric) workspace",
        when: "search(q, 2)",
        then: "the 2 nearest; ties break by id (deterministic)",
    },
    Scenario {
        name: "dimension-mismatch",
        given: "-",
        when: "upsert(wrong-dimension)",
        then: "INVALID_ARGUMENT",
    },
    Scenario {
        name: "index-is-derived",
        given: "same upserts in the same order on two Looms",
        when: "compare vector-workspace digests",
        then: "equal (identity is the embeddings, not the ANN index)",
    },
];

/// `ledger` facade, anchored to an append-only audit log / hash chain.
pub const LEDGER_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "append-monotonic",
        given: "-",
        when: "append(e0), append(e1)",
        then: "sequences 0 then 1; entries immutable",
    },
    Scenario {
        name: "verify-detects-tamper",
        given: "an altered stored hash",
        when: "verify()",
        then: "false (true on an intact chain)",
    },
    Scenario {
        name: "appends-serialize",
        given: "two concurrent appends",
        when: "both run",
        then: "ordered, never interleaved",
    },
];

/// `queue` facade, anchored to an append-only log / FIFO.
pub const QUEUE_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "append-then-read",
        given: "-",
        when: "append m0, m1 then read_from(0)",
        then: "[m0, m1] in order",
    },
    Scenario {
        name: "read-from-offset",
        given: "m0,m1 appended",
        when: "read_from(1)",
        then: "[m1]",
    },
    Scenario {
        name: "no-edit-or-delete",
        given: "-",
        when: "inspect the facade",
        then: "no operation mutates an existing entry",
    },
    Scenario {
        name: "reads-dont-block-writers",
        given: "a reader streaming",
        when: "a writer appends",
        then: "the append succeeds without waiting",
    },
];

/// Queue consumer progress over append-only streams.
pub const QUEUE_CONSUMER_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "missing-offset-is-zero",
        given: "a stream with entries and no stored consumer progress",
        when: "consumer_position(worker)",
        then: "the next sequence is 0",
    },
    Scenario {
        name: "read-does-not-advance",
        given: "a consumer at sequence 0",
        when: "consumer_read(worker, max=2) twice",
        then: "both reads return the same entries and position remains 0",
    },
    Scenario {
        name: "advance-is-monotonic",
        given: "a consumer advanced to sequence 2",
        when: "consumer_advance(worker, 1)",
        then: "INVALID_ARGUMENT",
    },
    Scenario {
        name: "reset-may-replay",
        given: "a consumer advanced to the stream tail",
        when: "consumer_reset(worker, 0)",
        then: "position moves backward for explicit replay",
    },
    Scenario {
        name: "offsets-are-authority-local",
        given: "a source consumer has stored progress",
        when: "clone or bundle import the queue content",
        then: "the destination consumer starts at sequence 0",
    },
];

/// Generic durable delivery over append-only streams.
pub const DELIVERY_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "redeliver-until-ack",
        given: "two messages in a delivery stream and no subscriber ack",
        when: "the subscriber replays twice",
        then: "the same message ids are returned until ack advances",
    },
    Scenario {
        name: "ack-resumes-after-seq",
        given: "a subscriber has acked sequence 0",
        when: "replay resumes from stored ack",
        then: "delivery starts at sequence 1",
    },
    Scenario {
        name: "payload-digest-round-trip",
        given: "a delivery envelope with CAS-backed payload and source cursor",
        when: "the message is replayed",
        then: "payload bytes, digest, length, and source cursor round-trip",
    },
    Scenario {
        name: "authorization-before-egress",
        given: "authenticated mode with no matching queue grants",
        when: "produce, replay, and ack are attempted",
        then: "the delivery API fails closed before enqueue or payload egress",
    },
];

/// `time-series` facade, anchored to a TSDB.
pub const TIME_SERIES_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "append-then-range",
        given: "-",
        when: "append (cpu,100), (cpu,200) then range(cpu,0,150)",
        then: "the ts=100 point only",
    },
    Scenario {
        name: "range-inclusive-ordered",
        given: "-",
        when: "range(cpu,100,200)",
        then: "ts=100 then ts=200",
    },
    Scenario {
        name: "rollup-is-derived",
        given: "a 1-minute rollup",
        when: "underlying points change + recompute",
        then: "the rollup reflects them; it is not independently writable",
    },
];

/// `columnar` facade, anchored to Arrow/Parquet + Polars.
pub const COLUMNAR_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "append-then-scan",
        given: "-",
        when: "append \"10\",\"20\" to \"price\" then scan_column(\"price\")",
        then: "[\"10\",\"20\"] in order",
    },
    Scenario {
        name: "segments-versioned",
        given: "a committed column",
        when: "append + commit more",
        then: "diff shows the added segment; old commits read the old column",
    },
    Scenario {
        name: "engine-matches-rowstore",
        given: "the same data",
        when: "a predicate via the columnar engine vs a row scan",
        then: "equal results",
    },
];

/// `dataframe` facade, anchored to deterministic source adapters plus materialization.
pub const DATAFRAME_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "csv-plan-collect",
        given: "a dataframe plan bound to a CSV file",
        when: "collect runs scan, filter, select, and sort",
        then: "the deterministic batch rows are returned",
    },
    Scenario {
        name: "materialize-columnar",
        given: "a dataframe plan with a columnar materialization policy",
        when: "materialize is called",
        then: "a versioned columnar dataset is written",
    },
    Scenario {
        name: "versioned-input-and-output",
        given: "commits before and after input refresh",
        when: "checkout restores the older commit",
        then: "the dataframe plan and materialized output match the older state",
    },
];

pub const CONDITIONAL_MUTATION_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "entity-tag-exact-match",
        given: "a target has an owner-issued EntityTag for its current state",
        when: "a mutation presents that EntityTag as an exact condition",
        then: "the mutation applies atomically and returns the new EntityTag when policy permits",
    },
    Scenario {
        name: "content-tag-not-compare-token",
        given: "canonical bytes have a ContentTag and a facade exposes a representation tag",
        when: "a compare-before-write mutation is evaluated",
        then: "only the owner-issued EntityTag is accepted as the compare token",
    },
    Scenario {
        name: "absent-condition-collision",
        given: "the owner-defined target already exists in the atomic scope",
        when: "a create presents the absent condition",
        then: "the mutation does not apply and reports absent_mismatch or ALREADY_EXISTS by operation contract",
    },
    Scenario {
        name: "generation-mismatch",
        given: "a mutable collection has advanced from generation 7 to generation 8",
        when: "a mutation presents generation 7",
        then: "the mutation does not apply and reports CONFLICT with generation_mismatch",
    },
    Scenario {
        name: "expected-tag-mismatch-reason",
        given: "a target has EntityTag A and the caller presents EntityTag B",
        when: "replace_if_match or delete_if_match is evaluated",
        then: "the mutation does not apply and reports CONFLICT with expected_tag_mismatch",
    },
    Scenario {
        name: "missing-record-reason",
        given: "the owner-defined target is absent",
        when: "replace_if_present or delete_if_present is evaluated",
        then: "the mutation does not apply and reports CONFLICT with missing_record",
    },
    Scenario {
        name: "record-already-exists-reason",
        given: "the owner-defined target exists",
        when: "create_if_absent is evaluated",
        then: "the mutation does not apply and reports CONFLICT with record_already_exists",
    },
    Scenario {
        name: "operation-anchor-stale",
        given: "a fenced writer presents an operation anchor below the applied high-water mark",
        when: "the guarded mutation is evaluated",
        then: "the mutation does not apply and reports FENCING_STALE",
    },
    Scenario {
        name: "create-if-absent",
        given: "the owner-defined target is absent",
        when: "create_if_absent is evaluated",
        then: "it maps to the absent condition and creates the target atomically",
    },
    Scenario {
        name: "replace-if-present",
        given: "the owner-defined target exists",
        when: "replace_if_present is evaluated",
        then: "it requires an existing record and replaces without accepting an entity tag",
    },
    Scenario {
        name: "replace-if-match",
        given: "the owner-defined target exists with an EntityTag",
        when: "replace_if_match presents that EntityTag",
        then: "it maps to an exact condition and replaces only on exact match",
    },
    Scenario {
        name: "delete-if-present",
        given: "the owner-defined target exists",
        when: "delete_if_present is evaluated",
        then: "it requires an existing record and deletes without accepting an entity tag",
    },
    Scenario {
        name: "delete-if-match",
        given: "the owner-defined target exists with an EntityTag",
        when: "delete_if_match presents that EntityTag",
        then: "it maps to an exact condition and deletes only on exact match",
    },
    Scenario {
        name: "upsert-blind",
        given: "the caller has mutation authority but no compare condition",
        when: "upsert_blind is evaluated",
        then: "it maps to any and creates or replaces without using an EntityTag",
    },
    Scenario {
        name: "idempotency-key-independent-from-entity-tag",
        given: "a retryable replace_if_match carries both an idempotency key and an EntityTag",
        when: "the retry key matches but the EntityTag is stale",
        then: "the idempotency key only deduplicates replay and the stale EntityTag still rejects the mutation",
    },
    Scenario {
        name: "same-entity-tag-different-idempotency-key",
        given: "two replace_if_match calls present the same EntityTag but different idempotency keys",
        when: "both requests are evaluated",
        then: "the EntityTag controls compare-before-write and each idempotency key has independent replay scope",
    },
];

/// `cas` facade, anchored to a content-addressed store. Runnable today via `run_cas_behavior`.
pub const CAS_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "put-returns-digest",
        given: "-",
        when: "d = put(bytes) then get(d)",
        then: "bytes round-trip; d == blake3(bytes)",
    },
    Scenario {
        name: "put-idempotent",
        given: "-",
        when: "put(bytes) twice",
        then: "same digest; stored once",
    },
    Scenario {
        name: "unknown-is-absent",
        given: "-",
        when: "get(unstored-digest)",
        then: "absent (NOT_FOUND)",
    },
];

/// `cas` facade, workspace-scoped (the `cas_put`/`cas_get`/`cas_has`/`cas_list` helpers over a `Loom`),
/// anchored to a content-addressed store versioned per workspace.
pub const CAS_FACADE_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "facade-put-get-has",
        given: "a cas-facet workspace",
        when: "d = put(bytes) then get(d) and has(d)",
        then: "bytes round-trip; d == blake3(bytes); has is true",
    },
    Scenario {
        name: "facade-list",
        given: "blobs put in a workspace",
        when: "list()",
        then: "the workspace working-tree digests, sorted",
    },
    Scenario {
        name: "facade-idempotent",
        given: "-",
        when: "put(bytes) twice",
        then: "same digest; one entry (dedup)",
    },
    Scenario {
        name: "facade-unknown-absent",
        given: "-",
        when: "get/has(unstored-digest)",
        then: "absent (None / false)",
    },
    Scenario {
        name: "facade-versioning",
        given: "commits over the cas facet",
        when: "checkout an earlier commit",
        then: "the reachable blob set is restored per workspace",
    },
];

/// `merge-conflict` facade, the in-progress merge state machine (0003b): a divergent same-path change
/// enters a recoverable conflict state that is either aborted or resolved and continued.
pub const MERGE_CONFLICT_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "conflict-enters-in-progress",
        given: "two branches change the same path differently",
        when: "merge the other branch",
        then: "conflicts are reported, an in-progress merge is recorded, the branch tip is unchanged",
    },
    Scenario {
        name: "abort-restores",
        given: "an in-progress merge",
        when: "merge_abort",
        then: "the pre-merge working tree is restored exactly and the state is cleared",
    },
    Scenario {
        name: "continue-requires-resolution",
        given: "an in-progress merge with unresolved paths",
        when: "merge_continue",
        then: "CONFLICT until every path is resolved",
    },
    Scenario {
        name: "resolve-and-continue",
        given: "an in-progress merge with each path resolved",
        when: "merge_continue",
        then: "a two-parent merge commit is recorded and the branch advances",
    },
];

/// `staging` facade: one shared per-workspace index across all facets, with a git-familiar flow where
/// `commit` records the whole working tree and `commit_staged` records only the staged index.
pub const STAGING_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "status-classifies",
        given: "a committed file, a modified file staged, and a new file",
        when: "status",
        then: "staged shows the staged change, untracked shows the new file",
    },
    Scenario {
        name: "commit-staged-records-index",
        given: "one of several changes staged",
        when: "commit_staged",
        then: "only the staged change is committed; the rest stay in the working tree",
    },
    Scenario {
        name: "commit-everything-records-worktree",
        given: "staged and unstaged changes",
        when: "commit",
        then: "the whole working tree is committed and the workspace is clean",
    },
    Scenario {
        name: "unstage-resets-to-head",
        given: "a staged change",
        when: "unstage",
        then: "the change leaves the index and reverts to its HEAD state",
    },
];

/// `file-ops` facade: the whole-file working-tree operations (write, read, append, remove), anchored to
/// a POSIX / local filesystem.
pub const FILE_OPS_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "write-read-round-trip",
        given: "an empty working tree",
        when: "write_file then read_file",
        then: "the bytes round-trip",
    },
    Scenario {
        name: "write-truncates",
        given: "an existing file",
        when: "write_file with new content",
        then: "the file is replaced (truncating write)",
    },
    Scenario {
        name: "append-creates-and-concatenates",
        given: "a missing file",
        when: "append_file twice",
        then: "the file is created then concatenated (like `>>`)",
    },
    Scenario {
        name: "append-missing-parent",
        given: "a path under a nonexistent directory",
        when: "append_file",
        then: "NOT_FOUND",
    },
    Scenario {
        name: "remove-deletes",
        given: "an existing file",
        when: "remove_file then read_file",
        then: "the file is absent (NOT_FOUND)",
    },
];

/// File-handle and byte-range I/O behavior, anchored to POSIX `open`/`pread`/`pwrite`/`ftruncate` and
/// open file descriptions. Runnable today via `run_file_handle_behavior`.
pub const FILE_HANDLE_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "write-at-zero-fills-and-overwrites",
        given: "a missing file",
        when: "write_at past the start, then read_at",
        then: "the gap is zero-filled and reads past the end clamp (pwrite/pread)",
    },
    Scenario {
        name: "truncate-grows-and-shrinks",
        given: "an existing file",
        when: "truncate_file smaller then larger",
        then: "bytes drop, then the file zero-extends (ftruncate)",
    },
    Scenario {
        name: "streamed-edit-matches-whole-rewrite",
        given: "a chunked file larger than the chunk threshold",
        when: "write_at edits one window",
        then: "the content address equals storing the whole edited bytes (chunks dedup)",
    },
    Scenario {
        name: "handles-share-one-inode",
        given: "two handles open on the same path",
        when: "one truncates and the other write_at past the new end",
        then: "both see the shared inode result (5 zeros then the byte)",
    },
    Scenario {
        name: "delete-on-last-close",
        given: "two handles open on a path",
        when: "the path is removed, then a handle writes and reads",
        then: "the path stays gone (no resurrection), the inode lives until last close",
    },
    Scenario {
        name: "replace-while-open",
        given: "a handle open on a path",
        when: "write_file replaces the whole file",
        then: "the open handle sees the new content (O_TRUNC same inode)",
    },
    Scenario {
        name: "open-modes",
        given: "the four open modes",
        when: "open Read on a missing file, write a read-only handle, read a write-only handle",
        then: "NOT_FOUND, INVALID_ARGUMENT, INVALID_ARGUMENT respectively",
    },
    Scenario {
        name: "handles-survive-reload",
        given: "an open handle with an advanced cursor",
        when: "export then import the engine state",
        then: "the handle id and cursor survive (open-file table is operational metadata)",
    },
];

/// Symlink behavior, anchored to POSIX `symlink`/`readlink` (git-style storage). Runnable today via
/// `run_symlink_behavior`.
pub const SYMLINK_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "create-and-read",
        given: "an empty working tree",
        when: "symlink then read_link",
        then: "the target round-trips; dangling targets are allowed",
    },
    Scenario {
        name: "stat-reports-symlink",
        given: "a symlink",
        when: "stat the link path",
        then: "the kind is Symlink and the mode carries S_IFLNK",
    },
    Scenario {
        name: "read-link-errors",
        given: "a regular file and a missing path",
        when: "read_link each",
        then: "INVALID_ARGUMENT for the file, NOT_FOUND for the missing path",
    },
    Scenario {
        name: "create-errors",
        given: "an existing path and a missing parent",
        when: "symlink over each",
        then: "ALREADY_EXISTS over an existing path, NOT_FOUND for the missing parent",
    },
    Scenario {
        name: "survives-commit",
        given: "a committed symlink",
        when: "checkout the commit",
        then: "the symlink mode and target survive the tree round-trip",
    },
];

/// Tag behavior, anchored to git tags (lightweight and annotated). Runnable today via
/// `run_tags_behavior`.
pub const TAGS_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "lightweight-tag-at-head",
        given: "a committed workspace",
        when: "tag_create at HEAD with no message",
        then: "the tag points straight at the HEAD commit",
    },
    Scenario {
        name: "annotated-tag-stores-object",
        given: "a committed workspace",
        when: "tag_create with a message and tagger",
        then: "the ref points at a Tag object carrying the message, target, and tagger",
    },
    Scenario {
        name: "rev-resolution",
        given: "a branch and a commit digest",
        when: "tag_create against a branch name and against a digest",
        then: "both resolve to the same commit; an unknown rev is NOT_FOUND; a non-commit is INVALID",
    },
    Scenario {
        name: "list-and-target",
        given: "several tags",
        when: "tag_list and tag_target",
        then: "names come back sorted and each target reads back (raw ref)",
    },
    Scenario {
        name: "delete-and-rename",
        given: "an existing tag",
        when: "rename then delete",
        then: "rename preserves the target; duplicate/missing names error; delete is NOT_FOUND after",
    },
];

/// Workspace behavior, anchored to the workspace-as-bucket contract. Runnable today via
/// `run_workspace_behavior`.
pub const WORKSPACE_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "fresh-has-zero-workspaces",
        given: "a fresh Loom",
        when: "list workspaces and read Default",
        then: "the list is empty and the read returns NOT_FOUND",
    },
    Scenario {
        name: "default-write-creates",
        given: "a fresh Loom",
        when: "write through the default workspace selector",
        then: "Default is created exactly once",
    },
    Scenario {
        name: "facets-coexist",
        given: "Default exists with one facet",
        when: "a second facet writes to Default",
        then: "the same workspace id has both facets",
    },
    Scenario {
        name: "canonical-root-and-facet-paths",
        given: "one workspace has user files and a reserved facet path",
        when: "commit, bundle import, and checkout",
        then: "root files and .loom/facets entries survive in one tree",
    },
    Scenario {
        name: "delete-recreate",
        given: "Default exists",
        when: "delete it and create Default again",
        then: "the old id is gone and the name can be reused",
    },
    Scenario {
        name: "bundle-preserves-facets",
        given: "a workspace with multiple facets and a committed branch",
        when: "export then import a bundle",
        then: "the imported workspace preserves id, name, refs, and facet set",
    },
    Scenario {
        name: "cross-workspace-rejected",
        given: "two workspace ids",
        when: "a single-workspace operation is checked across both",
        then: "CROSS_WORKSPACE",
    },
];

/// Sync behavior, anchored to the direct (Loom-to-Loom) and offline (bundle) transfer contract.
/// Runnable today via `run_sync_behavior`.
pub const SYNC_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "clone-copies-workspace",
        given: "a source workspace with a committed branch",
        when: "clone_workspace into a destination with a new id",
        then: "the clone keeps the name, branch tip, and facet set under the new id",
    },
    Scenario {
        name: "clone-is-bare-until-checkout",
        given: "a freshly cloned workspace",
        when: "read a file before and after checkout",
        then: "NOT_FOUND while bare, then the file materializes after checkout",
    },
    Scenario {
        name: "fast-forward-push",
        given: "a clone whose branch trails one new source commit",
        when: "push_branch the branch",
        then: "only the new objects move and the branch advances to the source tip",
    },
    Scenario {
        name: "non-fast-forward-refused",
        given: "source and destination committed independently from a shared base",
        when: "push_branch the branch",
        then: "NOT_FAST_FORWARD",
    },
    Scenario {
        name: "bundle-preserves-workspace",
        given: "a workspace with facets, a branch, and a tag",
        when: "bundle_export then bundle_import",
        then: "id, name, facets, branch refs, tag refs, and object closure survive",
    },
    Scenario {
        name: "bundle-round-trips",
        given: "an exported bundle",
        when: "Bundle::encode then Bundle::decode",
        then: "the decoded bundle equals the original",
    },
    Scenario {
        name: "invalid-bundle-rejected",
        given: "bytes that are not a bundle frame",
        when: "Bundle::decode",
        then: "an error (bad magic / corrupt frame)",
    },
    Scenario {
        name: "profile-mismatch-rejected",
        given: "a bundle whose identity profile differs from the destination",
        when: "bundle_import",
        then: "CONFLICT (no silent rehash)",
    },
];

/// Restore / path-restricted checkout behavior, anchored to `git restore`. Runnable today via
/// `run_restore_behavior`.
pub const RESTORE_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "restore-file-reverts",
        given: "a committed file edited in the working tree",
        when: "restore_file from HEAD",
        then: "the path reverts to the committed content; HEAD is untouched",
    },
    Scenario {
        name: "restore-file-removes-when-absent",
        given: "an untracked working-tree file",
        when: "restore_file from a snapshot lacking it",
        then: "the path is removed from the working tree",
    },
    Scenario {
        name: "restore-path-subtree",
        given: "edits inside and outside a subtree",
        when: "restore_path on the subtree prefix",
        then: "only the subtree is reset to the snapshot; paths outside are untouched",
    },
];

/// History-replay behavior (cherry-pick / revert / rebase), anchored to git. Runnable today via
/// `run_replay_behavior`.
pub const REPLAY_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "cherry-pick-applies",
        given: "a commit on another branch",
        when: "cherry_pick it onto HEAD",
        then: "its change applies as a new single-parent commit preserving the author",
    },
    Scenario {
        name: "revert-undoes",
        given: "a commit",
        when: "revert it",
        then: "a new commit restores the prior content",
    },
    Scenario {
        name: "rebase-replays",
        given: "a branch that diverged from the target",
        when: "rebase onto the target",
        then: "the branch commits replay linearly atop the target",
    },
    Scenario {
        name: "dry-run-previews-conflicts",
        given: "a conflicting cherry-pick",
        when: "cherry_pick with dry_run, then for real",
        then: "both report the conflict and make no change (atomic)",
    },
];

/// Squash behavior (collapse a commit range into one), anchored to git. Runnable today via
/// `run_squash_behavior`.
pub const SQUASH_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "squash-collapses",
        given: "several commits after a base",
        when: "squash onto the base",
        then: "one commit parented on the base holds the tip tree; all files remain",
    },
    Scenario {
        name: "squash-rejects-bad-base",
        given: "a base that is the tip or not an ancestor",
        when: "squash onto it",
        then: "INVALID_ARGUMENT",
    },
];

/// Protected-ref behavior, anchored to branch protection in enterprise VCS systems. Runnable today via
/// `run_protected_ref_behavior`.
pub const PROTECTED_REF_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "fast-forward-only-denies-rewrite",
        given: "branch/main has a fast-forward-only protected-ref policy",
        when: "a squash tries to rewrite the branch tip",
        then: "PERMISSION_DENIED and the ref remains unchanged",
    },
    Scenario {
        name: "signature-review-fails-closed",
        given: "branch/main requires commit signatures, ref-advance signatures, and reviews",
        when: "a normal commit tries to advance the branch without proof records",
        then: "PERMISSION_DENIED",
    },
    Scenario {
        name: "retention-governance-locks-tag",
        given: "tag/release has retention and governance locks",
        when: "delete is attempted",
        then: "PERMISSION_DENIED and the tag remains present",
    },
];

/// Every behavioral suite, keyed by the capability that gates it. A backend runs the suite for each
/// capability it advertises.
/// `calendar` facade, anchored to CalDAV / iCalendar (RFC 5545/4791) semantics over structured records.
pub const CALENDAR_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "mkcalendar-before-put",
        given: "no collection alice/work yet",
        when: "put_entry(alice, work, event)",
        then: "the put is NOT_FOUND until create_collection runs first",
    },
    Scenario {
        name: "entry-crud-and-etag",
        given: "an existing collection",
        when: "put_entry then edit the record and put again",
        then: "get returns the record and the content-addressed ETag changes on edit",
    },
    Scenario {
        name: "recurrence-range",
        given: "a weekly event with one EXDATE plus a one-off",
        when: "range(from, to) over the month",
        then: "occurrences are expanded, the EXDATE is removed, ordered by start",
    },
    Scenario {
        name: "ics-projection-round-trip",
        given: "an imported iCalendar VEVENT",
        when: "put_ics then entry_ics",
        then: "the .ics is serialized from the stored record (UID and DTSTART preserved)",
    },
    Scenario {
        name: "changes-since-diff",
        given: "two collection states",
        when: "diff_entries(old, new)",
        then: "per-UID added/updated/removed changes are reported with new ETags",
    },
    Scenario {
        name: "versions-and-clones",
        given: "entries committed across two commits",
        when: "checkout an earlier commit, then clone the workspace",
        then: "checkout restores the earlier entry set and a clone preserves the entries",
    },
];

/// `contacts` facade, anchored to CardDAV / vCard (RFC 6350/6352) semantics over structured records.
pub const CONTACTS_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "mkcol-before-put",
        given: "no address book alice/personal yet",
        when: "put_entry(alice, personal, contact)",
        then: "the put is NOT_FOUND until create_book runs first",
    },
    Scenario {
        name: "contact-crud-and-etag",
        given: "an existing book",
        when: "put_entry then edit and put again",
        then: "get returns the record and the content-addressed ETag changes on edit",
    },
    Scenario {
        name: "vcard-projection-round-trip",
        given: "an imported vCard",
        when: "put_vcard then entry_vcard",
        then: "the .vcf is serialized from the stored record (FN and EMAIL preserved)",
    },
    Scenario {
        name: "search-and-diff",
        given: "contacts with names, orgs, and emails",
        when: "search by substring, then diff two book states",
        then: "matches are returned and per-UID added/updated/removed changes are reported",
    },
    Scenario {
        name: "versions-and-clones",
        given: "contacts committed across two commits",
        when: "checkout an earlier commit, then clone",
        then: "checkout restores the earlier set and a clone preserves the contacts",
    },
];

/// `mail` facade, anchored to IMAP / RFC 5322 semantics over an immutable CAS body plus index and flags.
pub const MAIL_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "ingest-stores-body-and-index",
        given: "an existing mailbox",
        when: "ingest_message(raw rfc5322)",
        then: "the body is stored immutably in the CAS and the headers are parsed into the index",
    },
    Scenario {
        name: "eml-body-is-byte-exact",
        given: "an ingested message",
        when: "to_eml(uid)",
        then: "the raw RFC 5322 bytes round-trip byte-for-byte from the CAS (the .eml projection)",
    },
    Scenario {
        name: "flags-are-independent",
        given: "an ingested message",
        when: "set_flags then get_flags",
        then: "flags are a sorted, deduplicated set in a sub-tree separate from the message index",
    },
    Scenario {
        name: "search-and-diff",
        given: "messages with subjects and senders",
        when: "search by substring, then diff two mailbox states",
        then: "matches are returned and per-UID added/removed changes are reported (body is immutable)",
    },
    Scenario {
        name: "versions-and-clones",
        given: "messages committed across two commits",
        when: "checkout an earlier commit, then clone",
        then: "checkout restores the earlier set and a clone preserves the index and the CAS body",
    },
];

/// PIM lifecycle execution bridge, anchored to 0041 hooks using the 0029 trigger fire record path.
pub const PIM_TRIGGER_SCENARIOS: &[Scenario] = &[Scenario {
    name: "trigger-runs-pim-program",
    given: "trigger candidates whose programs write a calendar entry or overlap an already running binding",
    when: "the trigger execution bridge resolves, runs, skips, or queues the candidates",
    then: "direct execution commits the calendar entry, skip-if-running appends a skipped fire record, and queue returns the candidate without appending a deduping record",
}];

/// `search` facade, anchored to a versioned full-text/keyword collection with the portable linear-scan
/// query fallback.
pub const SEARCH_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "index-get-delete",
        given: "a collection with a text + keyword mapping",
        when: "index a document, get it, then delete it",
        then: "get round-trips, then None after delete",
    },
    Scenario {
        name: "linear-query-fallback",
        given: "indexed documents",
        when: "run a Match query through the portable fallback",
        then: "the matching ids come back with reduced=true",
    },
    Scenario {
        name: "no-such-field",
        given: "a collection mapping",
        when: "query an unmapped field",
        then: "NO_SUCH_FIELD",
    },
    Scenario {
        name: "versions-and-clones",
        given: "documents committed across two commits",
        when: "checkout an earlier commit, then clone",
        then: "checkout restores the earlier set and a clone preserves the documents",
    },
];

/// Inference capability seam, owned by 0043.
pub const INFERENCE_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "no-provider",
        given: "a loom with no inference provider installed",
        when: "infer is called",
        then: "UNSUPPORTED and no provider id is reported",
    },
    Scenario {
        name: "installed-provider-dispatch",
        given: "a loom with a provider installed",
        when: "infer is called with a conversation",
        then: "the provider handles the request and returns model, content, and stop reason",
    },
];

/// Embedding provider seam, owned by 0050.
pub const EMBEDDING_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "no-provider",
        given: "a loom with no embedding provider installed",
        when: "embed is called",
        then: "UNSUPPORTED and no model profile is reported",
    },
    Scenario {
        name: "installed-provider-batch",
        given: "a loom with an embedding provider installed",
        when: "embed is called with a batch",
        then: "the provider handles the batch and reports model id, dimension, and weights digest",
    },
];

pub const SQL_ERROR_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "syntax-error-code",
        given: "an invalid SQL statement",
        when: "SQL execution fails in the parser",
        then: "SQL_SYNTAX is returned",
    },
    Scenario {
        name: "missing-table-code",
        given: "a query references a table that does not exist",
        when: "SQL execution fails during table lookup",
        then: "SQL_TABLE_NOT_FOUND is returned",
    },
    Scenario {
        name: "constraint-error-code",
        given: "a table with primary-key and not-null constraints",
        when: "a mutation violates the table constraints",
        then: "SQL_CONSTRAINT_VIOLATION is returned",
    },
    Scenario {
        name: "type-mismatch-code",
        given: "a table with a typed integer column",
        when: "a mutation supplies text where an integer is required",
        then: "SQL_TYPE_MISMATCH is returned",
    },
    Scenario {
        name: "execution-fallback-code",
        given: "a SQL failure outside the narrower categories",
        when: "execution fails after parsing and planning",
        then: "SQL_EXECUTION_FAILED is returned",
    },
];

pub const SQL_HISTORY_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "read-table-at-commit",
        given: "a SQL table has commits before and after row changes",
        when: "a table is read at the older commit",
        then: "the older table rows are returned and the working tree is not checked out",
    },
    Scenario {
        name: "index-scan-at-commit",
        given: "a SQL secondary index has commits before and after row changes",
        when: "the index is scanned at the older commit",
        then: "only rows reachable from that commit are returned",
    },
    Scenario {
        name: "schema-aware-table-diff",
        given: "a SQL table schema changes between commits",
        when: "the table diff is requested",
        then: "a schema_changed record is returned instead of decoding rows with the wrong schema",
    },
];

/// Program-execution (`exec`) behavior, anchored to the spec `0015` compute facade over a WASM guest.
/// `run_exec_behavior` executes these against the `loom-compute` facade; this suite pins the
/// cross-backend contract.
pub const EXEC_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "gated-dry-run-is-deterministic-and-leaves-base-untouched",
        given: "a manifest-bound program that writes a file and a KV entry, run in gated mode against a fork of a base branch",
        when: "dry_run executes the program twice",
        then: "both runs produce the same after_root, path diff, and fuel_used, and the base branch is untouched until apply",
    },
    Scenario {
        name: "apply-adopts-the-gated-proposal",
        given: "a gated proposal committed to a scratch fork by dry_run",
        when: "apply merges the fork into the base branch",
        then: "the base adopts the program's file and KV writes and returns the merge outcome",
    },
    Scenario {
        name: "out-of-fuel-is-resource-exhausted-and-commits-nothing",
        given: "a program whose work exceeds its fuel budget",
        when: "the program runs",
        then: "the run fails with RESOURCE_EXHAUSTED, the base branch carries none of its writes, HEAD returns to the base branch, and the scratch branch is discarded",
    },
    Scenario {
        name: "denied-write-is-a-no-op",
        given: "a program whose manifest grant or principal Exec ACL does not permit a write target",
        when: "the program attempts that write and then an authorized write",
        then: "the denied write is a no-op (the guest sees the files-ABI failure signal) while the authorized write commits",
    },
    Scenario {
        name: "malformed-kv-key-traps-immediately",
        given: "a program that passes malformed key bytes to kv_put/kv_get/kv_delete",
        when: "the program runs",
        then: "the guest traps at that call and the run fails with INVALID_ARGUMENT, distinct from an authorization denial (which stays a no-op / -1)",
    },
    Scenario {
        name: "manifest-identity-is-stable-and-rejects-non-grantable-facets",
        given: "a program manifest in Loom Canonical CBOR v1",
        when: "the manifest is encoded, digested, and re-decoded",
        then: "the digest is stable across round-trips and decode rejects grants on non-grantable facets (Vcs/Program) and unknown schema versions",
    },
    Scenario {
        name: "authorization-is-the-acl-manifest-intersection",
        given: "a principal Exec-scoped ACL grant and a program manifest grant",
        when: "an operation is authorized for a facet/mode/target",
        then: "it is permitted only when both the ACL Exec scope and the manifest grant permit it; either alone denies with PERMISSION_DENIED",
    },
    Scenario {
        name: "program-logs-are-bounded-and-ordered",
        given: "a program that emits more log lines and bytes than the v1 bounds allow",
        when: "the program runs",
        then: "logs are captured in emission order up to 256 entries and 64 KiB total, over-bound entries are dropped, and the run is unaffected",
    },
    Scenario {
        name: "direct-mode-is-append-only-under-an-explicit-policy-grant",
        given: "a direct-mode request over an append-only facet (ledger append / cas put / queue append) with a direct-mode policy grant",
        when: "the request runs",
        then: "it applies immediately in one commit; a direct request over a non-append-only facet or without the policy grant is denied",
    },
    Scenario {
        name: "batched-mode-is-all-or-nothing",
        given: "a batched request of several per-manifest steps where one step fails validation or authorization",
        when: "the batch runs",
        then: "no step is applied (all-or-nothing rollback); a fully valid batch applies as a single merge commit",
    },
    Scenario {
        name: "state-access-promoted-facets-are-grant-gated",
        given: "a workspace with the promoted non-SQL, non-PIM StateAccess facets and a principal Exec ACL",
        when: "StateAccess performs one read/write round trip per facet under manifest grants",
        then: "the operations persist through the real Loom state and a facet missing from the manifest grant set is denied",
    },
];

pub const SQL_STATE_ACCESS_SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "exec-persists-query-reads",
        given: "a SQL grant scoped to one database",
        when: "StateAccess runs sql.exec followed by sql.query",
        then: "the query returns canonical result CBOR for the persisted rows",
    },
    Scenario {
        name: "query-is-read-only",
        given: "a mutating SQL statement submitted through sql.query",
        when: "StateAccess executes the query path",
        then: "PERMISSION_DENIED and no mutation is persisted",
    },
    Scenario {
        name: "manifest-mode-and-scope",
        given: "read/write SQL grants scoped to one database",
        when: "the program accesses another database or wrong mode",
        then: "the operation is denied before SQL execution",
    },
];

pub const METRICS_SCENARIOS: &[Scenario] = &[Scenario {
    name: "native-metric-canonical-identity",
    given: "a metric descriptor and observation in Loom Canonical CBOR v1",
    when: "the records are encoded, decoded, and stored through the metrics facet",
    then: "descriptor identity is stable, observations reference that identity, negative decode vectors are rejected, and records round-trip through native storage",
}];

pub const LOGS_SCENARIOS: &[Scenario] = &[Scenario {
    name: "native-log-canonical-identity",
    given: "a log record with resource attributes, scope attributes, body, severity, timing, and optional trace context in Loom Canonical CBOR v1",
    when: "the record is encoded, decoded, stored through the logs facet, and retrieved with bounded queries",
    then: "log identity is stable, negative decode vectors are rejected, records round-trip through native storage, and query bounds return deterministic partial prefixes",
}];

pub const TRACES_SCENARIOS: &[Scenario] = &[Scenario {
    name: "native-span-canonical-identity",
    given: "a trace span with resource attributes, scope attributes, events, links, status, and timing in Loom Canonical CBOR v1",
    when: "the span is encoded, decoded, stored through the traces facet, and retrieved with bounded queries",
    then: "span identity is stable, negative decode vectors are rejected, records round-trip through native storage, and query bounds return deterministic partial prefixes",
}];

pub const TICKET_COMMENT_SCENARIOS: &[Scenario] = &[Scenario {
    name: "comment-workflow",
    given: "an authenticated ticket workspace with one ticket",
    when: "add, list, update, and delete a typed ticket comment, then transition another ticket with an attached comment",
    then: "authorship, timestamps, summary metadata, history records, delivery events, redacted delete behavior, and atomic status-with-comment behavior are stable",
}];

pub const BEHAVIOR_SUITES: &[(&str, &[Scenario])] = &[
    ("workspace", WORKSPACE_SCENARIOS),
    ("sync", SYNC_SCENARIOS),
    ("files", FS_SCENARIOS),
    ("vcs", VCS_SCENARIOS),
    ("vcs-diff", DIFF_COMMITS_SCENARIOS),
    ("watch", WATCH_SCENARIOS),
    ("kv", KV_SCENARIOS),
    ("kv-ephemeral", EPHEMERAL_KV_SCENARIOS),
    ("conditional-mutation", CONDITIONAL_MUTATION_SCENARIOS),
    ("document", DOCUMENT_SCENARIOS),
    ("graph", GRAPH_SCENARIOS),
    ("vector", VECTOR_SCENARIOS),
    ("ledger", LEDGER_SCENARIOS),
    ("queue", QUEUE_SCENARIOS),
    ("queue-consumer", QUEUE_CONSUMER_SCENARIOS),
    ("delivery", DELIVERY_SCENARIOS),
    ("time-series", TIME_SERIES_SCENARIOS),
    ("metrics", METRICS_SCENARIOS),
    ("logs", LOGS_SCENARIOS),
    ("traces", TRACES_SCENARIOS),
    ("ticket-comments", TICKET_COMMENT_SCENARIOS),
    ("columnar", COLUMNAR_SCENARIOS),
    ("dataframe", DATAFRAME_SCENARIOS),
    ("search", SEARCH_SCENARIOS),
    ("calendar", CALENDAR_SCENARIOS),
    ("contacts", CONTACTS_SCENARIOS),
    ("mail", MAIL_SCENARIOS),
    ("pim-trigger", PIM_TRIGGER_SCENARIOS),
    ("inference", INFERENCE_SCENARIOS),
    ("providers.embedding", EMBEDDING_SCENARIOS),
    ("sql-errors", SQL_ERROR_SCENARIOS),
    ("sql-history", SQL_HISTORY_SCENARIOS),
    ("cas", CAS_SCENARIOS),
    ("cas-facade", CAS_FACADE_SCENARIOS),
    ("lock", LOCK_SCENARIOS),
    ("identity", IDENTITY_SCENARIOS),
    ("acl", ACL_SCENARIOS),
    ("merge-conflict", MERGE_CONFLICT_SCENARIOS),
    ("staging", STAGING_SCENARIOS),
    ("file-ops", FILE_OPS_SCENARIOS),
    ("file-handle", FILE_HANDLE_SCENARIOS),
    ("symlink", SYMLINK_SCENARIOS),
    ("tags", TAGS_SCENARIOS),
    ("restore", RESTORE_SCENARIOS),
    ("replay", REPLAY_SCENARIOS),
    ("squash", SQUASH_SCENARIOS),
    ("protected-ref", PROTECTED_REF_SCENARIOS),
    ("exec", EXEC_SCENARIOS),
    ("sql-state-access", SQL_STATE_ACCESS_SCENARIOS),
];

/// Capability names whose behavioral suite has an executable runner today. Every other
/// [`BEHAVIOR_SUITES`] entry is declarative inventory until its runner exists, so it is never reported
/// as passed.
pub const EXECUTABLE_BEHAVIOR_SUITES: &[&str] = &[
    "cas",
    "workspace",
    "sync",
    "queue",
    "queue-consumer",
    "delivery",
    "vcs-diff",
    "watch",
    "conditional-mutation",
    "cas-facade",
    "lock",
    "identity",
    "acl",
    "kv",
    "kv-ephemeral",
    "document",
    "time-series",
    "metrics",
    "logs",
    "traces",
    "ticket-comments",
    "ledger",
    "graph",
    "vector",
    "columnar",
    "dataframe",
    "search",
    "calendar",
    "contacts",
    "mail",
    "pim-trigger",
    "inference",
    "providers.embedding",
    "sql-errors",
    "sql-history",
    "merge-conflict",
    "staging",
    "file-ops",
    "file-handle",
    "symlink",
    "tags",
    "restore",
    "replay",
    "squash",
    "protected-ref",
    "exec",
    "sql-state-access",
];
