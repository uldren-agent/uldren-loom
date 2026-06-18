use js_sys::{Array, BigInt, Object, Reflect, Uint8Array};
use loom_codec::{Value as CborValue, encode as cbor_encode};
use loom_core::calendar::{
    self, CalendarEntry, CollectionMeta, Component, DateTime, IcalDate, IcalMonth, IcalTime,
};
use loom_core::contacts::{self, BookMeta, ContactEntry};
use loom_core::keys::{EncryptionMeta, KEY_LEN, KeySpec, Suite};
use loom_core::mail::{self, MailMessage, MailboxMeta};
use loom_core::tabular::Value;
use loom_core::vcs::ChangeKind;
use loom_core::workspace::{AclDomain, FacetKind, WorkspaceId};
use loom_core::{
    AclEffect, AclGrant, AclPredicate, AclRight, AclScope, AclScopeKind, AclStore, AclSubject,
    Algo, Digest, DocumentFieldPath, DocumentIndexDef, ExternalCredential, ExternalCredentialKind,
    ExternalCredentialSpec, IdentityPublicKeySpec, IdentityRole, IdentityStore, Loom, Principal,
    PrincipalKind, ProtectedRefPolicy, Series, WatchCursor, WatchSelector, WsSelector, cas_delete,
    cas_get, cas_has, cas_list, cas_put, doc_create_index, doc_delete, doc_drop_index, doc_find,
    doc_index_statuses, doc_list_indexes, doc_query, doc_rebuild_index, document_get_binary,
    document_get_text, document_ids_json, document_index_statuses_json,
    document_index_value_from_json, document_indexes_json, document_list_binary,
    document_put_binary_with_entity_tag, document_put_text_with_entity_tag,
    document_query_from_json, document_query_result_json, key_from_cbor, kv_delete, kv_get,
    kv_list, kv_put, kv_range, ledger_append, ledger_get, ledger_head, ledger_len, ledger_verify,
    ts_get, ts_latest, ts_put, ts_range, watch_batch_to_cbor,
};
use loom_result::result_view;
use loom_result::result_view::{ResultPayload, ShowVariable, Statement};
use loom_sql::{LoomSqlStore, lookup_cbor, result_cbor};
use loom_store::{
    BackingIo, FileStore, MemoryBacking, loom_over_backing, loom_over_backing_encrypted,
    loom_over_backing_profile, loom_over_backing_unlocked, save_loom,
};
use std::collections::{BTreeMap, BTreeSet};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetFileOptions,
    FileSystemReadWriteOptions, FileSystemSyncAccessHandle, WorkerGlobalScope,
};

fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

fn derive_sql_ns_id(name: &str) -> WorkspaceId {
    let d = Digest::blake3(format!("{}:{name}", FacetKind::Sql.as_str()).as_bytes());
    let mut id = [0u8; 16];
    id.copy_from_slice(&d.bytes()[..16]);
    WorkspaceId::from_bytes(id)
}

fn random_workspace_id() -> Result<WorkspaceId, JsError> {
    let mut id = [0u8; 16];
    rng_fill(&mut id)?;
    Ok(WorkspaceId::v4_from_bytes(id))
}

fn resolve_workspace_arg(loom: &Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
    let selector = match WorkspaceId::parse(workspace) {
        Ok(id) => WsSelector::Id(id),
        Err(_) => WsSelector::Name(workspace.to_string()),
    };
    loom.registry().open(&selector).map_err(le)
}

fn resolve_typed_ns(
    loom: &Loom<FileStore>,
    facet: &str,
    name: &str,
) -> Result<WorkspaceId, JsError> {
    let ty = FacetKind::parse(facet).map_err(le)?;
    loom.registry()
        .open(&WsSelector::Typed {
            ty,
            name: name.to_string(),
        })
        .map_err(le)
}

fn install_acl_predicate_evaluator(loom: &mut Loom<FileStore>) {
    loom.set_acl_predicate_evaluator(std::sync::Arc::new(loom_compute::CelAclPredicateEvaluator));
}

fn parse_watch_change_kinds(kinds: Vec<String>) -> Result<Vec<ChangeKind>, JsError> {
    kinds
        .into_iter()
        .map(|kind| match kind.as_str() {
            "added" => Ok(ChangeKind::Added),
            "modified" => Ok(ChangeKind::Modified),
            "deleted" => Ok(ChangeKind::Deleted),
            other => Err(JsError::new(&format!(
                "unknown watch change kind {other:?}"
            ))),
        })
        .collect()
}

/// Resolve a workspace for a queue write by UUID or name, ensuring the `queue` facet exists.
fn ensure_queue_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
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
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Queue)
        .map_err(le)?;
    Ok(ns)
}

/// Resolve a workspace for a CAS write by UUID or name, ensuring the `cas` facet exists.
fn ensure_cas_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
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
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Cas)
        .map_err(le)?;
    Ok(ns)
}

/// Resolve a workspace for a kv write by UUID or name, ensuring the `kv` facet exists.
fn ensure_kv_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
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
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Kv)
        .map_err(le)?;
    Ok(ns)
}

fn ensure_doc_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
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
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Document)
        .map_err(le)?;
    Ok(ns)
}

fn ensure_ts_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
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
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::TimeSeries)
        .map_err(le)?;
    Ok(ns)
}

fn ensure_ledger_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
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
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Ledger)
        .map_err(le)?;
    Ok(ns)
}

/// Resolve a workspace for a calendar write by UUID or name, ensuring the `calendar` facet exists.
fn ensure_cal_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
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
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Calendar)
        .map_err(le)?;
    Ok(ns)
}

/// Resolve a workspace for a contacts write by UUID or name, ensuring the `contacts` facet exists.
fn ensure_card_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
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
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Contacts)
        .map_err(le)?;
    Ok(ns)
}

/// Resolve a workspace for a mail write by UUID or name, ensuring the `mail` facet exists.
fn ensure_mail_ns(loom: &mut Loom<FileStore>, workspace: &str) -> Result<WorkspaceId, JsError> {
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
        .map_err(le)?;
    loom.registry_mut()
        .add_facet(ns, FacetKind::Mail)
        .map_err(le)?;
    Ok(ns)
}

/// Parse a comma-separated component list ("event,todo"; an empty string is the empty set) into the
/// `component_set` of a [`CollectionMeta`]. An unknown token is an error.
fn parse_component_set(components: &str) -> Result<Vec<Component>, JsError> {
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
                return Err(JsError::new(&format!(
                    "loom_cal: unknown component {other:?}"
                )));
            }
        }
    }
    Ok(out)
}

/// Map a component-filter string to the calendar facet's optional component: "" -> `None`,
/// "event" -> `Some(Event)`, "todo" -> `Some(Todo)`. Any other token is an error.
fn parse_component_filter(component: &str) -> Result<Option<Component>, JsError> {
    match component {
        "" => Ok(None),
        "event" => Ok(Some(Component::Event)),
        "todo" => Ok(Some(Component::Todo)),
        other => Err(JsError::new(&format!(
            "loom_cal: unknown component filter {other:?}"
        ))),
    }
}

/// Parse a `YYYYMMDDTHHMMSS` (15-char, `T` at index 8) wall-clock string into a [`DateTime`] for a
/// range window bound. Any other shape is an error.
fn parse_window_bound(s: &str, what: &str) -> Result<DateTime, JsError> {
    let bytes = s.as_bytes();
    let bad = || {
        JsError::new(&format!(
            "loom_cal: {what} must be YYYYMMDDTHHMMSS, got {s:?}"
        ))
    };
    if bytes.len() != 15 || bytes[8] != b'T' {
        return Err(bad());
    }
    let digits = |range: std::ops::Range<usize>| -> Result<&str, JsError> {
        let part = &s[range];
        if part.bytes().all(|b| b.is_ascii_digit()) {
            Ok(part)
        } else {
            Err(bad())
        }
    };
    let num = |part: &str| -> Result<u32, JsError> { part.parse::<u32>().map_err(|_| bad()) };
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

/// Render a wall-clock [`DateTime`] as the `YYYYMMDDTHHMMSS` form used in the `cal_range` wire array.
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
fn records_cbor(records: Vec<Vec<u8>>) -> Result<Vec<u8>, JsError> {
    let items = records.into_iter().map(CborValue::Bytes).collect();
    cbor_encode(&CborValue::Array(items)).map_err(|e| JsError::new(&format!("cbor: {e}")))
}

/// Encode a canonical-CBOR array of text strings (the `list_collections`/`list_books`/
/// `list_mailboxes`/`get_flags` wire form).
fn strings_cbor(strings: Vec<String>) -> Result<Vec<u8>, JsError> {
    let items = strings.into_iter().map(CborValue::Text).collect();
    cbor_encode(&CborValue::Array(items)).map_err(|e| JsError::new(&format!("cbor: {e}")))
}

/// Decode a canonical-CBOR `Array(Text)` flag-set buffer into the owned strings `set_flags` expects.
fn flags_from_cbor(bytes: &[u8]) -> Result<Vec<String>, JsError> {
    let value = loom_codec::decode(bytes).map_err(|e| JsError::new(&format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(JsError::new("loom_mail: flags must be a CBOR array"));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match item {
            CborValue::Text(s) => out.push(s),
            _ => return Err(JsError::new("loom_mail: flag must be CBOR text")),
        }
    }
    Ok(out)
}

/// Reject empty, traversal, or separator queue stream names.
fn validate_stream_name(name: &str) -> Result<(), JsError> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(JsError::new(&format!("invalid stream name {name:?}")));
    }
    Ok(())
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

fn principal_kind_str(kind: PrincipalKind) -> &'static str {
    match kind {
        PrincipalKind::Root => "root",
        PrincipalKind::User => "user",
        PrincipalKind::Service => "service",
    }
}

