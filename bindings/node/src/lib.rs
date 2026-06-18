//! Node.js binding for Uldren Loom via napi-rs. Published as `@uldrenai/loom`.
//!
//! napi maps snake_case Rust to camelCase JS, so `blob_digest` is `blobDigest` in JavaScript.
//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.
//!
//! These functions are thin napi shims over the C-ABI surface: each takes the same
//! loom-path/facet/workspace/target/options argument list as its C ABI counterpart plus an optional
//! passphrase, so a one-to-one mapping routinely needs eight arguments. That is deliberate, so
//! `too_many_arguments` is allowed crate-wide rather than splitting the shims into structs that would
//! diverge from the C ABI.
#![allow(clippy::too_many_arguments)]

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use loom_codec::{Value as CborValue, encode as cbor_encode};
use loom_core::calendar::{
    self, CalendarEntry, CollectionMeta, Component, DateTime, IcalDate, IcalMonth, IcalTime,
};
use loom_core::contacts::{self, BookMeta, ContactEntry};
use loom_core::keys::{EncryptionMeta, KeySpec, Suite};
use loom_core::mail::{self, MailMessage, MailboxMeta};
use loom_core::tabular::Value;
use loom_core::vcs::ChangeKind;
use loom_core::workspace::{FacetKind, WorkspaceId};
use loom_core::{
    AclEffect, AclGrant, AclPredicate, AclRight, AclScope, AclScopeKind, AclStore, AclSubject,
    Algo, ConflictResolution, Digest, ExternalCredential, ExternalCredentialKind,
    ExternalCredentialSpec, IdentityPublicKeySpec, IdentityRole, IdentityStore, Loom, Object,
    Principal, PrincipalKind, ProtectedRefPolicy, WatchCursor, WatchSelector, WsSelector,
    watch_batch_to_cbor,
};
use loom_result::result_view;
use loom_result::result_view::{ResultPayload, ShowVariable, Statement};
use loom_sql::{LoomSqlStore, lookup_cbor, result_cbor};
use loom_store::{
    FileStore, LocalOpenAuth, attach_local_auth, daemon,
    open_loom_read_unlocked as store_open_loom_read_unlocked,
    open_loom_unlocked as store_open_loom_unlocked, save_loom,
};
use napi::bindgen_prelude::{Array, AsyncTask, BigInt, Null, Object as JsObject, Uint8Array};
use napi::{Env, Task};
use napi_derive::napi;

mod admin;
mod archive;
mod calendar_fns;
mod cas;
mod chat;
mod columnar;
mod contacts_fns;
mod daemon_fns;
mod dataframe;
mod document;
mod drive;
mod files;
mod graph;
mod kv;
mod lanes;
mod ledger;
mod mail_fns;
mod meetings;
mod pages;
mod queue;
mod search;
mod sql;
mod telemetry;
mod tickets;
mod timeseries;
mod vcs;
mod vector;
mod workspace;

/// Resolve a held passphrase to a [`KeySpec`] for the loom-store unlocking openers; `None` passes
/// through (an unencrypted loom ignores it). The host (JS) supplies the passphrase; no environment
/// variable is read.
fn key_spec(key: Option<&str>) -> Option<KeySpec> {
    key.map(KeySpec::passphrase)
}

fn install_acl_predicate_evaluator(loom: &mut Loom<FileStore>) {
    loom.set_acl_predicate_evaluator(std::sync::Arc::new(loom_compute::CelAclPredicateEvaluator));
}

/// Classify a Loom locator string for this binding's local-vs-remote split.
///
/// A local filesystem path is returned unchanged; a `file://` URL is stripped to its local path. An
/// `http(s)://` URL (or any remote locator) routes to the remote branch, which this binding does not wire:
/// with the `remote` feature off (default) it returns a stable "remote requires the remote feature" error;
/// with `remote` on it returns a "not yet wired" error. Alias TOML is not consulted here; a bare non-path
/// string stays a local path, so no alias config is read.
fn normalize_locator(locator: &str) -> loom_core::error::Result<String> {
    use loom_core::error::{Code, LoomError};
    if let Some(rest) = locator.strip_prefix("file://") {
        return Ok(rest.to_string());
    }
    if locator.starts_with("https://") || locator.starts_with("http://") {
        #[cfg(not(feature = "remote"))]
        {
            return Err(LoomError::new(
                Code::Unsupported,
                "remote Loom locators require the remote feature in this binding",
            ));
        }
        #[cfg(feature = "remote")]
        {
            return Err(LoomError::new(
                Code::Unsupported,
                "remote Loom locators are not yet wired in this binding (constructor surface only)",
            ));
        }
    }
    Ok(locator.to_string())
}

fn open_loom_unlocked(
    path: &str,
    key: Option<&KeySpec>,
) -> loom_core::error::Result<Loom<FileStore>> {
    let path = normalize_locator(path)?;
    let mut loom = store_open_loom_unlocked(&path, key)?;
    install_acl_predicate_evaluator(&mut loom);
    Ok(loom)
}

fn open_loom_read_unlocked(
    path: &str,
    key: Option<&KeySpec>,
) -> loom_core::error::Result<Loom<FileStore>> {
    let path = normalize_locator(path)?;
    let mut loom = store_open_loom_read_unlocked(&path, key)?;
    install_acl_predicate_evaluator(&mut loom);
    Ok(loom)
}

fn local_auth_sql(
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
) -> napi::Result<LocalOpenAuth> {
    match (auth_principal, auth_passphrase) {
        (Some(principal), Some(passphrase)) => Ok(LocalOpenAuth {
            principal: Some(WorkspaceId::parse(&principal).map_err(reason)?),
            passphrase: Some(passphrase),
            ..Default::default()
        }),
        (None, None) => Ok(LocalOpenAuth::default()),
        _ => Err(napi::Error::from_reason(
            "authPrincipal and authPassphrase must be supplied together".to_string(),
        )),
    }
}

/// Parse the identity-profile selector.
fn parse_profile(s: &str) -> napi::Result<Algo> {
    match s {
        "default" | "blake3" => Ok(Algo::Blake3),
        "fips" | "sha256" => Ok(Algo::Sha256),
        other => Err(napi::Error::from_reason(format!(
            "unknown identity profile {other:?} (expected `default`/`blake3` or `fips`/`sha256`)"
        ))),
    }
}

fn rng_fill(buf: &mut [u8]) -> napi::Result<()> {
    getrandom::getrandom(buf).map_err(|e| napi::Error::from_reason(format!("rng: {e}")))
}

