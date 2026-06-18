use super::*;
use crate::MemoryStore;
use crate::workspace::{DEFAULT_BRANCH, FacetKind, WorkspaceId};

fn nid(seed: u8) -> WorkspaceId {
    WorkspaceId::from_bytes([seed; 16])
}

fn new_vcs_ns(loom: &mut Loom<MemoryStore>, seed: u8) -> WorkspaceId {
    loom.registry_mut()
        .create(FacetKind::Files, None, nid(seed))
        .unwrap()
}

fn authenticate_root_with_files_rights(
    loom: &mut Loom<MemoryStore>,
    ns: WorkspaceId,
    rights: impl IntoIterator<Item = crate::AclRight>,
) {
    let root = nid(200);
    let mut identity = crate::IdentityStore::new(root);
    identity.set_passphrase(root, "root", b"12345678").unwrap();
    let session = identity
        .authenticate_passphrase(root, "root", "session")
        .unwrap();
    loom.set_identity_store(identity);
    loom.set_session(session.id);
    let rights: Vec<crate::AclRight> = rights.into_iter().collect();
    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(root),
            Some(ns),
            Some(FacetKind::Files),
            rights.iter().copied(),
        )
        .unwrap();
    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(root),
            Some(ns),
            Some(FacetKind::Vcs),
            rights,
        )
        .unwrap();
}

fn authenticate_root_without_grants(loom: &mut Loom<MemoryStore>) {
    let root = nid(200);
    let mut identity = crate::IdentityStore::new(root);
    identity.set_passphrase(root, "root", b"12345678").unwrap();
    let session = identity
        .authenticate_passphrase(root, "root", "session")
        .unwrap();
    loom.set_identity_store(identity);
    loom.set_session(session.id);
}

fn search_mapping() -> crate::search::Mapping {
    let mut mapping = crate::search::Mapping::new();
    mapping.insert("body".to_string(), crate::search::FieldMapping::text());
    mapping
}

fn search_doc(body: &str) -> crate::search::Document {
    let mut doc = crate::search::Document::new();
    doc.insert(
        "body".to_string(),
        crate::search::FieldValue::Text(body.to_string()),
    );
    doc
}

#[test]
fn unauthenticated_root_mode_bypasses_engine_acl() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 10);
    loom.set_identity_store(crate::IdentityStore::new(nid(1)));
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    assert_eq!(loom.read_file(ns, "a.txt").unwrap(), b"a");
}

#[test]
fn authenticated_file_operations_are_acl_checked() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 11);
    let root = nid(1);
    let mut identity = crate::IdentityStore::new(root);
    identity.set_passphrase(root, "root", b"12345678").unwrap();
    loom.set_identity_store(identity);
    assert_eq!(
        loom.write_file(ns, "a.txt", b"a", 0o100644)
            .unwrap_err()
            .code,
        Code::AuthenticationFailed
    );

    let session = loom
        .identity_store_mut()
        .unwrap()
        .authenticate_passphrase(root, "root", "session")
        .unwrap();
    loom.set_session(session.id);
    assert_eq!(
        loom.write_file(ns, "a.txt", b"a", 0o100644)
            .unwrap_err()
            .code,
        Code::PermissionDenied
    );

    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(root),
            Some(ns),
            Some(FacetKind::Files),
            [crate::AclRight::Read, crate::AclRight::Write],
        )
        .unwrap();
    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(root),
            Some(ns),
            Some(FacetKind::Vcs),
            [
                crate::AclRight::Read,
                crate::AclRight::Write,
                crate::AclRight::Advance,
            ],
        )
        .unwrap();
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.commit(ns, "root", "commit", 0).unwrap();
    assert_eq!(loom.read_file(ns, "a.txt").unwrap(), b"a");
}

#[test]
fn authenticated_file_operations_honor_path_scopes() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 31);
    let root = nid(200);
    authenticate_root_without_grants(&mut loom);
    loom.acl_store_mut()
        .grant(crate::AclGrant {
            subject: crate::AclSubject::Principal(root),
            workspace: Some(ns),
            domain: Some(FacetKind::Files.into()),
            ref_glob: None,
            scopes: vec![crate::AclScope::Prefix {
                kind: crate::AclScopeKind::Path,
                prefix: b"allowed".to_vec(),
            }],
            rights: [crate::AclRight::Read, crate::AclRight::Write]
                .into_iter()
                .collect(),
            effect: crate::AclEffect::Allow,
            predicate: None,
        })
        .unwrap();

    loom.write_file(ns, "allowed.txt", b"a", 0o100644).unwrap();
    assert_eq!(loom.read_file(ns, "allowed.txt").unwrap(), b"a");
    assert_eq!(
        loom.write_file(ns, "blocked.txt", b"b", 0o100644)
            .unwrap_err()
            .code,
        Code::PermissionDenied
    );
}

#[test]
fn authenticated_tag_lifecycle_requires_vcs_admin() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 13);
    authenticate_root_with_files_rights(
        &mut loom,
        ns,
        [
            crate::AclRight::Read,
            crate::AclRight::Write,
            crate::AclRight::Advance,
        ],
    );

    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    let commit = loom.commit(ns, "root", "commit", 0).unwrap();
    assert_eq!(loom.tag_list(ns).unwrap(), Vec::<String>::new());
    assert_eq!(
        loom.tag_create(ns, "v1", "HEAD", "root", "", 0)
            .unwrap_err()
            .code,
        Code::PermissionDenied
    );

    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(nid(200)),
            Some(ns),
            Some(FacetKind::Vcs),
            [crate::AclRight::Admin],
        )
        .unwrap();
    assert_eq!(
        loom.tag_create(ns, "v1", "HEAD", "root", "", 0).unwrap(),
        commit
    );
    assert_eq!(loom.tag_target(ns, "v1").unwrap(), Some(commit));
    loom.tag_delete(ns, "v1").unwrap();
}

#[test]
fn branch_delete_rejects_current_missing_and_protected_refs() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 14);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.commit(ns, "root", "base", 0).unwrap();
    loom.branch(ns, "scratch").unwrap();

    assert_eq!(
        loom.branch_delete(ns, DEFAULT_BRANCH).unwrap_err().code,
        Code::InvalidArgument
    );
    assert_eq!(
        loom.branch_delete(ns, "missing").unwrap_err().code,
        Code::NotFound
    );
    loom.branch_delete(ns, "scratch").unwrap();
    assert_eq!(loom.registry().branch_tip(ns, "scratch").unwrap(), None);

    loom.branch(ns, "retained").unwrap();
    loom.set_protected_ref_policy(
        ns,
        "branch/retained",
        ProtectedRefPolicy {
            retention_lock: true,
            governance_lock: true,
            ..ProtectedRefPolicy::default()
        },
    )
    .unwrap();
    assert_eq!(
        loom.branch_delete(ns, "retained").unwrap_err().code,
        Code::PermissionDenied
    );
    assert!(
        loom.registry()
            .branch_tip(ns, "retained")
            .unwrap()
            .is_some()
    );
}

#[test]
fn authenticated_branch_delete_requires_vcs_admin() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 15);
    authenticate_root_with_files_rights(
        &mut loom,
        ns,
        [
            crate::AclRight::Read,
            crate::AclRight::Write,
            crate::AclRight::Advance,
        ],
    );
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.commit(ns, "root", "base", 0).unwrap();
    loom.branch(ns, "scratch").unwrap();

    assert_eq!(
        loom.branch_delete(ns, "scratch").unwrap_err().code,
        Code::PermissionDenied
    );
    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(nid(200)),
            Some(ns),
            Some(FacetKind::Vcs),
            [crate::AclRight::Admin],
        )
        .unwrap();
    loom.branch_delete(ns, "scratch").unwrap();
}

#[test]
fn protected_ref_policy_persists_and_denies_non_fast_forward_rewrite() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 41);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    let base = loom.commit(ns, "root", "base", 0).unwrap();
    loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
    loom.commit(ns, "root", "next", 1).unwrap();
    loom.set_protected_ref_policy(
        ns,
        "branch/main",
        ProtectedRefPolicy {
            fast_forward_only: true,
            ..ProtectedRefPolicy::default()
        },
    )
    .unwrap();

    let bytes = loom.export_state();
    let mut restored = Loom::new(MemoryStore::new());
    restored.import_state(&bytes).unwrap();
    assert!(
        restored
            .protected_ref_policy(ns, "branch/main")
            .unwrap()
            .unwrap()
            .fast_forward_only
    );
    assert_eq!(
        loom.squash(ns, &base.to_string(), "root", "squashed", 2)
            .unwrap_err()
            .code,
        Code::PermissionDenied
    );
}

#[test]
fn save_state_uses_independently_rooted_sections() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 42);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.commit(ns, "root", "base", 0).unwrap();
    loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();

    let root = loom.save_state().unwrap();
    let Object::Tree(entries) = loom.get_object(&root).unwrap() else {
        panic!("engine-state root should be a section tree");
    };
    assert_eq!(entries.len(), 10);
    assert_eq!(entries[0].name, "00-registry");
    assert_eq!(entries[1].name, "01-content");
    assert_eq!(entries[9].name, "09-protected-refs");
    for entry in &entries {
        if entry.name == "01-content" || entry.name == "02-work" || entry.name == "07-index" {
            let Object::Tree(section_entries) = loom.get_object(&entry.target).unwrap() else {
                panic!("engine-state section should be a structured tree");
            };
            assert!(!section_entries.is_empty());
        } else {
            let Object::Blob(section) = loom.get_object(&entry.target).unwrap() else {
                panic!("engine-state section should be a blob");
            };
            assert!(!section.is_empty());
        }
    }

    let mut restored = Loom::new(loom.store.clone());
    restored.load_state(root).unwrap();
    assert_eq!(restored.read_file(ns, "b.txt").unwrap(), b"b".to_vec());
}

#[test]
fn load_state_rejects_malformed_structured_content_section() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 43);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();

    let root = loom.save_state().unwrap();
    let Object::Tree(mut entries) = loom.get_object(&root).unwrap() else {
        panic!("engine-state root should be a section tree");
    };
    let content_index = entries
        .iter()
        .position(|entry| entry.name == "01-content")
        .unwrap();
    let Object::Tree(mut content_entries) =
        loom.get_object(&entries[content_index].target).unwrap()
    else {
        panic!("engine-state content section should be a structured tree");
    };
    content_entries[0].kind = EntryKind::Blob;
    let bad_content = loom
        .put_object(&Object::tree(content_entries).unwrap())
        .unwrap();
    entries[content_index].target = bad_content;
    let bad_root = loom.put_object(&Object::tree(entries).unwrap()).unwrap();

    let mut restored = Loom::new(loom.store.clone());
    assert_eq!(
        restored.load_state(bad_root).unwrap_err().code,
        Code::CorruptObject
    );
}

#[test]
fn load_state_rejects_legacy_blob_content_section() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 44);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();

    let root = loom.save_state().unwrap();
    let Object::Tree(mut entries) = loom.get_object(&root).unwrap() else {
        panic!("engine-state root should be a section tree");
    };
    let content_index = entries
        .iter()
        .position(|entry| entry.name == "01-content")
        .unwrap();
    entries[content_index].target = loom.put_object(&Object::Blob(vec![0])).unwrap();
    let bad_root = loom.put_object(&Object::tree(entries).unwrap()).unwrap();

    let mut restored = Loom::new(loom.store.clone());
    assert_eq!(
        restored.load_state(bad_root).unwrap_err().code,
        Code::CorruptObject
    );
}

#[test]
fn load_state_rejects_malformed_structured_work_section() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 45);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();

    let root = loom.save_state().unwrap();
    let Object::Tree(mut entries) = loom.get_object(&root).unwrap() else {
        panic!("engine-state root should be a section tree");
    };
    let work_index = entries
        .iter()
        .position(|entry| entry.name == "02-work")
        .unwrap();
    let Object::Tree(mut work_entries) = loom.get_object(&entries[work_index].target).unwrap()
    else {
        panic!("engine-state work section should be a structured tree");
    };
    work_entries[0].kind = EntryKind::Blob;
    let bad_work = loom
        .put_object(&Object::tree(work_entries).unwrap())
        .unwrap();
    entries[work_index].target = bad_work;
    let bad_root = loom.put_object(&Object::tree(entries).unwrap()).unwrap();

    let mut restored = Loom::new(loom.store.clone());
    assert_eq!(
        restored.load_state(bad_root).unwrap_err().code,
        Code::CorruptObject
    );
}

#[test]
fn load_state_rejects_legacy_blob_work_section() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 46);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();

    let root = loom.save_state().unwrap();
    let Object::Tree(mut entries) = loom.get_object(&root).unwrap() else {
        panic!("engine-state root should be a section tree");
    };
    let work_index = entries
        .iter()
        .position(|entry| entry.name == "02-work")
        .unwrap();
    entries[work_index].target = loom.put_object(&Object::Blob(vec![0])).unwrap();
    let bad_root = loom.put_object(&Object::tree(entries).unwrap()).unwrap();

    let mut restored = Loom::new(loom.store.clone());
    assert_eq!(
        restored.load_state(bad_root).unwrap_err().code,
        Code::CorruptObject
    );
}

