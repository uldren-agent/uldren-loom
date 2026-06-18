//! Durable reference-reconciliation records and bounded scheduling.

use loom_core::acl::{AclResource, AclResourceScope, AclScopeKind};
use loom_core::document::{doc_get, doc_list, doc_put};
use loom_core::error::{Code, LoomError, Result};
use loom_core::tabular::{ColumnType, Predicate, RowCursor, Schema, Table, Value as TableValue};
use loom_core::workspace::{FacetKind, WorkspaceId};
use loom_core::{
    AclRight, Digest, GraphValue, Loom, Props, doc_delete, graph_edges, graph_remove_edge,
    graph_upsert_edge, graph_upsert_node,
};
use loom_store::FileStore;
use loom_substrate::refs::{
    EntityRef, MarkdownReferenceCandidate, ReferenceArtifactInput, ReferenceArtifactKind,
    ReferenceArtifactRecord, ReferenceEdge, ReferenceIndex, ReferenceResolution, ReferenceSource,
    UnresolvedReference, extract_markdown_reference_candidates,
};
use sha2::{Digest as ShaDigest, Sha256};

pub const RECONCILIATION_DIR: &str = ".loom/substrate/refs/reconciliation";
pub const INDEX_DIR: &str = ".loom/substrate/refs";
pub const INDEX_PATH: &str = ".loom/substrate/refs/index.lrefs";
pub const REFERENCE_GRAPH: &str = "entity-references";
pub const TARGETS_TABLE: &str = ".loom/substrate/refs/reconciliation/targets";
pub const RESOLUTIONS_TABLE: &str = ".loom/substrate/refs/reconciliation/resolutions";
pub const FAILURES_TABLE: &str = ".loom/substrate/refs/reconciliation/failures";
pub const MAX_ATTEMPTS: u32 = 8;
pub const REFERENCE_ARTIFACT_PROFILE_PREFIX: &str = "profile/references/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceArtifactCreateRequest<'a> {
    pub workspace_id: &'a str,
    pub record_id: &'a str,
    pub kind: ReferenceArtifactKind,
    pub label: &'a str,
    pub source_ref: &'a str,
    pub source_operation_id: &'a str,
    pub target_ref: Option<&'a str>,
    pub created_by: &'a str,
    pub created_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ReferenceArtifactSummary {
    pub workspace_id: String,
    pub record_id: String,
    pub kind: String,
    pub entity_ref: String,
    pub label: String,
    pub source_ref: String,
    pub source_operation_id: String,
    pub target_ref: Option<String>,
    pub created_by: String,
    pub created_at_ms: u64,
    pub record_cbor_hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct ReferenceTarget {
    pub source_profile: String,
    pub source_scope: String,
    pub next_attempt_ms: u64,
    pub pending: u64,
}

impl ReferenceTarget {
    pub fn from_candidate(candidate: &UnresolvedReference) -> Self {
        Self {
            source_profile: candidate.source.facet.clone(),
            source_scope: candidate.source.collection.clone(),
            next_attempt_ms: candidate.next_attempt_ms,
            pending: 0,
        }
    }

    pub fn candidate_table(&self) -> String {
        candidate_table(&self.source_profile, &self.source_scope)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ReconciliationSummary {
    pub pending: u64,
    pub resolved: u64,
    pub failed: u64,
    pub processed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedReference {
    pub candidate: UnresolvedReference,
    pub record: ReferenceResolution,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplicitReferenceInput<'a> {
    pub source: ReferenceSource,
    pub relation: &'a str,
    pub target: EntityRef,
    pub evidence: &'a str,
}

pub fn create_reference_artifact(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    request: ReferenceArtifactCreateRequest<'_>,
) -> Result<ReferenceArtifactSummary> {
    loom.authorize(workspace, FacetKind::Vcs, AclRight::Write)?;
    let source_ref = EntityRef::parse(request.source_ref)?;
    let target_ref = request.target_ref.map(EntityRef::parse).transpose()?;
    let record = ReferenceArtifactRecord::new(ReferenceArtifactInput {
        record_id: request.record_id,
        kind: request.kind,
        label: request.label,
        source_ref,
        source_operation_id: request.source_operation_id,
        target_ref,
        created_by: request.created_by,
        created_at_ms: request.created_at_ms,
    })?;
    let key = reference_artifact_key(request.workspace_id, record.kind, &record.record_id)?;
    if loom.store().control_get(&key)?.is_some() {
        return Err(LoomError::new(
            Code::AlreadyExists,
            "reference artifact already exists",
        ));
    }
    let body = record.encode()?;
    loom.store().control_set(&key, body.clone())?;
    reference_artifact_summary(request.workspace_id, record, body)
}

pub fn get_reference_artifact(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    workspace_id: &str,
    kind: ReferenceArtifactKind,
    record_id: &str,
) -> Result<Option<ReferenceArtifactSummary>> {
    loom.authorize(workspace, FacetKind::Vcs, AclRight::Read)?;
    let key = reference_artifact_key(workspace_id, kind, record_id)?;
    loom.store()
        .control_get(&key)?
        .map(|bytes| {
            let record = ReferenceArtifactRecord::decode(&bytes)?;
            reference_artifact_summary(workspace_id, record, bytes)
        })
        .transpose()
}

pub fn load_index(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
) -> Result<Option<ReferenceIndex>> {
    loom.authorize_file_path(workspace, INDEX_PATH, AclRight::Read)?;
    match loom.read_file_reserved(workspace, INDEX_PATH) {
        Ok(bytes) => ReferenceIndex::decode(&bytes).map(Some),
        Err(error) if error.code == Code::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn save_index(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    index: &ReferenceIndex,
) -> Result<()> {
    loom.authorize_file_path(workspace, INDEX_DIR, AclRight::Write)?;
    loom.authorize_file_path(workspace, INDEX_PATH, AclRight::Write)?;
    loom.create_directory_reserved(workspace, INDEX_DIR, true)?;
    loom.write_file_reserved(workspace, INDEX_PATH, &index.encode()?, 0o100644)
}

pub fn replace_explicit_references(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    source: ReferenceSource,
    references: &[ExplicitReferenceInput<'_>],
) -> Result<()> {
    let mut index = load_or_rebuild_index(loom, workspace)?;
    index.remove_source(&source);
    for reference in references {
        if reference.source != source {
            return Err(LoomError::invalid(
                "explicit reference source must match replacement source",
            ));
        }
        index.add(ReferenceEdge::new(
            reference.source.clone(),
            reference.target.clone(),
            reference.relation,
            0,
            reference.evidence.len(),
            reference.evidence,
        )?);
    }
    save_index(loom, workspace, &index)?;
    project_reference_index_edges(loom, workspace, &index)
}

pub fn references_to(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    target: &EntityRef,
) -> Result<Vec<ReferenceEdge>> {
    Ok(load_or_rebuild_index(loom, workspace)?.inbound(target))
}

pub fn references_from(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    source: &ReferenceSource,
) -> Result<Vec<ReferenceEdge>> {
    Ok(load_or_rebuild_index(loom, workspace)?.outbound(source))
}

pub fn project_reference_index_edges(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    index: &ReferenceIndex,
) -> Result<()> {
    loom.authorize(workspace, FacetKind::Graph, AclRight::Write)?;
    let mut expected = std::collections::BTreeSet::new();
    for edge in index.edges() {
        let edge_id = reference_graph_edge_id(edge);
        expected.insert(edge_id.clone());
        let source_ref = reference_source_ref(&edge.source);
        graph_upsert_node(
            loom,
            workspace,
            REFERENCE_GRAPH,
            &source_ref,
            reference_source_node_props(edge, &source_ref),
        )?;
        graph_upsert_node(
            loom,
            workspace,
            REFERENCE_GRAPH,
            &edge.target.as_str(),
            reference_target_node_props(edge),
        )?;
        graph_upsert_edge(
            loom,
            workspace,
            REFERENCE_GRAPH,
            &edge_id,
            &source_ref,
            &edge.target.as_str(),
            &edge.relation,
            reference_graph_edge_props(edge, &source_ref),
        )?;
    }
    for (edge_id, edge) in graph_edges(loom, workspace, REFERENCE_GRAPH)? {
        if edge
            .props
            .get("derived_from")
            .is_some_and(|value| value == &GraphValue::Text("references".to_string()))
            && !expected.contains(&edge_id)
        {
            graph_remove_edge(loom, workspace, REFERENCE_GRAPH, &edge_id)?;
        }
    }
    Ok(())
}

pub fn reconcile_reference_graph_projection(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
) -> Result<usize> {
    let index = load_or_rebuild_index(loom, workspace)?;
    let before = graph_edges(loom, workspace, REFERENCE_GRAPH)
        .map(|edges| edges.len())
        .unwrap_or(0);
    project_reference_index_edges(loom, workspace, &index)?;
    let after = graph_edges(loom, workspace, REFERENCE_GRAPH)
        .map(|edges| edges.len())
        .unwrap_or(0);
    Ok(before.max(after))
}

pub fn apply_resolved_edges(
    index: &mut ReferenceIndex,
    records: &[ResolvedReference],
) -> Result<()> {
    for resolved in records {
        let candidate = &resolved.candidate;
        let record = &resolved.record;
        index.add(ReferenceEdge::new(
            candidate.source.clone(),
            record.target.clone(),
            candidate.relation.clone(),
            usize::try_from(candidate.span_start)
                .map_err(|_| LoomError::corrupt("reference span is too large"))?,
            usize::try_from(candidate.span_end)
                .map_err(|_| LoomError::corrupt("reference span is too large"))?,
            candidate.evidence.clone(),
        )?);
    }
    Ok(())
}

/// The outcome of [`replace_text_indexed`]: how many occurrences were replaced and the new document's
/// content address (`algo:hex`). Mirrors the MCP `document_replace_text` tool output exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplaceTextOutcome {
    pub replacements: u64,
    pub digest: String,
}

/// `document.put` + overlay: store `doc` at `id` in `collection`, then refresh the document's outgoing
/// references from its (UTF-8) body text.
pub fn put_document_indexed(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    id: &str,
    doc: Vec<u8>,
) -> Result<()> {
    let text = std::str::from_utf8(&doc).ok().map(str::to_string);
    doc_put(loom, workspace, collection, id, doc)?;
    update_document_refs(loom, workspace, collection, id, text.as_deref())
}

/// `document.delete` + overlay: remove `id`; if it was present, drop its reference-index source. Returns
/// whether the document existed.
pub fn delete_document_indexed(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    id: &str,
) -> Result<bool> {
    let deleted = doc_delete(loom, workspace, collection, id)?;
    if deleted {
        update_document_refs(loom, workspace, collection, id, None)?;
    }
    Ok(deleted)
}

/// `document.replace_text` + overlay: verify `base_digest` matches the current document, apply the
/// find/replace (all occurrences when `replace_all`, else the first), store the result, and refresh the
/// document's references from the new text. Returns the replacement count and the new content address.
pub fn replace_text_indexed(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    id: &str,
    find: &str,
    replace: &str,
    replace_all: bool,
    base_digest: &str,
) -> Result<ReplaceTextOutcome> {
    if find.is_empty() {
        return Err(LoomError::invalid("document replace find text is empty"));
    }
    let expected = Digest::parse(base_digest)?;
    let Some(doc) = doc_get(loom, workspace, collection, id)? else {
        return Err(LoomError::not_found("document not found"));
    };
    let current = Digest::hash(loom.store().digest_algo(), &doc);
    if current != expected {
        return Err(LoomError::new(
            Code::Conflict,
            "document base digest does not match current document",
        ));
    }
    let text =
        String::from_utf8(doc).map_err(|_| LoomError::invalid("document is not utf-8 text"))?;
    let matches = text.matches(find).count();
    if matches == 0 {
        return Err(LoomError::no_such_field("document find text not found"));
    }
    let updated = if replace_all {
        text.replace(find, replace)
    } else {
        text.replacen(find, replace, 1)
    };
    let replacements = if replace_all { matches } else { 1 };
    let bytes = updated.into_bytes();
    let digest = Digest::hash(loom.store().digest_algo(), &bytes);
    let new_text = std::str::from_utf8(&bytes)
        .map_err(|_| LoomError::invalid("updated document is not utf-8 text"))?
        .to_string();
    doc_put(loom, workspace, collection, id, bytes)?;
    update_document_refs(loom, workspace, collection, id, Some(&new_text))?;
    Ok(ReplaceTextOutcome {
        replacements: replacements as u64,
        digest: digest.to_string(),
    })
}

/// `graph.upsert_edge` + overlay: insert/replace the edge, then refresh the edge's reference from its
/// `dst` entity target.
pub fn upsert_graph_edge_indexed(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    edge_id: &str,
    src: &str,
    dst: &str,
    label: &str,
    props: Props,
) -> Result<()> {
    graph_upsert_edge(loom, workspace, collection, edge_id, src, dst, label, props)?;
    update_graph_edge_refs(loom, workspace, collection, edge_id, src, dst, label)
}

/// `graph.remove_edge` + overlay: remove the edge; if it was present, drop its reference-index source.
/// Returns whether the edge existed.
pub fn remove_graph_edge_indexed(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    edge_id: &str,
) -> Result<bool> {
    let removed = graph_remove_edge(loom, workspace, collection, edge_id)?;
    if removed {
        remove_graph_edge_refs(loom, workspace, collection, edge_id)?;
    }
    Ok(removed)
}

/// Refresh the reference-index entries whose source is document `collection`/`id` body: drop the old
/// entries and, when `text` is present, re-extract markdown references from it.
pub fn update_document_refs(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    id: &str,
    text: Option<&str>,
) -> Result<()> {
    let source = ReferenceSource::new("document", collection, id, "body")?;
    let mut index = load_or_rebuild_index(loom, workspace)?;
    index.remove_source(&source);
    if let Some(text) = text {
        index.add_text_refs(source, "refers_to", text)?;
    }
    save_index(loom, workspace, &index)?;
    project_reference_index_edges(loom, workspace, &index)
}

/// Refresh the reference-index entry for graph edge `collection`/`edge_id`: drop the old source and, when
/// `dst` parses as an entity reference, add an edge from the `"{src} {label} {dst}"` evidence.
pub fn update_graph_edge_refs(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    edge_id: &str,
    src: &str,
    dst: &str,
    label: &str,
) -> Result<()> {
    let mut index = load_or_rebuild_index(loom, workspace)?;
    let source = ReferenceSource::new("graph", collection, edge_id, "edge")?;
    index.remove_source(&source);
    if let Ok(target) = EntityRef::parse(dst) {
        let evidence = format!("{src} {label} {dst}");
        let span_start = evidence.len() - dst.len();
        if let Ok(edge) =
            ReferenceEdge::new(source, target, label, span_start, evidence.len(), evidence)
        {
            index.add(edge);
        }
    }
    save_index(loom, workspace, &index)?;
    project_reference_index_edges(loom, workspace, &index)
}

/// Drop the reference-index source for graph edge `collection`/`edge_id`.
pub fn remove_graph_edge_refs(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    collection: &str,
    edge_id: &str,
) -> Result<()> {
    let mut index = load_or_rebuild_index(loom, workspace)?;
    let source = ReferenceSource::new("graph", collection, edge_id, "edge")?;
    index.remove_source(&source);
    save_index(loom, workspace, &index)?;
    project_reference_index_edges(loom, workspace, &index)
}

/// Load the workspace's reference index, or rebuild it from the document/graph facets when absent. Shared
/// by the indexed writes and by the reconciliation/alias paths so they see one index-materialization rule.
pub fn load_or_rebuild_index(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
) -> Result<ReferenceIndex> {
    match load_index(loom, workspace)? {
        Some(index) => Ok(index),
        None => rebuild_reference_index(loom, workspace),
    }
}

fn rebuild_reference_index(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
) -> Result<ReferenceIndex> {
    let mut index = ReferenceIndex::new();
    for collection in loom.list_collections(workspace, FacetKind::Document) {
        let documents = match doc_list(loom, workspace, &collection) {
            Ok(documents) => documents,
            Err(e) if matches!(e.code, Code::PermissionDenied | Code::NotFound) => continue,
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
    for collection in loom.list_collections(workspace, FacetKind::Graph) {
        if collection == REFERENCE_GRAPH {
            continue;
        }
        let edges = match graph_edges(loom, workspace, &collection) {
            Ok(edges) => edges,
            Err(e) if matches!(e.code, Code::PermissionDenied | Code::NotFound) => continue,
            Err(e) => return Err(e),
        };
        for (edge_id, edge) in edges {
            let source = ReferenceSource::new("graph", &collection, &edge_id, "edge")?;
            if let Ok(target) = EntityRef::parse(&edge.dst) {
                let evidence = format!("{} {} {}", edge.src, edge.label, edge.dst);
                let span_start = evidence.len() - edge.dst.len();
                if let Ok(edge) = ReferenceEdge::new(
                    source,
                    target,
                    edge.label,
                    span_start,
                    evidence.len(),
                    evidence,
                ) {
                    index.add(edge);
                }
            }
        }
    }
    Ok(index)
}

fn reference_graph_edge_id(edge: &ReferenceEdge) -> String {
    let mut hasher = Sha256::new();
    hasher.update(edge.source.facet.as_bytes());
    hasher.update([0]);
    hasher.update(edge.source.collection.as_bytes());
    hasher.update([0]);
    hasher.update(edge.source.entity_id.as_bytes());
    hasher.update([0]);
    hasher.update(edge.source.field.as_bytes());
    hasher.update([0]);
    hasher.update(edge.relation.as_bytes());
    hasher.update([0]);
    hasher.update(edge.target.kind.as_bytes());
    hasher.update([0]);
    hasher.update(edge.target.id.as_bytes());
    hasher.update([0]);
    hasher.update(edge.span_start.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(edge.span_end.to_string().as_bytes());
    let digest = hasher.finalize();
    format!("reference:{}", hex::encode(digest))
}

fn reference_source_ref(source: &ReferenceSource) -> String {
    format!(
        "{}:{}:{}",
        source.facet, source.collection, source.entity_id
    )
}

fn reference_graph_edge_props(edge: &ReferenceEdge, source_ref: &str) -> Props {
    let mut props = Props::new();
    props.insert(
        "derived_from".to_string(),
        GraphValue::Text("references".to_string()),
    );
    props.insert(
        "source_ref".to_string(),
        GraphValue::Text(source_ref.to_string()),
    );
    props.insert(
        "source_facet".to_string(),
        GraphValue::Text(edge.source.facet.clone()),
    );
    props.insert(
        "source_collection".to_string(),
        GraphValue::Text(edge.source.collection.clone()),
    );
    props.insert(
        "source_entity_id".to_string(),
        GraphValue::Text(edge.source.entity_id.clone()),
    );
    props.insert(
        "source_field".to_string(),
        GraphValue::Text(edge.source.field.clone()),
    );
    props.insert(
        "target_kind".to_string(),
        GraphValue::Text(edge.target.kind.clone()),
    );
    props.insert(
        "target_id".to_string(),
        GraphValue::Text(edge.target.id.clone()),
    );
    props.insert(
        "relation".to_string(),
        GraphValue::Text(edge.relation.clone()),
    );
    props.insert(
        "span_start".to_string(),
        GraphValue::Int(edge.span_start as i64),
    );
    props.insert(
        "span_end".to_string(),
        GraphValue::Int(edge.span_end as i64),
    );
    props.insert(
        "evidence".to_string(),
        GraphValue::Text(edge.evidence.clone()),
    );
    props
}

fn reference_source_node_props(edge: &ReferenceEdge, source_ref: &str) -> Props {
    let mut props = Props::new();
    props.insert(
        "kind".to_string(),
        GraphValue::Text("reference_source".to_string()),
    );
    props.insert(
        "source_ref".to_string(),
        GraphValue::Text(source_ref.to_string()),
    );
    props.insert(
        "facet".to_string(),
        GraphValue::Text(edge.source.facet.clone()),
    );
    props.insert(
        "collection".to_string(),
        GraphValue::Text(edge.source.collection.clone()),
    );
    props.insert(
        "entity_id".to_string(),
        GraphValue::Text(edge.source.entity_id.clone()),
    );
    props.insert(
        "field".to_string(),
        GraphValue::Text(edge.source.field.clone()),
    );
    props
}

fn reference_target_node_props(edge: &ReferenceEdge) -> Props {
    let mut props = Props::new();
    props.insert(
        "kind".to_string(),
        GraphValue::Text(edge.target.kind.clone()),
    );
    props.insert(
        "entity_id".to_string(),
        GraphValue::Text(edge.target.id.clone()),
    );
    props
}

pub struct MarkdownReferenceUpdate<'a> {
    pub workspace: WorkspaceId,
    pub source: ReferenceSource,
    pub operation_id: &'a str,
    pub source_root: Digest,
    pub body: &'a [u8],
    pub now_ms: u64,
    pub relation: &'a str,
}

pub fn update_markdown_references<F>(
    loom: &mut Loom<FileStore>,
    mut index: ReferenceIndex,
    update: MarkdownReferenceUpdate<'_>,
    mut resolve: F,
) -> Result<ReferenceIndex>
where
    F: FnMut(&Loom<FileStore>, &MarkdownReferenceCandidate) -> Result<Option<EntityRef>>,
{
    remove_source_candidates(loom, update.workspace, &update.source)?;
    index.remove_source(&update.source);
    let Ok(text) = std::str::from_utf8(update.body) else {
        return Ok(index);
    };
    for candidate in extract_markdown_reference_candidates(text) {
        if let Some(target) = resolve(loom, &candidate)? {
            index.add(ReferenceEdge::new(
                update.source.clone(),
                target,
                update.relation.to_string(),
                candidate.span_start,
                candidate.span_end,
                candidate.text,
            )?);
            continue;
        }
        enqueue(
            loom,
            update.workspace,
            &UnresolvedReference::new(loom_substrate::refs::UnresolvedReferenceInput {
                candidate_id: format!("{}:body:{}", update.operation_id, candidate.span_start),
                source: update.source.clone(),
                source_operation_id: update.operation_id.to_string(),
                source_root: update.source_root,
                alias_text: candidate.text,
                relation: update.relation.to_string(),
                span_start: candidate.span_start as u64,
                span_end: candidate.span_end as u64,
                evidence: text.to_string(),
                next_attempt_ms: update.now_ms,
            })?,
        )?;
    }
    Ok(index)
}

pub fn enqueue(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    candidate: &UnresolvedReference,
) -> Result<()> {
    let mut target = ReferenceTarget::from_candidate(candidate);
    let candidates_table = target.candidate_table();
    authorize_table(loom, workspace, TARGETS_TABLE, AclRight::Write)?;
    authorize_table(loom, workspace, &candidates_table, AclRight::Write)?;
    ensure_shared_tables(loom, workspace)?;
    ensure_candidate_table(loom, workspace, &candidates_table)?;
    let existing = target_for(
        loom,
        workspace,
        &target.source_profile,
        &target.source_scope,
    )?;
    insert_candidate(loom, workspace, &candidates_table, candidate)?;
    target.pending = existing
        .as_ref()
        .map_or(1, |value| value.pending.saturating_add(1));
    target.next_attempt_ms = existing
        .map(|value| value.next_attempt_ms.min(candidate.next_attempt_ms))
        .unwrap_or(candidate.next_attempt_ms);
    save_target(loom, workspace, &target)
}

pub fn remove_source_candidates(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    source: &loom_substrate::refs::ReferenceSource,
) -> Result<usize> {
    let Some(target) = target_for(loom, workspace, &source.facet, &source.collection)? else {
        return Ok(0);
    };
    let table = target.candidate_table();
    authorize_table(loom, workspace, &table, AclRight::Read)?;
    authorize_table(loom, workspace, &table, AclRight::Write)?;
    authorize_table(loom, workspace, TARGETS_TABLE, AclRight::Write)?;
    let candidates = candidates_from_table(loom, workspace, &table)?;
    let mut removed = 0u64;
    for candidate in candidates
        .iter()
        .filter(|candidate| &candidate.source == source)
    {
        delete_candidate(loom, workspace, &table, candidate)?;
        removed = removed.saturating_add(1);
    }
    if removed > 0 {
        refresh_target(
            loom,
            workspace,
            &target,
            target.pending.saturating_sub(removed),
        )?;
    }
    Ok(removed as usize)
}

pub fn status(loom: &Loom<FileStore>, workspace: WorkspaceId) -> Result<ReconciliationSummary> {
    authorize_table(loom, workspace, TARGETS_TABLE, AclRight::Read)?;
    authorize_table(loom, workspace, RESOLUTIONS_TABLE, AclRight::Read)?;
    authorize_table(loom, workspace, FAILURES_TABLE, AclRight::Read)?;
    summary(loom, workspace)
}

pub fn targets(loom: &Loom<FileStore>, workspace: WorkspaceId) -> Result<Vec<ReferenceTarget>> {
    authorize_table(loom, workspace, TARGETS_TABLE, AclRight::Read)?;
    target_rows(loom, workspace)
}

pub fn due(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    target: &ReferenceTarget,
    now_ms: u64,
    max: usize,
) -> Result<Vec<UnresolvedReference>> {
    let table = target.candidate_table();
    authorize_table(loom, workspace, &table, AclRight::Read)?;
    due_from_table(loom, workspace, &table, now_ms, max)
}

pub fn reconcile<F>(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    target: &ReferenceTarget,
    now_ms: u64,
    max: usize,
    resolver_principal: &str,
    mut resolve: F,
) -> Result<Vec<ResolvedReference>>
where
    F: FnMut(&Loom<FileStore>, &UnresolvedReference) -> Result<Option<EntityRef>>,
{
    let candidates_table = target.candidate_table();
    authorize_table(loom, workspace, &candidates_table, AclRight::Read)?;
    authorize_table(loom, workspace, &candidates_table, AclRight::Write)?;
    authorize_table(loom, workspace, TARGETS_TABLE, AclRight::Write)?;
    authorize_table(loom, workspace, RESOLUTIONS_TABLE, AclRight::Write)?;
    authorize_table(loom, workspace, FAILURES_TABLE, AclRight::Write)?;
    ensure_shared_tables(loom, workspace)?;
    ensure_candidate_table(loom, workspace, &candidates_table)?;
    let mut records = Vec::new();
    let mut pending = target_for(
        loom,
        workspace,
        &target.source_profile,
        &target.source_scope,
    )?
    .map(|current| current.pending)
    .unwrap_or(0);
    for candidate in due_from_table(loom, workspace, &candidates_table, now_ms, max)? {
        let Some(entity) = resolve(loom, &candidate)? else {
            if reschedule_or_fail(loom, workspace, &candidates_table, &candidate, now_ms)? {
                pending = pending.saturating_sub(1);
            }
            continue;
        };
        let binding_root = Digest::hash(loom.store().digest_algo(), &loom.export_state());
        let record = ReferenceResolution::new(
            format!("reference.resolved:{}", candidate.candidate_id),
            &candidate.candidate_id,
            &candidate.source_operation_id,
            binding_root,
            entity,
            resolver_principal,
            now_ms,
        )?;
        insert_resolution(loom, workspace, &record)?;
        delete_candidate(loom, workspace, &candidates_table, &candidate)?;
        pending = pending.saturating_sub(1);
        records.push(ResolvedReference { candidate, record });
    }
    refresh_target(loom, workspace, target, pending)?;
    Ok(records)
}

pub fn summary(loom: &Loom<FileStore>, workspace: WorkspaceId) -> Result<ReconciliationSummary> {
    Ok(ReconciliationSummary {
        pending: target_rows(loom, workspace)?
            .into_iter()
            .map(|target| target.pending)
            .sum(),
        resolved: table_len(loom, workspace, RESOLUTIONS_TABLE)? as u64,
        failed: table_len(loom, workspace, FAILURES_TABLE)? as u64,
        processed: 0,
    })
}

fn ensure_shared_tables(loom: &mut Loom<FileStore>, workspace: WorkspaceId) -> Result<()> {
    if loom.staged_table_root(workspace, TARGETS_TABLE).is_none() {
        loom.stage_table_reserved(workspace, TARGETS_TABLE, &Table::new(target_schema()?))?;
    }
    if loom
        .staged_table_root(workspace, RESOLUTIONS_TABLE)
        .is_none()
    {
        loom.stage_table_reserved(workspace, RESOLUTIONS_TABLE, &Table::new(record_schema()?))?;
    }
    if loom.staged_table_root(workspace, FAILURES_TABLE).is_none() {
        loom.stage_table_reserved(workspace, FAILURES_TABLE, &Table::new(record_schema()?))?;
    }
    Ok(())
}

fn ensure_candidate_table(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    table: &str,
) -> Result<()> {
    if loom.staged_table_root(workspace, table).is_none() {
        loom.stage_table_reserved(workspace, table, &Table::new(candidate_schema()?))?;
    }
    Ok(())
}

fn target_schema() -> Result<Schema> {
    Schema::new(
        vec![
            ("source_profile".to_string(), ColumnType::Text),
            ("source_scope".to_string(), ColumnType::Text),
            ("next_attempt_ms".to_string(), ColumnType::U64),
            ("pending".to_string(), ColumnType::U64),
        ],
        vec![0, 1],
    )
}

fn candidate_schema() -> Result<Schema> {
    Schema::new(
        vec![
            ("next_attempt_ms".to_string(), ColumnType::U64),
            ("candidate_id".to_string(), ColumnType::Text),
            ("payload".to_string(), ColumnType::Bytes),
        ],
        vec![0, 1],
    )
}

fn record_schema() -> Result<Schema> {
    Schema::new(
        vec![
            ("candidate_id".to_string(), ColumnType::Text),
            ("payload".to_string(), ColumnType::Bytes),
        ],
        vec![0],
    )
}

fn insert_candidate(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    table: &str,
    candidate: &UnresolvedReference,
) -> Result<()> {
    loom.insert_row_reserved(
        workspace,
        table,
        vec![
            TableValue::U64(candidate.next_attempt_ms),
            TableValue::Text(candidate.candidate_id.clone()),
            TableValue::Bytes(candidate.encode()?),
        ],
    )
}

fn delete_candidate(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    table: &str,
    candidate: &UnresolvedReference,
) -> Result<()> {
    loom.delete_row_reserved(
        workspace,
        table,
        &[
            TableValue::U64(candidate.next_attempt_ms),
            TableValue::Text(candidate.candidate_id.clone()),
        ],
    )
}

fn insert_resolution(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    record: &ReferenceResolution,
) -> Result<()> {
    loom.insert_row_reserved(
        workspace,
        RESOLUTIONS_TABLE,
        vec![
            TableValue::Text(record.candidate_id.clone()),
            TableValue::Bytes(record.encode()?),
        ],
    )
}

fn reschedule_or_fail(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    candidates_table: &str,
    candidate: &UnresolvedReference,
    now_ms: u64,
) -> Result<bool> {
    delete_candidate(loom, workspace, candidates_table, candidate)?;
    let retried = candidate.retry_at(next_retry_ms(now_ms, candidate.attempts))?;
    if retried.attempts < MAX_ATTEMPTS {
        insert_candidate(loom, workspace, candidates_table, &retried)?;
        return Ok(false);
    }
    loom.insert_row_reserved(
        workspace,
        FAILURES_TABLE,
        vec![
            TableValue::Text(retried.candidate_id.clone()),
            TableValue::Bytes(retried.encode()?),
        ],
    )?;
    Ok(true)
}

fn refresh_target(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    target: &ReferenceTarget,
    pending: u64,
) -> Result<()> {
    if pending == 0 {
        return loom.delete_row_reserved(
            workspace,
            TARGETS_TABLE,
            &[
                TableValue::Text(target.source_profile.clone()),
                TableValue::Text(target.source_scope.clone()),
            ],
        );
    }
    let table = target.candidate_table();
    let next_attempt_ms = first_candidate_due(loom, workspace, &table)?.ok_or_else(|| {
        LoomError::corrupt("reference target has pending candidates without a due candidate")
    })?;
    save_target(
        loom,
        workspace,
        &ReferenceTarget {
            source_profile: target.source_profile.clone(),
            source_scope: target.source_scope.clone(),
            next_attempt_ms,
            pending,
        },
    )
}

fn save_target(
    loom: &mut Loom<FileStore>,
    workspace: WorkspaceId,
    target: &ReferenceTarget,
) -> Result<()> {
    loom.insert_row_reserved(
        workspace,
        TARGETS_TABLE,
        vec![
            TableValue::Text(target.source_profile.clone()),
            TableValue::Text(target.source_scope.clone()),
            TableValue::U64(target.next_attempt_ms),
            TableValue::U64(target.pending),
        ],
    )
}

fn target_for(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    source_profile: &str,
    source_scope: &str,
) -> Result<Option<ReferenceTarget>> {
    Ok(target_rows(loom, workspace)?.into_iter().find(|target| {
        target.source_profile == source_profile && target.source_scope == source_scope
    }))
}

fn target_rows(loom: &Loom<FileStore>, workspace: WorkspaceId) -> Result<Vec<ReferenceTarget>> {
    let table = match loom.read_table_reserved(workspace, TARGETS_TABLE) {
        Ok(table) => table,
        Err(error) if error.code == Code::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    table
        .scan(&Predicate::All)
        .into_iter()
        .map(|row| {
            let source_profile = row_text(row, 0, "reference target source profile")?;
            let source_scope = row_text(row, 1, "reference target source scope")?;
            let next_attempt_ms = row_u64(row, 2, "reference target next attempt")?;
            let pending = row_u64(row, 3, "reference target pending count")?;
            Ok(ReferenceTarget {
                source_profile,
                source_scope,
                next_attempt_ms,
                pending,
            })
        })
        .collect()
}

fn due_from_table(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    table: &str,
    now_ms: u64,
    max: usize,
) -> Result<Vec<UnresolvedReference>> {
    let Some((schema, Some(root))) = loom.table_reader_reserved(workspace, table)? else {
        return Ok(Vec::new());
    };
    let mut cursor = RowCursor::open(loom.store(), &schema, &root)?;
    let mut candidates = Vec::new();
    while candidates.len() < max {
        let Some(row) = cursor.next()? else { break };
        let due_at = row_u64(&row, 0, "reference candidate due time")?;
        if due_at > now_ms {
            break;
        }
        let payload = match row.get(2) {
            Some(TableValue::Bytes(value)) => value,
            _ => return Err(LoomError::corrupt("reference candidate payload is invalid")),
        };
        candidates.push(UnresolvedReference::decode(payload)?);
    }
    Ok(candidates)
}

fn candidates_from_table(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    table: &str,
) -> Result<Vec<UnresolvedReference>> {
    let Some((schema, Some(root))) = loom.table_reader_reserved(workspace, table)? else {
        return Ok(Vec::new());
    };
    let mut cursor = RowCursor::open(loom.store(), &schema, &root)?;
    let mut candidates = Vec::new();
    while let Some(row) = cursor.next()? {
        let payload = match row.get(2) {
            Some(TableValue::Bytes(value)) => value,
            _ => return Err(LoomError::corrupt("reference candidate payload is invalid")),
        };
        candidates.push(UnresolvedReference::decode(payload)?);
    }
    Ok(candidates)
}

fn first_candidate_due(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    table: &str,
) -> Result<Option<u64>> {
    let Some((schema, Some(root))) = loom.table_reader_reserved(workspace, table)? else {
        return Ok(None);
    };
    let mut cursor = RowCursor::open(loom.store(), &schema, &root)?;
    match cursor.next()? {
        Some(row) => row_u64(&row, 0, "reference candidate due time").map(Some),
        None => Ok(None),
    }
}

fn table_len(loom: &Loom<FileStore>, workspace: WorkspaceId, table: &str) -> Result<usize> {
    match loom.read_table_reserved(workspace, table) {
        Ok(table) => Ok(table.len()),
        Err(error) if error.code == Code::NotFound => Ok(0),
        Err(error) => Err(error),
    }
}

fn row_text(row: &[TableValue], index: usize, field: &str) -> Result<String> {
    match row.get(index) {
        Some(TableValue::Text(value)) => Ok(value.clone()),
        _ => Err(LoomError::corrupt(format!("{field} is invalid"))),
    }
}

fn row_u64(row: &[TableValue], index: usize, field: &str) -> Result<u64> {
    match row.get(index) {
        Some(TableValue::U64(value)) => Ok(*value),
        _ => Err(LoomError::corrupt(format!("{field} is invalid"))),
    }
}

fn candidate_table(source_profile: &str, source_scope: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update((source_profile.len() as u64).to_be_bytes());
    hasher.update(source_profile.as_bytes());
    hasher.update((source_scope.len() as u64).to_be_bytes());
    hasher.update(source_scope.as_bytes());
    format!(
        "{RECONCILIATION_DIR}/candidates/{}",
        hex::encode(hasher.finalize())
    )
}

fn reference_artifact_key(
    workspace_id: &str,
    kind: ReferenceArtifactKind,
    record_id: &str,
) -> Result<Vec<u8>> {
    if workspace_id.is_empty() {
        return Err(LoomError::invalid(
            "reference artifact workspace_id is empty",
        ));
    }
    if record_id.is_empty() {
        return Err(LoomError::invalid("reference artifact id is empty"));
    }
    EntityRef::parse(&format!("{}:{record_id}", kind.as_str()))?;
    Ok(format!(
        "{REFERENCE_ARTIFACT_PROFILE_PREFIX}/{workspace_id}/{}/{}",
        kind.as_str(),
        record_id
    )
    .into_bytes())
}

fn reference_artifact_summary(
    workspace_id: &str,
    record: ReferenceArtifactRecord,
    body: Vec<u8>,
) -> Result<ReferenceArtifactSummary> {
    let entity_ref = record.entity_ref().as_str();
    let source_ref = record.source_ref.as_str();
    let target_ref = record.target_ref.map(|target| target.as_str());
    Ok(ReferenceArtifactSummary {
        workspace_id: workspace_id.to_string(),
        record_id: record.record_id,
        kind: record.kind.as_str().to_string(),
        entity_ref,
        label: record.label,
        source_ref,
        source_operation_id: record.source_operation_id,
        target_ref,
        created_by: record.created_by,
        created_at_ms: record.created_at_ms,
        record_cbor_hex: hex::encode(body),
    })
}

fn next_retry_ms(now_ms: u64, attempts: u32) -> u64 {
    now_ms.saturating_add(1_000u64.saturating_mul(1u64 << attempts.min(16)))
}

fn authorize_table(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    table: &str,
    right: AclRight,
) -> Result<()> {
    loom.authorize_resource(
        AclResource::scoped(
            workspace,
            FacetKind::Vcs,
            None,
            AclResourceScope::Prefix {
                kind: AclScopeKind::Table,
                value: table.as_bytes(),
            },
        ),
        right,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::Algo;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_loom() -> (String, Loom<FileStore>, WorkspaceId) {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "loom-reference-{}-{}.loom",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&path);
        let store = FileStore::create_with_profile(&path, Algo::Blake3).unwrap();
        let mut loom = Loom::new(store);
        let workspace = WorkspaceId::v4_from_bytes([7; 16]);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some("studio"), workspace)
            .unwrap();
        (path.to_string_lossy().into_owned(), loom, workspace)
    }

    fn candidate(scope: &str, id: &str, alias: &str) -> UnresolvedReference {
        UnresolvedReference::new(loom_substrate::refs::UnresolvedReferenceInput {
            candidate_id: id.to_string(),
            source: loom_substrate::refs::ReferenceSource::new("tickets", scope, id, "body")
                .unwrap(),
            source_operation_id: format!("operation:{id}"),
            source_root: Digest::hash(Algo::Blake3, id.as_bytes()),
            alias_text: alias.to_string(),
            relation: "refers_to".to_string(),
            span_start: 0,
            span_end: alias.len() as u64,
            evidence: alias.to_string(),
            next_attempt_ms: 10,
        })
        .unwrap()
    }

    #[test]
    fn reference_artifact_records_persist_and_reject_duplicates() {
        let (path, mut loom, workspace) = test_loom();
        let summary = create_reference_artifact(
            &mut loom,
            workspace,
            ReferenceArtifactCreateRequest {
                workspace_id: "studio",
                record_id: "artifact-1",
                kind: ReferenceArtifactKind::Artifact,
                label: "Design recording",
                source_ref: "meeting-annotation:ann-1",
                source_operation_id: "promote-artifact-1",
                target_ref: Some("meeting:meet-1"),
                created_by: "principal-1",
                created_at_ms: 42,
            },
        )
        .unwrap();

        assert_eq!(summary.entity_ref, "artifact:artifact-1");
        assert_eq!(summary.target_ref.as_deref(), Some("meeting:meet-1"));
        let stored = get_reference_artifact(
            &loom,
            workspace,
            "studio",
            ReferenceArtifactKind::Artifact,
            "artifact-1",
        )
        .unwrap()
        .unwrap();
        assert_eq!(stored, summary);
        let duplicate = create_reference_artifact(
            &mut loom,
            workspace,
            ReferenceArtifactCreateRequest {
                workspace_id: "studio",
                record_id: "artifact-1",
                kind: ReferenceArtifactKind::Artifact,
                label: "Duplicate",
                source_ref: "meeting-annotation:ann-2",
                source_operation_id: "promote-artifact-2",
                target_ref: None,
                created_by: "principal-1",
                created_at_ms: 43,
            },
        )
        .unwrap_err();
        assert_eq!(duplicate.code, Code::AlreadyExists);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn targets_isolate_candidate_queues_by_profile_scope() {
        let (path, mut loom, workspace) = test_loom();
        let alpha = candidate("alpha", "candidate-alpha", "ALPHA-1");
        let beta = candidate("beta", "candidate-beta", "BETA-1");
        enqueue(&mut loom, workspace, &alpha).unwrap();
        enqueue(&mut loom, workspace, &beta).unwrap();

        let targets = targets(&loom, workspace).unwrap();
        assert_eq!(targets.len(), 2);
        let alpha_target = targets
            .iter()
            .find(|target| target.source_scope == "alpha")
            .unwrap()
            .clone();
        assert_eq!(
            due(&loom, workspace, &alpha_target, 10, 10).unwrap(),
            vec![alpha]
        );

        let records = reconcile(
            &mut loom,
            workspace,
            &alpha_target,
            10,
            10,
            "reference-resolver",
            |_, _| Ok(Some(EntityRef::parse("ticket:alpha-1").unwrap())),
        )
        .unwrap();
        assert_eq!(records.len(), 1);
        let summary = status(&loom, workspace).unwrap();
        assert_eq!(summary.pending, 1);
        assert_eq!(summary.resolved, 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn typed_document_references_project_to_graph_and_reverse_lookup() {
        let (path, mut loom, workspace) = test_loom();
        put_document_indexed(
            &mut loom,
            workspace,
            "notes",
            "decision",
            b"See !ticket:ticket-1 for ownership.".to_vec(),
        )
        .unwrap();

        let target = EntityRef::parse("ticket:ticket-1").unwrap();
        let inbound = references_to(&loom, workspace, &target).unwrap();
        assert_eq!(inbound.len(), 1);
        assert_eq!(inbound[0].source.facet, "document");
        assert_eq!(inbound[0].source.collection, "notes");
        assert_eq!(inbound[0].source.entity_id, "decision");
        assert_eq!(inbound[0].relation, "refers_to");

        let graph_edges = graph_edges(&loom, workspace, REFERENCE_GRAPH).unwrap();
        assert_eq!(graph_edges.len(), 1);
        let (_, edge) = graph_edges.into_iter().next().unwrap();
        assert_eq!(edge.src, "document:notes:decision");
        assert_eq!(edge.dst, "ticket:ticket-1");
        assert_eq!(edge.label, "refers_to");
        assert_eq!(
            edge.props.get("derived_from"),
            Some(&GraphValue::Text("references".to_string()))
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn explicit_page_reference_replaces_stale_graph_edges() {
        let (path, mut loom, workspace) = test_loom();
        let source = ReferenceSource::new("pages", "studio", "page-1", "declared_refs").unwrap();
        let first = ExplicitReferenceInput {
            source: source.clone(),
            relation: "references_ticket",
            target: EntityRef::parse("ticket:ticket-1").unwrap(),
            evidence: "page declared reference ticket-1",
        };
        replace_explicit_references(&mut loom, workspace, source.clone(), &[first]).unwrap();

        assert_eq!(references_from(&loom, workspace, &source).unwrap().len(), 1);
        assert_eq!(
            references_to(
                &loom,
                workspace,
                &EntityRef::parse("ticket:ticket-1").unwrap()
            )
            .unwrap()
            .len(),
            1
        );
        assert_eq!(
            graph_edges(&loom, workspace, REFERENCE_GRAPH)
                .unwrap()
                .len(),
            1
        );

        let second = ExplicitReferenceInput {
            source: source.clone(),
            relation: "references_ticket",
            target: EntityRef::parse("ticket:ticket-2").unwrap(),
            evidence: "page declared reference ticket-2",
        };
        replace_explicit_references(&mut loom, workspace, source.clone(), &[second]).unwrap();

        assert!(
            references_to(
                &loom,
                workspace,
                &EntityRef::parse("ticket:ticket-1").unwrap()
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(
            references_to(
                &loom,
                workspace,
                &EntityRef::parse("ticket:ticket-2").unwrap()
            )
            .unwrap()
            .len(),
            1
        );
        let graph_edges = graph_edges(&loom, workspace, REFERENCE_GRAPH).unwrap();
        assert_eq!(graph_edges.len(), 1);
        assert_eq!(graph_edges[0].1.dst, "ticket:ticket-2");
        let _ = std::fs::remove_file(path);
    }
}
