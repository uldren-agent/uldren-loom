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
use loom_substrate::versioning::load_current_revision_index;
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
                            ticket_id: "MX-202".to_string(),
                            order_key: "F".to_string(),
                        },
                        LaneTicket {
                            ticket_id: "MX-203".to_string(),
                            order_key: "V".to_string(),
                        },
                    ],
                    active_ticket_id: Some("MX-202"),
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
                ticket_id: "MX-204",
                placement: LaneTicketPlacement::First,
                updated_by: Some("agent:3"),
            },
        )
        .map_err(strerr)?;
        mcp.write_lanes_ticket_remove(
            "main",
            LaneTicketUpdateRequest {
                lane_id: "mcp",
                ticket_id: "MX-202",
                placement: LaneTicketPlacement::Last,
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
                    ticket_id: "MX-204",
                    placement: LaneTicketPlacement::Last,
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
            "{\"lane_id\":\"rest\",\"lane_key\":\"rest\",\"lane_kind\":\"assignment\",\"owner_principal\":\"agent:3\",\"lane_status\":\"ready\",\"ticket_ids\":[\"MX-302\",\"MX-303\"],\"active_ticket_id\":\"MX-302\",\"status_report\":\"ready\",\"reviewer_feedback\":\"\",\"updated_by\":\"agent:3\"}",
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
            "{\"lane_id\":\"rest\",\"ticket_id\":\"MX-304\",\"placement\":\"FIRST\",\"updated_by\":\"agent:3\"}",
            "lanes-rest-add",
        )
        .await?;
        let rest_removed = json_route(
            rest.clone(),
            "POST",
            "/lanes:ticket-remove",
            "{\"lane_id\":\"rest\",\"ticket_id\":\"MX-302\",\"updated_by\":\"agent:3\"}",
            "lanes-rest-remove",
        )
        .await?;
        expect_contains(&rest_removed, "\"lane_tickets\":[\"MX-304\"", "REST lanes remove")?;

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
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"lanes.create\",\"params\":{\"lane_id\":\"jsonrpc\",\"lane_key\":\"jsonrpc\",\"lane_kind\":\"assignment\",\"owner_principal\":\"agent:3\",\"lane_status\":\"ready\",\"ticket_ids\":[\"MX-402\",\"MX-403\"],\"active_ticket_id\":\"MX-402\",\"status_report\":\"ready\",\"reviewer_feedback\":\"\",\"updated_by\":\"agent:3\"}}",
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
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"lanes.ticket_add\",\"params\":{\"lane_id\":\"jsonrpc\",\"ticket_id\":\"MX-404\",\"placement\":\"FIRST\",\"updated_by\":\"agent:3\"}}",
            "lanes-jsonrpc-add",
        )
        .await?;
        let jsonrpc_removed = json_route(
            jsonrpc.clone(),
            "POST",
            "/jsonrpc",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"lanes.ticket_remove\",\"params\":{\"lane_id\":\"jsonrpc\",\"ticket_id\":\"MX-402\",\"updated_by\":\"agent:3\"}}",
            "lanes-jsonrpc-remove",
        )
        .await?;
        expect_contains(
            &jsonrpc_removed,
            "\"lane_tickets\":[\"MX-404\"",
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
    let index = kernel
        .read(&auth, |loom| {
            load_current_revision_index(loom, workspace, scope_id)
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
    let (first_ticket, second_ticket, _) = lane_conformance_ids(label)?;
    if lane.lane_status != "ready" {
        return Err(format!("{label} Lane status drifted: {}", lane.lane_status));
    }
    if lane.active_ticket_id.as_deref() != Some(first_ticket) {
        return Err(format!(
            "{label} Lane active_ticket_id drifted: {:?}",
            lane.active_ticket_id
        ));
    }
    if lane.lane_tickets.len() != 2
        || lane.lane_tickets[0].ticket_id != first_ticket
        || lane.lane_tickets[0].order_key != "F"
        || lane.lane_tickets[1].ticket_id != second_ticket
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
    let (_, second_ticket, inserted_ticket) = lane_conformance_ids(label)?;
    if lane.active_ticket_id.is_some() {
        return Err(format!(
            "{label} Lane active_ticket_id was not derived-only: {:?}",
            lane.active_ticket_id
        ));
    }
    if lane.lane_tickets.len() != 2
        || lane.lane_tickets[0].ticket_id != inserted_ticket
        || lane.lane_tickets[1].ticket_id != second_ticket
    {
        return Err(format!(
            "{label} Lane final membership drifted: {:?}",
            lane.lane_tickets
        ));
    }
    Ok(())
}

fn lane_conformance_ids(label: &str) -> Result<(&'static str, &'static str, &'static str), String> {
    match label {
        "local" => Ok(("MX-102", "MX-103", "MX-104")),
        "mcp" => Ok(("MX-202", "MX-203", "MX-204")),
        "rest" => Ok(("MX-302", "MX-303", "MX-304")),
        "jsonrpc" => Ok(("MX-402", "MX-403", "MX-404")),
        _ => Err(format!("unknown lane conformance label {label}")),
    }
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

#[cfg(test)]
mod mu6hdc_tests {
    use super::*;

    use loom_codec::Value as WireValue;
    use loom_hosted_core::generated_dispatch::{Dispatched, dispatch};
    use loom_hosted_core::remote::{RemoteAuthMode, RemoteServerConfig, RemoteTlsTrust};
    use loom_hosted_core::remote_http::RemoteHttpService;
    use loom_locator::ContextResolver;
    use loom_remote_client::{CallOptions, RemoteConnection, RemoteLoomClient, Transport};
    use loom_remote_protocol::discovery::{DiscoveryMode, DiscoveryRoutes};
    use loom_remote_protocol::generated_api::{
        Audit, Chat, Lifecycle, Refs, Sql, Store, Tickets, Workspaces,
    };
    use loom_remote_protocol::session::SessionAuth;
    use loom_substrate::lifecycle::{
        LifecycleDefinition, LifecycleStage, StandardLifecycleInput, StandardLifecycleKind,
        standard_lifecycle_definition,
    };

    #[derive(Clone)]
    struct InProcessRemoteTransport {
        service: Arc<RemoteHttpService>,
    }

    impl Transport for InProcessRemoteTransport {
        fn discover(
            &self,
            path: &str,
        ) -> impl std::future::Future<Output = Result<Vec<u8>, loom_core::LoomError>> + Send
        {
            let response = self.service.handle("GET", path, &[]);
            async move {
                if response.status == 200 {
                    Ok(response.body)
                } else {
                    Err(loom_core::LoomError::new(
                        Code::NotFound,
                        format!("discovery route returned {}", response.status),
                    ))
                }
            }
        }

        fn call(
            &self,
            request: Vec<u8>,
        ) -> impl std::future::Future<Output = Result<Vec<u8>, loom_core::LoomError>> + Send
        {
            let response = self.service.handle("POST", "/apps/loom/v1/call", &request);
            async move {
                if response.status == 200 {
                    Ok(response.body)
                } else {
                    Err(loom_core::LoomError::new(
                        Code::Internal,
                        format!("call route returned {}", response.status),
                    ))
                }
            }
        }

        fn open_stream(
            &self,
            request: Vec<u8>,
        ) -> impl std::future::Future<
            Output = Result<loom_remote_client::transport::FrameSource, loom_core::LoomError>,
        > + Send {
            let frames = self
                .service
                .open_stream("POST", "/apps/loom/v1/call", &request)
                .map(|mut stream| {
                    let mut frames = Vec::new();
                    while let Some(frame) = stream.next_frame() {
                        frames.push(frame);
                    }
                    frames
                })
                .unwrap_or_default();
            async move {
                Ok(loom_remote_client::transport::FrameSource::from_frames(
                    frames,
                ))
            }
        }

        fn open_session(
            &self,
            request: Vec<u8>,
        ) -> impl std::future::Future<Output = Result<Vec<u8>, loom_core::LoomError>> + Send
        {
            let response = self
                .service
                .handle("POST", "/apps/loom/v1/session", &request);
            async move {
                if response.status == 200 {
                    Ok(response.body)
                } else {
                    Err(loom_core::LoomError::new(
                        Code::Internal,
                        format!("session route returned {}", response.status),
                    ))
                }
            }
        }
    }

    fn block<T>(fut: impl std::future::Future<Output = T>) -> T {
        let mut fut = std::pin::pin!(fut);
        match fut
            .as_mut()
            .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
        {
            std::task::Poll::Ready(value) => value,
            std::task::Poll::Pending => panic!("generated conformance future returned pending"),
        }
    }

    fn create_client(
        tag: &str,
        workspace: &str,
    ) -> Result<
        (
            loom_client::LocalLoomClient,
            loom_client::types::LoomSession,
            PathBuf,
            WorkspaceId,
        ),
        String,
    > {
        let path = temp_path(tag);
        let store = FileStore::create_with_profile(&path, Algo::Blake3).map_err(strerr)?;
        let mut loom = Loom::new(store);
        let workspace_id = loom
            .registry_mut()
            .create(FacetKind::Vcs, Some(workspace), nid(60))
            .map_err(strerr)?;
        save_loom(&mut loom).map_err(strerr)?;
        drop(loom);
        let client = loom_client::LocalLoomClient::new(&path);
        let session = client.open().map_err(strerr)?;
        Ok((client, session, path, workspace_id))
    }

    fn remote_config() -> RemoteServerConfig {
        RemoteServerConfig {
            service_root: "https://host/apps/loom".to_string(),
            call_endpoint: "https://host/apps/loom/v1/call".to_string(),
            auth_modes: vec![RemoteAuthMode::Interactive, RemoteAuthMode::Principal],
            tls: vec![RemoteTlsTrust::System],
            discovery: DiscoveryRoutes {
                mode: DiscoveryMode::Default,
                service_root_path: "/apps/loom".to_string(),
                custom_path: None,
            },
            session_lease_ms: 60_000,
        }
    }

    fn create_remote_client(
        tag: &str,
        workspace: &str,
    ) -> Result<
        (
            RemoteLoomClient<InProcessRemoteTransport>,
            loom_client::types::LoomSession,
            PathBuf,
            WorkspaceId,
        ),
        String,
    > {
        let (seed_client, seed_session, path, workspace_id) = create_client(tag, workspace)?;
        assert!(seed_client.close(&seed_session));
        drop(seed_client);
        let (client, session) = remote_client_for_existing_store(&path)?;
        Ok((client, session, path, workspace_id))
    }

    fn remote_client_for_existing_store(
        path: &PathBuf,
    ) -> Result<
        (
            RemoteLoomClient<InProcessRemoteTransport>,
            loom_client::types::LoomSession,
        ),
        String,
    > {
        let runtime = Arc::new(
            loom_hosted_core::remote::RemoteRuntime::start(path.clone(), remote_config())
                .map_err(strerr)?,
        );
        let service = Arc::new(RemoteHttpService::new(runtime, "/apps/loom/v1/call"));
        let transport = InProcessRemoteTransport { service };
        let connection = block(RemoteConnection::connect(
            transport,
            "https://host/apps/loom",
            &ContextResolver::default(),
            DiscoveryMode::Default,
        ))
        .map_err(strerr)?;
        let client = RemoteLoomClient::new(connection);
        block(client.open_session(SessionAuth::Unauthenticated)).map_err(strerr)?;
        let session = block(<RemoteLoomClient<InProcessRemoteTransport> as Store>::open(
            &client,
        ))
        .map_err(strerr)?;
        Ok((client, session))
    }

    fn completion_digest() -> String {
        Digest::hash(Algo::Blake3, b"mu6hdc-complete").to_string()
    }

    fn custom_definition_bytes() -> Result<Vec<u8>, String> {
        let stages = vec![
            LifecycleStage::new("queued", "Queued").map_err(strerr)?,
            LifecycleStage::new("running", "Running").map_err(strerr)?,
        ];
        LifecycleDefinition::new("custom-flow", "2026-07-28", stages, "queued")
            .and_then(|definition| definition.encode())
            .map_err(strerr)
    }

    fn parse_json(raw: &str) -> Result<serde_json::Value, String> {
        serde_json::from_str(raw).map_err(strerr)
    }

    fn dispatch_text(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        interface: &str,
        method: &str,
        args: &[WireValue],
    ) -> Result<String, loom_core::LoomError> {
        match dispatch(client, session, interface, method, args)? {
            Dispatched::Unary(WireValue::Text(value)) => Ok(value),
            Dispatched::Unary(_) => Err(loom_core::LoomError::new(
                Code::Internal,
                "generated dispatch returned non-text lifecycle/refs result",
            )),
            Dispatched::Stream(_) => Err(loom_core::LoomError::new(
                Code::Internal,
                "generated dispatch returned a stream for lifecycle/refs result",
            )),
        }
    }

    fn standard_define_args(workspace: &str, version: &str) -> Vec<WireValue> {
        vec![
            WireValue::Null,
            WireValue::Text(workspace.to_string()),
            WireValue::Text("feature".to_string()),
            WireValue::Text(version.to_string()),
            WireValue::Text(completion_digest()),
        ]
    }

    fn remote_call_text_with_key(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        interface: &str,
        method: &str,
        args: Vec<WireValue>,
        key: &[u8],
    ) -> Result<String, loom_core::LoomError> {
        let value = block(client.call(
            interface,
            method,
            args,
            &CallOptions {
                idempotency_key: Some(key.to_vec()),
                ..CallOptions::default()
            },
        ))?;
        loom_remote_client::wire::from_wire::<String>(&value)
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ChatGeneratedConformanceSnapshot {
        writes: Vec<serde_json::Value>,
        reads: Vec<serde_json::Value>,
        audits: Vec<(String, Option<String>)>,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ChatGeneratedPublicationState {
        reads: Vec<serde_json::Value>,
        audits: Vec<(String, Option<String>)>,
        reference_root: Option<String>,
        retained_revisions: Vec<(String, usize)>,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct StableLoomError {
        code: Code,
        message: String,
        details: Vec<String>,
    }

    struct GeneratedPublicationObserver {
        attempts: AtomicU64,
        successes: AtomicU64,
        _guard: loom_client::local::GeneratedCandidatePublicationTestGuard,
    }

    impl GeneratedPublicationObserver {
        fn install(path: &std::path::Path) -> Arc<Self> {
            Arc::new_cyclic(|weak: &std::sync::Weak<Self>| {
                let installed = weak.clone();
                let guard =
                    loom_client::local::install_generated_candidate_publication_test_observer(
                        path.to_path_buf(),
                        Arc::new(move |event| {
                            if let Some(observer) = installed.upgrade() {
                                match event {
                                loom_client::local::GeneratedCandidatePublicationTestEvent::Attempt => {
                                    observer.attempts.fetch_add(1, Ordering::SeqCst);
                                }
                                loom_client::local::GeneratedCandidatePublicationTestEvent::Success => {
                                    observer.successes.fetch_add(1, Ordering::SeqCst);
                                }
                            }
                            }
                        }),
                    );
                Self {
                    attempts: AtomicU64::new(0),
                    successes: AtomicU64::new(0),
                    _guard: guard,
                }
            })
        }

        fn attempts(&self) -> u64 {
            self.attempts.load(Ordering::SeqCst)
        }

        fn successes(&self) -> u64 {
            self.successes.load(Ordering::SeqCst)
        }
    }

    struct StorePublicationFailure {
        attempts: AtomicU64,
        _guard: loom_store::StorePublicationFailureTestGuard,
    }

    impl StorePublicationFailure {
        fn install(path: &std::path::Path) -> Arc<Self> {
            Arc::new_cyclic(|weak: &std::sync::Weak<Self>| {
                let installed = weak.clone();
                let guard = loom_store::install_store_publication_failure_test_injector(
                    path.to_path_buf(),
                    Arc::new(move |boundary| {
                        if boundary
                            == loom_store::StorePublicationFailureTestBoundary::WorkflowOwnerStateCommit
                            && let Some(failure) = installed.upgrade()
                        {
                            failure.attempts.fetch_add(1, Ordering::SeqCst);
                            return Err(loom_core::LoomError::new(
                                Code::Io,
                                "injected store publication failure",
                            ));
                        }
                        Ok(())
                    }),
                );
                Self {
                    attempts: AtomicU64::new(0),
                    _guard: guard,
                }
            })
        }

        fn attempts(&self) -> u64 {
            self.attempts.load(Ordering::SeqCst)
        }
    }

    fn install_counting_store_publication_injector(
        path: &std::path::Path,
    ) -> (Arc<AtomicU64>, loom_store::StorePublicationFailureTestGuard) {
        let attempts = Arc::new(AtomicU64::new(0));
        let installed = Arc::clone(&attempts);
        let guard = loom_store::install_store_publication_failure_test_injector(
            path.to_path_buf(),
            Arc::new(move |_| {
                installed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
        );
        (attempts, guard)
    }

    fn register_chat_test_channel(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        workspace: &str,
        channel_id: String,
        handle: &str,
    ) {
        block(
            <loom_client::LocalLoomClient as Chat>::chat_create_channel_json(
                client,
                session.clone(),
                workspace.to_string(),
                "studio".to_string(),
                channel_id,
                handle.to_string(),
                "General".to_string(),
                None,
            ),
        )
        .expect("create channel");
    }

    fn stable_error(error: loom_core::LoomError) -> StableLoomError {
        StableLoomError {
            code: error.code,
            message: error.message,
            details: error
                .details
                .into_iter()
                .map(|detail| format!("{detail:?}"))
                .collect(),
        }
    }

    fn sql_exec_result_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        workspace: &str,
        sql: &str,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(<loom_client::LocalLoomClient as Sql>::sql_exec_result(
            client,
            session.clone(),
            workspace.to_string(),
            "main".to_string(),
            sql.to_string(),
        ))
    }

    fn sql_exec_result_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        workspace: &str,
        sql: &str,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Sql>::sql_exec_result(
                client,
                session.clone(),
                workspace.to_string(),
                "main".to_string(),
                sql.to_string(),
            ),
        )
    }

    fn close_remote_generated_session(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: loom_client::types::LoomSession,
    ) {
        block(<RemoteLoomClient<InProcessRemoteTransport> as Store>::close(client, session))
            .expect("remote close");
    }

    fn chat_entity_tag(json: &str) -> Result<String, String> {
        parse_json(json)?
            .get("entity_tag")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "chat result missing entity_tag".to_string())
    }

    fn chat_audit_events_from_json(raw: &str) -> Result<Vec<(String, Option<String>)>, String> {
        let value = parse_json(raw)?;
        Ok(value["records"]
            .as_array()
            .ok_or_else(|| "audit list records must be an array".to_string())?
            .iter()
            .filter_map(|record| {
                let action = record["action"].as_str()?;
                action.starts_with("chat.").then(|| {
                    Ok((
                        action.to_string(),
                        record["target"].as_str().map(str::to_string),
                    ))
                })
            })
            .collect::<Result<Vec<_>, String>>()?)
    }

    fn chat_audit_events_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
    ) -> Result<Vec<(String, Option<String>)>, String> {
        let raw = block(<loom_client::LocalLoomClient as Audit>::audit_list_json(
            client,
            session.clone(),
        ))
        .map_err(strerr)?;
        chat_audit_events_from_json(&raw)
    }

    fn chat_audit_events_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
    ) -> Result<Vec<(String, Option<String>)>, String> {
        let raw = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Audit>::audit_list_json(
                client,
                session.clone(),
            ),
        )
        .map_err(strerr)?;
        chat_audit_events_from_json(&raw)
    }

    fn chat_generated_publication_state_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        workspace: &str,
        chat_workspace_id: &str,
        channel: &str,
        retained_entities: &[String],
    ) -> Result<ChatGeneratedPublicationState, String> {
        let reads = chat_read_values_local(client, session, workspace, chat_workspace_id, channel)?;
        let audits = chat_audit_events_local(client, session)?;
        let (reference_root, retained_revisions) = client
            .with_session(session, |loom| {
                let workspace_id = loom
                    .registry()
                    .open(&loom_core::WsSelector::Name(workspace.to_string()))?;
                let scope_id = workspace_id.to_string();
                let retained =
                    match loom_substrate::versioning::load_optional_current_revision_index(
                        loom,
                        workspace_id,
                        &scope_id,
                    )? {
                        Some(index) => retained_entities
                            .iter()
                            .map(|entity| (entity.clone(), index.history(entity).len()))
                            .collect(),
                        None => retained_entities
                            .iter()
                            .map(|entity| (entity.clone(), 0))
                            .collect(),
                    };
                Ok::<_, loom_core::LoomError>((
                    loom.store().reference_root().map(|root| root.to_string()),
                    retained,
                ))
            })
            .map_err(strerr)?;
        Ok(ChatGeneratedPublicationState {
            reads,
            audits,
            reference_root,
            retained_revisions,
        })
    }

    fn chat_generated_publication_state_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        workspace: &str,
        chat_workspace_id: &str,
        channel: &str,
    ) -> Result<(Vec<serde_json::Value>, Vec<(String, Option<String>)>), String> {
        Ok((
            chat_read_values_remote(client, session, workspace, chat_workspace_id, channel)?,
            chat_audit_events_remote(client, session)?,
        ))
    }

    fn chat_read_values_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        workspace: &str,
        chat_workspace_id: &str,
        channel: &str,
    ) -> Result<Vec<serde_json::Value>, String> {
        Ok(vec![
            parse_json(
                &block(
                    <loom_client::LocalLoomClient as Chat>::chat_list_channels_json(
                        client,
                        session.clone(),
                        workspace.to_string(),
                        chat_workspace_id.to_string(),
                    ),
                )
                .map_err(strerr)?,
            )?,
            parse_json(
                &block(
                    <loom_client::LocalLoomClient as Chat>::chat_emoji_list_json(
                        client,
                        session.clone(),
                        workspace.to_string(),
                        chat_workspace_id.to_string(),
                    ),
                )
                .map_err(strerr)?,
            )?,
            parse_json(
                &block(<loom_client::LocalLoomClient as Chat>::chat_messages_json(
                    client,
                    session.clone(),
                    workspace.to_string(),
                    chat_workspace_id.to_string(),
                    channel.to_string(),
                ))
                .map_err(strerr)?,
            )?,
            parse_json(
                &block(<loom_client::LocalLoomClient as Chat>::chat_cursor_json(
                    client,
                    session.clone(),
                    workspace.to_string(),
                    chat_workspace_id.to_string(),
                    channel.to_string(),
                ))
                .map_err(strerr)?,
            )?,
            parse_json(
                &block(
                    <loom_client::LocalLoomClient as Chat>::chat_fetch_events_json(
                        client,
                        session.clone(),
                        workspace.to_string(),
                        chat_workspace_id.to_string(),
                        channel.to_string(),
                        1,
                        50,
                    ),
                )
                .map_err(strerr)?,
            )?,
        ])
    }

    fn chat_read_values_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        workspace: &str,
        chat_workspace_id: &str,
        channel: &str,
    ) -> Result<Vec<serde_json::Value>, String> {
        Ok(vec![
            parse_json(
                &block(
                    <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_list_channels_json(
                        client,
                        session.clone(),
                        workspace.to_string(),
                        chat_workspace_id.to_string(),
                    ),
                )
                .map_err(strerr)?,
            )?,
            parse_json(
                &block(
                    <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_emoji_list_json(
                        client,
                        session.clone(),
                        workspace.to_string(),
                        chat_workspace_id.to_string(),
                    ),
                )
                .map_err(strerr)?,
            )?,
            parse_json(
                &block(
                    <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_messages_json(
                        client,
                        session.clone(),
                        workspace.to_string(),
                        chat_workspace_id.to_string(),
                        channel.to_string(),
                    ),
                )
                .map_err(strerr)?,
            )?,
            parse_json(
                &block(
                    <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_cursor_json(
                        client,
                        session.clone(),
                        workspace.to_string(),
                        chat_workspace_id.to_string(),
                        channel.to_string(),
                    ),
                )
                .map_err(strerr)?,
            )?,
            parse_json(
                &block(
                    <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_fetch_events_json(
                        client,
                        session.clone(),
                        workspace.to_string(),
                        chat_workspace_id.to_string(),
                        channel.to_string(),
                        1,
                        50,
                    ),
                )
                .map_err(strerr)?,
            )?,
        ])
    }

    fn run_local_chat_generated_flow(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
    ) -> Result<ChatGeneratedConformanceSnapshot, String> {
        let workspace = "mu6hkg";
        let chat_workspace_id = "studio";
        let channel_id = WorkspaceId::from_bytes([71; 16]).to_string();
        let agent = WorkspaceId::from_bytes([72; 16]).to_string();
        let recipient = WorkspaceId::from_bytes([73; 16]).to_string();
        block(
            <loom_client::LocalLoomClient as Workspaces>::workspace_create(
                client,
                session.clone(),
                Some("mu6hkg-other".to_string()),
                None,
            ),
        )
        .map_err(strerr)?;

        let mut writes = Vec::new();
        let created = block(
            <loom_client::LocalLoomClient as Chat>::chat_create_channel_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.clone(),
                "general".to_string(),
                "General".to_string(),
                None,
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&created)?);
        let mut tag = chat_entity_tag(&created)?;

        let renamed = block(
            <loom_client::LocalLoomClient as Chat>::chat_rename_channel_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.clone(),
                "team".to_string(),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&renamed)?);

        let raw_post = block(
            <loom_client::LocalLoomClient as Chat>::chat_post_message_bytes_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "m-raw".to_string(),
                None,
                vec![0, 159, 255, b'a'],
                None,
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&raw_post)?);

        let post = block(
            <loom_client::LocalLoomClient as Chat>::chat_post_message_bytes_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "m1".to_string(),
                None,
                vec![b'h', 0xfe, b'i'],
                None,
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&post)?);
        tag = chat_entity_tag(&post)?;

        let edited = block(
            <loom_client::LocalLoomClient as Chat>::chat_edit_message_bytes_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "m1".to_string(),
                vec![b'e', 0xff, b'!'],
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&edited)?);
        tag = chat_entity_tag(&edited)?;

        let thread = block(
            <loom_client::LocalLoomClient as Chat>::chat_create_thread_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "t1".to_string(),
                "m1".to_string(),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&thread)?);
        tag = chat_entity_tag(&thread)?;

        let task = block(
            <loom_client::LocalLoomClient as Chat>::chat_create_task_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "task-1".to_string(),
                Some("m1".to_string()),
                "Investigate".to_string(),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&task)?);
        tag = chat_entity_tag(&task)?;

        let claimed = block(
            <loom_client::LocalLoomClient as Chat>::chat_claim_task_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "task-1".to_string(),
                "claim-1".to_string(),
                Some("lease-1".to_string()),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&claimed)?);
        tag = chat_entity_tag(&claimed)?;

        let completed = block(
            <loom_client::LocalLoomClient as Chat>::chat_complete_task_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "task-1".to_string(),
                "claim-1".to_string(),
                Some("m1".to_string()),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&completed)?);
        tag = chat_entity_tag(&completed)?;

        let invoked = block(
            <loom_client::LocalLoomClient as Chat>::chat_invoke_agent_bytes_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "inv-1".to_string(),
                agent.clone(),
                "[\"m1\"]".to_string(),
                vec![b'p', 0xf0, 0x28, 0x8c, 0x28],
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&invoked)?);
        tag = chat_entity_tag(&invoked)?;

        let replied = block(
            <loom_client::LocalLoomClient as Chat>::chat_agent_reply_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "inv-1".to_string(),
                "m1".to_string(),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&replied)?);
        tag = chat_entity_tag(&replied)?;

        let handoff = block(
            <loom_client::LocalLoomClient as Chat>::chat_request_handoff_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "handoff-1".to_string(),
                agent,
                Some(recipient),
                Some("owner review".to_string()),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&handoff)?);
        tag = chat_entity_tag(&handoff)?;

        let redacted = block(
            <loom_client::LocalLoomClient as Chat>::chat_redact_message_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "m-raw".to_string(),
                Some("test redaction".to_string()),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&redacted)?);
        tag = chat_entity_tag(&redacted)?;

        let emoji = block(
            <loom_client::LocalLoomClient as Chat>::chat_emoji_register_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "shipit".to_string(),
                None,
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&emoji)?);
        let duplicate_emoji = block(
            <loom_client::LocalLoomClient as Chat>::chat_emoji_register_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "shipit".to_string(),
                None,
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&duplicate_emoji)?);

        let added = block(
            <loom_client::LocalLoomClient as Chat>::chat_add_reaction_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "m1".to_string(),
                "shipit".to_string(),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&added)?);
        tag = chat_entity_tag(&added)?;

        let removed = block(
            <loom_client::LocalLoomClient as Chat>::chat_remove_reaction_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "m1".to_string(),
                "shipit".to_string(),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&removed)?);

        let cursor = block(
            <loom_client::LocalLoomClient as Chat>::chat_update_cursor_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                4,
                None,
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&cursor)?);
        let cursor_tag = chat_entity_tag(&cursor)?;
        let cursor_noop = block(
            <loom_client::LocalLoomClient as Chat>::chat_update_cursor_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                4,
                Some(cursor_tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&cursor_noop)?);

        let unregistered = block(
            <loom_client::LocalLoomClient as Chat>::chat_emoji_unregister_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "shipit".to_string(),
                None,
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&unregistered)?);

        let reads = chat_read_values_local(client, session, workspace, chat_workspace_id, "team")?;
        let isolated = parse_json(
            &block(
                <loom_client::LocalLoomClient as Chat>::chat_list_channels_json(
                    client,
                    session.clone(),
                    "mu6hkg-other".to_string(),
                    chat_workspace_id.to_string(),
                ),
            )
            .map_err(strerr)?,
        )?;
        if isolated != serde_json::json!([]) {
            return Err("chat state leaked into isolated workspace".to_string());
        }
        Ok(ChatGeneratedConformanceSnapshot {
            writes,
            reads,
            audits: chat_audit_events_local(client, session)?,
        })
    }

    fn run_remote_chat_generated_flow(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
    ) -> Result<ChatGeneratedConformanceSnapshot, String> {
        let workspace = "mu6hkg";
        let chat_workspace_id = "studio";
        let channel_id = WorkspaceId::from_bytes([71; 16]).to_string();
        let agent = WorkspaceId::from_bytes([72; 16]).to_string();
        let recipient = WorkspaceId::from_bytes([73; 16]).to_string();
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Workspaces>::workspace_create(
                client,
                session.clone(),
                Some("mu6hkg-other".to_string()),
                None,
            ),
        )
        .map_err(strerr)?;

        let mut writes = Vec::new();
        let created = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_create_channel_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.clone(),
                "general".to_string(),
                "General".to_string(),
                None,
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&created)?);
        let mut tag = chat_entity_tag(&created)?;
        let renamed = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_rename_channel_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                channel_id.clone(),
                "team".to_string(),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&renamed)?);
        let raw_post = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_post_message_bytes_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "m-raw".to_string(),
                None,
                vec![0, 159, 255, b'a'],
                None,
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&raw_post)?);
        let post = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_post_message_bytes_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "m1".to_string(),
                None,
                vec![b'h', 0xfe, b'i'],
                None,
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&post)?);
        tag = chat_entity_tag(&post)?;
        let edited = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_edit_message_bytes_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "m1".to_string(),
                vec![b'e', 0xff, b'!'],
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&edited)?);
        tag = chat_entity_tag(&edited)?;
        let thread = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_create_thread_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "t1".to_string(),
                "m1".to_string(),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&thread)?);
        tag = chat_entity_tag(&thread)?;
        let task = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_create_task_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "task-1".to_string(),
                Some("m1".to_string()),
                "Investigate".to_string(),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&task)?);
        tag = chat_entity_tag(&task)?;
        let claimed = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_claim_task_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "task-1".to_string(),
                "claim-1".to_string(),
                Some("lease-1".to_string()),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&claimed)?);
        tag = chat_entity_tag(&claimed)?;
        let completed = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_complete_task_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "task-1".to_string(),
                "claim-1".to_string(),
                Some("m1".to_string()),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&completed)?);
        tag = chat_entity_tag(&completed)?;
        let invoked = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_invoke_agent_bytes_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "inv-1".to_string(),
                agent.clone(),
                "[\"m1\"]".to_string(),
                vec![b'p', 0xf0, 0x28, 0x8c, 0x28],
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&invoked)?);
        tag = chat_entity_tag(&invoked)?;
        let replied = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_agent_reply_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "inv-1".to_string(),
                "m1".to_string(),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&replied)?);
        tag = chat_entity_tag(&replied)?;
        let handoff = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_request_handoff_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "handoff-1".to_string(),
                agent,
                Some(recipient),
                Some("owner review".to_string()),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&handoff)?);
        tag = chat_entity_tag(&handoff)?;
        let redacted = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_redact_message_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "m-raw".to_string(),
                Some("test redaction".to_string()),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&redacted)?);
        tag = chat_entity_tag(&redacted)?;
        let emoji = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_emoji_register_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "shipit".to_string(),
                None,
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&emoji)?);
        let duplicate_emoji = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_emoji_register_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "shipit".to_string(),
                None,
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&duplicate_emoji)?);
        let added = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_add_reaction_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "m1".to_string(),
                "shipit".to_string(),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&added)?);
        tag = chat_entity_tag(&added)?;
        let removed = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_remove_reaction_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                "m1".to_string(),
                "shipit".to_string(),
                Some(tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&removed)?);
        let cursor = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_update_cursor_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                4,
                None,
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&cursor)?);
        let cursor_tag = chat_entity_tag(&cursor)?;
        let cursor_noop = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_update_cursor_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "team".to_string(),
                4,
                Some(cursor_tag),
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&cursor_noop)?);
        let unregistered = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_emoji_unregister_json(
                client,
                session.clone(),
                workspace.to_string(),
                chat_workspace_id.to_string(),
                "shipit".to_string(),
                None,
            ),
        )
        .map_err(strerr)?;
        writes.push(parse_json(&unregistered)?);

        let reads = chat_read_values_remote(client, session, workspace, chat_workspace_id, "team")?;
        let isolated = parse_json(
            &block(
                <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_list_channels_json(
                    client,
                    session.clone(),
                    "mu6hkg-other".to_string(),
                    chat_workspace_id.to_string(),
                ),
            )
            .map_err(strerr)?,
        )?;
        if isolated != serde_json::json!([]) {
            return Err("remote chat state leaked into isolated workspace".to_string());
        }
        Ok(ChatGeneratedConformanceSnapshot {
            writes,
            reads,
            audits: chat_audit_events_remote(client, session)?,
        })
    }

    fn seed_refs_candidate(
        path: &PathBuf,
        workspace_name: &str,
        ticket_workspace_id: &str,
    ) -> Result<(), String> {
        let mut loom = open_loom_unlocked(path, None).map_err(strerr)?;
        let workspace = loom
            .registry()
            .open(&loom_core::WsSelector::Name(workspace_name.to_string()))
            .map_err(strerr)?;
        let candidate = loom_substrate::refs::UnresolvedReference::new(
            loom_substrate::refs::UnresolvedReferenceInput {
                candidate_id: "candidate-1".to_string(),
                source: loom_substrate::refs::ReferenceSource::new(
                    "tickets",
                    ticket_workspace_id,
                    "source-ticket",
                    "body",
                )
                .map_err(strerr)?,
                source_operation_id: "operation:candidate-1".to_string(),
                source_root: Digest::hash(Algo::Blake3, b"source-ticket"),
                alias_text: "!ticket:MX-1".to_string(),
                relation: "refers_to".to_string(),
                span_start: 0,
                span_end: 12,
                evidence: "!ticket:MX-1".to_string(),
                next_attempt_ms: 0,
            },
        )
        .map_err(strerr)?;
        loom_reference::enqueue(&mut loom, workspace, &candidate).map_err(strerr)?;
        save_loom(&mut loom).map_err(strerr)
    }

    fn create_ticket_target(
        client: &loom_client::LocalLoomClient,
        session: loom_client::types::LoomSession,
        workspace: &str,
        workspace_id: &str,
    ) -> Result<(), String> {
        block(
            <loom_client::LocalLoomClient as Tickets>::tickets_project_create_json(
                client,
                session.clone(),
                workspace.to_string(),
                workspace_id.to_string(),
                "matrix".to_string(),
                "MX".to_string(),
                "Matrix".to_string(),
                None,
            ),
        )
        .map_err(strerr)?;
        let created = block(
            <loom_client::LocalLoomClient as Tickets>::tickets_create_json(
                client,
                session,
                workspace.to_string(),
                workspace_id.to_string(),
                "matrix".to_string(),
                "task".to_string(),
                None,
                None,
                serde_json::json!({"status":"ready","title":"Reference target"}).to_string(),
                "[]".to_string(),
                None,
            ),
        )
        .map_err(strerr)?;
        if parse_json(&created)?["resource"]["primary_key"] != "MX-1" {
            return Err("created ticket did not receive MX-1 key".to_string());
        }
        Ok(())
    }

    #[test]
    fn mu6hkg_chat_generated_local_and_remote_success_conformance() {
        let _clock = loom_chat::set_test_now_ms(1_777_000_001);
        let (local, local_session, local_path, _) =
            create_client("mu6hkg-local-success", "mu6hkg").expect("local client");
        let local_snapshot =
            run_local_chat_generated_flow(&local, &local_session).expect("local flow");
        local.close(&local_session);

        let (remote, remote_session, remote_path, _) =
            create_remote_client("mu6hkg-remote-success", "mu6hkg").expect("remote client");
        let remote_snapshot =
            run_remote_chat_generated_flow(&remote, &remote_session).expect("remote flow");
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Store>::close(&remote, remote_session),
        )
        .expect("close remote session");

        assert_eq!(local_snapshot, remote_snapshot);
        let messages = &local_snapshot.reads[2];
        assert_eq!(messages["messages"][0]["message_id"], "m-raw");
        assert_eq!(messages["messages"][0]["redacted"], true);
        assert_eq!(
            messages["messages"][1]["body"],
            serde_json::json!([101, 255, 33])
        );
        assert_eq!(
            messages["agent_invocations"][0]["prompt"],
            serde_json::json!([112, 240, 40, 140, 40])
        );
        assert_eq!(local_snapshot.reads[4]["events"][0]["kind"], "operation");
        assert!(
            local_snapshot.reads[4]["events"][0]
                .get("target_entity_id")
                .is_none()
        );
        assert_eq!(
            local_snapshot.reads[4]["events"].as_array().unwrap().len(),
            13
        );
        assert_eq!(
            local_snapshot
                .audits
                .iter()
                .map(|(action, _)| action.as_str())
                .collect::<Vec<_>>(),
            vec![
                "chat.channel.create",
                "chat.channel.rename",
                "chat.agent.invoke",
                "chat.handoff.request",
                "chat.emoji.register",
                "chat.emoji.unregister",
            ]
        );

        let reopened_local = loom_client::LocalLoomClient::new(&local_path);
        let reopened_local_session = reopened_local.open().expect("reopen local");
        assert_eq!(
            chat_read_values_local(
                &reopened_local,
                &reopened_local_session,
                "mu6hkg",
                "studio",
                "team"
            )
            .expect("local reopened reads"),
            local_snapshot.reads
        );
        reopened_local.close(&reopened_local_session);

        let reopened_remote = loom_client::LocalLoomClient::new(&remote_path);
        let reopened_remote_session = reopened_remote.open().expect("reopen remote store");
        assert_eq!(
            chat_read_values_local(
                &reopened_remote,
                &reopened_remote_session,
                "mu6hkg",
                "studio",
                "team"
            )
            .expect("remote reopened reads"),
            remote_snapshot.reads
        );
        reopened_remote.close(&reopened_remote_session);
        fs::remove_file(local_path).ok();
        fs::remove_file(remote_path).ok();
    }

    #[test]
    fn mu6hkg_chat_generated_error_closure_and_failure_isolation_conformance() {
        let _clock = loom_chat::set_test_now_ms(1_777_000_002);
        let (local, local_session, local_path, _) =
            create_client("mu6hkg-local-errors", "mu6hkg").expect("local client");
        let (remote, remote_session, remote_path, _) =
            create_remote_client("mu6hkg-remote-errors", "mu6hkg").expect("remote client");
        let channel_id = WorkspaceId::from_bytes([81; 16]).to_string();

        let local_create = block(
            <loom_client::LocalLoomClient as Chat>::chat_create_channel_json(
                &local,
                local_session.clone(),
                "mu6hkg".to_string(),
                "studio".to_string(),
                channel_id.clone(),
                "general".to_string(),
                "General".to_string(),
                None,
            ),
        )
        .expect("local create");
        let remote_create = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_create_channel_json(
                &remote,
                remote_session.clone(),
                "mu6hkg".to_string(),
                "studio".to_string(),
                channel_id,
                "general".to_string(),
                "General".to_string(),
                None,
            ),
        )
        .expect("remote create");
        assert_eq!(
            parse_json(&local_create).unwrap(),
            parse_json(&remote_create).unwrap()
        );

        let local_post = block(
            <loom_client::LocalLoomClient as Chat>::chat_post_message_bytes_json(
                &local,
                local_session.clone(),
                "mu6hkg".to_string(),
                "studio".to_string(),
                "general".to_string(),
                "m1".to_string(),
                None,
                b"before".to_vec(),
                None,
            ),
        )
        .expect("local post");
        let remote_post = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_post_message_bytes_json(
                &remote,
                remote_session.clone(),
                "mu6hkg".to_string(),
                "studio".to_string(),
                "general".to_string(),
                "m1".to_string(),
                None,
                b"before".to_vec(),
                None,
            ),
        )
        .expect("remote post");
        let stale_local_tag = chat_entity_tag(&local_post).expect("local post tag");
        let stale_remote_tag = chat_entity_tag(&remote_post).expect("remote post tag");

        let local_edit = block(
            <loom_client::LocalLoomClient as Chat>::chat_edit_message_bytes_json(
                &local,
                local_session.clone(),
                "mu6hkg".to_string(),
                "studio".to_string(),
                "general".to_string(),
                "m1".to_string(),
                b"after".to_vec(),
                Some(stale_local_tag.clone()),
            ),
        )
        .expect("local edit");
        let remote_edit = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_edit_message_bytes_json(
                &remote,
                remote_session.clone(),
                "mu6hkg".to_string(),
                "studio".to_string(),
                "general".to_string(),
                "m1".to_string(),
                b"after".to_vec(),
                Some(stale_remote_tag.clone()),
            ),
        )
        .expect("remote edit");
        assert_eq!(
            parse_json(&local_edit).unwrap(),
            parse_json(&remote_edit).unwrap()
        );

        let local_before =
            chat_read_values_local(&local, &local_session, "mu6hkg", "studio", "general")
                .expect("local before stale");
        let remote_before =
            chat_read_values_remote(&remote, &remote_session, "mu6hkg", "studio", "general")
                .expect("remote before stale");
        let local_audit_before =
            chat_audit_events_local(&local, &local_session).expect("local audit before");
        let remote_audit_before =
            chat_audit_events_remote(&remote, &remote_session).expect("remote audit before");

        let local_stale = block(
            <loom_client::LocalLoomClient as Chat>::chat_edit_message_bytes_json(
                &local,
                local_session.clone(),
                "mu6hkg".to_string(),
                "studio".to_string(),
                "general".to_string(),
                "m1".to_string(),
                b"rejected".to_vec(),
                Some(stale_local_tag),
            ),
        )
        .expect_err("local stale edit");
        let remote_stale = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_edit_message_bytes_json(
                &remote,
                remote_session.clone(),
                "mu6hkg".to_string(),
                "studio".to_string(),
                "general".to_string(),
                "m1".to_string(),
                b"rejected".to_vec(),
                Some(stale_remote_tag),
            ),
        )
        .expect_err("remote stale edit");
        assert_eq!(stable_error(local_stale), stable_error(remote_stale));
        assert_eq!(
            chat_read_values_local(&local, &local_session, "mu6hkg", "studio", "general")
                .expect("local after stale"),
            local_before
        );
        assert_eq!(
            chat_read_values_remote(&remote, &remote_session, "mu6hkg", "studio", "general")
                .expect("remote after stale"),
            remote_before
        );
        assert_eq!(
            chat_audit_events_local(&local, &local_session).expect("local audit after stale"),
            local_audit_before
        );
        assert_eq!(
            chat_audit_events_remote(&remote, &remote_session).expect("remote audit after stale"),
            remote_audit_before
        );

        local.close(&local_session);
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Store>::close(
                &remote,
                remote_session.clone(),
            ),
        )
        .expect("close remote");
        let local_closed = block(<loom_client::LocalLoomClient as Chat>::chat_messages_json(
            &local,
            local_session,
            "bad/workspace".to_string(),
            "bad/chat".to_string(),
            "bad/channel".to_string(),
        ))
        .expect_err("local closed session wins");
        let remote_closed = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_messages_json(
                &remote,
                remote_session,
                "bad/workspace".to_string(),
                "bad/chat".to_string(),
                "bad/channel".to_string(),
            ),
        )
        .expect_err("remote closed session wins");
        assert_eq!(stable_error(local_closed), stable_error(remote_closed));

        let reopened_local = loom_client::LocalLoomClient::new(&local_path);
        let reopened_local_session = reopened_local.open().expect("reopen local");
        assert_eq!(
            chat_read_values_local(
                &reopened_local,
                &reopened_local_session,
                "mu6hkg",
                "studio",
                "general"
            )
            .expect("local reopened after stale"),
            local_before
        );
        reopened_local.close(&reopened_local_session);

        let reopened_remote = loom_client::LocalLoomClient::new(&remote_path);
        let reopened_remote_session = reopened_remote.open().expect("reopen remote");
        assert_eq!(
            chat_read_values_local(
                &reopened_remote,
                &reopened_remote_session,
                "mu6hkg",
                "studio",
                "general"
            )
            .expect("remote reopened after stale"),
            remote_before
        );
        reopened_remote.close(&reopened_remote_session);
        fs::remove_file(local_path).ok();
        fs::remove_file(remote_path).ok();
    }

    #[test]
    fn mu6hkg_chat_generated_single_publication_persists_chat_and_audit() {
        let _clock = loom_chat::set_test_now_ms(1_777_000_003);
        let channel_id = WorkspaceId::from_bytes([91; 16]).to_string();
        let retained_entities = vec![format!("chat:studio:channel:{channel_id}")];

        let (local, local_session, local_path, _) =
            create_client("mu6hkg-local-single-publication", "mu6hkg").expect("local client");
        let local_observer = GeneratedPublicationObserver::install(&local_path);
        block(
            <loom_client::LocalLoomClient as Chat>::chat_create_channel_json(
                &local,
                local_session.clone(),
                "mu6hkg".to_string(),
                "studio".to_string(),
                channel_id.clone(),
                "general".to_string(),
                "General".to_string(),
                None,
            ),
        )
        .expect("local create");
        assert_eq!(local_observer.attempts(), 1);
        assert_eq!(local_observer.successes(), 1);
        let local_live = chat_generated_publication_state_local(
            &local,
            &local_session,
            "mu6hkg",
            "studio",
            "general",
            &retained_entities,
        )
        .expect("local live state");
        assert_eq!(
            local_live
                .audits
                .iter()
                .map(|(action, _)| action.as_str())
                .collect::<Vec<_>>(),
            vec!["chat.channel.create"]
        );
        local.close(&local_session);
        let reopened_local = loom_client::LocalLoomClient::new(&local_path);
        let reopened_local_session = reopened_local.open().expect("reopen local");
        assert_eq!(
            chat_generated_publication_state_local(
                &reopened_local,
                &reopened_local_session,
                "mu6hkg",
                "studio",
                "general",
                &retained_entities,
            )
            .expect("local reopened state"),
            local_live
        );
        reopened_local.close(&reopened_local_session);

        let (remote, remote_session, remote_path, _) =
            create_remote_client("mu6hkg-remote-single-publication", "mu6hkg")
                .expect("remote client");
        let remote_observer = GeneratedPublicationObserver::install(&remote_path);
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_create_channel_json(
                &remote,
                remote_session.clone(),
                "mu6hkg".to_string(),
                "studio".to_string(),
                channel_id.clone(),
                "general".to_string(),
                "General".to_string(),
                None,
            ),
        )
        .expect("remote create");
        assert_eq!(remote_observer.attempts(), 1);
        assert_eq!(remote_observer.successes(), 1);
        let remote_live = chat_generated_publication_state_remote(
            &remote,
            &remote_session,
            "mu6hkg",
            "studio",
            "general",
        )
        .expect("remote live state");
        assert_eq!(
            remote_live
                .1
                .iter()
                .map(|(action, _)| action.as_str())
                .collect::<Vec<_>>(),
            vec!["chat.channel.create"]
        );
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Store>::close(&remote, remote_session),
        )
        .expect("close remote");
        let reopened_remote = loom_client::LocalLoomClient::new(&remote_path);
        let reopened_remote_session = reopened_remote.open().expect("reopen remote store");
        let remote_reopened = chat_generated_publication_state_local(
            &reopened_remote,
            &reopened_remote_session,
            "mu6hkg",
            "studio",
            "general",
            &retained_entities,
        )
        .expect("remote reopened state");
        assert_eq!(remote_reopened.reads, remote_live.0);
        assert_eq!(remote_reopened.audits, remote_live.1);
        reopened_remote.close(&reopened_remote_session);

        fs::remove_file(local_path).ok();
        fs::remove_file(remote_path).ok();
    }

    #[test]
    fn mu6hkg_chat_generated_publication_failure_isolates_live_and_reopened_state() {
        let _clock = loom_chat::set_test_now_ms(1_777_000_004);
        let channel_id = WorkspaceId::from_bytes([92; 16]).to_string();
        let retained_entities = vec![format!("chat:studio:channel:{channel_id}")];

        let (local, local_session, local_path, _) =
            create_client("mu6hkg-local-publication-failure", "mu6hkg").expect("local client");
        let local_create = block(
            <loom_client::LocalLoomClient as Chat>::chat_create_channel_json(
                &local,
                local_session.clone(),
                "mu6hkg".to_string(),
                "studio".to_string(),
                channel_id.clone(),
                "general".to_string(),
                "General".to_string(),
                None,
            ),
        )
        .expect("local baseline create");
        let local_tag = chat_entity_tag(&local_create).expect("local tag");
        let local_before = chat_generated_publication_state_local(
            &local,
            &local_session,
            "mu6hkg",
            "studio",
            "general",
            &retained_entities,
        )
        .expect("local before");
        let local_observer = GeneratedPublicationObserver::install(&local_path);
        let local_failure = StorePublicationFailure::install(&local_path);
        let local_error = stable_error(
            block(
                <loom_client::LocalLoomClient as Chat>::chat_rename_channel_json(
                    &local,
                    local_session.clone(),
                    "mu6hkg".to_string(),
                    "studio".to_string(),
                    channel_id.clone(),
                    "team".to_string(),
                    Some(local_tag),
                ),
            )
            .expect_err("local injected publication failure"),
        );
        assert_eq!(local_observer.attempts(), 1);
        assert_eq!(local_observer.successes(), 0);
        assert_eq!(local_failure.attempts(), 1);
        assert_eq!(
            chat_generated_publication_state_local(
                &local,
                &local_session,
                "mu6hkg",
                "studio",
                "general",
                &retained_entities,
            )
            .expect("local after"),
            local_before
        );
        local.close(&local_session);
        let reopened_local = loom_client::LocalLoomClient::new(&local_path);
        let reopened_local_session = reopened_local.open().expect("reopen local");
        assert_eq!(
            chat_generated_publication_state_local(
                &reopened_local,
                &reopened_local_session,
                "mu6hkg",
                "studio",
                "general",
                &retained_entities,
            )
            .expect("local reopened"),
            local_before
        );
        reopened_local.close(&reopened_local_session);

        let (remote, remote_session, remote_path, _) =
            create_remote_client("mu6hkg-remote-publication-failure", "mu6hkg")
                .expect("remote client");
        let remote_create = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_create_channel_json(
                &remote,
                remote_session.clone(),
                "mu6hkg".to_string(),
                "studio".to_string(),
                channel_id.clone(),
                "general".to_string(),
                "General".to_string(),
                None,
            ),
        )
        .expect("remote baseline create");
        let remote_tag = chat_entity_tag(&remote_create).expect("remote tag");
        let remote_before = chat_generated_publication_state_remote(
            &remote,
            &remote_session,
            "mu6hkg",
            "studio",
            "general",
        )
        .expect("remote before");
        let remote_observer = GeneratedPublicationObserver::install(&remote_path);
        let remote_failure = StorePublicationFailure::install(&remote_path);
        let remote_error = stable_error(
            block(
                <RemoteLoomClient<InProcessRemoteTransport> as Chat>::chat_rename_channel_json(
                    &remote,
                    remote_session.clone(),
                    "mu6hkg".to_string(),
                    "studio".to_string(),
                    channel_id.clone(),
                    "team".to_string(),
                    Some(remote_tag),
                ),
            )
            .expect_err("remote injected publication failure"),
        );
        assert_eq!(remote_error, local_error);
        assert_eq!(remote_observer.attempts(), 1);
        assert_eq!(remote_observer.successes(), 0);
        assert_eq!(remote_failure.attempts(), 1);
        assert_eq!(
            chat_generated_publication_state_remote(
                &remote,
                &remote_session,
                "mu6hkg",
                "studio",
                "general"
            )
            .expect("remote after"),
            remote_before
        );
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Store>::close(&remote, remote_session),
        )
        .expect("close remote");
        let reopened_remote = loom_client::LocalLoomClient::new(&remote_path);
        let reopened_remote_session = reopened_remote.open().expect("reopen remote store");
        let remote_reopened = chat_generated_publication_state_local(
            &reopened_remote,
            &reopened_remote_session,
            "mu6hkg",
            "studio",
            "general",
            &retained_entities,
        )
        .expect("remote reopened");
        assert_eq!(remote_reopened.reads, remote_before.0);
        assert_eq!(remote_reopened.audits, remote_before.1);
        assert_eq!(
            remote_reopened.retained_revisions,
            local_before.retained_revisions
        );
        reopened_remote.close(&reopened_remote_session);

        fs::remove_file(local_path).ok();
        fs::remove_file(remote_path).ok();
    }

    #[test]
    fn mu6hkg_publication_test_guards_cleanup_and_paths_remain_isolated() {
        let _clock = loom_chat::set_test_now_ms(1_777_000_005);
        let (left, left_session, left_path, _) =
            create_client("mu6hkg-left-guard-isolation", "mu6hkg").expect("left client");
        let (right, right_session, right_path, _) =
            create_client("mu6hkg-right-guard-isolation", "mu6hkg").expect("right client");

        {
            let observer = GeneratedPublicationObserver::install(&left_path);
            assert!(
                loom_client::local::generated_candidate_publication_test_observer_registered(
                    &left_path
                )
            );
            assert!(
                !loom_client::local::generated_candidate_publication_test_observer_registered(
                    &right_path
                )
            );
            register_chat_test_channel(
                &right,
                &right_session,
                "mu6hkg",
                WorkspaceId::from_bytes([93; 16]).to_string(),
                "right",
            );
            assert_eq!(observer.attempts(), 0);
            assert_eq!(observer.successes(), 0);
        }
        assert!(
            !loom_client::local::generated_candidate_publication_test_observer_registered(
                &left_path
            )
        );

        {
            let (attempts, guard) = install_counting_store_publication_injector(&left_path);
            assert!(loom_store::store_publication_failure_test_injector_registered(&left_path));
            assert!(!loom_store::store_publication_failure_test_injector_registered(&right_path));
            register_chat_test_channel(
                &right,
                &right_session,
                "mu6hkg",
                WorkspaceId::from_bytes([94; 16]).to_string(),
                "right-two",
            );
            assert_eq!(attempts.load(Ordering::SeqCst), 0);
            drop(guard);
        }
        assert!(!loom_store::store_publication_failure_test_injector_registered(&left_path));

        {
            let (attempts, _guard) = install_counting_store_publication_injector(&left_path);
            register_chat_test_channel(
                &left,
                &left_session,
                "mu6hkg",
                WorkspaceId::from_bytes([95; 16]).to_string(),
                "left",
            );
            assert_eq!(attempts.load(Ordering::SeqCst), 1);
        }

        left.close(&left_session);
        right.close(&right_session);
        fs::remove_file(left_path).ok();
        fs::remove_file(right_path).ok();
    }

    #[test]
    fn mu6hdc_local_lifecycle_semantics_and_persistence() {
        let (client, session, path, _) =
            create_client("mu6hdc-local-lifecycle", "mu6hdc").expect("client");

        let standard = block(
            <loom_client::LocalLoomClient as Lifecycle>::lifecycle_define_standard_json(
                &client,
                session.clone(),
                "mu6hdc".to_string(),
                "feature".to_string(),
                "1".to_string(),
                completion_digest(),
            ),
        )
        .expect("define standard lifecycle");
        let standard = parse_json(&standard).expect("standard json");
        assert_eq!(standard["definition_id"], "feature");
        assert_eq!(standard["initial_stage_id"], "ideate");

        let custom = block(
            <loom_client::LocalLoomClient as Lifecycle>::lifecycle_define_json(
                &client,
                session.clone(),
                "mu6hdc".to_string(),
                custom_definition_bytes().expect("custom definition bytes"),
            ),
        )
        .expect("define custom lifecycle");
        let custom = parse_json(&custom).expect("custom json");
        assert_eq!(custom["definition_id"], "custom-flow");
        assert_eq!(custom["initial_stage_id"], "queued");

        let instance = block(
            <loom_client::LocalLoomClient as Lifecycle>::lifecycle_instantiate_json(
                &client,
                session.clone(),
                "mu6hdc".to_string(),
                "feature-1".to_string(),
                "feature".to_string(),
                vec!["ticket:MX-1".to_string()],
            ),
        )
        .expect("instantiate lifecycle");
        let instance = parse_json(&instance).expect("instance json");
        assert_eq!(instance["current_stage_id"], "ideate");

        let transition = block(
            <loom_client::LocalLoomClient as Lifecycle>::lifecycle_transition_json(
                &client,
                session.clone(),
                "mu6hdc".to_string(),
                "feature-1".to_string(),
                "transition-1".to_string(),
                "draft".to_string(),
                None,
                r#"[{"gate_id":"enter-draft","passed":true}]"#.to_string(),
                None,
            ),
        )
        .expect("transition lifecycle");
        let transition = parse_json(&transition).expect("transition json");
        assert_eq!(transition["instance"]["current_stage_id"], "draft");
        assert_eq!(
            transition["operation_log"]["records"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        let duplicate_instance = block(
            <loom_client::LocalLoomClient as Lifecycle>::lifecycle_instantiate_json(
                &client,
                session.clone(),
                "mu6hdc".to_string(),
                "feature-1".to_string(),
                "feature".to_string(),
                Vec::new(),
            ),
        )
        .expect_err("duplicate instance is rejected");
        assert_eq!(duplicate_instance.code, Code::AlreadyExists);

        let duplicate_transition = block(
            <loom_client::LocalLoomClient as Lifecycle>::lifecycle_transition_json(
                &client,
                session.clone(),
                "mu6hdc".to_string(),
                "feature-1".to_string(),
                "transition-1".to_string(),
                "draft".to_string(),
                None,
                r#"[{"gate_id":"enter-draft","passed":true}]"#.to_string(),
                None,
            ),
        )
        .expect_err("repeated transition is rejected against current source semantics");
        assert_eq!(duplicate_transition.code, Code::InvalidArgument);

        let missing_gate_instance = block(
            <loom_client::LocalLoomClient as Lifecycle>::lifecycle_instantiate_json(
                &client,
                session.clone(),
                "mu6hdc".to_string(),
                "feature-gate".to_string(),
                "feature".to_string(),
                Vec::new(),
            ),
        )
        .expect("instantiate missing-gate case");
        assert_eq!(
            parse_json(&missing_gate_instance).unwrap()["current_stage_id"],
            "ideate"
        );
        let missing_gate = block(
            <loom_client::LocalLoomClient as Lifecycle>::lifecycle_transition_json(
                &client,
                session.clone(),
                "mu6hdc".to_string(),
                "feature-gate".to_string(),
                "transition-gate".to_string(),
                "draft".to_string(),
                None,
                "[]".to_string(),
                None,
            ),
        )
        .expect_err("missing required gate is rejected");
        assert_eq!(missing_gate.code, Code::Conflict);

        let malformed_json = block(
            <loom_client::LocalLoomClient as Lifecycle>::lifecycle_transition_json(
                &client,
                session.clone(),
                "mu6hdc".to_string(),
                "feature-gate".to_string(),
                "transition-malformed".to_string(),
                "draft".to_string(),
                None,
                "{".to_string(),
                None,
            ),
        )
        .expect_err("malformed gate json is rejected");
        assert_eq!(malformed_json.code, Code::InvalidArgument);

        let malformed_bytes = block(
            <loom_client::LocalLoomClient as Lifecycle>::lifecycle_define_json(
                &client,
                session.clone(),
                "mu6hdc".to_string(),
                vec![0xff],
            ),
        )
        .expect_err("malformed lifecycle bytes are rejected");
        assert_eq!(malformed_bytes.code, Code::CorruptObject);

        assert!(client.close(&session));
        let reopened = client.open().expect("reopen lifecycle store");
        let persisted_duplicate = block(
            <loom_client::LocalLoomClient as Lifecycle>::lifecycle_instantiate_json(
                &client,
                reopened.clone(),
                "mu6hdc".to_string(),
                "feature-1".to_string(),
                "feature".to_string(),
                Vec::new(),
            ),
        )
        .expect_err("persisted instance rejects duplicate after reopen");
        assert_eq!(persisted_duplicate.code, Code::AlreadyExists);
        assert!(client.close(&reopened));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn mu6hdc_generated_hosted_dispatch_matches_local_for_deterministic_results() {
        let (local, local_session, local_path, _) =
            create_client("mu6hdc-local-parity", "mu6hdc-parity").expect("local client");
        let (hosted, hosted_session, hosted_path, _) =
            create_client("mu6hdc-hosted-parity", "mu6hdc-parity").expect("hosted client");

        let local_standard = block(
            <loom_client::LocalLoomClient as Lifecycle>::lifecycle_define_standard_json(
                &local,
                local_session.clone(),
                "mu6hdc-parity".to_string(),
                "feature".to_string(),
                "1".to_string(),
                completion_digest(),
            ),
        )
        .expect("local standard");
        let hosted_standard = dispatch_text(
            &hosted,
            &hosted_session,
            "Lifecycle",
            "lifecycle_define_standard_json",
            &standard_define_args("mu6hdc-parity", "1"),
        )
        .expect("hosted standard");
        assert_eq!(
            parse_json(&local_standard).unwrap(),
            parse_json(&hosted_standard).unwrap()
        );

        let local_instance = block(
            <loom_client::LocalLoomClient as Lifecycle>::lifecycle_instantiate_json(
                &local,
                local_session.clone(),
                "mu6hdc-parity".to_string(),
                "inst-1".to_string(),
                "feature".to_string(),
                vec!["ticket:MX-1".to_string()],
            ),
        )
        .expect("local instantiate");
        let hosted_instance = dispatch_text(
            &hosted,
            &hosted_session,
            "Lifecycle",
            "lifecycle_instantiate_json",
            &[
                WireValue::Null,
                WireValue::Text("mu6hdc-parity".to_string()),
                WireValue::Text("inst-1".to_string()),
                WireValue::Text("feature".to_string()),
                WireValue::Array(vec![WireValue::Text("ticket:MX-1".to_string())]),
            ],
        )
        .expect("hosted instantiate");
        assert_eq!(
            parse_json(&local_instance).unwrap(),
            parse_json(&hosted_instance).unwrap()
        );

        let hosted_transition = dispatch_text(
            &hosted,
            &hosted_session,
            "Lifecycle",
            "lifecycle_transition_json",
            &[
                WireValue::Null,
                WireValue::Text("mu6hdc-parity".to_string()),
                WireValue::Text("inst-1".to_string()),
                WireValue::Text("transition-1".to_string()),
                WireValue::Text("draft".to_string()),
                WireValue::Null,
                WireValue::Text(r#"[{"gate_id":"enter-draft","passed":true}]"#.to_string()),
                WireValue::Null,
            ],
        )
        .expect("hosted transition");
        assert_eq!(
            parse_json(&hosted_transition).unwrap()["instance"]["current_stage_id"],
            "draft"
        );

        let bad_shape = match dispatch(
            &hosted,
            &hosted_session,
            "Lifecycle",
            "lifecycle_define_json",
            &[
                WireValue::Null,
                WireValue::Text("mu6hdc-parity".to_string()),
                WireValue::Text("not bytes".to_string()),
            ],
        ) {
            Ok(_) => panic!("hosted dispatch accepted malformed bytes shape"),
            Err(error) => error,
        };
        assert_eq!(bad_shape.code, Code::InvalidArgument);

        assert!(local.close(&local_session));
        assert!(hosted.close(&hosted_session));
        let _ = std::fs::remove_file(local_path);
        let _ = std::fs::remove_file(hosted_path);
    }

    #[test]
    fn mu6hdc_remote_client_round_trips_and_keyed_retries_are_exactly_once() {
        let (local, local_session, local_path, _) =
            create_client("mu6hdc-remote-local", "mu6hdc-remote").expect("local client");
        let (remote, remote_session, remote_path, _) =
            create_remote_client("mu6hdc-remote-hosted", "mu6hdc-remote").expect("remote client");

        let local_standard = block(
            <loom_client::LocalLoomClient as Lifecycle>::lifecycle_define_standard_json(
                &local,
                local_session.clone(),
                "mu6hdc-remote".to_string(),
                "feature".to_string(),
                "1".to_string(),
                completion_digest(),
            ),
        )
        .expect("local standard");
        let remote_standard = block(
            <RemoteLoomClient<InProcessRemoteTransport> as Lifecycle>::lifecycle_define_standard_json(
                &remote,
                remote_session.clone(),
                "mu6hdc-remote".to_string(),
                "feature".to_string(),
                "1".to_string(),
                completion_digest(),
            ),
        )
        .expect("remote standard");
        assert_eq!(
            parse_json(&local_standard).unwrap(),
            parse_json(&remote_standard).unwrap()
        );

        let keyed_args = vec![
            loom_remote_protocol::codec::ToValue::to_value(&remote_session),
            WireValue::Text("mu6hdc-remote".to_string()),
            WireValue::Text("keyed-inst".to_string()),
            WireValue::Text("feature".to_string()),
            WireValue::Array(vec![WireValue::Text("ticket:MX-1".to_string())]),
        ];
        let first = remote_call_text_with_key(
            &remote,
            "Lifecycle",
            "lifecycle_instantiate_json",
            keyed_args.clone(),
            b"lifecycle-key",
        )
        .expect("keyed lifecycle first");
        let replay = remote_call_text_with_key(
            &remote,
            "Lifecycle",
            "lifecycle_instantiate_json",
            keyed_args.clone(),
            b"lifecycle-key",
        )
        .expect("keyed lifecycle replay");
        assert_eq!(parse_json(&first).unwrap(), parse_json(&replay).unwrap());
        let duplicate_with_new_key = remote_call_text_with_key(
            &remote,
            "Lifecycle",
            "lifecycle_instantiate_json",
            keyed_args,
            b"lifecycle-key-new",
        )
        .expect_err("fresh key reapplies and observes existing definition");
        assert_eq!(duplicate_with_new_key.code, Code::AlreadyExists);
        let conflict = remote_call_text_with_key(
            &remote,
            "Lifecycle",
            "lifecycle_instantiate_json",
            vec![
                loom_remote_protocol::codec::ToValue::to_value(&remote_session),
                WireValue::Text("mu6hdc-remote".to_string()),
                WireValue::Text("keyed-inst".to_string()),
                WireValue::Text("feature".to_string()),
                WireValue::Array(Vec::new()),
            ],
            b"lifecycle-key",
        )
        .expect_err("same key with different lifecycle args conflicts");
        assert_eq!(conflict.code, Code::Conflict);

        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Store>::close(&remote, remote_session),
        )
        .expect("remote close");
        assert!(local.close(&local_session));
        let _ = std::fs::remove_file(local_path);
        let _ = std::fs::remove_file(remote_path);
    }

    #[test]
    fn mu_6h_l_d_sql_exec_result_direct_and_remote_results_reopen_and_close() {
        let workspace = "mu6hld-sql";
        let (local, local_session, local_path, _) =
            create_client("mu6hld-sql-local", workspace).expect("local sql client");
        let (remote, remote_session, remote_path, _) =
            create_remote_client("mu6hld-sql-remote", workspace).expect("remote sql client");

        let seed_sql = "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT); \
             INSERT INTO t VALUES (1, 'a'); \
             INSERT INTO t VALUES (2, 'b')";
        let local_seed =
            sql_exec_result_local(&local, &local_session, workspace, seed_sql).expect("local seed");
        let remote_seed = sql_exec_result_remote(&remote, &remote_session, workspace, seed_sql)
            .expect("remote seed");
        assert_eq!(local_seed, remote_seed);

        let select_sql = "SELECT id, v FROM t ORDER BY id";
        let local_before = sql_exec_result_local(&local, &local_session, workspace, select_sql)
            .expect("local select before");
        let remote_before = sql_exec_result_remote(&remote, &remote_session, workspace, select_sql)
            .expect("remote select before");
        assert_eq!(local_before, remote_before);
        assert_eq!(
            local_before,
            sql_exec_result_local(&local, &local_session, workspace, select_sql)
                .expect("local deterministic select")
        );
        assert_eq!(
            remote_before,
            sql_exec_result_remote(&remote, &remote_session, workspace, select_sql)
                .expect("remote deterministic select")
        );

        let rolled_back = "BEGIN; INSERT INTO t VALUES (3, 'rolled-back'); ROLLBACK";
        assert_eq!(
            sql_exec_result_local(&local, &local_session, workspace, rolled_back)
                .expect("local rollback"),
            sql_exec_result_remote(&remote, &remote_session, workspace, rolled_back)
                .expect("remote rollback")
        );
        assert_eq!(
            sql_exec_result_local(&local, &local_session, workspace, select_sql)
                .expect("local select after rollback"),
            local_before
        );
        assert_eq!(
            sql_exec_result_remote(&remote, &remote_session, workspace, select_sql)
                .expect("remote select after rollback"),
            remote_before
        );

        let commit_sql = "INSERT INTO t VALUES (3, 'c')";
        assert_eq!(
            sql_exec_result_local(&local, &local_session, workspace, commit_sql)
                .expect("local committed insert"),
            sql_exec_result_remote(&remote, &remote_session, workspace, commit_sql)
                .expect("remote committed insert")
        );
        let local_after = sql_exec_result_local(&local, &local_session, workspace, select_sql)
            .expect("local select after commit");
        let remote_after = sql_exec_result_remote(&remote, &remote_session, workspace, select_sql)
            .expect("remote select after commit");
        assert_eq!(local_after, remote_after);
        assert_ne!(local_after, local_before);

        assert!(local.close(&local_session));
        close_remote_generated_session(&remote, remote_session);
        drop(local);
        drop(remote);

        let reopened_local = loom_client::LocalLoomClient::new(&local_path);
        let reopened_local_session = reopened_local.open().expect("reopen local sql");
        assert_eq!(
            sql_exec_result_local(
                &reopened_local,
                &reopened_local_session,
                workspace,
                select_sql
            )
            .expect("reopened local select"),
            local_after
        );
        assert!(reopened_local.close(&reopened_local_session));
        drop(reopened_local);

        let (reopened_remote, reopened_remote_session) =
            remote_client_for_existing_store(&remote_path).expect("reopen remote sql");
        assert_eq!(
            sql_exec_result_remote(
                &reopened_remote,
                &reopened_remote_session,
                workspace,
                select_sql
            )
            .expect("reopened remote select"),
            remote_after
        );
        close_remote_generated_session(&reopened_remote, reopened_remote_session);

        let _ = std::fs::remove_file(local_path);
        let _ = std::fs::remove_file(remote_path);
    }

    #[test]
    fn mu_6h_l_d_sql_exec_result_stable_errors_and_dangling_transaction_isolation_match() {
        let workspace = "mu6hld-sql-error";
        let (local, local_session, local_path, _) =
            create_client("mu6hld-sql-error-local", workspace).expect("local sql client");
        let (remote, remote_session, remote_path, _) =
            create_remote_client("mu6hld-sql-error-remote", workspace).expect("remote sql client");

        let create_sql = "CREATE TABLE t (id INTEGER PRIMARY KEY)";
        assert_eq!(
            sql_exec_result_local(&local, &local_session, workspace, create_sql)
                .expect("local create"),
            sql_exec_result_remote(&remote, &remote_session, workspace, create_sql)
                .expect("remote create")
        );
        let before = sql_exec_result_local(&local, &local_session, workspace, "SELECT * FROM t")
            .expect("local before");
        assert_eq!(
            before,
            sql_exec_result_remote(&remote, &remote_session, workspace, "SELECT * FROM t")
                .expect("remote before")
        );

        let local_malformed = stable_error(
            sql_exec_result_local(&local, &local_session, workspace, "THIS IS NOT SQL")
                .expect_err("local malformed SQL"),
        );
        let remote_malformed = stable_error(
            sql_exec_result_remote(&remote, &remote_session, workspace, "THIS IS NOT SQL")
                .expect_err("remote malformed SQL"),
        );
        assert_eq!(local_malformed, remote_malformed);

        let dangling_sql = "BEGIN; INSERT INTO t VALUES (1)";
        let local_dangling = stable_error(
            sql_exec_result_local(&local, &local_session, workspace, dangling_sql)
                .expect_err("local dangling transaction"),
        );
        let remote_dangling = stable_error(
            sql_exec_result_remote(&remote, &remote_session, workspace, dangling_sql)
                .expect_err("remote dangling transaction"),
        );
        assert_eq!(local_dangling, remote_dangling);
        assert_eq!(local_dangling.code, Code::InvalidArgument);

        assert_eq!(
            sql_exec_result_local(&local, &local_session, workspace, "SELECT * FROM t")
                .expect("local after dangling"),
            before
        );
        assert_eq!(
            sql_exec_result_remote(&remote, &remote_session, workspace, "SELECT * FROM t")
                .expect("remote after dangling"),
            before
        );

        assert!(local.close(&local_session));
        close_remote_generated_session(&remote, remote_session);
        drop(local);
        drop(remote);

        let reopened_local = loom_client::LocalLoomClient::new(&local_path);
        let reopened_local_session = reopened_local.open().expect("reopen local sql");
        assert_eq!(
            sql_exec_result_local(
                &reopened_local,
                &reopened_local_session,
                workspace,
                "SELECT * FROM t"
            )
            .expect("reopened local after dangling"),
            before
        );
        assert!(reopened_local.close(&reopened_local_session));

        let (reopened_remote, reopened_remote_session) =
            remote_client_for_existing_store(&remote_path).expect("reopen remote sql");
        assert_eq!(
            sql_exec_result_remote(
                &reopened_remote,
                &reopened_remote_session,
                workspace,
                "SELECT * FROM t"
            )
            .expect("reopened remote after dangling"),
            before
        );
        close_remote_generated_session(&reopened_remote, reopened_remote_session);

        let _ = std::fs::remove_file(local_path);
        let _ = std::fs::remove_file(remote_path);
    }

    #[test]
    fn mu6hdc_refs_reconcile_generated_path_saves_and_reopens() {
        let (client, session, path, workspace_id) =
            create_client("mu6hdc-refs", "mu6hdc-refs").expect("refs client");
        let ticket_workspace_id = workspace_id.to_string();
        create_ticket_target(
            &client,
            session.clone(),
            "mu6hdc-refs",
            &ticket_workspace_id,
        )
        .expect("create ticket target");
        assert!(client.close(&session));

        seed_refs_candidate(&path, "mu6hdc-refs", &ticket_workspace_id).expect("save refs seed");

        let session = client.open().expect("reopen refs client");
        let zero = block(<loom_client::LocalLoomClient as Refs>::refs_reconcile_json(
            &client,
            session.clone(),
            "mu6hdc-refs".to_string(),
            0,
        ))
        .expect("zero refs limit");
        let zero = parse_json(&zero).expect("zero refs json");
        assert_eq!(zero["processed"], 0);
        assert_eq!(zero["pending"], 1);

        let reconciled = dispatch_text(
            &client,
            &session,
            "Refs",
            "refs_reconcile_json",
            &[
                WireValue::Null,
                WireValue::Text("mu6hdc-refs".to_string()),
                WireValue::Uint(1),
            ],
        )
        .expect("hosted dispatch refs reconcile");
        let reconciled = parse_json(&reconciled).expect("reconciled refs json");
        assert_eq!(reconciled["processed"], 1);
        assert_eq!(reconciled["pending"], 0);
        assert_eq!(reconciled["resolved"], 1);
        assert!(client.close(&session));

        let reopened = client.open().expect("reopen after refs reconcile");
        let after_reopen = block(<loom_client::LocalLoomClient as Refs>::refs_reconcile_json(
            &client,
            reopened.clone(),
            "mu6hdc-refs".to_string(),
            1,
        ))
        .expect("refs reconcile after reopen");
        let after_reopen = parse_json(&after_reopen).expect("after reopen refs json");
        assert_eq!(after_reopen["processed"], 0);
        assert_eq!(after_reopen["pending"], 0);
        assert_eq!(after_reopen["resolved"], 1);

        let bad_max = match dispatch(
            &client,
            &reopened,
            "Refs",
            "refs_reconcile_json",
            &[
                WireValue::Null,
                WireValue::Text("mu6hdc-refs".to_string()),
                WireValue::Text("not u64".to_string()),
            ],
        ) {
            Ok(_) => panic!("hosted dispatch accepted malformed max"),
            Err(error) => error,
        };
        assert_eq!(bad_max.code, Code::InvalidArgument);

        assert!(client.close(&reopened));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn mu6hdc_remote_refs_reconcile_audit_and_keyed_retry_are_exactly_once() {
        let (local, local_session, local_path, local_workspace) =
            create_client("mu6hdc-refs-local", "mu6hdc-refs-remote").expect("local refs");
        let local_workspace_id = local_workspace.to_string();
        create_ticket_target(
            &local,
            local_session.clone(),
            "mu6hdc-refs-remote",
            &local_workspace_id,
        )
        .expect("local ticket");
        assert!(local.close(&local_session));
        seed_refs_candidate(&local_path, "mu6hdc-refs-remote", &local_workspace_id)
            .expect("local candidate");
        let local_session = local.open().expect("local reopen");
        let local_reconciled = block(<loom_client::LocalLoomClient as Refs>::refs_reconcile_json(
            &local,
            local_session.clone(),
            "mu6hdc-refs-remote".to_string(),
            1,
        ))
        .expect("local refs reconcile");
        assert!(local.close(&local_session));

        let (seed, seed_session, remote_path, remote_workspace) =
            create_client("mu6hdc-refs-hosted", "mu6hdc-refs-remote").expect("remote seed");
        let remote_workspace_id = remote_workspace.to_string();
        create_ticket_target(
            &seed,
            seed_session.clone(),
            "mu6hdc-refs-remote",
            &remote_workspace_id,
        )
        .expect("remote ticket");
        assert!(seed.close(&seed_session));
        drop(seed);
        seed_refs_candidate(&remote_path, "mu6hdc-refs-remote", &remote_workspace_id)
            .expect("remote candidate");

        let (remote, remote_session) =
            remote_client_for_existing_store(&remote_path).expect("remote refs client");
        let refs_args = vec![
            loom_remote_protocol::codec::ToValue::to_value(&remote_session),
            WireValue::Text("mu6hdc-refs-remote".to_string()),
            WireValue::Uint(1),
        ];
        let remote_first = remote_call_text_with_key(
            &remote,
            "Refs",
            "refs_reconcile_json",
            refs_args.clone(),
            b"refs-key",
        )
        .expect("remote refs first");
        assert_eq!(
            parse_json(&local_reconciled).unwrap(),
            parse_json(&remote_first).unwrap()
        );
        let remote_replay = remote_call_text_with_key(
            &remote,
            "Refs",
            "refs_reconcile_json",
            refs_args.clone(),
            b"refs-key",
        )
        .expect("remote refs replay");
        assert_eq!(
            parse_json(&remote_first).unwrap(),
            parse_json(&remote_replay).unwrap()
        );
        let remote_new_key = remote_call_text_with_key(
            &remote,
            "Refs",
            "refs_reconcile_json",
            refs_args,
            b"refs-key-new",
        )
        .expect("remote refs new key");
        assert_eq!(parse_json(&remote_new_key).unwrap()["processed"], 0);
        let conflict = remote_call_text_with_key(
            &remote,
            "Refs",
            "refs_reconcile_json",
            vec![
                loom_remote_protocol::codec::ToValue::to_value(&remote_session),
                WireValue::Text("mu6hdc-refs-remote".to_string()),
                WireValue::Uint(0),
            ],
            b"refs-key",
        )
        .expect_err("same refs key with different args conflicts");
        assert_eq!(conflict.code, Code::Conflict);
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Store>::close(&remote, remote_session),
        )
        .expect("remote close");
        drop(remote);

        let audit = FileStore::open(&remote_path)
            .expect("open remote store for audit")
            .audit_records()
            .expect("audit records")
            .into_iter()
            .filter(|record| record.action == "refs.reconcile")
            .collect::<Vec<_>>();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].principal, Some(remote_workspace));
        let expected_target =
            format!("workspace={remote_workspace_id};processed=1;resolved=1;failed=0;pending=0");
        assert_eq!(audit[0].target.as_deref(), Some(expected_target.as_str()));

        let reopened = loom_client::LocalLoomClient::new(&remote_path);
        let reopened_session = reopened.open().expect("reopen remote store");
        let after_reopen = block(<loom_client::LocalLoomClient as Refs>::refs_reconcile_json(
            &reopened,
            reopened_session.clone(),
            "mu6hdc-refs-remote".to_string(),
            1,
        ))
        .expect("refs after reopen");
        assert_eq!(parse_json(&after_reopen).unwrap()["processed"], 0);
        assert_eq!(
            reopened
                .with_session(&reopened_session, |loom| loom.store().audit_records())
                .expect("reopened audit")
                .into_iter()
                .filter(|record| record.action == "refs.reconcile")
                .count(),
            1
        );
        assert!(reopened.close(&reopened_session));

        let _ = std::fs::remove_file(local_path);
        let _ = std::fs::remove_file(remote_path);
    }

    #[test]
    fn mu6hdc_custom_definition_bytes_are_canonical() {
        let bytes = custom_definition_bytes().expect("custom bytes");
        let decoded = LifecycleDefinition::decode(&bytes).expect("decode custom bytes");
        assert_eq!(decoded.definition_id, "custom-flow");
        assert_eq!(decoded.encode().expect("reencode custom bytes"), bytes);

        let standard = standard_lifecycle_definition(StandardLifecycleInput {
            kind: StandardLifecycleKind::Feature,
            version: "1".to_string(),
            completion_predicate_digest: Digest::hash(Algo::Blake3, b"mu6hdc-complete"),
        })
        .expect("standard definition");
        assert_eq!(standard.initial_stage_id, "ideate");
        assert!(standard.stage("draft").is_some());
    }
}

