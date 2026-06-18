//! Facet-aware mount overlay for the `calendar/`, `contacts/`, and `mail/` roots of a loom-vfs mount.
//!
//! A mount exposes the workspace working tree as an ordinary filesystem. This overlay layers facet
//! behaviour on three top-level roots without changing how any other path behaves:
//!
//! - A file whose name ends in the facet extension (`.ics`/`.vcf`/`.eml`) under
//!   `<root>/<principal>/<collection>/` is **ingested**: its bytes are parsed into the structured facet
//!   record on write-in, and the visible file is a **projection** of that record on read (the record is
//!   the source of truth; the raw wire bytes are not stored).
//! - A parse failure **quarantines** the file: the raw bytes stay as an ordinary working-tree file at
//!   that path (so it is still "there") and processing metadata records the error; no record is created.
//! - Any other file (`cat.jpg`, `notes.txt`, an editor sidecar) is a normal working-tree file, stored
//!   and served verbatim - there is no denylist.
//!
//! Writes land in the working tree **unstaged**; an explicit `vcs` commit persists them. Per-file
//! processing metadata is exposed as extended attributes (`user.loom.*`) by the backend.
//!
//! This module is the portable mechanism (classify, ingest, project, processing, list). A FUSE/NFS
//! backend wires it in by: on flush calling [`ingest`]; on read trying [`project`] before a normal read;
//! on readdir merging [`list_projected`] with the ordinary entries; and on getxattr returning
//! [`processing`]. Those backend hooks are thin and platform-specific; the semantics live and are tested
//! here.

use loom_core::error::{Code, Result};
use loom_core::object::content_address_with;
use loom_core::provider::ObjectStore;
use loom_core::workspace::{FacetKind, WorkspaceId, facet_path};
use loom_core::{Digest, Loom, calendar, contacts, mail};
use std::collections::BTreeMap;

/// The three communication facets exposed as mount roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facet {
    Calendar,
    Contacts,
    Mail,
}

impl Facet {
    /// The top-level mount-root name (`calendar`/`contacts`/`mail`).
    pub fn root(self) -> &'static str {
        match self {
            Facet::Calendar => "calendar",
            Facet::Contacts => "contacts",
            Facet::Mail => "mail",
        }
    }
    /// The file extension (without the dot) that triggers ingestion.
    pub fn ext(self) -> &'static str {
        match self {
            Facet::Calendar => "ics",
            Facet::Contacts => "vcf",
            Facet::Mail => "eml",
        }
    }
    fn kind(self) -> FacetKind {
        match self {
            Facet::Calendar => FacetKind::Calendar,
            Facet::Contacts => FacetKind::Contacts,
            Facet::Mail => FacetKind::Mail,
        }
    }
    fn from_root(s: &str) -> Option<Facet> {
        match s {
            "calendar" => Some(Facet::Calendar),
            "contacts" => Some(Facet::Contacts),
            "mail" => Some(Facet::Mail),
            _ => None,
        }
    }
}

/// A classified facet file: `<root>/<principal>/<collection>/<name>.<ext>`. `stem` is `<name>` (the
/// filename without its extension), which is the resource id for mail and the CalDAV-style resource name
/// for calendar/contacts (the authoritative UID for those comes from the parsed content).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetFile {
    pub facet: Facet,
    pub principal: String,
    pub collection: String,
    pub name: String,
    pub stem: String,
}

/// Classify a mount-relative path. Returns `Some` only for a file directly under
/// `<root>/<principal>/<collection>/` whose name ends in the facet extension; everything else (other
/// paths, wrong depth, non-facet extensions, the collection dir itself) is `None` and is handled as an
/// ordinary working-tree path by the caller.
pub fn classify(path: &str) -> Option<FacetFile> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() != 4 {
        return None;
    }
    let facet = Facet::from_root(parts[0])?;
    let (principal, collection, name) = (parts[1], parts[2], parts[3]);
    if principal.is_empty() || collection.is_empty() || name.is_empty() {
        return None;
    }
    let suffix = format!(".{}", facet.ext());
    let stem = name.strip_suffix(&suffix)?;
    if stem.is_empty() {
        return None;
    }
    Some(FacetFile {
        facet,
        principal: principal.to_string(),
        collection: collection.to_string(),
        name: name.to_string(),
        stem: stem.to_string(),
    })
}

