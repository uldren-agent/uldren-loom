//! Curated, human-authored titles for the MCP tool surface.
//!
//! Separate lookup map (not a field on `ToolSpec`). Keyed by the underscore
//! canonical tool name; `tool_title` normalizes `.` to `_` on lookup so it works
//! whether the caller passes the dotted or underscored form.
//!
//! Licensed under BUSL-1.1.

/// Human-authored title for a tool, or `None` if the name is unknown.
pub fn tool_title(name: &str) -> Option<String> {
    let key = name.replace('.', "_");
    if let Some((_, title)) = TITLE_REPLACEMENTS.iter().find(|(n, _)| *n == key) {
        return Some((*title).to_string());
    }
    TOOL_TITLES
        .iter()
        .find(|(n, _)| *n == key)
        .map(|(_, title)| sentence_case_action(title))
}

fn sentence_case_action(title: &str) -> String {
    let Some((area, action)) = title.split_once(": ") else {
        return title.to_string();
    };
    let mut chars = action.chars();
    let Some(first) = chars.next() else {
        return title.to_string();
    };
    format!("{area}: {}{}", first.to_uppercase(), chars.as_str())
}

const TITLE_REPLACEMENTS: &[(&str, &str)] = &[
    ("store_version", "Store: Get engine version"),
    (
        "store_capabilities",
        "Store: Inspect supported capabilities",
    ),
    (
        "store_capabilities_json",
        "Store: Inspect capability matrix JSON",
    ),
    ("store_blob_digest", "Store: Calculate a content digest"),
    (
        "store_maintenance_status",
        "Store: Inspect maintenance status",
    ),
    (
        "store_maintenance_policy_set",
        "Store: Update maintenance policy",
    ),
    ("store_maintenance_run", "Store: Run maintenance"),
    ("vcs_log", "VCS: Read commit history"),
    ("vcs_merge_in_progress", "VCS: Inspect merge status"),
    ("vcs_status", "VCS: Inspect working tree status"),
    ("fs_stat", "FS: Inspect file metadata"),
    ("apps_show", "Apps: Inspect app details"),
    ("ask_questions", "Ask: Ask the user questions"),
    ("ask_answers", "Ask: Get user answers"),
    ("cas_put", "CAS: Save content by digest"),
    ("cas_get", "CAS: Get content by digest"),
    ("cas_has", "CAS: Inspect content availability"),
    ("cas_delete", "CAS: Delete content by digest"),
    ("cas_list", "CAS: List stored content"),
    ("graph_upsert_node", "Graph: Save a node"),
    ("graph_upsert_edge", "Graph: Save an edge"),
    ("graph_remove_node", "Graph: Delete a node"),
    ("graph_remove_edge", "Graph: Delete an edge"),
    ("graph_reachable", "Graph: Find reachable nodes"),
    ("graph_shortest_path", "Graph: Find the shortest path"),
    ("graph_query", "Graph: Query nodes and edges"),
    ("graph_explain_query", "Graph: Explain a graph query"),
    ("vector_upsert", "Vector: Save a vector"),
    ("vector_upsert_source", "Vector: Index source text"),
    ("vector_source_text", "Vector: Read source text"),
    (
        "vector_embedding_model",
        "Vector: Inspect the embedding model",
    ),
    (
        "vector_metadata_index_keys",
        "Vector: List metadata index keys",
    ),
    ("vector_search", "Vector: Search by similarity"),
    ("vector_search_policy", "Vector: Update search policy"),
    ("columnar_append", "Columnar: Append a row"),
    ("columnar_scan", "Columnar: Read dataset rows"),
    (
        "columnar_source_digest",
        "Columnar: Get dataset source digest",
    ),
    ("dataframe_collect", "DataFrame: Materialize query results"),
    ("dataframe_preview", "DataFrame: Preview query results"),
    ("dataframe_plan_digest", "DataFrame: Get query plan digest"),
    ("dataframe_source_digests", "DataFrame: List source digests"),
    ("fts_index", "FTS: Index a document"),
    ("fts_remap", "FTS: Update indexed-field mapping"),
    ("fts_query", "FTS: Search a collection"),
    ("fts_source_digest", "FTS: Get source digest"),
    ("fts_status", "FTS: Inspect index status"),
    ("metrics_put_descriptor", "Metrics: Save a descriptor"),
    ("metrics_get_descriptor", "Metrics: Get a descriptor"),
    ("metrics_put_observation", "Metrics: Record an observation"),
    ("logs_put_record", "Logs: Save a record"),
    ("logs_get_record", "Logs: Get a record"),
    ("traces_put_span", "Traces: Save a span"),
    ("traces_get_span", "Traces: Get a span"),
    ("traces_trace_spans", "Traces: List spans in a trace"),
    ("search", "Search: Search readable collections"),
    ("workgraph_metrics", "Workgraph: Generate bounded metrics"),
    ("workgraph_fact_put", "Workgraph: Append a lifecycle fact"),
    ("substrate_history", "Substrate: List change history"),
    (
        "substrate_revision_latest",
        "Substrate: Get the latest revision",
    ),
    ("substrate_revision_at", "Substrate: Get a revision"),
    (
        "substrate_revision_as_of_root",
        "Substrate: Get a revision at a root",
    ),
    (
        "substrate_checkpoint_before",
        "Substrate: Get the prior checkpoint",
    ),
    (
        "substrate_transact",
        "Substrate: Apply an atomic transaction",
    ),
    ("substrate_view_get", "Substrate: Get a view"),
    (
        "substrate_write_admission_policy_get",
        "Substrate: Get write admission policy",
    ),
    (
        "substrate_write_admission_policy_set",
        "Substrate: Update write admission policy",
    ),
    (
        "tickets_project_settings_get",
        "Tickets: Get project settings",
    ),
    (
        "tickets_project_settings_set",
        "Tickets: Update project settings",
    ),
    ("tickets_relation_set", "Tickets: Update a ticket relation"),
    (
        "tickets_relation_remove",
        "Tickets: Delete a ticket relation",
    ),
    ("tickets_relations", "Tickets: List ticket relations"),
    ("tickets_comments", "Tickets: List ticket comments"),
    ("tickets_comment_add", "Tickets: Add ticket comment"),
    ("tickets_comment_update", "Tickets: Update ticket comment"),
    ("tickets_comment_delete", "Tickets: Delete ticket comment"),
    ("tickets_history", "Tickets: List ticket history"),
    ("lanes_create", "Lanes: Create a lane"),
    ("lanes_get", "Lanes: Get a lane"),
    ("lanes_list", "Lanes: List lanes"),
    ("pages_update", "Pages: Update a page draft"),
    ("pages_history", "Pages: List page history"),
    ("chat_fetch_events", "Chat: Get events"),
    ("chat_cursor", "Chat: Get the read cursor"),
    ("chat_presence", "Chat: Get presence"),
    ("chat_post_message", "Chat: Send a message"),
    ("chat_create_thread", "Chat: Create a thread"),
    ("chat_invoke_agent", "Chat: Invite an agent participant"),
    ("chat_agent_reply", "Chat: Link an agent reply"),
    (
        "chat_request_handoff",
        "Chat: Request a participant handoff",
    ),
    ("drive_stat", "Drive: Inspect entry metadata"),
    ("drive_create_upload", "Drive: Create a file upload"),
    ("drive_upload_chunk", "Drive: Upload a file chunk"),
    ("drive_commit_upload", "Drive: Complete a file upload"),
    ("drive_apply_share_expiry", "Drive: Apply share expiry"),
    ("drive_apply_retention", "Drive: Apply a retention policy"),
    (
        "meetings_projection_outputs",
        "Meetings: Generate projection outputs",
    ),
    (
        "meetings_extraction_review",
        "Meetings: Generate extraction review",
    ),
    ("meetings_add_entity_merge", "Meetings: Add an entity merge"),
    ("meetings_add_promotion", "Meetings: Add a promotion"),
    ("meetings_import_snapshot", "Meetings: Import a snapshot"),
    ("redmine_import_snapshot", "Redmine: Import a snapshot"),
    ("studio_reindex", "Studio: Reindex projections"),
    ("import_submit_batch", "Import: Submit a batch"),
    ("import_execute_batch", "Import: Execute a batch"),
    ("structures_bind", "Structures: Bind a structure"),
    (
        "structures_decompose_to_tickets",
        "Structures: Decompose into tickets",
    ),
    ("kv_put", "KV: Save a key value"),
    ("kv_get", "KV: Get a key value"),
    ("kv_range", "KV: Read a key range"),
    ("document_put_text", "Document: Save text content"),
    ("document_get_text", "Document: Read text content"),
    ("document_put_binary", "Document: Save binary content"),
    ("document_get_binary", "Document: Read binary content"),
    ("document_query", "Document: Search documents"),
    ("document_list_binary", "Document: List binary collections"),
    ("timeseries_range", "TimeSeries: Read a time range"),
    ("ledger_head", "Ledger: Get the latest entry"),
    ("ledger_len", "Ledger: Count entries"),
    ("ledger_verify", "Ledger: Verify the chain"),
    ("queue_range", "Queue: Read a message range"),
    ("queue_len", "Queue: Count messages"),
    ("queue_consumer_position", "Queue: Get consumer position"),
    ("queue_consumer_read", "Queue: Read as a consumer"),
    ("calendar_put_entry", "Calendar: Save an event"),
    ("contacts_put_entry", "Contacts: Save a contact"),
    ("mail_ingest_message", "Mail: Save a message"),
    ("mail_get_flags", "Mail: Get message flags"),
    ("mail_set_flags", "Mail: Update message flags"),
    ("sql_exec", "SQL: Execute SQL statements"),
    ("sql_query", "SQL: Run a read-only query"),
    ("sql_read_table_at", "SQL: Read a table at a commit"),
    ("sql_index_scan", "SQL: Read a secondary index"),
    ("sql_index_scan_at", "SQL: Read an index at a commit"),
    ("sql_table_diff", "SQL: Compare table versions"),
    ("sql_blame", "SQL: Inspect table-row history"),
];

