//! `LocalLoomClient` implementations of the generated `LoomApi` service traits: a bridge from the
//! wire-typed trait methods onto the in-process client's inherent surface. Round-trip methods run the
//! inherent op synchronously and return a ready future; local methods run in place. Diagnostics and
//! `ResultViews` decode through the shared engine-free `loom_result` accessors; `Daemon` control is not
//! available to an in-process client and reports `Unsupported`.
//!
//! Licensed under BUSL-1.1.

use crate::local::{DocumentReplaceTextArgs, LaneUpdateInput, LocalLoomClient};
use loom_codec::Value;
use loom_core::digest::Digest as CoreDigest;
use loom_core::identity::IdentityPublicKeySpec;
use loom_core::keys::{KEY_LEN, KeySpec};
use loom_core::{ProtectedRefPolicy, WorkspaceId, WsSelector, watch_batch_to_cbor};
use loom_remote_protocol::api_types::{
    Digest, HandleId, LoomSession, LoomStream, ResultView, RowIter, SqlBatch, SqlSession, Task,
    Uuid,
};
use loom_remote_protocol::generated_api::{
    Acl, Archive, Calendar, Car, Cas, Chat, Columnar, Contacts, Daemon, Dataframe, Diagnostics,
    Document, Drive, Exec, FileHandle, FileSystem, Graph, Identity, KeySource, Kv, Lanes, Ledger,
    Locks, Logs, LoomClient, Mail, ManagementKv, Meetings, Metrics, Pages, Program, ProtectedRefs,
    Queue, QueueConsumers, ResultViews, Search, Sessions, Sql, Store, StoreAdmin, StudioSurfaces,
    Tasks, Tickets, TimeSeries, Traces, Transfer, Triggers, Vector, VersionControl, Watch,
    Workspaces,
};
use loom_result::result_view::{Reader, ResultPayload};
use loom_result::view;
use loom_store::save_loom;
use loom_types::tabular::cell_from;
use loom_types::{Code, LoomError, MutationChange, MutationEnvelope, MutationReceipt};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value as JsonValue;

fn json_string<T: Serialize>(value: &T) -> Result<String, LoomError> {
    serde_json::to_string(value)
        .map_err(|err| LoomError::new(Code::InvalidArgument, err.to_string()))
}

fn parse_optional_json_list<T: DeserializeOwned>(
    value: Option<&str>,
    field: &str,
) -> Result<Vec<T>, LoomError> {
    value
        .map(serde_json::from_str)
        .transpose()
        .map_err(|err| LoomError::new(Code::InvalidArgument, format!("{field}: {err}")))?
        .map_or_else(|| Ok(Vec::new()), Ok)
}

fn ticket_field_value_changes(fields: &JsonValue) -> Vec<MutationChange> {
    fields.as_object().map_or_else(Vec::new, |fields| {
        fields
            .iter()
            .map(|(field, value)| MutationChange::field_set(field.clone(), value.to_string()))
            .collect()
    })
}

fn ticket_update_changes(
    set_fields: Option<&JsonValue>,
    delete_fields: &[String],
    action_applied: bool,
    target_status: Option<&str>,
    observed_source_status: Option<&str>,
    assignee: Option<&str>,
    comment_types: impl IntoIterator<Item = Option<String>>,
    relation_sets: impl IntoIterator<Item = (String, String, String)>,
    relation_removes: impl IntoIterator<Item = String>,
) -> Vec<MutationChange> {
    let mut changes = set_fields
        .map(ticket_field_value_changes)
        .unwrap_or_default();
    changes.extend(
        delete_fields
            .iter()
            .map(|field| MutationChange::field_deleted(field.clone(), None::<String>)),
    );
    if let Some(target_status) = target_status {
        changes.push(MutationChange::field_changed(
            "status",
            observed_source_status.map(str::to_string),
            Some(target_status.to_string()),
        ));
    }
    if let Some(assignee) = assignee {
        changes.push(MutationChange::field_changed(
            "assignee",
            None::<String>,
            Some(assignee.to_string()),
        ));
    }
    if action_applied && target_status.is_none() {
        changes.push(MutationChange::field_set("lifecycle_action", "applied"));
    }
    for comment_type in comment_types {
        changes.push(MutationChange::field_set(
            "comment",
            comment_type.unwrap_or_else(|| "comment".to_string()),
        ));
    }
    changes.extend(
        relation_sets
            .into_iter()
            .map(|(relation_id, kind, target_id)| {
                MutationChange::relation_set(relation_id, kind, target_id)
            }),
    );
    changes.extend(relation_removes.into_iter().map(|relation_id| {
        MutationChange::field_deleted(format!("relation:{relation_id}"), None::<String>)
    }));
    changes
}

fn ticket_mutation_json(
    ticket: loom_tickets::TicketSummary,
    operation: &str,
    root_before: Option<&str>,
    changes: Vec<MutationChange>,
) -> Result<String, LoomError> {
    let receipt = MutationReceipt::new(operation, "ticket", ticket.primary_key.clone())
        .operation_id(ticket.operation_id.clone())
        .roots(
            root_before.map(str::to_string),
            Some(ticket.profile_root.clone()),
        )
        .changes(changes);
    json_string(&MutationEnvelope::new(ticket, receipt))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn service_ns_selector(workspace: &str) -> WsSelector {
    match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Name(workspace.to_string()),
    }
}

fn sync_ticket_references(
    loom: &mut loom_core::Loom<loom_store::FileStore>,
    workspace: WorkspaceId,
    ticket: &loom_tickets::TicketSummary,
) -> Result<(), LoomError> {
    loom_tickets::update_ticket_field_references(
        loom,
        workspace,
        &ticket.workspace_id,
        &ticket.ticket_id,
        &ticket.fields,
    )?;
    if let Some(operation_id) = ticket.operation_id.as_deref() {
        loom_tickets::enqueue_ticket_reference_candidates(
            loom,
            workspace,
            loom_tickets::TicketReferenceCandidateRequest {
                workspace_id: &ticket.workspace_id,
                ticket_id: &ticket.ticket_id,
                operation_id,
                source_root: CoreDigest::parse(&ticket.profile_root)?,
                fields: &ticket.fields,
                now_ms: now_ms(),
            },
        )?;
    }
    Ok(())
}

fn parse_ticket_lifecycle_action(
    value: Option<&str>,
) -> Result<Option<loom_tickets::TicketLifecycleAction>, LoomError> {
    value
        .map(loom_tickets::TicketLifecycleAction::parse)
        .transpose()
}

#[derive(Deserialize)]
struct ServiceTicketUpdateComment {
    #[serde(default)]
    comment_id: Option<String>,
    #[serde(default)]
    comment_type: Option<String>,
    body: String,
}

#[derive(Deserialize)]
struct ServiceTicketUpdateRelationSet {
    #[serde(default)]
    relation_id: Option<String>,
    kind: String,
    target_id: String,
}

#[derive(Deserialize)]
struct ServiceTicketUpdateRelationRemove {
    relation_id: String,
}

fn parse_service_workspace_id(value: &str, field: &str) -> Result<WorkspaceId, LoomError> {
    WorkspaceId::parse(value)
        .map_err(|err| LoomError::new(Code::InvalidArgument, format!("{field}: {}", err.message)))
}

fn parse_string_list_json(value: &str, field: &str) -> Result<Vec<String>, LoomError> {
    serde_json::from_str(value)
        .map_err(|err| LoomError::new(Code::InvalidArgument, format!("{field}: {err}")))
}

fn make_result_view(id: u64) -> ResultView {
    ResultView(HandleId {
        kind: "result_view".to_string(),
        id: id.to_be_bytes().to_vec(),
        generation: 1,
        owner_session: Vec::new(),
    })
}

fn result_view_id(view: &ResultView) -> Result<u64, LoomError> {
    let bytes: [u8; 8] =
        view.0.id.as_slice().try_into().map_err(|_| {
            LoomError::new(Code::InvalidArgument, "malformed result view handle id")
        })?;
    Ok(u64::from_be_bytes(bytes))
}

fn daemon_unavailable(op: &str) -> LoomError {
    LoomError::new(
        Code::Unsupported,
        format!("{op} is host process control and is not available on the in-process client"),
    )
}

// Compile-contract floor for IDL interfaces owned by a concurrent session (Meetings, StudioSurfaces):
// they are present in `idl/loom.idl` and therefore in the generated `LoomClient` supertrait, so
// `LocalLoomClient` must implement them to satisfy the generated contract. These are not real
// implementations and make no claim about parity or spec status; they reject with a precise
// Unsupported error until the owning session lands the real behavior.
fn idl_contract_unimplemented(op: &str) -> LoomError {
    LoomError::new(
        Code::Unsupported,
        format!("{op} is declared in idl/loom.idl but not implemented by LocalLoomClient yet"),
    )
}

fn random_bytes(buf: &mut [u8]) -> Result<(), LoomError> {
    getrandom::fill(buf).map_err(|err| LoomError::new(Code::Internal, format!("rng: {err}")))
}

fn principal_from_uuid(uuid: Uuid) -> WorkspaceId {
    WorkspaceId::from_bytes(uuid.0)
}

/// Convert a wire `Uuid` into the stable id it carries (role, credential, or key id).
fn id_from_uuid(uuid: Uuid) -> WorkspaceId {
    WorkspaceId::from_bytes(uuid.0)
}

/// Mint a fresh v4 workspace/entity id for a server-assigned handle (external credential, public key).
fn mint_uuid() -> Result<WorkspaceId, LoomError> {
    let mut bytes = [0u8; 16];
    random_bytes(&mut bytes)?;
    Ok(WorkspaceId::v4_from_bytes(bytes))
}

fn digest_out(digest: CoreDigest) -> Digest {
    Digest(digest.to_string())
}

fn digest_in(digest: &Digest) -> Result<CoreDigest, LoomError> {
    CoreDigest::parse(&digest.0)
        .map_err(|_| LoomError::new(Code::InvalidArgument, "malformed digest"))
}

fn kek_bytes(kek: Vec<u8>) -> Result<[u8; KEY_LEN], LoomError> {
    kek.as_slice()
        .try_into()
        .map_err(|_| LoomError::new(Code::InvalidArgument, "kek must be 32 bytes"))
}

/// A ready [`LoomStream`] over an already-buffered SQL result. `LocalLoomClient` holds the full row
/// set in memory and yields it one row at a time through the generated streaming shape.
struct ReadyRows(std::vec::IntoIter<Vec<u8>>);

impl futures_core::Stream for ReadyRows {
    type Item = Result<Vec<u8>, LoomError>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Ready(self.get_mut().0.next().map(Ok))
    }
}

fn ready_rows(rows: Vec<Vec<u8>>) -> LoomStream<Vec<u8>> {
    Box::pin(ReadyRows(rows.into_iter()))
}

/// Server-advertised export chunk size for the byte-transfer export stream (`specs/0067` §17.5).
const TRANSFER_EXPORT_CHUNK_BYTES: usize = 1024 * 1024;

/// Split `bytes` into bounded chunks for a byte-transfer export stream. Empty input yields no items.
fn chunk_bytes(bytes: &[u8], chunk: usize) -> Vec<Vec<u8>> {
    bytes.chunks(chunk.max(1)).map(<[u8]>::to_vec).collect()
}

impl Exec for LocalLoomClient {
    fn exec_cbor(
        &self,
        handle: LoomSession,
        request: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.exec_cbor(&handle, &request);
        async move { out }
    }
}

impl Program for LocalLoomClient {
    fn program_put(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        manifest: Vec<u8>,
        body: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.program_put(&handle, &workspace, &name, &manifest, &body);
        async move { out }
    }

    fn program_inspect(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.program_inspect(&handle, &workspace, &name);
        async move { out }
    }

    fn program_get(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.program_get(&handle, &workspace, &name);
        async move { out }
    }

    fn program_list(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.program_list(&handle, &workspace);
        async move { out }
    }

    fn program_remove(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.program_remove(&handle, &workspace, &name);
        async move { out }
    }
}

impl Sessions for LocalLoomClient {
    fn authenticate_passphrase(
        &self,
        handle: LoomSession,
        principal: Uuid,
        passphrase: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out =
            self.authenticate_passphrase(&handle, principal_from_uuid(principal), &passphrase);
        async move { out }
    }

    fn clear_authentication(
        &self,
        handle: LoomSession,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.clear_authentication(&handle);
        async move { out }
    }
}

impl KeySource for LocalLoomClient {
    fn key_add_wrap_keyed(
        &self,
        handle: LoomSession,
        new_passphrase: Vec<u8>,
        allow_no_recovery: bool,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let mut salt = [0u8; 16];
            let mut wrap_nonce = [0u8; 24];
            random_bytes(&mut salt)?;
            random_bytes(&mut wrap_nonce)?;
            self.key_add_wrap_keyed(
                &handle,
                &new_passphrase,
                salt.to_vec(),
                wrap_nonce.to_vec(),
                allow_no_recovery,
            )
        })();
        async move { out }
    }

    fn key_add_wrap_with_kek(
        &self,
        handle: LoomSession,
        kek: Vec<u8>,
        allow_no_recovery: bool,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let kek: [u8; KEY_LEN] = kek
                .as_slice()
                .try_into()
                .map_err(|_| LoomError::new(Code::InvalidArgument, "kek must be 32 bytes"))?;
            let mut salt = [0u8; 16];
            let mut wrap_nonce = [0u8; 24];
            random_bytes(&mut salt)?;
            random_bytes(&mut wrap_nonce)?;
            self.key_add_wrap_with_kek(
                &handle,
                kek,
                salt.to_vec(),
                wrap_nonce.to_vec(),
                allow_no_recovery,
            )
        })();
        async move { out }
    }

    fn key_remove_wrap(
        &self,
        handle: LoomSession,
        index: u64,
        allow_no_recovery: bool,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let index = usize::try_from(index)
                .map_err(|_| LoomError::new(Code::InvalidArgument, "wrap index out of range"))?;
            self.key_remove_wrap(&handle, index, allow_no_recovery)
        })();
        async move { out }
    }
}

impl Store for LocalLoomClient {
    fn version(&self) -> impl ::core::future::Future<Output = Result<String, LoomError>> + Send {
        let out = Ok(self.store_version());
        async move { out }
    }

    fn capabilities(
        &self,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let caps: Vec<Value> = self
            .store_capabilities()
            .into_iter()
            .map(Value::Text)
            .collect();
        let out = loom_codec::encode(&Value::Array(caps))
            .map_err(|err| LoomError::new(Code::Internal, format!("capabilities cbor: {err}")));
        async move { out }
    }

    fn runtime_profile(
        &self,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = Ok(self.store_runtime_profile().to_cbor());
        async move { out }
    }

    fn blob_digest(&self, data: Vec<u8>) -> Result<Digest, LoomError> {
        Ok(Digest(self.blob_digest(&data).to_string()))
    }

    fn digest_algo(
        &self,
    ) -> impl ::core::future::Future<Output = Result<String, LoomError>> + Send {
        let out = self.store_digest_algo();
        async move { out }
    }