/// Classify a directory path as a facet collection directory `<root>/<principal>/<collection>`.
pub fn classify_collection(path: &str) -> Option<(Facet, String, String)> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() != 3 {
        return None;
    }
    let facet = Facet::from_root(parts[0])?;
    if parts[1].is_empty() || parts[2].is_empty() {
        return None;
    }
    Some((facet, parts[1].to_string(), parts[2].to_string()))
}

/// Ensure the facet collection backing a `<root>/<principal>/<collection>` directory exists (idempotent;
/// called when that directory is created on the mount). The display name defaults to the collection
/// segment; a calendar collection accepts both events and todos.
pub fn ensure_collection<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    facet: Facet,
    principal: &str,
    collection: &str,
) -> Result<()> {
    match facet {
        Facet::Calendar => calendar::create_collection(
            loom,
            ns,
            principal,
            collection,
            &calendar::CollectionMeta {
                display_name: collection.to_string(),
                component_set: vec![calendar::Component::Event, calendar::Component::Todo],
            },
        ),
        Facet::Contacts => contacts::create_book(
            loom,
            ns,
            principal,
            collection,
            &contacts::BookMeta {
                display_name: collection.to_string(),
            },
        ),
        Facet::Mail => mail::create_mailbox(
            loom,
            ns,
            principal,
            collection,
            &mail::MailboxMeta {
                display_name: collection.to_string(),
            },
        ),
    }
}

/// The outcome of ingesting a facet file's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    /// Parsed and stored as a structured record; carries the record ETag (hex content address).
    Stored { etag: String },
    /// Could not be parsed; the raw bytes were kept as an ordinary file and the error recorded.
    Quarantined { error: String },
}

/// Per-file processing metadata, surfaced as extended attributes by the backend.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Processing {
    /// `"ok"` for a stored record, `"quarantined"` for a kept-but-unparsed file, `""` if unknown.
    pub status: String,
    /// The parse error for a quarantined file.
    pub error: Option<String>,
    /// The record ETag (hex content address) for a stored record.
    pub etag: Option<String>,
}

impl Processing {
    /// Render as `user.loom.*` extended-attribute pairs (only the set fields).
    pub fn xattrs(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if !self.status.is_empty() {
            out.push(("user.loom.status".to_string(), self.status.clone()));
        }
        if let Some(e) = &self.error {
            out.push(("user.loom.error".to_string(), e.clone()));
        }
        if let Some(t) = &self.etag {
            out.push(("user.loom.etag".to_string(), t.clone()));
        }
        out
    }
}

// ---- processing-metadata store (reserved sidecar, plain UTF-8 lines, no extra deps) -----------------

fn hex_name(name: &str) -> String {
    let mut s = String::with_capacity(name.len() * 2);
    for b in name.bytes() {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('0'));
        s.push(char::from_digit((b & 0xf) as u32, 16).unwrap_or('0'));
    }
    s
}

fn processing_dir(f: &FacetFile) -> String {
    facet_path(
        f.facet.kind(),
        &format!("{}/{}/.processing", f.principal, f.collection),
    )
}

fn processing_path(f: &FacetFile) -> String {
    format!("{}/{}", processing_dir(f), hex_name(&f.name))
}

fn encode_processing(p: &Processing) -> Vec<u8> {
    // One `key=value` per line; values are single-line (newlines in an error are flattened to spaces).
    let mut s = format!("status={}\n", p.status);
    if let Some(e) = &p.error {
        s.push_str(&format!("error={}\n", e.replace('\n', " ")));
    }
    if let Some(t) = &p.etag {
        s.push_str(&format!("etag={t}\n"));
    }
    s.into_bytes()
}

fn decode_processing(bytes: &[u8]) -> Processing {
    let text = String::from_utf8_lossy(bytes);
    let mut p = Processing::default();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            match k {
                "status" => p.status = v.to_string(),
                "error" => p.error = Some(v.to_string()),
                "etag" => p.etag = Some(v.to_string()),
                _ => {}
            }
        }
    }
    p
}

