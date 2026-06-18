# Webish - Workspace Web Server and Application Runtime

**Status:** Target design. **Version:** 0.1.0-target.
**Capability:** `web`.

This document defines a Loom-hosted web server and application runtime. It inverts the Studio specs:
instead of Loom acting as the backing store for a product like chat, drive, issues, or pages, Loom serves
HTTP traffic directly from workspace files and lets those files invoke Loom-backed processing and data
access through a constrained presentation-marker and program model.

Webish is not only a static file server. It combines:

- nginx-like HTTP access control, TLS, routing, caching, compression, and logs;
- Spring Boot-like layering: presentation files, processing programs, and data facets;
- Loom-native content addressing, workspace isolation, sync, history, and policy enforcement.

## 1. Contract Boundaries

The design builds on these contracts:

- `0003-core-interface.md` defines the filesystem facade and path operations over workspace working
  trees.
- `0008-wire-protocols.md` defines hosted HTTP, authentication, authorization, caching, conditional
  requests, and protocol error mapping.
- `0015-execution-and-logic.md` defines the target execution boundary: deterministic metered programs,
  workspace-aware grants, and multi-facet `StateAccess`.
- `0020-document-layer.md`, `0021-append-log-layer.md`, and the other facet specs define data surfaces a
  Webish application may read or write through granted programs.
- `0035-durable-delivery.md` defines the durable delivery substrate for WebSocket, SSE, MCP, watch, and
  other push streams. Webish uses it for management streams, log tails, deployment updates, and any
  application push surface that must survive reconnect.
- `0031-end-to-end-encrypted-sync.md` defines the key-holding distinction. A blind remote can store and
  relay encrypted Webish content, but it cannot render presentations or execute content-aware routes.
- `0061.md` defines the shared operation substrate: operation envelope, sequencer, durable cursors,
  conflict records, annotations/attachments/watches (§7/§7.1), entity versioning, and
  projection/view machinery. Webish configuration changes (listener, route, hook) are operations in
  the 0061 envelope; the compiled/rendered cache is 0061 §8 derived state; management subscriptions
  use the 0061 §7.1 watch registry; durable cursors follow 0061 §12 / 0035. Where this document's
  local text differs, 0061 supersedes it (see §25).
- `SURFACES.md` defines the human experience layer (MCP Apps, elicitation flows, visualizations); Webish
  management UIs render over the same projections and elicitation contract.

This document depends on those boundaries. It does not make arbitrary template execution safe, does not
grant ambient access to all workspaces, and does not replace the served protocol contract in `0008`.
The recommended authoring model is Loom Templates: HTML remains mostly normal HTML, while
Jinja-compatible template calls trigger content-addressed programs that produce typed data or response
models.

## 2. Product Model

A Webish deployment maps ports and hostnames to Loom workspaces:

```text
web
  listeners
  virtual-hosts
  workspace-mounts
  routes
  presentation
  programs
  compiled-cache
  sessions
  access-logs
  error-logs
  metrics
  tls
  policy
```

Example:

```text
loom web serve --port 8080 --workspace marketing-site
loom web serve --port 8443 --workspace customer-portal --root /public --tls cert://portal
loom web serve --bind 127.0.0.1 --port 3000 --workspace dev-dashboard --ref main
loom web serve --port 9090 --workspace docs --ref blake3:... --root /site
```

The user may create as many listener mappings as the host, operating system, and policy allow. Each
listener maps to exactly one default workspace, with optional host and path routing to subtrees or other
workspaces.

## 3. Listener and Workspace Binding

The listener binding is durable configuration unless explicitly started as an ephemeral development
server.

```text
listener:
  listener_id
  bind_address
  port
  protocol http | https
  tls_profile optional
  default_workspace
  default_ref
  root_path
  virtual_hosts
  route_table
  hook_chain
  principal_policy
  cache_policy
  log_policy
```

Rules:

- A port can be bound by only one active listener on a host address.
- A listener can serve one workspace directly or route by `Host` and path prefix to multiple workspace
  roots.
- A listener may serve a branch, tag, or commit through `default_ref` or route-specific refs. Commit mounts
  are immutable and cacheable. Branch mounts are live and require cache invalidation on ref advance.
