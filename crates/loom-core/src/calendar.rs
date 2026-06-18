//! The calendar facet: structured iCalendar records as the source of truth.
//!
//! An entry is a typed [`CalendarEntry`] record (not raw `.ics` bytes): iCalendar text, the mounted
//! `.ics` file, and the hosted CalDAV body are all serialized from it on demand. Entries live per
//! principal and collection, one record per `UID`, at the reserved path
//! `calendar/<principal>/<collection>/<uid>`. The ETag is the content address of the canonical record;
//! the collection sync-token is a commit.
//!
//! Pure-Rust, `wasm32`-clean, deterministic. Recurrence expansion for range queries reuses the owned
//! `loom-rrule` engine, so this facet embeds no timezone database.

use crate::acl::AclRight;
use crate::change_set::{ChangeCursor, ChangeGapState, ChangeItem, ChangeItemKind, ChangeSet};
use crate::digest::Digest;
use crate::error::{Code, LoomError, Result};
use crate::hooks::{PimEventEnvelope, hook_emit_event_unchecked};
use crate::object::content_address_with;
use crate::provider::ObjectStore;
use crate::vcs::{Loom, StagedEntry};
use crate::workspace::{FacetKind, WorkspaceId, facet_path};
pub use loom_pim::calendar::{
    CalendarEntry, CollectionMeta, Component, ComponentField, DateTime, IcalDate, IcalMonth,
    IcalTime,
};
use time::PrimitiveDateTime;

/// The on-demand iCalendar (`.ics`) projection of the entry at `uid`, or `None` if absent. The
/// filesystem mount and CalDAV `GET` serve exactly these bytes.
pub fn entry_ics<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
    uid: &str,
) -> Result<Option<String>> {
    Ok(get_entry(loom, ns, principal, collection, uid)?.map(|e| e.to_ics()))
}

/// Parse an iCalendar document and store it as a record in `collection` (the validated write-in path for
/// the mount and CalDAV `PUT`); returns the new ETag.
pub fn put_ics<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
    ics: &str,
) -> Result<Digest> {
    let entry = CalendarEntry::from_ics(ics)?;
    put_entry(loom, ns, principal, collection, &entry)
}

const META_FILE: &str = ".collection";

fn validate_segment(seg: &str, what: &str) -> Result<()> {
    if seg.is_empty() || seg == "." || seg == ".." || seg.contains('/') || seg.starts_with('.') {
        return Err(LoomError::invalid(format!(
            "calendar: invalid {what} segment {seg:?}"
        )));
    }
    Ok(())
}

fn collection_dir(principal: &str, collection: &str) -> String {
    facet_path(FacetKind::Calendar, &format!("{principal}/{collection}"))
}

fn collection_scope(principal: &str, collection: &str) -> String {
    format!("{principal}/{collection}")
}

fn principal_scope(principal: &str) -> String {
    format!("{principal}/")
}

fn meta_path(principal: &str, collection: &str) -> String {
    format!("{}/{META_FILE}", collection_dir(principal, collection))
}

fn entry_path(principal: &str, collection: &str, uid: &str) -> String {
    format!(
        "{}/{}",
        collection_dir(principal, collection),
        hex::encode(uid.as_bytes())
    )
}

/// Create (or replace the metadata of) a collection under `principal`. Idempotent on the path; a later
/// call updates the metadata.
pub fn create_collection<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
    meta: &CollectionMeta,
) -> Result<()> {
    validate_segment(principal, "principal")?;
    validate_segment(collection, "collection")?;
    loom.authorize_collection(
        ns,
        FacetKind::Calendar,
        &collection_scope(principal, collection),
        AclRight::Write,
    )?;
    loom.create_directory_reserved(ns, &collection_dir(principal, collection), true)?;
    loom.write_file_reserved(
        ns,
        &meta_path(principal, collection),
        &meta.encode(),
        0o100644,
    )
}