fn write_processing<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    f: &FacetFile,
    p: &Processing,
) -> Result<()> {
    loom.create_directory_reserved(ns, &processing_dir(f), true)?;
    loom.write_file_reserved(ns, &processing_path(f), &encode_processing(p), 0o100644)
}

fn clear_processing<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    f: &FacetFile,
) -> Result<()> {
    let path = processing_path(f);
    if loom.staged_paths(ns).iter().any(|p| p == &path) {
        loom.remove_file_reserved(ns, &path)?;
    }
    Ok(())
}

/// Read the processing metadata for a facet file. A stored record reports `status=ok` with its current
/// ETag; a quarantined file reports `status=quarantined` with the error; an unknown file reports an empty
/// status.
pub fn processing<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    f: &FacetFile,
) -> Result<Processing> {
    // A live record takes precedence and reports its current ETag (recomputed, always fresh).
    if let Some(etag) = record_etag(loom, ns, f)? {
        return Ok(Processing {
            status: "ok".to_string(),
            error: None,
            etag: Some(etag),
        });
    }
    match loom.read_file(ns, &processing_path(f)) {
        Ok(bytes) => Ok(decode_processing(&bytes)),
        Err(e) if e.code == Code::NotFound => Ok(Processing::default()),
        Err(e) => Err(e),
    }
}

fn record_etag<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    f: &FacetFile,
) -> Result<Option<String>> {
    let etag = match f.facet {
        Facet::Calendar => calendar::get_entry(loom, ns, &f.principal, &f.collection, &f.stem)?
            .map(|e| calendar::entry_etag(loom, &e).to_hex()),
        Facet::Contacts => contacts::get_entry(loom, ns, &f.principal, &f.collection, &f.stem)?
            .map(|e| contacts::entry_etag(loom, &e).to_hex()),
        Facet::Mail => {
            mail::get_message(loom, ns, &f.principal, &f.collection, &f.stem)?.map(|m| m.body)
        }
    };
    Ok(etag)
}

// ---- ingest / project / list -----------------------------------------------------------------------

/// Ingest the full bytes written to a facet file (called by the backend on flush/close). On a parse
/// success the structured record is created/updated and any prior quarantine of this name is cleared; on
/// a parse failure the raw bytes are kept as an ordinary working-tree file and the error is recorded.
/// A missing collection (the parent was never created) is propagated as `NOT_FOUND`, not quarantined.
pub fn ingest<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    f: &FacetFile,
    bytes: &[u8],
) -> Result<WriteOutcome> {
    let parsed: std::result::Result<String, String> = match f.facet {
        Facet::Mail => {
            // The body is opaque bytes; ingestion never "fails to parse" for content reasons.
            match mail::ingest_message(loom, ns, &f.principal, &f.collection, &f.stem, bytes) {
                Ok(d) => Ok(d.to_hex()),
                Err(e) if e.code == Code::NotFound => return Err(e),
                Err(e) => Err(e.message),
            }
        }
        Facet::Calendar => match std::str::from_utf8(bytes) {
            Ok(s) => match calendar::put_ics(loom, ns, &f.principal, &f.collection, s) {
                Ok(d) => Ok(d.to_hex()),
                Err(e) if e.code == Code::NotFound => return Err(e),
                Err(e) => Err(e.message),
            },
            Err(_) => Err("not valid UTF-8".to_string()),
        },
        Facet::Contacts => match std::str::from_utf8(bytes) {
            Ok(s) => match contacts::put_vcard(loom, ns, &f.principal, &f.collection, s) {
                Ok(d) => Ok(d.to_hex()),
                Err(e) if e.code == Code::NotFound => return Err(e),
                Err(e) => Err(e.message),
            },
            Err(_) => Err("not valid UTF-8".to_string()),
        },
    };

    match parsed {
        Ok(etag) => {
            // A fixed re-upload supersedes any earlier quarantine of the same name.
            remove_quarantine_file(loom, ns, f)?;
            clear_processing(loom, ns, f)?;
            Ok(WriteOutcome::Stored { etag })
        }
        Err(error) => {
            quarantine(loom, ns, f, bytes, &error)?;
            Ok(WriteOutcome::Quarantined { error })
        }
    }
}

