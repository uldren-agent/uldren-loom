# 0012 - Interchange

**Status:** Target facade, current source-backed boundary documented. **Version:** 0.1.0.

Interchange is explicit conversion across the Loom boundary: host filesystems, external databases, and
portable tabular files into or out of a Loom workspace. It is not synchronization. Sync moves Loom
objects between Looms that already share the object model; interchange converts foreign data into or
out of that model.

## 1. Current Implementation

There is no monolithic stable `loom.interchange` IDL facade. Current source promotes the Archive and
CAR operations through their owning public surfaces in IDL, the C ABI, the C header, C++, and
iOS/Swift. Hosted projection and the remaining language-binding families are still target work. The
Rust crate and CLI surfaces below are source-backed.

Current source-backed primitives that a target interchange implementation can reuse:

- `loom-interchange` provides canonical schema records for import reports, export reports, fidelity
  issues, normalized import batches, import checkpoints, and archive manifests. These are reusable
  contracts for import/export reporting and profile-specific importer state.
- `loom-interchange` provides canonical profile-import plan records for foreign Studio source
  systems. Current source backs Redmine, Jira, Confluence storage-format XHTML, Confluence ADF,
  Markdown, Notion, Asana, Slack, Drive, and Granola-family source systems, plus action records that
  lower into ticket, page, chat, drive, or meetings profile operations. These records pin source
  entity id, source digest, target profile, action kind, optional target entity id, optional payload
  digest, source update time, notes, and fidelity issues. They are planning contracts, not importer
  execution.
- `loom-interchange-io` provides Rust filesystem import/export and archive import/export facades over
  the workspace file tree: `import_fs` imports a host directory into the selected workspace,
  `export_fs` writes either the current workspace file tree or a selected committed revision to a host
  directory, `import_archive` extracts zip, tar, tar.gz, tar.zstd, or single-file gzip inputs into
  workspace files, and `export_archive` writes deterministic tar.zstd, tar, tar.gz, or zip file-tree
  archives. It also provides deterministic Loom CAR import/export for moving a workspace object graph
  through a content-addressed archive file.
- `loom-core::vcs` provides workspace working-tree writes, commits, branch checkout, commit checkout,
  branch creation, merge, diff, and log.
- `loom-core::fs` provides file-oriented helpers over the workspace working tree and committed
  revisions, including explicit directories, stat, directory listing, revision file-entry reads,
  move, and copy.
- `loom-core::tabular` provides `put_table`, `get_table`, `list_tables`, and structured table storage.
- `loom-sql` persists GlueSQL tables into the selected workspace's SQL facet.
- `loom-interchange-io` provides source-backed CSV table import/export over the SQL facet through
  `import_table_csv`, `import_table_csv_bytes`, `export_table_csv`, and `export_table_csv_bytes`.
  The importer is schema-driven, supports snapshot and append-only modes, rejects append-only primary
  key collisions, gates writes with `sql` write permission, and preserves decimal values as Loom
  `Decimal { mantissa, scale }` instead of degrading them to floats or text. The exporter gates reads
  with `sql` read permission, emits stable header order and primary-key row order, writes NULL as an
  unquoted empty field, writes empty text as a quoted empty field, and preserves decimal scale.
- The CLI has `interchange import-fs`, `interchange export-fs`, `interchange import-archive`,
  `interchange export-archive`, `interchange import-redmine`, `interchange import-markdown`,
  `interchange import-notion`, `interchange import-asana`, `interchange import-jira`,
  `interchange import-confluence`, `interchange import-slack`, `interchange import-drive`,
  `interchange import-table-csv`, and `interchange export-table-csv`, plus `write`, `read`, `ls`,
  `commit`, `checkout`, `sql`, `table`, `bundle-export`, and
  `bundle-import`.
- The C ABI has workspace history checkout through `loom_checkout` and table reading through
  `loom_sql_read_table`. It also has `loom_fs_import`, `loom_fs_export`, `loom_archive_import`,
  `loom_archive_export`, `loom_car_import`, `loom_car_export`, and async task forms for filesystem,
  Archive, and CAR import/export operations. Filesystem import/export is source-backed through hosted
  REST/JSON-RPC adapters and Node, Python, JVM, Android, React Native, C++, and iOS/Swift bindings.
  Foreign table import/export is not promoted through the C ABI yet.

Current `checkout` means "switch a workspace branch and materialize it into the Loom working tree."
It does not mean "export a commit to a host directory." Bundle import/export belongs to 0006
synchronization because it moves Loom object bundles between Looms, not foreign filesystem or database
data.

