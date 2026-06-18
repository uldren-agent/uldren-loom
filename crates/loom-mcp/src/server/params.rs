//! Tool parameter structs deserialized from MCP tool-call arguments.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

fn default_true() -> bool {
    true
}

fn default_ticket_field_cardinality() -> String {
    "optional".to_string()
}

fn default_ticket_comment_type() -> String {
    "general".to_string()
}

fn default_board_mode() -> String {
    "status_mapped".to_string()
}

fn default_board_scope() -> String {
    "project".to_string()
}

fn default_updated_by() -> String {
    "mcp".to_string()
}

#[derive(Default, Deserialize, JsonSchema)]
pub(crate) struct PResultPage {
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) offset: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct PData {
    pub(crate) data: Vec<u8>,
}
#[derive(Default, Deserialize, JsonSchema)]
pub(crate) struct PCapabilities {
    #[serde(default)]
    pub(crate) detailed: bool,
}
#[derive(Default, Deserialize, JsonSchema)]
pub(crate) struct PStoreMaintenancePolicySet {
    #[serde(default)]
    pub(crate) min_candidate_pages: Option<u64>,
    #[serde(default)]
    pub(crate) min_reusable_pages: Option<u64>,
    #[serde(default)]
    pub(crate) interval_ms: Option<u64>,
    #[serde(default)]
    pub(crate) backoff_ms: Option<u64>,
    #[serde(default)]
    pub(crate) max_segments: Option<u64>,
    #[serde(default)]
    pub(crate) max_pages: Option<u64>,
    #[serde(default)]
    pub(crate) full_compaction_enabled: Option<bool>,
    #[serde(default)]
    pub(crate) tail_trim_enabled: Option<bool>,
    #[serde(default)]
    pub(crate) tail_compaction_enabled: Option<bool>,
    #[serde(default)]
    pub(crate) tail_compaction_max_pages: Option<u64>,
    #[serde(default)]
    pub(crate) tail_compaction_max_objects: Option<u64>,
    #[serde(default)]
    pub(crate) tail_compaction_max_bytes: Option<u64>,
    #[serde(default)]
    pub(crate) tail_compaction_interval_ms: Option<u64>,
    #[serde(default)]
    pub(crate) tail_compaction_backoff_ms: Option<u64>,
}
#[derive(Default, Deserialize, JsonSchema)]
pub(crate) struct PStoreMaintenanceRun {
    #[serde(default)]
    pub(crate) manual: bool,
    #[serde(default)]
    pub(crate) max_segments: Option<u64>,
    #[serde(default)]
    pub(crate) max_pages: Option<u64>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PNs {
    pub(crate) workspace: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMetricsPutDescriptor {
    pub(crate) workspace: String,
    pub(crate) descriptor: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMetricsGetDescriptor {
    pub(crate) workspace: String,
    pub(crate) name: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMetricsPutObservation {
    pub(crate) workspace: String,
    pub(crate) descriptor_name: String,
    pub(crate) observation: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMetricsQuery {
    pub(crate) workspace: String,
    pub(crate) descriptor_name: String,
    pub(crate) from_timestamp_ms: u64,
    pub(crate) to_timestamp_ms: u64,
    pub(crate) max_series: u32,
    pub(crate) max_groups: u32,
    pub(crate) max_samples: u32,
    pub(crate) max_output_bytes: u64,
    pub(crate) now_timestamp_ms: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLogsPutRecord {
    pub(crate) workspace: String,
    pub(crate) record: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLogsGetRecord {
    pub(crate) workspace: String,
    pub(crate) record_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLogsQuery {
    pub(crate) workspace: String,
    pub(crate) from_time_unix_nano: u64,
    pub(crate) to_time_unix_nano: u64,
    pub(crate) max_records: u32,
    pub(crate) max_output_bytes: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTracesPutSpan {
    pub(crate) workspace: String,
    pub(crate) span: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTracesGetSpan {
    pub(crate) workspace: String,
    pub(crate) trace_id: String,
    pub(crate) span_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTracesTraceSpans {
    pub(crate) workspace: String,
    pub(crate) trace_id: String,
    pub(crate) max_spans: u32,
    pub(crate) max_output_bytes: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTracesQuery {
    pub(crate) workspace: String,
    pub(crate) from_start_time_ns: u64,
    pub(crate) to_start_time_ns: u64,
    pub(crate) max_spans: u32,
    pub(crate) max_output_bytes: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsProfile {
    pub(crate) workspace: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsList {
    pub(crate) workspace: String,
    pub(crate) limit: Option<usize>,
    pub(crate) offset: Option<usize>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsGet {
    pub(crate) workspace: String,
    pub(crate) meeting_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsSearch {
    pub(crate) workspace: String,
    pub(crate) query: String,
    #[serde(default)]
    pub(crate) field: Option<String>,
    #[serde(default)]
    pub(crate) limit: Option<u32>,
    #[serde(default)]
    pub(crate) offset: Option<u32>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsAnnotation {
    pub(crate) workspace: String,
    pub(crate) annotation_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsVocabularyPropose {
    pub(crate) workspace: String,
    pub(crate) term_id: String,
    pub(crate) kind: String,
    pub(crate) label: String,
    pub(crate) evidence_annotation_ids: Vec<String>,
    #[serde(default)]
    pub(crate) aliases: Option<Vec<String>>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsVocabulary {
    pub(crate) workspace: String,
    pub(crate) term_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsEntityMerge {
    pub(crate) workspace: String,
    pub(crate) merge_id: String,
    pub(crate) canonical_entity_id: String,
    pub(crate) merged_entity_ids: Vec<String>,
    pub(crate) evidence_annotation_ids: Vec<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsPromotion {
    pub(crate) workspace: String,
    pub(crate) promotion_id: String,
    pub(crate) operation_kind: String,
    pub(crate) source_annotation_id: String,
    pub(crate) target_profile: String,
    pub(crate) target_entity_ref: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsPromoteTaskToTicket {
    pub(crate) workspace: String,
    pub(crate) promotion_id: String,
    pub(crate) source_annotation_id: String,
    pub(crate) project_id: String,
    pub(crate) ticket_type: String,
    #[serde(default)]
    pub(crate) policy_labels: Vec<String>,
    #[serde(default)]
    pub(crate) expected_ticket_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsPromoteDecisionToDecisionLog {
    pub(crate) workspace: String,
    pub(crate) promotion_id: String,
    pub(crate) source_annotation_id: String,
    pub(crate) decision_id: String,
    pub(crate) ledger_name: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsPromoteQuestionToLifecycle {
    pub(crate) workspace: String,
    pub(crate) promotion_id: String,
    pub(crate) source_annotation_id: String,
    pub(crate) instance_id: String,
    pub(crate) definition_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsPromoteArtifactToReferenceArtifact {
    pub(crate) workspace: String,
    pub(crate) promotion_id: String,
    pub(crate) source_annotation_id: String,
    pub(crate) artifact_id: String,
    #[serde(default)]
    pub(crate) target_ref: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsPromoteReferenceToReferenceArtifact {
    pub(crate) workspace: String,
    pub(crate) promotion_id: String,
    pub(crate) source_annotation_id: String,
    pub(crate) reference_id: String,
    #[serde(default)]
    pub(crate) target_ref: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMeetingsImportSnapshot {
    pub(crate) workspace: String,
    pub(crate) input_profile: String,
    pub(crate) snapshot: Vec<u8>,
    pub(crate) dry_run: Option<bool>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PRedmineImportSnapshot {
    pub(crate) workspace: String,
    pub(crate) profile: String,
    pub(crate) snapshot: Vec<u8>,
    #[serde(default)]
    pub(crate) source_path: Option<String>,
    #[serde(default)]
    pub(crate) field_policy: Option<String>,
    pub(crate) dry_run: Option<bool>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PStudioReindex {
    pub(crate) workspace: String,
    pub(crate) profile: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PImportSubmitBatch {
    pub(crate) workspace: String,
    pub(crate) batch: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PImportExecuteBatch {
    pub(crate) workspace: String,
    pub(crate) batch: Vec<u8>,
    pub(crate) dry_run: Option<bool>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PWatchSubscribe {
    pub(crate) workspace: String,
    pub(crate) branch: String,
    pub(crate) from: Option<String>,
    pub(crate) facet: Option<String>,
    pub(crate) path_prefix: Option<String>,
    pub(crate) change_kinds: Option<Vec<String>>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PWatchPoll {
    pub(crate) workspace: String,
    pub(crate) cursor: String,
    pub(crate) max: u32,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PApp {
    pub(crate) workspace: String,
    pub(crate) app: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PAppPath {
    pub(crate) workspace: String,
    pub(crate) app: String,
    pub(crate) path: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PAppCreate {
    pub(crate) workspace: String,
    pub(crate) app: String,
    pub(crate) index_html: Vec<u8>,
    pub(crate) meta_md: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PAppWrite {
    pub(crate) workspace: String,
    pub(crate) app: String,
    pub(crate) path: String,
    pub(crate) content: Vec<u8>,
    pub(crate) mode: u32,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PAppCallTool {
    pub(crate) app_uri: String,
    pub(crate) tool: String,
    #[serde(default)]
    pub(crate) arguments: Option<serde_json::Map<String, Value>>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PNsName {
    pub(crate) workspace: String,
    pub(crate) collection: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PNsNameId {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PGraphUpsertNode {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) id: String,
    pub(crate) props: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PGraphRemoveNode {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) id: String,
    pub(crate) cascade: bool,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PGraphUpsertEdge {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) id: String,
    pub(crate) src: String,
    pub(crate) dst: String,
    pub(crate) label: String,
    pub(crate) props: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PGraphReachable {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) start: String,
    pub(crate) max_depth: i64,
    pub(crate) via_label: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PGraphShortestPath {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) via_label: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PGraphQuery {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) query: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PVectorCreate {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) dim: u64,
    pub(crate) metric: i32,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PVectorUpsert {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) id: String,
    pub(crate) vector: Vec<u8>,
    pub(crate) metadata: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PVectorUpsertSource {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) id: String,
    pub(crate) vector: Vec<u8>,
    pub(crate) metadata: Vec<u8>,
    pub(crate) source_text: Vec<u8>,
    pub(crate) model_id: Option<String>,
    pub(crate) weights_digest: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PVectorIds {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) prefix: String,
    pub(crate) has_prefix: bool,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PVectorKey {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) key: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PVectorSearch {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) query: Vec<u8>,
    pub(crate) k: u64,
    pub(crate) filter: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PVectorSearchPolicy {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) query: Vec<u8>,
    pub(crate) k: u64,
    pub(crate) filter: Vec<u8>,
    pub(crate) policy: i32,
    pub(crate) threshold: u64,
    pub(crate) ef: u64,
    pub(crate) pq_m: u64,
    pub(crate) pq_k: u64,
    pub(crate) pq_iters: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PColumnarCreate {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) columns: Vec<u8>,
    pub(crate) target_segment_rows: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PColumnarAppend {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) row: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PColumnarSelect {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) columns: Vec<u8>,
    #[serde(default)]
    pub(crate) filter: Option<Vec<u8>>,
    #[serde(default)]
    pub(crate) predicate: Option<Value>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PColumnarAggregate {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) aggregates: Vec<u8>,
    pub(crate) filter: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDataframeCreate {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) plan: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDataframePreview {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) rows: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFtsCreate {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) mapping: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFtsIndex {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) id: Vec<u8>,
    pub(crate) doc: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFtsId {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) id: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFtsIds {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) prefix: Vec<u8>,
    pub(crate) has_prefix: bool,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFtsQuery {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) request: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFtsStatus {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) engine_version: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PStoreSearch {
    #[serde(default)]
    pub(crate) workspace: Option<String>,
    #[serde(default)]
    pub(crate) collection: Option<String>,
    pub(crate) query: String,
    #[serde(default)]
    pub(crate) field: Option<String>,
    #[serde(default)]
    pub(crate) limit: Option<u32>,
    #[serde(default)]
    pub(crate) offset: Option<u32>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateChanges {
    pub(crate) workspace: String,
    pub(crate) cursor: String,
    pub(crate) max: u32,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PWorkgraphChanges {
    pub(crate) workspace: String,
    pub(crate) workspace_id: String,
    pub(crate) next_sequence: u64,
    pub(crate) max: u32,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PWorkgraphMetrics {
    pub(crate) workspace: String,
    #[serde(default)]
    pub(crate) workspace_id: Option<String>,
    #[serde(default)]
    pub(crate) statuses: Vec<String>,
    #[serde(default)]
    pub(crate) lanes: Vec<String>,
    #[serde(default)]
    pub(crate) limit: Option<u32>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PWorkgraphFactPut {
    pub(crate) workspace: String,
    pub(crate) workspace_id: String,
    pub(crate) fact: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateRefs {
    pub(crate) workspace: String,
    pub(crate) target: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateAliasBind {
    pub(crate) workspace: String,
    pub(crate) scope_id: String,
    pub(crate) alias: String,
    pub(crate) target: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateAliasKey {
    pub(crate) workspace: String,
    pub(crate) scope_id: String,
    pub(crate) alias: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateAliasList {
    pub(crate) workspace: String,
    pub(crate) scope_id: String,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateReferenceStatus {
    pub(crate) workspace: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateReferenceReconcile {
    pub(crate) workspace: String,
    #[serde(default = "default_reference_reconcile_max")]
    pub(crate) max: usize,
}

fn default_reference_reconcile_max() -> usize {
    100
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateHistory {
    pub(crate) workspace: String,
    pub(crate) scope_id: String,
    pub(crate) entity_id: String,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateRevisionAt {
    pub(crate) workspace: String,
    pub(crate) scope_id: String,
    pub(crate) entity_id: String,
    pub(crate) revision: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateRevisionAsOfRoot {
    pub(crate) workspace: String,
    pub(crate) scope_id: String,
    pub(crate) entity_id: String,
    pub(crate) root: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateCheckpointBefore {
    pub(crate) workspace: String,
    pub(crate) scope_id: String,
    pub(crate) revision: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateViewDefine {
    pub(crate) workspace: String,
    pub(crate) view_id: String,
    pub(crate) source_scopes: Vec<String>,
    pub(crate) source_facets: Vec<String>,
    pub(crate) projection_ref: String,
    pub(crate) output_facet: Option<String>,
    pub(crate) media_type: String,
    pub(crate) freshness_policy: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateViewGet {
    pub(crate) workspace: String,
    pub(crate) view_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateViewList {
    pub(crate) workspace: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PWriteAdmissionTarget {
    pub(crate) target_kind: String,
    pub(crate) target_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateWriteAdmissionPolicyKey {
    pub(crate) workspace: String,
    pub(crate) surface: String,
    pub(crate) scope_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateWriteAdmissionPolicySet {
    pub(crate) workspace: String,
    pub(crate) surface: String,
    pub(crate) scope_id: String,
    pub(crate) default_mode: String,
    #[serde(default)]
    pub(crate) mandatory_targets: Vec<PWriteAdmissionTarget>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsProjectCreate {
    pub(crate) workspace: String,
    pub(crate) project_id: String,
    pub(crate) key_prefix: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsProjectRekey {
    pub(crate) workspace: String,
    pub(crate) project_id: String,
    pub(crate) key_prefix: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsProjectSettingsGet {
    pub(crate) workspace: String,
    pub(crate) project_id: String,
    #[serde(default)]
    pub(crate) include_contracts: bool,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsProjectSettingsSet {
    pub(crate) workspace: String,
    pub(crate) project_id: String,
    #[serde(default)]
    pub(crate) default_projection: Option<String>,
    #[serde(default)]
    pub(crate) actor_enforcement: Option<String>,
    #[serde(default)]
    pub(crate) project_owner_principal: Option<String>,
    #[serde(default)]
    pub(crate) clear_project_owner_principal: bool,
    #[serde(default)]
    pub(crate) acceptance_authorities: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) acceptance_evidence_enforcement: Option<bool>,
    #[serde(default)]
    pub(crate) required_acceptance_evidence_keys: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) owner_contract_summary: Option<String>,
    #[serde(default)]
    pub(crate) owner_contract_details: Option<String>,
    #[serde(default)]
    pub(crate) worker_contract_summary: Option<String>,
    #[serde(default)]
    pub(crate) worker_contract_details: Option<String>,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsFields {
    pub(crate) workspace: String,
    #[serde(default)]
    pub(crate) project_id: Option<String>,
    #[serde(default)]
    pub(crate) projection: Option<String>,
    #[serde(default)]
    pub(crate) operation: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsProjects {
    pub(crate) workspace: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsRelations {
    pub(crate) workspace: String,
    pub(crate) ticket_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsFieldPut {
    pub(crate) workspace: String,
    pub(crate) project_id: String,
    pub(crate) field_id: String,
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) field_type: String,
    #[serde(default)]
    pub(crate) option_set: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) max_length: Option<u32>,
    #[serde(default)]
    pub(crate) required: bool,
    #[serde(default = "default_true")]
    pub(crate) searchable: bool,
    #[serde(default)]
    pub(crate) orderable: bool,
    #[serde(default = "default_ticket_field_cardinality")]
    pub(crate) cardinality: String,
    #[serde(default)]
    pub(crate) applicable_type_ids: Vec<String>,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsFieldRetire {
    pub(crate) workspace: String,
    pub(crate) project_id: String,
    pub(crate) field_id: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsCreate {
    pub(crate) workspace: String,
    pub(crate) project_id: String,
    pub(crate) ticket_type: String,
    #[serde(default)]
    pub(crate) projection: Option<String>,
    #[serde(default)]
    pub(crate) external_source: Option<String>,
    #[serde(default)]
    pub(crate) external_id: Option<String>,
    pub(crate) fields: BTreeMap<String, Value>,
    #[serde(default)]
    pub(crate) policy_labels: Vec<String>,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsUpdate {
    pub(crate) workspace: String,
    pub(crate) ticket_id: String,
    #[serde(default)]
    pub(crate) projection: Option<String>,
    #[serde(default)]
    pub(crate) set_fields: Option<BTreeMap<String, Value>>,
    #[serde(default)]
    pub(crate) delete_fields: Vec<String>,
    #[serde(default)]
    pub(crate) action: Option<String>,
    #[serde(default)]
    pub(crate) target_status: Option<String>,
    #[serde(default)]
    pub(crate) observed_source_status: Option<String>,
    #[serde(default)]
    pub(crate) observed_workflow_version: Option<String>,
    #[serde(default)]
    pub(crate) assignee: Option<String>,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
    #[serde(default)]
    pub(crate) comment: Option<PTicketsUpdateComment>,
    #[serde(default)]
    pub(crate) comments: Vec<PTicketsUpdateComment>,
    #[serde(default)]
    pub(crate) relation_sets: Vec<PTicketsUpdateRelationSet>,
    #[serde(default)]
    pub(crate) relation_removes: Vec<PTicketsUpdateRelationRemove>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsUpdateComment {
    #[serde(default)]
    pub(crate) comment_id: Option<String>,
    #[serde(default)]
    pub(crate) comment_type: Option<String>,
    pub(crate) body: String,
    #[serde(default)]
    pub(crate) evidence: Option<BTreeMap<String, Vec<String>>>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsUpdateRelationSet {
    #[serde(default)]
    pub(crate) relation_id: Option<String>,
    pub(crate) kind: String,
    pub(crate) target_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsUpdateRelationRemove {
    pub(crate) relation_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsDelete {
    pub(crate) workspace: String,
    pub(crate) ticket_id: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsComments {
    pub(crate) workspace: String,
    pub(crate) ticket_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsCommentAdd {
    pub(crate) workspace: String,
    pub(crate) ticket_id: String,
    pub(crate) body: String,
    #[serde(default)]
    pub(crate) comment_id: Option<String>,
    #[serde(default = "default_ticket_comment_type")]
    pub(crate) comment_type: String,
    #[serde(default)]
    pub(crate) evidence: Option<BTreeMap<String, Vec<String>>>,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsCommentUpdate {
    pub(crate) workspace: String,
    pub(crate) ticket_id: String,
    pub(crate) comment_id: String,
    #[serde(default)]
    pub(crate) body: Option<String>,
    #[serde(default)]
    pub(crate) comment_type: Option<String>,
    #[serde(default)]
    pub(crate) evidence: Option<Option<BTreeMap<String, Vec<String>>>>,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsCommentDelete {
    pub(crate) workspace: String,
    pub(crate) ticket_id: String,
    pub(crate) comment_id: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PBoardColumn {
    pub(crate) column_id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) mapped_statuses: Vec<String>,
    #[serde(default)]
    pub(crate) wip_limit: Option<u32>,
    #[serde(default)]
    pub(crate) hidden: bool,
    #[serde(default)]
    pub(crate) rank: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PBoardSwimlane {
    pub(crate) swimlane_id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) predicate: Option<String>,
    #[serde(default)]
    pub(crate) rank: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsBoardCreate {
    pub(crate) workspace: String,
    pub(crate) board_id: String,
    pub(crate) board_key: String,
    pub(crate) name: String,
    pub(crate) project_id: String,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default = "default_board_mode")]
    pub(crate) mode: String,
    #[serde(default = "default_board_scope")]
    pub(crate) scope: String,
    #[serde(default)]
    pub(crate) columns: Vec<PBoardColumn>,
    #[serde(default)]
    pub(crate) swimlanes: Vec<PBoardSwimlane>,
    #[serde(default)]
    pub(crate) card_display_fields: Vec<String>,
    #[serde(default)]
    pub(crate) owner_principal: Option<String>,
    #[serde(default)]
    pub(crate) coordinator_principal: Option<String>,
    #[serde(default = "default_updated_by")]
    pub(crate) updated_by: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsBoardGet {
    pub(crate) workspace: String,
    pub(crate) board_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsBoardList {
    pub(crate) workspace: String,
    #[serde(default)]
    pub(crate) include_deleted: bool,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsBoardUpdate {
    pub(crate) workspace: String,
    pub(crate) board_id: String,
    #[serde(default)]
    pub(crate) board_key: Option<String>,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) board_status: Option<String>,
    #[serde(default)]
    pub(crate) card_display_fields: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) updated_by: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsBoardConfigureColumns {
    pub(crate) workspace: String,
    pub(crate) board_id: String,
    #[serde(default)]
    pub(crate) mode: Option<String>,
    #[serde(default)]
    pub(crate) columns: Vec<PBoardColumn>,
    #[serde(default)]
    pub(crate) swimlanes: Vec<PBoardSwimlane>,
    #[serde(default = "default_updated_by")]
    pub(crate) updated_by: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsBoardMoveCard {
    pub(crate) workspace: String,
    pub(crate) board_id: String,
    pub(crate) ticket_id: String,
    pub(crate) column_id: String,
    pub(crate) rank_token: String,
    #[serde(default)]
    pub(crate) swimlane_id: Option<String>,
    #[serde(default = "default_updated_by")]
    pub(crate) updated_by: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsRelationSet {
    pub(crate) workspace: String,
    pub(crate) ticket_id: String,
    pub(crate) kind: String,
    pub(crate) target_id: String,
    #[serde(default)]
    pub(crate) relation_id: Option<String>,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsRelationRemove {
    pub(crate) workspace: String,
    pub(crate) ticket_id: String,
    pub(crate) relation_id: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsGet {
    pub(crate) workspace: String,
    pub(crate) ticket_id: String,
    #[serde(default)]
    pub(crate) projection: Option<String>,
    #[serde(default)]
    pub(crate) detailed: bool,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsList {
    pub(crate) workspace: String,
    #[serde(default)]
    pub(crate) projection: Option<String>,
    /// Filter by status.
    #[serde(default)]
    pub(crate) statuses: Vec<String>,
    /// Filter by assignee.
    #[serde(default)]
    pub(crate) assignees: Vec<String>,
    /// Filter by priority.
    #[serde(default)]
    pub(crate) priorities: Vec<String>,
    /// Filter by ticket type.
    #[serde(default)]
    pub(crate) ticket_types: Vec<String>,
    /// Filter by ordinary label.
    #[serde(default)]
    pub(crate) labels: Vec<String>,
    /// Filter by policy label.
    #[serde(default)]
    pub(crate) policy_labels: Vec<String>,
    /// Restrict to members of this first-class Lane.
    #[serde(default)]
    pub(crate) lane: Option<String>,
    /// Restrict to cards on this first-class Board.
    #[serde(default)]
    pub(crate) board: Option<String>,
    /// Only dependency-ready, actionable tickets.
    #[serde(default)]
    pub(crate) ready: bool,
    /// Include terminal tickets. Lane- and Board-scoped lists hide them by default.
    #[serde(default)]
    pub(crate) include_completed: bool,
    /// Maximum tickets to return (default 25, hard cap 100).
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    /// Opaque continuation cursor from a previous page.
    #[serde(default)]
    pub(crate) cursor: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTicketsHistory {
    pub(crate) workspace: String,
    #[serde(default)]
    pub(crate) ticket_id: Option<String>,
    #[serde(default)]
    pub(crate) detailed: bool,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLanesCreate {
    pub(crate) workspace: String,
    pub(crate) lane_id: String,
    pub(crate) lane_key: String,
    #[serde(default)]
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) description: String,
    pub(crate) lane_kind: String,
    #[serde(default)]
    pub(crate) owner_principal: Option<String>,
    pub(crate) lane_status: String,
    #[serde(default)]
    pub(crate) ticket_ids: Vec<String>,
    #[serde(default)]
    pub(crate) active_ticket_id: Option<String>,
    #[serde(default)]
    pub(crate) status_report: String,
    #[serde(default)]
    pub(crate) reviewer_feedback: String,
    #[serde(default)]
    pub(crate) updated_by: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLanesGet {
    pub(crate) workspace: String,
    pub(crate) lane_id: String,
    #[serde(default)]
    pub(crate) detailed: bool,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLanesList {
    pub(crate) workspace: String,
    #[serde(default)]
    pub(crate) detailed: bool,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLanesUpdate {
    pub(crate) workspace: String,
    pub(crate) lane_id: String,
    /// Omit to leave the title unchanged; pass "" to clear it.
    #[serde(default)]
    pub(crate) title: Option<String>,
    /// Omit to leave the description unchanged; pass "" to clear it.
    #[serde(default)]
    pub(crate) description: Option<String>,
    /// Omit to leave the stored Lane status unchanged.
    #[serde(default)]
    pub(crate) lane_status: Option<String>,
    /// Omit to leave the status report unchanged; pass "" to clear it.
    #[serde(default)]
    pub(crate) status_report: Option<String>,
    /// Omit to leave reviewer feedback unchanged; pass "" to clear it.
    #[serde(default)]
    pub(crate) reviewer_feedback: Option<String>,
    #[serde(default)]
    pub(crate) updated_by: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLanesTicketAdd {
    pub(crate) workspace: String,
    pub(crate) lane_id: String,
    pub(crate) ticket_id: String,
    /// Placement verb: "append" (default), "first", "before", or "after".
    #[serde(default)]
    pub(crate) placement: Option<String>,
    /// Anchor ticket id required by the "before"/"after" placements.
    #[serde(default)]
    pub(crate) anchor: Option<String>,
    #[serde(default)]
    pub(crate) updated_by: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLanesTicketRemove {
    pub(crate) workspace: String,
    pub(crate) lane_id: String,
    pub(crate) ticket_id: String,
    #[serde(default)]
    pub(crate) updated_by: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLanesTicketTransfer {
    pub(crate) workspace: String,
    pub(crate) source_lane_id: String,
    pub(crate) target_lane_id: String,
    pub(crate) ticket_id: String,
    pub(crate) updated_by: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLanesDelete {
    pub(crate) workspace: String,
    pub(crate) lane_id: String,
    pub(crate) updated_by: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSpacesCreate {
    pub(crate) workspace: String,
    pub(crate) space_id: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSpacesGet {
    pub(crate) workspace: String,
    pub(crate) space_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSpacesList {
    pub(crate) workspace: String,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PPagesCreate {
    pub(crate) workspace: String,
    pub(crate) page_id: String,
    pub(crate) space_id: String,
    #[serde(default)]
    pub(crate) parent_page_id: Option<String>,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PPagesUpdate {
    pub(crate) workspace: String,
    pub(crate) page_id: String,
    pub(crate) body_text: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PPagesGet {
    pub(crate) workspace: String,
    pub(crate) page_id: String,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PPagesPublish {
    pub(crate) workspace: String,
    pub(crate) page_id: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLifecyclesDefine {
    pub(crate) workspace: String,
    pub(crate) definition_cbor: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLifecyclesDefineStandard {
    pub(crate) workspace: String,
    pub(crate) kind: String,
    pub(crate) version: String,
    pub(crate) completion_predicate_digest: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLifecyclesDefinition {
    pub(crate) workspace: String,
    pub(crate) definition_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLifecyclesWorkspace {
    pub(crate) workspace: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLifecyclesInstantiate {
    pub(crate) workspace: String,
    pub(crate) instance_id: String,
    pub(crate) definition_id: String,
    #[serde(default)]
    pub(crate) subject_refs: Vec<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLifecyclesInstance {
    pub(crate) workspace: String,
    pub(crate) instance_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLifecyclesActiveSet {
    pub(crate) workspace: String,
    pub(crate) instance_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLifecyclesSnapshotPlan {
    pub(crate) workspace: String,
    pub(crate) instance_id: String,
    pub(crate) to_stage_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLifecyclesGateEvaluation {
    pub(crate) gate_id: String,
    pub(crate) passed: bool,
    #[serde(default)]
    pub(crate) principal_id: Option<String>,
    #[serde(default)]
    pub(crate) evidence_digest: Option<String>,
    pub(crate) evaluated_at_ms: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLifecyclesTransition {
    pub(crate) workspace: String,
    pub(crate) instance_id: String,
    pub(crate) transition_id: String,
    pub(crate) to_stage_id: String,
    pub(crate) actor_principal_id: String,
    #[serde(default)]
    pub(crate) gate_evaluations: Vec<PLifecyclesGateEvaluation>,
    #[serde(default)]
    pub(crate) snapshot_digest: Option<String>,
    pub(crate) recorded_at_ms: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLifecyclesSnapshot {
    pub(crate) workspace: String,
    pub(crate) snapshot_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatWorkspace {
    pub(crate) workspace: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatChannel {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatCreateChannel {
    pub(crate) workspace: String,
    pub(crate) handle: String,
    pub(crate) name: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatRenameChannel {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) handle: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatFetchEvents {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) from_sequence: u64,
    pub(crate) max: u32,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatPostMessage {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) message_id: String,
    #[serde(default)]
    pub(crate) thread_id: Option<String>,
    pub(crate) body_text: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatEditMessage {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) message_id: String,
    pub(crate) body_text: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatRedactMessage {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) message_id: String,
    #[serde(default)]
    pub(crate) reason: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatCreateThread {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) thread_id: String,
    pub(crate) parent_message_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatCreateTask {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) task_id: String,
    #[serde(default)]
    pub(crate) message_id: Option<String>,
    pub(crate) title: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatClaimTask {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) task_id: String,
    pub(crate) claim_id: String,
    #[serde(default)]
    pub(crate) lease_token: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatCompleteTask {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) task_id: String,
    pub(crate) claim_id: String,
    #[serde(default)]
    pub(crate) result_message_id: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatInvokeAgent {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) invocation_id: String,
    pub(crate) agent_principal: String,
    #[serde(default)]
    pub(crate) source_message_ids: Vec<String>,
    pub(crate) prompt_text: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatAgentReply {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) invocation_id: String,
    pub(crate) message_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatRequestHandoff {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) handoff_id: String,
    pub(crate) from_agent_principal: String,
    #[serde(default)]
    pub(crate) to_principal: Option<String>,
    #[serde(default)]
    pub(crate) reason: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatReaction {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) message_id: String,
    pub(crate) kind: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatEmoji {
    pub(crate) workspace: String,
    pub(crate) kind: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatUpdateCursor {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) next_sequence: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PChatSetPresence {
    pub(crate) workspace: String,
    pub(crate) channel_id: String,
    pub(crate) status: String,
    pub(crate) ttl_ms: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveFolder {
    pub(crate) workspace: String,
    pub(crate) folder_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveStat {
    pub(crate) workspace: String,
    pub(crate) folder_id: String,
    pub(crate) name: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveFile {
    pub(crate) workspace: String,
    pub(crate) file_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveConflicts {
    pub(crate) workspace: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFence {
    pub(crate) authority: u32,
    pub(crate) epoch: u32,
    pub(crate) sequence: u64,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct PWriteAdmission {
    pub(crate) target_kind: String,
    pub(crate) target_id: String,
    pub(crate) fence: PFence,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveCreateFolder {
    pub(crate) workspace: String,
    pub(crate) parent_folder_id: String,
    pub(crate) folder_id: String,
    pub(crate) name: String,
    pub(crate) expected_root: String,
    #[serde(default)]
    pub(crate) write_admission: Option<PWriteAdmission>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveCreateUpload {
    pub(crate) workspace: String,
    pub(crate) upload_id: String,
    pub(crate) parent_folder_id: String,
    pub(crate) name: String,
    pub(crate) file_id: String,
    pub(crate) expected_root: String,
    pub(crate) replace_file: bool,
    #[serde(default)]
    pub(crate) write_admission: Option<PWriteAdmission>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveUploadChunk {
    pub(crate) workspace: String,
    pub(crate) upload_id: String,
    pub(crate) bytes: Vec<u8>,
    #[serde(default)]
    pub(crate) write_admission: Option<PWriteAdmission>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveCommitUpload {
    pub(crate) workspace: String,
    pub(crate) upload_id: String,
    #[serde(default)]
    pub(crate) write_admission: Option<PWriteAdmission>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveRename {
    pub(crate) workspace: String,
    pub(crate) folder_id: String,
    pub(crate) node_id: String,
    pub(crate) new_name: String,
    pub(crate) expected_root: String,
    #[serde(default)]
    pub(crate) write_admission: Option<PWriteAdmission>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveMove {
    pub(crate) workspace: String,
    pub(crate) source_folder_id: String,
    pub(crate) target_folder_id: String,
    pub(crate) node_id: String,
    pub(crate) expected_root: String,
    #[serde(default)]
    pub(crate) write_admission: Option<PWriteAdmission>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveDelete {
    pub(crate) workspace: String,
    pub(crate) folder_id: String,
    pub(crate) node_id: String,
    pub(crate) expected_root: String,
    #[serde(default)]
    pub(crate) write_admission: Option<PWriteAdmission>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveResolveConflict {
    pub(crate) workspace: String,
    pub(crate) conflict_id: String,
    pub(crate) resolution: String,
    #[serde(default)]
    pub(crate) write_admission: Option<PWriteAdmission>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveAcquireLease {
    pub(crate) workspace: String,
    pub(crate) target_kind: String,
    pub(crate) target_id: String,
    pub(crate) lease_ms: u64,
    pub(crate) wait_ms: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveRefreshLease {
    pub(crate) workspace: String,
    pub(crate) target_kind: String,
    pub(crate) target_id: String,
    pub(crate) fence: PFence,
    pub(crate) lease_ms: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveReleaseLease {
    pub(crate) workspace: String,
    pub(crate) target_kind: String,
    pub(crate) target_id: String,
    pub(crate) fence: PFence,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveBreakLease {
    pub(crate) workspace: String,
    pub(crate) target_kind: String,
    pub(crate) target_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveShareGrant {
    pub(crate) workspace: String,
    pub(crate) grant_id: String,
    pub(crate) target_kind: String,
    pub(crate) target_id: String,
    pub(crate) principal: String,
    pub(crate) role: String,
    #[serde(default)]
    pub(crate) expires_at_ms: Option<u64>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveShareRevoke {
    pub(crate) workspace: String,
    pub(crate) grant_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveRetentionPin {
    pub(crate) workspace: String,
    pub(crate) pin_id: String,
    pub(crate) kind: String,
    pub(crate) root: String,
    #[serde(default)]
    pub(crate) target_entity_id: Option<String>,
    #[serde(default)]
    pub(crate) expires_at_ms: Option<u64>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveRetentionUnpin {
    pub(crate) workspace: String,
    pub(crate) pin_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveRetentionApply {
    pub(crate) workspace: String,
    pub(crate) now_ms: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDriveShareExpiryApply {
    pub(crate) workspace: String,
    pub(crate) now_ms: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PStructuresCreate {
    pub(crate) workspace: String,
    pub(crate) structure_id: String,
    pub(crate) space_id: String,
    pub(crate) kind: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PStructuresGet {
    pub(crate) workspace: String,
    pub(crate) structure_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PStructuresAddNode {
    pub(crate) workspace: String,
    pub(crate) structure_id: String,
    pub(crate) node_id: String,
    pub(crate) kind: String,
    pub(crate) label: String,
    #[serde(default)]
    pub(crate) body_digest: Option<String>,
    #[serde(default)]
    pub(crate) entity_ref: Option<String>,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PStructuresBind {
    pub(crate) workspace: String,
    pub(crate) structure_id: String,
    pub(crate) node_id: String,
    #[serde(default)]
    pub(crate) entity_ref: Option<String>,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PStructuresMoveNode {
    pub(crate) workspace: String,
    pub(crate) structure_id: String,
    pub(crate) node_id: String,
    #[serde(default)]
    pub(crate) parent_node_id: Option<String>,
    #[serde(default)]
    pub(crate) label: Option<String>,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PStructuresLinkNode {
    pub(crate) workspace: String,
    pub(crate) structure_id: String,
    pub(crate) edge_id: String,
    pub(crate) src_node_id: String,
    pub(crate) dst_node_id: String,
    pub(crate) label: String,
    #[serde(default)]
    pub(crate) target_ref: Option<String>,
    #[serde(default)]
    pub(crate) expected_root: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PStructuresDecomposeToTickets {
    pub(crate) workspace: String,
    pub(crate) structure_id: String,
    pub(crate) items: Vec<PStructuresDecomposeItem>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PStructuresDecomposeItem {
    pub(crate) node_id: String,
    pub(crate) project_id: String,
    #[serde(default)]
    pub(crate) ticket_type: Option<String>,
    #[serde(default)]
    pub(crate) fields: Option<Value>,
    #[serde(default)]
    pub(crate) policy_labels: Vec<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSubstrateTransact {
    pub(crate) ops: Vec<PSubstrateTransactOp>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "kind")]
pub(crate) enum PSubstrateTransactOp {
    #[serde(rename = "cas.put")]
    CasPut {
        workspace: Option<String>,
        content: Vec<u8>,
    },
    #[serde(rename = "cas.delete")]
    CasDelete {
        workspace: Option<String>,
        digest: String,
    },
    #[serde(rename = "document.put")]
    DocumentPut {
        workspace: Option<String>,
        collection: Option<String>,
        id: String,
        doc: Vec<u8>,
    },
    #[serde(rename = "document.delete")]
    DocumentDelete {
        workspace: Option<String>,
        collection: Option<String>,
        id: String,
    },
    #[serde(rename = "document.replace_text")]
    DocumentReplaceText {
        workspace: Option<String>,
        collection: Option<String>,
        id: String,
        base_digest: String,
        find: String,
        replace: String,
        #[serde(default)]
        replace_all: bool,
    },
    #[serde(rename = "graph.upsert_node")]
    GraphUpsertNode {
        workspace: Option<String>,
        collection: Option<String>,
        id: String,
        props: Vec<u8>,
    },
    #[serde(rename = "graph.remove_node")]
    GraphRemoveNode {
        workspace: Option<String>,
        collection: Option<String>,
        id: String,
        cascade: bool,
    },
    #[serde(rename = "graph.upsert_edge")]
    GraphUpsertEdge {
        workspace: Option<String>,
        collection: Option<String>,
        id: String,
        src: String,
        dst: String,
        label: String,
        props: Vec<u8>,
    },
    #[serde(rename = "graph.remove_edge")]
    GraphRemoveEdge {
        workspace: Option<String>,
        collection: Option<String>,
        id: String,
    },
    #[serde(rename = "substrate.view_define")]
    SubstrateViewDefine {
        workspace: Option<String>,
        view_id: String,
        source_scopes: Vec<String>,
        source_facets: Vec<String>,
        projection_ref: String,
        output_facet: Option<String>,
        media_type: String,
        freshness_policy: String,
    },
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCasDigest {
    pub(crate) workspace: String,
    pub(crate) digest: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PKvKey {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) key: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PKvPut {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) key: Vec<u8>,
    pub(crate) value: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PKvRange {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) lo: Vec<u8>,
    pub(crate) hi: Vec<u8>,
    #[serde(default)]
    pub(crate) predicate: Option<Value>,
    #[serde(default)]
    pub(crate) max_output_bytes: Option<usize>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PAskOption {
    pub(crate) label: String,
    pub(crate) description: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PAskQuestion {
    pub(crate) question: String,
    pub(crate) context: Option<String>,
    pub(crate) examples: Option<String>,
    pub(crate) options: Option<Vec<PAskOption>>,
    pub(crate) recommendation: Option<String>,
    pub(crate) shape: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PAskBegin {
    pub(crate) workspace: String,
    pub(crate) questions: Vec<PAskQuestion>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PAskWait {
    pub(crate) workspace: String,
    pub(crate) id: String,
    pub(crate) timeout_ms: Option<u64>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PAskAnswer {
    pub(crate) index: u32,
    pub(crate) status: String,
    pub(crate) selected: Option<Vec<String>>,
    pub(crate) text: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PAskSubmit {
    pub(crate) workspace: String,
    pub(crate) id: String,
    pub(crate) answers: Vec<PAskAnswer>,
    pub(crate) aborted: Option<bool>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDocId {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDocQuery {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    #[serde(default)]
    pub(crate) id_prefix: Option<String>,
    #[serde(default)]
    pub(crate) predicate: Option<Value>,
    #[serde(default)]
    pub(crate) projections: Vec<PDocProjection>,
    #[serde(default)]
    pub(crate) index: Option<String>,
    #[serde(default)]
    pub(crate) value: Option<Value>,
    #[serde(default)]
    pub(crate) cursor: Option<String>,
    #[serde(default)]
    pub(crate) limit: Option<u64>,
    #[serde(default)]
    pub(crate) include_document: bool,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDocProjection {
    pub(crate) name: String,
    pub(crate) path: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDocReplaceText {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) id: String,
    pub(crate) base_digest: String,
    pub(crate) find: String,
    pub(crate) replace: String,
    #[serde(default)]
    pub(crate) replace_all: bool,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDocPutText {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) id: String,
    pub(crate) text: String,
    #[serde(default)]
    pub(crate) expected_entity_tag: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PDocPutBinary {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) id: String,
    pub(crate) bytes: Vec<u8>,
    #[serde(default)]
    pub(crate) expected_entity_tag: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTsGet {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) ts: i64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTsRange {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) from: i64,
    pub(crate) to: i64,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
    #[serde(default)]
    pub(crate) max_output_bytes: Option<usize>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTsPut {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) ts: i64,
    pub(crate) value: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLedgerSeq {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) seq: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PLedgerAppend {
    pub(crate) workspace: String,
    pub(crate) collection: String,
    pub(crate) payload: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PQueueGet {
    pub(crate) workspace: String,
    pub(crate) stream: String,
    pub(crate) seq: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PQueueRange {
    pub(crate) workspace: String,
    pub(crate) stream: String,
    pub(crate) lo: u64,
    pub(crate) hi: u64,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PQueueStream {
    pub(crate) workspace: String,
    pub(crate) stream: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PQueueAppend {
    pub(crate) workspace: String,
    pub(crate) stream: String,
    pub(crate) entry: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PConsumer {
    pub(crate) workspace: String,
    pub(crate) stream: String,
    pub(crate) consumer_id: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PConsumerRead {
    pub(crate) workspace: String,
    pub(crate) stream: String,
    pub(crate) consumer_id: String,
    pub(crate) max: u32,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PConsumerSeq {
    pub(crate) workspace: String,
    pub(crate) stream: String,
    pub(crate) consumer_id: String,
    pub(crate) next_seq: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFsPath {
    pub(crate) workspace: String,
    pub(crate) path: String,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFsDirectory {
    pub(crate) workspace: String,
    pub(crate) path: String,
    pub(crate) recursive: bool,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFsReadAt {
    pub(crate) workspace: String,
    pub(crate) path: String,
    pub(crate) offset: u64,
    pub(crate) len: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFsWrite {
    pub(crate) workspace: String,
    pub(crate) path: String,
    pub(crate) content: Vec<u8>,
    pub(crate) mode: u32,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFsAppend {
    pub(crate) workspace: String,
    pub(crate) path: String,
    pub(crate) content: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFsWriteAt {
    pub(crate) workspace: String,
    pub(crate) path: String,
    pub(crate) offset: u64,
    pub(crate) data: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFsTruncate {
    pub(crate) workspace: String,
    pub(crate) path: String,
    pub(crate) size: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PFsSymlink {
    pub(crate) workspace: String,
    pub(crate) target: String,
    pub(crate) link_path: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCasPut {
    pub(crate) workspace: String,
    pub(crate) content: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PBranch {
    pub(crate) workspace: String,
    pub(crate) branch: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PVcsName {
    pub(crate) workspace: String,
    pub(crate) name: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PVcsDiff {
    pub(crate) workspace: String,
    pub(crate) from_commit: String,
    pub(crate) to_commit: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCommit {
    pub(crate) workspace: String,
    pub(crate) author: String,
    pub(crate) message: String,
    pub(crate) timestamp_ms: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PNsPath {
    pub(crate) workspace: String,
    pub(crate) path: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTagCreate {
    pub(crate) workspace: String,
    pub(crate) name: String,
    pub(crate) rev: String,
    pub(crate) tagger: String,
    pub(crate) message: String,
    pub(crate) timestamp_ms: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PTagRename {
    pub(crate) workspace: String,
    pub(crate) old_name: String,
    pub(crate) new_name: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PRestoreFile {
    pub(crate) workspace: String,
    pub(crate) rev: String,
    pub(crate) path: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PRestorePath {
    pub(crate) workspace: String,
    pub(crate) rev: String,
    pub(crate) prefix: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMerge {
    pub(crate) workspace: String,
    pub(crate) from_branch: String,
    pub(crate) author: String,
    pub(crate) timestamp_ms: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMergeResolve {
    pub(crate) workspace: String,
    pub(crate) path: String,
    pub(crate) resolution: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCherryPick {
    pub(crate) workspace: String,
    pub(crate) commits: Vec<String>,
    pub(crate) timestamp_ms: u64,
    pub(crate) dry_run: bool,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PRevert {
    pub(crate) workspace: String,
    pub(crate) commits: Vec<String>,
    pub(crate) author: String,
    pub(crate) timestamp_ms: u64,
    pub(crate) dry_run: bool,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PRebase {
    pub(crate) workspace: String,
    pub(crate) onto: String,
    pub(crate) timestamp_ms: u64,
    pub(crate) dry_run: bool,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSquash {
    pub(crate) workspace: String,
    pub(crate) onto: String,
    pub(crate) author: String,
    pub(crate) message: String,
    pub(crate) timestamp_ms: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSqlTable {
    pub(crate) workspace: String,
    pub(crate) db: String,
    pub(crate) table: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSqlTableAt {
    pub(crate) workspace: String,
    pub(crate) db: String,
    pub(crate) table: String,
    pub(crate) commit: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSqlIndexScan {
    pub(crate) workspace: String,
    pub(crate) db: String,
    pub(crate) table: String,
    pub(crate) index: String,
    pub(crate) prefix: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSqlIndexScanAt {
    pub(crate) workspace: String,
    pub(crate) db: String,
    pub(crate) table: String,
    pub(crate) index: String,
    pub(crate) prefix: Vec<u8>,
    pub(crate) commit: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSqlBlame {
    pub(crate) workspace: String,
    pub(crate) db: String,
    pub(crate) branch: String,
    pub(crate) table: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSqlDiff {
    pub(crate) workspace: String,
    pub(crate) db: String,
    pub(crate) table: String,
    pub(crate) from_commit: String,
    pub(crate) to_commit: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSqlExec {
    pub(crate) workspace: String,
    pub(crate) db: String,
    pub(crate) sql: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PSqlCommit {
    pub(crate) workspace: String,
    pub(crate) author: String,
    pub(crate) message: String,
    pub(crate) timestamp_ms: u64,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCalColl {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) collection: String,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCalPrincipal {
    pub(crate) workspace: String,
    pub(crate) principal: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCalEntry {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) collection: String,
    pub(crate) uid: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCalRange {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) collection: String,
    pub(crate) from: String,
    pub(crate) to: String,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCalSearch {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) collection: String,
    pub(crate) component: String,
    pub(crate) text: String,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCalCreateColl {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) collection: String,
    pub(crate) display_name: String,
    pub(crate) components: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCalPutEntry {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) collection: String,
    pub(crate) entry: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCalPutIcs {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) collection: String,
    pub(crate) ics: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCardBook {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) book: String,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCardPrincipal {
    pub(crate) workspace: String,
    pub(crate) principal: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCardEntry {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) book: String,
    pub(crate) uid: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCardSearch {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) book: String,
    pub(crate) text: String,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCardCreateBook {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) book: String,
    pub(crate) display_name: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCardPutEntry {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) book: String,
    pub(crate) entry: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PCardPutVcard {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) book: String,
    pub(crate) vcard: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMailBox {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) mailbox: String,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMailPrincipal {
    pub(crate) workspace: String,
    pub(crate) principal: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMailMsg {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) mailbox: String,
    pub(crate) uid: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMailSearch {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) mailbox: String,
    pub(crate) text: String,
    #[serde(flatten)]
    pub(crate) page: PResultPage,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMailCreateBox {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) mailbox: String,
    pub(crate) display_name: String,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMailIngest {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) mailbox: String,
    pub(crate) uid: String,
    pub(crate) raw: Vec<u8>,
}
#[derive(Deserialize, JsonSchema)]
pub(crate) struct PMailSetFlags {
    pub(crate) workspace: String,
    pub(crate) principal: String,
    pub(crate) mailbox: String,
    pub(crate) uid: String,
    pub(crate) flags: Vec<String>,
}
