//! The curated MCP tool surface.
//!
//! This catalog defines every tool the host exposes, the area it lives in, its execution target, and
//! whether it reads or writes. It is
//! intentionally not a 1:1 emission of the IDL. Binding-ergonomic interfaces (sessions, key
//! administration, result decoding, stateful file handles, async plumbing) are folded into the host or
//! returned natively and never appear here.
//!
//! Two test layers keep this honest:
//!
//! - **Drift**: the catalog and documented tool table must list exactly the same tool names.
//! - **Coverage**: every generated target must name a real method on its IDL interface, and every
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

use loom_remote_protocol::generated::{GeneratedOperationId, METHODS, MethodSig};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum CompositeId {
    AppsSurfaceAppsCallTool,
    AppsSurfaceAppsCreate,
    AppsSurfaceAppsList,
    AppsSurfaceAppsReadFile,
    AppsSurfaceAppsRemoveFile,
    AppsSurfaceAppsShow,
    AppsSurfaceAppsWriteFile,
    AskSurfaceAskAnswers,
    AskSurfaceAskQuestions,
    AskSurfaceAskRecord,
    ChatPresence,
    ChatSetPresence,
    DocumentReplaceText,
    DriveAcquireLease,
    DriveBreakLease,
    DriveRefreshLease,
    DriveReleaseLease,
    GlobalSearchSearch,
    LanesCleanup,
    LanesCloseout,
    LifecycleSurfaceLifecyclesActiveClear,
    LifecycleSurfaceLifecyclesActiveSet,
    LifecycleSurfaceLifecyclesCurrentSurface,
    LifecycleSurfaceLifecyclesDefine,
    LifecycleSurfaceLifecyclesDefineStandard,
    LifecycleSurfaceLifecyclesDefinition,
    LifecycleSurfaceLifecyclesDefinitions,
    LifecycleSurfaceLifecyclesInstance,
    LifecycleSurfaceLifecyclesInstances,
    LifecycleSurfaceLifecyclesInstantiate,
    LifecycleSurfaceLifecyclesOperationLog,
    LifecycleSurfaceLifecyclesSnapshot,
    LifecycleSurfaceLifecyclesSnapshotContent,
    LifecycleSurfaceLifecyclesSnapshotPlan,
    LifecycleSurfaceLifecyclesSnapshots,
    LifecycleSurfaceLifecyclesTransition,
    MeetingsSurfaceMeetingsAcceptAnnotation,
    MeetingsSurfaceMeetingsAcceptVocabulary,
    MeetingsSurfaceMeetingsAddEntityMerge,
    MeetingsSurfaceMeetingsAddPromotion,
    MeetingsSurfaceMeetingsExtractionReview,
    MeetingsSurfaceMeetingsGet,
    MeetingsSurfaceMeetingsImportSnapshot,
    MeetingsSurfaceMeetingsList,
    MeetingsSurfaceMeetingsProjectionOutputs,
    MeetingsSurfaceMeetingsPromoteArtifactToReferenceArtifact,
    MeetingsSurfaceMeetingsPromoteDecisionToDecisionLog,
    MeetingsSurfaceMeetingsPromoteQuestionToLifecycle,
    MeetingsSurfaceMeetingsPromoteReferenceToReferenceArtifact,
    MeetingsSurfaceMeetingsPromoteTaskToTicket,
    MeetingsSurfaceMeetingsProposeVocabulary,
    MeetingsSurfaceMeetingsRejectAnnotation,
    MeetingsSurfaceMeetingsRejectVocabulary,
    MeetingsSurfaceMeetingsSearch,
    StoreMaintenancePolicySet,
    StoreMaintenanceRun,
    StoreMaintenanceStatus,
    StudioSurfaceStudioReindex,
    SubstrateSurfaceSubstrateAliasBind,
    SubstrateSurfaceSubstrateAliasList,
    SubstrateSurfaceSubstrateAliasRelease,
    SubstrateSurfaceSubstrateAliasResolve,
    SubstrateSurfaceSubstrateChanges,
    SubstrateSurfaceSubstrateCheckpointBefore,
    SubstrateSurfaceSubstrateHistory,
    SubstrateSurfaceSubstrateReferenceReconcile,
    SubstrateSurfaceSubstrateReferenceStatus,
    SubstrateSurfaceSubstrateRefs,
    SubstrateSurfaceSubstrateRevisionAsOfRoot,
    SubstrateSurfaceSubstrateRevisionAt,
    SubstrateSurfaceSubstrateRevisionLatest,
    SubstrateSurfaceSubstrateTransact,
    SubstrateSurfaceSubstrateViewDefine,
    SubstrateSurfaceSubstrateViewGet,
    SubstrateSurfaceSubstrateViewList,
    SubstrateSurfaceSubstrateWriteAdmissionPolicyGet,
    SubstrateSurfaceSubstrateWriteAdmissionPolicySet,
    TicketsProjects,
    WorkgraphSurfaceWorkgraphChanges,
    WorkgraphSurfaceWorkgraphFactPut,
    WorkgraphSurfaceWorkgraphMetrics,
}