    fn create(
        &self,
        profile: String,
        suite: Option<String>,
        passphrase: Option<Vec<u8>>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let key = match passphrase.as_deref() {
                Some(p) => Some(KeySpec::passphrase(std::str::from_utf8(p).map_err(
                    |_| LoomError::new(Code::InvalidArgument, "passphrase is not valid utf-8"),
                )?)),
                None => None,
            };
            self.create_store(&profile, suite.as_deref(), key)
        })();
        async move { out }
    }

    fn create_with_kek(
        &self,
        profile: String,
        suite: Option<String>,
        kek: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let kek: [u8; KEY_LEN] = kek
                .as_slice()
                .try_into()
                .map_err(|_| LoomError::new(Code::InvalidArgument, "kek must be 32 bytes"))?;
            self.create_store(&profile, suite.as_deref(), Some(KeySpec::raw_kek(kek)))
        })();
        async move { out }
    }

    fn open(&self) -> impl ::core::future::Future<Output = Result<LoomSession, LoomError>> + Send {
        let out = LocalLoomClient::open(self);
        async move { out }
    }

    fn open_keyed(
        &self,
        passphrase: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<LoomSession, LoomError>> + Send {
        let out = self.open_keyed(&passphrase);
        async move { out }
    }

    fn open_with_kek(
        &self,
        kek: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<LoomSession, LoomError>> + Send {
        let out = (|| {
            let kek: [u8; KEY_LEN] = kek
                .as_slice()
                .try_into()
                .map_err(|_| LoomError::new(Code::InvalidArgument, "kek must be 32 bytes"))?;
            self.open_with_kek(kek)
        })();
        async move { out }
    }

    fn close(
        &self,
        handle: LoomSession,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let _ = self.close(&handle);
        async move { Ok(()) }
    }
}

impl Diagnostics for LocalLoomClient {
    fn result_to_json(&self, result: Vec<u8>) -> Result<String, LoomError> {
        self.record(loom_result::result_to_json(&result))
    }

    fn result_to_bridge_json(&self, result: Vec<u8>) -> Result<String, LoomError> {
        self.record(loom_result::to_bridge_json(&result))
    }

    fn last_error(&self) -> Result<Vec<u8>, LoomError> {
        Ok(self.last_error_cbor())
    }
}

impl ResultViews for LocalLoomClient {
    fn result_open(&self, result: Vec<u8>) -> Result<ResultView, LoomError> {
        let payload = self.record(loom_result::result_view::decode(&result))?;
        Ok(make_result_view(self.register_result_view(payload)))
    }

    fn row_open(&self, row: Vec<u8>) -> Result<ResultView, LoomError> {
        let decoded = self.record(
            loom_codec::decode(&row)
                .map_err(|err| LoomError::new(Code::CorruptObject, format!("row cbor: {err}"))),
        )?;
        let Value::Array(cells) = decoded else {
            return Err(self
                .record::<()>(Err(LoomError::new(
                    Code::CorruptObject,
                    "row is not a cell array",
                )))
                .unwrap_err());
        };
        let cells = self.record(
            cells
                .into_iter()
                .map(cell_from)
                .collect::<Result<Vec<_>, LoomError>>(),
        )?;
        let payload = ResultPayload::Reader(Reader::Rows {
            columns: Vec::new(),
            rows: vec![cells],
        });
        Ok(make_result_view(self.register_result_view(payload)))
    }

    fn result_close(&self, view: ResultView) -> Result<(), LoomError> {
        self.drop_result_view(result_view_id(&view)?);
        Ok(())
    }

    fn result_len(&self, view: ResultView) -> Result<u64, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| Ok(view::len(p)))
    }

    fn result_is_statements(&self, view: ResultView) -> Result<Option<bool>, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| Ok(view::is_statements(p)))
    }

    fn result_item_kind(&self, view: ResultView, item: u64) -> Result<Option<Vec<u8>>, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| Ok(view::item_kind(p, item)))
    }

    fn result_column_count(&self, view: ResultView, item: u64) -> Result<u64, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| view::column_count(p, item))
    }

    fn result_column_name(
        &self,
        view: ResultView,
        item: u64,
        col: u64,
    ) -> Result<String, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| view::column_name(p, item, col))
    }

    fn result_column_type(
        &self,
        view: ResultView,
        item: u64,
        col: u64,
    ) -> Result<String, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| view::column_type(p, item, col))
    }

    fn result_row_count(&self, view: ResultView, item: u64) -> Result<u64, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| view::row_count(p, item))
    }

    fn result_row_len(&self, view: ResultView, item: u64, row: u64) -> Result<u64, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| view::row_len(p, item, row))
    }

    fn result_cell(
        &self,
        view: ResultView,
        item: u64,
        row: u64,
        col: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| view::cell(p, item, row, col))
    }

    fn result_row_commit(
        &self,
        view: ResultView,
        item: u64,
        row: u64,
    ) -> Result<String, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| view::row_commit(p, item, row))
    }

    fn result_count(&self, view: ResultView, item: u64) -> Result<u64, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| view::count(p, item))
    }

    fn result_string_count(&self, view: ResultView, item: u64) -> Result<u64, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| view::string_count(p, item))
    }

    fn result_string(&self, view: ResultView, item: u64, i: u64) -> Result<String, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| view::string(p, item, i))
    }

    fn result_variable_kind(&self, view: ResultView, item: u64) -> Result<Vec<u8>, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| view::variable_kind(p, item))
    }

    fn result_merge_outcome(&self, view: ResultView, item: u64) -> Result<Vec<u8>, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| view::merge_outcome(p, item))
    }

    fn result_diff_count(&self, view: ResultView, item: u64) -> Result<u64, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| view::diff_count(p, item))
    }

    fn result_diff_change(
        &self,
        view: ResultView,
        item: u64,
        entry: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| {
            view::diff_change(p, item, entry)
        })
    }

    fn result_diff_len(
        &self,
        view: ResultView,
        item: u64,
        entry: u64,
        side: Vec<u8>,
    ) -> Result<u64, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| {
            view::diff_len(p, item, entry, &side)
        })
    }

    fn result_diff_cell(
        &self,
        view: ResultView,
        item: u64,
        entry: u64,
        side: Vec<u8>,
        col: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| {
            view::diff_cell(p, item, entry, &side, col)
        })
    }

    fn result_map_len(&self, view: ResultView, item: u64, row: u64) -> Result<u64, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| view::map_len(p, item, row))
    }

    fn result_map_entry(
        &self,
        view: ResultView,
        item: u64,
        row: u64,
        idx: u64,
    ) -> Result<Vec<u8>, LoomError> {
        self.with_result_view(result_view_id(&view)?, |p| {
            view::map_entry(p, item, row, idx)
        })
    }
}

impl QueueConsumers for LocalLoomClient {
    fn consumer_position(
        &self,
        handle: LoomSession,
        workspace: String,
        stream: String,
        consumer_id: String,
    ) -> impl ::core::future::Future<Output = Result<u64, LoomError>> + Send {
        let out = self.consumer_position(&handle, &workspace, &stream, &consumer_id);
        async move { out }
    }

    fn consumer_read(
        &self,
        handle: LoomSession,
        workspace: String,
        stream: String,
        consumer_id: String,
        max: u32,
    ) -> impl ::core::future::Future<Output = Result<Vec<Vec<u8>>, LoomError>> + Send {
        let out = self.consumer_read(&handle, &workspace, &stream, &consumer_id, u64::from(max));
        async move { out }
    }

    fn consumer_advance(
        &self,
        handle: LoomSession,
        workspace: String,
        stream: String,
        consumer_id: String,
        next_seq: u64,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.consumer_advance(&handle, &workspace, &stream, &consumer_id, next_seq);
        async move { out }
    }

    fn consumer_reset(
        &self,
        handle: LoomSession,
        workspace: String,
        stream: String,
        consumer_id: String,
        next_seq: u64,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.consumer_reset(&handle, &workspace, &stream, &consumer_id, next_seq);
        async move { out }
    }
}

impl Tasks for LocalLoomClient {
    fn iter_next(
        &self,
        iter: RowIter,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.iter_next(&iter);
        async move { out }
    }

    fn iter_free(&self, iter: RowIter) -> Result<(), LoomError> {
        let _ = self.iter_free(&iter);
        Ok(())
    }

    fn sql_exec_async(
        &self,
        session: SqlSession,
        sql: String,
    ) -> impl ::core::future::Future<Output = Result<Task, LoomError>> + Send {
        let out = self.sql_exec_async(&session, &sql);
        async move { out }
    }

    fn task_poll(
        &self,
        task: Task,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.task_poll(&task);
        async move { out }
    }

    fn task_status(
        &self,
        task: Task,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.task_status(&task);
        async move { out }
    }

    fn task_result(
        &self,
        task: Task,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.task_result(&task);
        async move { out }
    }

    fn task_cancel(
        &self,
        task: Task,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.task_cancel(&task);
        async move { out }
    }

    fn task_free(&self, task: Task) -> Result<(), LoomError> {
        let _ = self.task_free(&task);
        Ok(())
    }

    fn task_wait(
        &self,
        task: Task,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.task_wait(&task);
        async move { out }
    }
}

impl Cas for LocalLoomClient {
    fn put(
        &self,
        handle: LoomSession,
        workspace: String,
        content: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self.cas_put(&handle, &workspace, &content).map(digest_out);
        async move { out }
    }

    fn get(
        &self,
        handle: LoomSession,
        workspace: String,
        digest: Digest,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = (|| self.cas_get(&handle, &workspace, &digest_in(&digest)?))();
        async move { out }
    }

    fn has(
        &self,
        handle: LoomSession,
        workspace: String,
        digest: Digest,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = (|| self.cas_has(&handle, &workspace, &digest_in(&digest)?))();
        async move { out }
    }

    fn delete(
        &self,
        handle: LoomSession,
        workspace: String,
        digest: Digest,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = (|| self.cas_delete(&handle, &workspace, &digest_in(&digest)?))();
        async move { out }
    }

    fn list(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<Digest>, LoomError>> + Send {
        let out = self
            .cas_list(&handle, &workspace)
            .map(|digests| digests.into_iter().map(digest_out).collect());
        async move { out }
    }
}

impl Dataframe for LocalLoomClient {
    fn create(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        plan: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.dataframe_create(&handle, &workspace, &name, &plan);
        async move { out }
    }

    fn collect(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.dataframe_collect(&handle, &workspace, &name);
        async move { out }
    }

    fn preview(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        rows: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.dataframe_preview(&handle, &workspace, &name, rows);
        async move { out }
    }

    fn materialize(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Digest>, LoomError>> + Send {
        let out = self
            .dataframe_materialize(&handle, &workspace, &name)
            .map(|digest| digest.map(digest_out));
        async move { out }
    }

    fn plan_digest(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self
            .dataframe_plan_digest(&handle, &workspace, &name)
            .map(digest_out);
        async move { out }
    }

    fn source_digests(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .dataframe_source_digests(&handle, &workspace, &name)
            .and_then(loom_wire::digest_list_to_cbor);
        async move { out }
    }
}

impl Kv for LocalLoomClient {
    fn put(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.kv_put(&handle, &workspace, &collection, &key, &value);
        async move { out }
    }

    fn get(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        key: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.kv_get(&handle, &workspace, &collection, &key);
        async move { out }
    }

    fn delete(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        key: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.kv_delete(&handle, &workspace, &collection, &key);
        async move { out }
    }

    fn list(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.kv_list(&handle, &workspace, &collection);
        async move { out }
    }

    fn range(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        lo: Vec<u8>,
        hi: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.kv_range(&handle, &workspace, &collection, &lo, &hi);
        async move { out }
    }

    fn list_collections(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .kv_list_collections(&handle, &workspace)
            .and_then(loom_wire::string_list_to_cbor);
        async move { out }
    }
}

impl Document for LocalLoomClient {
    fn put_text(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        id: String,
        text: String,
        expected_entity_tag: Option<String>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.document_put_text(
            &handle,
            &workspace,
            &collection,
            &id,
            &text,
            expected_entity_tag.as_deref(),
        );
        async move { out }
    }

    fn get_text(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        id: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.document_get_text(&handle, &workspace, &collection, &id);
        async move { out }
    }

    fn put_binary(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        id: String,
        bytes: Vec<u8>,
        expected_entity_tag: Option<String>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.document_put_binary(
            &handle,
            &workspace,
            &collection,
            &id,
            &bytes,
            expected_entity_tag.as_deref(),
        );
        async move { out }
    }

    fn get_binary(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        id: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.document_get_binary(&handle, &workspace, &collection, &id);
        async move { out }
    }

    fn list_binary(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.document_list_binary(&handle, &workspace, &collection);
        async move { out }
    }

    fn put_binary_indexed(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        id: String,
        bytes: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.document_put_binary_indexed(&handle, &workspace, &collection, &id, bytes);
        async move { out }
    }

    fn delete(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        id: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.document_delete(&handle, &workspace, &collection, &id);
        async move { out }
    }

    fn delete_indexed(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        id: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.document_delete_indexed(&handle, &workspace, &collection, &id);
        async move { out }
    }

    fn replace_text_indexed(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        id: String,
        find: String,
        replace: String,
        replace_all: bool,
        base_digest: Digest,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .document_replace_text_indexed(
                &handle,
                DocumentReplaceTextArgs {
                    workspace: &workspace,
                    collection: &collection,
                    id: &id,
                    find: &find,
                    replace: &replace,
                    replace_all,
                    base_digest: &base_digest.0,
                },
            )
            .and_then(|(replacements, digest, entity_tag)| {
                loom_wire::document::replace_text_result_to_cbor(replacements, &digest, &entity_tag)
            });
        async move { out }
    }

    fn list_collections(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .document_list_collections(&handle, &workspace)
            .and_then(loom_wire::string_list_to_cbor);
        async move { out }
    }

    fn index_create(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        name: String,
        path: String,
        unique: bool,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out =
            self.document_index_create(&handle, &workspace, &collection, &name, &path, unique);
        async move { out }
    }

    fn index_create_json(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        declaration_json: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out =
            self.document_index_create_json(&handle, &workspace, &collection, &declaration_json);
        async move { out }
    }

    fn index_drop(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.document_index_drop(&handle, &workspace, &collection, &name);
        async move { out }
    }

    fn index_rebuild(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.document_index_rebuild(&handle, &workspace, &collection, &name);
        async move { out }
    }

    fn index_list_json(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.document_index_list_json(&handle, &workspace, &collection);
        async move { out }
    }

    fn index_status_json(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.document_index_status_json(&handle, &workspace, &collection);
        async move { out }
    }

    fn find_json(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        index: String,
        value_json: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.document_find_json(&handle, &workspace, &collection, &index, &value_json);
        async move { out }
    }

    fn query_json(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        query_json: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.document_query_json(&handle, &workspace, &collection, &query_json);
        async move { out }
    }
}

impl Ledger for LocalLoomClient {
    fn append(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        payload: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<u64, LoomError>> + Send {
        let out = self.ledger_append(&handle, &workspace, &collection, &payload);
        async move { out }
    }

    fn get(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        seq: u64,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.ledger_get(&handle, &workspace, &collection, seq);
        async move { out }
    }

    fn head(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Digest>, LoomError>> + Send {
        let out = self
            .ledger_head(&handle, &workspace, &collection)
            .map(|digest| digest.map(digest_out));
        async move { out }
    }

    fn len(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
    ) -> impl ::core::future::Future<Output = Result<u64, LoomError>> + Send {
        let out = self.ledger_len(&handle, &workspace, &collection);
        async move { out }
    }

    fn verify(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.ledger_verify(&handle, &workspace, &collection);
        async move { out }
    }

    fn list_collections(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .ledger_list_collections(&handle, &workspace)
            .and_then(loom_wire::string_list_to_cbor);
        async move { out }
    }
}

impl TimeSeries for LocalLoomClient {
    fn put(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        ts: i64,
        value: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.ts_put(&handle, &workspace, &collection, ts, &value);
        async move { out }
    }

    fn get(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        ts: i64,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.ts_get(&handle, &workspace, &collection, ts);
        async move { out }
    }

    fn range(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        from: i64,
        to: i64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.ts_range(&handle, &workspace, &collection, from, to);
        async move { out }
    }

    fn latest(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self
            .ts_latest(&handle, &workspace, &collection)
            .map(|point| {
                point.map(|(ts, value)| loom_core::timeseries::latest_point_to_cbor(ts, &value))
            });
        async move { out }
    }

    fn list_collections(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .ts_list_collections(&handle, &workspace)
            .and_then(loom_wire::string_list_to_cbor);
        async move { out }
    }
}

impl FileSystem for LocalLoomClient {
    fn write_file(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
        content: Vec<u8>,
        mode: u32,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.write_file(&handle, &workspace, &path, &content, mode);
        async move { out }
    }

    fn import_fs(
        &self,
        handle: LoomSession,
        workspace: String,
        src_path: String,
        commit: bool,
        dry_run: bool,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.import_fs(&handle, &workspace, &src_path, commit, dry_run);
        async move { out }
    }

    fn export_fs(
        &self,
        handle: LoomSession,
        workspace: String,
        dst_path: String,
        revision: Option<String>,
        dry_run: bool,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.export_fs(&handle, &workspace, &dst_path, revision.as_deref(), dry_run);
        async move { out }
    }

    fn import_fs_async(
        &self,
        handle: LoomSession,
        workspace: String,
        src_path: String,
        commit: bool,
        dry_run: bool,
    ) -> impl ::core::future::Future<Output = Result<Task, LoomError>> + Send {
        let task = self.import_fs_async(&handle, &workspace, &src_path, commit, dry_run);
        async move { Ok(task) }
    }

    fn export_fs_async(
        &self,
        handle: LoomSession,
        workspace: String,
        dst_path: String,
        revision: Option<String>,
        dry_run: bool,
    ) -> impl ::core::future::Future<Output = Result<Task, LoomError>> + Send {
        let task =
            self.export_fs_async(&handle, &workspace, &dst_path, revision.as_deref(), dry_run);
        async move { Ok(task) }
    }

    fn read_file(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.read_file(&handle, &workspace, &path);
        async move { out }
    }

    fn append_file(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
        content: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.append_file(&handle, &workspace, &path, &content);
        async move { out }
    }

    fn remove_file(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.remove_file(&handle, &workspace, &path);
        async move { out }
    }

    fn read_at(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
        offset: u64,
        len: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.read_at(&handle, &workspace, &path, offset, len);
        async move { out }
    }

    fn write_at(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
        offset: u64,
        content: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.write_at(&handle, &workspace, &path, offset, &content);
        async move { out }
    }

    fn truncate(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
        size: u64,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.truncate(&handle, &workspace, &path, size);
        async move { out }
    }

    fn create_directory(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
        recursive: bool,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.create_directory(&handle, &workspace, &path, recursive);
        async move { out }
    }

    fn remove_directory(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
        recursive: bool,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.remove_directory(&handle, &workspace, &path, recursive);
        async move { out }
    }

    fn stat(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.stat(&handle, &workspace, &path);
        async move { out }
    }

    fn list_directory(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.list_directory(&handle, &workspace, &path);
        async move { out }
    }

    fn symlink(
        &self,
        handle: LoomSession,
        workspace: String,
        target: String,
        link_path: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.symlink(&handle, &workspace, &target, &link_path);
        async move { out }
    }

    fn read_link(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
    ) -> impl ::core::future::Future<Output = Result<String, LoomError>> + Send {
        let out = self.read_link(&handle, &workspace, &path);
        async move { out }
    }
}

impl Search for LocalLoomClient {
    fn source_digest(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self
            .search_source_digest(&handle, &workspace, &name)
            .map(digest_out);
        async move { out }
    }

    fn status(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        engine_version: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.search_status(&handle, &workspace, &name, &engine_version);
        async move { out }
    }

    fn create(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        mapping: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.search_create(&handle, &workspace, &name, &mapping);
        async move { out }
    }

    fn index(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: Vec<u8>,
        doc: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.search_index(&handle, &workspace, &name, &id, &doc);
        async move { out }
    }

    fn get(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.search_get(&handle, &workspace, &name, &id);
        async move { out }
    }

    fn delete(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.search_delete(&handle, &workspace, &name, &id);
        async move { out }
    }

    fn ids(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        prefix: Vec<u8>,
        has_prefix: bool,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .search_ids(
                &handle,
                &workspace,
                &name,
                has_prefix.then_some(prefix.as_slice()),
            )
            .map(loom_core::search_ids_cbor);
        async move { out }
    }

    fn remap(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        mapping: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.search_remap(&handle, &workspace, &name, &mapping);
        async move { out }
    }

    fn query(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        request: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.search_query(&handle, &workspace, &name, &request);
        async move { out }
    }
}

impl Columnar for LocalLoomClient {
    fn create(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        columns: Vec<u8>,
        target_segment_rows: u64,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let columns = loom_wire::columnar::columns_from_cbor(&columns)?;
            self.columnar_create(&handle, &workspace, &name, columns, target_segment_rows)
        })();
        async move { out }
    }

