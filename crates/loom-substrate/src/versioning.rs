use loom_codec::Value;
use loom_core::{
    AtomicityBoundary, AuditIntent, CompareToken, FacetKind, FacetSideEffects, FacetWrite,
    FacetWriteOp, Loom, ObjectStore, OverlayDurabilityPolicy, OverlayEntryKind, OverlayKey,
    WorkflowControlWrite, WorkflowTransaction,
};
use loom_types::{Code, Digest, LoomError, Result, WorkspaceId};

use crate::{codec_error, validate_text, view::validate_view_id};

pub const BODY_REF_SCHEMA: &str = "loom.substrate.body-ref.v1";
pub const ENTITY_REVISION_SCHEMA: &str = "loom.substrate.entity-revision.v1";
pub const REVISION_LOG_SCHEMA: &str = "loom.substrate.revision-log.v1";
pub const CHECKPOINT_SCHEMA: &str = "loom.substrate.checkpoint.v1";
pub const REVISION_INDEX_SCHEMA: &str = "loom.substrate.revision-index.v1";
pub const REVISION_INDEX_DIR: &str = ".loom/substrate/revisions";
pub const REVISION_INDEX_CURRENT_SCHEMA: &str = "loom.substrate.revision-index.current.v1";
const REVISION_HISTORY_RECORD_SCHEMA: &str = "loom.substrate.revision-history-record.v1";

pub fn revision_index_path(scope_id: &str) -> Result<String> {
    validate_view_id(scope_id)?;
    Ok(format!("{REVISION_INDEX_DIR}/{scope_id}.lri"))
}

pub fn revision_index_current_key(workspace: WorkspaceId, scope_id: &str) -> Result<OverlayKey> {
    validate_view_id(scope_id)?;
    OverlayKey::from_segments([
        REVISION_INDEX_CURRENT_SCHEMA.as_bytes(),
        workspace.as_bytes(),
        scope_id.as_bytes(),
        b"revision-index",
        b"",
        b"v1",
    ])
}

fn revision_index_history_key(workspace: WorkspaceId, scope_id: &str) -> Result<Vec<u8>> {
    validate_view_id(scope_id)?;
    Ok(OverlayKey::from_segments([
        REVISION_INDEX_CURRENT_SCHEMA.as_bytes(),
        workspace.as_bytes(),
        scope_id.as_bytes(),
        b"revision-history",
        b"",
        b"v1",
    ])?
    .as_bytes()
    .to_vec())
}

fn revision_latest_key(
    workspace: WorkspaceId,
    scope_id: &str,
    entity_id: &str,
) -> Result<OverlayKey> {
    validate_view_id(scope_id)?;
    validate_text("entity_id", entity_id)?;
    OverlayKey::from_segments([
        REVISION_INDEX_CURRENT_SCHEMA.as_bytes(),
        workspace.as_bytes(),
        scope_id.as_bytes(),
        b"latest-revision",
        entity_id.as_bytes(),
        b"v1",
    ])
}

fn revision_checkpoint_key(
    workspace: WorkspaceId,
    scope_id: &str,
    checkpoint_scope: &str,
    checkpoint_id: &str,
) -> Result<OverlayKey> {
    validate_view_id(scope_id)?;
    validate_text("checkpoint_scope", checkpoint_scope)?;
    validate_text("checkpoint_id", checkpoint_id)?;
    OverlayKey::from_segments([
        REVISION_INDEX_CURRENT_SCHEMA.as_bytes(),
        workspace.as_bytes(),
        scope_id.as_bytes(),
        b"checkpoint",
        format!("{checkpoint_scope}:{checkpoint_id}").as_bytes(),
        b"v1",
    ])
}

fn current_overlay_payload<S: ObjectStore>(
    loom: &Loom<S>,
    key: &OverlayKey,
) -> Result<Option<Vec<u8>>> {
    if loom.store().uses_mutable_overlay_current_records() {
        Ok(loom
            .store()
            .mutable_overlay_current_entry(key)?
            .and_then(|entry| match entry.kind {
                OverlayEntryKind::Value => Some(entry.payload),
                OverlayEntryKind::Tombstone => None,
            }))
    } else {
        loom.mutable_overlay_snapshot()
            .read_composite(key, |_| Ok(None))
    }
}

fn encode_revision_index_manifest(head: u64, point_index_complete: bool) -> Result<Vec<u8>> {
    loom_codec::encode(&Value::Array(vec![
        Value::Text(REVISION_INDEX_CURRENT_SCHEMA.to_string()),
        Value::Array(vec![Value::Uint(head), Value::Bool(point_index_complete)]),
    ]))
    .map_err(codec_error)
}

fn decode_revision_index_manifest(bytes: &[u8]) -> Result<(u64, bool)> {
    let mut outer = ArrayFields::new(
        loom_codec::decode(bytes).map_err(codec_error)?,
        "revision-index manifest",
    )?;
    outer.expect_schema(REVISION_INDEX_CURRENT_SCHEMA)?;
    let mut fields = ArrayFields::new(
        outer.next("revision-index manifest fields")?,
        "revision-index manifest fields",
    )?;
    outer.end("revision-index manifest")?;
    let head = fields.uint("history head")?;
    let point_index_complete = fields
        .optional_bool("point index complete")?
        .unwrap_or(false);
    fields.end("revision-index manifest fields")?;
    Ok((head, point_index_complete))
}

enum RevisionHistoryRecord {
    Revision(EntityRevision),
    Checkpoint(Checkpoint),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevisionIndexAppend {
    pub revisions: Vec<EntityRevision>,
    pub checkpoints: Vec<Checkpoint>,
}

impl RevisionIndexAppend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_revision(&mut self, revision: EntityRevision) -> Result<()> {
        if let Some(previous) = self
            .revisions
            .iter()
            .rev()
            .find(|entry| entry.entity_id == revision.entity_id)
            && revision.revision != previous.revision.saturating_add(1)
        {
            return Err(LoomError::new(
                Code::Conflict,
                "revision append batch is not monotonic for entity",
            ));
        }
        self.revisions.push(revision);
        Ok(())
    }

    pub fn push_checkpoint(&mut self, checkpoint: Checkpoint) -> Result<()> {
        if self.checkpoints.iter().any(|existing| {
            existing.scope_id == checkpoint.scope_id
                && existing.checkpoint_id == checkpoint.checkpoint_id
        }) {
            return Err(LoomError::new(
                Code::Conflict,
                "checkpoint id already exists for scope in append batch",
            ));
        }
        self.checkpoints.push(checkpoint);
        Ok(())
    }

    pub fn apply_to(&self, mut index: RevisionIndex) -> Result<RevisionIndex> {
        for revision in &self.revisions {
            index.append_revision(revision.clone())?;
        }
        for checkpoint in &self.checkpoints {
            index.add_checkpoint(checkpoint.clone())?;
        }
        Ok(index)
    }
}

fn encode_revision_history_record(record: RevisionHistoryRecord) -> Result<Vec<u8>> {
    let fields = match record {
        RevisionHistoryRecord::Revision(revision) => {
            vec![Value::Uint(1), revision.to_value()]
        }
        RevisionHistoryRecord::Checkpoint(checkpoint) => {
            vec![Value::Uint(2), checkpoint.to_value()]
        }
    };
    loom_codec::encode(&Value::Array(vec![
        Value::Text(REVISION_HISTORY_RECORD_SCHEMA.to_string()),
        Value::Array(fields),
    ]))
    .map_err(codec_error)
}

fn decode_revision_history_record(bytes: &[u8]) -> Result<RevisionHistoryRecord> {
    let mut outer = ArrayFields::new(
        loom_codec::decode(bytes).map_err(codec_error)?,
        "revision history record",
    )?;
    outer.expect_schema(REVISION_HISTORY_RECORD_SCHEMA)?;
    let mut fields = ArrayFields::new(
        outer.next("revision history record fields")?,
        "revision history record fields",
    )?;
    outer.end("revision history record")?;
    let kind = fields.uint("revision history record kind")?;
    let value = fields.next("revision history record payload")?;
    fields.end("revision history record fields")?;
    match kind {
        1 => EntityRevision::from_value(value).map(RevisionHistoryRecord::Revision),
        2 => Checkpoint::from_value(value).map(RevisionHistoryRecord::Checkpoint),
        _ => Err(LoomError::corrupt(
            "revision history record kind is unknown",
        )),
    }
}