Current archive support is source-backed through `loom-interchange-io` for file-tree import/export.
The canonical archive format is tar.zstd. Tar, tar.gz, and zip are compatibility formats. Single-file
gzip import remains supported as a compatibility input, but it is not a file-tree export format.
Archive import hashes the source archive, validates every entry as a safe relative Loom path, rejects
path traversal, rejects symlink entries, rejects encrypted zip entries with `UNSUPPORTED`, records an
`ArchiveManifest`, writes files through the workspace file-tree APIs, reports the actual object-count
delta added to the target store, and supports dry-run reporting. Filesystem import reports the same
object-count delta for source-backed `import_fs`. Filesystem import/export exposes IDL, C ABI, C
header, hosted REST/JSON-RPC, Node, Python, JVM, Android, React Native, C++, and iOS/Swift sync and
async-capable forms returning canonical `ImportReport` or `ExportReport` bytes. Archive export walks
the workspace working tree or a selected revision, sorts entries deterministically, writes
deterministic tar metadata for tar-family outputs, rejects symlink entries, records an
`ArchiveManifest`, and supports dry-run reporting. The source-backed CLI commands are
`loom interchange import-archive` and `loom interchange export-archive`. Archive import/export
exposes IDL, C ABI, C header, hosted REST/JSON-RPC adapters, Node, Python, JVM, Android,
React Native, C++, and iOS/Swift methods returning canonical `ImportReport` or `ExportReport`
bytes. WASM exposes byte-oriented archive import/export through the OPFS store surface. WASM archive
imports return canonical `ImportReport` bytes. WASM archive exports return a canonical CBOR array of
`[archive_bytes, ExportReport]`. WASM does not enable native zstd support, so `tar-zstd` remains a
native archive format while browser builds support tar, tar.gz, zip, and gzip import. Symlink
materialization remains target work.

Current CAR support is source-backed through `loom-interchange-io` and the CLI commands
`loom interchange export-car` and `loom interchange import-car`. Export writes a deterministic CAR
with a Loom manifest root, verifies object identity from content digests, supports dry-run reporting,
and does not mutate the source workspace. Import verifies CAR record lengths, root count, CID digest
matches, manifest root consistency, and required object presence before importing the bundle into the
store. CAR import/export exposes IDL, C ABI, C header, hosted REST/JSON-RPC adapters, Node,
Python, JVM, Android, React Native, C++, and iOS/Swift methods returning canonical `ImportReport`
or `ExportReport` bytes. WASM exposes byte-oriented CAR import/export through the OPFS store surface.
WASM CAR imports return canonical `ImportReport` bytes. WASM CAR exports return a canonical CBOR
array of `[car_bytes, ExportReport]`.

## 2. Target Boundary

Target interchange operations:

| Direction | Target operation | Foreign side | Loom side |
| --- | --- | --- | --- |
| In | `import_fs` | host filesystem tree | selected workspace file root |
| In | `import_table` | external database query or portable table file | selected workspace SQL facet |
| Out | `export_fs` | host filesystem tree | selected workspace commit tree |
| Out | `export_table` | external database or portable table file | selected workspace SQL facet |

The target facade should avoid naming the host-filesystem export operation `checkout`, because
`checkout` is already source-backed as workspace branch checkout in CLI, C ABI, and core VCS. Use
`export_fs` for writing a commit tree to a host directory.

Every target operation takes an explicit workspace selector. No operation relies on an ambient current
workspace.

## 3. Principles

- **P1 - Foreign systems are untrusted.** Imported bytes or rows enter only through source-backed Loom
  write paths, which then commit normal Loom objects.
- **P2 - Deterministic mapping.** Given the same input, import profile, target workspace state, and
  options, import should produce the same Loom object graph.
- **P3 - Explicit conversion.** Interchange is invoked deliberately and returns a report. It is not a
  background mirror.
- **P4 - Reuse content addressing.** Re-import should reuse existing objects and table rows where the
  source and profile prove equality.
- **P5 - Capability-gated.** Public surfaces are gated by the Loom facet they read or write. Current
  filesystem and archive import/export use `files` read/write checks through the shared execution
  crate. Current CAR export uses the core bundle read checks, and CAR import requires both `vcs`
  write over the imported workspace id and the core global-admin import gate. Current CSV table
  import/export use `sql` read/write checks.
- **P6 - Source fidelity.** Unmappable foreign content fails the operation unless the caller explicitly
  filters it out.

## 4. Target Public Surface Shape

Interchange is the shared conversion contract, not necessarily the public method namespace. Public
APIs SHOULD use the pragmatic surface that owns the operation, while reusing the shared report,
fidelity, checkpoint, and manifest records from this spec.

Illustrative IDL shape:

```idl
interface FileSystem {
  import_fs(ns: NsSelector, src_path: string, opts: ImportFsOptions): Future<ImportReport>
  export_fs(ns: NsSelector, rev: Rev, dst_path: string, opts: ExportFsOptions): Future<ExportReport>
}

interface Archive {
  import_archive(ns: NsSelector, src_path: string, opts: ImportArchiveOptions): Future<ImportReport>
  export_archive(ns: NsSelector, rev: Rev, dst_path: string, opts: ExportArchiveOptions): Future<ExportReport>
  inspect_archive(src_path: string): Future<ArchiveManifest>
  verify_archive(src_path: string, opts: VerifyArchiveOptions): Future<ArchiveVerificationReport>
}

interface Car {
  import_car(src_path: string, opts: ImportCarOptions): Future<ImportReport>
  export_car(ns: NsSelector, dst_path: string, opts: ExportCarOptions): Future<ExportReport>
  inspect_car(src_path: string): Future<CarInspectionReport>
  verify_car(src_path: string): Future<CarVerificationReport>
}

interface Sql {
  import_table(ns: NsSelector, src: TableSource, opts: ImportTableOptions): Future<ImportReport>
  export_table(ns: NsSelector, table: string, rev: Rev, dst: TableSink, opts: ExportTableOptions): Future<ExportReport>
}

struct ImportReport {
  profile: string
  source_scope: string
  commit: Option<Digest>
  objects_added: u64
  bytes_in: u64
  bytes_stored: u64
  rows_imported: u64
  skipped: u64
  operations_planned: u64
  operations_applied: u64
  dry_run: bool
  warnings: List<string>
  fidelity_issues: List<FidelityIssue>
}

struct ExportReport {
  profile: string
  destination_scope: string
  files_written: u64
  rows_written: u64
  bytes_out: u64
  dry_run: bool
  warnings: List<string>
  fidelity_issues: List<FidelityIssue>
}

struct FidelityIssue {
  severity: FidelitySeverity
  source_entity_id: string
  field: string
  reason: string
  source_digest: Option<Digest>
}
```

The final IDL, C ABI, binding, and wire shapes belong to 0003, 0007, and 0008. 0012 defines the
semantic boundary. The current Rust records live in `loom-interchange`; promoting them to the stable
IDL/C ABI/binding contract is still target work.

## 5. Filesystem Import

`import_fs` walks a host filesystem tree and stages files and explicit directories into the selected
workspace's file root, then optionally commits. It must not write into non-files facet paths unless
0014a and the owning facet permit that projection.

Target options:

```idl
struct ImportFsOptions {
  into_ref: string
  base: Option<Rev>
  message: string
  author: string
  ignore: List<string>
  follow_symlinks: bool
  verify: bool
  preserve_mode: bool
}
```

`base` enables incremental import. Stat-based reuse may use path, size, and mtime as a speed hint, but
`verify` forces content hashing. Identity still comes from Loom content bytes, not the host stat tuple.

Metadata that would affect object identity, such as mtime or xattrs, is not part of the current v1
object model. Such metadata needs a separate identity-profile decision before it can be preserved as
normative imported state.

## 6. Filesystem Export

`export_fs` materializes the current workspace file tree or a selected workspace commit tree to a
host directory. Source-backed Rust and CLI export currently support current-tree export and
revision-specific export for `HEAD`, branch names, and commit digests through
`loom-core::Loom::committed_fs_entries`. The shared VCS revision grammar accepts `HEAD`,
`commit:<digest>`, `branch:<name>`, `tag:<name>`, bare commit digests, and bare branch names.

Target options:

```idl
struct ExportFsOptions {
  overwrite: bool
  clean: bool
  sparse: Option<List<string>>
  restore_mode: bool
}
```

`clean` is scoped to the exported subtree and must not delete files outside it. Export reads must
verify object integrity before writing host bytes. Sparse export must preserve the same path
normalization and reserved path rules as workspace file operations.

## 7. Table Import

`import_table` ingests an external database query result or portable table file into a table in the
selected workspace SQL facet.

Current source backs the CSV source profile through `loom-interchange-io` and the CLI. It requires a
declared schema and primary key, stores into `.loom/facets/sql/{database}/tables/{table}`, defaults to
snapshot mode, supports append-only mode with duplicate primary-key rejection, and uses plain CSV as
a lossy interchange format. Supported CSV scalar mappings are integer, float, text, bool, sized
integer and float aliases, decimal, date, time, timestamp, and UUID. Empty unquoted fields import as
NULL; quoted empty fields import as empty text for text columns. Decimal import is exact for plain
decimal notation and stores the native Loom mantissa and scale.

Target sources:

- database source: DSN plus query;
- CSV source with declared schema and primary key;
- Arrow or Parquet source only after the columnar and SQL specs define the scalar mapping.

Target options:

```idl
struct ImportTableOptions {
  into_table: string
  primary_key: List<string>
  into_ref: string
  message: string
  chunk_rows: u64
  mode: ImportTableMode
}

enum ImportTableMode {
  Snapshot
  AppendOnly
}
```