    fn append(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        row: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let row = loom_wire::columnar::row_from_cbor(&row)?;
            self.columnar_append(&handle, &workspace, &name, row)
        })();
        async move { out }
    }

    fn compact(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.columnar_compact(&handle, &workspace, &name);
        async move { out }
    }

    fn inspect(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .columnar_inspect(&handle, &workspace, &name)
            .map(loom_wire::columnar::inspect_to_cbor);
        async move { out }
    }

    fn source_digest(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .columnar_source_digest(&handle, &workspace, &name)
            .map(loom_wire::columnar::digest_to_cbor);
        async move { out }
    }

    fn scan(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .columnar_scan(&handle, &workspace, &name)
            .map(loom_wire::columnar::rows_to_cbor);
        async move { out }
    }

    fn columns(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .columnar_columns(&handle, &workspace, &name)
            .map(loom_wire::columnar::columns_to_cbor);
        async move { out }
    }

    fn rows(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<u64, LoomError>> + Send {
        let out = self.columnar_rows(&handle, &workspace, &name);
        async move { out }
    }

    fn select(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        columns: Vec<u8>,
        filter: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let column_names = loom_wire::columnar::select_columns_from_cbor(&columns)?;
            let filter = loom_wire::columnar::select_filter_from_cbor(&filter)?;
            let col_refs: Vec<&str> = column_names.iter().map(String::as_str).collect();
            let filter_ref = filter.as_ref().map(|(c, op, v)| (c.as_str(), *op, v));
            self.columnar_select(&handle, &workspace, &name, &col_refs, filter_ref)
                .map(loom_wire::columnar::rows_to_cbor)
        })();
        async move { out }
    }

    fn aggregate(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        aggregates: Vec<u8>,
        filter: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let aggregates = loom_wire::columnar::aggregates_from_cbor(&aggregates)?;
            let filter = loom_wire::columnar::select_filter_from_cbor(&filter)?;
            let filter_ref = filter.as_ref().map(|(c, op, v)| (c.as_str(), *op, v));
            self.columnar_aggregate(&handle, &workspace, &name, &aggregates, filter_ref)
                .map(loom_wire::columnar::values_to_cbor)
        })();
        async move { out }
    }
}

impl Graph for LocalLoomClient {
    fn upsert_node(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
        props: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let props = loom_wire::graph::props_from_cbor(&props)?;
            self.graph_upsert_node(&handle, &workspace, &name, &id, props)
        })();
        async move { out }
    }

    fn get_node(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self
            .graph_get_node(&handle, &workspace, &name, &id)
            .map(|node| node.map(|props| loom_wire::graph::props_to_cbor(&props)));
        async move { out }
    }

    fn remove_node(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
        cascade: bool,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.graph_remove_node(&handle, &workspace, &name, &id, cascade);
        async move { out }
    }

    fn upsert_edge(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
        src: String,
        dst: String,
        label: String,
        props: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let props = loom_wire::graph::props_from_cbor(&props)?;
            self.graph_upsert_edge(&handle, &workspace, &name, &id, &src, &dst, &label, props)
        })();
        async move { out }
    }

    fn upsert_edge_indexed(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
        src: String,
        dst: String,
        label: String,
        props: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let props = loom_wire::graph::props_from_cbor(&props)?;
            self.graph_upsert_edge_indexed(
                &handle, &workspace, &name, &id, &src, &dst, &label, props,
            )
        })();
        async move { out }
    }

    fn get_edge(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self
            .graph_get_edge(&handle, &workspace, &name, &id)
            .map(|edge| edge.map(|e| loom_wire::graph::edge_to_cbor(&e)));
        async move { out }
    }

    fn remove_edge(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.graph_remove_edge(&handle, &workspace, &name, &id);
        async move { out }
    }

    fn remove_edge_indexed(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.graph_remove_edge_indexed(&handle, &workspace, &name, &id);
        async move { out }
    }

    fn neighbors(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .graph_neighbors(&handle, &workspace, &name, &id)
            .map(loom_wire::graph::strings_array_cbor);
        async move { out }
    }

    fn out_edges(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .graph_out_edges(&handle, &workspace, &name, &id)
            .map(loom_wire::graph::edges_array_cbor);
        async move { out }
    }

    fn in_edges(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .graph_in_edges(&handle, &workspace, &name, &id)
            .map(loom_wire::graph::edges_array_cbor);
        async move { out }
    }

    fn reachable(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        start: String,
        max_depth: i64,
        via_label: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let depth = (max_depth >= 0).then_some(max_depth as usize);
        let via = (!via_label.is_empty()).then_some(via_label.as_str());
        let out = self
            .graph_reachable(&handle, &workspace, &name, &start, depth, via)
            .map(loom_wire::graph::strings_array_cbor);
        async move { out }
    }

    fn shortest_path(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        from: String,
        to: String,
        via_label: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let via = (!via_label.is_empty()).then_some(via_label.as_str());
        let out = self
            .graph_shortest_path(&handle, &workspace, &name, &from, &to, via)
            .map(|path| path.map(loom_wire::graph::strings_array_cbor));
        async move { out }
    }

    fn query(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        query: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let query = loom_core::GraphQuery::parse_opencypher(&query)?;
            self.graph_query(&handle, &workspace, &name, &query)
                .map(|result| loom_wire::graph::graph_query_result_to_cbor(&result))
        })();
        async move { out }
    }

    fn explain_query(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        query: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let query = loom_core::GraphQuery::parse_opencypher(&query)?;
            self.graph_explain_query(&handle, &workspace, &name, &query)
                .map(|explain| loom_wire::graph::graph_query_explain_to_cbor(&explain))
        })();
        async move { out }
    }
}

impl Queue for LocalLoomClient {
    fn append(
        &self,
        handle: LoomSession,
        workspace: String,
        stream: String,
        entry: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<u64, LoomError>> + Send {
        let out = self.queue_append(&handle, &workspace, &stream, &entry);
        async move { out }
    }

    fn get(
        &self,
        handle: LoomSession,
        workspace: String,
        stream: String,
        seq: u64,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.queue_get(&handle, &workspace, &stream, seq);
        async move { out }
    }

    fn range(
        &self,
        handle: LoomSession,
        workspace: String,
        stream: String,
        lo: u64,
        hi: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<Vec<u8>>, LoomError>> + Send {
        let out = self.queue_range(&handle, &workspace, &stream, lo, hi);
        async move { out }
    }

    fn len(
        &self,
        handle: LoomSession,
        workspace: String,
        stream: String,
    ) -> impl ::core::future::Future<Output = Result<u64, LoomError>> + Send {
        let out = self.queue_len(&handle, &workspace, &stream);
        async move { out }
    }

    fn list_streams(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .queue_list_streams(&handle, &workspace)
            .and_then(loom_wire::string_list_to_cbor);
        async move { out }
    }
}

impl Lanes for LocalLoomClient {
    fn create(
        &self,
        handle: LoomSession,
        workspace: String,
        lane: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let lane = loom_lanes::Lane::decode(&lane)?;
            self.lanes_create(&handle, &workspace, lane)?.encode()
        })();
        async move { out }
    }

    fn get(
        &self,
        handle: LoomSession,
        workspace: String,
        lane_id: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self
            .lanes_get(&handle, &workspace, &lane_id)
            .and_then(|lane| lane.map(|lane| lane.encode()).transpose());
        async move { out }
    }

    fn list(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<Vec<u8>>, LoomError>> + Send {
        let out = self.lanes_list(&handle, &workspace).and_then(|lanes| {
            lanes
                .into_iter()
                .map(|lane| lane.encode())
                .collect::<Result<Vec<_>, _>>()
        });
        async move { out }
    }

    async fn get_view_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _lane_id: String,
        _detailed: bool,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Lanes.get_view_json"))
    }

    async fn list_views_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _detailed: bool,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Lanes.list_views_json"))
    }

    fn update(
        &self,
        handle: LoomSession,
        workspace: String,
        lane_id: String,
        title: Option<String>,
        description: Option<String>,
        lane_status: Option<String>,
        status_report: Option<String>,
        reviewer_feedback: Option<String>,
        updated_by: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .lanes_update(
                &handle,
                &workspace,
                LaneUpdateInput {
                    lane_id: &lane_id,
                    title: title.as_deref(),
                    description: description.as_deref(),
                    lane_status: lane_status.as_deref(),
                    status_report: status_report.as_deref(),
                    reviewer_feedback: reviewer_feedback.as_deref(),
                    updated_by: &updated_by,
                },
            )
            .and_then(|lane| lane.encode());
        async move { out }
    }

    fn ticket_add(
        &self,
        handle: LoomSession,
        workspace: String,
        lane_id: String,
        ticket_id: String,
        updated_by: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .lanes_ticket_add(&handle, &workspace, &lane_id, &ticket_id, &updated_by)
            .and_then(|lane| lane.encode());
        async move { out }
    }

    fn ticket_remove(
        &self,
        handle: LoomSession,
        workspace: String,
        lane_id: String,
        ticket_id: String,
        updated_by: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .lanes_ticket_remove(&handle, &workspace, &lane_id, &ticket_id, &updated_by)
            .and_then(|lane| lane.encode());
        async move { out }
    }

    fn ticket_transfer(
        &self,
        handle: LoomSession,
        workspace: String,
        source_lane_id: String,
        target_lane_id: String,
        ticket_id: String,
        updated_by: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .lanes_ticket_transfer(
                &handle,
                &workspace,
                &source_lane_id,
                &target_lane_id,
                &ticket_id,
                &updated_by,
            )
            .and_then(|lane| lane.encode());
        async move { out }
    }

    fn delete(
        &self,
        handle: LoomSession,
        workspace: String,
        lane_id: String,
        updated_by: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .lanes_delete(&handle, &workspace, &lane_id, &updated_by)
            .and_then(|lane| lane.encode());
        async move { out }
    }
}

impl Vector for LocalLoomClient {
    fn create(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        dim: u64,
        metric: i32,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let metric = loom_wire::vector::metric_from_int(metric)?;
            self.vector_create(&handle, &workspace, &name, dim, metric)
        })();
        async move { out }
    }

    fn upsert(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
        vector: Vec<u8>,
        metadata: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let vector = loom_wire::vector::floats_from_bytes(&vector)?;
            let metadata = loom_wire::vector::metadata_from_cbor(&metadata)?;
            self.vector_upsert(&handle, &workspace, &name, &id, vector, metadata)
        })();
        async move { out }
    }

    fn upsert_source(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
        vector: Vec<u8>,
        metadata: Vec<u8>,
        source_text: Vec<u8>,
        model_id: Option<String>,
        weights_digest: Option<String>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let vector = loom_wire::vector::floats_from_bytes(&vector)?;
            let metadata = loom_wire::vector::metadata_from_cbor(&metadata)?;
            let source_text = std::str::from_utf8(&source_text)
                .map_err(|err| {
                    LoomError::new(Code::InvalidArgument, format!("source_text: {err}"))
                })?
                .to_string();
            let model =
                model_id.map(|id| loom_core::EmbeddingModel::new(id, vector.len(), weights_digest));
            self.vector_upsert_source(
                &handle,
                &workspace,
                &name,
                &id,
                vector,
                metadata,
                &source_text,
                model,
            )
        })();
        async move { out }
    }

    fn get(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self
            .vector_get(&handle, &workspace, &name, &id)
            .map(|entry| {
                entry.map(|(vec, meta)| loom_wire::vector::vector_entry_to_cbor(&vec, &meta))
            });
        async move { out }
    }

    fn source_text(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self
            .vector_source_text(&handle, &workspace, &name, &id)
            .map(|text| text.map(String::into_bytes));
        async move { out }
    }

    fn embedding_model(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self
            .vector_embedding_model(&handle, &workspace, &name)
            .map(|model| model.map(|m| loom_wire::vector::embedding_model_cbor(&m)));
        async move { out }
    }

    fn ids(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        prefix: Option<String>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .vector_ids(&handle, &workspace, &name, prefix.as_deref())
            .and_then(loom_wire::string_list_to_cbor);
        async move { out }
    }

    fn metadata_index_keys(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .vector_metadata_index_keys(&handle, &workspace, &name)
            .and_then(loom_wire::string_list_to_cbor);
        async move { out }
    }

    fn create_metadata_index(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        key: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.vector_create_metadata_index(&handle, &workspace, &name, &key);
        async move { out }
    }

    fn drop_metadata_index(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        key: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.vector_drop_metadata_index(&handle, &workspace, &name, &key);
        async move { out }
    }

    fn delete(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        id: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.vector_delete(&handle, &workspace, &name, &id);
        async move { out }
    }

    fn search(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        query: Vec<u8>,
        k: u64,
        filter: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let query = loom_wire::vector::floats_from_bytes(&query)?;
            let filter = loom_wire::vector::meta_filter_from_cbor(&filter)?;
            self.vector_search(&handle, &workspace, &name, &query, k, &filter)
                .map(loom_wire::vector::hits_cbor)
        })();
        async move { out }
    }

    #[allow(clippy::too_many_arguments)]
    fn search_policy(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        query: Vec<u8>,
        k: u64,
        filter: Vec<u8>,
        policy: i32,
        threshold: u64,
        ef: u64,
        pq_m: u64,
        pq_k: u64,
        pq_iters: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let query = loom_wire::vector::floats_from_bytes(&query)?;
            let filter = loom_wire::vector::meta_filter_from_cbor(&filter)?;
            let policy =
                loom_wire::vector::accelerator_policy_from_int(policy, threshold as usize)?;
            self.vector_search_policy(
                &handle, &workspace, &name, &query, k, &filter, policy, ef, pq_m, pq_k, pq_iters,
            )
            .map(loom_wire::vector::hits_cbor)
        })();
        async move { out }
    }
}

impl ManagementKv for LocalLoomClient {
    fn set_config(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
        config: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let config = loom_core::KvMapConfig::decode(&config)?;
            self.set_config(&handle, &workspace, &collection, config)
        })();
        async move { out }
    }

    fn get_config(
        &self,
        handle: LoomSession,
        workspace: String,
        collection: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .get_config(&handle, &workspace, &collection)
            .map(|config| config.encode());
        async move { out }
    }
}

impl Triggers for LocalLoomClient {
    fn trigger_put(
        &self,
        handle: LoomSession,
        workspace: String,
        binding: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.trigger_put(&handle, &workspace, &binding);
        async move { out }
    }