fn random_workspace_id() -> napi::Result<WorkspaceId> {
    let mut id = [0u8; 16];
    rng_fill(&mut id)?;
    Ok(WorkspaceId::v4_from_bytes(id))
}

/// Open the file store and unlock it with `passphrase` so its DEK is available for a wrap update.
fn open_store_for_key_update(path: &str, passphrase: &str) -> napi::Result<FileStore> {
    let fs = FileStore::open(path).map_err(reason)?;
    fs.unlock(&KeySpec::passphrase(passphrase))
        .map_err(reason)?;
    fs.validate_runtime_policy().map_err(reason)?;
    Ok(fs)
}

/// Build a raw-KEK credential from caller-supplied bytes, which must be exactly 32 (a 256-bit key).
fn kek_spec(kek: &[u8]) -> napi::Result<KeySpec> {
    let bytes: [u8; loom_core::keys::KEY_LEN] = kek.try_into().map_err(|_| {
        napi::Error::from_reason(format!(
            "a raw KEK must be exactly {} bytes (256 bits), got {}",
            loom_core::keys::KEY_LEN,
            kek.len()
        ))
    })?;
    Ok(KeySpec::raw_kek(bytes))
}

/// Fresh per-wrap salt (16 bytes) and AEAD nonce (24 bytes).
fn fresh_wrap_material() -> napi::Result<([u8; 16], [u8; 24])> {
    let mut salt = [0u8; 16];
    let mut wrap_nonce = [0u8; 24];
    rng_fill(&mut salt)?;
    rng_fill(&mut wrap_nonce)?;
    Ok((salt, wrap_nonce))
}

/// Resolve a workspace for a CAS write by UUID or name, ensuring the `cas` facet exists. A name not yet
/// present is created carrying the `cas` facet; an unknown UUID is `NOT_FOUND`.
fn ensure_cas_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Cas,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Cas)
        .map_err(reason)?;
    Ok(ns)
}

/// Resolve a workspace for a kv write by UUID or name, ensuring the `kv` facet exists.
fn ensure_kv_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Kv,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Kv)
        .map_err(reason)?;
    Ok(ns)
}

fn ensure_lanes_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Document,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Document)
        .map_err(reason)?;
    Ok(ns)
}

// ---------------------------------------------------------------------------------------------------
// Document / Time-series / Ledger facet wrappers, mirroring the kv pattern over loom-core.
// ---------------------------------------------------------------------------------------------------

fn ensure_doc_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Document,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Document)
        .map_err(reason)?;
    Ok(ns)
}

fn ensure_ts_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::TimeSeries,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::TimeSeries)
        .map_err(reason)?;
    Ok(ns)
}

fn ensure_ledger_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Ledger,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Ledger)
        .map_err(reason)?;
    Ok(ns)
}

/// Resolve a workspace for a graph write by UUID or name, ensuring the `graph` facet exists.
fn ensure_graph_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Graph,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Graph)
        .map_err(reason)?;
    Ok(ns)
}

/// Resolve a workspace for a vector write by UUID or name, ensuring the `vector` facet exists.
fn ensure_vector_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Vector,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Vector)
        .map_err(reason)?;
    Ok(ns)
}

/// Resolve a workspace for a columnar write by UUID or name, ensuring the `columnar` facet exists.
fn ensure_columnar_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Columnar,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Columnar)
        .map_err(reason)?;
    Ok(ns)
}

/// Resolve a workspace for a search write by UUID or name, ensuring the `search` facet exists.
fn ensure_search_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Search,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Search)
        .map_err(reason)?;
    Ok(ns)
}

// ---------------------------------------------------------------------------------------------------
// Calendar / Contacts / Mail facet wrappers, mirroring the C ABI `*_ns` helpers in loom-ffi.
// ---------------------------------------------------------------------------------------------------

/// Resolve a workspace for a calendar write by UUID or name, ensuring the `calendar` facet exists. A name
/// not yet present is created carrying the `calendar` facet; an unknown UUID is `NOT_FOUND`.
fn ensure_cal_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Calendar,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Calendar)
        .map_err(reason)?;
    Ok(ns)
}

/// Resolve a workspace for a contacts write by UUID or name, ensuring the `contacts` facet exists.
fn ensure_card_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Contacts,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Contacts)
        .map_err(reason)?;
    Ok(ns)
}

/// Resolve a workspace for a mail write by UUID or name, ensuring the `mail` facet exists.
fn ensure_mail_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Mail,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Mail)
        .map_err(reason)?;
    Ok(ns)
}

/// Parse a comma-separated component list ("event,todo"; an empty string is the empty set) into the
/// `component_set` of a [`CollectionMeta`]. An unknown token is `INVALID_ARGUMENT`.
fn parse_component_set(components: &str) -> napi::Result<Vec<Component>> {
    let mut out = Vec::new();
    for tok in components.split(',') {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        match tok {
            "event" => out.push(Component::Event),
            "todo" => out.push(Component::Todo),
            other => {
                return Err(napi::Error::from_reason(format!(
                    "loom_cal: unknown component {other:?}"
                )));
            }
        }
    }
    Ok(out)
}

/// Map a component-filter string to the calendar facet's optional component: "" -> `None`,
/// "event" -> `Some(Event)`, "todo" -> `Some(Todo)`. Any other token is `INVALID_ARGUMENT`.
fn parse_component_filter(component: &str) -> napi::Result<Option<Component>> {
    match component {
        "" => Ok(None),
        "event" => Ok(Some(Component::Event)),
        "todo" => Ok(Some(Component::Todo)),
        other => Err(napi::Error::from_reason(format!(
            "loom_cal: unknown component filter {other:?}"
        ))),
    }
}

/// Parse a `YYYYMMDDTHHMMSS` (15-char, `T` at index 8) wall-clock string into a [`DateTime`] for a range
/// window bound. Any other shape is `INVALID_ARGUMENT`.
fn parse_window_bound(s: &str, what: &str) -> napi::Result<DateTime> {
    let bytes = s.as_bytes();
    let bad = || {
        napi::Error::from_reason(format!(
            "loom_cal: {what} must be YYYYMMDDTHHMMSS, got {s:?}"
        ))
    };
    if bytes.len() != 15 || bytes[8] != b'T' {
        return Err(bad());
    }
    let digits = |range: std::ops::Range<usize>| -> napi::Result<&str> {
        let part = &s[range];
        if part.bytes().all(|b| b.is_ascii_digit()) {
            Ok(part)
        } else {
            Err(bad())
        }
    };
    let num = |part: &str| -> napi::Result<u32> { part.parse::<u32>().map_err(|_| bad()) };
    let year = num(digits(0..4)?)?;
    let month = num(digits(4..6)?)?;
    let day = num(digits(6..8)?)?;
    let hour = num(digits(9..11)?)?;
    let minute = num(digits(11..13)?)?;
    let second = num(digits(13..15)?)?;
    let month = IcalMonth::try_from(u8::try_from(month).map_err(|_| bad())?).map_err(|_| bad())?;
    let date = IcalDate::from_calendar_date(
        i32::try_from(year).map_err(|_| bad())?,
        month,
        u8::try_from(day).map_err(|_| bad())?,
    )
    .map_err(|_| bad())?;
    let time = IcalTime::from_hms(
        u8::try_from(hour).map_err(|_| bad())?,
        u8::try_from(minute).map_err(|_| bad())?,
        u8::try_from(second).map_err(|_| bad())?,
    )
    .map_err(|_| bad())?;
    Ok(DateTime::new(date, time))
}

