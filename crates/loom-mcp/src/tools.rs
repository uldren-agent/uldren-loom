//! The curated MCP tool surface.
//!
//! This catalog defines every tool the host exposes, the area it lives in, the IDL interface and method
//! it projects, and whether it reads or writes. It is
//! intentionally not a 1:1 emission of the IDL. Binding-ergonomic interfaces (sessions, key
//! administration, result decoding, stateful file handles, async plumbing) are folded into the host or
//! returned natively and never appear here.
//!
//! Two test layers keep this honest:
//!
//! - **Drift**: the catalog and documented tool table must list exactly the same tool names.
//! - **Coverage**: every tool's `idl_method` must be a real method on its `idl_interface`, and every
//!   method of a projected interface must be either projected as a tool or named in [`EXCLUDED`] as a
//!   deliberate fold/drop. A new IDL method that is neither fails the test, forcing a decision.
//!
//! Licensed under BUSL-1.1.

/// Whether a tool reads engine state or mutates it. Used to classify the authority needed by the policy
/// enforcement point.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolKind {
    /// Reads state; no mutation, no commit.
    Read,
    /// Mutates state (and, where applicable, persists or commits).
    Write,
}

/// How a tool can be served when the MCP host is backed by a remote Loom endpoint (`loom mcp` against a
/// URL or remote alias). Derived from the tool's IDL projection, so it cannot drift from the catalog.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemoteCapability {
    /// Projects a unary IDL method; forwarded to the generated `LoomClient` method over the wire.
    Unary,
    /// Projects an IDL method on a handle/stream interface (`Sql`, `Watch`, `Dataframe`); remote-capable
    /// through the remote handle/stream machinery.
    HandleStream,
    /// No single IDL method (a host-level or composite feature); not available against a remote store and
    /// rejected with an explicit local-only error.
    LocalOnly,
}

/// The `(interface, idl_method)` pairs whose exposed MCP tool genuinely needs the remote host to reject
/// at the gate because it can only run against a local handle/stream with no per-request bridge. This is
/// currently empty: every MCP tool with an IDL method is a unary request/response in the MCP surface and
/// is either forwarded over remote or rejected inside its own method with a precise, current-behavior
/// error (the same pattern `document_query` uses). Specifically, `sql_exec` opens a per-request
/// `SqlSession` inside the backend (open -> exec -> close) and forwards byte-clean `exec_cbor`; `sql_query`
/// and `sql_commit` are Unary in the surface but reject in-method. `sql_query` rejects because the IDL
/// `sql_query` stream yields rows only and drops the statement labels/structure the tool's `exec_cbor`
/// result carries, and `sql_commit` because the IDL method carries no caller `timestamp_ms`, so the
/// content-addressed commit digest would diverge.
const HANDLE_STREAM_METHODS: &[(&str, &str)] = &[];

/// One curated tool in the MCP surface.
#[derive(Clone, Copy, Debug)]
pub struct ToolSpec {
    /// The wire name, `<area>.<verb>` in snake_case.
    pub name: &'static str,
    /// The lower-case area (facet or subsystem) the tool belongs to.
    pub area: &'static str,
    /// The IDL interface this tool projects.
    pub idl_interface: &'static str,
    /// The IDL method this tool projects.
    pub idl_method: Option<&'static str>,
    /// Whether the tool reads or writes.
    pub kind: ToolKind,
}

const fn read(
    name: &'static str,
    area: &'static str,
    idl_interface: &'static str,
    idl_method: Option<&'static str>,
) -> ToolSpec {
    ToolSpec {
        name,
        area,
        idl_interface,
        idl_method,
        kind: ToolKind::Read,
    }
}

const fn write(
    name: &'static str,
    area: &'static str,
    idl_interface: &'static str,
    idl_method: Option<&'static str>,
) -> ToolSpec {
    ToolSpec {
        name,
        area,
        idl_interface,
        idl_method,
        kind: ToolKind::Write,
    }
}