impl CompositeId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AppsSurfaceAppsCallTool => "AppsSurface.apps_call_tool",
            Self::AppsSurfaceAppsCreate => "AppsSurface.apps_create",
            Self::AppsSurfaceAppsList => "AppsSurface.apps_list",
            Self::AppsSurfaceAppsReadFile => "AppsSurface.apps_read_file",
            Self::AppsSurfaceAppsRemoveFile => "AppsSurface.apps_remove_file",
            Self::AppsSurfaceAppsShow => "AppsSurface.apps_show",
            Self::AppsSurfaceAppsWriteFile => "AppsSurface.apps_write_file",
            Self::AskSurfaceAskAnswers => "AskSurface.ask_answers",
            Self::AskSurfaceAskQuestions => "AskSurface.ask_questions",
            Self::AskSurfaceAskRecord => "AskSurface.ask_record",
            Self::ChatPresence => "ChatPresence",
            Self::ChatSetPresence => "ChatSetPresence",
            Self::DocumentReplaceText => "DocumentReplaceText",
            Self::DriveAcquireLease => "DriveAcquireLease",
            Self::DriveBreakLease => "DriveBreakLease",
            Self::DriveRefreshLease => "DriveRefreshLease",
            Self::DriveReleaseLease => "DriveReleaseLease",
            Self::GlobalSearchSearch => "GlobalSearch.search",
            Self::LanesCleanup => "LanesCleanup",
            Self::LanesCloseout => "LanesCloseout",
            Self::LifecycleSurfaceLifecyclesActiveClear => {
                "LifecycleSurface.lifecycles_active_clear"
            }
            Self::LifecycleSurfaceLifecyclesActiveSet => "LifecycleSurface.lifecycles_active_set",
            Self::LifecycleSurfaceLifecyclesCurrentSurface => {
                "LifecycleSurface.lifecycles_current_surface"
            }
            Self::LifecycleSurfaceLifecyclesDefine => "LifecycleSurface.lifecycles_define",
            Self::LifecycleSurfaceLifecyclesDefineStandard => {
                "LifecycleSurface.lifecycles_define_standard"
            }
            Self::LifecycleSurfaceLifecyclesDefinition => "LifecycleSurface.lifecycles_definition",
            Self::LifecycleSurfaceLifecyclesDefinitions => {
                "LifecycleSurface.lifecycles_definitions"
            }
            Self::LifecycleSurfaceLifecyclesInstance => "LifecycleSurface.lifecycles_instance",
            Self::LifecycleSurfaceLifecyclesInstances => "LifecycleSurface.lifecycles_instances",
            Self::LifecycleSurfaceLifecyclesInstantiate => {
                "LifecycleSurface.lifecycles_instantiate"
            }
            Self::LifecycleSurfaceLifecyclesOperationLog => {
                "LifecycleSurface.lifecycles_operation_log"
            }
            Self::LifecycleSurfaceLifecyclesSnapshot => "LifecycleSurface.lifecycles_snapshot",
            Self::LifecycleSurfaceLifecyclesSnapshotContent => {
                "LifecycleSurface.lifecycles_snapshot_content"
            }
            Self::LifecycleSurfaceLifecyclesSnapshotPlan => {
                "LifecycleSurface.lifecycles_snapshot_plan"
            }
            Self::LifecycleSurfaceLifecyclesSnapshots => "LifecycleSurface.lifecycles_snapshots",
            Self::LifecycleSurfaceLifecyclesTransition => "LifecycleSurface.lifecycles_transition",
            Self::MeetingsSurfaceMeetingsAcceptAnnotation => {
                "MeetingsSurface.meetings_accept_annotation"
            }
            Self::MeetingsSurfaceMeetingsAcceptVocabulary => {
                "MeetingsSurface.meetings_accept_vocabulary"
            }
            Self::MeetingsSurfaceMeetingsAddEntityMerge => {
                "MeetingsSurface.meetings_add_entity_merge"
            }
            Self::MeetingsSurfaceMeetingsAddPromotion => "MeetingsSurface.meetings_add_promotion",
            Self::MeetingsSurfaceMeetingsExtractionReview => {
                "MeetingsSurface.meetings_extraction_review"
            }
            Self::MeetingsSurfaceMeetingsGet => "MeetingsSurface.meetings_get",
            Self::MeetingsSurfaceMeetingsImportSnapshot => {
                "MeetingsSurface.meetings_import_snapshot"
            }
            Self::MeetingsSurfaceMeetingsList => "MeetingsSurface.meetings_list",
            Self::MeetingsSurfaceMeetingsProjectionOutputs => {
                "MeetingsSurface.meetings_projection_outputs"
            }
            Self::MeetingsSurfaceMeetingsPromoteArtifactToReferenceArtifact => {
                "MeetingsSurface.meetings_promote_artifact_to_reference_artifact"
            }
            Self::MeetingsSurfaceMeetingsPromoteDecisionToDecisionLog => {
                "MeetingsSurface.meetings_promote_decision_to_decision_log"
            }
            Self::MeetingsSurfaceMeetingsPromoteQuestionToLifecycle => {
                "MeetingsSurface.meetings_promote_question_to_lifecycle"
            }
            Self::MeetingsSurfaceMeetingsPromoteReferenceToReferenceArtifact => {
                "MeetingsSurface.meetings_promote_reference_to_reference_artifact"
            }
            Self::MeetingsSurfaceMeetingsPromoteTaskToTicket => {
                "MeetingsSurface.meetings_promote_task_to_ticket"
            }
            Self::MeetingsSurfaceMeetingsProposeVocabulary => {
                "MeetingsSurface.meetings_propose_vocabulary"
            }
            Self::MeetingsSurfaceMeetingsRejectAnnotation => {
                "MeetingsSurface.meetings_reject_annotation"
            }
            Self::MeetingsSurfaceMeetingsRejectVocabulary => {
                "MeetingsSurface.meetings_reject_vocabulary"
            }
            Self::MeetingsSurfaceMeetingsSearch => "MeetingsSurface.meetings_search",
            Self::StoreMaintenancePolicySet => "StoreMaintenancePolicySet",
            Self::StoreMaintenanceRun => "StoreMaintenanceRun",
            Self::StoreMaintenanceStatus => "StoreMaintenanceStatus",
            Self::StudioSurfaceStudioReindex => "StudioSurface.studio_reindex",
            Self::SubstrateSurfaceSubstrateAliasBind => "SubstrateSurface.substrate_alias_bind",
            Self::SubstrateSurfaceSubstrateAliasList => "SubstrateSurface.substrate_alias_list",
            Self::SubstrateSurfaceSubstrateAliasRelease => {
                "SubstrateSurface.substrate_alias_release"
            }
            Self::SubstrateSurfaceSubstrateAliasResolve => {
                "SubstrateSurface.substrate_alias_resolve"
            }
            Self::SubstrateSurfaceSubstrateChanges => "SubstrateSurface.substrate_changes",
            Self::SubstrateSurfaceSubstrateCheckpointBefore => {
                "SubstrateSurface.substrate_checkpoint_before"
            }
            Self::SubstrateSurfaceSubstrateHistory => "SubstrateSurface.substrate_history",
            Self::SubstrateSurfaceSubstrateReferenceReconcile => {
                "SubstrateSurface.substrate_reference_reconcile"
            }
            Self::SubstrateSurfaceSubstrateReferenceStatus => {
                "SubstrateSurface.substrate_reference_status"
            }
            Self::SubstrateSurfaceSubstrateRefs => "SubstrateSurface.substrate_refs",
            Self::SubstrateSurfaceSubstrateRevisionAsOfRoot => {
                "SubstrateSurface.substrate_revision_as_of_root"
            }
            Self::SubstrateSurfaceSubstrateRevisionAt => "SubstrateSurface.substrate_revision_at",
            Self::SubstrateSurfaceSubstrateRevisionLatest => {
                "SubstrateSurface.substrate_revision_latest"
            }
            Self::SubstrateSurfaceSubstrateTransact => "SubstrateSurface.substrate_transact",
            Self::SubstrateSurfaceSubstrateViewDefine => "SubstrateSurface.substrate_view_define",
            Self::SubstrateSurfaceSubstrateViewGet => "SubstrateSurface.substrate_view_get",
            Self::SubstrateSurfaceSubstrateViewList => "SubstrateSurface.substrate_view_list",
            Self::SubstrateSurfaceSubstrateWriteAdmissionPolicyGet => {
                "SubstrateSurface.substrate_write_admission_policy_get"
            }
            Self::SubstrateSurfaceSubstrateWriteAdmissionPolicySet => {
                "SubstrateSurface.substrate_write_admission_policy_set"
            }
            Self::TicketsProjects => "TicketsProjects",
            Self::WorkgraphSurfaceWorkgraphChanges => "WorkgraphSurface.workgraph_changes",
            Self::WorkgraphSurfaceWorkgraphFactPut => "WorkgraphSurface.workgraph_fact_put",
            Self::WorkgraphSurfaceWorkgraphMetrics => "WorkgraphSurface.workgraph_metrics",
        }
    }

    pub const fn reason(self) -> &'static str {
        match self {
            Self::AppsSurfaceAppsCallTool
            | Self::AppsSurfaceAppsCreate
            | Self::AppsSurfaceAppsList
            | Self::AppsSurfaceAppsReadFile
            | Self::AppsSurfaceAppsRemoveFile
            | Self::AppsSurfaceAppsShow
            | Self::AppsSurfaceAppsWriteFile => {
                "MCP Apps compose app inventory, resources, and file operations outside one IDL method"
            }
            Self::AskSurfaceAskAnswers
            | Self::AskSurfaceAskQuestions
            | Self::AskSurfaceAskRecord => {
                "Ask tools manage host-mediated owner interaction state outside one generated method"
            }
            Self::ChatPresence | Self::ChatSetPresence => {
                "Chat presence is runtime-local client state, not durable generated store state"
            }
            Self::DocumentReplaceText => {
                "Document replace-text preserves MCP editing ergonomics over multiple generated document operations"
            }
            Self::DriveAcquireLease
            | Self::DriveBreakLease
            | Self::DriveRefreshLease
            | Self::DriveReleaseLease => {
                "Drive lease tools coordinate runtime cache ownership outside one generated mutation"
            }
            Self::GlobalSearchSearch => {
                "Global search fans out across search-capable facets instead of one IDL method"
            }
            Self::LanesCleanup | Self::LanesCloseout => {
                "Lane helpers compose lane and ticket transitions into one model-facing operation"
            }
            Self::LifecycleSurfaceLifecyclesActiveClear
            | Self::LifecycleSurfaceLifecyclesActiveSet
            | Self::LifecycleSurfaceLifecyclesCurrentSurface
            | Self::LifecycleSurfaceLifecyclesDefine
            | Self::LifecycleSurfaceLifecyclesDefineStandard
            | Self::LifecycleSurfaceLifecyclesDefinition
            | Self::LifecycleSurfaceLifecyclesDefinitions
            | Self::LifecycleSurfaceLifecyclesInstance
            | Self::LifecycleSurfaceLifecyclesInstances
            | Self::LifecycleSurfaceLifecyclesInstantiate
            | Self::LifecycleSurfaceLifecyclesOperationLog
            | Self::LifecycleSurfaceLifecyclesSnapshot
            | Self::LifecycleSurfaceLifecyclesSnapshotContent
            | Self::LifecycleSurfaceLifecyclesSnapshotPlan
            | Self::LifecycleSurfaceLifecyclesSnapshots
            | Self::LifecycleSurfaceLifecyclesTransition => {
                "Lifecycle surface tools bind active lifecycle presentation and snapshots outside raw IDL calls"
            }
            Self::MeetingsSurfaceMeetingsAcceptAnnotation
            | Self::MeetingsSurfaceMeetingsAcceptVocabulary
            | Self::MeetingsSurfaceMeetingsAddEntityMerge
            | Self::MeetingsSurfaceMeetingsAddPromotion
            | Self::MeetingsSurfaceMeetingsExtractionReview
            | Self::MeetingsSurfaceMeetingsGet
            | Self::MeetingsSurfaceMeetingsImportSnapshot
            | Self::MeetingsSurfaceMeetingsList
            | Self::MeetingsSurfaceMeetingsProjectionOutputs
            | Self::MeetingsSurfaceMeetingsPromoteArtifactToReferenceArtifact
            | Self::MeetingsSurfaceMeetingsPromoteDecisionToDecisionLog
            | Self::MeetingsSurfaceMeetingsPromoteQuestionToLifecycle
            | Self::MeetingsSurfaceMeetingsPromoteReferenceToReferenceArtifact
            | Self::MeetingsSurfaceMeetingsPromoteTaskToTicket
            | Self::MeetingsSurfaceMeetingsProposeVocabulary
            | Self::MeetingsSurfaceMeetingsRejectAnnotation
            | Self::MeetingsSurfaceMeetingsRejectVocabulary
            | Self::MeetingsSurfaceMeetingsSearch => {
                "Meetings tools compose review, vocabulary, and promotion workflows outside one IDL method"
            }
            Self::StoreMaintenancePolicySet
            | Self::StoreMaintenanceRun
            | Self::StoreMaintenanceStatus => {
                "Store maintenance is a host-owned operational workflow over the concrete store"
            }
            Self::StudioSurfaceStudioReindex => {
                "Studio reindex is a host-owned cache rebuild operation"
            }
            Self::SubstrateSurfaceSubstrateAliasBind
            | Self::SubstrateSurfaceSubstrateAliasList
            | Self::SubstrateSurfaceSubstrateAliasRelease
            | Self::SubstrateSurfaceSubstrateAliasResolve
            | Self::SubstrateSurfaceSubstrateChanges
            | Self::SubstrateSurfaceSubstrateCheckpointBefore
            | Self::SubstrateSurfaceSubstrateHistory
            | Self::SubstrateSurfaceSubstrateReferenceReconcile
            | Self::SubstrateSurfaceSubstrateReferenceStatus
            | Self::SubstrateSurfaceSubstrateRefs
            | Self::SubstrateSurfaceSubstrateRevisionAsOfRoot
            | Self::SubstrateSurfaceSubstrateRevisionAt
            | Self::SubstrateSurfaceSubstrateRevisionLatest
            | Self::SubstrateSurfaceSubstrateTransact
            | Self::SubstrateSurfaceSubstrateViewDefine
            | Self::SubstrateSurfaceSubstrateViewGet
            | Self::SubstrateSurfaceSubstrateViewList
            | Self::SubstrateSurfaceSubstrateWriteAdmissionPolicyGet
            | Self::SubstrateSurfaceSubstrateWriteAdmissionPolicySet => {
                "Substrate surface tools compose revision, alias, reference, and admission workflows"
            }
            Self::TicketsProjects => {
                "Ticket project listing aggregates project state into one model-facing summary"
            }
            Self::WorkgraphSurfaceWorkgraphChanges
            | Self::WorkgraphSurfaceWorkgraphFactPut
            | Self::WorkgraphSurfaceWorkgraphMetrics => {
                "Workgraph tools compose task graph facts and metrics outside one IDL method"
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum AdapterId {
    InterchangeAdapterImportExecuteBatch,
    InterchangeAdapterImportSubmitBatch,
    InterchangeAdapterRedmineImportSnapshot,
}

impl AdapterId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InterchangeAdapterImportExecuteBatch => "InterchangeAdapter.import_execute_batch",
            Self::InterchangeAdapterImportSubmitBatch => "InterchangeAdapter.import_submit_batch",
            Self::InterchangeAdapterRedmineImportSnapshot => {
                "InterchangeAdapter.redmine_import_snapshot"
            }
        }
    }

    pub const fn reason(self) -> &'static str {
        match self {
            Self::InterchangeAdapterImportExecuteBatch
            | Self::InterchangeAdapterImportSubmitBatch
            | Self::InterchangeAdapterRedmineImportSnapshot => {
                "Interchange imports own external format parsing and batch execution before generated store effects"
            }
        }
    }
}

/// The accepted execution-boundary target for one MCP tool.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecutionTarget {
    Generated(GeneratedOperationId),
    Composite(CompositeId),
    OwningAdapter(AdapterId),
}

/// Runtime adapter contract for generated MCP tools that have been explicitly migrated.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GeneratedMcpProjection {
    Canonical,
    GraphRemoveEdge,
}

/// How a tool is served when the MCP host is backed by a remote Loom endpoint.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemoteCapability {
    /// A generated IDL target using ordinary request/response execution.
    Unary,
    /// A generated IDL target that needs handle/stream machinery.
    HandleStream,
    /// MCP-level orchestration that must execute beside the served store.
    ServerExecute,
}

/// The `(interface, method)` pairs whose exposed MCP tool genuinely needs the remote host to reject
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

#[cfg(test)]
const SHARED_GENERATED_OPERATION_OWNERSHIP: &[(GeneratedOperationId, &[&str], &str)] = &[(
    GeneratedOperationId::StoreCapabilities,
    &["store_capabilities", "store_capabilities_json"],
    "The same Store.capabilities bytes back the binary and JSON-presented MCP capability tools",
)];

/// One curated tool in the MCP surface.
#[derive(Clone, Copy, Debug)]
pub struct ToolSpec {
    /// The wire name, `<area>.<verb>` in snake_case.
    pub name: &'static str,
    /// The lower-case area (facet or subsystem) the tool belongs to.
    pub area: &'static str,
    /// The accepted execution target for this tool.
    pub target: ExecutionTarget,
    /// Whether the tool reads or writes.
    pub kind: ToolKind,
    /// The typed MCP-to-IDL projection contract for generated runtime execution.
    pub generated_projection: Option<GeneratedMcpProjection>,
}

const fn read_generated(
    name: &'static str,
    area: &'static str,
    operation: GeneratedOperationId,
) -> ToolSpec {
    ToolSpec {
        name,
        area,
        target: ExecutionTarget::Generated(operation),
        kind: ToolKind::Read,
        generated_projection: None,
    }
}

