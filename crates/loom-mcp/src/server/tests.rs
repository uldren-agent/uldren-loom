use super::*;
use crate::StoreAccess;
use std::collections::BTreeSet;

#[test]
fn mcp_error_data_includes_structured_loom_details() {
    let error = LoomError::invalid("bad status").with_detail(
        loom_core::error::ErrorDetail::invalid_field("target_status", Some("accepted"), ["ready"]),
    );
    let data = err(error).data.expect("structured error data");
    assert_eq!(data["details"][0]["kind"], "invalid_field");
    assert_eq!(data["details"][0]["field"], "target_status");
    assert_eq!(data["details"][0]["rejected"], "accepted");
    assert_eq!(data["details"][0]["accepted"][0], "ready");
}

fn sample_lifecycle_operation_log(workspace_id: &str) -> loom_core::Result<Vec<u8>> {
    let operation_id = "lifecycle-op-1";
    let payload = b"status lifecycle transition";
    let envelope = loom_substrate::OperationEnvelope::new(
        loom_core::Algo::Blake3,
        loom_substrate::OperationEnvelopeInput {
            workspace_id,
            app_id: "studio.lifecycle",
            scope_id: &loom_substrate::lifecycle::lifecycle_operation_cursor_scope(workspace_id),
            operation_id,
            operation_kind: "lifecycle.transitioned",
            sequence: 1,
            actor_principal: WorkspaceId::v4_from_bytes([81; 16]),
            actor_kind: loom_substrate::ActorKind::User,
            timestamp_ms: 1,
            idempotency_key: operation_id,
            base_root: loom_core::Digest::hash(loom_core::Algo::Blake3, b"base"),
            base_entity_version: None,
            target_entity_id: Some("lifecycle:feature-1"),
            payload,
            policy_labels: &[],
            signature: None,
            agent: None,
        },
    )?;
    let record = loom_substrate::lifecycle::LifecycleOperationRecord::new(
        1,
        operation_id,
        "lifecycle.transitioned",
        "feature-1",
        Some("lifecycle:feature-1".to_string()),
        loom_core::Digest::hash(loom_core::Algo::Blake3, b"root"),
        envelope.encode()?,
    )?;
    loom_substrate::lifecycle::LifecycleOperationLog::new(workspace_id, vec![record])?.encode()
}

/// Every tool in TOOL_SURFACE is registered, and nothing extra is.
#[test]
fn registered_tools_equal_the_surface() {
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        "/nonexistent.loom",
        None,
    ))));
    let registered: BTreeSet<String> = server
        .tool_router
        .list_all()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    let surface: BTreeSet<String> = crate::tools::TOOL_SURFACE
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert_eq!(
        registered, surface,
        "registered rmcp tools must equal TOOL_SURFACE"
    );
}

#[test]
fn store_maintenance_tools_update_status_and_run_tail_compaction() {
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-store-maintenance-{}.loom",
        std::process::id()
    ));
    let loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::persistent(loom))));

    let status = server.store_maintenance_status().unwrap().0;
    assert_eq!(status["value"]["policy"]["tail_compaction_enabled"], true);
    assert!(status["value"]["live_root_diagnostics"]["classes"].is_array());

    let updated = server
        .store_maintenance_policy_set(Parameters(PStoreMaintenancePolicySet {
            tail_compaction_enabled: Some(true),
            tail_compaction_max_pages: Some(4),
            tail_compaction_max_objects: Some(2),
            tail_compaction_max_bytes: Some(16 * 1024),
            ..PStoreMaintenancePolicySet::default()
        }))
        .unwrap()
        .0;
    assert_eq!(updated["value"]["policy"]["tail_compaction_enabled"], true);
    assert!(updated["value"]["live_root_diagnostics"]["classes"].is_array());

    let mut run = server
        .store_maintenance_run(Parameters(PStoreMaintenanceRun {
            manual: true,
            max_segments: Some(1),
            max_pages: Some(8),
        }))
        .unwrap()
        .0;
    if run["value"]["outcome"] == "marked" {
        run = server
            .store_maintenance_run(Parameters(PStoreMaintenanceRun {
                manual: true,
                max_segments: Some(1),
                max_pages: Some(8),
            }))
            .unwrap()
            .0;
    }
    assert!(
        ["marked", "reclaimed"].contains(&run["value"]["outcome"].as_str().unwrap()),
        "unexpected maintenance outcome: {run}"
    );
    let after = server.store_maintenance_status().unwrap().0;
    assert_eq!(
        after["value"]["run_state"]["last_tail_compaction_attempted"],
        true
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn mcp_surface_baseline_is_reproducible() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-surface-baseline-{}.loom",
        std::process::id()
    ));
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    loom.registry_mut()
        .create(
            FacetKind::Vcs,
            Some("repo"),
            WorkspaceId::v4_from_bytes([42u8; 16]),
        )
        .unwrap();
    let mcp = Arc::new(LoomMcp::new(StoreAccess::persistent(loom)));
    let server = LoomServer::new(mcp.clone());
    let read_only = Binding {
        allow_writes: false,
        ..Binding::default()
    };
    let read_only_server = LoomServer::with_binding(mcp, read_only);

    let tools = server
        .listed_model_tools_result()
        .expect("listed tools")
        .tools;
    let read_only_tools = read_only_server
        .listed_model_tools_result()
        .expect("read-only listed tools")
        .tools;
    let resources = server.list_all_resources().expect("resources");
    let resource_templates = server.resource_templates();
    let prompts = server.prompt_router.list_all();
    let app_launchers = server.list_app_launcher_tools().expect("app launchers");

    assert_eq!(tools.len(), 402);
    assert_eq!(
        serde_json::to_vec(&tools).expect("tools json").len(),
        475_538
    );
    assert_eq!(read_only_tools.len(), 214);
    assert_eq!(
        serde_json::to_vec(&read_only_tools)
            .expect("read-only tools json")
            .len(),
        220_228
    );
    assert_eq!(resources.len(), 31);
    assert_eq!(
        serde_json::to_vec(&resources)
            .expect("resources json")
            .len(),
        8_314
    );
    assert_eq!(resource_templates.len(), 8);
    assert_eq!(
        serde_json::to_vec(&resource_templates)
            .expect("resource templates json")
            .len(),
        1_549
    );
    assert_eq!(prompts.len(), 46);
    assert_eq!(
        serde_json::to_vec(&prompts).expect("prompts json").len(),
        6_810
    );
    assert_eq!(app_launchers.len(), 30);
    assert_eq!(
        serde_json::to_vec(&app_launchers)
            .expect("app launchers json")
            .len(),
        28_052
    );
}

#[test]
fn model_tool_list_includes_ticket_tools_without_pagination() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-tool-list-tickets-{}.loom",
        std::process::id()
    ));
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    loom.registry_mut()
        .create(
            FacetKind::Vcs,
            Some("repo"),
            WorkspaceId::v4_from_bytes([24u8; 16]),
        )
        .unwrap();
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::persistent(loom))));
    let result = server
        .listed_model_tools_result()
        .expect("listed tool result");
    assert_eq!(result.next_cursor, None);
    let listed = result.tools;
    assert!(
        listed.len() > LoomServer::PAGE_SIZE,
        "test requires a tool surface larger than one compatibility page"
    );
    let names = listed
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<BTreeSet<_>>();
    for expected in [
        "tickets_create",
        "tickets_update",
        "tickets_comments",
        "tickets_comment_add",
        "tickets_comment_update",
        "tickets_comment_delete",
        "tickets_relation_set",
        "tickets_relation_remove",
        "tickets_get",
        "tickets_history",
    ] {
        assert!(names.contains(expected), "{expected} must be advertised");
    }
    let _ = std::fs::remove_file(&path);
}

fn listed_tool<'a>(tools: &'a [Tool], name: &str) -> &'a Tool {
    tools
        .iter()
        .find(|tool| tool.name.as_ref() == name)
        .unwrap_or_else(|| panic!("{name} must be listed"))
}

fn schema_property<'a>(schema: &'a JsonObject, property: &str) -> &'a Value {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get(property))
        .unwrap_or_else(|| panic!("{property} must be declared"))
}

fn resolve_schema_ref<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
        return schema;
    };
    let Some(def_name) = reference.strip_prefix("#/$defs/") else {
        return schema;
    };
    root.pointer(&format!("/$defs/{def_name}"))
        .unwrap_or(schema)
}

fn nested_property<'a>(root: &'a Value, schema: &'a Value, property: &str) -> &'a Value {
    let schema = resolve_schema_ref(root, schema);
    schema
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get(property))
        .unwrap_or_else(|| panic!("{property} must be declared in {schema}"))
}

fn array_item_schema<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    let schema = resolve_schema_ref(root, schema);
    schema
        .get("items")
        .unwrap_or_else(|| panic!("array items schema must be declared in {schema}"))
}

fn assert_object_schema(schema: &Value, label: &str) {
    assert_eq!(
        schema.get("type").and_then(Value::as_str),
        Some("object"),
        "{label} must be an object schema: {schema}"
    );
    assert_ne!(schema, &json!({}), "{label} must not be an empty schema");
}

#[test]
fn model_tool_structured_input_properties_have_direct_object_schemas() {
    use loom_core::Algo;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-structured-input-schema-{}.loom",
        std::process::id()
    ));
    let loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    let server = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::persistent(loom))),
        Binding {
            allow_writes: true,
            ..Default::default()
        },
    );
    let tools = server
        .listed_model_tools_result()
        .expect("listed tools")
        .tools;

    for &(tool_name, property) in MODEL_TOOL_OBJECT_INPUT_FIELDS {
        let tool = listed_tool(&tools, tool_name);
        let property_schema = schema_property(&tool.input_schema, property);
        assert_object_schema(property_schema, &format!("{tool_name}.{property}"));
    }

    let tickets_update = listed_tool(&tools, "tickets_update");
    let tickets_update_input = Value::Object((*tickets_update.input_schema).clone());
    let comment = schema_property(&tickets_update.input_schema, "comment");
    let comment_evidence = nested_property(&tickets_update_input, comment, "evidence");
    assert_object_schema(comment_evidence, "tickets_update.comment.evidence");

    let comments = schema_property(&tickets_update.input_schema, "comments");
    let comment_item = array_item_schema(&tickets_update_input, comments);
    let comment_item_evidence = nested_property(&tickets_update_input, comment_item, "evidence");
    assert_object_schema(comment_item_evidence, "tickets_update.comments.evidence");

    for tool_name in ["tickets_comment_add", "tickets_comment_update"] {
        let tool = listed_tool(&tools, tool_name);
        let evidence = schema_property(&tool.input_schema, "evidence");
        assert_object_schema(evidence, &format!("{tool_name}.evidence"));
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn high_use_ticket_and_lane_response_schemas_include_structured_fields() {
    let ticket = ticket_schema();
    let ticket_properties = ticket
        .get("properties")
        .and_then(Value::as_object)
        .expect("ticket properties");
    for property in [
        "projection_profile",
        "projection_kind",
        "projection_source",
        "projection_selection_source",
        "relation_rollup",
    ] {
        assert!(
            ticket_properties.contains_key(property),
            "ticket schema must include {property}"
        );
    }
    assert!(
        nested_property(
            &ticket,
            array_item_schema(
                &ticket,
                ticket_properties.get("relations").expect("relations")
            ),
            "target"
        )
        .get("anyOf")
        .is_some(),
        "ticket relations must include nullable target state"
    );

    let comment = ticket_comment_schema();
    let evidence = nested_property(&comment, &comment, "evidence");
    let evidence_object = evidence
        .get("anyOf")
        .and_then(Value::as_array)
        .and_then(|branches| {
            branches
                .iter()
                .find(|branch| branch.get("type").and_then(Value::as_str) == Some("object"))
        })
        .expect("comment evidence object branch");
    assert_eq!(
        evidence_object["additionalProperties"]["items"]["type"],
        json!("string")
    );

    for schema in [lane_view_schema(), lane_compact_view_schema()] {
        assert!(
            schema
                .get("properties")
                .and_then(Value::as_object)
                .is_some_and(|properties| properties.contains_key("status_counts")),
            "lane schemas must include aggregate status_counts"
        );
    }
}

#[test]
fn result_page_defaults_to_balanced_limit() {
    let default_page = PResultPage {
        limit: None,
        offset: None,
    };
    assert_eq!(
        slice_results((0..600).collect::<Vec<_>>(), &default_page)
            .unwrap()
            .len(),
        DEFAULT_RESULT_LIMIT
    );

    let explicit_page = PResultPage {
        limit: Some(2),
        offset: Some(3),
    };
    assert_eq!(
        slice_results((0..10).collect::<Vec<_>>(), &explicit_page).unwrap(),
        vec![3, 4]
    );

    let invalid_page = PResultPage {
        limit: Some(0),
        offset: None,
    };
    let error = slice_results(vec![1], &invalid_page).unwrap_err();
    assert!(error.message.contains("limit"));
}

#[test]
fn delivered_payload_budget_reports_how_to_narrow() {
    assert!(budgeted_out_value("test_tool", json!("small"), Some(100)).is_ok());

    let error = match budgeted_out_value("test_tool", json!("too large"), Some(5)) {
        Ok(_) => panic!("budget must reject oversized response"),
        Err(error) => error,
    };
    assert!(error.message.contains("exceeds delivered payload budget"));
    assert!(error.message.contains("limit/offset"));
}

#[test]
fn fs_list_directory_applies_balanced_page_controls() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-fs-list-page-{}-{}.loom",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos()
    ));
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("repo"),
            WorkspaceId::v4_from_bytes([42u8; 16]),
        )
        .unwrap();
    loom.write_file(ns, "a.txt", b"a", 0o644).unwrap();
    loom.write_file(ns, "b.txt", b"b", 0o644).unwrap();
    loom.write_file(ns, "c.txt", b"c", 0o644).unwrap();

    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::persistent(loom))));
    let output = server
        .fs_list_directory(Parameters(PFsPath {
            workspace: "repo".to_string(),
            path: "/".to_string(),
            page: PResultPage {
                limit: Some(2),
                offset: Some(1),
            },
        }))
        .unwrap()
        .0;
    let bytes: Vec<u8> = serde_json::from_value(output["value"].clone()).unwrap();
    let entries = loom_wire::fs::dir_listing_from_cbor(&bytes).unwrap();
    let names = entries
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();

    assert_eq!(names, vec!["b.txt", "c.txt"]);
    let _ = std::fs::remove_file(&path);
}

/// Advertised wire names are host-compatible (`.` -> `_`) and reverse cleanly to canonical.
#[test]
fn wire_tool_names_are_sanitized_and_reversible() {
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        "/nonexistent.loom",
        None,
    ))));
    for tool in server.tool_router.list_all() {
        let wire = sanitize_tool_name(&tool.name);
        assert!(
            !wire.contains('.'),
            "wire name {wire} must not contain a dot"
        );
        assert!(
            wire.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
            "wire name {wire} must match ^[A-Za-z0-9_-]+$"
        );
        // Sanitization must be reversible against the canonical name set (no collisions).
        assert_eq!(
            server.canonical_tool_name(&wire),
            tool.name.to_string(),
            "round-trip failed for {}",
            tool.name
        );
    }
    // Underscore wire names resolve to the canonical (now underscore-native) name.
    assert_eq!(server.canonical_tool_name("store_version"), "store_version");
    // Legacy dotted callers (stdio / Inspector) still resolve to the canonical name.
    assert_eq!(server.canonical_tool_name("store.version"), "store_version");
}

#[test]
fn columnar_json_predicate_lowers_to_legacy_filter() {
    let predicate = json!({
        "version": 1,
        "expr": {
            "op": "gte",
            "path": ["priority"],
            "value": { "type": "i64", "value": 3 }
        }
    });
    let filter = columnar_select_filter_arg(None, Some(&predicate)).unwrap();
    let decoded = loom_codec::decode(&filter).unwrap();
    let loom_codec::Value::Array(items) = decoded else {
        panic!("filter must be an array");
    };
    assert_eq!(items.len(), 3);
    assert_eq!(items[0], loom_codec::Value::Text("priority".to_string()));
    assert_eq!(items[1], loom_codec::Value::Uint(5));
    assert_eq!(
        loom_core::tabular::cell_from(items[2].clone()).unwrap(),
        loom_core::tabular::Value::Int(3)
    );
}

#[test]
fn columnar_select_rejects_filter_and_predicate_together() {
    let predicate = json!({
        "version": 1,
        "expr": {
            "op": "eq",
            "path": ["status"],
            "value": { "type": "text", "value": "open" }
        }
    });
    let legacy = vec![0x80];
    let err = columnar_select_filter_arg(Some(&legacy), Some(&predicate)).unwrap_err();
    assert_eq!(err.code, Code::InvalidArgument);
}

#[test]
fn chat_presence_is_ephemeral_and_principal_scoped() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!("loom-mcp-presence-{}.loom", std::process::id()));
    let ns_id = WorkspaceId::v4_from_bytes([14u8; 16]);
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    loom.registry_mut()
        .create(FacetKind::Files, Some("repo"), ns_id)
        .unwrap();
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::persistent(loom))));
    server
        .mcp
        .write_chat_create_channel("repo", "studio", "general", "General")
        .unwrap();

    let set = server
        .chat_presence_set("repo", "studio", "general", "typing", 30_000)
        .unwrap();
    assert_eq!(set.status, "typing");
    assert_eq!(set.principal, ns_id.to_string());
    let live = server
        .chat_presence_list("repo", "studio", "general")
        .unwrap();
    assert_eq!(live, vec![set]);
    assert!(
        server
            .chat_presence_set("repo", "studio", "general", "typing", 0)
            .is_err()
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn document_query_range_and_replace_text_are_guarded() {
    use loom_core::document::doc_put;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Digest, Loom};
    use loom_store::FileStore;
    use rmcp::handler::server::wrapper::Parameters;

    let path = std::env::temp_dir().join(format!("loom-mcp-doc-{}.loom", std::process::id()));
    let ns_id = WorkspaceId::v4_from_bytes([4u8; 16]);
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    loom.registry_mut()
        .create(FacetKind::Document, Some("docs"), ns_id)
        .unwrap();
    doc_put(
        &mut loom,
        ns_id,
        "notes",
        "note-1",
        b"hello enterprise world".to_vec(),
    )
    .unwrap();
    doc_put(&mut loom, ns_id, "notes", "skip-1", b"ignored".to_vec()).unwrap();
    doc_put(
        &mut loom,
        ns_id,
        "people",
        "ann",
        br#"{"profile":{"city":"Paris"}}"#.to_vec(),
    )
    .unwrap();
    loom_core::doc_create_index(
        &mut loom,
        ns_id,
        "people",
        loom_core::DocumentIndexDef::new(
            "by_city",
            loom_core::DocumentFieldPath::dotted("profile.city").unwrap(),
            false,
        )
        .unwrap(),
    )
    .unwrap();
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::persistent(loom))));

    let query = server
        .document_query(Parameters(PDocQuery {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id_prefix: Some("note-".to_string()),
            predicate: None,
            projections: Vec::new(),
            index: None,
            value: None,
            cursor: None,
            limit: Some(10),
            include_document: false,
        }))
        .unwrap()
        .0;
    assert_eq!(query["value"]["items"][0]["id"], json!("note-1"));
    assert_eq!(query["value"]["items"][0]["len"], json!(22));
    let base_digest = query["value"]["items"][0]["digest"]
        .as_str()
        .expect("document digest")
        .to_string();
    let indexed = server
        .document_query(Parameters(PDocQuery {
            workspace: "docs".to_string(),
            collection: "people".to_string(),
            id_prefix: None,
            predicate: Some(json!({"path":"profile.city","op":"eq","value":"Paris"})),
            projections: vec![PDocProjection {
                name: "city".to_string(),
                path: "profile.city".to_string(),
            }],
            index: None,
            value: None,
            cursor: None,
            limit: Some(10),
            include_document: false,
        }))
        .unwrap()
        .0;
    assert_eq!(indexed["value"]["items"][0]["id"], json!("ann"));
    assert_eq!(
        indexed["value"]["items"][0]["projections"]["city"],
        json!("Paris")
    );

    let partial = server
        .document_get_binary(Parameters(PDocId {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id: "note-1".to_string(),
        }))
        .unwrap()
        .0;
    assert_eq!(
        partial["value"]["bytes"],
        json!(b"hello enterprise world".to_vec())
    );

    let replaced = server
        .document_replace_text(Parameters(PDocReplaceText {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id: "note-1".to_string(),
            base_digest: base_digest.clone(),
            find: "enterprise".to_string(),
            replace: "durable".to_string(),
            replace_all: false,
        }))
        .unwrap()
        .0;
    assert_eq!(replaced["value"]["replacements"], json!(1));
    assert_eq!(
        replaced["value"]["digest"],
        json!(Digest::hash(Algo::Blake3, b"hello durable world").to_string())
    );

    let stale = server.document_replace_text(Parameters(PDocReplaceText {
        workspace: "docs".to_string(),
        collection: "notes".to_string(),
        id: "note-1".to_string(),
        base_digest,
        find: "durable".to_string(),
        replace: "safe".to_string(),
        replace_all: false,
    }));
    assert!(stale.is_err());

    let _ = std::fs::remove_file(&path);
}

#[test]
fn substrate_transact_applies_typed_ops_with_bound_scope() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;
    use rmcp::handler::server::wrapper::Parameters;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-substrate-transact-{}.loom",
        std::process::id()
    ));
    let ns_id = WorkspaceId::v4_from_bytes([14u8; 16]);
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    loom.registry_mut()
        .create(FacetKind::Document, Some("docs"), ns_id)
        .unwrap();
    let server = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::persistent(loom))),
        Binding::collection("docs", "notes"),
    );
    let graph_props = loom_codec::encode(&loom_codec::Value::Map(Vec::new())).unwrap();

    let result = server
        .substrate_transact(Parameters(PSubstrateTransact {
            ops: vec![
                PSubstrateTransactOp::CasPut {
                    workspace: None,
                    content: b"shared blob".to_vec(),
                },
                PSubstrateTransactOp::DocumentPut {
                    workspace: None,
                    collection: None,
                    id: "note-1".to_string(),
                    doc: b"hello".to_vec(),
                },
                PSubstrateTransactOp::GraphUpsertNode {
                    workspace: None,
                    collection: None,
                    id: "alice".to_string(),
                    props: graph_props.clone(),
                },
                PSubstrateTransactOp::GraphUpsertNode {
                    workspace: None,
                    collection: None,
                    id: "bob".to_string(),
                    props: graph_props.clone(),
                },
                PSubstrateTransactOp::GraphUpsertNode {
                    workspace: None,
                    collection: None,
                    id: "charlie".to_string(),
                    props: graph_props.clone(),
                },
                PSubstrateTransactOp::GraphUpsertEdge {
                    workspace: None,
                    collection: None,
                    id: "edge-1".to_string(),
                    src: "alice".to_string(),
                    dst: "bob".to_string(),
                    label: "mentions".to_string(),
                    props: graph_props.clone(),
                },
                PSubstrateTransactOp::GraphRemoveEdge {
                    workspace: None,
                    collection: None,
                    id: "missing-edge".to_string(),
                },
                PSubstrateTransactOp::GraphRemoveNode {
                    workspace: None,
                    collection: None,
                    id: "charlie".to_string(),
                    cascade: false,
                },
                PSubstrateTransactOp::SubstrateViewDefine {
                    workspace: None,
                    view_id: "status".to_string(),
                    source_scopes: vec!["docs".to_string()],
                    source_facets: vec!["document".to_string()],
                    projection_ref: "loom://projection/status".to_string(),
                    output_facet: Some("document".to_string()),
                    media_type: "application/json".to_string(),
                    freshness_policy: "on_write".to_string(),
                },
            ],
        }))
        .unwrap()
        .0;
    assert_eq!(result["value"]["applied"], json!(9));
    assert_eq!(result["value"]["results"][0]["kind"], json!("cas.put"));
    assert_eq!(result["value"]["results"][1]["kind"], json!("document.put"));
    assert_eq!(
        result["value"]["results"][2]["kind"],
        json!("graph.upsert_node")
    );
    assert_eq!(
        result["value"]["results"][3]["kind"],
        json!("graph.upsert_node")
    );
    assert_eq!(
        result["value"]["results"][4]["kind"],
        json!("graph.upsert_node")
    );
    assert_eq!(
        result["value"]["results"][5]["kind"],
        json!("graph.upsert_edge")
    );
    assert_eq!(
        result["value"]["results"][6]["kind"],
        json!("graph.remove_edge")
    );
    assert_eq!(result["value"]["results"][6]["value"], json!(false));
    assert_eq!(
        result["value"]["results"][7]["kind"],
        json!("graph.remove_node")
    );
    assert_eq!(result["value"]["results"][7]["value"], json!(null));
    assert_eq!(
        result["value"]["results"][8]["kind"],
        json!("substrate.view_define")
    );

    let doc = server
        .document_get_binary(Parameters(PDocId {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id: "note-1".to_string(),
        }))
        .unwrap()
        .0;
    assert_eq!(doc["value"]["bytes"], json!(b"hello".to_vec()));
    let node = server
        .graph_get_node(Parameters(PNsNameId {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id: "alice".to_string(),
        }))
        .unwrap()
        .0;
    assert_eq!(node["value"], json!(graph_props));
    let edge = server
        .graph_get_edge(Parameters(PNsNameId {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id: "edge-1".to_string(),
        }))
        .unwrap()
        .0;
    assert!(edge["value"].is_array());
    let view = server
        .substrate_view_get(Parameters(PSubstrateViewGet {
            workspace: "docs".to_string(),
            view_id: "status".to_string(),
        }))
        .unwrap()
        .0;
    assert_eq!(view["value"]["view_id"], json!("status"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn document_text_and_binary_tools_project_explicit_contract() {
    let path = std::env::temp_dir().join(format!(
        "loom-mcp-document-text-binary-{}.loom",
        std::process::id()
    ));
    let mcp = LoomMcp::new(StoreAccess::per_request(&path, None));
    mcp.write_workspace_create(Some("docs"), "document")
        .unwrap();
    let server = LoomServer::new(Arc::new(mcp));

    let text_put = server
        .document_put_text(Parameters(PDocPutText {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id: "intro".to_string(),
            text: "hello text".to_string(),
            expected_entity_tag: None,
        }))
        .unwrap()
        .0;
    let text_digest = text_put["digest"]
        .as_str()
        .expect("text digest")
        .to_string();
    let text = server
        .document_get_text(Parameters(PDocId {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id: "intro".to_string(),
        }))
        .unwrap()
        .0;
    assert_eq!(text["value"]["text"], json!("hello text"));
    assert_eq!(text["value"]["digest"], json!(text_digest));

    let binary_put = server
        .document_put_binary(Parameters(PDocPutBinary {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id: "blob".to_string(),
            bytes: vec![0xff, 0x00, 0x61],
            expected_entity_tag: None,
        }))
        .unwrap()
        .0;
    assert!(binary_put["digest"].as_str().is_some());
    let binary = server
        .document_get_binary(Parameters(PDocId {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id: "blob".to_string(),
        }))
        .unwrap()
        .0;
    assert_eq!(binary["value"]["bytes"], json!(vec![0xff, 0x00, 0x61]));
    let error = match server.document_get_text(Parameters(PDocId {
        workspace: "docs".to_string(),
        collection: "notes".to_string(),
        id: "blob".to_string(),
    })) {
        Ok(_) => panic!("binary document must not decode as text"),
        Err(error) => error,
    };
    assert!(error.message.contains("DOCUMENT_NOT_TEXT"));

    let _ = std::fs::remove_file(path);
}

#[test]
fn substrate_transact_rolls_back_after_failed_op() {
    use loom_core::document::doc_put;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Digest, Loom};
    use loom_store::FileStore;
    use rmcp::handler::server::wrapper::Parameters;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-substrate-transact-rollback-{}.loom",
        std::process::id()
    ));
    let ns_id = WorkspaceId::v4_from_bytes([15u8; 16]);
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    loom.registry_mut()
        .create(FacetKind::Document, Some("docs"), ns_id)
        .unwrap();
    doc_put(&mut loom, ns_id, "notes", "base", b"hello".to_vec()).unwrap();
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::persistent(loom))));
    let stale_digest = Digest::hash(Algo::Blake3, b"not current").to_string();

    let err = match server.substrate_transact(Parameters(PSubstrateTransact {
        ops: vec![
            PSubstrateTransactOp::DocumentPut {
                workspace: Some("docs".to_string()),
                collection: Some("notes".to_string()),
                id: "created".to_string(),
                doc: b"must roll back".to_vec(),
            },
            PSubstrateTransactOp::DocumentReplaceText {
                workspace: Some("docs".to_string()),
                collection: Some("notes".to_string()),
                id: "base".to_string(),
                base_digest: stale_digest,
                find: "hello".to_string(),
                replace: "goodbye".to_string(),
                replace_all: false,
            },
        ],
    })) {
        Ok(_) => panic!("stale transaction must fail"),
        Err(err) => err,
    };
    assert!(err.message.contains("CONFLICT"));

    let created = server
        .document_get_binary(Parameters(PDocId {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id: "created".to_string(),
        }))
        .unwrap()
        .0;
    assert_eq!(created["value"], Value::Null);
    let base = server
        .document_get_binary(Parameters(PDocId {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id: "base".to_string(),
        }))
        .unwrap()
        .0;
    assert_eq!(base["value"]["bytes"], json!(b"hello".to_vec()));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn read_only_binding_omits_write_tools() {
    let binding = Binding {
        allow_writes: false,
        ..Default::default()
    };
    let server = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::per_request(
            "/nonexistent.loom",
            None,
        ))),
        binding,
    );
    let registered: BTreeSet<String> = server
        .tool_router
        .list_all()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(registered.contains("store_version"));
    assert!(registered.contains("vcs_diff"));
    assert!(!registered.contains("fs_write_file"));
    assert!(!registered.contains("sql_exec"));
    assert!(
        registered
            .iter()
            .all(|name| { crate::tools::tool(name).unwrap().kind == ToolKind::Read })
    );
}

