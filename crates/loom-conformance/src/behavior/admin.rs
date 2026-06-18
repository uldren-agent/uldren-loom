//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Execute the embedded `lock` behavioral suite against a fresh coordinator.
pub fn run_lock_behavior() -> Result<()> {
    let mut coordinator = LockCoordinator::default();
    let key = b"k";
    let owner = |principal: &str| LockOwner {
        principal: principal.to_string(),
        session: "s".to_string(),
    };

    let exclusive = coordinator.try_acquire(key, owner("a"), LockMode::Exclusive, 100, 0)?;
    let reentrant = coordinator.try_acquire(key, owner("a"), LockMode::Exclusive, 100, 1)?;
    assert_eq!(
        reentrant.fence, exclusive.fence,
        "reentrant exclusive acquire returns the same fence"
    );
    let err = coordinator
        .try_acquire(key, owner("b"), LockMode::Exclusive, 100, 1)
        .unwrap_err();
    assert_eq!(err.code, Code::Locked, "exclusive excludes another owner");
    coordinator.release(&exclusive, 1)?;
    coordinator.release(&exclusive, 1)?;

    coordinator.try_acquire(b"shared", owner("a"), LockMode::Shared, 100, 0)?;
    coordinator.try_acquire(b"shared", owner("b"), LockMode::Shared, 100, 0)?;
    let err = coordinator
        .try_acquire(b"shared", owner("c"), LockMode::Exclusive, 100, 0)
        .unwrap_err();
    assert_eq!(err.code, Code::Locked, "shared holders block exclusive");

    coordinator.try_acquire(
        b"sem",
        owner("a"),
        LockMode::Semaphore {
            permits: 2,
            capacity: 3,
        },
        100,
        0,
    )?;
    coordinator.try_acquire(
        b"sem",
        owner("b"),
        LockMode::Semaphore {
            permits: 1,
            capacity: 3,
        },
        100,
        0,
    )?;
    let err = coordinator
        .try_acquire(
            b"sem",
            owner("c"),
            LockMode::Semaphore {
                permits: 1,
                capacity: 3,
            },
            100,
            0,
        )
        .unwrap_err();
    assert_eq!(err.code, Code::Locked, "semaphore capacity is enforced");

    let expired = coordinator.try_acquire(b"expired", owner("a"), LockMode::Exclusive, 10, 0)?;
    let err = coordinator.release(&expired, 10).unwrap_err();
    assert_eq!(
        err.code,
        Code::LockLeaseExpired,
        "expired lease release reports the correct code"
    );
    coordinator.try_acquire(b"expired", owner("b"), LockMode::Exclusive, 10, 10)?;

    coordinator.try_acquire(b"bounded", owner("a"), LockMode::Exclusive, 10, 0)?;
    let err = bounded_acquire(
        &mut coordinator,
        b"bounded",
        owner("b"),
        LockMode::Exclusive,
        10,
        0,
        0,
    )
    .unwrap_err();
    assert_eq!(err.code, Code::Locked, "zero wait is non-blocking");
    let waited = bounded_acquire(
        &mut coordinator,
        b"bounded",
        owner("b"),
        LockMode::Exclusive,
        10,
        10,
        0,
    )?;
    assert_eq!(waited.owner.principal, "b");

    coordinator.apply_fence(b"fenced", Fence::embedded(7))?;
    let err = coordinator
        .apply_fence(b"fenced", Fence::embedded(6))
        .unwrap_err();
    assert_eq!(err.code, Code::FencingStale, "lower fences are stale");
    let err = coordinator
        .apply_fence(b"fenced", Fence::new(7, 3, 8))
        .unwrap_err();
    assert_eq!(
        err.code,
        Code::InvalidArgument,
        "embedded coordinator rejects external fences"
    );
    Ok(())
}

