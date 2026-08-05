use crate::{
    Code, Digest, FacetKind, LoomError, OverlayDurabilityPolicy, OverlayEntryKind,
    OverlayGeneration, OverlayKey, OverlayOwnerToken, OverlayReadSnapshot, PrincipalId, Result,
    WorkspaceId,
};
use loom_types::IdempotencyKey;

pub const WORKFLOW_RECEIPT_MAX_WRITES: usize = 4096;
pub const WORKFLOW_RECEIPT_MAX_OPERATIONS: usize = 4096;
pub const WORKFLOW_RECEIPT_MAX_REVISIONS: usize = 4096;
pub const WORKFLOW_RECEIPT_MAX_AUDIT_SEQUENCES: usize = 4096;
pub const WORKFLOW_RECEIPT_MAX_RETAINED_SEQUENCES: usize = 4096;
pub const WORKFLOW_RECEIPT_MAX_DELIVERY_RECEIPTS: usize = 4096;
pub const WORKFLOW_RECEIPT_MAX_CHANGED_PATHS: usize = 4096;
pub const WORKFLOW_RECEIPT_MAX_KEY_BYTES: usize = 16 * 1024;
pub const WORKFLOW_RECEIPT_MAX_STRING_BYTES: usize = 16 * 1024;
pub const WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
pub const WORKFLOW_RECEIPT_MAX_CHANGED_CONTENT_COUNT: u64 = 4096;
pub const WORKFLOW_TRANSACTION_MAX_AGGREGATE_ENCODED_BYTES: usize = 2 * 1024 * 1024;
pub const WORKFLOW_TRANSACTION_MAX_AUDIT_ACTION_BYTES: usize = 128;
pub const WORKFLOW_TRANSACTION_MAX_AUDIT_TARGET_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkflowAggregateByteBudget {
    used: usize,
}

impl WorkflowAggregateByteBudget {
    pub const fn new() -> Self {
        Self { used: 0 }
    }

    pub const fn used(self) -> usize {
        self.used
    }

    pub fn reserve(&mut self, bytes: usize) -> bool {
        let Some(next) = self.used.checked_add(bytes) else {
            return false;
        };
        if next > WORKFLOW_TRANSACTION_MAX_AGGREGATE_ENCODED_BYTES {
            return false;
        }
        self.used = next;
        true
    }
}

pub const fn workflow_varint_encoded_len(mut value: u64) -> usize {
    let mut len = 1;
    while value >= 0x80 {
        len += 1;
        value >>= 7;
    }
    len
}

#[derive(Debug)]
pub struct WorkflowPlanningSnapshot {
    read: OverlayReadSnapshot,
}

impl WorkflowPlanningSnapshot {
    pub fn open<S: crate::ObjectStore>(store: &S, owner: Option<&str>) -> Result<Self> {
        Ok(Self {
            read: store.open_workflow_planning_snapshot(owner)?,
        })
    }

    pub fn expected_generation(&self) -> OverlayGeneration {
        self.read.overlay_generation()
    }

    pub fn immutable_base_root(&self) -> Option<Digest> {
        self.read.immutable_base_root()
    }

    pub fn fork_overlay(&self) -> crate::MutableOverlay {
        self.read.fork_overlay()
    }

    pub fn owner_token(&self, key: &OverlayKey) -> Result<Option<OverlayOwnerToken>> {
        self.read.owner_token(key)
    }

    pub fn read_composite(
        &self,
        key: &OverlayKey,
        base_read: impl FnOnce(Option<Digest>, &OverlayKey) -> Result<Option<Vec<u8>>>,
    ) -> Result<Option<Vec<u8>>> {
        self.read.read_composite(key, base_read)
    }

    pub fn release(&self) -> Result<bool> {
        self.read.release()
    }
}

#[derive(Debug)]
pub struct BoundedMutationPlan {
    pub workspace: WorkspaceId,
    pub actor: PrincipalId,
    pub writes: Vec<FacetWrite>,
    pub owner_state: WorkflowOwnerState,
    pub post_commit: crate::EngineStateDelta,
    pub durability: OverlayDurabilityPolicy,
    snapshot: WorkflowPlanningSnapshot,
}

impl BoundedMutationPlan {
    pub fn new(
        snapshot: WorkflowPlanningSnapshot,
        workspace: WorkspaceId,
        actor: PrincipalId,
        writes: Vec<FacetWrite>,
        owner_state: WorkflowOwnerState,
        post_commit: crate::EngineStateDelta,
        durability: OverlayDurabilityPolicy,
    ) -> Self {
        Self {
            workspace,
            actor,
            writes,
            owner_state,
            post_commit,
            durability,
            snapshot,
        }
    }

    pub fn expected_generation(&self) -> OverlayGeneration {
        self.snapshot.expected_generation()
    }