#[test]
fn load_state_rejects_legacy_blob_index_section() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 47);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();

    let root = loom.save_state().unwrap();
    let Object::Tree(mut entries) = loom.get_object(&root).unwrap() else {
        panic!("engine-state root should be a section tree");
    };
    let index = entries
        .iter()
        .position(|entry| entry.name == "07-index")
        .unwrap();
    entries[index].target = loom.put_object(&Object::Blob(vec![0])).unwrap();
    let bad_root = loom.put_object(&Object::tree(entries).unwrap()).unwrap();

    let mut restored = Loom::new(loom.store.clone());
    assert_eq!(
        restored.load_state(bad_root).unwrap_err().code,
        Code::CorruptObject
    );
}

#[test]
fn load_state_registry_loads_only_workspace_registry() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 46);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();

    let root = loom.save_state().unwrap();
    let mut restored = Loom::new(loom.store.clone());
    restored.load_state_registry(root).unwrap();

    assert!(restored.is_state_lazy());
    assert!(restored.registry().open(&crate::WsSelector::Id(ns)).is_ok());
    assert!(restored.content.is_empty());
    assert!(restored.work.is_empty());
    assert!(restored.dirs.is_empty());
    assert!(restored.index.is_empty());
}

#[test]
fn lazy_state_fails_closed_until_materialized() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 47);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();

    let root = loom.save_state().unwrap();
    let mut restored = Loom::new(loom.store.clone());
    restored.load_state_lazy(root).unwrap();

    assert_eq!(
        restored.read_file(ns, "a.txt").unwrap_err().code,
        Code::InvalidArgument
    );
    restored.ensure_full_state_loaded().unwrap();
    assert!(!restored.is_state_lazy());
    assert_eq!(restored.read_file(ns, "a.txt").unwrap(), b"a".to_vec());
}

#[test]
fn lazy_state_materializes_before_mutation() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 48);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();

    let root = loom.save_state().unwrap();
    let mut restored = Loom::new(loom.store.clone());
    restored.load_state_lazy(root).unwrap();
    restored.write_file(ns, "b.txt", b"b", 0o100644).unwrap();

    assert!(!restored.is_state_lazy());
    assert_eq!(restored.read_file(ns, "a.txt").unwrap(), b"a".to_vec());
    assert_eq!(restored.read_file(ns, "b.txt").unwrap(), b"b".to_vec());
}

#[test]
fn protected_ref_review_and_signature_requirements_fail_closed() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 42);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.commit(ns, "root", "base", 0).unwrap();
    loom.set_protected_ref_policy(
        ns,
        "branch/main",
        ProtectedRefPolicy {
            signed_commits_required: true,
            signed_ref_advance_required: true,
            required_review_count: 1,
            ..ProtectedRefPolicy::default()
        },
    )
    .unwrap();

    loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
    assert_eq!(
        loom.commit(ns, "root", "blocked", 1).unwrap_err().code,
        Code::PermissionDenied
    );
}

#[test]
fn protected_tag_retention_blocks_delete_and_rename() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 43);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.commit(ns, "root", "base", 0).unwrap();
    loom.tag_create(ns, "v1", "HEAD", "root", "", 0).unwrap();
    loom.set_protected_ref_policy(
        ns,
        "tag/v1",
        ProtectedRefPolicy {
            retention_lock: true,
            governance_lock: true,
            ..ProtectedRefPolicy::default()
        },
    )
    .unwrap();

    assert_eq!(
        loom.tag_delete(ns, "v1").unwrap_err().code,
        Code::PermissionDenied
    );
    assert_eq!(
        loom.tag_rename(ns, "v1", "v2").unwrap_err().code,
        Code::PermissionDenied
    );
    assert!(loom.tag_target(ns, "v1").unwrap().is_some());
}

#[test]
fn authenticated_history_rewrite_requires_vcs_admin() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 14);
    authenticate_root_with_files_rights(
        &mut loom,
        ns,
        [
            crate::AclRight::Read,
            crate::AclRight::Write,
            crate::AclRight::Advance,
        ],
    );

    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    let base = loom.commit(ns, "root", "base", 0).unwrap();
    loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
    loom.commit(ns, "root", "second", 1).unwrap();
    assert_eq!(
        loom.squash(ns, &base.to_string(), "root", "squashed", 2)
            .unwrap_err()
            .code,
        Code::PermissionDenied
    );

    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(nid(200)),
            Some(ns),
            Some(FacetKind::Vcs),
            [crate::AclRight::Admin],
        )
        .unwrap();
    let squashed = loom
        .squash(ns, &base.to_string(), "root", "squashed", 2)
        .unwrap();
    assert_eq!(
        loom.registry().branch_tip(ns, DEFAULT_BRANCH).unwrap(),
        Some(squashed)
    );
}

#[test]
fn authenticated_diff_requires_vcs_read_and_visible_commits() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 15);
    let other = loom
        .registry_mut()
        .create(FacetKind::Files, Some("other"), nid(16))
        .unwrap();
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    let c0 = loom.commit(ns, "root", "base", 0).unwrap();
    loom.write_file(ns, "a.txt", b"b", 0o100644).unwrap();
    let c1 = loom.commit(ns, "root", "next", 1).unwrap();
    loom.write_file(other, "secret.txt", b"secret", 0o100644)
        .unwrap();
    let foreign = loom.commit(other, "root", "secret", 0).unwrap();

    authenticate_root_without_grants(&mut loom);
    assert_eq!(
        loom.diff(ns, c0, c1).unwrap_err().code,
        Code::PermissionDenied
    );

    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(nid(200)),
            Some(ns),
            Some(FacetKind::Vcs),
            [crate::AclRight::Read],
        )
        .unwrap();
    assert_eq!(loom.diff(ns, c0, c1).unwrap().len(), 1);
    assert_eq!(
        loom.diff(ns, foreign, c1).unwrap_err().code,
        Code::PermissionDenied
    );
}

#[test]
fn authenticated_stage_all_and_checkout_require_vcs_write() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 17);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.commit(ns, "root", "base", 0).unwrap();
    loom.branch(ns, "feature").unwrap();
    loom.write_file(ns, "a.txt", b"b", 0o100644).unwrap();

    authenticate_root_with_files_rights(&mut loom, ns, [crate::AclRight::Read]);
    assert_eq!(loom.stage_all(ns).unwrap_err().code, Code::PermissionDenied);
    assert_eq!(
        loom.checkout_branch(ns, "feature").unwrap_err().code,
        Code::PermissionDenied
    );

    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(nid(200)),
            Some(ns),
            Some(FacetKind::Vcs),
            [crate::AclRight::Write],
        )
        .unwrap();
    loom.stage_all(ns).unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
}

#[test]
fn authenticated_merge_state_reads_require_vcs_read() {
    let (mut loom, ns, _) = conflict_scenario();
    loom.merge(ns, "feature", "root", 4).unwrap();
    authenticate_root_without_grants(&mut loom);
    assert_eq!(
        loom.merge_in_progress(ns).unwrap_err().code,
        Code::PermissionDenied
    );
    assert_eq!(
        loom.merge_conflicts(ns).unwrap_err().code,
        Code::PermissionDenied
    );

    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(nid(200)),
            Some(ns),
            Some(FacetKind::Vcs),
            [crate::AclRight::Read],
        )
        .unwrap();
    assert!(loom.merge_in_progress(ns).unwrap());
    assert_eq!(loom.merge_conflicts(ns).unwrap(), vec!["a.txt".to_string()]);
}

#[test]
fn authenticated_configured_kv_operations_are_acl_checked() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Kv, None, nid(12))
        .unwrap();
    let root = nid(1);
    let mut identity = crate::IdentityStore::new(root);
    identity.set_passphrase(root, "root", b"12345678").unwrap();
    let session = identity
        .authenticate_passphrase(root, "root", "session")
        .unwrap();
    loom.set_identity_store(identity);
    loom.set_session(session.id);

    assert_eq!(
        loom.configure_kv_map(ns, "cache", crate::KvMapConfig::EPHEMERAL)
            .unwrap_err()
            .code,
        Code::PermissionDenied
    );
    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(root),
            Some(ns),
            Some(FacetKind::Kv),
            [
                crate::AclRight::Admin,
                crate::AclRight::Read,
                crate::AclRight::Write,
            ],
        )
        .unwrap();
    loom.configure_kv_map(ns, "cache", crate::KvMapConfig::EPHEMERAL)
        .unwrap();
    loom.kv_put_configured(
        ns,
        "cache",
        crate::Value::Text("k".into()),
        b"v".to_vec(),
        None,
        0,
    )
    .unwrap();
    assert_eq!(
        loom.kv_get_configured(ns, "cache", &crate::Value::Text("k".into()), 1)
            .unwrap()
            .as_deref(),
        Some(&b"v"[..])
    );
}

#[test]
fn authenticated_configured_kv_operations_honor_key_scopes() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Kv, None, nid(32))
        .unwrap();
    loom.configure_kv_map(ns, "cache", crate::KvMapConfig::EPHEMERAL)
        .unwrap();
    let root = nid(200);
    authenticate_root_without_grants(&mut loom);
    let allowed_key = crate::Value::Text("allowed".into());
    let mut key_scope = b"cache\0".to_vec();
    key_scope.extend_from_slice(&crate::key_to_cbor(&allowed_key));
    loom.acl_store_mut()
        .grant(crate::AclGrant {
            subject: crate::AclSubject::Principal(root),
            workspace: Some(ns),
            domain: Some(FacetKind::Kv.into()),
            ref_glob: None,
            scopes: vec![crate::AclScope::Prefix {
                kind: crate::AclScopeKind::Key,
                prefix: key_scope,
            }],
            rights: [crate::AclRight::Read, crate::AclRight::Write]
                .into_iter()
                .collect(),
            effect: crate::AclEffect::Allow,
            predicate: None,
        })
        .unwrap();

    loom.kv_put_configured(ns, "cache", allowed_key.clone(), b"v".to_vec(), None, 0)
        .unwrap();
    assert_eq!(
        loom.kv_get_configured(ns, "cache", &allowed_key, 1)
            .unwrap()
            .as_deref(),
        Some(&b"v"[..])
    );
    assert_eq!(
        loom.kv_put_configured(
            ns,
            "cache",
            crate::Value::Text("blocked".into()),
            b"x".to_vec(),
            None,
            0,
        )
        .unwrap_err()
        .code,
        Code::PermissionDenied
    );
}

#[test]
fn reserved_loom_subtree_rejects_user_writes_but_allows_reads() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 9);
    loom.write_file(ns, "README.md", b"r", 0o100644).unwrap();
    // A typed facet facade writes under `.loom` through the privileged path; the blob then exists at
    // its reserved facet path.
    let blob = crate::cas::cas_put(&mut loom, ns, b"hi").unwrap();
    let reserved = crate::workspace::facet_path(crate::workspace::FacetKind::Cas, &blob.to_hex());

    // Reads and directory listings of the reserved subtree are allowed.
    assert_eq!(loom.read_file(ns, &reserved).unwrap(), b"hi");
    assert!(loom.list_directory(ns, ".loom/facets/cas").is_ok());
    assert!(loom.file_open(ns, &reserved, OpenMode::Read).is_ok());

    // Every public user mutator targeting the reserved subtree is rejected with EACCES.
    let denied = |r: Result<()>| assert_eq!(r.unwrap_err().code, Code::PermissionDenied);
    denied(loom.write_file(ns, ".loom/x", b"x", 0o100644));
    denied(loom.append_file(ns, ".loom/x", b"x"));
    denied(loom.remove_file(ns, &reserved));
    denied(loom.symlink(ns, "target", ".loom/link"));
    denied(loom.write_at(ns, ".loom/x", 0, b"x"));
    denied(loom.truncate_file(ns, ".loom/x", 4));
    denied(loom.create_directory(ns, ".loom/d", true));
    denied(loom.remove_directory(ns, ".loom/facets/cas", true));
    denied(loom.move_path(ns, &reserved, "out")); // reserved source
    denied(loom.move_path(ns, "README.md", ".loom/evil")); // reserved destination
    assert_eq!(
        loom.file_open(ns, ".loom/x", OpenMode::Write)
            .unwrap_err()
            .code,
        Code::PermissionDenied
    );

    // The guard did not disturb the facet data or ordinary root files.
    assert_eq!(
        crate::cas::cas_get(&loom, ns, &blob).unwrap().as_deref(),
        Some(b"hi".as_slice())
    );
    assert_eq!(loom.read_file(ns, "README.md").unwrap(), b"r");
}

