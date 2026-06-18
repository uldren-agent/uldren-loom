//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Inspect, configure, and compact the durable audit log.
    Audit {
        #[command(subcommand)]
        action: AuditCmd,
    },
    /// Create collections, store entries, search, range, and project iCalendar.
    Calendar {
        #[command(subcommand)]
        action: CalendarCmd,
    },
    /// Put, get, test, list, and delete workspace content-addressed blobs.
    Cas {
        #[command(subcommand)]
        action: CasCmd,
    },
    /// Print the source-owned capability matrix.
    Capabilities {
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
        /// Include target-only and internal capability rows.
        #[arg(long)]
        all: bool,
    },
    /// Read and write organization chat channels.
    Chat {
        #[command(subcommand)]
        action: ChatCmd,
    },
    /// List, import, export, remove, audit, and generate portable TLS certificate bundles.
    Certificate {
        #[command(subcommand)]
        action: CertificateCmd,
    },
    /// Manage reusable network access policies for hosted listeners.
    NetworkAccess {
        #[command(subcommand)]
        action: NetworkAccessCmd,
    },
    /// Create, import, export, append, scan, select, aggregate, inspect, and compact columnar datasets.
    Columnar {
        #[command(subcommand)]
        action: ColumnarCmd,
    },
    /// Create books, store contacts, search, and project vCard.
    Contacts {
        #[command(subcommand)]
        action: ContactsCmd,
    },
    /// Create, collect, preview, materialize, and inspect dataframe plans.
    Dataframe {
        #[command(subcommand)]
        action: DataframeCmd,
    },
    /// Start, stop, restart, and inspect the local coordinator daemon.
    Daemon {
        #[command(subcommand)]
        action: DaemonCmd,
    },
    /// Manage first-class CLI contexts.
    Context {
        #[command(subcommand)]
        action: ContextCmd,
    },
    /// Put, get, delete, and list versioned documents.
    Document {
        #[command(subcommand)]
        action: DocumentCmd,
    },
    /// Inspect or reconcile unresolved references with a keyed Loom session.
    #[cfg(feature = "mcp")]
    Refs {
        #[command(subcommand)]
        action: RefsCmd,
    },
    /// Diagnose Loom store, daemon, inference, and runtime health.
    Doctor {
        #[command(subcommand)]
        action: DoctorCmd,
    },
    /// Run, inspect, and apply program-execution (`exec`) requests over the canonical CBOR contract.
    Exec {
        #[command(subcommand)]
        action: ExecCmd,
    },
    /// Store, inspect, list, retrieve, and remove local Program records.
    Program {
        #[command(subcommand)]
        action: ProgramCmd,
    },
    /// Read, write, delete, list, and create workspace files and directories.
    Files {
        #[command(subcommand)]
        action: FilesCmd,
    },
    /// Read and manage shared Drive profile files.
    Drive {
        #[command(subcommand)]
        action: DriveCmd,
    },
    /// Upsert nodes and edges, remove them, traverse neighbors, and find paths.
    Graph {
        #[command(subcommand)]
        action: GraphCmd,
    },
    /// Put, get, delete, list, and range typed key-value entries.
    Kv {
        #[command(subcommand)]
        action: KvCmd,
    },
    /// Append entries, read entries, inspect heads, verify chains, and list ledgers.
    Ledger {
        #[command(subcommand)]
        action: LedgerCmd,
    },
    /// Put, get, and query native metric descriptors and observations as canonical CBOR.
    Metrics {
        #[command(subcommand)]
        action: MetricsCmd,
    },
    /// Put, get, and query native log records as canonical CBOR.
    Logs {
        #[command(subcommand)]
        action: LogsCmd,
    },
    /// Put, get, and query native trace spans as canonical CBOR.
    Traces {
        #[command(subcommand)]
        action: TracesCmd,
    },
    /// Define, instantiate, transition, and inspect workspace lifecycles.
    Lifecycle {
        #[command(subcommand)]
        action: LifecycleCmd,
    },
    /// Acquire, refresh, release, and inspect coordinator locks.
    Lock {
        #[command(subcommand)]
        action: LockCmd,
    },
    /// Create mailboxes, ingest messages, set flags, search, and project EML.
    Mail {
        #[command(subcommand)]
        action: MailCmd,
    },
    /// Import normalized meeting-memory snapshots.
    Meetings {
        #[command(subcommand)]
        action: MeetingsCmd,
    },
    /// Create, update, publish, read, and inspect workspace pages.
    Pages {
        #[command(subcommand)]
        action: PagesCmd,
    },
    /// Create, update, list, read, and inspect workspace tickets.
    Tickets {
        #[command(subcommand)]
        action: TicketsCmd,
    },
    /// Create, inspect, update, and reorder Lane coordination records.
    ///
    /// Lane is coordination state; the ticket is the source of truth for work evidence. Record
    /// evidence, source anchors, decisions, questions, blockers, and result summaries on the active
    /// ticket (typed fields or comments), not on the Lane. `status_report` and `reviewer_feedback`
    /// are short pointers to that ticket state, not a place to store it.
    Lanes {
        #[command(subcommand)]
        action: LanesCmd,
    },
    /// Hidden compatibility umbrella for `workspace`, `identity`, `acl`, `kv`, and
    /// `protected-ref`, which are now top-level commands.
    #[command(hide = true)]
    Management {
        #[command(subcommand)]
        action: ManagementCmd,
    },
    /// Manage local inference model downloads and installed model records.
    Inference {
        #[command(subcommand)]
        action: InferenceCmd,
    },
    /// Manage direct ACL grants in a `.loom` store.
    Acl {
        #[command(subcommand)]
        action: AclCmd,
    },
    /// Manage principals, credentials, keys, and roles in a `.loom` store.
    Identity {
        #[command(subcommand)]
        action: IdentityCmd,
    },
    /// Import from and export to foreign host data sources.
    Interchange {
        #[command(subcommand)]
        action: InterchangeCmd,
    },
    /// Create, list, rename, and delete workspaces in a `.loom` store.
    Workspace {
        #[command(subcommand)]
        action: WorkspaceCmd,
    },
    /// Manage branch and tag protected-ref policy records.
    ProtectedRef {
        #[command(subcommand)]
        action: ProtectedRefCmd,
    },
    /// Serve a Loom store as an MCP host over stdio or Streamable HTTP. A local locator serves the full
    /// tool surface. A remote locator (URL or selected context) serves the KV, CAS, Queue, Ledger,
    /// TimeSeries, full-text search, columnar, calendar, contacts, mail, filesystem, and vector tool
    /// families (plus document reads, the VCS reads + non-timestamped writes, and the graph reads +
    /// node writes) over the remote Loom; document/graph ref-index (edge) writes and the timestamped VCS
    /// writes (commit/tag_create/merge/... whose caller timestamp has no remote IDL parameter) and other
    /// tools return a clear not-yet/local-only error, and `--stateless` is rejected.
    #[cfg(feature = "mcp")]
    Mcp {
        /// Local `.loom` path, `context`, or a remote URL.
        store: String,
        /// Optional workspace UUID or name to bind at launch.
        workspace: Option<String>,
        /// Optional collection name to bind at launch.
        collection: Option<String>,
        /// Serve over Streamable HTTP at this address (for example `127.0.0.1:8080`), mounting the host
        /// at `POST /mcp`, instead of stdio.
        #[arg(long)]
        http: Option<String>,
        /// Network access policy name for the Streamable HTTP listener.
        #[arg(long)]
        network_access: Option<String>,
        /// With `--http`, use MCP stateless mode: POST-only, a fresh server per request, no session -
        /// so no subscription push or progress streams. Ignored without `--http`.
        #[arg(long)]
        stateless: bool,
    },
    /// Mount workspace projections through FUSE or NFS.
    #[cfg(any(feature = "fuse", feature = "nfs"))]
    Mount {
        #[command(subcommand)]
        action: MountCmd,
    },
    /// Append, read, range, count, and advance queue streams.
    Queue {
        #[command(subcommand)]
        action: QueueCmd,
    },
    /// Search across Loom full-text collections with degraded unified-search status.
    Search {
        /// Path to the `.loom` file.
        store: String,
        /// Free-text query.
        query: String,
        /// Optional workspace UUID or name. Omit for all readable search workspaces.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: Option<String>,
        /// Optional collection name. Omit for all readable collections in scope.
        #[arg(long)]
        collection: Option<String>,
        /// Optional text field filter.
        #[arg(long)]
        field: Option<String>,
        /// Maximum hits to print.
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Hits to skip before printing.
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Create full-text collections, index documents, query, remap, list ids, rebuild native indexes, and inspect status.
    #[command(name = "fts")]
    Fts {
        #[command(subcommand)]
        action: SearchCmd,
    },
    /// Configure and manage hosted listeners for served surfaces.
    Serve {
        #[command(subcommand)]
        action: ServeCmd,
    },
    /// Rebuild derived Studio indexes and projections.
    Studio {
        #[command(subcommand)]
        action: StudioCmd,
    },
    /// Execute SQL, stream query rows, inspect tables, blame rows, and diff tables.
    Sql {
        #[command(subcommand)]
        action: SqlCmd,
    },
    /// Create, inspect, encrypt, rekey, clone, import, and export Loom stores.
    Store {
        #[command(subcommand)]
        action: StoreCmd,
    },
    /// Put, get, range, and read latest time-series points.
    #[command(name = "time-series", alias = "timeseries", visible_alias = "ts")]
    TimeSeries {
        #[command(subcommand)]
        action: TimeSeriesCmd,
    },
    /// Commit, log, diff, branch, checkout, merge, restore, tag, replay, and rewrite history.
    Vcs {
        #[command(subcommand)]
        action: VcsCmd,
    },
    /// Create sets, upsert vectors, index metadata, search, delete, and inspect sources.
    Vector {
        #[command(subcommand)]
        action: VectorCmd,
    },
    /// Print the usage reference for every command (`--llms`; `--llms-full` adds the
    /// argument and option glossaries).
    Llms,
    /// Print version information.
    Version,
}