/// The curated tool surface. Ordered by area.
pub const TOOL_SURFACE: &[ToolSpec] = &[
    // store
    read("store_version", "store", "Store", Some("version")),
    read("store_capabilities", "store", "Store", Some("capabilities")),
    read(
        "store_capabilities_json",
        "store",
        "Store",
        Some("capabilities"),
    ),
    read("store_blob_digest", "store", "Store", Some("blob_digest")),
    read("store_maintenance_status", "store", "StoreAdmin", None),
    write("store_maintenance_policy_set", "store", "StoreAdmin", None),
    write("store_maintenance_run", "store", "StoreAdmin", None),
    // telemetry
    write(
        "metrics_put_descriptor",
        "metrics",
        "Metrics",
        Some("put_descriptor"),
    ),
    read(
        "metrics_get_descriptor",
        "metrics",
        "Metrics",
        Some("get_descriptor"),
    ),
    write(
        "metrics_put_observation",
        "metrics",
        "Metrics",
        Some("put_observation"),
    ),
    read("metrics_query", "metrics", "Metrics", Some("query")),
    write("logs_put_record", "logs", "Logs", Some("put_record")),
    read("logs_get_record", "logs", "Logs", Some("get_record")),
    read("logs_query", "logs", "Logs", Some("query")),
    write("traces_put_span", "traces", "Traces", Some("put_span")),
    read("traces_get_span", "traces", "Traces", Some("get_span")),
    read(
        "traces_trace_spans",
        "traces",
        "Traces",
        Some("trace_spans"),
    ),
    read("traces_query", "traces", "Traces", Some("query")),
    // workspace
    read(
        "workspace_list",
        "workspace",
        "Workspaces",
        Some("workspace_list"),
    ),
    // vcs
    write("vcs_commit", "vcs", "VersionControl", Some("commit")),
    write("vcs_branch", "vcs", "VersionControl", Some("branch")),
    write("vcs_checkout", "vcs", "VersionControl", Some("checkout")),
    read(
        "vcs_head_branch",
        "vcs",
        "VersionControl",
        Some("head_branch"),
    ),
    read("vcs_log", "vcs", "VersionControl", Some("log")),
    write("vcs_merge", "vcs", "VersionControl", Some("merge")),
    read(
        "vcs_merge_in_progress",
        "vcs",
        "VersionControl",
        Some("merge_in_progress"),
    ),
    read(
        "vcs_merge_conflicts",
        "vcs",
        "VersionControl",
        Some("merge_conflicts"),
    ),
    write(
        "vcs_merge_resolve",
        "vcs",
        "VersionControl",
        Some("merge_resolve"),
    ),
    write(
        "vcs_merge_abort",
        "vcs",
        "VersionControl",
        Some("merge_abort"),
    ),
    write(
        "vcs_merge_continue",
        "vcs",
        "VersionControl",
        Some("merge_continue"),
    ),
    read("vcs_status", "vcs", "VersionControl", Some("status")),
    write("vcs_stage", "vcs", "VersionControl", Some("stage")),
    write("vcs_stage_all", "vcs", "VersionControl", Some("stage_all")),
    write("vcs_unstage", "vcs", "VersionControl", Some("unstage")),
    write(
        "vcs_commit_staged",
        "vcs",
        "VersionControl",
        Some("commit_staged"),
    ),
    write(
        "vcs_tag_create",
        "vcs",
        "VersionControl",
        Some("tag_create"),
    ),
    read("vcs_tag_list", "vcs", "VersionControl", Some("tag_list")),
    read(
        "vcs_tag_target",
        "vcs",
        "VersionControl",
        Some("tag_target"),
    ),
    write(
        "vcs_tag_delete",
        "vcs",
        "VersionControl",
        Some("tag_delete"),
    ),
    write(
        "vcs_tag_rename",
        "vcs",
        "VersionControl",
        Some("tag_rename"),
    ),
    write(
        "vcs_restore_file",
        "vcs",
        "VersionControl",
        Some("restore_file"),
    ),
    write(
        "vcs_restore_path",
        "vcs",
        "VersionControl",
        Some("restore_path"),
    ),
    write(
        "vcs_cherry_pick",
        "vcs",
        "VersionControl",
        Some("cherry_pick"),
    ),
    write("vcs_revert", "vcs", "VersionControl", Some("revert")),
    write("vcs_rebase", "vcs", "VersionControl", Some("rebase")),
    write("vcs_squash", "vcs", "VersionControl", Some("squash")),
    read("vcs_diff", "vcs", "VersionControl", Some("diff")),
    read("vcs_blame", "vcs", "VersionControl", Some("blame")),
    // watch
    read("watch_subscribe", "watch", "Watch", Some("subscribe")),
    read("watch_poll", "watch", "Watch", Some("poll")),
    // fs
    write("fs_write_file", "fs", "FileSystem", Some("write_file")),
    read("fs_read_file", "fs", "FileSystem", Some("read_file")),
    write("fs_append_file", "fs", "FileSystem", Some("append_file")),
    write("fs_remove_file", "fs", "FileSystem", Some("remove_file")),
    read("fs_read_at", "fs", "FileSystem", Some("read_at")),
    read("fs_stat", "fs", "FileSystem", Some("stat")),
    read(
        "fs_list_directory",
        "fs",
        "FileSystem",
        Some("list_directory"),
    ),
    write(
        "fs_create_directory",
        "fs",
        "FileSystem",
        Some("create_directory"),
    ),
    write(
        "fs_remove_directory",
        "fs",
        "FileSystem",
        Some("remove_directory"),
    ),
    write("fs_write_at", "fs", "FileSystem", Some("write_at")),
    write("fs_truncate", "fs", "FileSystem", Some("truncate")),
    write("fs_symlink", "fs", "FileSystem", Some("symlink")),
    read("fs_read_link", "fs", "FileSystem", Some("read_link")),
    // apps
    read("apps_list", "apps", "FileSystem", None),
    read("apps_show", "apps", "FileSystem", None),
    read("apps_read_file", "apps", "FileSystem", None),
    write("apps_create", "apps", "FileSystem", None),
    write("apps_write_file", "apps", "FileSystem", None),
    write("apps_remove_file", "apps", "FileSystem", None),
    write("apps_call_tool", "apps", "FileSystem", None),
    // ask
    write("ask_questions", "ask", "Document", None),
    read("ask_answers", "ask", "Document", None),
    write("ask_record", "ask", "Document", None),
    // cas
    write("cas_put", "cas", "Cas", Some("put")),
    read("cas_get", "cas", "Cas", Some("get")),
    read("cas_has", "cas", "Cas", Some("has")),
    write("cas_delete", "cas", "Cas", Some("delete")),
    read("cas_list", "cas", "Cas", Some("list")),
    // graph
    write("graph_upsert_node", "graph", "Graph", Some("upsert_node")),
    read("graph_get_node", "graph", "Graph", Some("get_node")),
    write("graph_remove_node", "graph", "Graph", Some("remove_node")),
    write("graph_upsert_edge", "graph", "Graph", Some("upsert_edge")),
    read("graph_get_edge", "graph", "Graph", Some("get_edge")),
    write("graph_remove_edge", "graph", "Graph", Some("remove_edge")),
    read("graph_neighbors", "graph", "Graph", Some("neighbors")),
    read("graph_out_edges", "graph", "Graph", Some("out_edges")),
    read("graph_in_edges", "graph", "Graph", Some("in_edges")),
    read("graph_reachable", "graph", "Graph", Some("reachable")),
    read(
        "graph_shortest_path",
        "graph",
        "Graph",
        Some("shortest_path"),
    ),
    read("graph_query", "graph", "Graph", Some("query")),
    read(
        "graph_explain_query",
        "graph",
        "Graph",
        Some("explain_query"),
    ),
    // vector
    write("vector_create", "vector", "Vector", Some("create")),
    write("vector_upsert", "vector", "Vector", Some("upsert")),
    write(
        "vector_upsert_source",
        "vector",
        "Vector",
        Some("upsert_source"),
    ),
    read("vector_get", "vector", "Vector", Some("get")),
    read(
        "vector_source_text",
        "vector",
        "Vector",
        Some("source_text"),
    ),
    read(
        "vector_embedding_model",
        "vector",
        "Vector",
        Some("embedding_model"),
    ),
    read("vector_ids", "vector", "Vector", Some("ids")),
    read(
        "vector_metadata_index_keys",
        "vector",
        "Vector",
        Some("metadata_index_keys"),
    ),
    write(
        "vector_create_metadata_index",
        "vector",
        "Vector",
        Some("create_metadata_index"),
    ),
    write(
        "vector_drop_metadata_index",
        "vector",
        "Vector",
        Some("drop_metadata_index"),
    ),
    write("vector_delete", "vector", "Vector", Some("delete")),
    read("vector_search", "vector", "Vector", Some("search")),
    read(
        "vector_search_policy",
        "vector",
        "Vector",
        Some("search_policy"),
    ),
    // columnar
    write("columnar_create", "columnar", "Columnar", Some("create")),
    write("columnar_append", "columnar", "Columnar", Some("append")),
    write("columnar_compact", "columnar", "Columnar", Some("compact")),
    read("columnar_scan", "columnar", "Columnar", Some("scan")),
    read("columnar_columns", "columnar", "Columnar", Some("columns")),
    read("columnar_rows", "columnar", "Columnar", Some("rows")),
    read("columnar_inspect", "columnar", "Columnar", Some("inspect")),
    read(
        "columnar_source_digest",
        "columnar",
        "Columnar",
        Some("source_digest"),
    ),
    read("columnar_select", "columnar", "Columnar", Some("select")),
    read(
        "columnar_aggregate",
        "columnar",
        "Columnar",
        Some("aggregate"),
    ),
    // dataframe
    write("dataframe_create", "dataframe", "Dataframe", Some("create")),
    read(
        "dataframe_collect",
        "dataframe",
        "Dataframe",
        Some("collect"),
    ),
    read(
        "dataframe_preview",
        "dataframe",
        "Dataframe",
        Some("preview"),
    ),
    write(
        "dataframe_materialize",
        "dataframe",
        "Dataframe",
        Some("materialize"),
    ),
    read(
        "dataframe_plan_digest",
        "dataframe",
        "Dataframe",
        Some("plan_digest"),
    ),
    read(
        "dataframe_source_digests",
        "dataframe",
        "Dataframe",
        Some("source_digests"),
    ),
    // fts
    write("fts_create", "fts", "Search", Some("create")),
    write("fts_index", "fts", "Search", Some("index")),
    read("fts_get", "fts", "Search", Some("get")),
    write("fts_delete", "fts", "Search", Some("delete")),
    read("fts_ids", "fts", "Search", Some("ids")),
    write("fts_remap", "fts", "Search", Some("remap")),
    read("fts_query", "fts", "Search", Some("query")),
    read("fts_source_digest", "fts", "Search", Some("source_digest")),
    read("fts_status", "fts", "Search", Some("status")),
    // tools
    read("search", "search", "Search", None),
    // substrate
    read("substrate_changes", "substrate", "Search", None),
    read("workgraph_changes", "workgraph", "Search", None),
    read("workgraph_metrics", "workgraph", "Search", None),
    write("workgraph_fact_put", "workgraph", "Search", None),
    read("substrate_refs", "substrate", "Search", None),
    write("substrate_alias_bind", "substrate", "Search", None),
    write("substrate_alias_release", "substrate", "Search", None),
    read("substrate_alias_resolve", "substrate", "Search", None),
    read("substrate_alias_list", "substrate", "Search", None),
    read("substrate_reference_status", "substrate", "Search", None),
    write("substrate_reference_reconcile", "substrate", "Store", None),
    read("substrate_history", "substrate", "Search", None),
    read("substrate_revision_latest", "substrate", "Search", None),
    read("substrate_revision_at", "substrate", "Search", None),
    read("substrate_revision_as_of_root", "substrate", "Search", None),
    read("substrate_checkpoint_before", "substrate", "Search", None),
    write("substrate_transact", "substrate", "Search", None),
    write("substrate_view_define", "substrate", "Search", None),
    read("substrate_view_get", "substrate", "Search", None),
    read("substrate_view_list", "substrate", "Search", None),
    read(
        "substrate_write_admission_policy_get",
        "substrate",
        "Search",
        None,
    ),
    write(
        "substrate_write_admission_policy_set",
        "substrate",
        "Search",
        None,
    ),
    // tickets
    write("tickets_project_create", "tickets", "Store", None),
    write("tickets_project_rekey", "tickets", "Store", None),
    read("tickets_project_settings_get", "tickets", "Store", None),
    write("tickets_project_settings_set", "tickets", "Store", None),
    read("tickets_projects", "tickets", "Store", None),
    read("tickets_relations", "tickets", "Store", None),
    read("tickets_fields", "tickets", "Store", None),
    write("tickets_field_put", "tickets", "Store", None),
    write("tickets_field_retire", "tickets", "Store", None),
    write("tickets_create", "tickets", "Store", None),
    write("tickets_update", "tickets", "Store", None),
    write("tickets_delete", "tickets", "Store", None),
    read("tickets_comments", "tickets", "Store", None),
    write("tickets_comment_add", "tickets", "Store", None),
    write("tickets_comment_update", "tickets", "Store", None),
    write("tickets_comment_delete", "tickets", "Store", None),
    write("tickets_board_create", "tickets", "Store", None),
    write("tickets_board_update", "tickets", "Store", None),
    write("tickets_board_delete", "tickets", "Store", None),
    write("tickets_board_configure_columns", "tickets", "Store", None),
    write("tickets_board_move_card", "tickets", "Store", None),
    write("tickets_relation_set", "tickets", "Store", None),
    write("tickets_relation_remove", "tickets", "Store", None),
    read("tickets_get", "tickets", "Store", None),
    read("tickets_list", "tickets", "Store", None),
    read("tickets_board_get", "tickets", "Store", None),
    read("tickets_board_list", "tickets", "Store", None),
    read("tickets_history", "tickets", "Store", None),
    // lanes
    write("lanes_create", "lanes", "Lanes", Some("create")),
    read("lanes_get", "lanes", "Lanes", Some("get")),
    read("lanes_list", "lanes", "Lanes", Some("list")),
    write("lanes_update", "lanes", "Lanes", Some("update")),
    write("lanes_ticket_add", "lanes", "Lanes", Some("ticket_add")),
    write(
        "lanes_ticket_remove",
        "lanes",
        "Lanes",
        Some("ticket_remove"),
    ),
    write(
        "lanes_ticket_transfer",
        "lanes",
        "Lanes",
        Some("ticket_transfer"),
    ),
    write("lanes_delete", "lanes", "Lanes", Some("delete")),
    // spaces and pages
    write("spaces_create", "spaces", "Store", None),
    read("spaces_get", "spaces", "Store", None),
    read("spaces_list", "spaces", "Store", None),
    write("pages_create", "pages", "Store", None),
    write("pages_update", "pages", "Store", None),
    write("pages_publish", "pages", "Store", None),
    read("pages_get", "pages", "Store", None),
    read("pages_list", "pages", "Store", None),
    read("pages_history", "pages", "Store", None),
    // lifecycles
    write("lifecycles_define", "lifecycles", "Store", None),
    write("lifecycles_define_standard", "lifecycles", "Store", None),
    read("lifecycles_definitions", "lifecycles", "Store", None),
    read("lifecycles_definition", "lifecycles", "Store", None),
    write("lifecycles_instantiate", "lifecycles", "Store", None),
    read("lifecycles_instances", "lifecycles", "Store", None),
    read("lifecycles_instance", "lifecycles", "Store", None),
    write("lifecycles_active_set", "lifecycles", "Store", None),
    write("lifecycles_active_clear", "lifecycles", "Store", None),
    read("lifecycles_snapshot_plan", "lifecycles", "Store", None),
    read("lifecycles_current_surface", "lifecycles", "Store", None),
    write("lifecycles_transition", "lifecycles", "Store", None),
    read("lifecycles_snapshots", "lifecycles", "Store", None),
    read("lifecycles_snapshot", "lifecycles", "Store", None),
    read("lifecycles_snapshot_content", "lifecycles", "Store", None),
    read("lifecycles_operation_log", "lifecycles", "Store", None),
    // chat
    read("chat_channels", "chat", "Store", None),
    read("chat_fetch_events", "chat", "Store", None),
    read("chat_messages", "chat", "Store", None),
    read("chat_cursor", "chat", "Store", None),
    read("chat_presence", "chat", "Store", None),
    write("chat_create_channel", "chat", "Store", None),
    write("chat_rename_channel", "chat", "Store", None),
    write("chat_post_message", "chat", "Store", None),
    write("chat_edit_message", "chat", "Store", None),
    write("chat_redact_message", "chat", "Store", None),
    read("chat_emoji_list", "chat", "Store", None),
    write("chat_emoji_register", "chat", "Store", None),
    write("chat_emoji_unregister", "chat", "Store", None),
    write("chat_add_reaction", "chat", "Store", None),
    write("chat_remove_reaction", "chat", "Store", None),
    write("chat_create_thread", "chat", "Store", None),
    write("chat_create_task", "chat", "Store", None),
    write("chat_claim_task", "chat", "Store", None),
    write("chat_complete_task", "chat", "Store", None),
    write("chat_invoke_agent", "chat", "Store", None),
    write("chat_agent_reply", "chat", "Store", None),
    write("chat_request_handoff", "chat", "Store", None),
    write("chat_update_cursor", "chat", "Store", None),
    write("chat_set_presence", "chat", "Store", None),
    // drive
    read("drive_list", "drive", "Store", None),
    read("drive_stat", "drive", "Store", None),
    read("drive_read", "drive", "Store", None),
    read("drive_list_versions", "drive", "Store", None),
    read("drive_list_conflicts", "drive", "Store", None),
    write("drive_create_folder", "drive", "Store", None),
    write("drive_create_upload", "drive", "Store", None),
    write("drive_upload_chunk", "drive", "Store", None),
    write("drive_commit_upload", "drive", "Store", None),
    write("drive_rename", "drive", "Store", None),
    write("drive_move", "drive", "Store", None),
    write("drive_delete", "drive", "Store", None),
    write("drive_resolve_conflict", "drive", "Store", None),
    read("drive_list_shares", "drive", "Store", None),
    write("drive_grant_share", "drive", "Store", None),
    write("drive_revoke_share", "drive", "Store", None),
    write("drive_apply_share_expiry", "drive", "Store", None),
    read("drive_list_retention", "drive", "Store", None),
    write("drive_pin_retention", "drive", "Store", None),
    write("drive_unpin_retention", "drive", "Store", None),
    write("drive_apply_retention", "drive", "Store", None),
    write("drive_acquire_lease", "drive", "Store", None),
    write("drive_refresh_lease", "drive", "Store", None),
    write("drive_release_lease", "drive", "Store", None),
    write("drive_break_lease", "drive", "Store", None),
    // meetings
    read("meetings_list", "meetings", "Store", None),
    read("meetings_get", "meetings", "Store", None),
    read("meetings_search", "meetings", "Store", None),
    read("meetings_projection_outputs", "meetings", "Store", None),
    read("meetings_extraction_review", "meetings", "Store", None),
    write("meetings_accept_annotation", "meetings", "Store", None),
    write("meetings_reject_annotation", "meetings", "Store", None),
    write("meetings_propose_vocabulary", "meetings", "Store", None),
    write("meetings_accept_vocabulary", "meetings", "Store", None),
    write("meetings_reject_vocabulary", "meetings", "Store", None),
    write("meetings_add_entity_merge", "meetings", "Store", None),
    write("meetings_add_promotion", "meetings", "Store", None),
    write("meetings_promote_task_to_ticket", "meetings", "Store", None),
    write(
        "meetings_promote_decision_to_decision_log",
        "meetings",
        "Store",
        None,
    ),
    write(
        "meetings_promote_question_to_lifecycle",
        "meetings",
        "Store",
        None,
    ),
    write(
        "meetings_promote_artifact_to_reference_artifact",
        "meetings",
        "Store",
        None,
    ),
    write(
        "meetings_promote_reference_to_reference_artifact",
        "meetings",
        "Store",
        None,
    ),
    write("meetings_import_snapshot", "meetings", "Store", None),
    write("redmine_import_snapshot", "redmine", "Store", None),
    write("studio_reindex", "studio", "Store", None),
    // import
    write("import_submit_batch", "import", "Store", None),
    write("import_execute_batch", "import", "Store", None),
    // structures
    write("structures_create", "structures", "Store", None),
    read("structures_get", "structures", "Store", None),
    read("structures_list", "structures", "Store", None),
    write("structures_add_node", "structures", "Store", None),
    write("structures_update_node", "structures", "Store", None),
    write("structures_move_node", "structures", "Store", None),
    write("structures_link_node", "structures", "Store", None),
    write("structures_bind", "structures", "Store", None),
    write(
        "structures_decompose_to_tickets",
        "structures",
        "Store",
        None,
    ),
    // kv
    write("kv_put", "kv", "Kv", Some("put")),
    read("kv_get", "kv", "Kv", Some("get")),
    write("kv_delete", "kv", "Kv", Some("delete")),
    read("kv_list", "kv", "Kv", Some("list")),
    read("kv_range", "kv", "Kv", Some("range")),
    read("kv_list_collections", "kv", "Kv", Some("list_collections")),
    // document
    write(
        "document_put_text",
        "document",
        "Document",
        Some("put_text"),
    ),
    read(
        "document_get_text",
        "document",
        "Document",
        Some("get_text"),
    ),
    write(
        "document_put_binary",
        "document",
        "Document",
        Some("put_binary"),
    ),
    read(
        "document_get_binary",
        "document",
        "Document",
        Some("get_binary"),
    ),
    read("document_query", "document", "Document", Some("query_json")),
    write("document_replace_text", "document", "Document", None),
    write("document_delete", "document", "Document", Some("delete")),
    read(
        "document_list_binary",
        "document",
        "Document",
        Some("list_binary"),
    ),
    read(
        "document_list_collections",
        "document",
        "Document",
        Some("list_collections"),
    ),
    // timeseries
    write("timeseries_put", "timeseries", "TimeSeries", Some("put")),
    read("timeseries_get", "timeseries", "TimeSeries", Some("get")),
    read(
        "timeseries_range",
        "timeseries",
        "TimeSeries",
        Some("range"),
    ),
    read(
        "timeseries_latest",
        "timeseries",
        "TimeSeries",
        Some("latest"),
    ),
    read(
        "timeseries_list_collections",
        "timeseries",
        "TimeSeries",
        Some("list_collections"),
    ),
    // ledger
    write("ledger_append", "ledger", "Ledger", Some("append")),
    read("ledger_get", "ledger", "Ledger", Some("get")),
    read("ledger_head", "ledger", "Ledger", Some("head")),
    read("ledger_len", "ledger", "Ledger", Some("len")),
    read("ledger_verify", "ledger", "Ledger", Some("verify")),
    read(
        "ledger_list_collections",
        "ledger",
        "Ledger",
        Some("list_collections"),
    ),
    // queue
    write("queue_append", "queue", "Queue", Some("append")),
    read("queue_get", "queue", "Queue", Some("get")),
    read("queue_range", "queue", "Queue", Some("range")),
    read("queue_len", "queue", "Queue", Some("len")),
    read("queue_list_streams", "queue", "Queue", Some("list_streams")),
    read(
        "queue_consumer_position",
        "queue",
        "QueueConsumers",
        Some("consumer_position"),
    ),
    read(
        "queue_consumer_read",
        "queue",
        "QueueConsumers",
        Some("consumer_read"),
    ),
    write(
        "queue_consumer_advance",
        "queue",
        "QueueConsumers",
        Some("consumer_advance"),
    ),
    write(
        "queue_consumer_reset",
        "queue",
        "QueueConsumers",
        Some("consumer_reset"),
    ),
    // calendar
    write(
        "calendar_create_collection",
        "calendar",
        "Calendar",
        Some("create_collection"),
    ),
    read(
        "calendar_get_collection",
        "calendar",
        "Calendar",
        Some("get_collection"),
    ),
    read(
        "calendar_list_collections",
        "calendar",
        "Calendar",
        Some("list_collections"),
    ),
    write(
        "calendar_delete_collection",
        "calendar",
        "Calendar",
        Some("delete_collection"),
    ),
    write(
        "calendar_put_entry",
        "calendar",
        "Calendar",
        Some("put_entry"),
    ),
    write("calendar_put_ics", "calendar", "Calendar", Some("put_ics")),
    read(
        "calendar_get_entry",
        "calendar",
        "Calendar",
        Some("get_entry"),
    ),
    write(
        "calendar_delete_entry",
        "calendar",
        "Calendar",
        Some("delete_entry"),
    ),
    read(
        "calendar_list_entries",
        "calendar",
        "Calendar",
        Some("list_entries"),
    ),
    read("calendar_range", "calendar", "Calendar", Some("range")),
    read("calendar_search", "calendar", "Calendar", Some("search")),
    read("calendar_to_ics", "calendar", "Calendar", Some("to_ics")),
    // contacts
    write(
        "contacts_create_book",
        "contacts",
        "Contacts",
        Some("create_book"),
    ),
    read(
        "contacts_get_book",
        "contacts",
        "Contacts",
        Some("get_book"),
    ),
    read(
        "contacts_list_books",
        "contacts",
        "Contacts",
        Some("list_books"),
    ),
    write(
        "contacts_delete_book",
        "contacts",
        "Contacts",
        Some("delete_book"),
    ),
    write(
        "contacts_put_entry",
        "contacts",
        "Contacts",
        Some("put_entry"),
    ),
    write(
        "contacts_put_vcard",
        "contacts",
        "Contacts",
        Some("put_vcard"),
    ),
    read(
        "contacts_get_entry",
        "contacts",
        "Contacts",
        Some("get_entry"),
    ),
    write(
        "contacts_delete_entry",
        "contacts",
        "Contacts",
        Some("delete_entry"),
    ),
    read(
        "contacts_list_entries",
        "contacts",
        "Contacts",
        Some("list_entries"),
    ),
    read("contacts_search", "contacts", "Contacts", Some("search")),
    read(
        "contacts_to_vcard",
        "contacts",
        "Contacts",
        Some("to_vcard"),
    ),
    // mail
    write(
        "mail_create_mailbox",
        "mail",
        "Mail",
        Some("create_mailbox"),
    ),
    read("mail_get_mailbox", "mail", "Mail", Some("get_mailbox")),
    read(
        "mail_list_mailboxes",
        "mail",
        "Mail",
        Some("list_mailboxes"),
    ),
    write(
        "mail_delete_mailbox",
        "mail",
        "Mail",
        Some("delete_mailbox"),
    ),
    write(
        "mail_ingest_message",
        "mail",
        "Mail",
        Some("ingest_message"),
    ),
    read("mail_get_message", "mail", "Mail", Some("get_message")),
    read("mail_to_eml", "mail", "Mail", Some("to_eml")),
    write(
        "mail_delete_message",
        "mail",
        "Mail",
        Some("delete_message"),
    ),
    read("mail_list_messages", "mail", "Mail", Some("list_messages")),
    read("mail_get_flags", "mail", "Mail", Some("get_flags")),
    write("mail_set_flags", "mail", "Mail", Some("set_flags")),
    read("mail_search", "mail", "Mail", Some("search")),
    // sql
    write("sql_exec", "sql", "Sql", Some("sql_exec")),
    read("sql_query", "sql", "Sql", Some("sql_query")),
    write("sql_commit", "sql", "Sql", Some("sql_commit")),
    read("sql_read_table", "sql", "Sql", Some("sql_read_table")),
    read("sql_read_table_at", "sql", "Sql", Some("sql_read_table_at")),
    read("sql_index_scan", "sql", "Sql", Some("sql_index_scan")),
    read("sql_index_scan_at", "sql", "Sql", Some("sql_index_scan_at")),
    read("sql_diff", "sql", "Sql", Some("sql_diff")),
    read("sql_table_diff", "sql", "Sql", Some("sql_table_diff")),
    read("sql_blame", "sql", "Sql", Some("sql_blame")),
    read(
        "sql_list_databases",
        "sql",
        "Sql",
        Some("sql_list_databases"),
    ),
];

