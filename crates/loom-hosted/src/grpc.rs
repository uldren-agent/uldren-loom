use loom_core::error::Code;
use loom_core::{Digest, WorkspaceId};

use crate::watch::{HostedWatchBatch, HostedWatchSubscribeInput, HostedWatchSubscription};
use crate::{HostedAuth, HostedError, HostedKernel};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpcResponse<T> {
    pub message: T,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpcFailure {
    pub status: GrpcStatusCode,
    pub error: HostedError,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrpcStatusCode {
    InvalidArgument,
    Unauthenticated,
    PermissionDenied,
    NotFound,
    AlreadyExists,
    FailedPrecondition,
    Aborted,
    Unimplemented,
    OutOfRange,
    Internal,
    Unavailable,
}

pub type GrpcResult<T> = std::result::Result<GrpcResponse<T>, GrpcFailure>;

pub struct GrpcAdapter<'a> {
    kernel: &'a HostedKernel,
}

impl HostedKernel {
    pub fn grpc(&self) -> GrpcAdapter<'_> {
        GrpcAdapter { kernel: self }
    }
}

impl GrpcAdapter<'_> {
    pub fn put_cas(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        bytes: &[u8],
    ) -> GrpcResult<String> {
        grpc_result(self.kernel.write(auth, |loom| {
            loom_core::cas_put(loom, workspace, bytes).map(|digest| digest.to_string())
        }))
    }

    pub fn get_cas(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        digest: &Digest,
    ) -> GrpcResult<Option<Vec<u8>>> {
        grpc_result(
            self.kernel
                .read(auth, |loom| loom_core::cas_get(loom, workspace, digest)),
        )
    }

    pub fn has_cas(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        digest: &Digest,
    ) -> GrpcResult<bool> {
        grpc_result(
            self.kernel
                .read(auth, |loom| loom_core::cas_has(loom, workspace, digest)),
        )
    }

    pub fn list_cas(&self, auth: &HostedAuth, workspace: WorkspaceId) -> GrpcResult<Vec<String>> {
        grpc_result(self.kernel.read(auth, |loom| {
            loom_core::cas_list(loom, workspace).map(|digests| {
                digests
                    .into_iter()
                    .map(|digest| digest.to_string())
                    .collect()
            })
        }))
    }

    pub fn delete_cas(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        digest: &Digest,
    ) -> GrpcResult<bool> {
        grpc_result(
            self.kernel
                .write(auth, |loom| loom_core::cas_delete(loom, workspace, digest)),
        )
    }

    pub fn read_file(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        path: &str,
    ) -> GrpcResult<Vec<u8>> {
        grpc_result(
            self.kernel
                .read(auth, |loom| loom.read_file(workspace, path)),
        )
    }

    pub fn write_file(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        path: &str,
        bytes: &[u8],
    ) -> GrpcResult<()> {
        grpc_result(self.kernel.write(auth, |loom| {
            loom.write_file(workspace, path, bytes, 0o100644)
        }))
    }

    pub fn stat_file(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        path: &str,
    ) -> GrpcResult<loom_core::Stat> {
        grpc_result(self.kernel.read(auth, |loom| loom.stat(workspace, path)))
    }

    pub fn list_directory(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        path: &str,
    ) -> GrpcResult<Vec<loom_core::DirEntry>> {
        grpc_result(
            self.kernel
                .read(auth, |loom| loom.list_directory(workspace, path)),
        )
    }

    pub fn create_directory(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        path: &str,
        recursive: bool,
    ) -> GrpcResult<()> {
        grpc_result(self.kernel.write(auth, |loom| {
            loom.create_directory(workspace, path, recursive)
        }))
    }

    pub fn delete_path(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        path: &str,
        recursive: bool,
    ) -> GrpcResult<()> {
        grpc_result(self.kernel.write(auth, |loom| {
            if loom.stat(workspace, path)?.kind == loom_core::FileKind::Directory {
                loom.remove_directory(workspace, path, recursive)
            } else {
                loom.remove_file(workspace, path)
            }
        }))
    }

    pub fn vcs_commit(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        author: &str,
        message: &str,
        timestamp_ms: u64,
        staged: bool,
    ) -> GrpcResult<String> {
        grpc_result(self.kernel.write(auth, |loom| {
            let commit = if staged {
                loom.commit_staged(workspace, author, message, timestamp_ms)?
            } else {
                loom.commit(workspace, author, message, timestamp_ms)?
            };
            Ok(commit.to_string())
        }))
    }

    pub fn vcs_log(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        requested_ref: Option<&str>,
        limit: Option<usize>,
    ) -> GrpcResult<Vec<String>> {
        grpc_result(self.kernel.read(auth, |loom| {
            let branch = match requested_ref {
                Some(value) => value.to_string(),
                None => loom.registry().head_branch(workspace)?,
            };
            let mut commits = loom.log(workspace, &branch)?;
            if let Some(limit) = limit {
                commits.truncate(limit);
            }
            Ok(commits
                .into_iter()
                .map(|commit| commit.to_string())
                .collect())
        }))
    }

    pub fn vcs_branch(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        name: &str,
    ) -> GrpcResult<()> {
        grpc_result(self.kernel.write(auth, |loom| loom.branch(workspace, name)))
    }

    pub fn vcs_checkout(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        target: &str,
    ) -> GrpcResult<()> {
        grpc_result(self.kernel.write(auth, |loom| {
            if loom.registry().branch_tip(workspace, target)?.is_some() {
                loom.checkout_branch(workspace, target)
            } else {
                let commit = loom.resolve_rev(workspace, target)?;
                loom.checkout_commit(workspace, commit)
            }
        }))
    }

    pub fn vcs_diff(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        from: &str,
        to: &str,
    ) -> GrpcResult<Vec<u8>> {
        grpc_result(self.kernel.read(auth, |loom| {
            let from = loom.resolve_rev(workspace, from)?;
            let to = loom.resolve_rev(workspace, to)?;
            loom.diff_commits(workspace, from, to)
        }))
    }

    pub fn vcs_merge(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        source: &str,
        author: &str,
        timestamp_ms: u64,
        cells: bool,
    ) -> GrpcResult<loom_core::MergeOutcome> {
        grpc_result(self.kernel.write(auth, |loom| {
            if cells {
                loom.merge_cell_level(workspace, source, author, timestamp_ms)
            } else {
                loom.merge(workspace, source, author, timestamp_ms)
            }
        }))
    }

    pub fn vcs_status(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
    ) -> GrpcResult<loom_core::Status> {
        grpc_result(self.kernel.read(auth, |loom| loom.status(workspace)))
    }

    pub fn vcs_stage(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        paths: &[String],
    ) -> GrpcResult<()> {
        let refs = paths.iter().map(String::as_str).collect::<Vec<_>>();
        grpc_result(self.kernel.write(auth, |loom| loom.stage(workspace, &refs)))
    }

    pub fn vcs_stage_all(&self, auth: &HostedAuth, workspace: WorkspaceId) -> GrpcResult<()> {
        grpc_result(self.kernel.write(auth, |loom| loom.stage_all(workspace)))
    }

    pub fn vcs_unstage(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        paths: &[String],
    ) -> GrpcResult<()> {
        let refs = paths.iter().map(String::as_str).collect::<Vec<_>>();
        grpc_result(
            self.kernel
                .write(auth, |loom| loom.unstage(workspace, &refs)),
        )
    }

    pub fn exec_cbor(&self, auth: &HostedAuth, request: &[u8]) -> GrpcResult<Vec<u8>> {
        grpc_result(self.kernel.write(auth, |loom| {
            loom_compute::execute_cbor(loom, request)
                .map_err(|err| loom_core::LoomError::new(err.code(), err.to_string()))
        }))
    }

    pub fn watch_subscribe(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        input: &HostedWatchSubscribeInput,
    ) -> GrpcResult<HostedWatchSubscription> {
        grpc_result(self.kernel.read(auth, |loom| {
            crate::watch::watch_subscribe(loom, workspace, input)
        }))
    }

    pub fn watch_poll(
        &self,
        auth: &HostedAuth,
        workspace: WorkspaceId,
        cursor: &str,
        max: u32,
    ) -> GrpcResult<HostedWatchBatch> {
        grpc_result(self.kernel.read(auth, |loom| {
            crate::watch::watch_poll(loom, workspace, cursor, max)
        }))
    }
}

fn grpc_result<T>(result: loom_core::Result<T>) -> GrpcResult<T> {
    result.map_or_else(
        |err| {
            let error = HostedError::from_error(err);
            Err(GrpcFailure {
                status: grpc_status(error.code),
                error,
            })
        },
        |message| Ok(GrpcResponse { message }),
    )
}

pub fn grpc_status(code: Code) -> GrpcStatusCode {
    match code {
        Code::InvalidArgument => GrpcStatusCode::InvalidArgument,
        Code::AuthenticationFailed | Code::E2eKeyInvalid => GrpcStatusCode::Unauthenticated,
        Code::PermissionDenied => GrpcStatusCode::PermissionDenied,
        Code::NotFound => GrpcStatusCode::NotFound,
        Code::AlreadyExists => GrpcStatusCode::AlreadyExists,
        Code::CasMismatch
        | Code::FencingStale
        | Code::LockLeaseExpired
        | Code::E2eLocked
        | Code::DocumentNotText
        | Code::DimensionMismatch => GrpcStatusCode::FailedPrecondition,
        Code::Conflict | Code::Locked | Code::LockNotHeld | Code::NotFastForward => {
            GrpcStatusCode::Aborted
        }
        Code::Unsupported => GrpcStatusCode::Unimplemented,
        Code::RetainedGap => GrpcStatusCode::OutOfRange,
        Code::Io | Code::Unavailable => GrpcStatusCode::Unavailable,
        _ => GrpcStatusCode::Internal,
    }
}