#[test]
fn unbound_read_only_tool_list_is_catalog_based_not_facet_based() {
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-tool-list-unbound-empty-{}.loom",
        std::process::id()
    ));
    let loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    let server = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::persistent(loom))),
        Binding {
            allow_writes: false,
            ..Default::default()
        },
    );
    let listed = server.listed_model_tools().expect("listed tools");
    let names = listed
        .iter()
        .map(|tool| tool.name.to_string())
        .collect::<BTreeSet<_>>();

    for read_tool in [
        "store_version",
        "vcs_diff",
        "document_get_text",
        "fs_read_file",
        "search",
        "chat_channels",
        "drive_list_conflicts",
    ] {
        assert!(names.contains(read_tool), "{read_tool} should be listed");
    }
    for write_tool in ["fs_write_file", "kv_put", "tickets_create"] {
        assert!(
            !names.contains(write_tool),
            "{write_tool} should be omitted"
        );
    }

    let expected_read_tools = crate::tools::TOOL_SURFACE
        .iter()
        .filter(|spec| spec.kind == ToolKind::Read)
        .filter(|spec| !super::APP_ONLY_TOOLS.contains(&spec.name))
        .count();
    assert!(
        names.len() >= expected_read_tools,
        "unbound read-only catalog should not collapse to existing facets"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn shutdown_controller_rejects_new_requests_and_waits_for_idle() {
    let shutdown = ShutdownController::new();
    let active = shutdown.begin_request().unwrap();
    shutdown.start_draining();
    assert!(shutdown.begin_request().is_err());

    let waiter = {
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            tokio::time::timeout(std::time::Duration::from_millis(50), shutdown.wait_idle())
                .await
                .is_ok()
        })
    };
    tokio::task::yield_now().await;
    assert!(!waiter.is_finished());
    drop(active);
    assert!(waiter.await.unwrap());
}

/// Every tool carries a title (identical in both places), group metadata, and read/write hints
/// consistent with its kind.
#[test]
fn every_tool_has_title_category_and_hints() {
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        "/nonexistent.loom",
        None,
    ))));
    for tool in server.tool_router.list_all() {
        let output = tool
            .output_schema
            .as_ref()
            .unwrap_or_else(|| panic!("{} has no output schema", tool.name));
        assert_eq!(
            output.get("type"),
            Some(&json!("object")),
            "{} output schema root",
            tool.name
        );
        assert_eq!(
            output.get("required"),
            Some(&json!(["value"])),
            "{} output schema required",
            tool.name
        );
        assert!(
            output
                .get("properties")
                .and_then(|v| v.as_object())
                .and_then(|p| p.get("value"))
                .is_some(),
            "{} output schema value",
            tool.name
        );
        let ann = tool
            .annotations
            .as_ref()
            .unwrap_or_else(|| panic!("{} has no annotations", tool.name));
        // Title set, non-empty, identical in Tool.title and annotations.title.
        let top = tool
            .title
            .as_deref()
            .unwrap_or_else(|| panic!("{} has no title", tool.name));
        assert!(!top.is_empty());
        assert_eq!(
            Some(top),
            ann.title.as_deref(),
            "{} title mismatch",
            tool.name
        );
        // _meta category and group present.
        let meta = tool
            .meta
            .as_ref()
            .unwrap_or_else(|| panic!("{} has no _meta", tool.name));
        assert!(
            meta.0.get("category").and_then(|v| v.as_str()).is_some(),
            "{} has no category",
            tool.name
        );
        assert!(
            meta.0.get("group").and_then(|v| v.as_str()).is_some(),
            "{} has no group",
            tool.name
        );
        // read/write hint matches the surface; closed-world everywhere.
        let spec = crate::tools::tool(&tool.name).unwrap();
        assert_eq!(
            ann.read_only_hint,
            Some(spec.kind == ToolKind::Read),
            "{} read_only",
            tool.name
        );
        assert_eq!(ann.open_world_hint, Some(false));
        // Write tools carry destructive + idempotent hints; reads omit them.
        if spec.kind == ToolKind::Write {
            assert!(
                ann.destructive_hint.is_some() && ann.idempotent_hint.is_some(),
                "{} missing write hints",
                tool.name
            );
        }
    }
}

/// MX-334: the Lane coordination-boundary rule must stay in every `lanes_*` tool description so the
/// contract -- Lane is coordination state; the ticket is the source of truth for work evidence --
/// is visible at the point of use and cannot drift back out of the tool surface.
#[test]
fn lane_tool_descriptions_carry_coordination_boundary() {
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        "/nonexistent.loom",
        None,
    ))));
    let mut checked = 0usize;
    for tool in server.tool_router.list_all() {
        if !tool.name.starts_with("lanes_") {
            continue;
        }
        let description = tool
            .description
            .as_deref()
            .unwrap_or_else(|| panic!("{} has no description", tool.name));
        assert!(
            description.contains("source of truth") && description.contains("coordination state"),
            "{} description must state the Lane coordination boundary, got: {description}",
            tool.name
        );
        checked += 1;
    }
    assert!(
        checked >= 8,
        "expected the full lanes_* tool surface to carry the boundary, saw {checked}"
    );
}

/// Spot-check representative classifications.
#[test]
fn metadata_spot_checks() {
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        "/nonexistent.loom",
        None,
    ))));
    let by_name = |n: &str| server.tool_router.get(n).cloned().unwrap();
    let cas_get = by_name("cas_get");
    assert_eq!(cas_get.title.as_deref(), Some("CAS: Get content by digest"));
    let cas_meta = cas_get.meta.unwrap().0;
    assert_eq!(cas_meta["category"], json!("CAS"));
    assert_eq!(cas_meta["group"], json!("CAS"));
    let search = by_name("search");
    assert_eq!(
        search.title.as_deref(),
        Some("Search: Search readable collections")
    );
    let search_meta = search.meta.unwrap().0;
    assert_eq!(search_meta["category"], json!("Search"));
    assert_eq!(search_meta["group"], json!("Search"));
    assert_eq!(
        search_meta["resource"]["resourceEquivalent"],
        json!("tool-only")
    );
    let substrate_refs = by_name("substrate_refs");
    assert_eq!(
        substrate_refs.meta.unwrap().0["resource"]["resourceTemplate"],
        json!("loom://{workspace}/substrate/refs/{target}.json")
    );
    assert_eq!(
        by_name("substrate_alias_bind").title.as_deref(),
        Some("Substrate: Bind an alias")
    );
    let substrate_view_get = by_name("substrate_view_get");
    assert_eq!(
        substrate_view_get.meta.unwrap().0["resource"]["resourceTemplate"],
        json!("loom://{workspace}/substrate/views/{view_id}.json")
    );
    let del = by_name("cas_delete");
    let a = del.annotations.unwrap();
    assert_eq!(a.read_only_hint, Some(false));
    assert_eq!(a.destructive_hint, Some(true));
    let put = by_name("kv_put").annotations.unwrap();
    assert_eq!(put.idempotent_hint, Some(true));
    assert_eq!(
        by_name("sql_read_table").title.as_deref(),
        Some("SQL: Read a table")
    );
    assert_eq!(
        by_name("studio_reindex").title.as_deref(),
        Some("Studio: Reindex projections")
    );
}

#[test]
fn dynamic_title_fallback_uses_capitalized_action_phrase() {
    assert_eq!(
        tool_title("custom.launch_example", "apps"),
        "Apps: Launch example"
    );
    assert_eq!(
        tool_title("custom_launch_example", "apps"),
        "Apps: Launch example"
    );
}

#[test]
fn output_schema_spot_checks() {
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        "/nonexistent.loom",
        None,
    ))));
    let value_schema = |name: &str| {
        server
            .tool_router
            .get(name)
            .and_then(|t| t.output_schema.as_ref())
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.as_object())
            .and_then(|p| p.get("value"))
            .cloned()
            .unwrap_or_else(|| panic!("{name} value schema"))
    };
    assert_eq!(value_schema("cas_has").get("type"), Some(&json!("boolean")));
    assert_eq!(
        value_schema("queue_append").get("type"),
        Some(&json!("integer"))
    );
    assert_eq!(
        value_schema("store_version").get("type"),
        Some(&json!("string"))
    );
    assert!(
        value_schema("studio_reindex")
            .get("required")
            .and_then(|required| required.as_array())
            .is_some_and(|required| required.contains(&json!("job_path")))
    );
    assert_eq!(
        value_schema("fs_write_file").get("type"),
        Some(&json!("null"))
    );
    assert_eq!(
        value_schema("watch_poll").get("type"),
        Some(&json!("object"))
    );
    let watch_poll_schema = value_schema("watch_poll");
    let watch_event_props = watch_poll_schema
        .get("properties")
        .and_then(|p| p.get("events"))
        .and_then(|e| e.get("items"))
        .and_then(|i| i.get("properties"))
        .and_then(|p| p.as_object())
        .expect("watch poll event properties");
    assert!(watch_event_props.contains_key("ref"));
    assert!(watch_event_props.contains_key("changes"));
    assert!(watch_event_props.contains_key("unsupported_domains"));
    assert!(value_schema("fs_read_file").get("anyOf").is_some());
    assert_eq!(value_schema("sql_query").get("type"), Some(&json!("array")));
    assert_eq!(
        value_schema("substrate_transact").get("required"),
        Some(&json!(["applied", "results"]))
    );
    assert_eq!(
        value_schema("substrate_changes")
            .get("properties")
            .and_then(|p| p.get("events"))
            .and_then(|e| e.get("items"))
            .and_then(|i| i.get("oneOf"))
            .and_then(|v| v.as_array())
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        value_schema("substrate_history").get("required"),
        Some(&json!([
            "scope_id",
            "entity_id",
            "index_present",
            "revisions",
            "latest",
            "checkpoints"
        ]))
    );
    assert_eq!(
        value_schema("substrate_revision_latest").get("required"),
        Some(&json!([
            "scope_id",
            "entity_id",
            "index_present",
            "revision"
        ]))
    );
    assert_eq!(
        value_schema("substrate_checkpoint_before").get("required"),
        Some(&json!([
            "scope_id",
            "index_present",
            "revision",
            "checkpoint"
        ]))
    );
    assert_eq!(
        value_schema("substrate_alias_bind").get("required"),
        Some(&json!([
            "alias", "target", "scope_id", "kind", "retired", "sequence"
        ]))
    );
    assert_eq!(
        value_schema("substrate_alias_release").get("type"),
        Some(&json!("boolean"))
    );
    assert!(
        value_schema("substrate_alias_resolve")
            .get("anyOf")
            .is_some()
    );
    assert_eq!(
        value_schema("substrate_alias_list")
            .get("items")
            .and_then(|item| item.get("required")),
        Some(&json!([
            "alias", "target", "scope_id", "kind", "retired", "sequence"
        ]))
    );
    assert!(
        value_schema("substrate_write_admission_policy_get")
            .get("anyOf")
            .is_some()
    );
    assert_eq!(
        value_schema("substrate_write_admission_policy_set").get("required"),
        Some(&json!([
            "workspace",
            "surface",
            "scope_id",
            "default_mode",
            "mandatory_targets"
        ]))
    );
    assert_eq!(
        value_schema("apps_call_tool").get("required"),
        Some(&json!(["tool", "result"]))
    );
}

#[test]
fn tools_list_returns_visible_callable_schemas() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-tool-list-visible-{}.loom",
        std::process::id()
    ));
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    loom.registry_mut()
        .create(
            FacetKind::Search,
            Some("search"),
            WorkspaceId::v4_from_bytes([15u8; 16]),
        )
        .unwrap();
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::persistent(loom))));
    let listed = server.listed_model_tools().expect("listed tools");
    let search = listed
        .iter()
        .find(|tool| tool.name == "search")
        .expect("search listed");
    assert_eq!(search.name, "search");
    assert_eq!(
        search.title.as_deref(),
        Some("Search: Search readable collections")
    );
    assert_eq!(
        search.meta.as_ref().expect("search meta").0["group"],
        json!("Search")
    );
    assert_eq!(
        search
            .output_schema
            .as_ref()
            .expect("search output schema")
            .get("properties")
            .and_then(|properties| properties.get("value"))
            .and_then(|value| value.get("required")),
        Some(&json!([
            "hits",
            "engine",
            "index_status",
            "reduced",
            "degraded"
        ]))
    );
    assert!(
        listed
            .iter()
            .all(|tool| tool.name != "ask_record" && tool.name != "apps_call_tool"),
        "app-only tools stay hidden"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn tools_list_keeps_full_schemas_for_visible_tools() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-tool-list-full-schema-{}.loom",
        std::process::id()
    ));
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    loom.registry_mut()
        .create(
            FacetKind::Vcs,
            Some("repo"),
            WorkspaceId::v4_from_bytes([16u8; 16]),
        )
        .unwrap();
    loom.registry_mut()
        .create(
            FacetKind::Search,
            Some("search"),
            WorkspaceId::v4_from_bytes([17u8; 16]),
        )
        .unwrap();
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::persistent(loom))));
    let listed = server.listed_model_tools().expect("listed tools");
    let by_name = |name: &str| {
        listed
            .iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("{name} listed"))
    };

    for name in ["search", "substrate_refs", "vcs_diff"] {
        let tool = by_name(name);
        assert!(tool.output_schema.is_some(), "{name} output schema");
        assert_eq!(
            tool.input_schema.get("type"),
            Some(&json!("object")),
            "{name} input schema"
        );
        assert!(
            !tool
                .meta
                .as_ref()
                .is_some_and(|meta| meta.0.contains_key("schemaDeferred")),
            "{name} must not use deferred schema metadata"
        );
        assert!(
            !tool
                .meta
                .as_ref()
                .is_some_and(|meta| meta.0.contains_key("schemaLookupTool")),
            "{name} must not advertise a schema lookup tool"
        );
    }
    let _ = std::fs::remove_file(&path);
}

fn collect_boolean_schema_paths(value: &serde_json::Value, path: &str, out: &mut Vec<String>) {
    let Some(object) = value.as_object() else {
        return;
    };
    for (key, child) in object {
        let child_path = format!("{path}.{key}");
        match key.as_str() {
            "const" | "default" | "enum" | "examples" => {}
            "$defs" | "definitions" | "dependentSchemas" | "patternProperties" | "properties" => {
                if let Some(children) = child.as_object() {
                    for (name, schema) in children {
                        let schema_path = format!("{child_path}.{name}");
                        if schema.is_boolean() {
                            out.push(schema_path);
                        } else {
                            collect_boolean_schema_paths(schema, &schema_path, out);
                        }
                    }
                }
            }
            "additionalItems"
            | "additionalProperties"
            | "contains"
            | "else"
            | "if"
            | "items"
            | "not"
            | "propertyNames"
            | "then"
            | "unevaluatedItems"
            | "unevaluatedProperties" => {
                if child.is_boolean() {
                    out.push(child_path);
                } else {
                    collect_boolean_schema_paths(child, &child_path, out);
                }
            }
            "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                if let Some(children) = child.as_array() {
                    for (index, schema) in children.iter().enumerate() {
                        let schema_path = format!("{child_path}.{index}");
                        if schema.is_boolean() {
                            out.push(schema_path);
                        } else {
                            collect_boolean_schema_paths(schema, &schema_path, out);
                        }
                    }
                }
            }
            _ => collect_boolean_schema_paths(child, &child_path, out),
        }
    }
}

#[test]
fn listed_tool_schemas_do_not_advertise_boolean_subschemas() {
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-tool-list-schema-bool-{}.loom",
        std::process::id()
    ));
    let loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    let server = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::persistent(loom))),
        Binding {
            allow_writes: false,
            ..Default::default()
        },
    );
    let listed = server.listed_model_tools().expect("listed tools");

    let mut failures = Vec::new();
    for (index, tool) in listed.iter().enumerate() {
        let input_schema = serde_json::Value::Object((*tool.input_schema).clone());
        let mut input_failures = Vec::new();
        collect_boolean_schema_paths(&input_schema, "inputSchema", &mut input_failures);
        failures.extend(
            input_failures
                .into_iter()
                .map(|path| format!("tools[{index}] {} {path}", tool.name)),
        );
        if let Some(output_schema) = &tool.output_schema {
            let output_schema = serde_json::Value::Object((**output_schema).clone());
            let mut output_failures = Vec::new();
            collect_boolean_schema_paths(&output_schema, "outputSchema", &mut output_failures);
            failures.extend(
                output_failures
                    .into_iter()
                    .map(|path| format!("tools[{index}] {} {path}", tool.name)),
            );
        }
    }

    assert!(
        failures.is_empty(),
        "boolean schema values rejected by stricter MCP clients: {failures:#?}"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn ticket_field_tool_parameters_are_object_shaped() {
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-ticket-field-schema-{}.loom",
        std::process::id()
    ));
    let loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    let server = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::persistent(loom))),
        Binding {
            allow_writes: true,
            ..Default::default()
        },
    );
    let listed = server.listed_model_tools().expect("listed tools");

    for (tool_name, field_name) in [
        ("tickets_create", "fields"),
        ("tickets_update", "set_fields"),
    ] {
        let tool = listed
            .iter()
            .find(|tool| tool.name == tool_name)
            .unwrap_or_else(|| panic!("missing tool {tool_name}"));
        let input_schema = serde_json::Value::Object((*tool.input_schema).clone());
        let field_schema = input_schema
            .pointer(&format!("/properties/{field_name}"))
            .unwrap_or_else(|| panic!("missing schema for {tool_name}.{field_name}"));
        assert_eq!(
            field_schema.get("type"),
            Some(&json!("object")),
            "{tool_name}.{field_name} must advertise a direct object schema: {field_schema}"
        );
        assert_eq!(
            field_schema.get("additionalProperties"),
            Some(&json!(true)),
            "{tool_name}.{field_name} must allow free-form ticket fields: {field_schema}"
        );
        assert!(
            !matches!(field_schema, serde_json::Value::Bool(_)),
            "{tool_name}.{field_name} must not collapse to a boolean schema"
        );
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn optional_ticket_update_scalars_are_not_empty_schemas() {
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-ticket-update-scalar-schema-{}.loom",
        std::process::id()
    ));
    let loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    let server = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::persistent(loom))),
        Binding {
            allow_writes: true,
            ..Default::default()
        },
    );
    let listed = server.listed_model_tools().expect("listed tools");
    let tool = listed
        .iter()
        .find(|tool| tool.name == "tickets_update")
        .expect("tickets_update tool");
    let input_schema = serde_json::Value::Object((*tool.input_schema).clone());

    for field_name in [
        "action",
        "assignee",
        "expected_root",
        "observed_source_status",
        "observed_workflow_version",
        "projection",
        "target_status",
    ] {
        let field_schema = input_schema
            .pointer(&format!("/properties/{field_name}"))
            .unwrap_or_else(|| panic!("missing schema for tickets_update.{field_name}"));
        assert_eq!(
            field_schema.get("type"),
            Some(&json!("string")),
            "tickets_update.{field_name} must advertise a direct string schema: {field_schema}"
        );
    }

    let _ = std::fs::remove_file(&path);
}

/// No model-facing input property may collapse to an empty `{}` schema. Stricter MCP clients treat
/// `{}` as "no shape" and stringify the value before dispatch, so every real object/map field must
/// advertise a direct `type: object` schema. The
/// only permitted `{}` properties are intentionally free-form JSON values (a polymorphic
/// scalar/array/object with no single applicable type), which must be allowlisted here with a
/// source-backed rationale. This test enumerates the whole model surface, so a new degraded
/// property fails the build until it is shaped (see `MODEL_TOOL_OBJECT_INPUT_FIELDS`) or allowlisted.
#[test]
fn model_tool_input_properties_are_not_empty_schemas() {
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    // (tool, property, rationale) properties intentionally free-form: the accepted value is a
    // polymorphic JSON scalar/array/object, so an empty `{}` schema is the accurate shape.
    const EMPTY_INPUT_ALLOWLIST: &[(&str, &str, &str)] = &[(
        "document_query",
        "value",
        "polymorphic tabular cell value (string/int/float/bool/bytes) matched against an index; no single JSON type applies",
    )];

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-empty-input-schema-{}.loom",
        std::process::id()
    ));
    let loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    let server = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::persistent(loom))),
        Binding {
            allow_writes: true,
            ..Default::default()
        },
    );
    let listed = server.listed_model_tools().expect("listed tools");

    let allow: std::collections::BTreeSet<(&str, &str)> = EMPTY_INPUT_ALLOWLIST
        .iter()
        .map(|(tool, property, _)| (*tool, *property))
        .collect();

    let mut failures = Vec::new();
    for tool in &listed {
        let input_schema = serde_json::Value::Object((*tool.input_schema).clone());
        let Some(properties) = input_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };
        for (property, schema) in properties {
            let is_empty_object = schema.as_object().is_some_and(|object| object.is_empty());
            if is_empty_object && !allow.contains(&(tool.name.as_ref(), property.as_str())) {
                failures.push(format!("{}.{property}", tool.name.as_ref()));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "model-facing input properties collapsed to an empty {{}} schema (shape them via MODEL_TOOL_OBJECT_INPUT_FIELDS or allowlist with rationale): {failures:?}"
    );

    let _ = std::fs::remove_file(&path);
}

/// Every prompt in PROMPT_SURFACE is registered (and nothing extra), each with a description.
#[test]
fn registered_prompts_equal_the_surface() {
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        "/nonexistent.loom",
        None,
    ))));
    let registered: BTreeSet<String> = server
        .prompt_router
        .list_all()
        .iter()
        .map(|p| p.name.to_string())
        .collect();
    let surface: BTreeSet<String> = crate::prompts::PROMPT_SURFACE
        .iter()
        .map(|p| p.name.to_string())
        .collect();
    assert_eq!(
        registered, surface,
        "registered prompts must equal PROMPT_SURFACE"
    );
    for p in server.prompt_router.list_all() {
        assert!(p.description.is_some(), "{} has no description", p.name);
    }
    for required in ["calendar_summarize_period", "contacts_find", "mail_triage"] {
        assert!(
            registered.contains(required),
            "missing PIM prompt {required}"
        );
    }
}

/// Resource templates are advertised, and `read_target` round-trips files and CAS blobs through
/// the facade (PEP-gated), with the content address as the version.
#[test]
fn resources_list_templates_and_read() {
    use crate::writes::SubstrateViewDefineRequest;
    use loom_core::document::doc_put;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom, cas_put};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-res-{}-{}.loom",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos()
    ));
    let digest = {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let fns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([1u8; 16]),
            )
            .unwrap();
        loom.write_file(fns, "a.txt", b"hello", 0o644).unwrap();
        let cns = loom
            .registry_mut()
            .create(
                FacetKind::Cas,
                Some("blobs"),
                WorkspaceId::v4_from_bytes([2u8; 16]),
            )
            .unwrap();
        let d = cas_put(&mut loom, cns, b"blobdata").unwrap().to_string();
        let dns = loom
            .registry_mut()
            .create(
                FacetKind::Document,
                Some("docs"),
                WorkspaceId::v4_from_bytes([3u8; 16]),
            )
            .unwrap();
        doc_put(
            &mut loom,
            dns,
            "notes",
            "note-1",
            b"references !ticket:42".to_vec(),
        )
        .unwrap();
        save_loom(&mut loom).unwrap();
        d
    };

    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        &path, None,
    ))));
    server
        .mcp
        .write_substrate_view_define(SubstrateViewDefineRequest {
            workspace: "repo",
            view_id: "status",
            source_scopes: &["repo"],
            source_facets: &["files"],
            projection_ref: "loom://projection/status",
            output_facet: Some("document"),
            media_type: "application/json",
            freshness_policy: "on_write",
        })
        .unwrap();
    let studio_workspace_id = server
        .mcp
        .read_workspace_get("repo")
        .expect("read repo workspace")
        .expect("repo workspace exists")
        .id;
    server
        .mcp
        .store()
        .write(|loom| {
            loom.store().control_set(
                &loom_substrate::lifecycle::lifecycle_operation_log_key(&studio_workspace_id)?,
                sample_lifecycle_operation_log(&studio_workspace_id)?,
            )
        })
        .unwrap();
    server
        .mcp
        .write_tickets_project_create(
            "repo",
            &studio_workspace_id,
            "eng",
            "ENG",
            "Engineering",
            None,
        )
        .unwrap();
    server
        .mcp
        .write_tickets_create(
            "repo",
            loom_tickets::TicketCreateRequest {
                workspace_id: &studio_workspace_id,
                project_id: "eng",
                ticket_type: "task",
                external_source: None,
                external_id: None,
                fields: &json!({
                    "title": "Ship status view",
                    "assignee": "owner",
                    "status_category": "in_progress"
                }),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .unwrap();
    assert_eq!(
        server.resource_templates().len(),
        crate::resources::TEMPLATES.len()
    );
    let nss = server.list_workspace_resources().expect("ns resources");
    assert!(nss.iter().any(|r| r.name == "repo") && nss.iter().any(|r| r.name == "blobs"));
    assert_eq!(
        server.resolve_resource_uri("loom://repo/"),
        Some(ResourceTarget::Workspace {
            workspace: "repo".into()
        })
    );
    assert_eq!(
        server.resolve_resource_uri("loom://repo/studio/views/status/principal/owner"),
        Some(ResourceTarget::StudioStatus {
            workspace: "repo".into(),
            principal: "owner".into(),
        })
    );
    assert_eq!(
        server.resolve_resource_uri("loom://repo/substrate/views/status.json"),
        Some(ResourceTarget::SubstrateView {
            workspace: "repo".into(),
            view_id: "status".into(),
        })
    );
    assert_eq!(
        server.resolve_resource_uri("loom://docs/substrate/refs/ticket:42.json"),
        Some(ResourceTarget::SubstrateRefs {
            workspace: "docs".into(),
            target: "ticket:42".into(),
        })
    );
    let root = server
        .read_target(&ResourceTarget::Workspace {
            workspace: "repo".into(),
        })
        .expect("workspace root");
    match root {
        ResourceContents::TextResourceContents {
            mime_type, text, ..
        } => {
            assert_eq!(mime_type.as_deref(), Some("application/json"));
            assert!(text.contains("\"name\": \"repo\""));
        }
        _ => panic!("workspace should be text contents"),
    }
    let status = server
        .read_target(&ResourceTarget::StudioStatus {
            workspace: "repo".into(),
            principal: "owner".into(),
        })
        .expect("studio status");
    match status {
        ResourceContents::TextResourceContents {
            mime_type, text, ..
        } => {
            assert_eq!(mime_type.as_deref(), Some("application/json"));
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value["view"], json!("studio.status"));
            assert_eq!(value["workspace"]["name"], json!("repo"));
            assert_eq!(value["principal"], json!("owner"));
            assert_eq!(
                value["projection_status"]["pending_decisions"],
                json!("source_backed")
            );
            assert_eq!(
                value["projection_status"]["changes_since_cursor"],
                json!("source_backed")
            );
            assert_eq!(
                value["projection_status"]["assigned_open_items"],
                json!("source_backed")
            );
            assert_eq!(
                value["projection_status"]["planning_markdown_mirror"],
                json!("source_backed")
            );
            assert_eq!(
                value["projection_status"]["active_lifecycle"],
                json!("source_backed")
            );
            assert_eq!(
                value["projection_status"]["review_comment_ownership"],
                json!("source_backed")
            );
            assert_eq!(
                value["sections"]["open_conflicts"]["status"],
                json!("source_backed")
            );
            assert_eq!(
                value["sections"]["assigned_open_items"]["value"]["assigned"][0]["primary_key"],
                json!("ENG-1")
            );
            assert_eq!(
                value["sections"]["assigned_open_items"]["value"]["assigned"][0]["title"],
                json!("Ship status view")
            );
            assert_eq!(
                value["sections"]["changes_since_cursor"]["value"]["sources"][0]["source"],
                json!("tickets")
            );
            assert_eq!(
                value["sections"]["changes_since_cursor"]["value"]["sources"][0]["count"],
                json!(2)
            );
            assert_eq!(
                value["sections"]["changes_since_cursor"]["value"]["sources"][0]["recent"][0]["operation_kind"],
                json!("project.created")
            );
            assert_eq!(
                value["sections"]["active_lifecycle"]["value"]["operation_log"]["count"],
                json!(1)
            );
            assert_eq!(
                value["sections"]["active_lifecycle"]["value"]["operation_log"]["recent"][0]["operation_kind"],
                json!("lifecycle.transitioned")
            );
            assert_eq!(
                value["sections"]["review_comment_ownership"]["value"]["meetings_extraction_review"]
                    ["configured"],
                json!(false)
            );
            assert_eq!(
                value["sections"]["review_comment_ownership"]["value"]["unresolved_inline_comments"]
                    ["status"],
                json!("target")
            );
            assert_eq!(
                value["sections"]["pending_decisions"]["value"]["has_pending"],
                json!(false)
            );
            assert_eq!(
                value["sections"]["planning_markdown_mirror"]["value"]["media_type"],
                json!("text/markdown")
            );
            assert!(
                value["sections"]["planning_markdown_mirror"]["value"]["body"]
                    .as_str()
                    .is_some_and(|body| body.contains("Ship status view"))
            );
            assert!(
                value["sections"]["suggested_prompts"]["value"]["items"]
                    .as_array()
                    .is_some_and(|items| !items.is_empty())
            );
        }
        _ => panic!("studio status should be text contents"),
    }
    let view = server
        .read_target(&ResourceTarget::SubstrateView {
            workspace: "repo".into(),
            view_id: "status".into(),
        })
        .expect("substrate view");
    match view {
        ResourceContents::TextResourceContents {
            mime_type, text, ..
        } => {
            assert_eq!(mime_type.as_deref(), Some("application/json"));
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value["view_id"], json!("status"));
            assert_eq!(value["freshness_policy"], json!("on_write"));
        }
        _ => panic!("substrate view should be text contents"),
    }
    let refs = server
        .read_target(&ResourceTarget::SubstrateRefs {
            workspace: "docs".into(),
            target: "ticket:42".into(),
        })
        .expect("substrate refs");
    match refs {
        ResourceContents::TextResourceContents {
            mime_type, text, ..
        } => {
            assert_eq!(mime_type.as_deref(), Some("application/json"));
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value["target"], json!("ticket:42"));
            assert_eq!(value["inbound"][0]["source_id"], json!("note-1"));
        }
        _ => panic!("substrate refs should be text contents"),
    }

    // File read.
    let f = server
        .read_target(&ResourceTarget::File {
            workspace: "repo".into(),
            path: "a.txt".into(),
        })
        .expect("file");
    match f {
        ResourceContents::BlobResourceContents { blob, .. } => {
            assert_eq!(blob, crate::resources::base64_encode(b"hello"))
        }
        _ => panic!("file should be blob contents"),
    }
    // CAS read.
    let c = server
        .read_target(&ResourceTarget::Cas {
            workspace: "blobs".into(),
            digest: digest.clone(),
        })
        .expect("cas");
    match c {
        ResourceContents::BlobResourceContents { blob, meta, .. } => {
            assert_eq!(blob, crate::resources::base64_encode(b"blobdata"));
            assert_eq!(meta.unwrap().0["version"], json!(digest));
        }
        _ => panic!("cas should be blob contents"),
    }
    // Missing CAS blob is a not-found error, not a panic.
    assert!(
        server
            .read_target(&ResourceTarget::Cas {
                workspace: "blobs".into(),
                digest: "blake3:00".into()
            })
            .is_err()
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn resource_read_rejects_oversized_delivered_payloads() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-res-budget-{}-{}.loom",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos()
    ));
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("repo"),
            WorkspaceId::v4_from_bytes([31u8; 16]),
        )
        .unwrap();
    loom.write_file(
        ns,
        "big.bin",
        &vec![b'x'; DEFAULT_RESOURCE_READ_MAX_BYTES + 1],
        0o644,
    )
    .unwrap();

    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::persistent(loom))));
    let error = server
        .read_target(&ResourceTarget::File {
            workspace: "repo".into(),
            path: "big.bin".into(),
        })
        .unwrap_err();
    assert!(error.message.contains("exceeds delivered payload budget"));
    assert!(
        error
            .message
            .contains("resources/read loom://repo/files/big.bin")
    );
    let _ = std::fs::remove_file(&path);
}

