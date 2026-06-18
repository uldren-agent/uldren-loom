//! The mail facet: email for the agent.
//!
//! Unlike calendar/contacts, only the message *body* is inherently immutable: the raw RFC 5322 bytes are
//! stored once in the content-addressed store (CAS), deduplicated and integrity-verified, and the `.eml`
//! projection is simply those bytes (no reconstruction). Around the body sit two mutable, structured
//! pieces:
//!
//! - a **structured index record** ([`MailMessage`]) - parsed `From`/`To`/`Subject`/`Date`/`Message-ID`
//!   plus a header bag and the body's CAS digest - so the facet is queryable without re-parsing the body;
//! - **flags / labels** kept in a **separate versioned sub-tree** (`flags/<uid>`) from the message index
//!   (`msg/<uid>`), so flag churn diffs independently of message arrivals and can be squash-bounded by a
//!   retention policy without rewriting message history.
//!
//! Messages live per principal and mailbox. Pure-Rust, `wasm32`-clean.
//! There is no SMTP; this facet stores, indexes, flags, and serves mail. Primary surface is MCP and
//! mount ingestion of exported mail.

use crate::acl::AclRight;
use crate::cas::{cas_get_unchecked, cas_put_unchecked};
use crate::cbor::{self, Value};
use crate::change_set::{ChangeCursor, ChangeGapState, ChangeItem, ChangeSet};
use crate::digest::{Algo, Digest};
use crate::error::{Code, LoomError, Result};
use crate::hooks::{PimEventEnvelope, hook_emit_event_unchecked};
use crate::provider::ObjectStore;
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId, facet_path};
pub use loom_pim::mail::{MailMessage, MailboxMeta};
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

const META_FILE: &str = ".collection";
const MSG_DIR: &str = "msg";
const FLAGS_DIR: &str = "flags";
const IMAP_UID_STATE_FILE: &str = "imap-uid-state";
const IMAP_SUBSCRIPTIONS_FILE: &str = ".imap-subscriptions";
const FLAG_RETENTION_POLICY_FILE: &str = ".flag-retention-policy";
const MUTABLE_STATE_FILE: &str = ".mutable-state";
const ACCOUNT_QUOTA_POLICY_FILE: &str = ".account-quota-policy";
const DEFAULT_FLAG_DELTA_WINDOW_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
const DEFAULT_MAX_DETAILED_FLAG_DELTAS: u32 = 10_000;

fn validate_segment(seg: &str, what: &str) -> Result<()> {
    if seg.is_empty() || seg == "." || seg == ".." || seg.contains('/') || seg.starts_with('.') {
        return Err(LoomError::invalid(format!(
            "mail: invalid {what} segment {seg:?}"
        )));
    }
    Ok(())
}

fn mailbox_dir(principal: &str, mailbox: &str) -> String {
    facet_path(FacetKind::Mail, &format!("{principal}/{mailbox}"))
}

fn mailbox_scope(principal: &str, mailbox: &str) -> String {
    format!("{principal}/{mailbox}")
}

fn principal_scope(principal: &str) -> String {
    format!("{principal}/")
}

fn meta_path(principal: &str, mailbox: &str) -> String {
    format!("{}/{META_FILE}", mailbox_dir(principal, mailbox))
}

fn msg_path(principal: &str, mailbox: &str, uid: &str) -> String {
    format!(
        "{}/{MSG_DIR}/{}",
        mailbox_dir(principal, mailbox),
        hex::encode(uid.as_bytes())
    )
}

fn flags_path(principal: &str, mailbox: &str, uid: &str) -> String {
    format!(
        "{}/{FLAGS_DIR}/{}",
        mailbox_dir(principal, mailbox),
        hex::encode(uid.as_bytes())
    )
}

fn flag_retention_policy_path(principal: &str, mailbox: &str) -> String {
    format!(
        "{}/{}",
        mailbox_dir(principal, mailbox),
        FLAG_RETENTION_POLICY_FILE
    )
}

fn mutable_state_path(principal: &str, mailbox: &str) -> String {
    format!("{}/{}", mailbox_dir(principal, mailbox), MUTABLE_STATE_FILE)
}

fn imap_uid_state_path(principal: &str, mailbox: &str) -> String {
    format!(
        "{}/{}",
        mailbox_dir(principal, mailbox),
        IMAP_UID_STATE_FILE
    )
}

fn imap_subscriptions_path(principal: &str) -> String {
    facet_path(
        FacetKind::Mail,
        &format!("{principal}/{IMAP_SUBSCRIPTIONS_FILE}"),
    )
}

fn account_quota_policy_path(principal: &str) -> String {
    facet_path(
        FacetKind::Mail,
        &format!("{principal}/{ACCOUNT_QUOTA_POLICY_FILE}"),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailAccountUsage {
    pub used_octets: u64,
    pub hard_limit_octets: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapUidState {
    pub uid_validity: u32,
    pub uid_next: u32,
    pub mappings: Vec<ImapUidMapping>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapUidMapping {
    pub uid: String,
    pub imap_uid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailFlagRetentionPolicy {
    pub detailed_delta_window_ms: u64,
    pub max_detailed_deltas: u32,
    pub audit_summary_class: String,
    pub retained_gap_behavior: String,
}

impl Default for MailFlagRetentionPolicy {
    fn default() -> Self {
        Self {
            detailed_delta_window_ms: DEFAULT_FLAG_DELTA_WINDOW_MS,
            max_detailed_deltas: DEFAULT_MAX_DETAILED_FLAG_DELTAS,
            audit_summary_class: "keyword-sets".to_string(),
            retained_gap_behavior: "full-resync-required".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailFlagDelta {
    pub version: u64,
    pub uid: String,
    pub old_flags: Vec<String>,
    pub new_flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailMutableState {
    pub version: u64,
    pub retained_since_version: u64,
    pub flag_deltas: Vec<MailFlagDelta>,
    pub audit_summaries: Vec<MailFlagAuditSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailFlagAuditSummary {
    pub version: u64,
    pub uid: String,
    pub old_flags_digest: Digest,
    pub new_flags_digest: Digest,
}

/// Create (or update the metadata of) a mailbox under `principal`.
pub fn create_mailbox<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    meta: &MailboxMeta,
) -> Result<()> {
    validate_segment(principal, "principal")?;
    validate_segment(mailbox, "mailbox")?;
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Write,
    )?;
    loom.create_directory_reserved(ns, &mailbox_dir(principal, mailbox), true)?;
    loom.write_file_reserved(ns, &meta_path(principal, mailbox), &meta.encode(), 0o100644)
}

/// The metadata of a mailbox, or `None` if absent.
pub fn get_mailbox<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<Option<MailboxMeta>> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Read,
    )?;
    get_mailbox_unchecked(loom, ns, principal, mailbox)
}

fn get_mailbox_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<Option<MailboxMeta>> {
    match loom.read_file_reserved(ns, &meta_path(principal, mailbox)) {
        Ok(bytes) => Ok(Some(MailboxMeta::decode(&bytes)?)),
        Err(e) if e.code == Code::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Mailbox ids under `principal`, sorted.
pub fn list_mailboxes<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
) -> Result<Vec<String>> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &principal_scope(principal),
        AclRight::Read,
    )?;
    let prefix = format!("{}/", facet_path(FacetKind::Mail, principal));
    let suffix = format!("/{META_FILE}");
    let mut out: Vec<String> = loom
        .staged_paths(ns)
        .into_iter()
        .filter_map(|p| {
            let rest = p.strip_prefix(&prefix)?;
            let mb = rest.strip_suffix(&suffix)?;
            if mb.contains('/') {
                return None;
            }
            Some(mb.to_string())
        })
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// Delete a mailbox and every message index and flag set in it (bodies stay in CAS until GC); returns
/// whether it existed.
pub fn delete_mailbox<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<bool> {
    validate_segment(principal, "principal")?;
    validate_segment(mailbox, "mailbox")?;
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Write,
    )?;
    let prefix = format!("{}/", mailbox_dir(principal, mailbox));
    let paths: Vec<String> = loom
        .staged_paths(ns)
        .into_iter()
        .filter(|p| p.starts_with(&prefix))
        .collect();
    let existed = !paths.is_empty();
    for p in paths {
        loom.remove_file_reserved(ns, &p)?;
    }
    if existed {
        remove_imap_subscription_unchecked(loom, ns, principal, mailbox)?;
    }
    Ok(existed)
}

pub fn rename_mailbox<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    source_mailbox: &str,
    target_mailbox: &str,
) -> Result<()> {
    validate_segment(principal, "principal")?;
    validate_segment(source_mailbox, "source mailbox")?;
    validate_segment(target_mailbox, "target mailbox")?;
    if source_mailbox.eq_ignore_ascii_case(target_mailbox) {
        return Ok(());
    }
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, source_mailbox),
        AclRight::Write,
    )?;
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, target_mailbox),
        AclRight::Write,
    )?;
    if get_mailbox_unchecked(loom, ns, principal, source_mailbox)?.is_none() {
        return Err(LoomError::not_found(format!(
            "mail: mailbox {principal}/{source_mailbox} does not exist"
        )));
    }
    if get_mailbox_unchecked(loom, ns, principal, target_mailbox)?.is_some() {
        return Err(LoomError::new(
            Code::AlreadyExists,
            format!("mail: mailbox {principal}/{target_mailbox} already exists"),
        ));
    }
    let source_prefix = format!("{}/", mailbox_dir(principal, source_mailbox));
    let target_prefix = format!("{}/", mailbox_dir(principal, target_mailbox));
    loom.create_directory_reserved(ns, &mailbox_dir(principal, target_mailbox), true)?;
    let paths = loom
        .staged_paths(ns)
        .into_iter()
        .filter(|path| path.starts_with(&source_prefix))
        .collect::<Vec<_>>();
    for path in &paths {
        let bytes = loom.read_file_reserved(ns, path)?;
        let target_path = format!(
            "{target_prefix}{}",
            path.strip_prefix(&source_prefix).unwrap_or(path)
        );
        if let Some((parent, _)) = target_path.rsplit_once('/') {
            loom.create_directory_reserved(ns, parent, true)?;
        }
        loom.write_file_reserved(ns, &target_path, &bytes, 0o100644)?;
    }
    for path in paths {
        loom.remove_file_reserved(ns, &path)?;
    }
    let mut subscriptions = read_imap_subscriptions(loom, ns, principal)?;
    if subscriptions.remove(source_mailbox) {
        subscriptions.insert(target_mailbox.to_string());
        write_imap_subscriptions(loom, ns, principal, &subscriptions)?;
    }
    Ok(())
}