`Snapshot` mirrors the source: rows absent from the source are removed from the Loom table. `AppendOnly`
only adds source rows and rejects primary-key collisions unless a later owning spec defines a stricter
policy. The default should be `Snapshot` for database query imports because it makes repeated imports
honest historical snapshots.

## 8. Table Export

`export_table` streams a table from a selected workspace revision to an external database table or a
portable table file.

Current source backs CSV export for the current workspace table. Export writes the declared table
columns as the header, rows in primary-key order, NULL as an unquoted empty field, text with CSV
quoting where needed, and decimals using the stored mantissa and scale. Export of selected historical
revisions, database sinks, Arrow, and Parquet remain target work.

Target sinks:

- database sink: DSN plus target table;
- CSV sink with stable column order and explicit scalar encoding;
- Arrow or Parquet sink after 0023 defines the columnar mapping.

Export is deterministic for one revision and one sink profile: stable column order, stable row order,
and lossless scalar encoding for supported types. Unsupported scalar values fail with
`INVALID_ARGUMENT` rather than lossy conversion.

## 9. Errors

Interchange reuses the stable error taxonomy:

- `NOT_FOUND` for missing workspace, revision, source path, source table, or Loom table;
- `INVALID_ARGUMENT` for bad selectors, malformed paths, unsupported options, or unmappable data;
- `PERMISSION_DENIED` for host or policy denial;
- `UNSUPPORTED` for absent capability or unsupported sink/source type;
- `IO` for host filesystem or external database I/O;
- `INTEGRITY_FAILURE` for failed Loom object verification;
- `CROSS_WORKSPACE` when one operation tries to span workspaces.

Additional stable error codes should be added to `loom_core::error::Code` only when current codes are
insufficient for a promoted public surface.

## 9.1 Source-Backed Reusable Records

`crates/loom-interchange` currently pins these canonical schemas:

| Schema | Role |
| --- | --- |
| `loom.interchange.import-report.v1` | Import result metrics, dry-run state, warnings, and fidelity issues. |
| `loom.interchange.export-report.v1` | Export result metrics, dry-run state, warnings, and fidelity issues. |
| `loom.interchange.fidelity-issue.v1` | Structured source-fidelity gap attached to a report. |
| `loom.interchange.import-batch.v1` | Normalized source observation batch with profile, source system, scope, coverage, cursor, sidecar, and source items. |
| `loom.interchange.import-batch-item.v1` | Stable source entity id, source digest, optional source update time, and optional sidecar digest. |
| `loom.interchange.import-execution-batch.v1` | Executable normalized import batch with profile, source system, scope, coverage, optional default space, and executable payloads. |
| `loom.interchange.import-execution-payload.v1` | One executable source snapshot payload with id, media type, exact bytes, digest, and optional source update time. |
| `loom.interchange.import-checkpoint.v1` | Durable importer checkpoint fields shared by API, filesystem, app-cache, MCP, and CSV-style importers. |
| `loom.interchange.archive-manifest.v1` | Archive container id, kind, root digest, and safe entry list. |
| `loom.interchange.archive-entry.v1` | Relative archive entry path, kind, size, digest, and optional link target. |
| `loom.interchange.profile-import-plan.v1` | Deterministic profile import plan for Redmine, Jira, Confluence storage XHTML, Confluence ADF, Markdown, Notion, Asana, Slack, Drive, and Granola-family source systems. |
| `loom.interchange.profile-import-action.v1` | One planned source-to-profile action with source digest, target profile, action kind, optional target entity id, optional payload digest, and notes. |

The crate enforces canonical encode/decode round trips, duplicate source entity rejection in a batch,
execution-payload digest verification, duplicate execution-payload id rejection, safe relative
archive paths, and duplicate archive-entry rejection. `loom-interchange-io` lowers zip, tar, tar.gz,
tar.zstd, and single-file gzip inputs into this manifest contract and workspace file writes. Current
source-backed tests also cover archive path traversal rejection, tar and zip symlink rejection,
encrypted zip rejection, Redmine mixed ticket/page actions, Jira ticket actions, Confluence's two
required source formats, Markdown, Notion, Asana, Slack, Drive, and Granola-family profile import
plans, plus duplicate planned-action rejection.

`crates/loom-conformance` runs the reusable interchange vectors as part of the canonical vector
inventory. The current vectors cover import reports, import batches, checkpoints, archive manifests
with directory entries, file digests, and symlink targets, duplicate archive path rejection,
duplicate source entity rejection, unsafe archive path rejection, Redmine mixed ticket/page planned
actions, Jira and Asana ticket planning, Confluence storage XHTML and ADF page planning, Markdown and
Notion page planning, Slack chat planning, Drive planning, Granola-family Meetings planning, CSV
source-system planning, and duplicate planned-action rejection. Executable import-batch conformance
vectors remain target work outside the current crate-local unit tests.