fn revision_index_delta_records(
    prior: &RevisionIndex,
    next: &RevisionIndex,
) -> Result<Vec<Vec<u8>>> {
    for prior_revision in prior.log.revisions() {
        if next.at_revision(&prior_revision.entity_id, prior_revision.revision)
            != Some(prior_revision)
        {
            return Err(LoomError::invalid(
                "revision index update must preserve retained revisions",
            ));
        }
    }
    for prior_checkpoint in &prior.checkpoints {
        if !next.checkpoints.contains(prior_checkpoint) {
            return Err(LoomError::invalid(
                "revision index update must preserve retained checkpoints",
            ));
        }
    }
    let mut records = Vec::new();
    for revision in next.log.revisions() {
        if prior
            .at_revision(&revision.entity_id, revision.revision)
            .is_none()
        {
            records.push(encode_revision_history_record(
                RevisionHistoryRecord::Revision(revision.clone()),
            )?);
        }
    }
    for checkpoint in &next.checkpoints {
        if !prior.checkpoints.contains(checkpoint) {
            records.push(encode_revision_history_record(
                RevisionHistoryRecord::Checkpoint(checkpoint.clone()),
            )?);
        }
    }
    Ok(records)
}

pub fn load_current_revision_index<S: ObjectStore>(
    loom: &Loom<S>,
    workspace: WorkspaceId,
    scope_id: &str,
) -> Result<RevisionIndex> {
    Ok(
        load_optional_current_revision_index(loom, workspace, scope_id)?
            .unwrap_or_else(RevisionIndex::new),
    )
}

pub fn load_latest_entity_revision<S: ObjectStore>(
    loom: &Loom<S>,
    workspace: WorkspaceId,
    scope_id: &str,
    entity_id: &str,
) -> Result<Option<EntityRevision>> {
    let key = revision_latest_key(workspace, scope_id, entity_id)?;
    if let Some(bytes) = current_overlay_payload(loom, &key)? {
        return EntityRevision::from_value(loom_codec::decode(&bytes).map_err(codec_error)?)
            .map(Some);
    }
    let manifest_key = revision_index_current_key(workspace, scope_id)?;
    if current_overlay_payload(loom, &manifest_key)?
        .as_deref()
        .and_then(|bytes| decode_revision_index_manifest(bytes).ok())
        .is_some_and(|(_, complete)| complete)
    {
        return Ok(None);
    }
    Ok(
        load_optional_current_revision_index(loom, workspace, scope_id)?
            .and_then(|index| index.latest(entity_id).cloned()),
    )
}

pub fn load_optional_current_revision_index<S: ObjectStore>(
    loom: &Loom<S>,
    workspace: WorkspaceId,
    scope_id: &str,
) -> Result<Option<RevisionIndex>> {
    let key = revision_index_current_key(workspace, scope_id)?;
    let bytes = if loom.store().uses_mutable_overlay_current_records() {
        loom.store()
            .mutable_overlay_current_entry(&key)?
            .and_then(|entry| match entry.kind {
                OverlayEntryKind::Value => Some(entry.payload),
                OverlayEntryKind::Tombstone => None,
            })
    } else {
        loom.mutable_overlay_snapshot()
            .read_composite(&key, |_| Ok(None))?
    };
    match bytes {
        Some(bytes) => match decode_revision_index_manifest(&bytes) {
            Ok((head, _)) => {
                let history_key = revision_index_history_key(workspace, scope_id)?;
                let records = loom
                    .store()
                    .retained_history_records(&history_key, 1, usize::MAX)?;
                if records.len() as u64 != head {
                    return Err(LoomError::corrupt(
                        "revision-index manifest does not match retained history",
                    ));
                }
                let mut index = RevisionIndex::new();
                for bytes in records {
                    match decode_revision_history_record(&bytes)? {
                        RevisionHistoryRecord::Revision(revision) => {
                            index.append_revision(revision)?;
                        }
                        RevisionHistoryRecord::Checkpoint(checkpoint) => {
                            index.add_checkpoint(checkpoint)?;
                        }
                    }
                }
                Ok(Some(index))
            }
            Err(_) => RevisionIndex::decode(&bytes).map(Some),
        },
        None => Ok(None),
    }
}

pub fn current_revision_index_write<S: ObjectStore>(
    loom: &Loom<S>,
    workspace: WorkspaceId,
    scope_id: &str,
    facet: FacetKind,
    index: &RevisionIndex,
) -> Result<(FacetWrite, Vec<WorkflowControlWrite>)> {
    let key = revision_index_current_key(workspace, scope_id)?;
    let expected = if loom.store().uses_mutable_overlay_current_records() {
        loom.store().mutable_overlay_owner_token(&key)?
    } else {
        loom.mutable_overlay_snapshot().owner_token(&key)?
    };
    let history_key = revision_index_history_key(workspace, scope_id)?;
    let (payload, controls) = match loom.store().retained_history_head(&history_key) {
        Ok(current_head) => {
            let prior = if current_head == 0 {
                RevisionIndex::new()
            } else {
                load_optional_current_revision_index(loom, workspace, scope_id)?
                    .unwrap_or_else(RevisionIndex::new)
            };
            let records = revision_index_delta_records(&prior, index)?;
            let next_head = current_head
                .checked_add(records.len() as u64)
                .ok_or_else(|| LoomError::invalid("revision-index history sequence overflow"))?;
            let controls = if records.is_empty() {
                Vec::new()
            } else {
                vec![WorkflowControlWrite::AppendRetained {
                    key: history_key,
                    expected_next_sequence: current_head + 1,
                    records,
                }]
            };
            (encode_revision_index_manifest(next_head, false)?, controls)
        }
        Err(error) if error.code == Code::Unsupported => (index.encode()?, Vec::new()),
        Err(error) => return Err(error),
    };
    Ok((
        FacetWrite {
            facet,
            target: key.clone(),
            op: FacetWriteOp::Put { payload },
            secondary_indexes: Vec::new(),
            expected: expected.map(CompareToken),
            durability: None,
            audit: Some(AuditIntent {
                operation: "revision-index.current.put".to_string(),
            }),
            side_effects: FacetSideEffects::default(),
        },
        controls,
    ))
}

