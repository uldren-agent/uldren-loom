# 0043 - MCP Serving Surface and the Inference Capability

**Status:** Draft. **Version:** 0.1.17.
**Capabilities:** `mcp-host` (0008), `mcp-apps` (this spec), `inference` (this spec).

This spec records how Loom maps onto the Model Context Protocol (MCP) serving surface, and draws the
line between an MCP *wire mechanic* and a Loom *capability*. It complements 0008 section 9 (which lists
the served primitives) by stating, per primitive, what Loom actually exposes and what richer uses are
planned. The host implementation lives in `loom-mcp` (rmcp/tokio); the data and capabilities it serves
live in `loom-core`.

## 1. Framing: surfaces vs capabilities

A useful distinction the first pass blurred:

- A **surface mechanic** is an MCP transport-level feature whose meaning is local to the protocol -
  pagination, completion, progress, cancellation, ping. These belong in `loom-mcp`.
- A **capability** is a Loom behavior that has value independent of MCP and should be reachable from
  every surface (MCP, CLI, bindings, programs, triggers, scheduled tasks). These belong in
  `loom-core`, with MCP as one projection.

Two MCP primitives are really capabilities in disguise and must not be modeled as MCP-only:

1. **Inference**: a request for an LLM completion. The completion backend is swappable and the
   *consumers* are many. Deprecated MCP sampling is not a Loom implementation target.
2. **Change feeds** (MCP "subscriptions" + "list-changed"): a live notification that a watched
   resource changed. The watchable thing is a Loom object (a branch head, a queue stream, a table, a
   calendar collection); MCP push is one delivery transport. 0030 owns data-change `DataChange`
   payloads, each watched domain owns its `DomainChange` records, and MCP lifecycle notifications keep
   their own lifecycle payloads. See section 2, row "Subscriptions".

## 2. Per-primitive surface

Default use is what is wired today (0008 section 9). Target uses are the richer mappings; each is a
proposal until promoted by its own task. Priorities here are design priorities, not an execution
roadmap.

| Primitive | Kind | Wired today | Design priority | Target Loom use |
| --- | --- | --- | --- | --- |
| Initialize identity | surface | `loom-mcp` reports implementation name `loom`, title `Loom MCP`, and the Loom package version during MCP initialization instead of exposing rmcp's build-environment default crate name. | (P0) | Keep host identity stable across stdio, Streamable HTTP, and future hosted MCP listeners. |
| Tools | surface | IDL-area projection of facet ops through the PEP. `tools/list` derives regular tool visibility from the current bound principal and live ACL state and returns complete callable schemas for visible tools, while every call still crosses the engine PEP for argument-scoped enforcement. | (P0) | Stable agent action surface. New promoted facets project automatically through curated tools, while MCP clients choose how much of the complete inventory to inject into model context. |
| Session/auth | surface | Local daemon-backed MCP can bind a launch-time passphrase principal and reattach it on every per-request open; missing launch auth fails as `AUTHENTICATION_FAILED`, valid principal without grants fails at the PEP. Attached stdio and Streamable HTTP hosts monitor their daemon session, reject new requests after daemon loss, wait for active MCP requests for a bounded grace window, and then close their transports. `loom serve configure <store> mcp` records durable hosted intent, but the daemon does not start a hosted MCP listener yet. | (P0) | Remote hosted MCP auth maps OAuth/OIDC/SAML or certificate credentials to a Loom principal without bypassing the engine PEP. |
| Tool output schemas | surface | Every tool advertises a root object output schema with a per-tool `value` payload schema and returns structured content as `{ "value": ... }`; scoped servers preserve the output schema while eliding workspace, collection, and principal inputs. | (P0) | Refine deeply nested field-level schemas as individual hosted/agent surfaces need stricter specialization. |
| Tool annotations | surface | Titles, categories, read-only, destructive, idempotent, and open-world hints are source-backed. | (P0) | Keep annotations aligned with the output schema and elided tool surface. |
| Resources / templates | surface | Facet data as resources + templates | (P1) | Commits/diffs/blame as addressable resources; `.ics`/`.vcard`/`.eml` VFS projections; capability and conformance reports as resources. |
| MCP Apps | extension surface | `ui://` HTML resources backed by multi-file app directories under `/.loom/facets/mcp/apps/{app-name}/`, plus `apps.*` authoring tools over that reserved storage. The `mcp-apps` capability is registered separately from base `mcp-host`. | (P0) | Independently loadable app resources, optional tool-result visualization links, `just verify-apps` visual fixtures, Loom Templates processing, and app-only `apps_call_tool` visible-tool dispatch. |
| Subscriptions | capability (change feed) | Resource-changed notifications over stdio and Streamable HTTP SSE are source-backed at the transport level. | (P2) | Subscribe to a branch head, queue stream, SQL table, calendar collection, or other watchable Loom resource after the owning change-feed contract exists. Data changes use 0030 `DataChange` and domain-owned `DomainChange`; MCP session and daemon lifecycle notifications use lifecycle payloads. |
| List-changed notifications | capability (change feed) | Resource and tool list-changed mechanics are source-backed at the MCP level by comparing visible `resources/list` URI sets and ACL-filtered `tools/list` names per session. | (P2) | Notify when resource collections and authorized tool projections change from the Loom change-feed capability rather than MCP-only polling semantics. |
| Completion | surface | Workspace prefix completion | (P2) | Complete workspaces, collections, SQL tables/columns, branch/tag names, principals, calendar/contact ids, and RRULE fields. |
| Pagination | surface | Cursor paging on lists | (P0) | Stable list mechanic for tools/resources/prompts that return list pages. |
| Elicitation | surface | Destructive-op confirmation | (P2) | Merge-conflict resolution, encrypted-loom passphrase/unlock, ACL grant approval, and SQL migration confirmation. |
| Prompts | surface | Per-area prompt set | (P2) | Curated workflows that compose existing tools/resources plus inference; prompts never bypass the PEP. |
| Sampling | dropped | Not wired | n/a | MCP sampling is deprecated upstream and is not implemented. Loom inference remains a separate future capability with non-deprecated providers. |
| Progress | surface | Tool-call start/finish progress | (P2) | Long-operation progress for bundle import/export, clone, replay, vector-index build, search-index rebuild, and model download. |
| Cancellation | surface | Tool-call cancellation, daemon-loss transport cancellation after bounded request drain, and CLI-configurable graceful or hard daemon stop policy | (P2) | Cancellation for the same long operations that use progress. |
| Tasks | surface | Not promoted as a Loom MCP task surface | Cut | Do not implement MCP Tasks in the current serving surface. The legacy Tasks model is not viable, and the newer extension is not ready enough for Loom to adopt as an enterprise contract. Long operations continue to use synchronous tools with progress and cancellation until a future queue reopens this surface. |
| Ping | surface | Transport keepalive | (P0) | Keep long-lived stdio and HTTP SSE sessions alive. |