/// The ask flow end to end: `ask_questions` stores the pending ask and returns the Decisions app
/// launch payload, the app resource renders the questions through the `loom.ask` template binding,
/// `ask_record` records the answers, and `ask_answers` returns them.
#[tokio::test]
async fn ask_questions_record_answers_and_render() {
    use crate::server::params::{
        PAskAnswer, PAskBegin, PAskOption, PAskQuestion, PAskSubmit, PAskWait,
    };
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};
    use rmcp::handler::server::wrapper::Parameters;

    let path = std::env::temp_dir().join(format!("loom-mcp-ask-{}.loom", std::process::id()));
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([5u8; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        &path, None,
    ))));

    let question = |shape: &str, options: Option<Vec<PAskOption>>| PAskQuestion {
        question: "Which storage facet should asks use?".to_string(),
        context: Some("Asks must be durable and auditable.".to_string()),
        examples: Some("Document keeps JSON readable; kv is byte-oriented.".to_string()),
        options,
        recommendation: Some(
            "Document: JSON documents with string ids fit the ask shape.".to_string(),
        ),
        shape: shape.to_string(),
    };
    let options = || {
        Some(vec![
            PAskOption {
                label: "document".to_string(),
                description: Some("JSON documents".to_string()),
            },
            PAskOption {
                label: "kv".to_string(),
                description: None,
            },
        ])
    };

    // Validation: empty questions, bad shape, and optionless radio are rejected.
    assert!(
        server
            .ask_questions(Parameters(PAskBegin {
                workspace: "repo".to_string(),
                questions: vec![],
            }))
            .is_err()
    );
    assert!(
        server
            .ask_questions(Parameters(PAskBegin {
                workspace: "repo".to_string(),
                questions: vec![question("dropdown", options())],
            }))
            .is_err()
    );
    assert!(
        server
            .ask_questions(Parameters(PAskBegin {
                workspace: "repo".to_string(),
                questions: vec![question("radio", None)],
            }))
            .is_err()
    );

    // Begin: launch payload targets the internal Decisions app and carries the ask id.
    let begun = server
        .ask_questions(Parameters(PAskBegin {
            workspace: "repo".to_string(),
            questions: vec![question("radio", options()), question("text", None)],
        }))
        .unwrap();
    let meta = begun
        .meta
        .as_ref()
        .expect("ask_questions result meta")
        .0
        .clone();
    let value = &begun.structured_content.expect("structured content")["value"];
    assert_eq!(value["app"], json!(apps::INTERNAL_DECISIONS_APP));
    let ask_id = value["ask_id"].as_str().expect("ask id").to_string();
    // The launch payload and result meta address the ask's own app instance.
    let instance_uri = format!("ui://repo/mcp/apps/internal/decisions/{ask_id}");
    assert_eq!(meta["ui"]["resourceUri"], json!(instance_uri));
    assert_eq!(value["uri"], json!(instance_uri));

    // Pending wait: a zero timeout reports pending as a timeout without blocking.
    let waited = server
        .ask_answers(Parameters(PAskWait {
            workspace: "repo".to_string(),
            id: ask_id.clone(),
            timeout_ms: Some(0),
        }))
        .await
        .unwrap();
    assert_eq!(waited.0["value"]["status"], json!("timeout"));

    // The instance resource renders the pending ask through the loom.ask binding; the bare app
    // URI renders the latest ask (the `current` pointer).
    for uri in [
        instance_uri.as_str(),
        "ui://repo/mcp/apps/internal/decisions",
    ] {
        let target = server
            .resolve_resource_uri(uri)
            .expect("internal ask app uri resolves");
        match server.read_target(&target).unwrap() {
            ResourceContents::TextResourceContents { text, meta, .. } => {
                assert!(text.contains("<title>Decisions</title>"));
                assert!(text.contains("--accent-soft"));
                assert!(text.contains("const ASK = {"));
                assert!(text.contains("Which storage facet should asks use?"));
                assert!(text.contains("\"pending\""));
                let meta = meta.expect("content meta");
                assert_eq!(meta.0["loom"]["processing"], json!("templates"));
            }
            other => panic!("expected text app contents, got {other:?}"),
        }
    }

    // A second concurrent ask gets its own instance: both views render their own questions, and
    // the bare URI follows the latest ask.
    let second = server
        .ask_questions(Parameters(PAskBegin {
            workspace: "repo".to_string(),
            questions: vec![PAskQuestion {
                question: "Ship instance URIs in this release?".to_string(),
                context: None,
                examples: None,
                options: options(),
                // Recommendation is desired, not required at the wire: this ask omits it.
                recommendation: None,
                shape: "radio".to_string(),
            }],
        }))
        .unwrap();
    let second_value = &second.structured_content.expect("structured content")["value"];
    let second_id = second_value["ask_id"].as_str().expect("ask id").to_string();
    assert_ne!(second_id, ask_id);
    let second_uri = format!("ui://repo/mcp/apps/internal/decisions/{second_id}");
    let rendered = |uri: &str| {
        let target = server.resolve_resource_uri(uri).expect("ask uri resolves");
        match server.read_target(&target).unwrap() {
            ResourceContents::TextResourceContents { text, .. } => text,
            other => panic!("expected text app contents, got {other:?}"),
        }
    };
    let first_view = rendered(&instance_uri);
    assert!(first_view.contains("Which storage facet should asks use?"));
    assert!(!first_view.contains("Ship instance URIs in this release?"));
    let second_view = rendered(&second_uri);
    assert!(second_view.contains("Ship instance URIs in this release?"));
    assert!(!second_view.contains("Which storage facet should asks use?"));
    let bare_view = rendered("ui://repo/mcp/apps/internal/decisions");
    assert!(bare_view.contains("Ship instance URIs in this release?"));
    // An invalid instance segment does not resolve.
    assert!(
        server
            .resolve_resource_uri("ui://repo/mcp/apps/internal/decisions/../escape")
            .is_none()
    );

    // Submit: one answered, one skipped.
    server
        .ask_record(Parameters(PAskSubmit {
            workspace: "repo".to_string(),
            id: ask_id.clone(),
            answers: vec![
                PAskAnswer {
                    index: 0,
                    status: "answered".to_string(),
                    selected: Some(vec!["document".to_string()]),
                    text: None,
                },
                PAskAnswer {
                    index: 1,
                    status: "skipped".to_string(),
                    selected: None,
                    text: None,
                },
            ],
            aborted: None,
        }))
        .unwrap();
    // A second submit is rejected: the ask is no longer pending.
    assert!(
        server
            .ask_record(Parameters(PAskSubmit {
                workspace: "repo".to_string(),
                id: ask_id.clone(),
                answers: vec![],
                aborted: None,
            }))
            .is_err()
    );

    // Wait returns the recorded answers.
    let waited = server
        .ask_answers(Parameters(PAskWait {
            workspace: "repo".to_string(),
            id: ask_id.clone(),
            timeout_ms: Some(1_000),
        }))
        .await
        .unwrap();
    let value = &waited.0["value"];
    assert_eq!(value["status"], json!("answered"));
    assert_eq!(value["answers"][0]["status"], json!("answered"));
    assert_eq!(value["answers"][0]["selected"], json!(["document"]));
    assert_eq!(value["answers"][1]["status"], json!("skipped"));

    // The second ask is untouched by the first ask's submit and resolves independently: still
    // pending, then aborted.
    let waited = server
        .ask_answers(Parameters(PAskWait {
            workspace: "repo".to_string(),
            id: second_id.clone(),
            timeout_ms: Some(0),
        }))
        .await
        .unwrap();
    assert_eq!(waited.0["value"]["status"], json!("timeout"));
    server
        .ask_record(Parameters(PAskSubmit {
            workspace: "repo".to_string(),
            id: second_id.clone(),
            answers: vec![],
            aborted: Some(true),
        }))
        .unwrap();
    let waited = server
        .ask_answers(Parameters(PAskWait {
            workspace: "repo".to_string(),
            id: second_id,
            timeout_ms: Some(1_000),
        }))
        .await
        .unwrap();
    assert_eq!(waited.0["value"]["status"], json!("aborted"));
    assert_eq!(waited.0["value"]["answers"][0]["status"], json!("skipped"));

    // ask_record is app-only (`_meta.ui.visibility: ["app"]`): omitted from the model's
    // tools/list while remaining registered and dispatchable; the model-facing pair is not.
    let record_tool = server
        .tool_router
        .get("ask_record")
        .cloned()
        .expect("ask_record registered");
    assert!(tool_hidden_from_model(&record_tool));
    let questions_tool = server
        .tool_router
        .get("ask_questions")
        .cloned()
        .expect("ask_questions registered");
    assert!(!tool_hidden_from_model(&questions_tool));

    // Unknown ask ids are invalid-params, not a hang.
    assert!(
        server
            .ask_answers(Parameters(PAskWait {
                workspace: "repo".to_string(),
                id: "ask-unknown".to_string(),
                timeout_ms: Some(0),
            }))
            .await
            .is_err()
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn mcp_app_resources_list_and_read() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!("loom-mcp-app-{}.loom", std::process::id()));
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([3u8; 16]),
            )
            .unwrap();
        for vector in apps::APP_CONFORMANCE_VECTORS {
            let dir = apps::app_dir(vector.app);
            loom.create_directory_reserved(ns, &dir, true).unwrap();
            if let Some(meta_md) = vector.meta_md {
                loom.write_file_reserved(ns, &apps::meta_path(vector.app), meta_md, 0o100644)
                    .unwrap();
            }
            if let Some(index_html) = vector.index_html {
                loom.write_file_reserved(ns, &apps::index_path(vector.app), index_html, 0o100644)
                    .unwrap();
            }
        }
        let other = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("other"),
                WorkspaceId::v4_from_bytes([4u8; 16]),
            )
            .unwrap();
        loom.create_directory_reserved(other, &apps::app_dir("map"), true)
            .unwrap();
        loom.write_file_reserved(
            other,
            &apps::meta_path("map"),
            b"---\nname: Map\ndescription: Other map app\n---\n",
            0o100644,
        )
        .unwrap();
        loom.write_file_reserved(
            other,
            &apps::index_path("map"),
            b"<!doctype html><html><body>Other Map</body></html>",
            0o100644,
        )
        .unwrap();
        save_loom(&mut loom).unwrap();
    }

    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        &path, None,
    ))));
    let inventory = server.mcp.read_mcp_app_inventory("repo").unwrap();
    assert_eq!(
        inventory.len(),
        apps::APP_CONFORMANCE_VECTORS.len() + apps::INTERNAL_APPS.len()
    );
    for vector in apps::APP_CONFORMANCE_VECTORS {
        let item = inventory
            .iter()
            .find(|item| item.app == vector.app)
            .expect("vector has inventory row");
        assert_eq!(item.valid, vector.expect_valid);
        assert_eq!(item.status, vector.expect_status);
    }
    let internal = inventory
        .iter()
        .find(|item| item.app == apps::INTERNAL_VCS_APP)
        .expect("internal VCS app has inventory row");
    assert!(internal.valid);
    assert_eq!(internal.status, "valid");
    assert_eq!(
        internal.uri.as_deref(),
        Some("ui://repo/mcp/apps/internal/vcs")
    );
    let internal_ask = inventory
        .iter()
        .find(|item| item.app == apps::INTERNAL_DECISIONS_APP)
        .expect("internal Decisions app has inventory row");
    assert!(internal_ask.valid);
    assert_eq!(internal_ask.status, "valid");
    assert_eq!(
        internal_ask.uri.as_deref(),
        Some("ui://repo/mcp/apps/internal/decisions")
    );
    let directed_graph = inventory
        .iter()
        .find(|item| item.app == apps::DIRECTED_GRAPH_APP)
        .expect("Directed Graph app has inventory row");
    assert!(directed_graph.valid);
    assert_eq!(directed_graph.status, "valid");
    assert_eq!(
        directed_graph.uri.as_deref(),
        Some("ui://repo/mcp/apps/directed-graph")
    );
    for (app, uri) in [
        (
            apps::DOCUMENT_VIEWER_APP,
            "ui://repo/mcp/apps/document-viewer",
        ),
        (apps::MIND_MAP_APP, "ui://repo/mcp/apps/mind-map"),
        (apps::CANVAS_APP, "ui://repo/mcp/apps/canvas"),
        (
            apps::DIAGRAM_EDITOR_APP,
            "ui://repo/mcp/apps/diagram-editor",
        ),
    ] {
        let item = inventory
            .iter()
            .find(|item| item.app == app)
            .expect("Pages app has inventory row");
        assert!(item.valid, "{app} must be valid");
        assert_eq!(item.status, "valid");
        assert_eq!(item.uri.as_deref(), Some(uri));
    }

    let resources = server.list_all_resources().unwrap();
    let app = resources
        .iter()
        .find(|r| r.uri == "ui://repo/mcp/apps/map")
        .expect("app resource listed");
    assert!(resources.iter().all(
        |r| r.uri != "ui://repo/mcp/apps/broken" && r.uri != "ui://repo/mcp/apps/missing-index"
    ));
    assert_eq!(app.name, "repo/Map");
    assert_eq!(app.title.as_deref(), Some("repo/Map"));
    assert_eq!(app.description.as_deref(), Some("Interactive map app"));
    assert_eq!(app.mime_type.as_deref(), Some(apps::APP_MIME));
    let meta = app.meta.as_ref().expect("resource meta");
    assert_eq!(meta.0["ui"]["prefersBorder"], json!(true));
    assert_eq!(meta.0["ui"]["permissions"]["geolocation"], json!({}));
    assert_eq!(
        meta.0["ui"]["csp"]["resourceDomains"],
        json!(["https://cdn.example.com"])
    );
    assert!(
        resources
            .iter()
            .any(|r| r.uri == "ui://repo/mcp/apps/dashboard")
    );
    let other_app = resources
        .iter()
        .find(|r| r.uri == "ui://other/mcp/apps/map")
        .expect("same app name in another workspace is listed");
    assert_eq!(other_app.name, "other/Map");
    assert_eq!(other_app.title.as_deref(), Some("other/Map"));
    let internal = resources
        .iter()
        .find(|r| r.uri == "ui://repo/mcp/apps/internal/vcs")
        .expect("internal VCS app resource listed");
    assert_eq!(internal.name, "repo/VCS");
    assert_eq!(internal.mime_type.as_deref(), Some(apps::APP_MIME));

    let launcher_tools = server.list_app_launcher_tools().unwrap();
    assert!(launcher_tools.iter().any(|t| t.name == APP_OPEN_TOOL));
    let open_launcher = launcher_tools
        .iter()
        .find(|t| t.name == APP_OPEN_TOOL)
        .expect("generic app launcher listed");
    assert_eq!(open_launcher.title.as_deref(), Some("Open Loom App"));
    let map_launcher = launcher_tools
        .iter()
        .find(|t| t.name == "apps.launch.repo.map")
        .expect("repo map launcher listed");
    assert_eq!(map_launcher.title.as_deref(), Some("repo/Map"));
    assert_eq!(
        map_launcher.meta.as_ref().expect("launcher meta").0["ui"]["resourceUri"],
        json!("ui://repo/mcp/apps/map")
    );
    // Deprecated flat key must also be emitted for host compatibility (claude-ai-mcp#71).
    assert_eq!(
        map_launcher.meta.as_ref().expect("launcher meta").0["ui/resourceUri"],
        json!("ui://repo/mcp/apps/map")
    );
    // `map` declares no `ui.visibility`, so it defaults to both model and app.
    assert_eq!(
        map_launcher.meta.as_ref().expect("launcher meta").0["ui"]["visibility"],
        json!(["model", "app"])
    );
    assert!(
        launcher_tools
            .iter()
            .any(|t| t.name == "apps.launch.other.map")
    );
    assert!(
        launcher_tools
            .iter()
            .any(|t| t.name == "apps.launch.repo.internal.vcs")
    );
    assert!(
        launcher_tools
            .iter()
            .any(|t| t.name == "apps.launch.repo.directed-graph")
    );
    let launch_result = server
        .call_app_launcher_tool("apps.launch.repo.map", None)
        .unwrap()
        .expect("launcher handled");
    assert_eq!(
        launch_result.meta.as_ref().expect("launcher result meta").0["ui"]["resourceUri"],
        json!("ui://repo/mcp/apps/map")
    );
    assert_eq!(
        launch_result.structured_content.unwrap()["value"]["uri"],
        json!("ui://repo/mcp/apps/map")
    );
    let mut open_args = JsonObject::new();
    open_args.insert("workspace".to_string(), json!("repo"));
    open_args.insert("app".to_string(), json!("map"));
    let open_result = server
        .call_app_launcher_tool(APP_OPEN_TOOL, Some(open_args))
        .unwrap()
        .expect("generic launcher handled");
    assert_eq!(
        open_result.meta.as_ref().expect("open result meta").0["ui"]["resourceUri"],
        json!("ui://repo/mcp/apps/map")
    );
    assert_eq!(
        open_result.structured_content.unwrap()["value"]["uri"],
        json!("ui://repo/mcp/apps/map")
    );

    let target = server
        .resolve_resource_uri("ui://repo/mcp/apps/map")
        .expect("app uri resolves");
    let contents = server.read_target(&target).unwrap();
    match contents {
        ResourceContents::TextResourceContents {
            uri,
            mime_type,
            text,
            meta,
        } => {
            assert_eq!(uri, "ui://repo/mcp/apps/map");
            assert_eq!(mime_type.as_deref(), Some(apps::APP_MIME));
            assert!(text.contains("<body>Map</body>"));
            let meta = meta.expect("content meta");
            assert!(meta.0["version"].as_str().is_some());
            assert_eq!(meta.0["loom"]["processing"], json!("static"));
            assert_eq!(meta.0["ui"]["prefersBorder"], json!(true));
        }
        other => panic!("expected text app contents, got {other:?}"),
    }

    let target = server
        .resolve_resource_uri("ui://repo/mcp/apps/internal/vcs")
        .expect("internal app uri resolves");
    let contents = server.read_target(&target).unwrap();
    match contents {
        ResourceContents::TextResourceContents {
            uri,
            mime_type,
            text,
            meta,
        } => {
            assert_eq!(uri, "ui://repo/mcp/apps/internal/vcs");
            assert_eq!(mime_type.as_deref(), Some(apps::APP_MIME));
            assert!(text.contains("<title>VCS</title>"));
            assert!(!text.contains("loom.program"));
            assert!(text.contains("--accent-soft"));
            assert!(text.contains("const loomVcs = {"));
            assert!(text.contains("\"workspaces\""));
            assert!(text.contains("\"status\""));
            let meta = meta.expect("content meta");
            assert!(meta.0["version"].as_str().is_some());
            assert_eq!(meta.0["loom"]["processing"], json!("templates"));
            assert_eq!(meta.0["ui"]["prefersBorder"], json!(true));
        }
        other => panic!("expected text app contents, got {other:?}"),
    }

    let target = server
        .resolve_resource_uri("ui://repo/mcp/apps/directed-graph")
        .expect("Directed Graph app uri resolves");
    let contents = server.read_target(&target).unwrap();
    match contents {
        ResourceContents::TextResourceContents {
            uri,
            mime_type,
            text,
            meta,
        } => {
            assert_eq!(uri, "ui://repo/mcp/apps/directed-graph");
            assert_eq!(mime_type.as_deref(), Some(apps::APP_MIME));
            assert!(text.contains("<title>Directed Graph</title>"));
            assert!(text.contains("--accent-soft"));
            assert!(text.contains("const loomGraph = {"));
            assert!(text.contains("\"app_id\":\"directed-graph\""));
            assert!(text.contains("\"catalog\":{\"apps\":"));
            assert!(text.contains("\"nodes\":["));
            assert!(text.contains("\"kind\":\"renders\""));
            assert!(text.contains("\"id\":\"ticket-details\""));
            assert!(text.contains("\"id\":\"meeting-details\""));
            assert!(text.contains("\"kind\":\"elicits\""));
            assert!(text.contains("\"kind\":\"prompts\""));
            assert!(text.contains("\"read_tools\""));
            let meta = meta.expect("content meta");
            assert!(meta.0["version"].as_str().is_some());
            assert_eq!(meta.0["loom"]["processing"], json!("templates"));
            assert_eq!(meta.0["ui"]["prefersBorder"], json!(true));
        }
        other => panic!("expected text app contents, got {other:?}"),
    }

    let target = server
        .resolve_resource_uri("ui://repo/mcp/apps/document-viewer")
        .expect("Document Viewer app uri resolves");
    let contents = server.read_target(&target).unwrap();
    match contents {
        ResourceContents::TextResourceContents {
            uri,
            mime_type,
            text,
            meta,
        } => {
            assert_eq!(uri, "ui://repo/mcp/apps/document-viewer");
            assert_eq!(mime_type.as_deref(), Some(apps::APP_MIME));
            assert!(text.contains("<title>Spec Document Viewer</title>"));
            assert!(text.contains("--accent-soft"));
            assert!(text.contains("const data = {"));
            assert!(text.contains("\"app_id\":\"document-viewer\""));
            assert!(text.contains("\"spaces\""));
            assert!(text.contains("\"pages\""));
            assert!(text.contains("\"structures\""));
            assert!(text.contains("\"page\":null"));
            let meta = meta.expect("content meta");
            assert!(meta.0["version"].as_str().is_some());
            assert_eq!(meta.0["loom"]["processing"], json!("templates"));
            assert_eq!(meta.0["ui"]["prefersBorder"], json!(true));
        }
        other => panic!("expected text app contents, got {other:?}"),
    }

    let target = server
        .resolve_resource_uri("ui://repo/mcp/apps/dashboard")
        .expect("template app uri resolves");
    let contents = server.read_target(&target).unwrap();
    match contents {
        ResourceContents::TextResourceContents {
            uri,
            mime_type,
            text,
            meta,
        } => {
            assert_eq!(uri, "ui://repo/mcp/apps/dashboard");
            assert_eq!(mime_type.as_deref(), Some(apps::APP_MIME));
            assert!(!text.contains("loom.program"));
            let meta = meta.expect("content meta");
            assert!(meta.0["version"].as_str().is_some());
            assert_eq!(meta.0["loom"]["processing"], json!("templates"));
        }
        other => panic!("expected text app contents, got {other:?}"),
    }

    let bound = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::per_request(&path, None))),
        Binding::workspace("repo"),
    );
    let bound_resources = bound.list_all_resources().unwrap();
    let bound_map = bound_resources
        .iter()
        .find(|r| r.uri == "ui://mcp/apps/map")
        .expect("bound app resource listed");
    assert_eq!(bound_map.name, "Map");
    assert!(bound.resolve_resource_uri("ui://mcp/apps/map").is_some());
    let bound_launcher_tools = bound.list_app_launcher_tools().unwrap();
    assert!(
        bound_launcher_tools
            .iter()
            .any(|t| t.name == "apps.launch.map")
    );
    assert!(
        bound_launcher_tools
            .iter()
            .any(|t| t.name == "apps.launch.internal.vcs")
    );
    assert!(
        bound_launcher_tools
            .iter()
            .any(|t| t.name == "apps.launch.directed-graph")
    );
    assert!(
        bound_launcher_tools
            .iter()
            .all(|t| t.name != "apps.launch.repo.map" && t.name != "apps.launch.other.map")
    );
    let bound_launch = bound
        .call_app_launcher_tool("apps.launch.map", None)
        .unwrap()
        .expect("bound launcher handled");
    assert_eq!(
        bound_launch
            .meta
            .as_ref()
            .expect("bound launcher result meta")
            .0["ui"]["resourceUri"],
        json!("ui://mcp/apps/map")
    );
    assert_eq!(
        bound_launch.structured_content.unwrap()["value"]["uri"],
        json!("ui://mcp/apps/map")
    );
    assert!(
        bound_resources
            .iter()
            .any(|r| r.uri == "ui://mcp/apps/internal/vcs")
    );
    assert!(
        bound_resources
            .iter()
            .any(|r| r.uri == "ui://mcp/apps/directed-graph")
    );
    assert!(
        bound_resources
            .iter()
            .all(|r| !r.uri.starts_with("ui://other/mcp/apps/"))
    );
    assert!(
        bound
            .resolve_resource_uri("ui://mcp/apps/internal/vcs")
            .is_some()
    );
    assert!(
        bound
            .resolve_resource_uri("ui://mcp/apps/directed-graph")
            .is_some()
    );
    assert!(
        bound
            .resolve_resource_uri("ui://mcp/apps/document-viewer")
            .is_some()
    );
    assert!(
        server
            .resolve_resource_uri("ui://repo/mcp/apps/document-viewer/page/page-1")
            .is_some()
    );
    assert!(
        bound
            .resolve_resource_uri("ui://mcp/apps/mind-map")
            .is_some()
    );
    assert!(
        server
            .resolve_resource_uri("ui://repo/mcp/apps/mind-map/structure/roadmap")
            .is_some()
    );
    assert!(bound.resolve_resource_uri("ui://mcp/apps/canvas").is_some());
    assert!(
        server
            .resolve_resource_uri("ui://repo/mcp/apps/canvas/structure/roadmap")
            .is_some()
    );
    assert!(
        bound
            .resolve_resource_uri("ui://mcp/apps/diagram-editor")
            .is_some()
    );
    assert!(
        server
            .resolve_resource_uri("ui://repo/mcp/apps/diagram-editor/structure/roadmap")
            .is_some()
    );
    assert!(
        server
            .resolve_resource_uri("ui://repo/mcp/apps/mind-map/page/page-1")
            .is_none()
    );

    let shown = server
        .mcp
        .read_mcp_app_show("repo", "map")
        .unwrap()
        .expect("valid app show result");
    assert_eq!(shown.uri, "ui://repo/mcp/apps/map");
    let internal = server
        .mcp
        .read_mcp_app_show("repo", apps::INTERNAL_VCS_APP)
        .unwrap()
        .expect("valid internal app show result");
    assert_eq!(internal.uri, "ui://repo/mcp/apps/internal/vcs");
    let graph = server
        .mcp
        .read_mcp_app_show("repo", apps::DIRECTED_GRAPH_APP)
        .unwrap()
        .expect("valid Directed Graph app show result");
    assert_eq!(graph.uri, "ui://repo/mcp/apps/directed-graph");
    let document_viewer = server
        .mcp
        .read_mcp_app_show("repo", apps::DOCUMENT_VIEWER_APP)
        .unwrap()
        .expect("valid Document Viewer app show result");
    assert_eq!(document_viewer.uri, "ui://repo/mcp/apps/document-viewer");

    // Handler layer: `apps_show` / `apps_list` honor the active workspace binding.
    // Unbound keeps the fully-qualified URI; bound elides the workspace so it matches
    // resources/list, resources/read, and the launcher tools.
    {
        use crate::server::params::{PApp, PNs};
        use rmcp::handler::server::tool::IntoCallToolResult;
        use rmcp::handler::server::wrapper::Parameters;

        let show_uri = |srv: &LoomServer, app: &str| {
            srv.apps_show(Parameters(PApp {
                workspace: "repo".to_string(),
                app: app.to_string(),
            }))
            .unwrap()
            .into_call_tool_result()
            .unwrap()
            .structured_content
            .expect("apps_show structured content")["value"]["uri"]
                .clone()
        };
        assert_eq!(show_uri(&server, "map"), json!("ui://repo/mcp/apps/map"));
        assert_eq!(show_uri(&bound, "map"), json!("ui://mcp/apps/map"));
        assert_eq!(
            show_uri(&server, apps::INTERNAL_VCS_APP),
            json!("ui://repo/mcp/apps/internal/vcs")
        );
        assert_eq!(
            show_uri(&bound, apps::INTERNAL_VCS_APP),
            json!("ui://mcp/apps/internal/vcs")
        );

        // apps_list elides on the bound surface for every reported URI.
        let listed = bound
            .apps_list(Parameters(PNs {
                workspace: "repo".to_string(),
            }))
            .unwrap()
            .into_call_tool_result()
            .unwrap()
            .structured_content
            .expect("apps_list structured content");
        let uris: Vec<String> = listed["value"]
            .as_array()
            .expect("inventory array")
            .iter()
            .filter_map(|item| item["uri"].as_str().map(str::to_string))
            .collect();
        assert!(!uris.is_empty(), "expected at least one inventory URI");
        assert!(
            uris.iter().all(|u| u.starts_with("ui://mcp/apps/")),
            "bound apps_list must elide the workspace: {uris:?}"
        );
    }

    assert!(
        server
            .mcp
            .read_mcp_app_file("repo", apps::INTERNAL_VCS_APP, apps::INDEX_FILE)
            .unwrap()
            .expect("internal index file")
            .starts_with(b"<!doctype html>")
    );
    assert!(
        server
            .mcp
            .read_mcp_app_file("repo", apps::DIRECTED_GRAPH_APP, apps::INDEX_FILE)
            .unwrap()
            .expect("Directed Graph index file")
            .starts_with(b"<!doctype html>")
    );
    for app in [
        apps::DOCUMENT_VIEWER_APP,
        apps::MIND_MAP_APP,
        apps::CANVAS_APP,
        apps::DIAGRAM_EDITOR_APP,
    ] {
        assert!(
            server
                .mcp
                .read_mcp_app_file("repo", app, apps::INDEX_FILE)
                .unwrap()
                .expect("Pages app index file")
                .starts_with(b"<!doctype html>")
        );
    }
    assert!(
        server
            .mcp
            .write_mcp_app_create(
                "repo",
                apps::INTERNAL_VCS_APP,
                b"<!doctype html><html></html>",
                b"---\nname: VCS\n---\n",
            )
            .is_err()
    );
    server
        .mcp
        .write_mcp_app_create(
            "repo",
            "panel",
            b"<!doctype html><html><body>Panel</body></html>",
            b"---\nname: Panel\n---\n",
        )
        .unwrap();
    server
        .mcp
        .write_mcp_app_write_file("repo", "panel", "assets/data.json", b"{}", 0o100644)
        .unwrap();
    assert_eq!(
        server
            .mcp
            .read_mcp_app_file("repo", "panel", "assets/data.json")
            .unwrap(),
        Some(b"{}".to_vec())
    );
    server
        .mcp
        .write_mcp_app_remove_file("repo", "panel", "index.html")
        .unwrap();
    let panel = server
        .mcp
        .read_mcp_app_inventory("repo")
        .unwrap()
        .into_iter()
        .find(|item| item.app == "panel")
        .expect("panel inventory row");
    assert!(!panel.valid);
    assert_eq!(panel.status, "missing_index");
    assert!(
        server
            .list_all_resources()
            .unwrap()
            .iter()
            .all(|r| r.uri != "ui://repo/mcp/apps/panel")
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn pages_app_instances_render_selected_records() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};
    use loom_substrate::body::{Block, BlockKind, Body, TextRun};
    use loom_substrate::order_token::first_token;
    use rmcp::handler::server::wrapper::Parameters;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-pages-app-instance-{}.loom",
        std::process::id()
    ));
    let workspace_id = WorkspaceId::v4_from_bytes([61u8; 16]).to_string();
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([61u8; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let mcp = Arc::new(LoomMcp::new(StoreAccess::per_request(&path, None)));
    let space = mcp
        .write_spaces_create("repo", &workspace_id, "eng", "Engineering", None)
        .expect("create space");
    let page = mcp
        .write_pages_create(
            "repo",
            PageCreateRequest {
                workspace_id: &workspace_id,
                page_id: "spec-001",
                space_id: "eng",
                parent_page_id: None,
                title: "Spec 001",
                expected_root: Some(&space.profile_root),
            },
        )
        .expect("create page");
    let update = mcp
        .write_pages_update(
            "repo",
            &workspace_id,
            "spec-001",
            Body::new(vec![
                Block::new(
                    "intro",
                    first_token(),
                    BlockKind::Paragraph,
                    vec![TextRun::new("Enterprise Pages body", vec![]).unwrap()],
                    vec![],
                )
                .unwrap(),
            ])
            .encode()
            .unwrap(),
            Some(&page.profile_root),
        )
        .expect("update page");
    let publish = mcp
        .write_pages_publish(
            "repo",
            &workspace_id,
            "spec-001",
            Some(&update.profile_root),
        )
        .expect("publish page");
    let source_page = mcp
        .write_pages_create(
            "repo",
            PageCreateRequest {
                workspace_id: &workspace_id,
                page_id: "source",
                space_id: "eng",
                parent_page_id: None,
                title: "Source",
                expected_root: Some(&publish.profile_root),
            },
        )
        .expect("create source page");
    let source_update = mcp
        .write_pages_update(
            "repo",
            &workspace_id,
            "source",
            Body::new(vec![
                Block::new(
                    "selected-page",
                    first_token(),
                    BlockKind::BlockRef {
                        entity_id: "page:spec-001".to_string(),
                        block_id: None,
                        section: false,
                        pin: None,
                    },
                    vec![],
                    vec![],
                )
                .unwrap(),
            ])
            .encode()
            .unwrap(),
            Some(&source_page.profile_root),
        )
        .expect("update source page");
    let source_publish = mcp
        .write_pages_publish(
            "repo",
            &workspace_id,
            "source",
            Some(&source_update.profile_root),
        )
        .expect("publish source page");
    let structure = mcp
        .write_structures_create(
            "repo",
            StructureCreateRequest {
                workspace_id: &workspace_id,
                structure_id: "roadmap",
                space_id: "eng",
                kind: "mindmap",
                title: "Roadmap",
                expected_root: Some(&source_publish.profile_root),
            },
        )
        .expect("create structure");
    let root = mcp
        .write_structures_add_node(
            "repo",
            StructureNodeRequest {
                workspace_id: &workspace_id,
                structure_id: "roadmap",
                node_id: "root",
                kind: "topic",
                label: "Root",
                body_digest: None,
                entity_ref: Some("page:spec-001".to_string()),
                expected_root: Some(&structure.structure.profile_root),
            },
        )
        .expect("add root node");
    mcp.write_structures_add_node(
        "repo",
        StructureNodeRequest {
            workspace_id: &workspace_id,
            structure_id: "roadmap",
            node_id: "feature",
            kind: "topic",
            label: "Feature",
            body_digest: None,
            entity_ref: None,
            expected_root: Some(&root.profile_root),
        },
    )
    .expect("add feature node");

    let server = LoomServer::new(mcp);
    let pages = server
        .pages_list(Parameters(PSpacesList {
            workspace: "repo".to_string(),
            page: PResultPage::default(),
        }))
        .unwrap()
        .0;
    assert!(
        pages["value"]
            .as_array()
            .expect("pages array")
            .iter()
            .any(|page| page["page_id"] == json!("spec-001"))
    );
    let structures = server
        .structures_list(Parameters(PSpacesList {
            workspace: "repo".to_string(),
            page: PResultPage::default(),
        }))
        .unwrap()
        .0;
    assert!(
        structures["value"]
            .as_array()
            .expect("structures array")
            .iter()
            .any(|structure| structure["structure_id"] == json!("roadmap"))
    );
    let page_target = server
        .resolve_resource_uri("ui://repo/mcp/apps/document-viewer/page/spec-001")
        .expect("page app instance resolves");
    match server.read_target(&page_target).unwrap() {
        ResourceContents::TextResourceContents { uri, text, .. } => {
            assert_eq!(uri, "ui://repo/mcp/apps/document-viewer/page/spec-001");
            assert!(text.contains("\"page_id\":\"spec-001\""));
            assert!(text.contains("\"title\":\"Spec 001\""));
            assert!(text.contains("\"pages\":["));
            assert!(text.contains("\"history\":["));
            assert!(text.contains("\"backlinks\":"));
            assert!(text.contains("\"source_id\":\"source\""));
            assert!(text.contains("\"relation\":\"transcludes\""));
            assert!(
                text.contains("\"app_uri\":\"ui://repo/mcp/apps/document-viewer/page/spec-001\"")
            );
            assert!(text.contains("apps_call_tool"));
            assert!(text.contains("pages_publish"));
            assert!(text.contains("Enterprise Pages body"));
        }
        other => panic!("expected text app contents, got {other:?}"),
    }
    let (tool, args) = server
        .prepare_app_tool_call(PAppCallTool {
            app_uri: "ui://repo/mcp/apps/document-viewer/page/spec-001".to_string(),
            tool: "pages_publish".to_string(),
            arguments: Some(serde_json::Map::from_iter([
                ("workspace".to_string(), json!("repo")),
                ("page_id".to_string(), json!("spec-001")),
                (
                    "expected_root".to_string(),
                    json!(publish.profile_root.clone()),
                ),
            ])),
        })
        .expect("page app publish action prepares");
    assert_eq!(tool, "pages_publish");
    assert_eq!(args.expect("publish args")["page_id"], json!("spec-001"));
    let structure_target = server
        .resolve_resource_uri("ui://repo/mcp/apps/mind-map/structure/roadmap")
        .expect("structure app instance resolves");
    match server.read_target(&structure_target).unwrap() {
        ResourceContents::TextResourceContents { uri, text, .. } => {
            assert_eq!(uri, "ui://repo/mcp/apps/mind-map/structure/roadmap");
            assert!(text.contains("\"structure_id\":\"roadmap\""));
            assert!(text.contains("\"title\":\"Roadmap\""));
            assert!(text.contains("\"app_uri\":\"ui://repo/mcp/apps/mind-map/structure/roadmap\""));
            assert!(text.contains("\"structures\":["));
            assert!(text.contains("\"node_id\":\"root\""));
            assert!(text.contains("\"node_id\":\"feature\""));
            assert!(text.contains("structures_add_node"));
            assert!(text.contains("structures_move_node"));
            assert!(text.contains("structures_link_node"));
        }
        other => panic!("expected text app contents, got {other:?}"),
    }
    for tool in [
        "structures_add_node",
        "structures_move_node",
        "structures_link_node",
    ] {
        let (prepared, _) = server
            .prepare_app_tool_call(PAppCallTool {
                app_uri: "ui://repo/mcp/apps/mind-map/structure/roadmap".to_string(),
                tool: tool.to_string(),
                arguments: Some(serde_json::Map::from_iter([
                    ("workspace".to_string(), json!("repo")),
                    ("structure_id".to_string(), json!("roadmap")),
                    (
                        "expected_root".to_string(),
                        json!(root.profile_root.clone()),
                    ),
                ])),
            })
            .expect("structure app action prepares");
        assert_eq!(prepared, tool);
    }
    let bound = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::per_request(&path, None))),
        Binding::workspace("repo"),
    );
    assert!(
        bound
            .resolve_resource_uri("ui://mcp/apps/document-viewer/page/spec-001")
            .is_some()
    );
    assert!(
        bound
            .resolve_resource_uri("ui://mcp/apps/canvas/structure/roadmap")
            .is_some()
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn ticket_apps_render_source_backed_ticket_state() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-ticket-app-instance-{}.loom",
        std::process::id()
    ));
    let workspace_id = WorkspaceId::v4_from_bytes([74u8; 16]).to_string();
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([74u8; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let mcp = Arc::new(LoomMcp::new(StoreAccess::per_request(&path, None)));
    mcp.write_tickets_project_create("repo", &workspace_id, "core", "CORE", "Core", None)
        .expect("create ticket project");
    for (field_id, field_type) in [("sprint", "string"), ("story_points", "integer")] {
        mcp.write_tickets_field_put(
            "repo",
            loom_tickets::TicketFieldDefinitionWriteRequest {
                workspace_id: &workspace_id,
                project_id: "core",
                field_id,
                key: field_id,
                name: field_id,
                description: None,
                field_type,
                option_set: None,
                max_length: Some(512),
                required: false,
                searchable: true,
                orderable: true,
                cardinality: loom_tickets::TicketFieldCardinality::Optional,
                applicable_type_ids: &[],
                expected_root: None,
            },
        )
        .expect("define ticket field");
    }
    let ticket = mcp
        .write_tickets_create(
            "repo",
            loom_tickets::TicketCreateRequest {
                workspace_id: &workspace_id,
                project_id: "core",
                ticket_type: "story",
                external_source: Some("fixture"),
                external_id: Some("T-1"),
                fields: &json!({
                    "title": "Build ticket planning apps",
                    "assignee": "principal:alice",
                    "status_category": "in_progress",
                    "sprint": "Sprint 1",
                    "story_points": 5,
                    "due_date": "2026-08-01"
                }),
                policy_labels: &[],
                expected_root: None,
            },
        )
        .expect("create ticket");
    mcp.write_lanes_create(
        "repo",
        crate::writes::LaneCreateRequest {
            lane_id: "lane-1",
            lane_key: "current",
            title: "Current lane",
            description: "MCP application projection test lane.",
            lane_kind: loom_lanes::LaneKind::Assignment.as_str(),
            owner_principal: Some("principal:alice"),
            lane_status: "working",
            lane_tickets: &[loom_lanes::LaneTicket {
                ticket_id: ticket.ticket_id.clone(),
                order_key: "F".to_string(),
            }],
            active_ticket_id: Some(&ticket.ticket_id),
            status_report: "working",
            reviewer_feedback: "",
            updated_by: Some("principal:alice"),
        },
    )
    .expect("create lane");

    let server = LoomServer::new(mcp);
    for app in [
        apps::TICKET_DETAILS_APP,
        apps::BOARD_APP,
        apps::ROADMAP_APP,
        apps::SPRINT_PLANNER_APP,
        apps::BACKLOG_TRIAGE_APP,
        apps::DASHBOARDS_APP,
    ] {
        assert!(
            server
                .resolve_resource_uri(&format!("ui://repo/mcp/apps/{app}"))
                .is_some(),
            "{app} resolves"
        );
        let app_show = server
            .mcp
            .read_mcp_app_show("repo", app)
            .unwrap()
            .expect("internal ticket app show result");
        assert_eq!(app_show.uri, format!("ui://repo/mcp/apps/{app}"));
    }
    let target = server
        .resolve_resource_uri(&format!(
            "ui://repo/mcp/apps/ticket-details/ticket/{}",
            ticket.ticket_id
        ))
        .expect("ticket detail instance resolves");
    match server.read_target(&target).unwrap() {
        ResourceContents::TextResourceContents { uri, text, .. } => {
            assert_eq!(
                uri,
                format!(
                    "ui://repo/mcp/apps/ticket-details/ticket/{}",
                    ticket.ticket_id
                )
            );
            assert!(text.contains("\"app_id\":\"ticket-details\""));
            assert!(text.contains("\"primary_key\":\"CORE-1\""));
            assert!(text.contains("Build ticket planning apps"));
            assert!(text.contains("\"lanes\":["));
            assert!(text.contains("\"lane_key\":\"current\""));
            assert!(text.contains("\"history\":["));
            assert!(text.contains("\"refs\":"));
            assert!(text.contains("tickets_update"));
            assert!(text.contains("apps_call_tool"));
            assert!(text.contains("waiting_for_review"));
            assert!(text.contains("feedback_available"));
            assert!(!text.contains("<option value=\"todo\">"));
            assert!(!text.contains("<option value=\"done\">"));
            assert!(!text.contains("Update requested"));
            assert!(!text.contains("tickets.rank"));
            assert!(!text.contains("tickets.transition"));
        }
        other => panic!("expected text ticket app contents, got {other:?}"),
    }
    let (tool, args) = server
        .prepare_app_tool_call(PAppCallTool {
            app_uri: format!(
                "ui://repo/mcp/apps/ticket-details/ticket/{}",
                ticket.ticket_id
            ),
            tool: "tickets_update".to_string(),
            arguments: Some(serde_json::Map::from_iter([
                ("workspace".to_string(), json!("repo")),
                ("ticket_id".to_string(), json!(ticket.ticket_id.clone())),
                ("target_status".to_string(), json!("accepted")),
                ("observed_source_status".to_string(), json!("in_progress")),
            ])),
        })
        .expect("ticket update action prepares");
    assert_eq!(tool, "tickets_update");
    assert_eq!(
        args.expect("ticket args")["ticket_id"],
        json!(ticket.ticket_id)
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn chat_apps_render_source_backed_channel_state() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-chat-app-instance-{}.loom",
        std::process::id()
    ));
    let workspace_id = WorkspaceId::v4_from_bytes([75u8; 16]).to_string();
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([75u8; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let mcp = Arc::new(LoomMcp::new(StoreAccess::per_request(&path, None)));
    let channel = mcp
        .write_chat_create_channel("repo", &workspace_id, "general", "General")
        .expect("create chat channel");
    mcp.write_chat_emoji_register("repo", &workspace_id, "approved")
        .expect("register emoji");
    mcp.write_chat_post_message(
        "repo",
        &workspace_id,
        "general",
        "m1",
        None,
        b"Welcome to the launch room".to_vec(),
    )
    .expect("post channel message");
    mcp.write_chat_create_thread("repo", &workspace_id, "general", "t1", "m1")
        .expect("create thread");
    mcp.write_chat_post_message(
        "repo",
        &workspace_id,
        "general",
        "m2",
        Some("t1"),
        b"Thread reply with the latest status".to_vec(),
    )
    .expect("post thread reply");
    mcp.write_chat_add_reaction("repo", &workspace_id, "general", "m1", "approved")
        .expect("add reaction");
    mcp.write_chat_create_task(
        "repo",
        &workspace_id,
        "general",
        "task-1",
        Some("m1"),
        "Follow up with release owner",
    )
    .expect("create task");
    mcp.write_chat_claim_task("repo", &workspace_id, "general", "task-1", "claim-1", None)
        .expect("claim task");
    let agent_principal = WorkspaceId::v4_from_bytes([76u8; 16]).to_string();
    mcp.write_chat_invoke_agent(
        "repo",
        &workspace_id,
        "general",
        "invoke-1",
        &agent_principal,
        vec!["m1".to_string()],
        b"Summarize blockers".to_vec(),
    )
    .expect("invoke agent participant");
    mcp.write_chat_request_handoff(
        "repo",
        &workspace_id,
        "general",
        "handoff-1",
        &agent_principal,
        None,
        Some("Needs human owner"),
    )
    .expect("request handoff");

    let server = LoomServer::new(mcp);
    server
        .chat_presence_set("repo", &workspace_id, "general", "active", 30_000)
        .expect("set presence");
    for app in [
        apps::CHAT_CHANNEL_APP,
        apps::CHAT_THREAD_APP,
        apps::CHAT_TASKS_APP,
        apps::CHAT_PRESENCE_APP,
        apps::CHAT_HANDOFFS_APP,
    ] {
        assert!(
            server
                .resolve_resource_uri(&format!("ui://repo/mcp/apps/{app}"))
                .is_some(),
            "{app} resolves"
        );
        let shown = server
            .mcp
            .read_mcp_app_show("repo", app)
            .unwrap()
            .expect("internal chat app show result");
        assert_eq!(shown.uri, format!("ui://repo/mcp/apps/{app}"));
    }
    let channel_target = server
        .resolve_resource_uri(&format!(
            "ui://repo/mcp/apps/chat-channel/channel/{}",
            channel.channel_id
        ))
        .expect("chat channel app instance resolves");
    match server.read_target(&channel_target).unwrap() {
        ResourceContents::TextResourceContents { uri, text, .. } => {
            assert_eq!(
                uri,
                format!(
                    "ui://repo/mcp/apps/chat-channel/channel/{}",
                    channel.channel_id
                )
            );
            assert!(text.contains("\"app_id\":\"chat-channel\""));
            assert!(text.contains("\"channel_handle\":\"general\""));
            assert!(text.contains("\"message_id\":\"m1\""));
            assert!(text.contains("\"message_id\":\"m2\""));
            assert!(text.contains("Follow up with release owner"));
            assert!(text.contains("handoff-1"));
            assert!(text.contains("\"status\":\"active\""));
            assert!(text.contains("chat_post_message"));
            assert!(text.contains("chat_set_presence"));
            assert!(!text.contains("chat.upload_attachment"));
        }
        other => panic!("expected text chat app contents, got {other:?}"),
    }
    assert!(
        server
            .resolve_resource_uri(&format!(
                "ui://repo/mcp/apps/chat-thread/channel/{}/thread/t1",
                channel.channel_id
            ))
            .is_some()
    );
    let (tool, args) = server
        .prepare_app_tool_call(PAppCallTool {
            app_uri: format!(
                "ui://repo/mcp/apps/chat-channel/channel/{}",
                channel.channel_id
            ),
            tool: "chat_post_message".to_string(),
            arguments: Some(serde_json::Map::from_iter([
                ("workspace".to_string(), json!("repo")),
                ("channel_id".to_string(), json!(channel.channel_id.clone())),
                ("message_id".to_string(), json!("m3")),
                ("body".to_string(), json!(b"new message".to_vec())),
            ])),
        })
        .expect("chat post action prepares");
    assert_eq!(tool, "chat_post_message");
    assert_eq!(args.expect("chat args")["message_id"], json!("m3"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn drive_apps_render_source_backed_drive_state() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-drive-app-instance-{}.loom",
        std::process::id()
    ));
    let workspace_id = WorkspaceId::v4_from_bytes([77u8; 16]).to_string();
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([77u8; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let mcp = Arc::new(LoomMcp::new(StoreAccess::per_request(&path, None)));
    let root = mcp
        .read_drive_list("repo", &workspace_id, "root")
        .expect("read drive root")
        .profile_root;
    let folder = mcp
        .write_drive_create_folder(
            "repo",
            &workspace_id,
            "root",
            "folder-1",
            "Specs",
            &root,
            None,
        )
        .expect("create folder");
    mcp.write_drive_create_upload(
        "repo",
        crate::drive::DriveCreateUploadRequest {
            workspace_id: &workspace_id,
            upload_id: "upload-1",
            parent_folder_id: "folder-1",
            name: "Plan.txt",
            file_id: "file-1",
            expected_root: &folder.profile_root,
            created_at_ms: 100,
            replace_file: false,
        },
        None,
    )
    .expect("create upload");
    mcp.write_drive_upload_chunk("repo", &workspace_id, "upload-1", b"drive app bytes", None)
        .expect("upload bytes");
    let committed = mcp
        .write_drive_commit_upload("repo", &workspace_id, "upload-1", None)
        .expect("commit upload");
    mcp.write_drive_grant_share(
        "repo",
        crate::drive::DriveGrantShareRequest {
            workspace_id: &workspace_id,
            grant_id: "grant-1",
            target_kind: "file",
            target_id: "file-1",
            principal: &WorkspaceId::v4_from_bytes([78u8; 16]).to_string(),
            role: "viewer",
            granted_at_ms: 200,
            expires_at_ms: Some(400),
        },
    )
    .expect("grant share");
    mcp.write_drive_pin_retention(
        "repo",
        crate::drive::DrivePinRetentionRequest {
            workspace_id: &workspace_id,
            pin_id: "pin-1",
            kind: "current_root",
            root: &committed.profile_root,
            target_entity_id: Some("drive:file:file-1"),
            added_at_ms: 300,
            expires_at_ms: Some(500),
        },
    )
    .expect("pin retention");
    mcp.write_drive_create_upload(
        "repo",
        crate::drive::DriveCreateUploadRequest {
            workspace_id: &workspace_id,
            upload_id: "upload-2",
            parent_folder_id: "folder-1",
            name: "Plan.txt",
            file_id: "file-2",
            expected_root: &committed.profile_root,
            created_at_ms: 400,
            replace_file: false,
        },
        None,
    )
    .expect("create conflict upload");
    mcp.write_drive_upload_chunk("repo", &workspace_id, "upload-2", b"conflict bytes", None)
        .expect("upload conflict bytes");
    mcp.write_drive_commit_upload("repo", &workspace_id, "upload-2", None)
        .expect("commit conflict upload");

    let server = LoomServer::new(mcp);
    for app in [
        apps::DRIVE_BROWSER_APP,
        apps::DRIVE_PREVIEW_APP,
        apps::DRIVE_SHARING_APP,
        apps::DRIVE_CONFLICTS_APP,
        apps::DRIVE_RETENTION_APP,
    ] {
        assert!(
            server
                .resolve_resource_uri(&format!("ui://repo/mcp/apps/{app}"))
                .is_some(),
            "{app} resolves"
        );
        let shown = server
            .mcp
            .read_mcp_app_show("repo", app)
            .unwrap()
            .expect("internal drive app show result");
        assert_eq!(shown.uri, format!("ui://repo/mcp/apps/{app}"));
    }
    let browser_target = server
        .resolve_resource_uri("ui://repo/mcp/apps/drive-browser/folder/folder-1")
        .expect("drive browser instance resolves");
    match server.read_target(&browser_target).unwrap() {
        ResourceContents::TextResourceContents { uri, text, .. } => {
            assert_eq!(uri, "ui://repo/mcp/apps/drive-browser/folder/folder-1");
            assert!(text.contains("\"app_id\":\"drive-browser\""));
            assert!(text.contains("\"folder_id\":\"folder-1\""));
            assert!(text.contains("Plan.txt"));
            assert!(text.contains("conflicted copy"));
            assert!(text.contains("drive_create_upload"));
            assert!(!text.contains("drive_hydrate"));
        }
        other => panic!("expected text drive app contents, got {other:?}"),
    }
    for (app, tool) in [
        (apps::DRIVE_CONFLICTS_APP, "drive_resolve_conflict"),
        (apps::DRIVE_SHARING_APP, "drive_grant_share"),
        (apps::DRIVE_RETENTION_APP, "drive_pin_retention"),
    ] {
        let target = server
            .resolve_resource_uri(&format!("ui://repo/mcp/apps/{app}"))
            .expect("drive management app resolves");
        match server.read_target(&target).unwrap() {
            ResourceContents::TextResourceContents { text, .. } => {
                assert!(text.contains(tool), "{app} exposes {tool}");
            }
            other => panic!("expected text drive management app contents, got {other:?}"),
        }
    }
    let preview_target = server
        .resolve_resource_uri("ui://repo/mcp/apps/drive-preview/file/file-1")
        .expect("drive preview instance resolves");
    match server.read_target(&preview_target).unwrap() {
        ResourceContents::TextResourceContents { uri, text, .. } => {
            assert_eq!(uri, "ui://repo/mcp/apps/drive-preview/file/file-1");
            assert!(text.contains("\"selected_file\":{\"file_id\":\"file-1\"}"));
            assert!(text.contains("\"versions\":["));
            assert!(text.contains("\"file_bytes\":["));
        }
        other => panic!("expected text drive preview contents, got {other:?}"),
    }
    let (tool, args) = server
        .prepare_app_tool_call(PAppCallTool {
            app_uri: "ui://repo/mcp/apps/drive-browser/folder/folder-1".to_string(),
            tool: "drive_create_folder".to_string(),
            arguments: Some(serde_json::Map::from_iter([
                ("workspace".to_string(), json!("repo")),
                ("parent_folder_id".to_string(), json!("folder-1")),
                ("folder_id".to_string(), json!("folder-2")),
                ("name".to_string(), json!("Design")),
                (
                    "expected_root".to_string(),
                    json!(committed.profile_root.clone()),
                ),
            ])),
        })
        .expect("drive create folder action prepares");
    assert_eq!(tool, "drive_create_folder");
    assert_eq!(args.expect("drive args")["folder_id"], json!("folder-2"));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn meetings_apps_render_source_backed_meeting_state() {
    use loom_core::Digest;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-meetings-app-instance-{}.loom",
        std::process::id()
    ));
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Vcs,
                Some("repo"),
                WorkspaceId::v4_from_bytes([87u8; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let mcp = Arc::new(LoomMcp::new(StoreAccess::per_request(&path, None)));
    let source_digest = Digest::hash(loom_core::Algo::Blake3, b"meeting-source").to_string();
    let input = json!({
        "snapshot_version": 1,
        "profile": "granola-mcp",
        "source_system": "granola-mcp",
        "source_scope": "assistant-session",
        "observed_at": 700,
        "coverage": "partial",
        "items": [{
            "source_entity_id": "note-1",
            "source_digest": source_digest,
            "source_sidecar": {"id": "note-1", "via": "mcp"},
            "title": "Architecture review",
            "summary_text": "Use normalized snapshots.",
            "transcript_spans": [{"text": "The importer writes Meetings records."}],
            "decisions": [{"label": "Keep source payloads."}],
            "tasks": [{"label": "Create the follow-up ticket."}],
            "questions": [{"label": "Who owns rollout?"}],
            "topics": [{"label": "Import coverage"}],
            "artifacts": [{"label": "Design recording"}],
            "references": [{"label": "Related roadmap page"}]
        }]
    });
    let bytes = serde_json::to_vec(&input).unwrap();
    mcp.write_meetings_import_snapshot("repo", "granola-mcp", &bytes, false)
        .expect("import meetings snapshot");

    let server = LoomServer::new(mcp);
    for app in [
        apps::MEETING_DETAILS_APP,
        apps::MEMORY_GRAPH_APP,
        apps::EXTRACTION_REVIEW_APP,
        apps::MEETING_SEARCH_APP,
        apps::IMPORT_COVERAGE_APP,
        apps::ACCESS_AUDIT_APP,
    ] {
        assert!(
            server
                .resolve_resource_uri(&format!("ui://repo/mcp/apps/{app}"))
                .is_some(),
            "{app} resolves"
        );
        let shown = server
            .mcp
            .read_mcp_app_show("repo", app)
            .unwrap()
            .expect("internal meetings app show result");
        assert_eq!(shown.uri, format!("ui://repo/mcp/apps/{app}"));
    }
    let detail_target = server
        .resolve_resource_uri("ui://repo/mcp/apps/meeting-details/meeting/meeting/note-1")
        .expect("meeting details instance resolves");
    match server.read_target(&detail_target).unwrap() {
        ResourceContents::TextResourceContents { uri, text, .. } => {
            assert_eq!(
                uri,
                "ui://repo/mcp/apps/meeting-details/meeting/meeting/note-1"
            );
            assert!(text.contains("\"app_id\":\"meeting-details\""));
            assert!(text.contains("\"meeting_id\":\"meeting/note-1\""));
            assert!(text.contains("Architecture review"));
            assert!(text.contains("Keep source payloads."));
            assert!(text.contains("meetings_accept_annotation"));
            assert!(text.contains("meetings_promote_task_to_ticket"));
            assert!(!text.contains("meetings.redact_span"));
        }
        other => panic!("expected text meetings app contents, got {other:?}"),
    }
    for (app, expected) in [
        (apps::MEMORY_GRAPH_APP, "meeting/note-1"),
        (apps::EXTRACTION_REVIEW_APP, "meetings_propose_vocabulary"),
        (apps::MEETING_SEARCH_APP, "meetings_search"),
        (apps::IMPORT_COVERAGE_APP, "meetings_import_snapshot"),
        (apps::ACCESS_AUDIT_APP, "shared Studio ACL"),
    ] {
        let target = server
            .resolve_resource_uri(&format!("ui://repo/mcp/apps/{app}"))
            .expect("meetings app resolves");
        match server.read_target(&target).unwrap() {
            ResourceContents::TextResourceContents { text, .. } => {
                assert!(text.contains(expected), "{app} exposes {expected}");
            }
            other => panic!("expected text meetings app contents, got {other:?}"),
        }
    }
    let (tool, args) = server
        .prepare_app_tool_call(PAppCallTool {
            app_uri: "ui://repo/mcp/apps/meeting-details/meeting/meeting/note-1".to_string(),
            tool: "meetings_accept_annotation".to_string(),
            arguments: Some(serde_json::Map::from_iter([
                ("workspace".to_string(), json!("repo")),
                ("annotation_id".to_string(), json!("decision/note-1/0")),
            ])),
        })
        .expect("meetings accept action prepares");
    assert_eq!(tool, "meetings_accept_annotation");
    assert_eq!(
        args.expect("meetings args")["annotation_id"],
        json!("decision/note-1/0")
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn apps_call_tool_validates_app_context_and_inner_tool() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-app-call-tool-{}-{}.loom",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([9u8; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        &path, None,
    ))));
    let (tool, args) = server
        .prepare_app_tool_call(PAppCallTool {
            app_uri: "ui://repo/mcp/apps/directed-graph".to_string(),
            tool: "workspace_list".to_string(),
            arguments: Some(serde_json::Map::new()),
        })
        .expect("app tool call prepares");
    assert_eq!(tool, "workspace_list");
    assert_eq!(args, Some(serde_json::Map::new()));

    let err = server
        .prepare_app_tool_call(PAppCallTool {
            app_uri: "ui://repo/mcp/apps/directed-graph".to_string(),
            tool: "apps_call_tool".to_string(),
            arguments: None,
        })
        .expect_err("recursive app bridge call is rejected");
    assert!(
        err.to_string().contains("app-only tool apps_call_tool"),
        "unexpected error: {err}"
    );
    let err = server
        .prepare_app_tool_call(PAppCallTool {
            app_uri: "ui://repo/mcp/apps/missing".to_string(),
            tool: "workspace_list".to_string(),
            arguments: None,
        })
        .expect_err("missing app is rejected");
    assert!(
        err.to_string().contains("is not visible"),
        "unexpected error: {err}"
    );
    let _ = std::fs::remove_file(&path);
}

/// Subscriptions: the ETag-diff change detector reports a resource the first time its ETag is
/// established, stays quiet when unchanged, and re-reports when the stored ETag is stale.
#[test]
fn subscription_change_detection() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom, cas_put};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!("loom-mcp-sub-{}.loom", std::process::id()));
    let digest = {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Cas,
                Some("blobs"),
                WorkspaceId::v4_from_bytes([8u8; 16]),
            )
            .unwrap();
        let d = cas_put(&mut loom, ns, b"data").unwrap().to_string();
        save_loom(&mut loom).unwrap();
        d
    };
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        &path, None,
    ))));
    let uri = format!("loom://blobs/cas/{digest}");
    // Subscribe with an unknown last-seen ETag -> first poll reports it, second is quiet.
    server
        .subscriptions
        .lock()
        .unwrap()
        .insert(uri.clone(), None);
    assert_eq!(server.compute_changed(), vec![uri.clone()]);
    assert!(server.compute_changed().is_empty());
    // A stale stored ETag is detected as changed again.
    server
        .subscriptions
        .lock()
        .unwrap()
        .insert(uri.clone(), Some("blake3:stale".into()));
    assert_eq!(server.compute_changed(), vec![uri]);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn app_subscription_advances_watch_cursor_on_committed_change() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!("loom-mcp-app-watch-{}.loom", std::process::id()));
    let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
    let mut loom = Loom::new(store);
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("repo"),
            WorkspaceId::v4_from_bytes([14u8; 16]),
        )
        .unwrap();
    loom.write_file(ns, "a.txt", b"a", 0o644).unwrap();
    loom.commit(ns, "seed", "c0", 0).unwrap();

    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::persistent(loom))));
    let uri = "ui://repo/mcp/apps/internal/vcs".to_string();
    let target = server
        .resolve_resource_uri(&uri)
        .expect("internal app uri resolves");
    server
        .subscribe_resource(uri.clone(), &target)
        .expect("subscribe app resource");
    let initial_cursor = server
        .app_watches
        .lock()
        .unwrap()
        .get(&uri)
        .cloned()
        .expect("app subscription has a watch cursor");
    assert!(
        server
            .mcp
            .read_watch_poll("repo", &initial_cursor, 10)
            .unwrap()
            .events
            .is_empty()
    );
    assert!(server.compute_changed().is_empty());

    server
        .mcp
        .store()
        .write(|loom| {
            loom.write_file(ns, "b.txt", b"b", 0o644)?;
            loom.commit(ns, "seed", "c1", 1)?;
            Ok(())
        })
        .unwrap();

    let wakeup_batch = server
        .mcp
        .read_watch_poll("repo", &initial_cursor, 10)
        .unwrap();
    assert_eq!(wakeup_batch.events.len(), 1);
    assert_eq!(wakeup_batch.events[0].changes.len(), 1);
    assert_eq!(wakeup_batch.events[0].changes[0].domain, "files");
    assert_eq!(wakeup_batch.events[0].changes[0].kind, "added");
    assert_eq!(wakeup_batch.events[0].unsupported_domains.len(), 0);

    assert_eq!(server.compute_changed(), vec![uri.clone()]);
    let advanced_cursor = server
        .app_watches
        .lock()
        .unwrap()
        .get(&uri)
        .cloned()
        .expect("app watch cursor advanced");
    assert_ne!(advanced_cursor, initial_cursor);
    assert!(
        server
            .mcp
            .read_watch_poll("repo", &advanced_cursor, 10)
            .unwrap()
            .events
            .is_empty()
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn app_subscription_records_delivery_until_ack() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::FileStore;

    let path =
        std::env::temp_dir().join(format!("loom-mcp-app-delivery-{}.loom", std::process::id()));
    let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
    let mut loom = Loom::new(store);
    let ns = loom
        .registry_mut()
        .create(
            FacetKind::Files,
            Some("repo"),
            WorkspaceId::v4_from_bytes([15u8; 16]),
        )
        .unwrap();
    loom.write_file(ns, "a.txt", b"a", 0o644).unwrap();
    loom.commit(ns, "seed", "c0", 0).unwrap();

    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::persistent(loom))));
    let uri = "ui://repo/mcp/apps/internal/vcs".to_string();
    let target = server
        .resolve_resource_uri(&uri)
        .expect("internal app uri resolves");
    server
        .subscribe_resource(uri.clone(), &target)
        .expect("subscribe app resource");
    assert!(server.compute_changed().is_empty());

    server
        .mcp
        .store()
        .write(|loom| {
            loom.write_file(ns, "b.txt", b"b", 0o644)?;
            loom.commit(ns, "seed", "c1", 1)?;
            Ok(())
        })
        .unwrap();

    assert_eq!(server.compute_changed(), vec![uri.clone()]);
    let stream_id = delivery::DeliveryState::app_stream_id(&uri);
    let replay = server
        .delivery_replay(&stream_id, "client", None, true, 10)
        .unwrap();
    assert_eq!(replay.events.len(), 1);
    assert_eq!(replay.events[0].stream_id, stream_id);
    assert_eq!(replay.events[0].subject, uri);
    assert!(replay.events[0].source_cursor.is_some());
    assert_eq!(replay.events[0].payload["type"], "resource.updated");
    let event_seq = replay.events[0].seq;
    let event_id = replay.events[0].id.clone();

    let redelivery = server
        .delivery_replay(&stream_id, "client", None, true, 10)
        .unwrap();
    assert_eq!(redelivery.events.len(), 1);
    assert_eq!(redelivery.events[0].id, event_id);

    assert_eq!(
        server.delivery_ack(&stream_id, "client", event_seq),
        event_seq
    );
    let after_ack = server
        .delivery_replay(&stream_id, "client", None, true, 10)
        .unwrap();
    assert!(after_ack.events.is_empty());

    let _ = std::fs::remove_file(&path);
}

