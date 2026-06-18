# uldren-loom-cli

The Uldren Loom command-line tool. Installs the `loom` binary.

Part of [Uldren Loom](https://github.com/uldrenai/uldren-loom): a universal, content-addressed,
versioned store.

## Install

```bash
cargo install uldren-loom-cli   # installs the `loom` binary
```

## Usage

```bash
loom version                       # print version information
loom hash path/to/file             # print a Blob content address ("blake3:..."); use - for stdin

# Persistent single-file (.loom) object store:
loom init   store.loom             # create an empty .loom file
loom put    store.loom path/file   # store a file (or - for stdin) as a Blob; prints its address
loom get    store.loom blake3:...  # write the Blob's bytes to stdout (or --out path/file)
loom stat   store.loom             # print the object count
```

A `.loom` is content-addressed and crash-consistent: `put` is atomic, and re-`put`ting identical
bytes is idempotent. `get` re-verifies `blake3(bytes) == address` before returning.

## Remote stores

Most store commands accept a **locator** in the store position: either a local `.loom` path, a
remote URL, or the special `context` locator selected by `--context` or the project-local current
context.

```bash
# A local path and a remote URL are interchangeable wherever a store is accepted:
loom kv list ./store.loom app
loom kv list https://loom.example.com/apps/loom app
loom --context prod kv list context app
```

Commands that need a local engine handle (for example store creation) reject a remote target with a
clear error rather than silently doing something else. Remote calls fail fast when the endpoint is
unavailable or speaks an incompatible protocol - there is no offline queueing or silent retry.

### Contexts (`contexts.toml`)

Contexts live in `contexts.toml` files, merged lowest-to-highest precedence:

1. system: `<system-config>/loom/contexts.toml`
2. user: `~/.loom/contexts.toml`
3. project: `<project>/.loom/contexts.toml` (project root; override with `--project <dir>`)
4. any explicit `--config <file>` (highest precedence, in command-line order)

Each `[contexts.<name>]` table takes `target` (required) and optional `default_workspace`, `auth`, `tls`, `discovery`,
`discovery_path`, `connect_timeout_ms`, and `request_timeout_ms`. Unknown fields are rejected, and
**secret-bearing fields are rejected** - contexts never carry tokens or keys, so `contexts.toml` is safe
to commit.

```toml
# ~/.loom/contexts.toml
[cli]
current_context = "prod"

[contexts.prod]
target = "https://loom.example.com/apps/loom"
default_workspace = "main"
auth = "interactive"          # interactive | token | mtls | principal | external
tls = "system"                # system | insecure-dev | bundle:NAME
discovery = "default"         # default | service-root | well-known | disabled

[contexts.local]
target = "file://app.loom"
```

### Serving a store: `loom serve remote`

`loom serve remote` binds a store as an HTTP/2-over-TLS endpoint and serves until interrupted
(SIGINT/SIGTERM):

```bash
loom serve remote ./store.loom \
  --bind 127.0.0.1:8443 \
  --service-root https://loom.example.com:8443/apps/loom \
  --tls-cert ./tls/fullchain.pem \
  --tls-key  ./tls/privkey.pem \
  --auth-mode interactive \
  --tls-trust system
```

Notable flags: `--call-endpoint` (defaults to `<service-root>/v1/call`), `--tls-client-trust <pem>`
to require and verify client certificates (mTLS), repeatable `--auth-mode` and `--tls-trust`,
`--session-lease-ms` (default `3600000`), `--max-request-bytes` (default 16 MiB), and
`--network-access-policy <name>`.

Clients discover the concrete call/stream endpoints from the service root over the endpoint-discovery
route (`discovery = "well-known"` queries `/.well-known/...`; `discovery_path` overrides it). For local
development against a self-signed loopback certificate, set `tls = "insecure-dev"` on the context (or
`--tls-trust insecure-dev` on the server); it is accepted only for loopback and never in production.

### MCP over a remote store

`loom mcp` serves a store as an MCP host over stdio or Streamable HTTP. A local locator serves the full
tool surface; a remote locator (URL or context) serves the data-family tools (KV, CAS, Queue, Ledger,
TimeSeries, full-text search, columnar, calendar, contacts, mail, filesystem, vector, plus document
reads, VCS reads and non-timestamped writes, and graph reads and node writes) over the wire:

```bash
loom mcp ./store.loom                          # full tool surface over stdio
loom --context prod mcp context                # remote context: data-family tools over the wire
loom --context prod mcp context --http 127.0.0.1:8080
```

Tools needing a local handle, the timestamped VCS writes, and the document/graph ref-index (edge)
writes return a clear not-yet/local-only error over a remote store, and `--stateless` is rejected for
remote hosts.

## License

Business Source License 1.1 (BUSL-1.1). See the [repository](https://github.com/uldrenai/uldren-loom).
