use crate::{
    Code, Digest, FacetKind, LoomError, OverlayDurabilityPolicy, OverlayEntryKind,
    OverlayGeneration, OverlayKey, OverlayOwnerToken, OverlayReadSnapshot, PrincipalId, Result,
    WorkspaceId,
};
use loom_types::IdempotencyKey;

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
                durability: self.durability,
                boundary: AtomicityBoundary::Single,
                idempotency: None,
                owner_state: self.owner_state,
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
    pub durability: OverlayDurabilityPolicy,
    pub boundary: AtomicityBoundary,
    pub idempotency: Option<IdempotencyKey>,
    pub owner_state: WorkflowOwnerState,
}

impl WorkflowTransaction {
    pub fn effective_durability(&self) -> OverlayDurabilityPolicy {
        strictest_durability(
            std::iter::once(self.durability)
                .chain(self.writes.iter().filter_map(|write| write.durability)),
        )
    }

    pub fn validate(&self) -> Result<()> {
        if self.writes.is_empty() && self.owner_state.is_empty() {
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
        for write in &self.owner_state.controls {
            let key = match write {
                WorkflowControlWrite::Put { key, .. }
                | WorkflowControlWrite::Delete { key }
                | WorkflowControlWrite::AppendRetained { key, .. } => key,
            };
            if key.is_empty() {
                return Err(LoomError::invalid(
                    "workflow control-write key must not be empty",
                ));
            }
            if let WorkflowControlWrite::AppendRetained {
                expected_next_sequence,
                records,
                ..
            } = write
            {
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
            }
        }
        if self
            .owner_state
            .audits
            .iter()
            .any(|write| write.action.trim().is_empty())
        {
            return Err(LoomError::invalid(
                "workflow audit-write action must not be blank",
            ));
        }
        Ok(())
    }
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
    pub replayed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteOutcome {
    pub facet: FacetKind,
    pub target: OverlayKey,
    pub owner_token: OverlayOwnerToken,
    pub change: OverlayEntryKind,
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
            durability: OverlayDurabilityPolicy::Normal,
            boundary: AtomicityBoundary::Single,
            idempotency: Some(IdempotencyKey::opaque(b"retry")),
            owner_state: WorkflowOwnerState::default(),
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
