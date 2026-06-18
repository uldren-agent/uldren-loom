use std::path::{Path, PathBuf};
use std::sync::Arc;

use loom_core::ExternalCredentialKind;
use loom_core::error::{LoomError, Result};
use loom_core::keys::KeySpec;
use loom_core::{Loom, WorkspaceId};
use loom_store::{FileStore, VerifiedExternalCredential};

#[cfg(all(feature = "fips", feature = "external-auth-standard"))]
compile_error!("FIPS hosted builds must not enable the standard external-auth provider feature");

pub mod admin;
pub mod archive;
pub mod chat;
pub mod data;
pub mod drive;
#[cfg(feature = "external-auth")]
mod external_auth;
pub mod grpc;
#[cfg(feature = "http")]
pub mod imap;
#[cfg(feature = "http")]
pub mod jmap;
pub mod jsonrpc;
#[cfg(feature = "kafka-wire")]
pub mod kafka_wire;
pub mod lanes;
pub mod meetings;
#[cfg(feature = "http")]
pub mod memcached;
#[cfg(feature = "mysql-wire")]
pub mod mysql_wire;
#[cfg(feature = "neo4j-wire")]
pub mod neo4j_wire;
#[cfg(feature = "pg-wire")]
pub mod pg_wire;
#[cfg(feature = "http")]
pub mod pim;
#[cfg(feature = "http")]
pub mod redis;
pub mod reference;
pub mod rest;
pub mod result_handles;
#[cfg(feature = "http")]
pub mod serve;
#[cfg(feature = "http")]
pub mod smtp;
pub mod sql;
pub mod substrate_changes;
pub mod tickets;
#[cfg(any(feature = "http", feature = "grpc"))]
pub mod vector_compat;
pub mod watch;
pub use chat::{
    HostedChatChannel, HostedChatCursor, HostedChatMessage, HostedChatPresence,
    HostedChatPresenceStore, HostedChatReaction, HostedChatThread, HostedChatWrite,
};
pub use data::{
    DocumentEntry, HostedDataAdapter, HostedDocumentIndex, HostedEtcdCompactResult,
    HostedEtcdCompare, HostedEtcdCompareResult, HostedEtcdCompareTarget,
    HostedEtcdDeleteRangeResult, HostedEtcdEventKind, HostedEtcdKv, HostedEtcdLeaseGrantResult,
    HostedEtcdLeaseKeepAliveResult, HostedEtcdPutResult, HostedEtcdRangeResult,
    HostedEtcdRequestOp, HostedEtcdResponseOp, HostedEtcdTxnResult, HostedEtcdWatchEvent,
    HostedEtcdWatchResult, HostedVectorEntry, HostedVectorInfo, KvEntry, QueueEntry,
    StructuredTimeSeriesPoint, TimeSeriesPoint,
};
pub use drive::{
    HostedDriveConflict, HostedDriveConflictResolution, HostedDriveCreateUpload, HostedDriveEntry,
    HostedDriveFolder, HostedDriveStat, HostedDriveUploadSession, HostedDriveVersion,
    HostedDriveWrite,
};
#[cfg(feature = "grpc")]
pub use grpc::service::{
    CasServer, ColumnarServer, DocumentServer, EtcdKvServer, EtcdLeaseServer, EtcdWatchServer,
    ExecServer, FilesServer, FtsServer, GraphServer, HostedCasGrpcService,
    HostedColumnarGrpcService, HostedDocumentGrpcService, HostedEtcdKvGrpcService,
    HostedEtcdLeaseGrpcService, HostedEtcdWatchGrpcService, HostedExecGrpcService,
    HostedFilesGrpcService, HostedFtsGrpcService, HostedGraphGrpcService, HostedKvGrpcService,
    HostedQdrantCollectionsGrpcService, HostedQdrantPointsGrpcService, HostedQueueGrpcService,
    HostedSqlGrpcService, HostedTimeSeriesGrpcService, HostedVcsGrpcService,
    HostedWatchGrpcService, KvServer, QdrantCollectionsServer, QdrantPointsServer, QueueServer,
    SqlServer, TimeSeriesServer, VcsServer, WatchServer, grpc_network_access_allows_request,
    serve_cas_grpc, serve_columnar_grpc, serve_document_grpc, serve_etcd_grpc, serve_exec_grpc,
    serve_files_grpc, serve_fts_grpc, serve_graph_grpc, serve_kv_grpc, serve_ledger_grpc,
    serve_qdrant_grpc, serve_queue_grpc, serve_sql_grpc, serve_time_series_grpc, serve_vcs_grpc,
    serve_watch_grpc,
};
pub use grpc::{GrpcAdapter, GrpcFailure, GrpcResponse, GrpcStatusCode};
#[cfg(feature = "http")]
pub use imap::serve_mail_imap;
#[cfg(all(feature = "http", feature = "tls"))]
pub use imap::serve_mail_imap_tls;
#[cfg(feature = "http")]
pub use jmap::{
    mail_jmap_router, mail_jmap_router_with_limit, mail_jmap_router_with_policy,
    serve_mail_jmap_with_limits,
};
pub use jsonrpc::{JsonRpcAdapter, JsonRpcError, JsonRpcErrorData, JsonRpcResponse};
#[cfg(feature = "kafka-wire")]
pub use kafka_wire::serve_kafka_tcp;
#[cfg(feature = "tls")]
pub use loom_hosted_core::HostedTlsConfig;
#[cfg(any(feature = "http", feature = "grpc"))]
pub use loom_hosted_core::network_access::{
    HostedNetworkAccessAuditEvent, HostedNetworkAccessAuditSink, HostedNetworkAccessCounter,
    HostedNetworkAccessMetrics, HostedNetworkAccessPolicy, HostedPeerCertificate,
    current_hosted_network_access_policy, network_access_allows,
    network_access_allows_with_denied_audit, network_access_metrics,
    with_hosted_network_access_policy, with_hosted_network_access_policy_for_listener,
    with_hosted_network_access_policy_for_listener_and_audit,
};
pub use loom_hosted_core::{
    HostedAuth, HostedAuthPolicy, HostedError, HostedHttpLimits, HostedOutcome,
    HostedRuntimeProfile, HostedWriteGuard, hosted_outcome, hosted_runtime_profile,
    validate_hosted_store_profile,
};
pub use loom_lifecycle::{
    LifecycleDefinitionSummary, LifecycleGateEvaluationInput, LifecycleInstanceSummary,
    LifecycleOperationLogSummary, LifecycleSnapshotPlanSummary, LifecycleSnapshotRecordSummary,
    LifecycleStageSurfaceSummary, LifecycleTransitionRequest, LifecycleTransitionResult,
    StandardLifecycleRequest,
};
pub use loom_pages::{
    PageCreateRequest, PageHistoryEntry, PagePublishSummary, PageSummary, PageUpdateSummary,
    SpaceSummary, StructureBindRequest, StructureCreateRequest, StructureEdgeSummary,
    StructureLinkRequest, StructureMoveRequest, StructureMoveSummary, StructureNodeRequest,
    StructureNodeSummary, StructureRenderSummary,
};
pub use loom_tickets::{TicketHistoryRecord, TicketProjectSummary, TicketSummary};
pub use meetings::{
    HostedMeetingDetail, HostedMeetingSummary, HostedMeetingsAnnotationReview,
    HostedMeetingsEntityMergeWrite, HostedMeetingsExtractionReview, HostedMeetingsList,
    HostedMeetingsProjection, HostedMeetingsProjectionApply, HostedMeetingsProjectionOutput,
    HostedMeetingsProjectionSkip, HostedMeetingsSearch, HostedMeetingsSearchHit,
    HostedMeetingsVocabularyReview,
};
#[cfg(feature = "http")]
pub use memcached::{MemcachedCacheMode, serve_memcached_text, serve_memcached_text_backed};
#[cfg(feature = "mysql-wire")]
pub use mysql_wire::serve_sql_mysql_wire;
#[cfg(all(feature = "mysql-wire", feature = "tls"))]
pub use mysql_wire::serve_sql_mysql_wire_with_tls;
#[cfg(feature = "neo4j-wire")]
pub use neo4j_wire::serve_neo4j_tcp;
#[cfg(feature = "pg-wire")]
pub use pg_wire::serve_sql_pg_wire;
#[cfg(all(feature = "pg-wire", feature = "tls"))]
pub use pg_wire::serve_sql_pg_wire_with_tls;
#[cfg(feature = "http")]
pub use pim::HostedPimAdapter;
#[cfg(feature = "http")]
pub use redis::serve_redis_resp;
pub use reference::HostedReferenceReconciliationStatus;
pub use rest::{RestAdapter, RestFailure, RestResponse, RestTreeEntry, RestTreeMetadata};
pub use result_handles::{ServedResultHandle, ServedResultHandles, ServedResultScope};
#[cfg(feature = "http")]
pub use serve::{
    HostedDataTarget, HostedDavWorkspaces, HostedServePolicy, HostedSqlTarget,
    admin_jsonrpc_router, admin_jsonrpc_router_with_limit, admin_jsonrpc_router_with_policy,
    admin_rest_router, admin_rest_router_with_limit, admin_rest_router_with_policy, caldav_router,
    caldav_router_with_limit, caldav_router_with_policy, carddav_router, carddav_router_with_limit,
    carddav_router_with_policy, cas_jsonrpc_router, cas_jsonrpc_router_with_limit,
    cas_jsonrpc_router_with_policy, cas_rest_router, cas_rest_router_with_limit,
    cas_rest_router_with_policy, data_jsonrpc_router, data_jsonrpc_router_with_limit,
    data_jsonrpc_router_with_policy, data_rest_router, data_rest_router_with_limit,
    data_rest_router_with_policy, exec_jsonrpc_router, exec_jsonrpc_router_with_limit,
    exec_jsonrpc_router_with_policy, exec_rest_router, exec_rest_router_with_limit,
    exec_rest_router_with_policy, files_jsonrpc_router, files_jsonrpc_router_with_limit,
    files_jsonrpc_router_with_policy, files_rest_router, files_rest_router_with_limit,
    files_rest_router_with_policy, grafana_http_router_with_policy, influx_http_router_with_policy,
    oci_rest_router, oci_rest_router_with_limit, oci_rest_router_with_policy,
    otlp_http_router_with_policy, prometheus_http_router_with_policy, s3_rest_router,
    s3_rest_router_with_limit, s3_rest_router_with_policy, serve_admin_jsonrpc,
    serve_admin_jsonrpc_with_limit, serve_admin_jsonrpc_with_limits,
    serve_admin_jsonrpc_with_policy, serve_admin_rest, serve_admin_rest_with_limit,
    serve_admin_rest_with_limits, serve_admin_rest_with_policy, serve_caldav_with_limits,
    serve_carddav_with_limits, serve_cas_jsonrpc, serve_cas_jsonrpc_with_limit,
    serve_cas_jsonrpc_with_limits, serve_cas_jsonrpc_with_policy, serve_cas_rest,
    serve_cas_rest_with_limit, serve_cas_rest_with_limits, serve_cas_rest_with_policy,
    serve_data_jsonrpc_with_limits, serve_data_jsonrpc_with_profile, serve_data_rest_with_limits,
    serve_data_rest_with_profile, serve_dav_with_limits, serve_exec_jsonrpc,
    serve_exec_jsonrpc_with_limit, serve_exec_jsonrpc_with_limits, serve_exec_jsonrpc_with_policy,
    serve_exec_rest, serve_exec_rest_with_limit, serve_exec_rest_with_limits,
    serve_exec_rest_with_policy, serve_files_jsonrpc, serve_files_jsonrpc_with_limit,
    serve_files_jsonrpc_with_limits, serve_files_jsonrpc_with_policy, serve_files_rest,
    serve_files_rest_with_limit, serve_files_rest_with_limits, serve_files_rest_with_policy,
    serve_grafana_http_with_limits, serve_influx_http_with_limits, serve_oci_rest,
    serve_oci_rest_with_limit, serve_oci_rest_with_limits, serve_oci_rest_with_policy,
    serve_otlp_http_with_limits, serve_prometheus_http_with_limits, serve_s3_rest,
    serve_s3_rest_with_limit, serve_s3_rest_with_limits, serve_s3_rest_with_policy,
    serve_sql_jsonrpc, serve_sql_jsonrpc_with_limit, serve_sql_jsonrpc_with_limits,
    serve_sql_jsonrpc_with_policy, serve_sql_rest, serve_sql_rest_with_limit,
    serve_sql_rest_with_limits, serve_sql_rest_with_policy, serve_vcs_jsonrpc,
    serve_vcs_jsonrpc_with_limit, serve_vcs_jsonrpc_with_limits, serve_vcs_jsonrpc_with_policy,
    serve_vcs_rest, serve_vcs_rest_with_limit, serve_vcs_rest_with_limits,
    serve_vcs_rest_with_policy, serve_watch_jsonrpc, serve_watch_jsonrpc_with_limit,
    serve_watch_jsonrpc_with_limits, serve_watch_jsonrpc_with_policy, serve_watch_rest,
    serve_watch_rest_with_limit, serve_watch_rest_with_limits, serve_watch_rest_with_policy,
    serve_web_rest_for_listener_with_limits, serve_web_rest_with_limits, vcs_jsonrpc_router,
    vcs_jsonrpc_router_with_limit, vcs_jsonrpc_router_with_policy, vcs_rest_router,
    vcs_rest_router_with_limit, vcs_rest_router_with_policy, watch_jsonrpc_router,
    watch_jsonrpc_router_with_limit, watch_jsonrpc_router_with_policy, watch_rest_router,
    watch_rest_router_with_limit, watch_rest_router_with_policy, web_rest_router,
    web_rest_router_for_listener, web_rest_router_for_listener_with_limit,
    web_rest_router_for_listener_with_policy, web_rest_router_with_limit,
    web_rest_router_with_policy,
};
#[cfg(all(feature = "http", feature = "tls"))]
pub use serve::{
    serve_admin_jsonrpc_tls_with_limit, serve_admin_jsonrpc_tls_with_limits,
    serve_admin_jsonrpc_tls_with_policy, serve_admin_rest_tls_with_limit,
    serve_admin_rest_tls_with_limits, serve_admin_rest_tls_with_policy,
    serve_caldav_tls_with_limits, serve_carddav_tls_with_limits, serve_cas_jsonrpc_tls_with_limit,
    serve_cas_jsonrpc_tls_with_limits, serve_cas_jsonrpc_tls_with_policy,
    serve_cas_rest_tls_with_limit, serve_cas_rest_tls_with_limits, serve_cas_rest_tls_with_policy,
    serve_data_jsonrpc_tls_with_limits, serve_data_jsonrpc_tls_with_profile,
    serve_data_rest_tls_with_limits, serve_data_rest_tls_with_profile, serve_dav_tls_with_limits,
    serve_exec_jsonrpc_tls_with_limits, serve_exec_rest_tls_with_limits,
    serve_files_jsonrpc_tls_with_limits, serve_files_rest_tls_with_limits,
    serve_mail_jmap_tls_with_limits, serve_sql_jsonrpc_tls_with_limits,
    serve_sql_rest_tls_with_limits, serve_vcs_jsonrpc_tls_with_limits,
    serve_vcs_rest_tls_with_limits, serve_watch_jsonrpc_tls_with_limits,
    serve_watch_rest_tls_with_limits, serve_web_rest_for_listener_tls_with_limits,
    serve_web_rest_tls_with_limits,
};
#[cfg(feature = "http")]
pub use smtp::serve_mail_smtp;
#[cfg(all(feature = "http", feature = "tls"))]
pub use smtp::{serve_mail_smtp_starttls, serve_mail_smtp_tls};
pub use sql::HostedSqlAdapter;
pub use substrate_changes::{HostedSubstrateChangeEvent, HostedSubstrateChangesBatch};
pub use tickets::{
    HostedTicketCommentAdd, HostedTicketCommentDelete, HostedTicketCommentUpdate,
    HostedTicketCreate, HostedTicketDelete, HostedTicketProjectSettings, HostedTicketProjectWrite,
    HostedTicketRelationRemove, HostedTicketRelationWrite, HostedTicketUpdate,
};
pub use watch::{
    HostedDataChange, HostedDomainChange, HostedUnsupportedDomain, HostedWatchBatch,
    HostedWatchMaterialization, HostedWatchMaterializeInput, HostedWatchStreamFrame,
    HostedWatchSubscribeInput, HostedWatchSubscription,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedExternalCredentialParts<'a> {
    pub kind: Option<String>,
    pub issuer: Option<String>,
    pub subject: Option<String>,
    pub material_digest: Option<String>,
    pub proof_kind: Option<String>,
    pub proof: Option<String>,
    pub challenge: Option<String>,
    pub peer_certificate_der: Option<&'a [u8]>,
}

