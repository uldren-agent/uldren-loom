//! Python binding for Uldren Loom via PyO3. Published as `uldrenai-loom`.
//!
//! The native extension module backing the `uldrenai_loom` package; the pure-Python wrapper in
//! `python/uldrenai_loom/__init__.py` re-exports these functions.
//! Licensed under BUSL-1.1 (see the repo `LICENSE`). (c) Uldren Technologies LLC.
//!
//! These functions are thin PyO3 shims over the C-ABI surface: each carries the mandatory `py: Python`
//! token plus the same path/facet/workspace/target/options argument list as its C ABI counterpart, so a
//! one-to-one mapping routinely needs eight arguments. That is deliberate, so `too_many_arguments` is
//! allowed crate-wide rather than splitting the shims into structs that would diverge from the C ABI.
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
use pyo3::IntoPyObjectExt;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};

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
/// through. The host (Python) supplies the passphrase; no environment variable is read.
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
    auth_principal: Option<&str>,
    auth_passphrase: Option<&str>,
) -> PyResult<LocalOpenAuth> {
    match (auth_principal, auth_passphrase) {
        (Some(principal), Some(passphrase)) => Ok(LocalOpenAuth {
            principal: Some(WorkspaceId::parse(principal).map_err(py_err)?),
            passphrase: Some(passphrase.to_string()),
            ..Default::default()
        }),
        (None, None) => Ok(LocalOpenAuth::default()),
        _ => Err(PyRuntimeError::new_err(
            "auth_principal and auth_passphrase must be supplied together",
        )),
    }
}

/// Parse the identity-profile selector.
fn parse_profile(s: &str) -> PyResult<Algo> {
    match s {
        "default" | "blake3" => Ok(Algo::Blake3),
        "fips" | "sha256" => Ok(Algo::Sha256),
        other => Err(PyRuntimeError::new_err(format!(
            "unknown identity profile {other:?} (expected `default`/`blake3` or `fips`/`sha256`)"
        ))),
    }
}

fn rng_fill(buf: &mut [u8]) -> PyResult<()> {
    getrandom::getrandom(buf).map_err(|e| PyRuntimeError::new_err(format!("rng: {e}")))
}

fn random_workspace_id() -> PyResult<WorkspaceId> {
    let mut id = [0u8; 16];
    rng_fill(&mut id)?;
    Ok(WorkspaceId::v4_from_bytes(id))
}

/// Open the file store and unlock it with `passphrase` so its DEK is available for a wrap update.
fn open_store_for_key_update(path: &str, passphrase: &str) -> PyResult<FileStore> {
    let fs = FileStore::open(path).map_err(py_err)?;
    fs.unlock(&KeySpec::passphrase(passphrase))
        .map_err(py_err)?;
    fs.validate_runtime_policy().map_err(py_err)?;
    Ok(fs)
}

/// Build a raw-KEK credential from caller-supplied bytes, which must be exactly 32 (a 256-bit key).
fn kek_spec(kek: &[u8]) -> PyResult<KeySpec> {
    let bytes: [u8; loom_core::keys::KEY_LEN] = kek.try_into().map_err(|_| {
        PyRuntimeError::new_err(format!(
            "a raw KEK must be exactly {} bytes (256 bits), got {}",
            loom_core::keys::KEY_LEN,
            kek.len()
        ))
    })?;
    Ok(KeySpec::raw_kek(bytes))
}

/// Fresh per-wrap salt (16 bytes) and AEAD nonce (24 bytes).
fn fresh_wrap_material() -> PyResult<([u8; 16], [u8; 24])> {
    let mut salt = [0u8; 16];
    let mut wrap_nonce = [0u8; 24];
    rng_fill(&mut salt)?;
    rng_fill(&mut wrap_nonce)?;
    Ok((salt, wrap_nonce))
}

/// Resolve a workspace for a CAS write by UUID or name, ensuring the `cas` facet exists. A name not yet
/// present is created carrying the `cas` facet; an unknown UUID is `NOT_FOUND`.
fn ensure_cas_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
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
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Cas)
        .map_err(py_err)?;
    Ok(ns)
}

/// Resolve a workspace for a kv write by UUID or name, ensuring the `kv` facet exists.
fn ensure_kv_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
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
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Kv)
        .map_err(py_err)?;
    Ok(ns)
}

fn ensure_lanes_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
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
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Document)
        .map_err(py_err)?;
    Ok(ns)
}

/// Resolve a workspace for a graph write by UUID or name, ensuring the `graph` facet exists.
fn ensure_graph_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
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
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Graph)
        .map_err(py_err)?;
    Ok(ns)
}

/// Resolve a workspace for a vector write by UUID or name, ensuring the `vector` facet exists.
fn ensure_vector_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
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
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Vector)
        .map_err(py_err)?;
    Ok(ns)
}

/// Resolve a workspace for a columnar write by UUID or name, ensuring the `columnar` facet exists.
fn ensure_columnar_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
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
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Columnar)
        .map_err(py_err)?;
    Ok(ns)
}

// ---------------------------------------------------------------------------------------------------
// Document / Time-series / Ledger facet wrappers, mirroring the kv pattern over loom-core.
// ---------------------------------------------------------------------------------------------------

fn ensure_doc_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
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
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Document)
        .map_err(py_err)?;
    Ok(ns)
}

fn ensure_ts_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
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
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::TimeSeries)
        .map_err(py_err)?;
    Ok(ns)
}

fn ensure_ledger_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
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
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Ledger)
        .map_err(py_err)?;
    Ok(ns)
}

// ---------------------------------------------------------------------------------------------------
// Calendar (Calendar facet, 0037) - collections under a principal, typed iCalendar records keyed by UID.
// ---------------------------------------------------------------------------------------------------

/// Resolve a workspace for a calendar write by UUID or name, ensuring the `calendar` facet exists.
fn ensure_cal_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
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
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Calendar)
        .map_err(py_err)?;
    Ok(ns)
}

/// Resolve a workspace for a contacts write by UUID or name, ensuring the `contacts` facet exists.
fn ensure_card_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
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
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Contacts)
        .map_err(py_err)?;
    Ok(ns)
}

/// Resolve a workspace for a mail write by UUID or name, ensuring the `mail` facet exists.
fn ensure_mail_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
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
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Mail)
        .map_err(py_err)?;
    Ok(ns)
}

/// Parse a comma-separated component list ("event,todo"; an empty string is the empty set) into the
/// `component_set` of a `CollectionMeta`. An unknown token raises.
fn parse_component_set(components: &str) -> PyResult<Vec<Component>> {
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
                return Err(PyRuntimeError::new_err(format!(
                    "loom_cal: unknown component {other:?}"
                )));
            }
        }
    }
    Ok(out)
}

/// Map a component-filter string to the calendar facet's optional component: "" -> `None`,
/// "event" -> `Some(Event)`, "todo" -> `Some(Todo)`. Any other token raises.
fn parse_component_filter(component: &str) -> PyResult<Option<Component>> {
    match component {
        "" => Ok(None),
        "event" => Ok(Some(Component::Event)),
        "todo" => Ok(Some(Component::Todo)),
        other => Err(PyRuntimeError::new_err(format!(
            "loom_cal: unknown component filter {other:?}"
        ))),
    }
}

/// Parse a `YYYYMMDDTHHMMSS` (14-digit, `T` at index 8) wall-clock string into a `DateTime` for a range
/// window bound. Any other shape raises.
fn parse_window_bound(s: &str, what: &str) -> PyResult<DateTime> {
    let bytes = s.as_bytes();
    let bad = || {
        PyRuntimeError::new_err(format!(
            "loom_cal: {what} must be YYYYMMDDTHHMMSS, got {s:?}"
        ))
    };
    if bytes.len() != 15 || bytes[8] != b'T' {
        return Err(bad());
    }
    let digits = |range: std::ops::Range<usize>| -> PyResult<&str> {
        let part = &s[range];
        if part.bytes().all(|b| b.is_ascii_digit()) {
            Ok(part)
        } else {
            Err(bad())
        }
    };
    let num = |part: &str| -> PyResult<u32> { part.parse::<u32>().map_err(|_| bad()) };
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

/// Render a wall-clock `DateTime` as the `YYYYMMDDTHHMMSS` form used in the calendar range wire array.
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
fn records_cbor(records: Vec<Vec<u8>>) -> PyResult<Vec<u8>> {
    let items = records.into_iter().map(CborValue::Bytes).collect();
    cbor_encode(&CborValue::Array(items)).map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))
}