- `root_path` defaults to `/` and is configurable per listener and per route. A listener with
  `--root /public` serves `/public/index.html` for HTTP `/`.
- Path traversal outside the configured root path is invalid.
- Hidden Loom metadata paths are not served unless an explicit route grants them.

Current source backs the Webish configuration model in `loom-substrate::web`: canonical listener
configuration, route tables, mount refs, hook chains, hook phase ordering, path traversal rejection,
HTTPS TLS-profile requirement, default static-file routing, and host/method/longest-prefix route
resolution. This is the reusable configuration substrate. `loom serve configure <store> web
<workspace> --transport rest` is source-backed as a durable daemon-opened static-file listener over
namespace files: `GET /` resolves `index.html`, `GET /path/` resolves `path/index.html`, `GET /path`
tries the exact file and then `path.html`, `HEAD` omits the body, common content types are emitted,
and hidden `.loom` paths are not served. The hosted Webish REST router can also serve a
caller-provided `WebListener` route table, including host-specific longest-prefix dispatch to a
different workspace/root, static route directory-index resolution, `.html` fallback, and fail-closed
rejection of unsupported route modes. Durable static-route management is source-backed for stored
`web/rest` listeners through `loom serve route list/set/remove`; the daemon loads the persisted
route table at listener startup. Hosted admin REST and JSON-RPC expose equivalent static route
list/set/remove operations over the same control-plane record. Loom Templates execution, program
hooks, full 0061 operation-envelope writes for route and hook lifecycle, cache workers, TLS
certificate providers, MCP Webish management tools/resources, and broader Webish protocol
conformance remain target work.

## 4. HTTP and TLS Requirements

Webish MUST support HTTP 1.x. Webish SHOULD support HTTP 2.x. HTTP 3 is a later optional capability.

Required HTTP behavior:

- `GET`, `HEAD`, `POST`, `PUT`, `PATCH`, `DELETE`, and `OPTIONS`;
- byte range reads for static files;
- conditional requests with `ETag`, `If-None-Match`, and `If-Match`;
- correct `Content-Type` selection by explicit metadata or extension;
- `Cache-Control`, `Vary`, and content negotiation;
- request and response streaming;
- request size limits;
- connection keep-alive;
- graceful shutdown and drain;
- structured access and error logs;
- stable Loom error codes in generated error responses.

TLS configuration:

- A listener may be plain HTTP or HTTPS.
- HTTPS listeners require a TLS profile with certificate source, private key source, supported protocol
  versions, cipher policy, and renewal policy.
- Server Name Indication selects certificates for virtual hosts.
- mTLS is allowed for service-to-service deployments.
- ACME automation is a deployment feature, not a storage-layer requirement.

## 5. Async Runtime Model

Webish is async in the same operational sense as Node.js or a modern evented server. It must allow many
listeners and many idle connections without assigning one blocking thread to each connection.

The target runtime:

- one or more event loops accept and poll sockets;
- request bodies and response bodies stream with backpressure;
- static file reads stream from Loom chunks;
- presentation rendering is async at I/O boundaries;
- program execution is metered and scheduled separately from socket polling;
- slow clients do not block unrelated requests;
- per-listener and global concurrency limits protect the host.

The async contract is a server implementation requirement. It does not change Loom object identity or
facet semantics.

## 6. Files as Presentation Layer

The workspace file tree is the presentation layer. The root path defaults to `/`, but every listener and
route may choose a different root path.

```text
/
  index.html
  about.html
  blog/
    index.html
    post.html
  assets/
    app.css
    app.js
  .loom/
    facets/
    web/
      routes.cbor
      cache/
      manifests/
```

Files look like normal HTML, CSS, JavaScript, images, and downloadable assets. HTML files may contain
Loom Template calls. Static files with no template calls are served directly.

Default resolution:

- `/` maps to `<root_path>/index.html`;
- `/path/` maps to `<root_path>/path/index.html`;
- `/path` maps to `<root_path>/path` if it exists, otherwise may map to `<root_path>/path.html` by route
  policy;
- missing paths return `404` or a configured fallback;
- directory listing is disabled by default.