/// The metadata of a collection, or `None` if it does not exist.
pub fn get_collection<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
) -> Result<Option<CollectionMeta>> {
    loom.authorize_collection(
        ns,
        FacetKind::Calendar,
        &collection_scope(principal, collection),
        AclRight::Read,
    )?;
    get_collection_unchecked(loom, ns, principal, collection)
}

fn get_collection_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
) -> Result<Option<CollectionMeta>> {
    match loom.read_file_reserved(ns, &meta_path(principal, collection)) {
        Ok(bytes) => Ok(Some(CollectionMeta::decode(&bytes)?)),
        Err(e) if e.code == Code::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Collection ids under `principal`, sorted. A collection is present once it has a metadata file.
pub fn list_collections<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
) -> Result<Vec<String>> {
    loom.authorize_collection(
        ns,
        FacetKind::Calendar,
        &principal_scope(principal),
        AclRight::Read,
    )?;
    let prefix = format!("{}/", facet_path(FacetKind::Calendar, principal));
    let suffix = format!("/{META_FILE}");
    let mut out: Vec<String> = loom
        .staged_paths(ns)
        .into_iter()
        .filter_map(|p| {
            let rest = p.strip_prefix(&prefix)?;
            let col = rest.strip_suffix(&suffix)?;
            if col.contains('/') {
                return None;
            }
            Some(col.to_string())
        })
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// Delete a collection and every entry in it; returns whether it existed.
pub fn delete_collection<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
) -> Result<bool> {
    loom.authorize_collection(
        ns,
        FacetKind::Calendar,
        &collection_scope(principal, collection),
        AclRight::Write,
    )?;
    let prefix = format!("{}/", collection_dir(principal, collection));
    let paths: Vec<String> = loom
        .staged_paths(ns)
        .into_iter()
        .filter(|p| p.starts_with(&prefix))
        .collect();
    let existed = !paths.is_empty();
    for p in paths {
        loom.remove_file_reserved(ns, &p)?;
    }
    Ok(existed)
}

/// Require that a collection exists, returning its metadata or `NOT_FOUND` (CalDAV requires the
/// calendar be created before resources are PUT into it).
fn require_collection<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
) -> Result<CollectionMeta> {
    get_collection_unchecked(loom, ns, principal, collection)?.ok_or_else(|| {
        LoomError::not_found(format!(
            "calendar: collection {principal}/{collection} does not exist"
        ))
    })
}

/// The ETag of a record: the content address of its canonical bytes under the store's digest profile.
pub fn entry_etag<S: ObjectStore>(loom: &Loom<S>, entry: &CalendarEntry) -> Digest {
    content_address_with(loom.store().digest_algo(), &entry.encode())
}

/// Put `entry` into an existing collection, keyed by its `UID`; returns the new ETag. A later put at the
/// same UID replaces the record.
pub fn put_entry<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
    entry: &CalendarEntry,
) -> Result<Digest> {
    validate_segment(principal, "principal")?;
    validate_segment(collection, "collection")?;
    loom.authorize_collection(
        ns,
        FacetKind::Calendar,
        &collection_scope(principal, collection),
        AclRight::Write,
    )?;
    if entry.uid.is_empty() {
        return Err(LoomError::invalid("calendar: entry UID must not be empty"));
    }
    if !entry.has_valid_dtstart() {
        return Err(LoomError::invalid(format!(
            "calendar: bad DTSTART {:?}",
            entry.dtstart
        )));
    }
    require_collection(loom, ns, principal, collection)?;
    let path = entry_path(principal, collection, &entry.uid);
    let before = match loom.read_file_reserved(ns, &path) {
        Ok(bytes) => Some(bytes),
        Err(err) if err.code == Code::NotFound => None,
        Err(err) => return Err(err),
    };
    let bytes = entry.encode();
    let lifecycle_event = if before.is_some() {
        "before_update"
    } else {
        "before_create"
    };
    emit_calendar_event(
        loom,
        ns,
        lifecycle_event,
        principal,
        collection,
        &entry.uid,
        (before.clone(), Some(bytes.clone())),
    )?;
    let etag = content_address_with(loom.store().digest_algo(), &bytes);
    loom.write_file_reserved(ns, &path, &bytes, 0o100644)?;
    let lifecycle_event = if before.is_some() {
        "after_update"
    } else {
        "after_create"
    };
    emit_calendar_event(
        loom,
        ns,
        lifecycle_event,
        principal,
        collection,
        &entry.uid,
        (before.clone(), Some(bytes.clone())),
    )?;
    let domain_event = if before.is_none() {
        "on_event_added"
    } else if entry.status.as_deref() == Some("CANCELLED") {
        "on_event_cancelled"
    } else {
        "on_event_updated"
    };
    emit_calendar_event(
        loom,
        ns,
        domain_event,
        principal,
        collection,
        &entry.uid,
        (before, Some(bytes)),
    )?;
    Ok(etag)
}

