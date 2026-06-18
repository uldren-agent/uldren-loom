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
use loom_core::FileKind;
use loom_locator::Target;

#[cfg(feature = "remote-client")]
use loom_locator::{ContextResolver, Discovery as LocatorDiscovery, RemoteTarget};
#[cfg(feature = "remote-client")]
use loom_remote_client::carrier::Http2TlsTransport;
#[cfg(feature = "remote-client")]
use loom_remote_client::{RemoteConnection, RemoteLoomClient};
#[cfg(feature = "remote-client")]
use loom_remote_protocol::api_types::{Digest as WireDigest, LoomSession, Uuid};
#[cfg(feature = "remote-client")]
use loom_remote_protocol::discovery::DiscoveryMode;
#[cfg(feature = "remote-client")]
use loom_remote_protocol::generated_api::{
    Acl, Calendar, Cas, Columnar, Contacts, Dataframe, Document, FileSystem, Graph, Identity,
    KeySource, Kv, Lanes, Ledger, Logs, Mail, Metrics, ProtectedRefs, Queue, QueueConsumers,
    Search, Sql, Store, StoreAdmin, TimeSeries, Traces, Transfer, Vector, VersionControl, Watch,
    Workspaces,
};
#[cfg(feature = "remote-client")]
use loom_remote_protocol::session::SessionAuth;
#[cfg(feature = "remote-client")]
use std::sync::Arc;

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

/// Open a store client: resolve the locator, connecting remotely for a remote target (or failing
/// clearly when remote support is not built in).
///
/// # Errors
/// Returns a message when the locator cannot be resolved or a remote connection cannot be established.
pub(crate) fn open_store_client(store: &str) -> Result<StoreClient, String> {
    match crate::locator_cx::current().resolve_target(store)? {
        Target::Local(path) => Ok(StoreClient::Local {
            locator: path.to_string_lossy().into_owned(),
        }),
        #[cfg(feature = "remote-client")]
        Target::Remote(target) => {
            let store = match remote_session_auth(&target)? {
                SessionAuth::Unauthenticated => RemoteStore::connect(&target)?,
                auth => RemoteStore::connect_with_auth(&target, auth)?,
            };
            Ok(StoreClient::Remote(Box::new(store)))
        }
        #[cfg(not(feature = "remote-client"))]
        Target::Remote(target) => Err(format!(
            "locator resolves to remote endpoint {}; rebuild with the `remote-client` feature to forward remote commands",
            target.url
        )),
    }
}

/// Defensive message for a StoreAdmin facade method invoked on a local client. The `store` admin
/// handlers branch on `is_remote` and only call these on a remote client, so this is not reached.
const LOCAL_ADMIN_VIA_LOCAL_PATH: &str =
    "store admin over a local store is handled by the local path, not the StoreAdmin facade";

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

/// Whether `store` resolves to a remote endpoint, without opening a connection. Used to reject the
/// path-shaped `fs` import/export over a remote locator (fs-tree byte transfer is deferred).
pub(crate) fn target_is_remote(store: &str) -> Result<bool, String> {
    Ok(matches!(
        crate::locator_cx::current().resolve_target(store)?,
        Target::Remote(_)
    ))
}

/// Chunk size for the byte-transfer import write loop (bounded frames, `specs/0067` §17.5).
const TRANSFER_CHUNK_BYTES: usize = 1024 * 1024;

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

