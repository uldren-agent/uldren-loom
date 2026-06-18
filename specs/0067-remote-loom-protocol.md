# 0067 - Remote Loom Protocol

**Status:** Target design. **Version:** 0.1.0-target.
**Capability:** `remote-loom`.

**Depends on:** 0003 (core interface), 0007 (language bindings), 0008 (wire protocols), 0009
(security and capabilities), 0010 (conformance and versioning), 0026 (principals and identity),
0027 (access control), 0028 (fine-grained access control), 0030 (observability), 0035 (durable
delivery), 0036 (locking and coordination), 0066 (network access).
**Relates to:** 0004a (hosted provider facade), 0006a (live sync and remotes), 0031 (end-to-end
encrypted sync), 0043 (MCP serving surface), 0052 (certificate bundles).

This spec defines the Loom-native remote source protocol. A remote Loom endpoint lets a client use a
Loom running on another machine as the authoritative source behind the same logical client API used
for a local `.loom` file.

This is not a sync protocol, a blind replica, a remote-tracking-ref workflow, or a cloud control plane.
It is a single authoritative Loom made available through a remote `LoomClient` implementation. Remote
unavailability fails the current operation immediately. Remote clients never queue offline writes for
later replay.

## 1. Current Source-Backed Boundary

The current source has pieces that this spec builds on, but it does not yet implement this protocol.

- `idl/loom.idl:389` starts the current language-neutral interface. That IDL includes store,
  key-source, workspace, exec, session, identity, ACL, protected-ref, daemon, lock, VCS, watch,
  filesystem, file-handle, CAS, KV, graph, vector, columnar, dataframe, search, management KV,
  document, calendar, contacts, mail, lanes, time-series, ledger, queue, SQL, diagnostics, task, and
  result-view interfaces across `idl/loom.idl:389`, `idl/loom.idl:413`, `idl/loom.idl:422`,
  `idl/loom.idl:434`, `idl/loom.idl:447`, `idl/loom.idl:452`, `idl/loom.idl:467`,
  `idl/loom.idl:479`, `idl/loom.idl:499`, `idl/loom.idl:511`, `idl/loom.idl:614`,
  `idl/loom.idl:693`, `idl/loom.idl:713`, `idl/loom.idl:766`, `idl/loom.idl:782`,
  `idl/loom.idl:799`, `idl/loom.idl:813`, `idl/loom.idl:833`, `idl/loom.idl:858`,
  `idl/loom.idl:874`, `idl/loom.idl:889`, `idl/loom.idl:950`, `idl/loom.idl:960`,
  `idl/loom.idl:982`, `idl/loom.idl:1013`, `idl/loom.idl:1038`, `idl/loom.idl:1064`,
  `idl/loom.idl:1080`, `idl/loom.idl:1094`, `idl/loom.idl:1109`, `idl/loom.idl:1128`,
  `idl/loom.idl:1195`, `idl/loom.idl:1210`, `idl/loom.idl:1237`, and `idl/loom.idl:1258`.
- `crates/loom-core/src/provider.rs:24` defines a low-level object-store trait. It is not broad
  enough to be the remote client contract because it does not cover sessions, refs, ACL, SQL, locks,
  watches, or persistence semantics.
- `crates/loom-cli/src/helpers.rs:220` and `crates/loom-cli/src/helpers.rs:317` centralize local CLI
  opens through `cli_open_loom` and `cli_open_loom_read`, making them migration points for locator
  aware clients.
- `crates/loom-cli/src/cli.rs:140`, `crates/loom-cli/src/daemon_cmd.rs:55`, and
  `crates/loom-cli/src/daemon_cmd.rs:72` show that `loom mcp` currently accepts a local store path
  and builds local `StoreAccess`.
- `crates/loom-mcp/src/lib.rs:107` and `crates/loom-mcp/src/lib.rs:408` show that MCP already has a
  facade boundary that can be moved behind a local or remote client.
- `crates/loom-hosted/proto/loom_hosted_v1.proto:5`, `crates/loom-hosted/src/grpc.rs:44`,
  `crates/loom-hosted/src/grpc.rs:103`, `crates/loom-hosted/src/grpc.rs:127`, and
  `crates/loom-hosted/src/grpc.rs:134` show partial hosted protocol work. That work is reusable
  evidence, not full remote-source coverage.
- `crates/loom-cli/src/serve_cmd.rs:38` shows the current served surface registry. There is no
  top-level `remote` served surface in source yet.
- `crates/loom-store/src/lib.rs:163` shows served listener records already carry surface,
  transport, TLS, auth, limits, route, exposure, and network policy metadata.
- `crates/loom-store/src/lib.rs:480` and `crates/loom-store/src/lib.rs:493` show local file-store
  writer lock and read-only open behavior.
- `crates/loom-store/src/daemon.rs:89`, `crates/loom-store/src/daemon.rs:249`, and
  `crates/loom-core/src/lock.rs:53` show local daemon session, pin, and lock coordination machinery
  that the remote runtime must match or exceed.
- `crates/loom-core/src/triggers.rs:21` shows trigger storage and keeper behavior exists in core, and
  `crates/loom-compute/src/trigger_exec.rs:90` shows the fire dispatch. Trigger management is now
  declared as `interface Triggers` at `idl/loom.idl:1280`; the C ABI export set (`include/loom.h`),
  binding projections, and conformance vectors for that interface are the remaining promotion
  follow-through tracked in Queue 11.

Everything after this section is the target protocol contract unless it explicitly cites source
behavior.

## 2. Design Decisions

- The remote protocol is Loom-native. It MAY be carried by gRPC as an implementation detail, but gRPC
  is not the contract.
- The logical method surface is the complete `idl/loom.idl` surface. Every IDL method MUST have local
  and remote `LoomClient` coverage before this capability is complete.
- A remote endpoint serves exactly one Loom. The URL selects an endpoint and service root, not a store
  id inside a multi-store service.
- Remote execution is authoritative. The server owns the writer authority, session table, lock
  coordinator, watch streams, task table, trigger keeper, and handle registry for its single Loom.
- Remote unavailable, authentication failure, TLS failure, unsupported protocol version, and rejected
  network policy fail immediately. The client MUST NOT persist a replay queue.
- `loom serve remote` is the primary human-facing command to expose this protocol.
- `loom mcp <STORE>` MUST support remote contexts and URLs through the same locator and `LoomClient`
  machinery used by other store-taking commands. Status: IMPLEMENTED + live-verified (task 370 Done,
  2026-07-13) - a remote locator launches a remote-backed MCP host and every IDL-backed tool family
  forwards to the remote Loom: KV, CAS, Queue, Ledger, TimeSeries (incl. timestamped `latest`, task 398),
  Search, Columnar, PIM (Calendar, Contacts, Mail), filesystem, vector, document reads + `document_query`
  (host-assembled over remote primitives, task 397), document writes incl. the reference-index overlay
  (task 395), the SQL-read group + `sql_exec` + full-result `sql_query` (task 399), Dataframe, Watch, the
  full VCS family incl. timestamped commits/`sql_commit` (task 396a) and the richer-return replay/merge
  writes (task 396b), the graph reads + node writes + edge writes incl. the reference-index overlay
  (task 395), and Lane management (create/get/list/status and membership updates). The remaining
  host/composite tools (no single IDL projection) are intentionally local-only
  and reject over remote with a precise error by design; handle/stream is empty (every IDL-backed tool is
  Unary). See section 10 and `0067a-mcp-remote-inventory.md`.
- `--stateless` applies only to local Loom MCP hosting. Remote Loom sessions are managed by the
  remote endpoint and MUST reject remote `--stateless` mode.
- Project context config is committable and MUST NOT contain secrets.
- Android, iOS, and browser bindings do not read context TOML by default. They accept explicit URLs or
  host-provided context resolvers unless a platform package explicitly opts into context config.

## 3. Locator Contract

Every CLI, binding, or MCP entry point that accepts a Loom source accepts a `LoomLocator`. The concrete
CLI placeholder remains `<STORE>` for compatibility.

Accepted forms:

```text
context                      selected first-class CLI context
https://remote.host          remote URL, default discovery path
https://remote.host:9443     remote URL with explicit port
https://remote.host/app/loom remote URL with service-root path
file://app.loom              local file URL
app.loom                     local path
```

Resolution order is:

1. Selected context: the literal `context` resolves the explicit `--context <name>` value, or the
   project-local current context selected by `loom context use`.
2. URL: `http://` or `https://` for remote Loom, `file://` for local Loom.
3. Path-like or bare local input.

Context names never shadow local filenames. A bare non-path string such as `prod` remains a local path
candidate. Users who want a configured target pass `context` with `--context prod`, or select it with
`loom context use prod`.

Remote URL locators identify the service root candidate before discovery. URL user-info is forbidden.
Credentials MUST NOT appear in locator strings.

## 4. Context Configuration

Context configuration is TOML because local and remote target selection has to happen before a Loom is
opened. The resolver loads layers in this order, highest precedence first:

1. Each explicit `--config <path>` file, in command-line order where later files override earlier
   files.
2. Project config at `<project>/.loom/contexts.toml`.
3. User config at `~/.loom/contexts.toml`.
4. System config at `/etc/loom/contexts.toml`.

The default project path is the process working directory after canonicalization. `--project <path>`
overrides it and is valid before or after the subcommand.

Minimal schema:

```toml
[cli]
current_context = "prod"

[contexts.prod]
target = "https://loom.example.com/prod"
default_workspace = "main"
auth = "interactive"
tls = "system"

[contexts.staging]
target = "https://staging.example.com:9443/loom"
auth = "token:loom-staging"
tls = "bundle:corp-root"
```

Allowed context fields:

```text
target              required remote URL or file URL
auth                optional auth selector, not a secret
tls                 optional TLS trust selector, not key material
discovery           optional default | service-root | well-known | disabled
discovery_path      optional absolute path for custom discovery
connect_timeout_ms  optional positive integer
request_timeout_ms  optional positive integer
description         optional operator text
```

Secret-bearing fields are invalid. Rejected field names include `token`, `password`, `passphrase`,
`secret`, `private_key`, `client_key`, `bearer`, and `api_key`. Context files may refer to credential
providers by name, keychain item, file descriptor, prompt, mTLS certificate identity, or external
host provider, but they MUST NOT store credential material.

Examples:

```text
loom context add prod https://loom.example.com/prod --default-workspace main
loom --context prod kv list context app
loom kv list file://app.loom
loom --context prod mcp context --project /path/to/project
loom --project /path/to/project --context prod mcp context
```

For an MCP launcher that is started outside the repository, the launcher passes the project path:

```text
loom --context prod mcp context --project /path/to/project
```

The command finds `/path/to/project/.loom/contexts.toml`, resolves `prod`, and starts the local MCP
adapter against the remote Loom authority. The MCP launcher does not pass the TOML file itself as the
store locator.

## 5. Endpoint Discovery

The client discovers a `remote-loom` endpoint before opening a protocol session.

Discovery inputs:

- Locator URL.
- Optional context discovery mode.
- Optional context `discovery_path`.
- Client-supported protocol versions and transports.

Discovery modes:

```text
default       Try service-root discovery first when the locator URL has a path other than "/", then
              well-known host discovery. For host-only URLs, try well-known host discovery first.
service-root  Fetch discovery from the locator service root only.
well-known    Fetch discovery from the host-level well-known path only.
disabled      Treat the locator URL as the exact remote protocol endpoint.
```

For `https://remote.host`, default discovery fetches:

```text
https://remote.host/.well-known/loom
```

For `https://remote.host:9443`, default discovery fetches:

```text
https://remote.host:9443/.well-known/loom
```

For `https://remote.host/apps/loom`, default discovery treats `/apps/loom` as a service root first:

```text
https://remote.host/apps/loom/.well-known/loom
https://remote.host/.well-known/loom
```

The first successful discovery document wins only if it advertises capability `remote-loom` and a
compatible protocol version. Redirects are allowed only within the same scheme and host unless the
context explicitly allows cross-host discovery.

Discovery document shape:

```cbor-diag
{
  "protocol": "loom.remote.v1",
  "capabilities": ["remote-loom"],
  "service_root": "https://remote.host/apps/loom",
  "endpoints": {
    "cbor-h2": "https://remote.host/apps/loom/v1/call"
  },
  "min_version": 1,
  "max_version": 1,
  "auth": ["interactive", "token", "mtls", "principal", "external"],
  "tls": ["system", "bundle"],
  "streams": true,
  "compression": ["zstd", "none"],
  "single_loom": true
}
```

The discovery document is public metadata and MUST NOT reveal secret material. It MAY require network
access policy admission, but it SHOULD NOT require Loom protocol authentication.

## 6. Carrier And Envelope

The v1 carrier is HTTP/2 over TLS with length-framed Loom Canonical CBOR payloads. HTTP/2 is selected
for multiplexing, bidirectional streams, mature proxy support, and enterprise deployment support. A
gRPC adapter MAY carry the same logical envelopes, but generated gRPC messages are not the normative
method contract.

Request envelope:

```cbor-diag
{
  "protocol": "loom.remote.v1",
  "request_id": bytes,
  "session_id": null / bytes,
  "interface": "Kv",
  "method": "list",
  "args": [ ... ],
  "deadline_ms": uint,
  "idempotency_key": null / bytes,
  "principal_hint": null / text,
  "compression": "none" / "zstd",
  "stream": false / true
}
```

Response envelope:

```cbor-diag
{
  "protocol": "loom.remote.v1",
  "request_id": bytes,
  "session_id": null / bytes,
  "ok": true,
  "value": ...
}
```

Error envelope:

```cbor-diag
{
  "protocol": "loom.remote.v1",
  "request_id": bytes,
  "session_id": null / bytes,
  "ok": false,
  "error": {
    "code": "PERMISSION_DENIED",
    "message": "string",
    "retry": "never" / "after" / "same_idempotency_key",
    "retry_after_ms": null / uint,
    "details": null / bytes
  }
}
```