### 2.1 Deprecated MCP surfaces (do not reintroduce)

The following MCP features are deprecated and are intentionally **not** part of Loom's surface. Future
work must not reintroduce them:

- **Roots** - deprecated. Client-advertised filesystem roots are not consumed. Workspace and VFS mount
  scoping is expressed through Loom's own selectors and the binding (0008 section 9.10), not MCP roots.
- **Logging** - deprecated upstream and already dropped (see task 189); diagnostics flow through the
  engine's error/last-error contract, not the MCP logging surface.
- **Sampling** - deprecated upstream and not implemented. Inference providers are modeled as Loom
  capability providers rather than as a deprecated MCP client primitive.

### 2.2 MCP Apps subset

MCP Apps support is source-backed for the resource-delivery subset. A Loom-backed app lives in a
workspace under:

```text
/.loom/facets/mcp/apps/{app-name}/
  index.html
  _meta.md
  ...
```

Both files are mandatory. An app is omitted from `resources/list` unless `{app-name}` is a safe single
path segment, `_meta.md` is valid UTF-8 with the required front matter profile, and `index.html` is
valid UTF-8. Additional files below the app directory are allowed from the beginning and are managed as
ordinary app files. App HTML is returned from `resources/read` as `text/html;profile=mcp-app`.
Apps are independently loadable MCP resources. A tool is not required to mount, discover, or render an
App.

The served URI shape is:

```text
ui://{workspace}/mcp/apps/{app-name}
```

When an MCP server is not workspace-bound, app resources are presented as one flattened list across all
workspaces. The resource URI already includes the workspace, and the display name is also
workspace-qualified as `{workspace}/{app display name}` so two workspaces can expose apps with the same
manifest `name`.

For workspace-bound MCP servers, the URI is re-rooted to:

```text
ui://mcp/apps/{app-name}
```

In workspace-bound mode, the display name is the app manifest `name` without a workspace prefix.

`_meta.md` is the app manifest. It uses a strict YAML-front-matter subset whose fields map to the MCP
Apps resource shape plus a Loom-workspaced processing switch. The currently accepted keys are:

```text
name
description
mimeType
ui.domain
ui.prefersBorder
ui.visibility
ui.availableDisplayModes
ui.permissions.camera
ui.permissions.microphone
ui.permissions.geolocation
ui.permissions.clipboardWrite
ui.csp.connectDomains
ui.csp.resourceDomains
ui.csp.frameDomains
ui.csp.baseUriDomains
loom.processing
```

`name` is required and becomes the MCP resource display name. `mimeType`, when present, must be
`text/html;profile=mcp-app`. The CSP keys accept either a bracketed comma list or YAML list items.
`resources/list` places the static metadata under `_meta.ui`; `resources/read` places `_meta.ui` on the
returned content item and also adds the content-address version. `_meta.loom.processing` is
Loom-specific and accepts `static` or `templates`. `static` serves `index.html` directly. `templates`
selects Loom Templates processing for `index.html`; the `.html` extension has no template semantics by
itself. The current implementation renders the template through `crates/loom-templates` and returns the
rendered HTML with a returned-content version.