const fn write_generated(
    name: &'static str,
    area: &'static str,
    operation: GeneratedOperationId,
) -> ToolSpec {
    ToolSpec {
        name,
        area,
        target: ExecutionTarget::Generated(operation),
        kind: ToolKind::Write,
        generated_projection: None,
    }
}

const fn read_projected(
    name: &'static str,
    area: &'static str,
    operation: GeneratedOperationId,
    projection: GeneratedMcpProjection,
) -> ToolSpec {
    ToolSpec {
        name,
        area,
        target: ExecutionTarget::Generated(operation),
        kind: ToolKind::Read,
        generated_projection: Some(projection),
    }
}

const fn write_projected(
    name: &'static str,
    area: &'static str,
    operation: GeneratedOperationId,
    projection: GeneratedMcpProjection,
) -> ToolSpec {
    ToolSpec {
        name,
        area,
        target: ExecutionTarget::Generated(operation),
        kind: ToolKind::Write,
        generated_projection: Some(projection),
    }
}

const fn read_composite(
    name: &'static str,
    area: &'static str,
    composite_id: CompositeId,
) -> ToolSpec {
    ToolSpec {
        name,
        area,
        target: ExecutionTarget::Composite(composite_id),
        kind: ToolKind::Read,
        generated_projection: None,
    }
}

const fn write_composite(
    name: &'static str,
    area: &'static str,
    composite_id: CompositeId,
) -> ToolSpec {
    ToolSpec {
        name,
        area,
        target: ExecutionTarget::Composite(composite_id),
        kind: ToolKind::Write,
        generated_projection: None,
    }
}

const fn write_owning_adapter(
    name: &'static str,
    area: &'static str,
    adapter_id: AdapterId,
) -> ToolSpec {
    ToolSpec {
        name,
        area,
        target: ExecutionTarget::OwningAdapter(adapter_id),
        kind: ToolKind::Write,
        generated_projection: None,
    }
}