The `code` field preserves the stable Loom error `Code` enum verbatim. Protocol implementations MUST
add codes rather than renaming, collapsing, or repurposing existing codes.

The `args` value is an IDL tuple encoded in Loom Canonical CBOR. The `value` field is the IDL return
type encoded in Loom Canonical CBOR. Opaque handles are represented by remote handle ids with declared
kind, generation, and session binding.

Idempotency keys are required for mutating methods that are not naturally compare-and-swap or
otherwise idempotent (the `key`-classified methods of section 12). Keys are scoped by `(endpoint
identity, principal, method, session)` and bound to a canonical request fingerprint. A replay with the
same fingerprint returns the same terminal result. A replay with a different fingerprint fails with
`Conflict` and `RetryAdvice::Never` (a reused key with a changed request is a client error, not a
retryable condition); `RetryAdvice::SameIdempotencyKey` is reserved for ambiguous retryable failures
where the caller should re-send under the same key.

Enforcement is two-sided and implemented:

- Server. The runtime keeps a per-session dedup registry keyed by `(session, interface, method,
  idempotency_key)` holding the request fingerprint and the terminal response. The check, execution, and
  remember happen under the single write authority, so an in-flight duplicate is deterministic:
  an exact-fingerprint replay returns the stored terminal result without re-applying the effect, and a
  same-key/different-fingerprint request returns `Conflict`. Entries are per-session capped (oldest
  evicted) and dropped on session close or expiry.
- Client. Generated clients auto-attach a fresh key to every `key`-classified method, so a transport
  retry of one logical call cannot double-apply it. The lower-level `call` surface still accepts a
  caller-supplied `idempotency_key` in `CallOptions`, letting an application drive durable, at-least-once
  retries under a stable key of its own choosing.

## 7. Streams And Backpressure

The protocol supports unary calls and streams. Streams carry frames:

```text
open
item
credit
cancel
complete
error
trailer
```

Stream ids are scoped to a session. Every stream starts with explicit client credit. The server MUST
NOT send more `item` frames than the remaining credit. The client grants more credit with `credit`
frames. Either side MAY cancel a stream; cancellation closes server resources and releases handles
owned only by that stream.

Required stream mappings:

- Watch streams carry ordered watch events and resume cursors.
- SQL query streams carry row batches and result metadata.
- File and CAS streams carry bounded byte chunks with digest or offset metadata where applicable.
- Task streams carry task state changes, progress, terminal result, and cancellation acknowledgement.
- Result views and iterators carry item batches with explicit close semantics.

Mid-stream errors use the same stable error object as unary errors and are followed by terminal stream
closure. Trailers carry counts, cursors, final digest, or task ids where the owning IDL method defines
that metadata.

## 8. Authentication And TLS

Remote protocol authentication modes:

```text
interactive  The client prompts or delegates to a local credential provider.
token        The client retrieves a named token from keychain, file descriptor, or host provider.
mtls         The client presents a configured certificate identity.
principal    The client presents an already established local principal assertion.
external     The client delegates proof to an external verifier or embedding host.
```

Context TOML stores selectors only. It does not store token bytes, passwords, passphrases, private keys,
or API keys.

TLS trust modes:

```text
system        Use platform trust.
bundle:<name> Use a named certificate bundle or host-provided trust anchor.
insecure-dev  Disable certificate verification only when the URL is loopback or an explicit unsafe
              development flag is present.
```

Plain HTTP is rejected by default. It is allowed only for loopback development or an explicit policy
that names the endpoint and reason. Network access policy from 0066 is evaluated before protocol
authentication where the server owns a TCP listener.

The server attaches an authenticated principal to every operation. Authorization is enforced by the
same engine PEP path as local operations. The remote adapter MUST NOT become a second policy engine.

## 9. Concurrency, Sessions, And Locks

A remote server runtime owns exactly one Loom and one writer authority for that Loom. Multiple client
connections share that authority. The runtime provides protections equal to or stricter than local
daemon and file-store protections.

Required runtime state:

- Connection registry.
- Session registry with leases, renewal, expiry, and cleanup.
- Remote handle registry with kind, generation, owner session, last-use time, and close state.
- Task table with cancellation and terminal-result retention policy.
- Stream registry with flow-control state and disconnect cleanup.
- Lock coordinator using the same exclusive, shared, semaphore, lease, reentrancy, and fencing
  concepts as local lock coordination.
- Watch registry with ordered cursors and explicit close semantics.
- Trigger keeper for enabled trigger behavior owned by the served Loom.
- Single write serializer around persistent mutation.

Read-only methods MAY run concurrently when the engine can prove they are safe against the current
write path. Mutating methods MUST pass through the single writer authority. Locks and fencing tokens
are evaluated by the remote authority and are never trusted solely from client state.

When a connection drops, the server closes streams for that connection immediately. Session-bound
handles remain alive until their lease expires unless the client requested connection-bound handles.
Locks are released according to their lease and owner semantics, not merely because a TCP connection
closed.

Remote fail-fast requirements:

- DNS failure, connect timeout, TLS trust failure, network access denial, auth denial, unsupported
  protocol version, and discovery failure return an error to the caller immediately.
- Clients MUST NOT save commands, method envelopes, writes, lock acquisitions, or stream opens for
  later replay.
- The correct offline mechanism is a separate sync or bundle workflow, not remote Loom.

## 10. MCP And Bindings

