//! Read-tool engine facade.
//!
//! Each method projects one read tool from [`crate::tools`] onto the engine, routed through
//! [`crate::StoreAccess::read`] - the per-request open or held handle path. Facet reads cross the
//! engine policy checks before returning data; passwordless looms resolve the caller to the owner.
//!
//! The facade returns plain Rust values (bytes, scalars, small summaries) so it is fully unit-testable
//! on the default build; the `server` feature's rmcp layer serializes these to MCP tool results.
//!
//! Licensed under BUSL-1.1.

use std::collections::{BTreeMap, BTreeSet};

use loom_coordination::with_local_store_write_lock;
use loom_core::MetaFilter;
use loom_core::calendar::{
    self, CalendarEntry, CollectionMeta, Component, DateTime, IcalDate, IcalMonth, IcalTime,
    Occurrence,
};
use loom_core::contacts::{self, BookMeta, ContactEntry};
use loom_core::document::{Collection, DocumentBinary, DocumentText};
use loom_core::error::{Code, LoomError, Result};
use loom_core::fs::FileKind;
use loom_core::mail::{self, MailMessage, MailboxMeta};
use loom_core::tabular::Value as TabularValue;
use loom_core::timeseries::Series;
use loom_core::vcs::{ChangeKind as VcsChangeKind, Status};
use loom_core::workspace::{FacetKind, WorkspaceId, WorkspaceInfo, WsSelector};
use loom_core::{
    AclDomain, AclRight, Algo, Digest, DomainChange, FieldValue, GraphQuery, KvMap, LogQuery, Loom,
    MetricQuery, MetricQueryResult, Object, TraceQuery, TraceQueryResult, UnsupportedDomainDetail,
    WatchBatch, WatchCursor, WatchSelector, cas_get, cas_has, cas_list, columnar_aggregate,
    columnar_columns, columnar_inspect, columnar_rows, columnar_scan, columnar_select,
    columnar_source_digest, dataframe_collect, dataframe_plan_digest, dataframe_preview,
    dataframe_source_digests, graph_explain_query, graph_get_edge, graph_get_node, graph_in_edges,
    graph_neighbors, graph_out_edges, graph_query, graph_reachable, graph_shortest_path,
    key_from_cbor, ledger_get, ledger_head, ledger_len, logs_get_record, logs_query,
    metrics_get_descriptor, metrics_query_observations, search_collections, search_get, search_ids,
    search_query, search_source_digest, traces_get_span, traces_query, traces_trace_spans, ts_get,
    ts_latest, vector_embedding_model, vector_get, vector_ids, vector_metadata_index_keys,
    vector_search, vector_search_with_pq_policy, vector_source_text, watch_batch_from_cbor,
};
use loom_sql::{LoomSqlStore, lookup_cbor, result_cbor};
use loom_store::{FileStore, daemon, local_auth_requires_write};
use loom_substrate::changes::{OperationChangeBatch, OperationChangeCursor, OperationChangeRecord};
use loom_substrate::lifecycle::{LifecycleOperationLog, lifecycle_operation_log_key};
use loom_substrate::predicate::{CompareOp, Predicate};
use loom_substrate::refs::{
    AliasBinding, AliasIndex, EntityRef, ReferenceEdge, ReferenceIndex, ReferenceSource,
};
use loom_substrate::versioning::{Checkpoint, EntityRevision, RevisionIndex};
use loom_substrate::view::ViewDefinition;
use loom_substrate::workgraph::{
    WorkgraphFact, WorkgraphFactKind, WorkgraphOperationRecord, WorkgraphState,
    workgraph_operation_cursor_scope, workgraph_operation_kind,
};
use loom_substrate::workgraph::{WorkgraphOperationLog, workgraph_operation_log_key};
use loom_substrate::{ActorKind, OperationEnvelope, OperationEnvelopeInput};
use serde_json::Value as JsonValue;

use crate::apps::{self, AppMeta};
use crate::chat::ChatChannelSummary;
use crate::drive::{DriveFolderSummary, DriveStatSummary, DriveVersionSummary};
use crate::facet_cbor::{
    columnar_aggregates_from_cbor, columnar_columns_cbor, columnar_filter_from_cbor,
    columnar_inspect_cbor, columnar_rows_cbor, columnar_select_columns_from_cbor,
    columnar_values_cbor, dataframe_batch_cbor, digest_strings_cbor, graph_edge_cbor,
    graph_edges_cbor, graph_strings_cbor, props_to_cbor, search_document_cbor, search_ids_cbor,
    search_request_from_cbor, search_response_cbor, vector_embedding_model_cbor, vector_entry_cbor,
    vector_filter_from_cbor, vector_from_bytes, vector_hits_cbor, vector_policy,
    vector_strings_cbor,
};
use crate::meetings::{MeetingsExtractionReviewSummary, MeetingsProjectionSummary};
use crate::pages::{
    PageHistoryEntry, PageSummary, SpaceSummary, StructureRenderSummary, StructureSummary,
};
pub use crate::substrate_refs::ReferenceReconciliationSummary;
use crate::substrate_refs::{ALIAS_INDEX_PATH, REF_INDEX_DIR, REF_INDEX_PATH};
use crate::substrate_revisions::{REVISION_INDEX_DIR, revision_index_path};
use crate::substrate_views::{VIEW_DIR, ViewDefinitionSummary, view_path};
use crate::{LoomMcp, authorize_workgraph_task, now_ms, reject_stateless_ephemeral_kv};
use loom_lanes::{Lane, LaneDecodeDiagnostic, LaneTicketView, LaneView};
use loom_lifecycle::{
    LifecycleDefinitionSummary, LifecycleInstanceSummary, LifecycleOperationLogSummary,
    LifecycleSnapshotPlanSummary, LifecycleSnapshotRecordSummary, LifecycleStageSurfaceSummary,
};
use loom_tickets::{BoardSummary, TicketHistoryRecord, TicketProjectSummary, TicketSummary};

pub struct VectorSearchPolicyRead<'a> {
    pub workspace: &'a str,
    pub name: &'a str,
    pub query: &'a [u8],
    pub k: u64,
    pub filter: &'a [u8],
    pub policy: i32,
    pub threshold: u64,
    pub ef: u64,
    pub pq_m: u64,
    pub pq_k: u64,
    pub pq_iters: u64,
}

pub struct FtsSearchReadRequest<'a> {
    pub workspace: &'a str,
    pub name: &'a str,
    pub query: &'a str,
    pub query_vector: Option<&'a [f32]>,
    pub query_model_id: Option<&'a str>,
    pub query_weights_digest: Option<&'a str>,
    pub field: Option<&'a str>,
    pub limit: u32,
    pub offset: u32,
}

/// Arguments for [`super::LoomMcp::read_document_query`].
pub struct DocumentQueryRead<'a> {
    pub workspace: &'a str,
    pub name: &'a str,
    pub id_prefix: Option<&'a str>,
    pub predicate: Option<&'a serde_json::Value>,
    pub projections: &'a [(&'a str, &'a str)],
    pub index: Option<&'a str>,
    pub value: Option<&'a serde_json::Value>,
    pub cursor: Option<&'a str>,
    pub limit: Option<u64>,
    pub include_document: bool,
}