#[test]
fn app_delivery_retention_can_be_configured() {
    let server = LoomServer::with_delivery_retention(
        Arc::new(LoomMcp::new(StoreAccess::per_request(
            "/nonexistent.loom",
            None,
        ))),
        delivery::DeliveryRetention {
            max_age_ms: 7 * 24 * 60 * 60 * 1000,
            max_events_per_stream: 100_000,
            max_bytes_per_stream: 512 * 1024 * 1024,
        },
    );
    assert_eq!(
        server.delivery_retention(),
        delivery::DeliveryRetention {
            max_age_ms: 7 * 24 * 60 * 60 * 1000,
            max_events_per_stream: 100_000,
            max_bytes_per_stream: 512 * 1024 * 1024,
        }
    );
}

#[test]
fn app_resource_list_change_detection() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!("loom-mcp-app-list-{}.loom", std::process::id()));
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([9u8; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }

    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        &path, None,
    ))));
    assert!(!server.compute_resource_list_changed());

    server
        .mcp
        .write_mcp_app_create(
            "repo",
            "panel",
            b"<!doctype html><html><body>Panel</body></html>",
            b"---\nname: Panel\n---\n",
        )
        .unwrap();
    assert!(server.compute_resource_list_changed());
    assert!(!server.compute_resource_list_changed());

    server
        .mcp
        .write_mcp_app_write_file(
            "repo",
            "panel",
            "index.html",
            b"<!doctype html><html><body>Panel v2</body></html>",
            0o100644,
        )
        .unwrap();
    assert!(!server.compute_resource_list_changed());

    server
        .mcp
        .write_mcp_app_remove_file("repo", "panel", "index.html")
        .unwrap();
    assert!(server.compute_resource_list_changed());
    assert!(!server.compute_resource_list_changed());

    server
        .mcp
        .write_mcp_app_write_file(
            "repo",
            "panel",
            "_meta.md",
            b"---\nunknown: true\n---\n",
            0o100644,
        )
        .unwrap();
    assert!(!server.compute_resource_list_changed());

    let _ = std::fs::remove_file(&path);
}

