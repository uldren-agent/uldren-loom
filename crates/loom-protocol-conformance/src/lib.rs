pub mod client_parity;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::Request;
use axum::http::header::CONTENT_TYPE;
use loom_core::Code;
use loom_core::{
    AclEffect, AclGrant, AclRight, AclScope, AclStore, AclSubject, Algo, Digest, FacetKind,
    IdentityStore, Loom, PrincipalKind, ProtectedRefPolicy, Value, WorkspaceId, key_to_cbor,
};
use loom_hosted::grpc::service::{
    Cas, CasDigestRequest, CasListRequest, CasPutRequest, HostedCasGrpcService,
    HostedQueueGrpcService, HostedTimeSeriesGrpcService, HostedVcsGrpcService,
    Queue as HostedQueue, QueueAppendRequest, QueueGetRequest, QueueLenRequest, QueueRangeRequest,
    TimeSeries as HostedTimeSeries, TimeSeriesGetRequest, TimeSeriesLatestRequest,
    TimeSeriesPutRequest, TimeSeriesRangeRequest, Vcs as HostedVcs, VcsCommitRequest,
};
use loom_hosted::serve::{
    cas_jsonrpc_router_with_policy, cas_rest_router_with_policy, data_jsonrpc_router_with_profile,
    data_rest_router_with_profile, vcs_jsonrpc_router_with_policy, vcs_rest_router_with_policy,
};
use loom_hosted::{
    HostedAuth, HostedAuthPolicy, HostedKernel, HostedWriteGuard, data_jsonrpc_router_with_policy,
    data_rest_router_with_policy,
};
use loom_lanes::{Lane, LaneInput, LaneStatus, LaneTicket, LaneTicketPlacement};
use loom_mcp::reads::StoreSearchReadRequest;
pub use loom_mcp::server::conformance::McpProtocolConformanceSummary;
use loom_mcp::tools::RemoteCapability;
use loom_mcp::writes::{LaneCreateRequest, LaneTicketUpdateRequest, LaneUpdateRequest};
use loom_mcp::{LoomMcp, StoreAccess};
use loom_store::{
    FileStore, LocalOpenAuth, NetworkAccessAction, NetworkAccessCidr, NetworkAccessPolicyRecord,
    NetworkAccessRule, attach_local_auth, open_loom_unlocked, save_loom,
};
use loom_substrate::chat::{ChatChannelDirectory, chat_channel_directory_key};
use loom_substrate::meetings::{
    AnnotationRecord, Coverage as MeetingsCoverage, ImportRunRecord, InputProfile, MeetingRecord,
    MeetingRecordInput, MeetingsProfileSnapshot, MeetingsProfileSnapshotParts, RedactionRecord,
    RedactionState, SourceRecord, SourceRecordInput, SpanKind, SpanRecord, meetings_profile_key,
};
use loom_substrate::versioning::{RevisionIndex, revision_index_path};
use tokio_stream::StreamExt;
use tonic::Request as GrpcRequest;
use tower::ServiceExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedProtocolConformanceSummary {
    pub suites_passed: usize,
    pub scenarios_passed: usize,
    pub suites: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolConformanceSummary {
    pub suites_passed: usize,
    pub scenarios_passed: usize,
    pub suites: Vec<&'static str>,
}

const HOSTED_PROTOCOL_CERTIFICATION_SCENARIOS: usize = 206;
const HOSTED_PROTOCOL_CERTIFICATION_SUITES: &[&str] = &[
    "hosted-meetings",
    "hosted-reference-reconciliation",
    "lanes-local-mcp-hosted-parity",
    "hosted-profile-transactions",
    "hosted-network-access",
    "hosted-cas-auth-acl",
    "hosted-timeseries-auth-acl",
    "hosted-timeseries-read-only-write-denial",
    "hosted-cas-rest-jsonrpc",
    "hosted-cas-grpc",
    "hosted-queue-grpc",
    "hosted-queue-read-only-write-denial",
    "hosted-timeseries-grpc",
    "hosted-queue-rest",
    "hosted-queue-jsonrpc",
    "hosted-timeseries-rest",
    "hosted-timeseries-jsonrpc",
    "hosted-ledger-rest",
    "hosted-ledger-jsonrpc",
    "hosted-ledger-read-only-write-denial",
    "hosted-fts-rest",
    "hosted-fts-jsonrpc",
    "hosted-graph-read-only-write-denial",
    "hosted-graph-rest",
    "hosted-graph-jsonrpc",
    "hosted-vector-read-only-write-denial",
    "hosted-vector-rest",
    "hosted-vector-jsonrpc",
    "hosted-columnar-read-only-write-denial",
    "hosted-columnar-result-handle-auth",
    "hosted-vcs-protected-ref-write",
    "hosted-columnar-rest",
    "hosted-columnar-jsonrpc",
    "hosted-kv-read-only-write-denial",
    "hosted-document-read-only-write-denial",
    "hosted-kv-rest",
    "hosted-kv-jsonrpc",
];

pub fn certify_in_process_mcp_protocol() -> Result<McpProtocolConformanceSummary, String> {
    loom_mcp::server::conformance::certify_in_process_mcp_protocol()
}

pub fn certify_in_process_hosted_protocol() -> Result<HostedProtocolConformanceSummary, String> {
    hosted_meetings_rest_and_jsonrpc_routes_project_snapshot()?;
    hosted_reference_reconciliation_adapters_preserve_auth()?;
    lane_behavioral_conformance_across_local_mcp_and_hosted()?;
    hosted_chat_drive_rest_and_jsonrpc_routes_project_revision_rows()?;
    hosted_network_access_matrix()?;
    hosted_cas_auth_acl_matrix()?;
    hosted_timeseries_auth_acl_matrix()?;
    hosted_timeseries_read_only_write_denial_matrix()?;
    hosted_cas_rest_and_jsonrpc_round_trip_matrix()?;
    hosted_cas_grpc_round_trip_matrix()?;
    hosted_queue_grpc_round_trip_matrix()?;
    hosted_queue_read_only_write_denial_matrix()?;
    hosted_timeseries_grpc_round_trip_matrix()?;
    hosted_queue_rest_round_trip_matrix()?;
    hosted_queue_jsonrpc_round_trip_matrix()?;
    hosted_timeseries_rest_round_trip_matrix()?;
    hosted_timeseries_jsonrpc_round_trip_matrix()?;
    hosted_ledger_rest_round_trip_matrix()?;
    hosted_ledger_jsonrpc_round_trip_matrix()?;
    hosted_ledger_read_only_write_denial_matrix()?;
    hosted_fts_rest_round_trip_matrix()?;
    hosted_fts_jsonrpc_round_trip_matrix()?;
    hosted_graph_read_only_write_denial_matrix()?;
    hosted_graph_rest_round_trip_matrix()?;
    hosted_graph_jsonrpc_round_trip_matrix()?;
    hosted_vector_read_only_write_denial_matrix()?;
    hosted_vector_rest_round_trip_matrix()?;
    hosted_vector_jsonrpc_round_trip_matrix()?;
    hosted_columnar_read_only_write_denial_matrix()?;
    hosted_columnar_result_handle_auth_matrix()?;
    hosted_vcs_protected_ref_write_matrix()?;
    hosted_columnar_rest_round_trip_matrix()?;
    hosted_columnar_jsonrpc_round_trip_matrix()?;
    hosted_kv_read_only_write_denial_matrix()?;
    hosted_document_read_only_write_denial_matrix()?;
    hosted_kv_rest_round_trip_matrix()?;
    hosted_kv_jsonrpc_round_trip_matrix()?;
    Ok(hosted_protocol_certification_summary())
}

fn hosted_protocol_certification_summary() -> HostedProtocolConformanceSummary {
    HostedProtocolConformanceSummary {
        suites_passed: HOSTED_PROTOCOL_CERTIFICATION_SUITES.len(),
        scenarios_passed: HOSTED_PROTOCOL_CERTIFICATION_SCENARIOS,
        suites: HOSTED_PROTOCOL_CERTIFICATION_SUITES.to_vec(),
    }
}

fn hosted_cas_auth_acl_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-cas-auth-acl");
        let workspace = seed_cas_auth_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = cas_rest_router_with_policy(
            kernel.clone(),
            workspace,
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let jsonrpc = cas_jsonrpc_router_with_policy(
            kernel.clone(),
            workspace,
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let user = nid(7);

        let missing = cas_http_request(rest.clone(), "GET", "/cas", None, None, "").await?;
        expect_status_and_code(&missing, 401, "AUTHENTICATION_FAILED")?;
        let bad = cas_http_request(rest.clone(), "GET", "/cas", Some(nid(1)), Some("bad"), "").await?;
        expect_status_and_code(&bad, 401, "AUTHENTICATION_FAILED")?;
        let denied = cas_http_request(rest.clone(), "PUT", "/cas", Some(user), Some("alice-pass"), "denied").await?;
        expect_status_and_code(&denied, 403, "PERMISSION_DENIED")?;

        let json_denied = cas_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            Some(user),
            Some("alice-pass"),
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"cas.list\",\"params\":{}}",
        )
        .await?;
        if json_denied.0 != 200 || !json_denied.1.contains("PERMISSION_DENIED") {
            return Err(format!(
                "JSON-RPC denied CAS list returned {}: {}",
                json_denied.0, json_denied.1
            ));
        }

        let service = HostedCasGrpcService::new(kernel.clone(), workspace);
        let grpc_denied = service
            .put(cas_grpc_request(CasPutRequest { bytes: b"denied".to_vec() }, user, "alice-pass"))
            .await
            .expect_err("ungranted gRPC CAS write must be denied");
        if grpc_denied.code() != tonic::Code::PermissionDenied {
            return Err(format!("gRPC denied CAS write returned {}", grpc_denied.code()));
        }

        let grant = cas_read_write_grant(user, workspace);
        update_cas_acl(&path, &grant, true)?;
        let created = cas_http_request(rest.clone(), "PUT", "/cas", Some(user), Some("alice-pass"), "allowed").await?;
        if created.0 != 201 {
            return Err(format!("granted REST CAS write returned {}", created.0));
        }
        let digest = serde_json::from_str::<serde_json::Value>(&created.1)
            .map_err(strerr)?
            .get("digest")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "REST CAS write omitted digest".to_string())?
            .to_string();
        let json_allowed = cas_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            Some(user),
            Some("alice-pass"),
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"cas.has\",\"params\":{{\"digest\":\"{digest}\"}}}}"
            ),
        )
        .await?;
        if json_allowed.0 != 200 || !json_allowed.1.contains("\"present\":true") {
            return Err("granted JSON-RPC CAS read did not confirm presence".to_string());
        }
        let grpc = service
            .get(cas_grpc_request(CasDigestRequest { digest: digest.clone() }, user, "alice-pass"))
            .await
            .map_err(|status| format!("granted gRPC CAS read failed: {status}"))?;
        if !grpc.get_ref().found || grpc.get_ref().bytes != b"allowed" {
            return Err("granted gRPC CAS read returned the wrong content".to_string());
        }

        update_cas_acl(&path, &grant, false)?;
        let revoked = cas_http_request(rest, "GET", &format!("/cas/{digest}"), Some(user), Some("alice-pass"), "").await?;
        expect_status_and_code(&revoked, 403, "PERMISSION_DENIED")?;
        let grpc_revoked = service
            .get(cas_grpc_request(CasDigestRequest { digest }, user, "alice-pass"))
            .await
            .expect_err("revoked gRPC CAS read must be denied");
        if grpc_revoked.code() != tonic::Code::PermissionDenied {
            return Err(format!("revoked gRPC CAS read returned {}", grpc_revoked.code()));
        }
        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_timeseries_auth_acl_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-timeseries-auth-acl");
        let workspace = seed_timeseries_auth_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_policy(
            kernel.clone(),
            "time-series",
            "main",
            "metrics",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let jsonrpc = data_jsonrpc_router_with_policy(
            kernel.clone(),
            "time-series",
            "main",
            "metrics",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let service = HostedTimeSeriesGrpcService::new(kernel, "main", "metrics");
        let user = nid(7);

        let missing =
            data_http_request_auth(rest.clone(), "POST", "/time-series:latest", None, None, "")
                .await?;
        expect_status_and_code(&missing, 401, "AUTHENTICATION_FAILED")?;
        let bad = data_http_request_auth(
            rest.clone(),
            "POST",
            "/time-series:latest",
            Some(nid(1)),
            Some("bad"),
            "",
        )
        .await?;
        expect_status_and_code(&bad, 403, "AUTHENTICATION_FAILED")?;
        let denied = data_http_request_auth(
            rest.clone(),
            "POST",
            "/time-series:put",
            Some(user),
            Some("alice-pass"),
            "{\"timestamp\":100,\"value_hex\":\"64656e696564\"}",
        )
        .await?;
        expect_status_and_code(&denied, 403, "PERMISSION_DENIED")?;

        let json_denied = data_http_request_auth(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            Some(user),
            Some("alice-pass"),
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"timeseries.latest\",\"params\":{}}",
        )
        .await?;
        if json_denied.0 != 200 || !json_denied.1.contains("PERMISSION_DENIED") {
            return Err(format!(
                "JSON-RPC denied TimeSeries latest returned {}: {}",
                json_denied.0, json_denied.1
            ));
        }

        let grpc_denied = service
            .put(grpc_auth_request(
                TimeSeriesPutRequest {
                    timestamp: 100,
                    value: b"denied".to_vec(),
                },
                user,
                "alice-pass",
            ))
            .await
            .expect_err("ungranted gRPC TimeSeries write must be denied");
        if grpc_denied.code() != tonic::Code::PermissionDenied {
            return Err(format!(
                "gRPC denied TimeSeries write returned {}",
                grpc_denied.code()
            ));
        }

        let grant = timeseries_read_write_grant(user, workspace);
        update_timeseries_acl(&path, &grant, true)?;
        let created = data_http_request_auth(
            rest.clone(),
            "POST",
            "/time-series:put",
            Some(user),
            Some("alice-pass"),
            "{\"timestamp\":100,\"value_hex\":\"616c6c6f776564\"}",
        )
        .await?;
        expect_status_and_contains(&created, 200, "\"ok\":true", "TimeSeries REST put")?;

        let json_allowed = data_http_request_auth(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            Some(user),
            Some("alice-pass"),
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"timeseries.latest\",\"params\":{}}",
        )
        .await?;
        if json_allowed.0 != 200 || !json_allowed.1.contains("\"value_hex\":\"616c6c6f776564\"") {
            return Err(format!(
                "granted JSON-RPC TimeSeries latest returned {}: {}",
                json_allowed.0, json_allowed.1
            ));
        }
        let grpc = service
            .get(grpc_auth_request(
                TimeSeriesGetRequest { timestamp: 100 },
                user,
                "alice-pass",
            ))
            .await
            .map_err(|status| format!("granted gRPC TimeSeries read failed: {status}"))?;
        let Some(point) = grpc.get_ref().point.as_ref() else {
            return Err("granted gRPC TimeSeries read returned no point".to_string());
        };
        if !grpc.get_ref().found || point.value != b"allowed" {
            return Err("granted gRPC TimeSeries read returned the wrong content".to_string());
        }

        update_timeseries_acl(&path, &grant, false)?;
        let revoked = data_http_request_auth(
            rest,
            "POST",
            "/time-series:get",
            Some(user),
            Some("alice-pass"),
            "{\"timestamp\":100}",
        )
        .await?;
        expect_status_and_code(&revoked, 403, "PERMISSION_DENIED")?;
        let grpc_revoked = service
            .get(grpc_auth_request(
                TimeSeriesGetRequest { timestamp: 100 },
                user,
                "alice-pass",
            ))
            .await
            .expect_err("revoked gRPC TimeSeries read must be denied");
        if grpc_revoked.code() != tonic::Code::PermissionDenied {
            return Err(format!(
                "revoked gRPC TimeSeries read returned {}",
                grpc_revoked.code()
            ));
        }
        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_timeseries_read_only_write_denial_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-timeseries-read-only-write-denial");
        let workspace = seed_timeseries_auth_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_policy(
            kernel.clone(),
            "time-series",
            "main",
            "metrics",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let jsonrpc = data_jsonrpc_router_with_policy(
            kernel.clone(),
            "time-series",
            "main",
            "metrics",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let service = HostedTimeSeriesGrpcService::new(kernel, "main", "metrics");
        let user = nid(7);
        let grant = timeseries_read_only_grant(user, workspace);
        update_timeseries_acl(&path, &grant, true)?;

        let rest_denied = data_http_request_auth(
            rest,
            "POST",
            "/time-series:put",
            Some(user),
            Some("alice-pass"),
            "{\"timestamp\":200,\"value_hex\":\"726573742d64656e696564\"}",
        )
        .await?;
        expect_status_and_code(&rest_denied, 403, "PERMISSION_DENIED")?;

        let json_denied = data_http_request_auth(
            jsonrpc,
            "POST",
            "/jsonrpc",
            Some(user),
            Some("alice-pass"),
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"timeseries.put\",\"params\":{\"timestamp\":200,\"value_hex\":\"6a736f6e2d64656e696564\"}}",
        )
        .await?;
        if json_denied.0 != 200 || !json_denied.1.contains("PERMISSION_DENIED") {
            return Err(format!(
                "JSON-RPC read-only TimeSeries write returned {}: {}",
                json_denied.0, json_denied.1
            ));
        }

        let grpc_denied = service
            .put(grpc_auth_request(
                TimeSeriesPutRequest {
                    timestamp: 200,
                    value: b"grpc-denied".to_vec(),
                },
                user,
                "alice-pass",
            ))
            .await
            .expect_err("read-only gRPC TimeSeries write must be denied");
        if grpc_denied.code() != tonic::Code::PermissionDenied {
            return Err(format!(
                "gRPC read-only TimeSeries write returned {}",
                grpc_denied.code()
            ));
        }

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_cas_rest_and_jsonrpc_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-cas-rest-jsonrpc");
        let workspace = seed_cas_auth_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = cas_rest_router_with_policy(
            kernel.clone(),
            workspace,
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let jsonrpc = cas_jsonrpc_router_with_policy(
            kernel,
            workspace,
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let principal = nid(1);
        let missing = Digest::hash(Algo::Blake3, b"missing-cas-payload").to_string();

        let rest_created =
            cas_http_request(rest.clone(), "PUT", "/cas", Some(principal), Some("root-pass"), "rest-alpha").await?;
        if rest_created.0 != 201 {
            return Err(format!("REST CAS put returned {}", rest_created.0));
        }
        let rest_digest = json_string_field(&rest_created.1, "digest")?;
        let rest_get = cas_http_request(
            rest.clone(),
            "GET",
            &format!("/cas/{rest_digest}"),
            Some(principal),
            Some("root-pass"),
            "",
        )
        .await?;
        if rest_get != (200, "rest-alpha".to_string()) {
            return Err(format!("REST CAS get returned {}: {}", rest_get.0, rest_get.1));
        }
        let rest_missing = cas_http_request(
            rest.clone(),
            "GET",
            &format!("/cas/{missing}"),
            Some(principal),
            Some("root-pass"),
            "",
        )
        .await?;
        expect_status_and_code(&rest_missing, 404, "NOT_FOUND")?;
        let rest_invalid = cas_http_request(
            rest.clone(),
            "GET",
            "/cas/not-a-digest",
            Some(principal),
            Some("root-pass"),
            "",
        )
        .await?;
        expect_status_and_code(&rest_invalid, 400, "INVALID_ARGUMENT")?;
        let rest_head = cas_http_request(
            rest.clone(),
            "HEAD",
            &format!("/cas/{rest_digest}"),
            Some(principal),
            Some("root-pass"),
            "",
        )
        .await?;
        if rest_head.0 != 204 {
            return Err(format!("REST CAS head returned {}", rest_head.0));
        }
        let rest_list =
            cas_http_request(rest.clone(), "GET", "/cas", Some(principal), Some("root-pass"), "").await?;
        if rest_list.0 != 200 || !rest_list.1.contains(&rest_digest) {
            return Err(format!("REST CAS list omitted digest: {} {}", rest_list.0, rest_list.1));
        }
        let rest_deleted = cas_http_request(
            rest.clone(),
            "DELETE",
            &format!("/cas/{rest_digest}"),
            Some(principal),
            Some("root-pass"),
            "",
        )
        .await?;
        if rest_deleted.0 != 200 || !rest_deleted.1.contains("\"deleted\":true") {
            return Err(format!(
                "REST CAS delete returned {}: {}",
                rest_deleted.0, rest_deleted.1
            ));
        }
        let rest_deleted_missing = cas_http_request(
            rest,
            "GET",
            &format!("/cas/{rest_digest}"),
            Some(principal),
            Some("root-pass"),
            "",
        )
        .await?;
        expect_status_and_code(&rest_deleted_missing, 404, "NOT_FOUND")?;

        let json_created = cas_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            Some(principal),
            Some("root-pass"),
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"cas.put\",\"params\":{\"bytes_hex\":\"6a736f6e2d616c706861\"}}",
        )
        .await?;
        if json_created.0 != 200 {
            return Err(format!("JSON-RPC CAS put returned {}", json_created.0));
        }
        let json_digest = json_result_string_field(&json_created.1, "digest")?;
        let json_get = cas_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            Some(principal),
            Some("root-pass"),
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"cas.get\",\"params\":{{\"digest\":\"{json_digest}\"}}}}"
            ),
        )
        .await?;
        if json_get.0 != 200 || !json_get.1.contains("\"bytes_hex\":\"6a736f6e2d616c706861\"") {
            return Err(format!("JSON-RPC CAS get returned {}: {}", json_get.0, json_get.1));
        }
        let json_missing = cas_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            Some(principal),
            Some("root-pass"),
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"cas.get\",\"params\":{{\"digest\":\"{missing}\"}}}}"
            ),
        )
        .await?;
        if json_missing.0 != 200 || !json_missing.1.contains("\"bytes_hex\":null") {
            return Err(format!(
                "JSON-RPC missing CAS get returned {}: {}",
                json_missing.0, json_missing.1
            ));
        }
        let json_invalid = cas_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            Some(principal),
            Some("root-pass"),
            "{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"cas.get\",\"params\":{\"digest\":\"not-a-digest\"}}",
        )
        .await?;
        if json_invalid.0 != 400 || !json_invalid.1.contains("\"code\":\"INVALID_ARGUMENT\"") {
            return Err(format!(
                "JSON-RPC invalid CAS digest returned {}: {}",
                json_invalid.0, json_invalid.1
            ));
        }
        let json_has = cas_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            Some(principal),
            Some("root-pass"),
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"cas.has\",\"params\":{{\"digest\":\"{json_digest}\"}}}}"
            ),
        )
        .await?;
        if json_has.0 != 200 || !json_has.1.contains("\"present\":true") {
            return Err(format!("JSON-RPC CAS has returned {}: {}", json_has.0, json_has.1));
        }
        let json_list = cas_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            Some(principal),
            Some("root-pass"),
            "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"cas.list\",\"params\":{}}",
        )
        .await?;
        if json_list.0 != 200 || !json_list.1.contains(&json_digest) {
            return Err(format!(
                "JSON-RPC CAS list omitted digest: {} {}",
                json_list.0, json_list.1
            ));
        }
        let json_deleted = cas_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            Some(principal),
            Some("root-pass"),
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"cas.delete\",\"params\":{{\"digest\":\"{json_digest}\"}}}}"
            ),
        )
        .await?;
        if json_deleted.0 != 200 || !json_deleted.1.contains("\"deleted\":true") {
            return Err(format!(
                "JSON-RPC CAS delete returned {}: {}",
                json_deleted.0, json_deleted.1
            ));
        }
        let json_deleted_missing = cas_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            Some(principal),
            Some("root-pass"),
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"cas.get\",\"params\":{{\"digest\":\"{json_digest}\"}}}}"
            ),
        )
        .await?;
        if json_deleted_missing.0 != 200 || !json_deleted_missing.1.contains("\"bytes_hex\":null") {
            return Err(format!(
                "JSON-RPC CAS get after delete returned {}: {}",
                json_deleted_missing.0, json_deleted_missing.1
            ));
        }
        let json_has_deleted = cas_http_request(
            jsonrpc,
            "POST",
            "/jsonrpc",
            Some(principal),
            Some("root-pass"),
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"cas.has\",\"params\":{{\"digest\":\"{json_digest}\"}}}}"
            ),
        )
        .await?;
        if json_has_deleted.0 != 200 || !json_has_deleted.1.contains("\"present\":false") {
            return Err(format!(
                "JSON-RPC CAS has after delete returned {}: {}",
                json_has_deleted.0, json_has_deleted.1
            ));
        }

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_cas_grpc_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-cas-grpc");
        let workspace = seed_cas_auth_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let service = HostedCasGrpcService::new(kernel, workspace);
        let principal = nid(1);
        let missing = Digest::hash(Algo::Blake3, b"missing-cas-grpc-payload").to_string();

        let created = service
            .put(cas_grpc_request(
                CasPutRequest {
                    bytes: b"grpc-alpha".to_vec(),
                },
                principal,
                "root-pass",
            ))
            .await
            .map_err(|status| format!("CAS gRPC put failed: {status}"))?;
        let digest = created.get_ref().digest.clone();
        if digest.is_empty() {
            return Err("CAS gRPC put returned an empty digest".to_string());
        }

        let got = service
            .get(cas_grpc_request(
                CasDigestRequest {
                    digest: digest.clone(),
                },
                principal,
                "root-pass",
            ))
            .await
            .map_err(|status| format!("CAS gRPC get failed: {status}"))?;
        if !got.get_ref().found || got.get_ref().bytes != b"grpc-alpha" {
            return Err("CAS gRPC get returned the wrong content".to_string());
        }

        let missing_get = service
            .get(cas_grpc_request(
                CasDigestRequest { digest: missing },
                principal,
                "root-pass",
            ))
            .await
            .map_err(|status| format!("CAS gRPC missing get failed: {status}"))?;
        if missing_get.get_ref().found || !missing_get.get_ref().bytes.is_empty() {
            return Err("CAS gRPC missing get returned content".to_string());
        }
        let invalid_get = service
            .get(cas_grpc_request(
                CasDigestRequest {
                    digest: "not-a-digest".to_string(),
                },
                principal,
                "root-pass",
            ))
            .await
            .expect_err("CAS gRPC invalid digest must fail");
        if invalid_get.code() != tonic::Code::InvalidArgument {
            return Err(format!(
                "CAS gRPC invalid digest returned {}",
                invalid_get.code()
            ));
        }

        let has = service
            .has(cas_grpc_request(
                CasDigestRequest {
                    digest: digest.clone(),
                },
                principal,
                "root-pass",
            ))
            .await
            .map_err(|status| format!("CAS gRPC has failed: {status}"))?;
        if !has.get_ref().present {
            return Err("CAS gRPC has did not confirm presence".to_string());
        }

        let listed = service
            .list(cas_grpc_request(CasListRequest {}, principal, "root-pass"))
            .await
            .map_err(|status| format!("CAS gRPC list failed: {status}"))?;
        if !listed.get_ref().digests.contains(&digest) {
            return Err("CAS gRPC list omitted the created digest".to_string());
        }

        let deleted = service
            .delete(cas_grpc_request(
                CasDigestRequest {
                    digest: digest.clone(),
                },
                principal,
                "root-pass",
            ))
            .await
            .map_err(|status| format!("CAS gRPC delete failed: {status}"))?;
        if !deleted.get_ref().deleted {
            return Err("CAS gRPC delete did not report deletion".to_string());
        }
        let deleted_get = service
            .get(cas_grpc_request(
                CasDigestRequest {
                    digest: digest.clone(),
                },
                principal,
                "root-pass",
            ))
            .await
            .map_err(|status| format!("CAS gRPC get after delete failed: {status}"))?;
        if deleted_get.get_ref().found || !deleted_get.get_ref().bytes.is_empty() {
            return Err("CAS gRPC get after delete returned content".to_string());
        }

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_queue_grpc_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-queue-grpc");
        seed_queue_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let service = HostedQueueGrpcService::new(kernel, "main", "events");
        let principal = nid(1);

        let seq0 = service
            .append(grpc_auth_request(
                QueueAppendRequest {
                    payload: b"one".to_vec(),
                },
                principal,
                "root-pass",
            ))
            .await
            .map_err(strerr)?
            .into_inner()
            .seq;
        if seq0 != 0 {
            return Err(format!("Queue gRPC append returned first seq {seq0}"));
        }
        let seq1 = service
            .append(grpc_auth_request(
                QueueAppendRequest {
                    payload: b"two".to_vec(),
                },
                principal,
                "root-pass",
            ))
            .await
            .map_err(strerr)?
            .into_inner()
            .seq;
        let seq2 = service
            .append(grpc_auth_request(
                QueueAppendRequest {
                    payload: b"three".to_vec(),
                },
                principal,
                "root-pass",
            ))
            .await
            .map_err(strerr)?
            .into_inner()
            .seq;
        if (seq1, seq2) != (1, 2) {
            return Err(format!("Queue gRPC append returned seqs {seq1}, {seq2}"));
        }

        let len = service
            .len(grpc_auth_request(
                QueueLenRequest {},
                principal,
                "root-pass",
            ))
            .await
            .map_err(strerr)?
            .into_inner()
            .len;
        if len != 3 {
            return Err(format!("Queue gRPC len returned {len}"));
        }

        let get = service
            .get(grpc_auth_request(
                QueueGetRequest { seq: 1 },
                principal,
                "root-pass",
            ))
            .await
            .map_err(strerr)?
            .into_inner();
        if !get.found || get.payload != b"two" {
            return Err(format!(
                "Queue gRPC get returned found={} payload={:?}",
                get.found, get.payload
            ));
        }

        let range = service
            .range(grpc_auth_request(
                QueueRangeRequest { lo: 1, hi: 3 },
                principal,
                "root-pass",
            ))
            .await
            .map_err(strerr)?
            .into_inner();
        if range.entries.len() != 2
            || range.entries[0].seq != 1
            || range.entries[0].payload != b"two"
            || range.entries[1].seq != 2
            || range.entries[1].payload != b"three"
        {
            return Err(format!(
                "Queue gRPC range returned {} entries",
                range.entries.len()
            ));
        }

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_timeseries_grpc_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-timeseries-grpc");
        seed_timeseries_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let service = HostedTimeSeriesGrpcService::new(kernel, "main", "metrics");
        let principal = nid(1);

        for (timestamp, value) in [
            (100, b"p100".to_vec()),
            (200, b"p200".to_vec()),
            (300, b"p300".to_vec()),
        ] {
            service
                .put(grpc_auth_request(
                    TimeSeriesPutRequest { timestamp, value },
                    principal,
                    "root-pass",
                ))
                .await
                .map_err(strerr)?;
        }

        let get = service
            .get(grpc_auth_request(
                TimeSeriesGetRequest { timestamp: 100 },
                principal,
                "root-pass",
            ))
            .await
            .map_err(strerr)?
            .into_inner();
        let Some(point) = get.point else {
            return Err("Time-series gRPC get returned no point".to_string());
        };
        if !get.found || point.timestamp != 100 || point.value != b"p100" {
            return Err(format!(
                "Time-series gRPC get returned found={} timestamp={}",
                get.found, point.timestamp
            ));
        }

        let latest = service
            .latest(grpc_auth_request(
                TimeSeriesLatestRequest {},
                principal,
                "root-pass",
            ))
            .await
            .map_err(strerr)?
            .into_inner();
        let Some(point) = latest.point else {
            return Err("Time-series gRPC latest returned no point".to_string());
        };
        if !latest.found || point.timestamp != 300 || point.value != b"p300" {
            return Err(format!(
                "Time-series gRPC latest returned found={} timestamp={}",
                latest.found, point.timestamp
            ));
        }

        let mut stream = service
            .range(grpc_auth_request(
                TimeSeriesRangeRequest {
                    from: 100,
                    to: 301,
                    batch_size: 2,
                },
                principal,
                "root-pass",
            ))
            .await
            .map_err(strerr)?
            .into_inner();
        let mut batches = Vec::new();
        while let Some(batch) = stream.next().await {
            batches.push(batch.map_err(strerr)?);
        }
        if batches.len() != 2
            || batches[0].points.len() != 2
            || batches[0].points[0].timestamp != 100
            || batches[0].points[0].value != b"p100"
            || batches[0].points[1].timestamp != 200
            || batches[0].points[1].value != b"p200"
            || batches[1].points.len() != 1
            || batches[1].points[0].timestamp != 300
            || batches[1].points[0].value != b"p300"
        {
            return Err(format!(
                "Time-series gRPC range returned {} batches",
                batches.len()
            ));
        }

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_queue_read_only_write_denial_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-queue-read-only-write-denial");
        let workspace = seed_queue_auth_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_policy(
            kernel.clone(),
            "queue",
            "main",
            "events",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let jsonrpc = data_jsonrpc_router_with_policy(
            kernel.clone(),
            "queue",
            "main",
            "events",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let service = HostedQueueGrpcService::new(kernel, "main", "events");
        let user = nid(7);
        let grant = queue_read_only_grant(user, workspace);
        update_queue_acl(&path, &grant, true)?;

        let rest_denied = data_http_request_auth(
            rest,
            "POST",
            "/queue:append",
            Some(user),
            Some("alice-pass"),
            "{\"payload_hex\":\"726573742d64656e696564\"}",
        )
        .await?;
        expect_status_and_code(&rest_denied, 403, "PERMISSION_DENIED")?;

        let json_denied = data_http_request_auth(
            jsonrpc,
            "POST",
            "/jsonrpc",
            Some(user),
            Some("alice-pass"),
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"queue.append\",\"params\":{\"payload_hex\":\"6a736f6e2d64656e696564\"}}",
        )
        .await?;
        if json_denied.0 != 200 || !json_denied.1.contains("PERMISSION_DENIED") {
            return Err(format!(
                "JSON-RPC read-only Queue write returned {}: {}",
                json_denied.0, json_denied.1
            ));
        }

        let grpc_denied = service
            .append(grpc_auth_request(
                QueueAppendRequest {
                    payload: b"grpc-denied".to_vec(),
                },
                user,
                "alice-pass",
            ))
            .await
            .expect_err("read-only gRPC Queue write must be denied");
        if grpc_denied.code() != tonic::Code::PermissionDenied {
            return Err(format!(
                "gRPC read-only Queue write returned {}",
                grpc_denied.code()
            ));
        }

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_queue_rest_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-queue-rest");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_profile(
            kernel,
            "queue",
            "main",
            "events",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let append = data_http_request(
            rest.clone(),
            "POST",
            "/queue:append",
            "{\"payload_hex\":\"6f6e65\"}",
        )
        .await?;
        expect_status_and_contains(&append, 200, "\"seq\":0", "Queue REST append")?;
        let append = data_http_request(
            rest.clone(),
            "POST",
            "/queue:append",
            "{\"payload_hex\":\"74776f\"}",
        )
        .await?;
        expect_status_and_contains(&append, 200, "\"seq\":1", "Queue REST append")?;
        let get = data_http_request(rest.clone(), "POST", "/queue:get", "{\"seq\":1}").await?;
        expect_status_and_contains(&get, 200, "\"payload_hex\":\"74776f\"", "Queue REST get")?;
        let range =
            data_http_request(rest.clone(), "POST", "/queue:range", "{\"lo\":0,\"hi\":2}").await?;
        expect_status_and_contains(&range, 200, "\"seq\":0", "Queue REST range")?;
        expect_status_and_contains(
            &range,
            200,
            "\"payload_hex\":\"6f6e65\"",
            "Queue REST range",
        )?;
        expect_status_and_contains(&range, 200, "\"seq\":1", "Queue REST range")?;
        expect_status_and_contains(
            &range,
            200,
            "\"payload_hex\":\"74776f\"",
            "Queue REST range",
        )?;
        let len = data_http_request(rest, "POST", "/queue:len", "{}").await?;
        expect_status_and_contains(&len, 200, "\"len\":2", "Queue REST len")?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_queue_jsonrpc_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-queue-jsonrpc");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let jsonrpc = data_jsonrpc_router_with_profile(
            kernel,
            "queue",
            "main",
            "events",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let append = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"queue.append\",\"params\":{\"payload_hex\":\"6f6e65\"}}",
        )
        .await?;
        expect_status_and_contains(&append, 200, "\"seq\":0", "Queue JSON-RPC append")?;
        let append = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"queue.append\",\"params\":{\"payload_hex\":\"74776f\"}}",
        )
        .await?;
        expect_status_and_contains(&append, 200, "\"seq\":1", "Queue JSON-RPC append")?;
        let get = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"queue.get\",\"params\":{\"seq\":1}}",
        )
        .await?;
        expect_status_and_contains(&get, 200, "\"payload_hex\":\"74776f\"", "Queue JSON-RPC get")?;
        let range = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"queue.range\",\"params\":{\"lo\":0,\"hi\":2}}",
        )
        .await?;
        expect_status_and_contains(&range, 200, "\"seq\":0", "Queue JSON-RPC range")?;
        expect_status_and_contains(
            &range,
            200,
            "\"payload_hex\":\"6f6e65\"",
            "Queue JSON-RPC range",
        )?;
        expect_status_and_contains(&range, 200, "\"seq\":1", "Queue JSON-RPC range")?;
        expect_status_and_contains(
            &range,
            200,
            "\"payload_hex\":\"74776f\"",
            "Queue JSON-RPC range",
        )?;
        let len = data_http_request(
            jsonrpc,
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"queue.len\",\"params\":{}}",
        )
        .await?;
        expect_status_and_contains(&len, 200, "\"len\":2", "Queue JSON-RPC len")?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_timeseries_rest_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-timeseries-rest");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_profile(
            kernel,
            "time-series",
            "main",
            "metrics",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let put = data_http_request(
            rest.clone(),
            "POST",
            "/time-series:put",
            "{\"timestamp\":100,\"value_hex\":\"70313030\"}",
        )
        .await?;
        expect_status_and_contains(&put, 200, "\"ok\":true", "Time-series REST put")?;
        let put = data_http_request(
            rest.clone(),
            "POST",
            "/time-series:put",
            "{\"timestamp\":200,\"value_hex\":\"70323030\"}",
        )
        .await?;
        expect_status_and_contains(&put, 200, "\"ok\":true", "Time-series REST put")?;
        let get = data_http_request(
            rest.clone(),
            "POST",
            "/time-series:get",
            "{\"timestamp\":100}",
        )
        .await?;
        expect_status_and_contains(
            &get,
            200,
            "\"value_hex\":\"70313030\"",
            "Time-series REST get",
        )?;
        let latest = data_http_request(rest.clone(), "POST", "/time-series:latest", "{}").await?;
        expect_status_and_contains(&latest, 200, "\"timestamp\":200", "Time-series REST latest")?;
        expect_status_and_contains(
            &latest,
            200,
            "\"value_hex\":\"70323030\"",
            "Time-series REST latest",
        )?;
        let range = data_http_request(
            rest,
            "POST",
            "/time-series:range",
            "{\"from\":50,\"to\":250}",
        )
        .await?;
        expect_status_and_contains(&range, 200, "\"timestamp\":100", "Time-series REST range")?;
        expect_status_and_contains(
            &range,
            200,
            "\"value_hex\":\"70313030\"",
            "Time-series REST range",
        )?;
        expect_status_and_contains(&range, 200, "\"timestamp\":200", "Time-series REST range")?;
        expect_status_and_contains(
            &range,
            200,
            "\"value_hex\":\"70323030\"",
            "Time-series REST range",
        )?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_timeseries_jsonrpc_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-timeseries-jsonrpc");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let jsonrpc = data_jsonrpc_router_with_profile(
            kernel,
            "time-series",
            "main",
            "metrics",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let put = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"timeseries.put\",\"params\":{\"timestamp\":100,\"value_hex\":\"70313030\"}}",
        )
        .await?;
        expect_status_and_contains(&put, 200, "\"ok\":true", "Time-series JSON-RPC put")?;
        let put = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"timeseries.put\",\"params\":{\"timestamp\":200,\"value_hex\":\"70323030\"}}",
        )
        .await?;
        expect_status_and_contains(&put, 200, "\"ok\":true", "Time-series JSON-RPC put")?;
        let get = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"timeseries.get\",\"params\":{\"timestamp\":100}}",
        )
        .await?;
        expect_status_and_contains(
            &get,
            200,
            "\"value_hex\":\"70313030\"",
            "Time-series JSON-RPC get",
        )?;
        let latest = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"timeseries.latest\",\"params\":{}}",
        )
        .await?;
        expect_status_and_contains(
            &latest,
            200,
            "\"timestamp\":200",
            "Time-series JSON-RPC latest",
        )?;
        expect_status_and_contains(
            &latest,
            200,
            "\"value_hex\":\"70323030\"",
            "Time-series JSON-RPC latest",
        )?;
        let range = data_http_request(
            jsonrpc,
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"timeseries.range\",\"params\":{\"from\":50,\"to\":250}}",
        )
        .await?;
        expect_status_and_contains(
            &range,
            200,
            "\"timestamp\":100",
            "Time-series JSON-RPC range",
        )?;
        expect_status_and_contains(
            &range,
            200,
            "\"value_hex\":\"70313030\"",
            "Time-series JSON-RPC range",
        )?;
        expect_status_and_contains(
            &range,
            200,
            "\"timestamp\":200",
            "Time-series JSON-RPC range",
        )?;
        expect_status_and_contains(
            &range,
            200,
            "\"value_hex\":\"70323030\"",
            "Time-series JSON-RPC range",
        )?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_ledger_rest_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-ledger-rest");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_profile(
            kernel,
            "ledger",
            "main",
            "audit",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let append = data_http_request(
            rest.clone(),
            "POST",
            "/ledger:append",
            "{\"payload_hex\":\"656e7472792d30\"}",
        )
        .await?;
        expect_status_and_contains(&append, 200, "\"seq\":0", "Ledger REST append")?;
        let append = data_http_request(
            rest.clone(),
            "POST",
            "/ledger:append",
            "{\"payload_hex\":\"656e7472792d31\"}",
        )
        .await?;
        expect_status_and_contains(&append, 200, "\"seq\":1", "Ledger REST append")?;
        let get = data_http_request(rest.clone(), "POST", "/ledger:get", "{\"seq\":1}").await?;
        expect_status_and_contains(
            &get,
            200,
            "\"payload_hex\":\"656e7472792d31\"",
            "Ledger REST get",
        )?;
        let head = data_http_request(rest.clone(), "POST", "/ledger:head", "{}").await?;
        expect_status_and_contains(&head, 200, "\"head\":", "Ledger REST head")?;
        let len = data_http_request(rest.clone(), "POST", "/ledger:len", "{}").await?;
        expect_status_and_contains(&len, 200, "\"len\":2", "Ledger REST len")?;
        let verify = data_http_request(rest, "POST", "/ledger:verify", "{}").await?;
        expect_status_and_contains(&verify, 200, "\"ok\":true", "Ledger REST verify")?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_ledger_jsonrpc_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-ledger-jsonrpc");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let jsonrpc = data_jsonrpc_router_with_profile(
            kernel,
            "ledger",
            "main",
            "audit",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let append = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ledger.append\",\"params\":{\"payload_hex\":\"656e7472792d30\"}}",
        )
        .await?;
        expect_status_and_contains(&append, 200, "\"seq\":0", "Ledger JSON-RPC append")?;
        let append = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ledger.append\",\"params\":{\"payload_hex\":\"656e7472792d31\"}}",
        )
        .await?;
        expect_status_and_contains(&append, 200, "\"seq\":1", "Ledger JSON-RPC append")?;
        let get = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ledger.get\",\"params\":{\"seq\":1}}",
        )
        .await?;
        expect_status_and_contains(
            &get,
            200,
            "\"payload_hex\":\"656e7472792d31\"",
            "Ledger JSON-RPC get",
        )?;
        let head = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"ledger.head\",\"params\":{}}",
        )
        .await?;
        expect_status_and_contains(&head, 200, "\"head\":", "Ledger JSON-RPC head")?;
        let len = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"ledger.len\",\"params\":{}}",
        )
        .await?;
        expect_status_and_contains(&len, 200, "\"len\":2", "Ledger JSON-RPC len")?;
        let verify = data_http_request(
            jsonrpc,
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"ledger.verify\",\"params\":{}}",
        )
        .await?;
        expect_status_and_contains(&verify, 200, "\"ok\":true", "Ledger JSON-RPC verify")?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_ledger_read_only_write_denial_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-ledger-read-only-write-denial");
        let workspace = seed_ledger_auth_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_policy(
            kernel.clone(),
            "ledger",
            "main",
            "audit",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let jsonrpc = data_jsonrpc_router_with_policy(
            kernel,
            "ledger",
            "main",
            "audit",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let user = nid(7);
        let grant = ledger_read_only_grant(user, workspace);
        update_ledger_acl(&path, &grant, true)?;

        let rest_denied = data_http_request_auth(
            rest,
            "POST",
            "/ledger:append",
            Some(user),
            Some("alice-pass"),
            "{\"payload_hex\":\"64656e696564\"}",
        )
        .await?;
        expect_status_and_code(&rest_denied, 403, "PERMISSION_DENIED")?;

        let json_denied = data_http_request_auth(
            jsonrpc,
            "POST",
            "/jsonrpc",
            Some(user),
            Some("alice-pass"),
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ledger.append\",\"params\":{\"payload_hex\":\"64656e696564\"}}",
        )
        .await?;
        if json_denied.0 != 200 || !json_denied.1.contains("PERMISSION_DENIED") {
            return Err(format!(
                "JSON-RPC read-only Ledger write returned {}: {}",
                json_denied.0, json_denied.1
            ));
        }

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_fts_rest_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-fts-rest");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_profile(
            kernel,
            "fts",
            "main",
            "docs",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let create = data_http_request(
            rest.clone(),
            "POST",
            "/fts:create",
            "{\"mapping\":{\"title\":\"text\"}}",
        )
        .await?;
        expect_status_and_contains(&create, 200, "\"ok\":true", "FTS REST create")?;
        let index = data_http_request(
            rest.clone(),
            "POST",
            "/fts:index",
            "{\"id_hex\":\"646f632d31\",\"document\":{\"title\":\"hello world\"}}",
        )
        .await?;
        expect_status_and_contains(&index, 200, "\"ok\":true", "FTS REST index")?;
        let get = data_http_request(
            rest.clone(),
            "POST",
            "/fts:get",
            "{\"id_hex\":\"646f632d31\"}",
        )
        .await?;
        expect_status_and_contains(&get, 200, "\"title\":\"hello world\"", "FTS REST get")?;
        let query = data_http_request(
            rest.clone(),
            "POST",
            "/fts:query",
            "{\"query\":{\"kind\":\"match\",\"field\":\"title\",\"text\":\"hello\"}}",
        )
        .await?;
        expect_status_and_contains(&query, 200, "\"id_hex\":\"646f632d31\"", "FTS REST query")?;
        let no_hit = data_http_request(
            rest.clone(),
            "POST",
            "/fts:query",
            "{\"query\":{\"kind\":\"match\",\"field\":\"title\",\"text\":\"absent\"}}",
        )
        .await?;
        expect_status_and_contains(&no_hit, 200, "\"hits\":[]", "FTS REST no-hit query")?;
        let ids = data_http_request(rest.clone(), "POST", "/fts:ids", "{}").await?;
        expect_status_and_contains(&ids, 200, "\"646f632d31\"", "FTS REST ids")?;
        let remap = data_http_request(
            rest.clone(),
            "POST",
            "/fts:remap",
            "{\"mapping\":{\"title\":\"text\",\"lang\":\"keyword\"}}",
        )
        .await?;
        expect_status_and_contains(&remap, 200, "\"ok\":true", "FTS REST remap")?;
        let delete =
            data_http_request(rest, "POST", "/fts:delete", "{\"id_hex\":\"646f632d31\"}").await?;
        expect_status_and_contains(&delete, 200, "\"deleted\":true", "FTS REST delete")?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_fts_jsonrpc_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-fts-jsonrpc");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let jsonrpc = data_jsonrpc_router_with_profile(
            kernel,
            "fts",
            "main",
            "docs",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let create = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"fts.create\",\"params\":{\"mapping\":{\"title\":\"text\"}}}",
        )
        .await?;
        expect_status_and_contains(&create, 200, "\"ok\":true", "FTS JSON-RPC create")?;
        let index = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"fts.index\",\"params\":{\"id_hex\":\"646f632d31\",\"document\":{\"title\":\"hello world\"}}}",
        )
        .await?;
        expect_status_and_contains(&index, 200, "\"ok\":true", "FTS JSON-RPC index")?;
        let get = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"fts.get\",\"params\":{\"id_hex\":\"646f632d31\"}}",
        )
        .await?;
        expect_status_and_contains(&get, 200, "\"title\":\"hello world\"", "FTS JSON-RPC get")?;
        let query = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"fts.query\",\"params\":{\"query\":{\"kind\":\"match\",\"field\":\"title\",\"text\":\"hello\"}}}",
        )
        .await?;
        expect_status_and_contains(
            &query,
            200,
            "\"id_hex\":\"646f632d31\"",
            "FTS JSON-RPC query",
        )?;
        let no_hit = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"fts.query\",\"params\":{\"query\":{\"kind\":\"match\",\"field\":\"title\",\"text\":\"absent\"}}}",
        )
        .await?;
        expect_status_and_contains(
            &no_hit,
            200,
            "\"hits\":[]",
            "FTS JSON-RPC no-hit query",
        )?;
        let ids = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"fts.ids\",\"params\":{}}",
        )
        .await?;
        expect_status_and_contains(&ids, 200, "\"646f632d31\"", "FTS JSON-RPC ids")?;
        let remap = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"fts.remap\",\"params\":{\"mapping\":{\"title\":\"text\",\"lang\":\"keyword\"}}}",
        )
        .await?;
        expect_status_and_contains(&remap, 200, "\"ok\":true", "FTS JSON-RPC remap")?;
        let delete = data_http_request(
            jsonrpc,
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"fts.delete\",\"params\":{\"id_hex\":\"646f632d31\"}}",
        )
        .await?;
        expect_status_and_contains(&delete, 200, "\"deleted\":true", "FTS JSON-RPC delete")?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_graph_read_only_write_denial_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-graph-read-only-write-denial");
        let workspace = seed_graph_auth_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_policy(
            kernel.clone(),
            "graph",
            "main",
            "relations",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let jsonrpc = data_jsonrpc_router_with_policy(
            kernel,
            "graph",
            "main",
            "relations",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let user = nid(7);
        let grant = graph_read_only_grant(user, workspace);
        update_graph_acl(&path, &grant, true)?;

        let rest_denied = data_http_request_auth(
            rest,
            "POST",
            "/graph:upsert-node",
            Some(user),
            Some("alice-pass"),
            "{\"id\":\"denied\"}",
        )
        .await?;
        expect_status_and_code(&rest_denied, 403, "PERMISSION_DENIED")?;

        let json_denied = data_http_request_auth(
            jsonrpc,
            "POST",
            "/jsonrpc",
            Some(user),
            Some("alice-pass"),
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"graph.upsert_node\",\"params\":{\"id\":\"denied\"}}",
        )
        .await?;
        if json_denied.0 != 200 || !json_denied.1.contains("PERMISSION_DENIED") {
            return Err(format!(
                "JSON-RPC read-only Graph write returned {}: {}",
                json_denied.0, json_denied.1
            ));
        }

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_graph_rest_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-graph-rest");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_profile(
            kernel,
            "graph",
            "main",
            "relations",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let upsert_a = data_http_request(
            rest.clone(),
            "POST",
            "/graph:upsert-node",
            "{\"id\":\"a\"}",
        )
        .await?;
        expect_status_and_contains(&upsert_a, 200, "\"ok\":true", "Graph REST upsert node")?;
        let upsert_b = data_http_request(
            rest.clone(),
            "POST",
            "/graph:upsert-node",
            "{\"id\":\"b\"}",
        )
        .await?;
        expect_status_and_contains(&upsert_b, 200, "\"ok\":true", "Graph REST upsert node")?;
        let edge = data_http_request(
            rest.clone(),
            "POST",
            "/graph:upsert-edge",
            "{\"id\":\"e1\",\"src\":\"a\",\"dst\":\"b\",\"label\":\"knows\"}",
        )
        .await?;
        expect_status_and_contains(&edge, 200, "\"ok\":true", "Graph REST upsert edge")?;
        let neighbors =
            data_http_request(rest.clone(), "POST", "/graph:neighbors", "{\"id\":\"a\"}").await?;
        expect_status_and_contains(&neighbors, 200, "\"nodes\":[\"b\"]", "Graph REST neighbors")?;
        let reachable = data_http_request(
            rest.clone(),
            "POST",
            "/graph:reachable",
            "{\"start\":\"a\",\"max_depth\":2}",
        )
        .await?;
        expect_status_and_contains(&reachable, 200, "\"b\"", "Graph REST reachable")?;
        let mutations = data_http_request(
            rest.clone(),
            "POST",
            "/graph:apply-mutations",
            "{\"mutations\":[{\"op\":\"create_node\",\"id\":\"c\",\"props\":{\"name\":\"Cara\"}},{\"op\":\"create_edge\",\"id\":\"e2\",\"src\":\"b\",\"dst\":\"c\",\"label\":\"knows\"}]}",
        )
        .await?;
        expect_status_and_contains(&mutations, 200, "\"applied\":2", "Graph REST mutations")?;
        let get = data_http_request(rest.clone(), "POST", "/graph:get-node", "{\"id\":\"c\"}").await?;
        expect_status_and_contains(&get, 200, "\"name\":\"Cara\"", "Graph REST get node")?;
        let query = data_http_request(
            rest.clone(),
            "POST",
            "/graph:query",
            "{\"query\":\"MATCH (p) RETURN p ORDER BY id(p)\"}",
        )
        .await?;
        expect_status_and_contains(&query, 200, "\"type\":\"node\"", "Graph REST query")?;
        expect_status_and_contains(&query, 200, "\"id\":\"c\"", "Graph REST query")?;
        let explain = data_http_request(
            rest.clone(),
            "POST",
            "/graph:explain-query",
            "{\"query\":\"MATCH (p) RETURN p\"}",
        )
        .await?;
        expect_status_and_contains(&explain, 200, "\"fallback_scan\"", "Graph REST explain")?;
        let capabilities = data_http_request(rest, "POST", "/capabilities", "{}").await?;
        expect_status_and_contains(
            &capabilities,
            200,
            "\"surface\":\"graph\"",
            "Graph REST capabilities",
        )?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_graph_jsonrpc_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-graph-jsonrpc");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let jsonrpc = data_jsonrpc_router_with_profile(
            kernel,
            "graph",
            "main",
            "relations",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let upsert_a = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"graph.upsert_node\",\"params\":{\"id\":\"a\"}}",
        )
        .await?;
        expect_status_and_contains(
            &upsert_a,
            200,
            "\"ok\":true",
            "Graph JSON-RPC upsert node",
        )?;
        let upsert_b = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"graph.upsert_node\",\"params\":{\"id\":\"b\"}}",
        )
        .await?;
        expect_status_and_contains(
            &upsert_b,
            200,
            "\"ok\":true",
            "Graph JSON-RPC upsert node",
        )?;
        let edge = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"graph.upsert_edge\",\"params\":{\"id\":\"e1\",\"src\":\"a\",\"dst\":\"b\",\"label\":\"knows\"}}",
        )
        .await?;
        expect_status_and_contains(&edge, 200, "\"ok\":true", "Graph JSON-RPC upsert edge")?;
        let neighbors = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"graph.neighbors\",\"params\":{\"id\":\"a\"}}",
        )
        .await?;
        expect_status_and_contains(
            &neighbors,
            200,
            "\"nodes\":[\"b\"]",
            "Graph JSON-RPC neighbors",
        )?;
        let mutations = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"graph.apply_mutations\",\"params\":{\"mutations\":[{\"op\":\"create_node\",\"id\":\"c\",\"props\":{\"name\":\"Cara\"}},{\"op\":\"create_edge\",\"id\":\"e2\",\"src\":\"b\",\"dst\":\"c\",\"label\":\"knows\"}]}}",
        )
        .await?;
        expect_status_and_contains(
            &mutations,
            200,
            "\"applied\":2",
            "Graph JSON-RPC mutations",
        )?;
        let get = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"graph.get_node\",\"params\":{\"id\":\"c\"}}",
        )
        .await?;
        expect_status_and_contains(&get, 200, "\"name\":\"Cara\"", "Graph JSON-RPC get node")?;
        let query = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"graph.query\",\"params\":{\"query\":\"MATCH (p) RETURN p ORDER BY id(p)\"}}",
        )
        .await?;
        expect_status_and_contains(&query, 200, "\"type\":\"node\"", "Graph JSON-RPC query")?;
        expect_status_and_contains(&query, 200, "\"id\":\"c\"", "Graph JSON-RPC query")?;
        let explain = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"graph.explain_query\",\"params\":{\"query\":\"MATCH (p) RETURN p\"}}",
        )
        .await?;
        expect_status_and_contains(&explain, 200, "\"fallback_scan\"", "Graph JSON-RPC explain")?;
        let capabilities = data_http_request(
            jsonrpc,
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"graph.capabilities\",\"params\":{}}",
        )
        .await?;
        expect_status_and_contains(
            &capabilities,
            200,
            "\"surface\":\"graph\"",
            "Graph JSON-RPC capabilities",
        )?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_vector_read_only_write_denial_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-vector-read-only-write-denial");
        let workspace = seed_vector_auth_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_policy(
            kernel.clone(),
            "vector",
            "main",
            "embeddings",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let jsonrpc = data_jsonrpc_router_with_policy(
            kernel,
            "vector",
            "main",
            "embeddings",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let user = nid(7);
        let grant = vector_read_only_grant(user, workspace);
        update_vector_acl(&path, &grant, true)?;

        let rest_denied = data_http_request_auth(
            rest,
            "POST",
            "/vector:create",
            Some(user),
            Some("alice-pass"),
            "{\"dim\":2,\"metric\":\"dot\"}",
        )
        .await?;
        expect_status_and_code(&rest_denied, 403, "PERMISSION_DENIED")?;

        let json_denied = data_http_request_auth(
            jsonrpc,
            "POST",
            "/jsonrpc",
            Some(user),
            Some("alice-pass"),
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"vector.create\",\"params\":{\"dim\":2,\"metric\":\"dot\"}}",
        )
        .await?;
        if json_denied.0 != 200 || !json_denied.1.contains("PERMISSION_DENIED") {
            return Err(format!(
                "JSON-RPC read-only Vector write returned {}: {}",
                json_denied.0, json_denied.1
            ));
        }

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_vector_rest_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-vector-rest");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_profile(
            kernel,
            "vector",
            "main",
            "embeddings",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let create = data_http_request(
            rest.clone(),
            "POST",
            "/vector:create",
            "{\"dim\":2,\"metric\":\"dot\"}",
        )
        .await?;
        expect_status_and_contains(&create, 200, "\"ok\":true", "Vector REST create")?;
        let upsert = data_http_request(
            rest.clone(),
            "POST",
            "/vector:upsert",
            "{\"id\":\"v1\",\"vector\":[1.0,0.0],\"metadata\":{\"label\":\"one\"}}",
        )
        .await?;
        expect_status_and_contains(&upsert, 200, "\"ok\":true", "Vector REST upsert")?;
        let get = data_http_request(rest.clone(), "POST", "/vector:get", "{\"id\":\"v1\"}").await?;
        expect_status_and_contains(&get, 200, "\"label\":\"one\"", "Vector REST get")?;
        expect_status_and_contains(&get, 200, "\"vector\":[1.0,0.0]", "Vector REST get")?;
        let search = data_http_request(
            rest,
            "POST",
            "/vector:search",
            "{\"query\":[1.0,0.0],\"k\":1}",
        )
        .await?;
        expect_status_and_contains(&search, 200, "\"id\":\"v1\"", "Vector REST search")?;
        expect_status_and_contains(&search, 200, "\"score\":1.0", "Vector REST search")?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_vector_jsonrpc_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-vector-jsonrpc");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let jsonrpc = data_jsonrpc_router_with_profile(
            kernel,
            "vector",
            "main",
            "embeddings",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let create = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"vector.create\",\"params\":{\"dim\":2,\"metric\":\"dot\"}}",
        )
        .await?;
        expect_status_and_contains(&create, 200, "\"ok\":true", "Vector JSON-RPC create")?;
        let upsert = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"vector.upsert\",\"params\":{\"id\":\"v1\",\"vector\":[1.0,0.0],\"metadata\":{\"label\":\"one\"}}}",
        )
        .await?;
        expect_status_and_contains(&upsert, 200, "\"ok\":true", "Vector JSON-RPC upsert")?;
        let get = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"vector.get\",\"params\":{\"id\":\"v1\"}}",
        )
        .await?;
        expect_status_and_contains(&get, 200, "\"label\":\"one\"", "Vector JSON-RPC get")?;
        expect_status_and_contains(&get, 200, "\"vector\":[1.0,0.0]", "Vector JSON-RPC get")?;
        let search = data_http_request(
            jsonrpc,
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"vector.search\",\"params\":{\"query\":[1.0,0.0],\"k\":1}}",
        )
        .await?;
        expect_status_and_contains(&search, 200, "\"id\":\"v1\"", "Vector JSON-RPC search")?;
        expect_status_and_contains(&search, 200, "\"score\":1.0", "Vector JSON-RPC search")?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_columnar_read_only_write_denial_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-columnar-read-only-write-denial");
        let workspace = seed_columnar_auth_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_policy(
            kernel.clone(),
            "columnar",
            "main",
            "events",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let jsonrpc = data_jsonrpc_router_with_policy(
            kernel,
            "columnar",
            "main",
            "events",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let user = nid(7);
        let grant = columnar_read_only_grant(user, workspace);
        update_columnar_acl(&path, &grant, true)?;

        let rest_denied = data_http_request_auth(
            rest,
            "PUT",
            "/columnar/events",
            Some(user),
            Some("alice-pass"),
            "{\"columns\":[{\"name\":\"id\",\"type\":\"int\"}],\"target_segment_rows\":2}",
        )
        .await?;
        expect_status_and_code(&rest_denied, 403, "PERMISSION_DENIED")?;

        let json_denied = data_http_request_auth(
            jsonrpc,
            "POST",
            "/jsonrpc",
            Some(user),
            Some("alice-pass"),
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"columnar.create\",\"params\":{\"dataset\":\"events\",\"columns\":[{\"name\":\"id\",\"type\":\"int\"}],\"target_segment_rows\":2}}",
        )
        .await?;
        if json_denied.0 != 200 || !json_denied.1.contains("PERMISSION_DENIED") {
            return Err(format!(
                "JSON-RPC read-only Columnar write returned {}: {}",
                json_denied.0, json_denied.1
            ));
        }

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_columnar_rest_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-columnar-rest");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_profile(
            kernel,
            "columnar",
            "main",
            "events",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let create = data_http_request(
            rest.clone(),
            "PUT",
            "/columnar/events",
            "{\"columns\":[{\"name\":\"id\",\"type\":\"int\"},{\"name\":\"value\",\"type\":\"text\"}],\"target_segment_rows\":2}",
        )
        .await?;
        expect_status_and_contains(&create, 200, "\"ok\":true", "Columnar REST create")?;
        let append = data_http_request(
            rest.clone(),
            "POST",
            "/columnar/events/rows",
            "{\"row\":[1,\"alpha\"]}",
        )
        .await?;
        expect_status_and_contains(&append, 200, "\"ok\":true", "Columnar REST append")?;
        let scan = data_http_request(rest.clone(), "GET", "/columnar/events/rows", "").await?;
        expect_status_and_contains(&scan, 200, "\"alpha\"", "Columnar REST scan")?;
        let columns = data_http_request(rest.clone(), "GET", "/columnar/events/columns", "").await?;
        expect_status_and_contains(&columns, 200, "\"name\":\"id\"", "Columnar REST columns")?;
        expect_status_and_contains(&columns, 200, "\"type\":\"int\"", "Columnar REST columns")?;
        let rows = data_http_request(rest.clone(), "GET", "/columnar/events/length", "").await?;
        expect_status_and_contains(&rows, 200, "\"rows\":1", "Columnar REST rows")?;
        let compact = data_http_request(rest.clone(), "POST", "/columnar/events:compact", "").await?;
        expect_status_and_contains(&compact, 200, "\"ok\":true", "Columnar REST compact")?;
        let inspect = data_http_request(rest.clone(), "GET", "/columnar/events", "").await?;
        expect_status_and_contains(&inspect, 200, "\"rows\":1", "Columnar REST inspect")?;
        let source_digest =
            data_http_request(rest.clone(), "GET", "/columnar/events/source-digest", "").await?;
        expect_status_and_contains(
            &source_digest,
            200,
            "\"digest\":\"",
            "Columnar REST source digest",
        )?;
        let select = data_http_request(
            rest.clone(),
            "POST",
            "/columnar/events:select",
            "{\"columns\":[\"value\"],\"filter\":{\"column\":\"id\",\"op\":\"eq\",\"value\":1}}",
        )
        .await?;
        expect_status_and_contains(&select, 200, "\"alpha\"", "Columnar REST select")?;
        let aggregate = data_http_request(
            rest,
            "POST",
            "/columnar/events:aggregate",
            "{\"aggregates\":[{\"op\":\"count\"}],\"filter\":null}",
        )
        .await?;
        expect_status_and_contains(&aggregate, 200, "\"values\":[1]", "Columnar REST aggregate")?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_columnar_result_handle_auth_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-columnar-result-handle-auth");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_profile(
            kernel,
            "columnar",
            "main",
            "events",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let create = data_http_request_auth_session(
            rest.clone(),
            "PUT",
            "/columnar/events",
            Some(nid(1)),
            Some("root-pass"),
            "session-a",
            "{\"columns\":[{\"name\":\"id\",\"type\":\"int\"},{\"name\":\"value\",\"type\":\"text\"}],\"target_segment_rows\":2}",
        )
        .await?;
        expect_status_and_contains(&create, 200, "\"ok\":true", "Columnar result create")?;
        let append = data_http_request_auth_session(
            rest.clone(),
            "POST",
            "/columnar/events/rows",
            Some(nid(1)),
            Some("root-pass"),
            "session-a",
            "{\"row\":[1,\"alpha\"]}",
        )
        .await?;
        expect_status_and_contains(&append, 200, "\"ok\":true", "Columnar result append")?;
        let prepared = data_http_request_auth_session(
            rest.clone(),
            "POST",
            "/columnar/events/arrow-ipc:prepare",
            Some(nid(1)),
            Some("root-pass"),
            "session-a",
            "",
        )
        .await?;
        expect_status_and_contains(
            &prepared,
            202,
            "\"format\":\"arrow-ipc\"",
            "Columnar result prepare",
        )?;
        let handle = json_string_field(&prepared.1, "handle")?;

        let wrong_session = data_http_request_auth_session(
            rest.clone(),
            "GET",
            &format!("/_loom/results/{handle}"),
            Some(nid(1)),
            Some("root-pass"),
            "session-b",
            "",
        )
        .await?;
        expect_status_and_code(&wrong_session, 404, "NOT_FOUND")?;

        let read = data_http_request_auth_session(
            rest.clone(),
            "GET",
            &format!("/_loom/results/{handle}"),
            Some(nid(1)),
            Some("root-pass"),
            "session-a",
            "",
        )
        .await?;
        if read.0 != 200 && read.0 != 501 {
            return Err(format!(
                "Columnar result read returned unexpected status {}: {}",
                read.0, read.1
            ));
        }
        if read.0 == 501 && !read.1.contains("UNSUPPORTED") {
            return Err(format!(
                "Columnar result read returned 501 without UNSUPPORTED: {}",
                read.1
            ));
        }

        let consumed = data_http_request_auth_session(
            rest,
            "GET",
            &format!("/_loom/results/{handle}"),
            Some(nid(1)),
            Some("root-pass"),
            "session-a",
            "",
        )
        .await?;
        expect_status_and_code(&consumed, 404, "NOT_FOUND")?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_vcs_protected_ref_write_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-vcs-protected-ref-write");
        let workspace = seed_vcs_protected_ref_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = vcs_rest_router_with_policy(
            kernel.clone(),
            workspace,
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let jsonrpc = vcs_jsonrpc_router_with_policy(
            kernel.clone(),
            workspace,
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );

        let rest_denied = data_http_request_auth(
            rest,
            "POST",
            "/commits",
            Some(nid(1)),
            Some("root-pass"),
            "{\"message\":\"blocked\",\"author\":\"root\"}",
        )
        .await?;
        expect_status_and_code(&rest_denied, 403, "PERMISSION_DENIED")?;
        if !rest_denied.1.contains("protected ref") {
            return Err(format!(
                "REST VCS commit denial did not identify protected ref policy: {}",
                rest_denied.1
            ));
        }

        let json_denied = data_http_request_auth(
            jsonrpc,
            "POST",
            "/jsonrpc",
            Some(nid(1)),
            Some("root-pass"),
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"vcs.commit\",\"params\":{\"message\":\"blocked\",\"author\":\"root\"}}",
        )
        .await?;
        if json_denied.0 != 200
            || !json_denied.1.contains("PERMISSION_DENIED")
            || !json_denied.1.contains("protected ref")
        {
            return Err(format!(
                "JSON-RPC VCS commit denial returned {}: {}",
                json_denied.0, json_denied.1
            ));
        }

        let service = HostedVcsGrpcService::new(kernel, workspace);
        let grpc_denied = HostedVcs::commit(
            &service,
            grpc_auth_request(
                VcsCommitRequest {
                    message: "blocked".to_string(),
                    author: "root".to_string(),
                    staged: false,
                },
                nid(1),
                "root-pass",
            ),
        )
        .await
        .expect_err("protected gRPC VCS commit must be denied");
        if grpc_denied.code() != tonic::Code::PermissionDenied
            || !grpc_denied.message().contains("protected ref")
        {
            return Err(format!(
                "gRPC VCS commit denial returned {}: {}",
                grpc_denied.code(),
                grpc_denied.message()
            ));
        }

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_columnar_jsonrpc_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-columnar-jsonrpc");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let jsonrpc = data_jsonrpc_router_with_profile(
            kernel,
            "columnar",
            "main",
            "events",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );

        let create = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"columnar.create\",\"params\":{\"dataset\":\"events\",\"columns\":[{\"name\":\"id\",\"type\":\"int\"},{\"name\":\"value\",\"type\":\"text\"}],\"target_segment_rows\":2}}",
        )
        .await?;
        expect_status_and_contains(&create, 200, "\"ok\":true", "Columnar JSON-RPC create")?;
        let append = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"columnar.append\",\"params\":{\"dataset\":\"events\",\"row\":[1,\"alpha\"]}}",
        )
        .await?;
        expect_status_and_contains(&append, 200, "\"ok\":true", "Columnar JSON-RPC append")?;
        let scan = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"columnar.scan\",\"params\":{\"dataset\":\"events\"}}",
        )
        .await?;
        expect_status_and_contains(&scan, 200, "\"alpha\"", "Columnar JSON-RPC scan")?;
        let columns = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"columnar.columns\",\"params\":{\"dataset\":\"events\"}}",
        )
        .await?;
        expect_status_and_contains(
            &columns,
            200,
            "\"name\":\"id\"",
            "Columnar JSON-RPC columns",
        )?;
        expect_status_and_contains(
            &columns,
            200,
            "\"type\":\"int\"",
            "Columnar JSON-RPC columns",
        )?;
        let rows = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"columnar.rows\",\"params\":{\"dataset\":\"events\"}}",
        )
        .await?;
        expect_status_and_contains(&rows, 200, "\"rows\":1", "Columnar JSON-RPC rows")?;
        let compact = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"columnar.compact\",\"params\":{\"dataset\":\"events\"}}",
        )
        .await?;
        expect_status_and_contains(&compact, 200, "\"ok\":true", "Columnar JSON-RPC compact")?;
        let inspect = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"columnar.inspect\",\"params\":{\"dataset\":\"events\"}}",
        )
        .await?;
        expect_status_and_contains(&inspect, 200, "\"rows\":1", "Columnar JSON-RPC inspect")?;
        let source_digest = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"columnar.source_digest\",\"params\":{\"dataset\":\"events\"}}",
        )
        .await?;
        expect_status_and_contains(
            &source_digest,
            200,
            "\"digest\":\"",
            "Columnar JSON-RPC source digest",
        )?;
        let select = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"columnar.select\",\"params\":{\"dataset\":\"events\",\"columns\":[\"value\"],\"filter\":{\"column\":\"id\",\"op\":\"eq\",\"value\":1}}}",
        )
        .await?;
        expect_status_and_contains(&select, 200, "\"alpha\"", "Columnar JSON-RPC select")?;
        let aggregate = data_http_request(
            jsonrpc,
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":10,\"method\":\"columnar.aggregate\",\"params\":{\"dataset\":\"events\",\"aggregates\":[{\"op\":\"count\"}],\"filter\":null}}",
        )
        .await?;
        expect_status_and_contains(
            &aggregate,
            200,
            "\"values\":[1]",
            "Columnar JSON-RPC aggregate",
        )?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_kv_rest_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-kv-rest");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_profile(
            kernel,
            "kv",
            "main",
            "cache",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );
        let key_a = hex_bytes(&key_to_cbor(&Value::Text("a".to_string())));
        let key_b = hex_bytes(&key_to_cbor(&Value::Text("b".to_string())));

        let put = data_http_request(
            rest.clone(),
            "POST",
            "/kv:put",
            &format!("{{\"key_hex\":\"{key_a}\",\"value_hex\":\"6f6e65\"}}"),
        )
        .await?;
        expect_status_and_contains(&put, 200, "\"ok\":true", "KV REST put")?;
        let put = data_http_request(
            rest.clone(),
            "POST",
            "/kv:put",
            &format!("{{\"key_hex\":\"{key_b}\",\"value_hex\":\"74776f\"}}"),
        )
        .await?;
        expect_status_and_contains(&put, 200, "\"ok\":true", "KV REST put")?;
        let get = data_http_request(
            rest.clone(),
            "POST",
            "/kv:get",
            &format!("{{\"key_hex\":\"{key_a}\"}}"),
        )
        .await?;
        expect_status_and_contains(&get, 200, "\"value_hex\":\"6f6e65\"", "KV REST get")?;
        let list = data_http_request(rest.clone(), "POST", "/kv:list", "{}").await?;
        expect_status_and_contains(
            &list,
            200,
            &format!("\"key_hex\":\"{key_a}\""),
            "KV REST list",
        )?;
        expect_status_and_contains(
            &list,
            200,
            &format!("\"key_hex\":\"{key_b}\""),
            "KV REST list",
        )?;
        let range = data_http_request(
            rest.clone(),
            "POST",
            "/kv:range",
            &format!("{{\"lo_key_hex\":\"{key_a}\",\"hi_key_hex\":\"{key_b}\"}}"),
        )
        .await?;
        expect_status_and_contains(
            &range,
            200,
            &format!("\"key_hex\":\"{key_a}\""),
            "KV REST range",
        )?;
        expect_status_and_contains(&range, 200, "\"value_hex\":\"6f6e65\"", "KV REST range")?;
        let delete = data_http_request(
            rest.clone(),
            "POST",
            "/kv:delete",
            &format!("{{\"key_hex\":\"{key_a}\"}}"),
        )
        .await?;
        expect_status_and_contains(&delete, 200, "\"deleted\":true", "KV REST delete")?;
        let missing = data_http_request(
            rest,
            "POST",
            "/kv:get",
            &format!("{{\"key_hex\":\"{key_a}\"}}"),
        )
        .await?;
        expect_status_and_contains(&missing, 200, "\"value_hex\":null", "KV REST get missing")?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_kv_jsonrpc_round_trip_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-kv-jsonrpc");
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let jsonrpc = data_jsonrpc_router_with_profile(
            kernel,
            "kv",
            "main",
            "cache",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );
        let key_a = hex_bytes(&key_to_cbor(&Value::Text("a".to_string())));
        let key_b = hex_bytes(&key_to_cbor(&Value::Text("b".to_string())));

        let put = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"kv.put\",\"params\":{{\"key_hex\":\"{key_a}\",\"value_hex\":\"6f6e65\"}}}}"
            ),
        )
        .await?;
        expect_status_and_contains(&put, 200, "\"ok\":true", "KV JSON-RPC put")?;
        let put = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"kv.put\",\"params\":{{\"key_hex\":\"{key_b}\",\"value_hex\":\"74776f\"}}}}"
            ),
        )
        .await?;
        expect_status_and_contains(&put, 200, "\"ok\":true", "KV JSON-RPC put")?;
        let get = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"kv.get\",\"params\":{{\"key_hex\":\"{key_a}\"}}}}"
            ),
        )
        .await?;
        expect_status_and_contains(&get, 200, "\"value_hex\":\"6f6e65\"", "KV JSON-RPC get")?;
        let list = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"kv.list\",\"params\":{}}",
        )
        .await?;
        expect_status_and_contains(
            &list,
            200,
            &format!("\"key_hex\":\"{key_a}\""),
            "KV JSON-RPC list",
        )?;
        expect_status_and_contains(
            &list,
            200,
            &format!("\"key_hex\":\"{key_b}\""),
            "KV JSON-RPC list",
        )?;
        let range = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"kv.range\",\"params\":{{\"lo_key_hex\":\"{key_a}\",\"hi_key_hex\":\"{key_b}\"}}}}"
            ),
        )
        .await?;
        expect_status_and_contains(
            &range,
            200,
            &format!("\"key_hex\":\"{key_a}\""),
            "KV JSON-RPC range",
        )?;
        expect_status_and_contains(
            &range,
            200,
            "\"value_hex\":\"6f6e65\"",
            "KV JSON-RPC range",
        )?;
        let delete = data_http_request(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"kv.delete\",\"params\":{{\"key_hex\":\"{key_a}\"}}}}"
            ),
        )
        .await?;
        expect_status_and_contains(&delete, 200, "\"deleted\":true", "KV JSON-RPC delete")?;
        let missing = data_http_request(
            jsonrpc,
            "POST",
            "/jsonrpc",
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"kv.get\",\"params\":{{\"key_hex\":\"{key_a}\"}}}}"
            ),
        )
        .await?;
        expect_status_and_contains(
            &missing,
            200,
            "\"value_hex\":null",
            "KV JSON-RPC get missing",
        )?;

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_kv_read_only_write_denial_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-kv-read-only-write-denial");
        let workspace = seed_kv_auth_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_policy(
            kernel.clone(),
            "kv",
            "main",
            "cache",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let jsonrpc = data_jsonrpc_router_with_policy(
            kernel,
            "kv",
            "main",
            "cache",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let user = nid(7);
        let grant = kv_read_only_grant(user, workspace);
        update_kv_acl(&path, &grant, true)?;
        let key = hex_bytes(&key_to_cbor(&Value::Text("denied".to_string())));

        let rest_denied = data_http_request_auth(
            rest,
            "POST",
            "/kv:put",
            Some(user),
            Some("alice-pass"),
            &format!("{{\"key_hex\":\"{key}\",\"value_hex\":\"726573742d64656e696564\"}}"),
        )
        .await?;
        expect_status_and_code(&rest_denied, 403, "PERMISSION_DENIED")?;

        let json_denied = data_http_request_auth(
            jsonrpc,
            "POST",
            "/jsonrpc",
            Some(user),
            Some("alice-pass"),
            &format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"kv.put\",\"params\":{{\"key_hex\":\"{key}\",\"value_hex\":\"6a736f6e2d64656e696564\"}}}}"
            ),
        )
        .await?;
        if json_denied.0 != 200 || !json_denied.1.contains("PERMISSION_DENIED") {
            return Err(format!(
                "JSON-RPC read-only KV write returned {}: {}",
                json_denied.0, json_denied.1
            ));
        }

        fs::remove_file(path).map_err(strerr)
    })
}