/// Apply a local byte-transfer import (archive family or CAR) to `workspace`, returning the report.
fn local_transfer_import(
    loom: &mut Loom<FileStore>,
    workspace: &str,
    kind: &str,
    bytes: &[u8],
    commit: bool,
    dry_run: bool,
) -> Result<loom_interchange::ImportReport, String> {
    use loom_interchange_io::transfer::TransferKind;
    let kind = TransferKind::parse(kind).map_err(|e| e.to_string())?;
    let report = match kind {
        TransferKind::Car => {
            let mut options = loom_interchange_io::CarImportOptions::new(workspace);
            options.dry_run = dry_run;
            loom_interchange_io::import_car_bytes(loom, bytes, &options)
                .map_err(|e| e.to_string())?
                .report
        }
        _ => {
            let archive_kind = transfer_kind_to_archive(kind)?;
            let ns = ensure_facet_workspace(loom, workspace, FacetKind::Files)?;
            let mut options = loom_interchange_io::ArchiveImportOptions::new(workspace);
            options.commit = commit;
            options.dry_run = dry_run;
            loom_interchange_io::import_archive_bytes(
                loom,
                ns,
                bytes,
                std::path::Path::new("transfer"),
                archive_kind,
                &options,
            )
            .map_err(|e| e.to_string())?
            .report
        }
    };
    if !dry_run {
        save_loom(loom).map_err(|e| e.to_string())?;
    }
    Ok(report)
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

/// A one-line summary of a typed import-report (local byte-transfer path).
fn summary_from_report(r: &loom_interchange::ImportReport) -> String {
    format!(
        "imported: profile={}, objects_added={}, bytes_in={}, dry_run={}",
        r.profile, r.objects_added, r.bytes_in, r.dry_run
    )
}

/// A one-line summary of an import-report CBOR (remote byte-transfer path). The canonical
/// `loom.interchange.import-report.v1` array is `[profile, source_scope, commit, objects_added,
/// bytes_in, bytes_stored, rows_imported, skipped, operations_planned, operations_applied, dry_run,
/// warnings, fidelity_issues]`.
fn summary_from_report_cbor(bytes: &[u8]) -> Result<String, String> {
    use loom_codec::Value;
    let Value::Array(items) = loom_codec::decode(bytes).map_err(|e| e.to_string())? else {
        return Err("import-report is not a CBOR array".to_string());
    };
    let text = |i: usize| match items.get(i) {
        Some(Value::Text(t)) => t.clone(),
        _ => String::new(),
    };
    let uint = |i: usize| match items.get(i) {
        Some(Value::Uint(n)) => *n,
        _ => 0,
    };
    let dry_run = matches!(items.get(10), Some(Value::Bool(true)));
    Ok(format!(
        "imported: profile={}, objects_added={}, bytes_in={}, dry_run={}",
        text(0),
        uint(3),
        uint(4),
        dry_run
    ))
}

/// Resolve the `SessionAuth` for a remote endpoint from its `target.auth` selector. The selector is a
/// non-secret principal id (never credential material); the passphrase is acquired at connect time via the
/// interactive prompt and never stored in locator/config files. No selector means an unauthenticated
/// session; a bad passphrase fails at session open, not later at mutation time.
#[cfg(feature = "remote-client")]
fn remote_session_auth(target: &RemoteTarget) -> Result<SessionAuth, String> {
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
    /// Store `value` under the typed `key` in `collection` of `workspace` (KV put).
    pub(crate) fn kv_put(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        key: loom_core::Value,
        value: Vec<u8>,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Kv)?;
                loom_core::kv_put(&mut loom, ns, collection, key, value)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Kv::put(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
                loom_core::kv::key_to_cbor(&key),
                value,
            )),
        }
    }

    /// Read the value under the typed `key` in `collection` of `workspace` (KV get), or `None` when
    /// absent.
    pub(crate) fn kv_get(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        key: loom_core::Value,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::kv_get(&loom, ns, collection, &key).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Kv::get(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
                loom_core::kv::key_to_cbor(&key),
            )),
        }
    }

    /// The canonical-CBOR `[key, value]` list for `collection` of `workspace` (KV list).
    pub(crate) fn kv_list(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let map = loom_core::kv_list(&loom, ns, collection).map_err(|e| e.to_string())?;
                Ok(map.encode())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Kv::list(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
            )),
        }
    }

    /// Delete the typed `key` from `collection` of `workspace` (KV delete); returns whether it existed.
    pub(crate) fn kv_delete(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        key: loom_core::Value,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let present = loom_core::kv_delete(&mut loom, ns, collection, &key)
                    .map_err(|e| e.to_string())?;
                if present {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(present)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Kv::delete(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
                loom_core::kv::key_to_cbor(&key),
            )),
        }
    }

    /// The canonical-CBOR `[key, value]` list for `[from, to]` of `collection` in `workspace` (KV range).
    pub(crate) fn kv_range(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        from: loom_core::Value,
        to: loom_core::Value,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let map = loom_core::kv_range(&loom, ns, collection, &from, &to)
                    .map_err(|e| e.to_string())?;
                Ok(map.encode())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Kv::range(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
                loom_core::kv::key_to_cbor(&from),
                loom_core::kv::key_to_cbor(&to),
            )),
        }
    }

    /// Append `entry` to `stream` of `workspace` (queue append), returning the assigned sequence.
    pub(crate) fn queue_append(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        stream: &str,
        entry: Vec<u8>,
    ) -> Result<u64, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Queue)?;
                let seq = loom_core::log::append(&mut loom, ns, stream, &entry)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                Ok(seq as u64)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Queue::append(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                stream.to_string(),
                entry,
            )),
        }
    }

    /// The entries in `[from, to)` of `stream` in `workspace` (queue range).
    pub(crate) fn queue_range(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        stream: &str,
        from: u64,
        to: u64,
    ) -> Result<Vec<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::log::range(&loom, ns, stream, from as usize, to as usize)
                    .map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Queue::range(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                stream.to_string(),
                from,
                to,
            )),
        }
    }

    /// The entry at `seq` of `stream` in `workspace` (queue get), or `None` when absent.
    pub(crate) fn queue_get(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        stream: &str,
        seq: usize,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::log::get(&loom, ns, stream, seq).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Queue::get(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                stream.to_string(),
                seq as u64,
            )),
        }
    }

    /// The number of entries in `stream` of `workspace` (queue len).
    pub(crate) fn queue_len(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        stream: &str,
    ) -> Result<usize, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::log::len(&loom, ns, stream).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let len = remote.block(Queue::len(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    stream.to_string(),
                ))?;
                Ok(len as usize)
            }
        }
    }

    /// The named consumer's next sequence in `stream` of `workspace` (queue position).
    pub(crate) fn queue_consumer_position(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        stream: &str,
        consumer: &str,
    ) -> Result<u64, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::log::consumer_position(&loom, ns, stream, consumer)
                    .map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(QueueConsumers::consumer_position(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                stream.to_string(),
                consumer.to_string(),
            )),
        }
    }

    /// Up to `max` entries from the named consumer's position in `stream` of `workspace`, without
    /// advancing (queue read).
    pub(crate) fn queue_consumer_read(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        stream: &str,
        consumer: &str,
        max: usize,
    ) -> Result<Vec<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::log::consumer_read(&loom, ns, stream, consumer, max)
                    .map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(QueueConsumers::consumer_read(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                stream.to_string(),
                consumer.to_string(),
                max as u32,
            )),
        }
    }

    /// Advance the named consumer's next sequence to `next` in `stream` of `workspace` (queue advance);
    /// rejects backward movement.
    pub(crate) fn queue_consumer_advance(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        stream: &str,
        consumer: &str,
        next: u64,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::log::consumer_advance(&mut loom, ns, stream, consumer, next)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(QueueConsumers::consumer_advance(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                stream.to_string(),
                consumer.to_string(),
                next,
            )),
        }
    }

    /// Set the named consumer's next sequence to `next` in `stream` of `workspace` (queue reset), which
    /// may move backward.
    pub(crate) fn queue_consumer_reset(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        stream: &str,
        consumer: &str,
        next: u64,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::log::consumer_reset(&mut loom, ns, stream, consumer, next)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(QueueConsumers::consumer_reset(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                stream.to_string(),
                consumer.to_string(),
                next,
            )),
        }
    }

    /// Store `content` in the content-addressed facet of `workspace`, returning its digest (CAS put).
    pub(crate) fn cas_put(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        content: Vec<u8>,
    ) -> Result<Digest, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Cas)?;
                let digest =
                    loom_core::cas_put(&mut loom, ns, &content).map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                Ok(digest)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Cas::put(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    content,
                ))?;
                Digest::parse(&wire.0).map_err(|e| e.to_string())
            }
        }
    }

    /// Read the blob at `digest` in `workspace` (CAS get), or `None` when absent.
    pub(crate) fn cas_get(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        digest: &Digest,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::cas_get(&loom, ns, digest).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Cas::get(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                loom_remote_protocol::api_types::Digest(digest.to_string()),
            )),
        }
    }

    /// Whether the blob at `digest` is present in `workspace` (CAS has).
    pub(crate) fn cas_has(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        digest: &Digest,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::cas_has(&loom, ns, digest).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Cas::has(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                loom_remote_protocol::api_types::Digest(digest.to_string()),
            )),
        }
    }

    /// Unlink the blob at `digest` from `workspace`'s working tree (CAS delete); returns whether present.
    pub(crate) fn cas_delete(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        digest: &Digest,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let present =
                    loom_core::cas_delete(&mut loom, ns, digest).map_err(|e| e.to_string())?;
                if present {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(present)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Cas::delete(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                loom_remote_protocol::api_types::Digest(digest.to_string()),
            )),
        }
    }

    /// The digests reachable in `workspace`'s content-addressed working tree (CAS list).
    pub(crate) fn cas_list(&self, keys: &KeyOpts, workspace: &str) -> Result<Vec<Digest>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::cas_list(&loom, ns).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Cas::list(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                ))?;
                wire.into_iter()
                    .map(|d| Digest::parse(&d.0).map_err(|e| e.to_string()))
                    .collect()
            }
        }
    }

    /// Store UTF-8 `text` under document `id` in `collection` of `workspace`.
    pub(crate) fn doc_put_text(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        id: &str,
        text: &str,
        expected_entity_tag: Option<&str>,
    ) -> Result<loom_core::document::DocumentPutResult, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Document)?;
                let result = loom_core::document::document_put_text_with_entity_tag(
                    &mut loom,
                    ns,
                    collection,
                    id,
                    text,
                    expected_entity_tag,
                )
                .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                Ok(result)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote
                .doc_put_binary_guarded(
                    workspace,
                    collection,
                    id,
                    text.as_bytes().to_vec(),
                    expected_entity_tag,
                )
                .map(|digest| loom_core::document::DocumentPutResult {
                    entity_tag: loom_core::document_entity_tag_string_from_digest(digest),
                    digest,
                }),
        }
    }

    /// Read document `id` as UTF-8 text in `collection` of `workspace`, or `None` when absent.
    pub(crate) fn doc_get_text(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<loom_core::document::DocumentText>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::document::document_get_text(&loom, ns, collection, id)
                    .map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.doc_get_text(workspace, collection, id),
        }
    }

    /// Store raw `content` under document `id` in `collection` of `workspace`.
    pub(crate) fn doc_put_binary(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        id: &str,
        content: Vec<u8>,
        expected_entity_tag: Option<&str>,
    ) -> Result<loom_core::document::DocumentPutResult, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Document)?;
                let result = loom_core::document::document_put_binary_with_entity_tag(
                    &mut loom,
                    ns,
                    collection,
                    id,
                    content,
                    expected_entity_tag,
                )
                .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                Ok(result)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote
                .doc_put_binary_guarded(workspace, collection, id, content, expected_entity_tag)
                .map(|digest| loom_core::document::DocumentPutResult {
                    entity_tag: loom_core::document_entity_tag_string_from_digest(digest),
                    digest,
                }),
        }
    }

    /// Read raw document bytes in `collection` of `workspace`, or `None` when absent.
    pub(crate) fn doc_get_binary(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<loom_core::document::DocumentBinary>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::document::document_get_binary(&loom, ns, collection, id)
                    .map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.doc_get_binary(workspace, collection, id),
        }
    }

    /// Delete document `id` in `collection` of `workspace` (Document delete); returns whether present.
    pub(crate) fn doc_delete(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let present = loom_core::document::doc_delete(&mut loom, ns, collection, id)
                    .map_err(|e| e.to_string())?;
                if present {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(present)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Document::delete(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
                id.to_string(),
            )),
        }
    }

    /// The canonical-CBOR binary document list for `collection` of `workspace`.
    pub(crate) fn doc_list_binary(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let docs = loom_core::document::doc_list(&loom, ns, collection)
                    .map_err(|e| e.to_string())?;
                Ok(docs.encode())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Document::list_binary(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
            )),
        }
    }

    pub(crate) fn doc_index_create(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        name: &str,
        path: &str,
        unique: bool,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Document)?;
                let index = loom_core::document::DocumentIndexDef::new(
                    name,
                    loom_core::document::DocumentFieldPath::dotted(path)
                        .map_err(|e| e.to_string())?,
                    unique,
                )
                .map_err(|e| e.to_string())?;
                loom_core::document::doc_create_index(&mut loom, ns, collection, index)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Document::index_create(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
                name.to_string(),
                path.to_string(),
                unique,
            )),
        }
    }

    pub(crate) fn doc_index_create_json(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        declaration_json: &[u8],
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Document)?;
                let value: serde_json::Value =
                    serde_json::from_slice(declaration_json).map_err(|e| e.to_string())?;
                let declaration = loom_core::document_index_declaration_from_json(&value)
                    .map_err(|e| e.to_string())?;
                loom_core::document::doc_create_index_declaration(
                    &mut loom,
                    ns,
                    collection,
                    declaration,
                )
                .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Document::index_create_json(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
                declaration_json.to_vec(),
            )),
        }
    }

    pub(crate) fn doc_index_drop(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        name: &str,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let dropped = loom_core::document::doc_drop_index(&mut loom, ns, collection, name)
                    .map_err(|e| e.to_string())?;
                if dropped {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(dropped)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Document::index_drop(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
                name.to_string(),
            )),
        }
    }

    /// The `{"indexes":[{name, path, unique}]}` JSON the CLI prints (Document index list). Both arms
    /// format through `document_indexes_json` (the remote arm decodes the server's identical bytes).
    pub(crate) fn doc_index_list(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
    ) -> Result<serde_json::Value, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let indexes =
                    loom_core::document::doc_list_index_declarations(&loom, ns, collection)
                        .map_err(|e| e.to_string())?;
                Ok(loom_core::document::document_index_declarations_json(
                    indexes,
                ))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let bytes = remote.block(Document::index_list_json(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    collection.to_string(),
                ))?;
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())
            }
        }
    }

    /// The `{"indexes":[{name, ready, entries}]}` JSON the CLI prints (Document index status). Both arms
    /// format through `document_index_statuses_json` (the remote arm decodes the server's identical bytes).
    pub(crate) fn doc_index_statuses(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
    ) -> Result<serde_json::Value, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let statuses = loom_core::document::doc_index_statuses(&loom, ns, collection)
                    .map_err(|e| e.to_string())?;
                Ok(loom_core::document::document_index_statuses_json(statuses))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let bytes = remote.block(Document::index_status_json(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    collection.to_string(),
                ))?;
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())
            }
        }
    }

    pub(crate) fn doc_index_rebuild(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        name: &str,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::document::doc_rebuild_index(&mut loom, ns, collection, name)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Document::index_rebuild(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
                name.to_string(),
            )),
        }
    }

    /// The ids matching `value` in `index` of `collection` (Document find). `value_json` is the raw JSON
    /// the user supplied; the local arm parses it, the remote arm forwards it and decodes the id array.
    pub(crate) fn doc_find(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        index: &str,
        value_json: &str,
    ) -> Result<Vec<String>, String> {
        match self {
            StoreClient::Local { locator } => {
                let value = serde_json::from_str::<serde_json::Value>(value_json)
                    .map_err(|e| format!("document index value must be JSON: {e}"))?;
                let value = loom_core::document::document_index_value_from_json(&value)
                    .map_err(|e| e.to_string())?;
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::document::doc_find(&loom, ns, collection, index, &value)
                    .map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let bytes = remote.block(Document::find_json(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    collection.to_string(),
                    index.to_string(),
                    value_json.as_bytes().to_vec(),
                ))?;
                serde_json::from_slice::<Vec<String>>(&bytes).map_err(|e| e.to_string())
            }
        }
    }

    /// The `{"items":[...], "next_cursor":...}` JSON the CLI prints (Document query). `query_json` is the
    /// raw JSON the user supplied; both arms format through `document_query_result_json` (the remote arm
    /// decodes the server's identical bytes).
    pub(crate) fn doc_query(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        query_json: &[u8],
    ) -> Result<serde_json::Value, String> {
        match self {
            StoreClient::Local { locator } => {
                let query = serde_json::from_slice::<serde_json::Value>(query_json)
                    .map_err(|e| format!("document query must be JSON: {e}"))?;
                let query = loom_core::document::document_query_from_json(&query)
                    .map_err(|e| e.to_string())?;
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let result = loom_core::document::doc_query(&loom, ns, collection, &query)
                    .map_err(|e| e.to_string())?;
                Ok(loom_core::document::document_query_result_json(result))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let bytes = remote.block(Document::query_json(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    collection.to_string(),
                    query_json.to_vec(),
                ))?;
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())
            }
        }
    }

    /// Append `payload` to the hash-linked `collection` of `workspace` (Ledger append), returning its seq.
    pub(crate) fn ledger_append(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        payload: Vec<u8>,
    ) -> Result<u64, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Ledger)?;
                let seq = loom_core::ledger_append(&mut loom, ns, collection, payload)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                Ok(seq)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Ledger::append(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
                payload,
            )),
        }
    }

    /// The payload at `seq` in `collection` of `workspace` (Ledger get), or `None` when absent.
    pub(crate) fn ledger_get(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
        seq: u64,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::ledger_get(&loom, ns, collection, seq).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Ledger::get(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
                seq,
            )),
        }
    }

    /// The head digest of `collection` in `workspace` (Ledger head), or `None` when empty.
    pub(crate) fn ledger_head(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
    ) -> Result<Option<Digest>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::ledger_head(&loom, ns, collection).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let head = remote.block(Ledger::head(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    collection.to_string(),
                ))?;
                match head {
                    Some(d) => Ok(Some(Digest::parse(&d.0).map_err(|e| e.to_string())?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// The number of entries in `collection` of `workspace` (Ledger len).
    pub(crate) fn ledger_len(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
    ) -> Result<u64, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::ledger_len(&loom, ns, collection).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Ledger::len(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
            )),
        }
    }

    /// Verify the hash-linked integrity of `collection` in `workspace` (Ledger verify).
    pub(crate) fn ledger_verify(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        collection: &str,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::ledger_verify(&loom, ns, collection).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Ledger::verify(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                collection.to_string(),
            )),
        }
    }

    /// Store `value` at `ts` in `series` of `workspace` (TimeSeries put).
    pub(crate) fn ts_put(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        series: &str,
        ts: i64,
        value: Vec<u8>,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::TimeSeries)?;
                loom_core::ts_put(&mut loom, ns, series, ts, value).map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(TimeSeries::put(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                series.to_string(),
                ts,
                value,
            )),
        }
    }

    /// The value at `ts` in `series` of `workspace` (TimeSeries get), or `None` when absent.
    pub(crate) fn ts_get(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        series: &str,
        ts: i64,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::ts_get(&loom, ns, series, ts).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(TimeSeries::get(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                series.to_string(),
                ts,
            )),
        }
    }

    /// The canonical-CBOR series of points in `[from, to]` of `series` in `workspace` (TimeSeries range).
    pub(crate) fn ts_range(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        series: &str,
        from: i64,
        to: i64,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let result =
                    loom_core::ts_range(&loom, ns, series, from, to).map_err(|e| e.to_string())?;
                Ok(result.encode())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(TimeSeries::range(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                series.to_string(),
                from,
                to,
            )),
        }
    }

    /// Create search index `name` in `workspace` from the canonical-CBOR `mapping` (Search create).
    ///
    /// `mapping` is the canonical-CBOR field mapping crossing the wire unchanged; the local arm decodes
    /// it with the shared [`search_mapping_from_cbor`] bridge before applying it to the engine.
    pub(crate) fn search_create(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        mapping: Vec<u8>,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mapping = search_mapping_from_cbor(&mapping)?;
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Search)?;
                loom_core::search_create(&mut loom, ns, name, mapping)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Search::create(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                mapping,
            )),
        }
    }

    /// Index the canonical-CBOR `doc` under `id` in search index `name` of `workspace` (Search index).
    pub(crate) fn search_index(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: Vec<u8>,
        doc: Vec<u8>,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let doc = search_document_from_cbor(&doc)?;
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::search_index(&mut loom, ns, name, id, doc).map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Search::index(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id,
                doc,
            )),
        }
    }

    /// Read the canonical-CBOR document under `id` in search index `name` of `workspace` (Search get),
    /// or `None` when absent.
    pub(crate) fn search_get(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                match loom_core::search_get(&loom, ns, name, &id).map_err(|e| e.to_string())? {
                    Some(doc) => Ok(Some(search_document_cbor(&doc)?)),
                    None => Ok(None),
                }
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Search::get(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id,
            )),
        }
    }

    /// Delete document `id` from search index `name` of `workspace` (Search delete); returns whether
    /// it was present.
    pub(crate) fn search_delete(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: Vec<u8>,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let present = loom_core::search_delete(&mut loom, ns, name, &id)
                    .map_err(|e| e.to_string())?;
                if present {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(present)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Search::delete(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id,
            )),
        }
    }

    /// The canonical-CBOR id list for search index `name` of `workspace`, optionally filtered by
    /// `prefix` (Search ids).
    pub(crate) fn search_ids(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        prefix: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let ids = loom_core::search_ids(&loom, ns, name, prefix.as_deref())
                    .map_err(|e| e.to_string())?;
                search_ids_cbor(ids)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Search::ids(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                prefix.clone().unwrap_or_default(),
                prefix.is_some(),
            )),
        }
    }

    /// Replace the field mapping of search index `name` in `workspace` with the canonical-CBOR
    /// `mapping` (Search remap).
    pub(crate) fn search_remap(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        mapping: Vec<u8>,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mapping = search_mapping_from_cbor(&mapping)?;
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::search_remap(&mut loom, ns, name, mapping).map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Search::remap(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                mapping,
            )),
        }
    }

    /// Execute the canonical-CBOR `request` against search index `name` of `workspace`, returning the
    /// canonical-CBOR response (Search query).
    pub(crate) fn search_query(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        request: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let request = search_request_from_cbor(&request)?;
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let response = loom_core::search_query(&loom, ns, name, &request)
                    .map_err(|e| e.to_string())?;
                search_response_cbor(&response)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Search::query(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                request,
            )),
        }
    }

    /// The full search-index status of `name` at `engine_version`: `(workspace_display, source_digest,
    /// DerivedArtifactStatus)` about the served store's derived tantivy artifact. The remote arm decodes
    /// the `[source_digest, status]` wire payload from `Search.status`.
    pub(crate) fn search_status(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        engine_version: &str,
    ) -> Result<(String, Digest, DerivedArtifactStatus), String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let source_digest =
                    loom_core::search_source_digest(&loom, ns, name).map_err(|e| e.to_string())?;
                let status = loom
                    .store()
                    .search_tantivy_status(ns, name, source_digest, engine_version)
                    .map_err(|e| e.to_string())?;
                Ok((ns.to_string(), source_digest, status))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Search::status(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    name.to_string(),
                    engine_version.to_string(),
                ))?;
                let (source_digest, status) =
                    loom_store::decode_search_status_result(&wire).map_err(|e| e.to_string())?;
                Ok((workspace.to_string(), source_digest, status))
            }
        }
    }

    // ---- Calendar ----
    //
    // The CLI's calendar output uses presentation encoders (`calendar_collection_cbor`,
    // `calendar_range_cbor`, `record_array_cbor`, `text_array_cbor`) that differ in shape from the
    // canonical wire encoders the server emits. So the remote arm of each such method decodes the
    // canonical server response and re-encodes it with the same CLI presentation encoder the local arm
    // uses, yielding identical output for a local and a remote locator.

    /// Create calendar `collection` for `principal` (Calendar create_collection). `display_name` and
    /// `component_set` are the CLI args; the remote arm encodes them as the canonical `CollectionMeta`.
    pub(crate) fn cal_create_collection(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        collection: &str,
        display_name: String,
        component_set: Vec<loom_core::calendar::Component>,
    ) -> Result<(), String> {
        let meta = loom_core::calendar::CollectionMeta {
            display_name,
            component_set,
        };
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Calendar)?;
                loom_core::calendar::create_collection(&mut loom, ns, principal, collection, &meta)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Calendar::create_collection(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                collection.to_string(),
                meta.encode(),
            )),
        }
    }

    /// Delete calendar `collection` for `principal` (Calendar delete_collection); returns whether present.
    pub(crate) fn cal_delete_collection(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let present =
                    loom_core::calendar::delete_collection(&mut loom, ns, principal, collection)
                        .map_err(|e| e.to_string())?;
                if present {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(present)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Calendar::delete_collection(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                collection.to_string(),
            )),
        }
    }

    /// Delete calendar entry `uid` in `collection` for `principal` (Calendar delete_entry); returns
    /// whether present.
    pub(crate) fn cal_delete_entry(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let present =
                    loom_core::calendar::delete_entry(&mut loom, ns, principal, collection, uid)
                        .map_err(|e| e.to_string())?;
                if present {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(present)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Calendar::delete_entry(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                collection.to_string(),
                uid.to_string(),
            )),
        }
    }

    /// The CLI presentation bytes for calendar `collection` of `principal` (Calendar get_collection), or
    /// `None` when absent. DIVERGENT: the remote arm decodes the canonical `CollectionMeta` and re-encodes
    /// it with the CLI's `calendar_collection_cbor`.
    pub(crate) fn cal_get_collection(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                match loom_core::calendar::get_collection(&loom, ns, principal, collection)
                    .map_err(|e| e.to_string())?
                {
                    Some(meta) => Ok(Some(calendar_collection_cbor(&meta)?)),
                    None => Ok(None),
                }
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Calendar::get_collection(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    collection.to_string(),
                ))?;
                match wire {
                    Some(bytes) => Ok(Some(cli_calendar_collection_from_remote(&bytes)?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// The canonical-CBOR calendar entry `uid` in `collection` of `principal` (Calendar get_entry), or
    /// `None` when absent. CLEAN: local and remote both use `CalendarEntry::encode`.
    pub(crate) fn cal_get_entry(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                Ok(
                    loom_core::calendar::get_entry(&loom, ns, principal, collection, uid)
                        .map_err(|e| e.to_string())?
                        .map(|entry| entry.encode()),
                )
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Calendar::get_entry(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                collection.to_string(),
                uid.to_string(),
            )),
        }
    }

    /// The calendar collection ids for `principal` (Calendar list_collections). CLEAN: the remote arm
    /// decodes the canonical string list, matching the CLI's `text_array_cbor`/line output.
    pub(crate) fn cal_list_collections(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
    ) -> Result<Vec<String>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::calendar::list_collections(&loom, ns, principal)
                    .map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Calendar::list_collections(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                ))?;
                cli_strings_from_remote(&wire)
            }
        }
    }

    /// The CLI presentation bytes for the entries of calendar `collection` (Calendar list_entries).
    /// DIVERGENT: the remote arm re-encodes the canonical byte-blob list with the CLI's
    /// `record_array_cbor`.
    pub(crate) fn cal_list_entries(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        collection: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let entries = loom_core::calendar::list_entries(&loom, ns, principal, collection)
                    .map_err(|e| e.to_string())?;
                record_array_cbor(entries.into_iter().map(|entry| entry.encode()))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Calendar::list_entries(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    collection.to_string(),
                ))?;
                cli_record_array_from_remote(&wire)
            }
        }
    }

    /// Put the canonical-CBOR calendar `entry` in `collection` of `principal` (Calendar put_entry),
    /// returning the etag string the CLI prints. CLEAN: the remote `Digest` string equals the local etag.
    pub(crate) fn cal_put_entry(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        collection: &str,
        entry: Vec<u8>,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let entry = loom_core::calendar::CalendarEntry::decode(&entry)
                    .map_err(|e| e.to_string())?;
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Calendar)?;
                let etag =
                    loom_core::calendar::put_entry(&mut loom, ns, principal, collection, &entry)
                        .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                Ok(etag.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Calendar::put_entry(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    collection.to_string(),
                    entry,
                ))?;
                Ok(wire.0)
            }
        }
    }

    /// Import an iCalendar document into `collection` (Calendar `put_ics`), returning the etag string.
    pub(crate) fn cal_put_ics(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        collection: &str,
        ics: String,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Calendar)?;
                let etag = loom_core::calendar::put_ics(&mut loom, ns, principal, collection, &ics)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                Ok(etag.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Calendar::put_ics(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    collection.to_string(),
                    ics,
                ))?;
                Ok(wire.0)
            }
        }
    }

    /// The CLI presentation bytes for the occurrences of `collection` in `[from, to)` (Calendar range).
    /// `from`/`to` are the raw CLI date-time args. DIVERGENT: the remote arm normalizes the bounds to the
    /// wire form, then reconstructs `Occurrence`s from the canonical response and re-encodes them with the
    /// CLI's `calendar_range_cbor`.
    pub(crate) fn cal_range(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        collection: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let from = parse_calendar_datetime(from)?;
                let to = parse_calendar_datetime(to)?;
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let entries =
                    loom_core::calendar::range(&loom, ns, principal, collection, from, to)
                        .map_err(|e| e.to_string())?;
                calendar_range_cbor(&entries)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                // The server parses bounds as a 15-char `YYYYMMDDTHHMMSS` wall-clock string; normalize the
                // CLI args (which also accept bare `YYYYMMDD`) so remote accepts the same inputs as local.
                let from = cli_window_bound(from)?;
                let to = cli_window_bound(to)?;
                let wire = remote.block(Calendar::range(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    collection.to_string(),
                    from,
                    to,
                ))?;
                cli_calendar_range_from_remote(&wire)
            }
        }
    }

    /// The CLI presentation bytes for the entries of `collection` matching `component`/`text` (Calendar
    /// search). DIVERGENT: same re-encode as `cal_list_entries`.
    pub(crate) fn cal_search(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        collection: &str,
        component: Option<loom_core::calendar::Component>,
        text: Option<String>,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let entries = loom_core::calendar::search(
                    &loom,
                    ns,
                    principal,
                    collection,
                    component,
                    text.as_deref(),
                )
                .map_err(|e| e.to_string())?;
                record_array_cbor(entries.into_iter().map(|entry| entry.encode()))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let component = component
                    .map(|c| c.as_str().to_string())
                    .unwrap_or_default();
                let wire = remote.block(Calendar::search(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    collection.to_string(),
                    component,
                    text.unwrap_or_default(),
                ))?;
                cli_record_array_from_remote(&wire)
            }
        }
    }

    /// The iCalendar text for entry `uid` in `collection` of `principal` (Calendar to_ics), or `None`
    /// when absent. CLEAN: the local arm's ics string bytes equal the server's ics bytes.
    pub(crate) fn cal_to_ics(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        collection: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                Ok(
                    loom_core::calendar::entry_ics(&loom, ns, principal, collection, uid)
                        .map_err(|e| e.to_string())?
                        .map(String::into_bytes),
                )
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Calendar::to_ics(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                collection.to_string(),
                uid.to_string(),
            )),
        }
    }

    // ---- Contacts ----
    //
    // Mirrors Calendar: the CLI presents `get_book` metadata with `metadata_cbor` and entry lists with
    // `record_array_cbor`, both of which differ from the server's canonical wire form, so the remote arm
    // re-encodes those. `list_books`/`get_entry`/`put_entry`/`create_book`/`delete_*`/`to_vcard` forward
    // directly.

    /// Create contacts `book` for `principal` (Contacts create_book). The remote arm encodes the CLI's
    /// `display_name` as the canonical `BookMeta`.
    pub(crate) fn con_create_book(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        book: &str,
        display_name: String,
    ) -> Result<(), String> {
        let meta = loom_core::contacts::BookMeta { display_name };
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Contacts)?;
                loom_core::contacts::create_book(&mut loom, ns, principal, book, &meta)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Contacts::create_book(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                book.to_string(),
                meta.encode(),
            )),
        }
    }

    /// Delete contacts `book` for `principal` (Contacts delete_book); returns whether present.
    pub(crate) fn con_delete_book(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        book: &str,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let present = loom_core::contacts::delete_book(&mut loom, ns, principal, book)
                    .map_err(|e| e.to_string())?;
                if present {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(present)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Contacts::delete_book(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                book.to_string(),
            )),
        }
    }

    /// Delete contacts entry `uid` in `book` for `principal` (Contacts delete_entry); returns whether
    /// present.
    pub(crate) fn con_delete_entry(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let present =
                    loom_core::contacts::delete_entry(&mut loom, ns, principal, book, uid)
                        .map_err(|e| e.to_string())?;
                if present {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(present)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Contacts::delete_entry(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                book.to_string(),
                uid.to_string(),
            )),
        }
    }

    /// The CLI presentation bytes for contacts `book` of `principal` (Contacts get_book), or `None` when
    /// absent. DIVERGENT: the remote arm decodes the canonical `BookMeta` and re-encodes it with the CLI's
    /// `metadata_cbor`.
    pub(crate) fn con_get_book(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        book: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                match loom_core::contacts::get_book(&loom, ns, principal, book)
                    .map_err(|e| e.to_string())?
                {
                    Some(meta) => Ok(Some(metadata_cbor(&meta.display_name)?)),
                    None => Ok(None),
                }
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Contacts::get_book(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    book.to_string(),
                ))?;
                match wire {
                    Some(bytes) => Ok(Some(cli_contacts_book_from_remote(&bytes)?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// The canonical-CBOR contacts entry `uid` in `book` of `principal` (Contacts get_entry), or `None`
    /// when absent. CLEAN: local and remote both use `ContactEntry::encode`.
    pub(crate) fn con_get_entry(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                Ok(
                    loom_core::contacts::get_entry(&loom, ns, principal, book, uid)
                        .map_err(|e| e.to_string())?
                        .map(|entry| entry.encode()),
                )
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Contacts::get_entry(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                book.to_string(),
                uid.to_string(),
            )),
        }
    }

    /// The contacts book ids for `principal` (Contacts list_books). CLEAN: the remote arm decodes the
    /// canonical string list, matching the CLI's `text_array_cbor`/line output.
    pub(crate) fn con_list_books(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
    ) -> Result<Vec<String>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::contacts::list_books(&loom, ns, principal).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Contacts::list_books(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                ))?;
                cli_strings_from_remote(&wire)
            }
        }
    }

    /// The CLI presentation bytes for the entries of contacts `book` (Contacts list_entries). DIVERGENT:
    /// same re-encode as Calendar's `list_entries`.
    pub(crate) fn con_list_entries(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        book: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let entries = loom_core::contacts::list_entries(&loom, ns, principal, book)
                    .map_err(|e| e.to_string())?;
                record_array_cbor(entries.into_iter().map(|entry| entry.encode()))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Contacts::list_entries(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    book.to_string(),
                ))?;
                cli_record_array_from_remote(&wire)
            }
        }
    }

    /// Put the canonical-CBOR contacts `entry` in `book` of `principal` (Contacts put_entry), returning
    /// the etag string the CLI prints. CLEAN: the remote `Digest` string equals the local etag.
    pub(crate) fn con_put_entry(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        book: &str,
        entry: Vec<u8>,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let entry =
                    loom_core::contacts::ContactEntry::decode(&entry).map_err(|e| e.to_string())?;
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Contacts)?;
                let etag = loom_core::contacts::put_entry(&mut loom, ns, principal, book, &entry)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                Ok(etag.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Contacts::put_entry(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    book.to_string(),
                    entry,
                ))?;
                Ok(wire.0)
            }
        }
    }

    /// Import a vCard document into `book` (Contacts `put_vcard`), returning the etag string.
    pub(crate) fn con_put_vcard(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        book: &str,
        vcard: String,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Contacts)?;
                let etag = loom_core::contacts::put_vcard(&mut loom, ns, principal, book, &vcard)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                Ok(etag.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Contacts::put_vcard(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    book.to_string(),
                    vcard,
                ))?;
                Ok(wire.0)
            }
        }
    }

    /// The CLI presentation bytes for the entries of `book` matching `text` (Contacts search). DIVERGENT:
    /// same re-encode as `con_list_entries`.
    pub(crate) fn con_search(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        book: &str,
        text: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let entries = loom_core::contacts::search(&loom, ns, principal, book, text)
                    .map_err(|e| e.to_string())?;
                record_array_cbor(entries.into_iter().map(|entry| entry.encode()))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Contacts::search(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    book.to_string(),
                    text.to_string(),
                ))?;
                cli_record_array_from_remote(&wire)
            }
        }
    }

    /// The vCard text for entry `uid` in `book` of `principal` (Contacts to_vcard), or `None` when absent.
    /// CLEAN: the local arm's vCard string bytes equal the server's vCard bytes.
    pub(crate) fn con_to_vcard(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        book: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                Ok(
                    loom_core::contacts::entry_vcard(&loom, ns, principal, book, uid)
                        .map_err(|e| e.to_string())?
                        .map(String::into_bytes),
                )
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Contacts::to_vcard(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                book.to_string(),
                uid.to_string(),
            )),
        }
    }

    // ---- Mail ----
    //
    // Same Option-1 shape as Calendar/Contacts: `get_mailbox` metadata and message lists diverge from the
    // canonical wire form and are re-encoded in the remote arm; the rest are clean direct-forward. Flag
    // lists cross as canonical `Array(Text)` (CLI `text_array_cbor` == server `string_list_to_cbor`).

    /// Create `mailbox` for `principal` (Mail create_mailbox). The remote arm encodes the CLI's
    /// `display_name` as the canonical `MailboxMeta`.
    pub(crate) fn mail_create_mailbox(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        display_name: String,
    ) -> Result<(), String> {
        let meta = loom_core::mail::MailboxMeta { display_name };
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Mail)?;
                loom_core::mail::create_mailbox(&mut loom, ns, principal, mailbox, &meta)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Mail::create_mailbox(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                mailbox.to_string(),
                meta.encode(),
            )),
        }
    }

    /// Delete `mailbox` for `principal` (Mail delete_mailbox); returns whether present.
    pub(crate) fn mail_delete_mailbox(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        mailbox: &str,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let present = loom_core::mail::delete_mailbox(&mut loom, ns, principal, mailbox)
                    .map_err(|e| e.to_string())?;
                if present {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(present)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Mail::delete_mailbox(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                mailbox.to_string(),
            )),
        }
    }

    /// Delete message `uid` in `mailbox` for `principal` (Mail delete_message); returns whether present.
    pub(crate) fn mail_delete_message(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let present =
                    loom_core::mail::delete_message(&mut loom, ns, principal, mailbox, uid)
                        .map_err(|e| e.to_string())?;
                if present {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(present)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Mail::delete_message(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                mailbox.to_string(),
                uid.to_string(),
            )),
        }
    }

    /// The flags on message `uid` in `mailbox` of `principal` (Mail get_flags). CLEAN: the remote arm
    /// decodes the canonical string list, matching the CLI's `text_array_cbor`/line output.
    pub(crate) fn mail_get_flags(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Vec<String>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::mail::get_flags(&loom, ns, principal, mailbox, uid)
                    .map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Mail::get_flags(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    mailbox.to_string(),
                    uid.to_string(),
                ))?;
                cli_strings_from_remote(&wire)
            }
        }
    }

    /// The CLI presentation bytes for `mailbox` of `principal` (Mail get_mailbox), or `None` when absent.
    /// DIVERGENT: the remote arm decodes the canonical `MailboxMeta` and re-encodes it with the CLI's
    /// `metadata_cbor`.
    pub(crate) fn mail_get_mailbox(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        mailbox: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                match loom_core::mail::get_mailbox(&loom, ns, principal, mailbox)
                    .map_err(|e| e.to_string())?
                {
                    Some(meta) => Ok(Some(metadata_cbor(&meta.display_name)?)),
                    None => Ok(None),
                }
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Mail::get_mailbox(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    mailbox.to_string(),
                ))?;
                match wire {
                    Some(bytes) => Ok(Some(cli_mail_mailbox_from_remote(&bytes)?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// The canonical-CBOR message `uid` in `mailbox` of `principal` (Mail get_message), or `None` when
    /// absent. CLEAN: local and remote both use `MailMessage::encode`.
    pub(crate) fn mail_get_message(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                Ok(
                    loom_core::mail::get_message(&loom, ns, principal, mailbox, uid)
                        .map_err(|e| e.to_string())?
                        .map(|message| message.encode()),
                )
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Mail::get_message(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                mailbox.to_string(),
                uid.to_string(),
            )),
        }
    }

    /// Ingest raw RFC822 `message` under `uid` in `mailbox` of `principal` (Mail ingest_message),
    /// returning the digest string the CLI prints. CLEAN: the remote `Digest` string equals the local one.
    pub(crate) fn mail_ingest_message(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
        message: Vec<u8>,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Mail)?;
                let digest = loom_core::mail::ingest_message(
                    &mut loom, ns, principal, mailbox, uid, &message,
                )
                .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                Ok(digest.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Mail::ingest_message(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    mailbox.to_string(),
                    uid.to_string(),
                    message,
                ))?;
                Ok(wire.0)
            }
        }
    }

    /// The mailbox ids for `principal` (Mail list_mailboxes). CLEAN: the remote arm decodes the canonical
    /// string list.
    pub(crate) fn mail_list_mailboxes(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
    ) -> Result<Vec<String>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::mail::list_mailboxes(&loom, ns, principal).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Mail::list_mailboxes(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                ))?;
                cli_strings_from_remote(&wire)
            }
        }
    }

    /// The CLI presentation bytes for the messages of `mailbox` (Mail list_messages). DIVERGENT: same
    /// re-encode as Calendar's `list_entries`.
    pub(crate) fn mail_list_messages(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        mailbox: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let messages = loom_core::mail::list_messages(&loom, ns, principal, mailbox)
                    .map_err(|e| e.to_string())?;
                record_array_cbor(messages.into_iter().map(|message| message.encode()))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Mail::list_messages(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    mailbox.to_string(),
                ))?;
                cli_record_array_from_remote(&wire)
            }
        }
    }

    /// The CLI presentation bytes for the messages of `mailbox` matching `text` (Mail search). DIVERGENT:
    /// same re-encode as `mail_list_messages`.
    pub(crate) fn mail_search(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        text: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let messages = loom_core::mail::search(&loom, ns, principal, mailbox, text)
                    .map_err(|e| e.to_string())?;
                record_array_cbor(messages.into_iter().map(|message| message.encode()))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Mail::search(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    mailbox.to_string(),
                    text.to_string(),
                ))?;
                cli_record_array_from_remote(&wire)
            }
        }
    }

    /// Set the `flags` on message `uid` in `mailbox` of `principal` (Mail set_flags). The remote arm
    /// encodes the flag list as canonical `Array(Text)` (`text_array_cbor`), which the server decodes with
    /// `string_list_from_cbor`.
    pub(crate) fn mail_set_flags(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
        flags: Vec<String>,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::mail::set_flags(&mut loom, ns, principal, mailbox, uid, &flags)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let encoded = text_array_cbor(&flags)?;
                remote.block(Mail::set_flags(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    principal.to_string(),
                    mailbox.to_string(),
                    uid.to_string(),
                    encoded,
                ))
            }
        }
    }

    /// The RFC822 (.eml) bytes for message `uid` in `mailbox` of `principal` (Mail to_eml), or `None`
    /// when absent. CLEAN: the local arm's eml bytes equal the server's.
    pub(crate) fn mail_to_eml(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        principal: &str,
        mailbox: &str,
        uid: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::mail::to_eml(&loom, ns, principal, mailbox, uid)
                    .map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Mail::to_eml(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                principal.to_string(),
                mailbox.to_string(),
                uid.to_string(),
            )),
        }
    }

    // ---- FileSystem (files read/write) ----
    //
    // `read_file`/`write_file` forward directly; raw file bytes cross unchanged.

    /// Read the staged bytes of `path` in the Files working tree of `workspace` (FileSystem read_file).
    pub(crate) fn fs_read_file(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        path: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom.read_file(ns, path).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(FileSystem::read_file(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                path.to_string(),
            )),
        }
    }

    /// Write `content` to `path` in the Files working tree of `workspace` (FileSystem write_file). The
    /// local arm ensures the Files facet and auto-creates the parent directory for a nested path; the
    /// remote arm forwards to `write_file`, whose server impl ensures the facet but does not create parent
    /// directories, so a nested remote write requires the parent to already exist.
    pub(crate) fn fs_write_file(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        path: &str,
        content: Vec<u8>,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                // Ensure the Files workspace, creating it on first write so `files write` does not require
                // a pre-existing workspace.
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Files)?;
                if let Some((parent, _)) = path.rsplit_once('/') {
                    loom.create_directory(ns, parent, true)
                        .map_err(|e| e.to_string())?;
                }
                loom.write_file(ns, path, &content, 0o100644)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(FileSystem::write_file(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                path.to_string(),
                content,
                0o100644,
            )),
        }
    }

    /// Whether this client targets a remote endpoint (used to route path-shaped commands to the
    /// byte-transfer contract for remote and keep the server-local/admin path for local).
    pub(crate) fn is_remote(&self) -> bool {
        match self {
            StoreClient::Local { .. } => false,
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(_) => true,
        }
    }

    // ---- StoreAdmin (server-owned store administration over remote, specs/0067 §13, task 640) ----
    // These facade methods serve the *remote* path only; local `store` admin commands keep their
    // existing FileStore-direct handlers (the caller branches on `is_remote`).

    /// Remote `store stat`: read the served store's maintenance snapshot as JSON.
    pub(crate) fn admin_stat_json(&self) -> Result<String, String> {
        match self {
            StoreClient::Local { .. } => Err(LOCAL_ADMIN_VIA_LOCAL_PATH.to_string()),
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let cbor = remote.block(StoreAdmin::store_stat(
                    &remote.client,
                    remote.handle.clone(),
                ))?;
                let stat = loom_wire::store_admin::store_stat_from_cbor(&cbor)
                    .map_err(|e| e.to_string())?;
                Ok(store_stat_json(&stat))
            }
        }
    }

    /// Remote `store policy` (get): read the served store policy as JSON.
    pub(crate) fn admin_policy_get_json(&self) -> Result<String, String> {
        match self {
            StoreClient::Local { .. } => Err(LOCAL_ADMIN_VIA_LOCAL_PATH.to_string()),
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let cbor = remote.block(StoreAdmin::store_policy_get(
                    &remote.client,
                    remote.handle.clone(),
                ))?;
                let r = loom_wire::store_admin::store_policy_result_from_cbor(&cbor)
                    .map_err(|e| e.to_string())?;
                Ok(store_policy_json(
                    loom_store::StorePolicy {
                        fips_required: r.fips_required,
                    },
                    r.audit_seq,
                ))
            }
        }
    }

    /// Remote `store policy` (set): audited set of the served store policy, returned as JSON.
    pub(crate) fn admin_policy_set_json(&self, fips_required: bool) -> Result<String, String> {
        match self {
            StoreClient::Local { .. } => Err(LOCAL_ADMIN_VIA_LOCAL_PATH.to_string()),
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let cbor = remote.block(StoreAdmin::store_policy_set(
                    &remote.client,
                    remote.handle.clone(),
                    fips_required,
                ))?;
                let r = loom_wire::store_admin::store_policy_result_from_cbor(&cbor)
                    .map_err(|e| e.to_string())?;
                Ok(store_policy_json(
                    loom_store::StorePolicy {
                        fips_required: r.fips_required,
                    },
                    r.audit_seq,
                ))
            }
        }
    }

    /// Remote `store rekey`: server-side rekey (fast rewrap or full reseal); the DEK never leaves the
    /// server. Returns a human summary of the audited result.
    pub(crate) fn admin_rekey_summary(
        &self,
        new_passphrase: Vec<u8>,
        reseal: bool,
        suite: Option<String>,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { .. } => Err(LOCAL_ADMIN_VIA_LOCAL_PATH.to_string()),
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let cbor = remote.block(StoreAdmin::store_rekey(
                    &remote.client,
                    remote.handle.clone(),
                    new_passphrase,
                    reseal,
                    suite,
                ))?;
                let r = loom_wire::store_admin::store_rekey_result_from_cbor(&cbor)
                    .map_err(|e| e.to_string())?;
                let bytes = match (r.bytes_before, r.bytes_after) {
                    (Some(b), Some(a)) => format!(" ({b} -> {a} bytes)"),
                    _ => String::new(),
                };
                Ok(format!(
                    "rekeyed remote store (resealed={}, suite={}, audit_seq={}){}",
                    r.resealed, r.suite, r.audit_seq, bytes
                ))
            }
        }
    }

    /// Remote `store key add-wrap` (passphrase): add an unlock wrap to the served store.
    pub(crate) fn admin_key_add_wrap(
        &self,
        new_passphrase: Vec<u8>,
        allow_no_recovery: bool,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { .. } => Err(LOCAL_ADMIN_VIA_LOCAL_PATH.to_string()),
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(KeySource::key_add_wrap_keyed(
                &remote.client,
                remote.handle.clone(),
                new_passphrase,
                allow_no_recovery,
            )),
        }
    }

    /// Remote `store key remove-wrap`: remove an unlock wrap by index from the served store.
    pub(crate) fn admin_key_remove_wrap(
        &self,
        index: u64,
        allow_no_recovery: bool,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { .. } => Err(LOCAL_ADMIN_VIA_LOCAL_PATH.to_string()),
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(KeySource::key_remove_wrap(
                &remote.client,
                remote.handle.clone(),
                index,
                allow_no_recovery,
            )),
        }
    }

    // ---- Transfer (byte-transfer interchange, specs/0067 §17) ----

    /// Import the local archive/CAR file at `local_path` into `workspace` as a byte transfer: the
    /// client reads the payload and drives `transfer_import_open`/`write`/`finish`, so the server
    /// never sees the client path. `kind` is a transfer kind name (`tar`/`tar-zstd`/`tar-gzip`/`zip`/
    /// `gzip`/`car`). Returns a human summary of the import-report.
    pub(crate) fn transfer_import(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        kind: &str,
        local_path: &str,
        commit: bool,
        dry_run: bool,
    ) -> Result<String, String> {
        let bytes = std::fs::read(local_path)
            .map_err(|e| format!("read transfer source {local_path}: {e}"))?;
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let import_scope = transfer_import_source_scope(kind, workspace, local_path)?;
                let report =
                    local_transfer_import(&mut loom, import_scope, kind, &bytes, commit, dry_run)?;
                Ok(summary_from_report(&report))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                // The final digest must use the served store's algo (digest equality is byte-only).
                let algo =
                    loom_core::Algo::from_name(&remote.block(Store::digest_algo(&remote.client))?)
                        .map_err(|e| e.to_string())?;
                let final_digest = WireDigest(loom_core::Digest::hash(algo, &bytes).to_string());
                let import_scope = transfer_import_source_scope(kind, workspace, local_path)?;
                let transfer = remote.block(Transfer::transfer_import_open(
                    &remote.client,
                    remote.handle.clone(),
                    import_scope.to_string(),
                    kind.to_string(),
                    Vec::new(),
                ))?;
                for (seq, chunk) in (0_u64..).zip(bytes.chunks(TRANSFER_CHUNK_BYTES)) {
                    remote.block(Transfer::transfer_import_write(
                        &remote.client,
                        remote.handle.clone(),
                        transfer.clone(),
                        chunk.to_vec(),
                        seq,
                        None,
                    ))?;
                }
                let report_cbor = remote.block(Transfer::transfer_import_finish(
                    &remote.client,
                    remote.handle.clone(),
                    transfer,
                    commit,
                    dry_run,
                    final_digest,
                ))?;
                summary_from_report_cbor(&report_cbor)
            }
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

    /// Create directory `path` in the Files working tree of `workspace` (FileSystem create_directory).
    /// `parents` creates missing intermediate directories.
    pub(crate) fn fs_mkdir(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        path: &str,
        parents: bool,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                // Ensure the Files workspace, creating it on first write so `files mkdir` does not require
                // a pre-existing workspace.
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Files)?;
                loom.create_directory(ns, path, parents)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(FileSystem::create_directory(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                path.to_string(),
                parents,
            )),
        }
    }

    /// Delete file or directory `path` in `workspace`. `stat` classifies the path, then a file/symlink is
    /// removed via `remove_file` and a directory via `remove_directory` (`recursive` deletes a non-empty
    /// directory; otherwise a non-empty directory is `INVALID_ARGUMENT`). Mirrors the local `files delete`.
    pub(crate) fn fs_delete(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        path: &str,
        recursive: bool,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                match loom.stat(ns, path).map_err(|e| e.to_string())?.kind {
                    FileKind::Directory => loom
                        .remove_directory(ns, path, recursive)
                        .map_err(|e| e.to_string())?,
                    FileKind::File | FileKind::Symlink => {
                        loom.remove_file(ns, path).map_err(|e| e.to_string())?;
                    }
                }
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let stat_bytes = remote.block(FileSystem::stat(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    path.to_string(),
                ))?;
                let stat =
                    loom_wire::fs::fs_stat_from_cbor(&stat_bytes).map_err(|e| e.to_string())?;
                match stat.kind {
                    FileKind::Directory => remote.block(FileSystem::remove_directory(
                        &remote.client,
                        remote.handle.clone(),
                        workspace.to_string(),
                        path.to_string(),
                        recursive,
                    )),
                    FileKind::File | FileKind::Symlink => remote.block(FileSystem::remove_file(
                        &remote.client,
                        remote.handle.clone(),
                        workspace.to_string(),
                        path.to_string(),
                    )),
                }
            }
        }
    }

    /// All file paths in the Files working tree of `workspace`, sorted, matching what `files ls` prints.
    /// The local arm returns `loom.staged_paths(ns)` (the working-tree file keys, already sorted); the
    /// remote arm reproduces that set by a recursive `FileSystem::list_directory` walk (descending into
    /// directories, emitting only file/symlink leaves), then sorts, so local and remote output match.
    pub(crate) fn fs_ls(&self, keys: &KeyOpts, workspace: &str) -> Result<Vec<String>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                Ok(loom.staged_paths(ns))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                fn walk(
                    remote: &RemoteStore,
                    workspace: &str,
                    dir: &str,
                    out: &mut Vec<String>,
                ) -> Result<(), String> {
                    let bytes = remote.block(FileSystem::list_directory(
                        &remote.client,
                        remote.handle.clone(),
                        workspace.to_string(),
                        dir.to_string(),
                    ))?;
                    let entries =
                        loom_wire::fs::dir_listing_from_cbor(&bytes).map_err(|e| e.to_string())?;
                    for entry in entries {
                        let child = if dir.is_empty() {
                            entry.name.clone()
                        } else {
                            format!("{dir}/{}", entry.name)
                        };
                        match entry.kind {
                            FileKind::Directory => walk(remote, workspace, &child, out)?,
                            FileKind::File | FileKind::Symlink => out.push(child),
                        }
                    }
                    Ok(())
                }
                let mut out = Vec::new();
                walk(remote, workspace, "", &mut out)?;
                out.sort();
                Ok(out)
            }
        }
    }

    // ---- ProtectedRefs ----
    //
    // The CLI prints protected-ref policies as JSON (`protected_ref_policy_json`/`_policies_json`), so
    // the remote arm decodes the canonical policy records into the typed `ProtectedRefPolicy` and returns
    // them for the handler to format through those same printers. The local arm applies its
    // effective-principal audit; the remote arm relies on the server session principal.

    /// The named protected-ref policies for `workspace` (ProtectedRefs list).
    pub(crate) fn pr_list(
        &self,
        keys: &KeyOpts,
        workspace: &str,
    ) -> Result<Vec<(String, loom_core::vcs::ProtectedRefPolicy)>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom.protected_ref_policies(ns).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(ProtectedRefs::protected_ref_list(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                ))?;
                wire.iter()
                    .map(|record| cli_named_protected_ref_from_remote(record))
                    .collect()
            }
        }
    }

    /// The protected-ref policy for `ref_name` in `workspace` (ProtectedRefs get), or `None` when absent.
    pub(crate) fn pr_get(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        ref_name: &str,
    ) -> Result<Option<loom_core::vcs::ProtectedRefPolicy>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom.protected_ref_policy(ns, ref_name)
                    .map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(ProtectedRefs::protected_ref_get(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    ref_name.to_string(),
                ))?;
                match wire {
                    Some(record) => Ok(Some(cli_protected_ref_policy_from_remote(&record)?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// Set the protected-ref `policy` for `ref_name` in `workspace` (ProtectedRefs set). The local arm
    /// keeps its effective-principal audit; the remote arm relies on the server session principal.
    pub(crate) fn pr_set(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        ref_name: &str,
        policy: loom_core::vcs::ProtectedRefPolicy,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = loom.effective_principal().map_err(|e| e.to_string())?;
                let ns = resolve_ns(&loom, workspace)?;
                loom.set_protected_ref_policy(ns, ref_name, policy)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                let target = format!("workspace={ns};ref={ref_name}");
                loom.store()
                    .audit_append(actor, "protected_ref.set", Some(&target))
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(ProtectedRefs::protected_ref_set(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                ref_name.to_string(),
                policy.fast_forward_only,
                policy.signed_commits_required,
                policy.signed_ref_advance_required,
                policy.required_review_count,
                policy.retention_lock,
                policy.governance_lock,
            )),
        }
    }

    /// Remove the protected-ref policy for `ref_name` in `workspace` (ProtectedRefs remove); returns
    /// whether it was present. The local arm keeps its effective-principal audit on removal.
    pub(crate) fn pr_remove(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        ref_name: &str,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = loom.effective_principal().map_err(|e| e.to_string())?;
                let ns = resolve_ns(&loom, workspace)?;
                let removed = loom
                    .remove_protected_ref_policy(ns, ref_name)
                    .map_err(|e| e.to_string())?;
                if removed {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                    let target = format!("workspace={ns};ref={ref_name}");
                    loom.store()
                        .audit_append(actor, "protected_ref.remove", Some(&target))
                        .map_err(|e| e.to_string())?;
                }
                Ok(removed)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(ProtectedRefs::protected_ref_remove(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                ref_name.to_string(),
            )),
        }
    }

    // ---- Workspaces (session-level workspace management) ----
    //
    // Authz + audit stay local on the local arm (`require_global_admin*` + `audit_append`); the remote arm
    // relies on the server session principal. `workspace list` output is reproduced by decoding the
    // canonical `WorkspaceInfo` records and formatting through `print_workspaces_infos`. `rename`/`delete`
    // print the resolved `WorkspaceId`; the remote arm resolves it from the remote workspace list before
    // mutating so the output matches local.

    /// Create a workspace (optionally typed by `facet`); returns the new workspace id string the CLI
    /// prints (Workspaces create).
    pub(crate) fn ws_create(
        &self,
        keys: &KeyOpts,
        name: &str,
        facet: Option<&str>,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let id = match facet {
                    Some(facet) => {
                        let facet = FacetKind::parse(facet).map_err(|e| e.to_string())?;
                        loom.registry_mut()
                            .ensure_for_write(
                                &loom_core::WsSelector::Typed {
                                    ty: facet,
                                    name: name.to_string(),
                                },
                                random_workspace_id()?,
                            )
                            .map_err(|e| e.to_string())?
                    }
                    None => loom
                        .registry_mut()
                        .create_workspace(Some(name), random_workspace_id()?)
                        .map_err(|e| e.to_string())?,
                };
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                let target = format!("workspace={id};name={name}");
                loom.store()
                    .audit_append(Some(actor), "workspace.create", Some(&target))
                    .map_err(|e| e.to_string())?;
                Ok(id.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let facet_bytes = match facet {
                    Some(facet) => Some(vec![
                        FacetKind::parse(facet)
                            .map_err(|e| e.to_string())?
                            .stable_tag(),
                    ]),
                    None => None,
                };
                let uuid = remote.block(Workspaces::workspace_create(
                    &remote.client,
                    remote.handle.clone(),
                    Some(name.to_string()),
                    facet_bytes,
                ))?;
                Ok(loom_core::WorkspaceId::from_bytes(uuid.0).to_string())
            }
        }
    }

    /// The workspaces for the store (Workspaces list), for the CLI to format via
    /// `print_workspaces_infos`.
    pub(crate) fn ws_list(&self, keys: &KeyOpts) -> Result<Vec<loom_core::WorkspaceInfo>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_registry_read(locator, keys)?;
                require_global_admin(&loom)?;
                Ok(loom.registry().list(None))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Workspaces::workspace_list(
                    &remote.client,
                    remote.handle.clone(),
                ))?;
                wire.iter()
                    .map(|record| cli_workspace_info_from_remote(record))
                    .collect()
            }
        }
    }

    /// Rename `workspace` to `new_name` (Workspaces rename); returns the resolved workspace id string the
    /// CLI prints.
    pub(crate) fn ws_rename(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        new_name: &str,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom.registry_mut()
                    .rename(ns, new_name)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                let target = format!("workspace={ns};name={new_name}");
                loom.store()
                    .audit_append(Some(actor), "workspace.rename", Some(&target))
                    .map_err(|e| e.to_string())?;
                Ok(ns.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let ns = remote.resolve_workspace_id(workspace)?;
                remote.block(Workspaces::workspace_rename(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    new_name.to_string(),
                ))?;
                Ok(ns.to_string())
            }
        }
    }

    /// Delete `workspace` (Workspaces delete); returns the resolved workspace id string the CLI prints.
    pub(crate) fn ws_delete(&self, keys: &KeyOpts, workspace: &str) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom.registry_mut().delete(ns).map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                let target = ns.to_string();
                loom.store()
                    .audit_append(Some(actor), "workspace.delete", Some(&target))
                    .map_err(|e| e.to_string())?;
                Ok(ns.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let ns = remote.resolve_workspace_id(workspace)?;
                remote.block(Workspaces::workspace_delete(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                ))?;
                Ok(ns.to_string())
            }
        }
    }

    // ---- Acl (global admin ACL management) ----
    //
    // `acl list` prints JSON; the remote arm decodes the canonical grant records (`acl_grant_from_cbor`)
    // and the handler formats through `acl_grants_json`. `grant`/`revoke` map to the generated 9-arg `Acl`
    // methods: the remote arm encodes each argument with the `loom_wire` ACL codecs and passes the raw
    // `workspace` string for the server to resolve (it does not pre-resolve against a local registry).
    // Authz + audit stay local on the local arm; the remote arm relies on the server session principal.

    /// The ACL grant list (Acl list), for the CLI to format via `acl_grants_json`.
    pub(crate) fn acl_list(&self, keys: &KeyOpts) -> Result<Vec<loom_core::AclGrant>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                require_global_admin(&loom)?;
                Ok(loom.acl_store().grants().to_vec())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Acl::acl_list(&remote.client, remote.handle.clone()))?;
                wire.iter()
                    .map(|record| {
                        loom_wire::acl::acl_grant_from_cbor(record).map_err(|e| e.to_string())
                    })
                    .collect()
            }
        }
    }

    /// Grant an ACL rule (Acl grant). The local arm applies it to the local `AclStore` with a local audit;
    /// the remote arm encodes the wire args and lets the server apply + audit it.
    pub(crate) fn acl_grant(&self, keys: &KeyOpts, args: AclGrantArgs<'_>) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let grant = acl_grant_from_args(&loom, args)?;
                let target = acl_grant_json(&grant);
                let snapshot = {
                    let acl = loom.acl_store_mut();
                    acl.grant(grant).map_err(|e| e.to_string())?;
                    acl.clone()
                };
                loom.store()
                    .save_acl_store_audited(&snapshot, Some(actor), "acl.grant", Some(&target))
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = encode_acl_wire_args(&args)?;
                remote.block(Acl::acl_grant(
                    &remote.client,
                    remote.handle.clone(),
                    wire.effect,
                    wire.subject,
                    wire.workspace,
                    wire.domain,
                    wire.ref_glob,
                    wire.scopes,
                    wire.rights,
                    wire.predicate,
                ))
            }
        }
    }

    /// Revoke an ACL rule (Acl revoke); returns whether a matching grant was present.
    pub(crate) fn acl_revoke(
        &self,
        keys: &KeyOpts,
        args: AclGrantArgs<'_>,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let grant = acl_grant_from_args(&loom, args)?;
                let target = acl_grant_json(&grant);
                let (removed, snapshot) = {
                    let acl = loom.acl_store_mut();
                    let removed = acl.revoke(&grant);
                    (removed, acl.clone())
                };
                if removed {
                    loom.store()
                        .save_acl_store_audited(&snapshot, Some(actor), "acl.revoke", Some(&target))
                        .map_err(|e| e.to_string())?;
                }
                Ok(removed)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = encode_acl_wire_args(&args)?;
                remote.block(Acl::acl_revoke(
                    &remote.client,
                    remote.handle.clone(),
                    wire.effect,
                    wire.subject,
                    wire.workspace,
                    wire.domain,
                    wire.ref_glob,
                    wire.scopes,
                    wire.rights,
                    wire.predicate,
                ))
            }
        }
    }

    // ---- Identity (global admin identity control plane) ----
    //
    // `list`/`public-key list` print JSON. The remote arm decodes the canonical `IdentitySnapshot`
    // (`identity_snapshot_from_cbor`) and formats through the shared `identity_snapshot_json` /
    // `identity_public_keys_json`, so the output matches the local arm byte-for-byte. Mutations forward
    // to the generated `Identity` methods; `set_passphrase` sends the passphrase bytes and the server
    // mints the salt. Authz + audit stay local on the local arm; the remote arm relies on the server
    // session principal. The audit-seq-bearing credential/key mutations are not on this facade (the IDL
    // methods return only a `Uuid` or nothing, so a remote client cannot reproduce the `seq` the CLI
    // prints); they open through `cli_open_loom` directly.

    /// The identity snapshot JSON the CLI prints (Identity list).
    pub(crate) fn id_list(&self, keys: &KeyOpts) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                require_global_admin(&loom)?;
                let identity = loom
                    .identity_store()
                    .ok_or_else(|| "identity store not initialized".to_string())?;
                Ok(identity_list_json(identity))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let bytes = remote.block(Identity::identity_list(
                    &remote.client,
                    remote.handle.clone(),
                ))?;
                let view = loom_wire::identity::identity_snapshot_from_cbor(&bytes)
                    .map_err(|e| e.to_string())?;
                Ok(identity_snapshot_json(&view))
            }
        }
    }

    /// Add a principal (Identity add); returns the new principal id string the CLI prints.
    pub(crate) fn id_add(
        &self,
        keys: &KeyOpts,
        handle: &str,
        name: &str,
        kind: loom_core::PrincipalKind,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let id = random_workspace_id()?;
                let snapshot = {
                    let identity = loom
                        .identity_store_mut()
                        .ok_or_else(|| "identity store not initialized".to_string())?;
                    identity
                        .add_principal_with_handle(id, handle, name, kind)
                        .map_err(|e| e.to_string())?;
                    identity.clone()
                };
                let target = id.to_string();
                loom.store()
                    .save_identity_store_audited(
                        &snapshot,
                        Some(actor),
                        "identity.add_principal",
                        Some(&target),
                    )
                    .map_err(|e| e.to_string())?;
                Ok(id.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let uuid = remote.block(Identity::identity_add_principal(
                    &remote.client,
                    remote.handle.clone(),
                    handle.to_string(),
                    name.to_string(),
                    vec![kind.stable_tag()],
                ))?;
                Ok(loom_core::WorkspaceId::from_bytes(uuid.0).to_string())
            }
        }
    }

    /// Rename a principal handle (Identity rename-handle); returns the principal id string the CLI prints.
    pub(crate) fn id_rename_handle(
        &self,
        keys: &KeyOpts,
        principal: &str,
        handle: &str,
    ) -> Result<String, String> {
        let principal = WorkspaceId::parse(principal).map_err(|e| e.to_string())?;
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let snapshot = {
                    let identity = loom
                        .identity_store_mut()
                        .ok_or_else(|| "identity store not initialized".to_string())?;
                    identity
                        .rename_principal_handle(principal, handle)
                        .map_err(|e| e.to_string())?;
                    identity.clone()
                };
                loom.store()
                    .save_identity_store_audited(
                        &snapshot,
                        Some(actor),
                        "identity.rename_principal_handle",
                        Some(&principal.to_string()),
                    )
                    .map_err(|e| e.to_string())?;
                Ok(principal.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                remote.block(Identity::identity_rename_principal_handle(
                    &remote.client,
                    remote.handle.clone(),
                    Uuid(*principal.as_bytes()),
                    handle.to_string(),
                ))?;
                Ok(principal.to_string())
            }
        }
    }

    /// Set a principal passphrase (Identity set-passphrase). The local arm mints the salt; the remote arm
    /// sends the passphrase bytes and the server mints its own salt.
    pub(crate) fn id_set_passphrase(
        &self,
        keys: &KeyOpts,
        principal: &str,
        passphrase: &[u8],
    ) -> Result<(), String> {
        let principal = WorkspaceId::parse(principal).map_err(|e| e.to_string())?;
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let passphrase = std::str::from_utf8(passphrase)
                    .map_err(|_| "passphrase is not valid utf-8".to_string())?;
                let salt = rand_bytes(16)?;
                let snapshot = {
                    let identity = loom
                        .identity_store_mut()
                        .ok_or_else(|| "identity store not initialized".to_string())?;
                    identity
                        .set_passphrase(principal, passphrase, &salt)
                        .map_err(|e| e.to_string())?;
                    identity.clone()
                };
                loom.store()
                    .save_identity_store_audited(
                        &snapshot,
                        Some(actor),
                        "identity.set_passphrase",
                        Some(&principal.to_string()),
                    )
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Identity::identity_set_passphrase(
                &remote.client,
                remote.handle.clone(),
                Uuid(*principal.as_bytes()),
                passphrase.to_vec(),
            )),
        }
    }

    /// Remove a principal (Identity remove).
    pub(crate) fn id_remove(&self, keys: &KeyOpts, principal: &str) -> Result<(), String> {
        let principal = WorkspaceId::parse(principal).map_err(|e| e.to_string())?;
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let snapshot = {
                    let identity = loom
                        .identity_store_mut()
                        .ok_or_else(|| "identity store not initialized".to_string())?;
                    identity
                        .remove_principal(principal)
                        .map_err(|e| e.to_string())?;
                    identity.clone()
                };
                loom.store()
                    .save_identity_store_audited(
                        &snapshot,
                        Some(actor),
                        "identity.remove_principal",
                        Some(&principal.to_string()),
                    )
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Identity::identity_remove_principal(
                &remote.client,
                remote.handle.clone(),
                Uuid(*principal.as_bytes()),
            )),
        }
    }

    /// Assign a role to a principal (Identity assign-role).
    pub(crate) fn id_assign_role(
        &self,
        keys: &KeyOpts,
        principal: &str,
        role: &str,
    ) -> Result<(), String> {
        let principal = WorkspaceId::parse(principal).map_err(|e| e.to_string())?;
        let role = WorkspaceId::parse(role).map_err(|e| e.to_string())?;
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let snapshot = {
                    let identity = loom
                        .identity_store_mut()
                        .ok_or_else(|| "identity store not initialized".to_string())?;
                    identity
                        .assign_role(principal, role)
                        .map_err(|e| e.to_string())?;
                    identity.clone()
                };
                let target = format!("principal={principal};role={role}");
                loom.store()
                    .save_identity_store_audited(
                        &snapshot,
                        Some(actor),
                        "identity.assign_role",
                        Some(&target),
                    )
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Identity::identity_assign_role(
                &remote.client,
                remote.handle.clone(),
                Uuid(*principal.as_bytes()),
                Uuid(*role.as_bytes()),
            )),
        }
    }

    /// Revoke a role from a principal (Identity revoke-role); returns whether a grant was removed.
    pub(crate) fn id_revoke_role(
        &self,
        keys: &KeyOpts,
        principal: &str,
        role: &str,
    ) -> Result<bool, String> {
        let principal = WorkspaceId::parse(principal).map_err(|e| e.to_string())?;
        let role = WorkspaceId::parse(role).map_err(|e| e.to_string())?;
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let (removed, snapshot) = {
                    let identity = loom
                        .identity_store_mut()
                        .ok_or_else(|| "identity store not initialized".to_string())?;
                    let removed = identity
                        .revoke_role(principal, role)
                        .map_err(|e| e.to_string())?;
                    (removed, identity.clone())
                };
                if removed {
                    let target = format!("principal={principal};role={role}");
                    loom.store()
                        .save_identity_store_audited(
                            &snapshot,
                            Some(actor),
                            "identity.revoke_role",
                            Some(&target),
                        )
                        .map_err(|e| e.to_string())?;
                }
                Ok(removed)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Identity::identity_revoke_role(
                &remote.client,
                remote.handle.clone(),
                Uuid(*principal.as_bytes()),
                Uuid(*role.as_bytes()),
            )),
        }
    }

    /// The identity public-key list JSON the CLI prints (Identity public-key list).
    /// Create an external credential and return the `{"seq":N,"credential":{...}}` line the CLI prints.
    /// The remote path reconstructs the full record with a follow-up `identity_list`, keyed by the minted
    /// id from the audit result, so its output matches the local command.
    pub(crate) fn id_external_credential_create(
        &self,
        keys: &KeyOpts,
        principal: WorkspaceId,
        kind: loom_core::ExternalCredentialKind,
        label: String,
        issuer: String,
        subject: String,
        material_digest: Option<String>,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let id = random_workspace_id()?;
                let (credential, snapshot) = {
                    let identity = loom
                        .identity_store_mut()
                        .ok_or_else(|| "identity store not initialized".to_string())?;
                    let credential = identity
                        .create_external_credential(
                            principal,
                            loom_core::ExternalCredentialSpec {
                                id,
                                kind,
                                label,
                                issuer,
                                subject,
                                material_digest,
                            },
                        )
                        .map_err(|e| e.to_string())?;
                    (credential, identity.clone())
                };
                let target = format!("principal={principal};credential={id}");
                let seq = loom
                    .store()
                    .save_identity_store_audited(
                        &snapshot,
                        Some(actor),
                        "identity.external_credential.create",
                        Some(&target),
                    )
                    .map_err(|e| e.to_string())?;
                Ok(format!(
                    "{{\"seq\":{seq},\"credential\":{}}}",
                    crate::helpers::external_credential_json(&credential)
                ))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let spec = loom_core::ExternalCredentialSpec {
                    id: random_workspace_id()?,
                    kind,
                    label,
                    issuer,
                    subject,
                    material_digest,
                };
                let wire = loom_wire::identity::external_credential_spec_to_wire(&spec)
                    .map_err(|e| e.to_string())?;
                let audit = remote.block(Identity::identity_create_external_credential(
                    &remote.client,
                    remote.handle.clone(),
                    Uuid(*principal.as_bytes()),
                    wire,
                ))?;
                let result = loom_wire::identity::identity_audit_result_from_cbor(&audit)
                    .map_err(|e| e.to_string())?;
                let id = result
                    .id
                    .ok_or_else(|| "create did not return a credential id".to_string())?;
                let view = loom_wire::identity::identity_snapshot_from_cbor(&remote.block(
                    Identity::identity_list(&remote.client, remote.handle.clone()),
                )?)
                .map_err(|e| e.to_string())?;
                let credential = view
                    .external_credentials
                    .iter()
                    .find(|c| c.id == id)
                    .ok_or_else(|| "created credential not found on read-back".to_string())?;
                Ok(format!(
                    "{{\"seq\":{},\"credential\":{}}}",
                    result.audit_seq,
                    crate::helpers::external_credential_json(credential)
                ))
            }
        }
    }

    /// Revoke an external credential and return the `{"seq":N,"credential":{...}}` line. The remote path
    /// reads the record before revoking it so its output matches the local command.
    pub(crate) fn id_external_credential_revoke(
        &self,
        keys: &KeyOpts,
        id: WorkspaceId,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let (credential, snapshot) = {
                    let identity = loom
                        .identity_store_mut()
                        .ok_or_else(|| "identity store not initialized".to_string())?;
                    let credential = identity
                        .revoke_external_credential(id)
                        .map_err(|e| e.to_string())?;
                    (credential, identity.clone())
                };
                let target = format!(
                    "principal={};credential={}",
                    credential.principal, credential.id
                );
                let seq = loom
                    .store()
                    .save_identity_store_audited(
                        &snapshot,
                        Some(actor),
                        "identity.external_credential.revoke",
                        Some(&target),
                    )
                    .map_err(|e| e.to_string())?;
                Ok(format!(
                    "{{\"seq\":{seq},\"credential\":{}}}",
                    crate::helpers::external_credential_json(&credential)
                ))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let view = loom_wire::identity::identity_snapshot_from_cbor(&remote.block(
                    Identity::identity_list(&remote.client, remote.handle.clone()),
                )?)
                .map_err(|e| e.to_string())?;
                let credential = view
                    .external_credentials
                    .iter()
                    .find(|c| c.id == id)
                    .cloned()
                    .ok_or_else(|| "external credential not found".to_string())?;
                let audit = remote.block(Identity::identity_revoke_external_credential(
                    &remote.client,
                    remote.handle.clone(),
                    Uuid(*id.as_bytes()),
                ))?;
                let result = loom_wire::identity::identity_audit_result_from_cbor(&audit)
                    .map_err(|e| e.to_string())?;
                Ok(format!(
                    "{{\"seq\":{},\"credential\":{}}}",
                    result.audit_seq,
                    crate::helpers::external_credential_json(&credential)
                ))
            }
        }
    }

    /// Add a public key and return the `{"seq":N,"public_key":{...}}` line. The remote path reconstructs
    /// the full record with a follow-up `identity_list`, keyed by the minted id from the audit result.
    pub(crate) fn id_add_public_key(
        &self,
        keys: &KeyOpts,
        principal: WorkspaceId,
        label: String,
        algorithm: String,
        public_key: Vec<u8>,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let id = random_workspace_id()?;
                let (key, snapshot) = {
                    let identity = loom
                        .identity_store_mut()
                        .ok_or_else(|| "identity store not initialized".to_string())?;
                    let key = identity
                        .add_public_key(
                            principal,
                            loom_core::IdentityPublicKeySpec {
                                id,
                                label,
                                algorithm,
                                public_key,
                            },
                        )
                        .map_err(|e| e.to_string())?;
                    (key, identity.clone())
                };
                let target = format!("principal={principal};key={id}");
                let seq = loom
                    .store()
                    .save_identity_store_audited(
                        &snapshot,
                        Some(actor),
                        "identity.public_key.add",
                        Some(&target),
                    )
                    .map_err(|e| e.to_string())?;
                Ok(format!(
                    "{{\"seq\":{seq},\"public_key\":{}}}",
                    crate::helpers::identity_public_key_json(&key)
                ))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let audit = remote.block(Identity::identity_add_public_key(
                    &remote.client,
                    remote.handle.clone(),
                    Uuid(*principal.as_bytes()),
                    label,
                    algorithm,
                    public_key,
                ))?;
                let result = loom_wire::identity::identity_audit_result_from_cbor(&audit)
                    .map_err(|e| e.to_string())?;
                let id = result
                    .id
                    .ok_or_else(|| "add did not return a public key id".to_string())?;
                let view = loom_wire::identity::identity_snapshot_from_cbor(&remote.block(
                    Identity::identity_list(&remote.client, remote.handle.clone()),
                )?)
                .map_err(|e| e.to_string())?;
                let key = view
                    .public_keys
                    .iter()
                    .find(|k| k.id == id)
                    .ok_or_else(|| "added public key not found on read-back".to_string())?;
                Ok(format!(
                    "{{\"seq\":{},\"public_key\":{}}}",
                    result.audit_seq,
                    crate::helpers::identity_public_key_json(key)
                ))
            }
        }
    }

    /// Revoke a public key and return the `{"seq":N,"public_key":{...}}` line. The remote path reads the
    /// record before revoking it so its output matches the local command.
    pub(crate) fn id_revoke_public_key(
        &self,
        keys: &KeyOpts,
        id: WorkspaceId,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let (key, snapshot) = {
                    let identity = loom
                        .identity_store_mut()
                        .ok_or_else(|| "identity store not initialized".to_string())?;
                    let key = identity.revoke_public_key(id).map_err(|e| e.to_string())?;
                    (key, identity.clone())
                };
                let target = format!("principal={};key={}", key.principal, key.id);
                let seq = loom
                    .store()
                    .save_identity_store_audited(
                        &snapshot,
                        Some(actor),
                        "identity.public_key.revoke",
                        Some(&target),
                    )
                    .map_err(|e| e.to_string())?;
                Ok(format!(
                    "{{\"seq\":{seq},\"public_key\":{}}}",
                    crate::helpers::identity_public_key_json(&key)
                ))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let view = loom_wire::identity::identity_snapshot_from_cbor(&remote.block(
                    Identity::identity_list(&remote.client, remote.handle.clone()),
                )?)
                .map_err(|e| e.to_string())?;
                let key = view
                    .public_keys
                    .iter()
                    .find(|k| k.id == id)
                    .cloned()
                    .ok_or_else(|| "public key not found".to_string())?;
                let audit = remote.block(Identity::identity_revoke_public_key(
                    &remote.client,
                    remote.handle.clone(),
                    Uuid(*id.as_bytes()),
                ))?;
                let result = loom_wire::identity::identity_audit_result_from_cbor(&audit)
                    .map_err(|e| e.to_string())?;
                Ok(format!(
                    "{{\"seq\":{},\"public_key\":{}}}",
                    result.audit_seq,
                    crate::helpers::identity_public_key_json(&key)
                ))
            }
        }
    }

    /// Create an app credential (server-minted secret) and return the
    /// `{"seq":N,"credential":{...},"secret":"<token>"}` line. The secret is minted by the authority that
    /// stores the verifier (the CLI process for a local store, the hosted server for a remote store) and
    /// returned exactly once; the store keeps only the salted verifier.
    pub(crate) fn id_app_credential_create(
        &self,
        keys: &KeyOpts,
        principal: WorkspaceId,
        label: String,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let (credential, token, snapshot) = {
                    let identity = loom
                        .identity_store_mut()
                        .ok_or_else(|| "identity store not initialized".to_string())?;
                    // Same server-side mint the hosted server runs via
                    // `LocalLoomClient::identity_create_app_credential`: id/secret/salt are generated by the
                    // authority that stores the verifier, and only the salted verifier is persisted.
                    let (credential, token) =
                        loom_client::local::mint_app_credential(identity, principal, &label)
                            .map_err(|e| e.to_string())?;
                    let snapshot = identity.clone();
                    (credential, token, snapshot)
                };
                let target = format!("principal={principal};credential={}", credential.id);
                let seq = loom
                    .store()
                    .save_identity_store_audited(
                        &snapshot,
                        Some(actor),
                        "identity.app_credential.create",
                        Some(&target),
                    )
                    .map_err(|e| e.to_string())?;
                Ok(format!(
                    "{{\"seq\":{seq},\"credential\":{},\"secret\":{}}}",
                    crate::helpers::app_credential_json(&credential),
                    crate::helpers::json_string(&token)
                ))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let result = loom_wire::identity::app_credential_create_result_from_cbor(
                    &remote.block(Identity::identity_create_app_credential(
                        &remote.client,
                        remote.handle.clone(),
                        Uuid(*principal.as_bytes()),
                        label,
                    ))?,
                )
                .map_err(|e| e.to_string())?;
                let credential = loom_core::AppCredential {
                    id: result.id,
                    principal: result.principal,
                    label: result.label,
                    enabled: result.enabled,
                };
                Ok(format!(
                    "{{\"seq\":{},\"credential\":{},\"secret\":{}}}",
                    result.audit_seq,
                    crate::helpers::app_credential_json(&credential),
                    crate::helpers::json_string(&result.secret_token)
                ))
            }
        }
    }

    /// Revoke an app credential and return the `{"seq":N,"credential":{...}}` line. The remote path reads
    /// the record before revoking it so its output matches the local command.
    pub(crate) fn id_app_credential_revoke(
        &self,
        keys: &KeyOpts,
        id: WorkspaceId,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let actor = require_global_admin_actor(&loom)?;
                let (credential, snapshot) = {
                    let identity = loom
                        .identity_store_mut()
                        .ok_or_else(|| "identity store not initialized".to_string())?;
                    let credential = identity
                        .revoke_app_credential(id)
                        .map_err(|e| e.to_string())?;
                    (credential, identity.clone())
                };
                let target = format!(
                    "principal={};credential={}",
                    credential.principal, credential.id
                );
                let seq = loom
                    .store()
                    .save_identity_store_audited(
                        &snapshot,
                        Some(actor),
                        "identity.app_credential.revoke",
                        Some(&target),
                    )
                    .map_err(|e| e.to_string())?;
                Ok(format!(
                    "{{\"seq\":{seq},\"credential\":{}}}",
                    crate::helpers::app_credential_json(&credential)
                ))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let view = loom_wire::identity::identity_snapshot_from_cbor(&remote.block(
                    Identity::identity_list(&remote.client, remote.handle.clone()),
                )?)
                .map_err(|e| e.to_string())?;
                let credential = view
                    .app_credentials
                    .iter()
                    .find(|c| c.id == id)
                    .cloned()
                    .ok_or_else(|| "app credential not found".to_string())?;
                let result = loom_wire::identity::identity_audit_result_from_cbor(&remote.block(
                    Identity::identity_revoke_app_credential(
                        &remote.client,
                        remote.handle.clone(),
                        Uuid(*id.as_bytes()),
                    ),
                )?)
                .map_err(|e| e.to_string())?;
                Ok(format!(
                    "{{\"seq\":{},\"credential\":{}}}",
                    result.audit_seq,
                    crate::helpers::app_credential_json(&credential)
                ))
            }
        }
    }

    pub(crate) fn id_public_key_list(&self, keys: &KeyOpts) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                require_global_admin(&loom)?;
                let identity = loom
                    .identity_store()
                    .ok_or_else(|| "identity store not initialized".to_string())?;
                let keys: Vec<_> = identity.public_keys().cloned().collect();
                Ok(identity_public_keys_json(&keys))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let bytes = remote.block(Identity::identity_list(
                    &remote.client,
                    remote.handle.clone(),
                ))?;
                let view = loom_wire::identity::identity_snapshot_from_cbor(&bytes)
                    .map_err(|e| e.to_string())?;
                Ok(identity_public_keys_json(&view.public_keys))
            }
        }
    }

    // ---- Columnar ----
    //
    // The CLI's `columnar_*` CBOR codecs are the same wire format as the server's `loom_wire::columnar`
    // codecs, so every payload is a bytes pass-through: inputs (schema / row / select columns+filter /
    // aggregates) cross as the raw file bytes the CLI reads, and outputs (scan / columns / inspect /
    // select / aggregate rows) cross as the canonical CBOR the CLI prints. `source_digest` is the one
    // exception (the CLI prints the digest string, the wire form is CBOR text) and is decoded back to the
    // string in the remote arm.

    /// Create columnar dataset `name` from the canonical-CBOR `columns` schema (Columnar create).
    pub(crate) fn col_create(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        columns: Vec<u8>,
        target_segment_rows: u64,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let columns = columnar_columns_from_cbor(&columns)?;
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Columnar)?;
                loom_core::columnar_create(
                    &mut loom,
                    ns,
                    name,
                    columns,
                    target_segment_rows as usize,
                )
                .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Columnar::create(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                columns,
                target_segment_rows,
            )),
        }
    }

    /// Append the canonical-CBOR `row` to columnar dataset `name` (Columnar append).
    pub(crate) fn col_append(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        row: Vec<u8>,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let row = columnar_row_from_cbor(&row)?;
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::columnar_append(&mut loom, ns, name, row).map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Columnar::append(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                row,
            )),
        }
    }

    /// The canonical-CBOR rows of columnar dataset `name` (Columnar scan).
    pub(crate) fn col_scan(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let rows = loom_core::columnar_scan(&loom, ns, name).map_err(|e| e.to_string())?;
                columnar_rows_cbor(rows)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Columnar::scan(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
            )),
        }
    }

    /// The canonical-CBOR schema of columnar dataset `name` (Columnar columns).
    pub(crate) fn col_columns(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let columns =
                    loom_core::columnar_columns(&loom, ns, name).map_err(|e| e.to_string())?;
                columnar_columns_cbor(columns)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Columnar::columns(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
            )),
        }
    }

    /// The row count of columnar dataset `name` (Columnar rows).
    pub(crate) fn col_rows(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
    ) -> Result<u64, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::columnar_rows(&loom, ns, name)
                    .map(|rows| rows as u64)
                    .map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Columnar::rows(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
            )),
        }
    }

    /// Compact columnar dataset `name` (Columnar compact).
    pub(crate) fn col_compact(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::columnar_compact(&mut loom, ns, name).map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Columnar::compact(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
            )),
        }
    }

    /// The canonical-CBOR inspection report of columnar dataset `name` (Columnar inspect).
    pub(crate) fn col_inspect(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let inspect =
                    loom_core::columnar_inspect(&loom, ns, name).map_err(|e| e.to_string())?;
                columnar_inspect_cbor(inspect)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Columnar::inspect(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
            )),
        }
    }

    /// The source digest string of columnar dataset `name` the CLI prints (Columnar source_digest). The
    /// remote arm decodes the canonical CBOR-text wire form back to the digest string.
    pub(crate) fn col_source_digest(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                Ok(loom_core::columnar_source_digest(&loom, ns, name)
                    .map_err(|e| e.to_string())?
                    .to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Columnar::source_digest(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    name.to_string(),
                ))?;
                match loom_codec::decode(&wire).map_err(|e| e.to_string())? {
                    loom_codec::Value::Text(text) => Ok(text),
                    _ => Err("expected a CBOR text digest from the remote endpoint".to_string()),
                }
            }
        }
    }

    /// The canonical-CBOR rows of a projection/filter over columnar dataset `name` (Columnar select).
    /// `columns` is the canonical-CBOR projected column list; `filter` is the canonical-CBOR filter (empty
    /// for no filter).
    pub(crate) fn col_select(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        columns: Vec<u8>,
        filter: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let columns = columnar_select_columns_from_cbor(&columns)?;
                let filter = columnar_filter_from_cbor(&filter)?;
                let column_refs = columns.iter().map(String::as_str).collect::<Vec<_>>();
                let filter_ref = filter
                    .as_ref()
                    .map(|(column, op, value)| (column.as_str(), *op, value));
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let rows = loom_core::columnar_select(&loom, ns, name, &column_refs, filter_ref)
                    .map_err(|e| e.to_string())?;
                columnar_rows_cbor(rows)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Columnar::select(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                columns,
                filter,
            )),
        }
    }

    /// The canonical-CBOR aggregate values over columnar dataset `name` (Columnar aggregate).
    /// `aggregates` is the canonical-CBOR aggregate list; `filter` is the canonical-CBOR filter (empty for
    /// no filter).
    pub(crate) fn col_aggregate(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        aggregates: Vec<u8>,
        filter: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let aggregates = columnar_aggregates_from_cbor(&aggregates)?;
                let filter = columnar_filter_from_cbor(&filter)?;
                let filter_ref = filter
                    .as_ref()
                    .map(|(column, op, value)| (column.as_str(), *op, value));
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let values =
                    loom_core::columnar_aggregate(&loom, ns, name, &aggregates, filter_ref)
                        .map_err(|e| e.to_string())?;
                columnar_values_cbor(values)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Columnar::aggregate(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                aggregates,
                filter,
            )),
        }
    }

    // ---- Graph ----
    //
    // The CLI's graph CBOR codecs (`props_*`, `graph_edge_cbor`, `graph_strings_cbor`,
    // `graph_edges_cbor`) are the same wire format as the server's `loom_wire::graph` codecs, so every
    // payload is a clean bytes pass-through (node/edge props in, node/edge/neighbor/path records out).
    // `reachable`'s `max_depth` bound is carried by the IDL (i64), so it forwards directly - no gap.

    /// Upsert graph node `id` with the canonical-CBOR `props` (Graph upsert_node). Empty `props` is an
    /// empty bag on both arms.
    pub(crate) fn g_upsert_node(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: &str,
        props: Vec<u8>,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let props = props_from_cbor(&props)?;
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Graph)?;
                loom_core::graph_upsert_node(&mut loom, ns, name, id, props)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Graph::upsert_node(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id.to_string(),
                props,
            )),
        }
    }

    /// The canonical-CBOR props of graph node `id` (Graph get_node), or `None` when absent.
    pub(crate) fn g_get_node(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                match loom_core::graph_get_node(&loom, ns, name, id).map_err(|e| e.to_string())? {
                    Some(props) => Ok(Some(props_to_cbor(&props)?)),
                    None => Ok(None),
                }
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Graph::get_node(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id.to_string(),
            )),
        }
    }

    /// Remove graph node `id` (Graph remove_node), optionally cascading to its edges.
    pub(crate) fn g_remove_node(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: &str,
        cascade: bool,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::graph_remove_node(&mut loom, ns, name, id, cascade)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Graph::remove_node(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id.to_string(),
                cascade,
            )),
        }
    }

    /// Upsert graph edge `id` from `src` to `dst` labelled `label` with the canonical-CBOR `props`
    /// (Graph upsert_edge).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn g_upsert_edge(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: &str,
        src: &str,
        dst: &str,
        label: &str,
        props: Vec<u8>,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let props = props_from_cbor(&props)?;
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_facet_workspace(&mut loom, workspace, FacetKind::Graph)?;
                loom_core::graph_upsert_edge(&mut loom, ns, name, id, src, dst, label, props)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Graph::upsert_edge(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id.to_string(),
                src.to_string(),
                dst.to_string(),
                label.to_string(),
                props,
            )),
        }
    }

    /// The canonical-CBOR record of graph edge `id` (Graph get_edge), or `None` when absent.
    pub(crate) fn g_get_edge(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                match loom_core::graph_get_edge(&loom, ns, name, id).map_err(|e| e.to_string())? {
                    Some(edge) => Ok(Some(graph_edge_cbor(&edge)?)),
                    None => Ok(None),
                }
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Graph::get_edge(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id.to_string(),
            )),
        }
    }

    /// Remove graph edge `id` (Graph remove_edge); returns whether it was present.
    pub(crate) fn g_remove_edge(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let present = loom_core::graph_remove_edge(&mut loom, ns, name, id)
                    .map_err(|e| e.to_string())?;
                if present {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(present)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Graph::remove_edge(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id.to_string(),
            )),
        }
    }

    /// The canonical-CBOR neighbor id list of graph node `id` (Graph neighbors).
    pub(crate) fn g_neighbors(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let ids =
                    loom_core::graph_neighbors(&loom, ns, name, id).map_err(|e| e.to_string())?;
                graph_strings_cbor(ids)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Graph::neighbors(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id.to_string(),
            )),
        }
    }

    /// The canonical-CBOR out-edge records of graph node `id` (Graph out_edges).
    pub(crate) fn g_out_edges(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let edges =
                    loom_core::graph_out_edges(&loom, ns, name, id).map_err(|e| e.to_string())?;
                graph_edges_cbor(edges)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Graph::out_edges(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id.to_string(),
            )),
        }
    }

    /// The canonical-CBOR in-edge records of graph node `id` (Graph in_edges).
    pub(crate) fn g_in_edges(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let edges =
                    loom_core::graph_in_edges(&loom, ns, name, id).map_err(|e| e.to_string())?;
                graph_edges_cbor(edges)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Graph::in_edges(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id.to_string(),
            )),
        }
    }

    /// The canonical-CBOR ids reachable from `start` (Graph reachable). `max_depth < 0` is unbounded;
    /// `via_label` empty is any label. The IDL `Graph.reachable` carries `max_depth` (i64), so the remote
    /// arm forwards it directly.
    pub(crate) fn g_reachable(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        start: &str,
        max_depth: i64,
        via_label: Option<&str>,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let depth = (max_depth >= 0).then_some(max_depth as usize);
                let via = via_label.filter(|value| !value.is_empty());
                let ids = loom_core::graph_reachable(&loom, ns, name, start, depth, via)
                    .map_err(|e| e.to_string())?;
                graph_strings_cbor(ids)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Graph::reachable(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                start.to_string(),
                max_depth,
                via_label.unwrap_or_default().to_string(),
            )),
        }
    }

    /// The canonical-CBOR shortest path from `from` to `to` (Graph shortest_path), or `None` when there
    /// is no path. `via_label` empty is any label.
    pub(crate) fn g_shortest_path(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        from: &str,
        to: &str,
        via_label: Option<&str>,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let via = via_label.filter(|value| !value.is_empty());
                match loom_core::graph_shortest_path(&loom, ns, name, from, to, via)
                    .map_err(|e| e.to_string())?
                {
                    Some(path) => Ok(Some(graph_strings_cbor(path)?)),
                    None => Ok(None),
                }
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Graph::shortest_path(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                from.to_string(),
                to.to_string(),
                via_label.unwrap_or_default().to_string(),
            )),
        }
    }

    pub(crate) fn g_query(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        query: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let query = loom_core::GraphQuery::parse_opencypher(query)
                    .map_err(|err| err.to_string())?;
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let result =
                    loom_core::graph_query(&loom, ns, name, &query).map_err(|e| e.to_string())?;
                Ok(loom_wire::graph::graph_query_result_to_cbor(&result))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Graph::query(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                query.to_string(),
            )),
        }
    }

    pub(crate) fn g_explain_query(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        query: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let query = loom_core::GraphQuery::parse_opencypher(query)
                    .map_err(|err| err.to_string())?;
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let explain = loom_core::graph_explain_query(&loom, ns, name, &query)
                    .map_err(|e| e.to_string())?;
                Ok(loom_wire::graph::graph_query_explain_to_cbor(&explain))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Graph::explain_query(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                query.to_string(),
            )),
        }
    }

    // ---- Vector ----
    //
    // Vectors cross as little-endian f32 bytes and metadata/filters/hits/entries as canonical CBOR; the
    // CLI's `vector_*` codecs are the same wire format as the server's `loom_wire::vector` codecs, so it
    // is a bytes pass-through. The `metric` (create) and accelerator `policy` (search) selectors are CLI
    // strings converted to the IDL's `i32` tags in the remote arm.

    /// Create vector set `name` with dimension `dim` and metric selector (Vector create).
    pub(crate) fn v_create(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        dim: u64,
        metric: &str,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let metric = parse_vector_metric(metric)?;
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = ensure_vector_workspace(&mut loom, workspace)?;
                loom_core::vector_create(&mut loom, ns, name, dim as usize, metric)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let metric_int: i32 = match parse_vector_metric(metric)? {
                    Metric::Cosine => 1,
                    Metric::L2 => 2,
                    Metric::Dot => 3,
                };
                remote.block(Vector::create(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    name.to_string(),
                    dim,
                    metric_int,
                ))
            }
        }
    }

    /// Upsert vector `id` from le-f32 `vector` bytes with canonical-CBOR `metadata` (Vector upsert).
    pub(crate) fn v_upsert(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: &str,
        vector: Vec<u8>,
        metadata: Vec<u8>,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let vector = vector_floats_from_bytes(&vector)?;
                let metadata = vector_metadata_from_cbor(&metadata)?;
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::vector_upsert(&mut loom, ns, name, id, vector, metadata)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Vector::upsert(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id.to_string(),
                vector,
                metadata,
            )),
        }
    }

    /// Upsert vector `id` with source text and an optional embedding model (Vector upsert_source).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn v_upsert_source(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: &str,
        vector: Vec<u8>,
        metadata: Vec<u8>,
        source_text: Vec<u8>,
        model_id: Option<String>,
        weights_digest: Option<String>,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let vector = vector_floats_from_bytes(&vector)?;
                let metadata = vector_metadata_from_cbor(&metadata)?;
                let source_text = String::from_utf8(source_text)
                    .map_err(|_| "vector source text must be UTF-8".to_string())?;
                let model = model_id
                    .map(|model_id| EmbeddingModel::new(model_id, vector.len(), weights_digest));
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::vector_upsert_with_source(
                    &mut loom,
                    ns,
                    name,
                    id,
                    vector,
                    metadata,
                    &source_text,
                    model,
                )
                .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Vector::upsert_source(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id.to_string(),
                vector,
                metadata,
                source_text,
                model_id,
                weights_digest,
            )),
        }
    }

    /// The canonical-CBOR `[vector, metadata]` entry for `id` (Vector get), or `None` when absent.
    pub(crate) fn v_get(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                match loom_core::vector_get(&loom, ns, name, id).map_err(|e| e.to_string())? {
                    Some((vector, metadata)) => Ok(Some(vector_get_cbor(vector, metadata)?)),
                    None => Ok(None),
                }
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Vector::get(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id.to_string(),
            )),
        }
    }

    /// The source text bytes for vector `id` (Vector source_text), or `None` when absent.
    pub(crate) fn v_source_text(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                Ok(loom_core::vector_source_text(&loom, ns, name, id)
                    .map_err(|e| e.to_string())?
                    .map(String::into_bytes))
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Vector::source_text(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id.to_string(),
            )),
        }
    }

    /// The vector ids for `name`, optionally filtered by `prefix` (Vector ids).
    pub(crate) fn v_ids(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::vector_ids(&loom, ns, name, prefix).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Vector::ids(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    name.to_string(),
                    prefix.map(str::to_string),
                ))?;
                cli_strings_from_remote(&wire)
            }
        }
    }

    /// The metadata index keys for `name` (Vector metadata_index_keys).
    pub(crate) fn v_index_keys(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
    ) -> Result<Vec<String>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom_core::vector_metadata_index_keys(&loom, ns, name).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(Vector::metadata_index_keys(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    name.to_string(),
                ))?;
                cli_strings_from_remote(&wire)
            }
        }
    }

    /// Create a metadata index on `key` (Vector create_metadata_index); returns whether it changed.
    pub(crate) fn v_create_index(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        key: &str,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let changed = loom_core::vector_create_metadata_index(&mut loom, ns, name, key)
                    .map_err(|e| e.to_string())?;
                if changed {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(changed)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Vector::create_metadata_index(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                key.to_string(),
            )),
        }
    }

    /// Drop the metadata index on `key` (Vector drop_metadata_index); returns whether it changed.
    pub(crate) fn v_drop_index(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        key: &str,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let changed = loom_core::vector_drop_metadata_index(&mut loom, ns, name, key)
                    .map_err(|e| e.to_string())?;
                if changed {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(changed)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Vector::drop_metadata_index(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                key.to_string(),
            )),
        }
    }

    /// Delete vector `id` (Vector delete); returns whether it was present.
    pub(crate) fn v_delete(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        id: &str,
    ) -> Result<bool, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let present =
                    loom_core::vector_delete(&mut loom, ns, name, id).map_err(|e| e.to_string())?;
                if present {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(present)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(Vector::delete(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                name.to_string(),
                id.to_string(),
            )),
        }
    }

    /// Search vector set `name` with the accelerator `policy` selector (Vector search_policy), returning
    /// the canonical-CBOR hits. `query` is le-f32 bytes; `filter` is canonical CBOR (empty = match all).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn v_search(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        name: &str,
        query: Vec<u8>,
        k: u64,
        filter: Vec<u8>,
        policy: &str,
        threshold: u64,
        ef: u64,
        pq_m: u64,
        pq_k: u64,
        pq_iters: u64,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let query = vector_floats_from_bytes(&query)?;
                let filter = vector_filter_from_cbor(&filter)?;
                let policy = match policy {
                    "exact" => AcceleratorPolicy::ExactAlways,
                    "approximate-pq" => AcceleratorPolicy::ApproximateAbove {
                        threshold: threshold as usize,
                    },
                    other => {
                        return Err(format!(
                            "unknown vector accelerator policy {other}; expected exact or approximate-pq"
                        ));
                    }
                };
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let hits = loom_core::vector_search_with_pq_policy(
                    &loom,
                    ns,
                    name,
                    &query,
                    k as usize,
                    &filter,
                    policy,
                    ef as usize,
                    pq_m as usize,
                    pq_k as usize,
                    pq_iters as usize,
                )
                .map_err(|e| e.to_string())?;
                vector_hits_cbor(&hits)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let policy_int: i32 = match policy {
                    "exact" => 0,
                    "approximate-pq" => 1,
                    other => {
                        return Err(format!(
                            "unknown vector accelerator policy {other}; expected exact or approximate-pq"
                        ));
                    }
                };
                remote.block(Vector::search_policy(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    name.to_string(),
                    query,
                    k,
                    filter,
                    policy_int,
                    threshold,
                    ef,
                    pq_m,
                    pq_k,
                    pq_iters,
                ))
            }
        }
    }

    // ---- VersionControl ----
    //
    // `branch`/`checkout` are void, `commit` returns the commit digest string, `diff` is a bytes
    // pass-through (the server `diff` is `loom.diff_commits`), and `merge` returns a typed `MergeOutcome`
    // that the remote arm decodes from the canonical `MergeResult` CBOR via
    // `loom_wire::vcs::merge_result_from_cbor`.

    /// The commit log of `workspace`'s current HEAD branch, newest first, as digest strings. The head
    /// is resolved via the `head_branch` accessor, then `log`.
    pub(crate) fn vcs_log(&self, keys: &KeyOpts, workspace: &str) -> Result<Vec<String>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let head = loom.registry().head_branch(ns).map_err(|e| e.to_string())?;
                Ok(loom
                    .log(ns, &head)
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .map(|c| c.to_string())
                    .collect())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let head = remote.block(VersionControl::head_branch(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                ))?;
                let commits = remote.block(VersionControl::log(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    head,
                ))?;
                Ok(commits.into_iter().map(|d| d.0).collect())
            }
        }
    }

    pub(crate) fn vcs_branch(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        branch: &str,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom.branch(ns, branch).map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(VersionControl::branch(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                branch.to_string(),
            )),
        }
    }

    /// Commit the working tree of `workspace` (VersionControl commit); returns the commit digest string.
    pub(crate) fn vcs_commit(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        author: &str,
        message: &str,
    ) -> Result<String, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let commit = loom
                    .commit(ns, author, message, now_ms())
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())?;
                Ok(commit.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(VersionControl::commit(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    author.to_string(),
                    message.to_string(),
                    now_ms(),
                ))?;
                Ok(wire.0)
            }
        }
    }

    /// Check out branch `branch` in `workspace` (VersionControl checkout).
    pub(crate) fn vcs_checkout(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        branch: &str,
    ) -> Result<(), String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                loom.checkout_branch(ns, branch)
                    .map_err(|e| e.to_string())?;
                save_loom(&mut loom).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(VersionControl::checkout(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                branch.to_string(),
            )),
        }
    }

    /// The canonical structural-diff bytes between commits `from` and `to` (VersionControl diff).
    pub(crate) fn vcs_diff(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<u8>, String> {
        match self {
            StoreClient::Local { locator } => {
                let loom = cli_open_loom_read(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let from = Digest::parse(from).map_err(|e| e.to_string())?;
                let to = Digest::parse(to).map_err(|e| e.to_string())?;
                loom.diff_commits(ns, from, to).map_err(|e| e.to_string())
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => remote.block(VersionControl::diff(
                &remote.client,
                remote.handle.clone(),
                workspace.to_string(),
                from.to_string(),
                to.to_string(),
            )),
        }
    }

    /// Merge branch `from` into the current branch of `workspace` (VersionControl merge), returning the
    /// typed outcome. The remote arm decodes the canonical `MergeResult` CBOR.
    pub(crate) fn vcs_merge(
        &self,
        keys: &KeyOpts,
        workspace: &str,
        from: &str,
        author: &str,
        cell_level: bool,
    ) -> Result<loom_core::MergeOutcome, String> {
        match self {
            StoreClient::Local { locator } => {
                let mut loom = cli_open_loom(locator, keys)?;
                let ns = resolve_ns(&loom, workspace)?;
                let outcome = if cell_level {
                    loom.merge_cell_level(ns, from, author, now_ms())
                } else {
                    loom.merge(ns, from, author, now_ms())
                }
                .map_err(|e| e.to_string())?;
                // Only persist a merge that changed something; a conflict leaves the store as-is.
                if !matches!(outcome, loom_core::MergeOutcome::Conflicts(_)) {
                    save_loom(&mut loom).map_err(|e| e.to_string())?;
                }
                Ok(outcome)
            }
            #[cfg(feature = "remote-client")]
            StoreClient::Remote(remote) => {
                let wire = remote.block(VersionControl::merge(
                    &remote.client,
                    remote.handle.clone(),
                    workspace.to_string(),
                    from.to_string(),
                    author.to_string(),
                    cell_level,
                    now_ms(),
                ))?;
                loom_wire::vcs::merge_result_from_cbor(&wire).map_err(|e| e.to_string())
            }
        }
    }
}

fn transfer_import_source_scope<'a>(
    kind: &str,
    workspace: &'a str,
    local_path: &'a str,
) -> Result<&'a str, String> {
    if kind == "car" && workspace.trim().is_empty() {
        if local_path.trim().is_empty() {
            Err("CAR transfer import source path must not be blank".to_string())
        } else {
            Ok(local_path)
        }
    } else {
        Ok(workspace)
    }
}