pub struct StoreSearchReadRequest<'a> {
    pub workspace: Option<&'a str>,
    pub collection: Option<&'a str>,
    pub query: &'a str,
    pub field: Option<&'a str>,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub engine: SearchEngine,
    pub index_status: SearchIndexStatus,
    pub reduced: bool,
    pub degraded: SearchDegraded,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct SearchHit {
    pub facet: String,
    pub workspace: String,
    pub collection: String,
    pub entity_id: String,
    pub field: String,
    pub snippet: String,
    pub offsets: Vec<[u64; 2]>,
    pub scope_context: SearchScopeContext,
    pub root: Option<String>,
    pub match_via: String,
    pub contributing_rungs: Vec<String>,
    pub fused_score: f64,
    pub raw_score: f64,
    pub rung: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SearchScopeContext {
    pub owning_entity: Option<String>,
    pub status_fields: Vec<String>,
    pub refs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SearchEngine {
    pub rungs_available: Vec<String>,
    pub rung_selected_ceiling: String,
    pub rrf_k: u32,
    pub rung_depth: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SearchIndexStatus {
    pub lexical: String,
    pub semantic: String,
    pub graph: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SearchDegraded {
    pub is_degraded: bool,
    pub reason: String,
}

struct SubstrateSemanticSearchArgs<'a> {
    ns: WorkspaceId,
    workspace: &'a str,
    name: &'a str,
    query_vector: &'a [f32],
    query_model_id: Option<&'a str>,
    query_weights_digest: Option<&'a str>,
    field: Option<&'a str>,
    limit: u32,
    offset: u32,
    root: Option<String>,
}

fn read_substrate_semantic_search(
    loom: &Loom<FileStore>,
    args: SubstrateSemanticSearchArgs<'_>,
) -> Result<SearchResult> {
    let model = match vector_embedding_model(loom, args.ns, args.name) {
        Ok(Some(model)) => model,
        Ok(None) => {
            return Ok(substrate_semantic_degraded(
                args.limit,
                "not_built",
                "no_semantic_index",
            ));
        }
        Err(err) if err.code == Code::NotFound => {
            return Ok(substrate_semantic_degraded(
                args.limit,
                "not_built",
                "no_semantic_index",
            ));
        }
        Err(err) => return Err(err),
    };
    let Some(query_model_id) = args.query_model_id else {
        return Ok(substrate_semantic_degraded(
            args.limit,
            "stale",
            "semantic_query_model_missing",
        ));
    };
    if model.model_id != query_model_id
        || model.weights_digest.as_deref() != args.query_weights_digest
    {
        return Ok(substrate_semantic_degraded(
            args.limit,
            "stale",
            "semantic_model_mismatch",
        ));
    }
    let k = semantic_k(args.limit, args.offset)?;
    let hits = vector_search(
        loom,
        args.ns,
        args.name,
        args.query_vector,
        k,
        &MetaFilter::All,
    )?;
    let hits = hits
        .into_iter()
        .skip(args.offset as usize)
        .take(if args.limit == 0 {
            usize::MAX
        } else {
            args.limit as usize
        })
        .map(|hit| {
            let source_text = vector_source_text(loom, args.ns, args.name, &hit.id)?;
            let snippet = source_text.unwrap_or_else(|| hit.id.clone());
            let visible = args
                .field
                .is_none_or(|field| field == "source_text" || field == "body");
            Ok((hit, snippet, visible))
        })
        .filter_map(|result: Result<_>| match result {
            Ok((hit, snippet, true)) => Some(Ok(SearchHit {
                facet: "vector".to_string(),
                workspace: args.workspace.to_string(),
                collection: args.name.to_string(),
                entity_id: hit.id,
                field: "source_text".to_string(),
                snippet,
                offsets: Vec::new(),
                scope_context: SearchScopeContext {
                    owning_entity: None,
                    status_fields: Vec::new(),
                    refs: Vec::new(),
                },
                root: args.root.clone(),
                match_via: "semantic".to_string(),
                contributing_rungs: vec!["semantic".to_string()],
                fused_score: f64::from(hit.score),
                raw_score: f64::from(hit.score),
                rung: "semantic".to_string(),
            })),
            Ok((_, _, false)) => None,
            Err(err) => Some(Err(err)),
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(SearchResult {
        hits,
        engine: SearchEngine {
            rungs_available: vec!["semantic".to_string()],
            rung_selected_ceiling: "semantic".to_string(),
            rrf_k: 60,
            rung_depth: args.limit,
        },
        index_status: SearchIndexStatus {
            lexical: "not_built".to_string(),
            semantic: "ready".to_string(),
            graph: "not_built".to_string(),
        },
        reduced: false,
        degraded: SearchDegraded {
            is_degraded: false,
            reason: String::new(),
        },
    })
}

fn semantic_k(limit: u32, offset: u32) -> Result<usize> {
    if limit == 0 {
        return Ok(usize::MAX);
    }
    let total = limit
        .checked_add(offset)
        .ok_or_else(|| LoomError::invalid("substrate.search limit plus offset overflow"))?;
    usize::try_from(total).map_err(|_| LoomError::invalid("substrate.search limit out of range"))
}

fn substrate_semantic_degraded(limit: u32, semantic_status: &str, reason: &str) -> SearchResult {
    SearchResult {
        hits: Vec::new(),
        engine: SearchEngine {
            rungs_available: Vec::new(),
            rung_selected_ceiling: "semantic".to_string(),
            rrf_k: 60,
            rung_depth: limit,
        },
        index_status: SearchIndexStatus {
            lexical: "not_built".to_string(),
            semantic: semantic_status.to_string(),
            graph: "not_built".to_string(),
        },
        reduced: true,
        degraded: SearchDegraded {
            is_degraded: true,
            reason: reason.to_string(),
        },
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SubstrateRefsResult {
    pub target: String,
    pub inbound: Vec<SubstrateRefEdgeSummary>,
    pub indexed_facets: Vec<String>,
    pub degraded: SubstrateRefsDegraded,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SubstrateRefEdgeSummary {
    pub source_facet: String,
    pub source_collection: String,
    pub source_id: String,
    pub field: String,
    pub relation: String,
    pub span_start: u64,
    pub span_end: u64,
    pub evidence: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SubstrateRefsDegraded {
    pub is_degraded: bool,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SubstrateAliasSummary {
    pub alias: String,
    pub target: String,
    pub scope_id: String,
    pub kind: String,
    pub retired: bool,
    pub sequence: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SubstrateHistorySummary {
    pub scope_id: String,
    pub entity_id: String,
    pub index_present: bool,
    pub revisions: Vec<SubstrateRevisionSummary>,
    pub latest: Option<SubstrateRevisionSummary>,
    pub checkpoints: Vec<SubstrateCheckpointSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SubstrateRevisionLookupSummary {
    pub scope_id: String,
    pub entity_id: String,
    pub index_present: bool,
    pub revision: Option<SubstrateRevisionSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SubstrateCheckpointLookupSummary {
    pub scope_id: String,
    pub index_present: bool,
    pub revision: u64,
    pub checkpoint: Option<SubstrateCheckpointSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SubstrateRevisionSummary {
    pub entity_id: String,
    pub revision: u64,
    pub operation_id: String,
    pub body_digest: String,
    pub body_len: u64,
    pub body_media_type: String,
    pub root: String,
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SubstrateCheckpointSummary {
    pub scope_id: String,
    pub checkpoint_id: String,
    pub root: String,
    pub max_revision: u64,
    pub operation_id: String,
    pub created_at_ms: u64,
}

impl From<&EntityRevision> for SubstrateRevisionSummary {
    fn from(revision: &EntityRevision) -> Self {
        Self {
            entity_id: revision.entity_id.clone(),
            revision: revision.revision,
            operation_id: revision.operation_id.clone(),
            body_digest: revision.body.digest.to_string(),
            body_len: revision.body.len,
            body_media_type: revision.body.media_type.clone(),
            root: revision.root.to_string(),
            timestamp_ms: revision.timestamp_ms,
        }
    }
}

impl From<&Checkpoint> for SubstrateCheckpointSummary {
    fn from(checkpoint: &Checkpoint) -> Self {
        Self {
            scope_id: checkpoint.scope_id.clone(),
            checkpoint_id: checkpoint.checkpoint_id.clone(),
            root: checkpoint.root.to_string(),
            max_revision: checkpoint.max_revision,
            operation_id: checkpoint.operation_id.clone(),
            created_at_ms: checkpoint.created_at_ms,
        }
    }
}

impl From<AliasBinding> for SubstrateAliasSummary {
    fn from(binding: AliasBinding) -> Self {
        Self {
            alias: binding.alias,
            target: binding.target.as_str(),
            scope_id: binding.scope_id,
            kind: "persisted".to_string(),
            retired: false,
            sequence: Some(binding.sequence),
        }
    }
}

impl From<&AliasBinding> for SubstrateAliasSummary {
    fn from(binding: &AliasBinding) -> Self {
        Self {
            alias: binding.alias.clone(),
            target: binding.target.as_str(),
            scope_id: binding.scope_id.clone(),
            kind: "persisted".to_string(),
            retired: false,
            sequence: Some(binding.sequence),
        }
    }
}

fn substrate_refs_result(
    target: &EntityRef,
    index: &ReferenceIndex,
    indexed_facets: Vec<String>,
    degraded: SubstrateRefsDegraded,
) -> SubstrateRefsResult {
    let inbound = index
        .inbound(target)
        .into_iter()
        .map(|edge| SubstrateRefEdgeSummary {
            source_facet: edge.source.facet,
            source_collection: edge.source.collection,
            source_id: edge.source.entity_id,
            field: edge.source.field,
            relation: edge.relation,
            span_start: edge.span_start as u64,
            span_end: edge.span_end as u64,
            evidence: edge.evidence,
        })
        .collect();
    SubstrateRefsResult {
        target: target.as_str(),
        inbound,
        indexed_facets,
        degraded,
    }
}

fn alias_index_from_reserved(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
) -> Result<Option<AliasIndex>> {
    match loom.read_file_reserved(ns, ALIAS_INDEX_PATH) {
        Ok(bytes) => AliasIndex::decode(&bytes).map(Some),
        Err(e) if e.code == Code::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn read_authorized_revision_index(
    loom: &Loom<FileStore>,
    workspace: &str,
    path: &str,
) -> Result<Option<RevisionIndex>> {
    let ns = resolve_ns(loom, workspace)?;
    loom.authorize_file_path(ns, REVISION_INDEX_DIR, AclRight::Read)?;
    loom.authorize_file_path(ns, path, AclRight::Read)?;
    match loom.read_file_reserved(ns, path) {
        Ok(bytes) => RevisionIndex::decode(&bytes).map(Some),
        Err(e) if e.code == Code::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DocumentQueryItem {
    pub id: String,
    pub len: u64,
    pub digest: String,
    pub document: Option<Vec<u8>>,
    pub projections: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DocumentQueryResult {
    pub items: Vec<DocumentQueryItem>,
    pub next_cursor: Option<String>,
}

/// A workspace registry entry rendered for the wire: ids/digests as strings, facets as their wire
/// names. Mirrors the C ABI `loom_workspace_list_json` shape.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct WorkspaceSummary {
    /// The stable workspace id (`uuid`).
    pub id: String,
    /// The workspace name.
    pub name: String,
    /// The facet wire names present in the workspace.
    pub facets: Vec<String>,
    /// The current head commit address, if any.
    pub head: Option<String>,
}

impl WorkspaceSummary {
    fn from_info(info: &WorkspaceInfo) -> Self {
        Self {
            id: info.id.to_string(),
            name: info.name.clone(),
            facets: info.facets.iter().map(|f| f.as_str().to_string()).collect(),
            head: info.head.map(|d| d.to_string()),
        }
    }
}

/// A single time-series point.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TsPoint {
    /// The point timestamp.
    pub ts: i64,
    /// The point value bytes.
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct McpAppResource {
    pub workspace: String,
    pub app: String,
    pub uri: String,
    pub meta: AppMeta,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct McpAppInventoryItem {
    pub workspace: String,
    pub app: String,
    pub valid: bool,
    pub status: String,
    pub reason: Option<String>,
    pub uri: Option<String>,
    pub meta: Option<AppMeta>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct WatchSubscriptionSummary {
    pub cursor: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct WatchBatchSummary {
    pub events: Vec<DataChangeSummary>,
    pub next: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct SubstrateChangesSummary {
    pub events: Vec<SubstrateChangeSummary>,
    pub next: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DataChangeSummary {
    pub workspace: String,
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub commit: String,
    pub parent: Option<String>,
    pub seq: u64,
    pub changes: Vec<DomainChangeSummary>,
    pub unsupported_domains: Vec<UnsupportedDomainSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubstrateChangeSummary {
    Data {
        workspace: String,
        #[serde(rename = "ref")]
        ref_name: String,
        commit: String,
        parent: Option<String>,
        seq: u64,
        changes: Vec<DomainChangeSummary>,
        unsupported_domains: Vec<UnsupportedDomainSummary>,
        lmdiff: Option<Vec<u8>>,
    },
    Operation {
        #[serde(rename = "workspace_id")]
        workspace_id: String,
        app_id: String,
        scope_id: String,
        operation_id: String,
        operation_kind: String,
        sequence: u64,
        actor_principal: String,
        timestamp_ms: u64,
        root_after: String,
        target_entity_id: Option<String>,
        payload_digest: String,
        policy_labels: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DomainChangeSummary {
    pub domain: String,
    pub schema_version: u32,
    pub kind: String,
    pub key: Vec<u8>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub detail: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct UnsupportedDomainSummary {
    pub domain: String,
    pub capability: String,
}

/// Parse a `YYYYMMDDTHHMMSS` calendar window bound into a `DateTime`, mirroring the C ABI.
fn parse_window_bound(s: &str) -> Result<DateTime> {
    let bad = || {
        LoomError::invalid(format!(
            "calendar window bound must be YYYYMMDDTHHMMSS, got {s:?}"
        ))
    };
    let b = s.as_bytes();
    if b.len() != 15
        || b[8] != b'T'
        || !s[0..8]
            .bytes()
            .chain(s[9..15].bytes())
            .all(|c| c.is_ascii_digit())
    {
        return Err(bad());
    }
    let n = |r: std::ops::Range<usize>| s[r].parse::<u32>().map_err(|_| bad());
    let month =
        IcalMonth::try_from(u8::try_from(n(4..6)?).map_err(|_| bad())?).map_err(|_| bad())?;
    let date = IcalDate::from_calendar_date(
        i32::try_from(n(0..4)?).map_err(|_| bad())?,
        month,
        u8::try_from(n(6..8)?).map_err(|_| bad())?,
    )
    .map_err(|_| bad())?;
    let time = IcalTime::from_hms(
        u8::try_from(n(9..11)?).map_err(|_| bad())?,
        u8::try_from(n(11..13)?).map_err(|_| bad())?,
        u8::try_from(n(13..15)?).map_err(|_| bad())?,
    )
    .map_err(|_| bad())?;
    Ok(DateTime::new(date, time))
}

/// Parse a calendar component filter (`""` -> all, `event`/`todo`).
fn parse_component_filter(component: &str) -> Result<Option<Component>> {
    match component {
        "" => Ok(None),
        "event" => Ok(Some(Component::Event)),
        "todo" => Ok(Some(Component::Todo)),
        other => Err(LoomError::invalid(format!(
            "unknown component filter {other:?}"
        ))),
    }
}

fn filter_kv_range_predicate(map: KvMap, predicate: Option<&JsonValue>) -> Result<KvMap> {
    let Some(predicate) = predicate else {
        return Ok(map);
    };
    if predicate.is_null() {
        return Ok(map);
    }
    let simple = Predicate::from_json_value(predicate)?
        .as_simple_comparison()
        .ok_or_else(|| {
            LoomError::invalid("kv.range predicate must be a single key comparison expression")
        })?;
    if simple.path != ["key"] {
        return Err(LoomError::invalid(
            "kv.range predicate path must be [\"key\"] because KV values are opaque bytes",
        ));
    }
    let rhs = simple.value.to_tabular_value()?;
    let mut out = KvMap::new();
    for (key, value) in map.iter() {
        if compare_tabular_values(key, simple.op, &rhs) {
            out.put(key.clone(), value.to_vec());
        }
    }
    Ok(out)
}

fn compare_tabular_values(left: &TabularValue, op: CompareOp, right: &TabularValue) -> bool {
    match op {
        CompareOp::Eq => left == right,
        CompareOp::Ne => left != right,
        CompareOp::Lt => left < right,
        CompareOp::Lte => left <= right,
        CompareOp::Gt => left > right,
        CompareOp::Gte => left >= right,
    }
}

/// Resolve a workspace argument (id or name) to a [`WorkspaceId`], mirroring the C ABI resolver.
pub(crate) fn resolve_ns(loom: &Loom<FileStore>, workspace: &str) -> Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Name(workspace.to_string()),
    };
    loom.registry().open(&selector)
}

fn workspace_label(loom: &Loom<FileStore>, ns: WorkspaceId) -> Option<String> {
    loom.registry()
        .list(None)
        .into_iter()
        .find(|info| info.id == ns)
        .map(|info| info.name)
}

/// The reserved working-tree path of a SQL table inside database `db`. The structured table readers
/// address a table by `(db, table)`; the database is an explicit parameter, never a hard-coded default.
/// The same `.loom/facets/<facet>/<collection>/tables/<table>` shape is shared by table facets.
fn sql_table_path(db: &str, table: &str) -> String {
    format!(".loom/facets/sql/{db}/tables/{table}")
}

/// Project a typed [`WatchBatch`] into the MCP `WatchBatchSummary` shape. Shared by the local read path
/// (which polls the engine directly) and the remote path (which decodes the batch wire via
/// `watch_batch_from_cbor`), so both produce byte-for-byte identical summaries including each event's
/// `parent`.
fn watch_batch_summary(batch: WatchBatch) -> WatchBatchSummary {
    WatchBatchSummary {
        events: batch
            .events
            .into_iter()
            .map(|event| DataChangeSummary {
                workspace: event.workspace.to_string(),
                ref_name: event.branch,
                commit: event.commit.to_string(),
                parent: event.parent.map(|parent| parent.to_string()),
                seq: event.seq,
                changes: event
                    .changes
                    .into_iter()
                    .map(domain_change_summary)
                    .collect(),
                unsupported_domains: event
                    .unsupported_domains
                    .into_iter()
                    .map(unsupported_domain_summary)
                    .collect(),
            })
            .collect(),
        next: batch.next.encode(),
    }
}

fn parse_watch_change_kind(kind: &str) -> Result<VcsChangeKind> {
    match kind {
        "added" => Ok(VcsChangeKind::Added),
        "modified" => Ok(VcsChangeKind::Modified),
        "deleted" => Ok(VcsChangeKind::Deleted),
        _ => Err(LoomError::invalid(format!(
            "watch change kind must be added, modified, or deleted, got {kind:?}"
        ))),
    }
}

fn domain_change_summary(change: DomainChange) -> DomainChangeSummary {
    DomainChangeSummary {
        domain: change.domain,
        schema_version: change.schema_version,
        kind: change.kind,
        key: change.key,
        before: change.before.map(|digest| digest.to_string()),
        after: change.after.map(|digest| digest.to_string()),
        detail: change.detail,
    }
}

fn operation_change_summary(change: OperationChangeRecord) -> SubstrateChangeSummary {
    SubstrateChangeSummary::Operation {
        workspace_id: change.workspace_id,
        app_id: change.app_id,
        scope_id: change.scope_id,
        operation_id: change.operation_id,
        operation_kind: change.operation_kind,
        sequence: change.sequence,
        actor_principal: change.actor_principal,
        timestamp_ms: change.timestamp_ms,
        root_after: change.root_after.to_string(),
        target_entity_id: change.target_entity_id,
        payload_digest: change.payload_digest.to_string(),
        policy_labels: change.policy_labels,
    }
}

fn unsupported_domain_summary(domain: UnsupportedDomainDetail) -> UnsupportedDomainSummary {
    UnsupportedDomainSummary {
        domain: domain.domain,
        capability: domain.capability,
    }
}

fn snippet(text: &str, start: usize, end: usize) -> String {
    let window_start = text[..start]
        .char_indices()
        .rev()
        .nth(40)
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    let window_end = text[end..]
        .char_indices()
        .nth(40)
        .map(|(idx, _)| end + idx)
        .unwrap_or(text.len());
    text[window_start..window_end].to_string()
}

fn app_inventory_error(
    workspace: &str,
    app: &str,
    status: &str,
    reason: impl Into<String>,
) -> McpAppInventoryItem {
    McpAppInventoryItem {
        workspace: workspace.to_string(),
        app: app.to_string(),
        valid: false,
        status: status.to_string(),
        reason: Some(reason.into()),
        uri: None,
        meta: None,
    }
}

fn internal_app_inventory_item(workspace: &str, app: &str) -> Option<McpAppInventoryItem> {
    let meta = apps::internal_app_meta(app)?.ok()?;
    Some(McpAppInventoryItem {
        workspace: workspace.to_string(),
        app: app.to_string(),
        valid: true,
        status: "valid".to_string(),
        reason: None,
        uri: Some(apps::app_uri(workspace, app, false)),
        meta: Some(meta),
    })
}

fn internal_app_resource(workspace: &str, app: &str) -> Option<McpAppResource> {
    let meta = apps::internal_app_meta(app)?.ok()?;
    Some(McpAppResource {
        workspace: workspace.to_string(),
        app: app.to_string(),
        uri: apps::app_uri(workspace, app, false),
        meta,
    })
}

fn mcp_app_inventory_item(
    loom: &Loom<FileStore>,
    ns: WorkspaceId,
    workspace: &str,
    app: &str,
) -> McpAppInventoryItem {
    if let Err(e) = apps::validate_app_name(app) {
        return app_inventory_error(workspace, app, "invalid_app_name", e.to_string());
    }
    let meta_path = apps::meta_path(app);
    let meta_bytes = match loom.read_file(ns, &meta_path) {
        Ok(bytes) => bytes,
        Err(e) if e.code == Code::NotFound => {
            return app_inventory_error(workspace, app, "missing_meta", "_meta.md is missing");
        }
        Err(e) => return app_inventory_error(workspace, app, "unreadable_meta", e.to_string()),
    };
    let meta_text = match String::from_utf8(meta_bytes) {
        Ok(text) => text,
        Err(e) => return app_inventory_error(workspace, app, "non_utf8_meta", e.to_string()),
    };
    let meta = match apps::parse_meta(app, &meta_text) {
        Ok(meta) => meta,
        Err(e) => return app_inventory_error(workspace, app, "malformed_meta", e.to_string()),
    };
    let index_path = apps::index_path(app);
    let index_bytes = match loom.read_file(ns, &index_path) {
        Ok(bytes) => bytes,
        Err(e) if e.code == Code::NotFound => {
            return app_inventory_error(workspace, app, "missing_index", "index.html is missing");
        }
        Err(e) => return app_inventory_error(workspace, app, "unreadable_index", e.to_string()),
    };
    if let Err(e) = String::from_utf8(index_bytes) {
        return app_inventory_error(workspace, app, "non_utf8_index", e.to_string());
    }
    McpAppInventoryItem {
        workspace: workspace.to_string(),
        app: app.to_string(),
        valid: true,
        status: "valid".to_string(),
        reason: None,
        uri: Some(apps::app_uri(workspace, app, false)),
        meta: Some(meta),
    }
}

type ProfileOperationChangeLoader = fn(
    &Loom<FileStore>,
    WorkspaceId,
    &OperationChangeCursor,
    usize,
) -> Result<OperationChangeBatch>;

const PROFILE_OPERATION_CHANGE_LOADERS: &[(&str, ProfileOperationChangeLoader)] = &[
    ("chat:", crate::chat::operation_changes),
    ("tickets:", loom_tickets::operation_changes),
    ("pages:", crate::pages::operation_changes),
    ("workgraph:", workgraph_operation_changes),
];

fn profile_operation_changes(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    cursor: &OperationChangeCursor,
    max: usize,
) -> Result<OperationChangeBatch> {
    for (prefix, loader) in PROFILE_OPERATION_CHANGE_LOADERS {
        if cursor.scope_id.starts_with(prefix) {
            return loader(loom, workspace, cursor, max);
        }
    }
    Err(LoomError::invalid("unsupported profile operation cursor"))
}

fn workgraph_operation_changes(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    cursor: &OperationChangeCursor,
    max: usize,
) -> Result<OperationChangeBatch> {
    let Some(workspace_id) = cursor.scope_id.strip_prefix("workgraph:") else {
        return Err(LoomError::invalid("invalid workgraph operation cursor"));
    };
    let log = match loom
        .store()
        .control_get(&workgraph_operation_log_key(workspace_id)?)?
    {
        Some(bytes) => WorkgraphOperationLog::decode(&bytes)?,
        None => WorkgraphOperationLog::new(workspace_id, Vec::new())?,
    };
    let mut batch = log.changes(cursor, max)?;
    let mut events = Vec::new();
    for event in batch.events {
        if workgraph_operation_change_is_readable(loom, workspace, &event)? {
            events.push(event);
        }
    }
    batch.events = events;
    Ok(batch)
}

fn workgraph_operation_change_is_readable(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    event: &OperationChangeRecord,
) -> Result<bool> {
    let Some(target) = event.target_entity_id.as_deref() else {
        return Ok(false);
    };
    let Some(task_id) = target.strip_prefix("workgraph:") else {
        return Ok(false);
    };
    match authorize_workgraph_task(loom, workspace, task_id, AclRight::Read) {
        Ok(()) => Ok(true),
        Err(e) if e.code == Code::PermissionDenied => Ok(false),
        Err(e) => Err(e),
    }
}

fn record_board_read_observed(
    loom: &mut Loom<FileStore>,
    ns: WorkspaceId,
    board_id: &str,
    bytes: &[u8],
) -> Result<()> {
    let board: JsonValue = serde_json::from_slice(bytes)
        .map_err(|e| LoomError::invalid(format!("board document must be JSON: {e}")))?;
    let task_id = board
        .get("current_task")
        .and_then(JsonValue::as_str)
        .unwrap_or(board_id);
    authorize_workgraph_task(loom, ns, task_id, AclRight::Read)?;
    let batch_id = board
        .get("current_batch")
        .and_then(JsonValue::as_str)
        .unwrap_or("workgraph");
    let payload_digest = Digest::hash(Algo::Blake3, bytes);
    let event_id = format!("board-read-observed:{board_id}:{payload_digest}");
    let fact = WorkgraphFact {
        event_id: event_id.clone(),
        occurred_at: now_ms(),
        task_id: task_id.to_string(),
        batch_id: batch_id.to_string(),
        actor_kind: ActorKind::Service,
        actor_id: "mcp-server".to_string(),
        correlation_id: board_id.to_string(),
        causation_id: payload_digest.to_string(),
        attempt: 1,
        previous_state: WorkgraphState::Assigned,
        next_state: WorkgraphState::Assigned,
        payload_digest,
        reason_code: None,
        kind: WorkgraphFactKind::BoardReadObserved,
    };
    let key = workgraph_operation_log_key(batch_id)?;
    let mut log = match loom.store().control_get(&key)? {
        Some(bytes) => WorkgraphOperationLog::decode(&bytes)?,
        None => WorkgraphOperationLog::new(batch_id, Vec::new())?,
    };
    if log.records.iter().any(|record| {
        record.event_id == event_id
            || (record.fact.task_id == fact.task_id
                && record.fact.previous_state == fact.previous_state
                && record.fact.next_state == fact.next_state
                && record.fact.kind == fact.kind)
    }) {
        return Ok(());
    }
    let sequence = log
        .records
        .last()
        .map(|record| record.sequence + 1)
        .unwrap_or(1);
    let fact_bytes = fact.encode()?;
    let previous = log.encode()?;
    let base_root = Digest::hash(Algo::Blake3, &previous);
    let mut root_input = previous;
    root_input.extend_from_slice(&fact_bytes);
    let root_after = Digest::hash(Algo::Blake3, &root_input);
    let operation_kind = workgraph_operation_kind(fact.kind);
    let target_entity_id = format!("workgraph:{task_id}");
    let envelope = OperationEnvelope::new(
        Algo::Blake3,
        OperationEnvelopeInput {
            workspace_id: batch_id,
            app_id: "workgraph",
            scope_id: &workgraph_operation_cursor_scope(batch_id),
            operation_id: &event_id,
            operation_kind: &operation_kind,
            sequence,
            actor_principal: ns,
            actor_kind: ActorKind::Service,
            timestamp_ms: now_ms(),
            idempotency_key: &event_id,
            base_root,
            base_entity_version: None,
            target_entity_id: Some(&target_entity_id),
            payload: &fact_bytes,
            policy_labels: &[],
            signature: None,
            agent: None,
        },
    )?;
    let record = WorkgraphOperationRecord::fact(sequence, fact, root_after, envelope.encode()?)?;
    log.append(record)?;
    loom.store().control_set(&key, log.encode()?)
}

impl LoomMcp {
    // ---- store ----

    /// `store_capabilities`: the engine capability registry plus this host's overlays.
    pub fn read_capabilities(&self) -> Result<Vec<u8>> {
        self.store
            .read(|loom| Ok(crate::served_capabilities(loom.capabilities()).to_cbor()))
    }

    pub fn read_capabilities_json(&self, detailed: bool) -> Result<String> {
        self.store.read(|loom| {
            let visibility = if detailed {
                loom_core::CapabilityVisibility::Detailed
            } else {
                loom_core::CapabilityVisibility::Default
            };
            Ok(crate::served_capabilities(loom.capabilities()).to_json(visibility))
        })
    }

    /// `store_blob_digest`: the Blob content address (`"algo:hex"`) of `data`. Pure; no store access.
    pub fn read_blob_digest(&self, data: &[u8]) -> String {
        Object::Blob(data.to_vec()).digest().to_string()
    }

    // ---- workspace ----

    /// Every workspace in the registry.
    pub fn read_workspace_list(&self) -> Result<Vec<WorkspaceSummary>> {
        self.store.read_registry(|loom| {
            loom.authorize_global_admin()?;
            Ok(loom
                .registry()
                .list(None)
                .iter()
                .map(WorkspaceSummary::from_info)
                .collect())
        })
    }

    /// One workspace by id or name, or `None`.
    pub fn read_workspace_get(&self, workspace: &str) -> Result<Option<WorkspaceSummary>> {
        self.store.read_registry(|loom| {
            loom.authorize_global_admin()?;
            Ok(loom
                .registry()
                .list(None)
                .iter()
                .find(|n| n.id.to_string() == workspace || n.name == workspace)
                .map(WorkspaceSummary::from_info))
        })
    }

    pub fn read_mcp_app_list(&self) -> Result<Vec<McpAppResource>> {
        self.store.read(|loom| {
            loom.authorize_global_admin()?;
            let mut out = Vec::new();
            for info in loom.registry().list(None) {
                if let Ok(entries) = loom.list_directory(info.id, apps::APP_ROOT) {
                    for entry in entries {
                        if entry.kind != FileKind::Directory
                            || apps::validate_app_name(&entry.name).is_err()
                        {
                            continue;
                        }
                        let meta_path = apps::meta_path(&entry.name);
                        let index_path = apps::index_path(&entry.name);
                        let Ok(meta_bytes) = loom.read_file(info.id, &meta_path) else {
                            continue;
                        };
                        let Ok(index_bytes) = loom.read_file(info.id, &index_path) else {
                            continue;
                        };
                        if String::from_utf8(index_bytes).is_err() {
                            continue;
                        }
                        let Ok(meta_text) = String::from_utf8(meta_bytes) else {
                            continue;
                        };
                        let Ok(meta) = apps::parse_meta(&entry.name, &meta_text) else {
                            continue;
                        };
                        out.push(McpAppResource {
                            workspace: info.name.clone(),
                            app: entry.name.clone(),
                            uri: apps::app_uri(&info.name, &entry.name, false),
                            meta,
                        });
                    }
                }
                for app in apps::INTERNAL_APPS {
                    if let Some(resource) = internal_app_resource(&info.name, app) {
                        out.push(resource);
                    }
                }
            }
            Ok(out)
        })
    }

    pub fn read_mcp_app_inventory(&self, workspace: &str) -> Result<Vec<McpAppInventoryItem>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let workspace_name = loom
                .registry()
                .list(None)
                .into_iter()
                .find(|info| info.id == ns)
                .map(|info| info.name)
                .unwrap_or_else(|| workspace.to_string());
            let mut out = Vec::new();
            let entries = match loom.list_directory(ns, apps::APP_ROOT) {
                Ok(entries) => entries,
                Err(e) if e.code == Code::NotFound => {
                    for app in apps::INTERNAL_APPS {
                        if let Some(item) = internal_app_inventory_item(&workspace_name, app) {
                            out.push(item);
                        }
                    }
                    return Ok(out);
                }
                Err(e) => return Err(e),
            };
            for entry in entries {
                if entry.kind != FileKind::Directory {
                    continue;
                }
                out.push(mcp_app_inventory_item(
                    loom,
                    ns,
                    &workspace_name,
                    &entry.name,
                ));
            }
            for app in apps::INTERNAL_APPS {
                if let Some(item) = internal_app_inventory_item(&workspace_name, app) {
                    out.push(item);
                }
            }
            Ok(out)
        })
    }

    pub fn read_mcp_app_show(&self, workspace: &str, app: &str) -> Result<Option<McpAppResource>> {
        if apps::is_internal_app(app) {
            return self.store.read(|loom| {
                let ns = resolve_ns(loom, workspace)?;
                loom.status(ns)?;
                let workspace_name = loom
                    .registry()
                    .list(None)
                    .into_iter()
                    .find(|info| info.id == ns)
                    .map(|info| info.name)
                    .unwrap_or_else(|| workspace.to_string());
                Ok(internal_app_resource(&workspace_name, app))
            });
        }
        apps::validate_app_name(app)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let workspace_name = loom
                .registry()
                .list(None)
                .into_iter()
                .find(|info| info.id == ns)
                .map(|info| info.name)
                .unwrap_or_else(|| workspace.to_string());
            let item = mcp_app_inventory_item(loom, ns, &workspace_name, app);
            Ok(item.valid.then_some(McpAppResource {
                workspace: workspace_name,
                app: app.to_string(),
                uri: item.uri.expect("valid app inventory item has a URI"),
                meta: item.meta.expect("valid app inventory item has metadata"),
            }))
        })
    }

    pub fn read_mcp_app_html(&self, workspace: &str, app: &str) -> Result<(String, AppMeta)> {
        if apps::is_internal_app(app) {
            return self.store.read(|loom| {
                let ns = resolve_ns(loom, workspace)?;
                loom.status(ns)?;
                let (html, meta) = apps::internal_app_html(app)
                    .ok_or_else(|| LoomError::not_found(format!("unknown internal app {app}")))?;
                Ok((html.to_string(), meta?))
            });
        }
        apps::validate_app_name(app)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let meta_bytes = loom.read_file(ns, &apps::meta_path(app))?;
            let meta_text = String::from_utf8(meta_bytes)
                .map_err(|_| LoomError::invalid("app metadata is not UTF-8"))?;
            let meta = apps::parse_meta(app, &meta_text)?;
            let html_bytes = loom.read_file(ns, &apps::index_path(app))?;
            let html = String::from_utf8(html_bytes)
                .map_err(|_| LoomError::invalid("app index.html is not UTF-8"))?;
            Ok((html, meta))
        })
    }

    pub fn read_mcp_app_file(
        &self,
        workspace: &str,
        app: &str,
        path: &str,
    ) -> Result<Option<Vec<u8>>> {
        if apps::is_internal_app(app) {
            apps::validate_app_file_path(path)?;
            return self.store.read(|loom| {
                let ns = resolve_ns(loom, workspace)?;
                loom.status(ns)?;
                Ok(apps::internal_app_file(app, path).map(Vec::from))
            });
        }
        let path = apps::app_file_path(app, path)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            match loom.read_file(ns, &path) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) if e.code == Code::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        })
    }

    // ---- cas ----

    /// `cas_get`: the blob bytes for `digest`, or `None` if unreferenced in the workspace.
    pub fn read_cas_get(&self, workspace: &str, digest: &str) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.cas_get(workspace, digest);
        }
        let digest = Digest::parse(digest)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            cas_get(loom, ns, &digest)
        })
    }

    /// `cas_has`: whether `digest` is reachable in the workspace.
    pub fn read_cas_has(&self, workspace: &str, digest: &str) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.cas_has(workspace, digest);
        }
        let digest = Digest::parse(digest)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            cas_has(loom, ns, &digest)
        })
    }

    /// `cas_list`: the content addresses reachable in the workspace.
    pub fn read_cas_list(&self, workspace: &str) -> Result<Vec<String>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.cas_list(workspace);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(cas_list(loom, ns)?.iter().map(|d| d.to_string()).collect())
        })
    }

    // ---- graph ----

    /// `graph_get_node`: node properties as canonical CBOR, or `None`.
    pub fn read_graph_get_node(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.graph_get_node(workspace, name, id);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            graph_get_node(loom, ns, name, id)?
                .map(|props| props_to_cbor(&props))
                .transpose()
        })
    }

    /// `graph_get_edge`: edge as canonical CBOR `[src, dst, label, props]`, or `None`.
    pub fn read_graph_get_edge(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.graph_get_edge(workspace, name, id);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            graph_get_edge(loom, ns, name, id)?
                .map(|edge| graph_edge_cbor(&edge))
                .transpose()
        })
    }

    /// `graph_neighbors`: adjacent node ids as canonical CBOR text array.
    pub fn read_graph_neighbors(&self, workspace: &str, name: &str, id: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.graph_neighbors(workspace, name, id);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            graph_strings_cbor(graph_neighbors(loom, ns, name, id)?)
        })
    }

    /// `graph_out_edges`: outgoing edges as canonical CBOR.
    pub fn read_graph_out_edges(&self, workspace: &str, name: &str, id: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.graph_out_edges(workspace, name, id);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            graph_edges_cbor(graph_out_edges(loom, ns, name, id)?)
        })
    }

    /// `graph_in_edges`: incoming edges as canonical CBOR.
    pub fn read_graph_in_edges(&self, workspace: &str, name: &str, id: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.graph_in_edges(workspace, name, id);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            graph_edges_cbor(graph_in_edges(loom, ns, name, id)?)
        })
    }

    /// `graph_reachable`: reachable node ids as canonical CBOR text array.
    pub fn read_graph_reachable(
        &self,
        workspace: &str,
        name: &str,
        start: &str,
        max_depth: i64,
        via_label: Option<&str>,
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.graph_reachable(
                workspace,
                name,
                start,
                max_depth,
                via_label.unwrap_or(""),
            );
        }
        let max_depth = if max_depth < 0 {
            None
        } else {
            Some(
                usize::try_from(max_depth)
                    .map_err(|_| LoomError::invalid("graph max_depth out of range"))?,
            )
        };
        let via_label = via_label.filter(|s| !s.is_empty());
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            graph_strings_cbor(graph_reachable(
                loom, ns, name, start, max_depth, via_label,
            )?)
        })
    }

    /// `graph_shortest_path`: shortest path as canonical CBOR text array, or `None`.
    pub fn read_graph_shortest_path(
        &self,
        workspace: &str,
        name: &str,
        from: &str,
        to: &str,
        via_label: Option<&str>,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.graph_shortest_path(workspace, name, from, to, via_label.unwrap_or(""));
        }
        let via_label = via_label.filter(|s| !s.is_empty());
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            graph_shortest_path(loom, ns, name, from, to, via_label)?
                .map(graph_strings_cbor)
                .transpose()
        })
    }

    pub fn read_graph_query(&self, workspace: &str, name: &str, query: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.graph_query(workspace, name, query);
        }
        let query = GraphQuery::parse_opencypher(query)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let result = graph_query(loom, ns, name, &query)?;
            Ok(loom_wire::graph::graph_query_result_to_cbor(&result))
        })
    }

    pub fn read_graph_explain_query(
        &self,
        workspace: &str,
        name: &str,
        query: &str,
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.graph_explain_query(workspace, name, query);
        }
        let query = GraphQuery::parse_opencypher(query)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let explain = graph_explain_query(loom, ns, name, &query)?;
            Ok(loom_wire::graph::graph_query_explain_to_cbor(&explain))
        })
    }

    // ---- vector ----

    /// `vector_get`: vector entry as canonical CBOR `[vector_bytes, metadata]`, or `None`.
    pub fn read_vector_get(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vector_get(workspace, name, id);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            vector_get(loom, ns, name, id)?
                .map(|(vector, metadata)| vector_entry_cbor(vector, metadata))
                .transpose()
        })
    }

    /// `vector_source_text`: stored UTF-8 source bytes, or `None`.
    pub fn read_vector_source_text(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vector_source_text(workspace, name, id);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(vector_source_text(loom, ns, name, id)?.map(String::into_bytes))
        })
    }

    /// `vector_embedding_model`: model profile as canonical CBOR, or `None`.
    pub fn read_vector_embedding_model(
        &self,
        workspace: &str,
        name: &str,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vector_embedding_model(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            vector_embedding_model(loom, ns, name)?
                .map(|model| vector_embedding_model_cbor(&model))
                .transpose()
        })
    }

    /// `vector_ids`: ids as canonical CBOR text array.
    pub fn read_vector_ids(
        &self,
        workspace: &str,
        name: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vector_ids(workspace, name, prefix);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            vector_strings_cbor(vector_ids(loom, ns, name, prefix)?)
        })
    }

    /// `vector_metadata_index_keys`: declared metadata-index keys as canonical CBOR text array.
    pub fn read_vector_metadata_index_keys(&self, workspace: &str, name: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vector_metadata_index_keys(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            vector_strings_cbor(vector_metadata_index_keys(loom, ns, name)?)
        })
    }

    /// `vector_search`: exact hits as canonical CBOR.
    pub fn read_vector_search(
        &self,
        workspace: &str,
        name: &str,
        query: &[u8],
        k: u64,
        filter: &[u8],
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vector_search(workspace, name, query, k, filter);
        }
        let query = vector_from_bytes(query)?;
        let k = usize::try_from(k).map_err(|_| LoomError::invalid("vector k out of range"))?;
        let filter = vector_filter_from_cbor(filter)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            vector_hits_cbor(&vector_search(loom, ns, name, &query, k, &filter)?)
        })
    }

    /// `vector_search_policy`: policy-selected exact/PQ hits as canonical CBOR.
    pub fn read_vector_search_policy(&self, args: VectorSearchPolicyRead<'_>) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vector_search_policy(
                args.workspace,
                args.name,
                crate::RemoteVectorSearchPolicy {
                    query: args.query,
                    k: args.k,
                    filter: args.filter,
                    policy: args.policy,
                    threshold: args.threshold,
                    ef: args.ef,
                    pq_m: args.pq_m,
                    pq_k: args.pq_k,
                    pq_iters: args.pq_iters,
                },
            );
        }
        let query = vector_from_bytes(args.query)?;
        let k = usize::try_from(args.k).map_err(|_| LoomError::invalid("vector k out of range"))?;
        let filter = vector_filter_from_cbor(args.filter)?;
        let threshold = usize::try_from(args.threshold)
            .map_err(|_| LoomError::invalid("vector threshold out of range"))?;
        let ef =
            usize::try_from(args.ef).map_err(|_| LoomError::invalid("vector ef out of range"))?;
        let pq_m = usize::try_from(args.pq_m)
            .map_err(|_| LoomError::invalid("vector pq_m out of range"))?;
        let pq_k = usize::try_from(args.pq_k)
            .map_err(|_| LoomError::invalid("vector pq_k out of range"))?;
        let pq_iters = usize::try_from(args.pq_iters)
            .map_err(|_| LoomError::invalid("vector pq_iters out of range"))?;
        let policy = vector_policy(args.policy, threshold)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, args.workspace)?;
            vector_hits_cbor(&vector_search_with_pq_policy(
                loom, ns, args.name, &query, k, &filter, policy, ef, pq_m, pq_k, pq_iters,
            )?)
        })
    }

    // ---- columnar ----

    /// `columnar_scan`: all rows as canonical CBOR row array.
    pub fn read_columnar_scan(&self, workspace: &str, name: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.columnar_scan(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            columnar_rows_cbor(columnar_scan(loom, ns, name)?)
        })
    }

    /// `columnar_columns`: columns as canonical CBOR `[[name, type_tag] ...]`.
    pub fn read_columnar_columns(&self, workspace: &str, name: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.columnar_columns(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            columnar_columns_cbor(columnar_columns(loom, ns, name)?)
        })
    }

    /// `columnar_rows`: row count.
    pub fn read_columnar_rows(&self, workspace: &str, name: &str) -> Result<u64> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.columnar_rows(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(columnar_rows(loom, ns, name)? as u64)
        })
    }

    /// `columnar_inspect`: dataset metadata as canonical CBOR.
    pub fn read_columnar_inspect(&self, workspace: &str, name: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.columnar_inspect(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            columnar_inspect_cbor(columnar_inspect(loom, ns, name)?)
        })
    }

    /// `columnar_source_digest`: source digest text for derived projections.
    pub fn read_columnar_source_digest(&self, workspace: &str, name: &str) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.columnar_source_digest(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(columnar_source_digest(loom, ns, name)?.to_string())
        })
    }

    /// `columnar_select`: selected rows as canonical CBOR row array.
    pub fn read_columnar_select(
        &self,
        workspace: &str,
        name: &str,
        columns: &[u8],
        filter: &[u8],
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.columnar_select(workspace, name, columns, filter);
        }
        let columns = columnar_select_columns_from_cbor(columns)?;
        let filter = columnar_filter_from_cbor(filter)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let column_refs = columns.iter().map(String::as_str).collect::<Vec<_>>();
            let filter_ref = filter
                .as_ref()
                .map(|(column, op, value)| (column.as_str(), *op, value));
            columnar_rows_cbor(columnar_select(loom, ns, name, &column_refs, filter_ref)?)
        })
    }

    /// `columnar_aggregate`: aggregate values as a canonical CBOR cell array.
    pub fn read_columnar_aggregate(
        &self,
        workspace: &str,
        name: &str,
        aggregates: &[u8],
        filter: &[u8],
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.columnar_aggregate(workspace, name, aggregates, filter);
        }
        let aggregates = columnar_aggregates_from_cbor(aggregates)?;
        let filter = columnar_filter_from_cbor(filter)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let filter_ref = filter
                .as_ref()
                .map(|(column, op, value)| (column.as_str(), *op, value));
            columnar_values_cbor(columnar_aggregate(loom, ns, name, &aggregates, filter_ref)?)
        })
    }

    // ---- dataframe ----

    /// `dataframe_collect`: execute a frame and return `[columns, rows]` canonical CBOR.
    pub fn read_dataframe_collect(&self, workspace: &str, name: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.dataframe_collect(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            dataframe_batch_cbor(dataframe_collect(loom, ns, name)?)
        })
    }

    /// `dataframe_preview`: execute a frame and return at most `rows` rows.
    pub fn read_dataframe_preview(
        &self,
        workspace: &str,
        name: &str,
        rows: u64,
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.dataframe_preview(workspace, name, rows);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            dataframe_batch_cbor(dataframe_preview(loom, ns, name, rows)?)
        })
    }

    /// `dataframe_plan_digest`: canonical dataframe plan digest as `algo:hex`.
    pub fn read_dataframe_plan_digest(&self, workspace: &str, name: &str) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.dataframe_plan_digest(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(dataframe_plan_digest(loom, ns, name)?.to_string())
        })
    }

    /// `dataframe_source_digests`: plan-pinned source digests as canonical CBOR text array.
    pub fn read_dataframe_source_digests(&self, workspace: &str, name: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.dataframe_source_digests(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            digest_strings_cbor(dataframe_source_digests(loom, ns, name)?)
        })
    }

    // ---- fts ----

    /// `fts_get`: document as canonical CBOR, or `None`.
    pub fn read_fts_get(&self, workspace: &str, name: &str, id: &[u8]) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.search_get(workspace, name, id);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            search_get(loom, ns, name, id)?
                .map(|doc| search_document_cbor(&doc))
                .transpose()
        })
    }

    /// `fts_ids`: ids as canonical CBOR byte array.
    pub fn read_fts_ids(
        &self,
        workspace: &str,
        name: &str,
        prefix: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.search_ids(workspace, name, prefix);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            search_ids_cbor(search_ids(loom, ns, name, prefix)?)
        })
    }

    /// `fts_query`: response as canonical CBOR `[reduced, hits]`.
    pub fn read_fts_query(&self, workspace: &str, name: &str, request: &[u8]) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.search_query(workspace, name, request);
        }
        let request = search_request_from_cbor(request)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            search_response_cbor(&search_query(loom, ns, name, &request)?)
        })
    }

    pub fn read_fts_source_digest(&self, workspace: &str, name: &str) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.search_source_digest(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(search_source_digest(loom, ns, name)?.to_string())
        })
    }

    pub fn read_fts_status(
        &self,
        workspace: &str,
        name: &str,
        engine_version: &str,
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.search_status(workspace, name, engine_version);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let source_digest = search_source_digest(loom, ns, name)?;
            let status =
                loom.store()
                    .search_tantivy_status(ns, name, source_digest, engine_version)?;
            loom_store::encode_search_status_result(&source_digest, &status)
        })
    }

    pub fn read_fts_search(&self, request: FtsSearchReadRequest<'_>) -> Result<SearchResult> {
        if request.query.is_empty() && request.query_vector.is_none() {
            return Err(LoomError::invalid(
                "fts search requires query or query_vector",
            ));
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, request.workspace)?;
            let root = loom
                .registry()
                .list(None)
                .into_iter()
                .find(|info| info.id == ns)
                .and_then(|info| info.head)
                .map(|digest| digest.to_string());
            if let Some(vector) = request.query_vector {
                return read_substrate_semantic_search(
                    loom,
                    SubstrateSemanticSearchArgs {
                        ns,
                        workspace: request.workspace,
                        name: request.name,
                        query_vector: vector,
                        query_model_id: request.query_model_id,
                        query_weights_digest: request.query_weights_digest,
                        field: request.field,
                        limit: request.limit,
                        offset: request.offset,
                        root,
                    },
                );
            }
            let lowered = request.query.to_ascii_lowercase();
            let mut hits = Vec::new();
            for id in search_ids(loom, ns, request.name, None)? {
                let Some(doc) = search_get(loom, ns, request.name, &id)? else {
                    continue;
                };
                for (field_name, value) in doc {
                    if request.field.is_some_and(|wanted| wanted != field_name) {
                        continue;
                    }
                    let FieldValue::Text(text) = value else {
                        continue;
                    };
                    let text_lower = text.to_ascii_lowercase();
                    let Some(byte_start) = text_lower.find(&lowered) else {
                        continue;
                    };
                    let byte_end = byte_start + lowered.len();
                    hits.push(SearchHit {
                        facet: "search".to_string(),
                        workspace: request.workspace.to_string(),
                        collection: request.name.to_string(),
                        entity_id: hex::encode(&id),
                        field: field_name,
                        snippet: snippet(&text, byte_start, byte_end),
                        offsets: vec![[byte_start as u64, byte_end as u64]],
                        scope_context: SearchScopeContext {
                            owning_entity: None,
                            status_fields: Vec::new(),
                            refs: Vec::new(),
                        },
                        root: root.clone(),
                        match_via: "lexical".to_string(),
                        contributing_rungs: vec!["lexical".to_string()],
                        fused_score: 1.0,
                        raw_score: 1.0,
                        rung: "lexical".to_string(),
                    });
                }
            }
            hits.sort_by(|a, b| {
                a.entity_id
                    .cmp(&b.entity_id)
                    .then_with(|| a.field.cmp(&b.field))
            });
            let hits = hits
                .into_iter()
                .skip(request.offset as usize)
                .take(if request.limit == 0 {
                    usize::MAX
                } else {
                    request.limit as usize
                })
                .collect();
            Ok(SearchResult {
                hits,
                engine: SearchEngine {
                    rungs_available: vec!["lexical".to_string()],
                    rung_selected_ceiling: "lexical".to_string(),
                    rrf_k: 60,
                    rung_depth: request.limit,
                },
                index_status: SearchIndexStatus {
                    lexical: "ready".to_string(),
                    semantic: "not_built".to_string(),
                    graph: "not_built".to_string(),
                },
                reduced: true,
                degraded: SearchDegraded {
                    is_degraded: true,
                    reason: "scan_backed_lexical".to_string(),
                },
            })
        })
    }

    pub fn read_store_search(&self, request: StoreSearchReadRequest<'_>) -> Result<SearchResult> {
        if request.query.is_empty() {
            return Err(LoomError::invalid("search query must not be empty"));
        }
        self.store.read(|loom| {
            let workspaces = match request.workspace {
                Some(workspace) => {
                    let ns = resolve_ns(loom, workspace)?;
                    let label = workspace_label(loom, ns).unwrap_or_else(|| ns.to_string());
                    vec![(ns, label)]
                }
                None => loom
                    .registry()
                    .list(Some(FacetKind::Search))
                    .into_iter()
                    .map(|info| (info.id, info.name))
                    .collect(),
            };
            let lowered = request.query.to_ascii_lowercase();
            let mut hits = Vec::new();
            for (ns, workspace_label) in workspaces {
                let root = loom
                    .registry()
                    .list(None)
                    .into_iter()
                    .find(|info| info.id == ns)
                    .and_then(|info| info.head)
                    .map(|digest| digest.to_string());
                let collections = match request.collection {
                    Some(collection) => vec![collection.to_string()],
                    None => search_collections(loom, ns)?,
                };
                for collection in collections {
                    for id in search_ids(loom, ns, &collection, None)? {
                        let Some(doc) = search_get(loom, ns, &collection, &id)? else {
                            continue;
                        };
                        for (field_name, value) in doc {
                            if request.field.is_some_and(|wanted| wanted != field_name) {
                                continue;
                            }
                            let FieldValue::Text(text) = value else {
                                continue;
                            };
                            let text_lower = text.to_ascii_lowercase();
                            let Some(byte_start) = text_lower.find(&lowered) else {
                                continue;
                            };
                            let byte_end = byte_start + lowered.len();
                            hits.push(SearchHit {
                                facet: "fts".to_string(),
                                workspace: workspace_label.clone(),
                                collection: collection.clone(),
                                entity_id: hex::encode(&id),
                                field: field_name,
                                snippet: snippet(&text, byte_start, byte_end),
                                offsets: vec![[byte_start as u64, byte_end as u64]],
                                scope_context: SearchScopeContext {
                                    owning_entity: None,
                                    status_fields: Vec::new(),
                                    refs: Vec::new(),
                                },
                                root: root.clone(),
                                match_via: "lexical".to_string(),
                                contributing_rungs: vec!["lexical".to_string()],
                                fused_score: 1.0,
                                raw_score: 1.0,
                                rung: "lexical".to_string(),
                            });
                        }
                    }
                }
            }
            hits.sort_by(|a, b| {
                a.workspace
                    .cmp(&b.workspace)
                    .then_with(|| a.collection.cmp(&b.collection))
                    .then_with(|| a.entity_id.cmp(&b.entity_id))
                    .then_with(|| a.field.cmp(&b.field))
            });
            let hits = hits
                .into_iter()
                .skip(request.offset as usize)
                .take(if request.limit == 0 {
                    usize::MAX
                } else {
                    request.limit as usize
                })
                .collect();
            Ok(SearchResult {
                hits,
                engine: SearchEngine {
                    rungs_available: vec!["lexical".to_string()],
                    rung_selected_ceiling: "lexical".to_string(),
                    rrf_k: 60,
                    rung_depth: request.limit,
                },
                index_status: SearchIndexStatus {
                    lexical: "ready".to_string(),
                    semantic: "not_built".to_string(),
                    graph: "not_built".to_string(),
                },
                reduced: true,
                degraded: SearchDegraded {
                    is_degraded: true,
                    reason: "scan_backed_lexical".to_string(),
                },
            })
        })
    }

    // ---- document ----

    /// `document_get_binary`: document `id` in collection `name`, or `None`.
    pub fn read_document_get_binary(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Option<DocumentBinary>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.document_get_binary(workspace, name, id);
        }
        if name == "boards" {
            return self.store.write(|loom| {
                let ns = resolve_ns(loom, workspace)?;
                let doc = loom_core::document::document_get_binary(loom, ns, name, id)?;
                if let Some(document) = &doc {
                    record_board_read_observed(loom, ns, id, &document.bytes)?;
                }
                Ok(doc)
            });
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_core::document::document_get_binary(loom, ns, name, id)
        })
    }

    /// `document_get_text`: document `id` decoded as UTF-8 text, or `None`.
    pub fn read_document_get_text(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Option<DocumentText>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.document_get_text(workspace, name, id);
        }
        if name == "boards" {
            return self.store.write(|loom| {
                let ns = resolve_ns(loom, workspace)?;
                let doc = loom_core::document::document_get_text(loom, ns, name, id)?;
                if let Some(document) = &doc {
                    record_board_read_observed(loom, ns, id, document.text.as_bytes())?;
                }
                Ok(doc)
            });
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_core::document::document_get_text(loom, ns, name, id)
        })
    }

    pub fn read_document_get_range(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
        offset: Option<u64>,
        length: Option<u64>,
    ) -> Result<Option<Vec<u8>>> {
        let Some(document) = self.read_document_get_binary(workspace, name, id)? else {
            return Ok(None);
        };
        let mut doc = document.bytes;
        let Some(offset) = offset else {
            return Ok(Some(doc));
        };
        let start = usize::try_from(offset)
            .map_err(|_| LoomError::invalid("document range offset is too large"))?;
        if start >= doc.len() {
            return Ok(Some(Vec::new()));
        }
        let end = match length {
            Some(length) => {
                let length = usize::try_from(length)
                    .map_err(|_| LoomError::invalid("document range length is too large"))?;
                start.saturating_add(length).min(doc.len())
            }
            None => doc.len(),
        };
        Ok(Some(doc.drain(start..end).collect()))
    }

    pub fn read_document_query(&self, req: DocumentQueryRead<'_>) -> Result<DocumentQueryResult> {
        let limit = req.limit.unwrap_or(100).clamp(1, 1000) as usize;
        // `document_query` is an MCP-host composite: candidate ids (predicate/index/list-all) + a
        // projection/paging pass + a per-item `Digest::hash(store_algo, doc)`. Over remote it is
        // reassembled in the host from source-backed primitives: `document_list_binary` (the full collection
        // bytes), `query_json`/`find_json` (candidate ids for the predicate/index branches), and
        // `store_digest_algo` (the store's algorithm). Then the same pure assembly runs. The local path
        // reads those inputs straight from the engine. Both feed `document_query_assemble`, so the result
        // (including per-item digests under the store's real algorithm) is identical.
        if let Some(backend) = self.store.remote_backend() {
            let algo = Algo::from_name(&backend.store_digest_algo()?)?;
            let collection =
                Collection::decode(&backend.document_list_binary(req.workspace, req.name)?)?;
            let candidate_ids = if let Some(predicate) = req.predicate {
                let query = serde_json::json!({ "predicate": predicate, "limit": 1000 });
                let bytes =
                    backend.document_query_json(req.workspace, req.name, &query.to_string())?;
                document_query_ids_from_json(&bytes)?
            } else if let Some(index) = req.index {
                let value = req
                    .value
                    .ok_or_else(|| LoomError::invalid("document query value is required"))?;
                let bytes = backend.document_find_json(
                    req.workspace,
                    req.name,
                    index,
                    &value.to_string(),
                )?;
                document_find_ids_from_json(&bytes)?
            } else {
                collection
                    .iter()
                    .map(|(id, _)| id.to_string())
                    .collect::<Vec<_>>()
            };
            return document_query_assemble(algo, &collection, candidate_ids, &req, limit);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, req.workspace)?;
            let algo = loom.store().digest_algo();
            let collection = loom_core::document::doc_list(loom, ns, req.name)?;
            let candidate_ids = if let Some(predicate) = req.predicate {
                let predicate = loom_core::document_query_from_json(&serde_json::json!({
                    "predicate": predicate,
                    "limit": limit
                }))?
                .predicate
                .ok_or_else(|| LoomError::invalid("document query predicate is required"))?;
                loom_core::doc_query(
                    loom,
                    ns,
                    req.name,
                    &loom_core::DocumentQuery {
                        predicate: Some(predicate),
                        projections: Vec::new(),
                        cursor: None,
                        limit: 1000,
                        include_document: false,
                    },
                )?
                .items
                .into_iter()
                .map(|item| item.id)
                .collect::<Vec<_>>()
            } else {
                match req.index {
                    Some(index) => {
                        let value = req.value.ok_or_else(|| {
                            LoomError::invalid("document query value is required")
                        })?;
                        let value = loom_core::document_index_value_from_json(value)?;
                        loom_core::doc_find(loom, ns, req.name, index, &value)?
                    }
                    None => collection
                        .iter()
                        .map(|(id, _)| id.to_string())
                        .collect::<Vec<_>>(),
                }
            };
            document_query_assemble(algo, &collection, candidate_ids, &req, limit)
        })
    }

    // ---- telemetry ----

    pub fn read_metrics_get_descriptor(
        &self,
        workspace: &str,
        name: &str,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.metrics_get_descriptor(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            metrics_get_descriptor(loom, ns, name)?
                .map(|descriptor| descriptor.encode())
                .transpose()
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn read_metrics_query(
        &self,
        workspace: &str,
        descriptor_name: &str,
        from_timestamp_ms: u64,
        to_timestamp_ms: u64,
        max_series: u32,
        max_groups: u32,
        max_samples: u32,
        max_output_bytes: u64,
        now_timestamp_ms: u64,
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.metrics_query(
                workspace,
                descriptor_name,
                from_timestamp_ms,
                to_timestamp_ms,
                max_series,
                max_groups,
                max_samples,
                max_output_bytes,
                now_timestamp_ms,
            );
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let result = metrics_query_observations(
                loom,
                ns,
                descriptor_name,
                &MetricQuery {
                    from_timestamp_ms,
                    to_timestamp_ms,
                    max_series,
                    max_groups,
                    max_samples,
                    max_output_bytes,
                    now_timestamp_ms,
                },
            )?;
            metric_query_result_cbor(&result)
        })
    }

    pub fn read_logs_get_record(
        &self,
        workspace: &str,
        record_id: &str,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.logs_get_record(workspace, record_id);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            logs_get_record(loom, ns, record_id)?
                .map(|record| record.encode())
                .transpose()
        })
    }

    pub fn read_logs_query(
        &self,
        workspace: &str,
        from_time_unix_nano: u64,
        to_time_unix_nano: u64,
        max_records: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.logs_query(
                workspace,
                from_time_unix_nano,
                to_time_unix_nano,
                max_records,
                max_output_bytes,
            );
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let result = logs_query(
                loom,
                ns,
                &LogQuery {
                    from_time_unix_nano,
                    to_time_unix_nano,
                    max_records,
                    max_output_bytes,
                },
            )?;
            log_query_result_cbor(&result)
        })
    }

    pub fn read_traces_get_span(
        &self,
        workspace: &str,
        trace_id: &str,
        span_id: &str,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.traces_get_span(workspace, trace_id, span_id);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            traces_get_span(loom, ns, trace_id, span_id)?
                .map(|span| span.encode())
                .transpose()
        })
    }

    pub fn read_traces_trace_spans(
        &self,
        workspace: &str,
        trace_id: &str,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.traces_trace_spans(workspace, trace_id, max_spans, max_output_bytes);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let result = traces_trace_spans(loom, ns, trace_id, max_spans, max_output_bytes)?;
            trace_query_result_cbor(&result)
        })
    }

    pub fn read_traces_query(
        &self,
        workspace: &str,
        from_start_time_ns: u64,
        to_start_time_ns: u64,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.traces_query(
                workspace,
                from_start_time_ns,
                to_start_time_ns,
                max_spans,
                max_output_bytes,
            );
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let result = traces_query(
                loom,
                ns,
                &TraceQuery {
                    from_start_time_ns,
                    to_start_time_ns,
                    max_spans,
                    max_output_bytes,
                },
            )?;
            trace_query_result_cbor(&result)
        })
    }

    // ---- timeseries ----

    /// `timeseries_get`: the value at exactly `ts` in series `name`, or `None`.
    pub fn read_timeseries_get(
        &self,
        workspace: &str,
        name: &str,
        ts: i64,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.ts_get(workspace, name, ts);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            ts_get(loom, ns, name, ts)
        })
    }

    /// `timeseries_latest`: the most recent point of series `name`, or `None` when empty.
    pub fn read_timeseries_latest(&self, workspace: &str, name: &str) -> Result<Option<TsPoint>> {
        if let Some(backend) = self.store.remote_backend() {
            return Ok(backend
                .ts_latest(workspace, name)?
                .map(|(ts, value)| TsPoint { ts, value }));
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(ts_latest(loom, ns, name)?.map(|(ts, value)| TsPoint { ts, value }))
        })
    }

    // ---- ledger ----

    /// `ledger_get`: the payload at `seq` in ledger `name`, or `None`.
    pub fn read_ledger_get(
        &self,
        workspace: &str,
        name: &str,
        seq: u64,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.ledger_get(workspace, name, seq);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            ledger_get(loom, ns, name, seq)
        })
    }

    /// `ledger_head`: the head hash of ledger `name`, or `None` when empty.
    pub fn read_ledger_head(&self, workspace: &str, name: &str) -> Result<Option<String>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.ledger_head(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(ledger_head(loom, ns, name)?.map(|d| d.to_string()))
        })
    }

    /// `ledger_len`: the number of entries in ledger `name`.
    pub fn read_ledger_len(&self, workspace: &str, name: &str) -> Result<u64> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.ledger_len(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            ledger_len(loom, ns, name)
        })
    }

    /// `ledger_verify`: check the hash chain of ledger `name`; errors if broken.
    pub fn read_ledger_verify(&self, workspace: &str, name: &str) -> Result<()> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.ledger_verify(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_core::ledger_verify(loom, ns, name)
        })
    }

    // ---- kv ----

    /// `kv_get`: the value for the canonical-CBOR typed `key_cbor` in map `name`, or `None`.
    pub fn read_kv_get(
        &self,
        workspace: &str,
        name: &str,
        key_cbor: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.kv_get(workspace, name, key_cbor);
        }
        let key = key_from_cbor(key_cbor)?;
        if let Some((paths, session, auth)) = self.store.daemon_session_parts()? {
            let target = self.store.read(|loom| {
                let ns = resolve_ns(loom, workspace)?;
                Ok((ns.to_string(), loom.kv_map_config(ns, name).tier))
            })?;
            if target.1 == loom_core::KvTier::Ephemeral {
                return daemon::kv_ephemeral_get_auth(
                    paths,
                    session,
                    &target.0,
                    name,
                    key_cbor,
                    now_ms(),
                    auth,
                );
            }
        }
        let has_runtime_state = self.store.has_runtime_state();
        self.store.read_runtime(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            reject_stateless_ephemeral_kv(loom, has_runtime_state, ns, name)?;
            loom.kv_get_configured(ns, name, &key, now_ms())
        })
    }

    /// `kv_list`: the whole map `name` as the canonical-CBOR `[key, value]` array (empty when absent).
    pub fn read_kv_list(&self, workspace: &str, name: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.kv_list(workspace, name);
        }
        if let Some((paths, session, auth)) = self.store.daemon_session_parts()? {
            let target = self.store.read(|loom| {
                let ns = resolve_ns(loom, workspace)?;
                Ok((ns.to_string(), loom.kv_map_config(ns, name).tier))
            })?;
            if target.1 == loom_core::KvTier::Ephemeral {
                return daemon::kv_ephemeral_list_auth(
                    paths,
                    session,
                    &target.0,
                    name,
                    now_ms(),
                    auth,
                );
            }
        }
        let has_runtime_state = self.store.has_runtime_state();
        self.store.read_runtime(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            reject_stateless_ephemeral_kv(loom, has_runtime_state, ns, name)?;
            Ok(loom.kv_list_configured(ns, name, now_ms())?.encode())
        })
    }

    /// `<facet>.list_collections`: the collection names present in `workspace` for `facet`; kv maps,
    /// document collections, time-series sets, ledger logs, queue streams, and sql databases all use
    /// this second addressing level. Per-principal facets use their own listers.
    pub fn read_collections(&self, workspace: &str, facet: FacetKind) -> Result<Vec<String>> {
        if let Some(backend) = self.store.remote_backend() {
            // SQL uses its dedicated list tool; the other data facets (Kv/Document/TimeSeries/Ledger/
            // Queue) forward through `list_collections`, which maps each facet to its IDL lister
            // (`<facet>.list_collections`, and `Queue.list_streams` for queues).
            if facet == FacetKind::Sql {
                return backend.sql_list_databases(workspace);
            }
            return backend.list_collections(workspace, facet);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(loom.list_collections(ns, facet))
        })
    }

    /// `kv_range`: the half-open `[lo, hi)` slice of map `name` (canonical-CBOR keys) as canonical CBOR.
    pub fn read_kv_range(
        &self,
        workspace: &str,
        name: &str,
        lo_cbor: &[u8],
        hi_cbor: &[u8],
        predicate: Option<&JsonValue>,
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            let bytes = backend.kv_range(workspace, name, lo_cbor, hi_cbor)?;
            return filter_kv_range_predicate(KvMap::decode(&bytes)?, predicate)
                .map(|map| map.encode());
        }
        let lo = key_from_cbor(lo_cbor)?;
        let hi = key_from_cbor(hi_cbor)?;
        if let Some((paths, session, auth)) = self.store.daemon_session_parts()? {
            let target = self.store.read(|loom| {
                let ns = resolve_ns(loom, workspace)?;
                Ok((ns.to_string(), loom.kv_map_config(ns, name).tier))
            })?;
            if target.1 == loom_core::KvTier::Ephemeral {
                let bytes = daemon::kv_ephemeral_range_auth(
                    paths,
                    daemon::KvRangeRequest {
                        session,
                        workspace: &target.0,
                        name,
                        lo_cbor,
                        hi_cbor,
                        now_ms: now_ms(),
                    },
                    auth,
                )?;
                return filter_kv_range_predicate(KvMap::decode(&bytes)?, predicate)
                    .map(|map| map.encode());
            }
        }
        let has_runtime_state = self.store.has_runtime_state();
        self.store.read_runtime(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            reject_stateless_ephemeral_kv(loom, has_runtime_state, ns, name)?;
            filter_kv_range_predicate(
                loom.kv_range_configured(ns, name, &lo, &hi, now_ms())?,
                predicate,
            )
            .map(|map| map.encode())
        })
    }

    // ---- fs ----

    /// `fs_read_file`: the whole contents of `path`.
    pub fn read_fs_read_file(&self, workspace: &str, path: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.fs_read_file(workspace, path);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.read_file(ns, path)
        })
    }

    /// `fs_read_link`: the target of the symlink at `path`.
    pub fn read_fs_read_link(&self, workspace: &str, path: &str) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.fs_read_link(workspace, path);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.read_link(ns, path)
        })
    }

    /// `fs_read_at`: `len` bytes of `path` starting at `offset`.
    pub fn read_fs_read_at(
        &self,
        workspace: &str,
        path: &str,
        offset: u64,
        len: u64,
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.fs_read_at(workspace, path, offset, len);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.read_at(ns, path, offset, len)
        })
    }

    pub fn read_fs_stat(&self, workspace: &str, path: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.fs_stat(workspace, path);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_wire::fs::fs_stat_to_cbor(&loom.stat(ns, path)?)
        })
    }

    pub fn read_fs_list_directory(&self, workspace: &str, path: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.fs_list_directory(workspace, path);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_wire::fs::dir_listing_to_cbor(&loom.list_directory(ns, path)?)
        })
    }

    // ---- mail ----

    /// `mail_get_message`: the parsed structured index record for `uid`, or `None`. This is the
    /// structured view; the raw serialization is [`Self::read_mail_to_eml`].
    pub fn read_mail_get_message(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Option<MailMessage>> {
        if let Some(backend) = self.store.remote_backend() {
            return match backend.mail_get_message(workspace, principal, mailbox, uid)? {
                Some(bytes) => Ok(Some(MailMessage::decode(&bytes)?)),
                None => Ok(None),
            };
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            mail::get_message(loom, ns, principal, mailbox, uid)
        })
    }

    /// `mail_to_eml`: the raw RFC 5322 bytes (the `.eml`) of the message at `uid`, or `None`.
    pub fn read_mail_to_eml(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.mail_to_eml(workspace, principal, mailbox, uid);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            mail::to_eml(loom, ns, principal, mailbox, uid)
        })
    }

    // ---- queue ----

    /// `queue_get`: the entry at `seq` in `stream`, or `None`.
    pub fn read_queue_get(
        &self,
        workspace: &str,
        stream: &str,
        seq: u64,
    ) -> Result<Option<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.queue_get(workspace, stream, seq);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.stream_get(ns, stream, seq as usize)
        })
    }

    /// `queue_range`: entries `[lo, hi)` of `stream`, oldest first.
    pub fn read_queue_range(
        &self,
        workspace: &str,
        stream: &str,
        lo: u64,
        hi: u64,
    ) -> Result<Vec<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.queue_range(workspace, stream, lo, hi);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.stream_range(ns, stream, lo as usize, hi as usize)
        })
    }

    /// `queue_len`: the number of entries in `stream`.
    pub fn read_queue_len(&self, workspace: &str, stream: &str) -> Result<u64> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.queue_len(workspace, stream);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(loom.stream_len(ns, stream)? as u64)
        })
    }

    /// `queue_consumer_position`: the named consumer's next sequence in `stream` (0 if unset).
    pub fn read_queue_consumer_position(
        &self,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
    ) -> Result<u64> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.queue_consumer_position(workspace, stream, consumer_id);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.consumer_position(ns, stream, consumer_id)
        })
    }

    /// `queue_consumer_read`: up to `max` entries from the consumer's position, without advancing it.
    pub fn read_queue_consumer_read(
        &self,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
        max: u32,
    ) -> Result<Vec<Vec<u8>>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.queue_consumer_read(workspace, stream, consumer_id, max);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.consumer_read(ns, stream, consumer_id, max as usize)
        })
    }

    // ---- watch ----

    /// `watch_subscribe`: start or resume a pull watch over one workspace branch.
    pub fn read_watch_subscribe(
        &self,
        workspace: &str,
        branch: &str,
        from: Option<&str>,
        facet: Option<&str>,
        path_prefix: Option<&str>,
        change_kinds: Option<&[String]>,
    ) -> Result<WatchSubscriptionSummary> {
        if let Some(backend) = self.store.remote_backend() {
            return Ok(WatchSubscriptionSummary {
                cursor: backend.watch_subscribe(
                    workspace,
                    branch,
                    from,
                    facet,
                    path_prefix,
                    change_kinds.unwrap_or(&[]),
                )?,
            });
        }
        let from = from.map(Digest::parse).transpose()?;
        let change_kinds = change_kinds
            .unwrap_or(&[])
            .iter()
            .map(|kind| parse_watch_change_kind(kind))
            .collect::<Result<Vec<_>>>()?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let mut selector = WatchSelector::new(ns, branch)?;
            if let Some(facet) = facet {
                selector = selector.with_facet(FacetKind::parse(facet)?);
            }
            if let Some(path_prefix) = path_prefix {
                selector = selector.with_path_prefix(path_prefix);
            }
            for kind in change_kinds {
                selector = selector.with_change_kind(kind);
            }
            Ok(WatchSubscriptionSummary {
                cursor: loom.watch_subscribe(&selector, from)?.encode(),
            })
        })
    }

    /// `watch_poll`: read a bounded batch from a pull-watch cursor.
    pub fn read_watch_poll(
        &self,
        workspace: &str,
        cursor: &str,
        max: u32,
    ) -> Result<WatchBatchSummary> {
        if let Some(backend) = self.store.remote_backend() {
            let bytes = backend.watch_poll(workspace, cursor, max)?;
            return Ok(watch_batch_summary(watch_batch_from_cbor(&bytes)?));
        }
        let cursor = WatchCursor::decode(cursor)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            if cursor.workspace != ns {
                return Err(LoomError::new(
                    Code::CursorInvalid,
                    "watch cursor workspace mismatch",
                ));
            }
            Ok(watch_batch_summary(loom.watch_poll(&cursor, max as usize)?))
        })
    }

    /// `substrate_changes`: watch events plus LMDIFF bytes for non-root commits.
    pub fn read_substrate_changes(
        &self,
        workspace: &str,
        cursor: &str,
        max: u32,
    ) -> Result<SubstrateChangesSummary> {
        if cursor.starts_with("oplog:") {
            let cursor = OperationChangeCursor::decode(cursor)?;
            return self.store.read(|loom| {
                let ns = resolve_ns(loom, workspace)?;
                let batch = profile_operation_changes(loom, ns, &cursor, max as usize)?;
                Ok(SubstrateChangesSummary {
                    events: batch
                        .events
                        .into_iter()
                        .map(operation_change_summary)
                        .collect(),
                    next: batch.next.encode(),
                })
            });
        }
        let cursor = WatchCursor::decode(cursor)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            if cursor.workspace != ns {
                return Err(LoomError::new(
                    Code::CursorInvalid,
                    "watch cursor workspace mismatch",
                ));
            }
            let batch = loom.watch_poll(&cursor, max as usize)?;
            let mut events = Vec::with_capacity(batch.events.len());
            for event in batch.events {
                let commit = event.commit;
                let parent = event.parent;
                let lmdiff = parent
                    .map(|parent| loom.diff_commits(ns, parent, commit))
                    .transpose()?;
                events.push(SubstrateChangeSummary::Data {
                    workspace: event.workspace.to_string(),
                    ref_name: event.branch,
                    commit: commit.to_string(),
                    parent: parent.map(|parent| parent.to_string()),
                    seq: event.seq,
                    changes: event
                        .changes
                        .into_iter()
                        .map(domain_change_summary)
                        .collect(),
                    unsupported_domains: event
                        .unsupported_domains
                        .into_iter()
                        .map(unsupported_domain_summary)
                        .collect(),
                    lmdiff,
                });
            }
            Ok(SubstrateChangesSummary {
                events,
                next: batch.next.encode(),
            })
        })
    }

    pub fn read_workgraph_changes(
        &self,
        workspace: &str,
        workspace_id: &str,
        next_sequence: u64,
        max: u32,
    ) -> Result<SubstrateChangesSummary> {
        let cursor = OperationChangeCursor::new(
            workgraph_operation_cursor_scope(workspace_id),
            next_sequence,
        )?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let batch = workgraph_operation_changes(loom, ns, &cursor, max as usize)?;
            Ok(SubstrateChangesSummary {
                events: batch
                    .events
                    .into_iter()
                    .map(operation_change_summary)
                    .collect(),
                next: batch.next.encode(),
            })
        })
    }

    pub fn read_substrate_refs(
        &self,
        workspace: &str,
        target: &str,
    ) -> Result<SubstrateRefsResult> {
        let target = EntityRef::parse(target)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_file_path(ns, REF_INDEX_DIR, AclRight::Read)?;
            loom.authorize_file_path(ns, REF_INDEX_PATH, AclRight::Read)?;
            match loom.read_file_reserved(ns, REF_INDEX_PATH) {
                Ok(bytes) => {
                    let index = ReferenceIndex::decode(&bytes)?;
                    let mut indexed_facets = index
                        .edges()
                        .iter()
                        .map(|edge| edge.source.facet.clone())
                        .collect::<Vec<_>>();
                    indexed_facets.sort();
                    indexed_facets.dedup();
                    return Ok(substrate_refs_result(
                        &target,
                        &index,
                        indexed_facets,
                        SubstrateRefsDegraded {
                            is_degraded: false,
                            reason: String::new(),
                        },
                    ));
                }
                Err(e) if e.code == Code::NotFound => {}
                Err(e) => return Err(e),
            }
            let mut index = ReferenceIndex::new();
            for collection in loom.list_collections(ns, FacetKind::Document) {
                let documents = match loom_core::document::doc_list(loom, ns, &collection) {
                    Ok(documents) => documents,
                    Err(e) if matches!(e.code, Code::PermissionDenied | Code::NotFound) => {
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                for (id, doc) in documents.iter() {
                    let Ok(text) = std::str::from_utf8(doc) else {
                        continue;
                    };
                    let source = ReferenceSource::new("document", &collection, id, "body")?;
                    index.add_text_refs(source, "refers_to", text)?;
                }
            }
            let target_id = target.as_str();
            for collection in loom.list_collections(ns, FacetKind::Graph) {
                let edges = match graph_in_edges(loom, ns, &collection, &target_id) {
                    Ok(edges) => edges,
                    Err(e) if matches!(e.code, Code::PermissionDenied | Code::NotFound) => {
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                for (edge_id, edge) in edges {
                    let source = ReferenceSource::new("graph", &collection, &edge_id, "edge")?;
                    let evidence = format!("{} {} {}", edge.src, edge.label, edge.dst);
                    let span_start = evidence.len() - edge.dst.len();
                    if let Ok(edge) = ReferenceEdge::new(
                        source,
                        target.clone(),
                        edge.label,
                        span_start,
                        evidence.len(),
                        evidence,
                    ) {
                        index.add(edge);
                    }
                }
            }
            Ok(substrate_refs_result(
                &target,
                &index,
                vec!["document".to_string(), "graph".to_string()],
                SubstrateRefsDegraded {
                    is_degraded: true,
                    reason: "current implementation scans readable UTF-8 document collections and typed graph edges without a persisted projection".to_string(),
                },
            ))
        })
    }

    pub fn read_substrate_alias_resolve(
        &self,
        workspace: &str,
        scope_id: &str,
        alias: &str,
    ) -> Result<Option<SubstrateAliasSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_file_path(ns, REF_INDEX_DIR, AclRight::Read)?;
            loom.authorize_file_path(ns, ALIAS_INDEX_PATH, AclRight::Read)?;
            if let Some(profile) = loom_tickets::TicketProfileReader::open(loom, ns, scope_id)?
                && let Some(resolution) = profile.resolve_ticket_key(alias)?
            {
                return Ok(Some(SubstrateAliasSummary {
                    alias: resolution.requested_key.canonical(),
                    target: format!("ticket:{}", resolution.ticket_id),
                    scope_id: scope_id.to_string(),
                    kind: "derived_ticket_key".to_string(),
                    retired: matches!(resolution.status, loom_tickets::TicketKeyStatus::Retired),
                    sequence: None,
                }));
            }
            let Some(index) = alias_index_from_reserved(loom, ns)? else {
                return Ok(None);
            };
            Ok(index
                .resolve(scope_id, alias)
                .map(SubstrateAliasSummary::from))
        })
    }

    pub fn read_substrate_alias_list(
        &self,
        workspace: &str,
        scope_id: &str,
    ) -> Result<Vec<SubstrateAliasSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_file_path(ns, REF_INDEX_DIR, AclRight::Read)?;
            loom.authorize_file_path(ns, ALIAS_INDEX_PATH, AclRight::Read)?;
            let Some(index) = alias_index_from_reserved(loom, ns)? else {
                return Ok(Vec::new());
            };
            Ok(index
                .bindings_for_scope(scope_id)
                .into_iter()
                .map(SubstrateAliasSummary::from)
                .collect())
        })
    }

    pub fn read_substrate_reference_reconciliation_status(
        &self,
        workspace: &str,
    ) -> Result<ReferenceReconciliationSummary> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::substrate_refs::reference_reconciliation_status(loom, ns)
        })
    }

    pub fn read_substrate_history(
        &self,
        workspace: &str,
        scope_id: &str,
        entity_id: &str,
    ) -> Result<SubstrateHistorySummary> {
        let path = revision_index_path(scope_id)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_file_path(ns, REVISION_INDEX_DIR, AclRight::Read)?;
            loom.authorize_file_path(ns, &path, AclRight::Read)?;
            let bytes = match loom.read_file_reserved(ns, &path) {
                Ok(bytes) => bytes,
                Err(e) if e.code == Code::NotFound => {
                    return Ok(SubstrateHistorySummary {
                        scope_id: scope_id.to_string(),
                        entity_id: entity_id.to_string(),
                        index_present: false,
                        revisions: Vec::new(),
                        latest: None,
                        checkpoints: Vec::new(),
                    });
                }
                Err(e) => return Err(e),
            };
            let index = RevisionIndex::decode(&bytes)?;
            let revisions = index
                .history(entity_id)
                .into_iter()
                .map(SubstrateRevisionSummary::from)
                .collect::<Vec<_>>();
            let latest = index.latest(entity_id).map(SubstrateRevisionSummary::from);
            let latest_revision = latest.as_ref().map(|entry| entry.revision).unwrap_or(0);
            let checkpoints = index
                .checkpoints()
                .iter()
                .filter(|entry| entry.scope_id == scope_id && entry.max_revision <= latest_revision)
                .map(SubstrateCheckpointSummary::from)
                .collect();
            Ok(SubstrateHistorySummary {
                scope_id: scope_id.to_string(),
                entity_id: entity_id.to_string(),
                index_present: true,
                revisions,
                latest,
                checkpoints,
            })
        })
    }

    pub fn read_substrate_revision_latest(
        &self,
        workspace: &str,
        scope_id: &str,
        entity_id: &str,
    ) -> Result<SubstrateRevisionLookupSummary> {
        let path = revision_index_path(scope_id)?;
        self.store.read(|loom| {
            let Some(index) = read_authorized_revision_index(loom, workspace, &path)? else {
                return Ok(SubstrateRevisionLookupSummary {
                    scope_id: scope_id.to_string(),
                    entity_id: entity_id.to_string(),
                    index_present: false,
                    revision: None,
                });
            };
            Ok(SubstrateRevisionLookupSummary {
                scope_id: scope_id.to_string(),
                entity_id: entity_id.to_string(),
                index_present: true,
                revision: index.latest(entity_id).map(SubstrateRevisionSummary::from),
            })
        })
    }

    pub fn read_substrate_revision_at(
        &self,
        workspace: &str,
        scope_id: &str,
        entity_id: &str,
        revision: u64,
    ) -> Result<SubstrateRevisionLookupSummary> {
        let path = revision_index_path(scope_id)?;
        self.store.read(|loom| {
            let Some(index) = read_authorized_revision_index(loom, workspace, &path)? else {
                return Ok(SubstrateRevisionLookupSummary {
                    scope_id: scope_id.to_string(),
                    entity_id: entity_id.to_string(),
                    index_present: false,
                    revision: None,
                });
            };
            Ok(SubstrateRevisionLookupSummary {
                scope_id: scope_id.to_string(),
                entity_id: entity_id.to_string(),
                index_present: true,
                revision: index
                    .at_revision(entity_id, revision)
                    .map(SubstrateRevisionSummary::from),
            })
        })
    }

    pub fn read_substrate_revision_as_of_root(
        &self,
        workspace: &str,
        scope_id: &str,
        entity_id: &str,
        root: &str,
    ) -> Result<SubstrateRevisionLookupSummary> {
        let path = revision_index_path(scope_id)?;
        let root = Digest::parse(root)?;
        self.store.read(|loom| {
            let Some(index) = read_authorized_revision_index(loom, workspace, &path)? else {
                return Ok(SubstrateRevisionLookupSummary {
                    scope_id: scope_id.to_string(),
                    entity_id: entity_id.to_string(),
                    index_present: false,
                    revision: None,
                });
            };
            Ok(SubstrateRevisionLookupSummary {
                scope_id: scope_id.to_string(),
                entity_id: entity_id.to_string(),
                index_present: true,
                revision: index
                    .as_of_root(entity_id, &root)
                    .map(SubstrateRevisionSummary::from),
            })
        })
    }

    pub fn read_substrate_checkpoint_before(
        &self,
        workspace: &str,
        scope_id: &str,
        revision: u64,
    ) -> Result<SubstrateCheckpointLookupSummary> {
        let path = revision_index_path(scope_id)?;
        self.store.read(|loom| {
            let Some(index) = read_authorized_revision_index(loom, workspace, &path)? else {
                return Ok(SubstrateCheckpointLookupSummary {
                    scope_id: scope_id.to_string(),
                    index_present: false,
                    revision,
                    checkpoint: None,
                });
            };
            Ok(SubstrateCheckpointLookupSummary {
                scope_id: scope_id.to_string(),
                index_present: true,
                revision,
                checkpoint: index
                    .checkpoint_before_or_at(scope_id, revision)
                    .map(SubstrateCheckpointSummary::from),
            })
        })
    }

    pub fn read_substrate_view_get(
        &self,
        workspace: &str,
        view_id: &str,
    ) -> Result<Option<ViewDefinitionSummary>> {
        let path = view_path(view_id)?;
        let mut view = self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_file_path(ns, &path, AclRight::Read)?;
            match loom.read_file_reserved(ns, &path) {
                Ok(bytes) => ViewDefinition::decode(&bytes)
                    .map(ViewDefinitionSummary::from)
                    .map(Some),
                Err(e) if e.code == Code::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        })?;
        if let Some(view) = &mut view {
            view.projection = Some(self.execute_substrate_view(workspace, view)?);
        }
        Ok(view)
    }

    pub fn read_substrate_view_list(&self, workspace: &str) -> Result<Vec<ViewDefinitionSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.authorize_file_path(ns, VIEW_DIR, AclRight::Read)?;
            let entries = match loom.list_directory(ns, VIEW_DIR) {
                Ok(entries) => entries,
                Err(e) if e.code == Code::NotFound => return Ok(Vec::new()),
                Err(e) => return Err(e),
            };
            let mut views = Vec::new();
            for entry in entries {
                if entry.kind != FileKind::File || !entry.name.ends_with(".lcv") {
                    continue;
                }
                let view_id = entry.name.trim_end_matches(".lcv");
                let path = view_path(view_id)?;
                loom.authorize_file_path(ns, &path, AclRight::Read)?;
                let view = ViewDefinition::decode(&loom.read_file_reserved(ns, &path)?)?;
                views.push(ViewDefinitionSummary::from(view));
            }
            Ok(views)
        })
    }

    fn execute_substrate_view(
        &self,
        workspace: &str,
        view: &ViewDefinitionSummary,
    ) -> Result<JsonValue> {
        let workspace_id = self.view_workspace_id(workspace, view)?;
        match view.projection_ref.as_str() {
            "view:tickets.open" | "view:tickets.planning" => {
                let tickets = self.read_tickets_list(workspace, &workspace_id, None)?;
                let open = tickets
                    .into_iter()
                    .filter(ticket_is_open)
                    .collect::<Vec<_>>();
                Ok(serde_json::json!({
                    "status": "source_backed",
                    "projection_ref": &view.projection_ref,
                    "workspace_id": workspace_id,
                    "profile": "tickets",
                    "count": open.len(),
                    "items": open
                }))
            }
            "view:planning.markdown" => {
                let tickets = self.read_tickets_list(workspace, &workspace_id, None)?;
                let open = tickets
                    .into_iter()
                    .filter(ticket_is_open)
                    .collect::<Vec<_>>();
                let body = planning_markdown(&workspace_id, &open);
                Ok(serde_json::json!({
                    "status": "source_backed",
                    "projection_ref": &view.projection_ref,
                    "workspace_id": workspace_id,
                    "profile": "tickets",
                    "media_type": "text/markdown",
                    "count": open.len(),
                    "body": body
                }))
            }
            "view:meetings.extraction-review" => {
                let review = self.read_meetings_extraction_review(workspace, &workspace_id)?;
                Ok(serde_json::json!({
                    "status": "source_backed",
                    "projection_ref": &view.projection_ref,
                    "workspace_id": workspace_id,
                    "profile": "meetings",
                    "review": review
                }))
            }
            "view:lifecycle.operations" | "view:lifecycle.stage" => self.store.read(|loom| {
                let ns = resolve_ns(loom, workspace)?;
                loom.authorize_domain(ns, AclDomain::Lifecycle, AclRight::Read)?;
                let records = match loom
                    .store()
                    .control_get(&lifecycle_operation_log_key(&workspace_id)?)?
                {
                    Some(bytes) => LifecycleOperationLog::decode(&bytes)?.records,
                    None => Vec::new(),
                };
                Ok(serde_json::json!({
                    "status": "source_backed",
                    "projection_ref": &view.projection_ref,
                    "workspace_id": workspace_id,
                    "profile": "lifecycle",
                    "operation_log": {
                        "count": records.len(),
                        "recent": records
                            .iter()
                            .map(|record| serde_json::json!({
                                "operation_id": record.operation_id,
                                "operation_kind": record.operation_kind,
                                "sequence": record.sequence,
                                "instance_id": record.instance_id,
                                "target_entity_id": record.target_entity_id,
                                "root_after": record.root_after.to_string()
                            }))
                            .collect::<Vec<_>>()
                    }
                }))
            }),
            _ => Ok(serde_json::json!({
                "status": "target",
                "projection_ref": &view.projection_ref,
                "reason": "projection function is registered but has no source-backed MCP executor"
            })),
        }
    }

    fn view_workspace_id(&self, workspace: &str, view: &ViewDefinitionSummary) -> Result<String> {
        if let Some(scope) = view
            .source_scopes
            .iter()
            .find(|scope| WorkspaceId::parse(scope).is_ok())
        {
            return Ok(scope.clone());
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(ns.to_string())
        })
    }

    pub fn read_tickets_get(
        &self,
        workspace: &str,
        workspace_id: &str,
        ticket_id: &str,
        projection: Option<&str>,
    ) -> Result<Option<TicketSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::get_ticket_with_projection(
                loom,
                ns,
                workspace_id,
                ticket_id,
                loom_tickets::parse_ticket_projection(projection)?,
            )
        })
    }

    pub fn read_tickets_get_readable(
        &self,
        workspace: &str,
        workspace_id: &str,
        ticket_id: &str,
        projection: Option<&str>,
    ) -> Result<Option<serde_json::Value>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let Some(ticket) = loom_tickets::get_ticket_with_projection(
                loom,
                ns,
                workspace_id,
                ticket_id,
                loom_tickets::parse_ticket_projection(projection)?,
            )?
            else {
                return Ok(None);
            };
            let history = loom_tickets::history(loom, ns, workspace_id, Some(&ticket.primary_key))?;
            let comments =
                loom_tickets::list_ticket_comments(loom, ns, workspace_id, &ticket.primary_key)?;
            Ok(Some(readable_ticket_json(&ticket, &history, &comments)))
        })
    }

    pub fn read_tickets_project_settings_get(
        &self,
        workspace: &str,
        workspace_id: &str,
        project_id: &str,
        include_contracts: bool,
    ) -> Result<Option<TicketProjectSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::get_project_with_contract_details(
                loom,
                ns,
                workspace_id,
                project_id,
                include_contracts,
            )
        })
    }

    pub fn read_tickets_fields(
        &self,
        workspace: &str,
        project_id: Option<&str>,
        projection: Option<&str>,
        operation: Option<&str>,
    ) -> Result<loom_tickets::TicketFieldCatalog> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let profile_id = ns.to_string();
            let projection = loom_tickets::parse_ticket_projection(projection)?;
            match project_id {
                Some(project_id) => loom_tickets::ticket_field_catalog_for_project(
                    loom,
                    ns,
                    &profile_id,
                    project_id,
                    projection,
                    operation,
                ),
                None => loom_tickets::ticket_field_catalog(projection, operation),
            }
        })
    }

    /// `tickets_projects`: the discoverable ticket projects in `workspace`, each with its key
    /// prefix, name, next ticket number, and default projection. Agents/users call this before
    /// creating or updating a ticket to learn available projects and the default projection;
    /// `tickets_fields` then lists the accepted fields for a chosen project. Shares the single
    /// `loom_tickets::list_projects` contract with the CLI.
    pub fn read_tickets_projects(&self, workspace: &str) -> Result<serde_json::Value> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let profile_id = ns.to_string();
            let projects = loom_tickets::list_projects(loom, ns, &profile_id)?;
            let items = projects
                .into_iter()
                .map(|project| {
                    serde_json::json!({
                        "project_id": project.project_id,
                        "key_prefix": project.key_prefix,
                        "name": project.name,
                        "next_ticket_number": project.next_ticket_number,
                        "default_projection": project
                            .projection_config
                            .default_display_projection
                            .profile_id(),
                    })
                })
                .collect::<Vec<_>>();
            Ok(serde_json::json!({ "projects": items }))
        })
    }

    /// `tickets_relations`: a ticket's canonical relations in both directions (outgoing from the
    /// ticket, incoming from the reverse `ticket-relations` graph), each with relation kind, the
    /// other ticket's id, and its title. Lets agents inspect depends_on/blocks without parsing
    /// descriptions or scanning the ticket set. Shares the `loom_tickets::list_ticket_relations`
    /// contract with the CLI.
    pub fn read_tickets_relations(
        &self,
        workspace: &str,
        ticket_id: &str,
    ) -> Result<serde_json::Value> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let profile_id = ns.to_string();
            let relations = loom_tickets::list_ticket_relations(loom, ns, &profile_id, ticket_id)?;
            let items = relations
                .into_iter()
                .map(|relation| {
                    serde_json::json!({
                        "direction": relation.direction,
                        "kind": relation.kind,
                        "target_ticket_id": relation.target_ticket_id,
                        "target_title": relation.target_title,
                    })
                })
                .collect::<Vec<_>>();
            Ok(serde_json::json!({ "relations": items }))
        })
    }

    /// Internal app reader (dashboards, Board). BOUNDED by contract: app/agent-facing reads must
    /// never materialize an unbounded ticket list, so this returns the most-recently-updated page
    /// capped at [`loom_tickets::TICKET_LIST_MAX_LIMIT`]. Callers needing more must paginate via
    /// [`Self::read_tickets_page`].
    pub fn read_tickets_list(
        &self,
        workspace: &str,
        workspace_id: &str,
        projection: Option<&str>,
    ) -> Result<Vec<TicketSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let query = loom_tickets::TicketListQuery {
                projection: loom_tickets::parse_ticket_projection(projection)?,
                limit: Some(loom_tickets::TICKET_LIST_MAX_LIMIT),
                ..Default::default()
            };
            Ok(loom_tickets::list_tickets_page(loom, ns, workspace_id, &query)?.items)
        })
    }

    pub fn read_tickets_query(
        &self,
        workspace: &str,
        workspace_id: &str,
        request: loom_tickets::TicketQueryRequest<'_>,
    ) -> Result<Vec<TicketSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::query_tickets(
                loom,
                ns,
                loom_tickets::TicketQueryRequest {
                    workspace_id,
                    projection: request.projection,
                    statuses: request.statuses,
                    buckets: request.buckets,
                    assignees: request.assignees,
                    lane_owners: request.lane_owners,
                    parent_tickets: request.parent_tickets,
                    dependency_tickets: request.dependency_tickets,
                    queue_lanes: request.queue_lanes,
                    title_contains: request.title_contains,
                    text_contains: request.text_contains,
                    field_equals: request.field_equals,
                    limit: request.limit,
                    offset: request.offset,
                },
            )
        })
    }

    /// Bounded, filtered, ordered, paginated ticket listing - the single list operation. Replaces
    /// the unbounded read path; callers resolve first-class Lane membership into
    /// `query.lane_member_ids` before calling.
    pub fn read_tickets_page(
        &self,
        workspace: &str,
        workspace_id: &str,
        query: loom_tickets::TicketListQuery,
    ) -> Result<loom_tickets::TicketListPage> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::list_tickets_page(loom, ns, workspace_id, &query)
        })
    }

    pub fn read_tickets_boards_get(
        &self,
        workspace: &str,
        workspace_id: &str,
        board_id: &str,
    ) -> Result<Option<BoardSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::get_board(loom, ns, workspace_id, board_id)
        })
    }

    pub fn read_tickets_boards_list(
        &self,
        workspace: &str,
        workspace_id: &str,
        include_deleted: bool,
    ) -> Result<Vec<BoardSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::list_boards(loom, ns, workspace_id, include_deleted)
        })
    }

    pub fn read_tickets_history(
        &self,
        workspace: &str,
        workspace_id: &str,
        ticket_id: Option<&str>,
    ) -> Result<Vec<TicketHistoryRecord>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_tickets::history(loom, ns, workspace_id, ticket_id)
        })
    }

    pub fn read_tickets_history_readable(
        &self,
        workspace: &str,
        workspace_id: &str,
        ticket_id: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(loom_tickets::history(loom, ns, workspace_id, ticket_id)?
                .iter()
                .map(readable_ticket_history_json)
                .collect())
        })
    }

    pub fn read_lanes_get(&self, workspace: &str, lane_id: &str) -> Result<Option<Lane>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.lanes_get(workspace, lane_id);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lanes::get_lane(loom, ns, lane_id)
        })
    }

    pub fn read_lanes_list(&self, workspace: &str) -> Result<Vec<Lane>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.lanes_list(workspace);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lanes::list_lanes(loom, ns)
        })
    }

    /// Fail-soft lane listing: the lanes that decode plus one diagnostic per record that does not.
    /// The remote backend path has no diagnostic channel, so it reports an empty diagnostic list.
    pub fn read_lanes_list_with_diagnostics(
        &self,
        workspace: &str,
    ) -> Result<(Vec<Lane>, Vec<LaneDecodeDiagnostic>)> {
        if let Some(backend) = self.store.remote_backend() {
            return Ok((backend.lanes_list(workspace)?, Vec::new()));
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lanes::list_lanes_with_diagnostics(loom, ns)
        })
    }

    pub fn read_lanes_get_view(&self, workspace: &str, lane_id: &str) -> Result<Option<LaneView>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let Some(lane) = loom_lanes::get_lane(loom, ns, lane_id)? else {
                return Ok(None);
            };
            Ok(Some(build_lane_view(loom, ns, &ns.to_string(), &lane)))
        })
    }

    /// Fail-soft lane view listing for the MCP reader: the healthy lane views plus a diagnostic per
    /// record that failed to decode, so malformed coordination records surface instead of vanishing.
    pub fn read_lanes_list_views_with_diagnostics(
        &self,
        workspace: &str,
    ) -> Result<(Vec<LaneView>, Vec<LaneDecodeDiagnostic>)> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let (lanes, diagnostics) = loom_lanes::list_lanes_with_diagnostics(loom, ns)?;
            let ticket_workspace_id = ns.to_string();
            let views = lanes
                .iter()
                .map(|lane| build_lane_view(loom, ns, &ticket_workspace_id, lane))
                .collect();
            Ok((views, diagnostics))
        })
    }

    pub fn read_workgraph_metrics(
        &self,
        workspace: &str,
        workspace_id: Option<&str>,
        statuses: &[String],
        lanes: &[String],
        limit: Option<u32>,
    ) -> Result<JsonValue> {
        const DEFAULT_LIMIT: usize = 50;
        const MAX_LIMIT: usize = 200;
        let workspace_id = match workspace_id {
            Some(id) => id.to_string(),
            None => self.store.read(|loom| {
                let ns = resolve_ns(loom, workspace)?;
                Ok(ns.to_string())
            })?,
        };
        let limit = limit
            .map(|value| value as usize)
            .unwrap_or(DEFAULT_LIMIT)
            .min(MAX_LIMIT);
        let status_filter = statuses.iter().cloned().collect::<BTreeSet<_>>();
        let lane_filter = lanes.iter().cloned().collect::<BTreeSet<_>>();
        let tickets = self.read_tickets_list(workspace, &workspace_id, None)?;
        let (lanes, lane_diagnostics) = self.read_lanes_list_with_diagnostics(workspace)?;
        let mut status_counts = BTreeMap::<String, usize>::new();
        let mut ticket_lane_index = BTreeMap::<String, Vec<&Lane>>::new();
        for lane in &lanes {
            for ticket in &lane.lane_tickets {
                ticket_lane_index
                    .entry(ticket.ticket_id.clone())
                    .or_default()
                    .push(lane);
            }
        }
        for ticket in &tickets {
            *status_counts.entry(ticket_status(ticket)).or_insert(0) += 1;
        }
        let waiting_for_review = status_counts
            .get("waiting_for_review")
            .copied()
            .unwrap_or(0);
        let feedback_available = status_counts
            .get("feedback_available")
            .copied()
            .unwrap_or(0);
        let blocked = status_counts.get("blocked").copied().unwrap_or(0);
        let accepted = status_counts.get("accepted").copied().unwrap_or(0);
        let mut cohort = Vec::new();
        let mut mismatch = Vec::new();
        for ticket in &tickets {
            let status = ticket_status(ticket);
            let assigned_lanes = ticket_lane_index
                .get(&ticket.primary_key)
                .cloned()
                .unwrap_or_default();
            if !status_filter.is_empty() && !status_filter.contains(&status) {
                continue;
            }
            if !lane_filter.is_empty()
                && !assigned_lanes
                    .iter()
                    .any(|lane| lane_filter.contains(&lane.lane_id))
            {
                continue;
            }
            if cohort.len() < limit {
                cohort.push(ticket_metric_item(ticket, &status, &assigned_lanes));
            }
            if let Some(reason) = ticket_lane_mismatch(ticket, &assigned_lanes)
                && mismatch.len() < limit
            {
                let mut item = ticket_metric_item(ticket, &status, &assigned_lanes);
                item["mismatch_reason"] = JsonValue::String(reason);
                mismatch.push(item);
            }
        }
        let lanes_value = lanes
            .iter()
            .filter(|lane| lane_filter.is_empty() || lane_filter.contains(&lane.lane_id))
            .map(lane_metric_item)
            .collect::<Vec<_>>();
        Ok(serde_json::json!({
            "status": "source_backed",
            "workspace_id": workspace_id,
            "limit": limit,
            "source_authority": {
                "tickets": "canonical_ticket",
                "lanes": "coordination_state"
            },
            "ticket_counts": {
                "total": tickets.len(),
                "by_status": status_counts,
                "waiting_for_review": waiting_for_review,
                "feedback_available": feedback_available,
                "blocked": blocked,
                "accepted": accepted
            },
            "lane_counts": {
                "total": lanes.len(),
                "blocked": lanes.iter().filter(|lane| lane.lane_status == "blocked").count(),
                "idle": lanes.iter().filter(|lane| lane_is_idle(lane)).count(),
                "malformed": lane_diagnostics.len()
            },
            "lanes": lanes_value,
            "lane_diagnostics": lane_diagnostics,
            "cohort": {
                "filters": {
                    "statuses": statuses,
                    "lanes": lane_filter.iter().collect::<Vec<_>>()
                },
                "returned": cohort.len(),
                "items": cohort
            },
            "lane_assignment_mismatches": {
                "returned": mismatch.len(),
                "items": mismatch
            }
        }))
    }

    pub fn read_spaces_list(
        &self,
        workspace: &str,
        workspace_id: &str,
    ) -> Result<Vec<SpaceSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::list_spaces(loom, ns, workspace_id)
        })
    }

    pub fn read_spaces_get(
        &self,
        workspace: &str,
        workspace_id: &str,
        space_id: &str,
    ) -> Result<Option<SpaceSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::get_space(loom, ns, workspace_id, space_id)
        })
    }

    pub fn read_pages_get(
        &self,
        workspace: &str,
        workspace_id: &str,
        page_id: &str,
    ) -> Result<Option<PageSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::get_page(loom, ns, workspace_id, page_id)
        })
    }

    pub fn read_pages_list(&self, workspace: &str, workspace_id: &str) -> Result<Vec<PageSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::list_pages(loom, ns, workspace_id)
        })
    }

    pub fn read_pages_history(
        &self,
        workspace: &str,
        workspace_id: &str,
        page_id: &str,
    ) -> Result<Vec<PageHistoryEntry>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::page_history(loom, ns, workspace_id, page_id)
        })
    }

    pub fn read_lifecycles_definition(
        &self,
        workspace: &str,
        workspace_id: &str,
        definition_id: &str,
    ) -> Result<Option<LifecycleDefinitionSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lifecycle::get_definition(loom, ns, workspace_id, definition_id)
        })
    }

    pub fn read_lifecycles_definitions(
        &self,
        workspace: &str,
        workspace_id: &str,
    ) -> Result<Vec<LifecycleDefinitionSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lifecycle::list_definitions(loom, ns, workspace_id)
        })
    }

    pub fn read_lifecycles_instance(
        &self,
        workspace: &str,
        workspace_id: &str,
        instance_id: &str,
    ) -> Result<Option<LifecycleInstanceSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lifecycle::get_instance(loom, ns, workspace_id, instance_id)
        })
    }

    pub fn read_lifecycles_instances(
        &self,
        workspace: &str,
        workspace_id: &str,
    ) -> Result<Vec<LifecycleInstanceSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lifecycle::list_instances(loom, ns, workspace_id)
        })
    }

    pub fn read_lifecycles_snapshot_plan(
        &self,
        workspace: &str,
        workspace_id: &str,
        instance_id: &str,
        to_stage_id: &str,
    ) -> Result<LifecycleSnapshotPlanSummary> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lifecycle::snapshot_plan(loom, ns, workspace_id, instance_id, to_stage_id)
        })
    }

    pub fn read_lifecycles_current_surface(
        &self,
        workspace: &str,
        workspace_id: &str,
        instance_id: &str,
    ) -> Result<LifecycleStageSurfaceSummary> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lifecycle::current_surface(loom, ns, workspace_id, instance_id)
        })
    }

    pub fn read_lifecycles_snapshot(
        &self,
        workspace: &str,
        workspace_id: &str,
        snapshot_id: &str,
    ) -> Result<Option<LifecycleSnapshotRecordSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lifecycle::get_snapshot(loom, ns, workspace_id, snapshot_id)
        })
    }

    pub fn read_lifecycles_snapshot_content(
        &self,
        workspace: &str,
        workspace_id: &str,
        snapshot_id: &str,
    ) -> Result<Option<Vec<u8>>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lifecycle::snapshot_content(loom, ns, workspace_id, snapshot_id)
        })
    }

    pub fn read_lifecycles_snapshots(
        &self,
        workspace: &str,
        workspace_id: &str,
    ) -> Result<Vec<LifecycleSnapshotRecordSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lifecycle::list_snapshots(loom, ns, workspace_id)
        })
    }

    pub fn read_lifecycles_operation_log(
        &self,
        workspace: &str,
        workspace_id: &str,
    ) -> Result<LifecycleOperationLogSummary> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_lifecycle::operation_log(loom, ns, workspace_id)
        })
    }

    pub fn read_chat_fetch_events(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
        from_sequence: u64,
        max: usize,
    ) -> Result<SubstrateChangesSummary> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let channel = crate::chat::resolve_channel_id(loom, ns, workspace_id, channel_id)?;
            let cursor = OperationChangeCursor::new(
                format!("chat:{workspace_id}:{channel}"),
                from_sequence,
            )?;
            let batch = crate::chat::operation_changes(loom, ns, &cursor, max)?;
            Ok(SubstrateChangesSummary {
                events: batch
                    .events
                    .into_iter()
                    .map(operation_change_summary)
                    .collect(),
                next: batch.next.encode(),
            })
        })
    }

    pub fn read_chat_channels(
        &self,
        workspace: &str,
        workspace_id: &str,
    ) -> Result<Vec<crate::chat::ChatChannelDirectorySummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::list_channels(loom, ns, workspace_id)
        })
    }

    pub fn read_chat_messages(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
    ) -> Result<ChatChannelSummary> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::channel_projection(loom, ns, workspace_id, channel_id)
        })
    }

    pub fn read_chat_cursor(
        &self,
        workspace: &str,
        workspace_id: &str,
        channel_id: &str,
    ) -> Result<crate::chat::ChatCursorSummary> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::chat::read_cursor(loom, ns, workspace_id, channel_id)
        })
    }

    pub fn read_drive_list(
        &self,
        workspace: &str,
        workspace_id: &str,
        folder_id: &str,
    ) -> Result<DriveFolderSummary> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::list_folder(loom, ns, workspace_id, folder_id)
        })
    }

    pub fn read_meetings_projection_outputs(
        &self,
        workspace: &str,
        workspace_id: &str,
    ) -> Result<MeetingsProjectionSummary> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::meetings::projection_outputs(loom, ns, workspace_id)
        })
    }

    pub fn read_meetings_list(
        &self,
        workspace: &str,
        workspace_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<crate::meetings::MeetingsListSummary> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::meetings::list(loom, ns, workspace_id, limit, offset)
        })
    }

    pub fn read_meetings_get(
        &self,
        workspace: &str,
        workspace_id: &str,
        meeting_id: &str,
    ) -> Result<crate::meetings::MeetingDetailSummary> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::meetings::get(loom, ns, workspace_id, meeting_id)
        })
    }

    pub fn read_meetings_search(
        &self,
        workspace: &str,
        workspace_id: &str,
        query: &str,
        field: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<SearchResult> {
        self.read_store_search(StoreSearchReadRequest {
            workspace: Some(workspace),
            collection: Some(workspace_id),
            query,
            field,
            limit,
            offset,
        })
    }

    pub fn read_meetings_extraction_review(
        &self,
        workspace: &str,
        workspace_id: &str,
    ) -> Result<MeetingsExtractionReviewSummary> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::meetings::extraction_review(loom, ns, workspace_id)
        })
    }

    pub fn read_drive_stat(
        &self,
        workspace: &str,
        workspace_id: &str,
        folder_id: &str,
        name: &str,
    ) -> Result<DriveStatSummary> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::stat_node(loom, ns, workspace_id, folder_id, name)
        })
    }

    pub fn read_drive_read(
        &self,
        workspace: &str,
        workspace_id: &str,
        file_id: &str,
    ) -> Result<Vec<u8>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::read_file(loom, ns, workspace_id, file_id)
        })
    }

    pub fn read_drive_list_versions(
        &self,
        workspace: &str,
        workspace_id: &str,
        file_id: &str,
    ) -> Result<Vec<DriveVersionSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::list_versions(loom, ns, workspace_id, file_id)
        })
    }

    pub fn read_drive_list_shares(
        &self,
        workspace: &str,
        workspace_id: &str,
    ) -> Result<Vec<crate::drive::DriveShareGrantSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::list_share_grants(loom, ns, workspace_id)
        })
    }

    pub fn read_drive_list_retention(
        &self,
        workspace: &str,
        workspace_id: &str,
    ) -> Result<Vec<crate::drive::DriveRetentionPinSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::drive::list_retention_pins(loom, ns, workspace_id)
        })
    }

    pub fn read_structures_get(
        &self,
        workspace: &str,
        workspace_id: &str,
        structure_id: &str,
    ) -> Result<Option<StructureRenderSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::get_structure(loom, ns, workspace_id, structure_id)
        })
    }

    pub fn read_structures_list(
        &self,
        workspace: &str,
        workspace_id: &str,
    ) -> Result<Vec<StructureSummary>> {
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            crate::pages::list_structures(loom, ns, workspace_id)
        })
    }

    // ---- vcs (workspace-level history) ----

    /// `vcs_log`: commit addresses on `branch`, newest first.
    pub fn read_vcs_log(&self, workspace: &str, branch: &str) -> Result<Vec<String>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_log(workspace, branch);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(loom
                .log(ns, branch)?
                .iter()
                .map(|d| d.to_string())
                .collect())
        })
    }

    pub fn read_vcs_head_branch(&self, workspace: &str) -> Result<String> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_head_branch(workspace);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.registry().head_branch(ns)
        })
    }

    /// `vcs_status`: the working state of the workspace (staged/unstaged/untracked/conflicts).
    pub fn read_vcs_status(&self, workspace: &str) -> Result<Status> {
        if let Some(backend) = self.store.remote_backend() {
            let wire = backend.vcs_status(workspace)?;
            return loom_wire::vcs::status_from_cbor(&wire);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.status(ns)
        })
    }

    /// `vcs_merge_in_progress`: whether a merge is paused with conflicts for the workspace.
    pub fn read_vcs_merge_in_progress(&self, workspace: &str) -> Result<bool> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_merge_in_progress(workspace);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.merge_in_progress(ns)
        })
    }

    /// `vcs_merge_conflicts`: the unresolved paths of an in-progress merge.
    pub fn read_vcs_merge_conflicts(&self, workspace: &str) -> Result<Vec<String>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_merge_conflicts(workspace);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.merge_conflicts(ns)
        })
    }

    /// `vcs_tag_list`: the tag names in the workspace.
    pub fn read_vcs_tag_list(&self, workspace: &str) -> Result<Vec<String>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_tag_list(workspace);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.tag_list(ns)
        })
    }

    /// `vcs_tag_target`: the ref target of tag `name`, or `None` if unknown.
    pub fn read_vcs_tag_target(&self, workspace: &str, name: &str) -> Result<Option<String>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_tag_target(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(loom.tag_target(ns, name)?.map(|d| d.to_string()))
        })
    }

    /// `vcs_diff`: the cross-facet structural diff envelope between two commits.
    pub fn read_vcs_diff(
        &self,
        workspace: &str,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.vcs_diff(workspace, from_commit, to_commit);
        }
        let from = Digest::parse(from_commit)?;
        let to = Digest::parse(to_commit)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom.diff_commits(ns, from, to)
        })
    }

    /// `vcs_blame`: each current path on `branch` paired with the commit that last set it.
    pub fn read_vcs_blame(&self, workspace: &str, branch: &str) -> Result<Vec<(String, String)>> {
        if let Some(backend) = self.store.remote_backend() {
            let wire = backend.vcs_blame(workspace, branch)?;
            return loom_wire::vcs::blame_rows_from_cbor(&wire);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            Ok(loom
                .blame(ns, branch)?
                .into_iter()
                .map(|(path, digest)| (path, digest.to_string()))
                .collect())
        })
    }

    // ---- sql (table-model readers; Loom Canonical CBOR) ----

    /// `sql_read_table`: the staged `table` in database `db` as canonical-CBOR columns and rows.
    pub fn read_sql_read_table(&self, workspace: &str, db: &str, table: &str) -> Result<Vec<u8>> {
        let path = sql_table_path(db, table);
        if let Some(backend) = self.store.remote_backend() {
            return backend.sql_read_table(workspace, &path);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let t = loom.read_table(ns, &path)?;
            result_cbor::table_cbor(&t)
        })
    }

    /// `sql_read_table_at`: the committed `table` in database `db` at `commit`.
    pub fn read_sql_read_table_at(
        &self,
        workspace: &str,
        db: &str,
        table: &str,
        commit: &str,
    ) -> Result<Vec<u8>> {
        let path = sql_table_path(db, table);
        if let Some(backend) = self.store.remote_backend() {
            return backend.sql_read_table_at(workspace, &path, commit);
        }
        let commit = Digest::parse(commit)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let t = loom.read_table_at(ns, &path, commit)?;
            result_cbor::table_cbor(&t)
        })
    }

    /// `sql_index_scan`: rows of secondary `index` matching the canonical-CBOR `prefix`, as canonical CBOR.
    pub fn read_sql_index_scan(
        &self,
        workspace: &str,
        db: &str,
        table: &str,
        index: &str,
        prefix: &[u8],
    ) -> Result<Vec<u8>> {
        let path = sql_table_path(db, table);
        if let Some(backend) = self.store.remote_backend() {
            return backend.sql_index_scan(workspace, &path, index, prefix);
        }
        let values = lookup_cbor::values_from_cbor(prefix)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let rows = loom.index_scan(ns, &path, index, &values)?;
            let schema = loom.read_table(ns, &path)?.schema().clone();
            result_cbor::rows_cbor(&schema, &rows)
        })
    }

    /// `sql_index_scan_at`: scan a secondary index at `commit`.
    pub fn read_sql_index_scan_at(
        &self,
        workspace: &str,
        db: &str,
        table: &str,
        index: &str,
        prefix: &[u8],
        commit: &str,
    ) -> Result<Vec<u8>> {
        let path = sql_table_path(db, table);
        if let Some(backend) = self.store.remote_backend() {
            return backend.sql_index_scan_at(workspace, &path, index, prefix, commit);
        }
        let values = lookup_cbor::values_from_cbor(prefix)?;
        let commit = Digest::parse(commit)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let rows = loom.index_scan_at(ns, &path, index, &values, commit)?;
            let schema = loom.read_table_at(ns, &path, commit)?.schema().clone();
            result_cbor::rows_cbor(&schema, &rows)
        })
    }

    /// `sql_blame`: each current row of `table` in `db` on `branch` with the commit that last set it.
    pub fn read_sql_blame(
        &self,
        workspace: &str,
        db: &str,
        branch: &str,
        table: &str,
    ) -> Result<Vec<u8>> {
        let path = sql_table_path(db, table);
        if let Some(backend) = self.store.remote_backend() {
            return backend.sql_blame(workspace, branch, &path);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let rows = loom.blame_table(ns, branch, &path)?;
            result_cbor::blame_cbor(&rows)
        })
    }

    /// `sql_diff`: the row-level diff of `table` in `db` between two commits (content addresses), as CBOR.
    pub fn read_sql_diff(
        &self,
        workspace: &str,
        db: &str,
        table: &str,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<Vec<u8>> {
        let path = sql_table_path(db, table);
        if let Some(backend) = self.store.remote_backend() {
            return backend.sql_diff(workspace, &path, from_commit, to_commit);
        }
        let from = Digest::parse(from_commit)?;
        let to = Digest::parse(to_commit)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let diffs = loom.diff_table(ns, &path, from, to)?;
            result_cbor::diff_cbor(&diffs)
        })
    }

    /// `sql_table_diff`: schema-aware table diff between two commits, as canonical CBOR.
    pub fn read_sql_table_diff(
        &self,
        workspace: &str,
        db: &str,
        table: &str,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<Vec<u8>> {
        let path = sql_table_path(db, table);
        if let Some(backend) = self.store.remote_backend() {
            return backend.sql_table_diff(workspace, &path, from_commit, to_commit);
        }
        let from = Digest::parse(from_commit)?;
        let to = Digest::parse(to_commit)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let records = loom.diff_table_records(ns, &path, from, to)?;
            result_cbor::table_diff_cbor(&records)
        })
    }

    /// MCP read projection for SQL query payloads.
    pub fn read_sql_query(&self, workspace: &str, db: &str, sql: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store().remote_backend() {
            return backend.sql_query(workspace, db, sql);
        }
        if let Some((path, auth, daemon_authorized)) = self.store().per_request_parts()? {
            let read = if local_auth_requires_write(auth) {
                with_local_store_write_lock(path, || {
                    crate::open_per_request_read_loom(path, auth, daemon_authorized)
                })?
            } else {
                crate::open_per_request_read_loom(path, auth, daemon_authorized)?
            };
            let ns = resolve_ns(&read, workspace)?;
            let mut store = LoomSqlStore::open_read(read, ns, db)?;
            return read_sql_query_cbor(&mut store, sql);
        }
        self.store().read_runtime(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            let mut store = LoomSqlStore::load_eager_read(loom, ns, db)?;
            read_sql_query_cbor(&mut store, sql)
        })
    }

    // ---- document (list) ----

    /// `document_list_binary`: the collection `name` as canonical bytes.
    pub fn read_document_list_binary(&self, workspace: &str, name: &str) -> Result<Vec<u8>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.document_list_binary(workspace, name);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_core::document::document_list_binary(loom, ns, name)
        })
    }

    /// `document_list`: the collection `name` (its document ids and metadata).
    pub fn read_document_list(&self, workspace: &str, name: &str) -> Result<Collection> {
        Collection::decode(&self.read_document_list_binary(workspace, name)?)
    }

    // ---- timeseries (range) ----

    /// `timeseries_range`: the points of series `name` with `from <= ts <= to`.
    pub fn read_timeseries_range(
        &self,
        workspace: &str,
        name: &str,
        from: i64,
        to: i64,
    ) -> Result<Series> {
        if let Some(backend) = self.store.remote_backend() {
            let wire = backend.ts_range(workspace, name, from, to)?;
            return Series::decode(&wire);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            loom_core::ts_range(loom, ns, name, from, to)
        })
    }

    // ---- calendar reads ----

    /// `calendar_get_collection`: a collection's metadata, or `None`.
    pub fn read_calendar_get_collection(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> Result<Option<CollectionMeta>> {
        if let Some(backend) = self.store.remote_backend() {
            return match backend.calendar_get_collection(workspace, principal, collection)? {
                Some(bytes) => Ok(Some(CollectionMeta::decode(&bytes)?)),
                None => Ok(None),
            };
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            calendar::get_collection(loom, ns, principal, collection)
        })
    }

    /// `calendar_list_collections`: the collection names for `principal`.
    pub fn read_calendar_list_collections(
        &self,
        workspace: &str,
        principal: &str,
    ) -> Result<Vec<String>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.calendar_list_collections(workspace, principal);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            calendar::list_collections(loom, ns, principal)
        })
    }

    /// `calendar_get_entry`: one entry by `uid`, or `None`.
    pub fn read_calendar_get_entry(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> Result<Option<CalendarEntry>> {
        if let Some(backend) = self.store.remote_backend() {
            return match backend.calendar_get_entry(workspace, principal, collection, uid)? {
                Some(bytes) => Ok(Some(CalendarEntry::decode(&bytes)?)),
                None => Ok(None),
            };
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            calendar::get_entry(loom, ns, principal, collection, uid)
        })
    }

    /// `calendar_list_entries`: every entry in `collection`.
    pub fn read_calendar_list_entries(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> Result<Vec<CalendarEntry>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend
                .calendar_list_entries(workspace, principal, collection)?
                .iter()
                .map(|bytes| CalendarEntry::decode(bytes))
                .collect();
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            calendar::list_entries(loom, ns, principal, collection)
        })
    }

    /// `calendar_range`: occurrences in `collection` overlapping `[from, to]` (RFC 5545 date-times).
    pub fn read_calendar_range(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<Occurrence>> {
        if let Some(backend) = self.store.remote_backend() {
            let wire = backend.calendar_range(workspace, principal, collection, from, to)?;
            return loom_wire::calendar::occurrences_from_cbor(&wire);
        }
        let from = parse_window_bound(from)?;
        let to = parse_window_bound(to)?;
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            calendar::range(loom, ns, principal, collection, from, to)
        })
    }

    /// `calendar_search`: entries in `collection` whose `component` text matches `text`.
    pub fn read_calendar_search(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        component: &str,
        text: &str,
    ) -> Result<Vec<CalendarEntry>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend
                .calendar_search(workspace, principal, collection, component, text)?
                .iter()
                .map(|bytes| CalendarEntry::decode(bytes))
                .collect();
        }
        let component = parse_component_filter(component)?;
        let text = if text.is_empty() { None } else { Some(text) };
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            calendar::search(loom, ns, principal, collection, component, text)
        })
    }

    /// `calendar_to_ics`: the iCalendar serialization of entry `uid`, or `None`.
    pub fn read_calendar_to_ics(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> Result<Option<String>> {
        if let Some(backend) = self.store.remote_backend() {
            return remote_optional_utf8(
                backend.calendar_to_ics(workspace, principal, collection, uid)?,
            );
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            calendar::entry_ics(loom, ns, principal, collection, uid)
        })
    }

    // ---- contacts reads ----

    /// `contacts_get_book`: an address book's metadata, or `None`.
    pub fn read_contacts_get_book(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
    ) -> Result<Option<BookMeta>> {
        if let Some(backend) = self.store.remote_backend() {
            return match backend.contacts_get_book(workspace, principal, book)? {
                Some(bytes) => Ok(Some(BookMeta::decode(&bytes)?)),
                None => Ok(None),
            };
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            contacts::get_book(loom, ns, principal, book)
        })
    }

    /// `contacts_list_books`: the book names for `principal`.
    pub fn read_contacts_list_books(
        &self,
        workspace: &str,
        principal: &str,
    ) -> Result<Vec<String>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.contacts_list_books(workspace, principal);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            contacts::list_books(loom, ns, principal)
        })
    }

    /// `contacts_get_entry`: one contact by `uid`, or `None`.
    pub fn read_contacts_get_entry(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> Result<Option<ContactEntry>> {
        if let Some(backend) = self.store.remote_backend() {
            return match backend.contacts_get_entry(workspace, principal, book, uid)? {
                Some(bytes) => Ok(Some(ContactEntry::decode(&bytes)?)),
                None => Ok(None),
            };
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            contacts::get_entry(loom, ns, principal, book, uid)
        })
    }

    /// `contacts_list_entries`: every contact in `book`.
    pub fn read_contacts_list_entries(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
    ) -> Result<Vec<ContactEntry>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend
                .contacts_list_entries(workspace, principal, book)?
                .iter()
                .map(|bytes| ContactEntry::decode(bytes))
                .collect();
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            contacts::list_entries(loom, ns, principal, book)
        })
    }

    /// `contacts_search`: contacts in `book` matching `text`.
    pub fn read_contacts_search(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        text: &str,
    ) -> Result<Vec<ContactEntry>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend
                .contacts_search(workspace, principal, book, text)?
                .iter()
                .map(|bytes| ContactEntry::decode(bytes))
                .collect();
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            contacts::search(loom, ns, principal, book, text)
        })
    }

    /// `contacts_to_vcard`: the vCard serialization of contact `uid`, or `None`.
    pub fn read_contacts_to_vcard(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> Result<Option<String>> {
        if let Some(backend) = self.store.remote_backend() {
            return remote_optional_utf8(
                backend.contacts_to_vcard(workspace, principal, book, uid)?,
            );
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            contacts::entry_vcard(loom, ns, principal, book, uid)
        })
    }

    // ---- mail reads ----

    /// `mail_get_mailbox`: a mailbox's metadata, or `None`.
    pub fn read_mail_get_mailbox(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
    ) -> Result<Option<MailboxMeta>> {
        if let Some(backend) = self.store.remote_backend() {
            return match backend.mail_get_mailbox(workspace, principal, mailbox)? {
                Some(bytes) => Ok(Some(MailboxMeta::decode(&bytes)?)),
                None => Ok(None),
            };
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            mail::get_mailbox(loom, ns, principal, mailbox)
        })
    }

    /// `mail_list_mailboxes`: the mailbox names for `principal`.
    pub fn read_mail_list_mailboxes(
        &self,
        workspace: &str,
        principal: &str,
    ) -> Result<Vec<String>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.mail_list_mailboxes(workspace, principal);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            mail::list_mailboxes(loom, ns, principal)
        })
    }

    /// `mail_list_messages`: the structured index records in `mailbox`.
    pub fn read_mail_list_messages(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
    ) -> Result<Vec<MailMessage>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend
                .mail_list_messages(workspace, principal, mailbox)?
                .iter()
                .map(|bytes| MailMessage::decode(bytes))
                .collect();
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            mail::list_messages(loom, ns, principal, mailbox)
        })
    }

    /// `mail_get_flags`: the flag set on message `uid`.
    pub fn read_mail_get_flags(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Vec<String>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend.mail_get_flags(workspace, principal, mailbox, uid);
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            mail::get_flags(loom, ns, principal, mailbox, uid)
        })
    }

    /// `mail_search`: messages in `mailbox` whose indexed headers match `text`.
    pub fn read_mail_search(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        text: &str,
    ) -> Result<Vec<MailMessage>> {
        if let Some(backend) = self.store.remote_backend() {
            return backend
                .mail_search(workspace, principal, mailbox, text)?
                .iter()
                .map(|bytes| MailMessage::decode(bytes))
                .collect();
        }
        self.store.read(|loom| {
            let ns = resolve_ns(loom, workspace)?;
            mail::search(loom, ns, principal, mailbox, text)
        })
    }
}