#[cfg(feature = "grpc")]
pub mod service {
    use std::collections::BTreeMap;
    use std::convert::Infallible;
    use std::net::SocketAddr;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context, Poll};
    use std::time::Duration;

    use loom_core::Value as CellValue;
    use loom_core::{
        Digest, FieldMapping, FieldType, FieldValue, LedgerRangeEntry, LedgerRangeState, Query,
        QueryRequest, QueryResponse, TimeSeriesAggregation, TimeSeriesValue, WorkspaceId,
    };
    use serde_json::{Value, json};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tokio::net::TcpListener;
    use tokio::sync::mpsc;
    use tokio_stream::{self, Stream, StreamExt as _};
    use tonic::body::Body;
    use tonic::codegen::{BoxFuture, Service, http};
    use tonic::server::{NamedService, ServerStreamingService, StreamingService, UnaryService};
    use tonic::transport::server::Connected;
    use tonic::{Request, Response, Status};

    use crate::grpc::{GrpcFailure, GrpcStatusCode};
    use crate::{
        DocumentEntry, HostedAuth, HostedDocumentIndex, HostedEtcdCompare as DataEtcdCompare,
        HostedEtcdCompareResult, HostedEtcdCompareTarget, HostedEtcdRequestOp,
        HostedEtcdResponseOp, HostedExternalCredentialParts, HostedKernel, HostedWatchBatch,
        HostedWatchSubscribeInput, KvEntry, QueueEntry, StructuredTimeSeriesPoint, TimeSeriesPoint,
        hosted_external_credential_from_parts,
    };

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct CasPutRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub bytes: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct CasDigestRequest {
        #[prost(string, tag = "1")]
        pub digest: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct CasListRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct CasDigestResponse {
        #[prost(string, tag = "1")]
        pub digest: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct CasGetResponse {
        #[prost(bool, tag = "1")]
        pub found: bool,
        #[prost(bytes = "vec", tag = "2")]
        pub bytes: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct CasHasResponse {
        #[prost(bool, tag = "1")]
        pub present: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct CasListResponse {
        #[prost(string, repeated, tag = "1")]
        pub digests: Vec<String>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct CasDeleteResponse {
        #[prost(bool, tag = "1")]
        pub deleted: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FilesPathRequest {
        #[prost(string, tag = "1")]
        pub path: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FilesWriteRequest {
        #[prost(string, tag = "1")]
        pub path: String,
        #[prost(bytes = "vec", tag = "2")]
        pub bytes: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FilesMkdirRequest {
        #[prost(string, tag = "1")]
        pub path: String,
        #[prost(bool, tag = "2")]
        pub recursive: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FilesDeleteRequest {
        #[prost(string, tag = "1")]
        pub path: String,
        #[prost(bool, tag = "2")]
        pub recursive: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FilesReadResponse {
        #[prost(bytes = "vec", tag = "1")]
        pub bytes: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FilesStatResponse {
        #[prost(string, tag = "1")]
        pub path: String,
        #[prost(string, tag = "2")]
        pub kind: String,
        #[prost(uint64, tag = "3")]
        pub size: u64,
        #[prost(uint32, tag = "4")]
        pub mode: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FilesListEntry {
        #[prost(string, tag = "1")]
        pub name: String,
        #[prost(string, tag = "2")]
        pub kind: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FilesListRequest {
        #[prost(string, tag = "1")]
        pub path: String,
        #[prost(uint32, tag = "2")]
        pub batch_size: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FilesListResponse {
        #[prost(message, repeated, tag = "1")]
        pub entries: Vec<FilesListEntry>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FilesEmptyResponse {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsCommitRequest {
        #[prost(string, tag = "1")]
        pub message: String,
        #[prost(string, tag = "2")]
        pub author: String,
        #[prost(bool, tag = "3")]
        pub staged: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsCommitResponse {
        #[prost(string, tag = "1")]
        pub commit: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsLogRequest {
        #[prost(string, tag = "1")]
        pub ref_name: String,
        #[prost(uint32, tag = "2")]
        pub limit: u32,
        #[prost(uint32, tag = "3")]
        pub batch_size: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsLogResponse {
        #[prost(string, repeated, tag = "1")]
        pub commits: Vec<String>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsBranchRequest {
        #[prost(string, tag = "1")]
        pub name: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsCheckoutRequest {
        #[prost(string, tag = "1")]
        pub target: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsDiffRequest {
        #[prost(string, tag = "1")]
        pub from: String,
        #[prost(string, tag = "2")]
        pub to: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsDiffResponse {
        #[prost(bytes = "vec", tag = "1")]
        pub diff_cbor: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsMergeRequest {
        #[prost(string, tag = "1")]
        pub source: String,
        #[prost(string, tag = "2")]
        pub author: String,
        #[prost(bool, tag = "3")]
        pub cells: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsMergeResponse {
        #[prost(string, tag = "1")]
        pub outcome: String,
        #[prost(string, tag = "2")]
        pub commit: String,
        #[prost(string, repeated, tag = "3")]
        pub paths: Vec<String>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsStatusRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsChangeMessage {
        #[prost(string, tag = "1")]
        pub path: String,
        #[prost(string, tag = "2")]
        pub kind: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsStatusResponse {
        #[prost(message, repeated, tag = "1")]
        pub staged: Vec<VcsChangeMessage>,
        #[prost(message, repeated, tag = "2")]
        pub unstaged: Vec<VcsChangeMessage>,
        #[prost(string, repeated, tag = "3")]
        pub untracked: Vec<String>,
        #[prost(string, repeated, tag = "4")]
        pub conflicts: Vec<String>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsPathsRequest {
        #[prost(string, repeated, tag = "1")]
        pub paths: Vec<String>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct VcsEmptyResponse {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct KvPutRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub key_cbor: Vec<u8>,
        #[prost(bytes = "vec", tag = "2")]
        pub value: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct KvKeyRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub key_cbor: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct KvListRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct KvRangeRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub lo_cbor: Vec<u8>,
        #[prost(bytes = "vec", tag = "2")]
        pub hi_cbor: Vec<u8>,
        #[prost(uint32, tag = "3")]
        pub batch_size: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct KvEntryMessage {
        #[prost(bytes = "vec", tag = "1")]
        pub key_cbor: Vec<u8>,
        #[prost(bytes = "vec", tag = "2")]
        pub value: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct KvGetResponse {
        #[prost(bool, tag = "1")]
        pub found: bool,
        #[prost(bytes = "vec", tag = "2")]
        pub value: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct KvDeleteResponse {
        #[prost(bool, tag = "1")]
        pub deleted: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct KvListResponse {
        #[prost(message, repeated, tag = "1")]
        pub entries: Vec<KvEntryMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct KvRangeResponse {
        #[prost(message, repeated, tag = "1")]
        pub entries: Vec<KvEntryMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct KvEmptyResponse {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentPutTextRequest {
        #[prost(string, tag = "1")]
        pub id: String,
        #[prost(string, tag = "2")]
        pub text: String,
        #[prost(string, optional, tag = "3")]
        pub expected_entity_tag: Option<String>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentIdRequest {
        #[prost(string, tag = "1")]
        pub id: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentTextResponse {
        #[prost(bool, tag = "1")]
        pub found: bool,
        #[prost(string, tag = "2")]
        pub text: String,
        #[prost(string, tag = "3")]
        pub digest: String,
        #[prost(string, tag = "4")]
        pub entity_tag: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentPutBinaryRequest {
        #[prost(string, tag = "1")]
        pub id: String,
        #[prost(bytes = "vec", tag = "2")]
        pub bytes: Vec<u8>,
        #[prost(string, optional, tag = "3")]
        pub expected_entity_tag: Option<String>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentBinaryResponse {
        #[prost(bool, tag = "1")]
        pub found: bool,
        #[prost(bytes = "vec", tag = "2")]
        pub bytes: Vec<u8>,
        #[prost(string, tag = "3")]
        pub digest: String,
        #[prost(string, tag = "4")]
        pub entity_tag: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentDigestResponse {
        #[prost(string, tag = "1")]
        pub digest: String,
        #[prost(string, tag = "2")]
        pub entity_tag: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentDeleteResponse {
        #[prost(bool, tag = "1")]
        pub deleted: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentListRequest {
        #[prost(uint32, tag = "1")]
        pub batch_size: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentEntryMessage {
        #[prost(string, tag = "1")]
        pub id: String,
        #[prost(bytes = "vec", tag = "2")]
        pub document: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentListResponse {
        #[prost(message, repeated, tag = "1")]
        pub entries: Vec<DocumentEntryMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentIndexCreateRequest {
        #[prost(string, tag = "1")]
        pub name: String,
        #[prost(string, tag = "2")]
        pub path: String,
        #[prost(bool, tag = "3")]
        pub unique: bool,
        #[prost(bytes = "vec", tag = "4")]
        pub declaration_json: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentIndexNameRequest {
        #[prost(string, tag = "1")]
        pub name: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentIndexListRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentIndexMessage {
        #[prost(string, tag = "1")]
        pub name: String,
        #[prost(string, tag = "2")]
        pub path: String,
        #[prost(bool, tag = "3")]
        pub unique: bool,
        #[prost(string, tag = "4")]
        pub index_id: String,
        #[prost(string, tag = "5")]
        pub extractor: String,
        #[prost(string, tag = "6")]
        pub key_codec: String,
        #[prost(string, tag = "7")]
        pub comparator: String,
        #[prost(string, tag = "8")]
        pub uniqueness: String,
        #[prost(string, tag = "9")]
        pub failure_policy: String,
        #[prost(uint64, tag = "10")]
        pub declaration_version: u64,
        #[prost(string, optional, tag = "11")]
        pub analyzer_profile: Option<String>,
        #[prost(string, optional, tag = "12")]
        pub projection: Option<String>,
        #[prost(string, optional, tag = "13")]
        pub partial_filter: Option<String>,
        #[prost(string, tag = "14")]
        pub metadata_json: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentIndexListResponse {
        #[prost(message, repeated, tag = "1")]
        pub indexes: Vec<DocumentIndexMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentIndexDropResponse {
        #[prost(bool, tag = "1")]
        pub dropped: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentIndexStatusRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentIndexStatusMessage {
        #[prost(string, tag = "1")]
        pub name: String,
        #[prost(bool, tag = "2")]
        pub ready: bool,
        #[prost(uint64, tag = "3")]
        pub entries: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentIndexStatusResponse {
        #[prost(message, repeated, tag = "1")]
        pub statuses: Vec<DocumentIndexStatusMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentFindRequest {
        #[prost(string, tag = "1")]
        pub index: String,
        #[prost(string, tag = "2")]
        pub value_json: String,
        #[prost(uint32, tag = "3")]
        pub batch_size: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentFindResponse {
        #[prost(string, repeated, tag = "1")]
        pub ids: Vec<String>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentQueryRequest {
        #[prost(string, tag = "1")]
        pub query_json: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentQueryResponse {
        #[prost(string, tag = "1")]
        pub result_json: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct DocumentEmptyResponse {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsMappingEntry {
        #[prost(string, tag = "1")]
        pub field: String,
        #[prost(string, tag = "2")]
        pub field_type: String,
        #[prost(bool, optional, tag = "3")]
        pub stored: Option<bool>,
        #[prost(bool, optional, tag = "4")]
        pub faceted: Option<bool>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsCreateRequest {
        #[prost(message, repeated, tag = "1")]
        pub mapping: Vec<FtsMappingEntry>,
    }

    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum FtsFieldValueKind {
        #[prost(string, tag = "2")]
        Text(String),
        #[prost(bytes, tag = "3")]
        Bytes(Vec<u8>),
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsFieldValueMessage {
        #[prost(string, tag = "1")]
        pub field: String,
        #[prost(oneof = "FtsFieldValueKind", tags = "2, 3")]
        pub value: Option<FtsFieldValueKind>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsIndexRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub id: Vec<u8>,
        #[prost(message, repeated, tag = "2")]
        pub document: Vec<FtsFieldValueMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsIdRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub id: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsGetResponse {
        #[prost(bool, tag = "1")]
        pub found: bool,
        #[prost(message, repeated, tag = "2")]
        pub document: Vec<FtsFieldValueMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsDeleteResponse {
        #[prost(bool, tag = "1")]
        pub deleted: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsIdsRequest {
        #[prost(bytes = "vec", optional, tag = "1")]
        pub prefix: Option<Vec<u8>>,
        #[prost(uint32, tag = "2")]
        pub batch_size: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsIdsResponse {
        #[prost(bytes = "vec", repeated, tag = "1")]
        pub ids: Vec<Vec<u8>>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsRemapRequest {
        #[prost(message, repeated, tag = "1")]
        pub mapping: Vec<FtsMappingEntry>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsQueryMessage {
        #[prost(string, tag = "1")]
        pub kind: String,
        #[prost(string, tag = "2")]
        pub field: String,
        #[prost(string, tag = "3")]
        pub text: String,
        #[prost(bytes = "vec", tag = "4")]
        pub value: Vec<u8>,
        #[prost(string, repeated, tag = "5")]
        pub terms: Vec<String>,
        #[prost(uint32, tag = "6")]
        pub slop: u32,
        #[prost(bytes = "vec", optional, tag = "7")]
        pub lower: Option<Vec<u8>>,
        #[prost(bytes = "vec", optional, tag = "8")]
        pub upper: Option<Vec<u8>>,
        #[prost(bool, tag = "9")]
        pub include_lower: bool,
        #[prost(bool, tag = "10")]
        pub include_upper: bool,
        #[prost(message, repeated, tag = "11")]
        pub must: Vec<FtsQueryMessage>,
        #[prost(message, repeated, tag = "12")]
        pub should: Vec<FtsQueryMessage>,
        #[prost(message, repeated, tag = "13")]
        pub must_not: Vec<FtsQueryMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsQueryRequest {
        #[prost(message, optional, tag = "1")]
        pub query: Option<FtsQueryMessage>,
        #[prost(uint32, tag = "2")]
        pub limit: u32,
        #[prost(uint32, tag = "3")]
        pub offset: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsHitMessage {
        #[prost(bytes = "vec", tag = "1")]
        pub id: Vec<u8>,
        #[prost(float, tag = "2")]
        pub score: f32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsQueryResponse {
        #[prost(bool, tag = "1")]
        pub reduced: bool,
        #[prost(message, repeated, tag = "2")]
        pub hits: Vec<FtsHitMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct FtsEmptyResponse {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GraphUpsertNodeRequest {
        #[prost(string, tag = "1")]
        pub id: String,
        #[prost(bytes = "vec", tag = "2")]
        pub props_cbor: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GraphNodeRequest {
        #[prost(string, tag = "1")]
        pub id: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GraphNodeResponse {
        #[prost(bool, tag = "1")]
        pub found: bool,
        #[prost(bytes = "vec", tag = "2")]
        pub props_cbor: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GraphUpsertEdgeRequest {
        #[prost(string, tag = "1")]
        pub id: String,
        #[prost(string, tag = "2")]
        pub src: String,
        #[prost(string, tag = "3")]
        pub dst: String,
        #[prost(string, tag = "4")]
        pub label: String,
        #[prost(bytes = "vec", tag = "5")]
        pub props_cbor: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GraphNeighborsRequest {
        #[prost(string, tag = "1")]
        pub id: String,
        #[prost(uint32, tag = "2")]
        pub batch_size: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GraphReachableRequest {
        #[prost(string, tag = "1")]
        pub start: String,
        #[prost(uint64, optional, tag = "2")]
        pub max_depth: Option<u64>,
        #[prost(string, optional, tag = "3")]
        pub via_label: Option<String>,
        #[prost(uint32, tag = "4")]
        pub batch_size: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GraphIdsResponse {
        #[prost(string, repeated, tag = "1")]
        pub ids: Vec<String>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GraphQueryRequest {
        #[prost(string, tag = "1")]
        pub opencypher: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GraphQueryResponse {
        #[prost(bytes = "vec", tag = "1")]
        pub result_cbor: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GraphExplainResponse {
        #[prost(bytes = "vec", tag = "1")]
        pub explain_cbor: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GraphCapabilitiesRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GraphCapabilitiesResponse {
        #[prost(string, tag = "1")]
        pub collection: String,
        #[prost(string, tag = "2")]
        pub query_language: String,
        #[prost(string, tag = "3")]
        pub neo4j: String,
        #[prost(string, tag = "4")]
        pub gremlin: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct GraphEmptyResponse {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QueueAppendRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub payload: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QueueAppendResponse {
        #[prost(uint64, tag = "1")]
        pub seq: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QueueGetRequest {
        #[prost(uint64, tag = "1")]
        pub seq: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QueueGetResponse {
        #[prost(bool, tag = "1")]
        pub found: bool,
        #[prost(bytes = "vec", tag = "2")]
        pub payload: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QueueRangeRequest {
        #[prost(uint64, tag = "1")]
        pub lo: u64,
        #[prost(uint64, tag = "2")]
        pub hi: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QueueEntryMessage {
        #[prost(uint64, tag = "1")]
        pub seq: u64,
        #[prost(bytes = "vec", tag = "2")]
        pub payload: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QueueRangeResponse {
        #[prost(message, repeated, tag = "1")]
        pub entries: Vec<QueueEntryMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QueueLenRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QueueLenResponse {
        #[prost(uint64, tag = "1")]
        pub len: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerAppendRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub payload: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerAppendResponse {
        #[prost(uint64, tag = "1")]
        pub seq: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerGetRequest {
        #[prost(uint64, tag = "1")]
        pub seq: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerGetResponse {
        #[prost(bool, tag = "1")]
        pub found: bool,
        #[prost(bytes = "vec", tag = "2")]
        pub payload: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerRangeRequest {
        #[prost(uint64, tag = "1")]
        pub start: u64,
        #[prost(uint64, tag = "2")]
        pub end: u64,
        #[prost(uint32, tag = "3")]
        pub batch_size: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerEntryMessage {
        #[prost(uint64, tag = "1")]
        pub seq: u64,
        #[prost(bytes = "vec", tag = "2")]
        pub payload: Vec<u8>,
        #[prost(string, tag = "3")]
        pub entry_hash: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerRangeResponse {
        #[prost(uint64, tag = "1")]
        pub start: u64,
        #[prost(uint64, tag = "2")]
        pub end: u64,
        #[prost(string, tag = "3")]
        pub state: String,
        #[prost(message, repeated, tag = "4")]
        pub entries: Vec<LedgerEntryMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerHeadRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerHeadResponse {
        #[prost(bool, tag = "1")]
        pub found: bool,
        #[prost(string, tag = "2")]
        pub digest: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerLenRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerLenResponse {
        #[prost(uint64, tag = "1")]
        pub len: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerVerifyRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerCollectionsRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerCollectionsResponse {
        #[prost(string, repeated, tag = "1")]
        pub collections: Vec<String>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerCheckpointPayloadRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerCheckpointPayloadResponse {
        #[prost(bytes = "vec", tag = "1")]
        pub payload_cbor: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerVerifyCheckpointRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerVerifyCheckpointResponse {
        #[prost(uint64, tag = "1")]
        pub signatures: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerProofTreeRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerProofResponse {
        #[prost(bytes = "vec", tag = "1")]
        pub proof_cbor: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerInclusionProofRequest {
        #[prost(uint64, tag = "1")]
        pub seq: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerConsistencyProofRequest {
        #[prost(uint64, tag = "1")]
        pub first_tree_size: u64,
        #[prost(uint64, tag = "2")]
        pub second_tree_size: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct LedgerEmptyResponse {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesPutRequest {
        #[prost(int64, tag = "1")]
        pub timestamp: i64,
        #[prost(bytes = "vec", tag = "2")]
        pub value: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesGetRequest {
        #[prost(int64, tag = "1")]
        pub timestamp: i64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesLatestRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesRangeRequest {
        #[prost(int64, tag = "1")]
        pub from: i64,
        #[prost(int64, tag = "2")]
        pub to: i64,
        #[prost(uint32, tag = "3")]
        pub batch_size: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesPointMessage {
        #[prost(int64, tag = "1")]
        pub timestamp: i64,
        #[prost(bytes = "vec", tag = "2")]
        pub value: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesPointResponse {
        #[prost(bool, tag = "1")]
        pub found: bool,
        #[prost(message, optional, tag = "2")]
        pub point: Option<TimeSeriesPointMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesRangeResponse {
        #[prost(message, repeated, tag = "1")]
        pub points: Vec<TimeSeriesPointMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesTagMessage {
        #[prost(string, tag = "1")]
        pub name: String,
        #[prost(string, tag = "2")]
        pub value: String,
    }

    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum TimeSeriesFieldKind {
        #[prost(int64, tag = "2")]
        Int(i64),
        #[prost(double, tag = "3")]
        Float(f64),
        #[prost(string, tag = "4")]
        Text(String),
        #[prost(bool, tag = "5")]
        Bool(bool),
        #[prost(bytes, tag = "6")]
        Bytes(Vec<u8>),
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesFieldMessage {
        #[prost(string, tag = "1")]
        pub name: String,
        #[prost(oneof = "TimeSeriesFieldKind", tags = "2, 3, 4, 5, 6")]
        pub value: Option<TimeSeriesFieldKind>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct StructuredTimeSeriesPointMessage {
        #[prost(string, tag = "1")]
        pub measurement: String,
        #[prost(message, repeated, tag = "2")]
        pub tags: Vec<TimeSeriesTagMessage>,
        #[prost(int64, tag = "3")]
        pub timestamp_ns: i64,
        #[prost(message, repeated, tag = "4")]
        pub fields: Vec<TimeSeriesFieldMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesPutStructuredRequest {
        #[prost(message, optional, tag = "1")]
        pub point: Option<StructuredTimeSeriesPointMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesStructuredRangeRequest {
        #[prost(int64, tag = "1")]
        pub from_ns: i64,
        #[prost(int64, tag = "2")]
        pub to_ns: i64,
        #[prost(uint32, tag = "3")]
        pub batch_size: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesStructuredRangeResponse {
        #[prost(message, repeated, tag = "1")]
        pub points: Vec<StructuredTimeSeriesPointMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesPolicyRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesRollupMessage {
        #[prost(string, tag = "1")]
        pub name: String,
        #[prost(int64, tag = "2")]
        pub resolution_ns: i64,
        #[prost(string, tag = "3")]
        pub aggregation: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesPolicyMessage {
        #[prost(int64, optional, tag = "1")]
        pub query_start_ns: Option<i64>,
        #[prost(message, repeated, tag = "2")]
        pub rollups: Vec<TimeSeriesRollupMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesSetPolicyRequest {
        #[prost(int64, optional, tag = "1")]
        pub query_start_ns: Option<i64>,
        #[prost(message, repeated, tag = "2")]
        pub rollups: Vec<TimeSeriesRollupMessage>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesMaterializeRollupRequest {
        #[prost(string, tag = "1")]
        pub rollup: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesRangeRollupStructuredRequest {
        #[prost(string, tag = "1")]
        pub rollup: String,
        #[prost(int64, tag = "2")]
        pub from_ns: i64,
        #[prost(int64, tag = "3")]
        pub to_ns: i64,
        #[prost(uint32, tag = "4")]
        pub batch_size: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesPruneBeforeRequest {
        #[prost(int64, tag = "1")]
        pub cutoff_ns: i64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesPruneBeforeResponse {
        #[prost(uint64, tag = "1")]
        pub pruned: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct TimeSeriesEmptyResponse {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ColumnarCreateRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub columns_cbor: Vec<u8>,
        #[prost(uint64, tag = "2")]
        pub target_segment_rows: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ColumnarAppendRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub row_cbor: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ColumnarSelectRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub columns_cbor: Vec<u8>,
        #[prost(bytes = "vec", tag = "2")]
        pub filter_cbor: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ColumnarAggregateRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub aggregates_cbor: Vec<u8>,
        #[prost(bytes = "vec", tag = "2")]
        pub filter_cbor: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ColumnarTransferRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub bytes: Vec<u8>,
        #[prost(uint64, tag = "2")]
        pub target_segment_rows: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ColumnarEmptyRequest {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ColumnarCborResponse {
        #[prost(bytes = "vec", tag = "1")]
        pub cbor: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ColumnarRowsResponse {
        #[prost(uint64, tag = "1")]
        pub rows: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ColumnarBytesResponse {
        #[prost(bytes = "vec", tag = "1")]
        pub bytes: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ColumnarEmptyResponse {}

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct SqlRequest {
        #[prost(string, tag = "1")]
        pub sql: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct SqlResponse {
        #[prost(bytes = "vec", tag = "1")]
        pub cbor: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ExecRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub request: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct ExecResponse {
        #[prost(bytes = "vec", tag = "1")]
        pub result: Vec<u8>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct WatchSubscribeRequest {
        #[prost(string, tag = "1")]
        pub branch: String,
        #[prost(string, optional, tag = "2")]
        pub from: Option<String>,
        #[prost(string, optional, tag = "3")]
        pub facet: Option<String>,
        #[prost(string, optional, tag = "4")]
        pub path_prefix: Option<String>,
        #[prost(string, repeated, tag = "5")]
        pub change_kinds: Vec<String>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct WatchSubscribeResponse {
        #[prost(string, tag = "1")]
        pub cursor: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct WatchPollRequest {
        #[prost(string, tag = "1")]
        pub cursor: String,
        #[prost(uint32, tag = "2")]
        pub max: u32,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct WatchPollResponse {
        #[prost(message, repeated, tag = "1")]
        pub events: Vec<WatchDataChange>,
        #[prost(string, tag = "2")]
        pub next: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct WatchStreamRequest {
        #[prost(string, tag = "1")]
        pub cursor: String,
        #[prost(uint32, tag = "2")]
        pub max: u32,
        #[prost(uint32, optional, tag = "3")]
        pub interval_ms: Option<u32>,
        #[prost(uint32, optional, tag = "4")]
        pub debounce_ms: Option<u32>,
        #[prost(uint64, optional, tag = "5")]
        pub limit: Option<u64>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct WatchStreamResponse {
        #[prost(string, tag = "1")]
        pub source_cursor: String,
        #[prost(message, repeated, tag = "2")]
        pub events: Vec<WatchDataChange>,
        #[prost(string, tag = "3")]
        pub next: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct WatchDataChange {
        #[prost(string, tag = "1")]
        pub workspace: String,
        #[prost(string, tag = "2")]
        pub ref_name: String,
        #[prost(string, tag = "3")]
        pub commit: String,
        #[prost(string, optional, tag = "4")]
        pub parent: Option<String>,
        #[prost(uint64, tag = "5")]
        pub seq: u64,
        #[prost(message, repeated, tag = "6")]
        pub changes: Vec<WatchDomainChange>,
        #[prost(message, repeated, tag = "7")]
        pub unsupported_domains: Vec<WatchUnsupportedDomain>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct WatchDomainChange {
        #[prost(string, tag = "1")]
        pub domain: String,
        #[prost(uint32, tag = "2")]
        pub schema_version: u32,
        #[prost(string, tag = "3")]
        pub kind: String,
        #[prost(bytes = "vec", tag = "4")]
        pub key: Vec<u8>,
        #[prost(string, optional, tag = "5")]
        pub before: Option<String>,
        #[prost(string, optional, tag = "6")]
        pub after: Option<String>,
        #[prost(bytes = "vec", optional, tag = "7")]
        pub detail: Option<Vec<u8>>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct WatchUnsupportedDomain {
        #[prost(string, tag = "1")]
        pub domain: String,
        #[prost(string, tag = "2")]
        pub capability: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdResponseHeader {
        #[prost(uint64, tag = "1")]
        pub cluster_id: u64,
        #[prost(uint64, tag = "2")]
        pub member_id: u64,
        #[prost(int64, tag = "3")]
        pub revision: i64,
        #[prost(uint64, tag = "4")]
        pub raft_term: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdKeyValue {
        #[prost(bytes = "vec", tag = "1")]
        pub key: Vec<u8>,
        #[prost(int64, tag = "2")]
        pub create_revision: i64,
        #[prost(int64, tag = "3")]
        pub mod_revision: i64,
        #[prost(int64, tag = "4")]
        pub version: i64,
        #[prost(bytes = "vec", tag = "5")]
        pub value: Vec<u8>,
        #[prost(int64, tag = "6")]
        pub lease: i64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdRangeRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub key: Vec<u8>,
        #[prost(bytes = "vec", tag = "2")]
        pub range_end: Vec<u8>,
        #[prost(int64, tag = "3")]
        pub limit: i64,
        #[prost(int64, tag = "4")]
        pub revision: i64,
        #[prost(enumeration = "EtcdRangeSortOrder", tag = "5")]
        pub sort_order: i32,
        #[prost(enumeration = "EtcdRangeSortTarget", tag = "6")]
        pub sort_target: i32,
        #[prost(bool, tag = "7")]
        pub serializable: bool,
        #[prost(bool, tag = "8")]
        pub keys_only: bool,
        #[prost(bool, tag = "9")]
        pub count_only: bool,
        #[prost(int64, tag = "10")]
        pub min_mod_revision: i64,
        #[prost(int64, tag = "11")]
        pub max_mod_revision: i64,
        #[prost(int64, tag = "12")]
        pub min_create_revision: i64,
        #[prost(int64, tag = "13")]
        pub max_create_revision: i64,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
    #[repr(i32)]
    pub enum EtcdRangeSortOrder {
        None = 0,
        Ascend = 1,
        Descend = 2,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
    #[repr(i32)]
    pub enum EtcdRangeSortTarget {
        Key = 0,
        Version = 1,
        Create = 2,
        Mod = 3,
        Value = 4,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdRangeResponse {
        #[prost(message, optional, tag = "1")]
        pub header: Option<EtcdResponseHeader>,
        #[prost(message, repeated, tag = "2")]
        pub kvs: Vec<EtcdKeyValue>,
        #[prost(bool, tag = "3")]
        pub more: bool,
        #[prost(int64, tag = "4")]
        pub count: i64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdPutRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub key: Vec<u8>,
        #[prost(bytes = "vec", tag = "2")]
        pub value: Vec<u8>,
        #[prost(int64, tag = "3")]
        pub lease: i64,
        #[prost(bool, tag = "4")]
        pub prev_kv: bool,
        #[prost(bool, tag = "5")]
        pub ignore_value: bool,
        #[prost(bool, tag = "6")]
        pub ignore_lease: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdPutResponse {
        #[prost(message, optional, tag = "1")]
        pub header: Option<EtcdResponseHeader>,
        #[prost(message, optional, tag = "2")]
        pub prev_kv: Option<EtcdKeyValue>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdDeleteRangeRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub key: Vec<u8>,
        #[prost(bytes = "vec", tag = "2")]
        pub range_end: Vec<u8>,
        #[prost(bool, tag = "3")]
        pub prev_kv: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdDeleteRangeResponse {
        #[prost(message, optional, tag = "1")]
        pub header: Option<EtcdResponseHeader>,
        #[prost(int64, tag = "2")]
        pub deleted: i64,
        #[prost(message, repeated, tag = "3")]
        pub prev_kvs: Vec<EtcdKeyValue>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdCompactionRequest {
        #[prost(int64, tag = "1")]
        pub revision: i64,
        #[prost(bool, tag = "2")]
        pub physical: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdCompactionResponse {
        #[prost(message, optional, tag = "1")]
        pub header: Option<EtcdResponseHeader>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdCompare {
        #[prost(enumeration = "EtcdCompareResult", tag = "1")]
        pub result: i32,
        #[prost(enumeration = "EtcdCompareTarget", tag = "2")]
        pub target: i32,
        #[prost(bytes = "vec", tag = "3")]
        pub key: Vec<u8>,
        #[prost(bytes = "vec", tag = "4")]
        pub value: Vec<u8>,
        #[prost(int64, tag = "5")]
        pub version: i64,
        #[prost(int64, tag = "6")]
        pub create_revision: i64,
        #[prost(int64, tag = "7")]
        pub mod_revision: i64,
        #[prost(int64, tag = "8")]
        pub lease: i64,
        #[prost(bytes = "vec", tag = "64")]
        pub range_end: Vec<u8>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
    #[repr(i32)]
    pub enum EtcdCompareResult {
        Equal = 0,
        Greater = 1,
        Less = 2,
        NotEqual = 3,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
    #[repr(i32)]
    pub enum EtcdCompareTarget {
        Version = 0,
        Create = 1,
        Mod = 2,
        Value = 3,
        Lease = 4,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdRequestOp {
        #[prost(oneof = "etcd_request_op::Request", tags = "1, 2, 3, 4")]
        pub request: Option<etcd_request_op::Request>,
    }

    pub mod etcd_request_op {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum Request {
            #[prost(message, tag = "1")]
            RequestRange(super::EtcdRangeRequest),
            #[prost(message, tag = "2")]
            RequestPut(super::EtcdPutRequest),
            #[prost(message, tag = "3")]
            RequestDeleteRange(super::EtcdDeleteRangeRequest),
            #[prost(message, tag = "4")]
            RequestTxn(super::EtcdTxnRequest),
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdResponseOp {
        #[prost(oneof = "etcd_response_op::Response", tags = "1, 2, 3, 4")]
        pub response: Option<etcd_response_op::Response>,
    }

    pub mod etcd_response_op {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum Response {
            #[prost(message, tag = "1")]
            ResponseRange(super::EtcdRangeResponse),
            #[prost(message, tag = "2")]
            ResponsePut(super::EtcdPutResponse),
            #[prost(message, tag = "3")]
            ResponseDeleteRange(super::EtcdDeleteRangeResponse),
            #[prost(message, tag = "4")]
            ResponseTxn(super::EtcdTxnResponse),
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdTxnRequest {
        #[prost(message, repeated, tag = "1")]
        pub compare: Vec<EtcdCompare>,
        #[prost(message, repeated, tag = "2")]
        pub success: Vec<EtcdRequestOp>,
        #[prost(message, repeated, tag = "3")]
        pub failure: Vec<EtcdRequestOp>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdTxnResponse {
        #[prost(message, optional, tag = "1")]
        pub header: Option<EtcdResponseHeader>,
        #[prost(bool, tag = "2")]
        pub succeeded: bool,
        #[prost(message, repeated, tag = "3")]
        pub responses: Vec<EtcdResponseOp>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdLeaseGrantRequest {
        #[prost(int64, tag = "1")]
        pub ttl: i64,
        #[prost(int64, tag = "2")]
        pub id: i64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdLeaseGrantResponse {
        #[prost(message, optional, tag = "1")]
        pub header: Option<EtcdResponseHeader>,
        #[prost(int64, tag = "2")]
        pub id: i64,
        #[prost(int64, tag = "3")]
        pub ttl: i64,
        #[prost(string, tag = "4")]
        pub error: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdLeaseRevokeRequest {
        #[prost(int64, tag = "1")]
        pub id: i64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdLeaseRevokeResponse {
        #[prost(message, optional, tag = "1")]
        pub header: Option<EtcdResponseHeader>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdLeaseKeepAliveRequest {
        #[prost(int64, tag = "1")]
        pub id: i64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdLeaseKeepAliveResponse {
        #[prost(message, optional, tag = "1")]
        pub header: Option<EtcdResponseHeader>,
        #[prost(int64, tag = "2")]
        pub id: i64,
        #[prost(int64, tag = "3")]
        pub ttl: i64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdWatchRequest {
        #[prost(oneof = "etcd_watch_request::RequestUnion", tags = "1, 2, 3")]
        pub request_union: Option<etcd_watch_request::RequestUnion>,
    }

    pub mod etcd_watch_request {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum RequestUnion {
            #[prost(message, tag = "1")]
            CreateRequest(super::EtcdWatchCreateRequest),
            #[prost(message, tag = "2")]
            CancelRequest(super::EtcdWatchCancelRequest),
            #[prost(message, tag = "3")]
            ProgressRequest(super::EtcdWatchProgressRequest),
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdWatchCreateRequest {
        #[prost(bytes = "vec", tag = "1")]
        pub key: Vec<u8>,
        #[prost(bytes = "vec", tag = "2")]
        pub range_end: Vec<u8>,
        #[prost(int64, tag = "3")]
        pub start_revision: i64,
        #[prost(bool, tag = "4")]
        pub progress_notify: bool,
        #[prost(enumeration = "EtcdWatchFilterType", repeated, tag = "5")]
        pub filters: Vec<i32>,
        #[prost(bool, tag = "6")]
        pub prev_kv: bool,
        #[prost(int64, tag = "7")]
        pub watch_id: i64,
        #[prost(bool, tag = "8")]
        pub fragment: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdWatchCancelRequest {
        #[prost(int64, tag = "1")]
        pub watch_id: i64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdWatchProgressRequest {}

    #[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
    #[repr(i32)]
    pub enum EtcdWatchFilterType {
        Noput = 0,
        Nodelete = 1,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, prost::Enumeration)]
    #[repr(i32)]
    pub enum EtcdEventType {
        Put = 0,
        Delete = 1,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdEvent {
        #[prost(enumeration = "EtcdEventType", tag = "1")]
        pub r#type: i32,
        #[prost(message, optional, tag = "2")]
        pub kv: Option<EtcdKeyValue>,
        #[prost(message, optional, tag = "3")]
        pub prev_kv: Option<EtcdKeyValue>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct EtcdWatchResponse {
        #[prost(message, optional, tag = "1")]
        pub header: Option<EtcdResponseHeader>,
        #[prost(int64, tag = "2")]
        pub watch_id: i64,
        #[prost(bool, tag = "3")]
        pub created: bool,
        #[prost(bool, tag = "4")]
        pub canceled: bool,
        #[prost(int64, tag = "5")]
        pub compact_revision: i64,
        #[prost(string, tag = "6")]
        pub cancel_reason: String,
        #[prost(message, repeated, tag = "11")]
        pub events: Vec<EtcdEvent>,
    }

    #[tonic::async_trait]
    pub trait EtcdKv: Send + Sync + 'static {
        async fn range(
            &self,
            request: Request<EtcdRangeRequest>,
        ) -> Result<Response<EtcdRangeResponse>, Status>;

        async fn put(
            &self,
            request: Request<EtcdPutRequest>,
        ) -> Result<Response<EtcdPutResponse>, Status>;

        async fn delete_range(
            &self,
            request: Request<EtcdDeleteRangeRequest>,
        ) -> Result<Response<EtcdDeleteRangeResponse>, Status>;

        async fn compact(
            &self,
            request: Request<EtcdCompactionRequest>,
        ) -> Result<Response<EtcdCompactionResponse>, Status>;

        async fn txn(
            &self,
            request: Request<EtcdTxnRequest>,
        ) -> Result<Response<EtcdTxnResponse>, Status>;
    }

    #[tonic::async_trait]
    pub trait EtcdLease: Send + Sync + 'static {
        async fn lease_grant(
            &self,
            request: Request<EtcdLeaseGrantRequest>,
        ) -> Result<Response<EtcdLeaseGrantResponse>, Status>;

        async fn lease_revoke(
            &self,
            request: Request<EtcdLeaseRevokeRequest>,
        ) -> Result<Response<EtcdLeaseRevokeResponse>, Status>;

        async fn lease_keep_alive_once(
            &self,
            request: Request<EtcdLeaseKeepAliveRequest>,
        ) -> Result<Response<EtcdLeaseKeepAliveResponse>, Status>;
    }

    #[tonic::async_trait]
    pub trait EtcdWatch: Send + Sync + 'static {
        type WatchStream: Stream<Item = Result<EtcdWatchResponse, Status>> + Send + 'static;

        async fn watch(
            &self,
            request: Request<tonic::Streaming<EtcdWatchRequest>>,
        ) -> Result<Response<Self::WatchStream>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedEtcdKvGrpcService {
        kernel: HostedKernel,
        workspace: String,
        collection: String,
    }

    impl HostedEtcdKvGrpcService {
        pub fn new(
            kernel: HostedKernel,
            workspace: impl Into<String>,
            collection: impl Into<String>,
        ) -> Self {
            Self {
                kernel,
                workspace: workspace.into(),
                collection: collection.into(),
            }
        }
    }

    #[tonic::async_trait]
    impl EtcdKv for HostedEtcdKvGrpcService {
        async fn range(
            &self,
            request: Request<EtcdRangeRequest>,
        ) -> Result<Response<EtcdRangeResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            ensure_etcd_range_request(&input)?;
            let result = self
                .kernel
                .data()
                .etcd_range(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    input.key,
                    input.range_end,
                    input.limit,
                    input.revision,
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(etcd_range_response(
                result,
                input.count_only,
                input.keys_only,
            )))
        }

        async fn put(
            &self,
            request: Request<EtcdPutRequest>,
        ) -> Result<Response<EtcdPutResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            if input.ignore_value || input.ignore_lease {
                return Err(Status::unimplemented(
                    "etcd put ignore_value and ignore_lease are not supported",
                ));
            }
            if input.key.is_empty() {
                return Err(Status::invalid_argument("etcd put key is required"));
            }
            let result = self
                .kernel
                .data()
                .etcd_put(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    input.key,
                    input.value,
                    input.lease,
                    input.prev_kv,
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(EtcdPutResponse {
                header: Some(etcd_header(result.revision)),
                prev_kv: result.prev_kv.map(etcd_key_value),
            }))
        }

        async fn delete_range(
            &self,
            request: Request<EtcdDeleteRangeRequest>,
        ) -> Result<Response<EtcdDeleteRangeResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            if input.key.is_empty() {
                return Err(Status::invalid_argument("etcd delete key is required"));
            }
            let result = self
                .kernel
                .data()
                .etcd_delete_range(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    input.key,
                    input.range_end,
                    input.prev_kv,
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(EtcdDeleteRangeResponse {
                header: Some(etcd_header(result.revision)),
                deleted: result.deleted,
                prev_kvs: result.prev_kvs.into_iter().map(etcd_key_value).collect(),
            }))
        }

        async fn compact(
            &self,
            request: Request<EtcdCompactionRequest>,
        ) -> Result<Response<EtcdCompactionResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let result = self
                .kernel
                .data()
                .etcd_compact(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    input.revision,
                    input.physical,
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(EtcdCompactionResponse {
                header: Some(etcd_header(result.revision)),
            }))
        }

        async fn txn(
            &self,
            request: Request<EtcdTxnRequest>,
        ) -> Result<Response<EtcdTxnResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let compare = input
                .compare
                .into_iter()
                .map(etcd_compare_from_proto)
                .collect::<Result<Vec<_>, _>>()?;
            let success = input
                .success
                .into_iter()
                .map(etcd_request_op_from_proto)
                .collect::<Result<Vec<_>, _>>()?;
            let failure = input
                .failure
                .into_iter()
                .map(etcd_request_op_from_proto)
                .collect::<Result<Vec<_>, _>>()?;
            let result = self
                .kernel
                .data()
                .etcd_txn(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    compare,
                    success,
                    failure,
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(EtcdTxnResponse {
                header: Some(etcd_header(result.revision)),
                succeeded: result.succeeded,
                responses: result
                    .responses
                    .into_iter()
                    .map(etcd_response_op_from_data)
                    .collect(),
            }))
        }
    }

    #[derive(Clone)]
    pub struct HostedEtcdLeaseGrpcService {
        kernel: HostedKernel,
        workspace: String,
        collection: String,
    }

    impl HostedEtcdLeaseGrpcService {
        pub fn new(
            kernel: HostedKernel,
            workspace: impl Into<String>,
            collection: impl Into<String>,
        ) -> Self {
            Self {
                kernel,
                workspace: workspace.into(),
                collection: collection.into(),
            }
        }
    }

    #[tonic::async_trait]
    impl EtcdLease for HostedEtcdLeaseGrpcService {
        async fn lease_grant(
            &self,
            request: Request<EtcdLeaseGrantRequest>,
        ) -> Result<Response<EtcdLeaseGrantResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let result = self
                .kernel
                .data()
                .etcd_lease_grant(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    input.id,
                    input.ttl,
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(EtcdLeaseGrantResponse {
                header: Some(etcd_header(result.revision)),
                id: result.id,
                ttl: result.ttl,
                error: String::new(),
            }))
        }

        async fn lease_revoke(
            &self,
            request: Request<EtcdLeaseRevokeRequest>,
        ) -> Result<Response<EtcdLeaseRevokeResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let result = self
                .kernel
                .data()
                .etcd_lease_revoke(&auth, &self.workspace, &self.collection, input.id)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(EtcdLeaseRevokeResponse {
                header: Some(etcd_header(result.revision)),
            }))
        }

        async fn lease_keep_alive_once(
            &self,
            request: Request<EtcdLeaseKeepAliveRequest>,
        ) -> Result<Response<EtcdLeaseKeepAliveResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let result = self
                .kernel
                .data()
                .etcd_lease_keep_alive(&auth, &self.workspace, &self.collection, input.id)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(EtcdLeaseKeepAliveResponse {
                header: Some(etcd_header(result.revision)),
                id: result.id,
                ttl: result.ttl,
            }))
        }
    }

    #[derive(Clone)]
    pub struct HostedEtcdWatchGrpcService {
        kernel: HostedKernel,
        workspace: String,
        collection: String,
    }

    impl HostedEtcdWatchGrpcService {
        pub fn new(
            kernel: HostedKernel,
            workspace: impl Into<String>,
            collection: impl Into<String>,
        ) -> Self {
            Self {
                kernel,
                workspace: workspace.into(),
                collection: collection.into(),
            }
        }
    }

    #[tonic::async_trait]
    impl EtcdWatch for HostedEtcdWatchGrpcService {
        type WatchStream =
            tokio_stream::wrappers::ReceiverStream<Result<EtcdWatchResponse, Status>>;

        async fn watch(
            &self,
            request: Request<tonic::Streaming<EtcdWatchRequest>>,
        ) -> Result<Response<Self::WatchStream>, Status> {
            let auth = hosted_auth(&request)?;
            let mut input = request.into_inner();
            let kernel = self.kernel.clone();
            let workspace = self.workspace.clone();
            let collection = self.collection.clone();
            let (tx, rx) = mpsc::channel::<Result<EtcdWatchResponse, Status>>(8);
            tokio::spawn(async move {
                loop {
                    match input.message().await {
                        Ok(Some(request)) => {
                            if send_etcd_watch_response(
                                &tx,
                                &kernel,
                                &auth,
                                &workspace,
                                &collection,
                                request,
                            )
                            .await
                            .is_err()
                            {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(err) => {
                            let _ = tx.send(Err(err)).await;
                            break;
                        }
                    }
                }
            });
            Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
                rx,
            )))
        }
    }

    #[derive(Clone)]
    pub struct EtcdKvServer<T> {
        inner: Arc<T>,
    }

    impl<T> EtcdKvServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for EtcdKvServer<T>
    where
        T: EtcdKv,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/etcdserverpb.KV/Range" => {
                    struct RangeSvc<T>(Arc<T>);
                    impl<T: EtcdKv> UnaryService<EtcdRangeRequest> for RangeSvc<T> {
                        type Response = EtcdRangeResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<EtcdRangeRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.range(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = RangeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/etcdserverpb.KV/Put" => {
                    struct PutSvc<T>(Arc<T>);
                    impl<T: EtcdKv> UnaryService<EtcdPutRequest> for PutSvc<T> {
                        type Response = EtcdPutResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<EtcdPutRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.put(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = PutSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/etcdserverpb.KV/DeleteRange" => {
                    struct DeleteRangeSvc<T>(Arc<T>);
                    impl<T: EtcdKv> UnaryService<EtcdDeleteRangeRequest> for DeleteRangeSvc<T> {
                        type Response = EtcdDeleteRangeResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<EtcdDeleteRangeRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.delete_range(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = DeleteRangeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/etcdserverpb.KV/Compact" => {
                    struct CompactSvc<T>(Arc<T>);
                    impl<T: EtcdKv> UnaryService<EtcdCompactionRequest> for CompactSvc<T> {
                        type Response = EtcdCompactionResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<EtcdCompactionRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.compact(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = CompactSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/etcdserverpb.KV/Txn" => {
                    struct TxnSvc<T>(Arc<T>);
                    impl<T: EtcdKv> UnaryService<EtcdTxnRequest> for TxnSvc<T> {
                        type Response = EtcdTxnResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<EtcdTxnRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.txn(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = TxnSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: EtcdKv> NamedService for EtcdKvServer<T> {
        const NAME: &'static str = "etcdserverpb.KV";
    }

    #[derive(Clone)]
    pub struct EtcdLeaseServer<T> {
        inner: Arc<T>,
    }

    impl<T> EtcdLeaseServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for EtcdLeaseServer<T>
    where
        T: EtcdLease,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/etcdserverpb.Lease/LeaseGrant" => {
                    struct GrantSvc<T>(Arc<T>);
                    impl<T: EtcdLease> UnaryService<EtcdLeaseGrantRequest> for GrantSvc<T> {
                        type Response = EtcdLeaseGrantResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<EtcdLeaseGrantRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.lease_grant(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = GrantSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/etcdserverpb.Lease/LeaseRevoke" => {
                    struct RevokeSvc<T>(Arc<T>);
                    impl<T: EtcdLease> UnaryService<EtcdLeaseRevokeRequest> for RevokeSvc<T> {
                        type Response = EtcdLeaseRevokeResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<EtcdLeaseRevokeRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.lease_revoke(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = RevokeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/etcdserverpb.Lease/LeaseKeepAliveOnce" => {
                    struct KeepAliveOnceSvc<T>(Arc<T>);
                    impl<T: EtcdLease> UnaryService<EtcdLeaseKeepAliveRequest> for KeepAliveOnceSvc<T> {
                        type Response = EtcdLeaseKeepAliveResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<EtcdLeaseKeepAliveRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.lease_keep_alive_once(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = KeepAliveOnceSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: EtcdLease> NamedService for EtcdLeaseServer<T> {
        const NAME: &'static str = "etcdserverpb.Lease";
    }

    #[derive(Clone)]
    pub struct EtcdWatchServer<T> {
        inner: Arc<T>,
    }

    impl<T> EtcdWatchServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for EtcdWatchServer<T>
    where
        T: EtcdWatch,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/etcdserverpb.Watch/Watch" => {
                    struct WatchSvc<T>(Arc<T>);
                    impl<T: EtcdWatch> StreamingService<EtcdWatchRequest> for WatchSvc<T> {
                        type Response = EtcdWatchResponse;
                        type ResponseStream = T::WatchStream;
                        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

                        fn call(
                            &mut self,
                            request: Request<tonic::Streaming<EtcdWatchRequest>>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.watch(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = WatchSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.streaming(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: EtcdWatch> NamedService for EtcdWatchServer<T> {
        const NAME: &'static str = "etcdserverpb.Watch";
    }

    macro_rules! etcd_unsupported_server {
        ($server:ident, $name:literal) => {
            #[derive(Clone)]
            pub struct $server;

            impl Service<http::Request<Body>> for $server {
                type Response = http::Response<Body>;
                type Error = Infallible;
                type Future = BoxFuture<Self::Response, Self::Error>;

                fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                    Poll::Ready(Ok(()))
                }

                fn call(&mut self, _req: http::Request<Body>) -> Self::Future {
                    Box::pin(async move { Ok(grpc_unimplemented_response()) })
                }
            }

            impl NamedService for $server {
                const NAME: &'static str = $name;
            }
        };
    }

    etcd_unsupported_server!(EtcdClusterServer, "etcdserverpb.Cluster");
    etcd_unsupported_server!(EtcdAuthServer, "etcdserverpb.Auth");
    etcd_unsupported_server!(EtcdMaintenanceServer, "etcdserverpb.Maintenance");

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantCollectionRequest {
        #[prost(string, tag = "1")]
        pub collection_name: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantVectorParams {
        #[prost(uint64, tag = "1")]
        pub size: u64,
        #[prost(string, tag = "2")]
        pub distance: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantNamedVectorParams {
        #[prost(string, tag = "1")]
        pub name: String,
        #[prost(message, optional, tag = "2")]
        pub params: Option<QdrantVectorParams>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantCreateCollectionRequest {
        #[prost(string, tag = "1")]
        pub collection_name: String,
        #[prost(message, optional, tag = "2")]
        pub vectors: Option<QdrantVectorParams>,
        #[prost(message, repeated, tag = "3")]
        pub named_vectors: Vec<QdrantNamedVectorParams>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantCollectionInfoResponse {
        #[prost(bool, tag = "1")]
        pub found: bool,
        #[prost(uint64, tag = "2")]
        pub vector_size: u64,
        #[prost(string, tag = "3")]
        pub distance: String,
        #[prost(uint64, tag = "4")]
        pub points_count: u64,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantOperationResponse {
        #[prost(bool, tag = "1")]
        pub ok: bool,
        #[prost(string, tag = "2")]
        pub status: String,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantPointId {
        #[prost(oneof = "qdrant_point_id::PointIdOptions", tags = "1, 2")]
        pub point_id_options: Option<qdrant_point_id::PointIdOptions>,
    }

    pub mod qdrant_point_id {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum PointIdOptions {
            #[prost(uint64, tag = "1")]
            Num(u64),
            #[prost(string, tag = "2")]
            Uuid(String),
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantPayloadEntry {
        #[prost(string, tag = "1")]
        pub key: String,
        #[prost(message, optional, tag = "2")]
        pub value: Option<QdrantValue>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantValue {
        #[prost(oneof = "qdrant_value::Kind", tags = "1, 2, 3, 4, 5")]
        pub kind: Option<qdrant_value::Kind>,
    }

    pub mod qdrant_value {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum Kind {
            #[prost(bool, tag = "1")]
            NullValue(bool),
            #[prost(bool, tag = "2")]
            BoolValue(bool),
            #[prost(int64, tag = "3")]
            IntegerValue(i64),
            #[prost(double, tag = "4")]
            DoubleValue(f64),
            #[prost(string, tag = "5")]
            StringValue(String),
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantNamedVector {
        #[prost(string, tag = "1")]
        pub name: String,
        #[prost(float, repeated, tag = "2")]
        pub data: Vec<f32>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantPointStruct {
        #[prost(message, optional, tag = "1")]
        pub id: Option<QdrantPointId>,
        #[prost(float, repeated, tag = "2")]
        pub vector: Vec<f32>,
        #[prost(message, repeated, tag = "3")]
        pub named_vectors: Vec<QdrantNamedVector>,
        #[prost(message, repeated, tag = "4")]
        pub payload: Vec<QdrantPayloadEntry>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantUpsertPointsRequest {
        #[prost(string, tag = "1")]
        pub collection_name: String,
        #[prost(message, repeated, tag = "2")]
        pub points: Vec<QdrantPointStruct>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantPointIdsRequest {
        #[prost(string, tag = "1")]
        pub collection_name: String,
        #[prost(message, repeated, tag = "2")]
        pub ids: Vec<QdrantPointId>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantGetPointsRequest {
        #[prost(string, tag = "1")]
        pub collection_name: String,
        #[prost(message, repeated, tag = "2")]
        pub ids: Vec<QdrantPointId>,
        #[prost(bool, tag = "3")]
        pub with_payload: bool,
        #[prost(bool, tag = "4")]
        pub with_vectors: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantPointResult {
        #[prost(message, optional, tag = "1")]
        pub id: Option<QdrantPointId>,
        #[prost(float, repeated, tag = "2")]
        pub vector: Vec<f32>,
        #[prost(message, repeated, tag = "3")]
        pub payload: Vec<QdrantPayloadEntry>,
        #[prost(float, tag = "4")]
        pub score: f32,
        #[prost(bool, tag = "5")]
        pub found: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantPointsResponse {
        #[prost(message, repeated, tag = "1")]
        pub result: Vec<QdrantPointResult>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantFilter {
        #[prost(message, repeated, tag = "1")]
        pub must: Vec<QdrantCondition>,
        #[prost(message, repeated, tag = "2")]
        pub should: Vec<QdrantCondition>,
        #[prost(message, repeated, tag = "3")]
        pub must_not: Vec<QdrantCondition>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantCondition {
        #[prost(string, tag = "1")]
        pub key: String,
        #[prost(oneof = "qdrant_condition::Kind", tags = "2, 3, 4, 5, 6")]
        pub kind: Option<qdrant_condition::Kind>,
    }

    pub mod qdrant_condition {
        #[derive(Clone, PartialEq, prost::Oneof)]
        pub enum Kind {
            #[prost(message, tag = "2")]
            Match(super::QdrantMatch),
            #[prost(message, tag = "3")]
            Range(super::QdrantRange),
            #[prost(bool, tag = "4")]
            IsEmpty(bool),
            #[prost(bool, tag = "5")]
            IsNull(bool),
            #[prost(string, tag = "6")]
            Unsupported(String),
        }
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantMatch {
        #[prost(message, optional, tag = "1")]
        pub value: Option<QdrantValue>,
        #[prost(message, repeated, tag = "2")]
        pub any: Vec<QdrantValue>,
        #[prost(message, repeated, tag = "3")]
        pub except: Vec<QdrantValue>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantRange {
        #[prost(message, optional, tag = "1")]
        pub gt: Option<QdrantValue>,
        #[prost(message, optional, tag = "2")]
        pub gte: Option<QdrantValue>,
        #[prost(message, optional, tag = "3")]
        pub lt: Option<QdrantValue>,
        #[prost(message, optional, tag = "4")]
        pub lte: Option<QdrantValue>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantSearchPointsRequest {
        #[prost(string, tag = "1")]
        pub collection_name: String,
        #[prost(float, repeated, tag = "2")]
        pub vector: Vec<f32>,
        #[prost(string, tag = "3")]
        pub vector_name: String,
        #[prost(message, optional, tag = "4")]
        pub filter: Option<QdrantFilter>,
        #[prost(uint64, tag = "5")]
        pub limit: u64,
        #[prost(bool, tag = "6")]
        pub with_payload: bool,
        #[prost(bool, tag = "7")]
        pub with_vectors: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantScrollPointsRequest {
        #[prost(string, tag = "1")]
        pub collection_name: String,
        #[prost(message, optional, tag = "2")]
        pub filter: Option<QdrantFilter>,
        #[prost(uint64, tag = "3")]
        pub limit: u64,
        #[prost(message, optional, tag = "4")]
        pub offset: Option<QdrantPointId>,
        #[prost(bool, tag = "5")]
        pub with_payload: bool,
        #[prost(bool, tag = "6")]
        pub with_vectors: bool,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantScrollPointsResponse {
        #[prost(message, repeated, tag = "1")]
        pub points: Vec<QdrantPointResult>,
        #[prost(message, optional, tag = "2")]
        pub next_page_offset: Option<QdrantPointId>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantCountPointsRequest {
        #[prost(string, tag = "1")]
        pub collection_name: String,
        #[prost(message, optional, tag = "2")]
        pub filter: Option<QdrantFilter>,
    }

    #[derive(Clone, PartialEq, prost::Message)]
    pub struct QdrantCountPointsResponse {
        #[prost(uint64, tag = "1")]
        pub count: u64,
    }

    #[tonic::async_trait]
    pub trait QdrantCollections: Send + Sync + 'static {
        async fn create(
            &self,
            request: Request<QdrantCreateCollectionRequest>,
        ) -> Result<Response<QdrantOperationResponse>, Status>;

        async fn get(
            &self,
            request: Request<QdrantCollectionRequest>,
        ) -> Result<Response<QdrantCollectionInfoResponse>, Status>;
    }

    #[tonic::async_trait]
    pub trait QdrantPoints: Send + Sync + 'static {
        async fn upsert(
            &self,
            request: Request<QdrantUpsertPointsRequest>,
        ) -> Result<Response<QdrantOperationResponse>, Status>;

        async fn get(
            &self,
            request: Request<QdrantGetPointsRequest>,
        ) -> Result<Response<QdrantPointsResponse>, Status>;

        async fn delete(
            &self,
            request: Request<QdrantPointIdsRequest>,
        ) -> Result<Response<QdrantOperationResponse>, Status>;

        async fn search(
            &self,
            request: Request<QdrantSearchPointsRequest>,
        ) -> Result<Response<QdrantPointsResponse>, Status>;

        async fn query(
            &self,
            request: Request<QdrantSearchPointsRequest>,
        ) -> Result<Response<QdrantPointsResponse>, Status>;

        async fn scroll(
            &self,
            request: Request<QdrantScrollPointsRequest>,
        ) -> Result<Response<QdrantScrollPointsResponse>, Status>;

        async fn count(
            &self,
            request: Request<QdrantCountPointsRequest>,
        ) -> Result<Response<QdrantCountPointsResponse>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedQdrantCollectionsGrpcService {
        kernel: HostedKernel,
        workspace: String,
        collection: String,
    }

    impl HostedQdrantCollectionsGrpcService {
        pub fn new(
            kernel: HostedKernel,
            workspace: impl Into<String>,
            collection: impl Into<String>,
        ) -> Self {
            Self {
                kernel,
                workspace: workspace.into(),
                collection: collection.into(),
            }
        }
    }

    #[tonic::async_trait]
    impl QdrantCollections for HostedQdrantCollectionsGrpcService {
        async fn create(
            &self,
            request: Request<QdrantCreateCollectionRequest>,
        ) -> Result<Response<QdrantOperationResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            ensure_qdrant_collection(&self.collection, &input.collection_name)?;
            let mut specs = Vec::new();
            if let Some(params) = input.vectors {
                specs.push((None, params));
            }
            for named in input.named_vectors {
                let params = named
                    .params
                    .ok_or_else(|| Status::invalid_argument("named vector params are required"))?;
                if named.name.is_empty() {
                    return Err(Status::invalid_argument("named vector must not be empty"));
                }
                specs.push((Some(named.name), params));
            }
            if specs.is_empty() {
                return Err(Status::invalid_argument("collection vectors are required"));
            }
            for (name, params) in specs {
                let mapping =
                    crate::vector_compat::qdrant_mapping(&input.collection_name, name.as_deref())
                        .map_err(status_from_loom_error)?;
                let dim = usize::try_from(params.size)
                    .map_err(|_| Status::invalid_argument("vector size is too large"))?;
                let metric = qdrant_metric(&params.distance)?;
                self.kernel
                    .data()
                    .vector_create(&auth, &self.workspace, &mapping.vector_set, dim, metric)
                    .map_err(status_from_hosted_error)?;
            }
            Ok(Response::new(QdrantOperationResponse {
                ok: true,
                status: "completed".to_string(),
            }))
        }

        async fn get(
            &self,
            request: Request<QdrantCollectionRequest>,
        ) -> Result<Response<QdrantCollectionInfoResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            ensure_qdrant_collection(&self.collection, &input.collection_name)?;
            let mapping = crate::vector_compat::qdrant_mapping(&input.collection_name, None)
                .map_err(status_from_loom_error)?;
            let info = self
                .kernel
                .data()
                .vector_info(&auth, &self.workspace, &mapping.vector_set)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(QdrantCollectionInfoResponse {
                found: true,
                vector_size: info.dim as u64,
                distance: qdrant_metric_name(info.metric).to_string(),
                points_count: info.count as u64,
            }))
        }
    }

    #[derive(Clone)]
    pub struct HostedQdrantPointsGrpcService {
        kernel: HostedKernel,
        workspace: String,
        collection: String,
    }

    impl HostedQdrantPointsGrpcService {
        pub fn new(
            kernel: HostedKernel,
            workspace: impl Into<String>,
            collection: impl Into<String>,
        ) -> Self {
            Self {
                kernel,
                workspace: workspace.into(),
                collection: collection.into(),
            }
        }
    }

    #[tonic::async_trait]
    impl QdrantPoints for HostedQdrantPointsGrpcService {
        async fn upsert(
            &self,
            request: Request<QdrantUpsertPointsRequest>,
        ) -> Result<Response<QdrantOperationResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            ensure_qdrant_collection(&self.collection, &input.collection_name)?;
            for point in input.points {
                let id = qdrant_point_id_string(point.id.as_ref())?;
                let vectors = qdrant_point_vectors(&point)?;
                let payload = qdrant_payload(point.payload)?;
                for (name, vector) in vectors {
                    let mapping = crate::vector_compat::qdrant_mapping(
                        &input.collection_name,
                        name.as_deref(),
                    )
                    .map_err(status_from_loom_error)?;
                    self.kernel
                        .data()
                        .vector_upsert(
                            &auth,
                            &self.workspace,
                            &mapping.vector_set,
                            &id,
                            vector,
                            payload.clone(),
                        )
                        .map_err(status_from_hosted_error)?;
                }
            }
            Ok(Response::new(QdrantOperationResponse {
                ok: true,
                status: "completed".to_string(),
            }))
        }

        async fn get(
            &self,
            request: Request<QdrantGetPointsRequest>,
        ) -> Result<Response<QdrantPointsResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            ensure_qdrant_collection(&self.collection, &input.collection_name)?;
            let mapping = crate::vector_compat::qdrant_mapping(&input.collection_name, None)
                .map_err(status_from_loom_error)?;
            let mut result = Vec::new();
            for id in input.ids {
                let id_string = qdrant_point_id_string(Some(&id))?;
                if let Some(entry) = self
                    .kernel
                    .data()
                    .vector_get(&auth, &self.workspace, &mapping.vector_set, &id_string)
                    .map_err(status_from_hosted_error)?
                {
                    result.push(qdrant_point_result(
                        id_string,
                        entry,
                        input.with_payload,
                        input.with_vectors,
                        None,
                    ));
                }
            }
            Ok(Response::new(QdrantPointsResponse { result }))
        }

        async fn delete(
            &self,
            request: Request<QdrantPointIdsRequest>,
        ) -> Result<Response<QdrantOperationResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            ensure_qdrant_collection(&self.collection, &input.collection_name)?;
            let mapping = crate::vector_compat::qdrant_mapping(&input.collection_name, None)
                .map_err(status_from_loom_error)?;
            for id in input.ids {
                let id = qdrant_point_id_string(Some(&id))?;
                self.kernel
                    .data()
                    .vector_delete(&auth, &self.workspace, &mapping.vector_set, &id)
                    .map_err(status_from_hosted_error)?;
            }
            Ok(Response::new(QdrantOperationResponse {
                ok: true,
                status: "completed".to_string(),
            }))
        }

        async fn search(
            &self,
            request: Request<QdrantSearchPointsRequest>,
        ) -> Result<Response<QdrantPointsResponse>, Status> {
            self.search_like(request).await
        }

        async fn query(
            &self,
            request: Request<QdrantSearchPointsRequest>,
        ) -> Result<Response<QdrantPointsResponse>, Status> {
            self.search_like(request).await
        }

        async fn scroll(
            &self,
            request: Request<QdrantScrollPointsRequest>,
        ) -> Result<Response<QdrantScrollPointsResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            ensure_qdrant_collection(&self.collection, &input.collection_name)?;
            let mapping = crate::vector_compat::qdrant_mapping(&input.collection_name, None)
                .map_err(status_from_loom_error)?;
            let filter = qdrant_filter_from_proto(input.filter.as_ref())?;
            let limit = qdrant_limit(input.limit)?;
            let offset = input
                .offset
                .as_ref()
                .map(|id| qdrant_point_id_string(Some(id)))
                .transpose()?;
            let points = self
                .kernel
                .data()
                .vector_scroll(
                    &auth,
                    &self.workspace,
                    &mapping.vector_set,
                    offset.as_deref(),
                    limit,
                    &filter,
                )
                .map_err(status_from_hosted_error)?;
            let next_page_offset = if points.len() == limit {
                points.last().map(|(id, _)| qdrant_point_id(id))
            } else {
                None
            };
            Ok(Response::new(QdrantScrollPointsResponse {
                points: points
                    .into_iter()
                    .map(|(id, entry)| {
                        qdrant_point_result(id, entry, input.with_payload, input.with_vectors, None)
                    })
                    .collect(),
                next_page_offset,
            }))
        }

        async fn count(
            &self,
            request: Request<QdrantCountPointsRequest>,
        ) -> Result<Response<QdrantCountPointsResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            ensure_qdrant_collection(&self.collection, &input.collection_name)?;
            let mapping = crate::vector_compat::qdrant_mapping(&input.collection_name, None)
                .map_err(status_from_loom_error)?;
            let filter = qdrant_filter_from_proto(input.filter.as_ref())?;
            let count = self
                .kernel
                .data()
                .vector_count(&auth, &self.workspace, &mapping.vector_set, &filter)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(QdrantCountPointsResponse {
                count: count as u64,
            }))
        }
    }

    impl HostedQdrantPointsGrpcService {
        async fn search_like(
            &self,
            request: Request<QdrantSearchPointsRequest>,
        ) -> Result<Response<QdrantPointsResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            ensure_qdrant_collection(&self.collection, &input.collection_name)?;
            let vector_name = if input.vector_name.is_empty() {
                None
            } else {
                Some(input.vector_name.as_str())
            };
            let mapping = crate::vector_compat::qdrant_mapping(&input.collection_name, vector_name)
                .map_err(status_from_loom_error)?;
            let filter = qdrant_filter_from_proto(input.filter.as_ref())?;
            let limit = qdrant_limit(input.limit)?;
            let hits = self
                .kernel
                .data()
                .vector_search_filtered(
                    &auth,
                    &self.workspace,
                    &mapping.vector_set,
                    &input.vector,
                    limit,
                    &filter,
                )
                .map_err(status_from_hosted_error)?;
            let mut result = Vec::with_capacity(hits.len());
            for hit in hits {
                if let Some(entry) = self
                    .kernel
                    .data()
                    .vector_get(&auth, &self.workspace, &mapping.vector_set, &hit.id)
                    .map_err(status_from_hosted_error)?
                {
                    result.push(qdrant_point_result(
                        hit.id,
                        entry,
                        input.with_payload,
                        input.with_vectors,
                        Some(hit.score),
                    ));
                }
            }
            Ok(Response::new(QdrantPointsResponse { result }))
        }
    }

    #[derive(Clone)]
    pub struct QdrantCollectionsServer<T> {
        inner: Arc<T>,
    }

    impl<T> QdrantCollectionsServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for QdrantCollectionsServer<T>
    where
        T: QdrantCollections,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/qdrant.Collections/Create" => {
                    struct CreateSvc<T>(Arc<T>);
                    impl<T: QdrantCollections> UnaryService<QdrantCreateCollectionRequest> for CreateSvc<T> {
                        type Response = QdrantOperationResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<QdrantCreateCollectionRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.create(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = CreateSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/qdrant.Collections/Get" => {
                    struct GetSvc<T>(Arc<T>);
                    impl<T: QdrantCollections> UnaryService<QdrantCollectionRequest> for GetSvc<T> {
                        type Response = QdrantCollectionInfoResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<QdrantCollectionRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.get(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = GetSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: QdrantCollections> NamedService for QdrantCollectionsServer<T> {
        const NAME: &'static str = "qdrant.Collections";
    }

    #[derive(Clone)]
    pub struct QdrantPointsServer<T> {
        inner: Arc<T>,
    }

    impl<T> QdrantPointsServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for QdrantPointsServer<T>
    where
        T: QdrantPoints,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/qdrant.Points/Upsert" => {
                    struct UpsertSvc<T>(Arc<T>);
                    impl<T: QdrantPoints> UnaryService<QdrantUpsertPointsRequest> for UpsertSvc<T> {
                        type Response = QdrantOperationResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<QdrantUpsertPointsRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.upsert(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = UpsertSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/qdrant.Points/Get" => {
                    struct GetSvc<T>(Arc<T>);
                    impl<T: QdrantPoints> UnaryService<QdrantGetPointsRequest> for GetSvc<T> {
                        type Response = QdrantPointsResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<QdrantGetPointsRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.get(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = GetSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/qdrant.Points/Delete" => {
                    struct DeleteSvc<T>(Arc<T>);
                    impl<T: QdrantPoints> UnaryService<QdrantPointIdsRequest> for DeleteSvc<T> {
                        type Response = QdrantOperationResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<QdrantPointIdsRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.delete(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = DeleteSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/qdrant.Points/Search" => {
                    struct SearchSvc<T>(Arc<T>);
                    impl<T: QdrantPoints> UnaryService<QdrantSearchPointsRequest> for SearchSvc<T> {
                        type Response = QdrantPointsResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<QdrantSearchPointsRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.search(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = SearchSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/qdrant.Points/Query" => {
                    struct QuerySvc<T>(Arc<T>);
                    impl<T: QdrantPoints> UnaryService<QdrantSearchPointsRequest> for QuerySvc<T> {
                        type Response = QdrantPointsResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<QdrantSearchPointsRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.query(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = QuerySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/qdrant.Points/Scroll" => {
                    struct ScrollSvc<T>(Arc<T>);
                    impl<T: QdrantPoints> UnaryService<QdrantScrollPointsRequest> for ScrollSvc<T> {
                        type Response = QdrantScrollPointsResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<QdrantScrollPointsRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.scroll(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ScrollSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/qdrant.Points/Count" => {
                    struct CountSvc<T>(Arc<T>);
                    impl<T: QdrantPoints> UnaryService<QdrantCountPointsRequest> for CountSvc<T> {
                        type Response = QdrantCountPointsResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<QdrantCountPointsRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.count(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = CountSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: QdrantPoints> NamedService for QdrantPointsServer<T> {
        const NAME: &'static str = "qdrant.Points";
    }

    #[tonic::async_trait]
    pub trait Cas: Send + Sync + 'static {
        async fn put(
            &self,
            request: Request<CasPutRequest>,
        ) -> Result<Response<CasDigestResponse>, Status>;

        async fn get(
            &self,
            request: Request<CasDigestRequest>,
        ) -> Result<Response<CasGetResponse>, Status>;

        async fn has(
            &self,
            request: Request<CasDigestRequest>,
        ) -> Result<Response<CasHasResponse>, Status>;

        async fn list(
            &self,
            request: Request<CasListRequest>,
        ) -> Result<Response<CasListResponse>, Status>;

        async fn delete(
            &self,
            request: Request<CasDigestRequest>,
        ) -> Result<Response<CasDeleteResponse>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedCasGrpcService {
        kernel: HostedKernel,
        workspace: WorkspaceId,
    }

    impl HostedCasGrpcService {
        pub fn new(kernel: HostedKernel, workspace: WorkspaceId) -> Self {
            Self { kernel, workspace }
        }
    }

    #[tonic::async_trait]
    impl Cas for HostedCasGrpcService {
        async fn put(
            &self,
            request: Request<CasPutRequest>,
        ) -> Result<Response<CasDigestResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .grpc()
                .put_cas(&auth, self.workspace, &request.into_inner().bytes)
                .map(|out| {
                    Response::new(CasDigestResponse {
                        digest: out.message,
                    })
                })
                .map_err(status_from_failure)
        }

        async fn get(
            &self,
            request: Request<CasDigestRequest>,
        ) -> Result<Response<CasGetResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let digest = parse_digest(request.into_inner().digest)?;
            self.kernel
                .grpc()
                .get_cas(&auth, self.workspace, &digest)
                .map(|out| {
                    let found = out.message.is_some();
                    let bytes = out.message.unwrap_or_default();
                    Response::new(CasGetResponse { found, bytes })
                })
                .map_err(status_from_failure)
        }

        async fn has(
            &self,
            request: Request<CasDigestRequest>,
        ) -> Result<Response<CasHasResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let digest = parse_digest(request.into_inner().digest)?;
            self.kernel
                .grpc()
                .has_cas(&auth, self.workspace, &digest)
                .map(|out| {
                    Response::new(CasHasResponse {
                        present: out.message,
                    })
                })
                .map_err(status_from_failure)
        }

        async fn list(
            &self,
            request: Request<CasListRequest>,
        ) -> Result<Response<CasListResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .grpc()
                .list_cas(&auth, self.workspace)
                .map(|out| {
                    Response::new(CasListResponse {
                        digests: out.message,
                    })
                })
                .map_err(status_from_failure)
        }

        async fn delete(
            &self,
            request: Request<CasDigestRequest>,
        ) -> Result<Response<CasDeleteResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let digest = parse_digest(request.into_inner().digest)?;
            self.kernel
                .grpc()
                .delete_cas(&auth, self.workspace, &digest)
                .map(|out| {
                    Response::new(CasDeleteResponse {
                        deleted: out.message,
                    })
                })
                .map_err(status_from_failure)
        }
    }

    #[derive(Clone)]
    pub struct CasServer<T> {
        inner: Arc<T>,
    }

    impl<T> CasServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for CasServer<T>
    where
        T: Cas,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/loom.hosted.v1.Cas/Put" => {
                    struct PutSvc<T>(Arc<T>);
                    impl<T: Cas> UnaryService<CasPutRequest> for PutSvc<T> {
                        type Response = CasDigestResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<CasPutRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.put(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = PutSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Cas/Get" => {
                    struct GetSvc<T>(Arc<T>);
                    impl<T: Cas> UnaryService<CasDigestRequest> for GetSvc<T> {
                        type Response = CasGetResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<CasDigestRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.get(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = GetSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Cas/Has" => {
                    struct HasSvc<T>(Arc<T>);
                    impl<T: Cas> UnaryService<CasDigestRequest> for HasSvc<T> {
                        type Response = CasHasResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<CasDigestRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.has(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = HasSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Cas/List" => {
                    struct ListSvc<T>(Arc<T>);
                    impl<T: Cas> UnaryService<CasListRequest> for ListSvc<T> {
                        type Response = CasListResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<CasListRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.list(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ListSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Cas/Delete" => {
                    struct DeleteSvc<T>(Arc<T>);
                    impl<T: Cas> UnaryService<CasDigestRequest> for DeleteSvc<T> {
                        type Response = CasDeleteResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<CasDigestRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.delete(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = DeleteSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move {
                    let mut response = http::Response::new(Body::empty());
                    response
                        .headers_mut()
                        .insert("grpc-status", http::HeaderValue::from_static("12"));
                    response.headers_mut().insert(
                        "content-type",
                        http::HeaderValue::from_static("application/grpc"),
                    );
                    Ok(response)
                }),
            }
        }
    }

    impl<T: Cas> NamedService for CasServer<T> {
        const NAME: &'static str = "loom.hosted.v1.Cas";
    }

    #[tonic::async_trait]
    pub trait Files: Send + Sync + 'static {
        async fn read(
            &self,
            request: Request<FilesPathRequest>,
        ) -> Result<Response<FilesReadResponse>, Status>;

        async fn write(
            &self,
            request: Request<FilesWriteRequest>,
        ) -> Result<Response<FilesEmptyResponse>, Status>;

        async fn stat(
            &self,
            request: Request<FilesPathRequest>,
        ) -> Result<Response<FilesStatResponse>, Status>;

        type ListStream: Stream<Item = Result<FilesListResponse, Status>> + Send + 'static;

        async fn list(
            &self,
            request: Request<FilesListRequest>,
        ) -> Result<Response<Self::ListStream>, Status>;

        async fn mkdir(
            &self,
            request: Request<FilesMkdirRequest>,
        ) -> Result<Response<FilesEmptyResponse>, Status>;

        async fn delete(
            &self,
            request: Request<FilesDeleteRequest>,
        ) -> Result<Response<FilesEmptyResponse>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedFilesGrpcService {
        kernel: HostedKernel,
        workspace: WorkspaceId,
    }

    impl HostedFilesGrpcService {
        pub fn new(kernel: HostedKernel, workspace: WorkspaceId) -> Self {
            Self { kernel, workspace }
        }
    }

    #[tonic::async_trait]
    impl Files for HostedFilesGrpcService {
        async fn read(
            &self,
            request: Request<FilesPathRequest>,
        ) -> Result<Response<FilesReadResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .grpc()
                .read_file(&auth, self.workspace, &request.into_inner().path)
                .map(|out| Response::new(FilesReadResponse { bytes: out.message }))
                .map_err(status_from_failure)
        }

        async fn write(
            &self,
            request: Request<FilesWriteRequest>,
        ) -> Result<Response<FilesEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            self.kernel
                .grpc()
                .write_file(&auth, self.workspace, &input.path, &input.bytes)
                .map(|_| Response::new(FilesEmptyResponse {}))
                .map_err(status_from_failure)
        }

        async fn stat(
            &self,
            request: Request<FilesPathRequest>,
        ) -> Result<Response<FilesStatResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .grpc()
                .stat_file(&auth, self.workspace, &request.into_inner().path)
                .map(|out| Response::new(files_stat_to_proto(out.message)))
                .map_err(status_from_failure)
        }

        type ListStream = tokio_stream::wrappers::ReceiverStream<Result<FilesListResponse, Status>>;

        async fn list(
            &self,
            request: Request<FilesListRequest>,
        ) -> Result<Response<Self::ListStream>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let batch_size = input.batch_size.clamp(1, 1024) as usize;
            let entries = self
                .kernel
                .grpc()
                .list_directory(&auth, self.workspace, &input.path)
                .map_err(status_from_failure)?
                .message;
            let (tx, rx) = mpsc::channel::<Result<FilesListResponse, Status>>(8);
            tokio::spawn(async move {
                for chunk in entries.chunks(batch_size) {
                    let response = FilesListResponse {
                        entries: chunk.iter().cloned().map(files_entry_to_proto).collect(),
                    };
                    if tx.send(Ok(response)).await.is_err() {
                        break;
                    }
                }
            });
            Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
                rx,
            )))
        }

        async fn mkdir(
            &self,
            request: Request<FilesMkdirRequest>,
        ) -> Result<Response<FilesEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            self.kernel
                .grpc()
                .create_directory(&auth, self.workspace, &input.path, input.recursive)
                .map(|_| Response::new(FilesEmptyResponse {}))
                .map_err(status_from_failure)
        }

        async fn delete(
            &self,
            request: Request<FilesDeleteRequest>,
        ) -> Result<Response<FilesEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            self.kernel
                .grpc()
                .delete_path(&auth, self.workspace, &input.path, input.recursive)
                .map(|_| Response::new(FilesEmptyResponse {}))
                .map_err(status_from_failure)
        }
    }

    fn files_stat_to_proto(stat: loom_core::Stat) -> FilesStatResponse {
        FilesStatResponse {
            path: stat.path,
            kind: file_kind_to_proto(stat.kind).to_string(),
            size: stat.size,
            mode: stat.mode,
        }
    }

    fn files_entry_to_proto(entry: loom_core::DirEntry) -> FilesListEntry {
        FilesListEntry {
            name: entry.name,
            kind: file_kind_to_proto(entry.kind).to_string(),
        }
    }

    fn file_kind_to_proto(kind: loom_core::FileKind) -> &'static str {
        match kind {
            loom_core::FileKind::File => "file",
            loom_core::FileKind::Directory => "directory",
            loom_core::FileKind::Symlink => "symlink",
        }
    }

    #[derive(Clone)]
    pub struct FilesServer<T> {
        inner: Arc<T>,
    }

    impl<T> FilesServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for FilesServer<T>
    where
        T: Files,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/loom.hosted.v1.Files/Read" => {
                    struct ReadSvc<T>(Arc<T>);
                    impl<T: Files> UnaryService<FilesPathRequest> for ReadSvc<T> {
                        type Response = FilesReadResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<FilesPathRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.read(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ReadSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Files/Write" => {
                    struct WriteSvc<T>(Arc<T>);
                    impl<T: Files> UnaryService<FilesWriteRequest> for WriteSvc<T> {
                        type Response = FilesEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<FilesWriteRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.write(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = WriteSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Files/Stat" => {
                    struct StatSvc<T>(Arc<T>);
                    impl<T: Files> UnaryService<FilesPathRequest> for StatSvc<T> {
                        type Response = FilesStatResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<FilesPathRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.stat(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = StatSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Files/List" => {
                    struct ListSvc<T>(Arc<T>);
                    impl<T: Files> ServerStreamingService<FilesListRequest> for ListSvc<T> {
                        type Response = FilesListResponse;
                        type ResponseStream = T::ListStream;
                        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

                        fn call(&mut self, request: Request<FilesListRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.list(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ListSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.server_streaming(method, req).await)
                    })
                }
                "/loom.hosted.v1.Files/Mkdir" => {
                    struct MkdirSvc<T>(Arc<T>);
                    impl<T: Files> UnaryService<FilesMkdirRequest> for MkdirSvc<T> {
                        type Response = FilesEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<FilesMkdirRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.mkdir(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = MkdirSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Files/Delete" => {
                    struct DeleteSvc<T>(Arc<T>);
                    impl<T: Files> UnaryService<FilesDeleteRequest> for DeleteSvc<T> {
                        type Response = FilesEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<FilesDeleteRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.delete(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = DeleteSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: Files> NamedService for FilesServer<T> {
        const NAME: &'static str = "loom.hosted.v1.Files";
    }

    #[tonic::async_trait]
    pub trait Vcs: Send + Sync + 'static {
        async fn commit(
            &self,
            request: Request<VcsCommitRequest>,
        ) -> Result<Response<VcsCommitResponse>, Status>;

        type LogStream: Stream<Item = Result<VcsLogResponse, Status>> + Send + 'static;

        async fn log(
            &self,
            request: Request<VcsLogRequest>,
        ) -> Result<Response<Self::LogStream>, Status>;

        async fn branch(
            &self,
            request: Request<VcsBranchRequest>,
        ) -> Result<Response<VcsEmptyResponse>, Status>;

        async fn checkout(
            &self,
            request: Request<VcsCheckoutRequest>,
        ) -> Result<Response<VcsEmptyResponse>, Status>;

        async fn diff(
            &self,
            request: Request<VcsDiffRequest>,
        ) -> Result<Response<VcsDiffResponse>, Status>;

        async fn merge(
            &self,
            request: Request<VcsMergeRequest>,
        ) -> Result<Response<VcsMergeResponse>, Status>;

        async fn status(
            &self,
            request: Request<VcsStatusRequest>,
        ) -> Result<Response<VcsStatusResponse>, Status>;

        async fn stage(
            &self,
            request: Request<VcsPathsRequest>,
        ) -> Result<Response<VcsEmptyResponse>, Status>;

        async fn stage_all(
            &self,
            request: Request<VcsStatusRequest>,
        ) -> Result<Response<VcsEmptyResponse>, Status>;

        async fn unstage(
            &self,
            request: Request<VcsPathsRequest>,
        ) -> Result<Response<VcsEmptyResponse>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedVcsGrpcService {
        kernel: HostedKernel,
        workspace: WorkspaceId,
    }

    impl HostedVcsGrpcService {
        pub fn new(kernel: HostedKernel, workspace: WorkspaceId) -> Self {
            Self { kernel, workspace }
        }
    }

    #[tonic::async_trait]
    impl Vcs for HostedVcsGrpcService {
        async fn commit(
            &self,
            request: Request<VcsCommitRequest>,
        ) -> Result<Response<VcsCommitResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let author = if input.author.is_empty() {
                "loom-hosted"
            } else {
                input.author.as_str()
            };
            self.kernel
                .grpc()
                .vcs_commit(
                    &auth,
                    self.workspace,
                    author,
                    &input.message,
                    grpc_now_ms(),
                    input.staged,
                )
                .map(|out| {
                    Response::new(VcsCommitResponse {
                        commit: out.message,
                    })
                })
                .map_err(status_from_failure)
        }

        type LogStream = tokio_stream::wrappers::ReceiverStream<Result<VcsLogResponse, Status>>;

        async fn log(
            &self,
            request: Request<VcsLogRequest>,
        ) -> Result<Response<Self::LogStream>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let requested_ref = (!input.ref_name.is_empty()).then_some(input.ref_name.as_str());
            let limit = if input.limit == 0 {
                None
            } else {
                Some(input.limit as usize)
            };
            let batch_size = input.batch_size.clamp(1, 1024) as usize;
            let commits = self
                .kernel
                .grpc()
                .vcs_log(&auth, self.workspace, requested_ref, limit)
                .map_err(status_from_failure)?
                .message;
            let (tx, rx) = mpsc::channel::<Result<VcsLogResponse, Status>>(8);
            tokio::spawn(async move {
                for chunk in commits.chunks(batch_size) {
                    let response = VcsLogResponse {
                        commits: chunk.to_vec(),
                    };
                    if tx.send(Ok(response)).await.is_err() {
                        break;
                    }
                }
            });
            Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
                rx,
            )))
        }

        async fn branch(
            &self,
            request: Request<VcsBranchRequest>,
        ) -> Result<Response<VcsEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .grpc()
                .vcs_branch(&auth, self.workspace, &request.into_inner().name)
                .map(|_| Response::new(VcsEmptyResponse {}))
                .map_err(status_from_failure)
        }

        async fn checkout(
            &self,
            request: Request<VcsCheckoutRequest>,
        ) -> Result<Response<VcsEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .grpc()
                .vcs_checkout(&auth, self.workspace, &request.into_inner().target)
                .map(|_| Response::new(VcsEmptyResponse {}))
                .map_err(status_from_failure)
        }

        async fn diff(
            &self,
            request: Request<VcsDiffRequest>,
        ) -> Result<Response<VcsDiffResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            self.kernel
                .grpc()
                .vcs_diff(&auth, self.workspace, &input.from, &input.to)
                .map(|out| {
                    Response::new(VcsDiffResponse {
                        diff_cbor: out.message,
                    })
                })
                .map_err(status_from_failure)
        }

        async fn merge(
            &self,
            request: Request<VcsMergeRequest>,
        ) -> Result<Response<VcsMergeResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let author = if input.author.is_empty() {
                "loom-hosted"
            } else {
                input.author.as_str()
            };
            self.kernel
                .grpc()
                .vcs_merge(
                    &auth,
                    self.workspace,
                    &input.source,
                    author,
                    grpc_now_ms(),
                    input.cells,
                )
                .map(|out| Response::new(vcs_merge_outcome_to_proto(out.message)))
                .map_err(status_from_failure)
        }

        async fn status(
            &self,
            request: Request<VcsStatusRequest>,
        ) -> Result<Response<VcsStatusResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .grpc()
                .vcs_status(&auth, self.workspace)
                .map(|out| Response::new(vcs_status_to_proto(out.message)))
                .map_err(status_from_failure)
        }

        async fn stage(
            &self,
            request: Request<VcsPathsRequest>,
        ) -> Result<Response<VcsEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .grpc()
                .vcs_stage(&auth, self.workspace, &request.into_inner().paths)
                .map(|_| Response::new(VcsEmptyResponse {}))
                .map_err(status_from_failure)
        }

        async fn stage_all(
            &self,
            request: Request<VcsStatusRequest>,
        ) -> Result<Response<VcsEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .grpc()
                .vcs_stage_all(&auth, self.workspace)
                .map(|_| Response::new(VcsEmptyResponse {}))
                .map_err(status_from_failure)
        }

        async fn unstage(
            &self,
            request: Request<VcsPathsRequest>,
        ) -> Result<Response<VcsEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .grpc()
                .vcs_unstage(&auth, self.workspace, &request.into_inner().paths)
                .map(|_| Response::new(VcsEmptyResponse {}))
                .map_err(status_from_failure)
        }
    }

    fn vcs_status_to_proto(status: loom_core::Status) -> VcsStatusResponse {
        VcsStatusResponse {
            staged: status.staged.into_iter().map(vcs_change_to_proto).collect(),
            unstaged: status
                .unstaged
                .into_iter()
                .map(vcs_change_to_proto)
                .collect(),
            untracked: status.untracked,
            conflicts: status.conflicts,
        }
    }

    fn vcs_change_to_proto(change: loom_core::Change) -> VcsChangeMessage {
        VcsChangeMessage {
            path: change.path,
            kind: vcs_change_kind(change.kind).to_string(),
        }
    }

    fn vcs_change_kind(kind: loom_core::ChangeKind) -> &'static str {
        match kind {
            loom_core::ChangeKind::Added => "added",
            loom_core::ChangeKind::Modified => "modified",
            loom_core::ChangeKind::Deleted => "deleted",
        }
    }

    fn vcs_merge_outcome_to_proto(outcome: loom_core::MergeOutcome) -> VcsMergeResponse {
        match outcome {
            loom_core::MergeOutcome::UpToDate => VcsMergeResponse {
                outcome: "up_to_date".to_string(),
                commit: String::new(),
                paths: Vec::new(),
            },
            loom_core::MergeOutcome::FastForward(commit) => VcsMergeResponse {
                outcome: "fast_forward".to_string(),
                commit: commit.to_string(),
                paths: Vec::new(),
            },
            loom_core::MergeOutcome::Merged(commit) => VcsMergeResponse {
                outcome: "merged".to_string(),
                commit: commit.to_string(),
                paths: Vec::new(),
            },
            loom_core::MergeOutcome::Conflicts(paths) => VcsMergeResponse {
                outcome: "conflicts".to_string(),
                commit: String::new(),
                paths,
            },
        }
    }

    #[derive(Clone)]
    pub struct VcsServer<T> {
        inner: Arc<T>,
    }

    impl<T> VcsServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for VcsServer<T>
    where
        T: Vcs,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/loom.hosted.v1.Vcs/Commit" => {
                    struct CommitSvc<T>(Arc<T>);
                    impl<T: Vcs> UnaryService<VcsCommitRequest> for CommitSvc<T> {
                        type Response = VcsCommitResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<VcsCommitRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.commit(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = CommitSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Vcs/Log" => {
                    struct LogSvc<T>(Arc<T>);
                    impl<T: Vcs> ServerStreamingService<VcsLogRequest> for LogSvc<T> {
                        type Response = VcsLogResponse;
                        type ResponseStream = T::LogStream;
                        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

                        fn call(&mut self, request: Request<VcsLogRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.log(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = LogSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.server_streaming(method, req).await)
                    })
                }
                "/loom.hosted.v1.Vcs/Branch" => {
                    struct BranchSvc<T>(Arc<T>);
                    impl<T: Vcs> UnaryService<VcsBranchRequest> for BranchSvc<T> {
                        type Response = VcsEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<VcsBranchRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.branch(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = BranchSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Vcs/Checkout" => {
                    struct CheckoutSvc<T>(Arc<T>);
                    impl<T: Vcs> UnaryService<VcsCheckoutRequest> for CheckoutSvc<T> {
                        type Response = VcsEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<VcsCheckoutRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.checkout(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = CheckoutSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Vcs/Diff" => {
                    struct DiffSvc<T>(Arc<T>);
                    impl<T: Vcs> UnaryService<VcsDiffRequest> for DiffSvc<T> {
                        type Response = VcsDiffResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<VcsDiffRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.diff(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = DiffSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Vcs/Merge" => {
                    struct MergeSvc<T>(Arc<T>);
                    impl<T: Vcs> UnaryService<VcsMergeRequest> for MergeSvc<T> {
                        type Response = VcsMergeResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<VcsMergeRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.merge(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = MergeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Vcs/Status" => {
                    struct StatusSvc<T>(Arc<T>);
                    impl<T: Vcs> UnaryService<VcsStatusRequest> for StatusSvc<T> {
                        type Response = VcsStatusResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<VcsStatusRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.status(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = StatusSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Vcs/Stage" => {
                    struct StageSvc<T>(Arc<T>);
                    impl<T: Vcs> UnaryService<VcsPathsRequest> for StageSvc<T> {
                        type Response = VcsEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<VcsPathsRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.stage(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = StageSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Vcs/StageAll" => {
                    struct StageAllSvc<T>(Arc<T>);
                    impl<T: Vcs> UnaryService<VcsStatusRequest> for StageAllSvc<T> {
                        type Response = VcsEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<VcsStatusRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.stage_all(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = StageAllSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Vcs/Unstage" => {
                    struct UnstageSvc<T>(Arc<T>);
                    impl<T: Vcs> UnaryService<VcsPathsRequest> for UnstageSvc<T> {
                        type Response = VcsEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<VcsPathsRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.unstage(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = UnstageSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: Vcs> NamedService for VcsServer<T> {
        const NAME: &'static str = "loom.hosted.v1.Vcs";
    }

    #[tonic::async_trait]
    pub trait Kv: Send + Sync + 'static {
        async fn put(
            &self,
            request: Request<KvPutRequest>,
        ) -> Result<Response<KvEmptyResponse>, Status>;

        async fn get(
            &self,
            request: Request<KvKeyRequest>,
        ) -> Result<Response<KvGetResponse>, Status>;

        async fn delete(
            &self,
            request: Request<KvKeyRequest>,
        ) -> Result<Response<KvDeleteResponse>, Status>;

        async fn list(
            &self,
            request: Request<KvListRequest>,
        ) -> Result<Response<KvListResponse>, Status>;

        type RangeStream: Stream<Item = Result<KvRangeResponse, Status>> + Send + 'static;

        async fn range(
            &self,
            request: Request<KvRangeRequest>,
        ) -> Result<Response<Self::RangeStream>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedKvGrpcService {
        kernel: HostedKernel,
        workspace: String,
        collection: String,
    }

    impl HostedKvGrpcService {
        pub fn new(
            kernel: HostedKernel,
            workspace: impl Into<String>,
            collection: impl Into<String>,
        ) -> Self {
            Self {
                kernel,
                workspace: workspace.into(),
                collection: collection.into(),
            }
        }
    }

    #[tonic::async_trait]
    impl Kv for HostedKvGrpcService {
        type RangeStream = tokio_stream::wrappers::ReceiverStream<Result<KvRangeResponse, Status>>;

        async fn put(
            &self,
            request: Request<KvPutRequest>,
        ) -> Result<Response<KvEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            self.kernel
                .data()
                .kv_put(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    &input.key_cbor,
                    input.value,
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(KvEmptyResponse {}))
        }

        async fn get(
            &self,
            request: Request<KvKeyRequest>,
        ) -> Result<Response<KvGetResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let value = self
                .kernel
                .data()
                .kv_get(&auth, &self.workspace, &self.collection, &input.key_cbor)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(KvGetResponse {
                found: value.is_some(),
                value: value.unwrap_or_default(),
            }))
        }

        async fn delete(
            &self,
            request: Request<KvKeyRequest>,
        ) -> Result<Response<KvDeleteResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let deleted = self
                .kernel
                .data()
                .kv_delete(&auth, &self.workspace, &self.collection, &input.key_cbor)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(KvDeleteResponse { deleted }))
        }

        async fn list(
            &self,
            request: Request<KvListRequest>,
        ) -> Result<Response<KvListResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let entries = self
                .kernel
                .data()
                .kv_list(&auth, &self.workspace, &self.collection)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(KvListResponse {
                entries: kv_entries_to_proto(entries),
            }))
        }

        async fn range(
            &self,
            request: Request<KvRangeRequest>,
        ) -> Result<Response<Self::RangeStream>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let batch_size = input.batch_size.clamp(1, 1024) as usize;
            let entries = self
                .kernel
                .data()
                .kv_range(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    &input.lo_cbor,
                    &input.hi_cbor,
                )
                .map_err(status_from_hosted_error)?;
            let (tx, rx) = mpsc::channel::<Result<KvRangeResponse, Status>>(8);
            tokio::spawn(async move {
                for chunk in entries.chunks(batch_size) {
                    let response = KvRangeResponse {
                        entries: kv_entries_to_proto(chunk.to_vec()),
                    };
                    if tx.send(Ok(response)).await.is_err() {
                        break;
                    }
                }
            });
            Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
                rx,
            )))
        }
    }

    #[derive(Clone)]
    pub struct KvServer<T> {
        inner: Arc<T>,
    }

    impl<T> KvServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for KvServer<T>
    where
        T: Kv,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/loom.hosted.v1.Kv/Put" => {
                    struct PutSvc<T>(Arc<T>);
                    impl<T: Kv> UnaryService<KvPutRequest> for PutSvc<T> {
                        type Response = KvEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<KvPutRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.put(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = PutSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Kv/Get" => {
                    struct GetSvc<T>(Arc<T>);
                    impl<T: Kv> UnaryService<KvKeyRequest> for GetSvc<T> {
                        type Response = KvGetResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<KvKeyRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.get(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = GetSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Kv/Delete" => {
                    struct DeleteSvc<T>(Arc<T>);
                    impl<T: Kv> UnaryService<KvKeyRequest> for DeleteSvc<T> {
                        type Response = KvDeleteResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<KvKeyRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.delete(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = DeleteSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Kv/List" => {
                    struct ListSvc<T>(Arc<T>);
                    impl<T: Kv> UnaryService<KvListRequest> for ListSvc<T> {
                        type Response = KvListResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<KvListRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.list(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ListSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Kv/Range" => {
                    struct RangeSvc<T>(Arc<T>);
                    impl<T: Kv> ServerStreamingService<KvRangeRequest> for RangeSvc<T> {
                        type Response = KvRangeResponse;
                        type ResponseStream = T::RangeStream;
                        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

                        fn call(&mut self, request: Request<KvRangeRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.range(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = RangeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.server_streaming(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: Kv> NamedService for KvServer<T> {
        const NAME: &'static str = "loom.hosted.v1.Kv";
    }

    fn kv_entries_to_proto(entries: Vec<KvEntry>) -> Vec<KvEntryMessage> {
        entries
            .into_iter()
            .map(|entry| KvEntryMessage {
                key_cbor: entry.key_cbor,
                value: entry.value,
            })
            .collect()
    }

    #[tonic::async_trait]
    pub trait Document: Send + Sync + 'static {
        async fn put_text(
            &self,
            request: Request<DocumentPutTextRequest>,
        ) -> Result<Response<DocumentDigestResponse>, Status>;

        async fn get_text(
            &self,
            request: Request<DocumentIdRequest>,
        ) -> Result<Response<DocumentTextResponse>, Status>;

        async fn put_binary(
            &self,
            request: Request<DocumentPutBinaryRequest>,
        ) -> Result<Response<DocumentDigestResponse>, Status>;

        async fn get_binary(
            &self,
            request: Request<DocumentIdRequest>,
        ) -> Result<Response<DocumentBinaryResponse>, Status>;

        async fn delete(
            &self,
            request: Request<DocumentIdRequest>,
        ) -> Result<Response<DocumentDeleteResponse>, Status>;

        type ListStream: Stream<Item = Result<DocumentListResponse, Status>> + Send + 'static;

        async fn list_binary(
            &self,
            request: Request<DocumentListRequest>,
        ) -> Result<Response<Self::ListStream>, Status>;

        async fn index_create(
            &self,
            request: Request<DocumentIndexCreateRequest>,
        ) -> Result<Response<DocumentEmptyResponse>, Status>;

        async fn index_drop(
            &self,
            request: Request<DocumentIndexNameRequest>,
        ) -> Result<Response<DocumentIndexDropResponse>, Status>;

        async fn index_rebuild(
            &self,
            request: Request<DocumentIndexNameRequest>,
        ) -> Result<Response<DocumentEmptyResponse>, Status>;

        async fn index_list(
            &self,
            request: Request<DocumentIndexListRequest>,
        ) -> Result<Response<DocumentIndexListResponse>, Status>;

        async fn index_status(
            &self,
            request: Request<DocumentIndexStatusRequest>,
        ) -> Result<Response<DocumentIndexStatusResponse>, Status>;

        type FindStream: Stream<Item = Result<DocumentFindResponse, Status>> + Send + 'static;

        async fn find(
            &self,
            request: Request<DocumentFindRequest>,
        ) -> Result<Response<Self::FindStream>, Status>;

        async fn query(
            &self,
            request: Request<DocumentQueryRequest>,
        ) -> Result<Response<DocumentQueryResponse>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedDocumentGrpcService {
        kernel: HostedKernel,
        workspace: String,
        collection: String,
    }

    impl HostedDocumentGrpcService {
        pub fn new(
            kernel: HostedKernel,
            workspace: impl Into<String>,
            collection: impl Into<String>,
        ) -> Self {
            Self {
                kernel,
                workspace: workspace.into(),
                collection: collection.into(),
            }
        }
    }

    #[tonic::async_trait]
    impl Document for HostedDocumentGrpcService {
        type ListStream =
            tokio_stream::wrappers::ReceiverStream<Result<DocumentListResponse, Status>>;
        type FindStream =
            tokio_stream::wrappers::ReceiverStream<Result<DocumentFindResponse, Status>>;

        async fn put_text(
            &self,
            request: Request<DocumentPutTextRequest>,
        ) -> Result<Response<DocumentDigestResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let result = self
                .kernel
                .data()
                .document_put_text(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    &input.id,
                    &input.text,
                    input.expected_entity_tag.as_deref(),
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(DocumentDigestResponse {
                digest: result.digest,
                entity_tag: result.entity_tag,
            }))
        }

        async fn get_text(
            &self,
            request: Request<DocumentIdRequest>,
        ) -> Result<Response<DocumentTextResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let document = self
                .kernel
                .data()
                .document_get_text(&auth, &self.workspace, &self.collection, &input.id)
                .map_err(status_from_hosted_error)?;
            let found = document.is_some();
            let document = document.unwrap_or(crate::data::HostedDocumentText {
                text: String::new(),
                digest: String::new(),
                entity_tag: String::new(),
            });
            Ok(Response::new(DocumentTextResponse {
                found,
                text: document.text,
                digest: document.digest,
                entity_tag: document.entity_tag,
            }))
        }

        async fn put_binary(
            &self,
            request: Request<DocumentPutBinaryRequest>,
        ) -> Result<Response<DocumentDigestResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let result = self
                .kernel
                .data()
                .document_put_binary(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    &input.id,
                    input.bytes,
                    input.expected_entity_tag.as_deref(),
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(DocumentDigestResponse {
                digest: result.digest,
                entity_tag: result.entity_tag,
            }))
        }

        async fn get_binary(
            &self,
            request: Request<DocumentIdRequest>,
        ) -> Result<Response<DocumentBinaryResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let document = self
                .kernel
                .data()
                .document_get_binary(&auth, &self.workspace, &self.collection, &input.id)
                .map_err(status_from_hosted_error)?;
            let found = document.is_some();
            let document = document.unwrap_or(crate::data::HostedDocumentBinary {
                bytes: Vec::new(),
                digest: String::new(),
                entity_tag: String::new(),
            });
            Ok(Response::new(DocumentBinaryResponse {
                found,
                bytes: document.bytes,
                digest: document.digest,
                entity_tag: document.entity_tag,
            }))
        }

        async fn delete(
            &self,
            request: Request<DocumentIdRequest>,
        ) -> Result<Response<DocumentDeleteResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let deleted = self
                .kernel
                .data()
                .document_delete(&auth, &self.workspace, &self.collection, &input.id)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(DocumentDeleteResponse { deleted }))
        }

        async fn list_binary(
            &self,
            request: Request<DocumentListRequest>,
        ) -> Result<Response<Self::ListStream>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let batch_size = input.batch_size.clamp(1, 1024) as usize;
            let entries = self
                .kernel
                .data()
                .document_list(&auth, &self.workspace, &self.collection)
                .map_err(status_from_hosted_error)?;
            let (tx, rx) = mpsc::channel::<Result<DocumentListResponse, Status>>(8);
            tokio::spawn(async move {
                for chunk in entries.chunks(batch_size) {
                    let response = DocumentListResponse {
                        entries: chunk.iter().cloned().map(document_entry_to_proto).collect(),
                    };
                    if tx.send(Ok(response)).await.is_err() {
                        break;
                    }
                }
            });
            Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
                rx,
            )))
        }

        async fn index_create(
            &self,
            request: Request<DocumentIndexCreateRequest>,
        ) -> Result<Response<DocumentEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            if input.declaration_json.is_empty() {
                self.kernel
                    .data()
                    .document_create_index(
                        &auth,
                        &self.workspace,
                        &self.collection,
                        &input.name,
                        &input.path,
                        input.unique,
                    )
                    .map_err(status_from_hosted_error)?;
            } else {
                let value: serde_json::Value = serde_json::from_slice(&input.declaration_json)
                    .map_err(|err| {
                        status_from_loom_error(loom_core::LoomError::invalid(err.to_string()))
                    })?;
                let declaration = loom_core::document_index_declaration_from_json(&value)
                    .map_err(status_from_loom_error)?;
                self.kernel
                    .data()
                    .document_create_index_declaration(
                        &auth,
                        &self.workspace,
                        &self.collection,
                        declaration,
                    )
                    .map_err(status_from_hosted_error)?;
            }
            Ok(Response::new(DocumentEmptyResponse {}))
        }

        async fn index_drop(
            &self,
            request: Request<DocumentIndexNameRequest>,
        ) -> Result<Response<DocumentIndexDropResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let dropped = self
                .kernel
                .data()
                .document_drop_index(&auth, &self.workspace, &self.collection, &input.name)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(DocumentIndexDropResponse { dropped }))
        }

        async fn index_rebuild(
            &self,
            request: Request<DocumentIndexNameRequest>,
        ) -> Result<Response<DocumentEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            self.kernel
                .data()
                .document_rebuild_index(&auth, &self.workspace, &self.collection, &input.name)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(DocumentEmptyResponse {}))
        }

        async fn index_list(
            &self,
            request: Request<DocumentIndexListRequest>,
        ) -> Result<Response<DocumentIndexListResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let indexes = self
                .kernel
                .data()
                .document_list_indexes(&auth, &self.workspace, &self.collection)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(DocumentIndexListResponse {
                indexes: indexes.into_iter().map(document_index_to_proto).collect(),
            }))
        }

        async fn index_status(
            &self,
            request: Request<DocumentIndexStatusRequest>,
        ) -> Result<Response<DocumentIndexStatusResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let statuses = self
                .kernel
                .data()
                .document_index_statuses(&auth, &self.workspace, &self.collection)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(DocumentIndexStatusResponse {
                statuses: statuses
                    .into_iter()
                    .map(|status| DocumentIndexStatusMessage {
                        name: status.name,
                        ready: status.ready,
                        entries: status.entries as u64,
                    })
                    .collect(),
            }))
        }

        async fn find(
            &self,
            request: Request<DocumentFindRequest>,
        ) -> Result<Response<Self::FindStream>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let batch_size = input.batch_size.clamp(1, 1024) as usize;
            let json = serde_json::from_str(&input.value_json)
                .map_err(|err| Status::invalid_argument(err.to_string()))?;
            let value =
                loom_core::document_index_value_from_json(&json).map_err(status_from_loom_error)?;
            let ids = self
                .kernel
                .data()
                .document_find(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    &input.index,
                    &value,
                )
                .map_err(status_from_hosted_error)?;
            let (tx, rx) = mpsc::channel::<Result<DocumentFindResponse, Status>>(8);
            tokio::spawn(async move {
                for chunk in ids.chunks(batch_size) {
                    let response = DocumentFindResponse {
                        ids: chunk.to_vec(),
                    };
                    if tx.send(Ok(response)).await.is_err() {
                        break;
                    }
                }
            });
            Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
                rx,
            )))
        }

        async fn query(
            &self,
            request: Request<DocumentQueryRequest>,
        ) -> Result<Response<DocumentQueryResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let json = serde_json::from_str(&input.query_json)
                .map_err(|err| Status::invalid_argument(err.to_string()))?;
            let query =
                loom_core::document_query_from_json(&json).map_err(status_from_loom_error)?;
            let result = self
                .kernel
                .data()
                .document_query(&auth, &self.workspace, &self.collection, &query)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(DocumentQueryResponse {
                result_json: loom_core::document_query_result_json(result).to_string(),
            }))
        }
    }

    fn document_entry_to_proto(entry: DocumentEntry) -> DocumentEntryMessage {
        DocumentEntryMessage {
            id: entry.id,
            document: entry.document,
        }
    }

    fn document_index_to_proto(index: HostedDocumentIndex) -> DocumentIndexMessage {
        let metadata_json = index.metadata.to_string();
        DocumentIndexMessage {
            name: index.name,
            path: index.path,
            unique: index.unique,
            index_id: index.index_id,
            extractor: index.extractor,
            key_codec: index.key_codec,
            comparator: index.comparator,
            uniqueness: index.uniqueness,
            failure_policy: index.failure_policy,
            declaration_version: index.declaration_version,
            analyzer_profile: index.analyzer_profile,
            projection: index.projection,
            partial_filter: index.partial_filter,
            metadata_json,
        }
    }

    #[derive(Clone)]
    pub struct DocumentServer<T> {
        inner: Arc<T>,
    }

    impl<T> DocumentServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for DocumentServer<T>
    where
        T: Document,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/loom.hosted.v1.Document/PutText" => {
                    struct PutTextSvc<T>(Arc<T>);
                    impl<T: Document> UnaryService<DocumentPutTextRequest> for PutTextSvc<T> {
                        type Response = DocumentDigestResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<DocumentPutTextRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.put_text(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = PutTextSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Document/GetText" => {
                    struct GetTextSvc<T>(Arc<T>);
                    impl<T: Document> UnaryService<DocumentIdRequest> for GetTextSvc<T> {
                        type Response = DocumentTextResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<DocumentIdRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.get_text(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = GetTextSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Document/PutBinary" => {
                    struct PutBinarySvc<T>(Arc<T>);
                    impl<T: Document> UnaryService<DocumentPutBinaryRequest> for PutBinarySvc<T> {
                        type Response = DocumentDigestResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<DocumentPutBinaryRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.put_binary(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = PutBinarySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Document/GetBinary" => {
                    struct GetBinarySvc<T>(Arc<T>);
                    impl<T: Document> UnaryService<DocumentIdRequest> for GetBinarySvc<T> {
                        type Response = DocumentBinaryResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<DocumentIdRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.get_binary(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = GetBinarySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Document/Delete" => {
                    struct DeleteSvc<T>(Arc<T>);
                    impl<T: Document> UnaryService<DocumentIdRequest> for DeleteSvc<T> {
                        type Response = DocumentDeleteResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<DocumentIdRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.delete(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = DeleteSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Document/ListBinary" => {
                    struct ListBinarySvc<T>(Arc<T>);
                    impl<T: Document> ServerStreamingService<DocumentListRequest> for ListBinarySvc<T> {
                        type Response = DocumentListResponse;
                        type ResponseStream = T::ListStream;
                        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

                        fn call(&mut self, request: Request<DocumentListRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.list_binary(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ListBinarySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.server_streaming(method, req).await)
                    })
                }
                "/loom.hosted.v1.Document/IndexCreate" => {
                    struct IndexCreateSvc<T>(Arc<T>);
                    impl<T: Document> UnaryService<DocumentIndexCreateRequest> for IndexCreateSvc<T> {
                        type Response = DocumentEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<DocumentIndexCreateRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.index_create(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = IndexCreateSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Document/IndexDrop" => {
                    struct IndexDropSvc<T>(Arc<T>);
                    impl<T: Document> UnaryService<DocumentIndexNameRequest> for IndexDropSvc<T> {
                        type Response = DocumentIndexDropResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<DocumentIndexNameRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.index_drop(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = IndexDropSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Document/IndexRebuild" => {
                    struct IndexRebuildSvc<T>(Arc<T>);
                    impl<T: Document> UnaryService<DocumentIndexNameRequest> for IndexRebuildSvc<T> {
                        type Response = DocumentEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<DocumentIndexNameRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.index_rebuild(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = IndexRebuildSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Document/IndexList" => {
                    struct IndexListSvc<T>(Arc<T>);
                    impl<T: Document> UnaryService<DocumentIndexListRequest> for IndexListSvc<T> {
                        type Response = DocumentIndexListResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<DocumentIndexListRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.index_list(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = IndexListSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Document/IndexStatus" => {
                    struct IndexStatusSvc<T>(Arc<T>);
                    impl<T: Document> UnaryService<DocumentIndexStatusRequest> for IndexStatusSvc<T> {
                        type Response = DocumentIndexStatusResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<DocumentIndexStatusRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.index_status(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = IndexStatusSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Document/Find" => {
                    struct FindSvc<T>(Arc<T>);
                    impl<T: Document> ServerStreamingService<DocumentFindRequest> for FindSvc<T> {
                        type Response = DocumentFindResponse;
                        type ResponseStream = T::FindStream;
                        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

                        fn call(&mut self, request: Request<DocumentFindRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.find(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = FindSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.server_streaming(method, req).await)
                    })
                }
                "/loom.hosted.v1.Document/Query" => {
                    struct QuerySvc<T>(Arc<T>);
                    impl<T: Document> UnaryService<DocumentQueryRequest> for QuerySvc<T> {
                        type Response = DocumentQueryResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<DocumentQueryRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.query(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = QuerySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: Document> NamedService for DocumentServer<T> {
        const NAME: &'static str = "loom.hosted.v1.Document";
    }

    #[tonic::async_trait]
    pub trait Fts: Send + Sync + 'static {
        async fn create(
            &self,
            request: Request<FtsCreateRequest>,
        ) -> Result<Response<FtsEmptyResponse>, Status>;

        async fn index(
            &self,
            request: Request<FtsIndexRequest>,
        ) -> Result<Response<FtsEmptyResponse>, Status>;

        async fn get(
            &self,
            request: Request<FtsIdRequest>,
        ) -> Result<Response<FtsGetResponse>, Status>;

        async fn delete(
            &self,
            request: Request<FtsIdRequest>,
        ) -> Result<Response<FtsDeleteResponse>, Status>;

        type IdsStream: Stream<Item = Result<FtsIdsResponse, Status>> + Send + 'static;

        async fn ids(
            &self,
            request: Request<FtsIdsRequest>,
        ) -> Result<Response<Self::IdsStream>, Status>;

        async fn remap(
            &self,
            request: Request<FtsRemapRequest>,
        ) -> Result<Response<FtsEmptyResponse>, Status>;

        async fn query(
            &self,
            request: Request<FtsQueryRequest>,
        ) -> Result<Response<FtsQueryResponse>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedFtsGrpcService {
        kernel: HostedKernel,
        workspace: String,
        collection: String,
    }

    impl HostedFtsGrpcService {
        pub fn new(
            kernel: HostedKernel,
            workspace: impl Into<String>,
            collection: impl Into<String>,
        ) -> Self {
            Self {
                kernel,
                workspace: workspace.into(),
                collection: collection.into(),
            }
        }
    }

    #[tonic::async_trait]
    impl Fts for HostedFtsGrpcService {
        type IdsStream = tokio_stream::wrappers::ReceiverStream<Result<FtsIdsResponse, Status>>;

        async fn create(
            &self,
            request: Request<FtsCreateRequest>,
        ) -> Result<Response<FtsEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let mapping = fts_mapping_from_proto(request.into_inner().mapping)?;
            self.kernel
                .data()
                .search_create(&auth, &self.workspace, &self.collection, mapping)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(FtsEmptyResponse {}))
        }

        async fn index(
            &self,
            request: Request<FtsIndexRequest>,
        ) -> Result<Response<FtsEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let document = fts_document_from_proto(input.document)?;
            self.kernel
                .data()
                .search_index(&auth, &self.workspace, &self.collection, input.id, document)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(FtsEmptyResponse {}))
        }

        async fn get(
            &self,
            request: Request<FtsIdRequest>,
        ) -> Result<Response<FtsGetResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let document = self
                .kernel
                .data()
                .search_get(&auth, &self.workspace, &self.collection, &input.id)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(FtsGetResponse {
                found: document.is_some(),
                document: document.map(fts_document_to_proto).unwrap_or_default(),
            }))
        }

        async fn delete(
            &self,
            request: Request<FtsIdRequest>,
        ) -> Result<Response<FtsDeleteResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let deleted = self
                .kernel
                .data()
                .search_delete(&auth, &self.workspace, &self.collection, &input.id)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(FtsDeleteResponse { deleted }))
        }

        async fn ids(
            &self,
            request: Request<FtsIdsRequest>,
        ) -> Result<Response<Self::IdsStream>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let batch_size = input.batch_size.clamp(1, 1024) as usize;
            let ids = self
                .kernel
                .data()
                .search_ids(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    input.prefix.as_deref(),
                )
                .map_err(status_from_hosted_error)?;
            let (tx, rx) = mpsc::channel::<Result<FtsIdsResponse, Status>>(8);
            tokio::spawn(async move {
                for chunk in ids.chunks(batch_size) {
                    let response = FtsIdsResponse {
                        ids: chunk.to_vec(),
                    };
                    if tx.send(Ok(response)).await.is_err() {
                        break;
                    }
                }
            });
            Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
                rx,
            )))
        }

        async fn remap(
            &self,
            request: Request<FtsRemapRequest>,
        ) -> Result<Response<FtsEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let mapping = fts_mapping_from_proto(request.into_inner().mapping)?;
            self.kernel
                .data()
                .search_remap(&auth, &self.workspace, &self.collection, mapping)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(FtsEmptyResponse {}))
        }

        async fn query(
            &self,
            request: Request<FtsQueryRequest>,
        ) -> Result<Response<FtsQueryResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let query = input
                .query
                .ok_or_else(|| Status::invalid_argument("fts query is required"))
                .and_then(fts_query_from_proto)?;
            let result = self
                .kernel
                .data()
                .search_query(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    QueryRequest::new(query, input.limit, input.offset),
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(fts_query_response_to_proto(result)))
        }
    }

    fn fts_mapping_from_proto(entries: Vec<FtsMappingEntry>) -> Result<loom_core::Mapping, Status> {
        let mut mapping = loom_core::Mapping::new();
        for entry in entries {
            let field_type = match entry.field_type.as_str() {
                "text" => FieldType::Text,
                "keyword" => FieldType::Keyword,
                _ => {
                    return Err(Status::invalid_argument(
                        "fts mapping field_type must be text or keyword",
                    ));
                }
            };
            let replaced = mapping.insert(
                entry.field,
                FieldMapping {
                    field_type,
                    stored: entry.stored.unwrap_or(true),
                    faceted: entry.faceted.unwrap_or(false),
                    analysis: Default::default(),
                },
            );
            if replaced.is_some() {
                return Err(Status::invalid_argument(
                    "fts mapping fields must be unique",
                ));
            }
        }
        Ok(mapping)
    }

    fn fts_document_from_proto(
        fields: Vec<FtsFieldValueMessage>,
    ) -> Result<loom_core::Document, Status> {
        let mut document = loom_core::Document::new();
        for field in fields {
            let value = match field
                .value
                .ok_or_else(|| Status::invalid_argument("fts field value is required"))?
            {
                FtsFieldValueKind::Text(value) => FieldValue::Text(value),
                FtsFieldValueKind::Bytes(value) => FieldValue::Bytes(value),
            };
            if document.insert(field.field, value).is_some() {
                return Err(Status::invalid_argument(
                    "fts document fields must be unique",
                ));
            }
        }
        Ok(document)
    }

    fn fts_document_to_proto(document: loom_core::Document) -> Vec<FtsFieldValueMessage> {
        document
            .into_iter()
            .map(|(field, value)| FtsFieldValueMessage {
                field,
                value: Some(match value {
                    FieldValue::Text(value) => FtsFieldValueKind::Text(value),
                    FieldValue::Bytes(value) => FtsFieldValueKind::Bytes(value),
                }),
            })
            .collect()
    }

    fn fts_query_from_proto(query: FtsQueryMessage) -> Result<Query, Status> {
        match query.kind.as_str() {
            "match" => Ok(Query::Match {
                field: query.field,
                text: query.text,
            }),
            "term" => Ok(Query::Term {
                field: query.field,
                value: query.value,
            }),
            "phrase" => Ok(Query::Phrase {
                field: query.field,
                terms: query.terms,
                slop: query.slop,
            }),
            "range" => Ok(Query::Range {
                field: query.field,
                lower: query.lower,
                upper: query.upper,
                include_lower: query.include_lower,
                include_upper: query.include_upper,
            }),
            "bool" => Ok(Query::Bool {
                must: query
                    .must
                    .into_iter()
                    .map(fts_query_from_proto)
                    .collect::<Result<Vec<_>, Status>>()?,
                should: query
                    .should
                    .into_iter()
                    .map(fts_query_from_proto)
                    .collect::<Result<Vec<_>, Status>>()?,
                must_not: query
                    .must_not
                    .into_iter()
                    .map(fts_query_from_proto)
                    .collect::<Result<Vec<_>, Status>>()?,
            }),
            _ => Err(Status::invalid_argument(
                "fts query kind must be match, term, phrase, range, or bool",
            )),
        }
    }

    fn fts_query_response_to_proto(result: QueryResponse) -> FtsQueryResponse {
        FtsQueryResponse {
            reduced: result.reduced,
            hits: result
                .hits
                .into_iter()
                .map(|hit| FtsHitMessage {
                    id: hit.id,
                    score: hit.score,
                })
                .collect(),
        }
    }

    #[derive(Clone)]
    pub struct FtsServer<T> {
        inner: Arc<T>,
    }

    impl<T> FtsServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for FtsServer<T>
    where
        T: Fts,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/loom.hosted.v1.Fts/Create" => {
                    struct CreateSvc<T>(Arc<T>);
                    impl<T: Fts> UnaryService<FtsCreateRequest> for CreateSvc<T> {
                        type Response = FtsEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<FtsCreateRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.create(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = CreateSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Fts/Index" => {
                    struct IndexSvc<T>(Arc<T>);
                    impl<T: Fts> UnaryService<FtsIndexRequest> for IndexSvc<T> {
                        type Response = FtsEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<FtsIndexRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.index(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = IndexSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Fts/Get" => {
                    struct GetSvc<T>(Arc<T>);
                    impl<T: Fts> UnaryService<FtsIdRequest> for GetSvc<T> {
                        type Response = FtsGetResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<FtsIdRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.get(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = GetSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Fts/Delete" => {
                    struct DeleteSvc<T>(Arc<T>);
                    impl<T: Fts> UnaryService<FtsIdRequest> for DeleteSvc<T> {
                        type Response = FtsDeleteResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<FtsIdRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.delete(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = DeleteSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Fts/Ids" => {
                    struct IdsSvc<T>(Arc<T>);
                    impl<T: Fts> ServerStreamingService<FtsIdsRequest> for IdsSvc<T> {
                        type Response = FtsIdsResponse;
                        type ResponseStream = T::IdsStream;
                        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

                        fn call(&mut self, request: Request<FtsIdsRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.ids(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = IdsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.server_streaming(method, req).await)
                    })
                }
                "/loom.hosted.v1.Fts/Remap" => {
                    struct RemapSvc<T>(Arc<T>);
                    impl<T: Fts> UnaryService<FtsRemapRequest> for RemapSvc<T> {
                        type Response = FtsEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<FtsRemapRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.remap(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = RemapSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Fts/Query" => {
                    struct QuerySvc<T>(Arc<T>);
                    impl<T: Fts> UnaryService<FtsQueryRequest> for QuerySvc<T> {
                        type Response = FtsQueryResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<FtsQueryRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.query(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = QuerySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: Fts> NamedService for FtsServer<T> {
        const NAME: &'static str = "loom.hosted.v1.Fts";
    }

    #[tonic::async_trait]
    pub trait Graph: Send + Sync + 'static {
        async fn upsert_node(
            &self,
            request: Request<GraphUpsertNodeRequest>,
        ) -> Result<Response<GraphEmptyResponse>, Status>;

        async fn get_node(
            &self,
            request: Request<GraphNodeRequest>,
        ) -> Result<Response<GraphNodeResponse>, Status>;

        async fn upsert_edge(
            &self,
            request: Request<GraphUpsertEdgeRequest>,
        ) -> Result<Response<GraphEmptyResponse>, Status>;

        type NeighborsStream: Stream<Item = Result<GraphIdsResponse, Status>> + Send + 'static;

        async fn neighbors(
            &self,
            request: Request<GraphNeighborsRequest>,
        ) -> Result<Response<Self::NeighborsStream>, Status>;

        type ReachableStream: Stream<Item = Result<GraphIdsResponse, Status>> + Send + 'static;

        async fn reachable(
            &self,
            request: Request<GraphReachableRequest>,
        ) -> Result<Response<Self::ReachableStream>, Status>;

        async fn query(
            &self,
            request: Request<GraphQueryRequest>,
        ) -> Result<Response<GraphQueryResponse>, Status>;

        async fn explain_query(
            &self,
            request: Request<GraphQueryRequest>,
        ) -> Result<Response<GraphExplainResponse>, Status>;

        async fn capabilities(
            &self,
            request: Request<GraphCapabilitiesRequest>,
        ) -> Result<Response<GraphCapabilitiesResponse>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedGraphGrpcService {
        kernel: HostedKernel,
        workspace: String,
        graph: String,
    }

    impl HostedGraphGrpcService {
        pub fn new(
            kernel: HostedKernel,
            workspace: impl Into<String>,
            graph: impl Into<String>,
        ) -> Self {
            Self {
                kernel,
                workspace: workspace.into(),
                graph: graph.into(),
            }
        }
    }

    #[tonic::async_trait]
    impl Graph for HostedGraphGrpcService {
        type NeighborsStream =
            tokio_stream::wrappers::ReceiverStream<Result<GraphIdsResponse, Status>>;
        type ReachableStream =
            tokio_stream::wrappers::ReceiverStream<Result<GraphIdsResponse, Status>>;

        async fn upsert_node(
            &self,
            request: Request<GraphUpsertNodeRequest>,
        ) -> Result<Response<GraphEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let props = loom_wire::graph::props_from_cbor(&input.props_cbor)
                .map_err(|err| Status::invalid_argument(err.to_string()))?;
            self.kernel
                .data()
                .graph_upsert_node(&auth, &self.workspace, &self.graph, &input.id, props)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(GraphEmptyResponse {}))
        }

        async fn get_node(
            &self,
            request: Request<GraphNodeRequest>,
        ) -> Result<Response<GraphNodeResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let props = self
                .kernel
                .data()
                .graph_get_node(&auth, &self.workspace, &self.graph, &input.id)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(GraphNodeResponse {
                found: props.is_some(),
                props_cbor: props
                    .as_ref()
                    .map(loom_wire::graph::props_to_cbor)
                    .unwrap_or_default(),
            }))
        }

        async fn upsert_edge(
            &self,
            request: Request<GraphUpsertEdgeRequest>,
        ) -> Result<Response<GraphEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let props = loom_wire::graph::props_from_cbor(&input.props_cbor)
                .map_err(|err| Status::invalid_argument(err.to_string()))?;
            self.kernel
                .data()
                .graph_upsert_edge(
                    &auth,
                    &self.workspace,
                    &self.graph,
                    &input.id,
                    &input.src,
                    &input.dst,
                    &input.label,
                    props,
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(GraphEmptyResponse {}))
        }

        async fn neighbors(
            &self,
            request: Request<GraphNeighborsRequest>,
        ) -> Result<Response<Self::NeighborsStream>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let batch_size = input.batch_size.clamp(1, 1024) as usize;
            let ids = self
                .kernel
                .data()
                .graph_neighbors(&auth, &self.workspace, &self.graph, &input.id)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(graph_ids_stream(ids, batch_size)))
        }

        async fn reachable(
            &self,
            request: Request<GraphReachableRequest>,
        ) -> Result<Response<Self::ReachableStream>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let batch_size = input.batch_size.clamp(1, 1024) as usize;
            let max_depth = input
                .max_depth
                .map(|value| {
                    usize::try_from(value)
                        .map_err(|_| Status::invalid_argument("max_depth exceeds usize"))
                })
                .transpose()?;
            let ids = self
                .kernel
                .data()
                .graph_reachable(
                    &auth,
                    &self.workspace,
                    &self.graph,
                    &input.start,
                    max_depth,
                    input.via_label.as_deref(),
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(graph_ids_stream(ids, batch_size)))
        }

        async fn query(
            &self,
            request: Request<GraphQueryRequest>,
        ) -> Result<Response<GraphQueryResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let query = loom_core::GraphQuery::parse_opencypher(&request.into_inner().opencypher)
                .map_err(|err| Status::invalid_argument(err.to_string()))?;
            let result = self
                .kernel
                .data()
                .graph_query(&auth, &self.workspace, &self.graph, &query)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(GraphQueryResponse {
                result_cbor: loom_wire::graph::graph_query_result_to_cbor(&result),
            }))
        }

        async fn explain_query(
            &self,
            request: Request<GraphQueryRequest>,
        ) -> Result<Response<GraphExplainResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let query = loom_core::GraphQuery::parse_opencypher(&request.into_inner().opencypher)
                .map_err(|err| Status::invalid_argument(err.to_string()))?;
            let explain = self
                .kernel
                .data()
                .graph_explain_query(&auth, &self.workspace, &self.graph, &query)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(GraphExplainResponse {
                explain_cbor: loom_wire::graph::graph_query_explain_to_cbor(&explain),
            }))
        }

        async fn capabilities(
            &self,
            request: Request<GraphCapabilitiesRequest>,
        ) -> Result<Response<GraphCapabilitiesResponse>, Status> {
            let _auth = hosted_auth(&request)?;
            Ok(Response::new(GraphCapabilitiesResponse {
                collection: self.graph.clone(),
                query_language: "bounded_opencypher_gql_aligned".to_string(),
                neo4j: "unsupported".to_string(),
                gremlin: "unsupported".to_string(),
            }))
        }
    }

    #[derive(Clone)]
    pub struct GraphServer<T> {
        inner: Arc<T>,
    }

    impl<T> GraphServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for GraphServer<T>
    where
        T: Graph,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/loom.hosted.v1.Graph/UpsertNode" => {
                    struct UpsertNodeSvc<T>(Arc<T>);
                    impl<T: Graph> UnaryService<GraphUpsertNodeRequest> for UpsertNodeSvc<T> {
                        type Response = GraphEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<GraphUpsertNodeRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.upsert_node(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = UpsertNodeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Graph/GetNode" => {
                    struct GetNodeSvc<T>(Arc<T>);
                    impl<T: Graph> UnaryService<GraphNodeRequest> for GetNodeSvc<T> {
                        type Response = GraphNodeResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<GraphNodeRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.get_node(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = GetNodeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Graph/UpsertEdge" => {
                    struct UpsertEdgeSvc<T>(Arc<T>);
                    impl<T: Graph> UnaryService<GraphUpsertEdgeRequest> for UpsertEdgeSvc<T> {
                        type Response = GraphEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<GraphUpsertEdgeRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.upsert_edge(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = UpsertEdgeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Graph/Neighbors" => {
                    struct NeighborsSvc<T>(Arc<T>);
                    impl<T: Graph> ServerStreamingService<GraphNeighborsRequest> for NeighborsSvc<T> {
                        type Response = GraphIdsResponse;
                        type ResponseStream = T::NeighborsStream;
                        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

                        fn call(
                            &mut self,
                            request: Request<GraphNeighborsRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.neighbors(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = NeighborsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.server_streaming(method, req).await)
                    })
                }
                "/loom.hosted.v1.Graph/Reachable" => {
                    struct ReachableSvc<T>(Arc<T>);
                    impl<T: Graph> ServerStreamingService<GraphReachableRequest> for ReachableSvc<T> {
                        type Response = GraphIdsResponse;
                        type ResponseStream = T::ReachableStream;
                        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

                        fn call(
                            &mut self,
                            request: Request<GraphReachableRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.reachable(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ReachableSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.server_streaming(method, req).await)
                    })
                }
                "/loom.hosted.v1.Graph/Query" => {
                    struct QuerySvc<T>(Arc<T>);
                    impl<T: Graph> UnaryService<GraphQueryRequest> for QuerySvc<T> {
                        type Response = GraphQueryResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<GraphQueryRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.query(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = QuerySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Graph/ExplainQuery" => {
                    struct ExplainQuerySvc<T>(Arc<T>);
                    impl<T: Graph> UnaryService<GraphQueryRequest> for ExplainQuerySvc<T> {
                        type Response = GraphExplainResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<GraphQueryRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.explain_query(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ExplainQuerySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Graph/Capabilities" => {
                    struct CapabilitiesSvc<T>(Arc<T>);
                    impl<T: Graph> UnaryService<GraphCapabilitiesRequest> for CapabilitiesSvc<T> {
                        type Response = GraphCapabilitiesResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<GraphCapabilitiesRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.capabilities(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = CapabilitiesSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: Graph> NamedService for GraphServer<T> {
        const NAME: &'static str = "loom.hosted.v1.Graph";
    }

    fn graph_ids_stream(
        ids: Vec<String>,
        batch_size: usize,
    ) -> tokio_stream::wrappers::ReceiverStream<Result<GraphIdsResponse, Status>> {
        let (tx, rx) = mpsc::channel::<Result<GraphIdsResponse, Status>>(8);
        tokio::spawn(async move {
            for chunk in ids.chunks(batch_size) {
                if tx
                    .send(Ok(GraphIdsResponse {
                        ids: chunk.to_vec(),
                    }))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        tokio_stream::wrappers::ReceiverStream::new(rx)
    }

    #[tonic::async_trait]
    pub trait Queue: Send + Sync + 'static {
        async fn append(
            &self,
            request: Request<QueueAppendRequest>,
        ) -> Result<Response<QueueAppendResponse>, Status>;

        async fn get(
            &self,
            request: Request<QueueGetRequest>,
        ) -> Result<Response<QueueGetResponse>, Status>;

        async fn range(
            &self,
            request: Request<QueueRangeRequest>,
        ) -> Result<Response<QueueRangeResponse>, Status>;

        async fn len(
            &self,
            request: Request<QueueLenRequest>,
        ) -> Result<Response<QueueLenResponse>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedQueueGrpcService {
        kernel: HostedKernel,
        workspace: String,
        stream: String,
    }

    impl HostedQueueGrpcService {
        pub fn new(
            kernel: HostedKernel,
            workspace: impl Into<String>,
            stream: impl Into<String>,
        ) -> Self {
            Self {
                kernel,
                workspace: workspace.into(),
                stream: stream.into(),
            }
        }
    }

    #[tonic::async_trait]
    impl Queue for HostedQueueGrpcService {
        async fn append(
            &self,
            request: Request<QueueAppendRequest>,
        ) -> Result<Response<QueueAppendResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let seq = self
                .kernel
                .data()
                .queue_append(
                    &auth,
                    &self.workspace,
                    &self.stream,
                    &request.into_inner().payload,
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(QueueAppendResponse { seq: seq as u64 }))
        }

        async fn get(
            &self,
            request: Request<QueueGetRequest>,
        ) -> Result<Response<QueueGetResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let seq = usize::try_from(input.seq)
                .map_err(|_| Status::invalid_argument("queue seq exceeds usize"))?;
            let payload = self
                .kernel
                .data()
                .queue_get(&auth, &self.workspace, &self.stream, seq)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(QueueGetResponse {
                found: payload.is_some(),
                payload: payload.unwrap_or_default(),
            }))
        }

        async fn range(
            &self,
            request: Request<QueueRangeRequest>,
        ) -> Result<Response<QueueRangeResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let lo = usize::try_from(input.lo)
                .map_err(|_| Status::invalid_argument("queue lo exceeds usize"))?;
            let hi = usize::try_from(input.hi)
                .map_err(|_| Status::invalid_argument("queue hi exceeds usize"))?;
            let entries = self
                .kernel
                .data()
                .queue_range(&auth, &self.workspace, &self.stream, lo, hi)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(QueueRangeResponse {
                entries: queue_entries_to_proto(entries),
            }))
        }

        async fn len(
            &self,
            request: Request<QueueLenRequest>,
        ) -> Result<Response<QueueLenResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let len = self
                .kernel
                .data()
                .queue_len(&auth, &self.workspace, &self.stream)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(QueueLenResponse { len: len as u64 }))
        }
    }

    #[derive(Clone)]
    pub struct QueueServer<T> {
        inner: Arc<T>,
    }

    impl<T> QueueServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for QueueServer<T>
    where
        T: Queue,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/loom.hosted.v1.Queue/Append" => {
                    struct AppendSvc<T>(Arc<T>);
                    impl<T: Queue> UnaryService<QueueAppendRequest> for AppendSvc<T> {
                        type Response = QueueAppendResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<QueueAppendRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.append(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = AppendSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Queue/Get" => {
                    struct GetSvc<T>(Arc<T>);
                    impl<T: Queue> UnaryService<QueueGetRequest> for GetSvc<T> {
                        type Response = QueueGetResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<QueueGetRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.get(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = GetSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Queue/Range" => {
                    struct RangeSvc<T>(Arc<T>);
                    impl<T: Queue> UnaryService<QueueRangeRequest> for RangeSvc<T> {
                        type Response = QueueRangeResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<QueueRangeRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.range(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = RangeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Queue/Len" => {
                    struct LenSvc<T>(Arc<T>);
                    impl<T: Queue> UnaryService<QueueLenRequest> for LenSvc<T> {
                        type Response = QueueLenResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<QueueLenRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.len(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = LenSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: Queue> NamedService for QueueServer<T> {
        const NAME: &'static str = "loom.hosted.v1.Queue";
    }

    fn queue_entries_to_proto(entries: Vec<QueueEntry>) -> Vec<QueueEntryMessage> {
        entries
            .into_iter()
            .map(|entry| QueueEntryMessage {
                seq: entry.seq as u64,
                payload: entry.payload,
            })
            .collect()
    }

    #[tonic::async_trait]
    pub trait Ledger: Send + Sync + 'static {
        async fn append(
            &self,
            request: Request<LedgerAppendRequest>,
        ) -> Result<Response<LedgerAppendResponse>, Status>;

        async fn get(
            &self,
            request: Request<LedgerGetRequest>,
        ) -> Result<Response<LedgerGetResponse>, Status>;

        type RangeStream: Stream<Item = Result<LedgerRangeResponse, Status>> + Send + 'static;

        async fn range(
            &self,
            request: Request<LedgerRangeRequest>,
        ) -> Result<Response<Self::RangeStream>, Status>;

        async fn head(
            &self,
            request: Request<LedgerHeadRequest>,
        ) -> Result<Response<LedgerHeadResponse>, Status>;

        async fn len(
            &self,
            request: Request<LedgerLenRequest>,
        ) -> Result<Response<LedgerLenResponse>, Status>;

        async fn verify(
            &self,
            request: Request<LedgerVerifyRequest>,
        ) -> Result<Response<LedgerEmptyResponse>, Status>;

        async fn list_collections(
            &self,
            request: Request<LedgerCollectionsRequest>,
        ) -> Result<Response<LedgerCollectionsResponse>, Status>;

        async fn checkpoint_payload(
            &self,
            request: Request<LedgerCheckpointPayloadRequest>,
        ) -> Result<Response<LedgerCheckpointPayloadResponse>, Status>;

        async fn verify_checkpoint_signatures(
            &self,
            request: Request<LedgerVerifyCheckpointRequest>,
        ) -> Result<Response<LedgerVerifyCheckpointResponse>, Status>;

        async fn proof_tree(
            &self,
            request: Request<LedgerProofTreeRequest>,
        ) -> Result<Response<LedgerProofResponse>, Status>;

        async fn inclusion_proof(
            &self,
            request: Request<LedgerInclusionProofRequest>,
        ) -> Result<Response<LedgerProofResponse>, Status>;

        async fn consistency_proof(
            &self,
            request: Request<LedgerConsistencyProofRequest>,
        ) -> Result<Response<LedgerProofResponse>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedLedgerGrpcService {
        kernel: HostedKernel,
        workspace: String,
        collection: String,
    }

    impl HostedLedgerGrpcService {
        pub fn new(
            kernel: HostedKernel,
            workspace: impl Into<String>,
            collection: impl Into<String>,
        ) -> Self {
            Self {
                kernel,
                workspace: workspace.into(),
                collection: collection.into(),
            }
        }
    }

    #[tonic::async_trait]
    impl Ledger for HostedLedgerGrpcService {
        type RangeStream =
            tokio_stream::wrappers::ReceiverStream<Result<LedgerRangeResponse, Status>>;

        async fn append(
            &self,
            request: Request<LedgerAppendRequest>,
        ) -> Result<Response<LedgerAppendResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let seq = self
                .kernel
                .data()
                .ledger_append(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    request.into_inner().payload,
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(LedgerAppendResponse { seq }))
        }

        async fn get(
            &self,
            request: Request<LedgerGetRequest>,
        ) -> Result<Response<LedgerGetResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let seq = request.into_inner().seq;
            let payload = self
                .kernel
                .data()
                .ledger_get(&auth, &self.workspace, &self.collection, seq)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(LedgerGetResponse {
                found: payload.is_some(),
                payload: payload.unwrap_or_default(),
            }))
        }

        async fn range(
            &self,
            request: Request<LedgerRangeRequest>,
        ) -> Result<Response<Self::RangeStream>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let batch_size = input.batch_size.clamp(1, 1024) as usize;
            let scan = self
                .kernel
                .data()
                .ledger_range(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    input.start,
                    input.end,
                )
                .map_err(status_from_hosted_error)?;
            let start = scan.start;
            let end = scan.end;
            let state = ledger_range_state_to_proto(scan.state);
            let (tx, rx) = mpsc::channel::<Result<LedgerRangeResponse, Status>>(8);
            tokio::spawn(async move {
                for chunk in scan.entries.chunks(batch_size) {
                    let response = LedgerRangeResponse {
                        start,
                        end,
                        state: state.to_string(),
                        entries: chunk.iter().cloned().map(ledger_entry_to_proto).collect(),
                    };
                    if tx.send(Ok(response)).await.is_err() {
                        break;
                    }
                }
            });
            Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
                rx,
            )))
        }

        async fn head(
            &self,
            request: Request<LedgerHeadRequest>,
        ) -> Result<Response<LedgerHeadResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let digest = self
                .kernel
                .data()
                .ledger_head(&auth, &self.workspace, &self.collection)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(LedgerHeadResponse {
                found: digest.is_some(),
                digest: digest.map(|digest| digest.to_string()).unwrap_or_default(),
            }))
        }

        async fn len(
            &self,
            request: Request<LedgerLenRequest>,
        ) -> Result<Response<LedgerLenResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let len = self
                .kernel
                .data()
                .ledger_len(&auth, &self.workspace, &self.collection)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(LedgerLenResponse { len }))
        }

        async fn verify(
            &self,
            request: Request<LedgerVerifyRequest>,
        ) -> Result<Response<LedgerEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .data()
                .ledger_verify(&auth, &self.workspace, &self.collection)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(LedgerEmptyResponse {}))
        }

        async fn list_collections(
            &self,
            request: Request<LedgerCollectionsRequest>,
        ) -> Result<Response<LedgerCollectionsResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let collections = self
                .kernel
                .data()
                .ledger_list_collections(&auth, &self.workspace)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(LedgerCollectionsResponse { collections }))
        }

        async fn checkpoint_payload(
            &self,
            request: Request<LedgerCheckpointPayloadRequest>,
        ) -> Result<Response<LedgerCheckpointPayloadResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let payload_cbor = self
                .kernel
                .data()
                .ledger_checkpoint_payload(&auth, &self.workspace, &self.collection)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(LedgerCheckpointPayloadResponse {
                payload_cbor,
            }))
        }

        async fn verify_checkpoint_signatures(
            &self,
            request: Request<LedgerVerifyCheckpointRequest>,
        ) -> Result<Response<LedgerVerifyCheckpointResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let signatures = self
                .kernel
                .data()
                .ledger_verify_checkpoint_signatures(&auth, &self.workspace, &self.collection)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(LedgerVerifyCheckpointResponse {
                signatures: signatures as u64,
            }))
        }

        async fn proof_tree(
            &self,
            request: Request<LedgerProofTreeRequest>,
        ) -> Result<Response<LedgerProofResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let proof_cbor = self
                .kernel
                .data()
                .ledger_proof_tree(&auth, &self.workspace, &self.collection)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(LedgerProofResponse { proof_cbor }))
        }

        async fn inclusion_proof(
            &self,
            request: Request<LedgerInclusionProofRequest>,
        ) -> Result<Response<LedgerProofResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let seq = request.into_inner().seq;
            let proof_cbor = self
                .kernel
                .data()
                .ledger_inclusion_proof(&auth, &self.workspace, &self.collection, seq)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(LedgerProofResponse { proof_cbor }))
        }

        async fn consistency_proof(
            &self,
            request: Request<LedgerConsistencyProofRequest>,
        ) -> Result<Response<LedgerProofResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let proof_cbor = self
                .kernel
                .data()
                .ledger_consistency_proof(
                    &auth,
                    &self.workspace,
                    &self.collection,
                    input.first_tree_size,
                    input.second_tree_size,
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(LedgerProofResponse { proof_cbor }))
        }
    }

    #[derive(Clone)]
    pub struct LedgerServer<T> {
        inner: Arc<T>,
    }

    impl<T> LedgerServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for LedgerServer<T>
    where
        T: Ledger,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/loom.hosted.v1.Ledger/Append" => {
                    struct AppendSvc<T>(Arc<T>);
                    impl<T: Ledger> UnaryService<LedgerAppendRequest> for AppendSvc<T> {
                        type Response = LedgerAppendResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<LedgerAppendRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.append(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = AppendSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Ledger/Get" => {
                    struct GetSvc<T>(Arc<T>);
                    impl<T: Ledger> UnaryService<LedgerGetRequest> for GetSvc<T> {
                        type Response = LedgerGetResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<LedgerGetRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.get(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = GetSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Ledger/Range" => {
                    struct RangeSvc<T>(Arc<T>);
                    impl<T: Ledger> ServerStreamingService<LedgerRangeRequest> for RangeSvc<T> {
                        type Response = LedgerRangeResponse;
                        type ResponseStream = T::RangeStream;
                        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

                        fn call(&mut self, request: Request<LedgerRangeRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.range(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = RangeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.server_streaming(method, req).await)
                    })
                }
                "/loom.hosted.v1.Ledger/Head" => {
                    struct HeadSvc<T>(Arc<T>);
                    impl<T: Ledger> UnaryService<LedgerHeadRequest> for HeadSvc<T> {
                        type Response = LedgerHeadResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<LedgerHeadRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.head(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = HeadSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Ledger/Len" => {
                    struct LenSvc<T>(Arc<T>);
                    impl<T: Ledger> UnaryService<LedgerLenRequest> for LenSvc<T> {
                        type Response = LedgerLenResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<LedgerLenRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.len(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = LenSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Ledger/Verify" => {
                    struct VerifySvc<T>(Arc<T>);
                    impl<T: Ledger> UnaryService<LedgerVerifyRequest> for VerifySvc<T> {
                        type Response = LedgerEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<LedgerVerifyRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.verify(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = VerifySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Ledger/ListCollections" => {
                    struct ListCollectionsSvc<T>(Arc<T>);
                    impl<T: Ledger> UnaryService<LedgerCollectionsRequest> for ListCollectionsSvc<T> {
                        type Response = LedgerCollectionsResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<LedgerCollectionsRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.list_collections(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ListCollectionsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Ledger/CheckpointPayload" => {
                    struct CheckpointPayloadSvc<T>(Arc<T>);
                    impl<T: Ledger> UnaryService<LedgerCheckpointPayloadRequest> for CheckpointPayloadSvc<T> {
                        type Response = LedgerCheckpointPayloadResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<LedgerCheckpointPayloadRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.checkpoint_payload(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = CheckpointPayloadSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Ledger/VerifyCheckpointSignatures" => {
                    struct VerifyCheckpointSignaturesSvc<T>(Arc<T>);
                    impl<T: Ledger> UnaryService<LedgerVerifyCheckpointRequest> for VerifyCheckpointSignaturesSvc<T> {
                        type Response = LedgerVerifyCheckpointResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<LedgerVerifyCheckpointRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(
                                async move { inner.verify_checkpoint_signatures(request).await },
                            )
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = VerifyCheckpointSignaturesSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Ledger/ProofTree" => {
                    struct ProofTreeSvc<T>(Arc<T>);
                    impl<T: Ledger> UnaryService<LedgerProofTreeRequest> for ProofTreeSvc<T> {
                        type Response = LedgerProofResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<LedgerProofTreeRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.proof_tree(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ProofTreeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Ledger/InclusionProof" => {
                    struct InclusionProofSvc<T>(Arc<T>);
                    impl<T: Ledger> UnaryService<LedgerInclusionProofRequest> for InclusionProofSvc<T> {
                        type Response = LedgerProofResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<LedgerInclusionProofRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.inclusion_proof(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = InclusionProofSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Ledger/ConsistencyProof" => {
                    struct ConsistencyProofSvc<T>(Arc<T>);
                    impl<T: Ledger> UnaryService<LedgerConsistencyProofRequest> for ConsistencyProofSvc<T> {
                        type Response = LedgerProofResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<LedgerConsistencyProofRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.consistency_proof(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ConsistencyProofSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: Ledger> NamedService for LedgerServer<T> {
        const NAME: &'static str = "loom.hosted.v1.Ledger";
    }

    fn ledger_range_state_to_proto(state: LedgerRangeState) -> &'static str {
        match state {
            LedgerRangeState::Retained => "retained",
            LedgerRangeState::PlannedPrune => "planned_prune",
            LedgerRangeState::Pruned => "pruned",
            LedgerRangeState::LegalHold => "legal_hold",
        }
    }

    fn ledger_entry_to_proto(entry: LedgerRangeEntry) -> LedgerEntryMessage {
        LedgerEntryMessage {
            seq: entry.seq,
            payload: entry.payload,
            entry_hash: entry.entry_hash.to_string(),
        }
    }

    #[tonic::async_trait]
    pub trait TimeSeries: Send + Sync + 'static {
        async fn put(
            &self,
            request: Request<TimeSeriesPutRequest>,
        ) -> Result<Response<TimeSeriesEmptyResponse>, Status>;

        async fn get(
            &self,
            request: Request<TimeSeriesGetRequest>,
        ) -> Result<Response<TimeSeriesPointResponse>, Status>;

        async fn latest(
            &self,
            request: Request<TimeSeriesLatestRequest>,
        ) -> Result<Response<TimeSeriesPointResponse>, Status>;

        type RangeStream: Stream<Item = Result<TimeSeriesRangeResponse, Status>> + Send + 'static;

        async fn range(
            &self,
            request: Request<TimeSeriesRangeRequest>,
        ) -> Result<Response<Self::RangeStream>, Status>;

        async fn put_structured(
            &self,
            request: Request<TimeSeriesPutStructuredRequest>,
        ) -> Result<Response<TimeSeriesEmptyResponse>, Status>;

        type RangeStructuredStream: Stream<Item = Result<TimeSeriesStructuredRangeResponse, Status>>
            + Send
            + 'static;

        async fn range_structured(
            &self,
            request: Request<TimeSeriesStructuredRangeRequest>,
        ) -> Result<Response<Self::RangeStructuredStream>, Status>;

        async fn policy(
            &self,
            request: Request<TimeSeriesPolicyRequest>,
        ) -> Result<Response<TimeSeriesPolicyMessage>, Status>;

        async fn set_policy(
            &self,
            request: Request<TimeSeriesSetPolicyRequest>,
        ) -> Result<Response<TimeSeriesEmptyResponse>, Status>;

        async fn materialize_rollup(
            &self,
            request: Request<TimeSeriesMaterializeRollupRequest>,
        ) -> Result<Response<TimeSeriesEmptyResponse>, Status>;

        type RangeRollupStructuredStream: Stream<Item = Result<TimeSeriesStructuredRangeResponse, Status>>
            + Send
            + 'static;

        async fn range_rollup_structured(
            &self,
            request: Request<TimeSeriesRangeRollupStructuredRequest>,
        ) -> Result<Response<Self::RangeRollupStructuredStream>, Status>;

        async fn prune_before(
            &self,
            request: Request<TimeSeriesPruneBeforeRequest>,
        ) -> Result<Response<TimeSeriesPruneBeforeResponse>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedTimeSeriesGrpcService {
        kernel: HostedKernel,
        workspace: String,
        series: String,
    }

    impl HostedTimeSeriesGrpcService {
        pub fn new(
            kernel: HostedKernel,
            workspace: impl Into<String>,
            series: impl Into<String>,
        ) -> Self {
            Self {
                kernel,
                workspace: workspace.into(),
                series: series.into(),
            }
        }
    }

    #[tonic::async_trait]
    impl TimeSeries for HostedTimeSeriesGrpcService {
        type RangeStream =
            tokio_stream::wrappers::ReceiverStream<Result<TimeSeriesRangeResponse, Status>>;
        type RangeStructuredStream = tokio_stream::wrappers::ReceiverStream<
            Result<TimeSeriesStructuredRangeResponse, Status>,
        >;
        type RangeRollupStructuredStream = tokio_stream::wrappers::ReceiverStream<
            Result<TimeSeriesStructuredRangeResponse, Status>,
        >;

        async fn put(
            &self,
            request: Request<TimeSeriesPutRequest>,
        ) -> Result<Response<TimeSeriesEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            self.kernel
                .data()
                .timeseries_put(
                    &auth,
                    &self.workspace,
                    &self.series,
                    input.timestamp,
                    input.value,
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(TimeSeriesEmptyResponse {}))
        }

        async fn get(
            &self,
            request: Request<TimeSeriesGetRequest>,
        ) -> Result<Response<TimeSeriesPointResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let value = self
                .kernel
                .data()
                .timeseries_get(&auth, &self.workspace, &self.series, input.timestamp)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(TimeSeriesPointResponse {
                found: value.is_some(),
                point: value.map(|value| TimeSeriesPointMessage {
                    timestamp: input.timestamp,
                    value,
                }),
            }))
        }

        async fn latest(
            &self,
            request: Request<TimeSeriesLatestRequest>,
        ) -> Result<Response<TimeSeriesPointResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let point = self
                .kernel
                .data()
                .timeseries_latest(&auth, &self.workspace, &self.series)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(TimeSeriesPointResponse {
                found: point.is_some(),
                point: point.map(time_series_point_to_proto),
            }))
        }

        async fn range(
            &self,
            request: Request<TimeSeriesRangeRequest>,
        ) -> Result<Response<Self::RangeStream>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let batch_size = input.batch_size.clamp(1, 1024) as usize;
            let points = self
                .kernel
                .data()
                .timeseries_range(&auth, &self.workspace, &self.series, input.from, input.to)
                .map_err(status_from_hosted_error)?;
            let (tx, rx) = mpsc::channel::<Result<TimeSeriesRangeResponse, Status>>(8);
            tokio::spawn(async move {
                for chunk in points.chunks(batch_size) {
                    let response = TimeSeriesRangeResponse {
                        points: chunk
                            .iter()
                            .cloned()
                            .map(time_series_point_to_proto)
                            .collect(),
                    };
                    if tx.send(Ok(response)).await.is_err() {
                        break;
                    }
                }
            });
            Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
                rx,
            )))
        }

        async fn put_structured(
            &self,
            request: Request<TimeSeriesPutStructuredRequest>,
        ) -> Result<Response<TimeSeriesEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let point = input
                .point
                .ok_or_else(|| Status::invalid_argument("structured point is required"))
                .and_then(structured_time_series_point_from_proto)?;
            self.kernel
                .data()
                .timeseries_put_structured(&auth, &self.workspace, &self.series, point)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(TimeSeriesEmptyResponse {}))
        }

        async fn range_structured(
            &self,
            request: Request<TimeSeriesStructuredRangeRequest>,
        ) -> Result<Response<Self::RangeStructuredStream>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let batch_size = input.batch_size.clamp(1, 1024) as usize;
            let points = self
                .kernel
                .data()
                .timeseries_range_structured(
                    &auth,
                    &self.workspace,
                    &self.series,
                    input.from_ns,
                    input.to_ns,
                )
                .map_err(status_from_hosted_error)?;
            let (tx, rx) = mpsc::channel::<Result<TimeSeriesStructuredRangeResponse, Status>>(8);
            tokio::spawn(async move {
                for chunk in points.chunks(batch_size) {
                    let response = TimeSeriesStructuredRangeResponse {
                        points: chunk
                            .iter()
                            .cloned()
                            .map(structured_time_series_point_to_proto)
                            .collect(),
                    };
                    if tx.send(Ok(response)).await.is_err() {
                        break;
                    }
                }
            });
            Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
                rx,
            )))
        }

        async fn policy(
            &self,
            request: Request<TimeSeriesPolicyRequest>,
        ) -> Result<Response<TimeSeriesPolicyMessage>, Status> {
            let auth = hosted_auth(&request)?;
            let policy = self
                .kernel
                .data()
                .timeseries_policy(&auth, &self.workspace, &self.series)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(TimeSeriesPolicyMessage {
                query_start_ns: policy.query_start_ns,
                rollups: policy
                    .rollups
                    .into_iter()
                    .map(|rollup| TimeSeriesRollupMessage {
                        name: rollup.name,
                        resolution_ns: rollup.resolution_ns,
                        aggregation: time_series_aggregation_to_proto(rollup.aggregation),
                    })
                    .collect(),
            }))
        }

        async fn set_policy(
            &self,
            request: Request<TimeSeriesSetPolicyRequest>,
        ) -> Result<Response<TimeSeriesEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let rollups = input
                .rollups
                .into_iter()
                .map(|rollup| {
                    Ok((
                        rollup.name,
                        rollup.resolution_ns,
                        time_series_aggregation_from_proto(&rollup.aggregation)?,
                    ))
                })
                .collect::<Result<Vec<_>, Status>>()?;
            self.kernel
                .data()
                .timeseries_set_policy(
                    &auth,
                    &self.workspace,
                    &self.series,
                    input.query_start_ns,
                    rollups,
                )
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(TimeSeriesEmptyResponse {}))
        }

        async fn materialize_rollup(
            &self,
            request: Request<TimeSeriesMaterializeRollupRequest>,
        ) -> Result<Response<TimeSeriesEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            self.kernel
                .data()
                .timeseries_materialize_rollup(&auth, &self.workspace, &self.series, &input.rollup)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(TimeSeriesEmptyResponse {}))
        }

        async fn range_rollup_structured(
            &self,
            request: Request<TimeSeriesRangeRollupStructuredRequest>,
        ) -> Result<Response<Self::RangeRollupStructuredStream>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let batch_size = input.batch_size.clamp(1, 1024) as usize;
            let points = self
                .kernel
                .data()
                .timeseries_range_rollup_structured(
                    &auth,
                    &self.workspace,
                    &self.series,
                    &input.rollup,
                    input.from_ns,
                    input.to_ns,
                )
                .map_err(status_from_hosted_error)?;
            let (tx, rx) = mpsc::channel::<Result<TimeSeriesStructuredRangeResponse, Status>>(8);
            tokio::spawn(async move {
                for chunk in points.chunks(batch_size) {
                    let response = TimeSeriesStructuredRangeResponse {
                        points: chunk
                            .iter()
                            .cloned()
                            .map(structured_time_series_point_to_proto)
                            .collect(),
                    };
                    if tx.send(Ok(response)).await.is_err() {
                        break;
                    }
                }
            });
            Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
                rx,
            )))
        }

        async fn prune_before(
            &self,
            request: Request<TimeSeriesPruneBeforeRequest>,
        ) -> Result<Response<TimeSeriesPruneBeforeResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let pruned = self
                .kernel
                .data()
                .timeseries_prune_before(&auth, &self.workspace, &self.series, input.cutoff_ns)
                .map_err(status_from_hosted_error)?;
            Ok(Response::new(TimeSeriesPruneBeforeResponse {
                pruned: pruned as u64,
            }))
        }
    }

    #[derive(Clone)]
    pub struct TimeSeriesServer<T> {
        inner: Arc<T>,
    }

    impl<T> TimeSeriesServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for TimeSeriesServer<T>
    where
        T: TimeSeries,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/loom.hosted.v1.TimeSeries/Put" => {
                    struct PutSvc<T>(Arc<T>);
                    impl<T: TimeSeries> UnaryService<TimeSeriesPutRequest> for PutSvc<T> {
                        type Response = TimeSeriesEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<TimeSeriesPutRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.put(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = PutSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.TimeSeries/Get" => {
                    struct GetSvc<T>(Arc<T>);
                    impl<T: TimeSeries> UnaryService<TimeSeriesGetRequest> for GetSvc<T> {
                        type Response = TimeSeriesPointResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<TimeSeriesGetRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.get(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = GetSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.TimeSeries/Latest" => {
                    struct LatestSvc<T>(Arc<T>);
                    impl<T: TimeSeries> UnaryService<TimeSeriesLatestRequest> for LatestSvc<T> {
                        type Response = TimeSeriesPointResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<TimeSeriesLatestRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.latest(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = LatestSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.TimeSeries/Range" => {
                    struct RangeSvc<T>(Arc<T>);
                    impl<T: TimeSeries> ServerStreamingService<TimeSeriesRangeRequest> for RangeSvc<T> {
                        type Response = TimeSeriesRangeResponse;
                        type ResponseStream = T::RangeStream;
                        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

                        fn call(
                            &mut self,
                            request: Request<TimeSeriesRangeRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.range(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = RangeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.server_streaming(method, req).await)
                    })
                }
                "/loom.hosted.v1.TimeSeries/PutStructured" => {
                    struct PutStructuredSvc<T>(Arc<T>);
                    impl<T: TimeSeries> UnaryService<TimeSeriesPutStructuredRequest> for PutStructuredSvc<T> {
                        type Response = TimeSeriesEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<TimeSeriesPutStructuredRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.put_structured(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = PutStructuredSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.TimeSeries/RangeStructured" => {
                    struct RangeStructuredSvc<T>(Arc<T>);
                    impl<T: TimeSeries> ServerStreamingService<TimeSeriesStructuredRangeRequest>
                        for RangeStructuredSvc<T>
                    {
                        type Response = TimeSeriesStructuredRangeResponse;
                        type ResponseStream = T::RangeStructuredStream;
                        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

                        fn call(
                            &mut self,
                            request: Request<TimeSeriesStructuredRangeRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.range_structured(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = RangeStructuredSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.server_streaming(method, req).await)
                    })
                }
                "/loom.hosted.v1.TimeSeries/Policy" => {
                    struct PolicySvc<T>(Arc<T>);
                    impl<T: TimeSeries> UnaryService<TimeSeriesPolicyRequest> for PolicySvc<T> {
                        type Response = TimeSeriesPolicyMessage;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<TimeSeriesPolicyRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.policy(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = PolicySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.TimeSeries/SetPolicy" => {
                    struct SetPolicySvc<T>(Arc<T>);
                    impl<T: TimeSeries> UnaryService<TimeSeriesSetPolicyRequest> for SetPolicySvc<T> {
                        type Response = TimeSeriesEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<TimeSeriesSetPolicyRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.set_policy(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = SetPolicySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.TimeSeries/MaterializeRollup" => {
                    struct MaterializeRollupSvc<T>(Arc<T>);
                    impl<T: TimeSeries> UnaryService<TimeSeriesMaterializeRollupRequest> for MaterializeRollupSvc<T> {
                        type Response = TimeSeriesEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<TimeSeriesMaterializeRollupRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.materialize_rollup(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = MaterializeRollupSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.TimeSeries/RangeRollupStructured" => {
                    struct RangeRollupStructuredSvc<T>(Arc<T>);
                    impl<T: TimeSeries>
                        ServerStreamingService<TimeSeriesRangeRollupStructuredRequest>
                        for RangeRollupStructuredSvc<T>
                    {
                        type Response = TimeSeriesStructuredRangeResponse;
                        type ResponseStream = T::RangeRollupStructuredStream;
                        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

                        fn call(
                            &mut self,
                            request: Request<TimeSeriesRangeRollupStructuredRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.range_rollup_structured(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = RangeRollupStructuredSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.server_streaming(method, req).await)
                    })
                }
                "/loom.hosted.v1.TimeSeries/PruneBefore" => {
                    struct PruneBeforeSvc<T>(Arc<T>);
                    impl<T: TimeSeries> UnaryService<TimeSeriesPruneBeforeRequest> for PruneBeforeSvc<T> {
                        type Response = TimeSeriesPruneBeforeResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<TimeSeriesPruneBeforeRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.prune_before(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = PruneBeforeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: TimeSeries> NamedService for TimeSeriesServer<T> {
        const NAME: &'static str = "loom.hosted.v1.TimeSeries";
    }

    fn time_series_point_to_proto(point: TimeSeriesPoint) -> TimeSeriesPointMessage {
        TimeSeriesPointMessage {
            timestamp: point.timestamp,
            value: point.value,
        }
    }

    fn structured_time_series_point_to_proto(
        point: StructuredTimeSeriesPoint,
    ) -> StructuredTimeSeriesPointMessage {
        StructuredTimeSeriesPointMessage {
            measurement: point.measurement,
            tags: point
                .tags
                .into_iter()
                .map(|(name, value)| TimeSeriesTagMessage { name, value })
                .collect(),
            timestamp_ns: point.timestamp_ns,
            fields: point
                .fields
                .into_iter()
                .map(|(name, value)| TimeSeriesFieldMessage {
                    name,
                    value: Some(time_series_value_to_proto(value)),
                })
                .collect(),
        }
    }

    fn structured_time_series_point_from_proto(
        point: StructuredTimeSeriesPointMessage,
    ) -> Result<StructuredTimeSeriesPoint, Status> {
        let mut tags = BTreeMap::new();
        for tag in point.tags {
            if tags.insert(tag.name, tag.value).is_some() {
                return Err(Status::invalid_argument(
                    "structured time-series tags must be unique",
                ));
            }
        }
        let mut fields = BTreeMap::new();
        for field in point.fields {
            let value = field
                .value
                .ok_or_else(|| Status::invalid_argument("structured field value is required"))?;
            if fields
                .insert(field.name, time_series_value_from_proto(value))
                .is_some()
            {
                return Err(Status::invalid_argument(
                    "structured time-series fields must be unique",
                ));
            }
        }
        Ok(StructuredTimeSeriesPoint {
            measurement: point.measurement,
            tags,
            timestamp_ns: point.timestamp_ns,
            fields,
        })
    }

    fn time_series_value_to_proto(value: TimeSeriesValue) -> TimeSeriesFieldKind {
        match value {
            TimeSeriesValue::Int(value) => TimeSeriesFieldKind::Int(value),
            TimeSeriesValue::Float(value) => TimeSeriesFieldKind::Float(value),
            TimeSeriesValue::Text(value) => TimeSeriesFieldKind::Text(value),
            TimeSeriesValue::Bool(value) => TimeSeriesFieldKind::Bool(value),
            TimeSeriesValue::Bytes(value) => TimeSeriesFieldKind::Bytes(value),
        }
    }

    fn time_series_value_from_proto(value: TimeSeriesFieldKind) -> TimeSeriesValue {
        match value {
            TimeSeriesFieldKind::Int(value) => TimeSeriesValue::Int(value),
            TimeSeriesFieldKind::Float(value) => TimeSeriesValue::Float(value),
            TimeSeriesFieldKind::Text(value) => TimeSeriesValue::Text(value),
            TimeSeriesFieldKind::Bool(value) => TimeSeriesValue::Bool(value),
            TimeSeriesFieldKind::Bytes(value) => TimeSeriesValue::Bytes(value),
        }
    }

    fn time_series_aggregation_from_proto(value: &str) -> Result<TimeSeriesAggregation, Status> {
        match value {
            "count" => Ok(TimeSeriesAggregation::Count),
            "sum" => Ok(TimeSeriesAggregation::Sum),
            "min" => Ok(TimeSeriesAggregation::Min),
            "max" => Ok(TimeSeriesAggregation::Max),
            "mean" => Ok(TimeSeriesAggregation::Mean),
            _ => Err(Status::invalid_argument(
                "time-series aggregation must be count, sum, min, max, or mean",
            )),
        }
    }

    fn time_series_aggregation_to_proto(value: TimeSeriesAggregation) -> String {
        match value {
            TimeSeriesAggregation::Count => "count",
            TimeSeriesAggregation::Sum => "sum",
            TimeSeriesAggregation::Min => "min",
            TimeSeriesAggregation::Max => "max",
            TimeSeriesAggregation::Mean => "mean",
        }
        .to_string()
    }

    #[tonic::async_trait]
    pub trait Columnar: Send + Sync + 'static {
        async fn create(
            &self,
            request: Request<ColumnarCreateRequest>,
        ) -> Result<Response<ColumnarEmptyResponse>, Status>;

        async fn append(
            &self,
            request: Request<ColumnarAppendRequest>,
        ) -> Result<Response<ColumnarEmptyResponse>, Status>;

        async fn compact(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarEmptyResponse>, Status>;

        async fn inspect(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarCborResponse>, Status>;

        async fn source_digest(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarCborResponse>, Status>;

        async fn scan(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarCborResponse>, Status>;

        async fn columns(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarCborResponse>, Status>;

        async fn rows(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarRowsResponse>, Status>;

        async fn select(
            &self,
            request: Request<ColumnarSelectRequest>,
        ) -> Result<Response<ColumnarCborResponse>, Status>;

        async fn aggregate(
            &self,
            request: Request<ColumnarAggregateRequest>,
        ) -> Result<Response<ColumnarCborResponse>, Status>;

        async fn export_arrow_ipc(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarBytesResponse>, Status>;

        async fn import_arrow_ipc(
            &self,
            request: Request<ColumnarTransferRequest>,
        ) -> Result<Response<ColumnarEmptyResponse>, Status>;

        async fn export_parquet(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarBytesResponse>, Status>;

        async fn import_parquet(
            &self,
            request: Request<ColumnarTransferRequest>,
        ) -> Result<Response<ColumnarEmptyResponse>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedColumnarGrpcService {
        kernel: HostedKernel,
        workspace: String,
        dataset: String,
    }

    impl HostedColumnarGrpcService {
        pub fn new(
            kernel: HostedKernel,
            workspace: impl Into<String>,
            dataset: impl Into<String>,
        ) -> Self {
            Self {
                kernel,
                workspace: workspace.into(),
                dataset: dataset.into(),
            }
        }
    }

    #[tonic::async_trait]
    impl Columnar for HostedColumnarGrpcService {
        async fn create(
            &self,
            request: Request<ColumnarCreateRequest>,
        ) -> Result<Response<ColumnarEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let columns = loom_wire::columnar::columns_from_cbor(&input.columns_cbor)
                .map_err(status_from_loom_error)?;
            let target_segment_rows = usize::try_from(input.target_segment_rows)
                .map_err(|_| Status::invalid_argument("target_segment_rows is too large"))?;
            self.kernel
                .data()
                .columnar_create(
                    &auth,
                    &self.workspace,
                    &self.dataset,
                    columns,
                    target_segment_rows,
                )
                .map(|_| Response::new(ColumnarEmptyResponse {}))
                .map_err(status_from_hosted_error)
        }

        async fn append(
            &self,
            request: Request<ColumnarAppendRequest>,
        ) -> Result<Response<ColumnarEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let row = loom_wire::columnar::row_from_cbor(&request.into_inner().row_cbor)
                .map_err(status_from_loom_error)?;
            self.kernel
                .data()
                .columnar_append(&auth, &self.workspace, &self.dataset, row)
                .map(|_| Response::new(ColumnarEmptyResponse {}))
                .map_err(status_from_hosted_error)
        }

        async fn compact(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .data()
                .columnar_compact(&auth, &self.workspace, &self.dataset)
                .map(|_| Response::new(ColumnarEmptyResponse {}))
                .map_err(status_from_hosted_error)
        }

        async fn inspect(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarCborResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .data()
                .columnar_inspect(&auth, &self.workspace, &self.dataset)
                .map(|inspect| {
                    Response::new(ColumnarCborResponse {
                        cbor: loom_wire::columnar::inspect_to_cbor(inspect),
                    })
                })
                .map_err(status_from_hosted_error)
        }

        async fn source_digest(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarCborResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .data()
                .columnar_source_digest(&auth, &self.workspace, &self.dataset)
                .map(|digest| {
                    Response::new(ColumnarCborResponse {
                        cbor: loom_wire::columnar::digest_to_cbor(digest),
                    })
                })
                .map_err(status_from_hosted_error)
        }

        async fn scan(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarCborResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .data()
                .columnar_scan(&auth, &self.workspace, &self.dataset)
                .map(|rows| {
                    Response::new(ColumnarCborResponse {
                        cbor: loom_wire::columnar::rows_to_cbor(rows),
                    })
                })
                .map_err(status_from_hosted_error)
        }

        async fn columns(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarCborResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .data()
                .columnar_columns(&auth, &self.workspace, &self.dataset)
                .map(|columns| {
                    Response::new(ColumnarCborResponse {
                        cbor: loom_wire::columnar::columns_to_cbor(columns),
                    })
                })
                .map_err(status_from_hosted_error)
        }

        async fn rows(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarRowsResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .data()
                .columnar_rows(&auth, &self.workspace, &self.dataset)
                .and_then(|rows| {
                    u64::try_from(rows).map_err(|_| {
                        crate::HostedError::from_error(loom_core::LoomError::corrupt(
                            "columnar row count exceeds u64",
                        ))
                    })
                })
                .map(|rows| Response::new(ColumnarRowsResponse { rows }))
                .map_err(status_from_hosted_error)
        }

        async fn select(
            &self,
            request: Request<ColumnarSelectRequest>,
        ) -> Result<Response<ColumnarCborResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let columns = loom_wire::columnar::select_columns_from_cbor(&input.columns_cbor)
                .map_err(status_from_loom_error)?;
            let filter = loom_wire::columnar::select_filter_from_cbor(&input.filter_cbor)
                .map_err(status_from_loom_error)?;
            let refs = columns.iter().map(String::as_str).collect::<Vec<_>>();
            self.kernel
                .data()
                .columnar_select(&auth, &self.workspace, &self.dataset, &refs, filter)
                .map(|rows| {
                    Response::new(ColumnarCborResponse {
                        cbor: loom_wire::columnar::rows_to_cbor(rows),
                    })
                })
                .map_err(status_from_hosted_error)
        }

        async fn aggregate(
            &self,
            request: Request<ColumnarAggregateRequest>,
        ) -> Result<Response<ColumnarCborResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let aggregates = loom_wire::columnar::aggregates_from_cbor(&input.aggregates_cbor)
                .map_err(status_from_loom_error)?;
            let filter = loom_wire::columnar::select_filter_from_cbor(&input.filter_cbor)
                .map_err(status_from_loom_error)?;
            self.kernel
                .data()
                .columnar_aggregate(&auth, &self.workspace, &self.dataset, &aggregates, filter)
                .map(|values| {
                    Response::new(ColumnarCborResponse {
                        cbor: loom_wire::columnar::values_to_cbor(values),
                    })
                })
                .map_err(status_from_hosted_error)
        }

        async fn export_arrow_ipc(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarBytesResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .data()
                .columnar_export_arrow_ipc(&auth, &self.workspace, &self.dataset)
                .map(|bytes| Response::new(ColumnarBytesResponse { bytes }))
                .map_err(status_from_hosted_error)
        }

        async fn import_arrow_ipc(
            &self,
            request: Request<ColumnarTransferRequest>,
        ) -> Result<Response<ColumnarEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let target_segment_rows = usize::try_from(input.target_segment_rows)
                .map_err(|_| Status::invalid_argument("target_segment_rows is too large"))?;
            self.kernel
                .data()
                .columnar_import_arrow_ipc(
                    &auth,
                    &self.workspace,
                    &self.dataset,
                    &input.bytes,
                    target_segment_rows,
                )
                .map(|_| Response::new(ColumnarEmptyResponse {}))
                .map_err(status_from_hosted_error)
        }

        async fn export_parquet(
            &self,
            request: Request<ColumnarEmptyRequest>,
        ) -> Result<Response<ColumnarBytesResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .data()
                .columnar_export_parquet(&auth, &self.workspace, &self.dataset)
                .map(|bytes| Response::new(ColumnarBytesResponse { bytes }))
                .map_err(status_from_hosted_error)
        }

        async fn import_parquet(
            &self,
            request: Request<ColumnarTransferRequest>,
        ) -> Result<Response<ColumnarEmptyResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let input = request.into_inner();
            let target_segment_rows = usize::try_from(input.target_segment_rows)
                .map_err(|_| Status::invalid_argument("target_segment_rows is too large"))?;
            self.kernel
                .data()
                .columnar_import_parquet(
                    &auth,
                    &self.workspace,
                    &self.dataset,
                    &input.bytes,
                    target_segment_rows,
                )
                .map(|_| Response::new(ColumnarEmptyResponse {}))
                .map_err(status_from_hosted_error)
        }
    }

    #[derive(Clone)]
    pub struct ColumnarServer<T> {
        inner: Arc<T>,
    }

    impl<T> ColumnarServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for ColumnarServer<T>
    where
        T: Columnar,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/loom.hosted.v1.Columnar/Create" => {
                    struct CreateSvc<T>(Arc<T>);
                    impl<T: Columnar> UnaryService<ColumnarCreateRequest> for CreateSvc<T> {
                        type Response = ColumnarEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<ColumnarCreateRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.create(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = CreateSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Columnar/Append" => {
                    struct AppendSvc<T>(Arc<T>);
                    impl<T: Columnar> UnaryService<ColumnarAppendRequest> for AppendSvc<T> {
                        type Response = ColumnarEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<ColumnarAppendRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.append(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = AppendSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Columnar/Compact" => {
                    struct CompactSvc<T>(Arc<T>);
                    impl<T: Columnar> UnaryService<ColumnarEmptyRequest> for CompactSvc<T> {
                        type Response = ColumnarEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<ColumnarEmptyRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.compact(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = CompactSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Columnar/Inspect" => {
                    struct InspectSvc<T>(Arc<T>);
                    impl<T: Columnar> UnaryService<ColumnarEmptyRequest> for InspectSvc<T> {
                        type Response = ColumnarCborResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<ColumnarEmptyRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.inspect(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = InspectSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Columnar/SourceDigest" => {
                    struct SourceDigestSvc<T>(Arc<T>);
                    impl<T: Columnar> UnaryService<ColumnarEmptyRequest> for SourceDigestSvc<T> {
                        type Response = ColumnarCborResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<ColumnarEmptyRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.source_digest(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = SourceDigestSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Columnar/Scan" => {
                    struct ScanSvc<T>(Arc<T>);
                    impl<T: Columnar> UnaryService<ColumnarEmptyRequest> for ScanSvc<T> {
                        type Response = ColumnarCborResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<ColumnarEmptyRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.scan(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ScanSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Columnar/Columns" => {
                    struct ColumnsSvc<T>(Arc<T>);
                    impl<T: Columnar> UnaryService<ColumnarEmptyRequest> for ColumnsSvc<T> {
                        type Response = ColumnarCborResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<ColumnarEmptyRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.columns(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ColumnsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Columnar/Rows" => {
                    struct RowsSvc<T>(Arc<T>);
                    impl<T: Columnar> UnaryService<ColumnarEmptyRequest> for RowsSvc<T> {
                        type Response = ColumnarRowsResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<ColumnarEmptyRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.rows(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = RowsSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Columnar/Select" => {
                    struct SelectSvc<T>(Arc<T>);
                    impl<T: Columnar> UnaryService<ColumnarSelectRequest> for SelectSvc<T> {
                        type Response = ColumnarCborResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<ColumnarSelectRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.select(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = SelectSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Columnar/Aggregate" => {
                    struct AggregateSvc<T>(Arc<T>);
                    impl<T: Columnar> UnaryService<ColumnarAggregateRequest> for AggregateSvc<T> {
                        type Response = ColumnarCborResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<ColumnarAggregateRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.aggregate(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = AggregateSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Columnar/ExportArrowIpc" => {
                    struct ExportArrowIpcSvc<T>(Arc<T>);
                    impl<T: Columnar> UnaryService<ColumnarEmptyRequest> for ExportArrowIpcSvc<T> {
                        type Response = ColumnarBytesResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<ColumnarEmptyRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.export_arrow_ipc(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ExportArrowIpcSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Columnar/ImportArrowIpc" => {
                    struct ImportArrowIpcSvc<T>(Arc<T>);
                    impl<T: Columnar> UnaryService<ColumnarTransferRequest> for ImportArrowIpcSvc<T> {
                        type Response = ColumnarEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<ColumnarTransferRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.import_arrow_ipc(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ImportArrowIpcSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Columnar/ExportParquet" => {
                    struct ExportParquetSvc<T>(Arc<T>);
                    impl<T: Columnar> UnaryService<ColumnarEmptyRequest> for ExportParquetSvc<T> {
                        type Response = ColumnarBytesResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<ColumnarEmptyRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.export_parquet(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ExportParquetSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Columnar/ImportParquet" => {
                    struct ImportParquetSvc<T>(Arc<T>);
                    impl<T: Columnar> UnaryService<ColumnarTransferRequest> for ImportParquetSvc<T> {
                        type Response = ColumnarEmptyResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<ColumnarTransferRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.import_parquet(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ImportParquetSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: Columnar> NamedService for ColumnarServer<T> {
        const NAME: &'static str = "loom.hosted.v1.Columnar";
    }

    #[tonic::async_trait]
    pub trait Sql: Send + Sync + 'static {
        async fn query(
            &self,
            request: Request<SqlRequest>,
        ) -> Result<Response<SqlResponse>, Status>;

        async fn exec(&self, request: Request<SqlRequest>)
        -> Result<Response<SqlResponse>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedSqlGrpcService {
        kernel: HostedKernel,
        workspace: String,
        database: String,
    }

    impl HostedSqlGrpcService {
        pub fn new(
            kernel: HostedKernel,
            workspace: impl Into<String>,
            database: impl Into<String>,
        ) -> Self {
            Self {
                kernel,
                workspace: workspace.into(),
                database: database.into(),
            }
        }
    }

    #[tonic::async_trait]
    impl Sql for HostedSqlGrpcService {
        async fn query(
            &self,
            request: Request<SqlRequest>,
        ) -> Result<Response<SqlResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .sql()
                .query_cbor(
                    &auth,
                    &self.workspace,
                    &self.database,
                    &request.into_inner().sql,
                )
                .map(|cbor| Response::new(SqlResponse { cbor }))
                .map_err(status_from_hosted_error)
        }

        async fn exec(
            &self,
            request: Request<SqlRequest>,
        ) -> Result<Response<SqlResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .sql()
                .exec_cbor(
                    &auth,
                    &self.workspace,
                    &self.database,
                    &request.into_inner().sql,
                )
                .map(|cbor| Response::new(SqlResponse { cbor }))
                .map_err(status_from_hosted_error)
        }
    }

    #[derive(Clone)]
    pub struct SqlServer<T> {
        inner: Arc<T>,
    }

    impl<T> SqlServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for SqlServer<T>
    where
        T: Sql,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/loom.hosted.v1.Sql/Query" => {
                    struct QuerySvc<T>(Arc<T>);
                    impl<T: Sql> UnaryService<SqlRequest> for QuerySvc<T> {
                        type Response = SqlResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<SqlRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.query(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = QuerySvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Sql/Exec" => {
                    struct ExecSvc<T>(Arc<T>);
                    impl<T: Sql> UnaryService<SqlRequest> for ExecSvc<T> {
                        type Response = SqlResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<SqlRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.exec(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = ExecSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move {
                    let mut response = http::Response::new(Body::empty());
                    response
                        .headers_mut()
                        .insert("grpc-status", http::HeaderValue::from_static("12"));
                    response.headers_mut().insert(
                        "content-type",
                        http::HeaderValue::from_static("application/grpc"),
                    );
                    Ok(response)
                }),
            }
        }
    }

    impl<T: Sql> NamedService for SqlServer<T> {
        const NAME: &'static str = "loom.hosted.v1.Sql";
    }

    #[tonic::async_trait]
    pub trait Exec: Send + Sync + 'static {
        async fn run(
            &self,
            request: Request<ExecRequest>,
        ) -> Result<Response<ExecResponse>, Status>;
    }

    #[derive(Clone)]
    pub struct HostedExecGrpcService {
        kernel: HostedKernel,
    }

    impl HostedExecGrpcService {
        pub fn new(kernel: HostedKernel) -> Self {
            Self { kernel }
        }
    }

    #[tonic::async_trait]
    impl Exec for HostedExecGrpcService {
        async fn run(
            &self,
            request: Request<ExecRequest>,
        ) -> Result<Response<ExecResponse>, Status> {
            let auth = hosted_auth(&request)?;
            self.kernel
                .grpc()
                .exec_cbor(&auth, &request.into_inner().request)
                .map(|out| {
                    Response::new(ExecResponse {
                        result: out.message,
                    })
                })
                .map_err(status_from_failure)
        }
    }

    #[derive(Clone)]
    pub struct ExecServer<T> {
        inner: Arc<T>,
    }

    impl<T> ExecServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for ExecServer<T>
    where
        T: Exec,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/loom.hosted.v1.Exec/Run" => {
                    struct RunSvc<T>(Arc<T>);
                    impl<T: Exec> UnaryService<ExecRequest> for RunSvc<T> {
                        type Response = ExecResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<ExecRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.run(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = RunSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                _ => Box::pin(async move {
                    let mut response = http::Response::new(Body::empty());
                    response
                        .headers_mut()
                        .insert("grpc-status", http::HeaderValue::from_static("12"));
                    response.headers_mut().insert(
                        "content-type",
                        http::HeaderValue::from_static("application/grpc"),
                    );
                    Ok(response)
                }),
            }
        }
    }

    impl<T: Exec> NamedService for ExecServer<T> {
        const NAME: &'static str = "loom.hosted.v1.Exec";
    }

    #[tonic::async_trait]
    pub trait Watch: Send + Sync + 'static {
        async fn subscribe(
            &self,
            request: Request<WatchSubscribeRequest>,
        ) -> Result<Response<WatchSubscribeResponse>, Status>;

        async fn poll(
            &self,
            request: Request<WatchPollRequest>,
        ) -> Result<Response<WatchPollResponse>, Status>;

        type StreamStream: Stream<Item = Result<WatchStreamResponse, Status>> + Send + 'static;

        async fn stream(
            &self,
            request: Request<WatchStreamRequest>,
        ) -> Result<Response<Self::StreamStream>, Status>;
    }

    #[derive(Clone)]
    struct WatchStreamState {
        kernel: HostedKernel,
        auth: HostedAuth,
        workspace: WorkspaceId,
        cursor: String,
        max: u32,
        interval: Duration,
        debounce: Duration,
        remaining: Option<usize>,
    }

    #[derive(Clone)]
    pub struct HostedWatchGrpcService {
        kernel: HostedKernel,
        workspace: WorkspaceId,
    }

    impl HostedWatchGrpcService {
        pub fn new(kernel: HostedKernel, workspace: WorkspaceId) -> Self {
            Self { kernel, workspace }
        }
    }

    #[tonic::async_trait]
    impl Watch for HostedWatchGrpcService {
        type StreamStream =
            tokio_stream::wrappers::ReceiverStream<Result<WatchStreamResponse, Status>>;

        async fn subscribe(
            &self,
            request: Request<WatchSubscribeRequest>,
        ) -> Result<Response<WatchSubscribeResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let request = request.into_inner();
            let input = HostedWatchSubscribeInput {
                branch: (!request.branch.is_empty()).then_some(request.branch),
                from: request.from,
                facet: request.facet,
                path_prefix: request.path_prefix,
                change_kinds: request.change_kinds,
            };
            self.kernel
                .grpc()
                .watch_subscribe(&auth, self.workspace, &input)
                .map(|out| {
                    Response::new(WatchSubscribeResponse {
                        cursor: out.message.cursor,
                    })
                })
                .map_err(status_from_failure)
        }

        async fn poll(
            &self,
            request: Request<WatchPollRequest>,
        ) -> Result<Response<WatchPollResponse>, Status> {
            let auth = hosted_auth(&request)?;
            let request = request.into_inner();
            self.kernel
                .grpc()
                .watch_poll(&auth, self.workspace, &request.cursor, request.max)
                .map(|out| watch_batch_response(out.message))
                .map_err(status_from_failure)
        }

        async fn stream(
            &self,
            request: Request<WatchStreamRequest>,
        ) -> Result<Response<Self::StreamStream>, Status> {
            let auth = hosted_auth(&request)?;
            let request = request.into_inner();
            let interval = validate_watch_interval_ms(request.interval_ms.unwrap_or(250))?;
            let debounce = validate_watch_interval_ms(request.debounce_ms.unwrap_or(0))?;
            let limit = match request.limit {
                Some(0) => {
                    return Err(Status::invalid_argument("limit must be greater than 0"));
                }
                Some(value) => Some(
                    usize::try_from(value)
                        .map_err(|_| Status::invalid_argument("limit is too large"))?,
                ),
                None => None,
            };
            let state = WatchStreamState {
                kernel: self.kernel.clone(),
                auth,
                workspace: self.workspace,
                cursor: request.cursor,
                max: request.max,
                interval,
                debounce,
                remaining: limit,
            };
            let (tx, rx) = mpsc::channel::<Result<WatchStreamResponse, Status>>(8);
            tokio::spawn(run_watch_stream(tx, state));
            Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
                rx,
            )))
        }
    }

    fn validate_watch_interval_ms(value_ms: u32) -> Result<Duration, Status> {
        if !(10..=60_000).contains(&value_ms) && value_ms != 0 {
            return Err(Status::invalid_argument(
                "watch stream interval/debounce must be 0 or between 10 and 60000",
            ));
        }
        Ok(Duration::from_millis(u64::from(value_ms)))
    }

    fn watch_stream_response(batch: HostedWatchBatch) -> WatchStreamResponse {
        let next = batch.next.clone();
        WatchStreamResponse {
            source_cursor: next.clone(),
            events: batch
                .events
                .into_iter()
                .map(|event| WatchDataChange {
                    workspace: event.workspace,
                    ref_name: event.ref_name,
                    commit: event.commit,
                    parent: event.parent,
                    seq: event.seq,
                    changes: event
                        .changes
                        .into_iter()
                        .map(|change| WatchDomainChange {
                            domain: change.domain,
                            schema_version: change.schema_version,
                            kind: change.kind,
                            key: change.key,
                            before: change.before,
                            after: change.after,
                            detail: change.detail,
                        })
                        .collect(),
                    unsupported_domains: event
                        .unsupported_domains
                        .into_iter()
                        .map(|domain| WatchUnsupportedDomain {
                            domain: domain.domain,
                            capability: domain.capability,
                        })
                        .collect(),
                })
                .collect(),
            next,
        }
    }

    async fn run_watch_stream(
        tx: mpsc::Sender<Result<WatchStreamResponse, Status>>,
        mut state: WatchStreamState,
    ) {
        loop {
            if state.remaining == Some(0) {
                break;
            }
            if !state.debounce.is_zero() {
                tokio::time::sleep(state.debounce).await;
            }

            match state.kernel.grpc().watch_poll(
                &state.auth,
                state.workspace,
                &state.cursor,
                state.max,
            ) {
                Ok(out) if out.message.events.is_empty() => {
                    if state.interval.is_zero() {
                        break;
                    }
                    tokio::time::sleep(state.interval).await;
                    continue;
                }
                Ok(out) => {
                    let frame = watch_stream_response(out.message);
                    state.cursor = frame.next.clone();
                    if let Some(remaining) = state.remaining.as_mut() {
                        *remaining = remaining.saturating_sub(1);
                    }
                    if tx.send(Ok(frame)).await.is_err() {
                        break;
                    }
                    if state.remaining == Some(0) {
                        break;
                    }
                }
                Err(err) => {
                    let _ = tx.send(Err(status_from_failure(err))).await;
                    break;
                }
            }
        }
    }

    #[derive(Clone)]
    pub struct WatchServer<T> {
        inner: Arc<T>,
    }

    impl<T> WatchServer<T> {
        pub fn new(inner: T) -> Self {
            Self {
                inner: Arc::new(inner),
            }
        }
    }

    impl<T> Service<http::Request<Body>> for WatchServer<T>
    where
        T: Watch,
    {
        type Response = http::Response<Body>;
        type Error = Infallible;
        type Future = BoxFuture<Self::Response, Self::Error>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: http::Request<Body>) -> Self::Future {
            match req.uri().path() {
                "/loom.hosted.v1.Watch/Subscribe" => {
                    struct SubscribeSvc<T>(Arc<T>);
                    impl<T: Watch> UnaryService<WatchSubscribeRequest> for SubscribeSvc<T> {
                        type Response = WatchSubscribeResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(
                            &mut self,
                            request: Request<WatchSubscribeRequest>,
                        ) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.subscribe(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = SubscribeSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Watch/Poll" => {
                    struct PollSvc<T>(Arc<T>);
                    impl<T: Watch> UnaryService<WatchPollRequest> for PollSvc<T> {
                        type Response = WatchPollResponse;
                        type Future = BoxFuture<Response<Self::Response>, Status>;

                        fn call(&mut self, request: Request<WatchPollRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.poll(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = PollSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.unary(method, req).await)
                    })
                }
                "/loom.hosted.v1.Watch/Stream" => {
                    struct StreamSvc<T>(Arc<T>);
                    impl<T: Watch> ServerStreamingService<WatchStreamRequest> for StreamSvc<T> {
                        type Response = WatchStreamResponse;
                        type ResponseStream = T::StreamStream;
                        type Future = BoxFuture<Response<Self::ResponseStream>, Status>;

                        fn call(&mut self, request: Request<WatchStreamRequest>) -> Self::Future {
                            let inner = self.0.clone();
                            Box::pin(async move { inner.stream(request).await })
                        }
                    }
                    let inner = self.inner.clone();
                    Box::pin(async move {
                        let method = StreamSvc(inner);
                        let codec = tonic_prost::ProstCodec::default();
                        let mut grpc = tonic::server::Grpc::new(codec);
                        Ok(grpc.server_streaming(method, req).await)
                    })
                }
                _ => Box::pin(async move { Ok(grpc_unimplemented_response()) }),
            }
        }
    }

    impl<T: Watch> NamedService for WatchServer<T> {
        const NAME: &'static str = "loom.hosted.v1.Watch";
    }

    pub async fn serve_cas_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: WorkspaceId,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let service = HostedCasGrpcService::new(kernel, workspace);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(CasServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_files_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: WorkspaceId,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let service = HostedFilesGrpcService::new(kernel, workspace);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(FilesServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_vcs_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: WorkspaceId,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let service = HostedVcsGrpcService::new(kernel, workspace);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(VcsServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_watch_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: WorkspaceId,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let service = HostedWatchGrpcService::new(kernel, workspace);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(WatchServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_exec_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let service = HostedExecGrpcService::new(kernel);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(ExecServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_sql_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: String,
        database: String,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let service = HostedSqlGrpcService::new(kernel, workspace, database);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(SqlServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_kv_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: String,
        collection: String,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let service = HostedKvGrpcService::new(kernel, workspace, collection);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(KvServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_document_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: String,
        collection: String,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let service = HostedDocumentGrpcService::new(kernel, workspace, collection);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(DocumentServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_fts_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: String,
        collection: String,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let service = HostedFtsGrpcService::new(kernel, workspace, collection);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(FtsServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_graph_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: String,
        graph: String,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let service = HostedGraphGrpcService::new(kernel, workspace, graph);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(GraphServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_queue_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: String,
        stream: String,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let service = HostedQueueGrpcService::new(kernel, workspace, stream);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(QueueServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_ledger_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: String,
        collection: String,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let service = HostedLedgerGrpcService::new(kernel, workspace, collection);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(LedgerServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_time_series_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: String,
        series: String,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let service = HostedTimeSeriesGrpcService::new(kernel, workspace, series);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(TimeSeriesServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_columnar_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: String,
        dataset: String,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let service = HostedColumnarGrpcService::new(kernel, workspace, dataset);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(ColumnarServer::new(service))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_etcd_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: String,
        collection: String,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let kv =
            HostedEtcdKvGrpcService::new(kernel.clone(), workspace.clone(), collection.clone());
        let lease =
            HostedEtcdLeaseGrpcService::new(kernel.clone(), workspace.clone(), collection.clone());
        let watch = HostedEtcdWatchGrpcService::new(kernel, workspace, collection);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(EtcdKvServer::new(kv))
            .add_service(EtcdLeaseServer::new(lease))
            .add_service(EtcdWatchServer::new(watch))
            .add_service(EtcdClusterServer)
            .add_service(EtcdAuthServer)
            .add_service(EtcdMaintenanceServer)
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    pub async fn serve_qdrant_grpc<S>(
        listener: TcpListener,
        kernel: HostedKernel,
        workspace: String,
        collection: String,
        shutdown: S,
    ) -> std::io::Result<()>
    where
        S: std::future::Future<Output = ()> + Send + 'static,
    {
        let collections = HostedQdrantCollectionsGrpcService::new(
            kernel.clone(),
            workspace.clone(),
            collection.clone(),
        );
        let points = HostedQdrantPointsGrpcService::new(kernel, workspace, collection);
        let policy = crate::current_hosted_network_access_policy();
        let incoming = network_access_grpc_incoming(listener, policy.clone());
        tonic::transport::Server::builder()
            .layer(network_access_grpc_layer(policy))
            .add_service(QdrantCollectionsServer::new(collections))
            .add_service(QdrantPointsServer::new(points))
            .serve_with_incoming_shutdown(incoming, shutdown)
            .await
            .map_err(std::io::Error::other)
    }

    #[derive(Clone, Debug)]
    pub struct GrpcNetworkAccessConnectInfo {
        peer_addr: Option<SocketAddr>,
        peer_certificate: Option<crate::HostedPeerCertificate>,
    }

    pub fn grpc_network_access_allows_request(
        policy: Option<&crate::HostedNetworkAccessPolicy>,
        peer_addr: Option<SocketAddr>,
        peer_certificate: Option<&crate::HostedPeerCertificate>,
        x_forwarded_for: Option<&str>,
        forwarded: Option<&str>,
    ) -> bool {
        let Some(policy) = policy else {
            return true;
        };
        let Some(peer_addr) = peer_addr else {
            return false;
        };
        crate::network_access_allows(
            Some(policy),
            peer_addr,
            peer_certificate,
            x_forwarded_for,
            forwarded,
        )
    }

    struct NetworkAccessGrpcStream {
        inner: tokio::net::TcpStream,
        connect_info: GrpcNetworkAccessConnectInfo,
    }

    impl AsyncRead for NetworkAccessGrpcStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_read(cx, buf)
        }
    }

    impl AsyncWrite for NetworkAccessGrpcStream {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Pin::new(&mut self.inner).poll_write(cx, buf)
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_flush(cx)
        }

        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }

    impl Connected for NetworkAccessGrpcStream {
        type ConnectInfo = GrpcNetworkAccessConnectInfo;

        fn connect_info(&self) -> Self::ConnectInfo {
            self.connect_info.clone()
        }
    }

    fn network_access_grpc_incoming(
        listener: TcpListener,
        policy: Option<crate::HostedNetworkAccessPolicy>,
    ) -> impl Stream<Item = std::io::Result<NetworkAccessGrpcStream>> {
        tokio_stream::wrappers::TcpListenerStream::new(listener).filter_map(move |accepted| {
            let policy = policy.clone();
            match accepted {
                Ok(stream) => {
                    let peer_addr = stream.peer_addr().ok();
                    let requires_request_context = policy
                        .as_ref()
                        .is_some_and(|policy| policy.requires_request_context());
                    let allowed = match (policy.as_ref(), peer_addr) {
                        (None, _) => true,
                        (Some(_), Some(_)) if requires_request_context => true,
                        (Some(policy), Some(addr)) => {
                            crate::network_access_allows(Some(policy), addr, None, None, None)
                        }
                        (Some(_), None) => false,
                    };
                    if allowed {
                        Some(Ok(NetworkAccessGrpcStream {
                            inner: stream,
                            connect_info: GrpcNetworkAccessConnectInfo {
                                peer_addr,
                                peer_certificate: None,
                            },
                        }))
                    } else {
                        None
                    }
                }
                Err(err) => Some(Err(err)),
            }
        })
    }

    fn network_access_grpc_layer(
        policy: Option<crate::HostedNetworkAccessPolicy>,
    ) -> tonic::service::InterceptorLayer<
        impl FnMut(Request<()>) -> Result<Request<()>, Status> + Clone,
    > {
        tonic::service::InterceptorLayer::new(move |request| {
            network_access_grpc_intercept(policy.as_ref(), request)
        })
    }

    fn network_access_grpc_intercept(
        policy: Option<&crate::HostedNetworkAccessPolicy>,
        request: Request<()>,
    ) -> Result<Request<()>, Status> {
        let Some(policy) = policy else {
            return Ok(request);
        };
        if !policy.requires_request_context() {
            return Ok(request);
        }
        let connect_info = request.extensions().get::<GrpcNetworkAccessConnectInfo>();
        let peer_addr = connect_info.and_then(|info| info.peer_addr);
        let peer_certificate = connect_info.and_then(|info| info.peer_certificate.as_ref());
        let x_forwarded_for = request
            .metadata()
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok());
        let forwarded = request
            .metadata()
            .get("forwarded")
            .and_then(|value| value.to_str().ok());
        if grpc_network_access_allows_request(
            Some(policy),
            peer_addr,
            peer_certificate,
            x_forwarded_for,
            forwarded,
        ) {
            Ok(request)
        } else {
            Err(Status::permission_denied("network access denied"))
        }
    }

    fn etcd_header(revision: i64) -> EtcdResponseHeader {
        EtcdResponseHeader {
            cluster_id: 1,
            member_id: 1,
            revision,
            raft_term: 1,
        }
    }

    fn ensure_etcd_range_request(input: &EtcdRangeRequest) -> Result<(), Status> {
        if input.key.is_empty() && input.range_end.is_empty() {
            return Err(Status::invalid_argument("etcd range key is required"));
        }
        if input.sort_order != EtcdRangeSortOrder::None as i32
            || input.sort_target != EtcdRangeSortTarget::Key as i32
        {
            return Err(Status::unimplemented("etcd range sorting is not supported"));
        }
        if input.min_mod_revision != 0
            || input.max_mod_revision != 0
            || input.min_create_revision != 0
            || input.max_create_revision != 0
        {
            return Err(Status::unimplemented(
                "etcd range revision filters are not supported",
            ));
        }
        Ok(())
    }

    fn etcd_key_value(kv: crate::HostedEtcdKv) -> EtcdKeyValue {
        EtcdKeyValue {
            key: kv.key,
            create_revision: kv.create_revision,
            mod_revision: kv.mod_revision,
            version: kv.version,
            value: kv.value,
            lease: kv.lease,
        }
    }

    fn etcd_range_response(
        result: crate::HostedEtcdRangeResult,
        count_only: bool,
        keys_only: bool,
    ) -> EtcdRangeResponse {
        let returned = result.kvs.len() as i64;
        let more = !count_only && result.count > returned;
        let kvs = if count_only {
            Vec::new()
        } else {
            result
                .kvs
                .into_iter()
                .map(|mut kv| {
                    if keys_only {
                        kv.value.clear();
                    }
                    etcd_key_value(kv)
                })
                .collect()
        };
        EtcdRangeResponse {
            header: Some(etcd_header(result.revision)),
            kvs,
            more,
            count: result.count,
        }
    }

    async fn send_etcd_watch_response(
        tx: &mpsc::Sender<Result<EtcdWatchResponse, Status>>,
        kernel: &HostedKernel,
        auth: &HostedAuth,
        workspace: &str,
        collection: &str,
        request: EtcdWatchRequest,
    ) -> Result<(), ()> {
        let response = match request
            .request_union
            .ok_or_else(|| Status::invalid_argument("etcd watch request is required"))
        {
            Ok(etcd_watch_request::RequestUnion::CreateRequest(create)) => {
                if create.key.is_empty() {
                    Err(Status::invalid_argument("etcd watch key is required"))
                } else if create.fragment {
                    Err(Status::unimplemented(
                        "etcd watch fragmented responses are not supported",
                    ))
                } else {
                    let watch_id = if create.watch_id == 0 {
                        1
                    } else {
                        create.watch_id
                    };
                    let result = match kernel
                        .data()
                        .etcd_watch_events(
                            auth,
                            workspace,
                            collection,
                            create.key,
                            create.range_end,
                            create.start_revision,
                        )
                        .map_err(status_from_hosted_error)
                    {
                        Ok(result) => result,
                        Err(err) => return tx.send(Err(err)).await.map_err(|_| ()),
                    };
                    let created = EtcdWatchResponse {
                        header: Some(etcd_header(result.revision)),
                        watch_id,
                        created: true,
                        canceled: false,
                        compact_revision: result.compacted_revision,
                        cancel_reason: String::new(),
                        events: Vec::new(),
                    };
                    if tx.send(Ok(created)).await.is_err() {
                        return Err(());
                    }
                    Ok(EtcdWatchResponse {
                        header: Some(etcd_header(result.revision)),
                        watch_id,
                        created: false,
                        canceled: false,
                        compact_revision: result.compacted_revision,
                        cancel_reason: String::new(),
                        events: etcd_watch_events(result, create.prev_kv, &create.filters),
                    })
                }
            }
            Ok(etcd_watch_request::RequestUnion::CancelRequest(cancel)) => Ok(EtcdWatchResponse {
                header: Some(etcd_header(0)),
                watch_id: cancel.watch_id,
                created: false,
                canceled: true,
                compact_revision: 0,
                cancel_reason: String::new(),
                events: Vec::new(),
            }),
            Ok(etcd_watch_request::RequestUnion::ProgressRequest(_)) => Ok(EtcdWatchResponse {
                header: Some(etcd_header(0)),
                watch_id: 0,
                created: false,
                canceled: false,
                compact_revision: 0,
                cancel_reason: String::new(),
                events: Vec::new(),
            }),
            Err(err) => Err(err),
        };
        tx.send(response).await.map_err(|_| ())
    }

    fn etcd_watch_events(
        result: crate::HostedEtcdWatchResult,
        include_prev_kv: bool,
        filters: &[i32],
    ) -> Vec<EtcdEvent> {
        let mut out = Vec::new();
        for event in result.events {
            let event_type = match event.kind {
                crate::HostedEtcdEventKind::Put => {
                    if filters.contains(&(EtcdWatchFilterType::Noput as i32)) {
                        continue;
                    }
                    EtcdEventType::Put
                }
                crate::HostedEtcdEventKind::Delete => {
                    if filters.contains(&(EtcdWatchFilterType::Nodelete as i32)) {
                        continue;
                    }
                    EtcdEventType::Delete
                }
            };
            out.push(EtcdEvent {
                r#type: event_type as i32,
                kv: Some(etcd_key_value(event.kv)),
                prev_kv: include_prev_kv
                    .then(|| event.prev_kv.map(etcd_key_value))
                    .flatten(),
            });
        }
        out
    }

    fn etcd_compare_from_proto(input: EtcdCompare) -> Result<DataEtcdCompare, Status> {
        if !input.range_end.is_empty() {
            return Err(Status::unimplemented(
                "range compare predicates are not supported",
            ));
        }
        let result = match EtcdCompareResult::try_from(input.result)
            .map_err(|_| Status::invalid_argument("invalid etcd compare result"))?
        {
            EtcdCompareResult::Equal => HostedEtcdCompareResult::Equal,
            EtcdCompareResult::Greater => HostedEtcdCompareResult::Greater,
            EtcdCompareResult::Less => HostedEtcdCompareResult::Less,
            EtcdCompareResult::NotEqual => HostedEtcdCompareResult::NotEqual,
        };
        let target = match EtcdCompareTarget::try_from(input.target)
            .map_err(|_| Status::invalid_argument("invalid etcd compare target"))?
        {
            EtcdCompareTarget::Version => HostedEtcdCompareTarget::Version(input.version),
            EtcdCompareTarget::Create => {
                HostedEtcdCompareTarget::CreateRevision(input.create_revision)
            }
            EtcdCompareTarget::Mod => HostedEtcdCompareTarget::ModRevision(input.mod_revision),
            EtcdCompareTarget::Value => HostedEtcdCompareTarget::Value(input.value),
            EtcdCompareTarget::Lease => HostedEtcdCompareTarget::Lease(input.lease),
        };
        Ok(DataEtcdCompare {
            key: input.key,
            result,
            target,
        })
    }

    fn etcd_request_op_from_proto(input: EtcdRequestOp) -> Result<HostedEtcdRequestOp, Status> {
        let request = input
            .request
            .ok_or_else(|| Status::invalid_argument("etcd request op is required"))?;
        match request {
            etcd_request_op::Request::RequestRange(range) => {
                ensure_etcd_range_request(&range)?;
                Ok(HostedEtcdRequestOp::Range {
                    key: range.key,
                    range_end: range.range_end,
                    limit: range.limit,
                    revision: range.revision,
                })
            }
            etcd_request_op::Request::RequestPut(put) => {
                if put.ignore_value || put.ignore_lease {
                    return Err(Status::unimplemented(
                        "etcd put ignore_value and ignore_lease are not supported",
                    ));
                }
                if put.key.is_empty() {
                    return Err(Status::invalid_argument("etcd put key is required"));
                }
                Ok(HostedEtcdRequestOp::Put {
                    key: put.key,
                    value: put.value,
                    lease: put.lease,
                    prev_kv: put.prev_kv,
                })
            }
            etcd_request_op::Request::RequestDeleteRange(delete) => {
                if delete.key.is_empty() {
                    return Err(Status::invalid_argument("etcd delete key is required"));
                }
                Ok(HostedEtcdRequestOp::DeleteRange {
                    key: delete.key,
                    range_end: delete.range_end,
                    prev_kv: delete.prev_kv,
                })
            }
            etcd_request_op::Request::RequestTxn(_) => Err(Status::unimplemented(
                "nested etcd transactions are not supported",
            )),
        }
    }

    fn etcd_response_op_from_data(input: HostedEtcdResponseOp) -> EtcdResponseOp {
        let response = match input {
            HostedEtcdResponseOp::Range(result) => {
                etcd_response_op::Response::ResponseRange(etcd_range_response(result, false, false))
            }
            HostedEtcdResponseOp::Put(result) => {
                etcd_response_op::Response::ResponsePut(EtcdPutResponse {
                    header: Some(etcd_header(result.revision)),
                    prev_kv: result.prev_kv.map(etcd_key_value),
                })
            }
            HostedEtcdResponseOp::DeleteRange(result) => {
                etcd_response_op::Response::ResponseDeleteRange(EtcdDeleteRangeResponse {
                    header: Some(etcd_header(result.revision)),
                    deleted: result.deleted,
                    prev_kvs: result.prev_kvs.into_iter().map(etcd_key_value).collect(),
                })
            }
        };
        EtcdResponseOp {
            response: Some(response),
        }
    }

    fn ensure_qdrant_collection(served: &str, requested: &str) -> Result<(), Status> {
        if served == requested {
            Ok(())
        } else {
            Err(Status::not_found(
                "collection is not served by this listener",
            ))
        }
    }

    fn qdrant_metric(value: &str) -> Result<loom_core::Metric, Status> {
        match value {
            "" | "Cosine" | "cosine" => Ok(loom_core::Metric::Cosine),
            "Euclid" | "euclid" | "L2" | "l2" => Ok(loom_core::Metric::L2),
            "Dot" | "dot" => Ok(loom_core::Metric::Dot),
            _ => Err(Status::invalid_argument(
                "distance must be Cosine, Euclid, or Dot",
            )),
        }
    }

    fn qdrant_metric_name(metric: loom_core::Metric) -> &'static str {
        match metric {
            loom_core::Metric::Cosine => "Cosine",
            loom_core::Metric::L2 => "Euclid",
            loom_core::Metric::Dot => "Dot",
        }
    }

    fn qdrant_limit(value: u64) -> Result<usize, Status> {
        let value = if value == 0 { 10 } else { value };
        usize::try_from(value).map_err(|_| Status::invalid_argument("limit is too large"))
    }

    fn qdrant_point_id(value: &str) -> QdrantPointId {
        if let Ok(value) = value.parse::<u64>() {
            QdrantPointId {
                point_id_options: Some(qdrant_point_id::PointIdOptions::Num(value)),
            }
        } else {
            QdrantPointId {
                point_id_options: Some(qdrant_point_id::PointIdOptions::Uuid(value.to_string())),
            }
        }
    }

    fn qdrant_point_id_string(value: Option<&QdrantPointId>) -> Result<String, Status> {
        match value.and_then(|value| value.point_id_options.as_ref()) {
            Some(qdrant_point_id::PointIdOptions::Num(value)) => Ok(value.to_string()),
            Some(qdrant_point_id::PointIdOptions::Uuid(value)) if !value.is_empty() => {
                Ok(value.clone())
            }
            _ => Err(Status::invalid_argument("point id is required")),
        }
    }

    fn watch_batch_response(batch: HostedWatchBatch) -> Response<WatchPollResponse> {
        Response::new(WatchPollResponse {
            events: batch
                .events
                .into_iter()
                .map(|event| WatchDataChange {
                    workspace: event.workspace,
                    ref_name: event.ref_name,
                    commit: event.commit,
                    parent: event.parent,
                    seq: event.seq,
                    changes: event
                        .changes
                        .into_iter()
                        .map(|change| WatchDomainChange {
                            domain: change.domain,
                            schema_version: change.schema_version,
                            kind: change.kind,
                            key: change.key,
                            before: change.before,
                            after: change.after,
                            detail: change.detail,
                        })
                        .collect(),
                    unsupported_domains: event
                        .unsupported_domains
                        .into_iter()
                        .map(|domain| WatchUnsupportedDomain {
                            domain: domain.domain,
                            capability: domain.capability,
                        })
                        .collect(),
                })
                .collect(),
            next: batch.next,
        })
    }

    type QdrantPointVectors = Vec<(Option<String>, Vec<f32>)>;

    fn qdrant_point_vectors(point: &QdrantPointStruct) -> Result<QdrantPointVectors, Status> {
        let mut out = Vec::new();
        if !point.vector.is_empty() {
            out.push((None, point.vector.clone()));
        }
        for named in &point.named_vectors {
            if named.name.is_empty() {
                return Err(Status::invalid_argument("named vector must not be empty"));
            }
            if named.data.is_empty() {
                return Err(Status::invalid_argument(
                    "named vector data must not be empty",
                ));
            }
            out.push((Some(named.name.clone()), named.data.clone()));
        }
        if out.is_empty() {
            Err(Status::invalid_argument("point vector is required"))
        } else {
            Ok(out)
        }
    }

    fn qdrant_payload(
        entries: Vec<QdrantPayloadEntry>,
    ) -> Result<BTreeMap<String, CellValue>, Status> {
        entries
            .into_iter()
            .map(|entry| {
                if entry.key.is_empty() {
                    return Err(Status::invalid_argument("payload key must not be empty"));
                }
                let value = qdrant_payload_value(entry.value.as_ref())?;
                Ok((entry.key, value))
            })
            .collect()
    }

    fn qdrant_payload_value(value: Option<&QdrantValue>) -> Result<CellValue, Status> {
        match value.and_then(|value| value.kind.as_ref()) {
            Some(qdrant_value::Kind::NullValue(_)) | None => Ok(CellValue::Null),
            Some(qdrant_value::Kind::BoolValue(value)) => Ok(CellValue::Bool(*value)),
            Some(qdrant_value::Kind::IntegerValue(value)) => Ok(CellValue::Int(*value)),
            Some(qdrant_value::Kind::DoubleValue(value)) if value.is_finite() => {
                Ok(CellValue::Float(*value))
            }
            Some(qdrant_value::Kind::DoubleValue(_)) => {
                Err(Status::invalid_argument("payload double must be finite"))
            }
            Some(qdrant_value::Kind::StringValue(value)) => Ok(CellValue::Text(value.clone())),
        }
    }

    fn qdrant_point_result(
        id: String,
        entry: crate::HostedVectorEntry,
        with_payload: bool,
        with_vectors: bool,
        score: Option<f32>,
    ) -> QdrantPointResult {
        QdrantPointResult {
            id: Some(qdrant_point_id(&id)),
            vector: if with_vectors {
                entry.vector
            } else {
                Vec::new()
            },
            payload: if with_payload {
                entry
                    .metadata
                    .into_iter()
                    .map(|(key, value)| QdrantPayloadEntry {
                        key,
                        value: Some(qdrant_value_from_cell(value)),
                    })
                    .collect()
            } else {
                Vec::new()
            },
            score: score.unwrap_or(0.0),
            found: true,
        }
    }

    fn qdrant_value_from_cell(value: CellValue) -> QdrantValue {
        let kind = match value {
            CellValue::Null => qdrant_value::Kind::NullValue(true),
            CellValue::Bool(value) => qdrant_value::Kind::BoolValue(value),
            CellValue::Int(value) => qdrant_value::Kind::IntegerValue(value),
            CellValue::Float(value) => qdrant_value::Kind::DoubleValue(value),
            CellValue::Text(value) => qdrant_value::Kind::StringValue(value),
            other => qdrant_value::Kind::StringValue(format!("{other:?}")),
        };
        QdrantValue { kind: Some(kind) }
    }

    fn qdrant_filter_from_proto(
        filter: Option<&QdrantFilter>,
    ) -> Result<loom_core::vector::MetaFilter, Status> {
        let Some(filter) = filter else {
            return Ok(loom_core::vector::MetaFilter::All);
        };
        let mut object = serde_json::Map::new();
        if !filter.must.is_empty() {
            object.insert(
                "must".to_string(),
                Value::Array(qdrant_conditions_json(&filter.must)?),
            );
        }
        if !filter.should.is_empty() {
            object.insert(
                "should".to_string(),
                Value::Array(qdrant_conditions_json(&filter.should)?),
            );
        }
        if !filter.must_not.is_empty() {
            object.insert(
                "must_not".to_string(),
                Value::Array(qdrant_conditions_json(&filter.must_not)?),
            );
        }
        let value = Value::Object(object);
        crate::vector_compat::qdrant_filter_from_json(&value).map_err(status_from_loom_error)
    }

    fn qdrant_conditions_json(conditions: &[QdrantCondition]) -> Result<Vec<Value>, Status> {
        conditions.iter().map(qdrant_condition_json).collect()
    }

    fn qdrant_condition_json(condition: &QdrantCondition) -> Result<Value, Status> {
        if condition.key.is_empty() {
            return Err(Status::invalid_argument("filter condition key is required"));
        }
        match condition.kind.as_ref() {
            Some(qdrant_condition::Kind::Match(value)) => {
                let mut object = serde_json::Map::new();
                if let Some(value) = value.value.as_ref() {
                    object.insert("value".to_string(), qdrant_value_json(value)?);
                }
                if !value.any.is_empty() {
                    object.insert(
                        "any".to_string(),
                        Value::Array(
                            value
                                .any
                                .iter()
                                .map(qdrant_value_json)
                                .collect::<Result<Vec<_>, _>>()?,
                        ),
                    );
                }
                if !value.except.is_empty() {
                    object.insert(
                        "except".to_string(),
                        Value::Array(
                            value
                                .except
                                .iter()
                                .map(qdrant_value_json)
                                .collect::<Result<Vec<_>, _>>()?,
                        ),
                    );
                }
                Ok(json!({ "key": condition.key, "match": Value::Object(object) }))
            }
            Some(qdrant_condition::Kind::Range(value)) => {
                let mut object = serde_json::Map::new();
                if let Some(value) = value.gt.as_ref() {
                    object.insert("gt".to_string(), qdrant_value_json(value)?);
                }
                if let Some(value) = value.gte.as_ref() {
                    object.insert("gte".to_string(), qdrant_value_json(value)?);
                }
                if let Some(value) = value.lt.as_ref() {
                    object.insert("lt".to_string(), qdrant_value_json(value)?);
                }
                if let Some(value) = value.lte.as_ref() {
                    object.insert("lte".to_string(), qdrant_value_json(value)?);
                }
                Ok(json!({ "key": condition.key, "range": Value::Object(object) }))
            }
            Some(qdrant_condition::Kind::IsEmpty(true)) => {
                Ok(json!({ "is_empty": { "key": condition.key } }))
            }
            Some(qdrant_condition::Kind::IsNull(true)) => {
                Ok(json!({ "is_null": { "key": condition.key } }))
            }
            Some(qdrant_condition::Kind::Unsupported(value)) => Err(Status::unimplemented(
                format!("unsupported vector compatibility filter: {value}"),
            )),
            _ => Err(Status::invalid_argument(
                "filter condition must contain a supported operator",
            )),
        }
    }

    fn qdrant_value_json(value: &QdrantValue) -> Result<Value, Status> {
        match value.kind.as_ref() {
            Some(qdrant_value::Kind::NullValue(_)) | None => Ok(Value::Null),
            Some(qdrant_value::Kind::BoolValue(value)) => Ok(Value::Bool(*value)),
            Some(qdrant_value::Kind::IntegerValue(value)) => Ok(json!(value)),
            Some(qdrant_value::Kind::DoubleValue(value)) if value.is_finite() => Ok(json!(value)),
            Some(qdrant_value::Kind::DoubleValue(_)) => {
                Err(Status::invalid_argument("filter double must be finite"))
            }
            Some(qdrant_value::Kind::StringValue(value)) => Ok(Value::String(value.clone())),
        }
    }

    fn grpc_unimplemented_response() -> http::Response<Body> {
        let mut response = http::Response::new(Body::empty());
        response
            .headers_mut()
            .insert("grpc-status", http::HeaderValue::from_static("12"));
        response.headers_mut().insert(
            "content-type",
            http::HeaderValue::from_static("application/grpc"),
        );
        response
    }

    fn hosted_auth<T>(request: &Request<T>) -> Result<HostedAuth, Status> {
        let principal = metadata_string(request, "x-loom-principal")?;
        let passphrase = metadata_string(request, "x-loom-passphrase")?;
        let app_credential = hosted_api_key(request)?;
        let external_credential = hosted_external_credential(request)?;
        let session =
            metadata_string(request, "x-loom-session")?.unwrap_or_else(|| "grpc".to_string());
        match (principal, passphrase, app_credential, external_credential) {
            (None, None, None, None) => Ok(HostedAuth::unauthenticated()),
            (Some(principal), Some(passphrase), None, None) => {
                let principal = WorkspaceId::parse(&principal)
                    .map_err(|err| Status::invalid_argument(err.to_string()))?;
                Ok(HostedAuth::passphrase(principal, passphrase, session))
            }
            (None, None, Some(app_credential), None) => {
                Ok(HostedAuth::app_credential(app_credential, session))
            }
            (None, None, None, Some(external_credential)) => {
                Ok(HostedAuth::verified_external(external_credential, session))
            }
            _ => Err(Status::invalid_argument(
                "supply one hosted credential: passphrase, API key, or verified external assertion",
            )),
        }
    }

    fn hosted_api_key<T>(request: &Request<T>) -> Result<Option<String>, Status> {
        let candidates = [
            metadata_string(request, "x-loom-api-key")?,
            metadata_string(request, "x-api-key")?,
            metadata_string(request, "api-key")?,
            metadata_string(request, "x-pinecone-api-key")?,
            authorization_bearer(request)?,
        ];
        let mut out = None;
        for value in candidates.into_iter().flatten() {
            if out.is_some() {
                return Err(Status::invalid_argument(
                    "only one API-key credential metadata entry may be supplied",
                ));
            }
            out = Some(value);
        }
        Ok(out)
    }

    fn hosted_external_credential<T>(
        request: &Request<T>,
    ) -> Result<Option<loom_store::VerifiedExternalCredential>, Status> {
        let kind = metadata_string(request, "x-loom-external-kind")?;
        let issuer = metadata_string(request, "x-loom-external-issuer")?;
        let subject = metadata_string(request, "x-loom-external-subject")?;
        let material_digest = metadata_string(request, "x-loom-external-material-digest")?;
        let proof_kind = metadata_string(request, "x-loom-external-proof-kind")?;
        let proof = metadata_string(request, "x-loom-external-proof")?;
        let challenge = metadata_string(request, "x-loom-external-challenge")?;
        let peer_certificate_der = request
            .peer_certs()
            .and_then(|certs| certs.first().map(|cert| cert.as_ref().to_vec()));
        hosted_external_credential_from_parts(HostedExternalCredentialParts {
            kind,
            issuer,
            subject,
            material_digest,
            proof_kind,
            proof,
            challenge,
            peer_certificate_der: peer_certificate_der.as_deref(),
        })
        .map_err(|err| {
            if err.code == loom_core::Code::Unsupported {
                Status::unimplemented(err.to_string())
            } else {
                Status::invalid_argument(err.to_string())
            }
        })
    }

    fn authorization_bearer<T>(request: &Request<T>) -> Result<Option<String>, Status> {
        let Some(value) = metadata_string(request, "authorization")? else {
            return Ok(None);
        };
        let Some(token) = value.strip_prefix("Bearer ") else {
            return Err(Status::invalid_argument(
                "authorization metadata must use Bearer token syntax",
            ));
        };
        if token.is_empty() {
            return Err(Status::invalid_argument(
                "authorization bearer token is empty",
            ));
        }
        Ok(Some(token.to_string()))
    }

    fn metadata_string<T>(request: &Request<T>, key: &str) -> Result<Option<String>, Status> {
        request
            .metadata()
            .get(key)
            .map(|value| {
                value
                    .to_str()
                    .map(|value| value.to_string())
                    .map_err(|err| Status::invalid_argument(err.to_string()))
            })
            .transpose()
    }

    fn parse_digest(value: String) -> Result<Digest, Status> {
        Digest::parse(&value).map_err(|err| Status::invalid_argument(err.to_string()))
    }

    fn grpc_now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis() as u64)
    }

    fn status_from_loom_error(err: loom_core::LoomError) -> Status {
        status_from_hosted_error(crate::HostedError::from_error(err))
    }

    fn status_from_failure(err: GrpcFailure) -> Status {
        let code = match err.status {
            GrpcStatusCode::InvalidArgument => tonic::Code::InvalidArgument,
            GrpcStatusCode::Unauthenticated => tonic::Code::Unauthenticated,
            GrpcStatusCode::PermissionDenied => tonic::Code::PermissionDenied,
            GrpcStatusCode::NotFound => tonic::Code::NotFound,
            GrpcStatusCode::AlreadyExists => tonic::Code::AlreadyExists,
            GrpcStatusCode::FailedPrecondition => tonic::Code::FailedPrecondition,
            GrpcStatusCode::Aborted => tonic::Code::Aborted,
            GrpcStatusCode::Unimplemented => tonic::Code::Unimplemented,
            GrpcStatusCode::OutOfRange => tonic::Code::OutOfRange,
            GrpcStatusCode::Unavailable => tonic::Code::Unavailable,
            GrpcStatusCode::Internal => tonic::Code::Internal,
        };
        let mut status = Status::new(code, err.error.message);
        if let Ok(value) = err.error.code_name.parse() {
            status.metadata_mut().insert("x-loom-code", value);
        }
        status
    }

    fn status_from_hosted_error(err: crate::HostedError) -> Status {
        status_from_failure(GrpcFailure {
            status: super::grpc_status(err.code),
            error: err,
        })
    }
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use std::fs;

    use loom_core::{Code, Digest};
    #[cfg(feature = "grpc")]
    use loom_core::{ExternalCredentialKind, ExternalCredentialSpec};
    #[cfg(feature = "grpc")]
    use loom_store::FileStore;
    #[cfg(feature = "grpc")]
    use tokio_stream::StreamExt;
    #[cfg(feature = "grpc")]
    use tonic::Status;

    use crate::test_support::{init, nid, temp_path, watch_history};
    use crate::{GrpcStatusCode, HostedAuth, HostedKernel, HostedWatchSubscribeInput};

    #[test]
    fn grpc_read_write_share_hosted_auth_and_pep() {
        let path = temp_path("grpc");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let grpc = kernel.grpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "grpc-1");

        grpc.write_file(&auth, ns, "notes.txt", b"grpc").unwrap();
        let result = grpc.read_file(&auth, ns, "notes.txt").unwrap();
        assert_eq!(result.message, b"grpc".to_vec());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn grpc_error_preserves_loom_code_data() {
        let path = temp_path("grpc-error");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let err = kernel
            .grpc()
            .read_file(&HostedAuth::unauthenticated(), ns, "notes.txt")
            .unwrap_err();
        assert_eq!(err.status, GrpcStatusCode::Unauthenticated);
        assert_eq!(err.error.code, Code::AuthenticationFailed);
        assert_eq!(err.error.code_name, "AUTHENTICATION_FAILED");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn grpc_exec_cbor_maps_invalid_request() {
        let path = temp_path("grpc-exec");
        let _ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "grpc-exec-root");

        let err = kernel.grpc().exec_cbor(&auth, b"not-cbor").unwrap_err();
        assert_eq!(err.status, GrpcStatusCode::InvalidArgument);
        assert_eq!(err.error.code, Code::InvalidArgument);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn grpc_cas_facade_round_trips() {
        let path = temp_path("grpc-cas");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let grpc = kernel.grpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "grpc-cas-1");

        let put = grpc.put_cas(&auth, ns, b"grpc-cas").unwrap();
        let digest = Digest::parse(&put.message).unwrap();
        assert!(grpc.has_cas(&auth, ns, &digest).unwrap().message);
        assert_eq!(
            grpc.get_cas(&auth, ns, &digest).unwrap().message,
            Some(b"grpc-cas".to_vec())
        );
        assert_eq!(
            grpc.list_cas(&auth, ns).unwrap().message,
            vec![put.message.clone()]
        );
        assert!(grpc.delete_cas(&auth, ns, &digest).unwrap().message);
        assert!(!grpc.has_cas(&auth, ns, &digest).unwrap().message);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn grpc_watch_subscribe_and_poll_project_domain_changes() {
        let path = temp_path("grpc-watch");
        let (ns, c0, c1) = watch_history(&path);
        let kernel = HostedKernel::new(&path);
        let grpc = kernel.grpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "grpc-watch-root");

        let sub = grpc
            .watch_subscribe(
                &auth,
                ns,
                &HostedWatchSubscribeInput {
                    branch: Some("main".to_string()),
                    from: Some(c0.to_string()),
                    facet: Some("files".to_string()),
                    path_prefix: Some("b.".to_string()),
                    change_kinds: vec!["added".to_string()],
                },
            )
            .unwrap();
        let batch = grpc.watch_poll(&auth, ns, &sub.message.cursor, 10).unwrap();

        assert_eq!(batch.message.events.len(), 1);
        assert_eq!(batch.message.events[0].commit, c1.to_string());
        assert_eq!(batch.message.events[0].changes.len(), 1);
        assert_eq!(batch.message.events[0].changes[0].domain, "files");
        assert_eq!(batch.message.events[0].changes[0].kind, "added");
        assert_eq!(batch.message.events[0].changes[0].key_hex, "622e747874");
        assert!(batch.message.events[0].unsupported_domains.is_empty());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn grpc_watch_poll_rejects_out_of_bounds_max() {
        let path = temp_path("grpc-watch-max");
        let (ns, c0, _) = watch_history(&path);
        let kernel = HostedKernel::new(&path);
        let grpc = kernel.grpc();
        let auth = HostedAuth::passphrase(nid(1), "root-pass", "grpc-watch-max-root");
        let sub = grpc
            .watch_subscribe(
                &auth,
                ns,
                &HostedWatchSubscribeInput {
                    branch: None,
                    from: Some(c0.to_string()),
                    facet: None,
                    path_prefix: None,
                    change_kinds: Vec::new(),
                },
            )
            .unwrap();

        let err = grpc
            .watch_poll(&auth, ns, &sub.message.cursor, 0)
            .unwrap_err();
        assert_eq!(err.status, GrpcStatusCode::InvalidArgument);
        assert_eq!(err.error.code, Code::InvalidArgument);
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_cas_service_uses_metadata_auth() {
        let path = temp_path("grpc-cas-service");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let service = crate::grpc::service::HostedCasGrpcService::new(kernel, ns);
        let mut request = tonic::Request::new(crate::grpc::service::CasPutRequest {
            bytes: b"service".to_vec(),
        });
        request
            .metadata_mut()
            .insert("x-loom-principal", nid(1).to_string().parse().unwrap());
        request
            .metadata_mut()
            .insert("x-loom-passphrase", "root-pass".parse().unwrap());
        request
            .metadata_mut()
            .insert("x-loom-session", "grpc-test".parse().unwrap());
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let response = runtime
            .block_on(crate::grpc::service::Cas::put(&service, request))
            .unwrap();
        assert!(Digest::parse(&response.into_inner().digest).is_ok());
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_files_service_covers_native_tree_operations() {
        let path = temp_path("grpc-files-service");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let service = crate::grpc::service::HostedFilesGrpcService::new(kernel, ns);
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime
            .block_on(crate::grpc::service::Files::mkdir(
                &service,
                qdrant_auth_request(crate::grpc::service::FilesMkdirRequest {
                    path: "docs".to_string(),
                    recursive: false,
                }),
            ))
            .unwrap();
        runtime
            .block_on(crate::grpc::service::Files::write(
                &service,
                qdrant_auth_request(crate::grpc::service::FilesWriteRequest {
                    path: "docs/a.txt".to_string(),
                    bytes: b"alpha".to_vec(),
                }),
            ))
            .unwrap();

        let read = runtime
            .block_on(crate::grpc::service::Files::read(
                &service,
                qdrant_auth_request(crate::grpc::service::FilesPathRequest {
                    path: "docs/a.txt".to_string(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(read.bytes, b"alpha".to_vec());

        let stat = runtime
            .block_on(crate::grpc::service::Files::stat(
                &service,
                qdrant_auth_request(crate::grpc::service::FilesPathRequest {
                    path: "docs/a.txt".to_string(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(stat.path, "docs/a.txt");
        assert_eq!(stat.kind, "file");
        assert_eq!(stat.size, 5);

        let mut list = runtime
            .block_on(crate::grpc::service::Files::list(
                &service,
                qdrant_auth_request(crate::grpc::service::FilesListRequest {
                    path: "docs".to_string(),
                    batch_size: 1,
                }),
            ))
            .unwrap()
            .into_inner();
        let batches = runtime.block_on(async move {
            let mut batches = Vec::new();
            while let Some(batch) = list.next().await {
                batches.push(batch.unwrap());
            }
            batches
        });
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].entries[0].name, "a.txt");
        assert_eq!(batches[0].entries[0].kind, "file");

        runtime
            .block_on(crate::grpc::service::Files::delete(
                &service,
                qdrant_auth_request(crate::grpc::service::FilesDeleteRequest {
                    path: "docs/a.txt".to_string(),
                    recursive: false,
                }),
            ))
            .unwrap();
        let missing = runtime
            .block_on(crate::grpc::service::Files::read(
                &service,
                qdrant_auth_request(crate::grpc::service::FilesPathRequest {
                    path: "docs/a.txt".to_string(),
                }),
            ))
            .unwrap_err();
        assert_eq!(missing.code(), tonic::Code::NotFound);
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_columnar_service_covers_native_dataset_operations() {
        let path = temp_path("grpc-columnar-service");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let service =
            crate::grpc::service::HostedColumnarGrpcService::new(kernel, "main", "events");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let schema = vec![
            ("id".to_string(), loom_core::ColumnType::Int),
            ("name".to_string(), loom_core::ColumnType::Text),
        ];
        runtime
            .block_on(crate::grpc::service::Columnar::create(
                &service,
                qdrant_auth_request(crate::grpc::service::ColumnarCreateRequest {
                    columns_cbor: loom_wire::columnar::columns_to_cbor(schema.clone()),
                    target_segment_rows: 2,
                }),
            ))
            .unwrap();

        let row_one = columnar_row_cbor(&[
            loom_core::Value::Int(1),
            loom_core::Value::Text("ada".to_string()),
        ]);
        let row_two = columnar_row_cbor(&[
            loom_core::Value::Int(2),
            loom_core::Value::Text("grace".to_string()),
        ]);
        for row_cbor in [row_one, row_two] {
            runtime
                .block_on(crate::grpc::service::Columnar::append(
                    &service,
                    qdrant_auth_request(crate::grpc::service::ColumnarAppendRequest { row_cbor }),
                ))
                .unwrap();
        }

        let rows = runtime
            .block_on(crate::grpc::service::Columnar::rows(
                &service,
                qdrant_auth_request(crate::grpc::service::ColumnarEmptyRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(rows.rows, 2);

        let columns = runtime
            .block_on(crate::grpc::service::Columnar::columns(
                &service,
                qdrant_auth_request(crate::grpc::service::ColumnarEmptyRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(
            loom_wire::columnar::columns_from_cbor(&columns.cbor).unwrap(),
            schema
        );

        let scan = runtime
            .block_on(crate::grpc::service::Columnar::scan(
                &service,
                qdrant_auth_request(crate::grpc::service::ColumnarEmptyRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(
            scan.cbor,
            loom_wire::columnar::rows_to_cbor(vec![
                vec![
                    loom_core::Value::Int(1),
                    loom_core::Value::Text("ada".to_string())
                ],
                vec![
                    loom_core::Value::Int(2),
                    loom_core::Value::Text("grace".to_string())
                ]
            ])
        );

        let select = runtime
            .block_on(crate::grpc::service::Columnar::select(
                &service,
                qdrant_auth_request(crate::grpc::service::ColumnarSelectRequest {
                    columns_cbor: columnar_select_columns_cbor(&["name"]),
                    filter_cbor: Vec::new(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(
            select.cbor,
            loom_wire::columnar::rows_to_cbor(vec![
                vec![loom_core::Value::Text("ada".to_string())],
                vec![loom_core::Value::Text("grace".to_string())]
            ])
        );

        let aggregate = runtime
            .block_on(crate::grpc::service::Columnar::aggregate(
                &service,
                qdrant_auth_request(crate::grpc::service::ColumnarAggregateRequest {
                    aggregates_cbor: columnar_aggregates_cbor(&[(0, None)]),
                    filter_cbor: Vec::new(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(
            aggregate.cbor,
            loom_wire::columnar::values_to_cbor(vec![loom_core::Value::U64(2)])
        );

        let inspect = runtime
            .block_on(crate::grpc::service::Columnar::inspect(
                &service,
                qdrant_auth_request(crate::grpc::service::ColumnarEmptyRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert!(!inspect.cbor.is_empty());

        let digest = runtime
            .block_on(crate::grpc::service::Columnar::source_digest(
                &service,
                qdrant_auth_request(crate::grpc::service::ColumnarEmptyRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert!(!digest.cbor.is_empty());

        runtime
            .block_on(crate::grpc::service::Columnar::compact(
                &service,
                qdrant_auth_request(crate::grpc::service::ColumnarEmptyRequest {}),
            ))
            .unwrap();
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_vcs_service_covers_native_history_operations() {
        let path = temp_path("grpc-vcs-service");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let files = crate::grpc::service::HostedFilesGrpcService::new(kernel.clone(), ns);
        let vcs = crate::grpc::service::HostedVcsGrpcService::new(kernel, ns);
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime
            .block_on(crate::grpc::service::Files::mkdir(
                &files,
                qdrant_auth_request(crate::grpc::service::FilesMkdirRequest {
                    path: "docs".to_string(),
                    recursive: false,
                }),
            ))
            .unwrap();
        runtime
            .block_on(crate::grpc::service::Files::write(
                &files,
                qdrant_auth_request(crate::grpc::service::FilesWriteRequest {
                    path: "docs/a.txt".to_string(),
                    bytes: b"alpha".to_vec(),
                }),
            ))
            .unwrap();
        let first = runtime
            .block_on(crate::grpc::service::Vcs::commit(
                &vcs,
                qdrant_auth_request(crate::grpc::service::VcsCommitRequest {
                    message: "one".to_string(),
                    author: "root".to_string(),
                    staged: false,
                }),
            ))
            .unwrap()
            .into_inner()
            .commit;

        runtime
            .block_on(crate::grpc::service::Files::write(
                &files,
                qdrant_auth_request(crate::grpc::service::FilesWriteRequest {
                    path: "docs/a.txt".to_string(),
                    bytes: b"bravo".to_vec(),
                }),
            ))
            .unwrap();
        let status = runtime
            .block_on(crate::grpc::service::Vcs::status(
                &vcs,
                qdrant_auth_request(crate::grpc::service::VcsStatusRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(status.unstaged[0].path, "docs/a.txt");
        assert_eq!(status.unstaged[0].kind, "modified");

        runtime
            .block_on(crate::grpc::service::Vcs::stage(
                &vcs,
                qdrant_auth_request(crate::grpc::service::VcsPathsRequest {
                    paths: vec!["docs/a.txt".to_string()],
                }),
            ))
            .unwrap();
        runtime
            .block_on(crate::grpc::service::Vcs::unstage(
                &vcs,
                qdrant_auth_request(crate::grpc::service::VcsPathsRequest {
                    paths: vec!["docs/a.txt".to_string()],
                }),
            ))
            .unwrap();
        runtime
            .block_on(crate::grpc::service::Vcs::stage_all(
                &vcs,
                qdrant_auth_request(crate::grpc::service::VcsStatusRequest {}),
            ))
            .unwrap();
        let second = runtime
            .block_on(crate::grpc::service::Vcs::commit(
                &vcs,
                qdrant_auth_request(crate::grpc::service::VcsCommitRequest {
                    message: "two".to_string(),
                    author: "root".to_string(),
                    staged: true,
                }),
            ))
            .unwrap()
            .into_inner()
            .commit;

        let mut log = runtime
            .block_on(crate::grpc::service::Vcs::log(
                &vcs,
                qdrant_auth_request(crate::grpc::service::VcsLogRequest {
                    ref_name: String::new(),
                    limit: 2,
                    batch_size: 1,
                }),
            ))
            .unwrap()
            .into_inner();
        let batches = runtime.block_on(async move {
            let mut batches = Vec::new();
            while let Some(batch) = log.next().await {
                batches.push(batch.unwrap());
            }
            batches
        });
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].commits, vec![second.clone()]);
        assert_eq!(batches[1].commits, vec![first.clone()]);

        let diff = runtime
            .block_on(crate::grpc::service::Vcs::diff(
                &vcs,
                qdrant_auth_request(crate::grpc::service::VcsDiffRequest {
                    from: first,
                    to: second,
                }),
            ))
            .unwrap()
            .into_inner();
        assert!(!diff.diff_cbor.is_empty());

        runtime
            .block_on(crate::grpc::service::Vcs::branch(
                &vcs,
                qdrant_auth_request(crate::grpc::service::VcsBranchRequest {
                    name: "feature".to_string(),
                }),
            ))
            .unwrap();
        runtime
            .block_on(crate::grpc::service::Vcs::checkout(
                &vcs,
                qdrant_auth_request(crate::grpc::service::VcsCheckoutRequest {
                    target: "feature".to_string(),
                }),
            ))
            .unwrap();
        let merge = runtime
            .block_on(crate::grpc::service::Vcs::merge(
                &vcs,
                qdrant_auth_request(crate::grpc::service::VcsMergeRequest {
                    source: "main".to_string(),
                    author: "root".to_string(),
                    cells: false,
                }),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(merge.outcome, "up_to_date");
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_kv_service_covers_put_get_list_range_and_delete() {
        let path = temp_path("grpc-kv-service");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let service = crate::grpc::service::HostedKvGrpcService::new(kernel, "main", "cache");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let key_one = loom_core::kv::key_to_cbor(&loom_core::Value::Int(1));
        let key_two = loom_core::kv::key_to_cbor(&loom_core::Value::Int(2));
        let key_three = loom_core::kv::key_to_cbor(&loom_core::Value::Int(3));
        let key_four = loom_core::kv::key_to_cbor(&loom_core::Value::Int(4));

        for (key_cbor, value) in [
            (key_one.clone(), b"one".to_vec()),
            (key_two.clone(), b"two".to_vec()),
            (key_three.clone(), b"three".to_vec()),
        ] {
            runtime
                .block_on(crate::grpc::service::Kv::put(
                    &service,
                    qdrant_auth_request(crate::grpc::service::KvPutRequest { key_cbor, value }),
                ))
                .unwrap();
        }

        let get = runtime
            .block_on(crate::grpc::service::Kv::get(
                &service,
                qdrant_auth_request(crate::grpc::service::KvKeyRequest {
                    key_cbor: key_two.clone(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert!(get.found);
        assert_eq!(get.value, b"two".to_vec());

        let list = runtime
            .block_on(crate::grpc::service::Kv::list(
                &service,
                qdrant_auth_request(crate::grpc::service::KvListRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(list.entries.len(), 3);

        let mut range = runtime
            .block_on(crate::grpc::service::Kv::range(
                &service,
                qdrant_auth_request(crate::grpc::service::KvRangeRequest {
                    lo_cbor: key_one,
                    hi_cbor: key_four,
                    batch_size: 2,
                }),
            ))
            .unwrap()
            .into_inner();
        let batches = runtime.block_on(async move {
            let mut batches = Vec::new();
            while let Some(batch) = range.next().await {
                batches.push(batch.unwrap());
            }
            batches
        });
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].entries.len(), 2);
        assert_eq!(batches[1].entries.len(), 1);

        let delete = runtime
            .block_on(crate::grpc::service::Kv::delete(
                &service,
                qdrant_auth_request(crate::grpc::service::KvKeyRequest { key_cbor: key_two }),
            ))
            .unwrap()
            .into_inner();
        assert!(delete.deleted);
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_document_service_covers_native_operations() {
        let path = temp_path("grpc-document-service");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let service = crate::grpc::service::HostedDocumentGrpcService::new(kernel, "main", "docs");
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let first = runtime
            .block_on(crate::grpc::service::Document::put_text(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentPutTextRequest {
                    id: "doc-1".to_string(),
                    text: r#"{"a":1,"name":"one"}"#.to_string(),
                    expected_entity_tag: None,
                }),
            ))
            .unwrap()
            .into_inner();
        let first_digest = first.digest.clone();
        let second = runtime
            .block_on(crate::grpc::service::Document::put_text(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentPutTextRequest {
                    id: "doc-2".to_string(),
                    text: r#"{"a":2,"name":"two"}"#.to_string(),
                    expected_entity_tag: None,
                }),
            ))
            .unwrap()
            .into_inner();
        let get = runtime
            .block_on(crate::grpc::service::Document::get_text(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentIdRequest {
                    id: "doc-1".to_string(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert!(get.found);
        assert_eq!(get.text, r#"{"a":1,"name":"one"}"#);
        assert_eq!(get.digest, first_digest);

        let absent = runtime
            .block_on(crate::grpc::service::Document::get_text(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentIdRequest {
                    id: "missing".to_string(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert!(!absent.found);

        let stale = runtime
            .block_on(crate::grpc::service::Document::put_text(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentPutTextRequest {
                    id: "doc-2".to_string(),
                    text: r#"{"a":3,"name":"stale"}"#.to_string(),
                    expected_entity_tag: Some(first.entity_tag.clone()),
                }),
            ))
            .unwrap_err();
        assert_eq!(stale.code(), tonic::Code::FailedPrecondition);

        let updated = runtime
            .block_on(crate::grpc::service::Document::put_text(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentPutTextRequest {
                    id: "doc-2".to_string(),
                    text: r#"{"a":2,"name":"two updated"}"#.to_string(),
                    expected_entity_tag: Some(second.entity_tag),
                }),
            ))
            .unwrap()
            .into_inner();
        assert!(Digest::parse(&updated.digest).is_ok());

        runtime
            .block_on(crate::grpc::service::Document::put_binary(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentPutBinaryRequest {
                    id: "raw".to_string(),
                    bytes: vec![0xff, 0x00],
                    expected_entity_tag: None,
                }),
            ))
            .unwrap();
        let raw = runtime
            .block_on(crate::grpc::service::Document::get_binary(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentIdRequest {
                    id: "raw".to_string(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert!(raw.found);
        assert_eq!(raw.bytes, vec![0xff, 0x00]);
        assert!(Digest::parse(&raw.digest).is_ok());
        let non_text = runtime
            .block_on(crate::grpc::service::Document::get_text(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentIdRequest {
                    id: "raw".to_string(),
                }),
            ))
            .unwrap_err();
        assert_eq!(non_text.code(), tonic::Code::FailedPrecondition);

        let mut list = runtime
            .block_on(crate::grpc::service::Document::list_binary(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentListRequest { batch_size: 1 }),
            ))
            .unwrap()
            .into_inner();
        let list_batches = runtime.block_on(async move {
            let mut batches = Vec::new();
            while let Some(batch) = list.next().await {
                batches.push(batch.unwrap());
            }
            batches
        });
        assert_eq!(list_batches.len(), 3);
        assert_eq!(list_batches[0].entries[0].id, "doc-1");

        let raw_delete = runtime
            .block_on(crate::grpc::service::Document::delete(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentIdRequest {
                    id: "raw".to_string(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert!(raw_delete.deleted);

        runtime
            .block_on(crate::grpc::service::Document::index_create(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentIndexCreateRequest {
                    name: "by_a".to_string(),
                    path: "a".to_string(),
                    unique: false,
                    declaration_json: Vec::new(),
                }),
            ))
            .unwrap();

        let indexes = runtime
            .block_on(crate::grpc::service::Document::index_list(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentIndexListRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(indexes.indexes.len(), 1);
        assert_eq!(indexes.indexes[0].name, "by_a");

        let statuses = runtime
            .block_on(crate::grpc::service::Document::index_status(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentIndexStatusRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(statuses.statuses.len(), 1);
        assert!(statuses.statuses[0].ready);
        assert_eq!(statuses.statuses[0].entries, 2);

        runtime
            .block_on(crate::grpc::service::Document::index_rebuild(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentIndexNameRequest {
                    name: "by_a".to_string(),
                }),
            ))
            .unwrap();

        let mut find = runtime
            .block_on(crate::grpc::service::Document::find(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentFindRequest {
                    index: "by_a".to_string(),
                    value_json: "1".to_string(),
                    batch_size: 1,
                }),
            ))
            .unwrap()
            .into_inner();
        let find_batches = runtime.block_on(async move {
            let mut batches = Vec::new();
            while let Some(batch) = find.next().await {
                batches.push(batch.unwrap());
            }
            batches
        });
        assert_eq!(find_batches.len(), 1);
        assert_eq!(find_batches[0].ids, vec!["doc-1".to_string()]);

        let query = runtime
            .block_on(crate::grpc::service::Document::query(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentQueryRequest {
                    query_json:
                        r#"{"predicate":{"path":"a","op":"eq","value":1},"include_document":true}"#
                            .to_string(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert!(query.result_json.contains("\"id\":\"doc-1\""));
        assert!(query.result_json.contains("\"document_hex\""));

        let drop_index = runtime
            .block_on(crate::grpc::service::Document::index_drop(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentIndexNameRequest {
                    name: "by_a".to_string(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert!(drop_index.dropped);

        let delete = runtime
            .block_on(crate::grpc::service::Document::delete(
                &service,
                qdrant_auth_request(crate::grpc::service::DocumentIdRequest {
                    id: "doc-2".to_string(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert!(delete.deleted);
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_fts_service_covers_native_operations() {
        let path = temp_path("grpc-fts-service");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let service = crate::grpc::service::HostedFtsGrpcService::new(kernel, "main", "docs");
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime
            .block_on(crate::grpc::service::Fts::create(
                &service,
                qdrant_auth_request(crate::grpc::service::FtsCreateRequest {
                    mapping: vec![
                        crate::grpc::service::FtsMappingEntry {
                            field: "title".to_string(),
                            field_type: "text".to_string(),
                            stored: Some(true),
                            faceted: Some(false),
                        },
                        crate::grpc::service::FtsMappingEntry {
                            field: "rank".to_string(),
                            field_type: "keyword".to_string(),
                            stored: Some(true),
                            faceted: Some(true),
                        },
                    ],
                }),
            ))
            .unwrap();

        for (id, title, rank) in [
            (b"doc-1".to_vec(), "hello world", b"01".to_vec()),
            (b"doc-2".to_vec(), "hello there", b"02".to_vec()),
        ] {
            runtime
                .block_on(crate::grpc::service::Fts::index(
                    &service,
                    qdrant_auth_request(crate::grpc::service::FtsIndexRequest {
                        id,
                        document: vec![
                            crate::grpc::service::FtsFieldValueMessage {
                                field: "title".to_string(),
                                value: Some(crate::grpc::service::FtsFieldValueKind::Text(
                                    title.to_string(),
                                )),
                            },
                            crate::grpc::service::FtsFieldValueMessage {
                                field: "rank".to_string(),
                                value: Some(crate::grpc::service::FtsFieldValueKind::Bytes(rank)),
                            },
                        ],
                    }),
                ))
                .unwrap();
        }

        let get = runtime
            .block_on(crate::grpc::service::Fts::get(
                &service,
                qdrant_auth_request(crate::grpc::service::FtsIdRequest {
                    id: b"doc-1".to_vec(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert!(get.found);
        assert_eq!(get.document.len(), 2);

        let mut ids = runtime
            .block_on(crate::grpc::service::Fts::ids(
                &service,
                qdrant_auth_request(crate::grpc::service::FtsIdsRequest {
                    prefix: Some(b"doc".to_vec()),
                    batch_size: 1,
                }),
            ))
            .unwrap()
            .into_inner();
        let id_batches = runtime.block_on(async move {
            let mut batches = Vec::new();
            while let Some(batch) = ids.next().await {
                batches.push(batch.unwrap());
            }
            batches
        });
        assert_eq!(id_batches.len(), 2);
        assert_eq!(id_batches[0].ids[0], b"doc-1".to_vec());

        let query = runtime
            .block_on(crate::grpc::service::Fts::query(
                &service,
                qdrant_auth_request(crate::grpc::service::FtsQueryRequest {
                    query: Some(crate::grpc::service::FtsQueryMessage {
                        kind: "bool".to_string(),
                        must: vec![
                            crate::grpc::service::FtsQueryMessage {
                                kind: "match".to_string(),
                                field: "title".to_string(),
                                text: "hello".to_string(),
                                ..Default::default()
                            },
                            crate::grpc::service::FtsQueryMessage {
                                kind: "range".to_string(),
                                field: "rank".to_string(),
                                lower: Some(b"01".to_vec()),
                                upper: Some(b"02".to_vec()),
                                include_lower: true,
                                include_upper: true,
                                ..Default::default()
                            },
                        ],
                        ..Default::default()
                    }),
                    limit: 10,
                    offset: 0,
                }),
            ))
            .unwrap()
            .into_inner();
        assert!(query.reduced);
        assert_eq!(query.hits.len(), 2);

        runtime
            .block_on(crate::grpc::service::Fts::remap(
                &service,
                qdrant_auth_request(crate::grpc::service::FtsRemapRequest {
                    mapping: vec![
                        crate::grpc::service::FtsMappingEntry {
                            field: "title".to_string(),
                            field_type: "text".to_string(),
                            stored: Some(true),
                            faceted: Some(false),
                        },
                        crate::grpc::service::FtsMappingEntry {
                            field: "rank".to_string(),
                            field_type: "keyword".to_string(),
                            stored: Some(true),
                            faceted: Some(true),
                        },
                        crate::grpc::service::FtsMappingEntry {
                            field: "lang".to_string(),
                            field_type: "keyword".to_string(),
                            stored: Some(true),
                            faceted: Some(false),
                        },
                    ],
                }),
            ))
            .unwrap();

        let delete = runtime
            .block_on(crate::grpc::service::Fts::delete(
                &service,
                qdrant_auth_request(crate::grpc::service::FtsIdRequest {
                    id: b"doc-2".to_vec(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert!(delete.deleted);
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_graph_service_covers_native_operations() {
        let path = temp_path("grpc-graph-service");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let service = crate::grpc::service::HostedGraphGrpcService::new(kernel, "main", "work");
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let mut ada = loom_core::Props::new();
        ada.insert(
            "name".to_string(),
            loom_core::GraphValue::Text("Ada".to_string()),
        );
        runtime
            .block_on(crate::grpc::service::Graph::upsert_node(
                &service,
                qdrant_auth_request(crate::grpc::service::GraphUpsertNodeRequest {
                    id: "ada".to_string(),
                    props_cbor: loom_wire::graph::props_to_cbor(&ada),
                }),
            ))
            .unwrap();

        let mut team = loom_core::Props::new();
        team.insert(
            "kind".to_string(),
            loom_core::GraphValue::Text("team".to_string()),
        );
        runtime
            .block_on(crate::grpc::service::Graph::upsert_node(
                &service,
                qdrant_auth_request(crate::grpc::service::GraphUpsertNodeRequest {
                    id: "team".to_string(),
                    props_cbor: loom_wire::graph::props_to_cbor(&team),
                }),
            ))
            .unwrap();

        runtime
            .block_on(crate::grpc::service::Graph::upsert_edge(
                &service,
                qdrant_auth_request(crate::grpc::service::GraphUpsertEdgeRequest {
                    id: "ada-team".to_string(),
                    src: "ada".to_string(),
                    dst: "team".to_string(),
                    label: "MEMBER_OF".to_string(),
                    props_cbor: Vec::new(),
                }),
            ))
            .unwrap();

        let node = runtime
            .block_on(crate::grpc::service::Graph::get_node(
                &service,
                qdrant_auth_request(crate::grpc::service::GraphNodeRequest {
                    id: "ada".to_string(),
                }),
            ))
            .unwrap()
            .into_inner();
        assert!(node.found);
        let node_props = loom_wire::graph::props_from_cbor(&node.props_cbor).unwrap();
        assert_eq!(
            node_props.get("name"),
            Some(&loom_core::GraphValue::Text("Ada".to_string()))
        );

        let mut neighbors = runtime
            .block_on(crate::grpc::service::Graph::neighbors(
                &service,
                qdrant_auth_request(crate::grpc::service::GraphNeighborsRequest {
                    id: "ada".to_string(),
                    batch_size: 1,
                }),
            ))
            .unwrap()
            .into_inner();
        let neighbor_batches = runtime.block_on(async move {
            let mut batches = Vec::new();
            while let Some(batch) = neighbors.next().await {
                batches.push(batch.unwrap());
            }
            batches
        });
        assert_eq!(neighbor_batches.len(), 1);
        assert_eq!(neighbor_batches[0].ids, vec!["team".to_string()]);

        let mut reachable = runtime
            .block_on(crate::grpc::service::Graph::reachable(
                &service,
                qdrant_auth_request(crate::grpc::service::GraphReachableRequest {
                    start: "ada".to_string(),
                    max_depth: Some(1),
                    via_label: Some("MEMBER_OF".to_string()),
                    batch_size: 1,
                }),
            ))
            .unwrap()
            .into_inner();
        let reachable_batches = runtime.block_on(async move {
            let mut batches = Vec::new();
            while let Some(batch) = reachable.next().await {
                batches.push(batch.unwrap());
            }
            batches
        });
        assert_eq!(reachable_batches.len(), 1);
        assert_eq!(reachable_batches[0].ids, vec!["team".to_string()]);

        let query = runtime
            .block_on(crate::grpc::service::Graph::query(
                &service,
                qdrant_auth_request(crate::grpc::service::GraphQueryRequest {
                    opencypher: "MATCH (p) RETURN p ORDER BY id(p)".to_string(),
                }),
            ))
            .unwrap()
            .into_inner();
        let query_value = loom_codec::decode(&query.result_cbor).unwrap();
        let loom_codec::Value::Array(rows) = query_value else {
            panic!("graph query result should be a CBOR array");
        };
        assert_eq!(rows.len(), 2);

        let explain = runtime
            .block_on(crate::grpc::service::Graph::explain_query(
                &service,
                qdrant_auth_request(crate::grpc::service::GraphQueryRequest {
                    opencypher: "MATCH (p) RETURN p".to_string(),
                }),
            ))
            .unwrap()
            .into_inner();
        let explain_value = loom_codec::decode(&explain.explain_cbor).unwrap();
        assert!(matches!(explain_value, loom_codec::Value::Map(_)));

        let capabilities = runtime
            .block_on(crate::grpc::service::Graph::capabilities(
                &service,
                qdrant_auth_request(crate::grpc::service::GraphCapabilitiesRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(capabilities.collection, "work");
        assert_eq!(
            capabilities.query_language,
            "bounded_opencypher_gql_aligned"
        );
        assert_eq!(capabilities.neo4j, "unsupported");
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_queue_service_covers_append_get_range_and_len() {
        let path = temp_path("grpc-queue-service");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let service = crate::grpc::service::HostedQueueGrpcService::new(kernel, "main", "events");
        let runtime = tokio::runtime::Runtime::new().unwrap();

        for payload in [b"one".to_vec(), b"two".to_vec(), b"three".to_vec()] {
            runtime
                .block_on(crate::grpc::service::Queue::append(
                    &service,
                    qdrant_auth_request(crate::grpc::service::QueueAppendRequest { payload }),
                ))
                .unwrap();
        }

        let len = runtime
            .block_on(crate::grpc::service::Queue::len(
                &service,
                qdrant_auth_request(crate::grpc::service::QueueLenRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(len.len, 3);

        let get = runtime
            .block_on(crate::grpc::service::Queue::get(
                &service,
                qdrant_auth_request(crate::grpc::service::QueueGetRequest { seq: 1 }),
            ))
            .unwrap()
            .into_inner();
        assert!(get.found);
        assert_eq!(get.payload, b"two".to_vec());

        let range = runtime
            .block_on(crate::grpc::service::Queue::range(
                &service,
                qdrant_auth_request(crate::grpc::service::QueueRangeRequest { lo: 1, hi: 3 }),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(range.entries.len(), 2);
        assert_eq!(range.entries[0].seq, 1);
        assert_eq!(range.entries[0].payload, b"two".to_vec());
        assert_eq!(range.entries[1].seq, 2);
        assert_eq!(range.entries[1].payload, b"three".to_vec());
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_ledger_service_covers_native_append_range_and_proofs() {
        let path = temp_path("grpc-ledger-service");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let service = crate::grpc::service::HostedLedgerGrpcService::new(kernel, "main", "audit");
        let runtime = tokio::runtime::Runtime::new().unwrap();

        for payload in [b"zero".to_vec(), b"one".to_vec(), b"two".to_vec()] {
            runtime
                .block_on(crate::grpc::service::Ledger::append(
                    &service,
                    qdrant_auth_request(crate::grpc::service::LedgerAppendRequest { payload }),
                ))
                .unwrap();
        }

        let len = runtime
            .block_on(crate::grpc::service::Ledger::len(
                &service,
                qdrant_auth_request(crate::grpc::service::LedgerLenRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(len.len, 3);

        let get = runtime
            .block_on(crate::grpc::service::Ledger::get(
                &service,
                qdrant_auth_request(crate::grpc::service::LedgerGetRequest { seq: 1 }),
            ))
            .unwrap()
            .into_inner();
        assert!(get.found);
        assert_eq!(get.payload, b"one".to_vec());

        let head = runtime
            .block_on(crate::grpc::service::Ledger::head(
                &service,
                qdrant_auth_request(crate::grpc::service::LedgerHeadRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert!(head.found);
        assert!(Digest::parse(&head.digest).is_ok());

        let mut range = runtime
            .block_on(crate::grpc::service::Ledger::range(
                &service,
                qdrant_auth_request(crate::grpc::service::LedgerRangeRequest {
                    start: 0,
                    end: 3,
                    batch_size: 2,
                }),
            ))
            .unwrap()
            .into_inner();
        let batches = runtime.block_on(async move {
            let mut batches = Vec::new();
            while let Some(batch) = range.next().await {
                batches.push(batch.unwrap());
            }
            batches
        });
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].start, 0);
        assert_eq!(batches[0].end, 3);
        assert_eq!(batches[0].state, "retained");
        assert_eq!(batches[0].entries.len(), 2);
        assert_eq!(batches[0].entries[1].seq, 1);
        assert_eq!(batches[0].entries[1].payload, b"one".to_vec());
        assert!(Digest::parse(&batches[0].entries[1].entry_hash).is_ok());
        assert_eq!(batches[1].entries.len(), 1);
        assert_eq!(batches[1].entries[0].seq, 2);

        let collections = runtime
            .block_on(crate::grpc::service::Ledger::list_collections(
                &service,
                qdrant_auth_request(crate::grpc::service::LedgerCollectionsRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(collections.collections, vec!["audit".to_string()]);

        let checkpoint = runtime
            .block_on(crate::grpc::service::Ledger::checkpoint_payload(
                &service,
                qdrant_auth_request(crate::grpc::service::LedgerCheckpointPayloadRequest {}),
            ))
            .unwrap()
            .into_inner();
        let checkpoint_payload =
            loom_core::LedgerCheckpointPayload::decode(&checkpoint.payload_cbor).unwrap();
        assert_eq!(checkpoint_payload.collection, "audit");
        assert_eq!(checkpoint_payload.latest_seq, Some(2));

        let signatures = runtime
            .block_on(crate::grpc::service::Ledger::verify_checkpoint_signatures(
                &service,
                qdrant_auth_request(crate::grpc::service::LedgerVerifyCheckpointRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(signatures.signatures, 0);

        let proof_tree = runtime
            .block_on(crate::grpc::service::Ledger::proof_tree(
                &service,
                qdrant_auth_request(crate::grpc::service::LedgerProofTreeRequest {}),
            ))
            .unwrap()
            .into_inner();
        let tree = loom_core::LedgerProofTree::decode(&proof_tree.proof_cbor).unwrap();
        assert_eq!(tree.collection, "audit");
        assert_eq!(tree.tree_size, 3);

        let inclusion = runtime
            .block_on(crate::grpc::service::Ledger::inclusion_proof(
                &service,
                qdrant_auth_request(crate::grpc::service::LedgerInclusionProofRequest { seq: 1 }),
            ))
            .unwrap()
            .into_inner();
        let inclusion_proof =
            loom_core::LedgerInclusionProof::decode(&inclusion.proof_cbor).unwrap();
        assert_eq!(inclusion_proof.seq, 1);
        loom_core::ledger_verify_inclusion_proof(&inclusion_proof).unwrap();

        let consistency = runtime
            .block_on(crate::grpc::service::Ledger::consistency_proof(
                &service,
                qdrant_auth_request(crate::grpc::service::LedgerConsistencyProofRequest {
                    first_tree_size: 2,
                    second_tree_size: 3,
                }),
            ))
            .unwrap()
            .into_inner();
        let consistency_proof =
            loom_core::LedgerConsistencyProof::decode(&consistency.proof_cbor).unwrap();
        assert_eq!(consistency_proof.first_tree_size, 2);
        assert_eq!(consistency_proof.second_tree_size, 3);
        loom_core::ledger_verify_consistency_proof(&consistency_proof).unwrap();

        runtime
            .block_on(crate::grpc::service::Ledger::verify(
                &service,
                qdrant_auth_request(crate::grpc::service::LedgerVerifyRequest {}),
            ))
            .unwrap();
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_time_series_service_covers_native_operations() {
        let path = temp_path("grpc-time-series-service");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let service =
            crate::grpc::service::HostedTimeSeriesGrpcService::new(kernel, "main", "metrics");
        let runtime = tokio::runtime::Runtime::new().unwrap();

        for (timestamp, value) in [(100, b"p100".to_vec()), (200, b"p200".to_vec())] {
            runtime
                .block_on(crate::grpc::service::TimeSeries::put(
                    &service,
                    qdrant_auth_request(crate::grpc::service::TimeSeriesPutRequest {
                        timestamp,
                        value,
                    }),
                ))
                .unwrap();
        }

        let get = runtime
            .block_on(crate::grpc::service::TimeSeries::get(
                &service,
                qdrant_auth_request(crate::grpc::service::TimeSeriesGetRequest { timestamp: 100 }),
            ))
            .unwrap()
            .into_inner();
        assert!(get.found);
        assert_eq!(get.point.unwrap().value, b"p100".to_vec());

        let latest = runtime
            .block_on(crate::grpc::service::TimeSeries::latest(
                &service,
                qdrant_auth_request(crate::grpc::service::TimeSeriesLatestRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(latest.point.unwrap().timestamp, 200);

        let mut range = runtime
            .block_on(crate::grpc::service::TimeSeries::range(
                &service,
                qdrant_auth_request(crate::grpc::service::TimeSeriesRangeRequest {
                    from: 0,
                    to: 300,
                    batch_size: 1,
                }),
            ))
            .unwrap()
            .into_inner();
        let legacy_batches = runtime.block_on(async move {
            let mut batches = Vec::new();
            while let Some(batch) = range.next().await {
                batches.push(batch.unwrap());
            }
            batches
        });
        assert_eq!(legacy_batches.len(), 2);

        runtime
            .block_on(crate::grpc::service::TimeSeries::put_structured(
                &service,
                qdrant_auth_request(crate::grpc::service::TimeSeriesPutStructuredRequest {
                    point: Some(crate::grpc::service::StructuredTimeSeriesPointMessage {
                        measurement: "cpu".to_string(),
                        tags: vec![crate::grpc::service::TimeSeriesTagMessage {
                            name: "host".to_string(),
                            value: "api-1".to_string(),
                        }],
                        timestamp_ns: 300,
                        fields: vec![crate::grpc::service::TimeSeriesFieldMessage {
                            name: "value".to_string(),
                            value: Some(crate::grpc::service::TimeSeriesFieldKind::Float(2.5)),
                        }],
                    }),
                }),
            ))
            .unwrap();

        let mut structured_range = runtime
            .block_on(crate::grpc::service::TimeSeries::range_structured(
                &service,
                qdrant_auth_request(crate::grpc::service::TimeSeriesStructuredRangeRequest {
                    from_ns: 250,
                    to_ns: 400,
                    batch_size: 2,
                }),
            ))
            .unwrap()
            .into_inner();
        let structured_batches = runtime.block_on(async move {
            let mut batches = Vec::new();
            while let Some(batch) = structured_range.next().await {
                batches.push(batch.unwrap());
            }
            batches
        });
        assert_eq!(structured_batches.len(), 1);
        assert_eq!(structured_batches[0].points[0].measurement, "cpu");

        runtime
            .block_on(crate::grpc::service::TimeSeries::set_policy(
                &service,
                qdrant_auth_request(crate::grpc::service::TimeSeriesSetPolicyRequest {
                    query_start_ns: Some(0),
                    rollups: vec![crate::grpc::service::TimeSeriesRollupMessage {
                        name: "hundred_mean".to_string(),
                        resolution_ns: 100,
                        aggregation: "mean".to_string(),
                    }],
                }),
            ))
            .unwrap();

        let policy = runtime
            .block_on(crate::grpc::service::TimeSeries::policy(
                &service,
                qdrant_auth_request(crate::grpc::service::TimeSeriesPolicyRequest {}),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(policy.rollups.len(), 1);
        assert_eq!(policy.rollups[0].aggregation, "mean");

        runtime
            .block_on(crate::grpc::service::TimeSeries::materialize_rollup(
                &service,
                qdrant_auth_request(crate::grpc::service::TimeSeriesMaterializeRollupRequest {
                    rollup: "hundred_mean".to_string(),
                }),
            ))
            .unwrap();

        let mut rollup_range = runtime
            .block_on(crate::grpc::service::TimeSeries::range_rollup_structured(
                &service,
                qdrant_auth_request(
                    crate::grpc::service::TimeSeriesRangeRollupStructuredRequest {
                        rollup: "hundred_mean".to_string(),
                        from_ns: 250,
                        to_ns: 400,
                        batch_size: 2,
                    },
                ),
            ))
            .unwrap()
            .into_inner();
        let rollup_batches = runtime.block_on(async move {
            let mut batches = Vec::new();
            while let Some(batch) = rollup_range.next().await {
                batches.push(batch.unwrap());
            }
            batches
        });
        assert_eq!(rollup_batches.len(), 1);
        assert_eq!(rollup_batches[0].points[0].measurement, "cpu");

        let prune = runtime
            .block_on(crate::grpc::service::TimeSeries::prune_before(
                &service,
                qdrant_auth_request(crate::grpc::service::TimeSeriesPruneBeforeRequest {
                    cutoff_ns: 250,
                }),
            ))
            .unwrap()
            .into_inner();
        assert_eq!(prune.pruned, 2);
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_cas_service_uses_external_metadata_auth() {
        let path = temp_path("grpc-cas-external-service");
        let ns = init(&path, None);
        loom_coordination::with_local_store_write_lock(&path, || {
            let fs = FileStore::open(&path).unwrap();
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
        let service = crate::grpc::service::HostedCasGrpcService::new(kernel, ns);
        let mut request = tonic::Request::new(crate::grpc::service::CasPutRequest {
            bytes: b"service".to_vec(),
        });
        request
            .metadata_mut()
            .insert("x-loom-external-kind", "oidc-subject".parse().unwrap());
        request.metadata_mut().insert(
            "x-loom-external-issuer",
            "https://issuer.example".parse().unwrap(),
        );
        request
            .metadata_mut()
            .insert("x-loom-external-subject", "00u123".parse().unwrap());
        request.metadata_mut().insert(
            "x-loom-external-material-digest",
            "sha256:metadata".parse().unwrap(),
        );
        request
            .metadata_mut()
            .insert("x-loom-session", "grpc-external-test".parse().unwrap());
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let response = runtime
            .block_on(crate::grpc::service::Cas::put(&service, request))
            .unwrap();
        assert!(Digest::parse(&response.into_inner().digest).is_ok());
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_cas_service_rejects_direct_external_proof_without_verifier() {
        let path = temp_path("grpc-cas-direct-external-proof");
        let ns = init(&path, None);
        let kernel = HostedKernel::new(&path);
        let service = crate::grpc::service::HostedCasGrpcService::new(kernel, ns);
        let mut request = tonic::Request::new(crate::grpc::service::CasPutRequest {
            bytes: b"service".to_vec(),
        });
        request.metadata_mut().insert(
            "x-loom-external-proof-kind",
            "mtls-certificate".parse().unwrap(),
        );
        request
            .metadata_mut()
            .insert("x-loom-external-proof", "{}".parse().unwrap());
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let err = runtime
            .block_on(crate::grpc::service::Cas::put(&service, request))
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unimplemented);
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_watch_service_uses_metadata_auth_and_byte_keys() {
        let path = temp_path("grpc-watch-service");
        let (ns, c0, c1) = watch_history(&path);
        let kernel = HostedKernel::new(&path);
        let service = crate::grpc::service::HostedWatchGrpcService::new(kernel, ns);
        let mut subscribe = tonic::Request::new(crate::grpc::service::WatchSubscribeRequest {
            branch: "main".to_string(),
            from: Some(c0.to_string()),
            facet: Some("files".to_string()),
            path_prefix: Some("b.".to_string()),
            change_kinds: vec!["added".to_string()],
        });
        add_root_metadata(&mut subscribe);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let cursor = runtime
            .block_on(crate::grpc::service::Watch::subscribe(&service, subscribe))
            .unwrap()
            .into_inner()
            .cursor;

        let mut poll =
            tonic::Request::new(crate::grpc::service::WatchPollRequest { cursor, max: 10 });
        add_root_metadata(&mut poll);
        let batch = runtime
            .block_on(crate::grpc::service::Watch::poll(&service, poll))
            .unwrap()
            .into_inner();

        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].commit, c1.to_string());
        assert_eq!(batch.events[0].changes.len(), 1);
        assert_eq!(batch.events[0].changes[0].domain, "files");
        assert_eq!(batch.events[0].changes[0].kind, "added");
        assert_eq!(batch.events[0].changes[0].key, b"b.txt".to_vec());
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_watch_service_stream_projects_debounced_pulses() {
        let path = temp_path("grpc-watch-service-stream");
        let (ns, c0, c1) = watch_history(&path);
        let kernel = HostedKernel::new(&path);
        let service = crate::grpc::service::HostedWatchGrpcService::new(kernel, ns);
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let mut subscribe = tonic::Request::new(crate::grpc::service::WatchSubscribeRequest {
            branch: "main".to_string(),
            from: Some(c0.to_string()),
            facet: Some("files".to_string()),
            path_prefix: Some("b.".to_string()),
            change_kinds: vec!["added".to_string()],
        });
        add_root_metadata(&mut subscribe);
        let cursor = runtime
            .block_on(crate::grpc::service::Watch::subscribe(&service, subscribe))
            .unwrap()
            .into_inner()
            .cursor;

        let mut stream_request = tonic::Request::new(crate::grpc::service::WatchStreamRequest {
            cursor,
            max: 10,
            interval_ms: Some(10),
            debounce_ms: Some(0),
            limit: Some(1),
        });
        add_root_metadata(&mut stream_request);
        let frame = runtime
            .block_on(async {
                let mut stream = crate::grpc::service::Watch::stream(&service, stream_request)
                    .await
                    .unwrap()
                    .into_inner();
                match stream.next().await {
                    Some(Ok(frame)) => Ok(frame),
                    Some(Err(err)) => Err(err),
                    None => Err(Status::failed_precondition("watch stream ended")),
                }
            })
            .unwrap();
        assert_eq!(frame.events[0].commit, c1.to_string());
        assert_eq!(frame.events[0].changes[0].domain, "files");
        assert_eq!(frame.events[0].changes[0].kind, "added");
        assert_eq!(frame.source_cursor, frame.next);

        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_sql_service_uses_metadata_auth() {
        let path = temp_path("grpc-sql-service");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let service = crate::grpc::service::HostedSqlGrpcService::new(kernel, "main", "service");
        let mut exec = tonic::Request::new(crate::grpc::service::SqlRequest {
            sql: "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); INSERT INTO t VALUES (1, 'a')"
                .to_string(),
        });
        exec.metadata_mut()
            .insert("x-loom-principal", nid(1).to_string().parse().unwrap());
        exec.metadata_mut()
            .insert("x-loom-passphrase", "root-pass".parse().unwrap());
        exec.metadata_mut()
            .insert("x-loom-session", "grpc-sql-test".parse().unwrap());
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let response = runtime
            .block_on(crate::grpc::service::Sql::exec(&service, exec))
            .unwrap();
        assert!(!response.into_inner().cbor.is_empty());

        let mut query = tonic::Request::new(crate::grpc::service::SqlRequest {
            sql: "SELECT id, v FROM t".to_string(),
        });
        query
            .metadata_mut()
            .insert("x-loom-principal", nid(1).to_string().parse().unwrap());
        query
            .metadata_mut()
            .insert("x-loom-passphrase", "root-pass".parse().unwrap());
        query
            .metadata_mut()
            .insert("x-loom-session", "grpc-sql-test".parse().unwrap());
        let response = runtime
            .block_on(crate::grpc::service::Sql::query(&service, query))
            .unwrap();
        assert!(!response.into_inner().cbor.is_empty());
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_qdrant_profile_covers_vector_compatibility_flow() {
        let path = temp_path("grpc-qdrant-service");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let collections = crate::grpc::service::HostedQdrantCollectionsGrpcService::new(
            kernel.clone(),
            "main",
            "docs",
        );
        let points =
            crate::grpc::service::HostedQdrantPointsGrpcService::new(kernel, "main", "docs");
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let create = qdrant_auth_request(crate::grpc::service::QdrantCreateCollectionRequest {
            collection_name: "docs".to_string(),
            vectors: Some(crate::grpc::service::QdrantVectorParams {
                size: 2,
                distance: "Dot".to_string(),
            }),
            named_vectors: Vec::new(),
        });
        runtime
            .block_on(crate::grpc::service::QdrantCollections::create(
                &collections,
                create,
            ))
            .unwrap();

        let upsert = qdrant_auth_request(crate::grpc::service::QdrantUpsertPointsRequest {
            collection_name: "docs".to_string(),
            points: vec![
                qdrant_point("a", [1.0, 0.0], "en", 9),
                qdrant_point("b", [0.0, 1.0], "fr", 7),
            ],
        });
        runtime
            .block_on(crate::grpc::service::QdrantPoints::upsert(&points, upsert))
            .unwrap();

        let info = qdrant_auth_request(crate::grpc::service::QdrantCollectionRequest {
            collection_name: "docs".to_string(),
        });
        let info = runtime
            .block_on(crate::grpc::service::QdrantCollections::get(
                &collections,
                info,
            ))
            .unwrap()
            .into_inner();
        assert_eq!(info.points_count, 2);
        assert_eq!(info.distance, "Dot");

        let filter = Some(crate::grpc::service::QdrantFilter {
            must: vec![crate::grpc::service::QdrantCondition {
                key: "lang".to_string(),
                kind: Some(crate::grpc::service::qdrant_condition::Kind::Match(
                    crate::grpc::service::QdrantMatch {
                        value: Some(qdrant_string("en")),
                        any: Vec::new(),
                        except: Vec::new(),
                    },
                )),
            }],
            should: Vec::new(),
            must_not: Vec::new(),
        });
        let search = qdrant_auth_request(crate::grpc::service::QdrantSearchPointsRequest {
            collection_name: "docs".to_string(),
            vector: vec![1.0, 0.0],
            vector_name: String::new(),
            filter: filter.clone(),
            limit: 10,
            with_payload: true,
            with_vectors: true,
        });
        let search = runtime
            .block_on(crate::grpc::service::QdrantPoints::search(&points, search))
            .unwrap()
            .into_inner();
        assert_eq!(search.result.len(), 1);
        assert_eq!(
            search.result[0]
                .id
                .as_ref()
                .and_then(|id| id.point_id_options.as_ref()),
            Some(&crate::grpc::service::qdrant_point_id::PointIdOptions::Uuid("a".to_string()))
        );
        assert_eq!(search.result[0].vector, vec![1.0, 0.0]);

        let count = qdrant_auth_request(crate::grpc::service::QdrantCountPointsRequest {
            collection_name: "docs".to_string(),
            filter,
        });
        let count = runtime
            .block_on(crate::grpc::service::QdrantPoints::count(&points, count))
            .unwrap()
            .into_inner();
        assert_eq!(count.count, 1);

        let scroll = qdrant_auth_request(crate::grpc::service::QdrantScrollPointsRequest {
            collection_name: "docs".to_string(),
            filter: None,
            limit: 10,
            offset: None,
            with_payload: true,
            with_vectors: false,
        });
        let scroll = runtime
            .block_on(crate::grpc::service::QdrantPoints::scroll(&points, scroll))
            .unwrap()
            .into_inner();
        assert_eq!(scroll.points.len(), 2);

        let delete = qdrant_auth_request(crate::grpc::service::QdrantPointIdsRequest {
            collection_name: "docs".to_string(),
            ids: vec![crate::grpc::service::QdrantPointId {
                point_id_options: Some(
                    crate::grpc::service::qdrant_point_id::PointIdOptions::Uuid("a".to_string()),
                ),
            }],
        });
        runtime
            .block_on(crate::grpc::service::QdrantPoints::delete(&points, delete))
            .unwrap();

        let get = qdrant_auth_request(crate::grpc::service::QdrantGetPointsRequest {
            collection_name: "docs".to_string(),
            ids: vec![crate::grpc::service::QdrantPointId {
                point_id_options: Some(
                    crate::grpc::service::qdrant_point_id::PointIdOptions::Uuid("a".to_string()),
                ),
            }],
            with_payload: true,
            with_vectors: true,
        });
        let get = runtime
            .block_on(crate::grpc::service::QdrantPoints::get(&points, get))
            .unwrap()
            .into_inner();
        assert!(get.result.is_empty());
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    #[test]
    fn grpc_etcd_profile_covers_kv_txn_and_lease_flow() {
        let path = temp_path("grpc-etcd-service");
        init(&path, None);
        let kernel = HostedKernel::new(&path);
        let kv =
            crate::grpc::service::HostedEtcdKvGrpcService::new(kernel.clone(), "main", "config");
        let lease = crate::grpc::service::HostedEtcdLeaseGrpcService::new(kernel, "main", "config");
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let grant =
            qdrant_auth_request(crate::grpc::service::EtcdLeaseGrantRequest { ttl: 60, id: 0 });
        let grant = runtime
            .block_on(crate::grpc::service::EtcdLease::lease_grant(&lease, grant))
            .unwrap()
            .into_inner();
        assert_eq!(grant.ttl, 60);
        assert!(grant.id > 0);

        let put = qdrant_auth_request(crate::grpc::service::EtcdPutRequest {
            key: b"alpha".to_vec(),
            value: b"one".to_vec(),
            lease: grant.id,
            prev_kv: false,
            ignore_value: false,
            ignore_lease: false,
        });
        let put = runtime
            .block_on(crate::grpc::service::EtcdKv::put(&kv, put))
            .unwrap()
            .into_inner();
        assert_eq!(put.header.unwrap().revision, 2);

        let range = qdrant_auth_request(crate::grpc::service::EtcdRangeRequest {
            key: b"alpha".to_vec(),
            range_end: Vec::new(),
            limit: 0,
            revision: 0,
            sort_order: 0,
            sort_target: 0,
            serializable: false,
            keys_only: false,
            count_only: false,
            min_mod_revision: 0,
            max_mod_revision: 0,
            min_create_revision: 0,
            max_create_revision: 0,
        });
        let range = runtime
            .block_on(crate::grpc::service::EtcdKv::range(&kv, range))
            .unwrap()
            .into_inner();
        assert_eq!(range.count, 1);
        assert_eq!(range.kvs[0].value, b"one".to_vec());
        assert_eq!(range.kvs[0].lease, grant.id);

        let txn = qdrant_auth_request(crate::grpc::service::EtcdTxnRequest {
            compare: vec![crate::grpc::service::EtcdCompare {
                result: crate::grpc::service::EtcdCompareResult::Equal as i32,
                target: crate::grpc::service::EtcdCompareTarget::Version as i32,
                key: b"alpha".to_vec(),
                value: Vec::new(),
                version: 1,
                create_revision: 0,
                mod_revision: 0,
                lease: 0,
                range_end: Vec::new(),
            }],
            success: vec![crate::grpc::service::EtcdRequestOp {
                request: Some(crate::grpc::service::etcd_request_op::Request::RequestPut(
                    crate::grpc::service::EtcdPutRequest {
                        key: b"alpha".to_vec(),
                        value: b"two".to_vec(),
                        lease: grant.id,
                        prev_kv: true,
                        ignore_value: false,
                        ignore_lease: false,
                    },
                )),
            }],
            failure: Vec::new(),
        });
        let txn = runtime
            .block_on(crate::grpc::service::EtcdKv::txn(&kv, txn))
            .unwrap()
            .into_inner();
        assert!(txn.succeeded);
        assert_eq!(txn.header.unwrap().revision, 3);
        assert_eq!(txn.responses.len(), 1);

        let compact = qdrant_auth_request(crate::grpc::service::EtcdCompactionRequest {
            revision: 2,
            physical: false,
        });
        let compact = runtime
            .block_on(crate::grpc::service::EtcdKv::compact(&kv, compact))
            .unwrap()
            .into_inner();
        assert_eq!(compact.header.unwrap().revision, 3);
        let compacted_range = qdrant_auth_request(crate::grpc::service::EtcdRangeRequest {
            key: b"alpha".to_vec(),
            range_end: Vec::new(),
            limit: 0,
            revision: 2,
            sort_order: 0,
            sort_target: 0,
            serializable: false,
            keys_only: false,
            count_only: false,
            min_mod_revision: 0,
            max_mod_revision: 0,
            min_create_revision: 0,
            max_create_revision: 0,
        });
        let err = runtime
            .block_on(crate::grpc::service::EtcdKv::range(&kv, compacted_range))
            .unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);

        let revoke =
            qdrant_auth_request(crate::grpc::service::EtcdLeaseRevokeRequest { id: grant.id });
        runtime
            .block_on(crate::grpc::service::EtcdLease::lease_revoke(
                &lease, revoke,
            ))
            .unwrap();
        let range = qdrant_auth_request(crate::grpc::service::EtcdRangeRequest {
            key: b"alpha".to_vec(),
            range_end: Vec::new(),
            limit: 0,
            revision: 0,
            sort_order: 0,
            sort_target: 0,
            serializable: false,
            keys_only: false,
            count_only: false,
            min_mod_revision: 0,
            max_mod_revision: 0,
            min_create_revision: 0,
            max_create_revision: 0,
        });
        let range = runtime
            .block_on(crate::grpc::service::EtcdKv::range(&kv, range))
            .unwrap()
            .into_inner();
        assert_eq!(range.count, 0);
        fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "grpc")]
    fn qdrant_auth_request<T>(message: T) -> tonic::Request<T> {
        let mut request = tonic::Request::new(message);
        add_root_metadata(&mut request);
        request
    }

    #[cfg(feature = "grpc")]
    fn columnar_row_cbor(values: &[loom_core::Value]) -> Vec<u8> {
        loom_codec::encode(&loom_codec::Value::Array(
            values.iter().map(loom_core::tabular::cell_value).collect(),
        ))
        .unwrap()
    }

    #[cfg(feature = "grpc")]
    fn columnar_select_columns_cbor(columns: &[&str]) -> Vec<u8> {
        loom_codec::encode(&loom_codec::Value::Array(
            columns
                .iter()
                .map(|column| loom_codec::Value::Text((*column).to_string()))
                .collect(),
        ))
        .unwrap()
    }

    #[cfg(feature = "grpc")]
    fn columnar_aggregates_cbor(aggregates: &[(u64, Option<&str>)]) -> Vec<u8> {
        loom_codec::encode(&loom_codec::Value::Array(
            aggregates
                .iter()
                .map(|(op, column)| {
                    loom_codec::Value::Array(vec![
                        loom_codec::Value::Uint(*op),
                        column
                            .map(|value| loom_codec::Value::Text(value.to_string()))
                            .unwrap_or(loom_codec::Value::Null),
                    ])
                })
                .collect(),
        ))
        .unwrap()
    }

    #[cfg(feature = "grpc")]
    fn add_root_metadata<T>(request: &mut tonic::Request<T>) {
        request
            .metadata_mut()
            .insert("x-loom-principal", nid(1).to_string().parse().unwrap());
        request
            .metadata_mut()
            .insert("x-loom-passphrase", "root-pass".parse().unwrap());
        request
            .metadata_mut()
            .insert("x-loom-session", "grpc-qdrant-test".parse().unwrap());
    }

    #[cfg(feature = "grpc")]
    fn qdrant_point(
        id: &str,
        vector: [f32; 2],
        lang: &str,
        score: i64,
    ) -> crate::grpc::service::QdrantPointStruct {
        crate::grpc::service::QdrantPointStruct {
            id: Some(crate::grpc::service::QdrantPointId {
                point_id_options: Some(
                    crate::grpc::service::qdrant_point_id::PointIdOptions::Uuid(id.to_string()),
                ),
            }),
            vector: vector.to_vec(),
            named_vectors: Vec::new(),
            payload: vec![
                crate::grpc::service::QdrantPayloadEntry {
                    key: "lang".to_string(),
                    value: Some(qdrant_string(lang)),
                },
                crate::grpc::service::QdrantPayloadEntry {
                    key: "score".to_string(),
                    value: Some(crate::grpc::service::QdrantValue {
                        kind: Some(crate::grpc::service::qdrant_value::Kind::IntegerValue(
                            score,
                        )),
                    }),
                },
            ],
        }
    }

    #[cfg(feature = "grpc")]
    fn qdrant_string(value: &str) -> crate::grpc::service::QdrantValue {
        crate::grpc::service::QdrantValue {
            kind: Some(crate::grpc::service::qdrant_value::Kind::StringValue(
                value.to_string(),
            )),
        }
    }
}