#[cfg(test)]
mod interchange_profiles_tests {
    use super::*;

    use std::io::{Cursor, Write};

    use loom_codec::Value as WireValue;
    use loom_hosted_core::remote::{RemoteAuthMode, RemoteServerConfig, RemoteTlsTrust};
    use loom_hosted_core::remote_http::RemoteHttpService;
    use loom_locator::ContextResolver;
    use loom_remote_client::{RemoteConnection, RemoteLoomClient, Transport};
    use loom_remote_protocol::discovery::{DiscoveryMode, DiscoveryRoutes};
    use loom_remote_protocol::generated_api::{
        Columnar, InferenceInstance, InterchangeProfiles, ServeConfig, Store, StoreAdmin,
        StudioMaintenance, Vector,
    };
    use loom_remote_protocol::session::SessionAuth;
    use zip::write::SimpleFileOptions;

    #[derive(Clone)]
    struct InProcessRemoteTransport {
        service: Arc<RemoteHttpService>,
    }

    impl Transport for InProcessRemoteTransport {
        fn discover(
            &self,
            path: &str,
        ) -> impl std::future::Future<Output = Result<Vec<u8>, loom_core::LoomError>> + Send
        {
            let response = self.service.handle("GET", path, &[]);
            async move {
                if response.status == 200 {
                    Ok(response.body)
                } else {
                    Err(loom_core::LoomError::new(
                        Code::NotFound,
                        format!("discovery route returned {}", response.status),
                    ))
                }
            }
        }