/// Render a wall-clock [`DateTime`] as the `YYYYMMDDTHHMMSS` form used in the `calRange` wire array.
fn format_window_bound(dt: &DateTime) -> String {
    let d = dt.date();
    let t = dt.time();
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}",
        d.year(),
        u8::from(d.month()),
        d.day(),
        t.hour(),
        t.minute(),
        t.second(),
    )
}

/// Encode a canonical-CBOR array of per-record `encode()` byte strings (the list/search wire form).
fn records_cbor(records: Vec<Vec<u8>>) -> napi::Result<Vec<u8>> {
    let items = records.into_iter().map(CborValue::Bytes).collect();
    cbor_encode(&CborValue::Array(items))
        .map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))
}

/// Encode a canonical-CBOR array of text strings (the `listCollections`/`listBooks`/`listMailboxes`/
/// `getFlags` wire form).
fn strings_cbor(strings: Vec<String>) -> napi::Result<Vec<u8>> {
    let items = strings.into_iter().map(CborValue::Text).collect();
    cbor_encode(&CborValue::Array(items))
        .map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))
}

/// Decode a canonical-CBOR `Array(Text)` flag-set buffer into the owned strings `setFlags` expects.
fn flags_from_cbor(bytes: &[u8]) -> napi::Result<Vec<String>> {
    let value =
        loom_codec::decode(bytes).map_err(|e| napi::Error::from_reason(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(napi::Error::from_reason(
            "loom_mail: flags must be a CBOR array".to_string(),
        ));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match item {
            CborValue::Text(s) => out.push(s),
            _ => {
                return Err(napi::Error::from_reason(
                    "loom_mail: flag must be CBOR text".to_string(),
                ));
            }
        }
    }
    Ok(out)
}

/// Resolve a workspace for a queue write by UUID or name, ensuring the `queue` facet exists. A name not
/// yet present is created carrying the `queue` facet; an unknown UUID is `NOT_FOUND`.
fn ensure_queue_ns(loom: &mut Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Typed {
            ty: FacetKind::Queue,
            name: workspace.to_string(),
        },
    };
    let ns = loom
        .registry_mut()
        .ensure_for_write(&selector, random_workspace_id()?)
        .map_err(reason)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Queue)
        .map_err(reason)?;
    Ok(ns)
}

/// Reject empty stream names and path-traversal forms so the public queue API never writes an arbitrary
/// path under the queue facet.
fn validate_stream_name(name: &str) -> napi::Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(napi::Error::from_reason(format!(
            "invalid stream name {name:?}"
        )));
    }
    Ok(())
}

/// Convert a JS `bigint` to a `usize`, rejecting negative, lossy, or out-of-range values.
fn bigint_to_usize(value: BigInt, what: &str) -> napi::Result<usize> {
    let (negative, magnitude, lossless) = value.get_u64();
    if negative || !lossless {
        return Err(napi::Error::from_reason(format!(
            "{what} must be a non-negative u64"
        )));
    }
    usize::try_from(magnitude).map_err(|_| napi::Error::from_reason(format!("{what} out of range")))
}

/// Convert a JS `bigint` to a `u64`, rejecting negative or lossy values.
fn bigint_to_u64(value: BigInt, what: &str) -> napi::Result<u64> {
    let (negative, magnitude, lossless) = value.get_u64();
    if negative || !lossless {
        return Err(napi::Error::from_reason(format!(
            "{what} must be a non-negative u64"
        )));
    }
    Ok(magnitude)
}

/// Resolve a workspace by facet and name for direct readers. The facet string is `"sql"`, `"files"`,
/// and so on; an unknown facet or workspace throws.
fn resolve_typed_ns(loom: &Loom<FileStore>, facet: &str, name: &str) -> napi::Result<WorkspaceId> {
    let ty = FacetKind::parse(facet).map_err(reason)?;
    loom.registry()
        .open(&WsSelector::Typed {
            ty,
            name: name.to_string(),
        })
        .map_err(reason)
}

/// Parse the conflict-resolution selector accepted by `mergeResolve`.
fn parse_conflict_resolution(s: &str) -> napi::Result<ConflictResolution> {
    match s {
        "ours" => Ok(ConflictResolution::Ours),
        "theirs" => Ok(ConflictResolution::Theirs),
        "working" => Ok(ConflictResolution::Working),
        other => Err(napi::Error::from_reason(format!(
            "unknown conflict resolution {other:?} (expected \"ours\", \"theirs\", or \"working\")"
        ))),
    }
}

/// Render a `Status` to the stable JSON shape (`{ staged, unstaged, untracked, conflicts }`, where
/// staged/unstaged are arrays of `{ "path", "kind" }`).
fn status_to_json(st: &loom_core::Status) -> String {
    fn kind(k: loom_core::ChangeKind) -> &'static str {
        match k {
            loom_core::ChangeKind::Added => "added",
            loom_core::ChangeKind::Modified => "modified",
            loom_core::ChangeKind::Deleted => "deleted",
        }
    }
    fn esc(s: &str) -> String {
        let mut o = String::with_capacity(s.len() + 2);
        o.push('"');
        for ch in s.chars() {
            match ch {
                '"' => o.push_str("\\\""),
                '\\' => o.push_str("\\\\"),
                '\n' => o.push_str("\\n"),
                '\r' => o.push_str("\\r"),
                '\t' => o.push_str("\\t"),
                c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
                c => o.push(c),
            }
        }
        o.push('"');
        o
    }
    let changes = |cs: &[loom_core::Change]| -> String {
        let mut s = String::from("[");
        for (i, c) in cs.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"path\":");
            s.push_str(&esc(&c.path));
            s.push_str(",\"kind\":");
            s.push_str(&esc(kind(c.kind)));
            s.push('}');
        }
        s.push(']');
        s
    };
    let strings = |xs: &[String]| -> String {
        let mut s = String::from("[");
        for (i, x) in xs.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&esc(x));
        }
        s.push(']');
        s
    };
    format!(
        "{{\"staged\":{},\"unstaged\":{},\"untracked\":{},\"conflicts\":{}}}",
        changes(&st.staged),
        changes(&st.unstaged),
        strings(&st.untracked),
        strings(&st.conflicts)
    )
}