`crates/loom-interchange-io` is the execution crate for filesystem import/export, archive import,
and reusable profile-import execution where the importer has crossed from CLI-only plumbing into a
shared Rust service. It owns host IO, path normalization, dry-run reporting, archive decompression,
workspace file-tree writes, and the current Redmine, Markdown, Notion, Asana, Jira, Confluence,
Slack, Drive, and Meetings execution services used by the CLI. `loom-core` remains the kernel: it stores
canonical objects, workspace state, and file-tree semantics, but does not learn host filesystem
traversal, archive decompression, or profile-specific foreign-source lowering.

Current `loom-interchange-io` also provides the shared import input resolver used by the profile
import CLI wrappers. It detects regular files, directories, and archive candidates, hashes the source
with the target store's digest profile, fingerprints directory inputs deterministically by sorted
relative paths, can derive the shared import checkpoint shell from the resolved input, and persists
retained source evidence through audited control storage. File and archive inputs retain their exact
bytes by digest. Directory inputs retain a canonical manifest plus each source file payload by digest.
The shared helper also persists canonical import batches and import checkpoints under stable
control-plane keys. The Redmine, Markdown, Notion, Asana, Jira, Confluence, Slack, and Drive CLI
profile import wrappers use this retained-source and checkpoint path for non-dry-run imports.

Current `loom-interchange-io` also provides `execute_import_execution_batch` for canonical
`loom.interchange.import-execution-batch.v1` CBOR bytes. The executor verifies payload digests,
dispatches through the same reusable import services used by the CLI, persists the exact execution
batch bytes under an audited control-plane key, and returns the shared import report. Most profiles
require one executable snapshot payload. Drive execution batches may carry additional sidecar
payloads named by safe relative paths; the executor materializes those sidecars into a temporary
snapshot directory before running the same Drive importer as the CLI. Source-backed execution
profiles are Redmine, Asana, Jira, Confluence storage XHTML/ADF normalized snapshots, Markdown
zip/tar vault payloads, Notion snapshots and API-shaped bundles, Slack normalized snapshots or
exports, Drive normalized snapshots with optional sidecars, and Meetings snapshots for generic,
Granola API, Granola app, Granola MCP, and CSV input profiles. Markdown execution materializes the
archive payload into a temporary vault directory and then uses the same Markdown importer as the CLI,
without consulting ambient host paths.

The Meetings execution service is source-backed through `import_meetings_bytes`. It accepts the
normalized Meetings snapshot used by `loom meetings import`, lowers source, meeting, span,
annotation, and import-run records, writes retained source payload leaves, merges by stable id into
the Meetings profile snapshot control record, records audited committed writes, updates shared
profile revision rows for changed meetings, and returns the shared import report shape.

The source-backed CLI surface is:

```text
loom interchange import-fs <store> <workspace> <src> [--commit] [--dry-run] [--author <name>] [--message <text>] [--format text|json]
loom interchange import-archive <store> <workspace> <archive> --kind <tar-zstd|tar|tar-gzip|zip|gzip> [--gzip-output-path <path>] [--commit] [--dry-run] [--author <name>] [--message <text>] [--format text|json]
loom interchange import-redmine <store> <workspace> <profile> <snapshot.json> [--dry-run] [--format text|json]
loom interchange import-markdown <store> <workspace> <profile> <src> [--space <space-id>] [--dry-run] [--format text|json]
loom interchange import-notion <store> <workspace> <profile> <snapshot.json> [--space <space-id>] [--dry-run] [--format text|json]
loom interchange import-asana <store> <workspace> <profile> <snapshot.json> [--dry-run] [--format text|json]
loom interchange import-jira <store> <workspace> <profile> <snapshot.json> [--dry-run] [--format text|json]
loom interchange import-confluence <store> <workspace> <profile> <snapshot.json> [--dry-run] [--format text|json]
loom interchange import-slack <store> <workspace> <profile> <snapshot.json|export.zip> [--dry-run] [--format text|json]
loom interchange import-drive <store> <workspace> <profile> <snapshot.json> [--dry-run] [--format text|json]
loom interchange import-table-csv <store> <workspace> <database> <table> <csv> --schema <name:type,...> --primary-key <name,...> [--mode <snapshot|append-only>] [--commit] [--dry-run] [--format text|json]
loom interchange export-fs <store> <workspace> <dst> [--revision <HEAD|commit:DIGEST|branch:NAME|tag:NAME|BRANCH|DIGEST>] [--dry-run] [--format text|json]
loom interchange export-archive <store> <workspace> <archive> --kind <tar-zstd|tar|tar-gzip|zip> [--revision <HEAD|commit:DIGEST|branch:NAME|tag:NAME|BRANCH|DIGEST>] [--dry-run] [--format text|json]
loom interchange export-table-csv <store> <workspace> <database> <table> <csv> [--dry-run] [--format text|json]
loom interchange export-car <store> <workspace> <dst> [--dry-run] [--format text|json]
loom interchange import-car <store> <src> [--dry-run] [--format text|json]
```