The `apps.*` MCP tools are an authoring and inspection surface over the same reserved app hierarchy.
`apps_list` is intentionally broader than the standard MCP Apps listing: it includes app candidate
directories and reports whether each candidate is valid, plus a status and reason when it is not. The
standard MCP Apps projection remains `resources/list`, which only advertises valid apps. `apps_show`
returns the display resource for one valid app. `apps_create`, `apps_write_file`, `apps_read_file`, and
`apps_remove_file` manage multi-file app contents under the app root without allowing generic `fs.*`
tools to write reserved facet paths.

The app surface reports as `mcp-apps`, separate from base `mcp-host`. `loom-core` registers the
capability as unsupported because the engine does not serve MCP; `loom-mcp` overlays it as supported
alongside `mcp-host` and returns the overlaid registry from `store_capabilities`. Its proof remains
`source-backed`: host-level conformance vectors cover valid app listing, invalid candidate inventory
status, valid-app-only `resources/list`, `resources/read` metadata, `apps_show`, and multi-file
authoring. Shared executable conformance remains future work until there is an MCP protocol backend
certification harness outside the engine-only `loom-conformance` crate.

This subset does not depend on the upstream MCP Apps iframe JSON-RPC channel for Loom MCP calls. The
bridge may be supported as part of host interoperability later, but Loom's first-class authoring path is
Loom Templates: AI agents write Jinja-compatible HTML templates and Loom processing metadata into the
loom. Static apps return `index.html` directly. Template-mode apps are rendered through
`crates/loom-templates` during resource read, and the returned-content digest becomes the app resource
version.

Tool `_meta.ui.resourceUri` is optional tool-result visualization metadata. It is not the existence
proof for an App and must not be required for standalone app loading. Loom emits `resourceUri` links
only from dynamic, source-backed launcher tools whose referenced app resource exists.

Some hosts, including MCP Inspector's Apps tab as of the checked source, discover apps only by scanning
tools for `_meta.ui.resourceUri`. Loom supports those hosts with a compatibility launcher-tool
projection over the same app resources. That projection is separate from the primary resource-backed
contract and uses the same flattened naming rule. Unbound launchers include the workspace in their
tool name, such as `apps.launch.{workspace}.{app}`. Workspace-bound launchers omit it, such as
`apps.launch.{app}`. `apps.open` is a generic resolver tool that returns the same launch payload, but it
does not advertise one fixed app resource because the caller supplies the app selector.

`ServerCapabilities.extensions` advertises `io.modelcontextprotocol/ui` for the implemented subset:
HTML `ui://` resources, `_meta.ui` metadata, and dynamic tool `resourceUri` linkage. Loom does not
require the iframe JSON-RPC bridge for its own app data path; hosts may still provide that bridge for
standard MCP Apps UI behavior.

The iframe bridge is host-owned compatibility behavior, not the Loom app data contract. Loom can certify
its server-side UI resources, dynamic launcher metadata, extension advertisement, template rendering,
and app delivery behavior. A host that exposes JavaScript bridge calls into the iframe must certify that
bridge in the hosted-protocol conformance suite rather than treating Loom's launcher metadata as proof
that the host bridge works.

App data delivery uses three prioritized channels:

1. Resource re-read plus Loom Templates rendering is the primary v1 path. The app shell is loaded as an
   MCP resource, and updated state is obtained by reading Loom resources or template-backed app routes
   again. This keeps the data path source-backed and avoids coupling Loom to the upstream iframe bridge.
2. Pull-watch app wakeups are source-backed for committed workspace changes. MCP App resource
   subscriptions keep a `watch` cursor for the app's workspace, advance it when 0030 `DataChange`
   payloads arrive with domain-owned `DomainChange` records or unsupported-domain markers, and then
   re-read the app resource or rendered template output through the existing PEP-backed resource path.
   ETag polling remains the compatibility fallback for uncommitted app edits and non-app resources.
3. Durable app notification delivery is source-backed at the MCP server layer. Subscribed app resource
   updates are recorded in per-app streams with stable ids, ack records, replay from ack or sequence,
   redelivery of unacked messages, and configurable retention. The app still re-reads the relevant
   resource or template output for data.
4. Upstream MCP Apps bridge compatibility is the final path. It may exist for host interoperability, but
   Loom-authored apps must not depend on the bridge for Loom MCP calls when the same flow can be served
   through Loom Templates or source-backed resources.