fn hosted_document_read_only_write_denial_matrix() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-document-read-only-write-denial");
        let workspace = seed_document_auth_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_policy(
            kernel.clone(),
            "document",
            "main",
            "docs",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let jsonrpc = data_jsonrpc_router_with_policy(
            kernel,
            "document",
            "main",
            "docs",
            16 * 1024 * 1024,
            HostedAuthPolicy::Passphrase,
        );
        let user = nid(7);
        let grant = document_read_only_grant(user, workspace);
        update_document_acl(&path, &grant, true)?;

        let rest_denied = data_http_request_auth(
            rest,
            "POST",
            "/documents:put-text",
            Some(user),
            Some("alice-pass"),
            "{\"id\":\"doc-denied\",\"text\":\"rest denied\"}",
        )
        .await?;
        expect_status_and_code(&rest_denied, 403, "PERMISSION_DENIED")?;

        let json_denied = data_http_request_auth(
            jsonrpc,
            "POST",
            "/jsonrpc",
            Some(user),
            Some("alice-pass"),
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"document.put_text\",\"params\":{\"id\":\"doc-denied\",\"text\":\"json denied\"}}",
        )
        .await?;
        if json_denied.0 != 200 || !json_denied.1.contains("PERMISSION_DENIED") {
            return Err(format!(
                "JSON-RPC read-only Document write returned {}: {}",
                json_denied.0, json_denied.1
            ));
        }

        fs::remove_file(path).map_err(strerr)
    })
}