fn require_mailbox<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<()> {
    if get_mailbox_unchecked(loom, ns, principal, mailbox)?.is_none() {
        return Err(LoomError::not_found(format!(
            "mail: mailbox {principal}/{mailbox} does not exist"
        )));
    }
    Ok(())
}

pub fn put_flag_retention_policy<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    policy: &MailFlagRetentionPolicy,
) -> Result<()> {
    validate_segment(principal, "principal")?;
    validate_segment(mailbox, "mailbox")?;
    validate_flag_retention_policy(policy)?;
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Write,
    )?;
    require_mailbox(loom, ns, principal, mailbox)?;
    loom.write_file_reserved(
        ns,
        &flag_retention_policy_path(principal, mailbox),
        &flag_retention_policy_to_cbor(policy),
        0o100644,
    )
}

pub fn get_flag_retention_policy<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<MailFlagRetentionPolicy> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Read,
    )?;
    get_flag_retention_policy_unchecked(loom, ns, principal, mailbox)
}

fn get_flag_retention_policy_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<MailFlagRetentionPolicy> {
    match loom.read_file_reserved(ns, &flag_retention_policy_path(principal, mailbox)) {
        Ok(bytes) => flag_retention_policy_from_cbor(&bytes),
        Err(err) if err.code == Code::NotFound => Ok(MailFlagRetentionPolicy::default()),
        Err(err) => Err(err),
    }
}

pub fn get_mutable_state<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<MailMutableState> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Read,
    )?;
    get_mutable_state_unchecked(loom, ns, principal, mailbox)
}

pub fn mutable_state_version<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<u64> {
    Ok(get_mutable_state(loom, ns, principal, mailbox)?.version)
}

pub fn apply_flag_ops<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    uid: &str,
    add: &[String],
    remove: &[String],
) -> Result<Vec<String>> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Write,
    )?;
    if get_message_unchecked(loom, ns, principal, mailbox, uid)?.is_none() {
        return Err(LoomError::not_found(format!(
            "mail: message {uid} does not exist"
        )));
    }
    let mut flags = get_flags_unchecked(loom, ns, principal, mailbox, uid)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    for flag in remove {
        flags.remove(flag);
    }
    for flag in add {
        flags.insert(flag.clone());
    }
    let normalized = flags.into_iter().collect::<Vec<_>>();
    set_flags(loom, ns, principal, mailbox, uid, &normalized)?;
    Ok(normalized)
}

pub fn replace_flags_observed<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    uid: &str,
    observed_version: u64,
    flags: &[String],
) -> Result<()> {
    let current = get_mutable_state_unchecked(loom, ns, principal, mailbox)?.version;
    if current != observed_version {
        return Err(LoomError::new(
            Code::Conflict,
            format!(
                "mail flags observed version {observed_version} is stale; current is {current}"
            ),
        ));
    }
    set_flags(loom, ns, principal, mailbox, uid, flags)
}

pub fn compact_mutable_state<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    retain_from_version: u64,
) -> Result<MailMutableState> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Write,
    )?;
    let mut state = get_mutable_state_unchecked(loom, ns, principal, mailbox)?;
    let retained = retain_from_version.min(state.version.saturating_add(1));
    state.flag_deltas.retain(|delta| delta.version >= retained);
    state.retained_since_version = state.retained_since_version.max(retained);
    loom.write_file_reserved(
        ns,
        &mutable_state_path(principal, mailbox),
        &mutable_state_to_cbor(&state),
        0o100644,
    )?;
    Ok(state)
}

pub fn require_mutable_state_since<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    since_version: u64,
) -> Result<()> {
    let state = get_mutable_state(loom, ns, principal, mailbox)?;
    if since_version < state.retained_since_version {
        return Err(LoomError::retained_gap(format!(
            "mail mutable-state version {since_version} predates retained detailed history {}",
            state.retained_since_version
        )));
    }
    Ok(())
}

pub fn mutable_state_changeset<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    since_version: u64,
) -> Result<ChangeSet> {
    let state = get_mutable_state(loom, ns, principal, mailbox)?;
    if since_version < state.retained_since_version {
        return Err(LoomError::retained_gap(format!(
            "mail mutable-state version {since_version} predates retained detailed history {}",
            state.retained_since_version
        )));
    }
    let scope = mail_change_scope(ns, principal, mailbox);
    let items = state
        .flag_deltas
        .iter()
        .filter(|delta| delta.version >= since_version)
        .map(|delta| {
            ChangeItem::sequence_record(delta.version, cbor::encode(&flag_delta_value(delta)))
        })
        .collect();
    ChangeSet::new(
        scope.clone(),
        ChangeGapState::Retained,
        Some(state.retained_since_version),
        ChangeCursor::sequence(scope, state.version.saturating_add(1)),
        items,
    )
}

pub fn mail_change_scope(ns: WorkspaceId, principal: &str, mailbox: &str) -> String {
    format!("mail:{}:{principal}/{mailbox}", hex::encode(ns.as_bytes()))
}

fn get_mutable_state_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<MailMutableState> {
    match loom.read_file_reserved(ns, &mutable_state_path(principal, mailbox)) {
        Ok(bytes) => mutable_state_from_cbor(&bytes),
        Err(err) if err.code == Code::NotFound => Ok(MailMutableState {
            version: 0,
            retained_since_version: 0,
            flag_deltas: Vec::new(),
            audit_summaries: Vec::new(),
        }),
        Err(err) => Err(err),
    }
}

