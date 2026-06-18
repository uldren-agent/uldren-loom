//! The contacts facet: structured vCard records as the source of truth.
//!
//! A contact is a typed [`ContactEntry`] record (not raw `.vcf` bytes): vCard text, the mounted `.vcf`
//! file, and the hosted CardDAV body are serialized from it on demand. Contacts live per principal and
//! address book, one record per `UID`, at the reserved path
//! `contacts/<principal>/<book>/<uid>`. The ETag is the content
//! address of the canonical record. Pure-Rust, `wasm32`-clean, deterministic.
//!
//! This mirrors the calendar facet's structured-record model; it has no time/recurrence dimension.

use crate::acl::AclRight;
use crate::change_set::{ChangeCursor, ChangeGapState, ChangeItem, ChangeItemKind, ChangeSet};
use crate::digest::Digest;
use crate::error::{Code, LoomError, Result};
use crate::hooks::{PimEventEnvelope, hook_emit_event_unchecked};
use crate::object::content_address_with;
use crate::provider::ObjectStore;
use crate::vcs::{Loom, StagedEntry};
use crate::workspace::{FacetKind, WorkspaceId, facet_path};
pub use loom_pim::contacts::{BookMeta, ContactEntry, TypedValue};

const META_FILE: &str = ".collection";

fn validate_segment(seg: &str, what: &str) -> Result<()> {
    if seg.is_empty() || seg == "." || seg == ".." || seg.contains('/') || seg.starts_with('.') {
        return Err(LoomError::invalid(format!(
            "contacts: invalid {what} segment {seg:?}"
        )));
    }
    Ok(())
}

fn book_dir(principal: &str, book: &str) -> String {
    facet_path(FacetKind::Contacts, &format!("{principal}/{book}"))
}

fn book_scope(principal: &str, book: &str) -> String {
    format!("{principal}/{book}")
}

fn principal_scope(principal: &str) -> String {
    format!("{principal}/")
}

fn meta_path(principal: &str, book: &str) -> String {
    format!("{}/{META_FILE}", book_dir(principal, book))
}

fn entry_path(principal: &str, book: &str, uid: &str) -> String {
    format!(
        "{}/{}",
        book_dir(principal, book),
        hex::encode(uid.as_bytes())
    )
}

/// Create (or update the metadata of) an address book under `principal`.
pub fn create_book<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
    meta: &BookMeta,
) -> Result<()> {
    validate_segment(principal, "principal")?;
    validate_segment(book, "book")?;
    loom.authorize_collection(
        ns,
        FacetKind::Contacts,
        &book_scope(principal, book),
        AclRight::Write,
    )?;
    loom.create_directory_reserved(ns, &book_dir(principal, book), true)?;
    loom.write_file_reserved(ns, &meta_path(principal, book), &meta.encode(), 0o100644)
}

/// The metadata of an address book, or `None` if absent.
pub fn get_book<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
) -> Result<Option<BookMeta>> {
    loom.authorize_collection(
        ns,
        FacetKind::Contacts,
        &book_scope(principal, book),
        AclRight::Read,
    )?;
    get_book_unchecked(loom, ns, principal, book)
}