pub fn current_revision_index_append_writes<S: ObjectStore>(
    loom: &Loom<S>,
    workspace: WorkspaceId,
    scope_id: &str,
    facet: FacetKind,
    additions: &RevisionIndexAppend,
) -> Result<(Vec<FacetWrite>, Vec<WorkflowControlWrite>)> {
    let current_key = revision_index_current_key(workspace, scope_id)?;
    let current_expected = if loom.store().uses_mutable_overlay_current_records() {
        loom.store().mutable_overlay_owner_token(&current_key)?
    } else {
        loom.mutable_overlay_snapshot().owner_token(&current_key)?
    };
    let history_key = revision_index_history_key(workspace, scope_id)?;
    let current_head = loom.store().retained_history_head(&history_key)?;
    let current_payload = current_overlay_payload(loom, &current_key)?;
    let point_index_complete = current_payload
        .as_deref()
        .and_then(|bytes| decode_revision_index_manifest(bytes).ok())
        .map(|(_, complete)| complete)
        .unwrap_or(current_payload.is_none() && current_head == 0);
    let prior = if point_index_complete {
        None
    } else {
        load_optional_current_revision_index(loom, workspace, scope_id)?
    };
    let mut records = Vec::new();
    if current_head == 0
        && let Some(legacy) = &prior
    {
        records.extend(revision_index_delta_records(&RevisionIndex::new(), legacy)?);
    }

    let mut writes = Vec::new();
    let mut latest_in_batch = prior
        .as_ref()
        .map(|index| {
            index
                .log
                .revisions()
                .iter()
                .map(|revision| (revision.entity_id.clone(), revision.revision))
                .collect::<std::collections::BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let touched_entities = additions
        .revisions
        .iter()
        .map(|revision| revision.entity_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(prior) = &prior {
        for revision in prior.log.revisions() {
            if prior.latest(&revision.entity_id) != Some(revision)
                || touched_entities.contains(revision.entity_id.as_str())
            {
                continue;
            }
            let key = revision_latest_key(workspace, scope_id, &revision.entity_id)?;
            let expected = if loom.store().uses_mutable_overlay_current_records() {
                loom.store().mutable_overlay_owner_token(&key)?
            } else {
                loom.mutable_overlay_snapshot().owner_token(&key)?
            };
            writes.push(FacetWrite {
                facet,
                target: key,
                op: FacetWriteOp::Put {
                    payload: loom_codec::encode(&revision.to_value()).map_err(codec_error)?,
                },
                secondary_indexes: Vec::new(),
                expected: expected.map(CompareToken),
                durability: None,
                audit: None,
                side_effects: FacetSideEffects::default(),
            });
        }
        let new_checkpoints = additions
            .checkpoints
            .iter()
            .map(|checkpoint| {
                (
                    checkpoint.scope_id.as_str(),
                    checkpoint.checkpoint_id.as_str(),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        for checkpoint in &prior.checkpoints {
            if new_checkpoints.contains(&(
                checkpoint.scope_id.as_str(),
                checkpoint.checkpoint_id.as_str(),
            )) {
                continue;
            }
            let key = revision_checkpoint_key(
                workspace,
                scope_id,
                &checkpoint.scope_id,
                &checkpoint.checkpoint_id,
            )?;
            let expected = if loom.store().uses_mutable_overlay_current_records() {
                loom.store().mutable_overlay_owner_token(&key)?
            } else {
                loom.mutable_overlay_snapshot().owner_token(&key)?
            };
            writes.push(FacetWrite {
                facet,
                target: key,
                op: FacetWriteOp::Put {
                    payload: checkpoint.encode()?,
                },
                secondary_indexes: Vec::new(),
                expected: expected.map(CompareToken),
                durability: None,
                audit: None,
                side_effects: FacetSideEffects::default(),
            });
        }
    }
    for revision in &additions.revisions {
        let current_revision = match latest_in_batch.get(&revision.entity_id) {
            Some(revision) => *revision,
            None => current_overlay_payload(
                loom,
                &revision_latest_key(workspace, scope_id, &revision.entity_id)?,
            )?
            .map(|bytes| {
                EntityRevision::from_value(loom_codec::decode(&bytes).map_err(codec_error)?)
                    .map(|revision| revision.revision)
            })
            .transpose()?
            .unwrap_or(0),
        };
        let expected_revision = current_revision
            .checked_add(1)
            .ok_or_else(|| LoomError::invalid("entity revision overflow"))?;
        if revision.revision != expected_revision {
            return Err(LoomError::new(
                Code::Conflict,
                format!(
                    "entity revision must be {expected_revision}, got {}",
                    revision.revision
                ),
            ));
        }
        let key = revision_latest_key(workspace, scope_id, &revision.entity_id)?;
        let expected = if loom.store().uses_mutable_overlay_current_records() {
            loom.store().mutable_overlay_owner_token(&key)?
        } else {
            loom.mutable_overlay_snapshot().owner_token(&key)?
        };
        writes.push(FacetWrite {
            facet,
            target: key,
            op: FacetWriteOp::Put {
                payload: loom_codec::encode(&revision.to_value()).map_err(codec_error)?,
            },
            secondary_indexes: Vec::new(),
            expected: expected.map(CompareToken),
            durability: None,
            audit: None,
            side_effects: FacetSideEffects::default(),
        });
        latest_in_batch.insert(revision.entity_id.clone(), revision.revision);
        records.push(encode_revision_history_record(
            RevisionHistoryRecord::Revision(revision.clone()),
        )?);
    }
    for checkpoint in &additions.checkpoints {
        let key = revision_checkpoint_key(
            workspace,
            scope_id,
            &checkpoint.scope_id,
            &checkpoint.checkpoint_id,
        )?;
        let existed_before = prior.as_ref().is_some_and(|index| {
            index.checkpoints.iter().any(|existing| {
                existing.scope_id == checkpoint.scope_id
                    && existing.checkpoint_id == checkpoint.checkpoint_id
            })
        });
        if existed_before || current_overlay_payload(loom, &key)?.is_some() {
            return Err(LoomError::new(
                Code::Conflict,
                "checkpoint id already exists for scope",
            ));
        }
        let expected = if loom.store().uses_mutable_overlay_current_records() {
            loom.store().mutable_overlay_owner_token(&key)?
        } else {
            loom.mutable_overlay_snapshot().owner_token(&key)?
        };
        writes.push(FacetWrite {
            facet,
            target: key,
            op: FacetWriteOp::Put {
                payload: checkpoint.encode()?,
            },
            secondary_indexes: Vec::new(),
            expected: expected.map(CompareToken),
            durability: None,
            audit: None,
            side_effects: FacetSideEffects::default(),
        });
        records.push(encode_revision_history_record(
            RevisionHistoryRecord::Checkpoint(checkpoint.clone()),
        )?);
    }
    let next_head = current_head
        .checked_add(records.len() as u64)
        .ok_or_else(|| LoomError::invalid("revision-index history sequence overflow"))?;
    writes.push(FacetWrite {
        facet,
        target: current_key,
        op: FacetWriteOp::Put {
            payload: encode_revision_index_manifest(next_head, true)?,
        },
        secondary_indexes: Vec::new(),
        expected: current_expected.map(CompareToken),
        durability: None,
        audit: Some(AuditIntent {
            operation: "revision-index.current.put".to_string(),
        }),
        side_effects: FacetSideEffects::default(),
    });
    let controls = if records.is_empty() {
        Vec::new()
    } else {
        vec![WorkflowControlWrite::AppendRetained {
            key: history_key,
            expected_next_sequence: current_head + 1,
            records,
        }]
    };
    Ok((writes, controls))
}

pub fn persist_current_revision_index<S: ObjectStore>(
    loom: &mut Loom<S>,
    workspace: WorkspaceId,
    scope_id: &str,
    facet: FacetKind,
    index: &RevisionIndex,
) -> Result<()> {
    persist_current_revision_index_with_owner_state(
        loom,
        workspace,
        scope_id,
        facet,
        index,
        loom_core::WorkflowOwnerState::default(),
    )
}

pub fn persist_current_revision_index_with_owner_state<S: ObjectStore>(
    loom: &mut Loom<S>,
    workspace: WorkspaceId,
    scope_id: &str,
    facet: FacetKind,
    index: &RevisionIndex,
    owner_state: loom_core::WorkflowOwnerState,
) -> Result<()> {
    persist_current_revision_index_with_owner_state_and_writes(
        loom,
        workspace,
        scope_id,
        facet,
        index,
        Vec::new(),
        None,
        owner_state,
    )
}

pub fn persist_current_revision_index_with_owner_state_and_writes<S: ObjectStore>(
    loom: &mut Loom<S>,
    workspace: WorkspaceId,
    scope_id: &str,
    facet: FacetKind,
    index: &RevisionIndex,
    mut writes: Vec<FacetWrite>,
    idempotency: Option<loom_core::IdempotencyKey>,
    mut owner_state: loom_core::WorkflowOwnerState,
) -> Result<()> {
    let expected_generation = if loom.store().uses_mutable_overlay_current_records() {
        loom.store().mutable_overlay_generation()?
    } else {
        loom.mutable_overlay_snapshot().generation()
    };
    let (write, controls) = current_revision_index_write(loom, workspace, scope_id, facet, index)?;
    writes.push(write);
    owner_state.controls.extend(controls);
    let receipt = loom
        .store()
        .commit_workflow_transaction(WorkflowTransaction {
            workspace,
            actor: loom.effective_principal()?.unwrap_or(workspace),
            expected_generation: Some(expected_generation),
            writes,
            prepared_operations: Vec::new(),
            revision_metadata: Vec::new(),
            delivery_intents: Vec::new(),
            durability: OverlayDurabilityPolicy::Normal,
            boundary: AtomicityBoundary::Single,
            idempotency,
            owner_state,
            post_commit_delta: None,
        })?;
    for outcome in receipt.writes {
        let current = loom
            .store()
            .mutable_overlay_current_entry(&outcome.target)?
            .ok_or_else(|| {
                LoomError::corrupt("workflow transaction omitted committed current record")
            })?;
        loom.mutable_overlay_mut()
            .synchronize_current_entry(current)?;
    }
    Ok(())
}

pub fn persist_revision_index_append_with_owner_state_and_writes<S: ObjectStore>(
    loom: &mut Loom<S>,
    workspace: WorkspaceId,
    scope_id: &str,
    facet: FacetKind,
    additions: &RevisionIndexAppend,
    mut writes: Vec<FacetWrite>,
    idempotency: Option<loom_core::IdempotencyKey>,
    mut owner_state: loom_core::WorkflowOwnerState,
) -> Result<()> {
    let expected_generation = if loom.store().uses_mutable_overlay_current_records() {
        loom.store().mutable_overlay_generation()?
    } else {
        loom.mutable_overlay_snapshot().generation()
    };
    let (mut revision_writes, controls) =
        current_revision_index_append_writes(loom, workspace, scope_id, facet, additions)?;
    writes.append(&mut revision_writes);
    owner_state.controls.extend(controls);
    let receipt = loom
        .store()
        .commit_workflow_transaction(WorkflowTransaction {
            workspace,
            actor: loom.effective_principal()?.unwrap_or(workspace),
            expected_generation: Some(expected_generation),
            writes,
            prepared_operations: Vec::new(),
            revision_metadata: Vec::new(),
            delivery_intents: Vec::new(),
            durability: OverlayDurabilityPolicy::Normal,
            boundary: AtomicityBoundary::Single,
            idempotency,
            owner_state,
            post_commit_delta: None,
        })?;
    for outcome in receipt.writes {
        let current = loom
            .store()
            .mutable_overlay_current_entry(&outcome.target)?
            .ok_or_else(|| {
                LoomError::corrupt("workflow transaction omitted committed current record")
            })?;
        loom.mutable_overlay_mut()
            .synchronize_current_entry(current)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRevisionUpdate {
    pub entity_id: String,
    pub operation_id: String,
    pub body: BodyRef,
    pub timestamp_ms: u64,
    pub checkpoint_id: String,
    pub expected_latest_revision: Option<u64>,
}

impl ProfileRevisionUpdate {
    pub fn new(
        entity_id: impl Into<String>,
        operation_id: impl Into<String>,
        body: BodyRef,
        timestamp_ms: u64,
        checkpoint_id: impl Into<String>,
        expected_latest_revision: Option<u64>,
    ) -> Result<Self> {
        let update = Self {
            entity_id: entity_id.into(),
            operation_id: operation_id.into(),
            body,
            timestamp_ms,
            checkpoint_id: checkpoint_id.into(),
            expected_latest_revision,
        };
        validate_text("entity_id", &update.entity_id)?;
        validate_text("operation_id", &update.operation_id)?;
        validate_text("checkpoint_id", &update.checkpoint_id)?;
        Ok(update)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileTransaction {
    pub scope_id: String,
    pub expected_root: Option<Digest>,
    pub root_after: Digest,
    pub revisions: Vec<ProfileRevisionUpdate>,
}

impl ProfileTransaction {
    pub fn new(
        scope_id: impl Into<String>,
        expected_root: Option<Digest>,
        root_after: Digest,
        revisions: Vec<ProfileRevisionUpdate>,
    ) -> Result<Self> {
        let transaction = Self {
            scope_id: scope_id.into(),
            expected_root,
            root_after,
            revisions,
        };
        validate_text("scope_id", &transaction.scope_id)?;
        if transaction.revisions.is_empty() {
            return Err(LoomError::invalid(
                "profile transaction must include at least one revision",
            ));
        }
        Ok(transaction)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRevisionReceipt {
    pub entity_id: String,
    pub revision: u64,
    pub operation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileTransactionReceipt {
    pub root_before: Digest,
    pub root_after: Digest,
    pub revisions: Vec<ProfileRevisionReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionBackfillUpdate {
    pub entity_id: String,
    pub operation_id: String,
    pub body: BodyRef,
    pub root: Digest,
    pub timestamp_ms: u64,
    pub checkpoint_id: String,
}

impl RevisionBackfillUpdate {
    pub fn new(
        entity_id: impl Into<String>,
        operation_id: impl Into<String>,
        body: BodyRef,
        root: Digest,
        timestamp_ms: u64,
        checkpoint_id: impl Into<String>,
    ) -> Result<Self> {
        let update = Self {
            entity_id: entity_id.into(),
            operation_id: operation_id.into(),
            body,
            root,
            timestamp_ms,
            checkpoint_id: checkpoint_id.into(),
        };
        validate_text("entity_id", &update.entity_id)?;
        validate_text("operation_id", &update.operation_id)?;
        validate_text("checkpoint_id", &update.checkpoint_id)?;
        Ok(update)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionBackfillReport {
    pub inserted: u64,
    pub skipped_existing: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileTransactionState {
    root: Digest,
    revision_index: RevisionIndex,
}

impl ProfileTransactionState {
    pub fn new(root: Digest, revision_index: RevisionIndex) -> Self {
        Self {
            root,
            revision_index,
        }
    }

    pub fn root(&self) -> Digest {
        self.root
    }

    pub fn revision_index(&self) -> &RevisionIndex {
        &self.revision_index
    }

    pub fn into_revision_index(self) -> RevisionIndex {
        self.revision_index
    }

    pub fn apply(&mut self, transaction: ProfileTransaction) -> Result<ProfileTransactionReceipt> {
        if let Some(expected_root) = transaction.expected_root
            && expected_root != self.root
        {
            return Err(LoomError::new(
                Code::Conflict,
                "profile transaction root does not match current root",
            ));
        }
        let mut next_index = self.revision_index.clone();
        let mut receipts = Vec::with_capacity(transaction.revisions.len());
        for update in transaction.revisions {
            let current_revision = next_index
                .latest(&update.entity_id)
                .map(|entry| entry.revision)
                .unwrap_or(0);
            if let Some(expected_latest_revision) = update.expected_latest_revision
                && expected_latest_revision != current_revision
            {
                return Err(LoomError::new(
                    Code::Conflict,
                    "profile entity revision does not match expected revision",
                ));
            }
            let revision = current_revision
                .checked_add(1)
                .ok_or_else(|| LoomError::invalid("profile entity revision overflow"))?;
            next_index.append_revision(EntityRevision::new(
                update.entity_id.clone(),
                revision,
                update.operation_id.clone(),
                update.body,
                transaction.root_after,
                update.timestamp_ms,
            )?)?;
            next_index.add_checkpoint(Checkpoint::new(
                transaction.scope_id.clone(),
                update.checkpoint_id,
                transaction.root_after,
                revision,
                update.operation_id.clone(),
                update.timestamp_ms,
            )?)?;
            receipts.push(ProfileRevisionReceipt {
                entity_id: update.entity_id,
                revision,
                operation_id: update.operation_id,
            });
        }
        let root_before = self.root;
        self.root = transaction.root_after;
        self.revision_index = next_index;
        Ok(ProfileTransactionReceipt {
            root_before,
            root_after: self.root,
            revisions: receipts,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyRef {
    pub digest: Digest,
    pub len: u64,
    pub media_type: String,
}

impl BodyRef {
    pub fn new(digest: Digest, len: u64, media_type: impl Into<String>) -> Result<Self> {
        let media_type = media_type.into();
        validate_text("media_type", &media_type)?;
        Ok(Self {
            digest,
            len,
            media_type,
        })
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(BODY_REF_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.digest.to_string()),
                Value::Uint(self.len),
                Value::Text(self.media_type.clone()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ArrayFields::new(value, "body ref")?;
        outer.expect_schema(BODY_REF_SCHEMA)?;
        let mut fields = ArrayFields::new(outer.next("body ref fields")?, "body ref fields")?;
        outer.end("body ref")?;
        let digest = Digest::parse(&fields.text("digest")?)?;
        let len = fields.uint("len")?;
        let media_type = fields.text("media_type")?;
        fields.end("body ref fields")?;
        BodyRef::new(digest, len, media_type)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRevision {
    pub entity_id: String,
    pub revision: u64,
    pub operation_id: String,
    pub body: BodyRef,
    pub root: Digest,
    pub timestamp_ms: u64,
}

impl EntityRevision {
    pub fn new(
        entity_id: impl Into<String>,
        revision: u64,
        operation_id: impl Into<String>,
        body: BodyRef,
        root: Digest,
        timestamp_ms: u64,
    ) -> Result<Self> {
        let entity_id = entity_id.into();
        let operation_id = operation_id.into();
        validate_text("entity_id", &entity_id)?;
        validate_text("operation_id", &operation_id)?;
        Ok(Self {
            entity_id,
            revision,
            operation_id,
            body,
            root,
            timestamp_ms,
        })
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(ENTITY_REVISION_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.entity_id.clone()),
                Value::Uint(self.revision),
                Value::Text(self.operation_id.clone()),
                self.body.to_value(),
                Value::Text(self.root.to_string()),
                Value::Uint(self.timestamp_ms),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ArrayFields::new(value, "entity revision")?;
        outer.expect_schema(ENTITY_REVISION_SCHEMA)?;
        let mut fields = ArrayFields::new(
            outer.next("entity revision fields")?,
            "entity revision fields",
        )?;
        outer.end("entity revision")?;
        let entity_id = fields.text("entity_id")?;
        let revision = fields.uint("revision")?;
        let operation_id = fields.text("operation_id")?;
        let body = BodyRef::from_value(fields.next("body")?)?;
        let root = Digest::parse(&fields.text("root")?)?;
        let timestamp_ms = fields.uint("timestamp_ms")?;
        fields.end("entity revision fields")?;
        EntityRevision::new(entity_id, revision, operation_id, body, root, timestamp_ms)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevisionLog {
    revisions: Vec<EntityRevision>,
}

impl RevisionLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, revision: EntityRevision) -> Result<()> {
        let expected = self
            .revisions
            .iter()
            .filter(|entry| entry.entity_id == revision.entity_id)
            .map(|entry| entry.revision)
            .max()
            .unwrap_or(0)
            + 1;
        if revision.revision != expected {
            return Err(LoomError::new(
                Code::Conflict,
                format!(
                    "entity revision must be {expected}, got {}",
                    revision.revision
                ),
            ));
        }
        self.revisions.push(revision);
        self.revisions.sort_by(|left, right| {
            left.entity_id
                .cmp(&right.entity_id)
                .then_with(|| left.revision.cmp(&right.revision))
        });
        Ok(())
    }

    pub fn latest(&self, entity_id: &str) -> Option<&EntityRevision> {
        self.revisions
            .iter()
            .filter(|entry| entry.entity_id == entity_id)
            .max_by_key(|entry| entry.revision)
    }

    pub fn at_revision(&self, entity_id: &str, revision: u64) -> Option<&EntityRevision> {
        self.revisions
            .iter()
            .find(|entry| entry.entity_id == entity_id && entry.revision == revision)
    }

    pub fn as_of_root(&self, entity_id: &str, root: &Digest) -> Option<&EntityRevision> {
        self.revisions
            .iter()
            .filter(|entry| entry.entity_id == entity_id && &entry.root == root)
            .max_by_key(|entry| entry.revision)
    }

    pub fn revisions(&self) -> &[EntityRevision] {
        &self.revisions
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(REVISION_LOG_SCHEMA.to_string()),
            Value::Array(
                self.revisions
                    .iter()
                    .map(EntityRevision::to_value)
                    .collect(),
            ),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ArrayFields::new(value, "revision log")?;
        outer.expect_schema(REVISION_LOG_SCHEMA)?;
        let revisions = array_items(outer.next("revisions")?, "revisions")?
            .into_iter()
            .map(EntityRevision::from_value)
            .collect::<Result<Vec<_>>>()?;
        outer.end("revision log")?;
        let mut log = RevisionLog::new();
        for revision in revisions {
            log.append(revision)?;
        }
        Ok(log)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub scope_id: String,
    pub checkpoint_id: String,
    pub root: Digest,
    pub max_revision: u64,
    pub operation_id: String,
    pub created_at_ms: u64,
}

impl Checkpoint {
    pub fn new(
        scope_id: impl Into<String>,
        checkpoint_id: impl Into<String>,
        root: Digest,
        max_revision: u64,
        operation_id: impl Into<String>,
        created_at_ms: u64,
    ) -> Result<Self> {
        let scope_id = scope_id.into();
        let checkpoint_id = checkpoint_id.into();
        let operation_id = operation_id.into();
        validate_text("scope_id", &scope_id)?;
        validate_text("checkpoint_id", &checkpoint_id)?;
        validate_text("operation_id", &operation_id)?;
        Ok(Self {
            scope_id,
            checkpoint_id,
            root,
            max_revision,
            operation_id,
            created_at_ms,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(CHECKPOINT_SCHEMA.to_string()),
            Value::Array(vec![
                Value::Text(self.scope_id.clone()),
                Value::Text(self.checkpoint_id.clone()),
                Value::Text(self.root.to_string()),
                Value::Uint(self.max_revision),
                Value::Text(self.operation_id.clone()),
                Value::Uint(self.created_at_ms),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ArrayFields::new(value, "checkpoint")?;
        outer.expect_schema(CHECKPOINT_SCHEMA)?;
        let mut fields = ArrayFields::new(outer.next("checkpoint fields")?, "checkpoint fields")?;
        outer.end("checkpoint")?;
        let scope_id = fields.text("scope_id")?;
        let checkpoint_id = fields.text("checkpoint_id")?;
        let root = Digest::parse(&fields.text("root")?)?;
        let max_revision = fields.uint("max_revision")?;
        let operation_id = fields.text("operation_id")?;
        let created_at_ms = fields.uint("created_at_ms")?;
        fields.end("checkpoint fields")?;
        Checkpoint::new(
            scope_id,
            checkpoint_id,
            root,
            max_revision,
            operation_id,
            created_at_ms,
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevisionIndex {
    log: RevisionLog,
    checkpoints: Vec<Checkpoint>,
}

impl RevisionIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append_revision(&mut self, revision: EntityRevision) -> Result<()> {
        self.log.append(revision)
    }

    pub fn add_checkpoint(&mut self, checkpoint: Checkpoint) -> Result<()> {
        if self.checkpoints.iter().any(|existing| {
            existing.scope_id == checkpoint.scope_id
                && existing.checkpoint_id == checkpoint.checkpoint_id
        }) {
            return Err(LoomError::new(
                Code::AlreadyExists,
                "checkpoint already exists in scope",
            ));
        }
        self.checkpoints.push(checkpoint);
        self.checkpoints.sort_by(|left, right| {
            left.scope_id
                .cmp(&right.scope_id)
                .then_with(|| left.max_revision.cmp(&right.max_revision))
                .then_with(|| left.checkpoint_id.cmp(&right.checkpoint_id))
        });
        Ok(())
    }

    pub fn history(&self, entity_id: &str) -> Vec<&EntityRevision> {
        self.log
            .revisions()
            .iter()
            .filter(|entry| entry.entity_id == entity_id)
            .collect()
    }

    pub fn latest(&self, entity_id: &str) -> Option<&EntityRevision> {
        self.log.latest(entity_id)
    }

    pub fn at_revision(&self, entity_id: &str, revision: u64) -> Option<&EntityRevision> {
        self.log.at_revision(entity_id, revision)
    }

    pub fn as_of_root(&self, entity_id: &str, root: &Digest) -> Option<&EntityRevision> {
        self.log.as_of_root(entity_id, root)
    }

    pub fn checkpoint_before_or_at(&self, scope_id: &str, revision: u64) -> Option<&Checkpoint> {
        self.checkpoints
            .iter()
            .filter(|entry| entry.scope_id == scope_id && entry.max_revision <= revision)
            .max_by_key(|entry| entry.max_revision)
    }

    pub fn checkpoints(&self) -> &[Checkpoint] {
        &self.checkpoints
    }

    pub fn backfill_missing_current(
        &mut self,
        scope_id: &str,
        updates: impl IntoIterator<Item = RevisionBackfillUpdate>,
    ) -> Result<RevisionBackfillReport> {
        validate_text("scope_id", scope_id)?;
        let mut inserted = 0u64;
        let mut skipped_existing = 0u64;
        for update in updates {
            if self.latest(&update.entity_id).is_some() {
                skipped_existing = skipped_existing.saturating_add(1);
                continue;
            }
            self.append_revision(EntityRevision::new(
                update.entity_id,
                1,
                update.operation_id.clone(),
                update.body,
                update.root,
                update.timestamp_ms,
            )?)?;
            self.add_checkpoint(Checkpoint::new(
                scope_id,
                update.checkpoint_id,
                update.root,
                1,
                update.operation_id,
                update.timestamp_ms,
            )?)?;
            inserted = inserted.saturating_add(1);
        }
        Ok(RevisionBackfillReport {
            inserted,
            skipped_existing,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        loom_codec::encode(&self.to_value()).map_err(codec_error)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        Self::from_value(loom_codec::decode(bytes).map_err(codec_error)?)
    }

    pub fn to_value(&self) -> Value {
        Value::Array(vec![
            Value::Text(REVISION_INDEX_SCHEMA.to_string()),
            Value::Array(vec![
                self.log.to_value(),
                Value::Array(self.checkpoints.iter().map(Checkpoint::to_value).collect()),
            ]),
        ])
    }

    pub fn from_value(value: Value) -> Result<Self> {
        let mut outer = ArrayFields::new(value, "revision index")?;
        outer.expect_schema(REVISION_INDEX_SCHEMA)?;
        let mut fields = ArrayFields::new(
            outer.next("revision index fields")?,
            "revision index fields",
        )?;
        outer.end("revision index")?;
        let log = RevisionLog::from_value(fields.next("revision log")?)?;
        let checkpoints = array_items(fields.next("checkpoints")?, "checkpoints")?
            .into_iter()
            .map(Checkpoint::from_value)
            .collect::<Result<Vec<_>>>()?;
        fields.end("revision index fields")?;
        let mut index = RevisionIndex::new();
        for revision in log.revisions() {
            index.append_revision(revision.clone())?;
        }
        for checkpoint in checkpoints {
            index.add_checkpoint(checkpoint)?;
        }
        Ok(index)
    }
}

fn array_items(value: Value, name: &str) -> Result<Vec<Value>> {
    match value {
        Value::Array(values) => Ok(values),
        _ => Err(LoomError::corrupt(format!("{name} must be an array"))),
    }
}

struct ArrayFields {
    values: std::vec::IntoIter<Value>,
}

impl ArrayFields {
    fn new(value: Value, name: &str) -> Result<Self> {
        Ok(Self {
            values: array_items(value, name)?.into_iter(),
        })
    }

    fn next(&mut self, name: &str) -> Result<Value> {
        self.values
            .next()
            .ok_or_else(|| LoomError::corrupt(format!("{name} is missing")))
    }

    fn expect_schema(&mut self, schema: &str) -> Result<()> {
        match self.next("schema")? {
            Value::Text(value) if value == schema => Ok(()),
            _ => Err(LoomError::corrupt(format!("expected schema {schema}"))),
        }
    }

    fn text(&mut self, name: &str) -> Result<String> {
        match self.next(name)? {
            Value::Text(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be text"))),
        }
    }

    fn uint(&mut self, name: &str) -> Result<u64> {
        match self.next(name)? {
            Value::Uint(value) => Ok(value),
            _ => Err(LoomError::corrupt(format!("{name} must be uint"))),
        }
    }

    fn optional_bool(&mut self, name: &str) -> Result<Option<bool>> {
        match self.values.next() {
            Some(Value::Bool(value)) => Ok(Some(value)),
            Some(_) => Err(LoomError::corrupt(format!("{name} must be bool"))),
            None => Ok(None),
        }
    }

    fn end(&mut self, name: &str) -> Result<()> {
        if self.values.next().is_some() {
            return Err(LoomError::corrupt(format!("{name} has trailing fields")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{
        CommitReceipt, MemoryStore, MutableOverlay, MutableOverlayEntrySnapshot, OverlayGeneration,
        OverlayOwnerToken, WorkflowControlWrite, WorkflowOwnerState, WorkflowReferenceUpdate,
        WriteOutcome,
    };
    use loom_types::Algo;
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Default)]
    struct TransactionalPointStore {
        objects: Mutex<BTreeMap<Digest, Vec<u8>>>,
        overlay: Mutex<MutableOverlay>,
        enumerations: AtomicUsize,
        retained_reads: AtomicUsize,
    }

    impl TransactionalPointStore {
        fn enumerations(&self) -> usize {
            self.enumerations.load(Ordering::Relaxed)
        }

        fn retained_reads(&self) -> usize {
            self.retained_reads.load(Ordering::Relaxed)
        }
    }

    impl ObjectStore for TransactionalPointStore {
        fn put(&self, canonical: &[u8]) -> Result<Digest> {
            let digest = Digest::blake3(canonical);
            self.objects
                .lock()
                .map_err(|_| LoomError::new(Code::Internal, "object lock poisoned"))?
                .insert(digest, canonical.to_vec());
            Ok(digest)
        }

        fn get(&self, digest: &Digest) -> Result<Option<Vec<u8>>> {
            Ok(self
                .objects
                .lock()
                .map_err(|_| LoomError::new(Code::Internal, "object lock poisoned"))?
                .get(digest)
                .cloned())
        }

        fn has(&self, digest: &Digest) -> Result<bool> {
            Ok(self
                .objects
                .lock()
                .map_err(|_| LoomError::new(Code::Internal, "object lock poisoned"))?
                .contains_key(digest))
        }

        fn len(&self) -> usize {
            self.objects
                .lock()
                .map(|objects| objects.len())
                .unwrap_or(0)
        }

        fn uses_mutable_overlay_current_records(&self) -> bool {
            true
        }

        fn mutable_overlay_current_entries(&self) -> Result<Vec<MutableOverlayEntrySnapshot>> {
            self.enumerations.fetch_add(1, Ordering::Relaxed);
            self.overlay
                .lock()
                .map_err(|_| LoomError::new(Code::Internal, "overlay lock poisoned"))?
                .export_entries()
        }

        fn mutable_overlay_current_entry(
            &self,
            key: &OverlayKey,
        ) -> Result<Option<MutableOverlayEntrySnapshot>> {
            Ok(self
                .overlay
                .lock()
                .map_err(|_| LoomError::new(Code::Internal, "overlay lock poisoned"))?
                .current_entry(key))
        }

        fn mutable_overlay_owner_token(
            &self,
            key: &OverlayKey,
        ) -> Result<Option<OverlayOwnerToken>> {
            Ok(self
                .overlay
                .lock()
                .map_err(|_| LoomError::new(Code::Internal, "overlay lock poisoned"))?
                .current_entry(key)
                .map(|entry| entry.owner_token))
        }

        fn mutable_overlay_generation(&self) -> Result<OverlayGeneration> {
            Ok(self
                .overlay
                .lock()
                .map_err(|_| LoomError::new(Code::Internal, "overlay lock poisoned"))?
                .generation())
        }

        fn retained_history_records(
            &self,
            _key: &[u8],
            _first_sequence: u64,
            _max: usize,
        ) -> Result<Vec<Vec<u8>>> {
            self.retained_reads.fetch_add(1, Ordering::Relaxed);
            Ok(Vec::new())
        }

        fn commit_workflow_transaction(&self, txn: WorkflowTransaction) -> Result<CommitReceipt> {
            txn.validate()?;
            if !txn.owner_state.is_empty() {
                return Err(LoomError::unsupported(
                    "test store does not support workflow owner state",
                ));
            }
            let mut overlay = self
                .overlay
                .lock()
                .map_err(|_| LoomError::new(Code::Internal, "overlay lock poisoned"))?;
            if txn
                .expected_generation
                .is_some_and(|expected| expected != overlay.generation())
            {
                return Err(LoomError::new(
                    Code::Conflict,
                    "workflow transaction overlay generation is stale",
                ));
            }
            let before = overlay.export_entries()?;
            let mut outcomes = Vec::new();
            for write in txn.writes {
                let expected = write.expected.as_ref().map(|token| &token.0);
                let token = match &write.op {
                    FacetWriteOp::Put { payload } => {
                        overlay.put_value(write.target.clone(), expected, payload.clone())
                    }
                    FacetWriteOp::Delete => overlay.put_tombstone(write.target.clone(), expected),
                };
                let token = match token {
                    Ok(token) => token,
                    Err(error) => {
                        *overlay = MutableOverlay::import_entries(&before)?;
                        return Err(error);
                    }
                };
                outcomes.push(WriteOutcome {
                    facet: write.facet,
                    target: write.target,
                    owner_token: token,
                    change: write.op.entry_kind(),
                });
            }
            let generation = overlay.generation();
            Ok(CommitReceipt {
                generation,
                root_after: Digest::blake3(&generation.as_u64().to_be_bytes()),
                writes: outcomes,
                operation_identities: Vec::new(),
                revision_identities: Vec::new(),
                audit_sequences: Vec::new(),
                retained_sequences: Vec::new(),
                delivery_receipts: Vec::new(),
                post_commit_delta: None,
                replayed: false,
            })
        }
    }

    fn digest(value: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, value)
    }

    fn migrate_legacy_revision_index_for_test(
        loom: &mut Loom<TransactionalPointStore>,
        workspace: WorkspaceId,
        scope_id: &str,
    ) -> Result<bool> {
        let path = revision_index_path(scope_id)?;
        let legacy_bytes = match loom.read_file_reserved(workspace, &path) {
            Ok(bytes) => bytes,
            Err(error) if error.code == Code::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        let legacy = RevisionIndex::decode(&legacy_bytes)?;
        match load_optional_current_revision_index(loom, workspace, scope_id)? {
            Some(current) if current != legacy => {
                return Err(LoomError::new(
                    Code::Conflict,
                    "legacy revision index differs from workspace-qualified destination",
                ));
            }
            Some(_) => {}
            None => persist_current_revision_index(
                loom,
                workspace,
                scope_id,
                FacetKind::Document,
                &legacy,
            )?,
        }
        if load_current_revision_index(loom, workspace, scope_id)? != legacy {
            return Err(LoomError::corrupt(
                "workspace-qualified revision index failed migration validation",
            ));
        }
        loom.remove_file_reserved(workspace, &path)?;
        Ok(true)
    }

    #[test]
    fn current_revision_index_round_trips_through_mutable_overlay() {
        let workspace = WorkspaceId::v4_from_bytes([7; 16]);
        let mut loom = Loom::new(TransactionalPointStore::default());
        let mut index = RevisionIndex::new();
        index
            .append_revision(
                EntityRevision::new(
                    "page:one",
                    1,
                    "op-1",
                    BodyRef::new(digest(b"body"), 4, "text/plain").unwrap(),
                    digest(b"root"),
                    10,
                )
                .unwrap(),
            )
            .unwrap();

        persist_current_revision_index(&mut loom, workspace, "studio", FacetKind::Document, &index)
            .unwrap();

        assert_eq!(
            load_current_revision_index(&loom, workspace, "studio").unwrap(),
            index
        );
        assert!(
            loom.read_file_reserved(workspace, &revision_index_path("studio").unwrap())
                .is_err()
        );
    }

    #[test]
    fn current_revision_index_isolated_by_workspace_and_uses_point_access() {
        let first_workspace = WorkspaceId::v4_from_bytes([8; 16]);
        let second_workspace = WorkspaceId::v4_from_bytes([9; 16]);
        let mut loom = Loom::new(TransactionalPointStore::default());
        let mut first = RevisionIndex::new();
        first
            .append_revision(
                EntityRevision::new(
                    "page:first",
                    1,
                    "op-first",
                    BodyRef::new(digest(b"first"), 5, "text/plain").unwrap(),
                    digest(b"first-root"),
                    1,
                )
                .unwrap(),
            )
            .unwrap();
        let mut second = RevisionIndex::new();
        second
            .append_revision(
                EntityRevision::new(
                    "page:second",
                    1,
                    "op-second",
                    BodyRef::new(digest(b"second"), 6, "text/plain").unwrap(),
                    digest(b"second-root"),
                    2,
                )
                .unwrap(),
            )
            .unwrap();

        persist_current_revision_index(
            &mut loom,
            first_workspace,
            "studio",
            FacetKind::Document,
            &first,
        )
        .unwrap();
        persist_current_revision_index(
            &mut loom,
            second_workspace,
            "studio",
            FacetKind::Document,
            &second,
        )
        .unwrap();

        assert_eq!(
            load_current_revision_index(&loom, first_workspace, "studio").unwrap(),
            first
        );
        assert_eq!(
            load_current_revision_index(&loom, second_workspace, "studio").unwrap(),
            second
        );
        assert_ne!(
            revision_index_current_key(first_workspace, "studio").unwrap(),
            revision_index_current_key(second_workspace, "studio").unwrap()
        );
        assert_eq!(loom.store().enumerations(), 0);
    }

    #[test]
    fn complete_point_index_avoids_retained_history_for_unknown_entity() {
        let workspace = WorkspaceId::v4_from_bytes([10; 16]);
        let loom = Loom::new(TransactionalPointStore::default());
        let key = revision_index_current_key(workspace, "studio").unwrap();
        let payload = encode_revision_index_manifest(42, true).unwrap();
        let expected_generation = loom.store().mutable_overlay_generation().unwrap();
        loom.store()
            .commit_workflow_transaction(WorkflowTransaction {
                workspace,
                actor: workspace,
                expected_generation: Some(expected_generation),
                writes: vec![FacetWrite {
                    facet: FacetKind::Document,
                    target: key,
                    op: FacetWriteOp::Put { payload },
                    secondary_indexes: Vec::new(),
                    expected: None,
                    durability: None,
                    audit: None,
                    side_effects: FacetSideEffects::default(),
                }],
                durability: OverlayDurabilityPolicy::Normal,
                boundary: AtomicityBoundary::Single,
                idempotency: None,
                owner_state: WorkflowOwnerState::default(),
            })
            .unwrap();

        assert_eq!(
            load_latest_entity_revision(&loom, workspace, "studio", "page:new").unwrap(),
            None
        );
        assert_eq!(loom.store().retained_reads(), 0);
    }

    #[test]
    fn unsupported_provider_does_not_apply_partial_revision_transaction() {
        let workspace = WorkspaceId::v4_from_bytes([10; 16]);
        let mut loom = Loom::new(MemoryStore::new());
        let extra_key =
            OverlayKey::from_segments([b"test", workspace.as_bytes(), b"extra", b"", b"", b"v1"])
                .unwrap();
        let extra_write = FacetWrite {
            facet: FacetKind::Document,
            target: extra_key.clone(),
            op: FacetWriteOp::Put {
                payload: b"extra".to_vec(),
            },
            secondary_indexes: Vec::new(),
            expected: None,
            durability: None,
            audit: None,
            side_effects: FacetSideEffects::default(),
        };
        let owner_state = WorkflowOwnerState {
            objects: vec![(digest(b"owner"), b"owner".to_vec())],
            reference: WorkflowReferenceUpdate::Set(Some(digest(b"owner"))),
            controls: vec![WorkflowControlWrite::Put {
                key: b"owner-control".to_vec(),
                payload: b"value".to_vec(),
            }],
            audits: Vec::new(),
        };
        let error = persist_current_revision_index_with_owner_state_and_writes(
            &mut loom,
            workspace,
            "studio",
            FacetKind::Document,
            &RevisionIndex::new(),
            vec![extra_write],
            None,
            owner_state,
        )
        .unwrap_err();

        assert_eq!(error.code, Code::Unsupported);
        assert!(
            loom.mutable_overlay_snapshot()
                .read_composite(&extra_key, |_| Ok(None))
                .unwrap()
                .is_none()
        );
        assert!(
            loom.mutable_overlay_snapshot()
                .read_composite(
                    &revision_index_current_key(workspace, "studio").unwrap(),
                    |_| Ok(None),
                )
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn controlled_legacy_revision_index_migration_is_restart_safe() {
        let workspace = WorkspaceId::v4_from_bytes([11; 16]);
        let mut loom = Loom::new(TransactionalPointStore::default());
        loom.registry_mut()
            .create_workspace(Some("migration"), workspace)
            .unwrap();
        let mut legacy = RevisionIndex::new();
        legacy
            .append_revision(
                EntityRevision::new(
                    "ticket:one",
                    1,
                    "op-1",
                    BodyRef::new(digest(b"legacy"), 6, "application/cbor").unwrap(),
                    digest(b"legacy-root"),
                    1,
                )
                .unwrap(),
            )
            .unwrap();
        let path = revision_index_path("tickets").unwrap();
        loom.create_directory_reserved(workspace, REVISION_INDEX_DIR, true)
            .unwrap();
        loom.write_file_reserved(workspace, &path, &legacy.encode().unwrap(), 0o100644)
            .unwrap();

        persist_current_revision_index(
            &mut loom,
            workspace,
            "tickets",
            FacetKind::Document,
            &legacy,
        )
        .unwrap();
        assert!(migrate_legacy_revision_index_for_test(&mut loom, workspace, "tickets").unwrap());
        assert!(loom.read_file_reserved(workspace, &path).is_err());
        assert_eq!(
            load_current_revision_index(&loom, workspace, "tickets").unwrap(),
            legacy
        );
        assert!(!migrate_legacy_revision_index_for_test(&mut loom, workspace, "tickets").unwrap());
    }

    #[test]
    fn controlled_legacy_revision_index_migration_rejects_collision() {
        let workspace = WorkspaceId::v4_from_bytes([12; 16]);
        let mut loom = Loom::new(TransactionalPointStore::default());
        loom.registry_mut()
            .create_workspace(Some("migration-collision"), workspace)
            .unwrap();
        let mut legacy = RevisionIndex::new();
        legacy
            .append_revision(
                EntityRevision::new(
                    "ticket:legacy",
                    1,
                    "op-legacy",
                    BodyRef::new(digest(b"legacy"), 6, "application/cbor").unwrap(),
                    digest(b"legacy-root"),
                    1,
                )
                .unwrap(),
            )
            .unwrap();
        let mut current = RevisionIndex::new();
        current
            .append_revision(
                EntityRevision::new(
                    "ticket:current",
                    1,
                    "op-current",
                    BodyRef::new(digest(b"current"), 7, "application/cbor").unwrap(),
                    digest(b"current-root"),
                    2,
                )
                .unwrap(),
            )
            .unwrap();
        let path = revision_index_path("tickets").unwrap();
        loom.create_directory_reserved(workspace, REVISION_INDEX_DIR, true)
            .unwrap();
        loom.write_file_reserved(workspace, &path, &legacy.encode().unwrap(), 0o100644)
            .unwrap();
        persist_current_revision_index(
            &mut loom,
            workspace,
            "tickets",
            FacetKind::Document,
            &current,
        )
        .unwrap();

        let error =
            migrate_legacy_revision_index_for_test(&mut loom, workspace, "tickets").unwrap_err();
        assert_eq!(error.code, Code::Conflict);
        assert_eq!(
            RevisionIndex::decode(&loom.read_file_reserved(workspace, &path).unwrap()).unwrap(),
            legacy
        );
        assert_eq!(
            load_current_revision_index(&loom, workspace, "tickets").unwrap(),
            current
        );
    }

    #[test]
    fn revision_log_assigns_monotonic_entity_revisions() {
        let mut log = RevisionLog::new();
        let body = BodyRef::new(digest(b"v1"), 2, "text/plain").unwrap();
        log.append(
            EntityRevision::new("ISSUE-1", 1, "op-1", body.clone(), digest(b"root-1"), 10).unwrap(),
        )
        .unwrap();
        assert_eq!(
            log.append(
                EntityRevision::new("ISSUE-1", 3, "op-3", body, digest(b"root-3"), 30).unwrap()
            )
            .unwrap_err()
            .code,
            loom_types::Code::Conflict
        );
    }

    #[test]
    fn revision_log_supports_latest_revision_and_root_lookup() {
        let mut log = RevisionLog::new();
        let root_1 = digest(b"root-1");
        let root_2 = digest(b"root-2");
        log.append(
            EntityRevision::new(
                "ISSUE-1",
                1,
                "op-1",
                BodyRef::new(digest(b"v1"), 2, "text/plain").unwrap(),
                root_1,
                10,
            )
            .unwrap(),
        )
        .unwrap();
        log.append(
            EntityRevision::new(
                "ISSUE-1",
                2,
                "op-2",
                BodyRef::new(digest(b"v2"), 2, "text/plain").unwrap(),
                root_2,
                20,
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(log.latest("ISSUE-1").unwrap().revision, 2);
        assert_eq!(log.at_revision("ISSUE-1", 1).unwrap().operation_id, "op-1");
        assert_eq!(log.as_of_root("ISSUE-1", &root_2).unwrap().revision, 2);
        let bytes = log.encode().unwrap();
        assert_eq!(RevisionLog::decode(&bytes).unwrap(), log);
    }

    #[test]
    fn checkpoint_encodes_scope_root_and_revision_boundary() {
        let checkpoint =
            Checkpoint::new("PROJ", "ready", digest(b"root"), 7, "op-ready", 40).unwrap();
        let bytes = checkpoint.encode().unwrap();
        assert_eq!(Checkpoint::decode(&bytes).unwrap(), checkpoint);
    }

    #[test]
    fn revision_index_projects_history_and_checkpoints() {
        let root_1 = digest(b"root-1");
        let root_2 = digest(b"root-2");
        let root_3 = digest(b"root-3");
        let mut index = RevisionIndex::new();
        for (revision, root, timestamp_ms) in [(1, root_1, 10), (2, root_2, 20), (3, root_3, 30)] {
            index
                .append_revision(
                    EntityRevision::new(
                        "ISSUE-1",
                        revision,
                        format!("op-{revision}"),
                        BodyRef::new(digest(format!("v{revision}").as_bytes()), 2, "text/plain")
                            .unwrap(),
                        root,
                        timestamp_ms,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        index
            .add_checkpoint(Checkpoint::new("PROJ", "cp-1", root_1, 1, "op-1", 11).unwrap())
            .unwrap();
        index
            .add_checkpoint(Checkpoint::new("PROJ", "cp-3", root_3, 3, "op-3", 31).unwrap())
            .unwrap();

        assert_eq!(index.history("ISSUE-1").len(), 3);
        assert_eq!(index.latest("ISSUE-1").unwrap().revision, 3);
        assert_eq!(index.at_revision("ISSUE-1", 2).unwrap().root, root_2);
        assert_eq!(index.as_of_root("ISSUE-1", &root_2).unwrap().revision, 2);
        assert_eq!(
            index
                .checkpoint_before_or_at("PROJ", 2)
                .unwrap()
                .checkpoint_id,
            "cp-1"
        );
        assert_eq!(
            index
                .checkpoint_before_or_at("PROJ", 3)
                .unwrap()
                .checkpoint_id,
            "cp-3"
        );
        assert!(index.checkpoint_before_or_at("PROJ", 0).is_none());
        assert_eq!(
            RevisionIndex::decode(&index.encode().unwrap()).unwrap(),
            index
        );
    }

    #[test]
    fn revision_index_rejects_duplicate_checkpoint_ids_per_scope() {
        let root = digest(b"root");
        let mut index = RevisionIndex::new();
        index
            .add_checkpoint(Checkpoint::new("PROJ", "cp", root, 1, "op-1", 10).unwrap())
            .unwrap();
        assert_eq!(
            index
                .add_checkpoint(Checkpoint::new("PROJ", "cp", root, 2, "op-2", 20).unwrap())
                .unwrap_err()
                .code,
            Code::AlreadyExists
        );
    }

    #[test]
    fn revision_index_backfills_missing_current_rows_once() {
        let root = digest(b"root");
        let mut index = RevisionIndex::new();
        index
            .append_revision(
                EntityRevision::new(
                    "page:existing",
                    1,
                    "op-existing",
                    BodyRef::new(digest(b"existing"), 8, "text/plain").unwrap(),
                    root,
                    10,
                )
                .unwrap(),
            )
            .unwrap();
        index
            .add_checkpoint(
                Checkpoint::new("studio", "page:existing:1", root, 1, "op-existing", 10).unwrap(),
            )
            .unwrap();

        let report = index
            .backfill_missing_current(
                "studio",
                vec![
                    RevisionBackfillUpdate::new(
                        "page:existing",
                        "op-existing-backfill",
                        BodyRef::new(digest(b"existing"), 8, "text/plain").unwrap(),
                        root,
                        20,
                        "page:existing:backfill:1",
                    )
                    .unwrap(),
                    RevisionBackfillUpdate::new(
                        "page:new",
                        "op-new-backfill",
                        BodyRef::new(digest(b"new"), 3, "text/plain").unwrap(),
                        root,
                        20,
                        "page:new:backfill:1",
                    )
                    .unwrap(),
                ],
            )
            .unwrap();

        assert_eq!(report.inserted, 1);
        assert_eq!(report.skipped_existing, 1);
        assert_eq!(index.history("page:existing").len(), 1);
        assert_eq!(index.history("page:new").len(), 1);
        assert_eq!(
            index.latest("page:new").unwrap().operation_id,
            "op-new-backfill"
        );
        assert!(
            index
                .checkpoints()
                .iter()
                .any(|checkpoint| checkpoint.checkpoint_id == "page:new:backfill:1")
        );
    }

    #[test]
    fn profile_transaction_compares_root_and_advances_revision_index() {
        let root_1 = digest(b"root-1");
        let root_2 = digest(b"root-2");
        let mut state = ProfileTransactionState::new(root_1, RevisionIndex::new());

        let receipt = state
            .apply(
                ProfileTransaction::new(
                    "studio",
                    Some(root_1),
                    root_2,
                    vec![
                        ProfileRevisionUpdate::new(
                            "page:one",
                            "op-1",
                            BodyRef::new(digest(b"body-1"), 6, "text/plain").unwrap(),
                            10,
                            "page:one:1",
                            Some(0),
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
            )
            .unwrap();

        assert_eq!(receipt.root_before, root_1);
        assert_eq!(receipt.root_after, root_2);
        assert_eq!(receipt.revisions[0].revision, 1);
        assert_eq!(state.root(), root_2);
        assert_eq!(
            state
                .revision_index()
                .latest("page:one")
                .unwrap()
                .operation_id,
            "op-1"
        );
        assert_eq!(
            state
                .revision_index()
                .checkpoint_before_or_at("studio", 1)
                .unwrap()
                .checkpoint_id,
            "page:one:1"
        );
    }

    #[test]
    fn profile_transaction_conflict_leaves_state_unchanged() {
        let root_1 = digest(b"root-1");
        let root_2 = digest(b"root-2");
        let root_3 = digest(b"root-3");
        let mut state = ProfileTransactionState::new(root_1, RevisionIndex::new());
        state
            .apply(
                ProfileTransaction::new(
                    "studio",
                    Some(root_1),
                    root_2,
                    vec![
                        ProfileRevisionUpdate::new(
                            "ticket:one",
                            "op-1",
                            BodyRef::new(digest(b"body-1"), 6, "text/plain").unwrap(),
                            10,
                            "ticket:one:1",
                            Some(0),
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
            )
            .unwrap();

        let conflict = state
            .apply(
                ProfileTransaction::new(
                    "studio",
                    Some(root_1),
                    root_3,
                    vec![
                        ProfileRevisionUpdate::new(
                            "ticket:one",
                            "op-2",
                            BodyRef::new(digest(b"body-2"), 6, "text/plain").unwrap(),
                            20,
                            "ticket:one:2",
                            Some(1),
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
            )
            .unwrap_err();

        assert_eq!(conflict.code, Code::Conflict);
        assert_eq!(state.root(), root_2);
        assert_eq!(
            state
                .revision_index()
                .latest("ticket:one")
                .unwrap()
                .revision,
            1
        );
        assert_eq!(state.revision_index().checkpoints().len(), 1);
    }

    #[test]
    fn profile_transaction_checks_expected_entity_revision_atomically() {
        let root_1 = digest(b"root-1");
        let root_2 = digest(b"root-2");
        let mut state = ProfileTransactionState::new(root_1, RevisionIndex::new());
        let conflict = state
            .apply(
                ProfileTransaction::new(
                    "studio",
                    Some(root_1),
                    root_2,
                    vec![
                        ProfileRevisionUpdate::new(
                            "meeting:one",
                            "op-1",
                            BodyRef::new(digest(b"body-1"), 6, "text/plain").unwrap(),
                            10,
                            "meeting:one:1",
                            Some(1),
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
            )
            .unwrap_err();

        assert_eq!(conflict.code, Code::Conflict);
        assert_eq!(state.root(), root_1);
        assert!(state.revision_index().latest("meeting:one").is_none());
        assert!(state.revision_index().checkpoints().is_empty());
    }
}