        fn call(
            &self,
            request: Vec<u8>,
        ) -> impl std::future::Future<Output = Result<Vec<u8>, loom_core::LoomError>> + Send
        {
            let response = self.service.handle("POST", "/apps/loom/v1/call", &request);
            async move {
                if response.status == 200 {
                    Ok(response.body)
                } else {
                    Err(loom_core::LoomError::new(
                        Code::Internal,
                        format!("call route returned {}", response.status),
                    ))
                }
            }
        }

        fn open_stream(
            &self,
            request: Vec<u8>,
        ) -> impl std::future::Future<
            Output = Result<loom_remote_client::transport::FrameSource, loom_core::LoomError>,
        > + Send {
            let frames = self
                .service
                .open_stream("POST", "/apps/loom/v1/call", &request)
                .map(|mut stream| {
                    let mut frames = Vec::new();
                    while let Some(frame) = stream.next_frame() {
                        frames.push(frame);
                    }
                    frames
                })
                .unwrap_or_default();
            async move {
                Ok(loom_remote_client::transport::FrameSource::from_frames(
                    frames,
                ))
            }
        }

        fn open_session(
            &self,
            request: Vec<u8>,
        ) -> impl std::future::Future<Output = Result<Vec<u8>, loom_core::LoomError>> + Send
        {
            let response = self
                .service
                .handle("POST", "/apps/loom/v1/session", &request);
            async move {
                if response.status == 200 {
                    Ok(response.body)
                } else {
                    Err(loom_core::LoomError::new(
                        Code::Internal,
                        format!("session route returned {}", response.status),
                    ))
                }
            }
        }
    }

    fn block<T>(fut: impl std::future::Future<Output = T>) -> T {
        let mut fut = std::pin::pin!(fut);
        match fut
            .as_mut()
            .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
        {
            std::task::Poll::Ready(value) => value,
            std::task::Poll::Pending => panic!("generated conformance future returned pending"),
        }
    }

