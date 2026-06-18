use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use loom_core::workspace::{FacetKind, WorkspaceId};
use loom_core::{Algo, Digest, Loom};
use loom_store::FileStore;
use loom_substrate::body::{Block, BlockKind, Body, TextRun};
use loom_substrate::chat::chat_operation_cursor_scope;
use loom_substrate::meetings::{
    AnnotationRecord, Coverage as MeetingsCoverage, ImportRunRecord, InputProfile, MeetingRecord,
    MeetingRecordInput, MeetingsProfileSnapshot, MeetingsProfileSnapshotParts, RedactionRecord,
    RedactionState, SourceRecord, SourceRecordInput, SpanKind, SpanRecord, meetings_profile_key,
};
use loom_substrate::order_token::first_token;
use loom_substrate::{ActorKind, OperationEnvelope, OperationEnvelopeInput};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ResourceContents;
use serde_json::{Value, json};

use crate::resources::ResourceTarget;

use super::params::{
    PChatAgentReply, PChatChannel, PChatClaimTask, PChatCompleteTask, PChatCreateTask,
    PChatCreateThread, PChatEmoji, PChatFetchEvents, PChatInvokeAgent, PChatPostMessage,
    PChatReaction, PChatRequestHandoff, PDocId, PDocPutText, PFtsCreate, PFtsIndex,
    PMeetingsAnnotation, PMeetingsEntityMerge, PMeetingsGet, PMeetingsList, PMeetingsProfile,
    PMeetingsVocabulary, PMeetingsVocabularyPropose, PNsNameId, PPagesCreate, PPagesGet,
    PPagesPublish, PPagesUpdate, PResultPage, PSpacesCreate, PStoreSearch, PStructuresAddNode,
    PStructuresBind, PStructuresCreate, PStructuresLinkNode, PStructuresMoveNode,
    PSubstrateAliasBind, PSubstrateAliasKey, PSubstrateAliasList, PSubstrateChanges,
    PSubstrateRefs, PSubstrateTransact, PSubstrateTransactOp, PSubstrateViewGet, PTicketsCreate,
    PTicketsProjectCreate, PTicketsProjectRekey, PTicketsRelationRemove, PTicketsRelationSet,
    PTicketsUpdate, PTicketsUpdateComment, PTicketsUpdateRelationSet,
};
use super::{Binding, LoomServer, workspace_profile_id};
use crate::writes::SubstrateViewDefineRequest;
use crate::{LoomMcp, StoreAccess};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpProtocolConformanceSummary {
    pub suites_passed: usize,
    pub scenarios_passed: usize,
    pub suites: Vec<&'static str>,
}

pub const MCP_PROTOCOL_CERTIFICATION_SCENARIOS: usize = 13;
pub const MCP_PROTOCOL_CERTIFICATION_SUITES: &[&str] = &[
    "mcp-substrate-transact",
    "mcp-search",
    "mcp-substrate-changes",
    "mcp-substrate-refs",
    "mcp-chat",
    "mcp-meetings",
    "mcp-studio-status",
];

pub fn certify_in_process_mcp_protocol() -> Result<McpProtocolConformanceSummary, String> {
    substrate_transact_applies_bound_scope()?;
    substrate_transact_rolls_back()?;
    search_reports_degraded_lexical_across_fts_collections()?;
    substrate_changes_reads_profile_operation_logs()?;
    substrate_changes_reads_chat_operation_logs()?;
    substrate_changes_reads_structure_operation_logs()?;
    substrate_refs_bootstraps_persisted_projection()?;
    substrate_refs_indexes_published_page_bodies()?;
    substrate_refs_indexes_published_block_refs()?;
    substrate_aliases_bind_rebind_list_and_release()?;
    chat_tools_project_messages_threads_tasks_agents_and_handoffs()?;
    meetings_tools_project_outputs_review_and_evidence()?;
    studio_status_resource_projects_ticket_assignments()?;
    Ok(McpProtocolConformanceSummary {
        suites_passed: MCP_PROTOCOL_CERTIFICATION_SUITES.len(),
        scenarios_passed: MCP_PROTOCOL_CERTIFICATION_SCENARIOS,
        suites: MCP_PROTOCOL_CERTIFICATION_SUITES.to_vec(),
    })
}