#[test]
fn commit_then_log_and_read_back() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "/README.md", b"# loom", 0o100644)
        .unwrap();
    loom.create_directory(ns, "src", false).unwrap();
    loom.write_file(ns, "src/main.rs", b"fn main() {}", 0o100644)
        .unwrap();
    let c0 = loom.commit(ns, "nas", "init", 1).unwrap();
    loom.write_file(ns, "README.md", b"# loom v2", 0o100644)
        .unwrap();
    let c1 = loom.commit(ns, "nas", "update", 2).unwrap();

    assert_eq!(loom.log(ns, DEFAULT_BRANCH).unwrap(), vec![c1, c0]);
    // Working tree reads the latest staged content.
    assert_eq!(loom.read_file(ns, "README.md").unwrap(), b"# loom v2");
}

#[test]
fn large_file_chunks_and_round_trips() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    // ~6 MiB of deterministic bytes, over the chunk threshold, so it is stored as a ChunkList
    // of chunk Blobs rather than one Blob.
    let data: Vec<u8> = (0..6_000_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    loom.write_file(ns, "big.bin", &data, 0o100644).unwrap();
    // Reassembled read from the working tree is byte-identical.
    assert_eq!(loom.read_file(ns, "big.bin").unwrap(), data);

    let c = loom.commit(ns, "nas", "add big", 1).unwrap();
    // Many objects exist (multiple chunk Blobs + the ChunkList + tree + commit), proving chunking.
    assert!(
        loom.store().len() > 4,
        "expected a multi-chunk object graph, got {} objects",
        loom.store().len()
    );
    // Re-materializing the commit reassembles the same bytes.
    loom.checkout_commit(ns, c).unwrap();
    assert_eq!(loom.read_file(ns, "big.bin").unwrap(), data);
}

#[test]
fn compression_hint_defaults_by_facets_and_overrides_persist() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    // A files workspace defaults to the read-heavy "Small" preference.
    assert_eq!(loom.compression_for(ns), CompressionHint::Small);

    let vector_ns = loom
        .registry_mut()
        .create(FacetKind::Vector, Some("vectors"), nid(2))
        .unwrap();
    assert_eq!(loom.compression_for(vector_ns), CompressionHint::None);
    loom.registry_mut()
        .add_facet(vector_ns, FacetKind::Sql)
        .unwrap();
    assert_eq!(loom.compression_for(vector_ns), CompressionHint::Small);

    // An explicit override wins over the facet-derived default and is what writes carry.
    loom.set_workspace_compression(ns, CompressionHint::None);
    assert_eq!(loom.compression_for(ns), CompressionHint::None);

    // Writes still round-trip regardless of hint (MemoryStore ignores it).
    loom.write_file(ns, "f.txt", b"hello", 0o100644).unwrap();
    assert_eq!(loom.read_file(ns, "f.txt").unwrap(), b"hello");

    // The override survives an engine-state save/load round-trip; the facet-derived default of an
    // un-overridden workspace is recomputed from the (also-persisted) registry.
    let bytes = loom.export_state();
    let mut restored = Loom::new(MemoryStore::new());
    restored.import_state(&bytes).unwrap();
    assert_eq!(restored.compression_for(ns), CompressionHint::None);
}

#[test]
fn large_directory_shards_and_round_trips() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    // 300 files in one directory, over DIR_SHARD_THRESHOLD (256), so "big" is stored as a
    // prolly-sharded tree of `Tree`-object shard nodes, not a single flat Tree.
    loom.create_directory(ns, "big", false).unwrap();
    for i in 0..300u32 {
        let path = format!("big/file-{i:04}.txt");
        loom.write_file(ns, &path, format!("content-{i}").as_bytes(), 0o100644)
            .unwrap();
    }
    let c = loom.commit(ns, "nas", "many files", 1).unwrap();

    // The root holds a single entry "big"; "big" is the root shard Tree, an interior prolly node
    // whose entries are all `TreeShard` (no new object type; shard nodes are Trees).
    let root = loom.commit_tree(c).unwrap();
    let big = loom
        .tree_entries(root)
        .unwrap()
        .into_iter()
        .find(|e| e.name == "big")
        .expect("big dir");
    let Object::Tree(shard_entries) = loom.get_object(&big.target).unwrap() else {
        panic!("big should be a Tree object");
    };
    assert!(
        shard_entries.iter().all(|e| e.kind == EntryKind::TreeShard) && shard_entries.len() > 1,
        "big directory should be a sharded interior node, got {} entries",
        shard_entries.len()
    );
    // Resolving the shard transparently yields all 300 entries back.
    assert_eq!(loom.tree_entries(big.target).unwrap().len(), 300);

    // Re-materializing the commit reassembles every file from the sharded directory.
    loom.checkout_commit(ns, c).unwrap();
    for i in [0u32, 150, 299] {
        let path = format!("big/file-{i:04}.txt");
        assert_eq!(
            loom.read_file(ns, &path).unwrap(),
            format!("content-{i}").as_bytes()
        );
    }

    // GC reachability descends through the shard Trees: every shard node is retained as live.
    let live = loom.live_object_set(None).unwrap();
    assert!(live.contains(&big.target), "root shard Tree must be live");
    for shard in &shard_entries {
        assert!(
            live.contains(&shard.target),
            "interior shard Tree {} must be live",
            shard.target
        );
    }
}

#[test]
fn live_root_diagnostics_reports_bounded_source_classes() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "tracked.txt", b"v1", 0o100644).unwrap();
    let commit = loom.commit(ns, "nas", "v1", 1).unwrap();
    loom.tag_create(ns, "v1", &commit.to_string(), "nas", "", 1)
        .unwrap();
    loom.write_file(ns, "draft.txt", b"draft", 0o100644)
        .unwrap();

    let diagnostics = loom
        .live_root_diagnostics(Some(commit), Vec::new(), 1)
        .unwrap();
    let class = |name: &str| {
        diagnostics
            .classes
            .iter()
            .find(|class| class.class == name)
            .unwrap_or_else(|| panic!("missing class {name}"))
    };
    assert!(class("current_branch_tips").count >= 1);
    assert_eq!(class("current_tag_targets").count, 1);
    assert!(class("persisted_working_tree_roots").count >= 1);
    assert_eq!(class("current_reference_root").count, 1);
    assert_eq!(diagnostics.sample_limit, 1);
}

#[test]
fn sharded_directory_syncs_with_structural_sharing() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.create_directory(ns, "big", false).unwrap();
    for i in 0..300u32 {
        loom.write_file(
            ns,
            &format!("big/file-{i:04}.txt"),
            format!("content-{i}").as_bytes(),
            0o100644,
        )
        .unwrap();
    }
    let c1 = loom.commit(ns, "nas", "v1", 1).unwrap();
    // Everything a peer holds after receiving c1 (includes the sharded dir's shard Trees).
    let have = loom.reachable(&[c1], &BTreeSet::new()).unwrap();

    // Change exactly one file in the big directory and re-commit.
    loom.write_file(ns, "big/file-0150.txt", b"CHANGED", 0o100644)
        .unwrap();
    let c2 = loom.commit(ns, "nas", "v2", 2).unwrap();

    // The objects a peer that already has c1 must receive for c2: only the re-chunked shard spine
    // (a handful of shard Trees), the new root Tree and Commit, and the one changed blob, not the
    // ~300-entry directory. This is the structural-sharing payoff of prolly under Tree.
    let want = loom.reachable(&[c2], &have).unwrap();
    assert!(want.contains(&c2));
    assert!(
        want.len() < 30,
        "expected O(changed) transfer, got {} objects",
        want.len()
    );
}

#[test]
fn checkout_round_trips_a_commit() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"alpha", 0o100644).unwrap();
    loom.create_directory(ns, "dir", false).unwrap();
    loom.write_file(ns, "dir/b.txt", b"bravo", 0o100644)
        .unwrap();
    let c0 = loom.commit(ns, "nas", "c0", 1).unwrap();
    // Mutate the working tree, then check out the old commit to restore it.
    loom.write_file(ns, "a.txt", b"changed", 0o100644).unwrap();
    loom.remove_file(ns, "dir/b.txt").unwrap();
    loom.checkout_commit(ns, c0).unwrap();
    assert_eq!(loom.read_file(ns, "a.txt").unwrap(), b"alpha");
    assert_eq!(loom.read_file(ns, "dir/b.txt").unwrap(), b"bravo");
}

#[test]
fn diff_reports_path_changes() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "keep.txt", b"k", 0o100644).unwrap();
    loom.write_file(ns, "gone.txt", b"g", 0o100644).unwrap();
    let c0 = loom.commit(ns, "nas", "c0", 1).unwrap();
    loom.write_file(ns, "keep.txt", b"k2", 0o100644).unwrap();
    loom.remove_file(ns, "gone.txt").unwrap();
    loom.write_file(ns, "new.txt", b"n", 0o100644).unwrap();
    let c1 = loom.commit(ns, "nas", "c1", 2).unwrap();

    let changes = loom.diff(ns, c0, c1).unwrap();
    assert_eq!(
        changes,
        vec![
            Change {
                path: "gone.txt".into(),
                kind: ChangeKind::Deleted
            },
            Change {
                path: "keep.txt".into(),
                kind: ChangeKind::Modified
            },
            Change {
                path: "new.txt".into(),
                kind: ChangeKind::Added
            },
        ]
    );
}

#[test]
fn diff_commits_reports_cross_facet_envelope() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 11);
    loom.registry_mut().add_facet(ns, FacetKind::Sql).unwrap();
    loom.registry_mut().add_facet(ns, FacetKind::Queue).unwrap();
    loom.registry_mut().add_facet(ns, FacetKind::Cas).unwrap();

    let table_path = crate::workspace::facet_path(FacetKind::Sql, "app/tables/items");
    let schema = tabular::Schema::new(
        vec![
            ("id".to_string(), tabular::ColumnType::Int),
            ("name".to_string(), tabular::ColumnType::Text),
        ],
        vec![0],
    )
    .unwrap();
    let mut table = tabular::Table::new(schema);
    table
        .insert(vec![
            tabular::Value::Int(1),
            tabular::Value::Text("one".to_string()),
        ])
        .unwrap();
    loom.write_file(ns, "readme.txt", b"v1", 0o100644).unwrap();
    loom.stage_table(ns, &table_path, &table).unwrap();
    loom.stream_append(ns, "events", b"first").unwrap();
    crate::cas::cas_put(&mut loom, ns, b"blob-one").unwrap();
    let c0 = loom.commit(ns, "root", "c0", 1).unwrap();

    loom.write_file(ns, "readme.txt", b"v2", 0o100644).unwrap();
    loom.insert_row(
        ns,
        &table_path,
        vec![
            tabular::Value::Int(2),
            tabular::Value::Text("two".to_string()),
        ],
    )
    .unwrap();
    loom.stream_append(ns, "events", b"second").unwrap();
    crate::cas::cas_put(&mut loom, ns, b"blob-two").unwrap();
    let c1 = loom.commit(ns, "root", "c1", 2).unwrap();

    let diff = loom.diff_commits(ns, c0, c1).unwrap();
    let frame = cbor::decode_array(&diff).unwrap();
    assert_eq!(frame.len(), 6);
    assert_eq!(cbor::as_text(frame[0].clone()).unwrap(), "LMDIFF");
    assert_eq!(cbor::as_uint(frame[1].clone()).unwrap(), 1);
    assert_eq!(cbor::as_bytes(frame[2].clone()).unwrap(), ns.as_bytes());
    assert_eq!(cbor::as_digest(frame[3].clone()).unwrap(), c0);
    assert_eq!(cbor::as_digest(frame[4].clone()).unwrap(), c1);

    assert_eq!(
        collection_summary(&diff, "files", &[]),
        Some((0, 0, 1, 0, false, 1))
    );
    assert_eq!(
        collection_summary(&diff, "sql", &["app", "items"]),
        Some((1, 0, 0, 0, false, 1))
    );
    assert_eq!(
        collection_summary(&diff, "queue", &["events"]),
        Some((0, 0, 0, 1, false, 1))
    );
    assert_eq!(
        collection_summary(&diff, "cas", &[]),
        Some((1, 0, 0, 0, false, 1))
    );
}

#[test]
fn diff_commits_reports_vector_entry_units() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 12);
    loom.registry_mut()
        .add_facet(ns, FacetKind::Vector)
        .unwrap();

    crate::vector::vector_create(&mut loom, ns, "emb", 2, crate::vector::Metric::Cosine).unwrap();
    crate::vector::vector_upsert(&mut loom, ns, "emb", "a", vec![1.0, 0.0], BTreeMap::new())
        .unwrap();
    let c0 = loom.commit(ns, "root", "c0", 1).unwrap();

    crate::vector::vector_upsert(&mut loom, ns, "emb", "b", vec![0.0, 1.0], BTreeMap::new())
        .unwrap();
    let c1 = loom.commit(ns, "root", "c1", 2).unwrap();

    let diff = loom.diff_commits(ns, c0, c1).unwrap();
    assert_eq!(
        collection_summary(&diff, "vector", &["emb"]),
        Some((1, 0, 0, 0, false, 1))
    );
}

