//! Shared stable model and error contracts for reusable Loom components.

pub mod conditional;
pub mod digest;
pub mod error;
pub mod fence;
pub mod inference;
pub mod order_key;
pub mod receipt;
pub mod tabular;
pub mod vcs;
pub mod workspace;

pub use conditional::{
    CompareCondition, CompareDisposition, CompareOutcome, ConflictReason, ContentTag, EntityTag,
    EntityTagDerivation, EntityTagSource, IdempotencyKey, MutationConflict, MutationKind,
    MutationMode, MutationRequest,
};
pub use digest::{Algo, ContentHasher, DIGEST_LEN, Digest};
pub use error::{Code, ErrorDetail, LoomError, Result};
pub use fence::Fence;
pub use inference::{
    DownloadJob, DownloadState, HardwareReport, InferenceInstanceDescriptor,
    InferenceInstanceSettings, InferenceModelKind, ModelFitReason, ModelFitReport, ModelRef,
    RevisionRef, RuntimeKind,
};
pub use receipt::{MutationChange, MutationEnvelope, MutationReceipt};
pub use tabular::{
    CmpOp, ColumnType, Row, Value, cell_from, cell_value, encode_cell, encode_cells,
    encode_key_value, encode_pk_values, key_bytes,
};
pub use vcs::ChangeKind;
pub use workspace::{AclDomain, FacetKind, WorkspaceId};