/// Keep the raw bytes as an ordinary working-tree file at the visible path and record the error.
fn quarantine<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    f: &FacetFile,
    bytes: &[u8],
    error: &str,
) -> Result<()> {
    let dir = format!("{}/{}/{}", f.facet.root(), f.principal, f.collection);
    loom.create_directory(ns, &dir, true)?;
    let path = format!("{dir}/{}", f.name);
    loom.write_file(ns, &path, bytes, 0o100644)?;
    write_processing(
        loom,
        ns,
        f,
        &Processing {
            status: "quarantined".to_string(),
            error: Some(error.to_string()),
            etag: None,
        },
    )
}

fn remove_quarantine_file<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    f: &FacetFile,
) -> Result<()> {
    let path = format!(
        "{}/{}/{}/{}",
        f.facet.root(),
        f.principal,
        f.collection,
        f.name
    );
    if loom.exists(ns, &path)? {
        loom.remove_file(ns, &path)?;
    }
    Ok(())
}

/// Project a facet file on read: returns the serialized record bytes if a record exists, or `None` if
/// no record exists (the caller then falls back to an ordinary working-tree read, which serves a
/// quarantined or arbitrary file verbatim).
pub fn project<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    f: &FacetFile,
) -> Result<Option<Vec<u8>>> {
    let bytes = match f.facet {
        Facet::Calendar => calendar::entry_ics(loom, ns, &f.principal, &f.collection, &f.stem)?
            .map(String::into_bytes),
        Facet::Contacts => contacts::entry_vcard(loom, ns, &f.principal, &f.collection, &f.stem)?
            .map(String::into_bytes),
        Facet::Mail => mail::to_eml(loom, ns, &f.principal, &f.collection, &f.stem)?,
    };
    Ok(bytes)
}

/// Delete a structured record behind a projected facet file. Returns `false` when no record exists.
pub fn delete<S: ObjectStore>(loom: &mut Loom<S>, ns: WorkspaceId, f: &FacetFile) -> Result<bool> {
    let deleted = match f.facet {
        Facet::Calendar => calendar::delete_entry(loom, ns, &f.principal, &f.collection, &f.stem)?,
        Facet::Contacts => contacts::delete_entry(loom, ns, &f.principal, &f.collection, &f.stem)?,
        Facet::Mail => mail::delete_message(loom, ns, &f.principal, &f.collection, &f.stem)?,
    };
    if deleted {
        clear_processing(loom, ns, f)?;
    }
    Ok(deleted)
}

/// The projected record filenames (`<uid>.<ext>`) in a collection, sorted. The backend merges these with
/// the ordinary directory listing (records are not working-tree files, so they do not otherwise appear);
/// quarantined and arbitrary files already appear as ordinary entries.
pub fn list_projected<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    facet: Facet,
    principal: &str,
    collection: &str,
) -> Result<Vec<String>> {
    let ext = facet.ext();
    let mut names: Vec<String> = match facet {
        Facet::Calendar => calendar::list_entries(loom, ns, principal, collection)?
            .into_iter()
            .map(|e| format!("{}.{ext}", e.uid))
            .collect(),
        Facet::Contacts => contacts::list_entries(loom, ns, principal, collection)?
            .into_iter()
            .map(|e| format!("{}.{ext}", e.uid))
            .collect(),
        Facet::Mail => mail::list_messages(loom, ns, principal, collection)?
            .into_iter()
            .map(|m| format!("{}.{ext}", m.uid))
            .collect(),
    };
    names.sort();
    Ok(names)
}

// ---- reconcile (generic; the common code behind on-demand pass B and debounced pass C) ------------
//
// A backend with a close signal (FUSE) ingests on flush. A backend without one (NFSv3 via nfsserve has
// no commit/fsync trait hook and advertises FILE_SYNC, so clients never send COMMIT) leaves a dropped
// facet file as a raw working-tree file. `reconcile` turns those pending raw files into records. It is
// facet-generic: it sweeps the `calendar/`, `contacts/`, and `mail/` roots alike.