pub fn hosted_external_credential_from_parts(
    parts: HostedExternalCredentialParts<'_>,
) -> Result<Option<VerifiedExternalCredential>> {
    let HostedExternalCredentialParts {
        kind,
        issuer,
        subject,
        material_digest,
        proof_kind,
        proof,
        challenge,
        peer_certificate_der: _peer_certificate_der,
    } = parts;
    let has_verified_assertion =
        kind.is_some() || issuer.is_some() || subject.is_some() || material_digest.is_some();
    let has_direct_proof = proof_kind.is_some() || proof.is_some() || challenge.is_some();
    if has_verified_assertion && has_direct_proof {
        return Err(LoomError::invalid(
            "supply either a verified external assertion or a direct external proof",
        ));
    }
    if has_direct_proof {
        let proof_kind = proof_kind.ok_or_else(|| {
            LoomError::invalid("direct external proof requires x-loom-external-proof-kind")
        })?;
        let kind = ExternalCredentialKind::parse(&proof_kind)?;
        let proof = proof.ok_or_else(|| {
            LoomError::invalid("direct external proof requires x-loom-external-proof")
        })?;
        #[cfg(feature = "external-auth")]
        {
            return external_auth::verify_direct_external_credential(
                kind,
                &proof,
                challenge.as_deref(),
                _peer_certificate_der,
            )
            .map(Some);
        }
        #[cfg(not(feature = "external-auth"))]
        {
            drop(proof);
            return Err(LoomError::new(
                loom_core::Code::Unsupported,
                format!(
                    "direct {} verifier is not source-backed for this hosted runtime",
                    kind.as_str()
                ),
            ));
        }
    }
    match (kind, issuer, subject) {
        (None, None, None) if material_digest.is_none() => Ok(None),
        (Some(kind), Some(issuer), Some(subject)) => {
            let kind = ExternalCredentialKind::parse(&kind)?;
            Ok(Some(VerifiedExternalCredential {
                kind,
                issuer,
                subject,
                material_digest,
                challenge_id: None,
            }))
        }
        _ => Err(LoomError::invalid(
            "verified external assertion requires kind, issuer, and subject",
        )),
    }
}