    pub fn into_transaction(self) -> (WorkflowTransaction, crate::EngineStateDelta) {
        (
            WorkflowTransaction {
                workspace: self.workspace,
                actor: self.actor,
                expected_generation: Some(self.snapshot.expected_generation()),
                writes: self.writes,
                prepared_operations: Vec::new(),
                revision_metadata: Vec::new(),
                delivery_intents: Vec::new(),
                durability: self.durability,
                boundary: AtomicityBoundary::Single,
                idempotency: None,
                owner_state: self.owner_state,
                post_commit_delta: Some(self.post_commit.clone()),
            },
            self.post_commit,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowTransaction {
    pub workspace: WorkspaceId,
    pub actor: PrincipalId,
    pub expected_generation: Option<OverlayGeneration>,
    pub writes: Vec<FacetWrite>,
    pub prepared_operations: Vec<PreparedOperation>,
    pub revision_metadata: Vec<PreparedRevisionMetadata>,
    pub delivery_intents: Vec<PreparedDeliveryIntent>,
    pub durability: OverlayDurabilityPolicy,
    pub boundary: AtomicityBoundary,
    pub idempotency: Option<IdempotencyKey>,
    pub owner_state: WorkflowOwnerState,
    pub post_commit_delta: Option<crate::EngineStateDelta>,
}

impl WorkflowTransaction {
    pub fn effective_durability(&self) -> OverlayDurabilityPolicy {
        strictest_durability(
            std::iter::once(self.durability)
                .chain(self.writes.iter().filter_map(|write| write.durability)),
        )
    }

    pub fn validate(&self) -> Result<()> {
        if self.writes.is_empty()
            && self.owner_state.is_empty()
            && self.prepared_operations.is_empty()
            && self.revision_metadata.is_empty()
            && self.delivery_intents.is_empty()
            && self.post_commit_delta.is_none()
        {
            return Err(LoomError::invalid(
                "workflow transaction write set must not be empty",
            ));
        }
        if self.boundary != AtomicityBoundary::Single {
            return Err(WorkflowTransactionErrorKind::UnsupportedOperation
                .into_error("workflow transaction boundary is not supported"));
        }
        if self.idempotency.is_some()
            && self.effective_durability() == OverlayDurabilityPolicy::Ephemeral
        {
            return Err(WorkflowTransactionErrorKind::UnhonoredDurabilityPolicy
                .into_error("ephemeral workflow transaction idempotency cannot be honored"));
        }
        let resolved = self.effective_durability();
        let has_ephemeral_write = self.writes.iter().any(|write| {
            write.durability.unwrap_or(self.durability) == OverlayDurabilityPolicy::Ephemeral
        });
        if resolved != OverlayDurabilityPolicy::Ephemeral && has_ephemeral_write {
            return Err(WorkflowTransactionErrorKind::UnhonoredDurabilityPolicy
                .into_error("ephemeral write cannot join a stronger single transaction"));
        }
        if !self.owner_state.is_empty() && resolved == OverlayDurabilityPolicy::Ephemeral {
            return Err(WorkflowTransactionErrorKind::UnhonoredDurabilityPolicy
                .into_error("owner state cannot join an ephemeral workflow transaction"));
        }
        let mut aggregate = WorkflowAggregateByteBudget::new();
        reserve_workflow_aggregate(
            &mut aggregate,
            workflow_varint_encoded_len(self.writes.len() as u64),
        )?;
        validate_workflow_count(
            "workflow write count",
            self.writes.len(),
            WORKFLOW_RECEIPT_MAX_WRITES,
        )?;
        for write in &self.writes {
            reserve_workflow_aggregate(&mut aggregate, 1)?;
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(write.target.as_bytes().len() as u64),
            )?;
            reserve_workflow_aggregate(&mut aggregate, write.target.as_bytes().len())?;
            reserve_workflow_aggregate(&mut aggregate, 32)?;
            reserve_workflow_aggregate(&mut aggregate, 1)?;
            validate_workflow_bytes(
                "workflow write target",
                write.target.as_bytes(),
                WORKFLOW_RECEIPT_MAX_KEY_BYTES,
            )?;
            if let FacetWriteOp::Put { payload } = &write.op {
                reserve_workflow_aggregate(
                    &mut aggregate,
                    workflow_varint_encoded_len(payload.len() as u64),
                )?;
                reserve_workflow_aggregate(&mut aggregate, payload.len())?;
                validate_workflow_bytes(
                    "workflow write payload",
                    payload,
                    WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES,
                )?;
            }
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(write.secondary_indexes.len() as u64),
            )?;
            for secondary in &write.secondary_indexes {
                reserve_workflow_aggregate(
                    &mut aggregate,
                    workflow_varint_encoded_len(secondary.index.as_bytes().len() as u64),
                )?;
                reserve_workflow_aggregate(&mut aggregate, secondary.index.as_bytes().len())?;
                validate_workflow_bytes(
                    "workflow secondary-index target",
                    secondary.index.as_bytes(),
                    WORKFLOW_RECEIPT_MAX_KEY_BYTES,
                )?;
                if let SecondaryIndexWriteOp::Put { payload } = &secondary.op {
                    reserve_workflow_aggregate(
                        &mut aggregate,
                        workflow_varint_encoded_len(payload.len() as u64),
                    )?;
                    reserve_workflow_aggregate(&mut aggregate, payload.len())?;
                    validate_workflow_bytes(
                        "workflow secondary-index payload",
                        payload,
                        WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES,
                    )?;
                }
            }
        }
        reserve_workflow_aggregate(
            &mut aggregate,
            workflow_varint_encoded_len(self.prepared_operations.len() as u64),
        )?;
        validate_workflow_count(
            "prepared operation count",
            self.prepared_operations.len(),
            WORKFLOW_RECEIPT_MAX_OPERATIONS,
        )?;
        for operation in &self.prepared_operations {
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(operation.operation_id.len() as u64),
            )?;
            reserve_workflow_aggregate(&mut aggregate, operation.operation_id.len())?;
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(operation.payload.len() as u64),
            )?;
            reserve_workflow_aggregate(&mut aggregate, operation.payload.len())?;
            validate_workflow_string(
                "prepared operation id",
                &operation.operation_id,
                WORKFLOW_RECEIPT_MAX_STRING_BYTES,
            )?;
            validate_workflow_bytes(
                "prepared operation payload",
                &operation.payload,
                WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES,
            )?;
        }
        reserve_workflow_aggregate(
            &mut aggregate,
            workflow_varint_encoded_len(self.revision_metadata.len() as u64),
        )?;
        validate_workflow_count(
            "prepared revision count",
            self.revision_metadata.len(),
            WORKFLOW_RECEIPT_MAX_REVISIONS,
        )?;
        for revision in &self.revision_metadata {
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(revision.entity_id.len() as u64),
            )?;
            reserve_workflow_aggregate(&mut aggregate, revision.entity_id.len())?;
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(revision.revision_id.len() as u64),
            )?;
            reserve_workflow_aggregate(&mut aggregate, revision.revision_id.len())?;
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(revision.payload.len() as u64),
            )?;
            reserve_workflow_aggregate(&mut aggregate, revision.payload.len())?;
            validate_workflow_string(
                "prepared revision entity",
                &revision.entity_id,
                WORKFLOW_RECEIPT_MAX_STRING_BYTES,
            )?;
            validate_workflow_string(
                "prepared revision id",
                &revision.revision_id,
                WORKFLOW_RECEIPT_MAX_STRING_BYTES,
            )?;
            validate_workflow_bytes(
                "prepared revision payload",
                &revision.payload,
                WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES,
            )?;
        }
        reserve_workflow_aggregate(
            &mut aggregate,
            workflow_varint_encoded_len(self.owner_state.audits.len() as u64),
        )?;
        validate_workflow_count(
            "workflow audit count",
            self.owner_state.audits.len(),
            WORKFLOW_RECEIPT_MAX_AUDIT_SEQUENCES,
        )?;
        reserve_workflow_aggregate(
            &mut aggregate,
            workflow_varint_encoded_len(self.owner_state.objects.len() as u64),
        )?;
        for (_, payload) in &self.owner_state.objects {
            reserve_workflow_aggregate(&mut aggregate, 32)?;
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(payload.len() as u64),
            )?;
            reserve_workflow_aggregate(&mut aggregate, payload.len())?;
            validate_workflow_bytes(
                "workflow owner-state object payload",
                payload,
                WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES,
            )?;
        }
        reserve_workflow_aggregate(&mut aggregate, 1)?;
        if let WorkflowReferenceUpdate::Set(Some(_)) = self.owner_state.reference {
            reserve_workflow_aggregate(&mut aggregate, 32)?;
        }
        reserve_workflow_aggregate(
            &mut aggregate,
            workflow_varint_encoded_len(self.owner_state.controls.len() as u64),
        )?;
        let mut retained_append_count = 0usize;
        for write in &self.owner_state.controls {
            let key = match write {
                WorkflowControlWrite::Put { key, .. }
                | WorkflowControlWrite::Delete { key }
                | WorkflowControlWrite::AppendRetained { key, .. } => key,
            };
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(key.len() as u64),
            )?;
            reserve_workflow_aggregate(&mut aggregate, key.len())?;
            validate_workflow_bytes(
                "workflow control-write key",
                key,
                WORKFLOW_RECEIPT_MAX_KEY_BYTES,
            )?;
            if key.is_empty() {
                return Err(LoomError::invalid(
                    "workflow control-write key must not be empty",
                ));
            }
            match write {
                WorkflowControlWrite::Put { payload, .. } => {
                    reserve_workflow_aggregate(
                        &mut aggregate,
                        workflow_varint_encoded_len(payload.len() as u64),
                    )?;
                    reserve_workflow_aggregate(&mut aggregate, payload.len())?;
                    validate_workflow_bytes(
                        "workflow control-write payload",
                        payload,
                        WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES,
                    )?;
                }
                WorkflowControlWrite::Delete { .. } => {}
                WorkflowControlWrite::AppendRetained {
                    expected_next_sequence,
                    records,
                    ..
                } => {
                    retained_append_count += 1;
                    reserve_workflow_aggregate(
                        &mut aggregate,
                        workflow_varint_encoded_len(*expected_next_sequence),
                    )?;
                    reserve_workflow_aggregate(
                        &mut aggregate,
                        workflow_varint_encoded_len(records.len() as u64),
                    )?;
                    if records.is_empty() {
                        return Err(LoomError::invalid(
                            "retained-history append must include at least one record",
                        ));
                    }
                    if *expected_next_sequence == 0 {
                        return Err(LoomError::invalid(
                            "retained-history sequence must start at one",
                        ));
                    }
                    if records.iter().any(Vec::is_empty) {
                        return Err(LoomError::invalid(
                            "retained-history records must not be empty",
                        ));
                    }
                    validate_workflow_count(
                        "retained-history record count",
                        records.len(),
                        WORKFLOW_RECEIPT_MAX_RETAINED_SEQUENCES,
                    )?;
                    for record in records {
                        reserve_workflow_aggregate(
                            &mut aggregate,
                            workflow_varint_encoded_len(record.len() as u64),
                        )?;
                        reserve_workflow_aggregate(&mut aggregate, record.len())?;
                        validate_workflow_bytes(
                            "retained-history record payload",
                            record,
                            WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES,
                        )?;
                    }
                }
            }
        }
        validate_workflow_count(
            "retained-history append count",
            retained_append_count,
            WORKFLOW_RECEIPT_MAX_RETAINED_SEQUENCES,
        )?;
        for write in &self.owner_state.audits {
            if write.action.trim().is_empty() {
                return Err(LoomError::invalid(
                    "workflow audit-write action must not be blank",
                ));
            }
            validate_workflow_string(
                "workflow audit-write action",
                &write.action,
                WORKFLOW_TRANSACTION_MAX_AUDIT_ACTION_BYTES,
            )?;
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(write.action.len() as u64),
            )?;
            reserve_workflow_aggregate(&mut aggregate, write.action.len())?;
            if let Some(target) = &write.target {
                validate_workflow_string(
                    "workflow audit-write target",
                    target,
                    WORKFLOW_TRANSACTION_MAX_AUDIT_TARGET_BYTES,
                )?;
                reserve_workflow_aggregate(
                    &mut aggregate,
                    workflow_varint_encoded_len(target.len() as u64),
                )?;
                reserve_workflow_aggregate(&mut aggregate, target.len())?;
            }
        }
        if self
            .prepared_operations
            .iter()
            .any(|operation| operation.operation_id.trim().is_empty())
        {
            return Err(LoomError::invalid(
                "prepared operation id must not be blank",
            ));
        }
        if self.revision_metadata.iter().any(|revision| {
            revision.entity_id.trim().is_empty() || revision.revision_id.trim().is_empty()
        }) {
            return Err(LoomError::invalid(
                "prepared revision metadata must identify an entity and revision",
            ));
        }
        validate_workflow_count(
            "prepared delivery count",
            self.delivery_intents.len(),
            WORKFLOW_RECEIPT_MAX_DELIVERY_RECEIPTS,
        )?;
        reserve_workflow_aggregate(
            &mut aggregate,
            workflow_varint_encoded_len(self.delivery_intents.len() as u64),
        )?;
        for delivery in &self.delivery_intents {
            if delivery.stream_id.trim().is_empty() || delivery.envelope_id.trim().is_empty() {
                return Err(LoomError::invalid(
                    "prepared delivery intent must identify a stream and envelope",
                ));
            }
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(delivery.stream_id.len() as u64),
            )?;
            reserve_workflow_aggregate(&mut aggregate, delivery.stream_id.len())?;
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(delivery.sequence),
            )?;
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(delivery.envelope_id.len() as u64),
            )?;
            reserve_workflow_aggregate(&mut aggregate, delivery.envelope_id.len())?;
            reserve_workflow_aggregate(&mut aggregate, 32)?;
            validate_workflow_string(
                "prepared delivery stream",
                &delivery.stream_id,
                WORKFLOW_RECEIPT_MAX_STRING_BYTES,
            )?;
            validate_workflow_string(
                "prepared delivery envelope",
                &delivery.envelope_id,
                WORKFLOW_RECEIPT_MAX_STRING_BYTES,
            )?;
        }
        if let Some(delta) = &self.post_commit_delta {
            reserve_workflow_aggregate(&mut aggregate, 1)?;
            reserve_workflow_aggregate(&mut aggregate, 16)?;
            let changed_paths = delta.changed_paths();
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(changed_paths.len() as u64),
            )?;
            validate_workflow_count(
                "post-commit changed path count",
                changed_paths.len(),
                WORKFLOW_RECEIPT_MAX_CHANGED_PATHS,
            )?;
            for path in changed_paths {
                reserve_workflow_aggregate(
                    &mut aggregate,
                    workflow_varint_encoded_len(path.len() as u64),
                )?;
                reserve_workflow_aggregate(&mut aggregate, path.len())?;
                validate_workflow_string(
                    "post-commit changed path",
                    &path,
                    WORKFLOW_RECEIPT_MAX_STRING_BYTES,
                )?;
            }
            let changed_content_count = delta.changed_content_count() as u64;
            if changed_content_count > WORKFLOW_RECEIPT_MAX_CHANGED_CONTENT_COUNT {
                return Err(LoomError::invalid(
                    "post-commit changed content count too large",
                ));
            }
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(changed_content_count),
            )?;
        } else {
            reserve_workflow_aggregate(&mut aggregate, 1)?;
        }
        Ok(())
    }
}