/// IDL methods that are present on a projected interface but deliberately not exposed as tools: store
/// session lifecycle is host launch configuration, SQL sessions and batches are folded, and the async
/// `*_async` forms are surfaced through MCP progress / Tasks, not standalone tools.
pub const EXCLUDED: &[(&str, &[&str])] = &[
    (
        "Workspaces",
        &["workspace_create", "workspace_rename", "workspace_delete"],
    ),
    (
        "Store",
        &[
            "create",
            "create_with_kek",
            "open",
            "open_keyed",
            "open_with_kek",
            "close",
            "runtime_profile",
            // Host-internal: backs the `document_query` composite over remote (the host reads the store's
            // digest algorithm to reproduce per-item `Digest::hash(algo, doc)`); not a standalone tool.
            "digest_algo",
        ],
    ),
    (
        "StoreAdmin",
        &[
            // Maintenance MCP tools are local host operations over the concrete store handle. The raw
            // administrative IDL methods are not projected as remote MCP tools.
            "store_stat",
            "store_policy_get",
            "store_policy_set",
            "store_rekey",
        ],
    ),
    ("VersionControl", &["log_async", "merge_async"]),
    ("Watch", &["stream"]),
    (
        "FileSystem",
        &[
            "export_fs",
            "export_fs_async",
            "import_fs",
            "import_fs_async",
        ],
    ),
    (
        "Document",
        &[
            "index_create",
            "index_create_json",
            "index_drop",
            "index_rebuild",
            "index_list_json",
            "index_status_json",
            "find_json",
            // Host-internal: document write tools call the indexed variant locally and over remote.
            // It is not a separate tool.
            "put_binary_indexed",
            "delete_indexed",
            "replace_text_indexed",
        ],
    ),
    (
        // Host-internal: the `graph_upsert_edge`/`graph_remove_edge` tools call the indexed variant
        // (engine write + reference-index overlay) locally and over remote; raw methods stay overlay-free.
        "Graph",
        &["upsert_edge_indexed", "remove_edge_indexed"],
    ),
    (
        "Lanes",
        &[
            // The view helpers are represented by `lanes_get` and `lanes_list`, which return the
            // persisted lane view shape directly.
            "get_view_json",
            "list_views_json",
            // The MCP surface exposes `delete` through `lanes_delete`; closed-lane validation lives
            // in the shared Lanes implementation.
        ],
    ),
    (
        "Sql",
        &[
            "sql_open",
            "sql_open_keyed",
            "sql_open_with_kek",
            "sql_open_authenticated",
            "sql_open_keyed_authenticated",
            "sql_open_with_kek_authenticated",
            "sql_authenticate_passphrase",
            "sql_close",
            "sql_batch_begin",
            "sql_batch_begin_keyed",
            "sql_batch_begin_with_kek",
            "sql_batch_begin_authenticated",
            "sql_batch_begin_keyed_authenticated",
            "sql_batch_begin_with_kek_authenticated",
            "sql_batch_exec",
            "sql_batch_commit",
            "sql_batch_commit_vcs",
            "sql_batch_abort",
            "sql_batch_close",
            "sql_read_table_async",
            "sql_index_scan_async",
            "sql_blame_async",
            "sql_diff_async",
            // The read-only full-result method backs the `sql_query` tool over remote while preserving
            // full `exec_cbor` parity without persisting.
            "sql_query_result",
        ],
    ),
];

