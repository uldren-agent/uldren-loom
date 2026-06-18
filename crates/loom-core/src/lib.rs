//! `loom-core` - the Uldren Loom engine.
//!
//! Implements the content-addressed object model and the low-level provider contract that the
//! command-line tool, the C ABI, and every language binding build upon.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

pub mod acl;
pub mod calendar;
pub mod capability;
pub mod cas;
pub(crate) mod cbor;
pub mod change_set;
pub mod chunk;
pub mod columnar;
#[cfg(feature = "columnar-arrow")]
pub mod columnar_arrow;
pub mod contacts;
pub mod dataframe;
pub mod delivery;
pub mod digest;
pub mod document;
pub mod error;
pub mod fs;
pub mod graph;
pub mod hooks;
pub mod identity;
pub mod inference;
pub mod keys;
pub mod kv;
pub mod ledger;
pub mod lock;
pub mod log;
pub mod logs;
pub mod mail;
pub mod metrics;
pub mod object;
pub mod optional_runtime;
pub mod prolly;
pub mod provider;
pub mod runtime;
pub mod search;
pub mod sync;
pub mod tabular;
pub mod timeseries;
pub mod traces;
pub mod triggers;
pub mod vcs;
pub mod vector;
pub mod vindex;
pub mod watch;
pub mod workspace;

