# Loom Templates

**Status:** Draft. **Version:** 0.1.1.
**Target crate:** `crates/loom-templates`
**Rust import name:** `loom_templates`

Loom Templates is the Jinja-compatible template processing layer for Loom-authored HTML and text
documents. It is not a web server, not an MCP host, and not a program runtime. It parses templates,
owns the template binding surface, validates host calls, extracts dependencies, and returns a
processing plan that callers can render or execute through their own policy boundary.

## 1. Boundary

`crates/loom-templates` owns:

- Jinja-compatible syntax and whitespace control.
- The `TemplateProcessor` API.
- The template-visible binding workspaces, starting with `loom.*` and later `request.*`,
  `response.*`, `session.*`, and `cookie.*`.
- Adapter traits or value types that callers use to supply authorized binding data.
- Template parsing, validation, diagnostics, and conformance fixtures.
- A template AST or stable processing plan.
- Dependency extraction for included templates, static assets, Loom programs, and query manifests.
- Host-call declarations such as `loom.program(...)`.
- Cache-key input calculation for compiled template plans.

It does not own:

- HTTP listeners, TLS, routing, hooks, logs, or deployment.
- MCP App discovery, `ui://` resources, or `apps.*` tools.
- Tool `_meta.ui.resourceUri` linkage or app-only tool visibility.
- Program execution, metering, or `StateAccess`.
- Durable delivery, WebSocket, SSE, or reconnect cursors.
- Storage of derived artifacts.
- Host-specific credentials, request transport state, and policy decisions.

## 2. Syntax

Loom Templates uses Jinja-compatible syntax. Loom-specific behavior is exposed through a workspaced function
surface rather than custom HTML tags.

```html
<section>
  {{- loom.program(name="dashboard/load") -}}
</section>
```

The target string passed to `loom.program` is an opaque program binding name. The template processor does
not split `dashboard/load` into program and method parts. The caller resolves it to a program manifest,
entrypoint, grants, and execution policy.

The long-term `loom.*` binding should be CLI/MCP shaped where possible and source-generated or
validated from the same IDL metadata used by other bindings. That keeps template calls aligned with the
public Loom contract instead of becoming a bespoke template-only API. `loom.program(...)` remains a
special host call because it targets a content-addressed program binding rather than a direct CLI or MCP
method.

Callers can also provide JSON-backed data values under `loom.*`. For example, the internal VCS app
renders source-backed workspace and VCS state through `{{ loom.vcs | tojson }}`. These values are
read-only render inputs, not host calls.

## 3. Processing Modes

Consumers opt into template processing with Loom-workspaced metadata.

```yaml
loom.processing: static
```

```yaml
loom.processing: templates
```

Accepted values:

- `static`: the caller serves or returns the source file without template processing.
- `templates`: the caller processes the source file through `TemplateProcessor`.

There is no template profile selector in v1. The only supported template language is Loom Templates,
with Jinja-compatible syntax.

## 4. Processor Output

A processor result contains at least:

```text
TemplatePlan
  source_path
  source_digest
  syntax_version
  ast_digest
  dependencies
  host_calls
  diagnostics
```

`dependencies` records referenced templates, static assets, program bindings, and query manifests.
`host_calls` records Loom calls that the caller may execute after checking policy. The processor itself
does not execute host calls.

## 5. Cache Keys

Template cache keys are derived, not allocated. `TemplateCacheInput` includes the source digest, syntax
version, consumer type, route or app metadata digest, program binding manifest digests, grants profile
digest, and render options. Program bindings and render options are sorted before hashing, so callers
can rebuild the same key from unordered policy data.

Deleting a cached rendered artifact is safe because the key is recomputed from source-backed inputs.
Rebuilding cannot reuse stale output after any source, metadata, program, grants, or render option
change because each of those values is part of the derived key.

## 6. MCP Apps Integration

An MCP App remains an MCP App first. Its `_meta.md` manifest maps MCP resource fields and UI metadata.
Loom Templates is selected only by the Loom-workspaced processing field:

```yaml
---
name: Dashboard
description: Template-backed dashboard app
mimeType: text/html;profile=mcp-app
ui.prefersBorder: true
loom.processing: templates
---
```

For `loom.processing: static`, `crates/loom-mcp` returns `index.html` directly. For
`loom.processing: templates`, `crates/loom-mcp` asks `crates/loom-templates` to process and render
`index.html`. The `.html` extension has no processing meaning by itself. The returned MCP resource text
is the rendered HTML, and the resource version is derived from the returned content. `crates/loom-mcp`
still owns app validity, resource URIs, and MCP metadata projection.

## 7. Web Serving Integration

The old Webish target design remains a web-serving consumer. Its listener, route, hook, cache, TLS, and
delivery concerns stay outside `crates/loom-templates`. Web serving can use `TemplateProcessor` for
template routes, but it supplies authorized request, response, session, and cookie values through the
binding adapters and owns policy checks, response streaming, and derived cache storage.

## 8. Conformance

The source-backed conformance set lives in `crates/loom-conformance` and is included in the aggregate
conformance runner and serialized report. It covers:

- Jinja-compatible condition, loop, include, import, from-import, extends, and whitespace syntax.
- `loom.program(...)` parsing.
- Rejection of unknown `loom.*` functions.
- Dependency graph stability.
- Stable diagnostics for malformed templates.
- Cache-key input stability.
- Rendered `loom.program(...)` output through caller-supplied bindings.
- JSON-backed `loom.<data>` render inputs such as `loom.vcs`.
- MCP App static versus templates metadata selection through `crates/loom-mcp` integration tests.

IDL-backed binding coverage, program execution, and live app data delivery remain outside the
conformance surface until those behaviors are promoted. MCP App wakeups, server-lifetime app
notification delivery, and launcher-tool bridge compatibility are covered by `loom-mcp` integration
tests rather than this template crate's conformance vectors.

The unfinished template-binding target remains owned here. The remaining work is:

- IDL-backed generation or validation for CLI/MCP-shaped `loom.*` operations beyond the current
  source-backed values and `loom.program(...)` special call;
- execution policy and result typing for `loom.program(...)` when a caller chooses to execute declared
  host calls;
- authorized `request.*`, `response.*`, `session.*`, and `cookie.*` binding adapters for web-serving
  consumers;
- shared conformance for promoted bindings once those bindings become part of the public Loom contract.

## 9. App Data Channel Order

Loom apps use Loom Templates and source-backed resources as the primary data/action path. The priority
order is:

1. Resource re-read plus template rendering.
2. Pull-watch app wakeups for committed workspace changes.
3. Server-lifetime app notification delivery with replay and ack.
4. Upstream bridge compatibility through dynamic launcher tools and `_meta.ui.resourceUri`.

The template processor stays independent in all three cases. It produces a plan and cache key from
source-backed inputs; callers supply authorized environment values and own policy checks, app refresh
mechanics, and bridge compatibility.

## 10. Open Decisions

Decision Points: none. The owner has selected `crates/loom-templates`, Jinja-compatible syntax,
workspaced `loom.*` functions, and `loom.processing: static|templates`.