/// Parse a list of `algo:hex` commit digests.
fn parse_commits(commits: &[String]) -> napi::Result<Vec<Digest>> {
    commits
        .iter()
        .map(|s| Digest::parse(s).map_err(reason))
        .collect()
}

/// Render a replay outcome as JSON: `{"outcome":...}` with `tip` for replayed and `paths` for conflicts.
fn replay_json(outcome: loom_core::ReplayOutcome) -> String {
    match outcome {
        loom_core::ReplayOutcome::Replayed(d) => {
            format!("{{\"outcome\":\"replayed\",\"tip\":{:?}}}", d.to_string())
        }
        loom_core::ReplayOutcome::Clean => "{\"outcome\":\"clean\"}".to_string(),
        loom_core::ReplayOutcome::Empty => "{\"outcome\":\"empty\"}".to_string(),
        loom_core::ReplayOutcome::Conflicts(paths) => {
            let items: Vec<String> = paths.iter().map(|p| format!("{p:?}")).collect();
            format!(
                "{{\"outcome\":\"conflicts\",\"paths\":[{}]}}",
                items.join(",")
            )
        }
    }
}

/// The live size and mode of an open file handle.
#[napi(object)]
pub struct FileStatJs {
    /// Live byte length.
    pub size: i64,
    /// POSIX-style mode bits.
    pub mode: u32,
}

/// Map an open-mode name to [`loom_core::OpenMode`].
fn parse_open_mode(mode: &str) -> napi::Result<loom_core::OpenMode> {
    Ok(match mode {
        "read" => loom_core::OpenMode::Read,
        "write" => loom_core::OpenMode::Write,
        "read_write" | "readWrite" | "readwrite" => loom_core::OpenMode::ReadWrite,
        "append" => loom_core::OpenMode::Append,
        other => {
            return Err(napi::Error::from_reason(format!(
                "unknown open mode '{other}' (use read|write|read_write|append)"
            )));
        }
    })
}