// Calendar facet. Functions are reached via `calendar::` to avoid name clashes with the
// document facet (`get_collection`) and vcs (`ChangeKind`); the record types are re-exported here.
pub use acl::{
    ACL_MAX_GRANTS_PER_SUBJECT, ACL_MAX_PREDICATE_LEN, ACL_MAX_SCOPE_PREFIX_LEN,
    ACL_MAX_SCOPES_PER_GRANT, ACL_PREDICATE_LANGUAGE_CEL, AclEffect, AclEvaluationContext,
    AclGrant, AclPredicate, AclPredicateEvaluator, AclResource, AclResourceScope, AclRight,
    AclScope, AclScopeKind, AclStore, AclSubject,
};
pub use calendar::{
    CalendarEntry, CollectionMeta, Component, ComponentField, EntryChange, Occurrence,
    calendar_change_scope, entry_changeset as calendar_entry_changeset,
};
pub use capability::{
    CapabilityDimensions, CapabilityInfo, CapabilityOperationalState, CapabilityProfile,
    CapabilityProof, CapabilityRegistry, CapabilityRegistryDimension, CapabilitySet,
    CapabilityVisibility,
};
pub use loom_types::Fence;
// Contacts facet. Functions via `contacts::` to avoid clashes (`EntryChange`/`ChangeKind` also
// exist on calendar); the record types are re-exported here.
pub use contacts::{
    BookMeta, ContactEntry, TypedValue, contacts_change_scope,
    entry_changeset as contacts_entry_changeset,
};
// Mail facet. Functions via `mail::` (it has its own `ChangeKind`); record types re-exported.
pub use cas::{cas_delete, cas_get, cas_has, cas_list, cas_put};
pub use change_set::{
    ChangeCursor, ChangeCursorPosition, ChangeGapState, ChangeItem, ChangeItemKind, ChangeSet,
};
pub use columnar::{
    ColumnarAggregate, ColumnarAggregateOp, ColumnarExecutor, ColumnarInspect, ColumnarSet,
    columnar_aggregate, columnar_aggregate_auto, columnar_append, columnar_columns,
    columnar_compact, columnar_create, columnar_inspect, columnar_rows, columnar_scan,
    columnar_select, columnar_select_auto, columnar_source_digest, get_columnar, put_columnar,
};
#[cfg(feature = "columnar-arrow")]
pub use columnar_arrow::{
    columnar_from_arrow_ipc, columnar_from_parquet, columnar_to_arrow_ipc, columnar_to_parquet,
};
pub use dataframe::{
    DataframeAggregation, DataframeBatch, DataframeColumn, DataframeExecutor, DataframeInputFormat,
    DataframeMaterialization, DataframeMaterializationTarget, DataframeOperation, DataframePlan,
    DataframeSchema, DataframeSourceBinding, DataframeSourceKind, dataframe_collect,
    dataframe_collect_auto, dataframe_create, dataframe_load_plan_sources, dataframe_load_source,
    dataframe_materialize, dataframe_materialize_auto, dataframe_parse_bytes,
    dataframe_parse_sql_result, dataframe_plan_digest, dataframe_preview, dataframe_preview_auto,
    dataframe_source_digests, get_dataframe_plan, put_dataframe_plan,
};
pub use delivery::{
    DeliveryEnvelope, DeliveryMessage, DeliveryProduceRequest, DeliveryReplay, delivery_ack,
    delivery_ack_position, delivery_change_scope, delivery_change_set, delivery_produce,
    delivery_replay, delivery_set_retained_low_water_mark,
};
pub use digest::{Algo, Digest};
pub use document::{
    Collection, DOCUMENT_CHUNK_THRESHOLD, DOCUMENT_COLLECTION_ROOT_FORMAT, DocumentBinary,
    DocumentBodyRef, DocumentCollectionManifest, DocumentCollectionPolicy, DocumentFieldPath,
    DocumentFieldPathSegment, DocumentId, DocumentIndexCatalog, DocumentIndexDeclaration,
    DocumentIndexDef, DocumentIndexStatus, DocumentIndexUniqueness, DocumentMutationResult,
    DocumentPolicyConfig, DocumentPredicate, DocumentProjection, DocumentPutResult, DocumentQuery,
    DocumentQueryItem, DocumentQueryResult, DocumentRecord, DocumentRecordState, DocumentText,
    DocumentTombstoneRecord, doc_create_index, doc_create_index_declaration, doc_delete,
    doc_drop_index, doc_extract_index_value, doc_find, doc_index_statuses, doc_list_collections,
    doc_list_index_declarations, doc_list_indexes, doc_query, doc_rebuild_index,
    document_content_tag, document_delete_with_request, document_entity_tag,
    document_entity_tag_string, document_entity_tag_string_from_digest, document_field_path_string,
    document_get_binary, document_get_text, document_ids_json,
    document_index_declaration_from_json, document_index_declaration_json,
    document_index_declarations_json, document_index_statuses_json, document_index_value_from_json,
    document_indexes_json, document_list_binary, document_put_binary,
    document_put_binary_with_entity_tag, document_put_binary_with_request, document_put_text,
    document_put_text_with_entity_tag, document_query_from_json, document_query_result_json,
    document_set_retention_policy, get_collection, parse_document_entity_tag, put_collection,
};
pub use error::{Code, LoomError, Result};
pub use fs::{DirEntry, FileKind, Stat};
pub use graph::{
    Edge, GRAPH_GEOMETRY_TAG, Graph, GraphCrs, GraphEdgeDiff, GraphEdgeEndpointChange,
    GraphEdgeLabelChange, GraphEdgePattern, GraphGeometry, GraphIndexDiff, GraphIndexEntity,
    GraphIndexStatus, GraphMergeConflict, GraphMergeConflictEntity, GraphMergeConflictKind,
    GraphMutation, GraphMutationIdentity, GraphMutationPlan, GraphMutationResult, GraphNodeDiff,
    GraphNodePattern, GraphOrder, GraphOrderDirection, GraphOrderItem, GraphPath, GraphPattern,
    GraphPredicate, GraphPropertyIndex, GraphPropertyIndexReport, GraphQuery, GraphQueryEdge,
    GraphQueryExplain, GraphQueryIndexSelection, GraphQueryNode, GraphQueryResult, GraphQueryValue,
    GraphReturn, GraphSemanticDiff, GraphSemanticMergeResult, GraphSpatialIndex,
    GraphSpatialIndexReport, GraphValue, Node, Props, get_graph, graph_apply_mutations,
    graph_declare_property_index, graph_declare_spatial_index, graph_edges, graph_explain_query,
    graph_get_edge, graph_get_node, graph_get_node_labels, graph_in_edges, graph_neighbors,
    graph_out_edges, graph_property_index_reports, graph_query, graph_query_with_full_text,
    graph_query_with_full_text_auto, graph_reachable, graph_remove_edge, graph_remove_node,
    graph_set_node_labels, graph_shortest_path, graph_spatial_index_reports, graph_upsert_edge,
    graph_upsert_node, graph_upsert_node_with_labels, put_graph,
};
pub use hooks::{
    HookExecutionPlan, HookPolicyRefusal, HookRegistration, HookScope, PimEventEnvelope,
    hook_emit_event, hook_event_envelope_from_cbor, hook_event_envelope_to_cbor,
    hook_event_history, hook_get, hook_list, hook_list_matching, hook_plan_event, hook_put,
    hook_registration_from_cbor, hook_registration_to_cbor, hook_remove,
};
pub use identity::{
    AppCredential, ExternalCredential, ExternalCredentialChallenge, ExternalCredentialKind,
    ExternalCredentialSpec, IDENTITY_AUTHORITY_HANDOFF_ALG_ES256,
    IDENTITY_AUTHORITY_HANDOFF_PAYLOAD_TYPE, IDENTITY_AUTHORITY_HANDOFF_RECORD_TYPE,
    IDENTITY_AUTHORITY_WITNESS_RECORD_TYPE, IDENTITY_MAX_PUBLIC_KEY_LEN,
    IDENTITY_PRINCIPAL_SIGNED_PAYLOAD_TYPE, IDENTITY_SIGNATURE_SUITE_ED25519,
    IdentityAuthorityDetach, IdentityAuthorityHandoff, IdentityAuthorityMode,
    IdentityAuthorityState, IdentityAuthoritySyncReport, IdentityAuthorityWitness,
    IdentityPublicKey, IdentityPublicKeySpec, IdentityRole, IdentityStore, Principal, PrincipalId,
    PrincipalKind, ROLE_ADMIN_ID, ROLE_OPERATOR_ID, ROLE_READER_ID, ROLE_SERVICE_ID,
    ROLE_WRITER_ID, RoleId, Session, VerifiedExternalCredentialAuth, app_credential_token,
    identity_authority_handoff_payload, identity_authority_handoff_record,
    principal_signature_payload,
};
pub use inference::{
    EmbeddingModel, EmbeddingProvider, Embeddings, INFERENCE_INSTANCE_STATE_PATH, Inference,
    InferenceProvider, InferenceRequest, InferenceResponse, ModelPreferences,
    inference_instance_state, put_inference_instance_state,
};
pub use kv::{
    BackPressure, EphemeralKvMap, EphemeralPutOptions, EvictionPolicy, KvCondition, KvExactToken,
    KvMap, KvMapConfig, KvTier, OnEvict, ephemeral_kv_get_read_through,
    ephemeral_kv_put_write_through, get_kv, key_from_cbor, key_to_cbor, kv_delete,
    kv_delete_conditioned, kv_exact_token, kv_get, kv_list, kv_put, kv_put_conditioned, kv_range,
    replace_kv_map,
};
pub use ledger::{
    LEDGER_CHECKPOINT_SIGNATURE_PURPOSE, Ledger, LedgerAppendMode, LedgerCheckpointPayload,
    LedgerCheckpointSignature, LedgerConsistencyProof, LedgerInclusionProof, LedgerProofTree,
    LedgerRangeEntry, LedgerRangeScan, LedgerRangeState, LedgerRetentionRange,
    LedgerSignedCheckpoint, get_ledger, ledger_append, ledger_append_with_mode,
    ledger_attach_checkpoint_signature, ledger_checkpoint_payload, ledger_checkpoint_payload_bytes,
    ledger_consistency_proof, ledger_get, ledger_head, ledger_inclusion_proof, ledger_len,
    ledger_list_collections, ledger_proof_tree, ledger_range, ledger_set_retention_ranges,
    ledger_verify, ledger_verify_checkpoint_signatures, ledger_verify_consistency_proof,
    ledger_verify_inclusion_proof, put_ledger,
};
pub use lock::{LockCoordinator, LockMode, LockOwner, LockToken};
pub use log::{
    Stream, consumer_change_cursor, consumer_change_set, get_stream, put_stream,
    queue_change_scope, retained_low_water_mark, set_retained_low_water_mark,
};
pub use logs::{LogQuery, LogQueryResult, logs_get_record, logs_put_record, logs_query};
pub use loom_logs::{LogRecord, LogSeverityNumber, LogTraceContext, LogValue};
pub use loom_metrics::{
    InstrumentKind as MetricInstrumentKind, MetricDescriptor, MetricDescriptorPolicy,
    MetricDistribution, MetricExemplar, MetricHistogram, MetricMaterializedRollup,
    MetricObservation, MetricRollupValue, MetricRollupWindowStatus, MetricValue,
    Temporality as MetricTemporality,
};
#[cfg(all(feature = "dataframe-polars", not(target_arch = "wasm32")))]
pub use loom_polars::{
    POLARS_DATAFRAME_EXECUTOR, PolarsDataframeExecutionMode, PolarsDataframeExecutionReport,
    PolarsDataframeExecutor, polars_dataframe_execution_report,
};
pub use loom_traces::{
    SpanContext, SpanDetails, SpanEvent, SpanKind, SpanLink, SpanRecord, SpanStatusCode, TraceValue,
};
pub use loom_triggers::{
    FireOutcome, FireRecord, MissedFirePolicy, OverlapPolicy, TriggerBinding, TriggerExecMode,
    TriggerFireCandidate, TriggerId, TriggerKeeperPlan, TriggerKind, TriggerOptions,
    TriggerStimulus, evaluate_time_trigger, fire_record_from_cbor, fire_record_to_cbor,
    stimulus_digest, trigger_binding_from_cbor, trigger_binding_to_cbor, trigger_stimulus_to_cbor,
};
pub use mail::{
    MailAccountUsage, MailFlagAuditSummary, MailFlagDelta, MailFlagRetentionPolicy, MailMessage,
    MailMutableState, MailboxMeta, account_usage as mail_account_usage, get_account_hard_limit,
    mail_change_scope, mutable_state_changeset as mail_mutable_state_changeset,
    set_account_hard_limit,
};
pub use metrics::{
    MetricQuery, MetricQueryPlan, MetricQueryResult, MetricQueryTemporalSemantics, MetricQueryTier,
    MetricRollupMaintenanceResult, MetricTieredQuery, metrics_compact_rollups,
    metrics_get_descriptor, metrics_get_observation, metrics_get_rollup,
    metrics_materialize_rollups, metrics_plan_query, metrics_put_descriptor,
    metrics_put_observation, metrics_query_observations, metrics_rebuild_rollups,
};
pub use object::{
    ChunkRef, Commit, EntryKind, Object, ObjectType, Tag, TreeEntry, content_address,
};
pub use optional_runtime::{
    IpfsGatewayCacheConfig, OptionalRuntimeCapability, OptionalRuntimeConfig, OptionalRuntimeKind,
    TorOnionServiceConfig, activate_optional_runtime, get_ipfs_gateway_cache_config,
    get_optional_runtime_config, get_tor_onion_service_config, optional_runtime_capabilities,
    optional_runtime_capability, set_ipfs_gateway_cache_config, set_optional_runtime_config,
    set_tor_onion_service_config,
};
pub use provider::ObjectStore;
pub use provider::{CompressionHint, memory::MemoryStore};
pub use runtime::{RuntimeProfile, runtime_profile, runtime_profile_with_tls};
pub use search::{
    AnalyzerMapping, Document, FieldMapping, FieldType, FieldValue, Mapping, Query, QueryRequest,
    QueryResponse, SearchCollection, SearchEngine, SearchHit, get_search, put_search,
    search_collections, search_create, search_delete, search_document_cbor,
    search_document_from_cbor, search_drop, search_get, search_ids, search_ids_cbor, search_index,
    search_mapping_from_cbor, search_query, search_query_auto, search_remap,
    search_request_from_cbor, search_response_cbor, search_source_digest,
};
pub use sync::{
    Bundle, MigrationReport, SyncReport, bundle_export, bundle_import, clone_workspace,
    migrate_workspace_profile, push_branch, push_branch_locked, sync_destination_lock_key,
};
pub use tabular::{
    CmpOp, ColumnType, Predicate, Row, Schema, Table, Value, drop_table, get_table, list_tables,
    put_table,
};
pub use timeseries::{
    Series, StructuredPoint, TimeSeriesAggregation, TimeSeriesPolicy, TimeSeriesRollup,
    TimeSeriesValue, get_series, put_series, ts_get, ts_latest, ts_list_collections,
    ts_materialize_rollup, ts_policy, ts_prune_before, ts_put, ts_put_point, ts_range,
    ts_range_points, ts_range_rollup_points, ts_set_policy,
};
pub use traces::{
    TraceQuery, TraceQueryResult, traces_get_span, traces_put_span, traces_query,
    traces_trace_spans,
};
pub use triggers::{
    trigger_append_fire_record, trigger_append_fire_record_system, trigger_enable, trigger_get,
    trigger_history, trigger_history_system, trigger_keeper_due, trigger_list, trigger_put,
    trigger_remove,
};
pub use vcs::{
    Change, ChangeKind, ConflictResolution, FileStat, LiveRootClassDiagnostics,
    LiveRootDiagnostics, LiveRootExample, Loom, MergeOutcome, OpenMode, ProtectedRefPolicy,
    ReachabilityMarkState, ReachabilityMarkStep, ReplayOutcome, Status,
};
pub use vector::{
    Hit, MetaFilter, Metric, VectorEntry, VectorSet, get_vector_set, put_vector_set,
    vector_build_pq_index, vector_create, vector_create_metadata_index, vector_delete,
    vector_drop_metadata_index, vector_embedding_model, vector_get, vector_ids,
    vector_metadata_index_keys, vector_search, vector_search_auto, vector_search_with_policy,
    vector_search_with_pq_policy, vector_source_digest, vector_source_text, vector_upsert,
    vector_upsert_text, vector_upsert_with_source,
};
pub use vindex::{
    AcceleratorPolicy, Csr, DEFAULT_EXACT_THRESHOLD, PqIndex, VectorAccelerator, prune_csr,
    search_auto, search_with_policy,
};
pub use watch::{
    ChangeEvent, DomainChange, UnsupportedDomainDetail, WatchBatch, WatchCursor, WatchDomainDetail,
    WatchDomainSupport, WatchPathChange, WatchSelector, change_event_from_cbor,
    watch_batch_from_cbor, watch_batch_to_cbor, watch_domain_support, watch_domain_supports,
};
pub use workspace::{AclDomain, FacetKind, Registry, WorkspaceId, WorkspaceInfo, WsSelector};

/// The crate version (from Cargo).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