/// Every raw working-tree file that classifies as a facet file but has no backing record yet (an
/// un-ingested drop, or a previously quarantined file), across all facet roots in `ns`.
pub fn pending_facet_files<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
) -> Result<Vec<FacetFile>> {
    let mut out = Vec::new();
    for path in loom.staged_paths(ns) {
        let Some(f) = classify(&path) else { continue };
        // A record-backed file is already ingested (it has no raw working-tree file); skip it.
        if project(loom, ns, &f)?.is_some() {
            continue;
        }
        // Only real working-tree files are pending (the record projections are not working-tree paths).
        if loom.exists(ns, &path)? {
            out.push(f);
        }
    }
    Ok(out)
}

/// Ingest every pending facet file immediately (parse into a record, or re-quarantine if still
/// unparseable). Returns the per-file outcomes.
pub fn reconcile<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
) -> Result<Vec<(FacetFile, WriteOutcome)>> {
    let mut out = Vec::new();
    for f in pending_facet_files(loom, ns)? {
        let path = format!(
            "{}/{}/{}/{}",
            f.facet.root(),
            f.principal,
            f.collection,
            f.name
        );
        let bytes = match loom.read_file(ns, &path) {
            Ok(b) => b,
            Err(e) if e.code == Code::NotFound => continue,
            Err(e) => return Err(e),
        };
        let outcome = ingest(loom, ns, &f, &bytes)?;
        out.push((f, outcome));
    }
    Ok(out)
}