#[test]
fn list_inventory_change_detection_uses_store_token() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path =
        std::env::temp_dir().join(format!("loom-mcp-list-token-{}.loom", std::process::id()));
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([10u8; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }

    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        &path, None,
    ))));
    let resources = server.list_all_resources().unwrap();
    server.record_resource_list_fingerprint(&resources);
    assert!(!server.list_inventories_may_have_changed());

    server
        .mcp
        .write_mcp_app_create(
            "repo",
            "panel",
            b"<!doctype html><html><body>Panel</body></html>",
            b"---\nname: Panel\n---\n",
        )
        .unwrap();
    assert!(server.list_inventories_may_have_changed());
    assert!(!server.list_inventories_may_have_changed());

    let _ = std::fs::remove_file(&path);
}

#[test]
fn subscription_presence_tracks_content_update_polling() {
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        "/nonexistent.loom",
        None,
    ))));
    assert!(!server.has_subscriptions());

    server
        .subscriptions
        .lock()
        .unwrap()
        .insert("loom://repo/file.txt".to_string(), None);
    assert!(server.has_subscriptions());

    server.subscriptions.lock().unwrap().clear();
    assert!(!server.has_subscriptions());
}

/// Pagination: the opaque cursor walks the list in PAGE_SIZE windows and ends with `None`; bad
/// cursors are rejected.
#[test]
fn paginate_cursors() {
    let items: Vec<u32> = (0..250).collect();
    let (p0, c0) = LoomServer::paginate(items.clone(), None).unwrap();
    assert_eq!(p0.len(), 100);
    assert_eq!(p0[0], 0);
    assert_eq!(c0.as_deref(), Some("100"));
    let (p1, c1) = LoomServer::paginate(items.clone(), c0).unwrap();
    assert_eq!(p1[0], 100);
    assert_eq!(c1.as_deref(), Some("200"));
    let (p2, c2) = LoomServer::paginate(items.clone(), c1).unwrap();
    assert_eq!(p2.len(), 50);
    assert_eq!(p2[0], 200);
    assert_eq!(c2, None);
    // Out-of-range and unparsable cursors are invalid params.
    assert!(LoomServer::paginate(items.clone(), Some("999".into())).is_err());
    assert!(LoomServer::paginate(items, Some("nope".into())).is_err());
}

/// Completion: the `workspace` argument prefix-filters live workspace names; other arguments are
/// empty.
#[test]
fn complete_workspace_prefix() {
    use loom_core::Loom;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!("loom-mcp-complete-{}.loom", std::process::id()));
    {
        let store = FileStore::create_with_profile(&path, loom_core::Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        for (i, name) in ["blue", "black", "red"].iter().enumerate() {
            loom.registry_mut()
                .create(
                    FacetKind::Kv,
                    Some(name),
                    WorkspaceId::v4_from_bytes([i as u8 + 1; 16]),
                )
                .unwrap();
        }
        save_loom(&mut loom).unwrap();
    }
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        &path, None,
    ))));
    let ns_arg = ArgumentInfo {
        name: "workspace".into(),
        value: "bl".into(),
    };
    let mut got = server
        .complete_argument(&Reference::for_resource("loom://"), &ns_arg)
        .unwrap();
    got.sort();
    assert_eq!(got, vec!["black".to_string(), "blue".to_string()]);
    // A non-workspace argument has no enumerable domain.
    let other = ArgumentInfo {
        name: "path".into(),
        value: String::new(),
    };
    assert!(
        server
            .complete_argument(&Reference::for_prompt("kv"), &other)
            .unwrap()
            .is_empty()
    );
    let _ = std::fs::remove_file(&path);
}

/// Utilities: the cancelled-request error carries the JSON-RPC -32800 code, and the progress
/// bracket reports `0/1` at the start and `1/1` on completion.
#[test]
fn progress_and_cancellation_helpers() {
    use rmcp::model::NumberOrString;

    let e = cancelled_error("sql_exec");
    assert_eq!(e.code, ErrorCode(-32800));
    assert!(e.message.contains("sql_exec"));

    let token = ProgressToken(NumberOrString::Number(7));
    let start = progress_param(token.clone(), false, "sql_exec");
    assert_eq!(start.progress, 0.0);
    assert_eq!(start.total, Some(1.0));
    assert_eq!(start.message.as_deref(), Some("sql_exec: started"));
    let done = progress_param(token, true, "sql_exec");
    assert_eq!(done.progress, 1.0);
    assert_eq!(done.message.as_deref(), Some("sql_exec: completed"));
}

/// Destructive annotations are exposed for the host approval layer. The MCP server does not run a
/// second in-band confirmation gate before dispatch.
#[test]
fn destructive_tool_recognition() {
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        "/nonexistent.loom",
        None,
    ))));
    assert!(server.is_destructive_tool("cas_delete"));
    assert!(server.is_destructive_tool("fs_remove_file"));
    assert!(!server.is_destructive_tool("kv_put")); // write, but not destructive
    assert!(!server.is_destructive_tool("cas_get")); // read
    assert!(!server.is_destructive_tool("nope.missing")); // unknown -> not destructive
}

/// Capability drift: `get_info` advertises exactly the MCP primitives this host serves: tools,
/// prompts, resources (with subscribe + listChanged), completions, and the MCP UI extension subset.
#[test]
fn capabilities_advertise_host_primitives() {
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        "/nonexistent.loom",
        None,
    ))));
    let info = server.get_info();
    assert_eq!(info.server_info.name, "loom");
    assert_eq!(info.server_info.title.as_deref(), Some("Loom MCP"));
    assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    let caps = info.capabilities;
    assert!(caps.tools.is_some(), "tools advertised");
    assert!(caps.prompts.is_some(), "prompts advertised");
    assert!(caps.completions.is_some(), "completions advertised");
    let ui = caps
        .extensions
        .expect("extensions advertised")
        .remove(MCP_UI_EXTENSION)
        .expect("MCP UI extension advertised");
    assert_eq!(ui["htmlResources"], json!(true));
    assert_eq!(ui["toolResourceUri"], json!(true));
    assert_eq!(ui["iframeJsonRpcBridgeRequired"], json!(false));
    let resources = caps.resources.expect("resources advertised");
    assert_eq!(resources.subscribe, Some(true));
    assert_eq!(resources.list_changed, Some(true));
}

/// Behavioral conformance: exercise the served read path end to end against a seeded loom through
/// the same methods the `ServerHandler` resource/completion handlers delegate to (the transport
/// plumbing is covered by a dedicated smoke test and rmcp's own conformance). Seeds a CAS blob, then
/// lists workspace resources, reads the blob back content-addressed, completes the workspace
/// argument, paginates the template catalog, and confirms destructive tools remain annotated for the
/// host approval layer.
#[test]
fn behavioral_conformance_read_path() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom, cas_put};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!("loom-mcp-e2e-{}.loom", std::process::id()));
    let digest = {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Cas,
                Some("blobs"),
                WorkspaceId::v4_from_bytes([9u8; 16]),
            )
            .unwrap();
        let d = cas_put(&mut loom, ns, b"hello").unwrap().to_string();
        save_loom(&mut loom).unwrap();
        d
    };
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        &path, None,
    ))));

    // Resource listing surfaces the seeded workspace.
    let resources = server.list_workspace_resources().unwrap();
    assert!(resources.iter().any(|r| r.uri == "loom://blobs/"));

    // Reading the blob resource returns its content-addressed bytes (version == digest).
    let target = resources::parse_uri(&format!("loom://blobs/cas/{digest}")).unwrap();
    let contents = server.read_target(&target).unwrap();
    match contents {
        ResourceContents::BlobResourceContents { blob, .. } => assert!(!blob.is_empty()),
        other => panic!("expected blob contents, got {other:?}"),
    }
    assert_eq!(
        server.resource_etag(&format!("loom://blobs/cas/{digest}")),
        Some(digest)
    );

    // Completion resolves the workspace argument; pagination yields a non-empty template page.
    let vals = server
        .complete_argument(
            &Reference::for_resource("loom://{workspace}/"),
            &ArgumentInfo {
                name: "workspace".into(),
                value: "bl".into(),
            },
        )
        .unwrap();
    assert!(vals.iter().any(|v| v == "blobs"));
    let (page, _next) = LoomServer::paginate(server.resource_templates(), None).unwrap();
    assert!(!page.is_empty());

    // The destructive annotation remains available to the host approval layer.
    assert!(server.is_destructive_tool("cas_delete"));

    let _ = std::fs::remove_file(&path);
}

/// The Streamable HTTP service constructs over the engine facade (owner mode), in both the stateful
/// (session) and stateless (POST-only, per-request) configurations.
#[cfg(feature = "http")]
#[test]
fn http_service_constructs() {
    let stateful_mcp = Arc::new(LoomMcp::new(StoreAccess::per_request(
        "/nonexistent.loom",
        None,
    )));
    let _stateful = super::http_service(stateful_mcp, Binding::default(), true);
    let stateless_mcp = Arc::new(LoomMcp::new(StoreAccess::per_request(
        "/nonexistent.loom",
        None,
    )));
    let _stateless = super::http_service(stateless_mcp, Binding::default(), false);
}

/// Scoping: a workspace+collection binding elides `workspace` and the collection-axis param from a
/// tool's schema, and `principal` is always elided from a per-principal area.
#[test]
fn binding_elides_scoped_params() {
    use rmcp::handler::server::router::tool::ToolRouter;
    let mut router: ToolRouter<LoomServer> = LoomServer::tool_router();
    enrich_metadata(&mut router);
    apply_binding(&mut router, &Binding::collection("work", "sessions"));
    let props = |t: &str| -> Vec<String> {
        let tool = router.get(t).unwrap();
        tool.input_schema
            .get("properties")
            .and_then(|p| p.as_object())
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default()
    };
    // kv_get: workspace + collection elided, key remains.
    let kv = props("kv_get");
    assert!(!kv.contains(&"workspace".to_string()));
    assert!(!kv.contains(&"collection".to_string()));
    assert!(kv.contains(&"key".to_string()));
    // watch: workspace is elided, cursor and max stay visible.
    let watch = props("watch_poll");
    assert!(!watch.contains(&"workspace".to_string()));
    assert!(watch.contains(&"cursor".to_string()));
    assert!(watch.contains(&"max".to_string()));
    let watch_subscribe = props("watch_subscribe");
    assert!(!watch_subscribe.contains(&"workspace".to_string()));
    assert!(watch_subscribe.contains(&"branch".to_string()));
    assert!(watch_subscribe.contains(&"change_kinds".to_string()));
    // mail: principal always elided; mailbox is the collection axis, elided when bound.
    let mail = props("mail_get_message");
    assert!(!mail.contains(&"principal".to_string()));
    assert!(!mail.contains(&"mailbox".to_string()));
    let calendar = props("calendar_put_entry");
    assert!(!calendar.contains(&"workspace".to_string()));
    assert!(!calendar.contains(&"principal".to_string()));
    assert!(!calendar.contains(&"collection".to_string()));
    assert!(calendar.contains(&"entry".to_string()));
    let contacts = props("contacts_get_entry");
    assert!(!contacts.contains(&"workspace".to_string()));
    assert!(!contacts.contains(&"principal".to_string()));
    assert!(!contacts.contains(&"book".to_string()));
    assert!(contacts.contains(&"uid".to_string()));
    let mail_flags = props("mail_set_flags");
    assert!(!mail_flags.contains(&"workspace".to_string()));
    assert!(!mail_flags.contains(&"principal".to_string()));
    assert!(!mail_flags.contains(&"mailbox".to_string()));
    assert!(mail_flags.contains(&"flags".to_string()));
    // sql collection axis is `db`.
    let sql = props("sql_read_table");
    assert!(!sql.contains(&"workspace".to_string()));
    assert!(!sql.contains(&"db".to_string()));
    assert!(sql.contains(&"table".to_string()));
    for tool in router.list_all() {
        assert!(
            tool.output_schema.is_some(),
            "{} lost output schema after binding",
            tool.name
        );
    }
}