#[derive(Subcommand)]
pub(crate) enum ContextCmd {
    /// List configured contexts.
    List {
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Show one configured context.
    Get {
        name: String,
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Add a project-local context.
    Add {
        name: String,
        target: String,
        #[arg(long)]
        default_workspace: Option<String>,
        #[arg(long)]
        auth: Option<String>,
        #[arg(long)]
        tls: Option<String>,
        #[arg(long)]
        discovery: Option<String>,
        #[arg(long)]
        discovery_path: Option<String>,
        #[arg(long)]
        connect_timeout_ms: Option<u64>,
        #[arg(long)]
        request_timeout_ms: Option<u64>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Update a project-local context.
    Update {
        name: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        default_workspace: Option<String>,
        #[arg(long)]
        auth: Option<String>,
        #[arg(long)]
        tls: Option<String>,
        #[arg(long)]
        discovery: Option<String>,
        #[arg(long)]
        discovery_path: Option<String>,
        #[arg(long)]
        connect_timeout_ms: Option<u64>,
        #[arg(long)]
        request_timeout_ms: Option<u64>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Remove a project-local context.
    Remove { name: String },
    /// Test context resolution without opening or mutating a store.
    Test {
        name: String,
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Select the project-local current context.
    Use { name: String },
    /// Show the selected current context.
    Current {
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum ChatCmd {
    /// List channels in one Chat workspace.
    Channels {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Create a channel.
    CreateChannel {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel handle.
        handle: String,
        /// Human-readable channel name.
        name: String,
        /// Optional channel UUID. A UUID is generated when omitted.
        #[arg(long)]
        channel_id: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Rename a channel handle.
    RenameChannel {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// New channel handle.
        handle: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List projected messages in one channel.
    Messages {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List operation events in one channel.
    Events {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// First operation sequence to read.
        #[arg(long, default_value_t = 0)]
        from_sequence: u64,
        /// Maximum events to return.
        #[arg(long, default_value_t = 100)]
        max: usize,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Read the durable cursor for the current principal.
    Cursor {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Move the durable cursor for the current principal.
    UpdateCursor {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Next unread sequence.
        next_sequence: u64,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Append a message to one channel.
    Post {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Message id supplied by the caller.
        message_id: String,
        /// Optional parent thread id.
        #[arg(long)]
        thread: Option<String>,
        /// Message body file, or `-` for standard input.
        #[arg(long, default_value = "-")]
        input: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Append a message edit.
    Edit {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Message id.
        message_id: String,
        /// Message body file, or `-` for standard input.
        #[arg(long, default_value = "-")]
        input: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Append a message redaction.
    Redact {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Message id.
        message_id: String,
        /// Optional redaction reason.
        #[arg(long)]
        reason: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Create a thread under a message.
    CreateThread {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Thread id.
        thread_id: String,
        /// Parent message id.
        parent_message_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Create a task from chat context.
    CreateTask {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Task id.
        task_id: String,
        /// Task title.
        title: String,
        /// Optional source message id.
        #[arg(long)]
        message_id: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Claim a chat task.
    ClaimTask {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Task id.
        task_id: String,
        /// Claim id.
        claim_id: String,
        /// Optional lease token.
        #[arg(long)]
        lease_token: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Complete a claimed chat task.
    CompleteTask {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Task id.
        task_id: String,
        /// Claim id.
        claim_id: String,
        /// Optional result message id.
        #[arg(long)]
        result_message_id: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Record an agent invocation.
    InvokeAgent {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Invocation id.
        invocation_id: String,
        /// Agent principal UUID.
        agent_principal: String,
        /// Source message ids.
        #[arg(long, value_delimiter = ',')]
        source_message_ids: Vec<String>,
        /// Prompt body file, or `-` for standard input.
        #[arg(long, default_value = "-")]
        input: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Attach a reply message to an agent invocation.
    AgentReply {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Invocation id.
        invocation_id: String,
        /// Reply message id.
        message_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Request a handoff from an agent to a principal.
    RequestHandoff {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Handoff id.
        handoff_id: String,
        /// Source agent principal UUID.
        from_agent_principal: String,
        /// Optional target principal UUID.
        #[arg(long)]
        to_principal: Option<String>,
        /// Optional handoff reason.
        #[arg(long)]
        reason: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Add a reaction to a message.
    AddReaction {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Message id.
        message_id: String,
        /// Reaction kind.
        kind: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Remove a reaction from a message.
    RemoveReaction {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Channel id or handle.
        channel: String,
        /// Message id.
        message_id: String,
        /// Reaction kind.
        kind: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List custom emoji registry entries.
    EmojiList {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Register a custom emoji kind.
    EmojiRegister {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Emoji kind.
        kind: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Unregister a custom emoji kind.
    EmojiUnregister {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Emoji kind.
        kind: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum DriveCmd {
    /// List one Drive folder.
    List {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Folder id.
        folder_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Stat a named child in a Drive folder.
    Stat {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Parent folder id.
        folder_id: String,
        /// Child name.
        name: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Read a Drive file version's bytes.
    Read {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// File id.
        file_id: String,
        /// Output file. Omit to write bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// List versions for one Drive file.
    ListVersions {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// File id.
        file_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List unresolved and resolved Drive conflicts.
    ListConflicts {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List Drive share grants.
    ListShares {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Grant access to a Drive file or folder.
    GrantShare {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Share grant id.
        grant_id: String,
        /// Target kind: file or folder.
        target_kind: String,
        /// Target file or folder id.
        target_id: String,
        /// Principal UUID.
        principal: String,
        /// Role: viewer, editor, or owner.
        role: String,
        /// Grant timestamp in milliseconds.
        #[arg(long)]
        granted_at_ms: u64,
        /// Optional expiry timestamp in milliseconds.
        #[arg(long)]
        expires_at_ms: Option<u64>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Revoke a Drive share grant.
    RevokeShare {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Share grant id.
        grant_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Expire Drive share grants at a timestamp.
    ApplyShareExpiry {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Timestamp in milliseconds.
        now_ms: u64,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List Drive retention pins.
    ListRetention {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Pin Drive content for retention.
    PinRetention {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Retention pin id.
        pin_id: String,
        /// Pin kind.
        kind: String,
        /// Root digest to retain.
        root: String,
        /// Optional target entity id.
        #[arg(long)]
        target_entity_id: Option<String>,
        /// Pin timestamp in milliseconds.
        #[arg(long)]
        added_at_ms: u64,
        /// Optional expiry timestamp in milliseconds.
        #[arg(long)]
        expires_at_ms: Option<u64>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Remove a Drive retention pin.
    UnpinRetention {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Retention pin id.
        pin_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Expire Drive retention pins at a timestamp.
    ApplyRetention {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Timestamp in milliseconds.
        now_ms: u64,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Create a Drive folder.
    CreateFolder {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Parent folder id.
        parent_folder_id: String,
        /// New folder id.
        folder_id: String,
        /// Folder name.
        name: String,
        /// Expected profile root.
        expected_root: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Begin a Drive file upload.
    CreateUpload {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Upload session id.
        upload_id: String,
        /// Parent folder id.
        parent_folder_id: String,
        /// File name.
        name: String,
        /// File id.
        file_id: String,
        /// Expected profile root.
        expected_root: String,
        /// Created timestamp in milliseconds.
        #[arg(long)]
        created_at_ms: u64,
        /// Replace an existing file id.
        #[arg(long)]
        replace_file: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Append bytes to a Drive upload session.
    UploadChunk {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Upload session id.
        upload_id: String,
        /// Chunk file, or `-` for standard input.
        #[arg(long, default_value = "-")]
        input: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Commit a Drive upload session.
    CommitUpload {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Upload session id.
        upload_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Rename a Drive folder entry.
    Rename {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Parent folder id.
        folder_id: String,
        /// Node id.
        node_id: String,
        /// New entry name.
        new_name: String,
        /// Expected profile root.
        expected_root: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Move a Drive folder entry.
    Move {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Source folder id.
        source_folder_id: String,
        /// Target folder id.
        target_folder_id: String,
        /// Node id.
        node_id: String,
        /// Expected profile root.
        expected_root: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Delete a Drive folder entry.
    Delete {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Parent folder id.
        folder_id: String,
        /// Node id.
        node_id: String,
        /// Expected profile root.
        expected_root: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Resolve a Drive conflict.
    ResolveConflict {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Conflict id.
        conflict_id: String,
        /// Resolution: keep-current, keep-conflict, or keep-both.
        resolution: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum InferenceCmd {
    /// Manage downloaded model artifacts and download jobs.
    Model {
        #[command(subcommand)]
        action: InferenceModelCmd,
    },
    /// Manage named configured model instances.
    Instance {
        #[command(subcommand)]
        action: InferenceInstanceCmd,
    },
    /// List installed models and durable download jobs.
    List {
        /// Hugging Face cache directory. Defaults to `HF_HOME/hub` or `~/.cache/huggingface/hub`.
        #[arg(long)]
        cache_dir: Option<String>,
        /// Show curated remote Hugging Face models instead of local state.
        #[arg(long)]
        remote: bool,
        /// Filter by model kind: `llm` or `text-embedding`.
        #[arg(long)]
        kind: Option<String>,
        /// Filter by runtime profile.
        #[arg(long)]
        runtime: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Show one download job or all durable download jobs.
    Status {
        /// Job id to inspect. Omit to show all jobs.
        job_id: Option<String>,
        /// Hugging Face cache directory. Defaults to `HF_HOME/hub` or `~/.cache/huggingface/hub`.
        #[arg(long)]
        cache_dir: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Show an installed model record.
    Show {
        /// Model kind: `llm` or `text-embedding`.
        kind: String,
        /// Hugging Face model repository id.
        repo: String,
        /// Runtime profile for the installed model record.
        #[arg(long, default_value = "candle-safetensors")]
        runtime: String,
        /// Revision reference. Prefix with `tag:` or `commit:` when it is not a branch.
        #[arg(long)]
        revision: Option<String>,
        /// Hugging Face cache directory. Defaults to `HF_HOME/hub` or `~/.cache/huggingface/hub`.
        #[arg(long)]
        cache_dir: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Download a model into the shared Hugging Face cache using the local fallback path.
    Download {
        /// Model kind: `llm` or `text-embedding`.
        kind: String,
        /// Hugging Face model repository id.
        repo: String,
        /// Files to fetch from the model repository.
        files: Vec<String>,
        /// Runtime profile for the installed model record.
        #[arg(long, default_value = "candle-safetensors")]
        runtime: String,
        /// Revision reference. Prefix with `tag:` or `commit:` when it is not a branch.
        #[arg(long)]
        revision: Option<String>,
        /// Explicit job id. Generated when omitted.
        #[arg(long)]
        job_id: Option<String>,
        /// Hugging Face cache directory. Defaults to `HF_HOME/hub` or `~/.cache/huggingface/hub`.
        #[arg(long)]
        cache_dir: Option<String>,
        /// Hugging Face token for gated or private models.
        #[arg(long)]
        token: Option<String>,
        /// Run inline even when a coordinator is added later.
        #[arg(long)]
        foreground: bool,
    },
    /// Cancel a queued or running local download job.
    Cancel {
        /// Job id to cancel.
        job_id: String,
        /// Hugging Face cache directory. Defaults to `HF_HOME/hub` or `~/.cache/huggingface/hub`.
        #[arg(long)]
        cache_dir: Option<String>,
    },
    /// Remove an installed model record and, with confirmation, its managed cache files.
    Remove {
        /// Model kind: `llm` or `text-embedding`.
        kind: String,
        /// Hugging Face model repository id.
        repo: String,
        /// Runtime profile for the installed model record.
        #[arg(long, default_value = "candle-safetensors")]
        runtime: String,
        /// Revision reference. Prefix with `tag:` or `commit:` when it is not a branch.
        #[arg(long)]
        revision: Option<String>,
        /// Hugging Face cache directory. Defaults to `HF_HOME/hub` or `~/.cache/huggingface/hub`.
        #[arg(long)]
        cache_dir: Option<String>,
        /// Print the files that would be removed without deleting them.
        #[arg(long)]
        dry_run: bool,
        /// Confirm deletion of cache files and the inventory record.
        #[arg(long)]
        yes: bool,
    },
    /// Validate and rewrite local inference inventory and job state.
    Refresh {
        /// Hugging Face cache directory. Defaults to `HF_HOME/hub` or `~/.cache/huggingface/hub`.
        #[arg(long)]
        cache_dir: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum InferenceModelCmd {
    /// List installed models, remote curated models, and durable download jobs.
    List {
        #[arg(long)]
        cache_dir: Option<String>,
        #[arg(long)]
        local: bool,
        #[arg(long)]
        remote: bool,
        #[arg(long)]
        downloads: bool,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        runtime: Option<String>,
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Show an installed model record.
    Show {
        repo: String,
        #[arg(long)]
        kind: String,
        #[arg(long, default_value = "candle-safetensors")]
        runtime: String,
        #[arg(long)]
        revision: Option<String>,
        #[arg(long)]
        cache_dir: Option<String>,
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Download a model into the shared model cache.
    Download {
        repo: String,
        files: Vec<String>,
        #[arg(long)]
        kind: String,
        #[arg(long, default_value = "candle-safetensors")]
        runtime: String,
        #[arg(long)]
        revision: Option<String>,
        #[arg(long)]
        job_id: Option<String>,
        #[arg(long)]
        cache_dir: Option<String>,
        #[arg(long)]
        token: Option<String>,
        #[arg(long)]
        foreground: bool,
    },
    /// Show one download job or all durable download jobs.
    Status {
        job_id: Option<String>,
        #[arg(long)]
        cache_dir: Option<String>,
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Cancel a queued or running local download job.
    Cancel {
        job_id: String,
        #[arg(long)]
        cache_dir: Option<String>,
    },
    /// Remove an installed model record and its managed cache files.
    Remove {
        repo: String,
        #[arg(long)]
        kind: String,
        #[arg(long, default_value = "candle-safetensors")]
        runtime: String,
        #[arg(long)]
        revision: Option<String>,
        #[arg(long)]
        cache_dir: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    /// Validate and rewrite local inference inventory and job state.
    Refresh {
        #[arg(long)]
        cache_dir: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum InferenceInstanceCmd {
    /// List named configured inference instances.
    List {
        store: String,
        workspace: String,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Show one named configured inference instance.
    Show {
        store: String,
        workspace: String,
        name: String,
        #[arg(long)]
        resolved: bool,
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Create a named configured inference instance.
    Create {
        store: String,
        workspace: String,
        name: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        kind: String,
        #[arg(long, default_value = "candle-safetensors")]
        runtime: String,
        #[arg(long)]
        preset: Option<String>,
        #[arg(long = "set")]
        settings: Vec<String>,
    },
    /// Update a named configured inference instance.
    Update {
        store: String,
        workspace: String,
        name: String,
        #[arg(long)]
        preset: Option<String>,
        #[arg(long = "set")]
        settings: Vec<String>,
    },
    /// Delete a named configured inference instance.
    Delete {
        store: String,
        workspace: String,
        name: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum DoctorCmd {
    /// Diagnose store, daemon, hardware, and inference health.
    All {
        /// Path to the `.loom` file. Omit to diagnose only local hardware and inference.
        store: Option<String>,
        /// Hugging Face cache directory. Defaults to `HF_HOME/hub` or `~/.cache/huggingface/hub`.
        #[arg(long)]
        cache_dir: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Diagnose `.loom` store maintenance, policy, and reference health.
    Store {
        /// Path to the `.loom` file.
        store: String,
    },
    /// Diagnose local coordinator daemon runtime and reachability.
    Daemon {
        /// Path to the `.loom` file.
        store: String,
    },
    /// Diagnose local inference cache, inventory, runtime, and model-fit health.
    Inference {
        /// Hugging Face cache directory. Defaults to `HF_HOME/hub` or `~/.cache/huggingface/hub`.
        #[arg(long)]
        cache_dir: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Diagnose one named configured inference instance.
    InferenceInstance {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Configured inference instance name.
        name: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum ServeCmd {
    /// Create or replace a durable listener for a served surface.
    #[command(visible_alias = "add")]
    Configure(Box<ServeConfigureArgs>),
    /// Print every stored listener as JSON.
    List {
        /// Path to the `.loom` file.
        store: String,
    },
    /// Enable a stored listener by id.
    Enable {
        /// Path to the `.loom` file.
        store: String,
        /// Listener id.
        id: String,
    },
    /// Disable a stored listener by id.
    Disable {
        /// Path to the `.loom` file.
        store: String,
        /// Listener id.
        id: String,
    },
    /// Remove a stored listener by id.
    Remove {
        /// Path to the `.loom` file.
        store: String,
        /// Listener id.
        id: String,
    },
    /// Manage Webish routes for a stored `web` listener.
    Route {
        #[command(subcommand)]
        action: ServeRouteCmd,
    },
    /// Run a foreground remote-protocol endpoint over the HTTP/2-over-TLS carrier.
    #[cfg(feature = "serve")]
    Remote(Box<ServeRemoteArgs>),
}

#[cfg(feature = "serve")]
#[derive(Args)]
pub(crate) struct ServeRemoteArgs {
    /// Path to the `.loom` file to serve.
    pub store: String,
    /// Bind address, for example `127.0.0.1:8443`.
    #[arg(long)]
    pub bind: String,
    /// Advertised service-root URL, for example `https://host:8443/apps/loom`.
    #[arg(long)]
    pub service_root: String,
    /// Concrete `cbor-h2` call endpoint URL. Defaults to `<service-root>/v1/call`.
    #[arg(long)]
    pub call_endpoint: Option<String>,
    /// PEM certificate chain file for TLS.
    #[arg(long)]
    pub tls_cert: String,
    /// PEM private key file for TLS.
    #[arg(long)]
    pub tls_key: String,
    /// Optional PEM client-trust bundle to require and verify client certificates (mTLS).
    #[arg(long)]
    pub tls_client_trust: Option<String>,
    /// Accepted auth mode(s): `interactive`, `token`, `mtls`, `principal`, or `external`. Repeatable;
    /// defaults to `interactive`.
    #[arg(long = "auth-mode")]
    pub auth_modes: Vec<String>,
    /// Advertised TLS trust selector(s): `system`, `insecure-dev`, or `bundle:NAME`. Repeatable;
    /// defaults to `system`.
    #[arg(long = "tls-trust")]
    pub tls_trust: Vec<String>,
    /// Session lease in milliseconds. Defaults to 3600000 (one hour).
    #[arg(long)]
    pub session_lease_ms: Option<u64>,
    /// Maximum accepted request body size in bytes. Defaults to 16 MiB.
    #[arg(long)]
    pub max_request_bytes: Option<u64>,
    /// Optional network-access policy name gating the listener.
    #[arg(long)]
    pub network_access_policy: Option<String>,
}

#[derive(Subcommand)]
pub(crate) enum ServeRouteCmd {
    /// Print the Webish route table for a stored listener.
    List {
        /// Path to the `.loom` file.
        store: String,
        /// Listener id.
        listener: String,
    },
    /// Create or replace a static Webish route.
    Set(Box<ServeRouteSetArgs>),
    /// Remove one Webish route by id.
    Remove {
        /// Path to the `.loom` file.
        store: String,
        /// Listener id.
        listener: String,
        /// Route id.
        route: String,
    },
}

#[derive(Args)]
pub(crate) struct ServeRouteSetArgs {
    /// Path to the `.loom` file.
    pub store: String,
    /// Listener id.
    pub listener: String,
    /// Route id.
    #[arg(long)]
    pub route: String,
    /// Optional exact host match such as `docs.example.com`.
    #[arg(long)]
    pub host: Option<String>,
    /// Public path prefix, for example `/docs`.
    #[arg(long)]
    pub prefix: String,
    /// Workspace selector for served files. Defaults to the listener workspace.
    #[arg(long)]
    pub workspace: Option<String>,
    /// Root path inside the workspace, for example `/site/docs`.
    #[arg(long)]
    pub root: String,
}

#[derive(Args)]
pub(crate) struct ServeConfigureArgs {
    /// Path to the `.loom` file.
    pub store: String,
    /// Served surface, e.g. `admin`, `mcp`, `sql`, or `vector`.
    pub surface: String,
    /// Surface selectors such as workspace, database, or collection.
    pub selector: Vec<String>,
    /// Bind address, for example `127.0.0.1:8001`.
    #[arg(long)]
    pub bind: String,
    /// Transport id, such as `rest`, `json-rpc`, `grpc`, `resp`, `text`, or a compatibility transport.
    #[arg(long)]
    pub transport: Option<String>,
    /// Surface compatibility profile, such as `generic`, `qdrant`, or `pinecone` for vector.
    #[arg(long)]
    pub profile: Option<String>,
    /// Memcached cache mode: volatile, versioned, read-through, write-through, write-around, or write-behind.
    #[arg(long)]
    pub mode: Option<String>,
    /// Store the listener disabled instead of enabled.
    #[arg(long)]
    pub disabled: bool,
    /// Stored certificate bundle name for TLS.
    #[arg(long)]
    pub tls_certificate_bundle: Option<String>,
    /// TLS mode: `direct` or `starttls`.
    #[arg(long)]
    pub tls_mode: Option<String>,
    /// Authentication mode: `owner-or-passphrase` or `passphrase`.
    #[arg(long)]
    pub auth_mode: Option<String>,
    /// Exposure mode: `read-only` or `read-write`.
    #[arg(long)]
    pub exposure: Option<String>,
    /// Audit mode: `management-and-security` or `all`.
    #[arg(long)]
    pub audit_mode: Option<String>,
    /// Maximum request body size in bytes.
    #[arg(long)]
    pub request_size_limit: Option<u64>,
    /// Idle timeout in milliseconds.
    #[arg(long)]
    pub idle_timeout_ms: Option<u64>,
    /// Session timeout in milliseconds.
    #[arg(long)]
    pub session_timeout_ms: Option<u64>,
    /// Network access policy name applied before hosted listener authentication.
    #[arg(long)]
    pub network_access_policy: Option<String>,
}

#[derive(Subcommand)]
pub(crate) enum NetworkAccessCmd {
    /// List stored network access policies.
    List {
        /// Path to the `.loom` file.
        store: String,
    },
    /// Create or replace a network access policy.
    Set(Box<NetworkAccessSetArgs>),
    /// Remove a network access policy by name.
    Remove {
        /// Path to the `.loom` file.
        store: String,
        /// Network access policy name.
        name: String,
    },
    /// Inspect one network access policy with references and digest.
    Audit {
        /// Path to the `.loom` file.
        store: String,
        /// Network access policy name.
        name: String,
    },
}

#[derive(Args)]
pub(crate) struct NetworkAccessSetArgs {
    /// Path to the `.loom` file.
    pub store: String,
    /// Network access policy name.
    pub name: String,
    /// Optional policy description.
    #[arg(long)]
    pub description: Option<String>,
    /// Default decision when no rule matches: `allow` or `deny`.
    #[arg(long, default_value = "deny")]
    pub default_action: String,
    /// Allow traffic from a source CIDR. Repeat for multiple rules.
    #[arg(long = "allow-source")]
    pub allow_sources: Vec<String>,
    /// Deny traffic from a source CIDR. Repeat for multiple rules.
    #[arg(long = "deny-source")]
    pub deny_sources: Vec<String>,
    /// Allow traffic when a client certificate is present.
    #[arg(long = "allow-mtls")]
    pub allow_mtls: bool,
    /// Deny traffic when a client certificate is present.
    #[arg(long = "deny-mtls")]
    pub deny_mtls: bool,
    /// Allow traffic from a client certificate subject substring.
    #[arg(long = "allow-mtls-subject")]
    pub allow_mtls_subjects: Vec<String>,
    /// Deny traffic from a client certificate subject substring.
    #[arg(long = "deny-mtls-subject")]
    pub deny_mtls_subjects: Vec<String>,
    /// Allow traffic from a client certificate SAN substring.
    #[arg(long = "allow-mtls-san")]
    pub allow_mtls_sans: Vec<String>,
    /// Deny traffic from a client certificate SAN substring.
    #[arg(long = "deny-mtls-san")]
    pub deny_mtls_sans: Vec<String>,
    /// Allow traffic from a client certificate issuer substring.
    #[arg(long = "allow-mtls-issuer")]
    pub allow_mtls_issuers: Vec<String>,
    /// Deny traffic from a client certificate issuer substring.
    #[arg(long = "deny-mtls-issuer")]
    pub deny_mtls_issuers: Vec<String>,
    /// Trust forwarded source headers only from this proxy CIDR.
    #[arg(long = "trusted-proxy")]
    pub trusted_proxies: Vec<String>,
    /// JSON file containing an array of explicit network access rules.
    #[arg(long = "rules")]
    pub rules: Option<String>,
}

#[derive(Subcommand)]
pub(crate) enum AuditCmd {
    /// Compact audit entries through a sequence number.
    Compact {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long)]
        through_seq: u64,
    },
    /// Inspect or update audit retention settings.
    Config {
        #[command(subcommand)]
        action: AuditConfigCmd,
    },
    /// List retained audit entries as a table.
    List {
        /// Path to the `.loom` file.
        store: String,
    },
    /// Print one retained audit entry as JSON.
    View {
        /// Path to the `.loom` file.
        store: String,
        /// Audit record sequence number.
        record: String,
    },
}

#[cfg(feature = "mcp")]
#[derive(Subcommand)]
pub(crate) enum RefsCmd {
    /// Run one bounded keyed reconciliation batch.
    Reconcile {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name. Defaults to `Default`.
        #[arg(long, default_value = "Default")]
        workspace: String,
        /// Maximum due candidates to process.
        #[arg(long, default_value_t = 100)]
        max: usize,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Show pending, resolved, and terminal reference-reconciliation counts.
    Status {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name. Defaults to `Default`.
        #[arg(long, default_value = "Default")]
        workspace: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum AuditConfigCmd {
    /// Print the audit retention settings as JSON.
    Show {
        /// Path to the `.loom` file.
        store: String,
    },
    /// Update audit retention settings.
    Set {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long)]
        retention_days: Option<u32>,
        #[arg(long)]
        legal_hold: Option<bool>,
    },
}

#[derive(Subcommand)]
pub(crate) enum CertificateCmd {
    /// List stored certificate bundles.
    List {
        /// Path to the `.loom` file.
        store: String,
    },
    /// Copy certificate material into the Loom store under a bundle name.
    Import {
        /// Path to the `.loom` file.
        store: String,
        /// Certificate bundle name.
        name: String,
        /// PEM certificate chain for direct TLS.
        #[arg(long = "cert-chain")]
        cert_chain: String,
        /// PEM private key for direct TLS.
        #[arg(long = "private-key")]
        private_key: String,
        /// Optional PEM trust bundle for client trust policy.
        #[arg(long = "trust-bundle")]
        trust_bundle: Option<String>,
        /// Permit private-key import into an unencrypted store.
        #[arg(long)]
        force: bool,
    },
    /// Copy certificate material from the Loom store to files.
    Export {
        /// Path to the `.loom` file.
        store: String,
        /// Certificate bundle name.
        name: String,
        /// Output PEM certificate chain path.
        #[arg(long = "cert-chain")]
        cert_chain: Option<String>,
        /// Output PEM private key path. Requires `--force`.
        #[arg(long = "private-key")]
        private_key: Option<String>,
        /// Output PEM trust bundle path.
        #[arg(long = "trust-bundle")]
        trust_bundle: Option<String>,
        /// Permit overwriting output files and exporting private keys.
        #[arg(long)]
        force: bool,
    },
    /// Generate new certificate material in the Loom store.
    Generate {
        #[command(subcommand)]
        action: CertificateGenerateCmd,
    },
    /// Remove a certificate bundle by name.
    Remove {
        /// Path to the `.loom` file.
        store: String,
        /// Certificate bundle name.
        name: String,
    },
    /// Inspect safe certificate metadata for a bundle.
    Audit {
        /// Path to the `.loom` file.
        store: String,
        /// Certificate bundle name.
        name: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum InterchangeCmd {
    /// Import a host directory into a workspace file tree.
    ImportFs {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Host directory to import.
        src: String,
        /// Commit after importing.
        #[arg(long)]
        commit: bool,
        /// Report the planned import without writing to the Loom.
        #[arg(long)]
        dry_run: bool,
        /// Commit author when `--commit` is used.
        #[arg(long, default_value = "loom-interchange")]
        author: String,
        /// Commit message when `--commit` is used.
        #[arg(long, default_value = "import filesystem")]
        message: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Import a zip, tar, or gzip archive into a workspace file tree.
    ImportArchive {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Host archive to import.
        archive: String,
        /// Archive kind: zip, tar, or gzip.
        #[arg(long)]
        kind: String,
        /// Destination path for a single-file gzip import.
        #[arg(long)]
        gzip_output_path: Option<String>,
        /// Commit after importing.
        #[arg(long)]
        commit: bool,
        /// Report the planned import without writing to the Loom.
        #[arg(long)]
        dry_run: bool,
        /// Commit author when `--commit` is used.
        #[arg(long, default_value = "loom-interchange")]
        author: String,
        /// Commit message when `--commit` is used.
        #[arg(long, default_value = "import archive")]
        message: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Import a schema-driven CSV file into a SQL table.
    ImportTableCsv {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// SQL database name.
        database: String,
        /// SQL table name.
        table: String,
        /// Host CSV file to import.
        csv: String,
        /// Comma-separated schema, for example `id:int,name:text,amount:decimal`.
        #[arg(long)]
        schema: String,
        /// Comma-separated primary-key column names.
        #[arg(long)]
        primary_key: String,
        /// Import mode: snapshot or append-only.
        #[arg(long, default_value = "snapshot")]
        mode: String,
        /// Commit after importing.
        #[arg(long)]
        commit: bool,
        /// Report the planned import without writing to the Loom.
        #[arg(long)]
        dry_run: bool,
        /// Commit author when `--commit` is used.
        #[arg(long, default_value = "loom-interchange")]
        author: String,
        /// Commit message when `--commit` is used.
        #[arg(long, default_value = "import table csv")]
        message: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Import a normalized Redmine snapshot into the ticket profile.
    ImportRedmine {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Ticket profile workspace id.
        profile: String,
        /// Normalized Redmine snapshot JSON file.
        snapshot: String,
        /// Report the planned import without writing to the Loom.
        #[arg(long)]
        dry_run: bool,
        /// Unknown ticket field policy: strict or infer.
        #[arg(long, default_value = "strict")]
        field_policy: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Import a normalized Asana snapshot into the ticket profile.
    ImportAsana {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Ticket profile workspace id.
        profile: String,
        /// Normalized Asana snapshot JSON file.
        snapshot: String,
        /// Report the planned import without writing to the Loom.
        #[arg(long)]
        dry_run: bool,
        /// Unknown ticket field policy: strict or infer.
        #[arg(long, default_value = "strict")]
        field_policy: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Import a normalized Jira snapshot into the ticket profile.
    ImportJira {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Ticket profile workspace id.
        profile: String,
        /// Normalized Jira snapshot JSON file.
        snapshot: String,
        /// Report the planned import without writing to the Loom.
        #[arg(long)]
        dry_run: bool,
        /// Unknown ticket field policy: strict or infer.
        #[arg(long, default_value = "strict")]
        field_policy: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Import a normalized Confluence snapshot into the pages profile.
    ImportConfluence {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Pages profile workspace id.
        profile: String,
        /// Normalized Confluence snapshot JSON file.
        snapshot: String,
        /// Default pages space id.
        #[arg(long, default_value = "confluence")]
        space: String,
        /// Report the planned import without writing to the Loom.
        #[arg(long)]
        dry_run: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Import a normalized Slack snapshot into the chat profile.
    ImportSlack {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Chat profile workspace id.
        profile: String,
        /// Normalized Slack snapshot JSON file.
        snapshot: String,
        /// Report the planned import without writing to the Loom.
        #[arg(long)]
        dry_run: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Import a normalized Drive or SharePoint snapshot into the Drive profile.
    ImportDrive {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Drive profile workspace id.
        profile: String,
        /// Normalized Drive snapshot JSON file.
        snapshot: String,
        /// Report the planned import without writing to the Loom.
        #[arg(long)]
        dry_run: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Import a Markdown directory into the pages profile.
    ImportMarkdown {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Pages profile workspace id.
        profile: String,
        /// Host directory containing Markdown files.
        src: String,
        /// Pages space id to receive imported pages.
        #[arg(long, default_value = "markdown")]
        space: String,
        /// Report the planned import without writing to the Loom.
        #[arg(long)]
        dry_run: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Import a normalized Notion snapshot into the pages profile.
    ImportNotion {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Pages profile workspace id.
        profile: String,
        /// Normalized Notion snapshot JSON file.
        snapshot: String,
        /// Default pages space id.
        #[arg(long, default_value = "notion")]
        space: String,
        /// Report the planned import without writing to the Loom.
        #[arg(long)]
        dry_run: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Export a workspace file tree to a zip, tar, tar.gz, or tar.zstd archive.
    ExportArchive {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Host archive to write.
        archive: String,
        /// Archive kind: tar-zstd, tar, tar-gzip, or zip.
        #[arg(long)]
        kind: String,
        /// Revision to export: HEAD, commit:<digest>, branch:<name>, tag:<name>, a bare branch, or a bare digest.
        #[arg(long)]
        revision: Option<String>,
        /// Report the planned export without writing the host archive.
        #[arg(long)]
        dry_run: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Export the current workspace file tree to a host directory.
    ExportFs {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Host directory to write.
        dst: String,
        /// Revision to export: HEAD, commit:<digest>, branch:<name>, tag:<name>, a bare branch, or a bare digest.
        #[arg(long)]
        revision: Option<String>,
        /// Report the planned export without writing host files.
        #[arg(long)]
        dry_run: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Export a SQL table to a schema-driven CSV file.
    ExportTableCsv {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// SQL database name.
        database: String,
        /// SQL table name.
        table: String,
        /// Host CSV file to write.
        csv: String,
        /// Report the planned export without writing the host CSV file.
        #[arg(long)]
        dry_run: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Export a workspace object graph to a deterministic CAR file.
    ExportCar {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Host CAR file to write.
        dst: String,
        /// Report the planned export without writing the CAR file.
        #[arg(long)]
        dry_run: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Import a deterministic Loom CAR file into a store.
    ImportCar {
        /// Path to the `.loom` file.
        store: String,
        /// Host CAR file to import.
        src: String,
        /// Report the planned import without writing to the Loom.
        #[arg(long)]
        dry_run: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum MeetingsCmd {
    /// List meetings in a workspace profile.
    List {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Maximum number of meetings to return.
        #[arg(long, default_value_t = 100)]
        limit: usize,
        /// Number of meetings to skip.
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Get one meeting from a workspace profile.
    Get {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Meeting id.
        meeting_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Search materialized Meetings projection text.
    Search {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Free-text query.
        query: String,
        /// Optional text field filter.
        #[arg(long)]
        field: Option<String>,
        /// Maximum hits to print.
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Hits to skip before printing.
        #[arg(long, default_value_t = 0)]
        offset: u32,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Read a retained source payload, such as source.json, summary.txt, or transcript.jsonl.
    SourceRead {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Source entity id.
        source_id: String,
        /// Payload leaf: source.json, summary.txt, or transcript.jsonl.
        leaf: String,
        /// Output file. Omit to write bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Import a normalized Meetings snapshot into the store.
    Import {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Input profile: generic, granola-api, granola-app, granola-mcp, or csv.
        #[arg(long)]
        input_profile: String,
        /// Normalized snapshot JSON path, or `-` for standard input.
        #[arg(long)]
        input: String,
        /// Validate and report without writing.
        #[arg(long)]
        dry_run: bool,
        /// Report output format: text or json.
        #[arg(long, default_value = "text")]
        report_format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum TicketsCmd {
    /// Create a ticket project and key prefix.
    ProjectCreate {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Project id.
        project_id: String,
        /// Ticket key prefix.
        key_prefix: String,
        /// Project display name.
        name: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Rekey a ticket project.
    ProjectRekey {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Project id.
        project_id: String,
        /// New ticket key prefix.
        key_prefix: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Read a ticket project's settings.
    ProjectSettingsGet {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Project id.
        project_id: String,
        /// Include full contract details in the JSON/text output.
        #[arg(long)]
        include_contracts: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Update a ticket project's settings.
    ProjectSettingsSet {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Project id.
        project_id: String,
        /// Default projection for human display: native, jira, asana, notion, or redmine.
        #[arg(long)]
        default_projection: Option<String>,
        /// Actor enforcement mode: write-access, assignee, review-authority, or ownership-governed.
        #[arg(long = "actor-enforcement")]
        actor_enforcement: Option<String>,
        /// Project owner principal for review-gated policies.
        #[arg(long)]
        project_owner: Option<String>,
        /// Clear the project owner principal.
        #[arg(long)]
        clear_project_owner: bool,
        /// Acceptance authority principal. Repeat for multiple principals.
        #[arg(long = "acceptance-authority")]
        acceptance_authorities: Vec<String>,
        /// Replace acceptance authorities with the provided list.
        #[arg(long)]
        replace_acceptance_authorities: bool,
        /// Enable or disable required acceptance evidence validation for this project.
        #[arg(long = "acceptance-evidence-enforcement")]
        acceptance_evidence_enforcement: Option<bool>,
        /// Required acceptance evidence key. Repeat for multiple keys.
        #[arg(long = "required-acceptance-evidence-key")]
        required_acceptance_evidence_keys: Vec<String>,
        /// Replace required acceptance evidence keys with the provided list.
        #[arg(long)]
        replace_required_acceptance_evidence_keys: bool,
        /// Project owner contract summary.
        #[arg(long)]
        owner_contract_summary: Option<String>,
        /// Project owner contract details markdown.
        #[arg(long)]
        owner_contract_details: Option<String>,
        /// Project worker contract summary.
        #[arg(long)]
        worker_contract_summary: Option<String>,
        /// Project worker contract details markdown.
        #[arg(long)]
        worker_contract_details: Option<String>,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List ticket projects with key prefix, name, and default projection.
    Projects {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List a ticket's relations (outgoing and incoming) with kind, target ticket id, and title.
    Relations {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Ticket id.
        ticket_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Discover settable ticket fields, projection paths, types, limits, and enum values.
    Fields {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Optional project id for project custom fields.
        #[arg(long)]
        project_id: Option<String>,
        /// Ticket projection profile: native, jira, asana, notion, or redmine.
        #[arg(long)]
        projection: Option<String>,
        /// Operation context: create, update, or write.
        #[arg(long)]
        operation: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Create or update a project custom-field definition.
    FieldPut {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Project id.
        project_id: String,
        /// Stable field id.
        field_id: String,
        /// Write key.
        key: String,
        /// Display name.
        name: String,
        /// Field type: string, integer, number, boolean, date, datetime, duration, principal, enum, url, or opaque_json.
        #[arg(long = "type")]
        field_type: String,
        /// Option set id for enum fields.
        #[arg(long)]
        option_set: Option<String>,
        /// Field description.
        #[arg(long)]
        description: Option<String>,
        /// Maximum string length.
        #[arg(long)]
        max_length: Option<u32>,
        /// Require the field on create.
        #[arg(long, default_value_t = false)]
        required: bool,
        /// Field is searchable.
        #[arg(long, default_value_t = true)]
        searchable: bool,
        /// Field is orderable.
        #[arg(long, default_value_t = false)]
        orderable: bool,
        /// Cardinality: single, optional, or list.
        #[arg(long, default_value = "optional")]
        cardinality: String,
        /// Applicable ticket type id. Repeat for multiple types.
        #[arg(long = "type-id")]
        applicable_type_ids: Vec<String>,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Retire a project custom-field definition.
    FieldRetire {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Project id.
        project_id: String,
        /// Stable field id.
        field_id: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Create a ticket, using first-class flags for common fields and `--fields` for the rest.
    Create {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Ticket type (for example `task`).
        ticket_type: String,
        /// Project id. Defaults to the workspace's sole project; required when there is more than one.
        #[arg(long)]
        project_id: Option<String>,
        /// Canonical field: ticket title.
        #[arg(long)]
        title: Option<String>,
        /// Canonical field: ticket description.
        #[arg(long)]
        description: Option<String>,
        /// Canonical field: ticket priority.
        #[arg(long)]
        priority: Option<String>,
        /// Canonical field: ticket assignee.
        #[arg(long)]
        assignee: Option<String>,
        /// Project custom fields as a JSON object, or `@path` / `@-` to read from a file or stdin.
        #[arg(long, default_value = "{}")]
        fields: String,
        /// Input projection profile: native, jira, asana, notion, or redmine. Defaults to the project's default projection.
        #[arg(long)]
        projection: Option<String>,
        /// External source system name.
        #[arg(long)]
        external_source: Option<String>,
        /// External source id.
        #[arg(long)]
        external_id: Option<String>,
        /// Policy label to attach. Repeat for multiple labels.
        #[arg(long = "policy-label")]
        policy_labels: Vec<String>,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Apply one atomic ticket update request.
    Update {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name. Required unless --request is supplied.
        workspace: Option<String>,
        /// Ticket UUID or primary key. Required unless --request is supplied.
        ticket_id: Option<String>,
        /// JSON request, or `@path` / `@-` to read JSON from a file or standard input.
        #[arg(long)]
        request: Option<String>,
        /// Input projection profile: native, jira, asana, notion, or redmine.
        #[arg(long)]
        projection: Option<String>,
        /// Canonical field: ticket status.
        #[arg(long)]
        status: Option<String>,
        /// Canonical field: ticket assignee.
        #[arg(long)]
        assignee: Option<String>,
        /// Canonical field: ticket title.
        #[arg(long)]
        title: Option<String>,
        /// Canonical field: ticket description.
        #[arg(long)]
        description: Option<String>,
        /// Canonical field: ticket priority.
        #[arg(long)]
        priority: Option<String>,
        /// Custom or projected field as key=value. Repeat for multiple fields.
        #[arg(long = "field")]
        fields: Vec<String>,
        /// Delete a field. Repeat for multiple fields.
        #[arg(long = "delete-field")]
        delete_fields: Vec<String>,
        /// Lifecycle action to apply with the update.
        #[arg(long)]
        action: Option<String>,
        /// Add this authenticated comment body as part of the same ticket update. Supports @path / @-.
        #[arg(long)]
        comment_body: Option<String>,
        /// Optional stable id for --comment-body.
        #[arg(long)]
        comment_id: Option<String>,
        /// Optional type for --comment-body. Defaults to general.
        #[arg(long)]
        comment_type: Option<String>,
        /// Structured evidence JSON object for --comment-body. Supports @path / @-.
        #[arg(long = "comment-evidence")]
        comment_evidence: Option<String>,
        /// Observed source status for lifecycle transition validation.
        #[arg(long)]
        observed_source_status: Option<String>,
        /// Observed workflow version for lifecycle transition validation.
        #[arg(long)]
        observed_workflow_version: Option<String>,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Delete a ticket as an audited tombstone operation.
    Delete {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Ticket UUID or primary key.
        ticket_id: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List comments for a ticket.
    Comments {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Ticket UUID or primary key.
        ticket_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Add an authenticated comment to a ticket.
    CommentAdd {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Ticket UUID or primary key.
        ticket_id: String,
        /// Comment body, or `@path` / `@-` to read UTF-8 text.
        body: String,
        /// Optional stable comment id.
        #[arg(long)]
        comment_id: Option<String>,
        /// Comment type.
        #[arg(long, default_value = "general")]
        comment_type: String,
        /// Structured evidence JSON object. Supports @path / @-.
        #[arg(long)]
        evidence: Option<String>,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Update an authenticated ticket comment body and/or type.
    CommentUpdate {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Ticket UUID or primary key.
        ticket_id: String,
        /// Comment id.
        comment_id: String,
        /// New comment body, or `@path` / `@-` to read UTF-8 text.
        #[arg(long)]
        body: Option<String>,
        /// New comment type.
        #[arg(long)]
        comment_type: Option<String>,
        /// Replace structured evidence with a JSON object, or null to clear it. Supports @path / @-.
        #[arg(long)]
        evidence: Option<String>,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Delete a ticket comment by redacting its body while retaining audit metadata.
    CommentDelete {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Ticket UUID or primary key.
        ticket_id: String,
        /// Comment id.
        comment_id: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Create a first-class Ticket Board.
    BoardCreate {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Stable Board id.
        board_id: String,
        /// Human-facing Board key.
        board_key: String,
        /// Project id.
        project_id: String,
        /// Board display name.
        name: String,
        /// Board mode: status_mapped or manual.
        #[arg(long, default_value = "status_mapped")]
        mode: String,
        /// Board description.
        #[arg(long, default_value = "")]
        description: String,
        /// Column as column_id:name[:status,status][:rank]. Repeat for multiple columns.
        #[arg(long = "column")]
        columns: Vec<String>,
        /// Display field. Repeat for multiple fields.
        #[arg(long = "card-field")]
        card_display_fields: Vec<String>,
        /// Updated-by actor.
        #[arg(long, default_value = "cli")]
        updated_by: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Get a first-class Ticket Board.
    BoardGet {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Stable Board id.
        board_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List first-class Ticket Boards.
    BoardList {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Include deleted Board tombstones.
        #[arg(long)]
        include_deleted: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Update first-class Ticket Board metadata.
    BoardUpdate {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Stable Board id.
        board_id: String,
        /// Human-facing Board key.
        #[arg(long)]
        board_key: Option<String>,
        /// Board display name.
        #[arg(long)]
        name: Option<String>,
        /// Board description.
        #[arg(long)]
        description: Option<String>,
        /// Board status: active, archived, or deleted.
        #[arg(long)]
        board_status: Option<String>,
        /// Display field. Repeat to replace the display field list.
        #[arg(long = "card-field")]
        card_display_fields: Vec<String>,
        /// Updated-by actor.
        #[arg(long, default_value = "cli")]
        updated_by: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Delete a first-class Ticket Board as a tombstone.
    BoardDelete {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Stable Board id.
        board_id: String,
        /// Updated-by actor.
        #[arg(long, default_value = "cli")]
        updated_by: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Replace first-class Ticket Board columns.
    BoardConfigureColumns {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Stable Board id.
        board_id: String,
        /// Board mode: status_mapped or manual.
        #[arg(long)]
        mode: Option<String>,
        /// Column as column_id:name[:status,status][:rank]. Repeat for multiple columns.
        #[arg(long = "column")]
        columns: Vec<String>,
        /// Updated-by actor.
        #[arg(long, default_value = "cli")]
        updated_by: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Move or reorder a Board card.
    BoardMoveCard {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Stable Board id.
        board_id: String,
        /// Ticket UUID or primary key.
        ticket_id: String,
        /// Target column id.
        column_id: String,
        /// Sparse rank token inside the target column.
        rank_token: String,
        /// Optional swimlane id.
        #[arg(long)]
        swimlane_id: Option<String>,
        /// Updated-by actor.
        #[arg(long, default_value = "cli")]
        updated_by: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Add or replace a ticket-owned typed relation.
    RelationSet {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Source ticket UUID or primary key.
        ticket_id: String,
        /// Relation kind, such as depends_on, blocks, references_page, or references_document.
        kind: String,
        /// Target id. Ticket targets may be UUIDs or primary keys.
        target_id: String,
        /// Caller-supplied relation id. Defaults to kind:target_type:target_id.
        #[arg(long)]
        relation_id: Option<String>,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Remove a ticket-owned typed relation.
    RelationRemove {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Source ticket UUID or primary key.
        ticket_id: String,
        /// Relation id to remove.
        relation_id: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List tickets in a workspace.
    List {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Ticket projection profile: native, jira, asana, notion, or redmine.
        #[arg(long)]
        projection: Option<String>,
        /// Filter by status (repeatable).
        #[arg(long = "status")]
        statuses: Vec<String>,
        /// Filter by assignee (repeatable).
        #[arg(long = "assignee")]
        assignees: Vec<String>,
        /// Filter by priority (repeatable).
        #[arg(long = "priority")]
        priorities: Vec<String>,
        /// Filter by ticket type (repeatable).
        #[arg(long = "type")]
        ticket_types: Vec<String>,
        /// Filter by ordinary label (repeatable).
        #[arg(long = "label")]
        labels: Vec<String>,
        /// Filter by policy label (repeatable).
        #[arg(long = "policy-label")]
        policy_labels: Vec<String>,
        /// Restrict to members of this Lane.
        #[arg(long)]
        lane: Option<String>,
        /// Restrict to cards on this Board by id, key, or name.
        #[arg(long)]
        board: Option<String>,
        /// Only dependency-ready, actionable tickets.
        #[arg(long)]
        ready: bool,
        /// Include terminal tickets. Lane- and Board-scoped lists hide them by default.
        #[arg(long)]
        include_completed: bool,
        /// Maximum tickets to return (default 25, hard cap 100).
        #[arg(long)]
        limit: Option<usize>,
        /// Opaque continuation cursor from a previous page.
        #[arg(long)]
        cursor: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Get one ticket by UUID or primary key.
    Get {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Ticket UUID or primary key.
        ticket_id: String,
        /// Ticket projection profile: native, jira, asana, notion, or redmine.
        #[arg(long)]
        projection: Option<String>,
        /// Include complete ticket metadata in text output.
        #[arg(long)]
        detailed: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List ticket operation history.
    History {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Optional ticket UUID or primary key.
        #[arg(long)]
        ticket_id: Option<String>,
        /// Include complete operation envelope metadata in text output.
        #[arg(long)]
        detailed: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

// `Create` carries every first-class Lane field, so it is naturally much larger than the small
// read/mutate variants. This is a one-shot CLI command enum (parsed once, never stored in bulk), so
// the size difference is not a real performance concern; boxing clap arg fields would only hurt
// ergonomics.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
pub(crate) enum LanesCmd {
    /// Create one Lane record.
    Create {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Lane document id.
        lane_id: String,
        /// Human-stable lane key.
        lane_key: String,
        /// Lane kind: assignment or tracking.
        #[arg(long)]
        kind: String,
        /// Optional Lane coordinator principal.
        #[arg(long)]
        owner_principal: Option<String>,
        /// Concise human display label for the lane.
        #[arg(long, default_value = "")]
        title: String,
        /// Durable statement of the lane's intention and goal.
        #[arg(long, default_value = "")]
        description: String,
        /// Initial Lane status.
        #[arg(long, default_value = "ready")]
        lane_status: String,
        /// Initial active ticket id.
        #[arg(long)]
        active_ticket_id: Option<String>,
        /// Initial status report.
        #[arg(long, default_value = "")]
        status_report: String,
        /// Initial reviewer feedback.
        #[arg(long, default_value = "")]
        reviewer_feedback: String,
        /// Updated timestamp in milliseconds. Defaults to current time.
        #[arg(long)]
        updated_at: Option<u64>,
        /// Actor principal override. Omit to derive from the authenticated principal.
        #[arg(long)]
        updated_by: Option<String>,
        /// Ticket membership in display order. Repeat to set multiple tickets.
        #[arg(long = "ticket")]
        tickets: Vec<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Read one Lane record.
    Get {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Lane document id.
        lane_id: String,
        /// Include stored status, owner, update metadata, reports, feedback, and compact ticket summaries.
        #[arg(long)]
        detailed: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List Lane records.
    List {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Include stored status, owner, update metadata, reports, feedback, and compact ticket summaries.
        #[arg(long)]
        detailed: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Atomically update one or more first-class Lane fields.
    Update {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Lane document id.
        lane_id: String,
        /// New title. Omit to leave unchanged; pass an empty string to clear it.
        #[arg(long)]
        title: Option<String>,
        /// New description. Omit to leave unchanged; pass an empty string to clear it.
        #[arg(long)]
        description: Option<String>,
        /// New stored Lane status.
        #[arg(long)]
        lane_status: Option<String>,
        /// New status report. Omit to leave unchanged; pass an empty string to clear it.
        #[arg(long)]
        status_report: Option<String>,
        /// New reviewer feedback. Omit to leave unchanged; pass an empty string to clear it.
        #[arg(long)]
        reviewer_feedback: Option<String>,
        /// Actor principal override. Omit to derive from the authenticated principal.
        #[arg(long)]
        updated_by: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Add one ticket to Lane membership.
    TicketAdd {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Lane document id.
        lane_id: String,
        /// Ticket id.
        ticket_id: String,
        /// Place the ticket first in the lane. Mutually exclusive with --before/--after.
        #[arg(long)]
        first: bool,
        /// Place the ticket immediately before this anchor ticket id.
        #[arg(long)]
        before: Option<String>,
        /// Place the ticket immediately after this anchor ticket id.
        #[arg(long)]
        after: Option<String>,
        /// Actor principal override. Omit to derive from the authenticated principal.
        #[arg(long)]
        updated_by: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Remove one ticket from Lane membership.
    TicketRemove {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Lane document id.
        lane_id: String,
        /// Ticket id.
        ticket_id: String,
        /// Actor principal override. Omit to derive from the authenticated principal.
        #[arg(long)]
        updated_by: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Transfer a ticket between assignment Lanes without mutating the ticket.
    TicketTransfer {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Source assignment Lane document id.
        source_lane_id: String,
        /// Target assignment Lane document id.
        target_lane_id: String,
        /// Ticket id.
        ticket_id: String,
        /// Actor principal for the update.
        #[arg(long)]
        updated_by: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Delete a closed Lane coordination record without mutating tickets.
    Delete {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Closed Lane document id.
        lane_id: String,
        /// Actor principal for the update.
        #[arg(long)]
        updated_by: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum PagesCmd {
    /// Create a page space.
    SpaceCreate {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Space id.
        space_id: String,
        /// Space title.
        title: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List page spaces.
    SpaceList {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Get one page space.
    SpaceGet {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Space id.
        space_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Create a page.
    Create {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Page id.
        page_id: String,
        /// Space id.
        space_id: String,
        /// Page title.
        title: String,
        /// Optional parent page id.
        #[arg(long)]
        parent_page_id: Option<String>,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Update the current principal's page draft body.
    Update {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Page id.
        page_id: String,
        /// Body text, or `@path` / `@-` to read UTF-8 text from a file or standard input.
        body: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Publish the current principal's page draft.
    Publish {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Page id.
        page_id: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Get one page.
    Get {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Page id.
        page_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List page history.
    History {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Page id.
        page_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Create a page structure.
    StructureCreate {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Structure id.
        structure_id: String,
        /// Space id.
        space_id: String,
        /// Structure kind.
        kind: String,
        /// Structure title.
        title: String,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Get a page structure render projection.
    StructureGet {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Structure id.
        structure_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Add a node to a page structure.
    StructureAddNode {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Structure id.
        structure_id: String,
        /// Node id.
        node_id: String,
        /// Node kind.
        kind: String,
        /// Node label.
        label: String,
        /// Optional body digest.
        #[arg(long)]
        body_digest: Option<String>,
        /// Optional entity ref.
        #[arg(long)]
        entity_ref: Option<String>,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Update a node in a page structure.
    StructureUpdateNode {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Structure id.
        structure_id: String,
        /// Node id.
        node_id: String,
        /// Node kind.
        kind: String,
        /// Node label.
        label: String,
        /// Optional body digest.
        #[arg(long)]
        body_digest: Option<String>,
        /// Optional entity ref.
        #[arg(long)]
        entity_ref: Option<String>,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Bind or clear a node entity ref in a page structure.
    StructureBind {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Structure id.
        structure_id: String,
        /// Node id.
        node_id: String,
        /// Optional entity ref. Omit to clear the binding.
        #[arg(long)]
        entity_ref: Option<String>,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Move a node under a parent in a page structure.
    StructureMoveNode {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Structure id.
        structure_id: String,
        /// Node id.
        node_id: String,
        /// Optional parent node id. Omit to detach from the parent edge.
        #[arg(long)]
        parent_node_id: Option<String>,
        /// Optional edge label. Defaults to child_of.
        #[arg(long)]
        label: Option<String>,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Link two nodes in a page structure.
    StructureLinkNode {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Structure id.
        structure_id: String,
        /// Edge id.
        edge_id: String,
        /// Source node id.
        src_node_id: String,
        /// Destination node id.
        dst_node_id: String,
        /// Edge label.
        label: String,
        /// Optional target ref.
        #[arg(long)]
        target_ref: Option<String>,
        /// Expected profile root for optimistic concurrency.
        #[arg(long)]
        expected_root: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Create tickets from selected structure nodes.
    StructureDecomposeToTickets {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Structure id.
        structure_id: String,
        /// JSON array, or `@path` / `@-`, of node decomposition items.
        items: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum LifecycleCmd {
    /// Define one of the built-in lifecycle templates.
    DefineStandard {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Standard lifecycle kind: feature, bug, incident, or design.
        kind: String,
        /// Definition version string.
        version: String,
        /// Completion predicate digest.
        completion_predicate_digest: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Define a lifecycle from canonical CBOR bytes.
    Define {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Canonical CBOR lifecycle definition path, or `-` for standard input.
        definition: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List lifecycle definitions.
    Definitions {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Get one lifecycle definition.
    Definition {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Definition id.
        definition_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Instantiate a lifecycle for one or more subject refs.
    Instantiate {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Instance id.
        instance_id: String,
        /// Definition id.
        definition_id: String,
        /// Subject ref. Repeat for multiple refs.
        #[arg(long = "subject-ref")]
        subject_refs: Vec<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List lifecycle instances.
    Instances {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Get one lifecycle instance.
    Instance {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Instance id.
        instance_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Transition a lifecycle instance.
    Transition {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Instance id.
        instance_id: String,
        /// Transition id.
        transition_id: String,
        /// Destination stage id.
        to_stage_id: String,
        /// Actor principal id. Defaults to the resolved workspace id.
        #[arg(long)]
        actor_principal_id: Option<String>,
        /// JSON gate-evaluation array, or `@path` / `@-`.
        #[arg(long, default_value = "[]")]
        gate_evaluations: String,
        /// Optional snapshot digest to bind to this transition.
        #[arg(long)]
        snapshot_digest: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Show the snapshot plan for an instance transition.
    SnapshotPlan {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Instance id.
        instance_id: String,
        /// Destination stage id.
        to_stage_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Show the current stage surface for an instance.
    CurrentSurface {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Instance id.
        instance_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// List lifecycle snapshots.
    Snapshots {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Get one lifecycle snapshot.
    Snapshot {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Snapshot id.
        snapshot_id: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Read stored lifecycle snapshot content.
    SnapshotContent {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Snapshot id.
        snapshot_id: String,
        /// Output path. Omit to write bytes to standard output.
        #[arg(long)]
        out: Option<String>,
    },
    /// Show the lifecycle operation log.
    OperationLog {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum CertificateGenerateCmd {
    /// Generate a self-signed certificate bundle.
    #[command(name = "self-signed")]
    SelfSigned {
        /// Path to the `.loom` file.
        store: String,
        /// Certificate bundle name.
        name: String,
        /// DNS subject alternative name. Repeat for multiple names.
        #[arg(long = "dns")]
        dns_names: Vec<String>,
        /// IP subject alternative name. Repeat for multiple addresses.
        #[arg(long = "ip")]
        ip_addresses: Vec<String>,
        /// Subject common name.
        #[arg(long)]
        cn: Option<String>,
        /// Validity period in days.
        #[arg(long, default_value_t = 365)]
        days: u32,
        /// Signing algorithm: `p256`, `p384`, or `ed25519`.
        #[arg(long, default_value = "p256")]
        algorithm: String,
        /// Permit private-key generation into an unencrypted store.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum CalendarCmd {
    /// Create or update collection metadata.
    CreateCollection {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Calendar collection id.
        collection: String,
        #[arg(long, default_value = "")]
        display_name: String,
        /// Component class: `event` or `todo`. Repeat to allow both.
        #[arg(long = "component")]
        component: Vec<String>,
    },
    /// Delete a collection and every entry in it.
    DeleteCollection {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Calendar collection id.
        collection: String,
    },
    /// Delete one entry.
    DeleteEntry {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Calendar collection id.
        collection: String,
        /// Entry UID.
        uid: String,
    },
    /// Fetch collection metadata as canonical CBOR.
    GetCollection {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Calendar collection id.
        collection: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Fetch one structured entry as canonical CBOR.
    GetEntry {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Calendar collection id.
        collection: String,
        /// Entry UID.
        uid: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// List collection ids under a principal.
    ListCollections {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Write canonical CBOR text array to this file instead of printing text.
        #[arg(long)]
        out: Option<String>,
    },
    /// List entries as a canonical CBOR array of entry maps.
    ListEntries {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Calendar collection id.
        collection: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Store one structured entry from canonical CBOR and print its ETag.
    PutEntry {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Calendar collection id.
        collection: String,
        /// Input file, or `-` for stdin.
        input: String,
    },
    /// Store one iCalendar document and print its ETag.
    PutIcs {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Calendar collection id.
        collection: String,
        /// Input file, or `-` for stdin.
        input: String,
    },
    /// Expand occurrences in `[from, to)` as canonical CBOR `[uid, start]` pairs.
    Range {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Calendar collection id.
        collection: String,
        /// iCalendar date or date-time: `YYYYMMDD` or `YYYYMMDDTHHMMSS[Z]`.
        from: String,
        /// iCalendar date or date-time: `YYYYMMDD` or `YYYYMMDDTHHMMSS[Z]`.
        to: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Search entries as a canonical CBOR array of entry maps.
    Search {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Calendar collection id.
        collection: String,
        #[arg(long)]
        component: Option<String>,
        #[arg(long)]
        text: Option<String>,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Project one entry as iCalendar.
    ToIcs {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Calendar collection id.
        collection: String,
        /// Entry UID.
        uid: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum CasCmd {
    /// Delete a reachable blob from a workspace CAS set.
    Delete {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Content address in `algo:hex` form.
        digest: String,
    },
    /// Fetch a blob by workspace content address.
    Get {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Content address in `algo:hex` form.
        digest: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Report whether a workspace CAS set contains a blob.
    Has {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Content address in `algo:hex` form.
        digest: String,
    },
    /// List reachable content addresses in canonical order.
    List {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Write canonical CBOR text array to this file instead of printing text.
        #[arg(long)]
        out: Option<String>,
    },
    /// Store bytes in a workspace CAS set and print the content address.
    Put {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Input file, or `-` for stdin.
        input: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum DocumentCmd {
    /// Delete a document id from a collection.
    Delete {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Document collection name.
        collection: String,
        /// Document id.
        id: String,
    },
    /// Fetch a UTF-8 document.
    GetText {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Document collection name.
        collection: String,
        /// Document id.
        id: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Insert or replace a UTF-8 document.
    PutText {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Document collection name.
        collection: String,
        /// Document id.
        id: String,
        /// Input file, or `-` for stdin.
        input: String,
        /// Optional entity tag guard for compare-and-swap writes.
        #[arg(long)]
        expected_entity_tag: Option<String>,
    },
    /// Fetch a document's raw bytes.
    GetBinary {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Document collection name.
        collection: String,
        /// Document id.
        id: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Insert or replace a document from bytes.
    PutBinary {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Document collection name.
        collection: String,
        /// Document id.
        id: String,
        /// Input file, or `-` for stdin.
        input: String,
        /// Optional entity tag guard for compare-and-swap writes.
        #[arg(long)]
        expected_entity_tag: Option<String>,
    },
    /// List documents as canonical CBOR `[id, bytes]` pairs.
    ListBinary {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Document collection name.
        collection: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Find document ids by an exact-match index value.
    Find {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Document collection name.
        collection: String,
        /// Index name.
        index: String,
        /// JSON scalar value to match.
        value_json: String,
    },
    /// Query documents with the native JSON predicate contract.
    Query {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Document collection name.
        collection: String,
        /// Query JSON file, or `-` for stdin.
        input: String,
    },
    /// Create an exact-match index on a dotted JSON path.
    IndexCreate {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Document collection name.
        collection: String,
        /// Index name.
        name: String,
        /// Dotted JSON path.
        path: String,
        /// Reject duplicate indexed scalar values.
        #[arg(long)]
        unique: bool,
    },
    /// Create a document index from a full declaration JSON object.
    IndexCreateJson {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Document collection name.
        collection: String,
        /// Declaration JSON file, or `-` for stdin.
        input: String,
    },
    /// Drop a document index.
    IndexDrop {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Document collection name.
        collection: String,
        /// Index name.
        name: String,
    },
    /// List document indexes as JSON.
    IndexList {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Document collection name.
        collection: String,
    },
    /// Rebuild one document index.
    IndexRebuild {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Document collection name.
        collection: String,
        /// Index name.
        name: String,
    },
    /// Show document index readiness as JSON.
    IndexStatus {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Document collection name.
        collection: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum ContactsCmd {
    /// Create or update address-book metadata.
    CreateBook {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Address-book id.
        book: String,
        #[arg(long, default_value = "")]
        display_name: String,
    },
    /// Delete an address book and every contact in it.
    DeleteBook {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Address-book id.
        book: String,
    },
    /// Delete one contact.
    DeleteEntry {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Address-book id.
        book: String,
        /// Contact UID.
        uid: String,
    },
    /// Fetch address-book metadata as canonical CBOR.
    GetBook {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Address-book id.
        book: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Fetch one structured contact as canonical CBOR.
    GetEntry {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Address-book id.
        book: String,
        /// Contact UID.
        uid: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// List address-book ids under a principal.
    ListBooks {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Write canonical CBOR text array to this file instead of printing text.
        #[arg(long)]
        out: Option<String>,
    },
    /// List contacts as a canonical CBOR array of contact maps.
    ListEntries {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Address-book id.
        book: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Store one structured contact from canonical CBOR and print its ETag.
    PutEntry {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Address-book id.
        book: String,
        /// Input file, or `-` for stdin.
        input: String,
    },
    /// Store one vCard document and print its ETag.
    PutVcard {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Address-book id.
        book: String,
        /// Input file, or `-` for stdin.
        input: String,
    },
    /// Search contacts as a canonical CBOR array of contact maps.
    Search {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Address-book id.
        book: String,
        /// Search text.
        text: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Project one contact as vCard.
    ToVcard {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Address-book id.
        book: String,
        /// Contact UID.
        uid: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum KvCmd {
    /// Delete one canonical CBOR cell key.
    Delete {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// KV map name.
        collection: String,
        /// File containing the canonical CBOR cell key, or `-` for stdin.
        key: String,
    },
    /// Fetch bytes for one canonical CBOR cell key.
    Get {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// KV map name.
        collection: String,
        /// File containing the canonical CBOR cell key.
        key: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// List entries as canonical CBOR `[key, bytes]` pairs.
    List {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// KV map name.
        collection: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Insert or replace bytes at one canonical CBOR cell key.
    Put {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// KV map name.
        collection: String,
        /// File containing the canonical CBOR cell key.
        key: String,
        /// Input file, or `-` for stdin.
        input: String,
    },
    /// Range entries with `from <= key < to`.
    Range {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// KV map name.
        collection: String,
        /// File containing the inclusive lower canonical CBOR cell key.
        from: String,
        /// File containing the exclusive upper canonical CBOR cell key.
        to: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum MailCmd {
    /// Create or update mailbox metadata.
    CreateMailbox {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Mailbox id.
        mailbox: String,
        #[arg(long, default_value = "")]
        display_name: String,
    },
    /// Delete a mailbox and every message index and flag set in it.
    DeleteMailbox {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Mailbox id.
        mailbox: String,
    },
    /// Delete one message index and its flags.
    DeleteMessage {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Mailbox id.
        mailbox: String,
        /// Message UID.
        uid: String,
    },
    /// Fetch message flags as text or a canonical CBOR text array.
    GetFlags {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Mailbox id.
        mailbox: String,
        /// Message UID.
        uid: String,
        /// Write canonical CBOR text array to this file instead of printing text.
        #[arg(long)]
        out: Option<String>,
    },
    /// Fetch mailbox metadata as canonical CBOR.
    GetMailbox {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Mailbox id.
        mailbox: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Fetch one structured message index as canonical CBOR.
    GetMessage {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Mailbox id.
        mailbox: String,
        /// Message UID.
        uid: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Ingest one raw RFC 5322 message and print the body digest.
    IngestMessage {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Mailbox id.
        mailbox: String,
        /// Message UID.
        uid: String,
        /// Input file, or `-` for stdin.
        input: String,
    },
    /// List mailbox ids under a principal.
    ListMailboxes {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Write canonical CBOR text array to this file instead of printing text.
        #[arg(long)]
        out: Option<String>,
    },
    /// List messages as a canonical CBOR array of message-index maps.
    ListMessages {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Mailbox id.
        mailbox: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Search messages as a canonical CBOR array of message-index maps.
    Search {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Mailbox id.
        mailbox: String,
        /// Search text.
        text: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Replace message flags.
    SetFlags {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Mailbox id.
        mailbox: String,
        /// Message UID.
        uid: String,
        /// Replacement message flags.
        flags: Vec<String>,
    },
    /// Project one message as raw RFC 5322 `.eml` bytes.
    ToEml {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Owning principal id.
        principal: String,
        /// Mailbox id.
        mailbox: String,
        /// Message UID.
        uid: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum QueueCmd {
    /// Append bytes to a stream and print the assigned sequence.
    Append {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Queue stream name.
        stream: String,
        /// Input file, or `-` for stdin.
        input: String,
    },
    /// Advance a consumer's next sequence.
    Advance {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Queue stream name.
        stream: String,
        /// Consumer id.
        consumer: String,
        /// Next sequence number for the consumer.
        next: u64,
    },
    /// Fetch one stream entry.
    Get {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Queue stream name.
        stream: String,
        /// Entry sequence number.
        seq: usize,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Print the stream length.
    Len {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Queue stream name.
        stream: String,
    },
    /// Print a consumer's next sequence.
    Position {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Queue stream name.
        stream: String,
        /// Consumer id.
        consumer: String,
    },
    /// Range entries as canonical CBOR byte array.
    Range {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Queue stream name.
        stream: String,
        /// Inclusive start sequence.
        from: usize,
        /// Exclusive end sequence.
        to: usize,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Read from a consumer's next sequence without advancing.
    Read {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Queue stream name.
        stream: String,
        /// Consumer id.
        consumer: String,
        /// Maximum number of entries to read.
        max: usize,
        /// Write canonical CBOR byte array to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Reset a consumer's next sequence.
    Reset {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Queue stream name.
        stream: String,
        /// Consumer id.
        consumer: String,
        /// Next sequence number for the consumer.
        next: u64,
    },
}

#[derive(Subcommand)]
pub(crate) enum TimeSeriesCmd {
    /// Fetch one point's raw bytes.
    Get {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Series name.
        series: String,
        /// Point timestamp.
        timestamp: i64,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Fetch the latest point as canonical CBOR `[[timestamp, bytes]]`.
    Latest {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Series name.
        series: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Put bytes at a timestamp.
    Put {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Series name.
        series: String,
        /// Point timestamp.
        timestamp: i64,
        /// Input file, or `-` for stdin.
        input: String,
    },
    /// Range points as canonical CBOR `[timestamp, bytes]` pairs.
    Range {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Series name.
        series: String,
        /// Inclusive start timestamp.
        from: i64,
        /// Exclusive end timestamp.
        to: i64,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum StoreCmd {
    /// Export a workspace and every reachable object to an offline `.bundle` file.
    BundleExport {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Output `.bundle` path.
        out: String,
    },
    /// Import an offline `.bundle` file into the `.loom` as a new workspace.
    BundleImport {
        /// Path to the `.loom` file.
        store: String,
        /// Input `.bundle` path.
        input: String,
    },
    /// Clone a workspace from one `.loom` into another.
    Clone {
        /// Source `.loom`.
        src: String,
        /// Source workspace UUID or name.
        workspace: String,
        /// Destination `.loom`.
        dst: String,
    },
    /// Copy a `.loom` store, optionally changing its identity profile or compacting it.
    Copy {
        /// Source `.loom`.
        src: String,
        /// Destination `.loom`, which must not already exist.
        dst: String,
        /// Copy modifier. Supported values: `fips`, `compacted`.
        #[arg(long = "with", value_name = "MODIFIER")]
        with: Vec<String>,
        /// Standard output format: `text` or `json`.
        #[arg(long, default_value = "text")]
        format: String,
        /// Write the machine-readable JSON migration report to this file.
        #[arg(long = "report-file")]
        report_file: Option<String>,
        /// Show the planned copy without writing the destination.
        #[arg(long)]
        dry_run: bool,
        /// Key source for the NEW target passphrase when migrating an encrypted store:
        /// `prompt` (default), `file:<path>`, `fd:<n>`, or `raw-kek:...`.
        #[arg(long)]
        new_key_source: Option<String>,
    },
    /// Fetch a Blob by content address and write its bytes to stdout or `--out`.
    Get {
        /// Path to the `.loom` file.
        store: String,
        /// The content address, e.g. `blake3:abcd...`.
        digest: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Hash bytes from a file or stdin and print the Blob content address.
    Hash {
        /// Path to a file, or `-` to read from standard input.
        path: String,
    },
    /// Create an empty `.loom` store file.
    Init {
        /// Path to the `.loom` file.
        store: String,
        /// Create the store encrypted at rest.
        #[arg(long)]
        encrypt: bool,
        /// AEAD suite for `--encrypt`.
        #[arg(long)]
        suite: Option<String>,
        /// Identity profile: `default` or `fips`.
        #[arg(long)]
        identity_profile: Option<String>,
        /// Shorthand for `--identity-profile fips`.
        #[arg(long)]
        fips: bool,
    },
    /// Add or remove encrypted-store unlock wraps.
    Key {
        #[command(subcommand)]
        action: KeyCmd,
    },
    /// Inspect or update store-level compliance policy.
    Policy {
        /// Path to the `.loom` file.
        store: String,
        /// Set whether this store requires FIPS-capable runtimes for hosted serving.
        #[arg(long)]
        fips_required: Option<bool>,
    },
    /// Store a file or stdin as a Blob and print its content address.
    Put {
        /// Path to the `.loom` file.
        store: String,
        /// Path to the input file, or `-` to read from standard input.
        path: String,
    },
    /// Rotate the data-encryption key.
    Rekey {
        /// Path to the `.loom` file.
        store: String,
        /// Target AEAD suite.
        #[arg(long)]
        suite: Option<String>,
        /// Re-seal every object under a fresh DEK.
        #[arg(long)]
        reseal: bool,
        /// Key source for the NEW passphrase: `prompt` (default), `file:<path>`, `fd:<n>`,
        /// or `raw-kek:...`.
        #[arg(long)]
        new_key_source: Option<String>,
    },
    /// Print summary statistics for a `.loom` store.
    Stat {
        /// Path to the `.loom` file.
        store: String,
    },
    /// Preflight a candidate store before replacing an active Matrix store.
    PreflightReplacement {
        /// Candidate `.loom` file.
        store: String,
        /// Coordination workspace UUID or name to verify.
        workspace: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum FilesCmd {
    /// Delete a file or directory from a workspace.
    Delete {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Working-tree path.
        path: String,
        /// Delete a directory and all descendants.
        #[arg(long)]
        recursive: bool,
    },
    /// List staged paths in a workspace.
    Ls {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
    },
    /// Create a directory in a workspace.
    Mkdir {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Directory path.
        path: String,
        /// Create parent directories as needed.
        #[arg(long)]
        parents: bool,
    },
    /// Read a staged file to stdout or `--out`.
    Read {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Working-tree path.
        path: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Stage a file or stdin at `path` in a workspace.
    Write {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Working-tree path, e.g. `src/main.rs`.
        path: String,
        /// Input file, or `-` for stdin.
        input: String,
    },
}

#[cfg(any(feature = "fuse", feature = "nfs"))]
#[derive(Subcommand)]
pub(crate) enum MountCmd {
    /// Mount a workspace through FUSE.
    #[cfg(feature = "fuse")]
    Fuse {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Mount point.
        mountpoint: String,
        /// Mount read-only.
        #[arg(long)]
        read_only: bool,
    },
    /// Mount a workspace through NFSv3.
    #[cfg(feature = "nfs")]
    Nfs {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Mount point.
        mountpoint: String,
        /// Address the NFS server listens on.
        #[arg(long, default_value = "127.0.0.1:12049")]
        listen: String,
        /// Mount read-only.
        #[arg(long)]
        read_only: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum ExecCmd {
    /// Execute a canonical `loom.exec.request.v1` CBOR request (gated, direct, or batch per its own
    /// `mode`) and print the `loom.exec.result.v1` response as JSON. Dry-run is a gated-mode request.
    Run {
        /// Path to the `.loom` file.
        store: String,
        /// Path to the canonical `loom.exec.request.v1` CBOR request file.
        request: String,
        /// Overlay a named input blob onto every step as `name=@file`. Repeatable.
        #[arg(long = "input", value_name = "NAME=@FILE")]
        input: Vec<String>,
    },
    /// Decode a canonical `loom.exec.request.v1` CBOR request and print it as JSON without executing.
    Inspect {
        /// Path to the canonical `loom.exec.request.v1` CBOR request file.
        request: String,
    },
    /// Merge a gated proposal fork branch into a base branch, adopting the program's committed state.
    Apply {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Base branch to merge the proposal into.
        base: String,
        /// Proposal fork branch produced by a gated run.
        fork: String,
        /// Merge commit author.
        #[arg(long, default_value = "loom")]
        author: String,
        /// Merge commit timestamp in milliseconds.
        #[arg(long, default_value_t = 0)]
        timestamp_ms: u64,
    },
}

#[derive(Subcommand)]
pub(crate) enum ProgramCmd {
    /// Store a WASM program body as `engine=wasm`.
    PutWasm {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Program name.
        name: String,
        /// Program body file, or `-` for standard input.
        #[arg(long, default_value = "-")]
        input: String,
        /// Output file for the stored record summary. Omit to write CBOR bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Store a Loom Templates source body as `engine=template`.
    PutTemplate {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Program name.
        name: String,
        /// Template source file, or `-` for standard input.
        #[arg(long, default_value = "-")]
        input: String,
        /// Output file for the stored record summary. Omit to write CBOR bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Store a CEL source body as `engine=cel`.
    PutCel {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Program name.
        name: String,
        /// CEL source file, or `-` for standard input.
        #[arg(long, default_value = "-")]
        input: String,
        /// Output file for the stored record summary. Omit to write CBOR bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Inspect a stored Program record without loading its body bytes.
    Inspect {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Program name.
        name: String,
        /// Output file. Omit to write CBOR bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Read the stored Program body bytes.
    Get {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Program name.
        name: String,
        /// Output file. Omit to write bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// List stored Program records.
    List {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Output file. Omit to write CBOR bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Remove a named Program record.
    Remove {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Program name.
        name: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum SqlCmd {
    /// Run SQL against a workspace database.
    Exec {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// The SQL text to run.
        sql: String,
        /// Database name within the workspace's SQL facet.
        #[arg(long, default_value = "main")]
        db: String,
    },
    /// Inspect committed table history.
    Table {
        #[command(subcommand)]
        action: TableCmd,
    },
}

#[derive(Subcommand)]
pub(crate) enum VcsCmd {
    /// Create a branch at the current HEAD tip.
    Branch {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// New branch name.
        branch: String,
    },
    /// Snapshot the working tree into a commit on the workspace's current HEAD branch.
    Commit {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Commit message.
        #[arg(short = 'm', long)]
        message: String,
        /// Author identity (default: `loom-cli`).
        #[arg(long, default_value = "loom-cli")]
        author: String,
    },
    /// Switch HEAD to `branch` and materialize its tip into the working tree.
    Checkout {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Branch name.
        branch: String,
    },
    /// Diff two commits using the structural cross-facet envelope.
    Diff {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Base commit (content address).
        from: String,
        /// Other commit (content address).
        to: String,
        /// Output format: text or cbor.
        #[arg(long, default_value = "text")]
        format: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Print the commit log for the current HEAD branch.
    Log {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
    },
    /// Merge `from` into the workspace's current HEAD branch (committing the result on success).
    Merge {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        workspace: String,
        /// Branch to merge into HEAD.
        from: String,
        /// Reconcile tables at cell granularity: different columns of the same row auto-merge.
        #[arg(long)]
        cells: bool,
        /// Author identity for the merge commit (default: `loom-cli`).
        #[arg(long, default_value = "loom-cli")]
        author: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum VectorCmd {
    /// Configure workspace-level vector provider bindings.
    Workspace {
        #[command(subcommand)]
        action: VectorWorkspaceCmd,
    },
    /// Embed source text through a configured inference instance and search stored source text.
    Text {
        #[command(subcommand)]
        action: VectorTextCmd,
    },
    /// Create a named vector set with fixed dimension and metric.
    Create {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Vector set name.
        name: String,
        /// Embedding dimension.
        #[arg(long)]
        dim: usize,
        /// Metric: cosine, l2, or dot.
        #[arg(long)]
        metric: String,
    },
    /// Insert or replace one vector id from little-endian f32 bytes.
    Upsert {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Vector set name.
        name: String,
        /// Vector id.
        id: String,
        /// File containing little-endian f32 bytes, or `-` for stdin.
        vector: String,
        /// Optional metadata file containing canonical CBOR `text -> cell`.
        #[arg(long)]
        metadata: Option<String>,
    },
    /// Insert or replace one vector id with externally-computed vector bytes and source text.
    UpsertSource {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Vector set name.
        name: String,
        /// Vector id.
        id: String,
        /// File containing little-endian f32 bytes, or `-` for stdin.
        vector: String,
        /// UTF-8 source text file.
        #[arg(long)]
        source: String,
        /// Optional metadata file containing canonical CBOR `text -> cell`.
        #[arg(long)]
        metadata: Option<String>,
        /// Optional embedding model id to record for source-aware writes.
        #[arg(long)]
        model_id: Option<String>,
        /// Optional embedding weights digest to record with `--model-id`.
        #[arg(long)]
        weights_digest: Option<String>,
    },
    /// Fetch one vector as canonical CBOR `[vector_bytes, metadata]`.
    Get {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Vector set name.
        name: String,
        /// Vector id.
        id: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Fetch stored source text for one vector id.
    Source {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Vector set name.
        name: String,
        /// Vector id.
        id: String,
        /// Write to this file instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// List vector ids in ascending order, optionally restricted by string prefix.
    Ids {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Vector set name.
        name: String,
        /// Optional string prefix.
        #[arg(long)]
        prefix: Option<String>,
        /// Write canonical CBOR text array to this file instead of printing text ids.
        #[arg(long)]
        out: Option<String>,
    },
    /// List declared metadata equality index keys in ascending order.
    IndexKeys {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Vector set name.
        name: String,
        /// Write canonical CBOR text array to this file instead of printing text keys.
        #[arg(long)]
        out: Option<String>,
    },
    /// Declare and build a metadata equality index.
    CreateIndex {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Vector set name.
        name: String,
        /// Metadata key to index.
        key: String,
    },
    /// Drop a metadata equality index.
    DropIndex {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Vector set name.
        name: String,
        /// Metadata key to stop indexing.
        key: String,
    },
    /// Remove one vector id.
    Delete {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Vector set name.
        name: String,
        /// Vector id.
        id: String,
    },
    /// Exact top-k search from little-endian f32 query bytes.
    Search {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Vector set name.
        name: String,
        /// Query file containing little-endian f32 bytes, or `-` for stdin.
        query: String,
        /// Number of hits.
        #[arg(long)]
        k: usize,
        /// Optional canonical CBOR filter: `[0]`, `[1,key,value_cell]`, or `[2,a,b]`.
        #[arg(long)]
        filter: Option<String>,
        /// Accelerator policy: `exact` or `approximate-pq`.
        #[arg(long, default_value = "exact")]
        policy: String,
        /// Use the accelerator only when the set has more vectors than this threshold.
        #[arg(long, default_value_t = 4096)]
        threshold: usize,
        /// Candidate shortlist size for approximate PQ search.
        #[arg(long, default_value_t = 0)]
        ef: usize,
        /// PQ subspace count. Must divide the set dimension when approximate PQ is used.
        #[arg(long, default_value_t = 1)]
        pq_m: usize,
        /// PQ centroids per subspace.
        #[arg(long, default_value_t = 16)]
        pq_k: usize,
        /// PQ k-means training iterations.
        #[arg(long, default_value_t = 8)]
        pq_iters: usize,
        /// Write canonical CBOR hit array to this file instead of printing `id<TAB>score`.
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum VectorTextCmd {
    /// Embed and insert or replace one source text record.
    Upsert {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Vector set name.
        name: String,
        /// Vector id.
        id: String,
        /// Source text. Use `--text-file` for longer input.
        #[arg(long, required_unless_present = "text_file")]
        text: Option<String>,
        /// UTF-8 source text file, or `-` for stdin.
        #[arg(long, value_name = "PATH", required_unless_present = "text")]
        text_file: Option<String>,
        /// Named text-embedding instance. Defaults to the workspace binding.
        #[arg(long)]
        embedding_instance: Option<String>,
        /// Optional metadata file containing canonical CBOR `text -> cell`.
        #[arg(long)]
        metadata: Option<String>,
        /// Create the vector set first when it does not already exist.
        #[arg(long)]
        create: bool,
        /// Metric for `--create`: cosine, l2, or dot.
        #[arg(long, default_value = "cosine")]
        metric: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Embed a query and return nearest stored source-text records.
    Query {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Vector set name.
        name: String,
        /// Query text. Use `--query-file` for longer input.
        #[arg(long, required_unless_present = "query_file")]
        query: Option<String>,
        /// UTF-8 query text file, or `-` for stdin.
        #[arg(long, value_name = "PATH", required_unless_present = "query")]
        query_file: Option<String>,
        /// Number of hits.
        #[arg(long, default_value_t = 5)]
        top_k: usize,
        /// Named text-embedding instance. Defaults to the workspace binding.
        #[arg(long)]
        embedding_instance: Option<String>,
        /// Optional canonical CBOR filter.
        #[arg(long)]
        filter: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum GraphCmd {
    /// Insert or replace a node; `props` is canonical CBOR `text -> bytes` when provided.
    UpsertNode {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Graph name.
        name: String,
        /// Node id.
        id: String,
        #[arg(long)]
        props: Option<String>,
    },
    /// Fetch a node's props as canonical CBOR `text -> bytes`.
    GetNode {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Graph name.
        name: String,
        /// Node id.
        id: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Remove a node; `--cascade` also removes incident edges.
    RemoveNode {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Graph name.
        name: String,
        /// Node id.
        id: String,
        #[arg(long)]
        cascade: bool,
    },
    /// Insert or replace a directed labelled edge; `props` is canonical CBOR `text -> bytes`.
    UpsertEdge {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Graph name.
        name: String,
        /// Edge id.
        id: String,
        /// Source node id.
        src: String,
        /// Destination node id.
        dst: String,
        /// Edge label.
        label: String,
        #[arg(long)]
        props: Option<String>,
    },
    /// Fetch an edge as canonical CBOR `[src, dst, label, props]`.
    GetEdge {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Graph name.
        name: String,
        /// Edge id.
        id: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Remove an edge and print whether it was present.
    RemoveEdge {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Graph name.
        name: String,
        /// Edge id.
        id: String,
    },
    /// Adjacent node ids as canonical CBOR text array.
    Neighbors {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Graph name.
        name: String,
        /// Node id.
        id: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Outgoing edges as canonical CBOR array of `[edge_id, edge]`.
    OutEdges {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Graph name.
        name: String,
        /// Node id.
        id: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Incoming edges as canonical CBOR array of `[edge_id, edge]`.
    InEdges {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Graph name.
        name: String,
        /// Node id.
        id: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Reachable node ids as canonical CBOR text array.
    Reachable {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Graph name.
        name: String,
        /// Start node id.
        start: String,
        #[arg(long, default_value_t = -1)]
        max_depth: i64,
        #[arg(long)]
        via_label: Option<String>,
        #[arg(long)]
        out: Option<String>,
    },
    /// Shortest directed path as canonical CBOR text array.
    ShortestPath {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Graph name.
        name: String,
        /// Source node id.
        from: String,
        /// Destination node id.
        to: String,
        #[arg(long)]
        via_label: Option<String>,
        #[arg(long)]
        out: Option<String>,
    },
    /// Run a bounded graph query and write canonical CBOR rows.
    Query {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Graph name.
        name: String,
        /// Bounded openCypher/GQL-aligned query text.
        query: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Explain a bounded graph query and write canonical CBOR plan metadata.
    ExplainQuery {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Graph name.
        name: String,
        /// Bounded openCypher/GQL-aligned query text.
        query: String,
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum LedgerCmd {
    /// Append a payload file (or `-` for stdin) and print the new sequence number.
    Append {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Ledger name.
        collection: String,
        /// Payload input file, or `-` for stdin.
        payload: String,
    },
    /// Fetch one payload by sequence.
    Get {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Ledger name.
        collection: String,
        /// Entry sequence number.
        seq: u64,
        #[arg(long)]
        out: Option<String>,
    },
    /// Print the head digest, or write raw head bytes with `--out`.
    Head {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Ledger name.
        collection: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Print the number of entries.
    Len {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Ledger name.
        collection: String,
    },
    /// Verify the ledger hash chain.
    Verify {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Ledger name.
        collection: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum ColumnarCmd {
    /// Create a dataset from canonical CBOR columns `[[name, type_tag] ...]`.
    Create {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataset name.
        name: String,
        /// File with canonical CBOR columns `[[name, type_tag] ...]`, or `-` for stdin.
        columns: String,
        #[arg(long, default_value_t = 0)]
        target_segment_rows: usize,
    },
    /// Append one canonical CBOR cell row.
    Append {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataset name.
        name: String,
        /// File with one canonical CBOR cell row, or `-` for stdin.
        row: String,
    },
    /// Scan all rows as canonical CBOR array of rows.
    Scan {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataset name.
        name: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Fetch columns as canonical CBOR `[[name, type_tag] ...]`.
    Columns {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataset name.
        name: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Print the row count.
    Rows {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataset name.
        name: String,
    },
    /// Compact segment layout at the dataset target segment size.
    Compact {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataset name.
        name: String,
    },
    /// Inspect dataset metadata as canonical CBOR.
    Inspect {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataset name.
        name: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Print the source digest used by derived columnar projections.
    SourceDigest {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataset name.
        name: String,
    },
    /// Project selected columns, optionally filtered by canonical CBOR `[column, op, value_cell]`.
    Select {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataset name.
        name: String,
        /// File with a canonical CBOR array of column names, or `-` for stdin.
        columns: String,
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        out: Option<String>,
    },
    /// Evaluate aggregate expressions from canonical CBOR `[[op, column?] ...]`.
    Aggregate {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataset name.
        name: String,
        /// File with canonical CBOR aggregates `[[op, column?] ...]`, or `-` for stdin.
        aggregates: String,
        #[arg(long)]
        filter: Option<String>,
        #[arg(long)]
        out: Option<String>,
    },
    /// Import an Arrow IPC stream as a columnar dataset.
    ImportArrow {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataset name.
        name: String,
        /// Input file, or `-` for stdin.
        input: String,
        #[arg(long, default_value_t = 0)]
        target_segment_rows: usize,
        #[arg(long)]
        replace: bool,
    },
    /// Export a columnar dataset as an Arrow IPC stream.
    ExportArrow {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataset name.
        name: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Import a Parquet file as a columnar dataset.
    ImportParquet {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataset name.
        name: String,
        /// Input file, or `-` for stdin.
        input: String,
        #[arg(long, default_value_t = 0)]
        target_segment_rows: usize,
        #[arg(long)]
        replace: bool,
    },
    /// Export a columnar dataset as a Parquet file.
    ExportParquet {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataset name.
        name: String,
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum DataframeCmd {
    /// Create a dataframe plan from canonical dataframe-plan CBOR.
    Create {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataframe plan name.
        name: String,
        /// File with canonical dataframe-plan CBOR, or `-` for stdin.
        plan: String,
    },
    /// Collect the dataframe into a canonical CBOR batch.
    Collect {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataframe plan name.
        name: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Materialize according to the plan policy and print a CAS digest when one is produced.
    Materialize {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataframe plan name.
        name: String,
    },
    /// Print the dataframe plan digest.
    PlanDigest {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataframe plan name.
        name: String,
    },
    /// Preview the first rows as a canonical CBOR batch.
    Preview {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataframe plan name.
        name: String,
        #[arg(long, default_value_t = 20)]
        rows: u64,
        #[arg(long)]
        out: Option<String>,
    },
    /// List source digests as canonical CBOR text array.
    SourceDigests {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Dataframe plan name.
        name: String,
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum VectorWorkspaceCmd {
    /// Configure a vector workspace binding.
    Configure {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Named text embedding instance used for vector text upserts.
        #[arg(long)]
        embedding_instance: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum StudioCmd {
    /// Inspect source-backed Studio app surface catalogs.
    Surfaces {
        #[command(subcommand)]
        action: StudioSurfacesCmd,
    },
    /// Enqueue a Studio reindex for one workspace.
    Reindex {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Studio profile to reindex, or `all`.
        #[arg(long, default_value = "all")]
        profile: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Rebuild and backfill Studio revision indexes.
    Revisions {
        #[command(subcommand)]
        action: StudioRevisionsCmd,
    },
}

#[derive(Subcommand)]
pub(crate) enum StudioSurfacesCmd {
    /// Print the deterministic Studio app catalog for a workspace id.
    Catalog {
        /// Workspace id used in `ui://` resource references.
        workspace: String,
        /// Catalog set: core, all, or meeting-memory.
        #[arg(long, default_value = "all")]
        set: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum StudioRevisionsCmd {
    /// Backfill missing current revision rows from source-backed profile state.
    Rebuild {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Studio profile to rebuild.
        #[arg(long, default_value = "meetings")]
        profile: String,
        /// Print the plan without writing the revision index.
        #[arg(long)]
        dry_run: bool,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum SearchCmd {
    /// Create a full-text collection from canonical CBOR mapping `field -> [type_tag, stored, faceted]`.
    Create {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Search collection name.
        name: String,
        /// File with canonical CBOR mapping `field -> [type_tag, stored, faceted]`, or `-` for stdin.
        mapping: String,
    },
    /// Insert or replace one document.
    Index {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Search collection name.
        name: String,
        /// Document id. Omit when `--id-file` provides the id bytes.
        #[arg(required_unless_present = "id_file")]
        id: Option<String>,
        #[arg(long, value_name = "PATH")]
        id_file: Option<String>,
        /// Document input file, or `-` for stdin. With `--id-file`, the doc input may occupy
        /// the id slot.
        doc: Option<String>,
    },
    /// Fetch a document as canonical CBOR `field -> value`.
    Get {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Search collection name.
        name: String,
        /// Document id. Omit when `--id-file` provides the id bytes.
        #[arg(required_unless_present = "id_file")]
        id: Option<String>,
        #[arg(long, value_name = "PATH")]
        id_file: Option<String>,
        #[arg(long)]
        out: Option<String>,
    },
    /// Delete a document and print whether it was present.
    Delete {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Search collection name.
        name: String,
        /// Document id. Omit when `--id-file` provides the id bytes.
        #[arg(required_unless_present = "id_file")]
        id: Option<String>,
        #[arg(long, value_name = "PATH")]
        id_file: Option<String>,
    },
    /// List document ids as canonical CBOR byte array.
    Ids {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Search collection name.
        name: String,
        #[arg(long)]
        prefix: Option<String>,
        #[arg(long, value_name = "PATH")]
        prefix_file: Option<String>,
        #[arg(long)]
        out: Option<String>,
    },
    /// Replace a collection mapping.
    Remap {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Search collection name.
        name: String,
        /// File with canonical CBOR mapping `field -> [type_tag, stored, faceted]`, or `-` for stdin.
        mapping: String,
    },
    /// Run a portable query request and return canonical CBOR `[reduced, hits]`.
    Query {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Search collection name.
        name: String,
        /// File with a canonical CBOR query request `[query, limit, offset]`, or `-` for stdin.
        request: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Rebuild the derived native Tantivy artifact for a collection.
    Rebuild {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Search collection name.
        name: String,
        /// Native search engine version stamp. Defaults to the linked Tantivy version when native FTS is enabled.
        #[arg(long)]
        engine_version: Option<String>,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Print the derived native Tantivy artifact status for a collection.
    Status {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Search collection name.
        name: String,
        /// Native search engine version stamp to check.
        #[arg(long)]
        engine_version: String,
        /// Output format: text or json.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum DaemonCmd {
    /// Start the local coordinator daemon if it is not already running.
    Start {
        /// Path to the `.loom` file.
        store: String,
        /// Daemon transport: `native` for secure platform IPC, or `tcp` for the degraded loopback fallback.
        #[arg(long, default_value = "native")]
        transport: String,
    },
    /// Stop the local coordinator daemon.
    Stop {
        /// Path to the `.loom` file.
        store: String,
        /// Stop even when permanent pins are live.
        #[arg(long)]
        force: bool,
        /// Graceful shutdown wait before timing out hosted listener drains.
        #[arg(long)]
        wait: Option<u64>,
        /// Stop accepting work and detach hosted listener threads without waiting for graceful drain.
        #[arg(long)]
        hard: bool,
    },
    /// Restart the local coordinator daemon.
    Restart {
        /// Path to the `.loom` file.
        store: String,
        /// Daemon transport: `native` for secure platform IPC, or `tcp` for the degraded loopback fallback.
        #[arg(long, default_value = "native")]
        transport: String,
    },
    /// Print local coordinator daemon status.
    Status {
        /// Path to the `.loom` file.
        store: String,
        /// Print machine-readable JSON status.
        #[arg(long)]
        json: bool,
    },
    /// Inspect and operate bounded store maintenance.
    Maintenance {
        #[command(subcommand)]
        action: DaemonMaintenanceCmd,
    },
    /// Attach or detach a named client session.
    Session {
        #[command(subcommand)]
        action: DaemonSessionCmd,
    },
    /// Pin or unpin the daemon for long-lived projections such as mounts.
    Pin {
        #[command(subcommand)]
        action: DaemonPinCmd,
    },
    /// Internal foreground daemon loop used by `daemon start`.
    #[command(hide = true)]
    Run {
        /// Path to the `.loom` file.
        store: String,
        #[arg(long)]
        addr_file: String,
        #[arg(long)]
        pid_file: String,
        #[arg(long)]
        lock_file: String,
        #[arg(long, default_value = "native")]
        transport: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum DaemonSessionCmd {
    /// Attach a named session to the local coordinator daemon.
    Attach {
        /// Path to the `.loom` file.
        store: String,
        /// Stable session id.
        session: String,
    },
    /// Detach a named session from the local coordinator daemon.
    Detach {
        /// Path to the `.loom` file.
        store: String,
        /// Stable session id.
        session: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum DaemonPinCmd {
    /// Add a daemon pin.
    Add {
        /// Path to the `.loom` file.
        store: String,
        /// Stable pin id.
        pin: String,
    },
    /// Remove a daemon pin.
    Remove {
        /// Path to the `.loom` file.
        store: String,
        /// Stable pin id.
        pin: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum DaemonMaintenanceCmd {
    /// Print store-maintenance policy, debt, epoch, and eligibility state.
    Status {
        /// Path to the `.loom` file.
        store: String,
        /// Print machine-readable JSON status.
        #[arg(long)]
        json: bool,
    },
    /// Update durable store-maintenance policy.
    Policy {
        /// Path to the `.loom` file.
        store: String,
        /// Minimum candidate-dead pages before maintenance is eligible.
        #[arg(long)]
        min_candidate_pages: Option<u64>,
        /// Minimum reusable-free pages before maintenance is eligible.
        #[arg(long)]
        min_reusable_pages: Option<u64>,
        /// Minimum milliseconds between successful maintenance checks.
        #[arg(long)]
        interval_ms: Option<u64>,
        /// Milliseconds to wait after a failed maintenance run.
        #[arg(long)]
        backoff_ms: Option<u64>,
        /// Maximum segments reclaimed by one bounded GC run.
        #[arg(long)]
        max_segments: Option<u64>,
        /// Maximum pages rewritten by one bounded GC run.
        #[arg(long)]
        max_pages: Option<u64>,
        /// Allow policy-gated whole-file compaction.
        #[arg(long, conflicts_with = "disallow_full_compaction")]
        allow_full_compaction: bool,
        /// Disallow policy-gated whole-file compaction.
        #[arg(long)]
        disallow_full_compaction: bool,
        /// Enable automatic tail trim after validated reclaim.
        #[arg(long, conflicts_with = "disable_tail_trim")]
        enable_tail_trim: bool,
        /// Disable automatic tail trim after validated reclaim.
        #[arg(long)]
        disable_tail_trim: bool,
        /// Enable bounded background tail compaction.
        #[arg(long, conflicts_with = "disable_tail_compaction")]
        enable_tail_compaction: bool,
        /// Disable bounded background tail compaction.
        #[arg(long)]
        disable_tail_compaction: bool,
        /// Maximum pages moved by one bounded tail-compaction pass.
        #[arg(long)]
        tail_compaction_max_pages: Option<u64>,
        /// Maximum objects moved by one bounded tail-compaction pass.
        #[arg(long)]
        tail_compaction_max_objects: Option<u64>,
        /// Maximum bytes moved by one bounded tail-compaction pass.
        #[arg(long)]
        tail_compaction_max_bytes: Option<u64>,
        /// Minimum milliseconds between bounded tail-compaction passes.
        #[arg(long)]
        tail_compaction_interval_ms: Option<u64>,
        /// Milliseconds to wait after a bounded tail-compaction conflict or error.
        #[arg(long)]
        tail_compaction_backoff_ms: Option<u64>,
    },
    /// Run one bounded daemon-authorized maintenance pass.
    Run {
        /// Path to the `.loom` file.
        store: String,
        /// Override maximum segments reclaimed by this run.
        #[arg(long)]
        max_segments: Option<u64>,
        /// Override maximum pages rewritten by this run.
        #[arg(long)]
        max_pages: Option<u64>,
    },
}

#[derive(Subcommand)]
pub(crate) enum LockCmd {
    /// Acquire a daemon-backed lock with a bounded wait by default.
    Acquire {
        /// Path to the `.loom` file.
        store: String,
        /// Lock key.
        key: String,
        /// Principal id or name for the lock owner.
        #[arg(long, default_value = "cli")]
        principal: String,
        /// Session id for the lock owner.
        #[arg(long, default_value = "cli")]
        session: String,
        /// Mode: exclusive, shared, or semaphore.
        #[arg(long, default_value = "exclusive")]
        mode: String,
        /// Semaphore permits requested when `--mode semaphore`.
        #[arg(long, default_value_t = 1)]
        permits: u32,
        /// Semaphore capacity when `--mode semaphore`.
        #[arg(long, default_value_t = 1)]
        capacity: u32,
        /// Lease duration in milliseconds.
        #[arg(long, default_value_t = 30000)]
        lease_ms: u64,
        /// Maximum time to wait in milliseconds.
        #[arg(long, value_name = "MS")]
        wait: Option<u64>,
        /// Return immediately if the lock is held.
        #[arg(long)]
        no_wait: bool,
    },
    /// Refresh a daemon-backed lock lease.
    Refresh {
        /// Path to the `.loom` file.
        store: String,
        /// Lock key.
        key: String,
        /// Principal id or name for the lock owner.
        #[arg(long)]
        principal: String,
        /// Session id for the lock owner.
        #[arg(long)]
        session: String,
        /// Mode: exclusive, shared, or semaphore.
        #[arg(long, default_value = "exclusive")]
        mode: String,
        /// Semaphore permits when `--mode semaphore`.
        #[arg(long, default_value_t = 1)]
        permits: u32,
        /// Semaphore capacity when `--mode semaphore`.
        #[arg(long, default_value_t = 1)]
        capacity: u32,
        /// Fence token returned by acquire.
        #[arg(long)]
        fence: u64,
        /// Lease duration in milliseconds.
        #[arg(long, default_value_t = 30000)]
        lease_ms: u64,
    },
    /// Release a daemon-backed lock.
    Release {
        /// Path to the `.loom` file.
        store: String,
        /// Lock key.
        key: String,
        /// Principal id or name for the lock owner.
        #[arg(long)]
        principal: String,
        /// Session id for the lock owner.
        #[arg(long)]
        session: String,
        /// Mode: exclusive, shared, or semaphore.
        #[arg(long, default_value = "exclusive")]
        mode: String,
        /// Semaphore permits when `--mode semaphore`.
        #[arg(long, default_value_t = 1)]
        permits: u32,
        /// Semaphore capacity when `--mode semaphore`.
        #[arg(long, default_value_t = 1)]
        capacity: u32,
        /// Fence token returned by acquire.
        #[arg(long)]
        fence: u64,
    },
}

#[derive(Subcommand)]
pub(crate) enum KeyCmd {
    /// Add another unlock credential for the same encrypted store.
    AddWrap {
        /// Path to the `.loom` file.
        store: String,
        /// Permit an external-only store with no passphrase recovery wrap.
        #[arg(long)]
        allow_no_recovery: bool,
        /// Key source for the NEW unlock credential: `prompt` (default), `file:<path>`,
        /// `fd:<n>`, or `raw-kek:...`.
        #[arg(long)]
        new_key_source: Option<String>,
    },
    /// Remove one unlock credential by zero-based wrap index.
    RemoveWrap {
        /// Path to the `.loom` file.
        store: String,
        /// Zero-based index in the store's current wrap list.
        index: usize,
        /// Permit an external-only store with no passphrase recovery wrap.
        #[arg(long)]
        allow_no_recovery: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum ManagementCmd {
    /// Manage workspaces in a `.loom` store.
    Workspace {
        #[command(subcommand)]
        action: WorkspaceCmd,
    },
    /// Manage principals in a `.loom` store.
    Identity {
        #[command(subcommand)]
        action: IdentityCmd,
    },
    /// Manage direct ACL grants in a `.loom` store.
    Acl {
        #[command(subcommand)]
        action: AclCmd,
    },
    /// Manage KV map control-plane config.
    Kv {
        #[command(subcommand)]
        action: ManagementKvCmd,
    },
    /// Manage branch and tag protected-ref policy records.
    ProtectedRef {
        #[command(subcommand)]
        action: ProtectedRefCmd,
    },
}

#[derive(Subcommand)]
pub(crate) enum WorkspaceCmd {
    /// Create a named workspace, optionally ensuring an initial facet exists.
    Create {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace name.
        name: String,
        /// Initial facet tag: files, kv, document, graph, ledger, queue, ...
        #[arg(long)]
        facet: Option<String>,
    },
    /// List the workspaces in the `.loom`.
    List {
        /// Path to the `.loom` file.
        store: String,
    },
    /// Rename an existing workspace.
    Rename {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or current name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// New workspace name.
        new_name: String,
    },
    /// Delete an existing workspace. Objects are reclaimed by a later GC over remaining roots.
    Delete {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum IdentityCmd {
    /// Print the principal registry as JSON.
    List {
        /// Path to the `.loom` file.
        store: String,
    },
    /// Add a principal with a durable handle and print its UUID.
    Add {
        /// Path to the `.loom` file.
        store: String,
        /// Principal handle used by `@` references.
        handle: String,
        /// Principal display name.
        name: String,
        /// Principal kind: root, user, or service.
        #[arg(long, default_value = "user")]
        kind: String,
    },
    /// Rename a principal handle while retaining the previous handle as an alias.
    RenameHandle {
        /// Path to the `.loom` file.
        store: String,
        /// Principal UUID.
        principal: String,
        /// Replacement handle used by `@` references.
        handle: String,
    },
    /// Set or replace a principal passphrase.
    SetPassphrase {
        /// Path to the `.loom` file.
        store: String,
        /// Principal UUID.
        principal: String,
        /// Key source for the principal passphrase: `prompt` (default), `file:<path>`,
        /// or `fd:<n>`.
        #[arg(long)]
        new_key_source: Option<String>,
    },
    /// Create an app-specific API key for a principal and print the secret once.
    CreateAppCredential {
        /// Path to the `.loom` file.
        store: String,
        /// Principal UUID.
        principal: String,
        /// Human-readable credential label.
        label: String,
    },
    /// Revoke an app-specific API key by credential id.
    RevokeAppCredential {
        /// Path to the `.loom` file.
        store: String,
        /// Credential UUID.
        credential: String,
    },
    /// Create an external-provider credential binding for a principal.
    CreateExternalCredential {
        /// Path to the `.loom` file.
        store: String,
        /// Principal UUID.
        principal: String,
        /// Credential kind: public-key, mtls-certificate, passkey, oidc-subject, or saml-subject.
        kind: String,
        /// Human-readable credential label.
        label: String,
        /// Provider issuer, authority, tenant, or trust root identifier.
        #[arg(long)]
        issuer: String,
        /// Provider subject, key id, certificate subject, or federated subject identifier.
        #[arg(long)]
        subject: String,
        /// Optional digest that pins the public key, certificate, metadata, or provider material.
        #[arg(long)]
        material_digest: Option<String>,
    },
    /// Revoke an external-provider credential by credential id.
    RevokeExternalCredential {
        /// Path to the `.loom` file.
        store: String,
        /// Credential UUID.
        credential: String,
    },
    /// Add, list, and revoke principal public verification keys.
    PublicKey {
        #[command(subcommand)]
        action: IdentityPublicKeyCmd,
    },
    /// Force this store to become its own policy authority.
    ForceDetachAuthority {
        /// Path to the `.loom` file.
        store: String,
        /// Principal UUID that becomes the local authority.
        principal: String,
        /// Monotonic authority generation greater than the current generation.
        #[arg(long)]
        generation: u64,
        /// Operator-visible reason recorded in the identity state and audit log.
        #[arg(long)]
        reason: String,
    },
    /// Print the current authority witness publication record.
    AuthorityWitness {
        /// Path to the `.loom` file.
        store: String,
    },
    /// Replicate signed authority state from another store into this store.
    ReplicateAuthority {
        /// Destination `.loom` file.
        store: String,
        /// Source `.loom` file to fast-forward from.
        #[arg(long)]
        source: String,
        /// Keep the destination in authority mode after applying the source snapshot.
        #[arg(long)]
        become_authority: bool,
    },
    /// Configure an automatic authority replication source for this store.
    ConfigureAuthorityReplication {
        /// Path to the `.loom` file.
        store: String,
        /// Stable policy id.
        id: String,
        /// Source `.loom` file to fast-forward from.
        #[arg(long)]
        source: String,
        /// Disable the policy after saving it.
        #[arg(long)]
        disabled: bool,
        /// Pull from this source when the daemon starts.
        #[arg(long, default_value_t = true)]
        pull_on_start: bool,
        /// Repeat pull interval in milliseconds. Omit for start-only.
        #[arg(long)]
        interval_ms: Option<u64>,
        /// Jitter window in milliseconds applied by daemon schedulers.
        #[arg(long, default_value_t = 0)]
        jitter_ms: u64,
        /// Backoff in milliseconds after a failed pull.
        #[arg(long, default_value_t = 60000)]
        backoff_ms: u64,
        /// Publish a witness report after an applied pull.
        #[arg(long, default_value_t = true)]
        publish_witness: bool,
    },
    /// Print configured authority replication sources.
    ListAuthorityReplication {
        /// Path to the `.loom` file.
        store: String,
    },
    /// Remove a configured authority replication source.
    RemoveAuthorityReplication {
        /// Path to the `.loom` file.
        store: String,
        /// Stable policy id.
        id: String,
    },
    /// Remove a principal.
    Remove {
        /// Path to the `.loom` file.
        store: String,
        /// Principal UUID.
        principal: String,
    },
    /// Assign a role to a principal.
    AssignRole {
        /// Path to the `.loom` file.
        store: String,
        /// Principal UUID.
        principal: String,
        /// Role UUID.
        role: String,
    },
    /// Revoke a role from a principal.
    RevokeRole {
        /// Path to the `.loom` file.
        store: String,
        /// Principal UUID.
        principal: String,
        /// Role UUID.
        role: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum IdentityPublicKeyCmd {
    /// Add a principal-bound public verification key.
    Add {
        /// Path to the `.loom` file.
        store: String,
        /// Principal UUID.
        principal: String,
        /// Human-readable key label.
        label: String,
        /// Verification algorithm. Currently `ES256`.
        #[arg(long, default_value = "ES256")]
        algorithm: String,
        /// SEC1 public key bytes encoded as lowercase or uppercase hex.
        #[arg(long)]
        public_key_hex: String,
    },
    /// Print public verification keys as JSON.
    List {
        /// Path to the `.loom` file.
        store: String,
    },
    /// Revoke a public verification key by key id.
    Revoke {
        /// Path to the `.loom` file.
        store: String,
        /// Public key UUID.
        key: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum AclCmd {
    /// Print direct ACL grants as JSON.
    List {
        /// Path to the `.loom` file.
        store: String,
    },
    /// Add a direct ACL grant.
    Grant {
        /// Path to the `.loom` file.
        store: String,
        /// Effect: allow or deny.
        #[arg(long)]
        effect: String,
        /// Subject: `*`, `everyone`, a principal UUID, or `role:<UUID>`.
        #[arg(long)]
        subject: String,
        /// Right to grant. Repeat for multiple rights.
        #[arg(long = "right", required = true)]
        rights: Vec<String>,
        /// Workspace UUID or name. Omit for a global grant.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: Option<String>,
        /// Authorization domain. Omit for all domains.
        #[arg(long)]
        domain: Option<String>,
        /// Ref glob. Omit for all refs.
        #[arg(long)]
        ref_glob: Option<String>,
        /// Scoped prefix as KIND:PREFIX. Repeat for multiple scopes.
        #[arg(long = "scope")]
        scopes: Vec<String>,
        /// CEL expression for a conditional grant.
        #[arg(long = "predicate-cel")]
        predicate_cel: Option<String>,
    },
    /// Remove a direct ACL grant that exactly matches the supplied fields.
    Revoke {
        /// Path to the `.loom` file.
        store: String,
        /// Effect: allow or deny.
        #[arg(long)]
        effect: String,
        /// Subject: `*`, `everyone`, a principal UUID, or `role:<UUID>`.
        #[arg(long)]
        subject: String,
        /// Right to revoke. Repeat for multiple rights.
        #[arg(long = "right", required = true)]
        rights: Vec<String>,
        /// Workspace UUID or name. Omit for a global grant.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: Option<String>,
        /// Authorization domain. Omit for all domains.
        #[arg(long)]
        domain: Option<String>,
        /// Ref glob. Omit for all refs.
        #[arg(long)]
        ref_glob: Option<String>,
        /// Scoped prefix as KIND:PREFIX. Repeat for multiple scopes.
        #[arg(long = "scope")]
        scopes: Vec<String>,
        /// CEL expression on the conditional grant to remove.
        #[arg(long = "predicate-cel")]
        predicate_cel: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum ProtectedRefCmd {
    /// List protected-ref policies for one workspace as JSON.
    List {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
    },
    /// Print one protected-ref policy as JSON, or null when absent.
    Get {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Exact ref: branch/name or tag/name.
        ref_name: String,
    },
    /// Create or replace a protected-ref policy.
    Set {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Exact ref: branch/name or tag/name.
        ref_name: String,
        /// Require updates to be fast-forward ref advances.
        #[arg(long)]
        fast_forward_only: bool,
        /// Require commit signatures before the ref may advance.
        #[arg(long)]
        signed_commits_required: bool,
        /// Require a signed ref-advance record before the ref may advance.
        #[arg(long)]
        signed_ref_advance_required: bool,
        /// Required approved review count before the ref may advance.
        #[arg(long, default_value_t = 0)]
        required_review_count: u32,
        /// Reject ref deletion.
        #[arg(long)]
        retention_lock: bool,
        /// Reject ref deletion until governance records unlock it.
        #[arg(long)]
        governance_lock: bool,
    },
    /// Remove a protected-ref policy.
    Remove {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Exact ref: branch/name or tag/name.
        ref_name: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum ManagementKvCmd {
    /// Manage a named KV map's durable storage-tier config.
    Config {
        #[command(subcommand)]
        action: ManagementKvConfigCmd,
    },
}

#[derive(Subcommand)]
pub(crate) enum ManagementKvConfigCmd {
    /// Set a named KV map's storage tier config.
    Set {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// KV map name.
        name: String,
        /// Tier: versioned or ephemeral.
        #[arg(long)]
        tier: String,
        /// Default TTL in milliseconds for ephemeral puts. Omit or pass 0 for no default.
        #[arg(long, default_value_t = 0)]
        default_ttl_ms: u64,
        /// Default idle TTL in milliseconds for ephemeral puts. Omit or pass 0 for no default.
        #[arg(long, default_value_t = 0)]
        default_idle_ttl_ms: u64,
        /// Populate an ephemeral cache from the versioned backing map on miss.
        #[arg(long)]
        read_through: bool,
        /// Write the backing versioned map synchronously before populating the cache.
        #[arg(long)]
        write_through: bool,
    },
    /// Print a named KV map's durable storage-tier config as JSON.
    Get {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// KV map name.
        name: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum MetricsCmd {
    /// Store a canonical metric descriptor CBOR record.
    PutDescriptor {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Descriptor CBOR file, or `-` for standard input.
        #[arg(long, default_value = "-")]
        input: String,
    },
    /// Read a canonical metric descriptor CBOR record.
    GetDescriptor {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Descriptor name.
        name: String,
        /// Output file. Omit to write bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Store a canonical metric observation CBOR record.
    PutObservation {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Descriptor name.
        descriptor: String,
        /// Observation CBOR file, or `-` for standard input.
        #[arg(long, default_value = "-")]
        input: String,
    },
    /// Query canonical metric observations and return `[observations, partial, stale]` as CBOR.
    Query {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Descriptor name.
        descriptor: String,
        /// Inclusive start timestamp in milliseconds.
        #[arg(long)]
        from: u64,
        /// Exclusive end timestamp in milliseconds.
        #[arg(long)]
        to: u64,
        /// Maximum scanned series.
        #[arg(long, default_value_t = 1024)]
        max_series: u32,
        /// Maximum returned groups.
        #[arg(long, default_value_t = 1024)]
        max_groups: u32,
        /// Maximum returned samples.
        #[arg(long, default_value_t = 4096)]
        max_samples: u32,
        /// Maximum encoded output bytes.
        #[arg(long, default_value_t = 1048576)]
        max_output_bytes: u64,
        /// Query evaluation timestamp in milliseconds.
        #[arg(long, default_value_t = 0)]
        now: u64,
        /// Output file. Omit to write bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum LogsCmd {
    /// Store a canonical log record CBOR record.
    PutRecord {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Log record CBOR file, or `-` for standard input.
        #[arg(long, default_value = "-")]
        input: String,
    },
    /// Read a canonical log record CBOR record by record id.
    GetRecord {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Log record id.
        record_id: String,
        /// Output file. Omit to write bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Query canonical log records and return `[records, partial]` as CBOR.
    Query {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Inclusive start timestamp in nanoseconds.
        #[arg(long)]
        from: u64,
        /// Exclusive end timestamp in nanoseconds.
        #[arg(long)]
        to: u64,
        /// Maximum returned records.
        #[arg(long, default_value_t = 4096)]
        max_records: u32,
        /// Maximum encoded output bytes.
        #[arg(long, default_value_t = 1048576)]
        max_output_bytes: u64,
        /// Output file. Omit to write bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum TracesCmd {
    /// Store a canonical span CBOR record.
    PutSpan {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Span CBOR file, or `-` for standard input.
        #[arg(long, default_value = "-")]
        input: String,
    },
    /// Read a canonical span CBOR record.
    GetSpan {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Trace id as lowercase hex.
        trace_id: String,
        /// Span id as lowercase hex.
        span_id: String,
        /// Output file. Omit to write bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Query one trace and return `[spans, partial]` as CBOR.
    TraceSpans {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Trace id as lowercase hex.
        trace_id: String,
        /// Maximum returned spans.
        #[arg(long, default_value_t = 4096)]
        max_spans: u32,
        /// Maximum encoded output bytes.
        #[arg(long, default_value_t = 1048576)]
        max_output_bytes: u64,
        /// Output file. Omit to write bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Query spans by start time and return `[spans, partial]` as CBOR.
    Query {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(value_name = "UUID|NAME")]
        workspace: String,
        /// Inclusive start timestamp in nanoseconds.
        #[arg(long)]
        from: u64,
        /// Exclusive end timestamp in nanoseconds.
        #[arg(long)]
        to: u64,
        /// Maximum returned spans.
        #[arg(long, default_value_t = 4096)]
        max_spans: u32,
        /// Maximum encoded output bytes.
        #[arg(long, default_value_t = 1048576)]
        max_output_bytes: u64,
        /// Output file. Omit to write bytes to stdout.
        #[arg(long)]
        out: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum TableCmd {
    /// Print each current row of a table with the commit that last set it (row-level blame), for the
    /// workspace's current HEAD branch.
    Blame {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Table path in the working tree, e.g. `.loom/facets/sql/main/tables/users`.
        table: String,
    },
    /// Show the row-level diff (added / updated / removed) of a table between two commits.
    Diff {
        /// Path to the `.loom` file.
        store: String,
        /// Workspace UUID or name.
        #[arg(long, value_name = "UUID|NAME")]
        workspace: String,
        /// Table path in the working tree, e.g. `.loom/facets/sql/main/tables/users`.
        table: String,
        /// Base commit (content address).
        from: String,
        /// Other commit (content address).
        to: String,
    },
}