fn get_book_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
) -> Result<Option<BookMeta>> {
    match loom.read_file_reserved(ns, &meta_path(principal, book)) {
        Ok(bytes) => Ok(Some(BookMeta::decode(&bytes)?)),
        Err(e) if e.code == Code::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Address-book ids under `principal`, sorted.
pub fn list_books<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
) -> Result<Vec<String>> {
    loom.authorize_collection(
        ns,
        FacetKind::Contacts,
        &principal_scope(principal),
        AclRight::Read,
    )?;
    let prefix = format!("{}/", facet_path(FacetKind::Contacts, principal));
    let suffix = format!("/{META_FILE}");
    let mut out: Vec<String> = loom
        .staged_paths(ns)
        .into_iter()
        .filter_map(|p| {
            let rest = p.strip_prefix(&prefix)?;
            let book = rest.strip_suffix(&suffix)?;
            if book.contains('/') {
                return None;
            }
            Some(book.to_string())
        })
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// Delete an address book and every contact in it; returns whether it existed.
pub fn delete_book<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
) -> Result<bool> {
    loom.authorize_collection(
        ns,
        FacetKind::Contacts,
        &book_scope(principal, book),
        AclRight::Write,
    )?;
    let prefix = format!("{}/", book_dir(principal, book));
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

fn require_book<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
) -> Result<()> {
    if get_book_unchecked(loom, ns, principal, book)?.is_none() {
        return Err(LoomError::not_found(format!(
            "contacts: address book {principal}/{book} does not exist"
        )));
    }
    Ok(())
}

/// The ETag of a record: the content address of its canonical bytes under the store's digest profile.
pub fn entry_etag<S: ObjectStore>(loom: &Loom<S>, entry: &ContactEntry) -> Digest {
    content_address_with(loom.store().digest_algo(), &entry.encode())
}

/// Put `entry` into an existing address book, keyed by its `UID`; returns the new ETag.
pub fn put_entry<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
    entry: &ContactEntry,
) -> Result<Digest> {
    validate_segment(principal, "principal")?;
    validate_segment(book, "book")?;
    loom.authorize_collection(
        ns,
        FacetKind::Contacts,
        &book_scope(principal, book),
        AclRight::Write,
    )?;
    if entry.uid.is_empty() {
        return Err(LoomError::invalid("contacts: entry UID must not be empty"));
    }
    if entry.full_name.is_empty() {
        return Err(LoomError::invalid("contacts: entry FN must not be empty"));
    }
    require_book(loom, ns, principal, book)?;
    let path = entry_path(principal, book, &entry.uid);
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
    emit_contact_event(
        loom,
        ns,
        lifecycle_event,
        principal,
        book,
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
    emit_contact_event(
        loom,
        ns,
        lifecycle_event,
        principal,
        book,
        &entry.uid,
        (before.clone(), Some(bytes.clone())),
    )?;
    let domain_event = if before.is_some() {
        "on_contact_updated"
    } else {
        "on_contact_added"
    };
    emit_contact_event(
        loom,
        ns,
        domain_event,
        principal,
        book,
        &entry.uid,
        (before, Some(bytes)),
    )?;
    Ok(etag)
}

/// The contact at `uid`, or `None` if absent.
pub fn get_entry<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
    uid: &str,
) -> Result<Option<ContactEntry>> {
    loom.authorize_collection(
        ns,
        FacetKind::Contacts,
        &book_scope(principal, book),
        AclRight::Read,
    )?;
    get_entry_unchecked(loom, ns, principal, book, uid)
}

fn get_entry_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
    uid: &str,
) -> Result<Option<ContactEntry>> {
    match loom.read_file_reserved(ns, &entry_path(principal, book, uid)) {
        Ok(bytes) => Ok(Some(ContactEntry::decode(&bytes)?)),
        Err(e) if e.code == Code::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Remove the contact at `uid`; returns whether it was present.
pub fn delete_entry<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
    uid: &str,
) -> Result<bool> {
    loom.authorize_collection(
        ns,
        FacetKind::Contacts,
        &book_scope(principal, book),
        AclRight::Write,
    )?;
    let path = entry_path(principal, book, uid);
    let before = match loom.read_file_reserved(ns, &path) {
        Ok(bytes) => Some(bytes),
        Err(err) if err.code == Code::NotFound => None,
        Err(err) => return Err(err),
    };
    if let Some(bytes) = before {
        emit_contact_event(
            loom,
            ns,
            "before_delete",
            principal,
            book,
            uid,
            (Some(bytes.clone()), None),
        )?;
        loom.remove_file_reserved(ns, &path)?;
        emit_contact_event(
            loom,
            ns,
            "after_delete",
            principal,
            book,
            uid,
            (Some(bytes), None),
        )?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn emit_contact_event<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    event: &str,
    principal: &str,
    book: &str,
    uid: &str,
    bodies: (Option<Vec<u8>>, Option<Vec<u8>>),
) -> Result<()> {
    let (before, after) = bodies;
    hook_emit_event_unchecked(
        loom,
        ns,
        &PimEventEnvelope {
            workspace: ns,
            facet: FacetKind::Contacts,
            event: event.to_string(),
            principal: principal.to_string(),
            collection: Some(book.to_string()),
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

/// All contacts in an address book, sorted by `UID`.
pub fn list_entries<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
) -> Result<Vec<ContactEntry>> {
    loom.authorize_collection(
        ns,
        FacetKind::Contacts,
        &book_scope(principal, book),
        AclRight::Read,
    )?;
    list_entries_unchecked(loom, ns, principal, book)
}

fn list_entries_unchecked<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
) -> Result<Vec<ContactEntry>> {
    let prefix = format!("{}/", book_dir(principal, book));
    let mut entries: Vec<ContactEntry> = Vec::new();
    for p in loom.staged_paths(ns) {
        let Some(seg) = p.strip_prefix(&prefix) else {
            continue;
        };
        if seg.contains('/') || seg == META_FILE {
            continue;
        }
        entries.push(ContactEntry::decode(&loom.read_file_reserved(ns, &p)?)?);
    }
    entries.sort_by(|a, b| a.uid.cmp(&b.uid));
    Ok(entries)
}

/// All contacts in an address book at `commit`, sorted by `UID`.
pub fn list_entries_at_commit<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    commit: Digest,
    principal: &str,
    book: &str,
) -> Result<Vec<ContactEntry>> {
    loom.authorize_collection(
        ns,
        FacetKind::Contacts,
        &book_scope(principal, book),
        AclRight::Read,
    )?;
    let prefix = format!("{}/", book_dir(principal, book));
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
        entries.push(ContactEntry::decode(
            &loom.load_content(file.content_addr)?,
        )?);
    }
    entries.sort_by(|a, b| a.uid.cmp(&b.uid));
    Ok(entries)
}

/// Search contacts by a case-insensitive substring over the formatted name, organization, and email
/// values (the CardDAV property-filtered model); UID-ordered.
pub fn search<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
    text: &str,
) -> Result<Vec<ContactEntry>> {
    let needle = text.to_lowercase();
    Ok(list_entries(loom, ns, principal, book)?
        .into_iter()
        .filter(|c| {
            c.full_name.to_lowercase().contains(&needle)
                || c.org
                    .as_deref()
                    .is_some_and(|o| o.to_lowercase().contains(&needle))
                || c.emails
                    .iter()
                    .any(|e| e.value.to_lowercase().contains(&needle))
        })
        .collect())
}