/// Encode a canonical-CBOR array of text strings (the list-collections/books/mailboxes/get-flags form).
fn strings_cbor(strings: Vec<String>) -> PyResult<Vec<u8>> {
    let items = strings.into_iter().map(CborValue::Text).collect();
    cbor_encode(&CborValue::Array(items)).map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))
}

/// Decode a canonical-CBOR `Array(Text)` flag-set buffer into the owned strings `set_flags` expects.
fn flags_from_cbor(bytes: &[u8]) -> PyResult<Vec<String>> {
    let value =
        loom_codec::decode(bytes).map_err(|e| PyRuntimeError::new_err(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(PyRuntimeError::new_err(
            "loom_mail: flags must be a CBOR array",
        ));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match item {
            CborValue::Text(s) => out.push(s),
            _ => {
                return Err(PyRuntimeError::new_err("loom_mail: flag must be CBOR text"));
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------------
// Contacts (Contacts facet, 0038) - address books under a principal, typed vCard records keyed by UID.
// ---------------------------------------------------------------------------------------------------

// ---------------------------------------------------------------------------------------------------
// Mail (Mail facet, 0039) - mailboxes under a principal; immutable RFC 5322 bodies plus a structured
// index and mutable flags, keyed by UID.
// ---------------------------------------------------------------------------------------------------

/// Resolve a workspace for a queue write by UUID or name, ensuring the `queue` facet exists. A name not
/// yet present is created carrying the `queue` facet; an unknown UUID is `NOT_FOUND`.
fn ensure_queue_ns(loom: &mut Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
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
        .map_err(py_err)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Queue)
        .map_err(py_err)?;
    Ok(ns)
}

/// Reject empty stream names and path-traversal forms so the public queue API never writes an arbitrary
/// path under the queue facet.
fn validate_stream_name(name: &str) -> PyResult<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(PyRuntimeError::new_err(format!(
            "invalid stream name {name:?}"
        )));
    }
    Ok(())
}

/// Parse the conflict-resolution selector accepted by `merge_resolve`.
fn parse_conflict_resolution(s: &str) -> PyResult<ConflictResolution> {
    match s {
        "ours" => Ok(ConflictResolution::Ours),
        "theirs" => Ok(ConflictResolution::Theirs),
        "working" => Ok(ConflictResolution::Working),
        other => Err(PyRuntimeError::new_err(format!(
            "unknown conflict resolution {other:?} (expected \"ours\", \"theirs\", or \"working\")"
        ))),
    }
}

/// Render a `Status` to the stable JSON shape (`{ staged, unstaged, untracked, conflicts }`).
fn status_to_json(st: &loom_core::Status) -> String {
    fn kind(k: loom_core::ChangeKind) -> &'static str {
        match k {
            loom_core::ChangeKind::Added => "added",
            loom_core::ChangeKind::Modified => "modified",
            loom_core::ChangeKind::Deleted => "deleted",
        }
    }
    let changes = |cs: &[loom_core::Change]| -> String {
        let mut s = String::from("[");
        for (i, c) in cs.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str("{\"path\":");
            s.push_str(&json_string(&c.path));
            s.push_str(",\"kind\":");
            s.push_str(&json_string(kind(c.kind)));
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
            s.push_str(&json_string(x));
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
fn parse_commits(commits: &[String]) -> PyResult<Vec<Digest>> {
    commits
        .iter()
        .map(|s| Digest::parse(s).map_err(py_err))
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

/// Map an open-mode name to [`loom_core::OpenMode`].
fn parse_open_mode(mode: &str) -> PyResult<loom_core::OpenMode> {
    Ok(match mode {
        "read" => loom_core::OpenMode::Read,
        "write" => loom_core::OpenMode::Write,
        "read_write" | "readwrite" => loom_core::OpenMode::ReadWrite,
        "append" => loom_core::OpenMode::Append,
        other => {
            return Err(PyRuntimeError::new_err(format!(
                "unknown open mode '{other}' (use read|write|read_write|append)"
            )));
        }
    })
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

/// Map a loom error into a Python `RuntimeError` carrying its message.
fn py_err(e: loom_core::error::LoomError) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

fn exec_py_err(e: loom_compute::ExecError) -> PyErr {
    py_err(loom_core::error::LoomError::new(e.code(), e.to_string()))
}

/// Execute a canonical `loom.exec.request.v1` request and return canonical `loom.exec.result.v1`.
#[pyfunction]
fn exec_cbor<'py>(
    py: Python<'py>,
    path: &str,
    request: &[u8],
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let key = key_spec(passphrase);
    let bytes = py.allow_threads(|| -> PyResult<Vec<u8>> {
        let mut loom = open_loom_unlocked(path, key.as_ref()).map_err(py_err)?;
        let bytes = loom_compute::execute_cbor(&mut loom, request).map_err(exec_py_err)?;
        save_loom(&mut loom).map_err(py_err)?;
        Ok(bytes)
    })?;
    Ok(PyBytes::new(py, &bytes))
}

fn resolve_workspace_arg(loom: &Loom<FileStore>, workspace: &str) -> PyResult<WorkspaceId> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Name(workspace.to_string()),
    };
    loom.registry().open(&selector).map_err(py_err)
}

/// Resolve a workspace by facet and name for direct readers. The facet string is `"sql"`, `"files"`,
/// and so on; an unknown facet or workspace raises.
fn resolve_typed_ns(loom: &Loom<FileStore>, facet: &str, name: &str) -> PyResult<WorkspaceId> {
    let ty = FacetKind::parse(facet).map_err(py_err)?;
    loom.registry()
        .open(&WsSelector::Typed {
            ty,
            name: name.to_string(),
        })
        .map_err(py_err)
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

/// An open SQL session over a workspace SQL facet in a `.loom`: run arbitrary SQL (results return as a JSON
/// array of result payloads) and commit the staged result. Exposes the whole versioned tabular + SQL
/// stack to Python; mirrors the C-ABI SQL session.
///
/// It is a reopenable handle, not a held lock: each `exec` / `commit` opens the `.loom` for the
/// duration of that call and releases it (matching the engine's single-writer / lock-free-reader model
/// and the `loom` CLI). So multiple `LoomSql` instances over the same file coexist and never deadlock.
#[pyclass]
struct LoomSql {
    path: String,
    ns_name: String,
    db: String,
    /// Unlock passphrase for an encrypted loom, or `None`. Held for the session's
    /// lifetime because each op reopens the loom; the host supplies it (no env var).
    key: Option<String>,
    auth: LocalOpenAuth,
}

impl LoomSql {
    /// Open the loom for write and resolve-or-create the session workspace's SQL facet. The caller drops
    /// the returned `Loom` to release the exclusive write lock.
    fn open_for_write(&self) -> PyResult<(Loom<FileStore>, WorkspaceId)> {
        open_for_write_path(&self.path, &self.ns_name, self.key.as_deref(), &self.auth)
            .map_err(py_err)
    }

    /// Construct a session (holding the optional unlock `key`), eagerly creating the workspace. Not a
    /// `#[pymethods]` member, so it is not exposed to Python (the `new` / `open_encrypted` ctors call it).
    fn open_inner(
        loom_path: &str,
        ns_name: &str,
        db: &str,
        key: Option<String>,
        auth: LocalOpenAuth,
    ) -> PyResult<Self> {
        let session = Self {
            path: loom_path.to_string(),
            ns_name: ns_name.to_string(),
            db: db.to_string(),
            key,
            auth,
        };
        let (mut loom, _ns) = session.open_for_write()?;
        save_loom(&mut loom).map_err(py_err)?;
        Ok(session)
    }
}

type LoomResult<T> = Result<T, loom_core::error::LoomError>;

/// Open the loom for write (unlocking with `key` if encrypted) and resolve-or-create workspace
/// `ns_name`'s SQL facet. Free-fn form returning the engine error (so it can run inside `Python::allow_threads`,
/// where the GIL is released and `PyErr` must not be built); callers map to `PyErr` after re-acquiring.
fn open_for_write_path(
    path: &str,
    ns_name: &str,
    key: Option<&str>,
    auth: &LocalOpenAuth,
) -> LoomResult<(Loom<FileStore>, WorkspaceId)> {
    let mut loom = attach_local_auth(open_loom_unlocked(path, key_spec(key).as_ref())?, auth)?;
    let id = derive_sql_ns_id(ns_name);
    let ns = loom.registry_mut().ensure_for_write(
        &WsSelector::Typed {
            ty: FacetKind::Sql,
            name: ns_name.to_string(),
        },
        id,
    )?;
    Ok((loom, ns))
}

/// Open the SQL store for database `db` over an owned, lock-free read snapshot of the loom at `path` -
/// the lazy base. The base owns its read view (distinct from the exclusive write loom
/// that `persist` flushes into) and streams durable rows on demand; `open` yields an empty store when no
/// catalog is staged yet. Shared by the per-op session path and the held-open [`LoomSqlBatch`].
fn load_store_read(
    path: &str,
    ns: WorkspaceId,
    db: &str,
    key: Option<&str>,
    auth: &LocalOpenAuth,
) -> LoomResult<LoomSqlStore> {
    let read = attach_local_auth(open_loom_read_unlocked(path, key_spec(key).as_ref())?, auth)?;
    LoomSqlStore::open_read(read, ns, db)
}

fn load_store_write(
    path: &str,
    ns: WorkspaceId,
    db: &str,
    key: Option<&str>,
    auth: &LocalOpenAuth,
) -> LoomResult<LoomSqlStore> {
    let read = attach_local_auth(open_loom_read_unlocked(path, key_spec(key).as_ref())?, auth)?;
    LoomSqlStore::open_write(read, ns, db)
}

/// A per-op exec is one atomic save: a transaction must open and resolve within the single call. Reject
/// a transaction left open (its mid-flight state would be persisted and lost); use a [`LoomSqlBatch`].
fn reject_dangling_txn(store: &LoomSqlStore) -> LoomResult<()> {
    if store.in_transaction() {
        return Err(loom_core::error::LoomError::invalid(
            "BEGIN without a matching COMMIT/ROLLBACK in one exec: open a LoomSqlBatch to run a transaction across statements",
        ));
    }
    Ok(())
}

/// Load the database, run the statement(s), persist, and save; return the result payloads as canonical
/// CBOR bytes. Backs `exec_bytes`.
fn run_exec_cbor(
    path: &str,
    ns_name: &str,
    db: &str,
    sql: &str,
    key: Option<&str>,
    auth: &LocalOpenAuth,
) -> LoomResult<Vec<u8>> {
    let ns = derive_sql_ns_id(ns_name);
    let mut store = load_store_write(path, ns, db, key, auth)?;
    let bytes = store.exec_cbor(sql)?;
    reject_dangling_txn(&store)?;
    persist_if_dirty(path, ns_name, db, &mut store, key, auth)?;
    Ok(bytes)
}

/// JSON-string variant of [`run_exec_cbor`] (the debug result form).
fn run_exec_json(
    path: &str,
    ns_name: &str,
    db: &str,
    sql: &str,
    key: Option<&str>,
    auth: &LocalOpenAuth,
) -> LoomResult<String> {
    let ns = derive_sql_ns_id(ns_name);
    let mut store = load_store_write(path, ns, db, key, auth)?;
    let json = store.exec_json(sql)?;
    reject_dangling_txn(&store)?;
    persist_if_dirty(path, ns_name, db, &mut store, key, auth)?;
    Ok(json)
}

/// Run SQL and return the first `SELECT`'s rows as decoded tabular values - the form [`LoomRows`]
/// iterates, mapping each row's cells to native Python objects.
fn run_select_rows(
    path: &str,
    ns_name: &str,
    db: &str,
    sql: &str,
    key: Option<&str>,
    auth: &LocalOpenAuth,
) -> LoomResult<Vec<Vec<Value>>> {
    let ns = derive_sql_ns_id(ns_name);
    let mut store = load_store_read(path, ns, db, key, auth)?;
    let rows = store.select_rows(sql)?;
    reject_dangling_txn(&store)?;
    if store.is_dirty() {
        return Err(loom_core::error::LoomError::new(
            loom_core::error::Code::PermissionDenied,
            "sql query is read-only; use exec for statements that mutate state",
        ));
    }
    Ok(rows)
}

/// Flush the store's overlay only if it changed something, taking the exclusive write lock for just that
/// flush. A pure `SELECT` is left lock-free.
fn persist_if_dirty(
    path: &str,
    ns_name: &str,
    db: &str,
    store: &mut LoomSqlStore,
    key: Option<&str>,
    auth: &LocalOpenAuth,
) -> LoomResult<()> {
    if store.is_dirty() {
        let (mut loom, ns) = open_for_write_path(path, ns_name, key, auth)?;
        store.persist(&mut loom, ns, db)?;
        save_loom(&mut loom)?;
    }
    Ok(())
}

/// A lazy iterator over a `SELECT`'s rows (the Python iterator protocol). `LoomSql.query` returns one;
/// iterate it with `for row in rows:` - each `row` is a list of idiomatic cells (the same mapping as
/// `exec`). The rows are decoded up front (the engine computes the result eagerly); this yields them one
/// at a time so the streaming idiom is available without re-querying.
#[pyclass]
struct LoomRows {
    iter: std::vec::IntoIter<Vec<Value>>,
}

#[pymethods]
impl LoomRows {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<Py<PyList>>> {
        match self.iter.next() {
            Some(row) => Ok(Some(row_to_py(py, &row)?.unbind())),
            None => Ok(None),
        }
    }
}

/// Commit the staged database state; return the new commit's content address.
fn run_commit(
    path: &str,
    ns_name: &str,
    message: &str,
    author: &str,
    key: Option<&str>,
    auth: &LocalOpenAuth,
) -> LoomResult<String> {
    let (mut loom, ns) = open_for_write_path(path, ns_name, key, auth)?;
    let digest = loom.commit(ns, author, message, now_ms())?;
    save_loom(&mut loom)?;
    Ok(digest.to_string())
}

#[pymethods]
impl LoomSql {
    /// Open `loom_path` and start a SQL session over workspace `ns_name`'s SQL facet (created if absent),
    /// database `db`.
    #[new]
    fn new(loom_path: &str, ns_name: &str, db: &str) -> PyResult<Self> {
        Self::open_inner(loom_path, ns_name, db, None, LocalOpenAuth::default())
    }

    /// Open an **encrypted** loom, unlocking with `passphrase` for the session's
    /// lifetime: `LoomSql.open_encrypted(path, ns, db, passphrase)`. A wrong passphrase raises
    /// `E2E_KEY_INVALID`; opening an encrypted loom without one raises `E2E_LOCKED`. The host acquires
    /// the passphrase securely; no environment variable is read.
    #[staticmethod]
    fn open_encrypted(
        loom_path: &str,
        ns_name: &str,
        db: &str,
        passphrase: &str,
    ) -> PyResult<Self> {
        Self::open_inner(
            loom_path,
            ns_name,
            db,
            Some(passphrase.to_string()),
            LocalOpenAuth::default(),
        )
    }

    #[staticmethod]
    fn authenticated(
        loom_path: &str,
        ns_name: &str,
        db: &str,
        auth_principal: &str,
        auth_passphrase: &str,
    ) -> PyResult<Self> {
        let auth = local_auth_sql(Some(auth_principal), Some(auth_passphrase))?;
        Self::open_inner(loom_path, ns_name, db, None, auth)
    }

    #[staticmethod]
    fn open_encrypted_authenticated(
        loom_path: &str,
        ns_name: &str,
        db: &str,
        passphrase: &str,
        auth_principal: &str,
        auth_passphrase: &str,
    ) -> PyResult<Self> {
        let auth = local_auth_sql(Some(auth_principal), Some(auth_passphrase))?;
        Self::open_inner(loom_path, ns_name, db, Some(passphrase.to_string()), auth)
    }

    /// Run one or more `;`-separated SQL statements and return **typed** results: a list of statement
    /// dicts (`{"kind": ..., ...}`). A `select` carries `columns` and `rows` of idiomatic cells - `int`
    /// (arbitrary precision, so 64/128-bit values are exact), `float`, `bytes`, `str`, `bool`, `None`,
    /// and `decimal.Decimal` for an exact decimal. The blocking work runs with the GIL released; the
    /// (cheap) decode + object building hold the GIL. For raw bytes use `exec_bytes`; for the JSON debug
    /// form use `exec_json`.
    fn exec<'py>(&self, py: Python<'py>, sql: &str) -> PyResult<Bound<'py, PyList>> {
        let bytes = py
            .allow_threads(|| {
                run_exec_cbor(
                    &self.path,
                    &self.ns_name,
                    &self.db,
                    sql,
                    self.key.as_deref(),
                    &self.auth,
                )
            })
            .map_err(py_err)?;
        let payload = result_view::decode(&bytes).map_err(py_err)?;
        statements_to_py(py, &payload)
    }

    /// Run SQL; returns a JSON array of the result payloads (debug/admin form, rendered from the
    /// canonical CBOR - not the type-faithful API; use `exec`). Releases the GIL for the blocking work.
    fn exec_json(&self, py: Python<'_>, sql: &str) -> PyResult<String> {
        py.allow_threads(|| {
            run_exec_json(
                &self.path,
                &self.ns_name,
                &self.db,
                sql,
                self.key.as_deref(),
                &self.auth,
            )
        })
        .map_err(py_err)
    }

    /// Run SQL; returns the result payloads as canonical CBOR `bytes`. Releases the
    /// GIL for the blocking work (use with `asyncio.to_thread`).
    fn exec_bytes<'py>(&self, py: Python<'py>, sql: &str) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = py
            .allow_threads(|| {
                run_exec_cbor(
                    &self.path,
                    &self.ns_name,
                    &self.db,
                    sql,
                    self.key.as_deref(),
                    &self.auth,
                )
            })
            .map_err(py_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Run a `SELECT` and return a lazy [`LoomRows`] iterator over its rows (`for row in db.query(sql)`).
    /// Each row is a list of idiomatic cells (same mapping as `exec`). The blocking query runs with the
    /// GIL released; statements that mutate state are rejected.
    fn query(&self, py: Python<'_>, sql: &str) -> PyResult<LoomRows> {
        let rows = py
            .allow_threads(|| {
                run_select_rows(
                    &self.path,
                    &self.ns_name,
                    &self.db,
                    sql,
                    self.key.as_deref(),
                    &self.auth,
                )
            })
            .map_err(py_err)?;
        Ok(LoomRows {
            iter: rows.into_iter(),
        })
    }

    /// Commit the staged database state onto the workspace's current branch; returns the new commit's
    /// content address (`"algo:hex"`). Releases the GIL for the blocking work.
    fn commit(&self, py: Python<'_>, message: &str, author: &str) -> PyResult<String> {
        py.allow_threads(|| {
            run_commit(
                &self.path,
                &self.ns_name,
                message,
                author,
                self.key.as_deref(),
                &self.auth,
            )
        })
        .map_err(py_err)
    }
}

// --- Typed result mapping: a decoded `ResultPayload` -> idiomatic Python (the primary `exec` API). ---

/// Build the typed Python result (a list of statement dicts). SQL `exec` yields statements.
fn statements_to_py<'py>(py: Python<'py>, payload: &ResultPayload) -> PyResult<Bound<'py, PyList>> {
    let stmts = match payload {
        ResultPayload::Statements(s) => s,
        ResultPayload::Reader(_) => {
            return Err(PyRuntimeError::new_err("exec returned a reader result"));
        }
    };
    let list = PyList::empty(py);
    for s in stmts {
        list.append(statement_to_py(py, s)?)?;
    }
    Ok(list)
}

