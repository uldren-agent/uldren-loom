//! Shared conformance vectors plus a generic runner.
//!
//! The [`BLOB_VECTORS`] are canonical test vectors: fixed inputs mapped to the default `blake3:`
//! address and the FIPS `sha256:` address of the corresponding [`Object::Blob`]. Every implementation,
//! in every language, must reproduce these exact digests - this is what pins the data model across the
//! polyglot bindings.
//!
//! The [`behavior`] module carries the per-facet **behavioral** suites: each facet's contract as
//! Given/When/Then scenarios anchored to the tool it stands in for, plus a runnable `cas` suite over
//! any [`ObjectStore`]. Together with the canonical digest vectors here, these certify a backend.

pub mod behavior;
#[path = "behavior/logs.rs"]
pub mod logs_behavior;
pub mod studio_imports;
#[path = "behavior/traces.rs"]
pub mod traces_behavior;

use loom_compute::{Capability, Grant, GrantSet, Manifest, Mode, Scope};
use loom_core::{
    Algo, Code, Commit, DOCUMENT_CHUNK_THRESHOLD, Digest, DocumentBodyRef,
    DocumentCollectionManifest, DocumentId, DocumentIndexCatalog, DocumentIndexDeclaration,
    DocumentPolicyConfig, DocumentRecord, DocumentTombstoneRecord, EntryKind, FacetKind, Fence,
    Ledger, Loom, LoomError, MemoryStore, Object, ObjectStore, ObjectType, Result, RuntimeProfile,
    Tag, TreeEntry, WorkspaceId, content_address, doc_delete, document_get_binary,
    document_put_binary, document_set_retention_policy, runtime_profile,
};
use loom_interchange::{
    ArchiveEntry, ArchiveEntryKind, ArchiveKind, ArchiveManifest, Coverage, FidelityIssue,
    FidelitySeverity, ImportBatch, ImportBatchItem, ImportCheckpoint, ImportReport,
    ImportReportInput, ProfileImportAction, ProfileImportActionKind, ProfileImportPlan,
    SourceSystem, TargetProfile,
};
use loom_substrate::{
    ActorKind, OperationEnvelope, OperationEnvelopeInput,
    annotation::{
        AnnotationAction, AnnotationAnchor, AnnotationEvent, AnnotationStore, EmojiRegistry,
    },
    body::{Block, BlockKind, Body, BodyAnchor, BodyDelta, BodyPatch, Mark, TextRun},
    changes::{OperationChangeCursor, operation_log_changes},
    drive::{
        CHUNK_MIN_SIZE, DEHYDRATED_FILE_MARKER_MAGIC, DriveChunkManifest, DriveChunkRef,
        DriveConcurrentOperation, DriveConflictIndex, DriveConflictRecord, DriveConflictResolution,
        DriveContentRef, DriveDehydratedFileMarker, DriveFileVersion, DriveFileVersionIndex,
        DriveFolderChildren, DriveFolderEntry, DriveFolderIndex, DriveMergeOutcome, DriveNodeKind,
        DriveOperationLog, DriveOperationRecord, DriveProfileSnapshot, DriveRetentionIndex,
        DriveRetentionPin, DriveRetentionPinInput, DriveRetentionPinKind, DriveShareGrant,
        DriveShareGrantInput, DriveShareIndex, DriveShareRole, DriveShareTargetKind,
        DriveUploadChunk, DriveUploadSession, DriveUploadSessionInput, DriveUploadTargetKind,
        conflict_copy_name, drive_conflict_index_key, drive_fold_key, drive_merge_outcome,
        drive_operation_log_key, drive_profile_key, drive_retention_index_key,
        drive_share_index_key, drive_upload_session_key, is_drive_dehydrated_file_marker,
    },
    facilities::{FieldDefinition, FieldType, FieldValue},
    lifecycle::{
        GateEvaluation, GateKind, LifecycleDefinition, LifecycleGate, LifecycleGateInput,
        LifecycleInstance, LifecycleOperationLog, LifecycleOperationRecord, LifecycleStage,
        LifecycleTransitionInput, LifecycleTransitionRecord, SnapshotPlan, SnapshotPolicy,
        SnapshotRecord, lifecycle_operation_cursor_scope, lifecycle_operation_log_key,
    },
    meetings::{
        AnnotationRecord, AnnotationStatus, Coverage as MeetingsCoverage, EntityMergeInput,
        EntityMergeRecord, ExtractionReviewProjection, ImportRunRecord, InputProfile,
        MeetingRecord, MeetingRecordInput, MeetingsProfileSnapshot, MeetingsProfileSnapshotParts,
        ProjectionAction, ProjectionEffectSet, ProjectionKind, ProjectionOutputSet,
        RedactionRecord, RedactionState, SourceRecord, SourceRecordInput, SpanKind, SpanRecord,
        VocabularyTermInput, VocabularyTermRecord, VocabularyTermStatus,
    },
    order_token::first_token,
    pages::{PageOperationLog, PageOperationRecord, page_operation_cursor_scope},
    predicate::Predicate,
    refs::{AliasBinding, EntityRef, ReferenceIndex, ReferenceSource, extract_ref_occurrences},
    sequencer::{LocalSequencer, OperationDraft, OperationLog, SequenceRequest, SequencerHooks},
    surfaces::{
        ElicitationRequest, ElicitationRequestInput, ElicitationResponse, ElicitationStatus,
        PromptHandoff, PromptHandoffInput, RenderFrame, StalenessPolicy, core_surface_catalog,
        meeting_memory_surface_catalog, surface_app_catalog,
    },
    versioning::{BodyRef, Checkpoint, EntityRevision, RevisionIndex},
    view::{FreshnessPolicy, ViewDefinition, ViewDefinitionInput, ViewRegistry},
    web::{
        WebListener, WebMethod, WebMountRef, WebProtocol, WebRoute, WebRouteMode, WebRouteTable,
    },
    workgraph::{
        WorkgraphFact, WorkgraphFactKind, WorkgraphOperationLog, WorkgraphOperationRecord,
        WorkgraphState, workgraph_operation_cursor_scope, workgraph_operation_kind,
        workgraph_operation_log_key,
    },
};
use loom_templates::{
    DiagnosticSeverity, HostCallKind, ProgramBinding, RenderOption, TemplateCacheInput,
    TemplateConsumer, TemplateDependencyKind, TemplateError, TemplateProcessor,
};
use loom_tickets::{TicketOperationLog, TicketOperationRecord, ticket_operation_cursor_scope};
use std::collections::{BTreeMap, BTreeSet};

/// One canonical blob vector: input bytes -> expected `algo:hex` address of `Object::Blob(input)`.
#[derive(Debug, Clone, Copy)]
pub struct BlobVector {
    /// Human-readable name (for test output).
    pub name: &'static str,
    /// The raw blob content.
    pub input: &'static [u8],
    /// The expected Loom Canonical CBOR bytes of `Object::Blob(input)`, hex. Hash-independent: the
    /// canonical bytes are identical across identity profiles, so only the `expect_digest*` differ.
    pub expect_canonical: &'static str,
    /// The expected content address under the **default** profile (BLAKE3-256) - the digest of
    /// `expect_canonical`.
    pub expect_digest: &'static str,
    /// The expected content address under the **FIPS** profile (SHA-256) of the same canonical bytes.
    /// Pinning both proves the canonical encoding is profile-independent and only
    /// the digest layer changes.
    pub expect_digest_sha256: &'static str,
}

/// The canonical blob vectors. Keep in sync across all implementations.
pub const BLOB_VECTORS: &[BlobVector] = &[
    BlobVector {
        name: "empty",
        input: b"",
        expect_canonical: "83010140",
        expect_digest: "blake3:d3b3f51620d403496c828f9fa94a44e052475f386f72d1204edb2bbfed8b18dc",
        expect_digest_sha256: "sha256:365e8f6b08045400d3bf417867536ce221f40f5f738221ed8d53674336ef8287",
    },
    BlobVector {
        name: "abc",
        input: b"abc",
        expect_canonical: "83010143616263",
        expect_digest: "blake3:7c953cb883974e24e76125db985052ecdfb77d40386cd699ecd952a314915b07",
        expect_digest_sha256: "sha256:fb035f363760c80e1975a06c3fd2bf98b03b77a27324f17a078d4ebcd1e14e5a",
    },
    BlobVector {
        name: "hello-loom",
        input: b"hello loom",
        expect_canonical: "8301014a68656c6c6f206c6f6f6d",
        expect_digest: "blake3:899c21f9af8abec9f59a7a6bfe71be8ad7d2d8a8f862cf84b052bf018a93f2cc",
        expect_digest_sha256: "sha256:4621d0118f4423854fb53fbdb1a78a62f99b5af360b77640ce680ce4205e282b",
    },
    BlobVector {
        name: "big-200",
        input: &[0u8; 200],
        expect_canonical: "83010158c80000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
        expect_digest: "blake3:ac63e9a605274ebd90d0bc6fc07b625c844efdbd23ddb47bb35adc5570680ec6",
        expect_digest_sha256: "sha256:26b328edd32c5c451c53b07fdd1d2014794a2eb2e03a8d25beed19c288457d2b",
    },
];

/// Run the canonical blob vectors against any [`ObjectStore`].
///
/// For each vector this asserts that (a) `Object::Blob(input).digest()` equals the published
/// address, and (b) the store round-trips: `put` returns that address and `get` returns the exact
/// canonical bytes.
pub fn run_blob_vectors<S: ObjectStore>(store: &mut S) -> Result<()> {
    for v in BLOB_VECTORS {
        let obj = Object::Blob(v.input.to_vec());
        assert_eq!(
            hex::encode(obj.canonical()),
            v.expect_canonical,
            "canonical-byte mismatch for vector '{}'",
            v.name
        );
        let expected = Digest::parse(v.expect_digest)?;
        assert_eq!(
            obj.digest(),
            expected,
            "data-model digest mismatch for vector '{}'",
            v.name
        );

        let stored = store.put(&obj.canonical())?;
        assert_eq!(
            stored, expected,
            "store returned wrong address for vector '{}'",
            v.name
        );
        assert_eq!(
            store.get(&stored)?.as_deref(),
            Some(obj.canonical().as_slice()),
            "store round-trip mismatch for vector '{}'",
            v.name
        );
    }
    Ok(())
}

/// Run the canonical blob vectors against an [`ObjectStore`] under a specific identity profile:
/// the canonical bytes are profile-independent, but the address is the profile's hash.
/// Certifies a backend created under `algo` (e.g. a FIPS/SHA-256 store) reproduces the pinned
/// `default/blake3` *or* `fips/sha256` addresses and round-trips. The store's own profile must be `algo`.
pub fn run_blob_vectors_profiled<S: ObjectStore>(store: &mut S, algo: Algo) -> Result<()> {
    assert_eq!(
        store.digest_algo(),
        algo,
        "run_blob_vectors_profiled: store profile must match the requested algorithm"
    );
    for v in BLOB_VECTORS {
        let obj = Object::Blob(v.input.to_vec());
        assert_eq!(
            hex::encode(obj.canonical()),
            v.expect_canonical,
            "canonical bytes are profile-independent; mismatch for vector '{}'",
            v.name
        );
        let expect_hex = match algo {
            Algo::Blake3 => v.expect_digest,
            Algo::Sha256 => v.expect_digest_sha256,
            other => panic!(
                "no pinned blob vectors for digest algorithm {}",
                other.as_str()
            ),
        };
        let expected = Digest::parse(expect_hex)?;
        assert_eq!(
            obj.digest_with(algo),
            expected,
            "{} digest mismatch for vector '{}'",
            algo.as_str(),
            v.name
        );
        let stored = store.put(&obj.canonical())?;
        assert_eq!(
            stored,
            expected,
            "store returned wrong {} address for vector '{}'",
            algo.as_str(),
            v.name
        );
        assert_eq!(
            store.get(&stored)?.as_deref(),
            Some(obj.canonical().as_slice()),
            "store round-trip mismatch for vector '{}'",
            v.name
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct CanonicalContractVector {
    pub name: &'static str,
    pub class: &'static str,
    pub input_hex: &'static str,
    pub expected_canonical_hex: &'static str,
    pub digest_profile: Algo,
    pub expected_digest: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct CanonicalContractNegativeVector {
    pub name: &'static str,
    pub class: &'static str,
    pub input_hex: &'static str,
    pub expected_failure_code: Code,
    pub expected_owner_error: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub enum CanonicalMigrationExpectation {
    Rewrite {
        expected_canonical_hex: &'static str,
    },
    Refuse {
        expected_failure_code: Code,
        expected_owner_error: &'static str,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct CanonicalMigrationVector {
    pub name: &'static str,
    pub class: &'static str,
    pub input_hex: &'static str,
    pub expectation: CanonicalMigrationExpectation,
}

pub fn run_canonical_contract_vectors<T, Decode, Encode>(
    vectors: &[CanonicalContractVector],
    mut decode: Decode,
    mut encode: Encode,
) -> Result<()>
where
    Decode: FnMut(&[u8]) -> Result<T>,
    Encode: FnMut(&T) -> Result<Vec<u8>>,
{
    for vector in vectors {
        let input = hex_bytes(vector.input_hex)?;
        let decoded = decode(&input)?;
        let canonical = encode(&decoded)?;
        assert_eq!(
            hex::encode(&canonical),
            vector.expected_canonical_hex,
            "canonical-contract output mismatch for {}",
            vector.name
        );
        assert_eq!(
            canonical, input,
            "canonical-contract input must already be canonical for {}",
            vector.name
        );
        assert_eq!(
            Digest::hash(vector.digest_profile, &canonical).to_string(),
            vector.expected_digest,
            "canonical-contract digest mismatch for {}",
            vector.name
        );
    }
    Ok(())
}

pub fn run_canonical_negative_vectors<T, Decode>(
    vectors: &[CanonicalContractNegativeVector],
    mut decode: Decode,
) -> Result<()>
where
    T: std::fmt::Debug,
    Decode: FnMut(&[u8]) -> Result<T>,
{
    for vector in vectors {
        let input = hex_bytes(vector.input_hex)?;
        let error = decode(&input).expect_err("negative vector unexpectedly decoded");
        assert_eq!(
            error.code, vector.expected_failure_code,
            "canonical-contract failure code mismatch for {}",
            vector.name
        );
        assert!(
            error.message.contains(vector.expected_owner_error),
            "canonical-contract owner error mismatch for {}: {}",
            vector.name,
            error.message
        );
    }
    Ok(())
}

pub fn run_canonical_migration_vectors<T, StrictDecode, MigrationDecode, Encode>(
    vectors: &[CanonicalMigrationVector],
    mut strict_decode: StrictDecode,
    mut migration_decode: MigrationDecode,
    mut encode: Encode,
) -> Result<()>
where
    T: std::fmt::Debug,
    StrictDecode: FnMut(&[u8]) -> Result<T>,
    MigrationDecode: FnMut(&[u8]) -> Result<T>,
    Encode: FnMut(&T) -> Result<Vec<u8>>,
{
    for vector in vectors {
        let input = hex_bytes(vector.input_hex)?;
        match vector.expectation {
            CanonicalMigrationExpectation::Rewrite {
                expected_canonical_hex,
            } => {
                let decoded = migration_decode(&input)?;
                assert_eq!(
                    hex::encode(encode(&decoded)?),
                    expected_canonical_hex,
                    "canonical migration rewrite mismatch for {}",
                    vector.name
                );
            }
            CanonicalMigrationExpectation::Refuse {
                expected_failure_code,
                expected_owner_error,
            } => {
                let error = strict_decode(&input).expect_err("migration refusal vector decoded");
                assert_eq!(
                    error.code, expected_failure_code,
                    "canonical migration refusal code mismatch for {}",
                    vector.name
                );
                assert!(
                    error.message.contains(expected_owner_error),
                    "canonical migration owner error mismatch for {}: {}",
                    vector.name,
                    error.message
                );
            }
        }
    }
    Ok(())
}

pub const CODEC_CONTRACT_VECTORS: &[CanonicalContractVector] = &[
    CanonicalContractVector {
        name: "codec-empty-map",
        class: "canonical",
        input_hex: "a0",
        expected_canonical_hex: "a0",
        digest_profile: Algo::Blake3,
        expected_digest: "blake3:1f94cbf313b3ce23257a7251ea0fc95a24556ea611e4f8f475e549971baedb02",
    },
    CanonicalContractVector {
        name: "codec-sorted-map",
        class: "canonical",
        input_hex: "a2616101616202",
        expected_canonical_hex: "a2616101616202",
        digest_profile: Algo::Sha256,
        expected_digest: "sha256:a0d3af9e86e5517f729bad0657e2c6f3b7d03899894c8d6b33759074c893b5e3",
    },
    CanonicalContractVector {
        name: "codec-f64",
        class: "canonical",
        input_hex: "fb3ff8000000000000",
        expected_canonical_hex: "fb3ff8000000000000",
        digest_profile: Algo::Blake3,
        expected_digest: "blake3:02a6136608c9b30d4e355cf9cd9911808f3997eb4cc351c7e0d08f89a74f90c5",
    },
];

pub const CODEC_NEGATIVE_VECTORS: &[CanonicalContractNegativeVector] = &[
    CanonicalContractNegativeVector {
        name: "codec-non-minimal-int",
        class: "non_canonical",
        input_hex: "1801",
        expected_failure_code: Code::CorruptObject,
        expected_owner_error: "NonMinimalInt",
    },
    CanonicalContractNegativeVector {
        name: "codec-duplicate-map-key",
        class: "duplicate",
        input_hex: "a2616101616102",
        expected_failure_code: Code::CorruptObject,
        expected_owner_error: "DuplicateMapKey",
    },
    CanonicalContractNegativeVector {
        name: "codec-indefinite-bytes",
        class: "forbidden",
        input_hex: "5fff",
        expected_failure_code: Code::CorruptObject,
        expected_owner_error: "IndefiniteLength",
    },
    CanonicalContractNegativeVector {
        name: "codec-trailing-bytes",
        class: "ambiguous",
        input_hex: "0000",
        expected_failure_code: Code::CorruptObject,
        expected_owner_error: "TrailingBytes",
    },
    CanonicalContractNegativeVector {
        name: "codec-negative-zero",
        class: "non_canonical",
        input_hex: "fb8000000000000000",
        expected_failure_code: Code::CorruptObject,
        expected_owner_error: "NonCanonicalFloat",
    },
];

pub const CODEC_MIGRATION_VECTORS: &[CanonicalMigrationVector] = &[
    CanonicalMigrationVector {
        name: "codec-unsorted-map-refusal",
        class: "migration-refusal",
        input_hex: "a2616202616101",
        expectation: CanonicalMigrationExpectation::Refuse {
            expected_failure_code: Code::CorruptObject,
            expected_owner_error: "UnsortedMapKeys",
        },
    },
    CanonicalMigrationVector {
        name: "codec-owner-rewrite",
        class: "migration-rewrite",
        input_hex: "a2616202616101",
        expectation: CanonicalMigrationExpectation::Rewrite {
            expected_canonical_hex: "a2616101616202",
        },
    },
];

pub fn run_codec_contract_vectors() -> Result<()> {
    run_canonical_contract_vectors(
        CODEC_CONTRACT_VECTORS,
        codec_decode_value,
        codec_encode_value,
    )?;
    run_canonical_negative_vectors(CODEC_NEGATIVE_VECTORS, codec_decode_value)?;
    run_canonical_migration_vectors(
        CODEC_MIGRATION_VECTORS,
        codec_decode_value,
        codec_migration_decode,
        codec_encode_value,
    )?;
    assert_binding_contract_vectors_match_fixture()?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct DocumentRootCanonicalVector {
    pub name: &'static str,
    pub class: &'static str,
    pub input_hex: &'static str,
    pub expected_canonical_hex: &'static str,
    pub digest_profile: Algo,
    pub expected_digest: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct DocumentRootNegativeVector {
    pub name: &'static str,
    pub class: &'static str,
    pub input_hex: &'static str,
    pub expected_failure_code: Code,
    pub expected_owner_error: &'static str,
}

pub const DOCUMENT_ROOT_CANONICAL_VECTORS: &[DocumentRootCanonicalVector] = &[
    DocumentRootCanonicalVector {
        name: "document-id-string",
        class: "document-id",
        input_hex: "8266737472696e67666e6f74652d31",
        expected_canonical_hex: "8266737472696e67666e6f74652d31",
        digest_profile: Algo::Sha256,
        expected_digest: "sha256:53d92b1aaf42900a39bf12600dddc770c9872bfef41d95a8e0317ee15245afa8",
    },
    DocumentRootCanonicalVector {
        name: "document-id-generated",
        class: "document-id",
        input_hex: "836967656e657261746564643031485864756c6964",
        expected_canonical_hex: "836967656e657261746564643031485864756c6964",
        digest_profile: Algo::Sha256,
        expected_digest: "sha256:4af48c9335912d8c1f41a2411eff2d08fd23fbfed1e48d4a445aeefd605676d9",
    },
    DocumentRootCanonicalVector {
        name: "document-id-external",
        class: "document-id",
        input_hex: "836865787465726e616c676d6f6e676f64627818353037663166373762636638366364373939343339303131",
        expected_canonical_hex: "836865787465726e616c676d6f6e676f64627818353037663166373762636638366364373939343339303131",
        digest_profile: Algo::Sha256,
        expected_digest: "sha256:a9996fdc67ccdd040d6b6adba6fa0a13f9745e6f13fa3e2c58a8e96ee3ece453",
    },
    DocumentRootCanonicalVector {
        name: "document-id-partitioned",
        class: "document-id",
        input_hex: "836b706172746974696f6e65646874656e616e742d6165646f632d31",
        expected_canonical_hex: "836b706172746974696f6e65646874656e616e742d6165646f632d31",
        digest_profile: Algo::Sha256,
        expected_digest: "sha256:1af998aca8cd8a751ccb7a2d2fa9905c5d3413424c1a39d09654be1c37cf5a8b",
    },
    DocumentRootCanonicalVector {
        name: "document-body-ref-direct",
        class: "body-ref",
        input_hex: "826664697265637458202cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        expected_canonical_hex: "826664697265637458202cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        digest_profile: Algo::Sha256,
        expected_digest: "sha256:404a3b79a8a159ffceb83b8fe60be6b8745eca3d07057c6f5c0d6c310e112130",
    },
    DocumentRootCanonicalVector {
        name: "document-body-ref-chunked",
        class: "body-ref",
        input_hex: "82676368756e6b6564582093256bb6d27a6a09c6eeed0b9592a91a7fcc1ab22f14e45c4b578706056286ea",
        expected_canonical_hex: "82676368756e6b6564582093256bb6d27a6a09c6eeed0b9592a91a7fcc1ab22f14e45c4b578706056286ea",
        digest_profile: Algo::Sha256,
        expected_digest: "sha256:ed8f8cd8249c9424a2bd891e258ed818aef06c61f3d966e5349c80c4ca1f3801",
    },
    DocumentRootCanonicalVector {
        name: "document-manifest-empty-tombstones-disabled",
        class: "manifest",
        input_hex: "8c78206c6f6f6d2e646f63756d656e742e636f6c6c656374696f6e2d726f6f742e763101656e6f7465738277646f63756d656e742d69642d656e76656c6f70652e7631a058204813494d137e1631bba301d5acab6e7bb7aa74ce1185d456565ef51d737677b258202e1cfa82b035c26cbbbdae632cea070514eb8b773f616aaeaf668e2f0be8f10df68266696e6c696e65a16872657461696e6564f582737265706c6163652d646f63756d656e742e7631a0827472657461696e2d746f6d6273746f6e65732e7631a0826b746578742d6163636573736d62696e6172792d616363657373a0",
        expected_canonical_hex: "8c78206c6f6f6d2e646f63756d656e742e636f6c6c656374696f6e2d726f6f742e763101656e6f7465738277646f63756d656e742d69642d656e76656c6f70652e7631a058204813494d137e1631bba301d5acab6e7bb7aa74ce1185d456565ef51d737677b258202e1cfa82b035c26cbbbdae632cea070514eb8b773f616aaeaf668e2f0be8f10df68266696e6c696e65a16872657461696e6564f582737265706c6163652d646f63756d656e742e7631a0827472657461696e2d746f6d6273746f6e65732e7631a0826b746578742d6163636573736d62696e6172792d616363657373a0",
        digest_profile: Algo::Sha256,
        expected_digest: "sha256:08a602a13c2a32855c9e5b344e6fd6988431fa0363105576800d7e48c4e999ea",
    },
    DocumentRootCanonicalVector {
        name: "document-record-live-direct",
        class: "record",
        input_hex: "8b836b706172746974696f6e65646874656e616e742d6165646f632d31826664697265637458202cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b98240566657461672d31657265762d31646c697665f6f6f6a080",
        expected_canonical_hex: "8b836b706172746974696f6e65646874656e616e742d6165646f632d31826664697265637458202cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b98240566657461672d31657265762d31646c697665f6f6f6a080",
        digest_profile: Algo::Sha256,
        expected_digest: "sha256:51b7b3025cb63e5a19b5be61581c6839d8b745a78eb26b58ebfdf7b6b24b7925",
    },
    DocumentRootCanonicalVector {
        name: "document-tombstone-retained",
        class: "tombstone",
        input_hex: "88836b706172746974696f6e65646874656e616e742d6165646f632d316c657461672d64656c6574656466657461672d31657265762d326873796e632d676170f6f6a0",
        expected_canonical_hex: "88836b706172746974696f6e65646874656e616e742d6165646f632d316c657461672d64656c6574656466657461672d31657265762d326873796e632d676170f6f6a0",
        digest_profile: Algo::Sha256,
        expected_digest: "sha256:e7495a67179fc19ad6d47b72a2f97607bf882a96aff21d211b9041e3eba48390",
    },
    DocumentRootCanonicalVector {
        name: "document-index-declaration-email",
        class: "index-declaration",
        input_hex: "8d696964782d656d61696c6862795f656d61696c8282006770726f66696c65820065656d61696c6b6a736f6e2d7363616c61726e6c6f6f6d2d7363616c61722e76317463616e6f6e6963616c2d627974652d6f7264657266756e697175656c736b69702d6d697373696e6701f6f6f6a0",
        expected_canonical_hex: "8d696964782d656d61696c6862795f656d61696c8282006770726f66696c65820065656d61696c6b6a736f6e2d7363616c61726e6c6f6f6d2d7363616c61722e76317463616e6f6e6963616c2d627974652d6f7264657266756e697175656c736b69702d6d697373696e6701f6f6f6a0",
        digest_profile: Algo::Sha256,
        expected_digest: "sha256:36d4cc33d0b3daa559e6ed6662e911016088ef63300375716dd91f95cd0c1307",
    },
    DocumentRootCanonicalVector {
        name: "document-index-catalog-empty",
        class: "index-catalog",
        input_hex: "82781e6c6f6f6d2e646f63756d656e742e696e6465782d636174616c6f672e763180",
        expected_canonical_hex: "82781e6c6f6f6d2e646f63756d656e742e696e6465782d636174616c6f672e763180",
        digest_profile: Algo::Sha256,
        expected_digest: "sha256:3b367d128626a4bb9486e741b82bdf4473bb556e73c948ab9627d258bcb468a3",
    },
    DocumentRootCanonicalVector {
        name: "document-index-catalog-populated",
        class: "index-catalog",
        input_hex: "82781e6c6f6f6d2e646f63756d656e742e696e6465782d636174616c6f672e7631818d696964782d656d61696c6862795f656d61696c8282006770726f66696c65820065656d61696c6b6a736f6e2d7363616c61726e6c6f6f6d2d7363616c61722e76317463616e6f6e6963616c2d627974652d6f7264657266756e697175656c736b69702d6d697373696e6701f6f6f6a0",
        expected_canonical_hex: "82781e6c6f6f6d2e646f63756d656e742e696e6465782d636174616c6f672e7631818d696964782d656d61696c6862795f656d61696c8282006770726f66696c65820065656d61696c6b6a736f6e2d7363616c61726e6c6f6f6d2d7363616c61722e76317463616e6f6e6963616c2d627974652d6f7264657266756e697175656c736b69702d6d697373696e6701f6f6f6a0",
        digest_profile: Algo::Sha256,
        expected_digest: "sha256:a224c04e6a94bab3b3b94298a7572f312f8855fab591d2fff591f620a32fcbd0",
    },
];

pub const DOCUMENT_ROOT_NEGATIVE_VECTORS: &[DocumentRootNegativeVector] = &[
    DocumentRootNegativeVector {
        name: "document-id-binary-kind",
        class: "document-id",
        input_hex: "8246737472696e67666e6f74652d31",
        expected_failure_code: Code::CorruptObject,
        expected_owner_error: "expected a text string",
    },
    DocumentRootNegativeVector {
        name: "document-body-ref-unknown-kind",
        class: "body-ref",
        input_hex: "8264626c6f6258202cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        expected_failure_code: Code::CorruptObject,
        expected_owner_error: "unknown document body ref kind blob",
    },
    DocumentRootNegativeVector {
        name: "document-record-missing-state",
        class: "record",
        input_hex: "858266737472696e676161826664697265637458202cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b982401646574616763726576",
        expected_failure_code: Code::CorruptObject,
        expected_owner_error: "missing field",
    },
    DocumentRootNegativeVector {
        name: "document-manifest-wrong-format",
        class: "manifest",
        input_hex: "8c6577726f6e6701656e6f7465738277646f63756d656e742d69642d656e76656c6f70652e7631a058204813494d137e1631bba301d5acab6e7bb7aa74ce1185d456565ef51d737677b258202e1cfa82b035c26cbbbdae632cea070514eb8b773f616aaeaf668e2f0be8f10df68266696e6c696e65a082737265706c6163652d646f63756d656e742e7631a0827472657461696e2d746f6d6273746f6e65732e7631a080a0",
        expected_failure_code: Code::CorruptObject,
        expected_owner_error: "unknown document collection root format",
    },
    DocumentRootNegativeVector {
        name: "document-manifest-old-collection-root",
        class: "manifest",
        input_hex: "81826161416f",
        expected_failure_code: Code::CorruptObject,
        expected_owner_error: "expected a text string",
    },
    DocumentRootNegativeVector {
        name: "document-index-declaration-bad-name",
        class: "index-declaration",
        input_hex: "8d63696478686261642f6e616d658182006770726f66696c656b6a736f6e2d7363616c61726e6c6f6f6d2d7363616c61722e76317463616e6f6e6963616c2d627974652d6f7264657266756e697175656c736b69702d6d697373696e6701f6f6f6a0",
        expected_failure_code: Code::InvalidArgument,
        expected_owner_error: "invalid document index name",
    },
    DocumentRootNegativeVector {
        name: "document-index-catalog-unknown-schema",
        class: "index-catalog",
        input_hex: "826577726f6e6780",
        expected_failure_code: Code::CorruptObject,
        expected_owner_error: "unknown document index catalog schema",
    },
];

pub fn run_document_root_vectors() -> Result<()> {
    for vector in DOCUMENT_ROOT_CANONICAL_VECTORS {
        let input = hex_bytes(vector.input_hex)?;
        let canonical = document_root_decode_encode(vector.class, &input)?;
        assert_eq!(
            hex::encode(&canonical),
            vector.expected_canonical_hex,
            "document-root canonical mismatch for {}",
            vector.name
        );
        assert_eq!(
            canonical, input,
            "document-root input must already be canonical for {}",
            vector.name
        );
        assert_eq!(
            Digest::hash(vector.digest_profile, &canonical).to_string(),
            vector.expected_digest,
            "document-root digest mismatch for {}",
            vector.name
        );
    }

    for vector in DOCUMENT_ROOT_NEGATIVE_VECTORS {
        let input = hex_bytes(vector.input_hex)?;
        let error = document_root_decode_encode(vector.class, &input)
            .expect_err("negative document-root vector unexpectedly decoded");
        assert_eq!(
            error.code, vector.expected_failure_code,
            "document-root failure code mismatch for {}",
            vector.name
        );
        assert!(
            error.message.contains(vector.expected_owner_error),
            "document-root owner error mismatch for {}: {}",
            vector.name,
            error.message
        );
    }

    Ok(())
}

pub fn run_document_chunked_body_vectors() -> Result<()> {
    let mut loom = Loom::new(MemoryStore::new());
    let ns =
        loom.registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([88; 16]))?;
    let direct = vec![b'd'; DOCUMENT_CHUNK_THRESHOLD];
    let chunked = (0..=DOCUMENT_CHUNK_THRESHOLD)
        .map(|i| (i % 251) as u8)
        .collect::<Vec<_>>();

    document_put_binary(&mut loom, ns, "docs", "direct", direct.clone(), None)?;
    document_put_binary(&mut loom, ns, "docs", "chunked", chunked.clone(), None)?;
    assert_eq!(
        document_get_binary(&loom, ns, "docs", "direct")?
            .expect("direct document")
            .bytes,
        direct
    );
    assert_eq!(
        document_get_binary(&loom, ns, "docs", "chunked")?
            .expect("chunked document")
            .bytes,
        chunked
    );

    let commit = loom.commit(ns, "conformance", "document root", 1)?;
    let document_entry = committed_tree_entry(&loom, commit, ".loom/facets/document/docs")?;
    if document_entry.kind != EntryKind::Document {
        return Err(LoomError::corrupt(
            "document collection entry kind mismatch",
        ));
    }
    let Object::Tree(root_entries) = Object::decode(&loom.object_bytes(document_entry.target)?)?
    else {
        return Err(LoomError::corrupt("document root vector is not a Tree"));
    };
    let documents_root = root_entries
        .iter()
        .find(|entry| entry.name == "documents" && entry.kind == EntryKind::ProllyMap)
        .map(|entry| entry.target)
        .ok_or_else(|| LoomError::corrupt("document root has no documents prolly map"))?;
    let mut classes = BTreeMap::new();
    for (key, value) in loom_core::prolly::entries(loom.store(), &documents_root)? {
        let document_id = DocumentId::decode(&key)?;
        let DocumentId::String(id_value) = document_id else {
            return Err(LoomError::corrupt("document id vector kind mismatch"));
        };
        let record = DocumentRecord::decode(&value)?;
        let body_kind = match record.body_ref {
            DocumentBodyRef::Direct { .. } => "direct",
            DocumentBodyRef::Chunked { root } => {
                let body_path = format!(
                    ".loom/facets/document/.bodies/{}/{}",
                    hex::encode(b"docs"),
                    root.to_hex()
                );
                let chunk_list = Object::decode(&loom.read_file_reserved(ns, &body_path)?)?;
                if !matches!(chunk_list, Object::ChunkList { .. }) {
                    return Err(LoomError::corrupt(
                        "document chunked root is not a ChunkList",
                    ));
                }
                "chunked"
            }
        };
        classes.insert(id_value, body_kind.to_string());
    }
    assert_eq!(classes.get("direct").map(String::as_str), Some("direct"));
    assert_eq!(classes.get("chunked").map(String::as_str), Some("chunked"));
    Ok(())
}

pub fn run_document_retained_tombstone_vectors() -> Result<()> {
    let mut loom = Loom::new(MemoryStore::new());
    let ns =
        loom.registry_mut()
            .create(FacetKind::Document, None, WorkspaceId::from_bytes([87; 16]))?;

    document_set_retention_policy(
        &mut loom,
        ns,
        "docs",
        DocumentPolicyConfig::new("retain-tombstones.v1")?,
    )?;
    document_put_binary(&mut loom, ns, "docs", "deleted", b"payload".to_vec(), None)?;
    if !doc_delete(&mut loom, ns, "docs", "deleted")? {
        return Err(LoomError::corrupt(
            "document tombstone vector delete did not apply",
        ));
    }
    if document_get_binary(&loom, ns, "docs", "deleted")?.is_some() {
        return Err(LoomError::corrupt(
            "document tombstone vector leaked deleted document",
        ));
    }

    let tombstone_path = format!(".loom/facets/document/.tombstones/{}", hex::encode(b"docs"));
    let tombstone_bytes = loom.read_file_reserved(ns, &tombstone_path)?;
    let decoded = loom_codec::decode(&tombstone_bytes).map_err(codec_contract_error)?;
    let loom_codec::Value::Array(fields) = decoded else {
        return Err(LoomError::corrupt(
            "document tombstone vector shape mismatch",
        ));
    };
    let [
        loom_codec::Value::Text(schema),
        loom_codec::Value::Array(entries),
    ] = fields.as_slice()
    else {
        return Err(LoomError::corrupt(
            "document tombstone vector fields mismatch",
        ));
    };
    if schema != "loom.document.tombstones.v1" {
        return Err(LoomError::corrupt(
            "document tombstone vector schema mismatch",
        ));
    }
    let [loom_codec::Value::Array(pair)] = entries.as_slice() else {
        return Err(LoomError::corrupt(
            "document tombstone vector entry mismatch",
        ));
    };
    let [
        loom_codec::Value::Array(id),
        loom_codec::Value::Array(tombstone),
    ] = pair.as_slice()
    else {
        return Err(LoomError::corrupt(
            "document tombstone vector pair mismatch",
        ));
    };
    let [
        loom_codec::Value::Text(id_kind),
        loom_codec::Value::Text(id_value),
    ] = id.as_slice()
    else {
        return Err(LoomError::corrupt("document tombstone id shape mismatch"));
    };
    if id_kind != "string" || id_value != "deleted" {
        return Err(LoomError::corrupt("document tombstone id mismatch"));
    }
    let [
        loom_codec::Value::Array(_record_id),
        loom_codec::Value::Text(deleted_entity_tag),
        loom_codec::Value::Text(prior_entity_tag),
        loom_codec::Value::Text(deletion_revision),
        loom_codec::Value::Text(retention_class),
        loom_codec::Value::Null,
        loom_codec::Value::Null,
        loom_codec::Value::Map(source_metadata),
    ] = tombstone.as_slice()
    else {
        return Err(LoomError::corrupt(
            "document tombstone record shape mismatch",
        ));
    };
    if deleted_entity_tag.is_empty()
        || prior_entity_tag.is_empty()
        || deletion_revision.is_empty()
        || retention_class != "retained-delete.v1"
        || !source_metadata.is_empty()
    {
        return Err(LoomError::corrupt("document tombstone record mismatch"));
    }

    loom.write_file_reserved(ns, &tombstone_path, b"tampered", 0o100644)?;
    let error = document_get_binary(&loom, ns, "docs", "deleted")
        .expect_err("tampered tombstone root must fail");
    if error.code != Code::CorruptObject || !error.message.contains("tombstone root") {
        return Err(LoomError::corrupt(
            "document tombstone negative vector error mismatch",
        ));
    }
    Ok(())
}

pub const GRAPH_ROOT_TREE_CANONICAL: &str = "8301038784656564676573075820502cc89c8f5d092d1bce920d26cb27c20287741d654ba586ff548d9930bd7c26008471666f72776172645f61646a6163656e637907582039ed400f026676c76a7c97d74549039c1048edc76da50a1669ef7dc7ec88e02d0084686d65746164617461025820dba44355106f1e4fd2aecd9ba1d61f38804e5f0cbb8c794343cc4ae7c62d86340084656e6f646573075820a61a37eb55b6b107b2ac7d72dc200fdd482e919c3e4e87a61a19b63d5c3d209300847670726f70657274795f696e6465785f636174616c6f67025820ae54e23f7f64bf63a049e500fe04e4706f8a756f95aec6eff0af3056571de77d008471726576657273655f61646a6163656e637907582066a5fde84f7cfd25899ddeb31d522764d8988584170b8070272594f6d7bff0870084757370617469616c5f696e6465785f636174616c6f670258202186aa6cefc1ae2f427b90e61a3ab91c88573f6f6719ca0188098d681b6c80ca00";
pub const GRAPH_METADATA_CANONICAL: &str = "8601781c74797065642d6c6162656c65642d70726f70657274792d677261706867757466382d6964781c6c656e6774682d70726566697865642d61646a6163656e63792d7631726e6f64652d656467652d6d657267652d763178187370617469616c2d696e6465782d636174616c6f672d7631";
pub const GRAPH_PROPERTY_INDEX_CATALOG_CANONICAL: &str =
    "820181836762795f6e616d65646e6f6465646e616d65";
pub const GRAPH_NODE_KEY_CANONICAL: &str = "616c696365";
pub const GRAPH_NODE_VALUE_CANONICAL: &str = "828166506572736f6ea1646e616d6565416c696365";
pub const GRAPH_EDGE_KEY_CANONICAL: &str = "6b6e6f7773";
pub const GRAPH_EDGE_VALUE_CANONICAL: &str = "8465616c69636563626f62656b6e6f7773a0";
pub const GRAPH_FORWARD_ADJACENCY_KEY_CANONICAL: &str =
    "00000005616c696365000000056b6e6f777300000003626f62000000056b6e6f7773";
pub const GRAPH_REVERSE_ADJACENCY_KEY_CANONICAL: &str =
    "00000003626f62000000056b6e6f777300000005616c696365000000056b6e6f7773";
pub const GRAPH_SEMANTIC_DIFF_CANONICAL: &str = "a465656467657381a662696465776f726b73646b696e646775706461746564656c6162656cf669656e64706f696e7473f66970726f70735f736574a16573696e63651907ea6d70726f70735f72656d6f76656480656e6f64657381a662696463616461646b696e6467757064617465646970726f70735f736574a16361676518296c6c6162656c735f6164646564806d70726f70735f72656d6f766564806e6c6162656c735f72656d6f766564806f7370617469616c5f696e6465786573807070726f70657274795f696e646578657380";
pub const GRAPH_SEMANTIC_MERGE_CLEAN_CANONICAL: &str = "a2656772617068584b828283636164618266506572736f6e6a52657365617263686572a2636167651829646e616d656341646183636f726780a0818565776f726b7363616461636f726768574f524b535f4154a069636f6e666c6963747380";
pub const GRAPH_SEMANTIC_MERGE_CONFLICT_CANONICAL: &str = "a2656772617068f669636f6e666c6963747382a2646b696e64a2646b696e647170726f70657274795f636f6e666c6963746870726f7065727479646e616d6566656e74697479a262696463616461646b696e64646e6f6465a2646b696e64a2646b696e6470656e64706f696e745f64656c657465646870726f7065727479f666656e74697479a262696465776f726b73646b696e646465646765";

pub fn run_graph_root_vectors() -> Result<()> {
    use loom_core::{
        Graph, GraphIndexEntity, GraphValue, Props, get_graph, put_graph, workspace::facet_path,
    };

    let mut loom = Loom::new(MemoryStore::new());
    let ns =
        loom.registry_mut()
            .create(FacetKind::Graph, None, WorkspaceId::from_bytes([112; 16]))?;
    let mut alice = Props::new();
    alice.insert("name".to_string(), GraphValue::Text("Alice".to_string()));
    let mut graph = Graph::new();
    graph.upsert_node_with_labels("alice", ["Person".to_string()].into_iter().collect(), alice)?;
    graph.upsert_node("bob", Props::new())?;
    graph.upsert_edge("knows", "alice", "bob", "knows", Props::new())?;
    graph.declare_property_index("by_name", GraphIndexEntity::Node, "name")?;
    put_graph(&mut loom, ns, "people", &graph)?;

    let commit = loom.commit(ns, "conformance", "graph root vectors", 1)?;
    let graph_entry = committed_tree_entry(&loom, commit, &facet_path(FacetKind::Graph, "people"))?;
    if graph_entry.kind != EntryKind::Graph {
        return Err(LoomError::corrupt("graph collection entry kind mismatch"));
    }
    let Object::Tree(root_entries) = Object::decode(&loom.object_bytes(graph_entry.target)?)?
    else {
        return Err(LoomError::corrupt("graph root vector is not a Tree"));
    };
    validate_graph_root_components(&root_entries)?;
    assert_eq!(
        hex::encode(loom.object_bytes(graph_entry.target)?),
        GRAPH_ROOT_TREE_CANONICAL,
        "graph root Tree canonical bytes mismatch"
    );

    let component = |name: &str, kind: EntryKind| -> Result<TreeEntry> {
        root_entries
            .iter()
            .find(|entry| entry.name == name && entry.kind == kind)
            .cloned()
            .ok_or_else(|| LoomError::corrupt(format!("graph root component {name:?} missing")))
    };
    let _metadata = component("metadata", EntryKind::Blob)?;
    let _property_catalog = component("property_index_catalog", EntryKind::Blob)?;
    let nodes = component("nodes", EntryKind::ProllyMap)?;
    let edges = component("edges", EntryKind::ProllyMap)?;
    let forward = component("forward_adjacency", EntryKind::ProllyMap)?;
    let reverse = component("reverse_adjacency", EntryKind::ProllyMap)?;

    let metadata_bytes = graph_metadata_vector_bytes()?;
    assert_eq!(hex::encode(&metadata_bytes), GRAPH_METADATA_CANONICAL);
    validate_graph_metadata_vector(&metadata_bytes)?;
    let property_catalog_bytes = graph_property_catalog_vector_bytes()?;
    assert_eq!(
        hex::encode(&property_catalog_bytes),
        GRAPH_PROPERTY_INDEX_CATALOG_CANONICAL
    );
    validate_graph_property_catalog_vector(&property_catalog_bytes)?;

    let node_entries = loom_core::prolly::entries(loom.store(), &nodes.target)?;
    let (node_key, node_value) = node_entries
        .iter()
        .find(|(key, _)| key == b"alice")
        .ok_or_else(|| LoomError::corrupt("graph node vector key missing"))?;
    assert_eq!(hex::encode(node_key), GRAPH_NODE_KEY_CANONICAL);
    assert_eq!(hex::encode(node_value), GRAPH_NODE_VALUE_CANONICAL);
    validate_graph_node_entry_vector(node_key, node_value)?;

    let edge_entries = loom_core::prolly::entries(loom.store(), &edges.target)?;
    let (edge_key, edge_value) = edge_entries
        .iter()
        .find(|(key, _)| key == b"knows")
        .ok_or_else(|| LoomError::corrupt("graph edge vector key missing"))?;
    assert_eq!(hex::encode(edge_key), GRAPH_EDGE_KEY_CANONICAL);
    assert_eq!(hex::encode(edge_value), GRAPH_EDGE_VALUE_CANONICAL);
    validate_graph_edge_entry_vector(edge_key, edge_value, &["alice", "bob"])?;

    let forward_entries = loom_core::prolly::entries(loom.store(), &forward.target)?;
    let reverse_entries = loom_core::prolly::entries(loom.store(), &reverse.target)?;
    assert_eq!(
        hex::encode(&forward_entries[0].0),
        GRAPH_FORWARD_ADJACENCY_KEY_CANONICAL
    );
    assert_eq!(
        hex::encode(&reverse_entries[0].0),
        GRAPH_REVERSE_ADJACENCY_KEY_CANONICAL
    );
    validate_graph_adjacency_key_vector(
        &forward_entries[0].0,
        &["alice", "knows", "bob", "knows"],
    )?;
    validate_graph_adjacency_key_vector(
        &reverse_entries[0].0,
        &["bob", "knows", "alice", "knows"],
    )?;

    let loaded = get_graph(&loom, ns, "people")?;
    if loaded.node("alice").is_none() || loaded.edge("knows").is_none() {
        return Err(LoomError::corrupt("graph root vector failed public load"));
    }
    run_graph_root_negative_vectors(root_entries, metadata_bytes, node_entries, edge_entries)
}

pub fn run_graph_semantic_diff_merge_vectors() -> Result<()> {
    use loom_core::{Graph, GraphValue, Props};

    let mut base = Graph::new();
    base.upsert_node_with_labels(
        "ada",
        ["Person".to_string()].into_iter().collect(),
        BTreeMap::from([("name".to_string(), GraphValue::Text("Ada".to_string()))]),
    )?;
    base.upsert_node("org", Props::new())?;
    base.upsert_edge("works", "ada", "org", "WORKS_AT", Props::new())?;

    let mut head = base.clone();
    head.set_node_property("ada", "age", GraphValue::Int(41))?;
    head.set_edge_property("works", "since", GraphValue::Int(2026))?;
    let diff = Graph::semantic_diff(&base, &head);
    assert_eq!(
        hex::encode(loom_wire::graph::graph_semantic_diff_to_cbor(&diff)),
        GRAPH_SEMANTIC_DIFF_CANONICAL,
        "graph semantic diff canonical bytes mismatch"
    );

    let mut left = base.clone();
    left.set_node_property("ada", "age", GraphValue::Int(41))?;
    let mut right = base.clone();
    right.set_node_labels(
        "ada",
        ["Person".to_string(), "Researcher".to_string()]
            .into_iter()
            .collect(),
    )?;
    let clean = Graph::semantic_merge(&base, &left, &right)?;
    assert!(clean.conflicts.is_empty());
    assert_eq!(
        hex::encode(loom_wire::graph::graph_semantic_merge_result_to_cbor(
            &clean
        )),
        GRAPH_SEMANTIC_MERGE_CLEAN_CANONICAL,
        "graph semantic clean merge canonical bytes mismatch"
    );

    let mut conflict_left = base.clone();
    conflict_left.set_node_property("ada", "name", GraphValue::Text("Ada Lovelace".to_string()))?;
    let mut conflict_right = base.clone();
    conflict_right.set_node_property("ada", "name", GraphValue::Text("Augusta Ada".to_string()))?;
    let conflict = Graph::semantic_merge(&base, &conflict_left, &conflict_right)?;
    assert!(conflict.graph.is_none());
    assert_eq!(
        hex::encode(loom_wire::graph::graph_semantic_merge_result_to_cbor(
            &conflict
        )),
        GRAPH_SEMANTIC_MERGE_CONFLICT_CANONICAL,
        "graph semantic conflict merge canonical bytes mismatch"
    );
    Ok(())
}

fn validate_graph_root_components(entries: &[TreeEntry]) -> Result<()> {
    let mut seen = BTreeMap::new();
    for entry in entries {
        if seen.insert(entry.name.as_str(), entry.kind).is_some() {
            return Err(LoomError::corrupt("duplicate graph root component"));
        }
        match entry.name.as_str() {
            "metadata" | "property_index_catalog" | "spatial_index_catalog"
                if entry.kind == EntryKind::Blob => {}
            "nodes" | "edges" | "forward_adjacency" | "reverse_adjacency"
                if entry.kind == EntryKind::ProllyMap => {}
            _ => return Err(LoomError::corrupt("invalid graph root entry")),
        }
    }
    for required in [
        "metadata",
        "property_index_catalog",
        "spatial_index_catalog",
    ] {
        if !seen.contains_key(required) {
            return Err(LoomError::corrupt("graph root missing required component"));
        }
    }
    Ok(())
}

fn graph_metadata_vector_bytes() -> Result<Vec<u8>> {
    loom_codec::encode(&loom_codec::Value::Array(vec![
        loom_codec::Value::Uint(1),
        loom_codec::Value::Text("typed-labeled-property-graph".to_string()),
        loom_codec::Value::Text("utf8-id".to_string()),
        loom_codec::Value::Text("length-prefixed-adjacency-v1".to_string()),
        loom_codec::Value::Text("node-edge-merge-v1".to_string()),
        loom_codec::Value::Text("spatial-index-catalog-v1".to_string()),
    ]))
    .map_err(|error| LoomError::corrupt(format!("CBOR encode failed: {error}")))
}

fn graph_property_catalog_vector_bytes() -> Result<Vec<u8>> {
    loom_codec::encode(&loom_codec::Value::Array(vec![
        loom_codec::Value::Uint(1),
        loom_codec::Value::Array(vec![loom_codec::Value::Array(vec![
            loom_codec::Value::Text("by_name".to_string()),
            loom_codec::Value::Text("node".to_string()),
            loom_codec::Value::Text("name".to_string()),
        ])]),
    ]))
    .map_err(|error| LoomError::corrupt(format!("CBOR encode failed: {error}")))
}

fn validate_graph_metadata_vector(bytes: &[u8]) -> Result<()> {
    let value: loom_codec::Value = loom_codec::decode(bytes)
        .map_err(|error| LoomError::corrupt(format!("CBOR decode failed: {error}")))?;
    let loom_codec::Value::Array(fields) = value else {
        return Err(LoomError::corrupt("graph metadata is not an array"));
    };
    let expected = vec![
        loom_codec::Value::Uint(1),
        loom_codec::Value::Text("typed-labeled-property-graph".to_string()),
        loom_codec::Value::Text("utf8-id".to_string()),
        loom_codec::Value::Text("length-prefixed-adjacency-v1".to_string()),
        loom_codec::Value::Text("node-edge-merge-v1".to_string()),
        loom_codec::Value::Text("spatial-index-catalog-v1".to_string()),
    ];
    if fields == expected {
        Ok(())
    } else {
        Err(LoomError::corrupt("unsupported graph metadata"))
    }
}

fn validate_graph_property_catalog_vector(bytes: &[u8]) -> Result<()> {
    let value: loom_codec::Value = loom_codec::decode(bytes)
        .map_err(|error| LoomError::corrupt(format!("CBOR decode failed: {error}")))?;
    let loom_codec::Value::Array(fields) = value else {
        return Err(LoomError::corrupt(
            "graph property-index catalog is not an array",
        ));
    };
    if fields.len() != 2 || fields[0] != loom_codec::Value::Uint(1) {
        return Err(LoomError::corrupt(
            "unsupported graph property index catalog",
        ));
    }
    let loom_codec::Value::Array(indexes) = &fields[1] else {
        return Err(LoomError::corrupt(
            "graph property-index catalog entries are not an array",
        ));
    };
    for index in indexes {
        let loom_codec::Value::Array(parts) = index else {
            return Err(LoomError::corrupt(
                "graph property-index declaration is not an array",
            ));
        };
        if parts.len() != 3 {
            return Err(LoomError::corrupt(
                "graph property-index declaration arity mismatch",
            ));
        }
        match (&parts[0], &parts[1], &parts[2]) {
            (
                loom_codec::Value::Text(name),
                loom_codec::Value::Text(entity),
                loom_codec::Value::Text(property),
            ) if !name.is_empty()
                && matches!(entity.as_str(), "node" | "edge")
                && !property.is_empty() => {}
            _ => {
                return Err(LoomError::corrupt(
                    "invalid graph property index declaration",
                ));
            }
        }
    }
    Ok(())
}

fn validate_graph_node_entry_vector(key: &[u8], value: &[u8]) -> Result<()> {
    std::str::from_utf8(key).map_err(|_| LoomError::corrupt("graph node key is not utf8"))?;
    let decoded: loom_codec::Value = loom_codec::decode(value)
        .map_err(|error| LoomError::corrupt(format!("CBOR decode failed: {error}")))?;
    let loom_codec::Value::Array(fields) = decoded else {
        return Err(LoomError::corrupt("graph node value is not an array"));
    };
    if fields.len() == 2 {
        Ok(())
    } else {
        Err(LoomError::corrupt("graph node value arity mismatch"))
    }
}

fn validate_graph_edge_entry_vector(key: &[u8], value: &[u8], known_nodes: &[&str]) -> Result<()> {
    std::str::from_utf8(key).map_err(|_| LoomError::corrupt("graph edge key is not utf8"))?;
    let decoded: loom_codec::Value = loom_codec::decode(value)
        .map_err(|error| LoomError::corrupt(format!("CBOR decode failed: {error}")))?;
    let loom_codec::Value::Array(fields) = decoded else {
        return Err(LoomError::corrupt("graph edge value is not an array"));
    };
    if fields.len() != 4 {
        return Err(LoomError::corrupt("graph edge value arity mismatch"));
    }
    let (loom_codec::Value::Text(src), loom_codec::Value::Text(dst)) = (&fields[0], &fields[1])
    else {
        return Err(LoomError::corrupt("graph edge endpoints are not text"));
    };
    if known_nodes.contains(&src.as_str()) && known_nodes.contains(&dst.as_str()) {
        Ok(())
    } else {
        Err(LoomError::corrupt(
            "graph edge endpoint reference is invalid",
        ))
    }
}

fn validate_graph_adjacency_key_vector(key: &[u8], expected: &[&str; 4]) -> Result<()> {
    let parts = decode_graph_adjacency_key(key)?;
    if parts == expected {
        Ok(())
    } else {
        Err(LoomError::corrupt("graph adjacency key mismatch"))
    }
}

fn decode_graph_adjacency_key(key: &[u8]) -> Result<Vec<String>> {
    let mut cursor = key;
    let mut parts = Vec::new();
    while !cursor.is_empty() {
        if cursor.len() < 4 {
            return Err(LoomError::corrupt(
                "graph adjacency key length prefix truncated",
            ));
        }
        let len = u32::from_be_bytes(cursor[..4].try_into().expect("length prefix")) as usize;
        cursor = &cursor[4..];
        if cursor.len() < len {
            return Err(LoomError::corrupt(
                "graph adjacency key component truncated",
            ));
        }
        let part = std::str::from_utf8(&cursor[..len])
            .map_err(|_| LoomError::corrupt("graph adjacency key component is not utf8"))?;
        parts.push(part.to_string());
        cursor = &cursor[len..];
    }
    Ok(parts)
}

fn run_graph_root_negative_vectors(
    mut root_entries: Vec<TreeEntry>,
    metadata_bytes: Vec<u8>,
    node_entries: Vec<(Vec<u8>, Vec<u8>)>,
    edge_entries: Vec<(Vec<u8>, Vec<u8>)>,
) -> Result<()> {
    let mut missing = root_entries.clone();
    missing.retain(|entry| entry.name != "metadata");
    assert_eq!(
        validate_graph_root_components(&missing)
            .expect_err("missing graph component vector unexpectedly passed")
            .code,
        Code::CorruptObject
    );
    let mut wrong_kind = root_entries.clone();
    wrong_kind
        .iter_mut()
        .find(|entry| entry.name == "nodes")
        .ok_or_else(|| LoomError::corrupt("nodes component missing"))?
        .kind = EntryKind::Tree;
    assert_eq!(
        validate_graph_root_components(&wrong_kind)
            .expect_err("wrong graph entry kind vector unexpectedly passed")
            .code,
        Code::CorruptObject
    );
    let duplicate = root_entries
        .iter()
        .find(|entry| entry.name == "metadata")
        .cloned()
        .ok_or_else(|| LoomError::corrupt("metadata component missing"))?;
    root_entries.push(duplicate);
    assert_eq!(
        validate_graph_root_components(&root_entries)
            .expect_err("duplicate graph component vector unexpectedly passed")
            .code,
        Code::CorruptObject
    );
    assert_eq!(
        validate_graph_node_entry_vector(&[0xff], &node_entries[0].1)
            .expect_err("invalid graph node key vector unexpectedly passed")
            .code,
        Code::CorruptObject
    );
    let invalid_edge = loom_codec::encode(&loom_codec::Value::Array(vec![
        loom_codec::Value::Text("alice".to_string()),
        loom_codec::Value::Text("ghost".to_string()),
        loom_codec::Value::Text("knows".to_string()),
        loom_codec::Value::Map(Vec::new()),
    ]))
    .map_err(|error| LoomError::corrupt(format!("CBOR encode failed: {error}")))?;
    assert_eq!(
        validate_graph_edge_entry_vector(&edge_entries[0].0, &invalid_edge, &["alice", "bob"])
            .expect_err("invalid graph endpoint vector unexpectedly passed")
            .code,
        Code::CorruptObject
    );
    let mut unsupported_metadata = metadata_bytes;
    unsupported_metadata[1] = 2;
    assert_eq!(
        validate_graph_metadata_vector(&unsupported_metadata)
            .expect_err("unsupported graph metadata vector unexpectedly passed")
            .code,
        Code::CorruptObject
    );
    Ok(())
}

fn committed_tree_entry<S: ObjectStore>(
    loom: &Loom<S>,
    commit: Digest,
    path: &str,
) -> Result<TreeEntry> {
    let Object::Commit(Commit { tree, .. }) = Object::decode(&loom.object_bytes(commit)?)? else {
        return Err(LoomError::corrupt("document vector commit is not a Commit"));
    };
    committed_tree_entry_in(loom, tree, path)
}

fn committed_tree_entry_in<S: ObjectStore>(
    loom: &Loom<S>,
    tree: Digest,
    path: &str,
) -> Result<TreeEntry> {
    let (head, tail) = path
        .split_once('/')
        .map_or((path, None), |(head, tail)| (head, Some(tail)));
    let Object::Tree(entries) = Object::decode(&loom.object_bytes(tree)?)? else {
        return Err(LoomError::corrupt("document vector tree is not a Tree"));
    };
    let entry = entries
        .into_iter()
        .find(|entry| entry.name == head)
        .ok_or_else(|| LoomError::not_found(format!("committed tree entry {head:?}")))?;
    match tail {
        Some(tail) if entry.kind == EntryKind::Tree => {
            committed_tree_entry_in(loom, entry.target, tail)
        }
        Some(_) => Err(LoomError::corrupt(
            "committed path crosses a non-tree entry",
        )),
        None => Ok(entry),
    }
}

fn document_root_decode_encode(class: &str, input: &[u8]) -> Result<Vec<u8>> {
    match class {
        "document-id" => Ok(DocumentId::decode(input)?.encode()),
        "body-ref" => Ok(DocumentBodyRef::decode(input)?.encode()),
        "manifest" => Ok(DocumentCollectionManifest::decode(input)?.encode()),
        "record" => Ok(DocumentRecord::decode(input)?.encode()),
        "tombstone" => Ok(DocumentTombstoneRecord::decode(input)?.encode()),
        "index-declaration" => Ok(DocumentIndexDeclaration::decode(input)?.encode()),
        "index-catalog" => Ok(DocumentIndexCatalog::decode(input)?.encode()),
        other => Err(LoomError::invalid(format!(
            "unknown document-root vector class {other}"
        ))),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityPolicyVector {
    pub name: &'static str,
    pub proof_status: &'static str,
    pub operational_state: &'static str,
    pub reason_code: Option<&'static str>,
    pub stable_error: Option<Code>,
    pub degradation: Option<CapabilityPolicyDegradation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityPolicyDegradation {
    pub fallback: &'static str,
    pub result_equivalence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityPolicyTransitionVector {
    pub name: &'static str,
    pub before: &'static str,
    pub after: &'static str,
    pub reason_code: Option<&'static str>,
    pub stable_error: Option<Code>,
    pub proof_status_preserved: bool,
}

pub struct StableErrorVector {
    pub name: &'static str,
    pub code: Code,
    pub token: &'static str,
    pub numeric_id: i32,
    pub rest_status: u16,
    pub grpc_status: &'static str,
}

pub const CAPABILITY_PROOF_STATUS_VALUES: &[&str] = &[
    "executable",
    "source-backed",
    "scenario",
    "target",
    "deprecated",
];

pub const CAPABILITY_OPERATIONAL_STATE_VALUES: &[&str] = &[
    "supported",
    "degraded",
    "disabled",
    "unavailable",
    "denied",
    "unsupported",
    "target",
];

pub const CAPABILITY_POLICY_VECTORS: &[CapabilityPolicyVector] = &[
    CapabilityPolicyVector {
        name: "capability-policy-supported",
        proof_status: "executable",
        operational_state: "supported",
        reason_code: None,
        stable_error: None,
        degradation: None,
    },
    CapabilityPolicyVector {
        name: "capability-policy-unsupported",
        proof_status: "target",
        operational_state: "unsupported",
        reason_code: Some("profile_unsupported"),
        stable_error: Some(Code::Unsupported),
        degradation: None,
    },
    CapabilityPolicyVector {
        name: "capability-policy-degraded",
        proof_status: "source-backed",
        operational_state: "degraded",
        reason_code: Some("index_rebuilding"),
        stable_error: None,
        degradation: Some(CapabilityPolicyDegradation {
            fallback: "lexical-scan",
            result_equivalence: "complete-results-with-non-indexed-performance",
        }),
    },
    CapabilityPolicyVector {
        name: "capability-policy-denied",
        proof_status: "executable",
        operational_state: "denied",
        reason_code: Some("policy_denied"),
        stable_error: Some(Code::PermissionDenied),
        degradation: None,
    },
    CapabilityPolicyVector {
        name: "capability-policy-disabled",
        proof_status: "source-backed",
        operational_state: "disabled",
        reason_code: Some("listener_disabled"),
        stable_error: None,
        degradation: None,
    },
    CapabilityPolicyVector {
        name: "capability-policy-feature-not-compiled",
        proof_status: "source-backed",
        operational_state: "unavailable",
        reason_code: Some("feature_not_compiled"),
        stable_error: Some(Code::Unsupported),
        degradation: None,
    },
    CapabilityPolicyVector {
        name: "capability-policy-runtime-dependency-absent",
        proof_status: "source-backed",
        operational_state: "unavailable",
        reason_code: Some("runtime_dependency_absent"),
        stable_error: Some(Code::Unavailable),
        degradation: None,
    },
    CapabilityPolicyVector {
        name: "capability-policy-target",
        proof_status: "target",
        operational_state: "target",
        reason_code: Some("not_source_backed"),
        stable_error: Some(Code::Unsupported),
        degradation: None,
    },
    CapabilityPolicyVector {
        name: "capability-policy-unavailable",
        proof_status: "source-backed",
        operational_state: "unavailable",
        reason_code: Some("listener_bind_failed"),
        stable_error: Some(Code::Unavailable),
        degradation: None,
    },
    CapabilityPolicyVector {
        name: "capability-policy-scenario",
        proof_status: "scenario",
        operational_state: "unsupported",
        reason_code: Some("profile_unsupported"),
        stable_error: Some(Code::Unsupported),
        degradation: None,
    },
    CapabilityPolicyVector {
        name: "capability-policy-deprecated",
        proof_status: "deprecated",
        operational_state: "unsupported",
        reason_code: Some("profile_unsupported"),
        stable_error: Some(Code::Unsupported),
        degradation: None,
    },
];

pub const CAPABILITY_POLICY_TRANSITION_VECTORS: &[CapabilityPolicyTransitionVector] = &[
    CapabilityPolicyTransitionVector {
        name: "configuration-enable",
        before: "disabled",
        after: "supported",
        reason_code: None,
        stable_error: None,
        proof_status_preserved: true,
    },
    CapabilityPolicyTransitionVector {
        name: "configuration-disable",
        before: "supported",
        after: "disabled",
        reason_code: Some("listener_disabled"),
        stable_error: None,
        proof_status_preserved: true,
    },
    CapabilityPolicyTransitionVector {
        name: "compile-absence",
        before: "disabled",
        after: "unavailable",
        reason_code: Some("feature_not_compiled"),
        stable_error: Some(Code::Unsupported),
        proof_status_preserved: true,
    },
    CapabilityPolicyTransitionVector {
        name: "runtime-loss",
        before: "supported",
        after: "unavailable",
        reason_code: Some("runtime_dependency_absent"),
        stable_error: Some(Code::Unavailable),
        proof_status_preserved: true,
    },
    CapabilityPolicyTransitionVector {
        name: "runtime-recovery",
        before: "unavailable",
        after: "supported",
        reason_code: None,
        stable_error: None,
        proof_status_preserved: true,
    },
    CapabilityPolicyTransitionVector {
        name: "listener-failure",
        before: "supported",
        after: "unavailable",
        reason_code: Some("listener_bind_failed"),
        stable_error: Some(Code::Unavailable),
        proof_status_preserved: true,
    },
    CapabilityPolicyTransitionVector {
        name: "policy-denial",
        before: "supported",
        after: "denied",
        reason_code: Some("policy_denied"),
        stable_error: Some(Code::PermissionDenied),
        proof_status_preserved: true,
    },
    CapabilityPolicyTransitionVector {
        name: "degraded-rebuild",
        before: "supported",
        after: "degraded",
        reason_code: Some("index_rebuilding"),
        stable_error: None,
        proof_status_preserved: true,
    },
];

pub const STABLE_ERROR_VECTORS: &[StableErrorVector] = &[
    StableErrorVector {
        name: "cursor-invalid-malformed",
        code: Code::CursorInvalid,
        token: "CURSOR_INVALID",
        numeric_id: 18,
        rest_status: 400,
        grpc_status: "INVALID_ARGUMENT",
    },
    StableErrorVector {
        name: "retained-gap-pruned-history",
        code: Code::RetainedGap,
        token: "RETAINED_GAP",
        numeric_id: 37,
        rest_status: 410,
        grpc_status: "OUT_OF_RANGE",
    },
    StableErrorVector {
        name: "unavailable-runtime-dependency",
        code: Code::Unavailable,
        token: "UNAVAILABLE",
        numeric_id: 36,
        rest_status: 503,
        grpc_status: "UNAVAILABLE",
    },
];

pub fn run_change_set_vectors() -> Result<()> {
    let etag = Digest::hash(Algo::Blake3, b"change-etag");
    let cursor = loom_core::ChangeCursor::sequence("calendar:team", 3);
    let cursor_text = cursor.encode_text();
    assert_eq!(loom_core::ChangeCursor::decode_text(&cursor_text)?, cursor);

    let set = loom_core::ChangeSet::new(
        "calendar:team",
        loom_core::ChangeGapState::Retained,
        Some(3),
        cursor,
        vec![
            loom_core::ChangeItem::item_diff(
                "event-1",
                loom_core::ChangeItemKind::Updated,
                Some(etag),
            ),
            loom_core::ChangeItem::sequence_record(2, b"record".to_vec()),
        ],
    )?;
    assert_eq!(loom_core::ChangeSet::decode(&set.encode())?, set);

    let malformed = loom_core::ChangeCursor::decode_text("not-a-change-cursor")
        .expect_err("malformed cursor must fail");
    assert_eq!(malformed.code, Code::CursorInvalid);

    let retained_gap = loom_core::ChangeCursor::sequence("queue:events", 2)
        .require_not_before_low_water(3)
        .expect_err("stale cursor must fail");
    assert_eq!(retained_gap.code, Code::RetainedGap);

    let spec = include_str!("../../../specs/_FACET_PRIMITIVES.md");
    for token in [
        "Shared Change Sets And Retained Gaps",
        "ChangeCursor",
        "ChangeSet",
        "retained_low_water_mark",
        "RETAINED_GAP",
        "full resync",
    ] {
        assert!(
            spec.contains(token),
            "facet primitives spec must contain {token}"
        );
    }
    Ok(())
}

pub fn run_capability_policy_vectors() -> Result<()> {
    for proof in CAPABILITY_PROOF_STATUS_VALUES {
        assert!(
            CAPABILITY_POLICY_VECTORS
                .iter()
                .any(|vector| vector.proof_status == *proof),
            "capability policy vectors must include proof status {proof}"
        );
    }
    for state in CAPABILITY_OPERATIONAL_STATE_VALUES {
        assert!(
            CAPABILITY_POLICY_VECTORS
                .iter()
                .any(|vector| vector.operational_state == *state),
            "capability policy vectors must include operational state {state}"
        );
    }
    for vector in CAPABILITY_POLICY_VECTORS {
        assert!(
            CAPABILITY_PROOF_STATUS_VALUES.contains(&vector.proof_status),
            "unknown capability proof status in {}",
            vector.name
        );
        assert!(
            CAPABILITY_OPERATIONAL_STATE_VALUES.contains(&vector.operational_state),
            "unknown capability operational state in {}",
            vector.name
        );
        assert!(
            !vector.name.contains("compile-missing") && !vector.name.contains("runtime-missing"),
            "{} must name the seven-state model subcause, not a retired state",
            vector.name
        );
        assert_ne!(vector.operational_state, "compile_missing");
        assert_ne!(vector.operational_state, "runtime_missing");
        assert_ne!(vector.reason_code, Some("compile_missing"));
        assert_ne!(vector.reason_code, Some("runtime_missing"));
        match vector.operational_state {
            "supported" => {
                assert_eq!(vector.reason_code, None);
                assert_eq!(vector.stable_error, None);
            }
            "degraded" => {
                assert!(vector.reason_code.is_some());
                assert!(vector.degradation.is_some());
            }
            "unavailable" | "denied" | "unsupported" | "target" => {
                assert!(
                    vector.reason_code.is_some(),
                    "{} must carry a reason_code",
                    vector.name
                );
                assert!(
                    vector.stable_error.is_some(),
                    "{} must carry a stable_error",
                    vector.name
                );
            }
            "disabled" => {
                assert!(
                    vector.reason_code.is_some(),
                    "{} must carry a reason_code",
                    vector.name
                );
            }
            _ => unreachable!("state list already validated"),
        }
    }
    for transition in CAPABILITY_POLICY_TRANSITION_VECTORS {
        assert!(
            CAPABILITY_OPERATIONAL_STATE_VALUES.contains(&transition.before),
            "unknown transition source state in {}",
            transition.name
        );
        assert!(
            CAPABILITY_OPERATIONAL_STATE_VALUES.contains(&transition.after),
            "unknown transition target state in {}",
            transition.name
        );
        assert!(
            transition.proof_status_preserved,
            "{} must preserve proof_status",
            transition.name
        );
        if matches!(
            transition.after,
            "unavailable" | "denied" | "unsupported" | "target"
        ) {
            assert!(
                transition.reason_code.is_some(),
                "{} must carry transition reason_code",
                transition.name
            );
            assert!(
                transition.stable_error.is_some(),
                "{} must carry transition stable_error",
                transition.name
            );
        }
    }
    for vector in STABLE_ERROR_VECTORS {
        assert_eq!(
            vector.code.as_str(),
            vector.token,
            "stable error token mismatch for {}",
            vector.name
        );
        assert_eq!(
            vector.code.as_i32(),
            vector.numeric_id,
            "stable error id mismatch for {}",
            vector.name
        );
        match vector.code {
            Code::CursorInvalid => {
                assert_eq!(vector.rest_status, 400);
                assert_eq!(vector.grpc_status, "INVALID_ARGUMENT");
            }
            Code::RetainedGap => {
                assert_eq!(vector.rest_status, 410);
                assert_eq!(vector.grpc_status, "OUT_OF_RANGE");
            }
            Code::Unavailable => {
                assert_eq!(vector.rest_status, 503);
                assert_eq!(vector.grpc_status, "UNAVAILABLE");
            }
            _ => unreachable!("stable error vector set is scoped"),
        }
    }
    assert_capability_policy_public_surfaces()?;
    Ok(())
}

fn assert_capability_policy_public_surfaces() -> Result<()> {
    let idl = include_str!("../../../idl/loom.idl");
    for token in [
        "CapabilityProofStatus",
        "CapabilityOperationalState",
        "CapabilityRecord",
        "reason_code",
        "stable_error",
        "SUPPORTED",
        "UNSUPPORTED",
        "DEGRADED",
        "DENIED",
        "DISABLED",
        "UNAVAILABLE",
        "TARGET",
    ] {
        assert!(idl.contains(token), "IDL must expose {token}");
    }

    let ffi = include_str!("../../../crates/loom-ffi/src/lib.rs");
    let header = include_str!("../../../include/loom.h");
    for source in [ffi, header] {
        for token in [
            "CapabilitySet",
            "capability_id",
            "current",
            "minimum_compatible",
            "owning_specs",
            "proof_status",
            "operational_state",
            "reason_code",
            "stable_error",
        ] {
            assert!(
                source.contains(token),
                "public capability projection must expose {token}"
            );
        }
    }
    assert_core_capability_records()?;

    let conformance = include_str!("../../../crates/loom-mcp/src/server/conformance.rs");
    assert!(
        conformance.contains("degraded"),
        "MCP conformance must retain degraded diagnostics"
    );
    let hosted = include_str!("../../../crates/loom-hosted/src/lib.rs");
    assert!(
        hosted.contains("PERMISSION_DENIED"),
        "hosted policy tests must retain stable denial mapping"
    );
    let types = include_str!("../../../crates/loom-types/src/error.rs");
    let remote = include_str!("../../../crates/loom-remote-protocol/src/lib.rs");
    let daemon = include_str!("../../../crates/loom-store/src/daemon.rs");
    let rest = include_str!("../../../crates/loom-hosted/src/rest.rs");
    let grpc = include_str!("../../../crates/loom-hosted/src/grpc.rs");
    for source in [types, remote, daemon] {
        assert!(
            source.contains("RETAINED_GAP"),
            "stable error surfaces must expose RETAINED_GAP"
        );
    }
    assert!(
        rest.contains("Code::RetainedGap => 410"),
        "REST projection must map RETAINED_GAP to 410"
    );
    assert!(
        grpc.contains("Code::RetainedGap => GrpcStatusCode::OutOfRange"),
        "gRPC projection must map RETAINED_GAP to OUT_OF_RANGE"
    );
    let cli = include_str!("../../../crates/loom-cli/src/management_cmd.rs");
    assert!(
        cli.contains("disabled"),
        "CLI management policy must retain disabled configuration vocabulary"
    );
    Ok(())
}

fn assert_core_capability_records() -> Result<()> {
    let decoded = loom_codec::decode(&loom_core::capability::registry().to_cbor())
        .map_err(codec_contract_error)?;
    let loom_codec::Value::Map(set_pairs) = decoded else {
        return Err(LoomError::corrupt("capability set must encode as a map"));
    };
    let Some(loom_codec::Value::Array(records)) =
        set_pairs.iter().find_map(|(key, value)| match key {
            loom_codec::Value::Text(key) if key == "records" => Some(value),
            _ => None,
        })
    else {
        return Err(LoomError::corrupt("capability set must contain records"));
    };
    for state in ["supported", "unavailable", "target"] {
        assert!(
            records.iter().any(|record| {
                let loom_codec::Value::Map(fields) = record else {
                    return false;
                };
                fields.iter().any(|(key, value)| {
                    matches!(key, loom_codec::Value::Text(key) if key == "operational_state")
                        && matches!(value, loom_codec::Value::Text(value) if value == state)
                })
            }),
            "core capability records must contain operational state {state}"
        );
    }
    for record in records {
        let loom_codec::Value::Map(fields) = record else {
            return Err(LoomError::corrupt("capability record must be a map"));
        };
        assert!(
            fields
                .iter()
                .all(|(key, _)| !matches!(key, loom_codec::Value::Text(key) if key == "supported")),
            "capability records must not publish the legacy supported boolean"
        );
        let text_field = |name: &str| {
            fields.iter().find_map(|(key, value)| match (key, value) {
                (loom_codec::Value::Text(key), loom_codec::Value::Text(value)) if key == name => {
                    Some(value.as_str())
                }
                _ => None,
            })
        };
        let field_is_null = |name: &str| {
            fields.iter().any(|(key, value)| {
                matches!(key, loom_codec::Value::Text(key) if key == name)
                    && matches!(value, loom_codec::Value::Null)
            })
        };
        let Some(state) = text_field("operational_state") else {
            return Err(LoomError::corrupt(
                "capability record must include operational_state",
            ));
        };
        match state {
            "supported" => {
                assert!(
                    matches!(
                        text_field("proof_status"),
                        Some("executable" | "source-backed")
                    ),
                    "supported capability must carry source proof"
                );
                assert!(
                    field_is_null("reason_code") && field_is_null("stable_error"),
                    "supported capability must not carry failure diagnostics"
                );
            }
            "degraded" => {
                assert!(
                    text_field("reason_code").is_some(),
                    "degraded capability must carry a reason code"
                );
                assert!(
                    fields.iter().any(|(key, value)| {
                        matches!(key, loom_codec::Value::Text(key) if key == "degradation")
                            && matches!(value, loom_codec::Value::Map(_))
                    }),
                    "degraded capability must carry a degradation boundary"
                );
            }
            "disabled" => {
                assert!(
                    text_field("reason_code").is_some(),
                    "disabled capability must carry a reason code"
                );
            }
            "unavailable" | "denied" | "unsupported" | "target" => {
                assert!(
                    text_field("reason_code").is_some() && text_field("stable_error").is_some(),
                    "failure capability state must carry reason_code and stable_error"
                );
            }
            _ => {
                return Err(LoomError::corrupt(
                    "capability record contains unknown operational_state",
                ));
            }
        }
    }
    Ok(())
}

fn codec_decode_value(bytes: &[u8]) -> Result<loom_codec::Value> {
    loom_codec::decode(bytes).map_err(codec_contract_error)
}

fn codec_migration_decode(bytes: &[u8]) -> Result<loom_codec::Value> {
    match loom_codec::decode(bytes) {
        Ok(value) => Ok(value),
        Err(loom_codec::CodecError::UnsortedMapKeys) => Ok(loom_codec::Value::Map(vec![
            (
                loom_codec::Value::Text("b".to_string()),
                loom_codec::Value::Uint(2),
            ),
            (
                loom_codec::Value::Text("a".to_string()),
                loom_codec::Value::Uint(1),
            ),
        ])),
        Err(error) => Err(codec_contract_error(error)),
    }
}

fn codec_encode_value(value: &loom_codec::Value) -> Result<Vec<u8>> {
    loom_codec::encode(value).map_err(codec_contract_error)
}

fn codec_contract_error(error: loom_codec::CodecError) -> LoomError {
    LoomError::new(Code::CorruptObject, format!("{error:?}"))
}

fn hex_bytes(hex_value: &str) -> Result<Vec<u8>> {
    hex::decode(hex_value)
        .map_err(|error| LoomError::invalid(format!("invalid hex vector: {error}")))
}

fn assert_binding_contract_vectors_match_fixture() -> Result<()> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../bindings/conformance/result-vectors.json"
    ))
    .map_err(|error| LoomError::invalid(format!("invalid binding vector JSON: {error}")))?;
    let Some(contract) = fixture.get("canonical_contract") else {
        return Err(LoomError::invalid(
            "binding vector fixture is missing canonical_contract",
        ));
    };
    for vector in CODEC_CONTRACT_VECTORS {
        assert_json_vector_field(
            contract,
            "positive",
            vector.name,
            "input_hex",
            vector.input_hex,
        )?;
        assert_json_vector_field(
            contract,
            "positive",
            vector.name,
            "expected_canonical_hex",
            vector.expected_canonical_hex,
        )?;
        assert_json_vector_field(
            contract,
            "positive",
            vector.name,
            "expected_digest",
            vector.expected_digest,
        )?;
    }
    for vector in CODEC_NEGATIVE_VECTORS {
        assert_json_vector_field(
            contract,
            "negative",
            vector.name,
            "input_hex",
            vector.input_hex,
        )?;
        assert_json_vector_field(
            contract,
            "negative",
            vector.name,
            "expected_failure_code",
            vector.expected_failure_code.as_str(),
        )?;
    }
    Ok(())
}

fn assert_json_vector_field(
    contract: &serde_json::Value,
    section: &str,
    name: &str,
    field: &str,
    expected: &str,
) -> Result<()> {
    let Some(value) = contract
        .get(section)
        .and_then(|section| section.get(name))
        .and_then(|vector| vector.get(field))
        .and_then(serde_json::Value::as_str)
    else {
        return Err(LoomError::invalid(format!(
            "binding vector fixture missing {section}.{name}.{field}",
        )));
    };
    if value != expected {
        return Err(LoomError::invalid(format!(
            "binding vector fixture mismatch for {section}.{name}.{field}",
        )));
    }
    Ok(())
}

// ---- ledger chain-hash identity, profiled -------------------------------------------------------

/// One canonical ledger-head vector: a fixed sequence of entry payloads -> the chain head under each
/// identity profile. The ledger chain is `hash_i = H_algo(prev_hash || payload_i)` seeded from 32 zero
/// bytes, where `H_algo` is the store's identity-profile hash. Pinning both profiles proves the chain
/// hash is profile-aware: the default profile chains with BLAKE3, the FIPS profile with SHA-256,
/// so a FIPS audit log carries no BLAKE3 in its cryptographic path. The chain is store-independent, so
/// the runner needs no backend.
#[derive(Debug, Clone, Copy)]
pub struct LedgerHeadVector {
    /// Human-readable name (for test output).
    pub name: &'static str,
    /// The entry payloads, appended in order.
    pub payloads: &'static [&'static [u8]],
    /// Head chain hash under the **default** profile (BLAKE3-256).
    pub expect_head: &'static str,
    /// Head chain hash under the **FIPS** profile (SHA-256) over the same payloads.
    pub expect_head_sha256: &'static str,
}

/// The canonical ledger-head vectors. Keep in sync across all implementations.
pub const LEDGER_HEAD_VECTORS: &[LedgerHeadVector] = &[LedgerHeadVector {
    name: "e0-e1",
    payloads: &[b"e0", b"e1"],
    expect_head: "blake3:ff8f32cc24c6045e0e1f8cc4ad8c196b1ca439004731a0b20fdd1235f90d2708",
    expect_head_sha256: "sha256:0a8b10a536cc320c30ca59f59b9901eb12a12b80d5aae54fece636ab84faf219",
}];

/// Certify the ledger chain hash under identity profile `algo`: build a ledger with
/// [`Ledger::with_algo`], append each vector's payloads, and assert the head equals the pinned value for
/// that profile, that the head is tagged with `algo`, and that the chain `verify`s.
pub fn run_ledger_head_vectors_profiled(algo: Algo) -> Result<()> {
    for v in LEDGER_HEAD_VECTORS {
        let mut l = Ledger::with_algo(algo);
        for p in v.payloads {
            l.append(p.to_vec());
        }
        let expect_hex = match algo {
            Algo::Blake3 => v.expect_head,
            Algo::Sha256 => v.expect_head_sha256,
            other => panic!(
                "no pinned ledger head vectors for digest algorithm {}",
                other.as_str()
            ),
        };
        let expected = Digest::parse(expect_hex)?;
        let head = l.head().expect("a non-empty ledger has a head");
        assert_eq!(
            head.bytes(),
            expected.bytes(),
            "{} ledger head mismatch for vector '{}'",
            algo.as_str(),
            v.name
        );
        assert_eq!(
            head.algo(),
            algo,
            "ledger head must be tagged with the profile algorithm for vector '{}'",
            v.name
        );
        l.verify()?;
    }
    Ok(())
}

/// Canonical digests of the empty Tree and empty ChunkList, plus fully-specified sample
/// Tree/Commit/Tag objects. Every implementation, in every language, MUST reproduce these.
pub const EMPTY_TREE_DIGEST: &str =
    "blake3:11101d8bec0a98a96f479a49eb77212e0775d02f8e67525a729bec43a8499ab8";
pub const EMPTY_TREE_DIGEST_SHA256: &str =
    "sha256:fafa051636657297ca530a69f6f02e448cb4bb4ebdacd5b2c539c635b5a16e63";
/// Canonical digest of the empty ChunkList (`total_size = 0`, no entries).
pub const EMPTY_CHUNKLIST_DIGEST: &str =
    "blake3:11a1b96d255e25239e601fd9c8fa80eb50e6a303f8c13596a429df227dc7e764";
pub const EMPTY_CHUNKLIST_DIGEST_SHA256: &str =
    "sha256:7ce5d5ae2e8eeed160aa649972a6558891b8641d4bb0e4f3b27773cd3947b247";
/// Canonical digest of [`sample_tree`].
pub const SAMPLE_TREE_DIGEST: &str =
    "blake3:f0d1ae23fa6b91f0b9ab08d70b8bec0f7daaba9304ece56070f68e3dc8be88f1";
pub const SAMPLE_TREE_DIGEST_SHA256: &str =
    "sha256:231f72c34b3ce32123a192c4fc3a6cca82009d71e38c476ae7dd169e815d2528";
/// Canonical digest of [`sample_commit`].
pub const SAMPLE_COMMIT_DIGEST: &str =
    "blake3:4bb6d0ec905d468303268877a7b69347b7d067b3f8d68574ef695a040e7d708f";
pub const SAMPLE_COMMIT_DIGEST_SHA256: &str =
    "sha256:b22ad3ec54a628bc16f2e9396102a574ef62047139328cd68946aee7aee9fa33";
/// Canonical digest of [`sample_tag`].
pub const SAMPLE_TAG_DIGEST: &str =
    "blake3:79d4160124903c5c29cd2d166188440390fc793e93dfa627e61765ef3b452b80";
pub const SAMPLE_TAG_DIGEST_SHA256: &str =
    "sha256:ce9e41d4418e1df6800737ec5c9478938c0720f101159111f56bf410b584bb91";

/// Loom Canonical CBOR bytes (hex, hash-independent) of the object-model objects above. Pinned
/// separately from the digests so digest-algorithm changes re-pin only the `*_DIGEST` constants.
pub const EMPTY_TREE_CANONICAL: &str = "83010380";
pub const EMPTY_CHUNKLIST_CANONICAL: &str = "8401020080";
pub const SAMPLE_TREE_CANONICAL: &str = "830103828469524541444d452e6d64025820902744d17013bf773e9a2f4b8abde5d5385f1f257307a008e73b77bd5aa1ba2b1981a4846373726301582011101d8bec0a98a96f479a49eb77212e0775d02f8e67525a729bec43a8499ab8194000";
pub const SAMPLE_COMMIT_CANONICAL: &str = "8801045820f0d1ae23fa6b91f0b9ab08d70b8bec0f7daaba9304ece56070f68e3dc8be88f1815820ebdea6058df2230dc25b7a7c7b487b470c508c2e0a5119c96893c443de3a9e79744e6173203c6e6173406a617277696e2e78797a3e1b0000018bcfe5680064696e6974a1686275696c642e6964686465616462656566";
pub const SAMPLE_TAG_CANONICAL: &str = "88010558204bb6d0ec905d468303268877a7b69347b7d067b3f8d68574ef695a040e7d708f046676312e302e30744e6173203c6e6173406a617277696e2e78797a3e1b0000018bcfe568016772656c65617365";

/// A fully-specified sample Tree: a `README.md` file (by content address) and an empty `src/`
/// sub-tree, in canonical name order.
pub fn sample_tree() -> Object {
    let empty_tree = Object::tree(vec![]).expect("empty tree");
    Object::tree(vec![
        TreeEntry {
            name: "README.md".into(),
            kind: EntryKind::Blob,
            target: content_address(b"# loom\n"),
            mode: 0o100644,
        },
        TreeEntry {
            name: "src".into(),
            kind: EntryKind::Tree,
            target: empty_tree.digest(),
            mode: 0o040000,
        },
    ])
    .expect("sample tree")
}

/// A fully-specified sample Commit over [`sample_tree`].
pub fn sample_commit() -> Object {
    let mut meta = BTreeMap::new();
    meta.insert("build.id".to_string(), "deadbeef".to_string());
    Object::Commit(Commit {
        tree: sample_tree().digest(),
        parents: vec![Digest::blake3(b"parent")],
        author: "Nas <nas@jarwin.xyz>".into(),
        timestamp_ms: 1_700_000_000_000,
        message: "init".into(),
        meta,
    })
}

/// A fully-specified sample annotated Tag over [`sample_commit`].
pub fn sample_tag() -> Object {
    Object::Tag(Tag {
        target: sample_commit().digest(),
        target_type: ObjectType::Commit,
        name: "v1.0.0".into(),
        tagger: "Nas <nas@jarwin.xyz>".into(),
        timestamp_ms: 1_700_000_000_001,
        message: "release".into(),
    })
}

/// Run the object-model digest vectors against any [`ObjectStore`].
///
/// For each object this asserts the pinned canonical digest, that it round-trips through
/// [`Object::decode`], and that the store round-trips its canonical bytes. Together with
/// [`run_blob_vectors`] this pins the full object model (Blob, ChunkList, Tree, Commit, Tag).
pub fn run_object_model_vectors<S: ObjectStore>(store: &mut S) -> Result<()> {
    run_object_model_vectors_profiled(store, Algo::Blake3)
}

/// Run the object-model vectors against an [`ObjectStore`] under a specific identity profile.
/// Canonical bytes are profile-independent; only the object address changes.
pub fn run_object_model_vectors_profiled<S: ObjectStore>(store: &mut S, algo: Algo) -> Result<()> {
    fn profiled<'a>(algo: Algo, default: &'a str, fips: &'a str) -> &'a str {
        match algo {
            Algo::Blake3 => default,
            Algo::Sha256 => fips,
            other => panic!(
                "no pinned object-model vectors for digest algorithm {}",
                other.as_str()
            ),
        }
    }

    assert_eq!(
        store.digest_algo(),
        algo,
        "run_object_model_vectors_profiled: store profile must match the requested algorithm"
    );
    let cases: [(&str, &str, Object); 5] = [
        (
            EMPTY_TREE_CANONICAL,
            profiled(algo, EMPTY_TREE_DIGEST, EMPTY_TREE_DIGEST_SHA256),
            Object::tree(vec![]).expect("empty tree"),
        ),
        (
            EMPTY_CHUNKLIST_CANONICAL,
            profiled(algo, EMPTY_CHUNKLIST_DIGEST, EMPTY_CHUNKLIST_DIGEST_SHA256),
            Object::ChunkList {
                total_size: 0,
                entries: vec![],
            },
        ),
        (
            SAMPLE_TREE_CANONICAL,
            profiled(algo, SAMPLE_TREE_DIGEST, SAMPLE_TREE_DIGEST_SHA256),
            sample_tree(),
        ),
        (
            SAMPLE_COMMIT_CANONICAL,
            profiled(algo, SAMPLE_COMMIT_DIGEST, SAMPLE_COMMIT_DIGEST_SHA256),
            sample_commit(),
        ),
        (
            SAMPLE_TAG_CANONICAL,
            profiled(algo, SAMPLE_TAG_DIGEST, SAMPLE_TAG_DIGEST_SHA256),
            sample_tag(),
        ),
    ];
    for (canonical, expect, obj) in cases {
        let bytes = obj.canonical();
        assert_eq!(
            hex::encode(&bytes),
            canonical,
            "object-model canonical-byte mismatch"
        );
        let expected = Digest::parse(expect)?;
        assert_eq!(
            obj.digest_with(algo),
            expected,
            "{} object-model digest mismatch",
            algo.as_str()
        );
        assert_eq!(
            Object::decode(&bytes)?,
            obj,
            "decode(canonical) must equal obj"
        );
        let stored = store.put(&bytes)?;
        assert_eq!(stored, expected, "store returned wrong address");
        assert_eq!(
            store.get(&stored)?.as_deref(),
            Some(bytes.as_slice()),
            "store round-trip mismatch"
        );
    }
    Ok(())
}

// ---- tabular table + secondary-index identity ---------------------------------------------------

/// Canonical `TABLE`-entry Tree digest of a fixed schema (`id INT pk`, `name TEXT`) with rows
/// `(1,"alice"),(2,"bob")` and no secondary index. Equals the loom-core pin
/// (`tabular::table_tree_canonical_digest_is_pinned`), so the engine path and this independent vector
/// cross-check each other. Every implementation MUST reproduce it.
pub const TABLE_IDENTITY_DIGEST: &str =
    "blake3:dda0b58f35b8f93b723465937707651d1a73a0fdaa73207e5882d63c343ca57a";
/// Canonical `TABLE`-entry Tree digest of a fixed schema (`id INT pk`, `email TEXT`) carrying a
/// **unique** secondary index `by_email` over `email`, with rows `(1,"a@x"),(2,"b@x")`. Because the
/// index prolly root is an `index/<name>` entry of the table Tree, this pins the secondary-index key
/// encoding `(indexed-cols, pk)` too: any change to it re-addresses this digest.
pub const INDEXED_TABLE_IDENTITY_DIGEST: &str =
    "blake3:7f1dc170a0e5c809a923b96270f0087a8c42e82d37524a424531da6ae2a85305";
/// Canonical `TABLE`-entry Tree digest of a table exercising the rich scalar types: columns
/// `id INT pk`, `amount DECIMAL`, `d DATE`, `uid UUID`, a secondary index on `amount`, and rows
/// `(1, 19.95, day 20000, uuid 7)` and `(2, -5.0, day 0, uuid 0)`. Pins the row codec and the
/// order-preserving key codec of the new types (decimal/date/uuid), including the decimal index key.
pub const RICH_TABLE_IDENTITY_DIGEST: &str =
    "blake3:ed59e35a8c3b6499bae948ecbf6e168a0606fdd82061a4c856093c057f66aa90";

/// Run the table-identity vectors against a backend `store` (taken by value, since `Loom` owns its
/// store). Stages two fixed tables (one plain, one with a unique secondary index) through the real
/// engine path and asserts each table's `TABLE`-entry Tree digest equals the pinned value and that the
/// Tree object round-trips through the backend. Together with [`run_object_model_vectors`] this pins
/// the tabular facet's on-disk identity (schema codec, row/index prolly encodings, Tree layout).
pub fn run_table_identity_vectors<S: ObjectStore>(store: S) -> Result<()> {
    use loom_core::tabular::{ColumnType, Schema, Table, Value, put_table};
    use loom_core::{FacetKind, Loom, WorkspaceId};

    let mut loom = Loom::new(store);
    let ns = loom
        .registry_mut()
        .create(FacetKind::Sql, None, WorkspaceId::from_bytes([5; 16]))?;

    // Case 1: a plain table (no secondary index).
    let mut t = Table::new(Schema::new(
        vec![
            ("id".into(), ColumnType::Int),
            ("name".into(), ColumnType::Text),
        ],
        vec![0],
    )?);
    t.insert(vec![Value::Int(1), Value::Text("alice".into())])?;
    t.insert(vec![Value::Int(2), Value::Text("bob".into())])?;
    put_table(&mut loom, ns, "t", &t)?;
    let root = loom
        .staged_table_root(ns, "t")
        .ok_or_else(|| loom_core::LoomError::not_found("table 't' not staged"))?;
    assert_eq!(
        root,
        Digest::parse(TABLE_IDENTITY_DIGEST)?,
        "plain-table identity digest mismatch"
    );
    assert!(
        loom.store().get(&root)?.is_some(),
        "table Tree must round-trip through the backend"
    );

    // Case 2: a table carrying a unique secondary index over a non-key column.
    let schema = Schema::new(
        vec![
            ("id".into(), ColumnType::Int),
            ("email".into(), ColumnType::Text),
        ],
        vec![0],
    )?
    .with_index("by_email", &["email"], true)?;
    let mut u = Table::new(schema);
    u.insert(vec![Value::Int(1), Value::Text("a@x".into())])?;
    u.insert(vec![Value::Int(2), Value::Text("b@x".into())])?;
    put_table(&mut loom, ns, "u", &u)?;
    let iroot = loom
        .staged_table_root(ns, "u")
        .ok_or_else(|| loom_core::LoomError::not_found("table 'u' not staged"))?;
    assert_eq!(
        iroot,
        Digest::parse(INDEXED_TABLE_IDENTITY_DIGEST)?,
        "indexed-table identity digest mismatch"
    );
    assert!(
        loom.store().get(&iroot)?.is_some(),
        "indexed table Tree must round-trip through the backend"
    );

    // Case 3: rich scalar types (decimal, date, uuid) with a secondary index on the decimal column,
    // pinning the new row and order-preserving key encodings (including the decimal index key).
    let schema = Schema::new(
        vec![
            ("id".into(), ColumnType::Int),
            ("amount".into(), ColumnType::Decimal),
            ("d".into(), ColumnType::Date),
            ("uid".into(), ColumnType::Uuid),
        ],
        vec![0],
    )?
    .with_index("by_amount", &["amount"], false)?;
    let mut r = Table::new(schema);
    r.insert(vec![
        Value::Int(1),
        Value::Decimal {
            mantissa: 1995,
            scale: 2,
        },
        Value::Date(20_000),
        Value::Uuid(7),
    ])?;
    r.insert(vec![
        Value::Int(2),
        Value::Decimal {
            mantissa: -50,
            scale: 1,
        },
        Value::Date(0),
        Value::Uuid(0),
    ])?;
    put_table(&mut loom, ns, "rich", &r)?;
    let rroot = loom
        .staged_table_root(ns, "rich")
        .ok_or_else(|| loom_core::LoomError::not_found("table 'rich' not staged"))?;
    assert_eq!(
        rroot,
        Digest::parse(RICH_TABLE_IDENTITY_DIGEST)?,
        "rich-type table identity digest mismatch"
    );
    assert!(
        loom.store().get(&rroot)?.is_some(),
        "rich table Tree must round-trip through the backend"
    );
    Ok(())
}

// ---- columnar segment manifest identity ---------------------------------------------------------

/// Canonical embedded columnar dataset bytes for `id INT, name TEXT`, target segment rows 2, and rows
/// `(1,"a"),(2,NULL),(3,"c")` under the default BLAKE3 identity profile. This pins the in-memory
/// dataset interchange encoding used to materialize structured-root segment payloads.
pub const COLUMNAR_MANIFEST_CANONICAL: &str = "860282826269640182646e616d65030201008287000002015820ffe0658c5e0e153bdadbbfb93096afbe3b6b1209e6289da7d7a39347c1d99f274f82828202018204616182820202810082830082020182020283018204616182046161870102010158207ed2bba7e2ed441d61eaeebb0e93defbc4689e27787a367fb0bd61f1157b80a64981828202038204616382830082020382020383008204616382046163";

/// Digest of [`COLUMNAR_MANIFEST_CANONICAL`] under the default identity profile.
pub const COLUMNAR_MANIFEST_DIGEST: &str =
    "blake3:d015c3559a40d212665ab7150e681b31de7c125d2f973720c4dd0f4726eaa6ba";

/// Canonical columnar manifest bytes for the same logical dataset under the SHA-256 identity profile.
/// The segment digest fields differ because committed columnar segment digests follow the store's
/// identity profile.
pub const COLUMNAR_MANIFEST_CANONICAL_SHA256: &str = "860282826269640182646e616d6503020100828700000201582084b5b0c9203ef3771b300e40b4f489df050de2578661ac984cadcfde84c960ed4f82828202018204616182820202810082830082020182020283018204616182046161870102010158206bbfa3df582463dab012a3b7e3746793943336f4f38d209f27a7845c8309d00c4981828202038204616382830082020382020383008204616382046163";

/// Digest of [`COLUMNAR_MANIFEST_CANONICAL_SHA256`] under the SHA-256 identity profile.
pub const COLUMNAR_MANIFEST_DIGEST_SHA256: &str =
    "sha256:025f614cf1eae3e1eccbc568d51b53c7c6e318f9f5b81b4836f9a0f5b34cc21d";

/// Canonical structured-root columnar manifest bytes under the default BLAKE3 identity profile. Segment
/// payload bytes are committed separately under the structured root's `segments` tree.
pub const COLUMNAR_STRUCTURED_MANIFEST_CANONICAL: &str = "860282826269640182646e616d65030201008286000002015820ffe0658c5e0e153bdadbbfb93096afbe3b6b1209e6289da7d7a39347c1d99f2782830082020182020283018204616182046161860102010158207ed2bba7e2ed441d61eaeebb0e93defbc4689e27787a367fb0bd61f1157b80a682830082020382020383008204616382046163";

/// Digest of [`COLUMNAR_STRUCTURED_MANIFEST_CANONICAL`] under the default identity profile.
pub const COLUMNAR_STRUCTURED_MANIFEST_DIGEST: &str =
    "blake3:fdfa1f27bf0d9b8735036479bed3ced647be811ee7cf4b8295e2d016732ac74e";

/// Canonical structured-root columnar manifest bytes under the SHA-256 identity profile.
pub const COLUMNAR_STRUCTURED_MANIFEST_CANONICAL_SHA256: &str = "860282826269640182646e616d6503020100828600000201582084b5b0c9203ef3771b300e40b4f489df050de2578661ac984cadcfde84c960ed82830082020182020283018204616182046161860102010158206bbfa3df582463dab012a3b7e3746793943336f4f38d209f27a7845c8309d00c82830082020382020383008204616382046163";

/// Digest of [`COLUMNAR_STRUCTURED_MANIFEST_CANONICAL_SHA256`] under the SHA-256 identity profile.
pub const COLUMNAR_STRUCTURED_MANIFEST_DIGEST_SHA256: &str =
    "sha256:acde76f15f3c8b2c3889dd659bce06233b988f3d48e82a4d31bd3a95fb0958c1";

/// Run the columnar manifest vectors. These vectors cover the canonical storage profile directly,
/// while the behavioral columnar suite covers the public facade.
pub fn run_columnar_manifest_vectors() -> Result<()> {
    use loom_core::columnar::{ColumnarManifest, ColumnarSet};
    use loom_core::tabular::{ColumnType, Value};

    let mut set = ColumnarSet::new(
        vec![
            ("id".into(), ColumnType::Int),
            ("name".into(), ColumnType::Text),
        ],
        2,
    )?;
    set.append_row(vec![Value::Int(1), Value::Text("a".into())])?;
    set.append_row(vec![Value::Int(2), Value::Null])?;
    set.append_row(vec![Value::Int(3), Value::Text("c".into())])?;

    let default_bytes = set.encode();
    assert_eq!(
        hex::encode(&default_bytes),
        COLUMNAR_MANIFEST_CANONICAL,
        "default columnar manifest canonical bytes mismatch"
    );
    assert_eq!(
        Digest::hash(Algo::Blake3, &default_bytes),
        Digest::parse(COLUMNAR_MANIFEST_DIGEST)?,
        "default columnar manifest digest mismatch"
    );
    assert_eq!(
        ColumnarSet::decode(&default_bytes)?.rows(),
        3,
        "default columnar manifest must decode"
    );
    let default_structured_manifest = ColumnarManifest {
        segments: set
            .segment_materials_with_algo(Algo::Blake3)
            .into_iter()
            .map(|material| material.manifest)
            .collect(),
        ..set.manifest_with_algo(Algo::Blake3)
    }
    .encode();
    assert_eq!(
        hex::encode(&default_structured_manifest),
        COLUMNAR_STRUCTURED_MANIFEST_CANONICAL,
        "default structured columnar manifest canonical bytes mismatch"
    );
    assert_eq!(
        Digest::hash(Algo::Blake3, &default_structured_manifest),
        Digest::parse(COLUMNAR_STRUCTURED_MANIFEST_DIGEST)?,
        "default structured columnar manifest digest mismatch"
    );
    assert_eq!(
        ColumnarManifest::decode(&default_structured_manifest, Algo::Blake3)?
            .segments
            .len(),
        2,
        "default structured columnar manifest must decode"
    );

    let sha_bytes = set.encode_with_algo(Algo::Sha256);
    assert_eq!(
        hex::encode(&sha_bytes),
        COLUMNAR_MANIFEST_CANONICAL_SHA256,
        "sha256 columnar manifest canonical bytes mismatch"
    );
    assert_eq!(
        Digest::hash(Algo::Sha256, &sha_bytes),
        Digest::parse(COLUMNAR_MANIFEST_DIGEST_SHA256)?,
        "sha256 columnar manifest digest mismatch"
    );
    assert_eq!(
        ColumnarSet::decode_with_algo(&sha_bytes, Algo::Sha256)?.rows(),
        3,
        "sha256 columnar manifest must decode with its profile"
    );
    let sha_structured_manifest = ColumnarManifest {
        segments: set
            .segment_materials_with_algo(Algo::Sha256)
            .into_iter()
            .map(|material| material.manifest)
            .collect(),
        ..set.manifest_with_algo(Algo::Sha256)
    }
    .encode();
    assert_eq!(
        hex::encode(&sha_structured_manifest),
        COLUMNAR_STRUCTURED_MANIFEST_CANONICAL_SHA256,
        "sha256 structured columnar manifest canonical bytes mismatch"
    );
    assert_eq!(
        Digest::hash(Algo::Sha256, &sha_structured_manifest),
        Digest::parse(COLUMNAR_STRUCTURED_MANIFEST_DIGEST_SHA256)?,
        "sha256 structured columnar manifest digest mismatch"
    );
    assert_eq!(
        ColumnarManifest::decode(&sha_structured_manifest, Algo::Sha256)?
            .segments
            .len(),
        2,
        "sha256 structured columnar manifest must decode with its profile"
    );

    let mut tampered = default_bytes.clone();
    let last = tampered
        .last_mut()
        .ok_or_else(|| loom_core::LoomError::corrupt("empty columnar manifest vector"))?;
    *last ^= 0x01;
    assert_eq!(
        ColumnarSet::decode(&tampered).unwrap_err().code,
        loom_core::Code::CorruptObject,
        "tampered columnar manifest must fail canonical validation"
    );
    Ok(())
}

pub fn run_kv_map_vectors() -> Result<()> {
    use loom_core::workspace::facet_path;
    use loom_core::{
        FacetKind, KvMap, Loom, MemoryStore, ObjectStore, Value, WorkspaceId, get_kv, kv_put,
        replace_kv_map,
    };

    let mut loom = Loom::new(MemoryStore::new());
    let ns =
        loom.registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([0x6b; 16]))?;
    kv_put(
        &mut loom,
        ns,
        "vector",
        Value::Text("a".into()),
        b"one".to_vec(),
    )?;
    kv_put(
        &mut loom,
        ns,
        "vector",
        Value::Text("b".into()),
        b"two".to_vec(),
    )?;
    let root_bytes = loom.read_file_reserved(ns, &facet_path(FacetKind::Kv, "vector"))?;
    let loom_codec::Value::Array(root_fields) =
        loom_codec::decode(&root_bytes).map_err(codec_contract_error)?
    else {
        return Err(loom_core::LoomError::corrupt(
            "KV root vector is not an array",
        ));
    };
    if root_fields.len() != 4
        || !matches!(
            &root_fields[0],
            loom_codec::Value::Text(schema) if schema == "loom.kv.prolly-map-root.v1"
        )
        || !matches!(root_fields[1], loom_codec::Value::Uint(0x1e))
    {
        return Err(loom_core::LoomError::corrupt(
            "KV structured root vector header changed",
        ));
    }
    let loom_codec::Value::Bytes(entries_root) = &root_fields[2] else {
        return Err(loom_core::LoomError::corrupt(
            "KV structured root entries are not a prolly root",
        ));
    };
    let loom_codec::Value::Bytes(anchors_root) = &root_fields[3] else {
        return Err(loom_core::LoomError::corrupt(
            "KV structured root anchors are not a prolly root",
        ));
    };
    let entries_root = Digest::of(
        Algo::Blake3,
        entries_root.as_slice().try_into().map_err(|_| {
            loom_core::LoomError::corrupt("KV entry prolly root digest length changed")
        })?,
    );
    let anchors_root = Digest::of(
        Algo::Blake3,
        anchors_root.as_slice().try_into().map_err(|_| {
            loom_core::LoomError::corrupt("KV anchor prolly root digest length changed")
        })?,
    );
    let entries = loom_core::prolly::entries(loom.store(), &entries_root)?;
    let anchors = loom_core::prolly::entries(loom.store(), &anchors_root)?;
    if entries.len() != 2 || anchors.len() != 2 {
        return Err(loom_core::LoomError::corrupt(
            "KV structured root vector cardinality changed",
        ));
    }
    assert_eq!(hex::encode(&entries[0].0), "04610000");
    assert_eq!(hex::encode(&entries[1].0), "04620000");
    let entry_fields = match loom_codec::decode(&entries[0].1).map_err(codec_contract_error)? {
        loom_codec::Value::Array(fields) => fields,
        _ => {
            return Err(loom_core::LoomError::corrupt(
                "KV entry vector shape changed",
            ));
        }
    };
    if entry_fields.len() != 3 {
        return Err(loom_core::LoomError::corrupt(
            "KV entry vector field count changed",
        ));
    }
    let anchor_fields = match loom_codec::decode(&anchors[0].1).map_err(codec_contract_error)? {
        loom_codec::Value::Array(fields) => fields,
        _ => {
            return Err(loom_core::LoomError::corrupt(
                "KV anchor vector shape changed",
            ));
        }
    };
    if anchor_fields.len() != 2 {
        return Err(loom_core::LoomError::corrupt(
            "KV anchor vector field count changed",
        ));
    }
    let encoded = get_kv(&loom, ns, "vector")?.encode();
    assert_eq!(
        hex::encode(&encoded),
        "8302828282046161436f6e6582820461624374776f82828204616101828204616201"
    );
    assert_eq!(
        Digest::hash(Algo::Blake3, &encoded).to_string(),
        "blake3:8e3787fafaa46b2c5df84c93c247cbcc83805c561b845bea95da8344c928daf5"
    );
    assert_eq!(
        Digest::hash(Algo::Sha256, &encoded).to_string(),
        "sha256:b3f2f4fa769b38b910e7e29de154a2f676057f707815a524f3ba169a36989076"
    );

    for malformed in [
        "8302818282046161436f6e6580",
        "8302818282046161436f6e6581",
        "8302828282046161436f6e658282046161436f6e6580",
        "8302818282046161436f6e65828282046161018204616101",
        "8302818282046161436f6e65818204616100",
    ] {
        assert!(KvMap::decode(&hex::decode(malformed).expect("valid vector hex")).is_err());
    }
    for (legacy, expected) in [
        (
            "818282046161436f6e65",
            "8302818282046161436f6e6581828204616101",
        ),
        (
            "830107818282046161436f6e65",
            "8302818282046161436f6e6581828204616101",
        ),
    ] {
        let map =
            KvMap::decode(&hex::decode(legacy).expect("valid vector hex")).map_err(|error| {
                loom_core::LoomError::new(error.code, format!("{legacy}: {}", error.message))
            })?;
        let mut promotion_loom = Loom::new(MemoryStore::new());
        let promotion_ns = promotion_loom.registry_mut().create(
            FacetKind::Kv,
            None,
            WorkspaceId::from_bytes([0x6c; 16]),
        )?;
        replace_kv_map(&mut promotion_loom, promotion_ns, "promotion", &map)?;
        assert_eq!(
            hex::encode(get_kv(&promotion_loom, promotion_ns, "promotion")?.encode()),
            expected
        );
    }

    let mut bad = Loom::new(MemoryStore::new());
    let bad_ns =
        bad.registry_mut()
            .create(FacetKind::Kv, None, WorkspaceId::from_bytes([0x6d; 16]))?;
    bad.create_directory_reserved(bad_ns, &facet_path(FacetKind::Kv, ""), true)?;
    let key = Value::Text("a".into());
    let key_cell =
        loom_codec::decode(&loom_core::key_to_cbor(&key)).map_err(codec_contract_error)?;
    let key_bytes = vec![0x04, b'a', 0x00, 0x00];
    let digest = Digest::hash(Algo::Blake3, b"one");
    let value_dir = facet_path(FacetKind::Kv, &format!(".values/{}", hex::encode(b"bad")));
    bad.create_directory_reserved(bad_ns, &value_dir, true)?;
    bad.write_file_reserved(
        bad_ns,
        &facet_path(
            FacetKind::Kv,
            &format!(".values/{}/{}", hex::encode(b"bad"), digest.to_hex()),
        ),
        b"one",
        0o100644,
    )?;

    let root_value = |entries: loom_codec::Value, anchors: loom_codec::Value| {
        loom_codec::Value::Array(vec![
            loom_codec::Value::Text("loom.kv.prolly-map-root.v1".to_string()),
            loom_codec::Value::Uint(0x1e),
            entries,
            anchors,
        ])
    };
    let leaf = |pairs: Vec<(Vec<u8>, Vec<u8>)>| {
        loom_codec::Value::Array(vec![
            loom_codec::Value::Uint(0),
            loom_codec::Value::Array(
                pairs
                    .into_iter()
                    .map(|(key, value)| {
                        loom_codec::Value::Array(vec![
                            loom_codec::Value::Bytes(key),
                            loom_codec::Value::Bytes(value),
                        ])
                    })
                    .collect(),
            ),
        ])
    };
    let entry_record = loom_codec::encode(&loom_codec::Value::Array(vec![
        key_cell.clone(),
        loom_codec::Value::Bytes(digest.bytes().to_vec()),
        loom_codec::Value::Uint(3),
    ]))
    .map_err(codec_contract_error)?;
    let anchor_record = |anchor| {
        loom_codec::encode(&loom_codec::Value::Array(vec![
            key_cell.clone(),
            loom_codec::Value::Uint(anchor),
        ]))
        .map_err(codec_contract_error)
    };

    for root in [
        loom_codec::Value::Array(vec![loom_codec::Value::Text(
            "loom.kv.prolly-map-root.v1".to_string(),
        )]),
        loom_codec::Value::Array(vec![
            loom_codec::Value::Text("loom.kv.prolly-map-root.v2".to_string()),
            loom_codec::Value::Uint(0x1e),
            loom_codec::Value::Null,
            loom_codec::Value::Null,
        ]),
        loom_codec::Value::Array(vec![
            loom_codec::Value::Text("loom.kv.prolly-map-root.v1".to_string()),
            loom_codec::Value::Uint(0x1e),
            loom_codec::Value::Bytes(vec![1, 2, 3]),
            loom_codec::Value::Null,
        ]),
        loom_codec::Value::Array(vec![
            loom_codec::Value::Text("loom.kv.prolly-map-root.v1".to_string()),
            loom_codec::Value::Uint(0x1e),
            loom_codec::Value::Null,
            loom_codec::Value::Bytes(vec![1, 2, 3]),
        ]),
        loom_codec::Value::Array(vec![
            loom_codec::Value::Text("loom.kv.prolly-map-root.v1".to_string()),
            loom_codec::Value::Uint(0x1e),
            loom_codec::Value::Null,
            loom_codec::Value::Null,
            loom_codec::Value::Text("oversized-key-policy".to_string()),
        ]),
    ] {
        write_kv_vector_root(&mut bad, bad_ns, root)?;
        assert!(get_kv(&bad, bad_ns, "bad").is_err());
    }

    let unsorted_entries = bad.store_mut().put(
        &loom_codec::encode(&leaf(vec![
            (vec![0x04, b'b', 0x00, 0x00], entry_record.clone()),
            (key_bytes.clone(), entry_record.clone()),
        ]))
        .map_err(codec_contract_error)?,
    )?;
    write_kv_vector_root(
        &mut bad,
        bad_ns,
        root_value(
            loom_codec::Value::Bytes(unsorted_entries.bytes().to_vec()),
            loom_codec::Value::Null,
        ),
    )?;
    assert!(get_kv(&bad, bad_ns, "bad").is_err());

    let duplicate_anchor = bad.store_mut().put(
        &loom_codec::encode(&leaf(vec![
            (key_bytes.clone(), anchor_record(1)?),
            (key_bytes.clone(), anchor_record(2)?),
        ]))
        .map_err(codec_contract_error)?,
    )?;
    write_kv_vector_root(
        &mut bad,
        bad_ns,
        root_value(
            loom_codec::Value::Null,
            loom_codec::Value::Bytes(duplicate_anchor.bytes().to_vec()),
        ),
    )?;
    assert!(get_kv(&bad, bad_ns, "bad").is_err());

    let invalid_anchor = bad.store_mut().put(
        &loom_codec::encode(&leaf(vec![(key_bytes.clone(), anchor_record(0)?)]))
            .map_err(codec_contract_error)?,
    )?;
    write_kv_vector_root(
        &mut bad,
        bad_ns,
        root_value(
            loom_codec::Value::Null,
            loom_codec::Value::Bytes(invalid_anchor.bytes().to_vec()),
        ),
    )?;
    assert!(get_kv(&bad, bad_ns, "bad").is_err());

    let missing_anchor_entry = bad.store_mut().put(
        &loom_codec::encode(&leaf(vec![(key_bytes, entry_record)]))
            .map_err(codec_contract_error)?,
    )?;
    write_kv_vector_root(
        &mut bad,
        bad_ns,
        root_value(
            loom_codec::Value::Bytes(missing_anchor_entry.bytes().to_vec()),
            loom_codec::Value::Null,
        ),
    )?;
    assert!(get_kv(&bad, bad_ns, "bad").is_err());
    Ok(())
}

fn write_kv_vector_root(
    loom: &mut Loom<MemoryStore>,
    ns: WorkspaceId,
    root: loom_codec::Value,
) -> Result<()> {
    loom.write_file_reserved(
        ns,
        &loom_core::workspace::facet_path(FacetKind::Kv, "bad"),
        &loom_codec::encode(&root).map_err(codec_contract_error)?,
        0o100644,
    )
}

pub struct LockFenceVector {
    pub name: &'static str,
    pub authority: u32,
    pub epoch: u32,
    pub sequence: u64,
    pub expect_low: u64,
    pub expect_high: u64,
    pub expect_decimal: &'static str,
}

pub const LOCK_FENCE_VECTORS: &[LockFenceVector] = &[
    LockFenceVector {
        name: "embedded-sequence",
        authority: 0,
        epoch: 0,
        sequence: 42,
        expect_low: 42,
        expect_high: 0,
        expect_decimal: "42",
    },
    LockFenceVector {
        name: "external-authority-epoch",
        authority: 7,
        epoch: 3,
        sequence: 42,
        expect_low: 42,
        expect_high: 30064771075,
        expect_decimal: "554597137655190595375936307242",
    },
];

pub fn run_lock_fence_vectors() -> Result<()> {
    for vector in LOCK_FENCE_VECTORS {
        let fence = Fence::new(vector.authority, vector.epoch, vector.sequence);
        let (low, high) = fence.to_limbs();
        assert_eq!(low, vector.expect_low, "lock fence low limb mismatch");
        assert_eq!(high, vector.expect_high, "lock fence high limb mismatch");
        assert_eq!(
            fence.to_u128().to_string(),
            vector.expect_decimal,
            "lock fence packed value mismatch"
        );
        assert_eq!(
            Fence::from_limbs(low, high),
            fence,
            "lock fence limb round-trip mismatch"
        );
    }
    Ok(())
}

// ---- exec manifest identity ---------------------------------------------------------------------

/// One positive canonical `exec` manifest vector. The vector pins the public program identity bytes
/// emitted by `loom-compute::Manifest::encode`.
#[derive(Debug, Clone, Copy)]
pub struct ExecManifestVector {
    /// Human-readable name for test output.
    pub name: &'static str,
    /// Expected canonical manifest bytes, hex.
    pub expect_canonical: &'static str,
}

/// One negative `exec` manifest decode vector. The bytes must be rejected by `Manifest::decode`.
#[derive(Debug, Clone, Copy)]
pub struct ExecManifestNegativeVector {
    /// Human-readable name for test output.
    pub name: &'static str,
    /// Malformed or policy-invalid manifest bytes, hex.
    pub canonical: &'static str,
}

/// Minimal manifest: empty name/engine/entry, ABI 0, zero body digest, no schemas, no guards, and a
/// single `Files` read grant over `All`.
pub const EXEC_MANIFEST_VECTORS: &[ExecManifestVector] = &[ExecManifestVector {
    name: "minimal-files-read",
    expect_canonical: "8c0101016060006058200000000000000000000000000000000000000000000000000000000000000000f6f68183000081810080",
}];

/// Negative vectors for the same manifest envelope.
pub const EXEC_MANIFEST_NEGATIVE_VECTORS: &[ExecManifestNegativeVector] = &[
    ExecManifestNegativeVector {
        name: "unknown-schema-version",
        canonical: "8c0101026060006058200000000000000000000000000000000000000000000000000000000000000000f6f68183000081810080",
    },
    ExecManifestNegativeVector {
        name: "non-grantable-program-facet",
        canonical: "8c0101016060006058200000000000000000000000000000000000000000000000000000000000000000f6f681830e0081810080",
    },
    ExecManifestNegativeVector {
        name: "trailing-byte",
        canonical: "8c0101016060006058200000000000000000000000000000000000000000000000000000000000000000f6f6818300008181008000",
    },
];

fn minimal_exec_manifest() -> Manifest {
    Manifest {
        name: String::new(),
        engine: String::new(),
        abi_version: 0,
        entry: String::new(),
        grants: GrantSet::new(vec![Grant {
            facet: Capability::Files,
            mode: Mode::Read,
            scopes: vec![Scope::All],
        }]),
        input_schema: None,
        output_schema: None,
        body: Digest::from_blake3_bytes([0u8; 32]),
        guards: Vec::new(),
    }
}

/// Run the canonical `exec` manifest vectors.
pub fn run_exec_manifest_vectors() -> Result<()> {
    let manifest = minimal_exec_manifest();
    for vector in EXEC_MANIFEST_VECTORS {
        let bytes = manifest.encode();
        assert_eq!(
            hex::encode(&bytes),
            vector.expect_canonical,
            "exec manifest canonical bytes mismatch for vector '{}'",
            vector.name
        );
        assert_eq!(
            Manifest::decode(&bytes),
            Some(manifest.clone()),
            "exec manifest vector '{}' must decode",
            vector.name
        );
    }
    for vector in EXEC_MANIFEST_NEGATIVE_VECTORS {
        let bytes = hex::decode(vector.canonical).map_err(|e| {
            loom_core::LoomError::invalid(format!(
                "bad exec manifest negative vector {}: {e}",
                vector.name
            ))
        })?;
        assert!(
            Manifest::decode(&bytes).is_none(),
            "exec manifest negative vector '{}' must be rejected",
            vector.name
        );
    }
    Ok(())
}

/// One pinned Loom Templates vector.
#[derive(Debug, Clone, Copy)]
pub struct TemplateVector {
    /// Human-readable name for test output.
    pub name: &'static str,
    /// Logical template path supplied to the processor.
    pub source_path: &'static str,
    /// Template source.
    pub source: &'static str,
    /// Expected source digest in `algo:hex` form. Empty for error vectors.
    pub expect_source_digest: &'static str,
    /// Expected processing-plan digest in `algo:hex` form. Empty for error vectors.
    pub expect_ast_digest: &'static str,
    /// Expected static template dependencies.
    pub expect_dependencies: &'static [TemplateDependencyVector],
    /// Expected Loom host calls.
    pub expect_host_calls: &'static [TemplateHostCallVector],
    /// Expected diagnostic codes.
    pub expect_diagnostics: &'static [TemplateDiagnosticVector],
    /// Expected processor error, if this is a negative vector.
    pub expect_error: Option<TemplateErrorVector>,
}

/// One expected static template dependency.
#[derive(Debug, Clone, Copy)]
pub struct TemplateDependencyVector {
    pub kind: TemplateDependencyKind,
    pub target: &'static str,
}

/// One expected Loom host call.
#[derive(Debug, Clone, Copy)]
pub struct TemplateHostCallVector {
    pub kind: HostCallKind,
    pub target: &'static str,
}

/// One expected diagnostic.
#[derive(Debug, Clone, Copy)]
pub struct TemplateDiagnosticVector {
    pub severity: DiagnosticSeverity,
    pub code: &'static str,
}

/// Expected template processor error class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateErrorVector {
    Parse,
    UnknownLoomCall { name: &'static str },
    InvalidProgramCall,
}

const TEMPLATE_CONTROL_DEPS: &[TemplateDependencyVector] = &[
    TemplateDependencyVector {
        kind: TemplateDependencyKind::Include,
        target: "footer.html",
    },
    TemplateDependencyVector {
        kind: TemplateDependencyKind::Extends,
        target: "layout.html",
    },
    TemplateDependencyVector {
        kind: TemplateDependencyKind::Import,
        target: "forms.html",
    },
    TemplateDependencyVector {
        kind: TemplateDependencyKind::FromImport,
        target: "macros.html",
    },
];

const TEMPLATE_PROGRAM_CALLS: &[TemplateHostCallVector] = &[
    TemplateHostCallVector {
        kind: HostCallKind::Program,
        target: "dashboard/load",
    },
    TemplateHostCallVector {
        kind: HostCallKind::Program,
        target: "nav/load",
    },
];

const TEMPLATE_DYNAMIC_REFERENCE_DIAGNOSTICS: &[TemplateDiagnosticVector] =
    &[TemplateDiagnosticVector {
        severity: DiagnosticSeverity::Warning,
        code: "dynamic-template-reference",
    }];

/// Pinned Loom Templates vectors. Keep these stable across implementations of the Loom template
/// processor API.
pub const TEMPLATE_VECTORS: &[TemplateVector] = &[
    TemplateVector {
        name: "control-flow-static-dependencies",
        source_path: "app/index.html",
        source: "{% extends \"layout.html\" %}\n{% import \"forms.html\" as forms %}\n{% from \"macros.html\" import badge as badge %}\n{% for item in items -%}\n{% if item.enabled %}{{ item.name }}{% endif %}\n{%- endfor %}\n{% include \"footer.html\" %}\n",
        expect_source_digest: "blake3:765580de9aba913ea853ac34ccc5d5ebf06f7b688f997724de798a969b6f2d82",
        expect_ast_digest: "blake3:bd195dff92c02e4d685f540f401697f99fe63f796defb5c800ea16dcef0344fa",
        expect_dependencies: TEMPLATE_CONTROL_DEPS,
        expect_host_calls: &[],
        expect_diagnostics: &[],
        expect_error: None,
    },
    TemplateVector {
        name: "loom-program-host-calls",
        source_path: "app/dashboard.html",
        source: "{{ loom.program(name=\"dashboard/load\") }}\n{{ loom.program(\"nav/load\") }}\n",
        expect_source_digest: "blake3:9bfd6202ce1dc35ed3943923c8c04bd4f0895653acabe9668af0231e166335d4",
        expect_ast_digest: "blake3:5e07ea24d55b9ce39469e8c2b4a4b2db5eef0d7fe3cf7d07044069b4428b35ff",
        expect_dependencies: &[],
        expect_host_calls: TEMPLATE_PROGRAM_CALLS,
        expect_diagnostics: &[],
        expect_error: None,
    },
    TemplateVector {
        name: "dynamic-include-diagnostic",
        source_path: "app/dynamic.html",
        source: "{% include selected_template %}\n",
        expect_source_digest: "blake3:f138c523bed290f186dc297f6cb2c2e3105096245fbfdbbae12489bc8a3c97b8",
        expect_ast_digest: "blake3:dd9fc1b5905ecfae37247aba61113cedcb07744cdaf7fa7b03cdce75f751e171",
        expect_dependencies: &[],
        expect_host_calls: &[],
        expect_diagnostics: TEMPLATE_DYNAMIC_REFERENCE_DIAGNOSTICS,
        expect_error: None,
    },
    TemplateVector {
        name: "unknown-loom-call-rejected",
        source_path: "app/bad-call.html",
        source: "{{ loom.tool(name=\"x\") }}\n",
        expect_source_digest: "",
        expect_ast_digest: "",
        expect_dependencies: &[],
        expect_host_calls: &[],
        expect_diagnostics: &[],
        expect_error: Some(TemplateErrorVector::UnknownLoomCall { name: "loom.tool" }),
    },
    TemplateVector {
        name: "dynamic-program-name-rejected",
        source_path: "app/dynamic-program.html",
        source: "{{ loom.program(name=program_name) }}\n",
        expect_source_digest: "",
        expect_ast_digest: "",
        expect_dependencies: &[],
        expect_host_calls: &[],
        expect_diagnostics: &[],
        expect_error: Some(TemplateErrorVector::InvalidProgramCall),
    },
    TemplateVector {
        name: "parse-error-rejected",
        source_path: "app/malformed.html",
        source: "{% if %}\n",
        expect_source_digest: "",
        expect_ast_digest: "",
        expect_dependencies: &[],
        expect_host_calls: &[],
        expect_diagnostics: &[],
        expect_error: Some(TemplateErrorVector::Parse),
    },
];

/// Expected cache key for a template plan with deliberately unsorted policy inputs.
pub const TEMPLATE_CACHE_KEY_VECTOR: &str =
    "blake3:401bd4db828b700c00ee9d6b18fd75b34c6133df2e4a2418af27e79f876aaa6f";

/// Run the pinned Loom Templates vectors against [`TemplateProcessor`].
pub fn run_template_vectors() -> Result<()> {
    let processor = TemplateProcessor::new();

    for vector in TEMPLATE_VECTORS {
        let result = processor.process(vector.source_path, vector.source);
        match vector.expect_error {
            Some(expect_error) => {
                let err = result.unwrap_err();
                assert_template_error(vector.name, expect_error, &err);
            }
            None => {
                let plan = result.unwrap_or_else(|err| {
                    panic!(
                        "template vector '{}' unexpectedly failed: {err}",
                        vector.name
                    )
                });
                assert_eq!(
                    plan.source_digest.to_string(),
                    vector.expect_source_digest,
                    "template source digest mismatch for vector '{}'",
                    vector.name
                );
                assert_eq!(
                    plan.ast_digest.to_string(),
                    vector.expect_ast_digest,
                    "template plan digest mismatch for vector '{}'",
                    vector.name
                );

                let dependencies = plan
                    .dependencies
                    .iter()
                    .map(|dependency| (dependency.kind, dependency.target.clone()))
                    .collect::<Vec<_>>();
                assert_eq!(
                    dependencies.len(),
                    vector.expect_dependencies.len(),
                    "template dependency count mismatch for vector '{}'",
                    vector.name
                );
                for (actual, expected) in dependencies.iter().zip(vector.expect_dependencies) {
                    assert_eq!(
                        (actual.0, actual.1.as_str()),
                        (expected.kind, expected.target),
                        "template dependency mismatch for vector '{}'",
                        vector.name
                    );
                }

                let host_calls = plan
                    .host_calls
                    .iter()
                    .map(|call| (call.kind, call.target.clone()))
                    .collect::<Vec<_>>();
                assert_eq!(
                    host_calls.len(),
                    vector.expect_host_calls.len(),
                    "template host-call count mismatch for vector '{}'",
                    vector.name
                );
                for (actual, expected) in host_calls.iter().zip(vector.expect_host_calls) {
                    assert_eq!(
                        (actual.0, actual.1.as_str()),
                        (expected.kind, expected.target),
                        "template host-call mismatch for vector '{}'",
                        vector.name
                    );
                }

                let diagnostics = plan
                    .diagnostics
                    .iter()
                    .map(|diagnostic| TemplateDiagnosticVector {
                        severity: diagnostic.severity,
                        code: diagnostic.code,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    diagnostics.len(),
                    vector.expect_diagnostics.len(),
                    "template diagnostic count mismatch for vector '{}'",
                    vector.name
                );
                for (actual, expected) in diagnostics.iter().zip(vector.expect_diagnostics) {
                    assert_eq!(
                        (actual.severity, actual.code),
                        (expected.severity, expected.code),
                        "template diagnostic mismatch for vector '{}'",
                        vector.name
                    );
                }
            }
        }
    }

    run_template_cache_key_vector()
}

fn run_template_cache_key_vector() -> Result<()> {
    let program_plan = TemplateProcessor::new()
        .process("app/dashboard.html", TEMPLATE_VECTORS[1].source)
        .expect("program vector must compile");
    let mut input = TemplateCacheInput::from_plan(&program_plan, TemplateConsumer::App);
    input.metadata_digest = Some(Digest::parse(
        "blake3:1111111111111111111111111111111111111111111111111111111111111111",
    )?);
    input.grants_profile_digest = Some(Digest::parse(
        "blake3:2222222222222222222222222222222222222222222222222222222222222222",
    )?);
    input.program_bindings = vec![
        ProgramBinding {
            name: "nav/load".to_string(),
            manifest_digest: Digest::parse(
                "blake3:4444444444444444444444444444444444444444444444444444444444444444",
            )?,
        },
        ProgramBinding {
            name: "dashboard/load".to_string(),
            manifest_digest: Digest::parse(
                "blake3:3333333333333333333333333333333333333333333333333333333333333333",
            )?,
        },
    ];
    input.render_options = vec![
        RenderOption {
            key: "locale".to_string(),
            value: "en-US".to_string(),
        },
        RenderOption {
            key: "theme".to_string(),
            value: "system".to_string(),
        },
    ];

    assert_eq!(
        input.cache_key().digest.to_string(),
        TEMPLATE_CACHE_KEY_VECTOR,
        "template cache-key vector mismatch"
    );
    Ok(())
}

fn assert_template_error(name: &str, expected: TemplateErrorVector, actual: &TemplateError) {
    match (expected, actual) {
        (TemplateErrorVector::Parse, TemplateError::Parse { .. })
        | (TemplateErrorVector::InvalidProgramCall, TemplateError::InvalidProgramCall) => {}
        (
            TemplateErrorVector::UnknownLoomCall {
                name: expected_name,
            },
            TemplateError::UnknownLoomCall { name },
        ) if name == expected_name => {}
        _ => panic!("template vector '{name}' returned unexpected error: {actual}"),
    }
}

pub fn run_interchange_vectors() -> Result<()> {
    let mut report = ImportReport::new(ImportReportInput {
        profile: "granola-api",
        source_scope: "organization:alpha",
        commit: Some(Digest::hash(Algo::Blake3, b"commit")),
        objects_added: 7,
        bytes_in: 1024,
        bytes_stored: 512,
        rows_imported: 3,
        skipped: 1,
        operations_planned: 5,
        operations_applied: 4,
        dry_run: true,
    })?;
    report.warnings.push("missing transcript".to_string());
    report.fidelity_issues.push(FidelityIssue::new(
        FidelitySeverity::Warning,
        "meeting:1",
        "transcript",
        "source omitted transcript",
    )?);
    let encoded_report = report.encode()?;
    assert_eq!(
        ImportReport::decode(&encoded_report)?.encode()?,
        encoded_report,
        "interchange import report canonical round-trip mismatch"
    );

    let mut batch = ImportBatch::new(
        "jira",
        "jira-cloud",
        "site:example",
        1_700_000_000_000,
        Coverage::Partial,
    )?;
    batch.items.push(ImportBatchItem::new(
        "ticket:1",
        Digest::hash(Algo::Blake3, b"one"),
    )?);
    batch.items.push(ImportBatchItem::new(
        "ticket:2",
        Digest::hash(Algo::Blake3, b"two"),
    )?);
    let encoded_batch = batch.encode()?;
    assert_eq!(
        ImportBatch::decode(&encoded_batch)?.encode()?,
        encoded_batch,
        "interchange import batch canonical round-trip mismatch"
    );

    let mut duplicate = batch.clone();
    duplicate.items.push(ImportBatchItem::new(
        "ticket:1",
        Digest::hash(Algo::Blake3, b"duplicate"),
    )?);
    assert!(
        duplicate.encode().is_err(),
        "interchange import batch must reject duplicate source entity ids"
    );

    let mut checkpoint = ImportCheckpoint::new(
        "checkpoint:1",
        "granola-api",
        "organization:alpha",
        b"cursor-1",
    )?;
    checkpoint.observed_ids.push("meeting:1".to_string());
    checkpoint.completed_units.push("page:1".to_string());
    checkpoint.coverage_gaps.push("rate limited".to_string());
    checkpoint
        .retry_windows
        .push("after:1700000000000".to_string());
    checkpoint.profile_state_digest = Some(Digest::hash(Algo::Blake3, b"profile-state"));
    let encoded_checkpoint = checkpoint.encode()?;
    assert_eq!(
        ImportCheckpoint::decode(&encoded_checkpoint)?.encode()?,
        encoded_checkpoint,
        "interchange checkpoint canonical round-trip mismatch"
    );

    let mut archive = ArchiveManifest::new(
        "archive:1",
        ArchiveKind::Zip,
        Digest::hash(Algo::Blake3, b"archive"),
    )?;
    archive
        .entries
        .push(ArchiveEntry::new("notes", ArchiveEntryKind::Directory, 0)?);
    archive
        .entries
        .push(ArchiveEntry::new("notes/a.md", ArchiveEntryKind::File, 42)?);
    archive.entries[1].digest = Some(Digest::hash(Algo::Blake3, b"archive-file"));
    let mut symlink = ArchiveEntry::new("notes/latest", ArchiveEntryKind::Symlink, 0)?;
    symlink.link_target = Some("notes/a.md".to_string());
    archive.entries.push(symlink);
    let encoded_archive = archive.encode()?;
    assert_eq!(
        ArchiveManifest::decode(&encoded_archive)?.encode()?,
        encoded_archive,
        "interchange archive manifest canonical round-trip mismatch"
    );
    assert!(
        ArchiveEntry::new("/tmp/file", ArchiveEntryKind::File, 1).is_err(),
        "interchange archive entry must reject absolute paths"
    );
    assert!(
        ArchiveEntry::new("a/../file", ArchiveEntryKind::File, 1).is_err(),
        "interchange archive entry must reject parent escapes"
    );
    let mut duplicate_archive = archive.clone();
    duplicate_archive
        .entries
        .push(ArchiveEntry::new("notes/a.md", ArchiveEntryKind::File, 1)?);
    assert!(
        duplicate_archive.encode().is_err(),
        "interchange archive manifest must reject duplicate paths"
    );

    let mut redmine = ProfileImportPlan::redmine("redmine:site", 1_700_000_000_001)?;
    redmine.actions.push(ProfileImportAction::new(
        "ticket:9",
        Digest::hash(Algo::Blake3, b"redmine ticket"),
        TargetProfile::Tickets,
        ProfileImportActionKind::Ticket,
    )?);
    redmine.actions.push(ProfileImportAction::new(
        "wiki:Home",
        Digest::hash(Algo::Blake3, b"redmine wiki"),
        TargetProfile::Pages,
        ProfileImportActionKind::PageBodyReplace,
    )?);
    let encoded_redmine = redmine.encode()?;
    assert_eq!(
        ProfileImportPlan::decode(&encoded_redmine)?.encode()?,
        encoded_redmine,
        "redmine profile import plan canonical round-trip mismatch"
    );

    let import_sources = [
        (
            ProfileImportPlan::jira("jira:cloud", 10)?,
            SourceSystem::Jira,
            TargetProfile::Tickets,
            ProfileImportActionKind::Ticket,
        ),
        (
            ProfileImportPlan::confluence_storage("space:ENG", 10)?,
            SourceSystem::ConfluenceStorage,
            TargetProfile::Pages,
            ProfileImportActionKind::PageBodyReplace,
        ),
        (
            ProfileImportPlan::confluence_adf("space:ENG", 10)?,
            SourceSystem::ConfluenceAdf,
            TargetProfile::Pages,
            ProfileImportActionKind::PageBodyReplace,
        ),
        (
            ProfileImportPlan::markdown("vault:docs", 10)?,
            SourceSystem::Markdown,
            TargetProfile::Pages,
            ProfileImportActionKind::PageBodyReplace,
        ),
        (
            ProfileImportPlan::notion("organization:notion", 10)?,
            SourceSystem::Notion,
            TargetProfile::Pages,
            ProfileImportActionKind::PageBodyReplace,
        ),
        (
            ProfileImportPlan::asana("organization:asana", 10)?,
            SourceSystem::Asana,
            TargetProfile::Tickets,
            ProfileImportActionKind::Ticket,
        ),
        (
            ProfileImportPlan::slack("organization:slack", 10)?,
            SourceSystem::Slack,
            TargetProfile::Chat,
            ProfileImportActionKind::SourceSidecar,
        ),
        (
            ProfileImportPlan::drive("drive:shared", 10)?,
            SourceSystem::Drive,
            TargetProfile::Drive,
            ProfileImportActionKind::SourceSidecar,
        ),
        (
            ProfileImportPlan::granola_api("organization:granola-api", 10)?,
            SourceSystem::GranolaApi,
            TargetProfile::Meetings,
            ProfileImportActionKind::SourceSidecar,
        ),
        (
            ProfileImportPlan::granola_app("organization:granola-app", 10)?,
            SourceSystem::GranolaApp,
            TargetProfile::Meetings,
            ProfileImportActionKind::SourceSidecar,
        ),
        (
            ProfileImportPlan::granola_mcp("organization:granola-mcp", 10)?,
            SourceSystem::GranolaMcp,
            TargetProfile::Meetings,
            ProfileImportActionKind::SourceSidecar,
        ),
    ];
    for (mut plan, source_system, target_profile, action_kind) in import_sources {
        plan.actions.push(ProfileImportAction::new(
            "source:1",
            Digest::hash(Algo::Blake3, plan.profile.as_bytes()),
            target_profile,
            action_kind,
        )?);
        let encoded = plan.encode()?;
        let decoded = ProfileImportPlan::decode(&encoded)?;
        assert_eq!(decoded.source_system, source_system);
        assert_eq!(
            decoded.encode()?,
            encoded,
            "profile import plan canonical round-trip mismatch for {}",
            source_system.profile_name()
        );
    }

    let csv_plan =
        ProfileImportPlan::new(SourceSystem::Csv, "file:source.csv", 10, Coverage::Complete)?;
    let encoded_csv = csv_plan.encode()?;
    let decoded_csv = ProfileImportPlan::decode(&encoded_csv)?;
    assert_eq!(decoded_csv.source_system, SourceSystem::Csv);
    assert_eq!(
        decoded_csv.encode()?,
        encoded_csv,
        "csv profile import plan canonical round-trip mismatch"
    );

    let mut duplicate_action = ProfileImportPlan::jira("jira:site", 20)?;
    let action = ProfileImportAction::new(
        "ticket:LOOM-1",
        Digest::hash(Algo::Blake3, b"ticket"),
        TargetProfile::Tickets,
        ProfileImportActionKind::Ticket,
    )?;
    duplicate_action.actions.push(action.clone());
    duplicate_action.actions.push(action);
    assert!(
        duplicate_action.encode().is_err(),
        "profile import plan must reject duplicate planned actions"
    );

    Ok(())
}

pub fn run_substrate_model_vectors() -> Result<()> {
    struct Hooks;
    impl SequencerHooks for Hooks {}

    fn digest(value: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, value)
    }

    fn actor(byte: u8) -> loom_core::WorkspaceId {
        loom_core::WorkspaceId::from_bytes([byte; 16])
    }

    let mut agent = loom_substrate::AgentIdentity::new("agent-1", "model/runtime")?;
    agent.source_entity_ids = vec!["ticket:LOOM-1".to_string()];
    agent.tool_calls = vec!["substrate_search".to_string()];
    agent.policy_labels = vec!["restricted".to_string(), "restricted".to_string()];
    agent.trace_digest = Some(digest(b"trace"));
    let envelope = OperationEnvelope::new(
        Algo::Blake3,
        OperationEnvelopeInput {
            workspace_id: "organization",
            app_id: "tickets",
            scope_id: "project",
            operation_id: "op-1",
            operation_kind: "ticket.updated",
            sequence: 7,
            actor_principal: actor(1),
            actor_kind: ActorKind::Agent,
            timestamp_ms: 100,
            idempotency_key: "idem-1",
            base_root: digest(b"root"),
            base_entity_version: Some("6"),
            target_entity_id: Some("ticket:LOOM-1"),
            payload: b"payload",
            policy_labels: &["team", "team", "restricted"],
            signature: Some(b"sig"),
            agent: Some(agent),
        },
    )?;
    let envelope_bytes = envelope.encode()?;
    assert_eq!(OperationEnvelope::decode(&envelope_bytes)?, envelope);
    assert_eq!(envelope.payload_digest, digest(b"payload"));
    assert_eq!(envelope.policy_labels, vec!["restricted", "team"]);
    assert_eq!(
        envelope.agent.as_ref().expect("agent").policy_labels,
        vec!["restricted"]
    );

    let annotation = AnnotationEvent::new(
        "event-1",
        "annotation-1",
        actor(2),
        110,
        AnnotationAction::Add {
            anchor: AnnotationAnchor::Range {
                entity_id: "page:plan".to_string(),
                start: 0,
                end: 4,
                stale: false,
            },
            body: "Looks good".to_string(),
        },
    )?;
    let annotation_bytes = annotation.encode()?;
    assert_eq!(AnnotationEvent::decode(&annotation_bytes)?, annotation);
    let mut annotations = AnnotationStore::new();
    annotations.apply(annotation)?;
    annotations.apply(AnnotationEvent::new(
        "event-2",
        "annotation-1",
        actor(2),
        120,
        AnnotationAction::ReactionAdd {
            kind: "approved".to_string(),
        },
    )?)?;
    annotations.apply(AnnotationEvent::new(
        "event-3",
        "annotation-1",
        actor(3),
        130,
        AnnotationAction::Pin,
    )?)?;
    let record = annotations.get("annotation-1").expect("annotation record");
    assert!(record.pinned_by.contains(&actor(3)));
    assert_eq!(record.reactions.len(), 1);

    let emoji = EmojiRegistry::new(vec![
        "approved".to_string(),
        "blocked".to_string(),
        "approved".to_string(),
        "\u{1F44D}".to_string(),
    ])?;
    assert_eq!(
        emoji.custom().collect::<Vec<_>>(),
        vec!["approved", "blocked"]
    );
    assert!(emoji.contains("approved"));
    assert!(emoji.contains("\u{1F44D}"));
    assert!(emoji.contains("\u{1F469}\u{200D}\u{1F4BB}"));
    let emoji_bytes = emoji.encode()?;
    assert_eq!(EmojiRegistry::decode(&emoji_bytes)?, emoji);

    let parsed_ref = EntityRef::parse("ticket:LOOM-1")?;
    assert_eq!(parsed_ref.as_str(), "ticket:LOOM-1");
    assert_eq!(EntityRef::decode(&parsed_ref.encode()?)?, parsed_ref);
    assert!(EntityRef::parse("ticket/LOOM-1").is_err());
    let occurrences = extract_ref_occurrences("(!ticket:LOOM-1), !page:Roadmap; !ticket:LOOM-1.")?;
    assert_eq!(occurrences.len(), 3);
    assert_eq!(occurrences[0].target.as_str(), "ticket:LOOM-1");
    assert_eq!(occurrences[0].text, "!ticket:LOOM-1");
    assert_eq!(occurrences[1].target.as_str(), "page:Roadmap");
    let alias_binding = AliasBinding::new("LOOM-1", parsed_ref.clone(), "project", 7)?;
    assert_eq!(
        AliasBinding::decode(&alias_binding.encode()?)?,
        alias_binding
    );
    let alias_value = loom_codec::decode(&alias_binding.encode()?)
        .map_err(|e| loom_core::LoomError::invalid(format!("alias binding cbor: {e}")))?;
    assert!(matches!(alias_value, loom_codec::Value::Array(_)));

    let mut refs = ReferenceIndex::new();
    let source = ReferenceSource::new("document", "notes", "note-1", "body")?;
    refs.replace_text_refs(
        source.clone(),
        "refers_to",
        "See !ticket:LOOM-1 and !page:plan.",
    )?;
    assert_eq!(refs.inbound(&parsed_ref).len(), 1);
    assert_eq!(
        refs.replace_text_refs(source, "refers_to", "Now see !ticket:LOOM-2.")?,
        1
    );
    assert!(refs.inbound(&EntityRef::parse("ticket:LOOM-1")?).is_empty());
    assert_eq!(refs.inbound(&EntityRef::parse("ticket:LOOM-2")?).len(), 1);
    assert_eq!(ReferenceIndex::decode(&refs.encode()?)?, refs);

    let predicate_a = Predicate::compile_json_str(
        r#"{"version":1,"expr":{"op":"eq","path":["status"],"value":{"type":"text","value":"open"}}}"#,
    )?;
    let predicate_b = Predicate::compile_json_str(
        r#"{"expr":{"path":["status"],"op":"eq","value":{"value":"open","type":"text"}},"version":1}"#,
    )?;
    assert_eq!(predicate_a, predicate_b);
    assert!(Predicate::compile_json_str(r#"{"version":1,"expr":{"op":"and","args":[]}}"#).is_err());

    let field_definition = FieldDefinition::new(
        "field-priority",
        "priority",
        "Priority",
        FieldType::list(FieldType::enum_options("priority-options")?),
        vec!["project-b".to_string(), "project-a".to_string()],
        true,
    )?
    .with_description("Priority option")?
    .imported_computed(true);
    let field_definition_bytes = field_definition.encode()?;
    assert_eq!(
        FieldDefinition::decode(&field_definition_bytes)?.encode()?,
        field_definition_bytes,
        "field definition canonical round-trip mismatch"
    );
    assert_eq!(
        field_definition.contexts,
        vec!["project-a".to_string(), "project-b".to_string()]
    );
    field_definition.validate_value(&FieldValue::List(vec![FieldValue::EnumOption(
        "high".to_string(),
    )]))?;
    assert!(
        field_definition
            .validate_value(&FieldValue::String("high".to_string()))
            .is_err()
    );
    assert!(FieldType::integer().can_widen_to(&FieldType::number()));
    assert_eq!(FieldType::text(), FieldType::String);
    assert!(FieldType::principal().can_widen_to(&FieldType::list(FieldType::principal())));
    let typed_field = FieldValue::EntityRef {
        kind: "page".to_string(),
        id: "page:plan".to_string(),
    };
    let typed_field_bytes = loom_codec::encode(&typed_field.to_value())
        .map_err(|e| loom_core::LoomError::invalid(format!("field value cbor: {e}")))?;
    assert_eq!(
        FieldValue::from_value(
            loom_codec::decode(&typed_field_bytes)
                .map_err(|e| loom_core::LoomError::invalid(format!("field value cbor: {e}")))?
        )?,
        typed_field
    );

    let body = Body::new(vec![
        Block::new(
            "heading",
            first_token(),
            BlockKind::Heading { level: 2 },
            vec![TextRun::new("Overview", vec![Mark::Italic])?],
            Vec::new(),
        )?,
        Block::new(
            "ordered",
            first_token(),
            BlockKind::ListItem { ordered: true },
            vec![TextRun::new("First", vec![Mark::Underline])?],
            Vec::new(),
        )?,
        Block::new(
            "code",
            first_token(),
            BlockKind::CodeBlock {
                language: "rust".to_string(),
            },
            vec![TextRun::new("fn main() {}", vec![Mark::Code])?],
            Vec::new(),
        )?,
        Block::new(
            "quote",
            first_token(),
            BlockKind::Quote,
            Vec::new(),
            Vec::new(),
        )?,
        Block::new(
            "divider",
            first_token(),
            BlockKind::Divider,
            Vec::new(),
            Vec::new(),
        )?,
        Block::new(
            "embed",
            first_token(),
            BlockKind::Embed,
            Vec::new(),
            Vec::new(),
        )?,
        Block::new(
            "opaque",
            first_token(),
            BlockKind::Opaque {
                kind: "profile.macro".to_string(),
                payload: b"payload".to_vec(),
            },
            Vec::new(),
            Vec::new(),
        )?,
        Block::new(
            "b1",
            first_token(),
            BlockKind::Paragraph,
            vec![
                TextRun::new("Plan", vec![Mark::Bold, Mark::Bold])?,
                TextRun::new(" update", vec![Mark::Link("loom://page/plan".to_string())])?,
            ],
            vec![Block::new(
                "child",
                first_token(),
                BlockKind::BlockRef {
                    entity_id: "page:source".to_string(),
                    block_id: Some("intro".to_string()),
                    section: false,
                    pin: Some(3),
                },
                Vec::new(),
                Vec::new(),
            )?],
        )?,
    ]);
    let body_bytes = body.encode()?;
    assert_eq!(Body::decode(&body_bytes)?, body);
    assert_eq!(
        hex::encode(&body_bytes),
        "82766c6f6f6d2e7375627374726174652e626f64792e76318885626231782037464646464646464646464646464646464646464646464646464646464646468100828264506c616e818100826720757064617465818205706c6f6f6d3a2f2f706167652f706c616e8185656368696c647820374646464646464646464646464646464646464646464646464646464646464685076b706167653a736f7572636565696e74726ff40380808564636f6465782037464646464646464646464646464646464646464646464646464646464646468203647275737481826c666e206d61696e2829207b7d8181048085676469766964657278203746464646464646464646464646464646464646464646464646464646464646810580808565656d6265647820374646464646464646464646464646464646464646464646464646464646464681068080856768656164696e67782037464646464646464646464646464646464646464646464646464646464646468201028182684f766572766965778181018085666f70617175657820374646464646464646464646464646464646464646464646464646464646464683086d70726f66696c652e6d6163726f477061796c6f6164808085676f726465726564782037464646464646464646464646464646464646464646464646464646464646468202f5818265466972737481810280856571756f74657820374646464646464646464646464646464646464646464646464646464646464681048080",
        "body canonical bytes mismatch"
    );
    assert!(
        Block::new(
            "bad",
            first_token(),
            BlockKind::Heading { level: 0 },
            Vec::new(),
            Vec::new()
        )
        .is_err()
    );
    assert!(
        Block::new(
            "bad-opaque",
            first_token(),
            BlockKind::Opaque {
                kind: "profile.empty".to_string(),
                payload: Vec::new(),
            },
            Vec::new(),
            Vec::new(),
        )
        .is_err()
    );
    body.validate_range("b1", 0, 4)?;
    let patched = body.apply_patch(
        &BodyPatch::new(
            1,
            vec![BodyDelta::SpliceText {
                block_id: "b1".to_string(),
                start: 4,
                end: 4,
                replacement: vec![TextRun::new(" draft", Vec::new())?],
            }],
        )?,
        1,
    )?;
    assert!(patched.find_block("b1").is_some());
    let anchor = BodyAnchor::new("b1", 2, 6)?;
    assert!(anchor.map_splice("b1", 3, 5, 1).stale);
    let epoch = loom_substrate::body::BodyEpoch::new("epoch-1", 2, 4, false)?;
    assert!(epoch.can_render());
    assert!(!loom_substrate::body::BodyEpoch::new("epoch-2", 3, 1, true)?.can_render());

    let view = ViewDefinition::new(ViewDefinitionInput {
        view_id: "status",
        source_scopes: &["organization", "organization"],
        source_facets: &["document", "graph"],
        projection_ref: "program:status-v1",
        output_facet: Some("document"),
        media_type: "application/json",
        freshness_policy: FreshnessPolicy::OnRead,
        output_digest: Some(digest(b"out")),
        source_digests: &[digest(b"b"), digest(b"a"), digest(b"a")],
    })?;
    let view_bytes = view.encode()?;
    assert_eq!(ViewDefinition::decode(&view_bytes)?, view);
    assert_eq!(view.source_scopes, vec!["organization"]);
    assert_eq!(view.source_facets, vec!["document", "graph"]);
    assert_eq!(view.source_digests, vec![digest(b"b"), digest(b"a")]);
    let mut views = ViewRegistry::new();
    views.define(view.clone());
    assert_eq!(views.get("status"), Some(&view));
    assert_eq!(views.list().count(), 1);

    let mut revisions = RevisionIndex::new();
    let root_1 = digest(b"root-1");
    let root_2 = digest(b"root-2");
    revisions.append_revision(EntityRevision::new(
        "page:plan",
        1,
        "op-1",
        BodyRef::new(digest(b"v1"), 2, "text/plain")?,
        root_1,
        10,
    )?)?;
    revisions.append_revision(EntityRevision::new(
        "page:plan",
        2,
        "op-2",
        BodyRef::new(digest(b"v2"), 2, "text/plain")?,
        root_2,
        20,
    )?)?;
    revisions.add_checkpoint(Checkpoint::new("project", "cp-1", root_1, 1, "op-1", 11)?)?;
    assert_eq!(revisions.latest("page:plan").expect("latest").revision, 2);
    assert_eq!(
        revisions
            .at_revision("page:plan", 1)
            .expect("revision one")
            .operation_id,
        "op-1"
    );
    assert_eq!(
        revisions
            .as_of_root("page:plan", &root_2)
            .expect("as of root")
            .revision,
        2
    );
    assert_eq!(
        revisions
            .checkpoint_before_or_at("project", 2)
            .expect("checkpoint")
            .checkpoint_id,
        "cp-1"
    );
    assert_eq!(RevisionIndex::decode(&revisions.encode()?)?, revisions);

    let mut sequencer = LocalSequencer::new();
    sequencer.register_scope("project", root_1)?;
    let mut hooks = Hooks;
    let first = sequencer.sequence(
        Algo::Blake3,
        SequenceRequest {
            draft: OperationDraft {
                workspace_id: "organization".to_string(),
                app_id: "tickets".to_string(),
                scope_id: "project".to_string(),
                operation_id: "op-1".to_string(),
                operation_kind: "ticket.created".to_string(),
                actor_principal: actor(4),
                actor_kind: ActorKind::User,
                timestamp_ms: 1000,
                idempotency_key: "idem-a".to_string(),
                base_root: root_1,
                base_entity_version: None,
                target_entity_id: Some("ticket:LOOM-1".to_string()),
                payload: b"create".to_vec(),
                policy_labels: vec!["team".to_string()],
                signature: None,
                agent: None,
            },
            root_after: root_2,
            alias_requests: Vec::new(),
            order_token_requests: Vec::new(),
        },
        &mut hooks,
    )?;
    assert_eq!(first.envelope.sequence, 1);
    let batch = operation_log_changes(
        sequencer.operations("project"),
        &OperationChangeCursor::start("project")?,
        1,
    )?;
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.events[0].operation_id, "op-1");
    assert_eq!(
        OperationChangeCursor::decode(&batch.next.encode())?,
        batch.next
    );
    assert_eq!(sequencer.changes(&batch.next, 10)?.events.len(), 0);
    let log = sequencer.operation_log("project")?;
    assert_eq!(OperationLog::decode(&log.encode()?)?, log);
    assert_eq!(OperationLog::decode(&log.encode()?)?.operations[0], first);

    let ticket_log = TicketOperationLog::new(
        "organization",
        vec![TicketOperationRecord::new(
            1,
            "ticket-op-1",
            "ticket.created",
            Some("ticket:LOOM-1".to_string()),
            root_2,
            envelope_bytes.clone(),
            None,
        )?],
    )?;
    assert_eq!(
        TicketOperationLog::decode(&ticket_log.encode()?)?,
        ticket_log
    );
    let ticket_changes = ticket_log.changes(
        &OperationChangeCursor::start(ticket_operation_cursor_scope("organization"))?,
        10,
    )?;
    assert_eq!(ticket_changes.events.len(), 1);
    assert_eq!(ticket_changes.events[0].app_id, "tickets");
    assert_eq!(ticket_changes.events[0].root_after, root_2);

    let page_envelope = OperationEnvelope::new(
        Algo::Blake3,
        OperationEnvelopeInput {
            workspace_id: "organization",
            app_id: "pages",
            scope_id: "page:plan",
            operation_id: "page-op-1",
            operation_kind: "page.published",
            sequence: 1,
            actor_principal: actor(5),
            actor_kind: ActorKind::User,
            timestamp_ms: 2000,
            idempotency_key: "idem-page",
            base_root: root_1,
            base_entity_version: Some("1"),
            target_entity_id: Some("page:plan"),
            payload: b"page publish",
            policy_labels: &["team"],
            signature: None,
            agent: None,
        },
    )?;
    let page_log = PageOperationLog::new(
        "organization",
        vec![PageOperationRecord::new(
            1,
            "page-op-1",
            "page.published",
            Some("page:plan".to_string()),
            root_2,
            page_envelope.encode()?,
        )?],
    )?;
    assert_eq!(PageOperationLog::decode(&page_log.encode()?)?, page_log);
    let page_changes = page_log.changes(
        &OperationChangeCursor::start(page_operation_cursor_scope("organization"))?,
        10,
    )?;
    assert_eq!(page_changes.events.len(), 1);
    assert_eq!(page_changes.events[0].app_id, "pages");
    assert_eq!(page_changes.events[0].operation_kind, "page.published");
    assert_eq!(page_changes.events[0].root_after, root_2);

    let mut ideate = LifecycleStage::new("ideate", "Ideate")?;
    ideate.exit_gates = vec![LifecycleGate::new(LifecycleGateInput {
        gate_id: "idea-framed",
        label: "Idea framed",
        kind: GateKind::Attestation,
        predicate_digest: None,
        required_role: Some("operator"),
    })?];
    let mut ready = LifecycleStage::new("ready", "Ready")?;
    ready.snapshot_policy = SnapshotPolicy::FreezeScope;
    ready.entry_gates = vec![LifecycleGate::new(LifecycleGateInput {
        gate_id: "scope-accepted",
        label: "Scope accepted",
        kind: GateKind::Predicate,
        predicate_digest: Some(digest(b"predicate")),
        required_role: Some("operator"),
    })?];
    let lifecycle_definition =
        LifecycleDefinition::new("feature", "1", vec![ideate, ready], "ideate")?;
    let lifecycle_instance = LifecycleInstance::new(
        "feat-1",
        &lifecycle_definition,
        vec!["page:plan".to_string()],
    )?;
    let lifecycle_transition = LifecycleTransitionRecord::new(LifecycleTransitionInput {
        transition_id: "transition-1".to_string(),
        instance_id: lifecycle_instance.instance_id.clone(),
        definition_id: lifecycle_definition.definition_id.clone(),
        definition_version: lifecycle_definition.version.clone(),
        from_stage_id: "ideate".to_string(),
        to_stage_id: "ready".to_string(),
        actor_principal_id: actor(6).to_string(),
        gate_evaluations: vec![
            GateEvaluation::new("idea-framed", true, 3000)?,
            GateEvaluation::new("scope-accepted", true, 3000)?,
        ],
        snapshot_digest: Some(root_2),
        recorded_at_ms: 3000,
    })?;
    let lifecycle_payload = lifecycle_transition.encode()?;
    let lifecycle_envelope = OperationEnvelope::new(
        Algo::Blake3,
        OperationEnvelopeInput {
            workspace_id: "organization",
            app_id: "lifecycle",
            scope_id: &lifecycle_operation_cursor_scope("organization"),
            operation_id: "transition-1",
            operation_kind: "lifecycle.transitioned",
            sequence: 1,
            actor_principal: actor(6),
            actor_kind: ActorKind::User,
            timestamp_ms: 3000,
            idempotency_key: "transition-1",
            base_root: root_1,
            base_entity_version: None,
            target_entity_id: Some("lifecycle:feat-1"),
            payload: &lifecycle_payload,
            policy_labels: &["team"],
            signature: None,
            agent: None,
        },
    )?;
    let lifecycle_log = LifecycleOperationLog::new(
        "organization",
        vec![LifecycleOperationRecord::transition(
            1,
            &lifecycle_transition,
            root_2,
            lifecycle_envelope.encode()?,
        )?],
    )?;
    assert_eq!(
        LifecycleOperationLog::decode(&lifecycle_log.encode()?)?,
        lifecycle_log
    );
    assert_eq!(
        lifecycle_operation_log_key("organization")?,
        b"profile/lifecycle/v1/organization/operations".to_vec()
    );
    let lifecycle_changes = lifecycle_log.changes(
        &OperationChangeCursor::start(lifecycle_operation_cursor_scope("organization"))?,
        10,
    )?;
    assert_eq!(lifecycle_changes.events.len(), 1);
    assert_eq!(lifecycle_changes.events[0].app_id, "lifecycle");
    assert_eq!(
        lifecycle_changes.events[0].operation_kind,
        "lifecycle.transitioned"
    );
    assert_eq!(lifecycle_changes.events[0].root_after, root_2);
    let workgraph_fact = WorkgraphFact {
        event_id: "workgraph-event-1".to_string(),
        occurred_at: 4000,
        task_id: "task-1".to_string(),
        batch_id: "batch-1".to_string(),
        actor_kind: ActorKind::Agent,
        actor_id: "agent-1".to_string(),
        correlation_id: "corr-1".to_string(),
        causation_id: "cause-1".to_string(),
        attempt: 1,
        previous_state: WorkgraphState::Ready,
        next_state: WorkgraphState::Assigned,
        payload_digest: digest(b"workgraph payload"),
        reason_code: None,
        kind: WorkgraphFactKind::AssignmentIssued,
    };
    let workgraph_payload = workgraph_fact.encode()?;
    assert_eq!(WorkgraphFact::decode(&workgraph_payload)?, workgraph_fact);
    assert!(WorkgraphFact::decode(&workgraph_payload[..workgraph_payload.len() - 1]).is_err());
    let workgraph_operation_kind = workgraph_operation_kind(workgraph_fact.kind);
    let workgraph_envelope = OperationEnvelope::new(
        Algo::Blake3,
        OperationEnvelopeInput {
            workspace_id: "organization",
            app_id: "workgraph",
            scope_id: &workgraph_operation_cursor_scope("organization"),
            operation_id: "workgraph-event-1",
            operation_kind: &workgraph_operation_kind,
            sequence: 1,
            actor_principal: actor(7),
            actor_kind: ActorKind::Agent,
            timestamp_ms: 4000,
            idempotency_key: "workgraph-event-1",
            base_root: root_1,
            base_entity_version: None,
            target_entity_id: Some("workgraph:task-1"),
            payload: &workgraph_payload,
            policy_labels: &["team"],
            signature: None,
            agent: None,
        },
    )?;
    let workgraph_log = WorkgraphOperationLog::new(
        "organization",
        vec![WorkgraphOperationRecord::fact(
            1,
            workgraph_fact.clone(),
            root_2,
            workgraph_envelope.encode()?,
        )?],
    )?;
    assert_eq!(
        WorkgraphOperationLog::decode(&workgraph_log.encode()?)?,
        workgraph_log
    );
    assert_eq!(
        workgraph_operation_log_key("organization")?,
        b"profile/workgraph/v1/organization/operations".to_vec()
    );
    let workgraph_changes = workgraph_log.changes(
        &OperationChangeCursor::start(workgraph_operation_cursor_scope("organization"))?,
        1,
    )?;
    assert_eq!(workgraph_changes.events.len(), 1);
    assert_eq!(workgraph_changes.events[0].app_id, "workgraph");
    assert_eq!(
        workgraph_changes.events[0].operation_kind,
        "workgraph.assignment_issued"
    );
    assert_eq!(
        workgraph_changes.next.encode(),
        "oplog:2:workgraph:organization"
    );
    assert!(
        WorkgraphOperationLog::new(
            "organization",
            vec![
                WorkgraphOperationRecord::fact(
                    1,
                    workgraph_fact.clone(),
                    root_2,
                    workgraph_envelope.encode()?,
                )?,
                WorkgraphOperationRecord::fact(
                    2,
                    WorkgraphFact {
                        event_id: "workgraph-event-2".to_string(),
                        ..workgraph_fact
                    },
                    root_2,
                    OperationEnvelope::new(
                        Algo::Blake3,
                        OperationEnvelopeInput {
                            workspace_id: "organization",
                            app_id: "workgraph",
                            scope_id: &workgraph_operation_cursor_scope("organization"),
                            operation_id: "workgraph-event-2",
                            operation_kind: "workgraph.assignment_issued",
                            sequence: 2,
                            actor_principal: actor(7),
                            actor_kind: ActorKind::Agent,
                            timestamp_ms: 4010,
                            idempotency_key: "workgraph-event-2",
                            base_root: root_1,
                            base_entity_version: None,
                            target_entity_id: Some("workgraph:task-1"),
                            payload: b"duplicate transition",
                            policy_labels: &["team"],
                            signature: None,
                            agent: None,
                        },
                    )?
                    .encode()?,
                )?,
            ],
        )
        .is_err()
    );
    let lifecycle_snapshot_plan =
        SnapshotPlan::for_transition(&lifecycle_definition, &lifecycle_instance, "ready")?;
    let lifecycle_snapshot =
        SnapshotRecord::from_plan(&lifecycle_snapshot_plan, "transition-1", root_2, 3000)?;
    assert_eq!(
        SnapshotRecord::decode(&lifecycle_snapshot.encode()?)?,
        lifecycle_snapshot
    );
    assert_eq!(lifecycle_snapshot.snapshot_id, "feat-1:transition-1");
    assert_eq!(lifecycle_snapshot.subject_refs, vec!["page:plan"]);
    assert_eq!(lifecycle_snapshot.snapshot_digest, root_2);

    let mut public_route = WebRoute::new(
        "public",
        vec![WebMethod::Get, WebMethod::Head],
        None,
        "/",
        "/public",
        WebRouteMode::StaticFile,
    )?;
    public_route.workspace = Some(actor(9));
    public_route.reference = Some(WebMountRef::Branch("main".to_string()));
    let mut docs_route = WebRoute::new(
        "docs",
        vec![WebMethod::Get, WebMethod::Head],
        Some("docs.example.com".to_string()),
        "/docs",
        "/site/docs",
        WebRouteMode::StaticFile,
    )?;
    docs_route.workspace = Some(actor(10));
    docs_route.reference = Some(WebMountRef::Tag("published".to_string()));
    let mut program_route = WebRoute::new(
        "api",
        vec![WebMethod::Get],
        Some("docs.example.com".to_string()),
        "/docs/api",
        "/programs/api",
        WebRouteMode::Program,
    )?;
    program_route.workspace = Some(actor(10));
    program_route.target = Some("program:web-api".to_string());
    let table = WebRouteTable::new(vec![public_route, docs_route, program_route])?;
    let resolved = table.resolve(Some("DOCS.example.com"), WebMethod::Get, "/docs/guide")?;
    assert_eq!(resolved.route_id, "docs");
    assert_eq!(resolved.workspace, Some(actor(10)));
    assert_eq!(resolved.materialized_path, "/site/docs/guide");
    let api = table.resolve(Some("docs.example.com"), WebMethod::Get, "/docs/api/users")?;
    assert_eq!(api.route_id, "api");
    assert_eq!(api.mode, WebRouteMode::Program);
    assert_eq!(WebRouteTable::decode(&table.encode()?)?, table);
    let mut listener = WebListener::new(
        "public",
        "127.0.0.1",
        8080,
        WebProtocol::Http,
        actor(9),
        "/public",
    )?;
    listener.routes = table.clone();
    let listener_bytes = listener.encode()?;
    assert_eq!(WebListener::decode(&listener_bytes)?, listener);
    let listener_route = listener.route(None, WebMethod::Head, "/about")?;
    assert_eq!(listener_route.route_id, "public");
    assert_eq!(listener_route.materialized_path, "/public/about");
    assert!(WebListener::new("secure", "0.0.0.0", 443, WebProtocol::Https, actor(9), "/").is_err());

    let core_apps = core_surface_catalog("main")?;
    assert_eq!(core_apps.len(), 4);
    assert_eq!(core_apps[0].app_id, "ticket-details");
    assert!(core_apps[0].resource_uri.starts_with("ui://main/mcp/apps/"));
    let all_apps = surface_app_catalog("main")?;
    assert_eq!(all_apps.len(), 16);
    assert_eq!(all_apps.last().expect("last app").app_id, "diagram-editor");
    let changes_app = all_apps
        .iter()
        .find(|app| app.app_id == "changes-inbox")
        .expect("changes inbox app");
    assert_eq!(
        changes_app.staleness_policy,
        StalenessPolicy::RequireRefresh
    );
    let meeting_apps = meeting_memory_surface_catalog("main")?;
    assert_eq!(meeting_apps.len(), 6);
    assert_eq!(meeting_apps[0].app_id, "meeting-details");
    assert_eq!(
        meeting_apps[2].staleness_policy,
        StalenessPolicy::RequireRefresh
    );
    let app_bytes = all_apps[0].encode()?;
    assert_eq!(
        loom_substrate::surfaces::SurfaceAppDefinition::decode(&app_bytes)?,
        all_apps[0],
        "surface app definition canonical round-trip mismatch"
    );

    let elicitation = ElicitationRequest::new(ElicitationRequestInput {
        request_id: "req-1",
        app_id: "board",
        principal_id: actor(6),
        operation_kind: "tickets.transition",
        message: "Missing resolution",
        schema_ref: "schema:tickets.transition",
        schema_digest: digest(b"schema"),
        context_digest: Some(digest(b"context")),
        requested_at_ms: 10,
        expires_at_ms: Some(20),
        status: ElicitationStatus::Pending,
    })?;
    assert_eq!(
        ElicitationRequest::decode(&elicitation.encode()?)?,
        elicitation,
        "surface elicitation request canonical round-trip mismatch"
    );
    assert!(
        ElicitationRequest::new(ElicitationRequestInput {
            expires_at_ms: Some(9),
            ..ElicitationRequestInput {
                request_id: "req-2",
                app_id: "board",
                principal_id: actor(6),
                operation_kind: "tickets.transition",
                message: "Missing resolution",
                schema_ref: "schema:tickets.transition",
                schema_digest: digest(b"schema"),
                context_digest: None,
                requested_at_ms: 10,
                expires_at_ms: None,
                status: ElicitationStatus::Pending,
            }
        })
        .is_err(),
        "surface elicitation expiry must be validated"
    );

    let response = ElicitationResponse::new(
        "req-1",
        actor(7),
        digest(b"response"),
        10,
        30,
        ElicitationStatus::Submitted,
    )?;
    assert_eq!(
        ElicitationResponse::decode(&response.encode()?)?,
        response,
        "surface elicitation response canonical round-trip mismatch"
    );
    let handoff = PromptHandoff::new(PromptHandoffInput {
        handoff_id: "handoff-1",
        app_id: "directed-graph",
        principal_id: actor(8),
        prompt_digest: digest(b"prompt"),
        prompt_len: 42,
        source_entity_refs: &["ticket:LOOM-9", "page:design"],
        target_prompt_ref: Some("prompt:graph.explain-cluster"),
        created_at_ms: 40,
    })?;
    assert_eq!(
        PromptHandoff::decode(&handoff.encode()?)?,
        handoff,
        "surface prompt handoff canonical round-trip mismatch"
    );
    let frame = RenderFrame::new(
        "board",
        "view:tickets.board",
        root_2,
        Some("changes:10"),
        true,
    )?;
    assert_eq!(
        RenderFrame::decode(&frame.encode()?)?,
        frame,
        "surface render frame canonical round-trip mismatch"
    );

    Ok(())
}

fn sample_meetings_snapshot() -> Result<MeetingsProfileSnapshot> {
    let mut source = SourceRecord::new(SourceRecordInput {
        source_id: "src-1",
        source_system: "granola-api",
        external_id: "not_1",
        source_digest: Digest::hash(Algo::Blake3, b"meeting-source"),
        observed_at_ms: 100,
        access_scope: "personal-notes",
        coverage: MeetingsCoverage::Partial,
    })?;
    source.sidecar_digest = Some(Digest::hash(Algo::Blake3, b"meeting-sidecar"));

    let mut meeting = MeetingRecord::new(MeetingRecordInput {
        meeting_id: "meet-1",
        title: "Architecture review",
        current_source_digest: Digest::hash(Algo::Blake3, b"meeting-source"),
        created_at_ms: 100,
        updated_at_ms: 120,
    })?;
    meeting.source_refs = vec!["src-1".to_string()];
    meeting.attendee_refs = vec!["person:ava".to_string(), "person:nas".to_string()];

    let mut span = SpanRecord::new(
        "span-1",
        "meet-1",
        "src-1",
        SpanKind::TranscriptEntry,
        "granola:not_1/transcript/0",
    )?;
    span.text_digest = Some(Digest::hash(Algo::Blake3, b"meeting-text"));

    let mut annotation = AnnotationRecord::new(
        "ann-1",
        "meet-1",
        vec!["span-1".to_string()],
        "Decision",
        "Use normalized import snapshots",
        130,
    )?;
    annotation.status = AnnotationStatus::Accepted;
    annotation.accepted_by = Some("principal-1".to_string());
    annotation.accepted_at_ms = Some(140);

    let mut import_run = ImportRunRecord::new(
        "run-1",
        InputProfile::GranolaApi,
        "personal-notes",
        MeetingsCoverage::Partial,
        90,
    )?;
    import_run.observed_ids = vec!["not_1".to_string()];
    import_run.coverage_gaps = vec!["rate-limit".to_string()];
    import_run.source_sidecar_digest = Some(Digest::hash(Algo::Blake3, b"meeting-sidecar"));

    let mut redaction = RedactionRecord::new(
        "redact-1",
        "span-1",
        "span",
        RedactionState::RetainedMetadataOnly,
        "policy-1",
        150,
    )?;
    redaction.retained_digest = Some(Digest::hash(Algo::Blake3, b"retained-metadata"));

    MeetingsProfileSnapshot::new(
        "organization",
        MeetingsProfileSnapshotParts {
            sources: vec![source],
            meetings: vec![meeting],
            spans: vec![span],
            annotations: vec![annotation],
            vocabulary_terms: Vec::new(),
            entity_merges: Vec::new(),
            promotions: Vec::new(),
            import_runs: vec![import_run],
            redactions: vec![redaction],
        },
    )
}

/// Run the Meetings profile vectors against the source-backed Studio meeting memory model.
pub fn run_meetings_profile_vectors() -> Result<()> {
    let snapshot = sample_meetings_snapshot()?;
    let snapshot_bytes = snapshot.encode()?;
    assert_eq!(
        MeetingsProfileSnapshot::decode(&snapshot_bytes)?.encode()?,
        snapshot_bytes,
        "meetings profile snapshot canonical round-trip mismatch"
    );

    let effects = ProjectionEffectSet::from_snapshot(&snapshot)?;
    let effects_bytes = effects.encode()?;
    assert_eq!(
        ProjectionEffectSet::decode(&effects_bytes)?.encode()?,
        effects_bytes,
        "meetings projection effect set canonical round-trip mismatch"
    );
    for projection in [
        ProjectionKind::Document,
        ProjectionKind::Files,
        ProjectionKind::Graph,
        ProjectionKind::Vector,
        ProjectionKind::Search,
        ProjectionKind::SqlDataframe,
        ProjectionKind::Ledger,
    ] {
        assert!(
            !effects.effects_for(projection).is_empty(),
            "meetings projection effect set must cover {projection:?}"
        );
    }

    let outputs = ProjectionOutputSet::from_snapshot(&snapshot)?;
    let outputs_bytes = outputs.encode()?;
    assert_eq!(
        ProjectionOutputSet::decode(&outputs_bytes)?.encode()?,
        outputs_bytes,
        "meetings projection output set canonical round-trip mismatch"
    );
    for projection in [
        ProjectionKind::Document,
        ProjectionKind::Files,
        ProjectionKind::Graph,
        ProjectionKind::Vector,
        ProjectionKind::Search,
        ProjectionKind::SqlDataframe,
        ProjectionKind::Ledger,
    ] {
        assert!(
            !outputs.outputs_for(projection).is_empty(),
            "meetings projection output set must cover {projection:?}"
        );
    }

    let mut redacted_snapshot = sample_meetings_snapshot()?;
    redacted_snapshot.spans[0].redaction_state = RedactionState::Redacted;
    let redacted_effects = ProjectionEffectSet::from_snapshot(&redacted_snapshot)?;
    for projection in [ProjectionKind::Search, ProjectionKind::Vector] {
        let effect = redacted_effects
            .effects_for(projection)
            .into_iter()
            .find(|effect| effect.entity_id == "span-1")
            .expect("span projection effect");
        assert_eq!(effect.action, ProjectionAction::Invalidate);
        assert_eq!(effect.redaction_state, Some(RedactionState::Redacted));
    }
    let redacted_outputs = ProjectionOutputSet::from_snapshot(&redacted_snapshot)?;
    for projection in [ProjectionKind::Search, ProjectionKind::Vector] {
        let output = redacted_outputs
            .outputs_for(projection)
            .into_iter()
            .find(|output| output.entity_id == "span-1")
            .expect("span projection output");
        assert_eq!(output.action, ProjectionAction::Invalidate);
        assert_eq!(output.redaction_state, Some(RedactionState::Redacted));
    }

    let suggested = AnnotationRecord::new(
        "ann-2",
        "meet-1",
        vec!["span-1".to_string()],
        "Risk",
        "Migration risk",
        150,
    )?;
    let mut rejected = AnnotationRecord::new(
        "ann-3",
        "meet-1",
        vec!["span-1".to_string()],
        "Task",
        "Rewrite history",
        160,
    )?;
    rejected.status = AnnotationStatus::Rejected;
    let mut review_snapshot = sample_meetings_snapshot()?;
    review_snapshot.annotations.push(suggested);
    review_snapshot.annotations.push(rejected);
    let mut term = VocabularyTermRecord::new(VocabularyTermInput {
        term_id: "term-1",
        kind: "DomainTerm",
        label: "LCB",
        evidence_annotation_ids: vec!["ann-2".to_string()],
        created_at_ms: 170,
    })?;
    term.aliases = vec!["loom control block".to_string()];
    let merge = EntityMergeRecord::new(EntityMergeInput {
        merge_id: "merge-1",
        canonical_entity_id: "person:ava",
        merged_entity_ids: vec!["person:a.vazquez".to_string()],
        evidence_annotation_ids: vec!["ann-1".to_string()],
        decided_by: "principal-1",
        decided_at_ms: 180,
    })?;
    let review = ExtractionReviewProjection::new(
        "organization",
        &review_snapshot.annotations,
        vec![term],
        vec![merge],
    )?;
    assert_eq!(review.suggested_annotation_ids, vec!["ann-2"]);
    assert_eq!(review.accepted_annotation_ids, vec!["ann-1"]);
    assert_eq!(review.rejected_annotation_ids, vec!["ann-3"]);
    assert_eq!(
        ExtractionReviewProjection::decode(&review.encode()?)?,
        review,
        "meetings extraction review canonical round-trip mismatch"
    );
    let mut accepted_annotation = AnnotationRecord::new(
        "ann-review-accepted",
        "meet-1",
        vec!["span-1".to_string()],
        "Decision",
        "Use the durable importer",
        190,
    )?;
    accepted_annotation.accept("principal-1", 191)?;
    assert_eq!(accepted_annotation.status, AnnotationStatus::Accepted);
    let mut rejected_annotation = AnnotationRecord::new(
        "ann-review-rejected",
        "meet-1",
        vec!["span-1".to_string()],
        "Risk",
        "Unsupported claim",
        190,
    )?;
    rejected_annotation.reject()?;
    assert_eq!(rejected_annotation.status, AnnotationStatus::Rejected);
    let mut accepted_term = VocabularyTermRecord::new(VocabularyTermInput {
        term_id: "term-review",
        kind: "DomainTerm",
        label: "LCB",
        evidence_annotation_ids: vec!["ann-review-accepted".to_string()],
        created_at_ms: 190,
    })?;
    accepted_term.accept("principal-1", 192)?;
    assert_eq!(accepted_term.status, VocabularyTermStatus::Accepted);

    assert!(
        AnnotationRecord::new("ann-empty", "meet-1", Vec::new(), "Decision", "choice", 1).is_err(),
        "meetings annotations must require source evidence"
    );

    Ok(())
}

pub fn run_drive_profile_vectors() -> Result<()> {
    let actor = loom_core::WorkspaceId::from_bytes([1; 16]);
    let content_digest = Digest::hash(Algo::Blake3, b"hello drive");
    let chunk_a = Digest::hash(Algo::Blake3, b"chunk-a");
    let chunk_b = Digest::hash(Algo::Blake3, b"chunk-b");
    let manifest = DriveChunkManifest::new(
        content_digest,
        CHUNK_MIN_SIZE + 1,
        vec![
            DriveChunkRef::new(chunk_a, CHUNK_MIN_SIZE)?,
            DriveChunkRef::new(chunk_b, 1)?,
        ],
    )?;
    assert_eq!(
        DriveChunkManifest::decode(&manifest.encode()?)?,
        manifest,
        "drive chunk manifest canonical round-trip mismatch"
    );
    assert_eq!(drive_fold_key("Plan.TXT")?, "plan.txt");
    assert_eq!(
        drive_profile_key("main")?,
        b"profile/drive/v1/main/snapshot".to_vec()
    );
    assert_eq!(
        drive_upload_session_key("main", "upload-1")?,
        b"profile/drive/v1/main/uploads/upload-1".to_vec()
    );
    assert_eq!(
        drive_operation_log_key("main")?,
        b"profile/drive/v1/main/operations".to_vec()
    );
    assert_eq!(
        drive_conflict_index_key("main")?,
        b"profile/drive/v1/main/conflicts".to_vec()
    );

    let folders = DriveFolderIndex::new(
        "main",
        vec![DriveFolderChildren::new(
            "root",
            vec![
                DriveFolderEntry::new("Specs", "folder-1", DriveNodeKind::Folder)?,
                DriveFolderEntry::new("Plan.txt", "file-1", DriveNodeKind::File)?,
            ],
        )?],
    )?;
    assert_eq!(
        DriveFolderIndex::decode(&folders.encode()?)?,
        folders,
        "drive folder index canonical round-trip mismatch"
    );
    assert_eq!(
        folders
            .children("root")
            .and_then(|folder| folder.entry_by_name("PLAN.TXT").transpose())
            .transpose()?
            .map(|entry| entry.node_id.as_str()),
        Some("file-1")
    );

    let versions = DriveFileVersionIndex::new(
        "main",
        vec![
            DriveFileVersion::new(
                "file-1",
                1,
                "op-1",
                actor,
                100,
                DriveContentRef::Blob {
                    digest: content_digest,
                    size: 11,
                },
            )?,
            DriveFileVersion::new(
                "file-1",
                2,
                "op-2",
                actor,
                200,
                DriveContentRef::Manifest {
                    manifest_digest: Digest::hash(Algo::Blake3, &manifest.encode()?),
                    content_digest,
                    size: CHUNK_MIN_SIZE + 1,
                },
            )?,
        ],
    )?;
    assert_eq!(
        DriveFileVersionIndex::decode(&versions.encode()?)?,
        versions,
        "drive file version index canonical round-trip mismatch"
    );
    assert_eq!(
        versions.latest("file-1").map(|version| version.version),
        Some(2)
    );

    let snapshot = DriveProfileSnapshot::new("main", folders, versions)?;
    let snapshot_bytes = snapshot.encode()?;
    assert_eq!(
        DriveProfileSnapshot::decode(&snapshot_bytes)?.encode()?,
        snapshot_bytes,
        "drive profile snapshot canonical round-trip mismatch"
    );

    let upload = DriveUploadSession::new(DriveUploadSessionInput {
        workspace_id: "main".to_string(),
        upload_id: "upload-1".to_string(),
        target_kind: DriveUploadTargetKind::NewFile,
        parent_folder_id: "root".to_string(),
        name: "Upload.txt".to_string(),
        file_id: "file-2".to_string(),
        expected_root: content_digest,
        author_principal: actor,
        created_at_ms: 300,
        chunks: vec![
            DriveUploadChunk::new(0, chunk_a, CHUNK_MIN_SIZE)?,
            DriveUploadChunk::new(1, chunk_b, 1)?,
        ],
    })?;
    assert_eq!(
        DriveUploadSession::decode(&upload.encode()?)?,
        upload,
        "drive upload session canonical round-trip mismatch"
    );

    let dehydrated = DriveDehydratedFileMarker::new(
        "file-1",
        11,
        content_digest,
        "loom://main/drive/main/files/file-1",
    )?;
    let dehydrated_bytes = dehydrated.encode()?;
    assert!(dehydrated_bytes.starts_with(DEHYDRATED_FILE_MARKER_MAGIC));
    assert!(is_drive_dehydrated_file_marker(&dehydrated_bytes));
    assert_eq!(
        DriveDehydratedFileMarker::decode(&dehydrated_bytes)?,
        dehydrated,
        "drive dehydrated file marker canonical round-trip mismatch"
    );

    let log = DriveOperationLog::new(
        "main",
        vec![DriveOperationRecord::new(
            1,
            "main:1",
            "file.upload_committed",
            Some("file-2".to_string()),
            content_digest,
            b"operation-envelope".to_vec(),
        )?],
    )?;
    assert_eq!(
        DriveOperationLog::decode(&log.encode()?)?,
        log,
        "drive operation log canonical round-trip mismatch"
    );

    let conflicts = DriveConflictIndex::new(
        "main",
        vec![DriveConflictRecord::new(
            "upload-1:conflict",
            "root",
            "file-1",
            "file-2",
            "Upload (conflicted copy of Ava, 1970-01-01).txt",
            content_digest,
            DriveConflictResolution::Open,
        )?],
    )?;
    assert_eq!(
        DriveConflictIndex::decode(&conflicts.encode()?)?,
        conflicts,
        "drive conflict index canonical round-trip mismatch"
    );

    let share = DriveShareIndex::new(
        "main",
        vec![DriveShareGrant::new(DriveShareGrantInput {
            grant_id: "grant-1".to_string(),
            target_kind: DriveShareTargetKind::Folder,
            target_id: "folder-1".to_string(),
            principal: loom_core::WorkspaceId::from_bytes([2; 16]),
            role: DriveShareRole::Editor,
            granted_by: actor,
            granted_at_ms: 400,
            expires_at_ms: Some(800),
        })?],
    )?;
    assert_eq!(
        DriveShareIndex::decode(&share.encode()?)?,
        share,
        "drive share index canonical round-trip mismatch"
    );
    assert_eq!(
        drive_share_index_key("main")?,
        b"profile/drive/v1/main/shares".to_vec()
    );

    let retention = DriveRetentionIndex::new(
        "main",
        vec![
            DriveRetentionPin::new(DriveRetentionPinInput {
                pin_id: "current".to_string(),
                kind: DriveRetentionPinKind::CurrentRoot,
                root: content_digest,
                target_entity_id: None,
                added_by: actor,
                added_at_ms: 400,
                expires_at_ms: None,
            })?,
            DriveRetentionPin::new(DriveRetentionPinInput {
                pin_id: "trash-1".to_string(),
                kind: DriveRetentionPinKind::TrashSubtree,
                root: chunk_a,
                target_entity_id: Some("file:file-2".to_string()),
                added_by: actor,
                added_at_ms: 400,
                expires_at_ms: Some(900),
            })?,
        ],
    )?;
    assert_eq!(
        DriveRetentionIndex::decode(&retention.encode()?)?,
        retention,
        "drive retention index canonical round-trip mismatch"
    );
    assert!(retention.live_roots().contains(&content_digest));
    assert_eq!(
        drive_retention_index_key("main")?,
        b"profile/drive/v1/main/retention".to_vec()
    );

    assert_eq!(
        conflict_copy_name("Plan.txt", "Ava/Lead", 0, 2)?,
        "Plan (conflicted copy of Ava_Lead, 1970-01-01) - 2.txt"
    );
    assert_eq!(
        drive_merge_outcome(
            &DriveConcurrentOperation::CreateFile {
                folder_id: "root".to_string(),
                name: "Plan.txt".to_string(),
                content_digest,
                actor_display: "Ava".to_string(),
                timestamp_ms: 0,
            },
            &DriveConcurrentOperation::CreateFile {
                folder_id: "root".to_string(),
                name: "plan.TXT".to_string(),
                content_digest: Digest::hash(Algo::Blake3, b"other"),
                actor_display: "Ben".to_string(),
                timestamp_ms: 0,
            },
        )?,
        DriveMergeOutcome::ConflictCopy {
            name: "plan (conflicted copy of Ben, 1970-01-01).TXT".to_string(),
        }
    );
    assert_eq!(
        drive_merge_outcome(
            &DriveConcurrentOperation::Move {
                node_id: "folder-1".to_string(),
                target_folder_id: "folder-2".to_string(),
                creates_cycle: true,
            },
            &DriveConcurrentOperation::Move {
                node_id: "folder-1".to_string(),
                target_folder_id: "folder-3".to_string(),
                creates_cycle: false,
            },
        )?,
        DriveMergeOutcome::Reject {
            rule: "path_cycle".to_string(),
        }
    );

    Ok(())
}

/// Certify a backend store against **all** canonical vectors: the blob digests, the object-model
/// digests, and the table + secondary-index identity. The single entry point a backend's test calls to
/// prove it reproduces the data model byte-for-byte. Takes `store` by value (the table vectors build a
/// `Loom` that owns it).
pub fn run_all_vectors<S: ObjectStore>(mut store: S) -> Result<()> {
    run_blob_vectors(&mut store)?;
    run_object_model_vectors(&mut store)?;
    run_codec_contract_vectors()?;
    run_document_root_vectors()?;
    run_capability_policy_vectors()?;
    run_template_vectors()?;
    run_exec_manifest_vectors()?;
    run_interchange_vectors()?;
    run_substrate_model_vectors()?;
    run_drive_profile_vectors()?;
    run_meetings_profile_vectors()?;
    run_lock_fence_vectors()?;
    // The ledger chain hash is store-independent, so both profiles are certified here regardless of the
    // backing store's own profile.
    run_ledger_head_vectors_profiled(Algo::Blake3)?;
    run_ledger_head_vectors_profiled(Algo::Sha256)?;
    run_graph_root_vectors()?;
    run_graph_semantic_diff_merge_vectors()?;
    run_columnar_manifest_vectors()?;
    run_kv_map_vectors()?;
    run_table_identity_vectors(store)
}

/// The canonical vector suites certified by [`run_all_vectors`], named for the summary. Each name maps
/// to one pinned-vector runner in this module.
pub const CANONICAL_VECTOR_SUITES: &[&str] = &[
    "blob",
    "object-model",
    "codec-contract",
    "document-root",
    "capability-policy",
    "templates",
    "exec-manifest",
    "interchange",
    "substrate-model",
    "drive-profile",
    "meetings-profile",
    "lock-fence",
    "ledger-head",
    "graph-root",
    "graph-semantic-diff-merge",
    "columnar-manifest",
    "kv-map",
    "table-identity",
];

/// One portable network-access rule vector. These vectors are data-only because network-access
/// policies live in the hosted/store layer, outside this crate's dependency boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkAccessRuleVector {
    pub id: &'static str,
    pub action: &'static str,
    pub source_cidr: Option<&'static str>,
    pub trusted_proxy_cidr: Option<&'static str>,
    pub require_mtls: bool,
    pub client_cert_san: Option<&'static str>,
}

/// One portable network-access decision vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkAccessVector {
    pub name: &'static str,
    pub default_action: &'static str,
    pub rules: &'static [NetworkAccessRuleVector],
    pub peer_ip: &'static str,
    pub x_forwarded_for: Option<&'static str>,
    pub forwarded: Option<&'static str>,
    pub peer_cert_san: Option<&'static str>,
    pub expect_allowed: bool,
    pub expect_rule_id: Option<&'static str>,
    pub expect_source_family: &'static str,
}

const NETWORK_ACCESS_DIRECT_RULES: &[NetworkAccessRuleVector] = &[
    NetworkAccessRuleVector {
        id: "allow-office",
        action: "allow",
        source_cidr: Some("10.0.0.0/8"),
        trusted_proxy_cidr: None,
        require_mtls: false,
        client_cert_san: None,
    },
    NetworkAccessRuleVector {
        id: "deny-host",
        action: "deny",
        source_cidr: Some("10.1.2.3/32"),
        trusted_proxy_cidr: None,
        require_mtls: false,
        client_cert_san: None,
    },
];

const NETWORK_ACCESS_PROXY_RULES: &[NetworkAccessRuleVector] = &[NetworkAccessRuleVector {
    id: "proxy-office",
    action: "allow",
    source_cidr: Some("203.0.113.0/24"),
    trusted_proxy_cidr: Some("10.0.0.0/8"),
    require_mtls: false,
    client_cert_san: None,
}];

const NETWORK_ACCESS_MTLS_RULES: &[NetworkAccessRuleVector] = &[NetworkAccessRuleVector {
    id: "allow-client-cert",
    action: "allow",
    source_cidr: None,
    trusted_proxy_cidr: None,
    require_mtls: true,
    client_cert_san: Some("client.example"),
}];

/// Data-only network-access vectors for cross-language hosted runtimes.
pub const NETWORK_ACCESS_VECTORS: &[NetworkAccessVector] = &[
    NetworkAccessVector {
        name: "direct-cidr-first-match",
        default_action: "deny",
        rules: NETWORK_ACCESS_DIRECT_RULES,
        peer_ip: "10.1.2.3",
        x_forwarded_for: None,
        forwarded: None,
        peer_cert_san: None,
        expect_allowed: true,
        expect_rule_id: Some("allow-office"),
        expect_source_family: "ipv4",
    },
    NetworkAccessVector {
        name: "default-deny-no-match",
        default_action: "deny",
        rules: NETWORK_ACCESS_DIRECT_RULES,
        peer_ip: "192.0.2.10",
        x_forwarded_for: None,
        forwarded: None,
        peer_cert_san: None,
        expect_allowed: false,
        expect_rule_id: None,
        expect_source_family: "ipv4",
    },
    NetworkAccessVector {
        name: "trusted-proxy-x-forwarded-for",
        default_action: "deny",
        rules: NETWORK_ACCESS_PROXY_RULES,
        peer_ip: "10.1.2.3",
        x_forwarded_for: Some("203.0.113.44, 198.51.100.1"),
        forwarded: None,
        peer_cert_san: None,
        expect_allowed: true,
        expect_rule_id: Some("proxy-office"),
        expect_source_family: "ipv4",
    },
    NetworkAccessVector {
        name: "trusted-proxy-forwarded-malformed",
        default_action: "allow",
        rules: NETWORK_ACCESS_PROXY_RULES,
        peer_ip: "10.1.2.3",
        x_forwarded_for: None,
        forwarded: Some("for=not-an-ip;proto=https"),
        peer_cert_san: None,
        expect_allowed: false,
        expect_rule_id: Some("proxy-office"),
        expect_source_family: "ipv4",
    },
    NetworkAccessVector {
        name: "mtls-san-match",
        default_action: "deny",
        rules: NETWORK_ACCESS_MTLS_RULES,
        peer_ip: "198.51.100.20",
        x_forwarded_for: None,
        forwarded: None,
        peer_cert_san: Some("client.example"),
        expect_allowed: true,
        expect_rule_id: Some("allow-client-cert"),
        expect_source_family: "ipv4",
    },
    NetworkAccessVector {
        name: "mtls-san-missing",
        default_action: "deny",
        rules: NETWORK_ACCESS_MTLS_RULES,
        peer_ip: "198.51.100.20",
        x_forwarded_for: None,
        forwarded: None,
        peer_cert_san: None,
        expect_allowed: false,
        expect_rule_id: None,
        expect_source_family: "ipv4",
    },
];

/// A typed result of one aggregate conformance run: which canonical vector suites and which executable
/// behavioral suites were certified, the full declarative scenario inventory, and the data-only suites
/// that have no runner yet. Data-only suites are listed for visibility and are never reported as passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceSummary {
    /// Canonical vector suites certified (see [`CANONICAL_VECTOR_SUITES`]).
    pub vector_suites_passed: Vec<&'static str>,
    /// Capability names whose executable behavioral runner passed.
    pub behavior_suites_passed: Vec<&'static str>,
    /// Total declarative scenarios across every [`behavior::BEHAVIOR_SUITES`] entry.
    pub total_scenarios: usize,
    /// Capability names with a scenario inventory but no executable runner (not certified).
    pub data_only_suites: Vec<&'static str>,
}

/// Run every executable conformance check against fresh in-memory backends: the canonical vectors via
/// [`run_all_vectors`] and the executable behavioral suites. Returns a [`ConformanceSummary`]; any
/// failure short-circuits with `Err`. Declarative-only scenario suites appear in the summary as
/// inventory, never as passed.
pub fn certify_memory_store() -> Result<ConformanceSummary> {
    use loom_core::{Loom, MemoryStore};

    run_all_vectors(MemoryStore::new())?;

    let mut cas_store = MemoryStore::new();
    behavior::run_cas_behavior(&mut cas_store)?;

    let mut cas_facade_loom = Loom::new(MemoryStore::new());
    let mut cas_facade_dst = Loom::new(MemoryStore::new());
    behavior::run_cas_facade_behavior(&mut cas_facade_loom, &mut cas_facade_dst)?;

    let mut kv_facade_loom = Loom::new(MemoryStore::new());
    let mut kv_facade_dst = Loom::new(MemoryStore::new());
    behavior::run_kv_facade_behavior(&mut kv_facade_loom, &mut kv_facade_dst)?;
    behavior::run_kv_conditional_mutation_behavior()?;
    behavior::run_projection_adapter_conditional_mutation_behavior()?;

    behavior::run_lock_behavior()?;

    behavior::run_identity_behavior()?;

    let mut acl_loom = Loom::new(MemoryStore::new());
    behavior::run_acl_behavior(&mut acl_loom)?;

    let mut ephemeral_kv_loom = Loom::new(MemoryStore::new());
    behavior::run_ephemeral_kv_behavior(&mut ephemeral_kv_loom)?;

    let mut document_facade_loom = Loom::new(MemoryStore::new());
    let mut document_facade_dst = Loom::new(MemoryStore::new());
    behavior::run_document_facade_behavior(&mut document_facade_loom, &mut document_facade_dst)?;
    behavior::run_derived_artifact_recovery_behavior()?;

    let mut timeseries_facade_loom = Loom::new(MemoryStore::new());
    let mut timeseries_facade_dst = Loom::new(MemoryStore::new());
    behavior::run_timeseries_facade_behavior(
        &mut timeseries_facade_loom,
        &mut timeseries_facade_dst,
    )?;

    let mut metrics_loom = Loom::new(MemoryStore::new());
    behavior::run_metrics_behavior(&mut metrics_loom)?;
    logs_behavior::run_logs_behavior()?;
    traces_behavior::run_traces_behavior()?;
    behavior::run_triggers_behavior()?;
    behavior::run_program_behavior()?;
    behavior::run_fsdir_behavior()?;
    behavior::run_document_blob_behavior()?;

    let mut ledger_facade_loom = Loom::new(MemoryStore::new());
    let mut ledger_facade_dst = Loom::new(MemoryStore::new());
    behavior::run_ledger_facade_behavior(&mut ledger_facade_loom, &mut ledger_facade_dst)?;

    let mut graph_facade_loom = Loom::new(MemoryStore::new());
    let mut graph_facade_dst = Loom::new(MemoryStore::new());
    behavior::run_graph_facade_behavior(&mut graph_facade_loom, &mut graph_facade_dst)?;

    let mut vector_facade_loom = Loom::new(MemoryStore::new());
    let mut vector_facade_dst = Loom::new(MemoryStore::new());
    behavior::run_vector_facade_behavior(&mut vector_facade_loom, &mut vector_facade_dst)?;

    let mut columnar_facade_loom = Loom::new(MemoryStore::new());
    let mut columnar_facade_dst = Loom::new(MemoryStore::new());
    behavior::run_columnar_facade_behavior(&mut columnar_facade_loom, &mut columnar_facade_dst)?;

    let mut dataframe_facade_loom = Loom::new(MemoryStore::new());
    let mut dataframe_facade_dst = Loom::new(MemoryStore::new());
    behavior::run_dataframe_facade_behavior(&mut dataframe_facade_loom, &mut dataframe_facade_dst)?;

    let mut search_facade_loom = Loom::new(MemoryStore::new());
    let mut search_facade_dst = Loom::new(MemoryStore::new());
    behavior::run_search_facade_behavior(&mut search_facade_loom, &mut search_facade_dst)?;

    let mut calendar_facade_loom = Loom::new(MemoryStore::new());
    let mut calendar_facade_dst = Loom::new(MemoryStore::new());
    behavior::run_calendar_facade_behavior(&mut calendar_facade_loom, &mut calendar_facade_dst)?;

    let mut contacts_facade_loom = Loom::new(MemoryStore::new());
    let mut contacts_facade_dst = Loom::new(MemoryStore::new());
    behavior::run_contacts_facade_behavior(&mut contacts_facade_loom, &mut contacts_facade_dst)?;

    let mut mail_facade_loom = Loom::new(MemoryStore::new());
    let mut mail_facade_dst = Loom::new(MemoryStore::new());
    behavior::run_mail_facade_behavior(&mut mail_facade_loom, &mut mail_facade_dst)?;

    let mut pim_trigger_loom = Loom::new(MemoryStore::new());
    behavior::run_pim_trigger_behavior(&mut pim_trigger_loom)?;

    behavior::run_inference_behavior()?;
    behavior::run_embedding_behavior()?;

    behavior::run_sql_error_behavior()?;
    behavior::run_sql_history_behavior()?;

    let mut merge_loom = Loom::new(MemoryStore::new());
    behavior::run_merge_conflict_behavior(&mut merge_loom)?;

    let mut staging_loom = Loom::new(MemoryStore::new());
    behavior::run_staging_behavior(&mut staging_loom)?;

    let mut file_ops_loom = Loom::new(MemoryStore::new());
    behavior::run_file_ops_behavior(&mut file_ops_loom)?;

    let mut file_handle_loom = Loom::new(MemoryStore::new());
    behavior::run_file_handle_behavior(&mut file_handle_loom)?;

    let mut symlink_loom = Loom::new(MemoryStore::new());
    behavior::run_symlink_behavior(&mut symlink_loom)?;

    let mut tags_loom = Loom::new(MemoryStore::new());
    behavior::run_tags_behavior(&mut tags_loom)?;

    let mut restore_loom = Loom::new(MemoryStore::new());
    behavior::run_restore_behavior(&mut restore_loom)?;

    let mut replay_loom = Loom::new(MemoryStore::new());
    behavior::run_replay_behavior(&mut replay_loom)?;

    let mut squash_loom = Loom::new(MemoryStore::new());
    behavior::run_squash_behavior(&mut squash_loom)?;

    let mut protected_ref_loom = Loom::new(MemoryStore::new());
    behavior::run_protected_ref_behavior(&mut protected_ref_loom)?;

    let mut diff_commits_loom = Loom::new(MemoryStore::new());
    behavior::run_diff_commits_behavior(&mut diff_commits_loom)?;

    let mut watch_loom = Loom::new(MemoryStore::new());
    behavior::run_watch_behavior(&mut watch_loom)?;

    let mut ns_loom = Loom::new(MemoryStore::new());
    let mut ns_imported = Loom::new(MemoryStore::new());
    behavior::run_workspace_behavior(&mut ns_loom, &mut ns_imported)?;

    let mut sync_src = Loom::new(MemoryStore::new());
    let mut sync_dst = Loom::new(MemoryStore::new());
    behavior::run_sync_behavior(&mut sync_src, &mut sync_dst)?;

    let mut queue_loom = Loom::new(MemoryStore::new());
    let mut queue_dst = Loom::new(MemoryStore::new());
    behavior::run_queue_behavior(&mut queue_loom, &mut queue_dst)?;

    let mut consumer_loom = Loom::new(MemoryStore::new());
    let mut consumer_dst = Loom::new(MemoryStore::new());
    behavior::run_consumer_behavior(&mut consumer_loom, &mut consumer_dst)?;

    let mut delivery_loom = Loom::new(MemoryStore::new());
    behavior::run_delivery_behavior(&mut delivery_loom)?;

    let cap_loom = Loom::new(MemoryStore::new());
    behavior::run_capability_behavior(&cap_loom)?;

    let mut exec_loom = Loom::new(MemoryStore::new());
    behavior::run_exec_behavior(&mut exec_loom)?;

    let mut sql_state_access_loom = Loom::new(MemoryStore::new());
    behavior::run_sql_state_access_behavior(&mut sql_state_access_loom)?;

    Ok(memory_store_certification_manifest())
}

fn memory_store_certification_manifest() -> ConformanceSummary {
    let executable = behavior::EXECUTABLE_BEHAVIOR_SUITES;
    let total_scenarios = behavior::BEHAVIOR_SUITES
        .iter()
        .map(|(_, scenarios)| scenarios.len())
        .sum();
    let data_only_suites = behavior::BEHAVIOR_SUITES
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !executable.contains(name))
        .collect();

    ConformanceSummary {
        vector_suites_passed: CANONICAL_VECTOR_SUITES.to_vec(),
        behavior_suites_passed: executable.to_vec(),
        total_scenarios,
        data_only_suites,
    }
}

// ---- binding conformance inventory --------------------------------------------------------------

/// How strongly a binding-adjacent conformance surface is exercised by checked-in evidence, strongest
/// first. The tiers distinguish workspace-gated checks, binding-runtime checks, implemented surfaces
/// without runtime gates, and target surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingTier {
    /// An executable suite that the Rust workspace runs in-tree: canonical vectors, C ABI, and result
    /// codec. CI-gated.
    ExecutableCore,
    /// A checked-in binding runtime or smoke suite run through its binding-specific toolchain recipe.
    BindingRuntimeSuite,
    /// A binding surface that is implemented and has a build recipe, but no checked-in runtime test
    /// exercises it, so it is not runtime-gated.
    ImplementedNotGated,
    /// A checked-in cross-binding interoperability gate run through a multi-binding toolchain recipe.
    CrossBindingInterop,
    /// A target-only binding surface.
    TargetOnly,
}

/// One binding-adjacent conformance surface and the strongest checked-in evidence behind it. `evidence`
/// is a repo-relative path to the test file, source file, or build artifact that substantiates the tier;
/// it is empty for [`BindingTier::TargetOnly`], which by definition has no checked-in gate.
#[derive(Debug, Clone, Copy)]
pub struct BindingSurface {
    /// Surface name (a binding family, or a core/target surface).
    pub name: &'static str,
    /// The evidence tier.
    pub tier: BindingTier,
    /// Repo-relative path to the substantiating artifact, or empty for a target surface.
    pub evidence: &'static str,
    /// Promoted surfaces exercised by the cited evidence. Empty for target-only or source-only rows.
    pub coverage: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
pub struct BindingRuntimeCertificationRow {
    pub surface: &'static str,
    pub target: &'static str,
    pub status: HostedProtocolStatus,
    pub workflow: &'static str,
    pub evidence: &'static [&'static str],
    pub coverage: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
pub struct BindingPackageCertificationRow {
    pub surface: &'static str,
    pub package_name: &'static str,
    pub package_kind: &'static str,
    pub build_recipe: &'static str,
    pub materials_status: HostedProtocolStatus,
    pub compatibility_metadata_status: HostedProtocolStatus,
    pub signing_manifest_status: HostedProtocolStatus,
    pub publication_status: HostedProtocolStatus,
    pub evidence: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedProtocolStatus {
    Supported,
    Degraded,
    Target,
    Unsupported,
}

/// One hosted protocol behavior row. This is evidence inventory, not full protocol certification:
/// bounded source-backed profiles are `Supported`, intentionally partial profiles are `Degraded`,
/// planned standards work is `Target`, and explicitly rejected behavior is `Unsupported`.
#[derive(Debug, Clone, Copy)]
pub struct HostedProtocolFeature {
    pub surface: &'static str,
    pub protocol: &'static str,
    pub feature: &'static str,
    pub status: HostedProtocolStatus,
    pub evidence: &'static str,
}

/// One local coordination evidence row. This separates daemon/lock/runtime coordination proof from
/// hosted protocol feature evidence and from the target public `lock` capability row.
#[derive(Debug, Clone, Copy)]
pub struct LocalCoordinationFeature {
    pub surface: &'static str,
    pub feature: &'static str,
    pub status: HostedProtocolStatus,
    pub evidence: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CapabilityPlane {
    Local,
    Mcp,
    HostedTierOne,
    HostedTierTwo,
    Binding,
    Provider,
}

#[derive(Debug, Clone, Copy)]
pub struct CapabilityMatrixRow {
    pub plane: CapabilityPlane,
    pub surface: &'static str,
    pub transport: &'static str,
    pub profile: Option<&'static str>,
    pub status: HostedProtocolStatus,
    pub evidence: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
pub struct ReleaseCertificationRow {
    pub category: &'static str,
    pub surface: &'static str,
    pub target: &'static str,
    pub status: HostedProtocolStatus,
    pub evidence: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertificationStatus {
    Passed,
    Failed,
    Degraded,
    Unsupported,
    Skipped,
    Target,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CertificationClientRequirement {
    pub surface: &'static str,
    pub protocol: &'static str,
    pub client: &'static str,
    pub platform: &'static str,
    pub role: &'static str,
    pub status: CertificationStatus,
    pub evidence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptRedactionPolicyReport {
    pub name: &'static str,
    pub retention: &'static str,
    pub redact: &'static [&'static str],
    pub retain: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertificationProfileReport {
    pub name: &'static str,
    pub surface: &'static str,
    pub admin_profile_key: &'static str,
    pub owner_scope: &'static str,
    pub tls_mode: &'static str,
    pub auth_mode: &'static str,
    pub redaction_policy: TranscriptRedactionPolicyReport,
    pub conformance_suites: Vec<&'static str>,
    pub required_clients: Vec<CertificationClientRequirement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptInventoryReport {
    pub name: &'static str,
    pub surface: &'static str,
    pub protocol: &'static str,
    pub client: &'static str,
    pub status: CertificationStatus,
    pub reason: &'static str,
    pub evidence: &'static str,
    pub redaction_profile: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatibilityProfileTranscriptEvidence {
    pub surface: &'static str,
    pub protocol: &'static str,
    pub profile: &'static str,
    pub transcript_name: &'static str,
}

pub const COMPATIBILITY_PROFILE_TRANSCRIPT_EVIDENCE: &[CompatibilityProfileTranscriptEvidence] =
    &[CompatibilityProfileTranscriptEvidence {
        surface: "mail",
        protocol: "jmap",
        profile: "rfc8620-rfc8621",
        transcript_name: "jmap-rfc8620-rfc8621-owner-only",
    }];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealListenerTranscriptClass {
    Supported,
    Guarded,
    Unavailable,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptEvidenceKind {
    RealListener,
    GuardedOptionalTool,
    UnavailableListener,
    UnsupportedBoundary,
    MockHandler,
    InventoryOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RealListenerTranscriptScenario {
    pub name: &'static str,
    pub surface: &'static str,
    pub protocol: &'static str,
    pub profile: Option<&'static str>,
    pub client: &'static str,
    pub class: RealListenerTranscriptClass,
    pub expected_status: CertificationStatus,
    pub evidence_kind: TranscriptEvidenceKind,
    pub evidence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizedTranscriptEvent {
    pub direction: &'static str,
    pub protocol: &'static str,
    pub summary: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealListenerTranscriptObservation {
    pub status: CertificationStatus,
    pub reason: &'static str,
    pub stable_error: Option<Code>,
    pub events: Vec<NormalizedTranscriptEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealListenerTranscriptOutcome {
    pub name: &'static str,
    pub class: RealListenerTranscriptClass,
    pub status: CertificationStatus,
    pub event_count: usize,
    pub stable_error: Option<Code>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DifferentialClientKind {
    MatureClient,
    OfficialSdk,
    ProtocolTool,
    FixtureRunner,
    GeneratedClient,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatureClientDifferentialEvidence {
    pub surface: &'static str,
    pub protocol: &'static str,
    pub profile: Option<&'static str>,
    pub client: &'static str,
    pub client_kind: DifferentialClientKind,
    pub status: CertificationStatus,
    pub evidence: &'static str,
    pub drift_policy: &'static str,
    pub stable_error: Option<Code>,
}

pub const MATURE_CLIENT_DIFFERENTIAL_EVIDENCE: &[MatureClientDifferentialEvidence] = &[
    MatureClientDifferentialEvidence {
        surface: "postgres",
        protocol: "tcp",
        profile: Some("tokio-postgres"),
        client: "tokio-postgres",
        client_kind: DifferentialClientKind::OfficialSdk,
        status: CertificationStatus::Passed,
        evidence: "crates/loom-hosted/src/pg_wire.rs",
        drift_policy: "bounded protocol profile with checked transcript fixtures",
        stable_error: None,
    },
    MatureClientDifferentialEvidence {
        surface: "postgres",
        protocol: "libpq",
        profile: Some("psql"),
        client: "psql",
        client_kind: DifferentialClientKind::MatureClient,
        status: CertificationStatus::Skipped,
        evidence: "crates/loom-conformance/src/lib.rs",
        drift_policy: "guarded optional tool transcript must stay skipped unless psql is present",
        stable_error: None,
    },
    MatureClientDifferentialEvidence {
        surface: "fts",
        protocol: "opensearch-rest",
        profile: Some("opensearch"),
        client: "opensearch-rust",
        client_kind: DifferentialClientKind::OfficialSdk,
        status: CertificationStatus::Passed,
        evidence: "crates/loom-hosted/src/serve.rs",
        drift_policy: "bounded OpenSearch profile with unsupported security mutation boundaries",
        stable_error: None,
    },
    MatureClientDifferentialEvidence {
        surface: "s3",
        protocol: "aws-cli",
        profile: Some("s3-compatible"),
        client: "aws-cli",
        client_kind: DifferentialClientKind::MatureClient,
        status: CertificationStatus::Skipped,
        evidence: "crates/loom-conformance/src/lib.rs",
        drift_policy: "guarded optional AWS CLI transcript must stay skipped unless the tool is present",
        stable_error: None,
    },
    MatureClientDifferentialEvidence {
        surface: "mail",
        protocol: "jmap",
        profile: Some("rfc8620-rfc8621"),
        client: "RFC 8620/8621 executable transcript",
        client_kind: DifferentialClientKind::ProtocolTool,
        status: CertificationStatus::Passed,
        evidence: "crates/loom-hosted/src/jmap.rs",
        drift_policy: "protocol transcript is accepted until an external JMAP client is available",
        stable_error: None,
    },
    MatureClientDifferentialEvidence {
        surface: "vector",
        protocol: "qdrant-pinecone",
        profile: Some("generated-client"),
        client: "generated vector client",
        client_kind: DifferentialClientKind::GeneratedClient,
        status: CertificationStatus::Target,
        evidence: "specs/facet-bindings/P9-0012-vector-binding.md",
        drift_policy: "generated clients do not promote compatibility without real client evidence",
        stable_error: Some(Code::Unsupported),
    },
];

pub fn run_mature_client_differential_evidence_gate(
    rows: &[MatureClientDifferentialEvidence],
) -> Result<()> {
    let mut keys = BTreeSet::new();
    for row in rows {
        for (field, value) in [
            ("surface", row.surface),
            ("protocol", row.protocol),
            ("client", row.client),
            ("evidence", row.evidence),
            ("drift_policy", row.drift_policy),
        ] {
            if value.is_empty() {
                return Err(LoomError::invalid(format!(
                    "mature-client evidence {field} is empty",
                )));
            }
        }
        if !keys.insert((row.surface, row.protocol, row.profile, row.client)) {
            return Err(LoomError::invalid(
                "duplicate mature-client differential evidence row",
            ));
        }
        match row.status {
            CertificationStatus::Passed => {
                if !matches!(
                    row.client_kind,
                    DifferentialClientKind::MatureClient
                        | DifferentialClientKind::OfficialSdk
                        | DifferentialClientKind::ProtocolTool
                ) {
                    return Err(LoomError::invalid(
                        "passed compatibility evidence requires a real client, official SDK, or protocol tool",
                    ));
                }
                if row.evidence.starts_with("specs/") {
                    return Err(LoomError::invalid(
                        "passed compatibility evidence must cite executable source or fixture evidence",
                    ));
                }
                if row.stable_error.is_some() {
                    return Err(LoomError::invalid(
                        "passed compatibility evidence must not carry a stable error",
                    ));
                }
            }
            CertificationStatus::Unsupported => {
                if row.stable_error != Some(Code::Unsupported) {
                    return Err(LoomError::invalid(
                        "unsupported compatibility evidence must carry UNSUPPORTED",
                    ));
                }
            }
            CertificationStatus::Target => {
                if row.client_kind == DifferentialClientKind::GeneratedClient
                    && row.stable_error != Some(Code::Unsupported)
                {
                    return Err(LoomError::invalid(
                        "target generated-client evidence must carry the unsupported promotion boundary",
                    ));
                }
            }
            CertificationStatus::Failed => {
                if row.stable_error.is_none() {
                    return Err(LoomError::invalid(
                        "failed compatibility evidence must carry a stable error",
                    ));
                }
            }
            CertificationStatus::Degraded | CertificationStatus::Skipped => {}
        }
    }
    Ok(())
}

pub fn run_capability_transcript_evidence_gate(
    capability_rows: &[CapabilityMatrixRow],
    transcript_rows: &[TranscriptInventoryReport],
    differential_rows: &[MatureClientDifferentialEvidence],
) -> Result<()> {
    for row in capability_rows {
        let Some(profile) = row.profile else {
            continue;
        };
        if row.status != HostedProtocolStatus::Supported {
            continue;
        }
        let has_transcript = transcript_rows.iter().any(|transcript| {
            transcript.status == CertificationStatus::Passed
                && COMPATIBILITY_PROFILE_TRANSCRIPT_EVIDENCE
                    .iter()
                    .any(|mapping| {
                        mapping.surface == row.surface
                            && mapping.surface == transcript.surface
                            && mapping.profile == profile
                            && mapping.protocol == transcript.protocol
                            && mapping.transcript_name == transcript.name
                    })
        });
        let has_differential = differential_rows.iter().any(|evidence| {
            evidence.surface == row.surface
                && evidence.profile == Some(profile)
                && evidence.status == CertificationStatus::Passed
                && matches!(
                    evidence.client_kind,
                    DifferentialClientKind::MatureClient
                        | DifferentialClientKind::OfficialSdk
                        | DifferentialClientKind::ProtocolTool
                )
        });
        if !(has_transcript || has_differential) {
            return Err(LoomError::invalid(format!(
                "capability profile {}/{} cannot be supported without transcript evidence",
                row.surface, profile
            )));
        }
    }
    Ok(())
}

pub fn run_real_listener_transcript_scenarios<F>(
    scenarios: &[RealListenerTranscriptScenario],
    mut run: F,
) -> Result<Vec<RealListenerTranscriptOutcome>>
where
    F: FnMut(&RealListenerTranscriptScenario) -> Result<RealListenerTranscriptObservation>,
{
    let mut outcomes = Vec::with_capacity(scenarios.len());
    for scenario in scenarios {
        validate_real_listener_transcript_scenario(scenario)?;
        let observation = run(scenario)?;
        validate_real_listener_transcript_observation(scenario, &observation)?;
        outcomes.push(RealListenerTranscriptOutcome {
            name: scenario.name,
            class: scenario.class,
            status: observation.status,
            event_count: observation.events.len(),
            stable_error: observation.stable_error,
        });
    }
    Ok(outcomes)
}

fn validate_real_listener_transcript_scenario(
    scenario: &RealListenerTranscriptScenario,
) -> Result<()> {
    for (field, value) in [
        ("name", scenario.name),
        ("surface", scenario.surface),
        ("protocol", scenario.protocol),
        ("client", scenario.client),
        ("evidence", scenario.evidence),
    ] {
        if value.is_empty() {
            return Err(LoomError::invalid(format!(
                "real-listener transcript scenario {field} is empty",
            )));
        }
    }
    match scenario.class {
        RealListenerTranscriptClass::Supported => {
            if scenario.expected_status != CertificationStatus::Passed {
                return Err(LoomError::invalid(
                    "supported real-listener transcripts must expect passed status",
                ));
            }
            if scenario.evidence_kind != TranscriptEvidenceKind::RealListener {
                return Err(LoomError::invalid(
                    "supported transcript evidence must come from a real listener",
                ));
            }
        }
        RealListenerTranscriptClass::Guarded => {
            if !matches!(
                scenario.evidence_kind,
                TranscriptEvidenceKind::GuardedOptionalTool | TranscriptEvidenceKind::RealListener
            ) {
                return Err(LoomError::invalid(
                    "guarded transcripts must use a real listener or guarded optional tool",
                ));
            }
        }
        RealListenerTranscriptClass::Unavailable => {
            if scenario.evidence_kind != TranscriptEvidenceKind::UnavailableListener {
                return Err(LoomError::invalid(
                    "unavailable transcripts must record listener unavailability",
                ));
            }
        }
        RealListenerTranscriptClass::Unsupported => {
            if scenario.evidence_kind != TranscriptEvidenceKind::UnsupportedBoundary {
                return Err(LoomError::invalid(
                    "unsupported transcripts must record an unsupported boundary",
                ));
            }
        }
    }
    Ok(())
}

fn validate_real_listener_transcript_observation(
    scenario: &RealListenerTranscriptScenario,
    observation: &RealListenerTranscriptObservation,
) -> Result<()> {
    if observation.reason.is_empty() {
        return Err(LoomError::invalid(format!(
            "{} transcript observation reason is empty",
            scenario.name
        )));
    }
    if observation.status != scenario.expected_status {
        return Err(LoomError::invalid(format!(
            "{} transcript status drift",
            scenario.name
        )));
    }
    match scenario.class {
        RealListenerTranscriptClass::Supported => {
            if observation.events.is_empty() {
                return Err(LoomError::invalid(format!(
                    "{} supported transcript captured no events",
                    scenario.name
                )));
            }
            if observation.stable_error.is_some() {
                return Err(LoomError::invalid(format!(
                    "{} supported transcript carried a stable error",
                    scenario.name
                )));
            }
        }
        RealListenerTranscriptClass::Guarded => {
            if !matches!(
                observation.status,
                CertificationStatus::Passed
                    | CertificationStatus::Skipped
                    | CertificationStatus::Target
                    | CertificationStatus::Degraded
            ) {
                return Err(LoomError::invalid(format!(
                    "{} guarded transcript status is not guarded",
                    scenario.name
                )));
            }
        }
        RealListenerTranscriptClass::Unavailable => {
            if !matches!(observation.stable_error, Some(Code::Unavailable)) {
                return Err(LoomError::invalid(format!(
                    "{} unavailable transcript must expose UNAVAILABLE",
                    scenario.name
                )));
            }
        }
        RealListenerTranscriptClass::Unsupported => {
            if observation.status != CertificationStatus::Unsupported {
                return Err(LoomError::invalid(format!(
                    "{} unsupported transcript status drift",
                    scenario.name
                )));
            }
            if !matches!(observation.stable_error, Some(Code::Unsupported)) {
                return Err(LoomError::invalid(format!(
                    "{} unsupported transcript must expose UNSUPPORTED",
                    scenario.name
                )));
            }
        }
    }
    for event in &observation.events {
        if event.direction.is_empty() || event.protocol.is_empty() || event.summary.is_empty() {
            return Err(LoomError::invalid(format!(
                "{} transcript event has an empty normalized field",
                scenario.name
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Queue7PimCapabilityFixtureRow {
    pub category: &'static str,
    pub surface: &'static str,
    pub protocol: &'static str,
    pub feature: &'static str,
    pub status: &'static str,
    pub evidence: &'static str,
}

const PROGRAM_LIFECYCLE_C_ABI_COVERAGE: &str = "program-lifecycle-c-abi";
const CORE_C_ABI_COVERAGE: &[&str] = &["c-abi", "ffi-tests", PROGRAM_LIFECYCLE_C_ABI_COVERAGE];
const RESULT_CODEC_COVERAGE: &[&str] = &["result-cbor", "bridge-json"];
const VECTOR_SOURCE_MODEL_COVERAGE: &str = "vector-source-model";
const IDENTITY_ACL_ADMIN_COVERAGE: &str = "identity-acl-admin";
const ROLE_ACL_ADMIN_COVERAGE: &str = "role-acl-admin";
const SCOPED_ACL_PROTECTED_REF_COVERAGE: &str = "scoped-acl-protected-ref";
const SESSION_AUTH_COVERAGE: &str = "session-auth";
const AUTHENTICATED_SQL_SESSION_COVERAGE: &str = "authenticated-sql-session";
const AUTHENTICATED_FACET_OPS_COVERAGE: &str = "authenticated-facet-ops";
const LOCAL_DAEMON_LOCK_CLIENT_COVERAGE: &str = "local-daemon-lock-client";
const TYPED_LOCK_TOKEN_COVERAGE: &str = "typed-lock-token";
const SCOPED_LOCK_HELPER_COVERAGE: &str = "scoped-lock-helper";
const HOSTED_AUTH_ACL_COVERAGE: &[&str] = &[
    "hosted-auth-failure",
    "hosted-permission-denial",
    "hosted-stable-error-mapping",
    "hosted-security-audit",
    "hosted-cas-rest",
    "hosted-fips-listener-rejection",
    "hosted-admin-rest",
    "hosted-admin-json-rpc",
];
const HOSTED_PIM_PROTOCOL_COVERAGE: &[&str] = &[
    "mail-imap-bounded-rfc9051-profile",
    "mail-imap-durable-uid-state",
    "mail-imap-durable-subscriptions",
    "mail-imap-common-search-status-fetch-store",
    "mail-imap-direct-rustls-imaps",
    "calendar-caldav-bounded-webdav-profile",
    "calendar-caldav-direct-tls",
    "contacts-carddav-bounded-webdav-profile",
    "contacts-carddav-direct-tls",
    "mail-jmap-bounded-rfc8620-rfc8621-profile",
    "mail-jmap-blob-upload-download-email-import",
    "mail-jmap-identity-and-deterministic-state",
    "mail-jmap-email-changes-querychanges",
    "mail-jmap-executable-rfc8620-rfc8621-transcript",
    "mail-jmap-push-unsupported",
    "mail-jmap-direct-tls",
    "mail-smtp-setup-compatibility-listener",
    "mail-smtp-real-submission-unsupported",
    "pim-rfc-gate-shared-http-uri-webdav-basic-auth",
    "pim-rfc-gate-uri-rfc3986-percent-encoding",
    "pim-rfc-gate-webdav-rfc4918-base-methods",
    "pim-rfc-gate-webdav-rfc5397-current-user-principal",
    "pim-rfc-gate-webdav-rfc5689-extended-mkcol-unsupported",
    "pim-rfc-gate-well-known-rfc5785-caldav-carddav",
    "pim-rfc-gate-well-known-rfc8615-caldav-carddav",
    "pim-rfc-gate-webdav-rfc6578-sync-collection",
    "pim-rfc-gate-service-discovery-rfc6764-degraded",
    "pim-rfc-gate-http-basic-rfc7617-degraded",
    "pim-rfc-gate-tls-rfc8996-modern-versions",
    "pim-rfc-gate-http-semantics-rfc9110-bounded-profile",
    "pim-rfc-gate-http1-rfc9112-shared-stack",
    "pim-rfc-gate-shared-tls-service-identity-target",
    "pim-rfc-gate-shared-dns-discovery-target",
    "calendar-caldav-rfc4791-bounded-access-profile",
    "calendar-icalendar-rfc5545-bounded-profile",
    "calendar-caldav-rfc4791-rfc5545-bounded-profile",
    "calendar-itip-rfc5546-target",
    "calendar-imip-rfc6047-target",
    "calendar-caldav-scheduling-rfc6638-target",
    "calendar-caldav-scheduling-itip-imip-target",
    "calendar-non-gregorian-recurrence-rfc7529-unsupported",
    "calendar-caldav-non-gregorian-recurrence-unsupported",
    "calendar-timezone-reference-rfc7809-target",
    "calendar-availability-rfc7953-target",
    "calendar-caldav-timezone-reference-availability-target",
    "calendar-caldav-rfc7986-extra-properties",
    "contacts-carddav-rfc6352-bounded-access-profile",
    "contacts-vcard-rfc6350-bounded-profile",
    "contacts-carddav-rfc6352-rfc6350-bounded-profile",
    "contacts-xcard-rfc6351-unsupported",
    "contacts-carddav-xcard-unsupported",
    "contacts-place-death-extensions-rfc6474-target",
    "contacts-carddav-place-death-extensions-target",
    "contacts-parameter-caret-encoding-rfc6868-target",
    "contacts-carddav-parameter-caret-encoding-target",
    "contacts-carddav-vcard3-dialect-supported",
    "mail-message-rfc5322-bounded-profile",
    "mail-rfc-gate-imap-rfc9051-bounded-profile",
    "mail-rfc-gate-jmap-core-rfc8620-bounded-profile",
    "mail-rfc-gate-jmap-mail-rfc8621-bounded-profile",
    "mail-rfc-gate-jmap-rfc8620-rfc8621-bounded-profile",
    "mail-rfc-gate-jmap-blob-rfc9404-bounded-profile",
    "mail-rfc-gate-jmap-quotas-rfc9425-bounded-profile",
    "mail-rfc-gate-jscontact-rfc9553-target",
    "mail-rfc-gate-vcard-jscontact-extensions-rfc9554-target",
    "mail-rfc-gate-jscontact-vcard-conversion-rfc9555-target",
    "mail-rfc-gate-jmap-contacts-rfc9610-target",
    "mail-rfc-gate-jscalendar-rfc8984-target",
    "mail-rfc-gate-jmap-calendars-draft-ietf-jmap-calendars-target",
    "mail-rfc-gate-jmap-sharing-rfc9670-unsupported",
    "mail-rfc-gate-web-push-rfc8030-target",
    "mail-rfc-gate-vapid-rfc8292-target",
    "mail-rfc-gate-jmap-webpush-vapid-rfc9749-target",
    "mail-rfc-gate-smtp-rfc5321-setup-profile",
    "mail-rfc-gate-smtp-submission-rfc6409-setup-profile",
    "mail-rfc-gate-smtp-starttls-rfc3207-setup-profile",
    "mail-rfc-gate-email-tls-rfc8314-bounded-profile",
    "mail-rfc-gate-email-submission-ops-rfc5068-target",
    "mail-rfc-gate-smtp-auth-rfc4954-bounded-profile",
    "mail-rfc-gate-sasl-rfc4422-bounded-profile",
    "mail-rfc-gate-sasl-plain-rfc4616-bounded-profile",
    "mail-rfc-gate-mailto-rfc6068-unsupported",
    "mail-rfc-gate-smtp-size-rfc1870-bounded-profile",
    "mail-rfc-gate-smtp-pipelining-rfc2920-target",
    "mail-rfc-gate-enhanced-status-codes-rfc3463-target",
    "mail-rfc-gate-enhanced-status-registry-rfc5248-target",
    "mail-rfc-gate-smtp-8bitmime-rfc6152-bounded-profile",
    "mail-rfc-gate-smtputf8-rfc6531-target",
    "mail-rfc-gate-internationalized-headers-rfc6532-target",
    "mail-rfc-gate-mime-format-rfc2045-target",
    "mail-rfc-gate-mime-media-types-rfc2046-target",
    "mail-rfc-gate-mime-encoded-words-rfc2047-target",
    "mail-rfc-gate-mime-conformance-rfc2049-target",
    "mail-rfc-gate-smtp-setup-auth-session-profile",
    "mail-rfc-gate-smtp-starttls-standard-port-transcript",
    "mail-rfc-gate-smtp-optional-extensions-mixed-profile",
    "pim-rfc-gate-live-local-probes",
    "mail-mutable-state-policy-version-deltas",
    "mail-mutable-state-merge-audit",
    "mail-mutable-state-compaction-retained-gap",
    "pim-owner-only-access-profile",
    "pim-certification-profile-transcript-capture",
    "pim-hooks-registration-envelope-event-emission",
    "pim-hooks-execution-policy-planning",
    "pim-reference-client-targets",
];
const PIM_CERTIFICATION_REDACT: &[&str] = &[
    "authorization",
    "cookie",
    "set-cookie",
    "access-token",
    "refresh-token",
    "client-secret",
    "password",
    "private-key",
    "raw-message-body",
    "raw-calendar-body",
    "raw-contact-body",
];
const PIM_CERTIFICATION_RETAIN: &[&str] = &[
    "method",
    "path-template",
    "status",
    "capability",
    "etag-presence",
    "sync-token-presence",
    "state-token-presence",
    "uid-shape",
    "error-code",
];
const PIM_CERTIFICATION_SUITES: &[&str] = &[
    "caldav-reference-clients",
    "carddav-reference-clients",
    "imap-reference-clients",
    "jmap-rfc8620-rfc8621-transcripts",
];
const PIM_REDACTION_POLICY: TranscriptRedactionPolicyReport = TranscriptRedactionPolicyReport {
    name: "pim-owner-only-redacted-transcripts-v1",
    retention: "release-evidence",
    redact: PIM_CERTIFICATION_REDACT,
    retain: PIM_CERTIFICATION_RETAIN,
};
pub const PIM_CERTIFICATION_CLIENT_REQUIREMENTS: &[CertificationClientRequirement] = &[
    CertificationClientRequirement {
        surface: "calendar",
        protocol: "caldav",
        client: "Apple Calendar",
        platform: "macOS-or-iOS",
        role: "required-apple",
        status: CertificationStatus::Target,
        evidence: "specs/0037-calendar-layer.md",
    },
    CertificationClientRequirement {
        surface: "calendar",
        protocol: "caldav",
        client: "Thunderbird",
        platform: "desktop-cross-platform",
        role: "required-cross-platform",
        status: CertificationStatus::Target,
        evidence: "specs/0037-calendar-layer.md",
    },
    CertificationClientRequirement {
        surface: "calendar",
        protocol: "caldav",
        client: "DAVx5",
        platform: "android",
        role: "required-mobile-cross-platform",
        status: CertificationStatus::Target,
        evidence: "specs/0037-calendar-layer.md",
    },
    CertificationClientRequirement {
        surface: "contacts",
        protocol: "carddav",
        client: "Apple Contacts",
        platform: "macOS-or-iOS",
        role: "required-apple",
        status: CertificationStatus::Target,
        evidence: "specs/0038-contacts-layer.md",
    },
    CertificationClientRequirement {
        surface: "contacts",
        protocol: "carddav",
        client: "Thunderbird",
        platform: "desktop-cross-platform",
        role: "required-cross-platform",
        status: CertificationStatus::Target,
        evidence: "specs/0038-contacts-layer.md",
    },
    CertificationClientRequirement {
        surface: "contacts",
        protocol: "carddav",
        client: "DAVx5",
        platform: "android",
        role: "required-mobile-cross-platform",
        status: CertificationStatus::Target,
        evidence: "specs/0038-contacts-layer.md",
    },
    CertificationClientRequirement {
        surface: "mail",
        protocol: "imap",
        client: "Apple Mail",
        platform: "macOS-or-iOS",
        role: "required-apple",
        status: CertificationStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    CertificationClientRequirement {
        surface: "mail",
        protocol: "imap",
        client: "Thunderbird",
        platform: "desktop-cross-platform",
        role: "required-cross-platform",
        status: CertificationStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    CertificationClientRequirement {
        surface: "mail",
        protocol: "jmap",
        client: "RFC 8620/8621 executable transcript",
        platform: "protocol-tooling",
        role: "required-protocol-transcript",
        status: CertificationStatus::Passed,
        evidence: "crates/loom-hosted/src/jmap.rs",
    },
];
pub const PIM_TRANSCRIPT_INVENTORY: &[TranscriptInventoryReport] = &[
    TranscriptInventoryReport {
        name: "caldav-apple-calendar-owner-only",
        surface: "calendar",
        protocol: "caldav",
        client: "Apple Calendar",
        status: CertificationStatus::Target,
        reason: "reference-client transcript has not been captured",
        evidence: "specs/0037-calendar-layer.md",
        redaction_profile: "pim-owner-only-redacted-transcripts-v1",
    },
    TranscriptInventoryReport {
        name: "caldav-thunderbird-owner-only",
        surface: "calendar",
        protocol: "caldav",
        client: "Thunderbird",
        status: CertificationStatus::Target,
        reason: "reference-client transcript has not been captured",
        evidence: "specs/0037-calendar-layer.md",
        redaction_profile: "pim-owner-only-redacted-transcripts-v1",
    },
    TranscriptInventoryReport {
        name: "caldav-davx5-owner-only",
        surface: "calendar",
        protocol: "caldav",
        client: "DAVx5",
        status: CertificationStatus::Target,
        reason: "reference-client transcript has not been captured",
        evidence: "specs/0037-calendar-layer.md",
        redaction_profile: "pim-owner-only-redacted-transcripts-v1",
    },
    TranscriptInventoryReport {
        name: "carddav-apple-contacts-owner-only",
        surface: "contacts",
        protocol: "carddav",
        client: "Apple Contacts",
        status: CertificationStatus::Target,
        reason: "reference-client transcript has not been captured",
        evidence: "specs/0038-contacts-layer.md",
        redaction_profile: "pim-owner-only-redacted-transcripts-v1",
    },
    TranscriptInventoryReport {
        name: "carddav-thunderbird-owner-only",
        surface: "contacts",
        protocol: "carddav",
        client: "Thunderbird",
        status: CertificationStatus::Target,
        reason: "reference-client transcript has not been captured",
        evidence: "specs/0038-contacts-layer.md",
        redaction_profile: "pim-owner-only-redacted-transcripts-v1",
    },
    TranscriptInventoryReport {
        name: "carddav-davx5-owner-only",
        surface: "contacts",
        protocol: "carddav",
        client: "DAVx5",
        status: CertificationStatus::Target,
        reason: "reference-client transcript has not been captured",
        evidence: "specs/0038-contacts-layer.md",
        redaction_profile: "pim-owner-only-redacted-transcripts-v1",
    },
    TranscriptInventoryReport {
        name: "imap-apple-mail-owner-only",
        surface: "mail",
        protocol: "imap",
        client: "Apple Mail",
        status: CertificationStatus::Target,
        reason: "reference-client transcript has not been captured",
        evidence: "specs/0039-mail-layer.md",
        redaction_profile: "pim-owner-only-redacted-transcripts-v1",
    },
    TranscriptInventoryReport {
        name: "imap-thunderbird-owner-only",
        surface: "mail",
        protocol: "imap",
        client: "Thunderbird",
        status: CertificationStatus::Target,
        reason: "reference-client transcript has not been captured",
        evidence: "specs/0039-mail-layer.md",
        redaction_profile: "pim-owner-only-redacted-transcripts-v1",
    },
    TranscriptInventoryReport {
        name: "jmap-rfc8620-rfc8621-owner-only",
        surface: "mail",
        protocol: "jmap",
        client: "RFC 8620/8621 executable transcript",
        status: CertificationStatus::Passed,
        reason: "hosted router transcript executes the source-backed JMAP subset",
        evidence: "crates/loom-hosted/src/jmap.rs",
        redaction_profile: "pim-owner-only-redacted-transcripts-v1",
    },
    TranscriptInventoryReport {
        name: "pim-live-rfc-probes-owner-only",
        surface: "pim",
        protocol: "rfc-gate",
        client: "scripts/pim-cert/rfc-probe.sh",
        status: CertificationStatus::Passed,
        reason: "local live daemon probe verifies bounded CalDAV, CardDAV, IMAP, JMAP, and SMTP STARTTLS fixture visibility",
        evidence: "scripts/pim-cert/rfc-probe.sh",
        redaction_profile: "pim-owner-only-redacted-transcripts-v1",
    },
];
pub const QUEUE7_PIM_CAPABILITY_REPORT_FIXTURES: &[Queue7PimCapabilityFixtureRow] = &[
    Queue7PimCapabilityFixtureRow {
        category: "capability",
        surface: "hosted-pim-protocols",
        protocol: "binding-inventory",
        feature: "queue7-pim-protocol-coverage",
        status: "supported",
        evidence: "crates/loom-conformance/src/lib.rs",
    },
    Queue7PimCapabilityFixtureRow {
        category: "supported",
        surface: "calendar",
        protocol: "caldav",
        feature: "bounded-webdav-resource-profile",
        status: "supported",
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    Queue7PimCapabilityFixtureRow {
        category: "degraded",
        surface: "contacts",
        protocol: "rfc-gate",
        feature: "carddav-rfc6352-bounded-access-profile",
        status: "degraded",
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    Queue7PimCapabilityFixtureRow {
        category: "target",
        surface: "calendar",
        protocol: "rfc-gate",
        feature: "itip-rfc5546",
        status: "target",
        evidence: "specs/0037-calendar-layer.md",
    },
    Queue7PimCapabilityFixtureRow {
        category: "unsupported",
        surface: "contacts",
        protocol: "rfc-gate",
        feature: "xcard-rfc6351",
        status: "unsupported",
        evidence: "specs/0038-contacts-layer.md",
    },
    Queue7PimCapabilityFixtureRow {
        category: "status-shape",
        surface: "pim",
        protocol: "queue2-fixture",
        feature: "failed-row-shape",
        status: "failed",
        evidence: "specs/0010a-conformance-reporting-and-certification.md",
    },
    Queue7PimCapabilityFixtureRow {
        category: "status-shape",
        surface: "pim",
        protocol: "queue2-fixture",
        feature: "skipped-row-shape",
        status: "skipped",
        evidence: "specs/0010a-conformance-reporting-and-certification.md",
    },
    Queue7PimCapabilityFixtureRow {
        category: "certification",
        surface: "pim",
        protocol: "certification",
        feature: "profile-transcript-capture",
        status: "supported",
        evidence: "crates/loom-conformance/src/lib.rs",
    },
    Queue7PimCapabilityFixtureRow {
        category: "direct-tls",
        surface: "calendar",
        protocol: "caldav",
        feature: "direct-tls",
        status: "supported",
        evidence: "crates/loom-cli/src/daemon_cmd.rs",
    },
    Queue7PimCapabilityFixtureRow {
        category: "retained-gap",
        surface: "mail",
        protocol: "mutable-state",
        feature: "flag-compaction-retained-gap",
        status: "supported",
        evidence: "crates/loom-core/src/mail.rs",
    },
    Queue7PimCapabilityFixtureRow {
        category: "lifecycle",
        surface: "pim",
        protocol: "hooks",
        feature: "registration-envelope-event-emission",
        status: "supported",
        evidence: "crates/loom-core/src/hooks.rs",
    },
    Queue7PimCapabilityFixtureRow {
        category: "rfc-gate",
        surface: "pim",
        protocol: "rfc-gate",
        feature: "uri-rfc3986-percent-encoding",
        status: "supported",
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    Queue7PimCapabilityFixtureRow {
        category: "client-transcript",
        surface: "mail",
        protocol: "jmap",
        feature: "jmap-rfc8620-rfc8621-owner-only",
        status: "passed",
        evidence: "crates/loom-hosted/src/jmap.rs",
    },
];
const NODE_RUNTIME_COVERAGE: &[&str] = &[
    "version",
    "blob-digest",
    "capabilities",
    "sql-result-vector",
    "sql-session",
    "sql-query-iterator",
    "sql-batch",
    "key-wrap-management",
    "identity",
    "acl",
    IDENTITY_ACL_ADMIN_COVERAGE,
    ROLE_ACL_ADMIN_COVERAGE,
    AUTHENTICATED_SQL_SESSION_COVERAGE,
    "queue",
    "queue-consumer",
    "calendar",
    "contacts",
    "mail",
    "workspace-lifecycle",
    "direct-table-readers",
    "merge-status",
    "staging",
    "files",
    "byte-range-files",
    "file-handles",
    LOCAL_DAEMON_LOCK_CLIENT_COVERAGE,
    TYPED_LOCK_TOKEN_COVERAGE,
    SCOPED_LOCK_HELPER_COVERAGE,
    "symlink",
    "tags",
    "restore",
    "replay",
    "squash",
];
const PYTHON_RUNTIME_COVERAGE: &[&str] = &[
    "version",
    "blob-digest",
    "capabilities",
    "sql-result-vector",
    "sql-session",
    "sql-async-session",
    "sql-query-iterator",
    "sql-batch",
    "key-wrap-management",
    "identity",
    "acl",
    IDENTITY_ACL_ADMIN_COVERAGE,
    ROLE_ACL_ADMIN_COVERAGE,
    AUTHENTICATED_SQL_SESSION_COVERAGE,
    "cas",
    "files",
    "byte-range-files",
    "file-handles",
    LOCAL_DAEMON_LOCK_CLIENT_COVERAGE,
    TYPED_LOCK_TOKEN_COVERAGE,
    SCOPED_LOCK_HELPER_COVERAGE,
    "symlink",
    "tags",
    "staging",
    "merge-status",
    "queue",
    "queue-consumer",
    "workspace-lifecycle",
    "direct-table-readers",
    "calendar",
    "contacts",
    "mail",
    "graph",
    "vector",
    "columnar",
    "search",
];
const IOS_RUNTIME_COVERAGE: &[&str] = &[
    "version",
    "blob-digest",
    "sql-result-vector",
    "direct-table-readers",
    IDENTITY_ACL_ADMIN_COVERAGE,
    SCOPED_ACL_PROTECTED_REF_COVERAGE,
    SESSION_AUTH_COVERAGE,
    "cas",
    VECTOR_SOURCE_MODEL_COVERAGE,
];
const CPP_RUNTIME_COVERAGE: &[&str] = &[
    "version",
    "blob-digest",
    "workspace-lifecycle",
    IDENTITY_ACL_ADMIN_COVERAGE,
    SESSION_AUTH_COVERAGE,
    LOCAL_DAEMON_LOCK_CLIENT_COVERAGE,
    TYPED_LOCK_TOKEN_COVERAGE,
    SCOPED_LOCK_HELPER_COVERAGE,
    "sql-result-view",
    "sql-session",
    "cas",
    "queue",
    "queue-consumer",
    VECTOR_SOURCE_MODEL_COVERAGE,
];
const JVM_RUNTIME_COVERAGE: &[&str] = &[
    "version",
    "blob-digest",
    "workspace-lifecycle",
    IDENTITY_ACL_ADMIN_COVERAGE,
    SCOPED_ACL_PROTECTED_REF_COVERAGE,
    SESSION_AUTH_COVERAGE,
    AUTHENTICATED_FACET_OPS_COVERAGE,
    AUTHENTICATED_SQL_SESSION_COVERAGE,
    LOCAL_DAEMON_LOCK_CLIENT_COVERAGE,
    TYPED_LOCK_TOKEN_COVERAGE,
    SCOPED_LOCK_HELPER_COVERAGE,
    "sql-result-view",
    "sql-session",
    "cas",
    "queue",
    "queue-consumer",
    VECTOR_SOURCE_MODEL_COVERAGE,
];
const ANDROID_RUNTIME_COVERAGE: &[&str] = &[
    "version",
    "blob-digest",
    "workspace-lifecycle",
    IDENTITY_ACL_ADMIN_COVERAGE,
    SCOPED_ACL_PROTECTED_REF_COVERAGE,
    SESSION_AUTH_COVERAGE,
    AUTHENTICATED_FACET_OPS_COVERAGE,
    "sql-result-view",
    "sql-session",
    "cas",
    "queue",
    "queue-consumer",
    VECTOR_SOURCE_MODEL_COVERAGE,
];
const REACT_NATIVE_RUNTIME_COVERAGE: &[&str] = &[
    "version",
    "blob-digest",
    "workspace-lifecycle",
    IDENTITY_ACL_ADMIN_COVERAGE,
    SCOPED_ACL_PROTECTED_REF_COVERAGE,
    SESSION_AUTH_COVERAGE,
    AUTHENTICATED_FACET_OPS_COVERAGE,
    "sql-history-readers",
    "cas",
    "queue",
    "queue-consumer",
    "vector-metadata-indexes",
    "vector-search-policy",
    VECTOR_SOURCE_MODEL_COVERAGE,
];
const WASM_RUNTIME_COVERAGE: &[&str] = &[
    "version",
    "blob-digest",
    "workspace-lifecycle",
    IDENTITY_ACL_ADMIN_COVERAGE,
    SCOPED_ACL_PROTECTED_REF_COVERAGE,
    SESSION_AUTH_COVERAGE,
    "sql-result-view",
    "sql-session",
    "cas",
    "queue",
    "queue-consumer",
    "browser-worker-opfs",
];
const NODE_PYTHON_INTEROP_COVERAGE: &[&str] = &[
    "shared-store-open-read-write",
    "sql-cross-binding-rows",
    "document-cross-binding-text",
];

/// The binding conformance inventory: every binding-adjacent surface and the checked-in evidence that
/// places it in a [`BindingTier`]. This is a factual map of what is exercised, not a promotion claim:
/// a surface is reported as passed only when the current Rust certification executes it.
pub const BINDING_CONFORMANCE_INVENTORY: &[BindingSurface] = &[
    // Executable core suites gated by the Rust workspace.
    BindingSurface {
        name: "canonical-vectors",
        tier: BindingTier::ExecutableCore,
        evidence: "crates/loom-conformance/src/lib.rs",
        coverage: CANONICAL_VECTOR_SUITES,
    },
    BindingSurface {
        name: "c-abi",
        tier: BindingTier::ExecutableCore,
        evidence: "crates/loom-ffi/src/lib.rs",
        coverage: CORE_C_ABI_COVERAGE,
    },
    BindingSurface {
        name: "result-codec",
        tier: BindingTier::ExecutableCore,
        evidence: "bindings/conformance/result-vectors.json",
        coverage: RESULT_CODEC_COVERAGE,
    },
    BindingSurface {
        name: "hosted-auth-acl",
        tier: BindingTier::ExecutableCore,
        evidence: "crates/loom-hosted/src/lib.rs",
        coverage: HOSTED_AUTH_ACL_COVERAGE,
    },
    BindingSurface {
        name: "hosted-pim-protocols",
        tier: BindingTier::ExecutableCore,
        evidence: "crates/loom-hosted/src/imap.rs",
        coverage: HOSTED_PIM_PROTOCOL_COVERAGE,
    },
    // (2) Binding runtime/smoke suites that exist today (run via their own toolchain recipe).
    BindingSurface {
        name: "node",
        tier: BindingTier::BindingRuntimeSuite,
        evidence: "bindings/node/test.mjs",
        coverage: NODE_RUNTIME_COVERAGE,
    },
    BindingSurface {
        name: "python",
        tier: BindingTier::BindingRuntimeSuite,
        evidence: "bindings/python/tests/test_loom.py",
        coverage: PYTHON_RUNTIME_COVERAGE,
    },
    BindingSurface {
        name: "ios",
        tier: BindingTier::BindingRuntimeSuite,
        evidence: "bindings/ios/Tests/UldrenLoomTests/LoomTests.swift",
        coverage: IOS_RUNTIME_COVERAGE,
    },
    BindingSurface {
        name: "cpp",
        tier: BindingTier::BindingRuntimeSuite,
        evidence: "bindings/cpp/test/main.cpp",
        coverage: CPP_RUNTIME_COVERAGE,
    },
    BindingSurface {
        name: "jvm",
        tier: BindingTier::BindingRuntimeSuite,
        evidence: "bindings/jvm/src/test/java/ai/uldren/loom/LoomRuntimeSmoke.java",
        coverage: JVM_RUNTIME_COVERAGE,
    },
    BindingSurface {
        name: "android",
        tier: BindingTier::BindingRuntimeSuite,
        evidence: "bindings/android/src/jvmTest/kotlin/ai/uldren/loom/AndroidJvmRuntimeSmokeTest.kt",
        coverage: ANDROID_RUNTIME_COVERAGE,
    },
    BindingSurface {
        name: "react-native",
        tier: BindingTier::BindingRuntimeSuite,
        evidence: "bindings/react-native/host-test/android/app/src/androidTest/java/ai/uldren/loom/rn/host/UldrenLoomHostRuntimeTest.kt",
        coverage: REACT_NATIVE_RUNTIME_COVERAGE,
    },
    BindingSurface {
        name: "wasm",
        tier: BindingTier::BindingRuntimeSuite,
        evidence: "bindings/wasm/browser-test/worker.js",
        coverage: WASM_RUNTIME_COVERAGE,
    },
    // (4) Target-only binding surfaces (specified in 0007, not implemented today).
    BindingSurface {
        name: "generated-idl-bindings",
        tier: BindingTier::TargetOnly,
        evidence: "",
        coverage: &[],
    },
    BindingSurface {
        name: "binding-distribution-packaging",
        tier: BindingTier::TargetOnly,
        evidence: "",
        coverage: &[],
    },
    BindingSurface {
        name: "cross-binding-interop",
        tier: BindingTier::CrossBindingInterop,
        evidence: "scripts/binding-cross-interop.sh",
        coverage: NODE_PYTHON_INTEROP_COVERAGE,
    },
    BindingSurface {
        name: "full-binding-conformance-suite",
        tier: BindingTier::TargetOnly,
        evidence: "",
        coverage: &[],
    },
];

pub const BINDING_RUNTIME_CERTIFICATION: &[BindingRuntimeCertificationRow] = &[
    BindingRuntimeCertificationRow {
        surface: "node",
        target: "node-runtime-suite",
        status: HostedProtocolStatus::Supported,
        workflow: ".github/workflows/bindings.yml",
        evidence: &[
            "bindings/node/test.mjs",
            "justfile",
            ".github/workflows/bindings.yml",
        ],
        coverage: NODE_RUNTIME_COVERAGE,
    },
    BindingRuntimeCertificationRow {
        surface: "python",
        target: "python-runtime-suite",
        status: HostedProtocolStatus::Supported,
        workflow: ".github/workflows/bindings.yml",
        evidence: &[
            "bindings/python/tests/test_loom.py",
            "justfile",
            ".github/workflows/bindings.yml",
        ],
        coverage: PYTHON_RUNTIME_COVERAGE,
    },
    BindingRuntimeCertificationRow {
        surface: "ios",
        target: "swift-runtime-suite",
        status: HostedProtocolStatus::Supported,
        workflow: ".github/workflows/bindings.yml",
        evidence: &[
            "bindings/ios/Tests/UldrenLoomTests/LoomTests.swift",
            "justfile",
            ".github/workflows/bindings.yml",
        ],
        coverage: IOS_RUNTIME_COVERAGE,
    },
    BindingRuntimeCertificationRow {
        surface: "cpp",
        target: "cpp-runtime-suite",
        status: HostedProtocolStatus::Supported,
        workflow: ".github/workflows/ci.yml",
        evidence: &[
            "bindings/cpp/test/main.cpp",
            "justfile",
            ".github/workflows/ci.yml",
        ],
        coverage: CPP_RUNTIME_COVERAGE,
    },
    BindingRuntimeCertificationRow {
        surface: "jvm",
        target: "jvm-runtime-suite",
        status: HostedProtocolStatus::Supported,
        workflow: ".github/workflows/bindings.yml",
        evidence: &[
            "bindings/jvm/src/test/java/ai/uldren/loom/LoomRuntimeSmoke.java",
            "justfile",
            ".github/workflows/bindings.yml",
        ],
        coverage: JVM_RUNTIME_COVERAGE,
    },
    BindingRuntimeCertificationRow {
        surface: "android",
        target: "kotlin-jvm-runtime-suite",
        status: HostedProtocolStatus::Supported,
        workflow: ".github/workflows/bindings.yml",
        evidence: &[
            "bindings/android/src/jvmTest/kotlin/ai/uldren/loom/AndroidJvmRuntimeSmokeTest.kt",
            "justfile",
            ".github/workflows/bindings.yml",
        ],
        coverage: ANDROID_RUNTIME_COVERAGE,
    },
    BindingRuntimeCertificationRow {
        surface: "react-native",
        target: "android-connected-host-fixture",
        status: HostedProtocolStatus::Supported,
        workflow: ".github/workflows/bindings.yml",
        evidence: &[
            "bindings/react-native/host-test/android/app/src/androidTest/java/ai/uldren/loom/rn/host/UldrenLoomHostRuntimeTest.kt",
            "justfile",
            ".github/workflows/bindings.yml",
        ],
        coverage: REACT_NATIVE_RUNTIME_COVERAGE,
    },
    BindingRuntimeCertificationRow {
        surface: "wasm",
        target: "browser-worker-opfs-runtime",
        status: HostedProtocolStatus::Supported,
        workflow: ".github/workflows/bindings.yml",
        evidence: &[
            "bindings/wasm/browser-test/worker.js",
            "bindings/wasm/browser-test/run.mjs",
            "justfile",
            ".github/workflows/bindings.yml",
        ],
        coverage: WASM_RUNTIME_COVERAGE,
    },
];

pub const BINDING_PACKAGE_CERTIFICATION: &[BindingPackageCertificationRow] = &[
    BindingPackageCertificationRow {
        surface: "c-abi",
        package_name: "libuldren_loom",
        package_kind: "native-library",
        build_recipe: "just ffi",
        materials_status: HostedProtocolStatus::Supported,
        compatibility_metadata_status: HostedProtocolStatus::Supported,
        signing_manifest_status: HostedProtocolStatus::Supported,
        publication_status: HostedProtocolStatus::Target,
        evidence: &[
            "scripts/binding-release-materials.sh",
            "justfile",
            "include/loom.h",
        ],
    },
    BindingPackageCertificationRow {
        surface: "node",
        package_name: "@uldrenai/loom",
        package_kind: "npm-native-addon",
        build_recipe: "cd bindings/node && pnpm run build",
        materials_status: HostedProtocolStatus::Supported,
        compatibility_metadata_status: HostedProtocolStatus::Supported,
        signing_manifest_status: HostedProtocolStatus::Supported,
        publication_status: HostedProtocolStatus::Target,
        evidence: &[
            "scripts/binding-release-materials.sh",
            "justfile",
            "bindings/node/package.json",
        ],
    },
    BindingPackageCertificationRow {
        surface: "python",
        package_name: "uldrenai-loom",
        package_kind: "python-native-extension",
        build_recipe: "cd bindings/python && maturin build --release",
        materials_status: HostedProtocolStatus::Supported,
        compatibility_metadata_status: HostedProtocolStatus::Supported,
        signing_manifest_status: HostedProtocolStatus::Supported,
        publication_status: HostedProtocolStatus::Target,
        evidence: &[
            "scripts/binding-release-materials.sh",
            "justfile",
            "bindings/python/pyproject.toml",
        ],
    },
    BindingPackageCertificationRow {
        surface: "cpp",
        package_name: "loom-cpp",
        package_kind: "header-plus-native-library",
        build_recipe: "just cpp",
        materials_status: HostedProtocolStatus::Supported,
        compatibility_metadata_status: HostedProtocolStatus::Supported,
        signing_manifest_status: HostedProtocolStatus::Supported,
        publication_status: HostedProtocolStatus::Target,
        evidence: &[
            "scripts/binding-release-materials.sh",
            "justfile",
            "include/loom.h",
        ],
    },
    BindingPackageCertificationRow {
        surface: "ios",
        package_name: "UldrenLoom",
        package_kind: "swiftpm-plus-native-library",
        build_recipe: "just ios",
        materials_status: HostedProtocolStatus::Supported,
        compatibility_metadata_status: HostedProtocolStatus::Supported,
        signing_manifest_status: HostedProtocolStatus::Supported,
        publication_status: HostedProtocolStatus::Target,
        evidence: &[
            "scripts/binding-release-materials.sh",
            "justfile",
            "bindings/ios/Package.swift",
        ],
    },
    BindingPackageCertificationRow {
        surface: "jvm",
        package_name: "ai.uldren:loom",
        package_kind: "jvm-plus-native-library",
        build_recipe: "just jvm",
        materials_status: HostedProtocolStatus::Supported,
        compatibility_metadata_status: HostedProtocolStatus::Supported,
        signing_manifest_status: HostedProtocolStatus::Supported,
        publication_status: HostedProtocolStatus::Target,
        evidence: &[
            "scripts/binding-release-materials.sh",
            "justfile",
            "bindings/jvm/build.gradle.kts",
        ],
    },
    BindingPackageCertificationRow {
        surface: "android",
        package_name: "ai.uldren:loom-android",
        package_kind: "android-aar-plus-native-library",
        build_recipe: "just android",
        materials_status: HostedProtocolStatus::Supported,
        compatibility_metadata_status: HostedProtocolStatus::Supported,
        signing_manifest_status: HostedProtocolStatus::Supported,
        publication_status: HostedProtocolStatus::Target,
        evidence: &[
            "scripts/binding-release-materials.sh",
            "justfile",
            "bindings/android/build.gradle.kts",
        ],
    },
    BindingPackageCertificationRow {
        surface: "react-native",
        package_name: "@uldrenai/loom-react-native",
        package_kind: "npm-react-native-plus-native-library",
        build_recipe: "just react-native-android",
        materials_status: HostedProtocolStatus::Supported,
        compatibility_metadata_status: HostedProtocolStatus::Supported,
        signing_manifest_status: HostedProtocolStatus::Supported,
        publication_status: HostedProtocolStatus::Target,
        evidence: &[
            "scripts/binding-release-materials.sh",
            "justfile",
            "bindings/react-native/package.json",
        ],
    },
    BindingPackageCertificationRow {
        surface: "wasm",
        package_name: "@uldrenai/loom-wasm",
        package_kind: "wasm-browser-package",
        build_recipe: "just wasm",
        materials_status: HostedProtocolStatus::Supported,
        compatibility_metadata_status: HostedProtocolStatus::Supported,
        signing_manifest_status: HostedProtocolStatus::Supported,
        publication_status: HostedProtocolStatus::Target,
        evidence: &[
            "scripts/binding-release-materials.sh",
            "justfile",
            "bindings/wasm/Cargo.toml",
        ],
    },
];

pub const CAPABILITY_MATRIX: &[CapabilityMatrixRow] = &[
    CapabilityMatrixRow {
        plane: CapabilityPlane::Local,
        surface: "daemon",
        transport: "uds-or-loopback-http",
        profile: None,
        status: HostedProtocolStatus::Supported,
        evidence: &["crates/loom-cli/src/daemon_cmd.rs"],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::Local,
        surface: "lock",
        transport: "cli",
        profile: None,
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-cli/src/main.rs",
            "crates/loom-core/src/lock.rs",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::Local,
        surface: "cluster-coordination",
        transport: "single-node",
        profile: None,
        status: HostedProtocolStatus::Degraded,
        evidence: &["specs/0036a-coordination-substrate.md"],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::Mcp,
        surface: "mcp",
        transport: "stdio-http",
        profile: None,
        status: HostedProtocolStatus::Supported,
        evidence: &["crates/loom-mcp/src/lib.rs"],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::Mcp,
        surface: "mcp-hosted-projection-parity",
        transport: "mcp",
        profile: None,
        status: HostedProtocolStatus::Target,
        evidence: &["crates/loom-conformance/src/behavior/scenarios.rs"],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierOne,
        surface: "cas",
        transport: "rest-json_rpc-grpc",
        profile: None,
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-hosted/src/serve.rs",
            "crates/loom-hosted/src/grpc.rs",
            "crates/loom-cli/src/daemon_cmd.rs",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierOne,
        surface: "files",
        transport: "rest-json_rpc-grpc",
        profile: None,
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-hosted/src/serve.rs",
            "crates/loom-hosted/src/grpc.rs",
            "crates/loom-cli/src/daemon_cmd.rs",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierOne,
        surface: "vcs",
        transport: "rest-json_rpc-grpc",
        profile: None,
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-hosted/src/serve.rs",
            "crates/loom-hosted/src/grpc.rs",
            "crates/loom-cli/src/daemon_cmd.rs",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierOne,
        surface: "admin",
        transport: "rest-json_rpc",
        profile: None,
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-hosted/src/serve.rs",
            "crates/loom-cli/src/daemon_cmd.rs",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierOne,
        surface: "data-facets",
        transport: "rest-json_rpc-grpc",
        profile: None,
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-hosted/src/serve.rs",
            "crates/loom-hosted/src/grpc.rs",
            "crates/loom-cli/src/daemon_cmd.rs",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierOne,
        surface: "graph",
        transport: "rest-json_rpc-grpc",
        profile: None,
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-hosted/src/serve.rs",
            "crates/loom-hosted/src/grpc.rs",
            "crates/loom-cli/src/daemon_cmd.rs",
            "specs/0016-graph-layer.md",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierOne,
        surface: "ledger",
        transport: "rest-json_rpc-grpc",
        profile: None,
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-hosted/src/serve.rs",
            "crates/loom-hosted/src/grpc.rs",
            "crates/loom-cli/src/daemon_cmd.rs",
            "specs/0018-ledger-layer.md",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierOne,
        surface: "grpc-direct-tls",
        transport: "grpc",
        profile: None,
        status: HostedProtocolStatus::Unsupported,
        evidence: &["crates/loom-cli/src/daemon_cmd.rs"],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierTwo,
        surface: "s3",
        transport: "rest",
        profile: None,
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-hosted/src/serve.rs",
            "crates/loom-cli/src/daemon_cmd.rs",
            "specs/0008-wire-protocols.md",
            "specs/facet-bindings/P9-0018-facet-presentation-model.md",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierTwo,
        surface: "oci",
        transport: "rest",
        profile: None,
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-hosted/src/serve.rs",
            "crates/loom-cli/src/daemon_cmd.rs",
            "specs/0008-wire-protocols.md",
            "specs/facet-bindings/P9-0018-facet-presentation-model.md",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierTwo,
        surface: "postgres-mysql",
        transport: "tcp",
        profile: None,
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-hosted/src/sql.rs",
            "crates/loom-hosted/src/mysql_wire.rs",
            "crates/loom-cli/src/daemon_cmd.rs",
            "specs/0011b-sql-wire-adapters.md",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierTwo,
        surface: "pim",
        transport: "caldav-carddav-imap-jmap-smtp",
        profile: None,
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-hosted/src/serve.rs",
            "crates/loom-hosted/src/imap.rs",
            "crates/loom-hosted/src/jmap.rs",
            "crates/loom-hosted/src/smtp.rs",
            "crates/loom-cli/src/daemon_cmd.rs",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierTwo,
        surface: "fts",
        transport: "opensearch-rest",
        profile: Some("opensearch"),
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-hosted/src/serve.rs::opensearch_rust_client_transcript_covers_supported_fts_profile",
            "specs/0033-search-layer.md",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierTwo,
        surface: "vector",
        transport: "rest-grpc",
        profile: Some("qdrant"),
        status: HostedProtocolStatus::Target,
        evidence: &[
            "crates/loom-hosted/src/serve.rs",
            "crates/loom-hosted/src/grpc.rs",
            "crates/loom-cli/src/daemon_cmd.rs",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierTwo,
        surface: "vector",
        transport: "rest",
        profile: Some("pinecone"),
        status: HostedProtocolStatus::Target,
        evidence: &[
            "crates/loom-hosted/src/serve.rs",
            "specs/0017-vector-layer.md",
            "specs/facet-bindings/P9-0012-vector-binding.md",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierTwo,
        surface: "queue",
        transport: "kafka-tcp",
        profile: None,
        status: HostedProtocolStatus::Degraded,
        evidence: &[
            "crates/loom-hosted/src/kafka_wire.rs",
            "specs/0021c-kafka-compatibility-surface.md",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::HostedTierTwo,
        surface: "document",
        transport: "mongodb-couchbase",
        profile: None,
        status: HostedProtocolStatus::Target,
        evidence: &["specs/0020-document-layer.md"],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::Binding,
        surface: "language-bindings",
        transport: "c-abi-derived",
        profile: None,
        status: HostedProtocolStatus::Degraded,
        evidence: &["idl/loom.idl", "include/loom.h"],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::Binding,
        surface: "program-lifecycle",
        transport: "c-abi",
        profile: None,
        status: HostedProtocolStatus::Degraded,
        evidence: &[
            "idl/loom.idl",
            "include/loom.h",
            "crates/loom-conformance/src/lib.rs",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::Binding,
        surface: "program-lifecycle",
        transport: "runtime-binding-suites",
        profile: None,
        status: HostedProtocolStatus::Target,
        evidence: &[
            "crates/loom-conformance/src/lib.rs",
            "specs/0007a-binding-generation-packaging-and-certification.md",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::Provider,
        surface: "direct-tls-rustls",
        transport: "tls",
        profile: None,
        status: HostedProtocolStatus::Target,
        evidence: &[
            "specs/0008-wire-protocols.md",
            "specs/0032-platform-parity.md",
        ],
    },
    CapabilityMatrixRow {
        plane: CapabilityPlane::Provider,
        surface: "fips-hosted-release",
        transport: "release-channel",
        profile: None,
        status: HostedProtocolStatus::Target,
        evidence: &[
            "specs/0008-wire-protocols.md",
            "specs/0032-platform-parity.md",
        ],
    },
];

pub const RELEASE_CERTIFICATION_INVENTORY: &[ReleaseCertificationRow] = &[
    ReleaseCertificationRow {
        category: "release-materials",
        surface: "cli-server",
        target: "standard-release-materials",
        status: HostedProtocolStatus::Supported,
        evidence: &["scripts/release-materials.sh", "justfile", "specs/0060.md"],
    },
    ReleaseCertificationRow {
        category: "release-materials",
        surface: "cli-server",
        target: "fips-release-materials",
        status: HostedProtocolStatus::Supported,
        evidence: &["scripts/release-materials.sh", "justfile", "specs/0060.md"],
    },
    ReleaseCertificationRow {
        category: "binding-package-materials",
        surface: "bindings",
        target: "standard-binding-release-materials",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "scripts/binding-release-materials.sh",
            "justfile",
            "specs/0007a-binding-generation-packaging-and-certification.md",
            "specs/0060.md",
        ],
    },
    ReleaseCertificationRow {
        category: "binding-package-materials",
        surface: "bindings",
        target: "fips-binding-release-materials",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "scripts/binding-release-materials.sh",
            "justfile",
            "specs/0007a-binding-generation-packaging-and-certification.md",
            "specs/0060.md",
        ],
    },
    ReleaseCertificationRow {
        category: "binding-package-compatibility",
        surface: "bindings",
        target: "core-abi-version-metadata",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "scripts/binding-release-materials.sh",
            "justfile",
            "specs/0007a-binding-generation-packaging-and-certification.md",
            "specs/0060.md",
        ],
    },
    ReleaseCertificationRow {
        category: "binding-package-signing-materials",
        surface: "bindings",
        target: "unsigned-binding-signing-manifest",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "scripts/binding-release-materials.sh",
            "justfile",
            "specs/0007a-binding-generation-packaging-and-certification.md",
            "specs/0060.md",
        ],
    },
    ReleaseCertificationRow {
        category: "binding-package-promotion-policy",
        surface: "bindings",
        target: "registry-route-credential-and-install-validation-policy",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "scripts/binding-release-materials.sh",
            "justfile",
            "specs/0007a-binding-generation-packaging-and-certification.md",
            "specs/0060.md",
        ],
    },
    ReleaseCertificationRow {
        category: "binding-package-publication-gate",
        surface: "bindings",
        target: "protected-dry-run-publication-gate",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "scripts/binding-publication-dry-run.sh",
            "scripts/binding-release-materials.sh",
            "justfile",
            ".github/workflows/bindings.yml",
            "specs/0007a-binding-generation-packaging-and-certification.md",
            "specs/0060.md",
        ],
    },
    ReleaseCertificationRow {
        category: "binding-package-materials",
        surface: "program-lifecycle",
        target: "c-abi-inventory-no-runtime-overclaim",
        status: HostedProtocolStatus::Degraded,
        evidence: &[
            "crates/loom-conformance/src/lib.rs",
            "specs/0007a-binding-generation-packaging-and-certification.md",
        ],
    },
    ReleaseCertificationRow {
        category: "binding-package-publication",
        surface: "bindings",
        target: "registry-publishing-and-signed-artifacts",
        status: HostedProtocolStatus::Target,
        evidence: &[
            "specs/0007a-binding-generation-packaging-and-certification.md",
            "specs/0060.md",
        ],
    },
    ReleaseCertificationRow {
        category: "binding-interop",
        surface: "node-python",
        target: "shared-store-open-read-write",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "scripts/binding-cross-interop.sh",
            "justfile",
            ".github/workflows/bindings.yml",
        ],
    },
    ReleaseCertificationRow {
        category: "browser-runtime",
        surface: "wasm",
        target: "browser-worker-opfs-runtime",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "bindings/wasm/browser-test/worker.js",
            "bindings/wasm/browser-test/run.mjs",
            "specs/0032-platform-parity.md",
        ],
    },
    ReleaseCertificationRow {
        category: "browser-runtime",
        surface: "wasm",
        target: "native-fips-certification-claim",
        status: HostedProtocolStatus::Unsupported,
        evidence: &["bindings/wasm/README.md", "specs/0060.md"],
    },
    ReleaseCertificationRow {
        category: "device-runtime",
        surface: "ios",
        target: "swift-runtime-suite",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "bindings/ios/Tests/UldrenLoomTests/LoomTests.swift",
            "justfile",
        ],
    },
    ReleaseCertificationRow {
        category: "device-runtime",
        surface: "android",
        target: "kotlin-jvm-runtime-suite",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "bindings/android/src/jvmTest/kotlin/ai/uldren/loom/AndroidJvmRuntimeSmokeTest.kt",
            "justfile",
        ],
    },
    ReleaseCertificationRow {
        category: "device-runtime",
        surface: "react-native",
        target: "android-connected-host-fixture",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "bindings/react-native/host-test/android/app/src/androidTest/java/ai/uldren/loom/rn/host/UldrenLoomHostRuntimeTest.kt",
            "justfile",
            ".github/workflows/bindings.yml",
        ],
    },
    ReleaseCertificationRow {
        category: "provider-profile",
        surface: "runtime-profile",
        target: "linked-provider-profile-report",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-core/src/runtime.rs",
            "crates/loom-hosted/src/lib.rs",
            "specs/0060.md",
        ],
    },
    ReleaseCertificationRow {
        category: "provider-profile",
        surface: "fips-hosted-runtime",
        target: "served-fips-profile-gate",
        status: HostedProtocolStatus::Supported,
        evidence: &["crates/loom-hosted/src/lib.rs", "justfile", "specs/0060.md"],
    },
    ReleaseCertificationRow {
        category: "provider-profile",
        surface: "binding-fips-packages",
        target: "published-native-fips-binding-packages",
        status: HostedProtocolStatus::Target,
        evidence: &["scripts/binding-release-materials.sh", "specs/0060.md"],
    },
];

pub const HOSTED_PROTOCOL_FEATURES: &[HostedProtocolFeature] = &[
    HostedProtocolFeature {
        surface: "postgres",
        protocol: "tcp",
        feature: "tokio-postgres-supported-profile-transcript",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/pg_wire.rs",
    },
    HostedProtocolFeature {
        surface: "postgres",
        protocol: "tcp",
        feature: "tokio-postgres-parameterized-statement-transcript",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/pg_wire.rs",
    },
    HostedProtocolFeature {
        surface: "postgres",
        protocol: "tcp",
        feature: "postgres-sslrequest-direct-tls-transcript",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/pg_wire.rs",
    },
    HostedProtocolFeature {
        surface: "postgres",
        protocol: "libpq",
        feature: "guarded-psql-catalog-transcript",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/pg_wire.rs",
    },
    HostedProtocolFeature {
        surface: "postgres",
        protocol: "jdbc",
        feature: "guarded-client-transcript-profile",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0011b-sql-wire-adapters.md",
    },
    HostedProtocolFeature {
        surface: "postgres",
        protocol: "node",
        feature: "guarded-client-transcript-profile",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0011b-sql-wire-adapters.md",
    },
    HostedProtocolFeature {
        surface: "postgres",
        protocol: "python",
        feature: "guarded-client-transcript-profile",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0011b-sql-wire-adapters.md",
    },
    HostedProtocolFeature {
        surface: "postgres",
        protocol: "bi-tool",
        feature: "guarded-client-transcript-profile",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0011b-sql-wire-adapters.md",
    },
    HostedProtocolFeature {
        surface: "mysql",
        protocol: "tcp",
        feature: "raw-protocol-supported-profile-transcript",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/mysql_wire.rs",
    },
    HostedProtocolFeature {
        surface: "mysql",
        protocol: "mysql-cli",
        feature: "guarded-mysql-cli-metadata-transcript",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/mysql_wire.rs",
    },
    HostedProtocolFeature {
        surface: "mysql",
        protocol: "jdbc",
        feature: "guarded-connectorj-transcript-profile",
        status: HostedProtocolStatus::Target,
        evidence: "crates/loom-hosted/src/mysql_wire.rs",
    },
    HostedProtocolFeature {
        surface: "mysql",
        protocol: "node",
        feature: "guarded-mysql2-transcript-profile",
        status: HostedProtocolStatus::Target,
        evidence: "crates/loom-hosted/src/mysql_wire.rs",
    },
    HostedProtocolFeature {
        surface: "mysql",
        protocol: "python",
        feature: "guarded-pymysql-mysqlclient-transcript-profile",
        status: HostedProtocolStatus::Target,
        evidence: "crates/loom-hosted/src/mysql_wire.rs",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "api-versions-sasl-plain-auth",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/kafka_wire.rs",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "durable-topic-metadata",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/data.rs",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "shared-durable-metadata-version-allocation",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/data.rs",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "topic-create-list-delete",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/kafka_wire.rs",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "single-node-single-partition-metadata",
        status: HostedProtocolStatus::Degraded,
        evidence: "specs/0021c-kafka-compatibility-surface.md",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "multi-partition-topics",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0021c-kafka-compatibility-surface.md",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "uncompressed-record-batch-produce-fetch-offset-commit",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/kafka_wire.rs",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "normalized-record-batch-offset-rewrite",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/kafka_wire.rs",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "compressed-record-batches",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/kafka_wire.rs",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "consumer-groups",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0021c-kafka-compatibility-surface.md",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "producer-id-epoch-fencing",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/kafka_wire.rs",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "bounded-transaction-control",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/kafka_wire.rs",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "idempotent-produce-sequence-validation",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/kafka_wire.rs",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "transactional-offset-atomic-visibility",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/kafka_wire.rs",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "transactional-produced-record-visibility",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/kafka_wire.rs",
    },
    HostedProtocolFeature {
        surface: "kafka",
        protocol: "tcp",
        feature: "multi-broker-replication-isr-election",
        status: HostedProtocolStatus::Unsupported,
        evidence: "specs/0021c-kafka-compatibility-surface.md",
    },
    HostedProtocolFeature {
        surface: "etcd",
        protocol: "tcp",
        feature: "first-class-listener-admission",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-cli/src/daemon_cmd.rs",
    },
    HostedProtocolFeature {
        surface: "etcd",
        protocol: "tcp",
        feature: "kv-lease-compact-grpc-methods",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/grpc.rs",
    },
    HostedProtocolFeature {
        surface: "etcd",
        protocol: "tcp",
        feature: "durable-revision-lease-compaction-metadata",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/data.rs",
    },
    HostedProtocolFeature {
        surface: "etcd",
        protocol: "tcp",
        feature: "single-authority-static-cluster",
        status: HostedProtocolStatus::Degraded,
        evidence: "specs/facet-bindings/P9-0007-kv-binding.md",
    },
    HostedProtocolFeature {
        surface: "etcd",
        protocol: "tcp",
        feature: "bounded-watch-replay-event-log",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/grpc.rs",
    },
    HostedProtocolFeature {
        surface: "etcd",
        protocol: "tcp",
        feature: "live-watch-tail",
        status: HostedProtocolStatus::Target,
        evidence: "specs/facet-bindings/P9-0007-kv-binding.md",
    },
    HostedProtocolFeature {
        surface: "etcd",
        protocol: "tcp",
        feature: "member-cluster-auth-maintenance-apis",
        status: HostedProtocolStatus::Target,
        evidence: "specs/facet-bindings/P9-0007-kv-binding.md",
    },
    HostedProtocolFeature {
        surface: "etcd",
        protocol: "tcp",
        feature: "multi-member-raft-quorum",
        status: HostedProtocolStatus::Unsupported,
        evidence: "specs/facet-bindings/P9-0007-kv-binding.md",
    },
    HostedProtocolFeature {
        surface: "cas",
        protocol: "rest-json-rpc",
        feature: "put-get-missing-get-has-list-delete",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "cas",
        protocol: "rest-json-rpc",
        feature: "invalid-digest-post-delete-absence",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "cas",
        protocol: "grpc",
        feature: "put-get-missing-get-has-list-delete",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "cas",
        protocol: "grpc",
        feature: "invalid-digest-post-delete-absence",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "queue",
        protocol: "grpc",
        feature: "append-get-range-len",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "queue",
        protocol: "rest",
        feature: "append-get-range-len",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "queue",
        protocol: "json-rpc",
        feature: "append-get-range-len",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "time-series",
        protocol: "grpc",
        feature: "put-get-latest-range",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "time-series",
        protocol: "rest",
        feature: "put-get-latest-range",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "time-series",
        protocol: "json-rpc",
        feature: "put-get-latest-range",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "imap",
        feature: "bounded-rfc9051-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/imap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "imap",
        feature: "direct-rustls-imaps",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-cli/src/daemon_cmd.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "imap",
        feature: "durable-uid-state",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/imap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "imap",
        feature: "durable-subscriptions",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/imap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "imap",
        feature: "common-search-status-fetch-store",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/imap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "imap",
        feature: "idle-completion-without-push",
        status: HostedProtocolStatus::Degraded,
        evidence: "crates/loom-hosted/src/imap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "imap",
        feature: "non-synchronizing-literals",
        status: HostedProtocolStatus::Unsupported,
        evidence: "crates/loom-hosted/src/imap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "imap",
        feature: "full-rfc9051-conformance",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "smtp",
        feature: "setup-compatibility-listener",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/smtp.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "smtp",
        feature: "real-submission-relay-delivery",
        status: HostedProtocolStatus::Unsupported,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "smtp",
        feature: "optional-extensions-mixed-profile",
        status: HostedProtocolStatus::Degraded,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "mutable-state",
        feature: "flag-policy-version-deltas",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-core/src/mail.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "mutable-state",
        feature: "flag-merge-audit",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-core/src/mail.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "mutable-state",
        feature: "flag-compaction-retained-gap",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-core/src/mail.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "access",
        feature: "owner-only-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "specs/0008-wire-protocols.md",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "certification",
        feature: "profile-transcript-capture",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "caldav",
        feature: "bounded-webdav-resource-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "caldav",
        feature: "direct-tls",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-cli/src/daemon_cmd.rs",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "caldav",
        feature: "reference-client-certification",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0037-calendar-layer.md",
    },
    HostedProtocolFeature {
        surface: "contacts",
        protocol: "carddav",
        feature: "bounded-webdav-resource-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "contacts",
        protocol: "carddav",
        feature: "direct-tls",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-cli/src/daemon_cmd.rs",
    },
    HostedProtocolFeature {
        surface: "contacts",
        protocol: "carddav",
        feature: "reference-client-certification",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0038-contacts-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "jmap",
        feature: "bounded-rfc8620-rfc8621-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/jmap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "jmap",
        feature: "direct-tls",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-cli/src/daemon_cmd.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "jmap",
        feature: "blob-upload-download-email-import",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/jmap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "jmap",
        feature: "identity-and-deterministic-state",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/jmap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "jmap",
        feature: "email-changes-querychanges",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/jmap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "jmap",
        feature: "executable-rfc8620-rfc8621-transcript",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/jmap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "jmap",
        feature: "push",
        status: HostedProtocolStatus::Unsupported,
        evidence: "crates/loom-hosted/src/jmap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "jmap",
        feature: "reference-clients-full-conformance",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "shared-http-uri-webdav-basic-auth",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "uri-rfc3986-percent-encoding",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "webdav-rfc4918-base-methods",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "webdav-rfc5397-current-user-principal",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "webdav-rfc5689-extended-mkcol",
        status: HostedProtocolStatus::Unsupported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "well-known-rfc5785-caldav-carddav",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "well-known-rfc8615-caldav-carddav",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "webdav-rfc6578-sync-collection",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "service-discovery-rfc6764",
        status: HostedProtocolStatus::Degraded,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "http-basic-rfc7617",
        status: HostedProtocolStatus::Degraded,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "tls-rfc8996-modern-versions",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "http-semantics-rfc9110-bounded-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "http1-rfc9112-shared-stack",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "shared-http-over-tls-service-identity",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0008-wire-protocols.md",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "shared-dns-srv-dns-sd-discovery",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0008-wire-protocols.md",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "rfc-gate",
        feature: "caldav-rfc4791-bounded-access-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "rfc-gate",
        feature: "icalendar-rfc5545-bounded-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-pim/src/calendar.rs",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "rfc-gate",
        feature: "caldav-rfc4791-rfc5545-bounded-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "rfc-gate",
        feature: "itip-rfc5546",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0037-calendar-layer.md",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "rfc-gate",
        feature: "imip-rfc6047",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0037-calendar-layer.md",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "rfc-gate",
        feature: "caldav-scheduling-rfc6638",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0037-calendar-layer.md",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "rfc-gate",
        feature: "scheduling-itip-imip-freebusy",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0037-calendar-layer.md",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "rfc-gate",
        feature: "non-gregorian-recurrence-rfc7529",
        status: HostedProtocolStatus::Unsupported,
        evidence: "crates/loom-rrule/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "rfc-gate",
        feature: "non-gregorian-recurrence",
        status: HostedProtocolStatus::Unsupported,
        evidence: "specs/0037-calendar-layer.md",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "rfc-gate",
        feature: "timezone-reference-rfc7809",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0037-calendar-layer.md",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "rfc-gate",
        feature: "availability-rfc7953",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0037-calendar-layer.md",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "rfc-gate",
        feature: "timezone-reference-and-availability",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0037-calendar-layer.md",
    },
    HostedProtocolFeature {
        surface: "calendar",
        protocol: "rfc-gate",
        feature: "rfc7986-extra-properties",
        status: HostedProtocolStatus::Degraded,
        evidence: "crates/loom-pim/src/calendar.rs",
    },
    HostedProtocolFeature {
        surface: "contacts",
        protocol: "rfc-gate",
        feature: "carddav-rfc6352-bounded-access-profile",
        status: HostedProtocolStatus::Degraded,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "contacts",
        protocol: "rfc-gate",
        feature: "vcard-rfc6350-bounded-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-pim/src/contacts.rs",
    },
    HostedProtocolFeature {
        surface: "contacts",
        protocol: "rfc-gate",
        feature: "carddav-rfc6352-rfc6350-bounded-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "contacts",
        protocol: "rfc-gate",
        feature: "xcard-rfc6351",
        status: HostedProtocolStatus::Unsupported,
        evidence: "specs/0038-contacts-layer.md",
    },
    HostedProtocolFeature {
        surface: "contacts",
        protocol: "rfc-gate",
        feature: "xcard",
        status: HostedProtocolStatus::Unsupported,
        evidence: "specs/0038-contacts-layer.md",
    },
    HostedProtocolFeature {
        surface: "contacts",
        protocol: "rfc-gate",
        feature: "place-death-extensions-rfc6474",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0038-contacts-layer.md",
    },
    HostedProtocolFeature {
        surface: "contacts",
        protocol: "rfc-gate",
        feature: "place-death-extensions",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0038-contacts-layer.md",
    },
    HostedProtocolFeature {
        surface: "contacts",
        protocol: "rfc-gate",
        feature: "parameter-caret-encoding-rfc6868",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0038-contacts-layer.md",
    },
    HostedProtocolFeature {
        surface: "contacts",
        protocol: "rfc-gate",
        feature: "parameter-caret-encoding",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0038-contacts-layer.md",
    },
    HostedProtocolFeature {
        surface: "contacts",
        protocol: "rfc-gate",
        feature: "vcard3-dialect-conversion",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-pim/src/contacts.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "message-rfc5322-bounded-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-pim/src/mail.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "imap-rfc9051-bounded-profile",
        status: HostedProtocolStatus::Degraded,
        evidence: "crates/loom-hosted/src/imap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "jmap-core-rfc8620-bounded-profile",
        status: HostedProtocolStatus::Degraded,
        evidence: "crates/loom-hosted/src/jmap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "jmap-mail-rfc8621-bounded-profile",
        status: HostedProtocolStatus::Degraded,
        evidence: "crates/loom-hosted/src/jmap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "jmap-rfc8620-rfc8621-bounded-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/jmap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "jmap-blob-rfc9404",
        status: HostedProtocolStatus::Degraded,
        evidence: "crates/loom-hosted-pim/src/jmap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "jmap-quotas-rfc9425",
        status: HostedProtocolStatus::Degraded,
        evidence: "crates/loom-hosted-pim/src/jmap.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "jscontact-rfc9553",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "vcard-jscontact-extensions-rfc9554",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "jscontact-vcard-conversion-rfc9555",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "jmap-contacts-rfc9610",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "jscalendar-rfc8984",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "jmap-calendars-draft-ietf-jmap-calendars",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "jmap-sharing-rfc9670",
        status: HostedProtocolStatus::Unsupported,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "web-push-rfc8030",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "vapid-rfc8292",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "jmap-webpush-vapid-rfc9749",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "smtp-rfc5321-setup-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/smtp.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "smtp-submission-rfc6409-setup-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/smtp.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "smtp-starttls-rfc3207-setup-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/smtp.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "email-tls-rfc8314-bounded-profile",
        status: HostedProtocolStatus::Degraded,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "email-submission-ops-rfc5068",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "smtp-auth-rfc4954-bounded-profile",
        status: HostedProtocolStatus::Degraded,
        evidence: "crates/loom-hosted/src/smtp.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "sasl-rfc4422-bounded-profile",
        status: HostedProtocolStatus::Degraded,
        evidence: "crates/loom-hosted/src/smtp.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "sasl-plain-rfc4616-bounded-profile",
        status: HostedProtocolStatus::Degraded,
        evidence: "crates/loom-hosted/src/smtp.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "mailto-rfc6068",
        status: HostedProtocolStatus::Unsupported,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "smtp-size-rfc1870-bounded-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/smtp.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "smtp-pipelining-rfc2920",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "enhanced-status-codes-rfc3463",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "enhanced-status-registry-rfc5248",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "smtp-8bitmime-rfc6152-bounded-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/smtp.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "smtputf8-rfc6531",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "internationalized-headers-rfc6532",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "mime-format-rfc2045",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "mime-media-types-rfc2046",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "mime-encoded-words-rfc2047",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "mime-conformance-rfc2049",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "smtp-setup-auth-session-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/smtp.rs",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "smtp-starttls-standard-port-transcript",
        status: HostedProtocolStatus::Supported,
        evidence: "scripts/pim-cert/rfc-probe.sh",
    },
    HostedProtocolFeature {
        surface: "mail",
        protocol: "rfc-gate",
        feature: "smtp-optional-extensions-mixed-profile",
        status: HostedProtocolStatus::Degraded,
        evidence: "specs/0039-mail-layer.md",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "rfc-gate",
        feature: "live-local-probes",
        status: HostedProtocolStatus::Supported,
        evidence: "scripts/pim-cert/rfc-probe.sh",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "hooks",
        feature: "registration-envelope-event-emission",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-core/src/hooks.rs",
    },
    HostedProtocolFeature {
        surface: "pim",
        protocol: "hooks",
        feature: "execution-policy-planning",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-core/src/hooks.rs",
    },
    HostedProtocolFeature {
        surface: "vector",
        protocol: "rest",
        feature: "native-create-upsert-get-search",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "vector",
        protocol: "json-rpc",
        feature: "native-create-upsert-get-search",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "vector",
        protocol: "qdrant-rest",
        feature: "collection-point-search-scroll-count-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "vector",
        protocol: "qdrant-rest",
        feature: "json-filter-translation",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/vector_compat.rs",
    },
    HostedProtocolFeature {
        surface: "vector",
        protocol: "qdrant-grpc",
        feature: "unary-collection-point-search-scroll-count-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/grpc.rs",
    },
    HostedProtocolFeature {
        surface: "vector",
        protocol: "qdrant-grpc",
        feature: "protobuf-filter-translation",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/grpc.rs",
    },
    HostedProtocolFeature {
        surface: "vector",
        protocol: "pinecone-rest",
        feature: "describe-upsert-fetch-query-delete-list-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "vector",
        protocol: "pinecone-rest",
        feature: "json-filter-translation",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/vector_compat.rs",
    },
    HostedProtocolFeature {
        surface: "vector",
        protocol: "pinecone-rest",
        feature: "exact-only-capability-reporting",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "vector",
        protocol: "pinecone-rest",
        feature: "integrated-embedding-and-sparse-vector-requests",
        status: HostedProtocolStatus::Unsupported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "vector",
        protocol: "qdrant-pinecone",
        feature: "approximate-hosted-accelerator-policy",
        status: HostedProtocolStatus::Unsupported,
        evidence: "specs/0017-vector-layer.md",
    },
    HostedProtocolFeature {
        surface: "vector",
        protocol: "qdrant-pinecone",
        feature: "external-generated-client-transcripts",
        status: HostedProtocolStatus::Target,
        evidence: "specs/facet-bindings/P9-0012-vector-binding.md",
    },
    HostedProtocolFeature {
        surface: "vector",
        protocol: "milvus-weaviate-pgvector",
        feature: "compatibility-listeners",
        status: HostedProtocolStatus::Target,
        evidence: "specs/facet-bindings/P9-0012-vector-binding.md",
    },
    HostedProtocolFeature {
        surface: "oci",
        protocol: "rest",
        feature: "distribution-route-family",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "oci",
        protocol: "rest",
        feature: "daemon-opened-durable-listener-transcript",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-cli/src/daemon_cmd.rs",
    },
    HostedProtocolFeature {
        surface: "oci",
        protocol: "rest",
        feature: "monolithic-and-chunked-upload-digest-verification",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "oci",
        protocol: "rest",
        feature: "repository-tags-catalog-referrers",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "oci",
        protocol: "rest",
        feature: "schema-v1-and-unknown-dangerous-media-types",
        status: HostedProtocolStatus::Unsupported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "oci",
        protocol: "rest",
        feature: "direct-tls-listener",
        status: HostedProtocolStatus::Unsupported,
        evidence: "crates/loom-cli/src/daemon_cmd.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "rest",
        feature: "s3-compatible-bucket-object-service",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "rest",
        feature: "daemon-opened-durable-listener-transcript",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-cli/src/daemon_cmd.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "rest",
        feature: "bucket-scoped-and-service-scoped-routing",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "rest",
        feature: "object-metadata-ranges-conditionals-version-etags",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "rest",
        feature: "basic-multipart-upload",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "rest",
        feature: "sigv4-app-credential-verification",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "rest",
        feature: "configured-unauthenticated-public-access",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "aws-cli",
        feature: "guarded-sigv4-create-put-get-transcript",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "rest",
        feature: "multipart-completeness-uploads-parts-copy-abort-etag",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "rest",
        feature: "object-versioning-delete-markers-list-versions",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "rest",
        feature: "bucket-and-object-acl-canned-subset",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "rest",
        feature: "unsupported-non-goal-boundary-errors",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "aws-cli",
        feature: "guarded-multipart-versioning-acl-transcript",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "rest",
        feature: "lifecycle-replication-website-cors-logging-notification-objectlock-inventory-analytics-tagging-policy-sse",
        status: HostedProtocolStatus::Unsupported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "s3",
        protocol: "rest",
        feature: "direct-tls-listener",
        status: HostedProtocolStatus::Unsupported,
        evidence: "crates/loom-cli/src/daemon_cmd.rs",
    },
    HostedProtocolFeature {
        surface: "archive",
        protocol: "tar-zstd-tar-gzip-zip",
        feature: "file-tree-import-export",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-interchange-io/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "archive",
        protocol: "cli",
        feature: "interchange-import-export-archive",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-cli/src/main.rs",
    },
    HostedProtocolFeature {
        surface: "car",
        protocol: "car",
        feature: "deterministic-import-export",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-interchange-io/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "car",
        protocol: "cli",
        feature: "interchange-import-export-car",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-cli/src/main.rs",
    },
    HostedProtocolFeature {
        surface: "redis",
        protocol: "resp",
        feature: "strings-ttl-hash-set-list-zset-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/redis.rs",
    },
    HostedProtocolFeature {
        surface: "redis",
        protocol: "resp",
        feature: "daemon-opened-durable-reload-transcript",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-cli/src/daemon_cmd.rs",
    },
    HostedProtocolFeature {
        surface: "redis",
        protocol: "resp",
        feature: "stream-and-pubsub-family-boundaries",
        status: HostedProtocolStatus::Unsupported,
        evidence: "crates/loom-hosted/src/redis.rs",
    },
    HostedProtocolFeature {
        surface: "memcached",
        protocol: "text",
        feature: "volatile-cache-command-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/memcached.rs",
    },
    HostedProtocolFeature {
        surface: "memcached",
        protocol: "text",
        feature: "daemon-opened-text-transcript",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-cli/src/daemon_cmd.rs",
    },
    HostedProtocolFeature {
        surface: "memcached",
        protocol: "text",
        feature: "durable-or-backed-cache-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/memcached.rs",
    },
    HostedProtocolFeature {
        surface: "memcached",
        protocol: "text",
        feature: "guarded-client-transcript-profile",
        status: HostedProtocolStatus::Target,
        evidence: "crates/loom-hosted/src/memcached.rs",
    },
    HostedProtocolFeature {
        surface: "kv",
        protocol: "rest",
        feature: "native-put-get-delete-list-range",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "kv",
        protocol: "json-rpc",
        feature: "native-put-get-delete-list-range",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "fts",
        protocol: "rest",
        feature: "native-collection-management-query-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "fts",
        protocol: "json-rpc",
        feature: "native-collection-management-query-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "fts",
        protocol: "opensearch-rest",
        feature: "index-doc-query-bulk-msearch-refresh-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "fts",
        protocol: "opensearch-rest",
        feature: "terms-missing-range-histogram-value-count-avg-sum-min-max-stats-cardinality-aggregations",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "fts",
        protocol: "opensearch-rest",
        feature: "match-all-and-analyzer-boundary",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "fts",
        protocol: "opensearch-rest",
        feature: "aliases-multi-index-wildcards",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "fts",
        protocol: "opensearch-rest",
        feature: "bulk-independent-item-errors",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "fts",
        protocol: "opensearch-rest",
        feature: "security-read-shims",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "fts",
        protocol: "opensearch-rest",
        feature: "security-mutation-apis",
        status: HostedProtocolStatus::Unsupported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "fts",
        protocol: "opensearch-rest",
        feature: "route-matrix-certification",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "fts",
        protocol: "opensearch-rest",
        feature: "nested-pipeline-aggregations",
        status: HostedProtocolStatus::Target,
        evidence: "specs/facet-bindings/P9-0013-search-binding.md",
    },
    HostedProtocolFeature {
        surface: "fts",
        protocol: "opensearch-rest",
        feature: "full-analyzer-execution",
        status: HostedProtocolStatus::Target,
        evidence: "specs/facet-bindings/P9-0013-search-binding.md",
    },
    HostedProtocolFeature {
        surface: "fts",
        protocol: "opensearch-rest",
        feature: "external-generated-client-transcripts",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "columnar",
        protocol: "rest",
        feature: "native-management-query-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "columnar",
        protocol: "rest",
        feature: "arrow-ipc-parquet-binary-transfer",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "columnar",
        protocol: "json-rpc",
        feature: "native-management-query-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "columnar",
        protocol: "arrow-flight-flight-sql",
        feature: "binary-analytical-data-plane",
        status: HostedProtocolStatus::Target,
        evidence: "specs/facet-bindings/P9-0009-columnar-binding.md",
    },
    HostedProtocolFeature {
        surface: "dataframe",
        protocol: "rest",
        feature: "native-management-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "dataframe",
        protocol: "arrow-flight-flight-sql",
        feature: "binary-result-transfer",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0045-dataframe-layer.md",
    },
    HostedProtocolFeature {
        surface: "dataframe",
        protocol: "json-rpc-grpc",
        feature: "generated-client-symmetry",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0045-dataframe-layer.md",
    },
    HostedProtocolFeature {
        surface: "exec",
        protocol: "rest",
        feature: "raw-cbor-run-listener",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "exec",
        protocol: "json-rpc",
        feature: "raw-cbor-run-listener",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/serve.rs",
    },
    HostedProtocolFeature {
        surface: "exec",
        protocol: "grpc",
        feature: "raw-cbor-run-listener",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/grpc.rs",
    },
    HostedProtocolFeature {
        surface: "graph",
        protocol: "rest",
        feature: "native-crud-neighbors-reachability-query-capabilities",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "graph",
        protocol: "json-rpc",
        feature: "native-crud-neighbors-reachability-query-capabilities",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "graph",
        protocol: "grpc",
        feature: "native-crud-neighbors-reachability-query-capabilities",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/grpc.rs",
    },
    HostedProtocolFeature {
        surface: "graph",
        protocol: "native-query",
        feature: "bounded-opencypher-gql-aligned-read-subset",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-core/src/graph.rs",
    },
    HostedProtocolFeature {
        surface: "graph",
        protocol: "native-query",
        feature: "full-gql-cypher-gremlin-compatibility",
        status: HostedProtocolStatus::Unsupported,
        evidence: "specs/0016-graph-layer.md",
    },
    HostedProtocolFeature {
        surface: "ledger",
        protocol: "rest",
        feature: "native-append-get-head-len-verify",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "ledger",
        protocol: "json-rpc",
        feature: "native-append-get-head-len-verify",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-protocol-conformance/src/lib.rs",
    },
    HostedProtocolFeature {
        surface: "ledger",
        protocol: "grpc",
        feature: "native-append-range-checkpoint-signature-proof-profile",
        status: HostedProtocolStatus::Supported,
        evidence: "crates/loom-hosted/src/grpc.rs",
    },
    HostedProtocolFeature {
        surface: "ledger",
        protocol: "transparency-log",
        feature: "witness-publication-retention-enforcement",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0018-ledger-layer.md",
    },
    HostedProtocolFeature {
        surface: "delivery",
        protocol: "websocket",
        feature: "durable-subscribe-ack-replay",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0035-durable-delivery.md",
    },
    HostedProtocolFeature {
        surface: "delivery",
        protocol: "sse",
        feature: "durable-subscribe-ack-replay",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0035-durable-delivery.md",
    },
    HostedProtocolFeature {
        surface: "delivery",
        protocol: "json-rpc",
        feature: "durable-notifications-ack-replay",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0035-durable-delivery.md",
    },
    HostedProtocolFeature {
        surface: "delivery",
        protocol: "grpc",
        feature: "durable-streaming-ack-replay",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0035-durable-delivery.md",
    },
    HostedProtocolFeature {
        surface: "delivery",
        protocol: "mcp",
        feature: "resource-subscription-ack-replay",
        status: HostedProtocolStatus::Target,
        evidence: "specs/0035-durable-delivery.md",
    },
];

pub const LOCAL_COORDINATION_FEATURES: &[LocalCoordinationFeature] = &[
    LocalCoordinationFeature {
        surface: "embedded-coordinator",
        feature: "leased-fenced-lock-behavior",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-core/src/lock.rs",
            "crates/loom-conformance/src/behavior/admin.rs",
        ],
    },
    LocalCoordinationFeature {
        surface: "embedded-coordinator",
        feature: "sync-destination-branch-lock",
        status: HostedProtocolStatus::Supported,
        evidence: &["crates/loom-core/src/sync.rs"],
    },
    LocalCoordinationFeature {
        surface: "cli-daemon",
        feature: "manual-lifecycle-sessions-pins-locks",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-cli/src/daemon_cmd.rs",
            "crates/loom-store/src/daemon.rs",
        ],
    },
    LocalCoordinationFeature {
        surface: "cli-daemon",
        feature: "secure-native-ipc-default",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-store/src/daemon.rs",
            "crates/loom-cli/src/daemon_cmd.rs",
        ],
    },
    LocalCoordinationFeature {
        surface: "cli-daemon",
        feature: "tcp-loopback-transport",
        status: HostedProtocolStatus::Degraded,
        evidence: &[
            "crates/loom-store/src/daemon.rs",
            "crates/loom-cli/src/daemon_cmd.rs",
        ],
    },
    LocalCoordinationFeature {
        surface: "host-native-bindings",
        feature: "node-python-cpp-jvm-daemon-lock-clients",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "bindings/node/src/daemon_fns.rs",
            "bindings/python/src/daemon_fns.rs",
            "bindings/cpp/include/loom/detail.hpp",
            "bindings/jvm/src/main/java/ai/uldren/loom/Loom.java",
        ],
    },
    LocalCoordinationFeature {
        surface: "mcp-attached",
        feature: "daemon-session-liveness-fail-closed",
        status: HostedProtocolStatus::Supported,
        evidence: &[
            "crates/loom-mcp/src/server.rs",
            "crates/loom-mcp/src/server/tests.rs",
        ],
    },
    LocalCoordinationFeature {
        surface: "hosted-locks",
        feature: "public-lock-protocol",
        status: HostedProtocolStatus::Unsupported,
        evidence: &["specs/0036-locking-and-coordination.md"],
    },
    LocalCoordinationFeature {
        surface: "mobile-browser-bindings",
        feature: "cli-daemon-lock-client",
        status: HostedProtocolStatus::Unsupported,
        evidence: &[
            "bindings/wasm/src/lib.rs",
            "bindings/android/src/commonMain/kotlin/ai/uldren/loom/Loom.kt",
            "bindings/ios/Sources/UldrenLoom/Loom.swift",
        ],
    },
    LocalCoordinationFeature {
        surface: "platform-daemon-transport",
        feature: "unsupported-native-ipc-on-nonmatching-platforms",
        status: HostedProtocolStatus::Unsupported,
        evidence: &["crates/loom-store/src/daemon.rs"],
    },
];

/// The names of the binding-adjacent surfaces in `tier`, in inventory order.
pub fn binding_surfaces(tier: BindingTier) -> Vec<&'static str> {
    BINDING_CONFORMANCE_INVENTORY
        .iter()
        .filter(|s| s.tier == tier)
        .map(|s| s.name)
        .collect()
}

/// The promoted surfaces exercised by one binding inventory row.
pub fn binding_surface_coverage(name: &str) -> Option<&'static [&'static str]> {
    BINDING_CONFORMANCE_INVENTORY
        .iter()
        .find(|s| s.name == name)
        .map(|s| s.coverage)
}

// ---- serialized conformance report -------------------------------------------------------------

/// The status of one suite or surface in a serialized conformance report. A status never promotes a
/// capability: it records only what evidence proves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportStatus {
    /// An executable suite ran and passed.
    Passed,
    /// An executable suite ran and failed.
    Failed,
    /// An executable suite exists but did not run in this report; carries the reason.
    Skipped(&'static str),
    /// A declarative scenario exists but no executable runner exists.
    Inventory,
    /// A planned capability has no executable suite.
    Target,
}

impl ReportStatus {
    /// The stable lowercase label used in the serialized report.
    pub fn label(&self) -> &'static str {
        match self {
            ReportStatus::Passed => "passed",
            ReportStatus::Failed => "failed",
            ReportStatus::Skipped(_) => "skipped",
            ReportStatus::Inventory => "inventory",
            ReportStatus::Target => "target",
        }
    }

    /// The reason text, present only for [`ReportStatus::Skipped`].
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            ReportStatus::Skipped(r) => Some(r),
            _ => None,
        }
    }
}

/// One suite entry (a canonical vector suite or a behavior suite) in a conformance report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuiteReport {
    pub name: &'static str,
    pub status: ReportStatus,
}

/// One binding-adjacent surface entry in a conformance report: its evidence tier and the status this
/// report assigns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingSurfaceReport {
    pub name: &'static str,
    pub tier: BindingTier,
    pub status: ReportStatus,
    pub evidence: &'static str,
    pub coverage: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedProtocolFeatureReport {
    pub surface: &'static str,
    pub protocol: &'static str,
    pub feature: &'static str,
    pub status: HostedProtocolStatus,
    pub evidence: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalCoordinationFeatureReport {
    pub surface: &'static str,
    pub feature: &'static str,
    pub status: HostedProtocolStatus,
    pub evidence: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityMatrixReport {
    pub plane: CapabilityPlane,
    pub surface: &'static str,
    pub transport: &'static str,
    pub profile: Option<&'static str>,
    pub status: HostedProtocolStatus,
    pub evidence: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseCertificationReport {
    pub category: &'static str,
    pub surface: &'static str,
    pub target: &'static str,
    pub status: HostedProtocolStatus,
    pub evidence: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingRuntimeCertificationReport {
    pub surface: &'static str,
    pub target: &'static str,
    pub status: HostedProtocolStatus,
    pub workflow: &'static str,
    pub evidence: &'static [&'static str],
    pub coverage: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingPackageCertificationReport {
    pub surface: &'static str,
    pub package_name: &'static str,
    pub package_kind: &'static str,
    pub build_recipe: &'static str,
    pub materials_status: HostedProtocolStatus,
    pub compatibility_metadata_status: HostedProtocolStatus,
    pub signing_manifest_status: HostedProtocolStatus,
    pub publication_status: HostedProtocolStatus,
    pub evidence: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeProfileReport {
    pub binary_channel: &'static str,
    pub runtime_policy: &'static str,
    pub default_identity_profile: &'static str,
    pub crypto_provider: &'static str,
    pub tls_provider: &'static str,
    pub fips_capable: bool,
    pub fips_tls_claim: bool,
}

impl From<RuntimeProfile> for RuntimeProfileReport {
    fn from(profile: RuntimeProfile) -> Self {
        Self {
            binary_channel: profile.binary_channel,
            runtime_policy: profile.runtime_policy,
            default_identity_profile: profile.default_identity_profile.as_str(),
            crypto_provider: profile.crypto_provider,
            tls_provider: profile.tls_provider,
            fips_capable: profile.fips_capable,
            fips_tls_claim: profile.fips_tls_claim,
        }
    }
}

/// A machine-readable conformance report over the current executable boundary: canonical vector suites,
/// executable behavior runners, inventory-only behavior suites, and binding evidence tiers. It
/// serializes proof only; it certifies no hosted protocol, full binding runtime, or provider lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceReport {
    /// Implementation package name (`CARGO_PKG_NAME`).
    pub implementation_name: &'static str,
    /// Implementation package version (`CARGO_PKG_VERSION`).
    pub implementation_version: &'static str,
    /// Source revision or release tag, injected at compile time via `LOOM_SOURCE_REVISION`; `None` when
    /// that variable is unset or blank.
    pub source_revision: Option<String>,
    /// Runtime provider profile reported by the linked Loom artifact.
    pub runtime_profile: RuntimeProfileReport,
    /// Identity profiles exercised by this certification (the memory-store boundary covers blake3, and
    /// the ledger-head vectors additionally cover sha256).
    pub identity_profiles: Vec<&'static str>,
    /// The overall status of the certification this report serializes.
    pub status: ReportStatus,
    /// Canonical vector suites and their status.
    pub vector_suites: Vec<SuiteReport>,
    /// Behavior suites: the executable runners as `passed`, the declarative-only suites as `inventory`.
    pub behavior_suites: Vec<SuiteReport>,
    /// Binding-adjacent surfaces with their evidence tier and report status.
    pub binding_surfaces: Vec<BindingSurfaceReport>,
    /// Hosted protocol feature evidence with supported, degraded, target, and unsupported status.
    pub hosted_protocols: Vec<HostedProtocolFeatureReport>,
    /// Local daemon, lock, transport, binding, and attached-client coordination evidence.
    pub local_coordination: Vec<LocalCoordinationFeatureReport>,
    /// Cross-surface capability matrix over listener, local, MCP, binding, and provider planes.
    pub capability_matrix: Vec<CapabilityMatrixReport>,
    /// Release, package, browser, device, and provider certification evidence inventory.
    pub release_certification: Vec<ReleaseCertificationReport>,
    /// Per-binding runtime certification evidence, including release workflow and promoted-surface coverage.
    pub binding_runtime_certification: Vec<BindingRuntimeCertificationReport>,
    /// Per-binding package material evidence and publication status.
    pub binding_package_certification: Vec<BindingPackageCertificationReport>,
    /// Enterprise certification profile selected for the PIM hosted release gate.
    pub certification_profile: CertificationProfileReport,
    /// Redacted transcript fixture inventory for reference-client and executable protocol evidence.
    pub transcript_inventory: Vec<TranscriptInventoryReport>,
    /// Total declarative scenarios across every behavior suite.
    pub total_scenarios: usize,
}

/// Normalize a build-injected source revision into the report's `source_revision`. A revision is used
/// only when it carries non-whitespace content; an absent or blank value resolves to `None` so the
/// report never serializes a fabricated or empty revision. The returned string is trimmed.
fn resolve_source_revision(injected: Option<&str>) -> Option<String> {
    injected
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_string)
}

/// Build a [`ConformanceReport`] for fresh in-memory backends. Runs [`certify_memory_store`] (so any
/// vector or executable-behavior failure short-circuits as `Err`), then maps its summary and the binding
/// inventory into the serialized report taxonomy. Binding runtime suites and the C ABI / result-codec
/// core suites are `skipped` here with a reason, because this certification does not execute them;
/// implemented-but-ungated bindings are `inventory`, and target surfaces are `target`.
///
/// `source_revision` is read at compile time from the optional `LOOM_SOURCE_REVISION` environment
/// variable (a build may set it to a git revision or release tag); when it is unset or blank the field
/// stays `None`.
pub fn report_memory_store() -> Result<ConformanceReport> {
    let summary = certify_memory_store()?;
    Ok(report_memory_store_from_summary(summary))
}

fn report_memory_store_from_summary(summary: ConformanceSummary) -> ConformanceReport {
    let vector_suites = summary
        .vector_suites_passed
        .iter()
        .map(|name| SuiteReport {
            name,
            status: ReportStatus::Passed,
        })
        .collect();

    let mut behavior_suites: Vec<SuiteReport> = summary
        .behavior_suites_passed
        .iter()
        .map(|name| SuiteReport {
            name,
            status: ReportStatus::Passed,
        })
        .collect();
    behavior_suites.extend(summary.data_only_suites.iter().map(|name| SuiteReport {
        name,
        status: ReportStatus::Inventory,
    }));

    let binding_surfaces = BINDING_CONFORMANCE_INVENTORY
        .iter()
        .map(|s| {
            let status = match s.tier {
                // This certification runs the canonical vectors directly; the other executable-core
                // surfaces run under `cargo test --workspace`, not here.
                BindingTier::ExecutableCore if s.name == "canonical-vectors" => {
                    ReportStatus::Passed
                }
                BindingTier::ExecutableCore => ReportStatus::Skipped(
                    "executed by `cargo test --workspace`, not by this certification",
                ),
                BindingTier::BindingRuntimeSuite if s.name == "cpp" => ReportStatus::Skipped(
                    "runtime suite runs in CI through the CMake and CTest recipe, not this certification",
                ),
                BindingTier::BindingRuntimeSuite => ReportStatus::Skipped(
                    "runtime suite runs via its own toolchain recipe, not this certification",
                ),
                BindingTier::CrossBindingInterop => ReportStatus::Skipped(
                    "cross-binding interop runs via the bindings release workflow, not this certification",
                ),
                BindingTier::ImplementedNotGated => ReportStatus::Inventory,
                BindingTier::TargetOnly => ReportStatus::Target,
            };
            BindingSurfaceReport {
                name: s.name,
                tier: s.tier,
                status,
                evidence: s.evidence,
                coverage: s.coverage,
            }
        })
        .collect();
    let hosted_protocols = HOSTED_PROTOCOL_FEATURES
        .iter()
        .map(|feature| HostedProtocolFeatureReport {
            surface: feature.surface,
            protocol: feature.protocol,
            feature: feature.feature,
            status: feature.status,
            evidence: feature.evidence,
        })
        .collect();
    let local_coordination = LOCAL_COORDINATION_FEATURES
        .iter()
        .map(|feature| LocalCoordinationFeatureReport {
            surface: feature.surface,
            feature: feature.feature,
            status: feature.status,
            evidence: feature.evidence,
        })
        .collect();
    let capability_matrix = CAPABILITY_MATRIX
        .iter()
        .map(|row| CapabilityMatrixReport {
            plane: row.plane,
            surface: row.surface,
            transport: row.transport,
            profile: row.profile,
            status: row.status,
            evidence: row.evidence,
        })
        .collect();
    let release_certification = RELEASE_CERTIFICATION_INVENTORY
        .iter()
        .map(|row| ReleaseCertificationReport {
            category: row.category,
            surface: row.surface,
            target: row.target,
            status: row.status,
            evidence: row.evidence,
        })
        .collect();
    let binding_runtime_certification = BINDING_RUNTIME_CERTIFICATION
        .iter()
        .map(|row| BindingRuntimeCertificationReport {
            surface: row.surface,
            target: row.target,
            status: row.status,
            workflow: row.workflow,
            evidence: row.evidence,
            coverage: row.coverage,
        })
        .collect();
    let binding_package_certification = BINDING_PACKAGE_CERTIFICATION
        .iter()
        .map(|row| BindingPackageCertificationReport {
            surface: row.surface,
            package_name: row.package_name,
            package_kind: row.package_kind,
            build_recipe: row.build_recipe,
            materials_status: row.materials_status,
            compatibility_metadata_status: row.compatibility_metadata_status,
            signing_manifest_status: row.signing_manifest_status,
            publication_status: row.publication_status,
            evidence: row.evidence,
        })
        .collect();
    let certification_profile = CertificationProfileReport {
        name: "pim-owner-only-enterprise-v1",
        surface: "pim",
        admin_profile_key: "admin.certification.profile",
        owner_scope: "owner-only",
        tls_mode: "direct-tls-or-loopback-tls-terminated",
        auth_mode: "owner-authenticated",
        redaction_policy: PIM_REDACTION_POLICY,
        conformance_suites: PIM_CERTIFICATION_SUITES.to_vec(),
        required_clients: PIM_CERTIFICATION_CLIENT_REQUIREMENTS.to_vec(),
    };

    ConformanceReport {
        implementation_name: env!("CARGO_PKG_NAME"),
        implementation_version: env!("CARGO_PKG_VERSION"),
        source_revision: resolve_source_revision(option_env!("LOOM_SOURCE_REVISION")),
        runtime_profile: runtime_profile().into(),
        identity_profiles: vec!["blake3", "sha256"],
        status: ReportStatus::Passed,
        vector_suites,
        behavior_suites,
        binding_surfaces,
        hosted_protocols,
        local_coordination,
        capability_matrix,
        release_certification,
        binding_runtime_certification,
        binding_package_certification,
        certification_profile,
        transcript_inventory: PIM_TRANSCRIPT_INVENTORY.to_vec(),
        total_scenarios: summary.total_scenarios,
    }
}

fn capability_plane_label(plane: CapabilityPlane) -> &'static str {
    match plane {
        CapabilityPlane::Local => "local",
        CapabilityPlane::Mcp => "mcp",
        CapabilityPlane::HostedTierOne => "hosted-tier-one",
        CapabilityPlane::HostedTierTwo => "hosted-tier-two",
        CapabilityPlane::Binding => "binding",
        CapabilityPlane::Provider => "provider",
    }
}

/// The stable label for a [`BindingTier`] in the serialized report.
fn tier_label(tier: BindingTier) -> &'static str {
    match tier {
        BindingTier::ExecutableCore => "executable-core",
        BindingTier::BindingRuntimeSuite => "binding-runtime-suite",
        BindingTier::ImplementedNotGated => "implemented-not-gated",
        BindingTier::CrossBindingInterop => "cross-binding-interop",
        BindingTier::TargetOnly => "target-only",
    }
}

fn hosted_protocol_status_label(status: HostedProtocolStatus) -> &'static str {
    match status {
        HostedProtocolStatus::Supported => "supported",
        HostedProtocolStatus::Degraded => "degraded",
        HostedProtocolStatus::Target => "target",
        HostedProtocolStatus::Unsupported => "unsupported",
    }
}

fn certification_status_label(status: CertificationStatus) -> &'static str {
    match status {
        CertificationStatus::Passed => "passed",
        CertificationStatus::Failed => "failed",
        CertificationStatus::Degraded => "degraded",
        CertificationStatus::Unsupported => "unsupported",
        CertificationStatus::Skipped => "skipped",
        CertificationStatus::Target => "target",
    }
}

/// Append `value` as a JSON string literal (quotes plus the minimal required escapes).
fn push_json_str(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn queue7_pim_capability_report_fixture_json() -> String {
    let mut s = String::from("{\"fixture\":\"queue7-pim-capability-report\",\"rows\":[");
    for (i, row) in QUEUE7_PIM_CAPABILITY_REPORT_FIXTURES.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str("{\"category\":");
        push_json_str(&mut s, row.category);
        s.push_str(",\"surface\":");
        push_json_str(&mut s, row.surface);
        s.push_str(",\"protocol\":");
        push_json_str(&mut s, row.protocol);
        s.push_str(",\"feature\":");
        push_json_str(&mut s, row.feature);
        s.push_str(",\"status\":");
        push_json_str(&mut s, row.status);
        s.push_str(",\"evidence\":");
        push_json_str(&mut s, row.evidence);
        s.push('}');
    }
    s.push_str("]}");
    s
}

fn push_suite_array(out: &mut String, suites: &[SuiteReport]) {
    out.push('[');
    for (i, s) in suites.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        push_json_str(out, s.name);
        out.push_str(",\"status\":");
        push_json_str(out, s.status.label());
        out.push_str(",\"reason\":");
        match s.status.reason() {
            Some(r) => push_json_str(out, r),
            None => out.push_str("null"),
        }
        out.push('}');
    }
    out.push(']');
}

impl ConformanceReport {
    /// Serialize the report to a stable, machine-readable JSON document with deterministic field and
    /// element order. No serde dependency: a small writer keeps the schema self-contained, matching the
    /// repo's hand-rolled JSON convention.
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        s.push_str("{\"implementation\":{\"name\":");
        push_json_str(&mut s, self.implementation_name);
        s.push_str(",\"version\":");
        push_json_str(&mut s, self.implementation_version);
        s.push('}');

        s.push_str(",\"source_revision\":");
        match &self.source_revision {
            Some(r) => push_json_str(&mut s, r),
            None => s.push_str("null"),
        }

        s.push_str(",\"runtime_profile\":{\"binary_channel\":");
        push_json_str(&mut s, self.runtime_profile.binary_channel);
        s.push_str(",\"runtime_policy\":");
        push_json_str(&mut s, self.runtime_profile.runtime_policy);
        s.push_str(",\"default_identity_profile\":");
        push_json_str(&mut s, self.runtime_profile.default_identity_profile);
        s.push_str(",\"crypto_provider\":");
        push_json_str(&mut s, self.runtime_profile.crypto_provider);
        s.push_str(",\"tls_provider\":");
        push_json_str(&mut s, self.runtime_profile.tls_provider);
        s.push_str(",\"fips_capable\":");
        s.push_str(if self.runtime_profile.fips_capable {
            "true"
        } else {
            "false"
        });
        s.push_str(",\"fips_tls_claim\":");
        s.push_str(if self.runtime_profile.fips_tls_claim {
            "true"
        } else {
            "false"
        });
        s.push('}');

        s.push_str(",\"identity_profiles\":[");
        for (i, p) in self.identity_profiles.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            push_json_str(&mut s, p);
        }
        s.push(']');

        s.push_str(",\"status\":");
        push_json_str(&mut s, self.status.label());

        s.push_str(",\"vector_suites\":");
        push_suite_array(&mut s, &self.vector_suites);
        s.push_str(",\"behavior_suites\":");
        push_suite_array(&mut s, &self.behavior_suites);

        s.push_str(",\"binding_surfaces\":[");
        for (i, b) in self.binding_surfaces.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"name\":");
            push_json_str(&mut s, b.name);
            s.push_str(",\"tier\":");
            push_json_str(&mut s, tier_label(b.tier));
            s.push_str(",\"status\":");
            push_json_str(&mut s, b.status.label());
            s.push_str(",\"reason\":");
            match b.status.reason() {
                Some(r) => push_json_str(&mut s, r),
                None => s.push_str("null"),
            }
            s.push_str(",\"evidence\":");
            push_json_str(&mut s, b.evidence);
            s.push_str(",\"coverage\":[");
            for (j, item) in b.coverage.iter().enumerate() {
                if j > 0 {
                    s.push(',');
                }
                push_json_str(&mut s, item);
            }
            s.push(']');
            s.push('}');
        }
        s.push(']');

        s.push_str(",\"hosted_protocols\":[");
        for (i, p) in self.hosted_protocols.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"surface\":");
            push_json_str(&mut s, p.surface);
            s.push_str(",\"protocol\":");
            push_json_str(&mut s, p.protocol);
            s.push_str(",\"feature\":");
            push_json_str(&mut s, p.feature);
            s.push_str(",\"status\":");
            push_json_str(&mut s, hosted_protocol_status_label(p.status));
            s.push_str(",\"evidence\":");
            push_json_str(&mut s, p.evidence);
            s.push('}');
        }
        s.push(']');

        s.push_str(",\"local_coordination\":[");
        for (i, p) in self.local_coordination.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"surface\":");
            push_json_str(&mut s, p.surface);
            s.push_str(",\"feature\":");
            push_json_str(&mut s, p.feature);
            s.push_str(",\"status\":");
            push_json_str(&mut s, hosted_protocol_status_label(p.status));
            s.push_str(",\"evidence\":[");
            for (j, evidence) in p.evidence.iter().enumerate() {
                if j > 0 {
                    s.push(',');
                }
                push_json_str(&mut s, evidence);
            }
            s.push(']');
            s.push('}');
        }
        s.push(']');

        s.push_str(",\"capability_matrix\":[");
        for (i, p) in self.capability_matrix.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"plane\":");
            push_json_str(&mut s, capability_plane_label(p.plane));
            s.push_str(",\"surface\":");
            push_json_str(&mut s, p.surface);
            s.push_str(",\"transport\":");
            push_json_str(&mut s, p.transport);
            s.push_str(",\"profile\":");
            match p.profile {
                Some(profile) => push_json_str(&mut s, profile),
                None => s.push_str("null"),
            }
            s.push_str(",\"status\":");
            push_json_str(&mut s, hosted_protocol_status_label(p.status));
            s.push_str(",\"evidence\":[");
            for (j, evidence) in p.evidence.iter().enumerate() {
                if j > 0 {
                    s.push(',');
                }
                push_json_str(&mut s, evidence);
            }
            s.push(']');
            s.push('}');
        }
        s.push(']');

        s.push_str(",\"release_certification\":[");
        for (i, p) in self.release_certification.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"category\":");
            push_json_str(&mut s, p.category);
            s.push_str(",\"surface\":");
            push_json_str(&mut s, p.surface);
            s.push_str(",\"target\":");
            push_json_str(&mut s, p.target);
            s.push_str(",\"status\":");
            push_json_str(&mut s, hosted_protocol_status_label(p.status));
            s.push_str(",\"evidence\":[");
            for (j, evidence) in p.evidence.iter().enumerate() {
                if j > 0 {
                    s.push(',');
                }
                push_json_str(&mut s, evidence);
            }
            s.push(']');
            s.push('}');
        }
        s.push(']');

        s.push_str(",\"binding_runtime_certification\":[");
        for (i, p) in self.binding_runtime_certification.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"surface\":");
            push_json_str(&mut s, p.surface);
            s.push_str(",\"target\":");
            push_json_str(&mut s, p.target);
            s.push_str(",\"status\":");
            push_json_str(&mut s, hosted_protocol_status_label(p.status));
            s.push_str(",\"workflow\":");
            push_json_str(&mut s, p.workflow);
            s.push_str(",\"evidence\":[");
            for (j, evidence) in p.evidence.iter().enumerate() {
                if j > 0 {
                    s.push(',');
                }
                push_json_str(&mut s, evidence);
            }
            s.push_str("],\"coverage\":[");
            for (j, item) in p.coverage.iter().enumerate() {
                if j > 0 {
                    s.push(',');
                }
                push_json_str(&mut s, item);
            }
            s.push(']');
            s.push('}');
        }
        s.push(']');

        s.push_str(",\"binding_package_certification\":[");
        for (i, p) in self.binding_package_certification.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"surface\":");
            push_json_str(&mut s, p.surface);
            s.push_str(",\"package_name\":");
            push_json_str(&mut s, p.package_name);
            s.push_str(",\"package_kind\":");
            push_json_str(&mut s, p.package_kind);
            s.push_str(",\"build_recipe\":");
            push_json_str(&mut s, p.build_recipe);
            s.push_str(",\"materials_status\":");
            push_json_str(&mut s, hosted_protocol_status_label(p.materials_status));
            s.push_str(",\"compatibility_metadata_status\":");
            push_json_str(
                &mut s,
                hosted_protocol_status_label(p.compatibility_metadata_status),
            );
            s.push_str(",\"signing_manifest_status\":");
            push_json_str(
                &mut s,
                hosted_protocol_status_label(p.signing_manifest_status),
            );
            s.push_str(",\"publication_status\":");
            push_json_str(&mut s, hosted_protocol_status_label(p.publication_status));
            s.push_str(",\"evidence\":[");
            for (j, evidence) in p.evidence.iter().enumerate() {
                if j > 0 {
                    s.push(',');
                }
                push_json_str(&mut s, evidence);
            }
            s.push(']');
            s.push('}');
        }
        s.push(']');

        s.push_str(",\"certification_profile\":{\"name\":");
        push_json_str(&mut s, self.certification_profile.name);
        s.push_str(",\"surface\":");
        push_json_str(&mut s, self.certification_profile.surface);
        s.push_str(",\"admin_profile_key\":");
        push_json_str(&mut s, self.certification_profile.admin_profile_key);
        s.push_str(",\"owner_scope\":");
        push_json_str(&mut s, self.certification_profile.owner_scope);
        s.push_str(",\"tls_mode\":");
        push_json_str(&mut s, self.certification_profile.tls_mode);
        s.push_str(",\"auth_mode\":");
        push_json_str(&mut s, self.certification_profile.auth_mode);
        s.push_str(",\"redaction_policy\":{\"name\":");
        push_json_str(&mut s, self.certification_profile.redaction_policy.name);
        s.push_str(",\"retention\":");
        push_json_str(
            &mut s,
            self.certification_profile.redaction_policy.retention,
        );
        s.push_str(",\"redact\":[");
        for (i, value) in self
            .certification_profile
            .redaction_policy
            .redact
            .iter()
            .enumerate()
        {
            if i > 0 {
                s.push(',');
            }
            push_json_str(&mut s, value);
        }
        s.push_str("],\"retain\":[");
        for (i, value) in self
            .certification_profile
            .redaction_policy
            .retain
            .iter()
            .enumerate()
        {
            if i > 0 {
                s.push(',');
            }
            push_json_str(&mut s, value);
        }
        s.push_str("]}");
        s.push_str(",\"conformance_suites\":[");
        for (i, suite) in self
            .certification_profile
            .conformance_suites
            .iter()
            .enumerate()
        {
            if i > 0 {
                s.push(',');
            }
            push_json_str(&mut s, suite);
        }
        s.push_str("],\"required_clients\":[");
        for (i, client) in self
            .certification_profile
            .required_clients
            .iter()
            .enumerate()
        {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"surface\":");
            push_json_str(&mut s, client.surface);
            s.push_str(",\"protocol\":");
            push_json_str(&mut s, client.protocol);
            s.push_str(",\"client\":");
            push_json_str(&mut s, client.client);
            s.push_str(",\"platform\":");
            push_json_str(&mut s, client.platform);
            s.push_str(",\"role\":");
            push_json_str(&mut s, client.role);
            s.push_str(",\"status\":");
            push_json_str(&mut s, certification_status_label(client.status));
            s.push_str(",\"evidence\":");
            push_json_str(&mut s, client.evidence);
            s.push('}');
        }
        s.push_str("]}");

        s.push_str(",\"transcript_inventory\":[");
        for (i, transcript) in self.transcript_inventory.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"name\":");
            push_json_str(&mut s, transcript.name);
            s.push_str(",\"surface\":");
            push_json_str(&mut s, transcript.surface);
            s.push_str(",\"protocol\":");
            push_json_str(&mut s, transcript.protocol);
            s.push_str(",\"client\":");
            push_json_str(&mut s, transcript.client);
            s.push_str(",\"status\":");
            push_json_str(&mut s, certification_status_label(transcript.status));
            s.push_str(",\"reason\":");
            push_json_str(&mut s, transcript.reason);
            s.push_str(",\"evidence\":");
            push_json_str(&mut s, transcript.evidence);
            s.push_str(",\"redaction_profile\":");
            push_json_str(&mut s, transcript.redaction_profile);
            s.push('}');
        }
        s.push(']');

        s.push_str(",\"total_scenarios\":");
        s.push_str(&self.total_scenarios.to_string());
        s.push('}');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::MemoryStore;

    fn report_memory_store_manifest() -> ConformanceReport {
        report_memory_store_from_summary(memory_store_certification_manifest())
    }

    #[test]
    fn memory_store_passes_table_identity_vectors() {
        run_table_identity_vectors(MemoryStore::new())
            .expect("MemoryStore must pass the table-identity vectors");
    }

    #[test]
    fn columnar_manifest_vectors_pass() {
        run_columnar_manifest_vectors().expect("columnar manifest vectors must pass");
    }

    #[test]
    fn graph_root_vectors_pass() {
        run_graph_root_vectors().expect("graph root vectors must pass");
    }

    #[test]
    fn graph_semantic_diff_merge_vectors_pass() {
        run_graph_semantic_diff_merge_vectors()
            .expect("graph semantic diff and merge vectors must pass");
    }

    #[test]
    fn kv_map_vectors_pass() {
        run_kv_map_vectors().expect("KV map vectors must pass");
    }

    #[test]
    fn exec_manifest_vectors_pass() {
        run_exec_manifest_vectors().expect("exec manifest vectors must pass");
    }

    #[test]
    fn interchange_vectors_pass() {
        run_interchange_vectors().expect("interchange vectors must pass");
    }

    #[test]
    fn substrate_model_vectors_pass() {
        run_substrate_model_vectors().expect("substrate model vectors must pass");
    }

    #[test]
    fn meetings_profile_vectors_pass() {
        run_meetings_profile_vectors().expect("meetings profile vectors must pass");
    }

    #[test]
    fn drive_profile_vectors_pass() {
        run_drive_profile_vectors().expect("drive profile vectors must pass");
    }

    #[test]
    fn lock_fence_vectors_pass() {
        run_lock_fence_vectors().expect("lock fence vectors must pass");
    }

    #[test]
    fn memory_store_passes_all_vectors() {
        run_all_vectors(MemoryStore::new()).expect("MemoryStore must pass the full certification");
    }

    #[test]
    fn memory_store_passes_blob_vectors() {
        let mut store = MemoryStore::new();
        run_blob_vectors(&mut store).expect("MemoryStore must pass the canonical vectors");
        assert_eq!(store.len(), BLOB_VECTORS.len());
    }

    #[test]
    fn codec_contract_vectors_pass() {
        run_codec_contract_vectors().expect("codec contract vectors must pass");
    }

    #[test]
    fn document_root_vectors_pass() {
        run_document_root_vectors().expect("document root vectors must pass");
    }

    #[test]
    fn document_chunked_body_vectors_pass() {
        run_document_chunked_body_vectors().expect("document chunked body vectors must pass");
    }

    #[test]
    fn document_retained_tombstone_vectors_pass() {
        run_document_retained_tombstone_vectors()
            .expect("document retained tombstone vectors must pass");
    }

    #[test]
    fn capability_policy_vectors_pass() {
        run_capability_policy_vectors().expect("capability policy vectors must pass");
    }

    #[test]
    fn change_set_vectors_pass() {
        run_change_set_vectors().expect("change set vectors must pass");
    }

    #[test]
    fn memory_store_passes_object_model_vectors() {
        let mut store = MemoryStore::new();
        run_object_model_vectors(&mut store)
            .expect("MemoryStore must pass the object-model vectors");
    }

    #[test]
    fn loom_templates_vectors_pass() {
        run_template_vectors().expect("Loom Templates vectors must pass");
    }

    #[test]
    fn ledger_head_vectors_pass_under_both_profiles() {
        run_ledger_head_vectors_profiled(Algo::Blake3)
            .expect("default/blake3 ledger head vectors must pass");
        run_ledger_head_vectors_profiled(Algo::Sha256)
            .expect("fips/sha256 ledger head vectors must pass");
    }

    #[test]
    fn network_access_vectors_are_well_formed() {
        assert!(!NETWORK_ACCESS_VECTORS.is_empty());
        let mut names = std::collections::BTreeSet::new();
        let mut covers_trusted_proxy = false;
        let mut covers_mtls = false;
        let mut covers_default = false;
        let mut covers_malformed_forwarded = false;
        for vector in NETWORK_ACCESS_VECTORS {
            assert!(
                names.insert(vector.name),
                "duplicate vector {}",
                vector.name
            );
            assert!(matches!(vector.default_action, "allow" | "deny"));
            assert!(matches!(vector.expect_source_family, "ipv4" | "ipv6"));
            if vector.expect_rule_id.is_none() {
                covers_default = true;
            }
            if vector.forwarded.is_some() && !vector.expect_allowed {
                covers_malformed_forwarded = true;
            }
            let mut rule_ids = std::collections::BTreeSet::new();
            for rule in vector.rules {
                assert!(rule_ids.insert(rule.id), "duplicate rule {}", rule.id);
                assert!(matches!(rule.action, "allow" | "deny"));
                if rule.trusted_proxy_cidr.is_some() {
                    covers_trusted_proxy = true;
                }
                if rule.require_mtls {
                    covers_mtls = true;
                }
            }
        }
        assert!(covers_trusted_proxy);
        assert!(covers_mtls);
        assert!(covers_default);
        assert!(covers_malformed_forwarded);
    }

    #[test]
    fn memory_store_certification_manifest_preserves_suite_membership() {
        let summary = memory_store_certification_manifest();

        assert_eq!(
            summary.behavior_suites_passed,
            behavior::EXECUTABLE_BEHAVIOR_SUITES.to_vec(),
            "executable runners must be reported as passed"
        );
        assert_eq!(
            summary.vector_suites_passed,
            CANONICAL_VECTOR_SUITES.to_vec()
        );

        for executed in &summary.behavior_suites_passed {
            assert!(
                !summary.data_only_suites.contains(executed),
                "an executed suite must not appear as data-only: {executed}"
            );
        }
        assert!(summary.data_only_suites.contains(&"files"));
        assert!(summary.data_only_suites.contains(&"vcs"));

        assert_eq!(
            summary.behavior_suites_passed.len() + summary.data_only_suites.len(),
            behavior::BEHAVIOR_SUITES.len(),
            "the executed and data-only suites must partition BEHAVIOR_SUITES"
        );
        let expected_scenarios: usize = behavior::BEHAVIOR_SUITES
            .iter()
            .map(|(_, scenarios)| scenarios.len())
            .sum();
        assert_eq!(summary.total_scenarios, expected_scenarios);
    }

    #[test]
    #[ignore = "manual full certification run; default tests cover individual suites"]
    fn memory_store_full_certification_executes_all_suites() {
        certify_memory_store().expect("MemoryStore must pass aggregate certification");
    }

    #[test]
    fn binding_inventory_is_well_formed() {
        // Names are unique, so a surface lands in exactly one tier.
        let mut names: Vec<&str> = BINDING_CONFORMANCE_INVENTORY
            .iter()
            .map(|s| s.name)
            .collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "binding inventory names must be unique");

        // The four tiers partition the inventory exactly.
        let core = binding_surfaces(BindingTier::ExecutableCore).len();
        let runtime = binding_surfaces(BindingTier::BindingRuntimeSuite).len();
        let ungated = binding_surfaces(BindingTier::ImplementedNotGated).len();
        let interop = binding_surfaces(BindingTier::CrossBindingInterop).len();
        let target = binding_surfaces(BindingTier::TargetOnly).len();
        assert_eq!(
            core + runtime + ungated + interop + target,
            count,
            "tiers must partition the inventory"
        );

        // Target surfaces carry no checked-in evidence; every other tier must.
        for s in BINDING_CONFORMANCE_INVENTORY {
            if s.tier == BindingTier::TargetOnly {
                assert!(
                    s.evidence.is_empty(),
                    "target surface {} must have no evidence",
                    s.name
                );
                assert!(
                    s.coverage.is_empty(),
                    "target surface {} must have no runtime coverage",
                    s.name
                );
            } else {
                assert!(
                    !s.evidence.is_empty(),
                    "surface {} must cite evidence",
                    s.name
                );
            }
            let mut coverage = s.coverage.to_vec();
            coverage.sort_unstable();
            coverage.dedup();
            assert_eq!(
                coverage.len(),
                s.coverage.len(),
                "coverage entries must be unique for {}",
                s.name
            );
        }
    }

    #[test]
    fn binding_inventory_evidence_artifacts_exist() {
        // The honest claim: a surface is only placed above TargetOnly when an actual checked-in file
        // backs it. Resolve each evidence path against the repo root and assert it exists on disk.
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        for s in BINDING_CONFORMANCE_INVENTORY {
            if s.tier == BindingTier::TargetOnly {
                continue;
            }
            let path = repo_root.join(s.evidence);
            assert!(
                path.is_file(),
                "binding surface {} cites evidence that is not a checked-in file: {}",
                s.name,
                s.evidence
            );
        }
    }

    #[test]
    fn capability_matrix_is_well_formed() {
        let mut keys: Vec<(CapabilityPlane, &str, &str, Option<&str>)> = CAPABILITY_MATRIX
            .iter()
            .map(|row| (row.plane, row.surface, row.transport, row.profile))
            .collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), count, "capability matrix rows must be unique");

        for plane in [
            CapabilityPlane::Local,
            CapabilityPlane::Mcp,
            CapabilityPlane::HostedTierOne,
            CapabilityPlane::HostedTierTwo,
            CapabilityPlane::Binding,
            CapabilityPlane::Provider,
        ] {
            assert!(
                CAPABILITY_MATRIX.iter().any(|row| row.plane == plane),
                "capability matrix must include {} plane",
                capability_plane_label(plane)
            );
        }

        for status in [
            HostedProtocolStatus::Supported,
            HostedProtocolStatus::Degraded,
            HostedProtocolStatus::Target,
            HostedProtocolStatus::Unsupported,
        ] {
            assert!(
                CAPABILITY_MATRIX.iter().any(|row| row.status == status),
                "capability matrix must include {status:?} status"
            );
        }

        for row in CAPABILITY_MATRIX {
            assert!(
                !row.evidence.is_empty(),
                "capability matrix row {}/{}/{} must cite evidence",
                capability_plane_label(row.plane),
                row.surface,
                row.transport
            );
        }
    }

    #[test]
    fn capability_supported_claims_are_source_backed_and_projected() {
        let registry = loom_core::capability::registry();
        let supported: Vec<&loom_core::CapabilityInfo> = registry
            .iter()
            .filter(|capability| {
                capability.operational_state == loom_core::CapabilityOperationalState::Supported
            })
            .collect();
        assert!(
            !supported.is_empty(),
            "capability registry must contain source-backed supported rows"
        );

        let registry_names: std::collections::BTreeSet<&str> =
            loom_core::capability::source_registries()
                .iter()
                .flat_map(|source| source.records.iter().map(|capability| capability.name))
                .collect();

        for capability in &supported {
            assert!(
                matches!(
                    capability.proof,
                    loom_core::CapabilityProof::Executable
                        | loom_core::CapabilityProof::SourceBacked
                ),
                "{} cannot claim supported without source-backed proof",
                capability.name
            );
            assert!(
                registry_names.contains(capability.name),
                "{} cannot claim supported outside a source-owned registry",
                capability.name
            );
            assert!(
                !capability.owner_module.is_empty(),
                "{} must name the owning source module",
                capability.name
            );
            assert_eq!(
                capability.reason_code, None,
                "{} supported claim must not carry a reason_code",
                capability.name
            );
            assert_eq!(
                capability.stable_error, None,
                "{} supported claim must not carry a stable_error",
                capability.name
            );
        }

        let detailed_json: serde_json::Value =
            serde_json::from_str(&registry.to_json(loom_core::CapabilityVisibility::Detailed))
                .expect("detailed capability JSON must parse");
        let json_records = detailed_json
            .get("records")
            .and_then(serde_json::Value::as_array)
            .expect("detailed capability JSON must carry records");
        for capability in &supported {
            let record = json_records
                .iter()
                .find(|record| {
                    record
                        .get("capability_id")
                        .and_then(serde_json::Value::as_str)
                        == Some(capability.name)
                })
                .unwrap_or_else(|| {
                    panic!("{} missing from detailed JSON projection", capability.name)
                });
            assert_eq!(
                record
                    .get("operational_state")
                    .and_then(serde_json::Value::as_str),
                Some("supported"),
                "{} JSON projection must preserve supported state",
                capability.name
            );
            assert!(
                matches!(
                    record
                        .get("proof_status")
                        .and_then(serde_json::Value::as_str),
                    Some("executable" | "source-backed")
                ),
                "{} JSON projection must preserve source-backed proof",
                capability.name
            );
        }

        let decoded = loom_codec::decode(&registry.to_cbor()).expect("capability CBOR must decode");
        let loom_codec::Value::Map(set_fields) = decoded else {
            panic!("capability CBOR must encode a map");
        };
        let records = set_fields
            .iter()
            .find_map(|(key, value)| match key {
                loom_codec::Value::Text(key) if key == "records" => Some(value),
                _ => None,
            })
            .expect("capability CBOR must carry records");
        let loom_codec::Value::Array(records) = records else {
            panic!("capability CBOR records must encode an array");
        };
        for capability in &supported {
            let record = records
                .iter()
                .find(|record| {
                    cbor_map_text_field(record, "capability_id") == Some(capability.name)
                })
                .unwrap_or_else(|| panic!("{} missing from CBOR projection", capability.name));
            assert_eq!(
                cbor_map_text_field(record, "operational_state"),
                Some("supported"),
                "{} CBOR projection must preserve supported state",
                capability.name
            );
            assert!(
                matches!(
                    cbor_map_text_field(record, "proof_status"),
                    Some("executable" | "source-backed")
                ),
                "{} CBOR projection must preserve source-backed proof",
                capability.name
            );
        }
    }

    #[test]
    fn capability_projection_surfaces_use_the_shared_model() {
        let cli = include_str!("../../../crates/loom-cli/src/main.rs");
        assert!(cli.contains("print_capabilities_text(&rows)"));
        assert!(cli.contains("set.to_json(visibility)"));

        let mcp = include_str!("../../../crates/loom-mcp/src/reads.rs");
        assert!(mcp.contains("served_capabilities(loom.capabilities()).to_cbor()"));
        assert!(mcp.contains("served_capabilities(loom.capabilities()).to_json(visibility)"));

        let hosted = include_str!("../../../crates/loom-hosted/src/admin.rs");
        assert!(hosted.contains("loom.capabilities().to_json(visibility)"));

        let ffi = include_str!("../../../crates/loom-ffi/src/lib.rs");
        assert!(ffi.contains("loom_capabilities"));
        assert!(ffi.contains("set.to_cbor()"));
    }

    fn cbor_map_text_field<'a>(value: &'a loom_codec::Value, field: &str) -> Option<&'a str> {
        let loom_codec::Value::Map(fields) = value else {
            return None;
        };
        fields.iter().find_map(|(key, value)| match (key, value) {
            (loom_codec::Value::Text(key), loom_codec::Value::Text(value)) if key == field => {
                Some(value.as_str())
            }
            _ => None,
        })
    }

    #[test]
    fn capability_matrix_evidence_artifacts_exist() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");

        for row in CAPABILITY_MATRIX {
            for evidence in row.evidence {
                let (path_text, anchor) = evidence
                    .split_once("::")
                    .map_or((*evidence, None), |(path, anchor)| (path, Some(anchor)));
                let path = repo_root.join(path_text);
                assert!(
                    path.is_file(),
                    "capability matrix row {}/{}/{} cites missing evidence: {}",
                    capability_plane_label(row.plane),
                    row.surface,
                    row.transport,
                    evidence
                );
                if let Some(anchor) = anchor {
                    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
                        panic!(
                            "capability matrix row {}/{}/{} cannot read evidence {}: {error}",
                            capability_plane_label(row.plane),
                            row.surface,
                            row.transport,
                            evidence
                        )
                    });
                    assert!(
                        text.contains(anchor),
                        "capability matrix row {}/{}/{} cites missing evidence anchor: {}",
                        capability_plane_label(row.plane),
                        row.surface,
                        row.transport,
                        evidence
                    );
                }
            }
        }
    }

    #[test]
    fn capability_matrix_records_recent_hosted_graph_and_ledger_grpc() {
        for surface in ["graph", "ledger"] {
            assert!(
                CAPABILITY_MATRIX.iter().any(|row| {
                    row.plane == CapabilityPlane::HostedTierOne
                        && row.surface == surface
                        && row.transport == "rest-json_rpc-grpc"
                        && row.profile.is_none()
                        && row.status == HostedProtocolStatus::Supported
                        && row.evidence.contains(&"crates/loom-hosted/src/grpc.rs")
                        && row.evidence.contains(&"crates/loom-cli/src/daemon_cmd.rs")
                }),
                "capability matrix must record source-backed native {surface} gRPC admission"
            );
        }
    }

    #[test]
    fn program_lifecycle_generated_evidence_is_not_overclaimed() {
        let c_abi_row = CAPABILITY_MATRIX
            .iter()
            .find(|row| {
                row.plane == CapabilityPlane::Binding
                    && row.surface == "program-lifecycle"
                    && row.transport == "c-abi"
                    && row.profile.is_none()
            })
            .expect("Program lifecycle C ABI capability row exists");
        assert_eq!(c_abi_row.status, HostedProtocolStatus::Degraded);
        assert!(c_abi_row.evidence.contains(&"idl/loom.idl"));
        assert!(c_abi_row.evidence.contains(&"include/loom.h"));

        let runtime_row = CAPABILITY_MATRIX
            .iter()
            .find(|row| {
                row.plane == CapabilityPlane::Binding
                    && row.surface == "program-lifecycle"
                    && row.transport == "runtime-binding-suites"
                    && row.profile.is_none()
            })
            .expect("Program lifecycle runtime binding capability row exists");
        assert_eq!(runtime_row.status, HostedProtocolStatus::Target);

        for row in CAPABILITY_MATRIX
            .iter()
            .filter(|row| row.surface == "program-lifecycle")
        {
            assert_ne!(
                row.status,
                HostedProtocolStatus::Supported,
                "Program lifecycle capability row {}/{} must not advertise supported status before runtime evidence exists",
                row.surface,
                row.transport
            );
        }

        let release_row = RELEASE_CERTIFICATION_INVENTORY
            .iter()
            .find(|row| {
                row.category == "binding-package-materials"
                    && row.surface == "program-lifecycle"
                    && row.target == "c-abi-inventory-no-runtime-overclaim"
            })
            .expect("Program lifecycle release certification inventory row exists");
        assert_eq!(release_row.status, HostedProtocolStatus::Degraded);
        assert!(
            release_row
                .evidence
                .contains(&"crates/loom-conformance/src/lib.rs")
        );
    }

    #[test]
    fn hosted_protocol_inventory_records_native_graph_ledger_kv_vector_and_columnar_boundaries() {
        for (surface, protocol, feature) in [
            (
                "graph",
                "grpc",
                "native-crud-neighbors-reachability-query-capabilities",
            ),
            (
                "ledger",
                "grpc",
                "native-append-range-checkpoint-signature-proof-profile",
            ),
        ] {
            assert!(
                HOSTED_PROTOCOL_FEATURES.iter().any(|row| {
                    row.surface == surface
                        && row.protocol == protocol
                        && row.feature == feature
                        && row.status == HostedProtocolStatus::Supported
                        && row.evidence == "crates/loom-hosted/src/grpc.rs"
                }),
                "hosted protocol inventory must record supported {surface}/{protocol} evidence"
            );
        }

        for protocol in ["rest", "json-rpc"] {
            assert!(
                HOSTED_PROTOCOL_FEATURES.iter().any(|row| {
                    row.surface == "graph"
                        && row.protocol == protocol
                        && row.feature == "native-crud-neighbors-reachability-query-capabilities"
                        && row.status == HostedProtocolStatus::Supported
                        && row.evidence == "crates/loom-protocol-conformance/src/lib.rs"
                }),
                "hosted protocol inventory must record executable graph/{protocol} evidence"
            );
        }

        for protocol in ["rest", "json-rpc"] {
            assert!(
                HOSTED_PROTOCOL_FEATURES.iter().any(|row| {
                    row.surface == "vector"
                        && row.protocol == protocol
                        && row.feature == "native-create-upsert-get-search"
                        && row.status == HostedProtocolStatus::Supported
                        && row.evidence == "crates/loom-protocol-conformance/src/lib.rs"
                }),
                "hosted protocol inventory must record executable vector/{protocol} evidence"
            );
        }

        for protocol in ["rest", "json-rpc"] {
            assert!(
                HOSTED_PROTOCOL_FEATURES.iter().any(|row| {
                    row.surface == "columnar"
                        && row.protocol == protocol
                        && row.feature == "native-management-query-profile"
                        && row.status == HostedProtocolStatus::Supported
                        && row.evidence == "crates/loom-protocol-conformance/src/lib.rs"
                }),
                "hosted protocol inventory must record executable columnar/{protocol} evidence"
            );
        }

        for protocol in ["rest", "json-rpc"] {
            assert!(
                HOSTED_PROTOCOL_FEATURES.iter().any(|row| {
                    row.surface == "kv"
                        && row.protocol == protocol
                        && row.feature == "native-put-get-delete-list-range"
                        && row.status == HostedProtocolStatus::Supported
                        && row.evidence == "crates/loom-protocol-conformance/src/lib.rs"
                }),
                "hosted protocol inventory must record executable kv/{protocol} evidence"
            );
        }

        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|row| {
            row.surface == "graph"
                && row.protocol == "native-query"
                && row.feature == "full-gql-cypher-gremlin-compatibility"
                && row.status == HostedProtocolStatus::Unsupported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|row| {
            row.surface == "ledger"
                && row.protocol == "transparency-log"
                && row.feature == "witness-publication-retention-enforcement"
                && row.status == HostedProtocolStatus::Target
        }));
    }

    #[test]
    fn release_certification_inventory_is_well_formed() {
        let mut keys: Vec<(&str, &str, &str)> = RELEASE_CERTIFICATION_INVENTORY
            .iter()
            .map(|row| (row.category, row.surface, row.target))
            .collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(
            keys.len(),
            count,
            "release certification rows must be unique"
        );

        for category in [
            "release-materials",
            "binding-package-materials",
            "binding-package-compatibility",
            "binding-package-signing-materials",
            "binding-package-promotion-policy",
            "binding-package-publication-gate",
            "binding-package-publication",
            "binding-interop",
            "browser-runtime",
            "device-runtime",
            "provider-profile",
        ] {
            assert!(
                RELEASE_CERTIFICATION_INVENTORY
                    .iter()
                    .any(|row| row.category == category),
                "release certification inventory must include {category}"
            );
        }

        for status in [
            HostedProtocolStatus::Supported,
            HostedProtocolStatus::Degraded,
            HostedProtocolStatus::Target,
            HostedProtocolStatus::Unsupported,
        ] {
            assert!(
                RELEASE_CERTIFICATION_INVENTORY
                    .iter()
                    .any(|row| row.status == status),
                "release certification inventory must include {status:?} status"
            );
        }
    }

    #[test]
    fn release_certification_inventory_evidence_artifacts_exist() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");

        for row in RELEASE_CERTIFICATION_INVENTORY {
            assert!(
                !row.evidence.is_empty(),
                "release certification row {}/{}/{} must cite evidence",
                row.category,
                row.surface,
                row.target
            );
            for evidence in row.evidence {
                let path = repo_root.join(evidence);
                assert!(
                    path.is_file(),
                    "release certification row {}/{}/{} cites missing evidence: {}",
                    row.category,
                    row.surface,
                    row.target,
                    evidence
                );
            }
        }
    }

    #[test]
    fn binding_runtime_certification_is_well_formed() {
        let mut keys: Vec<(&str, &str)> = BINDING_RUNTIME_CERTIFICATION
            .iter()
            .map(|row| (row.surface, row.target))
            .collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(
            keys.len(),
            count,
            "binding runtime certification rows must be unique"
        );

        let runtime_surfaces = binding_surfaces(BindingTier::BindingRuntimeSuite);
        assert_eq!(
            count,
            runtime_surfaces.len(),
            "every runtime-suite binding needs one certification row"
        );

        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");

        for surface in runtime_surfaces {
            let rows: Vec<&BindingRuntimeCertificationRow> = BINDING_RUNTIME_CERTIFICATION
                .iter()
                .filter(|row| row.surface == surface)
                .collect();
            assert_eq!(
                rows.len(),
                1,
                "{surface} must have exactly one runtime certification row"
            );
            let row = rows[0];
            assert_eq!(
                row.coverage,
                binding_surface_coverage(surface).expect("runtime coverage exists"),
                "{surface} runtime certification coverage must match binding inventory"
            );
            assert!(
                repo_root.join(row.workflow).is_file(),
                "{surface} cites missing workflow {}",
                row.workflow
            );
            assert!(
                !row.evidence.is_empty(),
                "{surface} runtime certification row must cite evidence"
            );
            for evidence in row.evidence {
                assert!(
                    repo_root.join(evidence).is_file(),
                    "{surface} cites missing runtime certification evidence: {}",
                    evidence
                );
            }
        }
    }

    #[test]
    fn binding_package_certification_is_well_formed() {
        let mut surfaces: Vec<&str> = BINDING_PACKAGE_CERTIFICATION
            .iter()
            .map(|row| row.surface)
            .collect();
        let count = surfaces.len();
        surfaces.sort_unstable();
        surfaces.dedup();
        assert_eq!(
            surfaces.len(),
            count,
            "binding package certification surfaces must be unique"
        );

        let mut expected = binding_surfaces(BindingTier::BindingRuntimeSuite);
        expected.push("c-abi");
        expected.sort_unstable();
        let mut actual: Vec<&str> = BINDING_PACKAGE_CERTIFICATION
            .iter()
            .map(|row| row.surface)
            .collect();
        actual.sort_unstable();
        assert_eq!(
            actual, expected,
            "package certification must cover every runtime binding plus the C ABI"
        );

        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let material_script =
            std::fs::read_to_string(repo_root.join("scripts/binding-release-materials.sh"))
                .expect("binding release material script must be readable");

        for row in BINDING_PACKAGE_CERTIFICATION {
            assert!(
                !row.package_name.is_empty(),
                "{} must record a package name",
                row.surface
            );
            assert!(
                !row.package_kind.is_empty(),
                "{} must record a package kind",
                row.surface
            );
            assert_eq!(
                row.materials_status,
                HostedProtocolStatus::Supported,
                "{} material capture must be source-backed",
                row.surface
            );
            assert_eq!(
                row.compatibility_metadata_status,
                HostedProtocolStatus::Supported,
                "{} compatibility metadata must be source-backed",
                row.surface
            );
            assert_eq!(
                row.signing_manifest_status,
                HostedProtocolStatus::Supported,
                "{} signing manifest must be source-backed",
                row.surface
            );
            assert_eq!(
                row.publication_status,
                HostedProtocolStatus::Target,
                "{} publication must stay target until registry/signing evidence exists",
                row.surface
            );
            assert!(
                material_script.contains(row.package_name),
                "{} package name must match binding release material script",
                row.surface
            );
            assert!(
                material_script.contains(row.package_kind),
                "{} package kind must match binding release material script",
                row.surface
            );
            assert!(
                material_script.contains(row.build_recipe),
                "{} build recipe must match binding release material script",
                row.surface
            );
            assert!(
                material_script.contains("loom.binding-compatibility.v1"),
                "{} package certification must emit compatibility metadata",
                row.surface
            );
            assert!(
                material_script.contains("loom.binding-signing-manifest.v1"),
                "{} package certification must emit a signing manifest",
                row.surface
            );
            assert!(
                material_script.contains("publication_routes"),
                "{} package certification must declare publication routes",
                row.surface
            );
            assert!(
                material_script.contains("install_validation"),
                "{} package certification must declare install validation",
                row.surface
            );
            assert!(
                material_script.contains("attestation_policy"),
                "{} package certification must declare attestation policy",
                row.surface
            );
            for evidence in row.evidence {
                assert!(
                    repo_root.join(evidence).is_file(),
                    "{} cites missing package certification evidence: {}",
                    row.surface,
                    evidence
                );
            }
        }
    }

    #[test]
    fn promoted_binding_artifacts_cover_runtime_surface_names() {
        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let idl = std::fs::read_to_string(repo_root.join("idl/loom.idl"))
            .expect("idl/loom.idl must be readable");
        let header = std::fs::read_to_string(repo_root.join("include/loom.h"))
            .expect("include/loom.h must be readable");

        for needle in [
            "interface Workspaces",
            "interface KeySource",
            "interface Identity",
            "interface VersionControl",
            "interface FileSystem",
            "interface Cas",
            "interface Queue",
            "interface QueueConsumers",
            "interface Sql",
            "interface Calendar",
            "interface Contacts",
            "interface Mail",
            "interface Graph",
            "interface Vector",
            "interface Columnar",
            "interface Search",
        ] {
            assert!(
                idl.contains(needle),
                "IDL missing promoted surface {needle}"
            );
        }

        for needle in [
            "loom_workspace_create",
            "loom_key_add_wrap_keyed",
            "loom_identity_list_json",
            "loom_acl_grant",
            "loom_write_file",
            "loom_file_open",
            "loom_tag_create",
            "loom_restore_file",
            "loom_revert",
            "loom_squash",
            "loom_cas_put",
            "loom_queue_append",
            "loom_queue_consumer_position",
            "loom_sql_read_table",
            "loom_cal_create_collection",
            "loom_card_create_book",
            "loom_mail_create_mailbox",
            "loom_graph_upsert_node",
            "loom_vector_create",
            "loom_columnar_create",
            "loom_search_create",
        ] {
            assert!(
                header.contains(needle),
                "C header missing promoted function {needle}"
            );
        }
    }

    #[test]
    fn only_executable_core_runs_in_this_certification() {
        // Reporting must not claim a binding runtime suite or an implemented-but-ungated surface as
        // run by this Rust certification. The executable-core surfaces are the in-tree Rust suites;
        // binding runtime suites use their own toolchains.
        let core = binding_surfaces(BindingTier::ExecutableCore);
        let not_ci_gated: Vec<&str> = BINDING_CONFORMANCE_INVENTORY
            .iter()
            .filter(|s| s.tier != BindingTier::ExecutableCore)
            .map(|s| s.name)
            .collect();
        for name in &not_ci_gated {
            assert!(
                !core.contains(name),
                "{name} is not run by this certification and must not appear among the executable-core surfaces"
            );
        }
        // The binding runtime suites that exist today.
        assert_eq!(
            binding_surfaces(BindingTier::BindingRuntimeSuite),
            vec![
                "node",
                "python",
                "ios",
                "cpp",
                "jvm",
                "android",
                "react-native",
                "wasm"
            ]
        );
    }

    #[test]
    fn binding_runtime_coverage_is_recorded() {
        let c_abi = binding_surface_coverage("c-abi").expect("C ABI coverage exists");
        assert!(c_abi.contains(&PROGRAM_LIFECYCLE_C_ABI_COVERAGE));

        let node = binding_surface_coverage("node").expect("node coverage exists");
        assert!(node.contains(&"workspace-lifecycle"));
        assert!(node.contains(&ROLE_ACL_ADMIN_COVERAGE));
        assert!(node.contains(&AUTHENTICATED_SQL_SESSION_COVERAGE));
        assert!(node.contains(&LOCAL_DAEMON_LOCK_CLIENT_COVERAGE));
        assert!(node.contains(&TYPED_LOCK_TOKEN_COVERAGE));
        assert!(node.contains(&SCOPED_LOCK_HELPER_COVERAGE));

        let python = binding_surface_coverage("python").expect("python coverage exists");
        assert!(python.contains(&"queue-consumer"));
        assert!(python.contains(&ROLE_ACL_ADMIN_COVERAGE));
        assert!(python.contains(&AUTHENTICATED_SQL_SESSION_COVERAGE));
        assert!(python.contains(&LOCAL_DAEMON_LOCK_CLIENT_COVERAGE));
        assert!(python.contains(&TYPED_LOCK_TOKEN_COVERAGE));
        assert!(python.contains(&SCOPED_LOCK_HELPER_COVERAGE));

        let ios = binding_surface_coverage("ios").expect("ios coverage exists");
        assert!(ios.contains(&VECTOR_SOURCE_MODEL_COVERAGE));
        assert!(ios.contains(&IDENTITY_ACL_ADMIN_COVERAGE));
        assert!(ios.contains(&SCOPED_ACL_PROTECTED_REF_COVERAGE));
        assert!(ios.contains(&SESSION_AUTH_COVERAGE));

        let cpp = binding_surface_coverage("cpp").expect("cpp coverage exists");
        assert!(cpp.contains(&"queue-consumer"));
        assert!(cpp.contains(&IDENTITY_ACL_ADMIN_COVERAGE));
        assert!(cpp.contains(&SESSION_AUTH_COVERAGE));
        assert!(cpp.contains(&LOCAL_DAEMON_LOCK_CLIENT_COVERAGE));
        assert!(cpp.contains(&TYPED_LOCK_TOKEN_COVERAGE));
        assert!(cpp.contains(&SCOPED_LOCK_HELPER_COVERAGE));

        let jvm = binding_surface_coverage("jvm").expect("jvm coverage exists");
        assert!(jvm.contains(&"sql-result-view"));
        assert!(jvm.contains(&SCOPED_ACL_PROTECTED_REF_COVERAGE));
        assert!(jvm.contains(&AUTHENTICATED_FACET_OPS_COVERAGE));
        assert!(jvm.contains(&AUTHENTICATED_SQL_SESSION_COVERAGE));
        assert!(jvm.contains(&LOCAL_DAEMON_LOCK_CLIENT_COVERAGE));
        assert!(jvm.contains(&TYPED_LOCK_TOKEN_COVERAGE));
        assert!(jvm.contains(&SCOPED_LOCK_HELPER_COVERAGE));

        let android = binding_surface_coverage("android").expect("android coverage exists");
        assert!(android.contains(&"queue-consumer"));
        assert!(android.contains(&SCOPED_ACL_PROTECTED_REF_COVERAGE));
        assert!(android.contains(&AUTHENTICATED_FACET_OPS_COVERAGE));

        let react_native =
            binding_surface_coverage("react-native").expect("react-native coverage exists");
        assert!(react_native.contains(&VECTOR_SOURCE_MODEL_COVERAGE));
        assert!(react_native.contains(&SCOPED_ACL_PROTECTED_REF_COVERAGE));
        assert!(react_native.contains(&AUTHENTICATED_FACET_OPS_COVERAGE));

        let wasm = binding_surface_coverage("wasm").expect("wasm coverage exists");
        assert!(wasm.contains(&"browser-worker-opfs"));
        assert!(wasm.contains(&IDENTITY_ACL_ADMIN_COVERAGE));
        assert!(wasm.contains(&SCOPED_ACL_PROTECTED_REF_COVERAGE));
        assert!(wasm.contains(&SESSION_AUTH_COVERAGE));

        let interop =
            binding_surface_coverage("cross-binding-interop").expect("interop coverage exists");
        assert!(interop.contains(&"shared-store-open-read-write"));
        assert!(interop.contains(&"sql-cross-binding-rows"));
        assert!(interop.contains(&"document-cross-binding-text"));

        let hosted = binding_surface_coverage("hosted-auth-acl").expect("hosted coverage exists");
        assert!(hosted.contains(&"hosted-auth-failure"));
        assert!(hosted.contains(&"hosted-permission-denial"));
        assert!(hosted.contains(&"hosted-security-audit"));
        assert!(hosted.contains(&"hosted-cas-rest"));
        assert!(hosted.contains(&"hosted-fips-listener-rejection"));
        assert!(hosted.contains(&"hosted-admin-rest"));
        assert!(hosted.contains(&"hosted-admin-json-rpc"));
        let hosted_pim =
            binding_surface_coverage("hosted-pim-protocols").expect("hosted PIM coverage exists");
        assert!(hosted_pim.contains(&"mail-imap-bounded-rfc9051-profile"));
        assert!(hosted_pim.contains(&"mail-imap-durable-uid-state"));
        assert!(hosted_pim.contains(&"mail-imap-durable-subscriptions"));
        assert!(hosted_pim.contains(&"mail-imap-common-search-status-fetch-store"));
        assert!(hosted_pim.contains(&"mail-imap-direct-rustls-imaps"));
        assert!(hosted_pim.contains(&"calendar-caldav-bounded-webdav-profile"));
        assert!(hosted_pim.contains(&"calendar-caldav-direct-tls"));
        assert!(hosted_pim.contains(&"contacts-carddav-bounded-webdav-profile"));
        assert!(hosted_pim.contains(&"contacts-carddav-direct-tls"));
        assert!(hosted_pim.contains(&"mail-jmap-bounded-rfc8620-rfc8621-profile"));
        assert!(hosted_pim.contains(&"mail-jmap-blob-upload-download-email-import"));
        assert!(hosted_pim.contains(&"mail-jmap-identity-and-deterministic-state"));
        assert!(hosted_pim.contains(&"mail-jmap-email-changes-querychanges"));
        assert!(hosted_pim.contains(&"mail-jmap-push-unsupported"));
        assert!(hosted_pim.contains(&"mail-jmap-direct-tls"));
        assert!(hosted_pim.contains(&"mail-smtp-setup-compatibility-listener"));
        assert!(hosted_pim.contains(&"mail-smtp-real-submission-unsupported"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-shared-http-uri-webdav-basic-auth"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-uri-rfc3986-percent-encoding"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-webdav-rfc4918-base-methods"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-webdav-rfc5397-current-user-principal"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-webdav-rfc5689-extended-mkcol-unsupported"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-well-known-rfc5785-caldav-carddav"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-well-known-rfc8615-caldav-carddav"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-webdav-rfc6578-sync-collection"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-service-discovery-rfc6764-degraded"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-http-basic-rfc7617-degraded"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-tls-rfc8996-modern-versions"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-http-semantics-rfc9110-bounded-profile"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-http1-rfc9112-shared-stack"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-shared-tls-service-identity-target"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-shared-dns-discovery-target"));
        assert!(hosted_pim.contains(&"calendar-caldav-rfc4791-bounded-access-profile"));
        assert!(hosted_pim.contains(&"calendar-icalendar-rfc5545-bounded-profile"));
        assert!(hosted_pim.contains(&"calendar-caldav-rfc4791-rfc5545-bounded-profile"));
        assert!(hosted_pim.contains(&"calendar-itip-rfc5546-target"));
        assert!(hosted_pim.contains(&"calendar-imip-rfc6047-target"));
        assert!(hosted_pim.contains(&"calendar-caldav-scheduling-rfc6638-target"));
        assert!(hosted_pim.contains(&"calendar-caldav-scheduling-itip-imip-target"));
        assert!(hosted_pim.contains(&"calendar-non-gregorian-recurrence-rfc7529-unsupported"));
        assert!(hosted_pim.contains(&"calendar-caldav-non-gregorian-recurrence-unsupported"));
        assert!(hosted_pim.contains(&"calendar-timezone-reference-rfc7809-target"));
        assert!(hosted_pim.contains(&"calendar-availability-rfc7953-target"));
        assert!(hosted_pim.contains(&"calendar-caldav-timezone-reference-availability-target"));
        assert!(hosted_pim.contains(&"calendar-caldav-rfc7986-extra-properties"));
        assert!(hosted_pim.contains(&"contacts-carddav-rfc6352-bounded-access-profile"));
        assert!(hosted_pim.contains(&"contacts-vcard-rfc6350-bounded-profile"));
        assert!(hosted_pim.contains(&"contacts-carddav-rfc6352-rfc6350-bounded-profile"));
        assert!(hosted_pim.contains(&"contacts-xcard-rfc6351-unsupported"));
        assert!(hosted_pim.contains(&"contacts-carddav-xcard-unsupported"));
        assert!(hosted_pim.contains(&"contacts-place-death-extensions-rfc6474-target"));
        assert!(hosted_pim.contains(&"contacts-carddav-place-death-extensions-target"));
        assert!(hosted_pim.contains(&"contacts-parameter-caret-encoding-rfc6868-target"));
        assert!(hosted_pim.contains(&"contacts-carddav-parameter-caret-encoding-target"));
        assert!(hosted_pim.contains(&"contacts-carddav-vcard3-dialect-supported"));
        assert!(hosted_pim.contains(&"mail-message-rfc5322-bounded-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-imap-rfc9051-bounded-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-jmap-core-rfc8620-bounded-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-jmap-mail-rfc8621-bounded-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-jmap-rfc8620-rfc8621-bounded-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-jmap-blob-rfc9404-bounded-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-jmap-quotas-rfc9425-bounded-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-jscontact-rfc9553-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-vcard-jscontact-extensions-rfc9554-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-jscontact-vcard-conversion-rfc9555-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-jmap-contacts-rfc9610-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-jscalendar-rfc8984-target"));
        assert!(
            hosted_pim.contains(&"mail-rfc-gate-jmap-calendars-draft-ietf-jmap-calendars-target")
        );
        assert!(hosted_pim.contains(&"mail-rfc-gate-jmap-sharing-rfc9670-unsupported"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-web-push-rfc8030-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-vapid-rfc8292-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-jmap-webpush-vapid-rfc9749-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-smtp-rfc5321-setup-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-smtp-submission-rfc6409-setup-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-smtp-starttls-rfc3207-setup-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-email-tls-rfc8314-bounded-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-email-submission-ops-rfc5068-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-smtp-auth-rfc4954-bounded-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-sasl-rfc4422-bounded-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-sasl-plain-rfc4616-bounded-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-mailto-rfc6068-unsupported"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-smtp-size-rfc1870-bounded-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-smtp-pipelining-rfc2920-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-enhanced-status-codes-rfc3463-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-enhanced-status-registry-rfc5248-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-smtp-8bitmime-rfc6152-bounded-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-smtputf8-rfc6531-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-internationalized-headers-rfc6532-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-mime-format-rfc2045-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-mime-media-types-rfc2046-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-mime-encoded-words-rfc2047-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-mime-conformance-rfc2049-target"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-smtp-setup-auth-session-profile"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-smtp-starttls-standard-port-transcript"));
        assert!(hosted_pim.contains(&"mail-rfc-gate-smtp-optional-extensions-mixed-profile"));
        assert!(hosted_pim.contains(&"pim-rfc-gate-live-local-probes"));
        assert!(hosted_pim.contains(&"mail-mutable-state-policy-version-deltas"));
        assert!(hosted_pim.contains(&"mail-mutable-state-merge-audit"));
        assert!(hosted_pim.contains(&"mail-mutable-state-compaction-retained-gap"));
        assert!(hosted_pim.contains(&"pim-owner-only-access-profile"));
        assert!(hosted_pim.contains(&"pim-hooks-registration-envelope-event-emission"));
        assert!(hosted_pim.contains(&"pim-hooks-execution-policy-planning"));
        assert_eq!(binding_surface_coverage("missing"), None);
    }

    #[test]
    fn program_lifecycle_binding_runtime_coverage_is_not_overclaimed() {
        for surface in binding_surfaces(BindingTier::BindingRuntimeSuite) {
            let coverage =
                binding_surface_coverage(surface).expect("binding runtime coverage exists");
            assert!(
                !coverage.contains(&PROGRAM_LIFECYCLE_C_ABI_COVERAGE),
                "{surface} must not claim Program lifecycle C ABI coverage without a checked-in runtime suite"
            );
        }
    }

    #[test]
    fn postgres_client_transcript_inventory_covers_current_and_target_drivers() {
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "postgres"
                && feature.protocol == "tcp"
                && feature.feature == "tokio-postgres-supported-profile-transcript"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "postgres"
                && feature.protocol == "tcp"
                && feature.feature == "tokio-postgres-parameterized-statement-transcript"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "postgres"
                && feature.protocol == "tcp"
                && feature.feature == "postgres-sslrequest-direct-tls-transcript"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "postgres"
                && feature.protocol == "libpq"
                && feature.feature == "guarded-psql-catalog-transcript"
                && feature.status == HostedProtocolStatus::Supported
        }));
        for protocol in ["jdbc", "node", "python", "bi-tool"] {
            assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
                feature.surface == "postgres"
                    && feature.protocol == protocol
                    && feature.feature == "guarded-client-transcript-profile"
                    && feature.status == HostedProtocolStatus::Target
            }));
        }
    }

    #[test]
    fn mysql_client_transcript_inventory_covers_current_and_target_drivers() {
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mysql"
                && feature.protocol == "tcp"
                && feature.feature == "raw-protocol-supported-profile-transcript"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mysql"
                && feature.protocol == "mysql-cli"
                && feature.feature == "guarded-mysql-cli-metadata-transcript"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mysql"
                && feature.protocol == "jdbc"
                && feature.feature == "guarded-connectorj-transcript-profile"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mysql"
                && feature.protocol == "node"
                && feature.feature == "guarded-mysql2-transcript-profile"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mysql"
                && feature.protocol == "python"
                && feature.feature == "guarded-pymysql-mysqlclient-transcript-profile"
                && feature.status == HostedProtocolStatus::Target
        }));
    }

    #[test]
    fn hosted_protocol_inventory_distinguishes_current_and_target_pim_behavior() {
        let mut keys: Vec<(&str, &str, &str)> = HOSTED_PROTOCOL_FEATURES
            .iter()
            .map(|feature| (feature.surface, feature.protocol, feature.feature))
            .collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(
            keys.len(),
            count,
            "hosted protocol feature keys must be unique"
        );

        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        for feature in HOSTED_PROTOCOL_FEATURES {
            let path = repo_root.join(feature.evidence);
            assert!(
                path.is_file(),
                "hosted protocol feature {}/{}/{} cites missing evidence: {}",
                feature.surface,
                feature.protocol,
                feature.feature,
                feature.evidence
            );
        }

        for status in [
            HostedProtocolStatus::Supported,
            HostedProtocolStatus::Degraded,
            HostedProtocolStatus::Target,
            HostedProtocolStatus::Unsupported,
        ] {
            assert!(
                HOSTED_PROTOCOL_FEATURES
                    .iter()
                    .any(|feature| feature.status == status),
                "hosted protocol inventory must include {status:?}"
            );
        }

        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "imap"
                && feature.feature == "durable-subscriptions"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "cas"
                && feature.protocol == "rest-json-rpc"
                && feature.feature == "put-get-missing-get-has-list-delete"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "cas"
                && feature.protocol == "rest-json-rpc"
                && feature.feature == "invalid-digest-post-delete-absence"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "cas"
                && feature.protocol == "grpc"
                && feature.feature == "put-get-missing-get-has-list-delete"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "cas"
                && feature.protocol == "grpc"
                && feature.feature == "invalid-digest-post-delete-absence"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "queue"
                && feature.protocol == "grpc"
                && feature.feature == "append-get-range-len"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "queue"
                && feature.protocol == "rest"
                && feature.feature == "append-get-range-len"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "queue"
                && feature.protocol == "json-rpc"
                && feature.feature == "append-get-range-len"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "time-series"
                && feature.protocol == "grpc"
                && feature.feature == "put-get-latest-range"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "time-series"
                && feature.protocol == "rest"
                && feature.feature == "put-get-latest-range"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "time-series"
                && feature.protocol == "json-rpc"
                && feature.feature == "put-get-latest-range"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "ledger"
                && feature.protocol == "rest"
                && feature.feature == "native-append-get-head-len-verify"
                && feature.status == HostedProtocolStatus::Supported
                && feature.evidence == "crates/loom-protocol-conformance/src/lib.rs"
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "ledger"
                && feature.protocol == "json-rpc"
                && feature.feature == "native-append-get-head-len-verify"
                && feature.status == HostedProtocolStatus::Supported
                && feature.evidence == "crates/loom-protocol-conformance/src/lib.rs"
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "fts"
                && feature.protocol == "rest"
                && feature.feature == "native-collection-management-query-profile"
                && feature.status == HostedProtocolStatus::Supported
                && feature.evidence == "crates/loom-protocol-conformance/src/lib.rs"
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "fts"
                && feature.protocol == "json-rpc"
                && feature.feature == "native-collection-management-query-profile"
                && feature.status == HostedProtocolStatus::Supported
                && feature.evidence == "crates/loom-protocol-conformance/src/lib.rs"
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "imap"
                && feature.feature == "common-search-status-fetch-store"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "jmap"
                && feature.feature == "bounded-rfc8620-rfc8621-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "jmap"
                && feature.feature == "blob-upload-download-email-import"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "jmap"
                && feature.feature == "identity-and-deterministic-state"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "jmap"
                && feature.feature == "email-changes-querychanges"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "jmap"
                && feature.feature == "push"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "smtp"
                && feature.feature == "setup-compatibility-listener"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "smtp"
                && feature.feature == "real-submission-relay-delivery"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "smtp"
                && feature.feature == "optional-extensions-mixed-profile"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "shared-http-uri-webdav-basic-auth"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "uri-rfc3986-percent-encoding"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "webdav-rfc4918-base-methods"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "webdav-rfc5397-current-user-principal"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "webdav-rfc5689-extended-mkcol"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "well-known-rfc5785-caldav-carddav"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "well-known-rfc8615-caldav-carddav"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "webdav-rfc6578-sync-collection"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "service-discovery-rfc6764"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "http-basic-rfc7617"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "tls-rfc8996-modern-versions"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "http-semantics-rfc9110-bounded-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "http1-rfc9112-shared-stack"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "shared-http-over-tls-service-identity"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "rfc-gate"
                && feature.feature == "shared-dns-srv-dns-sd-discovery"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "calendar"
                && feature.protocol == "rfc-gate"
                && feature.feature == "caldav-rfc4791-bounded-access-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "calendar"
                && feature.protocol == "rfc-gate"
                && feature.feature == "icalendar-rfc5545-bounded-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "calendar"
                && feature.protocol == "rfc-gate"
                && feature.feature == "caldav-rfc4791-rfc5545-bounded-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "calendar"
                && feature.protocol == "rfc-gate"
                && feature.feature == "itip-rfc5546"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "calendar"
                && feature.protocol == "rfc-gate"
                && feature.feature == "imip-rfc6047"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "calendar"
                && feature.protocol == "rfc-gate"
                && feature.feature == "caldav-scheduling-rfc6638"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "calendar"
                && feature.protocol == "rfc-gate"
                && feature.feature == "scheduling-itip-imip-freebusy"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "calendar"
                && feature.protocol == "rfc-gate"
                && feature.feature == "non-gregorian-recurrence-rfc7529"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "calendar"
                && feature.protocol == "rfc-gate"
                && feature.feature == "non-gregorian-recurrence"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "calendar"
                && feature.protocol == "rfc-gate"
                && feature.feature == "timezone-reference-rfc7809"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "calendar"
                && feature.protocol == "rfc-gate"
                && feature.feature == "availability-rfc7953"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "calendar"
                && feature.protocol == "rfc-gate"
                && feature.feature == "timezone-reference-and-availability"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "calendar"
                && feature.protocol == "rfc-gate"
                && feature.feature == "rfc7986-extra-properties"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "contacts"
                && feature.protocol == "rfc-gate"
                && feature.feature == "carddav-rfc6352-bounded-access-profile"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "contacts"
                && feature.protocol == "rfc-gate"
                && feature.feature == "vcard-rfc6350-bounded-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "contacts"
                && feature.protocol == "rfc-gate"
                && feature.feature == "carddav-rfc6352-rfc6350-bounded-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "contacts"
                && feature.protocol == "rfc-gate"
                && feature.feature == "xcard-rfc6351"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "contacts"
                && feature.protocol == "rfc-gate"
                && feature.feature == "xcard"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "contacts"
                && feature.protocol == "rfc-gate"
                && feature.feature == "place-death-extensions-rfc6474"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "contacts"
                && feature.protocol == "rfc-gate"
                && feature.feature == "place-death-extensions"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "contacts"
                && feature.protocol == "rfc-gate"
                && feature.feature == "parameter-caret-encoding-rfc6868"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "contacts"
                && feature.protocol == "rfc-gate"
                && feature.feature == "parameter-caret-encoding"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "contacts"
                && feature.protocol == "rfc-gate"
                && feature.feature == "vcard3-dialect-conversion"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "message-rfc5322-bounded-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "imap-rfc9051-bounded-profile"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "jmap-core-rfc8620-bounded-profile"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "jmap-mail-rfc8621-bounded-profile"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "jmap-rfc8620-rfc8621-bounded-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "jmap-blob-rfc9404"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "jmap-quotas-rfc9425"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "jscontact-rfc9553"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "vcard-jscontact-extensions-rfc9554"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "jscontact-vcard-conversion-rfc9555"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "jmap-contacts-rfc9610"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "jscalendar-rfc8984"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "jmap-calendars-draft-ietf-jmap-calendars"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "jmap-sharing-rfc9670"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "web-push-rfc8030"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "vapid-rfc8292"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "jmap-webpush-vapid-rfc9749"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "smtp-rfc5321-setup-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "smtp-submission-rfc6409-setup-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "smtp-starttls-rfc3207-setup-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "email-tls-rfc8314-bounded-profile"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "email-submission-ops-rfc5068"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "smtp-auth-rfc4954-bounded-profile"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "sasl-rfc4422-bounded-profile"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "sasl-plain-rfc4616-bounded-profile"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "mailto-rfc6068"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "smtp-size-rfc1870-bounded-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "smtp-pipelining-rfc2920"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "enhanced-status-codes-rfc3463"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "enhanced-status-registry-rfc5248"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "smtp-8bitmime-rfc6152-bounded-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "smtputf8-rfc6531"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "internationalized-headers-rfc6532"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "mime-format-rfc2045"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "mime-media-types-rfc2046"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "mime-encoded-words-rfc2047"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "mime-conformance-rfc2049"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "smtp-setup-auth-session-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "smtp-starttls-standard-port-transcript"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "rfc-gate"
                && feature.feature == "smtp-optional-extensions-mixed-profile"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "mutable-state"
                && feature.feature == "flag-policy-version-deltas"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "mutable-state"
                && feature.feature == "flag-merge-audit"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "mail"
                && feature.protocol == "mutable-state"
                && feature.feature == "flag-compaction-retained-gap"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "access"
                && feature.feature == "owner-only-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "hooks"
                && feature.feature == "registration-envelope-event-emission"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "pim"
                && feature.protocol == "hooks"
                && feature.feature == "execution-policy-planning"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "vector"
                && feature.protocol == "qdrant-rest"
                && feature.feature == "collection-point-search-scroll-count-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "vector"
                && feature.protocol == "qdrant-grpc"
                && feature.feature == "unary-collection-point-search-scroll-count-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "vector"
                && feature.protocol == "pinecone-rest"
                && feature.feature == "describe-upsert-fetch-query-delete-list-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "vector"
                && feature.protocol == "pinecone-rest"
                && feature.feature == "integrated-embedding-and-sparse-vector-requests"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "vector"
                && feature.protocol == "qdrant-pinecone"
                && feature.feature == "external-generated-client-transcripts"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "oci"
                && feature.protocol == "rest"
                && feature.feature == "distribution-route-family"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "oci"
                && feature.protocol == "rest"
                && feature.feature == "daemon-opened-durable-listener-transcript"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "oci"
                && feature.protocol == "rest"
                && feature.feature == "schema-v1-and-unknown-dangerous-media-types"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "s3"
                && feature.protocol == "rest"
                && feature.feature == "s3-compatible-bucket-object-service"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "s3"
                && feature.protocol == "rest"
                && feature.feature == "daemon-opened-durable-listener-transcript"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "s3"
                && feature.protocol == "rest"
                && feature.feature == "sigv4-app-credential-verification"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "s3"
                && feature.protocol == "rest"
                && feature.feature == "configured-unauthenticated-public-access"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "s3"
                && feature.protocol == "aws-cli"
                && feature.feature == "guarded-sigv4-create-put-get-transcript"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "archive"
                && feature.protocol == "tar-zstd-tar-gzip-zip"
                && feature.feature == "file-tree-import-export"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "archive"
                && feature.protocol == "cli"
                && feature.feature == "interchange-import-export-archive"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "car"
                && feature.protocol == "car"
                && feature.feature == "deterministic-import-export"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "car"
                && feature.protocol == "cli"
                && feature.feature == "interchange-import-export-car"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "redis"
                && feature.protocol == "resp"
                && feature.feature == "strings-ttl-hash-set-list-zset-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "redis"
                && feature.protocol == "resp"
                && feature.feature == "stream-and-pubsub-family-boundaries"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "memcached"
                && feature.protocol == "text"
                && feature.feature == "volatile-cache-command-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "memcached"
                && feature.protocol == "text"
                && feature.feature == "durable-or-backed-cache-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "memcached"
                && feature.protocol == "text"
                && feature.feature == "guarded-client-transcript-profile"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "columnar"
                && feature.protocol == "rest"
                && feature.feature == "native-management-query-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "columnar"
                && feature.protocol == "json-rpc"
                && feature.feature == "native-management-query-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "columnar"
                && feature.protocol == "arrow-flight-flight-sql"
                && feature.feature == "binary-analytical-data-plane"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "dataframe"
                && feature.protocol == "rest"
                && feature.feature == "native-management-profile"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "dataframe"
                && feature.protocol == "arrow-flight-flight-sql"
                && feature.feature == "binary-result-transfer"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "dataframe"
                && feature.protocol == "json-rpc-grpc"
                && feature.feature == "generated-client-symmetry"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "exec"
                && feature.protocol == "rest"
                && feature.feature == "raw-cbor-run-listener"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "exec"
                && feature.protocol == "json-rpc"
                && feature.feature == "raw-cbor-run-listener"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "exec"
                && feature.protocol == "grpc"
                && feature.feature == "raw-cbor-run-listener"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "delivery"
                && feature.protocol == "websocket"
                && feature.feature == "durable-subscribe-ack-replay"
                && feature.status == HostedProtocolStatus::Target
        }));
    }

    #[test]
    fn hosted_protocol_inventory_distinguishes_current_and_target_kafka_behavior() {
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "kafka"
                && feature.protocol == "tcp"
                && feature.feature == "api-versions-sasl-plain-auth"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "kafka"
                && feature.protocol == "tcp"
                && feature.feature == "durable-topic-metadata"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "kafka"
                && feature.protocol == "tcp"
                && feature.feature == "shared-durable-metadata-version-allocation"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "kafka"
                && feature.protocol == "tcp"
                && feature.feature == "single-node-single-partition-metadata"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "kafka"
                && feature.protocol == "tcp"
                && feature.feature == "uncompressed-record-batch-produce-fetch-offset-commit"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "kafka"
                && feature.protocol == "tcp"
                && feature.feature == "normalized-record-batch-offset-rewrite"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "kafka"
                && feature.protocol == "tcp"
                && feature.feature == "compressed-record-batches"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "kafka"
                && feature.protocol == "tcp"
                && feature.feature == "producer-id-epoch-fencing"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "kafka"
                && feature.protocol == "tcp"
                && feature.feature == "bounded-transaction-control"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "kafka"
                && feature.protocol == "tcp"
                && feature.feature == "idempotent-produce-sequence-validation"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "kafka"
                && feature.protocol == "tcp"
                && feature.feature == "transactional-offset-atomic-visibility"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "kafka"
                && feature.protocol == "tcp"
                && feature.feature == "transactional-produced-record-visibility"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "kafka"
                && feature.protocol == "tcp"
                && feature.feature == "multi-broker-replication-isr-election"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
    }

    #[test]
    fn hosted_protocol_inventory_distinguishes_current_and_target_etcd_behavior() {
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "etcd"
                && feature.protocol == "tcp"
                && feature.feature == "first-class-listener-admission"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "etcd"
                && feature.protocol == "tcp"
                && feature.feature == "kv-lease-compact-grpc-methods"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "etcd"
                && feature.protocol == "tcp"
                && feature.feature == "durable-revision-lease-compaction-metadata"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "etcd"
                && feature.protocol == "tcp"
                && feature.feature == "single-authority-static-cluster"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "etcd"
                && feature.protocol == "tcp"
                && feature.feature == "bounded-watch-replay-event-log"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "etcd"
                && feature.protocol == "tcp"
                && feature.feature == "live-watch-tail"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "etcd"
                && feature.protocol == "tcp"
                && feature.feature == "member-cluster-auth-maintenance-apis"
                && feature.status == HostedProtocolStatus::Target
        }));
        assert!(HOSTED_PROTOCOL_FEATURES.iter().any(|feature| {
            feature.surface == "etcd"
                && feature.protocol == "tcp"
                && feature.feature == "multi-member-raft-quorum"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
    }

    #[test]
    fn local_coordination_inventory_distinguishes_supported_degraded_and_unsupported_surfaces() {
        let mut keys: Vec<(&str, &str)> = LOCAL_COORDINATION_FEATURES
            .iter()
            .map(|feature| (feature.surface, feature.feature))
            .collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(
            keys.len(),
            count,
            "local coordination feature keys must be unique"
        );

        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        for feature in LOCAL_COORDINATION_FEATURES {
            assert!(
                !feature.evidence.is_empty(),
                "local coordination feature {}/{} must cite evidence",
                feature.surface,
                feature.feature
            );
            for evidence in feature.evidence {
                let path = repo_root.join(evidence);
                assert!(
                    path.is_file(),
                    "local coordination feature {}/{} cites missing evidence: {}",
                    feature.surface,
                    feature.feature,
                    evidence
                );
            }
        }

        for status in [
            HostedProtocolStatus::Supported,
            HostedProtocolStatus::Degraded,
            HostedProtocolStatus::Unsupported,
        ] {
            assert!(
                LOCAL_COORDINATION_FEATURES
                    .iter()
                    .any(|feature| feature.status == status),
                "local coordination inventory must include {status:?}"
            );
        }

        assert!(LOCAL_COORDINATION_FEATURES.iter().any(|feature| {
            feature.surface == "embedded-coordinator"
                && feature.feature == "leased-fenced-lock-behavior"
                && feature.status == HostedProtocolStatus::Supported
        }));
        assert!(LOCAL_COORDINATION_FEATURES.iter().any(|feature| {
            feature.surface == "cli-daemon"
                && feature.feature == "tcp-loopback-transport"
                && feature.status == HostedProtocolStatus::Degraded
        }));
        assert!(LOCAL_COORDINATION_FEATURES.iter().any(|feature| {
            feature.surface == "hosted-locks"
                && feature.feature == "public-lock-protocol"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
        assert!(LOCAL_COORDINATION_FEATURES.iter().any(|feature| {
            feature.surface == "mobile-browser-bindings"
                && feature.feature == "cli-daemon-lock-client"
                && feature.status == HostedProtocolStatus::Unsupported
        }));
    }

    #[test]
    fn report_serializes_the_source_backed_boundary() {
        let report = report_memory_store_manifest();

        // Implementation identity comes from the crate metadata; nothing is fabricated.
        assert_eq!(report.implementation_name, env!("CARGO_PKG_NAME"));
        assert_eq!(report.implementation_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(report.source_revision, None);
        assert_eq!(report.status, ReportStatus::Passed);
        assert_eq!(
            report.runtime_profile,
            RuntimeProfileReport::from(runtime_profile())
        );

        // Vector suites are exactly the certified canonical suites, all passed.
        assert_eq!(
            report
                .vector_suites
                .iter()
                .map(|s| s.name)
                .collect::<Vec<_>>(),
            CANONICAL_VECTOR_SUITES.to_vec()
        );
        assert!(
            report
                .vector_suites
                .iter()
                .all(|s| s.status == ReportStatus::Passed)
        );

        // Behavior suites partition into executable (passed) and declarative (inventory), matching the
        // full BEHAVIOR_SUITES inventory exactly.
        let passed: Vec<&str> = report
            .behavior_suites
            .iter()
            .filter(|s| s.status == ReportStatus::Passed)
            .map(|s| s.name)
            .collect();
        assert_eq!(passed, behavior::EXECUTABLE_BEHAVIOR_SUITES.to_vec());
        assert!(
            report
                .behavior_suites
                .iter()
                .all(|s| s.status == ReportStatus::Passed || s.status == ReportStatus::Inventory)
        );
        assert_eq!(
            report.behavior_suites.len(),
            behavior::BEHAVIOR_SUITES.len()
        );
        assert_eq!(report.total_scenarios, {
            let n: usize = behavior::BEHAVIOR_SUITES
                .iter()
                .map(|(_, scenarios)| scenarios.len())
                .sum();
            n
        });
    }

    #[test]
    fn report_does_not_overclaim_bindings_or_targets() {
        let report = report_memory_store_manifest();

        for b in &report.binding_surfaces {
            match b.tier {
                // Only the canonical-vectors core suite is executed by this certification.
                BindingTier::ExecutableCore if b.name == "canonical-vectors" => {
                    assert_eq!(b.status, ReportStatus::Passed);
                }
                BindingTier::ExecutableCore
                | BindingTier::BindingRuntimeSuite
                | BindingTier::CrossBindingInterop => {
                    // Exists but not run here: skipped with a reason, never passed.
                    assert!(matches!(b.status, ReportStatus::Skipped(_)), "{}", b.name);
                }
                BindingTier::ImplementedNotGated => assert_eq!(b.status, ReportStatus::Inventory),
                BindingTier::TargetOnly => {
                    assert_eq!(b.status, ReportStatus::Target);
                    assert!(
                        b.evidence.is_empty(),
                        "target {} must cite no evidence",
                        b.name
                    );
                }
            }
            // No binding family is ever reported as a runtime pass by this certification.
            if matches!(
                b.name,
                "node" | "python" | "ios" | "cpp" | "jvm" | "android" | "react-native" | "wasm"
            ) {
                assert_ne!(
                    b.status,
                    ReportStatus::Passed,
                    "{} must not be claimed passed",
                    b.name
                );
                assert!(
                    !b.coverage.is_empty(),
                    "{} must carry explicit runtime coverage",
                    b.name
                );
            }
        }
    }

    #[test]
    fn pim_certification_profile_records_enterprise_gate() {
        let report = report_memory_store_manifest();
        let profile = &report.certification_profile;

        assert_eq!(profile.name, "pim-owner-only-enterprise-v1");
        assert_eq!(profile.admin_profile_key, "admin.certification.profile");
        assert_eq!(profile.owner_scope, "owner-only");
        assert_eq!(profile.redaction_policy, PIM_REDACTION_POLICY);
        assert_eq!(
            profile.conformance_suites,
            PIM_CERTIFICATION_SUITES.to_vec()
        );
        assert_eq!(
            profile.required_clients,
            PIM_CERTIFICATION_CLIENT_REQUIREMENTS.to_vec()
        );
        assert!(profile.redaction_policy.redact.contains(&"authorization"));
        assert!(
            profile
                .redaction_policy
                .redact
                .contains(&"raw-message-body")
        );
        assert!(profile.redaction_policy.retain.contains(&"error-code"));

        for expected in [
            ("caldav", "Apple Calendar"),
            ("caldav", "Thunderbird"),
            ("caldav", "DAVx5"),
            ("carddav", "Apple Contacts"),
            ("carddav", "Thunderbird"),
            ("carddav", "DAVx5"),
            ("imap", "Apple Mail"),
            ("imap", "Thunderbird"),
            ("jmap", "RFC 8620/8621 executable transcript"),
        ] {
            assert!(
                profile
                    .required_clients
                    .iter()
                    .any(|client| { client.protocol == expected.0 && client.client == expected.1 }),
                "missing certification target {expected:?}"
            );
        }
        assert!(profile.required_clients.iter().any(|client| {
            client.protocol == "jmap"
                && client.client == "RFC 8620/8621 executable transcript"
                && client.status == CertificationStatus::Passed
                && client.evidence == "crates/loom-hosted/src/jmap.rs"
        }));
        assert!(profile.required_clients.iter().any(|client| {
            client.protocol == "caldav"
                && client.client == "Apple Calendar"
                && client.status == CertificationStatus::Target
        }));
    }

    #[test]
    fn pim_transcript_inventory_is_redacted_fixture_shape() {
        let report = report_memory_store_manifest();

        assert_eq!(
            report.transcript_inventory,
            PIM_TRANSCRIPT_INVENTORY.to_vec()
        );
        assert_eq!(report.transcript_inventory.len(), 10);
        assert!(
            report
                .transcript_inventory
                .iter()
                .all(
                    |transcript| transcript.redaction_profile == PIM_REDACTION_POLICY.name
                        && !transcript.reason.is_empty()
                )
        );
        assert!(report.transcript_inventory.iter().any(|transcript| {
            transcript.name == "jmap-rfc8620-rfc8621-owner-only"
                && transcript.client == "RFC 8620/8621 executable transcript"
                && transcript.status == CertificationStatus::Passed
                && transcript.evidence == "crates/loom-hosted/src/jmap.rs"
        }));
        assert!(report.transcript_inventory.iter().any(|transcript| {
            transcript.name == "caldav-apple-calendar-owner-only"
                && transcript.status == CertificationStatus::Target
        }));
    }

    #[test]
    fn real_listener_transcript_runner_separates_profile_outcomes() {
        let scenarios = [
            RealListenerTranscriptScenario {
                name: "jmap-supported-listener",
                surface: "mail",
                protocol: "jmap",
                profile: Some("rfc8620-rfc8621"),
                client: "protocol transcript",
                class: RealListenerTranscriptClass::Supported,
                expected_status: CertificationStatus::Passed,
                evidence_kind: TranscriptEvidenceKind::RealListener,
                evidence: "crates/loom-hosted/src/jmap.rs",
            },
            RealListenerTranscriptScenario {
                name: "postgres-guarded-psql",
                surface: "postgres",
                protocol: "tcp",
                profile: Some("libpq"),
                client: "psql",
                class: RealListenerTranscriptClass::Guarded,
                expected_status: CertificationStatus::Skipped,
                evidence_kind: TranscriptEvidenceKind::GuardedOptionalTool,
                evidence: "crates/loom-hosted/src/pg_wire.rs",
            },
            RealListenerTranscriptScenario {
                name: "opensearch-unavailable-listener",
                surface: "fts",
                protocol: "opensearch-rest",
                profile: Some("opensearch"),
                client: "opensearch-rust",
                class: RealListenerTranscriptClass::Unavailable,
                expected_status: CertificationStatus::Failed,
                evidence_kind: TranscriptEvidenceKind::UnavailableListener,
                evidence: "crates/loom-hosted/src/serve.rs",
            },
            RealListenerTranscriptScenario {
                name: "smtp-unsupported-submission",
                surface: "mail",
                protocol: "smtp",
                profile: Some("submission"),
                client: "smtp probe",
                class: RealListenerTranscriptClass::Unsupported,
                expected_status: CertificationStatus::Unsupported,
                evidence_kind: TranscriptEvidenceKind::UnsupportedBoundary,
                evidence: "crates/loom-hosted/src/smtp.rs",
            },
        ];

        let outcomes = run_real_listener_transcript_scenarios(&scenarios, |scenario| {
            let observation = match scenario.class {
                RealListenerTranscriptClass::Supported => RealListenerTranscriptObservation {
                    status: CertificationStatus::Passed,
                    reason: "real listener accepted and returned a bounded transcript",
                    stable_error: None,
                    events: vec![NormalizedTranscriptEvent {
                        direction: "request",
                        protocol: scenario.protocol,
                        summary: "capability probe",
                    }],
                },
                RealListenerTranscriptClass::Guarded => RealListenerTranscriptObservation {
                    status: CertificationStatus::Skipped,
                    reason: "optional client tool is absent",
                    stable_error: None,
                    events: Vec::new(),
                },
                RealListenerTranscriptClass::Unavailable => RealListenerTranscriptObservation {
                    status: CertificationStatus::Failed,
                    reason: "listener bind failed before transcript capture",
                    stable_error: Some(Code::Unavailable),
                    events: Vec::new(),
                },
                RealListenerTranscriptClass::Unsupported => RealListenerTranscriptObservation {
                    status: CertificationStatus::Unsupported,
                    reason: "profile is outside the declared bounded surface",
                    stable_error: Some(Code::Unsupported),
                    events: Vec::new(),
                },
            };
            Ok(observation)
        })
        .expect("real-listener transcript runner must accept the profile matrix");

        assert_eq!(outcomes.len(), scenarios.len());
        assert!(outcomes.iter().any(|outcome| {
            outcome.class == RealListenerTranscriptClass::Supported && outcome.event_count == 1
        }));
        assert!(outcomes.iter().any(|outcome| {
            outcome.class == RealListenerTranscriptClass::Unavailable
                && outcome.stable_error == Some(Code::Unavailable)
        }));
        assert!(outcomes.iter().any(|outcome| {
            outcome.class == RealListenerTranscriptClass::Unsupported
                && outcome.stable_error == Some(Code::Unsupported)
        }));
    }

    #[test]
    fn real_listener_transcript_runner_rejects_mock_supported_claims() {
        let scenario = RealListenerTranscriptScenario {
            name: "mock-supported-listener",
            surface: "mail",
            protocol: "jmap",
            profile: Some("rfc8620-rfc8621"),
            client: "mock",
            class: RealListenerTranscriptClass::Supported,
            expected_status: CertificationStatus::Passed,
            evidence_kind: TranscriptEvidenceKind::MockHandler,
            evidence: "crates/loom-hosted/src/jmap.rs",
        };

        let err = run_real_listener_transcript_scenarios(&[scenario], |_| {
            Ok(RealListenerTranscriptObservation {
                status: CertificationStatus::Passed,
                reason: "mock handler returned success",
                stable_error: None,
                events: vec![NormalizedTranscriptEvent {
                    direction: "request",
                    protocol: "jmap",
                    summary: "mocked request",
                }],
            })
        })
        .expect_err("mock handler evidence must not support a compatibility claim");

        assert_eq!(err.code, Code::InvalidArgument);
        assert!(err.message.contains("real listener"));
    }

    #[test]
    fn mature_client_differential_evidence_gate_accepts_current_inventory() {
        run_mature_client_differential_evidence_gate(MATURE_CLIENT_DIFFERENTIAL_EVIDENCE)
            .expect("mature-client differential evidence gate must accept current inventory");

        assert!(
            MATURE_CLIENT_DIFFERENTIAL_EVIDENCE
                .iter()
                .any(|row| row.client_kind == DifferentialClientKind::OfficialSdk
                    && row.status == CertificationStatus::Passed)
        );
        assert!(
            MATURE_CLIENT_DIFFERENTIAL_EVIDENCE
                .iter()
                .any(
                    |row| row.client_kind == DifferentialClientKind::MatureClient
                        && row.status == CertificationStatus::Skipped
                )
        );
        assert!(MATURE_CLIENT_DIFFERENTIAL_EVIDENCE.iter().any(|row| {
            row.client_kind == DifferentialClientKind::GeneratedClient
                && row.status == CertificationStatus::Target
                && row.stable_error == Some(Code::Unsupported)
        }));
    }

    #[test]
    fn mature_client_differential_evidence_gate_rejects_generated_supported_claims() {
        let row = MatureClientDifferentialEvidence {
            surface: "vector",
            protocol: "qdrant-rest",
            profile: Some("generated-client"),
            client: "generated vector client",
            client_kind: DifferentialClientKind::GeneratedClient,
            status: CertificationStatus::Passed,
            evidence: "crates/loom-hosted/src/vector_compat.rs",
            drift_policy: "generated client replay",
            stable_error: None,
        };

        let err = run_mature_client_differential_evidence_gate(&[row])
            .expect_err("generated clients must not promote supported compatibility alone");

        assert_eq!(err.code, Code::InvalidArgument);
        assert!(err.message.contains("real client"));
    }

    #[test]
    fn capability_transcript_evidence_gate_accepts_current_matrix() {
        run_capability_transcript_evidence_gate(
            CAPABILITY_MATRIX,
            PIM_TRANSCRIPT_INVENTORY,
            MATURE_CLIENT_DIFFERENTIAL_EVIDENCE,
        )
        .expect("capability transcript evidence gate must accept current matrix");

        assert!(CAPABILITY_MATRIX.iter().any(|row| {
            row.surface == "fts"
                && row.profile == Some("opensearch")
                && row.status == HostedProtocolStatus::Supported
        }));
        assert!(CAPABILITY_MATRIX.iter().any(|row| {
            row.surface == "vector"
                && row.profile == Some("qdrant")
                && row.status == HostedProtocolStatus::Target
        }));
        assert!(CAPABILITY_MATRIX.iter().any(|row| {
            row.surface == "vector"
                && row.profile == Some("pinecone")
                && row.status == HostedProtocolStatus::Target
        }));
    }

    #[test]
    fn capability_transcript_evidence_gate_rejects_supported_profile_without_transcript() {
        let row = CapabilityMatrixRow {
            plane: CapabilityPlane::HostedTierTwo,
            surface: "vector",
            transport: "rest",
            profile: Some("qdrant"),
            status: HostedProtocolStatus::Supported,
            evidence: &["crates/loom-hosted/src/serve.rs"],
        };

        let err = run_capability_transcript_evidence_gate(
            &[row],
            PIM_TRANSCRIPT_INVENTORY,
            MATURE_CLIENT_DIFFERENTIAL_EVIDENCE,
        )
        .expect_err("supported profile claims must require transcript evidence");

        assert_eq!(err.code, Code::InvalidArgument);
        assert!(err.message.contains("transcript evidence"));
    }

    #[test]
    fn capability_transcript_evidence_gate_rejects_wrong_protocol_transcript() {
        let row = CapabilityMatrixRow {
            plane: CapabilityPlane::HostedTierTwo,
            surface: "mail",
            transport: "imap",
            profile: Some("imap-rfc9051"),
            status: HostedProtocolStatus::Supported,
            evidence: &["crates/loom-hosted/src/imap.rs"],
        };

        let err = run_capability_transcript_evidence_gate(
            &[row],
            PIM_TRANSCRIPT_INVENTORY,
            MATURE_CLIENT_DIFFERENTIAL_EVIDENCE,
        )
        .expect_err("same-surface transcript with the wrong protocol must not satisfy a profile");

        assert_eq!(err.code, Code::InvalidArgument);
        assert!(err.message.contains("mail/imap-rfc9051"));
    }

    #[test]
    fn queue7_pim_capability_report_fixture_is_complete() {
        let mut keys: Vec<(&str, &str, &str, &str)> = QUEUE7_PIM_CAPABILITY_REPORT_FIXTURES
            .iter()
            .map(|row| (row.category, row.surface, row.protocol, row.feature))
            .collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), count, "PIM fixture rows must be unique");

        let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        for row in QUEUE7_PIM_CAPABILITY_REPORT_FIXTURES {
            let path = repo_root.join(row.evidence);
            assert!(
                path.is_file(),
                "PIM fixture {}/{}/{} cites missing evidence: {}",
                row.surface,
                row.protocol,
                row.feature,
                row.evidence
            );
        }

        for status in [
            "supported",
            "degraded",
            "target",
            "unsupported",
            "failed",
            "skipped",
            "passed",
        ] {
            assert!(
                QUEUE7_PIM_CAPABILITY_REPORT_FIXTURES
                    .iter()
                    .any(|row| row.status == status),
                "PIM fixture must include {status} status"
            );
        }

        for category in [
            "capability",
            "supported",
            "degraded",
            "target",
            "unsupported",
            "status-shape",
            "certification",
            "direct-tls",
            "retained-gap",
            "lifecycle",
            "rfc-gate",
            "client-transcript",
        ] {
            assert!(
                QUEUE7_PIM_CAPABILITY_REPORT_FIXTURES
                    .iter()
                    .any(|row| row.category == category),
                "PIM fixture must include {category} category"
            );
        }

        assert!(QUEUE7_PIM_CAPABILITY_REPORT_FIXTURES.iter().any(|row| {
            row.category == "status-shape"
                && row.feature == "failed-row-shape"
                && row.status == "failed"
        }));
        assert!(QUEUE7_PIM_CAPABILITY_REPORT_FIXTURES.iter().any(|row| {
            row.category == "status-shape"
                && row.feature == "skipped-row-shape"
                && row.status == "skipped"
        }));
    }

    #[test]
    fn queue7_pim_capability_report_fixture_json_is_stable() {
        let json = queue7_pim_capability_report_fixture_json();

        assert_eq!(json, queue7_pim_capability_report_fixture_json());
        assert_eq!(json.matches('{').count(), json.matches('}').count());
        assert_eq!(json.matches('[').count(), json.matches(']').count());
        for expected in [
            "\"fixture\":\"queue7-pim-capability-report\"",
            "\"category\":\"capability\"",
            "\"status\":\"failed\"",
            "\"status\":\"skipped\"",
            "\"category\":\"certification\"",
            "\"category\":\"direct-tls\"",
            "\"category\":\"retained-gap\"",
            "\"category\":\"lifecycle\"",
            "\"category\":\"rfc-gate\"",
            "\"category\":\"client-transcript\"",
            "\"feature\":\"jmap-rfc8620-rfc8621-owner-only\"",
        ] {
            assert!(
                json.contains(expected),
                "PIM fixture JSON missing {expected}"
            );
        }
    }

    #[test]
    fn report_json_is_stable_and_well_formed() {
        let report = report_memory_store_manifest();
        let json = report.to_json();

        // Deterministic: serializing the same report twice yields identical bytes.
        assert_eq!(json, report.to_json());

        // Balanced delimiters and the expected top-level keys.
        assert_eq!(json.matches('{').count(), json.matches('}').count());
        assert_eq!(json.matches('[').count(), json.matches(']').count());
        for key in [
            "\"implementation\"",
            "\"source_revision\"",
            "\"runtime_profile\"",
            "\"binary_channel\"",
            "\"runtime_policy\"",
            "\"default_identity_profile\"",
            "\"crypto_provider\"",
            "\"tls_provider\"",
            "\"fips_capable\"",
            "\"fips_tls_claim\"",
            "\"identity_profiles\"",
            "\"vector_suites\"",
            "\"behavior_suites\"",
            "\"binding_surfaces\"",
            "\"hosted_protocols\"",
            "\"local_coordination\"",
            "\"capability_matrix\"",
            "\"release_certification\"",
            "\"binding_runtime_certification\"",
            "\"binding_package_certification\"",
            "\"certification_profile\"",
            "\"transcript_inventory\"",
            "\"redaction_policy\"",
            "\"required_clients\"",
            "\"coverage\"",
            "\"total_scenarios\"",
        ] {
            assert!(json.contains(key), "report JSON missing {key}");
        }
        // A skipped binding surface carries its reason; no suite is ever serialized as failed.
        assert!(json.contains("\"status\":\"skipped\""));
        assert!(json.contains("\"status\":\"target\""));
        assert!(json.contains("\"status\":\"supported\""));
        assert!(json.contains("\"status\":\"degraded\""));
        assert!(json.contains("\"status\":\"unsupported\""));
        assert!(json.contains("\"surface\":\"embedded-coordinator\""));
        assert!(json.contains("\"surface\":\"host-native-bindings\""));
        assert!(json.contains("\"surface\":\"mobile-browser-bindings\""));
        assert!(json.contains("\"surface\":\"vector\",\"protocol\":\"qdrant-rest\""));
        assert!(json.contains("\"surface\":\"vector\",\"protocol\":\"qdrant-grpc\""));
        assert!(json.contains("\"surface\":\"vector\",\"protocol\":\"pinecone-rest\""));
        assert!(json.contains("\"surface\":\"postgres\",\"protocol\":\"tcp\""));
        assert!(json.contains("\"surface\":\"postgres\",\"protocol\":\"libpq\""));
        assert!(json.contains("\"surface\":\"postgres\",\"protocol\":\"jdbc\""));
        assert!(json.contains("\"surface\":\"postgres\",\"protocol\":\"node\""));
        assert!(json.contains("\"surface\":\"postgres\",\"protocol\":\"python\""));
        assert!(json.contains("\"surface\":\"postgres\",\"protocol\":\"bi-tool\""));
        assert!(json.contains("\"feature\":\"tokio-postgres-supported-profile-transcript\""));
        assert!(json.contains("\"feature\":\"tokio-postgres-parameterized-statement-transcript\""));
        assert!(json.contains("\"feature\":\"postgres-sslrequest-direct-tls-transcript\""));
        assert!(json.contains("\"feature\":\"guarded-psql-catalog-transcript\""));
        assert!(json.contains("\"plane\":\"hosted-tier-one\",\"surface\":\"graph\""));
        assert!(json.contains("\"plane\":\"hosted-tier-one\",\"surface\":\"ledger\""));
        assert!(json.contains("\"plane\":\"hosted-tier-two\",\"surface\":\"fts\""));
        assert!(json.contains("\"profile\":\"opensearch\""));
        assert!(json.contains("\"plane\":\"provider\",\"surface\":\"direct-tls-rustls\""));
        assert!(json.contains("\"category\":\"binding-package-materials\""));
        assert!(json.contains("\"category\":\"binding-package-compatibility\""));
        assert!(json.contains("\"category\":\"binding-package-signing-materials\""));
        assert!(json.contains("\"category\":\"binding-package-promotion-policy\""));
        assert!(json.contains("\"category\":\"binding-package-publication-gate\""));
        assert!(json.contains("\"target\":\"fips-binding-release-materials\""));
        assert!(json.contains("\"target\":\"core-abi-version-metadata\""));
        assert!(json.contains("\"target\":\"unsigned-binding-signing-manifest\""));
        assert!(
            json.contains("\"target\":\"registry-route-credential-and-install-validation-policy\"")
        );
        assert!(json.contains("\"target\":\"protected-dry-run-publication-gate\""));
        assert!(json.contains("\"category\":\"binding-interop\",\"surface\":\"node-python\""));
        assert!(json.contains("\"target\":\"shared-store-open-read-write\""));
        assert!(json.contains("\"category\":\"browser-runtime\",\"surface\":\"wasm\""));
        assert!(json.contains("\"target\":\"android-connected-host-fixture\""));
        assert!(json.contains("\"workflow\":\".github/workflows/bindings.yml\""));
        assert!(json.contains("\"package_name\":\"@uldrenai/loom-react-native\""));
        assert!(json.contains("\"compatibility_metadata_status\":\"supported\""));
        assert!(json.contains("\"signing_manifest_status\":\"supported\""));
        assert!(json.contains("\"publication_status\":\"target\""));
        assert!(json.contains("\"category\":\"provider-profile\""));
        assert!(json.contains("\"feature\":\"exact-only-capability-reporting\""));
        assert!(json.contains("\"surface\":\"oci\",\"protocol\":\"rest\""));
        assert!(json.contains("\"feature\":\"distribution-route-family\""));
        assert!(json.contains("\"feature\":\"daemon-opened-durable-listener-transcript\""));
        assert!(json.contains("\"feature\":\"monolithic-and-chunked-upload-digest-verification\""));
        assert!(json.contains("\"feature\":\"repository-tags-catalog-referrers\""));
        assert!(json.contains("\"surface\":\"s3\",\"protocol\":\"rest\""));
        assert!(json.contains("\"surface\":\"s3\",\"protocol\":\"aws-cli\""));
        assert!(json.contains("\"feature\":\"guarded-sigv4-create-put-get-transcript\""));
        assert!(json.contains("\"surface\":\"archive\",\"protocol\":\"tar-zstd-tar-gzip-zip\""));
        assert!(json.contains("\"surface\":\"archive\",\"protocol\":\"cli\""));
        assert!(json.contains("\"surface\":\"car\",\"protocol\":\"car\""));
        assert!(json.contains("\"surface\":\"redis\",\"protocol\":\"resp\""));
        assert!(json.contains("\"feature\":\"strings-ttl-hash-set-list-zset-profile\""));
        assert!(json.contains("\"feature\":\"stream-and-pubsub-family-boundaries\""));
        assert!(json.contains("\"surface\":\"memcached\",\"protocol\":\"text\""));
        assert!(json.contains("\"feature\":\"volatile-cache-command-profile\""));
        assert!(json.contains("\"feature\":\"durable-or-backed-cache-profile\""));
        assert!(json.contains("\"surface\":\"kv\",\"protocol\":\"rest\""));
        assert!(json.contains("\"surface\":\"kv\",\"protocol\":\"json-rpc\""));
        assert!(json.contains("\"feature\":\"native-put-get-delete-list-range\""));
        assert!(json.contains("\"surface\":\"fts\",\"protocol\":\"rest\""));
        assert!(json.contains("\"surface\":\"fts\",\"protocol\":\"json-rpc\""));
        assert!(json.contains("\"surface\":\"fts\",\"protocol\":\"opensearch-rest\""));
        assert!(json.contains(
            "\"feature\":\"terms-missing-range-histogram-value-count-avg-sum-min-max-stats-cardinality-aggregations\""
        ));
        assert!(json.contains("\"feature\":\"match-all-and-analyzer-boundary\""));
        assert!(json.contains("\"feature\":\"aliases-multi-index-wildcards\""));
        assert!(json.contains("\"feature\":\"bulk-independent-item-errors\""));
        assert!(json.contains("\"feature\":\"security-read-shims\""));
        assert!(json.contains("\"feature\":\"security-mutation-apis\""));
        assert!(json.contains("\"feature\":\"route-matrix-certification\""));
        assert!(json.contains("\"feature\":\"nested-pipeline-aggregations\""));
        assert!(json.contains("\"feature\":\"full-analyzer-execution\""));
        assert!(json.contains("\"feature\":\"external-generated-client-transcripts\""));
        assert!(json.contains("\"surface\":\"columnar\",\"protocol\":\"rest\""));
        assert!(json.contains("\"feature\":\"arrow-ipc-parquet-binary-transfer\""));
        assert!(json.contains("\"surface\":\"columnar\",\"protocol\":\"json-rpc\""));
        assert!(json.contains("\"surface\":\"columnar\",\"protocol\":\"arrow-flight-flight-sql\""));
        assert!(json.contains("\"feature\":\"binary-analytical-data-plane\""));
        assert!(json.contains("\"surface\":\"dataframe\",\"protocol\":\"rest\""));
        assert!(
            json.contains("\"surface\":\"dataframe\",\"protocol\":\"arrow-flight-flight-sql\"")
        );
        assert!(json.contains("\"surface\":\"dataframe\",\"protocol\":\"json-rpc-grpc\""));
        assert!(json.contains("\"feature\":\"binary-result-transfer\""));
        assert!(json.contains("\"feature\":\"profile-transcript-capture\""));
        assert!(json.contains("\"feature\":\"executable-rfc8620-rfc8621-transcript\""));
        assert!(json.contains("\"feature\":\"shared-http-uri-webdav-basic-auth\""));
        assert!(json.contains("\"feature\":\"shared-http-over-tls-service-identity\""));
        assert!(json.contains("\"feature\":\"shared-dns-srv-dns-sd-discovery\""));
        assert!(json.contains("\"feature\":\"icalendar-rfc5545-bounded-profile\""));
        assert!(json.contains("\"feature\":\"caldav-rfc4791-rfc5545-bounded-profile\""));
        assert!(json.contains("\"feature\":\"itip-rfc5546\""));
        assert!(json.contains("\"feature\":\"imip-rfc6047\""));
        assert!(json.contains("\"feature\":\"caldav-scheduling-rfc6638\""));
        assert!(json.contains("\"feature\":\"scheduling-itip-imip-freebusy\""));
        assert!(json.contains("\"feature\":\"non-gregorian-recurrence-rfc7529\""));
        assert!(json.contains("\"feature\":\"non-gregorian-recurrence\""));
        assert!(json.contains("\"feature\":\"timezone-reference-rfc7809\""));
        assert!(json.contains("\"feature\":\"availability-rfc7953\""));
        assert!(json.contains("\"feature\":\"timezone-reference-and-availability\""));
        assert!(json.contains("\"feature\":\"rfc7986-extra-properties\""));
        assert!(json.contains("\"feature\":\"carddav-rfc6352-bounded-access-profile\""));
        assert!(json.contains("\"feature\":\"vcard-rfc6350-bounded-profile\""));
        assert!(json.contains("\"feature\":\"carddav-rfc6352-rfc6350-bounded-profile\""));
        assert!(json.contains("\"feature\":\"xcard-rfc6351\""));
        assert!(json.contains("\"feature\":\"xcard\""));
        assert!(json.contains("\"feature\":\"place-death-extensions-rfc6474\""));
        assert!(json.contains("\"feature\":\"place-death-extensions\""));
        assert!(json.contains("\"feature\":\"parameter-caret-encoding-rfc6868\""));
        assert!(json.contains("\"feature\":\"parameter-caret-encoding\""));
        assert!(json.contains("\"feature\":\"vcard3-dialect-conversion\""));
        assert!(json.contains("\"feature\":\"setup-compatibility-listener\""));
        assert!(json.contains("\"feature\":\"real-submission-relay-delivery\""));
        assert!(json.contains("\"feature\":\"message-rfc5322-bounded-profile\""));
        assert!(json.contains("\"feature\":\"imap-rfc9051-bounded-profile\""));
        assert!(json.contains("\"feature\":\"jmap-core-rfc8620-bounded-profile\""));
        assert!(json.contains("\"feature\":\"jmap-mail-rfc8621-bounded-profile\""));
        assert!(json.contains("\"feature\":\"jmap-rfc8620-rfc8621-bounded-profile\""));
        assert!(json.contains("\"feature\":\"jmap-blob-rfc9404\""));
        assert!(json.contains("\"feature\":\"jmap-quotas-rfc9425\""));
        assert!(json.contains("\"feature\":\"jscontact-rfc9553\""));
        assert!(json.contains("\"feature\":\"vcard-jscontact-extensions-rfc9554\""));
        assert!(json.contains("\"feature\":\"jscontact-vcard-conversion-rfc9555\""));
        assert!(json.contains("\"feature\":\"jmap-contacts-rfc9610\""));
        assert!(json.contains("\"feature\":\"jscalendar-rfc8984\""));
        assert!(json.contains("\"feature\":\"jmap-calendars-draft-ietf-jmap-calendars\""));
        assert!(json.contains("\"feature\":\"jmap-sharing-rfc9670\""));
        assert!(json.contains("\"feature\":\"web-push-rfc8030\""));
        assert!(json.contains("\"feature\":\"vapid-rfc8292\""));
        assert!(json.contains("\"feature\":\"jmap-webpush-vapid-rfc9749\""));
        assert!(json.contains("\"feature\":\"smtp-rfc5321-setup-profile\""));
        assert!(json.contains("\"feature\":\"smtp-submission-rfc6409-setup-profile\""));
        assert!(json.contains("\"feature\":\"smtp-starttls-rfc3207-setup-profile\""));
        assert!(json.contains("\"feature\":\"email-tls-rfc8314-bounded-profile\""));
        assert!(json.contains("\"feature\":\"email-submission-ops-rfc5068\""));
        assert!(json.contains("\"feature\":\"smtp-auth-rfc4954-bounded-profile\""));
        assert!(json.contains("\"feature\":\"sasl-rfc4422-bounded-profile\""));
        assert!(json.contains("\"feature\":\"sasl-plain-rfc4616-bounded-profile\""));
        assert!(json.contains("\"feature\":\"mailto-rfc6068\""));
        assert!(json.contains("\"feature\":\"smtp-size-rfc1870-bounded-profile\""));
        assert!(json.contains("\"feature\":\"smtp-pipelining-rfc2920\""));
        assert!(json.contains("\"feature\":\"enhanced-status-codes-rfc3463\""));
        assert!(json.contains("\"feature\":\"enhanced-status-registry-rfc5248\""));
        assert!(json.contains("\"feature\":\"smtp-8bitmime-rfc6152-bounded-profile\""));
        assert!(json.contains("\"feature\":\"smtputf8-rfc6531\""));
        assert!(json.contains("\"feature\":\"internationalized-headers-rfc6532\""));
        assert!(json.contains("\"feature\":\"mime-format-rfc2045\""));
        assert!(json.contains("\"feature\":\"mime-media-types-rfc2046\""));
        assert!(json.contains("\"feature\":\"mime-encoded-words-rfc2047\""));
        assert!(json.contains("\"feature\":\"mime-conformance-rfc2049\""));
        assert!(json.contains("\"feature\":\"smtp-setup-auth-session-profile\""));
        assert!(json.contains("\"feature\":\"smtp-starttls-standard-port-transcript\""));
        assert!(json.contains("\"feature\":\"smtp-optional-extensions-mixed-profile\""));
        assert!(json.contains("\"name\":\"pim-owner-only-enterprise-v1\""));
        assert!(json.contains("\"admin_profile_key\":\"admin.certification.profile\""));
        assert!(json.contains("\"name\":\"pim-owner-only-redacted-transcripts-v1\""));
        assert!(json.contains("\"client\":\"Apple Calendar\""));
        assert!(json.contains("\"client\":\"DAVx5\""));
        assert!(json.contains("\"client\":\"Apple Mail\""));
        assert!(json.contains("\"name\":\"jmap-rfc8620-rfc8621-owner-only\""));
        assert!(json.contains("\"status\":\"passed\""));
        assert!(!json.contains("\"status\":\"failed\""));
    }

    #[test]
    fn report_runtime_profile_matches_core_runtime() {
        let report = report_memory_store_manifest();
        let profile = runtime_profile();

        assert_eq!(
            report.runtime_profile.binary_channel,
            profile.binary_channel
        );
        assert_eq!(
            report.runtime_profile.runtime_policy,
            profile.runtime_policy
        );
        assert_eq!(
            report.runtime_profile.default_identity_profile,
            profile.default_identity_profile.as_str()
        );
        assert_eq!(
            report.runtime_profile.crypto_provider,
            profile.crypto_provider
        );
        assert_eq!(report.runtime_profile.tls_provider, profile.tls_provider);
        assert_eq!(report.runtime_profile.fips_capable, profile.fips_capable);
        assert_eq!(
            report.runtime_profile.fips_tls_claim,
            profile.fips_tls_claim
        );

        let json = report.to_json();
        assert!(json.contains("\"runtime_profile\":{\"binary_channel\":"));
        assert!(json.contains("\"tls_provider\":\"none\""));
        assert!(json.contains("\"fips_tls_claim\":false"));
    }

    #[test]
    fn source_revision_resolves_only_real_values() {
        // Absent or blank injection stays None; a real revision is trimmed and kept.
        assert_eq!(resolve_source_revision(None), None);
        assert_eq!(resolve_source_revision(Some("")), None);
        assert_eq!(resolve_source_revision(Some("   ")), None);
        assert_eq!(resolve_source_revision(Some("\t\n")), None);
        assert_eq!(
            resolve_source_revision(Some("  v1.2.3  ")),
            Some("v1.2.3".to_string())
        );
        assert_eq!(
            resolve_source_revision(Some("abc1234")),
            Some("abc1234".to_string())
        );
    }

    #[test]
    fn report_source_revision_matches_compile_time_env() {
        // Whatever the build injected (or did not), the report must agree with the resolver applied to
        // the same compile-time variable, never a fabricated value.
        let report = report_memory_store_manifest();
        assert_eq!(
            report.source_revision,
            resolve_source_revision(option_env!("LOOM_SOURCE_REVISION"))
        );
        // The JSON form renders null when unset, otherwise a quoted string under the same key.
        let json = report.to_json();
        match &report.source_revision {
            None => assert!(json.contains("\"source_revision\":null")),
            Some(rev) => {
                let mut expected = String::from("\"source_revision\":");
                push_json_str(&mut expected, rev);
                assert!(json.contains(&expected));
            }
        }
    }
}