async fn cas_http_request(
    router: axum::Router,
    method: &str,
    uri: &str,
    principal: Option<WorkspaceId>,
    passphrase: Option<&str>,
    body: &str,
) -> Result<(u16, String), String> {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(principal) = principal {
        request = request.header("x-loom-principal", principal.to_string());
    }
    if let Some(passphrase) = passphrase {
        request = request.header("x-loom-passphrase", passphrase);
    }
    let response = router
        .oneshot(request.body(Body::from(body.to_string())).map_err(strerr)?)
        .await
        .map_err(strerr)?;
    let status = response.status().as_u16();
    let body = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .map_err(strerr)?;
    Ok((status, String::from_utf8(body.to_vec()).map_err(strerr)?))
}

async fn data_http_request(
    router: axum::Router,
    method: &str,
    uri: &str,
    body: &str,
) -> Result<(u16, String), String> {
    let response = router
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .map_err(strerr)?,
        )
        .await
        .map_err(strerr)?;
    let status = response.status().as_u16();
    let body = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .map_err(strerr)?;
    Ok((status, String::from_utf8(body.to_vec()).map_err(strerr)?))
}

async fn data_http_request_auth(
    router: axum::Router,
    method: &str,
    uri: &str,
    principal: Option<WorkspaceId>,
    passphrase: Option<&str>,
    body: &str,
) -> Result<(u16, String), String> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(CONTENT_TYPE, "application/json");
    if let Some(principal) = principal {
        request = request.header("x-loom-principal", principal.to_string());
    }
    if let Some(passphrase) = passphrase {
        request = request.header("x-loom-passphrase", passphrase);
    }
    let response = router
        .oneshot(request.body(Body::from(body.to_string())).map_err(strerr)?)
        .await
        .map_err(strerr)?;
    let status = response.status().as_u16();
    let body = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .map_err(strerr)?;
    Ok((status, String::from_utf8(body.to_vec()).map_err(strerr)?))
}