#[test]
fn diff_commits_reports_kv_key_units_for_prolly_roots() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 13);

    crate::kv::kv_put(
        &mut loom,
        ns,
        "cache",
        tabular::Value::Text("changed".to_string()),
        b"before".to_vec(),
    )
    .unwrap();
    crate::kv::kv_put(
        &mut loom,
        ns,
        "cache",
        tabular::Value::Text("removed".to_string()),
        b"gone".to_vec(),
    )
    .unwrap();
    let c0 = loom.commit(ns, "root", "c0", 1).unwrap();

    crate::kv::kv_put(
        &mut loom,
        ns,
        "cache",
        tabular::Value::Text("added".to_string()),
        b"new".to_vec(),
    )
    .unwrap();
    crate::kv::kv_put(
        &mut loom,
        ns,
        "cache",
        tabular::Value::Text("changed".to_string()),
        b"after".to_vec(),
    )
    .unwrap();
    crate::kv::kv_delete(
        &mut loom,
        ns,
        "cache",
        &tabular::Value::Text("removed".to_string()),
    )
    .unwrap();
    let c1 = loom.commit(ns, "root", "c1", 2).unwrap();

    let diff = loom.diff_commits(ns, c0, c1).unwrap();
    assert_eq!(
        collection_summary(&diff, "kv", &["cache"]),
        Some((1, 1, 1, 0, false, 3))
    );
    assert_eq!(
        collection_units(&diff, "kv", &["cache"]),
        vec![
            (
                "key".to_string(),
                crate::kv::key_to_cbor(&tabular::Value::Text("added".to_string())),
                "added".to_string(),
                false,
                true
            ),
            (
                "key".to_string(),
                crate::kv::key_to_cbor(&tabular::Value::Text("changed".to_string())),
                "changed".to_string(),
                true,
                true
            ),
            (
                "key".to_string(),
                crate::kv::key_to_cbor(&tabular::Value::Text("removed".to_string())),
                "removed".to_string(),
                true,
                false
            ),
        ]
    );
}

#[test]
fn diff_commits_reports_search_document_units() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 15);
    loom.registry_mut()
        .add_facet(ns, FacetKind::Search)
        .unwrap();

    crate::search::search_create(&mut loom, ns, "idx", search_mapping()).unwrap();
    crate::search::search_index(
        &mut loom,
        ns,
        "idx",
        b"changed".to_vec(),
        search_doc("before"),
    )
    .unwrap();
    crate::search::search_index(
        &mut loom,
        ns,
        "idx",
        b"removed".to_vec(),
        search_doc("gone"),
    )
    .unwrap();
    let c0 = loom.commit(ns, "root", "c0", 1).unwrap();

    let mut remapped = search_mapping();
    remapped.insert("tag".to_string(), crate::search::FieldMapping::keyword());
    crate::search::search_remap(&mut loom, ns, "idx", remapped).unwrap();
    crate::search::search_index(&mut loom, ns, "idx", b"added".to_vec(), search_doc("new"))
        .unwrap();
    crate::search::search_index(
        &mut loom,
        ns,
        "idx",
        b"changed".to_vec(),
        search_doc("after"),
    )
    .unwrap();
    crate::search::search_delete(&mut loom, ns, "idx", b"removed").unwrap();
    let c1 = loom.commit(ns, "root", "c1", 2).unwrap();

    let diff = loom.diff_commits(ns, c0, c1).unwrap();
    assert_eq!(
        collection_summary(&diff, "search", &["idx"]),
        Some((1, 1, 2, 0, false, 4))
    );
    assert_eq!(
        collection_units(&diff, "search", &["idx"]),
        vec![
            (
                "document".to_string(),
                cbor::encode(&cbor::Value::Bytes(b"added".to_vec())),
                "added".to_string(),
                false,
                true
            ),
            (
                "document".to_string(),
                cbor::encode(&cbor::Value::Bytes(b"changed".to_vec())),
                "changed".to_string(),
                true,
                true
            ),
            (
                "document".to_string(),
                cbor::encode(&cbor::Value::Bytes(b"removed".to_vec())),
                "removed".to_string(),
                true,
                false
            ),
            (
                "mapping".to_string(),
                cbor::encode(&cbor::Value::Text("mapping".to_string())),
                "changed".to_string(),
                true,
                true
            ),
        ]
    );
}

#[test]
fn merge_conflicts_on_divergent_kv_collection_roots() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 14);

    crate::kv::kv_put(
        &mut loom,
        ns,
        "cache",
        tabular::Value::Text("shared".to_string()),
        b"base".to_vec(),
    )
    .unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.branch(ns, "feature").unwrap();

    loom.checkout_branch(ns, "feature").unwrap();
    crate::kv::kv_put(
        &mut loom,
        ns,
        "cache",
        tabular::Value::Text("shared".to_string()),
        b"theirs".to_vec(),
    )
    .unwrap();
    loom.commit(ns, "nas", "feature", 2).unwrap();

    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    crate::kv::kv_put(
        &mut loom,
        ns,
        "cache",
        tabular::Value::Text("shared".to_string()),
        b"ours".to_vec(),
    )
    .unwrap();
    let ours = loom.commit(ns, "nas", "main", 3).unwrap();

    let conflict_path = crate::workspace::facet_path(FacetKind::Kv, "cache");
    assert_eq!(
        loom.merge(ns, "feature", "nas", 4).unwrap(),
        MergeOutcome::Conflicts(vec![conflict_path.clone()])
    );
    assert_eq!(
        loom.registry().branch_tip(ns, DEFAULT_BRANCH).unwrap(),
        Some(ours)
    );
    assert!(loom.merge_in_progress(ns).unwrap());
    assert_eq!(loom.merge_conflicts(ns).unwrap(), vec![conflict_path]);
    assert_eq!(
        crate::kv::kv_get(
            &loom,
            ns,
            "cache",
            &tabular::Value::Text("shared".to_string())
        )
        .unwrap(),
        Some(b"ours".to_vec())
    );
}

#[test]
fn merge_conflicts_on_divergent_search_collection_roots() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 16);
    loom.registry_mut()
        .add_facet(ns, FacetKind::Search)
        .unwrap();

    crate::search::search_create(&mut loom, ns, "idx", search_mapping()).unwrap();
    crate::search::search_index(&mut loom, ns, "idx", b"shared".to_vec(), search_doc("base"))
        .unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.branch(ns, "feature").unwrap();

    loom.checkout_branch(ns, "feature").unwrap();
    crate::search::search_index(
        &mut loom,
        ns,
        "idx",
        b"shared".to_vec(),
        search_doc("theirs"),
    )
    .unwrap();
    loom.commit(ns, "nas", "feature", 2).unwrap();

    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    crate::search::search_index(&mut loom, ns, "idx", b"shared".to_vec(), search_doc("ours"))
        .unwrap();
    let ours = loom.commit(ns, "nas", "main", 3).unwrap();

    let conflict_path = crate::workspace::facet_path(FacetKind::Search, "idx");
    assert_eq!(
        loom.merge(ns, "feature", "nas", 4).unwrap(),
        MergeOutcome::Conflicts(vec![conflict_path.clone()])
    );
    assert_eq!(
        loom.registry().branch_tip(ns, DEFAULT_BRANCH).unwrap(),
        Some(ours)
    );
    assert!(loom.merge_in_progress(ns).unwrap());
    assert_eq!(loom.merge_conflicts(ns).unwrap(), vec![conflict_path]);
    assert_eq!(
        crate::search::search_get(&loom, ns, "idx", b"shared")
            .unwrap()
            .and_then(|doc| doc.get("body").cloned()),
        Some(crate::search::FieldValue::Text("ours".to_string()))
    );
}

#[test]
fn merge_reconciles_independent_search_document_edits() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 17);
    loom.registry_mut()
        .add_facet(ns, FacetKind::Search)
        .unwrap();

    crate::search::search_create(&mut loom, ns, "idx", search_mapping()).unwrap();
    crate::search::search_index(&mut loom, ns, "idx", b"base".to_vec(), search_doc("base"))
        .unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.branch(ns, "feature").unwrap();

    loom.checkout_branch(ns, "feature").unwrap();
    crate::search::search_index(
        &mut loom,
        ns,
        "idx",
        b"theirs".to_vec(),
        search_doc("theirs"),
    )
    .unwrap();
    loom.commit(ns, "nas", "feature", 2).unwrap();

    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    crate::search::search_index(&mut loom, ns, "idx", b"ours".to_vec(), search_doc("ours"))
        .unwrap();
    loom.commit(ns, "nas", "main", 3).unwrap();

    let outcome = loom.merge(ns, "feature", "nas", 4).unwrap();
    assert!(matches!(outcome, MergeOutcome::Merged(_)), "{outcome:?}");
    assert_eq!(
        crate::search::search_ids(&loom, ns, "idx", None).unwrap(),
        vec![b"base".to_vec(), b"ours".to_vec(), b"theirs".to_vec()]
    );
    assert!(!loom.merge_in_progress(ns).unwrap());
}

#[test]
fn merge_conflicts_on_search_mapping_change() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 18);
    loom.registry_mut()
        .add_facet(ns, FacetKind::Search)
        .unwrap();

    crate::search::search_create(&mut loom, ns, "idx", search_mapping()).unwrap();
    crate::search::search_index(&mut loom, ns, "idx", b"base".to_vec(), search_doc("base"))
        .unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.branch(ns, "feature").unwrap();

    loom.checkout_branch(ns, "feature").unwrap();
    crate::search::search_index(
        &mut loom,
        ns,
        "idx",
        b"theirs".to_vec(),
        search_doc("theirs"),
    )
    .unwrap();
    loom.commit(ns, "nas", "feature", 2).unwrap();

    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    let mut remapped = search_mapping();
    remapped.insert("tag".to_string(), crate::search::FieldMapping::keyword());
    crate::search::search_remap(&mut loom, ns, "idx", remapped).unwrap();
    let ours = loom.commit(ns, "nas", "main", 3).unwrap();

    let conflict_path = crate::workspace::facet_path(FacetKind::Search, "idx");
    assert_eq!(
        loom.merge(ns, "feature", "nas", 4).unwrap(),
        MergeOutcome::Conflicts(vec![conflict_path.clone()])
    );
    assert_eq!(
        loom.registry().branch_tip(ns, DEFAULT_BRANCH).unwrap(),
        Some(ours)
    );
    assert_eq!(loom.merge_conflicts(ns).unwrap(), vec![conflict_path]);
}

#[test]
fn merge_conflicts_on_divergent_search_alias_changes() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 19);
    loom.registry_mut()
        .add_facet(ns, FacetKind::Search)
        .unwrap();

    crate::search::search_create(&mut loom, ns, "idx1", search_mapping()).unwrap();
    crate::search::search_create(&mut loom, ns, "idx2", search_mapping()).unwrap();
    crate::search::search_alias_set(
        &mut loom,
        ns,
        "all",
        crate::search::SearchAlias {
            targets: vec![crate::search::SearchAliasTarget {
                collection: "idx1".to_string(),
                is_write_index: true,
            }],
        },
    )
    .unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.branch(ns, "feature").unwrap();

    loom.checkout_branch(ns, "feature").unwrap();
    crate::search::search_alias_set(
        &mut loom,
        ns,
        "all",
        crate::search::SearchAlias {
            targets: vec![crate::search::SearchAliasTarget {
                collection: "idx2".to_string(),
                is_write_index: true,
            }],
        },
    )
    .unwrap();
    loom.commit(ns, "nas", "feature", 2).unwrap();

    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    crate::search::search_alias_set(
        &mut loom,
        ns,
        "all",
        crate::search::SearchAlias {
            targets: vec![crate::search::SearchAliasTarget {
                collection: "idx1".to_string(),
                is_write_index: false,
            }],
        },
    )
    .unwrap();
    let ours = loom.commit(ns, "nas", "main", 3).unwrap();

    let conflict_path = crate::workspace::facet_path(FacetKind::Search, ".aliases/all");
    assert_eq!(
        loom.merge(ns, "feature", "nas", 4).unwrap(),
        MergeOutcome::Conflicts(vec![conflict_path.clone()])
    );
    assert_eq!(
        loom.registry().branch_tip(ns, DEFAULT_BRANCH).unwrap(),
        Some(ours)
    );
    assert_eq!(loom.merge_conflicts(ns).unwrap(), vec![conflict_path]);
}