/// Ingest a raw RFC 5322 message into a mailbox under `uid`: store the immutable body in the CAS, parse
/// the headers into a structured index record, and write the index. Returns the body's content address.
/// The body is the source of truth; re-ingesting identical bytes dedups in the CAS.
pub fn ingest_message<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    uid: &str,
    raw: &[u8],
) -> Result<Digest> {
    validate_segment(principal, "principal")?;
    validate_segment(mailbox, "mailbox")?;
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Write,
    )?;
    if uid.is_empty() {
        return Err(LoomError::invalid("mail: message UID must not be empty"));
    }
    require_mailbox(loom, ns, principal, mailbox)?;

    let body = cas_put_unchecked(loom, ns, raw)?;
    let msg = MailMessage::from_rfc5322(uid, body.to_hex(), raw)?;
    let path = msg_path(principal, mailbox, uid);
    let before = match loom.read_file_reserved(ns, &path) {
        Ok(bytes) => Some(bytes),
        Err(err) if err.code == Code::NotFound => None,
        Err(err) => return Err(err),
    };
    let bytes = msg.encode();
    let lifecycle_event = if before.is_some() {
        "before_update"
    } else {
        "before_create"
    };
    emit_mail_event(
        loom,
        ns,
        lifecycle_event,
        principal,
        mailbox,
        uid,
        (before.clone(), Some(bytes.clone())),
    )?;
    loom.create_directory_reserved(
        ns,
        &format!("{}/{MSG_DIR}", mailbox_dir(principal, mailbox)),
        true,
    )?;
    loom.write_file_reserved(ns, &path, &bytes, 0o100644)?;
    let lifecycle_event = if before.is_some() {
        "after_update"
    } else {
        "after_create"
    };
    emit_mail_event(
        loom,
        ns,
        lifecycle_event,
        principal,
        mailbox,
        uid,
        (before.clone(), Some(bytes.clone())),
    )?;
    if before.is_none() {
        emit_mail_event(
            loom,
            ns,
            "on_message_ingested",
            principal,
            mailbox,
            uid,
            (None, Some(bytes)),
        )?;
    }
    Ok(body)
}

/// The structured index of the message at `uid`, or `None` if absent.
pub fn get_message<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    uid: &str,
) -> Result<Option<MailMessage>> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Read,
    )?;
    get_message_unchecked(loom, ns, principal, mailbox, uid)
}

fn get_message_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    uid: &str,
) -> Result<Option<MailMessage>> {
    match loom.read_file_reserved(ns, &msg_path(principal, mailbox, uid)) {
        Ok(bytes) => Ok(Some(MailMessage::decode(&bytes)?)),
        Err(e) if e.code == Code::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Rebuild a [`Digest`] from a stored hex body address under the store's identity profile.
fn body_digest<S: ObjectStore>(loom: &Loom<S>, hex: &str) -> Result<Digest> {
    let raw = hex::decode(hex).map_err(|_| LoomError::corrupt("mail: bad body digest hex"))?;
    let bytes: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::corrupt("mail: body digest is not 32 bytes"))?;
    Ok(Digest::of(loom.store().digest_algo(), bytes))
}

/// The raw RFC 5322 bytes (the `.eml`) of the message at `uid`, fetched from the CAS and digest-verified,
/// or `None` if the message is absent. This is the format-specific serialization; the parsed structured
/// view is [`get_message`].
pub fn to_eml<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    uid: &str,
) -> Result<Option<Vec<u8>>> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Read,
    )?;
    match get_message_unchecked(loom, ns, principal, mailbox, uid)? {
        Some(msg) => cas_get_unchecked(loom, ns, &body_digest(loom, &msg.body)?),
        None => Ok(None),
    }
}

/// Remove the message index and its flags at `uid` (the immutable body stays in the CAS until GC, and an
/// earlier commit still restores it); returns whether the message was present.
pub fn delete_message<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    uid: &str,
) -> Result<bool> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Write,
    )?;
    let mpath = msg_path(principal, mailbox, uid);
    let before = match loom.read_file_reserved(ns, &mpath) {
        Ok(bytes) => Some(bytes),
        Err(err) if err.code == Code::NotFound => None,
        Err(err) => return Err(err),
    };
    if let Some(bytes) = before {
        emit_mail_event(
            loom,
            ns,
            "before_delete",
            principal,
            mailbox,
            uid,
            (Some(bytes.clone()), None),
        )?;
        loom.remove_file_reserved(ns, &mpath)?;
        let fpath = flags_path(principal, mailbox, uid);
        if loom.staged_paths(ns).iter().any(|p| p == &fpath) {
            loom.remove_file_reserved(ns, &fpath)?;
        }
        emit_mail_event(
            loom,
            ns,
            "after_delete",
            principal,
            mailbox,
            uid,
            (Some(bytes), None),
        )?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn move_message<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    source_mailbox: &str,
    source_uid: &str,
    target_mailbox: &str,
    target_uid: &str,
) -> Result<bool> {
    validate_segment(principal, "principal")?;
    validate_segment(source_mailbox, "source mailbox")?;
    validate_segment(target_mailbox, "target mailbox")?;
    if source_uid.is_empty() || target_uid.is_empty() {
        return Err(LoomError::invalid("mail: message UID must not be empty"));
    }
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, source_mailbox),
        AclRight::Write,
    )?;
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, target_mailbox),
        AclRight::Write,
    )?;
    require_mailbox(loom, ns, principal, target_mailbox)?;
    let Some(source_message) =
        get_message_unchecked(loom, ns, principal, source_mailbox, source_uid)?
    else {
        return Ok(false);
    };
    let raw = cas_get_unchecked(loom, ns, &body_digest(loom, &source_message.body)?)?
        .ok_or_else(|| LoomError::corrupt("mail: message body is missing from CAS"))?;
    let before = Some(source_message.encode());
    let flags = get_flags_unchecked(loom, ns, principal, source_mailbox, source_uid)?;
    ingest_message(loom, ns, principal, target_mailbox, target_uid, &raw)?;
    if !flags.is_empty() {
        set_flags(loom, ns, principal, target_mailbox, target_uid, &flags)?;
    }
    let after = get_message_unchecked(loom, ns, principal, target_mailbox, target_uid)?
        .map(|message| message.encode());
    if delete_message(loom, ns, principal, source_mailbox, source_uid)? {
        emit_mail_event(
            loom,
            ns,
            "on_moved",
            principal,
            target_mailbox,
            target_uid,
            (before, after),
        )?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// All message indexes in a mailbox, sorted by `UID`.
pub fn list_messages<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<Vec<MailMessage>> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Read,
    )?;
    list_messages_unchecked(loom, ns, principal, mailbox)
}

fn list_messages_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<Vec<MailMessage>> {
    let prefix = format!("{}/{MSG_DIR}/", mailbox_dir(principal, mailbox));
    let mut out: Vec<MailMessage> = Vec::new();
    for p in loom.staged_paths(ns) {
        let Some(seg) = p.strip_prefix(&prefix) else {
            continue;
        };
        if seg.contains('/') {
            continue;
        }
        out.push(MailMessage::decode(&loom.read_file_reserved(ns, &p)?)?);
    }
    out.sort_by(|a, b| a.uid.cmp(&b.uid));
    Ok(out)
}

pub fn set_account_hard_limit<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    hard_limit_octets: Option<u64>,
) -> Result<()> {
    validate_segment(principal, "principal")?;
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &principal_scope(principal),
        AclRight::Write,
    )?;
    loom.create_directory_reserved(ns, &facet_path(FacetKind::Mail, principal), true)?;
    match hard_limit_octets {
        Some(limit) => loom.write_file_reserved(
            ns,
            &account_quota_policy_path(principal),
            &account_quota_policy_to_cbor(limit),
            0o100644,
        ),
        None => match loom.remove_file_reserved(ns, &account_quota_policy_path(principal)) {
            Ok(()) => Ok(()),
            Err(err) if err.code == Code::NotFound => Ok(()),
            Err(err) => Err(err),
        },
    }
}

pub fn get_account_hard_limit<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
) -> Result<Option<u64>> {
    validate_segment(principal, "principal")?;
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &principal_scope(principal),
        AclRight::Read,
    )?;
    get_account_hard_limit_unchecked(loom, ns, principal)
}

