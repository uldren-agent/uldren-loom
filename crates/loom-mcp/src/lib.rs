//! Model Context Protocol host for Uldren Loom.
//!
//! This crate is a host process, not a language binding and not wasm-embedded. The default build is the
//! transport-agnostic engine surface: [`StoreAccess`] and [`LoomMcp`]. The `server` feature adds the
//! `rmcp` stdio host (see [`server`]) used by the `loom mcp` CLI command.
//!
//! A default passwordless loom resolves the caller to the owner with full read/write, so this host adds
//! no auth of its own. Once identity is enforced, the engine routes each call through
//! `AclStore::authorize`; the host never reimplements policy.
//!
//! Licensed under BUSL-1.1.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use loom_coordination::with_local_store_write_lock;
use loom_core::error::{Code, LoomError, Result};
use loom_core::keys::KeySpec;
use loom_core::{
    AclDomain, AclResource, AclResourceScope, AclRight, AclScopeKind, Algo, Digest, FacetKind,
    KvTier, Loom, ObjectStore, WorkspaceId,
};
use loom_store::{
    FileStore, LocalOpenAuth, attach_local_auth, daemon, local_auth_requires_write,
    open_loom_daemon_authorized_unlocked, open_loom_read_unlocked,
    open_loom_registry_read_unlocked, open_loom_unlocked, save_loom,
};

pub mod apps;
mod chat;
mod drive;
mod facet_cbor;
mod meetings;
pub(crate) use loom_pages as pages;
pub mod prompts;
pub mod reads;
pub mod resources;
#[cfg(feature = "server")]
pub mod server;
mod substrate_refs;
mod substrate_revisions;
mod substrate_views;
pub mod tool_titles;
pub mod tools;
pub mod writes;

/// The capability this host owns and contributes: serving a loom over MCP. The engine registry contains
/// `mcp-host` as unavailable because `loom-core` does not serve MCP; this host overlays it as available.
pub const MCP_HOST_CAPABILITY: &str = "mcp-host";
/// The capability this host owns and contributes for the MCP Apps extension surface.
pub const MCP_APPS_CAPABILITY: &str = "mcp-apps";
/// The pull-watch capability projected by this MCP host.
pub const WATCH_CAPABILITY: &str = "watch";

pub(crate) fn authorize_workgraph_task(
    loom: &Loom<FileStore>,
    workspace: WorkspaceId,
    task_id: &str,
    right: AclRight,
) -> Result<()> {
    let target = format!("workgraph:{task_id}");
    loom.authorize_resource(
        AclResource::scoped(
            workspace,
            AclDomain::Tickets,
            None,
            AclResourceScope::Prefix {
                kind: AclScopeKind::Key,
                value: target.as_bytes(),
            },
        ),
        right,
    )
}

/// The engine capability set with this host's MCP contributions marked supported.
pub fn served_capabilities(
    base: loom_core::capability::CapabilitySet,
) -> loom_core::capability::CapabilitySet {
    base.with_state_overlay(
        &[MCP_HOST_CAPABILITY, MCP_APPS_CAPABILITY, WATCH_CAPABILITY],
        loom_core::CapabilityOperationalState::Supported,
    )
}

#[cfg(test)]
mod capability_tests {
    use super::*;
    use loom_core::capability::CapabilitySet;

    /// The host declares its MCP capabilities and overlays them supported.
    #[test]
    fn host_contributes_mcp_capabilities() {
        let base = CapabilitySet::registry();
        assert!(
            base.get(MCP_HOST_CAPABILITY).is_some(),
            "mcp-host registered"
        );
        assert!(
            base.get(MCP_APPS_CAPABILITY).is_some(),
            "mcp-apps registered"
        );
        assert!(base.get(WATCH_CAPABILITY).is_some(), "watch registered");
        assert!(
            !base.supports(MCP_HOST_CAPABILITY),
            "unsupported at the engine layer"
        );
        assert!(
            !base.supports(MCP_APPS_CAPABILITY),
            "unsupported at the engine layer"
        );
        // `watch` is implemented at the engine layer (Executable), so the base registry already
        // reports it supported; the host overlay keeps it supported. `mcp-host`/`mcp-apps` remain
        // source-backed and are only supported once this host overlays them.
        assert!(
            base.supports(WATCH_CAPABILITY),
            "watch is executable at the engine layer"
        );
        assert!(
            served_capabilities(CapabilitySet::registry()).supports(MCP_HOST_CAPABILITY),
            "host overlays mcp-host supported"
        );
        assert!(
            served_capabilities(CapabilitySet::registry()).supports(MCP_APPS_CAPABILITY),
            "host overlays mcp-apps supported"
        );
        assert!(
            served_capabilities(CapabilitySet::registry()).supports(WATCH_CAPABILITY),
            "host overlays watch supported"
        );
    }
}

type ReadSnapshotCache = Arc<Mutex<Option<(String, Loom<FileStore>)>>>;

/// How the host reaches the loom. The host never assumes a single held handle: it is written against
/// this strategy so the same tool/resource code serves both a local per-request opener and a long-lived
/// server-mode handle.
pub enum StoreAccess {
    /// Local stateless access. Read snapshots are retained while the durable file token is unchanged;
    /// writes invalidate the snapshot before opening the current writable generation.
    PerRequest {
        /// Filesystem path to the `.loom`.
        path: PathBuf,
        /// Unlock and principal-auth material held from launch config.
        auth: Box<LocalOpenAuth>,
        daemon_session: Option<Box<DaemonSession>>,
        read_cache: ReadSnapshotCache,
    },
    /// Persistent handle (server mode): one open loom served across requests, behind a mutex for the
    /// single-writer discipline. Used by a long-lived host (for example Streamable HTTP).
    Persistent(Arc<Mutex<Loom<FileStore>>>),
    /// Remote endpoint: the store lives behind a `loom serve remote` endpoint reached through
    /// [`RemoteMcpBackend`]. Tools that project a unary IDL method forward to the remote Loom; operations
    /// that need a local `Loom<FileStore>` handle are not available and return a clear error.
    Remote(Arc<dyn RemoteMcpBackend>),
}

/// The remote-forwarding backend for a remote-backed MCP host. It is implemented by the launcher (the
/// CLI) over the same `RemoteLoomClient` connection and session that the CLI remote facade uses, so the
/// MCP host does not open a second remote session or auth path. Methods are synchronous; the
/// implementation bridges to its own async client runtime. Only the tool families wired for remote MCP
/// are present here; unwired operations fall through to [`StoreAccess`]'s clear "not available over a
/// remote store" error.
/// The non-scalar arguments of a remote `Vector.upsert_source`, grouped to keep the backend method
/// within the argument-count budget.
pub struct RemoteVectorUpsertSource<'a> {
    /// The vector id.
    pub id: &'a str,
    /// The vector as little-endian `f32` bytes.
    pub vector: &'a [u8],
    /// The metadata as canonical CBOR.
    pub metadata: &'a [u8],
    /// The stored source text (UTF-8).
    pub source_text: &'a [u8],
    /// The optional embedding model id.
    pub model_id: Option<&'a str>,
    /// The optional embedding weights digest.
    pub weights_digest: Option<&'a str>,
}

/// The scalar fields of a remote `Lanes.update`, grouped to keep the backend method within the
/// argument-count budget.
pub struct RemoteLaneUpdate<'a> {
    pub lane_id: &'a str,
    pub title: Option<&'a str>,
    pub description: Option<&'a str>,
    pub lane_status: Option<&'a str>,
    pub status_report: Option<&'a str>,
    pub reviewer_feedback: Option<&'a str>,
    pub updated_by: &'a str,
}

/// The tuning arguments of a remote `Vector.search_policy`, grouped to keep the backend method within
/// the argument-count budget.
pub struct RemoteVectorSearchPolicy<'a> {
    /// The query vector as little-endian `f32` bytes.
    pub query: &'a [u8],
    /// The number of hits to return.
    pub k: u64,
    /// The metadata filter as canonical CBOR.
    pub filter: &'a [u8],
    /// The accelerator policy tag.
    pub policy: i32,
    /// The exact-search threshold below which the exact path is used.
    pub threshold: u64,
    /// The HNSW `ef` search breadth.
    pub ef: u64,
    /// The product-quantization subspace count.
    pub pq_m: u64,
    /// The product-quantization centroid count.
    pub pq_k: u64,
    /// The product-quantization training iterations.
    pub pq_iters: u64,
}

pub trait RemoteMcpBackend: Send + Sync {
    /// Workspaces create (`Workspaces.create`); returns the workspace id string.
    fn workspace_create(&self, name: Option<&str>, facet: Option<FacetKind>) -> Result<String>;

    /// KV get (`Kv.get`).
    fn kv_get(&self, workspace: &str, collection: &str, key_cbor: &[u8])
    -> Result<Option<Vec<u8>>>;
    /// KV put (`Kv.put`).
    fn kv_put(
        &self,
        workspace: &str,
        collection: &str,
        key_cbor: &[u8],
        value: Vec<u8>,
    ) -> Result<()>;
    /// KV delete (`Kv.delete`); returns whether the key existed.
    fn kv_delete(&self, workspace: &str, collection: &str, key_cbor: &[u8]) -> Result<bool>;
    /// KV list (`Kv.list`): the whole map as canonical CBOR.
    fn kv_list(&self, workspace: &str, collection: &str) -> Result<Vec<u8>>;
    /// KV range (`Kv.range`): the half-open `[lo, hi)` slice as canonical CBOR.
    fn kv_range(
        &self,
        workspace: &str,
        collection: &str,
        lo_cbor: &[u8],
        hi_cbor: &[u8],
    ) -> Result<Vec<u8>>;