This ordering ranks *data delivery* (reads): an app must not fetch through the bridge what template
rendering or a resource re-read already provides. It does not discourage tool calls as such. Writes
and other user-initiated actions from an embedded view have no template or resource equivalent, and
bridge `tools/call` is the sanctioned channel for them: an app submitting a form calls the same
PEP-gated tool an agent would, subject to the same visibility and ACL checks. The internal
Decisions app (0063) is the worked example of the split: questions render into the view through the
`loom.ask` template binding, and answers return through a bridge `tools/call` of `ask_record`.

The source-backed generic bridge entry point is `apps_call_tool`. It accepts a visible app resource
URI, a target tool name, and JSON arguments; verifies that the app is visible to the current binding;
rejects app-only and app-launcher recursion; and dispatches the target through the normal MCP tool
path. The result envelope includes the target tool name and its structured tool result.

Built-in apps can be sourced from the `loom` binary rather than user-writable app files. The reserved
internal path starts with:

```text
/.loom/facets/mcp/apps/internal/
```

The first source-backed internal app shell is exposed at:

```text
/.loom/facets/mcp/apps/internal/vcs/_meta.md
/.loom/facets/mcp/apps/internal/vcs/index.html
```

That internal hierarchy is special: `apps_create`, `apps_write_file`, `apps_remove_file`, and `fs.*`
must not mutate it. `apps_show`, `apps_read_file`, `resources/list`, and `resources/read` can inspect
or load it from the binary-sourced asset provider. The VCS app renders workspace and VCS data through
the current `loom.vcs` template binding. The current user-authored app name model remains a single safe
path segment; the internal hierarchy uses an explicit reserved-path mapping rather than loosening
ordinary app names to include slashes.

The first catalog-aligned built-in app is `directed-graph`:

```text
/.loom/facets/mcp/apps/directed-graph/_meta.md
/.loom/facets/mcp/apps/directed-graph/index.html
```

It is binary-sourced like the internal apps, but it intentionally uses the public catalog app id and
URI (`ui://{workspace}/mcp/apps/directed-graph`) so the Surfaces core catalog and MCP app resource
surface agree. Its data binding projects graph nodes and edges from the Studio app catalog plus the
Meeting Memory app catalog, so the first graph bundle is profile-aware without adding another MCP
tool.

Built-in template-backed apps receive shared shell CSS through the `loom.app_shell.css` template
binding. The rendered app remains a single self-contained HTML resource, so hosts do not need a
secondary CSS fetch before painting the view.

The Pages app bundles are source-backed built-ins with instance-addressed routes:

```text
ui://{workspace}/mcp/apps/document-viewer/page/{page_id}
ui://{workspace}/mcp/apps/mind-map/structure/{structure_id}
ui://{workspace}/mcp/apps/canvas/structure/{structure_id}
ui://{workspace}/mcp/apps/diagram-editor/structure/{structure_id}
```

Workspace-bound servers elide `{workspace}` in the same way as other MCP app resources. The
Document Viewer route renders the selected page, its history, and its backlinks through
`loom.pages`; the structure routes render the selected structure through the same binding. The
bundles call the generic app-only `apps_call_tool` bridge for selected-page publish and structure
node add, move, and link actions. This is the accepted Pages app editor boundary; the generic MCP
Apps mechanism remains responsible for resource rendering and app-only bridge dispatch.

The Meetings app bundles are source-backed built-ins with one instance-addressed details route:

```text
ui://{workspace}/mcp/apps/meeting-details/meeting/{meeting_id...}
```

The trailing meeting id may contain path separators so imported ids such as `meeting/note-1` remain
addressable. Meeting Details, Memory Graph, Extraction Review, Meeting Search, Import Coverage, and
Access Audit render through `loom.meetings`, which includes the workspace, profile id, app
definition, meeting list, selected meeting detail, projection outputs, extraction review,
import-coverage status, and access-audit status. App bridge actions are limited to promoted
Meetings MCP tools. Import-run browsing, export workflow controls, and a Meetings-specific audit-log
projection remain target work.

### 2.3 Capability and conformance resources

MCP capability and conformance resources are projections of the 0010 section 5.1 record and report
contracts. They MUST NOT define MCP-only state names, boolean availability shortcuts, or alternate
error vocabularies. Each capability resource is scoped to the bound store, listener when present,
transport, selected profile, and caller-visible resource boundary.

Resource listing and reads are ACL-filtered. A caller denied access to a capability resource receives
the normal MCP projection of `PERMISSION_DENIED` only when the resource is visible; otherwise it is
masked as `NOT_FOUND`. Neither response may disclose listener disablement, missing compiled features,
runtime dependencies, bind state, private topology, or unsupported profiles outside authorized
administration and audit views.

For authorized callers, MCP preserves `proof_status`, `operational_state`, `reason_code`, and
`stable_error` from the shared record. Missing runtime dependencies and other runtime readiness
failures are `unavailable` with registry-backed subcause reason codes such as
`runtime_dependency_absent`, `service_unavailable`, or `listener_bind_failed`, and use stable error
`UNAVAILABLE`; `unsupported` uses `UNSUPPORTED`; and a declared `degraded` fallback remains a
successful resource or tool result with its result-equivalence boundary. A tool or resource success
MUST NOT imply that a different requested profile, listener, or capability scope is supported.