fn parse_principal_kind(kind: &str) -> Result<PrincipalKind, JsError> {
    match kind {
        "root" => Ok(PrincipalKind::Root),
        "user" => Ok(PrincipalKind::User),
        "service" => Ok(PrincipalKind::Service),
        other => Err(JsError::new(&format!("unknown principal kind {other:?}"))),
    }
}

fn parse_external_credential_kind(kind: &str) -> Result<ExternalCredentialKind, JsError> {
    ExternalCredentialKind::parse(kind).map_err(le)
}

fn acl_effect_str(effect: AclEffect) -> &'static str {
    match effect {
        AclEffect::Allow => "allow",
        AclEffect::Deny => "deny",
    }
}

fn acl_right_str(right: AclRight) -> &'static str {
    match right {
        AclRight::Read => "read",
        AclRight::Write => "write",
        AclRight::Advance => "advance",
        AclRight::Merge => "merge",
        AclRight::Execute => "execute",
        AclRight::Admin => "admin",
    }
}

fn role_json(role: &IdentityRole) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&role.id.to_string()));
    out.push_str(",\"name\":");
    out.push_str(&json_string(&role.name));
    out.push_str(",\"enabled\":");
    out.push_str(if role.enabled { "true" } else { "false" });
    out.push('}');
    out
}

fn app_credential_json(credential: &loom_core::AppCredential) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&credential.id.to_string()));
    out.push_str(",\"principal\":");
    out.push_str(&json_string(&credential.principal.to_string()));
    out.push_str(",\"label\":");
    out.push_str(&json_string(&credential.label));
    out.push_str(",\"enabled\":");
    out.push_str(if credential.enabled { "true" } else { "false" });
    out.push('}');
    out
}

fn external_credential_json(credential: &ExternalCredential) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&credential.id.to_string()));
    out.push_str(",\"principal\":");
    out.push_str(&json_string(&credential.principal.to_string()));
    out.push_str(",\"kind\":");
    out.push_str(&json_string(credential.kind.as_str()));
    out.push_str(",\"label\":");
    out.push_str(&json_string(&credential.label));
    out.push_str(",\"issuer\":");
    out.push_str(&json_string(&credential.issuer));
    out.push_str(",\"subject\":");
    out.push_str(&json_string(&credential.subject));
    out.push_str(",\"material_digest\":");
    match credential.material_digest.as_deref() {
        Some(digest) => out.push_str(&json_string(digest)),
        None => out.push_str("null"),
    }
    out.push_str(",\"enabled\":");
    out.push_str(if credential.enabled { "true" } else { "false" });
    out.push('}');
    out
}

fn principal_json(principal: &Principal) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&principal.id.to_string()));
    out.push_str(",\"name\":");
    out.push_str(&json_string(&principal.name));
    out.push_str(",\"kind\":");
    out.push_str(&json_string(principal_kind_str(principal.kind)));
    out.push_str(",\"enabled\":");
    out.push_str(if principal.enabled { "true" } else { "false" });
    out.push_str(",\"has_passphrase\":");
    out.push_str(if principal.has_passphrase {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"roles\":[");
    for (idx, role) in principal.roles.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(&role.to_string()));
    }
    out.push(']');
    out.push('}');
    out
}

fn identity_list_json_inner(identity: &IdentityStore) -> String {
    let mut out = String::from("{\"authenticated_mode\":");
    out.push_str(if identity.authenticated_mode() {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"root\":");
    match identity.root_principal() {
        Some(root) => out.push_str(&json_string(&root.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"principals\":[");
    for (idx, principal) in identity.principals().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&principal_json(principal));
    }
    out.push_str("],\"roles\":[");
    for (idx, role) in identity.roles().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&role_json(role));
    }
    out.push_str("],\"app_credentials\":[");
    for (idx, credential) in identity.app_credentials().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&app_credential_json(credential));
    }
    out.push_str("],\"external_credentials\":[");
    for (idx, credential) in identity.external_credentials().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&external_credential_json(credential));
    }
    out.push_str("],\"public_keys\":[");
    for (idx, key) in identity.public_keys().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&identity_public_key_json(key));
    }
    out.push_str("]}");
    out
}

fn identity_public_key_json(key: &loom_core::IdentityPublicKey) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"id\":");
    out.push_str(&json_string(&key.id.to_string()));
    out.push_str(",\"principal\":");
    out.push_str(&json_string(&key.principal.to_string()));
    out.push_str(",\"label\":");
    out.push_str(&json_string(&key.label));
    out.push_str(",\"algorithm\":");
    out.push_str(&json_string(&key.algorithm));
    out.push_str(",\"public_key_hex\":");
    out.push_str(&json_string(&hex_bytes(&key.public_key)));
    out.push_str(",\"enabled\":");
    out.push_str(if key.enabled { "true" } else { "false" });
    out.push('}');
    out
}

fn acl_grant_json(grant: &AclGrant) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"effect\":");
    out.push_str(&json_string(acl_effect_str(grant.effect)));
    out.push_str(",\"subject\":");
    match grant.subject {
        AclSubject::Everyone => out.push_str(&json_string("*")),
        AclSubject::Principal(principal) => out.push_str(&json_string(&principal.to_string())),
        AclSubject::Role(role) => out.push_str(&json_string(&format!("role:{role}"))),
    }
    out.push_str(",\"subject_kind\":");
    match grant.subject {
        AclSubject::Everyone => out.push_str(&json_string("everyone")),
        AclSubject::Principal(_) => out.push_str(&json_string("principal")),
        AclSubject::Role(_) => out.push_str(&json_string("role")),
    }
    out.push_str(",\"workspace\":");
    match grant.workspace {
        Some(ns) => out.push_str(&json_string(&ns.to_string())),
        None => out.push_str("null"),
    }
    out.push_str(",\"domain\":");
    match grant.domain {
        Some(domain) => out.push_str(&json_string(domain.as_str())),
        None => out.push_str("null"),
    }
    out.push_str(",\"ref_glob\":");
    match grant.ref_glob.as_deref() {
        Some(ref_glob) => out.push_str(&json_string(ref_glob)),
        None => out.push_str("null"),
    }
    out.push_str(",\"rights\":[");
    for (idx, right) in grant.rights.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&json_string(acl_right_str(*right)));
    }
    out.push_str("],\"scopes\":[");
    for (idx, scope) in grant.scopes.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&acl_scope_json(scope));
    }
    out.push_str("],\"predicate\":");
    match &grant.predicate {
        Some(predicate) => {
            out.push_str("{\"language\":");
            out.push_str(&json_string(&predicate.language));
            out.push_str(",\"expression\":");
            out.push_str(&json_string(&predicate.expression));
            out.push('}');
        }
        None => out.push_str("null"),
    }
    out.push('}');
    out
}

fn acl_scope_json(scope: &AclScope) -> String {
    match scope {
        AclScope::All => String::from("{\"kind\":\"all\"}"),
        AclScope::Prefix { kind, prefix } => {
            let mut out = String::from("{\"kind\":");
            out.push_str(&json_string(acl_scope_kind_str(*kind)));
            out.push_str(",\"prefix_hex\":");
            out.push_str(&json_string(&hex_bytes(prefix)));
            out.push('}');
            out
        }
    }
}

fn acl_scope_kind_str(kind: AclScopeKind) -> &'static str {
    match kind {
        AclScopeKind::Ref => "ref",
        AclScopeKind::Collection => "collection",
        AclScopeKind::Path => "path",
        AclScopeKind::Key => "key",
        AclScopeKind::Table => "table",
        AclScopeKind::Exec => "exec",
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

fn decode_hex(value: &str) -> Result<Vec<u8>, JsError> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    if !value.len().is_multiple_of(2) {
        return Err(JsError::new("hex input must have an even number of digits"));
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        out.push((high << 4) | low);
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Result<u8, JsError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(JsError::new("hex input contains a non-hex digit")),
    }
}

fn acl_list_json_inner(acl: &AclStore) -> String {
    let mut out = String::from("[");
    for (idx, grant) in acl.grants().iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&acl_grant_json(grant));
    }
    out.push(']');
    out
}