    fn trigger_get(
        &self,
        handle: LoomSession,
        workspace: String,
        id: Uuid,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            self.trigger_get(&handle, &workspace, WorkspaceId::from_bytes(id.0))?
                .ok_or_else(|| LoomError::new(Code::NotFound, "trigger not found"))
        })();
        async move { out }
    }

    fn trigger_list(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .trigger_list(&handle, &workspace)
            .and_then(loom_wire::bytes_list_to_cbor);
        async move { out }
    }

    fn trigger_enable(
        &self,
        handle: LoomSession,
        workspace: String,
        id: Uuid,
        enabled: bool,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.trigger_enable(&handle, &workspace, WorkspaceId::from_bytes(id.0), enabled);
        async move { out }
    }

    fn trigger_remove(
        &self,
        handle: LoomSession,
        workspace: String,
        id: Uuid,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.trigger_remove(&handle, &workspace, WorkspaceId::from_bytes(id.0));
        async move { out }
    }

    fn trigger_history(
        &self,
        handle: LoomSession,
        workspace: String,
        id: Uuid,
        from_seq: u64,
        limit: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .trigger_history(
                &handle,
                &workspace,
                WorkspaceId::from_bytes(id.0),
                from_seq,
                limit,
            )
            .and_then(loom_wire::bytes_list_to_cbor);
        async move { out }
    }
}

impl Sql for LocalLoomClient {
    fn sql_open(
        &self,
        workspace: String,
        db: String,
    ) -> impl ::core::future::Future<Output = Result<SqlSession, LoomError>> + Send {
        let out = self.sql_open(&workspace, &db);
        async move { out }
    }

