//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Execute the workspace-scoped `calendar` facade suite (0037): collection lifecycle, entry CRUD with
/// an ETag that changes on edit, a put into a missing collection is `NOT_FOUND`, recurrence-expanded
/// `range` (EXDATE-aware) and `search`, the `diff_entries` change set, the iCalendar projection
/// round-trip (`put_ics`/`entry_ics`), commit/checkout versioning, and clone reachability.
pub fn run_calendar_facade_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    use loom_core::calendar;
    use loom_pim::calendar::{
        CalendarEntry, CollectionMeta, Component, DateTime, IcalDate, IcalMonth, IcalTime,
    };

    let ns =
        loom.registry_mut()
            .create(FacetKind::Calendar, None, WorkspaceId::from_bytes([40; 16]))?;

    // A put before the collection exists is NOT_FOUND (MKCALENDAR-before-PUT).
    let missing = calendar::put_entry(
        loom,
        ns,
        "alice",
        "work",
        &CalendarEntry::event("u0", "X", "20240101T090000"),
    );
    assert_eq!(
        missing.err().map(|e| e.code),
        Some(Code::NotFound),
        "put into a missing collection is NOT_FOUND"
    );

    calendar::create_collection(
        loom,
        ns,
        "alice",
        "work",
        &CollectionMeta {
            display_name: "Work".to_string(),
            component_set: vec![Component::Event],
        },
    )?;
    assert_eq!(
        calendar::list_collections(loom, ns, "alice")?,
        vec!["work".to_string()],
        "the created collection is listed"
    );

    // A recurring weekly event with one excluded occurrence, plus a one-off.
    let mut weekly = CalendarEntry::event("u1", "Standup", "20240101T090000");
    weekly.rrule = Some("FREQ=WEEKLY;BYDAY=MO".to_string());
    weekly.exdate = vec!["20240115T090000".to_string()];
    let tag1 = calendar::put_entry(loom, ns, "alice", "work", &weekly)?;
    calendar::put_entry(
        loom,
        ns,
        "alice",
        "work",
        &CalendarEntry::event("u2", "Review", "20240110T140000"),
    )?;

    // Editing the record changes the ETag (RD3).
    let mut weekly2 = weekly.clone();
    weekly2.summary = "Daily standup".to_string();
    let tag2 = calendar::put_entry(loom, ns, "alice", "work", &weekly2)?;
    assert_ne!(tag1, tag2, "an edit changes the content-addressed ETag");
    assert_eq!(
        calendar::get_entry(loom, ns, "alice", "work", "u1")?
            .unwrap()
            .summary,
        "Daily standup"
    );
    assert!(
        calendar::get_entry(loom, ns, "alice", "work", "absent")?.is_none(),
        "an absent uid reads as absent"
    );

    // Range expands recurrence within the window, EXDATE-aware, ordered by start.
    let mk = |y, mo, d, h, mi| -> DateTime {
        DateTime::new(
            IcalDate::from_calendar_date(y, IcalMonth::try_from(mo).unwrap(), d).unwrap(),
            IcalTime::from_hms(h, mi, 0).unwrap(),
        )
    };
    let occ = calendar::range(
        loom,
        ns,
        "alice",
        "work",
        mk(2024, 1, 1, 0, 0),
        mk(2024, 2, 1, 0, 0),
    )?;
    let days: Vec<(u8, &str)> = occ
        .iter()
        .map(|o| (o.start.day(), o.uid.as_str()))
        .collect();
    assert_eq!(
        days,
        [(1, "u1"), (8, "u1"), (10, "u2"), (22, "u1"), (29, "u1")],
        "range expands recurrence with EXDATE removed, ordered by start"
    );

    // Search by component and by summary substring.
    assert_eq!(
        calendar::search(loom, ns, "alice", "work", None, Some("review"))?.len(),
        1,
        "summary search is case-insensitive"
    );
    assert_eq!(
        calendar::search(loom, ns, "alice", "work", Some(Component::Event), None)?.len(),
        2,
        "both entries are events"
    );

    // iCalendar projection round-trip: import via put_ics, read back via entry_ics.
    let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:imported\r\nSUMMARY:Imported\r\nDTSTART:20240301T120000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    calendar::put_ics(loom, ns, "alice", "work", ics)?;
    let out = calendar::entry_ics(loom, ns, "alice", "work", "imported")?.expect("imported entry");
    assert!(
        out.contains("UID:imported") && out.contains("DTSTART:20240301T120000Z"),
        "the .ics projection serializes the stored record"
    );

    // diff_entries reports per-UID added/updated/removed.
    let old = calendar::list_entries(loom, ns, "alice", "work")?;
    calendar::delete_entry(loom, ns, "alice", "work", "u2")?;
    let new = calendar::list_entries(loom, ns, "alice", "work")?;
    let changes = calendar::diff_entries(loom, &old, &new);
    assert!(
        changes
            .iter()
            .any(|c| c.uid == "u2" && c.kind == calendar::ChangeKind::Removed),
        "the diff reports the removed entry"
    );

    // Commit/checkout versions the collection. After the deletes/imports above, c1 holds {u1, imported}.
    let c1 = loom.commit(ns, "conformance", "calendar c1", 1)?;
    let at_c1 = calendar::list_entries(loom, ns, "alice", "work")?.len();
    calendar::put_entry(
        loom,
        ns,
        "alice",
        "work",
        &CalendarEntry::event("u9", "Later", "20240401T090000"),
    )?;
    loom.commit(ns, "conformance", "calendar c2", 2)?;
    assert_eq!(
        calendar::list_entries(loom, ns, "alice", "work")?.len(),
        at_c1 + 1,
        "c2 adds one entry"
    );
    loom.checkout_commit(ns, c1)?;
    assert_eq!(
        calendar::list_entries(loom, ns, "alice", "work")?.len(),
        at_c1,
        "checkout restores the c1 entry set"
    );

    // Clone preserves the calendar object closure.
    let (dst_ns, report) = clone_workspace(loom, ns, dst, WorkspaceId::from_bytes([41; 16]))?;
    assert!(
        report.objects_transferred > 0,
        "a clone must transfer the calendar object closure"
    );
    dst.checkout_commit(dst_ns, c1)?;
    assert!(
        calendar::get_entry(dst, dst_ns, "alice", "work", "u1")?.is_some(),
        "clone preserves the entries"
    );
    Ok(())
}