// CLI-output bridge helpers for the remote arm. Each takes a canonical server response and produces the
// exact bytes (or values) the CLI presentation layer expects, so a remote locator prints the same output
// as a local one. These are named around CLI output (not protocol wire) to keep the direction clear.

/// Decode a canonical string-list response (`Array(Text)`) into the `Vec<String>` the CLI list handlers
/// print or pass to `text_array_cbor`.
#[cfg(feature = "remote-client")]
fn cli_strings_from_remote(wire: &[u8]) -> Result<Vec<String>, String> {
    match loom_codec::decode(wire).map_err(|e| e.to_string())? {
        loom_codec::Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                loom_codec::Value::Text(text) => Ok(text),
                _ => Err("expected a CBOR text list from the remote endpoint".to_string()),
            })
            .collect(),
        _ => Err("expected a CBOR array from the remote endpoint".to_string()),
    }
}

/// Re-encode a canonical byte-blob-list response (`Array(Bytes(record))`, the server's
/// `bytes_list_to_cbor` form) with the CLI's `record_array_cbor`, so the remote output matches the local
/// `record_array_cbor(...)` output.
#[cfg(feature = "remote-client")]
fn cli_record_array_from_remote(wire: &[u8]) -> Result<Vec<u8>, String> {
    let records = match loom_codec::decode(wire).map_err(|e| e.to_string())? {
        loom_codec::Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                loom_codec::Value::Bytes(bytes) => Ok(bytes),
                _ => Err("expected a CBOR byte-string list from the remote endpoint".to_string()),
            })
            .collect::<Result<Vec<_>, String>>()?,
        _ => return Err("expected a CBOR array from the remote endpoint".to_string()),
    };
    record_array_cbor(records)
}