/// The on-demand vCard (`.vcf`) projection of the contact at `uid`, or `None` if absent.
pub fn entry_vcard<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
    uid: &str,
) -> Result<Option<String>> {
    Ok(get_entry(loom, ns, principal, book, uid)?.map(|e| e.to_vcard()))
}

/// Parse a vCard and store it as a record (the validated write-in path); returns the new ETag.
pub fn put_vcard<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    principal: &str,
    book: &str,
    vcf: &str,
) -> Result<Digest> {
    let entry = ContactEntry::from_vcard(vcf)?;
    put_entry(loom, ns, principal, book, &entry)
}

/// A single contact's change between two address-book states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryChange {
    pub uid: String,
    pub kind: ChangeKind,
    pub etag: Option<Digest>,
}

/// The nature of a contact change in a book diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Updated,
    Removed,
}

/// Per-UID changes from `old` to `new` book states (the CardDAV `sync-collection` diff). UID is the
/// unit; the ETag decides whether a present contact changed.
pub fn diff_entries<S: ObjectStore>(
    loom: &Loom<S>,
    old: &[ContactEntry],
    new: &[ContactEntry],
) -> Vec<EntryChange> {
    use std::collections::BTreeMap;
    let algo = loom.store().digest_algo();
    let etag = |e: &ContactEntry| content_address_with(algo, &e.encode());
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
    book: &str,
    next_version: u64,
    retained_since_version: Option<u64>,
    changes: Vec<EntryChange>,
) -> Result<ChangeSet> {
    let scope = contacts_change_scope(ns, principal, book);
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

pub fn contacts_change_scope(ns: WorkspaceId, principal: &str, book: &str) -> String {
    format!("contacts:{}:{principal}/{book}", hex::encode(ns.as_bytes()))
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

    fn contacts_ns() -> (Loom<MemoryStore>, WorkspaceId) {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Contacts, None, WorkspaceId::from_bytes([24; 16]))
            .unwrap();
        (loom, ns)
    }

    fn personal(loom: &mut Loom<MemoryStore>, ns: WorkspaceId) {
        create_book(
            loom,
            ns,
            "alice",
            "personal",
            &BookMeta {
                display_name: "Personal".into(),
            },
        )
        .unwrap();
    }

    #[test]
    fn authenticated_contacts_operations_are_acl_checked() {
        let (mut loom, ns) = contacts_ns();
        let root = WorkspaceId::from_bytes([1; 16]);
        let mut identity = IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);

        assert_eq!(
            create_book(
                &mut loom,
                ns,
                "alice",
                "personal",
                &BookMeta {
                    display_name: "Personal".into(),
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
                Some(FacetKind::Contacts),
                [AclRight::Write, AclRight::Read],
            )
            .unwrap();

        personal(&mut loom, ns);
        assert_eq!(
            list_books(&loom, ns, "alice").unwrap(),
            vec!["personal".to_string()]
        );
    }

    #[test]
    fn record_round_trips() {
        let mut c = ContactEntry::new("u1", "Ada Lovelace");
        c.emails = vec![
            TypedValue::typed("ada@x.io", "work"),
            TypedValue::new("ada@home.io"),
        ];
        c.tels = vec![TypedValue::typed("+1-555", "cell")];
        c.org = Some("Analytical Engines".into());
        c.extra = vec![("X-NOTE".into(), "first programmer".into())];
        assert_eq!(ContactEntry::decode(&c.encode()).unwrap(), c);
    }

    #[test]
    fn vcard_round_trips_semantically() {
        let mut c = ContactEntry::new("u1", "Grace Hopper");
        c.n = Some("Hopper;Grace;;;".into());
        c.emails = vec![TypedValue::typed("grace@navy.mil", "work")];
        c.tels = vec![TypedValue::typed("+1-555-0101", "cell")];
        c.org = Some("US Navy".into());
        c.title = Some("Rear Admiral".into());
        c.extra = vec![("X-LOOM-TAG".into(), "pioneer".into())];
        let vcf = c.to_vcard();
        // vcard4 owns the exact wire format now; anchor on structure plus the semantic round-trip.
        assert!(vcf.contains("BEGIN:VCARD") && vcf.contains("END:VCARD"));
        assert!(vcf.contains("VERSION:4.0"));
        assert!(vcf.contains("TYPE=work"));
        assert!(vcf.contains("TYPE=cell"));
        assert_eq!(ContactEntry::from_vcard(&vcf).unwrap(), c);
    }

    #[test]
    fn book_and_entry_lifecycle() {
        let (mut loom, ns) = contacts_ns();
        // put before MKCOL is NOT_FOUND.
        let err = put_entry(
            &mut loom,
            ns,
            "alice",
            "personal",
            &ContactEntry::new("u1", "X"),
        )
        .unwrap_err();
        assert_eq!(err.code, Code::NotFound);
        personal(&mut loom, ns);
        assert_eq!(
            list_books(&loom, ns, "alice").unwrap(),
            vec!["personal".to_string()]
        );

        let c = ContactEntry::new("u1", "Ada");
        let t1 = put_entry(&mut loom, ns, "alice", "personal", &c).unwrap();
        let mut c2 = c.clone();
        c2.org = Some("AE".into());
        let t2 = put_entry(&mut loom, ns, "alice", "personal", &c2).unwrap();
        assert_ne!(t1, t2, "edit changes the ETag");
        assert_eq!(
            list_entries(&loom, ns, "alice", "personal").unwrap().len(),
            1
        );
        assert!(delete_entry(&mut loom, ns, "alice", "personal", "u1").unwrap());
        assert!(delete_book(&mut loom, ns, "alice", "personal").unwrap());
    }

    #[test]
    fn search_and_diff_and_versioning() {
        let (mut loom, ns) = contacts_ns();
        personal(&mut loom, ns);
        let mut a = ContactEntry::new("u1", "Ada Lovelace");
        a.emails = vec![TypedValue::new("ada@x.io")];
        put_entry(&mut loom, ns, "alice", "personal", &a).unwrap();
        put_entry(
            &mut loom,
            ns,
            "alice",
            "personal",
            &ContactEntry::new("u2", "Bob"),
        )
        .unwrap();

        assert_eq!(
            search(&loom, ns, "alice", "personal", "lovelace")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            search(&loom, ns, "alice", "personal", "ada@x")
                .unwrap()
                .len(),
            1
        );

        let old = list_entries(&loom, ns, "alice", "personal").unwrap();
        let c1 = loom.commit(ns, "alice", "two contacts", 1).unwrap();
        delete_entry(&mut loom, ns, "alice", "personal", "u2").unwrap();
        put_entry(
            &mut loom,
            ns,
            "alice",
            "personal",
            &ContactEntry::new("u3", "Cara"),
        )
        .unwrap();
        let new = list_entries(&loom, ns, "alice", "personal").unwrap();
        let changes = diff_entries(&loom, &old, &new);
        let summary: Vec<(&str, ChangeKind)> =
            changes.iter().map(|c| (c.uid.as_str(), c.kind)).collect();
        assert_eq!(
            summary,
            [("u2", ChangeKind::Removed), ("u3", ChangeKind::Added)]
        );
        let change_set = entry_changeset(ns, "alice", "personal", 4, Some(1), changes).unwrap();
        assert_eq!(change_set.gap_state, ChangeGapState::Retained);
        assert_eq!(change_set.retained_low_water_mark, Some(1));
        assert_eq!(change_set.items.len(), 2);
        assert_eq!(change_set.items[0].kind, ChangeItemKind::Removed);
        assert_eq!(change_set.items[1].id, "u3");

        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(
            list_entries(&loom, ns, "alice", "personal").unwrap().len(),
            2
        );
    }

    #[test]
    fn vcard_put_and_get_through_facet() {
        let (mut loom, ns) = contacts_ns();
        personal(&mut loom, ns);
        let vcf = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:imported\r\nFN:Imported Person\r\nEMAIL:i@x.io\r\nEND:VCARD\r\n";
        put_vcard(&mut loom, ns, "alice", "personal", vcf).unwrap();
        let out = entry_vcard(&loom, ns, "alice", "personal", "imported")
            .unwrap()
            .unwrap();
        assert!(out.contains("FN:Imported Person") && out.contains("EMAIL:i@x.io"));
    }
}
