use loom_core::error::{Code, LoomError, Result};
use loom_core::workspace::{FacetKind, WorkspaceId, facet_path};
use loom_core::{
    AclDomain, AclRight, Algo, Digest, FireOutcome, FireRecord, Loom, Object, ObjectStore,
    TriggerStimulus, stimulus_digest, trigger_append_fire_record_system, trigger_history_system,
};
use loom_store::FileStore;
use loom_substrate::lifecycle::{
    APP_ID, GateEvaluation, LifecycleDefinition, LifecycleInstance, LifecycleOperationLog,
    LifecycleOperationRecord, LifecycleTransitionInput, LifecycleTransitionRecord, SnapshotPlan,
    SnapshotPolicy, SnapshotRecord, StageSurface, StandardLifecycleInput, StandardLifecycleKind,
    lifecycle_operation_log_key, standard_lifecycle_definition,
};
use loom_substrate::versioning::{
    BodyRef, ProfileRevisionUpdate, ProfileTransaction, ProfileTransactionState,
    REVISION_INDEX_DIR, RevisionIndex, revision_index_path,
};
use loom_substrate::{ActorKind, OperationEnvelope, OperationEnvelopeInput};
use serde::Serialize;

const CONTROL_PREFIX: &str = "profile/lifecycle/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StandardLifecycleRequest<'a> {
    pub workspace_id: &'a str,
    pub kind: &'a str,
    pub version: &'a str,
    pub completion_predicate_digest: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleGateEvaluationInput {
    pub gate_id: String,
    pub passed: bool,
    pub principal_id: Option<String>,
    pub evidence_digest: Option<String>,
    pub evaluated_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleTransitionRequest<'a> {
    pub workspace_id: &'a str,
    pub instance_id: &'a str,
    pub transition_id: &'a str,
    pub to_stage_id: &'a str,
    pub actor_principal_id: &'a str,
    pub gate_evaluations: Vec<LifecycleGateEvaluationInput>,
    pub snapshot_digest: Option<&'a str>,
    pub recorded_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifecycleGateSummary {
    pub gate_id: String,
    pub label: String,
    pub kind: String,
    pub predicate_digest: Option<String>,
    pub required_role: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifecycleStageSummary {
    pub stage_id: String,
    pub label: String,
    pub entry_gates: Vec<LifecycleGateSummary>,
    pub exit_gates: Vec<LifecycleGateSummary>,
    pub snapshot_policy: String,
    pub surfaced_tools: Vec<String>,
    pub prompt_refs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifecycleDefinitionSummary {
    pub workspace_id: String,
    pub definition_id: String,
    pub version: String,
    pub initial_stage_id: String,
    pub stages: Vec<LifecycleStageSummary>,
    pub definition_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifecycleGateEvaluationSummary {
    pub gate_id: String,
    pub passed: bool,
    pub principal_id: Option<String>,
    pub evidence_digest: Option<String>,
    pub evaluated_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifecycleTransitionSummary {
    pub transition_id: String,
    pub instance_id: String,
    pub definition_id: String,
    pub definition_version: String,
    pub from_stage_id: String,
    pub to_stage_id: String,
    pub actor_principal_id: String,
    pub gate_evaluations: Vec<LifecycleGateEvaluationSummary>,
    pub snapshot_digest: Option<String>,
    pub recorded_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifecycleInstanceSummary {
    pub workspace_id: String,
    pub instance_id: String,
    pub definition_id: String,
    pub definition_version: String,
    pub subject_refs: Vec<String>,
    pub current_stage_id: String,
    pub stage_history: Vec<LifecycleTransitionSummary>,
    pub instance_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifecycleStageSurfaceSummary {
    pub workspace_id: String,
    pub instance_id: String,
    pub stage_id: String,
    pub surfaced_tools: Vec<String>,
    pub prompt_refs: Vec<String>,
    pub read_only: bool,
    pub surface_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifecycleSnapshotPlanSummary {
    pub workspace_id: String,
    pub instance_id: String,
    pub from_stage_id: String,
    pub to_stage_id: String,
    pub required: bool,
    pub subject_refs: Vec<String>,
    pub policy: String,
    pub plan_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifecycleSnapshotRecordSummary {
    pub workspace_id: String,
    pub snapshot_id: String,
    pub instance_id: String,
    pub transition_id: String,
    pub from_stage_id: String,
    pub to_stage_id: String,
    pub subject_refs: Vec<String>,
    pub policy: String,
    pub snapshot_digest: String,
    pub recorded_at_ms: u64,
    pub snapshot_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifecycleOperationSummary {
    pub sequence: u64,
    pub operation_id: String,
    pub operation_kind: String,
    pub instance_id: String,
    pub target_entity_id: Option<String>,
    pub root_after: String,
    pub envelope_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifecycleOperationLogSummary {
    pub workspace_id: String,
    pub records: Vec<LifecycleOperationSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifecycleTransitionResult {
    pub instance: LifecycleInstanceSummary,
    pub transition: LifecycleTransitionSummary,
    pub surface: LifecycleStageSurfaceSummary,
    pub snapshot: Option<LifecycleSnapshotRecordSummary>,
    pub operation_log: LifecycleOperationLogSummary,
}

pub fn define_lifecycle(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    definition_cbor: &[u8],
) -> Result<LifecycleDefinitionSummary> {
    loom.authorize_domain(workspace, AclDomain::Lifecycle, AclRight::Write)?;
    let definition = LifecycleDefinition::decode(definition_cbor)?;
    save_definition(loom.store(), workspace_id, &definition)?;
    recompute_surfaces_for_definition(loom.store(), workspace_id, &definition)?;
    record_lifecycle_revision(
        loom,
        workspace,
        workspace_id,
        &format!("lifecycle:definition:{}", definition.definition_id),
        &format!(
            "lifecycle.definition.define:{workspace_id}:{}",
            definition.definition_id
        ),
        &definition.encode()?,
        "application/vnd.uldren.loom.lifecycle.definition+cbor",
        now_ms(),
    )?;
    definition_summary(workspace_id, &definition)
}

pub fn define_standard_lifecycle(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: StandardLifecycleRequest<'_>,
) -> Result<LifecycleDefinitionSummary> {
    loom.authorize_domain(workspace, AclDomain::Lifecycle, AclRight::Write)?;
    let definition = standard_lifecycle_definition(StandardLifecycleInput {
        kind: standard_kind(request.kind)?,
        version: request.version.to_string(),
        completion_predicate_digest: Digest::parse(request.completion_predicate_digest)?,
    })?;
    save_definition(loom.store(), request.workspace_id, &definition)?;
    recompute_surfaces_for_definition(loom.store(), request.workspace_id, &definition)?;
    record_lifecycle_revision(
        loom,
        workspace,
        request.workspace_id,
        &format!("lifecycle:definition:{}", definition.definition_id),
        &format!(
            "lifecycle.definition.define:{}:{}",
            request.workspace_id, definition.definition_id
        ),
        &definition.encode()?,
        "application/vnd.uldren.loom.lifecycle.definition+cbor",
        now_ms(),
    )?;
    definition_summary(request.workspace_id, &definition)
}

pub fn get_definition(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    definition_id: &str,
) -> Result<Option<LifecycleDefinitionSummary>> {
    loom.authorize_domain(workspace, AclDomain::Lifecycle, AclRight::Read)?;
    load_definition(loom.store(), workspace_id, definition_id)?
        .as_ref()
        .map(|definition| definition_summary(workspace_id, definition))
        .transpose()
}

pub fn list_definitions(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<LifecycleDefinitionSummary>> {
    loom.authorize_domain(workspace, AclDomain::Lifecycle, AclRight::Read)?;
    let mut out = loom
        .store()
        .control_scan_prefix(&definition_prefix(workspace_id)?)?
        .into_iter()
        .map(|(_, bytes)| LifecycleDefinition::decode(&bytes))
        .collect::<Result<Vec<_>>>()?;
    out.sort_by(|a, b| a.definition_id.cmp(&b.definition_id));
    out.iter()
        .map(|definition| definition_summary(workspace_id, definition))
        .collect()
}

pub fn instantiate(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    instance_id: &str,
    definition_id: &str,
    subject_refs: Vec<String>,
) -> Result<LifecycleInstanceSummary> {
    loom.authorize_domain(workspace, AclDomain::Lifecycle, AclRight::Write)?;
    if load_instance(loom.store(), workspace_id, instance_id)?.is_some() {
        return Err(LoomError::new(
            Code::AlreadyExists,
            "lifecycle instance already exists",
        ));
    }
    let definition = load_definition(loom.store(), workspace_id, definition_id)?
        .ok_or_else(|| LoomError::not_found("lifecycle definition not found"))?;
    let instance = LifecycleInstance::new(instance_id, &definition, subject_refs)?;
    save_instance(loom.store(), workspace_id, &instance)?;
    let surface = StageSurface::for_instance(&definition, &instance)?;
    save_surface(loom.store(), workspace_id, &surface)?;
    record_lifecycle_revision(
        loom,
        workspace,
        workspace_id,
        &format!("lifecycle:instance:{}", instance.instance_id),
        &format!(
            "lifecycle.instance.instantiate:{workspace_id}:{}",
            instance.instance_id
        ),
        &instance.encode()?,
        "application/vnd.uldren.loom.lifecycle.instance+cbor",
        now_ms(),
    )?;
    instance_summary(workspace_id, &instance)
}

pub fn get_instance(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    instance_id: &str,
) -> Result<Option<LifecycleInstanceSummary>> {
    loom.authorize_domain(workspace, AclDomain::Lifecycle, AclRight::Read)?;
    load_instance(loom.store(), workspace_id, instance_id)?
        .as_ref()
        .map(|instance| instance_summary(workspace_id, instance))
        .transpose()
}

pub fn list_instances(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<LifecycleInstanceSummary>> {
    loom.authorize_domain(workspace, AclDomain::Lifecycle, AclRight::Read)?;
    let mut out = loom
        .store()
        .control_scan_prefix(&instance_prefix(workspace_id)?)?
        .into_iter()
        .map(|(_, bytes)| LifecycleInstance::decode(&bytes))
        .collect::<Result<Vec<_>>>()?;
    out.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
    out.iter()
        .map(|instance| instance_summary(workspace_id, instance))
        .collect()
}

pub fn snapshot_plan(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    instance_id: &str,
    to_stage_id: &str,
) -> Result<LifecycleSnapshotPlanSummary> {
    loom.authorize_domain(workspace, AclDomain::Lifecycle, AclRight::Read)?;
    let instance = load_instance(loom.store(), workspace_id, instance_id)?
        .ok_or_else(|| LoomError::not_found("lifecycle instance not found"))?;
    let definition = load_definition(loom.store(), workspace_id, &instance.definition_id)?
        .ok_or_else(|| LoomError::not_found("lifecycle definition not found"))?;
    let plan = SnapshotPlan::for_transition(&definition, &instance, to_stage_id)?;
    snapshot_plan_summary(workspace_id, &plan)
}

pub fn current_surface(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    instance_id: &str,
) -> Result<LifecycleStageSurfaceSummary> {
    loom.authorize_domain(workspace, AclDomain::Lifecycle, AclRight::Read)?;
    let instance = load_instance(loom.store(), workspace_id, instance_id)?
        .ok_or_else(|| LoomError::not_found("lifecycle instance not found"))?;
    let definition = load_definition(loom.store(), workspace_id, &instance.definition_id)?
        .ok_or_else(|| LoomError::not_found("lifecycle definition not found"))?;
    let surface = load_surface(loom.store(), workspace_id, instance_id)?
        .unwrap_or(StageSurface::for_instance(&definition, &instance)?);
    stage_surface_summary(workspace_id, &surface)
}

pub fn transition(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: LifecycleTransitionRequest<'_>,
) -> Result<LifecycleTransitionResult> {
    loom.authorize_domain(workspace, AclDomain::Lifecycle, AclRight::Write)?;
    let instance = load_instance(loom.store(), request.workspace_id, request.instance_id)?
        .ok_or_else(|| LoomError::not_found("lifecycle instance not found"))?;
    let definition = load_definition(loom.store(), request.workspace_id, &instance.definition_id)?
        .ok_or_else(|| LoomError::not_found("lifecycle definition not found"))?;
    let gate_evaluations = request
        .gate_evaluations
        .into_iter()
        .map(gate_evaluation)
        .collect::<Result<Vec<_>>>()?;
    let snapshot_digest = request.snapshot_digest.map(Digest::parse).transpose()?;
    if let Some(digest) = snapshot_digest {
        read_snapshot_content(loom, workspace, digest)?;
    }
    let transition = LifecycleTransitionRecord::new(LifecycleTransitionInput {
        transition_id: request.transition_id.to_string(),
        instance_id: request.instance_id.to_string(),
        definition_id: definition.definition_id.clone(),
        definition_version: definition.version.clone(),
        from_stage_id: instance.current_stage_id.clone(),
        to_stage_id: request.to_stage_id.to_string(),
        actor_principal_id: request.actor_principal_id.to_string(),
        gate_evaluations,
        snapshot_digest,
        recorded_at_ms: request.recorded_at_ms,
    })?;
    let previous_bytes = instance.encode()?;
    let base_root = Digest::hash(loom.store().digest_algo(), &previous_bytes);
    let next = instance.apply_transition(&definition, transition.clone())?;
    let next_bytes = next.encode()?;
    let root_after = Digest::hash(loom.store().digest_algo(), &next_bytes);
    let surface = StageSurface::for_instance(&definition, &next)?;
    let mut log = load_log(loom.store(), request.workspace_id)?;
    let sequence = log
        .records
        .last()
        .map(|record| record.sequence + 1)
        .unwrap_or(1);
    let payload = transition.encode()?;
    let envelope = OperationEnvelope::new(
        loom.store().digest_algo(),
        OperationEnvelopeInput {
            workspace_id: request.workspace_id,
            app_id: APP_ID,
            scope_id: &loom_substrate::lifecycle::lifecycle_operation_cursor_scope(
                request.workspace_id,
            ),
            operation_id: request.transition_id,
            operation_kind: "lifecycle.transitioned",
            sequence,
            actor_principal: WorkspaceId::parse(request.actor_principal_id)?,
            actor_kind: ActorKind::User,
            timestamp_ms: request.recorded_at_ms,
            idempotency_key: request.transition_id,
            base_root,
            base_entity_version: Some(&instance.current_stage_id),
            target_entity_id: Some(request.instance_id),
            payload: &payload,
            policy_labels: &[],
            signature: None,
            agent: None,
        },
    )?;
    log.append(LifecycleOperationRecord::transition(
        sequence,
        &transition,
        root_after,
        envelope.encode()?,
    )?)?;
    save_instance(loom.store(), request.workspace_id, &next)?;
    save_surface(loom.store(), request.workspace_id, &surface)?;
    save_log(loom.store(), request.workspace_id, &log)?;
    append_lifecycle_trigger_record(
        loom,
        workspace,
        request.workspace_id,
        &transition,
        root_after,
    )?;
    record_lifecycle_revision(
        loom,
        workspace,
        request.workspace_id,
        &format!("lifecycle:instance:{}", request.instance_id),
        request.transition_id,
        &next_bytes,
        "application/vnd.uldren.loom.lifecycle.instance+cbor",
        request.recorded_at_ms,
    )?;
    let snapshot = if let Some(digest) = snapshot_digest {
        let plan = SnapshotPlan::for_transition(&definition, &instance, request.to_stage_id)?;
        let record = SnapshotRecord::from_plan(
            &plan,
            request.transition_id,
            digest,
            request.recorded_at_ms,
        )?;
        save_snapshot(loom.store(), request.workspace_id, &record)?;
        Some(snapshot_record_summary(request.workspace_id, &record)?)
    } else {
        None
    };
    Ok(LifecycleTransitionResult {
        instance: instance_summary(request.workspace_id, &next)?,
        transition: transition_summary(&transition),
        surface: stage_surface_summary(request.workspace_id, &surface)?,
        snapshot,
        operation_log: operation_log_summary(&log),
    })
}

pub fn get_snapshot(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    snapshot_id: &str,
) -> Result<Option<LifecycleSnapshotRecordSummary>> {
    loom.authorize_domain(workspace, AclDomain::Lifecycle, AclRight::Read)?;
    load_snapshot(loom.store(), workspace_id, snapshot_id)?
        .as_ref()
        .map(|record| snapshot_record_summary(workspace_id, record))
        .transpose()
}

pub fn snapshot_content(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    snapshot_id: &str,
) -> Result<Option<Vec<u8>>> {
    loom.authorize_domain(workspace, AclDomain::Lifecycle, AclRight::Read)?;
    load_snapshot(loom.store(), workspace_id, snapshot_id)?
        .map(|record| read_snapshot_content(loom, workspace, record.snapshot_digest))
        .transpose()
}

pub fn list_snapshots(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<Vec<LifecycleSnapshotRecordSummary>> {
    loom.authorize_domain(workspace, AclDomain::Lifecycle, AclRight::Read)?;
    let mut out = loom
        .store()
        .control_scan_prefix(&snapshot_prefix(workspace_id)?)?
        .into_iter()
        .map(|(_, bytes)| SnapshotRecord::decode(&bytes))
        .collect::<Result<Vec<_>>>()?;
    out.sort_by(|a, b| a.snapshot_id.cmp(&b.snapshot_id));
    out.iter()
        .map(|record| snapshot_record_summary(workspace_id, record))
        .collect()
}

pub fn operation_log(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<LifecycleOperationLogSummary> {
    loom.authorize_domain(workspace, AclDomain::Lifecycle, AclRight::Read)?;
    Ok(operation_log_summary(&load_log(
        loom.store(),
        workspace_id,
    )?))
}

fn record_lifecycle_revision(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    entity_id: &str,
    operation_id: &str,
    body: &[u8],
    media_type: &str,
    timestamp_ms: u64,
) -> Result<()> {
    let index_path = revision_index_path(workspace_id)?;
    let index = match loom.read_file_reserved(workspace, &index_path) {
        Ok(bytes) => RevisionIndex::decode(&bytes)?,
        Err(err) if err.code == Code::NotFound => RevisionIndex::new(),
        Err(err) => return Err(err),
    };
    let expected_latest_revision = index
        .latest(entity_id)
        .map(|entry| entry.revision)
        .unwrap_or(0);
    let revision = expected_latest_revision.saturating_add(1);
    let root = Digest::hash(loom.store().digest_algo(), body);
    let mut state = ProfileTransactionState::new(root, index);
    let update = ProfileRevisionUpdate::new(
        entity_id,
        operation_id,
        BodyRef::new(
            Digest::hash(loom.store().digest_algo(), body),
            body.len() as u64,
            media_type,
        )?,
        timestamp_ms,
        format!("lifecycle:{workspace_id}:{entity_id}:{revision}"),
        Some(expected_latest_revision),
    )?;
    state.apply(ProfileTransaction::new(
        workspace_id,
        None,
        root,
        vec![update],
    )?)?;
    let index = state.into_revision_index();
    loom.create_directory_reserved(workspace, REVISION_INDEX_DIR, true)?;
    loom.write_file_reserved(workspace, &index_path, &index.encode()?, 0o100644)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn definition_prefix(workspace_id: &str) -> Result<Vec<u8>> {
    loom_substrate::validate_text("lifecycle workspace_id", workspace_id)?;
    Ok(format!("{CONTROL_PREFIX}/{workspace_id}/definitions/").into_bytes())
}

fn definition_key(workspace_id: &str, definition_id: &str) -> Result<Vec<u8>> {
    loom_substrate::validate_text("lifecycle definition_id", definition_id)?;
    let mut key = definition_prefix(workspace_id)?;
    key.extend_from_slice(definition_id.as_bytes());
    Ok(key)
}

fn instance_prefix(workspace_id: &str) -> Result<Vec<u8>> {
    loom_substrate::validate_text("lifecycle workspace_id", workspace_id)?;
    Ok(format!("{CONTROL_PREFIX}/{workspace_id}/instances/").into_bytes())
}

fn instance_key(workspace_id: &str, instance_id: &str) -> Result<Vec<u8>> {
    loom_substrate::validate_text("lifecycle instance_id", instance_id)?;
    let mut key = instance_prefix(workspace_id)?;
    key.extend_from_slice(instance_id.as_bytes());
    Ok(key)
}

fn snapshot_prefix(workspace_id: &str) -> Result<Vec<u8>> {
    loom_substrate::validate_text("lifecycle workspace_id", workspace_id)?;
    Ok(format!("{CONTROL_PREFIX}/{workspace_id}/snapshots/").into_bytes())
}

fn snapshot_key(workspace_id: &str, snapshot_id: &str) -> Result<Vec<u8>> {
    loom_substrate::validate_text("lifecycle snapshot_id", snapshot_id)?;
    let mut key = snapshot_prefix(workspace_id)?;
    key.extend_from_slice(snapshot_id.as_bytes());
    Ok(key)
}

fn load_definition(
    store: &FileStore,
    workspace_id: &str,
    definition_id: &str,
) -> Result<Option<LifecycleDefinition>> {
    store
        .control_get(&definition_key(workspace_id, definition_id)?)?
        .map(|bytes| LifecycleDefinition::decode(&bytes))
        .transpose()
}

fn save_definition(
    store: &FileStore,
    workspace_id: &str,
    definition: &LifecycleDefinition,
) -> Result<()> {
    store.control_set(
        &definition_key(workspace_id, &definition.definition_id)?,
        definition.encode()?,
    )
}

fn load_instance(
    store: &FileStore,
    workspace_id: &str,
    instance_id: &str,
) -> Result<Option<LifecycleInstance>> {
    store
        .control_get(&instance_key(workspace_id, instance_id)?)?
        .map(|bytes| LifecycleInstance::decode(&bytes))
        .transpose()
}

fn save_instance(
    store: &FileStore,
    workspace_id: &str,
    instance: &LifecycleInstance,
) -> Result<()> {
    store.control_set(
        &instance_key(workspace_id, &instance.instance_id)?,
        instance.encode()?,
    )
}

fn surface_prefix(workspace_id: &str) -> Result<Vec<u8>> {
    loom_substrate::validate_text("lifecycle workspace_id", workspace_id)?;
    Ok(format!("{CONTROL_PREFIX}/{workspace_id}/surfaces/").into_bytes())
}

fn surface_key(workspace_id: &str, instance_id: &str) -> Result<Vec<u8>> {
    loom_substrate::validate_text("lifecycle instance_id", instance_id)?;
    let mut key = surface_prefix(workspace_id)?;
    key.extend_from_slice(instance_id.as_bytes());
    Ok(key)
}

fn load_surface(
    store: &FileStore,
    workspace_id: &str,
    instance_id: &str,
) -> Result<Option<StageSurface>> {
    store
        .control_get(&surface_key(workspace_id, instance_id)?)?
        .map(|bytes| StageSurface::decode(&bytes))
        .transpose()
}

fn save_surface(store: &FileStore, workspace_id: &str, surface: &StageSurface) -> Result<()> {
    store.control_set(
        &surface_key(workspace_id, &surface.instance_id)?,
        surface.encode()?,
    )
}

fn recompute_surfaces_for_definition(
    store: &FileStore,
    workspace_id: &str,
    definition: &LifecycleDefinition,
) -> Result<()> {
    for (_, bytes) in store.control_scan_prefix(&instance_prefix(workspace_id)?)? {
        let instance = LifecycleInstance::decode(&bytes)?;
        if instance.definition_id == definition.definition_id
            && instance.definition_version == definition.version
        {
            save_surface(
                store,
                workspace_id,
                &StageSurface::for_instance(definition, &instance)?,
            )?;
        }
    }
    Ok(())
}

fn lifecycle_trigger_binding_id(workspace_id: &str) -> WorkspaceId {
    let digest = Digest::hash(
        Algo::Blake3,
        format!("lifecycle-trigger:{workspace_id}").as_bytes(),
    );
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest.bytes()[..16]);
    WorkspaceId::from_bytes(bytes)
}

fn append_lifecycle_trigger_record(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    transition: &LifecycleTransitionRecord,
    root_after: Digest,
) -> Result<()> {
    let binding = lifecycle_trigger_binding_id(workspace_id);
    let fired_at_seq = trigger_history_system(loom, workspace, binding, 0, usize::MAX)?
        .into_iter()
        .map(|record| record.fired_at_seq)
        .max()
        .map_or(0, |seq| seq.saturating_add(1));
    let stimulus = TriggerStimulus::Change {
        source_cursor: format!(
            "lifecycle-transition:v1:{workspace_id}:{}:{}",
            transition.instance_id, transition.transition_id
        ),
        commit: root_after,
    };
    trigger_append_fire_record_system(
        loom,
        workspace,
        &FireRecord {
            binding,
            stimulus_digest: stimulus_digest(Algo::Blake3, &stimulus)?,
            stimulus,
            proposed: Some(root_after),
            outcome: FireOutcome::Applied,
            cost: 0,
            fired_at_seq,
        },
    )
}

fn load_snapshot(
    store: &FileStore,
    workspace_id: &str,
    snapshot_id: &str,
) -> Result<Option<SnapshotRecord>> {
    store
        .control_get(&snapshot_key(workspace_id, snapshot_id)?)?
        .map(|bytes| SnapshotRecord::decode(&bytes))
        .transpose()
}

fn save_snapshot(store: &FileStore, workspace_id: &str, snapshot: &SnapshotRecord) -> Result<()> {
    store.control_set(
        &snapshot_key(workspace_id, &snapshot.snapshot_id)?,
        snapshot.encode()?,
    )
}

fn read_snapshot_content(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    digest: Digest,
) -> Result<Vec<u8>> {
    match loom.read_file_reserved(workspace, &facet_path(FacetKind::Cas, &digest.to_hex())) {
        Ok(bytes) => {
            let actual = Digest::hash(loom.store().digest_algo(), &bytes);
            if actual != digest {
                return Err(LoomError::integrity_failure(format!(
                    "lifecycle snapshot content {digest} hashes to {actual}"
                )));
            }
            return Ok(bytes);
        }
        Err(err) if err.code == Code::NotFound => {}
        Err(err) => return Err(err),
    }
    let canonical = loom
        .store()
        .get(&digest)?
        .ok_or_else(|| LoomError::not_found("lifecycle snapshot object not found"))?;
    match Object::decode(&canonical)? {
        Object::Blob(bytes) => Ok(bytes),
        other => Err(LoomError::invalid(format!(
            "lifecycle snapshot object is {:?}, not Blob",
            other.object_type()
        ))),
    }
}

fn load_log(store: &FileStore, workspace_id: &str) -> Result<LifecycleOperationLog> {
    match store.control_get(&lifecycle_operation_log_key(workspace_id)?)? {
        Some(bytes) => LifecycleOperationLog::decode(&bytes),
        None => LifecycleOperationLog::new(workspace_id, Vec::new()),
    }
}

fn save_log(store: &FileStore, workspace_id: &str, log: &LifecycleOperationLog) -> Result<()> {
    store.control_set(&lifecycle_operation_log_key(workspace_id)?, log.encode()?)
}

fn standard_kind(value: &str) -> Result<StandardLifecycleKind> {
    match value {
        "feature" => Ok(StandardLifecycleKind::Feature),
        "bug" => Ok(StandardLifecycleKind::Bug),
        "incident" => Ok(StandardLifecycleKind::Incident),
        "design" => Ok(StandardLifecycleKind::Design),
        _ => Err(LoomError::invalid("unknown standard lifecycle kind")),
    }
}

fn gate_evaluation(input: LifecycleGateEvaluationInput) -> Result<GateEvaluation> {
    let mut evaluation = GateEvaluation::new(input.gate_id, input.passed, input.evaluated_at_ms)?;
    evaluation.principal_id = input.principal_id;
    evaluation.evidence_digest = input
        .evidence_digest
        .map(|value| Digest::parse(&value))
        .transpose()?;
    Ok(evaluation)
}

fn definition_summary(
    workspace_id: &str,
    definition: &LifecycleDefinition,
) -> Result<LifecycleDefinitionSummary> {
    Ok(LifecycleDefinitionSummary {
        workspace_id: workspace_id.to_string(),
        definition_id: definition.definition_id.clone(),
        version: definition.version.clone(),
        initial_stage_id: definition.initial_stage_id.clone(),
        stages: definition.stages.iter().map(stage_summary).collect(),
        definition_cbor_hex: hex_bytes(&definition.encode()?),
    })
}

fn stage_summary(stage: &loom_substrate::lifecycle::LifecycleStage) -> LifecycleStageSummary {
    LifecycleStageSummary {
        stage_id: stage.stage_id.clone(),
        label: stage.label.clone(),
        entry_gates: stage.entry_gates.iter().map(gate_summary).collect(),
        exit_gates: stage.exit_gates.iter().map(gate_summary).collect(),
        snapshot_policy: snapshot_policy(stage.snapshot_policy).to_string(),
        surfaced_tools: stage.surfaced_tools.clone(),
        prompt_refs: stage.prompt_refs.clone(),
    }
}

fn gate_summary(gate: &loom_substrate::lifecycle::LifecycleGate) -> LifecycleGateSummary {
    LifecycleGateSummary {
        gate_id: gate.gate_id.clone(),
        label: gate.label.clone(),
        kind: match gate.kind {
            loom_substrate::lifecycle::GateKind::Predicate => "predicate",
            loom_substrate::lifecycle::GateKind::Attestation => "attestation",
        }
        .to_string(),
        predicate_digest: gate.predicate_digest.map(|digest| digest.to_string()),
        required_role: gate.required_role.clone(),
    }
}

fn instance_summary(
    workspace_id: &str,
    instance: &LifecycleInstance,
) -> Result<LifecycleInstanceSummary> {
    Ok(LifecycleInstanceSummary {
        workspace_id: workspace_id.to_string(),
        instance_id: instance.instance_id.clone(),
        definition_id: instance.definition_id.clone(),
        definition_version: instance.definition_version.clone(),
        subject_refs: instance.subject_refs.clone(),
        current_stage_id: instance.current_stage_id.clone(),
        stage_history: instance
            .stage_history
            .iter()
            .map(transition_summary)
            .collect(),
        instance_cbor_hex: hex_bytes(&instance.encode()?),
    })
}

fn transition_summary(transition: &LifecycleTransitionRecord) -> LifecycleTransitionSummary {
    LifecycleTransitionSummary {
        transition_id: transition.transition_id.clone(),
        instance_id: transition.instance_id.clone(),
        definition_id: transition.definition_id.clone(),
        definition_version: transition.definition_version.clone(),
        from_stage_id: transition.from_stage_id.clone(),
        to_stage_id: transition.to_stage_id.clone(),
        actor_principal_id: transition.actor_principal_id.clone(),
        gate_evaluations: transition
            .gate_evaluations
            .iter()
            .map(gate_evaluation_summary)
            .collect(),
        snapshot_digest: transition.snapshot_digest.map(|digest| digest.to_string()),
        recorded_at_ms: transition.recorded_at_ms,
    }
}

fn gate_evaluation_summary(evaluation: &GateEvaluation) -> LifecycleGateEvaluationSummary {
    LifecycleGateEvaluationSummary {
        gate_id: evaluation.gate_id.clone(),
        passed: evaluation.passed,
        principal_id: evaluation.principal_id.clone(),
        evidence_digest: evaluation.evidence_digest.map(|digest| digest.to_string()),
        evaluated_at_ms: evaluation.evaluated_at_ms,
    }
}

fn snapshot_plan_summary(
    workspace_id: &str,
    plan: &SnapshotPlan,
) -> Result<LifecycleSnapshotPlanSummary> {
    Ok(LifecycleSnapshotPlanSummary {
        workspace_id: workspace_id.to_string(),
        instance_id: plan.instance_id.clone(),
        from_stage_id: plan.from_stage_id.clone(),
        to_stage_id: plan.to_stage_id.clone(),
        required: plan.required,
        subject_refs: plan.subject_refs.clone(),
        policy: snapshot_policy(plan.policy).to_string(),
        plan_cbor_hex: hex_bytes(&plan.encode()?),
    })
}

fn snapshot_record_summary(
    workspace_id: &str,
    record: &SnapshotRecord,
) -> Result<LifecycleSnapshotRecordSummary> {
    Ok(LifecycleSnapshotRecordSummary {
        workspace_id: workspace_id.to_string(),
        snapshot_id: record.snapshot_id.clone(),
        instance_id: record.instance_id.clone(),
        transition_id: record.transition_id.clone(),
        from_stage_id: record.from_stage_id.clone(),
        to_stage_id: record.to_stage_id.clone(),
        subject_refs: record.subject_refs.clone(),
        policy: snapshot_policy(record.policy).to_string(),
        snapshot_digest: record.snapshot_digest.to_string(),
        recorded_at_ms: record.recorded_at_ms,
        snapshot_cbor_hex: hex_bytes(&record.encode()?),
    })
}

fn stage_surface_summary(
    workspace_id: &str,
    surface: &StageSurface,
) -> Result<LifecycleStageSurfaceSummary> {
    Ok(LifecycleStageSurfaceSummary {
        workspace_id: workspace_id.to_string(),
        instance_id: surface.instance_id.clone(),
        stage_id: surface.stage_id.clone(),
        surfaced_tools: surface.surfaced_tools.clone(),
        prompt_refs: surface.prompt_refs.clone(),
        read_only: surface.read_only,
        surface_cbor_hex: hex_bytes(&surface.encode()?),
    })
}

fn operation_log_summary(log: &LifecycleOperationLog) -> LifecycleOperationLogSummary {
    LifecycleOperationLogSummary {
        workspace_id: log.workspace_id.clone(),
        records: log
            .records
            .iter()
            .map(|record| LifecycleOperationSummary {
                sequence: record.sequence,
                operation_id: record.operation_id.clone(),
                operation_kind: record.operation_kind.clone(),
                instance_id: record.instance_id.clone(),
                target_entity_id: record.target_entity_id.clone(),
                root_after: record.root_after.to_string(),
                envelope_cbor_hex: hex_bytes(&record.envelope),
            })
            .collect(),
    }
}

fn snapshot_policy(policy: SnapshotPolicy) -> &'static str {
    match policy {
        SnapshotPolicy::None => "none",
        SnapshotPolicy::FreezeScope => "freeze_scope",
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{Algo, FacetKind};

    fn digest(label: &[u8]) -> String {
        Digest::hash(Algo::Blake3, label).to_string()
    }

    #[test]
    fn standard_lifecycle_instantiates_transitions_and_reads_snapshot() {
        let path = std::env::temp_dir().join(format!(
            "loom-lifecycle-{}.loom",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::from_bytes([7; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("repo"), workspace)
            .unwrap();
        let definition = define_standard_lifecycle(
            &mut loom,
            workspace,
            StandardLifecycleRequest {
                workspace_id: "studio",
                kind: "feature",
                version: "1",
                completion_predicate_digest: &digest(b"predicate"),
            },
        )
        .unwrap();
        assert_eq!(definition.definition_id, "feature");
        let instance = instantiate(
            &mut loom,
            workspace,
            "studio",
            "feat-1",
            "feature",
            vec!["page:roadmap".to_string()],
        )
        .unwrap();
        assert_eq!(instance.current_stage_id, "ideate");
        let initial_surface = load_surface(loom.store(), "studio", "feat-1")
            .unwrap()
            .unwrap();
        assert_eq!(initial_surface.stage_id, "ideate");
        assert_eq!(initial_surface.surfaced_tools, vec!["pages_create"]);
        let draft = transition(
            &mut loom,
            workspace,
            LifecycleTransitionRequest {
                workspace_id: "studio",
                instance_id: "feat-1",
                transition_id: "tr-1",
                to_stage_id: "draft",
                actor_principal_id: &workspace.to_string(),
                gate_evaluations: vec![LifecycleGateEvaluationInput {
                    gate_id: "enter-draft".to_string(),
                    passed: true,
                    principal_id: Some(workspace.to_string()),
                    evidence_digest: None,
                    evaluated_at_ms: 10,
                }],
                snapshot_digest: None,
                recorded_at_ms: 11,
            },
        )
        .unwrap();
        assert_eq!(draft.instance.current_stage_id, "draft");
        assert_eq!(draft.operation_log.records.len(), 1);
        let stored_surface = load_surface(loom.store(), "studio", "feat-1")
            .unwrap()
            .unwrap();
        assert_eq!(stored_surface.stage_id, "draft");
        assert_eq!(stored_surface.surfaced_tools, vec!["pages_update"]);
        assert_eq!(
            current_surface(&loom, workspace, "studio", "feat-1")
                .unwrap()
                .stage_id,
            "draft"
        );
        let history = trigger_history_system(
            &loom,
            workspace,
            lifecycle_trigger_binding_id("studio"),
            0,
            10,
        )
        .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].outcome, FireOutcome::Applied);
        assert_eq!(
            history[0].proposed.unwrap(),
            Digest::parse(&draft.operation_log.records[0].root_after).unwrap()
        );
        match &history[0].stimulus {
            TriggerStimulus::Change {
                source_cursor,
                commit,
            } => {
                assert_eq!(source_cursor, "lifecycle-transition:v1:studio:feat-1:tr-1");
                assert_eq!(
                    *commit,
                    Digest::parse(&draft.operation_log.records[0].root_after).unwrap()
                );
            }
            TriggerStimulus::Time { .. } => panic!("lifecycle transition stimulus must be change"),
        }
        transition(
            &mut loom,
            workspace,
            LifecycleTransitionRequest {
                workspace_id: "studio",
                instance_id: "feat-1",
                transition_id: "tr-2",
                to_stage_id: "structure",
                actor_principal_id: &workspace.to_string(),
                gate_evaluations: vec![LifecycleGateEvaluationInput {
                    gate_id: "enter-structure".to_string(),
                    passed: true,
                    principal_id: Some(workspace.to_string()),
                    evidence_digest: None,
                    evaluated_at_ms: 12,
                }],
                snapshot_digest: None,
                recorded_at_ms: 13,
            },
        )
        .unwrap();
        let snapshot_payload = b"ready scope snapshot".to_vec();
        let snapshot_digest = loom
            .store()
            .put(&Object::Blob(snapshot_payload.clone()).canonical())
            .unwrap();
        let ready = transition(
            &mut loom,
            workspace,
            LifecycleTransitionRequest {
                workspace_id: "studio",
                instance_id: "feat-1",
                transition_id: "tr-3",
                to_stage_id: "ready",
                actor_principal_id: &workspace.to_string(),
                gate_evaluations: vec![LifecycleGateEvaluationInput {
                    gate_id: "enter-ready".to_string(),
                    passed: true,
                    principal_id: Some(workspace.to_string()),
                    evidence_digest: Some(digest(b"ready-evidence")),
                    evaluated_at_ms: 14,
                }],
                snapshot_digest: Some(&snapshot_digest.to_string()),
                recorded_at_ms: 15,
            },
        )
        .unwrap();
        let ready_snapshot = ready.snapshot.unwrap();
        assert_eq!(ready_snapshot.snapshot_id, "feat-1:tr-3");
        assert_eq!(ready_snapshot.snapshot_digest, snapshot_digest.to_string());
        assert_eq!(
            snapshot_content(&loom, workspace, "studio", "feat-1:tr-3")
                .unwrap()
                .unwrap(),
            snapshot_payload
        );
        assert_eq!(
            trigger_history_system(
                &loom,
                workspace,
                lifecycle_trigger_binding_id("studio"),
                0,
                10,
            )
            .unwrap()
            .len(),
            3
        );
        assert!(list_instances(&loom, workspace, "studio").unwrap().len() == 1);
        std::fs::remove_file(path).unwrap();
    }
}