#[derive(Clone)]
pub struct HostedKernel {
    inner: loom_hosted_core::HostedKernel,
    chat_presence: Arc<HostedChatPresenceStore>,
    #[cfg(feature = "inference")]
    meetings_embedding_runtime: Option<meetings::HostedMeetingsEmbeddingRuntime>,
}

impl HostedKernel {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            inner: loom_hosted_core::HostedKernel::new(path),
            chat_presence: Arc::new(HostedChatPresenceStore::default()),
            #[cfg(feature = "inference")]
            meetings_embedding_runtime: None,
        }
    }

    pub fn with_unlock_key(mut self, unlock_key: KeySpec) -> Self {
        self.inner = self.inner.with_unlock_key(unlock_key);
        self
    }

    pub fn with_write_guard(mut self, write_guard: HostedWriteGuard) -> Self {
        self.inner = self.inner.with_write_guard(write_guard);
        self
    }

    pub fn path(&self) -> &Path {
        self.inner.path()
    }

    pub fn write_guard(&self) -> HostedWriteGuard {
        self.inner.write_guard()
    }

    pub fn as_core(&self) -> &loom_hosted_core::HostedKernel {
        &self.inner
    }

    pub fn into_core(self) -> loom_hosted_core::HostedKernel {
        self.inner
    }

    pub fn read<T>(
        &self,
        auth: &HostedAuth,
        f: impl FnOnce(&Loom<FileStore>) -> Result<T>,
    ) -> Result<T> {
        self.inner.read(auth, f)
    }

    pub fn read_mut<T>(
        &self,
        auth: &HostedAuth,
        f: impl FnOnce(&mut Loom<FileStore>) -> Result<T>,
    ) -> Result<T> {
        self.inner.read_mut(auth, f)
    }

    pub fn with_read_loom<T>(
        &self,
        auth: &HostedAuth,
        f: impl FnOnce(Loom<FileStore>) -> Result<T>,
    ) -> Result<T> {
        self.inner.with_read_loom(auth, f)
    }

    pub fn write<T>(
        &self,
        auth: &HostedAuth,
        f: impl FnOnce(&mut Loom<FileStore>) -> Result<T>,
    ) -> Result<T> {
        self.inner.write(auth, f)
    }

    pub fn audit_append(
        &self,
        principal: Option<WorkspaceId>,
        action: &str,
        target: Option<&str>,
    ) -> Result<u64> {
        self.inner.audit_append(principal, action, target)
    }

    pub fn audit_security_failure(&self, auth: &HostedAuth, err: &LoomError) {
        self.inner.audit_security_failure(auth, err);
    }

    pub fn chat_presence(&self) -> &HostedChatPresenceStore {
        &self.chat_presence
    }

    #[cfg(feature = "inference")]
    pub fn with_meetings_embedding_runtime(
        mut self,
        runtime: meetings::HostedMeetingsEmbeddingRuntime,
    ) -> Self {
        self.meetings_embedding_runtime = Some(runtime);
        self
    }

    #[cfg(feature = "inference")]
    pub fn meetings_embedding_runtime(&self) -> Option<&meetings::HostedMeetingsEmbeddingRuntime> {
        self.meetings_embedding_runtime.as_ref()
    }
}