fn protected_ref_policy_json(ref_name: &str, policy: &ProtectedRefPolicy) -> String {
    let mut out = String::new();
    out.push('{');
    out.push_str("\"ref\":");
    out.push_str(&json_string(ref_name));
    out.push_str(",\"fast_forward_only\":");
    out.push_str(if policy.fast_forward_only {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"signed_commits_required\":");
    out.push_str(if policy.signed_commits_required {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"signed_ref_advance_required\":");
    out.push_str(if policy.signed_ref_advance_required {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"required_review_count\":");
    out.push_str(&policy.required_review_count.to_string());
    out.push_str(",\"retention_lock\":");
    out.push_str(if policy.retention_lock {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"governance_lock\":");
    out.push_str(if policy.governance_lock {
        "true"
    } else {
        "false"
    });
    out.push('}');
    out
}

fn protected_ref_policies_json(policies: &[(String, ProtectedRefPolicy)]) -> String {
    let mut out = String::from("[");
    for (idx, (ref_name, policy)) in policies.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&protected_ref_policy_json(ref_name, policy));
    }
    out.push(']');
    out
}

fn parse_acl_subject(subject: &str) -> Result<AclSubject, JsError> {
    match subject {
        "*" | "everyone" => Ok(AclSubject::Everyone),
        role if role.starts_with("role:") => Ok(AclSubject::Role(
            WorkspaceId::parse(&role[5..]).map_err(le)?,
        )),
        other => Ok(AclSubject::Principal(
            WorkspaceId::parse(other).map_err(le)?,
        )),
    }
}

fn parse_acl_rights(mask: u32) -> Result<BTreeSet<AclRight>, JsError> {
    let mut rights = BTreeSet::new();
    if mask & 1 != 0 {
        rights.insert(AclRight::Read);
    }
    if mask & 2 != 0 {
        rights.insert(AclRight::Write);
    }
    if mask & 4 != 0 {
        rights.insert(AclRight::Advance);
    }
    if mask & 8 != 0 {
        rights.insert(AclRight::Merge);
    }
    if mask & 16 != 0 {
        rights.insert(AclRight::Execute);
    }
    if mask & 32 != 0 {
        rights.insert(AclRight::Admin);
    }
    if rights.is_empty() {
        return Err(JsError::new(
            "acl rights mask must include at least one right",
        ));
    }
    Ok(rights)
}

fn parse_acl_effect(effect: i32) -> Result<AclEffect, JsError> {
    match effect {
        0 => Ok(AclEffect::Allow),
        1 => Ok(AclEffect::Deny),
        other => Err(JsError::new(&format!("unknown acl effect {other}"))),
    }
}

fn parse_acl_scope_kind(value: &str) -> Result<AclScopeKind, JsError> {
    match value {
        "ref" => Ok(AclScopeKind::Ref),
        "collection" => Ok(AclScopeKind::Collection),
        "path" => Ok(AclScopeKind::Path),
        "key" => Ok(AclScopeKind::Key),
        "table" => Ok(AclScopeKind::Table),
        "exec" => Ok(AclScopeKind::Exec),
        other => Err(JsError::new(&format!("unknown acl scope kind {other:?}"))),
    }
}

fn parse_acl_scope(value: &str) -> Result<AclScope, JsError> {
    let Some((kind, prefix)) = value.split_once(':') else {
        return Err(JsError::new(&format!(
            "acl scope {value:?} must be KIND:PREFIX"
        )));
    };
    Ok(AclScope::Prefix {
        kind: parse_acl_scope_kind(kind)?,
        prefix: prefix.as_bytes().to_vec(),
    })
}

fn parse_acl_scopes(scopes: Vec<String>) -> Result<Vec<AclScope>, JsError> {
    if scopes.is_empty() {
        return Ok(vec![AclScope::All]);
    }
    scopes
        .iter()
        .map(|value| parse_acl_scope(value))
        .collect::<Result<Vec<_>, _>>()
}

fn optional_workspace_arg(
    loom: &Loom<FileStore>,
    workspace: Option<&str>,
) -> Result<Option<WorkspaceId>, JsError> {
    workspace
        .filter(|value| !value.is_empty())
        .map(|value| resolve_workspace_arg(loom, value))
        .transpose()
}

fn optional_domain_arg(domain: Option<&str>) -> Result<Option<AclDomain>, JsError> {
    domain
        .filter(|value| !value.is_empty())
        .map(AclDomain::parse)
        .transpose()
        .map_err(le)
}

fn acl_grant_from_args(
    loom: &Loom<FileStore>,
    effect: i32,
    subject: &str,
    workspace: Option<&str>,
    domain: Option<&str>,
    rights_mask: u32,
    ref_glob: Option<&str>,
    scopes: Vec<String>,
    predicate_cel: Option<&str>,
) -> Result<AclGrant, JsError> {
    Ok(AclGrant {
        subject: parse_acl_subject(subject)?,
        workspace: optional_workspace_arg(loom, workspace)?,
        domain: optional_domain_arg(domain)?,
        ref_glob: ref_glob
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        scopes: parse_acl_scopes(scopes)?,
        rights: parse_acl_rights(rights_mask)?,
        effect: parse_acl_effect(effect)?,
        predicate: optional_acl_predicate(predicate_cel)?,
    })
}

fn optional_acl_predicate(value: Option<&str>) -> Result<Option<AclPredicate>, JsError> {
    value
        .filter(|value| !value.is_empty())
        .map(AclPredicate::cel)
        .transpose()
        .map_err(le)
}

/// Parse the identity-profile selector: `"default"`/`"blake3"` -> BLAKE3 content
/// addresses; `"fips"`/`"sha256"` -> SHA-256.
fn parse_profile(s: &str) -> Result<Algo, JsError> {
    match s {
        "default" | "blake3" => Ok(Algo::Blake3),
        "fips" | "sha256" => Ok(Algo::Sha256),
        other => Err(JsError::new(&format!(
            "unknown identity profile {other:?} (expected `default`/`blake3` or `fips`/`sha256`)"
        ))),
    }
}

/// Fill `buf` with CSPRNG bytes. On wasm32 `getrandom` routes to the browser's
/// `crypto.getRandomValues` (the crate's `js` feature, enabled in Cargo.toml).
fn rng_fill(buf: &mut [u8]) -> Result<(), JsError> {
    getrandom::getrandom(buf).map_err(|e| JsError::new(&format!("rng: {e}")))
}

/// Resolve the DEK-wrap AEAD: an explicit `suite` name, or the profile default (AES-256-GCM under
/// FIPS/SHA-256, XChaCha20-Poly1305 otherwise) - matching the `loom` CLI and the node/python bindings.
fn wrap_suite(suite: Option<&str>, digest_algo: Algo) -> Result<Suite, JsError> {
    match suite {
        Some(s) => Suite::parse(s).map_err(le),
        None if digest_algo == Algo::Sha256 => Ok(Suite::Aes256Gcm),
        None => Ok(Suite::XChaCha20Poly1305),
    }
}

/// Validate a host-supplied raw KEK: it must be exactly 32 bytes.
fn parse_kek(kek: &[u8]) -> Result<[u8; KEY_LEN], JsError> {
    kek.try_into()
        .map_err(|_| JsError::new("KEK must be exactly 32 bytes"))
}

/// Build a fresh encryption descriptor + unlocked DEK session: a random DEK sealed under `spec` with
/// `suite` as the object AEAD. `with_salt` is true for a KDF-backed (passphrase) wrap and false for a
/// raw-KEK wrap (no KDF, empty salt). Returns the encoded `encryption_meta` and the live session. The
/// 24-byte wrap nonce covers both wrap AEADs ([`EncryptionMeta::create`] truncates to the suite's
/// nonce length: 24 for XChaCha20, 12 for AES-256-GCM).
fn build_encryption(
    spec: &KeySpec,
    suite: Suite,
    with_salt: bool,
) -> Result<(Vec<u8>, loom_core::keys::DekSession), JsError> {
    let mut dek = [0u8; KEY_LEN];
    let mut wrap_nonce = [0u8; 24];
    rng_fill(&mut dek)?;
    rng_fill(&mut wrap_nonce)?;
    let salt = if with_salt {
        let mut s = [0u8; 16];
        rng_fill(&mut s)?;
        s.to_vec()
    } else {
        Vec::new()
    };
    let (meta, session) =
        EncryptionMeta::create(spec, suite, salt, dek, wrap_nonce.to_vec()).map_err(le)?;
    Ok((meta.encode(), session))
}

/// Resolve-or-create the session workspace's SQL facet over a freshly opened `loom`, eagerly load the
/// database's rows, persist, and wrap it as a writable session. Shared by every writer constructor
/// ([`LoomSql::open`], [`LoomSql::open_encrypted`], [`LoomSql::open_with_kek`], [`LoomSql::create`],
/// [`LoomSql::create_with_kek`]).
fn finish_writer(mut loom: Loom<FileStore>, ns: &str, db: String) -> Result<LoomSql, JsError> {
    install_acl_predicate_evaluator(&mut loom);
    let id = derive_sql_ns_id(ns);
    let workspace = loom
        .registry_mut()
        .ensure_for_write(
            &WsSelector::Typed {
                ty: FacetKind::Sql,
                name: ns.to_string(),
            },
            id,
        )
        .map_err(le)?;
    // Eager in-memory base: the wasm `FileStore` is not `Send` (its backing erases to a
    // `Box<dyn BackingIo>` without `+ Send`), so it cannot back a streaming `RowIter`; load the
    // rows into the overlay up front (the wasm variant of the base, RAM-bound).
    let store = LoomSqlStore::load_eager_write(&loom, workspace, &db).map_err(le)?;
    save_loom(&mut loom).map_err(le)?;
    Ok(LoomSql {
        loom,
        ns: workspace,
        db,
        store,
        readonly: false,
        auth_principal: None,
        auth_passphrase: None,
    })
}

fn initialize_control_stores(loom: &mut Loom<FileStore>) -> Result<(), JsError> {
    let root = random_workspace_id()?;
    loom.store()
        .save_identity_store(&IdentityStore::new(root))
        .map_err(le)?;
    let mut acl = AclStore::new();
    acl.allow(AclSubject::Principal(root), None, None, [AclRight::Admin])
        .map_err(le)?;
    loom.store().save_acl_store(&acl).map_err(le)
}

/// Map a loom error into a JS error carrying its message.
fn le(e: loom_core::error::LoomError) -> JsError {
    JsError::new(&e.to_string())
}
/// Map a loom-compute exec error into a JS error carrying its message.
fn lce(e: loom_compute::ExecError) -> JsError {
    JsError::new(&e.to_string())
}
/// Map a rejected promise / DOM exception into a JS error.
fn je(e: JsValue) -> JsError {
    JsError::new(&format!("{e:?}"))
}
/// Map a JS value into a std::io::Error (for the BackingIo surface).
fn io(e: JsValue) -> std::io::Error {
    std::io::Error::other(format!("opfs: {e:?}"))
}

/// A [`BackingIo`] over an OPFS `FileSystemSyncAccessHandle`. The handle's read/write/getSize/
/// truncate/flush are synchronous (inside a Worker), so the engine's `block_on` runs over it with
/// no async bridge - the only async step is acquiring the handle in [`LoomSql::open`].
#[derive(Debug)]
struct OpfsBacking {
    handle: FileSystemSyncAccessHandle,
}

impl BackingIo for OpfsBacking {
    fn pread(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
        let opts = FileSystemReadWriteOptions::new();
        let mut filled = 0usize;
        while filled < buf.len() {
            opts.set_at((off + filled as u64) as f64);
            let n = self
                .handle
                .read_with_u8_array_and_options(&mut buf[filled..], &opts)
                .map_err(io)? as usize;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "opfs read past end of file",
                ));
            }
            filled += n;
        }
        Ok(())
    }

    fn pwrite(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()> {
        let opts = FileSystemReadWriteOptions::new();
        let mut written = 0usize;
        while written < buf.len() {
            opts.set_at((off + written as u64) as f64);
            let chunk = buf[written..].to_vec(); // the binding takes a fresh view
            let n = self
                .handle
                .write_with_u8_array_and_options(&chunk, &opts)
                .map_err(io)? as usize;
            if n == 0 {
                return Err(std::io::Error::other("opfs short write"));
            }
            written += n;
        }
        Ok(())
    }

    fn size(&self) -> std::io::Result<u64> {
        Ok(self.handle.get_size().map_err(io)? as u64)
    }

    fn grow(&mut self, len: u64) -> std::io::Result<()> {
        self.handle.truncate_with_f64(len as f64).map_err(io) // grows with zeros or truncates
    }

    fn fsync(&mut self) -> std::io::Result<()> {
        self.handle.flush().map_err(io)
    }
}