If a listener serves a commit digest, all file lookup, presentation parsing, program manifest lookup, and
cache-key computation are evaluated against that commit. Serving a commit is the strongest static-site
mode because content and cache validators are immutable. Serving a branch or tag is live-site mode and
requires invalidation when the ref advances.

## 7. Loom Templates

Webish consumes Loom Templates rather than defining its own template language. The reusable template
contract is `0044-loom-templates.md` and the target crate is `crates/loom-templates`.

The v1 presentation model is Jinja-compatible. Templates may call Loom host functions. Programs do the
processing and return typed data, rendered fragments, redirects, or full HTTP responses.

The Loom-specific surface stays small:

```html
<section>
  {{- loom.program(name="dashboard/load") -}}
</section>
```

Jinja-compatible control flow and composition use the Loom Templates syntax:

```text
{{ expression }}
{% if condition %}...{% endif %}
{% for item in items %}...{% endfor %}
{% include "partial.html" %}
{% extends "layout.html" %}
{% block content %}...{% endblock %}
{% set name = expression %}
{{ loom.program(name="program/name") }}
```

The template plan models parsed files and the safe host functions they may invoke:

```idl
struct WebPresentation {
  presentation_id: Digest
  source_path: string
  source_digest: Digest
  mode: ProgramFirst | TemplateProfile
  engine_profile: string
  ast_digest: Digest
  dependencies: List<WebDependency>
}

struct WebDependency {
  kind: TemplateFile | ProgramManifest | QueryManifest | StaticAsset
  target: string
  digest: Option<Digest>
}

struct WebRequestContext {
  method: string
  path: string
  query: Map<string, string>
  headers: Map<string, string>
  principal: Option<PrincipalId>
  route_params: Map<string, string>
  request_id: string
}

struct WebHostCall {
  kind: Program | Query | ReadResource
  target: string
  args: bytes
  grant_scope: string
}
```

Presentation rules:

- files with Loom Template calls are parsed before execution;
- unknown `loom.*` functions are rejected unless a route explicitly enables an extension;
- all host calls require grants;
- marker expansion, template rendering, and program calls have fuel, memory, recursion, include-depth, and
  output-size limits;
- presentation code cannot access wall clock, randomness, environment variables, files outside the workspace
  root, or network sockets unless exposed by a granted program;
- generated output is a representation, not a Loom object, unless explicitly stored.

Loom Templates treats HTML as the view, programs as controllers and services, and Loom facets as the data
layer. Webish owns routing, listeners, hooks, and HTTP semantics around that template processor.

## 8. Compiled and Cached Artifacts

Webish may store compiled presentation plans, dependency graphs, rendered fragments, and route plans in a hidden
area under the workspace:

```text
/.loom/web/cache/
/.loom/web/manifests/
/.loom/web/routes/
```

The hidden cache is derived state. It must be safe to delete and rebuild.

Cache keys include:

```text
source_digest
presentation_mode
presentation_engine_profile
presentation_idl_version
route_config_digest
program_manifest_digests
query_manifest_digests
grants_profile_digest
render_options
```

Invalidation occurs when:

- the source file changes;
- an included file changes;
- a program manifest changes;
- route configuration changes;
- a grant profile changes;
- the presentation engine version changes;
- a cache TTL expires;
- an administrator purges the cache.

Rendered whole-page caches are allowed only when the route declares cache safety. Personalized responses
must vary by principal and relevant request attributes.

## 9. Routing

Routes are explicit data, not hard-coded server behavior.

```text
route:
  route_id
  methods
  host_pattern
  path_pattern
  workspace
  root_path
  ref optional
  presentation_path optional
  static_path optional
  program optional
  hook_chain optional
  auth_policy
  cache_policy
  rate_limit_policy
  timeout_ms
```

Routing modes:

- static file route;
- presentation route;
- program route;
- reverse proxy route;
- redirect route;
- error route.

Reverse proxy is optional and must be capability-gated. It is useful for nginx-like deployments but is
not required for Loom-native applications.

## 10. Request Lifecycle and Program Hooks

Programs are not limited to markers inside HTML. A route or listener may bind ordered programs into the
request lifecycle. These programs are connection and request processors, analogous to nginx phases,
Spring filters, and interceptors.

Lifecycle phases:

