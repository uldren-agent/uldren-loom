---
name: loom-mcp-apps
description: Use when creating, editing, validating, or explaining Loom-backed MCP Apps under /.loom/facets/mcp/apps, including _meta.md metadata, app index.html files, app assets, and the apps_* MCP authoring tools.
---

# Loom MCP Apps

Use this skill when an agent needs to create or modify a Loom-backed MCP App.

## Contract

A Loom MCP App is a directory in a workspace:

```text
/.loom/facets/mcp/apps/{app-name}/
  index.html
  _meta.md
  ...
```

`index.html` and `_meta.md` are mandatory for the app to appear in the standard MCP Apps resource
listing. Other files below the app directory are allowed.

Use the dedicated `apps_*` MCP tools. Do not use generic `fs_*` tools to write reserved app paths.

## Tool Workflow

1. Inspect candidates with `apps_list`.
   - It returns valid and invalid app directories.
   - Check `valid`, `status`, and `reason`.
   - Invalid candidates may appear here but must not appear in MCP `resources/list`.
2. Create root files with `apps_create`.
   - Inputs: `workspace`, `app`, `index_html`, `meta_md`.
   - This validates app name, root HTML UTF-8, and `_meta.md` front matter.
3. Add or replace assets with `apps_write_file`.
   - Inputs: `workspace`, `app`, `path`, `content`, `mode`.
   - Use relative paths inside the app, such as `assets/data.json`.
   - Use `index.html` and `_meta.md` paths when replacing root files.
4. Read app files with `apps_read_file` when checking generated output.
5. Use `apps_show` to get the valid app resource URI and parsed metadata.
6. Verify the app through MCP resources:
   - `resources/list` should include only valid apps as `ui://.../mcp/apps/{app-name}`.
   - `resources/read` should return `text/html;profile=mcp-app` with `_meta.ui`.

## Metadata

`_meta.md` uses strict YAML front matter. Accepted keys:

```text
name
description
mimeType
ui.domain
ui.prefersBorder
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

`name` is required. `mimeType` must be `text/html;profile=mcp-app` when present. `loom.processing`
defaults to `static` and accepts only `static` or `templates`.

Example:

```markdown
---
name: Cohort Heatmap
description: Interactive retention cohort dashboard
mimeType: text/html;profile=mcp-app
ui.prefersBorder: true
ui.csp.resourceDomains: []
loom.processing: static
---
```

Unknown keys, malformed front matter, non-UTF-8 `_meta.md`, missing `_meta.md`, missing `index.html`,
or non-UTF-8 `index.html` make the candidate invalid for MCP Apps resource listing.

## HTML Rules

- Prefer a single self-contained `index.html` with inline CSS and JavaScript.
- Use extra app files only when they make data or assets clearer.
- Avoid external network resources. If an external resource is necessary, declare it in CSP metadata.
- Do not rely on the upstream MCP Apps iframe JSON-RPC bridge for Loom MCP calls.
- Treat Loom Templates as the primary future path for AI-authored dynamic content.

## Validation Checklist

Before calling the app complete:

1. `apps_list` reports the app with `valid: true` and `status: "valid"`.
2. `apps_show` returns the expected `ui://` resource URI.
3. `resources/list` includes the app only when valid.
4. `resources/read` returns the app HTML with `text/html;profile=mcp-app`.
5. Any intentionally invalid candidate is visible in `apps_list` with a useful status and absent from
   `resources/list`.
