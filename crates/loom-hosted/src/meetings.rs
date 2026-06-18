use std::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "inference")]
use std::sync::Arc;

use loom_core::error::{Code, LoomError, Result};
use loom_core::workspace::{AclDomain, FacetKind, WorkspaceId};
use loom_core::{
    AclResource, AclResourceScope, AclRight, AclScopeKind, Digest, FieldValue, Loom,
    Value as LoomValue, document, graph, ledger, search, vector_get,
};
#[cfg(feature = "inference")]
use loom_core::{Metric, vector_create, vector_delete, vector_upsert_text};
use loom_sql::LoomSqlStore;
use loom_store::FileStore;
use loom_substrate::meetings::{
    AnnotationRecord, AnnotationStatus, EntityMergeInput, EntityMergeRecord,
    ExtractionReviewProjection, MeetingRecord, MeetingStatus, MeetingsProfileSnapshot,
    ProjectionAction, ProjectionKind, ProjectionOutput, ProjectionOutputSet, RedactionState,
    VocabularyTermInput, VocabularyTermRecord, VocabularyTermStatus, meetings_profile_key,
};
use loom_substrate::search::{
    EMBEDDING_PROJECTION_JOBS_DIR, EmbeddingProjectionJob, EmbeddingProjectionKey,
    EmbeddingProjectionStamp,
};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingsProjectionOutput {
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

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingSummary {
    pub meeting_id: String,
    pub title: String,
    pub starts_at_ms: Option<u64>,
    pub ends_at_ms: Option<u64>,
    pub status: String,
    pub source_refs: Vec<String>,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingsList {
    pub workspace_id: String,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub meetings: Vec<HostedMeetingSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingAnnotation {
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

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingDetail {
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
    pub annotations: Vec<HostedMeetingAnnotation>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingsSearchHit {
    pub workspace_id: String,
    pub collection: String,
    pub meeting_id: String,
    pub field: String,
    pub snippet: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingsSearch {
    pub workspace_id: String,
    pub collection: String,
    pub query: String,
    pub hits: Vec<HostedMeetingsSearchHit>,
    pub degraded: bool,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingsProjection {
    pub workspace_id: String,
    pub profile_root: String,
    pub outputs: Vec<HostedMeetingsProjectionOutput>,
    pub output_set_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingsProjectionSkip {
    pub output_id: String,
    pub projection: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingsProjectionApply {
    pub workspace_id: String,
    pub profile_root: String,
    pub applied: usize,
    pub skipped: usize,
    pub already_applied: usize,
    pub document_writes: usize,
    pub file_writes: usize,
    pub graph_writes: usize,
    pub search_writes: usize,
    pub vector_jobs: usize,
    pub sql_dataframe_writes: usize,
    pub ledger_appends: usize,
    pub skipped_outputs: Vec<HostedMeetingsProjectionSkip>,
}

#[derive(Clone, Debug)]
pub struct HostedMeetingsEmbeddingRuntime {
    #[cfg(feature = "inference")]
    instance: loom_types::InferenceInstanceDescriptor,
    #[cfg(feature = "inference")]
    handle: Arc<loom_inference::TextEmbeddingHandle>,
}

impl HostedMeetingsEmbeddingRuntime {
    #[cfg(feature = "inference")]
    pub fn new(
        instance: loom_types::InferenceInstanceDescriptor,
        handle: loom_inference::TextEmbeddingHandle,
    ) -> Self {
        Self {
            instance,
            handle: Arc::new(handle),
        }
    }

    #[cfg(feature = "inference")]
    pub fn from_shared(
        instance: loom_types::InferenceInstanceDescriptor,
        handle: Arc<loom_inference::TextEmbeddingHandle>,
    ) -> Self {
        Self { instance, handle }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingsMaterializedOutput {
    pub output_id: String,
    pub projection: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub output_ref: String,
    pub state: String,
    pub artifact_ref: String,
    pub record_cbor_hex: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingsMaterializedOutputs {
    pub workspace_id: String,
    pub total: usize,
    pub materialized: usize,
    pub pending: usize,
    pub missing: usize,
    pub outputs: Vec<HostedMeetingsMaterializedOutput>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingsExtractionReview {
    pub workspace_id: String,
    pub suggested_annotation_ids: Vec<String>,
    pub accepted_annotation_ids: Vec<String>,
    pub rejected_annotation_ids: Vec<String>,
    pub vocabulary_terms: usize,
    pub review_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingsAnnotationReview {
    pub workspace_id: String,
    pub annotation_id: String,
    pub status: String,
    pub accepted_by: Option<String>,
    pub accepted_at_ms: Option<u64>,
    pub record_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingsVocabularyReview {
    pub workspace_id: String,
    pub term_id: String,
    pub status: String,
    pub reviewed_by: Option<String>,
    pub reviewed_at_ms: Option<u64>,
    pub record_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct HostedMeetingsEntityMergeWrite {
    pub workspace_id: String,
    pub merge_id: String,
    pub canonical_entity_id: String,
    pub merged_entity_ids: Vec<String>,
    pub record_cbor_hex: String,
}

pub fn list(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    limit: usize,
    offset: usize,
) -> Result<HostedMeetingsList> {
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
    Ok(HostedMeetingsList {
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
) -> Result<HostedMeetingDetail> {
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

pub fn search_meetings(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    query: &str,
    field: Option<&str>,
    limit: usize,
    offset: usize,
) -> Result<HostedMeetingsSearch> {
    authorize_meetings_read(loom, workspace, workspace_id)?;
    if query.is_empty() {
        return Err(LoomError::invalid("search query must not be empty"));
    }
    let lowered = query.to_ascii_lowercase();
    let mut hits = Vec::new();
    for id in search::search_ids(loom, workspace, workspace_id, None)? {
        let Some(doc) = search::search_get(loom, workspace, workspace_id, &id)? else {
            continue;
        };
        let meeting_id = String::from_utf8_lossy(&id).into_owned();
        for (field_name, value) in doc {
            if field.is_some_and(|wanted| wanted != field_name) {
                continue;
            }
            let FieldValue::Text(text) = value else {
                continue;
            };
            let text_lower = text.to_ascii_lowercase();
            let Some(start) = text_lower.find(&lowered) else {
                continue;
            };
            hits.push(HostedMeetingsSearchHit {
                workspace_id: workspace_id.to_string(),
                collection: workspace_id.to_string(),
                meeting_id: meeting_id.clone(),
                field: field_name,
                snippet: text_snippet(&text, start, start + lowered.len()),
            });
        }
    }
    hits.sort_by(|a, b| {
        a.meeting_id
            .cmp(&b.meeting_id)
            .then_with(|| a.field.cmp(&b.field))
    });
    let hits = hits.into_iter().skip(offset).take(limit).collect();
    Ok(HostedMeetingsSearch {
        workspace_id: workspace_id.to_string(),
        collection: workspace_id.to_string(),
        query: query.to_string(),
        hits,
        degraded: true,
        reason: "scan_backed_lexical".to_string(),
    })
}

pub fn projection_outputs(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<HostedMeetingsProjection> {
    authorize_meetings_read(loom, workspace, workspace_id)?;
    let snapshot = load_snapshot(loom, workspace_id)?;
    let profile_root = profile_root(loom, &snapshot)?;
    let output_set = ProjectionOutputSet::from_snapshot(&snapshot)?;
    Ok(HostedMeetingsProjection {
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

pub fn accept_annotation(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    annotation_id: &str,
) -> Result<HostedMeetingsAnnotationReview> {
    authorize_meetings_write(loom, workspace, workspace_id)?;
    let reviewer = loom.effective_principal()?.unwrap_or(workspace).to_string();
    let mut snapshot = load_snapshot(loom, workspace_id)?;
    let annotation = snapshot.accept_annotation(annotation_id, reviewer, crate::chat::now_ms())?;
    save_snapshot(
        loom,
        workspace,
        workspace_id,
        &snapshot,
        "meetings.annotation.accept",
        annotation_id,
    )?;
    annotation_review_summary(workspace_id, &annotation)
}

pub fn reject_annotation(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    annotation_id: &str,
) -> Result<HostedMeetingsAnnotationReview> {
    authorize_meetings_write(loom, workspace, workspace_id)?;
    let mut snapshot = load_snapshot(loom, workspace_id)?;
    let annotation = snapshot.reject_annotation(annotation_id)?;
    save_snapshot(
        loom,
        workspace,
        workspace_id,
        &snapshot,
        "meetings.annotation.reject",
        annotation_id,
    )?;
    annotation_review_summary(workspace_id, &annotation)
}

pub fn propose_vocabulary_term(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    input: VocabularyTermInput<'_>,
    aliases: Vec<String>,
) -> Result<HostedMeetingsVocabularyReview> {
    authorize_meetings_write(loom, workspace, workspace_id)?;
    let mut term = VocabularyTermRecord::new(input)?;
    term.proposed_by = Some(loom.effective_principal()?.unwrap_or(workspace).to_string());
    term.aliases = aliases;
    let mut snapshot = load_snapshot(loom, workspace_id)?;
    let term = snapshot.add_vocabulary_term(term)?;
    save_snapshot(
        loom,
        workspace,
        workspace_id,
        &snapshot,
        "meetings.vocabulary.propose",
        &term.term_id,
    )?;
    vocabulary_review_summary(workspace_id, &term)
}

pub fn accept_vocabulary_term(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    term_id: &str,
) -> Result<HostedMeetingsVocabularyReview> {
    authorize_meetings_write(loom, workspace, workspace_id)?;
    let reviewer = loom.effective_principal()?.unwrap_or(workspace).to_string();
    let mut snapshot = load_snapshot(loom, workspace_id)?;
    let term = snapshot.accept_vocabulary_term(term_id, reviewer, crate::chat::now_ms())?;
    save_snapshot(
        loom,
        workspace,
        workspace_id,
        &snapshot,
        "meetings.vocabulary.accept",
        term_id,
    )?;
    vocabulary_review_summary(workspace_id, &term)
}

pub fn reject_vocabulary_term(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    term_id: &str,
) -> Result<HostedMeetingsVocabularyReview> {
    authorize_meetings_write(loom, workspace, workspace_id)?;
    let reviewer = loom.effective_principal()?.unwrap_or(workspace).to_string();
    let mut snapshot = load_snapshot(loom, workspace_id)?;
    let term = snapshot.reject_vocabulary_term(term_id, reviewer, crate::chat::now_ms())?;
    save_snapshot(
        loom,
        workspace,
        workspace_id,
        &snapshot,
        "meetings.vocabulary.reject",
        term_id,
    )?;
    vocabulary_review_summary(workspace_id, &term)
}

pub fn add_entity_merge(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    merge_id: &str,
    canonical_entity_id: &str,
    merged_entity_ids: Vec<String>,
    evidence_annotation_ids: Vec<String>,
) -> Result<HostedMeetingsEntityMergeWrite> {
    authorize_meetings_write(loom, workspace, workspace_id)?;
    let decided_by = loom.effective_principal()?.unwrap_or(workspace).to_string();
    let input = EntityMergeInput {
        merge_id,
        canonical_entity_id,
        merged_entity_ids,
        evidence_annotation_ids,
        decided_by: &decided_by,
        decided_at_ms: crate::chat::now_ms(),
    };
    let mut snapshot = load_snapshot(loom, workspace_id)?;
    let merge = snapshot.add_entity_merge(EntityMergeRecord::new(input)?)?;
    save_snapshot(
        loom,
        workspace,
        workspace_id,
        &snapshot,
        "meetings.entity.merge",
        &merge.merge_id,
    )?;
    entity_merge_summary(workspace_id, &merge)
}

pub fn apply_projection_outputs(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<HostedMeetingsProjectionApply> {
    apply_projection_outputs_with_runtime(loom, workspace, workspace_id, None)
}

pub fn apply_projection_outputs_with_runtime(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    runtime: Option<&HostedMeetingsEmbeddingRuntime>,
) -> Result<HostedMeetingsProjectionApply> {
    authorize_meetings_write(loom, workspace, workspace_id)?;
    let snapshot = load_snapshot(loom, workspace_id)?;
    if snapshot.workspace_id != workspace_id {
        return Err(LoomError::invalid(
            "meetings snapshot organization mismatch",
        ));
    }
    let profile_root = profile_root(loom, &snapshot)?;
    let output_set = ProjectionOutputSet::from_snapshot(&snapshot)?;
    let mut summary =
        HostedMeetingsProjectionApply::new(output_set.workspace_id.clone(), profile_root);
    let mut ledger_applied = ledger_applied_output_ids(loom, workspace, workspace_id)?;
    let mut sql_dataframe = None;
    for output in &output_set.outputs {
        match output.projection {
            ProjectionKind::Document => {
                apply_document_projection(loom, workspace, workspace_id, output, &mut summary)?
            }
            ProjectionKind::Files => {
                apply_file_projection(loom, workspace, workspace_id, output, &mut summary)?
            }
            ProjectionKind::Graph => {
                apply_graph_projection(loom, workspace, workspace_id, output, &mut summary)?
            }
            ProjectionKind::Search => {
                apply_search_projection(loom, workspace, workspace_id, output, &mut summary)?
            }
            ProjectionKind::Ledger => apply_ledger_projection(
                loom,
                workspace,
                workspace_id,
                output,
                &mut ledger_applied,
                &mut summary,
            )?,
            ProjectionKind::SqlDataframe => apply_sql_dataframe_projection(
                loom,
                workspace,
                workspace_id,
                output,
                &mut sql_dataframe,
                &mut summary,
            )?,
            ProjectionKind::Vector => apply_vector_projection(
                loom,
                workspace,
                workspace_id,
                profile_root,
                output,
                runtime,
                &mut summary,
            )?,
        }
    }
    if let Some(mut sql_dataframe) = sql_dataframe
        && sql_dataframe.is_dirty()
    {
        sql_dataframe.persist(loom, workspace, &meetings_sql_database(workspace_id))?;
    }
    Ok(summary)
}

pub fn materialized_outputs(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<HostedMeetingsMaterializedOutputs> {
    authorize_meetings_read(loom, workspace, workspace_id)?;
    let snapshot = load_snapshot(loom, workspace_id)?;
    let profile_root = profile_root(loom, &snapshot)?;
    let output_set = ProjectionOutputSet::from_snapshot(&snapshot)?;
    let ledger_applied = ledger_applied_output_ids(loom, workspace, workspace_id)?;
    let outputs = output_set
        .outputs
        .iter()
        .map(|output| {
            materialized_output(
                loom,
                workspace,
                workspace_id,
                profile_root,
                output,
                &ledger_applied,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let materialized = outputs
        .iter()
        .filter(|output| output.state == "materialized")
        .count();
    let missing = outputs
        .iter()
        .filter(|output| output.state == "missing")
        .count();
    let pending = outputs.len().saturating_sub(materialized + missing);
    Ok(HostedMeetingsMaterializedOutputs {
        workspace_id: workspace_id.to_string(),
        total: outputs.len(),
        materialized,
        pending,
        missing,
        outputs,
    })
}

pub fn extraction_review(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
) -> Result<HostedMeetingsExtractionReview> {
    authorize_meetings_read(loom, workspace, workspace_id)?;
    let snapshot = load_snapshot(loom, workspace_id)?;
    let review = ExtractionReviewProjection::new(
        workspace_id,
        &snapshot.annotations,
        snapshot.vocabulary_terms.clone(),
        snapshot.entity_merges.clone(),
    )?;
    Ok(HostedMeetingsExtractionReview {
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

fn save_snapshot(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    snapshot: &MeetingsProfileSnapshot,
    action: &str,
    target_id: &str,
) -> Result<()> {
    let target = format!("{workspace_id}/{target_id}");
    loom.store().control_set_audited(
        &meetings_profile_key(workspace_id)?,
        snapshot.encode()?,
        loom.effective_principal()?.or(Some(workspace)),
        action,
        Some(&target),
    )?;
    Ok(())
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

fn authorize_meetings_write(
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
        AclRight::Write,
    )
}

impl HostedMeetingsProjectionApply {
    fn new(workspace_id: String, profile_root: Digest) -> Self {
        Self {
            workspace_id,
            profile_root: profile_root.to_string(),
            applied: 0,
            skipped: 0,
            already_applied: 0,
            document_writes: 0,
            file_writes: 0,
            graph_writes: 0,
            search_writes: 0,
            vector_jobs: 0,
            sql_dataframe_writes: 0,
            ledger_appends: 0,
            skipped_outputs: Vec::new(),
        }
    }

    fn applied_document(&mut self) {
        self.applied += 1;
        self.document_writes += 1;
    }

    fn applied_file(&mut self) {
        self.applied += 1;
        self.file_writes += 1;
    }

    fn applied_graph(&mut self) {
        self.applied += 1;
        self.graph_writes += 1;
    }

    fn applied_search(&mut self) {
        self.applied += 1;
        self.search_writes += 1;
    }

    fn applied_vector_job(&mut self) {
        self.applied += 1;
        self.vector_jobs += 1;
    }

    fn applied_sql_dataframe(&mut self) {
        self.applied += 1;
        self.sql_dataframe_writes += 1;
    }

    fn applied_ledger(&mut self) {
        self.applied += 1;
        self.ledger_appends += 1;
    }

    fn skip(&mut self, output: &ProjectionOutput, reason: &str) {
        self.skipped += 1;
        self.skipped_outputs.push(HostedMeetingsProjectionSkip {
            output_id: output.output_id.clone(),
            projection: projection_kind(output.projection).to_string(),
            reason: reason.to_string(),
        });
    }

    fn already_applied(&mut self) {
        self.already_applied += 1;
    }
}

fn apply_document_projection(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    output: &ProjectionOutput,
    summary: &mut HostedMeetingsProjectionApply,
) -> Result<()> {
    ensure_facet(loom, workspace, FacetKind::Document)?;
    let id = output_ref_suffix(&output.output_ref, "document:")?;
    match output.action {
        ProjectionAction::Upsert | ProjectionAction::RetainMetadata => {
            document::doc_put(loom, workspace, collection, id, output.encode()?)?;
        }
        ProjectionAction::Invalidate => {
            document::doc_delete(loom, workspace, collection, id)?;
        }
        ProjectionAction::Append => {
            summary.skip(output, "document projection does not support append");
            return Ok(());
        }
    }
    summary.applied_document();
    Ok(())
}

fn apply_file_projection(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    output: &ProjectionOutput,
    summary: &mut HostedMeetingsProjectionApply,
) -> Result<()> {
    let suffix = output
        .output_ref
        .strip_prefix("files/")
        .or_else(|| output.output_ref.strip_prefix("files:"))
        .ok_or_else(|| {
            LoomError::invalid("meetings file output_ref must start with files/ or files:")
        })?;
    let path = format!("meetings/{workspace_id}/{suffix}.cbor");
    match output.action {
        ProjectionAction::Upsert | ProjectionAction::RetainMetadata => {
            ensure_parent_dirs(loom, workspace, &path)?;
            loom.write_file(workspace, &path, &output.encode()?, 0o100644)?;
        }
        ProjectionAction::Invalidate => {
            loom.remove_file(workspace, &path)?;
        }
        ProjectionAction::Append => {
            summary.skip(output, "file projection does not support append");
            return Ok(());
        }
    }
    summary.applied_file();
    Ok(())
}

fn apply_graph_projection(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    graph_name: &str,
    output: &ProjectionOutput,
    summary: &mut HostedMeetingsProjectionApply,
) -> Result<()> {
    ensure_facet(loom, workspace, FacetKind::Graph)?;
    let id = output_ref_suffix(&output.output_ref, "graph:")?;
    match output.action {
        ProjectionAction::Upsert | ProjectionAction::RetainMetadata => {
            graph::graph_upsert_node(loom, workspace, graph_name, id, graph_props(output)?)?;
        }
        ProjectionAction::Invalidate => {
            match graph::graph_remove_node(loom, workspace, graph_name, id, true) {
                Ok(()) => {}
                Err(err) if err.code == Code::NotFound => {}
                Err(err) => return Err(err),
            }
        }
        ProjectionAction::Append => {
            summary.skip(output, "graph projection does not support append");
            return Ok(());
        }
    }
    summary.applied_graph();
    Ok(())
}

fn apply_search_projection(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    output: &ProjectionOutput,
    summary: &mut HostedMeetingsProjectionApply,
) -> Result<()> {
    ensure_facet(loom, workspace, FacetKind::Search)?;
    let id = output_ref_suffix(&output.output_ref, "search:")?
        .as_bytes()
        .to_vec();
    match output.action {
        ProjectionAction::Upsert | ProjectionAction::RetainMetadata => {
            ensure_search_collection(loom, workspace, collection)?;
            search::search_index(loom, workspace, collection, id, search_document(output)?)?;
        }
        ProjectionAction::Invalidate => {
            match search::search_delete(loom, workspace, collection, &id) {
                Ok(_) => {}
                Err(err) if err.code == Code::NotFound => {}
                Err(err) => return Err(err),
            }
        }
        ProjectionAction::Append => {
            summary.skip(output, "search projection does not support append");
            return Ok(());
        }
    }
    summary.applied_search();
    Ok(())
}

fn apply_vector_projection(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    profile_root: Digest,
    output: &ProjectionOutput,
    runtime: Option<&HostedMeetingsEmbeddingRuntime>,
    summary: &mut HostedMeetingsProjectionApply,
) -> Result<()> {
    let _ = runtime;
    #[cfg(feature = "inference")]
    if let Some(runtime) = runtime {
        return apply_vector_projection_with_runtime(
            loom,
            workspace,
            workspace_id,
            profile_root,
            output,
            runtime,
            summary,
        );
    }

    let key =
        EmbeddingProjectionKey::new(workspace_id, "meetings", workspace_id, &output.output_id)?;
    let stamp = EmbeddingProjectionStamp::new(
        profile_root,
        "loom-built-in-embedding",
        None,
        "unconfigured",
    )?;
    let job = EmbeddingProjectionJob::queued(key, stamp)
        .no_engine("built-in embedding inference is not configured")?;
    let path = job.job_path(loom.store().digest_algo())?;
    match loom.read_file_reserved(workspace, &path) {
        Ok(_) => {
            summary.already_applied();
            return Ok(());
        }
        Err(err) if err.code == Code::NotFound => {}
        Err(err) => return Err(err),
    }
    loom.create_directory_reserved(workspace, EMBEDDING_PROJECTION_JOBS_DIR, true)?;
    loom.write_file_reserved(workspace, &path, &job.encode()?, 0o100644)?;
    summary.applied_vector_job();
    Ok(())
}

#[cfg(feature = "inference")]
fn apply_vector_projection_with_runtime(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    profile_root: Digest,
    output: &ProjectionOutput,
    runtime: &HostedMeetingsEmbeddingRuntime,
    summary: &mut HostedMeetingsProjectionApply,
) -> Result<()> {
    let model = runtime
        .handle
        .model()
        .ok_or_else(|| LoomError::unsupported("text embedding provider did not expose a model"))?;
    ensure_facet(loom, workspace, FacetKind::Vector)?;
    let collection = meetings_vector_collection(workspace_id);
    match vector_create(
        loom,
        workspace,
        &collection,
        model.dimension,
        Metric::Cosine,
    ) {
        Ok(()) => {}
        Err(err) if err.code == Code::Conflict => {}
        Err(err) => return Err(err),
    }
    let job = meetings_vector_projection_job(workspace_id, profile_root, output, runtime)?;
    let path = job.job_path(loom.store().digest_algo())?;
    match output.action {
        ProjectionAction::Upsert | ProjectionAction::Append => {
            vector_upsert_text(
                loom,
                workspace,
                &collection,
                &meetings_vector_id(output),
                &output.text_body(),
                meetings_vector_metadata(output),
                &runtime.handle,
            )?;
        }
        ProjectionAction::Invalidate | ProjectionAction::RetainMetadata => {
            let _ = vector_delete(loom, workspace, &collection, &meetings_vector_id(output))?;
        }
    }
    loom.create_directory_reserved(workspace, EMBEDDING_PROJECTION_JOBS_DIR, true)?;
    loom.write_file_reserved(workspace, &path, &job.ready().encode()?, 0o100644)?;
    summary.applied_vector_job();
    Ok(())
}

fn meetings_vector_collection(workspace_id: &str) -> String {
    format!("meetings/{workspace_id}")
}

fn meetings_vector_id(output: &ProjectionOutput) -> String {
    output
        .output_ref
        .strip_prefix("vector:")
        .unwrap_or(&output.output_ref)
        .to_string()
}

#[cfg(feature = "inference")]
fn meetings_vector_metadata(output: &ProjectionOutput) -> BTreeMap<String, LoomValue> {
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "entity_kind".to_string(),
        LoomValue::Text(output.entity_kind.clone()),
    );
    metadata.insert(
        "entity_id".to_string(),
        LoomValue::Text(output.entity_id.clone()),
    );
    metadata.insert(
        "output_ref".to_string(),
        LoomValue::Text(output.output_ref.clone()),
    );
    metadata.insert(
        "output_id".to_string(),
        LoomValue::Text(output.output_id.clone()),
    );
    metadata.insert(
        "source_ids".to_string(),
        LoomValue::List(
            output
                .source_ids
                .iter()
                .cloned()
                .map(LoomValue::Text)
                .collect(),
        ),
    );
    metadata
}

#[cfg(feature = "inference")]
fn meetings_vector_projection_job(
    workspace_id: &str,
    profile_root: Digest,
    output: &ProjectionOutput,
    runtime: &HostedMeetingsEmbeddingRuntime,
) -> Result<EmbeddingProjectionJob> {
    let key =
        EmbeddingProjectionKey::new(workspace_id, "meetings", workspace_id, &output.output_id)?;
    let stamp = embedding_stamp_for_instance(profile_root, &runtime.instance)?;
    Ok(EmbeddingProjectionJob::queued(key, stamp))
}

#[cfg(feature = "inference")]
fn embedding_stamp_for_instance(
    content_digest: Digest,
    instance: &loom_types::InferenceInstanceDescriptor,
) -> Result<EmbeddingProjectionStamp> {
    let descriptor_bytes = serde_json::to_vec(instance).map_err(|err| {
        LoomError::invalid(format!(
            "embedding instance descriptor encode failed: {err}"
        ))
    })?;
    let descriptor_digest = Digest::hash(content_digest.algo(), &descriptor_bytes);
    EmbeddingProjectionStamp::new(
        content_digest,
        format!(
            "{}@{}",
            instance.model.repo_id,
            instance.model.revision.value()
        ),
        None,
        format!(
            "{}:{}",
            instance.runtime.as_str(),
            descriptor_digest.to_hex()
        ),
    )
}

fn apply_ledger_projection(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    output: &ProjectionOutput,
    applied_ids: &mut BTreeSet<String>,
    summary: &mut HostedMeetingsProjectionApply,
) -> Result<()> {
    ensure_facet(loom, workspace, FacetKind::Ledger)?;
    if output.action != ProjectionAction::Append {
        summary.skip(output, "ledger projection only supports append");
        return Ok(());
    }
    if applied_ids.contains(&output.output_id) {
        summary.already_applied();
        return Ok(());
    }
    ledger::ledger_append(loom, workspace, collection, output.encode()?)?;
    applied_ids.insert(output.output_id.clone());
    summary.applied_ledger();
    Ok(())
}

fn apply_sql_dataframe_projection(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    output: &ProjectionOutput,
    sql_dataframe: &mut Option<LoomSqlStore>,
    summary: &mut HostedMeetingsProjectionApply,
) -> Result<()> {
    let store = meetings_sql_store(loom, workspace, workspace_id, sql_dataframe)?;
    match output.action {
        ProjectionAction::Upsert | ProjectionAction::RetainMetadata => {
            store.exec_cbor(&format!(
                "DELETE FROM meetings_projection_outputs WHERE output_id = {}; INSERT INTO meetings_projection_outputs VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
                sql_text(&output.output_id),
                sql_text(&output.output_id),
                sql_text(projection_kind(output.projection)),
                sql_text(projection_action(output.action)),
                sql_text(&output.output_ref),
                sql_text(&output.entity_kind),
                sql_text(&output.entity_id),
                sql_text(&source_ids_text(&output.source_ids)),
                sql_text(&hex::encode(loom_codec::encode(&output.payload).map_err(|err| {
                    LoomError::corrupt(format!("projection output payload encode failed: {err}"))
                })?)),
                sql_text(&output.encode().map(hex::encode)?),
                sql_text(output.redaction_state.map(redaction_state).unwrap_or("")),
                output.recorded_at_ms
            ))?;
        }
        ProjectionAction::Invalidate => {
            store.exec_cbor(&format!(
                "DELETE FROM meetings_projection_outputs WHERE output_id = {}",
                sql_text(&output.output_id)
            ))?;
        }
        ProjectionAction::Append => {
            summary.skip(output, "sql-dataframe projection does not support append");
            return Ok(());
        }
    }
    summary.applied_sql_dataframe();
    Ok(())
}

fn materialized_output(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    profile_root: Digest,
    output: &ProjectionOutput,
    ledger_applied: &BTreeSet<String>,
) -> Result<HostedMeetingsMaterializedOutput> {
    let (state, artifact_ref, record_cbor_hex) = match output.projection {
        ProjectionKind::Document => materialized_document(loom, workspace, workspace_id, output)?,
        ProjectionKind::Files => materialized_file(loom, workspace, workspace_id, output)?,
        ProjectionKind::Graph => materialized_graph(loom, workspace, workspace_id, output)?,
        ProjectionKind::Search => materialized_search(loom, workspace, workspace_id, output)?,
        ProjectionKind::Vector => {
            materialized_vector_job(loom, workspace, workspace_id, profile_root, output)?
        }
        ProjectionKind::SqlDataframe => {
            materialized_sql_dataframe(loom, workspace, workspace_id, output)?
        }
        ProjectionKind::Ledger => materialized_ledger(workspace_id, output, ledger_applied),
    };
    Ok(HostedMeetingsMaterializedOutput {
        output_id: output.output_id.clone(),
        projection: projection_kind(output.projection).to_string(),
        entity_kind: output.entity_kind.clone(),
        entity_id: output.entity_id.clone(),
        output_ref: output.output_ref.clone(),
        state,
        artifact_ref,
        record_cbor_hex,
    })
}

fn materialized_document(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    output: &ProjectionOutput,
) -> Result<(String, String, Option<String>)> {
    let id = output_ref_suffix(&output.output_ref, "document:")?;
    let found = document::doc_get(loom, workspace, collection, id)?;
    Ok((
        materialized_state_for_output(output, found.is_some()),
        format!("document:{collection}/{id}"),
        found.map(hex::encode),
    ))
}

fn materialized_file(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    output: &ProjectionOutput,
) -> Result<(String, String, Option<String>)> {
    let suffix = output
        .output_ref
        .strip_prefix("files/")
        .or_else(|| output.output_ref.strip_prefix("files:"))
        .ok_or_else(|| {
            LoomError::invalid("meetings file output_ref must start with files/ or files:")
        })?;
    let path = format!("meetings/{workspace_id}/{suffix}.cbor");
    match loom.read_file(workspace, &path) {
        Ok(bytes) => Ok((
            materialized_state_for_output(output, true),
            path,
            Some(hex::encode(bytes)),
        )),
        Err(err) if err.code == Code::NotFound => {
            Ok((materialized_state_for_output(output, false), path, None))
        }
        Err(err) => Err(err),
    }
}

fn materialized_graph(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    graph_name: &str,
    output: &ProjectionOutput,
) -> Result<(String, String, Option<String>)> {
    let id = output_ref_suffix(&output.output_ref, "graph:")?;
    let props = graph::graph_get_node(loom, workspace, graph_name, id)?;
    let record_cbor_hex = props
        .as_ref()
        .and_then(|props| match props.get("record_cbor") {
            Some(graph::GraphValue::Bytes(bytes)) => Some(hex::encode(bytes)),
            _ => None,
        });
    Ok((
        materialized_state_for_output(output, props.is_some()),
        format!("graph:{graph_name}/{id}"),
        record_cbor_hex,
    ))
}

fn materialized_search(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    output: &ProjectionOutput,
) -> Result<(String, String, Option<String>)> {
    let id = output_ref_suffix(&output.output_ref, "search:")?
        .as_bytes()
        .to_vec();
    let found = search::search_get(loom, workspace, collection, &id)?;
    Ok((
        materialized_state_for_output(output, found.is_some()),
        format!("fts:{collection}/{}", String::from_utf8_lossy(&id)),
        None,
    ))
}

fn materialized_vector_job(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    profile_root: Digest,
    output: &ProjectionOutput,
) -> Result<(String, String, Option<String>)> {
    let collection = meetings_vector_collection(workspace_id);
    let id = meetings_vector_id(output);
    match vector_get(loom, workspace, &collection, &id) {
        Ok(Some(_)) => {
            return Ok((
                "materialized".to_string(),
                format!("vector:{collection}/{id}"),
                None,
            ));
        }
        Ok(None) => {}
        Err(err) if err.code == Code::NotFound => {}
        Err(err) => return Err(err),
    }
    let key =
        EmbeddingProjectionKey::new(workspace_id, "meetings", workspace_id, &output.output_id)?;
    if let Some((path, job, bytes)) = matching_embedding_projection_job(loom, workspace, &key)? {
        let state = if output.action == ProjectionAction::Invalidate
            && job.state == loom_substrate::search::EmbeddingProjectionState::Ready
        {
            "materialized".to_string()
        } else {
            job.state.as_str().to_string()
        };
        return Ok((state, path, Some(hex::encode(bytes))));
    }
    let stamp = EmbeddingProjectionStamp::new(
        profile_root,
        "loom-built-in-embedding",
        None,
        "unconfigured",
    )?;
    let job = EmbeddingProjectionJob::queued(key, stamp);
    let path = job.job_path(loom.store().digest_algo())?;
    match loom.read_file_reserved(workspace, &path) {
        Ok(bytes) => {
            let job = EmbeddingProjectionJob::decode(&bytes)?;
            Ok((
                job.state.as_str().to_string(),
                path,
                Some(hex::encode(bytes)),
            ))
        }
        Err(err) if err.code == Code::NotFound => Ok(("missing".to_string(), path, None)),
        Err(err) => Err(err),
    }
}

fn matching_embedding_projection_job(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    key: &EmbeddingProjectionKey,
) -> Result<Option<(String, EmbeddingProjectionJob, Vec<u8>)>> {
    let entries = match loom.list_directory(workspace, EMBEDDING_PROJECTION_JOBS_DIR) {
        Ok(entries) => entries,
        Err(err) if err.code == Code::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    for entry in entries {
        if entry.kind != loom_core::FileKind::File {
            continue;
        }
        let path = format!("{EMBEDDING_PROJECTION_JOBS_DIR}/{}", entry.name);
        let bytes = loom.read_file_reserved(workspace, &path)?;
        let job = EmbeddingProjectionJob::decode(&bytes)?;
        if &job.key == key {
            return Ok(Some((path, job, bytes)));
        }
    }
    Ok(None)
}

fn materialized_sql_dataframe(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    output: &ProjectionOutput,
) -> Result<(String, String, Option<String>)> {
    let artifact_ref = format!(
        "sql-dataframe:{}/meetings_projection_outputs",
        meetings_sql_database(workspace_id)
    );
    let mut store = match LoomSqlStore::load_eager_read(
        loom,
        workspace,
        &meetings_sql_database(workspace_id),
    ) {
        Ok(store) => store,
        Err(err) if err.code == Code::NotFound || err.code == Code::SqlTableNotFound => {
            return Ok(("missing".to_string(), artifact_ref, None));
        }
        Err(err) => return Err(err),
    };
    let rows = match store.select_rows(&format!(
        "SELECT record_cbor_hex FROM meetings_projection_outputs WHERE output_id = {}",
        sql_text(&output.output_id)
    )) {
        Ok(rows) => rows,
        Err(err) if err.code == Code::SqlTableNotFound => Vec::new(),
        Err(err) => return Err(err),
    };
    let record_cbor_hex = rows.first().and_then(|row| match row.first() {
        Some(LoomValue::Text(value)) => Some(value.clone()),
        _ => None,
    });
    Ok((
        materialized_state_for_output(output, record_cbor_hex.is_some()),
        artifact_ref,
        record_cbor_hex,
    ))
}

fn materialized_ledger(
    collection: &str,
    output: &ProjectionOutput,
    ledger_applied: &BTreeSet<String>,
) -> (String, String, Option<String>) {
    (
        materialized_state(ledger_applied.contains(&output.output_id)),
        format!("ledger:{collection}"),
        None,
    )
}

fn materialized_state(found: bool) -> String {
    if found {
        "materialized".to_string()
    } else {
        "missing".to_string()
    }
}

fn materialized_state_for_output(output: &ProjectionOutput, found: bool) -> String {
    if output.action == ProjectionAction::Invalidate {
        materialized_state(!found)
    } else {
        materialized_state(found)
    }
}

fn meetings_sql_store<'a>(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    sql_dataframe: &'a mut Option<LoomSqlStore>,
) -> Result<&'a mut LoomSqlStore> {
    if sql_dataframe.is_none() {
        ensure_facet(loom, workspace, FacetKind::Sql)?;
        let mut store =
            LoomSqlStore::load_eager_write(loom, workspace, &meetings_sql_database(workspace_id))?;
        ensure_meetings_sql_table(&mut store)?;
        *sql_dataframe = Some(store);
    }
    Ok(sql_dataframe
        .as_mut()
        .expect("meetings SQL store initialized"))
}

fn ensure_meetings_sql_table(store: &mut LoomSqlStore) -> Result<()> {
    match store.exec_cbor("SELECT output_id FROM meetings_projection_outputs") {
        Ok(_) => Ok(()),
        Err(err) if err.code == Code::SqlTableNotFound => store
            .exec_cbor(
                "CREATE TABLE meetings_projection_outputs (
                output_id TEXT PRIMARY KEY,
                projection TEXT,
                action TEXT,
                output_ref TEXT,
                entity_kind TEXT,
                entity_id TEXT,
                source_ids TEXT,
                payload_cbor_hex TEXT,
                record_cbor_hex TEXT,
                redaction_state TEXT,
                recorded_at_ms INTEGER
            )",
            )
            .map(|_| ()),
        Err(err) => Err(err),
    }
}

fn meetings_sql_database(workspace_id: &str) -> String {
    format!("meetings/{workspace_id}")
}

fn source_ids_text(source_ids: &[String]) -> String {
    source_ids.join("\n")
}

fn sql_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn ledger_applied_output_ids(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
) -> Result<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let len = match ledger::ledger_len(loom, workspace, collection) {
        Ok(len) => len,
        Err(err) if err.code == Code::NotFound => return Ok(ids),
        Err(err) => return Err(err),
    };
    for seq in 0..len {
        let Some(bytes) = ledger::ledger_get(loom, workspace, collection, seq)? else {
            continue;
        };
        if let Ok(output) = ProjectionOutput::decode(&bytes) {
            ids.insert(output.output_id);
        }
    }
    Ok(ids)
}

fn ensure_facet(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    facet: FacetKind,
) -> Result<()> {
    if !loom.registry().has_facet(workspace, facet)? {
        loom.registry_mut().add_facet(workspace, facet)?;
    }
    Ok(())
}

fn ensure_parent_dirs(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    path: &str,
) -> Result<()> {
    if let Some((parent, _)) = path.rsplit_once('/') {
        loom.create_directory(workspace, parent, true)?;
    }
    Ok(())
}

fn ensure_search_collection(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
) -> Result<()> {
    match search::get_search(loom, workspace, collection) {
        Ok(_) => Ok(()),
        Err(err) if err.code == Code::NotFound => {
            search::search_create(loom, workspace, collection, search_mapping())
        }
        Err(err) => Err(err),
    }
}

fn output_ref_suffix<'a>(output_ref: &'a str, prefix: &str) -> Result<&'a str> {
    output_ref.strip_prefix(prefix).ok_or_else(|| {
        LoomError::invalid(format!(
            "meetings projection output_ref must start with {prefix}"
        ))
    })
}

fn graph_props(output: &ProjectionOutput) -> Result<graph::Props> {
    let mut props = BTreeMap::new();
    props.insert(
        "output_id".to_string(),
        graph::GraphValue::Text(output.output_id.clone()),
    );
    props.insert(
        "projection".to_string(),
        graph::GraphValue::Text(projection_kind(output.projection).to_string()),
    );
    props.insert(
        "action".to_string(),
        graph::GraphValue::Text(projection_action(output.action).to_string()),
    );
    props.insert(
        "entity_kind".to_string(),
        graph::GraphValue::Text(output.entity_kind.clone()),
    );
    props.insert(
        "entity_id".to_string(),
        graph::GraphValue::Text(output.entity_id.clone()),
    );
    props.insert(
        "output_ref".to_string(),
        graph::GraphValue::Text(output.output_ref.clone()),
    );
    props.insert(
        "record_cbor".to_string(),
        graph::GraphValue::Bytes(output.encode()?),
    );
    Ok(props)
}

fn search_mapping() -> search::Mapping {
    let mut mapping = BTreeMap::new();
    mapping.insert("entity_kind".to_string(), search::FieldMapping::keyword());
    mapping.insert("entity_id".to_string(), search::FieldMapping::keyword());
    mapping.insert("output_ref".to_string(), search::FieldMapping::keyword());
    mapping.insert("source_ids".to_string(), search::FieldMapping::text());
    mapping.insert("body".to_string(), search::FieldMapping::text());
    mapping
}

fn search_document(output: &ProjectionOutput) -> Result<search::Document> {
    let mut document = BTreeMap::new();
    document.insert(
        "entity_kind".to_string(),
        search::FieldValue::Text(output.entity_kind.clone()),
    );
    document.insert(
        "entity_id".to_string(),
        search::FieldValue::Text(output.entity_id.clone()),
    );
    document.insert(
        "output_ref".to_string(),
        search::FieldValue::Text(output.output_ref.clone()),
    );
    document.insert(
        "source_ids".to_string(),
        search::FieldValue::Text(output.source_ids.join(" ")),
    );
    document.insert(
        "body".to_string(),
        search::FieldValue::Text(search_body(output)?),
    );
    Ok(document)
}

fn search_body(output: &ProjectionOutput) -> Result<String> {
    Ok(output.text_body())
}

fn text_snippet(text: &str, start: usize, end: usize) -> String {
    let prefix = text[..start]
        .char_indices()
        .rev()
        .nth(40)
        .map_or(0, |(idx, _)| idx);
    let suffix = text[end..]
        .char_indices()
        .nth(40)
        .map_or(text.len(), |(idx, _)| end + idx);
    text[prefix..suffix].to_string()
}

fn annotation_review_summary(
    workspace_id: &str,
    annotation: &AnnotationRecord,
) -> Result<HostedMeetingsAnnotationReview> {
    Ok(HostedMeetingsAnnotationReview {
        workspace_id: workspace_id.to_string(),
        annotation_id: annotation.annotation_id.clone(),
        status: annotation_status(annotation.status).to_string(),
        accepted_by: annotation.accepted_by.clone(),
        accepted_at_ms: annotation.accepted_at_ms,
        record_cbor_hex: hex::encode(annotation.encode()?),
    })
}

fn vocabulary_review_summary(
    workspace_id: &str,
    term: &VocabularyTermRecord,
) -> Result<HostedMeetingsVocabularyReview> {
    Ok(HostedMeetingsVocabularyReview {
        workspace_id: workspace_id.to_string(),
        term_id: term.term_id.clone(),
        status: vocabulary_status(term.status).to_string(),
        reviewed_by: term.reviewed_by.clone(),
        reviewed_at_ms: term.reviewed_at_ms,
        record_cbor_hex: hex::encode(term.encode()?),
    })
}

fn entity_merge_summary(
    workspace_id: &str,
    merge: &EntityMergeRecord,
) -> Result<HostedMeetingsEntityMergeWrite> {
    Ok(HostedMeetingsEntityMergeWrite {
        workspace_id: workspace_id.to_string(),
        merge_id: merge.merge_id.clone(),
        canonical_entity_id: merge.canonical_entity_id.clone(),
        merged_entity_ids: merge.merged_entity_ids.clone(),
        record_cbor_hex: hex::encode(merge.encode()?),
    })
}

fn meeting_summary(meeting: &MeetingRecord) -> HostedMeetingSummary {
    HostedMeetingSummary {
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
) -> HostedMeetingDetail {
    HostedMeetingDetail {
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
            .map(meeting_annotation)
            .collect(),
    }
}

fn meeting_annotation(annotation: &AnnotationRecord) -> HostedMeetingAnnotation {
    HostedMeetingAnnotation {
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

fn projection_output_summary(output: &ProjectionOutput) -> Result<HostedMeetingsProjectionOutput> {
    Ok(HostedMeetingsProjectionOutput {
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

fn vocabulary_status(status: VocabularyTermStatus) -> &'static str {
    match status {
        VocabularyTermStatus::Proposed => "proposed",
        VocabularyTermStatus::Accepted => "accepted",
        VocabularyTermStatus::Rejected => "rejected",
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