/// The entry at `uid`, or `None` if the uid or collection is absent.
pub fn get_entry<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
    uid: &str,
) -> Result<Option<CalendarEntry>> {
    loom.authorize_collection(
        ns,
        FacetKind::Calendar,
        &collection_scope(principal, collection),
        AclRight::Read,
    )?;
    get_entry_unchecked(loom, ns, principal, collection, uid)
}

fn get_entry_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
    uid: &str,
) -> Result<Option<CalendarEntry>> {
    match loom.read_file_reserved(ns, &entry_path(principal, collection, uid)) {
        Ok(bytes) => Ok(Some(CalendarEntry::decode(&bytes)?)),
        Err(e) if e.code == Code::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Remove the entry at `uid`; returns whether it was present.
pub fn delete_entry<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
    uid: &str,
) -> Result<bool> {
    loom.authorize_collection(
        ns,
        FacetKind::Calendar,
        &collection_scope(principal, collection),
        AclRight::Write,
    )?;
    let path = entry_path(principal, collection, uid);
    let before = match loom.read_file_reserved(ns, &path) {
        Ok(bytes) => Some(bytes),
        Err(err) if err.code == Code::NotFound => None,
        Err(err) => return Err(err),
    };
    if let Some(bytes) = before {
        emit_calendar_event(
            loom,
            ns,
            "before_delete",
            principal,
            collection,
            uid,
            (Some(bytes.clone()), None),
        )?;
        loom.remove_file_reserved(ns, &path)?;
        emit_calendar_event(
            loom,
            ns,
            "after_delete",
            principal,
            collection,
            uid,
            (Some(bytes.clone()), None),
        )?;
        emit_calendar_event(
            loom,
            ns,
            "on_event_cancelled",
            principal,
            collection,
            uid,
            (Some(bytes), None),
        )?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn emit_calendar_event<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    event: &str,
    principal: &str,
    collection: &str,
    uid: &str,
    bodies: (Option<Vec<u8>>, Option<Vec<u8>>),
) -> Result<()> {
    let (before, after) = bodies;
    hook_emit_event_unchecked(
        loom,
        ns,
        &PimEventEnvelope {
            workspace: ns,
            facet: FacetKind::Calendar,
            event: event.to_string(),
            principal: principal.to_string(),
            collection: Some(collection.to_string()),
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

/// All entries in a collection, sorted by `UID`. The `.collection` metadata file is skipped.
pub fn list_entries<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
) -> Result<Vec<CalendarEntry>> {
    loom.authorize_collection(
        ns,
        FacetKind::Calendar,
        &collection_scope(principal, collection),
        AclRight::Read,
    )?;
    list_entries_unchecked(loom, ns, principal, collection)
}

/// All entries in a collection as of `commit`, sorted by `UID`.
pub fn list_entries_at_commit<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    commit: Digest,
    principal: &str,
    collection: &str,
) -> Result<Vec<CalendarEntry>> {
    loom.authorize_collection(
        ns,
        FacetKind::Calendar,
        &collection_scope(principal, collection),
        AclRight::Read,
    )?;
    let prefix = format!("{}/", collection_dir(principal, collection));
    let (files, _) = loom.flatten_commit(commit)?;
    let mut entries = Vec::new();
    for (path, staged) in files {
        let Some(seg) = path.strip_prefix(&prefix) else {
            continue;
        };
        if seg.contains('/') || seg == META_FILE {
            continue;
        }
        let StagedEntry::File(file) = staged else {
            continue;
        };
        entries.push(CalendarEntry::decode(
            &loom.load_content(file.content_addr)?,
        )?);
    }
    entries.sort_by(|a, b| a.uid.cmp(&b.uid));
    Ok(entries)
}

fn list_entries_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
) -> Result<Vec<CalendarEntry>> {
    let prefix = format!("{}/", collection_dir(principal, collection));
    let mut entries: Vec<CalendarEntry> = Vec::new();
    for p in loom.staged_paths(ns) {
        let Some(seg) = p.strip_prefix(&prefix) else {
            continue;
        };
        if seg.contains('/') || seg == META_FILE {
            continue;
        }
        entries.push(CalendarEntry::decode(&loom.read_file_reserved(ns, &p)?)?);
    }
    entries.sort_by(|a, b| a.uid.cmp(&b.uid));
    Ok(entries)
}

/// One expanded occurrence: the originating entry's `UID` and the occurrence start (wall-clock).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub uid: String,
    pub start: PrimitiveDateTime,
}

/// Expand every entry in a collection into occurrences within the half-open window `[from, to)`,
/// ordered by start then UID. Recurring entries are expanded via `loom-rrule`; single entries
/// contribute their one start if in window. This is derived on demand from the stored records (no
/// materialized index), so it is always consistent with them.
pub fn range<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
    from: PrimitiveDateTime,
    to: PrimitiveDateTime,
) -> Result<Vec<Occurrence>> {
    let mut out: Vec<Occurrence> = Vec::new();
    for entry in list_entries(loom, ns, principal, collection)? {
        for start in entry.occurrence_starts(from, to)? {
            out.push(Occurrence {
                uid: entry.uid.clone(),
                start,
            });
        }
    }
    out.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.uid.cmp(&b.uid)));
    Ok(out)
}

