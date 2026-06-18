use loom_core::error::{LoomError, Result};
use loom_core::workspace::WorkspaceId;
use loom_core::{AclDomain, AclResource, AclResourceScope, AclRight, AclScopeKind, Digest, Loom};
use loom_store::FileStore;
use loom_substrate::meetings::{
    AnnotationRecord, AnnotationStatus, ExtractionReviewProjection, MeetingRecord, MeetingStatus,
    MeetingsProfileSnapshot, ProjectionAction, ProjectionKind, ProjectionOutput,
    ProjectionOutputSet, RedactionState, meetings_profile_key,
};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MeetingSummary {
    pub meeting_id: String,
    pub title: String,
    pub starts_at_ms: Option<u64>,
    pub ends_at_ms: Option<u64>,
    pub status: String,
    pub source_refs: Vec<String>,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MeetingsListSummary {
    pub workspace_id: String,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub meetings: Vec<MeetingSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MeetingAnnotationSummary {
    pub annotation_id: String,
    pub meeting_id: String,
    pub source_span_ids: Vec<String>,
    pub kind: String,
    pub label: String,
    pub normalized_id: Option<String>,
    pub confidence_ppm: Option<u32>,
    pub evidence_digest: Option<String>,
    pub extractor: Option<String>,
    pub status: String,
    pub created_at_ms: u64,
    pub accepted_by: Option<String>,
    pub accepted_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MeetingDetailSummary {
    pub workspace_id: String,
    pub meeting_id: String,
    pub title: String,
    pub starts_at_ms: Option<u64>,
    pub ends_at_ms: Option<u64>,
    pub calendar_event_ref: Option<String>,
    pub owner_principal: Option<String>,
    pub attendee_refs: Vec<String>,
    pub folder_refs: Vec<String>,
    pub source_refs: Vec<String>,
    pub current_source_digest: String,
    pub summary_ref: Option<String>,
    pub status: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub annotations: Vec<MeetingAnnotationSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MeetingsProjectionOutputSummary {
    pub output_id: String,
    pub projection: String,
    pub action: String,
    pub output_ref: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub source_ids: Vec<String>,
    pub payload_cbor_hex: String,
    pub redaction_state: Option<String>,
    pub recorded_at_ms: u64,
    pub record_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MeetingsProjectionSummary {
    pub workspace_id: String,
    pub profile_root: String,
    pub outputs: Vec<MeetingsProjectionOutputSummary>,
    pub output_set_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MeetingsExtractionReviewSummary {
    pub workspace_id: String,
    pub suggested_annotation_ids: Vec<String>,
    pub accepted_annotation_ids: Vec<String>,
    pub rejected_annotation_ids: Vec<String>,
    pub vocabulary_terms: usize,
    pub review_cbor_hex: String,
}

pub fn list(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    limit: usize,
    offset: usize,
) -> Result<MeetingsListSummary> {
    authorize_meetings_read(loom, workspace, workspace_id)?;
    let snapshot = load_snapshot(loom, workspace_id)?;
    let total = snapshot.meetings.len();
    let meetings = snapshot
        .meetings
        .iter()
        .skip(offset)
        .take(limit)
        .map(meeting_summary)
        .collect();
    Ok(MeetingsListSummary {
        workspace_id: snapshot.workspace_id,
        total,
        offset,
        limit,
        meetings,
    })
}

pub fn get(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    meeting_id: &str,
) -> Result<MeetingDetailSummary> {
    authorize_meetings_read(loom, workspace, workspace_id)?;
    let snapshot = load_snapshot(loom, workspace_id)?;
    let meeting = snapshot
        .meetings
        .iter()
        .find(|meeting| meeting.meeting_id == meeting_id)
        .ok_or_else(|| LoomError::not_found("meeting not found"))?;
    Ok(meeting_detail(
        &snapshot.workspace_id,
        meeting,
        &snapshot.annotations,
    ))
}

pub fn projection_outputs(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<MeetingsProjectionSummary> {
    authorize_meetings_read(loom, workspace, workspace_id)?;
    let snapshot = load_snapshot(loom, workspace_id)?;
    let profile_root = profile_root(loom, &snapshot)?;
    let output_set = ProjectionOutputSet::from_snapshot(&snapshot)?;
    Ok(MeetingsProjectionSummary {
        workspace_id: output_set.workspace_id.clone(),
        profile_root: profile_root.to_string(),
        outputs: output_set
            .outputs
            .iter()
            .map(projection_output_summary)
            .collect::<Result<Vec<_>>>()?,
        output_set_cbor_hex: hex::encode(output_set.encode()?),
    })
}

pub fn extraction_review(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<MeetingsExtractionReviewSummary> {
    authorize_meetings_read(loom, workspace, workspace_id)?;
    let snapshot = load_snapshot(loom, workspace_id)?;
    let review = ExtractionReviewProjection::new(
        workspace_id,
        &snapshot.annotations,
        snapshot.vocabulary_terms.clone(),
        snapshot.entity_merges.clone(),
    )?;
    Ok(MeetingsExtractionReviewSummary {
        workspace_id: review.workspace_id.clone(),
        suggested_annotation_ids: review.suggested_annotation_ids.clone(),
        accepted_annotation_ids: review.accepted_annotation_ids.clone(),
        rejected_annotation_ids: review.rejected_annotation_ids.clone(),
        vocabulary_terms: review.vocabulary_terms.len(),
        review_cbor_hex: hex::encode(review.encode()?),
    })
}

fn load_snapshot(loom: &Loom<FileStore>, workspace_id: &str) -> Result<MeetingsProfileSnapshot> {
    match loom
        .store()
        .control_get(&meetings_profile_key(workspace_id)?)?
    {
        Some(bytes) => MeetingsProfileSnapshot::decode(&bytes),
        None => Err(LoomError::not_found("meetings snapshot not found")),
    }
}

fn profile_root(loom: &Loom<FileStore>, snapshot: &MeetingsProfileSnapshot) -> Result<Digest> {
    Ok(Digest::hash(
        loom.store().digest_algo(),
        &snapshot.encode()?,
    ))
}

fn authorize_meetings_read(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<()> {
    loom.authorize_resource(
        AclResource::scoped(
            workspace,
            AclDomain::Meetings,
            None,
            AclResourceScope::Prefix {
                kind: AclScopeKind::Collection,
                value: workspace_id.as_bytes(),
            },
        ),
        AclRight::Read,
    )
}

fn meeting_summary(meeting: &MeetingRecord) -> MeetingSummary {
    MeetingSummary {
        meeting_id: meeting.meeting_id.clone(),
        title: meeting.title.clone(),
        starts_at_ms: meeting.starts_at_ms,
        ends_at_ms: meeting.ends_at_ms,
        status: meeting_status(meeting.status).to_string(),
        source_refs: meeting.source_refs.clone(),
        updated_at_ms: meeting.updated_at_ms,
    }
}

fn meeting_detail(
    workspace_id: &str,
    meeting: &MeetingRecord,
    annotations: &[AnnotationRecord],
) -> MeetingDetailSummary {
    MeetingDetailSummary {
        workspace_id: workspace_id.to_string(),
        meeting_id: meeting.meeting_id.clone(),
        title: meeting.title.clone(),
        starts_at_ms: meeting.starts_at_ms,
        ends_at_ms: meeting.ends_at_ms,
        calendar_event_ref: meeting.calendar_event_ref.clone(),
        owner_principal: meeting.owner_principal.clone(),
        attendee_refs: meeting.attendee_refs.clone(),
        folder_refs: meeting.folder_refs.clone(),
        source_refs: meeting.source_refs.clone(),
        current_source_digest: meeting.current_source_digest.to_string(),
        summary_ref: meeting.summary_ref.clone(),
        status: meeting_status(meeting.status).to_string(),
        created_at_ms: meeting.created_at_ms,
        updated_at_ms: meeting.updated_at_ms,
        annotations: annotations
            .iter()
            .filter(|annotation| annotation.meeting_id == meeting.meeting_id)
            .map(annotation_summary)
            .collect(),
    }
}

fn annotation_summary(annotation: &AnnotationRecord) -> MeetingAnnotationSummary {
    MeetingAnnotationSummary {
        annotation_id: annotation.annotation_id.clone(),
        meeting_id: annotation.meeting_id.clone(),
        source_span_ids: annotation.source_span_ids.clone(),
        kind: annotation.kind.clone(),
        label: annotation.label.clone(),
        normalized_id: annotation.normalized_id.clone(),
        confidence_ppm: annotation.confidence_ppm,
        evidence_digest: annotation.evidence_digest.map(|digest| digest.to_string()),
        extractor: annotation.extractor.clone(),
        status: annotation_status(annotation.status).to_string(),
        created_at_ms: annotation.created_at_ms,
        accepted_by: annotation.accepted_by.clone(),
        accepted_at_ms: annotation.accepted_at_ms,
    }
}

fn annotation_status(status: AnnotationStatus) -> &'static str {
    match status {
        AnnotationStatus::Observed => "observed",
        AnnotationStatus::Suggested => "suggested",
        AnnotationStatus::Accepted => "accepted",
        AnnotationStatus::Rejected => "rejected",
        AnnotationStatus::Superseded => "superseded",
        AnnotationStatus::Merged => "merged",
    }
}

fn projection_output_summary(output: &ProjectionOutput) -> Result<MeetingsProjectionOutputSummary> {
    Ok(MeetingsProjectionOutputSummary {
        output_id: output.output_id.clone(),
        projection: projection_kind(output.projection).to_string(),
        action: projection_action(output.action).to_string(),
        output_ref: output.output_ref.clone(),
        entity_kind: output.entity_kind.clone(),
        entity_id: output.entity_id.clone(),
        source_ids: output.source_ids.clone(),
        payload_cbor_hex: hex::encode(loom_codec::encode(&output.payload).map_err(|err| {
            LoomError::corrupt(format!("projection output payload encode failed: {err}"))
        })?),
        redaction_state: output
            .redaction_state
            .map(redaction_state)
            .map(str::to_string),
        recorded_at_ms: output.recorded_at_ms,
        record_cbor_hex: hex::encode(output.encode()?),
    })
}

fn meeting_status(status: MeetingStatus) -> &'static str {
    match status {
        MeetingStatus::Active => "active",
        MeetingStatus::DeletedAtSource => "deleted-at-source",
        MeetingStatus::Redacted => "redacted",
        MeetingStatus::RetainedMetadataOnly => "retained-metadata-only",
    }
}

fn projection_kind(kind: ProjectionKind) -> &'static str {
    match kind {
        ProjectionKind::Document => "document",
        ProjectionKind::Files => "files",
        ProjectionKind::Graph => "graph",
        ProjectionKind::Vector => "vector",
        ProjectionKind::Search => "search",
        ProjectionKind::SqlDataframe => "sql-dataframe",
        ProjectionKind::Ledger => "ledger",
    }
}

fn projection_action(action: ProjectionAction) -> &'static str {
    match action {
        ProjectionAction::Upsert => "upsert",
        ProjectionAction::Append => "append",
        ProjectionAction::Invalidate => "invalidate",
        ProjectionAction::RetainMetadata => "retain-metadata",
    }
}

fn redaction_state(state: RedactionState) -> &'static str {
    match state {
        RedactionState::Live => "live",
        RedactionState::Redacted => "redacted",
        RedactionState::RetainedMetadataOnly => "retained-metadata-only",
    }
}