pub fn account_usage<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
) -> Result<MailAccountUsage> {
    validate_segment(principal, "principal")?;
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &principal_scope(principal),
        AclRight::Read,
    )?;
    let mut used_octets = 0u64;
    for mailbox in list_mailboxes(loom, ns, principal)? {
        for message in list_messages(loom, ns, principal, &mailbox)? {
            used_octets = used_octets
                .checked_add(message.size)
                .ok_or_else(|| LoomError::corrupt("mail account usage overflow"))?;
        }
    }
    Ok(MailAccountUsage {
        used_octets,
        hard_limit_octets: get_account_hard_limit_unchecked(loom, ns, principal)?,
    })
}

fn get_account_hard_limit_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
) -> Result<Option<u64>> {
    match loom.read_file_reserved(ns, &account_quota_policy_path(principal)) {
        Ok(bytes) => account_quota_policy_from_cbor(&bytes).map(Some),
        Err(err) if err.code == Code::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

pub fn ensure_imap_uid_state<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<ImapUidState> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Read,
    )?;
    require_mailbox(loom, ns, principal, mailbox)?;
    let messages = list_messages_unchecked(loom, ns, principal, mailbox)?;
    let mut state =
        read_imap_uid_state(loom, ns, principal, mailbox)?.unwrap_or_else(|| ImapUidState {
            uid_validity: new_imap_uid_validity(loom, ns, principal, mailbox),
            uid_next: 1,
            mappings: Vec::new(),
        });
    let mut changed = false;
    if state.uid_validity == 0 {
        state.uid_validity = new_imap_uid_validity(loom, ns, principal, mailbox);
        changed = true;
    }
    let max_uid = state
        .mappings
        .iter()
        .map(|mapping| mapping.imap_uid)
        .max()
        .unwrap_or(0);
    if state.uid_next <= max_uid {
        state.uid_next = max_uid.saturating_add(1).max(1);
        changed = true;
    }
    for message in messages {
        if state
            .mappings
            .iter()
            .any(|mapping| mapping.uid == message.uid)
        {
            continue;
        }
        let imap_uid = state.uid_next.max(1);
        state.uid_next = state
            .uid_next
            .saturating_add(1)
            .max(imap_uid.saturating_add(1));
        state.mappings.push(ImapUidMapping {
            uid: message.uid,
            imap_uid,
        });
        changed = true;
    }
    state.mappings.sort_by(|a, b| a.uid.cmp(&b.uid));
    if changed {
        loom.write_file_reserved(
            ns,
            &imap_uid_state_path(principal, mailbox),
            &encode_imap_uid_state(&state),
            0o100644,
        )?;
    }
    Ok(state)
}

pub fn get_imap_uid_state<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<Option<ImapUidState>> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Read,
    )?;
    read_imap_uid_state(loom, ns, principal, mailbox)
}

pub fn reset_imap_uid_state<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<ImapUidState> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Write,
    )?;
    require_mailbox(loom, ns, principal, mailbox)?;
    let messages = list_messages_unchecked(loom, ns, principal, mailbox)?;
    let mut state = ImapUidState {
        uid_validity: new_imap_uid_validity(loom, ns, principal, mailbox),
        uid_next: 1,
        mappings: Vec::new(),
    };
    for message in messages {
        let imap_uid = state.uid_next.max(1);
        state.uid_next = state
            .uid_next
            .saturating_add(1)
            .max(imap_uid.saturating_add(1));
        state.mappings.push(ImapUidMapping {
            uid: message.uid,
            imap_uid,
        });
    }
    state.mappings.sort_by(|a, b| a.uid.cmp(&b.uid));
    loom.write_file_reserved(
        ns,
        &imap_uid_state_path(principal, mailbox),
        &encode_imap_uid_state(&state),
        0o100644,
    )?;
    Ok(state)
}

pub fn list_imap_subscriptions<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
) -> Result<Vec<String>> {
    validate_segment(principal, "principal")?;
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &principal_scope(principal),
        AclRight::Read,
    )?;
    Ok(read_imap_subscriptions(loom, ns, principal)?
        .into_iter()
        .collect())
}

pub fn subscribe_imap_mailbox<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<bool> {
    validate_segment(principal, "principal")?;
    validate_segment(mailbox, "mailbox")?;
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &principal_scope(principal),
        AclRight::Write,
    )?;
    require_mailbox(loom, ns, principal, mailbox)?;
    let mut subscriptions = read_imap_subscriptions(loom, ns, principal)?;
    let inserted = subscriptions.insert(mailbox.to_string());
    if inserted {
        write_imap_subscriptions(loom, ns, principal, &subscriptions)?;
    }
    Ok(inserted)
}

pub fn unsubscribe_imap_mailbox<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<bool> {
    validate_segment(principal, "principal")?;
    validate_segment(mailbox, "mailbox")?;
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &principal_scope(principal),
        AclRight::Write,
    )?;
    remove_imap_subscription_unchecked(loom, ns, principal, mailbox)
}

pub fn put_blob<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    bytes: &[u8],
) -> Result<Digest> {
    validate_segment(principal, "principal")?;
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &principal_scope(principal),
        AclRight::Write,
    )?;
    cas_put_unchecked(loom, ns, bytes)
}

pub fn get_blob<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    digest: &Digest,
) -> Result<Option<Vec<u8>>> {
    validate_segment(principal, "principal")?;
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &principal_scope(principal),
        AclRight::Read,
    )?;
    cas_get_unchecked(loom, ns, digest)
}

fn read_imap_uid_state<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<Option<ImapUidState>> {
    match loom.read_file_reserved(ns, &imap_uid_state_path(principal, mailbox)) {
        Ok(bytes) => Ok(Some(decode_imap_uid_state(&bytes)?)),
        Err(e) if e.code == Code::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn read_imap_subscriptions<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
) -> Result<BTreeSet<String>> {
    match loom.read_file_reserved(ns, &imap_subscriptions_path(principal)) {
        Ok(bytes) => cbor::as_array(cbor::decode(&bytes)?)?
            .into_iter()
            .map(cbor::as_text)
            .collect::<Result<BTreeSet<_>>>(),
        Err(e) if e.code == Code::NotFound => Ok(BTreeSet::new()),
        Err(e) => Err(e),
    }
}

fn write_imap_subscriptions<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    subscriptions: &BTreeSet<String>,
) -> Result<()> {
    let path = imap_subscriptions_path(principal);
    if subscriptions.is_empty() {
        if loom.staged_paths(ns).iter().any(|p| p == &path) {
            loom.remove_file_reserved(ns, &path)?;
        }
        return Ok(());
    }
    let arr = Value::Array(
        subscriptions
            .iter()
            .map(|mailbox| Value::Text(mailbox.clone()))
            .collect(),
    );
    loom.write_file_reserved(ns, &path, &cbor::encode(&arr), 0o100644)
}

fn remove_imap_subscription_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> Result<bool> {
    let mut subscriptions = read_imap_subscriptions(loom, ns, principal)?;
    let removed = subscriptions.remove(mailbox);
    if removed {
        write_imap_subscriptions(loom, ns, principal, &subscriptions)?;
    }
    Ok(removed)
}

fn new_imap_uid_validity<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
) -> u32 {
    let mut bytes = b"loom-imap-uidvalidity-v1\0".to_vec();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    bytes.extend_from_slice(&now.to_be_bytes());
    bytes.push(0);
    bytes.extend_from_slice(ns.to_string().as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(principal.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(mailbox.as_bytes());
    let digest = Digest::hash(loom.store().digest_algo(), &bytes);
    let raw = digest.bytes();
    u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]).max(1)
}

fn encode_imap_uid_state(state: &ImapUidState) -> Vec<u8> {
    cbor::encode(&Value::Map(vec![
        (
            Value::Text("uid_validity".into()),
            Value::Uint(state.uid_validity as u64),
        ),
        (
            Value::Text("uid_next".into()),
            Value::Uint(state.uid_next as u64),
        ),
        (
            Value::Text("mappings".into()),
            Value::Array(
                state
                    .mappings
                    .iter()
                    .map(|mapping| {
                        Value::Array(vec![
                            Value::Text(mapping.uid.clone()),
                            Value::Uint(mapping.imap_uid as u64),
                        ])
                    })
                    .collect(),
            ),
        ),
    ]))
}