Status: implemented and live-verified owner-side (task 370 Done, 2026-07-13). `loom mcp <STORE>` accepts a
local or a remote locator. A local locator serves the full MCP tool surface as before. A remote locator
(an `https://` URL or a remote context) launches a remote-backed host: the MCP host connects a
`RemoteLoomClient` over the same carrier/session path the CLI facade uses
(`crates/loom-cli/src/remote.rs` `McpRemoteBackend`) and forwards every IDL-backed tool family to the
remote Loom - KV, CAS, Queue, Ledger, TimeSeries (incl. timestamped `latest`, task 398), Search
(full-text), Columnar, PIM (Calendar, Contacts, Mail), FileSystem, Vector, document reads +
`document_query` (host-assembled over remote primitives, task 397) + document writes with the
reference-index overlay (task 395), the SQL-read group + `sql_exec` + full-result `sql_query` (task 399),
the Dataframe group, the Watch group (the batch wire form carries each event's `parent`), and the full VCS
family: reads, timestamped commits/`sql_commit` (task 396a), the richer-return replay/merge writes
`merge`/`cherry_pick`/`revert`/`rebase` (task 396b), and the graph reads + node writes + edge writes with
the reference-index overlay (task 395).

Tool dispatch is gated by each tool's `RemoteCapability` (`crates/loom-mcp/src/tools.rs`), classified at
the METHOD level. Every IDL-backed tool is Unary (`HANDLE_STREAM_METHODS` is empty). The only tools that
reject over a remote locator are the intentionally local-only host/composite tools with no single IDL
projection - they return a precise local-only error by design. The document/graph reference-index writes
run their `substrate_refs` overlay server-side via the `*_indexed` methods relocated to `loom-reference`
(task 395), so a remote write updates both the primary facet and the reference index. All of the above is
live-verified via the omnibus `mcp_kv_round_trip_through_remote_backend` (owner-run GREEN, 2026-07-13).

`loom mcp <STORE>` uses the same locator resolution as every other store-taking command. If `<STORE>`
resolves to a local path, the MCP host uses local per-request or persistent local access. If `<STORE>`
resolves to a remote URL or context, the MCP host acts as an adapter to the remote Loom authority through
`RemoteLoomClient`.

Local examples:

```text
loom mcp ./app.loom
loom --context prod mcp context --project /path/to/project   # when `prod` resolves to a local path
```

Remote examples (supported today):

```text
loom --context prod mcp context --project /path/to/project
loom --project /path/to/project --context prod mcp context
loom mcp https://loom.example.com/prod
```

`--stateless` applies only to a local Loom MCP host: a remote MCP host rejects `--stateless` with a
local-only diagnostic, because remote statefulness is controlled by the remote session, stream, task,
lock, and handle lifecycle.

Binding packages use one of three shapes:

- Local-only packages include the engine and local file access.
- Remote-only packages include the protocol client and no local engine.
- Combined packages include both local and remote implementations behind the same `LoomClient`
  abstraction.

Node, Python, JVM, desktop, and CLI packages MAY enable TOML context resolution. Browser, iOS, and
Android packages disable context TOML by default and accept explicit URLs or host-provided context
resolvers.

### 10.1 Binding package matrix

This is the normative per-binding package/API policy matrix (queue task 400). It fixes, for every
binding target, the default package shape, how local and remote implementations are packaged, how remote
mode is exposed, whether TOML contexts are available, where stores and config live, and the security/TLS
posture. It builds on the three package shapes and the context policy above and on the locator model in
sections 3-4; the binding API surface itself is owned by spec 0007. Current state: the native embeddable
bindings (`bindings/node`, `bindings/python`, `bindings/wasm`) ship local-only today (they depend on
`loom-core` + `loom-store` with no `loom-remote-client`); this matrix is the target policy for adding the
remote client, not a claim that remote is already wired in each binding.

Package shape and packaging mechanism:

```text
Binding        | Default shape | Local vs remote packaging          | Remote exposure
---------------|---------------|------------------------------------|------------------------------------
CLI            | Combined      | one artifact, runtime mode         | locator (path | file:// | https:// | context)
loom serve     | Server/host   | one artifact (authority, not a client) | it IS the remote endpoint; serves a local store
desktop        | Combined      | one artifact, runtime mode         | locator (as CLI)
Node           | Combined      | one package; remote on by default  | LoomClient(locator) or LoomClient.remote(url, auth)
Python         | Combined      | one wheel; remote on by default    | LoomClient(locator) or LoomClient.remote(url, auth)
JVM            | Combined      | one artifact; remote on by default | LoomClient(locator) or LoomClient.remote(url, auth)
C++            | Combined      | one lib; remote behind a build feature | LoomClient(locator) or explicit remote ctor
Android        | Combined*     | per-ABI native artifact; remote always on, local optional | explicit remote URL + host auth (no context TOML)
iOS            | Combined*     | one framework; remote always on, local optional | explicit remote URL + host auth (no context TOML)
browser (WASM) | Combined*     | one module; remote via fetch/WebSocket, local via OPFS | explicit remote URL + host auth (no context TOML)
React Native   | Combined*     | bridges to the iOS/Android native artifact | explicit remote URL + host auth (no context TOML)
```

`Combined*` (mobile/browser) means both implementations sit behind the same `LoomClient`, but the local
engine is optional per app and context TOML is off; size-sensitive apps MAY ship a remote-only build via the
remote-only package shape. `remote behind a build feature` (C++) reflects that an embedder chooses whether
to link the protocol client. All Combined bindings select local vs remote at runtime from the locator (a
`file://`/path resolves local; an `https://` URL or a remote context resolves remote) - never a compile-time
fork between local and remote behavior; the only compile-time choice is whether the remote client is linked
at all (relevant to footprint on C++/mobile/browser).

Contexts, storage/config location, and security per platform:

```text
Binding        | Contexts (TOML)      | Store + config location                 | Auth / TLS
---------------|---------------------|-----------------------------------------|-------------------------------
CLI            | Enabled             | fs paths; project/user/system TOML (sec 4) | system trust store; context TOML holds selectors only (sec 4/8)
loom serve     | Enabled (its store) | fs path of the served store             | server TLS cert/key; session auth (sec 6/8)
desktop        | Enabled             | OS config dirs; project/user/system TOML | system trust store; OS credential store for tokens
Node           | Opt-in (off default)| cwd + user config dir when opted in; else explicit | system trust store; tokens via host/env, never in context TOML
Python         | Opt-in (off default)| as Node                                 | as Node
JVM            | Opt-in (off default)| as Node                                 | as Node
C++            | Opt-in (off default)| embedder-provided paths                 | embedder-provided trust + tokens
Android        | Disabled by default | app-private internal storage (filesDir); no shared TOML | network security config; tokens in Keystore/host-injected
iOS            | Disabled by default | app sandbox (Application Support); no shared TOML | ATS; tokens in Keychain/host-injected
browser (WASM) | Disabled by default | OPFS for local; no fs TOML; config via JS host | browser TLS; host-injected tokens; CORS for cross-origin remote
React Native   | Disabled by default | platform store (iOS/Android rules above) | platform TLS; tokens in platform secure storage
```

Rules that hold across the matrix: (1) context TOML stores selectors only and never token bytes,
passwords, passphrases, or keys (section 4) - so disabling context TOML on mobile/browser removes a config
surface, not a secret surface; (2) mobile and browser bindings accept an explicit URL or a host-provided
context resolver in place of TOML (section 2); (3) remote statefulness (sessions, locks, watches, tasks) is
owned by the remote server, so every Combined binding that opens a remote locator is inherently stateful
and MUST NOT be run in a stateless per-request mode against a remote endpoint (mirrors the MCP
`--stateless` rejection, task 380); (4) local vs remote is a locator-time decision behind one
`LoomClient`, so a binding's public API is identical for local and remote sources - the "source file"
model holds on every platform.

Recommended defaults the owner may override: mobile (Android/iOS/RN) and browser default to Combined with
the local engine present but context TOML off; an owner who wants the smallest network-only footprint can
select the remote-only package shape for those targets. C++ defaults to linking the remote client behind a
build feature because embedders vary in footprint sensitivity. These are defaults, not contract forks: the
`LoomClient` behavior and the locator semantics are identical regardless of which shape a target ships.

Task 410 status (constructor URL/local split, stub-only - no real remote transport yet). The
explicit-URL(remote)/local-path constructor surface is implemented behind a `remote` cargo feature (off by
default) at each Rust open seam: a local path and a `file://` URL open locally; an `http(s)://` locator
routes to the remote branch, which returns a stable error - `remote Loom locators require the remote
feature in this binding` with the feature off, or a "not yet wired (constructor surface only)" error with
it on. No context TOML is read on any target by this classification (a bare non-path string stays a local
path). Coverage: `bindings/node`, `bindings/python`, and `bindings/wasm` got source + unit tests directly
(`normalize_locator` / `reject_remote_locator`, routed through their open/`acquire_handle` seams); the
host-language bindings (JVM, Android, iOS, C++, React Native) inherit the split centrally because they all
wrap the C ABI (`crates/loom-ffi`), whose shared `open_loom_unlocked`/`open_loom_read_unlocked` seam got
the same classifier + `remote` feature + unit tests. Still future (tracked, not built here): the real
remote transport per binding, and a typed persistent client-object / host-language remote constructor
(the ergonomic layer over the locator string).

## 11. Conformance

Remote Loom is complete only when conformance proves:

- Every `idl/loom.idl` method is inventoried and mapped to local and remote `LoomClient` behavior.
- The same behavior suite passes against local and remote clients.
- Locator tests cover `prod`, `context:prod`, URL, port URL, subpath URL, `file://`, local path,
  project/user/system precedence, explicit config, and context shadowing.
- Discovery tests cover host-only well-known, port well-known, service-root subpath, custom discovery
  path, disabled discovery, redirect policy, invalid capability, and incompatible version.
- Envelope tests cover scalar, bytes, optional, list, struct, enum, handle, stream, task, and result
  view values.
- Error tests preserve every stable `Code`.
- Failure tests cover remote unavailable, no offline queueing, duplicate idempotency keys, expired
  sessions, stale lock fences, dropped streams, cancelled tasks, permission denial, encrypted server
  startup, TLS trust failure, and unsupported HTTP policy.
- Concurrency tests cover multi-connection writes, safe parallel reads, locks, sessions, pins, stream
  cleanup, handle cleanup, task cleanup, trigger behavior, and shutdown drain.
- MCP tests cover local, remote URL, remote context, `--project`, and remote `--stateless` rejection.
- Binding evidence proves context policy and local-vs-remote package split for each supported platform, per
  the binding package matrix in section 10.1.

### 11.1 Local-vs-remote client parity harness (task 420)

The shared parity runner lives in `loom-protocol-conformance::client_parity` and is driver-agnostic: it is
one deterministic operation sequence, `run_client_parity_suite(driver)`, over a small `ParityDriver`
adapter trait that both a local and a remote `LoomClient` satisfy. The runner records each observable
result into a `ParityReport` (a `Vec<(label, bytes)>` with a fixed byte encoding for text, bytes, `u64`,
optional bytes, and optional time-series points), so parity is proven by comparing observable *outputs*
byte-for-byte, not merely by both sides succeeding. Setup is deterministic (fixed workspace/collection
names, fixed commit timestamps) so the content-addressed digests are stable across runs and stores.

Two drivers are wired:

- `LocalClientDriver` (in `loom-protocol-conformance`) drives `loom_client::LocalLoomClient` in-process. Its
  suite runs in the sandbox: `cargo test -p uldren-loom-protocol-conformance client_parity` is GREEN
  (`local_driver_runs_the_full_parity_suite` asserts the exact label list, concrete per-op values, and
  determinism across two fresh stores).
- `RemoteClientDriver` (in `crates/loom-cli/src/remote.rs`, `live_tests`) drives the generated `LoomClient`
  surface on a connected `RemoteLoomClient` over a live `loom serve remote` endpoint, blocking on each async
  call exactly as the CLI facade's remote arm does. The live test `client_parity_local_matches_remote`
  stands up the endpoint, runs the *same* `run_client_parity_suite` against both a local driver and the
  remote driver on fresh stores, and asserts the two `ParityReport`s are equal. It is owner-run (the loom-cli
  test binary does not link in the constrained sandbox - a verification-environment limit, not a code
  blocker): `cargo test -p uldren-loom-cli --features "serve remote-client mcp" client_parity_local_matches_remote -- --nocapture`.

Coverage in 420 is a broad representative slice across transport-relevant families: `Store::version`; KV
put/get (present and absent); CAS put (digest) + get; Queue append (sequence) + get; Document read (put +
get); TimeSeries `latest` (decoded to `(ts, value)` via the same `loom_core::timeseries::latest_point_from_cbor`
the remote-MCP path uses, so local and remote observables match); and a timestamped `VersionControl::commit`
(digest-sensitive, at a fixed timestamp). Deferred to task 430, with reasons: `document_query` (a host-assembled
composite, not a single generated call - proven separately at the MCP level in task 397, and expensive to
fixture here); `sql_exec`/`sql_query` (session/stream-shaped setup heavier than a unary slice warrants for
420); and `Watch` (subscribe/poll fixturing). None of these families is silently omitted - each is listed here
and carried on the 430 row. The in-process server-backed loopback transport (a third driver kind that would let
the remote path run without a live socket) was considered and deferred as a possible future enhancement; 420
uses the live `loom serve remote` socket driver instead.

### 11.2 Remote concurrency and coordination (task 440)

The remote runtime's coordination is exercised by a focused in-process suite over `RemoteRuntime` in
`crates/loom-hosted-core/src/remote.rs` (`#[cfg(test)] mod tests`), which drives real sessions, engine
leases, streams, and the drain/shutdown path without a TLS carrier or the loom-cli binary - so the whole
suite runs in-sandbox. Coverage maps to the task-440 requirements as follows:

- **Multi-connection writes + session isolation** - `two_sessions_share_one_store_and_reject_cross_session_handles`:
  two sessions on one bound store both write (serialized through the single writer authority), and a handle
  minted by one session is permission-denied to the other.
- **Parallel reads over the shared store** - `concurrent_sessions_see_each_others_committed_writes`: sessions
  observe each other's committed writes; protocol sessions take no exclusive writer lock, so readers are not
  blocked (task #46 decoupled sessions from the writer lock).
- **Session leases** - `session_renewal_and_expiry` (renewal extends the lease; an expired session is
  rejected).
- **Locks and stale fences** - the lock register and monotonic fence semantics (issue/apply/reject a stale
  fence) are owned by `loom-core` (`crates/loom-core/src/lock.rs`, e.g. `exclusive_lock_excludes_other_owners_and_is_reentrant`)
  and the `Locks` IDL methods dispatch to that single shared register over remote (generated dispatch, R2.2).
- **Engine pins** - `generated_dispatch_tracks_file_handle_lifecycle`: a `FileHandle` open pins the engine
  writer, later calls borrow the pinned writer, and teardown frees it (`release_engine`/`kept_pin`).
- **Disconnect cleanup** - `session_close_frees_registered_handles` and
  `streaming_backpressure_completion_and_disconnect_cleanup`: closing a session (or a dropped stream) frees
  its registered handles and streams.
- **Shutdown drain** - `starts_opens_a_session_and_shuts_down` (draining rejects new sessions) and
  `drain_rejects_new_sessions_and_streams_but_serves_existing` (draining also rejects new streams, while an
  already-open session keeps serving unary calls so in-flight work runs down).

The end-to-end over-TLS composition (multiple `RemoteLoomClient` connections against one `loom serve remote`)
is the multi-session model above layered on the separately-tested HTTP/2-over-TLS carrier (tasks 326/327,
R4c streaming); a dedicated multi-connection over-TLS live test is a recorded follow-up (owner-run, like the
420/430 remote drivers).

### 11.3 Remote failure modes (task 450)

The remote failure surface is exercised by focused tests across the client, the HTTP semantic core, and the
runtime - all in-sandbox. Coverage maps to the task-450 requirements as follows:

- **Unavailable endpoint fail-fast + no offline queueing** - `unavailable_endpoint_fails_fast`
  (`crates/loom-remote-client/src/connection.rs`): a connect to a dead endpoint returns an error promptly.
  The client never queues offline work - call failures carry `RetryAdvice::Never`
  (`crates/loom-remote-client/src/client.rs`); retry is the caller's decision, and there is no background
  queue.
- **Incompatible protocol version** - `incompatible_protocol_is_rejected` (same module).
- **Expired sessions** - `session_renewal_and_expiry` (`crates/loom-hosted-core/src/remote.rs`): a renewed
  lease is honored; an expired session is rejected.
- **Permission denial** - `auth_success_and_failure` (bad credentials rejected) and
  `two_sessions_share_one_store_and_reject_cross_session_handles` (a handle used by the wrong session is
  `PermissionDenied`).
- **Cancelled tasks + dropped streams** - `task_and_watch_registries_have_lifecycle` (`cancel_task`) and the
  stream cancel / `streaming_backpressure_completion_and_disconnect_cleanup` path (`cancel_stream`, dropped
  streams freed).
- **Unsupported HTTP policy** - `unsupported_http_methods_and_method_path_mismatch_are_rejected` (new,
  `crates/loom-hosted-core/src/remote_http.rs`): only `GET` (discovery/health) and `POST` (session/call) are
  served; other methods and method/path mismatches are rejected. Complemented by
  `post_dispatches_a_unary_call_and_rejects_a_bad_envelope` (a non-envelope body is a 400) and
  `get_serves_discovery_and_health_and_404s_unknown`.
- **TLS trust failure** - the client HTTP/2-over-TLS carrier validates the server certificate against the
  locator's trust config; the 420/430 remote live tests must select the `insecure-dev` trust to accept a
  self-signed localhost cert, which is direct evidence that the default `System` trust would reject it. A
  dedicated trust-rejection live test (System trust vs a self-signed endpoint -> TLS error) is a recorded
  owner-run follow-up.

Follow-ups (recorded, not hidden):

- **Duplicate idempotency-key enforcement - implemented (task 455), both sides.** Server:
  `RemoteRuntime::dispatch` (`crates/loom-hosted-core/src/remote.rs`) enforces section 6 under the single
  write authority - it keys a dedup table by `(session, interface, method, key)` bound to a canonical
  `(interface, method, args)` fingerprint, replays the stored terminal result on an exact-fingerprint
  retry (no re-applied effect), rejects a same-key/different-fingerprint reuse with `Code::Conflict`, and
  drops entries on session close/expiry (per-session capped). Client: the generated stubs auto-attach a
  fresh idempotency key to every section-12 `key` method (`requires_idempotency_key` in
  `uldren-loom-remote-codegen` selects them; `RemoteLoomClient::idempotency_options()` mints the key),
  while the low-level `call` path still accepts a caller-supplied key for durable app-level retries.
  Verified in-sandbox by `idempotency_replays_a_keyed_append_without_reapplying` (exactly-once on the
  sequence-appending `Queue.append`), `idempotency_rejects_same_key_with_a_different_request`,
  `idempotency_entries_are_dropped_on_session_close`, and the client
  `generated_key_methods_auto_attach_an_idempotency_key` (each keyed call mints a distinct key; a
  naturally idempotent method attaches none).
- **TLS-trust-rejection live test** (above).

## 12. IDL Method Coverage Matrix

This section is the normative per-method coverage contract required by section 11. It classifies every
interface and method in `idl/loom.idl` by transport kind, handle behavior, mutability, idempotency
expectation, stream shape, and implementation owner. Local and remote `LoomClient` implementations MUST
cover every remote round-trip method listed here; methods marked local-only run entirely in the client
and never cross the wire.

Coverage state: current. `idl/loom.idl` declares **41 interfaces and 353 methods** (`rg '^interface '
idl/loom.idl` = 41). The per-interface classification tables in this section are the normative contract
for transport kind, handle behavior, mutability, idempotency, stream shape, and owner. The tables below
cover every interface, including the `Archive`, `Car`, and `Metrics` families, the filesystem
import/export methods, and the `Document` text/binary contract added since the original draft; scalar
methods added to existing interfaces after the first draft (`Store.digest_algo`, the `*_indexed` graph
and document writes, and document index management) carry the same classification as their sibling rows
in the same interface.

> The authoritative, no-gaps map of **every** current IDL method to its client-parity status
> (suite-covered / live+protocol-covered / protocol-covered / session-connection / local-only /
> follow-up) is the companion report **`specs/0067c-client-parity-report.md`**, generated from
> `idl/loom.idl` and kept in sync by `uldren-loom-remote-codegen` (`--check`). Where an exact per-method
> enumeration is needed, that report is the source of truth; this section is the normative classification.

Owner invariant: for every remote round-trip method, the local owner is `LocalLoomClient` wrapping the
named subsystem directly, and the remote owner is `RemoteLoomClient` dispatching to the remote server
runtime, which invokes the same named subsystem as the single authority. The remote adapter never
becomes a second policy engine (section 8). The Owner column therefore names one subsystem that is
authoritative for both client kinds.

Legend.

Transport:

```text
U    unary request/response
Ua   unary that returns a Task; progress and terminal result arrive over a task stream
St   native IDL stream<T>
Uc   unary; MAY use a bounded byte-chunk or item-batch stream for large payloads (section 7)
Cn   connection/session establishment; a local path open maps to a remote protocol session
Sm   server metadata; unary, no session; describes the served build or runtime
Lp   local provisioning only; not exposed by the remote client in v1
Lo   local-only client operation; no remote round trip
Lc   local control plane; path-keyed local daemon, not the remote endpoint runtime
```

Handle (a leading `+` opens, a leading `-` frees, a bare name consumes):

```text
none   no handle
Sess   LoomSession (remote: the protocol session)
SqlS   SqlSession
Batch  SqlBatch
Iter   RowIter
Task   Task
View   ResultView
Fid    open file id (u64)
Path   path-keyed (loom_path); the remote client binds it to the endpoint's single Loom
```

Mutability: `read`, `write` (serialized through the single writer authority), `admin` (identity, ACL,
refs, key source, workspace management, control-plane config; enforced by the same PEP), `control`
(daemon, lock, task, and handle coordination), `session` (auth or session state on a handle), `pure`
(no store access, deterministic).

Idempotency: `n/a` (read, pure, or local), `idem` (naturally idempotent: keyed put/upsert/set,
content-addressed write, or compare-and-swap), `key` (idempotency key required per section 6:
sequence-appending, cursor-advancing, take-once, or replay effects), `sess` (session or auth op; safe to
re-issue), `ctl` (control-plane op that is idempotent for an already-satisfied state).

Stream shape: `none`, `watch` (ordered `DataChange` events plus resume cursor), `rows` (SQL row batches
plus result metadata), `chunk` (bounded byte chunks with digest or offset metadata), `task` (task state,
progress, terminal result, cancellation acknowledgement), `batch` (iterator or result-view item batches).

Owner: `ST` loom-store (persistence, at-rest key wraps, daemon and served-listener records); `CO`
loom-core engine (canonical object model, VCS, watch, filesystem, file handles, CAS, KV, graph, columnar,
dataframe, search, document, PIM calendar/contacts/mail, time-series, ledger, queue, identity, ACL,
protected refs, workspaces, locks); `HN` loom-hnsw (vector index, reached through the `CO` Vector facet);
`CP` loom-compute (Exec, and the server-authoritative trigger keeper and fire path, section 13); `SQ`
loom-sql (SQL sessions and batches, direct table and history readers, SQL-backed tasks and iterators,
result rendering); `BL` binding-local (client-side decode and thread-local error state; loom-ffi and the
result codec).

### Store

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| version | Sm | none | pure | n/a | none | ST/CO |
| capabilities | Sm | none | pure | n/a | none | ST/CO |
| runtime_profile | Sm | none | pure | n/a | none | ST/CO |
| blob_digest | Lo | none | pure | n/a | none | BL |
| create | Lp | Path | admin | n/a | none | ST |
| create_with_kek | Lp | Path | admin | n/a | none | ST |
| open | Cn | +Sess, Path | session | sess | none | ST |
| open_keyed | Cn | +Sess, Path | session | sess | none | ST |
| open_with_kek | Cn | +Sess, Path | session | sess | none | ST |
| close | Cn | -Sess | session | idem | none | ST |

Store notes: `create`/`create_with_kek` provision a new local `.loom`; a remote endpoint serves an
existing Loom, so provisioning is operator-side and out of the remote client surface in v1. `open*` on
the remote client establishes the protocol session against the endpoint; the at-rest passphrase or KEK
is a server-side unlock, so the remote open path carries authentication (section 8), not at-rest key
bytes. `version`/`capabilities`/`runtime_profile` report the served endpoint's build and runtime when
called remotely. `blob_digest` is deterministic hashing and runs client-side.

### KeySource

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| key_add_wrap_keyed | U | Sess | admin | key | none | ST |
| key_add_wrap_with_kek | U | Sess | admin | key | none | ST |
| key_remove_wrap | U | Sess | admin | key | none | ST |

KeySource note: at-rest DEK wraps are server-side material; the remote client manages them as
authenticated admin operations and never transmits raw key bytes in the context config (section 4).

### StoreAdmin

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| store_stat | U | Sess | read | n/a | none | ST |
| store_policy_get | U | Sess | read | n/a | none | ST |
| store_policy_set | U | Sess | admin | key | none | ST |
| store_rekey | U | Sess | admin | key | none | ST |

StoreAdmin note (task 640): the server-owned store-administration contract. Every method requires an
authenticated session and an explicit **global** (workspace=None, facet=None) `AclRight::Admin` grant -
the same engine ACL gate as other admin surfaces, so the server is not a second policy engine - and the
mutating methods (`store_policy_set`, `store_rekey`; plus `KeySource` key-wraps) are audited under the
authenticated actor. `store_rekey` runs entirely server-side: the client sends only the new passphrase;
the server mints the salt/nonce and (for a reseal) a fresh DEK, and the plaintext DEK is never returned
to or constructed by the client (fast DEK-rewrap and full `rekey_reseal` are both supported through the
existing `FileStore` APIs). It requires the served store to be unlocked (unencrypted, or an encrypted
store served with its unlock material - the encrypted-served-store follow-up); otherwise a precise
error. **Local/pure holdouts:** `store init` (creates a local store file) and `store hash` (a pure
digest) stay local and reject a remote locator. **Not exposed over remote:** raw global `store get`/`store
put` bypass workspace/facet authorization and reject a remote locator with a boundary error directing
callers to workspace-scoped `Cas` or `Transfer`; a privileged, audited raw-blob StoreAdmin surface is
deferred to a future task with its own security model.

### Workspaces

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| workspace_create | U | Sess | admin | idem by name, key when unnamed | none | CO |
| workspace_list | U | Sess | read | n/a | none | CO |
| workspace_rename | U | Sess | admin | idem | none | CO |
| workspace_delete | U | Sess | admin | idem | none | CO |

### Exec

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| exec_cbor | U | Sess | write | key | none | CP |

Exec note: `exec_cbor` runs a canonical `loom.exec.request.v1` program request and MAY commit, so it
requires an idempotency key. It is the program execution surface, not a trigger management surface;
trigger firing is dispatched by the server-authoritative keeper (section 13), not by this client method.

### Triggers

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| trigger_put | U | Sess | write | idem (by binding id) | none | CO |
| trigger_get | U | Sess | read | n/a | none | CO |
| trigger_list | Uc | Sess | read | n/a | chunk | CO |
| trigger_enable | U | Sess | write | idem | none | CO |
| trigger_remove | U | Sess | write | idem | none | CO |
| trigger_history | Uc | Sess | read | n/a | chunk | CO |

Triggers note: management CRUD lives on the Program facet and carries canonical-CBOR binding and fire
records (`bytes`). The keeper and fire path (`trigger_keeper_due`, `trigger_append_fire_record`, and the
compute-layer fire dispatch) is server-authoritative per section 9 and is intentionally not a
client-callable method, so it is not a row here.

### Sessions

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| authenticate_passphrase | U | Sess | session | sess | none | CO |
| clear_authentication | U | Sess | session | idem | none | CO |

### Identity

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| identity_list | U | Sess | read | n/a | none | CO |
| identity_add_principal | U | Sess | admin | key | none | CO |
| identity_set_passphrase | U | Sess | admin | idem | none | CO |
| identity_remove_principal | U | Sess | admin | idem | none | CO |
| identity_assign_role | U | Sess | admin | idem | none | CO |
| identity_revoke_role | U | Sess | admin | idem | none | CO |
| identity_create_external_credential | U | Sess | admin | key | none | CO |
| identity_revoke_external_credential | U | Sess | admin | idem | none | CO |
| identity_add_public_key | U | Sess | admin | key | none | CO |
| identity_revoke_public_key | U | Sess | admin | idem | none | CO |
| identity_create_app_credential | U | Sess | admin | key | none | CO |
| identity_revoke_app_credential | U | Sess | admin | idem | none | CO |

### Acl

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| acl_list | U | Sess | read | n/a | none | CO |
| acl_grant | U | Sess | admin | idem | none | CO |
| acl_revoke | U | Sess | admin | idem | none | CO |

### ProtectedRefs

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| protected_ref_list | U | Sess | read | n/a | none | CO |
| protected_ref_get | U | Sess | read | n/a | none | CO |
| protected_ref_set | U | Sess | admin | idem | none | CO |
| protected_ref_remove | U | Sess | admin | idem | none | CO |

### Daemon

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| daemon_start | Lc | Path | control | ctl | none | ST |
| daemon_stop | Lc | Path | control | ctl | none | ST |
| daemon_restart | Lc | Path | control | ctl | none | ST |
| daemon_status | Lc | Path | read | n/a | none | ST |
| daemon_doctor | Lc | Path | read | n/a | none | ST |
| daemon_session_attach | Lc | Path | control | idem | none | ST |
| daemon_session_detach | Lc | Path | control | idem | none | ST |
| daemon_pin_add | Lc | Path | control | idem | none | ST |
| daemon_pin_remove | Lc | Path | control | idem | none | ST |

Daemon note: the daemon is the local executable control plane keyed by a canonical `.loom` path. A
remote client cannot start or manage the endpoint's operating-system runtime, so `daemon_*` is local-only
and not part of the remote client surface. The remote endpoint owns the equivalent runtime state
server-side (section 9), and endpoint health is read through discovery (section 5), not `daemon_status`.

### Locks

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| lock_acquire | U | Path | control | idem | none | CO |
| lock_refresh | U | Path | control | idem | none | CO |
| lock_release | U | Path | control | idem | none | CO |

Locks note: locks are covered remotely. The `loom_path` argument binds to the endpoint's single Loom,
and the remote server evaluates fencing tokens and leases with the same coordinator concepts as local
locking (section 9). Fences are never trusted from client state alone.

### VersionControl

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| commit | U | Sess | write | idem (content-addressed) | none | CO |
| branch | U | Sess | write | idem | none | CO |
| checkout | U | Sess | write | idem | none | CO |
| log | U | Sess | read | n/a | none | CO |
| head_branch | U | Sess | read | n/a | none | CO |
| merge | U | Sess | write | key | none | CO |
| merge_in_progress | U | Sess | read | n/a | none | CO |
| merge_conflicts | U | Sess | read | n/a | none | CO |
| merge_resolve | U | Sess | write | idem | none | CO |
| merge_abort | U | Sess | write | idem | none | CO |
| merge_continue | U | Sess | write | key | none | CO |
| diff | Uc | Sess | read | n/a | chunk | CO |
| blame | Uc | Sess | read | n/a | chunk | CO |
| log_async | Ua | Sess, +Task | read | n/a | task | CO |
| merge_async | Ua | Sess, +Task | write | key | task | CO |
| status | U | Sess | read | n/a | none | CO |
| stage | U | Sess | write | idem | none | CO |
| stage_all | U | Sess | write | idem | none | CO |
| unstage | U | Sess | write | idem | none | CO |
| commit_staged | U | Sess | write | idem (content-addressed) | none | CO |
| tag_create | U | Sess | write | idem | none | CO |
| tag_list | U | Sess | read | n/a | none | CO |
| tag_target | U | Sess | read | n/a | none | CO |
| tag_delete | U | Sess | write | idem | none | CO |
| tag_rename | U | Sess | write | idem | none | CO |
| restore_file | U | Sess | write | idem | none | CO |
| restore_path | U | Sess | write | idem | none | CO |
| cherry_pick | U | Sess | write | key (idem when dry_run) | none | CO |
| revert | U | Sess | write | key (idem when dry_run) | none | CO |
| rebase | U | Sess | write | key (idem when dry_run) | none | CO |
| squash | U | Sess | write | key | none | CO |

### Watch

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| subscribe | U | Sess | control | idem | none | CO |
| poll | U | Sess | read | idem | none | CO |
| stream | St | Sess | read | n/a | watch | CO |

### FileSystem

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| write_file | Uc | Sess | write | idem (replace) | chunk | CO |
| read_file | Uc | Sess | read | n/a | chunk | CO |
| append_file | Uc | Sess | write | key | chunk | CO |
| remove_file | U | Sess | write | idem | none | CO |
| read_at | Uc | Sess | read | n/a | chunk | CO |
| write_at | Uc | Sess | write | idem (positional) | chunk | CO |
| truncate | U | Sess | write | idem | none | CO |
| symlink | U | Sess | write | idem | none | CO |
| read_link | U | Sess | read | n/a | none | CO |
| create_directory | U | Sess | write | idem | none | CO |
| remove_directory | U | Sess | write | idem | none | CO |
| stat | U | Sess | read | n/a | none | CO |
| list_directory | U | Sess | read | n/a | none | CO |
| import_fs | U | Sess | write | idem (content-addressed import) | none | CO |
| export_fs | U | Sess | read | n/a | none | CO |
| import_fs_async | Ua | Sess, +Task | write | idem (content-addressed import) | task | CO |
| export_fs_async | Ua | Sess, +Task | read | n/a | task | CO |

FileSystem notes: `create_directory`/`remove_directory` operate on the served store's working tree
(directories are first-class); `stat` returns canonical-CBOR `loom.fs.stat.v1` (`[path, kind, size,
mode]`) and `list_directory` returns canonical-CBOR `loom.fs.dir-listing.v1` (an array of `[name, kind]`
sorted by name), so both cross the wire as `bytes` and are decoded by the client. `import_fs`/`export_fs`
move a directory tree between the served store's Files facet
and a path on the **server's** host filesystem (the path argument is server-side), returning a canonical
CBOR import/export report. Re-importing the same source is content-addressed, so the object write is
`idem`; a caller wanting at-least-once safety around the optional commit MAY supply an idempotency key
through the low-level `call` path. The `_async` forms return a `Task`; in the current implementation the
task completes on first poll (the underlying operation is synchronous) and carries no
background/progress semantics beyond the task model.

**Remote classification (task 550): server-local/admin compatibility only.** `import_fs`/`export_fs`
(and the `_async` forms) interpret `src_path`/`dst_path` on the **server's** host filesystem, so they are
NOT part of the remote-capable public interchange contract - a remote caller's local path is meaningless
to the server. They remain as a server-local/admin convenience for the case where the caller and server
share a filesystem (or `loom` runs against a local `.loom`). The remote-capable interchange is the
byte-transfer `Transfer` interface (see section 17): the client owns local paths and streams bytes, and
the server only ever sees bytes, a workspace, a transfer kind, options, and integrity metadata.

### Archive

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| archive_import | U | Sess | write | idem (content-addressed import) | none | CO |
| archive_export | U | Sess | read | n/a | none | CO |
| archive_import_async | Ua | Sess, +Task | write | idem (content-addressed import) | task | CO |
| archive_export_async | Ua | Sess, +Task | read | n/a | task | CO |

Archive note: import/export a workspace's Files facet as a single archive (`zip`, `tar`, `tar-zstd`,
`tar-gzip`, or `gzip`) to or from a server-side host path, returning a canonical CBOR manifest plus
import/export report. An unknown archive kind is rejected with `InvalidArgument`. Idempotency and the
`_async` task semantics match the FileSystem import/export rows. **Remote classification (task 550):
server-local/admin compatibility only** (the `src_path`/`dst_path` is server-side); the remote-capable
path is the byte-transfer `Transfer` interface (section 17) with kind `tar-zstd`/`tar`/`tar-gzip`/`zip`/`gzip`.

### Car

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| car_import | U | Sess | write | idem (content-addressed blocks) | none | CO |
| car_export | U | Sess | read | n/a | none | CO |
| car_import_async | Ua | Sess, +Task | write | idem (content-addressed blocks) | task | CO |
| car_export_async | Ua | Sess, +Task | read | n/a | task | CO |

Car note: import/export content-addressed CAR blocks to or from a server-side host path. `car_import` is
**store-wide** (no workspace argument) and restores blocks by CID, so the write is content-addressed and
`idem`; `car_export` takes a workspace. Idempotency and the `_async` task semantics match the FileSystem
import/export rows. **Remote classification (task 550): server-local/admin compatibility only** (the
`src_path`/`dst_path` is server-side); the remote-capable path is the byte-transfer `Transfer` interface
(section 17) with kind `car`.

### Transfer

The remote-capable byte-transfer interchange (task 550). The client owns local filesystem paths; the
server only ever receives bytes, a workspace, a transfer kind, options, and integrity metadata - never a
client `src_path`/`dst_path`. Import is handle + chunked unary writes (v1, no reverse-stream carrier);
export is a server-to-client byte stream. Full contract in section 17.

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| transfer_import_open | U | Sess, +Transfer | control | key (mints a transfer; retry-safe) | none | CO |
| transfer_import_write | U | Sess, Transfer | write | key (per `(transfer, seq)`; replay of a seq is a no-op) | chunk | CO |
| transfer_import_finish | U | Sess, Transfer | write | key (finalize-once; verifies the running vs final digest) | none | CO |
| transfer_import_cancel | U | Sess, Transfer | control | idem | none | CO |
| transfer_export | Uc | Sess, +Transfer | read | n/a | chunk | CO |

Transfer note: `transfer_import_open` returns a `TransferId` and reserves a bounded server-side staging
buffer for `kind`. `transfer_import_write` appends a bounded chunk at a monotonic `seq` (or byte
`offset`), optionally carrying a per-chunk digest, and returns the accepted byte count / remaining credit
for backpressure. `transfer_import_finish` validates the running digest against the caller's
`final_digest`, applies the interchange (with `commit`/`dry_run`), and returns the canonical
`loom.interchange.import-report.v1`. `transfer_import_cancel` (and lease expiry) releases the staging
buffer. `transfer_export` returns a `stream<bytes>` of bounded chunks for `(workspace, kind, revision?)`;
the final `loom.interchange.export-report.v1` and content digest are delivered in the stream trailer. No
method takes a client-local path.

### FileHandle

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| open | U | Sess, +Fid | control | key (creates/truncates) | none | CO |
| read | Uc | Sess, Fid | read | key (cursor-advancing) | chunk | CO |
| read_at | Uc | Sess, Fid | read | n/a (positional) | chunk | CO |
| write | Uc | Sess, Fid | write | key (cursor-advancing) | chunk | CO |
| write_at | Uc | Sess, Fid | write | idem (positional) | chunk | CO |
| truncate | U | Sess, Fid | write | idem | none | CO |
| flush | U | Sess, Fid | control | idem | none | CO |
| stat | U | Sess, Fid | read | n/a | none | CO |
| close | U | Sess, -Fid | control | idem | none | CO |

FileHandle note: the open-file table is operational metadata excluded from commits and sync. A remote
file id is an opaque remote handle bound to its owning session; sequential `read`/`write` advance a
server-held cursor, so they carry idempotency keys, while the positional `read_at`/`write_at` do not.

### Cas

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| put | Uc | Sess | write | idem (content-addressed) | chunk | CO |
| get | Uc | Sess | read | n/a | chunk | CO |
| has | U | Sess | read | n/a | none | CO |
| delete | U | Sess | write | idem | none | CO |
| list | Uc | Sess | read | n/a | chunk | CO |

### Kv

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| put | U | Sess | write | idem | none | CO |
| get | U | Sess | read | n/a | none | CO |
| delete | U | Sess | write | idem | none | CO |
| list | Uc | Sess | read | n/a | chunk | CO |
| range | Uc | Sess | read | n/a | chunk | CO |
| list_collections | U | Sess | read | n/a | none | CO |

### Graph

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| upsert_node | U | Sess | write | idem | none | CO |
| get_node | U | Sess | read | n/a | none | CO |
| remove_node | U | Sess | write | idem | none | CO |
| upsert_edge | U | Sess | write | idem | none | CO |
| get_edge | U | Sess | read | n/a | none | CO |
| remove_edge | U | Sess | write | idem | none | CO |
| neighbors | U | Sess | read | n/a | none | CO |
| out_edges | U | Sess | read | n/a | none | CO |
| in_edges | U | Sess | read | n/a | none | CO |
| reachable | Uc | Sess | read | n/a | chunk | CO |
| shortest_path | U | Sess | read | n/a | none | CO |
| query | Uc | Sess | read | n/a | chunk | CO |
| explain_query | Uc | Sess | read | n/a | chunk | CO |

### Vector

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| create | U | Sess | write | idem | none | CO/HN |
| upsert | U | Sess | write | idem | none | CO/HN |
| upsert_source | U | Sess | write | idem | none | CO/HN |
| get | U | Sess | read | n/a | none | CO/HN |
| source_text | U | Sess | read | n/a | none | CO/HN |
| embedding_model | U | Sess | read | n/a | none | CO/HN |
| ids | Uc | Sess | read | n/a | chunk | CO/HN |
| metadata_index_keys | U | Sess | read | n/a | none | CO/HN |
| create_metadata_index | U | Sess | write | idem | none | CO/HN |
| drop_metadata_index | U | Sess | write | idem | none | CO/HN |
| delete | U | Sess | write | idem | none | CO/HN |
| search | Uc | Sess | read | n/a | chunk | CO/HN |
| search_policy | Uc | Sess | read | n/a | chunk | CO/HN |

### Columnar

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| create | U | Sess | write | idem | none | CO |
| append | U | Sess | write | key | none | CO |
| compact | U | Sess | control | idem | none | CO |
| inspect | U | Sess | read | n/a | none | CO |
| source_digest | U | Sess | read | n/a | none | CO |
| scan | Uc | Sess | read | n/a | chunk | CO |
| columns | U | Sess | read | n/a | none | CO |
| rows | U | Sess | read | n/a | none | CO |
| select | Uc | Sess | read | n/a | chunk | CO |
| aggregate | Uc | Sess | read | n/a | chunk | CO |

### Dataframe

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| create | U | Sess | write | idem | none | CO |
| collect | Uc | Sess | read | n/a | chunk | CO |
| preview | Uc | Sess | read | n/a | chunk | CO |
| materialize | U | Sess | write | idem (content-addressed) | none | CO |
| plan_digest | U | Sess | read | n/a | none | CO |
| source_digests | U | Sess | read | n/a | none | CO |

### Search

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| create | U | Sess | write | idem | none | CO |
| index | U | Sess | write | idem | none | CO |
| get | U | Sess | read | n/a | none | CO |
| delete | U | Sess | write | idem | none | CO |
| ids | Uc | Sess | read | n/a | chunk | CO |
| remap | U | Sess | write | idem | none | CO |
| query | Uc | Sess | read | n/a | chunk | CO |
| source_digest | U | Sess | read | n/a | none | CO |
| status | U | Sess | read | n/a | none | CO |

### ManagementKv

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| set_config | U | Sess | admin | idem | none | CO |
| get_config | U | Sess | read | n/a | none | CO |

### Document

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| put | U | Sess | write | idem | none | CO |
| get | U | Sess | read | n/a | none | CO |
| put_text | U | Sess | write | idem (keyed replace by id; optional entity-tag guard) | none | CO |
| get_text | U | Sess | read | n/a | none | CO |
| put_binary | U | Sess | write | idem (keyed replace by id; optional entity-tag guard) | none | CO |
| get_binary | U | Sess | read | n/a | none | CO |
| list_binary | U | Sess | read | n/a | none | CO |
| delete | U | Sess | write | idem | none | CO |
| list_collections | U | Sess | read | n/a | none | CO |

Document text/binary note: `put_text`/`put_binary` store a document by id (a keyed replace, so `idem`,
matching `Document.put_binary`; documents are content-addressed) and return `DocumentPutResult` with
the new content `Digest` plus the owner-issued document `entity_tag`. Their optional
`expected_entity_tag` is the native compare token for guarded replacement. A mismatch is a conditional
mutation conflict, not an idempotency-key concern. `get_text` returns `optional DocumentTextResult`,
canonical CBOR `[text, digest, entity_tag]`, and fails with `DOCUMENT_NOT_TEXT` when the stored bytes
are not valid UTF-8; `get_binary` returns `optional DocumentBinaryResult`, canonical CBOR
`[bytes, digest, entity_tag]`. `list_binary` returns the canonical-CBOR encoded collection. None are
section-6 `key` methods.

Native document surfaces MUST NOT mint a facet-specific compare-token name such as
`expected_digest`. Compatibility facades may keep protocol-specific vocabulary only as an explicit
adapter that maps into the shared entity-tag contract.

### Calendar

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| create_collection | U | Sess | write | idem | none | CO |
| get_collection | U | Sess | read | n/a | none | CO |
| list_collections | U | Sess | read | n/a | none | CO |
| delete_collection | U | Sess | write | idem | none | CO |
| put_entry | U | Sess | write | idem (by uid) | none | CO |
| put_ics | U | Sess | write | idem (content-addressed etag) | none | CO |
| get_entry | U | Sess | read | n/a | none | CO |
| delete_entry | U | Sess | write | idem | none | CO |
| list_entries | Uc | Sess | read | n/a | chunk | CO |
| range | Uc | Sess | read | n/a | chunk | CO |
| search | Uc | Sess | read | n/a | chunk | CO |
| to_ics | U | Sess | read | n/a | none | CO |

### Contacts

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| create_book | U | Sess | write | idem | none | CO |
| get_book | U | Sess | read | n/a | none | CO |
| list_books | U | Sess | read | n/a | none | CO |
| delete_book | U | Sess | write | idem | none | CO |
| put_entry | U | Sess | write | idem (by uid) | none | CO |
| put_vcard | U | Sess | write | idem (content-addressed etag) | none | CO |
| get_entry | U | Sess | read | n/a | none | CO |
| delete_entry | U | Sess | write | idem | none | CO |
| list_entries | Uc | Sess | read | n/a | chunk | CO |
| search | Uc | Sess | read | n/a | chunk | CO |
| to_vcard | U | Sess | read | n/a | none | CO |

### Mail

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| create_mailbox | U | Sess | write | idem | none | CO |
| get_mailbox | U | Sess | read | n/a | none | CO |
| list_mailboxes | U | Sess | read | n/a | none | CO |
| delete_mailbox | U | Sess | write | idem | none | CO |
| ingest_message | Uc | Sess | write | idem (by uid, CAS body) | chunk | CO |
| get_message | U | Sess | read | n/a | none | CO |
| to_eml | Uc | Sess | read | n/a | chunk | CO |
| delete_message | U | Sess | write | idem | none | CO |
| list_messages | Uc | Sess | read | n/a | chunk | CO |
| get_flags | U | Sess | read | n/a | none | CO |
| set_flags | U | Sess | write | idem (replace) | none | CO |
| search | Uc | Sess | read | n/a | chunk | CO |

### Lanes

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| create | U | Sess | write | idem (by lane id) | none | CO |
| get | U | Sess | read | n/a | none | CO |
| list | U | Sess | read | n/a | none | CO |
| update | U | Sess | write | idem (replace fields) | none | CO |
| ticket_add | U | Sess | write | idem (by ticket id) | none | CO |
| ticket_remove | U | Sess | write | idem | none | CO |
| get_view_json | U | Sess | read | n/a | none | CO |
| list_views_json | U | Sess | read | n/a | none | CO |

Lanes note: Lane management is a source-backed public surface for assignment queues and tracking
sets. It is declared in `idl/loom.idl:1324`, implemented by `loom-lanes`, forwarded by the generated
remote client/server surface, exposed through MCP Lane tools, hosted REST/JSON-RPC Lane handlers, and
C ABI functions `loom_lanes_*_cbor`. Lane records carry required `lane_kind` (`assignment` or
`tracking`) and optional `owner_principal` coordinator metadata. Ticket ownership remains a Tickets
facet concern. Lane records and ticket membership are canonical CBOR values; validation belongs to
the shared `loom-lanes` model, not to per-surface copies. Public Lane read and mutation surfaces
expose ordered ticket ids without numeric ranks; sparse order metadata is an internal storage detail.
The public `delete` operation removes only a closed Lane coordination record and its membership list.
It never deletes tickets, mutates ticket status, erases ticket history, or removes ticket-owned
relations. The `get_view_json` and
`list_views_json` methods return read-only Lane views resolved against a ticket workspace: the
compact projection carries the label, derived display status, and ordered ticket ids, while the
detailed projection adds stored status, owner, updated time, ordered ticket summaries with status,
priority, and title, status report, and reviewer feedback, excluding ticket descriptions, comments,
and history. Display status derives from the first ticket with explicit paused, closed, and
Lane-level blocked overrides. The C ABI realizes these as `loom_lanes_get_view_json` and
`loom_lanes_list_views_json`.

### TimeSeries

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| put | U | Sess | write | idem (replace by ts) | none | CO |
| get | U | Sess | read | n/a | none | CO |
| range | Uc | Sess | read | n/a | chunk | CO |
| latest | U | Sess | read | n/a | none | CO |
| list_collections | U | Sess | read | n/a | none | CO |

### Metrics

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| put_descriptor | U | Sess | write | idem (replace by name) | none | CO |
| get_descriptor | U | Sess | read | n/a | none | CO |
| put_observation | U | Sess | write | idem (content-addressed by series + timestamp) | none | CO |
| query | U | Sess | read | n/a | none | CO |

Metrics note: native metric descriptors and observations are Loom-owned canonical CBOR records on the
Metrics facet, siblings of `TimeSeries`. `put_descriptor` replaces by descriptor name and
`put_observation` is content-addressed by series id and timestamp, so both are `idem`. `query` is a
bounded, half-open `[from_timestamp_ms, to_timestamp_ms)` read returning canonical CBOR
`[observations, partial, stale]`; an absent workspace yields an empty, non-partial result.

### Ledger

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| append | U | Sess | write | key | none | CO |
| get | U | Sess | read | n/a | none | CO |
| head | U | Sess | read | n/a | none | CO |
| len | U | Sess | read | n/a | none | CO |
| verify | U | Sess | read | n/a | none | CO |
| list_collections | U | Sess | read | n/a | none | CO |

### Queue

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| append | U | Sess | write | key | none | CO |
| get | U | Sess | read | n/a | none | CO |
| range | Uc | Sess | read | n/a | chunk | CO |
| len | U | Sess | read | n/a | none | CO |
| list_streams | U | Sess | read | n/a | none | CO |

### QueueConsumers

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| consumer_position | U | Sess | read | n/a | none | CO |
| consumer_read | Uc | Sess | read | n/a | chunk | CO |
| consumer_advance | U | Sess | control | idem (monotonic) | none | CO |
| consumer_reset | U | Sess | control | idem | none | CO |

QueueConsumers note: consumer offsets are operational metadata in the served store; they are not part of
commits, stream roots, clone, push, bundle, or ordinary sync, and the remote runtime owns them.

### Sql

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| sql_open | Cn | +SqlS, Path | session | sess | none | SQ |
| sql_open_keyed | Cn | +SqlS, Path | session | sess | none | SQ |
| sql_open_with_kek | Cn | +SqlS, Path | session | sess | none | SQ |
| sql_open_authenticated | Cn | +SqlS, Path | session | sess | none | SQ |
| sql_open_keyed_authenticated | Cn | +SqlS, Path | session | sess | none | SQ |
| sql_open_with_kek_authenticated | Cn | +SqlS, Path | session | sess | none | SQ |
| sql_authenticate_passphrase | U | SqlS | session | sess | none | SQ |
| sql_exec | U | SqlS | write | key | none | SQ |
| sql_query | St | SqlS | read | n/a | rows | SQ |
| sql_commit | U | SqlS | write | idem (content-addressed) | none | SQ |
| sql_close | Cn | -SqlS | session | idem | none | SQ |
| sql_batch_begin | Cn | +Batch, Path | session | sess | none | SQ |
| sql_batch_begin_keyed | Cn | +Batch, Path | session | sess | none | SQ |
| sql_batch_begin_with_kek | Cn | +Batch, Path | session | sess | none | SQ |
| sql_batch_begin_authenticated | Cn | +Batch, Path | session | sess | none | SQ |
| sql_batch_begin_keyed_authenticated | Cn | +Batch, Path | session | sess | none | SQ |
| sql_batch_begin_with_kek_authenticated | Cn | +Batch, Path | session | sess | none | SQ |
| sql_batch_exec | U | Batch | write | key | none | SQ |
| sql_batch_commit | U | Batch | write | key | none | SQ |
| sql_batch_commit_vcs | U | Batch | write | key | none | SQ |
| sql_batch_abort | U | Batch | control | idem | none | SQ |
| sql_batch_close | Cn | -Batch | session | idem | none | SQ |
| sql_read_table | Uc | Sess | read | n/a | chunk | SQ |
| sql_read_table_at | Uc | Sess | read | n/a | chunk | SQ |
| sql_index_scan | Uc | Sess | read | n/a | chunk | SQ |
| sql_index_scan_at | Uc | Sess | read | n/a | chunk | SQ |
| sql_blame | Uc | Sess | read | n/a | chunk | SQ |
| sql_diff | Uc | Sess | read | n/a | chunk | SQ |
| sql_table_diff | Uc | Sess | read | n/a | chunk | SQ |
| sql_read_table_async | Ua | Sess, +Task | read | n/a | task | SQ |
| sql_index_scan_async | Ua | Sess, +Task | read | n/a | task | SQ |
| sql_blame_async | Ua | Sess, +Task | read | n/a | task | SQ |
| sql_diff_async | Ua | Sess, +Task | read | n/a | task | SQ |
| sql_list_databases | U | Sess | read | n/a | none | SQ |

Sql note: `sql_open*` and `sql_batch_begin*` are path-keyed on the local binding; the remote client binds
them to the endpoint's single Loom and carries the authenticated principal instead of at-rest key bytes.
`sql_query` maps to a row stream; its source realization is a `RowIter` (see Tasks).

### Diagnostics

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| result_to_json | Lo | none | pure | n/a | none | BL |
| result_to_bridge_json | Lo | none | pure | n/a | none | BL |
| last_error | Lo | none | pure | n/a | none | BL |

Diagnostics note: all three run client-side. `result_to_json` and `result_to_bridge_json` decode a
result buffer the client already holds. Remote per-call failures ride the response error envelope
(section 6); `last_error` reports the binding's thread-local error state, not a remote round trip.

### Tasks

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| iter_next | Uc | Iter | read | n/a | batch | SQ |
| iter_free | Lo | -Iter | control | idem | none | SQ |
| sql_exec_async | Ua | SqlS, +Task | write | key | task | SQ |
| task_poll | U | Task | control | idem | task | SQ |
| task_status | U | Task | read | n/a | none | SQ |
| task_result | U | Task | read | key (take-once) | none | SQ |
| task_cancel | U | Task | control | idem | task | SQ |
| task_free | Lo | -Task | control | idem | none | SQ |
| task_wait | U | Task | read | key (take-once) | task | SQ |

Tasks note: `RowIter` and `Task` are opaque remote handles bound to their owning session. `iter_next`
pulls the next item from the server row stream. `iter_free`/`task_free` release client-side handle state
and signal the server registry to close the remote handle; the free call itself is a local operation.

### ResultViews

| Method | Transport | Handle | Mutability | Idempotency | Stream | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| result_open | Lo | +View | pure | n/a | none | BL |
| row_open | Lo | +View | pure | n/a | none | BL |
| result_close | Lo | -View | pure | n/a | none | BL |
| result_len | Lo | View | pure | n/a | none | BL |
| result_is_statements | Lo | View | pure | n/a | none | BL |
| result_item_kind | Lo | View | pure | n/a | none | BL |
| result_column_count | Lo | View | pure | n/a | none | BL |
| result_column_name | Lo | View | pure | n/a | none | BL |
| result_column_type | Lo | View | pure | n/a | none | BL |
| result_row_count | Lo | View | pure | n/a | none | BL |
| result_row_len | Lo | View | pure | n/a | none | BL |
| result_cell | Lo | View | pure | n/a | none | BL |
| result_row_commit | Lo | View | pure | n/a | none | BL |
| result_count | Lo | View | pure | n/a | none | BL |
| result_string_count | Lo | View | pure | n/a | none | BL |
| result_string | Lo | View | pure | n/a | none | BL |
| result_variable_kind | Lo | View | pure | n/a | none | BL |
| result_merge_outcome | Lo | View | pure | n/a | none | BL |
| result_diff_count | Lo | View | pure | n/a | none | BL |
| result_diff_change | Lo | View | pure | n/a | none | BL |
| result_diff_len | Lo | View | pure | n/a | none | BL |
| result_diff_cell | Lo | View | pure | n/a | none | BL |
| result_map_len | Lo | View | pure | n/a | none | BL |
| result_map_entry | Lo | View | pure | n/a | none | BL |

ResultViews note: a `ResultView` decodes a canonical-CBOR buffer the client already holds into an
indexed, typed view. Every method runs entirely client-side, so remote clients decode the `value` or
streamed row bytes locally and never issue a `ResultView` round trip. This is the largest local-only
group and materially narrows the remote round-trip surface.

## 13. Trigger Surface

Source-backed state. `crates/loom-core/src/triggers.rs:21` provides the engine trigger storage facade:
`trigger_put` (`crates/loom-core/src/triggers.rs:21`), `trigger_get`
(`crates/loom-core/src/triggers.rs:30`), `trigger_list` (`crates/loom-core/src/triggers.rs:43`),
`trigger_enable` (`crates/loom-core/src/triggers.rs:68`), `trigger_remove`
(`crates/loom-core/src/triggers.rs:81`), `trigger_history` (`crates/loom-core/src/triggers.rs:97`),
`trigger_append_fire_record` (`crates/loom-core/src/triggers.rs:137`), and `trigger_keeper_due`
(`crates/loom-core/src/triggers.rs:155`). Firing is dispatched by `crates/loom-compute/src/trigger_exec.rs:90`
(`fire_trigger_candidate`) and the keeper wiring in `crates/loom-core/src/hooks.rs:10`.

Trigger management is not covered by `Exec`: `exec_cbor` (`idl/loom.idl:434`) runs a program request,
while trigger management is separate create/read/enable/remove/history storage on the Program facet.

Resolved decision (owner selected promotion). Trigger management is promoted to a first-class
`interface Triggers` at `idl/loom.idl:1280`, exposing `trigger_put`, `trigger_get`, `trigger_list`,
`trigger_enable`, `trigger_remove`, and `trigger_history` over the Program facet with canonical-CBOR
binding and fire-record values. This gives the management surface a normative IDL home, so the remote
`LoomClient` contract covers it like any other interface (see the `Triggers` row in section 12).

Coverage split for the remote contract:

- Trigger management (`put`, `get`, `list`, `enable`, `remove`, `history`) is a client-facing CRUD
  surface with an IDL home. Local and remote `LoomClient` implementations cover it through the same
  `Triggers` trait family as every other interface.
- The keeper and fire path (`trigger_keeper_due`, `trigger_append_fire_record`, and the compute-layer
  fire dispatch) is server-authoritative. Section 9 requires the remote runtime to own a "trigger keeper
  for enabled trigger behavior." These are not client methods and are not promoted to a client-callable
  interface.

Remaining promotion follow-through. Adding the `Triggers` interface to `idl/loom.idl` is the definition
step. Because "Output and ABI shapes are part of the contract" (`AGENTS.md`), the C ABI export set in
`include/loom.h`, the direct-binding projections, and the conformance vectors for `Triggers` must land
together as their own change before the C ABI and bindings expose the interface. Until then the IDL leads
the C ABI for this interface. Queue 11 client, server, and conformance tasks cover the `LoomClient`
surface directly over `loom_core::trigger_*`; the C ABI export set is tracked separately because it is
broader than the remote-client scope. The MCP tool surface folds `Triggers` (no MCP tools) in
`crates/loom-mcp/src/tools.rs`; whether to expose trigger management as MCP tools is a separate 0043
surface decision.

## 14. LoomApi Client Trait Surface

This section settles task 130. It defines the Rust trait surface that both `LocalLoomClient` and
`RemoteLoomClient` implement. The surface is derived mechanically from `idl/loom.idl` per the rules
below, so the IDL stays the single source of truth and the section 12 matrix is the coverage checklist.

Settled decisions:

- Async-first. Methods that can perform a remote round trip are `async fn`. `RemoteLoomClient` awaits
  network I/O; `LocalLoomClient` calls the synchronous engine and returns a ready result. Methods
  classified local-only in section 12 (transport `Lo`: all of `ResultViews`, the three `Diagnostics`
  methods, `Store.blob_digest`, and the handle-free calls `iter_free` and `task_free`) are plain `fn`
  because neither client performs I/O for them. The section 12 Transport column is the rule: `Lo` maps to
  `fn`, every other transport maps to `async fn`. Native `async fn` in traits is used (MSRV 1.89), with no
  `async_trait` dependency.
- One trait per interface. Each of the 42 IDL interfaces maps to one trait of the same name (`Store`,
  `Kv`, `VersionControl`, ..., `Exec`, `Lanes`, `Triggers`). A `LoomClient` supertrait composes all 42:
  `pub trait LoomClient: Store + KeySource + ... + Exec + Lanes + Triggers + Send + Sync {}`. This mirrors the IDL
  one to one, keeps each trait small enough to review, and lets platform packages implement subsets.
- One error type. Every fallible method returns `Result<T, LoomError>`, preserving the stable `Code` enum
  verbatim (the error contract in `AGENTS.md` and section 6).

Type mapping (IDL to Rust):

```text
bool i32 i64 u32 u64 f64   the same Rust scalar
string  (parameter)        &str
string  (return)           String
bytes   (parameter)        &[u8]
bytes   (return)           Vec<u8>
Uuid                       Uuid
Digest, structs, enums     the shared model type of the same name (Digest, Workspace, LockToken, ...)
optional<T>                Option<T>
list<T>                    Vec<T>
stream<T>                  LoomStream<T> = Pin<Box<dyn Stream<Item = Result<T, LoomError>> + Send>>
```

Handle mapping. The opaque handle types `LoomSession`, `SqlSession`, `SqlBatch`, `RowIter`, `Task`, and
`ResultView` appear in signatures exactly where the IDL declares them. Their ownership, lease renewal, and
close/free semantics are task 140; this section only fixes that they appear in signatures.

Uniform surface with runtime capability errors. Traits mirror the IDL interface one to one, so `Store`
keeps `create`/`create_with_kek` and `Daemon` keeps its lifecycle methods, and both are in the
`LoomClient` supertrait. Methods a remote endpoint cannot serve return `Code::Unsupported` at runtime
rather than being split into a separate trait: the local-provisioning methods (`Store.create`,
`Store.create_with_kek`, transport `Lp`) and the local daemon control plane (`Daemon.*`, transport `Lc`)
are supported by `LocalLoomClient` and return `Unsupported` from `RemoteLoomClient`. A uniform `LoomClient`
keeps `loom mcp <STORE>` and other store-taking commands polymorphic over local and remote without a
split type hierarchy.

Elision rule. Path-keyed methods drop their `loom_path` argument because a `LoomClient` is already bound to
one store (local file) or endpoint (remote): `Store.open*`, `Daemon.*`, `Locks.*`, `Sql.sql_open*`, and
`Sql.sql_batch_begin*` take the remaining arguments only. This follows section 3 (the locator binds the
store) and the one-Loom-per-endpoint rule.

Dispatch note. Whether runtime local-vs-remote selection uses a boxed-future dyn-compatible surface or a
generated enum wrapper is an implementation-structure choice deferred to task 150 (crate and module
layout); it does not change the trait signatures defined here.

Generated wire surface. `loom-remote-codegen` emits these trait families into `loom-client`
(`src/generated_api.rs`) from `idl/loom.idl`. To keep `loom-client` engine-free, the generated wire-level
signatures map named composite IDL types (structs and enums such as `Workspace`, `WatchSelector`,
`LockMode`) to canonical CBOR `Vec<u8>`, matching the IDL's own use of `bytes` for structured payloads;
scalars, `string`, `bytes`, `Uuid`, `Digest`, `optional`, `list`, `stream`, and the opaque handles map to
concrete Rust types. Async methods use `-> impl Future + Send` (RPITIT) rather than `async fn` so the
Send bound is explicit. A model-typed ergonomic layer over these composites may wrap the wire surface
later without changing it.

Canonical examples (each interface follows one of these shapes):

```rust
// unary read/write, session-scoped
pub trait Kv {
    async fn put(&self, session: &LoomSession, workspace: &str, collection: &str,
                 key: &[u8], value: &[u8]) -> Result<(), LoomError>;
    async fn get(&self, session: &LoomSession, workspace: &str, collection: &str,
                 key: &[u8]) -> Result<Option<Vec<u8>>, LoomError>;
    async fn list(&self, session: &LoomSession, workspace: &str, collection: &str)
        -> Result<Vec<u8>, LoomError>;
}

// native stream
pub trait Watch {
    async fn subscribe(&self, session: &LoomSession, selector: WatchSelector,
                       from: Option<Digest>) -> Result<WatchSubscription, LoomError>;
    async fn poll(&self, session: &LoomSession, cursor: &str, max: u32)
        -> Result<WatchBatch, LoomError>;
    async fn stream(&self, session: &LoomSession, selector: WatchSelector,
                    from: Option<Digest>) -> Result<LoomStream<DataChange>, LoomError>;
}

// handle-producing, path elided, task- and stream-returning
pub trait Sql {
    async fn sql_open(&self, workspace: &str, db: &str) -> Result<SqlSession, LoomError>;
    async fn sql_exec(&self, session: &SqlSession, sql: &str) -> Result<Vec<u8>, LoomError>;
    async fn sql_query(&self, session: &SqlSession, sql: &str)
        -> Result<LoomStream<Vec<u8>>, LoomError>;
    async fn sql_read_table_async(&self, session: &LoomSession, workspace: &str, table: &str)
        -> Result<Task, LoomError>;
    // remaining Sql methods, with sql_open* and sql_batch_begin* loom_path elided
}

// promoted interface (bytes returns are canonical CBOR, per section 12)
pub trait Triggers {
    async fn trigger_put(&self, session: &LoomSession, workspace: &str, binding: &[u8])
        -> Result<(), LoomError>;
    async fn trigger_get(&self, session: &LoomSession, workspace: &str, id: Uuid)
        -> Result<Vec<u8>, LoomError>;
    async fn trigger_list(&self, session: &LoomSession, workspace: &str)
        -> Result<Vec<u8>, LoomError>;
    async fn trigger_enable(&self, session: &LoomSession, workspace: &str, id: Uuid,
                            enabled: bool) -> Result<Vec<u8>, LoomError>;
    async fn trigger_remove(&self, session: &LoomSession, workspace: &str, id: Uuid)
        -> Result<bool, LoomError>;
    async fn trigger_history(&self, session: &LoomSession, workspace: &str, id: Uuid,
                             from_seq: u64, limit: u64) -> Result<Vec<u8>, LoomError>;
}

// local-only decode: plain fn, no I/O
pub trait ResultViews {
    fn result_open(&self, result: &[u8]) -> Result<ResultView, LoomError>;
    fn result_len(&self, view: &ResultView) -> u64;
    // remaining result_* accessors, all plain fn
}
```

Coverage. Applying these rules to `idl/loom.idl` yields 42 traits and 363 methods, one per section 12
row. Of the 363, 30 are plain `fn` (transport `Lo`: 24 `ResultViews`, 3 `Diagnostics`, `Store.blob_digest`,
`Tasks.iter_free`, `Tasks.task_free`) and 333 are `async fn`. Eleven methods return `Code::Unsupported`
from `RemoteLoomClient` (transport `Lp`: `Store.create`, `Store.create_with_kek`; transport `Lc`: the nine
`Daemon.*` methods). These counts are verified against the section 12 matrix.

## 15. Handle And Lifetime Contract

This section settles task 140. It defines how the opaque handle types behave across `LocalLoomClient` and
`RemoteLoomClient`: `LoomSession`, `SqlSession`, `SqlBatch`, `RowIter`, `Task`, `ResultView`, and the file
id (`u64`) returned by `FileHandle.open`. It refines the runtime registries in section 9 and the handle
mapping deferred from section 14. Local handles are in-process values over the engine; remote handles are
opaque ids resolved by the server registries. The observable lifecycle is identical for both clients
unless stated otherwise.

### 15.1 Remote handle identity

A remote handle id is a canonical-CBOR value carrying: `kind` (one of `session`, `sql_session`,
`sql_batch`, `row_iter`, `task`, `file`), opaque `id` bytes minted by the server, a `generation` counter,
and the `owner_session` id it is bound to. Generation makes a reclaimed id non-reusable: a call on a stale
id whose generation no longer matches returns `Code::NotFound`. Handle ids are opaque to the client; the
client MUST NOT synthesize or mutate them. `ResultView` is not in this list because it never becomes a
remote handle (see 15.6).

### 15.2 Ownership and binding

Every handle is owned by exactly one session and is bound to that session by default (`owner_session`).
A session-bound handle survives a dropped transport connection and remains usable after reconnect while
the session lease is valid (15.3). A client MAY request a connection-bound handle at open time; a
connection-bound handle is closed by the server the instant its connection drops. Handles are not
transferable between sessions. A handle call presented on a session other than its owner returns
`Code::PermissionDenied`.

### 15.3 Sessions: leases, renewal, expiry

A remote session carries a lease. Any request bearing the session id refreshes the lease as a side
effect, so an actively used session never expires; an idle client sends keepalive pings to hold the
lease. The default lease TTL is 60 seconds and the default keepalive interval is one third of the TTL;
both are endpoint-configurable and advertised so the client can adapt. Lease renewal is a transport-level
concern and adds no IDL method.

When a lease expires, the server reclaims the session and every handle it owns: sessions, SQL sessions,
batches (rolled back, since an uncommitted batch discards on close per the `Sql` contract), iterators,
tasks, and file ids (a file id closing as the last handle on an unlinked inode reclaims its bytes, per the
`FileHandle` contract). Expiry is the same code path as an explicit close, so no leak is possible.
`LocalLoomClient` maps a session to a live engine open with no lease; its handles live until explicit
close or process exit.

### 15.4 Close and free semantics

Every handle has an explicit release call from the IDL: `Store.close` (`LoomSession`), `Sql.sql_close`
(`SqlSession`), `Sql.sql_batch_close` (`SqlBatch`), `Tasks.iter_free` (`RowIter`), `Tasks.task_free`
(`Task`), `FileHandle.close` (file id), and `ResultViews.result_close` (`ResultView`). Release is
idempotent: releasing an already-released or expired handle succeeds without error. On the remote client,
`iter_free`, `task_free`, `result_close`, and the handle-close calls run locally to drop client state and
signal the server registry to close the remote handle; the transport `Lo` classification in section 12
marks the calls that need no round trip of their own. The server also closes a handle on lease expiry
(15.3) and closes connection-bound handles and all streams for a connection when that connection drops
(section 9). Closing a parent invalidates its children: closing a `LoomSession` closes the file ids,
iterators, and tasks it owns.

### 15.5 Reconnection limits

Remote reconnection is bounded and never queues work (section 9 fail-fast). On a transport drop mid
request, the client MAY retry the connection within the call deadline, with backoff, up to an
endpoint-and-client bounded attempt count; when the deadline passes or attempts are exhausted the call
returns `Code::Unavailable`. Mutating calls retry only when they carry an idempotency key (section 6), so
a retried write cannot double-apply. After reconnect, a session resumes if its lease is still valid and
its session-bound handles are still usable; connection-bound handles are gone. Streams do not auto-resume:
a watch stream is re-opened from its resume cursor (section 7), and a SQL row stream, byte-chunk stream,
or task-event stream is re-issued. The client MUST NOT persist any request, write, lock, or stream open
for later replay.

### 15.6 Tasks: cancellation and terminal-result retention

A `Task` starts `PENDING`; the first poll drives it to a terminal `READY`, `ERROR`, or `CANCELLED`; taking
its result leaves it `TAKEN` (the `TaskStatus` contract). `task_cancel` cancels a still-pending task and is
idempotent on an already-terminal task; cancellation releases the task's compute and streams promptly and
records `CANCELLED`. The server retains a terminal task's status and, for `READY`, its one result buffer
until the earliest of: `task_result` or `task_wait` taking it (moving it to `TAKEN`), `task_free`, or the
owning session's lease expiry. There is a per-endpoint terminal-result retention timeout, defaulting to
the session lease TTL, after which an untaken terminal result is dropped and its handle reclaimed. Task
event streams carry state changes, progress, the terminal result, and the cancellation acknowledgement
(section 7).

### 15.7 Result views and iterators: local-vs-remote decode

A `ResultView` decodes a canonical-CBOR result buffer that the client already holds; it never crosses the
wire and never becomes a remote handle (section 12 transport `Lo`). A `RowIter` is the forward-only source
realization behind `sql_query`: on `LocalLoomClient` it advances an in-process reader; on
`RemoteLoomClient` it is a client-side cursor over the server SQL row stream, and `iter_next` pulls the
next row batch.

Decode has two client options, chosen by result size and memory budget:

- Eager: fetch the whole result buffer from a unary `value` and `result_open` it once. Used for bounded
  results.
- Lazy: consume the row stream and `row_open` each streamed row as it arrives (`iter_next` then
  `row_open`), bounding client memory for large SELECTs. The remote client SHOULD stream large query
  results rather than buffering them.

Both options decode entirely on the client, so a remote result view and a local result view expose the
same typed accessors over the same bytes; only the fetch path differs.

### 15.8 Handle summary

| Handle | Opened by | Released by | Binding | Remote reclaim triggers |
| --- | --- | --- | --- | --- |
| LoomSession | `Store.open`/`open_keyed`/`open_with_kek` | `Store.close` | session root | explicit close, lease expiry |
| SqlSession | `Sql.sql_open*` | `Sql.sql_close` | owner session | explicit close, lease expiry |
| SqlBatch | `Sql.sql_batch_begin*` | `Sql.sql_batch_close` (rolls back if uncommitted) | owner session | explicit close, lease expiry |
| RowIter | `Sql.sql_query` source realization | `Tasks.iter_free` | owner session, or connection-bound | free, session/connection end, stream cancel |
| Task | `*_async` methods | `Tasks.task_free` | owner session | free, take + retention timeout, lease expiry |
| file id (u64) | `FileHandle.open` | `FileHandle.close` | owner session | close, session end; last close on unlinked inode reclaims bytes |
| ResultView | `ResultViews.result_open`/`row_open` | `ResultViews.result_close` | client-local only | not applicable (never a remote handle) |

## 16. Crate And Module Layout

This section settles task 150. It names where the locator, client, protocol, and server code live in the
Cargo workspace, reusing the existing hosted and coordination crates rather than duplicating them. It is a
recommended target layout; crate boundaries are the owner's to redirect before implementation hardens.

Current workspace crates this design builds on: `loom-codec` (canonical CBOR), `loom-core`/`loom-sql`/
`loom-store`/`loom-compute` (the engine), `loom-coordination` ("reusable single-node coordination
contracts for one Loom authority"), `loom-hosted-core` ("shared hosted runtime kernel"), `loom-hosted`
("shared hosted protocol kernel", which owns the served-surface registry and the gRPC adapter),
`loom-protocol-conformance` ("protocol-level conformance runners for hosted and MCP surfaces"),
`loom-cli`, and `loom-mcp`.

New crates (four):

- `loom-locator`: `LoomLocator` parsing (section 3) and the layered context TOML resolver (section 4).
  Modules `locator` and `context`. No engine dependency, so the CLI, MCP, and every binding share one
  resolver. An `context-config` cargo feature gates TOML file reading and is off by default on sandboxed
  platforms (section 10). Homes tasks 180, 190, and the resolution sites for 200.
- `loom-remote-protocol`: the Loom Remote Protocol in code, transport-agnostic. Request, response, and
  error envelopes (section 6), streaming frames (section 7), the discovery document (section 5), the
  canonical CBOR method argument and result codecs built on `loom-codec`, and the stable `Code` error
  mapping. No HTTP and no engine. Homes tasks 160 and 170.
- `loom-client`: the `LoomApi` trait families and the `LoomClient` supertrait (section 14), plus
  `LocalLoomClient` over the engine crates and `loom-locator`. Homes tasks 210 to 240.
- `loom-remote-client`: `RemoteLoomClient`, implementing the same `LoomApi` traits over
  `loom-remote-protocol` and an HTTP/2 over TLS transport. A separate crate so remote-only packages
  (section 10) link the protocol client without the engine. Homes tasks 300 to 320.
- `loom-remote-codegen`: the build tool (DP-2) that parses `idl/loom.idl` and emits the committed method
  registry in `loom-remote-protocol` with a `--check` drift guard. Homes the generated portion of task
  160.

Reused crates (no new server crate, no duplication of hosted adapters):

- Server runtime: extend `loom-hosted-core` with the remote-loom runtime surface and add a `remote`
  served surface to `loom-hosted` (which already carries the served-surface registry and a gRPC adapter).
  Reuse `loom-coordination` for the session, lock, handle, and writer-authority state required by section
  9. A `loom-hosted` `remote` module wires `loom-remote-protocol` dispatch into the hosted kernel. Homes
  tasks 250 to 290 and 330 to 340.
- Conformance: extend `loom-protocol-conformance` with the remote-loom protocol runner, and drive the
  shared IDL behavior suite from `loom-conformance` against both `LocalLoomClient` and `RemoteLoomClient`.
  Homes tasks 420 to 450.
- CLI and MCP: `loom-cli` opens stores through `loom-client` at the migration points cited in section 1;
  `loom-mcp`'s `StoreAccess` facade backs onto a `LoomClient` (local or remote). Homes tasks 350 to 380.

Dependency direction (acyclic): `loom-codec` <- `loom-remote-protocol`; engine crates and `loom-locator`
<- `loom-client`; `loom-remote-protocol` and `loom-locator` <- `loom-remote-client`; `loom-hosted-core`,
`loom-coordination`, and `loom-remote-protocol` <- `loom-hosted` (remote surface); `loom-client` and
`loom-remote-client` <- `loom-cli` and `loom-mcp`. All new crates are BUSL-1.1, use permissively licensed
dependencies only (`deny.toml`), and contain no `unsafe` (only `loom-ffi` may).

Dispatch resolution (the sub-item deferred from section 14). Runtime local-vs-remote selection uses a
generated enum wrapper, not boxed-future dynamic dispatch:

```rust
pub enum Loom { Local(LocalLoomClient), Remote(RemoteLoomClient) }
```

`Loom` implements `LoomClient` by forwarding each method to the active variant. The forwarding impl is
emitted by a small macro from the IDL so it stays DRY (the IDL remains the single source) and zero-cost,
and it sidesteps the object-safety friction of native `async fn` in traits. Store-taking commands hold a
`Loom`. The alternative, a `trait_variant` or boxed `dyn LoomClient`, is rejected because it adds a
per-call heap allocation and indirection on a hot path for no capability gain.

Workspace impact: five new members (`loom-locator`, `loom-remote-protocol`, `loom-remote-client`,
`loom-client`, and the `loom-remote-codegen` build tool) are added to `Cargo.toml`; the server runtime
and conformance extend `loom-hosted-core`, `loom-hosted`, and `loom-protocol-conformance`; no existing
crate is split or renamed.

Decision Points: none.

## 17. Streaming Interchange (Transfer Contract, task 550)

This section is a design/contract draft (owner decision 5). It does not implement the transfer layer; it
defines the remote-capable interchange surface and lists the implementation subtasks.

### 17.1 Boundary

Import/export must be **byte-transfer based**, not path-based, over a remote store:

- The client owns local filesystem paths. For import, the client reads the local
  file/archive/CAR/Arrow/Parquet payload and sends the bytes to the server. For export, the server sends
  bytes to the client, and the client writes the local destination path.
- The server never receives a client-local `src_path` or `dst_path`. The server sees only: bytes, a
  workspace, a transfer `kind`, options, and integrity metadata.
- The existing path-shaped methods (`import_fs`/`export_fs`, `archive_import`/`archive_export`,
  `car_import`/`car_export`, and their `_async` forms) are **server-local/admin compatibility** methods:
  the path is interpreted on the server. They are retained for the shared-filesystem/local-store case and
  are explicitly not part of the remote-capable public interchange contract (section 12 marks each).

### 17.2 Transfer kinds

A transfer's `kind` is a payload format, never a path:

`fs-tree` | `tar-zstd` | `tar` | `tar-gzip` | `zip` | `gzip` | `car` | `arrow-ipc` | `parquet`.

`fs-tree` (a multi-file directory tree) is represented as a self-describing transfer payload/manifest
(for example a canonical tree manifest plus content-addressed entries), never by asking the server to
walk a client-side path. In practice most fs-tree transfers ride an archive kind (`tar-zstd` etc.); a
dedicated `fs-tree` manifest format is specified before that kind is implemented.

### 17.3 Import: handle + chunked writes (v1)

v1 uses the existing unary/session/handle/idempotency patterns and a `Transfer` handle rather than
inventing a reverse-stream (client-to-server) carrier:

```text
transfer_import_open(handle, workspace, kind, opts)            -> TransferId
transfer_import_write(handle, transfer, chunk, seq, digest?)   -> TransferAccept   // { accepted_bytes, credit }
transfer_import_finish(handle, transfer, commit, dry_run, final_digest) -> bytes   // import-report.v1
transfer_import_cancel(handle, transfer)                       -> void
```

- `open` reserves a bounded server-side staging area for `kind` and returns a `TransferId` (a handle
  scoped to the session, released on `finish`/`cancel`/lease-expiry).
- `write` appends one **bounded** chunk at a **monotonic** `seq` (or byte `offset`); an optional
  per-chunk `digest` lets the server reject corruption early. The reply carries the accepted byte count
  and/or remaining `credit` so the client applies backpressure. Re-sending an already-accepted `seq` is a
  no-op (idempotent replay).
- `finish` validates the running digest against `final_digest`, then applies the interchange under the
  single write authority (honouring `commit`/`dry_run`), and returns the canonical
  `loom.interchange.import-report.v1`. Finalize-once: a replayed `finish` returns the same report.
- `cancel` (and lease expiry) discards the staging area and releases the handle.

### 17.4 Export: server-to-client byte stream

Export reuses the section 7 stream contract (server-to-client items + client credit) - no new carrier
direction:

```text
transfer_export(handle, workspace, kind, revision?, opts) -> stream<bytes>
```

- The server streams bounded byte chunks under section 7 credit-based backpressure; either side may
  `cancel`.
- The final `loom.interchange.export-report.v1` and the content digest are delivered in the stream
  **trailer** (section 7 trailers already carry final digest/counts). An `opts` flag MAY instead request
  an explicit `transfer_export_report(transfer)` finish call if a caller cannot read trailers.
- No `dst_path`: the CLI writes the destination path locally from the received bytes.

### 17.5 Integrity, backpressure, cancellation, idempotency

- **Bounded chunks**: a server-advertised max chunk size; oversized chunks are `InvalidArgument`.
- **Ordering**: monotonic `seq` or byte `offset`; gaps/rewinds are rejected.
- **Flow control**: `write` returns accepted bytes / credit; export uses section 7 credit frames.
- **Integrity**: optional per-chunk digest, a running digest maintained server-side, and a required
  `final_digest` checked at `finish` (import) or delivered in the trailer (export). Digest algorithm is
  the store's `digest_algo`.
- **Final report**: canonical `loom.interchange.import-report.v1` / `export-report.v1`.
- **Cancel/expiry**: `cancel` and session/transfer lease expiry release staging buffers and handles.
- **Idempotency (section 6)**: `open` is `key` (mints a transfer; safe to retry), `write` is idempotent
  per `(transfer, seq)`, `finish` is `key` (finalize-once). Generated clients auto-attach keys for the
  `key` methods (the codegen `requires_idempotency_key` list).

### 17.6 Implementation subtasks (not part of this design task)

1. **551 - IDL + codegen**: add the `Transfer` interface (import handle methods + `transfer_export`
   stream) and the `TransferId`/`TransferAccept` types; classify `open`/`finish` as `key` in section 12
   and `requires_idempotency_key`; regenerate; keep the path-shaped methods but mark them local/admin.
2. **552 - server staging + engine seam**: a bounded, cancellable, lease-expiring staging buffer keyed by
   `TransferId`; running-digest accumulation; wire the existing interchange (fs/archive/car/columnar
   arrow-ipc/parquet) encode/decode to operate on bytes instead of a server path.
3. **553 - `LocalLoomClient` + dispatch**: implement the five methods over the staging seam and the
   engine interchange; return the canonical reports.
4. **554 - CLI facade + routing**: `loom` import/export commands read/write the **local** path and drive
   `transfer_import_*` / `transfer_export`; keep the path-shaped commands as an explicit local/admin
   surface (reject remote for those, as today).
5. **555 - live parity + conformance**: local-vs-remote byte/report parity per kind, backpressure and
   cancel behaviour, bad-digest rejection, and finalize-once/idempotent-replay tests.

Decision Points: the `fs-tree` manifest format (17.2) and whether export delivers the report via trailer
only or also an explicit `transfer_export_report` call (17.4) are settled in subtask 551 before coding.

## 18. Vector Text-Embedding Placement (task 650)

Raw Vector operations (`create`, `upsert`, `upsert_source`, `get`, `search`, `search_policy`,
`embedding_model`, index/id/metadata methods) are already remote-capable on the Vector interface (§12):
they carry pre-computed vectors as bytes and never run inference. The `vector text` / `vector workspace`
CLI commands were the only vector holdouts, and only because they perform **local text-to-vector
inference** (a `loom-inference` text-embedding handle over the local model cache and hardware) before
calling the Vector engine. Inference placement - not vector storage/search - is what made them local.

There are two placement modes, and the same **model-selection rule** applies to both: whichever side
computes the embedding MUST declare the exact embedding model identity and weights digest used, so the
stored `source_text`, `model_id`, and `weights_digest` remain auditable (carried on `upsert_source`).

### 18.1 Client-embed (implemented, task 650)

The client owns local inference. For a remote store it: (1) obtains the embedding model identity -
either from the collection's recorded model via the existing remote `Vector.embedding_model` read
(remote read), or from an explicit client-side selection for a new collection - (2) embeds the text
locally with its own model cache/hardware (local embed), and (3) sends the computed vector plus
`source_text` / `model_id` / `weights_digest` through the already-remote `Vector.upsert_source` (or the
query vector through `Vector.search`) (remote upsert/search). The server never infers and never guesses
an embedding; it only stores/searches the vector the client computed. The remote client reads local
inference caches **only** in this explicitly-selected client-embed mode. No new IDL is required - the
flow is entirely `embedding_model` + `upsert_source` + `search` on the existing Vector surface. No local
filesystem path is ever sent to the server.

### 18.2 Server-embed (deferred - see §19.1)

Server-embed - where the served endpoint owns the inference runtime/provider and embeds text
server-side - is **NOT supported yet and MUST NOT be claimed until a real remotely-administered
workspace-to-embedding-provider binding exists.** It requires, as deferred work (§19.1): a new embed-from-text /
provider-binding IDL surface, a remotely-administrable workspace-to-provider binding (authorized + audited),
a server-side inference runtime/provider policy, a deterministic test provider, and local-vs-remote
parity + auth/audit tests. Until that lands, a remote endpoint has no server-side embedding: `vector
text` over remote uses client-embed (§18.1), and any request that would require server inference returns
a precise unsupported error rather than silently embedding on the server.

### 18.3 Classification

`vector text` / `vector workspace` are reclassified: **vector storage/search is remote-capable today**
(raw Vector interface); only the **embedding step is local** in client-embed mode (by design - the
client owns the model). They are therefore not "local-only holdouts" but "client-embed over the remote
Vector surface; server-embed deferred."

## 19. Deferred Follow-Up Work

Design-settled but not yet implemented. These were tracked as Queue 11 tasks 710 and 700 and are
recorded here so the queue can close; each carries the scope and acceptance criteria it needs when
picked up. Neither is required for the current remote surface to be correct - client-embed (§18.1)
and the existing binding stubs cover today's contract.

### 19.1 Server-embed for vector text (workspace-to-embedding-provider binding)

Realize the server-embed mode deferred in §18.2 so a served endpoint can embed text server-side and a
thin client with no local model can send raw text. Server-side inference **MUST NOT be claimed as
supported until this exists in full.** Scope:

1. A new embed-from-text / provider-binding IDL surface. The current Vector interface carries
   pre-computed vectors only; server-embed needs an explicit `embed_text` / binding contract, with
   regenerated codegen and idempotency classification.
2. A remotely-administrable workspace-to-embedding-provider binding that is authorized (fail-closed) and
   audited (acting principal), analogous to StoreAdmin (§13). The binding - not client caches -
   selects the model.
3. A server-side inference runtime/provider policy (which providers/models the endpoint offers), with
   model identity + weights digest recorded per the §18 model-selection rule.
4. A deterministic test provider so parity is reproducible without shipping real model weights.
5. Precise unsupported errors when a workspace has no provider binding - the server never silently
   embeds and never guesses.

Acceptance: local-vs-remote parity for server-embed with the deterministic test provider (byte/report
parity for recorded model metadata + stored vectors); auth/audit tests for the provider binding
(unauthorized bind rejected fail-closed; audit actor recorded); an endpoint-without-provider request
returns a clear unsupported error; the §18 model-selection rule (model id + weights digest declared
server-side) is asserted.

### 19.2 Remote-only browser/mobile package shapes

Move the binding/package policy (§16 and the target matrix from the binding tasks) from documentation
into real, buildable artifacts for browser, iOS, Android, and React Native. Scope:

1. Remote-only builds must not link the local engine, the local filesystem store, the inference
   runtime, or context-TOML loading. A remote-only client links only the remote transport + client
   abstraction.
2. Combined (local + remote) builds keep local and remote behind one client abstraction so callers do
   not branch on locator kind.
3. Host-provided URL / auth / context-resolver paths stay explicit rather than auto-discovered from
   local config.

Acceptance: package/build checks proving remote-only artifacts do not depend on `loom-store` / local
engine or context-TOML features; smoke constructor tests (an explicit remote URL works; local operations
error cleanly in a local-disabled build); a size/dependency report for the mobile/browser packages
demonstrating the slimming.