```text
accept
tls
early-request
normalize
pre-route
route
authenticate
authorize
variant-select
pre-handler
handler
post-handler
error
log
delivery
```

Hook binding:

```text
hook:
  hook_id
  phase
  order
  program
  grants
  match optional
  timeout_ms
  failure_policy fail-closed | continue | redirect
```

Rules:

- hooks run in ascending `order` within a phase;
- every hook program is content-addressed by manifest digest;
- every hook has explicit grants;
- hooks receive a bounded `WebRequestContext` and may return a typed action;
- hooks cannot read request bodies unless their phase and route allow it;
- hooks can short-circuit by returning a response, redirect, reject, route override, ref override, or
  principal update;
- hooks are metered and cannot block socket polling.

Examples:

```text
A/B testing:
  phase: variant-select
  program: traffic/split-by-cookie
  behavior: read cookie `experiment`, choose commit A or commit B, set route ref override

Authentication:
  phase: authenticate
  program: security/read-session-cookie
  behavior: validate auth cookie, attach principal, or redirect to /login

Authorization:
  phase: authorize
  program: security/check-route-grants
  behavior: allow, deny, mask as 404, or redirect to an access request page

Maintenance:
  phase: pre-handler
  program: ops/maintenance-mode
  behavior: return 503 for non-admin principals while deploy flag is set
```

The A/B testing example implies that the selected file ref is a request-time decision. Cache keys must
therefore include the selected ref or commit digest, the hook-chain digest, and any declared variance such
as experiment cookie, principal segment, or locale.

The authentication and authorization examples imply that route security is layered. Edge policy may reject
obvious requests early, but final permission decisions still go through Loom principal and grant
enforcement.

## 11. Processing Layer

The processing layer is made of content-addressed programs and query manifests.

Programs:

- are addressed by manifest digest;
- run under explicit workspace-aware grants;
- are metered;
- can read and write Loom facets only through `StateAccess`;
- return typed values, rendered fragments, redirects, or HTTP responses;
- can be run in dry-run, direct, or batched mode only when those execution modes are promoted.

Processing examples:

```text
GET /dashboard
  read dashboard.html
  call program dashboard/load
  inject returned model into declared Loom Template calls
  read SQL tables and vector indexes through grants

POST /contact
  validate request body
  call program contact/submit
  append to queue and send redirect

GET /api/items
  call program api/items
  return application/json
```

Programs must not inherit the full server authority. They run as the resolved web principal, a route
service principal, or the intersection of both, according to route policy.

Program-first processing is the default because it keeps application logic in programs:

- AI agents can author and test programs using normal language tooling;
- program manifests are content-addressed and grant-scoped;
- template interpretation stays small;
- data access is centralized in `StateAccess`;
- HTTP behavior is easier to audit and meter.

## 12. Data Layer

The data layer is Loom itself. A Webish application may read or write any promoted facet for which it has
grants:

```text
files
sql
kv
document
graph
vector
columnar
queue
time-series
cas
ledger
program
```

Data access patterns:

- presentation markers may read small values only through declared host calls;
- programs perform non-trivial reads and writes;
- long-running tasks enqueue work and return accepted responses;
- derived views are maintained by background workers or triggers;
- direct writes from presentation markers are discouraged except for explicit safe helpers.

This mirrors a Spring Boot tiered structure:

```text
Presentation layer:
  workspace files, Loom Templates, static assets, route declarations

Processing layer:
  programs, guards, validators, controllers, background jobs

Data layer:
  Loom facets, object store, indexes, refs, sync, audit
```

## 13. Nginx-Inspired Access and Edge Features

Webish should provide nginx-like request handling features without making them the security boundary.

Target features:

- virtual hosts;
- path routing;
- static file serving;
- TLS termination;
- mTLS;
- redirects and rewrites;
- ordered request phases and hook chains;
- request body limits;
- rate limiting;
- connection limits;
- gzip or brotli compression where configured;
- sendfile-like zero-copy where a backend supports it;
- reverse proxy routes;
- upstream health checks for reverse proxy routes;
- access logs and error logs;
- custom error pages;
- maintenance mode;
- security headers;
- CORS policy;
- content security policy;
- directory listing controls.

Authorization still belongs to Loom principal and grant enforcement. Edge routing may reject requests
early, but it does not grant access.