impl CommitReceipt {
    pub fn aggregate_encoded_len(&self) -> Result<usize> {
        let mut aggregate = WorkflowAggregateByteBudget::new();
        reserve_workflow_aggregate(
            &mut aggregate,
            workflow_varint_encoded_len(self.generation.as_u64()),
        )?;
        reserve_workflow_aggregate(&mut aggregate, 32)?;
        reserve_workflow_aggregate(
            &mut aggregate,
            workflow_varint_encoded_len(self.writes.len() as u64),
        )?;
        validate_workflow_count(
            "workflow receipt write count",
            self.writes.len(),
            WORKFLOW_RECEIPT_MAX_WRITES,
        )?;
        for write in &self.writes {
            reserve_workflow_aggregate(&mut aggregate, 1)?;
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(write.target.as_bytes().len() as u64),
            )?;
            reserve_workflow_aggregate(&mut aggregate, write.target.as_bytes().len())?;
            reserve_workflow_aggregate(&mut aggregate, 32)?;
            reserve_workflow_aggregate(&mut aggregate, 1)?;
            validate_workflow_bytes(
                "workflow receipt write target",
                write.target.as_bytes(),
                WORKFLOW_RECEIPT_MAX_KEY_BYTES,
            )?;
        }
        reserve_workflow_aggregate(
            &mut aggregate,
            workflow_varint_encoded_len(self.operation_identities.len() as u64),
        )?;
        validate_workflow_count(
            "workflow receipt operation count",
            self.operation_identities.len(),
            WORKFLOW_RECEIPT_MAX_OPERATIONS,
        )?;
        for operation_id in &self.operation_identities {
            account_workflow_string(
                &mut aggregate,
                "workflow receipt operation id",
                operation_id,
            )?;
        }
        reserve_workflow_aggregate(
            &mut aggregate,
            workflow_varint_encoded_len(self.revision_identities.len() as u64),
        )?;
        validate_workflow_count(
            "workflow receipt revision count",
            self.revision_identities.len(),
            WORKFLOW_RECEIPT_MAX_REVISIONS,
        )?;
        for revision in &self.revision_identities {
            account_workflow_string(
                &mut aggregate,
                "workflow receipt revision entity",
                &revision.entity_id,
            )?;
            account_workflow_string(
                &mut aggregate,
                "workflow receipt revision id",
                &revision.revision_id,
            )?;
        }
        reserve_workflow_aggregate(
            &mut aggregate,
            workflow_varint_encoded_len(self.audit_sequences.len() as u64),
        )?;
        validate_workflow_count(
            "workflow receipt audit count",
            self.audit_sequences.len(),
            WORKFLOW_RECEIPT_MAX_AUDIT_SEQUENCES,
        )?;
        for sequence in &self.audit_sequences {
            reserve_workflow_aggregate(&mut aggregate, workflow_varint_encoded_len(*sequence))?;
        }
        reserve_workflow_aggregate(
            &mut aggregate,
            workflow_varint_encoded_len(self.retained_sequences.len() as u64),
        )?;
        validate_workflow_count(
            "workflow receipt retained count",
            self.retained_sequences.len(),
            WORKFLOW_RECEIPT_MAX_RETAINED_SEQUENCES,
        )?;
        for retained in &self.retained_sequences {
            account_workflow_bytes(
                &mut aggregate,
                "workflow receipt retained key",
                &retained.key,
            )?;
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(retained.first_sequence),
            )?;
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(retained.last_sequence),
            )?;
        }
        reserve_workflow_aggregate(
            &mut aggregate,
            workflow_varint_encoded_len(self.delivery_receipts.len() as u64),
        )?;
        validate_workflow_count(
            "workflow receipt delivery count",
            self.delivery_receipts.len(),
            WORKFLOW_RECEIPT_MAX_DELIVERY_RECEIPTS,
        )?;
        for delivery in &self.delivery_receipts {
            account_workflow_string(
                &mut aggregate,
                "workflow receipt delivery stream",
                &delivery.stream_id,
            )?;
            reserve_workflow_aggregate(
                &mut aggregate,
                workflow_varint_encoded_len(delivery.sequence),
            )?;
            account_workflow_string(
                &mut aggregate,
                "workflow receipt delivery envelope",
                &delivery.envelope_id,
            )?;
            reserve_workflow_aggregate(&mut aggregate, 32)?;
        }
        match &self.post_commit_delta {
            Some(delta) => {
                reserve_workflow_aggregate(&mut aggregate, 1)?;
                reserve_workflow_aggregate(&mut aggregate, 16)?;
                reserve_workflow_aggregate(
                    &mut aggregate,
                    workflow_varint_encoded_len(delta.changed_paths.len() as u64),
                )?;
                validate_workflow_count(
                    "workflow receipt changed path count",
                    delta.changed_paths.len(),
                    WORKFLOW_RECEIPT_MAX_CHANGED_PATHS,
                )?;
                for path in &delta.changed_paths {
                    account_workflow_string(&mut aggregate, "workflow receipt changed path", path)?;
                }
                let changed_content_count =
                    u64::try_from(delta.changed_content_count).map_err(|_| {
                        LoomError::invalid("workflow receipt changed content count invalid")
                    })?;
                if changed_content_count > WORKFLOW_RECEIPT_MAX_CHANGED_CONTENT_COUNT {
                    return Err(LoomError::invalid(
                        "workflow receipt changed content count too large",
                    ));
                }
                reserve_workflow_aggregate(
                    &mut aggregate,
                    workflow_varint_encoded_len(changed_content_count),
                )?;
            }
            None => reserve_workflow_aggregate(&mut aggregate, 1)?,
        }
        Ok(aggregate.used())
    }
}