/// IDL interfaces folded into the host or returned natively, with no tools at all:
/// key/wrap administration, workspace lifecycle, management config, stateful file descriptors,
/// daemon lifecycle, locks, result decoding, async task plumbing, and trigger management.
pub const FULLY_FOLDED: &[&str] = &[
    "KeySource",
    "Daemon",
    "Locks",
    "FileHandle",
    "Diagnostics",
    "Tasks",
    "ResultViews",
    "ManagementKv",
    "Triggers",
];

/// The whole curated tool surface.
pub fn tool_surface() -> &'static [ToolSpec] {
    TOOL_SURFACE
}

/// The read-only tools.
pub fn read_tools() -> impl Iterator<Item = &'static ToolSpec> {
    TOOL_SURFACE.iter().filter(|t| t.kind == ToolKind::Read)
}

/// The mutating tools.
pub fn write_tools() -> impl Iterator<Item = &'static ToolSpec> {
    TOOL_SURFACE.iter().filter(|t| t.kind == ToolKind::Write)
}

/// Look up a tool by its wire name.
pub fn tool(name: &str) -> Option<&'static ToolSpec> {
    TOOL_SURFACE.iter().find(|t| t.name == name)
}

impl ToolSpec {
    /// How this tool can be served when the host is backed by a remote Loom endpoint. A tool with no IDL
    /// method is a host-level/composite feature and is local-only; a tool on a handle/stream interface is
    /// remote-capable through the handle/stream machinery; anything else projects a unary IDL method and
    /// is forwarded to the generated `LoomClient` method.
    pub fn remote_capability(&self) -> RemoteCapability {
        match self.idl_method {
            None => RemoteCapability::LocalOnly,
            Some(method) if HANDLE_STREAM_METHODS.contains(&(self.idl_interface, method)) => {
                RemoteCapability::HandleStream
            }
            Some(_) => RemoteCapability::Unary,
        }
    }
}