#[test]
fn merge_reconciles_vector_entry_disjoint_fields() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 15);
    loom.registry_mut()
        .add_facet(ns, FacetKind::Vector)
        .unwrap();

    crate::vector::vector_create(&mut loom, ns, "emb", 2, crate::vector::Metric::Cosine).unwrap();
    let mut metadata = BTreeMap::new();
    metadata.insert("tag".to_string(), tabular::Value::Text("base".to_string()));
    crate::vector::vector_upsert(&mut loom, ns, "emb", "a", vec![1.0, 0.0], metadata).unwrap();
    loom.commit(ns, "root", "base", 1).unwrap();

    loom.branch(ns, "feature").unwrap();
    let mut ours_meta = BTreeMap::new();
    ours_meta.insert("tag".to_string(), tabular::Value::Text("ours".to_string()));
    crate::vector::vector_upsert(&mut loom, ns, "emb", "a", vec![1.0, 0.0], ours_meta).unwrap();
    loom.commit(ns, "root", "ours", 2).unwrap();

    loom.checkout_branch(ns, "feature").unwrap();
    let mut theirs_meta = BTreeMap::new();
    theirs_meta.insert("tag".to_string(), tabular::Value::Text("base".to_string()));
    crate::vector::vector_upsert(&mut loom, ns, "emb", "a", vec![0.0, 1.0], theirs_meta).unwrap();
    loom.commit(ns, "root", "theirs", 3).unwrap();

    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    assert!(matches!(
        loom.merge(ns, "feature", "root", 4).unwrap(),
        MergeOutcome::Merged(_)
    ));
    let (vector, metadata) = crate::vector::vector_get(&loom, ns, "emb", "a")
        .unwrap()
        .unwrap();
    assert_eq!(vector, vec![0.0, 1.0]);
    assert_eq!(
        metadata.get("tag"),
        Some(&tabular::Value::Text("ours".to_string()))
    );
}

#[test]
fn merge_conflicts_on_vector_entry_same_field_edits() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 14);
    loom.registry_mut()
        .add_facet(ns, FacetKind::Vector)
        .unwrap();

    crate::vector::vector_create(&mut loom, ns, "emb", 2, crate::vector::Metric::Cosine).unwrap();
    let mut metadata = BTreeMap::new();
    metadata.insert("tag".to_string(), tabular::Value::Text("base".to_string()));
    crate::vector::vector_upsert(&mut loom, ns, "emb", "a", vec![1.0, 0.0], metadata).unwrap();
    loom.commit(ns, "root", "base", 1).unwrap();

    loom.branch(ns, "feature").unwrap();
    let mut ours_meta = BTreeMap::new();
    ours_meta.insert("tag".to_string(), tabular::Value::Text("ours".to_string()));
    crate::vector::vector_upsert(&mut loom, ns, "emb", "a", vec![1.0, 0.0], ours_meta).unwrap();
    loom.commit(ns, "root", "ours", 2).unwrap();

    loom.checkout_branch(ns, "feature").unwrap();
    let mut theirs_meta = BTreeMap::new();
    theirs_meta.insert(
        "tag".to_string(),
        tabular::Value::Text("theirs".to_string()),
    );
    crate::vector::vector_upsert(&mut loom, ns, "emb", "a", vec![1.0, 0.0], theirs_meta).unwrap();
    loom.commit(ns, "root", "theirs", 3).unwrap();

    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    let MergeOutcome::Conflicts(conflicts) = loom.merge(ns, "feature", "root", 4).unwrap() else {
        panic!("same metadata key edits must conflict");
    };
    assert_eq!(conflicts.len(), 1);
    assert!(conflicts[0].starts_with(".loom/facets/vector/emb/entries/"));
}

#[test]
fn authenticated_diff_commits_requires_vcs_read() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 22);
    loom.write_file(ns, "readme.txt", b"v1", 0o100644).unwrap();
    let c0 = loom.commit(ns, "root", "base message", 1).unwrap();
    loom.write_file(ns, "readme.txt", b"v2", 0o100644).unwrap();
    let c1 = loom.commit(ns, "root", "next message", 2).unwrap();

    authenticate_root_without_grants(&mut loom);
    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(nid(200)),
            Some(ns),
            Some(FacetKind::Files),
            [crate::AclRight::Read],
        )
        .unwrap();
    assert_eq!(
        loom.diff_commits(ns, c0, c1).unwrap_err().code,
        Code::PermissionDenied
    );

    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(nid(200)),
            Some(ns),
            Some(FacetKind::Vcs),
            [crate::AclRight::Read],
        )
        .unwrap();
    let diff = loom.diff_commits(ns, c0, c1).unwrap();
    assert_eq!(
        cbor::as_text(cbor::decode_array(&diff).unwrap()[0].clone()).unwrap(),
        "LMDIFF"
    );
}

#[test]
fn authenticated_diff_commits_rolls_up_facets_without_read() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 23);
    loom.create_directory(ns, "private", false).unwrap();
    loom.write_file(ns, "private/secret.txt", b"v1", 0o100644)
        .unwrap();
    let c0 = loom.commit(ns, "root", "base message", 1).unwrap();
    loom.write_file(ns, "private/secret.txt", b"v2", 0o100644)
        .unwrap();
    let c1 = loom.commit(ns, "root", "next message", 2).unwrap();

    authenticate_root_without_grants(&mut loom);
    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(nid(200)),
            Some(ns),
            Some(FacetKind::Vcs),
            [crate::AclRight::Read],
        )
        .unwrap();
    let redacted = loom.diff_commits(ns, c0, c1).unwrap();
    assert_eq!(
        collection_summary(&redacted, "files", &[]),
        Some((0, 0, 1, 0, true, 0))
    );
    assert_eq!(collection_summary(&redacted, "files", &["private"]), None);

    loom.acl_store_mut()
        .allow(
            crate::AclSubject::Principal(nid(200)),
            Some(ns),
            Some(FacetKind::Files),
            [crate::AclRight::Read],
        )
        .unwrap();
    let detailed = loom.diff_commits(ns, c0, c1).unwrap();
    assert_eq!(collection_summary(&detailed, "files", &[]), None);
    assert_eq!(
        collection_summary(&detailed, "files", &["private"]),
        Some((0, 0, 1, 0, false, 1))
    );
}

fn collection_summary(
    diff: &[u8],
    wanted_facet: &str,
    wanted_collection: &[&str],
) -> Option<(u64, u64, u64, u64, bool, usize)> {
    let frame = cbor::decode_array(diff).unwrap();
    for facet_section in cbor::as_array(frame[5].clone()).unwrap() {
        let mut facet_fields = cbor::Fields::new(cbor::as_array(facet_section).unwrap());
        let facet = facet_fields.text().unwrap();
        let collections = facet_fields.array().unwrap();
        facet_fields.end().unwrap();
        if facet != wanted_facet {
            continue;
        }
        for collection_section in collections {
            let mut collection_fields =
                cbor::Fields::new(cbor::as_array(collection_section).unwrap());
            let path = collection_fields
                .array()
                .unwrap()
                .into_iter()
                .map(|item| cbor::as_text(item).unwrap())
                .collect::<Vec<_>>();
            let summary = collection_fields.array().unwrap();
            let units = collection_fields.array().unwrap();
            collection_fields.end().unwrap();
            if path != wanted_collection {
                continue;
            }
            let mut fields = cbor::Fields::new(summary);
            let out = (
                fields.uint().unwrap(),
                fields.uint().unwrap(),
                fields.uint().unwrap(),
                fields.uint().unwrap(),
                fields.bool().unwrap(),
                units.len(),
            );
            fields.end().unwrap();
            return Some(out);
        }
    }
    None
}

fn collection_units(
    diff: &[u8],
    wanted_facet: &str,
    wanted_collection: &[&str],
) -> Vec<(String, Vec<u8>, String, bool, bool)> {
    let frame = cbor::decode_array(diff).unwrap();
    for facet_section in cbor::as_array(frame[5].clone()).unwrap() {
        let mut facet_fields = cbor::Fields::new(cbor::as_array(facet_section).unwrap());
        let facet = facet_fields.text().unwrap();
        let collections = facet_fields.array().unwrap();
        facet_fields.end().unwrap();
        if facet != wanted_facet {
            continue;
        }
        for collection_section in collections {
            let mut collection_fields =
                cbor::Fields::new(cbor::as_array(collection_section).unwrap());
            let path = collection_fields
                .array()
                .unwrap()
                .into_iter()
                .map(|item| cbor::as_text(item).unwrap())
                .collect::<Vec<_>>();
            let _summary = collection_fields.array().unwrap();
            let units = collection_fields.array().unwrap();
            collection_fields.end().unwrap();
            if path != wanted_collection {
                continue;
            }
            return units
                .into_iter()
                .map(|unit| {
                    let mut fields = cbor::Fields::new(cbor::as_array(unit).unwrap());
                    let unit_kind = fields.text().unwrap();
                    let unit_key = fields.bytes().unwrap();
                    let change = fields.text().unwrap();
                    let before = !matches!(fields.next_field().unwrap(), cbor::Value::Null);
                    let after = !matches!(fields.next_field().unwrap(), cbor::Value::Null);
                    let detail_kind = fields.text().unwrap();
                    let detail = fields.next_field().unwrap();
                    fields.end().unwrap();
                    assert_eq!(detail_kind, "none");
                    assert!(matches!(detail, cbor::Value::Null));
                    (unit_kind, unit_key, change, before, after)
                })
                .collect();
        }
    }
    Vec::new()
}

#[test]
fn blame_attributes_each_path_to_its_last_change() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"a0", 0o100644).unwrap();
    loom.write_file(ns, "b.txt", b"b0", 0o100644).unwrap();
    let c0 = loom.commit(ns, "nas", "c0", 1).unwrap();
    loom.write_file(ns, "b.txt", b"b1", 0o100644).unwrap(); // modify b
    loom.write_file(ns, "c.txt", b"c1", 0o100644).unwrap(); // add c
    let c1 = loom.commit(ns, "nas", "c1", 2).unwrap();

    let blame = loom.blame(ns, DEFAULT_BRANCH).unwrap();
    assert_eq!(
        blame,
        vec![
            ("a.txt".to_string(), c0), // untouched since the first commit
            ("b.txt".to_string(), c1), // last modified at c1
            ("c.txt".to_string(), c1), // added at c1
        ]
    );
}

#[test]
fn blame_on_empty_branch_is_empty() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    assert!(loom.blame(ns, DEFAULT_BRANCH).unwrap().is_empty());
}

#[test]
fn fast_forward_merge() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    let _c0 = loom.commit(ns, "nas", "c0", 1).unwrap();
    loom.branch(ns, "feature").unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
    let c1 = loom.commit(ns, "nas", "c1", 2).unwrap();
    // main has no new commits, so merging feature fast-forwards.
    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    assert_eq!(
        loom.merge(ns, "feature", "nas", 3).unwrap(),
        MergeOutcome::FastForward(c1)
    );
    assert_eq!(
        loom.registry().branch_tip(ns, DEFAULT_BRANCH).unwrap(),
        Some(c1)
    );
}

#[test]
fn three_way_merge_auto_resolves_disjoint_edits() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.branch(ns, "feature").unwrap();
    // feature adds b.txt
    loom.checkout_branch(ns, "feature").unwrap();
    loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
    loom.commit(ns, "nas", "feat", 2).unwrap();
    // main adds c.txt
    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    loom.write_file(ns, "c.txt", b"c", 0o100644).unwrap();
    loom.commit(ns, "nas", "main work", 3).unwrap();

    let outcome = loom.merge(ns, "feature", "nas", 4).unwrap();
    assert!(matches!(outcome, MergeOutcome::Merged(_)));
    // The merged working tree has all three files.
    let mut paths = loom.staged_paths(ns);
    paths.sort();
    assert_eq!(paths, vec!["a.txt", "b.txt", "c.txt"]);
    // The merge commit has two parents.
    let MergeOutcome::Merged(m) = outcome else {
        unreachable!()
    };
    assert_eq!(loom.get_commit(m).unwrap().parents.len(), 2);
}

#[test]
fn three_way_merge_reports_conflicts() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"base", 0o100644).unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.branch(ns, "feature").unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    loom.write_file(ns, "a.txt", b"theirs", 0o100644).unwrap();
    loom.commit(ns, "nas", "feat", 2).unwrap();
    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    loom.write_file(ns, "a.txt", b"ours", 0o100644).unwrap();
    let ours = loom.commit(ns, "nas", "main", 3).unwrap();

    assert_eq!(
        loom.merge(ns, "feature", "nas", 4).unwrap(),
        MergeOutcome::Conflicts(vec!["a.txt".to_string()])
    );
    // The branch tip is unchanged after a conflicted merge.
    assert_eq!(
        loom.registry().branch_tip(ns, DEFAULT_BRANCH).unwrap(),
        Some(ours)
    );
    // A conflicted merge enters an in-progress state with the path recorded, and presents the
    // text conflict as whole-file markers in the working tree.
    assert!(loom.merge_in_progress(ns).unwrap());
    assert_eq!(loom.merge_conflicts(ns).unwrap(), vec!["a.txt".to_string()]);
    let marked = String::from_utf8(loom.read_file(ns, "a.txt").unwrap()).unwrap();
    assert!(marked.contains("<<<<<<< ours") && marked.contains("ours"));
    assert!(marked.contains("=======") && marked.contains("theirs"));
}