/// Search entries in a collection by component class and/or a case-insensitive substring over
/// `summary`; a fuller `comp-filter`/`prop-filter` lands with the
/// CalDAV projection). Either filter is optional; results are UID-ordered.
pub fn search<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
    component: Option<Component>,
    text: Option<&str>,
) -> Result<Vec<CalendarEntry>> {
    let needle = text.map(str::to_lowercase);
    Ok(list_entries(loom, ns, principal, collection)?
        .into_iter()
        .filter(|e| component.is_none_or(|c| c == e.component.0))
        .filter(|e| {
            needle
                .as_ref()
                .is_none_or(|n| e.summary.to_lowercase().contains(n))
        })
        .collect())
}

/// A single entry's change between two collection states: the UID and what happened to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryChange {
    pub uid: String,
    pub kind: ChangeKind,
    /// The new ETag for added/updated entries; `None` for removed.
    pub etag: Option<Digest>,
}

/// The nature of an entry change in a collection diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Updated,
    Removed,
}

/// Compute the per-UID changes from `old` to `new` collection states (each a UID-ordered entry list),
/// the structured form of a CalDAV `sync-collection` diff. UID is the unit; the ETag (content
/// address) decides whether a present entry changed.
pub fn diff_entries<S: ObjectStore>(
    loom: &Loom<S>,
    old: &[CalendarEntry],
    new: &[CalendarEntry],
) -> Vec<EntryChange> {
    use std::collections::BTreeMap;
    let algo = loom.store().digest_algo();
    let etag = |e: &CalendarEntry| content_address_with(algo, &e.encode());
    let old_map: BTreeMap<&str, Digest> = old.iter().map(|e| (e.uid.as_str(), etag(e))).collect();
    let new_map: BTreeMap<&str, Digest> = new.iter().map(|e| (e.uid.as_str(), etag(e))).collect();
    let mut out = Vec::new();
    for (uid, new_tag) in &new_map {
        match old_map.get(uid) {
            None => out.push(EntryChange {
                uid: (*uid).to_string(),
                kind: ChangeKind::Added,
                etag: Some(*new_tag),
            }),
            Some(old_tag) if old_tag != new_tag => out.push(EntryChange {
                uid: (*uid).to_string(),
                kind: ChangeKind::Updated,
                etag: Some(*new_tag),
            }),
            Some(_) => {}
        }
    }
    for uid in old_map.keys() {
        if !new_map.contains_key(uid) {
            out.push(EntryChange {
                uid: (*uid).to_string(),
                kind: ChangeKind::Removed,
                etag: None,
            });
        }
    }
    out.sort_by(|a, b| a.uid.cmp(&b.uid));
    out
}