/// Decode an optional remote UTF-8 payload (e.g. an iCalendar or vCard serialization) into an
/// optional string, mapping a non-UTF-8 body to a corrupt-store error.
fn remote_optional_utf8(payload: Option<Vec<u8>>) -> Result<Option<String>> {
    match payload {
        Some(bytes) => Ok(Some(String::from_utf8(bytes).map_err(|_| {
            LoomError::corrupt("remote calendar/contacts serialization was not valid UTF-8")
        })?)),
        None => Ok(None),
    }
}

/// The pure assembly stage of `document_query`, shared by the local and remote paths: given the store's
/// digest `algo`, the full `collection` bytes, and the ordered `candidate_ids`, apply the request's
/// `id_prefix`/`cursor` filtering and `limit` paging, then build each `DocumentQueryItem` (len,
/// `Digest::hash(algo, doc)`, projections, optional document body). This is engine-free and operates only
/// on the already-fetched collection, so a remote host reproduces the local result byte-for-byte,
/// including per-item digests.
fn document_query_assemble(
    algo: Algo,
    collection: &Collection,
    candidate_ids: Vec<String>,
    req: &DocumentQueryRead<'_>,
    limit: usize,
) -> Result<DocumentQueryResult> {
    let projections = req
        .projections
        .iter()
        .map(|(projection_name, path)| {
            Ok((
                (*projection_name).to_string(),
                loom_core::DocumentFieldPath::dotted(path)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut items = Vec::new();
    let mut next_cursor = None;
    for id in candidate_ids {
        if req.id_prefix.is_some_and(|prefix| !id.starts_with(prefix)) {
            continue;
        }
        if req.cursor.is_some_and(|cursor| id.as_str() <= cursor) {
            continue;
        }
        if items.len() == limit {
            next_cursor = Some(id);
            break;
        }
        let doc = collection
            .get(&id)
            .ok_or_else(|| LoomError::corrupt("document query selected a missing document"))?;
        let projected = projections
            .iter()
            .map(|(name, path)| {
                Ok((
                    name.clone(),
                    loom_core::doc_extract_index_value(doc, path)?
                        .map_or(serde_json::Value::Null, tabular_value_json),
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        items.push(DocumentQueryItem {
            id,
            len: doc.len() as u64,
            digest: Digest::hash(algo, doc).to_string(),
            document: if req.include_document {
                Some(doc.to_vec())
            } else {
                None
            },
            projections: projected,
        });
    }
    Ok(DocumentQueryResult { items, next_cursor })
}

/// Extract candidate ids from a remote `Document.query_json` result (`{"items": [{"id": ...}, ...]}`).
fn document_query_ids_from_json(bytes: &[u8]) -> Result<Vec<String>> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| LoomError::corrupt(format!("document query result json: {e}")))?;
    let items = value
        .get("items")
        .and_then(|v| v.as_array())
        .ok_or_else(|| LoomError::corrupt("document query result missing items array"))?;
    items
        .iter()
        .map(|item| {
            item.get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .ok_or_else(|| LoomError::corrupt("document query item missing id"))
        })
        .collect()
}

/// Extract candidate ids from a remote `Document.find_json` result (a JSON array of id strings).
fn document_find_ids_from_json(bytes: &[u8]) -> Result<Vec<String>> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|e| LoomError::corrupt(format!("document find result json: {e}")))?;
    value
        .as_array()
        .ok_or_else(|| LoomError::corrupt("document find result must be a json array"))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| LoomError::corrupt("document find id must be a string"))
        })
        .collect()
}

fn metric_query_result_cbor(result: &MetricQueryResult) -> Result<Vec<u8>> {
    let observations = result
        .observations
        .iter()
        .map(|observation| observation.encode().map(loom_codec::Value::Bytes))
        .collect::<Result<Vec<_>>>()?;
    loom_codec::encode(&loom_codec::Value::Array(vec![
        loom_codec::Value::Array(observations),
        loom_codec::Value::Bool(result.partial),
        loom_codec::Value::Bool(result.stale),
    ]))
    .map_err(|error| LoomError::invalid(format!("metric query result encoding failed: {error}")))
}

fn log_query_result_cbor(result: &loom_core::LogQueryResult) -> Result<Vec<u8>> {
    let records = result
        .records
        .iter()
        .map(|record| record.encode().map(loom_codec::Value::Bytes))
        .collect::<Result<Vec<_>>>()?;
    loom_codec::encode(&loom_codec::Value::Array(vec![
        loom_codec::Value::Array(records),
        loom_codec::Value::Bool(result.partial),
    ]))
    .map_err(|error| LoomError::invalid(format!("log query result encoding failed: {error}")))
}

fn trace_query_result_cbor(result: &TraceQueryResult) -> Result<Vec<u8>> {
    let spans = result
        .spans
        .iter()
        .map(|span| span.encode().map(loom_codec::Value::Bytes))
        .collect::<Result<Vec<_>>>()?;
    loom_codec::encode(&loom_codec::Value::Array(vec![
        loom_codec::Value::Array(spans),
        loom_codec::Value::Bool(result.partial),
    ]))
    .map_err(|error| LoomError::invalid(format!("trace query result encoding failed: {error}")))
}

fn tabular_value_json(value: TabularValue) -> serde_json::Value {
    match value {
        TabularValue::Null => serde_json::Value::Null,
        TabularValue::Bool(value) => serde_json::Value::Bool(value),
        TabularValue::Int(value) => serde_json::json!(value),
        TabularValue::U64(value) => serde_json::json!(value),
        TabularValue::Float(value) => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        TabularValue::Text(value) => serde_json::Value::String(value),
        _ => serde_json::Value::Null,
    }
}

fn readable_ticket_json(
    ticket: &TicketSummary,
    history: &[TicketHistoryRecord],
    comments: &[loom_tickets::TicketComment],
) -> serde_json::Value {
    let latest = history.iter().max_by_key(|record| record.sequence);
    serde_json::json!({
        "primary_key": &ticket.primary_key,
        "title": ticket_title(ticket),
        "status": ticket_text_field(ticket, "status"),
        "priority": ticket_text_field(ticket, "priority"),
        "type": &ticket.ticket_type,
        "assignee": ticket_text_field(ticket, "assignee"),
        "assignee_display": ticket_text_field(ticket, "assignee_display"),
        "project": &ticket.project_id,
        "description": ticket_text_field(ticket, "description"),
        "dependencies": {
            "depends_on": &ticket.depends_on,
            "blocks": &ticket.blocks,
            "relations": ticket.relations.iter().map(|relation| {
                serde_json::json!({
                    "kind": &relation.kind,
                    "target_id": &relation.target_id
                })
            }).collect::<Vec<_>>()
        },
        "comment_count": comments.len(),
        "comments": comments,
        "latest_update": latest.map(|record| {
            serde_json::json!({
                "timestamp_ms": record.envelope
                    .get("timestamp_ms")
                    .and_then(serde_json::Value::as_u64),
                "actor": record.envelope
                    .get("actor_principal")
                    .and_then(serde_json::Value::as_str),
                "operation_kind": &record.operation_kind,
                "sequence": record.sequence,
                "operation_id": &record.operation_id
            })
        })
    })
}

fn readable_ticket_history_json(record: &TicketHistoryRecord) -> serde_json::Value {
    serde_json::json!({
        "timestamp_ms": record
            .envelope
            .get("timestamp_ms")
            .and_then(serde_json::Value::as_u64),
        "actor": record
            .envelope
            .get("actor_principal")
            .and_then(serde_json::Value::as_str),
        "operation_kind": &record.operation_kind,
        "summary": ticket_history_summary(record),
        "sequence": record.sequence,
        "operation_id": &record.operation_id
    })
}

fn ticket_history_summary(record: &TicketHistoryRecord) -> String {
    record
        .envelope
        .pointer("/payload/status")
        .or_else(|| record.envelope.pointer("/payload/target_status"))
        .and_then(serde_json::Value::as_str)
        .map(|status| format!("status={status}"))
        .or_else(|| {
            record
                .target_entity_id
                .as_ref()
                .map(|target| format!("target={target}"))
        })
        .unwrap_or_default()
}

fn ticket_is_open(ticket: &TicketSummary) -> bool {
    !ticket_text_field(ticket, "status_category")
        .or_else(|| ticket_text_field(ticket, "status"))
        .as_deref()
        .is_some_and(|status| matches!(status, "done" | "accepted"))
}

fn ticket_text_field(ticket: &TicketSummary, field: &str) -> Option<String> {
    match ticket.fields.get(field)? {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Object(map) => map
            .get("String")
            .or_else(|| map.get("Text"))
            .or_else(|| map.get("EnumOption"))
            .or_else(|| map.get("Principal"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn ticket_status(ticket: &TicketSummary) -> String {
    ticket_text_field(ticket, "status").unwrap_or_else(|| "unknown".to_string())
}

fn ticket_title(ticket: &TicketSummary) -> String {
    ticket_text_field(ticket, "title")
        .or_else(|| ticket_text_field(ticket, "summary"))
        .unwrap_or_default()
}

fn build_lane_view(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    ticket_workspace_id: &str,
    lane: &Lane,
) -> LaneView {
    let lane_tickets = lane
        .lane_tickets
        .iter()
        .map(|lane_ticket| {
            let ticket = loom_tickets::get_ticket(
                loom,
                workspace,
                ticket_workspace_id,
                &lane_ticket.ticket_id,
            )
            .ok()
            .flatten();
            LaneTicketView {
                ticket_id: lane_ticket.ticket_id.clone(),
                status: Some(
                    ticket
                        .as_ref()
                        .map(ticket_status)
                        .unwrap_or_else(|| "missing".to_string()),
                ),
                priority: ticket
                    .as_ref()
                    .and_then(|ticket| ticket_text_field(ticket, "priority")),
                title: ticket.as_ref().map(ticket_title),
            }
        })
        .collect();
    let mut view = loom_lanes::lane_view(lane, lane_tickets);
    // resolve the lane owner's display alias at the projection layer using the shared
    // ticket-service resolver (loom-lanes cannot see the identity store).
    view.owner_display = view
        .owner_principal
        .as_deref()
        .map(|id| loom_tickets::resolve_principal_display(loom.identity_store(), id));
    view
}

fn ticket_metric_item(ticket: &TicketSummary, status: &str, lanes: &[&Lane]) -> JsonValue {
    serde_json::json!({
        "ticket_id": &ticket.ticket_id,
        "primary_key": &ticket.primary_key,
        "project_id": &ticket.project_id,
        "status": status,
        "title": ticket_title(ticket),
        "lane_owner": ticket_text_field(ticket, "lane_owner"),
        "queue_lane": ticket_text_field(ticket, "queue_lane"),
        "lanes": lanes
            .iter()
            .map(|lane| serde_json::json!({
                "lane_id": &lane.lane_id,
                "lane_key": &lane.lane_key,
                "owner_principal": &lane.owner_principal
            }))
            .collect::<Vec<_>>()
    })
}

fn lane_metric_item(lane: &Lane) -> JsonValue {
    serde_json::json!({
        "lane_id": &lane.lane_id,
        "lane_key": &lane.lane_key,
        "owner_principal": &lane.owner_principal,
        "lane_status": &lane.lane_status,
        "active_ticket_id": &lane.active_ticket_id,
        "queue_depth": lane.lane_tickets.len(),
        "is_blocked": lane.lane_status == "blocked",
        "is_idle": lane_is_idle(lane)
    })
}

fn lane_is_idle(lane: &Lane) -> bool {
    lane.lane_status == "idle" || (lane.active_ticket_id.is_none() && lane.lane_tickets.is_empty())
}

fn ticket_lane_mismatch(ticket: &TicketSummary, lanes: &[&Lane]) -> Option<String> {
    let lane_owner = ticket_text_field(ticket, "lane_owner");
    let queue_lane = ticket_text_field(ticket, "queue_lane");
    if let Some(owner) = lane_owner.as_deref()
        && !lanes
            .iter()
            .any(|lane| lane.owner_principal.as_deref() == Some(owner))
    {
        return Some("lane_owner_not_in_assigned_lanes".to_string());
    }
    if let Some(queue) = queue_lane.as_deref()
        && !lanes
            .iter()
            .any(|lane| lane.lane_key == queue || lane.lane_id == queue)
    {
        return Some("queue_lane_not_in_assigned_lanes".to_string());
    }
    None
}

fn planning_markdown(workspace_id: &str, tickets: &[TicketSummary]) -> String {
    let mut out = String::new();
    out.push_str("# Studio Planning\n\n");
    out.push_str(&format!(
        "Workspace: `{}`\n\n",
        markdown_inline(workspace_id)
    ));
    out.push_str("## Open Items\n\n");
    if tickets.is_empty() {
        out.push_str("- none\n");
        return out;
    }
    for ticket in tickets {
        let key = markdown_inline(&ticket.primary_key);
        let title = markdown_text(&ticket_title(ticket));
        let status = markdown_text(
            &ticket_text_field(ticket, "status_category")
                .or_else(|| ticket_text_field(ticket, "status"))
                .unwrap_or_else(|| "open".to_string()),
        );
        out.push_str(&format!("- `{key}` {title} ({status})\n"));
    }
    out
}

fn markdown_inline(value: &str) -> String {
    value.replace('`', "'").replace('\n', " ")
}

fn markdown_text(value: &str) -> String {
    value.replace('\n', " ")
}

fn read_sql_query_cbor(store: &mut LoomSqlStore, sql: &str) -> Result<Vec<u8>> {
    let bytes = store.exec_cbor(sql)?;
    if store.in_transaction() {
        return Err(LoomError::invalid(
            "BEGIN without a matching COMMIT/ROLLBACK in one query: use sql.exec for statements that mutate state",
        ));
    }
    if store.is_dirty() {
        return Err(LoomError::new(
            Code::PermissionDenied,
            "sql.query is read-only; use sql.exec for statements that mutate state",
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoreAccess;
    use loom_core::error::Code;
    use loom_core::workspace::FacetKind;
    use loom_core::{
        AclDomain, AclEffect, AclGrant, AclRight, AclScope, AclScopeKind, AclSubject, Algo, Digest,
        IdentityStore, PrincipalKind, Props, cas_put, graph_upsert_edge, graph_upsert_node,
        key_to_cbor, kv_put, ledger_append,
    };
    use loom_store::{open_loom_unlocked, save_loom};
    use loom_substrate::chat::chat_operation_cursor_scope;
    use loom_substrate::drive::{
        DriveContentRef, DriveFileVersion, DriveFileVersionIndex, DriveFolderChildren,
        DriveFolderEntry, DriveFolderIndex, DriveNodeKind, DriveProfileSnapshot, drive_profile_key,
    };
    use loom_substrate::versioning::{BodyRef, Checkpoint, EntityRevision, RevisionIndex};
    use loom_tickets::TicketCreateRequest;
    use std::path::PathBuf;

    fn temp_path() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let uniq = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "loom-mcp-reads-{}-{seq}-{uniq}.loom",
            std::process::id()
        ))
    }

    fn setup<T>(path: &std::path::Path, f: impl FnOnce(&mut Loom<FileStore>) -> T) -> T {
        loom_coordination::with_local_store_write_lock(path, || {
            let store = FileStore::create_with_profile(path, Algo::Blake3).unwrap();
            let mut loom = Loom::new(store);
            let out = f(&mut loom);
            save_loom(&mut loom).unwrap();
            drop(loom);
            Ok(out)
        })
        .unwrap()
    }

    #[test]
    fn board_read_observed_honors_workgraph_task_key_scope() {
        let path = temp_path();
        setup(&path, |loom| {
            let root = WorkspaceId::v4_from_bytes([80; 16]);
            let principal = WorkspaceId::v4_from_bytes([81; 16]);
            let ns = loom
                .registry_mut()
                .create(
                    FacetKind::Files,
                    Some("repo"),
                    WorkspaceId::v4_from_bytes([82; 16]),
                )
                .unwrap();
            let mut identity = IdentityStore::new(root);
            identity
                .add_principal(principal, "scoped", PrincipalKind::User)
                .unwrap();
            identity.bind_session(principal, "workgraph-read").unwrap();
            loom.set_identity_store(identity);
            loom.set_session("workgraph-read");
            loom.acl_store_mut()
                .grant(AclGrant {
                    subject: AclSubject::Principal(principal),
                    workspace: Some(ns),
                    domain: Some(AclDomain::Tickets),
                    ref_glob: None,
                    scopes: vec![AclScope::Prefix {
                        kind: AclScopeKind::Key,
                        prefix: b"workgraph:task-1".to_vec(),
                    }],
                    rights: [AclRight::Read].into_iter().collect(),
                    effect: AclEffect::Allow,
                    predicate: None,
                })
                .unwrap();

            record_board_read_observed(
                loom,
                ns,
                "board-1",
                br#"{"current_task":"task-1","current_batch":"batch-1"}"#,
            )
            .unwrap();
            let denied = record_board_read_observed(
                loom,
                ns,
                "board-2",
                br#"{"current_task":"task-2","current_batch":"batch-1"}"#,
            )
            .unwrap_err();
            assert_eq!(denied.code, Code::PermissionDenied);
        });
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn workgraph_changes_filters_by_task_key_scope() {
        let path = temp_path();
        let mut loom = Loom::new(FileStore::create_with_profile(&path, Algo::Blake3).unwrap());
        let root = WorkspaceId::v4_from_bytes([83; 16]);
        let principal = WorkspaceId::v4_from_bytes([84; 16]);
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([85; 16]),
            )
            .unwrap();
        record_board_read_observed(
            &mut loom,
            ns,
            "board-1",
            br#"{"current_task":"task-1","current_batch":"batch-1"}"#,
        )
        .unwrap();
        record_board_read_observed(
            &mut loom,
            ns,
            "board-2",
            br#"{"current_task":"task-2","current_batch":"batch-1"}"#,
        )
        .unwrap();
        let mut identity = IdentityStore::new(root);
        identity
            .add_principal(principal, "scoped", PrincipalKind::User)
            .unwrap();
        identity
            .bind_session(principal, "workgraph-changes")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session("workgraph-changes");
        loom.acl_store_mut()
            .grant(AclGrant {
                subject: AclSubject::Principal(principal),
                workspace: Some(ns),
                domain: Some(AclDomain::Tickets),
                ref_glob: None,
                scopes: vec![AclScope::Prefix {
                    kind: AclScopeKind::Key,
                    prefix: b"workgraph:task-1".to_vec(),
                }],
                rights: [AclRight::Read].into_iter().collect(),
                effect: AclEffect::Allow,
                predicate: None,
            })
            .unwrap();

        let m = LoomMcp::new(StoreAccess::persistent(loom));
        let batch = m
            .read_workgraph_changes("repo", "batch-1", 1, 10)
            .expect("changes");
        assert_eq!(batch.next, "oplog:3:workgraph:batch-1");
        assert_eq!(batch.events.len(), 1);
        let SubstrateChangeSummary::Operation {
            target_entity_id, ..
        } = &batch.events[0]
        else {
            panic!("expected operation change");
        };
        assert_eq!(target_entity_id.as_deref(), Some("workgraph:task-1"));
        let _ = std::fs::remove_file(path);
    }

    /// Create a fresh loom seeded with one CAS blob and one ledger entry under a named workspace.
    fn seeded(path: &std::path::Path) -> (String, String) {
        setup(path, |loom| {
            let ns = loom
                .registry_mut()
                .create(
                    FacetKind::Cas,
                    Some("app"),
                    WorkspaceId::v4_from_bytes([7u8; 16]),
                )
                .unwrap();
            let digest = cas_put(loom, ns, b"hello cas").unwrap();
            let lns = loom
                .registry_mut()
                .create(
                    FacetKind::Ledger,
                    Some("audit"),
                    WorkspaceId::v4_from_bytes([9u8; 16]),
                )
                .unwrap();
            ledger_append(loom, lns, "events", b"e0".to_vec()).unwrap();
            (digest.to_string(), "app".to_string())
        })
    }

    fn mcp(path: &std::path::Path) -> LoomMcp {
        LoomMcp::new(StoreAccess::per_request(path, None))
    }

    fn define_optional_string_fields(
        m: &LoomMcp,
        workspace: &str,
        workspace_id: &str,
        project_id: &str,
        field_ids: &[&str],
    ) {
        for field_id in field_ids {
            m.write_tickets_field_put(
                workspace,
                loom_tickets::TicketFieldDefinitionWriteRequest {
                    workspace_id,
                    project_id,
                    field_id,
                    key: field_id,
                    name: field_id,
                    description: None,
                    field_type: "string",
                    option_set: None,
                    max_length: Some(512),
                    required: false,
                    searchable: true,
                    orderable: false,
                    cardinality: loom_tickets::TicketFieldCardinality::Optional,
                    applicable_type_ids: &[],
                    expected_root: None,
                },
            )
            .unwrap();
        }
    }

    #[test]
    fn readable_ticket_exposes_assignee_display_alias() {
        // the MCP ticket read projection surfaces the additive `assignee_display` alias
        // alongside the canonical `assignee`. With no identity store registered the display falls
        // back to the stored id string.
        let path = temp_path();
        setup(&path, |loom| {
            loom.registry_mut()
                .create(
                    FacetKind::Files,
                    Some("repo"),
                    WorkspaceId::v4_from_bytes([90u8; 16]),
                )
                .unwrap();
        });
        let m = mcp(&path);
        m.write_tickets_project_create("repo", "studio", "mx", "MX", "Matrix", None)
            .expect("project");
        let created = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "mx",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &serde_json::json!({"title": "Assigned", "assignee": "agent:5"}),
                    policy_labels: &[],
                    expected_root: None,
                },
            )
            .expect("ticket");
        let readable = m
            .read_tickets_get_readable("repo", "studio", &created.primary_key, None)
            .unwrap()
            .unwrap();
        assert_eq!(readable["assignee"], serde_json::json!("agent:5"));
        assert_eq!(readable["assignee_display"], serde_json::json!("agent:5"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn workgraph_metrics_derives_from_tickets_and_lanes() {
        let path = temp_path();
        setup(&path, |loom| {
            loom.registry_mut()
                .create(
                    FacetKind::Files,
                    Some("repo"),
                    WorkspaceId::v4_from_bytes([72u8; 16]),
                )
                .unwrap();
        });
        let m = mcp(&path);
        m.write_tickets_project_create("repo", "studio", "mx", "MX", "Matrix", None)
            .expect("project");
        define_optional_string_fields(&m, "repo", "studio", "mx", &["lane_owner", "queue_lane"]);
        let one = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "mx",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &serde_json::json!({
                        "title": "Needs review",
                        "status": "waiting_for_review",
                        "lane_owner": "agent:1",
                        "queue_lane": "matrix-workflow"
                    }),
                    policy_labels: &[],
                    expected_root: None,
                },
            )
            .expect("ticket one");
        let two = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "mx",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &serde_json::json!({
                        "title": "Has feedback",
                        "status": "feedback_available",
                        "lane_owner": "agent:1",
                        "queue_lane": "matrix-workflow"
                    }),
                    policy_labels: &[],
                    expected_root: Some(&one.profile_root),
                },
            )
            .expect("ticket two");
        let three = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "mx",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &serde_json::json!({
                        "title": "Blocked",
                        "status": "blocked",
                        "lane_owner": "agent:2",
                        "queue_lane": "other-lane"
                    }),
                    policy_labels: &[],
                    expected_root: Some(&two.profile_root),
                },
            )
            .expect("ticket three");
        let four = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "mx",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &serde_json::json!({
                        "title": "Accepted",
                        "status": "accepted"
                    }),
                    policy_labels: &[],
                    expected_root: Some(&three.profile_root),
                },
            )
            .expect("ticket four");
        assert_eq!(four.primary_key, "MX-4");
        m.write_lanes_create(
            "repo",
            crate::writes::LaneCreateRequest {
                lane_id: "agent-1",
                lane_key: "matrix-workflow",
                title: "Agent 1 lane",
                description: "Workgraph metrics test lane.",
                lane_kind: loom_lanes::LaneKind::Assignment.as_str(),
                owner_principal: Some("agent:1"),
                lane_status: "ready",
                lane_tickets: &[
                    loom_lanes::LaneTicket {
                        ticket_id: one.primary_key.clone(),
                        order_key: "F".to_string(),
                    },
                    loom_lanes::LaneTicket {
                        ticket_id: three.primary_key.clone(),
                        order_key: "V".to_string(),
                    },
                ],
                active_ticket_id: Some(&one.primary_key),
                status_report: "ready",
                reviewer_feedback: "",
                updated_by: Some("agent:1"),
            },
        )
        .expect("lane");
        let metrics = m
            .read_workgraph_metrics("repo", Some("studio"), &[], &[], Some(10))
            .expect("metrics");
        assert_eq!(metrics["ticket_counts"]["waiting_for_review"], 1);
        assert_eq!(metrics["ticket_counts"]["feedback_available"], 1);
        assert_eq!(metrics["ticket_counts"]["blocked"], 1);
        assert_eq!(metrics["ticket_counts"]["accepted"], 1);
        assert_eq!(metrics["lanes"][0]["active_ticket_id"], "MX-1");
        assert_eq!(metrics["lanes"][0]["queue_depth"], 2);
        assert!(
            metrics["lane_assignment_mismatches"]["items"]
                .as_array()
                .expect("mismatch items")
                .iter()
                .any(|item| item["primary_key"] == "MX-3")
        );
        let filtered = m
            .read_workgraph_metrics(
                "repo",
                Some("studio"),
                &["feedback_available".to_string()],
                &[],
                Some(1),
            )
            .expect("filtered");
        assert_eq!(filtered["cohort"]["returned"], 1);
        assert_eq!(filtered["cohort"]["items"][0]["primary_key"], "MX-2");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lane_views_resolve_ticket_state_and_count_missing_references() {
        let path = temp_path();
        let profile_id = setup(&path, |loom| {
            let id = WorkspaceId::v4_from_bytes([73u8; 16]);
            loom.registry_mut()
                .create(FacetKind::Files, Some("repo"), id)
                .unwrap();
            id.to_string()
        });
        let m = mcp(&path);
        m.write_tickets_project_create("repo", &profile_id, "mx", "MX", "Matrix", None)
            .expect("project");
        let ready = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: &profile_id,
                    project_id: "mx",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &serde_json::json!({
                        "title": "Ready task",
                        "status": "ready",
                        "priority": "P1"
                    }),
                    policy_labels: &[],
                    expected_root: None,
                },
            )
            .expect("ready ticket");
        let blocked = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: &profile_id,
                    project_id: "mx",
                    ticket_type: "bug",
                    external_source: None,
                    external_id: None,
                    fields: &serde_json::json!({
                        "title": "Blocked bug",
                        "status": "blocked",
                        "priority": "P0"
                    }),
                    policy_labels: &[],
                    expected_root: Some(&ready.profile_root),
                },
            )
            .expect("blocked ticket");
        m.write_lanes_create(
            "repo",
            crate::writes::LaneCreateRequest {
                lane_id: "agent-views",
                lane_key: "matrix-workflow",
                title: "Agent view lane",
                description: "Lane view projection regression.",
                lane_kind: loom_lanes::LaneKind::Assignment.as_str(),
                owner_principal: Some("agent:views"),
                lane_status: "ready",
                lane_tickets: &[
                    loom_lanes::LaneTicket {
                        ticket_id: ready.primary_key.clone(),
                        order_key: "F".to_string(),
                    },
                    loom_lanes::LaneTicket {
                        ticket_id: blocked.primary_key.clone(),
                        order_key: "V".to_string(),
                    },
                    loom_lanes::LaneTicket {
                        ticket_id: "MX-999".to_string(),
                        order_key: "l".to_string(),
                    },
                ],
                active_ticket_id: Some(&ready.primary_key),
                status_report: "ready",
                reviewer_feedback: "",
                updated_by: Some("agent:views"),
            },
        )
        .expect("lane");

        let view = m
            .read_lanes_get_view("repo", "agent-views")
            .expect("lane view")
            .expect("lane exists");
        assert_eq!(view.status_counts.total, 3);
        assert_eq!(view.status_counts.backlog, 1);
        assert_eq!(view.status_counts.blocked, 1);
        assert_eq!(view.status_counts.missing, 1);
        assert_eq!(view.status_counts.next_ticket_id.as_deref(), Some("MX-1"));
        assert_eq!(view.lane_tickets[0].status.as_deref(), Some("ready"));
        assert_eq!(view.lane_tickets[0].title.as_deref(), Some("Ready task"));
        assert_eq!(view.lane_tickets[0].priority.as_deref(), Some("P1"));
        assert_eq!(view.lane_tickets[1].status.as_deref(), Some("blocked"));
        assert_eq!(view.lane_tickets[1].title.as_deref(), Some("Blocked bug"));
        assert_eq!(view.lane_tickets[2].status.as_deref(), Some("missing"));
        assert_eq!(view.lane_tickets[2].title, None);

        let (views, diagnostics) = m
            .read_lanes_list_views_with_diagnostics("repo")
            .expect("lane views");
        assert!(diagnostics.is_empty());
        assert_eq!(views[0].status_counts, view.status_counts);

        let listed = m
            .read_tickets_page(
                "repo",
                &profile_id,
                loom_tickets::TicketListQuery {
                    include_completed: true,
                    lane_member_ids: Some(vec![
                        ready.primary_key.clone(),
                        blocked.primary_key.clone(),
                        "MX-999".to_string(),
                    ]),
                    limit: Some(10),
                    ..Default::default()
                },
            )
            .expect("ticket list");
        assert_eq!(listed.total, 2);
        let listed_statuses = listed
            .items
            .iter()
            .map(|ticket| (ticket.primary_key.as_str(), ticket_status(ticket)))
            .collect::<Vec<_>>();
        assert!(listed_statuses.contains(&("MX-1", "ready".to_string())));
        assert!(listed_statuses.contains(&("MX-2", "blocked".to_string())));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn blob_digest_is_pure_and_stable() {
        let m = mcp(std::path::Path::new("/nonexistent.loom"));
        let a = m.read_blob_digest(b"hello cas");
        let b = m.read_blob_digest(b"hello cas");
        assert_eq!(a, b);
        assert!(a.contains(':'));
    }

    #[test]
    fn capabilities_cross_the_pep_as_cbor() {
        let path = temp_path();
        let _ = seeded(&path);
        let bytes = mcp(&path).read_capabilities().expect("capabilities read");
        let decoded = loom_codec::decode(&bytes).expect("capabilities cbor");
        let loom_codec::Value::Map(set_pairs) = decoded else {
            panic!("capabilities are a record set");
        };
        let Some(loom_codec::Value::Array(items)) =
            set_pairs.iter().find_map(|(key, value)| match key {
                loom_codec::Value::Text(key) if key == "records" => Some(value),
                _ => None,
            })
        else {
            panic!("capabilities include records");
        };
        let supported = |name: &str| {
            items.iter().any(|item| {
                let loom_codec::Value::Map(pairs) = item else {
                    return false;
                };
                let field = |key: &str| {
                    pairs
                        .iter()
                        .find(|(k, _)| matches!(k, loom_codec::Value::Text(t) if t == key))
                        .map(|(_, v)| v)
                };
                matches!(field("capability_id"), Some(loom_codec::Value::Text(n)) if n == name)
                    && matches!(
                        field("operational_state"),
                        Some(loom_codec::Value::Text(state)) if state == "supported"
                    )
                    && field("supported").is_none()
            })
        };
        assert!(supported(crate::MCP_HOST_CAPABILITY));
        assert!(supported(crate::MCP_APPS_CAPABILITY));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn capabilities_cross_the_pep_as_json() {
        let path = temp_path();
        let _ = seeded(&path);
        let json = mcp(&path)
            .read_capabilities_json(false)
            .expect("capabilities json");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json shape");
        let records = value["records"].as_array().expect("records array");
        assert!(records.iter().any(|record| {
            record["capability_id"] == crate::MCP_HOST_CAPABILITY
                && record["operational_state"] == "supported"
        }));
        assert!(
            records
                .iter()
                .all(|record| record["operational_state"] != "target")
        );
        assert!(
            records
                .iter()
                .all(|record| record.get("dimensions").is_some())
        );
        let detailed = mcp(&path)
            .read_capabilities_json(true)
            .expect("detailed capabilities json");
        let detailed: serde_json::Value = serde_json::from_str(&detailed).expect("json shape");
        assert!(
            detailed["records"]
                .as_array()
                .expect("records array")
                .iter()
                .any(|record| record["operational_state"] == "target")
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn workspace_list_and_get_round_trip() {
        let path = temp_path();
        seeded(&path);
        let m = mcp(&path);
        let list = m.read_workspace_list().expect("list");
        assert!(list.iter().any(|n| n.name == "app"));
        assert!(list.iter().any(|n| n.name == "audit"));
        let app = m.read_workspace_get("app").expect("get").expect("present");
        assert_eq!(app.name, "app");
        assert!(app.facets.iter().any(|f| f == "cas"));
        assert!(m.read_workspace_get("missing").expect("get").is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn workspace_reads_require_global_admin_in_authenticated_mode() {
        let path = temp_path();
        let _ = seeded(&path);
        let mut loom = open_loom_unlocked(&path, None).unwrap();
        let root = WorkspaceId::v4_from_bytes([117u8; 16]);
        let mut identity = loom_core::IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);
        let m = LoomMcp::new(StoreAccess::persistent(loom));

        assert_eq!(
            m.read_workspace_list().unwrap_err().code,
            Code::PermissionDenied
        );
        assert_eq!(
            m.read_workspace_get("app").unwrap_err().code,
            Code::PermissionDenied
        );
        m.store()
            .write(|loom| {
                loom.acl_store_mut().allow(
                    loom_core::AclSubject::Principal(root),
                    None,
                    None,
                    [loom_core::AclRight::Admin],
                )
            })
            .unwrap();
        assert!(
            m.read_workspace_list()
                .unwrap()
                .iter()
                .any(|entry| entry.name == "app")
        );
        assert_eq!(m.read_workspace_get("app").unwrap().unwrap().name, "app");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cas_reads_round_trip() {
        let path = temp_path();
        let (digest, ns) = seeded(&path);
        let m = mcp(&path);
        assert_eq!(
            m.read_cas_get(&ns, &digest).expect("get"),
            Some(b"hello cas".to_vec())
        );
        assert!(m.read_cas_has(&ns, &digest).expect("has"));
        assert!(m.read_cas_list(&ns).expect("list").contains(&digest));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ledger_reads_round_trip() {
        let path = temp_path();
        seeded(&path);
        let m = mcp(&path);
        assert_eq!(m.read_ledger_len("audit", "events").expect("len"), 1);
        assert_eq!(
            m.read_ledger_get("audit", "events", 0).expect("get"),
            Some(b"e0".to_vec())
        );
        assert!(
            m.read_ledger_head("audit", "events")
                .expect("head")
                .is_some()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn kv_list_on_absent_map_is_decodable_and_empty() {
        let path = temp_path();
        seeded(&path);
        let m = mcp(&path);
        let bytes = m.read_kv_list("app", "missing").expect("kv list");
        let map = loom_core::KvMap::decode(&bytes).expect("decode");
        assert_eq!(map.len(), 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn kv_range_filters_with_key_predicate() {
        let path = temp_path();
        setup(&path, |loom| {
            let ns = loom
                .registry_mut()
                .create(
                    FacetKind::Kv,
                    Some("app"),
                    WorkspaceId::v4_from_bytes([31u8; 16]),
                )
                .unwrap();
            kv_put(
                loom,
                ns,
                "items",
                TabularValue::Text("a".to_string()),
                b"one".to_vec(),
            )
            .unwrap();
            kv_put(
                loom,
                ns,
                "items",
                TabularValue::Text("b".to_string()),
                b"two".to_vec(),
            )
            .unwrap();
            kv_put(
                loom,
                ns,
                "items",
                TabularValue::Text("c".to_string()),
                b"three".to_vec(),
            )
            .unwrap();
        });
        let predicate = serde_json::json!({
            "version": 1,
            "expr": {
                "op": "gte",
                "path": ["key"],
                "value": { "type": "text", "value": "b" }
            }
        });
        let bytes = mcp(&path)
            .read_kv_range(
                "app",
                "items",
                &key_to_cbor(&TabularValue::Text("a".to_string())),
                &key_to_cbor(&TabularValue::Text("z".to_string())),
                Some(&predicate),
            )
            .expect("kv range");
        let map = KvMap::decode(&bytes).expect("decode");
        let keys: Vec<_> = map.iter().map(|(key, _)| key.clone()).collect();
        assert_eq!(
            keys,
            vec![
                TabularValue::Text("b".to_string()),
                TabularValue::Text("c".to_string())
            ]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn kv_range_rejects_value_predicate_path() {
        let path = temp_path();
        seeded(&path);
        let predicate = serde_json::json!({
            "version": 1,
            "expr": {
                "op": "eq",
                "path": ["value"],
                "value": { "type": "bytes", "value": "01" }
            }
        });
        let err = mcp(&path)
            .read_kv_range(
                "app",
                "missing",
                &key_to_cbor(&TabularValue::Text("a".to_string())),
                &key_to_cbor(&TabularValue::Text("z".to_string())),
                Some(&predicate),
            )
            .unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn sql_query_is_read_only() {
        let path = temp_path();
        setup(&path, |_| {});
        let m = mcp(&path);
        m.write_sql_exec(
            "appdb",
            "main",
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)",
        )
        .expect("create table");
        m.write_sql_exec("appdb", "main", "INSERT INTO t VALUES (1, 'a')")
            .expect("insert");

        let rows = m
            .read_sql_query("appdb", "main", "SELECT id, v FROM t")
            .expect("query");
        assert!(!rows.is_empty());

        let err = m
            .read_sql_query("appdb", "main", "CREATE TABLE u (id INTEGER PRIMARY KEY)")
            .unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);
        assert!(m.read_sql_read_table("appdb", "main", "u").is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn mail_get_message_is_structured_and_to_eml_is_raw() {
        let path = temp_path();
        let raw: &[u8] = b"From: bob@x.io\r\nSubject: Hi\r\nDate: x\r\n\r\nbody";
        setup(&path, |loom| {
            let ns = loom
                .registry_mut()
                .create(
                    FacetKind::Mail,
                    Some("mbox"),
                    WorkspaceId::v4_from_bytes([3u8; 16]),
                )
                .unwrap();
            mail::create_mailbox(loom, ns, "alice", "inbox", &Default::default()).unwrap();
            mail::ingest_message(loom, ns, "alice", "inbox", "m1", raw).unwrap();
        });

        let m = mcp(&path);
        let msg = m
            .read_mail_get_message("mbox", "alice", "inbox", "m1")
            .expect("get_message")
            .expect("present");
        assert_eq!(msg.from, "bob@x.io");
        assert_eq!(msg.subject, "Hi");
        let eml = m
            .read_mail_to_eml("mbox", "alice", "inbox", "m1")
            .expect("to_eml");
        assert_eq!(eml.as_deref(), Some(raw));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn drive_reads_project_snapshot_and_verified_cas_content() {
        let path = temp_path();
        let digest = setup(&path, |loom| {
            let ns = loom
                .registry_mut()
                .create(
                    FacetKind::Vcs,
                    Some("repo"),
                    WorkspaceId::v4_from_bytes([30u8; 16]),
                )
                .unwrap();
            loom.registry_mut().add_facet(ns, FacetKind::Cas).unwrap();
            let digest = cas_put(loom, ns, b"hello drive").unwrap();
            let snapshot = DriveProfileSnapshot::new(
                "main",
                DriveFolderIndex::new(
                    "main",
                    vec![
                        DriveFolderChildren::new(
                            "root",
                            vec![
                                DriveFolderEntry::new("Specs", "folder-1", DriveNodeKind::Folder)
                                    .unwrap(),
                                DriveFolderEntry::new("Plan.txt", "file-1", DriveNodeKind::File)
                                    .unwrap(),
                            ],
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
                DriveFileVersionIndex::new(
                    "main",
                    vec![
                        DriveFileVersion::new(
                            "file-1",
                            1,
                            "op-1",
                            ns,
                            100,
                            DriveContentRef::Blob { digest, size: 11 },
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
            )
            .unwrap();
            loom.store()
                .control_set(
                    &drive_profile_key("main").unwrap(),
                    snapshot.encode().unwrap(),
                )
                .unwrap();
            digest
        });
        let m = mcp(&path);

        let list = m.read_drive_list("repo", "main", "root").unwrap();
        assert_eq!(list.entries.len(), 2);
        assert_eq!(list.entries[1].name, "Plan.txt");
        assert_eq!(list.entries[1].kind, "file");
        let stat = m
            .read_drive_stat("repo", "main", "root", "plan.txt")
            .unwrap();
        assert_eq!(stat.node_id, "file-1");
        assert_eq!(stat.latest_version.as_ref().unwrap().version, 1);
        assert_eq!(
            m.read_drive_read("repo", "main", "file-1").unwrap(),
            b"hello drive".to_vec()
        );
        assert_eq!(
            m.read_drive_list_versions("repo", "main", "file-1")
                .unwrap()[0]
                .content_digest,
            digest.to_string()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn queue_reads_round_trip() {
        let path = temp_path();
        setup(&path, |loom| {
            let ns = loom
                .registry_mut()
                .create(
                    FacetKind::Queue,
                    Some("jobs"),
                    WorkspaceId::v4_from_bytes([5u8; 16]),
                )
                .unwrap();
            loom.stream_append(ns, "q", b"a").unwrap();
            loom.stream_append(ns, "q", b"b").unwrap();
        });

        let m = mcp(&path);
        assert_eq!(m.read_queue_len("jobs", "q").expect("len"), 2);
        assert_eq!(
            m.read_queue_get("jobs", "q", 0).expect("get"),
            Some(b"a".to_vec())
        );
        assert_eq!(
            m.read_queue_range("jobs", "q", 0, 2).expect("range"),
            vec![b"a".to_vec(), b"b".to_vec()]
        );
        assert_eq!(
            m.read_queue_consumer_position("jobs", "q", "c1")
                .expect("pos"),
            0
        );
        assert_eq!(
            m.read_queue_consumer_read("jobs", "q", "c1", 10)
                .expect("read"),
            vec![b"a".to_vec(), b"b".to_vec()]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn vcs_reads_round_trip_and_sql_errors_gracefully() {
        let path = temp_path();
        let head = setup(&path, |loom| {
            let ns = loom
                .registry_mut()
                .create(
                    FacetKind::Files,
                    Some("repo"),
                    WorkspaceId::v4_from_bytes([4u8; 16]),
                )
                .unwrap();
            loom.write_file(ns, "readme.txt", b"hello", 0o644).unwrap();
            loom.commit(ns, "seed", "c1", 0).unwrap();
            loom.registry().head_branch(ns).unwrap()
        });

        let m = mcp(&path);
        assert_eq!(m.read_vcs_head_branch("repo").expect("head"), head);
        let log = m.read_vcs_log("repo", &head).expect("log");
        assert_eq!(log.len(), 1);
        let status = m.read_vcs_status("repo").expect("status");
        assert!(status.conflicts.is_empty());
        assert!(!m.read_vcs_merge_in_progress("repo").expect("merge"));
        assert!(m.read_vcs_tag_list("repo").expect("tags").is_empty());
        // A non-SQL workspace has no staged table: the sql reader errors rather than panicking.
        assert!(m.read_sql_read_table("repo", "main", "t").is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn watch_reads_round_trip() {
        let path = temp_path();
        let (c0, c1) = setup(&path, |loom| {
            let ns = loom
                .registry_mut()
                .create(
                    FacetKind::Files,
                    Some("repo"),
                    WorkspaceId::v4_from_bytes([12u8; 16]),
                )
                .unwrap();
            loom.registry_mut()
                .create(
                    FacetKind::Files,
                    Some("other"),
                    WorkspaceId::v4_from_bytes([13u8; 16]),
                )
                .unwrap();
            loom.write_file(ns, "a.txt", b"a", 0o644).unwrap();
            let c0 = loom.commit(ns, "seed", "c0", 0).unwrap();
            loom.write_file(ns, "a.txt", b"a2", 0o644).unwrap();
            loom.write_file(ns, "b.txt", b"b", 0o644).unwrap();
            let c1 = loom.commit(ns, "seed", "c1", 1).unwrap();
            (c0, c1)
        });

        let m = mcp(&path);
        let tip = m
            .read_watch_subscribe(
                "repo",
                loom_core::workspace::DEFAULT_BRANCH,
                None,
                None,
                None,
                None,
            )
            .expect("subscribe");
        assert!(
            m.read_watch_poll("repo", &tip.cursor, 10)
                .expect("poll")
                .events
                .is_empty()
        );

        let resumed = m
            .read_watch_subscribe(
                "repo",
                loom_core::workspace::DEFAULT_BRANCH,
                Some(&c0.to_string()),
                None,
                None,
                None,
            )
            .expect("resume");
        let batch = m
            .read_watch_poll("repo", &resumed.cursor, 10)
            .expect("poll resumed");
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].commit, c1.to_string());
        assert_eq!(batch.events[0].parent, Some(c0.to_string()));
        assert_eq!(
            batch.events[0].changes,
            vec![
                DomainChangeSummary {
                    domain: "files".to_string(),
                    schema_version: 1,
                    kind: "modified".to_string(),
                    key: b"a.txt".to_vec(),
                    before: batch.events[0].changes[0].before.clone(),
                    after: batch.events[0].changes[0].after.clone(),
                    detail: None,
                },
                DomainChangeSummary {
                    domain: "files".to_string(),
                    schema_version: 1,
                    kind: "added".to_string(),
                    key: b"b.txt".to_vec(),
                    before: None,
                    after: batch.events[0].changes[1].after.clone(),
                    detail: None,
                },
            ]
        );
        assert_eq!(batch.events[0].unsupported_domains, Vec::new());
        assert!(
            m.read_watch_poll("repo", &batch.next, 10)
                .expect("poll advanced")
                .events
                .is_empty()
        );
        assert_eq!(
            m.read_watch_poll("other", &resumed.cursor, 10)
                .unwrap_err()
                .code,
            Code::CursorInvalid
        );
        let narrowed = m
            .read_watch_subscribe(
                "repo",
                loom_core::workspace::DEFAULT_BRANCH,
                Some(&c0.to_string()),
                Some("files"),
                Some("a."),
                None,
            )
            .expect("narrowed subscribe");
        let narrowed_batch = m
            .read_watch_poll("repo", &narrowed.cursor, 10)
            .expect("narrowed poll");
        assert_eq!(narrowed_batch.events.len(), 1);
        assert_eq!(
            narrowed_batch.events[0].changes,
            vec![DomainChangeSummary {
                domain: "files".to_string(),
                schema_version: 1,
                kind: "modified".to_string(),
                key: b"a.txt".to_vec(),
                before: narrowed_batch.events[0].changes[0].before.clone(),
                after: narrowed_batch.events[0].changes[0].after.clone(),
                detail: None,
            }]
        );
        let added_only = vec!["added".to_string()];
        let kind_narrowed = m
            .read_watch_subscribe(
                "repo",
                loom_core::workspace::DEFAULT_BRANCH,
                Some(&c0.to_string()),
                Some("files"),
                None,
                Some(&added_only),
            )
            .expect("kind narrowed subscribe");
        let kind_narrowed_batch = m
            .read_watch_poll("repo", &kind_narrowed.cursor, 10)
            .expect("kind narrowed poll");
        assert_eq!(kind_narrowed_batch.events.len(), 1);
        assert_eq!(kind_narrowed_batch.events[0].changes.len(), 1);
        assert_eq!(kind_narrowed_batch.events[0].changes[0].kind, "added");
        assert_eq!(
            kind_narrowed_batch.events[0].changes[0].key,
            b"b.txt".to_vec()
        );
        assert_eq!(
            m.read_watch_subscribe(
                "repo",
                loom_core::workspace::DEFAULT_BRANCH,
                None,
                Some("sql"),
                None,
                None,
            )
            .unwrap_err()
            .code,
            Code::Unsupported
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn substrate_changes_include_lmdiff_and_next_cursor() {
        let path = temp_path();
        let (c0, c1) = setup(&path, |loom| {
            let ns = loom
                .registry_mut()
                .create(
                    FacetKind::Files,
                    Some("repo"),
                    WorkspaceId::v4_from_bytes([14u8; 16]),
                )
                .unwrap();
            loom.write_file(ns, "a.txt", b"a", 0o644).unwrap();
            let c0 = loom.commit(ns, "seed", "c0", 0).unwrap();
            loom.write_file(ns, "a.txt", b"a2", 0o644).unwrap();
            loom.write_file(ns, "b.txt", b"b", 0o644).unwrap();
            let c1 = loom.commit(ns, "seed", "c1", 1).unwrap();
            (c0, c1)
        });

        let m = mcp(&path);
        let resumed = m
            .read_watch_subscribe(
                "repo",
                loom_core::workspace::DEFAULT_BRANCH,
                Some(&c0.to_string()),
                None,
                None,
                None,
            )
            .expect("resume");
        let batch = m
            .read_substrate_changes("repo", &resumed.cursor, 10)
            .expect("changes");
        assert_eq!(batch.events.len(), 1);
        let SubstrateChangeSummary::Data {
            commit,
            parent,
            changes,
            lmdiff,
            ..
        } = &batch.events[0]
        else {
            panic!("expected data change");
        };
        assert_eq!(commit, &c1.to_string());
        assert_eq!(parent, &Some(c0.to_string()));
        assert_eq!(changes.len(), 2);
        assert!(lmdiff.as_ref().expect("lmdiff").starts_with(&[0x86]));
        assert!(
            m.read_substrate_changes("repo", &batch.next, 10)
                .expect("advanced")
                .events
                .is_empty()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn substrate_changes_read_ticket_operation_log_cursor() {
        let path = temp_path();
        setup(&path, |loom| {
            loom.registry_mut()
                .create(
                    FacetKind::Files,
                    Some("repo"),
                    WorkspaceId::v4_from_bytes([21u8; 16]),
                )
                .unwrap();
        });

        let m = mcp(&path);
        let project = m
            .write_tickets_project_create("repo", "studio", "eng", "ENG", "Engineering", None)
            .expect("project");
        let ticket = m
            .write_tickets_create(
                "repo",
                TicketCreateRequest {
                    workspace_id: "studio",
                    project_id: "eng",
                    ticket_type: "task",
                    external_source: None,
                    external_id: None,
                    fields: &serde_json::json!({ "title": "Build tickets" }),
                    policy_labels: &["internal".to_string()],
                    expected_root: Some(&project.profile_root),
                },
            )
            .expect("ticket");

        let batch = m
            .read_substrate_changes("repo", "oplog:1:tickets:studio", 10)
            .expect("changes");
        assert_eq!(batch.next, "oplog:3:tickets:studio");
        assert_eq!(batch.events.len(), 2);
        let SubstrateChangeSummary::Operation {
            workspace_id,
            app_id,
            scope_id,
            operation_id,
            operation_kind,
            sequence,
            root_after,
            target_entity_id,
            policy_labels,
            ..
        } = &batch.events[1]
        else {
            panic!("expected operation change");
        };
        assert_eq!(workspace_id, "studio");
        assert_eq!(app_id, "tickets");
        assert_eq!(scope_id, "eng");
        assert_eq!(operation_id, "studio:2");
        assert_eq!(operation_kind, "ticket.created");
        assert_eq!(*sequence, 2);
        assert!(Digest::parse(root_after).is_ok());
        assert_eq!(target_entity_id.as_deref(), Some(ticket.ticket_id.as_str()));
        assert_eq!(policy_labels, &vec!["internal".to_string()]);
        assert!(
            m.read_substrate_changes("repo", &batch.next, 10)
                .expect("advanced")
                .events
                .is_empty()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn substrate_changes_read_chat_operation_log_cursor() {
        let path = temp_path();
        setup(&path, |loom| {
            loom.registry_mut()
                .create(
                    FacetKind::Vcs,
                    Some("repo"),
                    WorkspaceId::v4_from_bytes([34u8; 16]),
                )
                .unwrap();
        });

        let m = mcp(&path);
        let channel = m
            .write_chat_create_channel("repo", "studio", "general", "General")
            .expect("chat channel");
        m.write_chat_post_message("repo", "studio", "general", "m1", None, b"hello".to_vec())
            .expect("chat message");
        let cursor = OperationChangeCursor::new(
            chat_operation_cursor_scope("studio", &channel.channel_id),
            1,
        )
        .unwrap()
        .encode();
        let batch = m
            .read_substrate_changes("repo", &cursor, 10)
            .expect("chat changes");

        assert_eq!(
            batch.next,
            format!("oplog:2:chat:studio:{}", channel.channel_id)
        );
        assert_eq!(batch.events.len(), 1);
        let SubstrateChangeSummary::Operation {
            workspace_id,
            app_id,
            scope_id,
            operation_id,
            operation_kind,
            sequence,
            policy_labels,
            ..
        } = &batch.events[0]
        else {
            panic!("expected operation change");
        };
        assert_eq!(workspace_id, "studio");
        assert_eq!(app_id, "chat");
        assert_eq!(scope_id, &channel.channel_id);
        assert_eq!(operation_id, &format!("studio:{}:1", channel.channel_id));
        assert_eq!(operation_kind, "message.created");
        assert_eq!(*sequence, 1);
        assert!(policy_labels.is_empty());
        assert!(
            m.read_substrate_changes("repo", &batch.next, 10)
                .expect("advanced")
                .events
                .is_empty()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn substrate_refs_scan_readable_document_collections() {
        let path = temp_path();
        setup(&path, |loom| {
            let ns = loom
                .registry_mut()
                .create(
                    FacetKind::Document,
                    Some("docs"),
                    WorkspaceId::v4_from_bytes([15u8; 16]),
                )
                .unwrap();
            loom_core::document::doc_put(
                loom,
                ns,
                "pages",
                "intro",
                b"See !ticket:LOOM-1 and !page:Roadmap.".to_vec(),
            )
            .unwrap();
            loom_core::document::doc_put(loom, ns, "pages", "notes", b"No target here.".to_vec())
                .unwrap();
            loom_core::document::doc_put(loom, ns, "bin", "blob", vec![0xff, 0xfe]).unwrap();
            graph_upsert_node(loom, ns, "links", "page:Roadmap", Props::new()).unwrap();
            graph_upsert_node(loom, ns, "links", "ticket:LOOM-1", Props::new()).unwrap();
            graph_upsert_edge(
                loom,
                ns,
                "links",
                "edge-1",
                "page:Roadmap",
                "ticket:LOOM-1",
                "refers_to",
                Props::new(),
            )
            .unwrap();
        });

        let result = mcp(&path)
            .read_substrate_refs("docs", "ticket:LOOM-1")
            .expect("refs");
        assert_eq!(result.target, "ticket:LOOM-1");
        assert_eq!(result.indexed_facets, vec!["document", "graph"]);
        assert!(result.degraded.is_degraded);
        assert_eq!(result.inbound.len(), 2);
        let doc_edge = result
            .inbound
            .iter()
            .find(|edge| edge.source_facet == "document")
            .expect("document edge");
        assert_eq!(doc_edge.source_collection, "pages");
        assert_eq!(doc_edge.source_id, "intro");
        assert_eq!(doc_edge.field, "body");
        assert_eq!(doc_edge.relation, "refers_to");
        assert_eq!(doc_edge.evidence, "!ticket:LOOM-1");
        let graph_edge = result
            .inbound
            .iter()
            .find(|edge| edge.source_facet == "graph")
            .expect("graph edge");
        assert_eq!(graph_edge.source_collection, "links");
        assert_eq!(graph_edge.source_id, "edge-1");
        assert_eq!(graph_edge.field, "edge");
        assert_eq!(graph_edge.relation, "refers_to");
        assert_eq!(graph_edge.evidence, "page:Roadmap refers_to ticket:LOOM-1");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn substrate_refs_prefers_reserved_reference_index() {
        let path = temp_path();
        setup(&path, |loom| {
            let ns = loom
                .registry_mut()
                .create(
                    FacetKind::Document,
                    Some("docs"),
                    WorkspaceId::v4_from_bytes([18u8; 16]),
                )
                .unwrap();
            loom_core::document::doc_put(
                loom,
                ns,
                "pages",
                "intro",
                b"See !ticket:SCAN-ONLY.".to_vec(),
            )
            .unwrap();
            let mut index = ReferenceIndex::new();
            index
                .add_text_refs(
                    ReferenceSource::new("tickets", "studio", "ticket-1", "description").unwrap(),
                    "refers_to",
                    "See !ticket:INDEXED.",
                )
                .unwrap();
            loom.create_directory_reserved(ns, crate::substrate_refs::REF_INDEX_DIR, true)
                .unwrap();
            loom.write_file_reserved(
                ns,
                crate::substrate_refs::REF_INDEX_PATH,
                &index.encode().unwrap(),
                0o100644,
            )
            .unwrap();
        });

        let result = mcp(&path)
            .read_substrate_refs("docs", "ticket:INDEXED")
            .expect("refs");

        assert_eq!(result.target, "ticket:INDEXED");
        assert!(!result.degraded.is_degraded);
        assert_eq!(result.degraded.reason, "");
        assert_eq!(result.indexed_facets, vec!["tickets"]);
        assert_eq!(result.inbound.len(), 1);
        assert_eq!(result.inbound[0].source_facet, "tickets");
        assert_eq!(result.inbound[0].source_collection, "studio");
        assert_eq!(
            mcp(&path)
                .read_substrate_refs("docs", "ticket:SCAN-ONLY")
                .expect("refs")
                .inbound
                .len(),
            0
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn substrate_history_reads_reserved_revision_index() {
        let path = temp_path();
        setup(&path, |loom| {
            let ns = loom
                .registry_mut()
                .create(
                    FacetKind::Document,
                    Some("docs"),
                    WorkspaceId::v4_from_bytes([16u8; 16]),
                )
                .unwrap();
            let mut index = RevisionIndex::new();
            index
                .append_revision(
                    EntityRevision::new(
                        "page:roadmap",
                        1,
                        "op-1",
                        BodyRef::new(Digest::hash(Algo::Blake3, b"body-1"), 6, "text/markdown")
                            .unwrap(),
                        Digest::hash(Algo::Blake3, b"root-1"),
                        10,
                    )
                    .unwrap(),
                )
                .unwrap();
            index
                .append_revision(
                    EntityRevision::new(
                        "page:roadmap",
                        2,
                        "op-2",
                        BodyRef::new(Digest::hash(Algo::Blake3, b"body-2"), 8, "text/markdown")
                            .unwrap(),
                        Digest::hash(Algo::Blake3, b"root-2"),
                        20,
                    )
                    .unwrap(),
                )
                .unwrap();
            index
                .add_checkpoint(
                    Checkpoint::new(
                        "studio",
                        "cp-1",
                        Digest::hash(Algo::Blake3, b"root-1"),
                        1,
                        "op-1",
                        11,
                    )
                    .unwrap(),
                )
                .unwrap();
            index
                .add_checkpoint(
                    Checkpoint::new(
                        "other",
                        "cp-other",
                        Digest::hash(Algo::Blake3, b"root-other"),
                        1,
                        "op-other",
                        12,
                    )
                    .unwrap(),
                )
                .unwrap();
            let path_in_loom = crate::substrate_revisions::revision_index_path("studio").unwrap();
            loom.create_directory_reserved(
                ns,
                crate::substrate_revisions::REVISION_INDEX_DIR,
                true,
            )
            .unwrap();
            loom.write_file_reserved(ns, &path_in_loom, &index.encode().unwrap(), 0o100644)
                .unwrap();
        });

        let result = mcp(&path)
            .read_substrate_history("docs", "studio", "page:roadmap")
            .expect("history");
        assert!(result.index_present);
        assert_eq!(result.scope_id, "studio");
        assert_eq!(result.entity_id, "page:roadmap");
        assert_eq!(result.revisions.len(), 2);
        assert_eq!(result.revisions[0].revision, 1);
        assert_eq!(result.latest.as_ref().unwrap().revision, 2);
        assert_eq!(result.latest.as_ref().unwrap().operation_id, "op-2");
        assert_eq!(result.checkpoints.len(), 1);
        assert_eq!(result.checkpoints[0].checkpoint_id, "cp-1");

        let latest = mcp(&path)
            .read_substrate_revision_latest("docs", "studio", "page:roadmap")
            .expect("latest revision");
        assert!(latest.index_present);
        assert_eq!(latest.revision.as_ref().unwrap().revision, 2);

        let at_revision = mcp(&path)
            .read_substrate_revision_at("docs", "studio", "page:roadmap", 1)
            .expect("revision at number");
        assert_eq!(at_revision.revision.as_ref().unwrap().operation_id, "op-1");

        let root_2 = Digest::hash(Algo::Blake3, b"root-2").to_string();
        let as_of_root = mcp(&path)
            .read_substrate_revision_as_of_root("docs", "studio", "page:roadmap", &root_2)
            .expect("revision at root");
        assert_eq!(as_of_root.revision.as_ref().unwrap().revision, 2);

        let checkpoint = mcp(&path)
            .read_substrate_checkpoint_before("docs", "studio", 2)
            .expect("checkpoint before revision");
        assert!(checkpoint.index_present);
        assert_eq!(
            checkpoint.checkpoint.as_ref().unwrap().checkpoint_id,
            "cp-1"
        );

        let missing = mcp(&path)
            .read_substrate_history("docs", "missing", "page:roadmap")
            .expect("missing history");
        assert!(!missing.index_present);
        assert!(missing.revisions.is_empty());
        let missing_latest = mcp(&path)
            .read_substrate_revision_latest("docs", "missing", "page:roadmap")
            .expect("missing latest");
        assert!(!missing_latest.index_present);
        assert!(missing_latest.revision.is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn vcs_diff_crosses_vcs_pep() {
        let path = temp_path();
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let ns = loom
            .registry_mut()
            .create(
                FacetKind::Files,
                Some("repo"),
                WorkspaceId::v4_from_bytes([6u8; 16]),
            )
            .unwrap();
        loom.write_file(ns, "readme.txt", b"v1", 0o644).unwrap();
        let c0 = loom.commit(ns, "seed", "c1", 0).unwrap();
        loom.write_file(ns, "readme.txt", b"v2", 0o644).unwrap();
        let c1 = loom.commit(ns, "seed", "c2", 1).unwrap();
        let root = WorkspaceId::v4_from_bytes([8u8; 16]);
        let mut identity = loom_core::IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);
        loom.acl_store_mut()
            .allow(
                loom_core::AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Files),
                [loom_core::AclRight::Read],
            )
            .unwrap();

        let m = LoomMcp::new(StoreAccess::persistent(loom));
        assert_eq!(
            m.read_vcs_diff("repo", &c0.to_string(), &c1.to_string())
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
        m.store()
            .write(|loom| {
                loom.acl_store_mut().allow(
                    loom_core::AclSubject::Principal(root),
                    Some(ns),
                    Some(FacetKind::Vcs),
                    [loom_core::AclRight::Read],
                )
            })
            .unwrap();
        assert!(
            m.read_vcs_diff("repo", &c0.to_string(), &c1.to_string())
                .expect("vcs read")
                .starts_with(&[0x86])
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reads_against_a_missing_loom_error_not_panic() {
        let m = mcp(&temp_path());
        assert!(m.read_workspace_list().is_err());
    }

    #[test]
    fn persistent_access_reads_through_one_handle() {
        let path = temp_path();
        let (digest, ns) = seeded(&path);
        let loom = open_loom_unlocked(&path, None).unwrap();
        let m = LoomMcp::new(StoreAccess::persistent(loom));
        assert!(m.read_cas_has(&ns, &digest).expect("has"));
        let _ = std::fs::remove_file(&path);
    }
}
