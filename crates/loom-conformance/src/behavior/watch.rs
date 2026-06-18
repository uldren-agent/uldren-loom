//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

pub fn run_watch_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Files, None, WorkspaceId::from_bytes([61; 16]))?;
    loom.write_file(ns, "a.txt", b"a", 0o100644)?;
    let c0 = loom.commit(ns, "conformance", "watch c0", 1)?;
    loom.write_file(ns, "a.txt", b"a2", 0o100644)?;
    loom.write_file(ns, "b.txt", b"b", 0o100644)?;
    let c1 = loom.commit(ns, "conformance", "watch c1", 2)?;
    loom.remove_file(ns, "b.txt")?;
    let c2 = loom.commit(ns, "conformance", "watch c2", 3)?;

    let selector = WatchSelector::new(ns, DEFAULT_BRANCH)?;
    let tip_cursor = loom.watch_subscribe(&selector, None)?;
    assert_eq!(
        tip_cursor.commit,
        Some(c2),
        "subscribe without from starts at branch tip"
    );
    assert!(
        loom.watch_poll(&tip_cursor, 10)?.events.is_empty(),
        "tip cursor has no unseen events"
    );
    assert!(
        loom.watch_poll(&tip_cursor, 1)?.events.is_empty(),
        "maxed poll from tip cursor remains empty"
    );

    let replay = loom.watch_poll(&WatchCursor::new(ns, DEFAULT_BRANCH, None, 0)?, 10)?;
    assert_eq!(
        replay
            .events
            .iter()
            .map(|event| event.commit)
            .collect::<Vec<_>>(),
        vec![c0, c1, c2],
        "empty cursor replays history in commit order"
    );
    assert_eq!(
        replay
            .events
            .iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>(),
        vec![1, 2, 3],
        "sequence numbers are workspace-local commit order"
    );
    assert_eq!(
        replay.events[0].path_changes,
        vec![WatchPathChange {
            path: "a.txt".to_string(),
            kind: ChangeKind::Added,
        }],
        "root commit reports added paths"
    );
    assert_eq!(
        replay.events[1].path_changes,
        vec![
            WatchPathChange {
                path: "a.txt".to_string(),
                kind: ChangeKind::Modified,
            },
            WatchPathChange {
                path: "b.txt".to_string(),
                kind: ChangeKind::Added,
            },
        ],
        "child commit reports sorted path changes"
    );
    assert_eq!(
        replay.events[2].path_changes,
        vec![WatchPathChange {
            path: "b.txt".to_string(),
            kind: ChangeKind::Deleted,
        }],
        "delete event reports the removed path"
    );
    let a0 = Digest::hash(loom.store().digest_algo(), b"a");
    let a1 = Digest::hash(loom.store().digest_algo(), b"a2");
    let b1 = Digest::hash(loom.store().digest_algo(), b"b");
    assert_eq!(
        replay.events[0].changes,
        vec![DomainChange {
            domain: "files".to_string(),
            schema_version: 1,
            kind: "added".to_string(),
            key: b"a.txt".to_vec(),
            before: None,
            after: Some(a0),
            detail: None,
        }],
        "root commit reports deterministic file domain records"
    );
    assert_eq!(
        replay.events[1].changes,
        vec![
            DomainChange {
                domain: "files".to_string(),
                schema_version: 1,
                kind: "modified".to_string(),
                key: b"a.txt".to_vec(),
                before: Some(a0),
                after: Some(a1),
                detail: None,
            },
            DomainChange {
                domain: "files".to_string(),
                schema_version: 1,
                kind: "added".to_string(),
                key: b"b.txt".to_vec(),
                before: None,
                after: Some(b1),
                detail: None,
            },
        ],
        "child commit reports sorted file domain records"
    );
    assert_eq!(
        replay.events[2].changes,
        vec![DomainChange {
            domain: "files".to_string(),
            schema_version: 1,
            kind: "deleted".to_string(),
            key: b"b.txt".to_vec(),
            before: Some(b1),
            after: None,
            detail: None,
        }],
        "delete event reports file domain record with before digest"
    );
    assert_eq!(
        replay.next.commit,
        Some(c2),
        "replay advances the cursor to the newest returned commit"
    );

    let resumed = loom.watch_poll(&WatchCursor::new(ns, DEFAULT_BRANCH, Some(c0), 0)?, 1)?;
    assert_eq!(
        resumed
            .events
            .iter()
            .map(|event| event.commit)
            .collect::<Vec<_>>(),
        vec![c1],
        "resume cursor returns only later commits up to max"
    );
    assert_eq!(
        resumed.next.commit,
        Some(c1),
        "resume cursor advances to the returned event"
    );

    let err = loom
        .watch_poll(
            &WatchCursor::new(ns, DEFAULT_BRANCH, Some(Digest::blake3(b"foreign")), 0)?,
            10,
        )
        .unwrap_err();
    assert_eq!(
        err.code,
        Code::CursorInvalid,
        "unknown commit cursors are rejected"
    );

    let narrowed = WatchSelector::new(ns, DEFAULT_BRANCH)?
        .with_facet(FacetKind::Files)
        .with_path_prefix("a.");
    let narrowed_cursor = loom.watch_subscribe(&narrowed, Some(c0))?;
    let narrowed_replay = loom.watch_poll(&WatchCursor::decode(&narrowed_cursor.encode())?, 10)?;
    assert_eq!(
        narrowed_replay.events.len(),
        1,
        "path-prefix narrowing skips commits with no matching file changes"
    );
    assert_eq!(
        narrowed_replay.events[0].path_changes,
        vec![WatchPathChange {
            path: "a.txt".to_string(),
            kind: ChangeKind::Modified,
        }],
        "path-prefix narrowing keeps only matching file changes"
    );
    assert_eq!(
        narrowed_replay.next.commit,
        Some(c2),
        "filtered polling still advances across scanned commits"
    );

    let added_only = WatchSelector::new(ns, DEFAULT_BRANCH)?.with_change_kind(ChangeKind::Added);
    let added_replay = loom.watch_poll(&loom.watch_subscribe(&added_only, Some(c0))?, 10)?;
    assert_eq!(
        added_replay.events[0].path_changes,
        vec![WatchPathChange {
            path: "b.txt".to_string(),
            kind: ChangeKind::Added,
        }],
        "change-kind narrowing keeps only matching file changes"
    );

    let unsupported = WatchSelector::new(ns, DEFAULT_BRANCH)?.with_facet(FacetKind::Sql);
    assert_eq!(
        loom.watch_subscribe(&unsupported, None).unwrap_err().code,
        Code::Unsupported,
        "non-file domain narrowing remains unsupported in the pull baseline"
    );
    assert_eq!(
        watch_domain_support(FacetKind::Files).unwrap().detail,
        WatchDomainDetail::Stable,
        "files domain reports stable watch detail"
    );
    assert_eq!(
        watch_domain_support(FacetKind::Kv).unwrap().detail,
        WatchDomainDetail::Unsupported,
        "kv domain reports unsupported watch detail"
    );

    let kv_ns = loom.registry_mut().create(
        FacetKind::Kv,
        Some("watch-kv"),
        WorkspaceId::from_bytes([63; 16]),
    )?;
    kv_put(
        loom,
        kv_ns,
        "settings",
        Value::Text("theme".to_string()),
        b"dark".to_vec(),
    )?;
    let kv_commit = loom.commit(kv_ns, "conformance", "watch kv", 4)?;
    let kv_replay = loom.watch_poll(&WatchCursor::new(kv_ns, DEFAULT_BRANCH, None, 0)?, 10)?;
    assert_eq!(
        kv_replay
            .events
            .iter()
            .map(|event| event.commit)
            .collect::<Vec<_>>(),
        vec![kv_commit],
        "non-file-only revisions remain visible in the workspace feed"
    );
    assert!(
        kv_replay.events[0].changes.is_empty(),
        "unsupported non-file domains do not emit stable domain records"
    );
    assert_eq!(
        kv_replay.events[0].unsupported_domains,
        vec![UnsupportedDomainDetail {
            domain: "kv".to_string(),
            capability: "watch.domain.kv".to_string(),
        }],
        "unsupported non-file domain detail is capability-labeled"
    );

    let root = WorkspaceId::from_bytes([62; 16]);
    let mut identity = IdentityStore::new(root);
    identity.set_passphrase(root, "root", b"12345678")?;
    let session = identity.authenticate_passphrase(root, "root", "watch")?;
    loom.set_identity_store(identity);
    loom.set_session(session.id);

    assert_eq!(
        loom.watch_subscribe(&WatchSelector::new(ns, DEFAULT_BRANCH)?, Some(c0))
            .unwrap_err()
            .code,
        Code::PermissionDenied,
        "watch subscribe requires ref read access"
    );

    loom.acl_store_mut().allow(
        AclSubject::Principal(root),
        Some(ns),
        Some(FacetKind::Vcs),
        [AclRight::Read],
    )?;
    loom.acl_store_mut().grant(AclGrant {
        subject: AclSubject::Principal(root),
        workspace: Some(ns),
        domain: Some(FacetKind::Files.into()),
        ref_glob: None,
        scopes: vec![AclScope::Prefix {
            kind: AclScopeKind::Path,
            prefix: b"a.".to_vec(),
        }],
        rights: [AclRight::Read].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    })?;

    let authorized_cursor =
        loom.watch_subscribe(&WatchSelector::new(ns, DEFAULT_BRANCH)?, Some(c0))?;
    let authorized_replay = loom.watch_poll(&authorized_cursor, 10)?;
    assert_eq!(
        authorized_replay.events.len(),
        1,
        "watch replay omits commits with no authorized file changes"
    );
    assert_eq!(
        authorized_replay.events[0].path_changes,
        vec![WatchPathChange {
            path: "a.txt".to_string(),
            kind: ChangeKind::Modified,
        }],
        "watch replay omits unauthorized file paths"
    );
    Ok(())
}
