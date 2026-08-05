//! Locator-aware CLI store facade.
//!
//! Commands that use [`StoreClient`] accept a local or remote locator. A [`StoreClient::Local`] runs
//! against a local `Loom` engine; a [`StoreClient::Remote`] (with the `remote-client` feature) forwards
//! to a `loom serve remote` endpoint through the generated `RemoteLoomClient` over HTTP/2-over-TLS,
//! obtaining its session over the carrier session-open route.
//!
//! Commands that open through `cli_open_loom` instead accept only a local locator and reject a remote
//! target with a clear error. When the crate is built without `remote-client`, a remote locator here
//! also fails clearly rather than opening.
//!
//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;
use loom_locator::Target;
#[cfg(all(feature = "remote-client", any(test, feature = "mcp")))]
use loom_remote_protocol::api_types::Digest as WireDigest;
use loom_remote_protocol::api_types::LoomSession;
#[cfg(test)]
use loom_remote_protocol::generated::GeneratedOperationId;
use loom_remote_protocol::generated::{METHODS, MethodSig};

#[cfg(feature = "remote-client")]
use loom_locator::{ContextResolver, Discovery as LocatorDiscovery, RemoteTarget};
#[cfg(feature = "remote-client")]
use loom_remote_client::carrier::Http2TlsTransport;
#[cfg(all(feature = "remote-client", feature = "mcp"))]
use loom_remote_client::transport::{FrameSource, Transport};
#[cfg(feature = "remote-client")]
use loom_remote_client::{CallOptions, RemoteConnection, RemoteLoomClient};
#[cfg(feature = "serve")]
use loom_remote_protocol::codec::FromValue;
#[cfg(any(feature = "serve", feature = "remote-client"))]
use loom_remote_protocol::codec::ToValue;
#[cfg(feature = "remote-client")]
use loom_remote_protocol::discovery::DiscoveryMode;
#[cfg(all(feature = "remote-client", feature = "mcp"))]
use loom_remote_protocol::generated_api::{
    Calendar, Cas, Columnar, Contacts, Dataframe, Document, FileSystem, Graph, Kv, Lanes, Ledger,
    Logs, Mail, Metrics, Pages, Queue, QueueConsumers, Search, Sql, StoreAdmin, Tickets,
    TimeSeries, Traces, Vector, VersionControl, Watch, Workspaces,
};
#[cfg(feature = "remote-client")]
use loom_remote_protocol::generated_api::{Store, Transfer};
#[cfg(any(feature = "serve", feature = "remote-client"))]
use loom_remote_protocol::session::SessionAuth;
#[cfg(feature = "remote-client")]
use std::sync::Arc;

fn remote_lane_ticket_placement(
    placement: Option<&str>,
) -> loom_core::Result<Option<loom_remote_protocol::api_types::LaneTicketPlacement>> {
    placement
        .map(|placement| match placement {
            "FIRST" => Ok(loom_remote_protocol::api_types::LaneTicketPlacement::First),
            "LAST" => Ok(loom_remote_protocol::api_types::LaneTicketPlacement::Last),
            "BEFORE" => Ok(loom_remote_protocol::api_types::LaneTicketPlacement::Before),
            "AFTER" => Ok(loom_remote_protocol::api_types::LaneTicketPlacement::After),
            other => Err(loom_types::LoomError::invalid(format!(
                "lane ticket placement must be FIRST, LAST, BEFORE, or AFTER: {other}"
            ))),
        })
        .transpose()
}

#[cfg(feature = "remote-client")]
fn remote_lane_ticket_placement_parts(
    placement: loom_lanes::LaneTicketPlacement<'_>,
) -> (
    Option<loom_remote_protocol::api_types::LaneTicketPlacement>,
    Option<String>,
) {
    match placement {
        loom_lanes::LaneTicketPlacement::First => (
            Some(loom_remote_protocol::api_types::LaneTicketPlacement::First),
            None,
        ),
        loom_lanes::LaneTicketPlacement::Last => (
            Some(loom_remote_protocol::api_types::LaneTicketPlacement::Last),
            None,
        ),
        loom_lanes::LaneTicketPlacement::Before(anchor) => (
            Some(loom_remote_protocol::api_types::LaneTicketPlacement::Before),
            Some(anchor.to_string()),
        ),
        loom_lanes::LaneTicketPlacement::After(anchor) => (
            Some(loom_remote_protocol::api_types::LaneTicketPlacement::After),
            Some(anchor.to_string()),
        ),
    }
}

/// A locator-aware store client: a local engine handle or a connected remote endpoint.
pub(crate) enum StoreClient {
    /// A local store, opened per operation through the existing helpers (read vs write as needed).
    Local {
        /// The resolved local locator string passed to `cli_open_loom*`.
        locator: String,
    },
    /// A connected remote endpoint with a wire-opened session and a bound store handle.
    #[cfg(feature = "remote-client")]
    Remote(Box<RemoteStore>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CliExecutionTarget {
    DirectLocal,
    DaemonLocal,
    Remote,
}

pub(crate) enum CliExecutionContext {
    DirectLocal {
        locator: String,
    },
    #[cfg(feature = "serve")]
    DaemonLocal(Box<DaemonLocalStore>),
    #[cfg(feature = "remote-client")]
    Remote(Box<RemoteStore>),
}

pub(crate) enum CliGeneratedClient {
    DirectLocal {
        client: Box<loom_client::LocalLoomClient>,
        handle: LoomSession,
    },
    #[cfg(feature = "serve")]
    DaemonLocal(Box<DaemonLocalStore>),
    #[cfg(feature = "remote-client")]
    Remote(Box<RemoteStore>),
}

pub(crate) struct LaneCloseoutArgs<'a> {
    pub(crate) workspace: &'a str,
    pub(crate) lane_id: &'a str,
    pub(crate) ticket_workspace_id: &'a str,
    pub(crate) ticket_id: &'a str,
    pub(crate) comment_type: &'a str,
    pub(crate) comment_body: &'a str,
    pub(crate) evidence_json: Option<&'a str>,
    pub(crate) status_report: &'a str,
    pub(crate) updated_by: &'a str,
    pub(crate) expected_root: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub(crate) struct CliGeneratedOperation {
    method: MethodSig,
    args_without_handle: Vec<loom_codec::Value>,
}

impl CliGeneratedOperation {
    pub(crate) fn new(
        interface: &str,
        method: &str,
        args_without_handle: Vec<loom_codec::Value>,
    ) -> Result<Self, String> {
        let method_sig = generated_method(interface, method)
            .ok_or_else(|| format!("unknown generated operation {interface}.{method}"))?;
        if method_sig.ret.starts_with("stream<") {
            return Err(format!(
                "generated operation {interface}.{method} is streaming; use the streaming boundary"
            ));
        }
        let expected = method_sig
            .args
            .iter()
            .filter(|(_, name)| *name != "handle")
            .count();
        if args_without_handle.len() != expected {
            return Err(format!(
                "generated operation {interface}.{method} expects {expected} non-handle arguments, got {}",
                args_without_handle.len()
            ));
        }
        Ok(Self {
            method: method_sig,
            args_without_handle,
        })
    }

    pub(crate) fn interface(&self) -> &'static str {
        self.method.interface
    }

    pub(crate) fn method(&self) -> &'static str {
        self.method.method
    }

    fn wire_args(&self, handle: &LoomSession) -> Vec<loom_codec::Value> {
        let mut args = Vec::with_capacity(self.method.args.len());
        let mut next_arg = 0usize;
        for (_, name) in self.method.args {
            if *name == "handle" {
                args.push(handle.to_value());
            } else {
                args.push(self.args_without_handle[next_arg].clone());
                next_arg += 1;
            }
        }
        args
    }
}

fn generated_method(interface: &str, method: &str) -> Option<MethodSig> {
    METHODS
        .iter()
        .copied()
        .find(|sig| sig.interface == interface && sig.method == method)
}

impl CliGeneratedClient {
    pub(crate) fn target(&self) -> CliExecutionTarget {
        match self {
            Self::DirectLocal { client, handle } => {
                let _ = (client, handle);
                CliExecutionTarget::DirectLocal
            }
            #[cfg(feature = "serve")]
            Self::DaemonLocal(store) => {
                let _ = store;
                CliExecutionTarget::DaemonLocal
            }
            #[cfg(feature = "remote-client")]
            Self::Remote(remote) => {
                let _ = remote;
                CliExecutionTarget::Remote
            }
        }
    }

    pub(crate) fn execute_unary(
        &self,
        operation: &CliGeneratedOperation,
    ) -> Result<loom_codec::Value, String> {
        match self {
            Self::DirectLocal { client, handle } => {
                let args = operation.wire_args(handle);
                match loom_hosted_core::generated_dispatch::dispatch_local(
                    client.as_ref(),
                    handle,
                    operation.interface(),
                    operation.method(),
                    &args,
                )
                .map_err(|e| e.to_string())?
                {
                    loom_hosted_core::generated_dispatch::Dispatched::Unary(value) => Ok(value),
                    loom_hosted_core::generated_dispatch::Dispatched::Stream(_) => Err(format!(
                        "generated operation {}.{} returned a stream on the unary boundary",
                        operation.interface(),
                        operation.method()
                    )),
                }
            }
            #[cfg(feature = "serve")]
            Self::DaemonLocal(store) => {
                let args = operation.wire_args(&store.handle);
                store.generated_unary(operation.interface(), operation.method(), args)
            }
            #[cfg(feature = "remote-client")]
            Self::Remote(remote) => {
                let args = operation.wire_args(&remote.handle);
                remote.block(remote.client.call(
                    operation.interface(),
                    operation.method(),
                    args,
                    &CallOptions::default(),
                ))
            }
        }
    }

    pub(crate) fn doc_put_text(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
        text: &str,
        expected_entity_tag: Option<&str>,
    ) -> Result<loom_core::document::DocumentPutResult, String> {
        self.doc_put_binary_value(
            "put_text",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
                id.to_string().to_value(),
                text.to_string().to_value(),
                expected_entity_tag.map(str::to_string).to_value(),
            ],
        )
    }

    pub(crate) fn doc_put_binary(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
        content: Vec<u8>,
        expected_entity_tag: Option<&str>,
    ) -> Result<loom_core::document::DocumentPutResult, String> {
        self.doc_put_binary_value(
            "put_binary",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
                id.to_string().to_value(),
                loom_codec::Value::Bytes(content),
                expected_entity_tag.map(str::to_string).to_value(),
            ],
        )
    }

    fn doc_put_binary_value(
        &self,
        method: &str,
        args: Vec<loom_codec::Value>,
    ) -> Result<loom_core::document::DocumentPutResult, String> {
        let value = self.execute_unary(&CliGeneratedOperation::new("Document", method, args)?)?;
        let loom_codec::Value::Bytes(bytes) = value else {
            return Err(format!(
                "Document.{method} returned unexpected value {value:?}"
            ));
        };
        let (digest, entity_tag) =
            loom_wire::document::put_result_from_cbor(&bytes).map_err(|e| e.to_string())?;
        let digest = Digest::parse(&digest).map_err(|e| e.to_string())?;
        Ok(loom_core::document::DocumentPutResult { entity_tag, digest })
    }

    pub(crate) fn doc_get_text(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<loom_core::document::DocumentText>, String> {
        let value = self.execute_unary(&CliGeneratedOperation::new(
            "Document",
            "get_text",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
                id.to_string().to_value(),
            ],
        )?)?;
        let bytes = match value {
            loom_codec::Value::Null => return Ok(None),
            loom_codec::Value::Bytes(bytes) => bytes,
            other => {
                return Err(format!(
                    "Document.get_text returned unexpected value {other:?}"
                ));
            }
        };
        let (text, digest, entity_tag) =
            loom_wire::document::text_result_from_cbor(&bytes).map_err(|err| err.to_string())?;
        let digest = Digest::parse(&digest).map_err(|err| err.to_string())?;
        Ok(Some(loom_core::document::DocumentText {
            text,
            digest,
            entity_tag,
        }))
    }

    pub(crate) fn doc_get_binary(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<loom_core::document::DocumentBinary>, String> {
        let value = self.execute_unary(&CliGeneratedOperation::new(
            "Document",
            "get_binary",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
                id.to_string().to_value(),
            ],
        )?)?;
        let bytes = match value {
            loom_codec::Value::Null => return Ok(None),
            loom_codec::Value::Bytes(bytes) => bytes,
            other => {
                return Err(format!(
                    "Document.get_binary returned unexpected value {other:?}"
                ));
            }
        };
        let (bytes, digest, entity_tag) =
            loom_wire::document::binary_result_from_cbor(&bytes).map_err(|e| e.to_string())?;
        Ok(Some(loom_core::document::DocumentBinary {
            bytes,
            digest: Digest::parse(&digest).map_err(|e| e.to_string())?,
            entity_tag,
        }))
    }

    pub(crate) fn doc_delete(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> Result<bool, String> {
        self.document_bool(
            "delete",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
                id.to_string().to_value(),
            ],
        )
    }

    pub(crate) fn doc_delete_collection(
        &self,
        workspace: &str,
        collection: &str,
    ) -> Result<bool, String> {
        self.document_bool(
            "delete_collection",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
            ],
        )
    }

    pub(crate) fn doc_list_binary(
        &self,
        workspace: &str,
        collection: &str,
    ) -> Result<Vec<u8>, String> {
        self.document_bytes(
            "list_binary",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
            ],
        )
    }

    pub(crate) fn doc_index_create(
        &self,
        workspace: &str,
        collection: &str,
        name: &str,
        path: &str,
        unique: bool,
    ) -> Result<(), String> {
        self.document_void(
            "index_create",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
                name.to_string().to_value(),
                path.to_string().to_value(),
                unique.to_value(),
            ],
        )
    }

    pub(crate) fn doc_index_create_json(
        &self,
        workspace: &str,
        collection: &str,
        declaration_json: Vec<u8>,
    ) -> Result<(), String> {
        self.document_void(
            "index_create_json",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
                loom_codec::Value::Bytes(declaration_json),
            ],
        )
    }

    pub(crate) fn doc_index_drop(
        &self,
        workspace: &str,
        collection: &str,
        name: &str,
    ) -> Result<bool, String> {
        self.document_bool(
            "index_drop",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
                name.to_string().to_value(),
            ],
        )
    }

    pub(crate) fn doc_index_rebuild(
        &self,
        workspace: &str,
        collection: &str,
        name: &str,
    ) -> Result<(), String> {
        self.document_void(
            "index_rebuild",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
                name.to_string().to_value(),
            ],
        )
    }

    pub(crate) fn doc_index_list(
        &self,
        workspace: &str,
        collection: &str,
    ) -> Result<serde_json::Value, String> {
        let bytes = self.document_bytes(
            "index_list_json",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
            ],
        )?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    pub(crate) fn doc_index_statuses(
        &self,
        workspace: &str,
        collection: &str,
    ) -> Result<serde_json::Value, String> {
        let bytes = self.document_bytes(
            "index_status_json",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
            ],
        )?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    pub(crate) fn doc_find(
        &self,
        workspace: &str,
        collection: &str,
        index: &str,
        value_json: &str,
    ) -> Result<Vec<String>, String> {
        let bytes = self.document_bytes(
            "find_json",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
                index.to_string().to_value(),
                loom_codec::Value::Bytes(value_json.as_bytes().to_vec()),
            ],
        )?;
        serde_json::from_slice::<Vec<String>>(&bytes).map_err(|e| e.to_string())
    }

    pub(crate) fn doc_query(
        &self,
        workspace: &str,
        collection: &str,
        query_json: Vec<u8>,
    ) -> Result<serde_json::Value, String> {
        let bytes = self.document_bytes(
            "query_json",
            vec![
                workspace.to_string().to_value(),
                collection.to_string().to_value(),
                loom_codec::Value::Bytes(query_json),
            ],
        )?;
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }

    fn document_bool(&self, method: &str, args: Vec<loom_codec::Value>) -> Result<bool, String> {
        match self.execute_unary(&CliGeneratedOperation::new("Document", method, args)?)? {
            loom_codec::Value::Bool(value) => Ok(value),
            other => Err(format!(
                "Document.{method} returned unexpected value {other:?}"
            )),
        }
    }

    fn document_bytes(
        &self,
        method: &str,
        args: Vec<loom_codec::Value>,
    ) -> Result<Vec<u8>, String> {
        match self.execute_unary(&CliGeneratedOperation::new("Document", method, args)?)? {
            loom_codec::Value::Bytes(bytes) => Ok(bytes),
            other => Err(format!(
                "Document.{method} returned unexpected value {other:?}"
            )),
        }
    }

    fn document_void(&self, method: &str, args: Vec<loom_codec::Value>) -> Result<(), String> {
        match self.execute_unary(&CliGeneratedOperation::new("Document", method, args)?)? {
            loom_codec::Value::Null => Ok(()),
            other => Err(format!(
                "Document.{method} returned unexpected value {other:?}"
            )),
        }
    }

    pub(crate) fn lanes_ticket_add(
        &self,
        workspace: &str,
        lane_id: &str,
        ticket_id: &str,
        placement: Option<&str>,
        anchor: Option<&str>,
        updated_by: &str,
    ) -> Result<loom_lanes::Lane, String> {
        let placement = remote_lane_ticket_placement(placement).map_err(|err| err.to_string())?;
        let value = self.execute_unary(&CliGeneratedOperation::new(
            "Lanes",
            "ticket_add",
            vec![
                workspace.to_string().to_value(),
                lane_id.to_string().to_value(),
                ticket_id.to_string().to_value(),
                placement.to_value(),
                anchor.map(str::to_string).to_value(),
                updated_by.to_string().to_value(),
            ],
        )?)?;
        let loom_codec::Value::Bytes(bytes) = value else {
            return Err(format!(
                "Lanes.ticket_add returned unexpected value {value:?}"
            ));
        };
        loom_lanes::Lane::decode(&bytes).map_err(|e| e.to_string())
    }

    pub(crate) fn lanes_create(
        &self,
        workspace: &str,
        lane: loom_lanes::Lane,
    ) -> Result<loom_lanes::Lane, String> {
        let value = self.execute_unary(&CliGeneratedOperation::new(
            "Lanes",
            "create",
            vec![
                workspace.to_string().to_value(),
                loom_codec::Value::Bytes(lane.encode().map_err(|e| e.to_string())?),
            ],
        )?)?;
        Self::lane_from_generated_value("create", value)
    }

    pub(crate) fn lanes_get(
        &self,
        workspace: &str,
        lane_id: &str,
    ) -> Result<Option<loom_lanes::Lane>, String> {
        let value = self.execute_unary(&CliGeneratedOperation::new(
            "Lanes",
            "get",
            vec![
                workspace.to_string().to_value(),
                lane_id.to_string().to_value(),
            ],
        )?)?;
        match value {
            loom_codec::Value::Null => Ok(None),
            other => Self::lane_from_generated_value("get", other).map(Some),
        }
    }

    pub(crate) fn lanes_update(
        &self,
        workspace: &str,
        lane_id: &str,
        title: Option<&str>,
        description: Option<&str>,
        lane_status: Option<&str>,
        status_report: Option<&str>,
        reviewer_feedback: Option<&str>,
        updated_by: &str,
    ) -> Result<loom_lanes::Lane, String> {
        let value = self.execute_unary(&CliGeneratedOperation::new(
            "Lanes",
            "update",
            vec![
                workspace.to_string().to_value(),
                lane_id.to_string().to_value(),
                title.map(str::to_string).to_value(),
                description.map(str::to_string).to_value(),
                lane_status.map(str::to_string).to_value(),
                status_report.map(str::to_string).to_value(),
                reviewer_feedback.map(str::to_string).to_value(),
                updated_by.to_string().to_value(),
            ],
        )?)?;
        Self::lane_from_generated_value("update", value)
    }

    pub(crate) fn lanes_ticket_remove(
        &self,
        workspace: &str,
        lane_id: &str,
        ticket_id: &str,
        updated_by: &str,
    ) -> Result<loom_lanes::Lane, String> {
        let value = self.execute_unary(&CliGeneratedOperation::new(
            "Lanes",
            "ticket_remove",
            vec![
                workspace.to_string().to_value(),
                lane_id.to_string().to_value(),
                ticket_id.to_string().to_value(),
                updated_by.to_string().to_value(),
            ],
        )?)?;
        Self::lane_from_generated_value("ticket_remove", value)
    }

    pub(crate) fn lanes_ticket_transfer(
        &self,
        workspace: &str,
        source_lane_id: &str,
        target_lane_id: &str,
        ticket_id: &str,
        updated_by: &str,
    ) -> Result<loom_lanes::Lane, String> {
        let value = self.execute_unary(&CliGeneratedOperation::new(
            "Lanes",
            "ticket_transfer",
            vec![
                workspace.to_string().to_value(),
                source_lane_id.to_string().to_value(),
                target_lane_id.to_string().to_value(),
                ticket_id.to_string().to_value(),
                updated_by.to_string().to_value(),
            ],
        )?)?;
        Self::lane_from_generated_value("ticket_transfer", value)
    }

    pub(crate) fn lanes_closeout(
        &self,
        args: LaneCloseoutArgs<'_>,
    ) -> Result<loom_lanes::Lane, String> {
        let value = self.execute_unary(&CliGeneratedOperation::new(
            "Lanes",
            "closeout",
            vec![
                args.workspace.to_string().to_value(),
                args.lane_id.to_string().to_value(),
                args.ticket_workspace_id.to_string().to_value(),
                args.ticket_id.to_string().to_value(),
                args.comment_type.to_string().to_value(),
                args.comment_body.to_string().to_value(),
                args.evidence_json.map(str::to_string).to_value(),
                args.status_report.to_string().to_value(),
                args.updated_by.to_string().to_value(),
                args.expected_root.map(str::to_string).to_value(),
            ],
        )?)?;
        Self::lane_from_generated_value("closeout", value)
    }

    pub(crate) fn lanes_delete(
        &self,
        workspace: &str,
        lane_id: &str,
        updated_by: &str,
    ) -> Result<loom_lanes::Lane, String> {
        let value = self.execute_unary(&CliGeneratedOperation::new(
            "Lanes",
            "delete",
            vec![
                workspace.to_string().to_value(),
                lane_id.to_string().to_value(),
                updated_by.to_string().to_value(),
            ],
        )?)?;
        Self::lane_from_generated_value("delete", value)
    }

    pub(crate) fn lanes_get_view_json(
        &self,
        workspace: &str,
        ticket_workspace_id: &str,
        lane_id: &str,
        detailed: bool,
    ) -> Result<String, String> {
        self.generated_json(
            "Lanes",
            "get_view_json",
            vec![
                workspace.to_string().to_value(),
                ticket_workspace_id.to_string().to_value(),
                lane_id.to_string().to_value(),
                detailed.to_value(),
            ],
        )
    }

    pub(crate) fn lanes_list_views_json(
        &self,
        workspace: &str,
        ticket_workspace_id: &str,
        detailed: bool,
    ) -> Result<String, String> {
        self.generated_json(
            "Lanes",
            "list_views_json",
            vec![
                workspace.to_string().to_value(),
                ticket_workspace_id.to_string().to_value(),
                detailed.to_value(),
            ],
        )
    }

    pub(crate) fn workspace_create(
        &self,
        name: Option<&str>,
        facet: Option<FacetKind>,
    ) -> Result<loom_core::WorkspaceId, String> {
        let value = self.execute_unary(&CliGeneratedOperation::new(
            "Workspaces",
            "workspace_create",
            vec![
                name.map(str::to_string).to_value(),
                facet
                    .map(|facet| loom_codec::Value::Bytes(vec![facet.stable_tag()]))
                    .unwrap_or(loom_codec::Value::Null),
            ],
        )?)?;
        let loom_codec::Value::Bytes(bytes) = value else {
            return Err(format!(
                "Workspaces.workspace_create returned unexpected value {value:?}"
            ));
        };
        let bytes = <[u8; 16]>::try_from(bytes.as_slice())
            .map_err(|_| "Workspaces.workspace_create returned invalid UUID bytes".to_string())?;
        Ok(loom_core::WorkspaceId::from_bytes(bytes))
    }

    pub(crate) fn ensure_workspace_id(
        &self,
        workspace: &str,
        facet: FacetKind,
    ) -> Result<loom_core::WorkspaceId, String> {
        match self.resolve_workspace_id(workspace) {
            Ok(id) => Ok(id),
            Err(error) if error == format!("workspace {workspace:?} not found") => {
                self.workspace_create(Some(workspace), Some(facet))
            }
            Err(error) => Err(error),
        }
    }

    fn lane_from_generated_value(
        method: &str,
        value: loom_codec::Value,
    ) -> Result<loom_lanes::Lane, String> {
        let loom_codec::Value::Bytes(bytes) = value else {
            return Err(format!(
                "Lanes.{method} returned unexpected value {value:?}"
            ));
        };
        loom_lanes::Lane::decode(&bytes).map_err(|e| e.to_string())
    }

    pub(crate) fn generated_json(
        &self,
        interface: &str,
        method: &str,
        args: Vec<loom_codec::Value>,
    ) -> Result<String, String> {
        match self.execute_unary(&CliGeneratedOperation::new(interface, method, args)?)? {
            loom_codec::Value::Text(text) => Ok(text),
            other => Err(format!(
                "{interface}.{method} returned unexpected value {other:?}"
            )),
        }
    }

    pub(crate) fn resolve_workspace_id(
        &self,
        workspace: &str,
    ) -> Result<loom_core::WorkspaceId, String> {
        let value = self.execute_unary(&CliGeneratedOperation::new(
            "Workspaces",
            "workspace_list",
            Vec::new(),
        )?)?;
        let loom_codec::Value::Array(records) = value else {
            return Err(format!(
                "Workspaces.workspace_list returned unexpected value {value:?}"
            ));
        };
        let infos = cli_workspace_infos_from_generated_records(&records)?;
        cli_select_workspace_id(&infos, workspace)
            .ok_or_else(|| format!("workspace {workspace:?} not found"))
    }

    pub(crate) fn workspace_list(&self) -> Result<Vec<loom_core::WorkspaceInfo>, String> {
        let value = self.execute_unary(&CliGeneratedOperation::new(
            "Workspaces",
            "workspace_list",
            Vec::new(),
        )?)?;
        let loom_codec::Value::Array(records) = value else {
            return Err(format!(
                "Workspaces.workspace_list returned unexpected value {value:?}"
            ));
        };
        cli_workspace_infos_from_generated_records(&records)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CliStoreAdministrationBoundary {
    GeneratedStoreAdmin,
    #[cfg(test)]
    OfflineStoreOwner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CliExecutionBoundary {
    GeneratedClient,
    StoreAdministration(CliStoreAdministrationBoundary),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
pub enum CliOperation {
    BundleExport,
    BundleImport,
    Clone,
    Copy,
    Get,
    Hash,
    Init,
    KeyChange,
    KeyCreate,
    KeyStatus,
    KeyVerify,
    Policy,
    Put,
    Rekey,
    Replace,
    Stat,
}

#[cfg(test)]
fn cli_store_administration_boundary_reason(
    operation: CliOperation,
) -> (CliStoreAdministrationBoundary, &'static str) {
    match operation {
        CliOperation::BundleExport
        | CliOperation::BundleImport
        | CliOperation::Clone
        | CliOperation::Copy
        | CliOperation::Get
        | CliOperation::Hash
        | CliOperation::Init
        | CliOperation::KeyChange
        | CliOperation::KeyCreate
        | CliOperation::Put
        | CliOperation::Replace => (
            CliStoreAdministrationBoundary::OfflineStoreOwner,
            "operation owns file creation, copy, replacement, or byte-level access outside a generated store session",
        ),
        CliOperation::KeyStatus
        | CliOperation::KeyVerify
        | CliOperation::Policy
        | CliOperation::Rekey
        | CliOperation::Stat => (
            CliStoreAdministrationBoundary::GeneratedStoreAdmin,
            "operation is represented by the generated StoreAdmin interface",
        ),
    }
}

#[cfg(test)]
pub(crate) fn classify_cli_operation(operation: CliOperation) -> CliExecutionBoundary {
    let (boundary, _reason) = cli_store_administration_boundary_reason(operation);
    CliExecutionBoundary::StoreAdministration(boundary)
}

pub(crate) fn classify_generated_operation(
    interface: &str,
    method: &str,
) -> Result<CliExecutionBoundary, String> {
    let method_sig = generated_method(interface, method)
        .ok_or_else(|| format!("unknown generated operation {interface}.{method}"))?;
    if method_sig.ret.starts_with("stream<") {
        return Err(format!(
            "generated operation {interface}.{method} is streaming; use the streaming boundary"
        ));
    }
    Ok(CliExecutionBoundary::GeneratedClient)
}

impl CliExecutionContext {
    pub(crate) fn target(&self) -> CliExecutionTarget {
        match self {
            Self::DirectLocal { locator } => {
                let _ = locator;
                CliExecutionTarget::DirectLocal
            }
            #[cfg(feature = "serve")]
            Self::DaemonLocal(store) => {
                let _ = (&store.locator, &store.session_id, &store.handle);
                CliExecutionTarget::DaemonLocal
            }
            #[cfg(feature = "remote-client")]
            Self::Remote(remote) => {
                let _ = &remote.handle;
                CliExecutionTarget::Remote
            }
        }
    }

    pub(crate) fn into_generated_client(self) -> Result<CliGeneratedClient, String> {
        self.into_generated_client_with_keys(&KeyOpts::default())
    }

    pub(crate) fn into_generated_client_with_keys(
        self,
        keys: &KeyOpts,
    ) -> Result<CliGeneratedClient, String> {
        self.into_generated_client_with_mode(keys, OpenGeneratedMode::ReadWrite)
    }

    pub(crate) fn into_read_only_generated_client_with_keys(
        self,
        keys: &KeyOpts,
    ) -> Result<CliGeneratedClient, String> {
        self.into_generated_client_with_mode(keys, OpenGeneratedMode::ReadOnly)
    }

    fn into_generated_client_with_mode(
        self,
        keys: &KeyOpts,
        mode: OpenGeneratedMode,
    ) -> Result<CliGeneratedClient, String> {
        match self {
            Self::DirectLocal { locator } => {
                let client = Box::new(loom_client::LocalLoomClient::new(&locator));
                let encrypted = {
                    let fs = match mode {
                        OpenGeneratedMode::ReadOnly => {
                            FileStore::open_read(&locator).map_err(|e| e.to_string())?
                        }
                        OpenGeneratedMode::ReadWrite => {
                            FileStore::open(&locator).map_err(|e| e.to_string())?
                        }
                    };
                    fs.is_encrypted()
                };
                let handle = if encrypted {
                    match acquire_key_spec(&keys.source, "Passphrase", false)? {
                        KeySpec::Passphrase(passphrase) => match mode {
                            OpenGeneratedMode::ReadOnly => client
                                .open_read_keyed(passphrase.as_bytes())
                                .map_err(|e| e.to_string())?,
                            OpenGeneratedMode::ReadWrite => client
                                .open_keyed(passphrase.as_bytes())
                                .map_err(|e| e.to_string())?,
                        },
                        KeySpec::RawKek(kek) => match mode {
                            OpenGeneratedMode::ReadOnly => {
                                client.open_read_with_kek(*kek).map_err(|e| e.to_string())?
                            }
                            OpenGeneratedMode::ReadWrite => {
                                client.open_with_kek(*kek).map_err(|e| e.to_string())?
                            }
                        },
                    }
                } else {
                    match mode {
                        OpenGeneratedMode::ReadOnly => {
                            client.open_read().map_err(|e| e.to_string())?
                        }
                        OpenGeneratedMode::ReadWrite => client.open().map_err(|e| e.to_string())?,
                    }
                };
                if let Some((principal, passphrase)) = acquire_auth_session(keys)? {
                    client
                        .authenticate_passphrase(&handle, principal, passphrase.as_bytes())
                        .map_err(|e| e.to_string())?;
                }
                Ok(CliGeneratedClient::DirectLocal { client, handle })
            }
            #[cfg(feature = "serve")]
            Self::DaemonLocal(store) => Ok(CliGeneratedClient::DaemonLocal(store)),
            #[cfg(feature = "remote-client")]
            Self::Remote(remote) => Ok(CliGeneratedClient::Remote(remote)),
        }
    }
}

#[derive(Clone, Copy)]
enum OpenGeneratedMode {
    ReadOnly,
    ReadWrite,
}

pub(crate) fn open_cli_execution_context(store: &str) -> Result<CliExecutionContext, String> {
    open_cli_execution_context_with_keys(store, &KeyOpts::default())
}

pub(crate) fn open_cli_generated_client(
    store: &str,
    keys: &KeyOpts,
) -> Result<CliGeneratedClient, String> {
    open_cli_execution_context_with_keys(store, keys)?.into_generated_client_with_keys(keys)
}

pub(crate) fn open_cli_generated_client_for_dry_run(
    store: &str,
    keys: &KeyOpts,
    dry_run: bool,
) -> Result<CliGeneratedClient, String> {
    if dry_run {
        open_cli_read_only_generated_client(store, keys)
    } else {
        open_cli_generated_client(store, keys)
    }
}

pub(crate) fn open_cli_read_only_generated_client(
    store: &str,
    keys: &KeyOpts,
) -> Result<CliGeneratedClient, String> {
    open_cli_read_only_execution_context_with_keys(store, keys)?
        .into_read_only_generated_client_with_keys(keys)
}

pub(crate) fn open_cli_execution_context_with_keys(
    store: &str,
    keys: &KeyOpts,
) -> Result<CliExecutionContext, String> {
    match crate::locator_cx::current().resolve_target(store)? {
        Target::Local(path) => open_local_execution_context(path.to_string_lossy().as_ref(), keys),
        #[cfg(feature = "remote-client")]
        Target::Remote(target) => {
            let store = match remote_session_auth(&target, keys)? {
                SessionAuth::Unauthenticated => RemoteStore::connect(&target)?,
                auth => RemoteStore::connect_with_auth(&target, auth)?,
            };
            Ok(CliExecutionContext::Remote(Box::new(store)))
        }
        #[cfg(not(feature = "remote-client"))]
        Target::Remote(target) => Err(format!(
            "locator resolves to remote endpoint {}; rebuild with the `remote-client` feature to forward remote commands",
            target.url
        )),
    }
}

fn open_cli_read_only_execution_context_with_keys(
    store: &str,
    keys: &KeyOpts,
) -> Result<CliExecutionContext, String> {
    match crate::locator_cx::current().resolve_target(store)? {
        Target::Local(path) => open_local_execution_context(path.to_string_lossy().as_ref(), keys),
        #[cfg(feature = "remote-client")]
        Target::Remote(target) => {
            let store = match remote_session_auth(&target, keys)? {
                SessionAuth::Unauthenticated => RemoteStore::connect(&target)?,
                auth => RemoteStore::connect_with_auth(&target, auth)?,
            };
            Ok(CliExecutionContext::Remote(Box::new(store)))
        }
        #[cfg(not(feature = "remote-client"))]
        Target::Remote(target) => {
            let _ = keys;
            Err(format!(
                "locator resolves to remote endpoint {}; rebuild with the `remote-client` feature to forward remote commands",
                target.url
            ))
        }
    }
}

fn open_local_execution_context(
    store: &str,
    keys: &KeyOpts,
) -> Result<CliExecutionContext, String> {
    #[cfg(feature = "serve")]
    {
        if let Ok(paths) = daemon::paths(store) {
            match daemon::runtime_compatibility(&paths) {
                daemon::DaemonRuntimeCompatibility::Current(_) => {
                    let auth = local_session_auth(keys)?;
                    return DaemonLocalStore::connect(store, auth)
                        .map(Box::new)
                        .map(CliExecutionContext::DaemonLocal);
                }
                daemon::DaemonRuntimeCompatibility::Prior(status) => {
                    return Err(format!(
                        "daemon owns store {:?} but uses an incompatible runtime: {}; stop or upgrade the daemon before running generated CLI commands",
                        paths.store, status.reason
                    ));
                }
                daemon::DaemonRuntimeCompatibility::Unresponsive(runtime) => {
                    return Err(format!(
                        "daemon owns store {:?} but generated CLI negotiation failed: {}; stop or restart daemon pid {} before running generated CLI commands",
                        paths.store, runtime.reason, runtime.metadata.pid
                    ));
                }
                daemon::DaemonRuntimeCompatibility::Starting(_) => {
                    return Err(format!(
                        "daemon owns store {:?} but is still starting; retry after daemon status is running",
                        paths.store
                    ));
                }
                daemon::DaemonRuntimeCompatibility::Stopped => {}
            }
        }
    }
    Ok(CliExecutionContext::DirectLocal {
        locator: store.to_string(),
    })
}

/// Open a store client: resolve the locator, connecting remotely for a remote target (or failing
/// clearly when remote support is not built in).
///
/// # Errors
/// Returns a message when the locator cannot be resolved or a remote connection cannot be established.
pub(crate) fn open_store_client(store: &str) -> Result<StoreClient, String> {
    let context = open_cli_execution_context(store)?;
    let _selected_target = context.target();
    match context {
        CliExecutionContext::DirectLocal { locator } => Ok(StoreClient::Local { locator }),
        #[cfg(feature = "remote-client")]
        CliExecutionContext::Remote(remote) => Ok(StoreClient::Remote(remote)),
        #[cfg(feature = "serve")]
        CliExecutionContext::DaemonLocal(daemon_store) => {
            let generated =
                CliExecutionContext::DaemonLocal(daemon_store).into_generated_client()?;
            let version_operation = CliGeneratedOperation::new("Store", "version", Vec::new())?;
            let _version = generated.execute_unary(&version_operation)?;
            let generated_boundary = classify_generated_operation("Store", "version")?;
            let command_boundary = CliExecutionBoundary::StoreAdministration(
                CliStoreAdministrationBoundary::GeneratedStoreAdmin,
            );
            Err(format!(
                "daemon-local generated execution selected for {store:?} as {:?}, but this command family still uses the legacy StoreClient path for {:?} after {:?} was validated; migrate it to CliExecutionContext instead of reopening the store directly",
                generated.target(),
                command_boundary,
                generated_boundary
            ))
        }
    }
}

#[cfg(feature = "serve")]
pub(crate) struct DaemonLocalStore {
    locator: String,
    paths: daemon::DaemonPaths,
    session_id: Vec<u8>,
    handle: LoomSession,
    close_session_on_drop: bool,
}

#[cfg(feature = "serve")]
impl DaemonLocalStore {
    pub(crate) fn connect(store: &str, auth: SessionAuth) -> Result<Self, String> {
        let paths = daemon::paths(store).map_err(|e| e.to_string())?;
        daemon::status_response(&paths).map_err(|e| {
            format!("daemon-local generated dispatch selected but daemon status failed: {e}")
        })?;
        let open_bytes = loom_remote_protocol::session::open_request_bytes(&auth);
        let reply_bytes = daemon::generated_session_open(&paths, &open_bytes)
            .map_err(|e| format!("daemon-local generated session open failed: {e}"))?;
        let reply = loom_remote_protocol::session::parse_open_reply(&reply_bytes)
            .map_err(|e| format!("decode daemon-local generated session open response: {e}"))?;
        let session_id = match reply {
            loom_remote_protocol::session::SessionOpenReply::Ok { session_id, .. } => session_id,
            loom_remote_protocol::session::SessionOpenReply::Err(error) => {
                return Err(format!(
                    "daemon-local generated session open failed: {:?}: {}",
                    error.code, error.message
                ));
            }
        };
        let handle = daemon_generated_open_store(&paths, &session_id)?;
        Ok(Self {
            locator: store.to_string(),
            paths,
            session_id,
            handle,
            close_session_on_drop: true,
        })
    }

    pub(crate) fn resume_logical_session(
        store: &str,
        auth: SessionAuth,
        credential: &[u8],
    ) -> Result<(Self, Vec<u8>), String> {
        let paths = daemon::paths(store).map_err(|e| e.to_string())?;
        daemon::status_response(&paths).map_err(|error| {
            format!("daemon-local logical session requires a running coordinator: {error}")
        })?;
        let request = loom_remote_protocol::session::resume_request_bytes(&auth, credential);
        let reply = daemon::generated_session_open(&paths, &request)
            .map_err(|error| format!("daemon-local logical session resume failed: {error}"))?;
        let reply = loom_remote_protocol::session::parse_open_reply(&reply)
            .map_err(|error| format!("decode logical session resume response: {error}"))?;
        let (session_id, credential) = match reply {
            loom_remote_protocol::session::SessionOpenReply::Ok {
                session_id,
                credential: Some(credential),
                ..
            } => (session_id, credential),
            loom_remote_protocol::session::SessionOpenReply::Ok {
                credential: None, ..
            } => return Err("logical session resume omitted rotated credential".to_string()),
            loom_remote_protocol::session::SessionOpenReply::Err(error) => {
                return Err(format!(
                    "logical session resume failed: {:?}: {}",
                    error.code, error.message
                ));
            }
        };
        let handle = daemon_generated_open_store(&paths, &session_id)?;
        Ok((
            Self {
                locator: store.to_string(),
                paths,
                session_id,
                handle,
                close_session_on_drop: false,
            },
            credential,
        ))
    }

    fn generated_unary(
        &self,
        interface: &str,
        method: &str,
        args: Vec<loom_codec::Value>,
    ) -> Result<loom_codec::Value, String> {
        let operation = format!("{interface}.{method}");
        let payload = daemon_generated_call(
            &self.paths,
            Some(self.session_id.clone()),
            interface,
            method,
            args,
        )?;
        let loom_remote_protocol::envelope::ResponsePayload::Ok(value) = payload else {
            return Err(daemon_generated_payload_error(&operation, payload));
        };
        Ok(value)
    }
}

#[cfg(feature = "serve")]
pub(crate) fn local_session_auth(keys: &KeyOpts) -> Result<SessionAuth, String> {
    match acquire_auth_session(keys)? {
        Some((principal, passphrase)) => Ok(SessionAuth::Passphrase {
            principal: *principal.as_bytes(),
            passphrase: passphrase.into_bytes(),
        }),
        None => Ok(SessionAuth::Unauthenticated),
    }
}

#[cfg(feature = "serve")]
impl Drop for DaemonLocalStore {
    fn drop(&mut self) {
        if !self.close_session_on_drop {
            return;
        }
        let _ = daemon_generated_call(
            &self.paths,
            Some(self.session_id.clone()),
            "Store",
            "close",
            vec![],
        );
    }
}

#[cfg(feature = "serve")]
fn daemon_generated_open_store(
    paths: &daemon::DaemonPaths,
    session_id: &[u8],
) -> Result<LoomSession, String> {
    let payload = daemon_generated_call(paths, Some(session_id.to_vec()), "Store", "open", vec![])?;
    let loom_remote_protocol::envelope::ResponsePayload::Ok(value) = payload else {
        return Err(daemon_generated_payload_error("Store.open", payload));
    };
    LoomSession::from_value(&value).map_err(|e| format!("decode daemon-local store handle: {e}"))
}

#[cfg(feature = "serve")]
fn daemon_generated_call(
    paths: &daemon::DaemonPaths,
    session_id: Option<Vec<u8>>,
    interface: &str,
    method: &str,
    args: Vec<loom_codec::Value>,
) -> Result<loom_remote_protocol::envelope::ResponsePayload, String> {
    static NEXT_REQUEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let request = loom_remote_protocol::envelope::Request {
        request_id: NEXT_REQUEST
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .to_be_bytes()
            .to_vec(),
        session_id,
        interface: interface.to_string(),
        method: method.to_string(),
        args,
        deadline_ms: 0,
        idempotency_key: None,
        principal_hint: None,
        compression: loom_remote_protocol::envelope::Compression::None,
        stream: false,
    };
    let request = request
        .encode()
        .map_err(|e| format!("encode daemon-local generated request: {e}"))?;
    let response = daemon::generated_call(paths, &request)
        .map_err(|e| format!("daemon-local generated dispatch failed: {e}"))?;
    loom_remote_protocol::envelope::Response::decode(&response)
        .map(|response| response.payload)
        .map_err(|e| format!("decode daemon-local generated response: {e}"))
}

#[cfg(feature = "serve")]
fn daemon_generated_payload_error(
    operation: &str,
    payload: loom_remote_protocol::envelope::ResponsePayload,
) -> String {
    match payload {
        loom_remote_protocol::envelope::ResponsePayload::Err(error) => {
            format!("{operation} failed: {:?}: {}", error.code, error.message)
        }
        other => format!("{operation} returned unexpected payload {other:?}"),
    }
}

/// Render a decoded [`loom_wire::store_admin::StoreStat`] as JSON (the remote `store stat` output).
fn store_stat_json(stat: &loom_wire::store_admin::StoreStat) -> String {
    format!(
        "{{\"object_count\":{},\"generation\":{},\"physical_page_count\":{},\"physical_bytes\":{},\"reusable_free_pages\":{},\"candidate_dead_pages\":{},\"last_validated_mark_epoch\":{},\"touched_segments\":{},\"candidate_segments\":{},\"segment_overflow\":{}}}",
        stat.object_count,
        stat.generation,
        stat.physical_page_count,
        stat.physical_bytes,
        stat.reusable_free_pages,
        stat.candidate_dead_pages,
        stat.last_validated_mark_epoch,
        stat.touched_segments,
        stat.candidate_segments,
        stat.segment_overflow
    )
}

pub(crate) fn generated_store_stat_json(
    context: CliExecutionContext,
    keys: &KeyOpts,
) -> Result<String, String> {
    let client = context.into_generated_client_with_keys(keys)?;
    let value = client.execute_unary(&CliGeneratedOperation::new(
        "StoreAdmin",
        "store_stat",
        Vec::new(),
    )?)?;
    let loom_codec::Value::Bytes(cbor) = value else {
        return Err(format!(
            "StoreAdmin.store_stat returned unexpected value {value:?}"
        ));
    };
    let stat = loom_wire::store_admin::store_stat_from_cbor(&cbor).map_err(|e| e.to_string())?;
    Ok(store_stat_json(&stat))
}

/// Whether `store` resolves to a remote endpoint, without opening a connection. Used to reject the
/// path-shaped `fs` import/export over a remote locator (fs-tree byte transfer is deferred).
pub(crate) fn target_is_remote(store: &str) -> Result<bool, String> {
    Ok(matches!(
        crate::locator_cx::current().resolve_target(store)?,
        Target::Remote(_)
    ))
}

/// Map a byte-transfer archive kind to the interchange `ArchiveKind`.
fn transfer_kind_to_archive(
    kind: loom_interchange_io::transfer::TransferKind,
) -> Result<loom_interchange::ArchiveKind, String> {
    use loom_interchange::ArchiveKind;
    use loom_interchange_io::transfer::TransferKind;
    Ok(match kind {
        TransferKind::Tar => ArchiveKind::Tar,
        TransferKind::TarZstd => ArchiveKind::TarZstd,
        TransferKind::TarGzip => ArchiveKind::TarGzip,
        TransferKind::Zip => ArchiveKind::Zip,
        TransferKind::Gzip => ArchiveKind::Gzip,
        other => {
            return Err(format!(
                "transfer kind '{}' has no archive codec",
                other.as_str()
            ));
        }
    })
}

/// Export a local `workspace` as a byte-transfer `kind` payload.
fn local_transfer_export_bytes(
    loom: &Loom<FileStore>,
    workspace: &str,
    kind: &str,
    revision: Option<&str>,
) -> Result<Vec<u8>, String> {
    use loom_interchange_io::transfer::TransferKind;
    let kind = TransferKind::parse(kind).map_err(|e| e.to_string())?;
    let ns = resolve_ns(loom, workspace)?;
    match kind {
        TransferKind::Car => {
            if revision.is_some() {
                return Err("car export does not support a revision selector".to_string());
            }
            let options = loom_interchange_io::CarExportOptions::new(workspace);
            Ok(loom_interchange_io::export_car_bytes(loom, ns, &options)
                .map_err(|e| e.to_string())?
                .bytes)
        }
        _ => {
            let archive_kind = transfer_kind_to_archive(kind)?;
            let mut options = loom_interchange_io::ArchiveExportOptions::new(workspace);
            options.revision = revision.map(str::to_string);
            Ok(
                loom_interchange_io::export_archive_bytes(loom, ns, archive_kind, &options)
                    .map_err(|e| e.to_string())?
                    .bytes,
            )
        }
    }
}

/// Resolve the `SessionAuth` for a remote endpoint from its `target.auth` selector. The selector is a
/// non-secret principal id (never credential material); the passphrase is acquired at connect time via the
/// interactive prompt and never stored in locator/config files. No selector means an unauthenticated
/// session; a bad passphrase fails at session open, not later at mutation time.
#[cfg(feature = "remote-client")]
pub(crate) fn remote_session_auth(
    target: &RemoteTarget,
    keys: &KeyOpts,
) -> Result<SessionAuth, String> {
    if let Some((principal, passphrase)) = acquire_auth_session(keys)? {
        return Ok(SessionAuth::Passphrase {
            principal: *principal.as_bytes(),
            passphrase: passphrase.into_bytes(),
        });
    }
    match target.auth.as_deref() {
        None => Ok(SessionAuth::Unauthenticated),
        Some(selector) => {
            let principal = WorkspaceId::parse(selector)
                .map_err(|e| format!("remote auth selector must be a principal id: {e}"))?;
            let passphrase = crate::acquire(
                &crate::KeySource::Prompt,
                "Remote principal passphrase",
                false,
            )?;
            Ok(SessionAuth::Passphrase {
                principal: *principal.as_bytes(),
                passphrase: passphrase.into_bytes(),
            })
        }
    }
}

impl StoreClient {
    /// Whether this client targets a remote endpoint (used to route path-shaped commands to the
    /// byte-transfer contract for remote and keep the server-local/admin path for local).
    pub(crate) fn is_remote(&self) -> bool {
        match self {
            StoreClient::Local { .. } => false,
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(_) => true,
        }
    }

    /// Export `workspace`'s Files facet (optionally at `revision`) as a `kind` payload to the local
    /// `local_path`: the server streams bytes and the client writes the destination path (no server
    /// `dst_path`, `specs/0067` §17.4). Returns a human summary.
    pub(crate) fn transfer_export(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        kind: &str,
        revision: Option<&str>,
        local_path: &str,
    ) -> Result<String, String> {
        let bytes = match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                local_transfer_export_bytes(&loom, workspace, kind, revision)?
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block_stream(Transfer::transfer_export(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                kind.to_string(),
                revision.map(str::to_string),
                Vec::new(),
            ))?,
        };
        std::fs::write(local_path, &bytes)
            .map_err(|e| format!("write transfer destination {local_path}: {e}"))?;
        Ok(format!("exported {} byte(s) to {local_path}", bytes.len()))
    }
}

// CLI-output bridge helpers for the remote arm. Each takes a canonical server response and produces the
// exact bytes (or values) the CLI presentation layer expects, so a remote locator prints the same output
// as a local one. These are named around CLI output (not protocol wire) to keep the direction clear.

/// Decode the 6 protected-ref policy fields (`[bool, bool, bool, uint, bool, bool]`, the
/// `protected_ref_policy_to_cbor` field order) into a typed `ProtectedRefPolicy`.
pub(crate) fn decode_protected_ref_policy_fields(
    fields: &[loom_codec::Value],
) -> Result<loom_core::vcs::ProtectedRefPolicy, String> {
    let flag = |index: usize| -> Result<bool, String> {
        match fields.get(index) {
            Some(loom_codec::Value::Bool(value)) => Ok(*value),
            _ => Err(
                "expected a bool in the protected-ref policy from the remote endpoint".to_string(),
            ),
        }
    };
    let required_review_count = match fields.get(3) {
        Some(loom_codec::Value::Uint(value)) => u32::try_from(*value)
            .map_err(|_| "protected-ref required_review_count out of range".to_string())?,
        _ => {
            return Err(
                "expected a uint required_review_count from the remote endpoint".to_string(),
            );
        }
    };
    Ok(loom_core::vcs::ProtectedRefPolicy {
        fast_forward_only: flag(0)?,
        signed_commits_required: flag(1)?,
        signed_ref_advance_required: flag(2)?,
        required_review_count,
        retention_lock: flag(4)?,
        governance_lock: flag(5)?,
    })
}

/// Decode a canonical `protected_ref_get` record (`[..6 policy fields]`) into a typed policy.
pub(crate) fn cli_protected_ref_policy_from_remote(
    wire: &[u8],
) -> Result<loom_core::vcs::ProtectedRefPolicy, String> {
    match loom_codec::decode(wire).map_err(|e| e.to_string())? {
        loom_codec::Value::Array(items) => decode_protected_ref_policy_fields(&items),
        _ => Err("expected a CBOR array from the remote endpoint".to_string()),
    }
}

/// Decode a canonical `workspace_info_to_cbor` record (`[id, name, [facet_tag...], head]`) into a typed
/// `WorkspaceInfo` for the CLI workspace-list presentation.
fn cli_workspace_info_from_remote(wire: &[u8]) -> Result<loom_core::WorkspaceInfo, String> {
    let items = match loom_codec::decode(wire).map_err(|e| e.to_string())? {
        loom_codec::Value::Array(items) => items,
        _ => return Err("expected a CBOR array from the remote endpoint".to_string()),
    };
    let id = match items.first() {
        Some(loom_codec::Value::Text(text)) => {
            loom_core::WorkspaceId::parse(text).map_err(|e| e.to_string())?
        }
        _ => return Err("expected a text workspace id from the remote endpoint".to_string()),
    };
    let name = match items.get(1) {
        Some(loom_codec::Value::Text(text)) => text.clone(),
        _ => return Err("expected a text workspace name from the remote endpoint".to_string()),
    };
    let facets = match items.get(2) {
        Some(loom_codec::Value::Array(tags)) => tags
            .iter()
            .map(|tag| match tag {
                loom_codec::Value::Uint(value) => {
                    let tag = u8::try_from(*value)
                        .map_err(|_| "workspace facet tag out of range".to_string())?;
                    FacetKind::from_stable_tag(tag)
                        .ok_or_else(|| format!("unknown workspace facet tag {tag}"))
                }
                _ => Err("expected a uint facet tag from the remote endpoint".to_string()),
            })
            .collect::<Result<Vec<_>, String>>()?,
        _ => return Err("expected a facet-tag array from the remote endpoint".to_string()),
    };
    let head = match items.get(3) {
        None | Some(loom_codec::Value::Null) => None,
        Some(loom_codec::Value::Text(text)) => {
            Some(Digest::parse(text).map_err(|e| e.to_string())?)
        }
        _ => return Err("expected a text head or null from the remote endpoint".to_string()),
    };
    Ok(loom_core::WorkspaceInfo {
        id,
        name,
        facets,
        head,
    })
}

fn cli_workspace_infos_from_generated_records(
    records: &[loom_codec::Value],
) -> Result<Vec<loom_core::WorkspaceInfo>, String> {
    records
        .iter()
        .map(|record| match record {
            loom_codec::Value::Bytes(bytes) => cli_workspace_info_from_remote(bytes),
            other => Err(format!(
                "Workspaces.workspace_list returned unexpected record {other:?}"
            )),
        })
        .collect()
}

fn cli_workspace_infos_from_remote_records(
    records: &[Vec<u8>],
) -> Result<Vec<loom_core::WorkspaceInfo>, String> {
    records
        .iter()
        .map(|record| cli_workspace_info_from_remote(record))
        .collect()
}

fn cli_select_workspace_id(
    infos: &[loom_core::WorkspaceInfo],
    workspace: &str,
) -> Option<loom_core::WorkspaceId> {
    let parsed = loom_core::WorkspaceId::parse(workspace).ok();
    infos
        .iter()
        .find(|info| match &parsed {
            Some(id) => info.id.as_bytes() == id.as_bytes(),
            None => info.name == workspace,
        })
        .map(|info| info.id)
}

/// Decode a canonical `protected_ref_list` record (`[ref_name, ..6 policy fields]`).
pub(crate) fn cli_named_protected_ref_from_remote(
    wire: &[u8],
) -> Result<(String, loom_core::vcs::ProtectedRefPolicy), String> {
    match loom_codec::decode(wire).map_err(|e| e.to_string())? {
        loom_codec::Value::Array(items) => {
            let name = match items.first() {
                Some(loom_codec::Value::Text(text)) => text.clone(),
                _ => return Err("expected a text ref name from the remote endpoint".to_string()),
            };
            let policy = decode_protected_ref_policy_fields(&items[1..])?;
            Ok((name, policy))
        }
        _ => Err("expected a CBOR array from the remote endpoint".to_string()),
    }
}

/// A connected remote endpoint: an async runtime, the discovered client, and the store session handle
/// returned by `Store.open`.
#[cfg(feature = "remote-client")]
pub(crate) struct RemoteStore {
    runtime: tokio::runtime::Runtime,
    client: RemoteLoomClient<Http2TlsTransport>,
    handle: LoomSession,
}

#[cfg(feature = "remote-client")]
impl RemoteStore {
    /// Connect to `target`, discover the endpoint, open a session over the carrier session route, and
    /// open the store, returning a ready client.
    pub(crate) fn connect(target: &RemoteTarget) -> Result<Self, String> {
        Self::connect_with_auth(target, SessionAuth::Unauthenticated)
    }

    /// Connect and open a session with the given authentication. `connect` opens an unauthenticated
    /// session; `open_store_client` resolves `target.auth` into a `SessionAuth::Passphrase` for
    /// authenticated endpoints. The auth is sent in `open_session`, where the hosted runtime validates it
    /// during session open (a bad passphrase fails here, not later at mutation time).
    pub(crate) fn connect_with_auth(
        target: &RemoteTarget,
        auth: SessionAuth,
    ) -> Result<Self, String> {
        use std::net::ToSocketAddrs;
        let (host, port) = url_host_port(&target.url)?;
        let addr = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| format!("resolve {host}:{port}: {e}"))?
            .next()
            .ok_or_else(|| format!("no address for {host}:{port}"))?;
        let call_path = format!("{}/v1/call", url_path(&target.url).trim_end_matches('/'));
        let client_config = build_client_config(target.tls.as_deref())?;
        let transport = Http2TlsTransport::new(addr, host, call_path, client_config);

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("build async runtime: {e}"))?;
        let url = target.url.clone();
        let mode = discovery_mode(target.discovery);
        let (client, handle) = runtime.block_on(async move {
            let conn =
                RemoteConnection::connect(transport, &url, &ContextResolver::default(), mode)
                    .await
                    .map_err(|e| e.to_string())?;
            let client = RemoteLoomClient::new(conn);
            client.open_session(auth).await.map_err(|e| e.to_string())?;
            let handle = Store::open(&client).await.map_err(|e| e.to_string())?;
            Ok::<_, String>((client, handle))
        })?;
        Ok(Self {
            runtime,
            client,
            handle,
        })
    }

    pub(crate) fn resume_logical_session(
        target: &RemoteTarget,
        auth: SessionAuth,
        credential: &[u8],
    ) -> Result<(Self, Vec<u8>), String> {
        use std::net::ToSocketAddrs;
        let (host, port) = url_host_port(&target.url)?;
        let addr = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|error| format!("resolve {host}:{port}: {error}"))?
            .next()
            .ok_or_else(|| format!("no address for {host}:{port}"))?;
        let call_path = format!("{}/v1/call", url_path(&target.url).trim_end_matches('/'));
        let transport = Http2TlsTransport::new(
            addr,
            host,
            call_path,
            build_client_config(target.tls.as_deref())?,
        );
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("build async runtime: {error}"))?;
        let url = target.url.clone();
        let mode = discovery_mode(target.discovery);
        let credential = credential.to_vec();
        let (client, handle, rotated) = runtime.block_on(async move {
            let connection =
                RemoteConnection::connect(transport, &url, &ContextResolver::default(), mode)
                    .await
                    .map_err(|error| error.to_string())?;
            let client = RemoteLoomClient::new(connection);
            let session = client
                .resume_logical_session(auth, &credential)
                .await
                .map_err(|error| error.to_string())?;
            let handle = Store::open(&client)
                .await
                .map_err(|error| error.to_string())?;
            Ok::<_, String>((client, handle, session.credential))
        })?;
        Ok((
            Self {
                runtime,
                client,
                handle,
            },
            rotated,
        ))
    }

    /// Drive `future` to completion on the endpoint's runtime, mapping the error to a message.
    fn block<F, T>(&self, future: F) -> Result<T, String>
    where
        F: std::future::Future<Output = Result<T, loom_types::LoomError>>,
    {
        self.runtime.block_on(future).map_err(|e| e.to_string())
    }

    /// Open a server-to-client byte stream (e.g. `Transfer::transfer_export`) and drain it to a single
    /// buffer, honoring the section-7 stream contract. Used by the byte-transfer export path.
    fn block_stream<F>(&self, future: F) -> Result<Vec<u8>, String>
    where
        F: std::future::Future<
                Output = Result<
                    loom_remote_protocol::api_types::LoomStream<Vec<u8>>,
                    loom_types::LoomError,
                >,
            >,
    {
        self.runtime
            .block_on(async move {
                use futures::StreamExt;
                let mut stream = future.await?;
                let mut buf = Vec::new();
                while let Some(item) = stream.next().await {
                    buf.extend(item?);
                }
                Ok::<Vec<u8>, loom_types::LoomError>(buf)
            })
            .map_err(|e| e.to_string())
    }
}

#[cfg(feature = "remote-client")]
pub(crate) fn create_remote_logical_session(
    target: &RemoteTarget,
    keys: &KeyOpts,
) -> Result<Vec<u8>, String> {
    use std::net::ToSocketAddrs;
    let auth = remote_session_auth(target, keys)?;
    let (host, port) = url_host_port(&target.url)?;
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| format!("resolve {host}:{port}: {error}"))?
        .next()
        .ok_or_else(|| format!("no address for {host}:{port}"))?;
    let call_path = format!("{}/v1/call", url_path(&target.url).trim_end_matches('/'));
    let transport = Http2TlsTransport::new(
        addr,
        host,
        call_path,
        build_client_config(target.tls.as_deref())?,
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("build async runtime: {error}"))?;
    let url = target.url.clone();
    let mode = discovery_mode(target.discovery);
    runtime.block_on(async move {
        let connection =
            RemoteConnection::connect(transport, &url, &ContextResolver::default(), mode)
                .await
                .map_err(|error| error.to_string())?;
        let client = RemoteLoomClient::new(connection);
        client
            .create_logical_session(auth)
            .await
            .map(|session| session.credential)
            .map_err(|error| error.to_string())
    })
}

#[cfg(feature = "remote-client")]
pub(crate) fn close_remote_logical_session(
    target: &RemoteTarget,
    keys: &KeyOpts,
    credential: &[u8],
) -> Result<(), String> {
    use std::net::ToSocketAddrs;
    let auth = remote_session_auth(target, keys)?;
    let (host, port) = url_host_port(&target.url)?;
    let addr = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| format!("resolve {host}:{port}: {error}"))?
        .next()
        .ok_or_else(|| format!("no address for {host}:{port}"))?;
    let call_path = format!("{}/v1/call", url_path(&target.url).trim_end_matches('/'));
    let transport = Http2TlsTransport::new(
        addr,
        host,
        call_path,
        build_client_config(target.tls.as_deref())?,
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("build async runtime: {error}"))?;
    let url = target.url.clone();
    let mode = discovery_mode(target.discovery);
    let credential = credential.to_vec();
    runtime.block_on(async move {
        let connection =
            RemoteConnection::connect(transport, &url, &ContextResolver::default(), mode)
                .await
                .map_err(|error| error.to_string())?;
        RemoteLoomClient::new(connection)
            .close_logical_session(auth, &credential)
            .await
            .map_err(|error| error.to_string())
    })
}

/// A remote backend for the MCP host: forwards the KV MCP tool family to a `loom serve remote` endpoint
/// over the same connection/session path the CLI remote facade uses. Each call runs on this backend's
/// own IO runtime and is awaited over a std channel, so it is safe to invoke from inside the MCP host's
/// serving runtime (no nested `block_on`).
#[cfg(all(feature = "mcp", feature = "remote-client"))]
pub(crate) struct McpRemoteBackend<T: Transport = Http2TlsTransport> {
    runtime: tokio::runtime::Runtime,
    client: Arc<RemoteLoomClient<T>>,
    handle: LoomSession,
    logical_auth: SessionAuth,
    logical_credential: std::sync::Mutex<Option<Vec<u8>>>,
}

#[cfg(all(feature = "mcp", feature = "remote-client"))]
impl McpRemoteBackend {
    /// Connect to `target` and open a session + store, mirroring `RemoteStore::connect` but keeping the
    /// client in an `Arc` so calls can be spawned onto the backend runtime.
    pub(crate) fn connect(target: &RemoteTarget) -> Result<Self, String> {
        use std::net::ToSocketAddrs;
        let (host, port) = url_host_port(&target.url)?;
        let addr = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|e| format!("resolve {host}:{port}: {e}"))?
            .next()
            .ok_or_else(|| format!("no address for {host}:{port}"))?;
        let call_path = format!("{}/v1/call", url_path(&target.url).trim_end_matches('/'));
        let client_config = build_client_config(target.tls.as_deref())?;
        let transport = Http2TlsTransport::new(addr, host, call_path, client_config);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("build async runtime: {e}"))?;
        let url = target.url.clone();
        let mode = discovery_mode(target.discovery);
        let logical_auth = SessionAuth::Unauthenticated;
        let auth = logical_auth.clone();
        let (client, handle, logical_credential) = runtime.block_on(async move {
            let conn =
                RemoteConnection::connect(transport, &url, &ContextResolver::default(), mode)
                    .await
                    .map_err(|e| e.to_string())?;
            let client = RemoteLoomClient::new(conn);
            let logical_session = client
                .create_logical_session(auth)
                .await
                .map_err(|e| e.to_string())?;
            let handle = Store::open(&client).await.map_err(|e| e.to_string())?;
            Ok::<_, String>((Arc::new(client), handle, logical_session.credential))
        })?;
        Ok(Self {
            runtime,
            client,
            handle,
            logical_auth,
            logical_credential: std::sync::Mutex::new(Some(logical_credential)),
        })
    }
}

#[cfg(all(feature = "mcp", feature = "remote-client", feature = "serve"))]
impl McpRemoteBackend<DaemonMcpTransport> {
    pub(crate) fn connect_local_daemon(store: &str, keys: &KeyOpts) -> Result<Self, String> {
        let paths = daemon::paths(store).map_err(|e| e.to_string())?;
        daemon::status_response(&paths)
            .map_err(|e| format!("local MCP requires a running daemon for {store:?}: {e}"))?;
        let transport = DaemonMcpTransport { paths };
        Self::connect_transport(
            "https://local-daemon.loom/",
            transport,
            DiscoveryMode::Default,
            local_session_auth(keys)?,
        )
    }
}

#[cfg(all(feature = "mcp", feature = "remote-client"))]
impl<T> McpRemoteBackend<T>
where
    T: Transport + Send + Sync + 'static,
{
    fn connect_transport(
        locator: &str,
        transport: T,
        mode: DiscoveryMode,
        auth: SessionAuth,
    ) -> Result<Self, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("build async runtime: {e}"))?;
        let locator = locator.to_string();
        let logical_auth = auth.clone();
        let (client, handle, logical_credential) = runtime.block_on(async move {
            let conn =
                RemoteConnection::connect(transport, &locator, &ContextResolver::default(), mode)
                    .await
                    .map_err(|e| e.to_string())?;
            let client = RemoteLoomClient::new(conn);
            let logical_session = client
                .create_logical_session(auth)
                .await
                .map_err(|e| e.to_string())?;
            let handle = Store::open(&client).await.map_err(|e| e.to_string())?;
            Ok::<_, String>((Arc::new(client), handle, logical_session.credential))
        })?;
        Ok(Self {
            runtime,
            client,
            handle,
            logical_auth,
            logical_credential: std::sync::Mutex::new(Some(logical_credential)),
        })
    }

    pub(crate) fn close_logical_session(&self) -> Result<(), String> {
        let credential = self
            .logical_credential
            .lock()
            .map_err(|_| "MCP logical-session credential lock is poisoned".to_string())?
            .take();
        let Some(credential) = credential else {
            return Ok(());
        };
        let result = self.runtime.block_on(
            self.client
                .close_logical_session(self.logical_auth.clone(), &credential),
        );
        if let Err(error) = result {
            *self
                .logical_credential
                .lock()
                .map_err(|_| "MCP logical-session credential lock is poisoned".to_string())? =
                Some(credential);
            return Err(error.to_string());
        }
        Ok(())
    }

    /// Resolve `workspace` (an id or a name) to its `WorkspaceId` via the remote workspace list, mirroring
    /// the local `resolve_ns` (a parseable id matches by id, otherwise by name). Needed because the watch
    /// selector wire form carries a `WorkspaceId`, which the remote MCP host cannot resolve locally.
    fn resolve_workspace_id(
        &self,
        workspace: &str,
    ) -> std::result::Result<loom_core::WorkspaceId, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Workspaces::workspace_list(client.as_ref(), handle).await);
        });
        let records = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        let infos = cli_workspace_infos_from_remote_records(&records)
            .map_err(loom_types::LoomError::invalid)?;
        if let Some(id) = cli_select_workspace_id(&infos, workspace) {
            return Ok(id);
        }
        Err(loom_types::LoomError::not_found(format!(
            "workspace {workspace:?}"
        )))
    }

    fn block_generated<R, F, C>(&self, call: C) -> std::result::Result<R, loom_types::LoomError>
    where
        R: Send + 'static,
        F: std::future::Future<Output = std::result::Result<R, loom_types::LoomError>>
            + Send
            + 'static,
        C: FnOnce(Arc<RemoteLoomClient<T>>, LoomSession) -> F + Send + 'static,
    {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(call(client, handle).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }
}

#[cfg(all(feature = "mcp", feature = "remote-client"))]
fn remote_backend_channel_closed() -> loom_types::LoomError {
    loom_types::LoomError::corrupt("remote MCP backend response channel closed")
}

#[cfg(all(feature = "mcp", feature = "remote-client"))]
fn decode_generated_json<T: serde::de::DeserializeOwned>(
    json: &str,
) -> std::result::Result<T, loom_types::LoomError> {
    serde_json::from_str(json)
        .map_err(|e| loom_types::LoomError::corrupt(format!("decode generated JSON result: {e}")))
}

#[cfg(all(feature = "mcp", feature = "remote-client"))]
fn optional_json_array<T>(
    items: &[T],
    encode: impl Fn(&T) -> serde_json::Value,
) -> std::result::Result<Option<String>, loom_types::LoomError> {
    if items.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(&items.iter().map(encode).collect::<Vec<_>>())
        .map(Some)
        .map_err(|e| loom_types::LoomError::invalid(format!("encode generated JSON array: {e}")))
}

#[cfg(all(feature = "mcp", feature = "remote-client"))]
fn ticket_action_name(action: loom_tickets::TicketLifecycleAction) -> &'static str {
    match action {
        loom_tickets::TicketLifecycleAction::Assign => "assign",
        loom_tickets::TicketLifecycleAction::Claim => "claim",
        loom_tickets::TicketLifecycleAction::Release => "release",
        loom_tickets::TicketLifecycleAction::RequestReview => "request_review",
        loom_tickets::TicketLifecycleAction::Accept => "accept",
        loom_tickets::TicketLifecycleAction::Reject => "reject",
        loom_tickets::TicketLifecycleAction::Block => "block",
        loom_tickets::TicketLifecycleAction::Complete => "complete",
    }
}

#[cfg(all(feature = "mcp", feature = "remote-client"))]
fn ticket_comment_update_json(
    comment: &loom_tickets::TicketUpdateCommentRequest<'_>,
) -> serde_json::Value {
    serde_json::json!({
        "comment_id": comment.comment_id,
        "comment_type": comment.comment_type,
        "body": comment.body,
        "evidence": comment.evidence,
    })
}

#[cfg(all(feature = "mcp", feature = "remote-client"))]
fn ticket_relation_set_json(
    relation: &loom_tickets::TicketUpdateRelationSetRequest<'_>,
) -> serde_json::Value {
    serde_json::json!({
        "relation_id": relation.relation_id,
        "kind": relation.kind.as_str(),
        "target_id": relation.target_id,
    })
}

#[cfg(all(feature = "mcp", feature = "remote-client"))]
fn ticket_relation_remove_json(
    relation: &loom_tickets::TicketUpdateRelationRemoveRequest<'_>,
) -> serde_json::Value {
    serde_json::json!({
        "relation_id": relation.relation_id,
    })
}

#[cfg(all(feature = "mcp", feature = "remote-client", feature = "serve"))]
pub(crate) struct DaemonMcpTransport {
    paths: daemon::DaemonPaths,
}

#[cfg(all(feature = "mcp", feature = "remote-client", feature = "serve"))]
fn daemon_mcp_transport_error(
    context: &str,
    error: impl std::fmt::Display,
) -> loom_types::LoomError {
    loom_types::LoomError::new(loom_types::Code::Io, format!("{context}: {error}"))
}

#[cfg(all(feature = "mcp", feature = "remote-client", feature = "serve"))]
impl Transport for DaemonMcpTransport {
    async fn discover(&self, _path: &str) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        loom_remote_protocol::discovery::Discovery::v1(
            "https://local-daemon.loom",
            "https://local-daemon.loom/v1/call",
            vec!["unauthenticated".to_string(), "passphrase".to_string()],
            Vec::new(),
        )
        .encode()
        .map_err(|e| daemon_mcp_transport_error("encode local daemon discovery", e))
    }

    async fn call(&self, request: Vec<u8>) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        daemon::generated_call(&self.paths, &request)
            .map_err(|e| daemon_mcp_transport_error("local daemon generated call", e))
    }

    async fn open_stream(
        &self,
        request: Vec<u8>,
    ) -> std::result::Result<FrameSource, loom_types::LoomError> {
        daemon::generated_stream(&self.paths, &request)
            .map(FrameSource::from_frames)
            .map_err(|e| daemon_mcp_transport_error("local daemon generated stream", e))
    }

    async fn open_session(
        &self,
        request: Vec<u8>,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        daemon::generated_session_open(&self.paths, &request)
            .map_err(|e| daemon_mcp_transport_error("local daemon session open", e))
    }
}

/// Parse an MCP watch change-kind string into a [`loom_core::ChangeKind`]. Mirrors the host's
/// `parse_watch_change_kind` so remote and local subscribe reject the same invalid inputs.
#[cfg(all(feature = "mcp", feature = "remote-client"))]
fn parse_watch_change_kind_cli(
    kind: &str,
) -> std::result::Result<loom_core::ChangeKind, loom_types::LoomError> {
    match kind {
        "added" => Ok(loom_core::ChangeKind::Added),
        "modified" => Ok(loom_core::ChangeKind::Modified),
        "deleted" => Ok(loom_core::ChangeKind::Deleted),
        _ => Err(loom_types::LoomError::invalid(format!(
            "watch change kind must be added, modified, or deleted, got {kind:?}"
        ))),
    }
}

#[cfg(all(feature = "mcp", feature = "remote-client"))]
impl<T> uldren_loom_mcp::RemoteMcpBackend for McpRemoteBackend<T>
where
    T: Transport + Send + Sync + 'static,
{
    fn execute_generated_operation(
        &self,
        call: uldren_loom_mcp::GeneratedMcpCall,
    ) -> uldren_loom_mcp::GeneratedMcpFuture<'_> {
        let sig = METHODS
            .iter()
            .find(|sig| sig.operation == call.operation)
            .ok_or_else(move || {
                loom_types::LoomError::not_found(format!(
                    "unknown generated operation {:?}",
                    call.operation
                ))
            });
        let sig = match sig {
            Ok(sig) => sig,
            Err(error) => return Box::pin(async move { Err(error) }),
        };
        if sig.ret.starts_with("stream<") {
            let error = loom_types::LoomError::unsupported(format!(
                "{}.{} is streaming and cannot use the unary MCP generated boundary",
                sig.interface, sig.method
            ));
            return Box::pin(async move { Err(error) });
        }
        if call.args_without_handle.len() != sig.args_without_handle.len() {
            let error = loom_types::LoomError::invalid(format!(
                "{}.{} expects {} MCP arguments, got {}",
                sig.interface,
                sig.method,
                sig.args_without_handle.len(),
                call.args_without_handle.len()
            ));
            return Box::pin(async move { Err(error) });
        }
        let mut args = Vec::with_capacity(sig.args.len());
        let mut next = 0usize;
        for (_, name) in sig.args {
            if *name == "handle" {
                args.push(self.handle.to_value());
            } else {
                args.push(call.args_without_handle[next].clone());
                next += 1;
            }
        }
        let options = if sig.requires_idempotency_key {
            self.client.idempotency_options()
        } else {
            loom_remote_client::CallOptions::default()
        };
        let client = self.client.clone();
        let interface = sig.interface;
        let method = sig.method;
        Box::pin(async move { client.call(interface, method, args, &options).await })
    }

    fn workspace_create(
        &self,
        name: Option<&str>,
        facet: Option<FacetKind>,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let name = name.map(str::to_string);
        let facet_bytes = facet.map(|facet| vec![facet.stable_tag()]);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Workspaces::workspace_create(client.as_ref(), handle, name, facet_bytes).await,
            );
        });
        Ok(loom_core::WorkspaceId::from_bytes(
            rx.recv().map_err(|_| remote_backend_channel_closed())??.0,
        )
        .to_string())
    }

    /// Thin-client transport: forward the whole MCP tool operation to the hosted server as an
    /// `Mcp.call_tool` request (`[tool_name, args_json]`) over the same session, and return the server's
    /// JSON result bytes. The local process does not reconstruct tool behavior; the server runs it beside
    /// the served store.
    fn execute_tool(
        &self,
        name: &str,
        args_json: &[u8],
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let name = name.to_string();
        let args = vec![
            loom_codec::Value::Text(name),
            loom_codec::Value::Bytes(args_json.to_vec()),
        ];
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                client
                    .call(
                        "Mcp",
                        "call_tool",
                        args,
                        &loom_remote_client::CallOptions::default(),
                    )
                    .await,
            );
        });
        match rx.recv().map_err(|_| remote_backend_channel_closed())?? {
            loom_codec::Value::Bytes(bytes) => Ok(bytes),
            other => Err(loom_types::LoomError::new(
                loom_types::Code::CorruptObject,
                format!("Mcp.call_tool returned a non-bytes value: {other:?}"),
            )),
        }
    }

    fn store_bundle_import(
        &self,
        bundle: &[u8],
        dry_run: bool,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let bundle = bundle.to_vec();
        self.block_generated(move |client, handle| async move {
            StoreAdmin::store_bundle_import(client.as_ref(), handle, bundle, dry_run).await
        })
    }

    fn store_maintenance_status(
        &self,
        request: &[u8],
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let request = request.to_vec();
        self.block_generated(move |client, handle| async move {
            StoreAdmin::store_maintenance_status(client.as_ref(), handle, request).await
        })
    }

    fn store_maintenance_policy_set(
        &self,
        update: &[u8],
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let update = update.to_vec();
        self.block_generated(move |client, handle| async move {
            StoreAdmin::store_maintenance_policy_set(client.as_ref(), handle, update).await
        })
    }

    fn store_maintenance_run(
        &self,
        request: &[u8],
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let request = request.to_vec();
        self.block_generated(move |client, handle| async move {
            StoreAdmin::store_maintenance_run(client.as_ref(), handle, request).await
        })
    }

    fn workspace_id(&self, workspace: &str) -> std::result::Result<String, loom_types::LoomError> {
        self.resolve_workspace_id(workspace)
            .map(|id| id.to_string())
    }

    fn tickets_create_json(
        &self,
        workspace: &str,
        request: loom_tickets::TicketCreateRequest<'_>,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let workspace = workspace.to_string();
        let workspace_id = request.workspace_id.to_string();
        let project_id = request.project_id.to_string();
        let ticket_type = request.ticket_type.to_string();
        let external_source = request.external_source.map(str::to_string);
        let external_id = request.external_id.map(str::to_string);
        let fields_json = serde_json::to_string(request.fields)
            .map_err(|e| loom_types::LoomError::invalid(format!("ticket fields json: {e}")))?;
        let policy_labels_json = serde_json::to_string(request.policy_labels).map_err(|e| {
            loom_types::LoomError::invalid(format!("ticket policy labels json: {e}"))
        })?;
        let expected_root = request.expected_root.map(str::to_string);
        self.block_generated(move |client, handle| async move {
            Tickets::tickets_create_json(
                client.as_ref(),
                handle,
                workspace,
                workspace_id,
                project_id,
                ticket_type,
                external_source,
                external_id,
                fields_json,
                policy_labels_json,
                expected_root,
            )
            .await
        })
    }

    fn tickets_update_json(
        &self,
        workspace: &str,
        request: loom_tickets::TicketUpdateRequest<'_>,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let workspace = workspace.to_string();
        let workspace_id = request.workspace_id.to_string();
        let ticket_id = request.ticket_id.to_string();
        let set_fields = request
            .set_fields
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| loom_types::LoomError::invalid(format!("ticket set fields json: {e}")))?;
        let delete_fields = serde_json::to_string(request.delete_fields).map_err(|e| {
            loom_types::LoomError::invalid(format!("ticket delete fields json: {e}"))
        })?;
        let action = request.action.map(ticket_action_name).map(str::to_string);
        let target_status = request.target_status.map(str::to_string);
        let observed_source_status = request.observed_source_status.map(str::to_string);
        let observed_workflow_version = request.observed_workflow_version.map(str::to_string);
        let assignee = request.assignee.map(str::to_string);
        let comment_id = request
            .comment
            .as_ref()
            .and_then(|comment| comment.comment_id.map(str::to_string));
        let comment_type = request
            .comment
            .as_ref()
            .and_then(|comment| comment.comment_type.map(str::to_string));
        let comment_body = request
            .comment
            .as_ref()
            .map(|comment| comment.body.to_string());
        let comment_evidence = request
            .comment
            .as_ref()
            .and_then(|comment| comment.evidence.as_ref())
            .map(|evidence| serde_json::to_string(&evidence))
            .transpose()
            .map_err(|e| loom_types::LoomError::invalid(format!("comment evidence json: {e}")))?;
        let expected_root = request.expected_root.map(str::to_string);
        let comments = optional_json_array(request.comments, ticket_comment_update_json)?;
        let relation_sets = optional_json_array(request.relation_sets, ticket_relation_set_json)?;
        let relation_removes =
            optional_json_array(request.relation_removes, ticket_relation_remove_json)?;
        self.block_generated(move |client, handle| async move {
            Tickets::tickets_update_json(
                client.as_ref(),
                handle,
                workspace,
                workspace_id,
                ticket_id,
                set_fields,
                delete_fields,
                action,
                target_status,
                observed_source_status,
                observed_workflow_version,
                assignee,
                comment_id,
                comment_type,
                comment_body,
                comment_evidence,
                expected_root,
                comments,
                relation_sets,
                relation_removes,
            )
            .await
        })
    }

    fn tickets_get_json(
        &self,
        workspace: &str,
        workspace_id: &str,
        ticket_id: &str,
        projection: Option<&str>,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let workspace = workspace.to_string();
        let workspace_id = workspace_id.to_string();
        let ticket_id = ticket_id.to_string();
        let projection = projection.map(str::to_string);
        self.block_generated(move |client, handle| async move {
            Tickets::tickets_get_json(
                client.as_ref(),
                handle,
                workspace,
                workspace_id,
                ticket_id,
                projection,
            )
            .await
        })
    }

    fn tickets_list_json(
        &self,
        workspace: &str,
        workspace_id: &str,
        query: &loom_tickets::TicketListQuery,
    ) -> std::result::Result<serde_json::Value, loom_types::LoomError> {
        let workspace = workspace.to_string();
        let workspace_id = workspace_id.to_string();
        let request = serde_json::json!({
            "projection": query.projection.map(loom_tickets::TicketProjectionProfile::profile_id),
            "statuses": &query.statuses,
            "assignees": &query.assignees,
            "priorities": &query.priorities,
            "ticket_types": &query.ticket_types,
            "labels": &query.labels,
            "policy_labels": &query.policy_labels,
            "lane": query.lane_id.as_deref(),
            "ready": query.ready_only,
            "include_completed": query.include_completed,
            "board": query.board_id.as_deref(),
            "cursor": query.cursor.as_deref(),
            "limit": query.limit,
        })
        .to_string();
        let json = self.block_generated(move |client, handle| async move {
            Tickets::tickets_list_json(
                client.as_ref(),
                handle,
                workspace,
                workspace_id,
                Some(request),
            )
            .await
        })?;
        decode_generated_json(&json)
    }

    fn tickets_history_json(
        &self,
        workspace: &str,
        workspace_id: &str,
        ticket_id: Option<&str>,
    ) -> std::result::Result<Vec<loom_tickets::TicketHistoryRecord>, loom_types::LoomError> {
        let workspace = workspace.to_string();
        let workspace_id = workspace_id.to_string();
        let ticket_id = ticket_id.map(str::to_string);
        let json = self.block_generated(move |client, handle| async move {
            Tickets::tickets_history_json(
                client.as_ref(),
                handle,
                workspace,
                workspace_id,
                ticket_id,
            )
            .await
        })?;
        decode_generated_json(&json)
    }

    fn lanes_get_view(
        &self,
        workspace: &str,
        ticket_workspace_id: &str,
        lane_id: &str,
        detailed: bool,
    ) -> std::result::Result<Option<loom_lanes::LaneView>, loom_types::LoomError> {
        let workspace = workspace.to_string();
        let ticket_workspace_id = ticket_workspace_id.to_string();
        let lane_id = lane_id.to_string();
        let json = self.block_generated(move |client, handle| async move {
            Lanes::get_view_json(
                client.as_ref(),
                handle,
                workspace,
                ticket_workspace_id,
                lane_id,
                detailed,
            )
            .await
        })?;
        decode_generated_json(&json)
    }

    fn lanes_list_views_json(
        &self,
        workspace: &str,
        ticket_workspace_id: &str,
    ) -> std::result::Result<Vec<loom_lanes::LaneView>, loom_types::LoomError> {
        let workspace = workspace.to_string();
        let ticket_workspace_id = ticket_workspace_id.to_string();
        let json = self.block_generated(move |client, handle| async move {
            Lanes::list_views_json(
                client.as_ref(),
                handle,
                workspace,
                ticket_workspace_id,
                true,
            )
            .await
        })?;
        decode_generated_json(&json)
    }

    fn spaces_create(
        &self,
        workspace: &str,
        workspace_id: &str,
        space_id: &str,
        title: &str,
        expected_root: Option<&str>,
    ) -> std::result::Result<loom_pages::SpaceSummary, loom_types::LoomError> {
        let workspace = workspace.to_string();
        let workspace_id = workspace_id.to_string();
        let space_id = space_id.to_string();
        let title = title.to_string();
        let expected_root = expected_root.map(str::to_string);
        let json = self.block_generated(move |client, handle| async move {
            Pages::spaces_create_json(
                client.as_ref(),
                handle,
                workspace,
                workspace_id,
                space_id,
                title,
                expected_root,
            )
            .await
        })?;
        decode_generated_json(&json)
    }

    fn spaces_get_json(
        &self,
        workspace: &str,
        workspace_id: &str,
        space_id: &str,
    ) -> std::result::Result<Option<loom_pages::SpaceSummary>, loom_types::LoomError> {
        let workspace = workspace.to_string();
        let workspace_id = workspace_id.to_string();
        let space_id = space_id.to_string();
        let json = self.block_generated(move |client, handle| async move {
            Pages::spaces_get_json(client.as_ref(), handle, workspace, workspace_id, space_id).await
        })?;
        decode_generated_json(&json)
    }

    fn pages_create(
        &self,
        workspace: &str,
        request: loom_pages::PageCreateRequest<'_>,
    ) -> std::result::Result<loom_pages::PageSummary, loom_types::LoomError> {
        let workspace = workspace.to_string();
        let workspace_id = request.workspace_id.to_string();
        let page_id = request.page_id.to_string();
        let space_id = request.space_id.to_string();
        let parent_page_id = request.parent_page_id.map(str::to_string);
        let title = request.title.to_string();
        let expected_root = request.expected_root.map(str::to_string);
        let json = self.block_generated(move |client, handle| async move {
            Pages::pages_create_json(
                client.as_ref(),
                handle,
                workspace,
                workspace_id,
                page_id,
                space_id,
                parent_page_id,
                title,
                expected_root,
            )
            .await
        })?;
        decode_generated_json(&json)
    }

    fn pages_update_text(
        &self,
        workspace: &str,
        workspace_id: &str,
        page_id: &str,
        body_text: &str,
        expected_root: Option<&str>,
    ) -> std::result::Result<loom_pages::PageUpdateSummary, loom_types::LoomError> {
        let workspace = workspace.to_string();
        let workspace_id = workspace_id.to_string();
        let page_id = page_id.to_string();
        let body_text = body_text.to_string();
        let expected_root = expected_root.map(str::to_string);
        let json = self.block_generated(move |client, handle| async move {
            Pages::pages_update_json(
                client.as_ref(),
                handle,
                workspace,
                workspace_id,
                page_id,
                body_text,
                expected_root,
            )
            .await
        })?;
        decode_generated_json(&json)
    }

    fn pages_publish(
        &self,
        workspace: &str,
        workspace_id: &str,
        page_id: &str,
        expected_root: Option<&str>,
    ) -> std::result::Result<loom_pages::PagePublishSummary, loom_types::LoomError> {
        let workspace = workspace.to_string();
        let workspace_id = workspace_id.to_string();
        let page_id = page_id.to_string();
        let expected_root = expected_root.map(str::to_string);
        let json = self.block_generated(move |client, handle| async move {
            Pages::pages_publish_json(
                client.as_ref(),
                handle,
                workspace,
                workspace_id,
                page_id,
                expected_root,
            )
            .await
        })?;
        decode_generated_json(&json)
    }

    fn pages_get(
        &self,
        workspace: &str,
        workspace_id: &str,
        page_id: &str,
    ) -> std::result::Result<Option<loom_pages::PageSummary>, loom_types::LoomError> {
        let workspace = workspace.to_string();
        let workspace_id = workspace_id.to_string();
        let page_id = page_id.to_string();
        let json = self.block_generated(move |client, handle| async move {
            Pages::pages_get_json(client.as_ref(), handle, workspace, workspace_id, page_id).await
        })?;
        decode_generated_json(&json)
    }

    fn lanes_create(
        &self,
        workspace: &str,
        lane: loom_lanes::Lane,
    ) -> std::result::Result<loom_lanes::Lane, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let workspace = workspace.to_string();
        let lane = lane.encode()?;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Lanes::create(client.as_ref(), handle, workspace, lane)
                    .await
                    .and_then(|lane| loom_lanes::Lane::decode(&lane)),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn lanes_get(
        &self,
        workspace: &str,
        lane_id: &str,
    ) -> std::result::Result<Option<loom_lanes::Lane>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let workspace = workspace.to_string();
        let lane_id = lane_id.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Lanes::get(client.as_ref(), handle, workspace, lane_id)
                    .await
                    .and_then(|lane| lane.map(|lane| loom_lanes::Lane::decode(&lane)).transpose()),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn lanes_list(
        &self,
        workspace: &str,
    ) -> std::result::Result<Vec<loom_lanes::Lane>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let workspace = workspace.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Lanes::list(client.as_ref(), handle, workspace)
                    .await
                    .and_then(|lanes| {
                        lanes
                            .iter()
                            .map(|lane| loom_lanes::Lane::decode(lane))
                            .collect::<std::result::Result<Vec<_>, _>>()
                    }),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn lanes_update(
        &self,
        workspace: &str,
        request: uldren_loom_mcp::RemoteLaneUpdate<'_>,
    ) -> std::result::Result<loom_lanes::Lane, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let workspace = workspace.to_string();
        let lane_id = request.lane_id.to_string();
        let title = request.title.map(str::to_string);
        let description = request.description.map(str::to_string);
        let lane_status = request.lane_status.map(str::to_string);
        let status_report = request.status_report.map(str::to_string);
        let reviewer_feedback = request.reviewer_feedback.map(str::to_string);
        let updated_by = request.updated_by.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Lanes::update(
                    client.as_ref(),
                    handle,
                    workspace,
                    lane_id,
                    title,
                    description,
                    lane_status,
                    status_report,
                    reviewer_feedback,
                    updated_by,
                )
                .await
                .and_then(|lane| loom_lanes::Lane::decode(&lane)),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn lanes_ticket_add(
        &self,
        workspace: &str,
        lane_id: &str,
        ticket_id: &str,
        placement: loom_lanes::LaneTicketPlacement<'_>,
        updated_by: &str,
    ) -> std::result::Result<loom_lanes::Lane, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let workspace = workspace.to_string();
        let lane_id = lane_id.to_string();
        let ticket_id = ticket_id.to_string();
        let (placement, anchor) = remote_lane_ticket_placement_parts(placement);
        let updated_by = updated_by.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Lanes::ticket_add(
                    client.as_ref(),
                    handle,
                    workspace,
                    lane_id,
                    ticket_id,
                    placement,
                    anchor,
                    updated_by,
                )
                .await
                .and_then(|lane| loom_lanes::Lane::decode(&lane)),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn lanes_ticket_remove(
        &self,
        workspace: &str,
        lane_id: &str,
        ticket_id: &str,
        updated_by: &str,
    ) -> std::result::Result<loom_lanes::Lane, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let workspace = workspace.to_string();
        let lane_id = lane_id.to_string();
        let ticket_id = ticket_id.to_string();
        let updated_by = updated_by.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Lanes::ticket_remove(
                    client.as_ref(),
                    handle,
                    workspace,
                    lane_id,
                    ticket_id,
                    updated_by,
                )
                .await
                .and_then(|lane| loom_lanes::Lane::decode(&lane)),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn document_put_binary_indexed(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
        bytes: Vec<u8>,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll, id) = (
            workspace.to_string(),
            collection.to_string(),
            id.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Document::put_binary_indexed(client.as_ref(), handle, ws, coll, id, bytes).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn document_delete_indexed(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll, id) = (
            workspace.to_string(),
            collection.to_string(),
            id.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Document::delete_indexed(client.as_ref(), handle, ws, coll, id).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn document_delete_collection(
        &self,
        workspace: &str,
        collection: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let workspace = workspace.to_string();
        let collection = collection.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Document::delete_collection(client.as_ref(), handle, workspace, collection).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn document_replace_text_indexed(
        &self,
        request: uldren_loom_mcp::writes::DocumentReplaceTextRequest<'_>,
    ) -> std::result::Result<
        uldren_loom_mcp::writes::DocumentReplaceTextResult,
        loom_types::LoomError,
    > {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let workspace = request.workspace.to_string();
        let collection = request.name.to_string();
        let id = request.id.to_string();
        let find = request.find.to_string();
        let replace = request.replace.to_string();
        let replace_all = request.replace_all;
        let base_digest = WireDigest(request.base_digest.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Document::replace_text_indexed(
                    client.as_ref(),
                    handle,
                    workspace,
                    collection,
                    id,
                    find,
                    replace,
                    replace_all,
                    base_digest,
                )
                .await
                .and_then(|bytes| {
                    loom_wire::document::replace_text_result_from_cbor(&bytes).map(
                        |(replacements, digest, entity_tag)| {
                            uldren_loom_mcp::writes::DocumentReplaceTextResult {
                                replacements,
                                digest,
                                entity_tag,
                            }
                        },
                    )
                }),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn graph_upsert_edge_indexed(
        &self,
        workspace: &str,
        name: &str,
        edge: uldren_loom_mcp::writes::GraphEdgeWrite<'_>,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let workspace = workspace.to_string();
        let name = name.to_string();
        let id = edge.id.to_string();
        let src = edge.src.to_string();
        let dst = edge.dst.to_string();
        let label = edge.label.to_string();
        let props = edge.props.to_vec();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Graph::upsert_edge_indexed(
                    client.as_ref(),
                    handle,
                    workspace,
                    name,
                    id,
                    src,
                    dst,
                    label,
                    props,
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn graph_remove_edge_indexed(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (workspace, name, id) = (workspace.to_string(), name.to_string(), id.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Graph::remove_edge_indexed(client.as_ref(), handle, workspace, name, id).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn kv_get(
        &self,
        workspace: &str,
        collection: &str,
        key_cbor: &[u8],
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll, key) = (
            workspace.to_string(),
            collection.to_string(),
            key_cbor.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Kv::get(client.as_ref(), handle, ws, coll, key).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn kv_put(
        &self,
        workspace: &str,
        collection: &str,
        key_cbor: &[u8],
        value: Vec<u8>,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll, key) = (
            workspace.to_string(),
            collection.to_string(),
            key_cbor.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Kv::put(client.as_ref(), handle, ws, coll, key, value).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn kv_delete(
        &self,
        workspace: &str,
        collection: &str,
        key_cbor: &[u8],
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll, key) = (
            workspace.to_string(),
            collection.to_string(),
            key_cbor.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Kv::delete(client.as_ref(), handle, ws, coll, key).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn kv_list(
        &self,
        workspace: &str,
        collection: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll) = (workspace.to_string(), collection.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Kv::list(client.as_ref(), handle, ws, coll).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn kv_range(
        &self,
        workspace: &str,
        collection: &str,
        lo_cbor: &[u8],
        hi_cbor: &[u8],
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll, lo, hi) = (
            workspace.to_string(),
            collection.to_string(),
            lo_cbor.to_vec(),
            hi_cbor.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Kv::range(client.as_ref(), handle, ws, coll, lo, hi).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn cas_put(
        &self,
        workspace: &str,
        content: &[u8],
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, content) = (workspace.to_string(), content.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Cas::put(client.as_ref(), handle, ws, content)
                    .await
                    .map(|d| d.0),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn cas_get(
        &self,
        workspace: &str,
        digest: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, digest) = (workspace.to_string(), digest.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Cas::get(
                    client.as_ref(),
                    handle,
                    ws,
                    loom_remote_protocol::api_types::Digest(digest),
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn cas_has(
        &self,
        workspace: &str,
        digest: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, digest) = (workspace.to_string(), digest.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Cas::has(
                    client.as_ref(),
                    handle,
                    ws,
                    loom_remote_protocol::api_types::Digest(digest),
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn cas_delete(
        &self,
        workspace: &str,
        digest: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, digest) = (workspace.to_string(), digest.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Cas::delete(
                    client.as_ref(),
                    handle,
                    ws,
                    loom_remote_protocol::api_types::Digest(digest),
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn cas_list(&self, workspace: &str) -> std::result::Result<Vec<String>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let ws = workspace.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Cas::list(client.as_ref(), handle, ws)
                    .await
                    .map(|v| v.into_iter().map(|d| d.0).collect()),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn queue_append(
        &self,
        workspace: &str,
        stream: &str,
        entry: &[u8],
    ) -> std::result::Result<u64, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, stream, entry) = (workspace.to_string(), stream.to_string(), entry.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Queue::append(client.as_ref(), handle, ws, stream, entry).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn queue_get(
        &self,
        workspace: &str,
        stream: &str,
        seq: u64,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, stream) = (workspace.to_string(), stream.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Queue::get(client.as_ref(), handle, ws, stream, seq).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn queue_range(
        &self,
        workspace: &str,
        stream: &str,
        lo: u64,
        hi: u64,
    ) -> std::result::Result<Vec<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, stream) = (workspace.to_string(), stream.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Queue::range(client.as_ref(), handle, ws, stream, lo, hi).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn queue_len(
        &self,
        workspace: &str,
        stream: &str,
    ) -> std::result::Result<u64, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, stream) = (workspace.to_string(), stream.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Queue::len(client.as_ref(), handle, ws, stream).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn queue_consumer_position(
        &self,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
    ) -> std::result::Result<u64, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, stream, consumer) = (
            workspace.to_string(),
            stream.to_string(),
            consumer_id.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                QueueConsumers::consumer_position(client.as_ref(), handle, ws, stream, consumer)
                    .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn queue_consumer_read(
        &self,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
        max: u32,
    ) -> std::result::Result<Vec<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, stream, consumer) = (
            workspace.to_string(),
            stream.to_string(),
            consumer_id.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                QueueConsumers::consumer_read(client.as_ref(), handle, ws, stream, consumer, max)
                    .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn queue_consumer_advance(
        &self,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
        next_seq: u64,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, stream, consumer) = (
            workspace.to_string(),
            stream.to_string(),
            consumer_id.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                QueueConsumers::consumer_advance(
                    client.as_ref(),
                    handle,
                    ws,
                    stream,
                    consumer,
                    next_seq,
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn queue_consumer_reset(
        &self,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
        next_seq: u64,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, stream, consumer) = (
            workspace.to_string(),
            stream.to_string(),
            consumer_id.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                QueueConsumers::consumer_reset(
                    client.as_ref(),
                    handle,
                    ws,
                    stream,
                    consumer,
                    next_seq,
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn ledger_append(
        &self,
        workspace: &str,
        collection: &str,
        payload: Vec<u8>,
    ) -> std::result::Result<u64, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll) = (workspace.to_string(), collection.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Ledger::append(client.as_ref(), handle, ws, coll, payload).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn ledger_get(
        &self,
        workspace: &str,
        collection: &str,
        seq: u64,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll) = (workspace.to_string(), collection.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Ledger::get(client.as_ref(), handle, ws, coll, seq).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn ledger_head(
        &self,
        workspace: &str,
        collection: &str,
    ) -> std::result::Result<Option<String>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll) = (workspace.to_string(), collection.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Ledger::head(client.as_ref(), handle, ws, coll)
                    .await
                    .map(|o| o.map(|d| d.0)),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn ledger_len(
        &self,
        workspace: &str,
        collection: &str,
    ) -> std::result::Result<u64, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll) = (workspace.to_string(), collection.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Ledger::len(client.as_ref(), handle, ws, coll).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn ledger_verify(
        &self,
        workspace: &str,
        collection: &str,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll) = (workspace.to_string(), collection.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Ledger::verify(client.as_ref(), handle, ws, coll).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn ts_get(
        &self,
        workspace: &str,
        collection: &str,
        ts: i64,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll) = (workspace.to_string(), collection.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(TimeSeries::get(client.as_ref(), handle, ws, coll, ts).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn ts_put(
        &self,
        workspace: &str,
        collection: &str,
        ts: i64,
        value: Vec<u8>,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll) = (workspace.to_string(), collection.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(TimeSeries::put(client.as_ref(), handle, ws, coll, ts, value).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn ts_range(
        &self,
        workspace: &str,
        collection: &str,
        from: i64,
        to: i64,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, coll) = (workspace.to_string(), collection.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(TimeSeries::range(client.as_ref(), handle, ws, coll, from, to).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn search_create(
        &self,
        workspace: &str,
        name: &str,
        mapping: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, mapping) = (workspace.to_string(), name.to_string(), mapping.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Search::create(client.as_ref(), handle, ws, name, mapping).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn search_index(
        &self,
        workspace: &str,
        name: &str,
        id: Vec<u8>,
        doc: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, doc) = (workspace.to_string(), name.to_string(), doc.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Search::index(client.as_ref(), handle, ws, name, id, doc).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn search_get(
        &self,
        workspace: &str,
        name: &str,
        id: &[u8],
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, id) = (workspace.to_string(), name.to_string(), id.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Search::get(client.as_ref(), handle, ws, name, id).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn search_delete(
        &self,
        workspace: &str,
        name: &str,
        id: &[u8],
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, id) = (workspace.to_string(), name.to_string(), id.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Search::delete(client.as_ref(), handle, ws, name, id).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn search_ids(
        &self,
        workspace: &str,
        name: &str,
        prefix: Option<&[u8]>,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (prefix, has_prefix) = match prefix {
            Some(p) => (p.to_vec(), true),
            None => (Vec::new(), false),
        };
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Search::ids(client.as_ref(), handle, ws, name, prefix, has_prefix).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn search_remap(
        &self,
        workspace: &str,
        name: &str,
        mapping: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, mapping) = (workspace.to_string(), name.to_string(), mapping.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Search::remap(client.as_ref(), handle, ws, name, mapping).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn search_query(
        &self,
        workspace: &str,
        name: &str,
        request: &[u8],
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, request) = (workspace.to_string(), name.to_string(), request.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Search::query(client.as_ref(), handle, ws, name, request).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn search_source_digest(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Search::source_digest(client.as_ref(), handle, ws, name)
                    .await
                    .map(|d| d.0),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn search_status(
        &self,
        workspace: &str,
        name: &str,
        engine_version: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, engine_version) = (
            workspace.to_string(),
            name.to_string(),
            engine_version.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Search::status(client.as_ref(), handle, ws, name, engine_version).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn columnar_create(
        &self,
        workspace: &str,
        name: &str,
        columns: &[u8],
        target_segment_rows: u64,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, columns) = (workspace.to_string(), name.to_string(), columns.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Columnar::create(
                    client.as_ref(),
                    handle,
                    ws,
                    name,
                    columns,
                    target_segment_rows,
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn columnar_append(
        &self,
        workspace: &str,
        name: &str,
        row: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, row) = (workspace.to_string(), name.to_string(), row.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Columnar::append(client.as_ref(), handle, ws, name, row).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn columnar_compact(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Columnar::compact(client.as_ref(), handle, ws, name).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn columnar_scan(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Columnar::scan(client.as_ref(), handle, ws, name).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn columnar_columns(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Columnar::columns(client.as_ref(), handle, ws, name).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn columnar_rows(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<u64, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Columnar::rows(client.as_ref(), handle, ws, name).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn columnar_inspect(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Columnar::inspect(client.as_ref(), handle, ws, name).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn columnar_source_digest(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Columnar::source_digest(client.as_ref(), handle, ws, name).await);
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        match loom_codec::decode(&wire) {
            Ok(loom_codec::Value::Text(text)) => Ok(text),
            _ => Err(loom_types::LoomError::corrupt(
                "expected a cbor text digest from the remote endpoint",
            )),
        }
    }

    fn columnar_select(
        &self,
        workspace: &str,
        name: &str,
        columns: &[u8],
        filter: &[u8],
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, columns, filter) = (
            workspace.to_string(),
            name.to_string(),
            columns.to_vec(),
            filter.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Columnar::select(client.as_ref(), handle, ws, name, columns, filter).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn columnar_aggregate(
        &self,
        workspace: &str,
        name: &str,
        aggregates: &[u8],
        filter: &[u8],
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, aggregates, filter) = (
            workspace.to_string(),
            name.to_string(),
            aggregates.to_vec(),
            filter.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Columnar::aggregate(client.as_ref(), handle, ws, name, aggregates, filter).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn columnar_import_arrow(
        &self,
        workspace: &str,
        name: &str,
        payload: &[u8],
        target_segment_rows: u64,
        replace: bool,
        dry_run: bool,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let (workspace, name, payload) =
            (workspace.to_string(), name.to_string(), payload.to_vec());
        self.block_generated(move |client, handle| async move {
            Columnar::columnar_import_arrow(
                client.as_ref(),
                handle,
                workspace,
                name,
                payload,
                target_segment_rows,
                replace,
                dry_run,
            )
            .await
        })
    }

    fn columnar_import_parquet(
        &self,
        workspace: &str,
        name: &str,
        payload: &[u8],
        target_segment_rows: u64,
        replace: bool,
        dry_run: bool,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let (workspace, name, payload) =
            (workspace.to_string(), name.to_string(), payload.to_vec());
        self.block_generated(move |client, handle| async move {
            Columnar::columnar_import_parquet(
                client.as_ref(),
                handle,
                workspace,
                name,
                payload,
                target_segment_rows,
                replace,
                dry_run,
            )
            .await
        })
    }

    fn calendar_create_collection(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        meta: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, collection, meta) = (
            workspace.to_string(),
            principal.to_string(),
            collection.to_string(),
            meta.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Calendar::create_collection(
                    client.as_ref(),
                    handle,
                    ws,
                    principal,
                    collection,
                    meta,
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn calendar_delete_collection(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, collection) = (
            workspace.to_string(),
            principal.to_string(),
            collection.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Calendar::delete_collection(client.as_ref(), handle, ws, principal, collection)
                    .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn calendar_put_entry(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        entry: &[u8],
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, collection, entry) = (
            workspace.to_string(),
            principal.to_string(),
            collection.to_string(),
            entry.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Calendar::put_entry(client.as_ref(), handle, ws, principal, collection, entry)
                    .await
                    .map(|d| d.0),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn calendar_put_ics(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        ics: &str,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, collection, ics) = (
            workspace.to_string(),
            principal.to_string(),
            collection.to_string(),
            ics.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Calendar::put_ics(client.as_ref(), handle, ws, principal, collection, ics)
                    .await
                    .map(|d| d.0),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn calendar_delete_entry(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, collection, uid) = (
            workspace.to_string(),
            principal.to_string(),
            collection.to_string(),
            uid.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Calendar::delete_entry(client.as_ref(), handle, ws, principal, collection, uid)
                    .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn calendar_get_entry(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, collection, uid) = (
            workspace.to_string(),
            principal.to_string(),
            collection.to_string(),
            uid.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Calendar::get_entry(client.as_ref(), handle, ws, principal, collection, uid).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn calendar_list_entries(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> std::result::Result<Vec<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, collection) = (
            workspace.to_string(),
            principal.to_string(),
            collection.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Calendar::list_entries(client.as_ref(), handle, ws, principal, collection).await,
            );
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        remote_bytes_list(&wire)
    }

    fn calendar_get_collection(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, collection) = (
            workspace.to_string(),
            principal.to_string(),
            collection.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Calendar::get_collection(client.as_ref(), handle, ws, principal, collection).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn calendar_list_collections(
        &self,
        workspace: &str,
        principal: &str,
    ) -> std::result::Result<Vec<String>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal) = (workspace.to_string(), principal.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Calendar::list_collections(client.as_ref(), handle, ws, principal).await);
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        remote_text_list(&wire)
    }

    fn calendar_range(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        from: &str,
        to: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, collection, from, to) = (
            workspace.to_string(),
            principal.to_string(),
            collection.to_string(),
            from.to_string(),
            to.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Calendar::range(client.as_ref(), handle, ws, principal, collection, from, to).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn calendar_search(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        component: &str,
        text: &str,
    ) -> std::result::Result<Vec<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, collection, component, text) = (
            workspace.to_string(),
            principal.to_string(),
            collection.to_string(),
            component.to_string(),
            text.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Calendar::search(
                    client.as_ref(),
                    handle,
                    ws,
                    principal,
                    collection,
                    component,
                    text,
                )
                .await,
            );
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        remote_bytes_list(&wire)
    }

    fn calendar_to_ics(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, collection, uid) = (
            workspace.to_string(),
            principal.to_string(),
            collection.to_string(),
            uid.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Calendar::to_ics(client.as_ref(), handle, ws, principal, collection, uid).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn contacts_create_book(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        meta: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, book, meta) = (
            workspace.to_string(),
            principal.to_string(),
            book.to_string(),
            meta.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Contacts::create_book(client.as_ref(), handle, ws, principal, book, meta).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn contacts_delete_book(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, book) = (
            workspace.to_string(),
            principal.to_string(),
            book.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Contacts::delete_book(client.as_ref(), handle, ws, principal, book).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn contacts_put_entry(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        entry: &[u8],
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, book, entry) = (
            workspace.to_string(),
            principal.to_string(),
            book.to_string(),
            entry.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Contacts::put_entry(client.as_ref(), handle, ws, principal, book, entry)
                    .await
                    .map(|d| d.0),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn contacts_put_vcard(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        vcard: &str,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, book, vcard) = (
            workspace.to_string(),
            principal.to_string(),
            book.to_string(),
            vcard.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Contacts::put_vcard(client.as_ref(), handle, ws, principal, book, vcard)
                    .await
                    .map(|d| d.0),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn contacts_delete_entry(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, book, uid) = (
            workspace.to_string(),
            principal.to_string(),
            book.to_string(),
            uid.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Contacts::delete_entry(client.as_ref(), handle, ws, principal, book, uid).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn contacts_get_entry(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, book, uid) = (
            workspace.to_string(),
            principal.to_string(),
            book.to_string(),
            uid.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx
                .send(Contacts::get_entry(client.as_ref(), handle, ws, principal, book, uid).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn contacts_list_entries(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
    ) -> std::result::Result<Vec<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, book) = (
            workspace.to_string(),
            principal.to_string(),
            book.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Contacts::list_entries(client.as_ref(), handle, ws, principal, book).await);
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        remote_bytes_list(&wire)
    }

    fn contacts_get_book(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, book) = (
            workspace.to_string(),
            principal.to_string(),
            book.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Contacts::get_book(client.as_ref(), handle, ws, principal, book).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn contacts_list_books(
        &self,
        workspace: &str,
        principal: &str,
    ) -> std::result::Result<Vec<String>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal) = (workspace.to_string(), principal.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Contacts::list_books(client.as_ref(), handle, ws, principal).await);
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        remote_text_list(&wire)
    }

    fn contacts_search(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        text: &str,
    ) -> std::result::Result<Vec<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, book, text) = (
            workspace.to_string(),
            principal.to_string(),
            book.to_string(),
            text.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Contacts::search(client.as_ref(), handle, ws, principal, book, text).await);
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        remote_bytes_list(&wire)
    }

    fn contacts_to_vcard(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, book, uid) = (
            workspace.to_string(),
            principal.to_string(),
            book.to_string(),
            uid.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx
                .send(Contacts::to_vcard(client.as_ref(), handle, ws, principal, book, uid).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn mail_create_mailbox(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        meta: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, mailbox, meta) = (
            workspace.to_string(),
            principal.to_string(),
            mailbox.to_string(),
            meta.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Mail::create_mailbox(client.as_ref(), handle, ws, principal, mailbox, meta).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn mail_delete_mailbox(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, mailbox) = (
            workspace.to_string(),
            principal.to_string(),
            mailbox.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx
                .send(Mail::delete_mailbox(client.as_ref(), handle, ws, principal, mailbox).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn mail_ingest_message(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
        raw: &[u8],
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, mailbox, uid, raw) = (
            workspace.to_string(),
            principal.to_string(),
            mailbox.to_string(),
            uid.to_string(),
            raw.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Mail::ingest_message(client.as_ref(), handle, ws, principal, mailbox, uid, raw)
                    .await
                    .map(|d| d.0),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn mail_delete_message(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, mailbox, uid) = (
            workspace.to_string(),
            principal.to_string(),
            mailbox.to_string(),
            uid.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Mail::delete_message(client.as_ref(), handle, ws, principal, mailbox, uid).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn mail_set_flags(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
        flags: &[String],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let encoded = encode_text_list(flags);
        let (ws, principal, mailbox, uid) = (
            workspace.to_string(),
            principal.to_string(),
            mailbox.to_string(),
            uid.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Mail::set_flags(
                    client.as_ref(),
                    handle,
                    ws,
                    principal,
                    mailbox,
                    uid,
                    encoded,
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn mail_get_message(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, mailbox, uid) = (
            workspace.to_string(),
            principal.to_string(),
            mailbox.to_string(),
            uid.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Mail::get_message(client.as_ref(), handle, ws, principal, mailbox, uid).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn mail_to_eml(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, mailbox, uid) = (
            workspace.to_string(),
            principal.to_string(),
            mailbox.to_string(),
            uid.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Mail::to_eml(client.as_ref(), handle, ws, principal, mailbox, uid).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn mail_list_messages(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
    ) -> std::result::Result<Vec<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, mailbox) = (
            workspace.to_string(),
            principal.to_string(),
            mailbox.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Mail::list_messages(client.as_ref(), handle, ws, principal, mailbox).await);
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        remote_bytes_list(&wire)
    }

    fn mail_get_mailbox(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, mailbox) = (
            workspace.to_string(),
            principal.to_string(),
            mailbox.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Mail::get_mailbox(client.as_ref(), handle, ws, principal, mailbox).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn mail_list_mailboxes(
        &self,
        workspace: &str,
        principal: &str,
    ) -> std::result::Result<Vec<String>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal) = (workspace.to_string(), principal.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Mail::list_mailboxes(client.as_ref(), handle, ws, principal).await);
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        remote_text_list(&wire)
    }

    fn mail_get_flags(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> std::result::Result<Vec<String>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, mailbox, uid) = (
            workspace.to_string(),
            principal.to_string(),
            mailbox.to_string(),
            uid.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx
                .send(Mail::get_flags(client.as_ref(), handle, ws, principal, mailbox, uid).await);
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        remote_text_list(&wire)
    }

    fn mail_search(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        text: &str,
    ) -> std::result::Result<Vec<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, principal, mailbox, text) = (
            workspace.to_string(),
            principal.to_string(),
            mailbox.to_string(),
            text.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Mail::search(client.as_ref(), handle, ws, principal, mailbox, text).await);
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        remote_bytes_list(&wire)
    }

    fn fs_read_file(
        &self,
        workspace: &str,
        path: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path) = (workspace.to_string(), path.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(FileSystem::read_file(client.as_ref(), handle, ws, path).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn fs_read_link(
        &self,
        workspace: &str,
        path: &str,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path) = (workspace.to_string(), path.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(FileSystem::read_link(client.as_ref(), handle, ws, path).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn fs_read_at(
        &self,
        workspace: &str,
        path: &str,
        offset: u64,
        len: u64,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path) = (workspace.to_string(), path.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(FileSystem::read_at(client.as_ref(), handle, ws, path, offset, len).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn fs_stat(
        &self,
        workspace: &str,
        path: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path) = (workspace.to_string(), path.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(FileSystem::stat(client.as_ref(), handle, ws, path).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn fs_list_directory(
        &self,
        workspace: &str,
        path: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path) = (workspace.to_string(), path.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(FileSystem::list_directory(client.as_ref(), handle, ws, path).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn fs_write_file(
        &self,
        workspace: &str,
        path: &str,
        content: &[u8],
        mode: u32,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path, content) = (workspace.to_string(), path.to_string(), content.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                FileSystem::write_file(client.as_ref(), handle, ws, path, content, mode).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn fs_append_file(
        &self,
        workspace: &str,
        path: &str,
        content: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path, content) = (workspace.to_string(), path.to_string(), content.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(FileSystem::append_file(client.as_ref(), handle, ws, path, content).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn fs_remove_file(
        &self,
        workspace: &str,
        path: &str,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path) = (workspace.to_string(), path.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(FileSystem::remove_file(client.as_ref(), handle, ws, path).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn fs_create_directory(
        &self,
        workspace: &str,
        path: &str,
        recursive: bool,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path) = (workspace.to_string(), path.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                FileSystem::create_directory(client.as_ref(), handle, ws, path, recursive).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn fs_remove_directory(
        &self,
        workspace: &str,
        path: &str,
        recursive: bool,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path) = (workspace.to_string(), path.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                FileSystem::remove_directory(client.as_ref(), handle, ws, path, recursive).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn fs_write_at(
        &self,
        workspace: &str,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path, data) = (workspace.to_string(), path.to_string(), data.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx
                .send(FileSystem::write_at(client.as_ref(), handle, ws, path, offset, data).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn fs_truncate(
        &self,
        workspace: &str,
        path: &str,
        size: u64,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path) = (workspace.to_string(), path.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(FileSystem::truncate(client.as_ref(), handle, ws, path, size).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn fs_symlink(
        &self,
        workspace: &str,
        target: &str,
        link_path: &str,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, target, link_path) = (
            workspace.to_string(),
            target.to_string(),
            link_path.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(FileSystem::symlink(client.as_ref(), handle, ws, target, link_path).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vector_create(
        &self,
        workspace: &str,
        name: &str,
        dim: u64,
        metric: i32,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Vector::create(client.as_ref(), handle, ws, name, dim, metric).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vector_upsert(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
        vector: &[u8],
        metadata: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, id, vector, metadata) = (
            workspace.to_string(),
            name.to_string(),
            id.to_string(),
            vector.to_vec(),
            metadata.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Vector::upsert(client.as_ref(), handle, ws, name, id, vector, metadata).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vector_upsert_source(
        &self,
        workspace: &str,
        name: &str,
        args: uldren_loom_mcp::RemoteVectorUpsertSource<'_>,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, id, vector, metadata, source_text, model_id, weights_digest) = (
            workspace.to_string(),
            name.to_string(),
            args.id.to_string(),
            args.vector.to_vec(),
            args.metadata.to_vec(),
            args.source_text.to_vec(),
            args.model_id.map(str::to_string),
            args.weights_digest.map(str::to_string),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Vector::upsert_source(
                    client.as_ref(),
                    handle,
                    ws,
                    name,
                    id,
                    vector,
                    metadata,
                    source_text,
                    model_id,
                    weights_digest,
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vector_text_upsert(
        &self,
        request: &[u8],
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let request = request.to_vec();
        self.block_generated(move |client, handle| async move {
            Vector::vector_text_upsert(client.as_ref(), handle, request).await
        })
    }

    fn vector_workspace_configure_json(
        &self,
        workspace: &str,
        request_json: &str,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let (workspace, request_json) = (workspace.to_string(), request_json.to_string());
        self.block_generated(move |client, handle| async move {
            Vector::vector_workspace_configure_json(
                client.as_ref(),
                handle,
                workspace,
                request_json,
            )
            .await
        })
    }

    fn vector_create_metadata_index(
        &self,
        workspace: &str,
        name: &str,
        key: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, key) = (workspace.to_string(), name.to_string(), key.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx
                .send(Vector::create_metadata_index(client.as_ref(), handle, ws, name, key).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vector_drop_metadata_index(
        &self,
        workspace: &str,
        name: &str,
        key: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, key) = (workspace.to_string(), name.to_string(), key.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Vector::drop_metadata_index(client.as_ref(), handle, ws, name, key).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vector_delete(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, id) = (workspace.to_string(), name.to_string(), id.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Vector::delete(client.as_ref(), handle, ws, name, id).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vector_get(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, id) = (workspace.to_string(), name.to_string(), id.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Vector::get(client.as_ref(), handle, ws, name, id).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vector_source_text(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, id) = (workspace.to_string(), name.to_string(), id.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Vector::source_text(client.as_ref(), handle, ws, name, id).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vector_embedding_model(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Vector::embedding_model(client.as_ref(), handle, ws, name).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vector_ids(
        &self,
        workspace: &str,
        name: &str,
        prefix: Option<&str>,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, prefix) = (
            workspace.to_string(),
            name.to_string(),
            prefix.map(str::to_string),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Vector::ids(client.as_ref(), handle, ws, name, prefix).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vector_metadata_index_keys(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Vector::metadata_index_keys(client.as_ref(), handle, ws, name).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vector_search(
        &self,
        workspace: &str,
        name: &str,
        query: &[u8],
        k: u64,
        filter: &[u8],
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, query, filter) = (
            workspace.to_string(),
            name.to_string(),
            query.to_vec(),
            filter.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Vector::search(client.as_ref(), handle, ws, name, query, k, filter).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vector_search_policy(
        &self,
        workspace: &str,
        name: &str,
        args: uldren_loom_mcp::RemoteVectorSearchPolicy<'_>,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, query, filter) = (
            workspace.to_string(),
            name.to_string(),
            args.query.to_vec(),
            args.filter.to_vec(),
        );
        let (k, policy, threshold, ef, pq_m, pq_k, pq_iters) = (
            args.k,
            args.policy,
            args.threshold,
            args.ef,
            args.pq_m,
            args.pq_k,
            args.pq_iters,
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Vector::search_policy(
                    client.as_ref(),
                    handle,
                    ws,
                    name,
                    query,
                    k,
                    filter,
                    policy,
                    threshold,
                    ef,
                    pq_m,
                    pq_k,
                    pq_iters,
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn metrics_put_descriptor(
        &self,
        workspace: &str,
        descriptor: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let workspace = workspace.to_string();
        let descriptor = descriptor.to_vec();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Metrics::put_descriptor(client.as_ref(), handle, workspace, descriptor).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn metrics_get_descriptor(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (workspace, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Metrics::get_descriptor(client.as_ref(), handle, workspace, name).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn metrics_put_observation(
        &self,
        workspace: &str,
        descriptor_name: &str,
        observation: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (workspace, descriptor_name, observation) = (
            workspace.to_string(),
            descriptor_name.to_string(),
            observation.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Metrics::put_observation(
                    client.as_ref(),
                    handle,
                    workspace,
                    descriptor_name,
                    observation,
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    #[allow(clippy::too_many_arguments)]
    fn metrics_query(
        &self,
        workspace: &str,
        descriptor_name: &str,
        from_timestamp_ms: u64,
        to_timestamp_ms: u64,
        max_series: u32,
        max_groups: u32,
        max_samples: u32,
        max_output_bytes: u64,
        now_timestamp_ms: u64,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (workspace, descriptor_name) = (workspace.to_string(), descriptor_name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Metrics::query(
                    client.as_ref(),
                    handle,
                    workspace,
                    descriptor_name,
                    from_timestamp_ms,
                    to_timestamp_ms,
                    max_series,
                    max_groups,
                    max_samples,
                    max_output_bytes,
                    now_timestamp_ms,
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn logs_put_record(
        &self,
        workspace: &str,
        record: &[u8],
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let workspace = workspace.to_string();
        let record = record.to_vec();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Logs::put_record(client.as_ref(), handle, workspace, record).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn logs_get_record(
        &self,
        workspace: &str,
        record_id: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (workspace, record_id) = (workspace.to_string(), record_id.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Logs::get_record(client.as_ref(), handle, workspace, record_id).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn logs_query(
        &self,
        workspace: &str,
        from_time_unix_nano: u64,
        to_time_unix_nano: u64,
        max_records: u32,
        max_output_bytes: u64,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let workspace = workspace.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Logs::query(
                    client.as_ref(),
                    handle,
                    workspace,
                    from_time_unix_nano,
                    to_time_unix_nano,
                    max_records,
                    max_output_bytes,
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn traces_put_span(
        &self,
        workspace: &str,
        span: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let workspace = workspace.to_string();
        let span = span.to_vec();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Traces::put_span(client.as_ref(), handle, workspace, span).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn traces_get_span(
        &self,
        workspace: &str,
        trace_id: &str,
        span_id: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (workspace, trace_id, span_id) = (
            workspace.to_string(),
            trace_id.to_string(),
            span_id.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Traces::get_span(client.as_ref(), handle, workspace, trace_id, span_id).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn traces_trace_spans(
        &self,
        workspace: &str,
        trace_id: &str,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (workspace, trace_id) = (workspace.to_string(), trace_id.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Traces::trace_spans(
                    client.as_ref(),
                    handle,
                    workspace,
                    trace_id,
                    max_spans,
                    max_output_bytes,
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn traces_query(
        &self,
        workspace: &str,
        from_start_time_ns: u64,
        to_start_time_ns: u64,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let workspace = workspace.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Traces::query(
                    client.as_ref(),
                    handle,
                    workspace,
                    from_start_time_ns,
                    to_start_time_ns,
                    max_spans,
                    max_output_bytes,
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn document_get_binary(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> std::result::Result<Option<loom_core::document::DocumentBinary>, loom_types::LoomError>
    {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, collection, id) = (
            workspace.to_string(),
            collection.to_string(),
            id.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let out = Document::get_binary(client.as_ref(), handle, ws, collection, id)
                .await
                .and_then(|value| {
                    value
                        .map(|bytes| {
                            let (bytes, digest, entity_tag) =
                                loom_wire::document::binary_result_from_cbor(&bytes)?;
                            Ok(loom_core::document::DocumentBinary {
                                bytes,
                                digest: Digest::parse(&digest)?,
                                entity_tag,
                            })
                        })
                        .transpose()
                });
            let _ = tx.send(out);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_log(
        &self,
        workspace: &str,
        branch: &str,
    ) -> std::result::Result<Vec<String>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, branch) = (workspace.to_string(), branch.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                VersionControl::log(client.as_ref(), handle, ws, branch)
                    .await
                    .map(|commits| commits.into_iter().map(|d| d.0).collect()),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_head_branch(
        &self,
        workspace: &str,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let ws = workspace.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(VersionControl::head_branch(client.as_ref(), handle, ws).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_status(&self, workspace: &str) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let ws = workspace.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(VersionControl::status(client.as_ref(), handle, ws).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_merge_in_progress(
        &self,
        workspace: &str,
    ) -> std::result::Result<bool, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let ws = workspace.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(VersionControl::merge_in_progress(client.as_ref(), handle, ws).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_merge_conflicts(
        &self,
        workspace: &str,
    ) -> std::result::Result<Vec<String>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let ws = workspace.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(VersionControl::merge_conflicts(client.as_ref(), handle, ws).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_tag_list(
        &self,
        workspace: &str,
    ) -> std::result::Result<Vec<String>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let ws = workspace.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(VersionControl::tag_list(client.as_ref(), handle, ws).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_tag_target(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<Option<String>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                VersionControl::tag_target(client.as_ref(), handle, ws, name)
                    .await
                    .map(|target| target.map(|d| d.0)),
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_diff(
        &self,
        workspace: &str,
        from_commit: &str,
        to_commit: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, from, to) = (
            workspace.to_string(),
            from_commit.to_string(),
            to_commit.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(VersionControl::diff(client.as_ref(), handle, ws, from, to).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_blame(
        &self,
        workspace: &str,
        branch: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, branch) = (workspace.to_string(), branch.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(VersionControl::blame(client.as_ref(), handle, ws, branch).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_branch(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(VersionControl::branch(client.as_ref(), handle, ws, name).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_checkout(
        &self,
        workspace: &str,
        branch: &str,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, branch) = (workspace.to_string(), branch.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(VersionControl::checkout(client.as_ref(), handle, ws, branch).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_stage(
        &self,
        workspace: &str,
        path: &str,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path) = (workspace.to_string(), path.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(VersionControl::stage(client.as_ref(), handle, ws, path).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_stage_all(&self, workspace: &str) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let ws = workspace.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(VersionControl::stage_all(client.as_ref(), handle, ws).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_unstage(
        &self,
        workspace: &str,
        path: &str,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path) = (workspace.to_string(), path.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(VersionControl::unstage(client.as_ref(), handle, ws, path).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_tag_delete(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(VersionControl::tag_delete(client.as_ref(), handle, ws, name).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_tag_rename(
        &self,
        workspace: &str,
        old_name: &str,
        new_name: &str,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, old_name, new_name) = (
            workspace.to_string(),
            old_name.to_string(),
            new_name.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                VersionControl::tag_rename(client.as_ref(), handle, ws, old_name, new_name).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_restore_file(
        &self,
        workspace: &str,
        rev: &str,
        path: &str,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, rev, path) = (workspace.to_string(), rev.to_string(), path.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(VersionControl::restore_file(client.as_ref(), handle, ws, rev, path).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_restore_path(
        &self,
        workspace: &str,
        rev: &str,
        prefix: &str,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, rev, prefix) = (workspace.to_string(), rev.to_string(), prefix.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx
                .send(VersionControl::restore_path(client.as_ref(), handle, ws, rev, prefix).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_merge_resolve(
        &self,
        workspace: &str,
        path: &str,
        resolution: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, path, resolution) = (workspace.to_string(), path.to_string(), resolution.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                VersionControl::merge_resolve(client.as_ref(), handle, ws, path, resolution).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_merge_abort(&self, workspace: &str) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let ws = workspace.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(VersionControl::merge_abort(client.as_ref(), handle, ws).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn graph_get_node(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, id) = (workspace.to_string(), name.to_string(), id.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Graph::get_node(client.as_ref(), handle, ws, name, id).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn graph_get_edge(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, id) = (workspace.to_string(), name.to_string(), id.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Graph::get_edge(client.as_ref(), handle, ws, name, id).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn graph_neighbors(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, id) = (workspace.to_string(), name.to_string(), id.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Graph::neighbors(client.as_ref(), handle, ws, name, id).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn graph_out_edges(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, id) = (workspace.to_string(), name.to_string(), id.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Graph::out_edges(client.as_ref(), handle, ws, name, id).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn graph_in_edges(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, id) = (workspace.to_string(), name.to_string(), id.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Graph::in_edges(client.as_ref(), handle, ws, name, id).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn graph_reachable(
        &self,
        workspace: &str,
        name: &str,
        start: &str,
        max_depth: i64,
        via_label: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, start, via_label) = (
            workspace.to_string(),
            name.to_string(),
            start.to_string(),
            via_label.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Graph::reachable(
                    client.as_ref(),
                    handle,
                    ws,
                    name,
                    start,
                    max_depth,
                    via_label,
                )
                .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn graph_shortest_path(
        &self,
        workspace: &str,
        name: &str,
        from: &str,
        to: &str,
        via_label: &str,
    ) -> std::result::Result<Option<Vec<u8>>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, from, to, via_label) = (
            workspace.to_string(),
            name.to_string(),
            from.to_string(),
            to.to_string(),
            via_label.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Graph::shortest_path(client.as_ref(), handle, ws, name, from, to, via_label).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn graph_query(
        &self,
        workspace: &str,
        name: &str,
        query: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, query) = (workspace.to_string(), name.to_string(), query.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Graph::query(client.as_ref(), handle, ws, name, query).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn graph_explain_query(
        &self,
        workspace: &str,
        name: &str,
        query: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, query) = (workspace.to_string(), name.to_string(), query.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Graph::explain_query(client.as_ref(), handle, ws, name, query).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn graph_upsert_node(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
        props: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, id, props) = (
            workspace.to_string(),
            name.to_string(),
            id.to_string(),
            props.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Graph::upsert_node(client.as_ref(), handle, ws, name, id, props).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn graph_remove_node(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
        cascade: bool,
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, id) = (workspace.to_string(), name.to_string(), id.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Graph::remove_node(client.as_ref(), handle, ws, name, id, cascade).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn document_list_binary(
        &self,
        workspace: &str,
        collection: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, collection) = (workspace.to_string(), collection.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Document::list_binary(client.as_ref(), handle, ws, collection).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn document_query_json(
        &self,
        workspace: &str,
        collection: &str,
        query_json: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, collection, query) = (
            workspace.to_string(),
            collection.to_string(),
            query_json.as_bytes().to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Document::query_json(client.as_ref(), handle, ws, collection, query).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn document_find_json(
        &self,
        workspace: &str,
        collection: &str,
        index: &str,
        value_json: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, collection, index, value) = (
            workspace.to_string(),
            collection.to_string(),
            index.to_string(),
            value_json.as_bytes().to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Document::find_json(client.as_ref(), handle, ws, collection, index, value).await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn store_digest_algo(&self) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Store::digest_algo(client.as_ref()).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn sql_read_table(
        &self,
        workspace: &str,
        table: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, table) = (workspace.to_string(), table.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Sql::sql_read_table(client.as_ref(), handle, ws, table).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn sql_read_table_at(
        &self,
        workspace: &str,
        table: &str,
        commit: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, table, commit) = (workspace.to_string(), table.to_string(), commit.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Sql::sql_read_table_at(client.as_ref(), handle, ws, table, commit).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn sql_index_scan(
        &self,
        workspace: &str,
        table: &str,
        index: &str,
        prefix: &[u8],
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, table, index, prefix) = (
            workspace.to_string(),
            table.to_string(),
            index.to_string(),
            prefix.to_vec(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx
                .send(Sql::sql_index_scan(client.as_ref(), handle, ws, table, index, prefix).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn sql_index_scan_at(
        &self,
        workspace: &str,
        table: &str,
        index: &str,
        prefix: &[u8],
        commit: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, table, index, prefix, commit) = (
            workspace.to_string(),
            table.to_string(),
            index.to_string(),
            prefix.to_vec(),
            commit.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                Sql::sql_index_scan_at(client.as_ref(), handle, ws, table, index, prefix, commit)
                    .await,
            );
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn sql_blame(
        &self,
        workspace: &str,
        branch: &str,
        table: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, branch, table) = (workspace.to_string(), branch.to_string(), table.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Sql::sql_blame(client.as_ref(), handle, ws, branch, table).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn sql_diff(
        &self,
        workspace: &str,
        table: &str,
        from_commit: &str,
        to_commit: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, table, from, to) = (
            workspace.to_string(),
            table.to_string(),
            from_commit.to_string(),
            to_commit.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Sql::sql_diff(client.as_ref(), handle, ws, table, from, to).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn sql_table_diff(
        &self,
        workspace: &str,
        table: &str,
        from_commit: &str,
        to_commit: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, table, from, to) = (
            workspace.to_string(),
            table.to_string(),
            from_commit.to_string(),
            to_commit.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ =
                tx.send(Sql::sql_table_diff(client.as_ref(), handle, ws, table, from, to).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn sql_list_databases(
        &self,
        workspace: &str,
    ) -> std::result::Result<Vec<String>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let ws = workspace.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Sql::sql_list_databases(client.as_ref(), handle, ws).await);
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        remote_text_list(&wire)
    }

    fn list_collections(
        &self,
        workspace: &str,
        facet: loom_core::FacetKind,
    ) -> std::result::Result<Vec<String>, loom_types::LoomError> {
        use loom_core::FacetKind;
        let client = self.client.clone();
        let handle = self.handle.clone();
        let ws = workspace.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let wire = match facet {
                FacetKind::Kv => Kv::list_collections(client.as_ref(), handle, ws).await,
                FacetKind::Document => {
                    Document::list_collections(client.as_ref(), handle, ws).await
                }
                FacetKind::TimeSeries => {
                    TimeSeries::list_collections(client.as_ref(), handle, ws).await
                }
                FacetKind::Ledger => Ledger::list_collections(client.as_ref(), handle, ws).await,
                FacetKind::Queue => Queue::list_streams(client.as_ref(), handle, ws).await,
                other => Err(loom_types::LoomError::new(
                    loom_types::Code::InvalidArgument,
                    format!("list_collections is not wired over remote for facet {other:?}"),
                )),
            };
            let _ = tx.send(wire);
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        remote_text_list(&wire)
    }

    fn dataframe_create(
        &self,
        workspace: &str,
        name: &str,
        plan: &[u8],
    ) -> std::result::Result<(), loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, plan) = (workspace.to_string(), name.to_string(), plan.to_vec());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Dataframe::create(client.as_ref(), handle, ws, name, plan).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn dataframe_collect(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Dataframe::collect(client.as_ref(), handle, ws, name).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn dataframe_preview(
        &self,
        workspace: &str,
        name: &str,
        rows: u64,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Dataframe::preview(client.as_ref(), handle, ws, name, rows).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn dataframe_materialize(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<Option<String>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Dataframe::materialize(client.as_ref(), handle, ws, name).await);
        });
        let digest = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        Ok(digest.map(|d| d.0))
    }

    fn dataframe_plan_digest(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Dataframe::plan_digest(client.as_ref(), handle, ws, name).await);
        });
        let digest = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        Ok(digest.0)
    }

    fn dataframe_source_digests(
        &self,
        workspace: &str,
        name: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name) = (workspace.to_string(), name.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Dataframe::source_digests(client.as_ref(), handle, ws, name).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn watch_subscribe(
        &self,
        workspace: &str,
        branch: &str,
        from: Option<&str>,
        facet: Option<&str>,
        path_prefix: Option<&str>,
        change_kinds: &[String],
    ) -> std::result::Result<String, loom_types::LoomError> {
        let ns = self.resolve_workspace_id(workspace)?;
        let mut selector = loom_core::WatchSelector::new(ns, branch)?;
        if let Some(facet) = facet {
            selector = selector.with_facet(loom_core::FacetKind::parse(facet)?);
        }
        if let Some(path_prefix) = path_prefix {
            selector = selector.with_path_prefix(path_prefix);
        }
        for kind in change_kinds {
            selector = selector.with_change_kind(parse_watch_change_kind_cli(kind)?);
        }
        let selector_bytes = loom_wire::watch::watch_selector_to_cbor(&selector)?;
        let from = from.map(|from| loom_remote_protocol::api_types::Digest(from.to_string()));
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Watch::subscribe(client.as_ref(), handle, selector_bytes, from).await);
        });
        let cursor = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        String::from_utf8(cursor)
            .map_err(|_| loom_types::LoomError::corrupt("watch cursor is not valid utf-8"))
    }

    fn watch_poll(
        &self,
        workspace: &str,
        cursor: &str,
        max: u32,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        // Reproduce the local cursor/workspace guard: a cursor binds to its own workspace, and the local
        // path rejects a mismatched (workspace, cursor) pair with `CursorInvalid` before polling.
        let ns = self.resolve_workspace_id(workspace)?;
        let decoded = loom_core::WatchCursor::decode(cursor)?;
        if decoded.workspace != ns {
            return Err(loom_types::LoomError::new(
                loom_types::Code::CursorInvalid,
                "watch cursor workspace mismatch",
            ));
        }
        let client = self.client.clone();
        let handle = self.handle.clone();
        let cursor = cursor.to_string();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(Watch::poll(client.as_ref(), handle, cursor, max).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn sql_exec(
        &self,
        workspace: &str,
        db: &str,
        sql: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let (ws, db, sql) = (workspace.to_string(), db.to_string(), sql.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            // Open a per-request SqlSession, run the statement, and always close/free the session on both
            // success and error. `sql_open` takes no session handle (it mints one); `sql_exec` returns the
            // canonical `exec_cbor` payload byte-for-byte.
            let out = async {
                let session = Sql::sql_open(client.as_ref(), ws, db).await?;
                let result = Sql::sql_exec(client.as_ref(), session.clone(), sql).await;
                let _ = Sql::sql_close(client.as_ref(), session).await;
                result
            }
            .await;
            let _ = tx.send(out);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn sql_exec_result(
        &self,
        workspace: &str,
        db: &str,
        sql: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let (workspace, db, sql) = (workspace.to_string(), db.to_string(), sql.to_string());
        self.block_generated(move |client, handle| async move {
            Sql::sql_exec_result(client.as_ref(), handle, workspace, db, sql).await
        })
    }

    fn ts_latest(
        &self,
        workspace: &str,
        collection: &str,
    ) -> std::result::Result<Option<(i64, Vec<u8>)>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, collection) = (workspace.to_string(), collection.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(TimeSeries::latest(client.as_ref(), handle, ws, collection).await);
        });
        // The wire payload is the canonical `[ts, value]` pair (or `None` when the series is empty).
        match rx.recv().map_err(|_| remote_backend_channel_closed())?? {
            Some(bytes) => loom_core::timeseries::latest_point_from_cbor(&bytes).map(Some),
            None => Ok(None),
        }
    }

    fn sql_query(
        &self,
        workspace: &str,
        db: &str,
        sql: &str,
    ) -> std::result::Result<Vec<u8>, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, db, sql) = (workspace.to_string(), db.to_string(), sql.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            // Read-only full-result query: the server runs `exec_cbor` on an eager read overlay (no
            // persist) and rejects a mutating statement, so the returned bytes are byte-identical to a
            // local `read_sql_query` and the read-only contract holds.
            let _ = tx.send(Sql::sql_query_result(client.as_ref(), handle, ws, db, sql).await);
        });
        rx.recv().map_err(|_| remote_backend_channel_closed())?
    }

    fn vcs_commit(
        &self,
        workspace: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, author, message) = (
            workspace.to_string(),
            author.to_string(),
            message.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                VersionControl::commit(client.as_ref(), handle, ws, author, message, timestamp_ms)
                    .await,
            );
        });
        Ok(rx.recv().map_err(|_| remote_backend_channel_closed())??.0)
    }

    fn vcs_commit_staged(
        &self,
        workspace: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, author, message) = (
            workspace.to_string(),
            author.to_string(),
            message.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                VersionControl::commit_staged(
                    client.as_ref(),
                    handle,
                    ws,
                    author,
                    message,
                    timestamp_ms,
                )
                .await,
            );
        });
        Ok(rx.recv().map_err(|_| remote_backend_channel_closed())??.0)
    }

    fn vcs_tag_create(
        &self,
        workspace: &str,
        name: &str,
        rev: &str,
        tagger: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, name, rev, tagger, message) = (
            workspace.to_string(),
            name.to_string(),
            rev.to_string(),
            tagger.to_string(),
            message.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                VersionControl::tag_create(
                    client.as_ref(),
                    handle,
                    ws,
                    name,
                    rev,
                    tagger,
                    message,
                    timestamp_ms,
                )
                .await,
            );
        });
        Ok(rx.recv().map_err(|_| remote_backend_channel_closed())??.0)
    }

    fn vcs_merge_continue(
        &self,
        workspace: &str,
        author: &str,
        timestamp_ms: u64,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, author) = (workspace.to_string(), author.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                VersionControl::merge_continue(client.as_ref(), handle, ws, author, timestamp_ms)
                    .await,
            );
        });
        Ok(rx.recv().map_err(|_| remote_backend_channel_closed())??.0)
    }

    fn vcs_squash(
        &self,
        workspace: &str,
        onto: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> std::result::Result<String, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, onto, author, message) = (
            workspace.to_string(),
            onto.to_string(),
            author.to_string(),
            message.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                VersionControl::squash(
                    client.as_ref(),
                    handle,
                    ws,
                    onto,
                    author,
                    message,
                    timestamp_ms,
                )
                .await,
            );
        });
        Ok(rx.recv().map_err(|_| remote_backend_channel_closed())??.0)
    }

    fn vcs_merge(
        &self,
        workspace: &str,
        from_branch: &str,
        author: &str,
        cell_level: bool,
        timestamp_ms: u64,
    ) -> std::result::Result<loom_core::MergeOutcome, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, from_branch, author) = (
            workspace.to_string(),
            from_branch.to_string(),
            author.to_string(),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                VersionControl::merge(
                    client.as_ref(),
                    handle,
                    ws,
                    from_branch,
                    author,
                    cell_level,
                    timestamp_ms,
                )
                .await,
            );
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        loom_wire::vcs::merge_result_from_cbor(&wire)
    }

    fn vcs_cherry_pick(
        &self,
        workspace: &str,
        commits: &[String],
        dry_run: bool,
        timestamp_ms: u64,
    ) -> std::result::Result<loom_core::ReplayOutcome, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let ws = workspace.to_string();
        let commits: Vec<loom_remote_protocol::api_types::Digest> = commits
            .iter()
            .map(|c| loom_remote_protocol::api_types::Digest(c.clone()))
            .collect();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                VersionControl::cherry_pick(
                    client.as_ref(),
                    handle,
                    ws,
                    commits,
                    dry_run,
                    timestamp_ms,
                )
                .await,
            );
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        loom_wire::vcs::replay_outcome_from_cbor(&wire)
    }

    fn vcs_revert(
        &self,
        workspace: &str,
        commits: &[String],
        author: &str,
        dry_run: bool,
        timestamp_ms: u64,
    ) -> std::result::Result<loom_core::ReplayOutcome, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, author) = (workspace.to_string(), author.to_string());
        let commits: Vec<loom_remote_protocol::api_types::Digest> = commits
            .iter()
            .map(|c| loom_remote_protocol::api_types::Digest(c.clone()))
            .collect();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                VersionControl::revert(
                    client.as_ref(),
                    handle,
                    ws,
                    commits,
                    author,
                    dry_run,
                    timestamp_ms,
                )
                .await,
            );
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        loom_wire::vcs::replay_outcome_from_cbor(&wire)
    }

    fn vcs_rebase(
        &self,
        workspace: &str,
        onto: &str,
        dry_run: bool,
        timestamp_ms: u64,
    ) -> std::result::Result<loom_core::ReplayOutcome, loom_types::LoomError> {
        let client = self.client.clone();
        let handle = self.handle.clone();
        let (ws, onto) = (workspace.to_string(), onto.to_string());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.runtime.handle().spawn(async move {
            let _ = tx.send(
                VersionControl::rebase(client.as_ref(), handle, ws, onto, dry_run, timestamp_ms)
                    .await,
            );
        });
        let wire = rx.recv().map_err(|_| remote_backend_channel_closed())??;
        loom_wire::vcs::replay_outcome_from_cbor(&wire)
    }
}

/// Decode a canonical `Array(Bytes)` remote response into the per-record byte blobs the MCP host's
/// typed decoders expect (each blob is one facet record's canonical CBOR).
#[cfg(feature = "remote-client")]
fn remote_bytes_list(wire: &[u8]) -> std::result::Result<Vec<Vec<u8>>, loom_types::LoomError> {
    match loom_codec::decode(wire)
        .map_err(|e| loom_types::LoomError::corrupt(format!("cbor: {e}")))?
    {
        loom_codec::Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                loom_codec::Value::Bytes(bytes) => Ok(bytes),
                _ => Err(loom_types::LoomError::corrupt(
                    "expected a CBOR byte-string list from the remote endpoint",
                )),
            })
            .collect(),
        _ => Err(loom_types::LoomError::corrupt(
            "expected a CBOR array from the remote endpoint",
        )),
    }
}

/// Decode a canonical `Array(Text)` remote response into a string list (collection/book/mailbox ids
/// and mail flag sets).
#[cfg(feature = "remote-client")]
fn remote_text_list(wire: &[u8]) -> std::result::Result<Vec<String>, loom_types::LoomError> {
    match loom_codec::decode(wire)
        .map_err(|e| loom_types::LoomError::corrupt(format!("cbor: {e}")))?
    {
        loom_codec::Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                loom_codec::Value::Text(text) => Ok(text),
                _ => Err(loom_types::LoomError::corrupt(
                    "expected a CBOR text list from the remote endpoint",
                )),
            })
            .collect(),
        _ => Err(loom_types::LoomError::corrupt(
            "expected a CBOR array from the remote endpoint",
        )),
    }
}

/// Encode a flag set as the canonical `Array(Text)` the server decodes with `string_list_from_cbor`.
#[cfg(feature = "remote-client")]
fn encode_text_list(items: &[String]) -> Vec<u8> {
    let array = loom_codec::Value::Array(
        items
            .iter()
            .map(|s| loom_codec::Value::Text(s.clone()))
            .collect(),
    );
    loom_codec::encode(&array).expect("encoding a CBOR text array is infallible")
}

#[cfg(test)]
mod selector_tests {
    use super::*;
    use loom_remote_protocol::codec::ToValue;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[cfg(feature = "serve")]
    const DAEMON_GENERATED_SESSION_OPEN_MAGIC: &[u8] = b"loom-daemon-generated-session-open-v1\0";
    #[cfg(feature = "serve")]
    const DAEMON_GENERATED_CALL_MAGIC: &[u8] = b"loom-daemon-generated-call-v1\0";
    #[cfg(feature = "serve")]
    const DAEMON_GENERATED_SESSION_RESPONSE_MAGIC: &[u8] =
        b"loom-daemon-generated-session-response-v1\0";
    #[cfg(feature = "serve")]
    const DAEMON_GENERATED_RESPONSE_MAGIC: &[u8] = b"loom-daemon-generated-response-v1\0";

    fn temp_store(tag: &str) -> String {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "loomcli-execution-selector-{tag}-{}-{seq}.loom",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let fs = FileStore::create_with_profile(&path, Algo::Blake3).expect("create store");
        drop(fs);
        path.to_string_lossy().into_owned()
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "loomcli-execution-selector-{tag}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    fn cas_put_operation(workspace: &str, content: Vec<u8>) -> CliGeneratedOperation {
        CliGeneratedOperation::new(
            "Cas",
            "put",
            vec![
                loom_codec::Value::Text(workspace.to_string()),
                loom_codec::Value::Bytes(content),
            ],
        )
        .expect("cas put operation")
    }

    fn cas_get_operation(workspace: &str, digest: WireDigest) -> CliGeneratedOperation {
        CliGeneratedOperation::new(
            "Cas",
            "get",
            vec![
                loom_codec::Value::Text(workspace.to_string()),
                digest.to_value(),
            ],
        )
        .expect("cas get operation")
    }

    fn workspace_create_operation(name: &str) -> CliGeneratedOperation {
        CliGeneratedOperation::new(
            "Workspaces",
            "workspace_create",
            vec![Some(name.to_string()).to_value(), loom_codec::Value::Null],
        )
        .expect("workspace create operation")
    }

    fn workspace_id_from_value(value: loom_codec::Value) -> String {
        match value {
            loom_codec::Value::Bytes(bytes) => {
                let id = WorkspaceId::from_bytes(bytes.try_into().expect("workspace id bytes"));
                id.to_string()
            }
            other => panic!("unexpected workspace output {other:?}"),
        }
    }

    fn tickets_project_create_operation(workspace: &str) -> CliGeneratedOperation {
        CliGeneratedOperation::new(
            "Tickets",
            "tickets_project_create_json",
            vec![
                loom_codec::Value::Text(workspace.to_string()),
                loom_codec::Value::Text("tickets-workspace".to_string()),
                loom_codec::Value::Text("selector-project".to_string()),
                loom_codec::Value::Text("SEL".to_string()),
                loom_codec::Value::Text("Selector Project".to_string()),
                loom_codec::Value::Null,
            ],
        )
        .expect("tickets project create operation")
    }

    #[cfg(feature = "serve")]
    fn generated_binary_body<'a>(request: &'a [u8], magic: &[u8]) -> Option<&'a [u8]> {
        let rest = request.strip_prefix(magic)?;
        let len_bytes: [u8; 4] = rest.get(..4)?.try_into().ok()?;
        let len = u32::from_be_bytes(len_bytes) as usize;
        rest.get(4..4 + len)
    }

    #[cfg(feature = "serve")]
    fn generated_binary_response(magic: &[u8], bodies: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(magic);
        out.extend_from_slice(&(bodies.len() as u32).to_be_bytes());
        for body in bodies {
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            out.extend_from_slice(body);
        }
        out
    }

    #[cfg(feature = "serve")]
    fn success_store_open_response(request_body: &[u8], session_id: &[u8]) -> Vec<u8> {
        let request =
            loom_remote_protocol::envelope::Request::decode(request_body).expect("decode request");
        assert_eq!(request.session_id.as_deref(), Some(session_id));
        assert_eq!(request.interface, "Store");
        assert_eq!(request.method, "open");
        let handle = LoomSession(loom_remote_protocol::api_types::HandleId {
            kind: "session".to_string(),
            id: vec![9, 8, 7],
            generation: 1,
            owner_session: session_id.to_vec(),
        });
        let response = loom_remote_protocol::envelope::Response::ok(
            request.request_id,
            request.session_id,
            handle.to_value(),
        )
        .encode()
        .expect("encode response");
        generated_binary_response(DAEMON_GENERATED_RESPONSE_MAGIC, &[response])
    }

    fn running_daemon_response(store_path: &str, store_id: &str) -> Vec<u8> {
        format!(
            "running\tprotocol=1\ttransport=tcp\tfake-pid\t{store_path}\tidentity={store_id}\tstartup_mode=managed\tstartup_initiator=cli.mcp.local\tsessions=0\tpins=0\n"
        )
        .into_bytes()
    }

    #[cfg(feature = "serve")]
    fn success_generated_response(
        request_body: &[u8],
        session_id: &[u8],
        interface: &str,
        method: &str,
        value: loom_codec::Value,
    ) -> Vec<u8> {
        let request =
            loom_remote_protocol::envelope::Request::decode(request_body).expect("decode request");
        assert_eq!(request.session_id.as_deref(), Some(session_id));
        assert_eq!(request.interface, interface);
        assert_eq!(request.method, method);
        let response = loom_remote_protocol::envelope::Response::ok(
            request.request_id,
            request.session_id,
            value,
        )
        .encode()
        .expect("encode response");
        generated_binary_response(DAEMON_GENERATED_RESPONSE_MAGIC, &[response])
    }

    #[cfg(feature = "serve")]
    fn error_generated_response(
        request_body: &[u8],
        session_id: &[u8],
        interface: &str,
        method: &str,
        error: loom_core::error::LoomError,
    ) -> Vec<u8> {
        let request =
            loom_remote_protocol::envelope::Request::decode(request_body).expect("decode request");
        assert_eq!(request.session_id.as_deref(), Some(session_id));
        assert_eq!(request.interface, interface);
        assert_eq!(request.method, method);
        let response = loom_remote_protocol::envelope::Response::err(
            request.request_id,
            request.session_id,
            loom_remote_protocol::RemoteError::from_loom_error(&error),
        )
        .encode()
        .expect("encode response");
        generated_binary_response(DAEMON_GENERATED_RESPONSE_MAGIC, &[response])
    }

    #[test]
    fn cli_execution_context_selects_direct_local_without_daemon() {
        let store = temp_store("direct-local");

        let context = open_cli_execution_context(&store).expect("open execution context");

        assert_eq!(context.target(), CliExecutionTarget::DirectLocal);
        let generated = context
            .into_generated_client()
            .expect("direct local generated client");
        assert_eq!(generated.target(), CliExecutionTarget::DirectLocal);
        match generated {
            CliGeneratedClient::DirectLocal { client, handle } => {
                let _ = client;
                assert_eq!(handle.0.kind, "session");
            }
            #[cfg(feature = "serve")]
            CliGeneratedClient::DaemonLocal(_) => panic!("expected direct local"),
            #[cfg(feature = "remote-client")]
            CliGeneratedClient::Remote(_) => panic!("expected direct local"),
        }
    }

    #[test]
    fn cli_generated_client_executes_direct_local_operations_without_target_branching() {
        let store = temp_store("direct-operation");
        let generated = open_cli_execution_context(&store)
            .expect("open execution context")
            .into_generated_client()
            .expect("direct local generated client");
        let workspace = "blobs".to_string();
        let content = b"direct generated operation".to_vec();

        let digest = WireDigest(
            match generated
                .execute_unary(&cas_put_operation(&workspace, content.clone()))
                .expect("cas put")
            {
                loom_codec::Value::Text(digest) => digest,
                other => panic!("unexpected output {other:?}"),
            },
        );
        let read = generated
            .execute_unary(&cas_get_operation(&workspace, digest))
            .expect("cas get");

        assert_eq!(read, loom_codec::Value::Bytes(content));
        let workgraph = workspace_id_from_value(
            generated
                .execute_unary(&workspace_create_operation("workgraph"))
                .expect("workspace create"),
        );
        let project = generated
            .execute_unary(&tickets_project_create_operation(&workgraph))
            .expect("tickets project create");
        match project {
            loom_codec::Value::Text(json) => assert!(json.contains("selector-project"), "{json}"),
            other => panic!("unexpected output {other:?}"),
        }
    }

    fn workspace_record(id: [u8; 16], name: &str) -> Vec<u8> {
        loom_wire::workspace::workspace_info_to_cbor(&loom_core::WorkspaceInfo {
            id: loom_core::WorkspaceId::from_bytes(id),
            name: name.to_string(),
            facets: Vec::new(),
            head: None,
        })
        .expect("encode workspace info")
    }

    #[test]
    fn shared_workspace_selector_matches_uuid() {
        let alpha = loom_core::WorkspaceId::from_bytes([1; 16]);
        let beta = loom_core::WorkspaceId::from_bytes([2; 16]);
        let infos = cli_workspace_infos_from_remote_records(&[
            workspace_record(*alpha.as_bytes(), "alpha"),
            workspace_record(*beta.as_bytes(), "beta"),
        ])
        .expect("decode workspace records");

        assert_eq!(
            cli_select_workspace_id(&infos, &beta.to_string()).expect("resolve beta"),
            beta
        );
    }

    #[test]
    fn shared_workspace_selector_matches_name() {
        let alpha = loom_core::WorkspaceId::from_bytes([3; 16]);
        let beta = loom_core::WorkspaceId::from_bytes([4; 16]);
        let infos = cli_workspace_infos_from_remote_records(&[
            workspace_record(*alpha.as_bytes(), "alpha"),
            workspace_record(*beta.as_bytes(), "beta"),
        ])
        .expect("decode workspace records");

        assert_eq!(
            cli_select_workspace_id(&infos, "alpha").expect("resolve alpha"),
            alpha
        );
    }

    #[test]
    fn shared_workspace_selector_reports_missing() {
        let alpha = loom_core::WorkspaceId::from_bytes([5; 16]);
        let infos = cli_workspace_infos_from_remote_records(&[workspace_record(
            *alpha.as_bytes(),
            "alpha",
        )])
        .expect("decode workspace records");

        assert!(cli_select_workspace_id(&infos, "missing").is_none());
    }

    #[test]
    fn shared_workspace_generated_decoder_rejects_malformed_record() {
        let error = cli_workspace_infos_from_generated_records(&[loom_codec::Value::Text(
            "not-workspace-cbor".to_string(),
        )])
        .expect_err("malformed generated record");

        assert!(error.contains("Workspaces.workspace_list returned unexpected record"));
    }

    #[test]
    fn cli_generated_client_resolves_workspace_name_and_uuid_to_same_id() {
        let store = temp_store("generated-workspace-resolver");
        let generated = open_cli_execution_context(&store)
            .expect("open execution context")
            .into_generated_client()
            .expect("direct local generated client");
        let workspace_id = workspace_id_from_value(
            generated
                .execute_unary(&workspace_create_operation("chatspace"))
                .expect("workspace create"),
        );

        assert_eq!(
            generated
                .resolve_workspace_id("chatspace")
                .expect("resolve by name")
                .to_string(),
            workspace_id
        );
        assert_eq!(
            generated
                .resolve_workspace_id(&workspace_id)
                .expect("resolve by id")
                .to_string(),
            workspace_id
        );
    }

    #[test]
    fn mu_6c_generated_pim_mutations_preserve_canonical_payloads() {
        let store = temp_store("generated-pim-mutations");
        let generated = open_cli_execution_context(&store)
            .expect("open execution context")
            .into_generated_client()
            .expect("direct local generated client");

        let calendar_meta = loom_core::calendar::CollectionMeta {
            display_name: "Work".to_string(),
            component_set: vec![loom_core::calendar::Component::Event],
        };
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Calendar",
                    "create_collection",
                    vec![
                        "pim".to_value(),
                        "alice".to_value(),
                        "work".to_value(),
                        loom_codec::Value::Bytes(calendar_meta.encode()),
                    ],
                )
                .expect("calendar create operation"),
            )
            .expect("calendar create_collection");
        let calendar_entry =
            loom_core::calendar::CalendarEntry::event("evt-1", "Standup", "20240115T100000");
        let calendar_digest = generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Calendar",
                    "put_entry",
                    vec![
                        "pim".to_value(),
                        "alice".to_value(),
                        "work".to_value(),
                        loom_codec::Value::Bytes(calendar_entry.encode()),
                    ],
                )
                .expect("calendar put_entry operation"),
            )
            .expect("calendar put_entry");
        assert!(
            matches!(calendar_digest, loom_codec::Value::Text(_)),
            "calendar put_entry returns digest text"
        );
        let calendar_ics_digest = generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Calendar",
                    "put_ics",
                    vec![
                        "pim".to_value(),
                        "alice".to_value(),
                        "work".to_value(),
                        "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:evt-2\r\nSUMMARY:Review\r\nDTSTART:20240116T100000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n".to_value(),
                    ],
                )
                .expect("calendar put_ics operation"),
            )
            .expect("calendar put_ics");
        assert!(
            matches!(calendar_ics_digest, loom_codec::Value::Text(_)),
            "calendar put_ics returns digest text"
        );
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Calendar",
                        "delete_entry",
                        vec![
                            "pim".to_value(),
                            "alice".to_value(),
                            "work".to_value(),
                            "evt-1".to_value(),
                        ],
                    )
                    .expect("calendar delete_entry operation"),
                )
                .expect("calendar delete_entry"),
            loom_codec::Value::Bool(true)
        );
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Calendar",
                        "delete_collection",
                        vec!["pim".to_value(), "alice".to_value(), "work".to_value()],
                    )
                    .expect("calendar delete_collection operation"),
                )
                .expect("calendar delete_collection"),
            loom_codec::Value::Bool(true)
        );

        let contacts_meta = loom_core::contacts::BookMeta {
            display_name: "Friends".to_string(),
        };
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Contacts",
                    "create_book",
                    vec![
                        "pim".to_value(),
                        "alice".to_value(),
                        "friends".to_value(),
                        loom_codec::Value::Bytes(contacts_meta.encode()),
                    ],
                )
                .expect("contacts create_book operation"),
            )
            .expect("contacts create_book");
        let contact = loom_core::contacts::ContactEntry::new("c-1", "Bob Jones");
        let contact_digest = generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Contacts",
                    "put_entry",
                    vec![
                        "pim".to_value(),
                        "alice".to_value(),
                        "friends".to_value(),
                        loom_codec::Value::Bytes(contact.encode()),
                    ],
                )
                .expect("contacts put_entry operation"),
            )
            .expect("contacts put_entry");
        assert!(
            matches!(contact_digest, loom_codec::Value::Text(_)),
            "contacts put_entry returns digest text"
        );
        let vcard_digest = generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Contacts",
                    "put_vcard",
                    vec![
                        "pim".to_value(),
                        "alice".to_value(),
                        "friends".to_value(),
                        "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c-2\r\nFN:Ada Lovelace\r\nEND:VCARD\r\n"
                            .to_value(),
                    ],
                )
                .expect("contacts put_vcard operation"),
            )
            .expect("contacts put_vcard");
        assert!(
            matches!(vcard_digest, loom_codec::Value::Text(_)),
            "contacts put_vcard returns digest text"
        );
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Contacts",
                        "delete_entry",
                        vec![
                            "pim".to_value(),
                            "alice".to_value(),
                            "friends".to_value(),
                            "c-1".to_value(),
                        ],
                    )
                    .expect("contacts delete_entry operation"),
                )
                .expect("contacts delete_entry"),
            loom_codec::Value::Bool(true)
        );
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Contacts",
                        "delete_book",
                        vec!["pim".to_value(), "alice".to_value(), "friends".to_value()],
                    )
                    .expect("contacts delete_book operation"),
                )
                .expect("contacts delete_book"),
            loom_codec::Value::Bool(true)
        );

        let mail_meta = loom_core::mail::MailboxMeta {
            display_name: "Inbox".to_string(),
        };
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Mail",
                    "create_mailbox",
                    vec![
                        "pim".to_value(),
                        "alice".to_value(),
                        "inbox".to_value(),
                        loom_codec::Value::Bytes(mail_meta.encode()),
                    ],
                )
                .expect("mail create_mailbox operation"),
            )
            .expect("mail create_mailbox");
        let message_digest = generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Mail",
                    "ingest_message",
                    vec![
                        "pim".to_value(),
                        "alice".to_value(),
                        "inbox".to_value(),
                        "m-1".to_value(),
                        loom_codec::Value::Bytes(
                            b"From: bob@example.com\r\nTo: alice@example.com\r\nSubject: Hello\r\n\r\nHi\r\n"
                                .to_vec(),
                        ),
                    ],
                )
                .expect("mail ingest_message operation"),
            )
            .expect("mail ingest_message");
        assert!(
            matches!(message_digest, loom_codec::Value::Text(_)),
            "mail ingest_message returns digest text"
        );
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Mail",
                    "set_flags",
                    vec![
                        "pim".to_value(),
                        "alice".to_value(),
                        "inbox".to_value(),
                        "m-1".to_value(),
                        loom_codec::Value::Bytes(
                            loom_wire::string_list_to_cbor(vec!["\\Seen".to_string()])
                                .expect("flags cbor"),
                        ),
                    ],
                )
                .expect("mail set_flags operation"),
            )
            .expect("mail set_flags");
        let flags = generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Mail",
                    "get_flags",
                    vec![
                        "pim".to_value(),
                        "alice".to_value(),
                        "inbox".to_value(),
                        "m-1".to_value(),
                    ],
                )
                .expect("mail get_flags operation"),
            )
            .expect("mail get_flags");
        let loom_codec::Value::Bytes(flags) = flags else {
            panic!("mail get_flags should return cbor bytes");
        };
        assert_eq!(
            loom_wire::string_list_from_cbor(&flags).expect("decode flags"),
            vec!["\\Seen".to_string()]
        );
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Mail",
                        "delete_message",
                        vec![
                            "pim".to_value(),
                            "alice".to_value(),
                            "inbox".to_value(),
                            "m-1".to_value(),
                        ],
                    )
                    .expect("mail delete_message operation"),
                )
                .expect("mail delete_message"),
            loom_codec::Value::Bool(true)
        );
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Mail",
                        "delete_mailbox",
                        vec!["pim".to_value(), "alice".to_value(), "inbox".to_value()],
                    )
                    .expect("mail delete_mailbox operation"),
                )
                .expect("mail delete_mailbox"),
            loom_codec::Value::Bool(true)
        );
    }

    #[test]
    fn mu_6d_generated_graph_vector_search_mutations_preserve_payloads() {
        let store = temp_store("generated-graph-vector-search-mutations");
        let generated = open_cli_execution_context(&store)
            .expect("open execution context")
            .into_generated_client()
            .expect("direct local generated client");

        let empty_props = loom_wire::graph::props_to_cbor(&Props::new());
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Graph",
                    "upsert_node",
                    vec![
                        "graphws".to_value(),
                        "main".to_value(),
                        "a".to_value(),
                        loom_codec::Value::Bytes(empty_props.clone()),
                    ],
                )
                .expect("graph upsert_node a operation"),
            )
            .expect("graph upsert_node a");
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Graph",
                    "upsert_node",
                    vec![
                        "graphws".to_value(),
                        "main".to_value(),
                        "b".to_value(),
                        loom_codec::Value::Bytes(empty_props.clone()),
                    ],
                )
                .expect("graph upsert_node b operation"),
            )
            .expect("graph upsert_node b");
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Graph",
                    "upsert_edge",
                    vec![
                        "graphws".to_value(),
                        "main".to_value(),
                        "e1".to_value(),
                        "a".to_value(),
                        "b".to_value(),
                        "knows".to_value(),
                        loom_codec::Value::Bytes(empty_props.clone()),
                    ],
                )
                .expect("graph upsert_edge operation"),
            )
            .expect("graph upsert_edge");
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Graph",
                        "remove_edge",
                        vec!["graphws".to_value(), "main".to_value(), "e1".to_value()],
                    )
                    .expect("graph remove_edge operation"),
                )
                .expect("graph remove_edge"),
            loom_codec::Value::Bool(true)
        );
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Graph",
                    "remove_node",
                    vec![
                        "graphws".to_value(),
                        "main".to_value(),
                        "a".to_value(),
                        false.to_value(),
                    ],
                )
                .expect("graph remove_node operation"),
            )
            .expect("graph remove_node");

        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Vector",
                    "create",
                    vec![
                        "vectorws".to_value(),
                        "embeddings".to_value(),
                        2_u64.to_value(),
                        1_i32.to_value(),
                    ],
                )
                .expect("vector create operation"),
            )
            .expect("vector create");
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Vector",
                    "upsert",
                    vec![
                        "vectorws".to_value(),
                        "embeddings".to_value(),
                        "v1".to_value(),
                        loom_codec::Value::Bytes(loom_wire::vector::floats_to_bytes(&[1.0, 0.0])),
                        loom_codec::Value::Bytes(Vec::new()),
                    ],
                )
                .expect("vector upsert operation"),
            )
            .expect("vector upsert");
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Vector",
                    "upsert_source",
                    vec![
                        "vectorws".to_value(),
                        "embeddings".to_value(),
                        "v2".to_value(),
                        loom_codec::Value::Bytes(loom_wire::vector::floats_to_bytes(&[0.0, 1.0])),
                        loom_codec::Value::Bytes(Vec::new()),
                        loom_codec::Value::Bytes(b"source text".to_vec()),
                        Some("model-a".to_string()).to_value(),
                        Some("weights-a".to_string()).to_value(),
                    ],
                )
                .expect("vector upsert_source operation"),
            )
            .expect("vector upsert_source");
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Vector",
                        "create_metadata_index",
                        vec![
                            "vectorws".to_value(),
                            "embeddings".to_value(),
                            "topic".to_value(),
                        ],
                    )
                    .expect("vector create_metadata_index operation"),
                )
                .expect("vector create_metadata_index"),
            loom_codec::Value::Bool(true)
        );
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Vector",
                        "drop_metadata_index",
                        vec![
                            "vectorws".to_value(),
                            "embeddings".to_value(),
                            "topic".to_value(),
                        ],
                    )
                    .expect("vector drop_metadata_index operation"),
                )
                .expect("vector drop_metadata_index"),
            loom_codec::Value::Bool(true)
        );
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Vector",
                        "delete",
                        vec![
                            "vectorws".to_value(),
                            "embeddings".to_value(),
                            "v1".to_value(),
                        ],
                    )
                    .expect("vector delete operation"),
                )
                .expect("vector delete"),
            loom_codec::Value::Bool(true)
        );

        let mapping = loom_codec::encode(&loom_codec::Value::Map(vec![(
            loom_codec::Value::Text("title".to_string()),
            loom_codec::Value::Array(vec![
                loom_codec::Value::Uint(0),
                loom_codec::Value::Bool(true),
                loom_codec::Value::Bool(false),
            ]),
        )]))
        .expect("search mapping cbor");
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Search",
                    "create",
                    vec![
                        "searchws".to_value(),
                        "docs".to_value(),
                        loom_codec::Value::Bytes(mapping.clone()),
                    ],
                )
                .expect("search create operation"),
            )
            .expect("search create");
        let doc = loom_codec::encode(&loom_codec::Value::Map(vec![(
            loom_codec::Value::Text("title".to_string()),
            loom_codec::Value::Text("Hello".to_string()),
        )]))
        .expect("search document cbor");
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Search",
                    "index",
                    vec![
                        "searchws".to_value(),
                        "docs".to_value(),
                        loom_codec::Value::Bytes(b"doc-1".to_vec()),
                        loom_codec::Value::Bytes(doc),
                    ],
                )
                .expect("search index operation"),
            )
            .expect("search index");
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Search",
                        "delete",
                        vec![
                            "searchws".to_value(),
                            "docs".to_value(),
                            loom_codec::Value::Bytes(b"doc-1".to_vec()),
                        ],
                    )
                    .expect("search delete operation"),
                )
                .expect("search delete"),
            loom_codec::Value::Bool(true)
        );
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Search",
                    "remap",
                    vec![
                        "searchws".to_value(),
                        "docs".to_value(),
                        loom_codec::Value::Bytes(mapping),
                    ],
                )
                .expect("search remap operation"),
            )
            .expect("search remap");
    }

    #[test]
    fn mu_6e_generated_columnar_files_vcs_import_fs_mutations_preserve_payloads() {
        let store = temp_store("generated-columnar-files-vcs-import-fs-mutations");
        let generated = open_cli_execution_context(&store)
            .expect("open execution context")
            .into_generated_client()
            .expect("direct local generated client");

        let columns = loom_wire::columnar::columns_to_cbor(vec![
            ("id".to_string(), ColumnType::Int),
            ("title".to_string(), ColumnType::Text),
        ]);
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Columnar",
                    "create",
                    vec![
                        "columnarws".to_value(),
                        "events".to_value(),
                        loom_codec::Value::Bytes(columns),
                        2_u64.to_value(),
                    ],
                )
                .expect("columnar create operation"),
            )
            .expect("columnar create");
        let row = loom_wire::columnar::values_to_cbor(vec![
            loom_core::Value::Int(1),
            loom_core::Value::Text("created".to_string()),
        ]);
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Columnar",
                    "append",
                    vec![
                        "columnarws".to_value(),
                        "events".to_value(),
                        loom_codec::Value::Bytes(row),
                    ],
                )
                .expect("columnar append operation"),
            )
            .expect("columnar append");
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Columnar",
                    "compact",
                    vec!["columnarws".to_value(), "events".to_value()],
                )
                .expect("columnar compact operation"),
            )
            .expect("columnar compact");

        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "FileSystem",
                    "create_directory",
                    vec!["filesws".to_value(), "dir".to_value(), true.to_value()],
                )
                .expect("filesystem create_directory operation"),
            )
            .expect("filesystem create_directory");
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "FileSystem",
                    "write_file",
                    vec![
                        "filesws".to_value(),
                        "dir/file.txt".to_value(),
                        loom_codec::Value::Bytes(b"hello".to_vec()),
                        0o100644_u64.to_value(),
                    ],
                )
                .expect("filesystem write_file operation"),
            )
            .expect("filesystem write_file");
        let read = generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "FileSystem",
                    "read_file",
                    vec!["filesws".to_value(), "dir/file.txt".to_value()],
                )
                .expect("filesystem read_file operation"),
            )
            .expect("filesystem read_file");
        assert_eq!(read, loom_codec::Value::Bytes(b"hello".to_vec()));
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "FileSystem",
                    "remove_file",
                    vec!["filesws".to_value(), "dir/file.txt".to_value()],
                )
                .expect("filesystem remove_file operation"),
            )
            .expect("filesystem remove_file");
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "FileSystem",
                    "remove_directory",
                    vec!["filesws".to_value(), "dir".to_value(), true.to_value()],
                )
                .expect("filesystem remove_directory operation"),
            )
            .expect("filesystem remove_directory");

        let commit = generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "VersionControl",
                    "commit",
                    vec![
                        "filesws".to_value(),
                        "agent".to_value(),
                        "checkpoint".to_value(),
                        1_u64.to_value(),
                    ],
                )
                .expect("vcs commit operation"),
            )
            .expect("vcs commit");
        assert!(matches!(commit, loom_codec::Value::Text(_)));
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "VersionControl",
                    "branch",
                    vec!["filesws".to_value(), "feature".to_value()],
                )
                .expect("vcs branch operation"),
            )
            .expect("vcs branch");
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "VersionControl",
                    "checkout",
                    vec!["filesws".to_value(), "feature".to_value()],
                )
                .expect("vcs checkout operation"),
            )
            .expect("vcs checkout");
        let merge = generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "VersionControl",
                    "merge",
                    vec![
                        "filesws".to_value(),
                        "feature".to_value(),
                        "agent".to_value(),
                        false.to_value(),
                        2_u64.to_value(),
                    ],
                )
                .expect("vcs merge operation"),
            )
            .expect("vcs merge");
        let loom_codec::Value::Bytes(merge) = merge else {
            panic!("vcs merge should return cbor bytes");
        };
        assert_eq!(
            loom_wire::vcs::merge_result_from_cbor(&merge).expect("decode vcs merge"),
            MergeOutcome::UpToDate
        );

        let source = temp_dir("generated-import-fs-source");
        std::fs::write(source.join("note.txt"), b"imported").expect("write import source");
        let report = generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "FileSystem",
                    "import_fs",
                    vec![
                        "importws".to_value(),
                        source.to_string_lossy().into_owned().to_value(),
                        Option::<String>::None.to_value(),
                        Option::<String>::None.to_value(),
                        false.to_value(),
                        true.to_value(),
                    ],
                )
                .expect("filesystem import_fs operation"),
            )
            .expect("filesystem import_fs");
        let loom_codec::Value::Bytes(report) = report else {
            panic!("filesystem import_fs should return cbor bytes");
        };
        let report = loom_interchange::ImportReport::decode(&report)
            .expect("decode generated import report");
        assert_eq!(report.profile, "fs");
        assert!(report.dry_run, "dry-run field should remain true");
    }

    #[test]
    fn read_only_generated_client_reads_without_store_drift() {
        let store = temp_store("read-only-generated");
        let writable = open_cli_execution_context(&store)
            .expect("open execution context")
            .into_generated_client()
            .expect("direct local generated client");
        writable
            .execute_unary(&workspace_create_operation("readonly"))
            .expect("workspace create");
        drop(writable);
        let before_bytes = std::fs::read(&store).expect("store bytes before");
        let before_meta = std::fs::metadata(&store).expect("store metadata before");
        let before_generation = FileStore::open_read(&store)
            .expect("open store read-only")
            .mutable_overlay_generation()
            .expect("read generation")
            .as_u64();

        let generated = open_cli_read_only_generated_client(&store, &KeyOpts::default())
            .expect("read-only generated client");
        let value = generated
            .execute_unary(
                &CliGeneratedOperation::new("Workspaces", "workspace_list", Vec::new())
                    .expect("workspace list operation"),
            )
            .expect("workspace list");
        drop(generated);

        match value {
            loom_codec::Value::Array(items) => assert!(!items.is_empty()),
            other => panic!("unexpected workspace list output {other:?}"),
        }
        assert_eq!(
            std::fs::read(&store).expect("store bytes after"),
            before_bytes
        );
        let after_meta = std::fs::metadata(&store).expect("store metadata after");
        assert_eq!(after_meta.len(), before_meta.len());
        assert_eq!(
            after_meta.modified().expect("mtime after"),
            before_meta.modified().expect("mtime before")
        );
        let after_generation = FileStore::open_read(&store)
            .expect("open store read-only after")
            .mutable_overlay_generation()
            .expect("read generation after")
            .as_u64();
        assert_eq!(after_generation, before_generation);
    }

    #[test]
    fn mu_17g_d1_doc_get_text_uses_generated_text_result() {
        let store = temp_store("doc-get-text-generated");
        let generated = open_cli_execution_context(&store)
            .expect("open execution context")
            .into_generated_client()
            .expect("direct local generated client");
        generated
            .execute_unary(&workspace_create_operation("docs"))
            .expect("workspace create");
        let put = generated
            .doc_put_text("docs", "notes", "one", "hello from text\n", None)
            .expect("put text");

        let document = generated
            .doc_get_text("docs", "notes", "one")
            .expect("get text")
            .expect("document exists");

        assert_eq!(document.text, "hello from text\n");
        assert_eq!(document.digest, put.digest);
        assert_eq!(document.entity_tag, put.entity_tag);
        let helper_body = function_body(include_str!("remote.rs"), "doc_get_text");
        assert!(
            helper_body.contains("\"get_text\""),
            "doc_get_text must execute Document.get_text"
        );
        assert!(
            helper_body.contains("text_result_from_cbor"),
            "doc_get_text must decode DocumentTextResult"
        );
        assert!(
            !helper_body.contains("doc_get_binary"),
            "doc_get_text must not reinterpret Document.get_binary"
        );
    }

    #[test]
    fn cli_store_administration_classification_is_explicit() {
        assert_eq!(
            classify_cli_operation(CliOperation::Init),
            CliExecutionBoundary::StoreAdministration(
                CliStoreAdministrationBoundary::OfflineStoreOwner
            )
        );
        assert_eq!(
            classify_cli_operation(CliOperation::Copy),
            CliExecutionBoundary::StoreAdministration(
                CliStoreAdministrationBoundary::OfflineStoreOwner
            )
        );
        assert_eq!(
            classify_cli_operation(CliOperation::Stat),
            CliExecutionBoundary::StoreAdministration(
                CliStoreAdministrationBoundary::GeneratedStoreAdmin
            )
        );
        for method in [
            "store_stat",
            "store_policy_get",
            "store_policy_set",
            "store_rekey",
        ] {
            assert_eq!(
                classify_generated_operation("StoreAdmin", method).expect("store admin method"),
                CliExecutionBoundary::GeneratedClient
            );
        }
    }

    #[test]
    fn cli_store_administration_classification_is_complete_and_unique() {
        for (operation, expected_boundary) in [
            (
                CliOperation::Init,
                CliStoreAdministrationBoundary::OfflineStoreOwner,
            ),
            (
                CliOperation::Copy,
                CliStoreAdministrationBoundary::OfflineStoreOwner,
            ),
            (
                CliOperation::BundleExport,
                CliStoreAdministrationBoundary::OfflineStoreOwner,
            ),
            (
                CliOperation::BundleImport,
                CliStoreAdministrationBoundary::OfflineStoreOwner,
            ),
            (
                CliOperation::Clone,
                CliStoreAdministrationBoundary::OfflineStoreOwner,
            ),
            (
                CliOperation::Get,
                CliStoreAdministrationBoundary::OfflineStoreOwner,
            ),
            (
                CliOperation::Hash,
                CliStoreAdministrationBoundary::OfflineStoreOwner,
            ),
            (
                CliOperation::KeyChange,
                CliStoreAdministrationBoundary::OfflineStoreOwner,
            ),
            (
                CliOperation::KeyCreate,
                CliStoreAdministrationBoundary::OfflineStoreOwner,
            ),
            (
                CliOperation::KeyStatus,
                CliStoreAdministrationBoundary::GeneratedStoreAdmin,
            ),
            (
                CliOperation::KeyVerify,
                CliStoreAdministrationBoundary::GeneratedStoreAdmin,
            ),
            (
                CliOperation::Policy,
                CliStoreAdministrationBoundary::GeneratedStoreAdmin,
            ),
            (
                CliOperation::Put,
                CliStoreAdministrationBoundary::OfflineStoreOwner,
            ),
            (
                CliOperation::Rekey,
                CliStoreAdministrationBoundary::GeneratedStoreAdmin,
            ),
            (
                CliOperation::Replace,
                CliStoreAdministrationBoundary::OfflineStoreOwner,
            ),
            (
                CliOperation::Stat,
                CliStoreAdministrationBoundary::GeneratedStoreAdmin,
            ),
        ] {
            let (boundary, reason) = cli_store_administration_boundary_reason(operation);
            assert_eq!(boundary, expected_boundary);
            assert!(
                !reason.trim().is_empty(),
                "store administration operation {operation:?} must have an architectural reason"
            );
        }

        for (arm, interface, method) in [
            (
                "StoreCmd::BundleImport",
                "StoreAdmin",
                "store_bundle_import",
            ),
            ("StoreCmd::Stat", "StoreAdmin", "store_stat"),
            ("StoreCmd::Policy", "StoreAdmin", "store_policy_get"),
            ("StoreCmd::Rekey", "StoreAdmin", "store_rekey"),
            ("KeyCmd::AddWrap", "KeySource", "key_add_wrap_keyed"),
            ("KeyCmd::RemoveWrap", "KeySource", "key_remove_wrap"),
        ] {
            assert!(!arm.is_empty());
            assert!(
                METHODS.iter().any(|candidate| {
                    candidate.interface == interface && candidate.method == method
                }),
                "generated registry must expose {interface}.{method}"
            );
        }
    }

    #[test]
    fn generated_operation_boundary_uses_source_inventory_for_hot_families() {
        for (interface, method) in [
            ("Tickets", "tickets_create_json"),
            ("Lanes", "update"),
            ("Pages", "pages_create_json"),
            ("Document", "put_text"),
            ("FileSystem", "write_file"),
            ("Program", "program_put"),
            ("Program", "program_inspect"),
            ("Program", "program_get"),
            ("Program", "program_list"),
            ("Program", "program_remove"),
            ("TimeSeries", "latest"),
            ("Metrics", "put_descriptor"),
            ("Metrics", "get_descriptor"),
            ("Metrics", "put_observation"),
            ("Metrics", "query"),
            ("Logs", "put_record"),
            ("Logs", "get_record"),
            ("Logs", "query"),
            ("Traces", "put_span"),
            ("Traces", "get_span"),
            ("Traces", "trace_spans"),
            ("Traces", "query"),
            ("Dataframe", "create"),
            ("Dataframe", "collect"),
            ("Dataframe", "preview"),
            ("Dataframe", "materialize"),
            ("Dataframe", "plan_digest"),
            ("Dataframe", "source_digests"),
        ] {
            assert_eq!(
                classify_generated_operation(interface, method).expect("hot family operation"),
                CliExecutionBoundary::GeneratedClient
            );
        }
        assert!(CliGeneratedOperation::new("Tickets", "missing", Vec::new()).is_err());
        assert!(
            CliGeneratedOperation::new("Transfer", "transfer_export", Vec::new())
                .expect_err("streaming operation is rejected")
                .contains("streaming")
        );
    }

    #[test]
    fn migrated_ordinary_cli_runners_do_not_open_loom_directly() {
        let source = include_str!("main.rs");
        for runner in [
            "run_time_series",
            "run_metrics",
            "run_logs",
            "run_traces",
            "run_program",
            "run_dataframe",
        ] {
            let body = function_body(source, runner);
            for forbidden in [
                "cli_open_loom(",
                "cli_open_loom_read(",
                "FileStore::open",
                "Loom::new(",
                "save_loom(",
            ] {
                assert!(
                    !body.contains(forbidden),
                    "{runner} must use the generated client boundary, found {forbidden}"
                );
            }
        }
    }

    #[test]
    fn mu_17g_d5_program_metrics_logs_traces_leaves_use_typed_generated_clients() {
        let source = include_str!("main.rs");
        for (runner, arm, opener, interface, method) in [
            (
                "run_program",
                "ProgramCmd::PutWasm",
                "remote::open_cli_generated_client(&store, keys)?",
                "Program",
                "program_put",
            ),
            (
                "run_program",
                "ProgramCmd::PutTemplate",
                "remote::open_cli_generated_client(&store, keys)?",
                "Program",
                "program_put",
            ),
            (
                "run_program",
                "ProgramCmd::PutCel",
                "remote::open_cli_generated_client(&store, keys)?",
                "Program",
                "program_put",
            ),
            (
                "run_program",
                "ProgramCmd::Inspect",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Program",
                "program_inspect",
            ),
            (
                "run_program",
                "ProgramCmd::Get",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Program",
                "program_get",
            ),
            (
                "run_program",
                "ProgramCmd::List",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Program",
                "program_list",
            ),
            (
                "run_program",
                "ProgramCmd::Remove",
                "remote::open_cli_generated_client(&store, keys)?",
                "Program",
                "program_remove",
            ),
            (
                "run_metrics",
                "MetricsCmd::PutDescriptor",
                "remote::open_cli_generated_client(&store, keys)?",
                "Metrics",
                "put_descriptor",
            ),
            (
                "run_metrics",
                "MetricsCmd::GetDescriptor",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Metrics",
                "get_descriptor",
            ),
            (
                "run_metrics",
                "MetricsCmd::PutObservation",
                "remote::open_cli_generated_client(&store, keys)?",
                "Metrics",
                "put_observation",
            ),
            (
                "run_metrics",
                "MetricsCmd::Query",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Metrics",
                "query",
            ),
            (
                "run_logs",
                "LogsCmd::PutRecord",
                "remote::open_cli_generated_client(&store, keys)?",
                "Logs",
                "put_record",
            ),
            (
                "run_logs",
                "LogsCmd::GetRecord",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Logs",
                "get_record",
            ),
            (
                "run_logs",
                "LogsCmd::Query",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Logs",
                "query",
            ),
            (
                "run_traces",
                "TracesCmd::PutSpan",
                "remote::open_cli_generated_client(&store, keys)?",
                "Traces",
                "put_span",
            ),
            (
                "run_traces",
                "TracesCmd::GetSpan",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Traces",
                "get_span",
            ),
            (
                "run_traces",
                "TracesCmd::TraceSpans",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Traces",
                "trace_spans",
            ),
            (
                "run_traces",
                "TracesCmd::Query",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Traces",
                "query",
            ),
        ] {
            let body = match_arm_body(function_body(source, runner), arm);
            assert!(body.contains(opener), "{runner} {arm} must select {opener}");
            assert!(
                body.contains(&format!("\"{interface}\"")),
                "{runner} {arm} must dispatch through generated interface {interface}"
            );
            assert!(
                body.contains(&format!("\"{method}\"")),
                "{runner} {arm} must dispatch through generated method {method}"
            );
            for forbidden in [
                "remote::open_store_client(",
                "cli_open_loom(",
                "cli_open_loom_read(",
                "FileStore::open",
                "Loom::new(",
                "save_loom(",
            ] {
                assert!(
                    !body.contains(forbidden),
                    "{runner} {arm} must not use legacy execution helper {forbidden}"
                );
            }
        }
    }

    #[test]
    fn mu_17g_f1_security_admin_leaves_use_typed_generated_clients() {
        let cli_source = include_str!("cli.rs");
        assert_eq!(
            enum_variants(cli_source, "AuditCmd"),
            ["Compact", "Config", "List", "View"]
        );
        assert_eq!(enum_variants(cli_source, "AuditConfigCmd"), ["Show", "Set"]);
        assert_eq!(
            enum_variants(cli_source, "CertificateCmd"),
            ["List", "Import", "Export", "Generate", "Remove", "Audit"]
        );
        assert_eq!(
            enum_variants(cli_source, "CertificateGenerateCmd"),
            ["SelfSigned"]
        );
        assert_eq!(
            enum_variants(cli_source, "NetworkAccessCmd"),
            ["List", "Set", "Remove", "Audit"]
        );

        let audit_source = include_str!("audit_cmd.rs");
        let audit_config = function_body(audit_source, "run_audit_config");
        let audit_config_show = match_arm_body(audit_config, "AuditConfigCmd::Show");
        assert!(audit_config_show.contains("remote::open_cli_generated_client"));
        assert!(audit_config_show.contains("\"Audit\""));
        assert!(audit_config_show.contains("\"audit_config_show_json\""));
        assert!(!audit_config_show.contains("audit_config_set_json"));
        let audit_config_set = match_arm_body(audit_config, "AuditConfigCmd::Set");
        assert!(audit_config_set.contains("remote::open_cli_generated_client"));
        assert!(audit_config_set.contains("\"Audit\""));
        assert!(audit_config_set.contains("\"audit_config_set_json\""));
        assert!(!audit_config_set.contains("audit_config_show_json"));

        for (arm, source, runner, interface, method) in [
            (
                "AuditCmd::Compact",
                include_str!("audit_cmd.rs"),
                "run_audit_compact",
                "Audit",
                "audit_compact",
            ),
            (
                "AuditCmd::List",
                include_str!("audit_cmd.rs"),
                "run_audit_list",
                "Audit",
                "audit_list_json",
            ),
            (
                "AuditCmd::View",
                include_str!("audit_cmd.rs"),
                "run_audit_view",
                "Audit",
                "audit_view_json",
            ),
            (
                "CertificateCmd::List",
                include_str!("certificate_cmd.rs"),
                "run_certificate_list",
                "Certificate",
                "certificate_list_json",
            ),
            (
                "CertificateCmd::Import",
                include_str!("certificate_cmd.rs"),
                "run_certificate_import",
                "Certificate",
                "certificate_import_json",
            ),
            (
                "CertificateCmd::Export",
                include_str!("certificate_cmd.rs"),
                "run_certificate_export",
                "Certificate",
                "certificate_export",
            ),
            (
                "CertificateGenerateCmd::SelfSigned",
                include_str!("certificate_cmd.rs"),
                "run_certificate_generate_self_signed",
                "Certificate",
                "certificate_generate_self_signed_json",
            ),
            (
                "CertificateCmd::Remove",
                include_str!("certificate_cmd.rs"),
                "run_certificate_remove",
                "Certificate",
                "certificate_remove_json",
            ),
            (
                "CertificateCmd::Audit",
                include_str!("certificate_cmd.rs"),
                "run_certificate_audit",
                "Certificate",
                "certificate_audit_json",
            ),
            (
                "NetworkAccessCmd::List",
                include_str!("network_access_cmd.rs"),
                "run_network_access_list",
                "NetworkAccess",
                "network_access_list_json",
            ),
            (
                "NetworkAccessCmd::Set",
                include_str!("network_access_cmd.rs"),
                "run_network_access_set",
                "NetworkAccess",
                "network_access_set_json",
            ),
            (
                "NetworkAccessCmd::Remove",
                include_str!("network_access_cmd.rs"),
                "run_network_access_remove",
                "NetworkAccess",
                "network_access_remove_json",
            ),
            (
                "NetworkAccessCmd::Audit",
                include_str!("network_access_cmd.rs"),
                "run_network_access_audit",
                "NetworkAccess",
                "network_access_audit_json",
            ),
        ] {
            assert!(!arm.is_empty());
            assert_eq!(
                classify_generated_operation(interface, method).expect("security admin operation"),
                CliExecutionBoundary::GeneratedClient
            );
            let body = function_body(source, runner);
            assert!(
                body.contains("remote::open_cli_generated_client"),
                "{runner} must select the typed generated mutation client"
            );
            assert!(
                body.contains(&format!("\"{interface}\""))
                    && body.contains(&format!("\"{method}\"")),
                "{runner} must execute {interface}.{method}"
            );
            for forbidden in [
                "open_store_client",
                "cli_open_loom(",
                "cli_open_loom_read(",
                "FileStore::open",
                "save_loom(",
            ] {
                assert!(
                    !body.contains(forbidden),
                    "{runner} returned to forbidden store authority {forbidden}"
                );
            }
        }
    }

    #[test]
    fn mu_17g_f3_studio_vcs_inference_leaves_have_exhaustive_ownership() {
        let cli_source = include_str!("cli.rs");
        let main_source = include_str!("main.rs");

        assert_eq!(
            enum_variants(cli_source, "StudioCmd"),
            ["Surfaces", "Reindex", "Revisions"]
        );
        assert_eq!(enum_variants(cli_source, "StudioSurfacesCmd"), ["Catalog"]);
        assert_eq!(enum_variants(cli_source, "StudioRevisionsCmd"), ["Rebuild"]);
        assert_eq!(
            enum_variants(cli_source, "VcsCmd"),
            ["Branch", "Commit", "Checkout", "Diff", "Log", "Merge"]
        );
        assert_eq!(
            enum_variants(cli_source, "InferenceCmd"),
            [
                "Model", "Instance", "List", "Status", "Show", "Download", "Cancel", "Remove",
                "Refresh",
            ]
        );
        assert_eq!(
            enum_variants(cli_source, "InferenceModelCmd"),
            [
                "List", "Show", "Download", "Status", "Cancel", "Remove", "Refresh",
            ]
        );
        assert_eq!(
            enum_variants(cli_source, "InferenceInstanceCmd"),
            ["List", "Show", "Create", "Update", "Delete"]
        );

        let generated = [
            (
                "run_studio",
                "StudioCmd::Reindex",
                "StudioMaintenance",
                &["studio_reindex_json"][..],
                false,
            ),
            (
                "run_studio_revisions",
                "StudioRevisionsCmd::Rebuild",
                "StudioMaintenance",
                &["studio_revisions_rebuild_json"][..],
                false,
            ),
            (
                "run_vcs",
                "VcsCmd::Branch",
                "VersionControl",
                &["branch"][..],
                false,
            ),
            (
                "run_vcs",
                "VcsCmd::Commit",
                "VersionControl",
                &["commit"][..],
                false,
            ),
            (
                "run_vcs",
                "VcsCmd::Checkout",
                "VersionControl",
                &["checkout"][..],
                false,
            ),
            (
                "run_vcs",
                "VcsCmd::Diff",
                "VersionControl",
                &["diff"][..],
                true,
            ),
            (
                "run_vcs",
                "VcsCmd::Log",
                "VersionControl",
                &["head_branch", "log"][..],
                true,
            ),
            (
                "run_vcs",
                "VcsCmd::Merge",
                "VersionControl",
                &["merge"][..],
                false,
            ),
            (
                "run_inference_instance",
                "InferenceInstanceCmd::List",
                "InferenceInstance",
                &["inference_instance_list_json"][..],
                true,
            ),
            (
                "run_inference_instance",
                "InferenceInstanceCmd::Show",
                "InferenceInstance",
                &["inference_instance_get_json"][..],
                true,
            ),
            (
                "run_inference_instance",
                "InferenceInstanceCmd::Create",
                "InferenceInstance",
                &["inference_instance_create_json"][..],
                false,
            ),
            (
                "run_inference_instance",
                "InferenceInstanceCmd::Update",
                "InferenceInstance",
                &["inference_instance_update_json"][..],
                false,
            ),
            (
                "run_inference_instance",
                "InferenceInstanceCmd::Delete",
                "InferenceInstance",
                &["inference_instance_delete_json"][..],
                false,
            ),
        ];
        let generated_methods = [
            "studio_reindex_json",
            "studio_revisions_rebuild_json",
            "branch",
            "commit",
            "checkout",
            "diff",
            "head_branch",
            "log",
            "merge",
            "inference_instance_list_json",
            "inference_instance_get_json",
            "inference_instance_create_json",
            "inference_instance_update_json",
            "inference_instance_delete_json",
        ];
        let mut owned = std::collections::BTreeSet::new();
        for (runner, arm, interface, methods, read_only) in generated {
            assert!(owned.insert(arm), "duplicate ownership for {arm}");
            let body = match_arm_body(function_body(main_source, runner), arm);
            let opener = if read_only {
                "remote::open_cli_read_only_generated_client(&store, keys)?"
            } else {
                "remote::open_cli_generated_client(&store, keys)?"
            };
            assert!(body.contains(opener), "{arm} must select {opener}");
            assert!(
                body.contains(&format!("\"{interface}\"")),
                "{arm} must dispatch through {interface}"
            );
            for method in methods {
                assert!(
                    body.contains(&format!("\"{method}\"")),
                    "{arm} must dispatch through {interface}.{method}"
                );
            }
            for other in generated_methods {
                if !methods.contains(&other) {
                    assert!(
                        !body.contains(&format!("\"{other}\"")),
                        "{arm} contains incorrect generated method mapping {other}"
                    );
                }
            }
            for forbidden in [
                "remote::open_store_client(",
                "cli_open_loom(",
                "cli_open_loom_read(",
                "FileStore::open",
                "Loom::new(",
                "save_loom(",
            ] {
                assert!(!body.contains(forbidden), "{arm} contains {forbidden}");
            }
        }
        assert_eq!(owned.len(), 13);

        let catalog = match_arm_body(
            function_body(main_source, "run_studio_surfaces"),
            "StudioSurfacesCmd::Catalog",
        );
        assert!(
            owned.insert("StudioSurfacesCmd::Catalog"),
            "duplicate ownership for StudioSurfacesCmd::Catalog"
        );
        for required in [
            "core_surface_catalog",
            "surface_app_catalog",
            "meeting_memory_surface_catalog",
        ] {
            assert!(catalog.contains(required), "catalog missing {required}");
        }
        for forbidden in [
            "open_cli_generated_client",
            "open_cli_read_only_generated_client",
            "open_store_client",
            "cli_open_loom(",
            "cli_open_loom_read(",
            "FileStore::open",
            "save_loom(",
        ] {
            assert!(!catalog.contains(forbidden), "catalog contains {forbidden}");
        }

        for (runner, arm) in [
            ("run_inference", "InferenceCmd::List"),
            ("run_inference", "InferenceCmd::Status"),
            ("run_inference", "InferenceCmd::Show"),
            ("run_inference", "InferenceCmd::Download"),
            ("run_inference", "InferenceCmd::Cancel"),
            ("run_inference", "InferenceCmd::Remove"),
            ("run_inference", "InferenceCmd::Refresh"),
            ("run_inference_model", "InferenceModelCmd::List"),
            ("run_inference_model", "InferenceModelCmd::Show"),
            ("run_inference_model", "InferenceModelCmd::Download"),
            ("run_inference_model", "InferenceModelCmd::Status"),
            ("run_inference_model", "InferenceModelCmd::Cancel"),
            ("run_inference_model", "InferenceModelCmd::Remove"),
            ("run_inference_model", "InferenceModelCmd::Refresh"),
        ] {
            assert!(owned.insert(arm), "duplicate ownership for {arm}");
            let body = match_arm_body(function_body(main_source, runner), arm);
            for forbidden in [
                "open_cli_generated_client",
                "open_cli_read_only_generated_client",
                "open_store_client",
                "cli_open_loom(",
                "cli_open_loom_read(",
                "FileStore::open",
                "Loom::new(",
                "save_loom(",
            ] {
                assert!(!body.contains(forbidden), "{arm} contains {forbidden}");
            }
        }
        assert_eq!(owned.len(), 28);
    }

    #[test]
    fn mu_17g_f4_serve_and_daemon_leaves_have_one_execution_owner() {
        let cli_source = include_str!("cli.rs");
        let serve_source = include_str!("serve_cmd.rs");
        let daemon_source = include_str!("daemon_cmd.rs");
        let idl_source = include_str!("../../../idl/loom.idl");
        let service_source = include_str!("../../loom-client/src/service.rs");

        assert_eq!(
            enum_variants(cli_source, "ServeCmd"),
            [
                "Configure",
                "List",
                "Enable",
                "Disable",
                "Remove",
                "Route",
                "Remote",
            ]
        );
        assert_eq!(
            enum_variants(cli_source, "ServeRouteCmd"),
            ["List", "Set", "Remove"]
        );
        assert_eq!(
            enum_variants(cli_source, "DaemonCmd"),
            [
                "Start",
                "Stop",
                "Restart",
                "Status",
                "Maintenance",
                "Session",
                "Pin",
                "Run",
            ]
        );
        assert_eq!(
            enum_variants(cli_source, "DaemonMaintenanceCmd"),
            ["Status", "Policy", "Run"]
        );
        assert_eq!(
            enum_variants(cli_source, "DaemonSessionCmd"),
            ["Open", "Close", "Attach", "Detach"]
        );
        assert_eq!(enum_variants(cli_source, "DaemonPinCmd"), ["Add", "Remove"]);

        let run_serve = function_body(serve_source, "run_serve");
        for (arm, helper) in [
            ("ServeCmd::Configure", "run_serve_configure"),
            ("ServeCmd::List", "run_serve_list"),
            ("ServeCmd::Enable", "run_serve_set_enabled"),
            ("ServeCmd::Disable", "run_serve_set_enabled"),
            ("ServeCmd::Remove", "run_serve_remove"),
            ("ServeCmd::Route", "run_serve_route"),
        ] {
            let body = match_arm_segment(run_serve, arm, "ServeCmd");
            assert!(body.contains(helper), "{arm} must delegate to {helper}");
        }
        assert!(match_arm_segment(run_serve, "ServeCmd::Enable", "ServeCmd").contains("true"));
        assert!(match_arm_segment(run_serve, "ServeCmd::Disable", "ServeCmd").contains("false"));

        let run_serve_route = function_body(serve_source, "run_serve_route");
        for (arm, helper) in [
            ("ServeRouteCmd::List", "run_serve_route_list"),
            ("ServeRouteCmd::Set", "run_serve_route_set"),
            ("ServeRouteCmd::Remove", "run_serve_route_remove"),
        ] {
            let body = match_arm_segment(run_serve_route, arm, "ServeRouteCmd");
            assert!(body.contains(helper), "{arm} must delegate to {helper}");
        }

        let serve_generated = [
            ("run_serve_configure", "serve_listener_configure_json"),
            ("run_serve_list", "serve_listener_list_json"),
            ("run_serve_set_enabled", "serve_listener_set_enabled_json"),
            ("run_serve_remove", "serve_listener_remove_json"),
            ("run_serve_route_list", "serve_web_route_list_json"),
            ("run_serve_route_set", "serve_web_route_set_json"),
            ("run_serve_route_remove", "serve_web_route_remove_json"),
        ];
        let mut generated_owners = std::collections::BTreeSet::new();
        for (runner, method) in serve_generated {
            let body = function_body(serve_source, runner);
            assert!(body.contains("remote::open_cli_generated_client"));
            assert!(body.contains("\"ServeConfig\""));
            assert!(body.contains(&format!("\"{method}\"")));
            for forbidden in [
                "open_generated_client",
                "open_store_client",
                "StoreClient::",
                "cli_open_loom(",
                "cli_open_loom_read(",
                "FileStore::open",
                "save_loom(",
            ] {
                assert!(!body.contains(forbidden), "{runner} found {forbidden}");
            }
            assert!(idl_source.contains(&format!(" {method}(")));
            assert!(service_source.contains(&format!("fn {method}(")));
            assert!(
                generated_owners.insert(("ServeConfig", method)),
                "duplicate generated owner ServeConfig.{method}"
            );
        }

        let run_maintenance = function_body(daemon_source, "run_daemon_maintenance");
        for (arm, method, opener) in [
            (
                "DaemonMaintenanceCmd::Status",
                "store_maintenance_status",
                "open_cli_read_only_generated_client",
            ),
            (
                "DaemonMaintenanceCmd::Policy",
                "store_maintenance_policy_set",
                "open_cli_generated_client",
            ),
            (
                "DaemonMaintenanceCmd::Run",
                "store_maintenance_run",
                "open_cli_generated_client",
            ),
        ] {
            let body = match_arm_segment(run_maintenance, arm, "DaemonMaintenanceCmd");
            assert!(body.contains(opener), "{arm} must use {opener}");
            assert!(body.contains("\"StoreAdmin\""));
            assert!(body.contains(&format!("\"{method}\"")));
            for forbidden in [
                "open_store_client",
                "StoreClient::",
                "cli_open_loom(",
                "cli_open_loom_read(",
                "FileStore::open",
                "save_loom(",
                "run_store_maintenance_once(",
            ] {
                assert!(!body.contains(forbidden), "{arm} found {forbidden}");
            }
            assert!(idl_source.contains(&format!(" {method}(")));
            assert!(service_source.contains(&format!("fn {method}(")));
            assert!(
                generated_owners.insert(("StoreAdmin", method)),
                "duplicate generated owner StoreAdmin.{method}"
            );
        }
        assert_eq!(generated_owners.len(), 10);

        let run_daemon = function_body(daemon_source, "run_daemon");
        for (arm, required) in [
            ("DaemonCmd::Start", "daemon_start_with_transport"),
            ("DaemonCmd::Stop", "daemon_stop"),
            ("DaemonCmd::Restart", "daemon_start_with_transport_for"),
            ("DaemonCmd::Status", "daemon_status"),
            ("DaemonCmd::Maintenance", "run_daemon_maintenance"),
            ("DaemonCmd::Session", "daemon_session"),
            ("DaemonCmd::Pin", "daemon_pin_with_keys"),
            ("DaemonCmd::Run", "daemon_run"),
        ] {
            let body = match_arm_segment(run_daemon, arm, "DaemonCmd");
            assert!(body.contains(required), "{arm} must retain {required}");
        }
        let session = match_arm_segment(run_daemon, "DaemonCmd::Session", "DaemonCmd");
        assert!(
            match_arm_segment(session, "DaemonSessionCmd::Open", "DaemonSessionCmd")
                .contains("daemon_logical_session_open")
        );
        assert!(
            match_arm_segment(session, "DaemonSessionCmd::Close", "DaemonSessionCmd")
                .contains("daemon_logical_session_close")
        );
        assert!(
            match_arm_segment(session, "DaemonSessionCmd::Attach", "DaemonSessionCmd")
                .contains("\"attach\"")
        );
        assert!(
            match_arm_segment(session, "DaemonSessionCmd::Detach", "DaemonSessionCmd")
                .contains("\"detach\"")
        );
        let pin = match_arm_segment(run_daemon, "DaemonCmd::Pin", "DaemonCmd");
        assert!(
            match_arm_segment(pin, "DaemonPinCmd::Add", "DaemonPinCmd")
                .contains("daemon_pin_with_keys")
        );
        assert!(
            match_arm_segment(pin, "DaemonPinCmd::Remove", "DaemonPinCmd")
                .contains("daemon_unpin_with_keys")
        );

        for arm in [
            "DaemonCmd::Start",
            "DaemonCmd::Stop",
            "DaemonCmd::Restart",
            "DaemonCmd::Status",
            "DaemonCmd::Session",
            "DaemonCmd::Pin",
            "DaemonCmd::Run",
        ] {
            let body = match_arm_segment(run_daemon, arm, "DaemonCmd");
            assert!(!body.contains("open_store_client"));
            assert!(!body.contains("open_cli_generated_client"));
            assert!(!body.contains("cli_open_loom("));
            assert!(!body.contains("FileStore::open"));
        }
        let serve_remote = match_arm_segment(run_serve, "ServeCmd::Remote", "ServeCmd");
        assert!(serve_remote.contains("run_serve_remote"));
        assert!(!serve_remote.contains("open_cli_generated_client"));
        let serve_remote_runner = function_body(serve_source, "run_serve_remote");
        assert!(serve_remote_runner.contains("bind_remote_endpoint"));
        assert!(serve_remote_runner.contains("ctrlc::set_handler"));

        for method in [
            "daemon_start",
            "daemon_stop",
            "daemon_restart",
            "daemon_status",
            "daemon_session_attach",
            "daemon_session_detach",
            "daemon_pin_add",
            "daemon_pin_remove",
        ] {
            assert!(idl_source.contains(&format!(" {method}(")));
            let owner = function_body(service_source, method);
            assert!(owner.contains("daemon_unavailable"));
        }
    }

    #[test]
    fn mu_17g_f5_operational_leaves_have_one_execution_owner() {
        let cli_source = include_str!("cli.rs");
        let main_source = include_str!("main.rs");
        let context_source = include_str!("context_cmd.rs");
        let daemon_source = include_str!("daemon_cmd.rs");
        let idl_source = include_str!("../../../idl/loom.idl");
        let service_source = include_str!("../../loom-client/src/service.rs");
        let mut owned = std::collections::BTreeSet::new();

        assert_eq!(
            enum_variants(cli_source, "ContextCmd"),
            [
                "List", "Get", "Add", "Update", "Remove", "Test", "Use", "Current"
            ]
        );
        assert_eq!(
            enum_variants(cli_source, "DoctorCmd"),
            ["All", "Store", "Daemon", "Inference", "InferenceInstance"]
        );
        assert_eq!(
            enum_variants(cli_source, "LockCmd"),
            ["Acquire", "Refresh", "Release"]
        );
        assert_eq!(enum_variants(cli_source, "MountCmd"), ["Fuse", "Nfs"]);

        let run_lock = function_body(daemon_source, "run_lock");
        for (arm, method) in [
            ("LockCmd::Acquire", "lock_acquire"),
            ("LockCmd::Refresh", "lock_refresh"),
            ("LockCmd::Release", "lock_release"),
        ] {
            assert!(owned.insert(arm));
            let body = match_arm_segment(run_lock, arm, "LockCmd");
            assert!(body.contains("resume_cli_lock_session"));
            assert!(body.contains("CliGeneratedOperation::new"));
            assert!(body.contains(&format!("\"{method}\"")));
            assert!(idl_source.contains(&format!(" {method}(")));
            assert!(service_source.contains(&format!("fn {method}(")));
            for forbidden in [
                "lock_acquire_auth",
                "lock_refresh_auth",
                "lock_release_auth",
                "open_store_client",
                "open_cli_generated_client",
                "StoreClient::",
                "cli_open_loom(",
                "cli_open_loom_read(",
                "FileStore::open",
                "save_loom(",
            ] {
                assert!(!body.contains(forbidden), "{arm} contains {forbidden}");
            }
        }

        let context = function_body(context_source, "run_context");
        for (arm, owner) in [
            ("ContextCmd::List", "resolver"),
            ("ContextCmd::Get", "resolver"),
            ("ContextCmd::Add", "write_project_context_file"),
            ("ContextCmd::Update", "write_project_context_file"),
            ("ContextCmd::Remove", "write_project_context_file"),
            ("ContextCmd::Test", "resolve_context"),
            ("ContextCmd::Use", "write_project_context_file"),
            ("ContextCmd::Current", "current_context"),
        ] {
            assert!(owned.insert(arm));
            let body = match_arm_segment(context, arm, "ContextCmd");
            assert!(body.contains(owner), "{arm} must retain {owner}");
            for forbidden in [
                "open_store_client",
                "open_cli_generated_client",
                "cli_open_loom(",
                "FileStore::open",
                "save_loom(",
            ] {
                assert!(!body.contains(forbidden), "{arm} contains {forbidden}");
            }
        }
        let context_write = function_body(context_source, "write_project_context_file");
        assert!(context_write.contains("project_context_file"));
        assert!(context_write.contains("std::fs::write"));

        let doctor = function_body(main_source, "run_doctor");
        for (arm, owner) in [
            ("DoctorCmd::All", "run_doctor_all"),
            ("DoctorCmd::Store", "store_doctor"),
            ("DoctorCmd::Daemon", "daemon_doctor"),
            ("DoctorCmd::Inference", "collect_inference_doctor_report"),
            (
                "DoctorCmd::InferenceInstance",
                "run_inference_instance_doctor",
            ),
        ] {
            assert!(owned.insert(arm));
            let body = match_arm_segment(doctor, arm, "DoctorCmd");
            assert!(body.contains(owner), "{arm} must retain {owner}");
            assert!(!body.contains("save_loom("));
            assert!(!body.contains("open_cli_generated_client"));
        }

        let run = function_body(main_source, "run");
        for (arm, owner) in [
            ("Command::Capabilities", "run_capabilities"),
            ("Command::Llms", "print_llms_reference"),
            ("Command::Version", "VERSION"),
            ("Command::Mcp", "run_mcp"),
        ] {
            assert!(owned.insert(arm));
            let body = match_arm_segment(run, arm, "Command");
            assert!(body.contains(owner), "{arm} must retain {owner}");
            assert!(!body.contains("save_loom("));
        }
        assert!(match_arm_segment(run, "Command::Mount", "Command").contains("run_mount"));
        let capabilities = function_body(main_source, "run_capabilities");
        assert!(capabilities.contains("loom_core::capability::registry"));
        assert!(!capabilities.contains("FileStore"));
        let mcp = function_body(daemon_source, "run_mcp");
        assert!(mcp.contains("serve_http_with_network_access"));
        assert!(mcp.contains("serve_stdio"));
        assert!(!mcp.contains("save_loom("));
        let mount = function_body(main_source, "run_mount");
        for arm in ["MountCmd::Fuse", "MountCmd::Nfs"] {
            assert!(owned.insert(arm));
            let body = match_arm_segment(mount, arm, "MountCmd");
            assert!(body.contains("mount_"));
            assert!(!body.contains("save_loom("));
        }

        assert_eq!(owned.len(), 22);
    }

    #[test]
    fn mu_6i_b_data_family_mutations_use_generated_clients() {
        let source = include_str!("main.rs");
        for (runner, arm, interface, method) in [
            ("run_kv", "KvCmd::Put", "Kv", "put"),
            ("run_kv", "KvCmd::Delete", "Kv", "delete"),
            ("run_cas", "CasCmd::Put", "Cas", "put"),
            ("run_cas", "CasCmd::Delete", "Cas", "delete"),
            ("run_queue", "QueueCmd::Append", "Queue", "append"),
            (
                "run_queue",
                "QueueCmd::Advance",
                "QueueConsumers",
                "consumer_advance",
            ),
            (
                "run_queue",
                "QueueCmd::Reset",
                "QueueConsumers",
                "consumer_reset",
            ),
            ("run_ledger", "LedgerCmd::Append", "Ledger", "append"),
            ("run_time_series", "TimeSeriesCmd::Put", "TimeSeries", "put"),
            ("run_search", "SearchCmd::Create", "Search", "create"),
            ("run_search", "SearchCmd::Index", "Search", "index"),
            ("run_search", "SearchCmd::Delete", "Search", "delete"),
            ("run_search", "SearchCmd::Remap", "Search", "remap"),
            (
                "run_calendar",
                "CalendarCmd::CreateCollection",
                "Calendar",
                "create_collection",
            ),
            (
                "run_calendar",
                "CalendarCmd::DeleteCollection",
                "Calendar",
                "delete_collection",
            ),
            (
                "run_calendar",
                "CalendarCmd::DeleteEntry",
                "Calendar",
                "delete_entry",
            ),
            (
                "run_calendar",
                "CalendarCmd::PutEntry",
                "Calendar",
                "put_entry",
            ),
            ("run_calendar", "CalendarCmd::PutIcs", "Calendar", "put_ics"),
            (
                "run_contacts",
                "ContactsCmd::CreateBook",
                "Contacts",
                "create_book",
            ),
            (
                "run_contacts",
                "ContactsCmd::DeleteBook",
                "Contacts",
                "delete_book",
            ),
            (
                "run_contacts",
                "ContactsCmd::DeleteEntry",
                "Contacts",
                "delete_entry",
            ),
            (
                "run_contacts",
                "ContactsCmd::PutEntry",
                "Contacts",
                "put_entry",
            ),
            (
                "run_contacts",
                "ContactsCmd::PutVcard",
                "Contacts",
                "put_vcard",
            ),
            (
                "run_mail",
                "MailCmd::CreateMailbox",
                "Mail",
                "create_mailbox",
            ),
            (
                "run_mail",
                "MailCmd::DeleteMailbox",
                "Mail",
                "delete_mailbox",
            ),
            (
                "run_mail",
                "MailCmd::DeleteMessage",
                "Mail",
                "delete_message",
            ),
            (
                "run_mail",
                "MailCmd::IngestMessage",
                "Mail",
                "ingest_message",
            ),
            ("run_mail", "MailCmd::SetFlags", "Mail", "set_flags"),
            ("run_files", "FilesCmd::Write", "FileSystem", "write_file"),
            (
                "run_files",
                "FilesCmd::Mkdir",
                "FileSystem",
                "create_directory",
            ),
            ("run_files", "FilesCmd::Delete", "FileSystem", "remove_"),
        ] {
            let body = match_arm_body(function_body(source, runner), arm);
            assert!(
                body.contains("remote::open_cli_generated_client(&store, keys)?"),
                "{runner} {arm} must select the generated client boundary"
            );
            assert!(
                body.contains(&format!("\"{interface}\"")),
                "{runner} {arm} must dispatch through generated interface {interface}"
            );
            assert!(
                body.contains(&format!("\"{method}")),
                "{runner} {arm} must dispatch through generated method {method}"
            );
            assert!(
                !body.contains("remote::open_store_client("),
                "{runner} {arm} must not use legacy StoreClient routing"
            );
            assert!(
                !body.contains("cli_open_loom("),
                "{runner} {arm} must not open a writable Loom directly"
            );
            assert!(
                !body.contains("FileStore::open("),
                "{runner} {arm} must not acquire direct FileStore write ownership"
            );
            assert!(
                !body.contains("Loom::new("),
                "{runner} {arm} must not construct a direct local Loom"
            );
            assert!(
                !body.contains("save_loom("),
                "{runner} {arm} must not save outside the generated owner"
            );
        }
    }

    #[test]
    fn immutable_generated_cli_read_arms_use_read_only_open_helpers() {
        let source = include_str!("main.rs");
        for (runner, arm) in [
            ("run_document", "DocumentCmd::GetText"),
            ("run_document", "DocumentCmd::GetBinary"),
            ("run_document", "DocumentCmd::ListBinary"),
            ("run_document", "DocumentCmd::Find"),
            ("run_document", "DocumentCmd::Query"),
            ("run_document", "DocumentCmd::IndexList"),
            ("run_document", "DocumentCmd::IndexStatus"),
            ("run_files", "FilesCmd::Ls"),
            ("run_files", "FilesCmd::Read"),
            ("run_tickets", "TicketsCmd::ProjectSettingsGet"),
            ("run_tickets", "TicketsCmd::Projects"),
            ("run_tickets", "TicketsCmd::Relations"),
            ("run_tickets", "TicketsCmd::Fields"),
            ("run_tickets", "TicketsCmd::BoardGet"),
            ("run_tickets", "TicketsCmd::BoardList"),
            ("run_tickets", "TicketsCmd::Comments"),
            ("run_tickets", "TicketsCmd::List"),
            ("run_tickets", "TicketsCmd::Get"),
            ("run_tickets", "TicketsCmd::History"),
            ("run_lanes", "LanesCmd::Get"),
            ("run_lanes", "LanesCmd::List"),
            ("run_pages", "PagesCmd::SpaceList"),
            ("run_pages", "PagesCmd::SpaceGet"),
            ("run_pages", "PagesCmd::Get"),
            ("run_pages", "PagesCmd::History"),
            ("run_pages", "PagesCmd::StructureGet"),
            ("run_time_series", "TimeSeriesCmd::Latest"),
            ("run_metrics", "MetricsCmd::GetDescriptor"),
            ("run_metrics", "MetricsCmd::Query"),
            ("run_logs", "LogsCmd::GetRecord"),
            ("run_logs", "LogsCmd::Query"),
            ("run_traces", "TracesCmd::GetSpan"),
            ("run_traces", "TracesCmd::TraceSpans"),
            ("run_traces", "TracesCmd::Query"),
            ("run_program", "ProgramCmd::Inspect"),
            ("run_program", "ProgramCmd::Get"),
            ("run_program", "ProgramCmd::List"),
            ("run_dataframe", "DataframeCmd::Collect"),
            ("run_dataframe", "DataframeCmd::PlanDigest"),
            ("run_dataframe", "DataframeCmd::Preview"),
            ("run_dataframe", "DataframeCmd::SourceDigests"),
            ("run_vector", "VectorCmd::Get"),
            ("run_vector", "VectorCmd::Source"),
            ("run_vector", "VectorCmd::Ids"),
            ("run_vector", "VectorCmd::IndexKeys"),
            ("run_vector", "VectorCmd::Search"),
            ("run_graph", "GraphCmd::GetNode"),
            ("run_graph", "GraphCmd::GetEdge"),
            ("run_graph", "GraphCmd::Neighbors"),
            ("run_graph", "GraphCmd::OutEdges"),
            ("run_graph", "GraphCmd::InEdges"),
            ("run_graph", "GraphCmd::Reachable"),
            ("run_graph", "GraphCmd::ShortestPath"),
            ("run_graph", "GraphCmd::Query"),
            ("run_graph", "GraphCmd::ExplainQuery"),
            ("run_columnar", "ColumnarCmd::Scan"),
            ("run_columnar", "ColumnarCmd::Columns"),
            ("run_columnar", "ColumnarCmd::Rows"),
            ("run_columnar", "ColumnarCmd::Inspect"),
            ("run_columnar", "ColumnarCmd::SourceDigest"),
            ("run_columnar", "ColumnarCmd::Select"),
            ("run_columnar", "ColumnarCmd::Aggregate"),
            ("run_calendar", "CalendarCmd::GetCollection"),
            ("run_calendar", "CalendarCmd::GetEntry"),
            ("run_calendar", "CalendarCmd::ListCollections"),
            ("run_calendar", "CalendarCmd::ListEntries"),
            ("run_calendar", "CalendarCmd::Range"),
            ("run_calendar", "CalendarCmd::Search"),
            ("run_calendar", "CalendarCmd::ToIcs"),
            ("run_contacts", "ContactsCmd::GetBook"),
            ("run_contacts", "ContactsCmd::GetEntry"),
            ("run_contacts", "ContactsCmd::ListBooks"),
            ("run_contacts", "ContactsCmd::ListEntries"),
            ("run_contacts", "ContactsCmd::Search"),
            ("run_contacts", "ContactsCmd::ToVcard"),
            ("run_mail", "MailCmd::GetFlags"),
            ("run_mail", "MailCmd::GetMailbox"),
            ("run_mail", "MailCmd::GetMessage"),
            ("run_mail", "MailCmd::ListMailboxes"),
            ("run_mail", "MailCmd::ListMessages"),
            ("run_mail", "MailCmd::Search"),
            ("run_mail", "MailCmd::ToEml"),
            ("run_search", "SearchCmd::Get"),
            ("run_search", "SearchCmd::Ids"),
            ("run_search", "SearchCmd::Query"),
            ("run_search", "SearchCmd::Status"),
        ] {
            let body = match_arm_body(function_body(source, runner), arm);
            assert!(
                body.contains("remote::open_cli_read_only_generated_client"),
                "{arm} must use the read-only generated client"
            );
            for forbidden in [
                "remote::open_cli_generated_client(",
                "cli_open_loom(",
                "FileStore::open(",
            ] {
                assert!(
                    !body.contains(forbidden),
                    "{arm} must not use writable helper {forbidden}"
                );
            }
        }

        let cleanup = match_arm_body(function_body(source, "run_lanes"), "LanesCmd::Cleanup");
        assert!(
            cleanup.contains("if apply")
                && cleanup.contains("remote::open_cli_generated_client")
                && cleanup.contains("remote::open_cli_read_only_generated_client"),
            "LanesCmd::Cleanup must keep apply writable and dry-run read-only"
        );
    }

    #[test]
    fn mu_1e_immutable_read_routes_use_read_only_open_helpers() {
        let main_source = include_str!("main.rs");
        let remote_source = include_str!("remote.rs");
        let table_source = include_str!("table_cmd.rs");
        let unified = function_body(main_source, "run_unified_search");
        assert!(unified.contains("cli_open_loom_read(&args.store, keys)?"));
        assert!(!unified.contains("cli_open_loom(&args.store"));

        for arm in [
            "SearchCmd::Get",
            "SearchCmd::Ids",
            "SearchCmd::Query",
            "SearchCmd::Status",
        ] {
            let body = match_arm_body(function_body(main_source, "run_search"), arm);
            assert!(body.contains("remote::open_cli_read_only_generated_client"));
            for forbidden in [
                "remote::open_cli_generated_client(",
                "cli_open_loom(",
                "FileStore::open(",
            ] {
                assert!(!body.contains(forbidden), "{arm} found {forbidden}");
            }
        }

        for method in ["search_get", "search_ids", "search_query", "search_status"] {
            let body = function_body(remote_source, method);
            assert!(body.contains("cli_open_loom_read(locator, keys)?"));
            assert!(!body.contains("cli_open_loom(locator, keys)?"));
        }

        for arm in ["TableCmd::Blame", "TableCmd::Diff"] {
            let body = match_arm_body(function_body(table_source, "run_table"), arm);
            assert!(body.contains("cli_open_loom_read(&store, keys)?"));
            assert!(!body.contains("cli_open_loom(&store, keys)?"));
        }

        let catalog = match_arm_body(
            function_body(main_source, "run_studio_surfaces"),
            "StudioSurfacesCmd::Catalog",
        );
        for forbidden in [
            "remote::open_cli_generated_client(",
            "cli_open_loom(",
            "FileStore::open(",
        ] {
            assert!(
                !catalog.contains(forbidden),
                "Studio catalog found {forbidden}"
            );
        }

        let rebuild = match_arm_body(
            function_body(main_source, "run_studio_revisions"),
            "StudioRevisionsCmd::Rebuild",
        );
        assert!(rebuild.contains("remote::open_cli_generated_client(&store, keys)?"));
        assert!(rebuild.contains("\"StudioMaintenance\""));
        assert!(rebuild.contains("\"studio_revisions_rebuild_json\""));
        assert!(rebuild.contains("dry_run.to_value()"));
        assert!(!rebuild.contains("cli_open_loom"));
    }

    #[test]
    fn mu_1f_immutable_read_routes_use_read_only_open_helpers() {
        let main_source = include_str!("main.rs");

        for (runner, arm) in [
            ("run_meetings", "MeetingsCmd::List"),
            ("run_meetings", "MeetingsCmd::Get"),
            ("run_meetings", "MeetingsCmd::Search"),
            ("run_lifecycle", "LifecycleCmd::Definitions"),
            ("run_lifecycle", "LifecycleCmd::Definition"),
            ("run_lifecycle", "LifecycleCmd::Instances"),
            ("run_lifecycle", "LifecycleCmd::Instance"),
            ("run_lifecycle", "LifecycleCmd::SnapshotPlan"),
            ("run_lifecycle", "LifecycleCmd::CurrentSurface"),
            ("run_lifecycle", "LifecycleCmd::Snapshots"),
            ("run_lifecycle", "LifecycleCmd::Snapshot"),
            ("run_lifecycle", "LifecycleCmd::SnapshotContent"),
            ("run_lifecycle", "LifecycleCmd::OperationLog"),
            ("run_vector_text", "VectorTextCmd::Query"),
            ("run_columnar", "ColumnarCmd::ExportArrow"),
            ("run_columnar", "ColumnarCmd::ExportParquet"),
            ("run_interchange", "InterchangeCmd::ExportArchive"),
            ("run_interchange", "InterchangeCmd::ExportFs"),
            ("run_interchange", "InterchangeCmd::ExportTableCsv"),
            ("run_interchange", "InterchangeCmd::ExportCar"),
        ] {
            let body = match_arm_body(function_body(main_source, runner), arm);
            assert!(body.contains("cli_open_loom_read(&store, keys)?"));
        }

        let meetings_source = match_arm_segment(
            function_body(main_source, "run_meetings"),
            "MeetingsCmd::SourceRead",
            "MeetingsCmd",
        );
        assert!(meetings_source.contains("remote::open_cli_read_only_generated_client"));
        assert!(meetings_source.contains("\"Meetings\""));
        assert!(meetings_source.contains("\"meetings_source_read\""));
        assert!(!meetings_source.contains("cli_open_loom_read"));

        for (arm, method) in [
            ("DriveCmd::List", "drive_list_json"),
            ("DriveCmd::Stat", "drive_stat_json"),
            ("DriveCmd::Read", "drive_read_file"),
            ("DriveCmd::ListVersions", "drive_list_versions_json"),
            ("DriveCmd::ListConflicts", "drive_list_conflicts_json"),
            ("DriveCmd::ListShares", "drive_list_shares_json"),
            ("DriveCmd::ListRetention", "drive_list_retention_json"),
        ] {
            let body = match_arm_segment(function_body(main_source, "run_drive"), arm, "DriveCmd");
            assert!(body.contains("remote::open_cli_read_only_generated_client"));
            assert!(body.contains("\"Drive\""));
            assert!(body.contains(&format!("\"{method}\"")));
            for forbidden in ["open_drive_read", "cli_open_loom_read", "open_store_client"] {
                assert!(!body.contains(forbidden), "{arm} found {forbidden}");
            }
        }
        assert!(!main_source.contains("fn open_drive_read("));
    }

    #[test]
    fn mu_6i_c4_interchange_mutations_use_generated_clients() {
        let main_source = include_str!("main.rs");
        let run_interchange = function_body(main_source, "run_interchange");

        let arms = [
            ("InterchangeCmd::ImportFs", "FileSystem", "import_fs"),
            ("InterchangeCmd::ImportArchive", "Archive", "archive_import"),
            (
                "InterchangeCmd::ImportTableCsv",
                "InterchangeProfiles",
                "import_table_csv",
            ),
            ("InterchangeCmd::ImportCar", "Car", "car_import"),
        ];
        for (arm, interface, method) in arms {
            let body = match_arm_body(run_interchange, arm);
            assert!(body.contains("remote::open_cli_generated_client(&store, keys)?"));
            assert!(body.contains("execute_generated_bytes("));
            assert!(
                body.contains(&format!("\"{interface}\"")),
                "{arm} must use generated {interface}"
            );
            assert!(
                body.contains(&format!("\"{method}\"")),
                "{arm} must use generated {interface}.{method}"
            );
            assert_interchange_generated_body_has_no_local_mutation_owner(arm, body);
        }

        let import_fs = match_arm_body(run_interchange, "InterchangeCmd::ImportFs");
        assert!(import_fs.contains("remote::target_is_remote(&store)?"));
        let import_archive = match_arm_body(run_interchange, "InterchangeCmd::ImportArchive");
        assert!(import_archive.contains("remote::target_is_remote(&store)?"));
        let import_table_csv = match_arm_body(run_interchange, "InterchangeCmd::ImportTableCsv");
        assert!(import_table_csv.contains("std::fs::read(&csv)"));
        assert!(import_table_csv.contains("WireValue::Bytes(payload)"));

        for (arm, runner) in [
            ("InterchangeCmd::ImportRedmine", "run_redmine_import("),
            ("InterchangeCmd::ImportAsana", "run_asana_import("),
            ("InterchangeCmd::ImportJira", "run_jira_import("),
            ("InterchangeCmd::ImportConfluence", "run_confluence_import("),
            ("InterchangeCmd::ImportSlack", "run_slack_import("),
            ("InterchangeCmd::ImportDrive", "run_drive_import("),
            ("InterchangeCmd::ImportMarkdown", "run_markdown_import("),
            ("InterchangeCmd::ImportNotion", "run_notion_import("),
        ] {
            assert!(run_interchange.contains(arm));
            assert!(run_interchange.contains(runner));
        }

        for (runner, method, payload_loader) in [
            (
                "run_redmine_import",
                "import_redmine",
                "std::fs::read(snapshot)",
            ),
            (
                "run_asana_import",
                "import_asana",
                "std::fs::read(snapshot)",
            ),
            ("run_jira_import", "import_jira", "std::fs::read(snapshot)"),
            (
                "run_confluence_import",
                "import_confluence",
                "std::fs::read(snapshot)",
            ),
            (
                "run_slack_import",
                "import_slack",
                "std::fs::read(snapshot)",
            ),
            (
                "run_drive_import",
                "import_drive",
                "std::fs::read(snapshot)",
            ),
            (
                "run_markdown_import",
                "import_markdown",
                "markdown_import_archive(src)?",
            ),
            (
                "run_notion_import",
                "import_notion",
                "std::fs::read(snapshot)",
            ),
        ] {
            let body = function_body(main_source, runner);
            assert!(
                body.contains(payload_loader),
                "{runner} must preserve host byte loading"
            );
            assert!(body.contains("remote::open_cli_generated_client(store, keys)?"));
            assert!(body.contains("execute_generated_bytes("));
            assert!(body.contains("\"InterchangeProfiles\""));
            assert!(body.contains(&format!("\"{method}\"")));
            assert!(body.contains("WireValue::Bytes(payload)"));
            assert!(body.contains("generated_import_report_from_cbor(&encoded)?"));
            assert!(body.contains("print_import_report(&report, format)"));
            assert_interchange_generated_body_has_no_local_mutation_owner(runner, body);
        }

        assert!(!main_source.contains("open_profile_import_input"));
        assert!(!main_source.contains("file_import_bytes"));
        assert!(!main_source.contains("persist_profile_import_artifacts"));
        assert!(!main_source.contains("CliImportLoom"));
    }

    fn assert_interchange_generated_body_has_no_local_mutation_owner(name: &str, body: &str) {
        for forbidden in [
            "cli_open_loom(",
            "cli_open_store_for_write",
            "FileStore::open",
            "save_loom(",
            "CliImportLoom",
            "import_fs(loom",
            "import_archive(loom",
            "import_table_csv(loom",
            "import_car(loom",
            "import_redmine_bytes",
            "import_asana_bytes",
            "import_jira_bytes",
            "import_confluence_bytes",
            "import_slack_bytes",
            "import_drive_bytes",
            "import_markdown_path",
            "import_notion_bytes",
            "persist_profile_import_artifacts",
            "retain_import_input",
            "persist_import_checkpoint",
        ] {
            assert!(
                !body.contains(forbidden),
                "{name} must not contain {forbidden}"
            );
        }
    }

    #[test]
    fn mu_1g_inference_instance_reads_use_read_only_open_helpers() {
        let main_source = include_str!("main.rs");
        let run_inference_instance = function_body(main_source, "run_inference_instance");

        for (arm, method) in [
            ("InferenceInstanceCmd::List", "inference_instance_list_json"),
            ("InferenceInstanceCmd::Show", "inference_instance_get_json"),
        ] {
            let body = match_arm_body(run_inference_instance, arm);
            assert!(body.contains("remote::open_cli_read_only_generated_client(&store, keys)?"));
            assert!(body.contains("\"InferenceInstance\""));
            assert!(body.contains(&format!("\"{method}\"")));
            assert!(!body.contains("cli_open_loom"));
        }

        for arm in [
            "InferenceInstanceCmd::Create",
            "InferenceInstanceCmd::Update",
            "InferenceInstanceCmd::Delete",
        ] {
            let body = match_arm_body(run_inference_instance, arm);
            assert!(body.contains("remote::open_cli_generated_client(&store, keys)?"));
            assert!(!body.contains("cli_open_loom(&store, keys)?"));
        }

        let doctor = function_body(main_source, "run_inference_instance_doctor");
        assert!(doctor.contains("cli_open_loom_read(store, keys)?"));
        assert!(!doctor.contains("cli_open_loom(store, keys)?"));
    }

    #[test]
    fn mu_1i_reviewed_immutable_read_routes_are_enforced() {
        let main_source = include_str!("main.rs");
        let remote_source = include_str!("remote.rs");
        let context_source = include_str!("context_cmd.rs");
        let management_source = include_str!("management_cmd.rs");
        let exec_source = include_str!("exec_cmd.rs");

        for (runner, arm) in [
            ("run_meetings", "MeetingsCmd::List"),
            ("run_meetings", "MeetingsCmd::Get"),
            ("run_meetings", "MeetingsCmd::Search"),
            ("run_lifecycle", "LifecycleCmd::Definitions"),
            ("run_lifecycle", "LifecycleCmd::Definition"),
            ("run_lifecycle", "LifecycleCmd::Instances"),
            ("run_lifecycle", "LifecycleCmd::Instance"),
            ("run_lifecycle", "LifecycleCmd::SnapshotPlan"),
            ("run_lifecycle", "LifecycleCmd::CurrentSurface"),
            ("run_lifecycle", "LifecycleCmd::Snapshots"),
            ("run_lifecycle", "LifecycleCmd::Snapshot"),
            ("run_lifecycle", "LifecycleCmd::SnapshotContent"),
            ("run_lifecycle", "LifecycleCmd::OperationLog"),
            ("run_vector_text", "VectorTextCmd::Query"),
            ("run_columnar", "ColumnarCmd::ExportArrow"),
            ("run_columnar", "ColumnarCmd::ExportParquet"),
            ("run_interchange", "InterchangeCmd::ExportArchive"),
            ("run_interchange", "InterchangeCmd::ExportFs"),
            ("run_interchange", "InterchangeCmd::ExportTableCsv"),
            ("run_interchange", "InterchangeCmd::ExportCar"),
        ] {
            let body = match_arm_body(function_body(main_source, runner), arm);
            assert!(body.contains("cli_open_loom_read(&store, keys)?"));
            assert!(!body.contains("cli_open_loom(&store, keys)?"));
        }

        let meetings_source = match_arm_segment(
            function_body(main_source, "run_meetings"),
            "MeetingsCmd::SourceRead",
            "MeetingsCmd",
        );
        assert!(meetings_source.contains("remote::open_cli_read_only_generated_client"));
        assert!(meetings_source.contains("\"Meetings\""));
        assert!(meetings_source.contains("\"meetings_source_read\""));
        for forbidden in ["cli_open_loom_read", "open_store_client", "FileStore::open"] {
            assert!(!meetings_source.contains(forbidden));
        }

        for (arm, method) in [
            ("ChatCmd::Channels", "chat_list_channels_json"),
            ("ChatCmd::Messages", "chat_messages_json"),
            ("ChatCmd::Events", "chat_fetch_events_json"),
            ("ChatCmd::Cursor", "chat_cursor_json"),
            ("ChatCmd::EmojiList", "chat_emoji_list_json"),
        ] {
            let body = match_arm_segment(function_body(main_source, "run_chat"), arm, "ChatCmd");
            assert!(body.contains("remote::open_cli_read_only_generated_client(&store, keys)?"));
            assert!(body.contains("\"Chat\""));
            assert!(
                body.contains(&format!("\"{method}\"")),
                "{arm} missing {method}"
            );
            for forbidden in ["cli_open_loom_read", "open_store_client", "FileStore::open"] {
                assert!(!body.contains(forbidden), "{arm} found {forbidden}");
            }
        }

        for (runner, arm, interface, method) in [
            (
                "run_calendar",
                "CalendarCmd::GetCollection",
                "Calendar",
                "get_collection",
            ),
            (
                "run_calendar",
                "CalendarCmd::GetEntry",
                "Calendar",
                "get_entry",
            ),
            (
                "run_calendar",
                "CalendarCmd::ListCollections",
                "Calendar",
                "list_collections",
            ),
            (
                "run_calendar",
                "CalendarCmd::ListEntries",
                "Calendar",
                "list_entries",
            ),
            ("run_calendar", "CalendarCmd::Range", "Calendar", "range"),
            ("run_calendar", "CalendarCmd::Search", "Calendar", "search"),
            ("run_calendar", "CalendarCmd::ToIcs", "Calendar", "to_ics"),
            (
                "run_contacts",
                "ContactsCmd::GetBook",
                "Contacts",
                "get_book",
            ),
            (
                "run_contacts",
                "ContactsCmd::GetEntry",
                "Contacts",
                "get_entry",
            ),
            (
                "run_contacts",
                "ContactsCmd::ListBooks",
                "Contacts",
                "list_books",
            ),
            (
                "run_contacts",
                "ContactsCmd::ListEntries",
                "Contacts",
                "list_entries",
            ),
            ("run_contacts", "ContactsCmd::Search", "Contacts", "search"),
            (
                "run_contacts",
                "ContactsCmd::ToVcard",
                "Contacts",
                "to_vcard",
            ),
            ("run_mail", "MailCmd::GetFlags", "Mail", "get_flags"),
            ("run_mail", "MailCmd::GetMailbox", "Mail", "get_mailbox"),
            ("run_mail", "MailCmd::GetMessage", "Mail", "get_message"),
            (
                "run_mail",
                "MailCmd::ListMailboxes",
                "Mail",
                "list_mailboxes",
            ),
            ("run_mail", "MailCmd::ListMessages", "Mail", "list_messages"),
            ("run_mail", "MailCmd::Search", "Mail", "search"),
            ("run_mail", "MailCmd::ToEml", "Mail", "to_eml"),
        ] {
            let enum_name = arm.split_once("::").expect("qualified CLI arm").0;
            let body = match_arm_segment(function_body(main_source, runner), arm, enum_name);
            assert!(body.contains("remote::open_cli_read_only_generated_client(&store, keys)?"));
            assert!(
                body.contains(&format!("\"{interface}\"")),
                "{arm} must use generated {interface}"
            );
            assert!(
                body.contains(&format!("\"{method}\"")),
                "{arm} must use generated {interface}.{method}"
            );
            for forbidden in [
                "remote::open_store_client",
                "cli_open_loom(",
                "FileStore::open(",
            ] {
                assert!(!body.contains(forbidden), "{arm} found {forbidden}");
            }
        }

        for (runner, arm, interface, method) in [
            ("run_vector", "VectorCmd::Get", "Vector", "get"),
            ("run_vector", "VectorCmd::Source", "Vector", "source_text"),
            ("run_vector", "VectorCmd::Ids", "Vector", "ids"),
            (
                "run_vector",
                "VectorCmd::IndexKeys",
                "Vector",
                "metadata_index_keys",
            ),
            ("run_vector", "VectorCmd::Search", "Vector", "search_policy"),
            ("run_graph", "GraphCmd::GetNode", "Graph", "get_node"),
            ("run_graph", "GraphCmd::GetEdge", "Graph", "get_edge"),
            ("run_graph", "GraphCmd::Neighbors", "Graph", "neighbors"),
            ("run_graph", "GraphCmd::OutEdges", "Graph", "out_edges"),
            ("run_graph", "GraphCmd::InEdges", "Graph", "in_edges"),
            ("run_graph", "GraphCmd::Reachable", "Graph", "reachable"),
            (
                "run_graph",
                "GraphCmd::ShortestPath",
                "Graph",
                "shortest_path",
            ),
            ("run_graph", "GraphCmd::Query", "Graph", "query"),
            (
                "run_graph",
                "GraphCmd::ExplainQuery",
                "Graph",
                "explain_query",
            ),
            ("run_columnar", "ColumnarCmd::Scan", "Columnar", "scan"),
            (
                "run_columnar",
                "ColumnarCmd::Columns",
                "Columnar",
                "columns",
            ),
            ("run_columnar", "ColumnarCmd::Rows", "Columnar", "rows"),
            (
                "run_columnar",
                "ColumnarCmd::Inspect",
                "Columnar",
                "inspect",
            ),
            (
                "run_columnar",
                "ColumnarCmd::SourceDigest",
                "Columnar",
                "source_digest",
            ),
            ("run_columnar", "ColumnarCmd::Select", "Columnar", "select"),
            (
                "run_columnar",
                "ColumnarCmd::Aggregate",
                "Columnar",
                "aggregate",
            ),
            ("run_vcs", "VcsCmd::Diff", "VersionControl", "diff"),
            ("run_vcs", "VcsCmd::Log", "VersionControl", "log"),
        ] {
            let enum_name = arm.split_once("::").expect("qualified CLI arm").0;
            let body = match_arm_segment(function_body(main_source, runner), arm, enum_name);
            assert!(body.contains("remote::open_cli_read_only_generated_client(&store, keys)?"));
            assert!(
                body.contains(&format!("\"{interface}\"")),
                "{arm} must use generated {interface}"
            );
            assert!(
                body.contains(&format!("\"{method}\"")),
                "{arm} must use generated {interface}.{method}"
            );
            for forbidden in [
                "remote::open_store_client",
                "cli_open_loom(",
                "FileStore::open(",
            ] {
                assert!(!body.contains(forbidden), "{arm} found {forbidden}");
            }
        }

        for (arm, required) in [
            ("ContextCmd::List", "locator_cx::current().resolver()?"),
            ("ContextCmd::Get", "locator_cx::current().resolver()?"),
            ("ContextCmd::Test", "locator_cx::current().resolver()?"),
            ("ContextCmd::Current", "locator_cx::current().resolver()?"),
        ] {
            let body = match_arm_body(function_body(context_source, "run_context"), arm);
            assert!(body.contains(required));
            assert!(!body.contains("cli_open_loom("));
            assert!(!body.contains("FileStore::open("));
        }

        let inspect = match_arm_body(
            function_body(exec_source, "run_exec_cmd"),
            "ExecCmd::Inspect",
        );
        assert!(inspect.contains("std::fs::read(&request)"));
        assert!(!inspect.contains("cli_open_loom("));
        assert!(!inspect.contains("save_loom("));

        let capabilities = function_body(main_source, "run_capabilities");
        assert!(!capabilities.contains("cli_open_loom("));
        assert!(!capabilities.contains("FileStore::open("));
        for arm in ["Command::Llms", "Command::Version"] {
            let body = match_arm_body(function_body(main_source, "run"), arm);
            assert!(!body.contains("cli_open_loom("));
            assert!(!body.contains("FileStore::open("));
        }

        for (runner, arm) in [
            ("run_identity", "IdentityCmd::List"),
            ("run_identity_public_key", "IdentityPublicKeyCmd::List"),
            ("run_acl", "AclCmd::List"),
            ("run_protected_ref", "ProtectedRefCmd::List"),
            ("run_protected_ref", "ProtectedRefCmd::Get"),
        ] {
            let body = match_arm_body(function_body(management_source, runner), arm);
            assert!(
                body.contains("crate::remote::open_cli_read_only_generated_client(&store, keys)?")
            );
            assert!(!body.contains("crate::remote::open_store_client"));
            assert!(!body.contains("cli_open_loom("));
            assert!(!body.contains("FileStore::open("));
        }

        for (runner, arm, enum_name, interface, method) in [
            (
                "run_identity",
                "IdentityCmd::AuthorityWitness",
                "IdentityCmd",
                "Identity",
                "identity_authority_witness",
            ),
            (
                "run_identity",
                "IdentityCmd::ListAuthorityReplication",
                "IdentityCmd",
                "Identity",
                "identity_list_authority_replication",
            ),
            (
                "run_management_kv_config",
                "ManagementKvConfigCmd::Get",
                "ManagementKvConfigCmd",
                "ManagementKv",
                "get_config",
            ),
        ] {
            let body = match_arm_segment(function_body(management_source, runner), arm, enum_name);
            assert!(body.contains("open_cli_read_only_generated_client(&store, keys)?"));
            assert!(body.contains(&format!("\"{interface}\"")));
            assert!(body.contains(&format!("\"{method}\"")));
            assert!(!body.contains("open_store_client"));
            assert!(!body.contains("cli_open_loom("));
            assert!(!body.contains("FileStore::open("));
        }

        let workspace_list = match_arm_body(
            function_body(management_source, "run_management_workspace"),
            "WorkspaceCmd::List",
        );
        assert!(workspace_list.contains("open_cli_read_only_generated_client(&store, keys)?"));
        assert!(workspace_list.contains("client.workspace_list()?"));
        let workspace_owner = function_body(remote_source, "workspace_list");
        assert!(workspace_owner.contains("\"Workspaces\""));
        assert!(workspace_owner.contains("\"workspace_list\""));

        let identity_owner = function_body(management_source, "generated_identity_snapshot");
        assert!(identity_owner.contains("\"Identity\""));
        assert!(identity_owner.contains("\"identity_list\""));
        for (runner, arm) in [
            ("run_identity", "IdentityCmd::List"),
            ("run_identity_public_key", "IdentityPublicKeyCmd::List"),
        ] {
            let body = match_arm_body(function_body(management_source, runner), arm);
            assert!(body.contains("open_cli_read_only_generated_client(&store, keys)?"));
            assert!(body.contains("generated_identity_snapshot(&client)?"));
        }

        for (runner, arm, interface, method) in [
            ("run_acl", "AclCmd::List", "Acl", "acl_list"),
            (
                "run_protected_ref",
                "ProtectedRefCmd::List",
                "ProtectedRefs",
                "protected_ref_list",
            ),
            (
                "run_protected_ref",
                "ProtectedRefCmd::Get",
                "ProtectedRefs",
                "protected_ref_get",
            ),
        ] {
            let body = match_arm_body(function_body(management_source, runner), arm);
            assert!(body.contains("open_cli_read_only_generated_client(&store, keys)?"));
            assert!(body.contains(&format!("\"{interface}\"")));
            assert!(body.contains(&format!("\"{method}\"")));
            assert!(!body.contains("open_store_client"));
        }

        for (runner, arm) in [
            ("run_inference", "InferenceCmd::List"),
            ("run_inference", "InferenceCmd::Status"),
            ("run_inference", "InferenceCmd::Show"),
            ("run_inference_model", "InferenceModelCmd::List"),
            ("run_inference_model", "InferenceModelCmd::Show"),
            ("run_inference_model", "InferenceModelCmd::Status"),
        ] {
            let body = match_arm_body(function_body(main_source, runner), arm);
            assert!(!body.contains("cli_open_loom("));
            assert!(!body.contains("FileStore::open("));
        }

        for (runner, arm) in [
            ("run_inference", "InferenceCmd::Remove"),
            ("run_inference_model", "InferenceModelCmd::Remove"),
        ] {
            let body = match_arm_body(function_body(main_source, runner), arm);
            assert!(body.contains("dry_run"));
            assert!(!body.contains("cli_open_loom("));
            assert!(!body.contains("FileStore::open("));
        }

        for arm in ["InferenceInstanceCmd::List", "InferenceInstanceCmd::Show"] {
            let body = match_arm_body(function_body(main_source, "run_inference_instance"), arm);
            assert!(body.contains("remote::open_cli_read_only_generated_client(&store, keys)?"));
            assert!(!body.contains("cli_open_loom"));
        }

        let doctor_instance = function_body(main_source, "run_inference_instance_doctor");
        assert!(doctor_instance.contains("cli_open_loom_read(store, keys)?"));
        assert!(!doctor_instance.contains("cli_open_loom(store, keys)?"));

        for arm in [
            "StoreCmd::BundleExport",
            "StoreCmd::Get",
            "StoreCmd::Hash",
            "StoreCmd::Stat",
            "StoreCmd::Attribution",
            "StoreCmd::PreflightReplacement",
        ] {
            let body = match_arm_body(function_body(main_source, "run_store"), arm);
            assert!(!body.contains("cli_open_loom(&store, keys)?"));
            assert!(!body.contains("FileStore::open(&store)"));
        }
        assert!(
            match_arm_body(
                function_body(main_source, "run_store"),
                "StoreCmd::BundleExport"
            )
            .contains("cli_open_loom_read(&store, keys)?")
        );
        assert!(
            match_arm_body(function_body(main_source, "run_store"), "StoreCmd::Get")
                .contains("FileStore::open_read(&store)")
        );
        let stat = match_arm_body(function_body(main_source, "run_store"), "StoreCmd::Stat");
        assert!(stat.contains("open_cli_execution_context(&store)?"));
        assert!(stat.contains("generated_store_stat_json(context, keys)?"));
        assert!(!stat.contains("FileStore::open_read(&store)"));
        assert!(
            function_body(main_source, "run_store_attribution")
                .contains("cli_open_loom_read(store, keys)?")
        );
        assert!(
            function_body(main_source, "build_store_replacement_preflight_report")
                .contains("FileStore::open_read(store)")
        );
        assert!(
            function_body(main_source, "build_store_replacement_preflight_report")
                .contains("cli_open_loom_read(store, keys)")
        );

        for (arm, read_helper, write_helper) in [
            (
                "StoreCmd::Copy",
                "cli_open_loom_read(&src, keys)?",
                "cli_open_loom(&dst, keys)?",
            ),
            (
                "LanesCmd::Cleanup",
                "remote::open_cli_read_only_generated_client",
                "remote::open_cli_generated_client",
            ),
        ] {
            let runner = match arm {
                "LanesCmd::Cleanup" => "run_lanes",
                _ => "run_store",
            };
            let body = match_arm_body(function_body(main_source, runner), arm);
            assert!(body.contains(read_helper));
            assert!(body.contains(write_helper));
        }
        let policy = match_arm_body(function_body(main_source, "run_store"), "StoreCmd::Policy");
        assert!(policy.contains("remote::open_cli_generated_client(&store, keys)?"));
        assert!(policy.contains("remote::open_cli_read_only_generated_client(&store, keys)?"));
        assert!(policy.contains("\"StoreAdmin\""));
        assert!(policy.contains("\"store_policy_get\""));
        assert!(policy.contains("\"store_policy_set\""));
        assert!(policy.contains("store_policy_update_to_cbor(&update)"));
        assert!(!policy.contains("remote::open_store_client"));
        assert!(!policy.contains("cli_open_store_for_write(&store)?"));
        assert!(!policy.contains("FileStore::open_read(&store)"));
        assert!(!policy.contains("durability updates are not available"));
        let rekey = match_arm_body(function_body(main_source, "run_store"), "StoreCmd::Rekey");
        assert!(rekey.contains("remote::open_cli_generated_client(&store, keys)?"));
        assert!(rekey.contains("\"StoreAdmin\""));
        assert!(rekey.contains("\"store_rekey\""));
        assert!(rekey.contains("store_rekey_request_to_cbor(&request)"));
        assert!(!rekey.contains("remote::open_store_client"));
        assert!(!rekey.contains("FileStore::open(&store)"));
        assert!(!rekey.contains("raw KEK rekey has no generated contract"));
        let run_store = function_body(main_source, "run_store");
        assert!(run_store.contains("StoreCmd::Replace"));
        assert!(run_store.contains("=> run_store_replacement_activation("));
        let replace_activation = function_body(main_source, "run_store_replacement_activation");
        assert!(replace_activation.contains("if dry_run"));
        assert!(replace_activation.contains("std::fs::copy(active_path, backup_path)"));
        assert!(replace_activation.contains("std::fs::rename(&temp_store, active_path)"));
    }

    #[test]
    fn mu_6b_existing_interface_mutations_use_generated_client() {
        let main_source = include_str!("main.rs");

        for (runner, arm, interface, method) in [
            ("run_cas", "CasCmd::Delete", "Cas", "delete"),
            ("run_cas", "CasCmd::Put", "Cas", "put"),
            ("run_kv", "KvCmd::Delete", "Kv", "delete"),
            ("run_kv", "KvCmd::Put", "Kv", "put"),
            ("run_queue", "QueueCmd::Append", "Queue", "append"),
            (
                "run_queue",
                "QueueCmd::Advance",
                "QueueConsumers",
                "consumer_advance",
            ),
            (
                "run_queue",
                "QueueCmd::Reset",
                "QueueConsumers",
                "consumer_reset",
            ),
            ("run_time_series", "TimeSeriesCmd::Put", "TimeSeries", "put"),
            ("run_ledger", "LedgerCmd::Append", "Ledger", "append"),
        ] {
            let body = match_arm_body(function_body(main_source, runner), arm);
            assert!(
                body.contains("remote::open_cli_generated_client"),
                "{arm} must use the generated client"
            );
            assert!(
                body.contains(&format!("\"{interface}\"")),
                "{arm} must dispatch through generated interface {interface}"
            );
            assert!(
                body.contains(&format!("\"{method}\"")),
                "{arm} must dispatch through generated method {method}"
            );
            assert!(
                !body.contains("remote::open_store_client"),
                "{arm} must not bypass generated dispatch through StoreClient"
            );
            assert!(
                !body.contains("cli_open_loom("),
                "{arm} must not open a writable Loom directly"
            );
            assert!(
                !body.contains("FileStore::open("),
                "{arm} must not open a writable FileStore directly"
            );
        }
    }

    #[test]
    fn mu_17g_a_foundational_data_cli_leaves_use_generated_clients() {
        let main_source = include_str!("main.rs");

        for (runner, arm, helper, interface, method) in [
            (
                "run_cas",
                "CasCmd::Delete",
                "remote::open_cli_generated_client(&store, keys)?",
                "Cas",
                "delete",
            ),
            (
                "run_cas",
                "CasCmd::Get",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Cas",
                "get",
            ),
            (
                "run_cas",
                "CasCmd::Has",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Cas",
                "has",
            ),
            (
                "run_cas",
                "CasCmd::List",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Cas",
                "list",
            ),
            (
                "run_cas",
                "CasCmd::Put",
                "remote::open_cli_generated_client(&store, keys)?",
                "Cas",
                "put",
            ),
            (
                "run_kv",
                "KvCmd::Delete",
                "remote::open_cli_generated_client(&store, keys)?",
                "Kv",
                "delete",
            ),
            (
                "run_kv",
                "KvCmd::Get",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Kv",
                "get",
            ),
            (
                "run_kv",
                "KvCmd::List",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Kv",
                "list",
            ),
            (
                "run_kv",
                "KvCmd::Put",
                "remote::open_cli_generated_client(&store, keys)?",
                "Kv",
                "put",
            ),
            (
                "run_kv",
                "KvCmd::Range",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Kv",
                "range",
            ),
            (
                "run_queue",
                "QueueCmd::Append",
                "remote::open_cli_generated_client(&store, keys)?",
                "Queue",
                "append",
            ),
            (
                "run_queue",
                "QueueCmd::Advance",
                "remote::open_cli_generated_client(&store, keys)?",
                "QueueConsumers",
                "consumer_advance",
            ),
            (
                "run_queue",
                "QueueCmd::Get",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Queue",
                "get",
            ),
            (
                "run_queue",
                "QueueCmd::Len",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Queue",
                "len",
            ),
            (
                "run_queue",
                "QueueCmd::Position",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "QueueConsumers",
                "consumer_position",
            ),
            (
                "run_queue",
                "QueueCmd::Range",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Queue",
                "range",
            ),
            (
                "run_queue",
                "QueueCmd::Read",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "QueueConsumers",
                "consumer_read",
            ),
            (
                "run_queue",
                "QueueCmd::Reset",
                "remote::open_cli_generated_client(&store, keys)?",
                "QueueConsumers",
                "consumer_reset",
            ),
            (
                "run_time_series",
                "TimeSeriesCmd::Get",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "TimeSeries",
                "get",
            ),
            (
                "run_time_series",
                "TimeSeriesCmd::Latest",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "TimeSeries",
                "latest",
            ),
            (
                "run_time_series",
                "TimeSeriesCmd::Put",
                "remote::open_cli_generated_client(&store, keys)?",
                "TimeSeries",
                "put",
            ),
            (
                "run_time_series",
                "TimeSeriesCmd::Range",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "TimeSeries",
                "range",
            ),
            (
                "run_ledger",
                "LedgerCmd::Append",
                "remote::open_cli_generated_client(&store, keys)?",
                "Ledger",
                "append",
            ),
            (
                "run_ledger",
                "LedgerCmd::Get",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Ledger",
                "get",
            ),
            (
                "run_ledger",
                "LedgerCmd::Head",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Ledger",
                "head",
            ),
            (
                "run_ledger",
                "LedgerCmd::Len",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Ledger",
                "len",
            ),
            (
                "run_ledger",
                "LedgerCmd::Verify",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Ledger",
                "verify",
            ),
        ] {
            let body = match_arm_body(function_body(main_source, runner), arm);
            assert!(body.contains(helper), "{arm} must use {helper}");
            assert!(
                body.contains(&format!("\"{interface}\"")),
                "{arm} must dispatch through generated interface {interface}"
            );
            assert!(
                body.contains(&format!("\"{method}\"")),
                "{arm} must dispatch through generated method {method}"
            );
            assert!(
                !body.contains("remote::open_store_client"),
                "{arm} must not bypass generated dispatch through StoreClient"
            );
            assert!(
                !body.contains("cli_open_loom("),
                "{arm} must not open a writable Loom directly"
            );
            assert!(
                !body.contains("FileStore::open("),
                "{arm} must not open a writable FileStore directly"
            );
        }
    }

    #[test]
    fn mu_17g_b_analytical_data_cli_leaves_use_generated_clients() {
        let main_source = include_str!("main.rs");

        for (runner, arm, helper, interface, method) in [
            (
                "run_graph",
                "GraphCmd::UpsertNode",
                "remote::open_cli_generated_client(&store, keys)?",
                "Graph",
                "upsert_node",
            ),
            (
                "run_graph",
                "GraphCmd::GetNode",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Graph",
                "get_node",
            ),
            (
                "run_graph",
                "GraphCmd::RemoveNode",
                "remote::open_cli_generated_client(&store, keys)?",
                "Graph",
                "remove_node",
            ),
            (
                "run_graph",
                "GraphCmd::UpsertEdge",
                "remote::open_cli_generated_client(&store, keys)?",
                "Graph",
                "upsert_edge",
            ),
            (
                "run_graph",
                "GraphCmd::GetEdge",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Graph",
                "get_edge",
            ),
            (
                "run_graph",
                "GraphCmd::RemoveEdge",
                "remote::open_cli_generated_client(&store, keys)?",
                "Graph",
                "remove_edge",
            ),
            (
                "run_graph",
                "GraphCmd::Neighbors",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Graph",
                "neighbors",
            ),
            (
                "run_graph",
                "GraphCmd::OutEdges",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Graph",
                "out_edges",
            ),
            (
                "run_graph",
                "GraphCmd::InEdges",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Graph",
                "in_edges",
            ),
            (
                "run_graph",
                "GraphCmd::Reachable",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Graph",
                "reachable",
            ),
            (
                "run_graph",
                "GraphCmd::ShortestPath",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Graph",
                "shortest_path",
            ),
            (
                "run_graph",
                "GraphCmd::Query",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Graph",
                "query",
            ),
            (
                "run_graph",
                "GraphCmd::ExplainQuery",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Graph",
                "explain_query",
            ),
            (
                "run_vector",
                "VectorCmd::Create",
                "remote::open_cli_generated_client(&store, keys)?",
                "Vector",
                "create",
            ),
            (
                "run_vector",
                "VectorCmd::Upsert",
                "remote::open_cli_generated_client(&store, keys)?",
                "Vector",
                "upsert",
            ),
            (
                "run_vector",
                "VectorCmd::UpsertSource",
                "remote::open_cli_generated_client(&store, keys)?",
                "Vector",
                "upsert_source",
            ),
            (
                "run_vector",
                "VectorCmd::Get",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Vector",
                "get",
            ),
            (
                "run_vector",
                "VectorCmd::Source",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Vector",
                "source_text",
            ),
            (
                "run_vector",
                "VectorCmd::Ids",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Vector",
                "ids",
            ),
            (
                "run_vector",
                "VectorCmd::IndexKeys",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Vector",
                "metadata_index_keys",
            ),
            (
                "run_vector",
                "VectorCmd::CreateIndex",
                "remote::open_cli_generated_client(&store, keys)?",
                "Vector",
                "create_metadata_index",
            ),
            (
                "run_vector",
                "VectorCmd::DropIndex",
                "remote::open_cli_generated_client(&store, keys)?",
                "Vector",
                "drop_metadata_index",
            ),
            (
                "run_vector",
                "VectorCmd::Delete",
                "remote::open_cli_generated_client(&store, keys)?",
                "Vector",
                "delete",
            ),
            (
                "run_vector",
                "VectorCmd::Search",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Vector",
                "search_policy",
            ),
            (
                "run_vector_workspace",
                "VectorWorkspaceCmd::Configure",
                "remote::open_cli_generated_client(&store, keys)?",
                "Vector",
                "vector_workspace_configure_json",
            ),
            (
                "run_vector_text",
                "VectorTextCmd::Upsert",
                "remote::open_cli_generated_client(&store, keys)?",
                "Vector",
                "vector_text_upsert",
            ),
            (
                "run_search",
                "SearchCmd::Create",
                "remote::open_cli_generated_client(&store, keys)?",
                "Search",
                "create",
            ),
            (
                "run_search",
                "SearchCmd::Index",
                "remote::open_cli_generated_client(&store, keys)?",
                "Search",
                "index",
            ),
            (
                "run_search",
                "SearchCmd::Get",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Search",
                "get",
            ),
            (
                "run_search",
                "SearchCmd::Delete",
                "remote::open_cli_generated_client(&store, keys)?",
                "Search",
                "delete",
            ),
            (
                "run_search",
                "SearchCmd::Ids",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Search",
                "ids",
            ),
            (
                "run_search",
                "SearchCmd::Remap",
                "remote::open_cli_generated_client(&store, keys)?",
                "Search",
                "remap",
            ),
            (
                "run_search",
                "SearchCmd::Query",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Search",
                "query",
            ),
            (
                "run_search",
                "SearchCmd::Status",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Search",
                "status",
            ),
            (
                "run_columnar",
                "ColumnarCmd::Create",
                "remote::open_cli_generated_client(&store, keys)?",
                "Columnar",
                "create",
            ),
            (
                "run_columnar",
                "ColumnarCmd::Append",
                "remote::open_cli_generated_client(&store, keys)?",
                "Columnar",
                "append",
            ),
            (
                "run_columnar",
                "ColumnarCmd::Scan",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Columnar",
                "scan",
            ),
            (
                "run_columnar",
                "ColumnarCmd::Columns",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Columnar",
                "columns",
            ),
            (
                "run_columnar",
                "ColumnarCmd::Rows",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Columnar",
                "rows",
            ),
            (
                "run_columnar",
                "ColumnarCmd::Compact",
                "remote::open_cli_generated_client(&store, keys)?",
                "Columnar",
                "compact",
            ),
            (
                "run_columnar",
                "ColumnarCmd::Inspect",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Columnar",
                "inspect",
            ),
            (
                "run_columnar",
                "ColumnarCmd::SourceDigest",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Columnar",
                "source_digest",
            ),
            (
                "run_columnar",
                "ColumnarCmd::Select",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Columnar",
                "select",
            ),
            (
                "run_columnar",
                "ColumnarCmd::Aggregate",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Columnar",
                "aggregate",
            ),
            (
                "run_columnar",
                "ColumnarCmd::ImportArrow",
                "remote::open_cli_generated_client(&store, keys)?",
                "Columnar",
                "columnar_import_arrow",
            ),
            (
                "run_columnar",
                "ColumnarCmd::ImportParquet",
                "remote::open_cli_generated_client(&store, keys)?",
                "Columnar",
                "columnar_import_parquet",
            ),
            (
                "run_dataframe",
                "DataframeCmd::Create",
                "remote::open_cli_generated_client(&store, keys)?",
                "Dataframe",
                "create",
            ),
            (
                "run_dataframe",
                "DataframeCmd::Collect",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Dataframe",
                "collect",
            ),
            (
                "run_dataframe",
                "DataframeCmd::Materialize",
                "remote::open_cli_generated_client(&store, keys)?",
                "Dataframe",
                "materialize",
            ),
            (
                "run_dataframe",
                "DataframeCmd::PlanDigest",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Dataframe",
                "plan_digest",
            ),
            (
                "run_dataframe",
                "DataframeCmd::Preview",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Dataframe",
                "preview",
            ),
            (
                "run_dataframe",
                "DataframeCmd::SourceDigests",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Dataframe",
                "source_digests",
            ),
        ] {
            let body = match_arm_body(function_body(main_source, runner), arm);
            assert!(body.contains(helper), "{arm} must use {helper}");
            assert!(
                body.contains(&format!("\"{interface}\"")),
                "{arm} must dispatch through generated interface {interface}"
            );
            assert!(
                body.contains(&format!("\"{method}\"")),
                "{arm} must dispatch through generated method {method}"
            );
            assert!(
                !body.contains("remote::open_store_client"),
                "{arm} must not bypass generated dispatch through StoreClient"
            );
            assert!(
                !body.contains("FileStore::open("),
                "{arm} must not acquire direct FileStore ownership"
            );
            assert!(
                !body.contains("save_loom("),
                "{arm} must not save outside the generated owner"
            );
        }

        let vector_text_query = match_arm_body(
            function_body(main_source, "run_vector_text"),
            "VectorTextCmd::Query",
        );
        assert!(vector_text_query.contains("remote::target_is_remote(&store)?"));
        assert!(vector_text_query.contains("\"Vector\""));
        assert!(vector_text_query.contains("\"search\""));
        assert!(vector_text_query.contains("\"source_text\""));
        assert!(vector_text_query.contains("cli_open_loom_read(&store, keys)?"));
        assert!(!vector_text_query.contains("remote::open_store_client"));

        for (runner, arm) in [
            ("run_columnar", "ColumnarCmd::ExportArrow"),
            ("run_columnar", "ColumnarCmd::ExportParquet"),
        ] {
            let body = match_arm_body(function_body(main_source, runner), arm);
            assert!(body.contains("cli_open_loom_read(&store, keys)?"));
            assert!(!body.contains("remote::open_store_client"));
            assert!(!body.contains("remote::open_cli_generated_client"));
        }

        let run_search = function_body(main_source, "run_search");
        assert!(run_search.contains("SearchCmd::Rebuild"));
        assert!(run_search.contains("=> rebuild_search_tantivy_index"));

        let unified = function_body(main_source, "run_unified_search");
        assert!(unified.contains("cli_open_loom_read(&args.store, keys)?"));
        assert!(!unified.contains("remote::open_store_client"));
    }

    #[test]
    fn mu_6c_pim_mutations_use_generated_client() {
        let main_source = include_str!("main.rs");

        for (runner, arm, interface, method) in [
            (
                "run_calendar",
                "CalendarCmd::CreateCollection",
                "Calendar",
                "create_collection",
            ),
            (
                "run_calendar",
                "CalendarCmd::DeleteCollection",
                "Calendar",
                "delete_collection",
            ),
            (
                "run_calendar",
                "CalendarCmd::DeleteEntry",
                "Calendar",
                "delete_entry",
            ),
            (
                "run_calendar",
                "CalendarCmd::PutEntry",
                "Calendar",
                "put_entry",
            ),
            ("run_calendar", "CalendarCmd::PutIcs", "Calendar", "put_ics"),
            (
                "run_contacts",
                "ContactsCmd::CreateBook",
                "Contacts",
                "create_book",
            ),
            (
                "run_contacts",
                "ContactsCmd::DeleteBook",
                "Contacts",
                "delete_book",
            ),
            (
                "run_contacts",
                "ContactsCmd::DeleteEntry",
                "Contacts",
                "delete_entry",
            ),
            (
                "run_contacts",
                "ContactsCmd::PutEntry",
                "Contacts",
                "put_entry",
            ),
            (
                "run_contacts",
                "ContactsCmd::PutVcard",
                "Contacts",
                "put_vcard",
            ),
            (
                "run_mail",
                "MailCmd::CreateMailbox",
                "Mail",
                "create_mailbox",
            ),
            (
                "run_mail",
                "MailCmd::DeleteMailbox",
                "Mail",
                "delete_mailbox",
            ),
            (
                "run_mail",
                "MailCmd::DeleteMessage",
                "Mail",
                "delete_message",
            ),
            (
                "run_mail",
                "MailCmd::IngestMessage",
                "Mail",
                "ingest_message",
            ),
            ("run_mail", "MailCmd::SetFlags", "Mail", "set_flags"),
        ] {
            let body = match_arm_body(function_body(main_source, runner), arm);
            assert!(
                body.contains("remote::open_cli_generated_client(&store, keys)?"),
                "{arm} must use the generated client"
            );
            assert!(
                body.contains(&format!("\"{interface}\"")),
                "{arm} must dispatch through generated interface {interface}"
            );
            assert!(
                body.contains(&format!("\"{method}\"")),
                "{arm} must dispatch through generated method {method}"
            );
            assert!(
                !body.contains("remote::open_store_client"),
                "{arm} must not bypass generated dispatch through StoreClient"
            );
            assert!(
                !body.contains("cli_open_loom("),
                "{arm} must not open a writable Loom directly"
            );
            assert!(
                !body.contains("FileStore::open("),
                "{arm} must not open a writable FileStore directly"
            );
        }
    }

    #[test]
    fn mu_17g_c_pim_cli_leaves_use_typed_generated_clients() {
        let main_source = include_str!("main.rs");

        for (runner, arm, opener, interface, method) in [
            (
                "run_calendar",
                "CalendarCmd::CreateCollection",
                "remote::open_cli_generated_client(&store, keys)?",
                "Calendar",
                "create_collection",
            ),
            (
                "run_calendar",
                "CalendarCmd::GetCollection",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Calendar",
                "get_collection",
            ),
            (
                "run_calendar",
                "CalendarCmd::ListCollections",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Calendar",
                "list_collections",
            ),
            (
                "run_calendar",
                "CalendarCmd::DeleteCollection",
                "remote::open_cli_generated_client(&store, keys)?",
                "Calendar",
                "delete_collection",
            ),
            (
                "run_calendar",
                "CalendarCmd::PutIcs",
                "remote::open_cli_generated_client(&store, keys)?",
                "Calendar",
                "put_ics",
            ),
            (
                "run_calendar",
                "CalendarCmd::PutEntry",
                "remote::open_cli_generated_client(&store, keys)?",
                "Calendar",
                "put_entry",
            ),
            (
                "run_calendar",
                "CalendarCmd::GetEntry",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Calendar",
                "get_entry",
            ),
            (
                "run_calendar",
                "CalendarCmd::DeleteEntry",
                "remote::open_cli_generated_client(&store, keys)?",
                "Calendar",
                "delete_entry",
            ),
            (
                "run_calendar",
                "CalendarCmd::ListEntries",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Calendar",
                "list_entries",
            ),
            (
                "run_calendar",
                "CalendarCmd::Range",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Calendar",
                "range",
            ),
            (
                "run_calendar",
                "CalendarCmd::Search",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Calendar",
                "search",
            ),
            (
                "run_calendar",
                "CalendarCmd::ToIcs",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Calendar",
                "to_ics",
            ),
            (
                "run_contacts",
                "ContactsCmd::CreateBook",
                "remote::open_cli_generated_client(&store, keys)?",
                "Contacts",
                "create_book",
            ),
            (
                "run_contacts",
                "ContactsCmd::GetBook",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Contacts",
                "get_book",
            ),
            (
                "run_contacts",
                "ContactsCmd::ListBooks",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Contacts",
                "list_books",
            ),
            (
                "run_contacts",
                "ContactsCmd::DeleteBook",
                "remote::open_cli_generated_client(&store, keys)?",
                "Contacts",
                "delete_book",
            ),
            (
                "run_contacts",
                "ContactsCmd::PutVcard",
                "remote::open_cli_generated_client(&store, keys)?",
                "Contacts",
                "put_vcard",
            ),
            (
                "run_contacts",
                "ContactsCmd::PutEntry",
                "remote::open_cli_generated_client(&store, keys)?",
                "Contacts",
                "put_entry",
            ),
            (
                "run_contacts",
                "ContactsCmd::GetEntry",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Contacts",
                "get_entry",
            ),
            (
                "run_contacts",
                "ContactsCmd::DeleteEntry",
                "remote::open_cli_generated_client(&store, keys)?",
                "Contacts",
                "delete_entry",
            ),
            (
                "run_contacts",
                "ContactsCmd::ListEntries",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Contacts",
                "list_entries",
            ),
            (
                "run_contacts",
                "ContactsCmd::Search",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Contacts",
                "search",
            ),
            (
                "run_contacts",
                "ContactsCmd::ToVcard",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Contacts",
                "to_vcard",
            ),
            (
                "run_mail",
                "MailCmd::CreateMailbox",
                "remote::open_cli_generated_client(&store, keys)?",
                "Mail",
                "create_mailbox",
            ),
            (
                "run_mail",
                "MailCmd::GetMailbox",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Mail",
                "get_mailbox",
            ),
            (
                "run_mail",
                "MailCmd::ListMailboxes",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Mail",
                "list_mailboxes",
            ),
            (
                "run_mail",
                "MailCmd::DeleteMailbox",
                "remote::open_cli_generated_client(&store, keys)?",
                "Mail",
                "delete_mailbox",
            ),
            (
                "run_mail",
                "MailCmd::IngestMessage",
                "remote::open_cli_generated_client(&store, keys)?",
                "Mail",
                "ingest_message",
            ),
            (
                "run_mail",
                "MailCmd::GetMessage",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Mail",
                "get_message",
            ),
            (
                "run_mail",
                "MailCmd::ToEml",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Mail",
                "to_eml",
            ),
            (
                "run_mail",
                "MailCmd::DeleteMessage",
                "remote::open_cli_generated_client(&store, keys)?",
                "Mail",
                "delete_message",
            ),
            (
                "run_mail",
                "MailCmd::ListMessages",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Mail",
                "list_messages",
            ),
            (
                "run_mail",
                "MailCmd::GetFlags",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Mail",
                "get_flags",
            ),
            (
                "run_mail",
                "MailCmd::SetFlags",
                "remote::open_cli_generated_client(&store, keys)?",
                "Mail",
                "set_flags",
            ),
            (
                "run_mail",
                "MailCmd::Search",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "Mail",
                "search",
            ),
        ] {
            let body = match_arm_body(function_body(main_source, runner), arm);
            assert!(body.contains(opener), "{runner} {arm} must use {opener}");
            assert!(
                body.contains(&format!("\"{interface}\"")),
                "{runner} {arm} must dispatch through generated interface {interface}"
            );
            assert!(
                body.contains(&format!("\"{method}\"")),
                "{runner} {arm} must dispatch through generated method {method}"
            );
            assert!(
                !body.contains("remote::open_store_client"),
                "{runner} {arm} must not use legacy StoreClient routing"
            );
            assert!(
                !body.contains("cli_open_loom("),
                "{runner} {arm} must not open a writable Loom directly"
            );
            assert!(
                !body.contains("save_loom("),
                "{runner} {arm} must not save outside the generated owner"
            );
        }
    }

    #[test]
    fn mu_17g_d2_identity_acl_protected_ref_leaves_use_typed_generated_clients() {
        let cli_source = include_str!("cli.rs");
        let management_source = include_str!("management_cmd.rs");
        let generated_api_source = include_str!("../../loom-remote-protocol/src/generated_api.rs");
        let service_source = include_str!("../../loom-client/src/service.rs");

        let inventory = [
            ("IdentityCmd::List", true),
            ("IdentityCmd::Add", true),
            ("IdentityCmd::RenameHandle", true),
            ("IdentityCmd::SetPassphrase", true),
            ("IdentityCmd::CreateAppCredential", true),
            ("IdentityCmd::RevokeAppCredential", true),
            ("IdentityCmd::CreateExternalCredential", true),
            ("IdentityCmd::RevokeExternalCredential", true),
            ("IdentityPublicKeyCmd::Add", true),
            ("IdentityPublicKeyCmd::List", true),
            ("IdentityPublicKeyCmd::Revoke", true),
            ("IdentityCmd::ForceDetachAuthority", true),
            ("IdentityCmd::AuthorityWitness", true),
            ("IdentityCmd::ReplicateAuthority", true),
            ("IdentityCmd::ConfigureAuthorityReplication", true),
            ("IdentityCmd::ListAuthorityReplication", true),
            ("IdentityCmd::RemoveAuthorityReplication", true),
            ("IdentityCmd::Remove", true),
            ("IdentityCmd::AssignRole", true),
            ("IdentityCmd::RevokeRole", true),
            ("AclCmd::List", true),
            ("AclCmd::Grant", true),
            ("AclCmd::Revoke", true),
            ("ProtectedRefCmd::List", true),
            ("ProtectedRefCmd::Get", true),
            ("ProtectedRefCmd::Set", true),
            ("ProtectedRefCmd::Remove", true),
        ];
        let mut seen = std::collections::BTreeSet::new();
        for (leaf, _) in inventory {
            assert!(seen.insert(leaf), "{leaf} is duplicated");
        }
        assert_eq!(seen.len(), 27);
        assert_eq!(
            inventory.iter().filter(|(_, generated)| *generated).count(),
            27
        );
        assert_eq!(
            inventory
                .iter()
                .filter(|(_, generated)| !*generated)
                .count(),
            0
        );
        for enum_name in [
            "IdentityCmd",
            "IdentityPublicKeyCmd",
            "AclCmd",
            "ProtectedRefCmd",
        ] {
            assert!(cli_source.contains(&format!("enum {enum_name}")));
        }

        for method in [
            "identity_list",
            "identity_add_principal",
            "identity_rename_principal_handle",
            "identity_set_passphrase",
            "identity_remove_principal",
            "identity_assign_role",
            "identity_revoke_role",
            "identity_create_external_credential",
            "identity_revoke_external_credential",
            "identity_add_public_key",
            "identity_revoke_public_key",
            "identity_create_app_credential",
            "identity_revoke_app_credential",
            "identity_authority_witness",
            "identity_force_detach_authority_json",
            "identity_replicate_authority_json",
            "identity_configure_authority_replication_json",
            "identity_list_authority_replication",
            "identity_remove_authority_replication_json",
            "acl_list",
            "acl_grant",
            "acl_revoke",
            "protected_ref_list",
            "protected_ref_get",
            "protected_ref_set",
            "protected_ref_remove",
        ] {
            assert!(generated_api_source.contains(&format!("fn {method}")));
        }
        for owner in [
            "impl Identity for LocalLoomClient",
            "impl Acl for LocalLoomClient",
            "impl ProtectedRefs for LocalLoomClient",
        ] {
            assert!(service_source.contains(owner));
        }

        for (runner, arm, opener, interface, methods) in [
            (
                "run_identity",
                "IdentityCmd::List",
                "crate::remote::open_cli_read_only_generated_client(&store, keys)?",
                "Identity",
                &["identity_list"][..],
            ),
            (
                "run_identity_public_key",
                "IdentityPublicKeyCmd::List",
                "crate::remote::open_cli_read_only_generated_client(&store, keys)?",
                "Identity",
                &["identity_list"],
            ),
            (
                "run_acl",
                "AclCmd::List",
                "crate::remote::open_cli_read_only_generated_client(&store, keys)?",
                "Acl",
                &["acl_list"],
            ),
            (
                "run_protected_ref",
                "ProtectedRefCmd::List",
                "crate::remote::open_cli_read_only_generated_client(&store, keys)?",
                "ProtectedRefs",
                &["protected_ref_list"],
            ),
            (
                "run_protected_ref",
                "ProtectedRefCmd::Get",
                "crate::remote::open_cli_read_only_generated_client(&store, keys)?",
                "ProtectedRefs",
                &["protected_ref_get"],
            ),
            (
                "run_identity",
                "IdentityCmd::Add",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "Identity",
                &["identity_add_principal"],
            ),
            (
                "run_identity",
                "IdentityCmd::RenameHandle",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "Identity",
                &["identity_rename_principal_handle"],
            ),
            (
                "run_identity",
                "IdentityCmd::SetPassphrase",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "Identity",
                &["identity_set_passphrase"],
            ),
            (
                "run_identity",
                "IdentityCmd::Remove {",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "Identity",
                &["identity_remove_principal"],
            ),
            (
                "run_identity",
                "IdentityCmd::AssignRole",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "Identity",
                &["identity_assign_role"],
            ),
            (
                "run_identity",
                "IdentityCmd::RevokeRole",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "Identity",
                &["identity_revoke_role"],
            ),
            (
                "run_identity",
                "IdentityCmd::ForceDetachAuthority",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "Identity",
                &["identity_force_detach_authority_json"],
            ),
            (
                "run_identity",
                "IdentityCmd::AuthorityWitness",
                "crate::remote::open_cli_read_only_generated_client(&store, keys)?",
                "Identity",
                &["identity_authority_witness"],
            ),
            (
                "run_identity",
                "IdentityCmd::ReplicateAuthority",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "Identity",
                &["identity_replicate_authority_json"],
            ),
            (
                "run_identity",
                "IdentityCmd::ConfigureAuthorityReplication",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "Identity",
                &["identity_configure_authority_replication_json"],
            ),
            (
                "run_identity",
                "IdentityCmd::ListAuthorityReplication",
                "crate::remote::open_cli_read_only_generated_client(&store, keys)?",
                "Identity",
                &["identity_list_authority_replication"],
            ),
            (
                "run_identity",
                "IdentityCmd::RemoveAuthorityReplication",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "Identity",
                &["identity_remove_authority_replication_json"],
            ),
            (
                "run_acl",
                "AclCmd::Grant",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "Acl",
                &["acl_grant"],
            ),
            (
                "run_acl",
                "AclCmd::Revoke",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "Acl",
                &["acl_revoke"],
            ),
            (
                "run_protected_ref",
                "ProtectedRefCmd::Set",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "ProtectedRefs",
                &["protected_ref_set"],
            ),
            (
                "run_protected_ref",
                "ProtectedRefCmd::Remove",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "ProtectedRefs",
                &["protected_ref_remove"],
            ),
        ] {
            let body = match_arm_body(function_body(management_source, runner), arm);
            let helper_body = if methods.contains(&"identity_list") {
                function_body(management_source, "generated_identity_snapshot")
            } else {
                ""
            };
            assert!(body.contains(opener), "{runner} {arm} must use {opener}");
            assert!(
                body.contains(&format!("\"{interface}\""))
                    || helper_body.contains(&format!("\"{interface}\"")),
                "{runner} {arm} must use generated interface {interface}"
            );
            for method in methods {
                assert!(
                    body.contains(&format!("\"{method}\""))
                        || helper_body.contains(&format!("\"{method}\"")),
                    "{runner} {arm} must use generated method {method}"
                );
            }
            for forbidden in [
                "crate::remote::open_store_client",
                "cli_open_loom_read(",
                "cli_open_loom(",
                "FileStore::open(",
                "save_loom(",
            ] {
                assert!(
                    !body.contains(forbidden),
                    "{runner} {arm} found {forbidden}"
                );
            }
        }

        for (runner, arm, helper, methods) in [
            (
                "run_identity",
                "IdentityCmd::CreateAppCredential",
                "generated_app_credential_create",
                &["identity_create_app_credential"][..],
            ),
            (
                "run_identity",
                "IdentityCmd::RevokeAppCredential",
                "generated_app_credential_revoke",
                &["identity_revoke_app_credential"],
            ),
            (
                "run_identity",
                "IdentityCmd::CreateExternalCredential",
                "generated_external_credential_create",
                &["identity_create_external_credential"],
            ),
            (
                "run_identity",
                "IdentityCmd::RevokeExternalCredential",
                "generated_external_credential_revoke",
                &["identity_revoke_external_credential"],
            ),
            (
                "run_identity_public_key",
                "IdentityPublicKeyCmd::Add",
                "generated_public_key_add",
                &["identity_add_public_key"],
            ),
            (
                "run_identity_public_key",
                "IdentityPublicKeyCmd::Revoke",
                "generated_public_key_revoke",
                &["identity_revoke_public_key"],
            ),
        ] {
            let body = match_arm_body(function_body(management_source, runner), arm);
            assert!(body.contains("crate::remote::open_cli_generated_client(&store, keys)?"));
            assert!(body.contains(helper));
            let helper_body = function_body(management_source, helper);
            assert!(helper_body.contains("\"Identity\""));
            for method in methods {
                assert!(helper_body.contains(&format!("\"{method}\"")));
            }
            for forbidden in [
                "crate::remote::open_store_client",
                "cli_open_loom_read(",
                "cli_open_loom(",
                "FileStore::open(",
                "save_loom(",
            ] {
                assert!(
                    !body.contains(forbidden),
                    "{runner} {arm} found {forbidden}"
                );
            }
        }
        assert!(generated_boundary_classifications().iter().any(|entry| {
            entry.path == "management kv config set"
                && entry.ownership
                    == (LeafOwnership::Generated {
                        interface: "ManagementKv",
                        method: "set_config",
                    })
        }));
    }

    #[test]
    fn mu_17g_d1_core_cli_leaves_use_typed_generated_clients() {
        let main_source = include_str!("main.rs");
        let management_source = include_str!("management_cmd.rs");
        let remote_source = include_str!("remote.rs");

        for (arm, opener, methods) in [
            (
                "FilesCmd::Delete",
                "remote::open_cli_generated_client(&store, keys)?",
                &["stat", "remove_directory", "remove_file"][..],
            ),
            (
                "FilesCmd::Ls",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                &["list_directory"],
            ),
            (
                "FilesCmd::Mkdir",
                "remote::open_cli_generated_client(&store, keys)?",
                &["create_directory"],
            ),
            (
                "FilesCmd::Read",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                &["read_file"],
            ),
            (
                "FilesCmd::Write",
                "remote::open_cli_generated_client(&store, keys)?",
                &["create_directory", "write_file"],
            ),
        ] {
            let body = match_arm_body(function_body(main_source, "run_files"), arm);
            assert!(body.contains(opener), "{arm} must use {opener}");
            assert!(
                body.contains("\"FileSystem\"") || body.contains("generated_files_list"),
                "{arm} must dispatch through FileSystem generated methods"
            );
            for method in methods {
                let present = body.contains(&format!("\"{method}\""))
                    || function_body(main_source, "generated_files_list")
                        .contains(&format!("\"{method}\""));
                assert!(
                    present,
                    "{arm} must dispatch through generated FileSystem.{method}"
                );
            }
            for forbidden in [
                "remote::open_store_client",
                "cli_open_loom(",
                "FileStore::open(",
                "save_loom(",
            ] {
                assert!(!body.contains(forbidden), "{arm} found {forbidden}");
            }
        }

        for (arm, opener, method) in [
            (
                "WorkspaceCmd::Create",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "workspace_create",
            ),
            (
                "WorkspaceCmd::List",
                "crate::remote::open_cli_read_only_generated_client(&store, keys)?",
                "workspace_list",
            ),
            (
                "WorkspaceCmd::Rename",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "workspace_rename",
            ),
            (
                "WorkspaceCmd::Delete",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "workspace_delete",
            ),
        ] {
            let body = match_arm_body(
                function_body(management_source, "run_management_workspace"),
                arm,
            );
            assert!(body.contains(opener), "{arm} must use {opener}");
            assert!(
                body.contains("\"Workspaces\"") || body.contains("workspace_list()"),
                "{arm} must dispatch through generated Workspaces authority"
            );
            assert!(
                body.contains(&format!("\"{method}\""))
                    || function_body(remote_source, "workspace_list")
                        .contains(&format!("\"{method}\"")),
                "{arm} must dispatch through generated Workspaces.{method}"
            );
            for forbidden in [
                "crate::remote::open_store_client",
                "cli_open_loom(",
                "FileStore::open(",
                "save_loom(",
            ] {
                assert!(!body.contains(forbidden), "{arm} found {forbidden}");
            }
        }

        for (arm, opener, helper, method) in [
            (
                "DocumentCmd::Delete",
                "remote::open_cli_generated_client(&store, keys)?",
                "doc_delete",
                "delete",
            ),
            (
                "DocumentCmd::DeleteCollection",
                "remote::open_cli_generated_client(&store, keys)?",
                "doc_delete_collection",
                "delete_collection",
            ),
            (
                "DocumentCmd::GetText",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "doc_get_text",
                "get_text",
            ),
            (
                "DocumentCmd::PutText",
                "remote::open_cli_generated_client(&store, keys)?",
                "doc_put_text",
                "put_text",
            ),
            (
                "DocumentCmd::GetBinary",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "doc_get_binary",
                "get_binary",
            ),
            (
                "DocumentCmd::PutBinary",
                "remote::open_cli_generated_client(&store, keys)?",
                "doc_put_binary",
                "put_binary",
            ),
            (
                "DocumentCmd::ListBinary",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "doc_list_binary",
                "list_binary",
            ),
            (
                "DocumentCmd::Find",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "doc_find",
                "find_json",
            ),
            (
                "DocumentCmd::Query",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "doc_query",
                "query_json",
            ),
            (
                "DocumentCmd::IndexCreate",
                "remote::open_cli_generated_client(&store, keys)?",
                "doc_index_create",
                "index_create",
            ),
            (
                "DocumentCmd::IndexCreateJson",
                "remote::open_cli_generated_client(&store, keys)?",
                "doc_index_create_json",
                "index_create_json",
            ),
            (
                "DocumentCmd::IndexDrop",
                "remote::open_cli_generated_client(&store, keys)?",
                "doc_index_drop",
                "index_drop",
            ),
            (
                "DocumentCmd::IndexList",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "doc_index_list",
                "index_list_json",
            ),
            (
                "DocumentCmd::IndexRebuild",
                "remote::open_cli_generated_client(&store, keys)?",
                "doc_index_rebuild",
                "index_rebuild",
            ),
            (
                "DocumentCmd::IndexStatus",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "doc_index_statuses",
                "index_status_json",
            ),
        ] {
            let body = match_arm_body(function_body(main_source, "run_document"), arm);
            assert!(body.contains(opener), "{arm} must use {opener}");
            assert!(body.contains(helper), "{arm} must use {helper}");
            let helper_body = function_body(remote_source, helper);
            assert!(
                helper_body.contains(&format!("\"{method}\"")),
                "{helper} must dispatch through generated Document.{method}"
            );
            if helper == "doc_get_text" {
                assert!(
                    !helper_body.contains("doc_get_binary"),
                    "DocumentCmd::GetText must not route through Document.get_binary"
                );
                assert!(
                    !helper_body.contains("\"get_binary\""),
                    "DocumentCmd::GetText must not dispatch Document.get_binary"
                );
            }
            for forbidden in [
                "remote::open_store_client",
                "cli_open_loom(",
                "FileStore::open(",
                "save_loom(",
            ] {
                assert!(!body.contains(forbidden), "{arm} found {forbidden}");
            }
        }
        for helper in [
            "document_bool",
            "document_bytes",
            "document_void",
            "doc_put_binary_value",
        ] {
            assert!(
                function_body(remote_source, helper).contains("\"Document\""),
                "{helper} must use the generated Document interface"
            );
        }

        for (arm, opener, method) in [
            (
                "PagesCmd::SpaceCreate",
                "remote::open_cli_generated_client(&store, keys)?",
                "spaces_create_json",
            ),
            (
                "PagesCmd::SpaceList",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "spaces_list_json",
            ),
            (
                "PagesCmd::SpaceGet",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "spaces_get_json",
            ),
            (
                "PagesCmd::Create",
                "remote::open_cli_generated_client(&store, keys)?",
                "pages_create_json",
            ),
            (
                "PagesCmd::Update",
                "remote::open_cli_generated_client(&store, keys)?",
                "pages_update_json",
            ),
            (
                "PagesCmd::Publish",
                "remote::open_cli_generated_client(&store, keys)?",
                "pages_publish_json",
            ),
            (
                "PagesCmd::Get",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "pages_get_json",
            ),
            (
                "PagesCmd::History",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "pages_history_json",
            ),
            (
                "PagesCmd::StructureCreate",
                "remote::open_cli_generated_client(&store, keys)?",
                "structures_create_json",
            ),
            (
                "PagesCmd::StructureGet",
                "remote::open_cli_read_only_generated_client(&store, keys)?",
                "structures_get_json",
            ),
            (
                "PagesCmd::StructureAddNode",
                "remote::open_cli_generated_client(&store, keys)?",
                "structures_add_node_json",
            ),
            (
                "PagesCmd::StructureUpdateNode",
                "remote::open_cli_generated_client(&store, keys)?",
                "structures_update_node_json",
            ),
            (
                "PagesCmd::StructureBind",
                "remote::open_cli_generated_client(&store, keys)?",
                "structures_bind_json",
            ),
            (
                "PagesCmd::StructureMoveNode",
                "remote::open_cli_generated_client(&store, keys)?",
                "structures_move_node_json",
            ),
            (
                "PagesCmd::StructureLinkNode",
                "remote::open_cli_generated_client(&store, keys)?",
                "structures_link_node_json",
            ),
            (
                "PagesCmd::StructureDecomposeToTickets",
                "remote::open_cli_generated_client(&store, keys)?",
                "structures_decompose_to_tickets_json",
            ),
        ] {
            let body = match_arm_body(function_body(main_source, "run_pages"), arm);
            assert!(body.contains(opener), "{arm} must use {opener}");
            assert!(body.contains("\"Pages\""), "{arm} must use Pages");
            assert!(
                body.contains(&format!("\"{method}\"")),
                "{arm} must dispatch through generated Pages.{method}"
            );
            for forbidden in [
                "remote::open_store_client",
                "cli_open_loom(",
                "FileStore::open(",
                "save_loom(",
            ] {
                assert!(!body.contains(forbidden), "{arm} found {forbidden}");
            }
        }
    }

    #[test]
    fn mu_17g_d3_tickets_lanes_cli_leaves_use_typed_generated_clients() {
        let cli_source = include_str!("cli.rs");
        let main_source = include_str!("main.rs");
        let generated_api_source = include_str!("../../loom-remote-protocol/src/generated_api.rs");
        let service_source = include_str!("../../loom-client/src/service.rs");

        struct Leaf<'a> {
            runner: &'a str,
            arm: &'a str,
            opener: &'a str,
            interface: &'a str,
            methods: &'a [&'a str],
        }

        let ticket_leaves = [
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::ProjectCreate",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_project_create_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::ProjectRekey",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_project_rekey_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::ProjectSettingsGet",
                opener: "remote::open_cli_read_only_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_project_settings_get_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::ProjectSettingsSet",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_project_settings_set_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::Projects",
                opener: "remote::open_cli_read_only_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_projects_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::Relations",
                opener: "remote::open_cli_read_only_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_relation_list_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::Fields",
                opener: "remote::open_cli_read_only_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_fields_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::FieldPut",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_field_put_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::FieldRetire",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_field_retire_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::Create",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_projects_json", "tickets_create_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::Update",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_update_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::Delete",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_delete_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::Comments",
                opener: "remote::open_cli_read_only_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_comments_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::CommentAdd",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_comment_add_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::CommentUpdate",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_comment_update_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::CommentDelete",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_comment_delete_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::BoardCreate",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["boards_create_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::BoardGet",
                opener: "remote::open_cli_read_only_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["boards_get_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::BoardList",
                opener: "remote::open_cli_read_only_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["boards_list_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::BoardUpdate",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["boards_update_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::BoardDelete",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["boards_delete_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::BoardConfigureColumns",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["boards_configure_columns_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::BoardMoveCard",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["boards_move_card_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::RelationSet",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_relation_set_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::RelationRemove",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_relation_remove_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::List",
                opener: "remote::open_cli_read_only_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_list_json"],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::Get",
                opener: "remote::open_cli_read_only_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &[
                    "tickets_get_json",
                    "tickets_history_json",
                    "tickets_comments_json",
                ],
            },
            Leaf {
                runner: "run_tickets",
                arm: "TicketsCmd::History",
                opener: "remote::open_cli_read_only_generated_client(&store, keys)?",
                interface: "Tickets",
                methods: &["tickets_history_json"],
            },
        ];

        let lane_leaves = [
            Leaf {
                runner: "run_lanes",
                arm: "LanesCmd::Create",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Lanes",
                methods: &["lanes_create"],
            },
            Leaf {
                runner: "run_lanes",
                arm: "LanesCmd::Get",
                opener: "remote::open_cli_read_only_generated_client(&store, keys)?",
                interface: "Lanes",
                methods: &["lanes_get", "lanes_get_view_json"],
            },
            Leaf {
                runner: "run_lanes",
                arm: "LanesCmd::List",
                opener: "remote::open_cli_read_only_generated_client(&store, keys)?",
                interface: "Lanes",
                methods: &["lanes_list_views_json"],
            },
            Leaf {
                runner: "run_lanes",
                arm: "LanesCmd::Update",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Lanes",
                methods: &["lanes_update"],
            },
            Leaf {
                runner: "run_lanes",
                arm: "LanesCmd::Closeout",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Lanes",
                methods: &["lanes_closeout"],
            },
            Leaf {
                runner: "run_lanes",
                arm: "LanesCmd::TicketAdd",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Lanes",
                methods: &["lanes_ticket_add"],
            },
            Leaf {
                runner: "run_lanes",
                arm: "LanesCmd::TicketRemove",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Lanes",
                methods: &["lanes_ticket_remove"],
            },
            Leaf {
                runner: "run_lanes",
                arm: "LanesCmd::TicketTransfer",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Lanes",
                methods: &["lanes_ticket_transfer"],
            },
            Leaf {
                runner: "run_lanes",
                arm: "LanesCmd::Delete",
                opener: "remote::open_cli_generated_client(&store, keys)?",
                interface: "Lanes",
                methods: &["lanes_delete"],
            },
            Leaf {
                runner: "run_lanes",
                arm: "LanesCmd::Cleanup",
                opener: "cleanup_dual_generated_client",
                interface: "Lanes",
                methods: &["cleanup_json"],
            },
        ];

        assert_eq!(
            enum_variants(cli_source, "TicketsCmd"),
            ticket_leaves
                .iter()
                .map(|leaf| leaf.arm.trim_start_matches("TicketsCmd::").to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            enum_variants(cli_source, "LanesCmd"),
            lane_leaves
                .iter()
                .map(|leaf| leaf.arm.trim_start_matches("LanesCmd::").to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(ticket_leaves.len(), 28);
        assert_eq!(lane_leaves.len(), 10);

        for method in ticket_leaves
            .iter()
            .filter(|leaf| leaf.interface == "Tickets")
            .flat_map(|leaf| leaf.methods.iter())
        {
            assert!(
                generated_api_source.contains(&format!("fn {method}")),
                "generated Tickets trait must expose {method}"
            );
        }
        for method in [
            "create",
            "get",
            "list",
            "update",
            "ticket_add",
            "ticket_remove",
            "ticket_transfer",
            "delete",
            "closeout",
            "get_view_json",
            "list_views_json",
            "cleanup_json",
        ] {
            assert!(
                generated_api_source.contains(&format!("fn {method}")),
                "generated Lanes trait must expose {method}"
            );
        }
        for owner in [
            "impl Tickets for LocalLoomClient",
            "impl Lanes for LocalLoomClient",
        ] {
            assert!(
                service_source.contains(owner),
                "LocalLoomClient must own {owner}"
            );
        }

        let forbidden = [
            "remote::open_store_client",
            "StoreClient::",
            "cli_open_loom(",
            "cli_open_loom_read(",
            "FileStore::open",
            "Loom::new(",
            "save_loom(",
            "loom_tickets::create_ticket",
            "loom_tickets::update_ticket",
            "loom_tickets::delete_ticket",
            "loom_lanes::create_lane",
            "loom_lanes::put_lane",
            "loom_lanes::delete_lane",
        ];
        for leaf in ticket_leaves.iter().chain(lane_leaves.iter()) {
            let body = match_arm_body(function_body(main_source, leaf.runner), leaf.arm);
            if leaf.opener == "cleanup_dual_generated_client" {
                assert!(body.contains("remote::open_cli_generated_client(&store, keys)?"));
                assert!(
                    body.contains("remote::open_cli_read_only_generated_client(&store, keys)?")
                );
            } else {
                assert!(
                    body.contains(leaf.opener),
                    "{} {} must use {}",
                    leaf.runner,
                    leaf.arm,
                    leaf.opener
                );
            }
            if leaf.interface == "Tickets" {
                assert!(body.contains("\"Tickets\""), "{} {}", leaf.runner, leaf.arm);
            }
            for method in leaf.methods {
                assert!(
                    body.contains(method),
                    "{} {} must dispatch through {method}",
                    leaf.runner,
                    leaf.arm
                );
            }
            for forbidden in forbidden {
                assert!(
                    !body.contains(forbidden),
                    "{} {} must not use legacy execution helper {forbidden}",
                    leaf.runner,
                    leaf.arm
                );
            }
        }
    }

    #[test]
    fn mu_17g_e1_sql_meetings_cli_leaves_use_typed_generated_clients() {
        let cli_source = include_str!("cli.rs");
        let main_source = include_str!("main.rs");
        let generated_api_source = include_str!("../../loom-remote-protocol/src/generated_api.rs");
        let remote_client_source = include_str!("../../loom-remote-client/src/generated_client.rs");
        let hosted_dispatch_source =
            include_str!("../../loom-hosted-core/src/generated_dispatch.rs");
        let service_source = include_str!("../../loom-client/src/service.rs");

        assert_eq!(enum_variants(cli_source, "SqlCmd"), vec!["Exec", "Table"]);
        assert_eq!(
            enum_variants(cli_source, "MeetingsCmd"),
            vec!["List", "Get", "Search", "SourceRead", "Import"]
        );

        struct Leaf<'a> {
            runner: &'a str,
            arm: &'a str,
            interface: &'a str,
            method: &'a str,
            result: &'a str,
        }

        let leaves = [
            Leaf {
                runner: "run_sql_cmd",
                arm: "SqlCmd::Exec",
                interface: "Sql",
                method: "sql_exec_result",
                result: "execute_generated_bytes",
            },
            Leaf {
                runner: "run_meetings",
                arm: "MeetingsCmd::Import",
                interface: "Meetings",
                method: "meetings_import_snapshot",
                result: "execute_generated_string",
            },
        ];

        for leaf in leaves {
            let body = match_arm_body(function_body(main_source, leaf.runner), leaf.arm);
            assert!(
                body.contains("remote::open_cli_generated_client(&store, keys)?"),
                "{} {} must open the generated CLI client",
                leaf.runner,
                leaf.arm
            );
            assert!(
                body.contains(&format!("\"{}\"", leaf.interface)),
                "{} {} must route through {}",
                leaf.runner,
                leaf.arm,
                leaf.interface
            );
            assert!(
                body.contains(&format!("\"{}\"", leaf.method)),
                "{} {} must call {}",
                leaf.runner,
                leaf.arm,
                leaf.method
            );
            assert!(
                body.contains(leaf.result),
                "{} {} must use the typed generated result adapter",
                leaf.runner,
                leaf.arm
            );
            for forbidden in [
                "remote::open_store_client",
                "StoreClient::",
                "cli_open_loom(",
                "cli_open_loom_read(",
                "FileStore::open",
                "Loom::new(",
                "LoomSqlStore::open_write(",
                "Glue::new(",
                "save_loom(",
                "ensure_facet_workspace(",
                "import_meetings_bytes(",
            ] {
                assert!(
                    !body.contains(forbidden),
                    "{} {} must not use legacy execution helper {forbidden}",
                    leaf.runner,
                    leaf.arm
                );
            }
            assert!(
                generated_api_source.contains(&format!("fn {}(", leaf.method)),
                "generated API must expose {}.{}",
                leaf.interface,
                leaf.method
            );
            assert!(
                remote_client_source.contains(&format!("fn {}(", leaf.method)),
                "remote client must expose {}.{}",
                leaf.interface,
                leaf.method
            );
            assert!(
                hosted_dispatch_source
                    .contains(&format!("(\"{}\", \"{}\")", leaf.interface, leaf.method)),
                "hosted dispatch must expose {}.{}",
                leaf.interface,
                leaf.method
            );
            assert!(
                service_source.contains(&format!("impl {} for LocalLoomClient", leaf.interface)),
                "LocalLoomClient must implement {}",
                leaf.interface
            );
        }

        let run_sql = function_body(main_source, "run_sql_cmd");
        assert!(
            run_sql.contains("SqlCmd::Table { action } => run_table(action, keys),"),
            "SqlCmd::Table remains delegated outside MU-17g-e1 SQL execution scope"
        );

        let meetings_read_only = [
            "MeetingsCmd::List",
            "MeetingsCmd::Get",
            "MeetingsCmd::Search",
        ];
        for arm in meetings_read_only {
            let body = match_arm_body(function_body(main_source, "run_meetings"), arm);
            assert!(
                body.contains("cli_open_loom_read(&store, keys)?"),
                "{arm} remains a read-only presentation path outside MU-17g-e1 import scope"
            );
            assert!(
                !body.contains("remote::open_cli_generated_client(&store, keys)?"),
                "{arm} must not be counted as a migrated Meetings import leaf"
            );
            assert!(
                !body.contains("save_loom("),
                "{arm} must not own a durable mutation"
            );
        }
        let source_read = match_arm_segment(
            function_body(main_source, "run_meetings"),
            "MeetingsCmd::SourceRead",
            "MeetingsCmd",
        );
        assert!(source_read.contains("remote::open_cli_read_only_generated_client"));
        assert!(source_read.contains("\"Meetings\""));
        assert!(source_read.contains("\"meetings_source_read\""));
        assert!(source_read.contains("execute_generated_bytes"));
        for forbidden in ["cli_open_loom_read", "open_store_client", "FileStore::open"] {
            assert!(!source_read.contains(forbidden));
        }
        assert!(generated_boundary_classifications().iter().any(|entry| {
            entry.path == "meetings source-read"
                && entry.ownership
                    == (LeafOwnership::Generated {
                        interface: "Meetings",
                        method: "meetings_source_read",
                    })
        }));
    }

    #[test]
    fn mu_17g_e2_chat_drive_mutation_leaves_use_typed_generated_clients() {
        let cli_source = include_str!("cli.rs");
        let main_source = include_str!("main.rs");
        let generated_api_source = include_str!("../../loom-remote-protocol/src/generated_api.rs");
        let remote_client_source = include_str!("../../loom-remote-client/src/generated_client.rs");
        let hosted_dispatch_source =
            include_str!("../../loom-hosted-core/src/generated_dispatch.rs");
        let service_source = include_str!("../../loom-client/src/service.rs");

        assert_eq!(
            enum_variants(cli_source, "ChatCmd"),
            vec![
                "Channels",
                "CreateChannel",
                "RenameChannel",
                "Messages",
                "Events",
                "Cursor",
                "UpdateCursor",
                "Post",
                "Edit",
                "Redact",
                "CreateThread",
                "CreateTask",
                "ClaimTask",
                "CompleteTask",
                "InvokeAgent",
                "AgentReply",
                "RequestHandoff",
                "AddReaction",
                "RemoveReaction",
                "EmojiList",
                "EmojiRegister",
                "EmojiUnregister",
            ]
        );
        assert_eq!(
            enum_variants(cli_source, "DriveCmd"),
            vec![
                "List",
                "Stat",
                "Read",
                "ListVersions",
                "ListConflicts",
                "ListShares",
                "GrantShare",
                "RevokeShare",
                "ApplyShareExpiry",
                "ListRetention",
                "PinRetention",
                "UnpinRetention",
                "ApplyRetention",
                "CreateFolder",
                "CreateUpload",
                "UploadChunk",
                "CommitUpload",
                "Rename",
                "Move",
                "Delete",
                "ResolveConflict",
            ]
        );

        struct Leaf<'a> {
            arm: &'a str,
            interface: &'a str,
            method: &'a str,
        }

        let chat = [
            Leaf {
                arm: "ChatCmd::CreateChannel",
                interface: "Chat",
                method: "chat_create_channel_json",
            },
            Leaf {
                arm: "ChatCmd::RenameChannel",
                interface: "Chat",
                method: "chat_rename_channel_json",
            },
            Leaf {
                arm: "ChatCmd::UpdateCursor",
                interface: "Chat",
                method: "chat_update_cursor_json",
            },
            Leaf {
                arm: "ChatCmd::Post",
                interface: "Chat",
                method: "chat_post_message_bytes_json",
            },
            Leaf {
                arm: "ChatCmd::Edit",
                interface: "Chat",
                method: "chat_edit_message_bytes_json",
            },
            Leaf {
                arm: "ChatCmd::Redact",
                interface: "Chat",
                method: "chat_redact_message_json",
            },
            Leaf {
                arm: "ChatCmd::CreateThread",
                interface: "Chat",
                method: "chat_create_thread_json",
            },
            Leaf {
                arm: "ChatCmd::CreateTask",
                interface: "Chat",
                method: "chat_create_task_json",
            },
            Leaf {
                arm: "ChatCmd::ClaimTask",
                interface: "Chat",
                method: "chat_claim_task_json",
            },
            Leaf {
                arm: "ChatCmd::CompleteTask",
                interface: "Chat",
                method: "chat_complete_task_json",
            },
            Leaf {
                arm: "ChatCmd::InvokeAgent",
                interface: "Chat",
                method: "chat_invoke_agent_bytes_json",
            },
            Leaf {
                arm: "ChatCmd::AgentReply",
                interface: "Chat",
                method: "chat_agent_reply_json",
            },
            Leaf {
                arm: "ChatCmd::RequestHandoff",
                interface: "Chat",
                method: "chat_request_handoff_json",
            },
            Leaf {
                arm: "ChatCmd::AddReaction",
                interface: "Chat",
                method: "chat_add_reaction_json",
            },
            Leaf {
                arm: "ChatCmd::RemoveReaction",
                interface: "Chat",
                method: "chat_remove_reaction_json",
            },
            Leaf {
                arm: "ChatCmd::EmojiRegister",
                interface: "Chat",
                method: "chat_emoji_register_json",
            },
            Leaf {
                arm: "ChatCmd::EmojiUnregister",
                interface: "Chat",
                method: "chat_emoji_unregister_json",
            },
        ];
        let drive = [
            Leaf {
                arm: "DriveCmd::GrantShare",
                interface: "Drive",
                method: "drive_grant_share_json",
            },
            Leaf {
                arm: "DriveCmd::RevokeShare",
                interface: "Drive",
                method: "drive_revoke_share_json",
            },
            Leaf {
                arm: "DriveCmd::ApplyShareExpiry",
                interface: "Drive",
                method: "drive_apply_share_expiry_json",
            },
            Leaf {
                arm: "DriveCmd::PinRetention",
                interface: "Drive",
                method: "drive_pin_retention_json",
            },
            Leaf {
                arm: "DriveCmd::UnpinRetention",
                interface: "Drive",
                method: "drive_unpin_retention_json",
            },
            Leaf {
                arm: "DriveCmd::ApplyRetention",
                interface: "Drive",
                method: "drive_apply_retention_json",
            },
            Leaf {
                arm: "DriveCmd::CreateFolder",
                interface: "Drive",
                method: "drive_create_folder_json",
            },
            Leaf {
                arm: "DriveCmd::CreateUpload",
                interface: "Drive",
                method: "drive_create_upload_json",
            },
            Leaf {
                arm: "DriveCmd::UploadChunk",
                interface: "Drive",
                method: "drive_upload_chunk_json",
            },
            Leaf {
                arm: "DriveCmd::CommitUpload",
                interface: "Drive",
                method: "drive_commit_upload_json",
            },
            Leaf {
                arm: "DriveCmd::Rename",
                interface: "Drive",
                method: "drive_rename_json",
            },
            Leaf {
                arm: "DriveCmd::Move",
                interface: "Drive",
                method: "drive_move_json",
            },
            Leaf {
                arm: "DriveCmd::Delete",
                interface: "Drive",
                method: "drive_delete_json",
            },
            Leaf {
                arm: "DriveCmd::ResolveConflict",
                interface: "Drive",
                method: "drive_resolve_conflict_json",
            },
        ];
        assert_eq!(chat.len(), 17);
        assert_eq!(drive.len(), 14);

        let forbidden = [
            "remote::open_store_client",
            "StoreClient::",
            "cli_open_loom(",
            "cli_open_loom_read(",
            "FileStore::open",
            "Loom::new(",
            "save_loom(",
        ];
        for leaf in chat.iter().chain(drive.iter()) {
            let runner = if leaf.interface == "Chat" {
                "run_chat"
            } else {
                "run_drive"
            };
            let body = match_arm_body(function_body(main_source, runner), leaf.arm);
            assert!(
                body.contains("generated_workspace_context(&store, &workspace, keys)?"),
                "{} must open the typed generated client context",
                leaf.arm
            );
            assert!(body.contains(&format!("\"{}\"", leaf.interface)));
            assert!(body.contains(&format!("\"{}\"", leaf.method)));
            assert!(body.contains("execute_generated_json::<"));
            for forbidden in forbidden {
                assert!(
                    !body.contains(forbidden),
                    "{} must not use legacy execution helper {forbidden}",
                    leaf.arm
                );
            }
            for (source, label) in [
                (generated_api_source, "generated API"),
                (remote_client_source, "remote client"),
            ] {
                assert!(
                    source.contains(&format!("fn {}(", leaf.method)),
                    "{label} must expose {}.{}",
                    leaf.interface,
                    leaf.method
                );
            }
            assert!(
                hosted_dispatch_source
                    .contains(&format!("(\"{}\", \"{}\")", leaf.interface, leaf.method)),
                "hosted dispatch must expose {}.{}",
                leaf.interface,
                leaf.method
            );
            assert!(
                service_source.contains(&format!("async fn {}(", leaf.method)),
                "LocalLoomClient must implement {}.{}",
                leaf.interface,
                leaf.method
            );
        }

        for arm in [
            "ChatCmd::Channels",
            "ChatCmd::Messages",
            "ChatCmd::Events",
            "ChatCmd::Cursor",
            "ChatCmd::EmojiList",
            "DriveCmd::List",
            "DriveCmd::Stat",
            "DriveCmd::Read",
            "DriveCmd::ListVersions",
            "DriveCmd::ListConflicts",
            "DriveCmd::ListShares",
            "DriveCmd::ListRetention",
        ] {
            let runner = if arm.starts_with("Chat") {
                "run_chat"
            } else {
                "run_drive"
            };
            let body = match_arm_body(function_body(main_source, runner), arm);
            assert!(!body.contains("save_loom("), "{arm} must remain read-only");
        }

        for (arm, method) in [
            ("DriveCmd::List", "drive_list_json"),
            ("DriveCmd::Stat", "drive_stat_json"),
            ("DriveCmd::Read", "drive_read_file"),
            ("DriveCmd::ListVersions", "drive_list_versions_json"),
            ("DriveCmd::ListConflicts", "drive_list_conflicts_json"),
            ("DriveCmd::ListShares", "drive_list_shares_json"),
            ("DriveCmd::ListRetention", "drive_list_retention_json"),
        ] {
            let body = match_arm_segment(function_body(main_source, "run_drive"), arm, "DriveCmd");
            assert!(body.contains("remote::open_cli_read_only_generated_client"));
            assert!(body.contains("\"Drive\""));
            assert!(body.contains(&format!("\"{method}\"")));
            for forbidden in [
                "open_drive_read",
                "cli_open_loom_read",
                "open_store_client",
                "FileStore::open",
            ] {
                assert!(!body.contains(forbidden), "{arm} found {forbidden}");
            }
        }
        assert!(!main_source.contains("fn open_drive_read("));
    }

    #[test]
    fn mu_17g_f2_lifecycle_refs_exec_interchange_leaves_are_exhaustive() {
        let cli_source = include_str!("cli.rs");
        let main_source = include_str!("main.rs");
        let refs_source = include_str!("refs_cmd.rs");
        let exec_source = include_str!("exec_cmd.rs");
        let idl_source = include_str!("../../../idl/loom.idl");
        let generated_api_source = include_str!("../../loom-remote-protocol/src/generated_api.rs");
        let remote_client_source = include_str!("../../loom-remote-client/src/generated_client.rs");
        let hosted_dispatch_source =
            include_str!("../../loom-hosted-core/src/generated_dispatch.rs");
        let service_source = include_str!("../../loom-client/src/service.rs");

        assert_eq!(
            enum_variants(cli_source, "LifecycleCmd"),
            vec![
                "DefineStandard",
                "Define",
                "Definitions",
                "Definition",
                "Instantiate",
                "Instances",
                "Instance",
                "Transition",
                "SnapshotPlan",
                "CurrentSurface",
                "Snapshots",
                "Snapshot",
                "SnapshotContent",
                "OperationLog",
            ]
        );
        assert_eq!(
            enum_variants(cli_source, "RefsCmd"),
            vec!["Reconcile", "Status"]
        );
        assert_eq!(
            enum_variants(cli_source, "ExecCmd"),
            vec!["Run", "Inspect", "Apply"]
        );
        assert_eq!(
            enum_variants(cli_source, "InterchangeCmd"),
            vec![
                "ImportFs",
                "ImportArchive",
                "ImportTableCsv",
                "ImportRedmine",
                "ImportAsana",
                "ImportJira",
                "ImportConfluence",
                "ImportSlack",
                "ImportDrive",
                "ImportMarkdown",
                "ImportNotion",
                "ExportArchive",
                "ExportFs",
                "ExportTableCsv",
                "ExportCar",
                "ImportCar",
            ]
        );

        struct Leaf<'a> {
            source: &'a str,
            runner: &'a str,
            arm: &'a str,
            interface: &'a str,
            method: &'a str,
        }

        let lifecycle = [
            Leaf {
                source: main_source,
                runner: "run_lifecycle",
                arm: "LifecycleCmd::DefineStandard",
                interface: "Lifecycle",
                method: "lifecycle_define_standard_json",
            },
            Leaf {
                source: main_source,
                runner: "run_lifecycle",
                arm: "LifecycleCmd::Define {",
                interface: "Lifecycle",
                method: "lifecycle_define_json",
            },
            Leaf {
                source: main_source,
                runner: "run_lifecycle",
                arm: "LifecycleCmd::Instantiate",
                interface: "Lifecycle",
                method: "lifecycle_instantiate_json",
            },
            Leaf {
                source: main_source,
                runner: "run_lifecycle",
                arm: "LifecycleCmd::Transition",
                interface: "Lifecycle",
                method: "lifecycle_transition_json",
            },
        ];
        let refs = [Leaf {
            source: refs_source,
            runner: "run_refs",
            arm: "RefsCmd::Reconcile",
            interface: "Refs",
            method: "refs_reconcile_json",
        }];
        let exec = [
            Leaf {
                source: exec_source,
                runner: "run_exec_cmd",
                arm: "ExecCmd::Run",
                interface: "Exec",
                method: "exec_cbor",
            },
            Leaf {
                source: exec_source,
                runner: "run_exec_cmd",
                arm: "ExecCmd::Apply",
                interface: "Exec",
                method: "apply_cbor",
            },
        ];
        let interchange = [
            Leaf {
                source: main_source,
                runner: "run_interchange",
                arm: "InterchangeCmd::ImportFs",
                interface: "FileSystem",
                method: "import_fs",
            },
            Leaf {
                source: main_source,
                runner: "run_interchange",
                arm: "InterchangeCmd::ImportArchive",
                interface: "Archive",
                method: "archive_import",
            },
            Leaf {
                source: main_source,
                runner: "run_interchange",
                arm: "InterchangeCmd::ImportTableCsv",
                interface: "InterchangeProfiles",
                method: "import_table_csv",
            },
            Leaf {
                source: main_source,
                runner: "run_interchange",
                arm: "InterchangeCmd::ImportCar",
                interface: "Car",
                method: "car_import",
            },
        ];
        let helper_imports = [
            (
                "InterchangeCmd::ImportRedmine",
                "run_redmine_import",
                "import_redmine",
            ),
            (
                "InterchangeCmd::ImportAsana",
                "run_asana_import",
                "import_asana",
            ),
            (
                "InterchangeCmd::ImportJira",
                "run_jira_import",
                "import_jira",
            ),
            (
                "InterchangeCmd::ImportConfluence",
                "run_confluence_import",
                "import_confluence",
            ),
            (
                "InterchangeCmd::ImportSlack",
                "run_slack_import",
                "import_slack",
            ),
            (
                "InterchangeCmd::ImportDrive",
                "run_drive_import",
                "import_drive",
            ),
            (
                "InterchangeCmd::ImportMarkdown",
                "run_markdown_import",
                "import_markdown",
            ),
            (
                "InterchangeCmd::ImportNotion",
                "run_notion_import",
                "import_notion",
            ),
        ];

        let forbidden = [
            "open_store_client",
            "StoreClient::",
            "cli_open_loom(",
            "cli_open_loom_read(",
            "FileStore::open",
            "Loom::new(",
            "save_loom(",
        ];
        for leaf in lifecycle
            .iter()
            .chain(refs.iter())
            .chain(exec.iter())
            .chain(interchange.iter())
        {
            let body = match_arm_body(function_body(leaf.source, leaf.runner), leaf.arm);
            assert!(
                body.contains("open_cli_generated_client"),
                "{} must open a generated client",
                leaf.arm
            );
            assert!(
                body.contains(leaf.method),
                "{} must call {}",
                leaf.arm,
                leaf.method
            );
            assert!(body.contains(&format!("\"{}\"", leaf.interface)));
            for forbidden in forbidden {
                assert!(
                    !body.contains(forbidden),
                    "{} found legacy owner {forbidden}",
                    leaf.arm
                );
            }
            assert!(idl_source.contains(&format!(" {}(", leaf.method)));
            assert!(generated_api_source.contains(&format!("fn {}(", leaf.method)));
            assert!(remote_client_source.contains(&format!("fn {}(", leaf.method)));
            assert!(
                hosted_dispatch_source
                    .contains(&format!("(\"{}\", \"{}\")", leaf.interface, leaf.method))
            );
            assert!(service_source.contains(&format!("fn {}(", leaf.method)));
        }
        for leaf in &interchange {
            let body = match_arm_body(function_body(leaf.source, leaf.runner), leaf.arm);
            assert!(
                body.contains(
                    "remote::open_cli_generated_client_for_dry_run(&store, keys, dry_run)?"
                )
            );
        }

        let run_interchange = function_body(main_source, "run_interchange");
        let expression_arm = |arm: &str| {
            let start = run_interchange.find(arm).expect("interchange arm");
            let tail = &run_interchange[start..];
            let end = tail[arm.len()..]
                .find("\n        InterchangeCmd::")
                .map(|offset| arm.len() + offset)
                .unwrap_or(tail.len());
            &tail[..end]
        };
        for (arm, helper, method) in helper_imports {
            let arm_body = expression_arm(arm);
            assert!(arm_body.contains(helper), "{arm} must delegate to {helper}");
            for forbidden in forbidden {
                assert!(
                    !arm_body.contains(forbidden),
                    "{arm} found legacy owner {forbidden}"
                );
            }
            let helper_body = function_body(main_source, helper);
            assert!(helper_body.contains("remote::open_cli_generated_client(store, keys)?"));
            assert!(helper_body.contains("\"InterchangeProfiles\""));
            assert!(helper_body.contains(&format!("\"{method}\"")));
            assert!(helper_body.contains("WireValue::Bytes"));
            for forbidden in forbidden {
                assert!(
                    !helper_body.contains(forbidden),
                    "{helper} found legacy owner {forbidden}"
                );
            }
            assert!(idl_source.contains(&format!(" {method}(")));
            assert!(generated_api_source.contains(&format!("fn {method}(")));
            assert!(remote_client_source.contains(&format!("fn {method}(")));
            assert!(
                hosted_dispatch_source
                    .contains(&format!("(\"InterchangeProfiles\", \"{method}\")"))
            );
            assert!(service_source.contains(&format!("fn {method}(")));
        }

        for arm in [
            "LifecycleCmd::Definitions",
            "LifecycleCmd::Definition",
            "LifecycleCmd::Instances",
            "LifecycleCmd::Instance",
            "LifecycleCmd::SnapshotPlan",
            "LifecycleCmd::CurrentSurface",
            "LifecycleCmd::Snapshots",
            "LifecycleCmd::Snapshot",
            "LifecycleCmd::SnapshotContent",
            "LifecycleCmd::OperationLog",
        ] {
            let body = match_arm_body(function_body(main_source, "run_lifecycle"), arm);
            assert!(body.contains("cli_open_loom_read(&store, keys)?"));
            assert!(!body.contains("save_loom("));
        }
        let refs_status = match_arm_body(function_body(refs_source, "run_refs"), "RefsCmd::Status");
        assert!(refs_status.contains("mcp_for_store(&store, keys)?"));
        assert!(!refs_status.contains("save_loom("));
        let exec_inspect = match_arm_body(
            function_body(exec_source, "run_exec_cmd"),
            "ExecCmd::Inspect",
        );
        assert!(exec_inspect.contains("decode(&raw)"));
        for forbidden in ["store", "open_generated_client", "save_loom"] {
            assert!(
                !exec_inspect.contains(forbidden),
                "Exec inspect must remain a pure local decoder"
            );
        }

        let export_archive = match_arm_body(run_interchange, "InterchangeCmd::ExportArchive");
        assert!(export_archive.contains("client.transfer_export("));
        assert!(export_archive.contains("export_archive(&loom"));
        let export_fs = match_arm_body(run_interchange, "InterchangeCmd::ExportFs");
        assert!(export_fs.contains("remote::target_is_remote(&store)?"));
        assert!(export_fs.contains("export_fs(&loom"));
        let export_table = match_arm_body(run_interchange, "InterchangeCmd::ExportTableCsv");
        assert!(export_table.contains("export_table_csv(&loom"));
        let export_car = match_arm_body(run_interchange, "InterchangeCmd::ExportCar");
        assert!(export_car.contains("client.transfer_export("));
        assert!(export_car.contains("export_car(&loom"));
    }

    #[test]
    fn mu_17g_g_post_migration_cli_inventory_and_legacy_boundary_are_exhaustive() {
        let cli_source = include_str!("cli.rs");
        let main_source = include_str!("main.rs");
        let remote_source = include_str!("remote.rs");

        assert_eq!(
            enum_variants(cli_source, "Command"),
            vec![
                "Audit",
                "Calendar",
                "Cas",
                "Capabilities",
                "Chat",
                "Certificate",
                "NetworkAccess",
                "Columnar",
                "Contacts",
                "Dataframe",
                "Daemon",
                "Context",
                "Document",
                "Refs",
                "Doctor",
                "Exec",
                "Program",
                "Files",
                "Drive",
                "Graph",
                "Kv",
                "Ledger",
                "Metrics",
                "Logs",
                "Traces",
                "Lifecycle",
                "Lock",
                "Mail",
                "Meetings",
                "Pages",
                "Tickets",
                "Lanes",
                "Management",
                "Inference",
                "Acl",
                "Identity",
                "Interchange",
                "Workspace",
                "ProtectedRef",
                "Mcp",
                "McpDaemonCliTestHoldSession",
                "Mount",
                "Queue",
                "Search",
                "Fts",
                "Serve",
                "Studio",
                "Sql",
                "Store",
                "TimeSeries",
                "Vcs",
                "Vector",
                "Llms",
                "Version",
            ]
        );

        let command_leaves = command_leaves(cli_source, "Command", "");
        let leaves = command_leaf_paths(cli_source, "Command", "");
        assert_eq!(leaves.len(), 466);
        let unique = leaves.iter().collect::<std::collections::BTreeSet<_>>();
        assert_eq!(unique.len(), leaves.len());
        let production = leaves
            .iter()
            .filter(|leaf| leaf.as_str() != "mcp-daemon-cli-test-hold-session")
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(production.len(), 465);
        assert_eq!(
            leaves
                .iter()
                .filter(|leaf| leaf.as_str() == "mcp-daemon-cli-test-hold-session")
                .count(),
            1
        );
        let carrier_lifecycle = ["daemon session open", "daemon session close"];
        for leaf in carrier_lifecycle {
            assert_eq!(
                leaves.iter().filter(|candidate| *candidate == leaf).count(),
                1
            );
        }
        let prior_production = leaves
            .iter()
            .filter(|leaf| leaf.as_str() != "mcp-daemon-cli-test-hold-session")
            .filter(|leaf| !carrier_lifecycle.contains(&leaf.as_str()))
            .count();
        assert_eq!(prior_production, 463);

        let guard_source = ownership_guard_source(remote_source);
        let classifications = authoritative_leaf_classifications(
            &command_leaves
                .into_iter()
                .filter(|leaf| leaf.path != "mcp-daemon-cli-test-hold-session")
                .collect::<Vec<_>>(),
            &guard_source,
        );
        let sets = validate_leaf_classifications(&production, &classifications)
            .unwrap_or_else(|error| panic!("invalid CLI ownership manifest: {error}"));
        assert_eq!(sets.generated.len(), 382);
        assert_eq!(sets.exceptions.len(), 83);

        let store_client = impl_body(remote_source, "StoreClient");
        assert_eq!(
            impl_method_names(store_client),
            vec!["is_remote", "transfer_export"]
        );
        assert_eq!(main_source.matches("remote::open_store_client(").count(), 2);
        let interchange = function_body(main_source, "run_interchange");
        for arm in ["InterchangeCmd::ExportArchive", "InterchangeCmd::ExportCar"] {
            let body = match_arm_body(interchange, arm);
            assert!(body.contains("remote::open_store_client(&store)?"));
            assert!(body.contains("client.transfer_export("));
            assert!(!body.contains("save_loom("));
        }
    }

    #[test]
    fn mu_17g_g_classification_validation_rejects_omitted_duplicate_and_both() {
        let production = ["alpha".to_string(), "beta".to_string()]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        let generated = |path: &str| LeafClassification {
            path: path.to_string(),
            ownership: LeafOwnership::Generated {
                interface: "Example",
                method: "read",
            },
        };
        let exception = |path: &str| LeafClassification {
            path: path.to_string(),
            ownership: LeafOwnership::Exception {
                category: "runtime",
                owner_anchor: "example.rs:1",
            },
        };

        let omitted = validate_leaf_classifications(&production, &[generated("alpha")])
            .expect_err("omitted classification must fail");
        assert!(omitted.contains("missing=[\"beta\"]"));

        let duplicate = validate_leaf_classifications(
            &production,
            &[generated("alpha"), generated("alpha"), exception("beta")],
        )
        .expect_err("duplicate classification must fail");
        assert!(duplicate.contains("duplicates={\"alpha\"}"));

        let both = validate_leaf_classifications(
            &production,
            &[generated("alpha"), exception("alpha"), exception("beta")],
        )
        .expect_err("dual classification must fail");
        assert!(both.contains("both={\"alpha\"}"));
    }

    #[test]
    fn mu_6d_graph_vector_search_mutations_use_generated_client() {
        let main_source = include_str!("main.rs");

        for (runner, arm, interface, method) in [
            ("run_graph", "GraphCmd::UpsertNode", "Graph", "upsert_node"),
            ("run_graph", "GraphCmd::RemoveNode", "Graph", "remove_node"),
            ("run_graph", "GraphCmd::UpsertEdge", "Graph", "upsert_edge"),
            ("run_graph", "GraphCmd::RemoveEdge", "Graph", "remove_edge"),
            ("run_vector", "VectorCmd::Create", "Vector", "create"),
            ("run_vector", "VectorCmd::Upsert", "Vector", "upsert"),
            (
                "run_vector",
                "VectorCmd::UpsertSource",
                "Vector",
                "upsert_source",
            ),
            (
                "run_vector",
                "VectorCmd::CreateIndex",
                "Vector",
                "create_metadata_index",
            ),
            (
                "run_vector",
                "VectorCmd::DropIndex",
                "Vector",
                "drop_metadata_index",
            ),
            ("run_vector", "VectorCmd::Delete", "Vector", "delete"),
            ("run_search", "SearchCmd::Create", "Search", "create"),
            ("run_search", "SearchCmd::Index", "Search", "index"),
            ("run_search", "SearchCmd::Delete", "Search", "delete"),
            ("run_search", "SearchCmd::Remap", "Search", "remap"),
        ] {
            let body = match_arm_body(function_body(main_source, runner), arm);
            assert!(
                body.contains("remote::open_cli_generated_client(&store, keys)?"),
                "{arm} must use the generated client"
            );
            assert!(
                body.contains(&format!("\"{interface}\"")),
                "{arm} must dispatch through generated interface {interface}"
            );
            assert!(
                body.contains(&format!("\"{method}\"")),
                "{arm} must dispatch through generated method {method}"
            );
            assert!(
                !body.contains("remote::open_store_client"),
                "{arm} must not bypass generated dispatch through StoreClient"
            );
            assert!(
                !body.contains("cli_open_loom("),
                "{arm} must not open a writable Loom directly"
            );
            assert!(
                !body.contains("FileStore::open("),
                "{arm} must not open a writable FileStore directly"
            );
        }
    }

    #[test]
    fn mu_6e_columnar_files_vcs_interchange_mutations_use_generated_client() {
        let main_source = include_str!("main.rs");
        let cases: &[(&str, &str, &str, &[&str])] = &[
            (
                "run_columnar",
                "ColumnarCmd::Create",
                "Columnar",
                &["create"],
            ),
            (
                "run_columnar",
                "ColumnarCmd::Append",
                "Columnar",
                &["append"],
            ),
            (
                "run_columnar",
                "ColumnarCmd::Compact",
                "Columnar",
                &["compact"],
            ),
            (
                "run_files",
                "FilesCmd::Delete",
                "FileSystem",
                &["stat", "remove_directory", "remove_file"],
            ),
            (
                "run_files",
                "FilesCmd::Mkdir",
                "FileSystem",
                &["create_directory"],
            ),
            (
                "run_files",
                "FilesCmd::Write",
                "FileSystem",
                &["create_directory", "write_file"],
            ),
            ("run_vcs", "VcsCmd::Branch", "VersionControl", &["branch"]),
            ("run_vcs", "VcsCmd::Commit", "VersionControl", &["commit"]),
            (
                "run_vcs",
                "VcsCmd::Checkout",
                "VersionControl",
                &["checkout"],
            ),
            ("run_vcs", "VcsCmd::Merge", "VersionControl", &["merge"]),
            (
                "run_interchange",
                "InterchangeCmd::ImportFs",
                "FileSystem",
                &["import_fs"],
            ),
            (
                "run_interchange",
                "InterchangeCmd::ImportCar",
                "Car",
                &["car_import"],
            ),
        ];

        for (runner, arm, interface, methods) in cases {
            let body = match_arm_body(function_body(main_source, runner), arm);
            assert!(
                body.contains("remote::open_cli_generated_client(&store, keys)?"),
                "{arm} must use the generated client"
            );
            assert!(
                body.contains(&format!("\"{interface}\"")),
                "{arm} must dispatch through generated interface {interface}"
            );
            for method in *methods {
                assert!(
                    body.contains(&format!("\"{method}\"")),
                    "{arm} must dispatch through generated method {method}"
                );
            }
            assert!(
                !body.contains("remote::open_store_client"),
                "{arm} must not bypass generated dispatch through StoreClient"
            );
            assert!(
                !body.contains("cli_open_loom("),
                "{arm} must not open a writable Loom directly"
            );
            assert!(
                !body.contains("FileStore::open("),
                "{arm} must not open a writable FileStore directly"
            );
        }
    }

    #[test]
    fn mu_6e_interchange_import_options_remain_on_generated_paths() {
        let main_source = include_str!("main.rs");
        let run_interchange = function_body(main_source, "run_interchange");

        let import_fs = match_arm_body(run_interchange, "InterchangeCmd::ImportFs");
        assert!(import_fs.contains("\"FileSystem\""));
        assert!(import_fs.contains("\"import_fs\""));
        assert!(import_fs.contains("Some(author).to_value()"));
        assert!(import_fs.contains("Some(message).to_value()"));
        assert!(!import_fs.contains("FsImportOptions::new(&src)"));
        assert!(!import_fs.contains("import_fs(loom.loom_mut()"));
        assert!(!import_fs.contains("CliImportLoom"));

        let import_archive = match_arm_body(run_interchange, "InterchangeCmd::ImportArchive");
        assert!(import_archive.contains("\"Archive\""));
        assert!(import_archive.contains("\"archive_import\""));
        assert!(import_archive.contains("gzip_output_path.to_value()"));
        assert!(import_archive.contains("commit.to_value()"));
        assert!(import_archive.contains("Some(author).to_value()"));
        assert!(import_archive.contains("Some(message).to_value()"));
        assert!(import_archive.contains("print_archive_import_result(&result, &format)"));
        assert!(!import_archive.contains("ArchiveImportOptions::new(&archive)"));
        assert!(!import_archive.contains("client.transfer_import("));
        assert!(!import_archive.contains("import_archive(loom.loom_mut()"));
    }

    #[test]
    fn mu_6e_custom_import_options_execute_on_existing_paths() {
        let store = temp_store("custom-import-options");
        let fs = FileStore::create_with_profile(&store, Algo::Blake3).expect("create store");
        let mut loom = open_loom_from(fs, &KeyOpts::default(), false).expect("open store");
        ensure_facet_workspace(&mut loom, "files", FacetKind::Files).expect("create workspace");
        save_loom(&mut loom).expect("save workspace");
        drop(loom);

        let source = temp_dir("custom-import-fs-source");
        std::fs::write(source.join("note.txt"), b"hello").expect("write source file");
        run_interchange(
            InterchangeCmd::ImportFs {
                store: store.clone(),
                workspace: "files".to_string(),
                src: source.to_string_lossy().into_owned(),
                commit: true,
                dry_run: true,
                author: "custom-author".to_string(),
                message: "custom message".to_string(),
                format: "json".to_string(),
            },
            &KeyOpts::default(),
        )
        .expect("custom filesystem import options should execute");

        let archive = source.join("note.txt.gz");
        std::fs::write(
            &archive,
            hex_bytes("1f8b08000000000002ff4bcecf2d284a2d2e4e4d01001e4b56970a000000"),
        )
        .expect("write gzip archive");
        run_interchange(
            InterchangeCmd::ImportArchive {
                store,
                workspace: "files".to_string(),
                archive: archive.to_string_lossy().into_owned(),
                kind: "gzip".to_string(),
                gzip_output_path: Some("custom/out.txt".to_string()),
                commit: true,
                dry_run: true,
                author: "custom-author".to_string(),
                message: "custom archive message".to_string(),
                format: "json".to_string(),
            },
            &KeyOpts::default(),
        )
        .expect("custom archive import options should execute");
    }

    #[test]
    fn mu_6f_management_identity_acl_protected_ref_mutations_use_generated_client() {
        let management_source = include_str!("management_cmd.rs");
        let main_source = include_str!("main.rs");

        assert!(
            function_body(main_source, "run")
                .contains("Command::Identity { action } => run_identity(action, keys)")
        );
        assert!(
            function_body(management_source, "run_management")
                .contains("ManagementCmd::Identity { action } => run_identity(action, keys)")
        );

        let cases: &[(&str, &str, &str, &[&str])] = &[
            (
                "run_management_workspace",
                "WorkspaceCmd::Create",
                "Workspaces",
                &["workspace_create"],
            ),
            (
                "run_management_workspace",
                "WorkspaceCmd::Rename",
                "Workspaces",
                &["workspace_rename"],
            ),
            (
                "run_management_workspace",
                "WorkspaceCmd::Delete",
                "Workspaces",
                &["workspace_delete"],
            ),
            (
                "run_identity",
                "IdentityCmd::Add",
                "Identity",
                &["identity_add_principal"],
            ),
            (
                "run_identity",
                "IdentityCmd::RenameHandle",
                "Identity",
                &["identity_rename_principal_handle"],
            ),
            (
                "run_identity",
                "IdentityCmd::SetPassphrase",
                "Identity",
                &["identity_set_passphrase"],
            ),
            (
                "run_identity",
                "IdentityCmd::Remove {",
                "Identity",
                &["identity_remove_principal"],
            ),
            (
                "run_identity",
                "IdentityCmd::AssignRole",
                "Identity",
                &["identity_assign_role"],
            ),
            (
                "run_identity",
                "IdentityCmd::RevokeRole",
                "Identity",
                &["identity_revoke_role"],
            ),
            ("run_acl", "AclCmd::Grant", "Acl", &["acl_grant"]),
            ("run_acl", "AclCmd::Revoke", "Acl", &["acl_revoke"]),
            (
                "run_protected_ref",
                "ProtectedRefCmd::Set",
                "ProtectedRefs",
                &["protected_ref_set"],
            ),
            (
                "run_protected_ref",
                "ProtectedRefCmd::Remove",
                "ProtectedRefs",
                &["protected_ref_remove"],
            ),
            (
                "run_management_kv_config",
                "ManagementKvConfigCmd::Set",
                "ManagementKv",
                &["set_config"],
            ),
        ];

        for (runner, arm, interface, methods) in cases {
            let body = match_arm_body(function_body(management_source, runner), arm);
            assert!(
                body.contains("crate::remote::open_cli_generated_client(&store, keys)?"),
                "{arm} must use the generated client"
            );
            assert!(
                body.contains(&format!("\"{interface}\"")),
                "{arm} must dispatch through generated interface {interface}"
            );
            for method in *methods {
                assert!(
                    body.contains(&format!("\"{method}\"")),
                    "{arm} must dispatch through generated method {method}"
                );
            }
            assert!(
                !body.contains("crate::remote::open_store_client"),
                "{arm} must not bypass generated dispatch through StoreClient"
            );
            assert!(
                !body.contains("cli_open_loom("),
                "{arm} must not open a writable Loom directly"
            );
            assert!(
                !body.contains("FileStore::open("),
                "{arm} must not open a writable FileStore directly"
            );
        }

        for (runner, arm, helper, methods) in [
            (
                "run_identity",
                "IdentityCmd::CreateAppCredential",
                "generated_app_credential_create",
                &["identity_create_app_credential"][..],
            ),
            (
                "run_identity",
                "IdentityCmd::RevokeAppCredential",
                "generated_app_credential_revoke",
                &["identity_revoke_app_credential"],
            ),
            (
                "run_identity",
                "IdentityCmd::CreateExternalCredential",
                "generated_external_credential_create",
                &["identity_create_external_credential"],
            ),
            (
                "run_identity",
                "IdentityCmd::RevokeExternalCredential",
                "generated_external_credential_revoke",
                &["identity_revoke_external_credential"],
            ),
            (
                "run_identity_public_key",
                "IdentityPublicKeyCmd::Add",
                "generated_public_key_add",
                &["identity_add_public_key"],
            ),
            (
                "run_identity_public_key",
                "IdentityPublicKeyCmd::Revoke",
                "generated_public_key_revoke",
                &["identity_revoke_public_key"],
            ),
        ] {
            let body = match_arm_body(function_body(management_source, runner), arm);
            assert!(body.contains("crate::remote::open_cli_generated_client(&store, keys)?"));
            assert!(body.contains(helper));
            let helper_body = function_body(management_source, helper);
            assert!(helper_body.contains("\"Identity\""));
            for method in methods {
                assert!(
                    helper_body.contains(&format!("\"{method}\"")),
                    "{helper} must dispatch through generated method {method}"
                );
            }
            assert!(!body.contains("crate::remote::open_store_client"));
            assert!(!body.contains("cli_open_loom("));
            assert!(!body.contains("FileStore::open("));
        }

        let identity_snapshot = function_body(management_source, "generated_identity_snapshot");
        assert!(identity_snapshot.contains("\"Identity\""));
        assert!(identity_snapshot.contains("\"identity_list\""));

        let workspace_resolver = function_body(management_source, "generated_workspace_id");
        assert!(workspace_resolver.contains("\"Workspaces\""));
        assert!(workspace_resolver.contains("\"workspace_list\""));

        for (runner, arm, method) in [
            (
                "run_identity",
                "IdentityCmd::ForceDetachAuthority",
                "identity_force_detach_authority_json",
            ),
            (
                "run_identity",
                "IdentityCmd::ReplicateAuthority",
                "identity_replicate_authority_json",
            ),
            (
                "run_identity",
                "IdentityCmd::ConfigureAuthorityReplication",
                "identity_configure_authority_replication_json",
            ),
            (
                "run_identity",
                "IdentityCmd::RemoveAuthorityReplication",
                "identity_remove_authority_replication_json",
            ),
        ] {
            let body = match_arm_body(function_body(management_source, runner), arm);
            assert!(body.contains("crate::remote::open_cli_generated_client(&store, keys)?"));
            assert!(body.contains("\"Identity\""));
            assert!(body.contains(&format!("\"{method}\"")));
            assert!(!body.contains("cli_open_loom("));
            assert!(!body.contains("FileStore::authority_replication_policy("));
            assert!(!body.contains("save_identity_store_audited("));
            assert!(!body.contains("save_authority_replication_policy_audited("));
            assert!(!body.contains("remove_authority_replication_policy_audited("));
        }

        for (arm, helper, method) in [
            (
                "ManagementKvConfigCmd::Set",
                "crate::remote::open_cli_generated_client(&store, keys)?",
                "set_config",
            ),
            (
                "ManagementKvConfigCmd::Get",
                "crate::remote::open_cli_read_only_generated_client(&store, keys)?",
                "get_config",
            ),
        ] {
            let body = match_arm_body(
                function_body(management_source, "run_management_kv_config"),
                arm,
            );
            assert!(body.contains(helper));
            assert!(body.contains("\"ManagementKv\""));
            assert!(body.contains(&format!("\"{method}\"")));
            assert!(!body.contains("cli_open_loom("));
            assert!(!body.contains("cli_open_loom_read("));
            assert!(!body.contains("FileStore::open("));
        }
    }

    #[test]
    fn mu_6h_i_c_exec_apply_and_meetings_import_use_generated_clients() {
        let exec_source = include_str!("exec_cmd.rs");
        let main_source = include_str!("main.rs");
        let remote_source = include_str!("remote.rs");
        let local_source = include_str!("../../loom-client/src/local.rs");
        let service_source = include_str!("../../loom-client/src/service.rs");
        let hosted_dispatch_source =
            include_str!("../../loom-hosted-core/src/generated_dispatch.rs");
        let remote_client_source = include_str!("../../loom-remote-client/src/generated_client.rs");
        let generated_api_source = include_str!("../../loom-remote-protocol/src/generated_api.rs");
        let generated_registry_source = include_str!("../../loom-remote-protocol/src/generated.rs");

        let exec_run = match_arm_body(function_body(exec_source, "run_exec_cmd"), "ExecCmd::Run");
        assert!(exec_run.contains("remote::open_cli_generated_client(&store, keys)?"));
        assert!(exec_run.contains("\"Exec\""));
        assert!(exec_run.contains("\"exec_cbor\""));
        assert!(!exec_run.contains("cli_open_loom(&store, keys)?"));
        assert!(!exec_run.contains("loom_compute::execute_cbor("));
        assert!(!exec_run.contains("save_loom("));

        let exec_apply =
            match_arm_body(function_body(exec_source, "run_exec_cmd"), "ExecCmd::Apply");
        assert!(exec_apply.contains("remote::open_cli_generated_client(&store, keys)?"));
        assert!(exec_apply.contains("\"Exec\""));
        assert!(exec_apply.contains("\"apply_cbor\""));
        assert!(!exec_apply.contains("cli_open_loom(&store, keys)?"));
        assert!(!exec_apply.contains("loom_compute::apply("));
        assert!(!exec_apply.contains("save_loom("));

        let sql_exec = match_arm_body(function_body(main_source, "run_sql_cmd"), "SqlCmd::Exec");
        assert!(sql_exec.contains("remote::open_cli_generated_client(&store, keys)?"));
        assert!(sql_exec.contains("\"Sql\""));
        assert!(sql_exec.contains("\"sql_exec_result\""));
        assert!(sql_exec.contains("print_sql_exec_result_cbor(&encoded)"));
        assert!(!sql_exec.contains("cli_open_loom"));
        assert!(!sql_exec.contains("cli_open_loom_read"));
        assert!(!sql_exec.contains("LoomSqlStore::open_write("));
        assert!(!sql_exec.contains("Glue::new("));
        assert!(!sql_exec.contains("save_loom("));
        let sql_presenter = function_body(main_source, "print_sql_exec_result_cbor");
        assert!(sql_presenter.contains("loom_result::result_view::decode(bytes)"));
        assert!(sql_presenter.contains("ResultPayload::Statements"));
        assert!(sql_presenter.contains("print_sql_payload_value(payload)?"));
        assert!(sql_presenter.contains("returned corrupt reader payload"));
        let sql_payload_presenter = function_body(main_source, "print_sql_payload_value");
        assert!(sql_payload_presenter.contains("sql_payload_from_result_statement"));
        assert!(sql_payload_presenter.contains("print_payload(&payload)"));
        let sql_value_bridge = function_body(main_source, "sql_gluesql_value_from_tabular");
        assert!(sql_value_bridge.contains("loom_sql::value_from_tabular(value)"));
        let sql_statement_bridge = function_body(main_source, "sql_payload_from_result_statement");
        assert!(sql_statement_bridge.contains("Statement::SelectMap"));
        assert!(sql_statement_bridge.contains("Statement::ShowColumns"));
        assert!(sql_statement_bridge.contains("loom_sql::data_type_from_result_label"));
        assert!(!sql_statement_bridge.contains("other =>"));
        assert!(!sql_statement_bridge.contains("println!(\"{other:?}\")"));

        let import_fs = match_arm_body(
            function_body(main_source, "run_interchange"),
            "InterchangeCmd::ImportFs",
        );
        assert!(
            import_fs
                .contains("remote::open_cli_generated_client_for_dry_run(&store, keys, dry_run)?")
        );
        assert!(import_fs.contains("\"FileSystem\""));
        assert!(import_fs.contains("\"import_fs\""));
        assert!(import_fs.contains("Some(author).to_value()"));
        assert!(import_fs.contains("Some(message).to_value()"));
        assert!(!import_fs.contains("FsImportOptions::new"));
        assert!(!import_fs.contains("import_fs("));

        let import_archive = match_arm_body(
            function_body(main_source, "run_interchange"),
            "InterchangeCmd::ImportArchive",
        );
        assert!(
            import_archive
                .contains("remote::open_cli_generated_client_for_dry_run(&store, keys, dry_run)?")
        );
        assert!(import_archive.contains("\"Archive\""));
        assert!(import_archive.contains("\"archive_import\""));
        assert!(import_archive.contains("gzip_output_path.to_value()"));
        assert!(import_archive.contains("Some(author).to_value()"));
        assert!(import_archive.contains("Some(message).to_value()"));
        assert!(import_archive.contains("generated_archive_import_result_from_cbor"));
        assert!(!import_archive.contains("ArchiveImportOptions::new"));
        assert!(!import_archive.contains("import_archive("));

        let meetings_import = match_arm_body(
            function_body(main_source, "run_meetings"),
            "MeetingsCmd::Import",
        );
        assert!(meetings_import.contains("remote::open_cli_generated_client(&store, keys)?"));
        assert!(meetings_import.contains("\"Meetings\""));
        assert!(meetings_import.contains("\"meetings_import_snapshot\""));
        assert!(meetings_import.contains("input_profile_label(input_profile).to_value()"));
        assert!(meetings_import.contains("WireValue::Bytes(bytes)"));
        assert!(!meetings_import.contains("cli_open_loom(&store, keys)?"));
        assert!(!meetings_import.contains("ensure_facet_workspace("));
        assert!(!meetings_import.contains("import_meetings_bytes("));
        assert!(!meetings_import.contains("save_loom("));

        let local_apply = function_body(local_source, "apply_cbor");
        assert!(local_apply.contains("decode_exec_apply_request(request)?"));
        assert!(local_apply.contains("fork_state_into(loom_core::provider::PlanningObjectStore"));
        assert!(local_apply.contains("apply("));
        assert!(local_apply.contains("save_exec_apply_candidate("));
        assert!(local_apply.contains("import_engine_state_preserving_mutable_overlay("));
        assert!(!local_apply.contains("cli_open_loom("));
        assert!(!local_apply.contains("save_loom("));

        let local_meetings = function_body(local_source, "meetings_import_snapshot");
        assert!(local_meetings.contains("PlanningObjectStore::new(loom.store())"));
        assert!(local_meetings.contains("ensure_generated_facet_workspace("));
        assert!(
            local_meetings.contains("authorize(workspace_id, FacetKind::Vcs, AclRight::Write)")
        );
        assert!(local_meetings.contains("plan_meetings_import("));
        assert!(local_meetings.contains("commit_workflow_transaction(WorkflowTransaction"));
        assert!(local_meetings.contains("import_engine_state_preserving_mutable_overlay("));
        assert!(!local_meetings.contains("cli_open_loom("));
        assert!(!local_meetings.contains("import_meetings_bytes("));
        assert!(!local_meetings.contains("save_loom("));

        let generated_execute = function_body(remote_source, "execute_unary");
        assert!(generated_execute.contains("Self::DirectLocal { client, handle }"));
        assert!(generated_execute.contains("loom_hosted_core::generated_dispatch::dispatch"));
        assert!(generated_execute.contains("Self::DaemonLocal(store)"));
        assert!(generated_execute.contains("store.generated_unary("));
        assert!(generated_execute.contains("Self::Remote(remote)"));
        assert!(generated_execute.contains("remote.client.call("));
        let generated_resolve = function_body(remote_source, "resolve_workspace_id");
        assert!(generated_resolve.contains("\"Workspaces\""));
        assert!(generated_resolve.contains("\"workspace_list\""));
        assert!(generated_resolve.contains("self.execute_unary("));

        assert_eq!(
            service_source
                .matches("impl Exec for LocalLoomClient")
                .count(),
            1
        );
        assert_eq!(
            service_source
                .matches("impl Meetings for LocalLoomClient")
                .count(),
            1
        );
        assert_eq!(generated_api_source.matches("fn apply_cbor(").count(), 1);
        assert_eq!(
            generated_api_source
                .matches("fn meetings_import_snapshot(")
                .count(),
            1
        );
        assert_eq!(remote_client_source.matches("fn apply_cbor(").count(), 1);
        assert_eq!(
            remote_client_source
                .matches("fn meetings_import_snapshot(")
                .count(),
            1
        );
        assert_eq!(
            remote_client_source
                .matches(".call(\"Exec\", \"apply_cbor\"")
                .count(),
            1
        );
        assert_eq!(
            remote_client_source
                .matches(".call(\n                    \"Meetings\",\n                    \"meetings_import_snapshot\",")
                .count(),
            1
        );
        assert_eq!(
            hosted_dispatch_source
                .matches("(\"Exec\", \"apply_cbor\")")
                .count(),
            1
        );
        assert_eq!(
            hosted_dispatch_source
                .matches("(\"Meetings\", \"meetings_import_snapshot\")")
                .count(),
            1
        );
        assert_eq!(
            generated_registry_source
                .matches("Self::ExecApplyCbor => (\"Exec\", \"apply_cbor\")")
                .count(),
            1
        );
        assert_eq!(
            generated_registry_source
                .matches(
                    "Self::MeetingsMeetingsImportSnapshot => (\"Meetings\", \"meetings_import_snapshot\")"
                )
                .count(),
            1
        );
        assert_eq!(
            generated_registry_source
                .matches("method: \"apply_cbor\"")
                .count(),
            1
        );
        assert_eq!(
            generated_registry_source
                .matches("method: \"meetings_import_snapshot\"")
                .count(),
            1
        );
    }

    #[test]
    fn mu_6i_c2_chat_mutations_use_generated_clients() {
        let main_source = include_str!("main.rs");
        let remote_source = include_str!("remote.rs");
        let run_chat = function_body(main_source, "run_chat");
        let mutations = [
            ("ChatCmd::CreateChannel", "chat_create_channel_json"),
            ("ChatCmd::RenameChannel", "chat_rename_channel_json"),
            ("ChatCmd::UpdateCursor", "chat_update_cursor_json"),
            ("ChatCmd::Post", "chat_post_message_bytes_json"),
            ("ChatCmd::Edit", "chat_edit_message_bytes_json"),
            ("ChatCmd::Redact", "chat_redact_message_json"),
            ("ChatCmd::CreateThread", "chat_create_thread_json"),
            ("ChatCmd::CreateTask", "chat_create_task_json"),
            ("ChatCmd::ClaimTask", "chat_claim_task_json"),
            ("ChatCmd::CompleteTask", "chat_complete_task_json"),
            ("ChatCmd::InvokeAgent", "chat_invoke_agent_bytes_json"),
            ("ChatCmd::AgentReply", "chat_agent_reply_json"),
            ("ChatCmd::RequestHandoff", "chat_request_handoff_json"),
            ("ChatCmd::AddReaction", "chat_add_reaction_json"),
            ("ChatCmd::RemoveReaction", "chat_remove_reaction_json"),
            ("ChatCmd::EmojiRegister", "chat_emoji_register_json"),
            ("ChatCmd::EmojiUnregister", "chat_emoji_unregister_json"),
        ];
        for (arm, method) in mutations {
            let body = match_arm_body(run_chat, arm);
            assert!(body.contains("generated_workspace_context(&store, &workspace, keys)?"));
            assert!(body.contains("chat_workspace_id.to_value()"));
            assert!(body.contains(method), "{arm} missing {method}");
            if arm == "ChatCmd::Edit" {
                assert!(body.contains("expected_entity_tag.to_value()"));
            } else {
                assert!(body.contains("WireValue::Null"));
            }
            assert!(!body.contains("cli_open_loom"));
            assert!(!body.contains("resolve_ns"));
            for direct_mutation in [
                "loom_chat::ensure_channel",
                "loom_chat::rename_channel",
                "loom_chat::update_cursor",
                "loom_chat::post_message",
                "loom_chat::edit_message",
                "loom_chat::redact_message",
                "loom_chat::create_thread",
                "loom_chat::create_task",
                "loom_chat::claim_task",
                "loom_chat::complete_task",
                "loom_chat::invoke_agent",
                "loom_chat::agent_reply",
                "loom_chat::request_handoff",
                "loom_chat::add_reaction",
                "loom_chat::remove_reaction",
                "loom_chat::register_emoji",
                "loom_chat::unregister_emoji",
            ] {
                assert!(!body.contains(direct_mutation));
            }
            assert!(!body.contains("save_loom("));
        }

        let post = match_arm_body(run_chat, "ChatCmd::Post");
        assert!(post.contains("\"chat_post_message_bytes_json\""));
        assert!(!post.contains("\"chat_post_message_json\""));
        assert!(post.contains("WireValue::Bytes(body)"));
        let edit = match_arm_body(run_chat, "ChatCmd::Edit");
        assert!(edit.contains("\"chat_edit_message_bytes_json\""));
        assert!(!edit.contains("\"chat_edit_message_json\""));
        assert!(edit.contains("WireValue::Bytes(body)"));
        assert!(edit.contains("expected_entity_tag.to_value()"));
        let invoke = match_arm_body(run_chat, "ChatCmd::InvokeAgent");
        assert!(invoke.contains("\"chat_invoke_agent_bytes_json\""));
        assert!(!invoke.contains("\"chat_invoke_agent_json\""));
        assert!(invoke.contains("WireValue::Bytes(prompt)"));
        assert!(invoke.contains("serde_json::to_string(&source_message_ids)"));

        assert!(!main_source.contains("fn chat_write"));
        let context = function_body(main_source, "generated_workspace_context");
        assert!(context.contains("remote::open_cli_generated_client(store, keys)?"));
        assert!(context.contains("client.resolve_workspace_id(workspace)?.to_string()"));
        let decoder = function_body(main_source, "execute_generated_json");
        assert!(decoder.contains("execute_generated_string(client, interface, method, args)?"));
        assert!(decoder.contains("serde_json::from_str(&json)"));
        let resolver = function_body(remote_source, "resolve_workspace_id");
        assert!(resolver.contains("\"Workspaces\""));
        assert!(resolver.contains("\"workspace_list\""));
        assert!(resolver.contains("cli_workspace_infos_from_generated_records(&records)?"));
        assert!(resolver.contains("cli_select_workspace_id(&infos, workspace)"));
        assert!(remote_source.contains("fn cli_workspace_infos_from_remote_records"));
        assert!(remote_source.contains("fn cli_select_workspace_id"));
    }

    #[test]
    fn mu_6i_c3_drive_mutations_use_generated_clients() {
        let main_source = include_str!("main.rs");
        let run_drive = function_body(main_source, "run_drive");
        let mutations = [
            ("DriveCmd::GrantShare", "drive_grant_share_json"),
            ("DriveCmd::RevokeShare", "drive_revoke_share_json"),
            (
                "DriveCmd::ApplyShareExpiry",
                "drive_apply_share_expiry_json",
            ),
            ("DriveCmd::PinRetention", "drive_pin_retention_json"),
            ("DriveCmd::UnpinRetention", "drive_unpin_retention_json"),
            ("DriveCmd::ApplyRetention", "drive_apply_retention_json"),
            ("DriveCmd::CreateFolder", "drive_create_folder_json"),
            ("DriveCmd::CreateUpload", "drive_create_upload_json"),
            ("DriveCmd::UploadChunk", "drive_upload_chunk_json"),
            ("DriveCmd::CommitUpload", "drive_commit_upload_json"),
            ("DriveCmd::Rename", "drive_rename_json"),
            ("DriveCmd::Move", "drive_move_json"),
            ("DriveCmd::Delete", "drive_delete_json"),
            ("DriveCmd::ResolveConflict", "drive_resolve_conflict_json"),
        ];
        for (arm, method) in mutations {
            let body = match_arm_body(run_drive, arm);
            assert!(body.contains("generated_workspace_context(&store, &workspace, keys)?"));
            assert!(body.contains("drive_workspace_id.to_value()"));
            assert!(body.contains("execute_generated_json::<loom_drive::"));
            assert!(body.contains("\"Drive\""));
            assert!(body.contains(method), "{arm} missing {method}");
            assert!(!body.contains("open_drive_write"));
            assert!(!body.contains("cli_open_loom"));
            assert!(!body.contains("resolve_ns"));
            for direct_mutation in [
                "loom_drive::grant_share",
                "loom_drive::revoke_share",
                "loom_drive::apply_share_expiry",
                "loom_drive::pin_retention",
                "loom_drive::unpin_retention",
                "loom_drive::apply_retention",
                "loom_drive::create_folder",
                "loom_drive::create_upload",
                "loom_drive::upload_chunk",
                "loom_drive::commit_upload",
                "loom_drive::rename_node",
                "loom_drive::move_node",
                "loom_drive::delete_node",
                "loom_drive::resolve_conflict",
            ] {
                assert!(!body.contains(direct_mutation));
            }
            assert!(!body.contains("save_loom("));
        }

        let upload_chunk = match_arm_body(run_drive, "DriveCmd::UploadChunk");
        assert!(upload_chunk.contains("read_input(&input)"));
        assert!(upload_chunk.contains("WireValue::Bytes(bytes)"));
        let resolve = match_arm_body(run_drive, "DriveCmd::ResolveConflict");
        assert!(resolve.contains("parse_drive_conflict_resolution(&resolution)?"));
        assert!(resolve.contains("resolution.to_value()"));
        assert!(!main_source.contains("fn open_drive_write"));
        let context = function_body(main_source, "generated_workspace_context");
        assert!(context.contains("remote::open_cli_generated_client(store, keys)?"));
        assert!(context.contains("client.resolve_workspace_id(workspace)?.to_string()"));
        let decoder = function_body(main_source, "execute_generated_json");
        assert!(decoder.contains("execute_generated_string(client, interface, method, args)?"));
        assert!(decoder.contains("serde_json::from_str(&json)"));
    }

    fn hex_bytes(hex: &str) -> Vec<u8> {
        assert!(hex.len().is_multiple_of(2));
        (0..hex.len())
            .step_by(2)
            .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).expect("hex byte"))
            .collect()
    }

    fn enum_variants(source: &str, enum_name: &str) -> Vec<String> {
        enum_body(source, enum_name)
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim_start();
                if !trimmed
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_uppercase)
                {
                    return None;
                }
                let name = trimmed
                    .split(|ch: char| ch == '{' || ch == '(' || ch == ',' || ch.is_whitespace())
                    .next()
                    .expect("variant name");
                Some(name.to_string())
            })
            .collect()
    }

    fn enum_variant_blocks<'a>(source: &'a str, enum_name: &str) -> Vec<(&'a str, &'a str)> {
        let body = enum_body(source, enum_name);
        let mut starts = Vec::new();
        let mut offset = 0usize;
        for line in body.split_inclusive('\n') {
            if line.starts_with("    ")
                && !line.starts_with("        ")
                && line.as_bytes().get(4).is_some_and(u8::is_ascii_uppercase)
            {
                let name_end = line[4..]
                    .find(|ch: char| ch == '{' || ch == '(' || ch == ',' || ch.is_whitespace())
                    .expect("variant name end");
                starts.push((offset, &line[4..4 + name_end]));
            }
            offset += line.len();
        }
        starts
            .iter()
            .enumerate()
            .map(|(index, (start, name))| {
                let end = starts
                    .get(index + 1)
                    .map_or(body.len(), |(next_start, _)| *next_start);
                (*name, &body[*start..end])
            })
            .collect()
    }

    fn command_leaf_paths(source: &str, enum_name: &str, prefix: &str) -> Vec<String> {
        command_leaves(source, enum_name, prefix)
            .into_iter()
            .map(|leaf| leaf.path)
            .collect()
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CommandLeaf<'a> {
        path: String,
        enum_name: &'a str,
        variant: &'a str,
    }

    fn command_leaves<'a>(
        source: &'a str,
        enum_name: &'a str,
        prefix: &str,
    ) -> Vec<CommandLeaf<'a>> {
        let mut leaves = Vec::new();
        for (variant, block) in enum_variant_blocks(source, enum_name) {
            let command_name =
                variant
                    .char_indices()
                    .fold(String::new(), |mut output, (index, ch)| {
                        if index > 0
                            && ch.is_ascii_uppercase()
                            && variant[..index]
                                .chars()
                                .next_back()
                                .is_some_and(|previous| {
                                    previous.is_ascii_lowercase() || previous.is_ascii_digit()
                                })
                        {
                            output.push('-');
                        }
                        output.push(ch.to_ascii_lowercase());
                        output
                    });
            let path = if prefix.is_empty() {
                command_name
            } else {
                format!("{prefix} {command_name}")
            };
            if block.contains("#[command(subcommand)]") {
                let action = block
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("action:"))
                    .expect("subcommand action")
                    .trim()
                    .trim_end_matches(',');
                leaves.extend(command_leaves(source, action, &path));
            } else {
                leaves.push(CommandLeaf {
                    path,
                    enum_name,
                    variant,
                });
            }
        }
        leaves
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum LeafOwnership<'a> {
        Generated {
            interface: &'a str,
            method: &'a str,
        },
        Exception {
            category: &'a str,
            owner_anchor: &'a str,
        },
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct LeafClassification<'a> {
        path: String,
        ownership: LeafOwnership<'a>,
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ClassificationSets {
        generated: std::collections::BTreeSet<String>,
        exceptions: std::collections::BTreeSet<String>,
    }

    fn exception_classifications() -> Vec<LeafClassification<'static>> {
        let analytical = [
            (
                "vector text upsert",
                "external-runtime",
                "main.rs:9012-9190",
            ),
            ("vector text query", "external-runtime", "main.rs:9012-9190"),
            ("fts rebuild", "external-cache", "main.rs:11887-12219"),
            ("search", "read-only-diagnostic", "main.rs:12509-12535"),
            (
                "columnar export-arrow",
                "physical-file",
                "main.rs:11575-11647",
            ),
            (
                "columnar export-parquet",
                "physical-file",
                "main.rs:11575-11647",
            ),
        ];
        let store = [
            (
                "store bundle-export",
                "physical-file",
                "main.rs:12622-13096",
            ),
            ("store clone", "physical-file", "main.rs:12622-13096"),
            ("store copy", "physical-file", "main.rs:12622-13096"),
            ("store get", "read-only-diagnostic", "main.rs:12810-12831"),
            ("store hash", "physical-file", "main.rs:12622-13096"),
            ("store init", "physical-file", "main.rs:12622-13096"),
            ("store put", "physical-file", "main.rs:12994-13017"),
            (
                "store attribution",
                "read-only-diagnostic",
                "main.rs:13046-13052",
            ),
            (
                "store preflight-replacement",
                "physical-file",
                "main.rs:13053-13070",
            ),
            ("store replace", "physical-file", "main.rs:13071-13096"),
        ];
        let sql_meetings = [
            ("sql table blame", "read-only-diagnostic", "table_cmd.rs"),
            ("sql table diff", "read-only-diagnostic", "table_cmd.rs"),
            ("meetings list", "read-only-diagnostic", "main.rs:2472-2549"),
            ("meetings get", "read-only-diagnostic", "main.rs:2472-2549"),
            (
                "meetings search",
                "read-only-diagnostic",
                "main.rs:2472-2549",
            ),
        ];
        let workflow = [
            (
                "lifecycle definitions",
                "read-only-diagnostic",
                "main.rs:4564-4799",
            ),
            (
                "lifecycle definition",
                "read-only-diagnostic",
                "main.rs:4564-4799",
            ),
            (
                "lifecycle instances",
                "read-only-diagnostic",
                "main.rs:4564-4799",
            ),
            (
                "lifecycle instance",
                "read-only-diagnostic",
                "main.rs:4564-4799",
            ),
            (
                "lifecycle snapshot-plan",
                "read-only-diagnostic",
                "main.rs:4564-4799",
            ),
            (
                "lifecycle current-surface",
                "read-only-diagnostic",
                "main.rs:4564-4799",
            ),
            (
                "lifecycle snapshots",
                "read-only-diagnostic",
                "main.rs:4564-4799",
            ),
            (
                "lifecycle snapshot",
                "read-only-diagnostic",
                "main.rs:4564-4799",
            ),
            (
                "lifecycle snapshot-content",
                "read-only-diagnostic",
                "main.rs:4564-4799",
            ),
            (
                "lifecycle operation-log",
                "read-only-diagnostic",
                "main.rs:4564-4799",
            ),
            ("refs status", "read-only-diagnostic", "refs_cmd.rs"),
            ("exec inspect", "pure-input", "exec_cmd.rs"),
            (
                "interchange export-archive",
                "streaming-output",
                "main.rs:16580-16677",
            ),
            (
                "interchange export-fs",
                "physical-file",
                "main.rs:16580-16677",
            ),
            (
                "interchange export-table-csv",
                "physical-file",
                "main.rs:16580-16677",
            ),
            (
                "interchange export-car",
                "streaming-output",
                "main.rs:16580-16677",
            ),
        ];
        let operational = [
            (
                "studio surfaces catalog",
                "read-only-diagnostic",
                "main.rs:10032-10101",
            ),
            ("inference list", "external-cache", "main.rs:6905-7242"),
            ("inference status", "external-cache", "main.rs:6905-7242"),
            ("inference show", "external-cache", "main.rs:6905-7242"),
            (
                "inference download",
                "external-runtime",
                "main.rs:6905-7242",
            ),
            ("inference cancel", "external-runtime", "main.rs:6905-7242"),
            ("inference remove", "external-cache", "main.rs:6905-7242"),
            ("inference refresh", "external-runtime", "main.rs:6905-7242"),
            (
                "inference model list",
                "external-cache",
                "main.rs:6905-7242",
            ),
            (
                "inference model show",
                "external-cache",
                "main.rs:6905-7242",
            ),
            (
                "inference model download",
                "external-runtime",
                "main.rs:6905-7242",
            ),
            (
                "inference model status",
                "external-cache",
                "main.rs:6905-7242",
            ),
            (
                "inference model cancel",
                "external-runtime",
                "main.rs:6905-7242",
            ),
            (
                "inference model remove",
                "external-cache",
                "main.rs:6905-7242",
            ),
            (
                "inference model refresh",
                "external-runtime",
                "main.rs:6905-7242",
            ),
            ("serve remote", "runtime", "serve_cmd.rs:209-385"),
            ("daemon start", "runtime", "daemon_cmd.rs:5-68"),
            ("daemon stop", "runtime", "daemon_cmd.rs:5-68"),
            ("daemon restart", "runtime", "daemon_cmd.rs:5-68"),
            ("daemon status", "runtime", "daemon_cmd.rs:5-68"),
            (
                "daemon session open",
                "carrier-lifecycle",
                "daemon_cmd.rs:409-445",
            ),
            (
                "daemon session close",
                "carrier-lifecycle",
                "daemon_cmd.rs:447-479",
            ),
            ("daemon session attach", "runtime", "daemon_cmd.rs:5-68"),
            ("daemon session detach", "runtime", "daemon_cmd.rs:5-68"),
            ("daemon pin add", "runtime", "daemon_cmd.rs:5-68"),
            ("daemon pin remove", "runtime", "daemon_cmd.rs:5-68"),
            ("daemon run", "runtime", "daemon_cmd.rs:5-68"),
            ("context list", "physical-file", "context_cmd.rs:8-184"),
            ("context get", "physical-file", "context_cmd.rs:8-184"),
            ("context add", "physical-file", "context_cmd.rs:8-184"),
            ("context update", "physical-file", "context_cmd.rs:8-184"),
            ("context remove", "physical-file", "context_cmd.rs:8-184"),
            ("context test", "physical-file", "context_cmd.rs:8-184"),
            ("context use", "physical-file", "context_cmd.rs:8-184"),
            ("context current", "physical-file", "context_cmd.rs:8-184"),
            ("doctor all", "read-only-diagnostic", "main.rs:8523-8590"),
            ("doctor store", "read-only-diagnostic", "main.rs:8523-8590"),
            ("doctor daemon", "read-only-diagnostic", "main.rs:8523-8590"),
            (
                "doctor inference",
                "read-only-diagnostic",
                "main.rs:8523-8590",
            ),
            (
                "doctor inference-instance",
                "read-only-diagnostic",
                "main.rs:7357-7415",
            ),
            (
                "capabilities",
                "read-only-diagnostic",
                "main.rs:13009-13034",
            ),
            ("llms", "read-only-diagnostic", "main.rs:13165-13173"),
            ("mcp", "runtime", "main.rs:13112-13127"),
            ("mount fuse", "runtime", "main.rs:13187-13238"),
            ("mount nfs", "runtime", "main.rs:13187-13238"),
            ("version", "read-only-diagnostic", "main.rs:13165-13173"),
        ];
        analytical
            .into_iter()
            .chain(store)
            .chain(sql_meetings)
            .chain(workflow)
            .chain(operational)
            .map(|(path, category, owner_anchor)| LeafClassification {
                path: path.to_string(),
                ownership: LeafOwnership::Exception {
                    category,
                    owner_anchor,
                },
            })
            .collect()
    }

    fn ownership_guard_source(source: &str) -> String {
        [
            "cli_store_administration_classification_is_complete_and_unique",
            "mu_17g_d5_program_metrics_logs_traces_leaves_use_typed_generated_clients",
            "mu_17g_f1_security_admin_leaves_use_typed_generated_clients",
            "mu_17g_f3_studio_vcs_inference_leaves_have_exhaustive_ownership",
            "mu_17g_f4_serve_and_daemon_leaves_have_one_execution_owner",
            "mu_17g_f5_operational_leaves_have_one_execution_owner",
            "mu_1f_immutable_read_routes_use_read_only_open_helpers",
            "mu_1i_reviewed_immutable_read_routes_are_enforced",
            "mu_17g_a_foundational_data_cli_leaves_use_generated_clients",
            "mu_17g_b_analytical_data_cli_leaves_use_generated_clients",
            "mu_17g_c_pim_cli_leaves_use_typed_generated_clients",
            "mu_17g_d2_identity_acl_protected_ref_leaves_use_typed_generated_clients",
            "mu_17g_d1_core_cli_leaves_use_typed_generated_clients",
            "mu_17g_d3_tickets_lanes_cli_leaves_use_typed_generated_clients",
            "mu_17g_e1_sql_meetings_cli_leaves_use_typed_generated_clients",
            "mu_17g_e2_chat_drive_mutation_leaves_use_typed_generated_clients",
            "mu_17g_f2_lifecycle_refs_exec_interchange_leaves_are_exhaustive",
        ]
        .into_iter()
        .map(|guard| {
            let marker = format!("fn {guard}");
            let start = source.find(&marker).expect("ownership guard");
            let tail = &source[start..];
            let end = tail.find("\n    #[test]").unwrap_or(tail.len());
            &tail[..end]
        })
        .collect::<Vec<_>>()
        .join("\n")
    }

    fn generated_boundary_classifications() -> [LeafClassification<'static>; 2] {
        [
            LeafClassification {
                path: "management kv config set".to_string(),
                ownership: LeafOwnership::Generated {
                    interface: "ManagementKv",
                    method: "set_config",
                },
            },
            LeafClassification {
                path: "meetings source-read".to_string(),
                ownership: LeafOwnership::Generated {
                    interface: "Meetings",
                    method: "meetings_source_read",
                },
            },
        ]
    }

    fn allowed_generated_interfaces(path: &str) -> &'static [&'static str] {
        let parts = path.split_whitespace().collect::<Vec<_>>();
        match parts.as_slice() {
            ["cas", ..] => &["Cas"],
            ["kv", ..] => &["Kv"],
            ["queue", ..] => &["Queue", "QueueConsumers"],
            ["time-series", ..] => &["TimeSeries"],
            ["ledger", ..] => &["Ledger"],
            ["graph", ..] => &["Graph"],
            ["vector", ..] => &["Vector"],
            ["fts", ..] => &["Search"],
            ["columnar", ..] => &["Columnar"],
            ["dataframe", ..] => &["Dataframe"],
            ["calendar", ..] => &["Calendar"],
            ["contacts", ..] => &["Contacts"],
            ["mail", ..] => &["Mail"],
            ["files", ..] => &["FileSystem"],
            ["workspace", ..] | ["management", "workspace", ..] => &["Workspaces"],
            ["document", ..] => &["Document"],
            ["pages", ..] => &["Pages"],
            ["identity", ..] | ["management", "identity", ..] => &["Identity"],
            ["acl", ..] | ["management", "acl", ..] => &["Acl"],
            ["protected-ref", ..] | ["management", "protected-ref", ..] => &["ProtectedRefs"],
            ["management", "kv", ..] => &["ManagementKv"],
            ["store", ..] => &["StoreAdmin", "KeySource"],
            ["tickets", ..] => &["Tickets"],
            ["lanes", ..] => &["Lanes"],
            ["program", ..] => &["Program"],
            ["metrics", ..] => &["Metrics"],
            ["logs", ..] => &["Logs"],
            ["traces", ..] => &["Traces"],
            ["sql", ..] => &["Sql"],
            ["meetings", ..] => &["Meetings"],
            ["chat", ..] => &["Chat"],
            ["drive", ..] => &["Drive"],
            ["audit", ..] => &["Audit"],
            ["certificate", ..] => &["Certificate"],
            ["network-access", ..] => &["NetworkAccess"],
            ["lifecycle", ..] => &["Lifecycle"],
            ["refs", ..] => &["Refs"],
            ["exec", ..] => &["Exec"],
            ["interchange", ..] => &["FileSystem", "Archive", "InterchangeProfiles", "Car"],
            ["studio", ..] => &["StudioMaintenance", "StudioSurfaces"],
            ["vcs", ..] => &["VersionControl"],
            ["inference", "instance", ..] => &["InferenceInstance"],
            ["serve", ..] => &["ServeConfig"],
            ["daemon", "maintenance", ..] => &["StoreAdmin"],
            ["lock", ..] => &["Locks"],
            _ => &[],
        }
    }

    fn generated_owner_from_guards<'a>(
        leaf: &CommandLeaf<'_>,
        guard_source: &'a str,
    ) -> Option<(&'a str, &'a str)> {
        let interfaces = allowed_generated_interfaces(&leaf.path);
        if interfaces.is_empty() {
            return None;
        }
        let marker = format!("\"{}::{}", leaf.enum_name, leaf.variant);
        let mut best: Option<(usize, &'a str, &'a str)> = None;
        let mut search_start = 0usize;
        while let Some(relative) = guard_source[search_start..].find(&marker) {
            let marker_end = search_start + relative + marker.len();
            let owner_start = guard_source[marker_end..]
                .find('"')
                .map(|offset| marker_end + offset + 1)
                .unwrap_or(marker_end);
            let end = (owner_start + 6_000).min(guard_source.len());
            let owner_source = &guard_source[owner_start..end];
            for interface in interfaces {
                for candidate in METHODS
                    .iter()
                    .filter(|candidate| candidate.interface == *interface)
                {
                    let quoted_method = format!("\"{}\"", candidate.method);
                    if let Some(distance) = owner_source.find(&quoted_method)
                        && best.is_none_or(|(best_distance, _, _)| distance < best_distance)
                    {
                        best = Some((distance, interface, candidate.method));
                    }
                }
            }
            search_start = marker_end;
        }
        best.map(|(_, interface, method)| (interface, method))
    }

    fn authoritative_leaf_classifications<'a>(
        leaves: &[CommandLeaf<'_>],
        guard_source: &'a str,
    ) -> Vec<LeafClassification<'a>> {
        let exceptions = exception_classifications();
        let exception_paths = exceptions
            .iter()
            .map(|classification| classification.path.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let mut classifications = exceptions
            .into_iter()
            .map(|classification| LeafClassification {
                path: classification.path,
                ownership: match classification.ownership {
                    LeafOwnership::Exception {
                        category,
                        owner_anchor,
                    } => LeafOwnership::Exception {
                        category,
                        owner_anchor,
                    },
                    LeafOwnership::Generated { .. } => unreachable!(),
                },
            })
            .collect::<Vec<_>>();
        classifications.extend(generated_boundary_classifications());
        let classified_paths = classifications
            .iter()
            .map(|classification| classification.path.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for leaf in leaves {
            if exception_paths.contains(&leaf.path) || classified_paths.contains(&leaf.path) {
                continue;
            }
            if let Some((interface, method)) = generated_owner_from_guards(leaf, guard_source) {
                classifications.push(LeafClassification {
                    path: leaf.path.clone(),
                    ownership: LeafOwnership::Generated { interface, method },
                });
            }
        }
        classifications
    }

    fn validate_leaf_classifications(
        production: &std::collections::BTreeSet<String>,
        classifications: &[LeafClassification<'_>],
    ) -> Result<ClassificationSets, String> {
        let mut generated = std::collections::BTreeSet::new();
        let mut exceptions = std::collections::BTreeSet::new();
        let mut duplicates = std::collections::BTreeSet::new();
        let mut both = std::collections::BTreeSet::new();
        for classification in classifications {
            match classification.ownership {
                LeafOwnership::Generated { interface, method } => {
                    if interface.is_empty() || method.is_empty() {
                        return Err(format!("blank generated owner for {}", classification.path));
                    }
                    if !generated.insert(classification.path.clone()) {
                        duplicates.insert(classification.path.clone());
                    }
                    if exceptions.contains(&classification.path) {
                        both.insert(classification.path.clone());
                    }
                }
                LeafOwnership::Exception {
                    category,
                    owner_anchor,
                } => {
                    if category.is_empty() || owner_anchor.is_empty() {
                        return Err(format!("blank exception owner for {}", classification.path));
                    }
                    if !exceptions.insert(classification.path.clone()) {
                        duplicates.insert(classification.path.clone());
                    }
                    if generated.contains(&classification.path) {
                        both.insert(classification.path.clone());
                    }
                }
            }
        }
        let union = generated
            .union(&exceptions)
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let missing = production.difference(&union).cloned().collect::<Vec<_>>();
        let extra = union.difference(production).cloned().collect::<Vec<_>>();
        if !duplicates.is_empty() || !both.is_empty() || !missing.is_empty() || !extra.is_empty() {
            return Err(format!(
                "duplicates={duplicates:?} both={both:?} missing={missing:?} extra={extra:?}"
            ));
        }
        Ok(ClassificationSets {
            generated,
            exceptions,
        })
    }

    fn impl_body<'a>(source: &'a str, type_name: &str) -> &'a str {
        let marker = format!("impl {type_name}");
        let impl_start = source.find(&marker).expect("impl marker");
        let body_start = source[impl_start..].find('{').expect("impl body") + impl_start;
        let mut depth = 0usize;
        for (offset, byte) in source[body_start..].bytes().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &source[body_start..=body_start + offset];
                    }
                }
                _ => {}
            }
        }
        panic!("unclosed impl body for {type_name}");
    }

    fn impl_method_names(source: &str) -> Vec<&str> {
        source
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim_start();
                let rest = trimmed
                    .strip_prefix("pub(crate) fn ")
                    .or_else(|| trimmed.strip_prefix("fn "))?;
                rest.split(|ch: char| ch == '(' || ch == '<' || ch.is_whitespace())
                    .next()
            })
            .collect()
    }

    fn enum_body<'a>(source: &'a str, enum_name: &str) -> &'a str {
        let marker = format!("enum {enum_name}");
        let enum_start = source.find(&marker).expect("enum marker");
        let body_start = source[enum_start..].find('{').expect("enum body") + enum_start;
        let mut depth = 0usize;
        for (offset, byte) in source[body_start..].bytes().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &source[body_start + 1..body_start + offset];
                    }
                }
                _ => {}
            }
        }
        panic!("unclosed enum body for {enum_name}");
    }

    fn function_body<'a>(source: &'a str, function: &str) -> &'a str {
        let marker = format!("fn {function}");
        let mut search_start = 0usize;
        let function_start = loop {
            let relative = source[search_start..]
                .find(&marker)
                .unwrap_or_else(|| panic!("function marker {marker}"));
            let candidate = search_start + relative;
            if matches!(
                source.as_bytes().get(candidate + marker.len()),
                Some(b'(' | b'<')
            ) {
                break candidate;
            }
            search_start = candidate + marker.len();
        };
        let body_start =
            source[function_start..].find('{').expect("function body") + function_start;
        let mut depth = 0usize;
        for (offset, byte) in source[body_start..].bytes().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &source[body_start..=body_start + offset];
                    }
                }
                _ => {}
            }
        }
        panic!("unclosed function body for {function}");
    }

    fn match_arm_body<'a>(source: &'a str, arm: &str) -> &'a str {
        let arm_start = source.find(arm).expect("arm marker");
        let arrow = source[arm_start..].find("=>").expect("arm arrow") + arm_start;
        let body_start = source[arrow..].find('{').expect("arm body") + arrow;
        let mut depth = 0usize;
        for (offset, byte) in source[body_start..].bytes().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &source[body_start..=body_start + offset];
                    }
                }
                _ => {}
            }
        }
        panic!("unclosed match arm body for {arm}");
    }

    fn match_arm_segment<'a>(source: &'a str, arm: &str, enum_name: &str) -> &'a str {
        let arm_start = source.find(arm).expect("arm marker");
        let tail = &source[arm_start..];
        let next_marker = format!("{enum_name}::");
        let end = tail[arm.len()..]
            .find(&next_marker)
            .map(|offset| arm.len() + offset)
            .unwrap_or(tail.len());
        &tail[..end]
    }

    #[cfg(feature = "serve")]
    #[test]
    fn cli_execution_context_does_not_fallback_when_daemon_generated_dispatch_fails() {
        let store = temp_store("daemon-fail-closed");
        let paths = daemon::paths(&store).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
        let store_path = paths.store.clone();
        let store_id = paths.store_id.clone();
        let server = std::thread::spawn(move || {
            for request_index in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                stream.read_to_end(&mut request).unwrap();
                let response = if request_index < 2 {
                    running_daemon_response(&store_path, &store_id)
                } else {
                    b"not-a-generated-session-response".to_vec()
                };
                stream.write_all(&response).unwrap();
            }
        });

        let error = match open_cli_execution_context(&store) {
            Ok(_) => panic!("daemon failure must not fall back to direct local"),
            Err(error) => error,
        };

        assert!(
            error.contains("daemon-local generated session open failed"),
            "{error}"
        );
        server.join().unwrap();
        let _ = std::fs::remove_file(&paths.addr_file);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn cli_execution_context_selects_daemon_local_and_carries_auth() {
        let store = temp_store("daemon-auth");
        let paths = daemon::paths(&store).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
        let store_path = paths.store.clone();
        let store_id = paths.store_id.clone();
        let principal = WorkspaceId::from_bytes([7; 16]);
        let passphrase_path = std::env::temp_dir().join(format!(
            "loomcli-execution-selector-passphrase-{}",
            std::process::id()
        ));
        std::fs::write(&passphrase_path, "selector-passphrase").unwrap();
        let server = std::thread::spawn(move || {
            let session_id = vec![1, 2, 3, 4];
            for request_index in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                stream.read_to_end(&mut request).unwrap();
                let response = match request_index {
                    0 | 1 => running_daemon_response(&store_path, &store_id),
                    2 => {
                        let body =
                            generated_binary_body(&request, DAEMON_GENERATED_SESSION_OPEN_MAGIC)
                                .expect("session open frame");
                        let auth = loom_remote_protocol::session::parse_open_request(body)
                            .expect("session auth");
                        assert_eq!(
                            auth,
                            SessionAuth::Passphrase {
                                principal: [7; 16],
                                passphrase: b"selector-passphrase".to_vec()
                            }
                        );
                        let reply = loom_remote_protocol::session::SessionOpenReply::Ok {
                            session_id: session_id.clone(),
                            lease_expires_ms: 123,
                            credential: None,
                        };
                        generated_binary_response(
                            DAEMON_GENERATED_SESSION_RESPONSE_MAGIC,
                            &[loom_remote_protocol::session::open_reply_bytes(&reply)],
                        )
                    }
                    _ => {
                        let body = generated_binary_body(&request, DAEMON_GENERATED_CALL_MAGIC)
                            .expect("generated call frame");
                        success_store_open_response(body, &session_id)
                    }
                };
                stream.write_all(&response).unwrap();
            }
        });
        let keys = KeyOpts {
            auth_principal: Some(principal.to_string()),
            auth_source: crate::KeySource::File(passphrase_path.to_string_lossy().into_owned()),
            ..KeyOpts::default()
        };

        let context =
            open_cli_execution_context_with_keys(&store, &keys).expect("open daemon context");

        assert_eq!(context.target(), CliExecutionTarget::DaemonLocal);
        server.join().unwrap();
        let _ = std::fs::remove_file(&paths.addr_file);
        let _ = std::fs::remove_file(&passphrase_path);
    }

    #[cfg(all(feature = "serve", feature = "mcp", feature = "remote-client"))]
    #[test]
    fn local_mcp_backend_connects_through_daemon_generated_boundary() {
        let store = temp_store("mcp-daemon-boundary");
        let paths = daemon::paths(&store).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
        let store_path = paths.store.clone();
        let store_id = paths.store_id.clone();
        let server = std::thread::spawn(move || {
            let session_id = vec![9, 7, 5, 3];
            let credential = b"logical-session-credential".to_vec();
            for request_index in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                stream.read_to_end(&mut request).unwrap();
                let response = match request_index {
                    0 => running_daemon_response(&store_path, &store_id),
                    1 => {
                        let body =
                            generated_binary_body(&request, DAEMON_GENERATED_SESSION_OPEN_MAGIC)
                                .expect("session open frame");
                        let auth = loom_remote_protocol::session::parse_open_request(body)
                            .expect("session auth");
                        assert_eq!(auth, SessionAuth::Unauthenticated);
                        let reply = loom_remote_protocol::session::SessionOpenReply::Ok {
                            session_id: session_id.clone(),
                            lease_expires_ms: 123,
                            credential: Some(credential.clone()),
                        };
                        generated_binary_response(
                            DAEMON_GENERATED_SESSION_RESPONSE_MAGIC,
                            &[loom_remote_protocol::session::open_reply_bytes(&reply)],
                        )
                    }
                    2 => {
                        let body = generated_binary_body(&request, DAEMON_GENERATED_CALL_MAGIC)
                            .expect("store open frame");
                        success_store_open_response(body, &session_id)
                    }
                    _ => {
                        let body =
                            generated_binary_body(&request, DAEMON_GENERATED_SESSION_OPEN_MAGIC)
                                .expect("session close frame");
                        assert_eq!(
                            loom_remote_protocol::session::parse_session_request(body)
                                .expect("session close request"),
                            loom_remote_protocol::session::SessionRequest::Close {
                                auth: SessionAuth::Unauthenticated,
                                credential: credential.clone(),
                            }
                        );
                        let reply = loom_remote_protocol::session::SessionOpenReply::Ok {
                            session_id: session_id.clone(),
                            lease_expires_ms: 0,
                            credential: None,
                        };
                        generated_binary_response(
                            DAEMON_GENERATED_SESSION_RESPONSE_MAGIC,
                            &[loom_remote_protocol::session::open_reply_bytes(&reply)],
                        )
                    }
                };
                stream.write_all(&response).unwrap();
            }
        });

        let backend = McpRemoteBackend::connect_local_daemon(&store, &KeyOpts::default())
            .expect("local daemon MCP backend");

        assert_eq!(backend.handle.0.owner_session, vec![9, 7, 5, 3]);
        backend
            .close_logical_session()
            .expect("close local daemon MCP logical session");
        server.join().unwrap();
        let _ = std::fs::remove_file(&paths.addr_file);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn cli_generated_client_executes_same_operation_through_daemon_local_boundary() {
        let store = temp_store("daemon-operation");
        let paths = daemon::paths(&store).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
        let store_path = paths.store.clone();
        let store_id = paths.store_id.clone();
        let digest = WireDigest(
            "blake3:1111111111111111111111111111111111111111111111111111111111111111".to_string(),
        );
        let expected_digest = digest.clone();
        let expected_content = b"daemon generated operation".to_vec();
        let expected_read = expected_content.clone();
        let server = std::thread::spawn(move || {
            let session_id = vec![4, 3, 2, 1];
            for request_index in 0..8 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                stream.read_to_end(&mut request).unwrap();
                let response = match request_index {
                    0 | 1 => running_daemon_response(&store_path, &store_id),
                    2 => {
                        let reply = loom_remote_protocol::session::SessionOpenReply::Ok {
                            session_id: session_id.clone(),
                            lease_expires_ms: 123,
                            credential: None,
                        };
                        generated_binary_response(
                            DAEMON_GENERATED_SESSION_RESPONSE_MAGIC,
                            &[loom_remote_protocol::session::open_reply_bytes(&reply)],
                        )
                    }
                    3 => {
                        let body = generated_binary_body(&request, DAEMON_GENERATED_CALL_MAGIC)
                            .expect("store open frame");
                        success_store_open_response(body, &session_id)
                    }
                    4 => {
                        let body = generated_binary_body(&request, DAEMON_GENERATED_CALL_MAGIC)
                            .expect("cas put frame");
                        success_generated_response(
                            body,
                            &session_id,
                            "Cas",
                            "put",
                            expected_digest.to_value(),
                        )
                    }
                    5 => {
                        let body = generated_binary_body(&request, DAEMON_GENERATED_CALL_MAGIC)
                            .expect("cas get frame");
                        success_generated_response(
                            body,
                            &session_id,
                            "Cas",
                            "get",
                            loom_codec::Value::Bytes(expected_read.clone()),
                        )
                    }
                    6 => {
                        let body = generated_binary_body(&request, DAEMON_GENERATED_CALL_MAGIC)
                            .expect("tickets project create frame");
                        success_generated_response(
                            body,
                            &session_id,
                            "Tickets",
                            "tickets_project_create_json",
                            loom_codec::Value::Text(
                                "{\"project_id\":\"selector-project\"}".to_string(),
                            ),
                        )
                    }
                    _ => {
                        let body = generated_binary_body(&request, DAEMON_GENERATED_CALL_MAGIC)
                            .expect("store close frame");
                        success_generated_response(
                            body,
                            &session_id,
                            "Store",
                            "close",
                            loom_codec::Value::Null,
                        )
                    }
                };
                stream.write_all(&response).unwrap();
            }
        });

        {
            let generated = open_cli_execution_context(&store)
                .expect("open daemon context")
                .into_generated_client()
                .expect("daemon generated client");
            let workspace = "blobs".to_string();
            let put = generated
                .execute_unary(&cas_put_operation(&workspace, expected_content))
                .expect("cas put");
            assert_eq!(put, digest.to_value());
            let get = generated
                .execute_unary(&cas_get_operation(&workspace, digest))
                .expect("cas get");
            assert_eq!(
                get,
                loom_codec::Value::Bytes(b"daemon generated operation".to_vec())
            );
            let project = generated
                .execute_unary(&tickets_project_create_operation("workgraph"))
                .expect("tickets project create");
            assert_eq!(
                project,
                loom_codec::Value::Text("{\"project_id\":\"selector-project\"}".to_string())
            );
        }

        server.join().unwrap();
        let _ = std::fs::remove_file(&paths.addr_file);
    }

    #[cfg(all(feature = "serve", feature = "mcp", feature = "remote-client"))]
    #[test]
    fn local_mcp_backend_executes_generated_read_and_mutation_through_daemon() {
        let store = temp_store("mcp-daemon-generated-execute");
        let paths = daemon::paths(&store).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
        let store_path = paths.store.clone();
        let store_id = paths.store_id.clone();
        let server = std::thread::spawn(move || {
            let session_id = vec![6, 5, 4, 3];
            for request_index in 0..5 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                stream.read_to_end(&mut request).unwrap();
                let response = match request_index {
                    0 => running_daemon_response(&store_path, &store_id),
                    1 => {
                        let reply = loom_remote_protocol::session::SessionOpenReply::Ok {
                            session_id: session_id.clone(),
                            lease_expires_ms: 123,
                            credential: Some(b"mcp-generated-credential".to_vec()),
                        };
                        generated_binary_response(
                            DAEMON_GENERATED_SESSION_RESPONSE_MAGIC,
                            &[loom_remote_protocol::session::open_reply_bytes(&reply)],
                        )
                    }
                    2 => {
                        let body = generated_binary_body(&request, DAEMON_GENERATED_CALL_MAGIC)
                            .expect("store open frame");
                        success_store_open_response(body, &session_id)
                    }
                    3 => {
                        let body = generated_binary_body(&request, DAEMON_GENERATED_CALL_MAGIC)
                            .expect("store version frame");
                        let decoded = loom_remote_protocol::envelope::Request::decode(body)
                            .expect("decode version request");
                        assert_eq!(decoded.args, Vec::<loom_codec::Value>::new());
                        success_generated_response(
                            body,
                            &session_id,
                            "Store",
                            "version",
                            loom_codec::Value::Text("daemon-mcp-version".to_string()),
                        )
                    }
                    _ => {
                        let body = generated_binary_body(&request, DAEMON_GENERATED_CALL_MAGIC)
                            .expect("graph remove edge frame");
                        let decoded = loom_remote_protocol::envelope::Request::decode(body)
                            .expect("decode graph request");
                        assert_eq!(
                            decoded.args,
                            vec![
                                LoomSession(loom_remote_protocol::api_types::HandleId {
                                    kind: "session".to_string(),
                                    id: vec![9, 8, 7],
                                    generation: 1,
                                    owner_session: session_id.clone(),
                                })
                                .to_value(),
                                loom_codec::Value::Text("repo".to_string()),
                                loom_codec::Value::Text("graph".to_string()),
                                loom_codec::Value::Text("edge-1".to_string()),
                            ]
                        );
                        success_generated_response(
                            body,
                            &session_id,
                            "Graph",
                            "remove_edge",
                            loom_codec::Value::Bool(true),
                        )
                    }
                };
                stream.write_all(&response).unwrap();
            }
        });

        let backend = McpRemoteBackend::connect_local_daemon(&store, &KeyOpts::default())
            .expect("local daemon MCP backend");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let version = rt
            .block_on(
                uldren_loom_mcp::RemoteMcpBackend::execute_generated_operation(
                    &backend,
                    uldren_loom_mcp::GeneratedMcpCall {
                        operation: GeneratedOperationId::StoreVersion,
                        args_without_handle: Vec::new(),
                    },
                ),
            )
            .expect("generated read through local daemon MCP");
        assert_eq!(
            version,
            loom_codec::Value::Text("daemon-mcp-version".to_string())
        );
        let removed = rt
            .block_on(
                uldren_loom_mcp::RemoteMcpBackend::execute_generated_operation(
                    &backend,
                    uldren_loom_mcp::GeneratedMcpCall {
                        operation: GeneratedOperationId::GraphRemoveEdge,
                        args_without_handle: vec![
                            loom_codec::Value::Text("repo".to_string()),
                            loom_codec::Value::Text("graph".to_string()),
                            loom_codec::Value::Text("edge-1".to_string()),
                        ],
                    },
                ),
            )
            .expect("generated mutation through local daemon MCP");
        assert_eq!(removed, loom_codec::Value::Bool(true));

        server.join().unwrap();
        let _ = std::fs::remove_file(&paths.addr_file);
    }

    #[cfg(all(feature = "serve", feature = "mcp", feature = "remote-client"))]
    #[test]
    fn mu17_local_mcp_backend_fails_closed_when_session_open_is_rejected() {
        let store = temp_store("mu17-mcp-daemon-session-reject");
        let paths = daemon::paths(&store).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
        let store_path = paths.store.clone();
        let store_id = paths.store_id.clone();
        let server = std::thread::spawn(move || {
            for request_index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                stream.read_to_end(&mut request).unwrap();
                let response = match request_index {
                    0 => running_daemon_response(&store_path, &store_id),
                    _ => {
                        let _body =
                            generated_binary_body(&request, DAEMON_GENERATED_SESSION_OPEN_MAGIC)
                                .expect("session open frame");
                        let reply = loom_remote_protocol::session::SessionOpenReply::Err(
                            loom_remote_protocol::RemoteError::from_loom_error(
                                &loom_core::error::LoomError::new(
                                    loom_core::error::Code::PermissionDenied,
                                    "session rejected",
                                ),
                            ),
                        );
                        generated_binary_response(
                            DAEMON_GENERATED_SESSION_RESPONSE_MAGIC,
                            &[loom_remote_protocol::session::open_reply_bytes(&reply)],
                        )
                    }
                };
                stream.write_all(&response).unwrap();
            }
        });

        let error = match McpRemoteBackend::connect_local_daemon(&store, &KeyOpts::default()) {
            Ok(_) => panic!("session-open rejection must fail closed"),
            Err(error) => error,
        };

        assert!(error.contains("session rejected"));
        server.join().unwrap();
        let _ = std::fs::remove_file(&paths.addr_file);
    }

    #[cfg(all(feature = "serve", feature = "mcp", feature = "remote-client"))]
    #[test]
    fn mu17_local_mcp_generated_call_error_preserves_code_without_direct_fallback() {
        let store = temp_store("mu17-mcp-daemon-call-error");
        let paths = daemon::paths(&store).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std::fs::write(&paths.addr_file, listener.local_addr().unwrap().to_string()).unwrap();
        let store_path = paths.store.clone();
        let store_id = paths.store_id.clone();
        let server = std::thread::spawn(move || {
            let session_id = vec![7, 1, 7, 1];
            for request_index in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                stream.read_to_end(&mut request).unwrap();
                let response = match request_index {
                    0 => running_daemon_response(&store_path, &store_id),
                    1 => {
                        let reply = loom_remote_protocol::session::SessionOpenReply::Ok {
                            session_id: session_id.clone(),
                            lease_expires_ms: 123,
                            credential: Some(b"mcp-error-credential".to_vec()),
                        };
                        generated_binary_response(
                            DAEMON_GENERATED_SESSION_RESPONSE_MAGIC,
                            &[loom_remote_protocol::session::open_reply_bytes(&reply)],
                        )
                    }
                    2 => {
                        let body = generated_binary_body(&request, DAEMON_GENERATED_CALL_MAGIC)
                            .expect("store open frame");
                        success_store_open_response(body, &session_id)
                    }
                    3 => {
                        let body = generated_binary_body(&request, DAEMON_GENERATED_CALL_MAGIC)
                            .expect("store version frame");
                        error_generated_response(
                            body,
                            &session_id,
                            "Store",
                            "version",
                            loom_core::error::LoomError::new(
                                loom_core::error::Code::NotFound,
                                "version unavailable",
                            ),
                        )
                    }
                    _ => unreachable!(),
                };
                stream.write_all(&response).unwrap();
            }
        });

        let backend = McpRemoteBackend::connect_local_daemon(&store, &KeyOpts::default())
            .expect("local daemon MCP backend");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let error = rt
            .block_on(
                uldren_loom_mcp::RemoteMcpBackend::execute_generated_operation(
                    &backend,
                    uldren_loom_mcp::GeneratedMcpCall {
                        operation: GeneratedOperationId::StoreVersion,
                        args_without_handle: Vec::new(),
                    },
                ),
            )
            .expect_err("generated call error");

        assert_eq!(error.code, loom_core::error::Code::NotFound);
        assert!(error.message.contains("version unavailable"));
        server.join().unwrap();
        let _ = std::fs::remove_file(&paths.addr_file);
    }

    #[cfg(all(feature = "serve", feature = "remote-client"))]
    #[test]
    fn cli_generated_client_executes_same_operation_through_remote_boundary() {
        let store = temp_store("remote-operation");
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = std::env::temp_dir();
        let cert_path = dir.join(format!(
            "loomcli-selector-remote-{}.crt",
            std::process::id()
        ));
        let key_path = dir.join(format!(
            "loomcli-selector-remote-{}.key",
            std::process::id()
        ));
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.signing_key.serialize_pem()).unwrap();
        let tls = loom_hosted_core::HostedTlsConfig::from_pem_files(
            &cert_path.to_string_lossy(),
            &key_path.to_string_lossy(),
        )
        .expect("server tls");
        let options = loom_hosted_core::remote::RemoteServeOptions::from_cli(
            "127.0.0.1:0".to_string(),
            "https://localhost/apps/loom".to_string(),
            None,
            vec![loom_hosted_core::remote::RemoteAuthMode::Interactive],
            vec![loom_hosted_core::remote::RemoteTlsTrust::System],
            60_000,
            1 << 20,
            None,
        );
        let server_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = server_rt
            .block_on(crate::serve_cmd::bind_remote_endpoint(
                &store,
                &options,
                tls.server_config(),
            ))
            .expect("bind remote endpoint");
        let target = RemoteTarget {
            url: format!("https://127.0.0.1:{}/apps/loom", server.local_addr().port()),
            auth: None,
            tls: Some("insecure-dev".to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };
        let context =
            CliExecutionContext::Remote(Box::new(RemoteStore::connect(&target).expect("connect")));

        assert_eq!(context.target(), CliExecutionTarget::Remote);
        let generated = context
            .into_generated_client()
            .expect("remote generated client");
        let workspace = "blobs".to_string();
        let content = b"remote generated operation".to_vec();
        let digest = WireDigest(
            match generated
                .execute_unary(&cas_put_operation(&workspace, content.clone()))
                .expect("cas put")
            {
                loom_codec::Value::Text(digest) => digest,
                other => panic!("unexpected output {other:?}"),
            },
        );
        let read = generated
            .execute_unary(&cas_get_operation(&workspace, digest))
            .expect("cas get");

        assert_eq!(read, loom_codec::Value::Bytes(content));
        let workgraph = workspace_id_from_value(
            generated
                .execute_unary(&workspace_create_operation("workgraph"))
                .expect("workspace create"),
        );
        let project = generated
            .execute_unary(&tickets_project_create_operation(&workgraph))
            .expect("tickets project create");
        match project {
            loom_codec::Value::Text(json) => assert!(json.contains("selector-project"), "{json}"),
            other => panic!("unexpected output {other:?}"),
        }

        let key_a = loom_core::kv::key_to_cbor(&loom_core::Value::Text("a".to_string()));
        let key_z = loom_core::kv::key_to_cbor(&loom_core::Value::Text("z".to_string()));
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "Kv",
                    "put",
                    vec![
                        loom_codec::Value::Text("mu17g-kv".to_string()),
                        loom_codec::Value::Text("settings".to_string()),
                        loom_codec::Value::Bytes(key_a.clone()),
                        loom_codec::Value::Bytes(b"alpha".to_vec()),
                    ],
                )
                .expect("kv put operation"),
            )
            .expect("kv put");
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Kv",
                        "get",
                        vec![
                            loom_codec::Value::Text("mu17g-kv".to_string()),
                            loom_codec::Value::Text("settings".to_string()),
                            loom_codec::Value::Bytes(key_a.clone()),
                        ],
                    )
                    .expect("kv get operation"),
                )
                .expect("kv get"),
            loom_codec::Value::Bytes(b"alpha".to_vec())
        );
        assert!(matches!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Kv",
                        "list",
                        vec![
                            loom_codec::Value::Text("mu17g-kv".to_string()),
                            loom_codec::Value::Text("settings".to_string()),
                        ],
                    )
                    .expect("kv list operation"),
                )
                .expect("kv list"),
            loom_codec::Value::Bytes(bytes) if !bytes.is_empty()
        ));
        assert!(matches!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Kv",
                        "range",
                        vec![
                            loom_codec::Value::Text("mu17g-kv".to_string()),
                            loom_codec::Value::Text("settings".to_string()),
                            loom_codec::Value::Bytes(key_a.clone()),
                            loom_codec::Value::Bytes(key_z),
                        ],
                    )
                    .expect("kv range operation"),
                )
                .expect("kv range"),
            loom_codec::Value::Bytes(bytes) if !bytes.is_empty()
        ));
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Kv",
                        "delete",
                        vec![
                            loom_codec::Value::Text("mu17g-kv".to_string()),
                            loom_codec::Value::Text("settings".to_string()),
                            loom_codec::Value::Bytes(key_a),
                        ],
                    )
                    .expect("kv delete operation"),
                )
                .expect("kv delete"),
            loom_codec::Value::Bool(true)
        );

        let queue_append = |entry: &[u8]| {
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Queue",
                        "append",
                        vec![
                            loom_codec::Value::Text("mu17g-queue".to_string()),
                            loom_codec::Value::Text("events".to_string()),
                            loom_codec::Value::Bytes(entry.to_vec()),
                        ],
                    )
                    .expect("queue append operation"),
                )
                .expect("queue append")
        };
        assert_eq!(queue_append(b"alpha"), loom_codec::Value::Uint(0));
        assert_eq!(queue_append(b"beta"), loom_codec::Value::Uint(1));
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Queue",
                        "get",
                        vec![
                            loom_codec::Value::Text("mu17g-queue".to_string()),
                            loom_codec::Value::Text("events".to_string()),
                            loom_codec::Value::Uint(0),
                        ],
                    )
                    .expect("queue get operation"),
                )
                .expect("queue get"),
            loom_codec::Value::Bytes(b"alpha".to_vec())
        );
        assert!(matches!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Queue",
                        "range",
                        vec![
                            loom_codec::Value::Text("mu17g-queue".to_string()),
                            loom_codec::Value::Text("events".to_string()),
                            loom_codec::Value::Uint(0),
                            loom_codec::Value::Uint(2),
                        ],
                    )
                    .expect("queue range operation"),
                )
                .expect("queue range"),
            loom_codec::Value::Array(entries) if entries.len() == 2
        ));
        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "QueueConsumers",
                    "consumer_advance",
                    vec![
                        loom_codec::Value::Text("mu17g-queue".to_string()),
                        loom_codec::Value::Text("events".to_string()),
                        loom_codec::Value::Text("worker".to_string()),
                        loom_codec::Value::Uint(1),
                    ],
                )
                .expect("queue consumer advance operation"),
            )
            .expect("queue consumer advance");
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "QueueConsumers",
                        "consumer_position",
                        vec![
                            loom_codec::Value::Text("mu17g-queue".to_string()),
                            loom_codec::Value::Text("events".to_string()),
                            loom_codec::Value::Text("worker".to_string()),
                        ],
                    )
                    .expect("queue consumer position operation"),
                )
                .expect("queue consumer position"),
            loom_codec::Value::Uint(1)
        );

        generated
            .execute_unary(
                &CliGeneratedOperation::new(
                    "TimeSeries",
                    "put",
                    vec![
                        loom_codec::Value::Text("mu17g-ts".to_string()),
                        loom_codec::Value::Text("cpu".to_string()),
                        loom_codec::Value::Uint(100),
                        loom_codec::Value::Bytes(b"alpha".to_vec()),
                    ],
                )
                .expect("time-series put operation"),
            )
            .expect("time-series put");
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "TimeSeries",
                        "get",
                        vec![
                            loom_codec::Value::Text("mu17g-ts".to_string()),
                            loom_codec::Value::Text("cpu".to_string()),
                            loom_codec::Value::Uint(100),
                        ],
                    )
                    .expect("time-series get operation"),
                )
                .expect("time-series get"),
            loom_codec::Value::Bytes(b"alpha".to_vec())
        );
        assert!(matches!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "TimeSeries",
                        "range",
                        vec![
                            loom_codec::Value::Text("mu17g-ts".to_string()),
                            loom_codec::Value::Text("cpu".to_string()),
                            loom_codec::Value::Uint(0),
                            loom_codec::Value::Uint(200),
                        ],
                    )
                    .expect("time-series range operation"),
                )
                .expect("time-series range"),
            loom_codec::Value::Bytes(bytes) if !bytes.is_empty()
        ));

        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Ledger",
                        "append",
                        vec![
                            loom_codec::Value::Text("mu17g-ledger".to_string()),
                            loom_codec::Value::Text("audit".to_string()),
                            loom_codec::Value::Bytes(b"alpha".to_vec()),
                        ],
                    )
                    .expect("ledger append operation"),
                )
                .expect("ledger append"),
            loom_codec::Value::Uint(0)
        );
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Ledger",
                        "get",
                        vec![
                            loom_codec::Value::Text("mu17g-ledger".to_string()),
                            loom_codec::Value::Text("audit".to_string()),
                            loom_codec::Value::Uint(0),
                        ],
                    )
                    .expect("ledger get operation"),
                )
                .expect("ledger get"),
            loom_codec::Value::Bytes(b"alpha".to_vec())
        );
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Ledger",
                        "len",
                        vec![
                            loom_codec::Value::Text("mu17g-ledger".to_string()),
                            loom_codec::Value::Text("audit".to_string()),
                        ],
                    )
                    .expect("ledger len operation"),
                )
                .expect("ledger len"),
            loom_codec::Value::Uint(1)
        );
        assert!(matches!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Ledger",
                        "head",
                        vec![
                            loom_codec::Value::Text("mu17g-ledger".to_string()),
                            loom_codec::Value::Text("audit".to_string()),
                        ],
                    )
                    .expect("ledger head operation"),
                )
                .expect("ledger head"),
            loom_codec::Value::Text(digest) if digest.starts_with("blake3:")
        ));
        assert_eq!(
            generated
                .execute_unary(
                    &CliGeneratedOperation::new(
                        "Ledger",
                        "verify",
                        vec![
                            loom_codec::Value::Text("mu17g-ledger".to_string()),
                            loom_codec::Value::Text("audit".to_string()),
                        ],
                    )
                    .expect("ledger verify operation"),
                )
                .expect("ledger verify"),
            loom_codec::Value::Null
        );
        server.shutdown();
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
    }

    #[cfg(all(feature = "serve", feature = "remote-client"))]
    #[test]
    fn identity_authority_policy_generated_remote_persists_and_audits() {
        use loom_core::WorkspaceId;
        use loom_core::identity::IdentityStore;

        let root = WorkspaceId::v4_from_bytes([7; 16]);
        let source_store = temp_store("authority-policy-source");
        let remote_store = temp_store("authority-policy-remote");
        let source = cli_open_loom(&source_store, &KeyOpts::default()).expect("open source seed");
        source
            .store()
            .save_identity_store(&IdentityStore::new(root))
            .expect("save source identity");
        let destination =
            cli_open_loom(&remote_store, &KeyOpts::default()).expect("open destination seed");
        let mut identity = IdentityStore::new(root);
        identity
            .set_passphrase(root, "rootpw", b"root-salt-bytes")
            .expect("seed root passphrase");
        destination
            .store()
            .save_identity_store(&identity)
            .expect("save destination identity");
        let mut acl = loom_core::AclStore::new();
        acl.allow(
            loom_core::AclSubject::Principal(root),
            None,
            None,
            [loom_core::AclRight::Admin],
        )
        .expect("grant root global admin");
        destination
            .store()
            .save_acl_store(&acl)
            .expect("save destination acl");
        drop(source);
        drop(destination);

        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = std::env::temp_dir();
        let cert_path = dir.join(format!("loomcli-idpolicy-{}.crt", std::process::id()));
        let key_path = dir.join(format!("loomcli-idpolicy-{}.key", std::process::id()));
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.signing_key.serialize_pem()).unwrap();
        let tls = loom_hosted_core::HostedTlsConfig::from_pem_files(
            &cert_path.to_string_lossy(),
            &key_path.to_string_lossy(),
        )
        .expect("server tls");
        let options = loom_hosted_core::remote::RemoteServeOptions::from_cli(
            "127.0.0.1:0".to_string(),
            "https://localhost/apps/loom".to_string(),
            None,
            vec![loom_hosted_core::remote::RemoteAuthMode::Interactive],
            vec![loom_hosted_core::remote::RemoteTlsTrust::System],
            60_000,
            1 << 20,
            None,
        );
        let server_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = server_rt
            .block_on(crate::serve_cmd::bind_remote_endpoint(
                &remote_store,
                &options,
                tls.server_config(),
            ))
            .expect("bind remote endpoint");
        let target = RemoteTarget {
            url: format!("https://127.0.0.1:{}/apps/loom", server.local_addr().port()),
            auth: None,
            tls: Some("insecure-dev".to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };

        let denied = CliGeneratedClient::Remote(Box::new(
            RemoteStore::connect(&target).expect("unauthenticated connect"),
        ));
        let denied_err = denied
            .generated_json(
                "Identity",
                "identity_configure_authority_replication_json",
                vec![
                    "not allowed".to_value(),
                    "".to_value(),
                    false.to_value(),
                    true.to_value(),
                    loom_codec::Value::Null,
                    0u64.to_value(),
                    0u64.to_value(),
                    true.to_value(),
                ],
            )
            .expect_err("unauthenticated generated configure must fail");
        assert!(denied_err.contains("AUTHENTICATION_FAILED"));

        let remote = CliGeneratedClient::Remote(Box::new(
            RemoteStore::connect_with_auth(
                &target,
                SessionAuth::Passphrase {
                    principal: *root.as_bytes(),
                    passphrase: b"rootpw".to_vec(),
                },
            )
            .expect("authenticated connect"),
        ));
        let configured = remote
            .generated_json(
                "Identity",
                "identity_configure_authority_replication_json",
                vec![
                    "primary".to_value(),
                    source_store.clone().to_value(),
                    false.to_value(),
                    true.to_value(),
                    250u64.to_value(),
                    5u64.to_value(),
                    60_000u64.to_value(),
                    true.to_value(),
                ],
            )
            .expect("configure over generated remote");
        assert!(configured.contains("\"id\":\"primary\""));
        let detached = remote
            .generated_json(
                "Identity",
                "identity_force_detach_authority_json",
                vec![
                    loom_codec::Value::Bytes(root.as_bytes().to_vec()),
                    7u64.to_value(),
                    "authority unreachable".to_value(),
                ],
            )
            .expect("detach over generated remote");
        assert!(detached.contains("\"generation\":7"));
        let removed = remote
            .generated_json(
                "Identity",
                "identity_remove_authority_replication_json",
                vec!["primary".to_value()],
            )
            .expect("remove over generated remote");
        assert!(removed.contains("\"id\":\"primary\""));

        server.shutdown();
        drop(server_rt);

        let reopened = FileStore::open(&remote_store).expect("reopen remote store");
        assert!(
            reopened
                .authority_replication_policy_by_id("primary")
                .expect("policy lookup")
                .is_none()
        );
        let audit = reopened.audit_records().expect("audit records");
        assert_eq!(audit[0].action, "authority.replication.configure");
        assert_eq!(audit[1].action, "identity.authority.force_detach");
        assert_eq!(audit[2].action, "authority.replication.remove");

        let _ = std::fs::remove_file(&source_store);
        let _ = std::fs::remove_file(&remote_store);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
    }

    #[cfg(not(feature = "remote-client"))]
    #[test]
    fn cli_execution_context_rejects_remote_locator_when_remote_client_is_disabled() {
        let error = match open_cli_execution_context("https://127.0.0.1:1/apps/loom") {
            Ok(_) => panic!("remote locator must require remote-client feature"),
            Err(error) => error,
        };

        assert!(
            error.contains("rebuild with the `remote-client` feature"),
            "{error}"
        );
    }
}

/// Map the locator discovery mode onto the protocol discovery mode.
#[cfg(feature = "remote-client")]
fn discovery_mode(discovery: LocatorDiscovery) -> DiscoveryMode {
    match discovery {
        LocatorDiscovery::Disabled => DiscoveryMode::Disabled,
        LocatorDiscovery::WellKnown => DiscoveryMode::WellKnown,
        LocatorDiscovery::ServiceRoot => DiscoveryMode::ServiceRoot,
        LocatorDiscovery::Default => DiscoveryMode::Default,
    }
}

/// The `host` and `port` of a `scheme://host[:port]/...` URL (defaulting to 443).
#[cfg(feature = "remote-client")]
fn url_host_port(url: &str) -> Result<(String, u16), String> {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let authority = authority.split('@').next_back().unwrap_or(authority);
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => {
            let port: u16 = port
                .parse()
                .map_err(|_| format!("invalid port in endpoint URL {url:?}"))?;
            Ok((host.to_string(), port))
        }
        _ => Ok((authority.to_string(), 443)),
    }
}

/// The path component of a `scheme://host/path` URL, or `/`.
#[cfg(feature = "remote-client")]
fn url_path(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    match rest.find('/') {
        Some(index) => rest[index..].to_string(),
        None => "/".to_string(),
    }
}

/// Build a client TLS config from the resolved `tls` trust selector: `system` (verify against the OS
/// trust store via `rustls-native-certs`), `insecure-dev` (loopback development: no certificate
/// verification), or a CA-bundle PEM path (verify against exactly those anchors).
#[cfg(feature = "remote-client")]
fn build_client_config(tls: Option<&str>) -> Result<rustls::ClientConfig, String> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    match tls {
        Some("insecure-dev") => Ok(rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(InsecureServerVerifier))
            .with_no_client_auth()),
        Some(path) if std::path::Path::new(path).is_file() => {
            use rustls::pki_types::pem::PemObject;
            let mut roots = rustls::RootCertStore::empty();
            for cert in rustls::pki_types::CertificateDer::pem_file_iter(path)
                .map_err(|e| format!("read TLS trust bundle {path:?}: {e}"))?
            {
                roots
                    .add(cert.map_err(|e| format!("parse TLS trust bundle {path:?}: {e}"))?)
                    .map_err(|e| format!("add TLS trust anchor from {path:?}: {e}"))?;
            }
            Ok(rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth())
        }
        Some("system") => {
            // OS trust store: verify the server certificate against the platform's native root anchors,
            // so a self-signed endpoint is rejected unless its CA is installed system-wide.
            let mut roots = rustls::RootCertStore::empty();
            let loaded = rustls_native_certs::load_native_certs();
            let (added, _ignored) = roots.add_parsable_certificates(loaded.certs);
            if added == 0 {
                let detail = if loaded.errors.is_empty() {
                    "no platform root certificates found".to_string()
                } else {
                    loaded
                        .errors
                        .iter()
                        .map(|e| e.to_string())
                        .collect::<Vec<_>>()
                        .join("; ")
                };
                return Err(format!("system TLS trust store unavailable: {detail}"));
            }
            Ok(rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth())
        }
        Some(other) => Err(format!(
            "unsupported --tls trust selector {other:?} (expected `system`, a CA bundle path, or `insecure-dev`)"
        )),
        None => Err(
            "a remote endpoint requires a TLS trust selector (`system`, a CA bundle path, or `insecure-dev`)"
                .to_string(),
        ),
    }
}

/// A development-only server certificate verifier that accepts any certificate. Used for the
/// `insecure-dev` trust selector against loopback endpoints with self-signed certificates.
#[cfg(feature = "remote-client")]
#[derive(Debug)]
struct InsecureServerVerifier;

#[cfg(feature = "remote-client")]
impl rustls::client::danger::ServerCertVerifier for InsecureServerVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::aws_lc_rs::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(all(
    test,
    feature = "serve",
    feature = "remote-client",
    feature = "integration-tests"
))]
mod live_tests {
    use super::*;
    use loom_locator::Discovery as LocatorDiscovery;
    use loom_remote_protocol::api_types::Uuid;
    use loom_remote_protocol::generated_api::Sessions;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_store(tag: &str) -> String {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "loomcli-remote-facade-{tag}-{}-{seq}.loom",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        FileStore::create_with_profile(&path, Algo::Blake3).expect("create store");
        path.to_string_lossy().into_owned()
    }

    /// A remote-backed MCP host forwards the KV, CAS, queue, ledger, time-series, and full-text search
    /// tool families to a live `loom serve remote` endpoint and refuses operations that need a local
    /// handle. `loom mcp <remote>` (URL target) connects and the tool calls succeed over the wire, while a
    /// local-handle op (`check_open`) is rejected with the remote-store error.
    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_kv_round_trip_through_remote_backend() {
        let store = temp_store("mcp-kv");

        // Seed a document through the local engine before the server binds the store, so
        // `document_get_binary` can be read back over the wire. MCP document writes use the indexed
        // server-side path so the reference index stays consistent.
        {
            let keys = KeyOpts::default();
            let mut loom = cli_open_loom(&store, &keys).expect("open store for document seed");
            let ns = ensure_facet_workspace(&mut loom, "docapp", FacetKind::Document)
                .expect("document ws");
            loom_core::document::doc_put(&mut loom, ns, "notes", "d1", b"{\"x\":1}".to_vec())
                .expect("seed doc_put");
            save_loom(&mut loom).expect("save document seed");
        }

        // Seed a two-commit VCS history through the local engine before the server binds, so the VCS
        // reads (log/status/diff/blame) have content over the wire. VCS *writes that take a caller
        // timestamp* (commit/tag_create/merge/...) reject over remote (the IDL has no timestamp param and
        // server-time forwarding would change the content digest), so they cannot be exercised via the
        // remote MCP host and are seeded here instead.
        let (vcs_c1, vcs_c2) = {
            let keys = KeyOpts::default();
            let mut loom = cli_open_loom(&store, &keys).expect("open store for vcs seed");
            let ns = ensure_facet_workspace(&mut loom, "vcsws", FacetKind::Files).expect("vcs ws");
            loom.write_file(ns, "/f.txt", b"v1", 0o100644)
                .expect("vcs seed write 1");
            let c1 = loom
                .commit(ns, "tester", "first", 1000)
                .expect("vcs seed commit 1")
                .to_string();
            loom.write_file(ns, "/f.txt", b"v2", 0o100644)
                .expect("vcs seed write 2");
            let c2 = loom
                .commit(ns, "tester", "second", 2000)
                .expect("vcs seed commit 2")
                .to_string();
            save_loom(&mut loom).expect("save vcs seed");
            (c1, c2)
        };

        // Seed a small graph through the local engine because remote edge writes require the local
        // reference-index overlay.
        {
            let keys = KeyOpts::default();
            let mut loom = cli_open_loom(&store, &keys).expect("open store for graph seed");
            let ns =
                ensure_facet_workspace(&mut loom, "graphws", FacetKind::Graph).expect("graph ws");
            loom_core::graph::graph_upsert_node(
                &mut loom,
                ns,
                "g",
                "n1",
                loom_core::graph::Props::new(),
            )
            .expect("seed node n1");
            loom_core::graph::graph_upsert_node(
                &mut loom,
                ns,
                "g",
                "n2",
                loom_core::graph::Props::new(),
            )
            .expect("seed node n2");
            loom_core::graph::graph_upsert_edge(
                &mut loom,
                ns,
                "g",
                "e1",
                "n1",
                "n2",
                "rel",
                loom_core::graph::Props::new(),
            )
            .expect("seed edge e1");
            save_loom(&mut loom).expect("save graph seed");
        }

        // Seed a two-commit SQL history through the local engine before the server binds, so the SQL-read
        // tools have committed content over the wire. The SQL read-side (`sql_read_table`,
        // `sql_read_table_at`, `sql_index_scan(_at)`, `sql_diff`, `sql_table_diff`, `sql_blame`,
        // `sql_list_databases`) is unary and forwards byte-for-byte; `sql_exec`/`sql_query`/`sql_commit`
        // are handle/stream and reject over remote, so the data is seeded here rather than written through
        // the remote host.
        let (sql_c1, sql_c2) = {
            let seed_mcp = uldren_loom_mcp::LoomMcp::new(
                uldren_loom_mcp::StoreAccess::per_request(&store, None),
            );
            seed_mcp
                .write_sql_exec(
                    "salesdb_ws",
                    "salesdb",
                    "CREATE TABLE orders (id INTEGER PRIMARY KEY, item TEXT)",
                )
                .expect("seed create orders");
            seed_mcp
                .write_sql_exec(
                    "salesdb_ws",
                    "salesdb",
                    "INSERT INTO orders VALUES (1, 'widget')",
                )
                .expect("seed insert row1");
            let c1 = seed_mcp
                .write_sql_commit("salesdb_ws", "seed", "sql c1", 1000)
                .expect("seed sql commit 1");
            seed_mcp
                .write_sql_exec(
                    "salesdb_ws",
                    "salesdb",
                    "INSERT INTO orders VALUES (2, 'gadget')",
                )
                .expect("seed insert row2");
            let c2 = seed_mcp
                .write_sql_commit("salesdb_ws", "seed", "sql c2", 2000)
                .expect("seed sql commit 2");
            (c1, c2)
        };

        // Seed a Dataframe workspace pre-bind: a CSV file in the working tree and a frame whose plan scans
        // it. The Dataframe read tools (`collect`/`preview`/`plan_digest`/`source_digests`) forward
        // byte-for-byte because the MCP host re-encodes with `facet_cbor::dataframe_batch_cbor` /
        // `digest_strings_cbor`, which are byte-identical to the server's `DataframeBatch::encode` /
        // `loom_wire::digest_list_to_cbor` (same `loom_codec` codec, same column tuple, same shared
        // `loom_types::cell_value`); `create`/`materialize` are clean writes forwarded to the IDL method.
        let df_plan_bytes = {
            use loom_core::dataframe::{
                DataframeInputFormat, DataframeMaterialization, DataframeMaterializationTarget,
                DataframeOperation, DataframePlan, DataframeSourceBinding, DataframeSourceKind,
            };
            DataframePlan::new(vec![
                DataframeSourceBinding::new(
                    "events",
                    DataframeSourceKind::Files,
                    "events.csv",
                    DataframeInputFormat::Csv,
                )
                .with_option("has_header", "true"),
            ])
            .expect("df plan sources")
            .with_operations(vec![
                DataframeOperation::Scan {
                    source: "events".into(),
                },
                DataframeOperation::Select {
                    columns: vec!["id".into(), "kind".into()],
                },
            ])
            .expect("df plan operations")
            .with_materialization(DataframeMaterialization::new(
                DataframeMaterializationTarget::Columnar,
                Some("analytics/out".into()),
                DataframeInputFormat::Parquet,
            ))
            .expect("df plan materialization")
            .encode()
        };
        {
            let seed_mcp = uldren_loom_mcp::LoomMcp::new(
                uldren_loom_mcp::StoreAccess::per_request(&store, None),
            );
            seed_mcp
                .write_workspace_create(Some("dfws"), "dataframe")
                .expect("seed dataframe workspace");
            seed_mcp
                .write_fs_write_file(
                    "dfws",
                    "events.csv",
                    b"id,kind\n1,purchase\n2,view\n3,purchase\n",
                    0o100644,
                )
                .expect("seed dataframe csv");
            seed_mcp
                .write_dataframe_create("dfws", "etl", &df_plan_bytes)
                .expect("seed dataframe frame");
        }

        // Seed a watched Files workspace pre-bind with two commits, so a resume-from-c0 poll yields exactly
        // one non-root event whose `parent` is `Some(c0)`. The live assertions below prove that `parent`
        // round-trips over remote.
        let watch_c0 = {
            let keys = KeyOpts::default();
            let mut loom = cli_open_loom(&store, &keys).expect("open store for watch seed");
            let ns =
                ensure_facet_workspace(&mut loom, "watchws", FacetKind::Files).expect("watch ws");
            loom.write_file(ns, "a.txt", b"a", 0o644)
                .expect("watch seed write a.txt @ c0");
            let c0 = loom
                .commit(ns, "seed", "watch c0", 0)
                .expect("watch seed commit c0")
                .to_string();
            loom.write_file(ns, "a.txt", b"a2", 0o644)
                .expect("watch seed rewrite a.txt");
            loom.write_file(ns, "b.txt", b"b", 0o644)
                .expect("watch seed write b.txt");
            loom.commit(ns, "seed", "watch c1", 1)
                .expect("watch seed commit c1");
            save_loom(&mut loom).expect("save watch seed");
            c0
        };

        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = std::env::temp_dir();
        let cert_path = dir.join(format!("loomcli-mcp-remote-{}.crt", std::process::id()));
        let key_path = dir.join(format!("loomcli-mcp-remote-{}.key", std::process::id()));
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.signing_key.serialize_pem()).unwrap();
        let tls = loom_hosted_core::HostedTlsConfig::from_pem_files(
            &cert_path.to_string_lossy(),
            &key_path.to_string_lossy(),
        )
        .expect("server tls");
        let options = loom_hosted_core::remote::RemoteServeOptions::from_cli(
            "127.0.0.1:0".to_string(),
            "https://localhost/apps/loom".to_string(),
            None,
            vec![loom_hosted_core::remote::RemoteAuthMode::Interactive],
            vec![loom_hosted_core::remote::RemoteTlsTrust::System],
            60_000,
            1 << 20,
            None,
        );
        let server_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = server_rt
            .block_on(crate::serve_cmd::bind_remote_endpoint(
                &store,
                &options,
                tls.server_config(),
            ))
            .expect("bind remote endpoint");
        let addr = server.local_addr();

        let target = RemoteTarget {
            url: format!("https://127.0.0.1:{}/apps/loom", addr.port()),
            auth: None,
            tls: Some("insecure-dev".to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };

        // The MCP host backed by the remote endpoint, built exactly as `run_mcp` builds it for a remote
        // locator.
        let backend = McpRemoteBackend::connect(&target).expect("mcp remote backend connect");
        let mcp =
            uldren_loom_mcp::LoomMcp::new(uldren_loom_mcp::StoreAccess::remote(Arc::new(backend)));

        let key = loom_core::kv::key_to_cbor(&loom_core::Value::Text("mk".to_string()));
        mcp.write_kv_put("kvapp", "c", &key, b"hello".to_vec())
            .expect("remote mcp kv put");
        assert_eq!(
            mcp.read_kv_get("kvapp", "c", &key)
                .expect("remote mcp kv get"),
            Some(b"hello".to_vec())
        );
        assert!(
            !mcp.read_kv_list("kvapp", "c")
                .expect("remote mcp kv list")
                .is_empty()
        );
        let lo = loom_core::kv::key_to_cbor(&loom_core::Value::Text(String::new()));
        let hi = loom_core::kv::key_to_cbor(&loom_core::Value::Text("~".to_string()));
        assert!(
            !mcp.read_kv_range("kvapp", "c", &lo, &hi, None)
                .expect("remote mcp kv range")
                .is_empty()
        );
        assert!(
            mcp.write_kv_delete("kvapp", "c", &key)
                .expect("remote mcp kv delete")
        );
        assert_eq!(
            mcp.read_kv_get("kvapp", "c", &key)
                .expect("remote mcp kv get after delete"),
            None
        );

        // CAS over the wire: put -> get/has/list -> delete.
        let digest = mcp
            .write_cas_put("casapp", b"blobdata")
            .expect("remote mcp cas put");
        assert_eq!(
            mcp.read_cas_get("casapp", &digest)
                .expect("remote mcp cas get"),
            Some(b"blobdata".to_vec())
        );
        assert!(
            mcp.read_cas_has("casapp", &digest)
                .expect("remote mcp cas has")
        );
        assert!(
            mcp.read_cas_list("casapp")
                .expect("remote mcp cas list")
                .contains(&digest)
        );
        assert!(
            mcp.write_cas_delete("casapp", &digest)
                .expect("remote mcp cas delete")
        );

        // Queue over the wire: append -> get/len/range -> consumer position/read/advance.
        assert_eq!(
            mcp.write_queue_append("qapp", "s", b"q0")
                .expect("remote mcp queue append"),
            0
        );
        mcp.write_queue_append("qapp", "s", b"q1")
            .expect("remote mcp queue append 2");
        assert_eq!(
            mcp.read_queue_get("qapp", "s", 0)
                .expect("remote mcp queue get"),
            Some(b"q0".to_vec())
        );
        assert_eq!(
            mcp.read_queue_len("qapp", "s")
                .expect("remote mcp queue len"),
            2
        );
        assert_eq!(
            mcp.read_queue_range("qapp", "s", 0, 2)
                .expect("remote mcp queue range"),
            vec![b"q0".to_vec(), b"q1".to_vec()]
        );
        assert_eq!(
            mcp.read_queue_consumer_position("qapp", "s", "w")
                .expect("remote mcp consumer position"),
            0
        );
        assert_eq!(
            mcp.read_queue_consumer_read("qapp", "s", "w", 10)
                .expect("remote mcp consumer read"),
            vec![b"q0".to_vec(), b"q1".to_vec()]
        );
        mcp.write_queue_consumer_advance("qapp", "s", "w", 2)
            .expect("remote mcp consumer advance");
        assert_eq!(
            mcp.read_queue_consumer_position("qapp", "s", "w")
                .expect("remote mcp consumer position after advance"),
            2
        );

        // Ledger over the wire: append -> get/head/len/verify.
        assert_eq!(
            mcp.write_ledger_append("ledapp", "log", b"e0".to_vec())
                .expect("remote mcp ledger append"),
            0
        );
        assert_eq!(
            mcp.read_ledger_get("ledapp", "log", 0)
                .expect("remote mcp ledger get"),
            Some(b"e0".to_vec())
        );
        assert!(
            mcp.read_ledger_head("ledapp", "log")
                .expect("remote mcp ledger head")
                .is_some()
        );
        assert_eq!(
            mcp.read_ledger_len("ledapp", "log")
                .expect("remote mcp ledger len"),
            1
        );
        mcp.read_ledger_verify("ledapp", "log")
            .expect("remote mcp ledger verify");

        // TimeSeries over the wire: put -> get.
        mcp.write_timeseries_put("tsapp", "cpu", 100, b"0.5".to_vec())
            .expect("remote mcp ts put");
        assert_eq!(
            mcp.read_timeseries_get("tsapp", "cpu", 100)
                .expect("remote mcp ts get"),
            Some(b"0.5".to_vec())
        );
        // TimeSeries range decodes the wire `Series` (one seeded point).
        assert_eq!(
            mcp.read_timeseries_range("tsapp", "cpu", 0, 1000)
                .expect("remote mcp ts range")
                .len(),
            1
        );
        // A later point makes latest non-trivial and verifies that the wire payload carries timestamp
        // and value together.
        mcp.write_timeseries_put("tsapp", "cpu", 200, b"0.9".to_vec())
            .expect("remote mcp ts put (second point)");
        let point = mcp
            .read_timeseries_latest("tsapp", "cpu")
            .expect("remote mcp ts latest")
            .expect("latest point present");
        // The `[ts, value]` payload carries both fields: the remote point is the most recent one, with
        // its timestamp intact (the value-only wire form could not have carried ts=200).
        assert_eq!(point.ts, 200, "latest must be the most recent timestamp");
        assert_eq!(point.value, b"0.9".to_vec(), "latest value mismatch");

        // Full-text search over the wire: create -> index -> get/ids -> delete. The mapping and document
        // are the canonical-CBOR shapes the server decodes (`{field: [type_tag, stored, faceted]}` and
        // `{field: value}`); `body` is a stored text field (type tag 0).
        let mapping = loom_codec::encode(&loom_codec::Value::Map(vec![(
            loom_codec::Value::Text("body".to_string()),
            loom_codec::Value::Array(vec![
                loom_codec::Value::Uint(0),
                loom_codec::Value::Bool(true),
                loom_codec::Value::Bool(false),
            ]),
        )]))
        .expect("mapping cbor");
        mcp.write_fts_create("ftsapp", "idx", &mapping)
            .expect("remote mcp fts create");
        let doc = loom_codec::encode(&loom_codec::Value::Map(vec![(
            loom_codec::Value::Text("body".to_string()),
            loom_codec::Value::Text("hello loom".to_string()),
        )]))
        .expect("doc cbor");
        mcp.write_fts_index("ftsapp", "idx", b"d1".to_vec(), &doc)
            .expect("remote mcp fts index");
        assert!(
            mcp.read_fts_get("ftsapp", "idx", b"d1")
                .expect("remote mcp fts get")
                .is_some()
        );
        assert!(
            !mcp.read_fts_ids("ftsapp", "idx", None)
                .expect("remote mcp fts ids")
                .is_empty()
        );
        assert!(
            mcp.write_fts_delete("ftsapp", "idx", b"d1")
                .expect("remote mcp fts delete")
        );

        // Columnar over the wire: create (schema is a CBOR array of `[name, type_tag]`) then
        // rows/columns/scan/inspect/source-digest and compact.
        let schema = loom_codec::encode(&loom_codec::Value::Array(vec![loom_codec::Value::Array(
            vec![
                loom_codec::Value::Text("v".to_string()),
                loom_codec::Value::Uint(3),
            ],
        )]))
        .expect("columnar schema cbor");
        mcp.write_columnar_create("colapp", "t", &schema, 1024)
            .expect("remote mcp columnar create");
        assert_eq!(
            mcp.read_columnar_rows("colapp", "t")
                .expect("remote mcp columnar rows"),
            0
        );
        assert!(
            !mcp.read_columnar_columns("colapp", "t")
                .expect("remote mcp columnar columns")
                .is_empty()
        );
        mcp.read_columnar_scan("colapp", "t")
            .expect("remote mcp columnar scan");
        mcp.read_columnar_inspect("colapp", "t")
            .expect("remote mcp columnar inspect");
        mcp.read_columnar_source_digest("colapp", "t")
            .expect("remote mcp columnar source digest");
        mcp.write_columnar_compact("colapp", "t")
            .expect("remote mcp columnar compact");

        // PIM (Calendar/Contacts/Mail) over the wire: create the container, put a typed entry, then read
        // it back through the decode bridges - typed `get_*`, the aggregate `list_*`/`search`, calendar
        // `range` (the occurrence pairs), and the text serialization accessors (`to_ics`/`to_vcard`/
        // `to_eml`).
        mcp.write_calendar_create_collection("pimapp", "alice", "work", "Work", "event")
            .expect("remote mcp calendar create_collection");
        let cal_entry =
            loom_core::calendar::CalendarEntry::event("evt-1", "Standup", "20240115T100000");
        mcp.write_calendar_put_entry("pimapp", "alice", "work", &cal_entry.encode())
            .expect("remote mcp calendar put_entry");
        assert_eq!(
            mcp.read_calendar_list_collections("pimapp", "alice")
                .expect("remote mcp calendar list_collections"),
            vec!["work".to_string()]
        );
        assert_eq!(
            mcp.read_calendar_get_entry("pimapp", "alice", "work", "evt-1")
                .expect("remote mcp calendar get_entry")
                .expect("calendar entry present")
                .summary,
            "Standup"
        );
        assert_eq!(
            mcp.read_calendar_list_entries("pimapp", "alice", "work")
                .expect("remote mcp calendar list_entries")
                .len(),
            1
        );
        assert_eq!(
            mcp.read_calendar_range(
                "pimapp",
                "alice",
                "work",
                "20240101T000000",
                "20240201T000000"
            )
            .expect("remote mcp calendar range")
            .len(),
            1
        );
        assert_eq!(
            mcp.read_calendar_search("pimapp", "alice", "work", "event", "Standup")
                .expect("remote mcp calendar search")
                .len(),
            1
        );
        assert!(
            mcp.read_calendar_to_ics("pimapp", "alice", "work", "evt-1")
                .expect("remote mcp calendar to_ics")
                .expect("ics present")
                .contains("Standup")
        );

        mcp.write_contacts_create_book("pimapp", "alice", "friends", "Friends")
            .expect("remote mcp contacts create_book");
        let contact = loom_core::contacts::ContactEntry::new("c-1", "Bob Jones");
        mcp.write_contacts_put_entry("pimapp", "alice", "friends", &contact.encode())
            .expect("remote mcp contacts put_entry");
        assert_eq!(
            mcp.read_contacts_list_books("pimapp", "alice")
                .expect("remote mcp contacts list_books"),
            vec!["friends".to_string()]
        );
        assert_eq!(
            mcp.read_contacts_get_entry("pimapp", "alice", "friends", "c-1")
                .expect("remote mcp contacts get_entry")
                .expect("contact present")
                .full_name,
            "Bob Jones"
        );
        assert_eq!(
            mcp.read_contacts_list_entries("pimapp", "alice", "friends")
                .expect("remote mcp contacts list_entries")
                .len(),
            1
        );
        assert!(
            mcp.read_contacts_to_vcard("pimapp", "alice", "friends", "c-1")
                .expect("remote mcp contacts to_vcard")
                .expect("vcard present")
                .contains("Bob Jones")
        );

        mcp.write_mail_create_mailbox("pimapp", "alice", "inbox", "Inbox")
            .expect("remote mcp mail create_mailbox");
        let raw: &[u8] =
            b"From: bob@example.com\r\nTo: alice@example.com\r\nSubject: Hello\r\n\r\nHi there\r\n";
        mcp.write_mail_ingest_message("pimapp", "alice", "inbox", "m-1", raw)
            .expect("remote mcp mail ingest_message");
        assert_eq!(
            mcp.read_mail_list_mailboxes("pimapp", "alice")
                .expect("remote mcp mail list_mailboxes"),
            vec!["inbox".to_string()]
        );
        assert_eq!(
            mcp.read_mail_get_message("pimapp", "alice", "inbox", "m-1")
                .expect("remote mcp mail get_message")
                .expect("message present")
                .subject,
            "Hello"
        );
        assert_eq!(
            mcp.read_mail_list_messages("pimapp", "alice", "inbox")
                .expect("remote mcp mail list_messages")
                .len(),
            1
        );
        mcp.write_mail_set_flags("pimapp", "alice", "inbox", "m-1", &["\\Seen".to_string()])
            .expect("remote mcp mail set_flags");
        assert_eq!(
            mcp.read_mail_get_flags("pimapp", "alice", "inbox", "m-1")
                .expect("remote mcp mail get_flags"),
            vec!["\\Seen".to_string()]
        );
        assert!(
            mcp.read_mail_to_eml("pimapp", "alice", "inbox", "m-1")
                .expect("remote mcp mail to_eml")
                .expect("eml present")
                .windows(5)
                .any(|w| w == b"Hello")
        );

        // Filesystem over the wire: write/read/append/read-at/write-at/truncate/remove and symlink +
        // read-link. These forward 1:1 to the generated `FileSystem` methods (no decode bridge needed).
        mcp.write_fs_write_file("fsapp", "/a.txt", b"hello", 0o100644)
            .expect("remote mcp fs write_file");
        assert_eq!(
            mcp.read_fs_read_file("fsapp", "/a.txt")
                .expect("remote mcp fs read_file"),
            b"hello".to_vec()
        );
        mcp.write_fs_append_file("fsapp", "/a.txt", b" world")
            .expect("remote mcp fs append_file");
        assert_eq!(
            mcp.read_fs_read_file("fsapp", "/a.txt")
                .expect("remote mcp fs read_file after append"),
            b"hello world".to_vec()
        );
        assert_eq!(
            mcp.read_fs_read_at("fsapp", "/a.txt", 6, 5)
                .expect("remote mcp fs read_at"),
            b"world".to_vec()
        );
        mcp.write_fs_write_at("fsapp", "/a.txt", 0, b"HELLO")
            .expect("remote mcp fs write_at");
        assert_eq!(
            mcp.read_fs_read_at("fsapp", "/a.txt", 0, 5)
                .expect("remote mcp fs read_at after write_at"),
            b"HELLO".to_vec()
        );
        mcp.write_fs_truncate("fsapp", "/a.txt", 5)
            .expect("remote mcp fs truncate");
        assert_eq!(
            mcp.read_fs_read_file("fsapp", "/a.txt")
                .expect("remote mcp fs read_file after truncate"),
            b"HELLO".to_vec()
        );
        mcp.write_fs_symlink("fsapp", "/a.txt", "/link.txt")
            .expect("remote mcp fs symlink");
        assert_eq!(
            mcp.read_fs_read_link("fsapp", "/link.txt")
                .expect("remote mcp fs read_link"),
            "/a.txt"
        );
        mcp.write_fs_remove_file("fsapp", "/a.txt")
            .expect("remote mcp fs remove_file");

        // Vector over the wire: create index, upsert, get, ids, exact search, metadata index, delete.
        // The reads forward the server's canonical CBOR unchanged (proven byte-identical to the MCP
        // facet encoders `vector_entry_cbor`/`vector_strings_cbor`/`vector_hits_cbor`).
        mcp.write_vector_create("vecapp", "v", 3, 1)
            .expect("remote mcp vector create");
        let vec_bytes: Vec<u8> = [1.0f32, 2.0, 3.0]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        mcp.write_vector_upsert("vecapp", "v", "e1", &vec_bytes, &[])
            .expect("remote mcp vector upsert");
        assert!(
            mcp.read_vector_get("vecapp", "v", "e1")
                .expect("remote mcp vector get")
                .is_some()
        );
        let ids_wire = mcp
            .read_vector_ids("vecapp", "v", None)
            .expect("remote mcp vector ids");
        match loom_codec::decode(&ids_wire).expect("vector ids cbor") {
            loom_codec::Value::Array(items) => assert_eq!(items.len(), 1),
            _ => panic!("vector ids not a CBOR array"),
        }
        let hits = mcp
            .read_vector_search("vecapp", "v", &vec_bytes, 1, &[])
            .expect("remote mcp vector search");
        match loom_codec::decode(&hits).expect("vector hits cbor") {
            loom_codec::Value::Array(items) => assert_eq!(items.len(), 1),
            _ => panic!("vector hits not a CBOR array"),
        }
        mcp.write_vector_create_metadata_index("vecapp", "v", "kind")
            .expect("remote mcp vector create_metadata_index");
        let keys_wire = mcp
            .read_vector_metadata_index_keys("vecapp", "v")
            .expect("remote mcp vector metadata_index_keys");
        match loom_codec::decode(&keys_wire).expect("vector index keys cbor") {
            loom_codec::Value::Array(items) => assert_eq!(items.len(), 1),
            _ => panic!("vector index keys not a CBOR array"),
        }
        assert!(
            mcp.write_vector_delete("vecapp", "v", "e1")
                .expect("remote mcp vector delete")
        );

        // Document reads over the wire: binary get and the binary-derived range helper.
        // The document was seeded through the engine before bind (writes are local-only, see above).
        assert_eq!(
            mcp.read_document_get_binary("docapp", "notes", "d1")
                .expect("remote mcp document get")
                .expect("document present")
                .bytes,
            b"{\"x\":1}".to_vec()
        );
        assert_eq!(
            mcp.read_document_get_range("docapp", "notes", "d1", Some(1), Some(3))
                .expect("remote mcp document get_range")
                .expect("document range present"),
            b"\"x\"".to_vec()
        );
        // document_list_binary decodes the server's `Collection::encode` wire back into a `Collection`.
        assert!(
            mcp.read_document_list("docapp", "notes")
                .expect("remote mcp document list")
                .ids()
                .any(|id| id == "d1"),
            "document list should contain the seeded d1"
        );

        // SQL-read tools over the wire. Each forwards the unary IDL method and the MCP host
        // re-encodes with the same `loom_sql::result_cbor::*` the local read path uses, so the remote bytes
        // must be identical to a local read of the same seeded store. A local per-request host over the
        // same on-disk store gives the ground-truth encoding to compare against.
        let local_ref =
            uldren_loom_mcp::LoomMcp::new(uldren_loom_mcp::StoreAccess::per_request(&store, None));
        let sql_expected_head = local_ref
            .read_sql_read_table("salesdb_ws", "salesdb", "orders")
            .expect("local sql read_table head");
        let sql_remote_head = mcp
            .read_sql_read_table("salesdb_ws", "salesdb", "orders")
            .expect("remote mcp sql read_table head");
        assert!(!sql_remote_head.is_empty(), "sql read_table head is empty");
        assert_eq!(
            sql_remote_head, sql_expected_head,
            "sql_read_table must be byte-identical local vs remote"
        );
        // A committed snapshot at c1 (one row) is a distinct, smaller table than head (two rows); parity
        // must hold and it must differ from head, proving the `commit` argument is forwarded and honoured.
        let sql_expected_c1 = local_ref
            .read_sql_read_table_at("salesdb_ws", "salesdb", "orders", &sql_c1)
            .expect("local sql read_table_at c1");
        let sql_remote_c1 = mcp
            .read_sql_read_table_at("salesdb_ws", "salesdb", "orders", &sql_c1)
            .expect("remote mcp sql read_table_at c1");
        assert_eq!(
            sql_remote_c1, sql_expected_c1,
            "sql_read_table_at must be byte-identical local vs remote"
        );
        assert_ne!(
            sql_remote_c1, sql_remote_head,
            "the c1 snapshot (1 row) must differ from head (2 rows)"
        );
        // sql_diff between the two commits: the row-level diff must be byte-identical over the wire.
        let sql_expected_diff = local_ref
            .read_sql_diff("salesdb_ws", "salesdb", "orders", &sql_c1, &sql_c2)
            .expect("local sql diff");
        let sql_remote_diff = mcp
            .read_sql_diff("salesdb_ws", "salesdb", "orders", &sql_c1, &sql_c2)
            .expect("remote mcp sql diff");
        assert!(!sql_remote_diff.is_empty(), "sql diff is empty");
        assert_eq!(
            sql_remote_diff, sql_expected_diff,
            "sql_diff must be byte-identical local vs remote"
        );
        // sql_table_diff (schema-aware) between the two commits: byte-identical over the wire.
        let sql_expected_tdiff = local_ref
            .read_sql_table_diff("salesdb_ws", "salesdb", "orders", &sql_c1, &sql_c2)
            .expect("local sql table_diff");
        let sql_remote_tdiff = mcp
            .read_sql_table_diff("salesdb_ws", "salesdb", "orders", &sql_c1, &sql_c2)
            .expect("remote mcp sql table_diff");
        assert_eq!(
            sql_remote_tdiff, sql_expected_tdiff,
            "sql_table_diff must be byte-identical local vs remote"
        );
        // sql_list_databases: the decoded database-name list must contain the seeded db.
        let sql_dbs = mcp
            .read_collections("salesdb_ws", FacetKind::Sql)
            .expect("remote mcp sql list_databases");
        assert!(
            sql_dbs.iter().any(|d| d == "salesdb"),
            "sql_list_databases must contain the seeded db, got {sql_dbs:?}"
        );
        // sql_exec/sql_query/sql_commit are handle/stream and are not remote-capable: the host has no local
        // handle to open a `LoomSqlStore` against, so a direct call rejects rather than silently no-ops.
        assert!(
            mcp.write_sql_exec(
                "salesdb_ws",
                "salesdb",
                "INSERT INTO orders VALUES (3, 'sprocket')"
            )
            .is_err(),
            "sql_exec must reject over a remote-backed host"
        );

        // Dataframe tools over the wire. The frame `etl` and its CSV source were seeded
        // pre-bind. Reads must be byte-identical to a local per-request read of the same store, and the
        // write path (`create`/`materialize`) must persist and read back.
        let df_expected_collect = local_ref
            .read_dataframe_collect("dfws", "etl")
            .expect("local dataframe collect");
        let df_remote_collect = mcp
            .read_dataframe_collect("dfws", "etl")
            .expect("remote mcp dataframe collect");
        assert!(!df_remote_collect.is_empty(), "dataframe collect is empty");
        assert_eq!(
            df_remote_collect, df_expected_collect,
            "dataframe_collect must be byte-identical local vs remote"
        );
        // preview(1) is a strict prefix of collect (fewer rows) and must also match byte-for-byte.
        let df_expected_preview = local_ref
            .read_dataframe_preview("dfws", "etl", 1)
            .expect("local dataframe preview");
        let df_remote_preview = mcp
            .read_dataframe_preview("dfws", "etl", 1)
            .expect("remote mcp dataframe preview");
        assert_eq!(
            df_remote_preview, df_expected_preview,
            "dataframe_preview must be byte-identical local vs remote"
        );
        assert_ne!(
            df_remote_preview, df_remote_collect,
            "preview(1) must differ from the full collect (row limit applied)"
        );
        // plan_digest is the `algo:hex` plan digest string; it must match exactly.
        assert_eq!(
            mcp.read_dataframe_plan_digest("dfws", "etl")
                .expect("remote mcp dataframe plan_digest"),
            local_ref
                .read_dataframe_plan_digest("dfws", "etl")
                .expect("local dataframe plan_digest"),
            "dataframe_plan_digest must match local vs remote"
        );
        // source_digests is the canonical CBOR text array of source digests; byte-identical over the wire.
        assert_eq!(
            mcp.read_dataframe_source_digests("dfws", "etl")
                .expect("remote mcp dataframe source_digests"),
            local_ref
                .read_dataframe_source_digests("dfws", "etl")
                .expect("local dataframe source_digests"),
            "dataframe_source_digests must be byte-identical local vs remote"
        );
        // Write path over remote: create a second frame from the same plan, then read it back and compare
        // to a local read of the same frame (proves the remote `create` persisted the plan correctly).
        mcp.write_dataframe_create("dfws", "etl2", &df_plan_bytes)
            .expect("remote mcp dataframe create");
        assert_eq!(
            mcp.read_dataframe_collect("dfws", "etl2")
                .expect("remote mcp dataframe collect etl2"),
            local_ref
                .read_dataframe_collect("dfws", "etl2")
                .expect("local dataframe collect etl2"),
            "a frame created over remote must collect identically to a local read"
        );
        // materialize is a write executed over the wire; this plan targets Columnar, which persists to the
        // columnar facet and returns no digest (a Cas-target plan would return `Some(algo:hex)`). The
        // assertion is that the write succeeds over remote and the `Option<Digest>` -> `Option<String>`
        // transform yields the expected `None`.
        let df_materialized = mcp
            .write_dataframe_materialize("dfws", "etl")
            .expect("remote mcp dataframe materialize");
        assert!(
            df_materialized.is_none(),
            "columnar materialize returns no digest, got {df_materialized:?}"
        );

        // Watch tools over the wire. `subscribe` resolves the workspace and builds the same
        // selector wire form; `poll` decodes the canonical batch (carrying `parent`) and rebuilds the
        // MCP summary. Both must match a local per-request read exactly, including each event's `parent`.
        let watch_branch = loom_core::workspace::DEFAULT_BRANCH;
        let remote_sub = mcp
            .read_watch_subscribe("watchws", watch_branch, Some(&watch_c0), None, None, None)
            .expect("remote mcp watch subscribe");
        let local_sub = local_ref
            .read_watch_subscribe("watchws", watch_branch, Some(&watch_c0), None, None, None)
            .expect("local watch subscribe");
        assert_eq!(
            remote_sub.cursor, local_sub.cursor,
            "watch_subscribe cursor must be identical local vs remote"
        );
        let remote_batch = mcp
            .read_watch_poll("watchws", &remote_sub.cursor, 10)
            .expect("remote mcp watch poll");
        let local_batch = local_ref
            .read_watch_poll("watchws", &remote_sub.cursor, 10)
            .expect("local watch poll");
        assert_eq!(
            remote_batch.events.len(),
            1,
            "resume-from-c0 poll should yield exactly one event"
        );
        assert!(
            remote_batch.events[0].parent.is_some(),
            "the non-root event must carry a parent"
        );
        assert_eq!(
            remote_batch, local_batch,
            "watch_poll batch (including each event's parent) must match local vs remote"
        );

        // SQL handle-stream: `sql_exec` is wired via a per-request SqlSession (open -> exec ->
        // close in the backend) and forwards byte-clean `exec_cbor`; `sql_query`/`sql_commit` reject
        // in-method with a precise contract reason. A remote CREATE then INSERT into the same db proves
        // the session-per-call path persists across calls; the INSERT `exec_cbor` is compared to an
        // identical run on an INDEPENDENT local store (no read-back, no cross-path ns resolution) to prove
        // byte-parity of the payload.
        mcp.write_sql_exec(
            "sqlx_remote",
            "main",
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)",
        )
        .expect("remote mcp sql_exec create");
        let remote_insert = mcp
            .write_sql_exec("sqlx_remote", "main", "INSERT INTO t VALUES (1, 'widget')")
            .expect("remote mcp sql_exec insert (persistence across sessions)");
        let sql_local_path = temp_store("mcp-sqlx-local");
        let sql_local = uldren_loom_mcp::LoomMcp::new(uldren_loom_mcp::StoreAccess::per_request(
            &sql_local_path,
            None,
        ));
        sql_local
            .write_sql_exec(
                "db",
                "main",
                "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)",
            )
            .expect("local sql_exec create");
        let local_insert = sql_local
            .write_sql_exec("db", "main", "INSERT INTO t VALUES (1, 'widget')")
            .expect("local sql_exec insert");
        assert!(!remote_insert.is_empty(), "sql_exec payload is empty");
        assert_eq!(
            remote_insert, local_insert,
            "sql_exec exec_cbor must be byte-identical local vs remote"
        );
        // The read-only SQL result payload is byte-identical to a local read of the same rows.
        let sql_query_remote = mcp
            .read_sql_query("sqlx_remote", "main", "SELECT id, v FROM t")
            .expect("remote mcp sql_query");
        let sql_query_local = sql_local
            .read_sql_query("db", "main", "SELECT id, v FROM t")
            .expect("local sql_query");
        assert!(!sql_query_remote.is_empty(), "sql_query result is empty");
        assert_eq!(
            sql_query_remote, sql_query_local,
            "sql_query full-result exec_cbor must be byte-identical local vs remote"
        );
        // The read-only/no-persist contract holds: a mutating statement through sql_query is rejected.
        assert!(
            mcp.read_sql_query("sqlx_remote", "main", "INSERT INTO t VALUES (2, 'gadget')")
                .is_err(),
            "sql_query must reject a mutating statement (read-only, no persist)"
        );
        // Timestamped commit digest parity. A commit over remote at a fixed `timestamp_ms`
        // produces the same content-addressed digest as a local commit with identical inputs (tree +
        // author + message + ts), because the IDL carries the caller timestamp rather than stamping
        // server time. Two independent stores with identical content + inputs must agree.
        let vcs_local_path = temp_store("mcp-vcs-local");
        let vcs_local = uldren_loom_mcp::LoomMcp::new(uldren_loom_mcp::StoreAccess::per_request(
            &vcs_local_path,
            None,
        ));
        vcs_local
            .write_workspace_create(Some("vcsts"), "files")
            .expect("local vcs workspace");
        vcs_local
            .write_fs_write_file("vcsts", "a.txt", b"x", 0o100644)
            .expect("local vcs write");
        let local_commit = vcs_local
            .write_vcs_commit("vcsts", "author", "m", 5000)
            .expect("local vcs commit");
        mcp.write_workspace_create(Some("vcsts"), "files")
            .expect("remote vcs workspace");
        mcp.write_fs_write_file("vcsts", "a.txt", b"x", 0o100644)
            .expect("remote mcp vcs write");
        let remote_commit = mcp
            .write_vcs_commit("vcsts", "author", "m", 5000)
            .expect("remote mcp vcs commit");
        assert!(
            remote_commit.contains(':'),
            "commit digest should be algo:hex, got {remote_commit:?}"
        );
        assert_eq!(
            remote_commit, local_commit,
            "timestamped vcs commit digest must match local vs remote for a fixed timestamp_ms"
        );
        // sql_commit forwards over remote (the same `loom.commit` path) and returns an `algo:hex` digest.
        let sql_commit_digest = mcp
            .write_sql_commit("sqlx_remote", "author", "m", 6000)
            .expect("remote mcp sql_commit");
        assert!(
            sql_commit_digest.contains(':'),
            "sql_commit digest should be algo:hex, got {sql_commit_digest:?}"
        );
        let _ = std::fs::remove_file(&vcs_local_path);
        let _ = std::fs::remove_file(&sql_local_path);

        // The remote document-query composite must match a local read of the same store bytes.
        let local_dq = local_ref
            .read_document_query(uldren_loom_mcp::reads::DocumentQueryRead {
                workspace: "docapp",
                name: "notes",
                id_prefix: None,
                predicate: None,
                projections: &[("x", "x")],
                index: None,
                value: None,
                cursor: None,
                limit: None,
                include_document: true,
            })
            .expect("local document_query");
        let remote_dq = mcp
            .read_document_query(uldren_loom_mcp::reads::DocumentQueryRead {
                workspace: "docapp",
                name: "notes",
                id_prefix: None,
                predicate: None,
                projections: &[("x", "x")],
                index: None,
                value: None,
                cursor: None,
                limit: None,
                include_document: true,
            })
            .expect("remote document_query");
        assert_eq!(remote_dq.items.len(), 1, "one document in docapp/notes");
        assert_eq!(
            remote_dq, local_dq,
            "remote document_query must match local exactly (ids, len, digest, document, projections)"
        );
        assert!(
            remote_dq.items[0].digest.contains(':'),
            "per-item digest is algo:hex under the store's real algorithm, got {:?}",
            remote_dq.items[0].digest
        );
        mcp.write_document_put_binary("docapp", "notes", "d2", b"{}".to_vec(), None)
            .expect("remote mcp document put with reference-index overlay");
        assert!(
            mcp.read_document_get_binary("docapp", "notes", "d2")
                .expect("remote mcp document get after put")
                .is_some()
        );
        mcp.write_graph_upsert_edge(
            "graphws",
            "g",
            uldren_loom_mcp::writes::GraphEdgeWrite {
                id: "e2",
                src: "n1",
                dst: "n2",
                label: "knows",
                props: &[],
            },
        )
        .expect("remote mcp graph upsert_edge with reference-index overlay");
        assert!(
            mcp.read_graph_get_edge("graphws", "g", "e2")
                .expect("remote mcp graph get_edge after upsert")
                .is_some()
        );

        // The indexed writes update the substrate reference index server-side too, not just the
        // primary facet. Applying identical indexed document + graph-edge writes to a fresh local store and
        // to the remote-served store must leave the reference index byte-identical, and deletes must remove
        // the sources on both.
        let refidx_local_path = temp_store("mcp-refidx-local");
        {
            let ref_local = uldren_loom_mcp::LoomMcp::new(
                uldren_loom_mcp::StoreAccess::per_request(&refidx_local_path, None),
            );
            let empty_graph_props =
                loom_wire::graph::props_to_cbor(&loom_core::graph::Props::new());
            for host in [&ref_local, &mcp] {
                host.write_workspace_create(Some("refapp"), "document")
                    .expect("refapp workspace create");
                host.write_graph_upsert_node("refapp", "g", "a", &empty_graph_props)
                    .expect("refapp graph node a");
                host.write_graph_upsert_node("refapp", "g", "principal:p1", &empty_graph_props)
                    .expect("refapp graph node principal");
                host.write_document_put_binary(
                    "refapp",
                    "notes",
                    "r1",
                    b"see !ticket:T-1".to_vec(),
                    None,
                )
                .expect("indexed document put forwards");
                host.write_graph_upsert_edge(
                    "refapp",
                    "g",
                    uldren_loom_mcp::writes::GraphEdgeWrite {
                        id: "e1",
                        src: "a",
                        dst: "principal:p1",
                        label: "refers_to",
                        props: &empty_graph_props,
                    },
                )
                .expect("indexed graph upsert_edge forwards");
            }
            let index_bytes = |path: &str| -> Option<Vec<u8>> {
                let keys = KeyOpts::default();
                let mut loom = cli_open_loom(path, &keys).expect("open store for ref index");
                let ns = ensure_facet_workspace(&mut loom, "refapp", FacetKind::Document)
                    .expect("refapp ns");
                loom_reference::load_index(&loom, ns)
                    .expect("load ref index")
                    .map(|index| index.encode().expect("encode ref index"))
            };
            assert!(
                index_bytes(&store).is_some(),
                "remote indexed writes populated the reference index"
            );
            assert_eq!(
                index_bytes(&store),
                index_bytes(&refidx_local_path),
                "remote reference-index state must match a local run for identical indexed writes"
            );
            for host in [&ref_local, &mcp] {
                assert!(
                    host.write_document_delete("refapp", "notes", "r1")
                        .expect("indexed document delete forwards")
                );
                assert!(
                    host.write_graph_remove_edge("refapp", "g", "e1")
                        .expect("indexed graph remove_edge forwards")
                );
            }
            assert_eq!(
                index_bytes(&store),
                index_bytes(&refidx_local_path),
                "remote reference-index state must match local after indexed deletes"
            );
        }
        let _ = std::fs::remove_file(&refidx_local_path);

        // VCS over the wire: the clean reads decode losslessly (log, status via `status_from_cbor`, diff
        // as the LMDIFF envelope, blame via `blame_rows_from_cbor`, tag_list, merge_in_progress) and the
        // writes forward (branch/checkout, plus the timestamped writes: commit family in 396a and the
        // richer-return replay/merge writes in 396b, decoded from the canonical wire).
        assert!(
            mcp.read_vcs_log("vcsws", "main")
                .expect("remote mcp vcs log")
                .len()
                >= 2
        );
        // status decodes into a Status struct (no panic / error).
        mcp.read_vcs_status("vcsws").expect("remote mcp vcs status");
        assert!(
            !mcp.read_vcs_diff("vcsws", &vcs_c1, &vcs_c2)
                .expect("remote mcp vcs diff")
                .is_empty()
        );
        assert!(
            !mcp.read_vcs_blame("vcsws", "main")
                .expect("remote mcp vcs blame")
                .is_empty()
        );
        assert!(
            !mcp.read_vcs_merge_in_progress("vcsws")
                .expect("remote mcp vcs merge_in_progress")
        );
        mcp.read_vcs_tag_list("vcsws")
            .expect("remote mcp vcs tag_list");
        mcp.write_vcs_branch("vcsws", "feature")
            .expect("remote mcp vcs branch");
        mcp.write_vcs_checkout("vcsws", "feature")
            .expect("remote mcp vcs checkout feature");
        mcp.write_vcs_checkout("vcsws", "main")
            .expect("remote mcp vcs checkout main");
        // The richer-return timestamped replay/merge writes forward over remote and the host
        // decodes the canonical `MergeResult`/`ReplayOutcome` wire back into the same typed outcome the
        // local path returns. `feature` was branched from `main` at the same tip, so merging it back is a
        // no-op; a dry-run rebase already based on the target, and empty cherry-pick/revert lists, all
        // replay nothing. These deterministic outcomes prove the forward + wire round-trip end to end.
        assert_eq!(
            mcp.write_vcs_merge("vcsws", "feature", "tester", 3000)
                .expect("remote mcp vcs merge"),
            loom_core::MergeOutcome::UpToDate,
        );
        assert_eq!(
            mcp.write_vcs_rebase("vcsws", "main", 3000, true)
                .expect("remote mcp vcs rebase dry_run"),
            loom_core::ReplayOutcome::Empty,
        );
        assert_eq!(
            mcp.write_vcs_cherry_pick("vcsws", &[], 3000, true)
                .expect("remote mcp vcs cherry_pick dry_run"),
            loom_core::ReplayOutcome::Empty,
        );
        assert_eq!(
            mcp.write_vcs_revert("vcsws", &[], "tester", 3000, true)
                .expect("remote mcp vcs revert dry_run"),
            loom_core::ReplayOutcome::Empty,
        );

        // Graph reads and indexed graph writes forward canonical CBOR unchanged.
        assert!(
            mcp.read_graph_get_node("graphws", "g", "n1")
                .expect("remote mcp graph get_node")
                .is_some()
        );
        assert!(
            mcp.read_graph_get_edge("graphws", "g", "e1")
                .expect("remote mcp graph get_edge")
                .is_some()
        );
        assert!(
            !mcp.read_graph_neighbors("graphws", "g", "n1")
                .expect("remote mcp graph neighbors")
                .is_empty()
        );
        mcp.read_graph_out_edges("graphws", "g", "n1")
            .expect("remote mcp graph out_edges");
        mcp.read_graph_in_edges("graphws", "g", "n2")
            .expect("remote mcp graph in_edges");
        assert!(
            !mcp.read_graph_reachable("graphws", "g", "n1", -1, None)
                .expect("remote mcp graph reachable")
                .is_empty()
        );
        assert!(
            mcp.read_graph_shortest_path("graphws", "g", "n1", "n2", None)
                .expect("remote mcp graph shortest_path")
                .is_some()
        );
        mcp.read_graph_query("graphws", "g", "MATCH (n) RETURN n")
            .expect("remote mcp graph query");
        mcp.write_graph_upsert_node("graphws", "g", "n3", &[])
            .expect("remote mcp graph upsert_node");
        assert!(
            mcp.write_graph_remove_edge("graphws", "g", "e1")
                .expect("remote mcp graph remove_edge")
        );

        // A tool that needs a local `Loom<FileStore>` handle is refused clearly over a remote store.
        let err = mcp
            .check_open()
            .expect_err("local-handle op rejected over remote");
        assert!(
            err.to_string().contains("not available against a remote"),
            "unexpected error: {err}"
        );

        server.shutdown();
        drop(server_rt);
    }

    /// Every server-promoted MCP tool runs on the hosted server beside the served store and returns the
    /// same result the local host produces for the same arguments; a host-runtime-local tool is refused
    /// rather than forwarded. Empty arguments make the write tools fail argument parsing before any
    /// mutation, so the served store and the local host observe identical state for every comparison.
    #[cfg(feature = "mcp")]
    #[test]
    fn promoted_mcp_tools_execute_server_side_with_local_parity() {
        use uldren_loom_mcp::RemoteMcpBackend;

        let store = temp_store("mcp-promoted");

        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = std::env::temp_dir();
        let cert_path = dir.join(format!("loomcli-mcp-promoted-{}.crt", std::process::id()));
        let key_path = dir.join(format!("loomcli-mcp-promoted-{}.key", std::process::id()));
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.signing_key.serialize_pem()).unwrap();
        let tls = loom_hosted_core::HostedTlsConfig::from_pem_files(
            &cert_path.to_string_lossy(),
            &key_path.to_string_lossy(),
        )
        .expect("server tls");
        let options = loom_hosted_core::remote::RemoteServeOptions::from_cli(
            "127.0.0.1:0".to_string(),
            "https://localhost/apps/loom".to_string(),
            None,
            vec![loom_hosted_core::remote::RemoteAuthMode::Interactive],
            vec![loom_hosted_core::remote::RemoteTlsTrust::System],
            60_000,
            1 << 20,
            None,
        );
        let server_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = server_rt
            .block_on(crate::serve_cmd::bind_remote_endpoint(
                &store,
                &options,
                tls.server_config(),
            ))
            .expect("bind remote endpoint");
        let addr = server.local_addr();

        let target = RemoteTarget {
            url: format!("https://127.0.0.1:{}/apps/loom", addr.port()),
            auth: None,
            tls: Some("insecure-dev".to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };

        let backend: std::sync::Arc<dyn RemoteMcpBackend> = std::sync::Arc::new(
            McpRemoteBackend::connect(&target).expect("mcp remote backend connect"),
        );
        let local =
            uldren_loom_mcp::LoomMcp::new(uldren_loom_mcp::StoreAccess::per_request(&store, None));

        fn norm(
            r: std::result::Result<Vec<u8>, loom_types::LoomError>,
        ) -> std::result::Result<Vec<u8>, (loom_types::Code, String)> {
            r.map_err(|e| (e.code, e.message))
        }

        for name in uldren_loom_mcp::tools::tool_surface()
            .iter()
            .filter(|tool| {
                matches!(
                    tool.remote_capability(),
                    uldren_loom_mcp::tools::RemoteCapability::ServerExecute
                )
            })
            .map(|tool| tool.name)
        {
            let over_wire = norm(backend.execute_tool(name, b"{}"));
            let in_process = norm(uldren_loom_mcp::server::execute_promoted_tool(
                &local, name, b"{}",
            ));
            assert_eq!(
                over_wire, in_process,
                "promoted tool `{name}` server-side parity"
            );
        }

        let host_local = norm(backend.execute_tool("chat_set_presence", b"{}"));
        assert!(
            host_local.is_err(),
            "host-runtime-local chat_set_presence must not execute server-side"
        );

        server.shutdown();
        drop(server_rt);
    }

    /// A [`ParityDriver`](loom_protocol_conformance::client_parity::ParityDriver) over a live
    /// `loom serve remote` endpoint. It drives the *generated* `LoomClient` surface (`Kv`, `Cas`, `Queue`,
    /// `Document`, `TimeSeries`, `VersionControl`, `Store`) on a connected [`RemoteLoomClient`], blocking on
    /// each async call exactly as the CLI facade's remote arm does. The operation sequence and assertions
    /// live in the shared runner (`run_client_parity_suite`); this type only supplies the transport, so the
    /// same suite that certifies the in-process `LocalClientDriver` also certifies the wire path.
    struct RemoteClientDriver {
        store: RemoteStore,
    }

    impl loom_protocol_conformance::client_parity::ParityDriver for RemoteClientDriver {
        fn store_version(&self) -> Result<String, String> {
            self.store.block(Store::version(&self.store.client))
        }

        fn kv_put(
            &self,
            ws: &str,
            collection: &str,
            key: &[u8],
            value: &[u8],
        ) -> Result<(), String> {
            self.store.block(Kv::put(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                collection.to_string(),
                key.to_vec(),
                value.to_vec(),
            ))
        }

        fn kv_get(
            &self,
            ws: &str,
            collection: &str,
            key: &[u8],
        ) -> Result<Option<Vec<u8>>, String> {
            self.store.block(Kv::get(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                collection.to_string(),
                key.to_vec(),
            ))
        }

        fn cas_put(&self, ws: &str, content: &[u8]) -> Result<String, String> {
            let digest = self.store.block(Cas::put(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                content.to_vec(),
            ))?;
            Ok(digest.0)
        }

        fn cas_get(&self, ws: &str, digest: &str) -> Result<Option<Vec<u8>>, String> {
            self.store.block(Cas::get(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                WireDigest(digest.to_string()),
            ))
        }

        fn queue_append(&self, ws: &str, stream: &str, entry: &[u8]) -> Result<u64, String> {
            self.store.block(Queue::append(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                stream.to_string(),
                entry.to_vec(),
            ))
        }

        fn queue_get(&self, ws: &str, stream: &str, seq: u64) -> Result<Option<Vec<u8>>, String> {
            self.store.block(Queue::get(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                stream.to_string(),
                seq,
            ))
        }

        fn document_put_binary_bytes(
            &self,
            ws: &str,
            collection: &str,
            id: &str,
            doc: &[u8],
        ) -> Result<(), String> {
            self.document_put_binary(ws, collection, id, doc)
                .map(|_| ())
        }

        fn document_get_binary_bytes(
            &self,
            ws: &str,
            collection: &str,
            id: &str,
        ) -> Result<Option<Vec<u8>>, String> {
            self.document_get_binary(ws, collection, id)
                .and_then(|value| {
                    value
                        .map(|bytes| {
                            loom_wire::document::binary_result_from_cbor(&bytes).map(|v| v.0)
                        })
                        .transpose()
                        .map_err(|e| e.to_string())
                })
        }

        fn document_query_json(
            &self,
            ws: &str,
            collection: &str,
            query_json: &[u8],
        ) -> Result<Vec<u8>, String> {
            // `Document::query_json` is a single unary generated call. The server dispatches it to the same
            // `<LocalLoomClient as Document>::query_json` the in-process driver runs, so the canonical-JSON
            // result (matching ids + per-item digests under the store algorithm + documents) is
            // byte-identical local vs remote. The host-assembled `document_query` composite is a separate
            // layer.
            self.store.block(Document::query_json(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                collection.to_string(),
                query_json.to_vec(),
            ))
        }

        fn document_put_text(
            &self,
            ws: &str,
            collection: &str,
            id: &str,
            text: &str,
        ) -> Result<String, String> {
            let bytes = self.store.block(Document::put_text(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                collection.to_string(),
                id.to_string(),
                text.to_string(),
                None,
            ))?;
            let (digest, _) =
                loom_wire::document::put_result_from_cbor(&bytes).map_err(|e| e.to_string())?;
            Ok(digest)
        }

        fn document_get_text(
            &self,
            ws: &str,
            collection: &str,
            id: &str,
        ) -> Result<Option<Vec<u8>>, String> {
            self.store.block(Document::get_text(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                collection.to_string(),
                id.to_string(),
            ))
        }

        fn document_put_binary(
            &self,
            ws: &str,
            collection: &str,
            id: &str,
            bytes: &[u8],
        ) -> Result<String, String> {
            let bytes = self.store.block(Document::put_binary(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                collection.to_string(),
                id.to_string(),
                bytes.to_vec(),
                None,
            ))?;
            let (digest, _) =
                loom_wire::document::put_result_from_cbor(&bytes).map_err(|e| e.to_string())?;
            Ok(digest)
        }

        fn document_get_binary(
            &self,
            ws: &str,
            collection: &str,
            id: &str,
        ) -> Result<Option<Vec<u8>>, String> {
            self.store.block(Document::get_binary(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                collection.to_string(),
                id.to_string(),
            ))
        }

        fn document_list_binary(&self, ws: &str, collection: &str) -> Result<Vec<u8>, String> {
            self.store.block(Document::list_binary(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                collection.to_string(),
            ))
        }

        fn calendar_create_collection(
            &self,
            ws: &str,
            principal: &str,
            collection: &str,
            meta: &[u8],
        ) -> Result<(), String> {
            self.store.block(Calendar::create_collection(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                principal.to_string(),
                collection.to_string(),
                meta.to_vec(),
            ))
        }

        fn calendar_put_ics(
            &self,
            ws: &str,
            principal: &str,
            collection: &str,
            ics: &str,
        ) -> Result<String, String> {
            let digest = self.store.block(Calendar::put_ics(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                principal.to_string(),
                collection.to_string(),
                ics.to_string(),
            ))?;
            Ok(digest.0)
        }

        fn contacts_create_book(
            &self,
            ws: &str,
            principal: &str,
            book: &str,
            meta: &[u8],
        ) -> Result<(), String> {
            self.store.block(Contacts::create_book(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                principal.to_string(),
                book.to_string(),
                meta.to_vec(),
            ))
        }

        fn contacts_put_vcard(
            &self,
            ws: &str,
            principal: &str,
            book: &str,
            vcard: &str,
        ) -> Result<String, String> {
            let digest = self.store.block(Contacts::put_vcard(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                principal.to_string(),
                book.to_string(),
                vcard.to_string(),
            ))?;
            Ok(digest.0)
        }

        fn metrics_put_descriptor(&self, ws: &str, descriptor: &[u8]) -> Result<(), String> {
            self.store.block(Metrics::put_descriptor(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                descriptor.to_vec(),
            ))
        }

        fn metrics_get_descriptor(&self, ws: &str, name: &str) -> Result<Option<Vec<u8>>, String> {
            self.store.block(Metrics::get_descriptor(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                name.to_string(),
            ))
        }

        fn metrics_put_observation(
            &self,
            ws: &str,
            descriptor_name: &str,
            observation: &[u8],
        ) -> Result<(), String> {
            self.store.block(Metrics::put_observation(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                descriptor_name.to_string(),
                observation.to_vec(),
            ))
        }

        #[allow(clippy::too_many_arguments)]
        fn metrics_query(
            &self,
            ws: &str,
            descriptor_name: &str,
            from_timestamp_ms: u64,
            to_timestamp_ms: u64,
            max_series: u32,
            max_groups: u32,
            max_samples: u32,
            max_output_bytes: u64,
            now_timestamp_ms: u64,
        ) -> Result<Vec<u8>, String> {
            self.store.block(Metrics::query(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                descriptor_name.to_string(),
                from_timestamp_ms,
                to_timestamp_ms,
                max_series,
                max_groups,
                max_samples,
                max_output_bytes,
                now_timestamp_ms,
            ))
        }

        fn logs_put_record(&self, ws: &str, record: &[u8]) -> Result<String, String> {
            self.store.block(Logs::put_record(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                record.to_vec(),
            ))
        }

        fn logs_get_record(&self, ws: &str, record_id: &str) -> Result<Option<Vec<u8>>, String> {
            self.store.block(Logs::get_record(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                record_id.to_string(),
            ))
        }

        fn logs_query(
            &self,
            ws: &str,
            from_time_unix_nano: u64,
            to_time_unix_nano: u64,
            max_records: u32,
            max_output_bytes: u64,
        ) -> Result<Vec<u8>, String> {
            self.store.block(Logs::query(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                from_time_unix_nano,
                to_time_unix_nano,
                max_records,
                max_output_bytes,
            ))
        }

        fn traces_put_span(&self, ws: &str, span: &[u8]) -> Result<(), String> {
            self.store.block(Traces::put_span(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                span.to_vec(),
            ))
        }

        fn traces_get_span(
            &self,
            ws: &str,
            trace_id: &str,
            span_id: &str,
        ) -> Result<Option<Vec<u8>>, String> {
            self.store.block(Traces::get_span(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                trace_id.to_string(),
                span_id.to_string(),
            ))
        }

        fn traces_trace_spans(
            &self,
            ws: &str,
            trace_id: &str,
            max_spans: u32,
            max_output_bytes: u64,
        ) -> Result<Vec<u8>, String> {
            self.store.block(Traces::trace_spans(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                trace_id.to_string(),
                max_spans,
                max_output_bytes,
            ))
        }

        fn traces_query(
            &self,
            ws: &str,
            from_start_time_ns: u64,
            to_start_time_ns: u64,
            max_spans: u32,
            max_output_bytes: u64,
        ) -> Result<Vec<u8>, String> {
            self.store.block(Traces::query(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                from_start_time_ns,
                to_start_time_ns,
                max_spans,
                max_output_bytes,
            ))
        }

        fn search_create(&self, ws: &str, name: &str, mapping: &[u8]) -> Result<(), String> {
            self.store.block(Search::create(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                name.to_string(),
                mapping.to_vec(),
            ))
        }

        fn search_index(&self, ws: &str, name: &str, id: &[u8], doc: &[u8]) -> Result<(), String> {
            self.store.block(Search::index(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                name.to_string(),
                id.to_vec(),
                doc.to_vec(),
            ))
        }

        fn search_source_digest(&self, ws: &str, name: &str) -> Result<String, String> {
            let digest = self.store.block(Search::source_digest(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                name.to_string(),
            ))?;
            Ok(digest.0)
        }

        fn search_status(
            &self,
            ws: &str,
            name: &str,
            engine_version: &str,
        ) -> Result<Vec<u8>, String> {
            self.store.block(Search::status(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                name.to_string(),
                engine_version.to_string(),
            ))
        }

        fn ts_put(&self, ws: &str, collection: &str, ts: i64, value: &[u8]) -> Result<(), String> {
            self.store.block(TimeSeries::put(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                collection.to_string(),
                ts,
                value.to_vec(),
            ))
        }

        fn ts_latest(&self, ws: &str, collection: &str) -> Result<Option<(i64, Vec<u8>)>, String> {
            // `TimeSeries::latest` returns the raw CBOR point (`[ts, value]`) over the wire; decode it with
            // `latest_point_from_cbor` so the observable `(ts, value)` matches the in-process
            // `LocalClientDriver`'s decoded pair byte-for-byte.
            let raw = self.store.block(TimeSeries::latest(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                collection.to_string(),
            ))?;
            match raw {
                None => Ok(None),
                Some(bytes) => loom_core::timeseries::latest_point_from_cbor(&bytes)
                    .map(Some)
                    .map_err(|e| e.to_string()),
            }
        }

        fn vcs_commit(
            &self,
            ws: &str,
            author: &str,
            message: &str,
            timestamp_ms: u64,
        ) -> Result<String, String> {
            let digest = self.store.block(VersionControl::commit(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                author.to_string(),
                message.to_string(),
                timestamp_ms,
            ))?;
            Ok(digest.0)
        }

        fn vcs_head_branch(&self, ws: &str) -> Result<String, String> {
            self.store.block(VersionControl::head_branch(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
            ))
        }

        fn sql_seed_and_query(
            &self,
            ws: &str,
            db: &str,
            setup: &[&str],
            commit_ts_ms: u64,
            query: &str,
        ) -> Result<Vec<u8>, String> {
            // Seed a committed table over the wire through the generated SQL session lifecycle (`sql_open`
            // -> `sql_exec`* -> `sql_commit` -> `sql_close`), then read it back with the read-only unary
            // `sql_query_result` (store handle, not the SQL session). The server dispatches each to the same
            // `<LocalLoomClient as Sql>::*`, so the committed table and the SELECT result are identical to
            // the in-process driver. Commit identity is the shared parity constant.
            let sql_session = self.store.block(Sql::sql_open(
                &self.store.client,
                ws.to_string(),
                db.to_string(),
            ))?;
            for stmt in setup {
                self.store.block(Sql::sql_exec(
                    &self.store.client,
                    sql_session.clone(),
                    stmt.to_string(),
                ))?;
            }
            self.store.block(Sql::sql_commit(
                &self.store.client,
                sql_session.clone(),
                loom_protocol_conformance::client_parity::SQL_COMMIT_MESSAGE.to_string(),
                loom_protocol_conformance::client_parity::SQL_COMMIT_AUTHOR.to_string(),
                commit_ts_ms,
            ))?;
            self.store
                .block(Sql::sql_close(&self.store.client, sql_session))?;
            self.store.block(Sql::sql_query_result(
                &self.store.client,
                self.store.handle.clone(),
                ws.to_string(),
                db.to_string(),
                query.to_string(),
            ))
        }
    }

    /// Local-vs-remote client parity: the same shared `run_client_parity_suite` drives an in-process
    /// `LocalClientDriver` and a `RemoteClientDriver` over a live `loom serve remote` endpoint, and the two
    /// `ParityReport`s are byte-for-byte identical. Because the suite is deterministic (fixed
    /// workspace/collection names, fixed timestamps for content-addressed digests) the two fresh stores
    /// converge on the same observable outputs, so any divergence between the local engine and the wire path
    /// is caught here rather than at each call site.
    #[test]
    fn client_parity_local_matches_remote() {
        use loom_protocol_conformance::client_parity::{
            LocalClientDriver, run_client_parity_suite,
        };

        // The server binds this fresh store; the remote driver writes/reads through it over the wire.
        let store = temp_store("parity-remote");

        // A self-signed localhost cert loaded through the same TLS path `loom serve remote` uses.
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = std::env::temp_dir();
        let cert_path = dir.join(format!("loomcli-parity-{}.crt", std::process::id()));
        let key_path = dir.join(format!("loomcli-parity-{}.key", std::process::id()));
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.signing_key.serialize_pem()).unwrap();
        let tls = loom_hosted_core::HostedTlsConfig::from_pem_files(
            &cert_path.to_string_lossy(),
            &key_path.to_string_lossy(),
        )
        .expect("server tls");

        let options = loom_hosted_core::remote::RemoteServeOptions::from_cli(
            "127.0.0.1:0".to_string(),
            "https://localhost/apps/loom".to_string(),
            None,
            vec![loom_hosted_core::remote::RemoteAuthMode::Interactive],
            vec![loom_hosted_core::remote::RemoteTlsTrust::System],
            60_000,
            1 << 20,
            None,
        );

        let server_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = server_rt
            .block_on(crate::serve_cmd::bind_remote_endpoint(
                &store,
                &options,
                tls.server_config(),
            ))
            .expect("bind remote endpoint");
        let addr = server.local_addr();

        let target = RemoteTarget {
            url: format!("https://127.0.0.1:{}/apps/loom", addr.port()),
            auth: None,
            tls: Some("insecure-dev".to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };
        let remote = RemoteClientDriver {
            store: RemoteStore::connect(&target).expect("connect"),
        };

        // A separate fresh store for the in-process driver. `LocalClientDriver::create` creates the file.
        let local_path = dir.join(format!("loomcli-parity-local-{}.loom", std::process::id()));
        let _ = std::fs::remove_file(&local_path);
        let local = LocalClientDriver::create(local_path.clone()).expect("local driver");

        let local_report = run_client_parity_suite(&local).expect("local parity suite");
        let remote_report = run_client_parity_suite(&remote).expect("remote parity suite");

        // Observable-output parity: same labels, same bytes, in the same order.
        assert_eq!(
            local_report.entries, remote_report.entries,
            "local and remote client reports diverged"
        );

        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(&local_path);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
    }

    #[cfg(feature = "remote-client")]
    fn generated_block<F, T>(runtime: &tokio::runtime::Runtime, future: F) -> Result<T, String>
    where
        F: std::future::Future<Output = Result<T, loom_types::LoomError>>,
    {
        runtime.block_on(future).map_err(|err| err.to_string())
    }

    #[cfg(feature = "remote-client")]
    fn generated_transfer_export_bytes<C>(
        runtime: &tokio::runtime::Runtime,
        client: &C,
        handle: LoomSession,
        workspace: &str,
        kind: &str,
    ) -> Result<Vec<u8>, String>
    where
        C: Transfer,
    {
        runtime
            .block_on(async {
                use futures::StreamExt;
                let mut stream = Transfer::transfer_export(
                    client,
                    handle,
                    workspace.to_string(),
                    kind.to_string(),
                    None,
                    Vec::new(),
                )
                .await?;
                let mut out = Vec::new();
                while let Some(chunk) = stream.next().await {
                    out.extend(chunk?);
                }
                Ok::<Vec<u8>, loom_types::LoomError>(out)
            })
            .map_err(|err| err.to_string())
    }

    #[cfg(feature = "remote-client")]
    fn generated_transfer_import<C>(
        runtime: &tokio::runtime::Runtime,
        client: &C,
        handle: LoomSession,
        workspace: &str,
        kind: &str,
        payload: &[u8],
        final_digest: WireDigest,
    ) -> Result<Vec<u8>, String>
    where
        C: Transfer,
    {
        let transfer = generated_block(
            runtime,
            Transfer::transfer_import_open(
                client,
                handle.clone(),
                workspace.to_string(),
                kind.to_string(),
                Vec::new(),
            ),
        )?;
        generated_block(
            runtime,
            Transfer::transfer_import_write(
                client,
                handle.clone(),
                transfer.clone(),
                payload.to_vec(),
                0,
                None,
            ),
        )?;
        generated_block(
            runtime,
            Transfer::transfer_import_finish(client, handle, transfer, true, false, final_digest),
        )
    }

    #[cfg(feature = "remote-client")]
    fn assert_stable_import_report_parity(kind: &str, local: &[u8], remote: &[u8]) {
        let local = generated_import_report_from_cbor(local).expect("decode local import report");
        let remote =
            generated_import_report_from_cbor(remote).expect("decode remote import report");
        assert!(local.commit.is_some(), "{kind}: local commit identity");
        assert!(remote.commit.is_some(), "{kind}: remote commit identity");
        assert_eq!(local.profile, remote.profile, "{kind}: profile");
        assert_eq!(
            local.source_scope, remote.source_scope,
            "{kind}: source scope"
        );
        assert_eq!(local.objects_added, remote.objects_added, "{kind}: objects");
        assert_eq!(local.bytes_in, remote.bytes_in, "{kind}: bytes in");
        assert_eq!(
            local.bytes_stored, remote.bytes_stored,
            "{kind}: bytes stored"
        );
        assert_eq!(local.rows_imported, remote.rows_imported, "{kind}: rows");
        assert_eq!(local.skipped, remote.skipped, "{kind}: skipped");
        assert_eq!(
            local.operations_planned, remote.operations_planned,
            "{kind}: planned"
        );
        assert_eq!(
            local.operations_applied, remote.operations_applied,
            "{kind}: applied"
        );
        assert_eq!(local.dry_run, remote.dry_run, "{kind}: dry run");
        assert_eq!(local.warnings, remote.warnings, "{kind}: warnings");
        assert_eq!(
            local.fidelity_issues, remote.fidelity_issues,
            "{kind}: fidelity issues"
        );
    }

    #[cfg(feature = "remote-client")]
    fn generated_file_directory_flow<C>(
        label: &str,
        runtime: &tokio::runtime::Runtime,
        client: &C,
        handle: LoomSession,
    ) where
        C: FileSystem,
    {
        generated_block(
            runtime,
            FileSystem::write_file(
                client,
                handle.clone(),
                "w".to_string(),
                "docs/readme.txt".to_string(),
                b"hello".to_vec(),
                0o100644,
            ),
        )
        .unwrap_or_else(|err| panic!("{label} write nested: {err}"));
        generated_block(
            runtime,
            FileSystem::write_file(
                client,
                handle.clone(),
                "w".to_string(),
                "top.txt".to_string(),
                b"top".to_vec(),
                0o100644,
            ),
        )
        .unwrap_or_else(|err| panic!("{label} write top: {err}"));
        assert_eq!(
            generated_block(
                runtime,
                FileSystem::read_file(
                    client,
                    handle.clone(),
                    "w".to_string(),
                    "docs/readme.txt".to_string(),
                ),
            )
            .expect("read"),
            b"hello",
            "{label} read"
        );
        let root_listing = generated_block(
            runtime,
            FileSystem::list_directory(client, handle.clone(), "w".to_string(), "".to_string()),
        )
        .expect("list root");
        let root_names: Vec<_> = loom_wire::fs::dir_listing_from_cbor(&root_listing)
            .expect("decode root listing")
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(
            root_names,
            vec!["docs".to_string(), "top.txt".to_string()],
            "{label} root listing"
        );
        let docs_listing = generated_block(
            runtime,
            FileSystem::list_directory(client, handle.clone(), "w".to_string(), "docs".to_string()),
        )
        .expect("list docs");
        let docs_names: Vec<_> = loom_wire::fs::dir_listing_from_cbor(&docs_listing)
            .expect("decode docs listing")
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(docs_names, vec!["readme.txt".to_string()], "{label} docs");
        assert!(
            generated_block(
                runtime,
                FileSystem::remove_directory(
                    client,
                    handle.clone(),
                    "w".to_string(),
                    "docs".to_string(),
                    false,
                ),
            )
            .is_err(),
            "{label}: non-recursive non-empty delete must fail"
        );
        generated_block(
            runtime,
            FileSystem::remove_directory(
                client,
                handle.clone(),
                "w".to_string(),
                "docs".to_string(),
                true,
            ),
        )
        .unwrap_or_else(|err| panic!("{label} recursive delete: {err}"));
        generated_block(
            runtime,
            FileSystem::remove_file(
                client,
                handle.clone(),
                "w".to_string(),
                "top.txt".to_string(),
            ),
        )
        .unwrap_or_else(|err| panic!("{label} file delete: {err}"));
        let empty = generated_block(
            runtime,
            FileSystem::list_directory(client, handle, "w".to_string(), "".to_string()),
        )
        .expect("list after delete");
        assert!(
            loom_wire::fs::dir_listing_from_cbor(&empty)
                .expect("decode empty listing")
                .is_empty(),
            "{label}: all entries removed"
        );
    }

    #[cfg(feature = "remote-client")]
    fn generated_seed_transfer_source<C>(
        label: &str,
        runtime: &tokio::runtime::Runtime,
        client: &C,
        handle: LoomSession,
        content: &[u8],
    ) where
        C: FileSystem + VersionControl,
    {
        generated_block(
            runtime,
            FileSystem::write_file(
                client,
                handle.clone(),
                "src".to_string(),
                "hello.txt".to_string(),
                content.to_vec(),
                0o100644,
            ),
        )
        .unwrap_or_else(|err| panic!("{label} seed: {err}"));
        generated_block(
            runtime,
            VersionControl::commit(
                client,
                handle,
                "src".to_string(),
                "MU-6j-b1d".to_string(),
                "seed transfer source".to_string(),
                0,
            ),
        )
        .unwrap_or_else(|err| panic!("{label} commit seed: {err}"));
    }

    #[cfg(feature = "remote-client")]
    fn generated_assert_imported_tar<C>(
        label: &str,
        runtime: &tokio::runtime::Runtime,
        client: &C,
        handle: LoomSession,
        content: &[u8],
    ) where
        C: FileSystem,
    {
        assert_eq!(
            generated_block(
                runtime,
                FileSystem::read_file(
                    client,
                    handle,
                    "dst_tar".to_string(),
                    "hello.txt".to_string(),
                ),
            )
            .expect("read imported tar"),
            content,
            "{label}: imported tar content"
        );
    }

    /// Restores the file-directory remote coverage through the generated `FileSystem` surface. The local
    /// and remote clients execute the same generated calls and must expose the same file, directory,
    /// non-recursive delete, recursive delete, and read behavior.
    #[cfg(feature = "remote-client")]
    #[test]
    fn files_dir_surface_local_and_remote_over_tls() {
        let remote_store = temp_store("files-remote");
        let local_store = temp_store("files-local");
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = std::env::temp_dir();
        let cert_path = dir.join(format!("loomcli-files-{}.crt", std::process::id()));
        let key_path = dir.join(format!("loomcli-files-{}.key", std::process::id()));
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.signing_key.serialize_pem()).unwrap();
        let tls = loom_hosted_core::HostedTlsConfig::from_pem_files(
            &cert_path.to_string_lossy(),
            &key_path.to_string_lossy(),
        )
        .expect("server tls");
        let options = loom_hosted_core::remote::RemoteServeOptions::from_cli(
            "127.0.0.1:0".to_string(),
            "https://localhost/apps/loom".to_string(),
            None,
            vec![loom_hosted_core::remote::RemoteAuthMode::Interactive],
            vec![loom_hosted_core::remote::RemoteTlsTrust::System],
            60_000,
            1 << 20,
            None,
        );
        let server_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = server_rt
            .block_on(crate::serve_cmd::bind_remote_endpoint(
                &remote_store,
                &options,
                tls.server_config(),
            ))
            .expect("bind remote endpoint");
        let target = RemoteTarget {
            url: format!("https://127.0.0.1:{}/apps/loom", server.local_addr().port()),
            auth: None,
            tls: Some("insecure-dev".to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };
        let remote = RemoteStore::connect(&target).expect("connect");
        let local_runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = loom_client::LocalLoomClient::new(&local_store);
        local.create().expect("create local store");
        let local_handle = local.open().expect("open local store");

        generated_block(
            &local_runtime,
            FileSystem::create_directory(
                &local,
                local_handle.clone(),
                "w".to_string(),
                "docs".to_string(),
                false,
            ),
        )
        .expect("local mkdir");
        generated_block(
            &remote.runtime,
            FileSystem::create_directory(
                &remote.client,
                remote.handle.clone(),
                "w".to_string(),
                "docs".to_string(),
                false,
            ),
        )
        .expect("remote mkdir");
        generated_file_directory_flow("local", &local_runtime, &local, local_handle.clone());
        generated_file_directory_flow(
            "remote",
            &remote.runtime,
            &remote.client,
            remote.handle.clone(),
        );

        server.shutdown();
        drop(server_rt);
        let _ = std::fs::remove_file(&remote_store);
        let _ = std::fs::remove_file(&local_store);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
    }

    /// Restores byte-transfer coverage through generated `Transfer` methods. The fixture drives local and
    /// remote import/export directly through `LocalLoomClient` and `RemoteLoomClient` and preserves the raw
    /// staging invariants for credit, idempotent write replay, finalize-once, digest rejection, unsupported
    /// kind rejection, and imported content.
    #[cfg(feature = "remote-client")]
    #[test]
    fn transfer_interchange_local_and_remote_parity_over_tls() {
        let remote_store = temp_store("transfer-remote");
        let local_store = temp_store("transfer-local");
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = std::env::temp_dir();
        let cert_path = dir.join(format!("loomcli-transfer-{}.crt", std::process::id()));
        let key_path = dir.join(format!("loomcli-transfer-{}.key", std::process::id()));
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.signing_key.serialize_pem()).unwrap();
        let tls = loom_hosted_core::HostedTlsConfig::from_pem_files(
            &cert_path.to_string_lossy(),
            &key_path.to_string_lossy(),
        )
        .expect("server tls");
        let options = loom_hosted_core::remote::RemoteServeOptions::from_cli(
            "127.0.0.1:0".to_string(),
            "https://localhost/apps/loom".to_string(),
            None,
            vec![loom_hosted_core::remote::RemoteAuthMode::Interactive],
            vec![loom_hosted_core::remote::RemoteTlsTrust::System],
            60_000,
            1 << 20,
            None,
        );
        let server_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = server_rt
            .block_on(crate::serve_cmd::bind_remote_endpoint(
                &remote_store,
                &options,
                tls.server_config(),
            ))
            .expect("bind remote endpoint");
        let target = RemoteTarget {
            url: format!("https://127.0.0.1:{}/apps/loom", server.local_addr().port()),
            auth: None,
            tls: Some("insecure-dev".to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };
        let remote = RemoteStore::connect(&target).expect("connect");
        let local_runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = loom_client::LocalLoomClient::new(&local_store);
        local.create().expect("create local store");
        let local_handle = local.open().expect("open local store");
        let content = b"hello transfer parity payload".to_vec();
        generated_seed_transfer_source(
            "local",
            &local_runtime,
            &local,
            local_handle.clone(),
            &content,
        );
        generated_seed_transfer_source(
            "remote",
            &remote.runtime,
            &remote.client,
            remote.handle.clone(),
            &content,
        );

        let algo = loom_core::Algo::from_name(
            &generated_block(&local_runtime, Store::digest_algo(&local)).expect("digest algo"),
        )
        .expect("algo");
        for kind in ["tar", "tar-zstd", "tar-gzip", "zip"] {
            let local_payload = generated_transfer_export_bytes(
                &local_runtime,
                &local,
                local_handle.clone(),
                "src",
                kind,
            )
            .unwrap_or_else(|err| panic!("local export {kind}: {err}"));
            let remote_payload = generated_transfer_export_bytes(
                &remote.runtime,
                &remote.client,
                remote.handle.clone(),
                "src",
                kind,
            )
            .unwrap_or_else(|err| panic!("remote export {kind}: {err}"));
            assert_eq!(local_payload, remote_payload, "{kind}: export parity");
            let digest = WireDigest(loom_core::Digest::hash(algo, &local_payload).to_string());
            let local_report = generated_transfer_import(
                &local_runtime,
                &local,
                local_handle.clone(),
                &format!("dst_{}", kind.replace('-', "_")),
                kind,
                &local_payload,
                digest.clone(),
            )
            .unwrap_or_else(|err| panic!("local import {kind}: {err}"));
            let remote_report = generated_transfer_import(
                &remote.runtime,
                &remote.client,
                remote.handle.clone(),
                &format!("dst_{}", kind.replace('-', "_")),
                kind,
                &remote_payload,
                digest,
            )
            .unwrap_or_else(|err| panic!("remote import {kind}: {err}"));
            assert_stable_import_report_parity(kind, &local_report, &remote_report);
        }

        assert!(
            generated_block(
                &local_runtime,
                Transfer::transfer_import_open(
                    &local,
                    local_handle.clone(),
                    "w".to_string(),
                    "parquet".to_string(),
                    Vec::new(),
                ),
            )
            .is_err(),
            "local: unsupported kind must be rejected"
        );
        assert!(
            generated_block(
                &remote.runtime,
                Transfer::transfer_import_open(
                    &remote.client,
                    remote.handle.clone(),
                    "w".to_string(),
                    "parquet".to_string(),
                    Vec::new(),
                ),
            )
            .is_err(),
            "remote: unsupported kind must be rejected"
        );
        generated_assert_imported_tar(
            "local",
            &local_runtime,
            &local,
            local_handle.clone(),
            &content,
        );
        generated_assert_imported_tar(
            "remote",
            &remote.runtime,
            &remote.client,
            remote.handle.clone(),
            &content,
        );

        let payload = generated_transfer_export_bytes(
            &remote.runtime,
            &remote.client,
            remote.handle.clone(),
            "src",
            "tar",
        )
        .expect("raw export");
        let good_digest = WireDigest(loom_core::Digest::hash(algo, &payload).to_string());
        let bad_digest = WireDigest(loom_core::Digest::hash(algo, b"tampered").to_string());
        let transfer = generated_block(
            &remote.runtime,
            Transfer::transfer_import_open(
                &remote.client,
                remote.handle.clone(),
                "rawdst".to_string(),
                "tar".to_string(),
                Vec::new(),
            ),
        )
        .expect("raw open");
        let accept0 = generated_block(
            &remote.runtime,
            Transfer::transfer_import_write(
                &remote.client,
                remote.handle.clone(),
                transfer.clone(),
                payload.clone(),
                0,
                None,
            ),
        )
        .expect("raw write");
        let (accepted0, credit0) =
            loom_wire::transfer::transfer_accept_from_cbor(&accept0).expect("decode accept");
        assert_eq!(accepted0, payload.len() as u64);
        assert_eq!(
            accepted0 + credit0,
            loom_interchange_io::transfer::StagingLimits::DEFAULT_MAX_TOTAL_BYTES
        );
        let replay = generated_block(
            &remote.runtime,
            Transfer::transfer_import_write(
                &remote.client,
                remote.handle.clone(),
                transfer.clone(),
                payload.clone(),
                0,
                None,
            ),
        )
        .expect("raw replay");
        assert_eq!(
            loom_wire::transfer::transfer_accept_from_cbor(&replay).expect("decode replay"),
            (accepted0, credit0)
        );
        let report1 = generated_block(
            &remote.runtime,
            Transfer::transfer_import_finish(
                &remote.client,
                remote.handle.clone(),
                transfer.clone(),
                true,
                false,
                good_digest.clone(),
            ),
        )
        .expect("raw finish");
        let report2 = generated_block(
            &remote.runtime,
            Transfer::transfer_import_finish(
                &remote.client,
                remote.handle.clone(),
                transfer,
                true,
                false,
                good_digest,
            ),
        )
        .expect("raw finish replay");
        assert_eq!(report1, report2, "finish is finalize-once");
        let bad_transfer = generated_block(
            &remote.runtime,
            Transfer::transfer_import_open(
                &remote.client,
                remote.handle.clone(),
                "rawbad".to_string(),
                "tar".to_string(),
                Vec::new(),
            ),
        )
        .expect("bad open");
        generated_block(
            &remote.runtime,
            Transfer::transfer_import_write(
                &remote.client,
                remote.handle.clone(),
                bad_transfer.clone(),
                payload,
                0,
                None,
            ),
        )
        .expect("bad write");
        assert!(
            generated_block(
                &remote.runtime,
                Transfer::transfer_import_finish(
                    &remote.client,
                    remote.handle.clone(),
                    bad_transfer,
                    true,
                    false,
                    bad_digest,
                ),
            )
            .is_err(),
            "mismatched final digest must be rejected"
        );

        server.shutdown();
        drop(server_rt);
        let _ = std::fs::remove_file(&remote_store);
        let _ = std::fs::remove_file(&local_store);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
    }

    /// Two generated remote sessions connected to one TLS endpoint share committed state without relying
    /// on the deleted local/remote `StoreClient` mutation adapter split.
    #[cfg(feature = "remote-client")]
    #[test]
    fn multi_connection_over_tls_sees_committed_writes() {
        let store = temp_store("multi-conn");
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = std::env::temp_dir();
        let cert_path = dir.join(format!("loomcli-mc-{}.crt", std::process::id()));
        let key_path = dir.join(format!("loomcli-mc-{}.key", std::process::id()));
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.signing_key.serialize_pem()).unwrap();
        let tls = loom_hosted_core::HostedTlsConfig::from_pem_files(
            &cert_path.to_string_lossy(),
            &key_path.to_string_lossy(),
        )
        .expect("server tls");
        let options = loom_hosted_core::remote::RemoteServeOptions::from_cli(
            "127.0.0.1:0".to_string(),
            "https://localhost/apps/loom".to_string(),
            None,
            vec![loom_hosted_core::remote::RemoteAuthMode::Interactive],
            vec![loom_hosted_core::remote::RemoteTlsTrust::System],
            60_000,
            1 << 20,
            None,
        );
        let server_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = server_rt
            .block_on(crate::serve_cmd::bind_remote_endpoint(
                &store,
                &options,
                tls.server_config(),
            ))
            .expect("bind remote endpoint");
        let target = || RemoteTarget {
            url: format!("https://127.0.0.1:{}/apps/loom", server.local_addr().port()),
            auth: None,
            tls: Some("insecure-dev".to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };
        let conn_a = RemoteStore::connect(&target()).expect("connect A");
        let conn_b = RemoteStore::connect(&target()).expect("connect B");
        let key = loom_core::kv::key_to_cbor(&loom_core::Value::Text("k".to_string()));
        let key2 = loom_core::kv::key_to_cbor(&loom_core::Value::Text("k2".to_string()));

        let malformed = generated_block(
            &conn_a.runtime,
            Kv::put(
                &conn_a.client,
                conn_a.handle.clone(),
                "w".to_string(),
                "malformed".to_string(),
                b"k".to_vec(),
                b"raw".to_vec(),
            ),
        )
        .expect_err("raw key is rejected");
        assert!(malformed.contains("CORRUPT_OBJECT"));
        assert!(malformed.contains("unexpected end of input"));
        assert_eq!(
            generated_block(
                &conn_b.runtime,
                Kv::get(
                    &conn_b.client,
                    conn_b.handle.clone(),
                    "w".to_string(),
                    "malformed".to_string(),
                    key.clone(),
                ),
            )
            .expect("malformed collection remains absent"),
            None
        );

        generated_block(
            &conn_a.runtime,
            Kv::put(
                &conn_a.client,
                conn_a.handle.clone(),
                "w".to_string(),
                "shared".to_string(),
                key.clone(),
                b"from-a".to_vec(),
            ),
        )
        .expect("A write");
        assert_eq!(
            generated_block(
                &conn_b.runtime,
                Kv::get(
                    &conn_b.client,
                    conn_b.handle.clone(),
                    "w".to_string(),
                    "shared".to_string(),
                    key,
                ),
            )
            .expect("B read"),
            Some(b"from-a".to_vec())
        );
        generated_block(
            &conn_b.runtime,
            Kv::put(
                &conn_b.client,
                conn_b.handle.clone(),
                "w".to_string(),
                "shared".to_string(),
                key2.clone(),
                b"from-b".to_vec(),
            ),
        )
        .expect("B write");
        assert_eq!(
            generated_block(
                &conn_a.runtime,
                Kv::get(
                    &conn_a.client,
                    conn_a.handle.clone(),
                    "w".to_string(),
                    "shared".to_string(),
                    key2,
                ),
            )
            .expect("A read"),
            Some(b"from-b".to_vec())
        );

        server.shutdown();
        drop(server_rt);
        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
    }

    #[cfg(feature = "remote-client")]
    fn seed_identity_store(store: &str, root: loom_core::WorkspaceId) {
        let keys = KeyOpts::default();
        let loom = cli_open_loom(store, &keys).expect("open store for identity seed");
        let mut identity = loom_core::identity::IdentityStore::new(root);
        identity
            .set_passphrase(root, "rootpw", b"root-salt-bytes")
            .expect("seed root passphrase");
        loom.store()
            .save_identity_store(&identity)
            .expect("save identity seed");
        let mut acl = loom_core::AclStore::new();
        acl.allow(
            loom_core::AclSubject::Principal(root),
            None,
            None,
            [loom_core::AclRight::Admin],
        )
        .expect("grant root global admin");
        loom.store().save_acl_store(&acl).expect("save acl seed");
    }

    /// Restores audited identity coverage through generated `Identity` methods and generated session
    /// authentication. The revoke calls return the same audit records over local and remote clients, and
    /// identity snapshots prove the returned ids refer to disabled records.
    #[cfg(feature = "remote-client")]
    #[test]
    fn identity_audited_commands_match_local_and_remote() {
        let root = loom_core::WorkspaceId::v4_from_bytes([7; 16]);
        let local_store = temp_store("id-audit-local");
        let remote_store = temp_store("id-audit-remote");
        seed_identity_store(&local_store, root);
        seed_identity_store(&remote_store, root);

        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = std::env::temp_dir();
        let cert_path = dir.join(format!("loomcli-idaudit-{}.crt", std::process::id()));
        let key_path = dir.join(format!("loomcli-idaudit-{}.key", std::process::id()));
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.signing_key.serialize_pem()).unwrap();
        let tls = loom_hosted_core::HostedTlsConfig::from_pem_files(
            &cert_path.to_string_lossy(),
            &key_path.to_string_lossy(),
        )
        .expect("server tls");
        let options = loom_hosted_core::remote::RemoteServeOptions::from_cli(
            "127.0.0.1:0".to_string(),
            "https://localhost/apps/loom".to_string(),
            None,
            vec![loom_hosted_core::remote::RemoteAuthMode::Interactive],
            vec![loom_hosted_core::remote::RemoteTlsTrust::System],
            60_000,
            1 << 20,
            None,
        );
        let server_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = server_rt
            .block_on(crate::serve_cmd::bind_remote_endpoint(
                &remote_store,
                &options,
                tls.server_config(),
            ))
            .expect("bind remote endpoint");
        let target = RemoteTarget {
            url: format!("https://127.0.0.1:{}/apps/loom", server.local_addr().port()),
            auth: None,
            tls: Some("insecure-dev".to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };
        assert!(
            RemoteStore::connect_with_auth(
                &target,
                SessionAuth::Passphrase {
                    principal: *root.as_bytes(),
                    passphrase: b"wrong".to_vec(),
                },
            )
            .is_err(),
            "bad passphrase must fail session open"
        );
        let remote = RemoteStore::connect_with_auth(
            &target,
            SessionAuth::Passphrase {
                principal: *root.as_bytes(),
                passphrase: b"rootpw".to_vec(),
            },
        )
        .expect("authenticated connect");
        let local_runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = loom_client::LocalLoomClient::new(&local_store);
        let local_handle = local.open().expect("open local");
        generated_block(
            &local_runtime,
            Sessions::authenticate_passphrase(
                &local,
                local_handle.clone(),
                Uuid(*root.as_bytes()),
                b"rootpw".to_vec(),
            ),
        )
        .expect("authenticate local");

        let ext_spec = loom_core::ExternalCredentialSpec {
            id: loom_core::WorkspaceId::from_bytes([0; 16]),
            kind: loom_core::ExternalCredentialKind::OidcSubject,
            label: "ci".to_string(),
            issuer: "https://issuer".to_string(),
            subject: "svc-bot".to_string(),
            material_digest: None,
        };
        let ext_wire = loom_wire::identity::external_credential_spec_to_wire(&ext_spec)
            .expect("external credential wire");
        let local_ext = generated_block(
            &local_runtime,
            Identity::identity_create_external_credential(
                &local,
                local_handle.clone(),
                Uuid(*root.as_bytes()),
                ext_wire.clone(),
            ),
        )
        .expect("local external create");
        let remote_ext = generated_block(
            &remote.runtime,
            Identity::identity_create_external_credential(
                &remote.client,
                remote.handle.clone(),
                Uuid(*root.as_bytes()),
                ext_wire,
            ),
        )
        .expect("remote external create");
        let local_ext_result =
            loom_wire::identity::identity_audit_result_from_cbor(&local_ext).expect("local ext");
        let remote_ext_result =
            loom_wire::identity::identity_audit_result_from_cbor(&remote_ext).expect("remote ext");
        assert_eq!(local_ext_result.audit_seq, remote_ext_result.audit_seq);
        assert_eq!(local_ext_result.action, remote_ext_result.action);
        assert!(local_ext_result.id.is_some());
        assert!(remote_ext_result.id.is_some());

        let local_key = generated_block(
            &local_runtime,
            Identity::identity_add_public_key(
                &local,
                local_handle.clone(),
                Uuid(*root.as_bytes()),
                "ci-key".to_string(),
                "Ed25519".to_string(),
                vec![9u8; 32],
            ),
        )
        .expect("local key create");
        let remote_key = generated_block(
            &remote.runtime,
            Identity::identity_add_public_key(
                &remote.client,
                remote.handle.clone(),
                Uuid(*root.as_bytes()),
                "ci-key".to_string(),
                "Ed25519".to_string(),
                vec![9u8; 32],
            ),
        )
        .expect("remote key create");
        let local_key_result =
            loom_wire::identity::identity_audit_result_from_cbor(&local_key).expect("local key");
        let remote_key_result =
            loom_wire::identity::identity_audit_result_from_cbor(&remote_key).expect("remote key");
        assert_eq!(local_key_result.audit_seq, remote_key_result.audit_seq);
        assert_eq!(local_key_result.action, remote_key_result.action);
        let local_key_id = local_key_result.id.expect("local key id");
        let remote_key_id = remote_key_result.id.expect("remote key id");

        let local_revoke = loom_wire::identity::identity_audit_result_from_cbor(
            &generated_block(
                &local_runtime,
                Identity::identity_revoke_public_key(
                    &local,
                    local_handle.clone(),
                    Uuid(*local_key_id.as_bytes()),
                ),
            )
            .expect("local key revoke"),
        )
        .expect("decode local revoke");
        let remote_revoke = loom_wire::identity::identity_audit_result_from_cbor(
            &generated_block(
                &remote.runtime,
                Identity::identity_revoke_public_key(
                    &remote.client,
                    remote.handle.clone(),
                    Uuid(*remote_key_id.as_bytes()),
                ),
            )
            .expect("remote key revoke"),
        )
        .expect("decode remote revoke");
        assert_eq!(local_revoke.audit_seq, remote_revoke.audit_seq);
        assert_eq!(local_revoke.action, remote_revoke.action);
        let local_snapshot = loom_wire::identity::identity_snapshot_from_cbor(
            &generated_block(
                &local_runtime,
                Identity::identity_list(&local, local_handle.clone()),
            )
            .expect("local list"),
        )
        .expect("decode local snapshot");
        let remote_snapshot = loom_wire::identity::identity_snapshot_from_cbor(
            &generated_block(
                &remote.runtime,
                Identity::identity_list(&remote.client, remote.handle.clone()),
            )
            .expect("remote list"),
        )
        .expect("decode remote snapshot");
        assert!(
            !local_snapshot
                .public_keys
                .iter()
                .any(|key| key.id == local_key_id),
            "local revoked key must be absent"
        );
        assert!(
            !remote_snapshot
                .public_keys
                .iter()
                .any(|key| key.id == remote_key_id),
            "remote revoked key must be absent"
        );

        server.shutdown();
        drop(server_rt);
        let _ = std::fs::remove_file(&local_store);
        let _ = std::fs::remove_file(&remote_store);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
    }

    /// Restores app-credential coverage through generated `Identity` methods. Create returns the secret
    /// once, list remains secret-free, and revoke returns audit records over both generated client paths.
    #[cfg(feature = "remote-client")]
    #[test]
    fn app_credential_commands_match_local_and_remote() {
        let root = loom_core::WorkspaceId::v4_from_bytes([7; 16]);
        let local_store = temp_store("appcred-local");
        let remote_store = temp_store("appcred-remote");
        seed_identity_store(&local_store, root);
        seed_identity_store(&remote_store, root);

        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = std::env::temp_dir();
        let cert_path = dir.join(format!("loomcli-appcred-{}.crt", std::process::id()));
        let key_path = dir.join(format!("loomcli-appcred-{}.key", std::process::id()));
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.signing_key.serialize_pem()).unwrap();
        let tls = loom_hosted_core::HostedTlsConfig::from_pem_files(
            &cert_path.to_string_lossy(),
            &key_path.to_string_lossy(),
        )
        .expect("server tls");
        let options = loom_hosted_core::remote::RemoteServeOptions::from_cli(
            "127.0.0.1:0".to_string(),
            "https://localhost/apps/loom".to_string(),
            None,
            vec![loom_hosted_core::remote::RemoteAuthMode::Interactive],
            vec![loom_hosted_core::remote::RemoteTlsTrust::System],
            60_000,
            1 << 20,
            None,
        );
        let server_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = server_rt
            .block_on(crate::serve_cmd::bind_remote_endpoint(
                &remote_store,
                &options,
                tls.server_config(),
            ))
            .expect("bind remote endpoint");
        let target = RemoteTarget {
            url: format!("https://127.0.0.1:{}/apps/loom", server.local_addr().port()),
            auth: None,
            tls: Some("insecure-dev".to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };
        assert!(
            RemoteStore::connect_with_auth(
                &target,
                SessionAuth::Passphrase {
                    principal: *root.as_bytes(),
                    passphrase: b"wrong".to_vec(),
                },
            )
            .is_err(),
            "bad passphrase must fail session open"
        );
        let remote = RemoteStore::connect_with_auth(
            &target,
            SessionAuth::Passphrase {
                principal: *root.as_bytes(),
                passphrase: b"rootpw".to_vec(),
            },
        )
        .expect("authenticated connect");
        let local_runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = loom_client::LocalLoomClient::new(&local_store);
        let local_handle = local.open().expect("open local");
        generated_block(
            &local_runtime,
            Sessions::authenticate_passphrase(
                &local,
                local_handle.clone(),
                Uuid(*root.as_bytes()),
                b"rootpw".to_vec(),
            ),
        )
        .expect("authenticate local");

        let local_created = loom_wire::identity::app_credential_create_result_from_cbor(
            &generated_block(
                &local_runtime,
                Identity::identity_create_app_credential(
                    &local,
                    local_handle.clone(),
                    Uuid(*root.as_bytes()),
                    "ci-runner".to_string(),
                ),
            )
            .expect("local app credential create"),
        )
        .expect("decode local app create");
        let remote_created = loom_wire::identity::app_credential_create_result_from_cbor(
            &generated_block(
                &remote.runtime,
                Identity::identity_create_app_credential(
                    &remote.client,
                    remote.handle.clone(),
                    Uuid(*root.as_bytes()),
                    "ci-runner".to_string(),
                ),
            )
            .expect("remote app credential create"),
        )
        .expect("decode remote app create");
        assert!(local_created.secret_token.starts_with("loom_app_"));
        assert!(remote_created.secret_token.starts_with("loom_app_"));
        assert_eq!(local_created.audit_seq, remote_created.audit_seq);
        assert_eq!(local_created.label, remote_created.label);
        let local_list = generated_block(
            &local_runtime,
            Identity::identity_list(&local, local_handle.clone()),
        )
        .expect("local list");
        let remote_list = generated_block(
            &remote.runtime,
            Identity::identity_list(&remote.client, remote.handle.clone()),
        )
        .expect("remote list");
        assert!(!String::from_utf8_lossy(&local_list).contains(&local_created.secret_token));
        assert!(!String::from_utf8_lossy(&remote_list).contains(&remote_created.secret_token));
        let local_snapshot =
            loom_wire::identity::identity_snapshot_from_cbor(&local_list).expect("local snapshot");
        let remote_snapshot = loom_wire::identity::identity_snapshot_from_cbor(&remote_list)
            .expect("remote snapshot");
        assert!(local_snapshot.app_credentials.iter().any(|credential| {
            credential.id == local_created.id
                && credential.principal == root
                && credential.label == "ci-runner"
                && credential.enabled
        }));
        assert!(remote_snapshot.app_credentials.iter().any(|credential| {
            credential.id == remote_created.id
                && credential.principal == root
                && credential.label == "ci-runner"
                && credential.enabled
        }));
        let local_revoke = loom_wire::identity::identity_audit_result_from_cbor(
            &generated_block(
                &local_runtime,
                Identity::identity_revoke_app_credential(
                    &local,
                    local_handle.clone(),
                    Uuid(*local_created.id.as_bytes()),
                ),
            )
            .expect("local revoke"),
        )
        .expect("decode local revoke");
        let remote_revoke = loom_wire::identity::identity_audit_result_from_cbor(
            &generated_block(
                &remote.runtime,
                Identity::identity_revoke_app_credential(
                    &remote.client,
                    remote.handle.clone(),
                    Uuid(*remote_created.id.as_bytes()),
                ),
            )
            .expect("remote revoke"),
        )
        .expect("decode remote revoke");
        assert_eq!(local_revoke.action, remote_revoke.action);
        assert_eq!(local_revoke.audit_seq, remote_revoke.audit_seq);

        server.shutdown();
        drop(server_rt);
        let _ = std::fs::remove_file(&local_store);
        let _ = std::fs::remove_file(&remote_store);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
    }

    /// Verifies TLS trust modes against a self-signed loopback endpoint: `insecure-dev` accepts it, a CA
    /// bundle without the server certificate rejects it, and system-root trust rejects it.
    #[test]
    fn files_tls_trust_accepts_dev_and_rejects_untrusted_bundle() {
        let store = temp_store("files-tls-trust");

        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = std::env::temp_dir();
        let cert_path = dir.join(format!("loomcli-tls-{}.crt", std::process::id()));
        let key_path = dir.join(format!("loomcli-tls-{}.key", std::process::id()));
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.signing_key.serialize_pem()).unwrap();
        let tls = loom_hosted_core::HostedTlsConfig::from_pem_files(
            &cert_path.to_string_lossy(),
            &key_path.to_string_lossy(),
        )
        .expect("server tls");

        let options = loom_hosted_core::remote::RemoteServeOptions::from_cli(
            "127.0.0.1:0".to_string(),
            "https://localhost/apps/loom".to_string(),
            None,
            vec![loom_hosted_core::remote::RemoteAuthMode::Interactive],
            vec![loom_hosted_core::remote::RemoteTlsTrust::System],
            60_000,
            1 << 20,
            None,
        );

        let server_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = server_rt
            .block_on(crate::serve_cmd::bind_remote_endpoint(
                &store,
                &options,
                tls.server_config(),
            ))
            .expect("bind remote endpoint");
        let port = server.local_addr().port();
        let base = |trust: &str| RemoteTarget {
            url: format!("https://127.0.0.1:{port}/apps/loom"),
            auth: None,
            tls: Some(trust.to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };

        // insecure-dev accepts the self-signed loopback cert.
        assert!(
            RemoteStore::connect(&base("insecure-dev")).is_ok(),
            "insecure-dev should accept the self-signed loopback endpoint"
        );

        // A CA bundle that does not contain the server's certificate is a genuine trust anchor that
        // rejects the self-signed cert at the TLS handshake (a real certificate rejection).
        let other = rcgen::generate_simple_self_signed(vec!["unrelated".to_string()]).unwrap();
        let untrusted_bundle =
            dir.join(format!("loomcli-tls-untrusted-{}.pem", std::process::id()));
        std::fs::write(&untrusted_bundle, other.cert.pem()).unwrap();
        assert!(
            RemoteStore::connect(&base(&untrusted_bundle.to_string_lossy())).is_err(),
            "a CA bundle without the server cert must reject the endpoint (real TLS cert rejection)"
        );

        // Default/system-root trust verifies against the OS trust store, which does not contain this
        // self-signed loopback cert, so the endpoint is rejected. (If the platform trust store is empty,
        // `build_client_config` errors before connecting - either way `system` does not accept it.)
        assert!(
            RemoteStore::connect(&base("system")).is_err(),
            "system trust must reject the self-signed loopback endpoint"
        );

        drop(server);
        server_rt.shutdown_background();
        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
        let _ = std::fs::remove_file(&untrusted_bundle);
    }

    /// Task 640: StoreAdmin over a served self-signed-TLS endpoint. Proves the server-owned
    /// store-administration surface works over the wire under an authenticated global admin
    /// (`store_policy_set`/`get`/`store_stat`), and fails closed for an unauthenticated session. The
    /// local side of StoreAdmin is covered by loom-client unit tests; the server executes the same
    /// `LocalLoomClient` StoreAdmin impl, so this fixture is the remote/wire half of parity.
    #[test]
    fn store_admin_over_tls_requires_authenticated_global_admin() {
        use loom_core::identity::IdentityStore;
        use loom_core::{WorkspaceId, runtime_profile};

        let root = WorkspaceId::v4_from_bytes([7; 16]);
        let seed = |store: &str| {
            let keys = KeyOpts::default();
            let loom = cli_open_loom(store, &keys).expect("open store for seed");
            let mut identity = IdentityStore::new(root);
            identity
                .set_passphrase(root, "rootpw", b"root-salt-bytes")
                .expect("seed root passphrase");
            loom.store()
                .save_identity_store(&identity)
                .expect("save identity");
            let mut acl = loom_core::AclStore::new();
            acl.allow(
                loom_core::AclSubject::Principal(root),
                None,
                None,
                [loom_core::AclRight::Admin],
            )
            .expect("grant root global admin");
            loom.store().save_acl_store(&acl).expect("save acl");
        };
        let remote_store = temp_store("storeadmin-remote");
        seed(&remote_store);

        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = std::env::temp_dir();
        let cert_path = dir.join(format!("loomcli-sa-{}.crt", std::process::id()));
        let key_path = dir.join(format!("loomcli-sa-{}.key", std::process::id()));
        std::fs::write(&cert_path, cert.cert.pem()).unwrap();
        std::fs::write(&key_path, cert.signing_key.serialize_pem()).unwrap();
        let tls = loom_hosted_core::HostedTlsConfig::from_pem_files(
            &cert_path.to_string_lossy(),
            &key_path.to_string_lossy(),
        )
        .expect("server tls");
        let options = loom_hosted_core::remote::RemoteServeOptions::from_cli(
            "127.0.0.1:0".to_string(),
            "https://localhost/apps/loom".to_string(),
            None,
            vec![loom_hosted_core::remote::RemoteAuthMode::Interactive],
            vec![loom_hosted_core::remote::RemoteTlsTrust::System],
            60_000,
            1 << 20,
            None,
        );
        let server_rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = server_rt
            .block_on(crate::serve_cmd::bind_remote_endpoint(
                &remote_store,
                &options,
                tls.server_config(),
            ))
            .expect("bind remote endpoint");
        let addr = server.local_addr();
        let target = RemoteTarget {
            url: format!("https://127.0.0.1:{}/apps/loom", addr.port()),
            auth: None,
            tls: Some("insecure-dev".to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };

        // Unauthenticated session: StoreAdmin fails closed (authenticated global admin required).
        let anon = RemoteStore::connect(&target).expect("anon connect");
        assert!(
            anon.block(StoreAdmin::store_policy_get(
                &anon.client,
                anon.handle.clone()
            ))
            .is_err(),
            "unauthenticated StoreAdmin must fail closed over the wire"
        );

        // Authenticated global admin: stat and policy set succeed over the wire.
        let admin = RemoteStore::connect_with_auth(
            &target,
            SessionAuth::Passphrase {
                principal: *root.as_bytes(),
                passphrase: b"rootpw".to_vec(),
            },
        )
        .expect("authenticated connect");
        let _stat = admin
            .block(StoreAdmin::store_stat(&admin.client, admin.handle.clone()))
            .expect("remote stat as admin");
        let set = admin
            .block(StoreAdmin::store_policy_set(
                &admin.client,
                admin.handle.clone(),
                loom_wire::store_admin::store_policy_update_to_cbor(
                    &loom_wire::store_admin::StorePolicyUpdate {
                        fips_required: Some(true),
                        default_durability: None,
                        facet_durability_assignments: Vec::new(),
                        clear_facet_durability: Vec::new(),
                    },
                ),
            ))
            .and_then(|cbor| {
                loom_wire::store_admin::store_policy_result_from_cbor(&cbor)
                    .map_err(|error| error.to_string())
            })
            .expect("remote policy set");
        assert!(set.fips_required);
        let get = admin
            .block(StoreAdmin::store_policy_get(
                &admin.client,
                admin.handle.clone(),
            ))
            .and_then(|cbor| {
                loom_wire::store_admin::store_policy_result_from_cbor(&cbor)
                    .map_err(|error| error.to_string())
            });
        if runtime_profile().fips_capable {
            assert!(get.expect("remote policy get").fips_required);
        } else {
            assert!(
                get.expect_err("non-FIPS runtime must reject FIPS-required store")
                    .contains("FIPS-required stores cannot be opened")
            );
        }

        drop(server);
        server_rt.shutdown_background();
        let _ = std::fs::remove_file(&remote_store);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
    }
}