#[test]
fn pim_binding_injects_and_overwrites_agent_scope() {
    let mut binding = Binding::collection("work", "sessions");
    binding.principal = "alice".to_string();
    let server = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::per_request(
            "/nonexistent.loom",
            None,
        ))),
        binding,
    );
    for (tool, collection_param) in [
        ("calendar_get_entry", "collection"),
        ("contacts_get_entry", "book"),
        ("mail_get_message", "mailbox"),
    ] {
        let mut args = JsonObject::new();
        args.insert("workspace".to_string(), json!("other"));
        args.insert("principal".to_string(), json!("bob"));
        args.insert(collection_param.to_string(), json!("foreign"));
        args.insert("uid".to_string(), json!("id-1"));
        let mut request = CallToolRequestParams::new(tool).with_arguments(args);

        server.inject_binding(&mut request);

        let args = request.arguments.expect("arguments");
        assert_eq!(args["workspace"], json!("work"));
        assert_eq!(args["principal"], json!("alice"));
        assert_eq!(args[collection_param], json!("sessions"));
        assert_eq!(args["uid"], json!("id-1"));
    }
}

#[test]
fn pim_arguments_reject_missing_or_malformed_scope_fields() {
    assert!(
        serde_json::from_value::<PCalPutEntry>(json!({
            "workspace": "cal",
            "principal": "alice",
            "collection": "work",
            "entry": "not bytes"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<PCardBook>(json!({
            "workspace": "contacts",
            "book": "personal"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<PMailSetFlags>(json!({
            "workspace": "mail",
            "principal": "alice",
            "mailbox": "inbox",
            "uid": "m1",
            "flags": "seen"
        }))
        .is_err()
    );
}

#[test]
fn tool_results_use_schema_envelope() {
    use rmcp::handler::server::tool::IntoCallToolResult;

    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::per_request(
        "/nonexistent.loom",
        None,
    ))));
    let result = server.store_version().unwrap();
    let call = result.into_call_tool_result().unwrap();
    let structured = call.structured_content.expect("structured content");
    assert!(structured.get("value").is_some());
}

#[test]
fn binding_scopes_resource_templates_and_reads() {
    let server = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::per_request(
            "/nonexistent.loom",
            None,
        ))),
        Binding::collection("work", "sessions"),
    );
    let templates: BTreeSet<String> = server
        .resource_templates()
        .into_iter()
        .map(|t| t.uri_template.clone())
        .collect();
    assert!(templates.contains("loom://files/{path}"));
    assert!(templates.contains("loom://calendar/{uid}.ics"));
    assert!(templates.contains("loom://studio/views/status"));
    assert!(templates.contains("loom://substrate/views/{view_id}.json"));
    assert!(templates.contains("loom://substrate/refs/{target}.json"));
    assert!(!templates.iter().any(|t| t.contains("{workspace}")));
    assert!(!templates.iter().any(|t| t.contains("{principal}")));
    assert!(!templates.iter().any(|t| t.contains("{collection}")));

    assert_eq!(
        server.resolve_resource_uri("loom://"),
        Some(ResourceTarget::Workspace {
            workspace: "work".into()
        })
    );
    assert_eq!(
        server.resolve_resource_uri("loom://work/"),
        Some(ResourceTarget::Workspace {
            workspace: "work".into()
        })
    );
    assert_eq!(
        server.resolve_resource_uri("loom://files/a.txt"),
        Some(ResourceTarget::File {
            workspace: "work".into(),
            path: "a.txt".into()
        })
    );
    assert_eq!(
        server.resolve_resource_uri("loom://calendar/e1.ics"),
        Some(ResourceTarget::CalendarIcs {
            workspace: "work".into(),
            principal: "owner".into(),
            collection: "sessions".into(),
            uid: "e1".into()
        })
    );
    assert_eq!(
        server.resolve_resource_uri("loom://studio/views/status"),
        Some(ResourceTarget::StudioStatus {
            workspace: "work".into(),
            principal: "owner".into(),
        })
    );
    assert_eq!(
        server.resolve_resource_uri("loom://substrate/views/status.json"),
        Some(ResourceTarget::SubstrateView {
            workspace: "work".into(),
            view_id: "status".into(),
        })
    );
    assert_eq!(
        server.resolve_resource_uri("loom://substrate/refs/ticket:42.json"),
        Some(ResourceTarget::SubstrateRefs {
            workspace: "work".into(),
            target: "ticket:42".into(),
        })
    );
    assert_eq!(
        server.resolve_resource_uri("loom://work/studio/views/status/principal/owner"),
        Some(ResourceTarget::StudioStatus {
            workspace: "work".into(),
            principal: "owner".into(),
        })
    );
    assert!(
        server
            .resolve_resource_uri("loom://other/files/a.txt")
            .is_none()
    );
    assert!(
        server
            .resolve_resource_uri("loom://calendar/other/e1.ics")
            .is_none()
    );
    assert!(
        server
            .resolve_resource_uri("loom://contacts/other/c1.vcf")
            .is_none()
    );
    assert!(
        server
            .resolve_resource_uri("loom://mail/other/m1.eml")
            .is_none()
    );
    assert!(
        server
            .resolve_resource_uri("loom://work/studio/views/status/principal/bob")
            .is_none()
    );
    assert!(server.resolve_resource_uri("loom://calendar/e1").is_none());
    assert!(
        server
            .resolve_resource_uri("loom://contacts/c1.txt")
            .is_none()
    );
    assert!(
        server
            .resolve_resource_uri("loom://mail/folder/m1.eml")
            .is_none()
    );
}

#[test]
fn pim_resource_reads_project_domain_bodies() {
    use loom_core::contacts::ContactEntry;
    use loom_core::{Algo, CalendarEntry, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-pim-resources-{}.loom",
        std::process::id()
    ));
    let mcp = Arc::new(LoomMcp::new(StoreAccess::persistent(Loom::new(
        FileStore::create_with_profile(&path, Algo::Blake3).unwrap(),
    ))));
    mcp.write_workspace_create(Some("cal"), "calendar")
        .expect("calendar workspace");
    mcp.write_workspace_create(Some("cards"), "contacts")
        .expect("contacts workspace");
    mcp.write_workspace_create(Some("mail"), "mail")
        .expect("mail workspace");
    mcp.write_calendar_create_collection("cal", "alice", "work", "Work", "event")
        .expect("calendar collection");
    mcp.write_contacts_create_book("cards", "alice", "personal", "Personal")
        .expect("contact book");
    mcp.write_mail_create_mailbox("mail", "alice", "inbox", "Inbox")
        .expect("mailbox");
    let event = CalendarEntry::event("event-1", "Standup", "20260101T090000");
    mcp.write_calendar_put_entry("cal", "alice", "work", &event.encode())
        .expect("calendar entry");
    let contact = ContactEntry::new("contact-1", "Ada Lovelace");
    mcp.write_contacts_put_entry("cards", "alice", "personal", &contact.encode())
        .expect("contact entry");
    let vcard = concat!(
        "BEGIN:VCARD\r\n",
        "VERSION:4.0\r\n",
        "UID:contact-2\r\n",
        "FN:Grace Hopper\r\n",
        "EMAIL:grace@example.com\r\n",
        "END:VCARD\r\n",
    );
    mcp.write_contacts_put_vcard("cards", "alice", "personal", vcard)
        .expect("contact vcard");
    let raw = b"From: alice@example.com\r\nTo: bob@example.com\r\nSubject: Lunch\r\nMessage-ID: <m1@example.com>\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\n\r\nHello";
    mcp.write_mail_ingest_message("mail", "alice", "inbox", "m1", raw)
        .expect("mail message");

    let server = LoomServer::new(mcp);
    let calendar = server
        .read_target(&ResourceTarget::CalendarIcs {
            workspace: "cal".into(),
            principal: "alice".into(),
            collection: "work".into(),
            uid: "event-1".into(),
        })
        .expect("calendar body");
    match calendar {
        ResourceContents::TextResourceContents {
            mime_type, text, ..
        } => {
            assert_eq!(mime_type.as_deref(), Some("text/calendar"));
            assert!(text.contains("BEGIN:VCALENDAR"));
            assert!(text.contains("UID:event-1"));
            assert!(text.contains("SUMMARY:Standup"));
        }
        _ => panic!("calendar body should be text"),
    }
    let contact = server
        .read_target(&ResourceTarget::ContactsVcf {
            workspace: "cards".into(),
            principal: "alice".into(),
            book: "personal".into(),
            uid: "contact-2".into(),
        })
        .expect("contact body");
    match contact {
        ResourceContents::TextResourceContents {
            mime_type, text, ..
        } => {
            assert_eq!(mime_type.as_deref(), Some("text/vcard"));
            assert!(text.contains("BEGIN:VCARD"));
            assert!(text.contains("UID:contact-2"));
            assert!(text.contains("FN:Grace Hopper"));
        }
        _ => panic!("contact body should be text"),
    }
    let mail = server
        .read_target(&ResourceTarget::MailEml {
            workspace: "mail".into(),
            principal: "alice".into(),
            mailbox: "inbox".into(),
            uid: "m1".into(),
        })
        .expect("mail body");
    match mail {
        ResourceContents::BlobResourceContents {
            mime_type, blob, ..
        } => {
            assert_eq!(mime_type.as_deref(), Some("message/rfc822"));
            assert_eq!(blob, resources::base64_encode(raw));
        }
        _ => panic!("mail body should be blob"),
    }
    let _ = std::fs::remove_file(&path);
}

/// Workspace scoping injects the workspace argument only; it does not hide tool areas by facet.
/// Collection scoping additionally injects the collection-axis argument and drops collection discovery.
#[test]
fn binding_elides_scope_without_facet_narrowing() {
    use loom_core::Loom;
    use loom_core::kv::kv_put;
    use loom_core::tabular::Value;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!("loom-mcp-narrow-{}.loom", std::process::id()));
    let ns_id = WorkspaceId::v4_from_bytes([3u8; 16]);
    {
        let store = FileStore::create_with_profile(&path, loom_core::Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = loom
            .registry_mut()
            .create(FacetKind::Kv, Some("work"), ns_id)
            .unwrap();
        kv_put(
            &mut loom,
            ns,
            "sessions",
            Value::Text("k".to_string()),
            b"v".to_vec(),
        )
        .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let server = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::per_request(&path, None))),
        Binding::workspace("work"),
    );
    let names: Vec<String> = server
        .tool_router
        .list_all()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(
        !names.iter().any(|n| n == "workspace_list"),
        "workspace discovery hidden when workspace is bound"
    );
    assert!(names.iter().any(|n| n.starts_with("kv_")), "kv kept");
    assert!(names.iter().any(|n| n.starts_with("vcs_")), "vcs kept");
    assert!(names.iter().any(|n| n.starts_with("sql_")), "sql kept");
    assert!(
        names.iter().any(|n| n == "import_submit_batch"),
        "import kept"
    );
    assert!(
        names.iter().any(|n| n == "meetings_import_snapshot"),
        "meetings import kept"
    );
    assert!(
        names.iter().any(|n| n == "redmine_import_snapshot"),
        "redmine import kept"
    );
    assert!(
        names.iter().any(|n| n == "meetings_add_promotion"),
        "meetings promotion kept"
    );
    assert!(
        names.iter().any(|n| n.starts_with("document_")),
        "document kept"
    );
    let import_tool = server.tool_router.get("import_submit_batch").unwrap();
    assert!(
        !import_tool
            .input_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|props| props.contains_key("workspace")),
        "workspace is elided for import"
    );
    let meetings_import_tool = server.tool_router.get("meetings_import_snapshot").unwrap();
    assert!(
        !meetings_import_tool
            .input_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|props| props.contains_key("workspace")),
        "workspace is elided for meetings import"
    );
    let redmine_import_tool = server.tool_router.get("redmine_import_snapshot").unwrap();
    assert!(
        !redmine_import_tool
            .input_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|props| props.contains_key("workspace")),
        "workspace is elided for redmine import"
    );
    let meetings_promotion_tool = server.tool_router.get("meetings_add_promotion").unwrap();
    assert!(
        !meetings_promotion_tool
            .input_schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|props| props.contains_key("workspace")),
        "workspace is elided for meetings promotion"
    );
    let prompts: Vec<String> = server
        .prompt_router
        .list_all()
        .iter()
        .map(|p| p.name.to_string())
        .collect();
    assert!(
        prompts.iter().any(|n| n == "store_inventory"),
        "store prompt kept"
    );
    assert!(
        prompts.iter().any(|n| n.starts_with("vcs_")),
        "vcs prompts kept"
    );
    assert!(
        prompts.iter().any(|n| n.starts_with("sql_")),
        "sql prompts kept"
    );
    assert!(
        prompts.iter().any(|n| n.starts_with("mail_")),
        "mail prompts kept"
    );
    assert!(
        !server.resource_templates().is_empty(),
        "workspace scope keeps resource templates"
    );

    let scoped_by_id = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::per_request(&path, None))),
        Binding::workspace(ns_id.to_string()),
    );
    let by_id_names: Vec<String> = scoped_by_id
        .tool_router
        .list_all()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(by_id_names.iter().any(|n| n == "kv_get"), "uuid keeps kv");
    assert!(
        by_id_names.iter().any(|n| n.starts_with("document_")),
        "uuid keeps document"
    );
    assert_eq!(
        scoped_by_id.list_workspace_resources().unwrap().len(),
        1,
        "uuid binding lists the workspace"
    );

    let scoped = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::per_request(&path, None))),
        Binding::collection("work", "future"),
    );
    let scoped_names: Vec<String> = scoped
        .tool_router
        .list_all()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(
        scoped_names.iter().any(|n| n == "kv_put"),
        "new collection can be populated"
    );
    assert!(
        !scoped_names.iter().any(|n| n == "kv_list_collections"),
        "collection discovery dropped"
    );
    assert!(
        scoped_names.iter().any(|n| n.starts_with("document_")),
        "collection scope keeps document tools"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn tool_visibility_tracks_live_acl_changes() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{
        AclEffect, AclGrant, AclRight, AclScope, AclStore, AclSubject, Algo, IdentityStore, Loom,
        PrincipalKind,
    };
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!("loom-mcp-tools-acl-{}.loom", std::process::id()));
    let root = WorkspaceId::v4_from_bytes([1u8; 16]);
    let user = WorkspaceId::v4_from_bytes([2u8; 16]);
    let ns_id = WorkspaceId::v4_from_bytes([3u8; 16]);
    let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
    let mut loom = Loom::new(store);
    let ns = loom
        .registry_mut()
        .create(FacetKind::Files, Some("work"), ns_id)
        .unwrap();
    let mut identity = IdentityStore::new(root);
    identity
        .add_principal(user, "writer", PrincipalKind::User)
        .unwrap();
    identity.bind_session(user, "mcp-session").unwrap();
    loom.set_identity_store(identity);
    loom.set_session("mcp-session");
    let mut acl = AclStore::new();
    acl.grant(AclGrant {
        subject: AclSubject::Principal(user),
        workspace: Some(ns),
        domain: Some(FacetKind::Files.into()),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Read].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    })
    .unwrap();
    loom.set_acl_store(acl);
    let server = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::persistent(loom))),
        Binding::workspace("work"),
    );

    let visible = || -> BTreeSet<String> {
        server
            .list_regular_tools()
            .unwrap()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect()
    };
    assert!(visible().contains("fs_read_file"));
    assert!(!visible().contains("fs_write_file"));
    assert!(!server.compute_tool_list_changed());

    server
        .mcp
        .store()
        .write(|loom| {
            let mut acl = AclStore::new();
            acl.grant(AclGrant {
                subject: AclSubject::Principal(user),
                workspace: Some(ns),
                domain: Some(FacetKind::Files.into()),
                ref_glob: None,
                scopes: vec![AclScope::All],
                rights: [AclRight::Read, AclRight::Write].into_iter().collect(),
                effect: AclEffect::Allow,
                predicate: None,
            })?;
            loom.set_acl_store(acl);
            Ok(())
        })
        .unwrap();
    assert!(visible().contains("fs_write_file"));
    assert!(server.compute_tool_list_changed());
    assert!(!server.compute_tool_list_changed());

    server
        .mcp
        .store()
        .write(|loom| {
            let mut acl = AclStore::new();
            acl.grant(AclGrant {
                subject: AclSubject::Principal(user),
                workspace: Some(ns),
                domain: Some(FacetKind::Files.into()),
                ref_glob: None,
                scopes: vec![AclScope::All],
                rights: [AclRight::Read].into_iter().collect(),
                effect: AclEffect::Allow,
                predicate: None,
            })?;
            loom.set_acl_store(acl);
            Ok(())
        })
        .unwrap();
    assert!(!visible().contains("fs_write_file"));
    assert!(server.compute_tool_list_changed());

    let _ = std::fs::remove_file(&path);
}

#[test]
fn tool_visibility_isolates_product_authorization_domains() {
    use loom_core::workspace::{AclDomain, FacetKind, WorkspaceId};
    use loom_core::{
        AclEffect, AclGrant, AclRight, AclScope, AclStore, AclSubject, Algo, IdentityStore, Loom,
        PrincipalKind,
    };
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-tool-domain-isolation-{}.loom",
        std::process::id()
    ));
    let root = WorkspaceId::v4_from_bytes([31; 16]);
    let user = WorkspaceId::v4_from_bytes([32; 16]);
    let workspace = WorkspaceId::v4_from_bytes([33; 16]);
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Files, Some("work"), workspace)
        .unwrap();
    let mut identity = IdentityStore::new(root);
    identity
        .add_principal(user, "worker", PrincipalKind::User)
        .unwrap();
    identity.bind_session(user, "mcp-session").unwrap();
    loom.set_identity_store(identity);
    loom.set_session("mcp-session");
    let mut acl = AclStore::new();
    acl.grant(AclGrant {
        subject: AclSubject::Principal(user),
        workspace: Some(ns),
        domain: Some(AclDomain::Tickets),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Read].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    })
    .unwrap();
    loom.set_acl_store(acl);
    let server = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::persistent(loom))),
        Binding::workspace("work"),
    );
    let visible = server
        .list_regular_tools()
        .unwrap()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect::<BTreeSet<_>>();

    assert!(visible.contains("tickets_get"));
    assert!(visible.contains("lanes_get"));
    assert!(!visible.contains("fs_read_file"));
    assert!(!visible.contains("pages_get"));
    assert!(!visible.contains("chat_channels"));

    let _ = std::fs::remove_file(path);
}

#[test]
fn active_lifecycle_filters_visible_tools_and_changes_fingerprint() {
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Digest, Loom};
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-active-lifecycle-{}.loom",
        std::process::id()
    ));
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    loom.registry_mut()
        .create(
            FacetKind::Vcs,
            Some("work"),
            WorkspaceId::v4_from_bytes([21u8; 16]),
        )
        .unwrap();
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::persistent(loom))));
    let visible = || -> BTreeSet<String> {
        server
            .list_regular_tools()
            .unwrap()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect()
    };

    assert!(visible().contains("tickets_update"));
    server.record_tool_list_fingerprint();

    let predicate_digest = Digest::hash(Algo::Blake3, b"predicate").to_string();
    server
        .lifecycles_define_standard(Parameters(PLifecyclesDefineStandard {
            workspace: "work".to_string(),
            kind: "feature".to_string(),
            version: "v1".to_string(),
            completion_predicate_digest: predicate_digest,
        }))
        .unwrap();
    server
        .lifecycles_instantiate(Parameters(PLifecyclesInstantiate {
            workspace: "work".to_string(),
            instance_id: "feature-1".to_string(),
            definition_id: "feature".to_string(),
            subject_refs: Vec::new(),
        }))
        .unwrap();
    let active = server
        .lifecycles_active_set(Parameters(PLifecyclesActiveSet {
            workspace: "work".to_string(),
            instance_id: "feature-1".to_string(),
        }))
        .unwrap();
    assert_eq!(active.0["value"]["surface"]["stage_id"], json!("ideate"));
    assert!(server.compute_tool_list_changed());

    let active_visible = visible();
    assert!(active_visible.contains("pages_create"));
    assert!(active_visible.contains("lifecycles_active_clear"));
    assert!(active_visible.contains("store_version"));
    assert!(!active_visible.contains("tickets_update"));

    server.lifecycles_active_clear().unwrap();
    assert!(server.compute_tool_list_changed());
    assert!(visible().contains("tickets_update"));

    let _ = std::fs::remove_file(&path);
}

/// A trivial remote backend for gate tests; methods return deterministic values only when the
/// remote-forwarding tests intentionally exercise them.
struct GateTestBackend;

fn gate_backend_unexpected<T>(method: &str) -> std::result::Result<T, LoomError> {
    Err(LoomError::new(
        loom_core::error::Code::Unsupported,
        format!("GateTestBackend unexpected remote method call: {method}"),
    ))
}

fn gate_lane(lane_id: &str) -> loom_lanes::Lane {
    loom_lanes::Lane::new(loom_lanes::LaneInput {
        lane_id,
        lane_key: lane_id,
        title: "",
        description: "",
        lane_kind: loom_lanes::LaneKind::Assignment,
        owner_principal: Some("agent:3"),
        lane_status: loom_lanes::LaneStatus::Ready,
        lane_tickets: &[loom_lanes::LaneTicket {
            ticket_id: "MX-1".to_string(),
            order_key: "F".to_string(),
        }],
        active_ticket_id: Some("MX-1"),
        status_report: "ready",
        reviewer_feedback: "",
        updated_at: 1,
        updated_by: "agent:3",
    })
    .expect("valid gate lane")
}

impl crate::RemoteMcpBackend for GateTestBackend {
    fn workspace_create(
        &self,
        _: Option<&str>,
        _: Option<loom_core::FacetKind>,
    ) -> std::result::Result<String, LoomError> {
        gate_backend_unexpected("workspace_create")
    }
    fn execute_tool(
        &self,
        name: &str,
        args_json: &[u8],
    ) -> std::result::Result<Vec<u8>, LoomError> {
        match name {
            "fixture_tool" => {
                let args: serde_json::Value =
                    serde_json::from_slice(args_json).unwrap_or(serde_json::Value::Null);
                serde_json::to_vec(&serde_json::json!({ "echo": args }))
                    .map_err(|e| LoomError::new(loom_core::error::Code::Internal, e.to_string()))
            }
            other => Err(LoomError::new(
                loom_core::error::Code::Unsupported,
                format!("MCP tool {other} is not server-promoted"),
            )),
        }
    }

    fn lanes_create(
        &self,
        _: &str,
        lane: loom_lanes::Lane,
    ) -> std::result::Result<loom_lanes::Lane, LoomError> {
        Ok(lane)
    }

    fn lanes_get(
        &self,
        _: &str,
        lane_id: &str,
    ) -> std::result::Result<Option<loom_lanes::Lane>, LoomError> {
        Ok(Some(gate_lane(lane_id)))
    }

    fn lanes_list(&self, _: &str) -> std::result::Result<Vec<loom_lanes::Lane>, LoomError> {
        Ok(vec![gate_lane("remote")])
    }

    fn lanes_update(
        &self,
        _: &str,
        request: crate::RemoteLaneUpdate<'_>,
    ) -> std::result::Result<loom_lanes::Lane, LoomError> {
        let mut lane = gate_lane(request.lane_id);
        if let Some(title) = request.title {
            lane.title = title.to_string();
        }
        if let Some(description) = request.description {
            lane.description = description.to_string();
        }
        if let Some(lane_status) = request.lane_status {
            lane.lane_status = lane_status.to_string();
        }
        if let Some(status_report) = request.status_report {
            lane.status_report = status_report.to_string();
        }
        if let Some(reviewer_feedback) = request.reviewer_feedback {
            lane.reviewer_feedback = reviewer_feedback.to_string();
        }
        lane.updated_by = request.updated_by.to_string();
        Ok(lane)
    }

    fn lanes_ticket_add(
        &self,
        _: &str,
        lane_id: &str,
        ticket_id: &str,
        updated_by: &str,
    ) -> std::result::Result<loom_lanes::Lane, LoomError> {
        let mut lane = gate_lane(lane_id);
        loom_lanes::append_lane_ticket(&mut lane, ticket_id)?;
        lane.updated_by = updated_by.to_string();
        Ok(lane)
    }

    fn lanes_ticket_remove(
        &self,
        _: &str,
        lane_id: &str,
        ticket_id: &str,
        updated_by: &str,
    ) -> std::result::Result<loom_lanes::Lane, LoomError> {
        let mut lane = gate_lane(lane_id);
        lane.lane_tickets
            .retain(|lane_ticket| lane_ticket.ticket_id != ticket_id);
        lane.updated_by = updated_by.to_string();
        Ok(lane)
    }

    fn list_collections(
        &self,
        _: &str,
        _: loom_core::FacetKind,
    ) -> std::result::Result<Vec<String>, LoomError> {
        Ok(vec!["c1".to_string(), "c2".to_string()])
    }