    fn sql_open_keyed(
        &self,
        workspace: String,
        db: String,
        passphrase: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<SqlSession, LoomError>> + Send {
        let out = self.sql_open_keyed(&workspace, &db, &passphrase);
        async move { out }
    }

    fn sql_open_with_kek(
        &self,
        workspace: String,
        db: String,
        kek: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<SqlSession, LoomError>> + Send {
        let out = (|| self.sql_open_with_kek(&workspace, &db, kek_bytes(kek)?))();
        async move { out }
    }

    fn sql_open_authenticated(
        &self,
        workspace: String,
        db: String,
        auth_principal: Uuid,
        auth_passphrase: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<SqlSession, LoomError>> + Send {
        let out = self.sql_open_authenticated(
            &workspace,
            &db,
            principal_from_uuid(auth_principal),
            &auth_passphrase,
        );
        async move { out }
    }

    fn sql_open_keyed_authenticated(
        &self,
        workspace: String,
        db: String,
        passphrase: Vec<u8>,
        auth_principal: Uuid,
        auth_passphrase: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<SqlSession, LoomError>> + Send {
        let out = self.sql_open_keyed_authenticated(
            &workspace,
            &db,
            &passphrase,
            principal_from_uuid(auth_principal),
            &auth_passphrase,
        );
        async move { out }
    }

    fn sql_open_with_kek_authenticated(
        &self,
        workspace: String,
        db: String,
        kek: Vec<u8>,
        auth_principal: Uuid,
        auth_passphrase: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<SqlSession, LoomError>> + Send {
        let out = (|| {
            self.sql_open_with_kek_authenticated(
                &workspace,
                &db,
                kek_bytes(kek)?,
                principal_from_uuid(auth_principal),
                &auth_passphrase,
            )
        })();
        async move { out }
    }

    fn sql_authenticate_passphrase(
        &self,
        session: SqlSession,
        principal: Uuid,
        passphrase: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out =
            self.sql_authenticate_passphrase(&session, principal_from_uuid(principal), &passphrase);
        async move { out }
    }

    fn sql_exec(
        &self,
        session: SqlSession,
        sql: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.sql_exec(&session, &sql);
        async move { out }
    }

    fn sql_query(
        &self,
        session: SqlSession,
        sql: String,
    ) -> impl ::core::future::Future<Output = Result<LoomStream<Vec<u8>>, LoomError>> + Send {
        let out = self.sql_query(&session, &sql).map(ready_rows);
        async move { out }
    }

    fn sql_commit(
        &self,
        session: SqlSession,
        message: String,
        author: String,
        timestamp_ms: u64,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self
            .sql_commit(&session, &message, &author, timestamp_ms)
            .map(digest_out);
        async move { out }
    }

    fn sql_close(
        &self,
        session: SqlSession,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let _ = self.sql_close(&session);
        async move { Ok(()) }
    }

    fn sql_batch_begin(
        &self,
        workspace: String,
        db: String,
    ) -> impl ::core::future::Future<Output = Result<SqlBatch, LoomError>> + Send {
        let out = self.sql_batch_begin(&workspace, &db);
        async move { out }
    }

    fn sql_batch_begin_keyed(
        &self,
        workspace: String,
        db: String,
        passphrase: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<SqlBatch, LoomError>> + Send {
        let out = self.sql_batch_begin_keyed(&workspace, &db, &passphrase);
        async move { out }
    }

    fn sql_batch_begin_with_kek(
        &self,
        workspace: String,
        db: String,
        kek: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<SqlBatch, LoomError>> + Send {
        let out = (|| self.sql_batch_begin_with_kek(&workspace, &db, kek_bytes(kek)?))();
        async move { out }
    }

    fn sql_batch_begin_authenticated(
        &self,
        workspace: String,
        db: String,
        auth_principal: Uuid,
        auth_passphrase: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<SqlBatch, LoomError>> + Send {
        let out = self.sql_batch_begin_authenticated(
            &workspace,
            &db,
            principal_from_uuid(auth_principal),
            &auth_passphrase,
        );
        async move { out }
    }

    fn sql_batch_begin_keyed_authenticated(
        &self,
        workspace: String,
        db: String,
        passphrase: Vec<u8>,
        auth_principal: Uuid,
        auth_passphrase: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<SqlBatch, LoomError>> + Send {
        let out = self.sql_batch_begin_keyed_authenticated(
            &workspace,
            &db,
            &passphrase,
            principal_from_uuid(auth_principal),
            &auth_passphrase,
        );
        async move { out }
    }

    fn sql_batch_begin_with_kek_authenticated(
        &self,
        workspace: String,
        db: String,
        kek: Vec<u8>,
        auth_principal: Uuid,
        auth_passphrase: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<SqlBatch, LoomError>> + Send {
        let out = (|| {
            self.sql_batch_begin_with_kek_authenticated(
                &workspace,
                &db,
                kek_bytes(kek)?,
                principal_from_uuid(auth_principal),
                &auth_passphrase,
            )
        })();
        async move { out }
    }

    fn sql_batch_exec(
        &self,
        batch: SqlBatch,
        sql: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.sql_batch_exec(&batch, &sql);
        async move { out }
    }

    fn sql_batch_commit(
        &self,
        batch: SqlBatch,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.sql_batch_commit(&batch);
        async move { out }
    }

    fn sql_batch_commit_vcs(
        &self,
        batch: SqlBatch,
        message: String,
        author: String,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self
            .sql_batch_commit_vcs(&batch, &message, &author)
            .map(digest_out);
        async move { out }
    }

    fn sql_batch_abort(
        &self,
        batch: SqlBatch,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.sql_batch_abort(&batch);
        async move { out }
    }

    fn sql_batch_close(
        &self,
        batch: SqlBatch,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let _ = self.sql_batch_close(&batch);
        async move { Ok(()) }
    }

    fn sql_read_table(
        &self,
        handle: LoomSession,
        workspace: String,
        table: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.sql_read_table(&handle, &workspace, &table);
        async move { out }
    }

    fn sql_read_table_at(
        &self,
        handle: LoomSession,
        workspace: String,
        table: String,
        commit: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.sql_read_table_at(&handle, &workspace, &table, &commit);
        async move { out }
    }

    fn sql_index_scan(
        &self,
        handle: LoomSession,
        workspace: String,
        table: String,
        index: String,
        prefix: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.sql_index_scan(&handle, &workspace, &table, &index, &prefix);
        async move { out }
    }

    fn sql_index_scan_at(
        &self,
        handle: LoomSession,
        workspace: String,
        table: String,
        index: String,
        prefix: Vec<u8>,
        commit: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.sql_index_scan_at(&handle, &workspace, &table, &index, &prefix, &commit);
        async move { out }
    }

    fn sql_blame(
        &self,
        handle: LoomSession,
        workspace: String,
        branch: String,
        table: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.sql_blame(&handle, &workspace, &branch, &table);
        async move { out }
    }

    fn sql_diff(
        &self,
        handle: LoomSession,
        workspace: String,
        table: String,
        from_commit: String,
        to_commit: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.sql_diff(&handle, &workspace, &table, &from_commit, &to_commit);
        async move { out }
    }

    fn sql_table_diff(
        &self,
        handle: LoomSession,
        workspace: String,
        table: String,
        from_commit: String,
        to_commit: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.sql_table_diff(&handle, &workspace, &table, &from_commit, &to_commit);
        async move { out }
    }

    fn sql_read_table_async(
        &self,
        handle: LoomSession,
        workspace: String,
        table: String,
    ) -> impl ::core::future::Future<Output = Result<Task, LoomError>> + Send {
        let out = self.sql_read_table_async(&handle, &workspace, &table);
        async move { out }
    }

    fn sql_index_scan_async(
        &self,
        handle: LoomSession,
        workspace: String,
        table: String,
        index: String,
        prefix: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Task, LoomError>> + Send {
        let out = self.sql_index_scan_async(&handle, &workspace, &table, &index, &prefix);
        async move { out }
    }

    fn sql_blame_async(
        &self,
        handle: LoomSession,
        workspace: String,
        branch: String,
        table: String,
    ) -> impl ::core::future::Future<Output = Result<Task, LoomError>> + Send {
        let out = self.sql_blame_async(&handle, &workspace, &branch, &table);
        async move { out }
    }

    fn sql_diff_async(
        &self,
        handle: LoomSession,
        workspace: String,
        table: String,
        from_commit: String,
        to_commit: String,
    ) -> impl ::core::future::Future<Output = Result<Task, LoomError>> + Send {
        let out = self.sql_diff_async(&handle, &workspace, &table, &from_commit, &to_commit);
        async move { out }
    }

    fn sql_list_databases(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.sql_list_databases(&handle, &workspace);
        async move { out }
    }

    fn sql_query_result(
        &self,
        handle: LoomSession,
        workspace: String,
        db: String,
        sql: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.sql_query_result(&handle, &workspace, &db, &sql);
        async move { out }
    }
}

impl Calendar for LocalLoomClient {
    fn put_ics(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        collection: String,
        ics: String,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self
            .calendar_put_ics(&handle, &workspace, &principal, &collection, &ics)
            .map(digest_out);
        async move { out }
    }

    fn create_collection(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        collection: String,
        meta: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out =
            self.calendar_create_collection(&handle, &workspace, &principal, &collection, &meta);
        async move { out }
    }

    fn get_collection(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        collection: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.calendar_get_collection(&handle, &workspace, &principal, &collection);
        async move { out }
    }

    fn list_collections(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .calendar_list_collections(&handle, &workspace, &principal)
            .and_then(loom_wire::string_list_to_cbor);
        async move { out }
    }

    fn delete_collection(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        collection: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.calendar_delete_collection(&handle, &workspace, &principal, &collection);
        async move { out }
    }

    fn put_entry(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        collection: String,
        entry: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self
            .calendar_put_entry(&handle, &workspace, &principal, &collection, &entry)
            .map(digest_out);
        async move { out }
    }

    fn get_entry(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.calendar_get_entry(&handle, &workspace, &principal, &collection, &uid);
        async move { out }
    }

    fn delete_entry(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.calendar_delete_entry(&handle, &workspace, &principal, &collection, &uid);
        async move { out }
    }

    fn list_entries(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        collection: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .calendar_list_entries(&handle, &workspace, &principal, &collection)
            .and_then(loom_wire::bytes_list_to_cbor);
        async move { out }
    }

    fn range(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        collection: String,
        from: String,
        to: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.calendar_range(&handle, &workspace, &principal, &collection, &from, &to);
        async move { out }
    }

    fn search(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        collection: String,
        component: String,
        text: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .calendar_search(
                &handle,
                &workspace,
                &principal,
                &collection,
                &component,
                &text,
            )
            .and_then(loom_wire::bytes_list_to_cbor);
        async move { out }
    }

    fn to_ics(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        collection: String,
        uid: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.calendar_to_ics(&handle, &workspace, &principal, &collection, &uid);
        async move { out }
    }
}

impl Contacts for LocalLoomClient {
    fn put_vcard(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        book: String,
        vcard: String,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self
            .contacts_put_vcard(&handle, &workspace, &principal, &book, &vcard)
            .map(digest_out);
        async move { out }
    }

    fn create_book(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        book: String,
        meta: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.contacts_create_book(&handle, &workspace, &principal, &book, &meta);
        async move { out }
    }

    fn get_book(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        book: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.contacts_get_book(&handle, &workspace, &principal, &book);
        async move { out }
    }

    fn list_books(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .contacts_list_books(&handle, &workspace, &principal)
            .and_then(loom_wire::string_list_to_cbor);
        async move { out }
    }

    fn delete_book(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        book: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.contacts_delete_book(&handle, &workspace, &principal, &book);
        async move { out }
    }

    fn put_entry(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        book: String,
        entry: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self
            .contacts_put_entry(&handle, &workspace, &principal, &book, &entry)
            .map(digest_out);
        async move { out }
    }

    fn get_entry(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.contacts_get_entry(&handle, &workspace, &principal, &book, &uid);
        async move { out }
    }

    fn delete_entry(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.contacts_delete_entry(&handle, &workspace, &principal, &book, &uid);
        async move { out }
    }

    fn list_entries(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        book: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .contacts_list_entries(&handle, &workspace, &principal, &book)
            .and_then(loom_wire::bytes_list_to_cbor);
        async move { out }
    }

    fn search(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        book: String,
        text: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .contacts_search(&handle, &workspace, &principal, &book, &text)
            .and_then(loom_wire::bytes_list_to_cbor);
        async move { out }
    }

    fn to_vcard(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        book: String,
        uid: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.contacts_to_vcard(&handle, &workspace, &principal, &book, &uid);
        async move { out }
    }
}

impl Mail for LocalLoomClient {
    fn create_mailbox(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        mailbox: String,
        meta: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.mail_create_mailbox(&handle, &workspace, &principal, &mailbox, &meta);
        async move { out }
    }

    fn get_mailbox(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        mailbox: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.mail_get_mailbox(&handle, &workspace, &principal, &mailbox);
        async move { out }
    }

    fn list_mailboxes(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .mail_list_mailboxes(&handle, &workspace, &principal)
            .and_then(loom_wire::string_list_to_cbor);
        async move { out }
    }

    fn delete_mailbox(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        mailbox: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.mail_delete_mailbox(&handle, &workspace, &principal, &mailbox);
        async move { out }
    }

    fn ingest_message(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        raw: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self
            .mail_ingest_message(&handle, &workspace, &principal, &mailbox, &uid, &raw)
            .map(digest_out);
        async move { out }
    }

    fn get_message(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.mail_get_message(&handle, &workspace, &principal, &mailbox, &uid);
        async move { out }
    }

    fn to_eml(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.mail_to_eml(&handle, &workspace, &principal, &mailbox, &uid);
        async move { out }
    }

    fn delete_message(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.mail_delete_message(&handle, &workspace, &principal, &mailbox, &uid);
        async move { out }
    }

    fn list_messages(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        mailbox: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .mail_list_messages(&handle, &workspace, &principal, &mailbox)
            .and_then(loom_wire::bytes_list_to_cbor);
        async move { out }
    }

    fn get_flags(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .mail_get_flags(&handle, &workspace, &principal, &mailbox, &uid)
            .and_then(loom_wire::string_list_to_cbor);
        async move { out }
    }

    fn set_flags(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        mailbox: String,
        uid: String,
        flags: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let flags = loom_wire::string_list_from_cbor(&flags)?;
            self.mail_set_flags(&handle, &workspace, &principal, &mailbox, &uid, &flags)
        })();
        async move { out }
    }

    fn search(
        &self,
        handle: LoomSession,
        workspace: String,
        principal: String,
        mailbox: String,
        text: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .mail_search(&handle, &workspace, &principal, &mailbox, &text)
            .and_then(loom_wire::bytes_list_to_cbor);
        async move { out }
    }
}

impl Metrics for LocalLoomClient {
    fn put_descriptor(
        &self,
        handle: LoomSession,
        workspace: String,
        descriptor: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.metrics_put_descriptor(&handle, &workspace, &descriptor);
        async move { out }
    }

    fn get_descriptor(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.metrics_get_descriptor(&handle, &workspace, &name);
        async move { out }
    }

    fn put_observation(
        &self,
        handle: LoomSession,
        workspace: String,
        descriptor_name: String,
        observation: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.metrics_put_observation(&handle, &workspace, &descriptor_name, &observation);
        async move { out }
    }

    #[allow(clippy::too_many_arguments)]
    fn query(
        &self,
        handle: LoomSession,
        workspace: String,
        descriptor_name: String,
        from_timestamp_ms: u64,
        to_timestamp_ms: u64,
        max_series: u32,
        max_groups: u32,
        max_samples: u32,
        max_output_bytes: u64,
        now_timestamp_ms: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.metrics_query(
            &handle,
            &workspace,
            &descriptor_name,
            from_timestamp_ms,
            to_timestamp_ms,
            max_series,
            max_groups,
            max_samples,
            max_output_bytes,
            now_timestamp_ms,
        );
        async move { out }
    }
}

impl Logs for LocalLoomClient {
    fn put_record(
        &self,
        handle: LoomSession,
        workspace: String,
        record: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<String, LoomError>> + Send {
        let out = self.logs_put_record(&handle, &workspace, &record);
        async move { out }
    }

    fn get_record(
        &self,
        handle: LoomSession,
        workspace: String,
        record_id: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.logs_get_record(&handle, &workspace, &record_id);
        async move { out }
    }

    fn query(
        &self,
        handle: LoomSession,
        workspace: String,
        from_time_unix_nano: u64,
        to_time_unix_nano: u64,
        max_records: u32,
        max_output_bytes: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.logs_query(
            &handle,
            &workspace,
            from_time_unix_nano,
            to_time_unix_nano,
            max_records,
            max_output_bytes,
        );
        async move { out }
    }
}

impl Traces for LocalLoomClient {
    fn put_span(
        &self,
        handle: LoomSession,
        workspace: String,
        span: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.traces_put_span(&handle, &workspace, &span);
        async move { out }
    }

    fn get_span(
        &self,
        handle: LoomSession,
        workspace: String,
        trace_id: String,
        span_id: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self.traces_get_span(&handle, &workspace, &trace_id, &span_id);
        async move { out }
    }

    fn trace_spans(
        &self,
        handle: LoomSession,
        workspace: String,
        trace_id: String,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out =
            self.traces_trace_spans(&handle, &workspace, &trace_id, max_spans, max_output_bytes);
        async move { out }
    }

    fn query(
        &self,
        handle: LoomSession,
        workspace: String,
        from_start_time_ns: u64,
        to_start_time_ns: u64,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.traces_query(
            &handle,
            &workspace,
            from_start_time_ns,
            to_start_time_ns,
            max_spans,
            max_output_bytes,
        );
        async move { out }
    }
}

impl Archive for LocalLoomClient {
    fn archive_import(
        &self,
        handle: LoomSession,
        workspace: String,
        src_path: String,
        kind: String,
        dry_run: bool,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.archive_import(&handle, &workspace, &src_path, &kind, dry_run);
        async move { out }
    }

    fn archive_export(
        &self,
        handle: LoomSession,
        workspace: String,
        dst_path: String,
        kind: String,
        revision: Option<String>,
        dry_run: bool,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.archive_export(
            &handle,
            &workspace,
            &dst_path,
            &kind,
            revision.as_deref(),
            dry_run,
        );
        async move { out }
    }

    fn archive_import_async(
        &self,
        handle: LoomSession,
        workspace: String,
        src_path: String,
        kind: String,
        dry_run: bool,
    ) -> impl ::core::future::Future<Output = Result<Task, LoomError>> + Send {
        let task = self.archive_import_async(&handle, &workspace, &src_path, &kind, dry_run);
        async move { Ok(task) }
    }

    fn archive_export_async(
        &self,
        handle: LoomSession,
        workspace: String,
        dst_path: String,
        kind: String,
        revision: Option<String>,
        dry_run: bool,
    ) -> impl ::core::future::Future<Output = Result<Task, LoomError>> + Send {
        let task = self.archive_export_async(
            &handle,
            &workspace,
            &dst_path,
            &kind,
            revision.as_deref(),
            dry_run,
        );
        async move { Ok(task) }
    }
}

impl Car for LocalLoomClient {
    fn car_import(
        &self,
        handle: LoomSession,
        src_path: String,
        dry_run: bool,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.car_import(&handle, &src_path, dry_run);
        async move { out }
    }

    fn car_export(
        &self,
        handle: LoomSession,
        workspace: String,
        dst_path: String,
        dry_run: bool,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.car_export(&handle, &workspace, &dst_path, dry_run);
        async move { out }
    }

    fn car_import_async(
        &self,
        handle: LoomSession,
        src_path: String,
        dry_run: bool,
    ) -> impl ::core::future::Future<Output = Result<Task, LoomError>> + Send {
        let task = self.car_import_async(&handle, &src_path, dry_run);
        async move { Ok(task) }
    }

    fn car_export_async(
        &self,
        handle: LoomSession,
        workspace: String,
        dst_path: String,
        dry_run: bool,
    ) -> impl ::core::future::Future<Output = Result<Task, LoomError>> + Send {
        let task = self.car_export_async(&handle, &workspace, &dst_path, dry_run);
        async move { Ok(task) }
    }
}

impl FileHandle for LocalLoomClient {
    fn open(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
        mode: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<u64, LoomError>> + Send {
        let out = (|| {
            let mode = loom_wire::fs::open_mode_from_wire(&mode)?;
            self.file_open(&handle, &workspace, &path, mode)
        })();
        async move { out }
    }

    fn read(
        &self,
        handle: LoomSession,
        file: u64,
        len: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.file_read(&handle, file, len);
        async move { out }
    }

    fn read_at(
        &self,
        handle: LoomSession,
        file: u64,
        offset: u64,
        len: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.file_read_at(&handle, file, offset, len);
        async move { out }
    }

    fn write(
        &self,
        handle: LoomSession,
        file: u64,
        content: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<u64, LoomError>> + Send {
        let out = self.file_write(&handle, file, &content);
        async move { out }
    }

    fn write_at(
        &self,
        handle: LoomSession,
        file: u64,
        offset: u64,
        content: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<u64, LoomError>> + Send {
        let out = self.file_write_at(&handle, file, offset, &content);
        async move { out }
    }

    fn truncate(
        &self,
        handle: LoomSession,
        file: u64,
        size: u64,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.file_truncate(&handle, file, size);
        async move { out }
    }

    fn flush(
        &self,
        handle: LoomSession,
        file: u64,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.file_flush(&handle, file);
        async move { out }
    }

    fn stat(
        &self,
        handle: LoomSession,
        file: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .file_stat(&handle, file)
            .and_then(loom_wire::fs::file_stat_to_cbor);
        async move { out }
    }

    fn close(
        &self,
        handle: LoomSession,
        file: u64,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.file_close(&handle, file);
        async move { out }
    }
}

impl Workspaces for LocalLoomClient {
    fn workspace_create(
        &self,
        handle: LoomSession,
        name: Option<String>,
        facet: Option<Vec<u8>>,
    ) -> impl ::core::future::Future<Output = Result<Uuid, LoomError>> + Send {
        let out = (|| {
            let facet = match &facet {
                Some(bytes) => Some(loom_wire::workspace::facet_from_wire(bytes)?),
                None => None,
            };
            let id = self.workspace_create(&handle, name.as_deref(), facet)?;
            Ok(Uuid(*id.as_bytes()))
        })();
        async move { out }
    }

    fn workspace_list(
        &self,
        handle: LoomSession,
    ) -> impl ::core::future::Future<Output = Result<Vec<Vec<u8>>, LoomError>> + Send {
        let out = self.workspace_list(&handle).and_then(|infos| {
            infos
                .iter()
                .map(loom_wire::workspace::workspace_info_to_cbor)
                .collect()
        });
        async move { out }
    }

    fn workspace_rename(
        &self,
        handle: LoomSession,
        workspace: String,
        new_name: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.workspace_rename(&handle, &workspace, &new_name);
        async move { out }
    }

    fn workspace_delete(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.workspace_delete(&handle, &workspace);
        async move { out }
    }
}

impl Acl for LocalLoomClient {
    fn acl_list(
        &self,
        handle: LoomSession,
    ) -> impl ::core::future::Future<Output = Result<Vec<Vec<u8>>, LoomError>> + Send {
        let out = self.acl_list(&handle).and_then(|grants| {
            grants
                .iter()
                .map(loom_wire::acl::acl_grant_to_cbor)
                .collect()
        });
        async move { out }
    }

    #[allow(clippy::too_many_arguments)]
    fn acl_grant(
        &self,
        handle: LoomSession,
        effect: Vec<u8>,
        subject: String,
        workspace: Option<String>,
        facet: Option<Vec<u8>>,
        ref_glob: Option<String>,
        scopes: Option<Vec<Vec<u8>>>,
        rights: Option<Vec<Vec<u8>>>,
        predicate: Option<Vec<u8>>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let workspace = match &workspace {
                Some(ws) => Some(self.resolve_workspace_id(&handle, ws)?),
                None => None,
            };
            let grant = loom_wire::acl::acl_grant_from_wire(
                &effect,
                &subject,
                workspace,
                facet.as_deref(),
                ref_glob,
                scopes.as_deref(),
                rights.as_deref(),
                predicate.as_deref(),
            )?;
            self.acl_grant(&handle, grant)
        })();
        async move { out }
    }

    #[allow(clippy::too_many_arguments)]
    fn acl_revoke(
        &self,
        handle: LoomSession,
        effect: Vec<u8>,
        subject: String,
        workspace: Option<String>,
        facet: Option<Vec<u8>>,
        ref_glob: Option<String>,
        scopes: Option<Vec<Vec<u8>>>,
        rights: Option<Vec<Vec<u8>>>,
        predicate: Option<Vec<u8>>,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = (|| {
            let workspace = match &workspace {
                Some(ws) => Some(self.resolve_workspace_id(&handle, ws)?),
                None => None,
            };
            let grant = loom_wire::acl::acl_grant_from_wire(
                &effect,
                &subject,
                workspace,
                facet.as_deref(),
                ref_glob,
                scopes.as_deref(),
                rights.as_deref(),
                predicate.as_deref(),
            )?;
            self.acl_revoke(&handle, &grant)
        })();
        async move { out }
    }
}

impl ProtectedRefs for LocalLoomClient {
    fn protected_ref_list(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<Vec<u8>>, LoomError>> + Send {
        let out = self
            .protected_ref_list(&handle, &workspace)
            .and_then(|policies| {
                policies
                    .iter()
                    .map(|(ref_name, policy)| {
                        loom_wire::protected_ref::named_protected_ref_policy_to_cbor(
                            ref_name, policy,
                        )
                    })
                    .collect()
            });
        async move { out }
    }

    fn protected_ref_get(
        &self,
        handle: LoomSession,
        workspace: String,
        ref_name: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Vec<u8>>, LoomError>> + Send {
        let out = self
            .protected_ref_get(&handle, &workspace, &ref_name)
            .and_then(|policy| {
                policy
                    .as_ref()
                    .map(loom_wire::protected_ref::protected_ref_policy_to_cbor)
                    .transpose()
            });
        async move { out }
    }

    #[allow(clippy::too_many_arguments)]
    fn protected_ref_set(
        &self,
        handle: LoomSession,
        workspace: String,
        ref_name: String,
        fast_forward_only: bool,
        signed_commits_required: bool,
        signed_ref_advance_required: bool,
        required_review_count: u32,
        retention_lock: bool,
        governance_lock: bool,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let policy = ProtectedRefPolicy {
            fast_forward_only,
            signed_commits_required,
            signed_ref_advance_required,
            required_review_count,
            retention_lock,
            governance_lock,
        };
        let out = self.protected_ref_set(&handle, &workspace, &ref_name, policy);
        async move { out }
    }

    fn protected_ref_remove(
        &self,
        handle: LoomSession,
        workspace: String,
        ref_name: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.protected_ref_remove(&handle, &workspace, &ref_name);
        async move { out }
    }
}

impl Watch for LocalLoomClient {
    fn subscribe(
        &self,
        handle: LoomSession,
        selector: Vec<u8>,
        from: Option<Digest>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let selector = loom_wire::watch::watch_selector_from_wire(&selector)?;
            let from = from.as_ref().map(digest_in).transpose()?;
            let cursor = self.watch_subscribe_selector(&handle, selector, from)?;
            Ok(cursor.into_bytes())
        })();
        async move { out }
    }

    fn poll(
        &self,
        handle: LoomSession,
        cursor: String,
        max: u32,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let batch = self.watch_poll(&handle, &cursor, max)?;
            watch_batch_to_cbor(&batch)
        })();
        async move { out }
    }

    fn stream(
        &self,
        handle: LoomSession,
        selector: Vec<u8>,
        from: Option<Digest>,
    ) -> impl ::core::future::Future<Output = Result<LoomStream<Vec<u8>>, LoomError>> + Send {
        // The in-process client buffers one poll of the currently-available events and yields it as a
        // single batch item (the same CBOR shape `poll` returns); the cursor advances within that batch.
        let out = (|| {
            let selector = loom_wire::watch::watch_selector_from_wire(&selector)?;
            let from = from.as_ref().map(digest_in).transpose()?;
            let cursor = self.watch_subscribe_selector(&handle, selector, from)?;
            let batch = self.watch_poll(&handle, &cursor, u32::MAX)?;
            Ok(ready_rows(vec![watch_batch_to_cbor(&batch)?]))
        })();
        async move { out }
    }
}

impl Identity for LocalLoomClient {
    fn identity_list(
        &self,
        handle: LoomSession,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .identity_snapshot_store(&handle)
            .and_then(|store| loom_wire::identity::identity_snapshot_to_cbor(&store));
        async move { out }
    }

    fn identity_add_principal(
        &self,
        handle: LoomSession,
        principal_handle: String,
        name: String,
        kind: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Uuid, LoomError>> + Send {
        let out = (|| {
            let kind = loom_wire::identity::principal_kind_from_wire(&kind)?;
            let id = self.identity_add_principal(&handle, &principal_handle, &name, kind)?;
            Ok(Uuid(*id.as_bytes()))
        })();
        async move { out }
    }

    fn identity_rename_principal_handle(
        &self,
        handle: LoomSession,
        principal: Uuid,
        new_handle: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.identity_rename_principal_handle(
            &handle,
            principal_from_uuid(principal),
            &new_handle,
        );
        async move { out }
    }

    fn identity_set_passphrase(
        &self,
        handle: LoomSession,
        principal: Uuid,
        passphrase: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            // The IDL method carries no salt; mirror the C ABI and mint a fresh random 16-byte salt.
            let mut salt = [0u8; 16];
            random_bytes(&mut salt)?;
            self.identity_set_passphrase(
                &handle,
                principal_from_uuid(principal),
                &passphrase,
                &salt,
            )
        })();
        async move { out }
    }

    fn identity_remove_principal(
        &self,
        handle: LoomSession,
        principal: Uuid,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.identity_remove_principal(&handle, principal_from_uuid(principal));
        async move { out }
    }

    fn identity_assign_role(
        &self,
        handle: LoomSession,
        principal: Uuid,
        role: Uuid,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out =
            self.identity_assign_role(&handle, principal_from_uuid(principal), id_from_uuid(role));
        async move { out }
    }

    fn identity_revoke_role(
        &self,
        handle: LoomSession,
        principal: Uuid,
        role: Uuid,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out =
            self.identity_revoke_role(&handle, principal_from_uuid(principal), id_from_uuid(role));
        async move { out }
    }

    fn identity_create_external_credential(
        &self,
        handle: LoomSession,
        principal: Uuid,
        credential: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let id = mint_uuid()?;
            let spec = loom_wire::identity::external_credential_spec_from_wire(&credential, id)?;
            let result = self.identity_create_external_credential(
                &handle,
                principal_from_uuid(principal),
                spec,
            )?;
            loom_wire::identity::identity_audit_result_to_cbor(&result)
        })();
        async move { out }
    }

    fn identity_revoke_external_credential(
        &self,
        handle: LoomSession,
        credential: Uuid,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let result =
                self.identity_revoke_external_credential(&handle, id_from_uuid(credential))?;
            loom_wire::identity::identity_audit_result_to_cbor(&result)
        })();
        async move { out }
    }

    fn identity_add_public_key(
        &self,
        handle: LoomSession,
        principal: Uuid,
        label: String,
        algorithm: String,
        public_key: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let id = mint_uuid()?;
            let spec = IdentityPublicKeySpec {
                id,
                label,
                algorithm,
                public_key,
            };
            let result =
                self.identity_add_public_key(&handle, principal_from_uuid(principal), spec)?;
            loom_wire::identity::identity_audit_result_to_cbor(&result)
        })();
        async move { out }
    }

    fn identity_revoke_public_key(
        &self,
        handle: LoomSession,
        key: Uuid,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let result = self.identity_revoke_public_key(&handle, id_from_uuid(key))?;
            loom_wire::identity::identity_audit_result_to_cbor(&result)
        })();
        async move { out }
    }