pub fn entry_changeset(
    ns: WorkspaceId,
    principal: &str,
    collection: &str,
    next_version: u64,
    retained_since_version: Option<u64>,
    changes: Vec<EntryChange>,
) -> Result<ChangeSet> {
    let scope = calendar_change_scope(ns, principal, collection);
    let items = changes
        .into_iter()
        .map(|change| ChangeItem::item_diff(change.uid, change_item_kind(change.kind), change.etag))
        .collect();
    ChangeSet::new(
        scope.clone(),
        ChangeGapState::Retained,
        retained_since_version,
        ChangeCursor::sequence(scope, next_version),
        items,
    )
}

pub fn calendar_change_scope(ns: WorkspaceId, principal: &str, collection: &str) -> String {
    format!(
        "calendar:{}:{principal}/{collection}",
        hex::encode(ns.as_bytes())
    )
}

fn change_item_kind(kind: ChangeKind) -> ChangeItemKind {
    match kind {
        ChangeKind::Added => ChangeItemKind::Added,
        ChangeKind::Updated => ChangeItemKind::Updated,
        ChangeKind::Removed => ChangeItemKind::Removed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acl::{AclRight, AclSubject};
    use crate::error::Code;
    use crate::identity::IdentityStore;
    use crate::provider::memory::MemoryStore;
    use time::{Date, Month, Time};

    fn cal_ns() -> (Loom<MemoryStore>, WorkspaceId) {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Calendar, None, WorkspaceId::from_bytes([7; 16]))
            .unwrap();
        (loom, ns)
    }

    fn work_collection(loom: &mut Loom<MemoryStore>, ns: WorkspaceId) {
        create_collection(
            loom,
            ns,
            "alice",
            "work",
            &CollectionMeta {
                display_name: "Work".into(),
                component_set: vec![Component::Event],
            },
        )
        .unwrap();
    }

    #[test]
    fn authenticated_calendar_operations_are_acl_checked() {
        let (mut loom, ns) = cal_ns();
        let root = WorkspaceId::from_bytes([1; 16]);
        let mut identity = IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);

        assert_eq!(
            create_collection(
                &mut loom,
                ns,
                "alice",
                "work",
                &CollectionMeta {
                    display_name: "Work".into(),
                    component_set: vec![Component::Event],
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
                Some(FacetKind::Calendar),
                [AclRight::Write, AclRight::Read],
            )
            .unwrap();

        work_collection(&mut loom, ns);
        assert_eq!(
            list_collections(&loom, ns, "alice").unwrap(),
            vec!["work".to_string()]
        );
    }

    fn dt(y: i32, m: u8, d: u8, hh: u8, mm: u8) -> PrimitiveDateTime {
        PrimitiveDateTime::new(
            Date::from_calendar_date(y, Month::try_from(m).unwrap(), d).unwrap(),
            Time::from_hms(hh, mm, 0).unwrap(),
        )
    }

    #[test]
    fn entry_encode_round_trips() {
        let mut e = CalendarEntry::event("u1", "Standup", "20240101T090000");
        e.rrule = Some("FREQ=WEEKLY;BYDAY=MO".into());
        e.exdate = vec!["20240115T090000".into()];
        e.tzid = Some("America/New_York".into());
        e.extra = vec![("X-FOO".into(), "bar".into())];
        let back = CalendarEntry::decode(&e.encode()).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn collection_lifecycle() {
        let (mut loom, ns) = cal_ns();
        assert!(
            get_collection(&loom, ns, "alice", "work")
                .unwrap()
                .is_none()
        );
        work_collection(&mut loom, ns);
        assert_eq!(
            get_collection(&loom, ns, "alice", "work")
                .unwrap()
                .unwrap()
                .display_name,
            "Work"
        );
        assert_eq!(
            list_collections(&loom, ns, "alice").unwrap(),
            vec!["work".to_string()]
        );
        assert!(delete_collection(&mut loom, ns, "alice", "work").unwrap());
        assert!(
            get_collection(&loom, ns, "alice", "work")
                .unwrap()
                .is_none()
        );
        assert!(list_collections(&loom, ns, "alice").unwrap().is_empty());
    }

    #[test]
    fn entry_crud_and_etag_changes_on_edit() {
        let (mut loom, ns) = cal_ns();
        work_collection(&mut loom, ns);
        // Put into a missing collection fails; into the existing one succeeds.
        let e1 = CalendarEntry::event("u1", "Standup", "20240101T090000");
        let tag1 = put_entry(&mut loom, ns, "alice", "work", &e1).unwrap();
        assert_eq!(
            get_entry(&loom, ns, "alice", "work", "u1")
                .unwrap()
                .unwrap()
                .summary,
            "Standup"
        );
        // Editing the record changes the ETag.
        let mut e2 = e1.clone();
        e2.summary = "Daily standup".into();
        let tag2 = put_entry(&mut loom, ns, "alice", "work", &e2).unwrap();
        assert_ne!(tag1, tag2);
        assert_eq!(list_entries(&loom, ns, "alice", "work").unwrap().len(), 1);
        assert!(delete_entry(&mut loom, ns, "alice", "work", "u1").unwrap());
        assert!(!delete_entry(&mut loom, ns, "alice", "work", "u1").unwrap());
    }

    #[test]
    fn put_into_missing_collection_is_not_found() {
        let (mut loom, ns) = cal_ns();
        let e = CalendarEntry::event("u1", "X", "20240101T090000");
        let err = put_entry(&mut loom, ns, "alice", "work", &e).unwrap_err();
        assert_eq!(err.code, Code::NotFound);
    }

    #[test]
    fn entries_version_with_commits() {
        let (mut loom, ns) = cal_ns();
        work_collection(&mut loom, ns);
        put_entry(
            &mut loom,
            ns,
            "alice",
            "work",
            &CalendarEntry::event("u1", "A", "20240101T090000"),
        )
        .unwrap();
        let c1 = loom.commit(ns, "alice", "one event", 1).unwrap();
        put_entry(
            &mut loom,
            ns,
            "alice",
            "work",
            &CalendarEntry::event("u2", "B", "20240102T090000"),
        )
        .unwrap();
        loom.commit(ns, "alice", "two events", 2).unwrap();
        assert_eq!(list_entries(&loom, ns, "alice", "work").unwrap().len(), 2);
        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(list_entries(&loom, ns, "alice", "work").unwrap().len(), 1);
    }

    #[test]
    fn range_expands_recurrence() {
        let (mut loom, ns) = cal_ns();
        work_collection(&mut loom, ns);
        // 2024-01-01 is a Monday; weekly on Monday, drop the 3rd via EXDATE.
        let mut weekly = CalendarEntry::event("u1", "Standup", "20240101T090000");
        weekly.rrule = Some("FREQ=WEEKLY;BYDAY=MO".into());
        weekly.exdate = vec!["20240115T090000".into()];
        put_entry(&mut loom, ns, "alice", "work", &weekly).unwrap();
        // A one-off on the 10th.
        put_entry(
            &mut loom,
            ns,
            "alice",
            "work",
            &CalendarEntry::event("u2", "Review", "20240110T140000"),
        )
        .unwrap();

        let occ = range(
            &loom,
            ns,
            "alice",
            "work",
            dt(2024, 1, 1, 0, 0),
            dt(2024, 2, 1, 0, 0),
        )
        .unwrap();
        let got: Vec<(u8, &str)> = occ
            .iter()
            .map(|o| (o.start.day(), o.uid.as_str()))
            .collect();
        // Mondays 1, 8, 22, 29 (15 excluded) for u1, plus the 10th for u2, ordered by start.
        assert_eq!(
            got,
            [(1, "u1"), (8, "u1"), (10, "u2"), (22, "u1"), (29, "u1")]
        );
    }

    #[test]
    fn search_by_component_and_text() {
        let (mut loom, ns) = cal_ns();
        work_collection(&mut loom, ns);
        put_entry(
            &mut loom,
            ns,
            "alice",
            "work",
            &CalendarEntry::event("u1", "Team Standup", "20240101T090000"),
        )
        .unwrap();
        let mut todo = CalendarEntry::event("u2", "Write report", "20240102T090000");
        todo.component = ComponentField(Component::Todo);
        put_entry(&mut loom, ns, "alice", "work", &todo).unwrap();

        assert_eq!(
            search(&loom, ns, "alice", "work", Some(Component::Todo), None)
                .unwrap()
                .len(),
            1
        );
        let hits = search(&loom, ns, "alice", "work", None, Some("stand")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].uid, "u1");
    }

    #[test]
    fn ics_round_trips_semantically() {
        let mut e = CalendarEntry::event("u1@loom", "Lunch, with team; offsite", "20240101T090000");
        e.dtend = Some("20240101T100000".into());
        e.tzid = Some("America/New_York".into());
        e.rrule = Some("FREQ=WEEKLY;BYDAY=MO".into());
        e.rdate = vec!["20240122T090000".into()];
        e.exdate = vec!["20240115T090000".into()];
        e.status = Some("CONFIRMED".into());
        e.extra = vec![
            ("DESCRIPTION".into(), "Discuss roadmap\nand budget".into()),
            ("SEQUENCE".into(), "7".into()),
            ("X-LOOM-ROOM".into(), "C-4".into()),
        ];
        let ics = e.to_ics();
        // icalendar owns the exact wire format; we assert the structural anchors and a full semantic
        // round-trip rather than byte-exact escaping/ordering.
        assert!(ics.contains("BEGIN:VCALENDAR") && ics.contains("BEGIN:VEVENT"));
        assert!(ics.contains("TZID=America/New_York"));
        assert!(ics.contains("RDATE"));
        assert!(ics.contains("EXDATE"));
        assert!(ics.contains("SEQUENCE"));
        assert!(ics.contains("DESCRIPTION"));
        let back = CalendarEntry::from_ics(&ics).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn ics_todo_and_long_line_fold() {
        let mut e = CalendarEntry::event("u2", "x".repeat(200), "20240101T090000");
        e.component = ComponentField(Component::Todo);
        let ics = e.to_ics();
        assert!(ics.contains("BEGIN:VTODO"));
        // icalendar folds long lines (continuation lines begin with a space); the 200-char SUMMARY
        // survives a full round-trip regardless of where the folds land.
        assert!(ics.contains("\r\n "), "a 200-char summary must be folded");
        assert_eq!(CalendarEntry::from_ics(&ics).unwrap(), e);
    }

    #[test]
    fn vtodo_without_dtstart_round_trips_and_stores() {
        let (mut loom, ns) = cal_ns();
        work_collection(&mut loom, ns);
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTODO\r\nUID:reminder-1\r\nSUMMARY:Call back\r\nDUE:20260708T170000Z\r\nSTATUS:NEEDS-ACTION\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";

        put_ics(&mut loom, ns, "alice", "work", ics).unwrap();
        let entry = get_entry(&loom, ns, "alice", "work", "reminder-1")
            .unwrap()
            .unwrap();

        assert_eq!(entry.component.0, Component::Todo);
        assert_eq!(entry.dtstart, "");
        assert_eq!(entry.status.as_deref(), Some("NEEDS-ACTION"));
        assert!(
            entry
                .extra
                .iter()
                .any(|(key, value)| key == "DUE" && value == "20260708T170000Z")
        );
        let out = entry_ics(&loom, ns, "alice", "work", "reminder-1")
            .unwrap()
            .unwrap();
        assert!(out.contains("BEGIN:VTODO"));
        assert!(!out.contains("DTSTART"));
        assert!(out.contains("DUE"));
    }

    #[test]
    fn put_and_get_ics_through_the_facet() {
        let (mut loom, ns) = cal_ns();
        work_collection(&mut loom, ns);
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:imported\r\nSUMMARY:Imported\r\nDTSTART:20240301T120000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        put_ics(&mut loom, ns, "alice", "work", ics).unwrap();
        let out = entry_ics(&loom, ns, "alice", "work", "imported")
            .unwrap()
            .unwrap();
        assert!(out.contains("UID:imported"));
        assert!(out.contains("DTSTART:20240301T120000Z"));
    }

    #[test]
    fn diff_reports_added_updated_removed() {
        let (mut loom, ns) = cal_ns();
        work_collection(&mut loom, ns);
        let a = CalendarEntry::event("u1", "A", "20240101T090000");
        let b = CalendarEntry::event("u2", "B", "20240102T090000");
        put_entry(&mut loom, ns, "alice", "work", &a).unwrap();
        put_entry(&mut loom, ns, "alice", "work", &b).unwrap();
        let old = list_entries(&loom, ns, "alice", "work").unwrap();

        // Update u1, remove u2, add u3.
        let mut a2 = a.clone();
        a2.summary = "A2".into();
        put_entry(&mut loom, ns, "alice", "work", &a2).unwrap();
        delete_entry(&mut loom, ns, "alice", "work", "u2").unwrap();
        put_entry(
            &mut loom,
            ns,
            "alice",
            "work",
            &CalendarEntry::event("u3", "C", "20240103T090000"),
        )
        .unwrap();
        let new = list_entries(&loom, ns, "alice", "work").unwrap();

        let changes = diff_entries(&loom, &old, &new);
        let summary: Vec<(&str, ChangeKind)> =
            changes.iter().map(|c| (c.uid.as_str(), c.kind)).collect();
        assert_eq!(
            summary,
            [
                ("u1", ChangeKind::Updated),
                ("u2", ChangeKind::Removed),
                ("u3", ChangeKind::Added)
            ]
        );
        let change_set = entry_changeset(ns, "alice", "work", 4, Some(1), changes).unwrap();
        assert_eq!(change_set.gap_state, ChangeGapState::Retained);
        assert_eq!(change_set.retained_low_water_mark, Some(1));
        assert_eq!(change_set.items.len(), 3);
        assert_eq!(change_set.items[0].id, "u1");
        assert_eq!(change_set.items[1].kind, ChangeItemKind::Removed);
    }
}