/// The curated tool surface. Ordered by area.
pub const TOOL_SURFACE: &[ToolSpec] = &[
    // store
    read_projected(
        "store_version",
        "store",
        GeneratedOperationId::StoreVersion,
        GeneratedMcpProjection::Canonical,
    ),
    read_generated(
        "store_capabilities",
        "store",
        GeneratedOperationId::StoreCapabilities,
    ),
    read_generated(
        "store_capabilities_json",
        "store",
        GeneratedOperationId::StoreCapabilities,
    ),
    read_generated(
        "store_blob_digest",
        "store",
        GeneratedOperationId::StoreBlobDigest,
    ),
    read_generated(
        "store_policy_get",
        "store",
        GeneratedOperationId::StoreAdminStorePolicyGet,
    ),
    write_generated(
        "store_policy_set",
        "store",
        GeneratedOperationId::StoreAdminStorePolicySet,
    ),
    write_generated(
        "store_bundle_import",
        "store",
        GeneratedOperationId::StoreAdminStoreBundleImport,
    ),
    read_generated(
        "store_maintenance_status",
        "store",
        GeneratedOperationId::StoreAdminStoreMaintenanceStatus,
    ),
    write_generated(
        "store_maintenance_policy_set",
        "store",
        GeneratedOperationId::StoreAdminStoreMaintenancePolicySet,
    ),
    write_generated(
        "store_maintenance_run",
        "store",
        GeneratedOperationId::StoreAdminStoreMaintenanceRun,
    ),
    // metrics
    write_generated(
        "metrics_put_descriptor",
        "metrics",
        GeneratedOperationId::MetricsPutDescriptor,
    ),
    read_generated(
        "metrics_get_descriptor",
        "metrics",
        GeneratedOperationId::MetricsGetDescriptor,
    ),
    write_generated(
        "metrics_put_observation",
        "metrics",
        GeneratedOperationId::MetricsPutObservation,
    ),
    read_generated(
        "metrics_query",
        "metrics",
        GeneratedOperationId::MetricsQuery,
    ),
    // logs
    write_generated(
        "logs_put_record",
        "logs",
        GeneratedOperationId::LogsPutRecord,
    ),
    read_generated(
        "logs_get_record",
        "logs",
        GeneratedOperationId::LogsGetRecord,
    ),
    read_generated("logs_query", "logs", GeneratedOperationId::LogsQuery),
    // traces
    write_generated(
        "traces_put_span",
        "traces",
        GeneratedOperationId::TracesPutSpan,
    ),
    read_generated(
        "traces_get_span",
        "traces",
        GeneratedOperationId::TracesGetSpan,
    ),
    read_generated(
        "traces_trace_spans",
        "traces",
        GeneratedOperationId::TracesTraceSpans,
    ),
    read_generated("traces_query", "traces", GeneratedOperationId::TracesQuery),
    // workspace
    read_generated(
        "workspace_list",
        "workspace",
        GeneratedOperationId::WorkspacesWorkspaceList,
    ),
    // vcs
    write_generated(
        "vcs_commit",
        "vcs",
        GeneratedOperationId::VersionControlCommit,
    ),
    write_generated(
        "vcs_branch",
        "vcs",
        GeneratedOperationId::VersionControlBranch,
    ),
    write_generated(
        "vcs_checkout",
        "vcs",
        GeneratedOperationId::VersionControlCheckout,
    ),
    read_generated(
        "vcs_head_branch",
        "vcs",
        GeneratedOperationId::VersionControlHeadBranch,
    ),
    read_generated("vcs_log", "vcs", GeneratedOperationId::VersionControlLog),
    write_generated(
        "vcs_merge",
        "vcs",
        GeneratedOperationId::VersionControlMerge,
    ),
    read_generated(
        "vcs_merge_in_progress",
        "vcs",
        GeneratedOperationId::VersionControlMergeInProgress,
    ),
    read_generated(
        "vcs_merge_conflicts",
        "vcs",
        GeneratedOperationId::VersionControlMergeConflicts,
    ),
    write_generated(
        "vcs_merge_resolve",
        "vcs",
        GeneratedOperationId::VersionControlMergeResolve,
    ),
    write_generated(
        "vcs_merge_abort",
        "vcs",
        GeneratedOperationId::VersionControlMergeAbort,
    ),
    write_generated(
        "vcs_merge_continue",
        "vcs",
        GeneratedOperationId::VersionControlMergeContinue,
    ),
    read_generated(
        "vcs_status",
        "vcs",
        GeneratedOperationId::VersionControlStatus,
    ),
    write_generated(
        "vcs_stage",
        "vcs",
        GeneratedOperationId::VersionControlStage,
    ),
    write_generated(
        "vcs_stage_all",
        "vcs",
        GeneratedOperationId::VersionControlStageAll,
    ),
    write_generated(
        "vcs_unstage",
        "vcs",
        GeneratedOperationId::VersionControlUnstage,
    ),
    write_generated(
        "vcs_commit_staged",
        "vcs",
        GeneratedOperationId::VersionControlCommitStaged,
    ),
    write_generated(
        "vcs_tag_create",
        "vcs",
        GeneratedOperationId::VersionControlTagCreate,
    ),
    read_generated(
        "vcs_tag_list",
        "vcs",
        GeneratedOperationId::VersionControlTagList,
    ),
    read_generated(
        "vcs_tag_target",
        "vcs",
        GeneratedOperationId::VersionControlTagTarget,
    ),
    write_generated(
        "vcs_tag_delete",
        "vcs",
        GeneratedOperationId::VersionControlTagDelete,
    ),
    write_generated(
        "vcs_tag_rename",
        "vcs",
        GeneratedOperationId::VersionControlTagRename,
    ),
    write_generated(
        "vcs_restore_file",
        "vcs",
        GeneratedOperationId::VersionControlRestoreFile,
    ),
    write_generated(
        "vcs_restore_path",
        "vcs",
        GeneratedOperationId::VersionControlRestorePath,
    ),
    write_generated(
        "vcs_cherry_pick",
        "vcs",
        GeneratedOperationId::VersionControlCherryPick,
    ),
    write_generated(
        "vcs_revert",
        "vcs",
        GeneratedOperationId::VersionControlRevert,
    ),
    write_generated(
        "vcs_rebase",
        "vcs",
        GeneratedOperationId::VersionControlRebase,
    ),
    write_generated(
        "vcs_squash",
        "vcs",
        GeneratedOperationId::VersionControlSquash,
    ),
    read_generated("vcs_diff", "vcs", GeneratedOperationId::VersionControlDiff),
    read_generated(
        "vcs_blame",
        "vcs",
        GeneratedOperationId::VersionControlBlame,
    ),
    // watch
    read_generated(
        "watch_subscribe",
        "watch",
        GeneratedOperationId::WatchSubscribe,
    ),
    read_generated("watch_poll", "watch", GeneratedOperationId::WatchPoll),
    // fs
    write_generated(
        "fs_write_file",
        "fs",
        GeneratedOperationId::FileSystemWriteFile,
    ),
    read_generated(
        "fs_read_file",
        "fs",
        GeneratedOperationId::FileSystemReadFile,
    ),
    write_generated(
        "fs_append_file",
        "fs",
        GeneratedOperationId::FileSystemAppendFile,
    ),
    write_generated(
        "fs_remove_file",
        "fs",
        GeneratedOperationId::FileSystemRemoveFile,
    ),
    read_generated("fs_read_at", "fs", GeneratedOperationId::FileSystemReadAt),
    read_generated("fs_stat", "fs", GeneratedOperationId::FileSystemStat),
    read_generated(
        "fs_list_directory",
        "fs",
        GeneratedOperationId::FileSystemListDirectory,
    ),
    write_generated(
        "fs_create_directory",
        "fs",
        GeneratedOperationId::FileSystemCreateDirectory,
    ),
    write_generated(
        "fs_remove_directory",
        "fs",
        GeneratedOperationId::FileSystemRemoveDirectory,
    ),
    write_generated("fs_write_at", "fs", GeneratedOperationId::FileSystemWriteAt),
    write_generated(
        "fs_truncate",
        "fs",
        GeneratedOperationId::FileSystemTruncate,
    ),
    write_generated("fs_symlink", "fs", GeneratedOperationId::FileSystemSymlink),
    read_generated(
        "fs_read_link",
        "fs",
        GeneratedOperationId::FileSystemReadLink,
    ),
    // apps
    read_composite("apps_list", "apps", CompositeId::AppsSurfaceAppsList),
    read_composite("apps_show", "apps", CompositeId::AppsSurfaceAppsShow),
    read_composite(
        "apps_read_file",
        "apps",
        CompositeId::AppsSurfaceAppsReadFile,
    ),
    write_composite("apps_create", "apps", CompositeId::AppsSurfaceAppsCreate),
    write_composite(
        "apps_write_file",
        "apps",
        CompositeId::AppsSurfaceAppsWriteFile,
    ),
    write_composite(
        "apps_remove_file",
        "apps",
        CompositeId::AppsSurfaceAppsRemoveFile,
    ),
    write_composite(
        "apps_call_tool",
        "apps",
        CompositeId::AppsSurfaceAppsCallTool,
    ),
    // ask
    write_composite("ask_questions", "ask", CompositeId::AskSurfaceAskQuestions),
    read_composite("ask_answers", "ask", CompositeId::AskSurfaceAskAnswers),
    write_composite("ask_record", "ask", CompositeId::AskSurfaceAskRecord),
    // cas
    write_generated("cas_put", "cas", GeneratedOperationId::CasPut),
    read_generated("cas_get", "cas", GeneratedOperationId::CasGet),
    read_generated("cas_has", "cas", GeneratedOperationId::CasHas),
    write_generated("cas_delete", "cas", GeneratedOperationId::CasDelete),
    read_generated("cas_list", "cas", GeneratedOperationId::CasList),
    // graph
    write_generated(
        "graph_upsert_node",
        "graph",
        GeneratedOperationId::GraphUpsertNode,
    ),
    read_generated(
        "graph_get_node",
        "graph",
        GeneratedOperationId::GraphGetNode,
    ),
    write_generated(
        "graph_remove_node",
        "graph",
        GeneratedOperationId::GraphRemoveNode,
    ),
    write_generated(
        "graph_upsert_edge",
        "graph",
        GeneratedOperationId::GraphUpsertEdge,
    ),
    read_generated(
        "graph_get_edge",
        "graph",
        GeneratedOperationId::GraphGetEdge,
    ),
    write_projected(
        "graph_remove_edge",
        "graph",
        GeneratedOperationId::GraphRemoveEdge,
        GeneratedMcpProjection::GraphRemoveEdge,
    ),
    read_generated(
        "graph_neighbors",
        "graph",
        GeneratedOperationId::GraphNeighbors,
    ),
    read_generated(
        "graph_out_edges",
        "graph",
        GeneratedOperationId::GraphOutEdges,
    ),
    read_generated(
        "graph_in_edges",
        "graph",
        GeneratedOperationId::GraphInEdges,
    ),
    read_generated(
        "graph_reachable",
        "graph",
        GeneratedOperationId::GraphReachable,
    ),
    read_generated(
        "graph_shortest_path",
        "graph",
        GeneratedOperationId::GraphShortestPath,
    ),
    read_generated("graph_query", "graph", GeneratedOperationId::GraphQuery),
    read_generated(
        "graph_explain_query",
        "graph",
        GeneratedOperationId::GraphExplainQuery,
    ),
    // vector
    write_generated(
        "vector_create",
        "vector",
        GeneratedOperationId::VectorCreate,
    ),
    write_generated(
        "vector_upsert",
        "vector",
        GeneratedOperationId::VectorUpsert,
    ),
    write_generated(
        "vector_upsert_source",
        "vector",
        GeneratedOperationId::VectorUpsertSource,
    ),
    read_generated("vector_get", "vector", GeneratedOperationId::VectorGet),
    read_generated(
        "vector_source_text",
        "vector",
        GeneratedOperationId::VectorSourceText,
    ),
    read_generated(
        "vector_embedding_model",
        "vector",
        GeneratedOperationId::VectorEmbeddingModel,
    ),
    read_generated("vector_ids", "vector", GeneratedOperationId::VectorIds),
    read_generated(
        "vector_metadata_index_keys",
        "vector",
        GeneratedOperationId::VectorMetadataIndexKeys,
    ),
    write_generated(
        "vector_create_metadata_index",
        "vector",
        GeneratedOperationId::VectorCreateMetadataIndex,
    ),
    write_generated(
        "vector_drop_metadata_index",
        "vector",
        GeneratedOperationId::VectorDropMetadataIndex,
    ),
    write_generated(
        "vector_delete",
        "vector",
        GeneratedOperationId::VectorDelete,
    ),
    read_generated(
        "vector_search",
        "vector",
        GeneratedOperationId::VectorSearch,
    ),
    read_generated(
        "vector_search_policy",
        "vector",
        GeneratedOperationId::VectorSearchPolicy,
    ),
    write_generated(
        "vector_text_upsert",
        "vector",
        GeneratedOperationId::VectorVectorTextUpsert,
    ),
    write_generated(
        "vector_workspace_configure_json",
        "vector",
        GeneratedOperationId::VectorVectorWorkspaceConfigureJson,
    ),
    // columnar
    write_generated(
        "columnar_create",
        "columnar",
        GeneratedOperationId::ColumnarCreate,
    ),
    write_generated(
        "columnar_append",
        "columnar",
        GeneratedOperationId::ColumnarAppend,
    ),
    write_generated(
        "columnar_compact",
        "columnar",
        GeneratedOperationId::ColumnarCompact,
    ),
    read_generated(
        "columnar_scan",
        "columnar",
        GeneratedOperationId::ColumnarScan,
    ),
    read_generated(
        "columnar_columns",
        "columnar",
        GeneratedOperationId::ColumnarColumns,
    ),
    read_generated(
        "columnar_rows",
        "columnar",
        GeneratedOperationId::ColumnarRows,
    ),
    read_generated(
        "columnar_inspect",
        "columnar",
        GeneratedOperationId::ColumnarInspect,
    ),
    read_generated(
        "columnar_source_digest",
        "columnar",
        GeneratedOperationId::ColumnarSourceDigest,
    ),
    read_generated(
        "columnar_select",
        "columnar",
        GeneratedOperationId::ColumnarSelect,
    ),
    read_generated(
        "columnar_aggregate",
        "columnar",
        GeneratedOperationId::ColumnarAggregate,
    ),
    write_generated(
        "columnar_import_arrow",
        "columnar",
        GeneratedOperationId::ColumnarColumnarImportArrow,
    ),
    write_generated(
        "columnar_import_parquet",
        "columnar",
        GeneratedOperationId::ColumnarColumnarImportParquet,
    ),
    // dataframe
    write_generated(
        "dataframe_create",
        "dataframe",
        GeneratedOperationId::DataframeCreate,
    ),
    read_generated(
        "dataframe_collect",
        "dataframe",
        GeneratedOperationId::DataframeCollect,
    ),
    read_generated(
        "dataframe_preview",
        "dataframe",
        GeneratedOperationId::DataframePreview,
    ),
    write_generated(
        "dataframe_materialize",
        "dataframe",
        GeneratedOperationId::DataframeMaterialize,
    ),
    read_generated(
        "dataframe_plan_digest",
        "dataframe",
        GeneratedOperationId::DataframePlanDigest,
    ),
    read_generated(
        "dataframe_source_digests",
        "dataframe",
        GeneratedOperationId::DataframeSourceDigests,
    ),
    // fts
    write_generated("fts_create", "fts", GeneratedOperationId::SearchCreate),
    write_generated("fts_index", "fts", GeneratedOperationId::SearchIndex),
    read_generated("fts_get", "fts", GeneratedOperationId::SearchGet),
    write_generated("fts_delete", "fts", GeneratedOperationId::SearchDelete),
    read_generated("fts_ids", "fts", GeneratedOperationId::SearchIds),
    write_generated("fts_remap", "fts", GeneratedOperationId::SearchRemap),
    read_generated("fts_query", "fts", GeneratedOperationId::SearchQuery),
    read_generated(
        "fts_source_digest",
        "fts",
        GeneratedOperationId::SearchSourceDigest,
    ),
    read_generated("fts_status", "fts", GeneratedOperationId::SearchStatus),
    // search
    read_composite("search", "search", CompositeId::GlobalSearchSearch),
    // substrate
    read_composite(
        "substrate_changes",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateChanges,
    ),
    // workgraph
    read_composite(
        "workgraph_changes",
        "workgraph",
        CompositeId::WorkgraphSurfaceWorkgraphChanges,
    ),
    read_composite(
        "workgraph_metrics",
        "workgraph",
        CompositeId::WorkgraphSurfaceWorkgraphMetrics,
    ),
    write_composite(
        "workgraph_fact_put",
        "workgraph",
        CompositeId::WorkgraphSurfaceWorkgraphFactPut,
    ),
    // substrate
    read_composite(
        "substrate_refs",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateRefs,
    ),
    write_composite(
        "substrate_alias_bind",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateAliasBind,
    ),
    write_composite(
        "substrate_alias_release",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateAliasRelease,
    ),
    read_composite(
        "substrate_alias_resolve",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateAliasResolve,
    ),
    read_composite(
        "substrate_alias_list",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateAliasList,
    ),
    read_composite(
        "substrate_reference_status",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateReferenceStatus,
    ),
    write_composite(
        "substrate_reference_reconcile",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateReferenceReconcile,
    ),
    read_composite(
        "substrate_history",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateHistory,
    ),
    read_composite(
        "substrate_revision_latest",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateRevisionLatest,
    ),
    read_composite(
        "substrate_revision_at",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateRevisionAt,
    ),
    read_composite(
        "substrate_revision_as_of_root",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateRevisionAsOfRoot,
    ),
    read_composite(
        "substrate_checkpoint_before",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateCheckpointBefore,
    ),
    write_composite(
        "substrate_transact",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateTransact,
    ),
    write_composite(
        "substrate_view_define",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateViewDefine,
    ),
    read_composite(
        "substrate_view_get",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateViewGet,
    ),
    read_composite(
        "substrate_view_list",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateViewList,
    ),
    read_composite(
        "substrate_write_admission_policy_get",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateWriteAdmissionPolicyGet,
    ),
    write_composite(
        "substrate_write_admission_policy_set",
        "substrate",
        CompositeId::SubstrateSurfaceSubstrateWriteAdmissionPolicySet,
    ),
    // tickets
    write_generated(
        "tickets_project_create",
        "tickets",
        GeneratedOperationId::TicketsTicketsProjectCreateJson,
    ),
    write_generated(
        "tickets_project_rekey",
        "tickets",
        GeneratedOperationId::TicketsTicketsProjectRekeyJson,
    ),
    read_generated(
        "tickets_project_settings_get",
        "tickets",
        GeneratedOperationId::TicketsTicketsProjectSettingsGetJson,
    ),
    write_generated(
        "tickets_project_settings_set",
        "tickets",
        GeneratedOperationId::TicketsTicketsProjectSettingsSetJson,
    ),
    read_composite("tickets_projects", "tickets", CompositeId::TicketsProjects),
    read_generated(
        "tickets_relations",
        "tickets",
        GeneratedOperationId::TicketsTicketsRelationListJson,
    ),
    read_generated(
        "tickets_fields",
        "tickets",
        GeneratedOperationId::TicketsTicketsFieldsJson,
    ),
    write_generated(
        "tickets_field_put",
        "tickets",
        GeneratedOperationId::TicketsTicketsFieldPutJson,
    ),
    write_generated(
        "tickets_field_retire",
        "tickets",
        GeneratedOperationId::TicketsTicketsFieldRetireJson,
    ),
    write_generated(
        "tickets_create",
        "tickets",
        GeneratedOperationId::TicketsTicketsCreateJson,
    ),
    write_generated(
        "tickets_update",
        "tickets",
        GeneratedOperationId::TicketsTicketsUpdateJson,
    ),
    write_generated(
        "tickets_delete",
        "tickets",
        GeneratedOperationId::TicketsTicketsDeleteJson,
    ),
    read_generated(
        "tickets_comments",
        "tickets",
        GeneratedOperationId::TicketsTicketsCommentsJson,
    ),
    write_generated(
        "tickets_comment_add",
        "tickets",
        GeneratedOperationId::TicketsTicketsCommentAddJson,
    ),
    write_generated(
        "tickets_comment_update",
        "tickets",
        GeneratedOperationId::TicketsTicketsCommentUpdateJson,
    ),
    write_generated(
        "tickets_comment_delete",
        "tickets",
        GeneratedOperationId::TicketsTicketsCommentDeleteJson,
    ),
    write_generated(
        "tickets_board_create",
        "tickets",
        GeneratedOperationId::TicketsBoardsCreateJson,
    ),
    write_generated(
        "tickets_board_update",
        "tickets",
        GeneratedOperationId::TicketsBoardsUpdateJson,
    ),
    write_generated(
        "tickets_board_delete",
        "tickets",
        GeneratedOperationId::TicketsBoardsDeleteJson,
    ),
    write_generated(
        "tickets_board_configure_columns",
        "tickets",
        GeneratedOperationId::TicketsBoardsConfigureColumnsJson,
    ),
    write_generated(
        "tickets_board_move_card",
        "tickets",
        GeneratedOperationId::TicketsBoardsMoveCardJson,
    ),
    write_generated(
        "tickets_relation_set",
        "tickets",
        GeneratedOperationId::TicketsTicketsRelationSetJson,
    ),
    write_generated(
        "tickets_relation_remove",
        "tickets",
        GeneratedOperationId::TicketsTicketsRelationRemoveJson,
    ),
    read_generated(
        "tickets_get",
        "tickets",
        GeneratedOperationId::TicketsTicketsGetJson,
    ),
    read_generated(
        "tickets_list",
        "tickets",
        GeneratedOperationId::TicketsTicketsListJson,
    ),
    read_generated(
        "tickets_board_get",
        "tickets",
        GeneratedOperationId::TicketsBoardsGetJson,
    ),
    read_generated(
        "tickets_board_list",
        "tickets",
        GeneratedOperationId::TicketsBoardsListJson,
    ),
    read_generated(
        "tickets_history",
        "tickets",
        GeneratedOperationId::TicketsTicketsHistoryJson,
    ),
    // lanes
    write_generated("lanes_create", "lanes", GeneratedOperationId::LanesCreate),
    read_generated("lanes_get", "lanes", GeneratedOperationId::LanesGet),
    read_generated("lanes_list", "lanes", GeneratedOperationId::LanesList),
    write_generated("lanes_update", "lanes", GeneratedOperationId::LanesUpdate),
    write_composite("lanes_closeout", "lanes", CompositeId::LanesCloseout),
    write_generated(
        "lanes_ticket_add",
        "lanes",
        GeneratedOperationId::LanesTicketAdd,
    ),
    write_generated(
        "lanes_ticket_remove",
        "lanes",
        GeneratedOperationId::LanesTicketRemove,
    ),
    write_generated(
        "lanes_ticket_transfer",
        "lanes",
        GeneratedOperationId::LanesTicketTransfer,
    ),
    write_generated("lanes_delete", "lanes", GeneratedOperationId::LanesDelete),
    write_composite("lanes_cleanup", "lanes", CompositeId::LanesCleanup),
    // spaces
    write_generated(
        "spaces_create",
        "spaces",
        GeneratedOperationId::PagesSpacesCreateJson,
    ),
    read_generated(
        "spaces_get",
        "spaces",
        GeneratedOperationId::PagesSpacesGetJson,
    ),
    read_generated(
        "spaces_list",
        "spaces",
        GeneratedOperationId::PagesSpacesListJson,
    ),
    // pages
    write_generated(
        "pages_create",
        "pages",
        GeneratedOperationId::PagesPagesCreateJson,
    ),
    write_generated(
        "pages_update",
        "pages",
        GeneratedOperationId::PagesPagesUpdateJson,
    ),
    write_generated(
        "pages_publish",
        "pages",
        GeneratedOperationId::PagesPagesPublishJson,
    ),
    read_generated(
        "pages_get",
        "pages",
        GeneratedOperationId::PagesPagesGetJson,
    ),
    read_generated(
        "pages_list",
        "pages",
        GeneratedOperationId::PagesPagesListJson,
    ),
    read_generated(
        "pages_history",
        "pages",
        GeneratedOperationId::PagesPagesHistoryJson,
    ),
    // lifecycles
    write_composite(
        "lifecycles_define",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesDefine,
    ),
    write_composite(
        "lifecycles_define_standard",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesDefineStandard,
    ),
    read_composite(
        "lifecycles_definitions",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesDefinitions,
    ),
    read_composite(
        "lifecycles_definition",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesDefinition,
    ),
    write_composite(
        "lifecycles_instantiate",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesInstantiate,
    ),
    read_composite(
        "lifecycles_instances",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesInstances,
    ),
    read_composite(
        "lifecycles_instance",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesInstance,
    ),
    write_composite(
        "lifecycles_active_set",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesActiveSet,
    ),
    write_composite(
        "lifecycles_active_clear",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesActiveClear,
    ),
    read_composite(
        "lifecycles_snapshot_plan",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesSnapshotPlan,
    ),
    read_composite(
        "lifecycles_current_surface",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesCurrentSurface,
    ),
    write_composite(
        "lifecycles_transition",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesTransition,
    ),
    read_composite(
        "lifecycles_snapshots",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesSnapshots,
    ),
    read_composite(
        "lifecycles_snapshot",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesSnapshot,
    ),
    read_composite(
        "lifecycles_snapshot_content",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesSnapshotContent,
    ),
    read_composite(
        "lifecycles_operation_log",
        "lifecycles",
        CompositeId::LifecycleSurfaceLifecyclesOperationLog,
    ),
    // chat
    read_generated(
        "chat_channels",
        "chat",
        GeneratedOperationId::ChatChatListChannelsJson,
    ),
    read_generated(
        "chat_fetch_events",
        "chat",
        GeneratedOperationId::ChatChatFetchEventsJson,
    ),
    read_generated(
        "chat_messages",
        "chat",
        GeneratedOperationId::ChatChatMessagesJson,
    ),
    read_generated(
        "chat_cursor",
        "chat",
        GeneratedOperationId::ChatChatCursorJson,
    ),
    read_composite("chat_presence", "chat", CompositeId::ChatPresence),
    write_generated(
        "chat_create_channel",
        "chat",
        GeneratedOperationId::ChatChatCreateChannelJson,
    ),
    write_generated(
        "chat_rename_channel",
        "chat",
        GeneratedOperationId::ChatChatRenameChannelJson,
    ),
    write_generated(
        "chat_post_message",
        "chat",
        GeneratedOperationId::ChatChatPostMessageJson,
    ),
    write_generated(
        "chat_post_message_bytes",
        "chat",
        GeneratedOperationId::ChatChatPostMessageBytesJson,
    ),
    write_generated(
        "chat_edit_message",
        "chat",
        GeneratedOperationId::ChatChatEditMessageJson,
    ),
    write_generated(
        "chat_edit_message_bytes",
        "chat",
        GeneratedOperationId::ChatChatEditMessageBytesJson,
    ),
    write_generated(
        "chat_redact_message",
        "chat",
        GeneratedOperationId::ChatChatRedactMessageJson,
    ),
    read_generated(
        "chat_emoji_list",
        "chat",
        GeneratedOperationId::ChatChatEmojiListJson,
    ),
    write_generated(
        "chat_emoji_register",
        "chat",
        GeneratedOperationId::ChatChatEmojiRegisterJson,
    ),
    write_generated(
        "chat_emoji_unregister",
        "chat",
        GeneratedOperationId::ChatChatEmojiUnregisterJson,
    ),
    write_generated(
        "chat_add_reaction",
        "chat",
        GeneratedOperationId::ChatChatAddReactionJson,
    ),
    write_generated(
        "chat_remove_reaction",
        "chat",
        GeneratedOperationId::ChatChatRemoveReactionJson,
    ),
    write_generated(
        "chat_create_thread",
        "chat",
        GeneratedOperationId::ChatChatCreateThreadJson,
    ),
    write_generated(
        "chat_create_task",
        "chat",
        GeneratedOperationId::ChatChatCreateTaskJson,
    ),
    write_generated(
        "chat_claim_task",
        "chat",
        GeneratedOperationId::ChatChatClaimTaskJson,
    ),
    write_generated(
        "chat_complete_task",
        "chat",
        GeneratedOperationId::ChatChatCompleteTaskJson,
    ),
    write_generated(
        "chat_invoke_agent",
        "chat",
        GeneratedOperationId::ChatChatInvokeAgentJson,
    ),
    write_generated(
        "chat_invoke_agent_bytes",
        "chat",
        GeneratedOperationId::ChatChatInvokeAgentBytesJson,
    ),
    write_generated(
        "chat_agent_reply",
        "chat",
        GeneratedOperationId::ChatChatAgentReplyJson,
    ),
    write_generated(
        "chat_request_handoff",
        "chat",
        GeneratedOperationId::ChatChatRequestHandoffJson,
    ),
    write_generated(
        "chat_update_cursor",
        "chat",
        GeneratedOperationId::ChatChatUpdateCursorJson,
    ),
    write_composite("chat_set_presence", "chat", CompositeId::ChatSetPresence),
    // drive
    read_generated(
        "drive_list",
        "drive",
        GeneratedOperationId::DriveDriveListJson,
    ),
    read_generated(
        "drive_stat",
        "drive",
        GeneratedOperationId::DriveDriveStatJson,
    ),
    read_generated(
        "drive_read",
        "drive",
        GeneratedOperationId::DriveDriveReadFile,
    ),
    read_generated(
        "drive_list_versions",
        "drive",
        GeneratedOperationId::DriveDriveListVersionsJson,
    ),
    read_generated(
        "drive_list_conflicts",
        "drive",
        GeneratedOperationId::DriveDriveListConflictsJson,
    ),
    write_generated(
        "drive_create_folder",
        "drive",
        GeneratedOperationId::DriveDriveCreateFolderJson,
    ),
    write_generated(
        "drive_create_upload",
        "drive",
        GeneratedOperationId::DriveDriveCreateUploadJson,
    ),
    write_generated(
        "drive_upload_chunk",
        "drive",
        GeneratedOperationId::DriveDriveUploadChunkJson,
    ),
    write_generated(
        "drive_commit_upload",
        "drive",
        GeneratedOperationId::DriveDriveCommitUploadJson,
    ),
    write_generated(
        "drive_rename",
        "drive",
        GeneratedOperationId::DriveDriveRenameJson,
    ),
    write_generated(
        "drive_move",
        "drive",
        GeneratedOperationId::DriveDriveMoveJson,
    ),
    write_generated(
        "drive_delete",
        "drive",
        GeneratedOperationId::DriveDriveDeleteJson,
    ),
    write_generated(
        "drive_resolve_conflict",
        "drive",
        GeneratedOperationId::DriveDriveResolveConflictJson,
    ),
    read_generated(
        "drive_list_shares",
        "drive",
        GeneratedOperationId::DriveDriveListSharesJson,
    ),
    write_generated(
        "drive_grant_share",
        "drive",
        GeneratedOperationId::DriveDriveGrantShareJson,
    ),
    write_generated(
        "drive_revoke_share",
        "drive",
        GeneratedOperationId::DriveDriveRevokeShareJson,
    ),
    write_generated(
        "drive_apply_share_expiry",
        "drive",
        GeneratedOperationId::DriveDriveApplyShareExpiryJson,
    ),
    read_generated(
        "drive_list_retention",
        "drive",
        GeneratedOperationId::DriveDriveListRetentionJson,
    ),
    write_generated(
        "drive_pin_retention",
        "drive",
        GeneratedOperationId::DriveDrivePinRetentionJson,
    ),
    write_generated(
        "drive_unpin_retention",
        "drive",
        GeneratedOperationId::DriveDriveUnpinRetentionJson,
    ),
    write_generated(
        "drive_apply_retention",
        "drive",
        GeneratedOperationId::DriveDriveApplyRetentionJson,
    ),
    write_composite(
        "drive_acquire_lease",
        "drive",
        CompositeId::DriveAcquireLease,
    ),
    write_composite(
        "drive_refresh_lease",
        "drive",
        CompositeId::DriveRefreshLease,
    ),
    write_composite(
        "drive_release_lease",
        "drive",
        CompositeId::DriveReleaseLease,
    ),
    write_composite("drive_break_lease", "drive", CompositeId::DriveBreakLease),
    // meetings
    read_composite(
        "meetings_list",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsList,
    ),
    read_composite(
        "meetings_get",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsGet,
    ),
    read_composite(
        "meetings_search",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsSearch,
    ),
    read_composite(
        "meetings_projection_outputs",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsProjectionOutputs,
    ),
    read_composite(
        "meetings_extraction_review",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsExtractionReview,
    ),
    write_composite(
        "meetings_accept_annotation",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsAcceptAnnotation,
    ),
    write_composite(
        "meetings_reject_annotation",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsRejectAnnotation,
    ),
    write_composite(
        "meetings_propose_vocabulary",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsProposeVocabulary,
    ),
    write_composite(
        "meetings_accept_vocabulary",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsAcceptVocabulary,
    ),
    write_composite(
        "meetings_reject_vocabulary",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsRejectVocabulary,
    ),
    write_composite(
        "meetings_add_entity_merge",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsAddEntityMerge,
    ),
    write_composite(
        "meetings_add_promotion",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsAddPromotion,
    ),
    write_composite(
        "meetings_promote_task_to_ticket",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsPromoteTaskToTicket,
    ),
    write_composite(
        "meetings_promote_decision_to_decision_log",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsPromoteDecisionToDecisionLog,
    ),
    write_composite(
        "meetings_promote_question_to_lifecycle",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsPromoteQuestionToLifecycle,
    ),
    write_composite(
        "meetings_promote_artifact_to_reference_artifact",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsPromoteArtifactToReferenceArtifact,
    ),
    write_composite(
        "meetings_promote_reference_to_reference_artifact",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsPromoteReferenceToReferenceArtifact,
    ),
    write_composite(
        "meetings_import_snapshot",
        "meetings",
        CompositeId::MeetingsSurfaceMeetingsImportSnapshot,
    ),
    // redmine
    write_owning_adapter(
        "redmine_import_snapshot",
        "redmine",
        AdapterId::InterchangeAdapterRedmineImportSnapshot,
    ),
    // studio
    write_composite(
        "studio_reindex",
        "studio",
        CompositeId::StudioSurfaceStudioReindex,
    ),
    // import
    write_owning_adapter(
        "import_submit_batch",
        "import",
        AdapterId::InterchangeAdapterImportSubmitBatch,
    ),
    write_owning_adapter(
        "import_execute_batch",
        "import",
        AdapterId::InterchangeAdapterImportExecuteBatch,
    ),
    // structures
    write_generated(
        "structures_create",
        "structures",
        GeneratedOperationId::PagesStructuresCreateJson,
    ),
    read_generated(
        "structures_get",
        "structures",
        GeneratedOperationId::PagesStructuresGetJson,
    ),
    read_generated(
        "structures_list",
        "structures",
        GeneratedOperationId::PagesStructuresListJson,
    ),
    write_generated(
        "structures_add_node",
        "structures",
        GeneratedOperationId::PagesStructuresAddNodeJson,
    ),
    write_generated(
        "structures_update_node",
        "structures",
        GeneratedOperationId::PagesStructuresUpdateNodeJson,
    ),
    write_generated(
        "structures_move_node",
        "structures",
        GeneratedOperationId::PagesStructuresMoveNodeJson,
    ),
    write_generated(
        "structures_link_node",
        "structures",
        GeneratedOperationId::PagesStructuresLinkNodeJson,
    ),
    write_generated(
        "structures_bind",
        "structures",
        GeneratedOperationId::PagesStructuresBindJson,
    ),
    write_generated(
        "structures_decompose_to_tickets",
        "structures",
        GeneratedOperationId::PagesStructuresDecomposeToTicketsJson,
    ),
    // kv
    write_generated("kv_put", "kv", GeneratedOperationId::KvPut),
    read_generated("kv_get", "kv", GeneratedOperationId::KvGet),
    write_generated("kv_delete", "kv", GeneratedOperationId::KvDelete),
    read_generated("kv_list", "kv", GeneratedOperationId::KvList),
    read_generated("kv_range", "kv", GeneratedOperationId::KvRange),
    read_generated(
        "kv_list_collections",
        "kv",
        GeneratedOperationId::KvListCollections,
    ),
    // document
    write_generated(
        "document_put_text",
        "document",
        GeneratedOperationId::DocumentPutText,
    ),
    read_generated(
        "document_get_text",
        "document",
        GeneratedOperationId::DocumentGetText,
    ),
    write_generated(
        "document_put_binary",
        "document",
        GeneratedOperationId::DocumentPutBinary,
    ),
    read_generated(
        "document_get_binary",
        "document",
        GeneratedOperationId::DocumentGetBinary,
    ),
    read_generated(
        "document_query",
        "document",
        GeneratedOperationId::DocumentQueryJson,
    ),
    write_composite(
        "document_replace_text",
        "document",
        CompositeId::DocumentReplaceText,
    ),
    write_generated(
        "document_delete",
        "document",
        GeneratedOperationId::DocumentDelete,
    ),
    write_generated(
        "document_delete_collection",
        "document",
        GeneratedOperationId::DocumentDeleteCollection,
    ),
    read_generated(
        "document_list_binary",
        "document",
        GeneratedOperationId::DocumentListBinary,
    ),
    read_generated(
        "document_list_collections",
        "document",
        GeneratedOperationId::DocumentListCollections,
    ),
    // timeseries
    write_generated(
        "timeseries_put",
        "timeseries",
        GeneratedOperationId::TimeSeriesPut,
    ),
    read_generated(
        "timeseries_get",
        "timeseries",
        GeneratedOperationId::TimeSeriesGet,
    ),
    read_generated(
        "timeseries_range",
        "timeseries",
        GeneratedOperationId::TimeSeriesRange,
    ),
    read_generated(
        "timeseries_latest",
        "timeseries",
        GeneratedOperationId::TimeSeriesLatest,
    ),
    read_generated(
        "timeseries_list_collections",
        "timeseries",
        GeneratedOperationId::TimeSeriesListCollections,
    ),
    // ledger
    write_generated(
        "ledger_append",
        "ledger",
        GeneratedOperationId::LedgerAppend,
    ),
    read_generated("ledger_get", "ledger", GeneratedOperationId::LedgerGet),
    read_generated("ledger_head", "ledger", GeneratedOperationId::LedgerHead),
    read_generated("ledger_len", "ledger", GeneratedOperationId::LedgerLen),
    read_generated(
        "ledger_verify",
        "ledger",
        GeneratedOperationId::LedgerVerify,
    ),
    read_generated(
        "ledger_list_collections",
        "ledger",
        GeneratedOperationId::LedgerListCollections,
    ),
    // queue
    write_generated("queue_append", "queue", GeneratedOperationId::QueueAppend),
    read_generated("queue_get", "queue", GeneratedOperationId::QueueGet),
    read_generated("queue_range", "queue", GeneratedOperationId::QueueRange),
    read_generated("queue_len", "queue", GeneratedOperationId::QueueLen),
    read_generated(
        "queue_list_streams",
        "queue",
        GeneratedOperationId::QueueListStreams,
    ),
    read_generated(
        "queue_consumer_position",
        "queue",
        GeneratedOperationId::QueueConsumersConsumerPosition,
    ),
    read_generated(
        "queue_consumer_read",
        "queue",
        GeneratedOperationId::QueueConsumersConsumerRead,
    ),
    write_generated(
        "queue_consumer_advance",
        "queue",
        GeneratedOperationId::QueueConsumersConsumerAdvance,
    ),
    write_generated(
        "queue_consumer_reset",
        "queue",
        GeneratedOperationId::QueueConsumersConsumerReset,
    ),
    // calendar
    write_generated(
        "calendar_create_collection",
        "calendar",
        GeneratedOperationId::CalendarCreateCollection,
    ),
    read_generated(
        "calendar_get_collection",
        "calendar",
        GeneratedOperationId::CalendarGetCollection,
    ),
    read_generated(
        "calendar_list_collections",
        "calendar",
        GeneratedOperationId::CalendarListCollections,
    ),
    write_generated(
        "calendar_delete_collection",
        "calendar",
        GeneratedOperationId::CalendarDeleteCollection,
    ),
    write_generated(
        "calendar_put_entry",
        "calendar",
        GeneratedOperationId::CalendarPutEntry,
    ),
    write_generated(
        "calendar_put_ics",
        "calendar",
        GeneratedOperationId::CalendarPutIcs,
    ),
    read_generated(
        "calendar_get_entry",
        "calendar",
        GeneratedOperationId::CalendarGetEntry,
    ),
    write_generated(
        "calendar_delete_entry",
        "calendar",
        GeneratedOperationId::CalendarDeleteEntry,
    ),
    read_generated(
        "calendar_list_entries",
        "calendar",
        GeneratedOperationId::CalendarListEntries,
    ),
    read_generated(
        "calendar_range",
        "calendar",
        GeneratedOperationId::CalendarRange,
    ),
    read_generated(
        "calendar_search",
        "calendar",
        GeneratedOperationId::CalendarSearch,
    ),
    read_generated(
        "calendar_to_ics",
        "calendar",
        GeneratedOperationId::CalendarToIcs,
    ),
    // contacts
    write_generated(
        "contacts_create_book",
        "contacts",
        GeneratedOperationId::ContactsCreateBook,
    ),
    read_generated(
        "contacts_get_book",
        "contacts",
        GeneratedOperationId::ContactsGetBook,
    ),
    read_generated(
        "contacts_list_books",
        "contacts",
        GeneratedOperationId::ContactsListBooks,
    ),
    write_generated(
        "contacts_delete_book",
        "contacts",
        GeneratedOperationId::ContactsDeleteBook,
    ),
    write_generated(
        "contacts_put_entry",
        "contacts",
        GeneratedOperationId::ContactsPutEntry,
    ),
    write_generated(
        "contacts_put_vcard",
        "contacts",
        GeneratedOperationId::ContactsPutVcard,
    ),
    read_generated(
        "contacts_get_entry",
        "contacts",
        GeneratedOperationId::ContactsGetEntry,
    ),
    write_generated(
        "contacts_delete_entry",
        "contacts",
        GeneratedOperationId::ContactsDeleteEntry,
    ),
    read_generated(
        "contacts_list_entries",
        "contacts",
        GeneratedOperationId::ContactsListEntries,
    ),
    read_generated(
        "contacts_search",
        "contacts",
        GeneratedOperationId::ContactsSearch,
    ),
    read_generated(
        "contacts_to_vcard",
        "contacts",
        GeneratedOperationId::ContactsToVcard,
    ),
    // mail
    write_generated(
        "mail_create_mailbox",
        "mail",
        GeneratedOperationId::MailCreateMailbox,
    ),
    read_generated(
        "mail_get_mailbox",
        "mail",
        GeneratedOperationId::MailGetMailbox,
    ),
    read_generated(
        "mail_list_mailboxes",
        "mail",
        GeneratedOperationId::MailListMailboxes,
    ),
    write_generated(
        "mail_delete_mailbox",
        "mail",
        GeneratedOperationId::MailDeleteMailbox,
    ),
    write_generated(
        "mail_ingest_message",
        "mail",
        GeneratedOperationId::MailIngestMessage,
    ),
    read_generated(
        "mail_get_message",
        "mail",
        GeneratedOperationId::MailGetMessage,
    ),
    read_generated("mail_to_eml", "mail", GeneratedOperationId::MailToEml),
    write_generated(
        "mail_delete_message",
        "mail",
        GeneratedOperationId::MailDeleteMessage,
    ),
    read_generated(
        "mail_list_messages",
        "mail",
        GeneratedOperationId::MailListMessages,
    ),
    read_generated("mail_get_flags", "mail", GeneratedOperationId::MailGetFlags),
    write_generated("mail_set_flags", "mail", GeneratedOperationId::MailSetFlags),
    read_generated("mail_search", "mail", GeneratedOperationId::MailSearch),
    // sql
    write_generated("sql_exec", "sql", GeneratedOperationId::SqlSqlExec),
    write_generated(
        "sql_exec_result",
        "sql",
        GeneratedOperationId::SqlSqlExecResult,
    ),
    read_generated("sql_query", "sql", GeneratedOperationId::SqlSqlQuery),
    write_generated("sql_commit", "sql", GeneratedOperationId::SqlSqlCommit),
    read_generated(
        "sql_read_table",
        "sql",
        GeneratedOperationId::SqlSqlReadTable,
    ),
    read_generated(
        "sql_read_table_at",
        "sql",
        GeneratedOperationId::SqlSqlReadTableAt,
    ),
    read_generated(
        "sql_index_scan",
        "sql",
        GeneratedOperationId::SqlSqlIndexScan,
    ),
    read_generated(
        "sql_index_scan_at",
        "sql",
        GeneratedOperationId::SqlSqlIndexScanAt,
    ),
    read_generated("sql_diff", "sql", GeneratedOperationId::SqlSqlDiff),
    read_generated(
        "sql_table_diff",
        "sql",
        GeneratedOperationId::SqlSqlTableDiff,
    ),
    read_generated("sql_blame", "sql", GeneratedOperationId::SqlSqlBlame),
    read_generated(
        "sql_list_databases",
        "sql",
        GeneratedOperationId::SqlSqlListDatabases,
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
        "Tickets",
        &[
            // The MCP surface exposes this through the composite `tickets_projects` tool.
            "tickets_projects_json",
        ],
    ),
    (
        "Lanes",
        &[
            // The view helpers are represented by `lanes_get` and `lanes_list`, which return the
            // persisted lane view shape directly.
            "get_view_json",
            "list_views_json",
            // The generated local helpers back the composite MCP tools with ticket-aware behavior.
            "closeout",
            "cleanup_json",
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
    pub fn idl_projection(&self) -> Option<(&'static str, &'static str)> {
        match self.target {
            ExecutionTarget::Generated(operation) => Some(operation.projection()),
            ExecutionTarget::Composite(_) | ExecutionTarget::OwningAdapter(_) => None,
        }
    }

    pub fn remote_capability(&self) -> RemoteCapability {
        match self.target {
            ExecutionTarget::Generated(operation)
                if {
                    let (idl_interface, idl_method) = operation.projection();
                    HANDLE_STREAM_METHODS.contains(&(idl_interface, idl_method))
                } =>
            {
                RemoteCapability::HandleStream
            }
            ExecutionTarget::Generated(_) => RemoteCapability::Unary,
            ExecutionTarget::Composite(_) | ExecutionTarget::OwningAdapter(_) => {
                RemoteCapability::ServerExecute
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RemoteToolRoute {
    UnaryForward,
    ServerExecute,
    Reject(String),
}

pub fn remote_tool_route(name: &str) -> RemoteToolRoute {
    remote_tool_route_for(name, tool(name).map(ToolSpec::remote_capability))
}

pub fn remote_tool_route_for(name: &str, capability: Option<RemoteCapability>) -> RemoteToolRoute {
    match capability {
        Some(RemoteCapability::Unary) => RemoteToolRoute::UnaryForward,
        Some(RemoteCapability::ServerExecute) => RemoteToolRoute::ServerExecute,
        Some(RemoteCapability::HandleStream) => RemoteToolRoute::Reject(format!(
            "MCP tool {name} uses a handle/stream interface that is not supported against a remote Loom store"
        )),
        None => RemoteToolRoute::Reject(format!(
            "MCP tool {name} is not in the MCP execution-boundary manifest"
        )),
    }
}

pub fn generated_operation_signature(
    operation: GeneratedOperationId,
) -> Option<&'static MethodSig> {
    METHODS.iter().find(|sig| sig.operation == operation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    const IDL: &str = include_str!("../../../idl/loom.idl");
    const SPEC: &str = include_str!("../../../specs/0008-wire-protocols.md");

    #[derive(Clone, Copy)]
    struct GeneratedClassificationExpectation {
        owner: &'static str,
        operation: GeneratedOperationId,
        interface: &'static str,
        method: &'static str,
        args: &'static [(&'static str, &'static str)],
        ret: &'static str,
    }

    fn generated_expectation_for_tool(
        tool: &ToolSpec,
    ) -> Option<GeneratedClassificationExpectation> {
        let ExecutionTarget::Generated(operation) = tool.target else {
            return None;
        };
        let sig =
            generated_operation_signature(operation).expect("generated operation has signature");
        Some(GeneratedClassificationExpectation {
            owner: tool.name,
            operation,
            interface: sig.interface,
            method: sig.method,
            args: sig.args,
            ret: sig.ret,
        })
    }

    fn validate_generated_classifications(
        records: &[GeneratedClassificationExpectation],
    ) -> Result<(), String> {
        validate_generated_classifications_with_shared(
            records,
            SHARED_GENERATED_OPERATION_OWNERSHIP,
        )
    }

    fn validate_generated_classifications_with_shared(
        records: &[GeneratedClassificationExpectation],
        shared_ownership: &[(GeneratedOperationId, &[&str], &str)],
    ) -> Result<(), String> {
        let mut owners = BTreeSet::new();
        let mut operations: BTreeMap<GeneratedOperationId, Vec<&str>> = BTreeMap::new();
        for record in records {
            if !owners.insert(record.owner) {
                return Err(format!(
                    "duplicate generated classification owner {}",
                    record.owner
                ));
            }
            let (projected_interface, projected_method) = record.operation.projection();
            if projected_interface != record.interface || projected_method != record.method {
                return Err(format!(
                    "{} declares stale operation identity {}.{} for {:?}, canonical is {}.{}",
                    record.owner,
                    record.interface,
                    record.method,
                    record.operation,
                    projected_interface,
                    projected_method
                ));
            }
            let sig = generated_operation_signature(record.operation).ok_or_else(|| {
                format!(
                    "{} references {:?}, which has no generated MethodSig",
                    record.owner, record.operation
                )
            })?;
            if sig.args != record.args {
                return Err(format!(
                    "{} declares stale argument shape for {}.{}",
                    record.owner, sig.interface, sig.method
                ));
            }
            if sig.ret != record.ret {
                return Err(format!(
                    "{} declares stale return shape for {}.{}",
                    record.owner, sig.interface, sig.method
                ));
            }
            operations
                .entry(record.operation)
                .or_default()
                .push(record.owner);
        }
        let mut observed_shared = BTreeMap::new();
        for (operation, mut observed) in operations {
            if observed.len() <= 1 {
                continue;
            }
            observed.sort_unstable();
            observed_shared.insert(operation, observed.clone());
            let Some((declared, reason)) =
                shared_ownership
                    .iter()
                    .find_map(|(shared, owners, reason)| {
                        (*shared == operation).then_some((*owners, *reason))
                    })
            else {
                return Err(format!(
                    "{operation:?} has duplicate generated classification owners {observed:?}"
                ));
            };
            let mut declared = declared.to_vec();
            declared.sort_unstable();
            if declared != observed {
                return Err(format!(
                    "{operation:?} shared ownership declares {declared:?} but observed {observed:?}"
                ));
            }
            if reason.trim().is_empty() {
                return Err(format!("{operation:?} shared ownership reason is empty"));
            }
        }
        let mut declared_operations = BTreeSet::new();
        for (operation, declared, reason) in shared_ownership {
            if !declared_operations.insert(*operation) {
                return Err(format!(
                    "{operation:?} has duplicate shared ownership declarations"
                ));
            }
            if reason.trim().is_empty() {
                return Err(format!("{operation:?} shared ownership reason is empty"));
            }
            let mut declared = declared.to_vec();
            declared.sort_unstable();
            match observed_shared.get(operation) {
                Some(observed) if *observed == declared => {}
                Some(observed) => {
                    return Err(format!(
                        "{operation:?} shared ownership declares {declared:?} but observed {observed:?}"
                    ));
                }
                None => {
                    return Err(format!(
                        "{operation:?} shared ownership declaration is stale"
                    ));
                }
            }
        }
        Ok(())
    }

    struct ExceptionExpectation {
        owner: &'static str,
        reason: &'static str,
    }

    fn validate_exception_records(records: &[ExceptionExpectation]) -> Result<(), String> {
        let mut owners = BTreeSet::new();
        for record in records {
            if !owners.insert(record.owner) {
                return Err(format!("duplicate exception owner {}", record.owner));
            }
            if record.reason.trim().is_empty() {
                return Err(format!("{} has an implicit exception", record.owner));
            }
        }
        Ok(())
    }

    fn validate_exception_reasons(surface: &[ToolSpec]) -> Result<(), String> {
        let records: Vec<_> = surface
            .iter()
            .filter_map(|tool| match tool.target {
                ExecutionTarget::Generated(_) => None,
                ExecutionTarget::Composite(composite_id) => Some(ExceptionExpectation {
                    owner: tool.name,
                    reason: composite_id.reason(),
                }),
                ExecutionTarget::OwningAdapter(adapter_id) => Some(ExceptionExpectation {
                    owner: tool.name,
                    reason: adapter_id.reason(),
                }),
            })
            .collect();
        validate_exception_records(&records)
    }

    #[test]
    fn remote_capability_partitions_the_surface() {
        let mut unary = 0usize;
        let mut handle_stream = 0usize;
        let mut server_execute = 0usize;
        for tool in tool_surface() {
            match tool.remote_capability() {
                RemoteCapability::Unary => unary += 1,
                RemoteCapability::HandleStream => handle_stream += 1,
                RemoteCapability::ServerExecute => server_execute += 1,
            }
        }
        let derived_handle_stream = tool_surface()
            .iter()
            .filter(|t| {
                t.idl_projection().is_some_and(|(interface, method)| {
                    HANDLE_STREAM_METHODS.contains(&(interface, method))
                })
            })
            .count();
        let derived_server_execute = tool_surface()
            .iter()
            .filter(|t| {
                matches!(
                    t.target,
                    ExecutionTarget::Composite(_) | ExecutionTarget::OwningAdapter(_)
                )
            })
            .count();
        let derived_unary = tool_surface().len() - derived_handle_stream - derived_server_execute;
        assert_eq!(
            handle_stream, derived_handle_stream,
            "handle/stream count drift"
        );
        assert_eq!(
            server_execute, derived_server_execute,
            "server-execute count drift"
        );
        assert_eq!(unary, derived_unary, "unary count drift");
        assert_eq!(unary + handle_stream + server_execute, tool_surface().len());

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
                "{sql_tool} should classify Unary"
            );
        }
    }

    #[test]
    fn execution_targets_use_closed_identifiers() {
        let mut generated = BTreeSet::new();
        let mut composite = BTreeSet::new();
        let mut adapters = BTreeSet::new();
        for tool in tool_surface() {
            match tool.target {
                ExecutionTarget::Generated(operation) => {
                    generated.insert(operation);
                }
                ExecutionTarget::Composite(composite_id) => {
                    composite.insert(composite_id);
                }
                ExecutionTarget::OwningAdapter(adapter_id) => {
                    adapters.insert(adapter_id);
                }
            }
        }
        assert_eq!(generated.len(), 306);
        assert_eq!(composite.len(), 78);
        assert_eq!(adapters.len(), 3);
        assert!(generated.contains(&GeneratedOperationId::StoreAdminStorePolicyGet));
        assert!(generated.contains(&GeneratedOperationId::StoreAdminStorePolicySet));
        assert!(composite.contains(&CompositeId::ChatSetPresence));
        assert!(adapters.contains(&AdapterId::InterchangeAdapterImportExecuteBatch));
    }

    #[test]
    fn generated_tool_surface_entries_resolve_to_single_method_signature_once() {
        for tool in TOOL_SURFACE {
            let ExecutionTarget::Generated(operation) = tool.target else {
                continue;
            };
            let matches: Vec<_> = METHODS
                .iter()
                .filter(|sig| sig.operation == operation)
                .collect();
            assert_eq!(
                matches.len(),
                1,
                "{} must resolve to exactly one generated method signature record",
                tool.name
            );
            let sig = generated_operation_signature(operation)
                .unwrap_or_else(|| panic!("{} references absent generated operation", tool.name));
            let (projected_interface, projected_method) = operation.projection();
            assert_eq!(sig.interface, projected_interface);
            assert_eq!(sig.method, projected_method);
            let expected_args_without_handle = sig
                .args
                .strip_prefix(&[("LoomSession", "handle")])
                .unwrap_or(sig.args);
            assert_eq!(
                sig.args_without_handle, expected_args_without_handle,
                "{} generated method signature must preserve handle-stripped argument order",
                tool.name
            );
            assert!(
                !sig.request_json_schema.is_empty(),
                "{} must carry an IDL-derived request JSON Schema",
                tool.name
            );
            assert!(
                !sig.response_json_schema.is_empty(),
                "{} must carry an IDL-derived response JSON Schema",
                tool.name
            );
        }
    }

    #[test]
    fn generated_methods_have_one_record_per_generated_operation_id() {
        let mut operations = BTreeSet::new();
        for sig in METHODS {
            assert!(
                operations.insert(sig.operation),
                "duplicate generated method signature for {:?}",
                sig.operation
            );
        }
        assert_eq!(operations.len(), METHODS.len());
    }

    #[test]
    fn generated_method_json_schemas_are_idl_derived_and_complete() {
        for sig in METHODS {
            let request: serde_json::Value = serde_json::from_str(sig.request_json_schema)
                .unwrap_or_else(|err| {
                    panic!("{} request schema is invalid JSON: {err}", sig.method)
                });
            let response: serde_json::Value = serde_json::from_str(sig.response_json_schema)
                .unwrap_or_else(|err| {
                    panic!("{} response schema is invalid JSON: {err}", sig.method)
                });
            assert_eq!(
                request.get("$schema").and_then(serde_json::Value::as_str),
                Some("https://json-schema.org/draft/2020-12/schema"),
                "{} request schema missing dialect",
                sig.method
            );
            assert_eq!(
                response.get("$schema").and_then(serde_json::Value::as_str),
                Some("https://json-schema.org/draft/2020-12/schema"),
                "{} response schema missing dialect",
                sig.method
            );
        }
    }

    #[test]
    fn generated_nested_json_schema_covers_records_enums_optionals_lists_and_bytes() {
        let workspace_list =
            generated_operation_signature(GeneratedOperationId::WorkspacesWorkspaceList)
                .expect("workspace_list signature");
        let response: serde_json::Value =
            serde_json::from_str(workspace_list.response_json_schema).expect("workspace schema");
        assert_eq!(
            response.get("type").and_then(serde_json::Value::as_str),
            Some("array")
        );
        assert_eq!(
            response
                .pointer("/items/$ref")
                .and_then(serde_json::Value::as_str),
            Some("#/$defs/Workspace")
        );
        let workspace = response
            .pointer("/$defs/Workspace")
            .expect("Workspace definition");
        assert_eq!(
            workspace
                .pointer("/properties/id/type")
                .and_then(serde_json::Value::as_str),
            Some("string")
        );
        assert_eq!(
            workspace
                .pointer("/properties/id/format")
                .and_then(serde_json::Value::as_str),
            Some("uuid")
        );
        assert_eq!(
            workspace
                .pointer("/properties/facets/type")
                .and_then(serde_json::Value::as_str),
            Some("array")
        );
        assert_eq!(
            workspace
                .pointer("/properties/facets/items/$ref")
                .and_then(serde_json::Value::as_str),
            Some("#/$defs/FacetKind")
        );
        assert!(
            workspace
                .pointer("/properties/head/anyOf")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|items| items
                    .iter()
                    .any(
                        |item| item.get("type").and_then(serde_json::Value::as_str) == Some("null")
                    )),
            "optional Digest head must preserve nullability"
        );
        assert!(
            response
                .pointer("/$defs/FacetKind/enum")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|values| values.iter().any(|value| value.as_str() == Some("FILES"))),
            "FacetKind enum values must be schema-visible"
        );

        let blob_digest = generated_operation_signature(GeneratedOperationId::StoreBlobDigest)
            .expect("blob_digest signature");
        let request: serde_json::Value =
            serde_json::from_str(blob_digest.request_json_schema).expect("blob_digest schema");
        assert_eq!(
            request
                .pointer("/properties/data/contentEncoding")
                .and_then(serde_json::Value::as_str),
            Some("base64")
        );
        assert_eq!(
            request
                .pointer("/properties/data/x-loom-bytes")
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );

        let policy_get =
            generated_operation_signature(GeneratedOperationId::StoreAdminStorePolicySet)
                .expect("store_policy_set signature");
        let request: serde_json::Value =
            serde_json::from_str(policy_get.request_json_schema).expect("policy request schema");
        assert!(
            request.pointer("/properties/handle").is_some(),
            "IDL request schema must retain the full generated handle argument"
        );
        assert_eq!(
            policy_get.args_without_handle,
            &[("bool", "fips_required")],
            "MCP presentation transforms stay outside the IDL schema authority"
        );
    }

    #[test]
    fn non_generated_tool_surface_entries_remain_typed_exclusions() {
        let mut composite = 0usize;
        let mut adapters = 0usize;
        for tool in TOOL_SURFACE {
            match tool.target {
                ExecutionTarget::Generated(_) => {}
                ExecutionTarget::Composite(composite_id) => {
                    composite += 1;
                    assert!(!composite_id.as_str().is_empty());
                    assert!(!composite_id.reason().is_empty());
                }
                ExecutionTarget::OwningAdapter(adapter_id) => {
                    adapters += 1;
                    assert!(!adapter_id.as_str().is_empty());
                    assert!(!adapter_id.reason().is_empty());
                }
            }
        }
        assert_eq!(composite, 78);
        assert_eq!(adapters, 3);
    }

    #[test]
    fn generated_classifications_match_canonical_method_signatures() {
        let records: Vec<_> = TOOL_SURFACE
            .iter()
            .filter_map(generated_expectation_for_tool)
            .collect();
        validate_generated_classifications(&records)
            .expect("generated classifications must match canonical generated method signatures");
    }

    #[test]
    fn generated_classification_gate_rejects_stale_identity() {
        let record =
            generated_expectation_for_tool(tool("lanes_create").expect("lanes_create tool"))
                .expect("generated record");
        let stale = [GeneratedClassificationExpectation {
            interface: "Lanes",
            method: "not_create",
            ..record
        }];
        let err = validate_generated_classifications(&stale).expect_err("stale identity rejected");
        assert!(err.contains("stale operation identity"), "{err}");
    }

    #[test]
    fn generated_classification_gate_rejects_stale_argument_order() {
        let record =
            generated_expectation_for_tool(tool("lanes_create").expect("lanes_create tool"))
                .expect("generated record");
        const STALE_ARGS: &[(&str, &str)] = &[("string", "lane"), ("LoomSession", "handle")];
        let stale = [GeneratedClassificationExpectation {
            args: STALE_ARGS,
            ..record
        }];
        let err = validate_generated_classifications(&stale).expect_err("stale args rejected");
        assert!(err.contains("stale argument shape"), "{err}");
    }

    #[test]
    fn generated_classification_gate_rejects_stale_return_type() {
        let record =
            generated_expectation_for_tool(tool("lanes_create").expect("lanes_create tool"))
                .expect("generated record");
        let stale = [GeneratedClassificationExpectation {
            ret: "void",
            ..record
        }];
        let err = validate_generated_classifications(&stale).expect_err("stale return rejected");
        assert!(err.contains("stale return shape"), "{err}");
    }

    #[test]
    fn generated_classification_gate_rejects_duplicate_owners_without_explicit_sharing() {
        let record = generated_expectation_for_tool(tool("lanes_get").expect("lanes_get tool"))
            .expect("generated record");
        let records = [
            record,
            GeneratedClassificationExpectation {
                owner: "lanes_get_duplicate",
                ..record
            },
        ];
        let err = validate_generated_classifications(&records).expect_err("duplicate rejected");
        assert!(
            err.contains("duplicate generated classification owners"),
            "{err}"
        );
    }

    #[test]
    fn generated_classification_gate_accepts_declared_shared_ownership() {
        let records: Vec<_> = ["store_capabilities", "store_capabilities_json"]
            .into_iter()
            .map(|name| {
                generated_expectation_for_tool(tool(name).expect("shared tool"))
                    .expect("generated record")
            })
            .collect();
        validate_generated_classifications(&records)
            .expect("declared shared operation ownership is accepted");
    }

    #[test]
    fn generated_classification_gate_rejects_stale_shared_ownership_declaration() {
        let record = generated_expectation_for_tool(tool("lanes_get").expect("lanes_get tool"))
            .expect("generated record");
        let stale_shared = [(
            GeneratedOperationId::LanesGet,
            &["lanes_get", "lanes_get_duplicate"][..],
            "stale declaration for a duplicate that is not present",
        )];
        let err = validate_generated_classifications_with_shared(&[record], &stale_shared)
            .expect_err("stale shared ownership rejected");
        assert!(
            err.contains("shared ownership declaration is stale"),
            "{err}"
        );
    }

    #[test]
    fn composite_and_adapter_exceptions_are_explicit() {
        validate_exception_reasons(TOOL_SURFACE)
            .expect("composite and owning-adapter exceptions must have source-declared reasons");
    }

    #[test]
    fn exception_gate_rejects_implicit_composite_exception() {
        let implicit = [ExceptionExpectation {
            owner: "implicit_exception",
            reason: "",
        }];
        let err = validate_exception_records(&implicit).expect_err("implicit exception rejected");
        assert!(err.contains("implicit exception"), "{err}");
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

        // Every generated target's interface and method must exist in the IDL.
        for spec in TOOL_SURFACE {
            let Some((interface, method)) = spec.idl_projection() else {
                continue;
            };
            let methods = idl
                .get(interface)
                .unwrap_or_else(|| panic!("tool {} names unknown interface", spec.name));
            assert!(
                methods.contains(method),
                "tool {} projects {}.{}, absent from the IDL",
                spec.name,
                interface,
                method
            );
        }

        // Per generated interface: idl methods minus excluded == the projected methods.
        let mut projected_ifaces = BTreeSet::new();
        for spec in TOOL_SURFACE {
            if let Some((interface, _)) = spec.idl_projection() {
                projected_ifaces.insert(interface);
            }
        }
        for iface in projected_ifaces {
            let idl_methods = &idl[iface];
            let projected: BTreeSet<&str> = TOOL_SURFACE
                .iter()
                .filter_map(|t| t.idl_projection())
                .filter_map(|(interface, method)| (interface == iface).then_some(method))
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
                !TOOL_SURFACE
                    .iter()
                    .filter_map(ToolSpec::idl_projection)
                    .any(|(interface, _)| interface == *iface),
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