    fn identity_create_app_credential(
        &self,
        handle: LoomSession,
        principal: Uuid,
        label: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let result = self.identity_create_app_credential(
                &handle,
                principal_from_uuid(principal),
                &label,
            )?;
            loom_wire::identity::app_credential_create_result_to_cbor(&result)
        })();
        async move { out }
    }

    fn identity_revoke_app_credential(
        &self,
        handle: LoomSession,
        credential: Uuid,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let result = self.identity_revoke_app_credential(&handle, id_from_uuid(credential))?;
            loom_wire::identity::identity_audit_result_to_cbor(&result)
        })();
        async move { out }
    }
}

impl VersionControl for LocalLoomClient {
    fn head_branch(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<String, LoomError>> + Send {
        let out = self.vcs_head_branch(&handle, &workspace);
        async move { out }
    }

    fn commit(
        &self,
        handle: LoomSession,
        workspace: String,
        author: String,
        message: String,
        timestamp_ms: u64,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self
            .commit(&handle, &workspace, &author, &message, timestamp_ms)
            .map(digest_out);
        async move { out }
    }

    fn branch(
        &self,
        handle: LoomSession,
        workspace: String,
        branch: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.branch(&handle, &workspace, &branch);
        async move { out }
    }

    fn checkout(
        &self,
        handle: LoomSession,
        workspace: String,
        branch: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.checkout(&handle, &workspace, &branch);
        async move { out }
    }

    fn log(
        &self,
        handle: LoomSession,
        workspace: String,
        branch: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<Digest>, LoomError>> + Send {
        let out = self
            .log(&handle, &workspace, &branch)
            .map(|digests| digests.into_iter().map(digest_out).collect());
        async move { out }
    }

    fn merge(
        &self,
        handle: LoomSession,
        workspace: String,
        from_branch: String,
        author: String,
        cell_level: bool,
        timestamp_ms: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .vcs_merge(
                &handle,
                &workspace,
                &from_branch,
                &author,
                cell_level,
                timestamp_ms,
            )
            .and_then(|outcome| loom_wire::vcs::merge_result_to_cbor(&outcome));
        async move { out }
    }

    fn merge_in_progress(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<bool, LoomError>> + Send {
        let out = self.merge_in_progress(&handle, &workspace);
        async move { out }
    }

    fn merge_conflicts(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<String>, LoomError>> + Send {
        let out = self.merge_conflicts(&handle, &workspace);
        async move { out }
    }

    fn merge_resolve(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
        resolution: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let resolution = loom_wire::vcs::conflict_resolution_from_wire(&resolution)?;
            self.merge_resolve(&handle, &workspace, &path, resolution)
        })();
        async move { out }
    }

    fn merge_abort(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.merge_abort(&handle, &workspace);
        async move { out }
    }

    fn merge_continue(
        &self,
        handle: LoomSession,
        workspace: String,
        author: String,
        timestamp_ms: u64,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self
            .merge_continue(&handle, &workspace, &author, timestamp_ms)
            .map(digest_out);
        async move { out }
    }

    fn diff(
        &self,
        handle: LoomSession,
        workspace: String,
        from_commit: String,
        to_commit: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.vcs_diff(&handle, &workspace, &from_commit, &to_commit);
        async move { out }
    }

    fn blame(
        &self,
        handle: LoomSession,
        workspace: String,
        branch: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .vcs_blame(&handle, &workspace, &branch)
            .and_then(|rows| loom_wire::vcs::blame_rows_to_cbor(&rows));
        async move { out }
    }

    fn log_async(
        &self,
        handle: LoomSession,
        workspace: String,
        branch: String,
    ) -> impl ::core::future::Future<Output = Result<Task, LoomError>> + Send {
        let out = Ok(self.log_async(&handle, &workspace, &branch));
        async move { out }
    }

    fn merge_async(
        &self,
        handle: LoomSession,
        workspace: String,
        from_branch: String,
        author: String,
        cell_level: bool,
    ) -> impl ::core::future::Future<Output = Result<Task, LoomError>> + Send {
        let out = Ok(self.merge_async(&handle, &workspace, &from_branch, &author, cell_level));
        async move { out }
    }

    fn status(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .status(&handle, &workspace)
            .and_then(|status| loom_wire::vcs::status_to_cbor(&status));
        async move { out }
    }

    fn stage(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.stage(&handle, &workspace, &path);
        async move { out }
    }

    fn stage_all(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.stage_all(&handle, &workspace);
        async move { out }
    }

    fn unstage(
        &self,
        handle: LoomSession,
        workspace: String,
        path: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.unstage(&handle, &workspace, &path);
        async move { out }
    }

    fn commit_staged(
        &self,
        handle: LoomSession,
        workspace: String,
        author: String,
        message: String,
        timestamp_ms: u64,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self
            .commit_staged(&handle, &workspace, &author, &message, timestamp_ms)
            .map(digest_out);
        async move { out }
    }

    fn tag_create(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
        rev: String,
        tagger: String,
        message: String,
        timestamp_ms: u64,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self
            .tag_create(
                &handle,
                &workspace,
                &name,
                &rev,
                &tagger,
                &message,
                timestamp_ms,
            )
            .map(digest_out);
        async move { out }
    }

    fn tag_list(
        &self,
        handle: LoomSession,
        workspace: String,
    ) -> impl ::core::future::Future<Output = Result<Vec<String>, LoomError>> + Send {
        let out = self.tag_list(&handle, &workspace);
        async move { out }
    }

    fn tag_target(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<Option<Digest>, LoomError>> + Send {
        let out = self
            .tag_target(&handle, &workspace, &name)
            .map(|target| target.map(digest_out));
        async move { out }
    }

    fn tag_delete(
        &self,
        handle: LoomSession,
        workspace: String,
        name: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.tag_delete(&handle, &workspace, &name);
        async move { out }
    }

    fn tag_rename(
        &self,
        handle: LoomSession,
        workspace: String,
        old_name: String,
        new_name: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.tag_rename(&handle, &workspace, &old_name, &new_name);
        async move { out }
    }

    fn restore_file(
        &self,
        handle: LoomSession,
        workspace: String,
        rev: String,
        path: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.restore_file(&handle, &workspace, &rev, &path);
        async move { out }
    }

    fn restore_path(
        &self,
        handle: LoomSession,
        workspace: String,
        rev: String,
        prefix: String,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.restore_path(&handle, &workspace, &rev, &prefix);
        async move { out }
    }

    fn cherry_pick(
        &self,
        handle: LoomSession,
        workspace: String,
        commits: Vec<Digest>,
        dry_run: bool,
        timestamp_ms: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let commits = commits
                .iter()
                .map(digest_in)
                .collect::<Result<Vec<_>, _>>()?;
            let outcome =
                self.vcs_cherry_pick(&handle, &workspace, &commits, dry_run, timestamp_ms)?;
            loom_wire::vcs::replay_outcome_to_cbor(&outcome)
        })();
        async move { out }
    }

    fn revert(
        &self,
        handle: LoomSession,
        workspace: String,
        commits: Vec<Digest>,
        author: String,
        dry_run: bool,
        timestamp_ms: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let commits = commits
                .iter()
                .map(digest_in)
                .collect::<Result<Vec<_>, _>>()?;
            let outcome = self.vcs_revert(
                &handle,
                &workspace,
                &commits,
                &author,
                dry_run,
                timestamp_ms,
            )?;
            loom_wire::vcs::replay_outcome_to_cbor(&outcome)
        })();
        async move { out }
    }

    fn rebase(
        &self,
        handle: LoomSession,
        workspace: String,
        onto: String,
        dry_run: bool,
        timestamp_ms: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self
            .vcs_rebase(&handle, &workspace, &onto, dry_run, timestamp_ms)
            .and_then(|outcome| loom_wire::vcs::replay_outcome_to_cbor(&outcome));
        async move { out }
    }

    fn squash(
        &self,
        handle: LoomSession,
        workspace: String,
        onto: String,
        author: String,
        message: String,
        timestamp_ms: u64,
    ) -> impl ::core::future::Future<Output = Result<Digest, LoomError>> + Send {
        let out = self
            .squash(&handle, &workspace, &onto, &author, &message, timestamp_ms)
            .map(digest_out);
        async move { out }
    }
}

impl Locks for LocalLoomClient {
    #[allow(clippy::too_many_arguments)]
    fn lock_acquire(
        &self,
        key: String,
        principal: String,
        session: String,
        mode: Vec<u8>,
        permits: u32,
        capacity: u32,
        lease_ms: u64,
        // The in-process coordinator tries once and returns the contention error immediately; it never
        // queues or sleeps, so `wait_ms` is unused.
        _wait_ms: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let mode = loom_wire::lock::lock_mode_from_wire(&mode, permits, capacity)?;
            let token = self.lock_acquire(key.as_bytes(), &principal, &session, mode, lease_ms)?;
            loom_wire::lock::lock_token_to_cbor(&token)
        })();
        async move { out }
    }

    #[allow(clippy::too_many_arguments)]
    fn lock_refresh(
        &self,
        key: String,
        principal: String,
        session: String,
        mode: Vec<u8>,
        permits: u32,
        capacity: u32,
        fence_low: u64,
        fence_high: u64,
        lease_ms: u64,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let token = loom_wire::lock::lock_token_from_wire(
                key, principal, session, &mode, permits, capacity, fence_low, fence_high,
            )?;
            let updated = self.lock_refresh(&token, lease_ms)?;
            loom_wire::lock::lock_token_to_cbor(&updated)
        })();
        async move { out }
    }

    #[allow(clippy::too_many_arguments)]
    fn lock_release(
        &self,
        key: String,
        principal: String,
        session: String,
        mode: Vec<u8>,
        permits: u32,
        capacity: u32,
        fence_low: u64,
        fence_high: u64,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = (|| {
            let token = loom_wire::lock::lock_token_from_wire(
                key, principal, session, &mode, permits, capacity, fence_low, fence_high,
            )?;
            self.lock_release(&token)
        })();
        async move { out }
    }
}

impl Daemon for LocalLoomClient {
    async fn daemon_start(&self) -> Result<(), LoomError> {
        Err(daemon_unavailable("daemon_start"))
    }

    async fn daemon_stop(&self) -> Result<(), LoomError> {
        Err(daemon_unavailable("daemon_stop"))
    }

    async fn daemon_restart(&self) -> Result<(), LoomError> {
        Err(daemon_unavailable("daemon_restart"))
    }

    async fn daemon_status(&self) -> Result<Vec<u8>, LoomError> {
        Err(daemon_unavailable("daemon_status"))
    }

    async fn daemon_doctor(&self) -> Result<Vec<u8>, LoomError> {
        Err(daemon_unavailable("daemon_doctor"))
    }

    async fn daemon_session_attach(&self, _session: String) -> Result<(), LoomError> {
        Err(daemon_unavailable("daemon_session_attach"))
    }

    async fn daemon_session_detach(&self, _session: String) -> Result<(), LoomError> {
        Err(daemon_unavailable("daemon_session_detach"))
    }

    async fn daemon_pin_add(&self, _pin: String) -> Result<(), LoomError> {
        Err(daemon_unavailable("daemon_pin_add"))
    }

    async fn daemon_pin_remove(&self, _pin: String) -> Result<(), LoomError> {
        Err(daemon_unavailable("daemon_pin_remove"))
    }
}

impl Transfer for LocalLoomClient {
    fn transfer_import_open(
        &self,
        handle: LoomSession,
        workspace: String,
        kind: String,
        opts: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = self.transfer_import_open(&handle, &workspace, &kind, &opts);
        async move { out }
    }

    fn transfer_import_write(
        &self,
        handle: LoomSession,
        transfer: Vec<u8>,
        chunk: Vec<u8>,
        seq: u64,
        digest: Option<Digest>,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let digest = match digest {
                Some(d) => Some(digest_in(&d)?),
                None => None,
            };
            self.transfer_import_write(&handle, &transfer, &chunk, seq, digest.as_ref())
        })();
        async move { out }
    }

    fn transfer_import_finish(
        &self,
        handle: LoomSession,
        transfer: Vec<u8>,
        commit: bool,
        dry_run: bool,
        final_digest: Digest,
    ) -> impl ::core::future::Future<Output = Result<Vec<u8>, LoomError>> + Send {
        let out = (|| {
            let final_digest = digest_in(&final_digest)?;
            self.transfer_import_finish(&handle, &transfer, commit, dry_run, &final_digest)
        })();
        async move { out }
    }

    fn transfer_import_cancel(
        &self,
        handle: LoomSession,
        transfer: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<(), LoomError>> + Send {
        let out = self.transfer_import_cancel(&handle, &transfer);
        async move { out }
    }

    fn transfer_export(
        &self,
        handle: LoomSession,
        workspace: String,
        kind: String,
        revision: Option<String>,
        opts: Vec<u8>,
    ) -> impl ::core::future::Future<Output = Result<LoomStream<Vec<u8>>, LoomError>> + Send {
        // Export the full payload, then chunk it into a section-7 byte stream. The client
        // concatenates the chunks, writes the local destination path, and derives the content
        // digest (specs/0067 §17.4). Report-in-trailer is a follow-up requiring a carrier
        // trailer-payload extension; v1 delivers the payload bytes.
        let out = self
            .transfer_export_bytes(&handle, &workspace, &kind, revision.as_deref(), &opts)
            .map(|bytes| ready_rows(chunk_bytes(&bytes, TRANSFER_EXPORT_CHUNK_BYTES)));
        async move { out }
    }
}

impl Drive for LocalLoomClient {
    async fn drive_list_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _folder_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_list_json"))
    }

    async fn drive_stat_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _folder_id: String,
        _name: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_stat_json"))
    }

    async fn drive_read_file(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _file_id: String,
    ) -> Result<Vec<u8>, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_read_file"))
    }

    async fn drive_list_versions_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _file_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_list_versions_json"))
    }

    async fn drive_list_conflicts_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Drive.drive_list_conflicts_json",
        ))
    }

    async fn drive_list_shares_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_list_shares_json"))
    }

    async fn drive_list_retention_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Drive.drive_list_retention_json",
        ))
    }

    async fn drive_create_folder_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _parent_folder_id: String,
        _folder_id: String,
        _name: String,
        _expected_root: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_create_folder_json"))
    }

    async fn drive_create_upload_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _upload_id: String,
        _parent_folder_id: String,
        _name: String,
        _file_id: String,
        _expected_root: String,
        _created_at_ms: u64,
        _replace_file: bool,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_create_upload_json"))
    }

    async fn drive_upload_chunk_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _upload_id: String,
        _chunk: Vec<u8>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_upload_chunk_json"))
    }

    async fn drive_commit_upload_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _upload_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_commit_upload_json"))
    }

    async fn drive_rename_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _folder_id: String,
        _node_id: String,
        _new_name: String,
        _expected_root: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_rename_json"))
    }

    async fn drive_move_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _source_folder_id: String,
        _target_folder_id: String,
        _node_id: String,
        _expected_root: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_move_json"))
    }

    async fn drive_delete_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _folder_id: String,
        _node_id: String,
        _expected_root: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_delete_json"))
    }

    async fn drive_resolve_conflict_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _conflict_id: String,
        _resolution: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Drive.drive_resolve_conflict_json",
        ))
    }

    async fn drive_grant_share_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _grant_id: String,
        _target_kind: String,
        _target_id: String,
        _principal: String,
        _role: String,
        _granted_at_ms: u64,
        _expires_at_ms: Option<u64>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_grant_share_json"))
    }

    async fn drive_revoke_share_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _grant_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_revoke_share_json"))
    }

    async fn drive_apply_share_expiry_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _now_ms: u64,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Drive.drive_apply_share_expiry_json",
        ))
    }

    async fn drive_pin_retention_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _pin_id: String,
        _kind: String,
        _root: String,
        _target_entity_id: Option<String>,
        _added_at_ms: u64,
        _expires_at_ms: Option<u64>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Drive.drive_pin_retention_json"))
    }

    async fn drive_unpin_retention_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _pin_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Drive.drive_unpin_retention_json",
        ))
    }

    async fn drive_apply_retention_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _drive_workspace_id: String,
        _now_ms: u64,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Drive.drive_apply_retention_json",
        ))
    }
}