The source-backed generic MCP import surfaces are `import_submit_batch` and `import_execute_batch`.
`import_submit_batch` accepts a workspace selector and canonical `loom.interchange.import-batch.v1`
CBOR bytes, validates the observation batch through the shared canonical decoder, hashes the exact
batch bytes with the store digest profile, stores those bytes under the same shared audited
control-plane key helper used by other import submission paths, and returns a structured summary
containing workspace id, source system, source scope, coverage, observed time, item count, batch
digest, and control key. `import_execute_batch` accepts canonical
`loom.interchange.import-execution-batch.v1` CBOR bytes, validates executable payload integrity,
dispatches source-backed byte-snapshot importers through `loom-interchange-io`, stores the exact
execution batch bytes under an audited control-plane key, and returns the shared import metrics
including rows imported, operations planned/applied, skipped rows, bytes in/stored, warnings, and
fidelity issue count. Workspace scoping elides and injects the workspace argument for
`loom mcp <store> <workspace>`. Redmine and Meetings also retain their profile-specific MCP
execution surfaces, `redmine_import_snapshot` and `meetings_import_snapshot`, for direct importer
ergonomics. Markdown vault execution through generic MCP batches is source-backed for zip, tar,
tar.gz, and tar.zstd payloads. Drive content-path execution through generic MCP batches is
source-backed when the batch supplies each referenced content path as a sidecar payload.