/// Acquire the OPFS sync-access handle for `path` (creating the file if absent). Async: the OPFS
/// directory/file lookups and `createSyncAccessHandle` return promises. Once held, the handle's I/O
/// is synchronous. Acquiring the handle is itself exclusive - it serves as the single-writer lock.
async fn acquire_handle(path: &str) -> Result<FileSystemSyncAccessHandle, JsError> {
    // A remote locator has no OPFS handle, so reject it before touching storage.
    crate::reject_remote_locator(path).map_err(|msg| JsError::new(&msg))?;
    let global: WorkerGlobalScope = js_sys::global()
        .dyn_into()
        .map_err(|_| JsError::new("OPFS requires a Web Worker context"))?;
    let storage = global.navigator().storage();
    let dir: FileSystemDirectoryHandle = JsFuture::from(storage.get_directory())
        .await
        .map_err(je)?
        .dyn_into()
        .map_err(je)?;
    let opts = FileSystemGetFileOptions::new();
    opts.set_create(true);
    let file: FileSystemFileHandle = JsFuture::from(dir.get_file_handle_with_options(path, &opts))
        .await
        .map_err(je)?
        .dyn_into()
        .map_err(je)?;
    let handle: FileSystemSyncAccessHandle = JsFuture::from(file.create_sync_access_handle())
        .await
        .map_err(|e| {
            JsError::new(&format!(
                "OPFS createSyncAccessHandle failed (browser without OPFS sync access, or the file \
                 is already open): {e:?}"
            ))
        })?
        .dyn_into()
        .map_err(je)?;
    Ok(handle)
}

/// Conformance vector over the real OPFS backing: acquire a throwaway sync handle at `path`, run the
/// deterministic SQL vector over it, and return the commit address. Must equal the native pin
/// ([`crate::conformance_expected`]) - proving the OPFS `BackingIo` round-trips bytes without
/// perturbing the canonical object encoding.
#[wasm_bindgen]
pub async fn conformance_digest_opfs(path: String) -> Result<String, JsError> {
    let handle = acquire_handle(&path).await?;
    let store = FileStore::with_backing(Box::new(OpfsBacking { handle }), true).map_err(le)?;
    loom_sql::conformance_commit_digest(store).map_err(le)
}

#[wasm_bindgen]
pub struct LoomStore {
    loom: Loom<FileStore>,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
}

#[wasm_bindgen]
impl LoomStore {
    pub async fn open(path: String) -> Result<LoomStore, JsError> {
        let handle = acquire_handle(&path).await?;
        let mut loom = loom_over_backing(Box::new(OpfsBacking { handle }), true).map_err(le)?;
        install_acl_predicate_evaluator(&mut loom);
        Ok(LoomStore {
            loom,
            auth_principal: None,
            auth_passphrase: None,
        })
    }

    pub async fn open_encrypted(path: String, passphrase: String) -> Result<LoomStore, JsError> {
        let handle = acquire_handle(&path).await?;
        let key = KeySpec::passphrase(passphrase);
        let mut loom =
            loom_over_backing_unlocked(Box::new(OpfsBacking { handle }), true, Some(&key))
                .map_err(le)?;
        install_acl_predicate_evaluator(&mut loom);
        Ok(LoomStore {
            loom,
            auth_principal: None,
            auth_passphrase: None,
        })
    }

    pub async fn open_with_kek(path: String, kek: Vec<u8>) -> Result<LoomStore, JsError> {
        let handle = acquire_handle(&path).await?;
        let key = KeySpec::raw_kek(parse_kek(&kek)?);
        let mut loom =
            loom_over_backing_unlocked(Box::new(OpfsBacking { handle }), true, Some(&key))
                .map_err(le)?;
        install_acl_predicate_evaluator(&mut loom);
        Ok(LoomStore {
            loom,
            auth_principal: None,
            auth_passphrase: None,
        })
    }

    pub async fn create(
        path: String,
        profile: String,
        suite: Option<String>,
        passphrase: Option<String>,
    ) -> Result<LoomStore, JsError> {
        let digest_algo = parse_profile(&profile)?;
        let handle = acquire_handle(&path).await?;
        let backing = Box::new(OpfsBacking { handle });
        let mut loom = match passphrase.filter(|p| !p.is_empty()) {
            None => loom_over_backing_profile(backing, true, digest_algo).map_err(le)?,
            Some(pass) => {
                let (meta, session) = build_encryption(
                    &KeySpec::passphrase(pass),
                    wrap_suite(suite.as_deref(), digest_algo)?,
                    true,
                )?;
                loom_over_backing_encrypted(backing, meta, session, digest_algo).map_err(le)?
            }
        };
        install_acl_predicate_evaluator(&mut loom);
        initialize_control_stores(&mut loom)?;
        save_loom(&mut loom).map_err(le)?;
        Ok(LoomStore {
            loom,
            auth_principal: None,
            auth_passphrase: None,
        })
    }

    pub async fn create_with_kek(
        path: String,
        profile: String,
        suite: Option<String>,
        kek: Vec<u8>,
    ) -> Result<LoomStore, JsError> {
        let digest_algo = parse_profile(&profile)?;
        let (meta, session) = build_encryption(
            &KeySpec::raw_kek(parse_kek(&kek)?),
            wrap_suite(suite.as_deref(), digest_algo)?,
            false,
        )?;
        let handle = acquire_handle(&path).await?;
        let mut loom = loom_over_backing_encrypted(
            Box::new(OpfsBacking { handle }),
            meta,
            session,
            digest_algo,
        )
        .map_err(le)?;
        install_acl_predicate_evaluator(&mut loom);
        initialize_control_stores(&mut loom)?;
        save_loom(&mut loom).map_err(le)?;
        Ok(LoomStore {
            loom,
            auth_principal: None,
            auth_passphrase: None,
        })
    }

    fn load_control_state(&mut self) -> Result<(IdentityStore, AclStore), JsError> {
        let identity = self
            .loom
            .store()
            .identity_store()
            .map_err(le)?
            .ok_or_else(|| JsError::new("identity store not initialized"))?;
        let acl = self
            .loom
            .store()
            .acl_store()
            .map_err(le)?
            .ok_or_else(|| JsError::new("acl store not initialized"))?;
        self.loom.set_identity_store(identity.clone());
        self.loom.set_acl_store(acl.clone());
        Ok((identity, acl))
    }

    fn authorize_global_admin(&mut self) -> Result<(IdentityStore, AclStore), JsError> {
        let (identity, acl) = self.load_control_state()?;
        let mut authorized_identity = identity.clone();
        if let (Some(principal), Some(passphrase)) = (&self.auth_principal, &self.auth_passphrase) {
            let principal = WorkspaceId::parse(principal).map_err(le)?;
            let session = authorized_identity
                .authenticate_passphrase(principal, passphrase, random_workspace_id()?.to_string())
                .map_err(le)?;
            self.loom.set_session(session.id);
        }
        self.loom.set_identity_store(authorized_identity);
        self.loom.set_acl_store(acl.clone());
        self.loom.authorize_global_admin().map_err(le)?;
        Ok((identity, acl))
    }

    pub fn authenticate_passphrase(
        &mut self,
        principal: String,
        principal_passphrase: String,
    ) -> Result<(), JsError> {
        let (mut identity, acl) = self.load_control_state()?;
        let principal = WorkspaceId::parse(&principal).map_err(le)?;
        let session = identity
            .authenticate_passphrase(
                principal,
                &principal_passphrase,
                random_workspace_id()?.to_string(),
            )
            .map_err(le)?;
        self.loom.set_session(session.id);
        self.loom.set_identity_store(identity);
        self.loom.set_acl_store(acl);
        self.auth_principal = Some(principal.to_string());
        self.auth_passphrase = Some(principal_passphrase);
        Ok(())
    }

    pub fn acl_list_json(&mut self) -> Result<String, JsError> {
        let (_, acl) = self.authorize_global_admin()?;
        Ok(acl_list_json_inner(&acl))
    }