impl Tickets for LocalLoomClient {
    async fn tickets_project_create_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _project_id: String,
        _key_prefix: String,
        _name: String,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Tickets.tickets_project_create_json",
        ))
    }

    async fn tickets_project_rekey_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _project_id: String,
        _key_prefix: String,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Tickets.tickets_project_rekey_json",
        ))
    }

    async fn tickets_project_settings_get_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _project_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Tickets.tickets_project_settings_get_json",
        ))
    }

    async fn tickets_fields_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _project_id: Option<String>,
        _projection: Option<String>,
        _operation: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Tickets.tickets_fields_json"))
    }

    async fn tickets_create_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _project_id: String,
        _ticket_type: String,
        _external_source: Option<String>,
        _external_id: Option<String>,
        _fields_json: String,
        _policy_labels_json: String,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Tickets.tickets_create_json"))
    }

    async fn tickets_update_json(
        &self,
        handle: LoomSession,
        workspace: String,
        ticket_workspace_id: String,
        ticket_id: String,
        set_fields_json: Option<String>,
        delete_fields_json: String,
        action: Option<String>,
        target_status: Option<String>,
        observed_source_status: Option<String>,
        observed_workflow_version: Option<String>,
        assignee: Option<String>,
        comment_id: Option<String>,
        comment_type: Option<String>,
        comment_body: Option<String>,
        expected_root: Option<String>,
        comments_json: Option<String>,
        relation_sets_json: Option<String>,
        relation_removes_json: Option<String>,
    ) -> Result<String, LoomError> {
        if comment_body.is_none() && (comment_id.is_some() || comment_type.is_some()) {
            return Err(LoomError::invalid(
                "ticket update comment id and type require comment body",
            ));
        }
        let set_fields = set_fields_json
            .as_deref()
            .map(|value| {
                serde_json::from_str(value).map_err(|err| {
                    LoomError::new(
                        Code::InvalidArgument,
                        format!("ticket set fields json: {err}"),
                    )
                })
            })
            .transpose()?;
        let delete_fields =
            parse_string_list_json(&delete_fields_json, "ticket delete fields json")?;
        let action_applied = action.is_some();
        let action = parse_ticket_lifecycle_action(action.as_deref())?;
        let comments_input = parse_optional_json_list::<ServiceTicketUpdateComment>(
            comments_json.as_deref(),
            "ticket comments json",
        )?;
        let relation_sets_input = parse_optional_json_list::<ServiceTicketUpdateRelationSet>(
            relation_sets_json.as_deref(),
            "ticket relation sets json",
        )?;
        let relation_removes_input = parse_optional_json_list::<ServiceTicketUpdateRelationRemove>(
            relation_removes_json.as_deref(),
            "ticket relation removes json",
        )?;
        let relation_kinds = relation_sets_input
            .iter()
            .map(|relation| loom_tickets::TicketRelationKind::parse(&relation.kind))
            .collect::<Result<Vec<_>, _>>()?;
        let changes = ticket_update_changes(
            set_fields.as_ref(),
            &delete_fields,
            action_applied,
            target_status.as_deref(),
            observed_source_status.as_deref(),
            assignee.as_deref(),
            comment_type
                .as_ref()
                .map(|value| Some(value.clone()))
                .into_iter()
                .chain(
                    comments_input
                        .iter()
                        .map(|comment| comment.comment_type.clone()),
                ),
            relation_sets_input.iter().map(|relation| {
                (
                    relation
                        .relation_id
                        .clone()
                        .unwrap_or_else(|| relation.target_id.clone()),
                    relation.kind.clone(),
                    relation.target_id.clone(),
                )
            }),
            relation_removes_input
                .iter()
                .map(|relation| relation.relation_id.clone()),
        );
        let comment =
            comment_body
                .as_deref()
                .map(|body| loom_tickets::TicketUpdateCommentRequest {
                    comment_id: comment_id.as_deref(),
                    comment_type: comment_type.as_deref(),
                    body,
                    evidence: None,
                });
        let comments = comments_input
            .iter()
            .map(|comment| loom_tickets::TicketUpdateCommentRequest {
                comment_id: comment.comment_id.as_deref(),
                comment_type: comment.comment_type.as_deref(),
                body: &comment.body,
                evidence: None,
            })
            .collect::<Vec<_>>();
        let relation_sets = relation_sets_input
            .iter()
            .zip(relation_kinds.iter())
            .map(
                |(relation, kind)| loom_tickets::TicketUpdateRelationSetRequest {
                    relation_id: relation.relation_id.as_deref(),
                    kind: *kind,
                    target_id: &relation.target_id,
                },
            )
            .collect::<Vec<_>>();
        let relation_removes = relation_removes_input
            .iter()
            .map(|relation| loom_tickets::TicketUpdateRelationRemoveRequest {
                relation_id: &relation.relation_id,
            })
            .collect::<Vec<_>>();
        self.with_session(&handle, |loom| {
            let ns = loom.registry().open(&service_ns_selector(&workspace))?;
            let ticket = loom_tickets::update_ticket(
                loom,
                ns,
                loom_tickets::TicketUpdateRequest {
                    workspace_id: &ticket_workspace_id,
                    ticket_id: &ticket_id,
                    set_fields: set_fields.as_ref(),
                    delete_fields: &delete_fields,
                    action,
                    target_status: target_status.as_deref(),
                    observed_source_status: observed_source_status.as_deref(),
                    observed_workflow_version: observed_workflow_version.as_deref(),
                    assignee: assignee.as_deref(),
                    expected_root: expected_root.as_deref(),
                    comment,
                    comments: &comments,
                    relation_sets: &relation_sets,
                    relation_removes: &relation_removes,
                },
            )?;
            sync_ticket_references(loom, ns, &ticket)?;
            let result =
                ticket_mutation_json(ticket, "ticket.updated", expected_root.as_deref(), changes)?;
            save_loom(loom)?;
            Ok(result)
        })
    }

    async fn tickets_delete_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _ticket_id: String,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Tickets.tickets_delete_json"))
    }

    async fn tickets_comments_json(
        &self,
        handle: LoomSession,
        workspace: String,
        ticket_workspace_id: String,
        ticket_id: String,
    ) -> Result<String, LoomError> {
        self.with_session(&handle, |loom| {
            let ns = loom.registry().open(&service_ns_selector(&workspace))?;
            let comments =
                loom_tickets::list_ticket_comments(loom, ns, &ticket_workspace_id, &ticket_id)?;
            json_string(&comments)
        })
    }

    async fn tickets_comment_add_json(
        &self,
        handle: LoomSession,
        workspace: String,
        ticket_workspace_id: String,
        ticket_id: String,
        comment_id: Option<String>,
        comment_type: Option<String>,
        body: String,
        expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        let mut changes = vec![MutationChange::field_set(
            "comment_type",
            comment_type
                .as_deref()
                .unwrap_or(loom_tickets::TICKET_DEFAULT_COMMENT_TYPE),
        )];
        if let Some(comment_id) = comment_id.as_deref() {
            changes.push(MutationChange::field_set("comment_id", comment_id));
        }
        self.with_session(&handle, |loom| {
            let ns = loom.registry().open(&service_ns_selector(&workspace))?;
            let ticket = loom_tickets::add_ticket_comment(
                loom,
                ns,
                loom_tickets::TicketCommentRequest {
                    workspace_id: &ticket_workspace_id,
                    ticket_id: &ticket_id,
                    comment_id: comment_id.as_deref(),
                    comment_type: comment_type.as_deref(),
                    body: &body,
                    evidence: None,
                    expected_root: expected_root.as_deref(),
                },
            )?;
            sync_ticket_references(loom, ns, &ticket)?;
            let result = ticket_mutation_json(
                ticket,
                "ticket.comment_added",
                expected_root.as_deref(),
                changes,
            )?;
            save_loom(loom)?;
            Ok(result)
        })
    }

    async fn tickets_comment_update_json(
        &self,
        handle: LoomSession,
        workspace: String,
        ticket_workspace_id: String,
        ticket_id: String,
        comment_id: String,
        comment_type: Option<String>,
        body: Option<String>,
        expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        let mut changes = vec![MutationChange::field_set("comment_id", &comment_id)];
        if let Some(comment_type) = comment_type.as_deref() {
            changes.push(MutationChange::field_set("comment_type", comment_type));
        }
        if body.is_some() {
            changes.push(MutationChange::field_set("body", "updated"));
        }
        self.with_session(&handle, |loom| {
            let ns = loom.registry().open(&service_ns_selector(&workspace))?;
            let ticket = loom_tickets::update_ticket_comment(
                loom,
                ns,
                loom_tickets::TicketCommentUpdateRequest {
                    workspace_id: &ticket_workspace_id,
                    ticket_id: &ticket_id,
                    comment_id: &comment_id,
                    comment_type: comment_type.as_deref(),
                    body: body.as_deref(),
                    evidence: None,
                    expected_root: expected_root.as_deref(),
                },
            )?;
            sync_ticket_references(loom, ns, &ticket)?;
            let result = ticket_mutation_json(
                ticket,
                "ticket.comment_updated",
                expected_root.as_deref(),
                changes,
            )?;
            save_loom(loom)?;
            Ok(result)
        })
    }

    async fn tickets_comment_delete_json(
        &self,
        handle: LoomSession,
        workspace: String,
        ticket_workspace_id: String,
        ticket_id: String,
        comment_id: String,
        expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        self.with_session(&handle, |loom| {
            let ns = loom.registry().open(&service_ns_selector(&workspace))?;
            let ticket = loom_tickets::delete_ticket_comment(
                loom,
                ns,
                loom_tickets::TicketCommentDeleteRequest {
                    workspace_id: &ticket_workspace_id,
                    ticket_id: &ticket_id,
                    comment_id: &comment_id,
                    expected_root: expected_root.as_deref(),
                },
            )?;
            sync_ticket_references(loom, ns, &ticket)?;
            let result = ticket_mutation_json(
                ticket,
                "ticket.comment_deleted",
                expected_root.as_deref(),
                vec![MutationChange::field_deleted(
                    "comment",
                    Some(comment_id.to_string()),
                )],
            )?;
            save_loom(loom)?;
            Ok(result)
        })
    }

    async fn tickets_relation_set_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _ticket_id: String,
        _relation_id: Option<String>,
        _kind: String,
        _target_id: String,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Tickets.tickets_relation_set_json",
        ))
    }

    async fn tickets_relation_remove_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _ticket_id: String,
        _relation_id: String,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Tickets.tickets_relation_remove_json",
        ))
    }

    async fn tickets_relation_list_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _ticket_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Tickets.tickets_relation_list_json",
        ))
    }

    async fn tickets_get_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _ticket_id: String,
        _projection: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Tickets.tickets_get_json"))
    }

    async fn tickets_list_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _projection: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Tickets.tickets_list_json"))
    }

    async fn tickets_history_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _ticket_id: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Tickets.tickets_history_json"))
    }

    async fn boards_create_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _request_json: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Tickets.boards_create_json"))
    }

    async fn boards_get_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _board_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Tickets.boards_get_json"))
    }

    async fn boards_list_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _include_deleted: bool,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Tickets.boards_list_json"))
    }

    async fn boards_update_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _board_id: String,
        _request_json: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Tickets.boards_update_json"))
    }

    async fn boards_delete_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _board_id: String,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Tickets.boards_delete_json"))
    }

    async fn boards_configure_columns_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _board_id: String,
        _request_json: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Tickets.boards_configure_columns_json",
        ))
    }

    async fn boards_move_card_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _board_id: String,
        _request_json: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Tickets.boards_move_card_json"))
    }

    async fn tickets_project_settings_set_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _project_id: String,
        _default_projection: Option<String>,
        _enable_projections_json: String,
        _disable_projections_json: String,
        _actor_enforcement: Option<String>,
        _project_owner_principal: Option<String>,
        _clear_project_owner_principal: bool,
        _acceptance_authorities_json: Option<String>,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Tickets.tickets_project_settings_set_json",
        ))
    }

    async fn tickets_field_put_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _project_id: String,
        _field_id: String,
        _key: String,
        _name: String,
        _description: Option<String>,
        _field_type: String,
        _option_set: Option<String>,
        _max_length: u32,
        _has_max_length: bool,
        _required: bool,
        _searchable: bool,
        _orderable: bool,
        _cardinality: String,
        _applicable_type_ids_json: String,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Tickets.tickets_field_put_json"))
    }

    async fn tickets_field_retire_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _ticket_workspace_id: String,
        _project_id: String,
        _field_id: String,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Tickets.tickets_field_retire_json",
        ))
    }
}

impl Pages for LocalLoomClient {
    async fn spaces_create_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
        _space_id: String,
        _title: String,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Pages.spaces_create_json"))
    }

    async fn spaces_list_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Pages.spaces_list_json"))
    }

    async fn spaces_get_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
        _space_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Pages.spaces_get_json"))
    }

    async fn pages_create_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
        _page_id: String,
        _space_id: String,
        _parent_page_id: Option<String>,
        _title: String,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Pages.pages_create_json"))
    }

    async fn pages_update_json(
        &self,
        handle: LoomSession,
        workspace: String,
        page_workspace_id: String,
        page_id: String,
        body_text: String,
        expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        self.with_session(&handle, |loom| {
            let ns = loom.registry().open(&service_ns_selector(&workspace))?;
            let summary = loom_pages::update_page_text(
                loom,
                ns,
                &page_workspace_id,
                &page_id,
                &body_text,
                now_ms(),
                expected_root.as_deref(),
            )?;
            save_loom(loom)?;
            json_string(&summary)
        })
    }

    async fn pages_publish_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
        _page_id: String,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Pages.pages_publish_json"))
    }

    async fn pages_get_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
        _page_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Pages.pages_get_json"))
    }

    async fn pages_list_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Pages.pages_list_json"))
    }

    async fn pages_history_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
        _page_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Pages.pages_history_json"))
    }

    async fn structures_create_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
        _structure_id: String,
        _space_id: String,
        _kind: String,
        _title: String,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Pages.structures_create_json"))
    }

    async fn structures_add_node_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
        _structure_id: String,
        _node_id: String,
        _kind: String,
        _label: String,
        _body_digest: Option<String>,
        _entity_ref: Option<String>,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Pages.structures_add_node_json"))
    }

    async fn structures_update_node_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
        _structure_id: String,
        _node_id: String,
        _kind: String,
        _label: String,
        _body_digest: Option<String>,
        _entity_ref: Option<String>,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Pages.structures_update_node_json",
        ))
    }

    async fn structures_bind_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
        _structure_id: String,
        _node_id: String,
        _entity_ref: Option<String>,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Pages.structures_bind_json"))
    }

    async fn structures_move_node_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
        _structure_id: String,
        _node_id: String,
        _parent_node_id: Option<String>,
        _label: Option<String>,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Pages.structures_move_node_json",
        ))
    }

    async fn structures_link_node_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
        _structure_id: String,
        _edge_id: String,
        _src_node_id: String,
        _dst_node_id: String,
        _label: String,
        _target_ref: Option<String>,
        _expected_root: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Pages.structures_link_node_json",
        ))
    }

    async fn structures_decompose_to_tickets_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
        _structure_id: String,
        _items_json: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Pages.structures_decompose_to_tickets_json",
        ))
    }

    async fn structures_get_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
        _structure_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Pages.structures_get_json"))
    }

    async fn structures_list_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _page_workspace_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Pages.structures_list_json"))
    }
}

impl Meetings for LocalLoomClient {
    async fn meetings_import_snapshot(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _input_profile: String,
        _snapshot: Vec<u8>,
        _dry_run: bool,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Meetings.meetings_import_snapshot",
        ))
    }

    async fn meetings_source_read(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _source_id: String,
        _leaf: String,
    ) -> Result<Vec<u8>, LoomError> {
        Err(idl_contract_unimplemented("Meetings.meetings_source_read"))
    }
}

impl StudioSurfaces for LocalLoomClient {
    async fn studio_surface_catalog_json(
        &self,
        _workspace: String,
        _set: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "StudioSurfaces.studio_surface_catalog_json",
        ))
    }
}

impl StoreAdmin for LocalLoomClient {
    async fn store_stat(&self, handle: LoomSession) -> Result<Vec<u8>, LoomError> {
        self.store_stat(&handle)
    }

    async fn store_policy_get(&self, handle: LoomSession) -> Result<Vec<u8>, LoomError> {
        self.store_policy_get(&handle)
    }

    async fn store_policy_set(
        &self,
        handle: LoomSession,
        fips_required: bool,
    ) -> Result<Vec<u8>, LoomError> {
        self.store_policy_set(&handle, fips_required)
    }

    async fn store_rekey(
        &self,
        handle: LoomSession,
        new_passphrase: Vec<u8>,
        reseal: bool,
        suite: Option<String>,
    ) -> Result<Vec<u8>, LoomError> {
        // All key material is generated server-side: the client never handles the DEK, salt, or nonce.
        let mut salt = [0u8; 16];
        let mut wrap_nonce = [0u8; 24];
        random_bytes(&mut salt)?;
        random_bytes(&mut wrap_nonce)?;
        let new_dek = if reseal {
            let mut dek = [0u8; KEY_LEN];
            random_bytes(&mut dek)?;
            Some(dek)
        } else {
            None
        };
        self.store_rekey(
            &handle,
            &new_passphrase,
            reseal,
            suite.as_deref(),
            salt.to_vec(),
            wrap_nonce.to_vec(),
            new_dek,
        )
    }
}