async fn data_http_request_auth_session(
    router: axum::Router,
    method: &str,
    uri: &str,
    principal: Option<WorkspaceId>,
    passphrase: Option<&str>,
    session: &str,
    body: &str,
) -> Result<(u16, String), String> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(CONTENT_TYPE, "application/json")
        .header("x-loom-session", session);
    if let Some(principal) = principal {
        request = request.header("x-loom-principal", principal.to_string());
    }
    if let Some(passphrase) = passphrase {
        request = request.header("x-loom-passphrase", passphrase);
    }
    let response = router
        .oneshot(request.body(Body::from(body.to_string())).map_err(strerr)?)
        .await
        .map_err(strerr)?;
    let status = response.status().as_u16();
    let body = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .map_err(strerr)?;
    Ok((status, String::from_utf8(body.to_vec()).map_err(strerr)?))
}

fn expect_status_and_code(response: &(u16, String), status: u16, code: &str) -> Result<(), String> {
    if response.0 != status || !response.1.contains(code) {
        return Err(format!(
            "expected HTTP {status} with {code}, received {}: {}",
            response.0, response.1
        ));
    }
    Ok(())
}

fn expect_status_and_contains(
    response: &(u16, String),
    status: u16,
    needle: &str,
    label: &str,
) -> Result<(), String> {
    if response.0 != status || !response.1.contains(needle) {
        return Err(format!(
            "{label} expected HTTP {status} with {needle}, received {}: {}",
            response.0, response.1
        ));
    }
    Ok(())
}