fn decode_imap_uid_state(bytes: &[u8]) -> Result<ImapUidState> {
    let pairs = cbor::as_map(cbor::decode(bytes)?)?;
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(field, _)| matches!(field, Value::Text(text) if text == key))
            .map(|(_, value)| value.clone())
    };
    let number = |key: &str| -> Result<u32> {
        let value = get(key).ok_or_else(|| LoomError::corrupt(format!("mail: missing {key}")))?;
        let raw = cbor::as_int(value)?;
        u32::try_from(raw).map_err(|_| LoomError::corrupt(format!("mail: invalid {key}")))
    };
    let mappings = match get("mappings") {
        Some(value) => cbor::as_array(value)?
            .into_iter()
            .map(|item| {
                let mut fields = cbor::Fields::new(cbor::as_array(item)?);
                let uid = fields.text()?;
                let imap_uid = u32::try_from(fields.int()?)
                    .map_err(|_| LoomError::corrupt("mail: invalid IMAP UID"))?;
                fields.end()?;
                Ok(ImapUidMapping { uid, imap_uid })
            })
            .collect::<Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    Ok(ImapUidState {
        uid_validity: number("uid_validity")?,
        uid_next: number("uid_next")?.max(1),
        mappings,
    })
}

/// The flags/labels on the message at `uid` (sorted, deduplicated). An absent flag set reads as empty.
pub fn get_flags<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    uid: &str,
) -> Result<Vec<String>> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Read,
    )?;
    get_flags_unchecked(loom, ns, principal, mailbox, uid)
}

fn get_flags_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    uid: &str,
) -> Result<Vec<String>> {
    match loom.read_file_reserved(ns, &flags_path(principal, mailbox, uid)) {
        Ok(bytes) => {
            let flags = cbor::as_array(cbor::decode(&bytes)?)?
                .into_iter()
                .map(cbor::as_text)
                .collect::<Result<Vec<_>>>()?;
            Ok(flags)
        }
        Err(e) if e.code == Code::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Replace the flags/labels on the message at `uid` (stored in the separate `flags/` sub-tree so flag
/// churn diffs independently of message arrivals. The message must exist.
pub fn set_flags<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    uid: &str,
    flags: &[String],
) -> Result<()> {
    loom.authorize_collection(
        ns,
        FacetKind::Mail,
        &mailbox_scope(principal, mailbox),
        AclRight::Write,
    )?;
    if get_message_unchecked(loom, ns, principal, mailbox, uid)?.is_none() {
        return Err(LoomError::not_found(format!(
            "mail: message {uid} does not exist"
        )));
    }
    let old_flags = get_flags_unchecked(loom, ns, principal, mailbox, uid)?;
    let set: BTreeSet<&str> = flags.iter().map(String::as_str).collect();
    let normalized = set.into_iter().map(str::to_string).collect::<Vec<_>>();
    let old_bytes = flags_to_cbor(&old_flags);
    let new_bytes = flags_to_cbor(&normalized);
    if old_flags != normalized {
        append_flag_delta_unchecked(loom, ns, principal, mailbox, uid, &old_flags, &normalized)?;
        emit_mail_event(
            loom,
            ns,
            "before_update",
            principal,
            mailbox,
            uid,
            (Some(old_bytes.clone()), Some(new_bytes.clone())),
        )?;
    }
    loom.create_directory_reserved(
        ns,
        &format!("{}/{FLAGS_DIR}", mailbox_dir(principal, mailbox)),
        true,
    )?;
    loom.write_file_reserved(
        ns,
        &flags_path(principal, mailbox, uid),
        &new_bytes,
        0o100644,
    )?;
    if old_flags != normalized {
        emit_mail_event(
            loom,
            ns,
            "after_update",
            principal,
            mailbox,
            uid,
            (Some(old_bytes.clone()), Some(new_bytes.clone())),
        )?;
        emit_mail_event(
            loom,
            ns,
            "on_flags_changed",
            principal,
            mailbox,
            uid,
            (Some(old_bytes), Some(new_bytes)),
        )?;
    }
    Ok(())
}

fn flags_to_cbor(flags: &[String]) -> Vec<u8> {
    cbor::encode(&Value::Array(
        flags.iter().map(|flag| Value::Text(flag.clone())).collect(),
    ))
}

fn append_flag_delta_unchecked<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    uid: &str,
    old_flags: &[String],
    new_flags: &[String],
) -> Result<u64> {
    let mut state = get_mutable_state_unchecked(loom, ns, principal, mailbox)?;
    let next_version = state.version.saturating_add(1);
    state.version = next_version;
    state.flag_deltas.push(MailFlagDelta {
        version: next_version,
        uid: uid.to_string(),
        old_flags: old_flags.to_vec(),
        new_flags: new_flags.to_vec(),
    });
    state.audit_summaries.push(MailFlagAuditSummary {
        version: next_version,
        uid: uid.to_string(),
        old_flags_digest: Digest::hash(loom.store().digest_algo(), &flags_to_cbor(old_flags)),
        new_flags_digest: Digest::hash(loom.store().digest_algo(), &flags_to_cbor(new_flags)),
    });
    loom.write_file_reserved(
        ns,
        &mutable_state_path(principal, mailbox),
        &mutable_state_to_cbor(&state),
        0o100644,
    )?;
    Ok(next_version)
}

fn validate_flag_retention_policy(policy: &MailFlagRetentionPolicy) -> Result<()> {
    if policy.detailed_delta_window_ms == 0 {
        return Err(LoomError::invalid(
            "mail flag retention window must be nonzero",
        ));
    }
    if policy.max_detailed_deltas == 0 {
        return Err(LoomError::invalid(
            "mail flag retention max_detailed_deltas must be nonzero",
        ));
    }
    if policy.audit_summary_class.trim().is_empty() {
        return Err(LoomError::invalid(
            "mail flag retention audit_summary_class must not be empty",
        ));
    }
    if policy.retained_gap_behavior.trim().is_empty() {
        return Err(LoomError::invalid(
            "mail flag retention retained_gap_behavior must not be empty",
        ));
    }
    Ok(())
}

fn flag_retention_policy_to_cbor(policy: &MailFlagRetentionPolicy) -> Vec<u8> {
    cbor::encode(&Value::Array(vec![
        Value::Uint(1),
        Value::Uint(policy.detailed_delta_window_ms),
        Value::Uint(u64::from(policy.max_detailed_deltas)),
        Value::Text(policy.audit_summary_class.clone()),
        Value::Text(policy.retained_gap_behavior.clone()),
    ]))
}

fn flag_retention_policy_from_cbor(bytes: &[u8]) -> Result<MailFlagRetentionPolicy> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    match fields.uint()? {
        1 => {}
        _ => {
            return Err(LoomError::corrupt(
                "unsupported mail flag retention policy version",
            ));
        }
    }
    let policy = MailFlagRetentionPolicy {
        detailed_delta_window_ms: fields.uint()?,
        max_detailed_deltas: u32::try_from(fields.uint()?).map_err(|_| {
            LoomError::corrupt("mail flag retention max_detailed_deltas out of u32 range")
        })?,
        audit_summary_class: fields.text()?,
        retained_gap_behavior: fields.text()?,
    };
    fields.end()?;
    validate_flag_retention_policy(&policy)?;
    Ok(policy)
}

fn account_quota_policy_to_cbor(hard_limit_octets: u64) -> Vec<u8> {
    cbor::encode(&Value::Array(vec![
        Value::Text("loom.mail.account-quota-policy.v1".into()),
        Value::Uint(hard_limit_octets),
    ]))
}

fn account_quota_policy_from_cbor(bytes: &[u8]) -> Result<u64> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let schema = fields.text()?;
    if schema != "loom.mail.account-quota-policy.v1" {
        return Err(LoomError::corrupt("mail account quota policy schema"));
    }
    let hard_limit_octets = fields.uint()?;
    fields.end()?;
    Ok(hard_limit_octets)
}