pub const SERVER_PROMOTED_TOOLS: &[&str] = &[
    "apps_list",
    "apps_show",
    "apps_read_file",
    "apps_create",
    "apps_write_file",
    "apps_remove_file",
    "drive_list",
    "drive_stat",
    "drive_read",
    "drive_list_versions",
    "drive_list_conflicts",
    "drive_list_shares",
    "drive_list_retention",
    "drive_grant_share",
    "drive_revoke_share",
    "drive_apply_share_expiry",
    "drive_pin_retention",
    "drive_unpin_retention",
    "drive_apply_retention",
    "drive_acquire_lease",
    "drive_refresh_lease",
    "drive_release_lease",
    "drive_break_lease",
    "drive_create_folder",
    "drive_create_upload",
    "drive_upload_chunk",
    "drive_commit_upload",
    "drive_rename",
    "drive_move",
    "drive_delete",
    "drive_resolve_conflict",
    "meetings_projection_outputs",
    "meetings_list",
    "meetings_get",
    "meetings_search",
    "meetings_extraction_review",
    "meetings_accept_annotation",
    "meetings_reject_annotation",
    "meetings_propose_vocabulary",
    "meetings_accept_vocabulary",
    "meetings_reject_vocabulary",
    "meetings_add_entity_merge",
    "meetings_add_promotion",
    "meetings_promote_task_to_ticket",
    "meetings_promote_decision_to_decision_log",
    "meetings_promote_question_to_lifecycle",
    "meetings_promote_artifact_to_reference_artifact",
    "meetings_promote_reference_to_reference_artifact",
    "meetings_import_snapshot",
    // `ask_answers` waits client-side and polls the served ask state between attempts.
    "ask_questions",
    "ask_record",
    // chat (Kv + Document + Queue over the substrate chat profile): server-side execution via the shared
    // `LoomMcp` chat facade. `chat_presence`/`chat_set_presence` are deliberately excluded because they operate
    // on in-process ephemeral presence (host-runtime state, not served-store state), like `studio-status`,
    // so they stay local.
    "chat_fetch_events",
    "chat_channels",
    "chat_create_channel",
    "chat_rename_channel",
    "chat_messages",
    "chat_cursor",
    "chat_post_message",
    "chat_edit_message",
    "chat_redact_message",
    "chat_emoji_list",
    "chat_emoji_register",
    "chat_emoji_unregister",
    "chat_add_reaction",
    "chat_remove_reaction",
    "chat_create_thread",
    "chat_create_task",
    "chat_claim_task",
    "chat_complete_task",
    "chat_invoke_agent",
    "chat_agent_reply",
    "chat_request_handoff",
    "chat_update_cursor",
    // spaces / pages / structures (Studio profile-root families over Document + reference): server-side
    // execution via the shared `LoomMcp` facade, preserving expected-root optimistic concurrency (each
    // write threads `expected_root` and commits server-side).
    "spaces_create",
    "spaces_get",
    "spaces_list",
    "pages_create",
    "pages_update",
    "pages_publish",
    "pages_get",
    "pages_list",
    "pages_history",
    "structures_create",
    "structures_get",
    "structures_list",
    "structures_add_node",
    "structures_update_node",
    "structures_move_node",
    "structures_link_node",
    "structures_bind",
    "structures_decompose_to_tickets",
    "substrate_changes",
    "workgraph_metrics",
    "substrate_refs",
    "substrate_alias_bind",
    "substrate_alias_release",
    "substrate_alias_resolve",
    "substrate_alias_list",
    "substrate_reference_status",
    "substrate_reference_reconcile",
    "substrate_history",
    "substrate_revision_latest",
    "substrate_revision_at",
    "substrate_revision_as_of_root",
    "substrate_checkpoint_before",
    "substrate_view_define",
    "substrate_view_get",
    "substrate_view_list",
    "substrate_write_admission_policy_get",
    "substrate_write_admission_policy_set",
    "substrate_transact",
    "tickets_project_create",
    "tickets_project_rekey",
    "tickets_project_settings_get",
    "tickets_project_settings_set",
    "tickets_projects",
    "tickets_relations",
    "tickets_fields",
    "tickets_field_put",
    "tickets_field_retire",
    "tickets_create",
    "tickets_update",
    "tickets_delete",
    "tickets_comments",
    "tickets_comment_add",
    "tickets_comment_update",
    "tickets_comment_delete",
    "tickets_board_create",
    "tickets_board_update",
    "tickets_board_delete",
    "tickets_board_configure_columns",
    "tickets_board_move_card",
    "tickets_relation_set",
    "tickets_relation_remove",
    "tickets_relations",
    "tickets_get",
    "tickets_list",
    "tickets_board_get",
    "tickets_board_list",
    "tickets_history",
    // 660: deferred store-backed families promoted server-side.
    "workgraph_changes",
    "workgraph_fact_put",
    "import_submit_batch",
    "import_execute_batch",
    "redmine_import_snapshot",
    // lifecycles (store-backed); `lifecycles_active_set`/`lifecycles_active_clear` stay host-local
    // (in-process active-lifecycle selection) and are NOT promoted.
    "lifecycles_define",
    "lifecycles_define_standard",
    "lifecycles_definitions",
    "lifecycles_definition",
    "lifecycles_instantiate",
    "lifecycles_instances",
    "lifecycles_instance",
    "lifecycles_snapshot_plan",
    "lifecycles_current_surface",
    "lifecycles_transition",
    "lifecycles_snapshots",
    "lifecycles_snapshot",
    "lifecycles_snapshot_content",
    "lifecycles_operation_log",
];