/// Re-encode a canonical `CollectionMeta` response with the CLI's `calendar_collection_cbor`.
#[cfg(feature = "remote-client")]
fn cli_calendar_collection_from_remote(meta: &[u8]) -> Result<Vec<u8>, String> {
    let meta = loom_core::calendar::CollectionMeta::decode(meta).map_err(|e| e.to_string())?;
    calendar_collection_cbor(&meta)
}

/// Re-encode a canonical contacts `BookMeta` response with the CLI's `metadata_cbor`.
#[cfg(feature = "remote-client")]
fn cli_contacts_book_from_remote(meta: &[u8]) -> Result<Vec<u8>, String> {
    let meta = loom_core::contacts::BookMeta::decode(meta).map_err(|e| e.to_string())?;
    metadata_cbor(&meta.display_name)
}

/// Re-encode a canonical mail `MailboxMeta` response with the CLI's `metadata_cbor`.
#[cfg(feature = "remote-client")]
fn cli_mail_mailbox_from_remote(meta: &[u8]) -> Result<Vec<u8>, String> {
    let meta = loom_core::mail::MailboxMeta::decode(meta).map_err(|e| e.to_string())?;
    metadata_cbor(&meta.display_name)
}

/// Decode the 6 protected-ref policy fields (`[bool, bool, bool, uint, bool, bool]`, the
/// `protected_ref_policy_to_cbor` field order) into a typed `ProtectedRefPolicy`.
#[cfg(feature = "remote-client")]
fn decode_protected_ref_policy_fields(
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
#[cfg(feature = "remote-client")]
fn cli_protected_ref_policy_from_remote(
    wire: &[u8],
) -> Result<loom_core::vcs::ProtectedRefPolicy, String> {
    match loom_codec::decode(wire).map_err(|e| e.to_string())? {
        loom_codec::Value::Array(items) => decode_protected_ref_policy_fields(&items),
        _ => Err("expected a CBOR array from the remote endpoint".to_string()),
    }
}

/// The wire-typed `acl_grant`/`acl_revoke` arguments, encoded from the raw CLI args for the remote arm.
/// `workspace` stays a raw string for the server to resolve (name or id), matching the server's
/// `acl_grant`/`acl_revoke` handling.
#[cfg(feature = "remote-client")]
struct AclWireArgs {
    effect: Vec<u8>,
    subject: String,
    workspace: Option<String>,
    domain: Option<Vec<u8>>,
    ref_glob: Option<String>,
    scopes: Option<Vec<Vec<u8>>>,
    rights: Option<Vec<Vec<u8>>>,
    predicate: Option<Vec<u8>>,
}

/// Encode the raw CLI ACL args into their canonical wire atoms via the `loom_wire` ACL codecs (the same
/// forms the server's `acl_grant_from_wire` decodes). Reuses the CLI's own parsers for effect / rights /
/// scopes / domain / predicate so the typed interpretation matches the local path exactly.
#[cfg(feature = "remote-client")]
fn encode_acl_wire_args(args: &AclGrantArgs<'_>) -> Result<AclWireArgs, String> {
    let effect = loom_wire::acl::acl_effect_to_wire(parse_acl_effect(args.effect)?);
    let domain = optional_acl_domain_arg(args.domain)?.map(|domain| vec![domain.stable_tag()]);
    // An empty scope list means "all resources" on both sides (the CLI uses `[AclScope::All]`, the server
    // treats an absent/empty list as all), so send `None` rather than an encoded `All` for parity.
    let scopes = if args.scopes.is_empty() {
        None
    } else {
        Some(
            args.scopes
                .iter()
                .map(|scope| {
                    loom_wire::acl::acl_scope_to_wire(&parse_acl_scope(scope)?)
                        .map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, String>>()?,
        )
    };
    let rights = Some(
        args.rights
            .iter()
            .map(|right| Ok(loom_wire::acl::acl_right_to_wire(parse_acl_right(right)?)))
            .collect::<Result<Vec<_>, String>>()?,
    );
    let predicate = match optional_acl_predicate(args.predicate_cel)? {
        Some(predicate) => {
            Some(loom_wire::acl::acl_predicate_to_wire(&predicate).map_err(|e| e.to_string())?)
        }
        None => None,
    };
    Ok(AclWireArgs {
        effect,
        subject: args.subject.to_string(),
        workspace: args.workspace.map(str::to_string),
        domain,
        ref_glob: args
            .ref_glob
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        scopes,
        rights,
        predicate,
    })
}

/// Decode a canonical `workspace_info_to_cbor` record (`[id, name, [facet_tag...], head]`) into a typed
/// `WorkspaceInfo` for the CLI workspace-list presentation.
#[cfg(feature = "remote-client")]
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

/// Decode a canonical `protected_ref_list` record (`[ref_name, ..6 policy fields]`).
#[cfg(feature = "remote-client")]
fn cli_named_protected_ref_from_remote(
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

/// Re-encode a canonical range response (`Array([Text(uid), Text(YYYYMMDDTHHMMSS)])`) with the CLI's
/// `calendar_range_cbor` by reconstructing the `Occurrence`s.
#[cfg(feature = "remote-client")]
fn cli_calendar_range_from_remote(wire: &[u8]) -> Result<Vec<u8>, String> {
    let occurrences = match loom_codec::decode(wire).map_err(|e| e.to_string())? {
        loom_codec::Value::Array(items) => items
            .into_iter()
            .map(|item| {
                let mut fields = match item {
                    loom_codec::Value::Array(fields) => fields.into_iter(),
                    _ => {
                        return Err(
                            "expected a [uid, start] pair from the remote endpoint".to_string()
                        );
                    }
                };
                let uid = match fields.next() {
                    Some(loom_codec::Value::Text(uid)) => uid,
                    _ => return Err("expected a text uid from the remote endpoint".to_string()),
                };
                let start = match fields.next() {
                    Some(loom_codec::Value::Text(bound)) => parse_calendar_datetime(&bound)?,
                    _ => return Err("expected a text start from the remote endpoint".to_string()),
                };
                Ok(loom_core::calendar::Occurrence { uid, start })
            })
            .collect::<Result<Vec<_>, String>>()?,
        _ => return Err("expected a CBOR array from the remote endpoint".to_string()),
    };
    calendar_range_cbor(&occurrences)
}

/// Normalize a CLI calendar date-time arg (`YYYYMMDD` or `YYYYMMDDTHHMMSS[Z]`) to the 15-char
/// `YYYYMMDDTHHMMSS` wall-clock string the server's range/window parser requires, so a remote range
/// accepts the same inputs as a local one.
#[cfg(feature = "remote-client")]
fn cli_window_bound(raw: &str) -> Result<String, String> {
    let dt = parse_calendar_datetime(raw)?;
    Ok(format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}",
        dt.year(),
        u8::from(dt.month()),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
    ))
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

    fn doc_get_binary(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<loom_core::document::DocumentBinary>, String> {
        let encoded = self.block(Document::get_binary(
            &self.client,
            self.handle.clone(),
            workspace.to_string(),
            collection.to_string(),
            id.to_string(),
        ))?;
        encoded
            .map(|bytes| {
                let (bytes, digest, entity_tag) =
                    loom_wire::document::binary_result_from_cbor(&bytes)
                        .map_err(|e| e.to_string())?;
                Ok(loom_core::document::DocumentBinary {
                    bytes,
                    digest: Digest::parse(&digest).map_err(|e| e.to_string())?,
                    entity_tag,
                })
            })
            .transpose()
    }

    fn doc_get_text(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
    ) -> Result<Option<loom_core::document::DocumentText>, String> {
        let Some(document) = self.doc_get_binary(workspace, collection, id)? else {
            return Ok(None);
        };
        let text = String::from_utf8(document.bytes).map_err(|_| {
            loom_types::LoomError::document_not_text("document payload is not valid UTF-8 text")
                .to_string()
        })?;
        Ok(Some(loom_core::document::DocumentText {
            text,
            digest: document.digest,
            entity_tag: document.entity_tag,
        }))
    }

    fn doc_put_binary_guarded(
        &self,
        workspace: &str,
        collection: &str,
        id: &str,
        content: Vec<u8>,
        expected_entity_tag: Option<&str>,
    ) -> Result<Digest, String> {
        if let Some(expected_entity_tag) = expected_entity_tag {
            let current = self.doc_get_binary(workspace, collection, id)?;
            match current {
                Some(document) if document.entity_tag == expected_entity_tag => {}
                Some(_) | None => {
                    return Err(loom_types::LoomError::cas_mismatch(
                        "document entity tag guard did not match",
                    )
                    .to_string());
                }
            }
        }
        let bytes = self.block(Document::put_binary(
            &self.client,
            self.handle.clone(),
            workspace.to_string(),
            collection.to_string(),
            id.to_string(),
            content,
            None,
        ))?;
        let (digest, _) =
            loom_wire::document::put_result_from_cbor(&bytes).map_err(|e| e.to_string())?;
        Digest::parse(&digest).map_err(|e| e.to_string())
    }

    /// Resolve `workspace` (an id or a name) to its `WorkspaceId` using the remote workspace list, so a
    /// remote `workspace rename`/`delete` can print the same resolved id as the local path. Mirrors
    /// `resolve_ns`: a parseable id is matched by id, otherwise the value is matched by name.
    fn resolve_workspace_id(&self, workspace: &str) -> Result<loom_core::WorkspaceId, String> {
        let wire = self.block(Workspaces::workspace_list(
            &self.client,
            self.handle.clone(),
        ))?;
        let infos = wire
            .iter()
            .map(|record| cli_workspace_info_from_remote(record))
            .collect::<Result<Vec<_>, String>>()?;
        let found = match loom_core::WorkspaceId::parse(workspace) {
            Ok(id) => infos
                .iter()
                .find(|info| info.id.as_bytes() == id.as_bytes())
                .map(|info| info.id),
            Err(_) => infos
                .iter()
                .find(|info| info.name == workspace)
                .map(|info| info.id),
        };
        found.ok_or_else(|| format!("workspace {workspace:?} not found"))
    }
}

/// A remote backend for the MCP host: forwards the KV MCP tool family to a `loom serve remote` endpoint
/// over the same connection/session path the CLI remote facade uses. Each call runs on this backend's
/// own IO runtime and is awaited over a std channel, so it is safe to invoke from inside the MCP host's
/// serving runtime (no nested `block_on`).
#[cfg(all(feature = "mcp", feature = "remote-client"))]
pub(crate) struct McpRemoteBackend {
    runtime: tokio::runtime::Runtime,
    client: Arc<RemoteLoomClient<Http2TlsTransport>>,
    handle: LoomSession,
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
        let (client, handle) = runtime.block_on(async move {
            let conn =
                RemoteConnection::connect(transport, &url, &ContextResolver::default(), mode)
                    .await
                    .map_err(|e| e.to_string())?;
            let client = RemoteLoomClient::new(conn);
            client
                .open_session(SessionAuth::Unauthenticated)
                .await
                .map_err(|e| e.to_string())?;
            let handle = Store::open(&client).await.map_err(|e| e.to_string())?;
            Ok::<_, String>((Arc::new(client), handle))
        })?;
        Ok(Self {
            runtime,
            client,
            handle,
        })
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
        let parsed = loom_core::WorkspaceId::parse(workspace).ok();
        for record in &records {
            let info =
                cli_workspace_info_from_remote(record).map_err(loom_types::LoomError::invalid)?;
            let matches = match &parsed {
                Some(id) => info.id.as_bytes() == id.as_bytes(),
                None => info.name == workspace,
            };
            if matches {
                return Ok(info.id);
            }
        }
        Err(loom_types::LoomError::not_found(format!(
            "workspace {workspace:?}"
        )))
    }
}

#[cfg(all(feature = "mcp", feature = "remote-client"))]
fn remote_backend_channel_closed() -> loom_types::LoomError {
    loom_types::LoomError::corrupt("remote MCP backend response channel closed")
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
impl uldren_loom_mcp::RemoteMcpBackend for McpRemoteBackend {
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
                Lanes::ticket_add(
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

        for name in uldren_loom_mcp::tools::SERVER_PROMOTED_TOOLS {
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

    /// The KV and queue commands round-trip through the `StoreClient::Remote` facade against a
    /// live `loom serve remote` endpoint. The client obtains its session over the carrier session route
    /// inside `RemoteStore::connect` (via `RemoteLoomClient::open_session`) - the test never calls
    /// `runtime.open_session` and never binds a session manually.
    #[test]
    fn kv_and_queue_round_trip_through_remote_facade() {
        let store = temp_store("rt");

        // Seed a calendar collection + event directly through the local engine so the local and remote
        // facade arms later read identical data (for the byte-for-byte output comparison below). The store
        // file is saved and released here, before the server binds it.
        {
            let keys = KeyOpts::default();
            let mut loom = cli_open_loom(&store, &keys).expect("open store for calendar seed");
            let ns =
                ensure_facet_workspace(&mut loom, "cal", FacetKind::Calendar).expect("calendar ws");
            loom_core::calendar::create_collection(
                &mut loom,
                ns,
                "alice",
                "work",
                &loom_core::calendar::CollectionMeta {
                    display_name: "Work".to_string(),
                    component_set: vec![loom_core::calendar::Component::Event],
                },
            )
            .expect("seed create_collection");
            let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:evt-1\r\nSUMMARY:Standup\r\nDTSTART:20240115T100000Z\r\nDTEND:20240115T103000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
            loom_core::calendar::put_ics(&mut loom, ns, "alice", "work", ics)
                .expect("seed put_ics");

            // Seed a contacts book + entry the same way (read-only fixtures for the local vs remote byte
            // comparison below).
            let con_ns =
                ensure_facet_workspace(&mut loom, "con", FacetKind::Contacts).expect("contacts ws");
            loom_core::contacts::create_book(
                &mut loom,
                con_ns,
                "alice",
                "personal",
                &loom_core::contacts::BookMeta {
                    display_name: "Personal".to_string(),
                },
            )
            .expect("seed create_book");
            let vcard = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:imported\r\nFN:Imported Person\r\nEMAIL:i@x.io\r\nEND:VCARD\r\n";
            loom_core::contacts::put_vcard(&mut loom, con_ns, "alice", "personal", vcard)
                .expect("seed put_vcard");

            // Seed a mailbox + ingested message (read-only fixtures for the local vs remote byte
            // comparison below).
            let mail_ns =
                ensure_facet_workspace(&mut loom, "mail", FacetKind::Mail).expect("mail ws");
            loom_core::mail::create_mailbox(
                &mut loom,
                mail_ns,
                "alice",
                "inbox",
                &loom_core::mail::MailboxMeta {
                    display_name: "Inbox".to_string(),
                },
            )
            .expect("seed create_mailbox");
            let rfc822 = b"From: a@x.io\r\nTo: b@y.io\r\nSubject: Standup\r\nDate: Mon, 15 Jan 2024 10:00:00 +0000\r\nMessage-ID: <msg-1@x.io>\r\n\r\nBody of the message.\r\n";
            loom_core::mail::ingest_message(&mut loom, mail_ns, "alice", "inbox", "msg-1", rfc822)
                .expect("seed ingest_message");

            // A workspace for the protected-ref policy tests to target (the ref-policy methods resolve an
            // existing workspace; they do not create one).
            ensure_facet_workspace(&mut loom, "refs", FacetKind::Vcs).expect("refs ws");

            save_loom(&mut loom).expect("save seed");
        }

        // A self-signed localhost cert for the server, loaded through the same TLS path `loom serve
        // remote` uses.
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let dir = std::env::temp_dir();
        let cert_path = dir.join(format!("loomcli-remote-facade-{}.crt", std::process::id()));
        let key_path = dir.join(format!("loomcli-remote-facade-{}.key", std::process::id()));
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

        // Stand up the server on its own runtime; the accept loop runs on it while the client connects.
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

        // The remote target the CLI resolves from a context: the live endpoint, trusting the
        // self-signed cert via the loopback `insecure-dev` selector.
        let target = RemoteTarget {
            url: format!("https://127.0.0.1:{}/apps/loom", addr.port()),
            auth: None,
            tls: Some("insecure-dev".to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };
        let client = StoreClient::Remote(Box::new(RemoteStore::connect(&target).expect("connect")));
        let keys = KeyOpts::default();

        // KV: unary write then unary read, plus range and delete.
        let key = loom_core::Value::Text("k".to_string());
        client
            .kv_put(&keys, "app", "c", key.clone(), b"v".to_vec())
            .expect("kv put");
        assert_eq!(
            client
                .kv_get(&keys, "app", "c", key.clone())
                .expect("kv get"),
            Some(b"v".to_vec())
        );
        // Range over the single key returns a non-empty canonical `[key, value]` map.
        assert!(
            !client
                .kv_range(
                    &keys,
                    "app",
                    "c",
                    loom_core::Value::Text(String::new()),
                    loom_core::Value::Text("~".to_string()),
                )
                .expect("kv range")
                .is_empty()
        );
        // Delete removes it (present -> true), and a second delete reports absent.
        assert!(
            client
                .kv_delete(&keys, "app", "c", key.clone())
                .expect("kv delete")
        );
        assert!(
            !client
                .kv_delete(&keys, "app", "c", key)
                .expect("kv delete absent")
        );

        // Queue: append, range, get, len, and consumer position/read/advance/reset.
        client
            .queue_append(&keys, "jobs", "in", b"a".to_vec())
            .expect("queue append a");
        client
            .queue_append(&keys, "jobs", "in", b"b".to_vec())
            .expect("queue append b");
        assert_eq!(
            client
                .queue_range(&keys, "jobs", "in", 0, 2)
                .expect("queue range"),
            vec![b"a".to_vec(), b"b".to_vec()]
        );
        assert_eq!(
            client.queue_get(&keys, "jobs", "in", 0).expect("queue get"),
            Some(b"a".to_vec())
        );
        assert_eq!(client.queue_len(&keys, "jobs", "in").expect("queue len"), 2);
        // A fresh consumer starts at 0, reads both entries without advancing, then advance/reset move it.
        assert_eq!(
            client
                .queue_consumer_position(&keys, "jobs", "in", "worker")
                .expect("queue position"),
            0
        );
        assert_eq!(
            client
                .queue_consumer_read(&keys, "jobs", "in", "worker", 10)
                .expect("queue read"),
            vec![b"a".to_vec(), b"b".to_vec()]
        );
        client
            .queue_consumer_advance(&keys, "jobs", "in", "worker", 2)
            .expect("queue advance");
        assert_eq!(
            client
                .queue_consumer_position(&keys, "jobs", "in", "worker")
                .expect("queue position after advance"),
            2
        );
        client
            .queue_consumer_reset(&keys, "jobs", "in", "worker", 1)
            .expect("queue reset");
        assert_eq!(
            client
                .queue_consumer_position(&keys, "jobs", "in", "worker")
                .expect("queue position after reset"),
            1
        );

        // CAS: put, then get/has/list/delete through the facade.
        let digest = client
            .cas_put(&keys, "blobs", b"hello".to_vec())
            .expect("cas put");
        assert_eq!(
            client.cas_get(&keys, "blobs", &digest).expect("cas get"),
            Some(b"hello".to_vec())
        );
        assert!(client.cas_has(&keys, "blobs", &digest).expect("cas has"));
        assert!(
            client
                .cas_list(&keys, "blobs")
                .expect("cas list")
                .contains(&digest)
        );
        assert!(
            client
                .cas_delete(&keys, "blobs", &digest)
                .expect("cas delete")
        );

        // Document: put, then get/list/delete through the facade.
        client
            .doc_put_text(&keys, "docs", "notes", "d1", "{\"x\":1}", None)
            .expect("doc put");
        assert_eq!(
            client
                .doc_get_text(&keys, "docs", "notes", "d1")
                .expect("doc get")
                .map(|document| document.text),
            Some("{\"x\":1}".to_string())
        );
        assert!(
            !client
                .doc_list_binary(&keys, "docs", "notes")
                .expect("doc list")
                .is_empty()
        );
        assert!(
            client
                .doc_delete(&keys, "docs", "notes", "d1")
                .expect("doc delete")
        );

        // Document indexing over the wire: create an index, find/query by it, inspect, rebuild, drop.
        client
            .doc_put_text(&keys, "docs", "people", "p1", "{\"age\":30}", None)
            .expect("doc put p1");
        client
            .doc_index_create(&keys, "docs", "people", "by_age", "age", false)
            .expect("doc index create");
        // list/status render as `{"indexes":[...]}` and include the new index.
        let idx_list = client
            .doc_index_list(&keys, "docs", "people")
            .expect("doc index list");
        assert!(idx_list.to_string().contains("by_age"));
        let idx_status = client
            .doc_index_statuses(&keys, "docs", "people")
            .expect("doc index status");
        assert!(idx_status.to_string().contains("by_age"));
        // find by the indexed value returns the document id.
        assert_eq!(
            client
                .doc_find(&keys, "docs", "people", "by_age", "30")
                .expect("doc find"),
            vec!["p1".to_string()]
        );
        // query returns the canonical `{"items":[...],"next_cursor":...}` shape.
        let query = br#"{"collection":"people"}"#;
        let query_result = client
            .doc_query(&keys, "docs", "people", query)
            .expect("doc query");
        assert!(query_result.get("items").is_some());
        client
            .doc_index_rebuild(&keys, "docs", "people", "by_age")
            .expect("doc index rebuild");
        assert!(
            client
                .doc_index_drop(&keys, "docs", "people", "by_age")
                .expect("doc index drop")
        );

        // Ledger: append, then get/len/head/verify.
        let seq = client
            .ledger_append(&keys, "audit", "log", b"e0".to_vec())
            .expect("ledger append");
        assert_eq!(
            client
                .ledger_get(&keys, "audit", "log", seq)
                .expect("ledger get"),
            Some(b"e0".to_vec())
        );
        assert_eq!(
            client
                .ledger_len(&keys, "audit", "log")
                .expect("ledger len"),
            1
        );
        assert!(
            client
                .ledger_head(&keys, "audit", "log")
                .expect("ledger head")
                .is_some()
        );
        client
            .ledger_verify(&keys, "audit", "log")
            .expect("ledger verify");

        // TimeSeries: put, then get/range.
        client
            .ts_put(&keys, "metrics", "cpu", 100, b"0.5".to_vec())
            .expect("ts put");
        assert_eq!(
            client.ts_get(&keys, "metrics", "cpu", 100).expect("ts get"),
            Some(b"0.5".to_vec())
        );
        assert!(
            !client
                .ts_range(&keys, "metrics", "cpu", 0, 200)
                .expect("ts range")
                .is_empty()
        );

        // Search: create an index, index a document, then get/ids/delete/query through the facade.
        // Inputs are canonical CBOR built the same way the CLI would read them from a file: the mapping
        // is a field -> [type_tag, stored, faceted] map, the document is a field -> value map, and the
        // request is a [query_node, limit, offset] array.
        use loom_codec::Value as WireValue;
        let mapping = loom_codec::encode(&WireValue::Map(vec![(
            WireValue::Text("body".to_string()),
            WireValue::Array(vec![
                WireValue::Uint(0),
                WireValue::Bool(true),
                WireValue::Bool(false),
            ]),
        )]))
        .unwrap();
        client
            .search_create(&keys, "search", "notes", mapping)
            .expect("search create");
        let doc = loom_codec::encode(&WireValue::Map(vec![(
            WireValue::Text("body".to_string()),
            WireValue::Text("hello world".to_string()),
        )]))
        .unwrap();
        client
            .search_index(&keys, "search", "notes", b"doc-1".to_vec(), doc.clone())
            .expect("search index");
        assert_eq!(
            client
                .search_get(&keys, "search", "notes", b"doc-1".to_vec())
                .expect("search get"),
            Some(doc)
        );
        assert!(
            !client
                .search_ids(&keys, "search", "notes", None)
                .expect("search ids")
                .is_empty()
        );
        let request = loom_codec::encode(&WireValue::Array(vec![
            WireValue::Array(vec![
                WireValue::Uint(0),
                WireValue::Text("body".to_string()),
                WireValue::Text("hello".to_string()),
            ]),
            WireValue::Uint(10),
            WireValue::Uint(0),
        ]))
        .unwrap();
        assert!(
            !client
                .search_query(&keys, "search", "notes", request)
                .expect("search query")
                .is_empty()
        );
        assert!(
            client
                .search_delete(&keys, "search", "notes", b"doc-1".to_vec())
                .expect("search delete")
        );

        // Calendar: capture the CLI-presentation output of the divergent read methods over the wire (these
        // exercise the remote bridge re-encoders), read a clean method, and cover the clean create/delete
        // write path. The reads target the seeded read-only "work" collection so they stay stable for the
        // local vs remote byte comparison after shutdown.
        let remote_cal_list = client
            .cal_list_entries(&keys, "cal", "alice", "work")
            .expect("remote cal list_entries");
        let remote_cal_range = client
            .cal_range(&keys, "cal", "alice", "work", "20240101", "20241231")
            .expect("remote cal range");
        let remote_cal_collection = client
            .cal_get_collection(&keys, "cal", "alice", "work")
            .expect("remote cal get_collection");
        let remote_cal_collections = client
            .cal_list_collections(&keys, "cal", "alice")
            .expect("remote cal list_collections");
        assert!(
            client
                .cal_get_entry(&keys, "cal", "alice", "work", "evt-1")
                .expect("remote cal get_entry")
                .is_some()
        );
        assert!(
            !remote_cal_list.is_empty(),
            "seeded event should appear in list_entries"
        );
        // Clean write coverage over the wire: create then delete a scratch collection.
        client
            .cal_create_collection(
                &keys,
                "cal",
                "alice",
                "scratch",
                "Scratch".to_string(),
                vec![loom_core::calendar::Component::Event],
            )
            .expect("remote cal create_collection");
        assert!(
            client
                .cal_delete_collection(&keys, "cal", "alice", "scratch")
                .expect("remote cal delete_collection")
        );

        // Contacts: capture the divergent read output over the wire (bridge re-encoders), a clean read,
        // and clean create/delete write coverage.
        let remote_con_list = client
            .con_list_entries(&keys, "con", "alice", "personal")
            .expect("remote con list_entries");
        let remote_con_search = client
            .con_search(&keys, "con", "alice", "personal", "Imported")
            .expect("remote con search");
        let remote_con_book = client
            .con_get_book(&keys, "con", "alice", "personal")
            .expect("remote con get_book");
        let remote_con_books = client
            .con_list_books(&keys, "con", "alice")
            .expect("remote con list_books");
        assert!(
            client
                .con_get_entry(&keys, "con", "alice", "personal", "imported")
                .expect("remote con get_entry")
                .is_some()
        );
        assert!(
            !remote_con_list.is_empty(),
            "seeded contact should appear in list_entries"
        );
        client
            .con_create_book(&keys, "con", "alice", "scratchbook", "Scratch".to_string())
            .expect("remote con create_book");
        assert!(
            client
                .con_delete_book(&keys, "con", "alice", "scratchbook")
                .expect("remote con delete_book")
        );

        // Mail: do the mutating writes first (set flags on the seeded message; create+delete a scratch
        // mailbox) so both the remote captures and the post-shutdown local reads observe the same final
        // state, then capture the divergent/clean reads for the byte comparison.
        client
            .mail_set_flags(
                &keys,
                "mail",
                "alice",
                "inbox",
                "msg-1",
                vec!["\\Seen".to_string()],
            )
            .expect("remote mail set_flags");
        client
            .mail_create_mailbox(&keys, "mail", "alice", "archive", "Archive".to_string())
            .expect("remote mail create_mailbox");
        assert!(
            client
                .mail_delete_mailbox(&keys, "mail", "alice", "archive")
                .expect("remote mail delete_mailbox")
        );
        let remote_mail_list = client
            .mail_list_messages(&keys, "mail", "alice", "inbox")
            .expect("remote mail list_messages");
        let remote_mail_search = client
            .mail_search(&keys, "mail", "alice", "inbox", "Standup")
            .expect("remote mail search");
        let remote_mail_mailbox = client
            .mail_get_mailbox(&keys, "mail", "alice", "inbox")
            .expect("remote mail get_mailbox");
        let remote_mail_mailboxes = client
            .mail_list_mailboxes(&keys, "mail", "alice")
            .expect("remote mail list_mailboxes");
        let remote_mail_flags = client
            .mail_get_flags(&keys, "mail", "alice", "inbox", "msg-1")
            .expect("remote mail get_flags");
        assert!(
            client
                .mail_get_message(&keys, "mail", "alice", "inbox", "msg-1")
                .expect("remote mail get_message")
                .is_some()
        );
        assert!(
            !remote_mail_list.is_empty(),
            "seeded message should appear in list_messages"
        );
        assert!(
            !remote_mail_flags.is_empty(),
            "set_flags should persist a flag on the message"
        );

        // Files: write then read a top-level path over the wire.
        client
            .fs_write_file(&keys, "fsapp", "notes.txt", b"hello files".to_vec())
            .expect("remote fs write_file");
        let remote_fs_read = client
            .fs_read_file(&keys, "fsapp", "notes.txt")
            .expect("remote fs read_file");
        assert_eq!(remote_fs_read, b"hello files".to_vec());

        // ProtectedRefs: mutations first (set "main"; set+remove a scratch ref) so remote captures and the
        // post-shutdown local reads observe the same final state, then capture get/list for the JSON
        // output-equivalence comparison.
        let policy = loom_core::vcs::ProtectedRefPolicy {
            fast_forward_only: true,
            signed_commits_required: false,
            signed_ref_advance_required: true,
            required_review_count: 2,
            retention_lock: false,
            governance_lock: true,
        };
        client
            .pr_set(&keys, "refs", "branch/main", policy.clone())
            .expect("remote pr set main");
        client
            .pr_set(&keys, "refs", "branch/scratch", policy.clone())
            .expect("remote pr set scratch");
        assert!(
            client
                .pr_remove(&keys, "refs", "branch/scratch")
                .expect("remote pr remove scratch")
        );
        let remote_pr_get = client
            .pr_get(&keys, "refs", "branch/main")
            .expect("remote pr get");
        let remote_pr_list = client.pr_list(&keys, "refs").expect("remote pr list");
        assert!(remote_pr_get.is_some(), "the set policy should be readable");
        assert!(!remote_pr_list.is_empty());

        // Workspaces: create/list/rename/delete over the wire (session-level management), exercising the
        // round-trip and the remote id resolution that reproduces the rename/delete output. Local
        // `workspace *` gates on a configured identity store that this bare test store lacks, so this
        // section does not byte-compare against a local run: authz runs server-side on the remote arm.
        let ws_id = client
            .ws_create(&keys, "wsnew", None)
            .expect("remote ws create");
        assert!(!ws_id.is_empty());
        assert!(
            !client.ws_list(&keys).expect("remote ws list").is_empty(),
            "the created workspace should appear in the list"
        );
        assert_eq!(
            client
                .ws_rename(&keys, "wsnew", "wsrenamed")
                .expect("remote ws rename"),
            ws_id,
            "rename should resolve to the created workspace id"
        );
        assert_eq!(
            client
                .ws_delete(&keys, "wsrenamed")
                .expect("remote ws delete"),
            ws_id
        );

        // Acl: grant/list/revoke over the wire (global-admin management). Output-equivalence for the group
        // is covered by protected-refs above; local `acl *` gates on a configured identity store this bare
        // test lacks, so this is remote-only (the accepted local-admin-vs-server-authz split).
        let acl_rights = vec!["read".to_string()];
        let acl_scopes: Vec<String> = Vec::new();
        let acl_args = || AclGrantArgs {
            effect: "allow",
            subject: "everyone",
            workspace: None,
            domain: None,
            rights: &acl_rights,
            ref_glob: None,
            scopes: &acl_scopes,
            predicate_cel: None,
        };
        client
            .acl_grant(&keys, acl_args())
            .expect("remote acl grant");
        assert!(
            !client.acl_list(&keys).expect("remote acl list").is_empty(),
            "the granted rule should appear in acl list"
        );
        assert!(
            client
                .acl_revoke(&keys, acl_args())
                .expect("remote acl revoke"),
            "the granted rule should be revocable"
        );

        // Columnar: create + append then read every accessor over the wire, and compact. The CLI codecs
        // are the same wire format as the server's, so this is a clean bytes pass-through - verified by the
        // scan byte-equality (local vs remote) after shutdown.
        let col_columns =
            columnar_columns_cbor(vec![("v".to_string(), loom_core::ColumnType::Int)]).unwrap();
        let col_row = columnar_values_cbor(vec![loom_core::Value::Int(7)]).unwrap();
        client
            .col_create(&keys, "cols", "t", col_columns, 1024)
            .expect("remote col create");
        client
            .col_append(&keys, "cols", "t", col_row)
            .expect("remote col append");
        assert_eq!(
            client
                .col_rows(&keys, "cols", "t")
                .expect("remote col rows"),
            1
        );
        let remote_col_scan = client
            .col_scan(&keys, "cols", "t")
            .expect("remote col scan");
        assert!(!remote_col_scan.is_empty());
        assert!(
            !client
                .col_columns(&keys, "cols", "t")
                .expect("remote col columns")
                .is_empty()
        );
        assert!(
            !client
                .col_inspect(&keys, "cols", "t")
                .expect("remote col inspect")
                .is_empty()
        );
        assert!(
            !client
                .col_source_digest(&keys, "cols", "t")
                .expect("remote col source_digest")
                .is_empty()
        );
        let select_cols =
            loom_codec::encode(&loom_codec::Value::Array(vec![loom_codec::Value::Text(
                "v".to_string(),
            )]))
            .unwrap();
        assert!(
            !client
                .col_select(&keys, "cols", "t", select_cols, Vec::new())
                .expect("remote col select")
                .is_empty()
        );
        let aggregates =
            loom_codec::encode(&loom_codec::Value::Array(vec![loom_codec::Value::Array(
                vec![loom_codec::Value::Uint(0)],
            )]))
            .unwrap();
        assert!(
            !client
                .col_aggregate(&keys, "cols", "t", aggregates, Vec::new())
                .expect("remote col aggregate")
                .is_empty()
        );
        client
            .col_compact(&keys, "cols", "t")
            .expect("remote col compact");

        // Graph: upsert two nodes + an edge, then read every accessor over the wire; get_node
        // byte-equality (local vs remote) after shutdown confirms the shared graph wire format.
        let node_props = loom_codec::encode(&loom_codec::Value::Map(vec![(
            loom_codec::Value::Text("k".to_string()),
            loom_codec::Value::Bytes(b"v".to_vec()),
        )]))
        .unwrap();
        client
            .g_upsert_node(&keys, "graph", "g", "n1", node_props)
            .expect("remote g upsert_node n1");
        client
            .g_upsert_node(&keys, "graph", "g", "n2", Vec::new())
            .expect("remote g upsert_node n2");
        client
            .g_upsert_edge(&keys, "graph", "g", "e1", "n1", "n2", "links", Vec::new())
            .expect("remote g upsert_edge");
        let remote_g_node = client
            .g_get_node(&keys, "graph", "g", "n1")
            .expect("remote g get_node");
        assert!(remote_g_node.is_some());
        assert!(
            client
                .g_get_edge(&keys, "graph", "g", "e1")
                .expect("remote g get_edge")
                .is_some()
        );
        assert!(
            !client
                .g_neighbors(&keys, "graph", "g", "n1")
                .expect("remote g neighbors")
                .is_empty()
        );
        assert!(
            !client
                .g_out_edges(&keys, "graph", "g", "n1")
                .expect("remote g out_edges")
                .is_empty()
        );
        assert!(
            !client
                .g_in_edges(&keys, "graph", "g", "n2")
                .expect("remote g in_edges")
                .is_empty()
        );
        assert!(
            !client
                .g_reachable(&keys, "graph", "g", "n1", -1, None)
                .expect("remote g reachable")
                .is_empty()
        );
        assert!(
            client
                .g_shortest_path(&keys, "graph", "g", "n1", "n2", None)
                .expect("remote g shortest_path")
                .is_some()
        );
        assert!(
            !client
                .g_query(
                    &keys,
                    "graph",
                    "g",
                    "MATCH p = (a)-[r:links]->(b) RETURN p, r, a, b",
                )
                .expect("remote g query")
                .is_empty()
        );
        assert!(
            !client
                .g_explain_query(&keys, "graph", "g", "MATCH (n) RETURN n")
                .expect("remote g explain_query")
                .is_empty()
        );
        // A bounded reachable works over the wire too (the IDL carries max_depth).
        assert!(
            !client
                .g_reachable(&keys, "graph", "g", "n1", 2, None)
                .expect("remote g reachable bounded")
                .is_empty()
        );
        // Write coverage: remove the edge and a node (n1 is left intact for the byte comparison).
        assert!(
            client
                .g_remove_edge(&keys, "graph", "g", "e1")
                .expect("remote g remove_edge")
        );
        client
            .g_remove_node(&keys, "graph", "g", "n2", false)
            .expect("remote g remove_node");

        // Vector: create + upsert then exercise every accessor over the wire (get/ids/index keys/source/
        // search/delete); get byte-equality (local vs remote) after shutdown confirms the shared format.
        let vec_bytes = vector_floats_to_bytes(&[1.0f32, 2.0]);
        client
            .v_create(&keys, "vec", "v", 2, "cosine")
            .expect("remote v create");
        client
            .v_upsert(&keys, "vec", "v", "a", vec_bytes.clone(), Vec::new())
            .expect("remote v upsert");
        let remote_v_get = client.v_get(&keys, "vec", "v", "a").expect("remote v get");
        assert!(remote_v_get.is_some());
        assert_eq!(
            client.v_ids(&keys, "vec", "v", None).expect("remote v ids"),
            vec!["a".to_string()]
        );
        assert!(
            client
                .v_create_index(&keys, "vec", "v", "kind")
                .expect("remote v create_index")
        );
        assert!(
            client
                .v_index_keys(&keys, "vec", "v")
                .expect("remote v index_keys")
                .contains(&"kind".to_string())
        );
        assert!(
            client
                .v_drop_index(&keys, "vec", "v", "kind")
                .expect("remote v drop_index")
        );
        client
            .v_upsert_source(
                &keys,
                "vec",
                "v",
                "b",
                vec_bytes.clone(),
                Vec::new(),
                b"hello".to_vec(),
                None,
                None,
            )
            .expect("remote v upsert_source");
        assert_eq!(
            client
                .v_source_text(&keys, "vec", "v", "b")
                .expect("remote v source_text"),
            Some(b"hello".to_vec())
        );
        assert!(
            !client
                .v_search(
                    &keys,
                    "vec",
                    "v",
                    vec_bytes,
                    5,
                    Vec::new(),
                    "exact",
                    4096,
                    0,
                    1,
                    16,
                    8,
                )
                .expect("remote v search")
                .is_empty()
        );
        assert!(
            client
                .v_delete(&keys, "vec", "v", "b")
                .expect("remote v delete")
        );

        // VersionControl: a commit workflow over the wire (commit/branch/checkout/diff/merge). Commit
        // digests embed a server-side timestamp so they are not locally reproducible, but the structural
        // diff between two committed digests is - checked by the diff byte-equality after shutdown.
        client
            .kv_put(
                &keys,
                "vcsws",
                "c",
                loom_core::Value::Text("k1".to_string()),
                b"v1".to_vec(),
            )
            .expect("vcs seed kv 1");
        let c1 = client
            .vcs_commit(&keys, "vcsws", "tester", "first")
            .expect("remote vcs commit 1");
        assert!(!c1.is_empty());
        client
            .kv_put(
                &keys,
                "vcsws",
                "c",
                loom_core::Value::Text("k2".to_string()),
                b"v2".to_vec(),
            )
            .expect("vcs seed kv 2");
        let c2 = client
            .vcs_commit(&keys, "vcsws", "tester", "second")
            .expect("remote vcs commit 2");
        assert_ne!(c1, c2, "the two commits should differ");
        let remote_vcs_diff = client
            .vcs_diff(&keys, "vcsws", &c1, &c2)
            .expect("remote vcs diff");
        assert!(!remote_vcs_diff.is_empty());
        client
            .vcs_branch(&keys, "vcsws", "feature")
            .expect("remote vcs branch");
        client
            .vcs_checkout(&keys, "vcsws", "feature")
            .expect("remote vcs checkout feature");
        client
            .vcs_checkout(&keys, "vcsws", "main")
            .expect("remote vcs checkout main");
        let outcome = client
            .vcs_merge(&keys, "vcsws", "feature", "tester", false)
            .expect("remote vcs merge");
        assert!(
            !matches!(outcome, loom_core::MergeOutcome::Conflicts(_)),
            "merging a non-divergent branch should not conflict"
        );

        // Identity runs against its own store + endpoint: an `identity add` introduces a second principal
        // and flips the store into authenticated mode, which would make the shared store's post-shutdown
        // local reads require auth. Isolating it keeps the other families' fixtures untouched.
        {
            let id_store = temp_store("id");
            {
                let id_keys = KeyOpts::default();
                let loom = cli_open_loom(&id_store, &id_keys).expect("open identity store");
                // Root-only identity store: the control plane the remote identity commands mutate.
                let identity = loom_core::IdentityStore::new(WorkspaceId::v4_from_bytes([7; 16]));
                loom.store()
                    .save_identity_store(&identity)
                    .expect("seed identity store");
            }
            let id_server = server_rt
                .block_on(crate::serve_cmd::bind_remote_endpoint(
                    &id_store,
                    &options,
                    tls.server_config(),
                ))
                .expect("bind identity endpoint");
            let id_addr = id_server.local_addr();
            let id_target = RemoteTarget {
                url: format!("https://127.0.0.1:{}/apps/loom", id_addr.port()),
                auth: None,
                tls: Some("insecure-dev".to_string()),
                discovery: LocatorDiscovery::Default,
                discovery_path: None,
                connect_timeout_ms: None,
                request_timeout_ms: None,
            };
            let id_client = StoreClient::Remote(Box::new(
                RemoteStore::connect(&id_target).expect("id connect"),
            ));

            // list, add, rename-handle, revoke-role, and public-key list over the wire.
            let id_list_before = id_client.id_list(&keys).expect("remote identity list");
            assert!(id_list_before.contains("\"authenticated_mode\""));
            let new_principal = id_client
                .id_add(
                    &keys,
                    "svc-bot",
                    "Service Bot",
                    loom_core::PrincipalKind::Service,
                )
                .expect("remote identity add");
            assert!(!new_principal.is_empty());
            let renamed = id_client
                .id_rename_handle(&keys, &new_principal, "svc-bot-2")
                .expect("remote identity rename-handle");
            assert_eq!(renamed, new_principal);
            // The snapshot carries the new principal with its renamed handle, the field the wire form
            // round-trips.
            let id_list_after = id_client
                .id_list(&keys)
                .expect("remote identity list after");
            assert!(id_list_after.contains(&new_principal));
            assert!(id_list_after.contains("\"handle\":\"svc-bot-2\""));
            // Revoking a role the principal never had removes nothing.
            let unheld_role = loom_core::WorkspaceId::v4_from_bytes([88; 16]).to_string();
            assert!(
                !id_client
                    .id_revoke_role(&keys, &new_principal, &unheld_role)
                    .expect("remote identity revoke-role")
            );
            // The public-key list renders from the snapshot (no keys seeded here).
            assert_eq!(
                id_client
                    .id_public_key_list(&keys)
                    .expect("remote identity public-key list"),
                "[]"
            );
            id_server.shutdown();
        }

        server.shutdown();
        drop(server_rt);

        // The same calendar commands through the local facade produce byte-for-byte identical
        // output to the remote arm.
        {
            let keys = KeyOpts::default();
            let local = StoreClient::Local {
                locator: store.clone(),
            };
            assert_eq!(
                local
                    .cal_list_entries(&keys, "cal", "alice", "work")
                    .expect("local cal list_entries"),
                remote_cal_list
            );
            assert_eq!(
                local
                    .cal_range(&keys, "cal", "alice", "work", "20240101", "20241231")
                    .expect("local cal range"),
                remote_cal_range
            );
            assert_eq!(
                local
                    .cal_get_collection(&keys, "cal", "alice", "work")
                    .expect("local cal get_collection"),
                remote_cal_collection
            );
            assert_eq!(
                local
                    .cal_list_collections(&keys, "cal", "alice")
                    .expect("local cal list_collections"),
                remote_cal_collections
            );
            assert_eq!(
                local
                    .con_list_entries(&keys, "con", "alice", "personal")
                    .expect("local con list_entries"),
                remote_con_list
            );
            assert_eq!(
                local
                    .con_search(&keys, "con", "alice", "personal", "Imported")
                    .expect("local con search"),
                remote_con_search
            );
            assert_eq!(
                local
                    .con_get_book(&keys, "con", "alice", "personal")
                    .expect("local con get_book"),
                remote_con_book
            );
            assert_eq!(
                local
                    .con_list_books(&keys, "con", "alice")
                    .expect("local con list_books"),
                remote_con_books
            );
            assert_eq!(
                local
                    .mail_list_messages(&keys, "mail", "alice", "inbox")
                    .expect("local mail list_messages"),
                remote_mail_list
            );
            assert_eq!(
                local
                    .mail_search(&keys, "mail", "alice", "inbox", "Standup")
                    .expect("local mail search"),
                remote_mail_search
            );
            assert_eq!(
                local
                    .mail_get_mailbox(&keys, "mail", "alice", "inbox")
                    .expect("local mail get_mailbox"),
                remote_mail_mailbox
            );
            assert_eq!(
                local
                    .mail_list_mailboxes(&keys, "mail", "alice")
                    .expect("local mail list_mailboxes"),
                remote_mail_mailboxes
            );
            assert_eq!(
                local
                    .mail_get_flags(&keys, "mail", "alice", "inbox", "msg-1")
                    .expect("local mail get_flags"),
                remote_mail_flags
            );
            assert_eq!(
                local
                    .fs_read_file(&keys, "fsapp", "notes.txt")
                    .expect("local fs read_file"),
                remote_fs_read
            );
            // Output-equivalence: the CLI JSON printers produce byte-for-byte identical text for the local
            // and remote protected-ref reads.
            let local_pr_list = local.pr_list(&keys, "refs").expect("local pr list");
            assert_eq!(
                crate::helpers::protected_ref_policies_json(&local_pr_list),
                crate::helpers::protected_ref_policies_json(&remote_pr_list)
            );
            let local_pr_get = local
                .pr_get(&keys, "refs", "branch/main")
                .expect("local pr get");
            let local_get_json = local_pr_get
                .map(|policy| crate::helpers::protected_ref_policy_json("main", &policy));
            let remote_get_json = remote_pr_get
                .map(|policy| crate::helpers::protected_ref_policy_json("main", &policy));
            assert_eq!(local_get_json, remote_get_json);
            // Columnar scan output must be byte-for-byte identical local vs remote (confirms the CLI and
            // server share the columnar wire format).
            assert_eq!(
                local.col_scan(&keys, "cols", "t").expect("local col scan"),
                remote_col_scan
            );
            assert_eq!(
                local
                    .g_get_node(&keys, "graph", "g", "n1")
                    .expect("local g get_node"),
                remote_g_node
            );
            assert_eq!(
                local.v_get(&keys, "vec", "v", "a").expect("local v get"),
                remote_v_get
            );
            assert_eq!(
                local
                    .vcs_diff(&keys, "vcsws", &c1, &c2)
                    .expect("local vcs diff"),
                remote_vcs_diff
            );
        }

        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
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

    /// Exercises `files mkdir`/`write`/`ls`/`read`/`delete` through the `StoreClient` facade against a
    /// live `loom serve remote` endpoint over self-signed TLS, asserting the remote path produces the same
    /// observable results as a local store.
    #[test]
    fn files_dir_surface_local_and_remote_over_tls() {
        let store = temp_store("files-remote");

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
                &store,
                &options,
                tls.server_config(),
            ))
            .expect("bind remote endpoint");
        let addr = server.local_addr();

        let target = RemoteTarget {
            url: format!("https://127.0.0.1:{}/apps/loom", addr.port()),
            auth: None,
            // insecure-dev accepts the self-signed loopback cert (see the TLS-trust test below).
            tls: Some("insecure-dev".to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };

        let keys = KeyOpts::default();
        let remote_client =
            StoreClient::Remote(Box::new(RemoteStore::connect(&target).expect("connect")));
        let local_store = temp_store("files-local");
        let local_client = StoreClient::Local {
            locator: local_store.clone(),
        };

        // Run the identical sequence against the local store and the remote (TLS) store; both must agree.
        for (label, client) in [("local", &local_client), ("remote", &remote_client)] {
            client
                .fs_mkdir(&keys, "w", "docs", false)
                .unwrap_or_else(|e| panic!("{label} mkdir: {e}"));
            client
                .fs_write_file(&keys, "w", "docs/readme.txt", b"hello".to_vec())
                .unwrap_or_else(|e| panic!("{label} write nested: {e}"));
            client
                .fs_write_file(&keys, "w", "top.txt", b"top".to_vec())
                .unwrap_or_else(|e| panic!("{label} write top: {e}"));

            assert_eq!(
                client
                    .fs_read_file(&keys, "w", "docs/readme.txt")
                    .expect("read"),
                b"hello",
                "{label} read"
            );
            assert_eq!(
                client.fs_ls(&keys, "w").expect("ls"),
                vec!["docs/readme.txt".to_string(), "top.txt".to_string()],
                "{label} ls (sorted file paths)"
            );

            // A non-empty directory cannot be deleted without `recursive`.
            assert!(
                client.fs_delete(&keys, "w", "docs", false).is_err(),
                "{label}: non-empty dir delete without recursive must error"
            );
            // Recursive delete removes the directory and its contents; a plain file deletes directly.
            client
                .fs_delete(&keys, "w", "docs", true)
                .unwrap_or_else(|e| panic!("{label} recursive delete: {e}"));
            client
                .fs_delete(&keys, "w", "top.txt", false)
                .unwrap_or_else(|e| panic!("{label} file delete: {e}"));
            assert!(
                client
                    .fs_ls(&keys, "w")
                    .expect("ls after delete")
                    .is_empty(),
                "{label}: all entries removed"
            );
        }

        drop(server);
        server_rt.shutdown_background();
        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(&local_store);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
    }

    /// Task 555: byte-transfer interchange parity + conformance over a served self-signed-TLS endpoint
    /// (`specs/0067` §17). Part A runs the identical export -> import -> read-back sequence against a
    /// local store and the remote (TLS) store for the archive family and asserts local-vs-remote byte
    /// parity of the exported payload, import-report summary parity, and content round-trip. CAR
    /// restores are checked separately because CAR carries the source workspace identity. Part B
    /// drives the raw `Transfer` client against the endpoint to prove backpressure credit, idempotent
    /// `write` replay, finalize-once `finish`, bad-`final_digest` rejection, and unsupported-kind
    /// rejection.
    #[test]
    fn transfer_interchange_local_and_remote_parity_over_tls() {
        let remote_store = temp_store("transfer-remote");

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

        let keys = KeyOpts::default();
        let remote_client =
            StoreClient::Remote(Box::new(RemoteStore::connect(&target).expect("connect")));
        let local_store = temp_store("transfer-local");
        let local_client = StoreClient::Local {
            locator: local_store.clone(),
        };

        let content = b"hello transfer parity payload".to_vec();
        // Seed an identical Files tree in "src" on both stores.
        for (label, client) in [("local", &local_client), ("remote", &remote_client)] {
            client
                .fs_write_file(&keys, "src", "hello.txt", content.clone())
                .unwrap_or_else(|e| panic!("{label} seed: {e}"));
        }

        // Part A: archive-family export/import parity + content round-trip. `gzip` is excluded because
        // single-file gzip export is unsupported; `car` is checked separately (it derives its own
        // workspace from the manifest, so a `dst`-workspace read-back does not apply).
        for kind in ["tar", "tar-zstd", "tar-gzip", "zip"] {
            let export_path = |label: &str| {
                dir.join(format!(
                    "loom-transfer-{label}-{kind}-{}.bin",
                    std::process::id()
                ))
            };
            let dst = format!("dst_{}", kind.replace('-', "_"));

            let mut summaries = Vec::new();
            let mut payloads = Vec::new();
            for (label, client) in [("local", &local_client), ("remote", &remote_client)] {
                let path = export_path(label);
                let path_str = path.to_string_lossy().into_owned();
                client
                    .transfer_export(&keys, "src", kind, None, &path_str)
                    .unwrap_or_else(|e| panic!("{label} export {kind}: {e}"));
                let payload = std::fs::read(&path).expect("read exported payload");
                assert!(!payload.is_empty(), "{label} {kind}: empty export");

                let summary = client
                    .transfer_import(&keys, &dst, kind, &path_str, true, false)
                    .unwrap_or_else(|e| panic!("{label} import {kind}: {e}"));

                assert_eq!(
                    client
                        .fs_read_file(&keys, &dst, "hello.txt")
                        .expect("read back"),
                    content,
                    "{label} {kind}: content round-trip"
                );

                summaries.push(summary);
                payloads.push(payload);
                let _ = std::fs::remove_file(&path);
            }
            // Local-vs-remote parity: byte-identical export payload and identical import-report summary
            // for the same deterministic codec.
            assert_eq!(payloads[0], payloads[1], "{kind}: export byte parity");
            assert_eq!(
                summaries[0], summaries[1],
                "{kind}: import-report summary parity"
            );
        }

        // `car` restores the manifest workspace over both arms. The CAR payload includes the source
        // workspace id, so local and remote stores with independently-created workspaces are not
        // byte-identical.
        {
            for (label, client) in [("local", &local_client), ("remote", &remote_client)] {
                let path = dir.join(format!(
                    "loom-transfer-{label}-car-{}.car",
                    std::process::id()
                ));
                let path_str = path.to_string_lossy().into_owned();
                client
                    .transfer_export(&keys, "src", "car", None, &path_str)
                    .unwrap_or_else(|e| panic!("{label} export car: {e}"));
                let payload = std::fs::read(&path).expect("read car");
                assert!(!payload.is_empty(), "{label} car: empty export");
                client
                    .ws_delete(&keys, "src")
                    .unwrap_or_else(|e| panic!("{label} delete src before car import: {e}"));
                client
                    .transfer_import(&keys, "", "car", &path_str, false, false)
                    .unwrap_or_else(|e| panic!("{label} import car: {e}"));
                assert_eq!(
                    client
                        .fs_read_file(&keys, "src", "hello.txt")
                        .expect("read car-restored src"),
                    content,
                    "{label} car: content restored"
                );
                let _ = std::fs::remove_file(&path);
            }
        }

        // A facade import of an unsupported kind is rejected on both arms.
        for (label, client) in [("local", &local_client), ("remote", &remote_client)] {
            let path = dir.join(format!(
                "loom-transfer-{label}-none-{}.bin",
                std::process::id()
            ));
            std::fs::write(&path, b"unused").unwrap();
            assert!(
                client
                    .transfer_import(&keys, "w", "parquet", &path.to_string_lossy(), false, false)
                    .is_err(),
                "{label}: parquet import must be rejected (unsupported kind)"
            );
            let _ = std::fs::remove_file(&path);
        }

        // Part B: raw `Transfer` client conformance against the remote endpoint. Uses a fresh tar
        // export of "src" as the payload.
        let StoreClient::Remote(remote) = &remote_client else {
            unreachable!("remote client is remote");
        };
        let raw_path = dir.join(format!("loom-transfer-raw-{}.tar", std::process::id()));
        let raw_path_str = raw_path.to_string_lossy().into_owned();
        remote_client
            .transfer_export(&keys, "src", "tar", None, &raw_path_str)
            .expect("raw export");
        let payload = std::fs::read(&raw_path).expect("read raw payload");

        let algo = loom_core::Algo::from_name(
            &remote
                .block(Store::digest_algo(&remote.client))
                .expect("digest_algo"),
        )
        .expect("algo");
        let good_digest = WireDigest(loom_core::Digest::hash(algo, &payload).to_string());
        let bad_digest = WireDigest(loom_core::Digest::hash(algo, b"tampered").to_string());

        let transfer = remote
            .block(Transfer::transfer_import_open(
                &remote.client,
                remote.handle.clone(),
                "rawdst".to_string(),
                "tar".to_string(),
                Vec::new(),
            ))
            .expect("raw open");

        let accept0 = remote
            .block(Transfer::transfer_import_write(
                &remote.client,
                remote.handle.clone(),
                transfer.clone(),
                payload.clone(),
                0,
                None,
            ))
            .expect("raw write");
        let (accepted0, credit0) =
            loom_wire::transfer::transfer_accept_from_cbor(&accept0).expect("decode accept");
        assert_eq!(
            accepted0,
            payload.len() as u64,
            "accepted-bytes tracks the write"
        );
        assert_eq!(
            accepted0 + credit0,
            loom_interchange_io::transfer::StagingLimits::DEFAULT_MAX_TOTAL_BYTES,
            "accepted + credit equals the staging allowance (backpressure)"
        );

        // Idempotent replay of an already-accepted seq is a no-op with unchanged counters.
        let accept_replay = remote
            .block(Transfer::transfer_import_write(
                &remote.client,
                remote.handle.clone(),
                transfer.clone(),
                payload.clone(),
                0,
                None,
            ))
            .expect("raw replay write");
        assert_eq!(
            loom_wire::transfer::transfer_accept_from_cbor(&accept_replay).unwrap(),
            (accepted0, credit0),
            "replayed write is a no-op"
        );

        let report1 = remote
            .block(Transfer::transfer_import_finish(
                &remote.client,
                remote.handle.clone(),
                transfer.clone(),
                true,
                false,
                good_digest.clone(),
            ))
            .expect("raw finish");
        // Finalize-once: a replayed finish returns the same report without reapplying.
        let report2 = remote
            .block(Transfer::transfer_import_finish(
                &remote.client,
                remote.handle.clone(),
                transfer.clone(),
                true,
                false,
                good_digest.clone(),
            ))
            .expect("raw finish replay");
        assert_eq!(report1, report2, "finish is finalize-once");

        // A bad final digest is rejected at finish.
        let bad_transfer = remote
            .block(Transfer::transfer_import_open(
                &remote.client,
                remote.handle.clone(),
                "rawbad".to_string(),
                "tar".to_string(),
                Vec::new(),
            ))
            .expect("raw open bad");
        remote
            .block(Transfer::transfer_import_write(
                &remote.client,
                remote.handle.clone(),
                bad_transfer.clone(),
                payload.clone(),
                0,
                None,
            ))
            .expect("raw write bad");
        assert!(
            remote
                .block(Transfer::transfer_import_finish(
                    &remote.client,
                    remote.handle.clone(),
                    bad_transfer,
                    true,
                    false,
                    bad_digest,
                ))
                .is_err(),
            "a mismatched final digest must be rejected at finish"
        );

        // An unsupported kind is rejected at open over the wire.
        assert!(
            remote
                .block(Transfer::transfer_import_open(
                    &remote.client,
                    remote.handle.clone(),
                    "w".to_string(),
                    "parquet".to_string(),
                    Vec::new(),
                ))
                .is_err(),
            "open of an unsupported kind must be rejected server-side"
        );

        let _ = std::fs::remove_file(&raw_path);
        drop(server);
        server_rt.shutdown_background();
        let _ = std::fs::remove_file(&remote_store);
        let _ = std::fs::remove_file(&local_store);
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

    /// Verifies coordination across two independent `RemoteStore` connections to the same served
    /// self-signed-TLS endpoint. A write on connection A is visible to a read on connection B, and both
    /// connections stay usable concurrently.
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
        let port = server.local_addr().port();
        let target = || RemoteTarget {
            url: format!("https://127.0.0.1:{port}/apps/loom"),
            auth: None,
            tls: Some("insecure-dev".to_string()),
            discovery: LocatorDiscovery::Default,
            discovery_path: None,
            connect_timeout_ms: None,
            request_timeout_ms: None,
        };

        let keys = KeyOpts::default();
        // Two independent connections (separate sessions) to the same served endpoint.
        let conn_a = StoreClient::Remote(Box::new(
            RemoteStore::connect(&target()).expect("connect A"),
        ));
        let conn_b = StoreClient::Remote(Box::new(
            RemoteStore::connect(&target()).expect("connect B"),
        ));

        // A writes; B (a different connection) sees the committed write.
        conn_a
            .fs_write_file(&keys, "w", "shared.txt", b"from-a".to_vec())
            .expect("A write");
        assert_eq!(
            conn_b
                .fs_read_file(&keys, "w", "shared.txt")
                .expect("B read"),
            b"from-a",
            "connection B must see connection A's committed write"
        );

        // The reverse direction works too, and A remains usable after B has written - both connections
        // stay live concurrently over the one TLS endpoint.
        conn_b
            .fs_write_file(&keys, "w", "shared2.txt", b"from-b".to_vec())
            .expect("B write");
        assert_eq!(
            conn_a
                .fs_read_file(&keys, "w", "shared2.txt")
                .expect("A read"),
            b"from-b",
            "connection A must see connection B's committed write"
        );

        drop(server);
        server_rt.shutdown_background();
        let _ = std::fs::remove_file(&store);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
    }

    /// The four audited identity commands print byte-identical CLI output through the local facade and the
    /// remote facade. Two stores are seeded identically (fixed ids) so the deterministic revoke output
    /// matches (same record, same audit sequence); the remote revoke reconstructs the record by reading it
    /// before revoking, and `revoke_*` returns the record unchanged, so the two agree. Create is
    /// shape-checked over remote because it mints a fresh id.
    #[cfg(feature = "remote-client")]
    #[test]
    fn identity_audited_commands_match_local_and_remote() {
        use loom_core::{
            ExternalCredentialKind, ExternalCredentialSpec, IdentityPublicKeySpec, IdentityStore,
            WorkspaceId,
        };

        let root = WorkspaceId::v4_from_bytes([7; 16]);
        let cred_id = WorkspaceId::v4_from_bytes([0x21; 16]);
        let key_id = WorkspaceId::v4_from_bytes([0x22; 16]);
        let seed = |store: &str| {
            let keys = KeyOpts::default();
            let loom = cli_open_loom(store, &keys).expect("open store for identity seed");
            let mut identity = IdentityStore::new(root);
            identity
                .set_passphrase(root, "rootpw", b"root-salt-bytes")
                .expect("seed root passphrase");
            identity
                .create_external_credential(
                    root,
                    ExternalCredentialSpec {
                        id: cred_id,
                        kind: ExternalCredentialKind::OidcSubject,
                        label: "ci".to_string(),
                        issuer: "https://issuer".to_string(),
                        subject: "svc-bot".to_string(),
                        material_digest: None,
                    },
                )
                .expect("seed external credential");
            identity
                .add_public_key(
                    root,
                    IdentityPublicKeySpec {
                        id: key_id,
                        label: "laptop".to_string(),
                        algorithm: "Ed25519".to_string(),
                        public_key: vec![7u8; 32],
                    },
                )
                .expect("seed public key");
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
        };

        let local_store = temp_store("id-audit-local");
        let remote_store = temp_store("id-audit-remote");
        seed(&local_store);
        seed(&remote_store);

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
        // A bad passphrase must fail at session open, not later at mutation time.
        assert!(
            RemoteStore::connect_with_auth(
                &target,
                SessionAuth::Passphrase {
                    principal: *root.as_bytes(),
                    passphrase: b"wrong".to_vec(),
                },
            )
            .is_err(),
            "a bad passphrase must fail session open"
        );

        let remote = StoreClient::Remote(Box::new(
            RemoteStore::connect_with_auth(
                &target,
                SessionAuth::Passphrase {
                    principal: *root.as_bytes(),
                    passphrase: b"rootpw".to_vec(),
                },
            )
            .expect("authenticated connect"),
        ));
        let local = StoreClient::Local {
            locator: local_store.clone(),
        };
        // The local arm authenticates as root through the passphrase-file key source.
        let pw_path = dir.join(format!("loomcli-idaudit-pw-{}.txt", std::process::id()));
        std::fs::write(&pw_path, "rootpw").unwrap();
        let keys = KeyOpts {
            auth_principal: Some(root.to_string()),
            auth_source: crate::KeySource::File(pw_path.to_string_lossy().into_owned()),
            ..KeyOpts::default()
        };

        let remote_cred = remote
            .id_external_credential_revoke(&keys, cred_id)
            .expect("remote revoke external credential");
        let local_cred = local
            .id_external_credential_revoke(&keys, cred_id)
            .expect("local revoke external credential");
        assert_eq!(remote_cred, local_cred);
        assert!(remote_cred.contains("\"seq\":0"));

        let remote_key = remote
            .id_revoke_public_key(&keys, key_id)
            .expect("remote revoke public key");
        let local_key = local
            .id_revoke_public_key(&keys, key_id)
            .expect("local revoke public key");
        assert_eq!(remote_key, local_key);
        assert!(remote_key.contains("\"seq\":1"));

        let created = remote
            .id_add_public_key(
                &keys,
                root,
                "ci-key".to_string(),
                "Ed25519".to_string(),
                vec![9u8; 32],
            )
            .expect("remote add public key");
        assert!(created.contains("\"seq\":") && created.contains("\"public_key\":"));
        assert!(created.contains("\"algorithm\":\"Ed25519\""));

        server.shutdown();
        drop(server_rt);
        let _ = std::fs::remove_file(&local_store);
        let _ = std::fs::remove_file(&remote_store);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
        let _ = std::fs::remove_file(&pw_path);
    }

    /// The app-credential commands print byte-identical CLI output through the local and remote facades.
    /// Two stores are seeded with the same fixed-id credential, so revoke output matches exactly. Create
    /// mints a fresh secret server-side (fresh id, so not byte-compared); the test proves the one-time
    /// secret token is returned by create and never echoed by a subsequent identity list.
    #[cfg(feature = "remote-client")]
    #[test]
    fn app_credential_commands_match_local_and_remote() {
        use loom_core::WorkspaceId;
        use loom_core::identity::IdentityStore;

        let root = WorkspaceId::v4_from_bytes([7; 16]);
        let cred_id = WorkspaceId::v4_from_bytes([0x41; 16]);
        let seed = |store: &str| {
            let keys = KeyOpts::default();
            let loom = cli_open_loom(store, &keys).expect("open store for seed");
            let mut identity = IdentityStore::new(root);
            identity
                .set_passphrase(root, "rootpw", b"root-salt-bytes")
                .expect("seed root passphrase");
            identity
                .create_app_credential(root, cred_id, "seeded", &[9u8; 32], &[8u8; 16])
                .expect("seed app credential");
            loom.store()
                .save_identity_store(&identity)
                .expect("save seed");
            let mut acl = loom_core::AclStore::new();
            acl.allow(
                loom_core::AclSubject::Principal(root),
                None,
                None,
                [loom_core::AclRight::Admin],
            )
            .expect("grant root global admin");
            loom.store().save_acl_store(&acl).expect("save acl seed");
        };
        let local_store = temp_store("appcred-local");
        let remote_store = temp_store("appcred-remote");
        seed(&local_store);
        seed(&remote_store);

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
        assert!(
            RemoteStore::connect_with_auth(
                &target,
                SessionAuth::Passphrase {
                    principal: *root.as_bytes(),
                    passphrase: b"wrong".to_vec(),
                },
            )
            .is_err(),
            "a bad passphrase must fail session open"
        );
        let remote = StoreClient::Remote(Box::new(
            RemoteStore::connect_with_auth(
                &target,
                SessionAuth::Passphrase {
                    principal: *root.as_bytes(),
                    passphrase: b"rootpw".to_vec(),
                },
            )
            .expect("authenticated connect"),
        ));
        let local = StoreClient::Local {
            locator: local_store.clone(),
        };
        let pw_path = dir.join(format!("loomcli-appcred-pw-{}.txt", std::process::id()));
        std::fs::write(&pw_path, "rootpw").unwrap();
        let keys = KeyOpts {
            auth_principal: Some(root.to_string()),
            auth_source: crate::KeySource::File(pw_path.to_string_lossy().into_owned()),
            ..KeyOpts::default()
        };

        let remote_rev = remote
            .id_app_credential_revoke(&keys, cred_id)
            .expect("remote revoke app credential");
        let local_rev = local
            .id_app_credential_revoke(&keys, cred_id)
            .expect("local revoke app credential");
        assert_eq!(remote_rev, local_rev);
        assert!(remote_rev.contains("\"seq\":0"));
        assert!(!remote_rev.contains("secret"));

        let created = remote
            .id_app_credential_create(&keys, root, "ci-runner".to_string())
            .expect("remote create app credential");
        assert!(
            created.contains("\"seq\":")
                && created.contains("\"credential\":")
                && created.contains("\"secret\":\"loom_app_"),
            "create output: {created}"
        );
        let token = created
            .rsplit("\"secret\":\"")
            .next()
            .unwrap()
            .trim_end_matches("\"}");
        let listed = remote.id_list(&keys).expect("remote identity list");
        assert!(
            !listed.contains(token),
            "one-time secret token leaked into the identity list"
        );

        server.shutdown();
        drop(server_rt);
        let _ = std::fs::remove_file(&local_store);
        let _ = std::fs::remove_file(&remote_store);
        let _ = std::fs::remove_file(&cert_path);
        let _ = std::fs::remove_file(&key_path);
        let _ = std::fs::remove_file(&pw_path);
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
        let anon = StoreClient::Remote(Box::new(
            RemoteStore::connect(&target).expect("anon connect"),
        ));
        assert!(
            anon.admin_policy_get_json().is_err(),
            "unauthenticated StoreAdmin must fail closed over the wire"
        );

        // Authenticated global admin: stat and policy set succeed over the wire.
        let admin = StoreClient::Remote(Box::new(
            RemoteStore::connect_with_auth(
                &target,
                SessionAuth::Passphrase {
                    principal: *root.as_bytes(),
                    passphrase: b"rootpw".to_vec(),
                },
            )
            .expect("authenticated connect"),
        ));
        let _stat = admin.admin_stat_json().expect("remote stat as admin");
        let set = admin
            .admin_policy_set_json(true)
            .expect("remote policy set");
        assert!(set.contains("\"fips_required\":true"));
        let get = admin.admin_policy_get_json();
        if runtime_profile().fips_capable {
            assert!(
                get.expect("remote policy get")
                    .contains("\"fips_required\":true")
            );
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