    pub fn acl_grant_scoped(
        &mut self,
        effect: i32,
        subject: String,
        workspace: Option<String>,
        domain: Option<String>,
        rights_mask: u32,
        ref_glob: Option<String>,
        scopes: Vec<String>,
        predicate_cel: Option<String>,
    ) -> Result<(), JsError> {
        let (_, mut acl) = self.authorize_global_admin()?;
        let grant = acl_grant_from_args(
            &self.loom,
            effect,
            &subject,
            workspace.as_deref(),
            domain.as_deref(),
            rights_mask,
            ref_glob.as_deref(),
            scopes,
            predicate_cel.as_deref(),
        )?;
        acl.grant(grant).map_err(le)?;
        self.loom.store().save_acl_store(&acl).map_err(le)?;
        self.loom.set_acl_store(acl);
        Ok(())
    }

    pub fn acl_revoke_scoped(
        &mut self,
        effect: i32,
        subject: String,
        workspace: Option<String>,
        domain: Option<String>,
        rights_mask: u32,
        ref_glob: Option<String>,
        scopes: Vec<String>,
        predicate_cel: Option<String>,
    ) -> Result<bool, JsError> {
        let (_, mut acl) = self.authorize_global_admin()?;
        let grant = acl_grant_from_args(
            &self.loom,
            effect,
            &subject,
            workspace.as_deref(),
            domain.as_deref(),
            rights_mask,
            ref_glob.as_deref(),
            scopes,
            predicate_cel.as_deref(),
        )?;
        let removed = acl.revoke(&grant);
        if removed {
            self.loom.store().save_acl_store(&acl).map_err(le)?;
            self.loom.set_acl_store(acl);
        }
        Ok(removed)
    }

    pub fn protected_ref_list_json(&mut self, workspace: String) -> Result<String, JsError> {
        self.authorize_global_admin()?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(protected_ref_policies_json(
            &self.loom.protected_ref_policies(ns).map_err(le)?,
        ))
    }

    pub fn protected_ref_get_json(
        &mut self,
        workspace: String,
        ref_name: String,
    ) -> Result<String, JsError> {
        self.authorize_global_admin()?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        Ok(
            match self.loom.protected_ref_policy(ns, &ref_name).map_err(le)? {
                Some(policy) => protected_ref_policy_json(&ref_name, &policy),
                None => "null".to_string(),
            },
        )
    }

    pub fn protected_ref_set(
        &mut self,
        workspace: String,
        ref_name: String,
        fast_forward_only: bool,
        signed_commits_required: bool,
        signed_ref_advance_required: bool,
        required_review_count: u32,
        retention_lock: bool,
        governance_lock: bool,
    ) -> Result<(), JsError> {
        self.authorize_global_admin()?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        self.loom
            .set_protected_ref_policy(
                ns,
                &ref_name,
                ProtectedRefPolicy {
                    fast_forward_only,
                    signed_commits_required,
                    signed_ref_advance_required,
                    required_review_count,
                    retention_lock,
                    governance_lock,
                },
            )
            .map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    pub fn protected_ref_remove(
        &mut self,
        workspace: String,
        ref_name: String,
    ) -> Result<bool, JsError> {
        self.authorize_global_admin()?;
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let removed = self
            .loom
            .remove_protected_ref_policy(ns, &ref_name)
            .map_err(le)?;
        if removed {
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(removed)
    }

    pub fn exec_cbor(&mut self, request: Vec<u8>) -> Result<Vec<u8>, JsError> {
        let out = loom_compute::execute_cbor(&mut self.loom, &request).map_err(lce)?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(out)
    }
}

/// An OPFS-backed SQL session over a workspace SQL facet in a `.loom`. Unlike the native
/// per-op sessions, this holds the loom open for its lifetime: acquiring the OPFS sync handle is an
/// async, exclusive, one-time step, so re-acquiring per call is not viable. `open` is async (it
/// awaits the handle); `exec` / `commit` are synchronous (the handle's I/O is sync in the Worker).
#[wasm_bindgen]
pub struct LoomSql {
    loom: Loom<FileStore>,
    ns: WorkspaceId,
    db: String,
    store: LoomSqlStore,
    auth_principal: Option<String>,
    auth_passphrase: Option<String>,
    // A writer (held the exclusive OPFS sync handle via `open`) or a lock-free read-only snapshot
    // (`open_read`). A reader never persists and rejects `commit`.
    readonly: bool,
}

#[wasm_bindgen]
impl LoomSql {
    /// Open `path` in OPFS and start a SQL session over workspace `ns`'s SQL facet (created if
    /// absent), database `db`. Hard-fails with a clear error if OPFS sync access is unavailable.
    pub async fn open(path: String, ns: String, db: String) -> Result<LoomSql, JsError> {
        let handle = acquire_handle(&path).await?;
        let loom = loom_over_backing(Box::new(OpfsBacking { handle }), true).map_err(le)?;
        finish_writer(loom, &ns, db)
    }

    /// Open an **existing encrypted** loom in OPFS, unlocking it with a `passphrase`
    /// (passphrase -> KDF -> KEK -> unwrap DEK). The KDF (Argon2id or, under the FIPS
    /// profile, PBKDF2-HMAC-SHA-256) is recorded in the store's wrap descriptor, so the same
    /// passphrase unlocks regardless of profile. Then starts a SQL session like [`open`].
    pub async fn open_encrypted(
        path: String,
        ns: String,
        db: String,
        passphrase: String,
    ) -> Result<LoomSql, JsError> {
        let handle = acquire_handle(&path).await?;
        let key = KeySpec::passphrase(passphrase);
        let loom = loom_over_backing_unlocked(Box::new(OpfsBacking { handle }), true, Some(&key))
            .map_err(le)?;
        finish_writer(loom, &ns, db)
    }

    /// Open an **existing encrypted** loom in OPFS with a host-supplied 256-bit `kek` that directly
    /// unwraps the DEK (keychain / Secure Enclave / passkey-PRF / KMS material; no KDF). `kek`
    /// must be exactly 32 bytes. Then starts a SQL session like [`open`].
    pub async fn open_with_kek(
        path: String,
        ns: String,
        db: String,
        kek: Vec<u8>,
    ) -> Result<LoomSql, JsError> {
        let key = KeySpec::raw_kek(parse_kek(&kek)?);
        let handle = acquire_handle(&path).await?;
        let loom = loom_over_backing_unlocked(Box::new(OpfsBacking { handle }), true, Some(&key))
            .map_err(le)?;
        finish_writer(loom, &ns, db)
    }

    /// Create a **fresh** loom in OPFS under an identity `profile` (`"default"`/`"blake3"` or
    /// `"fips"`/`"sha256"`) and start a SQL session over it. A non-empty `passphrase`
    /// makes an encrypted store: the random DEK is wrapped under the passphrase with `suite`,
    /// or the profile default, as the object AEAD; a null/empty `passphrase` makes an
    /// unencrypted store. Fails (`ALREADY_EXISTS`) if the OPFS file already holds a store.
    pub async fn create(
        path: String,
        ns: String,
        db: String,
        profile: String,
        suite: Option<String>,
        passphrase: Option<String>,
    ) -> Result<LoomSql, JsError> {
        let digest_algo = parse_profile(&profile)?;
        let handle = acquire_handle(&path).await?;
        let backing = Box::new(OpfsBacking { handle });
        let mut loom = match passphrase.filter(|p| !p.is_empty()) {
            None => loom_over_backing_profile(backing, true, digest_algo).map_err(le)?,
            Some(pass) => {
                let (meta, session) = build_encryption(
                    &KeySpec::passphrase(pass),
                    wrap_suite(suite.as_deref(), digest_algo)?,
                    true,
                )?;
                loom_over_backing_encrypted(backing, meta, session, digest_algo).map_err(le)?
            }
        };
        initialize_control_stores(&mut loom)?;
        finish_writer(loom, &ns, db)
    }

    /// Create a **fresh encrypted** loom in OPFS whose DEK is wrapped under a host-supplied 256-bit
    /// `kek`. `profile` selects the content-address algorithm and `suite` the object AEAD
    /// (defaulting per profile). `kek` must be exactly 32 bytes.
    pub async fn create_with_kek(
        path: String,
        ns: String,
        db: String,
        profile: String,
        suite: Option<String>,
        kek: Vec<u8>,
    ) -> Result<LoomSql, JsError> {
        let digest_algo = parse_profile(&profile)?;
        let (meta, session) = build_encryption(
            &KeySpec::raw_kek(parse_kek(&kek)?),
            wrap_suite(suite.as_deref(), digest_algo)?,
            false,
        )?;
        let handle = acquire_handle(&path).await?;
        let mut loom = loom_over_backing_encrypted(
            Box::new(OpfsBacking { handle }),
            meta,
            session,
            digest_algo,
        )
        .map_err(le)?;
        initialize_control_stores(&mut loom)?;
        finish_writer(loom, &ns, db)
    }

    /// Open a lock-free, read-only snapshot from the raw `.loom` bytes - the browser counterpart of
    /// the native lock-free reader (`FileStore::open_read`). The worker reads the OPFS file
    /// through the async File API (`getFile().arrayBuffer()`) and passes the bytes here; this takes
    /// **no** OPFS sync-access handle, so it coexists with a writer in another tab and never blocks.
    /// The snapshot is whatever the writer last flushed (the crash-consistent committed state).
    /// `exec` runs queries but never persists; `commit` is rejected.
    pub fn open_read(bytes: Vec<u8>, ns: String, db: String) -> Result<LoomSql, JsError> {
        let backing = MemoryBacking::from_bytes(bytes);
        let mut loom = loom_over_backing(Box::new(backing), false).map_err(le)?;
        install_acl_predicate_evaluator(&mut loom);
        let id = derive_sql_ns_id(&ns);
        // Eager in-memory base (see `open`): read the snapshot's rows into the overlay up front.
        let store = LoomSqlStore::load_eager_read(&loom, id, &db).map_err(le)?;
        Ok(LoomSql {
            loom,
            ns: id,
            db,
            store,
            readonly: true,
            auth_principal: None,
            auth_passphrase: None,
        })
    }

    /// Create a workspace in the open OPFS loom and return its UUID string.
    pub fn workspace_create(
        &mut self,
        name: Option<String>,
        domain: Option<String>,
    ) -> Result<String, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let id = random_workspace_id()?;
        let name = name.as_deref().filter(|value| !value.is_empty());
        let ns = match facet.as_deref().filter(|value| !value.is_empty()) {
            Some(facet) => self
                .loom
                .registry_mut()
                .create(FacetKind::parse(facet).map_err(le)?, name, id)
                .map_err(le)?,
            None => self
                .loom
                .registry_mut()
                .create_workspace(name, id)
                .map_err(le)?,
        };
        save_loom(&mut self.loom).map_err(le)?;
        Ok(ns.to_string())
    }

    /// List workspaces as JSON records with id, name, facets, and head.
    pub fn workspace_list_json(&self) -> String {
        workspace_list_json_inner(&self.loom)
    }

    /// Rename a workspace selected by UUID or current name.
    pub fn workspace_rename(&mut self, workspace: String, new_name: String) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        self.loom.registry_mut().rename(ns, &new_name).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    /// Delete a workspace selected by UUID or name.
    pub fn workspace_delete(&mut self, workspace: String) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        self.loom.registry_mut().delete(ns).map_err(le)?;
        save_loom(&mut self.loom).map_err(le)
    }