fn cas_grpc_request<T>(message: T, principal: WorkspaceId, passphrase: &str) -> GrpcRequest<T> {
    grpc_auth_request(message, principal, passphrase)
}

fn grpc_auth_request<T>(message: T, principal: WorkspaceId, passphrase: &str) -> GrpcRequest<T> {
    let mut request = GrpcRequest::new(message);
    request.metadata_mut().insert(
        "x-loom-principal",
        principal
            .to_string()
            .parse()
            .expect("valid principal metadata"),
    );
    request.metadata_mut().insert(
        "x-loom-passphrase",
        passphrase.parse().expect("valid passphrase metadata"),
    );
    request
}

fn cas_read_write_grant(principal: WorkspaceId, workspace: WorkspaceId) -> AclGrant {
    AclGrant {
        subject: AclSubject::Principal(principal),
        workspace: Some(workspace),
        domain: Some(FacetKind::Cas.into()),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Read, AclRight::Write].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    }
}

fn timeseries_read_write_grant(principal: WorkspaceId, workspace: WorkspaceId) -> AclGrant {
    AclGrant {
        subject: AclSubject::Principal(principal),
        workspace: Some(workspace),
        domain: Some(FacetKind::TimeSeries.into()),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Read, AclRight::Write].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    }
}

fn timeseries_read_only_grant(principal: WorkspaceId, workspace: WorkspaceId) -> AclGrant {
    AclGrant {
        subject: AclSubject::Principal(principal),
        workspace: Some(workspace),
        domain: Some(FacetKind::TimeSeries.into()),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Read].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    }
}

fn queue_read_only_grant(principal: WorkspaceId, workspace: WorkspaceId) -> AclGrant {
    AclGrant {
        subject: AclSubject::Principal(principal),
        workspace: Some(workspace),
        domain: Some(FacetKind::Queue.into()),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Read].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    }
}

fn graph_read_only_grant(principal: WorkspaceId, workspace: WorkspaceId) -> AclGrant {
    AclGrant {
        subject: AclSubject::Principal(principal),
        workspace: Some(workspace),
        domain: Some(FacetKind::Graph.into()),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Read].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    }
}

fn vector_read_only_grant(principal: WorkspaceId, workspace: WorkspaceId) -> AclGrant {
    AclGrant {
        subject: AclSubject::Principal(principal),
        workspace: Some(workspace),
        domain: Some(FacetKind::Vector.into()),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Read].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    }
}

fn columnar_read_only_grant(principal: WorkspaceId, workspace: WorkspaceId) -> AclGrant {
    AclGrant {
        subject: AclSubject::Principal(principal),
        workspace: Some(workspace),
        domain: Some(FacetKind::Columnar.into()),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Read].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    }
}

fn ledger_read_only_grant(principal: WorkspaceId, workspace: WorkspaceId) -> AclGrant {
    AclGrant {
        subject: AclSubject::Principal(principal),
        workspace: Some(workspace),
        domain: Some(FacetKind::Ledger.into()),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Read].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    }
}

fn kv_read_only_grant(principal: WorkspaceId, workspace: WorkspaceId) -> AclGrant {
    AclGrant {
        subject: AclSubject::Principal(principal),
        workspace: Some(workspace),
        domain: Some(FacetKind::Kv.into()),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Read].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    }
}

fn document_read_only_grant(principal: WorkspaceId, workspace: WorkspaceId) -> AclGrant {
    AclGrant {
        subject: AclSubject::Principal(principal),
        workspace: Some(workspace),
        domain: Some(FacetKind::Document.into()),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Read].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    }
}

fn update_cas_acl(path: &PathBuf, grant: &AclGrant, add: bool) -> Result<(), String> {
    let store = FileStore::open(path).map_err(strerr)?;
    let mut acl = store
        .acl_store()
        .map_err(strerr)?
        .ok_or_else(|| "CAS auth fixture is missing ACL state".to_string())?;
    if add {
        acl.grant(grant.clone()).map_err(strerr)?;
    } else if !acl.revoke(grant) {
        return Err("CAS auth fixture could not revoke its ACL grant".to_string());
    }
    store.save_acl_store(&acl).map_err(strerr)
}

fn update_timeseries_acl(path: &PathBuf, grant: &AclGrant, add: bool) -> Result<(), String> {
    let store = FileStore::open(path).map_err(strerr)?;
    let mut acl = store
        .acl_store()
        .map_err(strerr)?
        .ok_or_else(|| "TimeSeries auth fixture is missing ACL state".to_string())?;
    if add {
        acl.grant(grant.clone()).map_err(strerr)?;
    } else if !acl.revoke(grant) {
        return Err("TimeSeries auth fixture could not revoke its ACL grant".to_string());
    }
    store.save_acl_store(&acl).map_err(strerr)
}

fn update_queue_acl(path: &PathBuf, grant: &AclGrant, add: bool) -> Result<(), String> {
    let store = FileStore::open(path).map_err(strerr)?;
    let mut acl = store
        .acl_store()
        .map_err(strerr)?
        .ok_or_else(|| "Queue auth fixture is missing ACL state".to_string())?;
    if add {
        acl.grant(grant.clone()).map_err(strerr)?;
    } else if !acl.revoke(grant) {
        return Err("Queue auth fixture could not revoke its ACL grant".to_string());
    }
    store.save_acl_store(&acl).map_err(strerr)
}

fn update_graph_acl(path: &PathBuf, grant: &AclGrant, add: bool) -> Result<(), String> {
    let store = FileStore::open(path).map_err(strerr)?;
    let mut acl = store
        .acl_store()
        .map_err(strerr)?
        .ok_or_else(|| "Graph auth fixture is missing ACL state".to_string())?;
    if add {
        acl.grant(grant.clone()).map_err(strerr)?;
    } else if !acl.revoke(grant) {
        return Err("Graph auth fixture could not revoke its ACL grant".to_string());
    }
    store.save_acl_store(&acl).map_err(strerr)
}

fn update_vector_acl(path: &PathBuf, grant: &AclGrant, add: bool) -> Result<(), String> {
    let store = FileStore::open(path).map_err(strerr)?;
    let mut acl = store
        .acl_store()
        .map_err(strerr)?
        .ok_or_else(|| "Vector auth fixture is missing ACL state".to_string())?;
    if add {
        acl.grant(grant.clone()).map_err(strerr)?;
    } else if !acl.revoke(grant) {
        return Err("Vector auth fixture could not revoke its ACL grant".to_string());
    }
    store.save_acl_store(&acl).map_err(strerr)
}

fn update_columnar_acl(path: &PathBuf, grant: &AclGrant, add: bool) -> Result<(), String> {
    let store = FileStore::open(path).map_err(strerr)?;
    let mut acl = store
        .acl_store()
        .map_err(strerr)?
        .ok_or_else(|| "Columnar auth fixture is missing ACL state".to_string())?;
    if add {
        acl.grant(grant.clone()).map_err(strerr)?;
    } else if !acl.revoke(grant) {
        return Err("Columnar auth fixture could not revoke its ACL grant".to_string());
    }
    store.save_acl_store(&acl).map_err(strerr)
}

fn update_ledger_acl(path: &PathBuf, grant: &AclGrant, add: bool) -> Result<(), String> {
    let store = FileStore::open(path).map_err(strerr)?;
    let mut acl = store
        .acl_store()
        .map_err(strerr)?
        .ok_or_else(|| "Ledger auth fixture is missing ACL state".to_string())?;
    if add {
        acl.grant(grant.clone()).map_err(strerr)?;
    } else if !acl.revoke(grant) {
        return Err("Ledger auth fixture could not revoke its ACL grant".to_string());
    }
    store.save_acl_store(&acl).map_err(strerr)
}

fn update_kv_acl(path: &PathBuf, grant: &AclGrant, add: bool) -> Result<(), String> {
    let store = FileStore::open(path).map_err(strerr)?;
    let mut acl = store
        .acl_store()
        .map_err(strerr)?
        .ok_or_else(|| "KV auth fixture is missing ACL state".to_string())?;
    if add {
        acl.grant(grant.clone()).map_err(strerr)?;
    } else if !acl.revoke(grant) {
        return Err("KV auth fixture could not revoke its ACL grant".to_string());
    }
    store.save_acl_store(&acl).map_err(strerr)
}

fn update_document_acl(path: &PathBuf, grant: &AclGrant, add: bool) -> Result<(), String> {
    let store = FileStore::open(path).map_err(strerr)?;
    let mut acl = store
        .acl_store()
        .map_err(strerr)?
        .ok_or_else(|| "Document auth fixture is missing ACL state".to_string())?;
    if add {
        acl.grant(grant.clone()).map_err(strerr)?;
    } else if !acl.revoke(grant) {
        return Err("Document auth fixture could not revoke its ACL grant".to_string());
    }
    store.save_acl_store(&acl).map_err(strerr)
}

fn hosted_network_access_matrix() -> Result<(), String> {
    let allow_loopback = network_access_policy(
        "loopback-only",
        NetworkAccessAction::Deny,
        vec![network_access_rule(
            "allow-loopback",
            NetworkAccessAction::Allow,
            Some("127.0.0.0/8"),
            None,
            false,
        )?],
    )?;
    let allow_loopback = loom_hosted::HostedNetworkAccessPolicy::from_record(allow_loopback);
    let loopback = "127.0.0.1:443".parse().map_err(strerr)?;
    let internet = "198.51.100.9:443".parse().map_err(strerr)?;

    for transport in ["REST", "JSON-RPC", "gRPC direct-peer"] {
        if !loom_hosted::network_access_allows(Some(&allow_loopback), loopback, None, None, None) {
            return Err(format!("{transport} loopback admission was denied"));
        }
        if loom_hosted::network_access_allows(Some(&allow_loopback), internet, None, None, None) {
            return Err(format!("{transport} internet admission was allowed"));
        }
    }

    let trusted_proxy = network_access_policy(
        "trusted-proxy",
        NetworkAccessAction::Deny,
        vec![network_access_rule(
            "allow-forwarded-loopback",
            NetworkAccessAction::Allow,
            Some("127.0.0.0/8"),
            Some("10.0.0.0/8"),
            false,
        )?],
    )?;
    let trusted_proxy = loom_hosted::HostedNetworkAccessPolicy::from_record(trusted_proxy);
    let proxy = "10.1.2.3:443".parse().map_err(strerr)?;
    if !loom_hosted::network_access_allows(
        Some(&trusted_proxy),
        proxy,
        None,
        Some("127.0.0.1, 10.1.2.3"),
        None,
    ) {
        return Err("REST and JSON-RPC trusted-proxy admission was denied".to_string());
    }
    if !loom_hosted::grpc_network_access_allows_request(
        Some(&trusted_proxy),
        Some(proxy),
        None,
        Some("127.0.0.1, 10.1.2.3"),
        None,
    ) {
        return Err("gRPC trusted-proxy admission was denied".to_string());
    }
    if loom_hosted::network_access_allows(
        Some(&trusted_proxy),
        internet,
        None,
        Some("127.0.0.1, 198.51.100.9"),
        None,
    ) {
        return Err("untrusted forwarded address was accepted".to_string());
    }
    if loom_hosted::network_access_allows(
        Some(&trusted_proxy),
        proxy,
        None,
        Some("not-an-address"),
        None,
    ) {
        return Err("malformed trusted-proxy header was accepted".to_string());
    }

    let mtls = network_access_policy(
        "mtls-required",
        NetworkAccessAction::Deny,
        vec![network_access_rule(
            "allow-mtls",
            NetworkAccessAction::Allow,
            None,
            None,
            true,
        )?],
    )?;
    let mtls = loom_hosted::HostedNetworkAccessPolicy::from_record(mtls);
    if loom_hosted::network_access_allows(Some(&mtls), loopback, None, None, None) {
        return Err("missing mTLS peer certificate was accepted".to_string());
    }
    if loom_hosted::grpc_network_access_allows_request(
        Some(&mtls),
        Some(loopback),
        None,
        None,
        None,
    ) {
        return Err("gRPC missing mTLS peer certificate was accepted".to_string());
    }

    let denied_events = Arc::new(Mutex::new(Vec::new()));
    let audit_events = denied_events.clone();
    let denied_audit: loom_hosted::HostedNetworkAccessAuditSink = Arc::new(move |event| {
        if let Ok(mut events) = audit_events.lock() {
            events.push(event);
        }
    });
    let denied_policy = loom_hosted::HostedNetworkAccessPolicy::from_record_for_listener(
        Some("protocol-conformance".to_string()),
        network_access_policy("deny-audit", NetworkAccessAction::Deny, Vec::new())?,
    );
    if loom_hosted::network_access_allows_with_denied_audit(
        Some(&denied_policy),
        internet,
        None,
        None,
        None,
        Some(&denied_audit),
    ) {
        return Err("deny-audit policy allowed an internet connection".to_string());
    }
    let events = denied_events
        .lock()
        .map_err(|_| "network access denied-audit lock was poisoned".to_string())?;
    if events.len() != 1
        || events[0].listener_id != "protocol-conformance"
        || events[0].policy_name != "deny-audit"
    {
        return Err(format!("unexpected denied-audit events: {events:?}"));
    }

    Ok(())
}

fn network_access_policy(
    name: &str,
    default_action: NetworkAccessAction,
    rules: Vec<NetworkAccessRule>,
) -> Result<NetworkAccessPolicyRecord, String> {
    FileStore::network_access_policy_record(name, None, default_action, rules).map_err(strerr)
}