/// Execute the PIM trigger bridge suite (0015/0029/0041): a trigger candidate resolves a content
/// addressed WASM program, runs through the compute facade, mutates a PIM facet through domain-shaped
/// host calls, and appends a fire record.
pub fn run_pim_trigger_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    use loom_compute::{
        Capability, Grant, GrantSet, Manifest, Mode, ResolvedTriggerProgram, Scope,
        TriggerExecutionState, TriggerFireDisposition, fire_trigger_candidate,
        fire_trigger_candidate_with_state,
    };
    use loom_core::calendar;
    use loom_core::{
        FireOutcome, OverlapPolicy, TriggerBinding, TriggerExecMode, TriggerFireCandidate,
        TriggerKind, TriggerOptions, TriggerStimulus,
    };
    use loom_pim::calendar::{CalendarEntry, CollectionMeta, Component};
    use std::collections::BTreeMap;

    let program_ns = loom.registry_mut().create(
        FacetKind::Program,
        Some("programs"),
        WorkspaceId::from_bytes([61; 16]),
    )?;
    let target_ns = loom.registry_mut().create(
        FacetKind::Calendar,
        Some("calendar"),
        WorkspaceId::from_bytes([62; 16]),
    )?;
    loom.commit(program_ns, "conformance", "init programs", 0)?;
    loom.commit(target_ns, "conformance", "init calendar", 0)?;

    let wasm = calendar_trigger_program()?;
    let grants = GrantSet::new(vec![Grant {
        facet: Capability::Calendar,
        mode: Mode::ReadWrite,
        scopes: vec![Scope::All],
    }]);
    let manifest = Manifest::for_wasm("calendar-trigger", &wasm, grants);
    let program = manifest.store(loom.store_mut())?;
    let stimulus = TriggerStimulus::Time {
        fired_at_ms: 60_000,
    };
    let trigger_id = WorkspaceId::from_bytes([63; 16]);
    let candidate = TriggerFireCandidate {
        binding: TriggerBinding {
            id: trigger_id,
            kind: TriggerKind::Time {
                cron: "0 * * * * *".to_string(),
                timezone: "UTC".to_string(),
            },
            program,
            target_workspace: target_ns,
            branch: "main".to_string(),
            budget: 2_000_000,
            mode: TriggerExecMode::Direct,
            options: TriggerOptions::default(),
            run_as: Some(WorkspaceId::from_bytes([64; 16])),
            enabled: true,
        },
        stimulus_digest: loom_core::stimulus_digest(Algo::Blake3, &stimulus)?,
        stimulus,
        fired_at_seq: 0,
    };
    let inputs = BTreeMap::from([
        (
            "cal_meta".to_string(),
            CollectionMeta {
                display_name: "Work".to_string(),
                component_set: vec![Component::Event],
            }
            .encode(),
        ),
        (
            "cal_entry".to_string(),
            CalendarEntry::event("u1", "Standup", "20240101T090000").encode(),
        ),
    ]);
    let resolver = |digest| {
        assert_eq!(digest, program, "the trigger resolves the stored program");
        Ok(ResolvedTriggerProgram {
            manifest: manifest.clone(),
            wasm: wasm.clone(),
            inputs: inputs.clone(),
        })
    };

    let report = fire_trigger_candidate(loom, program_ns, candidate, &resolver, 60_001)
        .map_err(|err| loom_core::LoomError::new(err.code(), err.to_string()))?;
    assert_eq!(report.record.outcome, FireOutcome::Applied);
    assert!(
        report.record.proposed.is_some(),
        "direct trigger execution records the committed result"
    );
    assert!(report.record.cost > 0, "fuel usage is recorded");
    assert_eq!(
        calendar::get_entry(loom, target_ns, "alice", "work", "u1")?
            .unwrap()
            .summary,
        "Standup",
        "the guest mutated the calendar facet through the PIM host ABI"
    );
    assert_eq!(
        loom_core::trigger_history(loom, program_ns, trigger_id, 0, 10)?,
        vec![report.record],
        "the fire record is appended to trigger history"
    );
    let skipped_id = WorkspaceId::from_bytes([65; 16]);
    let skipped_stimulus = TriggerStimulus::Time {
        fired_at_ms: 120_000,
    };
    let skipped_candidate = TriggerFireCandidate {
        binding: TriggerBinding {
            id: skipped_id,
            kind: TriggerKind::Time {
                cron: "0 * * * * *".to_string(),
                timezone: "UTC".to_string(),
            },
            program,
            target_workspace: target_ns,
            branch: "main".to_string(),
            budget: 2_000_000,
            mode: TriggerExecMode::Direct,
            options: TriggerOptions {
                overlap: OverlapPolicy::SkipIfRunning,
                ..TriggerOptions::default()
            },
            run_as: Some(WorkspaceId::from_bytes([64; 16])),
            enabled: true,
        },
        stimulus_digest: loom_core::stimulus_digest(Algo::Blake3, &skipped_stimulus)?,
        stimulus: skipped_stimulus,
        fired_at_seq: 0,
    };
    let skipped = fire_trigger_candidate_with_state(
        loom,
        program_ns,
        skipped_candidate,
        &resolver,
        120_001,
        &TriggerExecutionState::with_running([skipped_id]),
    )
    .map_err(|err| loom_core::LoomError::new(err.code(), err.to_string()))?;
    let TriggerFireDisposition::Skipped(skipped) = skipped else {
        return Err(loom_core::LoomError::new(
            loom_core::Code::Internal,
            "skip-if-running trigger did not return skipped disposition",
        ));
    };
    assert_eq!(skipped.record.outcome, FireOutcome::Skipped);
    assert_eq!(
        loom_core::trigger_history(loom, program_ns, skipped_id, 0, 10)?,
        vec![skipped.record],
        "skip-if-running is audited as a fire record"
    );
    let queued_id = WorkspaceId::from_bytes([66; 16]);
    let queued_stimulus = TriggerStimulus::Time {
        fired_at_ms: 180_000,
    };
    let queued_candidate = TriggerFireCandidate {
        binding: TriggerBinding {
            id: queued_id,
            kind: TriggerKind::Time {
                cron: "0 * * * * *".to_string(),
                timezone: "UTC".to_string(),
            },
            program,
            target_workspace: target_ns,
            branch: "main".to_string(),
            budget: 2_000_000,
            mode: TriggerExecMode::Direct,
            options: TriggerOptions {
                overlap: OverlapPolicy::Queue,
                ..TriggerOptions::default()
            },
            run_as: Some(WorkspaceId::from_bytes([64; 16])),
            enabled: true,
        },
        stimulus_digest: loom_core::stimulus_digest(Algo::Blake3, &queued_stimulus)?,
        stimulus: queued_stimulus,
        fired_at_seq: 0,
    };
    let queued = fire_trigger_candidate_with_state(
        loom,
        program_ns,
        queued_candidate.clone(),
        &resolver,
        180_001,
        &TriggerExecutionState::with_running([queued_id]),
    )
    .map_err(|err| loom_core::LoomError::new(err.code(), err.to_string()))?;
    assert_eq!(queued, TriggerFireDisposition::Queued(queued_candidate));
    assert!(
        loom_core::trigger_history(loom, program_ns, queued_id, 0, 10)?.is_empty(),
        "queue overlap must not append a deduping fire record"
    );
    Ok(())
}