fn validate_workflow_count(name: &str, count: usize, max: usize) -> Result<()> {
    if count > max {
        return Err(LoomError::invalid(format!("{name} exceeds maximum")));
    }
    Ok(())
}

fn validate_workflow_bytes(name: &str, bytes: &[u8], max: usize) -> Result<()> {
    if bytes.len() > max {
        return Err(LoomError::invalid(format!("{name} exceeds maximum length")));
    }
    Ok(())
}

fn validate_workflow_string(name: &str, value: &str, max: usize) -> Result<()> {
    validate_workflow_bytes(name, value.as_bytes(), max)
}

fn reserve_workflow_aggregate(
    aggregate: &mut WorkflowAggregateByteBudget,
    bytes: usize,
) -> Result<()> {
    if !aggregate.reserve(bytes) {
        return Err(LoomError::invalid(
            "workflow transaction aggregate encoded size exceeds maximum",
        ));
    }
    Ok(())
}

fn account_workflow_bytes(
    aggregate: &mut WorkflowAggregateByteBudget,
    name: &str,
    bytes: &[u8],
) -> Result<()> {
    validate_workflow_bytes(name, bytes, WORKFLOW_RECEIPT_MAX_KEY_BYTES)?;
    reserve_workflow_aggregate(aggregate, workflow_varint_encoded_len(bytes.len() as u64))?;
    reserve_workflow_aggregate(aggregate, bytes.len())
}