/// Reject a negative offset/length/size coming from JS, then widen to `u64`.
fn as_u64(value: i64, what: &str) -> napi::Result<u64> {
    u64::try_from(value)
        .map_err(|_| napi::Error::from_reason(format!("{what} must be non-negative")))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A deterministic workspace id from a workspace name for SQL sessions, matching the `loom` CLI and the C ABI so
/// the same name resolves to the same workspace everywhere.
fn derive_sql_ns_id(name: &str) -> WorkspaceId {
    let d = Digest::blake3(format!("{}:{name}", FacetKind::Sql.as_str()).as_bytes());
    let mut id = [0u8; 16];
    id.copy_from_slice(&d.bytes()[..16]);
    WorkspaceId::from_bytes(id)
}

/// An open SQL session over a workspace SQL facet in a `.loom`: run arbitrary SQL (results return as a JSON
/// array of result payloads) and commit the staged result. The whole versioned tabular + SQL stack
/// exposed to JavaScript; mirrors the C-ABI SQL session. In JS this is the `LoomSql` class.
///
/// It is a reopenable handle, not a held lock: each `exec` / `commit` opens the `.loom` for the
/// duration of that call and releases it (matching the engine's single-writer / lock-free-reader model
/// and the `loom` CLI). So multiple `LoomSql` instances over the same file coexist and never deadlock.
#[napi]
pub struct LoomSql {
    path: String,
    ns_name: String,
    db: String,
    /// Unlock passphrase for an encrypted loom, or `None`. Held for the session's
    /// lifetime because each op reopens the loom; the host supplies it (no env var).
    key: Option<String>,
    auth: LocalOpenAuth,
}

impl LoomSql {
    /// Open the loom for write and resolve-or-create the session's workspace SQL facet. The caller drops
    /// the returned `Loom` to release the exclusive write lock.
    fn open_for_write(&self) -> napi::Result<(Loom<FileStore>, WorkspaceId)> {
        let mut loom = attach_local_auth(
            open_loom_unlocked(&self.path, key_spec(self.key.as_deref()).as_ref())
                .map_err(reason)?,
            &self.auth,
        )
        .map_err(reason)?;
        let id = derive_sql_ns_id(&self.ns_name);
        let ns = loom
            .registry_mut()
            .ensure_for_write(
                &WsSelector::Typed {
                    ty: FacetKind::Sql,
                    name: self.ns_name.clone(),
                },
                id,
            )
            .map_err(reason)?;
        Ok((loom, ns))
    }
}

#[napi]
impl LoomSql {
    /// Open `loomPath` and start a SQL session over workspace `nsName`'s SQL facet (created if absent),
    /// database `db`.
    #[napi(constructor)]
    pub fn new(loom_path: String, ns_name: String, db: String) -> napi::Result<Self> {
        Self::open_inner(loom_path, ns_name, db, None, LocalOpenAuth::default())
    }

    /// Open an **encrypted** loom: same as the constructor but unlocks with
    /// `passphrase`, held for the session's lifetime. In JS: `LoomSql.openEncrypted(path, ns, db, pass)`.
    /// A wrong passphrase throws `E2E_KEY_INVALID`; opening an encrypted loom without one throws
    /// `E2E_LOCKED`. The host acquires the passphrase securely; no environment variable is consulted.
    #[napi(factory)]
    pub fn open_encrypted(
        loom_path: String,
        ns_name: String,
        db: String,
        passphrase: String,
    ) -> napi::Result<Self> {
        Self::open_inner(
            loom_path,
            ns_name,
            db,
            Some(passphrase),
            LocalOpenAuth::default(),
        )
    }

    #[napi(factory)]
    pub fn authenticated(
        loom_path: String,
        ns_name: String,
        db: String,
        auth_principal: String,
        auth_passphrase: String,
    ) -> napi::Result<Self> {
        let auth = local_auth_sql(Some(auth_principal), Some(auth_passphrase))?;
        Self::open_inner(loom_path, ns_name, db, None, auth)
    }

    #[napi(factory)]
    pub fn open_encrypted_authenticated(
        loom_path: String,
        ns_name: String,
        db: String,
        passphrase: String,
        auth_principal: String,
        auth_passphrase: String,
    ) -> napi::Result<Self> {
        let auth = local_auth_sql(Some(auth_principal), Some(auth_passphrase))?;
        Self::open_inner(loom_path, ns_name, db, Some(passphrase), auth)
    }

    fn open_inner(
        loom_path: String,
        ns_name: String,
        db: String,
        key: Option<String>,
        auth: LocalOpenAuth,
    ) -> napi::Result<Self> {
        let session = Self {
            path: loom_path,
            ns_name,
            db,
            key,
            auth,
        };
        // Fail-fast and create the workspace eagerly, then release the lock immediately.
        let (mut loom, _ns) = session.open_for_write()?;
        save_loom(&mut loom).map_err(reason)?;
        Ok(session)
    }

    /// Run one or more `;`-separated SQL statements and return **typed** results: an array of statement
    /// results, each a `{ kind, ... }` object. A `select` carries `columns` and `rows` of idiomatic
    /// cells - `BigInt` for 64/128-bit integers, `number` for <=32-bit integers and floats, `Uint8Array`
    /// for bytes, `string` for text, and `{ mantissa: BigInt, scale: number }` for an exact decimal.
    /// Mutations are staged and persisted; call [`LoomSql::commit`] to record one. For the raw canonical
    /// bytes use [`LoomSql::exec_bytes`]; for the JSON debug form use [`LoomSql::exec_json`].
    #[napi]
    pub fn exec<'env>(&self, env: &'env Env, sql: String) -> napi::Result<Array<'env>> {
        let bytes = exec_to_bytes(
            &self.path,
            &self.ns_name,
            &self.db,
            &sql,
            self.key.as_deref(),
            &self.auth,
        )?;
        let payload = result_view::decode(&bytes).map_err(reason)?;
        statements_to_js(env, &payload)
    }

    /// Run SQL; returns a JSON array of the result payloads (debug/admin form, rendered from the
    /// canonical CBOR - not the type-faithful API; use [`LoomSql::exec`]).
    #[napi]
    pub fn exec_json(&self, sql: String) -> napi::Result<String> {
        let ns = derive_sql_ns_id(&self.ns_name);
        let mut store =
            load_store_write(&self.path, ns, &self.db, self.key.as_deref(), &self.auth)?;
        let json = store.exec_json(&sql).map_err(reason)?;
        if store.is_dirty() {
            let (mut loom, ns) = self.open_for_write()?;
            store.persist(&mut loom, ns, &self.db).map_err(reason)?;
            save_loom(&mut loom).map_err(reason)?;
        }
        Ok(json)
    }

    /// Run SQL; returns the result payloads as canonical CBOR bytes.
    #[napi]
    pub fn exec_bytes(&self, sql: String) -> napi::Result<Uint8Array> {
        Ok(exec_to_bytes(
            &self.path,
            &self.ns_name,
            &self.db,
            &sql,
            self.key.as_deref(),
            &self.auth,
        )?
        .into())
    }

    /// Run a `SELECT` and return its rows as typed cell arrays (the streaming form). A JS
    /// array is natively iterable, so `for (const row of db.query(sql))` walks the rows; each cell uses
    /// the same idiomatic mapping as `exec`. Statements that mutate state are rejected.
    #[napi]
    pub fn query<'env>(&self, env: &'env Env, sql: String) -> napi::Result<Array<'env>> {
        let rows = query_to_rows(
            &self.path,
            &self.ns_name,
            &self.db,
            &sql,
            self.key.as_deref(),
            &self.auth,
        )?;
        rows_to_js(env, &rows)
    }

    /// Run SQL asynchronously: the returned Promise resolves to the canonical-CBOR result bytes. The
    /// blocking work runs on the libuv thread pool, off the JS event loop.
    #[napi(ts_return_type = "Promise<Uint8Array>")]
    pub fn exec_async(&self, sql: String) -> AsyncTask<ExecTask> {
        AsyncTask::new(ExecTask {
            path: self.path.clone(),
            ns_name: self.ns_name.clone(),
            db: self.db.clone(),
            sql,
            key: self.key.clone(),
            auth: self.auth.clone(),
        })
    }

    /// Commit the staged database state onto the workspace's current branch; returns the new commit's
    /// content address (`"algo:hex"`).
    #[napi]
    pub fn commit(&self, message: String, author: String) -> napi::Result<String> {
        let (mut loom, ns) = self.open_for_write()?;
        let digest = loom
            .commit(ns, &author, &message, now_ms())
            .map_err(reason)?;
        save_loom(&mut loom).map_err(reason)?;
        Ok(digest.to_string())
    }
}

/// Map a loom error into a napi error carrying its message.
fn reason(e: loom_core::error::LoomError) -> napi::Error {
    napi::Error::from_reason(e.to_string())
}

fn exec_reason(e: loom_compute::ExecError) -> napi::Error {
    reason(loom_core::error::LoomError::new(e.code(), e.to_string()))
}

/// Execute a canonical `loom.exec.request.v1` request and return canonical `loom.exec.result.v1`.
#[napi]
pub fn exec_cbor(
    loom_path: String,
    request: Uint8Array,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let key = key_spec(passphrase.as_deref());
    let mut loom = open_loom_unlocked(&loom_path, key.as_ref()).map_err(reason)?;
    let bytes = loom_compute::execute_cbor(&mut loom, request.as_ref()).map_err(exec_reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}

fn resolve_workspace_arg(loom: &Loom<FileStore>, workspace: &str) -> napi::Result<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Name(workspace.to_string()),
    };
    loom.registry().open(&selector).map_err(reason)
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch < '\u{20}' => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn workspace_list_json_inner(loom: &Loom<FileStore>) -> String {
    let mut out = String::from("[");
    for (i, ns) in loom.registry().list(None).iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"id\":");
        out.push_str(&json_string(&ns.id.to_string()));
        out.push_str(",\"name\":");
        out.push_str(&json_string(&ns.name));
        out.push_str(",\"facets\":[");
        for (j, facet) in ns.facets.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            out.push_str(&json_string(facet.as_str()));
        }
        out.push_str("],\"head\":");
        match ns.head {
            Some(head) => out.push_str(&json_string(&head.to_string())),
            None => out.push_str("null"),
        }
        out.push('}');
    }
    out.push(']');
    out
}

/// Open the loom for write and resolve-or-create workspace `ns_name`'s SQL facet. Free-fn form (the async
/// task owns the session's fields rather than borrowing `&self`); mirrors `LoomSql::open_for_write`.
fn open_for_write_path(
    path: &str,
    ns_name: &str,
    key: Option<&str>,
    auth: &LocalOpenAuth,
) -> napi::Result<(Loom<FileStore>, WorkspaceId)> {
    let mut loom = attach_local_auth(
        open_loom_unlocked(path, key_spec(key).as_ref()).map_err(reason)?,
        auth,
    )
    .map_err(reason)?;
    let id = derive_sql_ns_id(ns_name);
    let ns = loom
        .registry_mut()
        .ensure_for_write(
            &WsSelector::Typed {
                ty: FacetKind::Sql,
                name: ns_name.to_string(),
            },
            id,
        )
        .map_err(reason)?;
    Ok((loom, ns))
}