/// Build a single-file modify/modify conflict on the default branch and return `(loom, ns, ours)`.
fn conflict_scenario() -> (Loom<MemoryStore>, WorkspaceId, Digest) {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"base", 0o100644).unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.branch(ns, "feature").unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    loom.write_file(ns, "a.txt", b"theirs", 0o100644).unwrap();
    loom.commit(ns, "nas", "feat", 2).unwrap();
    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    loom.write_file(ns, "a.txt", b"ours", 0o100644).unwrap();
    let ours = loom.commit(ns, "nas", "main", 3).unwrap();
    (loom, ns, ours)
}

#[test]
fn merge_abort_restores_pre_merge_working_tree() {
    let (mut loom, ns, ours) = conflict_scenario();
    assert!(matches!(
        loom.merge(ns, "feature", "nas", 4).unwrap(),
        MergeOutcome::Conflicts(_)
    ));
    loom.merge_abort(ns).unwrap();
    assert!(!loom.merge_in_progress(ns).unwrap());
    // The working tree is restored to the pre-merge `ours` content, not markers.
    assert_eq!(loom.read_file(ns, "a.txt").unwrap(), b"ours");
    assert_eq!(
        loom.registry().branch_tip(ns, DEFAULT_BRANCH).unwrap(),
        Some(ours)
    );
    // Abort with no merge in progress is rejected.
    assert_eq!(
        loom.merge_abort(ns).unwrap_err().code,
        Code::InvalidArgument
    );
}

#[test]
fn merge_resolve_theirs_then_continue_makes_two_parent_commit() {
    let (mut loom, ns, ours) = conflict_scenario();
    loom.merge(ns, "feature", "nas", 4).unwrap();
    // Cannot continue while a conflict is unresolved.
    assert_eq!(
        loom.merge_continue(ns, "nas", 5).unwrap_err().code,
        Code::Conflict
    );
    loom.merge_resolve(ns, "a.txt", ConflictResolution::Theirs)
        .unwrap();
    assert!(loom.merge_conflicts(ns).unwrap().is_empty());
    let m = loom.merge_continue(ns, "nas", 5).unwrap();
    assert!(!loom.merge_in_progress(ns).unwrap());
    assert_eq!(loom.read_file(ns, "a.txt").unwrap(), b"theirs");
    let commit = loom.get_commit(m).unwrap();
    assert_eq!(
        commit.parents,
        vec![ours, {
            loom.registry().branch_tip(ns, "feature").unwrap().unwrap()
        }]
    );
    assert_eq!(
        loom.registry().branch_tip(ns, DEFAULT_BRANCH).unwrap(),
        Some(m)
    );
}

#[test]
fn merge_resolve_working_accepts_hand_merged_content() {
    let (mut loom, ns, _ours) = conflict_scenario();
    loom.merge(ns, "feature", "nas", 4).unwrap();
    loom.write_file(ns, "a.txt", b"hand-merged", 0o100644)
        .unwrap();
    loom.merge_resolve(ns, "a.txt", ConflictResolution::Working)
        .unwrap();
    loom.merge_continue(ns, "nas", 5).unwrap();
    assert_eq!(loom.read_file(ns, "a.txt").unwrap(), b"hand-merged");
}

#[test]
fn second_merge_while_in_progress_is_rejected() {
    let (mut loom, ns, _ours) = conflict_scenario();
    loom.merge(ns, "feature", "nas", 4).unwrap();
    assert_eq!(
        loom.merge(ns, "feature", "nas", 5).unwrap_err().code,
        Code::Conflict
    );
}

#[test]
fn merge_state_survives_export_import_round_trip() {
    let (mut loom, ns, _ours) = conflict_scenario();
    loom.merge(ns, "feature", "nas", 4).unwrap();
    let bytes = loom.export_state();

    // Reopen over the same object store (markers and slots are content-addressed objects) and
    // restore the in-progress merge from the persisted engine state.
    let store = loom.into_store();
    let mut reopened = Loom::new(store);
    reopened.import_state(&bytes).unwrap();
    assert!(reopened.merge_in_progress(ns).unwrap());
    assert_eq!(
        reopened.merge_conflicts(ns).unwrap(),
        vec!["a.txt".to_string()]
    );
    reopened
        .merge_resolve(ns, "a.txt", ConflictResolution::Theirs)
        .unwrap();
    reopened.merge_continue(ns, "nas", 6).unwrap();
    assert_eq!(reopened.read_file(ns, "a.txt").unwrap(), b"theirs");
}

#[test]
fn commit_staged_records_only_the_index() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
    loom.stage(ns, &["a.txt"]).unwrap();
    let c = loom.commit_staged(ns, "nas", "stage a", 1).unwrap();
    let (head, _) = loom.flatten_commit(c).unwrap();
    assert!(head.contains_key("a.txt") && !head.contains_key("b.txt"));
    // The index now matches HEAD, and b.txt remains an untracked working-tree change.
    let st = loom.status(ns).unwrap();
    assert!(st.staged.is_empty());
    assert_eq!(st.untracked, vec!["b.txt".to_string()]);
}

#[test]
fn commit_everything_includes_unstaged_changes() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
    loom.stage(ns, &["a.txt"]).unwrap(); // only a staged
    let c = loom.commit(ns, "nas", "all", 1).unwrap(); // commits everything
    let (head, _) = loom.flatten_commit(c).unwrap();
    assert!(head.contains_key("a.txt") && head.contains_key("b.txt"));
    let st = loom.status(ns).unwrap();
    assert!(st.staged.is_empty() && st.unstaged.is_empty() && st.untracked.is_empty());
}

#[test]
fn status_classifies_staged_unstaged_untracked() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "base.txt", b"v1", 0o100644).unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.write_file(ns, "base.txt", b"v2", 0o100644).unwrap();
    loom.write_file(ns, "new.txt", b"n", 0o100644).unwrap();
    loom.stage(ns, &["base.txt"]).unwrap();
    let st = loom.status(ns).unwrap();
    assert_eq!(
        st.staged
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        vec!["base.txt"]
    );
    assert_eq!(st.untracked, vec!["new.txt".to_string()]);
    assert!(st.unstaged.is_empty());
}

#[test]
fn unstage_resets_index_to_head() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"v1", 0o100644).unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.write_file(ns, "a.txt", b"v2", 0o100644).unwrap();
    loom.stage(ns, &["a.txt"]).unwrap();
    assert_eq!(loom.status(ns).unwrap().staged.len(), 1);
    loom.unstage(ns, &["a.txt"]).unwrap();
    let st = loom.status(ns).unwrap();
    assert!(st.staged.is_empty());
    assert_eq!(
        st.unstaged
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        vec!["a.txt"]
    );
}

#[test]
fn staging_index_survives_export_import() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"v1", 0o100644).unwrap();
    loom.commit(ns, "nas", "base", 1).unwrap();
    loom.write_file(ns, "a.txt", b"v2", 0o100644).unwrap();
    loom.stage(ns, &["a.txt"]).unwrap();
    let bytes = loom.export_state();
    let store = loom.into_store();
    let mut re = Loom::new(store);
    re.import_state(&bytes).unwrap();
    assert_eq!(re.status(ns).unwrap().staged.len(), 1);
    let c = re.commit_staged(ns, "nas", "staged v2", 2).unwrap();
    let (head, _) = re.flatten_commit(c).unwrap();
    assert!(head.contains_key("a.txt"));
}

#[test]
fn append_file_creates_then_concatenates() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    // Append to a missing file creates it (like `>>`).
    loom.append_file(ns, "log.txt", b"a").unwrap();
    assert_eq!(loom.read_file(ns, "log.txt").unwrap(), b"a");
    // A second append concatenates.
    loom.append_file(ns, "log.txt", b"b").unwrap();
    assert_eq!(loom.read_file(ns, "log.txt").unwrap(), b"ab");
}

#[test]
fn append_file_requires_existing_parent_and_rejects_a_directory() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    // Missing parent directory -> NOT_FOUND (Mac/Linux: append into a nonexistent folder fails).
    assert_eq!(
        loom.append_file(ns, "missing/log.txt", b"x")
            .unwrap_err()
            .code,
        Code::NotFound
    );
    // Appending to a directory path -> ALREADY_EXISTS.
    loom.create_directory(ns, "d", false).unwrap();
    assert_eq!(
        loom.append_file(ns, "d", b"x").unwrap_err().code,
        Code::AlreadyExists
    );
}

#[test]
fn write_at_and_read_at_follow_posix_byte_semantics() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    // write_at on a missing file creates it, zero-filling the gap before the offset.
    loom.write_at(ns, "f", 5, b"XY").unwrap();
    assert_eq!(
        loom.read_file(ns, "f").unwrap(),
        vec![0, 0, 0, 0, 0, b'X', b'Y']
    );
    // read_at past the end clamps to the available bytes, and is empty beyond the end.
    assert_eq!(loom.read_at(ns, "f", 6, 100).unwrap(), b"Y");
    assert_eq!(loom.read_at(ns, "f", 50, 10).unwrap(), b"");
    // An in-place overwrite touches only the addressed range.
    loom.write_at(ns, "f", 0, b"AB").unwrap();
    assert_eq!(
        loom.read_file(ns, "f").unwrap(),
        vec![b'A', b'B', 0, 0, 0, b'X', b'Y']
    );
    // A directory parent must exist, and a directory path is rejected.
    assert_eq!(
        loom.write_at(ns, "nope/g", 0, b"x").unwrap_err().code,
        Code::NotFound
    );
    loom.create_directory(ns, "d", false).unwrap();
    assert_eq!(
        loom.write_at(ns, "d", 0, b"x").unwrap_err().code,
        Code::AlreadyExists
    );
    // read_at on a missing file is NOT_FOUND.
    assert_eq!(
        loom.read_at(ns, "absent", 0, 1).unwrap_err().code,
        Code::NotFound
    );
}

#[test]
fn truncate_grows_with_zeros_and_shrinks() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "f", b"hello world", 0o100644).unwrap();
    loom.truncate_file(ns, "f", 5).unwrap();
    assert_eq!(loom.read_file(ns, "f").unwrap(), b"hello");
    loom.truncate_file(ns, "f", 8).unwrap();
    assert_eq!(
        loom.read_file(ns, "f").unwrap(),
        vec![b'h', b'e', b'l', b'l', b'o', 0, 0, 0]
    );
    // truncate on a missing file creates it zero-filled.
    loom.truncate_file(ns, "g", 3).unwrap();
    assert_eq!(loom.read_file(ns, "g").unwrap(), vec![0, 0, 0]);
}

#[test]
fn streamed_edit_converges_on_the_whole_rewrite_bytes_and_address() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    // ~2 MiB so the file is chunked; the edit must stream, not reassemble.
    let data: Vec<u8> = (0..2_000_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8)
        .collect();
    loom.write_file(ns, "big.bin", &data, 0o100644).unwrap();
    let patch = vec![0xABu8; 1000];
    loom.write_at(ns, "big.bin", 1_000_000, &patch).unwrap();
    let mut expected = data.clone();
    expected[1_000_000..1_001_000].copy_from_slice(&patch);
    // Byte-identical full and streamed sub-range reads.
    assert_eq!(loom.read_file(ns, "big.bin").unwrap(), expected);
    assert_eq!(
        loom.read_at(ns, "big.bin", 999_500, 1000).unwrap(),
        expected[999_500..1_000_500]
    );
    // The edited file's content address equals storing the same final bytes wholesale: streaming
    // the edit converges on the identical content object, so unchanged chunks dedup across versions.
    let direct = loom.store_content(ns, &expected).unwrap();
    let StagedEntry::File(f) = loom.work.get(&ns).unwrap().get("big.bin").unwrap() else {
        panic!("expected a file slot");
    };
    assert_eq!(f.content_addr, direct);
}

#[test]
fn two_handles_share_one_inode_truncate_then_write_at() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"hello world", 0o100644)
        .unwrap();
    let h1 = loom.file_open(ns, "a.txt", OpenMode::ReadWrite).unwrap();
    let h2 = loom.file_open(ns, "a.txt", OpenMode::ReadWrite).unwrap();
    // One handle truncates; the other writes past the new end. They share the inode, so the
    // result is 5 zeros then X (POSIX), visible to both handles and to the path.
    loom.file_truncate(h1, 0).unwrap();
    loom.file_write_at(h2, 5, b"X").unwrap();
    let want = vec![0, 0, 0, 0, 0, b'X'];
    assert_eq!(loom.file_read_at(h1, 0, 100).unwrap(), want);
    assert_eq!(loom.read_file(ns, "a.txt").unwrap(), want);
    loom.file_close(h1).unwrap();
    loom.file_close(h2).unwrap();
}