fn calendar_trigger_program() -> Result<Vec<u8>> {
    wat::parse_str(
        r#"(module
             (import "env" "input_get" (func $in (param i32 i32 i32 i32) (result i32)))
             (import "env" "calendar_create_collection" (func $cal_create (param i32 i32 i32 i32 i32 i32)))
             (import "env" "calendar_put_entry" (func $cal_put (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
             (memory (export "memory") 1)
             (data (i32.const 0) "alice")
             (data (i32.const 16) "work")
             (data (i32.const 32) "cal_meta")
             (data (i32.const 48) "cal_entry")
             (func (export "run") (local $n i32)
               (local.set $n (call $in (i32.const 32)(i32.const 8)(i32.const 1000)(i32.const 256)))
               (call $cal_create (i32.const 0)(i32.const 5)(i32.const 16)(i32.const 4)(i32.const 1000)(local.get $n))
               (local.set $n (call $in (i32.const 48)(i32.const 9)(i32.const 1400)(i32.const 512)))
               (drop (call $cal_put (i32.const 0)(i32.const 5)(i32.const 16)(i32.const 4)(i32.const 1400)(local.get $n)(i32.const 1900)(i32.const 32)))))"#,
    )
    .map_err(|err| loom_core::LoomError::invalid(format!("calendar trigger program: {err}")))
}

/// Execute the workspace-scoped `contacts` facade suite (0038): book lifecycle, contact CRUD with an
/// ETag that changes on edit, a put into a missing book is `NOT_FOUND`, substring `search`, the vCard
/// projection round-trip (`put_vcard`/`entry_vcard`), `diff_entries`, commit/checkout versioning, and
/// clone reachability.
pub fn run_contacts_facade_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    use loom_core::contacts;
    use loom_pim::contacts::{BookMeta, ContactEntry, TypedValue};

    let ns =
        loom.registry_mut()
            .create(FacetKind::Contacts, None, WorkspaceId::from_bytes([42; 16]))?;

    let missing = contacts::put_entry(loom, ns, "alice", "personal", &ContactEntry::new("u0", "X"));
    assert_eq!(
        missing.err().map(|e| e.code),
        Some(Code::NotFound),
        "put into a missing book is NOT_FOUND"
    );
    contacts::create_book(
        loom,
        ns,
        "alice",
        "personal",
        &BookMeta {
            display_name: "Personal".to_string(),
        },
    )?;
    assert_eq!(
        contacts::list_books(loom, ns, "alice")?,
        vec!["personal".to_string()]
    );

    let mut a = ContactEntry::new("u1", "Ada Lovelace");
    a.emails = vec![TypedValue::new("ada@x.io")];
    let t1 = contacts::put_entry(loom, ns, "alice", "personal", &a)?;
    let mut a2 = a.clone();
    a2.org = Some("Analytical Engines".to_string());
    let t2 = contacts::put_entry(loom, ns, "alice", "personal", &a2)?;
    assert_ne!(t1, t2, "an edit changes the content-addressed ETag");
    contacts::put_entry(
        loom,
        ns,
        "alice",
        "personal",
        &ContactEntry::new("u2", "Bob"),
    )?;
    assert!(contacts::get_entry(loom, ns, "alice", "personal", "absent")?.is_none());

    assert_eq!(
        contacts::search(loom, ns, "alice", "personal", "lovelace")?.len(),
        1,
        "name search is case-insensitive"
    );
    assert_eq!(
        contacts::search(loom, ns, "alice", "personal", "ada@x")?.len(),
        1,
        "email search matches"
    );

    let vcf = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:imported\r\nFN:Imported Person\r\nEMAIL:i@x.io\r\nEND:VCARD\r\n";
    contacts::put_vcard(loom, ns, "alice", "personal", vcf)?;
    let out = contacts::entry_vcard(loom, ns, "alice", "personal", "imported")?.expect("imported");
    assert!(
        out.contains("FN:Imported Person") && out.contains("EMAIL:i@x.io"),
        "the vCard projection serializes the stored record"
    );

    let old = contacts::list_entries(loom, ns, "alice", "personal")?;
    let c1 = loom.commit(ns, "conformance", "contacts c1", 1)?;
    let at_c1 = contacts::list_entries(loom, ns, "alice", "personal")?.len();
    contacts::delete_entry(loom, ns, "alice", "personal", "u2")?;
    contacts::put_entry(
        loom,
        ns,
        "alice",
        "personal",
        &ContactEntry::new("u3", "Cara"),
    )?;
    let new = contacts::list_entries(loom, ns, "alice", "personal")?;
    let changes = contacts::diff_entries(loom, &old, &new);
    assert!(
        changes
            .iter()
            .any(|c| c.uid == "u2" && c.kind == contacts::ChangeKind::Removed)
            && changes
                .iter()
                .any(|c| c.uid == "u3" && c.kind == contacts::ChangeKind::Added),
        "the diff reports the removed and added contacts"
    );
    loom.commit(ns, "conformance", "contacts c2", 2)?;
    loom.checkout_commit(ns, c1)?;
    assert_eq!(
        contacts::list_entries(loom, ns, "alice", "personal")?.len(),
        at_c1,
        "checkout restores the c1 contact set"
    );

    let (dst_ns, report) = clone_workspace(loom, ns, dst, WorkspaceId::from_bytes([43; 16]))?;
    assert!(
        report.objects_transferred > 0,
        "a clone must transfer the contacts closure"
    );
    dst.checkout_commit(dst_ns, c1)?;
    assert!(
        contacts::get_entry(dst, dst_ns, "alice", "personal", "u1")?.is_some(),
        "clone preserves the contacts"
    );
    Ok(())
}

/// Execute the workspace-scoped `mail` facade suite (0039): mailbox lifecycle, message ingestion that
/// stores the immutable body in the CAS and parses the header index, byte-exact `.eml` body retrieval, a
/// missing-mailbox ingest is `NOT_FOUND`, independent flags (sorted/deduped, missing-message
/// `NOT_FOUND`), `search`, the added/removed `diff_messages`, commit/checkout versioning, and clone
/// reachability of both the index and the CAS body.
pub fn run_mail_facade_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    use loom_core::mail;
    use loom_pim::mail::MailboxMeta;

    let ns =
        loom.registry_mut()
            .create(FacetKind::Mail, None, WorkspaceId::from_bytes([44; 16]))?;

    let raw1: &[u8] = b"From: bob@x.io\r\nTo: alice@x.io\r\nSubject: Hello\r\nDate: d\r\nMessage-ID: <a@x>\r\n\r\nbody one";
    let missing = mail::ingest_message(loom, ns, "alice", "inbox", "m1", raw1);
    assert_eq!(
        missing.err().map(|e| e.code),
        Some(Code::NotFound),
        "ingest into a missing mailbox is NOT_FOUND"
    );
    mail::create_mailbox(
        loom,
        ns,
        "alice",
        "inbox",
        &MailboxMeta {
            display_name: "Inbox".to_string(),
        },
    )?;
    assert_eq!(
        mail::list_mailboxes(loom, ns, "alice")?,
        vec!["inbox".to_string()]
    );

    mail::ingest_message(loom, ns, "alice", "inbox", "m1", raw1)?;
    let msg = mail::get_message(loom, ns, "alice", "inbox", "m1")?.expect("m1");
    assert_eq!(msg.from, "bob@x.io");
    assert_eq!(msg.subject, "Hello");
    assert_eq!(
        mail::to_eml(loom, ns, "alice", "inbox", "m1")?.as_deref(),
        Some(raw1),
        "the body round-trips byte-for-byte from the CAS"
    );

    // Flags are an independent, sorted, deduplicated set; setting on a missing message is NOT_FOUND.
    assert!(mail::get_flags(loom, ns, "alice", "inbox", "m1")?.is_empty());
    mail::set_flags(
        loom,
        ns,
        "alice",
        "inbox",
        "m1",
        &[
            "\\Seen".to_string(),
            "Work".to_string(),
            "\\Seen".to_string(),
        ],
    )?;
    assert_eq!(
        mail::get_flags(loom, ns, "alice", "inbox", "m1")?,
        vec!["Work".to_string(), "\\Seen".to_string()]
    );
    assert_eq!(
        mail::set_flags(loom, ns, "alice", "inbox", "absent", &["x".to_string()])
            .err()
            .map(|e| e.code),
        Some(Code::NotFound)
    );

    // MX-236: mail retained-gap behavioral conformance. A stale incremental-sync cursor that
    // predates the retained detailed-history low-water mark maps to RETAINED_GAP (full-resync
    // recovery), which is distinct from NOT_FOUND for a missing mailbox/message (asserted above).
    // Mirrors the source-backed guard require_mutable_state_since.
    mail::set_flags(loom, ns, "alice", "inbox", "m1", &["Draft".to_string()])?;
    mail::set_flags(
        loom,
        ns,
        "alice",
        "inbox",
        "m1",
        &["Draft".to_string(), "Flagged".to_string()],
    )?;
    let mail_version = mail::mutable_state_version(loom, ns, "alice", "inbox")?;
    assert!(
        mail_version >= 1,
        "flag mutations advance the mutable-state version"
    );
    let compacted = mail::compact_mutable_state(loom, ns, "alice", "inbox", mail_version)?;
    assert_eq!(
        compacted.retained_since_version, mail_version,
        "compaction advances the retained low-water mark to the retained-from version"
    );
    assert_eq!(
        mail::require_mutable_state_since(loom, ns, "alice", "inbox", mail_version - 1)
            .err()
            .map(|e| e.code),
        Some(Code::RetainedGap),
        "a cursor predating retained detailed history is a retained gap requiring full resync"
    );
    mail::require_mutable_state_since(loom, ns, "alice", "inbox", mail_version)?;

    let raw2 = b"From: carol@x.io\r\nSubject: Lunch?\r\nDate: d\r\n\r\nbody two".to_vec();
    mail::ingest_message(loom, ns, "alice", "inbox", "m2", &raw2)?;
    assert_eq!(mail::search(loom, ns, "alice", "inbox", "lunch")?.len(), 1);
    assert_eq!(mail::search(loom, ns, "alice", "inbox", "bob@")?.len(), 1);

    let old = mail::list_messages(loom, ns, "alice", "inbox")?;
    let c1 = loom.commit(ns, "conformance", "mail c1", 1)?;
    let at_c1 = mail::list_messages(loom, ns, "alice", "inbox")?.len();
    mail::delete_message(loom, ns, "alice", "inbox", "m2")?;
    let raw3 = b"From: dave@x.io\r\nSubject: Re\r\nDate: d\r\n\r\nbody three".to_vec();
    mail::ingest_message(loom, ns, "alice", "inbox", "m3", &raw3)?;
    let new = mail::list_messages(loom, ns, "alice", "inbox")?;
    let changes = mail::diff_messages(&old, &new);
    assert!(
        changes
            .iter()
            .any(|c| c.uid == "m2" && c.kind == mail::ChangeKind::Removed)
            && changes
                .iter()
                .any(|c| c.uid == "m3" && c.kind == mail::ChangeKind::Added),
        "the diff reports the removed and added messages"
    );
    loom.commit(ns, "conformance", "mail c2", 2)?;
    loom.checkout_commit(ns, c1)?;
    assert_eq!(
        mail::list_messages(loom, ns, "alice", "inbox")?.len(),
        at_c1,
        "checkout restores the c1 message set"
    );

    let (dst_ns, report) = clone_workspace(loom, ns, dst, WorkspaceId::from_bytes([45; 16]))?;
    assert!(
        report.objects_transferred > 0,
        "a clone must transfer the mail + body closure"
    );
    dst.checkout_commit(dst_ns, c1)?;
    assert_eq!(
        mail::to_eml(dst, dst_ns, "alice", "inbox", "m1")?.as_deref(),
        Some(raw1),
        "clone preserves the immutable body in the CAS"
    );
    Ok(())
}