MCP capability resources remain target until the shared record codec, target `UNAVAILABLE` Code,
ACL-filtered resource projection, and conformance evidence are implemented. The local and hosted MCP
hosts may expose current source-backed capability reports only with their present declaration-catalog
semantics.

## 3. The inference capability

Inference is a first-class Loom capability (`inference`), decoupled from both MCP and any single
consumer. The seam is defined in `loom_core::inference`:

- `InferenceProvider` - a `Send + Sync` trait: `id()` and `infer(&InferenceRequest) -> Result<InferenceResponse>`.
- `InferenceRequest` - provider-agnostic: ordered `Message`s (user/assistant), optional `system_prompt`,
  `max_tokens`, `temperature`, and soft `ModelPreferences` (name hints plus cost/speed/intelligence
  priorities, mirroring the axes hosted model surfaces expose).
- `InferenceResponse` - the chosen `model`, `content`, and an optional `stop_reason`.
- `Inference` - the capability handle: holds at most one provider. `Inference::none()` is the default
  (core links no model) and returns `UNSUPPORTED` from `infer`; a host installs a backend with
  `Inference::with_provider`.

### 3.1 Backends (providers)

Providers live outside `loom-core`:

- **remote-api**: a hosted vendor endpoint. Future.
- **local**: a local-first downloaded model so Loom can infer with no connected client and no remote
  API. Weight storage, size/license, offline-first UX, and download progress (over the progress
  surface) are evaluated in task 219.

### 3.2 Consumers (connection points)

Any subsystem calls `Inference::infer`: programs (0015), triggers, scheduled tasks, GraphRAG (0040),
and serving surfaces (e.g. an MCP prompt that summarizes a diff). GraphRAG is therefore a *consumer*,
not the owner of inference. Each consumer is gated by the PEP (who may invoke, token budget, opt-in).

### 3.3 Policy

`infer` is a privileged op: a no-provider loom returns `UNSUPPORTED`; an installed provider is still
subject to PEP authorization and any configured token budget. Inference never reads a key or endpoint
secret from an environment variable; secrets are supplied by the host that installs the provider.

## 4. Scenarios

- **No provider.** A loom with `Inference::none()` answers `infer` with `UNSUPPORTED`; `provider_id()`
  is `None`; `is_available()` is false. (Executable: `loom_core::inference` unit tests.)
- **Installed provider dispatches.** With a provider installed, `infer` returns its
  `InferenceResponse`; `provider_id()` reports the backend id. (Executable.)
- **Trigger-driven inference.** A trigger fires, calls `infer`, and writes the completion into a facet
  (e.g. a document), all without an MCP client present, provided a non-MCP backend is installed.
  (Target, task 219.)
- **Branch-head change feed.** A client subscribes to a branch-head resource; a commit on that branch
  produces a 0030 `DataChange` and pushes `notifications/resources/updated`, over stdio and over the
  Streamable HTTP SSE channel alike (task 220). The per-session poll loop self-terminates when the
  transport closes. Session and daemon shutdown use lifecycle payloads, not `DomainChange`. (Target
  resources; transport push is source-backed.)
- **Roots refused.** A client advertising MCP roots has them ignored; scoping uses the Loom binding.
  (By construction; roots are deprecated, section 2.1.)

## 5. Implementation slices (inference capability)

The inference work is tracked here rather than in an external task list.

- **Core seam (source-backed).** `loom_core::inference` defines `InferenceProvider`,
  `InferenceRequest`/`InferenceResponse`, `ModelPreferences`, and the `Inference` handle with a
  no-provider `UNSUPPORTED` default, covered by `loom_core::inference` unit tests (the "no provider" and
  "installed provider dispatches" scenarios of section 4).
- **218a - PEP gating.** Authorize `infer` through the engine PEP: who may invoke, an optional token
  budget, and opt-in; a no-provider loom still returns `UNSUPPORTED` first. Secrets stay host-supplied
  (section 3.3), never read from the environment.
- **218b - conformance + capability flip (source-backed).** `loom-conformance` runs the inference
  seam scenarios: no-provider `UNSUPPORTED` and installed-provider dispatch through a deterministic
  test provider. The `inference` capability is executable and supported in the core registry. This does
  not implement trigger consumers, a local model backend, or a remote model backend.
- **218c - spec finalization.** Fold the above into this spec as source-backed and record it in the
  change log (this section + the entry below).
- **219 - consumers + local-first model.** Wire the non-MCP connection points (programs (0015),
  triggers, scheduled tasks, GraphRAG (0040)) to `Inference::infer`, and evaluate a local-first
  downloaded backend: weight storage, model size/license, offline-first UX, and download progress over
  the MCP progress surface. The "trigger-driven inference" scenario (section 4) is the acceptance case.