/// One SQL statement result as a `{"kind": ..., ...}` Python dict.
fn statement_to_py<'py>(py: Python<'py>, s: &Statement) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    match s {
        Statement::Select { labels, rows } => {
            d.set_item("kind", "select")?;
            let cols = PyList::empty(py);
            for name in labels {
                let c = PyDict::new(py);
                c.set_item("name", name)?;
                cols.append(c)?;
            }
            d.set_item("columns", cols)?;
            let rs = PyList::empty(py);
            for r in rows {
                rs.append(row_to_py(py, r)?)?;
            }
            d.set_item("rows", rs)?;
        }
        Statement::SelectMap(rows) => {
            d.set_item("kind", "selectMap")?;
            let rs = PyList::empty(py);
            for m in rows {
                rs.append(map_to_py(py, m)?)?;
            }
            d.set_item("rows", rs)?;
        }
        Statement::ShowColumns(cols) => {
            d.set_item("kind", "showColumns")?;
            let cs = PyList::empty(py);
            for c in cols {
                let co = PyDict::new(py);
                co.set_item("name", &c.name)?;
                co.set_item("type", &c.type_name)?;
                cs.append(co)?;
            }
            d.set_item("columns", cs)?;
        }
        Statement::Insert(n) => count_dict(&d, "insert", *n)?,
        Statement::Delete(n) => count_dict(&d, "delete", *n)?,
        Statement::Update(n) => count_dict(&d, "update", *n)?,
        Statement::DropTable(n) => count_dict(&d, "dropTable", *n)?,
        Statement::Create => d.set_item("kind", "create")?,
        Statement::DropFunction => d.set_item("kind", "dropFunction")?,
        Statement::AlterTable => d.set_item("kind", "alterTable")?,
        Statement::CreateIndex => d.set_item("kind", "createIndex")?,
        Statement::DropIndex => d.set_item("kind", "dropIndex")?,
        Statement::StartTransaction => d.set_item("kind", "startTransaction")?,
        Statement::Commit => d.set_item("kind", "commit")?,
        Statement::Rollback => d.set_item("kind", "rollback")?,
        Statement::ShowVariable(sv) => {
            d.set_item("kind", "showVariable")?;
            match sv {
                ShowVariable::Tables(v) => {
                    d.set_item("variable", "tables")?;
                    d.set_item("values", v)?;
                }
                ShowVariable::Functions(v) => {
                    d.set_item("variable", "functions")?;
                    d.set_item("values", v)?;
                }
                ShowVariable::Version(s) => {
                    d.set_item("variable", "version")?;
                    d.set_item("value", s)?;
                }
            }
        }
    }
    Ok(d)
}