/// Open the SQL store for database `db` over an owned, lock-free read snapshot of the loom at `path` -
/// the lazy base. The base owns its read view (distinct from the exclusive write loom
/// `persist` flushes into) and streams durable rows on demand; `open` yields an empty store when no
/// catalog is staged yet. Shared by the per-op session path and the held-open `LoomSqlBatch`.
fn load_store_read(
    path: &str,
    ns: WorkspaceId,
    db: &str,
    key: Option<&str>,
    auth: &LocalOpenAuth,
) -> napi::Result<LoomSqlStore> {
    let read = attach_local_auth(
        open_loom_read_unlocked(path, key_spec(key).as_ref()).map_err(reason)?,
        auth,
    )
    .map_err(reason)?;
    LoomSqlStore::open_read(read, ns, db).map_err(reason)
}

fn load_store_write(
    path: &str,
    ns: WorkspaceId,
    db: &str,
    key: Option<&str>,
    auth: &LocalOpenAuth,
) -> napi::Result<LoomSqlStore> {
    let read = attach_local_auth(
        open_loom_read_unlocked(path, key_spec(key).as_ref()).map_err(reason)?,
        auth,
    )
    .map_err(reason)?;
    LoomSqlStore::open_write(read, ns, db).map_err(reason)
}

/// Run SQL and return the result payloads as canonical CBOR bytes. Shared by the synchronous
/// `exec_bytes` and the async `ExecTask`.
fn exec_to_bytes(
    path: &str,
    ns_name: &str,
    db: &str,
    sql: &str,
    key: Option<&str>,
    auth: &LocalOpenAuth,
) -> napi::Result<Vec<u8>> {
    let ns = derive_sql_ns_id(ns_name);
    let mut store = load_store_write(path, ns, db, key, auth)?;
    let bytes = store.exec_cbor(sql).map_err(reason)?;
    if store.in_transaction() {
        return Err(napi::Error::from_reason(
            "BEGIN without a matching COMMIT/ROLLBACK in one exec: open a LoomSqlBatch to run a transaction across statements",
        ));
    }
    if store.is_dirty() {
        let (mut loom, ns) = open_for_write_path(path, ns_name, key, auth)?;
        store.persist(&mut loom, ns, db).map_err(reason)?;
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(bytes)
}

/// Run a `SELECT` and return its first result's rows as decoded tabular values. Backs `query`.
fn query_to_rows(
    path: &str,
    ns_name: &str,
    db: &str,
    sql: &str,
    key: Option<&str>,
    auth: &LocalOpenAuth,
) -> napi::Result<Vec<Vec<Value>>> {
    let ns = derive_sql_ns_id(ns_name);
    let mut store = load_store_read(path, ns, db, key, auth)?;
    let rows = store.select_rows(sql).map_err(reason)?;
    if store.in_transaction() {
        return Err(napi::Error::from_reason(
            "BEGIN without a matching COMMIT/ROLLBACK in one query: open a LoomSqlBatch to run a transaction across statements",
        ));
    }
    if store.is_dirty() {
        return Err(napi::Error::from_reason(
            "sql query is read-only; use exec for statements that mutate state",
        ));
    }
    Ok(rows)
}

/// The held-open state of a [`LoomSqlBatch`]: the loom (whose lifetime holds the exclusive write lock),
/// the resolved workspace, the database name, and the SQL store loaded once for the batch.
struct BatchInner {
    loom: Loom<FileStore>,
    ns: WorkspaceId,
    db: String,
    path: String,
    store: LoomSqlStore,
    /// Unlock passphrase for an encrypted loom, or `None`. The held write `loom` is
    /// already unlocked; kept so `abort` can re-snapshot a lock-free read view.
    key: Option<String>,
    auth: LocalOpenAuth,
}

/// An explicit transaction/batch scope: holds the `.loom` (and its write lock) open across statements,
/// so an SQL transaction (`BEGIN`/`COMMIT`/`ROLLBACK`) can span `exec` calls, and changes are made
/// durable by one atomic save at `commit`. Because JS GC is non-deterministic, call `close()` to release
/// the lock promptly (closing without a commit discards un-persisted changes). The SQL `COMMIT` is
/// distinct from the VCS `commitVcs`.
#[napi]
pub struct LoomSqlBatch {
    inner: Option<BatchInner>,
}

#[napi]
impl LoomSqlBatch {
    /// Begin a batch over workspace `nsName`'s SQL facet (created if absent), database `db`, in `loomPath`.
    /// Holds the write lock until `close()`.
    #[napi(constructor)]
    pub fn new(loom_path: String, ns_name: String, db: String) -> napi::Result<Self> {
        Self::begin_inner(loom_path, ns_name, db, None, LocalOpenAuth::default())
    }

    /// Begin a batch over an **encrypted** loom, unlocking with `passphrase` for the
    /// batch's lifetime. In JS: `LoomSqlBatch.openEncrypted(path, ns, db, pass)`.
    #[napi(factory)]
    pub fn open_encrypted(
        loom_path: String,
        ns_name: String,
        db: String,
        passphrase: String,
    ) -> napi::Result<Self> {
        Self::begin_inner(
            loom_path,
            ns_name,
            db,
            Some(passphrase),
            LocalOpenAuth::default(),
        )
    }

    #[napi(factory)]
    pub fn authenticated(
        loom_path: String,
        ns_name: String,
        db: String,
        auth_principal: String,
        auth_passphrase: String,
    ) -> napi::Result<Self> {
        let auth = local_auth_sql(Some(auth_principal), Some(auth_passphrase))?;
        Self::begin_inner(loom_path, ns_name, db, None, auth)
    }

    #[napi(factory)]
    pub fn open_encrypted_authenticated(
        loom_path: String,
        ns_name: String,
        db: String,
        passphrase: String,
        auth_principal: String,
        auth_passphrase: String,
    ) -> napi::Result<Self> {
        let auth = local_auth_sql(Some(auth_principal), Some(auth_passphrase))?;
        Self::begin_inner(loom_path, ns_name, db, Some(passphrase), auth)
    }

    fn begin_inner(
        loom_path: String,
        ns_name: String,
        db: String,
        key: Option<String>,
        auth: LocalOpenAuth,
    ) -> napi::Result<Self> {
        let (loom, ns) = open_for_write_path(&loom_path, &ns_name, key.as_deref(), &auth)?;
        let store = load_store_write(&loom_path, ns, &db, key.as_deref(), &auth)?;
        Ok(Self {
            inner: Some(BatchInner {
                loom,
                ns,
                db,
                path: loom_path,
                store,
                key,
                auth,
            }),
        })
    }

    fn inner_mut(&mut self) -> napi::Result<&mut BatchInner> {
        self.inner
            .as_mut()
            .ok_or_else(|| napi::Error::from_reason("batch is closed"))
    }

    /// Run SQL in the batch and return typed results (same shape as `LoomSql.exec`). Includes
    /// `BEGIN`/`COMMIT`/`ROLLBACK`; changes accumulate until `commit`.
    #[napi]
    pub fn exec<'env>(&mut self, env: &'env Env, sql: String) -> napi::Result<Array<'env>> {
        let bytes = self.inner_mut()?.store.exec_cbor(&sql).map_err(reason)?;
        let payload = result_view::decode(&bytes).map_err(reason)?;
        statements_to_js(env, &payload)
    }

    /// Run SQL in the batch; returns the result payloads as canonical CBOR bytes.
    #[napi]
    pub fn exec_bytes(&mut self, sql: String) -> napi::Result<Uint8Array> {
        Ok(self
            .inner_mut()?
            .store
            .exec_cbor(&sql)
            .map_err(reason)?
            .into())
    }

    /// Run SQL in the batch; returns the JSON debug form.
    #[napi]
    pub fn exec_json(&mut self, sql: String) -> napi::Result<String> {
        self.inner_mut()?.store.exec_json(&sql).map_err(reason)
    }

    /// Make the batch's changes durable with one atomic save (no history entry). Rejected while an SQL
    /// transaction is open. The batch stays open.
    #[napi]
    pub fn commit(&mut self) -> napi::Result<()> {
        let b = self.inner_mut()?;
        if b.store.in_transaction() {
            return Err(napi::Error::from_reason(
                "the batch has an open SQL transaction; COMMIT or ROLLBACK first",
            ));
        }
        b.store.persist(&mut b.loom, b.ns, &b.db).map_err(reason)?;
        save_loom(&mut b.loom).map_err(reason)?;
        Ok(())
    }

    /// Like `commit`, but also records a VCS commit; returns the commit's content address. Distinct from
    /// a SQL `COMMIT`. Rejected while an SQL transaction is open.
    #[napi]
    pub fn commit_vcs(&mut self, message: String, author: String) -> napi::Result<String> {
        let b = self.inner_mut()?;
        if b.store.in_transaction() {
            return Err(napi::Error::from_reason(
                "the batch has an open SQL transaction; COMMIT or ROLLBACK first",
            ));
        }
        b.store.persist(&mut b.loom, b.ns, &b.db).map_err(reason)?;
        let digest = b
            .loom
            .commit(b.ns, &author, &message, now_ms())
            .map_err(reason)?;
        save_loom(&mut b.loom).map_err(reason)?;
        Ok(digest.to_string())
    }

    /// Discard un-persisted in-memory changes (and any open SQL transaction), reloading from the last
    /// durable state. The batch stays open.
    #[napi]
    pub fn abort(&mut self) -> napi::Result<()> {
        let b = self.inner_mut()?;
        b.store = load_store_write(&b.path, b.ns, &b.db, b.key.as_deref(), &b.auth)?;
        Ok(())
    }

    /// Release the write lock and free the batch. Closing without a commit discards un-persisted changes.
    #[napi]
    pub fn close(&mut self) {
        self.inner = None;
    }
}