## 14. Spring-Boot-Inspired Application Structure

Webish applications should have a conventional layout:

```text
/
  app/
    routes.web.cbor
    hooks.web.cbor
    programs/
  presentation/
    controllers/
    policies/
  public/
    index.html
    assets/
  data/
    seed/
  .loom/web/
    cache/
    manifests/
```

The layout is a convention, not a required path shape. A route table decides what is public.

Spring Boot analogues:

- controller: Webish program bound to a route;
- model: typed values returned from programs and queries;
- view: Webish presentation file;
- service: program or module with reusable processing logic;
- repository: Loom facet access through grants;
- filter: route policy, authentication, CORS, rate limit, request transforms, and hook programs;
- actuator: metrics, health, route inventory, cache status, and dependency state.

## 15. Sessions, Forms, and APIs

Webish must support both human web pages and programmatic APIs.

Sessions:

- session storage may use signed cookies, encrypted cookies, or a Loom-backed session store;
- session secrets are deployment credentials, not presentation variables;
- CSRF protection is required for browser form writes;
- same-site cookie policy must be configurable;
- logout and session revocation must be auditable when authentication is enabled.

Forms:

- form routes declare accepted fields, size limits, and validation programs;
- file uploads stream into Loom CAS before final route processing;
- idempotency keys are required for retryable writes.

APIs:

- JSON responses are first-class;
- content negotiation can select HTML or JSON;
- stable Loom error codes appear in structured API errors;
- long-running operations return `202 Accepted` with a status resource.

## 16. Observability

Webish must emit structured operational data:

```text
request_id
listener_id
route_id
workspace
selected_ref
principal optional
method
path
status
bytes_in
bytes_out
duration_ms
cache_status
program_digests
hook_digests
error_code optional
```

Metrics:

- open connections;
- idle connections;
- requests per route;
- requests per selected ref;
- response latency;
- presentation compile time;
- render time;
- program execution time;
- cache hit ratio;
- error counts;
- TLS handshake failures;
- rejected requests by policy.

Logs may be written to Loom append logs, host logs, or both. Privacy policy decides whether request paths,
principals, and headers are stored in plaintext.

## 17. Security Model

Webish has a larger attack surface than ordinary local Loom use.

Required controls:

- served write paths require authenticated principal context unless deployment-confined owner mode is
  explicitly selected;
- presentation markers and optional templates are sandboxed and metered;
- programs are sandboxed and metered;
- hook programs are sandboxed, metered, ordered, and grant-scoped;
- route grants are least privilege;
- request bodies have size and time limits;
- response generation has output limits;
- upload content is quarantined until accepted;
- hidden Loom paths are denied by default;
- secrets are never exposed as presentation variables by default;
- SSRF-sensitive reverse proxy routes are disabled unless explicitly configured;
- error responses do not leak hidden paths, grant details, or stack traces in production mode.

Blind Loom Cloud cannot render presentations, execute programs, or serve decrypted content. A public Webish
server that serves plaintext must run on a key-holding local replica or keyed remote.

## 18. Background Workers

Required workers:

- **Listener worker:** owns sockets, TLS, protocol negotiation, and graceful shutdown.
- **Route worker:** watches route and listener configuration and updates dispatch tables.
- **Hook worker:** watches hook-chain configuration and validates hook ordering and grants.
- **Compile worker:** parses presentation files and maintains compiled cache artifacts.
- **Render worker:** renders presentation files and fragments where keys and grants allow it.
- **Program worker:** schedules metered program execution.
- **Cache worker:** invalidates and evicts compiled and rendered cache entries.
- **Asset worker:** computes MIME metadata, ETags, compression variants, and static asset indexes.
- **Log worker:** writes access, error, and audit records.
- **Sync worker:** syncs workspace content and route configuration to remotes.
- **Notification worker:** emits MCP, SSE, or WebSocket updates for route state and deployment changes
  through the durable delivery model from `0035`.

## 19. MCP Control Plane

MCP is the agent-native control plane for managing Webish deployments.

Resources:

```text
loom://{workspace}/web/listener/{listener_id}
loom://{workspace}/web/routes
loom://{workspace}/web/hooks
loom://{workspace}/web/cache
loom://{workspace}/web/logs
loom://{workspace}/web/health
```