fn count_dict(d: &Bound<'_, PyDict>, kind: &str, n: u64) -> PyResult<()> {
    d.set_item("kind", kind)?;
    d.set_item("count", n)?;
    Ok(())
}

fn row_to_py<'py>(py: Python<'py>, cells: &[Value]) -> PyResult<Bound<'py, PyList>> {
    let list = PyList::empty(py);
    for v in cells {
        list.append(cell_to_py(py, v)?)?;
    }
    Ok(list)
}

fn map_to_py<'py>(py: Python<'py>, m: &BTreeMap<String, Value>) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    for (k, v) in m {
        d.set_item(k, cell_to_py(py, v)?)?;
    }
    Ok(d)
}

/// Construct a `decimal.Decimal` from a `(mantissa, scale)` pair (e.g. `12345`, `2` -> `Decimal("123.45")`).
fn decimal_to_py<'py>(py: Python<'py>, mantissa: i128, scale: u32) -> PyResult<Bound<'py, PyAny>> {
    let s = format!("{mantissa}e-{scale}");
    py.import("decimal")?.getattr("Decimal")?.call1((s,))
}

/// One decoded cell as an idiomatic Python object.
fn cell_to_py<'py>(py: Python<'py>, v: &Value) -> PyResult<Bound<'py, PyAny>> {
    Ok(match v {
        Value::Null => py.None().into_bound(py),
        Value::Bool(b) => b.into_bound_py_any(py)?,
        Value::Int(x) => x.into_bound_py_any(py)?,
        Value::I8(x) => x.into_bound_py_any(py)?,
        Value::I16(x) => x.into_bound_py_any(py)?,
        Value::I32(x) => x.into_bound_py_any(py)?,
        Value::U8(x) => x.into_bound_py_any(py)?,
        Value::U16(x) => x.into_bound_py_any(py)?,
        Value::U32(x) => x.into_bound_py_any(py)?,
        Value::U64(x) => x.into_bound_py_any(py)?,
        Value::I128(x) => x.into_bound_py_any(py)?,
        Value::U128(x) => x.into_bound_py_any(py)?,
        Value::Float(x) => x.into_bound_py_any(py)?,
        Value::F32(x) => f64::from(*x).into_bound_py_any(py)?,
        Value::Text(s) => s.as_str().into_bound_py_any(py)?,
        Value::Bytes(b) => PyBytes::new(py, b).into_any(),
        Value::Decimal { mantissa, scale } => decimal_to_py(py, *mantissa, *scale)?,
        Value::Date(d) => d.into_bound_py_any(py)?,
        Value::Time(t) => t.into_bound_py_any(py)?,
        Value::Timestamp(t) => t.into_bound_py_any(py)?,
        Value::Interval { months, micros } => {
            let d = PyDict::new(py);
            d.set_item("months", *months)?;
            d.set_item("micros", *micros)?;
            d.into_any()
        }
        Value::Uuid(u) => format!("{u:032x}").into_bound_py_any(py)?,
        Value::Inet(ip) => ip.to_string().into_bound_py_any(py)?,
        Value::Point { x, y } => {
            let d = PyDict::new(py);
            d.set_item("x", *x)?;
            d.set_item("y", *y)?;
            d.into_any()
        }
        Value::List(items) => row_to_py(py, items)?.into_any(),
        Value::Map(m) => map_to_py(py, m)?.into_any(),
    })
}