    /// CAS put (`Cas.put`); returns the content address as a digest string.
    fn cas_put(&self, workspace: &str, content: &[u8]) -> Result<String>;
    /// CAS get (`Cas.get`) by digest string.
    fn cas_get(&self, workspace: &str, digest: &str) -> Result<Option<Vec<u8>>>;
    /// CAS has (`Cas.has`) by digest string.
    fn cas_has(&self, workspace: &str, digest: &str) -> Result<bool>;
    /// CAS delete (`Cas.delete`) by digest string; returns whether it was present.
    fn cas_delete(&self, workspace: &str, digest: &str) -> Result<bool>;
    /// CAS list (`Cas.list`): the reachable content addresses as digest strings.
    fn cas_list(&self, workspace: &str) -> Result<Vec<String>>;

    /// Queue append (`Queue.append`); returns the assigned sequence.
    fn queue_append(&self, workspace: &str, stream: &str, entry: &[u8]) -> Result<u64>;
    /// Queue get (`Queue.get`) at `seq`.
    fn queue_get(&self, workspace: &str, stream: &str, seq: u64) -> Result<Option<Vec<u8>>>;
    /// Queue range (`Queue.range`): entries `[lo, hi)`.
    fn queue_range(&self, workspace: &str, stream: &str, lo: u64, hi: u64) -> Result<Vec<Vec<u8>>>;
    /// Queue len (`Queue.len`).
    fn queue_len(&self, workspace: &str, stream: &str) -> Result<u64>;
    /// Consumer position (`QueueConsumers.consumer_position`).
    fn queue_consumer_position(
        &self,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
    ) -> Result<u64>;
    /// Consumer read (`QueueConsumers.consumer_read`): up to `max` entries without advancing.
    fn queue_consumer_read(
        &self,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
        max: u32,
    ) -> Result<Vec<Vec<u8>>>;
    /// Consumer advance (`QueueConsumers.consumer_advance`).
    fn queue_consumer_advance(
        &self,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
        next_seq: u64,
    ) -> Result<()>;
    /// Consumer reset (`QueueConsumers.consumer_reset`).
    fn queue_consumer_reset(
        &self,
        workspace: &str,
        stream: &str,
        consumer_id: &str,
        next_seq: u64,
    ) -> Result<()>;

    /// Ledger append (`Ledger.append`); returns the new entry's sequence.
    fn ledger_append(&self, workspace: &str, collection: &str, payload: Vec<u8>) -> Result<u64>;
    /// Ledger get (`Ledger.get`) at `seq`.
    fn ledger_get(&self, workspace: &str, collection: &str, seq: u64) -> Result<Option<Vec<u8>>>;
    /// Ledger head (`Ledger.head`) as a digest string.
    fn ledger_head(&self, workspace: &str, collection: &str) -> Result<Option<String>>;
    /// Ledger len (`Ledger.len`).
    fn ledger_len(&self, workspace: &str, collection: &str) -> Result<u64>;
    /// Ledger verify (`Ledger.verify`); errors if the hash chain is broken.
    fn ledger_verify(&self, workspace: &str, collection: &str) -> Result<()>;

    /// Time-series get (`TimeSeries.get`) at timestamp `ts`.
    fn ts_get(&self, workspace: &str, collection: &str, ts: i64) -> Result<Option<Vec<u8>>>;
    /// Time-series put (`TimeSeries.put`) of `value` at timestamp `ts`.
    fn ts_put(&self, workspace: &str, collection: &str, ts: i64, value: Vec<u8>) -> Result<()>;
    /// Time-series range (`TimeSeries.range`): the `[from, to]` window as `Series::encode` bytes.
    fn ts_range(&self, workspace: &str, collection: &str, from: i64, to: i64) -> Result<Vec<u8>>;

    /// Full-text create (`Search.create`) with a canonical-CBOR `mapping`.
    fn search_create(&self, workspace: &str, name: &str, mapping: &[u8]) -> Result<()>;
    /// Full-text index (`Search.index`) of the canonical-CBOR `doc` under `id`.
    fn search_index(&self, workspace: &str, name: &str, id: Vec<u8>, doc: &[u8]) -> Result<()>;
    /// Full-text get (`Search.get`): the document CBOR for `id`, or `None`.
    fn search_get(&self, workspace: &str, name: &str, id: &[u8]) -> Result<Option<Vec<u8>>>;
    /// Full-text delete (`Search.delete`); returns whether the document existed.
    fn search_delete(&self, workspace: &str, name: &str, id: &[u8]) -> Result<bool>;
    /// Full-text ids (`Search.ids`): the ids (optionally prefix-filtered) as canonical CBOR.
    fn search_ids(&self, workspace: &str, name: &str, prefix: Option<&[u8]>) -> Result<Vec<u8>>;
    /// Full-text remap (`Search.remap`) to a new canonical-CBOR `mapping`.
    fn search_remap(&self, workspace: &str, name: &str, mapping: &[u8]) -> Result<()>;
    /// Full-text query (`Search.query`): the canonical-CBOR `request` -> canonical-CBOR response.
    fn search_query(&self, workspace: &str, name: &str, request: &[u8]) -> Result<Vec<u8>>;
    /// Full-text source-digest (`Search.source_digest`) as a digest string.
    fn search_source_digest(&self, workspace: &str, name: &str) -> Result<String>;
    /// Full-text status (`Search.status`) as canonical-CBOR status bytes.
    fn search_status(&self, workspace: &str, name: &str, engine_version: &str) -> Result<Vec<u8>>;

    /// Columnar create (`Columnar.create`) from the canonical-CBOR `columns` schema.
    fn columnar_create(
        &self,
        workspace: &str,
        name: &str,
        columns: &[u8],
        target_segment_rows: u64,
    ) -> Result<()>;
    /// Columnar append (`Columnar.append`) of the canonical-CBOR `row`.
    fn columnar_append(&self, workspace: &str, name: &str, row: &[u8]) -> Result<()>;
    /// Columnar compact (`Columnar.compact`).
    fn columnar_compact(&self, workspace: &str, name: &str) -> Result<()>;
    /// Columnar scan (`Columnar.scan`): all rows as canonical CBOR.
    fn columnar_scan(&self, workspace: &str, name: &str) -> Result<Vec<u8>>;
    /// Columnar columns (`Columnar.columns`): the schema as canonical CBOR.
    fn columnar_columns(&self, workspace: &str, name: &str) -> Result<Vec<u8>>;
    /// Columnar rows (`Columnar.rows`): the row count.
    fn columnar_rows(&self, workspace: &str, name: &str) -> Result<u64>;
    /// Columnar inspect (`Columnar.inspect`): segment metadata as canonical CBOR.
    fn columnar_inspect(&self, workspace: &str, name: &str) -> Result<Vec<u8>>;
    /// Columnar source digest (`Columnar.source_digest`) as a digest string.
    fn columnar_source_digest(&self, workspace: &str, name: &str) -> Result<String>;
    /// Columnar select (`Columnar.select`): a projection/filter as canonical-CBOR rows.
    fn columnar_select(
        &self,
        workspace: &str,
        name: &str,
        columns: &[u8],
        filter: &[u8],
    ) -> Result<Vec<u8>>;
    /// Columnar aggregate (`Columnar.aggregate`): aggregates over a filter as canonical-CBOR cells.
    fn columnar_aggregate(
        &self,
        workspace: &str,
        name: &str,
        aggregates: &[u8],
        filter: &[u8],
    ) -> Result<Vec<u8>>;