pub fn server_promoted(name: &str) -> bool {
    SERVER_PROMOTED_TOOLS.contains(&name)
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RemoteToolRoute {
    UnaryForward,
    ServerExecute,
    Reject(String),
}

pub fn remote_tool_route(name: &str) -> RemoteToolRoute {
    remote_tool_route_for(
        name,
        server_promoted(name),
        tool(name).map(ToolSpec::remote_capability),
    )
}

/// The pure routing decision, factored out so both production (`remote_tool_route`) and tests can
/// exercise every branch (including the promoted branch, which has no catalog entry yet).
pub fn remote_tool_route_for(
    name: &str,
    promoted: bool,
    capability: Option<RemoteCapability>,
) -> RemoteToolRoute {
    if promoted {
        return RemoteToolRoute::ServerExecute;
    }
    match capability {
        Some(RemoteCapability::Unary) | None => RemoteToolRoute::UnaryForward,
        Some(RemoteCapability::LocalOnly) => RemoteToolRoute::Reject(format!(
            "MCP tool {name} is not available against a remote Loom store: it has no remote projection and runs only against a local .loom"
        )),
        Some(RemoteCapability::HandleStream) => RemoteToolRoute::Reject(format!(
            "MCP tool {name} uses a handle/stream interface that is not supported against a remote Loom store"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    const IDL: &str = include_str!("../../../idl/loom.idl");
    const SPEC: &str = include_str!("../../../specs/0008-wire-protocols.md");

    /// The remote-capability partition of the tool surface is the source of truth for the remote-MCP
    /// dispatch gate: unary IDL tools forward to `LoomClient`, handle/stream tools are rejected over
    /// remote, and host/composite tools are local-only. This locks the partition counts into code so a
    /// new tool cannot silently change remote coverage.
    #[test]
    fn remote_capability_partitions_the_surface() {
        let mut unary = 0usize;
        let mut handle_stream = 0usize;
        let mut local_only = 0usize;
        for tool in tool_surface() {
            match tool.remote_capability() {
                RemoteCapability::Unary => unary += 1,
                RemoteCapability::HandleStream => handle_stream += 1,
                RemoteCapability::LocalOnly => local_only += 1,
            }
        }
        // Counts are DERIVED from TOOL_SURFACE, not hardcoded: local-only is exactly the tools without an
        // IDL method; handle/stream is exactly the tools whose (interface, method) is in
        // HANDLE_STREAM_METHODS; unary is the remainder. This keeps one source of truth - a new tool or a
        // classification change moves these in lockstep.
        let derived_local_only = tool_surface()
            .iter()
            .filter(|t| t.idl_method.is_none())
            .count();
        let derived_handle_stream = tool_surface()
            .iter()
            .filter(|t| {
                t.idl_method
                    .is_some_and(|m| HANDLE_STREAM_METHODS.contains(&(t.idl_interface, m)))
            })
            .count();
        let derived_unary = tool_surface().len() - derived_local_only - derived_handle_stream;
        assert_eq!(local_only, derived_local_only, "local-only count drift");
        assert_eq!(
            handle_stream, derived_handle_stream,
            "handle/stream count drift"
        );
        assert_eq!(unary, derived_unary, "unary count drift");
        assert_eq!(unary + handle_stream + local_only, tool_surface().len());
        // No tool is gate-rejected as handle/stream: `HANDLE_STREAM_METHODS` is empty, so every IDL-backed
        // tool classifies Unary (forwarded, or rejected precisely inside its own method). The three
        // session/stream SQL tools are Unary: `sql_exec` is wired (per-request SqlSession in the backend),
        // while `sql_query`/`sql_commit` forward at the gate and reject in-method for a contract reason.
        let hs_names: BTreeSet<&str> = tool_surface()
            .iter()
            .filter(|t| matches!(t.remote_capability(), RemoteCapability::HandleStream))
            .map(|t| t.name)
            .collect();
        assert!(
            hs_names.is_empty(),
            "no tool should be gate-rejected as handle/stream; got {hs_names:?}"
        );
        for sql_tool in ["sql_exec", "sql_query", "sql_commit"] {
            assert!(
                matches!(
                    tool(sql_tool).unwrap().remote_capability(),
                    RemoteCapability::Unary
                ),
                "{sql_tool} should classify Unary (forwarded or method-rejected)"
            );
        }
        // Every local-only tool genuinely lacks an IDL method; every remote-capable tool has one.
        for tool in tool_surface() {
            match tool.remote_capability() {
                RemoteCapability::LocalOnly => {
                    assert!(tool.idl_method.is_none(), "{} misclassified", tool.name)
                }
                _ => assert!(tool.idl_method.is_some(), "{} misclassified", tool.name),
            }
        }
    }

    /// The IDL `enum FacetKind` must mirror `loom_core::FacetKind`, so a facet added on one side but
    /// not the other is caught. IDL names are `UPPER_SNAKE`; the Rust tags are lower-kebab.
    #[test]
    fn idl_facet_kinds_match_core() {
        let start = IDL
            .find("enum FacetKind {")
            .expect("IDL has enum FacetKind");
        let body = &IDL[start..];
        let end = body.find('}').expect("FacetKind enum closes");
        let from_idl: BTreeSet<String> = body[..end]
            .lines()
            .map(|l| l.trim().trim_end_matches(',').trim())
            .filter(|t| !t.is_empty() && !t.starts_with("enum ") && !t.starts_with("//"))
            .map(|t| t.to_ascii_lowercase().replace('_', "-"))
            .collect();
        let from_core: BTreeSet<String> = loom_core::FacetKind::ALL
            .iter()
            .map(|f| f.as_str().to_string())
            .collect();
        assert_eq!(
            from_idl, from_core,
            "IDL enum FacetKind drifted from loom_core::FacetKind"
        );
    }

    /// Parse `idl/loom.idl` into interface name -> set of method names. A method is any line inside an
    /// `interface { ... }` block that opens a parameter list `(`; the method name is the last
    /// whitespace-separated token before that `(`. Struct/enum blocks have no `(` lines, so they
    /// contribute nothing.
    fn idl_interfaces() -> BTreeMap<String, BTreeSet<String>> {
        let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut current: Option<String> = None;
        for raw in IDL.lines() {
            let line = raw.trim();
            if line.starts_with("//") {
                continue; // comments may contain '(' (e.g. "working tree (making ...")
            }
            if let Some(rest) = line.strip_prefix("interface ") {
                let name = rest.split_whitespace().next().unwrap_or("").to_string();
                current = Some(name.clone());
                out.entry(name).or_default();
                continue;
            }
            if line == "}" {
                current = None;
                continue;
            }
            let Some(iface) = current.as_ref() else {
                continue;
            };
            // A method declaration: a line opening a parameter list, with a return type plus name
            // before the `(`. The method name is the last token of that prefix.
            if !line.contains('(') {
                continue;
            }
            let prefix = line.split('(').next().unwrap_or("");
            let tokens: Vec<&str> = prefix.split_whitespace().collect();
            if tokens.len() >= 2 {
                out.get_mut(iface)
                    .unwrap()
                    .insert(tokens[tokens.len() - 1].to_string());
            }
        }
        out
    }

    /// Extract every backtick-quoted token from a markdown cell.
    fn backtick_tokens(cell: &str) -> Vec<String> {
        cell.split('`')
            .skip(1)
            .step_by(2)
            .map(|s| s.to_string())
            .collect()
    }

    /// Parse the documented "Area | IDL interface | Tools" table into the set of tool names, skipping
    /// rows outside the live tool surface.
    fn spec_tool_names() -> BTreeSet<String> {
        let start = SPEC
            .find("| Area | IDL interface | Tools |")
            .expect("documented tool table exists");
        let mut names = BTreeSet::new();
        let mut seen_rows = false;
        for line in SPEC[start..].lines() {
            let line = line.trim();
            if !line.starts_with('|') {
                if seen_rows {
                    break;
                }
                continue;
            }
            let cols: Vec<&str> = line
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim())
                .collect();
            if cols.len() != 3 || cols[0] == "Area" || cols[0].starts_with("---") {
                continue;
            }
            seen_rows = true;
            for tool in backtick_tokens(cols[2]) {
                names.insert(tool);
            }
        }
        names
    }

    #[test]
    fn surface_has_unique_names_and_valid_areas() {
        let mut seen = BTreeSet::new();
        for spec in TOOL_SURFACE {
            assert!(seen.insert(spec.name), "duplicate tool name {}", spec.name);
            if spec.name == "search" {
                assert_eq!(spec.area, "search");
                continue;
            }
            let (area, _verb) = spec.name.split_once('_').expect("tool name is area_verb");
            assert_eq!(area, spec.area, "tool {} area mismatch", spec.name);
        }
    }

    /// Drift: the catalog and documented table list exactly the same tool names.
    #[test]
    fn surface_matches_documented_tool_table() {
        let from_source: BTreeSet<String> =
            TOOL_SURFACE.iter().map(|t| t.name.to_string()).collect();
        let from_spec = spec_tool_names();
        assert_eq!(
            from_source, from_spec,
            "TOOL_SURFACE and documented tool table have drifted; update both together"
        );
    }

    /// Coverage + drift against the IDL: every tool maps to a real method, and every method of a
    /// projected interface is either projected or explicitly excluded.
    #[test]
    fn surface_covers_projected_idl_interfaces() {
        let idl = idl_interfaces();
        let excluded: BTreeMap<&str, BTreeSet<&str>> = EXCLUDED
            .iter()
            .map(|(iface, methods)| (*iface, methods.iter().copied().collect()))
            .collect();

        // Every tool's interface and (concrete) method must exist in the IDL.
        for spec in TOOL_SURFACE {
            let methods = idl
                .get(spec.idl_interface)
                .unwrap_or_else(|| panic!("tool {} names unknown interface", spec.name));
            if let Some(method) = spec.idl_method {
                assert!(
                    methods.contains(method),
                    "tool {} projects {}.{}, absent from the IDL",
                    spec.name,
                    spec.idl_interface,
                    method
                );
            }
        }

        // Per projected interface: idl methods minus excluded == the projected (concrete) methods.
        let mut projected_ifaces = BTreeSet::new();
        for spec in TOOL_SURFACE {
            projected_ifaces.insert(spec.idl_interface);
        }
        for iface in projected_ifaces {
            let idl_methods = &idl[iface];
            let projected: BTreeSet<&str> = TOOL_SURFACE
                .iter()
                .filter(|t| t.idl_interface == iface)
                .filter_map(|t| t.idl_method)
                .collect();
            let empty = BTreeSet::new();
            let excl = excluded.get(iface).unwrap_or(&empty);
            let expected: BTreeSet<&str> = idl_methods
                .iter()
                .map(String::as_str)
                .filter(|m| !excl.contains(m))
                .collect();
            assert_eq!(
                expected, projected,
                "interface {iface}: IDL methods minus EXCLUDED do not match the projected tools; \
                 a new method must be projected as a tool or named in EXCLUDED"
            );
        }
    }

    /// The fully-folded interfaces really exist in the IDL (so a rename is caught) and have no tools.
    #[test]
    fn fully_folded_interfaces_have_no_tools() {
        let idl = idl_interfaces();
        for iface in FULLY_FOLDED {
            assert!(
                idl.contains_key(*iface),
                "FULLY_FOLDED names unknown interface {iface}"
            );
            assert!(
                !TOOL_SURFACE.iter().any(|t| t.idl_interface == *iface),
                "interface {iface} is marked fully folded but has a tool"
            );
        }
    }

    #[test]
    fn read_and_write_partition_the_surface() {
        let r = read_tools().count();
        let w = write_tools().count();
        assert_eq!(r + w, TOOL_SURFACE.len());
        assert!(r > 0 && w > 0);
        // Spot-check the classification on representative tools.
        assert_eq!(tool("sql_query").unwrap().kind, ToolKind::Read);
        assert_eq!(tool("sql_exec").unwrap().kind, ToolKind::Write);
        assert_eq!(tool("queue_consumer_read").unwrap().kind, ToolKind::Read);
        assert_eq!(
            tool("queue_consumer_advance").unwrap().kind,
            ToolKind::Write
        );
    }
}