/// The held-open state of a [`LoomSqlBatch`]: the loom (whose lifetime holds the exclusive write lock),
/// the resolved workspace, the database name, and the SQL store loaded once for the batch.
struct BatchInner {
    loom: Loom<FileStore>,
    ns: WorkspaceId,
    db: String,
    path: String,
    store: LoomSqlStore,
    /// Unlock passphrase for an encrypted loom, or `None`; kept so `abort` can
    /// re-snapshot a lock-free read view.
    key: Option<String>,
    auth: LocalOpenAuth,
}

/// An explicit transaction/batch scope: holds the `.loom` (and its write lock) open across statements,
/// so an SQL transaction (`BEGIN`/`COMMIT`/`ROLLBACK`) can span `exec` calls, and changes are made
/// durable by one atomic save at `commit`. Because Python GC is non-deterministic, call `close()` to
/// release the lock promptly (closing without a commit discards un-persisted changes). The SQL `COMMIT`
/// is distinct from the VCS `commit_vcs`. Unsendable: the held loom is bound to the creating thread.
#[pyclass(unsendable)]
struct LoomSqlBatch {
    inner: Option<BatchInner>,
}

impl LoomSqlBatch {
    fn inner_mut(&mut self) -> PyResult<&mut BatchInner> {
        self.inner
            .as_mut()
            .ok_or_else(|| PyRuntimeError::new_err("batch is closed"))
    }

    /// Open the held write loom + read snapshot (unlocking with `key` if encrypted). Not a
    /// `#[pymethods]` member, so it is not exposed to Python (the `new` / `open_encrypted` ctors call it).
    fn begin_inner(
        loom_path: &str,
        ns_name: &str,
        db: &str,
        key: Option<String>,
        auth: LocalOpenAuth,
    ) -> PyResult<Self> {
        let (loom, ns) =
            open_for_write_path(loom_path, ns_name, key.as_deref(), &auth).map_err(py_err)?;
        let store = load_store_write(loom_path, ns, db, key.as_deref(), &auth).map_err(py_err)?;
        Ok(Self {
            inner: Some(BatchInner {
                loom,
                ns,
                db: db.to_string(),
                path: loom_path.to_string(),
                store,
                key,
                auth,
            }),
        })
    }
}

#[pymethods]
impl LoomSqlBatch {
    /// Begin a batch over workspace `ns_name`'s SQL facet (created if absent), database `db`, in
    /// `loom_path`. Holds the write lock until `close()`.
    #[new]
    fn new(loom_path: &str, ns_name: &str, db: &str) -> PyResult<Self> {
        Self::begin_inner(loom_path, ns_name, db, None, LocalOpenAuth::default())
    }

    /// Begin a batch over an **encrypted** loom, unlocking with `passphrase` for the
    /// batch's lifetime: `LoomSqlBatch.open_encrypted(path, ns, db, passphrase)`.
    #[staticmethod]
    fn open_encrypted(
        loom_path: &str,
        ns_name: &str,
        db: &str,
        passphrase: &str,
    ) -> PyResult<Self> {
        Self::begin_inner(
            loom_path,
            ns_name,
            db,
            Some(passphrase.to_string()),
            LocalOpenAuth::default(),
        )
    }

    #[staticmethod]
    fn authenticated(
        loom_path: &str,
        ns_name: &str,
        db: &str,
        auth_principal: &str,
        auth_passphrase: &str,
    ) -> PyResult<Self> {
        let auth = local_auth_sql(Some(auth_principal), Some(auth_passphrase))?;
        Self::begin_inner(loom_path, ns_name, db, None, auth)
    }

    #[staticmethod]
    fn open_encrypted_authenticated(
        loom_path: &str,
        ns_name: &str,
        db: &str,
        passphrase: &str,
        auth_principal: &str,
        auth_passphrase: &str,
    ) -> PyResult<Self> {
        let auth = local_auth_sql(Some(auth_principal), Some(auth_passphrase))?;
        Self::begin_inner(loom_path, ns_name, db, Some(passphrase.to_string()), auth)
    }