fn mutable_state_to_cbor(state: &MailMutableState) -> Vec<u8> {
    cbor::encode(&Value::Array(vec![
        Value::Uint(3),
        Value::Uint(state.version),
        Value::Uint(state.retained_since_version),
        Value::Array(state.flag_deltas.iter().map(flag_delta_value).collect()),
        Value::Array(
            state
                .audit_summaries
                .iter()
                .map(flag_audit_summary_value)
                .collect(),
        ),
    ]))
}

fn mutable_state_from_cbor(bytes: &[u8]) -> Result<MailMutableState> {
    let mut fields = cbor::Fields::new(cbor::decode_array(bytes)?);
    let version_tag = fields.uint()?;
    match version_tag {
        1..=3 => {}
        _ => return Err(LoomError::corrupt("unsupported mail mutable state version")),
    }
    let version = fields.uint()?;
    let retained_since_version = if version_tag == 3 { fields.uint()? } else { 0 };
    let flag_deltas = fields
        .array()?
        .into_iter()
        .map(flag_delta_from_value)
        .collect::<Result<Vec<_>>>()?;
    let audit_summaries = if version_tag >= 2 {
        fields
            .array()?
            .into_iter()
            .map(flag_audit_summary_from_value)
            .collect::<Result<Vec<_>>>()?
    } else {
        Vec::new()
    };
    fields.end()?;
    if flag_deltas.iter().any(|delta| delta.version > version) {
        return Err(LoomError::corrupt(
            "mail mutable state delta version exceeds state version",
        ));
    }
    if audit_summaries
        .iter()
        .any(|summary| summary.version > version)
    {
        return Err(LoomError::corrupt(
            "mail mutable state audit version exceeds state version",
        ));
    }
    Ok(MailMutableState {
        version,
        retained_since_version,
        flag_deltas,
        audit_summaries,
    })
}

fn flag_delta_value(delta: &MailFlagDelta) -> Value {
    Value::Array(vec![
        Value::Uint(delta.version),
        Value::Text(delta.uid.clone()),
        flag_list_value(&delta.old_flags),
        flag_list_value(&delta.new_flags),
    ])
}

fn flag_delta_from_value(value: Value) -> Result<MailFlagDelta> {
    let mut fields = cbor::Fields::new(cbor::as_array(value)?);
    let delta = MailFlagDelta {
        version: fields.uint()?,
        uid: fields.text()?,
        old_flags: flag_list_from_value(fields.next_field()?)?,
        new_flags: flag_list_from_value(fields.next_field()?)?,
    };
    fields.end()?;
    Ok(delta)
}

fn flag_audit_summary_value(summary: &MailFlagAuditSummary) -> Value {
    Value::Array(vec![
        Value::Uint(summary.version),
        Value::Text(summary.uid.clone()),
        digest_text_value(&summary.old_flags_digest),
        digest_text_value(&summary.new_flags_digest),
    ])
}

fn flag_audit_summary_from_value(value: Value) -> Result<MailFlagAuditSummary> {
    let mut fields = cbor::Fields::new(cbor::as_array(value)?);
    let summary = MailFlagAuditSummary {
        version: fields.uint()?,
        uid: fields.text()?,
        old_flags_digest: digest_text_from_value(fields.next_field()?)?,
        new_flags_digest: digest_text_from_value(fields.next_field()?)?,
    };
    fields.end()?;
    Ok(summary)
}

fn digest_text_value(digest: &Digest) -> Value {
    Value::Text(digest.to_string())
}

fn digest_text_from_value(value: Value) -> Result<Digest> {
    Digest::parse(&cbor::as_text(value)?)
}

fn flag_list_value(flags: &[String]) -> Value {
    Value::Array(flags.iter().map(|flag| Value::Text(flag.clone())).collect())
}

fn flag_list_from_value(value: Value) -> Result<Vec<String>> {
    cbor::as_array(value)?
        .into_iter()
        .map(cbor::as_text)
        .collect()
}

fn emit_mail_event<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    event: &str,
    principal: &str,
    mailbox: &str,
    uid: &str,
    bodies: (Option<Vec<u8>>, Option<Vec<u8>>),
) -> Result<()> {
    let (before, after) = bodies;
    hook_emit_event_unchecked(
        loom,
        ns,
        &PimEventEnvelope {
            workspace: ns,
            facet: FacetKind::Mail,
            event: event.to_string(),
            principal: principal.to_string(),
            collection: Some(mailbox.to_string()),
            unit: Some(uid.to_string()),
            commit: None,
            before,
            after,
            depth: 0,
            causation: None,
        },
    )?;
    Ok(())
}

/// Search message indexes by a case-insensitive substring over `subject` and `from`; UID-ordered.
pub fn search<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    mailbox: &str,
    text: &str,
) -> Result<Vec<MailMessage>> {
    let needle = text.to_lowercase();
    Ok(list_messages(loom, ns, principal, mailbox)?
        .into_iter()
        .filter(|m| {
            m.subject.to_lowercase().contains(&needle) || m.from.to_lowercase().contains(&needle)
        })
        .collect())
}

/// A message change between two mailbox states. Because the body is immutable, a present message never
/// "updates"; it is only added or removed (a re-ingest under the same UID with different bytes is a
/// remove+add). Flag changes are reported separately by `diff_flags`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageChange {
    pub uid: String,
    pub kind: ChangeKind,
}

/// The nature of a mailbox change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
}

/// Per-UID added/removed messages from `old` to `new` index states (body UID is the unit).
pub fn diff_messages(old: &[MailMessage], new: &[MailMessage]) -> Vec<MessageChange> {
    let old_uids: BTreeSet<&str> = old.iter().map(|m| m.uid.as_str()).collect();
    let new_uids: BTreeSet<&str> = new.iter().map(|m| m.uid.as_str()).collect();
    let mut out = Vec::new();
    for uid in new_uids.difference(&old_uids) {
        out.push(MessageChange {
            uid: (*uid).to_string(),
            kind: ChangeKind::Added,
        });
    }
    for uid in old_uids.difference(&new_uids) {
        out.push(MessageChange {
            uid: (*uid).to_string(),
            kind: ChangeKind::Removed,
        });
    }
    out.sort_by(|a, b| a.uid.cmp(&b.uid));
    out
}