impl Chat for LocalLoomClient {
    async fn chat_create_channel_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _channel_id: String,
        _channel_handle: String,
        _name: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_create_channel_json"))
    }

    async fn chat_rename_channel_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _selector: String,
        _channel_handle: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_rename_channel_json"))
    }

    async fn chat_list_channels_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_list_channels_json"))
    }

    async fn chat_post_message_json(
        &self,
        handle: LoomSession,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        message_id: String,
        thread_id: Option<String>,
        body_text: String,
    ) -> Result<String, LoomError> {
        self.with_session(&handle, |loom| {
            let ns = loom.registry().open(&service_ns_selector(&workspace))?;
            let summary = loom_chat::post_message(
                loom,
                ns,
                &chat_workspace_id,
                &channel_id,
                &message_id,
                thread_id.as_deref(),
                body_text.into_bytes(),
            )?;
            save_loom(loom)?;
            json_string(&summary)
        })
    }

    async fn chat_edit_message_json(
        &self,
        handle: LoomSession,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        message_id: String,
        body_text: String,
    ) -> Result<String, LoomError> {
        self.with_session(&handle, |loom| {
            let ns = loom.registry().open(&service_ns_selector(&workspace))?;
            let summary = loom_chat::edit_message(
                loom,
                ns,
                &chat_workspace_id,
                &channel_id,
                &message_id,
                body_text.into_bytes(),
            )?;
            save_loom(loom)?;
            json_string(&summary)
        })
    }

    async fn chat_redact_message_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _channel_id: String,
        _message_id: String,
        _reason: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_redact_message_json"))
    }

    async fn chat_create_thread_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _channel_id: String,
        _thread_id: String,
        _parent_message_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_create_thread_json"))
    }

    async fn chat_create_task_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _channel_id: String,
        _task_id: String,
        _message_id: Option<String>,
        _title: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_create_task_json"))
    }

    async fn chat_claim_task_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _channel_id: String,
        _task_id: String,
        _claim_id: String,
        _lease_token: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_claim_task_json"))
    }

    async fn chat_complete_task_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _channel_id: String,
        _task_id: String,
        _claim_id: String,
        _result_message_id: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_complete_task_json"))
    }

    async fn chat_invoke_agent_json(
        &self,
        handle: LoomSession,
        workspace: String,
        chat_workspace_id: String,
        channel_id: String,
        invocation_id: String,
        agent_principal: String,
        source_message_ids_json: String,
        prompt_text: String,
    ) -> Result<String, LoomError> {
        let agent_principal = parse_service_workspace_id(&agent_principal, "agent_principal")?;
        let source_message_ids =
            parse_string_list_json(&source_message_ids_json, "source_message_ids_json")?;
        self.with_session(&handle, |loom| {
            let ns = loom.registry().open(&service_ns_selector(&workspace))?;
            let summary = loom_chat::invoke_agent(
                loom,
                ns,
                &chat_workspace_id,
                &channel_id,
                &invocation_id,
                agent_principal,
                source_message_ids,
                prompt_text.into_bytes(),
            )?;
            save_loom(loom)?;
            json_string(&summary)
        })
    }

    async fn chat_agent_reply_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _channel_id: String,
        _invocation_id: String,
        _message_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_agent_reply_json"))
    }

    async fn chat_request_handoff_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _channel_id: String,
        _handoff_id: String,
        _from_agent_principal: String,
        _to_principal: Option<String>,
        _reason: Option<String>,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_request_handoff_json"))
    }

    async fn chat_add_reaction_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _channel_id: String,
        _message_id: String,
        _kind: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_add_reaction_json"))
    }

    async fn chat_remove_reaction_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _channel_id: String,
        _message_id: String,
        _kind: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_remove_reaction_json"))
    }

    async fn chat_emoji_list_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_emoji_list_json"))
    }

    async fn chat_emoji_register_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _kind: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_emoji_register_json"))
    }

    async fn chat_emoji_unregister_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _kind: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented(
            "Chat.chat_emoji_unregister_json",
        ))
    }

    async fn chat_messages_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _channel_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_messages_json"))
    }

    async fn chat_cursor_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _channel_id: String,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_cursor_json"))
    }

    async fn chat_update_cursor_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _channel_id: String,
        _next_sequence: u64,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_update_cursor_json"))
    }

    async fn chat_fetch_events_json(
        &self,
        _handle: LoomSession,
        _workspace: String,
        _chat_workspace_id: String,
        _channel_id: String,
        _from_sequence: u64,
        _max: u64,
    ) -> Result<String, LoomError> {
        Err(idl_contract_unimplemented("Chat.chat_fetch_events_json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::FacetKind;

    fn block<T>(
        fut: impl ::core::future::Future<Output = Result<T, LoomError>>,
    ) -> Result<T, LoomError> {
        let mut fut = ::std::pin::pin!(fut);
        match fut.as_mut().poll(&mut ::core::task::Context::from_waker(
            ::std::task::Waker::noop(),
        )) {
            ::core::task::Poll::Ready(output) => output,
            ::core::task::Poll::Pending => Err(LoomError::new(
                Code::Internal,
                "in-process future returned Pending",
            )),
        }
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("loom-client-service-{}-{tag}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn seed_client(
        tag: &str,
    ) -> (
        LocalLoomClient,
        LoomSession,
        WorkspaceId,
        std::path::PathBuf,
    ) {
        let dir = temp_dir(tag);
        let client = LocalLoomClient::new(dir.join("t.loom"));
        client.create().expect("create store");
        let session = LocalLoomClient::open(&client).expect("open");
        let workspace = client
            .workspace_create(&session, Some("repo"), Some(FacetKind::Document))
            .expect("workspace");
        (client, session, workspace, dir)
    }

    #[test]
    fn pages_update_json_uses_string_body_text() {
        let (client, session, workspace, dir) = seed_client("pages-json");
        client
            .with_session(&session, |loom| {
                let space =
                    loom_pages::create_space(loom, workspace, "studio", "eng", "Eng", None)?;
                loom_pages::create_page(
                    loom,
                    workspace,
                    loom_pages::PageCreateRequest {
                        workspace_id: "studio",
                        page_id: "page-1",
                        space_id: "eng",
                        parent_page_id: None,
                        title: "Roadmap",
                        expected_root: Some(&space.profile_root),
                    },
                )?;
                save_loom(loom)
            })
            .expect("seed page");

        let out = block(<LocalLoomClient as Pages>::pages_update_json(
            &client,
            session.clone(),
            "repo".to_string(),
            "studio".to_string(),
            "page-1".to_string(),
            "plain text body".to_string(),
            None,
        ))
        .expect("update page");
        let value: serde_json::Value = serde_json::from_str(&out).expect("json");
        assert_eq!(value["workspace_id"], "studio");
        assert_eq!(value["page_id"], "page-1");
        assert_eq!(value["status"], "draft");

        client
            .with_session(&session, |loom| {
                let page =
                    loom_pages::get_page(loom, workspace, "studio", "page-1")?.expect("page");
                assert_eq!(page.draft_body_text.as_deref(), Some("plain text body\n"));
                Ok(())
            })
            .expect("read page");
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tickets_comment_json_methods_roundtrip_locally() {
        let (client, session, workspace, dir) = seed_client("tickets-comments-json");
        let workspace_id = workspace.to_string();
        let ticket = client
            .with_session(&session, |loom| {
                loom_tickets::create_project(
                    loom,
                    workspace,
                    &workspace_id,
                    "matrix",
                    "MX",
                    "Matrix",
                    None,
                )?;
                let ticket = loom_tickets::create_ticket(
                    loom,
                    workspace,
                    loom_tickets::TicketCreateRequest {
                        workspace_id: &workspace_id,
                        project_id: "matrix",
                        ticket_type: "task",
                        external_source: None,
                        external_id: None,
                        fields: &serde_json::json!({"status": "open"}),
                        policy_labels: &[],
                        expected_root: None,
                    },
                )?;
                save_loom(loom)?;
                Ok(ticket)
            })
            .expect("seed ticket");

        let add = block(<LocalLoomClient as Tickets>::tickets_comment_add_json(
            &client,
            session.clone(),
            "repo".to_string(),
            workspace_id.clone(),
            ticket.ticket_id.clone(),
            Some("c1".to_string()),
            Some("review_request".to_string()),
            "Ready for review".to_string(),
            Some(ticket.profile_root.clone()),
        ))
        .expect("add comment");
        let add: serde_json::Value = serde_json::from_str(&add).expect("add json");
        assert_eq!(add["receipt"]["operation"], "ticket.comment_added");
        assert_eq!(add["resource"]["primary_key"], ticket.primary_key);
        let add_root = add["resource"]["profile_root"].as_str().expect("add root");

        let comments = block(<LocalLoomClient as Tickets>::tickets_comments_json(
            &client,
            session.clone(),
            "repo".to_string(),
            workspace_id.clone(),
            ticket.ticket_id.clone(),
        ))
        .expect("list comments");
        let comments: serde_json::Value = serde_json::from_str(&comments).expect("comments json");
        assert_eq!(comments.as_array().expect("comments").len(), 1);
        assert_eq!(comments[0]["comment_id"], "c1");
        assert_eq!(comments[0]["comment_type"], "review_request");
        assert_eq!(comments[0]["body"], "Ready for review");

        let update = block(<LocalLoomClient as Tickets>::tickets_comment_update_json(
            &client,
            session.clone(),
            "repo".to_string(),
            workspace_id.clone(),
            ticket.ticket_id.clone(),
            "c1".to_string(),
            Some("review_feedback".to_string()),
            Some("Needs evidence".to_string()),
            Some(add_root.to_string()),
        ))
        .expect("update comment");
        let update: serde_json::Value = serde_json::from_str(&update).expect("update json");
        assert_eq!(update["receipt"]["operation"], "ticket.comment_updated");
        let update_root = update["resource"]["profile_root"]
            .as_str()
            .expect("update root");

        let delete = block(<LocalLoomClient as Tickets>::tickets_comment_delete_json(
            &client,
            session.clone(),
            "repo".to_string(),
            workspace_id.clone(),
            ticket.ticket_id.clone(),
            "c1".to_string(),
            Some(update_root.to_string()),
        ))
        .expect("delete comment");
        let delete: serde_json::Value = serde_json::from_str(&delete).expect("delete json");
        assert_eq!(delete["receipt"]["operation"], "ticket.comment_deleted");

        let comments = block(<LocalLoomClient as Tickets>::tickets_comments_json(
            &client,
            session.clone(),
            "repo".to_string(),
            workspace_id,
            ticket.ticket_id,
        ))
        .expect("list deleted comments");
        let comments: serde_json::Value = serde_json::from_str(&comments).expect("deleted json");
        assert_eq!(comments[0]["comment_type"], "review_feedback");
        assert_eq!(comments[0]["body"], "");
        assert_eq!(comments[0]["redacted"], true);

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn tickets_update_json_composes_fields_status_comments_and_relations_locally() {
        let (client, session, workspace, dir) = seed_client("tickets-update-json");
        let workspace_id = workspace.to_string();
        let (source, target) = client
            .with_session(&session, |loom| {
                loom_tickets::create_project(
                    loom,
                    workspace,
                    &workspace_id,
                    "matrix",
                    "MX",
                    "Matrix",
                    None,
                )?;
                let source = loom_tickets::create_ticket(
                    loom,
                    workspace,
                    loom_tickets::TicketCreateRequest {
                        workspace_id: &workspace_id,
                        project_id: "matrix",
                        ticket_type: "task",
                        external_source: None,
                        external_id: None,
                        fields: &serde_json::json!({"status": "planned", "priority": "P2"}),
                        policy_labels: &[],
                        expected_root: None,
                    },
                )?;
                let target = loom_tickets::create_ticket(
                    loom,
                    workspace,
                    loom_tickets::TicketCreateRequest {
                        workspace_id: &workspace_id,
                        project_id: "matrix",
                        ticket_type: "task",
                        external_source: None,
                        external_id: None,
                        fields: &serde_json::json!({"status": "planned"}),
                        policy_labels: &[],
                        expected_root: Some(&source.profile_root),
                    },
                )?;
                save_loom(loom)?;
                Ok((source, target))
            })
            .expect("seed tickets");

        let update = block(<LocalLoomClient as Tickets>::tickets_update_json(
            &client,
            session.clone(),
            "repo".to_string(),
            workspace_id.clone(),
            source.ticket_id.clone(),
            Some(serde_json::json!({"priority": "P1"}).to_string()),
            "[]".to_string(),
            None,
            Some("blocked".to_string()),
            Some("planned".to_string()),
            None,
            None,
            Some("single-comment".to_string()),
            Some("blocker".to_string()),
            Some("Blocked on dependency".to_string()),
            Some(target.profile_root.clone()),
            Some(
                serde_json::json!([
                    {"comment_id": "array-comment", "comment_type": "progress", "body": "Investigated root cause"}
                ])
                .to_string(),
            ),
            Some(
                serde_json::json!([
                    {"relation_id": "dependency", "kind": "depends_on", "target_id": target.ticket_id}
                ])
                .to_string(),
            ),
            None,
        ))
        .expect("update ticket");
        let update: serde_json::Value = serde_json::from_str(&update).expect("update json");
        assert_eq!(update["receipt"]["operation"], "ticket.updated");
        assert_eq!(update["resource"]["fields"]["status"], "blocked");
        assert_eq!(update["resource"]["fields"]["priority"], "P1");
        assert_eq!(update["resource"]["comments"].as_array().unwrap().len(), 2);
        assert_eq!(
            update["resource"]["relations"][0]["relation_id"],
            "dependency"
        );
        assert_eq!(update["resource"]["relations"][0]["kind"], "depends_on");
        let update_root = update["resource"]["profile_root"]
            .as_str()
            .expect("update root");

        let remove = block(<LocalLoomClient as Tickets>::tickets_update_json(
            &client,
            session.clone(),
            "repo".to_string(),
            workspace_id.clone(),
            source.ticket_id.clone(),
            None,
            "[]".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(update_root.to_string()),
            None,
            None,
            Some(serde_json::json!([{"relation_id": "dependency"}]).to_string()),
        ))
        .expect("remove relation");
        let remove: serde_json::Value = serde_json::from_str(&remove).expect("remove json");
        assert_eq!(remove["resource"]["relations"].as_array().unwrap().len(), 0);

        let comments = block(<LocalLoomClient as Tickets>::tickets_comments_json(
            &client,
            session.clone(),
            "repo".to_string(),
            workspace_id,
            source.ticket_id,
        ))
        .expect("list comments");
        let comments: serde_json::Value = serde_json::from_str(&comments).expect("comments json");
        assert_eq!(comments.as_array().expect("comments").len(), 2);

        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn chat_json_methods_use_string_body_and_prompt_text() {
        let (client, session, workspace, dir) = seed_client("chat-json");
        let channel_id = WorkspaceId::from_bytes([9; 16]);
        client
            .with_session(&session, |loom| {
                loom_chat::ensure_channel(
                    loom, workspace, "studio", channel_id, "general", "General",
                )?;
                save_loom(loom)
            })
            .expect("seed channel");

        block(<LocalLoomClient as Chat>::chat_post_message_json(
            &client,
            session.clone(),
            "repo".to_string(),
            "studio".to_string(),
            "general".to_string(),
            "m1".to_string(),
            None,
            "hello".to_string(),
        ))
        .expect("post message");
        block(<LocalLoomClient as Chat>::chat_edit_message_json(
            &client,
            session.clone(),
            "repo".to_string(),
            "studio".to_string(),
            "general".to_string(),
            "m1".to_string(),
            "edited".to_string(),
        ))
        .expect("edit message");
        block(<LocalLoomClient as Chat>::chat_invoke_agent_json(
            &client,
            session.clone(),
            "repo".to_string(),
            "studio".to_string(),
            "general".to_string(),
            "inv-1".to_string(),
            WorkspaceId::from_bytes([7; 16]).to_string(),
            "[\"m1\"]".to_string(),
            "summarize".to_string(),
        ))
        .expect("invoke agent");

        client
            .with_session(&session, |loom| {
                let channel = loom_chat::channel_projection(loom, workspace, "studio", "general")?;
                assert_eq!(channel.messages.len(), 1);
                assert_eq!(channel.messages[0].body, b"edited");
                assert_eq!(channel.agent_invocations.len(), 1);
                assert_eq!(channel.agent_invocations[0].source_message_ids, ["m1"]);
                assert_eq!(channel.agent_invocations[0].prompt, b"summarize");
                Ok(())
            })
            .expect("read channel");
        client.close(&session);
        std::fs::remove_dir_all(&dir).ok();
    }
}

// `LocalLoomClient` satisfies every generated interface trait, so it is a complete `LoomClient`.
impl LoomClient for LocalLoomClient {}