    fn create_client(
        tag: &str,
        workspace: &str,
    ) -> Result<
        (
            loom_client::LocalLoomClient,
            loom_client::types::LoomSession,
            PathBuf,
        ),
        String,
    > {
        let path = temp_path(tag);
        let store = FileStore::create_with_profile(&path, Algo::Blake3).map_err(strerr)?;
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some(workspace), nid(60))
            .map_err(strerr)?;
        save_loom(&mut loom).map_err(strerr)?;
        drop(loom);
        let client = loom_client::LocalLoomClient::new(&path);
        let session = client.open().map_err(strerr)?;
        Ok((client, session, path))
    }

    fn remote_client_for_existing_store(
        path: &PathBuf,
    ) -> Result<
        (
            RemoteLoomClient<InProcessRemoteTransport>,
            loom_client::types::LoomSession,
        ),
        String,
    > {
        let runtime = Arc::new(
            loom_hosted_core::remote::RemoteRuntime::start(path.clone(), remote_config())
                .map_err(strerr)?,
        );
        let service = Arc::new(RemoteHttpService::new(runtime, "/apps/loom/v1/call"));
        let transport = InProcessRemoteTransport { service };
        let connection = block(RemoteConnection::connect(
            transport,
            "https://host/apps/loom",
            &ContextResolver::default(),
            DiscoveryMode::Default,
        ))
        .map_err(strerr)?;
        let client = RemoteLoomClient::new(connection);
        block(client.open_session(SessionAuth::Unauthenticated)).map_err(strerr)?;
        let session = block(<RemoteLoomClient<InProcessRemoteTransport> as Store>::open(
            &client,
        ))
        .map_err(strerr)?;
        Ok((client, session))
    }

    fn create_inference_client(
        tag: &str,
        workspace: &str,
    ) -> Result<
        (
            loom_client::LocalLoomClient,
            loom_client::types::LoomSession,
            PathBuf,
        ),
        String,
    > {
        let path = temp_path(tag);
        let store = FileStore::create_with_profile(&path, Algo::Blake3).map_err(strerr)?;
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(FacetKind::Inference, Some(workspace), nid(60))
            .map_err(strerr)?;
        save_loom(&mut loom).map_err(strerr)?;
        drop(loom);
        let client = loom_client::LocalLoomClient::new(&path);
        let session = client.open().map_err(strerr)?;
        Ok((client, session, path))
    }

    fn remote_config() -> RemoteServerConfig {
        RemoteServerConfig {
            service_root: "https://host/apps/loom".to_string(),
            call_endpoint: "https://host/apps/loom/v1/call".to_string(),
            auth_modes: vec![RemoteAuthMode::Interactive, RemoteAuthMode::Principal],
            tls: vec![RemoteTlsTrust::System],
            discovery: DiscoveryRoutes {
                mode: DiscoveryMode::Default,
                service_root_path: "/apps/loom".to_string(),
                custom_path: None,
            },
            session_lease_ms: 60_000,
        }
    }

    fn create_remote_client(
        tag: &str,
        workspace: &str,
    ) -> Result<
        (
            RemoteLoomClient<InProcessRemoteTransport>,
            loom_client::types::LoomSession,
            PathBuf,
        ),
        String,
    > {
        let (seed_client, seed_session, path) = create_client(tag, workspace)?;
        assert!(seed_client.close(&seed_session));
        drop(seed_client);
        let runtime = Arc::new(
            loom_hosted_core::remote::RemoteRuntime::start(path.clone(), remote_config())
                .map_err(strerr)?,
        );
        let service = Arc::new(RemoteHttpService::new(runtime, "/apps/loom/v1/call"));
        let transport = InProcessRemoteTransport { service };
        let connection = block(RemoteConnection::connect(
            transport,
            "https://host/apps/loom",
            &ContextResolver::default(),
            DiscoveryMode::Default,
        ))
        .map_err(strerr)?;
        let client = RemoteLoomClient::new(connection);
        block(client.open_session(SessionAuth::Unauthenticated)).map_err(strerr)?;
        let session = block(<RemoteLoomClient<InProcessRemoteTransport> as Store>::open(
            &client,
        ))
        .map_err(strerr)?;
        Ok((client, session, path))
    }

    fn create_remote_inference_client(
        tag: &str,
        workspace: &str,
    ) -> Result<
        (
            RemoteLoomClient<InProcessRemoteTransport>,
            loom_client::types::LoomSession,
            PathBuf,
        ),
        String,
    > {
        let (seed_client, seed_session, path) = create_inference_client(tag, workspace)?;
        assert!(seed_client.close(&seed_session));
        drop(seed_client);
        let runtime = Arc::new(
            loom_hosted_core::remote::RemoteRuntime::start(path.clone(), remote_config())
                .map_err(strerr)?,
        );
        let service = Arc::new(RemoteHttpService::new(runtime, "/apps/loom/v1/call"));
        let transport = InProcessRemoteTransport { service };
        let connection = block(RemoteConnection::connect(
            transport,
            "https://host/apps/loom",
            &ContextResolver::default(),
            DiscoveryMode::Default,
        ))
        .map_err(strerr)?;
        let client = RemoteLoomClient::new(connection);
        block(client.open_session(SessionAuth::Unauthenticated)).map_err(strerr)?;
        let session = block(<RemoteLoomClient<InProcessRemoteTransport> as Store>::open(
            &client,
        ))
        .map_err(strerr)?;
        Ok((client, session, path))
    }

    fn create_authenticated_non_admin_inference_client(
        tag: &str,
        workspace: &str,
    ) -> Result<
        (
            loom_client::LocalLoomClient,
            loom_client::types::LoomSession,
            PathBuf,
        ),
        String,
    > {
        let path = temp_path(tag);
        let store = FileStore::create_with_profile(&path, Algo::Blake3).map_err(strerr)?;
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(FacetKind::Inference, Some(workspace), nid(60))
            .map_err(strerr)?;
        let root = nid(62);
        let mut identity = IdentityStore::new(root);
        identity
            .set_passphrase(root, "root-pass", b"12345678")
            .map_err(strerr)?;
        loom.store()
            .save_identity_store(&identity)
            .map_err(strerr)?;
        save_loom(&mut loom).map_err(strerr)?;
        drop(loom);
        let client = loom_client::LocalLoomClient::new(&path);
        let session = client.open().map_err(strerr)?;
        client
            .authenticate_passphrase(&session, root, b"root-pass")
            .map_err(strerr)?;
        Ok((client, session, path))
    }

    fn create_authenticated_non_admin_remote_inference_client(
        tag: &str,
        workspace: &str,
    ) -> Result<
        (
            RemoteLoomClient<InProcessRemoteTransport>,
            loom_client::types::LoomSession,
            PathBuf,
        ),
        String,
    > {
        let (local, session, path) =
            create_authenticated_non_admin_inference_client(tag, workspace)?;
        assert!(local.close(&session));
        drop(local);
        let runtime = Arc::new(
            loom_hosted_core::remote::RemoteRuntime::start(path.clone(), remote_config())
                .map_err(strerr)?,
        );
        let service = Arc::new(RemoteHttpService::new(runtime, "/apps/loom/v1/call"));
        let transport = InProcessRemoteTransport { service };
        let connection = block(RemoteConnection::connect(
            transport,
            "https://host/apps/loom",
            &ContextResolver::default(),
            DiscoveryMode::Default,
        ))
        .map_err(strerr)?;
        let client = RemoteLoomClient::new(connection);
        block(client.open_session(SessionAuth::Passphrase {
            principal: *nid(62).as_bytes(),
            passphrase: b"root-pass".to_vec(),
        }))
        .map_err(strerr)?;
        let remote_session = block(<RemoteLoomClient<InProcessRemoteTransport> as Store>::open(
            &client,
        ))
        .map_err(strerr)?;
        Ok((client, remote_session, path))
    }

    fn connect_remote_path(
        path: PathBuf,
    ) -> Result<
        (
            RemoteLoomClient<InProcessRemoteTransport>,
            loom_client::types::LoomSession,
        ),
        String,
    > {
        let runtime = Arc::new(
            loom_hosted_core::remote::RemoteRuntime::start(path, remote_config())
                .map_err(strerr)?,
        );
        let service = Arc::new(RemoteHttpService::new(runtime, "/apps/loom/v1/call"));
        let transport = InProcessRemoteTransport { service };
        let connection = block(RemoteConnection::connect(
            transport,
            "https://host/apps/loom",
            &ContextResolver::default(),
            DiscoveryMode::Default,
        ))
        .map_err(strerr)?;
        let client = RemoteLoomClient::new(connection);
        block(client.open_session(SessionAuth::Unauthenticated)).map_err(strerr)?;
        let session = block(<RemoteLoomClient<InProcessRemoteTransport> as Store>::open(
            &client,
        ))
        .map_err(strerr)?;
        Ok((client, session))
    }

    fn create_seeded_remote_client(
        tag: &str,
        workspace: &str,
    ) -> Result<
        (
            RemoteLoomClient<InProcessRemoteTransport>,
            loom_client::types::LoomSession,
            PathBuf,
        ),
        String,
    > {
        let (seed_client, seed_session, path) = create_client(tag, workspace)?;
        assert!(seed_client.close(&seed_session));
        drop(seed_client);
        seed_meetings_revision_source(&path)?;
        let (client, session) = connect_remote_path(path.clone())?;
        Ok((client, session, path))
    }

    #[allow(clippy::too_many_arguments)]
    fn import_csv_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        source_scope: &str,
        payload: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as InterchangeProfiles>::import_table_csv(
                client,
                session.clone(),
                "main".to_string(),
                source_scope.to_string(),
                payload.to_vec(),
                "app".to_string(),
                "items".to_string(),
                "id:int,name:text,note:text".to_string(),
                "id".to_string(),
                "snapshot".to_string(),
                true,
                Some("tester".to_string()),
                Some("table import".to_string()),
                dry_run,
            ),
        )
    }

    fn import_csv_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        source_scope: &str,
        payload: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as InterchangeProfiles>::import_table_csv(
                client,
                session.clone(),
                "main".to_string(),
                source_scope.to_string(),
                payload.to_vec(),
                "app".to_string(),
                "items".to_string(),
                "id:int,name:text,note:text".to_string(),
                "id".to_string(),
                "snapshot".to_string(),
                true,
                Some("tester".to_string()),
                Some("table import".to_string()),
                dry_run,
            ),
        )
    }

    fn import_profile_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        method: &str,
        payload: &[u8],
        field_policy: &str,
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        match method {
            "import_redmine" => block(
                <loom_client::LocalLoomClient as InterchangeProfiles>::import_redmine(
                    client,
                    session.clone(),
                    "main".to_string(),
                    "studio".to_string(),
                    format!("memory://{method}.json"),
                    payload.to_vec(),
                    field_policy.to_string(),
                    dry_run,
                ),
            ),
            "import_asana" => block(
                <loom_client::LocalLoomClient as InterchangeProfiles>::import_asana(
                    client,
                    session.clone(),
                    "main".to_string(),
                    "studio".to_string(),
                    format!("memory://{method}.json"),
                    payload.to_vec(),
                    field_policy.to_string(),
                    dry_run,
                ),
            ),
            "import_jira" => block(
                <loom_client::LocalLoomClient as InterchangeProfiles>::import_jira(
                    client,
                    session.clone(),
                    "main".to_string(),
                    "studio".to_string(),
                    format!("memory://{method}.json"),
                    payload.to_vec(),
                    field_policy.to_string(),
                    dry_run,
                ),
            ),
            _ => unreachable!("known profile import method"),
        }
    }

    fn import_profile_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        method: &str,
        payload: &[u8],
        field_policy: &str,
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        match method {
            "import_redmine" => block(
                <RemoteLoomClient<InProcessRemoteTransport> as InterchangeProfiles>::import_redmine(
                    client,
                    session.clone(),
                    "main".to_string(),
                    "studio".to_string(),
                    format!("memory://{method}.json"),
                    payload.to_vec(),
                    field_policy.to_string(),
                    dry_run,
                ),
            ),
            "import_asana" => block(
                <RemoteLoomClient<InProcessRemoteTransport> as InterchangeProfiles>::import_asana(
                    client,
                    session.clone(),
                    "main".to_string(),
                    "studio".to_string(),
                    format!("memory://{method}.json"),
                    payload.to_vec(),
                    field_policy.to_string(),
                    dry_run,
                ),
            ),
            "import_jira" => block(
                <RemoteLoomClient<InProcessRemoteTransport> as InterchangeProfiles>::import_jira(
                    client,
                    session.clone(),
                    "main".to_string(),
                    "studio".to_string(),
                    format!("memory://{method}.json"),
                    payload.to_vec(),
                    field_policy.to_string(),
                    dry_run,
                ),
            ),
            _ => unreachable!("known profile import method"),
        }
    }

    fn import_confluence_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        payload: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as InterchangeProfiles>::import_confluence(
                client,
                session.clone(),
                "main".to_string(),
                "pages".to_string(),
                "memory://confluence.json".to_string(),
                payload.to_vec(),
                "wiki".to_string(),
                dry_run,
            ),
        )
    }

    fn import_confluence_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        payload: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as InterchangeProfiles>::import_confluence(
                client,
                session.clone(),
                "main".to_string(),
                "pages".to_string(),
                "memory://confluence.json".to_string(),
                payload.to_vec(),
                "wiki".to_string(),
                dry_run,
            ),
        )
    }

    fn import_slack_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        source_scope: &str,
        payload: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as InterchangeProfiles>::import_slack(
                client,
                session.clone(),
                "main".to_string(),
                "chat".to_string(),
                source_scope.to_string(),
                payload.to_vec(),
                dry_run,
            ),
        )
    }

    fn import_slack_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        source_scope: &str,
        payload: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as InterchangeProfiles>::import_slack(
                client,
                session.clone(),
                "main".to_string(),
                "chat".to_string(),
                source_scope.to_string(),
                payload.to_vec(),
                dry_run,
            ),
        )
    }

    fn import_drive_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        payload: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as InterchangeProfiles>::import_drive(
                client,
                session.clone(),
                "main".to_string(),
                "drive".to_string(),
                "memory://drive.zip".to_string(),
                payload.to_vec(),
                dry_run,
            ),
        )
    }

    fn import_drive_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        payload: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as InterchangeProfiles>::import_drive(
                client,
                session.clone(),
                "main".to_string(),
                "drive".to_string(),
                "memory://drive.zip".to_string(),
                payload.to_vec(),
                dry_run,
            ),
        )
    }

    fn import_markdown_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        payload: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as InterchangeProfiles>::import_markdown(
                client,
                session.clone(),
                "main".to_string(),
                "pages".to_string(),
                "memory://markdown.zip".to_string(),
                payload.to_vec(),
                "docs".to_string(),
                dry_run,
            ),
        )
    }

    fn import_markdown_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        payload: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as InterchangeProfiles>::import_markdown(
                client,
                session.clone(),
                "main".to_string(),
                "pages".to_string(),
                "memory://markdown.zip".to_string(),
                payload.to_vec(),
                "docs".to_string(),
                dry_run,
            ),
        )
    }

    fn import_notion_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        payload: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as InterchangeProfiles>::import_notion(
                client,
                session.clone(),
                "main".to_string(),
                "pages".to_string(),
                "memory://notion.json".to_string(),
                payload.to_vec(),
                "notion".to_string(),
                dry_run,
            ),
        )
    }

    fn import_notion_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        payload: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as InterchangeProfiles>::import_notion(
                client,
                session.clone(),
                "main".to_string(),
                "pages".to_string(),
                "memory://notion.json".to_string(),
                payload.to_vec(),
                "notion".to_string(),
                dry_run,
            ),
        )
    }

    fn import_arrow_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        name: &str,
        payload: &[u8],
        replace: bool,
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as Columnar>::columnar_import_arrow(
                client,
                session.clone(),
                "main".to_string(),
                name.to_string(),
                payload.to_vec(),
                2,
                replace,
                dry_run,
            ),
        )
    }

    fn import_arrow_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        name: &str,
        payload: &[u8],
        replace: bool,
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Columnar>::columnar_import_arrow(
                client,
                session.clone(),
                "main".to_string(),
                name.to_string(),
                payload.to_vec(),
                2,
                replace,
                dry_run,
            ),
        )
    }

    fn import_parquet_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        name: &str,
        payload: &[u8],
        replace: bool,
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as Columnar>::columnar_import_parquet(
                client,
                session.clone(),
                "main".to_string(),
                name.to_string(),
                payload.to_vec(),
                2,
                replace,
                dry_run,
            ),
        )
    }

    fn import_parquet_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        name: &str,
        payload: &[u8],
        replace: bool,
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Columnar>::columnar_import_parquet(
                client,
                session.clone(),
                "main".to_string(),
                name.to_string(),
                payload.to_vec(),
                2,
                replace,
                dry_run,
            ),
        )
    }

    struct VectorTextUpsertArgs<'a> {
        id: &'a str,
        vector: &'a [f32],
        metadata: &'a [u8],
        source_text: &'a [u8],
        model_id: &'a str,
        weights_digest: Option<&'a str>,
        create: bool,
        expected_token: Option<Vec<u8>>,
        expect_absent: bool,
    }

    fn vector_text_request(args: &VectorTextUpsertArgs<'_>) -> Vec<u8> {
        loom_wire::vector::text_upsert_request_to_cbor(&loom_wire::vector::TextUpsertRequest {
            workspace: "main".to_string(),
            name: "notes".to_string(),
            id: args.id.to_string(),
            vector: loom_wire::vector::floats_to_bytes(args.vector),
            metadata: args.metadata.to_vec(),
            source_text: args.source_text.to_vec(),
            model_id: Some(args.model_id.to_string()),
            weights_digest: args.weights_digest.map(str::to_string),
            create: args.create,
            metric: 1,
            expected_token: args.expected_token.clone(),
            expect_absent: args.expect_absent,
        })
    }

    fn vector_text_upsert_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        args: VectorTextUpsertArgs<'_>,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as Vector>::vector_text_upsert(
                client,
                session.clone(),
                vector_text_request(&args),
            ),
        )
    }

    fn vector_text_upsert_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        args: VectorTextUpsertArgs<'_>,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Vector>::vector_text_upsert(
                client,
                session.clone(),
                vector_text_request(&args),
            ),
        )
    }

    fn vector_workspace_configure_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        request_json: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as Vector>::vector_workspace_configure_json(
                client,
                session.clone(),
                "main".to_string(),
                request_json.to_string(),
            ),
        )
    }

    fn vector_workspace_configure_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        request_json: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Vector>::vector_workspace_configure_json(
                client,
                session.clone(),
                "main".to_string(),
                request_json.to_string(),
            ),
        )
    }

    fn studio_reindex_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        workspace: &str,
        profile: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as StudioMaintenance>::studio_reindex_json(
                client,
                session.clone(),
                workspace.to_string(),
                profile.to_string(),
            ),
        )
    }

    fn studio_reindex_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        workspace: &str,
        profile: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as StudioMaintenance>::studio_reindex_json(
                client,
                session.clone(),
                workspace.to_string(),
                profile.to_string(),
            ),
        )
    }

    fn studio_revisions_rebuild_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        workspace: &str,
        profile: &str,
        dry_run: bool,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as StudioMaintenance>::studio_revisions_rebuild_json(
                client,
                session.clone(),
                workspace.to_string(),
                profile.to_string(),
                dry_run,
            ),
        )
    }

    fn studio_revisions_rebuild_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        workspace: &str,
        profile: &str,
        dry_run: bool,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as StudioMaintenance>::studio_revisions_rebuild_json(
                client,
                session.clone(),
                workspace.to_string(),
                profile.to_string(),
                dry_run,
            ),
        )
    }

    fn store_bundle_import_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        bundle: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as StoreAdmin>::store_bundle_import(
                client,
                session.clone(),
                bundle.to_vec(),
                dry_run,
            ),
        )
    }

    fn store_bundle_import_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        bundle: &[u8],
        dry_run: bool,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as StoreAdmin>::store_bundle_import(
                client,
                session.clone(),
                bundle.to_vec(),
                dry_run,
            ),
        )
    }

    fn sample_bundle_bytes(tag: &str) -> Result<Vec<u8>, String> {
        let path = temp_path(tag);
        let store = FileStore::create_with_profile(&path, Algo::Blake3).map_err(strerr)?;
        let mut loom = Loom::new(store);
        let workspace = loom
            .registry_mut()
            .create(FacetKind::Vcs, Some("bundle-src"), nid(61))
            .map_err(strerr)?;
        loom.create_directory_reserved(workspace, "docs", true)
            .map_err(strerr)?;
        loom.write_file_reserved(workspace, "docs/readme.txt", b"bundle bytes", 0o100644)
            .map_err(strerr)?;
        loom.commit(workspace, "agent3", "bundle import fixture", 1)
            .map_err(strerr)?;
        let bundle = loom_core::bundle_export(&loom, workspace).map_err(strerr)?;
        let bytes = bundle.encode();
        let _ = std::fs::remove_file(path);
        Ok(bytes)
    }

    fn bundle_from_bytes(bytes: &[u8]) -> loom_core::Bundle {
        loom_core::Bundle::decode(bytes).expect("decode bundle fixture")
    }

    fn bundle_object_digests(bundle: &loom_core::Bundle) -> Vec<Digest> {
        bundle
            .objects
            .iter()
            .map(|object| Digest::hash(bundle.digest_algo, object))
            .collect()
    }

    fn unknown_session(
        session: &loom_client::types::LoomSession,
    ) -> loom_client::types::LoomSession {
        let mut handle = session.0.clone();
        handle.id = 999_999u64.to_be_bytes().to_vec();
        loom_client::types::LoomSession(handle)
    }

    fn create_authenticated_non_admin_client(
        tag: &str,
    ) -> Result<
        (
            loom_client::LocalLoomClient,
            loom_client::types::LoomSession,
            PathBuf,
        ),
        String,
    > {
        let path = temp_path(tag);
        let store = FileStore::create_with_profile(&path, Algo::Blake3).map_err(strerr)?;
        let mut loom = Loom::new(store);
        let root = nid(62);
        let mut identity = IdentityStore::new(root);
        identity
            .set_passphrase(root, "rootpw", b"root-salt-bytes")
            .map_err(strerr)?;
        loom.store()
            .save_identity_store(&identity)
            .map_err(strerr)?;
        save_loom(&mut loom).map_err(strerr)?;
        drop(loom);
        let client = loom_client::LocalLoomClient::new(&path);
        let session = client.open().map_err(strerr)?;
        client
            .authenticate_passphrase(&session, root, b"rootpw")
            .map_err(strerr)?;
        Ok((client, session, path))
    }

    fn create_authenticated_non_admin_remote_client(
        tag: &str,
    ) -> Result<
        (
            RemoteLoomClient<InProcessRemoteTransport>,
            loom_client::types::LoomSession,
            PathBuf,
        ),
        String,
    > {
        let (local, session, path) = create_authenticated_non_admin_client(tag)?;
        assert!(local.close(&session));
        drop(local);
        let runtime = Arc::new(
            loom_hosted_core::remote::RemoteRuntime::start(path.clone(), remote_config())
                .map_err(strerr)?,
        );
        let service = Arc::new(RemoteHttpService::new(runtime, "/apps/loom/v1/call"));
        let transport = InProcessRemoteTransport { service };
        let connection = block(RemoteConnection::connect(
            transport,
            "https://host/apps/loom",
            &ContextResolver::default(),
            DiscoveryMode::Default,
        ))
        .map_err(strerr)?;
        let client = RemoteLoomClient::new(connection);
        block(client.open_session(SessionAuth::Passphrase {
            principal: *nid(62).as_bytes(),
            passphrase: b"rootpw".to_vec(),
        }))
        .map_err(strerr)?;
        let remote_session = block(<RemoteLoomClient<InProcessRemoteTransport> as Store>::open(
            &client,
        ))
        .map_err(strerr)?;
        Ok((client, remote_session, path))
    }

    fn create_bundle_collision_client(
        tag: &str,
        name: &str,
        id: WorkspaceId,
    ) -> Result<
        (
            loom_client::LocalLoomClient,
            loom_client::types::LoomSession,
            PathBuf,
        ),
        String,
    > {
        let path = temp_path(tag);
        let store = FileStore::create_with_profile(&path, Algo::Blake3).map_err(strerr)?;
        let mut loom = Loom::new(store);
        loom.registry_mut()
            .create(FacetKind::Vcs, Some(name), id)
            .map_err(strerr)?;
        save_loom(&mut loom).map_err(strerr)?;
        drop(loom);
        let client = loom_client::LocalLoomClient::new(&path);
        let session = client.open().map_err(strerr)?;
        Ok((client, session, path))
    }

    fn create_bundle_collision_remote_client(
        tag: &str,
        name: &str,
        id: WorkspaceId,
    ) -> Result<
        (
            RemoteLoomClient<InProcessRemoteTransport>,
            loom_client::types::LoomSession,
            PathBuf,
        ),
        String,
    > {
        let (seed_client, seed_session, path) = create_bundle_collision_client(tag, name, id)?;
        assert!(seed_client.close(&seed_session));
        drop(seed_client);
        let (client, session) = connect_remote_path(path.clone())?;
        Ok((client, session, path))
    }

    fn missing_reachable_bundle(bytes: &[u8]) -> (Vec<u8>, Vec<Digest>) {
        let mut bundle = bundle_from_bytes(bytes);
        assert!(
            !bundle.objects.is_empty(),
            "bundle fixture must contain reachable objects"
        );
        bundle.objects.remove(0);
        let remaining = bundle_object_digests(&bundle);
        (bundle.encode(), remaining)
    }

    fn session_has_any_object(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        digests: &[Digest],
    ) -> Result<bool, String> {
        client
            .with_session(session, |loom| {
                for digest in digests {
                    if loom.has_object(*digest)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            })
            .map_err(strerr)
    }

    fn store_has_any_object(path: &PathBuf, digests: &[Digest]) -> Result<bool, String> {
        let loom = open_loom_unlocked(path, None).map_err(strerr)?;
        for digest in digests {
            if loom.has_object(*digest).map_err(strerr)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn assert_local_import_leaves_bundle_objects_absent(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        bundle: &[u8],
        absent: &[Digest],
        expected: Code,
    ) {
        assert!(
            !session_has_any_object(client, session, absent).expect("precondition object absence")
        );
        assert_eq!(
            store_bundle_import_local(client, session, bundle, false)
                .expect_err("bundle import must fail")
                .code,
            expected
        );
        assert!(
            !session_has_any_object(client, session, absent).expect("same-session object absence")
        );
    }

    fn assert_remote_collision_leaves_bundle_objects_absent(
        bundle: &[u8],
        absent: &[Digest],
        expected: Code,
        tag: &str,
        name: &str,
        id: WorkspaceId,
    ) {
        let (client, session, remote_path) =
            create_bundle_collision_remote_client(tag, name, id).expect("remote collision client");
        assert!(
            !store_has_any_object(&remote_path, absent)
                .expect("remote precondition object absence")
        );
        assert_eq!(
            store_bundle_import_remote(&client, &session, bundle, false)
                .expect_err("remote bundle import must fail")
                .code,
            expected
        );
        block(<RemoteLoomClient<InProcessRemoteTransport> as Store>::close(&client, session))
            .expect("remote close");
        drop(client);
        assert!(
            !store_has_any_object(&remote_path, absent).expect("remote reopened object absence")
        );
    }

    fn assert_remote_missing_reachable_leaves_bundle_objects_absent(
        bundle: &[u8],
        absent: &[Digest],
    ) {
        let (client, session, path) =
            create_remote_client("store-bundle-import-missing-remote", "main")
                .expect("remote missing client");
        assert!(!store_has_any_object(&path, absent).expect("remote missing precondition absence"));
        assert_eq!(
            store_bundle_import_remote(&client, &session, bundle, false)
                .expect_err("remote missing reachable object")
                .code,
            Code::NotFound
        );
        block(<RemoteLoomClient<InProcessRemoteTransport> as Store>::close(&client, session))
            .expect("remote close");
        drop(client);
        assert!(!store_has_any_object(&path, absent).expect("remote missing reopened absence"));
    }

    fn store_bundle_report(bytes: &[u8]) -> loom_wire::store_admin::StoreBundleImportResult {
        loom_wire::store_admin::store_bundle_import_result_from_cbor(bytes)
            .expect("store bundle import report cbor")
    }

    fn bundle_workspace_exists(path: &PathBuf) -> Result<bool, String> {
        let loom = open_loom_unlocked(path, None).map_err(strerr)?;
        match loom
            .registry()
            .open(&loom_core::WsSelector::Name("bundle-src".to_string()))
        {
            Ok(_) => Ok(true),
            Err(error) if error.code == Code::NotFound => Ok(false),
            Err(error) => Err(strerr(error)),
        }
    }

    fn imported_bundle_file(path: &PathBuf) -> Result<Vec<u8>, String> {
        let mut loom = open_loom_unlocked(path, None).map_err(strerr)?;
        let workspace = loom
            .registry()
            .open(&loom_core::WsSelector::Name("bundle-src".to_string()))
            .map_err(strerr)?;
        loom.checkout_branch(workspace, "main").map_err(strerr)?;
        loom.read_file_reserved(workspace, "docs/readme.txt")
            .map_err(strerr)
    }

    #[derive(Debug, PartialEq, Eq)]
    struct StudioReindexGeneratedReport {
        workspace: String,
        profile: String,
        job_path: String,
        state: String,
        source_digest: String,
        model_id: String,
        vector_records_indexed: u64,
        vector_records_deleted: u64,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct StudioRevisionGeneratedReport {
        workspace: String,
        scope_id: String,
        profile: String,
        index_present_before: bool,
        candidates: u64,
        inserted: u64,
        skipped_existing: u64,
        dry_run: bool,
    }

    fn studio_reindex_report(json: &str) -> StudioReindexGeneratedReport {
        let value: serde_json::Value = serde_json::from_str(json).expect("studio reindex json");
        StudioReindexGeneratedReport {
            workspace: json_text(&value, "workspace"),
            profile: json_text(&value, "profile"),
            job_path: json_text(&value, "job_path"),
            state: json_text(&value, "state"),
            source_digest: json_text(&value, "source_digest"),
            model_id: json_text(&value, "model_id"),
            vector_records_indexed: json_u64(&value, "vector_records_indexed"),
            vector_records_deleted: json_u64(&value, "vector_records_deleted"),
        }
    }

    fn studio_revision_report(json: &str) -> StudioRevisionGeneratedReport {
        let value: serde_json::Value = serde_json::from_str(json).expect("studio revision json");
        StudioRevisionGeneratedReport {
            workspace: json_text(&value, "workspace"),
            scope_id: json_text(&value, "scope_id"),
            profile: json_text(&value, "profile"),
            index_present_before: json_bool(&value, "index_present_before"),
            candidates: json_u64(&value, "candidates"),
            inserted: json_u64(&value, "inserted"),
            skipped_existing: json_u64(&value, "skipped_existing"),
            dry_run: json_bool(&value, "dry_run"),
        }
    }

    fn json_text(value: &serde_json::Value, field: &str) -> String {
        value[field]
            .as_str()
            .unwrap_or_else(|| panic!("{field} must be text"))
            .to_string()
    }

    fn json_u64(value: &serde_json::Value, field: &str) -> u64 {
        value[field]
            .as_u64()
            .unwrap_or_else(|| panic!("{field} must be u64"))
    }

    fn json_bool(value: &serde_json::Value, field: &str) -> bool {
        value[field]
            .as_bool()
            .unwrap_or_else(|| panic!("{field} must be bool"))
    }

    fn conformance_digest(label: &[u8]) -> Digest {
        Digest::hash(Algo::Blake3, label)
    }

    fn sample_studio_meetings_snapshot(workspace: WorkspaceId) -> MeetingsProfileSnapshot {
        let mut source = SourceRecord::new(SourceRecordInput {
            source_id: "src-1",
            source_system: "conformance",
            external_id: "meeting-1",
            source_digest: conformance_digest(b"studio-source"),
            observed_at_ms: 10,
            access_scope: "conformance",
            coverage: MeetingsCoverage::Complete,
        })
        .expect("source");
        source.sidecar_digest = Some(conformance_digest(b"studio-sidecar"));
        let mut meeting = MeetingRecord::new(MeetingRecordInput {
            meeting_id: "meet-1",
            title: "Studio maintenance conformance",
            current_source_digest: conformance_digest(b"studio-source"),
            created_at_ms: 10,
            updated_at_ms: 20,
        })
        .expect("meeting");
        meeting.source_refs = vec!["src-1".to_string()];
        MeetingsProfileSnapshot::new(
            workspace.to_string(),
            MeetingsProfileSnapshotParts {
                sources: vec![source],
                meetings: vec![meeting],
                spans: Vec::new(),
                annotations: Vec::new(),
                vocabulary_terms: Vec::new(),
                entity_merges: Vec::new(),
                promotions: Vec::new(),
                import_runs: Vec::new(),
                redactions: Vec::new(),
            },
        )
        .expect("snapshot")
    }

    fn seed_meetings_revision_source(path: &PathBuf) -> Result<WorkspaceId, String> {
        let mut loom = open_loom_unlocked(path, None).map_err(strerr)?;
        let workspace = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))
            .map_err(strerr)?;
        let snapshot = sample_studio_meetings_snapshot(workspace);
        loom.store()
            .control_set(
                &meetings_profile_key(&workspace.to_string()).map_err(strerr)?,
                snapshot.encode().map_err(strerr)?,
            )
            .map_err(strerr)?;
        save_loom(&mut loom).map_err(strerr)?;
        Ok(workspace)
    }

    fn studio_revision_index_len(path: &PathBuf, entity_id: &str) -> Result<usize, String> {
        let loom = open_loom_unlocked(path, None).map_err(strerr)?;
        let workspace = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))
            .map_err(strerr)?;
        let scope_id = workspace.to_string();
        let Some(index) = loom_substrate::versioning::load_optional_current_revision_index(
            &loom, workspace, &scope_id,
        )
        .map_err(strerr)?
        else {
            return Ok(0);
        };
        Ok(index.history(entity_id).len())
    }

    fn studio_reindex_job_exists(path: &PathBuf, job_path: &str) -> Result<bool, String> {
        let loom = open_loom_unlocked(path, None).map_err(strerr)?;
        let workspace = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))
            .map_err(strerr)?;
        Ok(loom.read_file_reserved(workspace, job_path).is_ok())
    }

    #[test]
    fn studio_reindex_generated_contract_persists_reports_and_remote_parity() {
        let (client, session, path) =
            create_client("studio-reindex-generated-local", "main").expect("client");
        let local_json =
            studio_reindex_local(&client, &session, "main", "all").expect("local reindex");
        let local = studio_reindex_report(&local_json);
        assert_eq!(local.workspace, nid(60).to_string());
        assert_eq!(local.profile, "all");
        assert_eq!(local.state, "no_engine");
        assert_eq!(local.model_id, "loom-built-in-embedding");
        assert_eq!(local.vector_records_indexed, 0);
        assert_eq!(local.vector_records_deleted, 0);
        assert_eq!(
            studio_reindex_local(&client, &session, "missing", "all")
                .expect_err("invalid workspace")
                .code,
            Code::NotFound
        );
        assert!(client.close(&session));
        assert!(
            studio_reindex_job_exists(&path, &local.job_path).expect("local persisted job"),
            "reindex must persist its deterministic projection job"
        );

        let (remote_client, remote_session, remote_path) =
            create_remote_client("studio-reindex-generated-remote", "main").expect("remote");
        let remote_json = studio_reindex_remote(&remote_client, &remote_session, "main", "all")
            .expect("remote reindex");
        assert_eq!(studio_reindex_report(&remote_json), local);
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Store>::close(
                &remote_client,
                remote_session,
            ),
        )
        .expect("remote close");
        drop(remote_client);
        assert!(
            studio_reindex_job_exists(&remote_path, &local.job_path).expect("remote persisted job"),
            "remote generated reindex must persist the same job"
        );
    }

    #[test]
    fn studio_revisions_rebuild_generated_contract_dry_run_write_parity_and_reopen() {
        let (client, session, path) =
            create_client("studio-revisions-generated-local", "main").expect("client");
        assert!(client.close(&session));
        seed_meetings_revision_source(&path).expect("seed local");
        let client = loom_client::LocalLoomClient::new(&path);
        let session = client.open().expect("reopen local");

        let dry = studio_revision_report(
            &studio_revisions_rebuild_local(&client, &session, "main", "meetings", true)
                .expect("local dry run"),
        );
        assert!(dry.dry_run);
        assert!(!dry.index_present_before);
        assert_eq!(dry.candidates, 1);
        assert_eq!(dry.inserted, 1);
        assert!(client.close(&session));
        assert_eq!(
            studio_revision_index_len(&path, "meeting:meet-1").expect("dry-run reopen"),
            0
        );

        let client = loom_client::LocalLoomClient::new(&path);
        let session = client.open().expect("reopen local for write");
        let written = studio_revision_report(
            &studio_revisions_rebuild_local(&client, &session, "main", "meetings", false)
                .expect("local write"),
        );
        assert!(!written.dry_run);
        assert_eq!(written.inserted, 1);
        assert!(client.close(&session));
        assert_eq!(
            studio_revision_index_len(&path, "meeting:meet-1").expect("write reopen"),
            1
        );

        let client = loom_client::LocalLoomClient::new(&path);
        let session = client.open().expect("reopen local for repeat");
        let repeated = studio_revision_report(
            &studio_revisions_rebuild_local(&client, &session, "main", "meetings", false)
                .expect("local repeated write"),
        );
        assert!(repeated.index_present_before);
        assert_eq!(repeated.inserted, 0);
        assert_eq!(repeated.skipped_existing, 1);
        assert_eq!(
            studio_revisions_rebuild_local(&client, &session, "main", "bogus", true)
                .expect_err("invalid profile")
                .code,
            Code::InvalidArgument
        );
        assert!(client.close(&session));

        let (local_client, local_session, local_path) =
            create_client("studio-revisions-parity-local", "main").expect("local parity");
        assert!(local_client.close(&local_session));
        seed_meetings_revision_source(&local_path).expect("seed local parity");
        let local_client = loom_client::LocalLoomClient::new(&local_path);
        let local_session = local_client.open().expect("local parity reopen");
        let local_dry = studio_revision_report(
            &studio_revisions_rebuild_local(
                &local_client,
                &local_session,
                "main",
                "meetings",
                true,
            )
            .expect("local parity dry"),
        );
        let local_write = studio_revision_report(
            &studio_revisions_rebuild_local(
                &local_client,
                &local_session,
                "main",
                "meetings",
                false,
            )
            .expect("local parity write"),
        );
        assert!(local_client.close(&local_session));

        let (remote_client, remote_session, remote_path) =
            create_seeded_remote_client("studio-revisions-parity-remote", "main")
                .expect("remote parity");
        let remote_dry = studio_revision_report(
            &studio_revisions_rebuild_remote(
                &remote_client,
                &remote_session,
                "main",
                "meetings",
                true,
            )
            .expect("remote parity dry"),
        );
        let remote_write = studio_revision_report(
            &studio_revisions_rebuild_remote(
                &remote_client,
                &remote_session,
                "main",
                "meetings",
                false,
            )
            .expect("remote parity write"),
        );
        assert_eq!(remote_dry, local_dry);
        assert_eq!(remote_write, local_write);
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Store>::close(
                &remote_client,
                remote_session,
            ),
        )
        .expect("remote close");
        drop(remote_client);
        assert_eq!(
            studio_revision_index_len(&remote_path, "meeting:meet-1").expect("remote reopen"),
            1
        );
    }

    #[test]
    fn store_bundle_import_generated_contract_validates_persists_and_matches_remote() {
        let bundle = sample_bundle_bytes("store-bundle-import-source").expect("bundle");
        let (client, session, path) =
            create_client("store-bundle-import-local", "main").expect("client");
        assert_eq!(
            store_bundle_import_local(&client, &session, b"not a loom bundle", true)
                .expect_err("malformed bundle")
                .code,
            Code::InvalidArgument
        );
        let dry = store_bundle_report(
            &store_bundle_import_local(&client, &session, &bundle, true).expect("dry run"),
        );
        assert!(dry.dry_run);
        assert_eq!(dry.workspace_id, nid(61).to_string());
        assert_eq!(dry.workspace_name, "bundle-src");
        assert_eq!(dry.facets, vec!["vcs".to_string()]);
        assert!(dry.objects_transferred > 0);
        assert_eq!(dry.objects_skipped, 0);
        assert_eq!(dry.new_tips.len(), 1);
        assert!(client.close(&session));
        assert!(!bundle_workspace_exists(&path).expect("dry-run durable state"));

        let client = loom_client::LocalLoomClient::new(&path);
        let session = client.open().expect("write session");
        let written = store_bundle_report(
            &store_bundle_import_local(&client, &session, &bundle, false).expect("write import"),
        );
        assert!(!written.dry_run);
        assert_eq!(written.objects_transferred, dry.objects_transferred);
        assert_eq!(written.objects_skipped, dry.objects_skipped);
        assert_eq!(written.new_tips, dry.new_tips);
        assert_eq!(
            store_bundle_import_local(&client, &session, &bundle, false)
                .expect_err("duplicate import")
                .code,
            Code::AlreadyExists
        );
        assert!(client.close(&session));
        assert_eq!(
            imported_bundle_file(&path).expect("reopen imported file"),
            b"bundle bytes"
        );

        let (local_client, local_session, local_path) =
            create_client("store-bundle-import-parity-local", "main").expect("local parity");
        let local_dry = store_bundle_report(
            &store_bundle_import_local(&local_client, &local_session, &bundle, true)
                .expect("local dry parity"),
        );
        let local_written = store_bundle_report(
            &store_bundle_import_local(&local_client, &local_session, &bundle, false)
                .expect("local write parity"),
        );
        assert!(local_client.close(&local_session));
        assert_eq!(
            imported_bundle_file(&local_path).expect("local parity reopen"),
            b"bundle bytes"
        );

        let (remote_client, remote_session, remote_path) =
            create_remote_client("store-bundle-import-parity-remote", "main")
                .expect("remote parity");
        assert_eq!(
            store_bundle_import_remote(&remote_client, &remote_session, b"not a loom bundle", true)
                .expect_err("remote malformed")
                .code,
            Code::InvalidArgument
        );
        let remote_dry = store_bundle_report(
            &store_bundle_import_remote(&remote_client, &remote_session, &bundle, true)
                .expect("remote dry parity"),
        );
        let remote_written = store_bundle_report(
            &store_bundle_import_remote(&remote_client, &remote_session, &bundle, false)
                .expect("remote write parity"),
        );
        assert_eq!(remote_dry, local_dry);
        assert_eq!(remote_written, local_written);
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Store>::close(
                &remote_client,
                remote_session,
            ),
        )
        .expect("remote close");
        drop(remote_client);
        assert_eq!(
            imported_bundle_file(&remote_path).expect("remote reopen"),
            b"bundle bytes"
        );
    }

    #[test]
    fn store_bundle_import_authenticates_before_parsing_bundle_bytes() {
        let (client, session, _path) =
            create_client("store-bundle-import-auth-order-local", "main").expect("client");
        let unknown = unknown_session(&session);
        assert_eq!(
            store_bundle_import_local(&client, &unknown, b"not a loom bundle", true)
                .expect_err("unknown local session")
                .code,
            Code::NotFound
        );
        assert!(client.close(&session));

        let (client, session, _path) =
            create_authenticated_non_admin_client("store-bundle-import-auth-order-non-admin")
                .expect("non-admin client");
        assert_eq!(
            store_bundle_import_local(&client, &session, b"not a loom bundle", true)
                .expect_err("local non-admin")
                .code,
            Code::PermissionDenied
        );
        assert!(client.close(&session));

        let (remote_client, remote_session, _path) =
            create_remote_client("store-bundle-import-auth-order-remote", "main")
                .expect("remote client");
        remote_client.bind_session(999_999u64.to_be_bytes().to_vec());
        assert_eq!(
            store_bundle_import_remote(&remote_client, &remote_session, b"not a loom bundle", true)
                .expect_err("unknown remote session")
                .code,
            Code::NotFound
        );
        drop(remote_client);

        let (remote_client, remote_session, _path) = create_authenticated_non_admin_remote_client(
            "store-bundle-import-auth-order-remote-non-admin",
        )
        .expect("remote non-admin client");
        assert_eq!(
            store_bundle_import_remote(&remote_client, &remote_session, b"not a loom bundle", true)
                .expect_err("remote non-admin")
                .code,
            Code::PermissionDenied
        );
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Store>::close(
                &remote_client,
                remote_session,
            ),
        )
        .expect("remote close");
    }

    #[test]
    fn store_bundle_import_preflight_prevents_orphan_objects_on_rejected_write() {
        let bundle_bytes =
            sample_bundle_bytes("store-bundle-import-preflight-source").expect("bundle fixture");
        let bundle = bundle_from_bytes(&bundle_bytes);
        let object_digests = bundle_object_digests(&bundle);

        let (client, session, path) = create_bundle_collision_client(
            "store-bundle-import-name-collision-local",
            "bundle-src",
            nid(63),
        )
        .expect("name collision client");
        assert_local_import_leaves_bundle_objects_absent(
            &client,
            &session,
            &bundle_bytes,
            &object_digests,
            Code::AlreadyExists,
        );
        assert!(client.close(&session));
        assert!(!store_has_any_object(&path, &object_digests).expect("name collision reopen"));
        assert_remote_collision_leaves_bundle_objects_absent(
            &bundle_bytes,
            &object_digests,
            Code::AlreadyExists,
            "store-bundle-import-name-collision-remote",
            "bundle-src",
            nid(63),
        );

        let (client, session, path) = create_bundle_collision_client(
            "store-bundle-import-id-collision-local",
            "other-bundle-id",
            nid(61),
        )
        .expect("id collision client");
        assert_local_import_leaves_bundle_objects_absent(
            &client,
            &session,
            &bundle_bytes,
            &object_digests,
            Code::AlreadyExists,
        );
        assert!(client.close(&session));
        assert!(!store_has_any_object(&path, &object_digests).expect("id collision reopen"));
        assert_remote_collision_leaves_bundle_objects_absent(
            &bundle_bytes,
            &object_digests,
            Code::AlreadyExists,
            "store-bundle-import-id-collision-remote",
            "other-bundle-id",
            nid(61),
        );

        let (missing_bundle, remaining_digests) = missing_reachable_bundle(&bundle_bytes);
        let (client, session, path) =
            create_client("store-bundle-import-missing-local", "main").expect("missing client");
        assert_local_import_leaves_bundle_objects_absent(
            &client,
            &session,
            &missing_bundle,
            &remaining_digests,
            Code::NotFound,
        );
        assert!(client.close(&session));
        assert!(!store_has_any_object(&path, &remaining_digests).expect("missing reopen"));
        assert_remote_missing_reachable_leaves_bundle_objects_absent(
            &missing_bundle,
            &remaining_digests,
        );
    }

    #[test]
    fn inference_instance_generated_contract_persists_validates_and_matches_remote() {
        let create_settings = r#"{"batch_size":"4","extra.owner":"agent3"}"#;
        let update_settings = r#"{"batch_size":"8","normalize":"false"}"#;

        let (client, session, path) =
            create_inference_client("inference-instance-generated-local", "main").expect("client");
        let created = inference_instance_create_local(&client, &session, "embed", create_settings)
            .expect("local create");
        assert_eq!(inference_instance_name(&created), "embed");
        assert_eq!(inference_instance_setting(&created, "batch_size"), "4");
        assert_eq!(inference_audit_sequence(&created), 0);
        assert_eq!(
            inference_instance_create_local(&client, &session, "embed", create_settings)
                .expect_err("duplicate create")
                .code,
            Code::AlreadyExists
        );
        assert_eq!(
            inference_instance_create_local(&client, &session, "bad name", create_settings)
                .expect_err("invalid descriptor")
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            inference_instance_create_local(
                &client,
                &session,
                "bad-settings",
                r#"{"bad.key":"1"}"#
            )
            .expect_err("invalid settings")
            .code,
            Code::InvalidArgument
        );
        assert_eq!(
            inference_instance_update_local(&client, &session, "missing", update_settings)
                .expect_err("missing update")
                .code,
            Code::NotFound
        );
        let updated = inference_instance_update_local(&client, &session, "embed", update_settings)
            .expect("local update");
        assert_eq!(inference_instance_setting(&updated, "batch_size"), "8");
        assert_eq!(
            inference_instance_setting(&updated, "effort"),
            "deterministic"
        );
        assert_eq!(inference_audit_sequence(&updated), 1);
        assert_eq!(
            inference_instance_delete_local(&client, &session, "missing")
                .expect_err("missing delete")
                .code,
            Code::NotFound
        );
        assert_eq!(
            inference_delete_name(
                &inference_instance_delete_local(&client, &session, "embed").expect("local delete")
            ),
            "embed"
        );
        let audit = client
            .with_session(&session, |loom| loom.store().audit_records())
            .expect("local audit records");
        assert_eq!(audit.len(), 3);
        assert_eq!(
            audit.iter().map(|record| record.seq).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(client.close(&session));
        assert!(
            inference_state(&path).find_instance("embed").is_none(),
            "delete persists after reopen"
        );

        let (local_client, local_session, local_path) =
            create_inference_client("inference-instance-parity-local", "main")
                .expect("local parity");
        let local_create = inference_instance_create_local(
            &local_client,
            &local_session,
            "embed",
            create_settings,
        )
        .expect("local parity create");
        let local_update = inference_instance_update_local(
            &local_client,
            &local_session,
            "embed",
            update_settings,
        )
        .expect("local parity update");
        let local_delete = inference_instance_delete_local(&local_client, &local_session, "embed")
            .expect("local parity delete");
        assert!(local_client.close(&local_session));
        assert!(
            inference_state(&local_path)
                .find_instance("embed")
                .is_none(),
            "local parity delete persists"
        );

        let (remote_client, remote_session, remote_path) =
            create_remote_inference_client("inference-instance-parity-remote", "main")
                .expect("remote parity");
        let remote_create = inference_instance_create_remote(
            &remote_client,
            &remote_session,
            "embed",
            create_settings,
        )
        .expect("remote parity create");
        assert_eq!(
            inference_instance_create_remote(
                &remote_client,
                &remote_session,
                "embed",
                create_settings,
            )
            .expect_err("remote duplicate")
            .code,
            Code::AlreadyExists
        );
        assert_eq!(
            inference_instance_update_remote(&remote_client, &remote_session, "missing", "{}")
                .expect_err("remote missing update")
                .code,
            Code::NotFound
        );
        let remote_update = inference_instance_update_remote(
            &remote_client,
            &remote_session,
            "embed",
            update_settings,
        )
        .expect("remote parity update");
        assert_eq!(
            inference_instance_delete_remote(&remote_client, &remote_session, "missing")
                .expect_err("remote missing delete")
                .code,
            Code::NotFound
        );
        let remote_delete =
            inference_instance_delete_remote(&remote_client, &remote_session, "embed")
                .expect("remote parity delete");
        assert_eq!(inference_audit_sequence(&local_create), 0);
        assert_eq!(inference_audit_sequence(&local_update), 1);
        assert_eq!(inference_audit_sequence(&local_delete), 2);
        assert_eq!(inference_audit_sequence(&remote_create), 0);
        assert_eq!(inference_audit_sequence(&remote_update), 1);
        assert_eq!(inference_audit_sequence(&remote_delete), 2);
        assert_eq!(remote_create, local_create);
        assert_eq!(remote_update, local_update);
        assert_eq!(remote_delete, local_delete);
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Store>::close(
                &remote_client,
                remote_session,
            ),
        )
        .expect("remote close");
        drop(remote_client);
        assert!(
            inference_state(&remote_path)
                .find_instance("embed")
                .is_none(),
            "remote delete persists after reopen"
        );
    }

    #[test]
    fn inference_instance_generated_contract_authenticates_before_payload_validation() {
        let (client, session, _path) =
            create_inference_client("inference-instance-auth-order-local", "main")
                .expect("local inference client");
        let unknown = unknown_session(&session);
        assert_eq!(
            inference_instance_create_local(&client, &unknown, "bad name", r#"{"bad.key":"1"}"#)
                .expect_err("unknown local create")
                .code,
            Code::NotFound
        );
        assert_eq!(
            inference_instance_update_local(&client, &unknown, "bad name", r#"{"bad.key":"1"}"#)
                .expect_err("unknown local update")
                .code,
            Code::NotFound
        );
        assert_eq!(
            inference_instance_delete_local(&client, &unknown, "bad name")
                .expect_err("unknown local delete")
                .code,
            Code::NotFound
        );
        assert!(client.close(&session));

        let (client, session, path) = create_authenticated_non_admin_inference_client(
            "inference-instance-auth-denied-local",
            "main",
        )
        .expect("local non-admin");
        assert_eq!(
            inference_instance_create_local(&client, &session, "bad name", r#"{"bad.key":"1"}"#)
                .expect_err("denied local create")
                .code,
            Code::PermissionDenied
        );
        assert_eq!(
            inference_instance_update_local(&client, &session, "bad name", r#"{"bad.key":"1"}"#)
                .expect_err("denied local update")
                .code,
            Code::PermissionDenied
        );
        assert_eq!(
            inference_instance_delete_local(&client, &session, "bad name")
                .expect_err("denied local delete")
                .code,
            Code::PermissionDenied
        );
        assert!(client.close(&session));
        assert!(
            inference_state(&path).find_instance("bad name").is_none(),
            "denied local request created no state"
        );
        assert!(
            FileStore::open(&path)
                .expect("open local denied store")
                .audit_records()
                .expect("local denied audit")
                .is_empty()
        );

        let (remote_client, remote_session, _path) =
            create_remote_inference_client("inference-instance-auth-order-remote", "main")
                .expect("remote inference client");
        remote_client.bind_session(999_999u64.to_be_bytes().to_vec());
        assert_eq!(
            inference_instance_create_remote(
                &remote_client,
                &remote_session,
                "bad name",
                r#"{"bad.key":"1"}"#
            )
            .expect_err("unknown remote create")
            .code,
            Code::NotFound
        );
        assert_eq!(
            inference_instance_update_remote(
                &remote_client,
                &remote_session,
                "bad name",
                r#"{"bad.key":"1"}"#
            )
            .expect_err("unknown remote update")
            .code,
            Code::NotFound
        );
        assert_eq!(
            inference_instance_delete_remote(&remote_client, &remote_session, "bad name")
                .expect_err("unknown remote delete")
                .code,
            Code::NotFound
        );
        drop(remote_client);

        let (remote_client, remote_session, remote_path) =
            create_authenticated_non_admin_remote_inference_client(
                "inference-instance-auth-denied-remote",
                "main",
            )
            .expect("remote non-admin");
        assert_eq!(
            inference_instance_create_remote(
                &remote_client,
                &remote_session,
                "bad name",
                r#"{"bad.key":"1"}"#
            )
            .expect_err("denied remote create")
            .code,
            Code::PermissionDenied
        );
        assert_eq!(
            inference_instance_update_remote(
                &remote_client,
                &remote_session,
                "bad name",
                r#"{"bad.key":"1"}"#
            )
            .expect_err("denied remote update")
            .code,
            Code::PermissionDenied
        );
        assert_eq!(
            inference_instance_delete_remote(&remote_client, &remote_session, "bad name")
                .expect_err("denied remote delete")
                .code,
            Code::PermissionDenied
        );
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Store>::close(
                &remote_client,
                remote_session,
            ),
        )
        .expect("remote close");
        drop(remote_client);
        assert!(
            inference_state(&remote_path)
                .find_instance("bad name")
                .is_none(),
            "denied remote request created no state"
        );
        assert!(
            FileStore::open(&remote_path)
                .expect("open remote denied store")
                .audit_records()
                .expect("remote denied audit")
                .is_empty()
        );
    }

    fn generated_report(bytes: &[u8]) -> loom_interchange::ImportReport {
        loom_interchange::ImportReport::decode(bytes).expect("decode generated import report")
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct ColumnarGeneratedImportReport {
        format: String,
        columns: Vec<(String, u64)>,
        rows: u64,
        segment_count: u64,
        target_segment_rows: u64,
        bytes_in: u64,
        replaced: bool,
        dry_run: bool,
    }

    fn columnar_generated_import_report(bytes: &[u8]) -> ColumnarGeneratedImportReport {
        let WireValue::Array(fields) =
            loom_codec::decode(bytes).expect("columnar import report cbor")
        else {
            panic!("columnar import report must be an array");
        };
        let [
            WireValue::Text(format),
            WireValue::Array(columns),
            WireValue::Uint(rows),
            WireValue::Uint(segment_count),
            WireValue::Uint(target_segment_rows),
            WireValue::Uint(bytes_in),
            WireValue::Bool(replaced),
            WireValue::Bool(dry_run),
        ] = fields.as_slice()
        else {
            panic!("columnar import report has invalid generated shape");
        };
        let columns = columns
            .iter()
            .map(|column| {
                let WireValue::Array(fields) = column else {
                    panic!("columnar import report column must be an array");
                };
                let [WireValue::Text(name), WireValue::Uint(tag)] = fields.as_slice() else {
                    panic!("columnar import report column has invalid shape");
                };
                (name.clone(), *tag)
            })
            .collect();
        ColumnarGeneratedImportReport {
            format: format.clone(),
            columns,
            rows: *rows,
            segment_count: *segment_count,
            target_segment_rows: *target_segment_rows,
            bytes_in: *bytes_in,
            replaced: *replaced,
            dry_run: *dry_run,
        }
    }

    fn assert_empty_report_fidelity(report: &loom_interchange::ImportReport) {
        assert!(report.warnings.is_empty());
        assert!(report.fidelity_issues.is_empty());
    }

    fn fidelity_tuples(report: &loom_interchange::ImportReport) -> Vec<(String, String, String)> {
        report
            .fidelity_issues
            .iter()
            .map(|issue| {
                assert_eq!(issue.severity, loom_interchange::FidelitySeverity::Warning);
                (
                    issue.source_entity_id.clone(),
                    issue.field.clone(),
                    issue.reason.clone(),
                )
            })
            .collect()
    }

    fn expected_slack_message_source(channel_id: &str, ts: &str) -> String {
        let digest = Digest::hash(
            Algo::Sha256,
            format!("slack:message:{channel_id}:{ts}").as_bytes(),
        );
        let hex = digest.to_hex();
        format!("message:slack-{}", &hex[..24])
    }

    fn query_items(path: &PathBuf) -> Result<Vec<(i64, String, String)>, loom_core::LoomError> {
        let loom = open_loom_unlocked(path, None)?;
        let ns = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))?;
        let table = loom_core::get_table(
            &loom,
            ns,
            &loom_core::workspace::facet_path(FacetKind::Sql, "app/tables/items"),
        )?;
        let mut rows = table
            .scan(&loom_core::Predicate::All)
            .into_iter()
            .map(|row| {
                let id = match &row[0] {
                    Value::Int(id) => id,
                    other => panic!("unexpected id cell {other:?}"),
                };
                let name = match &row[1] {
                    Value::Text(name) => name.clone(),
                    other => panic!("unexpected name cell {other:?}"),
                };
                let note = match &row[2] {
                    Value::Text(note) => note.clone(),
                    other => panic!("unexpected note cell {other:?}"),
                };
                (*id, name, note)
            })
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| row.0);
        Ok(rows)
    }

    fn sample_columnar_dataset() -> loom_core::ColumnarSet {
        let mut dataset = loom_core::ColumnarSet::new(
            vec![
                ("id".to_string(), loom_core::tabular::ColumnType::Int),
                ("label".to_string(), loom_core::tabular::ColumnType::Text),
            ],
            2,
        )
        .expect("sample dataset");
        dataset
            .append_row(vec![Value::Int(1), Value::Text("alpha".to_string())])
            .expect("append alpha");
        dataset
            .append_row(vec![Value::Int(2), Value::Text("beta".to_string())])
            .expect("append beta");
        dataset
    }

    fn sample_arrow_bytes() -> Vec<u8> {
        loom_core::columnar_to_arrow_ipc(&sample_columnar_dataset()).expect("arrow bytes")
    }

    fn sample_parquet_bytes() -> Vec<u8> {
        loom_core::columnar_to_parquet(&sample_columnar_dataset()).expect("parquet bytes")
    }

    fn query_columnar_rows(
        path: &PathBuf,
        name: &str,
    ) -> Result<Vec<Vec<Value>>, loom_core::LoomError> {
        let loom = open_loom_unlocked(path, None)?;
        let ns = loom.registry().open(&loom_core::WsSelector::Typed {
            ty: FacetKind::Columnar,
            name: "main".to_string(),
        })?;
        loom_core::columnar_scan(&loom, ns, name)
    }

    fn vector_metadata_cbor(label: &str) -> Vec<u8> {
        let cell = loom_core::key_to_cbor(&Value::Text(label.to_string()));
        let cell = loom_codec::decode(&cell).expect("metadata cell");
        loom_codec::encode(&WireValue::Map(vec![(
            WireValue::Text("label".to_string()),
            cell,
        )]))
        .expect("metadata cbor")
    }

    fn query_vector_entry(
        path: &PathBuf,
        id: &str,
    ) -> Result<(Vec<f32>, std::collections::BTreeMap<String, Value>), loom_core::LoomError> {
        let loom = open_loom_unlocked(path, None)?;
        let ns = loom.registry().open(&loom_core::WsSelector::Typed {
            ty: FacetKind::Vector,
            name: "main".to_string(),
        })?;
        loom_core::vector_get(&loom, ns, "notes", id)?
            .ok_or_else(|| loom_core::LoomError::new(Code::NotFound, "missing vector entry"))
    }

    fn query_vector_source(path: &PathBuf, id: &str) -> Result<String, loom_core::LoomError> {
        let loom = open_loom_unlocked(path, None)?;
        let ns = loom.registry().open(&loom_core::WsSelector::Typed {
            ty: FacetKind::Vector,
            name: "main".to_string(),
        })?;
        loom_core::vector_source_text(&loom, ns, "notes", id)?
            .ok_or_else(|| loom_core::LoomError::new(Code::NotFound, "missing vector source"))
    }

    fn query_vector_model(
        path: &PathBuf,
    ) -> Result<Option<loom_core::EmbeddingModel>, loom_core::LoomError> {
        let loom = open_loom_unlocked(path, None)?;
        let ns = loom.registry().open(&loom_core::WsSelector::Typed {
            ty: FacetKind::Vector,
            name: "main".to_string(),
        })?;
        loom_core::vector_embedding_model(&loom, ns, "notes")
    }

    fn seed_inference_instance_state(path: &PathBuf, kind: loom_types::InferenceModelKind) {
        let mut loom = open_loom_unlocked(path, None).expect("open store");
        let workspace = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))
            .expect("workspace");
        let mut state = loom_inference::InferenceInstanceState::default();
        state.upsert_instance(loom_types::InferenceInstanceDescriptor {
            name: "embed".to_string(),
            kind,
            model: loom_types::ModelRef::new(kind, "example/model"),
            runtime: loom_types::RuntimeKind::HostedApi,
            preset: None,
            settings: loom_types::InferenceInstanceSettings::empty(),
            resolved_settings: std::collections::BTreeMap::new(),
        });
        loom_core::put_inference_instance_state(&mut loom, workspace, &state)
            .expect("write inference state");
        save_loom(&mut loom).expect("save inference state");
    }

    fn inference_state(path: &PathBuf) -> loom_inference::InferenceInstanceState {
        let loom = open_loom_unlocked(path, None).expect("open inference state store");
        let workspace = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))
            .expect("workspace");
        loom_core::inference_instance_state(&loom, workspace).expect("inference state")
    }

    fn inference_instance_create_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        name: &str,
        settings_json: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as InferenceInstance>::inference_instance_create_json(
                client,
                session.clone(),
                "main".to_string(),
                name.to_string(),
                "sentence-transformers/all-MiniLM-L6-v2".to_string(),
                "text-embedding".to_string(),
                "hosted-api".to_string(),
                Some("fast".to_string()),
                settings_json.to_string(),
            ),
        )
    }

    fn inference_instance_update_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        name: &str,
        settings_json: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as InferenceInstance>::inference_instance_update_json(
                client,
                session.clone(),
                "main".to_string(),
                name.to_string(),
                Some("deterministic".to_string()),
                settings_json.to_string(),
            ),
        )
    }

    fn inference_instance_delete_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        name: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as InferenceInstance>::inference_instance_delete_json(
                client,
                session.clone(),
                "main".to_string(),
                name.to_string(),
            ),
        )
    }

    fn inference_instance_create_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        name: &str,
        settings_json: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as InferenceInstance>::inference_instance_create_json(
                client,
                session.clone(),
                "main".to_string(),
                name.to_string(),
                "sentence-transformers/all-MiniLM-L6-v2".to_string(),
                "text-embedding".to_string(),
                "hosted-api".to_string(),
                Some("fast".to_string()),
                settings_json.to_string(),
            ),
        )
    }

    fn inference_instance_update_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        name: &str,
        settings_json: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as InferenceInstance>::inference_instance_update_json(
                client,
                session.clone(),
                "main".to_string(),
                name.to_string(),
                Some("deterministic".to_string()),
                settings_json.to_string(),
            ),
        )
    }

    fn inference_instance_delete_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        name: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as InferenceInstance>::inference_instance_delete_json(
                client,
                session.clone(),
                "main".to_string(),
                name.to_string(),
            ),
        )
    }

    fn inference_instance_name(json: &str) -> String {
        let value: serde_json::Value = serde_json::from_str(json).expect("instance result json");
        value["instance"]["name"]
            .as_str()
            .expect("instance name")
            .to_string()
    }

    fn inference_instance_setting(json: &str, key: &str) -> String {
        let value: serde_json::Value = serde_json::from_str(json).expect("instance result json");
        value["instance"]["resolved-settings"][key]
            .as_str()
            .expect("resolved setting")
            .to_string()
    }

    fn inference_delete_name(json: &str) -> String {
        let value: serde_json::Value = serde_json::from_str(json).expect("delete result json");
        assert_eq!(value["deleted"].as_bool(), Some(true));
        value["name"].as_str().expect("deleted name").to_string()
    }

    fn serve_listener_request(
        surface: &str,
        selectors: &[&str],
        bind: &str,
        transport: &str,
        enabled: bool,
    ) -> String {
        serde_json::json!({
            "surface": surface,
            "selectors": selectors,
            "bind": bind,
            "transport": transport,
            "enabled": enabled
        })
        .to_string()
    }

    fn serve_listener_request_with_refs(
        tls_certificate_bundle: Option<&str>,
        network_access_policy: Option<&str>,
    ) -> String {
        let mut value = serde_json::json!({
            "surface": "admin",
            "selectors": [],
            "bind": "127.0.0.1:19082",
            "transport": "rest",
            "enabled": true
        });
        if let Some(bundle) = tls_certificate_bundle {
            value["tls_certificate_bundle"] = serde_json::Value::String(bundle.to_string());
        }
        if let Some(policy) = network_access_policy {
            value["network_access_policy"] = serde_json::Value::String(policy.to_string());
        }
        value.to_string()
    }

    fn serve_route_request(
        listener: &str,
        route: &str,
        prefix: &str,
        workspace: Option<&str>,
        root: &str,
    ) -> String {
        let mut value = serde_json::json!({
            "listener": listener,
            "route": route,
            "prefix": prefix,
            "root": root
        });
        if let Some(workspace) = workspace {
            value["workspace"] = serde_json::Value::String(workspace.to_string());
        }
        value.to_string()
    }

    fn serve_json(raw: &str) -> serde_json::Value {
        serde_json::from_str(raw).expect("serve json")
    }

    fn seed_files_workspace_session(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        name: &str,
        id: WorkspaceId,
    ) -> Result<(), String> {
        client
            .with_session(session, |loom| {
                if loom
                    .registry()
                    .open(&loom_core::WsSelector::Name(name.to_string()))
                    .is_err()
                {
                    loom.registry_mut()
                        .create(FacetKind::Files, Some(name), id)?;
                    save_loom(loom)?;
                }
                Ok(())
            })
            .map_err(strerr)
    }

    fn serve_audit_actions(path: &PathBuf) -> Result<Vec<String>, String> {
        Ok(FileStore::open_read(path)
            .map_err(strerr)?
            .audit_records()
            .map_err(strerr)?
            .into_iter()
            .map(|record| record.action)
            .collect())
    }

    fn serve_listener_ids(path: &PathBuf) -> Result<Vec<String>, String> {
        Ok(FileStore::open_read(path)
            .map_err(strerr)?
            .served_listeners()
            .map_err(strerr)?
            .into_iter()
            .map(|record| record.id)
            .collect())
    }

    fn serve_route_ids(path: &PathBuf, listener_id: &str) -> Result<Vec<String>, String> {
        let store = FileStore::open_read(path).map_err(strerr)?;
        let key =
            loom_client::serve_config::web_listener_control_key(listener_id).map_err(strerr)?;
        let Some(bytes) = store.control_get(&key).map_err(strerr)? else {
            return Ok(Vec::new());
        };
        Ok(loom_substrate::web::WebListener::decode(&bytes)
            .map_err(strerr)?
            .routes
            .routes
            .into_iter()
            .map(|route| route.route_id)
            .collect())
    }

    fn serve_listener_configure_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        request: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as ServeConfig>::serve_listener_configure_json(
                client,
                session.clone(),
                request.to_string(),
            ),
        )
    }

    fn serve_listener_list_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as ServeConfig>::serve_listener_list_json(
                client,
                session.clone(),
            ),
        )
    }

    fn serve_listener_set_enabled_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        listener_id: &str,
        enabled: bool,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as ServeConfig>::serve_listener_set_enabled_json(
                client,
                session.clone(),
                listener_id.to_string(),
                enabled,
            ),
        )
    }

    fn serve_listener_remove_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        listener_id: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as ServeConfig>::serve_listener_remove_json(
                client,
                session.clone(),
                listener_id.to_string(),
            ),
        )
    }

    fn serve_route_list_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        listener_id: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as ServeConfig>::serve_web_route_list_json(
                client,
                session.clone(),
                listener_id.to_string(),
            ),
        )
    }

    fn serve_route_set_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        request: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as ServeConfig>::serve_web_route_set_json(
                client,
                session.clone(),
                request.to_string(),
            ),
        )
    }

    fn serve_route_remove_local(
        client: &loom_client::LocalLoomClient,
        session: &loom_client::types::LoomSession,
        listener_id: &str,
        route_id: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <loom_client::LocalLoomClient as ServeConfig>::serve_web_route_remove_json(
                client,
                session.clone(),
                listener_id.to_string(),
                route_id.to_string(),
            ),
        )
    }

    fn serve_listener_configure_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        request: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as ServeConfig>::serve_listener_configure_json(
                client,
                session.clone(),
                request.to_string(),
            ),
        )
    }

    fn serve_listener_list_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as ServeConfig>::serve_listener_list_json(
                client,
                session.clone(),
            ),
        )
    }

    fn serve_listener_set_enabled_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        listener_id: &str,
        enabled: bool,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as ServeConfig>::serve_listener_set_enabled_json(
                client,
                session.clone(),
                listener_id.to_string(),
                enabled,
            ),
        )
    }

    fn serve_listener_remove_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        listener_id: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as ServeConfig>::serve_listener_remove_json(
                client,
                session.clone(),
                listener_id.to_string(),
            ),
        )
    }

    fn serve_route_list_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        listener_id: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as ServeConfig>::serve_web_route_list_json(
                client,
                session.clone(),
                listener_id.to_string(),
            ),
        )
    }

    fn serve_route_set_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        request: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as ServeConfig>::serve_web_route_set_json(
                client,
                session.clone(),
                request.to_string(),
            ),
        )
    }

    fn serve_route_remove_remote(
        client: &RemoteLoomClient<InProcessRemoteTransport>,
        session: &loom_client::types::LoomSession,
        listener_id: &str,
        route_id: &str,
    ) -> Result<String, loom_core::LoomError> {
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as ServeConfig>::serve_web_route_remove_json(
                client,
                session.clone(),
                listener_id.to_string(),
                route_id.to_string(),
            ),
        )
    }

    #[test]
    fn serve_config_generated_contract_authenticates_before_caller_input_parsing() {
        let (client, session, _path) =
            create_client("serve-config-auth-order-local", "main").expect("local");
        let unknown = unknown_session(&session);
        assert_eq!(
            serve_listener_configure_local(&client, &unknown, "{not json")
                .expect_err("unknown local listener configure")
                .code,
            Code::NotFound
        );
        assert_eq!(
            serve_route_set_local(&client, &unknown, "{not json")
                .expect_err("unknown local route set")
                .code,
            Code::NotFound
        );
        assert_eq!(
            serve_listener_set_enabled_local(&client, &unknown, "bad listener", true)
                .expect_err("unknown local enable")
                .code,
            Code::NotFound
        );
        assert_eq!(
            serve_listener_remove_local(&client, &unknown, "bad listener")
                .expect_err("unknown local remove")
                .code,
            Code::NotFound
        );
        assert_eq!(
            serve_route_list_local(&client, &unknown, "bad listener")
                .expect_err("unknown local route list")
                .code,
            Code::NotFound
        );
        assert_eq!(
            serve_route_remove_local(&client, &unknown, "bad listener", "bad route")
                .expect_err("unknown local route remove")
                .code,
            Code::NotFound
        );
        assert!(client.close(&session));

        let (client, session, path) =
            create_authenticated_non_admin_client("serve-config-auth-denied-local")
                .expect("local non-admin");
        for error in [
            serve_listener_configure_local(&client, &session, "{not json")
                .expect_err("denied listener configure"),
            serve_listener_list_local(&client, &session).expect_err("denied listener list"),
            serve_listener_set_enabled_local(&client, &session, "bad listener", true)
                .expect_err("denied listener enable"),
            serve_listener_remove_local(&client, &session, "bad listener")
                .expect_err("denied listener remove"),
            serve_route_set_local(&client, &session, "{not json").expect_err("denied route set"),
            serve_route_list_local(&client, &session, "bad listener")
                .expect_err("denied route list"),
            serve_route_remove_local(&client, &session, "bad listener", "bad route")
                .expect_err("denied route remove"),
        ] {
            assert_eq!(error.code, Code::PermissionDenied);
        }
        assert!(client.close(&session));
        assert!(
            serve_audit_actions(&path)
                .expect("local denied audit")
                .is_empty()
        );

        let (remote_client, remote_session, _path) =
            create_remote_client("serve-config-auth-order-remote", "main").expect("remote");
        remote_client.bind_session(999_999u64.to_be_bytes().to_vec());
        assert_eq!(
            serve_listener_configure_remote(&remote_client, &remote_session, "{not json")
                .expect_err("unknown remote listener configure")
                .code,
            Code::NotFound
        );
        assert_eq!(
            serve_route_set_remote(&remote_client, &remote_session, "{not json")
                .expect_err("unknown remote route set")
                .code,
            Code::NotFound
        );
        drop(remote_client);

        let (remote_client, remote_session, path) =
            create_authenticated_non_admin_remote_client("serve-config-auth-denied-remote")
                .expect("remote non-admin");
        for error in [
            serve_listener_configure_remote(&remote_client, &remote_session, "{not json")
                .expect_err("denied remote listener configure"),
            serve_listener_list_remote(&remote_client, &remote_session)
                .expect_err("denied remote listener list"),
            serve_listener_set_enabled_remote(
                &remote_client,
                &remote_session,
                "bad listener",
                true,
            )
            .expect_err("denied remote listener enable"),
            serve_listener_remove_remote(&remote_client, &remote_session, "bad listener")
                .expect_err("denied remote listener remove"),
            serve_route_set_remote(&remote_client, &remote_session, "{not json")
                .expect_err("denied remote route set"),
            serve_route_list_remote(&remote_client, &remote_session, "bad listener")
                .expect_err("denied remote route list"),
            serve_route_remove_remote(&remote_client, &remote_session, "bad listener", "bad route")
                .expect_err("denied remote route remove"),
        ] {
            assert_eq!(error.code, Code::PermissionDenied);
        }
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Store>::close(
                &remote_client,
                remote_session,
            ),
        )
        .expect("remote close");
        drop(remote_client);
        assert!(
            serve_audit_actions(&path)
                .expect("remote denied audit")
                .is_empty()
        );
    }

    #[test]
    fn serve_config_generated_contract_local_semantics_and_reopen() {
        let (client, session, path) =
            create_client("serve-config-local-semantics", "main").expect("local");
        seed_files_workspace_session(&client, &session, "site", nid(71)).expect("seed files");

        assert_eq!(
            serve_listener_configure_local(
                &client,
                &session,
                &serve_listener_request_with_refs(Some("missing"), None),
            )
            .expect_err("missing certificate")
            .code,
            Code::NotFound
        );
        assert!(serve_listener_ids(&path).expect("listeners").is_empty());
        assert!(serve_audit_actions(&path).expect("audit").is_empty());

        let web_a = serve_json(
            &serve_listener_configure_local(
                &client,
                &session,
                &serve_listener_request("web", &["site"], "127.0.0.1:19083", "rest", true),
            )
            .expect("configure web a"),
        );
        let web_b = serve_json(
            &serve_listener_configure_local(
                &client,
                &session,
                &serve_listener_request("web", &["site"], "127.0.0.1:19084", "rest", true),
            )
            .expect("configure web b"),
        );
        let web_a_id = web_a["id"].as_str().expect("web a id").to_string();
        let web_b_id = web_b["id"].as_str().expect("web b id").to_string();
        let listed = serve_json(&serve_listener_list_local(&client, &session).expect("list"));
        let ids = listed["listeners"]
            .as_array()
            .expect("listeners")
            .iter()
            .map(|listener| listener["id"].as_str().expect("id").to_string())
            .collect::<Vec<_>>();
        let mut sorted_ids = ids.clone();
        sorted_ids.sort();
        assert_eq!(ids, sorted_ids);

        assert_eq!(
            serve_json(
                &serve_listener_set_enabled_local(&client, &session, &web_a_id, false)
                    .expect("disable")
            )["enabled"],
            false
        );
        assert_eq!(
            serve_json(
                &serve_listener_set_enabled_local(&client, &session, &web_a_id, true)
                    .expect("enable")
            )["enabled"],
            true
        );

        serve_route_set_local(
            &client,
            &session,
            &serve_route_request(&web_a_id, "route-b", "/b", Some("site"), "/content/b"),
        )
        .expect("set route b");
        serve_route_set_local(
            &client,
            &session,
            &serve_route_request(&web_a_id, "route-a", "a", None, "/content/a"),
        )
        .expect("set route a");
        let routes =
            serve_json(&serve_route_list_local(&client, &session, &web_a_id).expect("list routes"));
        let route_ids = routes["routes"]
            .as_array()
            .expect("routes")
            .iter()
            .map(|route| route["route_id"].as_str().expect("route id"))
            .collect::<Vec<_>>();
        assert_eq!(route_ids, vec!["route-a", "route-b"]);
        assert_eq!(routes["routes"][0]["path_prefix"], "/a");

        let baseline_routes = serve_route_ids(&path, &web_a_id).expect("baseline routes");
        let baseline_audit = serve_audit_actions(&path).expect("baseline audit");
        assert_eq!(
            serve_route_set_local(
                &client,
                &session,
                &serve_route_request(&web_a_id, "bad", "/bad", Some("missing"), "/bad"),
            )
            .expect_err("missing route workspace")
            .code,
            Code::NotFound
        );
        assert_eq!(
            serve_route_remove_local(&client, &session, &web_a_id, "missing")
                .expect_err("missing route")
                .code,
            Code::NotFound
        );
        assert_eq!(
            serve_route_ids(&path, &web_a_id).expect("routes after failed"),
            baseline_routes
        );
        assert_eq!(
            serve_audit_actions(&path).expect("audit after failed"),
            baseline_audit
        );

        assert!(client.close(&session));
        let reopened = client.open().expect("reopen");
        assert_eq!(
            serve_listener_ids(&path).expect("reopened listeners").len(),
            2
        );
        assert_eq!(
            serve_route_ids(&path, &web_a_id).expect("reopened routes"),
            vec!["route-a".to_string(), "route-b".to_string()]
        );
        serve_route_remove_local(&client, &reopened, &web_a_id, "route-b").expect("remove route");
        serve_listener_remove_local(&client, &reopened, &web_b_id).expect("remove listener");
        assert_eq!(
            serve_audit_actions(&path).expect("final audit"),
            vec![
                "serve.listener.configure".to_string(),
                "serve.listener.configure".to_string(),
                "serve.listener.list".to_string(),
                "serve.listener.disable".to_string(),
                "serve.listener.enable".to_string(),
                "serve.web.route.set".to_string(),
                "serve.web.route.set".to_string(),
                "serve.web.route.list".to_string(),
                "serve.web.route.remove".to_string(),
                "serve.listener.remove".to_string(),
            ]
        );
        assert!(client.close(&reopened));
    }

    #[test]
    fn serve_config_generated_contract_remote_semantics_match_local_shape() {
        let (local_client, local_session, _local_path) =
            create_client("serve-config-parity-local", "main").expect("local");
        seed_files_workspace_session(&local_client, &local_session, "site", nid(72))
            .expect("seed local files");
        let local_configured = serve_listener_configure_local(
            &local_client,
            &local_session,
            &serve_listener_request("web", &["site"], "127.0.0.1:19085", "rest", true),
        )
        .expect("local configure");
        let local_id = serve_json(&local_configured)["id"]
            .as_str()
            .expect("local id")
            .to_string();
        let local_route = serve_route_set_local(
            &local_client,
            &local_session,
            &serve_route_request(&local_id, "docs", "docs", Some("site"), "/docs"),
        )
        .expect("local route");
        let local_listeners =
            serve_listener_list_local(&local_client, &local_session).expect("local list");
        let local_disabled =
            serve_listener_set_enabled_local(&local_client, &local_session, &local_id, false)
                .expect("local disable");
        let local_enabled =
            serve_listener_set_enabled_local(&local_client, &local_session, &local_id, true)
                .expect("local enable");
        let local_routes =
            serve_route_list_local(&local_client, &local_session, &local_id).expect("local routes");
        let local_removed_route =
            serve_route_remove_local(&local_client, &local_session, &local_id, "docs")
                .expect("local remove route");
        let local_removed_listener =
            serve_listener_remove_local(&local_client, &local_session, &local_id)
                .expect("local remove listener");
        assert!(local_client.close(&local_session));

        let (seed_client, seed_session, remote_path) =
            create_client("serve-config-parity-remote", "main").expect("remote seed");
        seed_files_workspace_session(&seed_client, &seed_session, "site", nid(72))
            .expect("seed remote files");
        assert!(seed_client.close(&seed_session));
        drop(seed_client);
        let (remote_client, remote_session) =
            remote_client_for_existing_store(&remote_path).expect("remote");
        let remote_configured = serve_listener_configure_remote(
            &remote_client,
            &remote_session,
            &serve_listener_request("web", &["site"], "127.0.0.1:19085", "rest", true),
        )
        .expect("remote configure");
        assert_eq!(
            serve_json(&remote_configured),
            serve_json(&local_configured)
        );
        let remote_id = serve_json(&remote_configured)["id"]
            .as_str()
            .expect("remote id")
            .to_string();
        let remote_route = serve_route_set_remote(
            &remote_client,
            &remote_session,
            &serve_route_request(&remote_id, "docs", "docs", Some("site"), "/docs"),
        )
        .expect("remote route");
        assert_eq!(serve_json(&remote_route), serve_json(&local_route));
        let remote_listeners =
            serve_listener_list_remote(&remote_client, &remote_session).expect("remote list");
        assert_eq!(serve_json(&remote_listeners), serve_json(&local_listeners));
        let remote_disabled =
            serve_listener_set_enabled_remote(&remote_client, &remote_session, &remote_id, false)
                .expect("remote disable");
        assert_eq!(serve_json(&remote_disabled), serve_json(&local_disabled));
        let remote_enabled =
            serve_listener_set_enabled_remote(&remote_client, &remote_session, &remote_id, true)
                .expect("remote enable");
        assert_eq!(serve_json(&remote_enabled), serve_json(&local_enabled));
        let remote_routes = serve_route_list_remote(&remote_client, &remote_session, &remote_id)
            .expect("remote routes");
        assert_eq!(serve_json(&remote_routes), serve_json(&local_routes));
        assert_eq!(
            serve_route_ids(&remote_path, &remote_id).expect("remote route persisted"),
            vec!["docs".to_string()]
        );
        let remote_baseline_routes =
            serve_route_ids(&remote_path, &remote_id).expect("remote baseline routes");
        let remote_baseline_audit =
            serve_audit_actions(&remote_path).expect("remote baseline audit");
        assert_eq!(
            serve_route_set_remote(
                &remote_client,
                &remote_session,
                &serve_route_request(&remote_id, "bad", "/bad", Some("missing"), "/bad"),
            )
            .expect_err("remote missing workspace")
            .code,
            Code::NotFound
        );
        assert_eq!(
            serve_route_ids(&remote_path, &remote_id).expect("remote failed routes"),
            remote_baseline_routes
        );
        assert_eq!(
            serve_audit_actions(&remote_path).expect("remote failed audit"),
            remote_baseline_audit
        );
        let remote_removed_route =
            serve_route_remove_remote(&remote_client, &remote_session, &remote_id, "docs")
                .expect("remote remove route");
        assert_eq!(
            serve_json(&remote_removed_route),
            serve_json(&local_removed_route)
        );
        assert!(
            serve_route_ids(&remote_path, &remote_id)
                .expect("remote route removed")
                .is_empty()
        );
        let remote_removed_listener =
            serve_listener_remove_remote(&remote_client, &remote_session, &remote_id)
                .expect("remote remove listener");
        assert_eq!(
            serve_json(&remote_removed_listener),
            serve_json(&local_removed_listener)
        );
        assert!(
            serve_listener_ids(&remote_path)
                .expect("remote listener removed")
                .is_empty()
        );
        assert_eq!(
            serve_audit_actions(&remote_path).expect("remote final audit"),
            vec![
                "serve.listener.configure".to_string(),
                "serve.web.route.set".to_string(),
                "serve.listener.list".to_string(),
                "serve.listener.disable".to_string(),
                "serve.listener.enable".to_string(),
                "serve.web.route.list".to_string(),
                "serve.web.route.remove".to_string(),
                "serve.listener.remove".to_string(),
            ]
        );
        block(
            <RemoteLoomClient<InProcessRemoteTransport> as Store>::close(
                &remote_client,
                remote_session,
            ),
        )
        .expect("remote close");
        drop(remote_client);
        assert!(
            serve_listener_ids(&remote_path)
                .expect("remote reopen listeners")
                .is_empty()
        );
        assert!(
            serve_route_ids(&remote_path, &remote_id)
                .expect("remote reopen routes")
                .is_empty()
        );
    }

    fn inference_audit_sequence(json: &str) -> u64 {
        serde_json::from_str::<serde_json::Value>(json)
            .expect("inference result json")
            .get("audit-sequence")
            .and_then(serde_json::Value::as_u64)
            .expect("audit sequence")
    }

    fn page_body_text(
        path: &PathBuf,
        page_id: &str,
    ) -> Result<(String, Option<String>), loom_core::LoomError> {
        let loom = open_loom_unlocked(path, None)?;
        let ns = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))?;
        let page = loom_pages::get_page(&loom, ns, "pages", page_id)?
            .ok_or_else(|| loom_core::LoomError::new(Code::NotFound, "missing page"))?;
        let body = loom_substrate::body::Body::decode(
            page.body
                .as_deref()
                .ok_or_else(|| loom_core::LoomError::new(Code::NotFound, "missing page body"))?,
        )?;
        let mut text = String::new();
        for block in &body.blocks {
            match &block.kind {
                loom_substrate::body::BlockKind::Opaque { payload, .. } => {
                    text.push_str(
                        &String::from_utf8(payload.clone())
                            .map_err(|error| loom_core::LoomError::invalid(error.to_string()))?,
                    );
                }
                _ => {
                    for run in &block.runs {
                        text.push_str(&run.text);
                    }
                }
            }
            text.push('\n');
        }
        Ok((text, page.parent_page_id))
    }

    fn page_opaque_body(
        path: &PathBuf,
        page_id: &str,
        expected_kind: &str,
    ) -> Result<Vec<u8>, loom_core::LoomError> {
        let loom = open_loom_unlocked(path, None)?;
        let ns = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))?;
        let page = loom_pages::get_page(&loom, ns, "pages", page_id)?
            .ok_or_else(|| loom_core::LoomError::new(Code::NotFound, "missing page"))?;
        let body = loom_substrate::body::Body::decode(
            page.body
                .as_deref()
                .ok_or_else(|| loom_core::LoomError::new(Code::NotFound, "missing page body"))?,
        )?;
        match body.blocks.as_slice() {
            [block] => match &block.kind {
                loom_substrate::body::BlockKind::Opaque { kind, payload }
                    if kind == expected_kind =>
                {
                    Ok(payload.clone())
                }
                _ => Err(loom_core::LoomError::corrupt(
                    "page body is not the expected opaque source block",
                )),
            },
            _ => Err(loom_core::LoomError::corrupt(
                "page body must contain one opaque source block",
            )),
        }
    }

    fn assert_no_page_profile(path: &PathBuf) {
        let loom = open_loom_unlocked(path, None).expect("open store");
        let ns = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))
            .expect("workspace");
        assert!(
            loom_pages::get_space(&loom, ns, "pages", "wiki")
                .expect("space read")
                .is_none()
        );
    }

    fn page_history_len(path: &PathBuf, workspace_id: &str, page_id: &str) -> usize {
        let loom = open_loom_unlocked(path, None).expect("open store");
        let ns = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))
            .expect("workspace");
        loom_pages::page_history(&loom, ns, workspace_id, page_id)
            .expect("page history")
            .len()
    }

    fn chat_projection_texts(path: &PathBuf) -> Result<(Vec<String>, usize), loom_core::LoomError> {
        let loom = open_loom_unlocked(path, None)?;
        let ns = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))?;
        let channel = loom_chat::resolve_channel_id(&loom, ns, "chat", "general")?;
        let projection = loom_chat::channel_projection(&loom, ns, "chat", &channel)?;
        let texts = projection
            .messages
            .iter()
            .map(|message| {
                String::from_utf8(message.body.clone())
                    .map_err(|error| loom_core::LoomError::invalid(error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((texts, projection.threads.len()))
    }

    fn assert_no_chat_profile(path: &PathBuf) {
        let loom = open_loom_unlocked(path, None).expect("open store");
        let ns = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))
            .expect("workspace");
        assert_eq!(
            loom_chat::resolve_channel_id(&loom, ns, "chat", "general")
                .expect_err("missing channel")
                .code,
            Code::NotFound
        );
    }

    fn drive_file_bytes(path: &PathBuf, file_id: &str) -> Result<Vec<u8>, loom_core::LoomError> {
        let loom = open_loom_unlocked(path, None)?;
        let ns = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))?;
        loom_drive::read_file(&loom, ns, "drive", file_id)
    }

    fn drive_folder_names(
        path: &PathBuf,
        folder_id: &str,
    ) -> Result<Vec<String>, loom_core::LoomError> {
        let loom = open_loom_unlocked(path, None)?;
        let ns = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))?;
        let mut names = loom_drive::list_folder(&loom, ns, "drive", folder_id)?
            .entries
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>();
        names.sort();
        Ok(names)
    }

    fn assert_no_drive_profile(path: &PathBuf) {
        let loom = open_loom_unlocked(path, None).expect("open store");
        let ns = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))
            .expect("workspace");
        assert_eq!(
            loom_drive::read_file(&loom, ns, "drive", "payload")
                .expect_err("missing drive file")
                .code,
            Code::NotFound
        );
    }

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (path, bytes) in entries {
            if path.ends_with('/') {
                archive
                    .add_directory(*path, options)
                    .expect("add directory");
            } else {
                archive.start_file(path, options).expect("start file");
                archive.write_all(bytes).expect("write file");
            }
        }
        archive.finish().expect("finish archive").into_inner()
    }

    fn drive_archive(manifest: &[u8], entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut all = Vec::with_capacity(entries.len() + 1);
        all.push(("manifest.json", manifest));
        all.extend_from_slice(entries);
        zip_bytes(&all)
    }

    fn ticket_by_identity(
        path: &PathBuf,
        source: &str,
        external_id: &str,
    ) -> Result<loom_tickets::Ticket, loom_core::LoomError> {
        let loom = open_loom_unlocked(path, None)?;
        let ns = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))?;
        let reader = loom_tickets::TicketProfileReader::open(&loom, ns, "studio")?
            .ok_or_else(|| loom_core::LoomError::new(Code::NotFound, "missing ticket profile"))?;
        let identity = loom_tickets::ExternalTicketIdentity::new(source, external_id)?;
        reader
            .ticket_by_external_identity(&identity)?
            .ok_or_else(|| loom_core::LoomError::new(Code::NotFound, "missing imported ticket"))
    }

    fn assert_no_ticket_profile(path: &PathBuf) {
        let loom = open_loom_unlocked(path, None).expect("open store");
        let ns = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))
            .expect("workspace");
        assert!(
            loom_tickets::TicketProfileReader::open(&loom, ns, "studio")
                .expect("profile read")
                .is_none()
        );
    }

    #[test]
    fn import_table_csv_local_preserves_payload_validates_and_reopens() {
        let payload = b"id,name,note\n1,alpha,\"quoted, preserved\"\n2,beta,\"\"\n";
        let (client, session, path) =
            create_client("interchange-table-csv-local-dry", "main").expect("client");

        let dry = import_csv_local(&client, &session, "memory://items.csv", payload, true)
            .expect("dry run import");
        let dry_report = generated_report(&dry);
        assert_eq!(dry_report.profile, "table-csv");
        assert_eq!(dry_report.source_scope, "memory://items.csv");
        assert_eq!(dry_report.bytes_in, payload.len() as u64);
        assert_eq!(dry_report.rows_imported, 2);
        assert_eq!(dry_report.operations_planned, 2);
        assert_eq!(dry_report.operations_applied, 0);
        assert!(dry_report.dry_run);
        assert!(dry_report.commit.is_none());
        assert_empty_report_fidelity(&dry_report);
        assert!(client.close(&session));
        assert!(query_items(&path).is_err());

        let (client, session, path) =
            create_client("interchange-table-csv-local-write", "main").expect("client");

        let bad = block(
            <loom_client::LocalLoomClient as InterchangeProfiles>::import_table_csv(
                &client,
                session.clone(),
                "main".to_string(),
                "memory://bad.csv".to_string(),
                b"id,name,note\nx,alpha,note\n".to_vec(),
                "app".to_string(),
                "items".to_string(),
                "id:int,name:text,note:text".to_string(),
                "id".to_string(),
                "snapshot".to_string(),
                true,
                None,
                None,
                true,
            ),
        )
        .expect_err("invalid CSV value is rejected");
        assert_eq!(bad.code, Code::InvalidArgument);

        let written = import_csv_local(&client, &session, "memory://items.csv", payload, false)
            .expect("write import");
        let write_report = generated_report(&written);
        assert_eq!(write_report.bytes_in, payload.len() as u64);
        assert_eq!(write_report.operations_applied, 2);
        assert!(!write_report.dry_run);
        assert!(write_report.commit.is_some());
        assert_empty_report_fidelity(&write_report);
        assert!(client.close(&session));
        assert_eq!(
            query_items(&path).expect("reopen query"),
            vec![
                (1, "alpha".to_string(), "quoted, preserved".to_string()),
                (2, "beta".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn import_table_csv_remote_matches_report_and_persists() {
        let payload = b"id,name,note\n1,alpha,remote\n2,beta,preserved\n";
        let (local_client, local_session, _) =
            create_client("interchange-table-csv-local-parity", "main").expect("local client");
        let local_dry = import_csv_local(
            &local_client,
            &local_session,
            "memory://items.csv",
            payload,
            true,
        )
        .expect("local dry run");
        assert!(local_client.close(&local_session));

        let (remote_client, remote_session, remote_path) =
            create_remote_client("interchange-table-csv-remote", "main").expect("remote client");
        let remote_dry = import_csv_remote(
            &remote_client,
            &remote_session,
            "memory://items.csv",
            payload,
            true,
        )
        .expect("remote dry run");
        assert_eq!(remote_dry, local_dry);

        let written = import_csv_remote(
            &remote_client,
            &remote_session,
            "memory://items.csv",
            payload,
            false,
        )
        .expect("remote write");
        let report = generated_report(&written);
        assert_eq!(report.operations_applied, 2);
        assert!(report.commit.is_some());
        drop(remote_session);
        drop(remote_client);
        assert_eq!(
            query_items(&remote_path).expect("remote reopen query"),
            vec![
                (1, "alpha".to_string(), "remote".to_string()),
                (2, "beta".to_string(), "preserved".to_string()),
            ]
        );
    }

    #[test]
    fn import_redmine_profile_validates_infers_preserves_and_reopens() {
        let payload = br#"{
          "source_scope": "redmine://example",
          "projects": [{"id": 1, "identifier": "core", "key_prefix": "CORE", "name": "Core"}],
          "issues": [{
            "id": 42,
            "project_identifier": "core",
            "tracker": "Bug",
            "subject": "Login fails",
            "description": "Fails on Safari",
            "status": "New",
            "priority": "High",
            "custom_fields": {"severity": "critical"}
          }]
        }"#;
        let (client, session, _) =
            create_client("interchange-redmine-strict", "main").expect("client");
        let strict = import_profile_local(
            &client,
            &session,
            "import_redmine",
            payload,
            "strict",
            false,
        )
        .expect_err("strict rejects undeclared projection fields");
        assert_eq!(strict.code, Code::InvalidArgument);
        assert!(client.close(&session));

        let (client, session, path) =
            create_client("interchange-redmine-local", "main").expect("client");
        let dry = import_profile_local(&client, &session, "import_redmine", payload, "infer", true)
            .expect("dry run");
        let dry = generated_report(&dry);
        assert_eq!(dry.profile, "redmine");
        assert_eq!(dry.source_scope, "redmine://example");
        assert_eq!(dry.bytes_in, payload.len() as u64);
        assert_eq!(dry.operations_applied, 0);
        assert!(dry.dry_run);
        assert_empty_report_fidelity(&dry);
        assert!(client.close(&session));
        assert_no_ticket_profile(&path);

        let (client, session, path) =
            create_client("interchange-redmine-write", "main").expect("client");
        let written =
            import_profile_local(&client, &session, "import_redmine", payload, "infer", false)
                .expect("write import");
        let report = generated_report(&written);
        assert!(report.rows_imported >= 1);
        assert!(report.operations_applied >= 1);
        assert_empty_report_fidelity(&report);
        assert!(client.close(&session));
        let ticket = ticket_by_identity(&path, "redmine", "issue:42").expect("reopen ticket");
        assert_eq!(ticket.project_id, "core");
        assert_eq!(ticket.ticket_type, loom_tickets::TicketType::Bug);

        let invalid = br#"{"projects":[{"id":1,"name":"Core"}],"issues":[{"id":42}]}"#;
        let (client, session, _) =
            create_client("interchange-redmine-invalid", "main").expect("client");
        let missing =
            import_profile_local(&client, &session, "import_redmine", invalid, "infer", true)
                .expect_err("required fields rejected");
        assert_eq!(missing.code, Code::InvalidArgument);

        let (local_client, local_session, _) =
            create_client("interchange-redmine-parity-local", "main").expect("local client");
        let local_dry = import_profile_local(
            &local_client,
            &local_session,
            "import_redmine",
            payload,
            "infer",
            true,
        )
        .expect("local dry run");
        assert!(local_client.close(&local_session));
        let (remote_client, remote_session, remote_path) =
            create_remote_client("interchange-redmine-remote", "main").expect("remote client");
        let remote_dry = import_profile_remote(
            &remote_client,
            &remote_session,
            "import_redmine",
            payload,
            "infer",
            true,
        )
        .expect("remote dry run");
        assert_eq!(remote_dry, local_dry);
        import_profile_remote(
            &remote_client,
            &remote_session,
            "import_redmine",
            payload,
            "infer",
            false,
        )
        .expect("remote write");
        drop(remote_session);
        drop(remote_client);
        assert_eq!(
            ticket_by_identity(&remote_path, "redmine", "issue:42")
                .expect("remote reopen")
                .project_id,
            "core"
        );
    }

    #[test]
    fn import_asana_profile_validates_infers_preserves_and_reopens() {
        let payload = br#"{
          "source_scope": "asana://workspace",
          "projects": [{"gid": "p1", "key_prefix": "AS", "name": "Asana Project"}],
          "tasks": [{
            "gid": "t1",
            "project_gid": "p1",
            "name": "Ship importer",
            "notes": "Normalize Asana task data",
            "resource_subtype": "default_task",
            "completed": false,
            "custom_fields": {"size": "M"}
          }]
        }"#;
        let (client, session, _) =
            create_client("interchange-asana-strict", "main").expect("client");
        assert_eq!(
            import_profile_local(&client, &session, "import_asana", payload, "strict", false)
                .expect_err("strict rejection")
                .code,
            Code::InvalidArgument
        );
        assert!(client.close(&session));

        let (client, session, path) =
            create_client("interchange-asana-local", "main").expect("client");
        let dry = generated_report(
            &import_profile_local(&client, &session, "import_asana", payload, "infer", true)
                .expect("dry run"),
        );
        assert_eq!(dry.profile, "asana");
        assert_eq!(dry.source_scope, "asana://workspace");
        assert_eq!(dry.bytes_in, payload.len() as u64);
        assert_eq!(dry.operations_applied, 0);
        assert_empty_report_fidelity(&dry);
        assert!(client.close(&session));
        assert_no_ticket_profile(&path);

        let (client, session, path) =
            create_client("interchange-asana-write", "main").expect("client");
        let report = generated_report(
            &import_profile_local(&client, &session, "import_asana", payload, "infer", false)
                .expect("write import"),
        );
        assert!(report.operations_applied >= 1);
        assert_empty_report_fidelity(&report);
        assert!(client.close(&session));
        let ticket = ticket_by_identity(&path, "asana", "task:t1").expect("reopen ticket");
        assert_eq!(ticket.project_id, "p1");
        assert_eq!(
            ticket.fields.get("subject").unwrap().to_json(),
            serde_json::json!("Ship importer")
        );

        let invalid = br#"{"projects":[{"gid":"p1","name":"Project"}],"tasks":[{"gid":"t1"}]}"#;
        let (client, session, _) =
            create_client("interchange-asana-invalid", "main").expect("client");
        assert_eq!(
            import_profile_local(&client, &session, "import_asana", invalid, "infer", true)
                .expect_err("required fields rejected")
                .code,
            Code::InvalidArgument
        );

        let (local_client, local_session, _) =
            create_client("interchange-asana-parity-local", "main").expect("local client");
        let local_dry = import_profile_local(
            &local_client,
            &local_session,
            "import_asana",
            payload,
            "infer",
            true,
        )
        .expect("local dry run");
        assert!(local_client.close(&local_session));
        let (remote_client, remote_session, remote_path) =
            create_remote_client("interchange-asana-remote", "main").expect("remote client");
        let remote_dry = import_profile_remote(
            &remote_client,
            &remote_session,
            "import_asana",
            payload,
            "infer",
            true,
        )
        .expect("remote dry run");
        assert_eq!(remote_dry, local_dry);
        import_profile_remote(
            &remote_client,
            &remote_session,
            "import_asana",
            payload,
            "infer",
            false,
        )
        .expect("remote write");
        drop(remote_session);
        drop(remote_client);
        assert_eq!(
            ticket_by_identity(&remote_path, "asana", "task:t1")
                .expect("remote reopen")
                .project_id,
            "p1"
        );
    }

    #[test]
    fn import_jira_profile_validates_infers_preserves_and_reopens() {
        let payload = br#"{
          "source_scope": "jira://site",
          "projects": [{"id": 10001, "key": "CORE", "name": "Core"}],
          "issues": [{
            "id": 10042,
            "key": "CORE-42",
            "project_key": "CORE",
            "issue_type": "Bug",
            "summary": "Login fails",
            "description": "Fails on Safari",
            "status": "To Do",
            "priority": "High",
            "custom_fields": {"severity": "critical"}
          }]
        }"#;
        let (client, session, _) =
            create_client("interchange-jira-strict", "main").expect("client");
        assert_eq!(
            import_profile_local(&client, &session, "import_jira", payload, "strict", false)
                .expect_err("strict rejection")
                .code,
            Code::InvalidArgument
        );
        assert!(client.close(&session));

        let (client, session, path) =
            create_client("interchange-jira-local", "main").expect("client");
        let dry = generated_report(
            &import_profile_local(&client, &session, "import_jira", payload, "infer", true)
                .expect("dry run"),
        );
        assert_eq!(dry.profile, "jira");
        assert_eq!(dry.source_scope, "jira://site");
        assert_eq!(dry.bytes_in, payload.len() as u64);
        assert_eq!(dry.operations_applied, 0);
        assert_empty_report_fidelity(&dry);
        assert!(client.close(&session));
        assert_no_ticket_profile(&path);

        let (client, session, path) =
            create_client("interchange-jira-write", "main").expect("client");
        let report = generated_report(
            &import_profile_local(&client, &session, "import_jira", payload, "infer", false)
                .expect("write import"),
        );
        assert!(report.operations_applied >= 1);
        assert_empty_report_fidelity(&report);
        assert!(client.close(&session));
        let ticket = ticket_by_identity(&path, "jira", "issue:10042").expect("reopen ticket");
        assert_eq!(ticket.project_id, "CORE");
        assert_eq!(ticket.ticket_type, loom_tickets::TicketType::Bug);
        assert_eq!(
            ticket.fields.get("jira_issue_key").unwrap().to_json(),
            serde_json::json!("CORE-42")
        );

        let invalid =
            br#"{"projects":[{"id":10001,"key":"CORE","name":"Core"}],"issues":[{"id":10042}]}"#;
        let (client, session, _) =
            create_client("interchange-jira-invalid", "main").expect("client");
        assert_eq!(
            import_profile_local(&client, &session, "import_jira", invalid, "infer", true)
                .expect_err("required fields rejected")
                .code,
            Code::InvalidArgument
        );

        let (local_client, local_session, _) =
            create_client("interchange-jira-parity-local", "main").expect("local client");
        let local_dry = import_profile_local(
            &local_client,
            &local_session,
            "import_jira",
            payload,
            "infer",
            true,
        )
        .expect("local dry run");
        assert!(local_client.close(&local_session));
        let (remote_client, remote_session, remote_path) =
            create_remote_client("interchange-jira-remote", "main").expect("remote client");
        let remote_dry = import_profile_remote(
            &remote_client,
            &remote_session,
            "import_jira",
            payload,
            "infer",
            true,
        )
        .expect("remote dry run");
        assert_eq!(remote_dry, local_dry);
        import_profile_remote(
            &remote_client,
            &remote_session,
            "import_jira",
            payload,
            "infer",
            false,
        )
        .expect("remote write");
        drop(remote_session);
        drop(remote_client);
        assert_eq!(
            ticket_by_identity(&remote_path, "jira", "issue:10042")
                .expect("remote reopen")
                .project_id,
            "CORE"
        );
    }

    #[test]
    fn import_confluence_profile_preserves_hierarchy_reports_parity_and_reopens() {
        let payload = br#"{
          "source_scope": "confluence://site",
          "spaces": [{"id": "wiki", "name": "Wiki"}],
          "pages": [
            {"id": "home", "title": "Home", "space_id": "wiki", "storage_xhtml": "<p>Hello Confluence</p>"},
            {"id": "child", "title": "Child", "space_id": "wiki", "parent_page_id": "home", "text": "Child text"}
          ]
        }"#;
        let (client, session, path) =
            create_client("interchange-confluence-local", "main").expect("client");
        let dry = generated_report(
            &import_confluence_local(&client, &session, payload, true).expect("dry run"),
        );
        assert_eq!(dry.profile, "confluence");
        assert_eq!(dry.source_scope, "confluence://site");
        assert_eq!(dry.bytes_in, payload.len() as u64);
        assert_eq!(dry.operations_applied, 0);
        assert!(dry.dry_run);
        assert_empty_report_fidelity(&dry);
        assert!(client.close(&session));
        assert_no_page_profile(&path);

        let (client, session, path) =
            create_client("interchange-confluence-write", "main").expect("client");
        let report = generated_report(
            &import_confluence_local(&client, &session, payload, false).expect("write import"),
        );
        assert_eq!(report.rows_imported, 2);
        assert!(report.operations_applied >= 2);
        assert_empty_report_fidelity(&report);
        assert!(client.close(&session));
        assert_eq!(
            page_opaque_body(&path, "home", "confluence.storage").expect("reopen home"),
            b"<p>Hello Confluence</p>"
        );
        assert_eq!(
            page_body_text(&path, "child")
                .expect("reopen child")
                .1
                .as_deref(),
            Some("home")
        );

        let (local_client, local_session, _) =
            create_client("interchange-confluence-parity-local", "main").expect("local client");
        let local_dry =
            import_confluence_local(&local_client, &local_session, payload, true).expect("local");
        assert!(local_client.close(&local_session));
        let (remote_client, remote_session, remote_path) =
            create_remote_client("interchange-confluence-remote", "main").expect("remote client");
        let remote_dry = import_confluence_remote(&remote_client, &remote_session, payload, true)
            .expect("remote dry run");
        assert_eq!(remote_dry, local_dry);
        import_confluence_remote(&remote_client, &remote_session, payload, false)
            .expect("remote write");
        drop(remote_session);
        drop(remote_client);
        assert_eq!(
            page_opaque_body(&remote_path, "home", "confluence.storage").expect("remote reopen"),
            b"<p>Hello Confluence</p>"
        );
    }

    #[test]
    fn import_slack_profile_accepts_json_and_zip_reports_parity_and_reopens() {
        let payload = br#"{
          "source_scope": "slack://workspace",
          "channels": [{"id": "C123", "name": "general", "members": ["U1", "U2"]}],
          "messages": [
            {"channel_id": "C123", "ts": "1710000000.000100", "user": "U1", "text": "Hello from Slack", "reactions": [{"name": "wave", "users": ["U2"]}]},
            {"channel_id": "C123", "ts": "1710000001.000200", "thread_ts": "1710000000.000100", "user": "U2", "text": "Thread reply"}
          ]
        }"#;
        let (client, session, path) =
            create_client("interchange-slack-local", "main").expect("client");
        let dry = generated_report(
            &import_slack_local(&client, &session, "memory://slack.json", payload, true)
                .expect("dry run"),
        );
        assert_eq!(dry.profile, "slack");
        assert_eq!(dry.source_scope, "slack://workspace");
        assert_eq!(dry.bytes_in, payload.len() as u64);
        assert_eq!(dry.operations_applied, 0);
        assert_eq!(
            fidelity_tuples(&dry),
            vec![
                (
                    "channel:C123".to_string(),
                    "members".to_string(),
                    "Slack channel membership is not lowered by this importer slice".to_string(),
                ),
                (
                    expected_slack_message_source("C123", "1710000000.000100"),
                    "user".to_string(),
                    "Slack message user is not lowered as a principal by this importer slice"
                        .to_string(),
                ),
                (
                    expected_slack_message_source("C123", "1710000000.000100"),
                    "reaction_users".to_string(),
                    "Slack per-user reaction authorship is not lowered by this importer slice"
                        .to_string(),
                ),
                (
                    expected_slack_message_source("C123", "1710000001.000200"),
                    "user".to_string(),
                    "Slack message user is not lowered as a principal by this importer slice"
                        .to_string(),
                ),
            ]
        );
        assert!(client.close(&session));
        assert_no_chat_profile(&path);

        let (client, session, path) =
            create_client("interchange-slack-write", "main").expect("client");
        let report = generated_report(
            &import_slack_local(&client, &session, "memory://slack.json", payload, false)
                .expect("write import"),
        );
        assert_eq!(report.rows_imported, 2);
        assert!(report.operations_applied >= 2);
        assert_eq!(fidelity_tuples(&report), fidelity_tuples(&dry));
        assert!(client.close(&session));
        let (texts, threads) = chat_projection_texts(&path).expect("reopen chat");
        assert_eq!(texts, vec!["Hello from Slack", "Thread reply"]);
        assert_eq!(threads, 1);

        let slack_zip = zip_bytes(&[
            (
                "channels.json",
                br#"[{"id":"CZIP","name":"general","members":["U1"]}]"#,
            ),
            (
                "general/2024-01-01.json",
                br#"[{"ts":"1710000100.000100","user":"U1","text":"Hello from zip","reactions":[{"name":"wave","users":["U2"]}]}]"#,
            ),
        ]);
        let (zip_client, zip_session, zip_path) =
            create_client("interchange-slack-zip", "main").expect("client");
        let zip_report = generated_report(
            &import_slack_local(
                &zip_client,
                &zip_session,
                "memory://slack.zip",
                &slack_zip,
                false,
            )
            .expect("zip write"),
        );
        assert_eq!(zip_report.rows_imported, 1);
        assert_eq!(
            fidelity_tuples(&zip_report),
            vec![
                (
                    "channel:CZIP".to_string(),
                    "members".to_string(),
                    "Slack channel membership is not lowered by this importer slice".to_string(),
                ),
                (
                    expected_slack_message_source("CZIP", "1710000100.000100"),
                    "user".to_string(),
                    "Slack message user is not lowered as a principal by this importer slice"
                        .to_string(),
                ),
                (
                    expected_slack_message_source("CZIP", "1710000100.000100"),
                    "reaction_users".to_string(),
                    "Slack per-user reaction authorship is not lowered by this importer slice"
                        .to_string(),
                ),
            ]
        );
        assert!(zip_client.close(&zip_session));
        assert_eq!(
            chat_projection_texts(&zip_path).expect("zip reopen").0,
            vec!["Hello from zip"]
        );

        let (local_client, local_session, _) =
            create_client("interchange-slack-parity-local", "main").expect("local client");
        let local_dry = import_slack_local(
            &local_client,
            &local_session,
            "memory://slack.json",
            payload,
            true,
        )
        .expect("local dry");
        assert!(local_client.close(&local_session));
        let (remote_client, remote_session, remote_path) =
            create_remote_client("interchange-slack-remote", "main").expect("remote client");
        let remote_dry = import_slack_remote(
            &remote_client,
            &remote_session,
            "memory://slack.json",
            payload,
            true,
        )
        .expect("remote dry");
        assert_eq!(remote_dry, local_dry);
        assert_eq!(
            fidelity_tuples(&generated_report(&remote_dry)),
            fidelity_tuples(&generated_report(&local_dry))
        );
        import_slack_remote(
            &remote_client,
            &remote_session,
            "memory://slack.json",
            payload,
            false,
        )
        .expect("remote write");
        drop(remote_session);
        drop(remote_client);
        assert_eq!(
            chat_projection_texts(&remote_path)
                .expect("remote reopen")
                .0,
            vec!["Hello from Slack", "Thread reply"]
        );
    }

    #[test]
    fn import_drive_profile_archive_validates_reports_parity_and_reopens() {
        let manifest = br#"{
          "source_scope": "drive://export",
          "folders": [{"id": "docs", "parent_id": "root", "name": "Docs"}],
          "files": [
            {"id": "readme", "parent_id": "docs", "name": "README.md", "text": "Inline text"},
            {"id": "binary", "parent_id": "docs", "name": "binary.bin", "content_hex": "000102ff"},
            {"id": "payload", "parent_id": "docs", "name": "payload.txt", "content_path": "payloads/./payload.txt"}
          ]
        }"#;
        let archive = drive_archive(
            manifest,
            &[
                ("payloads/", b""),
                ("payloads/payload.txt", b"Archive body"),
            ],
        );
        let (client, session, path) =
            create_client("interchange-drive-local", "main").expect("client");
        let dry =
            generated_report(&import_drive_local(&client, &session, &archive, true).expect("dry"));
        assert_eq!(dry.profile, "drive");
        assert_eq!(dry.source_scope, "drive://export");
        assert_eq!(dry.bytes_in, archive.len() as u64);
        assert_eq!(dry.operations_applied, 0);
        assert_empty_report_fidelity(&dry);
        assert!(client.close(&session));
        assert_no_drive_profile(&path);

        let (client, session, path) =
            create_client("interchange-drive-write", "main").expect("client");
        let report = generated_report(
            &import_drive_local(&client, &session, &archive, false).expect("write"),
        );
        assert_eq!(report.rows_imported, 4);
        assert!(report.operations_applied >= 4);
        assert_empty_report_fidelity(&report);
        assert!(client.close(&session));
        assert_eq!(
            drive_folder_names(&path, "docs").expect("folder reopen"),
            vec!["README.md", "binary.bin", "payload.txt"]
        );
        assert_eq!(
            drive_file_bytes(&path, "readme").expect("inline reopen"),
            b"Inline text"
        );
        assert_eq!(
            drive_file_bytes(&path, "binary").expect("hex reopen"),
            [0, 1, 2, 255]
        );
        assert_eq!(
            drive_file_bytes(&path, "payload").expect("archive reopen"),
            b"Archive body"
        );

        let missing = drive_archive(manifest, &[]);
        let (bad_client, bad_session, _) =
            create_client("interchange-drive-missing", "main").expect("client");
        assert_eq!(
            import_drive_local(&bad_client, &bad_session, &missing, true)
                .expect_err("missing content")
                .code,
            Code::InvalidArgument
        );

        let traversal = drive_archive(
            br#"{"files":[{"id":"payload","name":"payload.txt","content_path":"../payload.txt"}]}"#,
            &[("payload.txt", b"payload")],
        );
        assert_eq!(
            import_drive_local(&bad_client, &bad_session, &traversal, true)
                .expect_err("traversal")
                .code,
            Code::InvalidArgument
        );

        let duplicate = drive_archive(
            br#"{"files":[{"id":"payload","name":"payload.txt","content_path":"payload.txt"}]}"#,
            &[("payload.txt", b"one"), ("./payload.txt", b"two")],
        );
        assert_eq!(
            import_drive_local(&bad_client, &bad_session, &duplicate, true)
                .expect_err("duplicate")
                .code,
            Code::InvalidArgument
        );

        let undeclared = drive_archive(
            br#"{"files":[{"id":"payload","name":"payload.txt","content_path":"payload.txt"}]}"#,
            &[("payload.txt", b"payload"), ("extra.txt", b"extra")],
        );
        assert_eq!(
            import_drive_local(&bad_client, &bad_session, &undeclared, true)
                .expect_err("undeclared")
                .code,
            Code::InvalidArgument
        );

        let directory_collision = drive_archive(
            br#"{"files":[{"id":"payload","name":"payload.txt","content_path":"payload.txt"}]}"#,
            &[("payload.txt/", b""), ("payload.txt", b"payload")],
        );
        assert_eq!(
            import_drive_local(&bad_client, &bad_session, &directory_collision, true)
                .expect_err("directory collision")
                .code,
            Code::InvalidArgument
        );

        let oversized = vec![0u8; 64 * 1024 * 1024 + 1];
        assert_eq!(
            import_drive_local(&bad_client, &bad_session, &oversized, true)
                .expect_err("archive byte limit")
                .code,
            Code::InvalidArgument
        );
        assert!(bad_client.close(&bad_session));

        let (local_client, local_session, _) =
            create_client("interchange-drive-parity-local", "main").expect("local client");
        let local_dry =
            import_drive_local(&local_client, &local_session, &archive, true).expect("local dry");
        assert!(local_client.close(&local_session));
        let (remote_client, remote_session, remote_path) =
            create_remote_client("interchange-drive-remote", "main").expect("remote client");
        let remote_dry = import_drive_remote(&remote_client, &remote_session, &archive, true)
            .expect("remote dry");
        assert_eq!(remote_dry, local_dry);
        import_drive_remote(&remote_client, &remote_session, &archive, false)
            .expect("remote write");
        drop(remote_session);
        drop(remote_client);
        assert_eq!(
            drive_file_bytes(&remote_path, "payload").expect("remote reopen"),
            b"Archive body"
        );
    }

    #[test]
    fn import_markdown_profile_preserves_hierarchy_fidelity_parity_and_reopens() {
        let archive = zip_bytes(&[
            (".obsidian/", b""),
            (".obsidian/app.json", br#"{"legacyEditor":true}"#),
            (
                "Intro.md",
                b"---\ntags: [demo]\n---\n# Intro\nWelcome [[Setup]].\n",
            ),
            ("guides/", b""),
            ("guides/Setup.md", b"# Setup\nRun `loom init`.\n"),
            ("board.canvas", br#"{"nodes":[]}"#),
        ]);
        let (client, session, path) =
            create_client("interchange-markdown-local", "main").expect("client");
        let dry = generated_report(
            &import_markdown_local(&client, &session, &archive, true).expect("dry run"),
        );
        assert_eq!(dry.profile, "markdown");
        assert_eq!(dry.source_scope, "memory://markdown.zip");
        assert_eq!(dry.bytes_in, archive.len() as u64);
        assert_eq!(dry.operations_applied, 0);
        assert_eq!(
            fidelity_tuples(&dry),
            vec![
                (
                    "vault".to_string(),
                    "canvas".to_string(),
                    "Markdown or Obsidian vault construct is not lowered by this importer slice"
                        .to_string(),
                ),
                (
                    "vault".to_string(),
                    "obsidian-config".to_string(),
                    "Markdown or Obsidian vault construct is not lowered by this importer slice"
                        .to_string(),
                ),
                (
                    "page:Intro.md".to_string(),
                    "frontmatter".to_string(),
                    "Markdown or Obsidian construct is not lowered by this importer slice"
                        .to_string(),
                ),
                (
                    "page:Intro.md".to_string(),
                    "wikilinks".to_string(),
                    "Markdown or Obsidian construct is not lowered by this importer slice"
                        .to_string(),
                ),
                (
                    "page:guides/Setup.md".to_string(),
                    "inline-code".to_string(),
                    "Markdown or Obsidian construct is not lowered by this importer slice"
                        .to_string(),
                ),
            ]
        );
        assert!(client.close(&session));
        assert_no_page_profile(&path);

        let (client, session, path) =
            create_client("interchange-markdown-write", "main").expect("client");
        let report = generated_report(
            &import_markdown_local(&client, &session, &archive, false).expect("write import"),
        );
        assert_eq!(report.rows_imported, 2);
        assert!(report.operations_applied >= 2);
        assert_eq!(fidelity_tuples(&report), fidelity_tuples(&dry));
        assert!(client.close(&session));
        assert!(
            page_body_text(&path, "intro")
                .expect("intro reopen")
                .0
                .contains("Welcome [[Setup]].")
        );
        assert!(
            page_body_text(&path, "guides-setup")
                .expect("setup reopen")
                .0
                .contains("Run `loom init`.")
        );
        assert_eq!(page_history_len(&path, "pages", "intro"), 1);

        let (local_client, local_session, _) =
            create_client("interchange-markdown-parity-local", "main").expect("local client");
        let local_dry = import_markdown_local(&local_client, &local_session, &archive, true)
            .expect("local dry");
        assert!(local_client.close(&local_session));
        let (remote_client, remote_session, remote_path) =
            create_remote_client("interchange-markdown-remote", "main").expect("remote client");
        let remote_dry = import_markdown_remote(&remote_client, &remote_session, &archive, true)
            .expect("remote dry");
        assert_eq!(remote_dry, local_dry);
        import_markdown_remote(&remote_client, &remote_session, &archive, false)
            .expect("remote write");
        drop(remote_session);
        drop(remote_client);
        assert!(
            page_body_text(&remote_path, "intro")
                .expect("remote reopen")
                .0
                .contains("Welcome [[Setup]].")
        );
    }

    #[test]
    fn import_notion_profile_preserves_hierarchy_fidelity_parity_and_reopens() {
        let payload = br##"{
          "source_scope": "notion://workspace",
          "pages": [
            {
              "id": "page-intro",
              "title": "Intro",
              "space_id": "notion",
              "markdown": "# Intro\nWelcome to Notion.",
              "database": {"id": "db1"},
              "users": [{"id": "user1"}],
              "synced_blocks": [{"id": "sync1"}]
            },
            {
              "id": "child",
              "title": "Child",
              "space_id": "notion",
              "parent_page_id": "page-intro",
              "text": "Child text",
              "comments": [{"id": "comment1"}]
            }
          ]
        }"##;
        let (client, session, path) =
            create_client("interchange-notion-local", "main").expect("client");
        let dry = generated_report(
            &import_notion_local(&client, &session, payload, true).expect("dry run"),
        );
        assert_eq!(dry.profile, "notion");
        assert_eq!(dry.source_scope, "notion://workspace");
        assert_eq!(dry.bytes_in, payload.len() as u64);
        assert_eq!(dry.operations_applied, 0);
        assert_eq!(
            fidelity_tuples(&dry),
            vec![
                (
                    "page:page-intro".to_string(),
                    "database".to_string(),
                    "Notion databases are not lowered by this importer slice".to_string(),
                ),
                (
                    "page:page-intro".to_string(),
                    "synced_blocks".to_string(),
                    "Notion synced blocks are not lowered by this importer slice".to_string(),
                ),
                (
                    "page:page-intro".to_string(),
                    "users".to_string(),
                    "Notion users are not mapped to principals by this importer slice".to_string(),
                ),
                (
                    "page:child".to_string(),
                    "comments".to_string(),
                    "Notion comments are not lowered by this importer slice".to_string(),
                ),
            ]
        );
        assert!(client.close(&session));
        assert_no_page_profile(&path);

        let (client, session, path) =
            create_client("interchange-notion-write", "main").expect("client");
        let report = generated_report(
            &import_notion_local(&client, &session, payload, false).expect("write import"),
        );
        assert_eq!(report.rows_imported, 2);
        assert!(report.operations_applied >= 2);
        assert_eq!(fidelity_tuples(&report), fidelity_tuples(&dry));
        assert!(client.close(&session));
        assert!(
            page_body_text(&path, "page-intro")
                .expect("intro reopen")
                .0
                .contains("Welcome to Notion.")
        );
        assert_eq!(
            page_body_text(&path, "child")
                .expect("child reopen")
                .1
                .as_deref(),
            Some("page-intro")
        );
        assert_eq!(page_history_len(&path, "pages", "page-intro"), 1);

        let (local_client, local_session, _) =
            create_client("interchange-notion-parity-local", "main").expect("local client");
        let local_dry =
            import_notion_local(&local_client, &local_session, payload, true).expect("local dry");
        assert!(local_client.close(&local_session));
        let (remote_client, remote_session, remote_path) =
            create_remote_client("interchange-notion-remote", "main").expect("remote client");
        let remote_dry = import_notion_remote(&remote_client, &remote_session, payload, true)
            .expect("remote dry");
        assert_eq!(remote_dry, local_dry);
        import_notion_remote(&remote_client, &remote_session, payload, false)
            .expect("remote write");
        drop(remote_session);
        drop(remote_client);
        assert!(
            page_body_text(&remote_path, "page-intro")
                .expect("remote reopen")
                .0
                .contains("Welcome to Notion.")
        );
    }

    #[test]
    fn columnar_import_arrow_generated_contract_preserves_rows_parity_and_reopens() {
        let payload = sample_arrow_bytes();
        let (client, session, path) =
            create_client("columnar-import-arrow-dry", "main").expect("client");
        let dry = columnar_generated_import_report(
            &import_arrow_local(&client, &session, "events", &payload, false, true)
                .expect("dry import"),
        );
        assert_eq!(dry.format, "arrow-ipc");
        assert_eq!(
            dry.columns,
            vec![("id".to_string(), 1), ("label".to_string(), 3)]
        );
        assert_eq!(dry.rows, 2);
        assert_eq!(dry.bytes_in, payload.len() as u64);
        assert!(dry.dry_run);
        assert!(!dry.replaced);
        assert!(client.close(&session));
        assert_eq!(
            query_columnar_rows(&path, "events")
                .expect_err("dry run must not save")
                .code,
            Code::NotFound
        );

        let (client, session, path) =
            create_client("columnar-import-arrow-write", "main").expect("client");
        let write = columnar_generated_import_report(
            &import_arrow_local(&client, &session, "events", &payload, false, false)
                .expect("write import"),
        );
        assert!(!write.dry_run);
        assert_eq!(write.rows, 2);
        assert!(client.close(&session));
        assert_eq!(
            query_columnar_rows(&path, "events").expect("reopen rows"),
            vec![
                vec![Value::Int(1), Value::Text("alpha".to_string())],
                vec![Value::Int(2), Value::Text("beta".to_string())],
            ]
        );

        let (bad_client, bad_session, _) =
            create_client("columnar-import-arrow-bad", "main").expect("bad client");
        assert_eq!(
            import_arrow_local(
                &bad_client,
                &bad_session,
                "events",
                b"not arrow",
                false,
                true
            )
            .expect_err("malformed arrow")
            .code,
            Code::InvalidArgument
        );
        assert!(bad_client.close(&bad_session));

        let (local_client, local_session, _) =
            create_client("columnar-import-arrow-parity-local", "main").expect("local client");
        let local_dry = import_arrow_local(
            &local_client,
            &local_session,
            "events",
            &payload,
            false,
            true,
        )
        .expect("local dry");
        assert!(local_client.close(&local_session));
        let (remote_client, remote_session, remote_path) =
            create_remote_client("columnar-import-arrow-remote", "main").expect("remote client");
        let remote_dry = import_arrow_remote(
            &remote_client,
            &remote_session,
            "events",
            &payload,
            false,
            true,
        )
        .expect("remote dry");
        assert_eq!(remote_dry, local_dry);
        import_arrow_remote(
            &remote_client,
            &remote_session,
            "events",
            &payload,
            false,
            false,
        )
        .expect("remote write");
        drop(remote_session);
        drop(remote_client);
        assert_eq!(
            query_columnar_rows(&remote_path, "events").expect("remote reopen rows"),
            vec![
                vec![Value::Int(1), Value::Text("alpha".to_string())],
                vec![Value::Int(2), Value::Text("beta".to_string())],
            ]
        );
    }

    #[test]
    fn columnar_import_parquet_generated_contract_preserves_rows_parity_and_reopens() {
        let payload = sample_parquet_bytes();
        let (client, session, path) =
            create_client("columnar-import-parquet-dry", "main").expect("client");
        let dry = columnar_generated_import_report(
            &import_parquet_local(&client, &session, "events", &payload, false, true)
                .expect("dry import"),
        );
        assert_eq!(dry.format, "parquet");
        assert_eq!(
            dry.columns,
            vec![("id".to_string(), 1), ("label".to_string(), 3)]
        );
        assert_eq!(dry.rows, 2);
        assert_eq!(dry.bytes_in, payload.len() as u64);
        assert!(dry.dry_run);
        assert!(!dry.replaced);
        assert!(client.close(&session));
        assert_eq!(
            query_columnar_rows(&path, "events")
                .expect_err("dry run must not save")
                .code,
            Code::NotFound
        );

        let (client, session, path) =
            create_client("columnar-import-parquet-write", "main").expect("client");
        let write = columnar_generated_import_report(
            &import_parquet_local(&client, &session, "events", &payload, false, false)
                .expect("write import"),
        );
        assert!(!write.dry_run);
        assert_eq!(write.rows, 2);
        assert!(client.close(&session));
        assert_eq!(
            query_columnar_rows(&path, "events").expect("reopen rows"),
            vec![
                vec![Value::Int(1), Value::Text("alpha".to_string())],
                vec![Value::Int(2), Value::Text("beta".to_string())],
            ]
        );

        let (bad_client, bad_session, _) =
            create_client("columnar-import-parquet-bad", "main").expect("bad client");
        assert_eq!(
            import_parquet_local(
                &bad_client,
                &bad_session,
                "events",
                b"not parquet",
                false,
                true,
            )
            .expect_err("malformed parquet")
            .code,
            Code::InvalidArgument
        );
        assert!(bad_client.close(&bad_session));

        let (local_client, local_session, _) =
            create_client("columnar-import-parquet-parity-local", "main").expect("local client");
        let local_dry = import_parquet_local(
            &local_client,
            &local_session,
            "events",
            &payload,
            false,
            true,
        )
        .expect("local dry");
        assert!(local_client.close(&local_session));
        let (remote_client, remote_session, remote_path) =
            create_remote_client("columnar-import-parquet-remote", "main").expect("remote client");
        let remote_dry = import_parquet_remote(
            &remote_client,
            &remote_session,
            "events",
            &payload,
            false,
            true,
        )
        .expect("remote dry");
        assert_eq!(remote_dry, local_dry);
        import_parquet_remote(
            &remote_client,
            &remote_session,
            "events",
            &payload,
            false,
            false,
        )
        .expect("remote write");
        drop(remote_session);
        drop(remote_client);
        assert_eq!(
            query_columnar_rows(&remote_path, "events").expect("remote reopen rows"),
            vec![
                vec![Value::Int(1), Value::Text("alpha".to_string())],
                vec![Value::Int(2), Value::Text("beta".to_string())],
            ]
        );
    }

    #[test]
    fn vector_text_upsert_generated_contract_preserves_source_metadata_conflicts_and_reopens() {
        let metadata = vector_metadata_cbor("blue");
        let (client, session, path) =
            create_client("vector-text-upsert-local", "main").expect("client");
        let report = vector_text_upsert_local(
            &client,
            &session,
            VectorTextUpsertArgs {
                id: "doc-1",
                vector: &[1.0, 0.0],
                metadata: &metadata,
                source_text: b"hello vector",
                model_id: "model-a",
                weights_digest: Some("sha256:aaa"),
                create: true,
                expected_token: None,
                expect_absent: false,
            },
        )
        .expect("local upsert");
        let report = loom_wire::vector::text_upsert_report_from_cbor(&report).expect("report");
        assert_eq!(report.id, "doc-1");
        assert_eq!(report.collection, "notes");
        assert!(!report.current_token.is_empty());
        let expected_token_len = loom_types::ContentTag::new(loom_core::Digest::hash(
            loom_core::Algo::Blake3,
            b"same-length-reference",
        ))
        .to_entity_tag()
        .as_bytes()
        .len();
        assert_eq!(report.current_token.len(), expected_token_len);
        for needle in [
            b"hello vector".as_slice(),
            b"model-a".as_slice(),
            b"sha256:aaa".as_slice(),
            b"blue".as_slice(),
            b"doc-1".as_slice(),
            b"notes".as_slice(),
            b"main".as_slice(),
            &1.0f32.to_le_bytes(),
        ] {
            assert!(
                !report
                    .current_token
                    .windows(needle.len())
                    .any(|window| window == needle),
                "token disclosed {:?}",
                String::from_utf8_lossy(needle)
            );
        }
        let first_token = report.current_token.clone();
        let absent_error = vector_text_upsert_local(
            &client,
            &session,
            VectorTextUpsertArgs {
                id: "doc-1",
                vector: &[0.0, 1.0],
                metadata: &metadata,
                source_text: b"should not apply",
                model_id: "model-a",
                weights_digest: Some("sha256:aaa"),
                create: false,
                expected_token: None,
                expect_absent: true,
            },
        )
        .expect_err("create-if-absent existing conflict");
        assert_eq!(absent_error.code, Code::AlreadyExists);
        let stale_error = vector_text_upsert_local(
            &client,
            &session,
            VectorTextUpsertArgs {
                id: "doc-1",
                vector: &[0.0, 1.0],
                metadata: &metadata,
                source_text: b"should not apply",
                model_id: "model-a",
                weights_digest: Some("sha256:aaa"),
                create: false,
                expected_token: Some(b"stale".to_vec()),
                expect_absent: false,
            },
        )
        .expect_err("stale token conflict");
        assert_eq!(stale_error.code, Code::Conflict);
        let replace_report = vector_text_upsert_local(
            &client,
            &session,
            VectorTextUpsertArgs {
                id: "doc-1",
                vector: &[0.0, 1.0],
                metadata: &metadata,
                source_text: b"replacement vector",
                model_id: "model-a",
                weights_digest: Some("sha256:aaa"),
                create: false,
                expected_token: Some(first_token.clone()),
                expect_absent: false,
            },
        )
        .expect("matching token replacement");
        let replace_report =
            loom_wire::vector::text_upsert_report_from_cbor(&replace_report).expect("replace");
        assert!(!replace_report.current_token.is_empty());
        assert_ne!(replace_report.current_token, first_token);
        let second_token = replace_report.current_token.clone();
        let red_metadata = vector_metadata_cbor("red");
        let metadata_report = vector_text_upsert_local(
            &client,
            &session,
            VectorTextUpsertArgs {
                id: "doc-1",
                vector: &[0.0, 1.0],
                metadata: &red_metadata,
                source_text: b"replacement vector",
                model_id: "model-a",
                weights_digest: Some("sha256:aaa"),
                create: false,
                expected_token: Some(second_token),
                expect_absent: false,
            },
        )
        .expect("metadata change");
        let metadata_report =
            loom_wire::vector::text_upsert_report_from_cbor(&metadata_report).expect("metadata");
        assert_ne!(metadata_report.current_token, replace_report.current_token);
        let (other_client, other_session, _) =
            create_client("vector-text-upsert-token-model", "main").expect("other client");
        let other_report = vector_text_upsert_local(
            &other_client,
            &other_session,
            VectorTextUpsertArgs {
                id: "doc-1",
                vector: &[1.0, 0.0],
                metadata: &metadata,
                source_text: b"hello vector",
                model_id: "model-c",
                weights_digest: Some("sha256:ccc"),
                create: true,
                expected_token: None,
                expect_absent: true,
            },
        )
        .expect("other model");
        let other_report =
            loom_wire::vector::text_upsert_report_from_cbor(&other_report).expect("other model");
        assert_ne!(other_report.current_token, first_token);
        assert!(other_client.close(&other_session));
        assert_eq!(
            vector_text_upsert_local(
                &client,
                &session,
                VectorTextUpsertArgs {
                    id: "doc-2",
                    vector: &[0.0, 1.0],
                    metadata: &metadata,
                    source_text: b"conflicting model",
                    model_id: "model-b",
                    weights_digest: Some("sha256:bbb"),
                    create: false,
                    expected_token: None,
                    expect_absent: true,
                },
            )
            .expect_err("model conflict")
            .code,
            Code::Conflict
        );
        assert_eq!(
            vector_text_upsert_local(
                &client,
                &session,
                VectorTextUpsertArgs {
                    id: "bad",
                    vector: &[1.0],
                    metadata: b"not cbor",
                    source_text: b"bad metadata",
                    model_id: "model-a",
                    weights_digest: Some("sha256:aaa"),
                    create: false,
                    expected_token: None,
                    expect_absent: false,
                },
            )
            .expect_err("malformed metadata")
            .code,
            Code::CorruptObject
        );
        assert!(client.close(&session));

        let (vector, stored_metadata) =
            query_vector_entry(&path, "doc-1").expect("reopened vector");
        assert_eq!(vector, vec![0.0, 1.0]);
        assert_eq!(
            stored_metadata.get("label"),
            Some(&Value::Text("red".to_string()))
        );
        assert_eq!(
            query_vector_source(&path, "doc-1").expect("reopened source"),
            "replacement vector"
        );
        let model = query_vector_model(&path)
            .expect("reopened model")
            .expect("model profile");
        assert_eq!(model.model_id, "model-a");
        assert_eq!(model.weights_digest.as_deref(), Some("sha256:aaa"));

        let (rollback_client, rollback_session, rollback_path) =
            create_client("vector-text-upsert-rollback", "main").expect("rollback client");
        let rollback_error = vector_text_upsert_local(
            &rollback_client,
            &rollback_session,
            VectorTextUpsertArgs {
                id: "doc-1",
                vector: &[1.0, 0.0],
                metadata: &metadata,
                source_text: b"must not create",
                model_id: "model-a",
                weights_digest: Some("sha256:aaa"),
                create: true,
                expected_token: Some(b"stale".to_vec()),
                expect_absent: false,
            },
        )
        .expect_err("missing collection exact token conflict");
        assert_eq!(rollback_error.code, Code::Conflict);
        assert!(
            rollback_client
                .with_session(&rollback_session, |loom| loom
                    .registry()
                    .open(&loom_core::WsSelector::Typed {
                        ty: FacetKind::Vector,
                        name: "main".to_string(),
                    })
                    .map(|_| ()))
                .is_err(),
            "rejected create=true exact-token request mutated the live session"
        );
        assert!(rollback_client.close(&rollback_session));
        let reopened_rollback = loom_client::LocalLoomClient::new(&rollback_path);
        let reopened_rollback_session = reopened_rollback.open().expect("rollback reopen");
        assert!(
            reopened_rollback
                .with_session(&reopened_rollback_session, |loom| loom
                    .registry()
                    .open(&loom_core::WsSelector::Typed {
                        ty: FacetKind::Vector,
                        name: "main".to_string(),
                    })
                    .map(|_| ()))
                .is_err(),
            "rejected create=true exact-token request mutated durable state"
        );
        assert!(reopened_rollback.close(&reopened_rollback_session));

        let (local_client, local_session, _) =
            create_client("vector-text-upsert-parity-local", "main").expect("local client");
        let local_report = vector_text_upsert_local(
            &local_client,
            &local_session,
            VectorTextUpsertArgs {
                id: "doc-1",
                vector: &[1.0, 0.0],
                metadata: &metadata,
                source_text: b"hello vector",
                model_id: "model-a",
                weights_digest: Some("sha256:aaa"),
                create: true,
                expected_token: None,
                expect_absent: true,
            },
        )
        .expect("local parity upsert");
        assert!(local_client.close(&local_session));
        let (remote_client, remote_session, remote_path) =
            create_remote_client("vector-text-upsert-remote", "main").expect("remote client");
        let remote_report = vector_text_upsert_remote(
            &remote_client,
            &remote_session,
            VectorTextUpsertArgs {
                id: "doc-1",
                vector: &[1.0, 0.0],
                metadata: &metadata,
                source_text: b"hello vector",
                model_id: "model-a",
                weights_digest: Some("sha256:aaa"),
                create: true,
                expected_token: None,
                expect_absent: true,
            },
        )
        .expect("remote upsert");
        assert_eq!(remote_report, local_report);
        let remote_report_token = loom_wire::vector::text_upsert_report_from_cbor(&remote_report)
            .expect("remote report")
            .current_token;
        assert_eq!(
            vector_text_upsert_remote(
                &remote_client,
                &remote_session,
                VectorTextUpsertArgs {
                    id: "doc-1",
                    vector: &[0.0, 1.0],
                    metadata: &metadata,
                    source_text: b"remote stale",
                    model_id: "model-a",
                    weights_digest: Some("sha256:aaa"),
                    create: false,
                    expected_token: Some(b"stale".to_vec()),
                    expect_absent: false,
                },
            )
            .expect_err("remote stale token")
            .code,
            Code::Conflict
        );
        vector_text_upsert_remote(
            &remote_client,
            &remote_session,
            VectorTextUpsertArgs {
                id: "doc-1",
                vector: &[0.0, 1.0],
                metadata: &metadata,
                source_text: b"remote replacement",
                model_id: "model-a",
                weights_digest: Some("sha256:aaa"),
                create: false,
                expected_token: Some(remote_report_token),
                expect_absent: false,
            },
        )
        .expect("remote matching token replacement");
        drop(remote_session);
        drop(remote_client);
        assert_eq!(
            query_vector_source(&remote_path, "doc-1").expect("remote reopen"),
            "remote replacement"
        );
    }

    #[test]
    fn vector_workspace_configure_generated_contract_persists_validation_and_remote_parity() {
        let (client, session, path) =
            create_client("vector-workspace-configure-local", "main").expect("client");
        assert!(client.close(&session));
        seed_inference_instance_state(&path, loom_types::InferenceModelKind::TextEmbedding);
        let client = loom_client::LocalLoomClient::new(&path);
        let session = client.open().expect("reopen client");
        let request = r#"{"embedding-instance":"embed"}"#;
        let binding_json =
            vector_workspace_configure_local(&client, &session, request).expect("configure");
        assert!(binding_json.contains("\"embedding-instance\":\"embed\""));
        assert!(client.close(&session));
        let loom = open_loom_unlocked(&path, None).expect("open configured store");
        let workspace = loom
            .registry()
            .open(&loom_core::WsSelector::Name("main".to_string()))
            .expect("workspace");
        let state = loom_core::inference_instance_state(&loom, workspace).expect("state");
        assert_eq!(state.vector_bindings.len(), 1);
        assert_eq!(state.vector_bindings[0].workspace, workspace.to_string());
        assert_eq!(state.vector_bindings[0].embedding_instance, "embed");

        let (bad_client, bad_session, bad_path) =
            create_client("vector-workspace-configure-bad", "main").expect("bad client");
        assert!(bad_client.close(&bad_session));
        seed_inference_instance_state(&bad_path, loom_types::InferenceModelKind::Llm);
        let bad_client = loom_client::LocalLoomClient::new(&bad_path);
        let bad_session = bad_client.open().expect("bad reopen");
        assert_eq!(
            vector_workspace_configure_local(&bad_client, &bad_session, "{}")
                .expect_err("malformed request")
                .code,
            Code::InvalidArgument
        );
        assert_eq!(
            vector_workspace_configure_local(&bad_client, &bad_session, request)
                .expect_err("wrong kind")
                .code,
            Code::InvalidArgument
        );
        assert!(bad_client.close(&bad_session));

        let (local_client, local_session, local_path) =
            create_client("vector-workspace-configure-parity-local", "main").expect("local");
        assert!(local_client.close(&local_session));
        seed_inference_instance_state(&local_path, loom_types::InferenceModelKind::TextEmbedding);
        let local_client = loom_client::LocalLoomClient::new(&local_path);
        let local_session = local_client.open().expect("local reopen");
        let local_json = vector_workspace_configure_local(&local_client, &local_session, request)
            .expect("local parity");
        assert!(local_client.close(&local_session));

        let (remote_client, remote_session, remote_path) =
            create_remote_client("vector-workspace-configure-remote", "main").expect("remote");
        drop(remote_session);
        drop(remote_client);
        seed_inference_instance_state(&remote_path, loom_types::InferenceModelKind::TextEmbedding);
        let runtime = Arc::new(
            loom_hosted_core::remote::RemoteRuntime::start(remote_path.clone(), remote_config())
                .map_err(strerr)
                .expect("remote runtime"),
        );
        let service = Arc::new(RemoteHttpService::new(runtime, "/apps/loom/v1/call"));
        let transport = InProcessRemoteTransport { service };
        let connection = block(RemoteConnection::connect(
            transport,
            "https://host/apps/loom",
            &ContextResolver::default(),
            DiscoveryMode::Default,
        ))
        .expect("remote connection");
        let remote_client = RemoteLoomClient::new(connection);
        block(remote_client.open_session(SessionAuth::Unauthenticated)).expect("remote auth");
        let remote_session = block(<RemoteLoomClient<InProcessRemoteTransport> as Store>::open(
            &remote_client,
        ))
        .expect("remote session");
        let remote_json =
            vector_workspace_configure_remote(&remote_client, &remote_session, request)
                .expect("remote configure");
        assert_eq!(remote_json, local_json);
    }
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