Current `import-fs` imports the current host directory tree into the selected workspace file tree.
Current `import-archive` imports tar.zstd, tar, tar.gz, zip, or single-file gzip archive contents
into the selected workspace file tree and returns both the shared import report and archive manifest
summary. It rejects path traversal, tar symlink entries, zip symlink entries, and encrypted zip
entries. Current `export-archive` exports the selected workspace working tree or selected committed
tree to tar.zstd, tar, tar.gz, or zip without checking out or mutating the workspace working tree.
Current `export-fs` exports the selected workspace working tree by default; with `--revision`, it
exports a selected committed tree without checking out or mutating the workspace working tree.
Current `export-car` and `import-car` delegate to the deterministic CAR service in
`loom-interchange-io`, returning shared export/import reports in text or JSON.
Current `import-redmine` delegates to the reusable Redmine importer in `loom-interchange-io`. It
imports normalized Redmine snapshot JSON or Redmine XML, lowers projects and issues through the
ticket profile service, lowers normalized wiki pages through the Pages profile service, preserves
Redmine issue identity as an external ticket identity, skips duplicate projects, issues, spaces, and
unchanged pages idempotently, retains Redmine journals, comments, attachments, time entries, and
relations as structured `redmine_*` ticket fields, and emits the shared import report in text or
JSON. The XML adapter normalizes `<projects>`, `<issues>`, and `<wiki_pages>` into the same import
model rather than adding a second lowering path. The source-backed verifier imports
`specs/studio/fixtures/redmine/source/redmine-api-bundle.xml` into a clean store and compares the
result against `specs/studio/fixtures/redmine/expected/comparison.json`. Projects, issue mapped
fields, wiki title/body, and Redmine source extras are either imported 1:1 or retained structurally;
wiki revision metadata is classified as a Pages-profile target gap. Native ticket comment,
attachment, link, work-log, and replayed-history operations remain target work in the ticket
profile, but Redmine import no longer drops those source records.
Loom-owned live Redmine API fetching is outside the importer target.
Current `import-markdown` delegates to the reusable Markdown importer in `loom-interchange-io`. It
walks `.md` files in a host directory, derives deterministic page ids from relative paths, creates
or reuses a pages space, lowers headings and paragraphs into canonical page body blocks, lowers list
items, quote lines, dividers, and whole-page, heading-target, and block-target Obsidian embeds,
publishes changed pages through the pages service, skips unchanged pages idempotently, and emits the
shared import report. Generic execution batches also run the same importer for Markdown vault
archives. Folder-note files such as `Folder/Folder.md` and `Folder/index.md` use the folder identity
instead of creating duplicate folder-note pages. The source-backed verifier imports
`specs/studio/fixtures/markdown/source/vault` into a clean store and compares the result against
`specs/studio/fixtures/markdown/expected/comparison.json`. Supported page content, folder-note
identity, and Obsidian page embeds are imported; frontmatter field mapping, aliases, tags, wikilink
references, explicit block ids, attachments, callouts, tables, footnotes, equations, Mermaid diagrams,
Dataview, Tasks-plugin syntax, Excalidraw, `.obsidian` config semantics, richer Obsidian extensions,
and canvas structures remain target work. Present but unsupported source constructs emit shared
fidelity issues.
Current `import-notion` delegates to the reusable Notion importer in `loom-interchange-io`. It
imports normalized Notion snapshot JSON and Notion API-shaped page plus block-children bundles,
derives deterministic page ids, creates or reuses pages spaces, lowers optional parent page
placement, lowers headings, paragraphs, list items, quote lines, and dividers into canonical page
body blocks, publishes changed pages through the pages service, skips unchanged pages idempotently,
classifies source metadata, database parents, formula and rollup properties, views, comments,
permissions, attachments, synced blocks, and unsupported blocks as fidelity issues, and emits the
shared import report. The source-backed verifier imports
`specs/studio/fixtures/notion/source/notion-api-bundle.json` into a clean store and compares the
result against `specs/studio/fixtures/notion/expected/comparison.json`. Exact block-tree lowering,
native database structures, formulas, rollups, views, comments, permissions, attachments, synced
blocks, rich-text marks, and unsupported Notion block preservation remain target work. Present but
unsupported database, formula, rollup, view, comment, permission, attachment, synced-block,
source-metadata, and unsupported-block fields emit shared fidelity issues.
Current `import-asana` delegates to the reusable Asana importer in `loom-interchange-io`. It imports
normalized Asana snapshot JSON, lowers projects and tasks through the ticket profile service,
preserves Asana task identity as an external ticket identity, stores source task fields as ticket
fields, applies source tags as policy labels, skips duplicate projects and tasks idempotently, and
emits the shared import report. The fixture-backed Asana vector at
`specs/studio/fixtures/asana/expected/comparison.json` verifies project creation, task creation,
external identity preservation, date fields, custom-field bundle retention, source tags as policy
labels, approval task retention, unsupported-field fidelity issues, and generic import-execution
batch dispatch. Asana organization/resource export parsing, multi-homing edges, native stories,
native attachments, portfolios, and goals remain target work. If normalized task records contain
subtasks, stories, attachments, portfolios, or goals before those lowerings exist, the importer emits
shared fidelity issues instead of dropping them silently.
Current `import-jira` delegates to the reusable Jira importer in `loom-interchange-io`. It imports
normalized Jira snapshot JSON, lowers projects and issues through the ticket profile service,
preserves Jira issue identity as an external ticket identity, stores Jira issue keys and core fields
as ticket fields, applies labels as policy labels, skips duplicate projects and issues idempotently,
and emits the shared import report. The fixture-backed Jira vector at
`specs/studio/fixtures/jira/expected/comparison.json` verifies project creation, issue creation,
external identity preservation, Jira key retention, core fields, custom-field bundle retention,
source labels as policy labels, unsupported-field fidelity issues, and generic import-execution
batch dispatch. Jira export parsing, issue-key alias preservation, native changelog/workflow/agile
lowering, identity mapping, comments, attachments, and worklog operations remain target work. If
normalized issue records contain changelogs, comments, attachments, or worklog entries before those
lowerings exist, the importer emits shared fidelity issues instead of dropping them silently.
Current `import-confluence` delegates to the reusable Confluence importer in `loom-interchange-io`.
It imports normalized Confluence snapshot JSON, creates or reuses Pages spaces, preserves
storage-format XHTML or ADF JSON bytes in canonical opaque page-body blocks, publishes changed pages
through the Pages profile service, skips unchanged pages idempotently, and emits the shared import
report. The fixture-backed Confluence vector at
`specs/studio/fixtures/confluence/expected/comparison.json` verifies space creation, storage XHTML
opaque body retention, ADF opaque body retention, markdown/text body lowering, parent placement,
unsupported-field fidelity issues, and generic import-execution batch dispatch. Confluence
site/export parsing, full XHTML/ADF block lowering, attachments, comments, and cross-format
semantic equivalence vectors remain target work. If normalized page records contain attachments or
comments before those lowerings exist, the importer emits shared fidelity issues instead of dropping
them silently.
Current `import-slack` delegates to the reusable Slack importer in `loom-interchange-io`. It imports
normalized Slack snapshot JSON, creates or reuses Chat channels, lowers plain text message bodies
through the Chat service, creates thread records when the parent message is present, registers
reaction names, applies one reaction per imported reaction kind, skips duplicate channels and
messages idempotently, and emits the shared import report. It also accepts a standard Slack export
zip with `channels.json` and channel message JSON files, normalizing those files into the same
channel and message import path. The fixture-backed Slack vector verifies normalized snapshots,
export zip parsing, channel creation, message body import, thread creation, reaction-kind lowering,
unsupported-field fidelity issues, and generic import-execution batch dispatch. mrkdwn and Block Kit
to canonical 0061 block conversion, imported user/principal mapping, per-user reaction authorship,
files, pins, membership, custom emoji assets, and coexistence bridge behavior remain target work. If
normalized records contain files, pins, custom emoji, channel members, or per-user reaction
authorship before those lowerings exist, the importer emits shared fidelity issues instead of
dropping them silently.
Current `import-drive` imports normalized Drive or SharePoint snapshot JSON through the reusable
Drive importer in `loom-interchange-io`. It creates folders through the Drive service, writes file
bytes supplied as inline text, inline hex bytes, local content paths, or import-execution sidecar
payloads through the Drive upload/commit service path, replaces changed existing files, skips
unchanged files idempotently, preserves Drive revision rows for committed uploads, and emits the
shared import report. The fixture-backed Drive vector verifies normalized Drive/SharePoint
snapshots, folder and file placement, direct `content_path` reads, generic import-execution sidecar
materialization, unsupported-field fidelity issues, changed-file replacement, and unchanged-file
skips. Google Drive and SharePoint export parsing, direct permission and share mapping, comments,
shortcut directory entries, multi-parent lowering, SharePoint metadata projection, and identity
mapping remain target work. If normalized records contain permissions, comments, historical
revisions, metadata, or shortcut targets before those lowerings exist, the importer emits shared
fidelity issues instead of dropping them silently.