#[test]
fn delete_on_last_close_does_not_resurrect_the_path() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"hello world", 0o100644)
        .unwrap();
    let h1 = loom.file_open(ns, "a.txt", OpenMode::ReadWrite).unwrap();
    let h2 = loom.file_open(ns, "a.txt", OpenMode::ReadWrite).unwrap();
    loom.remove_file(ns, "a.txt").unwrap();
    // The path is gone immediately.
    assert_eq!(
        loom.read_file(ns, "a.txt").unwrap_err().code,
        Code::NotFound
    );
    // A write through a surviving handle hits the detached inode, not the path: no resurrection.
    loom.file_write_at(h2, 5, b"X").unwrap();
    assert_eq!(
        loom.read_file(ns, "a.txt").unwrap_err().code,
        Code::NotFound
    );
    // Both handles still read the shared detached inode.
    assert_eq!(loom.file_read_at(h1, 0, 100).unwrap(), b"helloXworld");
    loom.file_close(h1).unwrap();
    loom.file_close(h2).unwrap();
    // After the last close the inode is gone and the path is still absent.
    assert_eq!(
        loom.read_file(ns, "a.txt").unwrap_err().code,
        Code::NotFound
    );
    assert!(loom.inodes.is_empty());
    assert!(loom.handles.is_empty());
}

#[test]
fn whole_file_replace_while_open_is_seen_by_handles() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"old", 0o100644).unwrap();
    let h = loom.file_open(ns, "a.txt", OpenMode::ReadWrite).unwrap();
    // A whole-file write on the open path is O_TRUNC on the same inode: the handle sees it.
    loom.write_file(ns, "a.txt", b"brand new content", 0o100644)
        .unwrap();
    assert_eq!(loom.file_read_at(h, 0, 100).unwrap(), b"brand new content");
    loom.file_close(h).unwrap();
}

#[test]
fn open_modes_enforce_posix_rules() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    // Read on a missing file is NOT_FOUND.
    assert_eq!(
        loom.file_open(ns, "x", OpenMode::Read).unwrap_err().code,
        Code::NotFound
    );
    // Write creates and truncates; reading a write-only handle is rejected.
    let w = loom.file_open(ns, "x", OpenMode::Write).unwrap();
    loom.file_write(w, b"abc").unwrap();
    assert_eq!(
        loom.file_read(w, 10).unwrap_err().code,
        Code::InvalidArgument
    );
    loom.file_close(w).unwrap();
    // Append always writes at the current end.
    let a = loom.file_open(ns, "x", OpenMode::Append).unwrap();
    loom.file_write(a, b"de").unwrap();
    loom.file_close(a).unwrap();
    assert_eq!(loom.read_file(ns, "x").unwrap(), b"abcde");
    // Writing a read-only handle is rejected; sequential reads advance the cursor.
    loom.write_file(ns, "y", b"hi", 0o100644).unwrap();
    let r = loom.file_open(ns, "y", OpenMode::Read).unwrap();
    assert_eq!(
        loom.file_write(r, b"x").unwrap_err().code,
        Code::InvalidArgument
    );
    assert_eq!(loom.file_read(r, 1).unwrap(), b"h");
    assert_eq!(loom.file_read(r, 10).unwrap(), b"i");
    assert_eq!(loom.file_read(r, 10).unwrap(), b"");
    loom.file_close(r).unwrap();
}

#[test]
fn open_handles_survive_export_import() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"hello", 0o100644).unwrap();
    let h = loom.file_open(ns, "a.txt", OpenMode::ReadWrite).unwrap();
    loom.file_write_at(h, 5, b" world").unwrap();
    // Advance the read cursor, then round-trip the engine state (same store keeps the objects).
    assert_eq!(loom.file_read(h, 5).unwrap(), b"hello");
    let bytes = loom.export_state();
    loom.import_state(&bytes).unwrap();
    // The handle id is still valid and its cursor survived.
    assert_eq!(loom.file_read(h, 100).unwrap(), b" world");
    assert_eq!(loom.file_stat(h).unwrap().size, 11);
    loom.file_close(h).unwrap();
}

#[test]
fn lightweight_and_annotated_tags_resolve_and_persist() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a", b"x", 0o100644).unwrap();
    let c0 = loom.commit(ns, "nas", "init", 1).unwrap();
    // A lightweight tag at HEAD points straight at the commit.
    let t1 = loom.tag_create(ns, "v1", "HEAD", "", "", 0).unwrap();
    assert_eq!(t1, c0);
    assert_eq!(loom.tag_target(ns, "v1").unwrap(), Some(c0));
    // An annotated tag (non-empty message) points at a stored Tag object that carries the metadata.
    let t2 = loom
        .tag_create(ns, "v1-ann", &c0.to_string(), "nas", "release 1", 5)
        .unwrap();
    assert_ne!(
        t2, c0,
        "an annotated ref points at the tag object, not the commit"
    );
    match loom.get_object(&t2).unwrap() {
        Object::Tag(t) => {
            assert_eq!(t.target, c0);
            assert_eq!(t.tagger, "nas");
            assert_eq!(t.message, "release 1");
            assert_eq!(t.target_type, ObjectType::Commit);
        }
        other => panic!("expected a tag object, got {:?}", other.object_type()),
    }
    assert_eq!(
        loom.tag_list(ns).unwrap(),
        vec!["v1".to_string(), "v1-ann".to_string()]
    );
    // Both the tag object and its target commit are GC-retained (so sync/clone carry the annotation).
    let live = loom.live_object_set(None).unwrap();
    assert!(live.contains(&t2) && live.contains(&c0));
    // Tags survive an engine-state round trip (they live in the persisted registry).
    let bytes = loom.export_state();
    loom.import_state(&bytes).unwrap();
    assert_eq!(loom.tag_target(ns, "v1-ann").unwrap(), Some(t2));
}

#[test]
fn tag_rev_resolution_rules() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a", b"x", 0o100644).unwrap();
    let c0 = loom.commit(ns, "nas", "init", 1).unwrap();
    loom.branch(ns, "feature").unwrap();
    // A branch name resolves to its tip.
    assert_eq!(
        loom.tag_create(ns, "byb", "feature", "", "", 0).unwrap(),
        c0
    );
    assert_eq!(loom.resolve_rev(ns, &format!("commit:{c0}")).unwrap(), c0);
    assert_eq!(loom.resolve_rev(ns, "branch:feature").unwrap(), c0);
    loom.tag_create(ns, "v1", "HEAD", "", "", 0).unwrap();
    loom.tag_create(ns, "v1-ann", "HEAD", "nas", "release", 2)
        .unwrap();
    assert_eq!(loom.resolve_rev(ns, "tag:v1").unwrap(), c0);
    assert_eq!(loom.resolve_rev(ns, "tag:v1-ann").unwrap(), c0);
    // An unresolvable revision is NOT_FOUND.
    assert_eq!(
        loom.tag_create(ns, "bad", "nope", "", "", 0)
            .unwrap_err()
            .code,
        Code::NotFound
    );
    // A digest that resolves to a non-commit (the commit's tree) is INVALID_ARGUMENT.
    let tree = loom.get_commit(c0).unwrap().tree;
    assert_eq!(
        loom.tag_create(ns, "bad2", &tree.to_string(), "", "", 0)
            .unwrap_err()
            .code,
        Code::InvalidArgument
    );
}

#[test]
fn tag_delete_and_rename_semantics() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a", b"x", 0o100644).unwrap();
    let c0 = loom.commit(ns, "nas", "init", 1).unwrap();
    loom.tag_create(ns, "v1", "HEAD", "", "", 0).unwrap();
    // A duplicate name is ALREADY_EXISTS.
    assert_eq!(
        loom.tag_create(ns, "v1", "HEAD", "", "", 0)
            .unwrap_err()
            .code,
        Code::AlreadyExists
    );
    // Rename preserves the target.
    loom.tag_rename(ns, "v1", "v2").unwrap();
    assert_eq!(loom.tag_target(ns, "v2").unwrap(), Some(c0));
    assert!(loom.tag_target(ns, "v1").unwrap().is_none());
    // Rename of a missing tag is NOT_FOUND; onto an existing name is ALREADY_EXISTS.
    assert_eq!(
        loom.tag_rename(ns, "nope", "x").unwrap_err().code,
        Code::NotFound
    );
    loom.tag_create(ns, "v3", "HEAD", "", "", 0).unwrap();
    assert_eq!(
        loom.tag_rename(ns, "v2", "v3").unwrap_err().code,
        Code::AlreadyExists
    );
    // Delete removes it; deleting again is NOT_FOUND.
    loom.tag_delete(ns, "v2").unwrap();
    assert!(loom.tag_target(ns, "v2").unwrap().is_none());
    assert_eq!(loom.tag_delete(ns, "v2").unwrap_err().code, Code::NotFound);
}

#[test]
fn symlink_create_read_and_stat() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.create_directory(ns, "dir", false).unwrap();
    loom.write_file(ns, "dir/real.txt", b"hello", 0o100644)
        .unwrap();
    // A dangling target is allowed (the target need not exist).
    loom.symlink(ns, "dir/real.txt", "link").unwrap();
    loom.symlink(ns, "nowhere", "dangling").unwrap();
    assert_eq!(loom.read_link(ns, "link").unwrap(), "dir/real.txt");
    assert_eq!(loom.read_link(ns, "dangling").unwrap(), "nowhere");
    // stat reports a symlink kind and the symlink mode; the size is the target length.
    let st = loom.stat(ns, "link").unwrap();
    assert_eq!(st.kind, crate::fs::FileKind::Symlink);
    assert!(is_symlink_mode(st.mode));
    assert_eq!(st.size, "dir/real.txt".len() as u64);
    // read_link on a regular file is INVALID_ARGUMENT; on a missing path NOT_FOUND.
    assert_eq!(
        loom.read_link(ns, "dir/real.txt").unwrap_err().code,
        Code::InvalidArgument
    );
    assert_eq!(
        loom.read_link(ns, "absent").unwrap_err().code,
        Code::NotFound
    );
    // symlink over an existing path is ALREADY_EXISTS; a missing parent is NOT_FOUND.
    assert_eq!(
        loom.symlink(ns, "x", "link").unwrap_err().code,
        Code::AlreadyExists
    );
    assert_eq!(
        loom.symlink(ns, "x", "nope/l").unwrap_err().code,
        Code::NotFound
    );
}

#[test]
fn symlink_round_trips_through_commit_and_checkout() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.symlink(ns, "target/path", "link").unwrap();
    let c0 = loom.commit(ns, "nas", "add link", 1).unwrap();
    // Replace the working tree, then check the link back out: the symlink mode survives the Tree.
    loom.write_file(ns, "link2", b"x", 0o100644).unwrap();
    loom.remove_file(ns, "link").unwrap();
    loom.checkout_commit(ns, c0).unwrap();
    assert_eq!(loom.read_link(ns, "link").unwrap(), "target/path");
    assert_eq!(
        loom.stat(ns, "link").unwrap().kind,
        crate::fs::FileKind::Symlink
    );
}

#[test]
fn restore_file_restores_a_path_and_removes_when_absent() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"v1", 0o100644).unwrap();
    loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
    let c0 = loom.commit(ns, "nas", "init", 1).unwrap();
    // Working-tree edits: change a.txt and add an untracked c.txt.
    loom.write_file(ns, "a.txt", b"v2", 0o100644).unwrap();
    loom.write_file(ns, "c.txt", b"c", 0o100644).unwrap();
    // Restore a.txt from HEAD; it reverts to the committed content.
    loom.restore_file(ns, "HEAD", "a.txt").unwrap();
    assert_eq!(loom.read_file(ns, "a.txt").unwrap(), b"v1");
    // Restore c.txt, which is absent in the snapshot: it is removed from the working tree.
    loom.restore_file(ns, "HEAD", "c.txt").unwrap();
    assert_eq!(
        loom.read_file(ns, "c.txt").unwrap_err().code,
        Code::NotFound
    );
    // HEAD is untouched (no new commit).
    assert_eq!(loom.log(ns, DEFAULT_BRANCH).unwrap(), vec![c0]);
}

#[test]
fn restore_path_restores_a_subtree_only() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.create_directory(ns, "src", false).unwrap();
    loom.write_file(ns, "src/x.rs", b"x", 0o100644).unwrap();
    loom.write_file(ns, "src/y.rs", b"y", 0o100644).unwrap();
    loom.write_file(ns, "top.txt", b"t1", 0o100644).unwrap();
    loom.commit(ns, "nas", "init", 1).unwrap();
    // Working-tree edits inside and outside the subtree.
    loom.remove_file(ns, "src/x.rs").unwrap();
    loom.write_file(ns, "src/z.rs", b"z", 0o100644).unwrap();
    loom.write_file(ns, "top.txt", b"t2", 0o100644).unwrap();
    // Restore only the src subtree from HEAD.
    loom.restore_path(ns, "HEAD", "src").unwrap();
    assert_eq!(loom.read_file(ns, "src/x.rs").unwrap(), b"x", "restored");
    assert_eq!(loom.read_file(ns, "src/y.rs").unwrap(), b"y");
    assert_eq!(
        loom.read_file(ns, "src/z.rs").unwrap_err().code,
        Code::NotFound,
        "a path absent in the snapshot is removed"
    );
    // A path outside the prefix is untouched.
    assert_eq!(loom.read_file(ns, "top.txt").unwrap(), b"t2");
}