    fn load_control_state(&mut self) -> Result<(IdentityStore, AclStore), JsError> {
        let identity = self
            .loom
            .store()
            .identity_store()
            .map_err(le)?
            .ok_or_else(|| JsError::new("identity store not initialized"))?;
        let acl = self
            .loom
            .store()
            .acl_store()
            .map_err(le)?
            .ok_or_else(|| JsError::new("acl store not initialized"))?;
        self.loom.set_identity_store(identity.clone());
        self.loom.set_acl_store(acl.clone());
        Ok((identity, acl))
    }

    fn authorize_global_admin(&mut self) -> Result<(IdentityStore, AclStore), JsError> {
        let (identity, acl) = self.load_control_state()?;
        let mut authorized_identity = identity.clone();
        if let (Some(principal), Some(passphrase)) = (&self.auth_principal, &self.auth_passphrase) {
            let principal = WorkspaceId::parse(principal).map_err(le)?;
            let session = authorized_identity
                .authenticate_passphrase(principal, passphrase, random_workspace_id()?.to_string())
                .map_err(le)?;
            self.loom.set_session(session.id);
        }
        self.loom.set_identity_store(authorized_identity);
        self.loom.set_acl_store(acl.clone());
        self.loom.authorize_global_admin().map_err(le)?;
        Ok((identity, acl))
    }

    pub fn authenticate_passphrase(
        &mut self,
        principal: String,
        principal_passphrase: String,
    ) -> Result<(), JsError> {
        let (mut identity, acl) = self.load_control_state()?;
        let principal = WorkspaceId::parse(&principal).map_err(le)?;
        let session = identity
            .authenticate_passphrase(
                principal,
                &principal_passphrase,
                random_workspace_id()?.to_string(),
            )
            .map_err(le)?;
        self.loom.set_session(session.id);
        self.loom.set_identity_store(identity);
        self.loom.set_acl_store(acl);
        self.auth_principal = Some(principal.to_string());
        self.auth_passphrase = Some(principal_passphrase);
        Ok(())
    }

    pub fn identity_list_json(&mut self) -> Result<String, JsError> {
        let (identity, _) = self.authorize_global_admin()?;
        Ok(identity_list_json_inner(&identity))
    }