## 10. Relationship to Other Specs

- 0003 owns final facade shape and workspace selector projection.
- 0006 owns Loom bundle import/export and clone. Those are sync operations, not interchange.
- 0007 owns language binding projection.
- 0008 owns hosted protocol projection.
- 0011 owns SQL table encoding, row order, row merge, and SQL result semantics.
- 0014 owns workspace identity and reserved facet paths.
- 0014a owns direct file-style projection for non-files facets.
- 0023 owns Arrow and Parquet posture before columnar table export is promoted.
- 0026 through 0028 must precede hosted write surfaces for external import/export.

## 11. Resolved Decisions

- **RD1 - Interchange is not sync.** Bundle import/export, workspace clone, and branch push remain
  0006 sync behavior.
- **RD2 - Host export name.** The target host-directory export operation is `export_fs`, not
  `checkout`, to avoid colliding with source-backed branch checkout.
- **RD3 - Workspace explicitness.** Every operation takes a workspace selector.
- **RD4 - Strict fidelity.** Unmappable foreign content fails unless explicitly filtered.
- **RD5 - Table import default.** Database query import defaults to snapshot semantics.
- **RD6 - Git import.** Git repository import and live Git protocol interoperability are out of scope.
- **RD7 - Metadata identity.** mtime, xattrs, and platform metadata are not identity-affecting import
  state until a separate data-model profile promotes them.
- **RD8 - Hosted writes.** Served import/export write paths wait on principal and access-control specs.
- **RD9 - Shared interchange crate.** Reusable import/export contracts live in `loom-interchange`,
  separate from `loom-core` and from profile-specific importers.
- **RD10 - Execution crate split.** Host filesystem traversal and import/export execution live in
  `loom-interchange-io`, not `loom-core`, so future profile-specific importers can reuse the same
  contracts without coupling the kernel to host IO dependencies.
- **RD11 - Archive dependencies.** Archive import/export uses in-process permissive Rust crates in
  `loom-interchange-io`: stable `zip`, `tar`, `flate2`, and `zstd`. It does not shell out to host
  archive tools and does not pull archive dependencies into `loom-core`.
- **RD12 - Public facade names.** `interchange` is the shared substrate and semantic contract. Public
  APIs use the pragmatic owning surfaces: filesystem import/export on `FileSystem`, archive operations
  on `Archive`, CAR operations on `Car`, and table import/export on `Sql` or the promoted table
  surface. Shared report, fidelity, checkpoint, and manifest records remain owned by 0012.
- **RD13 - CSV table interchange is intentionally lossy.** CSV table import/export is for common
  tool interoperability, not lossless table movement. Supported scalar types are encoded explicitly,
  decimals preserve mantissa and scale, and unsupported scalars fail rather than being converted
  silently. Lossless table movement belongs to canonical table or object-archive formats.

## 12. Unfinished Work

- (P1) Add symlink materialization support if a future profile needs to preserve links as first-class
  exported filesystem entries.
- (P1) Add selected-revision table export after the table revision selector is promoted through the
  public SQL/table surface.
- (P1) Add Arrow and Parquet table import/export after the 0023 columnar mapping is source-backed.
- (P1) Add executable import-batch conformance vectors for
  `loom.interchange.import-execution-batch.v1` after the profile-specific importer vectors are
  promoted.
- (P1) Add profile-specific importer conformance vectors once filesystem, table, and Studio importers
  are source-backed.
