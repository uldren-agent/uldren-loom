use crate::digest::Digest;
use crate::fence::Fence;
use loom_codec::Value as CborValue;

const ENTITY_TAG_TYPE_CODE: u16 = 11;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EntityTag {
    bytes: Vec<u8>,
}

impl EntityTag {
    pub fn opaque(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
        }
    }

    pub fn from_digest(digest: Digest) -> Self {
        let mut bytes = Vec::with_capacity(1 + crate::DIGEST_LEN);
        bytes.push(digest.algo().code());
        bytes.extend_from_slice(digest.bytes());
        Self { bytes }
    }

    pub fn from_generation(scope: &[u8], generation: u64) -> Self {
        Self::from_canonical_parts(&[
            CborValue::Text("generation".to_string()),
            CborValue::Bytes(scope.to_vec()),
            CborValue::Uint(generation),
        ])
    }

    pub fn from_operation_anchor(fence: Fence) -> Self {
        let (low, high) = fence.to_limbs();
        Self::from_canonical_parts(&[
            CborValue::Text("operation_anchor".to_string()),
            CborValue::Uint(high),
            CborValue::Uint(low),
        ])
    }

    pub fn from_canonical_parts(parts: &[CborValue]) -> Self {
        let bytes = loom_codec::encode_object(ENTITY_TAG_TYPE_CODE, parts)
            .expect("entity tag parts are canonical values");
        Self { bytes }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IdempotencyKey {
    bytes: Vec<u8>,
}

impl IdempotencyKey {
    pub fn opaque(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContentTag {
    digest: Digest,
}

impl ContentTag {
    pub const fn new(digest: Digest) -> Self {
        Self { digest }
    }

    pub const fn digest(&self) -> Digest {
        self.digest
    }

    pub fn to_entity_tag(&self) -> EntityTag {
        EntityTag::from_canonical_parts(&[
            CborValue::Text("content".to_string()),
            CborValue::Uint(u64::from(self.digest.algo().code())),
            CborValue::Bytes(self.digest.bytes().to_vec()),
        ])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompareCondition {
    Any,
    Absent,
    Exact(EntityTag),
    Generation(u64),
    OperationAnchor(Fence),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MutationMode {
    CreateIfAbsent,
    ReplaceIfPresent,
    ReplaceIfMatch(EntityTag),
    DeleteIfPresent,
    DeleteIfMatch(EntityTag),
    UpsertBlind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationKind {
    Create,
    Replace,
    Delete,
    Upsert,
}

impl MutationMode {
    pub const fn kind(&self) -> MutationKind {
        match self {
            Self::CreateIfAbsent => MutationKind::Create,
            Self::ReplaceIfPresent | Self::ReplaceIfMatch(_) => MutationKind::Replace,
            Self::DeleteIfPresent | Self::DeleteIfMatch(_) => MutationKind::Delete,
            Self::UpsertBlind => MutationKind::Upsert,
        }
    }

    pub fn compare_condition(&self) -> CompareCondition {
        match self {
            Self::CreateIfAbsent => CompareCondition::Absent,
            Self::ReplaceIfPresent | Self::DeleteIfPresent | Self::UpsertBlind => {
                CompareCondition::Any
            }
            Self::ReplaceIfMatch(tag) | Self::DeleteIfMatch(tag) => {
                CompareCondition::Exact(tag.clone())
            }
        }
    }

    pub const fn requires_existing_record(&self) -> bool {
        matches!(
            self,
            Self::ReplaceIfPresent
                | Self::ReplaceIfMatch(_)
                | Self::DeleteIfPresent
                | Self::DeleteIfMatch(_)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationRequest {
    pub mode: MutationMode,
    pub idempotency_key: Option<IdempotencyKey>,
}

impl MutationRequest {
    pub const fn new(mode: MutationMode) -> Self {
        Self {
            mode,
            idempotency_key: None,
        }
    }

    pub fn with_idempotency_key(mode: MutationMode, idempotency_key: IdempotencyKey) -> Self {
        Self {
            mode,
            idempotency_key: Some(idempotency_key),
        }
    }

    pub fn compare_condition(&self) -> CompareCondition {
        self.mode.compare_condition()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareDisposition {
    Applied,
    AbsentMismatch,
    ExactMismatch,
    GenerationMismatch,
    OperationAnchorStale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictReason {
    ExpectedTagMismatch,
    MissingRecord,
    RecordAlreadyExists,
    StaleRevision,
    ConditionNotSatisfied,
    StaleOperationAnchor,
}

impl ConflictReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExpectedTagMismatch => "expected_tag_mismatch",
            Self::MissingRecord => "missing_record",
            Self::RecordAlreadyExists => "record_already_exists",
            Self::StaleRevision => "stale_revision",
            Self::ConditionNotSatisfied => "condition_not_satisfied",
            Self::StaleOperationAnchor => "stale_operation_anchor",
        }
    }
}

impl CompareDisposition {
    pub const fn conflict_reason(self) -> Option<ConflictReason> {
        match self {
            Self::Applied => None,
            Self::AbsentMismatch => Some(ConflictReason::RecordAlreadyExists),
            Self::ExactMismatch => Some(ConflictReason::ExpectedTagMismatch),
            Self::GenerationMismatch => Some(ConflictReason::StaleRevision),
            Self::OperationAnchorStale => Some(ConflictReason::StaleOperationAnchor),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationConflict {
    pub reason: ConflictReason,
    pub disposition: CompareDisposition,
}

impl MutationConflict {
    pub const fn new(reason: ConflictReason, disposition: CompareDisposition) -> Self {
        Self {
            reason,
            disposition,
        }
    }

    pub const fn from_disposition(disposition: CompareDisposition) -> Option<Self> {
        match disposition.conflict_reason() {
            Some(reason) => Some(Self {
                reason,
                disposition,
            }),
            None => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareOutcome {
    pub disposition: CompareDisposition,
    pub entity_tag: Option<EntityTag>,
    pub conflict: Option<MutationConflict>,
}

impl CompareOutcome {
    pub fn applied(entity_tag: Option<EntityTag>) -> Self {
        Self {
            disposition: CompareDisposition::Applied,
            entity_tag,
            conflict: None,
        }
    }

    pub const fn rejected(disposition: CompareDisposition) -> Self {
        Self {
            disposition,
            entity_tag: None,
            conflict: MutationConflict::from_disposition(disposition),
        }
    }

    pub const fn rejected_with_reason(
        disposition: CompareDisposition,
        reason: ConflictReason,
    ) -> Self {
        Self {
            disposition,
            entity_tag: None,
            conflict: Some(MutationConflict::new(reason, disposition)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityTagDerivation {
    pub source: EntityTagSource,
    pub atomic_scope: Vec<u8>,
    pub representation: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntityTagSource {
    CanonicalBytes(Digest),
    MutableStateVersion(u64),
    OperationAnchor(Fence),
    FacadeRepresentation,
}

impl EntityTagDerivation {
    pub fn entity_tag(&self) -> EntityTag {
        match self.source {
            EntityTagSource::CanonicalBytes(digest) => ContentTag::new(digest).to_entity_tag(),
            EntityTagSource::MutableStateVersion(generation) => {
                EntityTag::from_generation(&self.atomic_scope, generation)
            }
            EntityTagSource::OperationAnchor(fence) => EntityTag::from_operation_anchor(fence),
            EntityTagSource::FacadeRepresentation => {
                let mut fields = vec![
                    CborValue::Text("facade".to_string()),
                    CborValue::Bytes(self.atomic_scope.clone()),
                ];
                if let Some(representation) = &self.representation {
                    fields.push(CborValue::Bytes(representation.clone()));
                }
                EntityTag::from_canonical_parts(&fields)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CompareCondition, CompareDisposition, CompareOutcome, ConflictReason, ContentTag,
        EntityTag, EntityTagDerivation, EntityTagSource, IdempotencyKey, MutationMode,
        MutationRequest,
    };
    use crate::{Digest, Fence};

    #[test]
    fn content_tag_is_not_the_compare_token() {
        let digest = Digest::blake3(b"record bytes");
        let content = ContentTag::new(digest);
        let entity = content.to_entity_tag();

        assert_eq!(content.digest(), digest);
        assert_ne!(entity.as_bytes(), digest.bytes());
    }

    #[test]
    fn entity_tag_derivation_is_canonical_for_generation_scope() {
        let first = EntityTagDerivation {
            source: EntityTagSource::MutableStateVersion(7),
            atomic_scope: b"kv/a".to_vec(),
            representation: None,
        }
        .entity_tag();
        let second = EntityTag::from_generation(b"kv/a", 7);
        let third = EntityTag::from_generation(b"kv/a", 8);

        assert_eq!(first, second);
        assert_ne!(second, third);
    }

    #[test]
    fn operation_anchor_condition_uses_fence_identity() {
        let fence = Fence::new(1, 2, 3);
        let condition = CompareCondition::OperationAnchor(fence);
        let tag = EntityTag::from_operation_anchor(fence);

        assert!(matches!(
            condition,
            CompareCondition::OperationAnchor(actual) if actual == fence
        ));
        assert_eq!(tag, EntityTag::from_operation_anchor(Fence::new(1, 2, 3)));
    }

    #[test]
    fn rejected_compare_outcome_discloses_no_current_tag() {
        let outcome = CompareOutcome::rejected(CompareDisposition::ExactMismatch);

        assert_eq!(outcome.disposition, CompareDisposition::ExactMismatch);
        assert!(outcome.entity_tag.is_none());
        assert_eq!(
            outcome.conflict.as_ref().map(|conflict| conflict.reason),
            Some(ConflictReason::ExpectedTagMismatch)
        );
    }

    #[test]
    fn standard_mutation_modes_map_to_compare_conditions() {
        let tag = EntityTag::opaque(b"owner-token");

        assert_eq!(
            MutationMode::CreateIfAbsent.compare_condition(),
            CompareCondition::Absent
        );
        assert_eq!(
            MutationMode::UpsertBlind.compare_condition(),
            CompareCondition::Any
        );
        assert_eq!(
            MutationMode::ReplaceIfMatch(tag.clone()).compare_condition(),
            CompareCondition::Exact(tag.clone())
        );
        assert!(MutationMode::DeleteIfMatch(tag).requires_existing_record());
    }

    #[test]
    fn conflict_reasons_are_stable_snake_case() {
        assert_eq!(
            ConflictReason::ExpectedTagMismatch.as_str(),
            "expected_tag_mismatch"
        );
        assert_eq!(ConflictReason::MissingRecord.as_str(), "missing_record");
        assert_eq!(
            ConflictReason::RecordAlreadyExists.as_str(),
            "record_already_exists"
        );
        assert_eq!(ConflictReason::StaleRevision.as_str(), "stale_revision");
        assert_eq!(
            ConflictReason::ConditionNotSatisfied.as_str(),
            "condition_not_satisfied"
        );
    }

    #[test]
    fn idempotency_key_does_not_change_entity_tag_condition() {
        let tag = EntityTag::opaque(b"entity-token");
        let key = IdempotencyKey::opaque(b"retry-key");
        let request =
            MutationRequest::with_idempotency_key(MutationMode::ReplaceIfMatch(tag.clone()), key);
        let compare_tag = match request.compare_condition() {
            CompareCondition::Exact(tag) => tag,
            _ => unreachable!(),
        };

        assert_eq!(request.compare_condition(), CompareCondition::Exact(tag));
        assert_ne!(
            request.idempotency_key.as_ref().unwrap().as_bytes(),
            compare_tag.as_bytes()
        );
    }
}