    /// Run SQL in the batch and return typed results (same shape as `LoomSql.exec`). Includes
    /// `BEGIN`/`COMMIT`/`ROLLBACK`; changes accumulate until `commit`.
    fn exec<'py>(&mut self, py: Python<'py>, sql: &str) -> PyResult<Bound<'py, PyList>> {
        let bytes = self.inner_mut()?.store.exec_cbor(sql).map_err(py_err)?;
        let payload = result_view::decode(&bytes).map_err(py_err)?;
        statements_to_py(py, &payload)
    }

    /// Run SQL in the batch; returns the result payloads as canonical CBOR `bytes`.
    fn exec_bytes<'py>(&mut self, py: Python<'py>, sql: &str) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.inner_mut()?.store.exec_cbor(sql).map_err(py_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Run SQL in the batch; returns the JSON debug form.
    fn exec_json(&mut self, sql: &str) -> PyResult<String> {
        self.inner_mut()?.store.exec_json(sql).map_err(py_err)
    }

    /// Make the batch's changes durable with one atomic save (no history entry). Rejected while an SQL
    /// transaction is open. The batch stays open.
    fn commit(&mut self) -> PyResult<()> {
        let b = self.inner_mut()?;
        if b.store.in_transaction() {
            return Err(PyRuntimeError::new_err(
                "the batch has an open SQL transaction; COMMIT or ROLLBACK first",
            ));
        }
        b.store.persist(&mut b.loom, b.ns, &b.db).map_err(py_err)?;
        save_loom(&mut b.loom).map_err(py_err)?;
        Ok(())
    }

    /// Like `commit`, but also records a VCS commit; returns the commit's content address. Distinct from
    /// a SQL `COMMIT`. Rejected while an SQL transaction is open.
    fn commit_vcs(&mut self, message: &str, author: &str) -> PyResult<String> {
        let b = self.inner_mut()?;
        if b.store.in_transaction() {
            return Err(PyRuntimeError::new_err(
                "the batch has an open SQL transaction; COMMIT or ROLLBACK first",
            ));
        }
        b.store.persist(&mut b.loom, b.ns, &b.db).map_err(py_err)?;
        let digest = b
            .loom
            .commit(b.ns, author, message, now_ms())
            .map_err(py_err)?;
        save_loom(&mut b.loom).map_err(py_err)?;
        Ok(digest.to_string())
    }

    /// Discard un-persisted in-memory changes (and any open SQL transaction), reloading from the last
    /// durable state. The batch stays open.
    fn abort(&mut self) -> PyResult<()> {
        let b = self.inner_mut()?;
        b.store =
            load_store_write(&b.path, b.ns, &b.db, b.key.as_deref(), &b.auth).map_err(py_err)?;
        Ok(())
    }

    /// Release the write lock and free the batch. Closing without a commit discards un-persisted changes.
    fn close(&mut self) {
        self.inner = None;
    }
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(admin::version, m)?)?;
    m.add_function(wrap_pyfunction!(admin::capabilities, m)?)?;
    m.add_function(wrap_pyfunction!(admin::runtime_profile, m)?)?;
    m.add_function(wrap_pyfunction!(admin::studio_surface_catalog_json, m)?)?;
    m.add_function(wrap_pyfunction!(admin::blob_digest, m)?)?;
    m.add_function(wrap_pyfunction!(admin::create_loom, m)?)?;
    m.add_function(wrap_pyfunction!(exec_cbor, m)?)?;
    m.add_function(wrap_pyfunction!(admin::authenticate_passphrase, m)?)?;
    m.add_function(wrap_pyfunction!(admin::identity_list_json, m)?)?;
    m.add_function(wrap_pyfunction!(admin::identity_add_principal, m)?)?;
    m.add_function(wrap_pyfunction!(
        admin::identity_rename_principal_handle,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(admin::identity_set_passphrase, m)?)?;
    m.add_function(wrap_pyfunction!(admin::identity_remove_principal, m)?)?;
    m.add_function(wrap_pyfunction!(admin::identity_assign_role, m)?)?;
    m.add_function(wrap_pyfunction!(admin::identity_revoke_role, m)?)?;
    m.add_function(wrap_pyfunction!(
        admin::identity_create_external_credential,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        admin::identity_revoke_external_credential,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(admin::identity_add_public_key, m)?)?;
    m.add_function(wrap_pyfunction!(admin::identity_revoke_public_key, m)?)?;
    m.add_function(wrap_pyfunction!(admin::acl_list_json, m)?)?;
    m.add_function(wrap_pyfunction!(admin::acl_grant, m)?)?;
    m.add_function(wrap_pyfunction!(admin::acl_grant_scoped, m)?)?;
    m.add_function(wrap_pyfunction!(admin::acl_revoke, m)?)?;
    m.add_function(wrap_pyfunction!(admin::acl_revoke_scoped, m)?)?;
    m.add_function(wrap_pyfunction!(admin::protected_ref_list_json, m)?)?;
    m.add_function(wrap_pyfunction!(admin::protected_ref_get_json, m)?)?;
    m.add_function(wrap_pyfunction!(admin::protected_ref_set, m)?)?;
    m.add_function(wrap_pyfunction!(admin::protected_ref_remove, m)?)?;
    m.add_function(wrap_pyfunction!(workspace::workspace_create, m)?)?;
    m.add_function(wrap_pyfunction!(workspace::workspace_list_json, m)?)?;
    m.add_function(wrap_pyfunction!(workspace::workspace_rename, m)?)?;
    m.add_function(wrap_pyfunction!(workspace::workspace_delete, m)?)?;
    m.add_function(wrap_pyfunction!(admin::key_add_wrap_keyed, m)?)?;
    m.add_function(wrap_pyfunction!(admin::key_add_wrap_with_kek, m)?)?;
    m.add_function(wrap_pyfunction!(admin::key_remove_wrap, m)?)?;
    m.add_function(wrap_pyfunction!(archive::fs_import, m)?)?;
    m.add_function(wrap_pyfunction!(archive::fs_export, m)?)?;
    m.add_function(wrap_pyfunction!(archive::archive_import, m)?)?;
    m.add_function(wrap_pyfunction!(archive::archive_export, m)?)?;
    m.add_function(wrap_pyfunction!(archive::car_import, m)?)?;
    m.add_function(wrap_pyfunction!(archive::car_export, m)?)?;
    m.add_function(wrap_pyfunction!(cas::cas_put, m)?)?;
    m.add_function(wrap_pyfunction!(cas::cas_get, m)?)?;
    m.add_function(wrap_pyfunction!(cas::cas_has, m)?)?;
    m.add_function(wrap_pyfunction!(cas::cas_delete, m)?)?;
    m.add_function(wrap_pyfunction!(cas::cas_list_json, m)?)?;
    m.add_function(wrap_pyfunction!(meetings::meetings_import_snapshot, m)?)?;
    m.add_function(wrap_pyfunction!(meetings::meetings_source_read, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_list_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_stat_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_read_file, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_list_versions_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_list_conflicts_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_list_shares_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_list_retention_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_create_folder_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_create_upload_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_upload_chunk_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_commit_upload_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_rename_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_move_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_delete_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_resolve_conflict_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_grant_share_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_revoke_share_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_apply_share_expiry_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_pin_retention_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_unpin_retention_json, m)?)?;
    m.add_function(wrap_pyfunction!(drive::drive_apply_retention_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_project_create_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_project_rekey_json, m)?)?;
    m.add_function(wrap_pyfunction!(
        tickets::tickets_project_settings_get_json,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        tickets::tickets_project_settings_set_json,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_fields_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_field_put_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_field_retire_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_create_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_update_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_delete_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_comments_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_comment_add_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_comment_update_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_comment_delete_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_board_create_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_board_get_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_board_list_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_board_update_json, m)?)?;
    m.add_function(wrap_pyfunction!(
        tickets::tickets_board_configure_columns_json,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_board_move_card_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_board_delete_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_relation_set_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_relation_remove_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_relation_list_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_get_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_list_json, m)?)?;
    m.add_function(wrap_pyfunction!(tickets::tickets_history_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::spaces_create_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::spaces_list_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::spaces_get_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::pages_create_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::pages_update_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::pages_publish_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::pages_get_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::pages_list_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::pages_history_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::structures_create_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::structures_add_node_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::structures_update_node_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::structures_bind_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::structures_move_node_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::structures_link_node_json, m)?)?;
    m.add_function(wrap_pyfunction!(
        pages::structures_decompose_to_tickets_json,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(pages::structures_get_json, m)?)?;
    m.add_function(wrap_pyfunction!(pages::structures_list_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_create_channel_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_rename_channel_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_list_channels_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_post_message_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_edit_message_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_redact_message_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_create_thread_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_create_task_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_claim_task_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_complete_task_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_invoke_agent_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_agent_reply_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_request_handoff_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_add_reaction_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_remove_reaction_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_emoji_list_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_emoji_register_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_emoji_unregister_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_messages_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_cursor_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_update_cursor_json, m)?)?;
    m.add_function(wrap_pyfunction!(chat::chat_fetch_events_json, m)?)?;
    m.add_function(wrap_pyfunction!(kv::kv_put, m)?)?;
    m.add_function(wrap_pyfunction!(kv::kv_get, m)?)?;
    m.add_function(wrap_pyfunction!(kv::kv_delete, m)?)?;
    m.add_function(wrap_pyfunction!(kv::kv_list, m)?)?;
    m.add_function(wrap_pyfunction!(kv::kv_range, m)?)?;
    m.add_function(wrap_pyfunction!(lanes::lanes_create, m)?)?;
    m.add_function(wrap_pyfunction!(lanes::lanes_get, m)?)?;
    m.add_function(wrap_pyfunction!(lanes::lanes_list, m)?)?;
    m.add_function(wrap_pyfunction!(lanes::lanes_update, m)?)?;
    m.add_function(wrap_pyfunction!(lanes::lanes_ticket_add, m)?)?;
    m.add_function(wrap_pyfunction!(lanes::lanes_ticket_remove, m)?)?;
    m.add_function(wrap_pyfunction!(lanes::lanes_ticket_transfer, m)?)?;
    m.add_function(wrap_pyfunction!(lanes::lanes_delete, m)?)?;
    m.add_function(wrap_pyfunction!(graph::graph_upsert_node, m)?)?;
    m.add_function(wrap_pyfunction!(graph::graph_get_node, m)?)?;
    m.add_function(wrap_pyfunction!(graph::graph_remove_node, m)?)?;
    m.add_function(wrap_pyfunction!(graph::graph_upsert_edge, m)?)?;
    m.add_function(wrap_pyfunction!(graph::graph_get_edge, m)?)?;
    m.add_function(wrap_pyfunction!(graph::graph_remove_edge, m)?)?;
    m.add_function(wrap_pyfunction!(graph::graph_neighbors, m)?)?;
    m.add_function(wrap_pyfunction!(graph::graph_out_edges, m)?)?;
    m.add_function(wrap_pyfunction!(graph::graph_in_edges, m)?)?;
    m.add_function(wrap_pyfunction!(graph::graph_reachable, m)?)?;
    m.add_function(wrap_pyfunction!(graph::graph_shortest_path, m)?)?;
    m.add_function(wrap_pyfunction!(graph::graph_query, m)?)?;
    m.add_function(wrap_pyfunction!(graph::graph_explain_query, m)?)?;
    m.add_function(wrap_pyfunction!(vector::vector_create, m)?)?;
    m.add_function(wrap_pyfunction!(vector::vector_upsert, m)?)?;
    m.add_function(wrap_pyfunction!(vector::vector_upsert_source, m)?)?;
    m.add_function(wrap_pyfunction!(vector::vector_get, m)?)?;
    m.add_function(wrap_pyfunction!(vector::vector_source_text, m)?)?;
    m.add_function(wrap_pyfunction!(vector::vector_embedding_model, m)?)?;
    m.add_function(wrap_pyfunction!(vector::vector_ids, m)?)?;
    m.add_function(wrap_pyfunction!(vector::vector_metadata_index_keys, m)?)?;
    m.add_function(wrap_pyfunction!(vector::vector_create_metadata_index, m)?)?;
    m.add_function(wrap_pyfunction!(vector::vector_drop_metadata_index, m)?)?;
    m.add_function(wrap_pyfunction!(vector::vector_delete, m)?)?;
    m.add_function(wrap_pyfunction!(vector::vector_search, m)?)?;
    m.add_function(wrap_pyfunction!(vector::vector_search_policy, m)?)?;
    m.add_function(wrap_pyfunction!(columnar::columnar_create, m)?)?;
    m.add_function(wrap_pyfunction!(columnar::columnar_append, m)?)?;
    m.add_function(wrap_pyfunction!(columnar::columnar_scan, m)?)?;
    m.add_function(wrap_pyfunction!(columnar::columnar_columns, m)?)?;
    m.add_function(wrap_pyfunction!(columnar::columnar_rows, m)?)?;
    m.add_function(wrap_pyfunction!(columnar::columnar_compact, m)?)?;
    m.add_function(wrap_pyfunction!(columnar::columnar_inspect, m)?)?;
    m.add_function(wrap_pyfunction!(columnar::columnar_source_digest, m)?)?;
    m.add_function(wrap_pyfunction!(columnar::columnar_select, m)?)?;
    m.add_function(wrap_pyfunction!(columnar::columnar_aggregate, m)?)?;
    m.add_function(wrap_pyfunction!(dataframe::dataframe_create, m)?)?;
    m.add_function(wrap_pyfunction!(dataframe::dataframe_collect, m)?)?;
    m.add_function(wrap_pyfunction!(dataframe::dataframe_preview, m)?)?;
    m.add_function(wrap_pyfunction!(dataframe::dataframe_materialize, m)?)?;
    m.add_function(wrap_pyfunction!(dataframe::dataframe_plan_digest, m)?)?;
    m.add_function(wrap_pyfunction!(dataframe::dataframe_source_digests, m)?)?;
    m.add_function(wrap_pyfunction!(search::search_create, m)?)?;
    m.add_function(wrap_pyfunction!(search::search_index, m)?)?;
    m.add_function(wrap_pyfunction!(search::search_get, m)?)?;
    m.add_function(wrap_pyfunction!(search::search_delete, m)?)?;
    m.add_function(wrap_pyfunction!(search::search_ids, m)?)?;
    m.add_function(wrap_pyfunction!(search::search_remap, m)?)?;
    m.add_function(wrap_pyfunction!(search::search_query, m)?)?;
    m.add_function(wrap_pyfunction!(document::doc_put_text, m)?)?;
    m.add_function(wrap_pyfunction!(document::doc_get_text, m)?)?;
    m.add_function(wrap_pyfunction!(document::doc_put_binary, m)?)?;
    m.add_function(wrap_pyfunction!(document::doc_get_binary, m)?)?;
    m.add_function(wrap_pyfunction!(document::doc_delete, m)?)?;
    m.add_function(wrap_pyfunction!(document::doc_list_binary, m)?)?;
    m.add_function(wrap_pyfunction!(document::doc_index_create, m)?)?;
    m.add_function(wrap_pyfunction!(document::doc_index_create_json, m)?)?;
    m.add_function(wrap_pyfunction!(document::doc_index_drop, m)?)?;
    m.add_function(wrap_pyfunction!(document::doc_index_rebuild, m)?)?;
    m.add_function(wrap_pyfunction!(document::doc_index_list_json, m)?)?;
    m.add_function(wrap_pyfunction!(document::doc_index_status_json, m)?)?;
    m.add_function(wrap_pyfunction!(document::doc_find_json, m)?)?;
    m.add_function(wrap_pyfunction!(document::doc_query_json, m)?)?;
    m.add_function(wrap_pyfunction!(timeseries::ts_put, m)?)?;
    m.add_function(wrap_pyfunction!(timeseries::ts_get, m)?)?;
    m.add_function(wrap_pyfunction!(timeseries::ts_range, m)?)?;
    m.add_function(wrap_pyfunction!(timeseries::ts_latest, m)?)?;
    m.add_function(wrap_pyfunction!(telemetry::metrics_put_descriptor, m)?)?;
    m.add_function(wrap_pyfunction!(telemetry::metrics_get_descriptor, m)?)?;
    m.add_function(wrap_pyfunction!(telemetry::metrics_put_observation, m)?)?;
    m.add_function(wrap_pyfunction!(telemetry::metrics_query, m)?)?;
    m.add_function(wrap_pyfunction!(telemetry::logs_put_record, m)?)?;
    m.add_function(wrap_pyfunction!(telemetry::logs_get_record, m)?)?;
    m.add_function(wrap_pyfunction!(telemetry::logs_query, m)?)?;
    m.add_function(wrap_pyfunction!(telemetry::traces_put_span, m)?)?;
    m.add_function(wrap_pyfunction!(telemetry::traces_get_span, m)?)?;
    m.add_function(wrap_pyfunction!(telemetry::traces_trace_spans, m)?)?;
    m.add_function(wrap_pyfunction!(telemetry::traces_query, m)?)?;
    m.add_function(wrap_pyfunction!(ledger::ledger_append, m)?)?;
    m.add_function(wrap_pyfunction!(ledger::ledger_get, m)?)?;
    m.add_function(wrap_pyfunction!(ledger::ledger_head, m)?)?;
    m.add_function(wrap_pyfunction!(ledger::ledger_len, m)?)?;
    m.add_function(wrap_pyfunction!(ledger::ledger_verify, m)?)?;
    m.add_function(wrap_pyfunction!(calendar_fns::cal_create_collection, m)?)?;
    m.add_function(wrap_pyfunction!(calendar_fns::cal_delete_collection, m)?)?;
    m.add_function(wrap_pyfunction!(calendar_fns::cal_list_collections, m)?)?;
    m.add_function(wrap_pyfunction!(calendar_fns::cal_put_entry, m)?)?;
    m.add_function(wrap_pyfunction!(calendar_fns::cal_get_entry, m)?)?;
    m.add_function(wrap_pyfunction!(calendar_fns::cal_delete_entry, m)?)?;
    m.add_function(wrap_pyfunction!(calendar_fns::cal_list_entries, m)?)?;
    m.add_function(wrap_pyfunction!(calendar_fns::cal_range, m)?)?;
    m.add_function(wrap_pyfunction!(calendar_fns::cal_search, m)?)?;
    m.add_function(wrap_pyfunction!(calendar_fns::cal_entry_ics, m)?)?;
    m.add_function(wrap_pyfunction!(calendar_fns::cal_put_ics, m)?)?;
    m.add_function(wrap_pyfunction!(contacts_fns::card_create_book, m)?)?;
    m.add_function(wrap_pyfunction!(contacts_fns::card_delete_book, m)?)?;
    m.add_function(wrap_pyfunction!(contacts_fns::card_list_books, m)?)?;
    m.add_function(wrap_pyfunction!(contacts_fns::card_put_entry, m)?)?;
    m.add_function(wrap_pyfunction!(contacts_fns::card_get_entry, m)?)?;
    m.add_function(wrap_pyfunction!(contacts_fns::card_delete_entry, m)?)?;
    m.add_function(wrap_pyfunction!(contacts_fns::card_list_entries, m)?)?;
    m.add_function(wrap_pyfunction!(contacts_fns::card_search, m)?)?;
    m.add_function(wrap_pyfunction!(contacts_fns::card_entry_vcard, m)?)?;
    m.add_function(wrap_pyfunction!(contacts_fns::card_put_vcard, m)?)?;
    m.add_function(wrap_pyfunction!(mail_fns::mail_create_mailbox, m)?)?;
    m.add_function(wrap_pyfunction!(mail_fns::mail_delete_mailbox, m)?)?;
    m.add_function(wrap_pyfunction!(mail_fns::mail_list_mailboxes, m)?)?;
    m.add_function(wrap_pyfunction!(mail_fns::mail_ingest_message, m)?)?;
    m.add_function(wrap_pyfunction!(mail_fns::mail_get_message, m)?)?;
    m.add_function(wrap_pyfunction!(mail_fns::mail_to_eml, m)?)?;
    m.add_function(wrap_pyfunction!(mail_fns::mail_delete_message, m)?)?;
    m.add_function(wrap_pyfunction!(mail_fns::mail_list_messages, m)?)?;
    m.add_function(wrap_pyfunction!(mail_fns::mail_get_flags, m)?)?;
    m.add_function(wrap_pyfunction!(mail_fns::mail_set_flags, m)?)?;
    m.add_function(wrap_pyfunction!(mail_fns::mail_search, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::merge_in_progress, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::merge_conflicts, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::merge_resolve, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::merge_abort, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::merge_continue, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::stage, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::stage_all, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::unstage, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::status_json, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::commit_staged, m)?)?;
    m.add_function(wrap_pyfunction!(files::write_file, m)?)?;
    m.add_function(wrap_pyfunction!(files::read_file, m)?)?;
    m.add_function(wrap_pyfunction!(files::append_file, m)?)?;
    m.add_function(wrap_pyfunction!(files::remove_file, m)?)?;
    m.add_function(wrap_pyfunction!(files::symlink, m)?)?;
    m.add_function(wrap_pyfunction!(files::read_link, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::restore_file, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::restore_path, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::cherry_pick, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::revert, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::rebase, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::squash, m)?)?;
    m.add_function(wrap_pyfunction!(files::read_at, m)?)?;
    m.add_function(wrap_pyfunction!(files::write_at, m)?)?;
    m.add_function(wrap_pyfunction!(files::truncate_file, m)?)?;
    m.add_function(wrap_pyfunction!(files::file_open, m)?)?;
    m.add_function(wrap_pyfunction!(files::file_read, m)?)?;
    m.add_function(wrap_pyfunction!(files::file_read_at, m)?)?;
    m.add_function(wrap_pyfunction!(files::file_write, m)?)?;
    m.add_function(wrap_pyfunction!(files::file_write_at, m)?)?;
    m.add_function(wrap_pyfunction!(files::file_truncate, m)?)?;
    m.add_function(wrap_pyfunction!(files::file_flush, m)?)?;
    m.add_function(wrap_pyfunction!(files::file_stat, m)?)?;
    m.add_function(wrap_pyfunction!(files::file_close, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::tag_create, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::tag_list, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::tag_target, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::tag_delete, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::tag_rename, m)?)?;
    m.add_function(wrap_pyfunction!(queue::queue_append, m)?)?;
    m.add_function(wrap_pyfunction!(queue::queue_consumer_position, m)?)?;
    m.add_function(wrap_pyfunction!(queue::queue_consumer_read, m)?)?;
    m.add_function(wrap_pyfunction!(queue::queue_consumer_advance, m)?)?;
    m.add_function(wrap_pyfunction!(queue::queue_consumer_reset, m)?)?;
    m.add_function(wrap_pyfunction!(queue::queue_get, m)?)?;
    m.add_function(wrap_pyfunction!(queue::queue_range, m)?)?;
    m.add_function(wrap_pyfunction!(queue::queue_len, m)?)?;
    m.add_function(wrap_pyfunction!(daemon_fns::daemon_status_json, m)?)?;
    m.add_function(wrap_pyfunction!(daemon_fns::daemon_session_attach, m)?)?;
    m.add_function(wrap_pyfunction!(daemon_fns::daemon_session_detach, m)?)?;
    m.add_function(wrap_pyfunction!(daemon_fns::daemon_pin_add, m)?)?;
    m.add_function(wrap_pyfunction!(daemon_fns::daemon_pin_remove, m)?)?;
    m.add_function(wrap_pyfunction!(daemon_fns::lock_acquire_json, m)?)?;
    m.add_function(wrap_pyfunction!(daemon_fns::lock_refresh_json, m)?)?;
    m.add_function(wrap_pyfunction!(daemon_fns::lock_release, m)?)?;
    m.add_function(wrap_pyfunction!(sql::sql_read_table, m)?)?;
    m.add_function(wrap_pyfunction!(sql::sql_read_table_at, m)?)?;
    m.add_function(wrap_pyfunction!(sql::sql_index_scan, m)?)?;
    m.add_function(wrap_pyfunction!(sql::sql_index_scan_at, m)?)?;
    m.add_function(wrap_pyfunction!(sql::sql_blame, m)?)?;
    m.add_function(wrap_pyfunction!(sql::sql_diff, m)?)?;
    m.add_function(wrap_pyfunction!(sql::sql_table_diff, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::vcs_blame, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::vcs_diff, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::watch_subscribe, m)?)?;
    m.add_function(wrap_pyfunction!(vcs::watch_poll, m)?)?;
    m.add_function(wrap_pyfunction!(admin::result_to_json, m)?)?;
    m.add_function(wrap_pyfunction!(admin::result_to_bridge_json, m)?)?;
    m.add_class::<LoomSql>()?;
    m.add_class::<LoomSqlBatch>()?;
    m.add_class::<LoomRows>()?;
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
        assert!(
            err.to_string().contains("remote feature"),
            "unexpected error: {err}"
        );
        assert!(normalize_locator("http://loom.example.com/prod").is_err());
    }
}