/// A napi async task: runs a blocking SQL exec on the libuv thread pool (off the JS event loop) and
/// resolves the JS Promise with the canonical-CBOR result bytes.
pub struct ExecTask {
    path: String,
    ns_name: String,
    db: String,
    sql: String,
    key: Option<String>,
    auth: LocalOpenAuth,
}

impl Task for ExecTask {
    type Output = Vec<u8>;
    type JsValue = Uint8Array;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        exec_to_bytes(
            &self.path,
            &self.ns_name,
            &self.db,
            &self.sql,
            self.key.as_deref(),
            &self.auth,
        )
    }

    fn resolve(&mut self, _env: Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        Ok(output.into())
    }
}

// --- Typed result mapping: a decoded `ResultPayload` -> idiomatic JS (the primary `exec` API). ---

/// Build the typed JS result array from a decoded payload. SQL `exec` yields statements.
fn statements_to_js<'env>(env: &'env Env, payload: &ResultPayload) -> napi::Result<Array<'env>> {
    let stmts = match payload {
        ResultPayload::Statements(s) => s,
        ResultPayload::Reader(_) => {
            return Err(napi::Error::from_reason("exec returned a reader result"));
        }
    };
    let mut arr = env.create_array(stmts.len() as u32)?;
    for (i, s) in stmts.iter().enumerate() {
        arr.set(i as u32, statement_to_js(env, s)?)?;
    }
    Ok(arr)
}

/// One SQL statement result as a `{ kind, ... }` JS object.
fn statement_to_js<'env>(env: &'env Env, s: &Statement) -> napi::Result<JsObject<'env>> {
    let mut o = JsObject::new(env)?;
    match s {
        Statement::Select { labels, rows } => {
            o.set("kind", "select")?;
            let mut cols = env.create_array(labels.len() as u32)?;
            for (i, name) in labels.iter().enumerate() {
                let mut c = JsObject::new(env)?;
                c.set("name", name.clone())?;
                cols.set(i as u32, c)?;
            }
            o.set("columns", cols)?;
            o.set("rows", rows_to_js(env, rows)?)?;
        }
        Statement::SelectMap(rows) => {
            o.set("kind", "selectMap")?;
            let mut rs = env.create_array(rows.len() as u32)?;
            for (i, m) in rows.iter().enumerate() {
                rs.set(i as u32, map_entries(env, m)?)?;
            }
            o.set("rows", rs)?;
        }
        Statement::ShowColumns(cols) => {
            o.set("kind", "showColumns")?;
            let mut cs = env.create_array(cols.len() as u32)?;
            for (i, c) in cols.iter().enumerate() {
                let mut co = JsObject::new(env)?;
                co.set("name", c.name.clone())?;
                co.set("type", c.type_name.clone())?;
                cs.set(i as u32, co)?;
            }
            o.set("columns", cs)?;
        }
        Statement::Insert(n) => count_obj(&mut o, "insert", *n)?,
        Statement::Delete(n) => count_obj(&mut o, "delete", *n)?,
        Statement::Update(n) => count_obj(&mut o, "update", *n)?,
        Statement::DropTable(n) => count_obj(&mut o, "dropTable", *n)?,
        Statement::Create => o.set("kind", "create")?,
        Statement::DropFunction => o.set("kind", "dropFunction")?,
        Statement::AlterTable => o.set("kind", "alterTable")?,
        Statement::CreateIndex => o.set("kind", "createIndex")?,
        Statement::DropIndex => o.set("kind", "dropIndex")?,
        Statement::StartTransaction => o.set("kind", "startTransaction")?,
        Statement::Commit => o.set("kind", "commit")?,
        Statement::Rollback => o.set("kind", "rollback")?,
        Statement::ShowVariable(sv) => {
            o.set("kind", "showVariable")?;
            match sv {
                ShowVariable::Tables(v) => {
                    o.set("variable", "tables")?;
                    o.set("values", string_array(env, v)?)?;
                }
                ShowVariable::Functions(v) => {
                    o.set("variable", "functions")?;
                    o.set("values", string_array(env, v)?)?;
                }
                ShowVariable::Version(s) => {
                    o.set("variable", "version")?;
                    o.set("value", s.clone())?;
                }
            }
        }
    }
    Ok(o)
}