## 6. MCP surface promotion backlog

- (P0) **Tool output schema contract.** Source now advertises an output schema for every tool and
  returns every structured payload inside the stable `{ "value": ... }` envelope. Each tool has an
  explicit `value` schema for its scalar, byte-array, list, object, nullable, or void result shape; the
  release-blocking MCP object-root contract and elision behavior are source-backed.
- (P0) **Promoted data-tool projection.** Graph, vector, columnar, search, explicit
  collection-scoped `substrate_search`, workspace-scoped `substrate_changes`, and degraded
  `substrate_refs` data tools are source-backed through curated MCP tools, registered server
  methods, output schemas, and facade-boundary tests. `substrate_transact` source-backs typed
  `cas.*`, `document.*`, `graph.*`, and `substrate_view_define` operation batches with
  all-or-nothing rollback inside one store write closure. `substrate_view_define`,
  `substrate_view_get`, and
  `substrate_view_list` source-back the view-definition registry. The standard Studio status view
  resource `loom://{workspace}/studio/views/status/principal/{principal}` is source-backed as a
  status envelope. Resource templates for `substrate_view_get` and `substrate_refs` are
  source-backed as `loom://{workspace}/substrate/views/{view_id}.json` and
  `loom://{workspace}/substrate/refs/{target}.json`; `substrate_search` and `substrate_changes`
  are explicitly marked tool-only in tool metadata. `substrate_search` source-backs exact semantic
  search over an existing vector projection when the request supplies `query_vector`,
  `query_model_id`, and `query_weights_digest`; text query embedding, hybrid fusion, and whole-Loom
  search remain 0064 target work. Full projection population and the planning-store markdown mirror
  remain 0061 projection work.
- (P0) **Complete tool inventory.** `tools/list` is source-backed for visible model-facing tools and
  returns complete callable schemas with host-compatible advertised names. Loom does not expose a
  separate metadata-search tool for schema lookup; capable MCP clients decide how to progressively
  load or inject the complete inventory.
- (P0) **Bounded model-facing outputs.** Collection-shaped MCP tools use a default result page of 500
  items with optional `limit` and `offset` controls when the underlying result order is stable.
  Opaque byte-heavy tool outputs and `resources/read` enforce a 4 MiB delivered payload budget by
  default. `fs_list_directory` preserves the canonical CBOR `loom.fs.dir-listing.v1` result shape by
  decoding, slicing, and re-encoding the directory listing at the MCP boundary.
- (P2) **Resource subscriptions and list-changed.** Keep the MCP transport mechanics and current
  resource-list and tool-list fingerprinting, but promote them only as projections of the Loom
  change-feed capability. Resource-list change polling is independent of per-resource content
  subscriptions; content update polling still requires a subscription. Branch heads, queue streams,
  SQL tables, PIM collections, and ACL projection changes are the meaningful watched resources.
- (P2) **Prompts.** Keep curated prompts as workflow affordances over tools/resources. Prompt handlers
  do not read or write state directly.
- (P2) **Completion.** Expand completions beyond workspace prefix: collections, SQL tables/columns,
  branch/tag names, principals, calendar/contact ids, and RRULE fields.
- (P2) **Progress and cancellation.** Extend them to long operations rather than only short tool calls.
- (P2) **Elicitation.** Use it for user decisions: merge conflict choices, encrypted unlock, ACL grant
  approval, and SQL migration confirmation.
- (Cut) **Tasks.** Do not promote MCP task handles in this serving surface. The legacy Tasks model is
  not viable, and the newer extension is not ready enough for Loom to adopt as an enterprise
  contract. Long operations continue to use synchronous tools with progress and cancellation until a
  future queue reopens this surface against a stable extension and SDK contract.
- (P2) **Capability and conformance resources.** Add both capability and conformance reports as MCP
  resources once the report shape is stable enough to expose.

The current inference backlog lives outside this active MCP serving queue. The PEP/policy shape lives in
`loom-core`; future provider implementations live in host crates or provider crates.

### 6.1 Active hosted MCP owner gate

Completion state: active implementation owner. Local MCP stdio, Streamable HTTP mechanics, tool output
schemas, Apps authoring storage, Ask flows, and selected remote-backed forwarding paths are
source-backed. Hosted MCP listener startup, remote hosted authentication, served result-handle
projection for concrete hosted surfaces, iframe bridge certification, capability and conformance
report resources, and shared protocol conformance remain P0 implementation work. MCP task routes are
not part of the current serving surface.

Decision Points: none.