fn network_access_rule(
    id: &str,
    action: NetworkAccessAction,
    source_cidr: Option<&str>,
    trusted_proxy_cidr: Option<&str>,
    require_mtls: bool,
) -> Result<NetworkAccessRule, String> {
    Ok(NetworkAccessRule {
        id: id.to_string(),
        action,
        source_cidr: source_cidr
            .map(NetworkAccessCidr::parse)
            .transpose()
            .map_err(strerr)?,
        trusted_proxy_cidr: trusted_proxy_cidr
            .map(NetworkAccessCidr::parse)
            .transpose()
            .map_err(strerr)?,
        require_mtls,
        client_cert_subject: None,
        client_cert_san: None,
        client_cert_issuer: None,
        description: None,
    })
}

fn hosted_reference_reconciliation_adapters_preserve_auth() -> Result<(), String> {
    let path = temp_path("hosted-reference-reconciliation");
    let workspace = seed_meetings_store(&path)?;
    let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
    let auth = HostedAuth::passphrase(nid(1), "root-pass", "hosted-reference-status");
    let rest = kernel.rest();
    let rest_status = rest
        .reference_reconciliation_status(&auth, workspace)
        .map_err(|error| error.error.message)?;
    if rest_status.status != 200 || rest_status.body.pending != 0 {
        return Err("REST reference reconciliation status is invalid".to_string());
    }
    let jsonrpc = kernel.jsonrpc();
    let jsonrpc_status = jsonrpc
        .reference_reconciliation_status(&auth, workspace)
        .map_err(|error| error.message)?;
    if jsonrpc_status.result.active_targets != 0 || jsonrpc_status.result.failed != 0 {
        return Err("JSON-RPC reference reconciliation status is invalid".to_string());
    }
    fs::remove_file(path).map_err(strerr)
}

fn lane_behavioral_conformance_across_local_mcp_and_hosted() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("lanes-parity");
        let workspace = seed_lane_store(&path)?;
        let mut loom = attach_local_auth(
            open_loom_unlocked(&path, None).map_err(strerr)?,
            &root_local_auth("lanes-local"),
        )
        .map_err(strerr)?;
        let local_lane = Lane::new(LaneInput {
            lane_id: "local",
            lane_key: "local",
            title: "Local lane",
            description: "Durable local-lane intention for conformance.",
            lane_kind: loom_lanes::LaneKind::Assignment,
            owner_principal: Some("agent:3"),
            lane_status: LaneStatus::Ready,
            lane_tickets: &[
                LaneTicket {
                    ticket_id: "MX-102".to_string(),
                    order_key: "F".to_string(),
                },
                LaneTicket {
                    ticket_id: "MX-103".to_string(),
                    order_key: "V".to_string(),
                },
            ],
            active_ticket_id: Some("MX-102"),
            status_report: "ready",
            reviewer_feedback: "",
            updated_at: 1,
            updated_by: "agent:3",
        })
        .map_err(strerr)?;
        loom_lanes::create_lane(&mut loom, workspace, local_lane).map_err(strerr)?;
        let invalid_active = Lane::new(LaneInput {
            lane_id: "invalid-active",
            lane_key: "invalid-active",
            title: "",
            description: "",
            lane_kind: loom_lanes::LaneKind::Assignment,
            owner_principal: Some("agent:3"),
            lane_status: LaneStatus::Ready,
            lane_tickets: &[LaneTicket {
                ticket_id: "MX-102".to_string(),
                order_key: "F".to_string(),
            }],
            active_ticket_id: Some("MX-999"),
            status_report: "",
            reviewer_feedback: "",
            updated_at: 1,
            updated_by: "agent:3",
        })
        .unwrap_err();
        if invalid_active.code != Code::InvalidArgument {
            return Err("local Lane model accepted invalid active_ticket_id".to_string());
        }
        let local = loom_lanes::get_lane(&loom, workspace, "local")
            .map_err(strerr)?
            .ok_or_else(|| "local lane missing after create".to_string())?;
        assert_lane_baseline("local", &local)?;
        save_loom(&mut loom).map_err(strerr)?;
        drop(loom);

        let mcp = LoomMcp::new(StoreAccess::per_request_auth(
            &path,
            root_local_auth("lanes-mcp"),
        ));
        let mcp_lane = mcp
            .write_lanes_create(
                "main",
                LaneCreateRequest {
                    lane_id: "mcp",
                    lane_key: "mcp",
                    title: "MCP conformance lane",
                    description: "Lane protocol parity fixture.",
                    lane_kind: loom_lanes::LaneKind::Assignment.as_str(),
                    owner_principal: Some("agent:3"),
                    lane_status: "ready",
                    lane_tickets: &[
                        LaneTicket {
                            ticket_id: "MX-102".to_string(),
                            order_key: "F".to_string(),
                        },
                        LaneTicket {
                            ticket_id: "MX-103".to_string(),
                            order_key: "V".to_string(),
                        },
                    ],
                    active_ticket_id: Some("MX-102"),
                    status_report: "ready",
                    reviewer_feedback: "",
                    updated_by: Some("agent:3"),
                },
            )
            .map_err(strerr)?;
        assert_lane_baseline("mcp", &mcp_lane)?;
        let mcp_lane = mcp
            .write_lanes_update(
                "main",
                LaneUpdateRequest {
                    lane_id: "mcp",
                    title: Some("Coordinated MCP lane"),
                    description: None,
                    lane_status: Some("working"),
                    status_report: Some("working"),
                    reviewer_feedback: Some("revise order"),
                    updated_by: Some("reviewer"),
                },
            )
            .map_err(strerr)?;
        if mcp_lane.title != "Coordinated MCP lane"
            || mcp_lane.description != "Lane protocol parity fixture."
            || mcp_lane.lane_status != "working"
            || mcp_lane.status_report != "working"
            || mcp_lane.reviewer_feedback != "revise order"
            || mcp_lane.updated_by != "reviewer"
        {
            return Err("MCP lane update must atomically set supplied fields".to_string());
        }
        mcp.write_lanes_ticket_add(
            "main",
            LaneTicketUpdateRequest {
                lane_id: "mcp",
                ticket_id: "MX-104",
                placement: LaneTicketPlacement::First,
                updated_by: Some("agent:3"),
            },
        )
        .map_err(strerr)?;
        mcp.write_lanes_ticket_remove(
            "main",
            LaneTicketUpdateRequest {
                lane_id: "mcp",
                ticket_id: "MX-102",
                placement: LaneTicketPlacement::Append,
                updated_by: Some("agent:3"),
            },
        )
        .map_err(strerr)?;
        let mcp_lane = mcp
            .write_lanes_update(
                "main",
                LaneUpdateRequest {
                    lane_id: "mcp",
                    title: None,
                    description: None,
                    lane_status: None,
                    status_report: Some("working MX-104"),
                    reviewer_feedback: None,
                    updated_by: Some("agent:3"),
                },
            )
            .map_err(strerr)?;
        assert_lane_final("mcp", &mcp_lane)?;
        let active_removed = mcp
            .write_lanes_ticket_remove(
                "main",
                LaneTicketUpdateRequest {
                    lane_id: "mcp",
                    ticket_id: "MX-104",
                    placement: LaneTicketPlacement::Append,
                    updated_by: Some("agent:3"),
                },
            )
            .map_err(strerr)?;
        if active_removed.active_ticket_id.is_some() {
            return Err("MCP ticket removal retained deleted active_ticket_id".to_string());
        }

        assert_lane_mcp_capabilities_are_idl_backed()?;

        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_profile(
            kernel.clone(),
            "lanes",
            "main",
            "lanes",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );
        let rest_created = json_route(
            rest.clone(),
            "POST",
            "/lanes:create",
            "{\"lane_id\":\"rest\",\"lane_key\":\"rest\",\"lane_kind\":\"assignment\",\"owner_principal\":\"agent:3\",\"lane_status\":\"ready\",\"ticket_ids\":[\"MX-102\",\"MX-103\"],\"active_ticket_id\":\"MX-102\",\"status_report\":\"ready\",\"reviewer_feedback\":\"\",\"updated_by\":\"agent:3\"}",
            "lanes-rest-create",
        )
        .await?;
        expect_contains(&rest_created, "\"lane_id\":\"rest\"", "REST lanes create")?;
        let rest_feedback = json_route(
            rest.clone(),
            "POST",
            "/lanes:update",
            "{\"lane_id\":\"rest\",\"status_report\":\"working\",\"reviewer_feedback\":\"revise order\",\"updated_by\":\"reviewer\"}",
            "lanes-rest-feedback",
        )
        .await?;
        expect_contains(
            &rest_feedback,
            "\"updated_by\":\"reviewer\"",
            "REST lanes reviewer feedback",
        )?;
        json_route(
            rest.clone(),
            "POST",
            "/lanes:ticket-add",
            "{\"lane_id\":\"rest\",\"ticket_id\":\"MX-104\",\"placement\":\"first\",\"updated_by\":\"agent:3\"}",
            "lanes-rest-add",
        )
        .await?;
        let rest_removed = json_route(
            rest.clone(),
            "POST",
            "/lanes:ticket-remove",
            "{\"lane_id\":\"rest\",\"ticket_id\":\"MX-102\",\"updated_by\":\"agent:3\"}",
            "lanes-rest-remove",
        )
        .await?;
        expect_contains(&rest_removed, "\"ticket_id\":\"MX-104\"", "REST lanes remove")?;

        let jsonrpc = data_jsonrpc_router_with_profile(
            kernel,
            "lanes",
            "main",
            "lanes",
            Option::<String>::None,
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );
        let jsonrpc_created = json_route(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"lanes.create\",\"params\":{\"lane_id\":\"jsonrpc\",\"lane_key\":\"jsonrpc\",\"lane_kind\":\"assignment\",\"owner_principal\":\"agent:3\",\"lane_status\":\"ready\",\"ticket_ids\":[\"MX-102\",\"MX-103\"],\"active_ticket_id\":\"MX-102\",\"status_report\":\"ready\",\"reviewer_feedback\":\"\",\"updated_by\":\"agent:3\"}}",
            "lanes-jsonrpc-create",
        )
        .await?;
        expect_contains(
            &jsonrpc_created,
            "\"lane_id\":\"jsonrpc\"",
            "JSON-RPC lanes create",
        )?;
        json_route(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"lanes.ticket_add\",\"params\":{\"lane_id\":\"jsonrpc\",\"ticket_id\":\"MX-104\",\"placement\":\"first\",\"updated_by\":\"agent:3\"}}",
            "lanes-jsonrpc-add",
        )
        .await?;
        let jsonrpc_removed = json_route(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"lanes.ticket_remove\",\"params\":{\"lane_id\":\"jsonrpc\",\"ticket_id\":\"MX-102\",\"updated_by\":\"agent:3\"}}",
            "lanes-jsonrpc-remove",
        )
        .await?;
        expect_contains(
            &jsonrpc_removed,
            "\"ticket_id\":\"MX-104\"",
            "JSON-RPC lanes remove",
        )?;

        fs::remove_file(path).map_err(strerr)
    })
}

pub fn certify_in_process_protocols() -> Result<ProtocolConformanceSummary, String> {
    let mcp = certify_in_process_mcp_protocol()?;
    let hosted = certify_in_process_hosted_protocol()?;
    let mut suites = Vec::new();
    suites.extend(mcp.suites);
    suites.extend(hosted.suites);
    Ok(ProtocolConformanceSummary {
        suites_passed: mcp.suites_passed + hosted.suites_passed,
        scenarios_passed: mcp.scenarios_passed + hosted.scenarios_passed,
        suites,
    })
}

fn hosted_meetings_rest_and_jsonrpc_routes_project_snapshot() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-meetings-protocol");
        seed_meetings_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);
        let rest = data_rest_router_with_policy(
            kernel.clone(),
            "meetings",
            "main",
            "organization",
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );
        let projection = json_route(
            rest.clone(),
            "POST",
            "/meetings:projection-outputs",
            "{}",
            "hosted-meetings-rest",
        )
        .await?;
        expect_contains(&projection, "\"workspace_id\":\"organization\"", "REST projection")?;
        expect_contains(&projection, "\"projection\":\"document\"", "REST projection")?;

        let list = json_route(
            rest.clone(),
            "POST",
            "/meetings:list?limit=1",
            "{}",
            "hosted-meetings-rest",
        )
        .await?;
        expect_contains(&list, "\"meeting_id\":\"meet-1\"", "REST meetings list")?;
        expect_contains(&list, "\"total\":1", "REST meetings list")?;

        let get = json_route(
            rest.clone(),
            "POST",
            "/meetings:get",
            "{\"meeting_id\":\"meet-1\"}",
            "hosted-meetings-rest",
        )
        .await?;
        expect_contains(&get, "\"title\":\"Architecture review\"", "REST meetings get")?;
        expect_contains(&get, "\"kind\":\"Decision\"", "REST meetings annotation")?;
        expect_contains(&get, "\"status\":\"accepted\"", "REST meetings annotation")?;

        let review = json_route(
            rest.clone(),
            "POST",
            "/meetings:extraction-review",
            "{}",
            "hosted-meetings-rest",
        )
        .await?;
        expect_contains(
            &review,
            "\"accepted_annotation_ids\":[\"ann-1\"]",
            "REST review",
        )?;
        let accept = json_route(
            rest.clone(),
            "POST",
            "/meetings:accept-annotation",
            "{\"annotation_id\":\"ann-2\"}",
            "hosted-meetings-rest",
        )
        .await?;
        expect_contains(&accept, "\"status\":\"accepted\"", "REST accept annotation")?;
        let vocabulary = json_route(
            rest.clone(),
            "POST",
            "/meetings:propose-vocabulary",
            "{\"term_id\":\"term-1\",\"kind\":\"DomainTerm\",\"label\":\"LCB\",\"evidence_annotation_ids\":[\"ann-2\"],\"aliases\":[\"loom control block\"]}",
            "hosted-meetings-rest",
        )
        .await?;
        expect_contains(
            &vocabulary,
            "\"status\":\"proposed\"",
            "REST propose vocabulary",
        )?;
        let vocabulary = json_route(
            rest.clone(),
            "POST",
            "/meetings:accept-vocabulary",
            "{\"term_id\":\"term-1\"}",
            "hosted-meetings-rest",
        )
        .await?;
        expect_contains(
            &vocabulary,
            "\"status\":\"accepted\"",
            "REST accept vocabulary",
        )?;
        let merge = json_route(
            rest.clone(),
            "POST",
            "/meetings:add-entity-merge",
            "{\"merge_id\":\"merge-1\",\"canonical_entity_id\":\"person:ava\",\"merged_entity_ids\":[\"person:a.vazquez\"],\"evidence_annotation_ids\":[\"ann-1\"]}",
            "hosted-meetings-rest",
        )
        .await?;
        expect_contains(
            &merge,
            "\"canonical_entity_id\":\"person:ava\"",
            "REST entity merge",
        )?;

        let apply = json_route(
            rest.clone(),
            "POST",
            "/meetings:apply-projection-outputs",
            "{}",
            "hosted-meetings-rest",
        )
        .await?;
        expect_contains(&apply, "\"applied\":38", "REST apply")?;
        expect_contains(&apply, "\"skipped\":0", "REST apply")?;
        expect_contains(&apply, "\"document_writes\":8", "REST apply")?;
        expect_contains(&apply, "\"file_writes\":4", "REST apply")?;
        expect_contains(&apply, "\"graph_writes\":6", "REST apply")?;
        expect_contains(&apply, "\"search_writes\":6", "REST apply")?;
        expect_contains(&apply, "\"vector_jobs\":5", "REST apply")?;
        expect_contains(&apply, "\"sql_dataframe_writes\":5", "REST apply")?;
        expect_contains(&apply, "\"ledger_appends\":4", "REST apply")?;

        let materialized = json_route(
            rest.clone(),
            "POST",
            "/meetings:materialized-outputs",
            "{}",
            "hosted-meetings-rest",
        )
        .await?;
        expect_contains(&materialized, "\"total\":38", "REST materialized outputs")?;
        expect_contains(
            &materialized,
            "\"materialized\":33",
            "REST materialized outputs",
        )?;
        expect_contains(&materialized, "\"pending\":5", "REST materialized outputs")?;
        expect_contains(
            &materialized,
            "\"state\":\"no_engine\"",
            "REST materialized outputs",
        )?;
        expect_contains(
            &materialized,
            "\"artifact_ref\":\"sql-dataframe:meetings/organization/meetings_projection_outputs\"",
            "REST materialized outputs",
        )?;

        let search = json_route(
            rest.clone(),
            "POST",
            "/meetings:search",
            "{\"query\":\"Architecture\",\"field\":\"body\",\"limit\":10}",
            "hosted-meetings-rest",
        )
        .await?;
        expect_contains(
            &search,
            "\"meeting_id\":\"meeting/meet-1\"",
            "REST meetings search",
        )?;
        expect_contains(
            &search,
            "\"reason\":\"scan_backed_lexical\"",
            "REST meetings search",
        )?;

        let mcp = LoomMcp::new(StoreAccess::per_request_auth(
            &path,
            LocalOpenAuth {
                principal: Some(nid(1)),
                passphrase: Some("root-pass".to_string()),
                session_id: Some("hosted-meetings-search".to_string()),
                ..LocalOpenAuth::default()
            },
        ));
        let search = mcp
            .read_store_search(StoreSearchReadRequest {
                workspace: Some("main"),
                collection: Some("organization"),
                query: "Architecture",
                field: Some("body"),
                limit: 10,
                offset: 0,
            })
            .map_err(strerr)?;
        if !search.hits.iter().any(|hit| {
            hit.collection == "organization"
                && hit.field == "body"
                && hit.snippet.contains("Architecture")
        }) {
            return Err(format!(
                "MCP search did not retrieve applied Meetings projection: {:?}",
                search.hits
            ));
        }

        let jsonrpc = data_jsonrpc_router_with_policy(
            kernel,
            "meetings",
            "main",
            "organization",
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );
        let projection = json_route(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"meetings.projection_outputs\",\"params\":{}}",
            "hosted-meetings-jsonrpc",
        )
        .await?;
        expect_contains(
            &projection,
            "\"workspace_id\":\"organization\"",
            "JSON-RPC projection",
        )?;
        expect_contains(&projection, "\"projection\":\"document\"", "JSON-RPC projection")?;

        let list = json_route(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"meetings.list\",\"params\":{\"limit\":1}}",
            "hosted-meetings-jsonrpc",
        )
        .await?;
        expect_contains(
            &list,
            "\"meeting_id\":\"meet-1\"",
            "JSON-RPC meetings list",
        )?;

        let get = json_route(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"meetings.get\",\"params\":{\"meeting_id\":\"meet-1\"}}",
            "hosted-meetings-jsonrpc",
        )
        .await?;
        expect_contains(
            &get,
            "\"title\":\"Architecture review\"",
            "JSON-RPC meetings get",
        )?;
        expect_contains(
            &get,
            "\"kind\":\"Decision\"",
            "JSON-RPC meetings annotation",
        )?;
        expect_contains(
            &get,
            "\"status\":\"accepted\"",
            "JSON-RPC meetings annotation",
        )?;

        let review = json_route(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"meetings.extraction_review\",\"params\":{}}",
            "hosted-meetings-jsonrpc",
        )
        .await?;
        expect_contains(
            &review,
            "\"accepted_annotation_ids\":[\"ann-1\",\"ann-2\"]",
            "JSON-RPC review accepted REST write",
        )?;
        let reject = json_route(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"meetings.reject_annotation\",\"params\":{\"annotation_id\":\"ann-3\"}}",
            "hosted-meetings-jsonrpc",
        )
        .await?;
        expect_contains(
            &reject,
            "\"status\":\"rejected\"",
            "JSON-RPC reject annotation",
        )?;
        let review = json_route(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":8,\"method\":\"meetings.extraction_review\",\"params\":{}}",
            "hosted-meetings-jsonrpc",
        )
        .await?;
        expect_contains(
            &review,
            "\"rejected_annotation_ids\":[\"ann-3\"]",
            "JSON-RPC review rejected annotation",
        )?;
        expect_contains(
            &review,
            "\"vocabulary_terms\":1",
            "JSON-RPC review vocabulary terms",
        )?;

        let search = json_route(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"meetings.search\",\"params\":{\"query\":\"Architecture\",\"field\":\"body\",\"limit\":10}}",
            "hosted-meetings-jsonrpc",
        )
        .await?;
        expect_contains(
            &search,
            "\"meeting_id\":\"meeting/meet-1\"",
            "JSON-RPC meetings search",
        )?;

        let apply = json_route(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":6,\"method\":\"meetings.apply_projection_outputs\",\"params\":{}}",
            "hosted-meetings-jsonrpc",
        )
        .await?;
        expect_contains(&apply, "\"already_applied\":4", "JSON-RPC apply")?;
        expect_contains(&apply, "\"applied\":34", "JSON-RPC apply")?;
        expect_contains(&apply, "\"ledger_appends\":0", "JSON-RPC apply")?;

        let materialized = json_route(
            jsonrpc,
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"meetings.materialized_outputs\",\"params\":{}}",
            "hosted-meetings-jsonrpc",
        )
        .await?;
        expect_contains(
            &materialized,
            "\"materialized\":33",
            "JSON-RPC materialized outputs",
        )?;
        expect_contains(
            &materialized,
            "\"state\":\"no_engine\"",
            "JSON-RPC materialized outputs",
        )?;

        fs::remove_file(path).map_err(strerr)?;
        Ok(())
    })
}