/// `(canonical_underscore_name, title)`, ordered by area to match the tool surface.
pub const TOOL_TITLES: &[(&str, &str)] = &[
    ("store_version", "Store: engine version"),
    ("store_capabilities", "Store: supported capabilities"),
    ("store_capabilities_json", "Store: capability matrix JSON"),
    ("store_blob_digest", "Store: digest a blob"),
    (
        "lifecycles_active_clear",
        "Lifecycle: clear the active lifecycle",
    ),
    (
        "lifecycles_active_set",
        "Lifecycle: set the active lifecycle",
    ),
    (
        "lifecycles_current_surface",
        "Lifecycle: get the current tool surface",
    ),
    (
        "lifecycles_define",
        "Lifecycle: create a lifecycle definition",
    ),
    (
        "lifecycles_define_standard",
        "Lifecycle: create a standard lifecycle",
    ),
    (
        "lifecycles_definition",
        "Lifecycle: get a lifecycle definition",
    ),
    (
        "lifecycles_definitions",
        "Lifecycle: list lifecycle definitions",
    ),
    (
        "lifecycles_instantiate",
        "Lifecycle: create a lifecycle instance",
    ),
    ("lifecycles_instance", "Lifecycle: get a lifecycle instance"),
    (
        "lifecycles_instances",
        "Lifecycle: list lifecycle instances",
    ),
    (
        "lifecycles_operation_log",
        "Lifecycle: list lifecycle operations",
    ),
    ("lifecycles_snapshot", "Lifecycle: get a lifecycle snapshot"),
    (
        "lifecycles_snapshot_content",
        "Lifecycle: read lifecycle snapshot content",
    ),
    (
        "lifecycles_snapshot_plan",
        "Lifecycle: inspect lifecycle snapshot plan",
    ),
    (
        "lifecycles_snapshots",
        "Lifecycle: list lifecycle snapshots",
    ),
    (
        "lifecycles_transition",
        "Lifecycle: apply a lifecycle transition",
    ),
    (
        "meetings_promote_artifact_to_reference_artifact",
        "Meetings: promote an artifact to a reference artifact",
    ),
    (
        "meetings_promote_decision_to_decision_log",
        "Meetings: promote a decision to the decision log",
    ),
    (
        "meetings_promote_question_to_lifecycle",
        "Meetings: promote a question to a lifecycle",
    ),
    (
        "meetings_promote_reference_to_reference_artifact",
        "Meetings: promote a reference to a reference artifact",
    ),
    (
        "substrate_reference_reconcile",
        "Substrate: reconcile unresolved references",
    ),
    (
        "substrate_reference_status",
        "Substrate: inspect reference reconciliation",
    ),
    (
        "tickets_project_rekey",
        "Tickets: update a project key prefix",
    ),
    ("workspace_list", "Workspace: list workspaces"),
    ("vcs_commit", "VCS: commit the working tree"),
    ("vcs_branch", "VCS: create a branch"),
    ("vcs_checkout", "VCS: switch branch"),
    ("vcs_log", "VCS: view commit history"),
    ("vcs_merge", "VCS: merge a branch"),
    (
        "vcs_merge_in_progress",
        "VCS: check for an in-progress merge",
    ),
    ("vcs_merge_conflicts", "VCS: list merge conflicts"),
    ("vcs_merge_resolve", "VCS: resolve a conflicted path"),
    ("vcs_merge_abort", "VCS: abort the in-progress merge"),
    ("vcs_merge_continue", "VCS: complete the merge commit"),
    ("vcs_status", "VCS: working tree status"),
    ("vcs_stage", "VCS: stage a path"),
    ("vcs_stage_all", "VCS: stage all changes"),
    ("vcs_unstage", "VCS: unstage a path"),
    ("vcs_commit_staged", "VCS: commit the staged index"),
    ("vcs_tag_create", "VCS: create a tag"),
    ("vcs_tag_list", "VCS: list tags"),
    ("vcs_tag_target", "VCS: resolve a tag's target"),
    ("vcs_tag_delete", "VCS: delete a tag"),
    ("vcs_tag_rename", "VCS: rename a tag"),
    ("vcs_restore_file", "VCS: restore a file to a revision"),
    ("vcs_restore_path", "VCS: restore a subtree to a revision"),
    ("vcs_cherry_pick", "VCS: cherry-pick commits"),
    ("vcs_revert", "VCS: revert commits"),
    ("vcs_rebase", "VCS: rebase onto a target"),
    ("vcs_squash", "VCS: squash commits"),
    ("vcs_diff", "VCS: diff changes"),
    ("vcs_blame", "VCS: blame a file"),
    ("watch_subscribe", "Watch: subscribe to changes"),
    ("watch_poll", "Watch: poll for changes"),
    ("fs_write_file", "FS: write a file"),
    ("fs_read_file", "FS: read a file"),
    ("fs_append_file", "FS: append to a file"),
    ("fs_remove_file", "FS: remove a file"),
    ("fs_read_at", "FS: read at an offset"),
    ("fs_stat", "FS: read metadata"),
    ("fs_list_directory", "FS: list a directory"),
    ("fs_create_directory", "FS: create a directory"),
    ("fs_remove_directory", "FS: remove a directory"),
    ("fs_write_at", "FS: write at an offset"),
    ("fs_truncate", "FS: resize a file"),
    ("fs_symlink", "FS: create a symlink"),
    ("fs_read_link", "FS: read a symlink target"),
    ("vcs_head_branch", "VCS: current branch"),
    ("apps_list", "Apps: list installed apps"),
    ("apps_show", "Apps: show app details"),
    ("apps_read_file", "Apps: read an app file"),
    ("apps_create", "Apps: create an app"),
    ("apps_write_file", "Apps: write an app file"),
    ("apps_remove_file", "Apps: remove an app file"),
    ("apps_call_tool", "Apps: call a visible tool"),
    ("ask_questions", "Ask: pose questions to the user"),
    ("ask_answers", "Ask: retrieve user answers"),
    ("ask_record", "Ask: record a response"),
    ("cas_put", "CAS: store a blob"),
    ("cas_get", "CAS: fetch a blob"),
    ("cas_has", "CAS: check a blob exists"),
    ("cas_delete", "CAS: delete a blob"),
    ("cas_list", "CAS: list blobs"),
    ("graph_upsert_node", "Graph: upsert a node"),
    ("graph_get_node", "Graph: get a node"),
    ("graph_remove_node", "Graph: remove a node"),
    ("graph_upsert_edge", "Graph: upsert an edge"),
    ("graph_get_edge", "Graph: get an edge"),
    ("graph_remove_edge", "Graph: remove an edge"),
    ("graph_neighbors", "Graph: list a node's neighbors"),
    ("graph_out_edges", "Graph: list outgoing edges"),
    ("graph_in_edges", "Graph: list incoming edges"),
    ("graph_reachable", "Graph: find reachable nodes"),
    ("graph_shortest_path", "Graph: shortest path between nodes"),
    ("graph_query", "Graph: run a bounded query"),
    ("graph_explain_query", "Graph: explain a bounded query"),
    ("vector_create", "Vector: create a vector set"),
    ("vector_upsert", "Vector: upsert a vector"),
    (
        "vector_upsert_source",
        "Vector: upsert a vector from source text",
    ),
    ("vector_get", "Vector: get a vector"),
    ("vector_source_text", "Vector: get a vector's source text"),
    ("vector_embedding_model", "Vector: get the embedding model"),
    ("vector_ids", "Vector: list vector IDs"),
    (
        "vector_metadata_index_keys",
        "Vector: list metadata index keys",
    ),
    (
        "vector_create_metadata_index",
        "Vector: create a metadata index",
    ),
    (
        "vector_drop_metadata_index",
        "Vector: drop a metadata index",
    ),
    ("vector_delete", "Vector: delete a vector"),
    ("vector_search", "Vector: search by similarity"),
    ("vector_search_policy", "Vector: configure search policy"),
    ("columnar_create", "Columnar: create a dataset"),
    ("columnar_append", "Columnar: append a row"),
    ("columnar_compact", "Columnar: compact segments"),
    ("columnar_scan", "Columnar: scan rows"),
    ("columnar_columns", "Columnar: list columns"),
    ("columnar_rows", "Columnar: read rows"),
    ("columnar_inspect", "Columnar: inspect a dataset"),
    ("columnar_source_digest", "Columnar: dataset source digest"),
    ("columnar_select", "Columnar: select columns"),
    ("columnar_aggregate", "Columnar: aggregate rows"),
    ("dataframe_create", "DataFrame: create a frame"),
    ("dataframe_collect", "DataFrame: collect results"),
    ("dataframe_preview", "DataFrame: preview rows"),
    ("dataframe_materialize", "DataFrame: materialize a frame"),
    ("dataframe_plan_digest", "DataFrame: query plan digest"),
    ("dataframe_source_digests", "DataFrame: source digests"),
    ("fts_create", "FTS: create a collection"),
    ("fts_index", "FTS: index a document"),
    ("fts_get", "FTS: get a document"),
    ("fts_delete", "FTS: delete a document"),
    ("fts_ids", "FTS: list document IDs"),
    ("fts_remap", "FTS: replace the field mapping"),
    ("fts_query", "FTS: run a collection query"),
    ("fts_source_digest", "FTS: source digest"),
    ("fts_status", "FTS: index status"),
    ("metrics_put_descriptor", "Metrics: store a descriptor"),
    ("metrics_get_descriptor", "Metrics: fetch a descriptor"),
    ("metrics_put_observation", "Metrics: store an observation"),
    ("metrics_query", "Metrics: query observations"),
    ("logs_put_record", "Logs: store a record"),
    ("logs_get_record", "Logs: fetch a record"),
    ("logs_query", "Logs: query records"),
    ("traces_put_span", "Traces: store a span"),
    ("traces_get_span", "Traces: fetch a span"),
    ("traces_trace_spans", "Traces: query trace spans"),
    ("traces_query", "Traces: query spans"),
    ("search", "Search: search across readable collections"),
    ("substrate_changes", "Substrate: list changes"),
    ("workgraph_changes", "Workgraph: list lifecycle changes"),
    ("workgraph_metrics", "Workgraph: derive bounded metrics"),
    ("workgraph_fact_put", "Workgraph: append lifecycle fact"),
    ("substrate_refs", "Substrate: list references"),
    ("substrate_alias_bind", "Substrate: bind an alias"),
    ("substrate_alias_release", "Substrate: release an alias"),
    ("substrate_alias_resolve", "Substrate: resolve an alias"),
    ("substrate_alias_list", "Substrate: list aliases"),
    ("substrate_history", "Substrate: view change history"),
    (
        "substrate_revision_latest",
        "Substrate: view latest revision",
    ),
    ("substrate_revision_at", "Substrate: view revision"),
    (
        "substrate_revision_as_of_root",
        "Substrate: view revision at root",
    ),
    (
        "substrate_checkpoint_before",
        "Substrate: view checkpoint before revision",
    ),
    ("substrate_transact", "Substrate: run a transaction"),
    ("substrate_view_define", "Substrate: define a view"),
    ("substrate_view_get", "Substrate: get a view"),
    ("substrate_view_list", "Substrate: list views"),
    (
        "substrate_write_admission_policy_get",
        "Substrate: get the write admission policy",
    ),
    (
        "substrate_write_admission_policy_set",
        "Substrate: set the write admission policy",
    ),
    ("tickets_project_create", "Tickets: create a project"),
    (
        "tickets_project_settings_get",
        "Tickets: read project settings",
    ),
    (
        "tickets_project_settings_set",
        "Tickets: update project settings",
    ),
    ("tickets_projects", "Tickets: list projects"),
    ("tickets_relations", "Tickets: list relations"),
    ("tickets_fields", "Tickets: discover fields"),
    ("tickets_field_put", "Tickets: save custom field"),
    ("tickets_field_retire", "Tickets: retire custom field"),
    ("tickets_create", "Tickets: create a ticket"),
    ("tickets_update", "Tickets: update ticket"),
    ("tickets_delete", "Tickets: delete ticket"),
    ("tickets_comments", "Tickets: list comments"),
    ("tickets_comment_add", "Tickets: add comment"),
    ("tickets_comment_update", "Tickets: update comment"),
    ("tickets_comment_delete", "Tickets: delete comment"),
    ("tickets_board_create", "Tickets: create a board"),
    ("tickets_board_update", "Tickets: update a board"),
    ("tickets_board_delete", "Tickets: delete a board"),
    (
        "tickets_board_configure_columns",
        "Tickets: configure board columns",
    ),
    ("tickets_board_move_card", "Tickets: move a board card"),
    ("tickets_relation_set", "Tickets: set ticket relation"),
    ("tickets_relation_remove", "Tickets: remove ticket relation"),
    ("tickets_get", "Tickets: get a ticket"),
    ("tickets_list", "Tickets: list tickets"),
    ("tickets_board_get", "Tickets: get a board"),
    ("tickets_board_list", "Tickets: list boards"),
    ("tickets_history", "Tickets: view ticket history"),
    ("lanes_create", "Lanes: create a Lane"),
    ("lanes_get", "Lanes: get a Lane"),
    ("lanes_list", "Lanes: list Lanes"),
    ("lanes_update", "Lanes: update a Lane"),
    ("lanes_ticket_add", "Lanes: add ticket"),
    ("lanes_ticket_remove", "Lanes: remove ticket"),
    ("lanes_ticket_transfer", "Lanes: transfer ticket"),
    ("lanes_delete", "Lanes: delete closed Lane"),
    ("spaces_create", "Spaces: create a space"),
    ("spaces_get", "Spaces: get a space"),
    ("spaces_list", "Spaces: list spaces"),
    ("pages_create", "Pages: create a page"),
    ("pages_update", "Pages: update a page draft"),
    ("pages_publish", "Pages: publish a page"),
    ("pages_get", "Pages: get a page"),
    ("pages_list", "Pages: list pages"),
    ("pages_history", "Pages: view page history"),
    ("chat_channels", "Chat: list channels"),
    ("chat_fetch_events", "Chat: fetch events"),
    ("chat_messages", "Chat: list messages"),
    ("chat_cursor", "Chat: get the read cursor"),
    ("chat_presence", "Chat: get presence"),
    ("chat_create_channel", "Chat: create a channel"),
    ("chat_rename_channel", "Chat: rename a channel"),
    ("chat_post_message", "Chat: post a message"),
    ("chat_edit_message", "Chat: edit a message"),
    ("chat_redact_message", "Chat: redact a message"),
    ("chat_emoji_list", "Chat: list custom emoji"),
    ("chat_emoji_register", "Chat: register custom emoji"),
    ("chat_emoji_unregister", "Chat: unregister custom emoji"),
    ("chat_add_reaction", "Chat: add a reaction"),
    ("chat_remove_reaction", "Chat: remove a reaction"),
    ("chat_create_thread", "Chat: start a thread"),
    ("chat_create_task", "Chat: create a task"),
    ("chat_claim_task", "Chat: claim a task"),
    ("chat_complete_task", "Chat: complete a task"),
    ("chat_invoke_agent", "Chat: invite an agent participant"),
    ("chat_agent_reply", "Chat: link an agent participant reply"),
    ("chat_request_handoff", "Chat: request participant handoff"),
    ("chat_update_cursor", "Chat: update the read cursor"),
    ("chat_set_presence", "Chat: set presence"),
    ("drive_list", "Drive: list entries"),
    ("drive_stat", "Drive: stat an entry"),
    ("drive_read", "Drive: read a file"),
    ("drive_list_versions", "Drive: list file versions"),
    ("drive_list_conflicts", "Drive: list conflicts"),
    ("drive_create_folder", "Drive: create a folder"),
    ("drive_create_upload", "Drive: start an upload"),
    ("drive_upload_chunk", "Drive: upload a chunk"),
    ("drive_commit_upload", "Drive: finish an upload"),
    ("drive_rename", "Drive: rename an entry"),
    ("drive_move", "Drive: move an entry"),
    ("drive_delete", "Drive: delete an entry"),
    ("drive_resolve_conflict", "Drive: resolve a conflict"),
    ("drive_list_shares", "Drive: list shares"),
    ("drive_grant_share", "Drive: grant a share"),
    ("drive_revoke_share", "Drive: revoke a share"),
    ("drive_apply_share_expiry", "Drive: apply share expiry"),
    ("drive_list_retention", "Drive: list retention holds"),
    ("drive_pin_retention", "Drive: pin a retention hold"),
    ("drive_unpin_retention", "Drive: remove a retention hold"),
    ("drive_apply_retention", "Drive: apply a retention policy"),
    ("drive_acquire_lease", "Drive: acquire a lease"),
    ("drive_refresh_lease", "Drive: refresh a lease"),
    ("drive_release_lease", "Drive: release a lease"),
    ("drive_break_lease", "Drive: break a lease"),
    ("meetings_list", "Meetings: list records"),
    ("meetings_get", "Meetings: get a record"),
    ("meetings_search", "Meetings: search records"),
    (
        "meetings_projection_outputs",
        "Meetings: derive projection outputs",
    ),
    (
        "meetings_extraction_review",
        "Meetings: derive extraction review",
    ),
    (
        "meetings_accept_annotation",
        "Meetings: accept an annotation",
    ),
    (
        "meetings_reject_annotation",
        "Meetings: reject an annotation",
    ),
    (
        "meetings_propose_vocabulary",
        "Meetings: propose a vocabulary term",
    ),
    (
        "meetings_accept_vocabulary",
        "Meetings: accept a vocabulary term",
    ),
    (
        "meetings_reject_vocabulary",
        "Meetings: reject a vocabulary term",
    ),
    ("meetings_add_entity_merge", "Meetings: add an entity merge"),
    ("meetings_add_promotion", "Meetings: add a promotion"),
    (
        "meetings_promote_task_to_ticket",
        "Meetings: promote a task to a ticket",
    ),
    ("meetings_import_snapshot", "Meetings: import snapshot"),
    ("redmine_import_snapshot", "Redmine: import snapshot"),
    ("studio_reindex", "Studio: reindex projections"),
    ("import_submit_batch", "Import: submit batch"),
    ("import_execute_batch", "Import: execute batch"),
    ("structures_create", "Structures: create a structure"),
    ("structures_get", "Structures: get a structure"),
    ("structures_list", "Structures: list structures"),
    ("structures_add_node", "Structures: add a node"),
    ("structures_update_node", "Structures: update a node"),
    ("structures_move_node", "Structures: move a node"),
    ("structures_link_node", "Structures: link a node"),
    ("structures_bind", "Structures: bind a structure"),
    (
        "structures_decompose_to_tickets",
        "Structures: decompose into tickets",
    ),
    ("kv_put", "KV: set a key"),
    ("kv_get", "KV: get a key"),
    ("kv_delete", "KV: delete a key"),
    ("kv_list", "KV: list keys"),
    ("kv_range", "KV: range over keys"),
    ("kv_list_collections", "KV: list collections"),
    ("document_put_text", "Document: upsert text"),
    ("document_get_text", "Document: get text"),
    ("document_put_binary", "Document: upsert binary"),
    ("document_get_binary", "Document: get binary"),
    ("document_query", "Document: query documents"),
    (
        "document_replace_text",
        "Document: replace text in a document",
    ),
    ("document_delete", "Document: delete a document"),
    ("document_list_binary", "Document: list binary collection"),
    ("document_list_collections", "Document: list collections"),
    ("timeseries_put", "TimeSeries: record a point"),
    ("timeseries_get", "TimeSeries: get a point"),
    ("timeseries_range", "TimeSeries: read a time range"),
    ("timeseries_latest", "TimeSeries: get the latest point"),
    (
        "timeseries_list_collections",
        "TimeSeries: list collections",
    ),
    ("ledger_append", "Ledger: append an entry"),
    ("ledger_get", "Ledger: get an entry"),
    ("ledger_head", "Ledger: get the head entry"),
    ("ledger_len", "Ledger: count entries"),
    ("ledger_verify", "Ledger: verify the chain"),
    ("ledger_list_collections", "Ledger: list collections"),
    ("queue_append", "Queue: enqueue a message"),
    ("queue_get", "Queue: get a message"),
    ("queue_range", "Queue: read a range of messages"),
    ("queue_len", "Queue: count messages"),
    ("queue_list_streams", "Queue: list streams"),
    (
        "queue_consumer_position",
        "Queue: get a consumer's position",
    ),
    ("queue_consumer_read", "Queue: read as a consumer"),
    ("queue_consumer_advance", "Queue: advance a consumer"),
    ("queue_consumer_reset", "Queue: reset a consumer"),
    ("calendar_create_collection", "Calendar: create a calendar"),
    ("calendar_get_collection", "Calendar: get a calendar"),
    ("calendar_list_collections", "Calendar: list calendars"),
    ("calendar_delete_collection", "Calendar: delete a calendar"),
    ("calendar_put_entry", "Calendar: add or update an event"),
    ("calendar_put_ics", "Calendar: import iCalendar"),
    ("calendar_get_entry", "Calendar: get an event"),
    ("calendar_delete_entry", "Calendar: delete an event"),
    ("calendar_list_entries", "Calendar: list events"),
    ("calendar_range", "Calendar: list events in a range"),
    ("calendar_search", "Calendar: search events"),
    ("calendar_to_ics", "Calendar: export to iCalendar"),
    ("contacts_create_book", "Contacts: create an address book"),
    ("contacts_get_book", "Contacts: get an address book"),
    ("contacts_list_books", "Contacts: list address books"),
    ("contacts_delete_book", "Contacts: delete an address book"),
    ("contacts_put_entry", "Contacts: add or update a contact"),
    ("contacts_put_vcard", "Contacts: import vCard"),
    ("contacts_get_entry", "Contacts: get a contact"),
    ("contacts_delete_entry", "Contacts: delete a contact"),
    ("contacts_list_entries", "Contacts: list contacts"),
    ("contacts_search", "Contacts: search contacts"),
    ("contacts_to_vcard", "Contacts: export to vCard"),
    ("mail_create_mailbox", "Mail: create a mailbox"),
    ("mail_get_mailbox", "Mail: get a mailbox"),
    ("mail_list_mailboxes", "Mail: list mailboxes"),
    ("mail_delete_mailbox", "Mail: delete a mailbox"),
    ("mail_ingest_message", "Mail: ingest a message"),
    ("mail_get_message", "Mail: get a message"),
    ("mail_to_eml", "Mail: export a message to EML"),
    ("mail_delete_message", "Mail: delete a message"),
    ("mail_list_messages", "Mail: list messages"),
    ("mail_get_flags", "Mail: get message flags"),
    ("mail_set_flags", "Mail: set message flags"),
    ("mail_search", "Mail: search messages"),
    ("sql_exec", "SQL: run statements"),
    ("sql_query", "SQL: run a read-only query"),
    ("sql_commit", "SQL: commit the SQL workspace"),
    ("sql_read_table", "SQL: read a table"),
    ("sql_read_table_at", "SQL: read a table at a commit"),
    ("sql_index_scan", "SQL: scan a secondary index"),
    ("sql_index_scan_at", "SQL: scan an index at a commit"),
    ("sql_diff", "SQL: diff changes"),
    ("sql_table_diff", "SQL: schema-aware table diff"),
    ("sql_blame", "SQL: blame table rows"),
    ("sql_list_databases", "SQL: list databases"),
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{TITLE_REPLACEMENTS, TOOL_TITLES, tool_title};

    #[test]
    fn static_title_entries_match_tool_surface() {
        let surface_names = crate::tools::TOOL_SURFACE
            .iter()
            .map(|spec| spec.name)
            .collect::<BTreeSet<_>>();
        let mut title_names = BTreeSet::new();
        for (name, _) in TOOL_TITLES {
            assert!(title_names.insert(*name), "duplicate title entry {name}");
        }
        let mut replacement_names = BTreeSet::new();
        for (name, _) in TITLE_REPLACEMENTS {
            assert!(
                replacement_names.insert(*name),
                "duplicate replacement title entry {name}"
            );
            title_names.insert(*name);
            assert!(
                surface_names.contains(name),
                "replacement for unknown tool {name}"
            );
        }
        assert_eq!(title_names, surface_names);
    }

    #[test]
    fn static_titles_use_capitalized_action_phrases() {
        for spec in crate::tools::TOOL_SURFACE {
            let title =
                tool_title(spec.name).unwrap_or_else(|| panic!("missing title {}", spec.name));
            let (_, action) = title
                .split_once(": ")
                .unwrap_or_else(|| panic!("missing area separator in {}", spec.name));
            let first = action
                .chars()
                .next()
                .unwrap_or_else(|| panic!("empty action phrase in {}", spec.name));
            assert!(
                first.is_uppercase(),
                "title action must start uppercase for {}: {title}",
                spec.name
            );
        }
    }

    #[test]
    fn approved_semantic_replacements_resolve() {
        let examples = [
            ("cas_get", "CAS: Get content by digest"),
            ("drive_commit_upload", "Drive: Complete a file upload"),
            (
                "lifecycles_current_surface",
                "Lifecycle: Get the current tool surface",
            ),
            ("metrics_put_observation", "Metrics: Record an observation"),
            (
                "substrate_write_admission_policy_set",
                "Substrate: Update write admission policy",
            ),
        ];
        for (name, expected) in examples {
            assert_eq!(tool_title(name).as_deref(), Some(expected), "{name}");
        }
    }
}