    pub fn identity_add_principal(
        &mut self,
        principal_handle: String,
        name: String,
        kind: String,
    ) -> Result<String, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let (mut identity, _) = self.authorize_global_admin()?;
        let id = random_workspace_id()?;
        identity
            .add_principal_with_handle(id, principal_handle, name, parse_principal_kind(&kind)?)
            .map_err(le)?;
        self.loom
            .store()
            .save_identity_store(&identity)
            .map_err(le)?;
        self.loom.set_identity_store(identity);
        Ok(id.to_string())
    }

    pub fn identity_rename_principal_handle(
        &mut self,
        principal: String,
        principal_handle: String,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let (mut identity, _) = self.authorize_global_admin()?;
        let principal = WorkspaceId::parse(&principal).map_err(le)?;
        identity
            .rename_principal_handle(principal, principal_handle)
            .map_err(le)?;
        self.loom
            .store()
            .save_identity_store(&identity)
            .map_err(le)?;
        self.loom.set_identity_store(identity);
        Ok(())
    }

    pub fn identity_set_passphrase(
        &mut self,
        principal: String,
        principal_passphrase: String,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let (mut identity, _) = self.authorize_global_admin()?;
        let principal = WorkspaceId::parse(&principal).map_err(le)?;
        let mut salt = [0u8; 16];
        rng_fill(&mut salt)?;
        identity
            .set_passphrase(principal, &principal_passphrase, &salt)
            .map_err(le)?;
        self.loom
            .store()
            .save_identity_store(&identity)
            .map_err(le)?;
        self.loom.set_identity_store(identity);
        Ok(())
    }

    pub fn identity_remove_principal(&mut self, principal: String) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let (mut identity, _) = self.authorize_global_admin()?;
        let principal = WorkspaceId::parse(&principal).map_err(le)?;
        identity.remove_principal(principal).map_err(le)?;
        self.loom
            .store()
            .save_identity_store(&identity)
            .map_err(le)?;
        self.loom.set_identity_store(identity);
        Ok(())
    }

    pub fn identity_assign_role(&mut self, principal: String, role: String) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let (mut identity, _) = self.authorize_global_admin()?;
        let principal = WorkspaceId::parse(&principal).map_err(le)?;
        let role = WorkspaceId::parse(&role).map_err(le)?;
        identity.assign_role(principal, role).map_err(le)?;
        self.loom
            .store()
            .save_identity_store(&identity)
            .map_err(le)?;
        self.loom.set_identity_store(identity);
        Ok(())
    }

    pub fn identity_revoke_role(
        &mut self,
        principal: String,
        role: String,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let (mut identity, _) = self.authorize_global_admin()?;
        let principal = WorkspaceId::parse(&principal).map_err(le)?;
        let role = WorkspaceId::parse(&role).map_err(le)?;
        let removed = identity.revoke_role(principal, role).map_err(le)?;
        self.loom
            .store()
            .save_identity_store(&identity)
            .map_err(le)?;
        self.loom.set_identity_store(identity);
        Ok(removed)
    }

    pub fn identity_create_external_credential(
        &mut self,
        principal: String,
        kind: String,
        label: String,
        issuer: String,
        subject: String,
        material_digest: Option<String>,
    ) -> Result<String, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let (mut identity, _) = self.authorize_global_admin()?;
        let principal = WorkspaceId::parse(&principal).map_err(le)?;
        let kind = parse_external_credential_kind(&kind)?;
        let id = random_workspace_id()?;
        identity
            .create_external_credential(
                principal,
                ExternalCredentialSpec {
                    id,
                    kind,
                    label,
                    issuer,
                    subject,
                    material_digest,
                },
            )
            .map_err(le)?;
        self.loom
            .store()
            .save_identity_store(&identity)
            .map_err(le)?;
        self.loom.set_identity_store(identity);
        Ok(id.to_string())
    }

    pub fn identity_revoke_external_credential(
        &mut self,
        credential: String,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let (mut identity, _) = self.authorize_global_admin()?;
        let credential = WorkspaceId::parse(&credential).map_err(le)?;
        identity
            .revoke_external_credential(credential)
            .map_err(le)?;
        self.loom
            .store()
            .save_identity_store(&identity)
            .map_err(le)?;
        self.loom.set_identity_store(identity);
        Ok(())
    }

    pub fn identity_add_public_key(
        &mut self,
        principal: String,
        label: String,
        algorithm: String,
        public_key_hex: String,
    ) -> Result<String, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let (mut identity, _) = self.authorize_global_admin()?;
        let principal = WorkspaceId::parse(&principal).map_err(le)?;
        let id = random_workspace_id()?;
        let public_key = decode_hex(&public_key_hex)?;
        identity
            .add_public_key(
                principal,
                IdentityPublicKeySpec {
                    id,
                    label,
                    algorithm,
                    public_key,
                },
            )
            .map_err(le)?;
        self.loom
            .store()
            .save_identity_store(&identity)
            .map_err(le)?;
        self.loom.set_identity_store(identity);
        Ok(id.to_string())
    }

    pub fn identity_revoke_public_key(&mut self, key: String) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let (mut identity, _) = self.authorize_global_admin()?;
        let key = WorkspaceId::parse(&key).map_err(le)?;
        identity.revoke_public_key(key).map_err(le)?;
        self.loom
            .store()
            .save_identity_store(&identity)
            .map_err(le)?;
        self.loom.set_identity_store(identity);
        Ok(())
    }

    pub fn acl_list_json(&mut self) -> Result<String, JsError> {
        let (_, acl) = self.authorize_global_admin()?;
        Ok(acl_list_json_inner(&acl))
    }

    pub fn acl_grant(
        &mut self,
        effect: i32,
        subject: String,
        workspace: Option<String>,
        domain: Option<String>,
        rights_mask: u32,
        predicate_cel: Option<String>,
    ) -> Result<(), JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let (_, mut acl) = self.authorize_global_admin()?;
        let grant = acl_grant_from_args(
            &self.loom,
            effect,
            &subject,
            workspace.as_deref(),
            domain.as_deref(),
            rights_mask,
            None,
            Vec::new(),
            predicate_cel.as_deref(),
        )?;
        acl.grant(grant).map_err(le)?;
        self.loom.store().save_acl_store(&acl).map_err(le)?;
        self.loom.set_acl_store(acl);
        Ok(())
    }

    pub fn acl_revoke(
        &mut self,
        effect: i32,
        subject: String,
        workspace: Option<String>,
        domain: Option<String>,
        rights_mask: u32,
        predicate_cel: Option<String>,
    ) -> Result<bool, JsError> {
        if self.readonly {
            return Err(JsError::new("this session is a read-only snapshot"));
        }
        let (_, mut acl) = self.authorize_global_admin()?;
        let grant = acl_grant_from_args(
            &self.loom,
            effect,
            &subject,
            workspace.as_deref(),
            domain.as_deref(),
            rights_mask,
            None,
            Vec::new(),
            predicate_cel.as_deref(),
        )?;
        let removed = acl.revoke(&grant);
        if removed {
            self.loom.store().save_acl_store(&acl).map_err(le)?;
            self.loom.set_acl_store(acl);
        }
        Ok(removed)
    }

    pub fn sql_read_table(&self, workspace: String, table: String) -> Result<Vec<u8>, JsError> {
        let ns = resolve_typed_ns(&self.loom, "sql", &workspace)?;
        let table = self.loom.read_table(ns, &table).map_err(le)?;
        result_cbor::table_cbor(&table).map_err(le)
    }

    pub fn sql_read_table_at(
        &self,
        workspace: String,
        table: String,
        commit: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_typed_ns(&self.loom, "sql", &workspace)?;
        let commit = Digest::parse(&commit).map_err(le)?;
        let table = self.loom.read_table_at(ns, &table, commit).map_err(le)?;
        result_cbor::table_cbor(&table).map_err(le)
    }

    pub fn sql_index_scan(
        &self,
        workspace: String,
        table: String,
        index: String,
        prefix: Vec<u8>,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_typed_ns(&self.loom, "sql", &workspace)?;
        let values = lookup_cbor::values_from_cbor(&prefix).map_err(le)?;
        let rows = self
            .loom
            .index_scan(ns, &table, &index, &values)
            .map_err(le)?;
        let schema = self
            .loom
            .read_table(ns, &table)
            .map_err(le)?
            .schema()
            .clone();
        result_cbor::rows_cbor(&schema, &rows).map_err(le)
    }

    pub fn sql_index_scan_at(
        &self,
        workspace: String,
        table: String,
        index: String,
        prefix: Vec<u8>,
        commit: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_typed_ns(&self.loom, "sql", &workspace)?;
        let values = lookup_cbor::values_from_cbor(&prefix).map_err(le)?;
        let commit = Digest::parse(&commit).map_err(le)?;
        let rows = self
            .loom
            .index_scan_at(ns, &table, &index, &values, commit)
            .map_err(le)?;
        let schema = self
            .loom
            .read_table_at(ns, &table, commit)
            .map_err(le)?
            .schema()
            .clone();
        result_cbor::rows_cbor(&schema, &rows).map_err(le)
    }

    pub fn sql_blame(
        &self,
        workspace: String,
        branch: String,
        table: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_typed_ns(&self.loom, "sql", &workspace)?;
        let rows = self.loom.blame_table(ns, &branch, &table).map_err(le)?;
        result_cbor::blame_cbor(&rows).map_err(le)
    }

    pub fn sql_diff(
        &self,
        workspace: String,
        table: String,
        from_commit: String,
        to_commit: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_typed_ns(&self.loom, "sql", &workspace)?;
        let from = Digest::parse(&from_commit).map_err(le)?;
        let to = Digest::parse(&to_commit).map_err(le)?;
        let diffs = self.loom.diff_table(ns, &table, from, to).map_err(le)?;
        result_cbor::diff_cbor(&diffs).map_err(le)
    }

    pub fn sql_table_diff(
        &self,
        workspace: String,
        table: String,
        from_commit: String,
        to_commit: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_typed_ns(&self.loom, "sql", &workspace)?;
        let from = Digest::parse(&from_commit).map_err(le)?;
        let to = Digest::parse(&to_commit).map_err(le)?;
        let records = self
            .loom
            .diff_table_records(ns, &table, from, to)
            .map_err(le)?;
        result_cbor::table_diff_cbor(&records).map_err(le)
    }

    /// Workspace/entry-level blame for `branch` (which commit last set each path), as canonical
    /// CBOR (`{ kind: "PathBlame", paths: [...] }`). Mirrors the C ABI `loom_vcs_blame`.
    pub fn vcs_blame(&self, workspace: String, branch: String) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let paths = self.loom.blame(ns, &branch).map_err(le)?;
        result_cbor::path_blame_cbor(&paths).map_err(le)
    }

    /// Cross-facet structural diff between commits as the raw `LMDIFF` canonical-CBOR envelope.
    pub fn vcs_diff(
        &self,
        workspace: String,
        from_commit: String,
        to_commit: String,
    ) -> Result<Vec<u8>, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let from = Digest::parse(&from_commit).map_err(le)?;
        let to = Digest::parse(&to_commit).map_err(le)?;
        self.loom.diff_commits(ns, from, to).map_err(le)
    }

    /// Subscribe to workspace history changes and return an opaque watch cursor string.
    pub fn watch_subscribe(
        &self,
        workspace: String,
        branch: String,
        facet: Option<String>,
        path_prefix: Option<String>,
        change_kinds: Vec<String>,
        from_commit: Option<String>,
    ) -> Result<String, JsError> {
        let ns = resolve_workspace_arg(&self.loom, &workspace)?;
        let mut selector = WatchSelector::new(ns, &branch).map_err(le)?;
        if let Some(facet) = facet.as_deref().filter(|value| !value.is_empty()) {
            selector = selector.with_facet(FacetKind::parse(facet).map_err(le)?);
        }
        if let Some(path_prefix) = path_prefix.filter(|value| !value.is_empty()) {
            selector = selector.with_path_prefix(path_prefix);
        }
        for kind in parse_watch_change_kinds(change_kinds)? {
            selector = selector.with_change_kind(kind);
        }
        let from = from_commit
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(Digest::parse)
            .transpose()
            .map_err(le)?;
        Ok(self
            .loom
            .watch_subscribe(&selector, from)
            .map_err(le)?
            .encode())
    }

    /// Poll an opaque watch cursor and return a canonical-CBOR `loom.watch.batch.v1` batch.
    pub fn watch_poll(&self, cursor: String, max: u32) -> Result<Vec<u8>, JsError> {
        let cursor = WatchCursor::decode(&cursor).map_err(le)?;
        let batch = self.loom.watch_poll(&cursor, max as usize).map_err(le)?;
        watch_batch_to_cbor(&batch).map_err(le)
    }

    /// Run one or more `;`-separated SQL statements and return **typed** results: a JS array of
    /// statement objects (`{ kind, ... }`). A `select` carries `columns` and `rows` of idiomatic
    /// cells - `BigInt` for 64/128-bit integers, `number` for <=32-bit integers and floats,
    /// `Uint8Array` for bytes, `string` for text, and `{ mantissa: BigInt, scale }` for an exact
    /// decimal. A read-only session does not persist. For raw bytes use `exec_bytes`; for the JSON
    /// debug form use `exec_json`.
    pub fn exec(&mut self, sql: String) -> Result<JsValue, JsError> {
        let bytes = self.store.exec_cbor(&sql).map_err(le)?;
        self.persist_if_writable()?;
        let payload = result_view::decode(&bytes).map_err(le)?;
        statements_to_js(&payload)
    }

    /// Run SQL; returns a JSON array of the result payloads (debug/admin form, rendered from the
    /// canonical CBOR - not the type-faithful API; use `exec`). A read-only session does not persist.
    pub fn exec_json(&mut self, sql: String) -> Result<String, JsError> {
        let json = self.store.exec_json(&sql).map_err(le)?;
        self.persist_if_writable()?;
        Ok(json)
    }

    /// Persist + save the staged state unless this is a read-only snapshot **or** an explicit SQL
    /// transaction is open. Because the OPFS session holds the loom open for its lifetime, it is
    /// itself the transaction/batch scope: `BEGIN`/`COMMIT`/`ROLLBACK` span `exec`
    /// calls, and changes are persisted only once the transaction resolves (or for a plain
    /// autocommit statement). Closing the session with a transaction still open never persisted its
    /// changes, so it is an implicit rollback. Shared by the exec paths.
    fn persist_if_writable(&mut self) -> Result<(), JsError> {
        if self.readonly && self.store.is_dirty() {
            return Err(JsError::new(
                "sql exec is read-only on this session; open a writable session for statements that mutate state",
            ));
        }
        if !self.readonly && !self.store.in_transaction() {
            self.store
                .persist(&mut self.loom, self.ns, &self.db)
                .map_err(le)?;
            save_loom(&mut self.loom).map_err(le)?;
        }
        Ok(())
    }

    /// Run SQL; returns the result payloads as canonical CBOR bytes - a JS
    /// `Uint8Array`. A read-only session does not persist. (wasm is single-threaded; off-main-thread
    /// execution is the integrator's Web Worker, not a per-call Promise.)
    pub fn exec_bytes(&mut self, sql: String) -> Result<Vec<u8>, JsError> {
        let bytes = self.store.exec_cbor(&sql).map_err(le)?;
        self.persist_if_writable()?;
        Ok(bytes)
    }

    /// Run a `SELECT` and return its rows as a JS array of typed cell arrays (the streaming
    /// form). A JS array is natively iterable, so `for (const row of db.query(sql))` walks the
    /// rows; each cell uses the same idiomatic mapping as `exec`. A read-only session does not
    /// persist; a non-`SELECT` yields an empty array.
    pub fn query(&mut self, sql: String) -> Result<JsValue, JsError> {
        self.loom
            .authorize(self.ns, FacetKind::Sql, AclRight::Read)
            .map_err(le)?;
        let rows = self.store.select_rows(&sql).map_err(le)?;
        if self.store.is_dirty() {
            return Err(JsError::new(
                "sql query is read-only; use exec for statements that mutate state",
            ));
        }
        let arr = Array::new();
        for row in &rows {
            arr.push(&row_to_js(row)?);
        }
        Ok(arr.into())
    }

    /// Commit the staged database state; returns the new commit's content address (`"algo:hex"`).
    pub fn commit(&mut self, message: String, author: String) -> Result<String, JsError> {
        if self.readonly {
            return Err(JsError::new(
                "this session is a read-only snapshot (another tab holds the writer); reopen for \
                 writing once the writer is released",
            ));
        }
        let digest = self
            .loom
            .commit(self.ns, &author, &message, now_ms())
            .map_err(le)?;
        save_loom(&mut self.loom).map_err(le)?;
        Ok(digest.to_string())
    }

    // --- Document facet (0020) ---

    // --- Time-series facet (0022) ---

    // --- Ledger facet (0018) ---

    // --- Calendar facet (0037) ---

    // --- Contacts facet (0038) ---

    // --- Mail facet (0039) ---
}