Tools:

```text
web.serve
web.stop
web.list_listeners
web.configure_tls
web.add_route
web.remove_route
web.add_hook
web.remove_hook
web.reload
web.purge_cache
web.get_health
web.tail_logs
web.run_request_test
web.update_cursor
```

Example:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "web.serve",
    "arguments": {
      "port": 8080,
      "workspace": "marketing-site",
      "root_path": "/public",
      "protocol": "http"
    }
  }
}
```

Agents may subscribe to listener health, route changes, hook-chain changes, deployment events, or log
streams. Subscriptions that require reconnect-safe delivery use `0035` delivery cursors and
acknowledgements.

## 20. Elicitation

MCP elicitation is used when a web management agent needs structured approval.

Use elicitation for:

- approving exposure of a listener on a public interface;
- choosing whether to enable TLS;
- approving a route that invokes a program with write grants;
- approving a hook program in the authentication or authorization phase;
- approving a variant-selection hook that serves different commits to different users;
- selecting a certificate source;
- confirming cache purge;
- approving reverse proxy access to an upstream;
- deciding whether to roll back a route change.

Example:

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "elicitation/create",
  "params": {
    "message": "Expose workspace marketing-site on 0.0.0.0:443 with TLS?",
    "requestedSchema": {
      "type": "object",
      "properties": {
        "approve": { "type": "boolean" },
        "tls_profile": { "type": "string" }
      },
      "required": ["approve"]
    }
  }
}
```

Elicitation responses become durable configuration operations when they change listener or route state.

## 21. Performance Requirements

The design must meet these requirements:

- idle keep-alive connections do not consume one blocking thread each;
- many listeners can sit idle concurrently;
- static files stream from chunks without full buffering;
- large uploads stream into CAS;
- presentation compilation is cached by source and dependency digests;
- rendered output caching respects principal and request variance;
- route dispatch is index-backed and does not scan the workspace tree per request;
- hook chains are precompiled and ordered before traffic reaches them;
- conditional requests avoid unnecessary reads and rendering;
- TLS handshakes and slow clients are bounded by policy;
- program execution cannot starve socket polling.

## 22. Open Design Decisions

These choices must be pinned before implementation:

1. The exact program-first marker syntax and parser conformance fixtures.
2. The Jinja-compatible syntax and parser conformance fixtures.
3. The presentation AST IDL and canonical compiled artifact format.
4. The route table canonical encoding.
5. The hook-chain canonical encoding and phase list.
6. The listener configuration storage path and lifecycle.
7. The grant intersection rule for request principal, route service principal, hook program, and program
   manifest.
8. The response cache key schema, including selected commit, hook-chain digest, and declared variance.
9. The TLS profile and certificate-provider model.
10. The reverse proxy capability and SSRF policy.
11. The HTTP 2.x implementation profile.
12. The delivery-stream mapping for WebSocket, SSE, MCP, and log tails under `0035`.
13. The conformance vector set for routing, hook phases, presentation markers, cache invalidation, program calls, TLS config, and
    error mapping.

## 23. Long-Term Contract Shape

