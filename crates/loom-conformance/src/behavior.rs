//! Behavioral conformance scenarios: each facet's contract as Given/When/Then steps, anchored to the
//! established tool it stands in for (POSIX fs, git, Redis, ...). A backend runs the suite for each
//! capability it advertises.
//!
//! Scenario tables are data until an executable runner exists for the public surface. The `cas` facet
//! maps onto the object store and is executed against any [`ObjectStore`] by [`run_cas_behavior`].

use loom_codec::Value as CborValue;
use loom_core::inference::Message;
use loom_core::tabular::{CmpOp, ColumnType, Table, Value};
use loom_core::workspace::DEFAULT_BRANCH;
use loom_core::workspace::facet_path;
use loom_core::{
    AclEffect, AclGrant, AclResource, AclResourceScope, AclRight, AclScope, AclScopeKind, AclStore,
    AclSubject, Algo, BackPressure, BookMeta, Bundle, CalendarEntry, ChangeKind, Code, Collection,
    CollectionMeta, Component, ConflictResolution, ContactEntry, Digest, DocumentFieldPath,
    DocumentIndexDef, EmbeddingModel, EmbeddingProvider, Embeddings, EphemeralKvMap,
    EphemeralPutOptions, EvictionPolicy, FacetKind, Fence, Graph, GraphCrs, GraphGeometry,
    GraphIndexEntity, GraphIndexStatus, GraphMutation, GraphMutationIdentity, GraphMutationPlan,
    GraphPath, GraphQuery, GraphQueryEdge, GraphQueryNode, GraphQueryValue, GraphValue,
    IDENTITY_SIGNATURE_SUITE_ED25519, IdentityPublicKeySpec, IdentityStore, Inference,
    InferenceProvider, InferenceRequest, InferenceResponse, KvCondition, KvMap, KvMapConfig,
    LEDGER_CHECKPOINT_SIGNATURE_PURPOSE, LedgerAppendMode, LedgerRangeState, LedgerRetentionRange,
    LockCoordinator, LockMode, LockOwner, LockToken, Loom, MailboxMeta, MergeOutcome, Object,
    ObjectStore, OpenMode, PrincipalKind, Props, ProtectedRefPolicy, ROLE_ADMIN_ID, ROLE_READER_ID,
    ReplayOutcome, Result, Schema, StructuredPoint, TimeSeriesAggregation, TimeSeriesPolicy,
    TimeSeriesRollup, TimeSeriesValue, WorkspaceId, WsSelector, bundle_export, bundle_import,
    calendar, cas_delete, cas_get, cas_has, cas_list, cas_put, clone_workspace, columnar, contacts,
    content_address, dataframe, delivery_ack, delivery_ack_position, delivery_change_set,
    delivery_produce, delivery_replay, delivery_set_retained_low_water_mark, doc_create_index,
    doc_delete, doc_find, doc_index_statuses, doc_query, document_delete_with_request,
    document_get_binary, document_get_text, document_list_binary, document_put_binary,
    document_put_binary_with_entity_tag, document_put_binary_with_request, document_put_text,
    document_query_from_json, document_query_result_json, ephemeral_kv_get_read_through,
    ephemeral_kv_put_write_through, graph, kv_delete, kv_delete_conditioned, kv_exact_token,
    kv_get, kv_list, kv_put, kv_put_conditioned, kv_range, ledger_append, ledger_append_with_mode,
    ledger_attach_checkpoint_signature, ledger_checkpoint_payload_bytes, ledger_consistency_proof,
    ledger_get, ledger_head, ledger_inclusion_proof, ledger_len, ledger_proof_tree, ledger_range,
    ledger_set_retention_ranges, ledger_verify, ledger_verify_checkpoint_signatures,
    ledger_verify_consistency_proof, ledger_verify_inclusion_proof, mail,
    principal_signature_payload, push_branch, replace_kv_map, search, ts_get, ts_latest,
    ts_materialize_rollup, ts_policy, ts_prune_before, ts_put, ts_put_point, ts_range,
    ts_range_points, ts_range_rollup_points, ts_set_policy, vector,
};
use loom_delivery::DeliveryProduceRequest;
use loom_watch::{
    DomainChange, UnsupportedDomainDetail, WatchCursor, WatchDomainDetail, WatchPathChange,
    WatchSelector, watch_domain_support,
};
use std::collections::{BTreeMap, BTreeSet};

/// One behavioral scenario: a Given / When / Then case. Declarative so it travels across languages
/// and backends unchanged.
#[derive(Debug, Clone, Copy)]
pub struct Scenario {
    /// Short identifier (for test output).
    pub name: &'static str,
    /// Precondition.
    pub given: &'static str,
    /// The operation under test.
    pub when: &'static str,
    /// The asserted outcome.
    pub then: &'static str,
}

mod scenarios;
pub use scenarios::*;

// ---- behavioral conformance runners ----
mod admin;
mod conditional;
mod delivery;
mod derived;
mod document_blob;
mod engine;
mod exec;
mod fsdir;
mod metrics;
mod pim;
mod program;
mod sql;
mod tickets;
mod triggers;
mod vcs;
mod watch;
pub use admin::*;
pub use conditional::*;
pub use delivery::*;
pub use derived::*;
pub use document_blob::*;
pub use engine::*;
pub use exec::*;
pub use fsdir::*;
pub use metrics::*;
pub use pim::*;
pub use program::*;
pub use sql::*;
pub use tickets::*;
pub use triggers::*;
pub use vcs::*;
pub use watch::*;

#[cfg(test)]
mod tests;