fn substrate_transact_applies_bound_scope() -> Result<(), String> {
    let path = temp_path("mcp-protocol-transact");
    let ns_id = WorkspaceId::v4_from_bytes([14u8; 16]);
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).map_err(strerr)?);
    loom.registry_mut()
        .create(FacetKind::Document, Some("docs"), ns_id)
        .map_err(strerr)?;
    let server = LoomServer::with_binding(
        Arc::new(LoomMcp::new(StoreAccess::persistent(loom))),
        Binding::collection("docs", "notes"),
    );
    let props = loom_codec::encode(&loom_codec::Value::Map(Vec::new())).map_err(strerr)?;
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
                    props: props.clone(),
                },
                PSubstrateTransactOp::GraphUpsertNode {
                    workspace: None,
                    collection: None,
                    id: "bob".to_string(),
                    props: props.clone(),
                },
                PSubstrateTransactOp::GraphUpsertEdge {
                    workspace: None,
                    collection: None,
                    id: "edge-1".to_string(),
                    src: "alice".to_string(),
                    dst: "bob".to_string(),
                    label: "mentions".to_string(),
                    props: props.clone(),
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
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(&result["value"]["applied"], &json!(6), "applied count")?;
    expect_eq(
        &result["value"]["results"][5]["kind"],
        &json!("substrate.view_define"),
        "view result kind",
    )?;
    let doc = server
        .document_get_binary(Parameters(PDocId {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id: "note-1".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &doc["value"]["bytes"],
        &json!(b"hello".to_vec()),
        "document value",
    )?;
    let node = server
        .graph_get_node(Parameters(PNsNameId {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id: "alice".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(&node["value"], &json!(props), "graph node value")?;
    let view = server
        .substrate_view_get(Parameters(PSubstrateViewGet {
            workspace: "docs".to_string(),
            view_id: "status".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(&view["value"]["view_id"], &json!("status"), "view id")?;
    remove_temp(&path);
    Ok(())
}

fn substrate_transact_rolls_back() -> Result<(), String> {
    let path = temp_path("mcp-protocol-transact-rollback");
    let ns_id = WorkspaceId::v4_from_bytes([15u8; 16]);
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).map_err(strerr)?);
    loom.registry_mut()
        .create(FacetKind::Document, Some("docs"), ns_id)
        .map_err(strerr)?;
    loom_core::document::doc_put(&mut loom, ns_id, "notes", "base", b"hello".to_vec())
        .map_err(strerr)?;
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
        Ok(_) => return Err("stale transaction should fail".to_string()),
        Err(error) => error,
    };
    if !err.message.contains("CONFLICT") {
        return Err(format!("expected conflict, got {}", err.message));
    }
    let created = server
        .document_get_binary(Parameters(PDocId {
            workspace: "docs".to_string(),
            collection: "notes".to_string(),
            id: "created".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(&created["value"], &Value::Null, "rolled back document")?;
    remove_temp(&path);
    Ok(())
}

fn search_reports_degraded_lexical_across_fts_collections() -> Result<(), String> {
    let path = temp_path("mcp-protocol-search");
    let mcp = LoomMcp::new(StoreAccess::per_request(&path, None));
    mcp.write_workspace_create(Some("search-ns"), "search")
        .map_err(strerr)?;
    let server = LoomServer::new(Arc::new(mcp));
    server
        .fts_create(Parameters(PFtsCreate {
            workspace: "search-ns".to_string(),
            collection: "docs".to_string(),
            mapping: search_mapping()?,
        }))
        .map_err(|e| e.to_string())?;
    server
        .fts_index(Parameters(PFtsIndex {
            workspace: "search-ns".to_string(),
            collection: "docs".to_string(),
            id: b"doc1".to_vec(),
            doc: search_doc("loom search")?,
        }))
        .map_err(|e| e.to_string())?;
    let result = server
        .search(Parameters(PStoreSearch {
            workspace: Some("search-ns".to_string()),
            collection: None,
            query: "loom".to_string(),
            field: None,
            limit: Some(10),
            offset: Some(0),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(&result["value"]["reduced"], &json!(true), "reduced")?;
    expect_eq(
        &result["value"]["degraded"]["is_degraded"],
        &json!(true),
        "degraded flag",
    )?;
    expect_eq(
        &result["value"]["degraded"]["reason"],
        &json!("scan_backed_lexical"),
        "degraded reason",
    )?;
    expect_eq(
        &result["value"]["hits"][0]["facet"],
        &json!("fts"),
        "hit facet",
    )?;
    expect_eq(
        &result["value"]["hits"][0]["match_via"],
        &json!("lexical"),
        "hit match_via",
    )?;
    remove_temp(&path);
    Ok(())
}

fn substrate_changes_reads_profile_operation_logs() -> Result<(), String> {
    let path = temp_path("mcp-protocol-profile-changes");
    let mcp = LoomMcp::new(StoreAccess::per_request(&path, None));
    mcp.write_workspace_create(Some("repo"), "vcs")
        .map_err(strerr)?;
    let server = LoomServer::new(Arc::new(mcp));
    let project = server
        .tickets_project_create(Parameters(PTicketsProjectCreate {
            workspace: "repo".to_string(),
            project_id: "eng".to_string(),
            key_prefix: "ENG".to_string(),
            name: "Engineering".to_string(),
            expected_root: None,
        }))
        .map_err(|e| e.to_string())?
        .0;
    let profile_id = project["value"]["workspace_id"]
        .as_str()
        .ok_or_else(|| "ticket project workspace_id missing".to_string())?
        .to_string();
    let ticket = server
        .tickets_create(Parameters(PTicketsCreate {
            workspace: "repo".to_string(),
            project_id: "eng".to_string(),
            ticket_type: "task".to_string(),
            projection: None,
            external_source: None,
            external_id: None,
            fields: json_fields(json!({"title": "Ship substrate changes for !page:Roadmap"})),
            policy_labels: Vec::new(),
            expected_root: None,
        }))
        .map_err(|e| e.to_string())?
        .0;
    let ticket_root = ticket["value"]["resource"]["profile_root"]
        .as_str()
        .ok_or_else(|| "ticket profile root missing".to_string())?
        .to_string();
    let ticket_id = ticket["value"]["resource"]["ticket_id"]
        .as_str()
        .ok_or_else(|| "ticket id missing".to_string())?
        .to_string();
    let updated = server
        .tickets_update(Parameters(PTicketsUpdate {
            workspace: "repo".to_string(),
            ticket_id: ticket_id.clone(),
            projection: None,
            set_fields: Some(json_fields(json!({"status_category": "done"}))),
            delete_fields: Vec::new(),
            action: None,
            target_status: None,
            observed_source_status: None,
            observed_workflow_version: None,
            assignee: None,
            expected_root: Some(ticket_root),
            comment: None,
            comments: vec![PTicketsUpdateComment {
                comment_id: Some("update-evidence".to_string()),
                comment_type: Some("progress".to_string()),
                body: "Status, comment, and relation update atomically.".to_string(),
                evidence: None,
            }],
            relation_sets: vec![PTicketsUpdateRelationSet {
                relation_id: Some("roadmap-link".to_string()),
                kind: "references_page".to_string(),
                target_id: "Roadmap".to_string(),
            }],
            relation_removes: Vec::new(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    let update_changes = updated["value"]["receipt"]["changes"]
        .as_array()
        .ok_or_else(|| "ticket update receipt changes missing".to_string())?;
    if !update_changes.iter().any(|change| {
        change["kind"] == "field_set"
            && change["field"] == "comment"
            && change["after"] == "progress"
    }) {
        return Err("ticket update receipt comment change missing".to_string());
    }
    if !update_changes.iter().any(|change| {
        change["kind"] == "relation_set"
            && change["relation_id"] == "roadmap-link"
            && change["relation_kind"] == "references_page"
            && change["target_id"] == "Roadmap"
    }) {
        return Err("ticket update receipt relation change missing".to_string());
    }
    let tickets = server
        .substrate_changes(Parameters(PSubstrateChanges {
            workspace: "repo".to_string(),
            cursor: format!("oplog:1:tickets:{profile_id}"),
            max: 10,
        }))
        .map_err(|e| e.to_string())?
        .0;
    let ticket_next_cursor = tickets["value"]["next"]
        .as_str()
        .ok_or_else(|| "ticket next cursor missing".to_string())?
        .to_string();
    expect_eq(
        &tickets["value"]["events"][0]["kind"],
        &json!("operation"),
        "ticket event kind",
    )?;
    let ticket_events = tickets["value"]["events"]
        .as_array()
        .ok_or_else(|| "ticket events missing".to_string())?;
    if !ticket_events
        .iter()
        .any(|event| event["operation_kind"] == "ticket.created")
    {
        return Err("ticket created operation missing".to_string());
    }
    if !ticket_events
        .iter()
        .any(|event| event["operation_kind"] == "ticket.comment_added")
    {
        return Err("ticket comment operation missing".to_string());
    }
    let generated_alias = server
        .substrate_alias_resolve(Parameters(PSubstrateAliasKey {
            workspace: "repo".to_string(),
            scope_id: profile_id.clone(),
            alias: "ENG-1".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &generated_alias["value"]["target"],
        &json!(format!("ticket:{ticket_id}")),
        "ticket generated alias target",
    )?;
    expect_eq(
        &generated_alias["value"]["kind"],
        &json!("derived_ticket_key"),
        "ticket derived alias kind",
    )?;
    let rekeyed = server
        .tickets_project_rekey(Parameters(PTicketsProjectRekey {
            workspace: "repo".to_string(),
            project_id: "eng".to_string(),
            key_prefix: "CORE".to_string(),
            expected_root: Some(
                updated["value"]["resource"]["profile_root"]
                    .as_str()
                    .ok_or_else(|| "ticket update root missing".to_string())?
                    .to_string(),
            ),
        }))
        .map_err(|e| e.to_string())?
        .0;
    let retired_alias = server
        .substrate_alias_resolve(Parameters(PSubstrateAliasKey {
            workspace: "repo".to_string(),
            scope_id: profile_id.clone(),
            alias: "ENG-1".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &retired_alias["value"]["retired"],
        &json!(true),
        "ticket retired alias",
    )?;
    let rekey_changes = server
        .substrate_changes(Parameters(PSubstrateChanges {
            workspace: "repo".to_string(),
            cursor: ticket_next_cursor,
            max: 10,
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &rekey_changes["value"]["events"][0]["operation_kind"],
        &json!("project.rekeyed"),
        "ticket rekey operation",
    )?;
    let relation = server
        .tickets_relation_set(Parameters(PTicketsRelationSet {
            workspace: "repo".to_string(),
            ticket_id: ticket_id.clone(),
            kind: "references_document".to_string(),
            target_id: "spec/JIRAISH".to_string(),
            relation_id: Some("spec-doc".to_string()),
            expected_root: Some(
                rekeyed["value"]["profile_root"]
                    .as_str()
                    .ok_or_else(|| "ticket rekey root missing".to_string())?
                    .to_string(),
            ),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &relation["value"]["resource"]["relation_id"],
        &json!("spec-doc"),
        "ticket relation id",
    )?;
    expect_eq(
        &relation["value"]["resource"]["target_type"],
        &json!("document"),
        "ticket relation target type",
    )?;
    server
        .tickets_relation_remove(Parameters(PTicketsRelationRemove {
            workspace: "repo".to_string(),
            ticket_id: ticket_id.clone(),
            relation_id: "spec-doc".to_string(),
            expected_root: Some(
                relation["value"]["resource"]["profile_root"]
                    .as_str()
                    .ok_or_else(|| "ticket relation root missing".to_string())?
                    .to_string(),
            ),
        }))
        .map_err(|e| e.to_string())?;
    let ticket_field_refs = server
        .substrate_refs(Parameters(PSubstrateRefs {
            workspace: "repo".to_string(),
            target: "page:Roadmap".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &ticket_field_refs["value"]["inbound"][0]["source_facet"],
        &json!("tickets"),
        "ticket field ref source facet",
    )?;
    expect_eq(
        &ticket_field_refs["value"]["inbound"][0]["field"],
        &json!("title"),
        "ticket field ref field",
    )?;
    let space = server
        .spaces_create(Parameters(PSpacesCreate {
            workspace: "repo".to_string(),
            space_id: "eng".to_string(),
            title: "Engineering".to_string(),
            expected_root: None,
        }))
        .map_err(|e| e.to_string())?
        .0;
    let space_root = space["value"]["profile_root"]
        .as_str()
        .ok_or_else(|| "space profile root missing".to_string())?
        .to_string();
    let page_profile_id = space["value"]["workspace_id"]
        .as_str()
        .ok_or_else(|| "space workspace_id missing".to_string())?
        .to_string();
    let page = server
        .pages_create(Parameters(PPagesCreate {
            workspace: "repo".to_string(),
            page_id: "page-1".to_string(),
            space_id: "eng".to_string(),
            parent_page_id: None,
            title: "Roadmap".to_string(),
            expected_root: Some(space_root.clone()),
        }))
        .map_err(|e| e.to_string())?
        .0;
    let page_root = page["value"]["profile_root"]
        .as_str()
        .ok_or_else(|| "page profile root missing".to_string())?
        .to_string();
    let err = match server.pages_create(Parameters(PPagesCreate {
        workspace: "repo".to_string(),
        page_id: "page-2".to_string(),
        space_id: "eng".to_string(),
        parent_page_id: None,
        title: "Stale".to_string(),
        expected_root: Some(space_root),
    })) {
        Ok(_) => return Err("stale page create should fail".to_string()),
        Err(error) => error,
    };
    if !err.message.contains("CONFLICT") {
        return Err(format!("expected conflict, got {}", err.message));
    }
    let structure = server
        .structures_create(Parameters(PStructuresCreate {
            workspace: "repo".to_string(),
            structure_id: "roadmap".to_string(),
            space_id: "eng".to_string(),
            kind: "mindmap".to_string(),
            title: "Roadmap".to_string(),
            expected_root: Some(page_root.clone()),
        }))
        .map_err(|e| e.to_string())?
        .0;
    if structure["value"]["structure"]["profile_root"]
        .as_str()
        .is_none()
    {
        return Err("structure profile root missing".to_string());
    }
    let err = match server.structures_create(Parameters(PStructuresCreate {
        workspace: "repo".to_string(),
        structure_id: "stale".to_string(),
        space_id: "eng".to_string(),
        kind: "mindmap".to_string(),
        title: "Stale".to_string(),
        expected_root: Some(page_root),
    })) {
        Ok(_) => return Err("stale structure create should fail".to_string()),
        Err(error) => error,
    };
    if !err.message.contains("CONFLICT") {
        return Err(format!("expected conflict, got {}", err.message));
    }
    server
        .pages_update(Parameters(PPagesUpdate {
            workspace: "repo".to_string(),
            page_id: "page-1".to_string(),
            body_text: "v1".to_string(),
            expected_root: None,
        }))
        .map_err(|e| e.to_string())?;
    let pages = server
        .substrate_changes(Parameters(PSubstrateChanges {
            workspace: "repo".to_string(),
            cursor: format!("oplog:1:pages:{page_profile_id}"),
            max: 10,
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &pages["value"]["next"],
        &json!(format!("oplog:5:pages:{page_profile_id}")),
        "page next cursor",
    )?;
    expect_eq(
        &pages["value"]["events"][3]["operation_kind"],
        &json!("page.updated"),
        "page operation kind",
    )?;
    remove_temp(&path);
    Ok(())
}

fn substrate_refs_bootstraps_persisted_projection() -> Result<(), String> {
    let path = temp_path("mcp-protocol-refs");
    let mcp = LoomMcp::new(StoreAccess::per_request(&path, None));
    mcp.write_workspace_create(Some("docs"), "document")
        .map_err(strerr)?;
    let server = LoomServer::new(Arc::new(mcp));
    server
        .document_put_text(Parameters(PDocPutText {
            workspace: "docs".to_string(),
            collection: "pages".to_string(),
            id: "intro".to_string(),
            text: "See !ticket:LOOM-1.".to_string(),
            expected_entity_tag: None,
        }))
        .map_err(|e| e.to_string())?;
    let result = server
        .substrate_refs(Parameters(PSubstrateRefs {
            workspace: "docs".to_string(),
            target: "ticket:LOOM-1".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &result["value"]["degraded"]["is_degraded"],
        &json!(false),
        "refs degraded flag",
    )?;
    expect_eq(
        &result["value"]["indexed_facets"],
        &json!(["document"]),
        "refs indexed facets",
    )?;
    expect_eq(
        &result["value"]["inbound"][0]["source_facet"],
        &json!("document"),
        "refs source facet",
    )?;
    expect_eq(
        &result["value"]["inbound"][0]["source_collection"],
        &json!("pages"),
        "refs source collection",
    )?;
    expect_eq(
        &result["value"]["inbound"][0]["source_id"],
        &json!("intro"),
        "refs source id",
    )?;
    remove_temp(&path);
    Ok(())
}

fn substrate_refs_indexes_published_page_bodies() -> Result<(), String> {
    let path = temp_path("mcp-protocol-page-refs");
    let mcp = LoomMcp::new(StoreAccess::per_request(&path, None));
    mcp.write_workspace_create(Some("repo"), "vcs")
        .map_err(strerr)?;
    let server = LoomServer::new(Arc::new(mcp));
    let space = server
        .spaces_create(Parameters(PSpacesCreate {
            workspace: "repo".to_string(),
            space_id: "eng".to_string(),
            title: "Engineering".to_string(),
            expected_root: None,
        }))
        .map_err(|e| e.to_string())?
        .0;
    let space_root = space["value"]["profile_root"]
        .as_str()
        .ok_or_else(|| "space profile root missing".to_string())?
        .to_string();
    let page_profile_id = space["value"]["workspace_id"]
        .as_str()
        .ok_or_else(|| "space workspace_id missing".to_string())?
        .to_string();
    let page = server
        .pages_create(Parameters(PPagesCreate {
            workspace: "repo".to_string(),
            page_id: "roadmap".to_string(),
            space_id: "eng".to_string(),
            parent_page_id: None,
            title: "Roadmap".to_string(),
            expected_root: Some(space_root),
        }))
        .map_err(|e| e.to_string())?
        .0;
    let page_root = page["value"]["profile_root"]
        .as_str()
        .ok_or_else(|| "page profile root missing".to_string())?
        .to_string();
    server
        .pages_update(Parameters(PPagesUpdate {
            workspace: "repo".to_string(),
            page_id: "roadmap".to_string(),
            body_text: "See !ticket:LOOM-1".to_string(),
            expected_root: Some(page_root),
        }))
        .map_err(|e| e.to_string())?;
    let draft_refs = server
        .substrate_refs(Parameters(PSubstrateRefs {
            workspace: "repo".to_string(),
            target: "ticket:LOOM-1".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &draft_refs["value"]["inbound"],
        &json!([]),
        "draft refs must not be shared",
    )?;
    server
        .pages_publish(Parameters(PPagesPublish {
            workspace: "repo".to_string(),
            page_id: "roadmap".to_string(),
            expected_root: None,
        }))
        .map_err(|e| e.to_string())?;
    let refs = server
        .substrate_refs(Parameters(PSubstrateRefs {
            workspace: "repo".to_string(),
            target: "ticket:LOOM-1".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &refs["value"]["degraded"]["is_degraded"],
        &json!(false),
        "published refs degraded",
    )?;
    expect_eq(
        &refs["value"]["indexed_facets"],
        &json!(["pages"]),
        "published refs indexed facets",
    )?;
    expect_eq(
        &refs["value"]["inbound"][0]["source_facet"],
        &json!("pages"),
        "published refs source facet",
    )?;
    expect_eq(
        &refs["value"]["inbound"][0]["source_collection"],
        &json!(page_profile_id),
        "published refs source collection",
    )?;
    expect_eq(
        &refs["value"]["inbound"][0]["source_id"],
        &json!("roadmap"),
        "published refs source id",
    )?;
    remove_temp(&path);
    Ok(())
}

fn substrate_refs_indexes_published_block_refs() -> Result<(), String> {
    let path = temp_path("mcp-protocol-page-block-ref");
    let mcp = LoomMcp::new(StoreAccess::per_request(&path, None));
    mcp.write_workspace_create(Some("repo"), "vcs")
        .map_err(strerr)?;
    let server = LoomServer::new(Arc::new(mcp));
    let space = server
        .spaces_create(Parameters(PSpacesCreate {
            workspace: "repo".to_string(),
            space_id: "eng".to_string(),
            title: "Engineering".to_string(),
            expected_root: None,
        }))
        .map_err(|e| e.to_string())?
        .0;
    let space_root = string_value(&space, "/value/profile_root", "space root")?;
    let target = server
        .pages_create(Parameters(PPagesCreate {
            workspace: "repo".to_string(),
            page_id: "target".to_string(),
            space_id: "eng".to_string(),
            parent_page_id: None,
            title: "Target".to_string(),
            expected_root: Some(space_root),
        }))
        .map_err(|e| e.to_string())?
        .0;
    let target_root = string_value(&target, "/value/profile_root", "target root")?;
    let target_body = Body::new(vec![
        Block::new(
            "intro",
            first_token(),
            BlockKind::Paragraph,
            vec![TextRun::new("hello", Vec::new()).map_err(strerr)?],
            Vec::new(),
        )
        .map_err(strerr)?,
    ]);
    let target_update = server
        .pages_update(Parameters(PPagesUpdate {
            workspace: "repo".to_string(),
            page_id: "target".to_string(),
            body_text: String::from_utf8(target_body.encode().map_err(strerr)?)
                .map_err(|e| e.to_string())?,
            expected_root: Some(target_root),
        }))
        .map_err(|e| e.to_string())?
        .0;
    let target_update_root =
        string_value(&target_update, "/value/profile_root", "target update root")?;
    let target_publish = server
        .pages_publish(Parameters(PPagesPublish {
            workspace: "repo".to_string(),
            page_id: "target".to_string(),
            expected_root: Some(target_update_root),
        }))
        .map_err(|e| e.to_string())?
        .0;
    let target_publish_root = string_value(
        &target_publish,
        "/value/profile_root",
        "target publish root",
    )?;
    let page = server
        .pages_create(Parameters(PPagesCreate {
            workspace: "repo".to_string(),
            page_id: "source".to_string(),
            space_id: "eng".to_string(),
            parent_page_id: None,
            title: "Source".to_string(),
            expected_root: Some(target_publish_root),
        }))
        .map_err(|e| e.to_string())?
        .0;
    let page_root = string_value(&page, "/value/profile_root", "page root")?;
    let body = Body::new(vec![
        Block::new(
            "ref-block",
            first_token(),
            BlockKind::BlockRef {
                entity_id: "page:target".to_string(),
                block_id: Some("intro".to_string()),
                section: false,
                pin: Some(1),
            },
            Vec::new(),
            Vec::new(),
        )
        .map_err(strerr)?,
    ]);
    let update = server
        .pages_update(Parameters(PPagesUpdate {
            workspace: "repo".to_string(),
            page_id: "source".to_string(),
            body_text: String::from_utf8(body.encode().map_err(strerr)?)
                .map_err(|e| e.to_string())?,
            expected_root: Some(page_root),
        }))
        .map_err(|e| e.to_string())?
        .0;
    let update_root = string_value(&update, "/value/profile_root", "update root")?;
    server
        .pages_publish(Parameters(PPagesPublish {
            workspace: "repo".to_string(),
            page_id: "source".to_string(),
            expected_root: Some(update_root),
        }))
        .map_err(|e| e.to_string())?;
    let refs = server
        .substrate_refs(Parameters(PSubstrateRefs {
            workspace: "repo".to_string(),
            target: "page:target".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &refs["value"]["inbound"][0]["field"],
        &json!("block_ref"),
        "block_ref field",
    )?;
    expect_eq(
        &refs["value"]["inbound"][0]["relation"],
        &json!("transcludes"),
        "block_ref relation",
    )?;
    let rendered = server
        .pages_get(Parameters(PPagesGet {
            workspace: "repo".to_string(),
            page_id: "source".to_string(),
            page: PResultPage::default(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &rendered["value"]["rendered_body"],
        &json!("hello\n"),
        "block_ref rendered body",
    )?;
    expect_eq(
        &rendered["value"]["render_issues"],
        &json!([]),
        "block_ref render tickets",
    )?;
    remove_temp(&path);
    Ok(())
}

fn substrate_aliases_bind_rebind_list_and_release() -> Result<(), String> {
    let path = temp_path("mcp-protocol-aliases");
    let mcp = LoomMcp::new(StoreAccess::per_request(&path, None));
    mcp.write_workspace_create(Some("repo"), "vcs")
        .map_err(strerr)?;
    let server = LoomServer::new(Arc::new(mcp));
    let bound = server
        .substrate_alias_bind(Parameters(PSubstrateAliasBind {
            workspace: "repo".to_string(),
            scope_id: "studio".to_string(),
            alias: "LOOM-1".to_string(),
            target: "ticket:01HX".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &bound["value"]["sequence"],
        &json!(1),
        "alias first sequence",
    )?;
    let rebound = server
        .substrate_alias_bind(Parameters(PSubstrateAliasBind {
            workspace: "repo".to_string(),
            scope_id: "studio".to_string(),
            alias: "LOOM-1".to_string(),
            target: "ticket:01HY".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &rebound["value"]["target"],
        &json!("ticket:01HY"),
        "alias rebound target",
    )?;
    expect_eq(
        &rebound["value"]["sequence"],
        &json!(2),
        "alias rebound sequence",
    )?;
    let resolved = server
        .substrate_alias_resolve(Parameters(PSubstrateAliasKey {
            workspace: "repo".to_string(),
            scope_id: "studio".to_string(),
            alias: "LOOM-1".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &resolved["value"]["target"],
        &json!("ticket:01HY"),
        "alias resolved target",
    )?;
    let listed = server
        .substrate_alias_list(Parameters(PSubstrateAliasList {
            workspace: "repo".to_string(),
            scope_id: "studio".to_string(),
            page: PResultPage::default(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    let listed_values = listed["value"]
        .as_array()
        .ok_or_else(|| "alias list value must be an array".to_string())?;
    expect_eq(&json!(listed_values.len()), &json!(1), "alias list length")?;
    let released = server
        .substrate_alias_release(Parameters(PSubstrateAliasKey {
            workspace: "repo".to_string(),
            scope_id: "studio".to_string(),
            alias: "LOOM-1".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(&released["value"], &json!(true), "alias released")?;
    let missing = server
        .substrate_alias_resolve(Parameters(PSubstrateAliasKey {
            workspace: "repo".to_string(),
            scope_id: "studio".to_string(),
            alias: "LOOM-1".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(&missing["value"], &json!(null), "released alias missing")?;
    remove_temp(&path);
    Ok(())
}

fn substrate_changes_reads_chat_operation_logs() -> Result<(), String> {
    let path = temp_path("mcp-protocol-chat-changes");
    let mcp = LoomMcp::new(StoreAccess::per_request(&path, None));
    mcp.write_workspace_create(Some("repo"), "vcs")
        .map_err(strerr)?;
    let server = LoomServer::new(Arc::new(mcp));
    let chat_profile_id = workspace_profile_id(&server.mcp, "repo").map_err(strerr)?;
    server
        .mcp
        .write_chat_create_channel("repo", &chat_profile_id, "general", "General")
        .map_err(strerr)?;
    let posted = server
        .chat_post_message(Parameters(PChatPostMessage {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
            message_id: "m1".to_string(),
            thread_id: None,
            body_text: "hello".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    let profile_id = posted["value"]["workspace_id"]
        .as_str()
        .ok_or_else(|| "chat workspace_id missing".to_string())?;
    let channel_id = posted["value"]["channel_id"]
        .as_str()
        .ok_or_else(|| "chat channel_id missing".to_string())?;
    let cursor = chat_operation_cursor_scope(profile_id, channel_id);
    let changes = server
        .substrate_changes(Parameters(PSubstrateChanges {
            workspace: "repo".to_string(),
            cursor: format!("oplog:1:{cursor}"),
            max: 10,
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &changes["value"]["next"],
        &json!(format!("oplog:2:{cursor}")),
        "chat next cursor",
    )?;
    expect_eq(
        &changes["value"]["events"][0]["operation_kind"],
        &json!("message.created"),
        "chat operation kind",
    )?;
    expect_eq(
        &changes["value"]["events"][0]["app_id"],
        &json!("chat"),
        "chat app id",
    )?;
    remove_temp(&path);
    Ok(())
}

fn chat_tools_project_messages_threads_tasks_agents_and_handoffs() -> Result<(), String> {
    let path = temp_path("mcp-protocol-chat");
    let mcp = LoomMcp::new(StoreAccess::per_request(&path, None));
    mcp.write_workspace_create(Some("repo"), "vcs")
        .map_err(strerr)?;
    let server = LoomServer::new(Arc::new(mcp));
    let chat_profile_id = workspace_profile_id(&server.mcp, "repo").map_err(strerr)?;
    server
        .mcp
        .write_chat_create_channel("repo", &chat_profile_id, "general", "General")
        .map_err(strerr)?;
    let posted = server
        .chat_post_message(Parameters(PChatPostMessage {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
            message_id: "m1".to_string(),
            thread_id: None,
            body_text: "hello".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &posted["value"]["operation_kind"],
        &json!("message.created"),
        "chat post kind",
    )?;
    let chat_profile_id = posted["value"]["workspace_id"]
        .as_str()
        .ok_or_else(|| "chat workspace_id missing".to_string())?
        .to_string();
    let chat_channel_id = posted["value"]["channel_id"]
        .as_str()
        .ok_or_else(|| "chat channel_id missing".to_string())?
        .to_string();
    server
        .chat_create_thread(Parameters(PChatCreateThread {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
            thread_id: "t1".to_string(),
            parent_message_id: "m1".to_string(),
        }))
        .map_err(|e| e.to_string())?;
    server
        .chat_post_message(Parameters(PChatPostMessage {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
            message_id: "m2".to_string(),
            thread_id: Some("t1".to_string()),
            body_text: "reply".to_string(),
        }))
        .map_err(|e| e.to_string())?;
    let emoji = server
        .chat_emoji_register(Parameters(PChatEmoji {
            workspace: "repo".to_string(),
            kind: "approved".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &emoji["value"]["custom"],
        &json!(["approved"]),
        "chat emoji registry",
    )?;
    server
        .chat_add_reaction(Parameters(PChatReaction {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
            message_id: "m1".to_string(),
            kind: "approved".to_string(),
        }))
        .map_err(|e| e.to_string())?;
    server
        .chat_create_task(Parameters(PChatCreateTask {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
            task_id: "task-1".to_string(),
            message_id: Some("m1".to_string()),
            title: "triage".to_string(),
        }))
        .map_err(|e| e.to_string())?;
    server
        .chat_claim_task(Parameters(PChatClaimTask {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
            task_id: "task-1".to_string(),
            claim_id: "claim-1".to_string(),
            lease_token: Some("lease-1".to_string()),
        }))
        .map_err(|e| e.to_string())?;
    if server
        .chat_claim_task(Parameters(PChatClaimTask {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
            task_id: "task-1".to_string(),
            claim_id: "claim-2".to_string(),
            lease_token: None,
        }))
        .is_ok()
    {
        return Err("second chat task claim should fail".to_string());
    }
    server
        .chat_complete_task(Parameters(PChatCompleteTask {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
            task_id: "task-1".to_string(),
            claim_id: "claim-1".to_string(),
            result_message_id: Some("m2".to_string()),
        }))
        .map_err(|e| e.to_string())?;
    let agent_principal = WorkspaceId::v4_from_bytes([9u8; 16]).to_string();
    server
        .chat_invoke_agent(Parameters(PChatInvokeAgent {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
            invocation_id: "invoke-1".to_string(),
            agent_principal: agent_principal.clone(),
            source_message_ids: vec!["m1".to_string()],
            prompt_text: "summarize".to_string(),
        }))
        .map_err(|e| e.to_string())?;
    server
        .chat_post_message(Parameters(PChatPostMessage {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
            message_id: "m3".to_string(),
            thread_id: None,
            body_text: "summary".to_string(),
        }))
        .map_err(|e| e.to_string())?;
    server
        .chat_agent_reply(Parameters(PChatAgentReply {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
            invocation_id: "invoke-1".to_string(),
            message_id: "m3".to_string(),
        }))
        .map_err(|e| e.to_string())?;
    server
        .chat_request_handoff(Parameters(PChatRequestHandoff {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
            handoff_id: "handoff-1".to_string(),
            from_agent_principal: agent_principal,
            to_principal: None,
            reason: Some("needs human".to_string()),
        }))
        .map_err(|e| e.to_string())?;
    let messages = server
        .chat_messages(Parameters(PChatChannel {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &messages["value"]["messages"][0]["message_id"],
        &json!("m1"),
        "chat first message",
    )?;
    expect_eq(
        &messages["value"]["messages"][0]["reactions"][0]["kind"],
        &json!("approved"),
        "chat reaction",
    )?;
    expect_eq(
        &messages["value"]["messages"][1]["thread_id"],
        &json!("t1"),
        "chat threaded reply",
    )?;
    expect_eq(
        &messages["value"]["tasks"][0]["state"]["kind"],
        &json!("Completed"),
        "chat completed task",
    )?;
    expect_eq(
        &messages["value"]["agent_invocations"][0]["reply_message_ids"][0],
        &json!("m3"),
        "chat agent reply",
    )?;
    expect_eq(
        &messages["value"]["handoffs"][0]["reason"],
        &json!("needs human"),
        "chat handoff reason",
    )?;
    let events = server
        .chat_fetch_events(Parameters(PChatFetchEvents {
            workspace: "repo".to_string(),
            channel_id: "general".to_string(),
            from_sequence: 1,
            max: 20,
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &events["value"]["events"][3]["operation_kind"],
        &json!("reaction.added"),
        "chat operation event",
    )?;
    expect_eq(
        &events["value"]["next"],
        &json!(format!("oplog:12:chat:{chat_profile_id}:{chat_channel_id}")),
        "chat next cursor",
    )?;
    remove_temp(&path);
    Ok(())
}

fn meetings_tools_project_outputs_review_and_evidence() -> Result<(), String> {
    let path = temp_path("mcp-protocol-meetings");
    let ns_id = WorkspaceId::v4_from_bytes([41u8; 16]);
    let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).map_err(strerr)?);
    loom.registry_mut()
        .create(FacetKind::Vcs, Some("repo"), ns_id)
        .map_err(strerr)?;
    let profile_id = ns_id.to_string();
    let snapshot = sample_meetings_snapshot(&profile_id).map_err(strerr)?;
    loom.store()
        .control_set(
            &meetings_profile_key(&snapshot.workspace_id).map_err(strerr)?,
            snapshot.encode().map_err(strerr)?,
        )
        .map_err(strerr)?;
    let server = LoomServer::new(Arc::new(LoomMcp::new(StoreAccess::persistent(loom))));
    let list = server
        .meetings_list(Parameters(PMeetingsList {
            workspace: "repo".to_string(),
            limit: None,
            offset: None,
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &list["value"]["meetings"][0]["meeting_id"],
        &json!("meet-1"),
        "meetings list first meeting",
    )?;
    let get = server
        .meetings_get(Parameters(PMeetingsGet {
            workspace: "repo".to_string(),
            meeting_id: "meet-1".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &get["value"]["title"],
        &json!("Architecture review"),
        "meetings get title",
    )?;
    expect_eq(
        &get["value"]["annotations"][0]["kind"],
        &json!("Decision"),
        "meetings get annotation kind",
    )?;
    expect_eq(
        &get["value"]["annotations"][0]["status"],
        &json!("accepted"),
        "meetings get annotation status",
    )?;
    let projection = server
        .meetings_projection_outputs(Parameters(PMeetingsProfile {
            workspace: "repo".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &projection["value"]["workspace_id"],
        &json!(profile_id),
        "meetings projection workspace",
    )?;
    expect_eq(
        &projection["value"]["outputs"][0]["projection"],
        &json!("document"),
        "meetings first projection",
    )?;
    expect_eq(
        &projection["value"]["outputs"][0]["action"],
        &json!("upsert"),
        "meetings first projection action",
    )?;
    let review = server
        .meetings_extraction_review(Parameters(PMeetingsProfile {
            workspace: "repo".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &review["value"]["accepted_annotation_ids"][0],
        &json!("ann-1"),
        "meetings accepted annotation",
    )?;
    let accepted = server
        .meetings_accept_annotation(Parameters(PMeetingsAnnotation {
            workspace: "repo".to_string(),
            annotation_id: "ann-2".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &accepted["value"]["status"],
        &json!("accepted"),
        "meetings accept annotation status",
    )?;
    let rejected = server
        .meetings_reject_annotation(Parameters(PMeetingsAnnotation {
            workspace: "repo".to_string(),
            annotation_id: "ann-3".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &rejected["value"]["status"],
        &json!("rejected"),
        "meetings reject annotation status",
    )?;
    let vocabulary = server
        .meetings_propose_vocabulary(Parameters(PMeetingsVocabularyPropose {
            workspace: "repo".to_string(),
            term_id: "term-1".to_string(),
            kind: "DomainTerm".to_string(),
            label: "LCB".to_string(),
            evidence_annotation_ids: vec!["ann-2".to_string()],
            aliases: Some(vec!["loom control block".to_string()]),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &vocabulary["value"]["status"],
        &json!("proposed"),
        "meetings proposed vocabulary status",
    )?;
    let vocabulary = server
        .meetings_accept_vocabulary(Parameters(PMeetingsVocabulary {
            workspace: "repo".to_string(),
            term_id: "term-1".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &vocabulary["value"]["status"],
        &json!("accepted"),
        "meetings accepted vocabulary status",
    )?;
    let merge = server
        .meetings_add_entity_merge(Parameters(PMeetingsEntityMerge {
            workspace: "repo".to_string(),
            merge_id: "merge-1".to_string(),
            canonical_entity_id: "person:ava".to_string(),
            merged_entity_ids: vec!["person:a.vazquez".to_string()],
            evidence_annotation_ids: vec!["ann-1".to_string()],
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &merge["value"]["canonical_entity_id"],
        &json!("person:ava"),
        "meetings entity merge canonical id",
    )?;
    let review = server
        .meetings_extraction_review(Parameters(PMeetingsProfile {
            workspace: "repo".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &review["value"]["accepted_annotation_ids"],
        &json!(["ann-1", "ann-2"]),
        "meetings accepted annotations after review writes",
    )?;
    expect_eq(
        &review["value"]["rejected_annotation_ids"],
        &json!(["ann-3"]),
        "meetings rejected annotations after review writes",
    )?;
    expect_eq(
        &review["value"]["vocabulary_terms"],
        &json!(1),
        "meetings vocabulary after review writes",
    )?;
    remove_temp(&path);
    Ok(())
}

fn sample_meetings_snapshot(profile_id: &str) -> loom_core::Result<MeetingsProfileSnapshot> {
    let mut source = SourceRecord::new(SourceRecordInput {
        source_id: "src-1",
        source_system: "granola-api",
        external_id: "not_1",
        source_digest: Digest::hash(Algo::Blake3, b"meeting-source"),
        observed_at_ms: 100,
        access_scope: "personal-notes",
        coverage: MeetingsCoverage::Partial,
    })?;
    source.sidecar_digest = Some(Digest::hash(Algo::Blake3, b"meeting-sidecar"));

    let mut meeting = MeetingRecord::new(MeetingRecordInput {
        meeting_id: "meet-1",
        title: "Architecture review",
        current_source_digest: Digest::hash(Algo::Blake3, b"meeting-source"),
        created_at_ms: 100,
        updated_at_ms: 120,
    })?;
    meeting.source_refs = vec!["src-1".to_string()];
    meeting.attendee_refs = vec!["person:ava".to_string(), "person:nas".to_string()];

    let mut span = SpanRecord::new(
        "span-1",
        "meet-1",
        "src-1",
        SpanKind::TranscriptEntry,
        "granola:not_1/transcript/0",
    )?;
    span.text_digest = Some(Digest::hash(Algo::Blake3, b"meeting-text"));

    let mut annotation = AnnotationRecord::new(
        "ann-1",
        "meet-1",
        vec!["span-1".to_string()],
        "Decision",
        "Use normalized import snapshots",
        130,
    )?;
    annotation.accept("principal-1", 140)?;
    let suggested_annotation = AnnotationRecord::new(
        "ann-2",
        "meet-1",
        vec!["span-1".to_string()],
        "Risk",
        "Migration risk",
        150,
    )?;
    let rejected_annotation = AnnotationRecord::new(
        "ann-3",
        "meet-1",
        vec!["span-1".to_string()],
        "Task",
        "Rewrite history",
        160,
    )?;

    let mut import_run = ImportRunRecord::new(
        "run-1",
        InputProfile::GranolaApi,
        "personal-notes",
        MeetingsCoverage::Partial,
        90,
    )?;
    import_run.observed_ids = vec!["not_1".to_string()];
    import_run.coverage_gaps = vec!["rate-limit".to_string()];
    import_run.source_sidecar_digest = Some(Digest::hash(Algo::Blake3, b"meeting-sidecar"));

    let mut redaction = RedactionRecord::new(
        "redact-1",
        "span-1",
        "span",
        RedactionState::RetainedMetadataOnly,
        "policy-1",
        150,
    )?;
    redaction.retained_digest = Some(Digest::hash(Algo::Blake3, b"retained-metadata"));

    MeetingsProfileSnapshot::new(
        profile_id,
        MeetingsProfileSnapshotParts {
            sources: vec![source],
            meetings: vec![meeting],
            spans: vec![span],
            annotations: vec![annotation, suggested_annotation, rejected_annotation],
            vocabulary_terms: Vec::new(),
            entity_merges: Vec::new(),
            promotions: Vec::new(),
            import_runs: vec![import_run],
            redactions: vec![redaction],
        },
    )
}

fn sample_lifecycle_operation_log(workspace_id: &str) -> loom_core::Result<Vec<u8>> {
    let operation_id = "lifecycle-op-1";
    let payload = b"status lifecycle transition";
    let envelope = OperationEnvelope::new(
        Algo::Blake3,
        OperationEnvelopeInput {
            workspace_id,
            app_id: "studio.lifecycle",
            scope_id: &loom_substrate::lifecycle::lifecycle_operation_cursor_scope(workspace_id),
            operation_id,
            operation_kind: "lifecycle.transitioned",
            sequence: 1,
            actor_principal: WorkspaceId::v4_from_bytes([81; 16]),
            actor_kind: ActorKind::User,
            timestamp_ms: 1,
            idempotency_key: operation_id,
            base_root: Digest::hash(Algo::Blake3, b"base"),
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
        Digest::hash(Algo::Blake3, b"root"),
        envelope.encode()?,
    )?;
    loom_substrate::lifecycle::LifecycleOperationLog::new(workspace_id, vec![record])
        .and_then(|log| log.encode())
}

fn studio_status_resource_projects_ticket_assignments() -> Result<(), String> {
    let path = temp_path("mcp-protocol-studio-status");
    let mcp = LoomMcp::new(StoreAccess::per_request(&path, None));
    mcp.write_workspace_create(Some("repo"), "vcs")
        .map_err(strerr)?;
    let profile_id = workspace_profile_id(&mcp, "repo").map_err(strerr)?;
    let snapshot = sample_meetings_snapshot(&profile_id).map_err(strerr)?;
    mcp.store()
        .write(|loom| {
            loom.store().control_set(
                &meetings_profile_key(&snapshot.workspace_id)?,
                snapshot.encode()?,
            )?;
            loom.store().control_set(
                &loom_substrate::lifecycle::lifecycle_operation_log_key(&profile_id)?,
                sample_lifecycle_operation_log(&profile_id)?,
            )
        })
        .map_err(strerr)?;
    let server = LoomServer::new(Arc::new(mcp));
    server
        .tickets_project_create(Parameters(PTicketsProjectCreate {
            workspace: "repo".to_string(),
            project_id: "eng".to_string(),
            key_prefix: "ENG".to_string(),
            name: "Engineering".to_string(),
            expected_root: None,
        }))
        .map_err(|e| e.to_string())?;
    server
        .tickets_create(Parameters(PTicketsCreate {
            workspace: "repo".to_string(),
            project_id: "eng".to_string(),
            ticket_type: "task".to_string(),
            projection: None,
            external_source: None,
            external_id: None,
            fields: json_fields(json!({
                "title": "Ship status view",
                "assignee": "owner",
                "status_category": "in_progress"
            })),
            policy_labels: Vec::new(),
            expected_root: None,
        }))
        .map_err(|e| e.to_string())?;
    let view_scopes = [profile_id.as_str()];
    let view_facets = ["vcs"];
    for (view_id, projection_ref, media_type) in [
        ("tickets-open", "view:tickets.open", "application/json"),
        (
            "planning-markdown",
            "view:planning.markdown",
            "text/markdown",
        ),
        (
            "meetings-review",
            "view:meetings.extraction-review",
            "application/json",
        ),
        (
            "lifecycle-ops",
            "view:lifecycle.operations",
            "application/json",
        ),
    ] {
        server
            .mcp
            .write_substrate_view_define(SubstrateViewDefineRequest {
                workspace: "repo",
                view_id,
                source_scopes: &view_scopes,
                source_facets: &view_facets,
                projection_ref,
                output_facet: Some("document"),
                media_type,
                freshness_policy: "on_read",
            })
            .map_err(strerr)?;
    }
    let ticket_view = server
        .substrate_view_get(Parameters(PSubstrateViewGet {
            workspace: "repo".to_string(),
            view_id: "tickets-open".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &ticket_view["value"]["projection"]["profile"],
        &json!("tickets"),
        "ticket view profile",
    )?;
    expect_eq(
        &ticket_view["value"]["projection"]["items"][0]["primary_key"],
        &json!("ENG-1"),
        "ticket view item",
    )?;
    let planning_view = server
        .substrate_view_get(Parameters(PSubstrateViewGet {
            workspace: "repo".to_string(),
            view_id: "planning-markdown".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &planning_view["value"]["projection"]["media_type"],
        &json!("text/markdown"),
        "planning markdown media type",
    )?;
    let planning_body = planning_view["value"]["projection"]["body"]
        .as_str()
        .ok_or_else(|| "planning markdown body missing".to_string())?;
    if !planning_body.contains("Ship status view") {
        return Err(format!(
            "planning markdown body missing ticket summary: {planning_body}"
        ));
    }
    let meetings_view = server
        .substrate_view_get(Parameters(PSubstrateViewGet {
            workspace: "repo".to_string(),
            view_id: "meetings-review".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &meetings_view["value"]["projection"]["review"]["accepted_annotation_ids"][0],
        &json!("ann-1"),
        "meetings view accepted annotation",
    )?;
    let lifecycle_view = server
        .substrate_view_get(Parameters(PSubstrateViewGet {
            workspace: "repo".to_string(),
            view_id: "lifecycle-ops".to_string(),
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &lifecycle_view["value"]["projection"]["operation_log"]["count"],
        &json!(1),
        "lifecycle view operation count",
    )?;
    let value = read_text_json(
        &server,
        &ResourceTarget::StudioStatus {
            workspace: "repo".to_string(),
            principal: "owner".to_string(),
        },
    )?;
    expect_eq(
        &value["projection_status"]["assigned_open_items"],
        &json!("source_backed"),
        "assigned items status",
    )?;
    expect_eq(
        &value["projection_status"]["planning_markdown_mirror"],
        &json!("source_backed"),
        "markdown status",
    )?;
    expect_eq(
        &value["projection_status"]["changes_since_cursor"],
        &json!("source_backed"),
        "changes status",
    )?;
    expect_eq(
        &value["projection_status"]["active_lifecycle"],
        &json!("source_backed"),
        "lifecycle status",
    )?;
    expect_eq(
        &value["projection_status"]["review_comment_ownership"],
        &json!("source_backed"),
        "review ownership status",
    )?;
    expect_eq(
        &value["sections"]["changes_since_cursor"]["value"]["sources"][0]["source"],
        &json!("tickets"),
        "changes source",
    )?;
    expect_eq(
        &value["sections"]["changes_since_cursor"]["value"]["sources"][0]["recent"][0]["operation_kind"],
        &json!("project.created"),
        "changes operation kind",
    )?;
    expect_eq(
        &value["sections"]["assigned_open_items"]["value"]["assigned"][0]["primary_key"],
        &json!("ENG-1"),
        "assigned primary key",
    )?;
    expect_eq(
        &value["sections"]["active_lifecycle"]["value"]["operation_log"]["recent"][0]["operation_kind"],
        &json!("lifecycle.transitioned"),
        "lifecycle operation kind",
    )?;
    expect_eq(
        &value["sections"]["review_comment_ownership"]["value"]["meetings_extraction_review"]["accepted_annotation_ids"]
            [0],
        &json!("ann-1"),
        "review accepted annotation",
    )?;
    expect_eq(
        &value["sections"]["assigned_open_items"]["value"]["assigned"][0]["title"],
        &json!("Ship status view"),
        "assigned title",
    )?;
    let markdown = value["sections"]["planning_markdown_mirror"]["value"]["body"]
        .as_str()
        .ok_or_else(|| "markdown body missing".to_string())?;
    if !markdown.contains("Ship status view") {
        return Err(format!("markdown body missing ticket title: {markdown}"));
    }
    remove_temp(&path);
    Ok(())
}

fn substrate_changes_reads_structure_operation_logs() -> Result<(), String> {
    let path = temp_path("mcp-protocol-structure-changes");
    let mcp = LoomMcp::new(StoreAccess::per_request(&path, None));
    mcp.write_workspace_create(Some("repo"), "vcs")
        .map_err(strerr)?;
    let server = LoomServer::new(Arc::new(mcp));
    let space = server
        .spaces_create(Parameters(PSpacesCreate {
            workspace: "repo".to_string(),
            space_id: "eng".to_string(),
            title: "Engineering".to_string(),
            expected_root: None,
        }))
        .map_err(|e| e.to_string())?
        .0;
    let mut root = space["value"]["profile_root"]
        .as_str()
        .ok_or_else(|| "space profile root missing".to_string())?
        .to_string();
    let page_profile_id = space["value"]["workspace_id"]
        .as_str()
        .ok_or_else(|| "space workspace_id missing".to_string())?
        .to_string();
    let structure = server
        .structures_create(Parameters(PStructuresCreate {
            workspace: "repo".to_string(),
            structure_id: "roadmap".to_string(),
            space_id: "eng".to_string(),
            kind: "mindmap".to_string(),
            title: "Roadmap".to_string(),
            expected_root: Some(root),
        }))
        .map_err(|e| e.to_string())?
        .0;
    root = structure["value"]["structure"]["profile_root"]
        .as_str()
        .ok_or_else(|| "structure profile root missing".to_string())?
        .to_string();
    let node = server
        .structures_add_node(Parameters(PStructuresAddNode {
            workspace: "repo".to_string(),
            structure_id: "roadmap".to_string(),
            node_id: "root".to_string(),
            kind: "topic".to_string(),
            label: "Root".to_string(),
            body_digest: None,
            entity_ref: None,
            expected_root: Some(root),
        }))
        .map_err(|e| e.to_string())?
        .0;
    root = node["value"]["profile_root"]
        .as_str()
        .ok_or_else(|| "root node profile root missing".to_string())?
        .to_string();
    let node = server
        .structures_add_node(Parameters(PStructuresAddNode {
            workspace: "repo".to_string(),
            structure_id: "roadmap".to_string(),
            node_id: "feature".to_string(),
            kind: "topic".to_string(),
            label: "Feature".to_string(),
            body_digest: None,
            entity_ref: Some("page:page-1".to_string()),
            expected_root: Some(root),
        }))
        .map_err(|e| e.to_string())?
        .0;
    root = node["value"]["profile_root"]
        .as_str()
        .ok_or_else(|| "feature node profile root missing".to_string())?
        .to_string();
    let node = server
        .structures_update_node(Parameters(PStructuresAddNode {
            workspace: "repo".to_string(),
            structure_id: "roadmap".to_string(),
            node_id: "feature".to_string(),
            kind: "feature".to_string(),
            label: "Feature updated".to_string(),
            body_digest: None,
            entity_ref: Some("page:page-2".to_string()),
            expected_root: Some(root),
        }))
        .map_err(|e| e.to_string())?
        .0;
    root = node["value"]["profile_root"]
        .as_str()
        .ok_or_else(|| "updated node profile root missing".to_string())?
        .to_string();
    let node = server
        .structures_bind(Parameters(PStructuresBind {
            workspace: "repo".to_string(),
            structure_id: "roadmap".to_string(),
            node_id: "feature".to_string(),
            entity_ref: Some("ticket:ENG-1".to_string()),
            expected_root: Some(root),
        }))
        .map_err(|e| e.to_string())?
        .0;
    root = node["value"]["profile_root"]
        .as_str()
        .ok_or_else(|| "bound node profile root missing".to_string())?
        .to_string();
    let moved = server
        .structures_move_node(Parameters(PStructuresMoveNode {
            workspace: "repo".to_string(),
            structure_id: "roadmap".to_string(),
            node_id: "feature".to_string(),
            parent_node_id: Some("root".to_string()),
            label: None,
            expected_root: Some(root),
        }))
        .map_err(|e| e.to_string())?
        .0;
    root = moved["value"]["profile_root"]
        .as_str()
        .ok_or_else(|| "move profile root missing".to_string())?
        .to_string();
    server
        .structures_link_node(Parameters(PStructuresLinkNode {
            workspace: "repo".to_string(),
            structure_id: "roadmap".to_string(),
            edge_id: "edge-1".to_string(),
            src_node_id: "root".to_string(),
            dst_node_id: "feature".to_string(),
            label: "child_of".to_string(),
            target_ref: None,
            expected_root: Some(root),
        }))
        .map_err(|e| e.to_string())?;
    let changes = server
        .substrate_changes(Parameters(PSubstrateChanges {
            workspace: "repo".to_string(),
            cursor: format!("oplog:1:pages:{page_profile_id}"),
            max: 20,
        }))
        .map_err(|e| e.to_string())?
        .0;
    expect_eq(
        &changes["value"]["next"],
        &json!(format!("oplog:9:pages:{page_profile_id}")),
        "structure next cursor",
    )?;
    let kinds = changes["value"]["events"]
        .as_array()
        .ok_or_else(|| "structure events missing".to_string())?
        .iter()
        .map(|event| event["operation_kind"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>();
    expect_eq(
        &json!(kinds),
        &json!([
            "space.created",
            "structure.created",
            "structure.node_added",
            "structure.node_added",
            "structure.node_updated",
            "structure.node_bound",
            "structure.node_moved",
            "structure.node_linked"
        ]),
        "structure operation kinds",
    )?;
    remove_temp(&path);
    Ok(())
}

fn read_text_json(server: &LoomServer, target: &ResourceTarget) -> Result<Value, String> {
    match server.read_target(target).map_err(|e| e.to_string())? {
        ResourceContents::TextResourceContents {
            text, mime_type, ..
        } => {
            expect_eq(
                &json!(mime_type),
                &json!(Some("application/json".to_string())),
                "resource mime type",
            )?;
            serde_json::from_str(&text).map_err(strerr)
        }
        _ => Err("resource did not return text".to_string()),
    }
}

fn search_mapping() -> Result<Vec<u8>, String> {
    loom_codec::encode(&loom_codec::Value::Map(vec![(
        loom_codec::Value::Text("body".to_string()),
        loom_codec::Value::Array(vec![
            loom_codec::Value::Uint(0),
            loom_codec::Value::Bool(true),
            loom_codec::Value::Bool(false),
        ]),
    )]))
    .map_err(strerr)
}

fn search_doc(body: &str) -> Result<Vec<u8>, String> {
    loom_codec::encode(&loom_codec::Value::Map(vec![(
        loom_codec::Value::Text("body".to_string()),
        loom_codec::Value::Text(body.to_string()),
    )]))
    .map_err(strerr)
}

fn temp_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("loom-{label}-{}-{nanos}.loom", std::process::id()))
}

fn json_fields(value: Value) -> BTreeMap<String, Value> {
    match value {
        Value::Object(map) => map.into_iter().collect(),
        _ => panic!("ticket fields fixture must be a JSON object"),
    }
}

fn remove_temp(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
}

fn expect_eq(actual: &Value, expected: &Value, label: &str) -> Result<(), String> {
    if actual == expected {
        return Ok(());
    }
    Err(format!("{label}: expected {expected}, got {actual}"))
}

fn string_value(value: &Value, pointer: &str, label: &str) -> Result<String, String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{label} missing"))
}

fn strerr(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use super::*;

    #[test]
    fn mcp_protocol_certification_manifest_is_pinned() {
        assert_eq!(MCP_PROTOCOL_CERTIFICATION_SCENARIOS, 13);
        assert_eq!(
            MCP_PROTOCOL_CERTIFICATION_SUITES,
            [
                "mcp-substrate-transact",
                "mcp-search",
                "mcp-substrate-changes",
                "mcp-substrate-refs",
                "mcp-chat",
                "mcp-meetings",
                "mcp-studio-status",
            ]
        );
    }

    #[test]
    fn substrate_transact_applies_bound_scope_passes() {
        substrate_transact_applies_bound_scope().unwrap();
    }

    #[test]
    fn substrate_transact_rolls_back_passes() {
        substrate_transact_rolls_back().unwrap();
    }

    #[test]
    fn search_reports_degraded_lexical_across_fts_collections_passes() {
        search_reports_degraded_lexical_across_fts_collections().unwrap();
    }

    #[test]
    fn substrate_changes_reads_profile_operation_logs_passes() {
        substrate_changes_reads_profile_operation_logs().unwrap();
    }

    #[test]
    fn substrate_changes_reads_chat_operation_logs_passes() {
        substrate_changes_reads_chat_operation_logs().unwrap();
    }

    #[test]
    fn substrate_changes_reads_structure_operation_logs_passes() {
        substrate_changes_reads_structure_operation_logs().unwrap();
    }

    #[test]
    fn substrate_refs_bootstraps_persisted_projection_passes() {
        substrate_refs_bootstraps_persisted_projection().unwrap();
    }

    #[test]
    fn substrate_refs_indexes_published_page_bodies_passes() {
        substrate_refs_indexes_published_page_bodies().unwrap();
    }

    #[test]
    fn substrate_refs_indexes_published_block_refs_passes() {
        substrate_refs_indexes_published_block_refs().unwrap();
    }

    #[test]
    fn substrate_aliases_bind_rebind_list_and_release_passes() {
        substrate_aliases_bind_rebind_list_and_release().unwrap();
    }

    #[test]
    fn chat_tools_project_messages_threads_tasks_agents_and_handoffs_passes() {
        chat_tools_project_messages_threads_tasks_agents_and_handoffs().unwrap();
    }

    #[test]
    fn meetings_tools_project_outputs_review_and_evidence_passes() {
        meetings_tools_project_outputs_review_and_evidence().unwrap();
    }

    #[test]
    fn studio_status_resource_projects_ticket_assignments_passes() {
        studio_status_resource_projects_ticket_assignments().unwrap();
    }
}