#[test]
fn cherry_pick_applies_a_commit_onto_head() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"base", 0o100644).unwrap();
    let c0 = loom.commit(ns, "nas", "init", 1).unwrap();
    // A feature branch that adds b.txt on top of c0.
    loom.branch(ns, "feature").unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    loom.write_file(ns, "b.txt", b"feature", 0o100644).unwrap();
    let cf = loom.commit(ns, "dev", "add b", 2).unwrap();
    // Back on the default branch, cherry-pick the feature commit.
    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    assert_eq!(
        loom.read_file(ns, "b.txt").unwrap_err().code,
        Code::NotFound
    );
    let out = loom.cherry_pick(ns, &[cf], 3, false).unwrap();
    let tip = match out {
        ReplayOutcome::Replayed(d) => d,
        other => panic!("expected Replayed, got {other:?}"),
    };
    // The picked change is present, the base file is intact, and the new commit preserves the author.
    assert_eq!(loom.read_file(ns, "b.txt").unwrap(), b"feature");
    assert_eq!(loom.read_file(ns, "a.txt").unwrap(), b"base");
    assert_eq!(
        loom.get_commit(tip).unwrap().author,
        "dev",
        "cherry-pick keeps the author"
    );
    assert_eq!(loom.log(ns, DEFAULT_BRANCH).unwrap(), vec![tip, c0]);
}

#[test]
fn revert_undoes_a_commit() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"v1", 0o100644).unwrap();
    loom.commit(ns, "nas", "init", 1).unwrap();
    loom.write_file(ns, "a.txt", b"v2", 0o100644).unwrap();
    let c1 = loom.commit(ns, "nas", "bump", 2).unwrap();
    // Reverting c1 restores a.txt to v1 in a new commit.
    let out = loom.revert(ns, &[c1], "nas", 3, false).unwrap();
    assert!(matches!(out, ReplayOutcome::Replayed(_)));
    assert_eq!(loom.read_file(ns, "a.txt").unwrap(), b"v1");
}

#[test]
fn cherry_pick_dry_run_reports_conflicts_without_changing_anything() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"base", 0o100644).unwrap();
    let c0 = loom.commit(ns, "nas", "init", 1).unwrap();
    // Feature edits a.txt one way.
    loom.branch(ns, "feature").unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    loom.write_file(ns, "a.txt", b"feature", 0o100644).unwrap();
    let cf = loom.commit(ns, "dev", "edit a", 2).unwrap();
    // The default branch edits a.txt a conflicting way.
    loom.checkout_branch(ns, DEFAULT_BRANCH).unwrap();
    loom.write_file(ns, "a.txt", b"mainline", 0o100644).unwrap();
    let c1 = loom.commit(ns, "nas", "edit a too", 3).unwrap();
    // A dry-run cherry-pick reports the conflict and changes nothing.
    match loom.cherry_pick(ns, &[cf], 4, true).unwrap() {
        ReplayOutcome::Conflicts(paths) => assert_eq!(paths, vec!["a.txt".to_string()]),
        other => panic!("expected Conflicts, got {other:?}"),
    }
    // A real cherry-pick is atomic: same conflict report, branch tip unchanged, working tree intact.
    match loom.cherry_pick(ns, &[cf], 4, false).unwrap() {
        ReplayOutcome::Conflicts(paths) => assert_eq!(paths, vec!["a.txt".to_string()]),
        other => panic!("expected Conflicts, got {other:?}"),
    }
    assert_eq!(loom.log(ns, DEFAULT_BRANCH).unwrap(), vec![c1, c0]);
    assert_eq!(loom.read_file(ns, "a.txt").unwrap(), b"mainline");
}

#[test]
fn rebase_replays_commits_onto_a_target() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    let c0 = loom.commit(ns, "nas", "init", 1).unwrap();
    // Default branch advances with an independent file.
    loom.write_file(ns, "main.txt", b"m", 0o100644).unwrap();
    let cmain = loom.commit(ns, "nas", "main work", 2).unwrap();
    // A feature branch from c0 adds its own file.
    loom.registry_mut()
        .branch_create(ns, "feature", c0)
        .unwrap();
    loom.checkout_branch(ns, "feature").unwrap();
    loom.write_file(ns, "feat.txt", b"f", 0o100644).unwrap();
    loom.commit(ns, "dev", "feat work", 3).unwrap();
    // Rebase feature onto the default branch tip: both files end up present, linearly atop cmain.
    let out = loom.rebase(ns, &cmain.to_string(), 4, false).unwrap();
    let tip = match out {
        ReplayOutcome::Replayed(d) => d,
        other => panic!("expected Replayed, got {other:?}"),
    };
    assert_eq!(loom.read_file(ns, "feat.txt").unwrap(), b"f");
    assert_eq!(loom.read_file(ns, "main.txt").unwrap(), b"m");
    assert_eq!(loom.read_file(ns, "a.txt").unwrap(), b"a");
    // The rebased commit sits directly on cmain.
    assert_eq!(loom.get_commit(tip).unwrap().parents, vec![cmain]);
}

#[test]
fn squash_collapses_commits_into_one() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);
    loom.write_file(ns, "a.txt", b"a", 0o100644).unwrap();
    let c0 = loom.commit(ns, "nas", "init", 1).unwrap();
    loom.write_file(ns, "b.txt", b"b", 0o100644).unwrap();
    loom.commit(ns, "nas", "add b", 2).unwrap();
    loom.write_file(ns, "c.txt", b"c", 0o100644).unwrap();
    loom.commit(ns, "nas", "add c", 3).unwrap();
    // Squash everything after c0 into one commit parented on c0.
    let sq = loom
        .squash(ns, &c0.to_string(), "nas", "squashed", 4)
        .unwrap();
    assert_eq!(loom.get_commit(sq).unwrap().parents, vec![c0]);
    assert_eq!(loom.get_commit(sq).unwrap().message, "squashed");
    // The collapsed history is just [squashed, c0]; all files are still present.
    assert_eq!(loom.log(ns, DEFAULT_BRANCH).unwrap(), vec![sq, c0]);
    assert_eq!(loom.read_file(ns, "a.txt").unwrap(), b"a");
    assert_eq!(loom.read_file(ns, "b.txt").unwrap(), b"b");
    assert_eq!(loom.read_file(ns, "c.txt").unwrap(), b"c");
    // Squashing onto the tip, or onto a non-ancestor, is rejected.
    assert_eq!(
        loom.squash(ns, &sq.to_string(), "nas", "x", 5)
            .unwrap_err()
            .code,
        Code::InvalidArgument
    );
}

#[test]
fn crisscross_merge_uses_virtual_base_not_an_arbitrary_pick() {
    // A crisscross history has two equally-valid merge bases. Picking one arbitrarily can
    // turn a clean merge into a spurious conflict; the recursive virtual base folds both bases
    // together and resolves cleanly. This is the rebase/PR-merge case.
    let mut loom = Loom::new(MemoryStore::new());
    let ns = new_vcs_ns(&mut loom, 1);

    // c0: f = "A".
    loom.write_file(ns, "f.txt", b"A", 0o100644).unwrap();
    loom.commit(ns, "nas", "c0", 1).unwrap();
    loom.branch(ns, "b1").unwrap();
    loom.branch(ns, "b2").unwrap();

    // b1 (commit d): f -> "B"; keep a ref at d so the later merge can still reach it.
    loom.checkout_branch(ns, "b1").unwrap();
    loom.write_file(ns, "f.txt", b"B", 0o100644).unwrap();
    let d = loom.commit(ns, "nas", "d", 2).unwrap();
    loom.branch(ns, "dref").unwrap();

    // b2 (commit e): leaves f = "A", adds g.
    loom.checkout_branch(ns, "b2").unwrap();
    loom.write_file(ns, "g.txt", b"g", 0o100644).unwrap();
    let e = loom.commit(ns, "nas", "e", 3).unwrap();

    // The crisscross: b1 merges b2, and b2 merges d.
    loom.checkout_branch(ns, "b1").unwrap();
    assert!(matches!(
        loom.merge(ns, "b2", "nas", 4).unwrap(),
        MergeOutcome::Merged(_)
    ));
    loom.checkout_branch(ns, "b2").unwrap();
    assert!(matches!(
        loom.merge(ns, "dref", "nas", 5).unwrap(),
        MergeOutcome::Merged(_)
    ));

    // On b1, change f -> "X".
    loom.checkout_branch(ns, "b1").unwrap();
    loom.write_file(ns, "f.txt", b"X", 0o100644).unwrap();
    let c1x = loom.commit(ns, "nas", "x", 6).unwrap();
    let m2 = loom.registry().branch_tip(ns, "b2").unwrap().unwrap();

    // Exactly two maximal common ancestors: a genuine crisscross.
    let mut bases = loom.merge_base_set(c1x, m2).unwrap();
    bases.sort();
    let mut expect = vec![d, e];
    expect.sort();
    assert_eq!(bases, expect, "expected the two crisscross merge bases");

    // The merge resolves cleanly via the virtual base, and f settles to "X".
    let outcome = loom.merge(ns, "b2", "nas", 7).unwrap();
    let MergeOutcome::Merged(m) = outcome else {
        panic!("crisscross merge should resolve via the virtual base, got {outcome:?}");
    };
    let (files, _dirs) = loom.flatten_commit(m).unwrap();
    let f_addr = match files.get("f.txt") {
        Some(StagedEntry::File(f)) => Some(f.content_addr),
        _ => None,
    };
    assert_eq!(f_addr, Some(crate::object::content_address(b"X")));
    assert!(files.contains_key("g.txt"));
}

#[test]
fn files_workspace_can_branch_and_merge() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = loom
        .registry_mut()
        .create(FacetKind::Files, None, nid(9))
        .unwrap();
    loom.write_file(ns, "note.txt", b"hello", 0o100644).unwrap();
    loom.commit(ns, "nas", "snapshot", 1).unwrap();
    loom.branch(ns, "feature").unwrap();
    assert!(matches!(
        loom.merge(ns, DEFAULT_BRANCH, "nas", 2).unwrap(),
        MergeOutcome::UpToDate
    ));
}

fn queue_ns(loom: &mut Loom<MemoryStore>, seed: u8) -> WorkspaceId {
    loom.registry_mut()
        .create(FacetKind::Queue, None, nid(seed))
        .unwrap()
}

#[test]
fn structured_stream_small_root_is_pinned() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = queue_ns(&mut loom, 20);
    loom.stream_append(ns, "events", b"e0").unwrap();
    loom.stream_append(ns, "events", b"e1").unwrap();
    let root = loom.stream_root(ns, "events").unwrap();
    assert_eq!(
        root.to_string(),
        "blake3:e99b469a3e49cf0b4019d67eeeeebd60e2e2756f53c88a8f1310c275e468b566"
    );
}

#[test]
fn structured_stream_multi_leaf_root_is_pinned() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = queue_ns(&mut loom, 21);
    for i in 0..300u64 {
        loom.stream_append(ns, "events", &i.to_be_bytes()).unwrap();
    }
    let root = loom.stream_root(ns, "events").unwrap();
    let (length, entries_root) = loom.stream_root_parts(root).unwrap();
    assert_eq!(length, 300);
    let nodes = crate::prolly::reachable_nodes(&loom.store, &entries_root.unwrap()).unwrap();
    assert!(
        nodes.len() > 1,
        "a multi-leaf entry map must have more than one node, got {}",
        nodes.len()
    );
    assert_eq!(
        root.to_string(),
        "blake3:5d69f613f12b0e1a5822d15b4946d0a6690f79e36a4183491f7095eb7c479be1"
    );
}

#[test]
fn appending_one_entry_shares_most_prolly_nodes() {
    let mut loom = Loom::new(MemoryStore::new());
    let ns = queue_ns(&mut loom, 22);
    for i in 0..300u64 {
        loom.stream_append(ns, "events", &i.to_be_bytes()).unwrap();
    }
    let before_root = loom.stream_root(ns, "events").unwrap();
    let (_, before_entries) = loom.stream_root_parts(before_root).unwrap();
    let before: BTreeSet<Digest> =
        crate::prolly::reachable_nodes(&loom.store, &before_entries.unwrap())
            .unwrap()
            .into_iter()
            .collect();

    loom.stream_append(ns, "events", &300u64.to_be_bytes())
        .unwrap();
    let after_root = loom.stream_root(ns, "events").unwrap();
    let (_, after_entries) = loom.stream_root_parts(after_root).unwrap();
    let after: BTreeSet<Digest> =
        crate::prolly::reachable_nodes(&loom.store, &after_entries.unwrap())
            .unwrap()
            .into_iter()
            .collect();

    assert!(
        before.len() >= 3,
        "need a multi-node tree to measure sharing, got {}",
        before.len()
    );
    let shared = before.intersection(&after).count();
    assert!(
        shared + 4 >= before.len(),
        "appending one entry must share most prolly nodes: shared {shared} of {}",
        before.len()
    );
}