/// Pass C - debounced reconcile: ingest only pending files whose content has been **stable since the
/// previous tick** (the same file content was seen last call), so a file still being written is never
/// ingested mid-write. `seen` carries the content address of each pending file across ticks; the caller
/// (a backend timer) holds it and calls this on each tick. Returns the files ingested on this tick.
pub fn reconcile_quiescent<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    seen: &mut BTreeMap<String, Digest>,
) -> Result<Vec<(FacetFile, WriteOutcome)>> {
    let algo = loom.store().digest_algo();
    let pending = pending_facet_files(loom, ns)?;
    let mut current: BTreeMap<String, (FacetFile, Digest, Vec<u8>)> = BTreeMap::new();
    for f in pending {
        let path = format!(
            "{}/{}/{}/{}",
            f.facet.root(),
            f.principal,
            f.collection,
            f.name
        );
        match loom.read_file(ns, &path) {
            Ok(bytes) => {
                let digest = content_address_with(algo, &bytes);
                current.insert(path, (f, digest, bytes));
            }
            Err(e) if e.code == Code::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    let mut out = Vec::new();
    let mut next: BTreeMap<String, Digest> = BTreeMap::new();
    for (path, (f, digest, bytes)) in current {
        if seen.get(&path) == Some(&digest) {
            // Unchanged since the last tick: safe to finalize.
            let outcome = ingest(loom, ns, &f, &bytes)?;
            out.push((f, outcome));
        } else {
            // Changed (or new) this tick: remember it and wait one more tick for stability.
            next.insert(path, digest);
        }
    }
    *seen = next;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::MemoryStore;

    fn loom_ns() -> (Loom<MemoryStore>, WorkspaceId) {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Calendar, None, WorkspaceId::from_bytes([3; 16]))
            .unwrap();
        (loom, ns)
    }

    const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:ev1\r\nSUMMARY:Standup\r\nDTSTART:20240101T090000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    const VCF: &str = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c1\r\nFN:Ada Lovelace\r\nEND:VCARD\r\n";

    #[test]
    fn classify_only_facet_files() {
        assert_eq!(classify("calendar/alice/work/ev1.ics").unwrap().stem, "ev1");
        assert_eq!(
            classify("contacts/alice/personal/c1.vcf").unwrap().facet,
            Facet::Contacts
        );
        assert_eq!(
            classify("mail/alice/inbox/m1.eml").unwrap().facet,
            Facet::Mail
        );
        // wrong depth, wrong root, wrong extension, the collection dir, a non-facet file: all None.
        assert!(classify("calendar/alice/work").is_none());
        assert!(classify("calendar/alice/work/sub/ev.ics").is_none());
        assert!(classify("notes/alice/work/ev.ics").is_none());
        assert!(classify("calendar/alice/work/cat.jpg").is_none());
        assert!(classify("calendar/alice/work/.ics").is_none());
    }

    #[test]
    fn valid_file_becomes_a_projected_record() {
        let (mut loom, ns) = loom_ns();
        calendar::create_collection(
            &mut loom,
            ns,
            "alice",
            "work",
            &calendar::CollectionMeta {
                display_name: "Work".into(),
                component_set: vec![calendar::Component::Event],
            },
        )
        .unwrap();
        // The dropped filename ("dropme") differs from the content UID ("ev1").
        let f = classify("calendar/alice/work/dropme.ics").unwrap();
        let out = ingest(&mut loom, ns, &f, ICS.as_bytes()).unwrap();
        assert!(matches!(out, WriteOutcome::Stored { .. }));
        // The record is keyed by the content UID; it projects as <uid>.ics, not the dropped name.
        let listed = list_projected(&loom, ns, Facet::Calendar, "alice", "work").unwrap();
        assert_eq!(listed, vec!["ev1.ics".to_string()]);
        // Reading the canonical projected name serializes the record (no raw bytes were stored).
        let proj = classify("calendar/alice/work/ev1.ics").unwrap();
        let bytes = project(&loom, ns, &proj).unwrap().unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("UID:ev1"));
        // No quarantine file at the dropped path.
        assert!(!loom.exists(ns, "calendar/alice/work/dropme.ics").unwrap());
        // Processing reports ok + an etag for the record.
        assert_eq!(processing(&loom, ns, &proj).unwrap().status, "ok");
        assert!(processing(&loom, ns, &proj).unwrap().etag.is_some());
    }

    #[test]
    fn invalid_facet_file_is_quarantined_and_stays_there() {
        let (mut loom, ns) = loom_ns();
        calendar::create_collection(
            &mut loom,
            ns,
            "alice",
            "work",
            &calendar::CollectionMeta::default(),
        )
        .unwrap();
        let f = classify("calendar/alice/work/broken.ics").unwrap();
        let out = ingest(&mut loom, ns, &f, b"this is not iCalendar").unwrap();
        let WriteOutcome::Quarantined { error } = out else {
            panic!("expected quarantine");
        };
        assert!(!error.is_empty());
        // The raw file is kept verbatim as an ordinary working-tree file (it is "there").
        assert!(loom.exists(ns, "calendar/alice/work/broken.ics").unwrap());
        assert_eq!(
            loom.read_file(ns, "calendar/alice/work/broken.ics")
                .unwrap(),
            b"this is not iCalendar"
        );
        // No record was created; processing reports the quarantine error (for xattr).
        assert!(project(&loom, ns, &f).unwrap().is_none());
        let p = processing(&loom, ns, &f).unwrap();
        assert_eq!(p.status, "quarantined");
        assert!(p.error.is_some());
        assert_eq!(
            p.xattrs()
                .iter()
                .find(|(k, _)| k == "user.loom.status")
                .map(|(_, v)| v.as_str()),
            Some("quarantined")
        );
    }

    #[test]
    fn fixing_a_quarantined_file_supersedes_it() {
        let (mut loom, ns) = loom_ns();
        calendar::create_collection(
            &mut loom,
            ns,
            "alice",
            "work",
            &calendar::CollectionMeta::default(),
        )
        .unwrap();
        let f = classify("calendar/alice/work/ev1.ics").unwrap();
        ingest(&mut loom, ns, &f, b"garbage").unwrap();
        assert!(loom.exists(ns, "calendar/alice/work/ev1.ics").unwrap());
        // Re-upload valid content under the same name: the quarantine file is removed and a record made.
        let out = ingest(&mut loom, ns, &f, ICS.as_bytes()).unwrap();
        assert!(matches!(out, WriteOutcome::Stored { .. }));
        assert!(!loom.exists(ns, "calendar/alice/work/ev1.ics").unwrap());
        assert_eq!(processing(&loom, ns, &f).unwrap().status, "ok");
    }

    #[test]
    fn arbitrary_file_is_not_a_facet_file() {
        // A .jpg under the calendar root is not classified, so the backend stores it as a normal file.
        assert!(classify("calendar/alice/work/cat.jpg").is_none());
    }

    #[test]
    fn missing_collection_is_not_quarantined() {
        let (mut loom, ns) = loom_ns();
        for (path, bytes) in [
            ("calendar/alice/work/ev1.ics", ICS.as_bytes()),
            ("contacts/alice/people/c1.vcf", VCF.as_bytes()),
            (
                "mail/alice/inbox/m1.eml",
                b"From: bob@x.io\r\nSubject: Hi\r\n\r\nbody".as_slice(),
            ),
        ] {
            let f = classify(path).unwrap();
            let err = ingest(&mut loom, ns, &f, bytes).unwrap_err();
            assert_eq!(err.code, Code::NotFound);
            assert!(!loom.exists(ns, path).unwrap());
        }
    }

    #[test]
    fn contacts_projection_updates_and_deletes_by_record_uid() {
        let (mut loom, ns) = loom_ns();
        contacts::create_book(
            &mut loom,
            ns,
            "alice",
            "people",
            &contacts::BookMeta::default(),
        )
        .unwrap();
        let dropped = classify("contacts/alice/people/dropme.vcf").unwrap();
        ingest(&mut loom, ns, &dropped, VCF.as_bytes()).unwrap();
        assert_eq!(
            list_projected(&loom, ns, Facet::Contacts, "alice", "people").unwrap(),
            vec!["c1.vcf".to_string()]
        );
        let projected = classify("contacts/alice/people/c1.vcf").unwrap();
        let body = project(&loom, ns, &projected).unwrap().unwrap();
        assert!(String::from_utf8_lossy(&body).contains("FN:Ada Lovelace"));

        let updated = VCF.replace("Ada Lovelace", "Ada Byron");
        ingest(&mut loom, ns, &projected, updated.as_bytes()).unwrap();
        let body = project(&loom, ns, &projected).unwrap().unwrap();
        assert!(String::from_utf8_lossy(&body).contains("FN:Ada Byron"));
        assert!(delete(&mut loom, ns, &projected).unwrap());
        assert!(project(&loom, ns, &projected).unwrap().is_none());
        assert_eq!(
            list_projected(&loom, ns, Facet::Contacts, "alice", "people").unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn records_survive_commit_and_checkout() {
        let (mut loom, ns) = loom_ns();
        calendar::create_collection(
            &mut loom,
            ns,
            "alice",
            "work",
            &calendar::CollectionMeta::default(),
        )
        .unwrap();
        let f = classify("calendar/alice/work/ev1.ics").unwrap();
        ingest(&mut loom, ns, &f, ICS.as_bytes()).unwrap();
        let c1 = loom.commit(ns, "alice", "one event", 1).unwrap();
        // Add a second, commit, then check out c1: the projection reflects the restored state.
        let ics2 = ICS.replace("ev1", "ev2");
        let f2 = classify("calendar/alice/work/ev2.ics").unwrap();
        ingest(&mut loom, ns, &f2, ics2.as_bytes()).unwrap();
        loom.commit(ns, "alice", "two events", 2).unwrap();
        assert_eq!(
            list_projected(&loom, ns, Facet::Calendar, "alice", "work")
                .unwrap()
                .len(),
            2
        );
        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(
            list_projected(&loom, ns, Facet::Calendar, "alice", "work").unwrap(),
            vec!["ev1.ics".to_string()]
        );
    }

    fn drop_raw<S: ObjectStore>(loom: &mut Loom<S>, ns: WorkspaceId, path: &str, bytes: &[u8]) {
        // Simulate an NFS-style raw drop: a working-tree file with no record yet.
        let parent = &path[..path.rfind('/').unwrap()];
        loom.create_directory(ns, parent, true).unwrap();
        loom.write_file(ns, path, bytes, 0o100644).unwrap();
    }

    #[test]
    fn reconcile_ingests_pending_dropped_files() {
        let (mut loom, ns) = loom_ns();
        calendar::create_collection(
            &mut loom,
            ns,
            "alice",
            "work",
            &calendar::CollectionMeta::default(),
        )
        .unwrap();
        // Two raw drops (as NFS would leave them): one valid, one garbage.
        drop_raw(&mut loom, ns, "calendar/alice/work/ev1.ics", ICS.as_bytes());
        drop_raw(&mut loom, ns, "calendar/alice/work/bad.ics", b"nope");
        let pending = pending_facet_files(&loom, ns).unwrap();
        assert_eq!(pending.len(), 2);
        let outcomes = reconcile(&mut loom, ns).unwrap();
        assert_eq!(outcomes.len(), 2);
        // The valid one became a record (raw removed); the bad one is quarantined (raw kept).
        assert!(!loom.exists(ns, "calendar/alice/work/ev1.ics").unwrap());
        assert!(loom.exists(ns, "calendar/alice/work/bad.ics").unwrap());
        assert_eq!(
            list_projected(&loom, ns, Facet::Calendar, "alice", "work").unwrap(),
            vec!["ev1.ics".to_string()]
        );
        // After reconciling, only the quarantined raw file remains pending.
        assert_eq!(pending_facet_files(&loom, ns).unwrap().len(), 1);
    }

    #[test]
    fn reconcile_quiescent_waits_for_stability() {
        let (mut loom, ns) = loom_ns();
        calendar::create_collection(
            &mut loom,
            ns,
            "alice",
            "work",
            &calendar::CollectionMeta::default(),
        )
        .unwrap();
        let mut seen = BTreeMap::new();
        // Tick 1: a fresh drop is seen but NOT ingested (could still be mid-write).
        drop_raw(&mut loom, ns, "calendar/alice/work/ev1.ics", ICS.as_bytes());
        assert!(
            reconcile_quiescent(&mut loom, ns, &mut seen)
                .unwrap()
                .is_empty()
        );
        assert!(loom.exists(ns, "calendar/alice/work/ev1.ics").unwrap());
        // Tick 2: unchanged since tick 1 -> finalized into a record.
        let out = reconcile_quiescent(&mut loom, ns, &mut seen).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(
            list_projected(&loom, ns, Facet::Calendar, "alice", "work").unwrap(),
            vec!["ev1.ics".to_string()]
        );
    }

    #[test]
    fn reconcile_quiescent_skips_a_file_still_changing() {
        let (mut loom, ns) = loom_ns();
        calendar::create_collection(
            &mut loom,
            ns,
            "alice",
            "work",
            &calendar::CollectionMeta::default(),
        )
        .unwrap();
        let mut seen = BTreeMap::new();
        drop_raw(
            &mut loom,
            ns,
            "calendar/alice/work/ev1.ics",
            b"BEGIN:VCALENDAR\r\n",
        );
        assert!(
            reconcile_quiescent(&mut loom, ns, &mut seen)
                .unwrap()
                .is_empty()
        ); // tick 1: new
        // The "writer" appends more before tick 2: content changed, so it is still not ingested.
        loom.write_file(ns, "calendar/alice/work/ev1.ics", ICS.as_bytes(), 0o100644)
            .unwrap();
        assert!(
            reconcile_quiescent(&mut loom, ns, &mut seen)
                .unwrap()
                .is_empty()
        ); // tick 2: changed
        // Now stable across a tick -> ingested on tick 3.
        assert_eq!(
            reconcile_quiescent(&mut loom, ns, &mut seen).unwrap().len(),
            1
        );
    }

    #[test]
    fn mail_uses_the_filename_stem_as_uid() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Mail, None, WorkspaceId::from_bytes([4; 16]))
            .unwrap();
        mail::create_mailbox(
            &mut loom,
            ns,
            "alice",
            "inbox",
            &mail::MailboxMeta::default(),
        )
        .unwrap();
        let raw = b"From: bob@x.io\r\nSubject: Hi\r\nDate: d\r\n\r\nbody";
        let f = classify("mail/alice/inbox/m1.eml").unwrap();
        let out = ingest(&mut loom, ns, &f, raw).unwrap();
        assert!(matches!(out, WriteOutcome::Stored { .. }));
        // The .eml projection is the byte-exact body from the CAS.
        assert_eq!(project(&loom, ns, &f).unwrap().as_deref(), Some(&raw[..]));
        assert_eq!(
            list_projected(&loom, ns, Facet::Mail, "alice", "inbox").unwrap(),
            vec!["m1.eml".to_string()]
        );
    }
}