fn account_workflow_string(
    aggregate: &mut WorkflowAggregateByteBudget,
    name: &str,
    value: &str,
) -> Result<()> {
    validate_workflow_string(name, value, WORKFLOW_RECEIPT_MAX_STRING_BYTES)?;
    reserve_workflow_aggregate(aggregate, workflow_varint_encoded_len(value.len() as u64))?;
    reserve_workflow_aggregate(aggregate, value.len())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedOperation {
    pub operation_id: String,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedRevisionMetadata {
    pub entity_id: String,
    pub revision_id: String,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedDeliveryIntent {
    pub stream_id: String,
    pub sequence: u64,
    pub envelope_id: String,
    pub payload_digest: Digest,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkflowOwnerState {
    pub objects: Vec<(Digest, Vec<u8>)>,
    pub reference: WorkflowReferenceUpdate,
    pub controls: Vec<WorkflowControlWrite>,
    pub audits: Vec<WorkflowAuditWrite>,
}

impl WorkflowOwnerState {
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
            && self.reference == WorkflowReferenceUpdate::Keep
            && self.controls.is_empty()
            && self.audits.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorkflowReferenceUpdate {
    #[default]
    Keep,
    Set(Option<Digest>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkflowControlWrite {
    Put {
        key: Vec<u8>,
        payload: Vec<u8>,
    },
    Delete {
        key: Vec<u8>,
    },
    AppendRetained {
        key: Vec<u8>,
        expected_next_sequence: u64,
        records: Vec<Vec<u8>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowAuditWrite {
    pub principal: Option<WorkspaceId>,
    pub action: String,
    pub target: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FacetWrite {
    pub facet: FacetKind,
    pub target: OverlayKey,
    pub op: FacetWriteOp,
    pub secondary_indexes: Vec<SecondaryIndexWrite>,
    pub expected: Option<CompareToken>,
    pub durability: Option<OverlayDurabilityPolicy>,
    pub audit: Option<AuditIntent>,
    pub side_effects: FacetSideEffects,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FacetWriteOp {
    Put { payload: Vec<u8> },
    Delete,
}

impl FacetWriteOp {
    pub const fn entry_kind(&self) -> OverlayEntryKind {
        match self {
            Self::Put { .. } => OverlayEntryKind::Value,
            Self::Delete => OverlayEntryKind::Tombstone,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecondaryIndexWrite {
    pub index: OverlayKey,
    pub op: SecondaryIndexWriteOp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecondaryIndexWriteOp {
    Put { payload: Vec<u8> },
    Delete,
}

impl SecondaryIndexWriteOp {
    pub const fn entry_kind(&self) -> OverlayEntryKind {
        match self {
            Self::Put { .. } => OverlayEntryKind::Value,
            Self::Delete => OverlayEntryKind::Tombstone,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareToken(pub OverlayOwnerToken);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditIntent {
    pub operation: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FacetSideEffects {
    pub intents: Vec<FacetSideEffect>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FacetSideEffect {
    OperationLog { operation_id: String },
    AuditRecord { operation: String },
    RevisionIndex { entity_id: String },
    ReferenceIndex { source_id: String },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AtomicityBoundary {
    #[default]
    Single,
    Separate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitReceipt {
    pub generation: OverlayGeneration,
    pub root_after: Digest,
    pub writes: Vec<WriteOutcome>,
    pub operation_identities: Vec<String>,
    pub revision_identities: Vec<RevisionReceipt>,
    pub audit_sequences: Vec<u64>,
    pub retained_sequences: Vec<RetainedSequenceReceipt>,
    pub delivery_receipts: Vec<DeliveryReceipt>,
    pub post_commit_delta: Option<PostCommitDeltaReceipt>,
    pub replayed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteOutcome {
    pub facet: FacetKind,
    pub target: OverlayKey,
    pub owner_token: OverlayOwnerToken,
    pub change: OverlayEntryKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevisionReceipt {
    pub entity_id: String,
    pub revision_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedSequenceReceipt {
    pub key: Vec<u8>,
    pub first_sequence: u64,
    pub last_sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliveryReceipt {
    pub stream_id: String,
    pub sequence: u64,
    pub envelope_id: String,
    pub payload_digest: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostCommitDeltaReceipt {
    pub workspace: WorkspaceId,
    pub changed_paths: Vec<String>,
    pub changed_content_count: usize,
}

impl From<&crate::EngineStateDelta> for PostCommitDeltaReceipt {
    fn from(delta: &crate::EngineStateDelta) -> Self {
        Self {
            workspace: delta.workspace(),
            changed_paths: delta.changed_paths(),
            changed_content_count: delta.changed_content_count(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkflowTransactionErrorKind {
    RetryableStaleGeneration,
    StaleOwnerToken,
    DuplicateIdempotencyKey,
    MalformedCompareToken,
    PermissionDenied,
    UnhonoredDurabilityPolicy,
    UnsupportedFacet,
    UnsupportedOperation,
}

impl WorkflowTransactionErrorKind {
    pub const fn code(self) -> Code {
        match self {
            Self::RetryableStaleGeneration
            | Self::StaleOwnerToken
            | Self::DuplicateIdempotencyKey => Code::Conflict,
            Self::MalformedCompareToken | Self::UnhonoredDurabilityPolicy => Code::InvalidArgument,
            Self::PermissionDenied => Code::PermissionDenied,
            Self::UnsupportedFacet | Self::UnsupportedOperation => Code::Unsupported,
        }
    }

    pub fn into_error(self, message: impl Into<String>) -> LoomError {
        LoomError::new(self.code(), message)
    }
}

pub trait WorkflowCommitter {
    fn commit(&self, txn: WorkflowTransaction) -> Result<CommitReceipt>;
}

pub trait FacetWriteBuilder {
    fn facet(&self) -> FacetKind;
    fn prepare(&self, snapshot: &OverlayReadSnapshot) -> Result<Vec<FacetWrite>>;
}

pub fn strictest_durability(
    durabilities: impl IntoIterator<Item = OverlayDurabilityPolicy>,
) -> OverlayDurabilityPolicy {
    let mut resolved = OverlayDurabilityPolicy::Ephemeral;
    for durability in durabilities {
        resolved = match (resolved, durability) {
            (OverlayDurabilityPolicy::Strict, _) | (_, OverlayDurabilityPolicy::Strict) => {
                OverlayDurabilityPolicy::Strict
            }
            (OverlayDurabilityPolicy::Normal, _) | (_, OverlayDurabilityPolicy::Normal) => {
                OverlayDurabilityPolicy::Normal
            }
            (OverlayDurabilityPolicy::Relaxed, _) | (_, OverlayDurabilityPolicy::Relaxed) => {
                OverlayDurabilityPolicy::Relaxed
            }
            _ => OverlayDurabilityPolicy::Ephemeral,
        };
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> OverlayKey {
        OverlayKey::from_segments([
            b"workspace",
            &[1; 16],
            b"tickets",
            b"matrix",
            b"ticket",
            name.as_bytes(),
        ])
        .unwrap()
    }

    fn transaction(writes: Vec<FacetWrite>) -> WorkflowTransaction {
        WorkflowTransaction {
            workspace: WorkspaceId::from_bytes([1; 16]),
            actor: PrincipalId::from_bytes([2; 16]),
            expected_generation: Some(OverlayGeneration::new(7)),
            writes,
            prepared_operations: Vec::new(),
            revision_metadata: Vec::new(),
            delivery_intents: Vec::new(),
            durability: OverlayDurabilityPolicy::Normal,
            boundary: AtomicityBoundary::Single,
            idempotency: Some(IdempotencyKey::opaque(b"retry")),
            owner_state: WorkflowOwnerState::default(),
            post_commit_delta: None,
        }
    }

    fn write(name: &str, durability: Option<OverlayDurabilityPolicy>) -> FacetWrite {
        FacetWrite {
            facet: FacetKind::Document,
            target: key(name),
            op: FacetWriteOp::Put {
                payload: name.as_bytes().to_vec(),
            },
            secondary_indexes: vec![SecondaryIndexWrite {
                index: key(&format!("{name}-by-status")),
                op: SecondaryIndexWriteOp::Put {
                    payload: b"open".to_vec(),
                },
            }],
            expected: Some(CompareToken(OverlayOwnerToken::from_bytes([3; 32]))),
            durability,
            audit: Some(AuditIntent {
                operation: "ticket.update".to_string(),
            }),
            side_effects: FacetSideEffects {
                intents: vec![
                    FacetSideEffect::OperationLog {
                        operation_id: "op-1".to_string(),
                    },
                    FacetSideEffect::RevisionIndex {
                        entity_id: name.to_string(),
                    },
                ],
            },
        }
    }

    #[test]
    fn transaction_type_carries_compare_idempotency_generation_and_side_effects() {
        let txn = transaction(vec![write("MX-453", Some(OverlayDurabilityPolicy::Strict))]);

        assert_eq!(txn.expected_generation, Some(OverlayGeneration::new(7)));
        assert!(txn.idempotency.is_some());
        assert!(txn.writes[0].expected.is_some());
        assert_eq!(txn.writes[0].op.entry_kind(), OverlayEntryKind::Value);
        assert_eq!(
            txn.writes[0].secondary_indexes[0].op.entry_kind(),
            OverlayEntryKind::Value
        );
        assert_eq!(txn.writes[0].side_effects.intents.len(), 2);
        assert_eq!(txn.effective_durability(), OverlayDurabilityPolicy::Strict);
        txn.validate().unwrap();
    }

    #[test]
    fn transaction_type_carries_prepared_operation_revision_delivery_and_delta_fields() {
        let mut txn = transaction(vec![write("MX-454", Some(OverlayDurabilityPolicy::Normal))]);
        txn.prepared_operations.push(PreparedOperation {
            operation_id: "op-454".to_string(),
            payload: b"operation".to_vec(),
        });
        txn.revision_metadata.push(PreparedRevisionMetadata {
            entity_id: "ticket:MX-454".to_string(),
            revision_id: "rev-1".to_string(),
            payload: b"revision".to_vec(),
        });
        txn.delivery_intents.push(PreparedDeliveryIntent {
            stream_id: "tickets".to_string(),
            sequence: 9,
            envelope_id: "env-9".to_string(),
            payload_digest: Digest::blake3(b"delivery"),
        });
        txn.post_commit_delta = Some(crate::EngineStateDelta::empty(txn.workspace));

        txn.validate().unwrap();
        assert_eq!(txn.prepared_operations[0].operation_id, "op-454");
        assert_eq!(txn.revision_metadata[0].revision_id, "rev-1");
        assert_eq!(txn.delivery_intents[0].sequence, 9);
        assert_eq!(
            txn.post_commit_delta.as_ref().unwrap().workspace(),
            txn.workspace
        );
    }

    #[test]
    fn transaction_rejects_aggregate_budget_overflow() {
        let mut txn = transaction(vec![write("MX-455", Some(OverlayDurabilityPolicy::Normal))]);
        txn.prepared_operations = vec![
            PreparedOperation {
                operation_id: "large-a".to_string(),
                payload: vec![b'a'; WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES],
            },
            PreparedOperation {
                operation_id: "large-b".to_string(),
                payload: vec![b'b'; WORKFLOW_RECEIPT_MAX_PAYLOAD_BYTES],
            },
        ];

        let error = txn.validate().unwrap_err();

        assert_eq!(error.code, Code::InvalidArgument);
    }

    #[test]
    fn aggregate_budget_checked_accounting_rejects_overflow_and_one_byte_over() {
        let mut exact = WorkflowAggregateByteBudget::new();
        assert!(exact.reserve(WORKFLOW_TRANSACTION_MAX_AGGREGATE_ENCODED_BYTES));
        assert_eq!(
            exact.used(),
            WORKFLOW_TRANSACTION_MAX_AGGREGATE_ENCODED_BYTES
        );
        assert!(!exact.reserve(1));

        let mut overflow = WorkflowAggregateByteBudget::new();
        assert!(!overflow.reserve(usize::MAX));
    }

    #[test]
    fn commit_receipt_carries_log_revision_audit_retained_delivery_and_delta_outputs() {
        let receipt = CommitReceipt {
            generation: OverlayGeneration::new(8),
            root_after: Digest::blake3(b"root"),
            writes: Vec::new(),
            operation_identities: vec!["op-8".to_string()],
            revision_identities: vec![RevisionReceipt {
                entity_id: "entity".to_string(),
                revision_id: "revision".to_string(),
            }],
            audit_sequences: vec![3],
            retained_sequences: vec![RetainedSequenceReceipt {
                key: b"history".to_vec(),
                first_sequence: 4,
                last_sequence: 6,
            }],
            delivery_receipts: vec![DeliveryReceipt {
                stream_id: "stream".to_string(),
                sequence: 7,
                envelope_id: "envelope".to_string(),
                payload_digest: Digest::blake3(b"payload"),
            }],
            post_commit_delta: Some(PostCommitDeltaReceipt {
                workspace: WorkspaceId::from_bytes([9; 16]),
                changed_paths: vec!["README.md".to_string()],
                changed_content_count: 1,
            }),
            replayed: false,
        };

        assert_eq!(receipt.operation_identities, ["op-8"]);
        assert_eq!(receipt.revision_identities[0].entity_id, "entity");
        assert_eq!(receipt.audit_sequences, [3]);
        assert_eq!(receipt.retained_sequences[0].last_sequence, 6);
        assert_eq!(receipt.delivery_receipts[0].envelope_id, "envelope");
        assert_eq!(
            receipt.post_commit_delta.as_ref().unwrap().changed_paths,
            ["README.md"]
        );
    }

    #[test]
    fn single_boundary_rejects_ephemeral_write_mixed_into_stronger_commit() {
        let err = transaction(vec![
            write("durable", Some(OverlayDurabilityPolicy::Normal)),
            write("ephemeral", Some(OverlayDurabilityPolicy::Ephemeral)),
        ])
        .validate()
        .unwrap_err();

        assert_eq!(err.code, Code::InvalidArgument);
    }

    #[test]
    fn separate_boundary_is_rejected_until_implemented() {
        let mut txn = transaction(vec![
            write("durable", Some(OverlayDurabilityPolicy::Normal)),
            write("ephemeral", Some(OverlayDurabilityPolicy::Ephemeral)),
        ]);
        txn.boundary = AtomicityBoundary::Separate;
        let err = txn.validate().unwrap_err();

        assert_eq!(err.code, Code::Unsupported);
    }

    #[test]
    fn idempotent_ephemeral_transaction_is_rejected_until_replay_can_be_honored() {
        let mut txn = transaction(vec![write(
            "ephemeral",
            Some(OverlayDurabilityPolicy::Ephemeral),
        )]);
        txn.durability = OverlayDurabilityPolicy::Ephemeral;
        let err = txn.validate().unwrap_err();

        assert_eq!(err.code, Code::InvalidArgument);
    }

    #[test]
    fn transaction_error_mapping_matches_stable_codes() {
        assert_eq!(
            WorkflowTransactionErrorKind::DuplicateIdempotencyKey.code(),
            Code::Conflict
        );
        assert_eq!(
            WorkflowTransactionErrorKind::MalformedCompareToken.code(),
            Code::InvalidArgument
        );
        assert_eq!(
            WorkflowTransactionErrorKind::PermissionDenied.code(),
            Code::PermissionDenied
        );
        assert_eq!(
            WorkflowTransactionErrorKind::UnsupportedFacet.code(),
            Code::Unsupported
        );
    }
}