    fn kv_get(
        &self,
        _: &str,
        _: &str,
        _: &[u8],
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("kv_get")
    }
    fn kv_put(&self, _: &str, _: &str, _: &[u8], _: Vec<u8>) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("kv_put")
    }
    fn kv_delete(&self, _: &str, _: &str, _: &[u8]) -> std::result::Result<bool, LoomError> {
        gate_backend_unexpected("kv_delete")
    }
    fn kv_list(&self, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("kv_list")
    }
    fn kv_range(
        &self,
        _: &str,
        _: &str,
        _: &[u8],
        _: &[u8],
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("kv_range")
    }
    fn cas_put(&self, _: &str, _: &[u8]) -> std::result::Result<String, LoomError> {
        gate_backend_unexpected("cas_put")
    }
    fn cas_get(&self, _: &str, _: &str) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("cas_get")
    }
    fn cas_has(&self, _: &str, _: &str) -> std::result::Result<bool, LoomError> {
        gate_backend_unexpected("cas_has")
    }
    fn cas_delete(&self, _: &str, _: &str) -> std::result::Result<bool, LoomError> {
        gate_backend_unexpected("cas_delete")
    }
    fn cas_list(&self, _: &str) -> std::result::Result<Vec<String>, LoomError> {
        gate_backend_unexpected("cas_list")
    }
    fn queue_append(&self, _: &str, _: &str, _: &[u8]) -> std::result::Result<u64, LoomError> {
        gate_backend_unexpected("queue_append")
    }
    fn queue_get(
        &self,
        _: &str,
        _: &str,
        _: u64,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("queue_get")
    }
    fn queue_range(
        &self,
        _: &str,
        _: &str,
        _: u64,
        _: u64,
    ) -> std::result::Result<Vec<Vec<u8>>, LoomError> {
        gate_backend_unexpected("queue_range")
    }
    fn queue_len(&self, _: &str, _: &str) -> std::result::Result<u64, LoomError> {
        gate_backend_unexpected("queue_len")
    }
    fn queue_consumer_position(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<u64, LoomError> {
        gate_backend_unexpected("queue_consumer_position")
    }
    fn queue_consumer_read(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: u32,
    ) -> std::result::Result<Vec<Vec<u8>>, LoomError> {
        gate_backend_unexpected("queue_consumer_read")
    }
    fn queue_consumer_advance(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: u64,
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("queue_consumer_advance")
    }
    fn queue_consumer_reset(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: u64,
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("queue_consumer_reset")
    }
    fn ledger_append(&self, _: &str, _: &str, _: Vec<u8>) -> std::result::Result<u64, LoomError> {
        gate_backend_unexpected("ledger_append")
    }
    fn ledger_get(
        &self,
        _: &str,
        _: &str,
        _: u64,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("ledger_get")
    }
    fn ledger_head(&self, _: &str, _: &str) -> std::result::Result<Option<String>, LoomError> {
        gate_backend_unexpected("ledger_head")
    }
    fn ledger_len(&self, _: &str, _: &str) -> std::result::Result<u64, LoomError> {
        gate_backend_unexpected("ledger_len")
    }
    fn ledger_verify(&self, _: &str, _: &str) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("ledger_verify")
    }
    fn ts_get(&self, _: &str, _: &str, _: i64) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("ts_get")
    }
    fn ts_put(&self, _: &str, _: &str, _: i64, _: Vec<u8>) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("ts_put")
    }
    fn ts_range(
        &self,
        _: &str,
        _: &str,
        _: i64,
        _: i64,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("ts_range")
    }
    fn search_create(&self, _: &str, _: &str, _: &[u8]) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("search_create")
    }
    fn search_index(
        &self,
        _: &str,
        _: &str,
        _: Vec<u8>,
        _: &[u8],
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("search_index")
    }
    fn search_get(
        &self,
        _: &str,
        _: &str,
        _: &[u8],
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("search_get")
    }
    fn search_delete(&self, _: &str, _: &str, _: &[u8]) -> std::result::Result<bool, LoomError> {
        gate_backend_unexpected("search_delete")
    }
    fn search_ids(
        &self,
        _: &str,
        _: &str,
        _: Option<&[u8]>,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("search_ids")
    }
    fn search_remap(&self, _: &str, _: &str, _: &[u8]) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("search_remap")
    }
    fn search_query(&self, _: &str, _: &str, _: &[u8]) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("search_query")
    }
    fn search_source_digest(&self, _: &str, _: &str) -> std::result::Result<String, LoomError> {
        gate_backend_unexpected("search_source_digest")
    }
    fn search_status(&self, _: &str, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("search_status")
    }
    fn columnar_create(
        &self,
        _: &str,
        _: &str,
        _: &[u8],
        _: u64,
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("columnar_create")
    }
    fn columnar_append(&self, _: &str, _: &str, _: &[u8]) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("columnar_append")
    }
    fn columnar_compact(&self, _: &str, _: &str) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("columnar_compact")
    }
    fn columnar_scan(&self, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("columnar_scan")
    }
    fn columnar_columns(&self, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("columnar_columns")
    }
    fn columnar_rows(&self, _: &str, _: &str) -> std::result::Result<u64, LoomError> {
        gate_backend_unexpected("columnar_rows")
    }
    fn columnar_inspect(&self, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("columnar_inspect")
    }
    fn columnar_source_digest(&self, _: &str, _: &str) -> std::result::Result<String, LoomError> {
        gate_backend_unexpected("columnar_source_digest")
    }
    fn columnar_select(
        &self,
        _: &str,
        _: &str,
        _: &[u8],
        _: &[u8],
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("columnar_select")
    }
    fn columnar_aggregate(
        &self,
        _: &str,
        _: &str,
        _: &[u8],
        _: &[u8],
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("columnar_aggregate")
    }
    fn calendar_create_collection(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &[u8],
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("calendar_create_collection")
    }
    fn calendar_delete_collection(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<bool, LoomError> {
        gate_backend_unexpected("calendar_delete_collection")
    }
    fn calendar_put_entry(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &[u8],
    ) -> std::result::Result<String, LoomError> {
        gate_backend_unexpected("calendar_put_entry")
    }
    fn calendar_put_ics(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<String, LoomError> {
        gate_backend_unexpected("calendar_put_ics")
    }
    fn calendar_delete_entry(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<bool, LoomError> {
        gate_backend_unexpected("calendar_delete_entry")
    }
    fn calendar_get_entry(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("calendar_get_entry")
    }
    fn calendar_list_entries(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<Vec<u8>>, LoomError> {
        gate_backend_unexpected("calendar_list_entries")
    }
    fn calendar_get_collection(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("calendar_get_collection")
    }
    fn calendar_list_collections(
        &self,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<String>, LoomError> {
        gate_backend_unexpected("calendar_list_collections")
    }
    fn calendar_range(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("calendar_range")
    }
    fn calendar_search(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<Vec<u8>>, LoomError> {
        gate_backend_unexpected("calendar_search")
    }
    fn calendar_to_ics(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("calendar_to_ics")
    }
    fn contacts_create_book(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &[u8],
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("contacts_create_book")
    }
    fn contacts_delete_book(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<bool, LoomError> {
        gate_backend_unexpected("contacts_delete_book")
    }
    fn contacts_put_entry(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &[u8],
    ) -> std::result::Result<String, LoomError> {
        gate_backend_unexpected("contacts_put_entry")
    }
    fn contacts_put_vcard(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<String, LoomError> {
        gate_backend_unexpected("contacts_put_vcard")
    }
    fn contacts_delete_entry(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<bool, LoomError> {
        gate_backend_unexpected("contacts_delete_entry")
    }
    fn contacts_get_entry(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("contacts_get_entry")
    }
    fn contacts_list_entries(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<Vec<u8>>, LoomError> {
        gate_backend_unexpected("contacts_list_entries")
    }
    fn contacts_get_book(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("contacts_get_book")
    }
    fn contacts_list_books(&self, _: &str, _: &str) -> std::result::Result<Vec<String>, LoomError> {
        gate_backend_unexpected("contacts_list_books")
    }
    fn contacts_search(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<Vec<u8>>, LoomError> {
        gate_backend_unexpected("contacts_search")
    }
    fn contacts_to_vcard(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("contacts_to_vcard")
    }
    fn mail_create_mailbox(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &[u8],
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("mail_create_mailbox")
    }
    fn mail_delete_mailbox(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<bool, LoomError> {
        gate_backend_unexpected("mail_delete_mailbox")
    }
    fn mail_ingest_message(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: &[u8],
    ) -> std::result::Result<String, LoomError> {
        gate_backend_unexpected("mail_ingest_message")
    }
    fn mail_delete_message(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<bool, LoomError> {
        gate_backend_unexpected("mail_delete_message")
    }
    fn mail_set_flags(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: &[String],
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("mail_set_flags")
    }
    fn mail_get_message(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("mail_get_message")
    }
    fn mail_to_eml(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("mail_to_eml")
    }
    fn mail_list_messages(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<Vec<u8>>, LoomError> {
        gate_backend_unexpected("mail_list_messages")
    }
    fn mail_get_mailbox(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("mail_get_mailbox")
    }
    fn mail_list_mailboxes(&self, _: &str, _: &str) -> std::result::Result<Vec<String>, LoomError> {
        gate_backend_unexpected("mail_list_mailboxes")
    }
    fn mail_get_flags(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<String>, LoomError> {
        gate_backend_unexpected("mail_get_flags")
    }
    fn mail_search(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<Vec<u8>>, LoomError> {
        gate_backend_unexpected("mail_search")
    }
    fn fs_read_file(&self, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("fs_read_file")
    }
    fn fs_read_link(&self, _: &str, _: &str) -> std::result::Result<String, LoomError> {
        gate_backend_unexpected("fs_read_link")
    }
    fn fs_read_at(
        &self,
        _: &str,
        _: &str,
        _: u64,
        _: u64,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("fs_read_at")
    }
    fn fs_stat(&self, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("fs_stat")
    }
    fn fs_list_directory(&self, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("fs_list_directory")
    }
    fn fs_write_file(
        &self,
        _: &str,
        _: &str,
        _: &[u8],
        _: u32,
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("fs_write_file")
    }
    fn fs_append_file(&self, _: &str, _: &str, _: &[u8]) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("fs_append_file")
    }
    fn fs_remove_file(&self, _: &str, _: &str) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("fs_remove_file")
    }
    fn fs_create_directory(&self, _: &str, _: &str, _: bool) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("fs_create_directory")
    }
    fn fs_remove_directory(&self, _: &str, _: &str, _: bool) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("fs_remove_directory")
    }
    fn fs_write_at(
        &self,
        _: &str,
        _: &str,
        _: u64,
        _: &[u8],
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("fs_write_at")
    }
    fn fs_truncate(&self, _: &str, _: &str, _: u64) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("fs_truncate")
    }
    fn fs_symlink(&self, _: &str, _: &str, _: &str) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("fs_symlink")
    }
    fn vector_create(
        &self,
        _: &str,
        _: &str,
        _: u64,
        _: i32,
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("vector_create")
    }
    fn vector_upsert(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &[u8],
        _: &[u8],
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("vector_upsert")
    }
    fn vector_upsert_source(
        &self,
        _: &str,
        _: &str,
        _: crate::RemoteVectorUpsertSource<'_>,
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("vector_upsert_source")
    }
    fn vector_create_metadata_index(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<bool, LoomError> {
        gate_backend_unexpected("vector_create_metadata_index")
    }
    fn vector_drop_metadata_index(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<bool, LoomError> {
        gate_backend_unexpected("vector_drop_metadata_index")
    }
    fn vector_delete(&self, _: &str, _: &str, _: &str) -> std::result::Result<bool, LoomError> {
        gate_backend_unexpected("vector_delete")
    }
    fn vector_get(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("vector_get")
    }
    fn vector_source_text(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("vector_source_text")
    }
    fn vector_embedding_model(
        &self,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("vector_embedding_model")
    }
    fn vector_ids(
        &self,
        _: &str,
        _: &str,
        _: Option<&str>,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("vector_ids")
    }
    fn vector_metadata_index_keys(
        &self,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("vector_metadata_index_keys")
    }
    fn vector_search(
        &self,
        _: &str,
        _: &str,
        _: &[u8],
        _: u64,
        _: &[u8],
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("vector_search")
    }
    fn vector_search_policy(
        &self,
        _: &str,
        _: &str,
        _: crate::RemoteVectorSearchPolicy<'_>,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("vector_search_policy")
    }
    fn metrics_put_descriptor(&self, _: &str, _: &[u8]) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("metrics_put_descriptor")
    }
    fn metrics_get_descriptor(
        &self,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("metrics_get_descriptor")
    }
    fn metrics_put_observation(
        &self,
        _: &str,
        _: &str,
        _: &[u8],
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("metrics_put_observation")
    }
    fn metrics_query(
        &self,
        _: &str,
        _: &str,
        _: u64,
        _: u64,
        _: u32,
        _: u32,
        _: u32,
        _: u64,
        _: u64,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("metrics_query")
    }
    fn logs_put_record(&self, _: &str, _: &[u8]) -> std::result::Result<String, LoomError> {
        gate_backend_unexpected("logs_put_record")
    }
    fn logs_get_record(&self, _: &str, _: &str) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("logs_get_record")
    }
    fn logs_query(
        &self,
        _: &str,
        _: u64,
        _: u64,
        _: u32,
        _: u64,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("logs_query")
    }
    fn traces_put_span(&self, _: &str, _: &[u8]) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("traces_put_span")
    }
    fn traces_get_span(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("traces_get_span")
    }
    fn traces_trace_spans(
        &self,
        _: &str,
        _: &str,
        _: u32,
        _: u64,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("traces_trace_spans")
    }
    fn traces_query(
        &self,
        _: &str,
        _: u64,
        _: u64,
        _: u32,
        _: u64,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("traces_query")
    }
    fn document_get_binary(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<loom_core::document::DocumentBinary>, LoomError> {
        gate_backend_unexpected("document_get_binary")
    }
    fn vcs_log(&self, _: &str, _: &str) -> std::result::Result<Vec<String>, LoomError> {
        gate_backend_unexpected("vcs_log")
    }
    fn vcs_head_branch(&self, _: &str) -> std::result::Result<String, LoomError> {
        gate_backend_unexpected("vcs_head_branch")
    }
    fn vcs_status(&self, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("vcs_status")
    }
    fn vcs_merge_in_progress(&self, _: &str) -> std::result::Result<bool, LoomError> {
        gate_backend_unexpected("vcs_merge_in_progress")
    }
    fn vcs_merge_conflicts(&self, _: &str) -> std::result::Result<Vec<String>, LoomError> {
        gate_backend_unexpected("vcs_merge_conflicts")
    }
    fn vcs_tag_list(&self, _: &str) -> std::result::Result<Vec<String>, LoomError> {
        gate_backend_unexpected("vcs_tag_list")
    }
    fn vcs_tag_target(&self, _: &str, _: &str) -> std::result::Result<Option<String>, LoomError> {
        gate_backend_unexpected("vcs_tag_target")
    }
    fn vcs_diff(&self, _: &str, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("vcs_diff")
    }
    fn vcs_blame(&self, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("vcs_blame")
    }
    fn vcs_branch(&self, _: &str, _: &str) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("vcs_branch")
    }
    fn vcs_checkout(&self, _: &str, _: &str) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("vcs_checkout")
    }
    fn vcs_stage(&self, _: &str, _: &str) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("vcs_stage")
    }
    fn vcs_stage_all(&self, _: &str) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("vcs_stage_all")
    }
    fn vcs_unstage(&self, _: &str, _: &str) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("vcs_unstage")
    }
    fn vcs_tag_delete(&self, _: &str, _: &str) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("vcs_tag_delete")
    }
    fn vcs_tag_rename(&self, _: &str, _: &str, _: &str) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("vcs_tag_rename")
    }
    fn vcs_restore_file(&self, _: &str, _: &str, _: &str) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("vcs_restore_file")
    }
    fn vcs_restore_path(&self, _: &str, _: &str, _: &str) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("vcs_restore_path")
    }
    fn vcs_merge_resolve(&self, _: &str, _: &str, _: &[u8]) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("vcs_merge_resolve")
    }
    fn vcs_merge_abort(&self, _: &str) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("vcs_merge_abort")
    }
    fn graph_get_node(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("graph_get_node")
    }
    fn graph_get_edge(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("graph_get_edge")
    }
    fn graph_neighbors(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("graph_neighbors")
    }
    fn graph_out_edges(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("graph_out_edges")
    }
    fn graph_in_edges(&self, _: &str, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("graph_in_edges")
    }
    fn graph_reachable(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: i64,
        _: &str,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("graph_reachable")
    }
    fn graph_shortest_path(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<Vec<u8>>, LoomError> {
        gate_backend_unexpected("graph_shortest_path")
    }
    fn graph_query(&self, _: &str, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("graph_query")
    }
    fn graph_explain_query(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("graph_explain_query")
    }
    fn graph_upsert_node(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &[u8],
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("graph_upsert_node")
    }
    fn graph_remove_node(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: bool,
    ) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("graph_remove_node")
    }
    fn document_list_binary(&self, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        Ok(loom_core::document::Collection::new().encode())
    }

    fn document_query_json(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        Ok(br#"{"items":[]}"#.to_vec())
    }

    fn document_find_json(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        Ok(b"[]".to_vec())
    }

    fn store_digest_algo(&self) -> std::result::Result<String, LoomError> {
        Ok("blake3".to_string())
    }

    fn document_put_binary_indexed(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: Vec<u8>,
    ) -> std::result::Result<(), LoomError> {
        Ok(())
    }

    fn document_delete_indexed(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<bool, LoomError> {
        Ok(true)
    }

    fn document_replace_text_indexed(
        &self,
        _: crate::writes::DocumentReplaceTextRequest<'_>,
    ) -> std::result::Result<crate::writes::DocumentReplaceTextResult, LoomError> {
        Ok(crate::writes::DocumentReplaceTextResult {
            replacements: 0,
            digest: String::new(),
            entity_tag: String::new(),
        })
    }

    fn graph_upsert_edge_indexed(
        &self,
        _: &str,
        _: &str,
        _: crate::writes::GraphEdgeWrite<'_>,
    ) -> std::result::Result<(), LoomError> {
        Ok(())
    }

    fn graph_remove_edge_indexed(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<bool, LoomError> {
        Ok(true)
    }

    fn sql_read_table(&self, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("sql_read_table")
    }
    fn sql_read_table_at(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("sql_read_table_at")
    }
    fn sql_index_scan(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &[u8],
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("sql_index_scan")
    }
    fn sql_index_scan_at(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &[u8],
        _: &str,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("sql_index_scan_at")
    }
    fn sql_blame(&self, _: &str, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("sql_blame")
    }
    fn sql_diff(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("sql_diff")
    }
    fn sql_table_diff(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("sql_table_diff")
    }
    fn sql_list_databases(&self, _: &str) -> std::result::Result<Vec<String>, LoomError> {
        gate_backend_unexpected("sql_list_databases")
    }
    fn dataframe_create(&self, _: &str, _: &str, _: &[u8]) -> std::result::Result<(), LoomError> {
        gate_backend_unexpected("dataframe_create")
    }
    fn dataframe_collect(&self, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("dataframe_collect")
    }
    fn dataframe_preview(
        &self,
        _: &str,
        _: &str,
        _: u64,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("dataframe_preview")
    }
    fn dataframe_materialize(
        &self,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<String>, LoomError> {
        gate_backend_unexpected("dataframe_materialize")
    }
    fn dataframe_plan_digest(&self, _: &str, _: &str) -> std::result::Result<String, LoomError> {
        gate_backend_unexpected("dataframe_plan_digest")
    }
    fn dataframe_source_digests(
        &self,
        _: &str,
        _: &str,
    ) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("dataframe_source_digests")
    }
    fn watch_subscribe(
        &self,
        _: &str,
        _: &str,
        _: Option<&str>,
        _: Option<&str>,
        _: Option<&str>,
        _: &[String],
    ) -> std::result::Result<String, LoomError> {
        gate_backend_unexpected("watch_subscribe")
    }
    fn watch_poll(&self, _: &str, _: &str, _: u32) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("watch_poll")
    }
    fn sql_exec(&self, _: &str, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        gate_backend_unexpected("sql_exec")
    }
    fn ts_latest(
        &self,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<(i64, Vec<u8>)>, LoomError> {
        Ok(None)
    }

    fn sql_query(&self, _: &str, _: &str, _: &str) -> std::result::Result<Vec<u8>, LoomError> {
        Ok(Vec::new())
    }

    fn vcs_commit(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: u64,
    ) -> std::result::Result<String, LoomError> {
        Ok(String::new())
    }

    fn vcs_commit_staged(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: u64,
    ) -> std::result::Result<String, LoomError> {
        Ok(String::new())
    }

    fn vcs_tag_create(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: u64,
    ) -> std::result::Result<String, LoomError> {
        Ok(String::new())
    }

    fn vcs_merge_continue(
        &self,
        _: &str,
        _: &str,
        _: u64,
    ) -> std::result::Result<String, LoomError> {
        Ok(String::new())
    }

    fn vcs_squash(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: u64,
    ) -> std::result::Result<String, LoomError> {
        Ok(String::new())
    }

    fn vcs_merge(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: bool,
        _: u64,
    ) -> std::result::Result<loom_core::MergeOutcome, LoomError> {
        Ok(loom_core::MergeOutcome::UpToDate)
    }

    fn vcs_cherry_pick(
        &self,
        _: &str,
        _: &[String],
        _: bool,
        _: u64,
    ) -> std::result::Result<loom_core::ReplayOutcome, LoomError> {
        Ok(loom_core::ReplayOutcome::Clean)
    }

    fn vcs_revert(
        &self,
        _: &str,
        _: &[String],
        _: &str,
        _: bool,
        _: u64,
    ) -> std::result::Result<loom_core::ReplayOutcome, LoomError> {
        Ok(loom_core::ReplayOutcome::Clean)
    }

    fn vcs_rebase(
        &self,
        _: &str,
        _: &str,
        _: bool,
        _: u64,
    ) -> std::result::Result<loom_core::ReplayOutcome, LoomError> {
        Ok(loom_core::ReplayOutcome::Clean)
    }
}

#[test]
fn remote_tool_route_classifies_by_capability_and_promotion() {
    use crate::tools::{
        RemoteCapability, RemoteToolRoute, remote_tool_route, remote_tool_route_for,
    };

    assert_eq!(
        remote_tool_route("kv_get"),
        RemoteToolRoute::UnaryForward,
        "a unary IDL-backed tool forwards over remote"
    );
    // `chat_set_presence` is a permanent host-local tool (in-process ephemeral presence), so it stays a
    // rejected host/composite tool over remote even after the chat family is promoted.
    match remote_tool_route("chat_set_presence") {
        RemoteToolRoute::Reject(message) => assert!(
            message.contains("not available against a remote Loom store"),
            "unexpected message: {message}"
        ),
        other => panic!("an unpromoted host/composite tool must reject, got {other:?}"),
    }
    assert_eq!(
        remote_tool_route("watch_subscribe"),
        RemoteToolRoute::UnaryForward,
        "watch_subscribe is unary and forwards over remote"
    );
    for tool in ["sql_exec", "sql_query", "sql_commit"] {
        assert_eq!(
            remote_tool_route(tool),
            RemoteToolRoute::UnaryForward,
            "{tool} is unary in the surface and forwards"
        );
    }
    assert_eq!(
        remote_tool_route("lanes_create"),
        RemoteToolRoute::UnaryForward,
        "Lane tools are IDL-backed and forward over remote"
    );
    assert_eq!(
        remote_tool_route_for("apps_open", true, Some(RemoteCapability::LocalOnly)),
        RemoteToolRoute::ServerExecute,
        "a server-promoted tool routes to server-side execution"
    );
}

#[test]
fn remote_execute_tool_transport_roundtrips_and_rejects_precisely() {
    use crate::RemoteMcpBackend;

    let echoed = GateTestBackend
        .execute_tool("fixture_tool", br#"{"in":1}"#)
        .expect("fixture tool executes server-side");
    let value: serde_json::Value = serde_json::from_slice(&echoed).expect("json result");
    assert_eq!(value["echo"]["in"], serde_json::json!(1));

    let err = GateTestBackend
        .execute_tool("chat_post_message", b"{}")
        .expect_err("a not-promoted tool is declined");
    assert!(
        err.to_string().contains("not server-promoted"),
        "unexpected error: {err}"
    );
}

#[test]
fn server_side_execute_promoted_apps_tool_roundtrips() {
    use crate::server::execute_promoted_tool;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-exec-apps-{}-{}.loom",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([7u8; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let mcp = crate::LoomMcp::new(StoreAccess::per_request(&path, None));

    // apps_create runs server-side against the served store.
    let create_args = serde_json::to_vec(&json!({
        "workspace": "repo",
        "app": "panel",
        "index_html": b"<!doctype html><html><body>Panel</body></html>".to_vec(),
        "meta_md": b"---\nname: Panel\n---\n".to_vec(),
    }))
    .unwrap();
    execute_promoted_tool(&mcp, "apps_create", &create_args).expect("apps_create server-side");

    // apps_write_file then apps_read_file roundtrip byte-for-byte.
    let write_args = serde_json::to_vec(&json!({
        "workspace": "repo", "app": "panel", "path": "assets/data.json",
        "content": b"{}".to_vec(), "mode": 0o100644,
    }))
    .unwrap();
    execute_promoted_tool(&mcp, "apps_write_file", &write_args)
        .expect("apps_write_file server-side");
    let read_out = execute_promoted_tool(
        &mcp,
        "apps_read_file",
        &serde_json::to_vec(
            &json!({ "workspace": "repo", "app": "panel", "path": "assets/data.json" }),
        )
        .unwrap(),
    )
    .expect("apps_read_file server-side");
    let read_val: serde_json::Value = serde_json::from_slice(&read_out).unwrap();
    assert_eq!(read_val["value"], json!(b"{}".to_vec()));

    // apps_list surfaces the created app.
    let list_out = execute_promoted_tool(
        &mcp,
        "apps_list",
        &serde_json::to_vec(&json!({ "workspace": "repo" })).unwrap(),
    )
    .expect("apps_list server-side");
    let list_val: serde_json::Value = serde_json::from_slice(&list_out).unwrap();
    let apps_named: Vec<&str> = list_val["value"]
        .as_array()
        .expect("inventory array")
        .iter()
        .filter_map(|item| item["app"].as_str())
        .collect();
    assert!(
        apps_named.contains(&"panel"),
        "panel listed: {apps_named:?}"
    );

    // A tool outside the promoted set is declined precisely (`chat_set_presence` stays host-local).
    let err = execute_promoted_tool(&mcp, "chat_set_presence", b"{}")
        .expect_err("an unpromoted tool is declined");
    assert!(
        err.to_string().contains("not server-promoted"),
        "unexpected error: {err}"
    );

    std::fs::remove_file(&path).ok();
    std::fs::remove_dir_all(&path).ok();
}

#[test]
fn server_side_execute_promoted_drive_tool_is_routed() {
    use crate::server::execute_promoted_tool;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-exec-drive-{}-{}.loom",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([8u8; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let mcp = crate::LoomMcp::new(StoreAccess::per_request(&path, None));

    let args = serde_json::to_vec(&json!({ "workspace": "repo" })).unwrap();
    match execute_promoted_tool(&mcp, "drive_list_conflicts", &args) {
        Ok(out) => {
            let val: serde_json::Value = serde_json::from_slice(&out).unwrap();
            assert!(
                val.get("value").is_some(),
                "expected value envelope, got {val}"
            );
        }
        Err(e) => assert!(
            !e.to_string().contains("not server-promoted"),
            "drive tool must be routed to server-side execution, got: {e}"
        ),
    }
    // A meetings read is likewise routed to server-side execution (never the unpromoted rejection).
    match execute_promoted_tool(&mcp, "meetings_list", &args) {
        Ok(out) => {
            let val: serde_json::Value = serde_json::from_slice(&out).unwrap();
            assert!(
                val.get("value").is_some(),
                "expected value envelope, got {val}"
            );
        }
        Err(e) => assert!(
            !e.to_string().contains("not server-promoted"),
            "meetings tool must be routed to server-side execution, got: {e}"
        ),
    }

    std::fs::remove_file(&path).ok();
    std::fs::remove_dir_all(&path).ok();
}

// Promoted ask tools execute beside the served store and preserve ask lifecycle state.
#[test]
fn server_side_execute_promoted_ask_tool_roundtrips() {
    use crate::server::execute_promoted_tool;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-exec-ask-{}-{}.loom",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Document,
                Some("repo"),
                WorkspaceId::v4_from_bytes([9u8; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let mcp = crate::LoomMcp::new(StoreAccess::per_request(&path, None));

    // ask_questions runs server-side, persisting the ask and returning its id.
    let begin = execute_promoted_tool(
        &mcp,
        "ask_questions",
        &serde_json::to_vec(&json!({
            "workspace": "repo",
            "questions": [{ "question": "Proceed?", "shape": "text" }],
        }))
        .unwrap(),
    )
    .expect("ask_questions server-side");
    let begin_val: serde_json::Value = serde_json::from_slice(&begin).unwrap();
    let ask_id = begin_val["value"]["ask_id"]
        .as_str()
        .expect("ask_id in payload")
        .to_string();

    // ask_answers single-shot poll reports pending before any answer.
    let poll = execute_promoted_tool(
        &mcp,
        "ask_answers",
        &serde_json::to_vec(&json!({ "workspace": "repo", "id": ask_id })).unwrap(),
    )
    .expect("ask_answers poll");
    let poll_val: serde_json::Value = serde_json::from_slice(&poll).unwrap();
    assert_eq!(poll_val["value"]["status"], json!("pending"));

    // ask_record submits an answer server-side.
    execute_promoted_tool(
        &mcp,
        "ask_record",
        &serde_json::to_vec(&json!({
            "workspace": "repo",
            "id": ask_id,
            "answers": [{ "index": 0, "status": "answered", "text": "yes" }],
        }))
        .unwrap(),
    )
    .expect("ask_record server-side");

    // The next poll reports answered with the recorded answer.
    let poll2 = execute_promoted_tool(
        &mcp,
        "ask_answers",
        &serde_json::to_vec(&json!({ "workspace": "repo", "id": ask_id })).unwrap(),
    )
    .expect("ask_answers poll after record");
    let poll2_val: serde_json::Value = serde_json::from_slice(&poll2).unwrap();
    assert_eq!(poll2_val["value"]["status"], json!("answered"));
    assert_eq!(poll2_val["value"]["answers"][0]["text"], json!("yes"));

    // An unknown ask is a routed domain error, not the unpromoted rejection.
    let err = execute_promoted_tool(
        &mcp,
        "ask_answers",
        &serde_json::to_vec(&json!({ "workspace": "repo", "id": "nope" })).unwrap(),
    )
    .expect_err("unknown ask errors");
    assert!(
        !err.to_string().contains("not server-promoted"),
        "unexpected error: {err}"
    );

    std::fs::remove_file(&path).ok();
    std::fs::remove_dir_all(&path).ok();
}

// Promoted chat tools execute beside the served store; ephemeral presence remains host-local.
#[test]
fn server_side_execute_promoted_chat_tool_is_routed() {
    use crate::server::execute_promoted_tool;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-exec-chat-{}-{}.loom",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Document,
                Some("repo"),
                WorkspaceId::v4_from_bytes([0x0a; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let mcp = crate::LoomMcp::new(StoreAccess::per_request(&path, None));

    // A chat read is routed to server-side execution (never the unpromoted rejection).
    let args = serde_json::to_vec(&json!({ "workspace": "repo" })).unwrap();
    match execute_promoted_tool(&mcp, "chat_channels", &args) {
        Ok(out) => {
            let val: serde_json::Value = serde_json::from_slice(&out).unwrap();
            assert!(
                val.get("value").is_some(),
                "expected value envelope, got {val}"
            );
        }
        Err(e) => assert!(
            !e.to_string().contains("not server-promoted"),
            "chat tool must be routed to server-side execution, got: {e}"
        ),
    }

    // Ephemeral presence stays host-local: the server-side executor declines it precisely.
    let presence_err = execute_promoted_tool(
        &mcp,
        "chat_set_presence",
        &serde_json::to_vec(&json!({ "workspace": "repo", "channel_id": "c", "status": "online" }))
            .unwrap(),
    )
    .expect_err("chat_set_presence is not server-promoted");
    assert!(
        presence_err.to_string().contains("not server-promoted"),
        "unexpected error: {presence_err}"
    );

    std::fs::remove_file(&path).ok();
    std::fs::remove_dir_all(&path).ok();
}

// Promoted Studio profile-root tools execute beside the served store.
#[test]
fn server_side_execute_promoted_studio_tools_are_routed() {
    use crate::server::execute_promoted_tool;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-exec-studio-{}-{}.loom",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Document,
                Some("repo"),
                WorkspaceId::v4_from_bytes([0x0b; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let mcp = crate::LoomMcp::new(StoreAccess::per_request(&path, None));

    let ws = serde_json::to_vec(&json!({ "workspace": "repo" })).unwrap();
    for name in ["spaces_list", "pages_list", "structures_list"] {
        match execute_promoted_tool(&mcp, name, &ws) {
            Ok(out) => {
                let val: serde_json::Value = serde_json::from_slice(&out).unwrap();
                assert!(val.get("value").is_some(), "{name}: {val}");
            }
            Err(e) => assert!(
                !e.to_string().contains("not server-promoted"),
                "{name} must be routed to server-side execution, got: {e}"
            ),
        }
    }
    // A page/structure read is likewise routed (never the unpromoted rejection).
    for (name, args) in [
        ("pages_get", json!({ "workspace": "repo", "page_id": "p1" })),
        (
            "structures_get",
            json!({ "workspace": "repo", "structure_id": "s1" }),
        ),
    ] {
        match execute_promoted_tool(&mcp, name, &serde_json::to_vec(&args).unwrap()) {
            Ok(_) => {}
            Err(e) => assert!(
                !e.to_string().contains("not server-promoted"),
                "{name} must be routed to server-side execution, got: {e}"
            ),
        }
    }

    std::fs::remove_file(&path).ok();
    std::fs::remove_dir_all(&path).ok();
}

/// Task 660: the previously-deferred store-backed families (lifecycles store-backed, workgraph,
/// import, redmine) now route to server-side execution instead of the class-3 "not server-promoted"
/// reject. Each tool must reach its `execute_promoted_tool` arm (an arg/engine error is fine; a
/// "not server-promoted" error is not). The host-local `lifecycles_active_clear` stays unpromoted.
#[test]
fn server_side_execute_promoted_deferred_families_are_routed() {
    use crate::server::execute_promoted_tool;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-exec-deferred-{}-{}.loom",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Document,
                Some("repo"),
                WorkspaceId::v4_from_bytes([0x0d; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let mcp = crate::LoomMcp::new(StoreAccess::per_request(&path, None));

    for (name, args) in [
        ("lifecycles_definitions", json!({ "workspace": "repo" })),
        ("lifecycles_operation_log", json!({ "workspace": "repo" })),
        (
            "workgraph_changes",
            json!({ "workspace": "repo", "workspace_id": "w", "next_sequence": 0, "max": 10 }),
        ),
        (
            "import_submit_batch",
            json!({ "workspace": "repo", "batch": {} }),
        ),
        (
            "redmine_import_snapshot",
            json!({ "workspace": "repo", "profile": "p", "snapshot": {} }),
        ),
    ] {
        match execute_promoted_tool(&mcp, name, &serde_json::to_vec(&args).unwrap()) {
            Ok(out) => {
                let val: serde_json::Value = serde_json::from_slice(&out).unwrap();
                assert!(val.get("value").is_some(), "{name}: {val}");
            }
            Err(e) => assert!(
                !e.to_string().contains("not server-promoted"),
                "{name} must route to server-side execution, got: {e}"
            ),
        }
    }

    // The in-process host-local active-lifecycle selection is NOT promoted.
    let err = execute_promoted_tool(
        &mcp,
        "lifecycles_active_clear",
        &serde_json::to_vec(&json!({})).unwrap(),
    )
    .expect_err("lifecycles_active_clear stays host-local");
    assert!(err.to_string().contains("not server-promoted"));

    std::fs::remove_file(&path).ok();
    std::fs::remove_dir_all(&path).ok();
}

#[test]
fn server_side_execute_promoted_substrate_tools_are_routed() {
    use crate::server::execute_promoted_tool;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-exec-substrate-{}-{}.loom",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Document,
                Some("repo"),
                WorkspaceId::v4_from_bytes([0x0c; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let mcp = crate::LoomMcp::new(StoreAccess::per_request(&path, None));

    for (name, args) in [
        ("substrate_view_list", json!({ "workspace": "repo" })),
        (
            "substrate_alias_list",
            json!({ "workspace": "repo", "scope_id": "s" }),
        ),
    ] {
        match execute_promoted_tool(&mcp, name, &serde_json::to_vec(&args).unwrap()) {
            Ok(out) => {
                let val: serde_json::Value = serde_json::from_slice(&out).unwrap();
                assert!(val.get("value").is_some(), "{name}: {val}");
            }
            Err(e) => assert!(
                !e.to_string().contains("not server-promoted"),
                "{name} must be routed to server-side execution, got: {e}"
            ),
        }
    }

    execute_promoted_tool(
        &mcp,
        "substrate_transact",
        &serde_json::to_vec(&json!({
            "ops": [
                { "kind": "document.put", "workspace": "repo", "collection": "c", "id": "x", "doc": [1] }
            ]
        }))
        .unwrap(),
    )
    .expect("explicit substrate_transact executes server-side");

    let err = execute_promoted_tool(
        &mcp,
        "substrate_transact",
        &serde_json::to_vec(&json!({ "ops": [{ "kind": "document.put", "id": "x", "doc": [1] }] }))
            .unwrap(),
    )
    .expect_err("implicit substrate_transact op is rejected");
    assert!(
        err.to_string().contains("missing workspace"),
        "unexpected error: {err}"
    );

    let server = LoomServer::with_binding(
        std::sync::Arc::new(crate::LoomMcp::new(StoreAccess::per_request(&path, None))),
        Binding::workspace("repo"),
    );
    let mut args = serde_json::Map::new();
    args.insert(
        "ops".to_string(),
        json!([{ "kind": "document.put", "id": "x", "doc": [1] }]),
    );
    let normalized = server
        .normalize_substrate_transact_arguments(Some(args))
        .expect("normalized arguments");
    assert_eq!(normalized["ops"][0]["workspace"], json!("repo"));

    std::fs::remove_file(&path).ok();
    std::fs::remove_dir_all(&path).ok();
}

#[test]
fn server_side_execute_promoted_tickets_tools_are_routed() {
    use crate::server::execute_promoted_tool;
    use loom_core::workspace::{FacetKind, WorkspaceId};
    use loom_core::{Algo, Loom};
    use loom_store::{FileStore, save_loom};

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-exec-tickets-{}-{}.loom",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    {
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(
                FacetKind::Document,
                Some("repo"),
                WorkspaceId::v4_from_bytes([0x0d; 16]),
            )
            .unwrap();
        save_loom(&mut loom).unwrap();
    }
    let mcp = crate::LoomMcp::new(StoreAccess::per_request(&path, None));

    let created = execute_promoted_tool(
        &mcp,
        "tickets_project_create",
        &serde_json::to_vec(&json!({
            "workspace": "repo",
            "project_id": "eng",
            "key_prefix": "ENG",
            "name": "Engineering"
        }))
        .unwrap(),
    )
    .expect("tickets_project_create executes server-side");
    let project: serde_json::Value = serde_json::from_slice(&created).unwrap();
    let root = project["value"]["profile_root"]
        .as_str()
        .expect("project profile_root")
        .to_string();

    let ticket_out = execute_promoted_tool(
        &mcp,
        "tickets_create",
        &serde_json::to_vec(&json!({
            "workspace": "repo",
            "project_id": "eng",
            "ticket_type": "task",
            "fields": { "title": "Build tickets" },
            "expected_root": root
        }))
        .unwrap(),
    )
    .expect("tickets_create executes server-side");
    let ticket: serde_json::Value = serde_json::from_slice(&ticket_out).unwrap();
    let ticket_resource = &ticket["value"]["resource"];
    assert_eq!(ticket_resource["primary_key"], json!("ENG-1"));
    assert_eq!(ticket["value"]["receipt"]["resource_kind"], json!("ticket"));
    assert_eq!(ticket["value"]["receipt"]["resource_id"], json!("ENG-1"));
    let ticket_id = ticket_resource["ticket_id"]
        .as_str()
        .expect("ticket_id")
        .to_string();

    let got = execute_promoted_tool(
        &mcp,
        "tickets_get",
        &serde_json::to_vec(&json!({ "workspace": "repo", "ticket_id": ticket_id })).unwrap(),
    )
    .expect("tickets_get executes server-side");
    let got_val: serde_json::Value = serde_json::from_slice(&got).unwrap();
    assert_eq!(got_val["value"]["primary_key"], json!("ENG-1"));
    assert_eq!(got_val["value"]["title"], json!("Build tickets"));
    let got_obj = got_val["value"]
        .as_object()
        .expect("readable ticket object");
    assert!(got_obj.contains_key("status"));
    assert!(got_obj.contains_key("description"));
    assert_eq!(got_val["value"]["dependencies"]["depends_on"], json!([]));
    assert_eq!(got_val["value"]["dependencies"]["blocks"], json!([]));
    assert_eq!(got_val["value"]["comment_count"], json!(0));
    assert_eq!(
        got_val["value"]["latest_update"]["operation_kind"],
        json!("ticket.created")
    );

    let got_detailed = execute_promoted_tool(
        &mcp,
        "tickets_get",
        &serde_json::to_vec(&json!({
            "workspace": "repo",
            "ticket_id": &ticket_id,
            "detailed": true
        }))
        .unwrap(),
    )
    .expect("detailed tickets_get executes server-side");
    let got_detailed_val: serde_json::Value = serde_json::from_slice(&got_detailed).unwrap();
    assert_eq!(got_detailed_val["value"]["primary_key"], json!("ENG-1"));
    assert_eq!(
        got_detailed_val["value"]["fields"]["title"],
        json!("Build tickets")
    );

    let history = execute_promoted_tool(
        &mcp,
        "tickets_history",
        &serde_json::to_vec(&json!({ "workspace": "repo", "ticket_id": &ticket_id })).unwrap(),
    )
    .expect("tickets_history executes server-side");
    let history_val: serde_json::Value = serde_json::from_slice(&history).unwrap();
    assert!(
        history_val["value"]
            .as_array()
            .expect("history items")
            .iter()
            .any(|record| record["operation_kind"] == json!("ticket.created"))
    );
    let detailed_history = execute_promoted_tool(
        &mcp,
        "tickets_history",
        &serde_json::to_vec(&json!({
            "workspace": "repo",
            "ticket_id": &ticket_id,
            "detailed": true
        }))
        .unwrap(),
    )
    .expect("detailed tickets_history executes server-side");
    let detailed_history_val: serde_json::Value =
        serde_json::from_slice(&detailed_history).unwrap();
    assert!(
        detailed_history_val["value"]
            .as_array()
            .expect("detailed history items")
            .iter()
            .any(|record| record["envelope"]["payload_digest"].is_string())
    );

    let listed = execute_promoted_tool(
        &mcp,
        "tickets_list",
        &serde_json::to_vec(&json!({
            "workspace": "repo",
            "limit": 5
        }))
        .unwrap(),
    )
    .expect("tickets_list executes server-side");
    let listed_val: serde_json::Value = serde_json::from_slice(&listed).unwrap();
    assert_eq!(listed_val["value"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        listed_val["value"]["items"][0]["primary_key"],
        json!("ENG-1")
    );

    execute_promoted_tool(
        &mcp,
        "lanes_create",
        &serde_json::to_vec(&json!({
            "workspace": "repo",
            "lane_id": "agent-1",
            "lane_key": "agent-1",
            "title": "Agent 1",
            "description": "Lane filter test",
            "lane_kind": "assignment",
            "owner_principal": "agent:1",
            "lane_status": "ready",
            "ticket_ids": ["ENG-1"],
            "active_ticket_id": "ENG-1",
            "status_report": "",
            "reviewer_feedback": "",
            "updated_by": "agent:1"
        }))
        .unwrap(),
    )
    .expect("lanes_create executes server-side");
    let lane_listed = execute_promoted_tool(
        &mcp,
        "tickets_list",
        &serde_json::to_vec(&json!({
            "workspace": "repo",
            "lane": "agent-1",
            "limit": 5
        }))
        .unwrap(),
    )
    .expect("tickets_list lane filter executes server-side");
    let lane_listed_val: serde_json::Value = serde_json::from_slice(&lane_listed).unwrap();
    assert_eq!(
        lane_listed_val["value"]["items"][0]["primary_key"],
        json!("ENG-1")
    );

    let jira_ticket_out = execute_promoted_tool(
        &mcp,
        "tickets_create",
        &serde_json::to_vec(&json!({
            "workspace": "repo",
            "project_id": "eng",
            "ticket_type": "bug",
            "projection": "jira",
            "fields": {
                "fields": {
                    "summary": "Jira shaped ticket",
                    "description": "Jira shaped body"
                }
            }
        }))
        .unwrap(),
    )
    .expect("jira-shaped tickets_create executes server-side");
    let jira_ticket: serde_json::Value = serde_json::from_slice(&jira_ticket_out).unwrap();
    assert_eq!(
        jira_ticket["value"]["resource"]["fields"]["title"],
        json!("Jira shaped ticket")
    );

    let jira_projected = execute_promoted_tool(
        &mcp,
        "tickets_get",
        &serde_json::to_vec(&json!({
            "workspace": "repo",
            "ticket_id": jira_ticket["value"]["resource"]["ticket_id"],
            "projection": "jira",
            "detailed": true
        }))
        .unwrap(),
    )
    .expect("jira-projected tickets_get executes server-side");
    let jira_projected: serde_json::Value = serde_json::from_slice(&jira_projected).unwrap();
    assert_eq!(jira_projected["value"]["projection"], json!("jira"));
    assert_eq!(
        jira_projected["value"]["fields"]["summary"],
        json!("Jira shaped ticket")
    );
    assert!(
        jira_projected["value"].get("projection_profile").is_none(),
        "{jira_projected}"
    );
    assert!(
        jira_projected["value"].get("projection_kind").is_none(),
        "{jira_projected}"
    );

    let fields = execute_promoted_tool(
        &mcp,
        "tickets_fields",
        &serde_json::to_vec(&json!({
            "workspace": "repo",
            "projection": "jira",
            "operation": "create"
        }))
        .unwrap(),
    )
    .expect("tickets_fields executes server-side");
    let fields: serde_json::Value = serde_json::from_slice(&fields).unwrap();
    assert_eq!(fields["value"]["projection"], json!("jira"));
    assert!(fields["value"].get("projection_profile").is_none());
    let title = fields["value"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["native_field"] == json!("title"))
        .unwrap();
    assert_eq!(title["write_path"], json!("fields.summary"));
    let status = fields["value"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["native_field"] == json!("status"))
        .unwrap();
    assert_eq!(status["write_path"], json!("transition.to.name"));

    for (name, args) in [
        ("tickets_history", json!({ "workspace": "repo" })),
        (
            "tickets_project_settings_get",
            json!({ "workspace": "repo", "project_id": "eng" }),
        ),
    ] {
        match execute_promoted_tool(&mcp, name, &serde_json::to_vec(&args).unwrap()) {
            Ok(out) => {
                let val: serde_json::Value = serde_json::from_slice(&out).unwrap();
                assert!(val.get("value").is_some(), "{name}: {val}");
            }
            Err(e) => assert!(
                !e.to_string().contains("not server-promoted"),
                "{name} must be routed to server-side execution, got: {e}"
            ),
        }
    }

    std::fs::remove_file(&path).ok();
    std::fs::remove_dir_all(&path).ok();
}

// The non-SQL `*_list_collections` tools forward over a remote store via
// `RemoteMcpBackend::list_collections`, rather than rejecting as local-handle-only.
#[test]
fn remote_non_sql_list_collections_forwards() {
    let mcp = crate::LoomMcp::new(StoreAccess::remote(std::sync::Arc::new(GateTestBackend)));
    for facet in [
        loom_core::FacetKind::Kv,
        loom_core::FacetKind::Document,
        loom_core::FacetKind::TimeSeries,
        loom_core::FacetKind::Ledger,
        loom_core::FacetKind::Queue,
    ] {
        assert_eq!(
            mcp.read_collections("ws", facet).unwrap_or_else(|e| panic!(
                "{facet:?} list_collections forwards over remote: {e:?}"
            )),
            vec!["c1".to_string(), "c2".to_string()],
            "facet {facet:?}"
        );
    }
}

// Remote `sql_query` calls use the read-only full-result SQL wire method and surface the returned
// bytes unchanged.
#[test]
fn remote_sql_query_forwards() {
    let mcp = crate::LoomMcp::new(StoreAccess::remote(std::sync::Arc::new(GateTestBackend)));
    assert_eq!(
        mcp.read_sql_query("ws", "main", "SELECT 1")
            .expect("sql_query forwards over remote"),
        Vec::<u8>::new()
    );
}

// `timeseries_latest` now forwards over a remote store: the `TimeSeries.latest` wire payload carries the
// `[ts, value]` pair, so the MCP host rebuilds the timestamped `TsPoint`. The mock backend returns `None`
// (empty series), which the host surfaces as `Ok(None)` rather than a rejection.
#[test]
fn remote_ts_latest_forwards() {
    let mcp = crate::LoomMcp::new(StoreAccess::remote(std::sync::Arc::new(GateTestBackend)));
    assert_eq!(
        mcp.read_timeseries_latest("ws", "cpu")
            .expect("timeseries latest forwards over remote"),
        None
    );
}

#[test]
fn remote_lane_tools_forward() {
    let mcp = crate::LoomMcp::new(StoreAccess::remote(std::sync::Arc::new(GateTestBackend)));

    let created = mcp
        .write_lanes_create(
            "ws",
            crate::writes::LaneCreateRequest {
                lane_id: "remote",
                lane_key: "remote",
                title: "Remote lane",
                description: "Remote forwarding test lane.",
                lane_kind: loom_lanes::LaneKind::Assignment.as_str(),
                owner_principal: Some("agent:3"),
                lane_status: "ready",
                lane_tickets: &[loom_lanes::LaneTicket {
                    ticket_id: "MX-1".to_string(),
                    order_key: "F".to_string(),
                }],
                active_ticket_id: Some("MX-1"),
                status_report: "ready",
                reviewer_feedback: "",
                updated_by: Some("agent:3"),
            },
        )
        .expect("lanes_create forwards over remote");
    assert_eq!(created.lane_id, "remote");

    assert_eq!(
        mcp.read_lanes_get("ws", "remote")
            .expect("lanes_get forwards over remote")
            .expect("lane exists")
            .lane_id,
        "remote"
    );
    assert_eq!(
        mcp.read_lanes_list("ws")
            .expect("lanes_list forwards over remote")
            .len(),
        1
    );
    assert_eq!(
        mcp.write_lanes_update(
            "ws",
            crate::writes::LaneUpdateRequest {
                lane_id: "remote",
                title: None,
                description: None,
                lane_status: None,
                status_report: Some("working"),
                reviewer_feedback: Some("revise"),
                updated_by: Some("reviewer"),
            },
        )
        .expect("lanes_update forwards over remote")
        .status_report,
        "working"
    );
    assert_eq!(
        mcp.write_lanes_update(
            "ws",
            crate::writes::LaneUpdateRequest {
                lane_id: "remote",
                title: None,
                description: None,
                lane_status: None,
                status_report: None,
                reviewer_feedback: Some("revise"),
                updated_by: Some("reviewer"),
            },
        )
        .expect("lanes_update forwards over remote")
        .reviewer_feedback,
        "revise"
    );
}

// MX-250: build an authenticated MCP whose effective principal is `writer`, granting the given
// rights on both the Tickets domain (the lane authorization gate) and the Document facet (lane
// document storage). Admin is only granted when the caller includes it, so override-authorization
// can be exercised without it.
fn mx250_authenticated_lane_mcp(
    suffix: &str,
    rights: &[loom_core::AclRight],
) -> (std::path::PathBuf, Arc<LoomMcp>, String) {
    use loom_core::workspace::{AclDomain, FacetKind, WorkspaceId};
    use loom_core::{
        AclEffect, AclGrant, AclScope, AclStore, AclSubject, Algo, IdentityStore, Loom,
        PrincipalKind,
    };
    use loom_store::FileStore;

    let path = std::env::temp_dir().join(format!(
        "loom-mcp-mx250-{suffix}-{}.loom",
        std::process::id()
    ));
    let root = WorkspaceId::v4_from_bytes([90u8; 16]);
    let user = WorkspaceId::v4_from_bytes([91u8; 16]);
    let ns_id = WorkspaceId::v4_from_bytes([92u8; 16]);
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Files, Some("work"), ns_id)
        .unwrap();
    for facet in [FacetKind::Document, FacetKind::Queue] {
        loom.registry_mut().add_facet(ns, facet).unwrap();
    }
    let mut identity = IdentityStore::new(root);
    identity
        .add_principal(user, "writer", PrincipalKind::User)
        .unwrap();
    identity.bind_session(user, "mcp-session").unwrap();
    loom.set_identity_store(identity);
    loom.set_session("mcp-session");
    let mut acl = AclStore::new();
    for domain in [AclDomain::Tickets, AclDomain::Document, AclDomain::Queue] {
        acl.grant(AclGrant {
            subject: AclSubject::Principal(user),
            workspace: Some(ns),
            domain: Some(domain),
            ref_glob: None,
            scopes: vec![AclScope::All],
            rights: rights.iter().copied().collect(),
            effect: AclEffect::Allow,
            predicate: None,
        })
        .unwrap();
    }
    loom.set_acl_store(acl);
    (
        path,
        Arc::new(LoomMcp::new(StoreAccess::persistent(loom))),
        user.to_string(),
    )
}

fn mx250_create_lane(
    mcp: &LoomMcp,
    updated_by: Option<&str>,
) -> std::result::Result<loom_lanes::Lane, loom_core::error::LoomError> {
    mcp.write_lanes_create(
        "work",
        crate::writes::LaneCreateRequest {
            lane_id: "lane-1",
            lane_key: "lane-1",
            title: "",
            description: "",
            lane_kind: loom_lanes::LaneKind::Assignment.as_str(),
            owner_principal: None,
            lane_status: "ready",
            lane_tickets: &[],
            active_ticket_id: None,
            status_report: "",
            reviewer_feedback: "",
            updated_by,
        },
    )
}

// MX-250 (a): a lane mutation with `updated_by` omitted records the effective principal.
#[test]
fn mx250_lane_mutation_omitted_updated_by_derives_effective_principal() {
    use loom_core::AclRight;
    let (path, mcp, principal) =
        mx250_authenticated_lane_mcp("derive", &[AclRight::Read, AclRight::Write]);
    let created = mx250_create_lane(&mcp, None).expect("create derives actor");
    assert_eq!(
        created.updated_by, principal,
        "create derives effective principal"
    );
    let updated = mcp
        .write_lanes_update(
            "work",
            crate::writes::LaneUpdateRequest {
                lane_id: "lane-1",
                title: None,
                description: None,
                lane_status: None,
                status_report: Some("working"),
                reviewer_feedback: None,
                updated_by: None,
            },
        )
        .expect("lane update derives actor");
    assert_eq!(
        updated.updated_by, principal,
        "mutation derives effective principal"
    );
    let _ = std::fs::remove_file(&path);
}

// MX-250 (b): an explicit override equal to the effective principal is accepted without Admin.
#[test]
fn mx250_lane_override_matching_principal_needs_no_admin() {
    use loom_core::AclRight;
    let (path, mcp, principal) =
        mx250_authenticated_lane_mcp("match", &[AclRight::Read, AclRight::Write]);
    mx250_create_lane(&mcp, None).expect("create lane");
    let updated = mcp
        .write_lanes_update(
            "work",
            crate::writes::LaneUpdateRequest {
                lane_id: "lane-1",
                title: None,
                description: None,
                lane_status: None,
                status_report: Some("working"),
                reviewer_feedback: None,
                updated_by: Some(&principal),
            },
        )
        .expect("override equal to effective principal is accepted without admin");
    assert_eq!(updated.updated_by, principal);
    let _ = std::fs::remove_file(&path);
}

// MX-250 (c): an explicit override that differs from the effective principal requires Admin on the
// Tickets domain, so a writer without Admin is rejected.
#[test]
fn mx250_lane_override_differing_principal_requires_admin() {
    use loom_core::AclRight;
    let (path, mcp, _principal) =
        mx250_authenticated_lane_mcp("differ", &[AclRight::Read, AclRight::Write]);
    mx250_create_lane(&mcp, None).expect("create lane");
    let err = mcp
        .write_lanes_update(
            "work",
            crate::writes::LaneUpdateRequest {
                lane_id: "lane-1",
                title: None,
                description: None,
                lane_status: None,
                status_report: Some("working"),
                reviewer_feedback: None,
                updated_by: Some("someone-else"),
            },
        )
        .expect_err("differing override without admin is rejected");
    assert_eq!(
        err.code,
        loom_core::error::Code::PermissionDenied,
        "expected permission denial, got {err:?}"
    );
    let _ = std::fs::remove_file(&path);
}

// Reference-indexed writes route through the remote backend so the engine write and reference-index
// overlay execute in the same store context.
#[test]
fn remote_ref_index_writes_forward() {
    let mcp = crate::LoomMcp::new(StoreAccess::remote(std::sync::Arc::new(GateTestBackend)));
    mcp.write_document_put("ws", "notes", "d1", b"{}".to_vec())
        .expect("document put forwards over remote");
    assert!(
        mcp.write_document_delete("ws", "notes", "d1")
            .expect("document delete forwards over remote")
    );
    let replaced = mcp
        .write_document_replace_text(crate::writes::DocumentReplaceTextRequest {
            workspace: "ws",
            name: "notes",
            id: "d1",
            base_digest: "",
            find: "a",
            replace: "b",
            replace_all: false,
        })
        .expect("document replace_text forwards over remote");
    assert_eq!(replaced.replacements, 0);
    mcp.write_graph_upsert_edge(
        "ws",
        "g",
        crate::writes::GraphEdgeWrite {
            id: "e1",
            src: "a",
            dst: "b",
            label: "rel",
            props: &[],
        },
    )
    .expect("graph upsert_edge forwards over remote");
    assert!(
        mcp.write_graph_remove_edge("ws", "g", "e1")
            .expect("graph remove_edge forwards over remote")
    );
}

// The document_query host composite (list + query + projections + digest algorithm) is reassembled in
// the host over remote primitives (document_list_binary + query_json/find_json + store_digest_algo), so it
// forwards over a remote store instead of rejecting. Against the empty mock backend it yields an empty
// result; local-vs-remote byte parity (including per-item digests under the store algo) is asserted in
// the live test.
#[test]
fn remote_document_query_composite_forwards() {
    let mcp = crate::LoomMcp::new(StoreAccess::remote(std::sync::Arc::new(GateTestBackend)));
    let result = mcp
        .read_document_query(crate::reads::DocumentQueryRead {
            workspace: "ws",
            name: "notes",
            id_prefix: None,
            predicate: None,
            projections: &[],
            index: None,
            value: None,
            cursor: None,
            limit: None,
            include_document: false,
        })
        .expect("document_query composite forwards over remote");
    assert!(result.items.is_empty());
    assert!(result.next_cursor.is_none());
}

// Remote timestamped VCS writes preserve the local call shape, including timestamped digest writes
// and replay or merge outcomes decoded from the backend response.
#[test]
fn remote_vcs_timestamped_writes_forward() {
    let mcp = crate::LoomMcp::new(StoreAccess::remote(std::sync::Arc::new(GateTestBackend)));
    mcp.write_vcs_commit("ws", "a", "m", 1)
        .expect("commit forwards over remote");
    mcp.write_vcs_commit_staged("ws", "a", "m", 1)
        .expect("commit_staged forwards over remote");
    mcp.write_vcs_tag_create("ws", "t", "HEAD", "a", "m", 1)
        .expect("tag_create forwards over remote");
    mcp.write_vcs_merge_continue("ws", "a", 1)
        .expect("merge_continue forwards over remote");
    mcp.write_vcs_squash("ws", "onto", "a", "m", 1)
        .expect("squash forwards over remote");
    mcp.write_sql_commit("ws", "me", "msg", 1)
        .expect("sql_commit forwards over remote");
    assert_eq!(
        mcp.write_vcs_merge("ws", "b", "a", 1)
            .expect("merge forwards over remote"),
        loom_core::MergeOutcome::UpToDate,
    );
    assert_eq!(
        mcp.write_vcs_cherry_pick("ws", &[], 1, false)
            .expect("cherry_pick forwards over remote"),
        loom_core::ReplayOutcome::Clean,
    );
    assert_eq!(
        mcp.write_vcs_revert("ws", &[], "a", 1, false)
            .expect("revert forwards over remote"),
        loom_core::ReplayOutcome::Clean,
    );
    assert_eq!(
        mcp.write_vcs_rebase("ws", "onto", 1, false)
            .expect("rebase forwards over remote"),
        loom_core::ReplayOutcome::Clean,
    );
}