// --- Typed result mapping (the primary `exec` API): decoded ResultPayload -> idiomatic JS. ---

/// Set a string-keyed property on a JS object.
fn set(o: &Object, k: &str, v: &JsValue) -> Result<(), JsError> {
    Reflect::set(o, &JsValue::from_str(k), v)
        .map(|_| ())
        .map_err(je)
}

/// Build the typed JS result array from a decoded payload. SQL `exec` yields statements.
fn statements_to_js(payload: &ResultPayload) -> Result<JsValue, JsError> {
    let stmts = match payload {
        ResultPayload::Statements(s) => s,
        ResultPayload::Reader(_) => return Err(JsError::new("exec returned a reader result")),
    };
    let arr = Array::new();
    for s in stmts {
        arr.push(&statement_to_js(s)?);
    }
    Ok(arr.into())
}

/// One SQL statement result as a `{ kind, ... }` JS object.
fn statement_to_js(s: &Statement) -> Result<JsValue, JsError> {
    let o = Object::new();
    match s {
        Statement::Select { labels, rows } => {
            set(&o, "kind", &JsValue::from_str("select"))?;
            let cols = Array::new();
            for name in labels {
                let c = Object::new();
                set(&c, "name", &JsValue::from_str(name))?;
                cols.push(&JsValue::from(c));
            }
            set(&o, "columns", &JsValue::from(cols))?;
            let rs = Array::new();
            for r in rows {
                rs.push(&row_to_js(r)?);
            }
            set(&o, "rows", &JsValue::from(rs))?;
        }
        Statement::SelectMap(rows) => {
            set(&o, "kind", &JsValue::from_str("selectMap"))?;
            let rs = Array::new();
            for m in rows {
                rs.push(&map_to_js(m)?);
            }
            set(&o, "rows", &JsValue::from(rs))?;
        }
        Statement::ShowColumns(cols) => {
            set(&o, "kind", &JsValue::from_str("showColumns"))?;
            let cs = Array::new();
            for c in cols {
                let co = Object::new();
                set(&co, "name", &JsValue::from_str(&c.name))?;
                set(&co, "type", &JsValue::from_str(&c.type_name))?;
                cs.push(&JsValue::from(co));
            }
            set(&o, "columns", &JsValue::from(cs))?;
        }
        Statement::Insert(n) => count_obj(&o, "insert", *n)?,
        Statement::Delete(n) => count_obj(&o, "delete", *n)?,
        Statement::Update(n) => count_obj(&o, "update", *n)?,
        Statement::DropTable(n) => count_obj(&o, "dropTable", *n)?,
        Statement::Create => set(&o, "kind", &JsValue::from_str("create"))?,
        Statement::DropFunction => set(&o, "kind", &JsValue::from_str("dropFunction"))?,
        Statement::AlterTable => set(&o, "kind", &JsValue::from_str("alterTable"))?,
        Statement::CreateIndex => set(&o, "kind", &JsValue::from_str("createIndex"))?,
        Statement::DropIndex => set(&o, "kind", &JsValue::from_str("dropIndex"))?,
        Statement::StartTransaction => set(&o, "kind", &JsValue::from_str("startTransaction"))?,
        Statement::Commit => set(&o, "kind", &JsValue::from_str("commit"))?,
        Statement::Rollback => set(&o, "kind", &JsValue::from_str("rollback"))?,
        Statement::ShowVariable(sv) => {
            set(&o, "kind", &JsValue::from_str("showVariable"))?;
            match sv {
                ShowVariable::Tables(v) => {
                    set(&o, "variable", &JsValue::from_str("tables"))?;
                    set(&o, "values", &string_array(v))?;
                }
                ShowVariable::Functions(v) => {
                    set(&o, "variable", &JsValue::from_str("functions"))?;
                    set(&o, "values", &string_array(v))?;
                }
                ShowVariable::Version(s) => {
                    set(&o, "variable", &JsValue::from_str("version"))?;
                    set(&o, "value", &JsValue::from_str(s))?;
                }
            }
        }
    }
    Ok(o.into())
}

fn count_obj(o: &Object, kind: &str, n: u64) -> Result<(), JsError> {
    set(o, "kind", &JsValue::from_str(kind))?;
    set(o, "count", &JsValue::from_f64(n as f64))?;
    Ok(())
}

fn string_array(items: &[String]) -> JsValue {
    let arr = Array::new();
    for s in items {
        arr.push(&JsValue::from_str(s));
    }
    arr.into()
}

fn row_to_js(cells: &[Value]) -> Result<JsValue, JsError> {
    let arr = Array::new();
    for v in cells {
        arr.push(&cell_to_js(v)?);
    }
    Ok(arr.into())
}

/// A map cell / SelectMap row as a JS array of `[key, value]` pairs (consumable by `new Map(...)`).
fn map_to_js(m: &BTreeMap<String, Value>) -> Result<JsValue, JsError> {
    let entries = Array::new();
    for (k, v) in m {
        let pair = Array::new();
        pair.push(&JsValue::from_str(k));
        pair.push(&cell_to_js(v)?);
        entries.push(&JsValue::from(pair));
    }
    Ok(entries.into())
}

/// One decoded cell as an idiomatic JS value.
fn cell_to_js(v: &Value) -> Result<JsValue, JsError> {
    Ok(match v {
        Value::Null => JsValue::NULL,
        Value::Bool(b) => JsValue::from_bool(*b),
        Value::I8(x) => JsValue::from_f64(f64::from(*x)),
        Value::I16(x) => JsValue::from_f64(f64::from(*x)),
        Value::I32(x) => JsValue::from_f64(f64::from(*x)),
        Value::U8(x) => JsValue::from_f64(f64::from(*x)),
        Value::U16(x) => JsValue::from_f64(f64::from(*x)),
        Value::U32(x) => JsValue::from_f64(f64::from(*x)),
        Value::Int(x) => JsValue::from(BigInt::from(*x)),
        Value::U64(x) => JsValue::from(BigInt::from(*x)),
        Value::I128(x) => JsValue::from(BigInt::from(*x)),
        Value::U128(x) => JsValue::from(BigInt::from(*x)),
        Value::Float(x) => JsValue::from_f64(*x),
        Value::F32(x) => JsValue::from_f64(f64::from(*x)),
        Value::Text(s) => JsValue::from_str(s),
        Value::Bytes(b) => JsValue::from(Uint8Array::from(b.as_slice())),
        Value::Decimal { mantissa, scale } => {
            let o = Object::new();
            set(&o, "mantissa", &JsValue::from(BigInt::from(*mantissa)))?;
            set(&o, "scale", &JsValue::from_f64(f64::from(*scale)))?;
            o.into()
        }
        Value::Date(d) => JsValue::from_f64(f64::from(*d)),
        Value::Time(t) => JsValue::from(BigInt::from(*t)),
        Value::Timestamp(t) => JsValue::from(BigInt::from(*t)),
        Value::Interval { months, micros } => {
            let o = Object::new();
            set(&o, "months", &JsValue::from_f64(f64::from(*months)))?;
            set(&o, "micros", &JsValue::from(BigInt::from(*micros)))?;
            o.into()
        }
        Value::Uuid(u) => JsValue::from_str(&format!("{u:032x}")),
        Value::Inet(ip) => JsValue::from_str(&ip.to_string()),
        Value::Point { x, y } => {
            let o = Object::new();
            set(&o, "x", &JsValue::from_f64(*x))?;
            set(&o, "y", &JsValue::from_f64(*y))?;
            o.into()
        }
        Value::List(items) => row_to_js(items)?,
        Value::Map(m) => map_to_js(m)?,
    })
}

// ---- facet method groups ----
mod archive;
mod calendar_fns;
mod cas;
mod chat;
mod columnar;
mod contacts_fns;
mod dataframe;
mod document;
mod drive;
mod graph;
mod kv;
mod ledger;
mod lanes;
mod mail_fns;
mod meetings;
mod pages;
mod queue;
mod search;
mod telemetry;
mod tickets;
mod timeseries;
mod vector;