fn bounded_acquire(
    coordinator: &mut LockCoordinator,
    key: &[u8],
    owner: LockOwner,
    mode: LockMode,
    lease_ms: u64,
    wait_ms: u64,
    now_ms: u64,
) -> Result<LockToken> {
    let deadline = now_ms + wait_ms;
    let mut now = now_ms;
    loop {
        match coordinator.try_acquire(key.to_vec(), owner.clone(), mode, lease_ms, now) {
            Ok(token) => return Ok(token),
            Err(e) if e.code == Code::Locked && now < deadline => now += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Execute the `identity` behavioral suite against the in-memory principal registry.
pub fn run_identity_behavior() -> Result<()> {
    let root = WorkspaceId::from_bytes([1; 16]);
    let user = WorkspaceId::from_bytes([2; 16]);
    let mut identity = IdentityStore::new(root);
    assert_eq!(
        identity.effective_principal(None)?,
        root,
        "bootstrap root is effective before authentication is enforced"
    );

    identity.set_passphrase(root, "root", b"12345678")?;
    assert_eq!(
        identity.effective_principal(None).unwrap_err().code,
        Code::AuthenticationFailed,
        "setting a passphrase enables authenticated mode"
    );
    assert_eq!(
        identity
            .authenticate_passphrase(root, "wrong", "bad")
            .unwrap_err()
            .code,
        Code::AuthenticationFailed,
        "wrong passphrase fails closed"
    );
    assert_eq!(
        identity
            .authenticate_passphrase(root, "root", "root-session")?
            .principal,
        root,
        "correct passphrase creates a session"
    );

    identity.add_principal(user, "alice", PrincipalKind::User)?;
    identity.set_passphrase(user, "alice", b"abcdefgh")?;
    assert_eq!(
        identity.remove_principal(root).unwrap_err().code,
        Code::IdentityNoRootCredential,
        "a credentialed replacement without admin role cannot recover the store"
    );
    identity.assign_role(user, ROLE_ADMIN_ID)?;
    identity.remove_principal(root)?;
    assert_eq!(
        identity.root_principal(),
        None,
        "root can be removed after a credentialed admin replacement exists"
    );
    assert_eq!(
        identity
            .authenticate_passphrase(user, "alice", "user-session")?
            .principal,
        user,
        "replacement principal remains usable"
    );
    Ok(())
}

/// Execute the `acl` behavioral suite against the core evaluator and selected engine PEP hooks.
pub fn run_acl_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let principal = WorkspaceId::from_bytes([3; 16]);
    let ns = loom.registry_mut().create(
        FacetKind::Files,
        Some("acl-files"),
        WorkspaceId::from_bytes([4; 16]),
    )?;
    let kv_ns = loom.registry_mut().create(
        FacetKind::Kv,
        Some("acl-kv"),
        WorkspaceId::from_bytes([5; 16]),
    )?;

    let mut acl = AclStore::new();
    assert_eq!(
        acl.authorize(true, principal, ns, FacetKind::Files, AclRight::Read)
            .unwrap_err()
            .code,
        Code::PermissionDenied,
        "authenticated mode defaults to deny"
    );
    acl.allow(
        AclSubject::Everyone,
        Some(ns),
        Some(FacetKind::Files),
        [AclRight::Write],
    )?;
    acl.deny(
        AclSubject::Principal(principal),
        Some(ns),
        Some(FacetKind::Files),
        [AclRight::Write],
    )?;
    assert_eq!(
        acl.authorize(true, principal, ns, FacetKind::Files, AclRight::Write)
            .unwrap_err()
            .code,
        Code::PermissionDenied,
        "deny takes precedence over allow"
    );

    let mut identity = IdentityStore::new(principal);
    identity.set_passphrase(principal, "secret", b"12345678")?;
    let session = identity.authenticate_passphrase(principal, "secret", "session")?;
    loom.set_identity_store(identity);
    loom.set_session(session.id);
    assert_eq!(
        loom.write_file(ns, "a.txt", b"a", 0o100644)
            .unwrap_err()
            .code,
        Code::PermissionDenied,
        "engine write fails closed without a matching grant"
    );

    let mut engine_acl = AclStore::new();
    engine_acl.allow(
        AclSubject::Principal(principal),
        Some(ns),
        Some(FacetKind::Files),
        [AclRight::Read, AclRight::Write],
    )?;
    engine_acl.allow(
        AclSubject::Principal(principal),
        Some(ns),
        Some(FacetKind::Vcs),
        [AclRight::Read, AclRight::Write, AclRight::Advance],
    )?;
    engine_acl.allow(
        AclSubject::Principal(principal),
        Some(kv_ns),
        Some(FacetKind::Kv),
        [AclRight::Read, AclRight::Write, AclRight::Admin],
    )?;
    loom.set_acl_store(engine_acl);
    loom.write_file(ns, "a.txt", b"a", 0o100644)?;
    loom.commit(ns, "conformance", "acl", 1)?;
    assert_eq!(loom.read_file(ns, "a.txt")?, b"a");
    loom.configure_kv_map(kv_ns, "cache", KvMapConfig::EPHEMERAL)?;
    loom.kv_put_configured(
        kv_ns,
        "cache",
        Value::Text("k".into()),
        b"v".to_vec(),
        None,
        2,
    )?;
    assert_eq!(
        loom.kv_get_configured(kv_ns, "cache", &Value::Text("k".into()), 3)?
            .as_deref(),
        Some(&b"v"[..]),
        "configured KV operations pass with matching grants"
    );

    let mut role_identity = IdentityStore::new(principal);
    role_identity.set_passphrase(principal, "secret", b"12345678")?;
    role_identity.assign_role(principal, ROLE_READER_ID)?;
    let role_session =
        role_identity.authenticate_passphrase(principal, "secret", "role-session")?;
    loom.set_identity_store(role_identity);
    loom.set_session(role_session.id);

    let role_grant = AclGrant {
        subject: AclSubject::Role(ROLE_READER_ID),
        workspace: Some(ns),
        domain: Some(FacetKind::Files.into()),
        ref_glob: None,
        scopes: vec![AclScope::All],
        rights: [AclRight::Read].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    };
    let mut role_acl = AclStore::new();
    role_acl.grant(role_grant.clone())?;
    loom.set_acl_store(role_acl);
    assert_eq!(
        loom.read_file(ns, "a.txt")?,
        b"a",
        "a role grant must flow through the engine PEP"
    );
    loom.identity_store_mut()
        .expect("identity store installed")
        .revoke_role(principal, ROLE_READER_ID)?;
    assert_eq!(
        loom.read_file(ns, "a.txt").unwrap_err().code,
        Code::PermissionDenied,
        "revoking the role must affect the next engine operation"
    );

    let mut scoped_acl = AclStore::new();
    scoped_acl.grant(AclGrant {
        subject: AclSubject::Principal(principal),
        workspace: Some(ns),
        domain: Some(FacetKind::Files.into()),
        ref_glob: Some("branch/release-*".to_string()),
        scopes: vec![AclScope::Prefix {
            kind: AclScopeKind::Path,
            prefix: b"docs/".to_vec(),
        }],
        rights: [AclRight::Read].into_iter().collect(),
        effect: AclEffect::Allow,
        predicate: None,
    })?;
    scoped_acl.authorize_resource_with_roles(
        true,
        principal,
        [],
        AclResource::scoped(
            ns,
            FacetKind::Files,
            Some("branch/release-1"),
            AclResourceScope::Prefix {
                kind: AclScopeKind::Path,
                value: b"docs/a.txt",
            },
        ),
        AclRight::Read,
    )?;
    assert_eq!(
        scoped_acl
            .authorize_resource_with_roles(
                true,
                principal,
                [],
                AclResource::scoped(
                    ns,
                    FacetKind::Files,
                    Some("branch/main"),
                    AclResourceScope::Prefix {
                        kind: AclScopeKind::Path,
                        value: b"docs/a.txt",
                    },
                ),
                AclRight::Read,
            )
            .unwrap_err()
            .code,
        Code::PermissionDenied,
        "ref_glob must reject non-matching refs"
    );
    assert_eq!(
        scoped_acl
            .authorize_resource_with_roles(
                true,
                principal,
                [],
                AclResource::scoped(
                    ns,
                    FacetKind::Files,
                    Some("branch/release-1"),
                    AclResourceScope::Prefix {
                        kind: AclScopeKind::Path,
                        value: b"secrets/a.txt",
                    },
                ),
                AclRight::Read,
            )
            .unwrap_err()
            .code,
        Code::PermissionDenied,
        "path prefix scopes must reject sibling prefixes"
    );
    assert_eq!(
        scoped_acl
            .authorize_resource_with_roles(
                true,
                principal,
                [],
                AclResource::all(ns, FacetKind::Files),
                AclRight::Read,
            )
            .unwrap_err()
            .code,
        Code::PermissionDenied,
        "a prefix grant must not authorize a broad resource"
    );
    Ok(())
}

/// Execute the workspace behavioral suite against fresh source and destination [`Loom`] values. Fresh
/// Looms have no workspace, reads do not create, default writes create exactly one workspace, facets
/// coexist in the same workspace, deletion frees the name, bundles preserve the workspace id and facet
/// set, and cross-workspace operations are rejected.
pub fn run_workspace_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    imported: &mut Loom<T>,
) -> Result<()> {
    fn nid(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    assert!(
        loom.registry().list(None).is_empty(),
        "a fresh Loom must start with zero workspaces"
    );
    let missing = loom
        .registry()
        .open(&WsSelector::Default(FacetKind::Files))
        .unwrap_err();
    assert_eq!(
        missing.code,
        Code::NotFound,
        "reading Default must not create a workspace"
    );
    assert!(
        loom.registry().list(None).is_empty(),
        "a failed read must leave the workspace registry unchanged"
    );

    let default = loom
        .registry_mut()
        .ensure_for_write(&WsSelector::Default(FacetKind::Files), nid(1))?;
    let again = loom
        .registry_mut()
        .ensure_for_write(&WsSelector::Default(FacetKind::Sql), nid(2))?;
    assert_eq!(
        again, default,
        "a second default facet write must reuse the existing workspace"
    );
    assert_eq!(
        loom.registry().facets(default)?,
        vec![FacetKind::Files, FacetKind::Sql],
        "one workspace must be able to hold multiple facets"
    );

    loom.write_file(default, "README.md", b"workspace", 0o100644)?;
    // Reserved facet storage is written only through a typed facet facade (privileged); the public fs
    // facade now rejects user writes under the reserved `.loom` subtree (0014a baseline). Use the CAS
    // facade to populate a reserved path, then confirm a direct user write there is refused.
    let blob = cas_put(loom, default, b"sql-catalog")?;
    let reserved_blob = facet_path(FacetKind::Cas, &blob.to_hex());
    assert!(
        matches!(
            loom.write_file(default, &reserved_blob, b"forged", 0o100644),
            Err(ref e) if e.code == Code::PermissionDenied
        ),
        "a direct user write under the reserved .loom subtree must be rejected"
    );
    assert!(
        matches!(
            loom.create_directory(default, ".loom/facets/kv/main", true),
            Err(ref e) if e.code == Code::PermissionDenied
        ),
        "a direct user mkdir under the reserved .loom subtree must be rejected"
    );
    assert_eq!(
        loom.read_file(default, "/README.md")?,
        b"workspace",
        "user files must occupy root paths"
    );
    assert_eq!(
        loom.read_file(default, &format!("/{reserved_blob}"))?,
        b"sql-catalog",
        "a reserved facet path is readable (leading slash normalized) though not user-writable"
    );
    let tip = loom.commit(default, "conformance", "workspace", 1)?;
    assert_eq!(
        loom.registry()
            .branch_tip(default, loom_core::workspace::DEFAULT_BRANCH)?,
        Some(tip),
        "the committed branch tip must be present before bundling"
    );

    let bundle = bundle_export(loom, default)?;
    let (imported_ns, _) = bundle_import(imported, &bundle)?;
    assert_eq!(
        imported_ns, default,
        "bundle import must preserve the source workspace id"
    );
    assert_eq!(
        imported.registry().name(imported_ns)?,
        "Default",
        "bundle import must preserve the workspace name"
    );
    assert_eq!(
        imported.registry().facets(imported_ns)?,
        vec![FacetKind::Files, FacetKind::Sql],
        "bundle import must preserve the full facet set"
    );
    assert_eq!(
        imported
            .registry()
            .branch_tip(imported_ns, loom_core::workspace::DEFAULT_BRANCH)?,
        Some(tip),
        "bundle import must preserve branch refs"
    );
    imported.checkout_commit(imported_ns, tip)?;
    assert_eq!(
        imported.read_file(imported_ns, "README.md")?,
        b"workspace",
        "bundle checkout must preserve root user files"
    );
    assert_eq!(
        cas_get(imported, imported_ns, &blob)?.as_deref(),
        Some(b"sql-catalog".as_slice()),
        "bundle checkout must preserve reserved facet paths"
    );

    loom.registry_mut().delete(default)?;
    assert_eq!(
        loom.registry()
            .open(&WsSelector::Id(default))
            .unwrap_err()
            .code,
        Code::NotFound,
        "deleting a workspace must remove its id from the registry"
    );
    let recreated = loom
        .registry_mut()
        .ensure_for_write(&WsSelector::Default(FacetKind::Files), nid(4))?;
    assert_ne!(
        recreated, default,
        "recreating a deleted workspace must use the caller-provided new id"
    );

    let cross = loom_core::Registry::require_same(recreated, imported_ns).unwrap_err();
    assert_eq!(
        cross.code,
        Code::CrossWorkspace,
        "cross-workspace operations must be rejected"
    );

    Ok(())
}

/// Execute the sync behavioral suite against a source and destination [`Loom`].
pub fn run_sync_behavior<S: ObjectStore, T: ObjectStore>(
    src: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    fn nid(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    let src_ns = src.registry_mut().create(FacetKind::Files, None, nid(1))?;
    src.registry_mut().add_facet(src_ns, FacetKind::Sql)?;
    src.write_file(src_ns, "a.txt", b"alpha", 0o100644)?;
    src.create_directory(src_ns, "dir", false)?;
    src.write_file(src_ns, "dir/b.txt", b"bravo", 0o100644)?;
    let c0 = src.commit(src_ns, "conformance", "c0", 1)?;
    src.registry_mut().tag_create(src_ns, "v1", c0)?;
    let src_name = src.registry().name(src_ns)?;

    let (dst_ns, clone_report) = clone_workspace(src, src_ns, dst, nid(2))?;
    assert_eq!(dst_ns, nid(2), "clone must use the caller-provided new id");
    assert!(
        clone_report.objects_transferred > 0 && clone_report.objects_skipped == 0,
        "a clone into an empty destination must transfer every reachable object"
    );
    assert_eq!(
        dst.registry().name(dst_ns)?,
        src_name,
        "clone must preserve the workspace name"
    );
    assert_eq!(
        dst.registry()
            .branch_tip(dst_ns, loom_core::workspace::DEFAULT_BRANCH)?,
        Some(c0),
        "clone must preserve the branch tip"
    );
    assert_eq!(
        dst.registry().facets(dst_ns)?,
        vec![FacetKind::Files, FacetKind::Sql],
        "clone must preserve the facet set"
    );

    assert_eq!(
        dst.read_file(dst_ns, "a.txt").unwrap_err().code,
        Code::NotFound,
        "a bare clone must not expose a working tree before checkout"
    );
    dst.checkout_commit(dst_ns, c0)?;
    assert_eq!(
        dst.read_file(dst_ns, "a.txt")?,
        b"alpha",
        "checkout must materialize root files"
    );
    assert_eq!(
        dst.read_file(dst_ns, "dir/b.txt")?,
        b"bravo",
        "checkout must materialize nested files"
    );

    src.write_file(src_ns, "c.txt", b"charlie", 0o100644)?;
    let c1 = src.commit(src_ns, "conformance", "c1", 2)?;
    let push_report = push_branch(
        src,
        src_ns,
        loom_core::workspace::DEFAULT_BRANCH,
        dst,
        dst_ns,
    )?;
    assert_eq!(
        (push_report.objects_transferred, push_report.objects_skipped),
        (3, 0),
        "a fast-forward push must move only the genuinely new objects"
    );
    assert_eq!(
        dst.registry()
            .branch_tip(dst_ns, loom_core::workspace::DEFAULT_BRANCH)?,
        Some(c1),
        "a fast-forward push must advance the destination branch"
    );

    src.write_file(src_ns, "a.txt", b"src-side", 0o100644)?;
    let c2 = src.commit(src_ns, "conformance", "src", 3)?;
    dst.write_file(dst_ns, "a.txt", b"dst-side", 0o100644)?;
    dst.commit(dst_ns, "conformance", "dst", 3)?;
    let not_ff = push_branch(
        src,
        src_ns,
        loom_core::workspace::DEFAULT_BRANCH,
        dst,
        dst_ns,
    )
    .unwrap_err();
    assert_eq!(
        not_ff.code,
        Code::NotFastForward,
        "a divergent push must be refused as non-fast-forward"
    );

    dst.registry_mut().delete(dst_ns)?;

    let bundle = bundle_export(src, src_ns)?;
    assert_eq!(bundle.ns_id, src_ns, "bundle must carry the source id");
    assert_eq!(
        bundle.ns_name, src_name,
        "bundle must carry the source name"
    );
    assert_eq!(
        bundle.facets,
        vec![FacetKind::Files, FacetKind::Sql],
        "bundle must carry the full facet set"
    );
    assert_eq!(
        bundle.branches,
        vec![(loom_core::workspace::DEFAULT_BRANCH.to_string(), c2)],
        "bundle must carry branch refs at their current tips"
    );
    assert_eq!(
        bundle.tags,
        vec![("v1".to_string(), c0)],
        "bundle must carry tag refs"
    );

    let encoded = bundle.encode();
    let decoded = Bundle::decode(&encoded)?;
    assert_eq!(
        decoded, bundle,
        "encode then decode must round-trip exactly"
    );
    assert!(
        Bundle::decode(b"not a loom bundle").is_err(),
        "invalid bundle bytes must be rejected"
    );

    let (imported_ns, import_report) = bundle_import(dst, &decoded)?;
    assert_eq!(imported_ns, src_ns, "bundle import must preserve the id");
    assert_eq!(
        dst.registry().name(imported_ns)?,
        src_name,
        "bundle import must preserve the name"
    );
    assert_eq!(
        dst.registry().facets(imported_ns)?,
        vec![FacetKind::Files, FacetKind::Sql],
        "bundle import must preserve the facet set"
    );
    assert_eq!(
        dst.registry()
            .branch_tip(imported_ns, loom_core::workspace::DEFAULT_BRANCH)?,
        Some(c2),
        "bundle import must preserve branch refs"
    );
    assert_eq!(
        dst.registry().tag_target(imported_ns, "v1")?,
        Some(c0),
        "bundle import must preserve tag refs"
    );
    assert!(
        import_report.objects_transferred > 0,
        "importing into a destination missing the closure must transfer objects"
    );
    dst.checkout_commit(imported_ns, c2)?;
    assert_eq!(
        dst.read_file(imported_ns, "a.txt")?,
        b"src-side",
        "bundle import must carry the full object closure"
    );

    let dst_algo = dst.store().digest_algo();
    let mut mismatched = decoded;
    mismatched.digest_algo = if dst_algo == Algo::Blake3 {
        Algo::Sha256
    } else {
        Algo::Blake3
    };
    let conflict = bundle_import(dst, &mismatched).unwrap_err();
    assert_eq!(
        conflict.code,
        Code::Conflict,
        "an identity-profile mismatch must be rejected"
    );

    let auth_src_ns = src
        .registry_mut()
        .create(FacetKind::Files, Some("sync-auth"), nid(61))?;
    src.write_file(auth_src_ns, "a.txt", b"alpha", 0o100644)?;
    src.commit(auth_src_ns, "conformance", "auth-c0", 1)?;
    let (auth_dst_ns, _) = clone_workspace(src, auth_src_ns, dst, nid(62))?;
    src.write_file(auth_src_ns, "b.txt", b"bravo", 0o100644)?;
    src.commit(auth_src_ns, "conformance", "auth-c1", 2)?;

    let root = nid(63);
    let mut src_identity = IdentityStore::new(root);
    src_identity.set_passphrase(root, "root", b"12345678")?;
    let src_session = src_identity.authenticate_passphrase(root, "root", "src-session")?;
    src.set_identity_store(src_identity);
    src.set_session(src_session.id);
    let mut dst_identity = IdentityStore::new(root);
    dst_identity.set_passphrase(root, "root", b"12345678")?;
    let dst_session = dst_identity.authenticate_passphrase(root, "root", "dst-session")?;
    dst.set_identity_store(dst_identity);
    dst.set_session(dst_session.id);

    assert_eq!(
        push_branch(
            src,
            auth_src_ns,
            loom_core::workspace::DEFAULT_BRANCH,
            dst,
            auth_dst_ns,
        )
        .unwrap_err()
        .code,
        Code::PermissionDenied,
        "authenticated sync push must require source read"
    );
    src.acl_store_mut().allow(
        AclSubject::Principal(root),
        Some(auth_src_ns),
        None,
        [AclRight::Read],
    )?;
    dst.acl_store_mut().allow(
        AclSubject::Principal(root),
        Some(auth_dst_ns),
        None,
        [AclRight::Write],
    )?;
    assert_eq!(
        push_branch(
            src,
            auth_src_ns,
            loom_core::workspace::DEFAULT_BRANCH,
            dst,
            auth_dst_ns,
        )
        .unwrap_err()
        .code,
        Code::PermissionDenied,
        "authenticated sync push must require destination advance"
    );
    dst.acl_store_mut().allow(
        AclSubject::Principal(root),
        Some(auth_dst_ns),
        None,
        [AclRight::Advance],
    )?;
    push_branch(
        src,
        auth_src_ns,
        loom_core::workspace::DEFAULT_BRANCH,
        dst,
        auth_dst_ns,
    )?;

    Ok(())
}

/// Execute the queue behavioral suite against a source and destination [`Loom`]: append assigns 0 then
/// 1, len reflects appends, get returns a payload or absence, range is half-open and ordered,
/// commit then checkout restores the prior stream state, and a clone preserves the queue payloads.
pub fn run_queue_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    fn nid(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    let ns = loom.registry_mut().create(FacetKind::Queue, None, nid(1))?;

    assert_eq!(
        loom.stream_append(ns, "events", b"e0")?,
        0,
        "the first append is sequence 0"
    );
    assert_eq!(
        loom.stream_append(ns, "events", b"e1")?,
        1,
        "the second append is sequence 1"
    );
    assert_eq!(loom.stream_append(ns, "events", b"e2")?, 2);

    assert_eq!(
        loom.stream_len(ns, "events")?,
        3,
        "len reflects the appends"
    );

    assert_eq!(
        loom.stream_get(ns, "events", 1)?.as_deref(),
        Some(&b"e1"[..]),
        "get returns the payload at a sequence"
    );
    assert!(
        loom.stream_get(ns, "events", 9)?.is_none(),
        "an out-of-range get is absent"
    );

    assert_eq!(
        loom.stream_range(ns, "events", 0, 2)?,
        vec![b"e0".to_vec(), b"e1".to_vec()],
        "range is half-open [lo, hi) and ordered by sequence"
    );

    let c1 = loom.commit(ns, "conformance", "three", 1)?;
    loom.stream_append(ns, "events", b"e3")?;
    loom.commit(ns, "conformance", "four", 2)?;
    assert_eq!(loom.stream_len(ns, "events")?, 4);
    loom.checkout_commit(ns, c1)?;
    assert_eq!(
        loom.stream_len(ns, "events")?,
        3,
        "checkout restores the prior stream length"
    );
    assert_eq!(
        loom.stream_get(ns, "events", 2)?.as_deref(),
        Some(&b"e2"[..]),
        "checkout restores the prior payloads"
    );

    let (dst_ns, _) = clone_workspace(loom, ns, dst, nid(2))?;
    dst.checkout_commit(dst_ns, c1)?;
    assert_eq!(
        dst.stream_range(dst_ns, "events", 0, 3)?,
        vec![b"e0".to_vec(), b"e1".to_vec(), b"e2".to_vec()],
        "clone preserves the queue payloads"
    );

    Ok(())
}

/// Execute the queue consumer-offset suite: a missing offset reads as 0, read does not advance
/// progress, advance is monotonic, reset may move backward, checkout does not move the offset, invalid
/// ids and stream names are rejected, and a clone does not transfer consumer offsets.
pub fn run_consumer_behavior<S: ObjectStore, T: ObjectStore>(
    loom: &mut Loom<S>,
    dst: &mut Loom<T>,
) -> Result<()> {
    fn nid(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    let ns = loom.registry_mut().create(FacetKind::Queue, None, nid(1))?;
    for i in 0..3u8 {
        loom.stream_append(ns, "events", &[b'e', b'0' + i])?;
    }

    assert_eq!(
        loom.consumer_position(ns, "events", "worker")?,
        0,
        "a missing offset reads as next_seq 0"
    );
    let first = loom.consumer_read(ns, "events", "worker", 2)?;
    assert_eq!(first, vec![b"e0".to_vec(), b"e1".to_vec()]);
    assert_eq!(
        loom.consumer_position(ns, "events", "worker")?,
        0,
        "consumer_read must not advance the offset"
    );
    assert_eq!(
        loom.consumer_read(ns, "events", "worker", 2)?,
        first,
        "read without advance redelivers the same entries"
    );

    loom.consumer_advance(ns, "events", "worker", 2)?;
    assert_eq!(loom.consumer_position(ns, "events", "worker")?, 2);
    assert_eq!(
        loom.consumer_read(ns, "events", "worker", 10)?,
        vec![b"e2".to_vec()],
        "read resumes from the advanced offset"
    );
    assert!(
        loom.consumer_advance(ns, "events", "worker", 1).is_err(),
        "backward advance is rejected"
    );

    loom.consumer_reset(ns, "events", "worker", 0)?;
    assert_eq!(
        loom.consumer_position(ns, "events", "worker")?,
        0,
        "reset may move the offset backward"
    );
    loom.consumer_advance(ns, "events", "worker", 2)?;

    loom.stream_set_retained_low_water_mark(ns, "events", 2)?;
    assert_eq!(
        loom.consumer_reset(ns, "events", "stale-worker", 1)
            .unwrap_err()
            .code,
        Code::RetainedGap,
        "a stale queue cursor reports RETAINED_GAP"
    );
    assert_eq!(
        loom.consumer_position(ns, "events", "stale-worker")
            .unwrap_err()
            .code,
        Code::RetainedGap,
        "missing consumer offsets below the retained mark report RETAINED_GAP"
    );

    assert!(loom.consumer_position(ns, "events", "").is_err());
    assert!(loom.consumer_position(ns, "events", "a/b").is_err());
    assert!(loom.consumer_position(ns, "../escape", "worker").is_err());

    let tip = loom.commit(ns, "conformance", "snapshot", 1)?;
    loom.stream_append(ns, "events", b"e3")?;
    loom.commit(ns, "conformance", "grow", 2)?;
    loom.checkout_commit(ns, tip)?;
    assert_eq!(
        loom.consumer_position(ns, "events", "worker")?,
        2,
        "checkout must not mutate the consumer offset"
    );

    let (dst_ns, _) = clone_workspace(loom, ns, dst, nid(2))?;
    assert_eq!(
        dst.consumer_position(dst_ns, "events", "worker")?,
        0,
        "clone must not transfer consumer offsets"
    );

    Ok(())
}

/// Capability registry surface (0010 §5): the runtime report is non-empty, known capabilities resolve
/// with the expected version pair, proof status, and operational state, unknown names are absent,
/// `loom-core` defers downstream-owned and target capabilities, the contribution overlay sets support,
/// and negotiation intersects supported capabilities with overlapping versions. Cross-implementation:
/// the registry is identical on every backend, so this runs against any [`ObjectStore`].
pub fn run_capability_behavior<S: ObjectStore>(loom: &Loom<S>) -> Result<()> {
    use loom_core::{CapabilityOperationalState, CapabilityProof};

    let set = loom.capabilities();
    assert!(set.len() >= 40, "the registry reports the full catalog");

    let os = set
        .get("object-store")
        .expect("object-store is a registry capability");
    assert_eq!((os.current, os.minimum_compatible), (1, 1));
    assert_eq!(os.proof, CapabilityProof::Executable);
    assert_eq!(os.operational_state, CapabilityOperationalState::Supported);

    assert!(
        set.get("not-a-capability").is_none(),
        "unknown names are absent"
    );

    // The core view defers support it does not own (downstream crates) and target capabilities.
    for n in [
        "sql",
        "single-file-store",
        "compression",
        "encryption-at-rest",
        "rekey",
    ] {
        assert!(
            !set.supports(n),
            "{n} is downstream-owned; core must not assert it"
        );
    }
    assert!(
        !set.supports("acl") && !set.supports("exec"),
        "target capabilities are unsupported"
    );
    // Core facets are supported even when their proof status is only `scenario`.
    assert!(set.supports("kv") && set.supports("workspace") && set.supports("files"));

    // The contribution overlay is how a layer asserts support for what it owns.
    let overlaid = set.clone().with_state_overlay(
        &["sql", "single-file-store"],
        CapabilityOperationalState::Supported,
    );
    assert!(overlaid.supports("sql") && overlaid.supports("single-file-store"));
    assert!(
        !overlaid
            .clone()
            .with_state("nope", CapabilityOperationalState::Supported)
            .supports("nope")
    );

    // Two peers negotiate the intersection of supported capabilities with overlapping versions.
    let agreed: Vec<&str> = overlaid
        .negotiate(&overlaid)
        .iter()
        .map(|c| c.name)
        .collect();
    assert!(agreed.contains(&"object-store") && agreed.contains(&"sql"));
    assert!(
        !agreed.contains(&"acl"),
        "neither peer supports a target capability"
    );

    Ok(())
}