fn hosted_chat_drive_rest_and_jsonrpc_routes_project_revision_rows() -> Result<(), String> {
    let runtime = tokio::runtime::Runtime::new().map_err(strerr)?;
    runtime.block_on(async {
        let path = temp_path("hosted-profile-transactions");
        let (workspace, channel) = seed_profile_transaction_store(&path)?;
        let kernel = HostedKernel::new(&path).with_write_guard(HostedWriteGuard::DirectFileLock);

        let rest_drive = data_rest_router_with_policy(
            kernel.clone(),
            "drive",
            "main",
            "main",
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );
        drive_upload_via_rest(rest_drive, "file-rest", "upload-rest").await?;
        expect_revision_history(
            &kernel,
            workspace,
            "main",
            "drive:file:file-rest",
            1,
            "application/vnd.uldren.loom.drive.file-content",
        )?;

        let jsonrpc_drive = data_jsonrpc_router_with_policy(
            kernel.clone(),
            "drive",
            "main",
            "main",
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );
        drive_upload_via_jsonrpc(jsonrpc_drive, "file-jsonrpc", "upload-jsonrpc").await?;
        expect_revision_history(
            &kernel,
            workspace,
            "main",
            "drive:file:file-jsonrpc",
            1,
            "application/vnd.uldren.loom.drive.file-content",
        )?;

        let rest_chat = data_rest_router_with_profile(
            kernel.clone(),
            "chat",
            "main",
            "studio",
            Some("general"),
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );
        chat_post_edit_emoji_via_rest(rest_chat, "m-rest").await?;
        expect_revision_history(
            &kernel,
            workspace,
            "studio",
            &format!("chat:{channel}:message:m-rest"),
            2,
            "application/vnd.uldren.loom.chat.operation+cbor",
        )?;

        let jsonrpc_chat = data_jsonrpc_router_with_profile(
            kernel.clone(),
            "chat",
            "main",
            "studio",
            Some("general"),
            16 * 1024 * 1024,
            HostedAuthPolicy::OwnerOrPassphrase,
        );
        chat_post_edit_emoji_via_jsonrpc(jsonrpc_chat, "m-jsonrpc").await?;
        expect_revision_history(
            &kernel,
            workspace,
            "studio",
            &format!("chat:{channel}:message:m-jsonrpc"),
            2,
            "application/vnd.uldren.loom.chat.operation+cbor",
        )?;

        fs::remove_file(path).map_err(strerr)?;
        Ok(())
    })
}

async fn drive_upload_via_rest(
    router: axum::Router,
    file_id: &str,
    upload_id: &str,
) -> Result<(), String> {
    let root = json_route(
        router.clone(),
        "POST",
        "/drive:list",
        "{\"folder_id\":\"root\"}",
        "hosted-profile-rest-drive",
    )
    .await?;
    let root = json_string_field(&root, "profile_root")?;
    let create = format!(
        "{{\"upload_id\":\"{upload_id}\",\"parent_folder_id\":\"root\",\"name\":\"Rest.txt\",\"file_id\":\"{file_id}\",\"expected_root\":\"{root}\",\"created_at_ms\":100,\"replace_file\":false}}"
    );
    json_route(
        router.clone(),
        "POST",
        "/drive:create-upload",
        &create,
        "hosted-profile-rest-drive",
    )
    .await?;
    json_route(
        router.clone(),
        "POST",
        "/drive:upload-chunk",
        &format!("{{\"upload_id\":\"{upload_id}\",\"bytes_hex\":\"72657374\"}}"),
        "hosted-profile-rest-drive",
    )
    .await?;
    let commit = json_route(
        router,
        "POST",
        "/drive:commit-upload",
        &format!("{{\"upload_id\":\"{upload_id}\"}}"),
        "hosted-profile-rest-drive",
    )
    .await?;
    expect_contains(
        &commit,
        "\"operation_kind\":\"file.upload_committed\"",
        "REST drive commit",
    )
}

async fn drive_upload_via_jsonrpc(
    router: axum::Router,
    file_id: &str,
    upload_id: &str,
) -> Result<(), String> {
    let root = json_route(
        router.clone(),
        "POST",
        "/jsonrpc",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"drive.list\",\"params\":{\"folder_id\":\"root\"}}",
        "hosted-profile-jsonrpc-drive",
    )
    .await?;
    let root = json_result_string_field(&root, "profile_root")?;
    let create = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"drive.create_upload\",\"params\":{{\"upload_id\":\"{upload_id}\",\"parent_folder_id\":\"root\",\"name\":\"Jsonrpc.txt\",\"file_id\":\"{file_id}\",\"expected_root\":\"{root}\",\"created_at_ms\":100,\"replace_file\":false}}}}"
    );
    json_route(
        router.clone(),
        "POST",
        "/jsonrpc",
        &create,
        "hosted-profile-jsonrpc-drive",
    )
    .await?;
    let chunk = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"drive.upload_chunk\",\"params\":{{\"upload_id\":\"{upload_id}\",\"bytes_hex\":\"6a736f6e727063\"}}}}"
    );
    json_route(
        router.clone(),
        "POST",
        "/jsonrpc",
        &chunk,
        "hosted-profile-jsonrpc-drive",
    )
    .await?;
    let commit = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"drive.commit_upload\",\"params\":{{\"upload_id\":\"{upload_id}\"}}}}"
    );
    let commit = json_route(
        router,
        "POST",
        "/jsonrpc",
        &commit,
        "hosted-profile-jsonrpc-drive",
    )
    .await?;
    expect_contains(
        &commit,
        "\"operation_kind\":\"file.upload_committed\"",
        "JSON-RPC drive commit",
    )
}

async fn chat_post_edit_emoji_via_rest(
    router: axum::Router,
    message_id: &str,
) -> Result<(), String> {
    let post = format!("{{\"message_id\":\"{message_id}\",\"body_hex\":\"68656c6c6f\"}}");
    json_route(
        router.clone(),
        "POST",
        "/chat:post-message",
        &post,
        "hosted-profile-rest-chat",
    )
    .await?;
    let edit = format!("{{\"message_id\":\"{message_id}\",\"body_hex\":\"656469746564\"}}");
    let edit = json_route(
        router.clone(),
        "POST",
        "/chat:edit-message",
        &edit,
        "hosted-profile-rest-chat",
    )
    .await?;
    expect_contains(
        &edit,
        "\"operation_kind\":\"message.edited\"",
        "REST chat edit",
    )?;
    json_route(
        router.clone(),
        "POST",
        "/chat:emoji-register",
        "{\"kind\":\"reviewed\"}",
        "hosted-profile-rest-chat",
    )
    .await?;
    let reaction = format!("{{\"message_id\":\"{message_id}\",\"kind\":\"reviewed\"}}");
    let reaction = json_route(
        router.clone(),
        "POST",
        "/chat:add-reaction",
        &reaction,
        "hosted-profile-rest-chat",
    )
    .await?;
    expect_contains(
        &reaction,
        "\"operation_kind\":\"reaction.added\"",
        "REST chat reaction",
    )?;
    let emoji = json_route(
        router,
        "POST",
        "/chat:emoji-unregister",
        "{\"kind\":\"reviewed\"}",
        "hosted-profile-rest-chat",
    )
    .await?;
    if emoji.contains("\"reviewed\"") {
        return Err("REST chat emoji unregister retained kind".to_string());
    }
    Ok(())
}

async fn chat_post_edit_emoji_via_jsonrpc(
    router: axum::Router,
    message_id: &str,
) -> Result<(), String> {
    let post = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"chat.post_message\",\"params\":{{\"message_id\":\"{message_id}\",\"body_hex\":\"68656c6c6f\"}}}}"
    );
    json_route(
        router.clone(),
        "POST",
        "/jsonrpc",
        &post,
        "hosted-profile-jsonrpc-chat",
    )
    .await?;
    let edit = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"chat.edit_message\",\"params\":{{\"message_id\":\"{message_id}\",\"body_hex\":\"656469746564\"}}}}"
    );
    let edit = json_route(
        router.clone(),
        "POST",
        "/jsonrpc",
        &edit,
        "hosted-profile-jsonrpc-chat",
    )
    .await?;
    expect_contains(
        &edit,
        "\"operation_kind\":\"message.edited\"",
        "JSON-RPC chat edit",
    )?;
    json_route(
        router.clone(),
        "POST",
        "/jsonrpc",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"chat.emoji_register\",\"params\":{\"kind\":\"reviewed\"}}",
        "hosted-profile-jsonrpc-chat",
    )
    .await?;
    let reaction = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"chat.add_reaction\",\"params\":{{\"message_id\":\"{message_id}\",\"kind\":\"reviewed\"}}}}"
    );
    let reaction = json_route(
        router.clone(),
        "POST",
        "/jsonrpc",
        &reaction,
        "hosted-profile-jsonrpc-chat",
    )
    .await?;
    expect_contains(
        &reaction,
        "\"operation_kind\":\"reaction.added\"",
        "JSON-RPC chat reaction",
    )?;
    let emoji = json_route(
        router,
        "POST",
        "/jsonrpc",
        "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"chat.emoji_unregister\",\"params\":{\"kind\":\"reviewed\"}}",
        "hosted-profile-jsonrpc-chat",
    )
    .await?;
    if emoji.contains("\"reviewed\"") {
        return Err("JSON-RPC chat emoji unregister retained kind".to_string());
    }
    Ok(())
}

async fn json_route(
    router: axum::Router,
    method: &str,
    uri: &str,
    body: &str,
    session: &str,
) -> Result<String, String> {
    let response = router
        .oneshot(root_json_request(method, uri, body, session))
        .await
        .map_err(strerr)?;
    if !response.status().is_success() {
        return Err(format!("route {uri} returned {}", response.status()));
    }
    let body = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .map_err(strerr)?;
    String::from_utf8(body.to_vec()).map_err(strerr)
}

fn root_json_request(method: &str, uri: &str, body: &str, session: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(CONTENT_TYPE, "application/json")
        .header("x-loom-principal", nid(1).to_string())
        .header("x-loom-passphrase", "root-pass")
        .header("x-loom-session", session)
        .body(Body::from(body.to_string()))
        .expect("valid conformance request")
}

fn root_local_auth(session: &str) -> LocalOpenAuth {
    LocalOpenAuth {
        principal: Some(nid(1)),
        passphrase: Some("root-pass".to_string()),
        session_id: Some(session.to_string()),
        ..LocalOpenAuth::default()
    }
}

fn seed_lane_store(path: &PathBuf) -> Result<WorkspaceId, String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let workspace = nid(9);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let workspace = loom
        .registry_mut()
        .create(FacetKind::Files, Some("main"), workspace)
        .map_err(strerr)?;
    loom.registry_mut()
        .add_facet(workspace, FacetKind::Document)
        .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok(workspace)
}

fn seed_meetings_store(path: &PathBuf) -> Result<WorkspaceId, String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let ns = nid(9);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let ns = loom
        .registry_mut()
        .create(FacetKind::Files, Some("main"), ns)
        .map_err(strerr)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Vcs)
        .map_err(strerr)?;
    let snapshot = sample_meetings_snapshot();
    loom.store()
        .control_set(
            &meetings_profile_key(&snapshot.workspace_id).map_err(strerr)?,
            snapshot.encode().map_err(strerr)?,
        )
        .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok(ns)
}

fn seed_cas_auth_store(path: &PathBuf) -> Result<WorkspaceId, String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let user = nid(7);
    let workspace = nid(9);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    identity
        .add_principal(user, "alice", PrincipalKind::User)
        .map_err(strerr)?;
    identity
        .set_passphrase(user, "alice-pass", b"abcdefgh")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let workspace = loom
        .registry_mut()
        .create(FacetKind::Files, Some("main"), workspace)
        .map_err(strerr)?;
    loom.registry_mut()
        .add_facet(workspace, FacetKind::Cas)
        .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok(workspace)
}

fn seed_vcs_protected_ref_store(path: &PathBuf) -> Result<WorkspaceId, String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let workspace = nid(9);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let workspace = loom
        .registry_mut()
        .create(FacetKind::Files, Some("main"), workspace)
        .map_err(strerr)?;
    loom.registry_mut()
        .add_facet(workspace, FacetKind::Vcs)
        .map_err(strerr)?;
    loom.set_protected_ref_policy(
        workspace,
        "branch/main",
        ProtectedRefPolicy {
            signed_commits_required: true,
            ..ProtectedRefPolicy::default()
        },
    )
    .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok(workspace)
}

fn seed_queue_store(path: &PathBuf) -> Result<WorkspaceId, String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let workspace = nid(9);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let workspace = loom
        .registry_mut()
        .create(FacetKind::Queue, Some("main"), workspace)
        .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok(workspace)
}

fn seed_queue_auth_store(path: &PathBuf) -> Result<WorkspaceId, String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let user = nid(7);
    let workspace = nid(9);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    identity
        .add_principal(user, "alice", PrincipalKind::User)
        .map_err(strerr)?;
    identity
        .set_passphrase(user, "alice-pass", b"abcdefgh")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let workspace = loom
        .registry_mut()
        .create(FacetKind::Queue, Some("main"), workspace)
        .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok(workspace)
}

fn seed_timeseries_store(path: &PathBuf) -> Result<WorkspaceId, String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let workspace = nid(9);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let workspace = loom
        .registry_mut()
        .create(FacetKind::TimeSeries, Some("main"), workspace)
        .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok(workspace)
}

fn seed_timeseries_auth_store(path: &PathBuf) -> Result<WorkspaceId, String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let user = nid(7);
    let workspace = nid(9);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    identity
        .add_principal(user, "alice", PrincipalKind::User)
        .map_err(strerr)?;
    identity
        .set_passphrase(user, "alice-pass", b"abcdefgh")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let workspace = loom
        .registry_mut()
        .create(FacetKind::TimeSeries, Some("main"), workspace)
        .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok(workspace)
}

fn seed_graph_auth_store(path: &PathBuf) -> Result<WorkspaceId, String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let user = nid(7);
    let workspace = nid(9);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    identity
        .add_principal(user, "alice", PrincipalKind::User)
        .map_err(strerr)?;
    identity
        .set_passphrase(user, "alice-pass", b"abcdefgh")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let workspace = loom
        .registry_mut()
        .create(FacetKind::Graph, Some("main"), workspace)
        .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok(workspace)
}

fn seed_vector_auth_store(path: &PathBuf) -> Result<WorkspaceId, String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let user = nid(7);
    let workspace = nid(9);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    identity
        .add_principal(user, "alice", PrincipalKind::User)
        .map_err(strerr)?;
    identity
        .set_passphrase(user, "alice-pass", b"abcdefgh")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let workspace = loom
        .registry_mut()
        .create(FacetKind::Vector, Some("main"), workspace)
        .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok(workspace)
}

fn seed_columnar_auth_store(path: &PathBuf) -> Result<WorkspaceId, String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let user = nid(7);
    let workspace = nid(9);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    identity
        .add_principal(user, "alice", PrincipalKind::User)
        .map_err(strerr)?;
    identity
        .set_passphrase(user, "alice-pass", b"abcdefgh")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let workspace = loom
        .registry_mut()
        .create(FacetKind::Columnar, Some("main"), workspace)
        .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok(workspace)
}

fn seed_ledger_auth_store(path: &PathBuf) -> Result<WorkspaceId, String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let user = nid(7);
    let workspace = nid(9);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    identity
        .add_principal(user, "alice", PrincipalKind::User)
        .map_err(strerr)?;
    identity
        .set_passphrase(user, "alice-pass", b"abcdefgh")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let workspace = loom
        .registry_mut()
        .create(FacetKind::Ledger, Some("main"), workspace)
        .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok(workspace)
}

fn seed_kv_auth_store(path: &PathBuf) -> Result<WorkspaceId, String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let user = nid(7);
    let workspace = nid(9);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    identity
        .add_principal(user, "alice", PrincipalKind::User)
        .map_err(strerr)?;
    identity
        .set_passphrase(user, "alice-pass", b"abcdefgh")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let workspace = loom
        .registry_mut()
        .create(FacetKind::Kv, Some("main"), workspace)
        .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok(workspace)
}

fn seed_document_auth_store(path: &PathBuf) -> Result<WorkspaceId, String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let user = nid(7);
    let workspace = nid(9);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    identity
        .add_principal(user, "alice", PrincipalKind::User)
        .map_err(strerr)?;
    identity
        .set_passphrase(user, "alice-pass", b"abcdefgh")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let workspace = loom
        .registry_mut()
        .create(FacetKind::Document, Some("main"), workspace)
        .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok(workspace)
}

fn seed_profile_transaction_store(path: &PathBuf) -> Result<(WorkspaceId, WorkspaceId), String> {
    let fs = FileStore::create_with_profile(path, Algo::Blake3).map_err(strerr)?;
    let root = nid(1);
    let ns = nid(9);
    let channel = nid(44);
    let mut identity = IdentityStore::new(root);
    identity
        .set_passphrase(root, "root-pass", b"12345678")
        .map_err(strerr)?;
    fs.save_identity_store(&identity).map_err(strerr)?;
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
    .map_err(strerr)?;
    fs.save_acl_store(&acl).map_err(strerr)?;
    let mut loom = Loom::new(fs);
    let ns = loom
        .registry_mut()
        .create(FacetKind::Files, Some("main"), ns)
        .map_err(strerr)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Vcs)
        .map_err(strerr)?;
    let mut directory = ChatChannelDirectory::new("studio").map_err(strerr)?;
    directory
        .create_channel(channel, "general", "General")
        .map_err(strerr)?;
    let path =
        String::from_utf8(chat_channel_directory_key("studio").map_err(strerr)?).map_err(strerr)?;
    let parent = path
        .rsplit_once('/')
        .ok_or_else(|| "chat directory path missing parent".to_string())?
        .0;
    loom.create_directory_reserved(ns, parent, true)
        .map_err(strerr)?;
    loom.write_file_reserved(ns, &path, &directory.encode().map_err(strerr)?, 0o100644)
        .map_err(strerr)?;
    save_loom(&mut loom).map_err(strerr)?;
    Ok((ns, channel))
}