| Gate | Source-backed evidence | Remaining implementation work | Disposition |
| --- | --- | --- | --- |
| Hosted listener startup | `loom serve configure <store> mcp` records durable hosted intent, and attached stdio plus Streamable HTTP hosts source-back local transport behavior. | Start and manage a hosted MCP listener from daemon configuration without bypassing the daemon session model or hosted PEP. | Target P0. |
| Remote hosted authentication | Local daemon-backed MCP binds a launch-time principal and reattaches it on request open. | Map OAuth, OIDC, SAML, certificate, or deployment credentials to a Loom principal for hosted MCP, then re-check every tool/resource request through the engine PEP. | Target P0. |
| Served result handles | The shared hosted result-handle authorization substrate exists, and 0008 records the first concrete served route for prepared columnar Arrow IPC exports. MCP task routes are not promoted in the current serving surface. | Promote served result-handle routes only for concrete hosted surfaces that need them. Bind each handle to principal, session family, operation, workspace, scopes, and expiry, then re-check auth and PEP on poll, update, cancel, close, and result retrieval. MCP task routes stay out of this queue unless a future task reopens Tasks against a stable extension and SDK contract. | Target P0 for served result handles. MCP Tasks are cut. |
| Report resources | Current source-backed capability reports can be declared, and 0010 defines report records. | Expose capability and conformance reports as MCP resources only after report shape, ACL-filtered projection, unavailable mapping, and executable conformance are stable. | Target P0. |
| Apps iframe bridge certification | Loom Apps are source-backed as `ui://` resources and `apps.*` authoring tools; the upstream iframe JSON-RPC bridge is host-owned compatibility behavior. | Certify any JavaScript iframe bridge behavior in the hosted-protocol conformance suite instead of treating app metadata as bridge proof. | Target P0. |
| Shared protocol conformance | Local MCP server conformance and hosted protocol suites are valuable but separate evidence. | Keep local MCP, hosted MCP listener, remote MCP adapter, REST, JSON-RPC, and gRPC conformance rows distinct in release reports. | Target P0. |

## 7. Change log

- 0.1.17: Aligned the active hosted MCP owner gate with the Tasks cut: V2 work owns hosted listener,
  authentication, report resources, Apps bridge certification, shared protocol conformance, and served
  result-handle projection for concrete hosted surfaces. MCP task routes remain outside the current
  serving surface until a future queue reopens Tasks against a stable extension and SDK contract.
- 0.1.16: Added source-backed MCP ergonomics for large surfaces: complete standard `tools/list`
  remains the discovery contract; collection-shaped outputs default to 500 items with `limit` and
  `offset` controls; byte-heavy tool results and `resources/read` default to a 4 MiB delivered payload
  budget; `fs_list_directory` pages canonical `loom.fs.dir-listing.v1` without changing its result
  shape; and resource-list change notifications no longer require an active resource-content
  subscription.
- 0.1.15: Added source-backed MCP `substrate_view_get` projection payloads for built-in tickets
  open-item, Meetings extraction-review, and lifecycle operation-log view references. Unknown
  projection refs return a target-status payload instead of fabricated output.
- 0.1.14: Added source-backed MCP `kv_range` JSON predicate input over typed keys only. The predicate
  is parsed through `loom-substrate::predicate` and applied after the existing source-backed range
  result. KV values remain opaque bytes and are not inspected.
- 0.1.13: Added source-backed MCP document ergonomics: `document_query` returns id-filtered metadata
  rows, `document_get_text` returns UTF-8 text plus a digest, `document_put_text` writes exact UTF-8
  text with an optional digest guard, `document_get_binary` and `document_put_binary` preserve raw
  byte semantics, `document_list_binary` exports the canonical byte collection, and
  `document_replace_text` performs guarded UTF-8 find/replace using a base document digest.
  `document_query` is the MCP-native projection of the raw IDL `query_json` contract and also covers
  indexed exact-match lookup; raw `find_json` and document index-management IDL methods are not
  separate MCP tools. The public `document.patch` name remains reserved for schema-aware or 0061
  body-model patch semantics.
- 0.1.12: Added source-backed `columnar_select` JSON predicate input for the 0061 predicate root subset
  that lowers to the current single-column comparison filter through `loom-substrate::predicate`. The
  legacy byte-form `filter` remains accepted, and supplying both `filter` and `predicate` is rejected.
- 0.1.11: Added the source-backed MCP initialize identity contract. `loom-mcp` overrides rmcp's
  build-environment implementation default and reports implementation name `loom`, title `Loom MCP`,
  and the Loom package version.
- 0.1.10: Moved the MCP Apps and Loom Templates closeout state into the spec. The remaining MCP Apps
  protocol work is explicitly split between source-backed server behavior and a future host-owned
  iframe bridge certification profile.
- 0.1.9: Advertised the upstream MCP UI extension subset through `ServerCapabilities.extensions` and
  completed the source-backed launcher-tool compatibility path using dynamic `_meta.ui.resourceUri`
  links over valid app resources.
- 0.1.8: Added source-backed dynamic MCP tool visibility. `tools/list` filters regular tools from the
  bound session's current ACL-derived read/write visibility, advertises `tools.list_changed`, emits
  tool-list change notifications when the visible set changes, and rejects hidden stale tool calls
  before router dispatch while leaving argument-scoped enforcement in the engine PEP.