fn count_obj(o: &mut JsObject, kind: &str, n: u64) -> napi::Result<()> {
    o.set("kind", kind)?;
    o.set("count", n as f64)?;
    Ok(())
}

fn string_array<'env>(env: &'env Env, items: &[String]) -> napi::Result<Array<'env>> {
    let mut arr = env.create_array(items.len() as u32)?;
    for (i, s) in items.iter().enumerate() {
        arr.set(i as u32, s.clone())?;
    }
    Ok(arr)
}

fn rows_to_js<'env>(env: &'env Env, rows: &[Vec<Value>]) -> napi::Result<Array<'env>> {
    let mut rs = env.create_array(rows.len() as u32)?;
    for (i, r) in rows.iter().enumerate() {
        rs.set(i as u32, cells_to_array(env, r)?)?;
    }
    Ok(rs)
}

fn cells_to_array<'env>(env: &'env Env, cells: &[Value]) -> napi::Result<Array<'env>> {
    let mut arr = env.create_array(cells.len() as u32)?;
    for (i, v) in cells.iter().enumerate() {
        set_cell(env, &mut arr, i as u32, v)?;
    }
    Ok(arr)
}

/// A map cell / SelectMap row as a JS array of `[key, value]` pairs (consumable by `new Map(...)`).
fn map_entries<'env>(env: &'env Env, m: &BTreeMap<String, Value>) -> napi::Result<Array<'env>> {
    let mut entries = env.create_array(m.len() as u32)?;
    for (j, (k, val)) in m.iter().enumerate() {
        let mut pair = env.create_array(2)?;
        pair.set(0, k.clone())?;
        set_cell(env, &mut pair, 1, val)?;
        entries.set(j as u32, pair)?;
    }
    Ok(entries)
}

/// Set array index `i` to the idiomatic JS value for one decoded cell.
fn set_cell<'env>(env: &'env Env, arr: &mut Array<'env>, i: u32, v: &Value) -> napi::Result<()> {
    match v {
        Value::Null => arr.set(i, Null)?,
        Value::Bool(b) => arr.set(i, *b)?,
        Value::I8(x) => arr.set(i, i32::from(*x))?,
        Value::I16(x) => arr.set(i, i32::from(*x))?,
        Value::I32(x) => arr.set(i, *x)?,
        Value::U8(x) => arr.set(i, i32::from(*x))?,
        Value::U16(x) => arr.set(i, i32::from(*x))?,
        Value::U32(x) => arr.set(i, f64::from(*x))?,
        Value::Int(x) => arr.set(i, BigInt::from(*x))?,
        Value::U64(x) => arr.set(i, BigInt::from(*x))?,
        Value::I128(x) => arr.set(i, BigInt::from(*x))?,
        Value::U128(x) => arr.set(i, BigInt::from(*x))?,
        Value::Float(x) => arr.set(i, *x)?,
        Value::F32(x) => arr.set(i, f64::from(*x))?,
        Value::Text(s) => arr.set(i, s.clone())?,
        Value::Bytes(b) => arr.set(i, Uint8Array::from(b.clone()))?,
        Value::Decimal { mantissa, scale } => {
            let mut o = JsObject::new(env)?;
            o.set("mantissa", BigInt::from(*mantissa))?;
            o.set("scale", f64::from(*scale))?;
            arr.set(i, o)?;
        }
        Value::Date(d) => arr.set(i, *d)?,
        Value::Time(t) => arr.set(i, BigInt::from(*t))?,
        Value::Timestamp(t) => arr.set(i, BigInt::from(*t))?,
        Value::Interval { months, micros } => {
            let mut o = JsObject::new(env)?;
            o.set("months", *months)?;
            o.set("micros", BigInt::from(*micros))?;
            arr.set(i, o)?;
        }
        Value::Uuid(u) => arr.set(i, format!("{u:032x}"))?,
        Value::Inet(ip) => arr.set(i, ip.to_string())?,
        Value::Point { x, y } => {
            let mut o = JsObject::new(env)?;
            o.set("x", *x)?;
            o.set("y", *y)?;
            arr.set(i, o)?;
        }
        Value::List(items) => arr.set(i, cells_to_array(env, items)?)?,
        Value::Map(m) => arr.set(i, map_entries(env, m)?)?,
    }
    Ok(())
}

#[cfg(test)]
mod locator_tests {
    use super::normalize_locator;

    #[test]
    fn local_paths_pass_through() {
        assert_eq!(normalize_locator("./app.loom").unwrap(), "./app.loom");
        assert_eq!(normalize_locator("/abs/app.loom").unwrap(), "/abs/app.loom");
        // A bare non-path string stays local (no alias TOML is read in this binding).
        assert_eq!(normalize_locator("prod").unwrap(), "prod");
    }

    #[test]
    fn file_url_is_stripped_to_local_path() {
        assert_eq!(
            normalize_locator("file:///abs/app.loom").unwrap(),
            "/abs/app.loom"
        );
    }

    #[test]
    fn remote_url_is_rejected_without_remote_feature() {
        let err = normalize_locator("https://loom.example.com/prod").unwrap_err();
        // Default build has no `remote` feature: the precise feature-gated message.
        assert!(
            err.to_string().contains("remote feature"),
            "unexpected error: {err}"
        );
        assert!(normalize_locator("http://loom.example.com/prod").is_err());
    }
}