impl From<loom_hosted_core::HostedKernel> for HostedKernel {
    fn from(inner: loom_hosted_core::HostedKernel) -> Self {
        Self {
            inner,
            chat_presence: Arc::new(HostedChatPresenceStore::default()),
            #[cfg(feature = "inference")]
            meetings_embedding_runtime: None,
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    #[cfg(feature = "integration-tests")]
    use loom_core::cas_put;
    use loom_core::digest::Algo;
    use loom_core::{
        AclRight, AclStore, AclSubject, Digest, FacetKind, IdentityStore, Loom, PrincipalKind,
        WorkspaceId,
    };
    use loom_store::{FileStore, save_loom};
    #[cfg(feature = "integration-tests")]
    use loom_substrate::annotation::{EMOJI_REGISTRY_DIR, EmojiRegistry, emoji_registry_path};
    #[cfg(feature = "integration-tests")]
    use loom_substrate::chat::{ChatChannelDirectory, chat_channel_directory_key};
    #[cfg(feature = "integration-tests")]
    use loom_substrate::drive::{
        DriveContentRef, DriveFileVersion, DriveFileVersionIndex, DriveFolderChildren,
        DriveFolderEntry, DriveFolderIndex, DriveNodeKind, DriveProfileSnapshot, drive_profile_key,
    };
    #[cfg(feature = "integration-tests")]
    use loom_substrate::meetings::{
        AnnotationRecord, Coverage as MeetingsCoverage, ImportRunRecord, InputProfile,
        MeetingRecord, MeetingRecordInput, MeetingsProfileSnapshot, MeetingsProfileSnapshotParts,
        RedactionRecord, RedactionState, SourceRecord, SourceRecordInput, SpanKind, SpanRecord,
        meetings_profile_key,
    };

    pub(crate) fn nid(byte: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([byte; 16])
    }

    pub(crate) fn temp_path(name: &str) -> PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!(
            "loom-hosted-{name}-{}-{}-{nonce}.loom",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_file(&path);
        path
    }

    pub(crate) fn init(path: &Path, user: Option<WorkspaceId>) -> WorkspaceId {
        with_init_loom(path, user, |_, ns| ns)
    }

    fn with_init_loom<T>(
        path: &Path,
        user: Option<WorkspaceId>,
        f: impl FnOnce(&mut Loom<FileStore>, WorkspaceId) -> T,
    ) -> T {
        loom_coordination::with_local_store_write_lock(path, || {
            let (mut loom, ns) = init_loom(path, user);
            let out = f(&mut loom, ns);
            save_loom(&mut loom)?;
            drop(loom);
            Ok(out)
        })
        .unwrap()
    }

    fn init_loom(path: &Path, user: Option<WorkspaceId>) -> (Loom<FileStore>, WorkspaceId) {
        let root = nid(1);
        let ns = nid(9);
        let algo = if cfg!(feature = "fips") {
            Algo::Sha256
        } else {
            Algo::Blake3
        };
        let fs = FileStore::create_with_profile(path, algo).unwrap();
        let mut identity = IdentityStore::new(root);
        identity
            .set_passphrase(root, "root-pass", b"12345678")
            .unwrap();
        if let Some(user) = user {
            identity
                .add_principal(user, "alice", PrincipalKind::User)
                .unwrap();
            identity
                .set_passphrase(user, "alice-pass", b"abcdefgh")
                .unwrap();
        }
        fs.save_identity_store(&identity).unwrap();
        let mut acl = AclStore::new();
        acl.allow(
            AclSubject::Principal(root),
            None,
            None,
            [
                AclRight::Admin,
                AclRight::Read,
                AclRight::Write,
                AclRight::Advance,
                AclRight::Merge,
                AclRight::Execute,
            ],
        )
        .unwrap();
        fs.save_acl_store(&acl).unwrap();
        let mut loom = Loom::new(fs);
        let ns = loom
            .registry_mut()
            .create(FacetKind::Files, Some("main"), ns)
            .unwrap();
        for facet in [
            FacetKind::Cas,
            FacetKind::Sql,
            FacetKind::Calendar,
            FacetKind::Contacts,
            FacetKind::Mail,
        ] {
            loom.registry_mut().add_facet(ns, facet).unwrap();
        }
        (loom, ns)
    }

    pub(crate) fn watch_history(path: &Path) -> (WorkspaceId, Digest, Digest) {
        with_init_loom(path, None, |loom, ns| {
            loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
            let c0 = loom.commit(ns, "watch", "c0", 1).unwrap();
            loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
            let c1 = loom.commit(ns, "watch", "c1", 2).unwrap();
            (ns, c0, c1)
        })
    }

    #[cfg(feature = "integration-tests")]
    pub(crate) fn chat_snapshot(path: &Path) -> (WorkspaceId, WorkspaceId) {
        with_init_loom(path, None, |loom, ns| {
            loom.registry_mut().add_facet(ns, FacetKind::Vcs).unwrap();
            let channel = nid(44);
            let mut directory = ChatChannelDirectory::new("studio").unwrap();
            directory
                .create_channel(channel, "general", "General")
                .unwrap();
            let path = String::from_utf8(chat_channel_directory_key("studio").unwrap()).unwrap();
            let parent = path.rsplit_once('/').unwrap().0;
            loom.create_directory_reserved(ns, parent, true).unwrap();
            loom.write_file_reserved(ns, &path, &directory.encode().unwrap(), 0o100644)
                .unwrap();
            let registry = EmojiRegistry::new(vec!["approved".to_string()]).unwrap();
            let emoji_path = emoji_registry_path("studio").unwrap();
            loom.create_directory_reserved(ns, EMOJI_REGISTRY_DIR, true)
                .unwrap();
            loom.write_file_reserved(ns, &emoji_path, &registry.encode().unwrap(), 0o100644)
                .unwrap();
            (ns, channel)
        })
    }

    #[cfg(feature = "integration-tests")]
    pub(crate) fn drive_snapshot(path: &Path) -> (WorkspaceId, Digest) {
        with_init_loom(path, None, |loom, ns| {
            loom.registry_mut().add_facet(ns, FacetKind::Vcs).unwrap();
            let digest = cas_put(loom, ns, b"hello drive").unwrap();
            let folders = DriveFolderIndex::new(
                "main",
                vec![
                    DriveFolderChildren::new(
                        "root",
                        vec![
                            DriveFolderEntry::new("Specs", "folder-1", DriveNodeKind::Folder)
                                .unwrap(),
                            DriveFolderEntry::new("Plan.txt", "file-1", DriveNodeKind::File)
                                .unwrap(),
                        ],
                    )
                    .unwrap(),
                ],
            )
            .unwrap();
            let versions = DriveFileVersionIndex::new(
                "main",
                vec![
                    DriveFileVersion::new(
                        "file-1",
                        1,
                        "op-1",
                        nid(1),
                        100,
                        DriveContentRef::Blob { digest, size: 11 },
                    )
                    .unwrap(),
                ],
            )
            .unwrap();
            let snapshot = DriveProfileSnapshot::new("main", folders, versions).unwrap();
            loom.store()
                .control_set(
                    &drive_profile_key("main").unwrap(),
                    snapshot.encode().unwrap(),
                )
                .unwrap();
            (ns, digest)
        })
    }

    #[cfg(feature = "integration-tests")]
    pub(crate) fn meetings_snapshot(path: &Path) -> WorkspaceId {
        with_init_loom(path, None, |loom, ns| {
            loom.registry_mut().add_facet(ns, FacetKind::Vcs).unwrap();
            let snapshot = sample_meetings_snapshot();
            loom.store()
                .control_set(
                    &meetings_profile_key(&snapshot.workspace_id).unwrap(),
                    snapshot.encode().unwrap(),
                )
                .unwrap();
            ns
        })
    }

    #[cfg(feature = "integration-tests")]
    fn sample_meetings_snapshot() -> MeetingsProfileSnapshot {
        let mut source = SourceRecord::new(SourceRecordInput {
            source_id: "src-1",
            source_system: "granola-api",
            external_id: "not_1",
            source_digest: Digest::hash(Algo::Blake3, b"meeting-source"),
            observed_at_ms: 100,
            access_scope: "personal-notes",
            coverage: MeetingsCoverage::Partial,
        })
        .unwrap();
        source.sidecar_digest = Some(Digest::hash(Algo::Blake3, b"meeting-sidecar"));

        let mut meeting = MeetingRecord::new(MeetingRecordInput {
            meeting_id: "meet-1",
            title: "Architecture review",
            current_source_digest: Digest::hash(Algo::Blake3, b"meeting-source"),
            created_at_ms: 100,
            updated_at_ms: 120,
        })
        .unwrap();
        meeting.source_refs = vec!["src-1".to_string()];
        meeting.attendee_refs = vec!["person:ava".to_string(), "person:nas".to_string()];

        let mut span = SpanRecord::new(
            "span-1",
            "meet-1",
            "src-1",
            SpanKind::TranscriptEntry,
            "granola:not_1/transcript/0",
        )
        .unwrap();
        span.text_digest = Some(Digest::hash(Algo::Blake3, b"meeting-text"));

        let mut annotation = AnnotationRecord::new(
            "ann-1",
            "meet-1",
            vec!["span-1".to_string()],
            "Decision",
            "Use normalized import snapshots",
            130,
        )
        .unwrap();
        annotation.accept("principal-1", 140).unwrap();

        let mut import_run = ImportRunRecord::new(
            "run-1",
            InputProfile::GranolaApi,
            "personal-notes",
            MeetingsCoverage::Partial,
            90,
        )
        .unwrap();
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
        )
        .unwrap();
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
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use loom_core::{
        AclEffect, AclGrant, AclPredicate, AclRight, AclScope, AclScopeKind, AclSubject, Algo,
        Code, ExternalCredentialKind, ExternalCredentialSpec, FacetKind,
    };
    use loom_store::open_loom_read_unlocked;

    use super::*;
    use crate::test_support::{init, nid, temp_path};

    #[test]
    fn unauthenticated_mode_uses_root_without_prompt() {
        let path = temp_path("unauthenticated");
        let ns_id = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::unauthenticated();
        let ns = kernel
            .read(&auth, |loom| {
                loom.registry()
                    .open(&loom_core::WsSelector::Name("main".into()))
            })
            .unwrap();
        assert_eq!(ns, ns_id);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn authenticated_mode_requires_a_presented_principal() {
        let path = temp_path("auth-required");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let err = kernel
            .read(&HostedAuth::unauthenticated(), |loom| {
                let ns = loom
                    .registry()
                    .open(&loom_core::WsSelector::Name("main".into()))?;
                loom.authorize(ns, FacetKind::Files, AclRight::Read)
            })
            .unwrap_err();
        assert_eq!(err.code, Code::AuthenticationFailed);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn passphrase_authentication_binds_the_request_session() {
        let path = temp_path("passphrase");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "request-1");
        let ns = kernel
            .read(&auth, |loom| {
                let ns = loom
                    .registry()
                    .open(&loom_core::WsSelector::Name("main".into()))?;
                loom.authorize(ns, FacetKind::Files, AclRight::Admin)?;
                Ok(ns)
            })
            .unwrap();
        assert_eq!(ns, nid(9));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn hosted_runtime_installs_acl_cel_predicate_evaluator() {
        let path = temp_path("hosted-acl-cel");
        init(&path, Some(nid(2)));
        loom_coordination::with_local_store_write_lock(&path, || {
            let fs = loom_store::FileStore::open(&path).unwrap();
            let mut acl = fs.acl_store().unwrap().unwrap();
            acl.grant(AclGrant {
                subject: AclSubject::Principal(nid(2)),
                workspace: Some(nid(9)),
                domain: Some(FacetKind::Files.into()),
                ref_glob: None,
                scopes: vec![AclScope::Prefix {
                    kind: AclScopeKind::Path,
                    prefix: b"public/".to_vec(),
                }],
                rights: std::collections::BTreeSet::from([AclRight::Read]),
                effect: AclEffect::Allow,
                predicate: Some(
                    AclPredicate::cel(
                        r#"principal == "02020202-0202-0202-0202-020202020202" &&
                       domain == "files" &&
                       right == "read" &&
                       scope_text == "public/report.txt""#,
                    )
                    .unwrap(),
                ),
            })
            .unwrap();
            fs.save_acl_store(&acl).unwrap();
            drop(fs);
            Ok(())
        })
        .unwrap();

        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::passphrase(nid(2), "alice-pass", "request-predicate");
        kernel
            .read(&auth, |loom| {
                loom.authorize_file_path(nid(9), "public/report.txt", AclRight::Read)
            })
            .unwrap();
        assert_eq!(
            kernel
                .read(&auth, |loom| {
                    loom.authorize_file_path(nid(9), "public/secret.txt", AclRight::Read)
                })
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn verified_external_authentication_binds_the_request_session() {
        let path = temp_path("external-auth");
        init(&path, None);
        loom_coordination::with_local_store_write_lock(&path, || {
            let fs = loom_store::FileStore::open(&path).unwrap();
            let mut identity = fs.identity_store().unwrap().unwrap();
            identity
                .create_external_credential(
                    nid(1),
                    ExternalCredentialSpec {
                        id: nid(22),
                        kind: ExternalCredentialKind::OidcSubject,
                        label: "okta-prod".to_string(),
                        issuer: "https://issuer.example".to_string(),
                        subject: "00u123".to_string(),
                        material_digest: Some("sha256:metadata".to_string()),
                    },
                )
                .unwrap();
            fs.save_identity_store(&identity).unwrap();
            drop(fs);
            Ok(())
        })
        .unwrap();

        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::verified_external(
            VerifiedExternalCredential {
                kind: ExternalCredentialKind::OidcSubject,
                issuer: "https://issuer.example".to_string(),
                subject: "00u123".to_string(),
                material_digest: Some("sha256:metadata".to_string()),
                challenge_id: None,
            },
            "external-1",
        );
        let principal = kernel
            .read(&auth, |loom| loom.effective_principal())
            .unwrap()
            .unwrap();
        assert_eq!(principal, nid(1));
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "external-auth")]
    #[test]
    fn public_key_direct_proof_binds_to_registered_material() {
        use base64::Engine as _;
        use ring::signature::{Ed25519KeyPair, KeyPair};

        let path = temp_path("external-public-key-proof");
        init(&path, None);
        let key_pair = Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
        let public_key = key_pair.public_key().as_ref();
        let material_digest = loom_core::Digest::hash(Algo::Sha256, public_key).to_string();
        let challenge = "request-challenge";
        let challenge_id = nid(24);
        loom_coordination::with_local_store_write_lock(&path, || {
            let fs = loom_store::FileStore::open(&path).unwrap();
            let mut identity = fs.identity_store().unwrap().unwrap();
            identity
                .create_external_credential(
                    nid(1),
                    ExternalCredentialSpec {
                        id: nid(23),
                        kind: ExternalCredentialKind::PublicKey,
                        label: "admin-key".to_string(),
                        issuer: "loom".to_string(),
                        subject: "key-1".to_string(),
                        material_digest: Some(material_digest.clone()),
                    },
                )
                .unwrap();
            identity
                .create_external_credential_challenge(
                    nid(23),
                    challenge_id,
                    challenge.as_bytes().to_vec(),
                    0,
                    u64::MAX,
                )
                .unwrap();
            fs.save_identity_store(&identity).unwrap();
            drop(fs);
            Ok(())
        })
        .unwrap();

        let signature = key_pair.sign(challenge.as_bytes());
        let proof = serde_json::json!({
            "issuer": "loom",
            "subject": "key-1",
            "material_digest": material_digest,
            "public_key": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public_key),
            "signature": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.as_ref()),
            "challenge_id": challenge_id.to_string(),
            "algorithm": "ed25519"
        })
        .to_string();
        let credential = hosted_external_credential_from_parts(HostedExternalCredentialParts {
            kind: None,
            issuer: None,
            subject: None,
            material_digest: None,
            proof_kind: Some("public-key".to_string()),
            proof: Some(proof),
            challenge: Some(challenge.to_string()),
            peer_certificate_der: None,
        })
        .unwrap()
        .unwrap();
        let kernel = HostedKernel::new(&path);
        let principal = kernel
            .read(
                &HostedAuth::verified_external(credential, "external-key"),
                |loom| loom.effective_principal(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(principal, nid(1));
        let fs = loom_store::FileStore::open_read(&path).unwrap();
        let identity = fs.identity_store().unwrap().unwrap();
        assert!(
            !identity
                .external_challenges()
                .any(|challenge| challenge.id == challenge_id)
        );
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "external-auth")]
    #[test]
    fn mtls_direct_proof_stays_fail_closed_without_peer_certificate_binding() {
        let err = hosted_external_credential_from_parts(HostedExternalCredentialParts {
            kind: None,
            issuer: None,
            subject: None,
            material_digest: None,
            proof_kind: Some("mtls-certificate".to_string()),
            proof: Some("{}".to_string()),
            challenge: Some("request-challenge".to_string()),
            peer_certificate_der: None,
        })
        .unwrap_err();
        assert_eq!(err.code, Code::Unsupported);
        assert!(
            err.message
                .contains("mTLS direct proof requires TLS peer certificate binding")
        );
    }

    #[cfg(feature = "external-auth")]
    #[test]
    fn mtls_direct_proof_binds_to_peer_certificate_material() {
        let rcgen::CertifiedKey { cert, .. } =
            rcgen::generate_simple_self_signed(vec!["client.example".to_string()]).unwrap();
        let leaf = cert.der().as_ref();
        let material_digest = loom_core::Digest::hash(Algo::Sha256, leaf).to_string();
        let proof = serde_json::json!({
            "issuer": "loom-ca",
            "subject": "client.example",
            "material_digest": material_digest
        })
        .to_string();
        let credential = hosted_external_credential_from_parts(HostedExternalCredentialParts {
            kind: None,
            issuer: None,
            subject: None,
            material_digest: None,
            proof_kind: Some("mtls-certificate".to_string()),
            proof: Some(proof),
            challenge: None,
            peer_certificate_der: Some(leaf),
        })
        .unwrap()
        .unwrap();
        assert_eq!(credential.kind, ExternalCredentialKind::MtlsCertificate);
        assert_eq!(credential.issuer, "loom-ca");
        assert_eq!(credential.subject, "client.example");
        assert_eq!(
            credential.material_digest.as_deref(),
            Some(material_digest.as_str())
        );

        let wrong_digest =
            loom_core::Digest::hash(Algo::Sha256, b"not the certificate").to_string();
        let bad_proof = serde_json::json!({
            "issuer": "loom-ca",
            "subject": "client.example",
            "material_digest": wrong_digest
        })
        .to_string();
        let err = hosted_external_credential_from_parts(HostedExternalCredentialParts {
            kind: None,
            issuer: None,
            subject: None,
            material_digest: None,
            proof_kind: Some("mtls-certificate".to_string()),
            proof: Some(bad_proof),
            challenge: None,
            peer_certificate_der: Some(leaf),
        })
        .unwrap_err();
        assert_eq!(err.code, Code::AuthenticationFailed);
    }

    #[cfg(all(feature = "external-auth", feature = "fips"))]
    #[test]
    fn fips_direct_proofs_reject_non_provider_backed_verifiers() {
        for kind in ["oidc-subject", "saml-subject", "passkey"] {
            let err = hosted_external_credential_from_parts(HostedExternalCredentialParts {
                kind: None,
                issuer: None,
                subject: None,
                material_digest: None,
                proof_kind: Some(kind.to_string()),
                proof: Some("{}".to_string()),
                challenge: Some("request-challenge".to_string()),
                peer_certificate_der: None,
            })
            .unwrap_err();
            assert_eq!(err.code, Code::Unsupported);
            assert!(
                err.message.contains("requires a provider-backed verifier"),
                "{kind}: {}",
                err.message
            );
        }
    }

    #[test]
    fn bad_passphrase_fails_before_the_operation_runs() {
        let path = temp_path("bad-passphrase");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::passphrase(nid(1), "wrong", "request-1");
        let err = kernel
            .read(&auth, |_| -> Result<()> {
                panic!("operation must not run after authentication failure");
            })
            .unwrap_err();
        assert_eq!(err.code, Code::AuthenticationFailed);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn write_success_is_persisted() {
        let path = temp_path("write");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "request-1");
        kernel
            .write(&auth, |loom| {
                let ns = loom
                    .registry()
                    .open(&loom_core::WsSelector::Name("main".into()))?;
                loom.write_file(ns, "hosted.txt", b"hosted", 0o100644)?;
                Ok(())
            })
            .unwrap();
        let reopened = open_loom_read_unlocked(&path, None).unwrap();
        let ns = reopened
            .registry()
            .open(&loom_core::WsSelector::Name("main".into()))
            .unwrap();
        assert_eq!(
            reopened.read_file(ns, "hosted.txt").unwrap(),
            b"hosted".to_vec()
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn hosted_error_preserves_stable_code_shape() {
        let err = hosted_outcome::<()>(Err(LoomError::new(
            Code::PermissionDenied,
            "acl default deny",
        )))
        .unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);
        assert_eq!(err.code_name, "PERMISSION_DENIED");
        assert_eq!(err.code_number, Code::PermissionDenied.as_i32());
        assert_eq!(err.message, "acl default deny");
    }

    #[test]
    fn hosted_protocol_auth_acl_conformance_matrix() {
        let path = temp_path("protocol-auth-matrix");
        let user = nid(7);
        let ns = init(&path, Some(user));
        let kernel = HostedKernel::new(&path);
        let root = HostedAuth::passphrase(nid(1), "root-pass", "matrix-root");
        let bad_root = HostedAuth::passphrase(nid(1), "bad-pass", "matrix-bad");
        let user = HostedAuth::passphrase(user, "alice-pass", "matrix-user");

        kernel
            .rest()
            .put_tree(&root, ns, "secret.txt", b"secret")
            .unwrap();

        let rest_auth = kernel
            .rest()
            .get_tree(&bad_root, ns, "secret.txt")
            .unwrap_err();
        assert_eq!(rest_auth.status, 401);
        assert_eq!(rest_auth.error.code, Code::AuthenticationFailed);
        let rest_denied = kernel.rest().get_tree(&user, ns, "secret.txt").unwrap_err();
        assert_eq!(rest_denied.status, 403);
        assert_eq!(rest_denied.error.code, Code::PermissionDenied);

        let json_auth = kernel
            .jsonrpc()
            .fs_read_file(&bad_root, ns, "secret.txt")
            .unwrap_err();
        assert_eq!(json_auth.data.loom_code, "AUTHENTICATION_FAILED");
        let json_denied = kernel
            .jsonrpc()
            .fs_read_file(&user, ns, "secret.txt")
            .unwrap_err();
        assert_eq!(json_denied.data.loom_code, "PERMISSION_DENIED");

        let grpc_auth = kernel
            .grpc()
            .read_file(&bad_root, ns, "secret.txt")
            .unwrap_err();
        assert_eq!(grpc_auth.status, GrpcStatusCode::Unauthenticated);
        let grpc_denied = kernel
            .grpc()
            .read_file(&user, ns, "secret.txt")
            .unwrap_err();
        assert_eq!(grpc_denied.status, GrpcStatusCode::PermissionDenied);

        let sql_auth = kernel
            .sql()
            .query_cbor(&bad_root, "main", "db", "SELECT 1")
            .unwrap_err();
        assert_eq!(sql_auth.code, Code::AuthenticationFailed);

        #[cfg(feature = "http")]
        {
            let pim_denied = kernel
                .pim()
                .calendar_get_entry(&user, ns, "alice", "work", "missing")
                .unwrap_err();
            assert_eq!(pim_denied.code, Code::PermissionDenied);
        }

        let records = loom_store::FileStore::open_read(&path)
            .unwrap()
            .audit_records()
            .unwrap();
        assert!(
            records
                .iter()
                .any(|record| record.action == "hosted.auth.failed")
        );
        assert!(
            records
                .iter()
                .any(|record| record.action == "hosted.auth.denied")
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn daemon_guard_mode_is_part_of_the_kernel_contract() {
        let kernel = HostedKernel::new("/tmp/loom-hosted-noop.loom")
            .with_write_guard(HostedWriteGuard::DaemonAuthorized);
        assert_eq!(kernel.write_guard(), HostedWriteGuard::DaemonAuthorized);
    }

    #[test]
    fn hosted_runtime_profile_reports_build_policy() {
        let profile = hosted_runtime_profile();
        if cfg!(feature = "fips") {
            assert_eq!(profile.binary_channel, "fips");
            assert_eq!(profile.runtime_policy, "strict");
            assert_eq!(profile.default_identity_profile, Algo::Sha256);
            assert!(profile.fips_capable);
            assert!(profile.fips_tls_claim);
            assert!(validate_hosted_store_profile(Algo::Sha256, true).is_ok());
            assert!(validate_hosted_store_profile(Algo::Sha256, false).is_ok());
            let err = validate_hosted_store_profile(Algo::Blake3, false).unwrap_err();
            assert_eq!(err.code, Code::PermissionDenied);
        } else {
            assert_eq!(profile.binary_channel, "standard");
            assert_eq!(profile.runtime_policy, "capable");
            assert_eq!(profile.default_identity_profile, Algo::Blake3);
            assert!(!profile.fips_capable);
            assert!(!profile.fips_tls_claim);
            assert!(validate_hosted_store_profile(Algo::Blake3, false).is_ok());
            let err = validate_hosted_store_profile(Algo::Sha256, false).unwrap_err();
            assert_eq!(err.code, Code::PermissionDenied);
            assert_eq!(
                err.message,
                "FIPS-profile stores cannot be served by the current non-FIPS hosted runtime"
            );
            let err = validate_hosted_store_profile(Algo::Blake3, true).unwrap_err();
            assert_eq!(err.code, Code::PermissionDenied);
        }
    }
}
pub use admin::HostedAdminAdapter;