fn expect_revision_history(
    kernel: &HostedKernel,
    workspace: WorkspaceId,
    scope_id: &str,
    entity_id: &str,
    expected_len: usize,
    expected_media_type: &str,
) -> Result<(), String> {
    let auth = HostedAuth::passphrase(nid(1), "root-pass", "hosted-profile-history");
    let path = revision_index_path(scope_id).map_err(strerr)?;
    let index = kernel
        .read(&auth, |loom| {
            loom.read_file_reserved(workspace, &path)
                .and_then(|bytes| RevisionIndex::decode(&bytes))
        })
        .map_err(|error| error.message)?;
    let history = index.history(entity_id);
    if history.len() != expected_len {
        return Err(format!(
            "{entity_id} expected {expected_len} revisions, found {}",
            history.len()
        ));
    }
    let Some(last) = history.last() else {
        return Err(format!("{entity_id} has no revision history"));
    };
    if last.revision
        != u64::try_from(expected_len).map_err(|_| "revision count overflow".to_string())?
    {
        return Err(format!(
            "{entity_id} latest revision is {}, expected {expected_len}",
            last.revision
        ));
    }
    if last.body.media_type != expected_media_type {
        return Err(format!(
            "{entity_id} media type is {}, expected {expected_media_type}",
            last.body.media_type
        ));
    }
    Ok(())
}

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
    .expect("valid meeting source");
    source.sidecar_digest = Some(Digest::hash(Algo::Blake3, b"meeting-sidecar"));

    let mut meeting = MeetingRecord::new(MeetingRecordInput {
        meeting_id: "meet-1",
        title: "Architecture review",
        current_source_digest: Digest::hash(Algo::Blake3, b"meeting-source"),
        created_at_ms: 100,
        updated_at_ms: 120,
    })
    .expect("valid meeting");
    meeting.source_refs = vec!["src-1".to_string()];
    meeting.attendee_refs = vec!["person:ava".to_string(), "person:nas".to_string()];

    let mut span = SpanRecord::new(
        "span-1",
        "meet-1",
        "src-1",
        SpanKind::TranscriptEntry,
        "granola:not_1/transcript/0",
    )
    .expect("valid span");
    span.text_digest = Some(Digest::hash(Algo::Blake3, b"meeting-text"));

    let mut annotation = AnnotationRecord::new(
        "ann-1",
        "meet-1",
        vec!["span-1".to_string()],
        "Decision",
        "Use normalized import snapshots",
        130,
    )
    .expect("valid annotation");
    annotation
        .accept("principal-1", 140)
        .expect("valid accepted annotation");
    let suggested_annotation = AnnotationRecord::new(
        "ann-2",
        "meet-1",
        vec!["span-1".to_string()],
        "Risk",
        "Migration risk",
        150,
    )
    .expect("valid suggested annotation");
    let rejected_annotation = AnnotationRecord::new(
        "ann-3",
        "meet-1",
        vec!["span-1".to_string()],
        "Task",
        "Rewrite history",
        160,
    )
    .expect("valid suggested annotation");

    let mut import_run = ImportRunRecord::new(
        "run-1",
        InputProfile::GranolaApi,
        "personal-notes",
        MeetingsCoverage::Partial,
        90,
    )
    .expect("valid import run");
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
    .expect("valid redaction");
    redaction.retained_digest = Some(Digest::hash(Algo::Blake3, b"retained-metadata"));

    MeetingsProfileSnapshot::new(
        "organization",
        MeetingsProfileSnapshotParts {
            sources: vec![source],
            meetings: vec![meeting],
            spans: vec![span],
            annotations: vec![annotation, suggested_annotation, rejected_annotation],
            vocabulary_terms: Vec::new(),
            entity_merges: Vec::new(),
            promotions: Vec::new(),
            import_runs: vec![import_run],
            redactions: vec![redaction],
        },
    )
    .expect("valid meetings snapshot")
}

fn json_string_field(body: &str, field: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(strerr)?;
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("JSON field {field} missing from {body}"))
}

fn json_result_string_field(body: &str, field: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(strerr)?;
    value
        .get("result")
        .and_then(|result| result.get(field))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("JSON-RPC result field {field} missing from {body}"))
}

fn assert_lane_baseline(label: &str, lane: &Lane) -> Result<(), String> {
    if lane.lane_status != "ready" {
        return Err(format!("{label} Lane status drifted: {}", lane.lane_status));
    }
    if lane.active_ticket_id.as_deref() != Some("MX-102") {
        return Err(format!(
            "{label} Lane active_ticket_id drifted: {:?}",
            lane.active_ticket_id
        ));
    }
    if lane.lane_tickets.len() != 2
        || lane.lane_tickets[0].ticket_id != "MX-102"
        || lane.lane_tickets[0].order_key != "F"
        || lane.lane_tickets[1].ticket_id != "MX-103"
        || lane.lane_tickets[1].order_key != "V"
    {
        return Err(format!(
            "{label} Lane membership baseline drifted: {:?}",
            lane.lane_tickets
        ));
    }
    Ok(())
}

fn assert_lane_final(label: &str, lane: &Lane) -> Result<(), String> {
    if lane.active_ticket_id.is_some() {
        return Err(format!(
            "{label} Lane active_ticket_id was not derived-only: {:?}",
            lane.active_ticket_id
        ));
    }
    if lane.lane_tickets.len() != 2
        || lane.lane_tickets[0].ticket_id != "MX-104"
        || lane.lane_tickets[1].ticket_id != "MX-103"
    {
        return Err(format!(
            "{label} Lane final membership drifted: {:?}",
            lane.lane_tickets
        ));
    }
    Ok(())
}

fn assert_lane_mcp_capabilities_are_idl_backed() -> Result<(), String> {
    for name in [
        "lanes_create",
        "lanes_get",
        "lanes_list",
        "lanes_update",
        "lanes_ticket_add",
        "lanes_ticket_remove",
    ] {
        let Some(tool) = loom_mcp::tools::tool(name) else {
            return Err(format!("MCP Lane tool {name} is missing from the catalog"));
        };
        if tool.remote_capability() != RemoteCapability::Unary {
            return Err(format!(
                "MCP Lane tool {name} did not advertise IDL-backed remote capability"
            ));
        }
    }
    Ok(())
}

fn expect_contains(haystack: &str, needle: &str, label: &str) -> Result<(), String> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(format!("{label} missing {needle}: {haystack}"))
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn temp_path(name: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut path = std::env::temp_dir();
    path.push(format!(
        "loom-protocol-conformance-{name}-{}-{}.loom",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_file(&path);
    path
}

fn nid(byte: u8) -> WorkspaceId {
    WorkspaceId::from_bytes([byte; 16])
}

fn strerr(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(all(test, feature = "integration-tests"))]
mod tests {
    use super::*;

    #[test]
    fn mcp_protocol_certification_manifest_is_pinned() {
        use loom_mcp::server::conformance::{
            MCP_PROTOCOL_CERTIFICATION_SCENARIOS, MCP_PROTOCOL_CERTIFICATION_SUITES,
        };

        assert_eq!(MCP_PROTOCOL_CERTIFICATION_SCENARIOS, 13);
        assert_eq!(
            MCP_PROTOCOL_CERTIFICATION_SUITES,
            [
                "mcp-substrate-transact",
                "mcp-search",
                "mcp-substrate-changes",
                "mcp-substrate-refs",
                "mcp-chat",
                "mcp-meetings",
                "mcp-studio-status"
            ]
        );
    }

    #[test]
    fn hosted_protocol_certification_manifest_is_pinned() {
        assert_eq!(HOSTED_PROTOCOL_CERTIFICATION_SCENARIOS, 206);
        assert_eq!(
            HOSTED_PROTOCOL_CERTIFICATION_SUITES,
            [
                "hosted-meetings",
                "hosted-reference-reconciliation",
                "lanes-local-mcp-hosted-parity",
                "hosted-profile-transactions",
                "hosted-network-access",
                "hosted-cas-auth-acl",
                "hosted-timeseries-auth-acl",
                "hosted-timeseries-read-only-write-denial",
                "hosted-cas-rest-jsonrpc",
                "hosted-cas-grpc",
                "hosted-queue-grpc",
                "hosted-queue-read-only-write-denial",
                "hosted-timeseries-grpc",
                "hosted-queue-rest",
                "hosted-queue-jsonrpc",
                "hosted-timeseries-rest",
                "hosted-timeseries-jsonrpc",
                "hosted-ledger-rest",
                "hosted-ledger-jsonrpc",
                "hosted-ledger-read-only-write-denial",
                "hosted-fts-rest",
                "hosted-fts-jsonrpc",
                "hosted-graph-read-only-write-denial",
                "hosted-graph-rest",
                "hosted-graph-jsonrpc",
                "hosted-vector-read-only-write-denial",
                "hosted-vector-rest",
                "hosted-vector-jsonrpc",
                "hosted-columnar-read-only-write-denial",
                "hosted-columnar-result-handle-auth",
                "hosted-vcs-protected-ref-write",
                "hosted-columnar-rest",
                "hosted-columnar-jsonrpc",
                "hosted-kv-read-only-write-denial",
                "hosted-document-read-only-write-denial",
                "hosted-kv-rest",
                "hosted-kv-jsonrpc"
            ]
        );
    }

    #[test]
    fn hosted_meetings_rest_and_jsonrpc_routes_project_snapshot_passes() {
        hosted_meetings_rest_and_jsonrpc_routes_project_snapshot()
            .expect("hosted meetings REST and JSON-RPC routes");
    }

    #[test]
    fn hosted_reference_reconciliation_adapters_preserve_auth_passes() {
        hosted_reference_reconciliation_adapters_preserve_auth()
            .expect("hosted reference reconciliation adapters");
    }

    #[test]
    fn hosted_chat_drive_rest_and_jsonrpc_routes_project_revision_rows_passes() {
        hosted_chat_drive_rest_and_jsonrpc_routes_project_revision_rows()
            .expect("hosted profile transactions");
    }

    #[test]
    fn hosted_network_access_matrix_passes() {
        hosted_network_access_matrix().expect("hosted network-access matrix");
    }

    #[test]
    fn lane_behavioral_conformance_across_local_mcp_and_hosted_passes() {
        lane_behavioral_conformance_across_local_mcp_and_hosted()
            .expect("Lane behavioral conformance");
    }

    #[test]
    fn hosted_cas_auth_acl_matrix_passes() {
        hosted_cas_auth_acl_matrix().expect("hosted CAS auth and ACL matrix");
    }

    #[test]
    fn hosted_timeseries_auth_acl_matrix_passes() {
        hosted_timeseries_auth_acl_matrix().expect("hosted TimeSeries auth and ACL matrix");
    }

    #[test]
    fn hosted_timeseries_read_only_write_denial_matrix_passes() {
        hosted_timeseries_read_only_write_denial_matrix()
            .expect("hosted TimeSeries read-only write-denial matrix");
    }

    #[test]
    fn hosted_cas_rest_and_jsonrpc_round_trip_matrix_passes() {
        hosted_cas_rest_and_jsonrpc_round_trip_matrix()
            .expect("hosted CAS REST and JSON-RPC round-trip matrix");
    }

    #[test]
    fn hosted_cas_grpc_round_trip_matrix_passes() {
        hosted_cas_grpc_round_trip_matrix().expect("hosted CAS gRPC round-trip matrix");
    }

    #[test]
    fn hosted_queue_grpc_round_trip_matrix_passes() {
        hosted_queue_grpc_round_trip_matrix().expect("hosted Queue gRPC round-trip matrix");
    }

    #[test]
    fn hosted_queue_read_only_write_denial_matrix_passes() {
        hosted_queue_read_only_write_denial_matrix()
            .expect("hosted Queue read-only write-denial matrix");
    }

    #[test]
    fn hosted_timeseries_grpc_round_trip_matrix_passes() {
        hosted_timeseries_grpc_round_trip_matrix()
            .expect("hosted Time-series gRPC round-trip matrix");
    }

    #[test]
    fn hosted_queue_rest_round_trip_matrix_passes() {
        hosted_queue_rest_round_trip_matrix().expect("hosted Queue REST round-trip matrix");
    }

    #[test]
    fn hosted_queue_jsonrpc_round_trip_matrix_passes() {
        hosted_queue_jsonrpc_round_trip_matrix().expect("hosted Queue JSON-RPC round-trip matrix");
    }

    #[test]
    fn hosted_timeseries_rest_round_trip_matrix_passes() {
        hosted_timeseries_rest_round_trip_matrix()
            .expect("hosted Time-series REST round-trip matrix");
    }

    #[test]
    fn hosted_timeseries_jsonrpc_round_trip_matrix_passes() {
        hosted_timeseries_jsonrpc_round_trip_matrix()
            .expect("hosted Time-series JSON-RPC round-trip matrix");
    }

    #[test]
    fn hosted_ledger_rest_round_trip_matrix_passes() {
        hosted_ledger_rest_round_trip_matrix().expect("hosted Ledger REST round-trip matrix");
    }

    #[test]
    fn hosted_ledger_jsonrpc_round_trip_matrix_passes() {
        hosted_ledger_jsonrpc_round_trip_matrix()
            .expect("hosted Ledger JSON-RPC round-trip matrix");
    }

    #[test]
    fn hosted_ledger_read_only_write_denial_matrix_passes() {
        hosted_ledger_read_only_write_denial_matrix()
            .expect("hosted Ledger read-only write-denial matrix");
    }

    #[test]
    fn hosted_fts_rest_round_trip_matrix_passes() {
        hosted_fts_rest_round_trip_matrix().expect("hosted FTS REST round-trip matrix");
    }

    #[test]
    fn hosted_fts_jsonrpc_round_trip_matrix_passes() {
        hosted_fts_jsonrpc_round_trip_matrix().expect("hosted FTS JSON-RPC round-trip matrix");
    }

    #[test]
    fn hosted_graph_read_only_write_denial_matrix_passes() {
        hosted_graph_read_only_write_denial_matrix()
            .expect("hosted Graph read-only write-denial matrix");
    }

    #[test]
    fn hosted_graph_rest_round_trip_matrix_passes() {
        hosted_graph_rest_round_trip_matrix().expect("hosted Graph REST round-trip matrix");
    }

    #[test]
    fn hosted_graph_jsonrpc_round_trip_matrix_passes() {
        hosted_graph_jsonrpc_round_trip_matrix().expect("hosted Graph JSON-RPC round-trip matrix");
    }

    #[test]
    fn hosted_vector_read_only_write_denial_matrix_passes() {
        hosted_vector_read_only_write_denial_matrix()
            .expect("hosted Vector read-only write-denial matrix");
    }

    #[test]
    fn hosted_vector_rest_round_trip_matrix_passes() {
        hosted_vector_rest_round_trip_matrix().expect("hosted Vector REST round-trip matrix");
    }

    #[test]
    fn hosted_vector_jsonrpc_round_trip_matrix_passes() {
        hosted_vector_jsonrpc_round_trip_matrix()
            .expect("hosted Vector JSON-RPC round-trip matrix");
    }

    #[test]
    fn hosted_columnar_read_only_write_denial_matrix_passes() {
        hosted_columnar_read_only_write_denial_matrix()
            .expect("hosted Columnar read-only write-denial matrix");
    }

    #[test]
    fn hosted_columnar_result_handle_auth_matrix_passes() {
        hosted_columnar_result_handle_auth_matrix()
            .expect("hosted Columnar result-handle auth matrix");
    }

    #[test]
    fn hosted_vcs_protected_ref_write_matrix_passes() {
        hosted_vcs_protected_ref_write_matrix().expect("hosted VCS protected-ref write matrix");
    }

    #[test]
    fn hosted_columnar_rest_round_trip_matrix_passes() {
        hosted_columnar_rest_round_trip_matrix().expect("hosted Columnar REST round-trip matrix");
    }

    #[test]
    fn hosted_columnar_jsonrpc_round_trip_matrix_passes() {
        hosted_columnar_jsonrpc_round_trip_matrix()
            .expect("hosted Columnar JSON-RPC round-trip matrix");
    }

    #[test]
    fn hosted_kv_read_only_write_denial_matrix_passes() {
        hosted_kv_read_only_write_denial_matrix().expect("hosted KV read-only write-denial matrix");
    }

    #[test]
    fn hosted_document_read_only_write_denial_matrix_passes() {
        hosted_document_read_only_write_denial_matrix()
            .expect("hosted Document read-only write-denial matrix");
    }

    #[test]
    fn hosted_kv_rest_round_trip_matrix_passes() {
        hosted_kv_rest_round_trip_matrix().expect("hosted KV REST round-trip matrix");
    }

    #[test]
    fn hosted_kv_jsonrpc_round_trip_matrix_passes() {
        hosted_kv_jsonrpc_round_trip_matrix().expect("hosted KV JSON-RPC round-trip matrix");
    }

    #[test]
    fn aggregate_protocol_certification_manifest_includes_mcp_and_hosted() {
        use loom_mcp::server::conformance::{
            MCP_PROTOCOL_CERTIFICATION_SCENARIOS, MCP_PROTOCOL_CERTIFICATION_SUITES,
        };

        let mut suites = MCP_PROTOCOL_CERTIFICATION_SUITES.to_vec();
        suites.extend(HOSTED_PROTOCOL_CERTIFICATION_SUITES);
        assert_eq!(suites.len(), 44);
        assert_eq!(
            MCP_PROTOCOL_CERTIFICATION_SCENARIOS + HOSTED_PROTOCOL_CERTIFICATION_SCENARIOS,
            219
        );
        assert!(suites.contains(&"mcp-meetings"));
        assert!(suites.contains(&"hosted-meetings"));
        assert!(suites.contains(&"hosted-reference-reconciliation"));
        assert!(suites.contains(&"lanes-local-mcp-hosted-parity"));
        assert!(suites.contains(&"hosted-profile-transactions"));
        assert!(suites.contains(&"hosted-network-access"));
        assert!(suites.contains(&"hosted-cas-auth-acl"));
        assert!(suites.contains(&"hosted-timeseries-auth-acl"));
        assert!(suites.contains(&"hosted-timeseries-read-only-write-denial"));
        assert!(suites.contains(&"hosted-cas-rest-jsonrpc"));
        assert!(suites.contains(&"hosted-cas-grpc"));
        assert!(suites.contains(&"hosted-queue-grpc"));
        assert!(suites.contains(&"hosted-queue-read-only-write-denial"));
        assert!(suites.contains(&"hosted-timeseries-grpc"));
        assert!(suites.contains(&"hosted-queue-rest"));
        assert!(suites.contains(&"hosted-queue-jsonrpc"));
        assert!(suites.contains(&"hosted-timeseries-rest"));
        assert!(suites.contains(&"hosted-timeseries-jsonrpc"));
        assert!(suites.contains(&"hosted-ledger-rest"));
        assert!(suites.contains(&"hosted-ledger-jsonrpc"));
        assert!(suites.contains(&"hosted-ledger-read-only-write-denial"));
        assert!(suites.contains(&"hosted-fts-rest"));
        assert!(suites.contains(&"hosted-fts-jsonrpc"));
        assert!(suites.contains(&"hosted-graph-read-only-write-denial"));
        assert!(suites.contains(&"hosted-graph-rest"));
        assert!(suites.contains(&"hosted-graph-jsonrpc"));
        assert!(suites.contains(&"hosted-vector-read-only-write-denial"));
        assert!(suites.contains(&"hosted-vector-rest"));
        assert!(suites.contains(&"hosted-vector-jsonrpc"));
        assert!(suites.contains(&"hosted-columnar-read-only-write-denial"));
        assert!(suites.contains(&"hosted-columnar-result-handle-auth"));
        assert!(suites.contains(&"hosted-vcs-protected-ref-write"));
        assert!(suites.contains(&"hosted-columnar-rest"));
        assert!(suites.contains(&"hosted-columnar-jsonrpc"));
        assert!(suites.contains(&"hosted-kv-read-only-write-denial"));
        assert!(suites.contains(&"hosted-document-read-only-write-denial"));
        assert!(suites.contains(&"hosted-kv-rest"));
        assert!(suites.contains(&"hosted-kv-jsonrpc"));
    }
}