- 0.1.8a: Extracted the shared built-in MCP App shell CSS and injected it into VCS, Decisions, and
  Directed Graph through `loom.app_shell.css`.
- 0.1.8b: Expanded Directed Graph's template binding from a single-app self graph to catalog-derived
  profile graph data spanning the Studio and Meeting Memory app catalogs.
- 0.1.8c: Added `apps_call_tool` for app-only visible-tool dispatch and `just verify-apps` for
  Playwright-backed visual verification of VCS, Decisions, and Directed Graph bundles.
- 0.1.8d: Added binary-sourced Pages app bundles for Document Viewer, Mind Map, Canvas, and Diagram
  Editor, plus `loom.pages` template data and `just verify-apps` coverage for those bundles.
- 0.1.8e: Added instance-addressed Pages app routes for selected page/history and selected structure
  rendering.
- 0.1.8f: Added binary-sourced Tickets and planning app bundles for Ticket Details, Board, Roadmap,
  Sprint Planner, Backlog Triage, and Dashboards, plus `loom.tickets` template data, selected-ticket
  routes, source-backed ticket/lane tool metadata filtering, and `just verify-apps` coverage.
- 0.1.8g: Added binary-sourced Chat app bundles for Chat Channel, Chat Thread, Chat Tasks, Chat
  Presence, and Chat Handoffs, plus `loom.chat` template data, channel/thread routes, app-only
  message and presence tool dispatch, and `just verify-apps` coverage.
- 0.1.8h: Added binary-sourced Drive app bundles for Drive Browser, Drive Preview, Drive Sharing,
  Drive Conflicts, and Drive Retention, plus `loom.drive` template data, folder/file routes,
  app-only folder/upload tool dispatch, and `just verify-apps` coverage.
- 0.1.8i: Added binary-sourced Meetings app bundles for Meeting Details, Memory Graph, Extraction
  Review, Meeting Search, Import Coverage, and Access Audit, plus `loom.meetings` template data,
  path-shaped meeting detail routes, promoted Meetings tool dispatch, and `just verify-apps`
  coverage.
- 0.1.7: Added the source-backed binary-sourced internal VCS app at
  `/.loom/facets/mcp/apps/internal/vcs/`. It is advertised as `ui://.../mcp/apps/internal/vcs`,
  inspectable through read-only app surfaces, processed through Loom Templates, and excluded from app
  mutation tools by the existing user-app name validator.
- 0.1.6: Added the source-backed `apps.*` authoring surface. `apps_list` reports valid and invalid
  app candidates with status, `resources/list` remains valid-app-only, and `apps_create`,
  `apps_write_file`, `apps_read_file`, and `apps_remove_file` manage multi-file app directories under
  the reserved app root. Added the distinct `mcp-apps` capability and host-level conformance vectors.
- 0.1.5: Added the source-backed MCP Apps resource subset. `loom-mcp` discovers valid apps under
  `/.loom/facets/mcp/apps/{app-name}/`, parses strict `_meta.md` front matter, lists `ui://` resources
  with `_meta.ui`, and returns `index.html` as `text/html;profile=mcp-app` with content metadata.
- 0.1.4: Promoted the core inference seam to executable conformance and flipped the `inference`
  capability to supported/executable in the core registry. MCP sampling is dropped because it is
  deprecated upstream; non-MCP consumers and model providers remain target work.
- 0.1.3: Promoted the P0 MCP tool output-schema contract to source-backed: every tool advertises an
  object-root schema with an explicit `value` payload shape, every structured result uses the
  `{ "value": ... }` envelope, and scoped workspace/collection/principal bindings keep output schemas
  while eliding bound inputs.
- 0.1.2: Added the complete MCP surface design matrix, made full per-tool output schemas a P0
  contract item, and recorded the accepted design priorities for subscriptions/list-changed, prompts,
  completion, progress/cancellation, elicitation, tasks, and capability/conformance
  resources.
- 0.1.1: Folded the inference implementation plan into section 5 (core seam source-backed, PEP gating,
  conformance + capability flip, spec finalization, and consumers + local-first model). The work now
  lives in this spec rather than an external task list.
- 2026-06-28 (P-mcp-surface): New spec. Captured the MCP serving-surface mapping (surfaces vs
  capabilities), recorded the deprecated surfaces (Roots, Logging) as not-to-be-reintroduced, and
  defined the `inference` capability seam (`loom_core::inference`: `InferenceProvider`,
  `InferenceRequest`/`InferenceResponse`, `Inference` handle with a no-provider `UNSUPPORTED` default).
  programs/triggers/scheduled-tasks/GraphRAG are consumers; a local-first model is a future backend.
  Registered capability `inference` (source-backed, supported overlaid by hosts) in
  `loom-core::capability` and 0010 section 5.