/// Whether the store's digest profile is FIPS (SHA-256); exposed so callers can report the body
/// addressing profile. (Mostly used in tests; keeps `Algo` in the public surface.)
pub fn body_profile<S: ObjectStore>(loom: &Loom<S>) -> Algo {
    loom.store().digest_algo()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acl::{AclRight, AclSubject};
    use crate::error::Code;
    use crate::identity::IdentityStore;
    use crate::provider::memory::MemoryStore;

    fn mail_ns() -> (Loom<MemoryStore>, WorkspaceId) {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Mail, None, WorkspaceId::from_bytes([25; 16]))
            .unwrap();
        (loom, ns)
    }

    fn inbox(loom: &mut Loom<MemoryStore>, ns: WorkspaceId) {
        create_mailbox(
            loom,
            ns,
            "alice",
            "inbox",
            &MailboxMeta {
                display_name: "Inbox".into(),
            },
        )
        .unwrap();
    }

    #[test]
    fn authenticated_mail_operations_are_acl_checked_without_cas_grant() {
        let (mut loom, ns) = mail_ns();
        let root = WorkspaceId::from_bytes([1; 16]);
        let mut identity = IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);

        assert_eq!(
            create_mailbox(
                &mut loom,
                ns,
                "alice",
                "inbox",
                &MailboxMeta {
                    display_name: "Inbox".into(),
                },
            )
            .unwrap_err()
            .code,
            Code::PermissionDenied
        );

        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Mail),
                [AclRight::Write, AclRight::Read],
            )
            .unwrap();

        inbox(&mut loom, ns);
        ingest_message(&mut loom, ns, "alice", "inbox", "m1", RAW).unwrap();
        assert_eq!(
            to_eml(&loom, ns, "alice", "inbox", "m1")
                .unwrap()
                .as_deref(),
            Some(RAW)
        );
    }

    const RAW: &[u8] = b"From: bob@x.io\r\nTo: alice@x.io, team@x.io\r\nSubject: Hello there\r\nDate: Mon, 1 Jan 2024 09:00:00 +0000\r\nMessage-ID: <abc@x.io>\r\n\r\nThis is the body.\r\n";

    #[test]
    fn ingest_parses_headers_and_stores_body() {
        let (mut loom, ns) = mail_ns();
        inbox(&mut loom, ns);
        ingest_message(&mut loom, ns, "alice", "inbox", "m1", RAW).unwrap();
        let msg = get_message(&loom, ns, "alice", "inbox", "m1")
            .unwrap()
            .unwrap();
        assert_eq!(msg.from, "bob@x.io");
        assert_eq!(
            msg.to,
            vec!["alice@x.io".to_string(), "team@x.io".to_string()]
        );
        assert_eq!(msg.subject, "Hello there");
        // mail-parser returns the Message-ID without its surrounding angle brackets.
        assert_eq!(msg.message_id.as_deref(), Some("abc@x.io"));
        // The raw .eml round-trips byte-for-byte from the CAS.
        assert_eq!(
            to_eml(&loom, ns, "alice", "inbox", "m1")
                .unwrap()
                .as_deref(),
            Some(RAW)
        );
    }

    #[test]
    fn ingest_into_missing_mailbox_is_not_found() {
        let (mut loom, ns) = mail_ns();
        let err = ingest_message(&mut loom, ns, "alice", "inbox", "m1", RAW).unwrap_err();
        assert_eq!(err.code, Code::NotFound);
    }

    #[test]
    fn flags_are_independent_and_sorted() {
        let (mut loom, ns) = mail_ns();
        inbox(&mut loom, ns);
        ingest_message(&mut loom, ns, "alice", "inbox", "m1", RAW).unwrap();
        assert!(
            get_flags(&loom, ns, "alice", "inbox", "m1")
                .unwrap()
                .is_empty()
        );
        set_flags(
            &mut loom,
            ns,
            "alice",
            "inbox",
            "m1",
            &["\\Seen".into(), "Important".into(), "\\Seen".into()],
        )
        .unwrap();
        assert_eq!(
            get_flags(&loom, ns, "alice", "inbox", "m1").unwrap(),
            vec!["Important".to_string(), "\\Seen".to_string()]
        );
        // Setting flags on an absent message fails.
        assert_eq!(
            set_flags(&mut loom, ns, "alice", "inbox", "absent", &["x".into()])
                .unwrap_err()
                .code,
            Code::NotFound
        );
    }

    #[test]
    fn imap_uid_state_assigns_stable_numeric_uids() {
        let (mut loom, ns) = mail_ns();
        inbox(&mut loom, ns);
        ingest_message(&mut loom, ns, "alice", "inbox", "alpha", RAW).unwrap();
        let raw2 = b"From: carol@x.io\r\nSubject: Lunch?\r\nDate: x\r\n\r\nbody2".to_vec();
        ingest_message(&mut loom, ns, "alice", "inbox", "zeta", &raw2).unwrap();

        let first = ensure_imap_uid_state(&mut loom, ns, "alice", "inbox").unwrap();
        let first_mappings = first
            .mappings
            .iter()
            .map(|mapping| (mapping.uid.as_str(), mapping.imap_uid))
            .collect::<Vec<_>>();
        assert_eq!(first_mappings, [("alpha", 1), ("zeta", 2)]);
        assert_ne!(first.uid_validity, 0);
        assert_eq!(first.uid_next, 3);

        delete_message(&mut loom, ns, "alice", "inbox", "alpha").unwrap();
        let second = ensure_imap_uid_state(&mut loom, ns, "alice", "inbox").unwrap();
        assert_eq!(second.uid_validity, first.uid_validity);
        assert_eq!(second.uid_next, 3);
        assert_eq!(second.mappings, first.mappings);

        let raw3 = b"From: dave@x.io\r\nSubject: Re\r\nDate: x\r\n\r\nb3".to_vec();
        ingest_message(&mut loom, ns, "alice", "inbox", "beta", &raw3).unwrap();
        let third = ensure_imap_uid_state(&mut loom, ns, "alice", "inbox").unwrap();
        let third_mappings = third
            .mappings
            .iter()
            .map(|mapping| (mapping.uid.as_str(), mapping.imap_uid))
            .collect::<Vec<_>>();
        assert_eq!(third_mappings, [("alpha", 1), ("beta", 3), ("zeta", 2)]);
        assert_eq!(third.uid_next, 4);
    }

    #[test]
    fn imap_subscriptions_are_principal_scoped_and_cleaned_up() {
        let (mut loom, ns) = mail_ns();
        inbox(&mut loom, ns);
        create_mailbox(
            &mut loom,
            ns,
            "alice",
            "archive",
            &MailboxMeta {
                display_name: "Archive".into(),
            },
        )
        .unwrap();

        assert!(subscribe_imap_mailbox(&mut loom, ns, "alice", "inbox").unwrap());
        assert!(!subscribe_imap_mailbox(&mut loom, ns, "alice", "inbox").unwrap());
        assert!(subscribe_imap_mailbox(&mut loom, ns, "alice", "archive").unwrap());
        assert_eq!(
            list_imap_subscriptions(&loom, ns, "alice").unwrap(),
            vec!["archive".to_string(), "inbox".to_string()]
        );

        assert!(unsubscribe_imap_mailbox(&mut loom, ns, "alice", "archive").unwrap());
        assert!(!unsubscribe_imap_mailbox(&mut loom, ns, "alice", "archive").unwrap());
        assert_eq!(
            list_imap_subscriptions(&loom, ns, "alice").unwrap(),
            vec!["inbox".to_string()]
        );
        delete_mailbox(&mut loom, ns, "alice", "inbox").unwrap();
        assert!(
            list_imap_subscriptions(&loom, ns, "alice")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            subscribe_imap_mailbox(&mut loom, ns, "alice", "missing")
                .unwrap_err()
                .code,
            Code::NotFound
        );
    }

    #[test]
    fn flag_policy_and_mutable_state_record_detailed_deltas() {
        let (mut loom, ns) = mail_ns();
        inbox(&mut loom, ns);
        let default_policy = get_flag_retention_policy(&loom, ns, "alice", "inbox").unwrap();
        assert_eq!(
            default_policy.detailed_delta_window_ms,
            DEFAULT_FLAG_DELTA_WINDOW_MS
        );
        assert_eq!(
            default_policy.max_detailed_deltas,
            DEFAULT_MAX_DETAILED_FLAG_DELTAS
        );
        let policy = MailFlagRetentionPolicy {
            detailed_delta_window_ms: 86_400_000,
            max_detailed_deltas: 32,
            audit_summary_class: "keyword-digests".to_string(),
            retained_gap_behavior: "full-resync-required".to_string(),
        };
        put_flag_retention_policy(&mut loom, ns, "alice", "inbox", &policy).unwrap();
        assert_eq!(
            get_flag_retention_policy(&loom, ns, "alice", "inbox").unwrap(),
            policy
        );

        ingest_message(&mut loom, ns, "alice", "inbox", "m1", RAW).unwrap();
        assert_eq!(
            get_mutable_state(&loom, ns, "alice", "inbox")
                .unwrap()
                .version,
            0
        );
        set_flags(&mut loom, ns, "alice", "inbox", "m1", &["\\Seen".into()]).unwrap();
        set_flags(&mut loom, ns, "alice", "inbox", "m1", &["\\Seen".into()]).unwrap();
        set_flags(
            &mut loom,
            ns,
            "alice",
            "inbox",
            "m1",
            &["Important".into(), "\\Seen".into()],
        )
        .unwrap();

        let state = get_mutable_state(&loom, ns, "alice", "inbox").unwrap();
        assert_eq!(state.version, 2);
        assert_eq!(
            mutable_state_version(&loom, ns, "alice", "inbox").unwrap(),
            2
        );
        assert_eq!(state.flag_deltas.len(), 2);
        assert_eq!(state.audit_summaries.len(), 2);
        assert_eq!(state.flag_deltas[0].version, 1);
        assert_eq!(state.flag_deltas[0].old_flags, Vec::<String>::new());
        assert_eq!(state.flag_deltas[0].new_flags, vec!["\\Seen".to_string()]);
        assert_eq!(state.audit_summaries[0].version, 1);
        assert_eq!(state.audit_summaries[0].uid, "m1");
        assert_ne!(
            state.audit_summaries[0].old_flags_digest,
            state.audit_summaries[0].new_flags_digest
        );
        assert_eq!(state.flag_deltas[1].version, 2);
        assert_eq!(state.flag_deltas[1].old_flags, vec!["\\Seen".to_string()]);
        assert_eq!(
            state.flag_deltas[1].new_flags,
            vec!["Important".to_string(), "\\Seen".to_string()]
        );
    }

    #[test]
    fn flag_ops_merge_by_keyword_and_observed_replacements_conflict() {
        let (mut loom, ns) = mail_ns();
        inbox(&mut loom, ns);
        ingest_message(&mut loom, ns, "alice", "inbox", "m1", RAW).unwrap();

        let flags = apply_flag_ops(
            &mut loom,
            ns,
            "alice",
            "inbox",
            "m1",
            &["\\Seen".to_string(), "Important".to_string()],
            &[],
        )
        .unwrap();
        assert_eq!(flags, vec!["Important".to_string(), "\\Seen".to_string()]);

        let flags = apply_flag_ops(
            &mut loom,
            ns,
            "alice",
            "inbox",
            "m1",
            &["\\Flagged".to_string()],
            &["\\Seen".to_string()],
        )
        .unwrap();
        assert_eq!(
            flags,
            vec!["Important".to_string(), "\\Flagged".to_string()]
        );

        assert_eq!(
            replace_flags_observed(
                &mut loom,
                ns,
                "alice",
                "inbox",
                "m1",
                0,
                &["Draft".to_string()],
            )
            .unwrap_err()
            .code,
            Code::Conflict
        );
        replace_flags_observed(
            &mut loom,
            ns,
            "alice",
            "inbox",
            "m1",
            2,
            &["Draft".to_string()],
        )
        .unwrap();

        let state = get_mutable_state(&loom, ns, "alice", "inbox").unwrap();
        assert_eq!(state.version, 3);
        assert_eq!(state.flag_deltas.len(), 3);
        assert_eq!(state.audit_summaries.len(), 3);
        assert_eq!(
            get_flags(&loom, ns, "alice", "inbox", "m1").unwrap(),
            vec!["Draft".to_string()]
        );
    }

    #[test]
    fn mutable_state_compaction_keeps_audit_and_reports_retained_gaps() {
        let (mut loom, ns) = mail_ns();
        inbox(&mut loom, ns);
        ingest_message(&mut loom, ns, "alice", "inbox", "m1", RAW).unwrap();
        set_flags(&mut loom, ns, "alice", "inbox", "m1", &["one".into()]).unwrap();
        set_flags(&mut loom, ns, "alice", "inbox", "m1", &["two".into()]).unwrap();
        set_flags(&mut loom, ns, "alice", "inbox", "m1", &["three".into()]).unwrap();

        let compacted = compact_mutable_state(&mut loom, ns, "alice", "inbox", 3).unwrap();

        assert_eq!(compacted.version, 3);
        assert_eq!(compacted.retained_since_version, 3);
        assert_eq!(compacted.flag_deltas.len(), 1);
        assert_eq!(compacted.flag_deltas[0].version, 3);
        assert_eq!(compacted.audit_summaries.len(), 3);
        assert_eq!(
            require_mutable_state_since(&loom, ns, "alice", "inbox", 2)
                .unwrap_err()
                .code,
            Code::RetainedGap
        );
        require_mutable_state_since(&loom, ns, "alice", "inbox", 3).unwrap();
        assert_eq!(
            mutable_state_changeset(&loom, ns, "alice", "inbox", 2)
                .unwrap_err()
                .code,
            Code::RetainedGap
        );
        let changes = mutable_state_changeset(&loom, ns, "alice", "inbox", 3).unwrap();
        assert_eq!(changes.gap_state, ChangeGapState::Retained);
        assert_eq!(changes.retained_low_water_mark, Some(3));
        assert_eq!(changes.items.len(), 1);
        assert_eq!(changes.items[0].sequence, Some(3));
    }

    #[test]
    fn account_usage_sums_owner_mailbox_message_octets_and_hard_limit() {
        let (mut loom, ns) = mail_ns();
        inbox(&mut loom, ns);
        create_mailbox(
            &mut loom,
            ns,
            "alice",
            "archive",
            &MailboxMeta {
                display_name: "Archive".into(),
            },
        )
        .unwrap();
        create_mailbox(
            &mut loom,
            ns,
            "bob",
            "inbox",
            &MailboxMeta {
                display_name: "Inbox".into(),
            },
        )
        .unwrap();
        let raw2 = b"From: carol@x.io\r\nSubject: Lunch?\r\nDate: x\r\n\r\nbody2";
        let bob_raw = b"From: bob@x.io\r\nSubject: Private\r\nDate: x\r\n\r\nbody";

        ingest_message(&mut loom, ns, "alice", "inbox", "m1", RAW).unwrap();
        ingest_message(&mut loom, ns, "alice", "archive", "m2", raw2).unwrap();
        ingest_message(&mut loom, ns, "bob", "inbox", "b1", bob_raw).unwrap();
        set_account_hard_limit(&mut loom, ns, "alice", Some(10_000)).unwrap();

        let usage = account_usage(&loom, ns, "alice").unwrap();
        assert_eq!(usage.used_octets, (RAW.len() + raw2.len()) as u64);
        assert_eq!(usage.hard_limit_octets, Some(10_000));
        assert_eq!(
            account_usage(&loom, ns, "bob").unwrap().used_octets,
            bob_raw.len() as u64
        );

        delete_message(&mut loom, ns, "alice", "inbox", "m1").unwrap();
        assert_eq!(
            account_usage(&loom, ns, "alice").unwrap().used_octets,
            raw2.len() as u64
        );
        set_account_hard_limit(&mut loom, ns, "alice", None).unwrap();
        assert_eq!(get_account_hard_limit(&loom, ns, "alice").unwrap(), None);
    }

    #[test]
    fn search_diff_and_versioning() {
        let (mut loom, ns) = mail_ns();
        inbox(&mut loom, ns);
        ingest_message(&mut loom, ns, "alice", "inbox", "m1", RAW).unwrap();
        let raw2 = b"From: carol@x.io\r\nSubject: Lunch?\r\nDate: x\r\n\r\nbody2".to_vec();
        ingest_message(&mut loom, ns, "alice", "inbox", "m2", &raw2).unwrap();

        assert_eq!(
            search(&loom, ns, "alice", "inbox", "lunch").unwrap().len(),
            1
        );
        assert_eq!(
            search(&loom, ns, "alice", "inbox", "bob@").unwrap().len(),
            1
        );

        let old = list_messages(&loom, ns, "alice", "inbox").unwrap();
        let c1 = loom.commit(ns, "alice", "two messages", 1).unwrap();
        delete_message(&mut loom, ns, "alice", "inbox", "m2").unwrap();
        let raw3 = b"From: dave@x.io\r\nSubject: Re\r\nDate: x\r\n\r\nb3".to_vec();
        ingest_message(&mut loom, ns, "alice", "inbox", "m3", &raw3).unwrap();
        let new = list_messages(&loom, ns, "alice", "inbox").unwrap();
        let changes = diff_messages(&old, &new);
        let summary: Vec<(&str, ChangeKind)> =
            changes.iter().map(|c| (c.uid.as_str(), c.kind)).collect();
        assert_eq!(
            summary,
            [("m2", ChangeKind::Removed), ("m3", ChangeKind::Added)]
        );

        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(list_messages(&loom, ns, "alice", "inbox").unwrap().len(), 2);
    }
}