All contracts in this document are long-term decisions; the shape below is implementation staging
only and never changes a contract (no-v1 principle; converted from "Recommended v1" 2026-07-05,
SLACKISH session, per the queue's supersession-rewrite rule):

- support `loom web serve --port X --workspace Y` for many available ports and workspaces;
- support configurable `--root`, defaulting to `/`;
- support serving a branch, tag, or chosen commit through `--ref`;
- support HTTP 1.x first and design the protocol layer so HTTP 2.x can be added without changing route
  semantics;
- support TLS listeners through explicit TLS profiles;
- serve static files from workspace files;
- support Loom Templates with Jinja-compatible syntax and `loom.program(...)` calls;
- store compiled presentation and route cache artifacts under `/.loom/web/cache`;
- use programs for processing and Loom facets for data access;
- support ordered hook programs for request phases such as authentication, authorization, and variant
  selection;
- require grants for every Webish host call and program execution;
- use `0035` durable delivery for reconnect-safe MCP, SSE, WebSocket, deployment, and log streams;
- provide nginx-like edge features as configuration, not as the authorization boundary;
- provide Spring Boot-like application layering through conventions, route tables, and program manifests;
- expose management through MCP tools and resources.

## 24. Example Tool Surface (illustrative only - not a design decision)

Names, grouping, and parameters are examples to make assistant ergonomics concrete, not designed
contracts. Underscore-flattened per MCP; capability `web`.

| Category | Tool | Description |
| --- | --- | --- |
| Listeners | `web.serve` | Bind a listener: port, workspace, root, ref, protocol |
| Listeners | `web.stop` | Stop a listener; graceful drain |
| Listeners | `web.list_listeners` | Enumerate listeners with binding and health state |
| TLS | `web.configure_tls` | Attach or update a TLS profile on a listener |
| Routes | `web.add_route` | Add a route (sequenced configuration operation, 0061 §2) |
| Routes | `web.remove_route` | Remove a route (sequenced configuration operation) |
| Hooks | `web.add_hook` | Bind an ordered, grant-scoped hook program to a phase |
| Hooks | `web.remove_hook` | Unbind a hook program |
| Deploy | `web.reload` | Re-read configuration and rebuild dispatch tables |
| Cache | `web.purge_cache` | Purge compiled/rendered cache (derived state, 0061 §8) |
| Observability | `web.get_health` | Listener, route, and dependency health |
| Observability | `web.tail_logs` | Stream access/error logs via 0035 durable delivery |
| Testing | `web.run_request_test` | Execute a synthetic request against a route without traffic |
| Cursors | `web.update_cursor` | Advance the principal's durable cursor (0061 §12 / 0035) |

## 25. Unfinished Tasks (pushed back from Queue 8)

`specs/0061.md` now owns the shared substrate. Webish, unlike the four collaboration profiles,
never defined its own operation log, but it restates substrate machinery in places, and those
restatements now resolve to 0061:

- **Configuration operations.** "Durable configuration operations" (§20) and durable listener,
  route, and hook state (§3, §9, §10) get their operation shape from the 0061 §2 envelope and are
  sequenced per 0061 §3; open decisions 4, 5, and 6 in §22 keep only the payload encodings and
  lifecycle rules, not the envelope or sequencing.
- **Compiled and cached artifacts.** The §8 cache (deterministic derived artifacts keyed by source
  and dependency digests, safe to delete and rebuild) is an instance of 0061 §8
  projection/view machinery; the Webish-specific part is the cache key schema (§22 decision 8),
  not the derived-state model.
- **Management subscriptions.** Agent subscriptions to listener health, route changes, hook-chain
  changes, deployment events, and log streams (§19) use the 0061 §7.1 watch registry and the
  0035 cursor contract via 0061 §12; the cursor/replay portion of §22 decision 12 resolves there,
  leaving only the Webish stream-to-surface mapping.
- **Uploads.** Form file uploads streaming into CAS with quarantine and scan status (§15, §17)
  follow the 0061 §7.1 attachments model.
- **Raw-facet history.** Workspace commits are Webish's serving refs, consistent with 0061 §9:
  commits are storage/sync identity, and `substrate_changes` is the history bridge for the raw
  facet trees Webish serves.

The following remains uniquely Webish and is unowned by any queue:

- Listener, TLS/SNI/mTLS, and certificate-provider model (§3, §4, §22 decisions 6, 9).
- HTTP semantics, async runtime, and performance requirements (§4, §5, §21, §22 decision 11).
- Route table and hook-chain canonical encodings, phase list, and routing modes (§9, §10, §22
  decisions 4, 5).
- The grant intersection rule for request principal, route service principal, hook program, and
  program manifest (§22 decision 7).
- Loom Templates consumption profile, including marker syntax, AST IDL, and conformance fixtures, shared with
  `0044-loom-templates.md` (§7, §22 decisions 1-3).
- Response cache key schema including selected commit, hook-chain digest, and declared variance
  (§8, §22 decision 8).
- Sessions, CSRF, forms, and API conventions (§15).
- Reverse proxy capability and SSRF policy (§13, §22 decision 10).
- Conformance vectors for routing, hook phases, presentation markers, cache invalidation, program
  calls, TLS config, and error mapping (§22 decision 13).