    /// Calendar create-collection (`Calendar.create_collection`) with canonical-CBOR `meta`.
    fn calendar_create_collection(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        meta: &[u8],
    ) -> Result<()>;
    /// Calendar delete-collection (`Calendar.delete_collection`); returns whether it existed.
    fn calendar_delete_collection(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> Result<bool>;
    /// Calendar put-entry (`Calendar.put_entry`) of a canonical-CBOR entry; returns its digest string.
    fn calendar_put_entry(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        entry: &[u8],
    ) -> Result<String>;
    /// Calendar put-ics (`Calendar.put_ics`) of an iCalendar document; returns its digest string.
    fn calendar_put_ics(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        ics: &str,
    ) -> Result<String>;
    /// Calendar delete-entry (`Calendar.delete_entry`); returns whether it existed.
    fn calendar_delete_entry(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> Result<bool>;
    /// Calendar get-entry (`Calendar.get_entry`): the entry CBOR, or `None`.
    fn calendar_get_entry(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>>;
    /// Calendar list-entries (`Calendar.list_entries`): each entry as CBOR.
    fn calendar_list_entries(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> Result<Vec<Vec<u8>>>;
    /// Calendar get-collection (`Calendar.get_collection`): the meta CBOR, or `None`.
    fn calendar_get_collection(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> Result<Option<Vec<u8>>>;
    /// Calendar list-collections (`Calendar.list_collections`): the collection ids.
    fn calendar_list_collections(&self, workspace: &str, principal: &str) -> Result<Vec<String>>;
    /// Calendar range (`Calendar.range`): the `[from, to]` occurrences as one CBOR payload.
    fn calendar_range(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<u8>>;
    /// Calendar search (`Calendar.search`): the matching entries, each as CBOR.
    fn calendar_search(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        component: &str,
        text: &str,
    ) -> Result<Vec<Vec<u8>>>;
    /// Calendar to-ics (`Calendar.to_ics`): the iCalendar bytes for `uid`, or `None`.
    fn calendar_to_ics(
        &self,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>>;

    /// Contacts create-book (`Contacts.create_book`) with canonical-CBOR `meta`.
    fn contacts_create_book(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        meta: &[u8],
    ) -> Result<()>;
    /// Contacts delete-book (`Contacts.delete_book`); returns whether it existed.
    fn contacts_delete_book(&self, workspace: &str, principal: &str, book: &str) -> Result<bool>;
    /// Contacts put-entry (`Contacts.put_entry`) of a canonical-CBOR entry; returns its digest string.
    fn contacts_put_entry(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        entry: &[u8],
    ) -> Result<String>;
    /// Contacts put-vcard (`Contacts.put_vcard`) of a vCard document; returns its digest string.
    fn contacts_put_vcard(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        vcard: &str,
    ) -> Result<String>;
    /// Contacts delete-entry (`Contacts.delete_entry`); returns whether it existed.
    fn contacts_delete_entry(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> Result<bool>;
    /// Contacts get-entry (`Contacts.get_entry`): the entry CBOR, or `None`.
    fn contacts_get_entry(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>>;
    /// Contacts list-entries (`Contacts.list_entries`): each entry as CBOR.
    fn contacts_list_entries(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
    ) -> Result<Vec<Vec<u8>>>;
    /// Contacts get-book (`Contacts.get_book`): the book meta CBOR, or `None`.
    fn contacts_get_book(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
    ) -> Result<Option<Vec<u8>>>;
    /// Contacts list-books (`Contacts.list_books`): the book ids.
    fn contacts_list_books(&self, workspace: &str, principal: &str) -> Result<Vec<String>>;
    /// Contacts search (`Contacts.search`): the matching contacts, each as CBOR.
    fn contacts_search(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        text: &str,
    ) -> Result<Vec<Vec<u8>>>;
    /// Contacts to-vcard (`Contacts.to_vcard`): the vCard bytes for `uid`, or `None`.
    fn contacts_to_vcard(
        &self,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>>;

    /// Mail create-mailbox (`Mail.create_mailbox`) with canonical-CBOR `meta`.
    fn mail_create_mailbox(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        meta: &[u8],
    ) -> Result<()>;
    /// Mail delete-mailbox (`Mail.delete_mailbox`); returns whether it existed.
    fn mail_delete_mailbox(&self, workspace: &str, principal: &str, mailbox: &str) -> Result<bool>;
    /// Mail ingest-message (`Mail.ingest_message`) of raw RFC 5322 bytes; returns the body digest string.
    fn mail_ingest_message(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
        raw: &[u8],
    ) -> Result<String>;
    /// Mail delete-message (`Mail.delete_message`); returns whether it existed.
    fn mail_delete_message(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<bool>;
    /// Mail set-flags (`Mail.set_flags`) on message `uid`.
    fn mail_set_flags(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
        flags: &[String],
    ) -> Result<()>;
    /// Mail get-message (`Mail.get_message`): the structured record CBOR, or `None`.
    fn mail_get_message(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>>;
    /// Mail to-eml (`Mail.to_eml`): the raw RFC 5322 bytes for `uid`, or `None`.
    fn mail_to_eml(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>>;
    /// Mail list-messages (`Mail.list_messages`): each record as CBOR.
    fn mail_list_messages(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
    ) -> Result<Vec<Vec<u8>>>;
    /// Mail get-mailbox (`Mail.get_mailbox`): the mailbox meta CBOR, or `None`.
    fn mail_get_mailbox(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
    ) -> Result<Option<Vec<u8>>>;
    /// Mail list-mailboxes (`Mail.list_mailboxes`): the mailbox ids.
    fn mail_list_mailboxes(&self, workspace: &str, principal: &str) -> Result<Vec<String>>;
    /// Mail get-flags (`Mail.get_flags`): the flags on message `uid`.
    fn mail_get_flags(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Vec<String>>;
    /// Mail search (`Mail.search`): the matching messages, each as CBOR.
    fn mail_search(
        &self,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        text: &str,
    ) -> Result<Vec<Vec<u8>>>;

    /// Filesystem read-file (`FileSystem.read_file`): the whole contents of `path`.
    fn fs_read_file(&self, workspace: &str, path: &str) -> Result<Vec<u8>>;
    /// Filesystem read-link (`FileSystem.read_link`): the target of the symlink at `path`.
    fn fs_read_link(&self, workspace: &str, path: &str) -> Result<String>;
    /// Filesystem read-at (`FileSystem.read_at`): `len` bytes of `path` from `offset`.
    fn fs_read_at(&self, workspace: &str, path: &str, offset: u64, len: u64) -> Result<Vec<u8>>;
    /// Filesystem stat (`FileSystem.stat`): canonical-CBOR `loom.fs.stat.v1`.
    fn fs_stat(&self, workspace: &str, path: &str) -> Result<Vec<u8>>;
    /// Filesystem list-directory (`FileSystem.list_directory`): canonical-CBOR `loom.fs.dir-listing.v1`.
    fn fs_list_directory(&self, workspace: &str, path: &str) -> Result<Vec<u8>>;
    /// Filesystem write-file (`FileSystem.write_file`): write `content` to `path` with `mode`.
    fn fs_write_file(&self, workspace: &str, path: &str, content: &[u8], mode: u32) -> Result<()>;
    /// Filesystem append-file (`FileSystem.append_file`): append `content` to `path`.
    fn fs_append_file(&self, workspace: &str, path: &str, content: &[u8]) -> Result<()>;
    /// Filesystem remove-file (`FileSystem.remove_file`): remove `path`.
    fn fs_remove_file(&self, workspace: &str, path: &str) -> Result<()>;
    /// Filesystem create-directory (`FileSystem.create_directory`): create `path`.
    fn fs_create_directory(&self, workspace: &str, path: &str, recursive: bool) -> Result<()>;
    /// Filesystem remove-directory (`FileSystem.remove_directory`): remove `path`.
    fn fs_remove_directory(&self, workspace: &str, path: &str, recursive: bool) -> Result<()>;
    /// Filesystem write-at (`FileSystem.write_at`): write `data` into `path` at `offset`.
    fn fs_write_at(&self, workspace: &str, path: &str, offset: u64, data: &[u8]) -> Result<()>;
    /// Filesystem truncate (`FileSystem.truncate`): resize `path` to `size` bytes.
    fn fs_truncate(&self, workspace: &str, path: &str, size: u64) -> Result<()>;
    /// Filesystem symlink (`FileSystem.symlink`): create a symlink at `link_path` -> `target`.
    fn fs_symlink(&self, workspace: &str, target: &str, link_path: &str) -> Result<()>;

    /// Vector create (`Vector.create`) of an index with `dim` and metric tag `metric`.
    fn vector_create(&self, workspace: &str, name: &str, dim: u64, metric: i32) -> Result<()>;
    /// Vector upsert (`Vector.upsert`) of `vector` bytes + canonical-CBOR `metadata` under `id`.
    fn vector_upsert(
        &self,
        workspace: &str,
        name: &str,
        id: &str,
        vector: &[u8],
        metadata: &[u8],
    ) -> Result<()>;
    /// Vector upsert-with-source (`Vector.upsert_source`).
    fn vector_upsert_source(
        &self,
        workspace: &str,
        name: &str,
        args: RemoteVectorUpsertSource<'_>,
    ) -> Result<()>;
    /// Vector create-metadata-index (`Vector.create_metadata_index`); returns whether it changed.
    fn vector_create_metadata_index(&self, workspace: &str, name: &str, key: &str) -> Result<bool>;
    /// Vector drop-metadata-index (`Vector.drop_metadata_index`); returns whether it changed.
    fn vector_drop_metadata_index(&self, workspace: &str, name: &str, key: &str) -> Result<bool>;
    /// Vector delete (`Vector.delete`); returns whether the id existed.
    fn vector_delete(&self, workspace: &str, name: &str, id: &str) -> Result<bool>;
    /// Vector get (`Vector.get`): the `[vector, metadata]` entry CBOR, or `None`.
    fn vector_get(&self, workspace: &str, name: &str, id: &str) -> Result<Option<Vec<u8>>>;
    /// Vector source-text (`Vector.source_text`): the stored source bytes, or `None`.
    fn vector_source_text(&self, workspace: &str, name: &str, id: &str) -> Result<Option<Vec<u8>>>;
    /// Vector embedding-model (`Vector.embedding_model`): the model profile CBOR, or `None`.
    fn vector_embedding_model(&self, workspace: &str, name: &str) -> Result<Option<Vec<u8>>>;
    /// Vector ids (`Vector.ids`): the ids (optionally prefix-filtered) as canonical-CBOR text array.
    fn vector_ids(&self, workspace: &str, name: &str, prefix: Option<&str>) -> Result<Vec<u8>>;
    /// Vector metadata-index-keys (`Vector.metadata_index_keys`): the keys as canonical-CBOR text array.
    fn vector_metadata_index_keys(&self, workspace: &str, name: &str) -> Result<Vec<u8>>;
    /// Vector search (`Vector.search`): exact hits as canonical CBOR.
    fn vector_search(
        &self,
        workspace: &str,
        name: &str,
        query: &[u8],
        k: u64,
        filter: &[u8],
    ) -> Result<Vec<u8>>;
    /// Vector policy search (`Vector.search_policy`): policy-selected hits as canonical CBOR.
    fn vector_search_policy(
        &self,
        workspace: &str,
        name: &str,
        args: RemoteVectorSearchPolicy<'_>,
    ) -> Result<Vec<u8>>;

    fn metrics_put_descriptor(&self, workspace: &str, descriptor: &[u8]) -> Result<()>;
    fn metrics_get_descriptor(&self, workspace: &str, name: &str) -> Result<Option<Vec<u8>>>;
    fn metrics_put_observation(
        &self,
        workspace: &str,
        descriptor_name: &str,
        observation: &[u8],
    ) -> Result<()>;
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
    ) -> Result<Vec<u8>>;
    fn logs_put_record(&self, workspace: &str, record: &[u8]) -> Result<String>;
    fn logs_get_record(&self, workspace: &str, record_id: &str) -> Result<Option<Vec<u8>>>;
    fn logs_query(
        &self,
        workspace: &str,
        from_time_unix_nano: u64,
        to_time_unix_nano: u64,
        max_records: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>>;
    fn traces_put_span(&self, workspace: &str, span: &[u8]) -> Result<()>;
    fn traces_get_span(
        &self,
        workspace: &str,
        trace_id: &str,
        span_id: &str,
    ) -> Result<Option<Vec<u8>>>;
    fn traces_trace_spans(
        &self,
        workspace: &str,
        trace_id: &str,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>>;
    fn traces_query(
        &self,
        workspace: &str,
        from_start_time_ns: u64,
        to_start_time_ns: u64,
        max_spans: u32,
        max_output_bytes: u64,
    ) -> Result<Vec<u8>>;

    fn document_get_binary(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<loom_core::document::DocumentBinary>>;

    fn document_get_text(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<loom_core::document::DocumentText>> {
        let Some(document) = self.document_get_binary(workspace, collection, id)? else {
            return Ok(None);
        };
        let text = String::from_utf8(document.bytes).map_err(|_| {
            LoomError::document_not_text("document payload is not valid UTF-8 text")
        })?;
        Ok(Some(loom_core::document::DocumentText {
            text,
            digest: document.digest,
            entity_tag: document.entity_tag,
        }))
    }

    /// VCS log (`VersionControl.log`): commit addresses on `branch`, newest first.
    fn vcs_log(&self, workspace: &str, branch: &str) -> Result<Vec<String>>;
    /// VCS head-branch (`VersionControl.head_branch`): current branch name.
    fn vcs_head_branch(&self, workspace: &str) -> Result<String>;
    /// VCS status (`VersionControl.status`): the working-state envelope as canonical CBOR.
    fn vcs_status(&self, workspace: &str) -> Result<Vec<u8>>;
    /// VCS merge-in-progress (`VersionControl.merge_in_progress`).
    fn vcs_merge_in_progress(&self, workspace: &str) -> Result<bool>;
    /// VCS merge-conflicts (`VersionControl.merge_conflicts`): unresolved paths.
    fn vcs_merge_conflicts(&self, workspace: &str) -> Result<Vec<String>>;
    /// VCS tag-list (`VersionControl.tag_list`).
    fn vcs_tag_list(&self, workspace: &str) -> Result<Vec<String>>;
    /// VCS tag-target (`VersionControl.tag_target`): the ref target of `name` as a digest string.
    fn vcs_tag_target(&self, workspace: &str, name: &str) -> Result<Option<String>>;
    /// VCS diff (`VersionControl.diff`): the `LMDIFF` envelope between two commits.
    fn vcs_diff(&self, workspace: &str, from_commit: &str, to_commit: &str) -> Result<Vec<u8>>;
    /// VCS blame (`VersionControl.blame`): the blame rows as canonical CBOR.
    fn vcs_blame(&self, workspace: &str, branch: &str) -> Result<Vec<u8>>;

    /// VCS branch (`VersionControl.branch`): create `name` at the current tip.
    fn vcs_branch(&self, workspace: &str, name: &str) -> Result<()>;
    /// VCS checkout (`VersionControl.checkout`): switch to `branch`.
    fn vcs_checkout(&self, workspace: &str, branch: &str) -> Result<()>;
    /// VCS stage (`VersionControl.stage`) one `path`.
    fn vcs_stage(&self, workspace: &str, path: &str) -> Result<()>;
    /// VCS stage-all (`VersionControl.stage_all`).
    fn vcs_stage_all(&self, workspace: &str) -> Result<()>;
    /// VCS unstage (`VersionControl.unstage`) one `path`.
    fn vcs_unstage(&self, workspace: &str, path: &str) -> Result<()>;
    /// VCS tag-delete (`VersionControl.tag_delete`).
    fn vcs_tag_delete(&self, workspace: &str, name: &str) -> Result<()>;
    /// VCS tag-rename (`VersionControl.tag_rename`).
    fn vcs_tag_rename(&self, workspace: &str, old_name: &str, new_name: &str) -> Result<()>;
    /// VCS restore-file (`VersionControl.restore_file`): reset `path` to `rev`.
    fn vcs_restore_file(&self, workspace: &str, rev: &str, path: &str) -> Result<()>;
    /// VCS restore-path (`VersionControl.restore_path`): reset subtree `prefix` to `rev`.
    fn vcs_restore_path(&self, workspace: &str, rev: &str, prefix: &str) -> Result<()>;
    /// VCS merge-resolve (`VersionControl.merge_resolve`): settle `path` with the 1-byte resolution wire.
    fn vcs_merge_resolve(&self, workspace: &str, path: &str, resolution: &[u8]) -> Result<()>;
    /// VCS merge-abort (`VersionControl.merge_abort`).
    fn vcs_merge_abort(&self, workspace: &str) -> Result<()>;

    /// Graph get-node (`Graph.get_node`): node props CBOR, or `None`.
    fn graph_get_node(&self, workspace: &str, name: &str, id: &str) -> Result<Option<Vec<u8>>>;
    /// Graph get-edge (`Graph.get_edge`): the `[src, dst, label, props]` edge CBOR, or `None`.
    fn graph_get_edge(&self, workspace: &str, name: &str, id: &str) -> Result<Option<Vec<u8>>>;
    /// Graph neighbors (`Graph.neighbors`): adjacent node ids as canonical CBOR text array.
    fn graph_neighbors(&self, workspace: &str, name: &str, id: &str) -> Result<Vec<u8>>;
    /// Graph out-edges (`Graph.out_edges`): outgoing edges as canonical CBOR.
    fn graph_out_edges(&self, workspace: &str, name: &str, id: &str) -> Result<Vec<u8>>;
    /// Graph in-edges (`Graph.in_edges`): incoming edges as canonical CBOR.
    fn graph_in_edges(&self, workspace: &str, name: &str, id: &str) -> Result<Vec<u8>>;
    /// Graph reachable (`Graph.reachable`): reachable node ids as canonical CBOR text array
    /// (`max_depth < 0` = unbounded; empty `via_label` = any edge label).
    fn graph_reachable(
        &self,
        workspace: &str,
        name: &str,
        start: &str,
        max_depth: i64,
        via_label: &str,
    ) -> Result<Vec<u8>>;
    /// Graph shortest-path (`Graph.shortest_path`): the path node ids as canonical CBOR, or `None`.
    fn graph_shortest_path(
        &self,
        workspace: &str,
        name: &str,
        from: &str,
        to: &str,
        via_label: &str,
    ) -> Result<Option<Vec<u8>>>;
    /// Graph query (`Graph.query`): the openCypher query result as canonical CBOR.
    fn graph_query(&self, workspace: &str, name: &str, query: &str) -> Result<Vec<u8>>;
    /// Graph explain-query (`Graph.explain_query`): the query plan as canonical CBOR.
    fn graph_explain_query(&self, workspace: &str, name: &str, query: &str) -> Result<Vec<u8>>;
    /// Graph upsert-node (`Graph.upsert_node`) with canonical-CBOR `props`.
    fn graph_upsert_node(&self, workspace: &str, name: &str, id: &str, props: &[u8]) -> Result<()>;
    /// Graph remove-node (`Graph.remove_node`), optionally cascading incident edges.
    fn graph_remove_node(&self, workspace: &str, name: &str, id: &str, cascade: bool)
    -> Result<()>;

    /// Document list-binary (`Document.list_binary`): the collection's document index as
    /// `Collection::encode` bytes.
    fn document_list_binary(&self, workspace: &str, collection: &str) -> Result<Vec<u8>>;
    /// Document query-json (`Document.query_json`): run the JSON `query_json` over `collection` and
    /// return the canonical JSON result bytes. Used by the `document_query` host composite over remote to
    /// obtain candidate ids for a predicate branch (the host then re-projects over the collection bytes).
    fn document_query_json(
        &self,
        workspace: &str,
        collection: &str,
        query_json: &str,
    ) -> Result<Vec<u8>>;
    /// Document find-json (`Document.find_json`): the ids matching `value_json` on `index` of
    /// `collection`, as a canonical JSON array of ids. Used by the `document_query` host composite over
    /// remote to obtain candidate ids for an index branch.
    fn document_find_json(
        &self,
        workspace: &str,
        collection: &str,
        index: &str,
        value_json: &str,
    ) -> Result<Vec<u8>>;
    /// Store digest-algo (`Store.digest_algo`): the remote store's digest algorithm as a stable name
    /// (`"blake3"`/`"sha256"`). The `document_query` host composite parses it into a `loom_types::Algo`
    /// and computes each item's `Digest::hash(algo, doc)` so a remote digest matches the local one.
    fn store_digest_algo(&self) -> Result<String>;
    /// Document put-binary + ref-index overlay (`Document.put_binary_indexed`): store the document and refresh the
    /// substrate reference index in one server-side op, so a remote MCP `document_put` keeps the ref index
    /// consistent exactly like the local host.
    fn document_put_binary_indexed(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
        doc: Vec<u8>,
    ) -> Result<()>;
    fn document_put_binary_indexed_guarded(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
        bytes: Vec<u8>,
        expected_entity_tag: Option<&str>,
    ) -> Result<Digest> {
        let algo = Algo::from_name(&self.store_digest_algo()?)?;
        if let Some(expected_entity_tag) = expected_entity_tag {
            match self.document_get_binary(workspace, collection, id)? {
                Some(current) if current.entity_tag == expected_entity_tag => {}
                Some(_) | None => {
                    return Err(LoomError::new(
                        Code::Conflict,
                        loom_types::ConflictReason::ExpectedTagMismatch.as_str(),
                    ));
                }
            }
        }
        let digest = Digest::hash(algo, &bytes);
        self.document_put_binary_indexed(workspace, collection, id, bytes)?;
        Ok(digest)
    }

    fn document_put_text_indexed(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
        text: &str,
        expected_entity_tag: Option<&str>,
    ) -> Result<Digest> {
        self.document_put_binary_indexed_guarded(
            workspace,
            collection,
            id,
            text.as_bytes().to_vec(),
            expected_entity_tag,
        )
    }
    /// Document delete + ref-index overlay (`Document.delete_indexed`): delete the document and drop its
    /// reference-index source; returns whether it existed.
    fn document_delete_indexed(&self, workspace: &str, collection: &str, id: &str) -> Result<bool>;
    /// Document replace-text + ref-index overlay (`Document.replace_text_indexed`): the timestamp-free
    /// find/replace the MCP tool performs, plus the reference-index refresh, returning the same
    /// `{replacements, digest}` the local tool returns.
    fn document_replace_text_indexed(
        &self,
        request: crate::writes::DocumentReplaceTextRequest<'_>,
    ) -> Result<crate::writes::DocumentReplaceTextResult>;
    /// Graph upsert-edge + ref-index overlay (`Graph.upsert_edge_indexed`): upsert the edge and refresh
    /// the edge's reference from its `dst` target.
    fn graph_upsert_edge_indexed(
        &self,
        workspace: &str,
        name: &str,
        edge: crate::writes::GraphEdgeWrite<'_>,
    ) -> Result<()>;
    /// Graph remove-edge + ref-index overlay (`Graph.remove_edge_indexed`): remove the edge and drop its
    /// reference-index source; returns whether it existed.
    fn graph_remove_edge_indexed(&self, workspace: &str, name: &str, id: &str) -> Result<bool>;

    /// SQL read-table (`Sql.sql_read_table`): the staged `table` (full facet path) as canonical CBOR.
    fn sql_read_table(&self, workspace: &str, table: &str) -> Result<Vec<u8>>;
    /// SQL read-table-at (`Sql.sql_read_table_at`): the `table` committed at `commit`.
    fn sql_read_table_at(&self, workspace: &str, table: &str, commit: &str) -> Result<Vec<u8>>;
    /// SQL index-scan (`Sql.sql_index_scan`): rows of `index` matching the canonical-CBOR `prefix`.
    fn sql_index_scan(
        &self,
        workspace: &str,
        table: &str,
        index: &str,
        prefix: &[u8],
    ) -> Result<Vec<u8>>;
    /// SQL index-scan-at (`Sql.sql_index_scan_at`): a secondary-index scan at `commit`.
    fn sql_index_scan_at(
        &self,
        workspace: &str,
        table: &str,
        index: &str,
        prefix: &[u8],
        commit: &str,
    ) -> Result<Vec<u8>>;
    /// SQL blame (`Sql.sql_blame`): each current row of `table` on `branch` with its last-set commit.
    fn sql_blame(&self, workspace: &str, branch: &str, table: &str) -> Result<Vec<u8>>;
    /// SQL diff (`Sql.sql_diff`): the row-level diff of `table` between two commits.
    fn sql_diff(
        &self,
        workspace: &str,
        table: &str,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<Vec<u8>>;
    /// SQL table-diff (`Sql.sql_table_diff`): the schema-aware table diff between two commits.
    fn sql_table_diff(
        &self,
        workspace: &str,
        table: &str,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<Vec<u8>>;
    /// SQL list-databases (`Sql.sql_list_databases`): the SQL database (collection) names. The backend
    /// decodes the canonical-CBOR text array so the MCP host's `read_collections` gets a `Vec<String>`.
    fn sql_list_databases(&self, workspace: &str) -> Result<Vec<String>>;
    /// List the collection names for `facet` in `workspace`, decoding the canonical-CBOR text array to a
    /// `Vec<String>`. Kv/Document/TimeSeries/Ledger forward to their `<facet>.list_collections` IDL method;
    /// Queue forwards to `Queue.list_streams`. SQL uses [`RemoteMcpBackend::sql_list_databases`] instead.
    /// The default is the local-handle-unsupported error, so a backend that has not wired this fails
    /// clearly rather than silently returning empty; the real remote backend overrides it to forward per
    /// facet.
    fn list_collections(&self, workspace: &str, facet: FacetKind) -> Result<Vec<String>> {
        let _ = (workspace, facet);
        Err(remote_local_handle_unsupported())
    }
    /// Dataframe create (`Dataframe.create`): create a frame `name` from canonical `DataframePlan` CBOR.
    fn dataframe_create(&self, workspace: &str, name: &str, plan: &[u8]) -> Result<()>;
    /// Dataframe collect (`Dataframe.collect`): the `[columns, rows]` canonical CBOR. Forwarded unchanged:
    /// the server encodes with `DataframeBatch::encode`, byte-identical to the MCP `dataframe_batch_cbor`
    /// (same `loom_codec` codec, same column tuple, same shared `loom_types::cell_value`).
    fn dataframe_collect(&self, workspace: &str, name: &str) -> Result<Vec<u8>>;
    /// Dataframe preview (`Dataframe.preview`): at most `rows` rows, same `[columns, rows]` CBOR shape.
    fn dataframe_preview(&self, workspace: &str, name: &str, rows: u64) -> Result<Vec<u8>>;
    /// Dataframe materialize (`Dataframe.materialize`): the optional `algo:hex` digest. The backend
    /// stringifies the wire `Option<Digest>` to match the MCP tool's `Option<String>` shape.
    fn dataframe_materialize(&self, workspace: &str, name: &str) -> Result<Option<String>>;
    /// Dataframe plan-digest (`Dataframe.plan_digest`): the `algo:hex` plan digest. The backend
    /// stringifies the wire `Digest` to match the MCP tool's `String` shape.
    fn dataframe_plan_digest(&self, workspace: &str, name: &str) -> Result<String>;
    /// Dataframe source-digests (`Dataframe.source_digests`): the canonical-CBOR text array of `algo:hex`
    /// digests. Forwarded unchanged: the server encodes with `loom_wire::digest_list_to_cbor`,
    /// byte-identical to the MCP `digest_strings_cbor` (both `Array(Text(digest.to_string()))`).
    fn dataframe_source_digests(&self, workspace: &str, name: &str) -> Result<Vec<u8>>;
    /// Watch subscribe (`Watch.subscribe`): resolve `workspace` to its id, build the same
    /// `[workspace, branch, facet, path_prefix, change_kinds]` selector wire form the local host builds,
    /// call the IDL `subscribe`, and return the encoded pull cursor string. The implementation owns the
    /// name->id resolution because the selector wire form carries a `WorkspaceId`, which the remote MCP
    /// host cannot resolve without a local registry.
    fn watch_subscribe(
        &self,
        workspace: &str,
        branch: &str,
        from: Option<&str>,
        facet: Option<&str>,
        path_prefix: Option<&str>,
        change_kinds: &[String],
    ) -> Result<String>;
    /// Watch poll (`Watch.poll`): return the canonical watch-batch CBOR (`loom.watch.batch.v1`, now
    /// carrying each event's nullable `parent`) for `cursor`. The host decodes it with
    /// `watch_batch_from_cbor` and projects it into the same `WatchBatchSummary` the local path builds.
    /// The implementation reproduces the local cursor/workspace guard before polling.
    fn watch_poll(&self, workspace: &str, cursor: &str, max: u32) -> Result<Vec<u8>>;
    /// TimeSeries latest (`TimeSeries.latest`): the most recent point of `collection` as `(ts, value)`,
    /// or `None` when the series is empty. The wire payload carries the `[ts, value]` pair, so the backend
    /// decodes both and the MCP host rebuilds the timestamped `TsPoint`.
    fn ts_latest(&self, workspace: &str, collection: &str) -> Result<Option<(i64, Vec<u8>)>>;
    /// VCS commit (`VersionControl.commit`): commit `workspace`'s working tree with the caller
    /// `timestamp_ms` and return the `algo:hex` digest. The IDL carries `timestamp_ms`, so the remote
    /// digest matches the local MCP commit for identical inputs (content-addressed). Also backs the MCP
    /// `sql_commit` tool, whose local path is the same `loom.commit` over the SQL-facet workspace.
    fn vcs_commit(
        &self,
        workspace: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<String>;
    /// VCS commit-staged (`VersionControl.commit_staged`): commit only the staged index; digest string.
    fn vcs_commit_staged(
        &self,
        workspace: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<String>;
    /// VCS tag-create (`VersionControl.tag_create`): create a tag at `rev` (annotated when `message` is
    /// non-empty) with the caller `timestamp_ms`; returns the ref target digest string.
    fn vcs_tag_create(
        &self,
        workspace: &str,
        name: &str,
        rev: &str,
        tagger: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<String>;
    /// VCS merge-continue (`VersionControl.merge_continue`): record the two-parent merge commit with the
    /// caller `timestamp_ms`; digest string.
    fn vcs_merge_continue(
        &self,
        workspace: &str,
        author: &str,
        timestamp_ms: u64,
    ) -> Result<String>;
    /// VCS squash (`VersionControl.squash`): collapse commits after `onto` into one with the caller
    /// `timestamp_ms`; digest string.
    fn vcs_squash(
        &self,
        workspace: &str,
        onto: &str,
        author: &str,
        message: &str,
        timestamp_ms: u64,
    ) -> Result<String>;
    /// VCS merge (`VersionControl.merge`): reconcile `from_branch` into `workspace`'s current branch with
    /// the caller `timestamp_ms` (`cell_level` reconciles tables at cell granularity), returning the same
    /// `MergeOutcome` the local path produces. The server encodes the canonical `MergeResult` wire and the
    /// backend decodes it losslessly, so a remote merge at a fixed timestamp yields the same digest as local.
    fn vcs_merge(
        &self,
        workspace: &str,
        from_branch: &str,
        author: &str,
        cell_level: bool,
        timestamp_ms: u64,
    ) -> Result<loom_core::MergeOutcome>;
    /// VCS cherry-pick (`VersionControl.cherry_pick`): replay `commits` onto the current branch with the
    /// caller `timestamp_ms`; `dry_run` previews conflicts without changing anything. Returns the same
    /// `ReplayOutcome` the local path produces (decoded from the canonical wire).
    fn vcs_cherry_pick(
        &self,
        workspace: &str,
        commits: &[String],
        dry_run: bool,
        timestamp_ms: u64,
    ) -> Result<loom_core::ReplayOutcome>;
    /// VCS revert (`VersionControl.revert`): apply the inverse of `commits` as new commits authored by
    /// `author` with the caller `timestamp_ms`; `dry_run` previews only. Returns the decoded `ReplayOutcome`.
    fn vcs_revert(
        &self,
        workspace: &str,
        commits: &[String],
        author: &str,
        dry_run: bool,
        timestamp_ms: u64,
    ) -> Result<loom_core::ReplayOutcome>;
    /// VCS rebase (`VersionControl.rebase`): replay the current branch onto `onto` with the caller
    /// `timestamp_ms`; `dry_run` previews only. Returns the decoded `ReplayOutcome`.
    fn vcs_rebase(
        &self,
        workspace: &str,
        onto: &str,
        dry_run: bool,
        timestamp_ms: u64,
    ) -> Result<loom_core::ReplayOutcome>;
    /// SQL query (`Sql.sql_query_result`): run a read-only `sql` against `db` and return the FULL
    /// canonical `exec_cbor` payload (per-statement labels + rows), byte-identical to the local
    /// `read_sql_query`. The server never persists and rejects a mutating statement, preserving the
    /// read-only contract; this is the full-result counterpart to the row-only `sql_query` stream.
    fn sql_query(&self, workspace: &str, db: &str, sql: &str) -> Result<Vec<u8>>;
    /// SQL exec (`Sql.sql_exec`): open a per-request `SqlSession` on `db`, run `sql`, and return the
    /// canonical `exec_cbor` payload byte-for-byte (the server runs the same `LoomSqlStore::exec_cbor` on
    /// the same engine). The implementation opens the session, execs, and closes/frees it on both success
    /// and error. `sql_query`/`sql_commit` are NOT part of this trait: they reject in-method (a full
    /// read-only result method and a timestamp-carrying commit are pending contract decisions).
    fn sql_exec(&self, workspace: &str, db: &str, sql: &str) -> Result<Vec<u8>>;

    fn lanes_create(&self, workspace: &str, lane: loom_lanes::Lane) -> Result<loom_lanes::Lane> {
        let _ = (workspace, lane);
        Err(remote_local_handle_unsupported())
    }

    fn lanes_get(&self, workspace: &str, lane_id: &str) -> Result<Option<loom_lanes::Lane>> {
        let _ = (workspace, lane_id);
        Err(remote_local_handle_unsupported())
    }

    fn lanes_list(&self, workspace: &str) -> Result<Vec<loom_lanes::Lane>> {
        let _ = workspace;
        Err(remote_local_handle_unsupported())
    }

    fn lanes_update(
        &self,
        workspace: &str,
        request: RemoteLaneUpdate<'_>,
    ) -> Result<loom_lanes::Lane> {
        let _ = (workspace, request);
        Err(remote_local_handle_unsupported())
    }

    fn lanes_ticket_add(
        &self,
        workspace: &str,
        lane_id: &str,
        ticket_id: &str,
        updated_by: &str,
    ) -> Result<loom_lanes::Lane> {
        let _ = (workspace, lane_id, ticket_id, updated_by);
        Err(remote_local_handle_unsupported())
    }

    fn lanes_ticket_remove(
        &self,
        workspace: &str,
        lane_id: &str,
        ticket_id: &str,
        updated_by: &str,
    ) -> Result<loom_lanes::Lane> {
        let _ = (workspace, lane_id, ticket_id, updated_by);
        Err(remote_local_handle_unsupported())
    }

    /// Execute a server-promoted MCP tool by name on the hosted server. This is the thin-client transport
    /// for host/composite families: instead of reconstructing tool behavior from low-level primitives, the
    /// local host forwards the whole tool operation and renders the server's response. `args_json` is the
    /// tool's JSON arguments encoded with `serde_json`; the returned bytes are the tool's JSON result value
    /// (the same value the local MCP path returns), so the host renders an identical `CallToolResult`. The
    /// default rejects with a precise error, so only a backend that wires the server-side execution
    /// transport participates.
    ///
    /// # Errors
    /// Returns [`LoomError`] when the backend does not wire server-side execution, or the server declines
    /// or fails the tool.
    fn execute_tool(&self, name: &str, args_json: &[u8]) -> Result<Vec<u8>> {
        let _ = args_json;
        Err(LoomError::new(
            loom_core::error::Code::Unsupported,
            format!(
                "MCP tool {name} is not available against a remote Loom store: server-side tool execution is not wired for this backend"
            ),
        ))
    }
}

/// The error returned when an MCP operation that needs a local `Loom<FileStore>` handle is attempted
/// against a remote-backed host.
fn remote_local_handle_unsupported() -> LoomError {
    LoomError::unsupported(
        "this MCP operation is not available against a remote Loom store; run `loom mcp` against a local .loom path",
    )
}

impl StoreAccess {
    fn path_change_token(path: &Path) -> Option<String> {
        let metadata = std::fs::metadata(path).ok()?;
        let modified = metadata.modified().ok()?;
        let elapsed = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
        Some(format!(
            "{}:{}:{}",
            metadata.len(),
            elapsed.as_secs(),
            elapsed.subsec_nanos()
        ))
    }

    /// Return a cheap token that changes when locally served durable state changes.
    #[cfg(feature = "server")]
    pub(crate) fn change_token(&self) -> Option<String> {
        match self {
            StoreAccess::PerRequest { path, .. } => Self::path_change_token(path),
            StoreAccess::Persistent(handle) => {
                let loom = handle.lock().ok()?;
                Some(format!(
                    "{}:{}",
                    loom.store()
                        .reference_root()
                        .map_or_else(|| "none".to_string(), |root| root.to_string()),
                    loom.store()
                        .control_root()
                        .map_or_else(|| "none".to_string(), |root| root.to_string())
                ))
            }
            StoreAccess::Remote(_) => None,
        }
    }

    /// A per-request opener over `path`, optionally unlocking with `key`.
    pub fn per_request(path: impl Into<PathBuf>, key: Option<KeySpec>) -> Self {
        StoreAccess::PerRequest {
            path: path.into(),
            auth: Box::new(LocalOpenAuth {
                unlock_key: key,
                ..Default::default()
            }),
            daemon_session: None,
            read_cache: Arc::new(Mutex::new(None)),
        }
    }

    /// A per-request opener over `path` with launch-time auth.
    pub fn per_request_auth(path: impl Into<PathBuf>, auth: LocalOpenAuth) -> Self {
        StoreAccess::PerRequest {
            path: path.into(),
            auth: Box::new(auth),
            daemon_session: None,
            read_cache: Arc::new(Mutex::new(None)),
        }
    }

    /// A per-request opener that attaches this host process to the local coordinator daemon.
    pub fn per_request_attached(path: impl Into<PathBuf>, key: Option<KeySpec>) -> Result<Self> {
        let path = path.into();
        let daemon_session = DaemonSession::attach(&path, daemon::DaemonAuth::default())?;
        Ok(StoreAccess::PerRequest {
            path,
            auth: Box::new(LocalOpenAuth {
                unlock_key: key,
                ..Default::default()
            }),
            daemon_session: Some(Box::new(daemon_session)),
            read_cache: Arc::new(Mutex::new(None)),
        })
    }

    /// A per-request opener attached to the local coordinator daemon with launch-time auth.
    pub fn per_request_attached_auth(
        path: impl Into<PathBuf>,
        auth: LocalOpenAuth,
    ) -> Result<Self> {
        let path = path.into();
        let daemon_auth = daemon_auth_from_local(&auth);
        let daemon_session = DaemonSession::attach(&path, daemon_auth)?;
        Ok(StoreAccess::PerRequest {
            path,
            auth: Box::new(auth),
            daemon_session: Some(Box::new(daemon_session)),
            read_cache: Arc::new(Mutex::new(None)),
        })
    }

    /// A persistent handle over an already-open loom (server mode).
    pub fn persistent(loom: Loom<FileStore>) -> Self {
        StoreAccess::Persistent(Arc::new(Mutex::new(loom)))
    }

    /// A remote-backed access mode: tools that project a unary IDL method forward to `backend`.
    pub fn remote(backend: Arc<dyn RemoteMcpBackend>) -> Self {
        StoreAccess::Remote(backend)
    }

    /// The remote backend, when this is a remote-backed host.
    pub(crate) fn remote_backend(&self) -> Option<&Arc<dyn RemoteMcpBackend>> {
        match self {
            StoreAccess::Remote(backend) => Some(backend),
            _ => None,
        }
    }

    /// The filesystem path and unlock key, for the per-request mode only. SQL sessions
    /// (`sql_exec`/`sql_commit`) open their own lock-free read snapshot plus a write flush over this
    /// path and cannot run against a single held handle; this returns `None` in persistent mode.
    pub(crate) fn per_request_parts(
        &self,
    ) -> Result<Option<(&std::path::Path, &LocalOpenAuth, bool)>> {
        match self {
            StoreAccess::PerRequest {
                path,
                auth,
                daemon_session,
                ..
            } => {
                if let Some(session) = daemon_session {
                    session.ensure_live()?;
                }
                Ok(Some((
                    path.as_path(),
                    auth.as_ref(),
                    daemon_session.is_some(),
                )))
            }
            StoreAccess::Persistent(_) | StoreAccess::Remote(_) => Ok(None),
        }
    }

    pub(crate) fn has_runtime_state(&self) -> bool {
        matches!(self, StoreAccess::Persistent(_))
    }

    pub(crate) fn daemon_session_parts(
        &self,
    ) -> Result<Option<(&daemon::DaemonPaths, &str, &daemon::DaemonAuth)>> {
        match self {
            StoreAccess::PerRequest {
                daemon_session: Some(session),
                ..
            } => {
                session.ensure_live()?;
                Ok(Some((&session.paths, &session.session, &session.auth)))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn signal_reference_reconcile(&self) -> Result<()> {
        let StoreAccess::PerRequest {
            daemon_session: Some(session),
            ..
        } = self
        else {
            return Ok(());
        };
        session.ensure_live()?;
        let response = daemon::request_checked(
            &session.paths,
            &format!("reference-reconcile\t{}\n", session.session),
        )?;
        if response == "ok\n" {
            Ok(())
        } else {
            Err(LoomError::corrupt(
                "daemon returned an invalid reference reconcile response",
            ))
        }
    }

    #[cfg(feature = "server")]
    pub(crate) fn has_attached_daemon_session(&self) -> bool {
        matches!(
            self,
            StoreAccess::PerRequest {
                daemon_session: Some(_),
                ..
            }
        )
    }

    #[cfg(feature = "server")]
    pub(crate) fn ensure_attached_daemon_live(&self) -> Result<()> {
        match self {
            StoreAccess::PerRequest {
                daemon_session: Some(session),
                ..
            } => session.ensure_live(),
            _ => Ok(()),
        }
    }

    /// Run `f` against a read-only view of the loom. Local stateless access reuses a read snapshot
    /// until the durable file changes; server mode locks the held handle.
    pub fn read<T>(&self, f: impl FnOnce(&Loom<FileStore>) -> Result<T>) -> Result<T> {
        match self {
            StoreAccess::PerRequest {
                path,
                auth,
                daemon_session,
                read_cache,
            } => {
                if let Some(session) = daemon_session {
                    session.ensure_live()?;
                }
                if local_auth_requires_write(auth) {
                    with_local_store_write_lock(path, || {
                        let loom =
                            open_per_request_read_loom(path, auth, daemon_session.is_some())?;
                        f(&loom)
                    })
                } else {
                    let token = Self::path_change_token(path).ok_or_else(|| {
                        LoomError::not_found(format!("store {} not found", path.display()))
                    })?;
                    let mut cache = read_cache.lock().map_err(|_| lock_poisoned())?;
                    let stale = cache
                        .as_ref()
                        .is_none_or(|(cached_token, _)| cached_token != &token);
                    if stale {
                        let loom =
                            open_per_request_read_loom(path, auth, daemon_session.is_some())?;
                        let token = Self::path_change_token(path).unwrap_or(token);
                        *cache = Some((token, loom));
                    }
                    f(&cache.as_ref().expect("read cache populated").1)
                }
            }
            StoreAccess::Persistent(handle) => {
                let loom = handle.lock().map_err(|_| lock_poisoned())?;
                f(&loom)
            }
            StoreAccess::Remote(_) => Err(remote_local_handle_unsupported()),
        }
    }

    pub(crate) fn read_registry<T>(
        &self,
        f: impl FnOnce(&Loom<FileStore>) -> Result<T>,
    ) -> Result<T> {
        match self {
            StoreAccess::PerRequest {
                path,
                auth,
                daemon_session,
                ..
            } if !local_auth_requires_write(auth) => {
                if let Some(session) = daemon_session {
                    session.ensure_live()?;
                }
                let loom = open_loom_registry_read_unlocked(path, auth.unlock_key.as_ref())?;
                let loom = attach_local_auth(loom, auth)?;
                f(&loom)
            }
            _ => self.read(f),
        }
    }

    pub(crate) fn read_runtime<T>(
        &self,
        f: impl FnOnce(&mut Loom<FileStore>) -> Result<T>,
    ) -> Result<T> {
        match self {
            StoreAccess::PerRequest {
                path,
                auth,
                daemon_session,
                read_cache,
            } => {
                if let Some(session) = daemon_session {
                    session.ensure_live()?;
                }
                if local_auth_requires_write(auth) {
                    with_local_store_write_lock(path, || {
                        let mut loom =
                            open_per_request_read_loom(path, auth, daemon_session.is_some())?;
                        f(&mut loom)
                    })
                } else {
                    let token = Self::path_change_token(path).ok_or_else(|| {
                        LoomError::not_found(format!("store {} not found", path.display()))
                    })?;
                    let mut cache = read_cache.lock().map_err(|_| lock_poisoned())?;
                    let stale = cache
                        .as_ref()
                        .is_none_or(|(cached_token, _)| cached_token != &token);
                    if stale {
                        let loom =
                            open_per_request_read_loom(path, auth, daemon_session.is_some())?;
                        let token = Self::path_change_token(path).unwrap_or(token);
                        *cache = Some((token, loom));
                    }
                    f(&mut cache.as_mut().expect("read cache populated").1)
                }
            }
            StoreAccess::Persistent(handle) => {
                let mut loom = handle.lock().map_err(|_| lock_poisoned())?;
                f(&mut loom)
            }
            StoreAccess::Remote(_) => Err(remote_local_handle_unsupported()),
        }
    }

    /// Run `f` against a writable loom, persisting after `f` succeeds. In per-request mode this opens,
    /// mutates, saves, and closes; in server mode it locks the held handle and saves.
    pub fn write<T>(&self, f: impl FnOnce(&mut Loom<FileStore>) -> Result<T>) -> Result<T> {
        match self {
            StoreAccess::PerRequest {
                path,
                auth,
                daemon_session,
                read_cache,
            } => {
                if let Some(session) = daemon_session {
                    session.ensure_live()?;
                }
                *read_cache.lock().map_err(|_| lock_poisoned())? = None;
                with_local_store_write_lock(path, || {
                    let mut loom = if daemon_session.is_some() {
                        open_loom_daemon_authorized_unlocked(path, auth.unlock_key.as_ref())?
                    } else {
                        open_loom_unlocked(path, auth.unlock_key.as_ref())?
                    };
                    loom = attach_local_auth(loom, auth)?;
                    let out = f(&mut loom)?;
                    save_loom(&mut loom)?;
                    drop(loom);
                    Ok(out)
                })
            }
            StoreAccess::Persistent(handle) => {
                let mut loom = handle.lock().map_err(|_| lock_poisoned())?;
                let out = f(&mut loom)?;
                save_loom(&mut loom)?;
                Ok(out)
            }
            StoreAccess::Remote(_) => Err(remote_local_handle_unsupported()),
        }
    }
}

pub(crate) fn open_per_request_read_loom(
    path: &Path,
    auth: &LocalOpenAuth,
    daemon_authorized: bool,
) -> Result<Loom<FileStore>> {
    let loom = if local_auth_requires_write(auth) {
        if daemon_authorized {
            open_loom_daemon_authorized_unlocked(path, auth.unlock_key.as_ref())?
        } else {
            open_loom_unlocked(path, auth.unlock_key.as_ref())?
        }
    } else {
        open_loom_read_unlocked(path, auth.unlock_key.as_ref())?
    };
    attach_local_auth(loom, auth)
}

pub struct DaemonSession {
    paths: daemon::DaemonPaths,
    session: String,
    auth: daemon::DaemonAuth,
}

impl DaemonSession {
    fn attach(path: &std::path::Path, mut auth: daemon::DaemonAuth) -> Result<Self> {
        let paths = daemon::paths(path)?;
        let session = daemon_session_id();
        auth.session = Some(session.clone());
        daemon::session_attach_auth(&paths, &session, &auth)?;
        Ok(Self {
            paths,
            session,
            auth,
        })
    }

    fn ensure_live(&self) -> Result<()> {
        daemon::session_check_auth(&self.paths, &self.session, &self.auth)
            .map(|_| ())
            .map_err(|e| {
                if e.code == loom_core::Code::NotFound {
                    LoomError::not_found(
                        "attached daemon session is no longer live; reconnect to a running daemon",
                    )
                } else {
                    e
                }
            })
    }
}

impl Drop for DaemonSession {
    fn drop(&mut self) {
        let _ = daemon::session_detach_auth(&self.paths, &self.session, &self.auth);
    }
}

fn daemon_auth_from_local(auth: &LocalOpenAuth) -> daemon::DaemonAuth {
    daemon::DaemonAuth {
        principal: auth.principal.map(|principal| principal.to_string()),
        passphrase: auth.passphrase.clone(),
        session: auth.session_id.clone(),
    }
}

fn daemon_session_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("mcp:{}:{now}", std::process::id())
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn reject_stateless_ephemeral_kv<S: ObjectStore>(
    loom: &Loom<S>,
    has_runtime_state: bool,
    ns: WorkspaceId,
    name: &str,
) -> Result<()> {
    if !has_runtime_state && loom.kv_map_config(ns, name).tier == KvTier::Ephemeral {
        return Err(LoomError::unsupported(
            "pure ephemeral KV maps require a stateful MCP host or daemon-hosted runtime state",
        ));
    }
    Ok(())
}

fn lock_poisoned() -> LoomError {
    LoomError::corrupt("loom handle lock poisoned")
}

/// The engine facade the MCP wire host calls. It owns the [`StoreAccess`] and exposes engine
/// operations; the `rmcp` layer (feature `server`) projects these as tools, resources, and prompts.
pub struct LoomMcp {
    store: StoreAccess,
}

impl LoomMcp {
    /// Build a host over `store`.
    pub fn new(store: StoreAccess) -> Self {
        Self { store }
    }

    /// The store-access strategy backing this host.
    pub fn store(&self) -> &StoreAccess {
        &self.store
    }

    #[cfg(feature = "server")]
    pub(crate) fn has_attached_daemon_session(&self) -> bool {
        self.store.has_attached_daemon_session()
    }

    #[cfg(feature = "server")]
    pub(crate) fn ensure_attached_daemon_live(&self) -> Result<()> {
        self.store.ensure_attached_daemon_live()
    }

    /// The engine version (`store_version`). Static, no store access.
    pub fn version(&self) -> &'static str {
        loom_core::VERSION
    }

    /// Confirm the loom opens and the engine state loads (a read through the policy enforcement point).
    /// Proves the host's store wiring end to end without mutating anything.
    pub fn check_open(&self) -> Result<()> {
        self.store.read(|_loom| Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::Algo;

    fn fresh_loom(path: &std::path::Path) {
        loom_coordination::with_local_store_write_lock(path, || {
            let store = FileStore::create_with_profile(path, Algo::Blake3).unwrap();
            let mut loom = Loom::new(store);
            save_loom(&mut loom).unwrap();
            drop(loom);
            Ok(())
        })
        .unwrap();
    }

    fn temp_path() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let uniq = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("loom-mcp-{}-{seq}-{uniq}.loom", std::process::id()))
    }

    #[test]
    fn version_is_reported() {
        let mcp = LoomMcp::new(StoreAccess::per_request("/nonexistent.loom", None));
        assert_eq!(mcp.version(), loom_core::VERSION);
        assert!(!mcp.version().is_empty());
    }

    #[test]
    fn per_request_access_opens_a_fresh_loom() {
        let path = temp_path();
        fresh_loom(&path);
        let mcp = LoomMcp::new(StoreAccess::per_request(&path, None));
        mcp.check_open()
            .expect("per-request read should open the loom");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn per_request_access_reuses_reads_and_invalidates_after_write() {
        let path = temp_path();
        fresh_loom(&path);
        let access = StoreAccess::per_request(&path, None);

        access.read(|_loom| Ok(())).expect("first read");
        let StoreAccess::PerRequest { read_cache, .. } = &access else {
            panic!("per-request access expected");
        };
        let first_token = read_cache
            .lock()
            .unwrap()
            .as_ref()
            .expect("read cache populated")
            .0
            .clone();
        access.read(|_loom| Ok(())).expect("second read");
        assert_eq!(
            read_cache
                .lock()
                .unwrap()
                .as_ref()
                .expect("read cache retained")
                .0,
            first_token,
            "unchanged reads retain the cached snapshot"
        );

        access.write(|_loom| Ok(())).expect("write");
        assert!(
            read_cache.lock().unwrap().is_none(),
            "writes invalidate the read snapshot"
        );
        access.read(|_loom| Ok(())).expect("read after write");
        assert!(read_cache.lock().unwrap().is_some());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn per_request_access_refreshes_after_external_write() {
        let path = temp_path();
        fresh_loom(&path);
        let reader = StoreAccess::per_request(&path, None);
        let writer = StoreAccess::per_request(&path, None);

        reader.read(|_loom| Ok(())).expect("initial read");
        let StoreAccess::PerRequest { read_cache, .. } = &reader else {
            panic!("per-request access expected");
        };
        let initial_token = read_cache
            .lock()
            .unwrap()
            .as_ref()
            .expect("read cache populated")
            .0
            .clone();

        writer.write(|_loom| Ok(())).expect("external write");
        reader.read(|_loom| Ok(())).expect("refreshed read");
        let refreshed_token = read_cache
            .lock()
            .unwrap()
            .as_ref()
            .expect("read cache refreshed")
            .0
            .clone();
        assert_ne!(initial_token, refreshed_token);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn persistent_access_reads_and_writes_through_one_handle() {
        let path = temp_path();
        fresh_loom(&path);
        let loom = open_loom_unlocked(&path, None).unwrap();
        let mcp = LoomMcp::new(StoreAccess::persistent(loom));
        // Read through the held handle.
        mcp.check_open().expect("persistent read should succeed");
        // A no-op write persists cleanly (save after the closure).
        mcp.store()
            .write(|_loom| Ok(()))
            .expect("persistent write should succeed");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn per_request_writes_are_linearized_by_store_path() {
        let path = temp_path();
        fresh_loom(&path);
        let access = Arc::new(StoreAccess::per_request(&path, None));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let first = std::thread::spawn({
            let access = Arc::clone(&access);
            move || {
                access
                    .write(|_loom| {
                        entered_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        Ok(())
                    })
                    .unwrap();
            }
        });
        entered_rx.recv().unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let second = std::thread::spawn({
            let access = Arc::clone(&access);
            move || {
                access
                    .write(|_loom| {
                        done_tx.send(()).unwrap();
                        Ok(())
                    })
                    .unwrap();
            }
        });
        assert!(
            done_rx
                .recv_timeout(std::time::Duration::from_millis(50))
                .is_err()
        );
        release_tx.send(()).unwrap();
        first.join().unwrap();
        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        second.join().unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn persistent_writes_are_linearized_by_store_handle() {
        let path = temp_path();
        fresh_loom(&path);
        let loom = open_loom_unlocked(&path, None).unwrap();
        let access = Arc::new(StoreAccess::persistent(loom));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let first = std::thread::spawn({
            let access = Arc::clone(&access);
            move || {
                access
                    .write(|_loom| {
                        entered_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        Ok(())
                    })
                    .unwrap();
            }
        });
        entered_rx.recv().unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let second = std::thread::spawn({
            let access = Arc::clone(&access);
            move || {
                access
                    .write(|_loom| {
                        done_tx.send(()).unwrap();
                        Ok(())
                    })
                    .unwrap();
            }
        });
        assert!(
            done_rx
                .recv_timeout(std::time::Duration::from_millis(50))
                .is_err()
        );
        release_tx.send(()).unwrap();
        first.join().unwrap();
        done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        second.join().unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_loom_is_an_error_not_a_panic() {
        let mcp = LoomMcp::new(StoreAccess::per_request(temp_path(), None));
        assert!(mcp.check_open().is_err());
    }

    #[test]
    fn attached_access_requires_a_running_daemon() {
        let path = temp_path();
        fresh_loom(&path);
        let err = match StoreAccess::per_request_attached(&path, None) {
            Ok(_) => panic!("attached access must require a running daemon"),
            Err(err) => err,
        };
        assert_eq!(err.code, loom_core::error::Code::NotFound);
        let _ = std::fs::remove_file(&path);
    }
}
