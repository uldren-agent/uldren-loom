//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Execute the `merge-conflict` suite against a fresh [`Loom`]: build a divergent same-path change so a
/// merge conflicts, prove the conflict enters a recoverable in-progress state without moving the branch
/// tip, prove abort restores the pre-merge tree, and prove resolve-then-continue records a two-parent
/// merge commit and advances the branch.
pub fn run_merge_conflict_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Files, None, WorkspaceId::from_bytes([11; 16]))?;
    loom.write_file(ns, "a.txt", b"base", 0o100644)?;
    loom.commit(ns, "nas", "base", 1)?;
    loom.branch(ns, "feature")?;
    loom.checkout_branch(ns, "feature")?;
    loom.write_file(ns, "a.txt", b"theirs", 0o100644)?;
    loom.commit(ns, "nas", "feat", 2)?;
    loom.checkout_branch(ns, DEFAULT_BRANCH)?;
    loom.write_file(ns, "a.txt", b"ours", 0o100644)?;
    let ours = loom.commit(ns, "nas", "main", 3)?;

    // A divergent same-path change conflicts and enters an in-progress merge; the tip does not move.
    match loom.merge(ns, "feature", "nas", 4)? {
        MergeOutcome::Conflicts(paths) => {
            assert_eq!(paths, vec!["a.txt".to_string()], "conflict path reported");
        }
        other => panic!("expected conflicts, got {other:?}"),
    }
    assert!(
        loom.merge_in_progress(ns)?,
        "a conflict must enter the in-progress merge state"
    );
    assert_eq!(
        loom.registry().branch_tip(ns, DEFAULT_BRANCH)?,
        Some(ours),
        "the branch tip must not move during a conflicted merge"
    );

    // Abort restores the pre-merge working tree exactly and clears the state.
    loom.merge_abort(ns)?;
    assert!(
        !loom.merge_in_progress(ns)?,
        "abort clears the in-progress state"
    );
    assert_eq!(
        loom.read_file(ns, "a.txt")?,
        b"ours",
        "abort restores the pre-merge content"
    );

    // Re-enter, refuse continue while unresolved, then resolve and continue into a two-parent commit.
    loom.merge(ns, "feature", "nas", 5)?;
    assert_eq!(
        loom.merge_continue(ns, "nas", 6).unwrap_err().code,
        Code::Conflict,
        "continue is refused while conflicts remain"
    );
    loom.merge_resolve(ns, "a.txt", ConflictResolution::Theirs)?;
    assert!(
        loom.merge_conflicts(ns)?.is_empty(),
        "resolving the last path clears the unresolved set"
    );
    let merged = loom.merge_continue(ns, "nas", 7)?;
    assert!(
        !loom.merge_in_progress(ns)?,
        "continue clears the in-progress state"
    );
    assert_eq!(
        loom.read_file(ns, "a.txt")?,
        b"theirs",
        "the resolved content is committed"
    );
    assert_eq!(
        loom.registry().branch_tip(ns, DEFAULT_BRANCH)?,
        Some(merged),
        "continue advances the branch to the merge commit"
    );
    assert_ne!(merged, ours, "the merge commit is a new commit");
    Ok(())
}

/// Execute the `staging` suite against a fresh [`Loom`]: one shared index, `status` classification,
/// `commit_staged` recording only the index, `commit` recording the whole working tree, and `unstage`
/// reverting an entry to HEAD.
pub fn run_staging_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Files, None, WorkspaceId::from_bytes([13; 16]))?;
    loom.write_file(ns, "a.txt", b"v1", 0o100644)?;
    loom.commit(ns, "nas", "base", 1)?;

    // Modify a (staged) and add b (untracked).
    loom.write_file(ns, "a.txt", b"v2", 0o100644)?;
    loom.write_file(ns, "b.txt", b"b", 0o100644)?;
    loom.stage(ns, &["a.txt"])?;
    let st = loom.status(ns)?;
    assert_eq!(st.staged.len(), 1, "one staged change");
    assert_eq!(st.untracked, vec!["b.txt".to_string()], "b is untracked");

    // commit_staged records only a; b stays untracked.
    loom.commit_staged(ns, "nas", "stage a", 2)?;
    let st = loom.status(ns)?;
    assert!(st.staged.is_empty(), "index clean after commit_staged");
    assert_eq!(st.untracked, vec!["b.txt".to_string()], "b still untracked");
    assert_eq!(
        loom.read_file(ns, "a.txt")?,
        b"v2",
        "a's staged change persisted"
    );

    // unstage round-trip: stage b, then unstage it back to untracked.
    loom.stage(ns, &["b.txt"])?;
    assert!(loom.status(ns)?.untracked.is_empty(), "b staged");
    loom.unstage(ns, &["b.txt"])?;
    assert_eq!(
        loom.status(ns)?.untracked,
        vec!["b.txt".to_string()],
        "unstage returns b to untracked"
    );

    // commit (everything) captures b too; the workspace is then clean.
    loom.commit(ns, "nas", "all", 3)?;
    let st = loom.status(ns)?;
    assert!(
        st.staged.is_empty() && st.unstaged.is_empty() && st.untracked.is_empty(),
        "clean after commit everything"
    );
    Ok(())
}

/// Execute the `file-ops` suite against a fresh [`Loom`]: write/read round-trip, truncating write,
/// append create-and-concatenate, missing-parent rejection, and remove.
pub fn run_file_ops_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Files, None, WorkspaceId::from_bytes([15; 16]))?;

    loom.write_file(ns, "a.txt", b"hello", 0o100644)?;
    assert_eq!(
        loom.read_file(ns, "a.txt")?,
        b"hello",
        "write/read round-trip"
    );
    loom.write_file(ns, "a.txt", b"hi", 0o100644)?;
    assert_eq!(loom.read_file(ns, "a.txt")?, b"hi", "write truncates");

    loom.append_file(ns, "log.txt", b"x")?;
    loom.append_file(ns, "log.txt", b"y")?;
    assert_eq!(loom.read_file(ns, "log.txt")?, b"xy", "append concatenates");

    assert_eq!(
        loom.append_file(ns, "nope/log.txt", b"z").unwrap_err().code,
        Code::NotFound,
        "append into a nonexistent directory fails"
    );

    loom.remove_file(ns, "a.txt")?;
    assert_eq!(
        loom.read_file(ns, "a.txt").unwrap_err().code,
        Code::NotFound,
        "a removed file is absent"
    );
    Ok(())
}

/// Execute the `file-handle` behavioral suite: byte-range `read_at`/`write_at`/`truncate_file` with
/// POSIX semantics, and the open-file-description matrix (shared inode, delete-on-last-close with no
/// path resurrection, whole-file replace while open, open-mode rules, and handle survival across an
/// engine-state reload). The `files` facet backs this, so it runs today.
pub fn run_file_handle_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Files, None, WorkspaceId::from_bytes([16; 16]))?;

    // write_at zero-fills the gap; read_at clamps past the end (pwrite/pread).
    loom.write_at(ns, "f", 5, b"XY")?;
    assert_eq!(
        loom.read_file(ns, "f")?,
        vec![0, 0, 0, 0, 0, b'X', b'Y'],
        "write_at zero-fills the gap before the offset"
    );
    assert_eq!(
        loom.read_at(ns, "f", 6, 100)?,
        b"Y",
        "read_at clamps to EOF"
    );
    assert!(
        loom.read_at(ns, "f", 50, 10)?.is_empty(),
        "read_at past EOF is empty"
    );

    // truncate shrinks then zero-extends (ftruncate).
    loom.write_file(ns, "t", b"hello world", 0o100644)?;
    loom.truncate_file(ns, "t", 5)?;
    assert_eq!(
        loom.read_file(ns, "t")?,
        b"hello",
        "truncate drops the tail"
    );
    loom.truncate_file(ns, "t", 8)?;
    assert_eq!(
        loom.read_file(ns, "t")?,
        vec![b'h', b'e', b'l', b'l', b'o', 0, 0, 0],
        "truncate zero-extends"
    );

    // A streamed edit of a chunked file converges on the whole-rewrite content (chunks dedup).
    let big: Vec<u8> = (0..200_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8)
        .collect();
    loom.write_file(ns, "big.bin", &big, 0o100644)?;
    loom.write_at(ns, "big.bin", 100_000, &[0xAB; 1000])?;
    let mut expected = big.clone();
    expected[100_000..101_000].copy_from_slice(&[0xAB; 1000]);
    assert_eq!(
        loom.read_file(ns, "big.bin")?,
        expected,
        "streamed write_at matches the whole-rewrite bytes"
    );

    // Two handles share one inode: truncate on one is seen by a write_at on the other.
    loom.write_file(ns, "a.txt", b"hello world", 0o100644)?;
    let h1 = loom.file_open(ns, "a.txt", OpenMode::ReadWrite)?;
    let h2 = loom.file_open(ns, "a.txt", OpenMode::ReadWrite)?;
    loom.file_truncate(h1, 0)?;
    loom.file_write_at(h2, 5, b"X")?;
    assert_eq!(
        loom.file_read_at(h1, 0, 100)?,
        vec![0, 0, 0, 0, 0, b'X'],
        "handles share one inode"
    );
    loom.file_close(h1)?;
    loom.file_close(h2)?;

    // Delete-on-last-close: an unlinked path is not resurrected by a surviving handle's write.
    loom.write_file(ns, "d.txt", b"hello world", 0o100644)?;
    let g1 = loom.file_open(ns, "d.txt", OpenMode::ReadWrite)?;
    let g2 = loom.file_open(ns, "d.txt", OpenMode::ReadWrite)?;
    loom.remove_file(ns, "d.txt")?;
    loom.file_write_at(g2, 5, b"X")?;
    assert_eq!(
        loom.read_file(ns, "d.txt").unwrap_err().code,
        Code::NotFound,
        "a write through a handle does not resurrect the unlinked path"
    );
    assert_eq!(
        loom.file_read_at(g1, 0, 100)?,
        b"helloXworld",
        "surviving handles share the detached inode"
    );
    loom.file_close(g1)?;
    loom.file_close(g2)?;
    assert_eq!(
        loom.read_file(ns, "d.txt").unwrap_err().code,
        Code::NotFound,
        "the path stays gone after the last close"
    );

    // A whole-file replace on an open path is visible to the handle (O_TRUNC same inode).
    loom.write_file(ns, "r.txt", b"old", 0o100644)?;
    let r = loom.file_open(ns, "r.txt", OpenMode::ReadWrite)?;
    loom.write_file(ns, "r.txt", b"brand new content", 0o100644)?;
    assert_eq!(
        loom.file_read_at(r, 0, 100)?,
        b"brand new content",
        "an open handle sees a whole-file replace"
    );
    loom.file_close(r)?;

    // Open-mode rules.
    assert_eq!(
        loom.file_open(ns, "missing", OpenMode::Read)
            .unwrap_err()
            .code,
        Code::NotFound,
        "Read on a missing file is NOT_FOUND"
    );
    let w = loom.file_open(ns, "m", OpenMode::Write)?;
    assert_eq!(
        loom.file_read(w, 1).unwrap_err().code,
        Code::InvalidArgument,
        "reading a write-only handle is rejected"
    );
    loom.file_close(w)?;
    loom.write_file(ns, "ro", b"hi", 0o100644)?;
    let ro = loom.file_open(ns, "ro", OpenMode::Read)?;
    assert_eq!(
        loom.file_write(ro, b"x").unwrap_err().code,
        Code::InvalidArgument,
        "writing a read-only handle is rejected"
    );
    loom.file_close(ro)?;

    // A handle and its cursor survive an engine-state reload.
    loom.write_file(ns, "p.txt", b"hello", 0o100644)?;
    let p = loom.file_open(ns, "p.txt", OpenMode::ReadWrite)?;
    loom.file_write_at(p, 5, b" world")?;
    assert_eq!(loom.file_read(p, 5)?, b"hello", "cursor starts at zero");
    let state = loom.export_state();
    loom.import_state(&state)?;
    assert_eq!(
        loom.file_read(p, 100)?,
        b" world",
        "the handle cursor survives a reload"
    );
    loom.file_close(p)?;
    Ok(())
}

/// Execute the `symlink` behavioral suite: git-style symlink create/read, stat reporting, the error
/// matrix, and a commit round-trip. The `files` facet backs this, so it runs today.
pub fn run_symlink_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Files, None, WorkspaceId::from_bytes([18; 16]))?;
    loom.write_file(ns, "real.txt", b"hello", 0o100644)?;
    // Create and read; a dangling target is allowed.
    loom.symlink(ns, "real.txt", "link")?;
    loom.symlink(ns, "nowhere", "dangling")?;
    assert_eq!(
        loom.read_link(ns, "link")?,
        "real.txt",
        "symlink target round-trips"
    );
    assert_eq!(
        loom.read_link(ns, "dangling")?,
        "nowhere",
        "dangling target allowed"
    );
    // stat reports a symlink.
    let st = loom.stat(ns, "link")?;
    assert_eq!(
        st.kind,
        loom_core::FileKind::Symlink,
        "stat reports a symlink kind"
    );
    // read_link errors: a regular file is INVALID, a missing path is NOT_FOUND.
    assert_eq!(
        loom.read_link(ns, "real.txt").unwrap_err().code,
        Code::InvalidArgument,
        "read_link on a regular file is INVALID_ARGUMENT"
    );
    assert_eq!(
        loom.read_link(ns, "absent").unwrap_err().code,
        Code::NotFound,
        "read_link on a missing path is NOT_FOUND"
    );
    // symlink errors: over an existing path ALREADY_EXISTS, missing parent NOT_FOUND.
    assert_eq!(
        loom.symlink(ns, "x", "link").unwrap_err().code,
        Code::AlreadyExists,
        "symlink over an existing path is ALREADY_EXISTS"
    );
    assert_eq!(
        loom.symlink(ns, "x", "nope/l").unwrap_err().code,
        Code::NotFound,
        "symlink under a missing parent is NOT_FOUND"
    );
    // The symlink survives a commit + checkout round-trip.
    let c0 = loom.commit(ns, "conformance", "add link", 1)?;
    loom.remove_file(ns, "link")?;
    loom.checkout_commit(ns, c0)?;
    assert_eq!(
        loom.read_link(ns, "link")?,
        "real.txt",
        "symlink survives commit + checkout"
    );
    Ok(())
}

/// Execute the `tags` behavioral suite: lightweight and annotated tag creation, revision resolution
/// (HEAD / branch / digest), list/target reads, rename and delete, and the error matrix. The `vcs`
/// surface backs this, so it runs today.
pub fn run_tags_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Files, None, WorkspaceId::from_bytes([17; 16]))?;
    loom.write_file(ns, "a.txt", b"hello", 0o100644)?;
    let c0 = loom.commit(ns, "conformance", "init", 1)?;

    // Lightweight tag at HEAD points straight at the commit.
    let v1 = loom.tag_create(ns, "v1", "HEAD", "", "", 0)?;
    assert_eq!(v1, c0, "lightweight tag points at the HEAD commit");
    assert_eq!(
        loom.tag_target(ns, "v1")?,
        Some(c0),
        "tag_target reads back"
    );

    // Annotated tag stores a Tag object carrying the metadata.
    let v1ann = loom.tag_create(ns, "v1-ann", &c0.to_string(), "conformance", "release 1", 5)?;
    assert_ne!(v1ann, c0, "an annotated ref points at the tag object");
    match Object::decode(&loom.object_bytes(v1ann)?)? {
        Object::Tag(t) => {
            assert_eq!(t.target, c0, "annotated tag targets the commit");
            assert_eq!(t.message, "release 1", "annotation message is stored");
        }
        other => panic!("expected a tag object, got {:?}", other.object_type()),
    }

    // Revision resolution: a branch name and a digest resolve to the same commit.
    loom.branch(ns, "feature")?;
    assert_eq!(
        loom.tag_create(ns, "by-branch", "feature", "", "", 0)?,
        c0,
        "a branch name resolves to its tip"
    );
    assert_eq!(
        loom.tag_create(ns, "bad", "nope", "", "", 0)
            .unwrap_err()
            .code,
        Code::NotFound,
        "an unknown revision is NOT_FOUND"
    );

    // Names list sorted; rename preserves the target; duplicate and missing names error.
    assert_eq!(
        loom.tag_list(ns)?,
        vec![
            "by-branch".to_string(),
            "v1".to_string(),
            "v1-ann".to_string()
        ],
        "tag names list sorted"
    );
    assert_eq!(
        loom.tag_create(ns, "v1", "HEAD", "", "", 0)
            .unwrap_err()
            .code,
        Code::AlreadyExists,
        "a duplicate tag name is ALREADY_EXISTS"
    );
    loom.tag_rename(ns, "v1", "v2")?;
    assert_eq!(
        loom.tag_target(ns, "v2")?,
        Some(c0),
        "rename preserves the target"
    );
    assert!(loom.tag_target(ns, "v1")?.is_none(), "the old name is gone");
    assert_eq!(
        loom.tag_rename(ns, "missing", "x").unwrap_err().code,
        Code::NotFound,
        "renaming a missing tag is NOT_FOUND"
    );

    // Delete removes it; deleting again is NOT_FOUND.
    loom.tag_delete(ns, "v2")?;
    assert!(
        loom.tag_target(ns, "v2")?.is_none(),
        "delete removes the tag"
    );
    assert_eq!(
        loom.tag_delete(ns, "v2").unwrap_err().code,
        Code::NotFound,
        "deleting a missing tag is NOT_FOUND"
    );
    Ok(())
}

/// Execute the `restore` behavioral suite: `restore_file` (revert + remove-when-absent) and
/// `restore_path` (subtree-only), all working-tree-only with `HEAD` untouched. The `vcs` surface backs
/// this, so it runs today.
pub fn run_restore_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Files, None, WorkspaceId::from_bytes([19; 16]))?;
    loom.create_directory(ns, "src", false)?;
    loom.write_file(ns, "src/x.rs", b"x", 0o100644)?;
    loom.write_file(ns, "src/y.rs", b"y", 0o100644)?;
    loom.write_file(ns, "top.txt", b"t1", 0o100644)?;
    let c0 = loom.commit(ns, "conformance", "init", 1)?;

    // restore_file reverts an edit and removes an untracked path.
    loom.write_file(ns, "top.txt", b"t2", 0o100644)?;
    loom.write_file(ns, "extra.txt", b"e", 0o100644)?;
    loom.restore_file(ns, "HEAD", "top.txt")?;
    assert_eq!(
        loom.read_file(ns, "top.txt")?,
        b"t1",
        "restore_file reverts an edit"
    );
    loom.restore_file(ns, "HEAD", "extra.txt")?;
    assert_eq!(
        loom.read_file(ns, "extra.txt").unwrap_err().code,
        Code::NotFound,
        "restore_file removes a path absent in the snapshot"
    );
    assert_eq!(loom.log(ns, DEFAULT_BRANCH)?, vec![c0], "HEAD is untouched");

    // restore_path resets only the subtree.
    loom.remove_file(ns, "src/x.rs")?;
    loom.write_file(ns, "src/z.rs", b"z", 0o100644)?;
    loom.write_file(ns, "top.txt", b"t3", 0o100644)?;
    loom.restore_path(ns, "HEAD", "src")?;
    assert_eq!(
        loom.read_file(ns, "src/x.rs")?,
        b"x",
        "subtree path restored"
    );
    assert_eq!(
        loom.read_file(ns, "src/z.rs").unwrap_err().code,
        Code::NotFound,
        "a subtree path absent in the snapshot is removed"
    );
    assert_eq!(
        loom.read_file(ns, "top.txt")?,
        b"t3",
        "paths outside the prefix are untouched"
    );
    Ok(())
}

/// Execute the `replay` behavioral suite: cherry-pick, revert, rebase, and a dry-run conflict preview
/// that makes no change. The `vcs` surface backs this, so it runs today.
pub fn run_replay_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Files, None, WorkspaceId::from_bytes([20; 16]))?;
    loom.write_file(ns, "a.txt", b"base", 0o100644)?;
    let c0 = loom.commit(ns, "conformance", "init", 1)?;

    // A feature branch adds b.txt; cherry-pick it onto the default branch.
    loom.branch(ns, "feature")?;
    loom.checkout_branch(ns, "feature")?;
    loom.write_file(ns, "b.txt", b"feature", 0o100644)?;
    let cf = loom.commit(ns, "dev", "add b", 2)?;
    loom.checkout_branch(ns, DEFAULT_BRANCH)?;
    match loom.cherry_pick(ns, &[cf], 3, false)? {
        ReplayOutcome::Replayed(tip) => {
            assert_eq!(
                loom.read_file(ns, "b.txt")?,
                b"feature",
                "cherry-pick applies the change"
            );
            let author = match Object::decode(&loom.object_bytes(tip)?)? {
                Object::Commit(c) => c.author,
                other => panic!("expected a commit, got {:?}", other.object_type()),
            };
            assert_eq!(author, "dev", "cherry-pick preserves the author");
        }
        other => panic!("expected Replayed, got {other:?}"),
    }

    // Revert the last commit restores the prior state.
    let head_tip = loom.log(ns, DEFAULT_BRANCH)?[0];
    match loom.revert(ns, &[head_tip], "conformance", 4, false)? {
        ReplayOutcome::Replayed(_) => {
            assert_eq!(
                loom.read_file(ns, "b.txt").unwrap_err().code,
                Code::NotFound,
                "revert undoes the cherry-picked add"
            );
        }
        other => panic!("expected Replayed, got {other:?}"),
    }

    // A conflicting cherry-pick is reported atomically by both a dry run and a real run.
    loom.branch(ns, "other")?;
    loom.checkout_branch(ns, "other")?;
    loom.write_file(ns, "a.txt", b"other", 0o100644)?;
    let conflicting = loom.commit(ns, "dev", "edit a", 5)?;
    loom.checkout_branch(ns, DEFAULT_BRANCH)?;
    loom.write_file(ns, "a.txt", b"mainline", 0o100644)?;
    let main_tip = loom.commit(ns, "conformance", "edit a too", 6)?;
    assert!(
        matches!(
            loom.cherry_pick(ns, &[conflicting], 7, true)?,
            ReplayOutcome::Conflicts(_)
        ),
        "dry-run reports the conflict"
    );
    assert!(
        matches!(
            loom.cherry_pick(ns, &[conflicting], 7, false)?,
            ReplayOutcome::Conflicts(_)
        ),
        "a real conflicting pick is atomic"
    );
    assert_eq!(
        loom.log(ns, DEFAULT_BRANCH)?[0],
        main_tip,
        "the branch tip is unchanged"
    );

    // Rebase: a branch editing its own file replays cleanly onto an advanced default tip.
    let _ = c0; // c0 is the original shared base.
    loom.branch(ns, "rb")?; // created at the current default tip (main_tip)
    loom.checkout_branch(ns, "rb")?;
    loom.write_file(ns, "rfile.txt", b"r", 0o100644)?;
    loom.commit(ns, "dev", "rb work", 8)?;
    loom.checkout_branch(ns, DEFAULT_BRANCH)?;
    loom.write_file(ns, "mfile.txt", b"m", 0o100644)?;
    let main2 = loom.commit(ns, "conformance", "more main", 9)?;
    loom.checkout_branch(ns, "rb")?;
    match loom.rebase(ns, &main2.to_string(), 10, false)? {
        ReplayOutcome::Replayed(tip) => {
            let parents = match Object::decode(&loom.object_bytes(tip)?)? {
                Object::Commit(c) => c.parents,
                other => panic!("expected a commit, got {:?}", other.object_type()),
            };
            assert_eq!(parents, vec![main2], "rebased atop the target");
            assert_eq!(
                loom.read_file(ns, "rfile.txt")?,
                b"r",
                "the branch change is kept"
            );
            assert_eq!(
                loom.read_file(ns, "mfile.txt")?,
                b"m",
                "the target change is present"
            );
        }
        other => panic!("expected Replayed, got {other:?}"),
    }
    Ok(())
}

/// Execute the `squash` behavioral suite: collapse a commit range into one, and reject a bad base. The
/// `vcs` surface backs this, so it runs today.
pub fn run_squash_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns =
        loom.registry_mut()
            .create(FacetKind::Files, None, WorkspaceId::from_bytes([21; 16]))?;
    loom.write_file(ns, "a.txt", b"a", 0o100644)?;
    let c0 = loom.commit(ns, "conformance", "init", 1)?;
    loom.write_file(ns, "b.txt", b"b", 0o100644)?;
    loom.commit(ns, "conformance", "add b", 2)?;
    loom.write_file(ns, "c.txt", b"c", 0o100644)?;
    loom.commit(ns, "conformance", "add c", 3)?;

    let sq = loom.squash(ns, &c0.to_string(), "conformance", "squashed", 4)?;
    assert_eq!(
        loom.log(ns, DEFAULT_BRANCH)?,
        vec![sq, c0],
        "history collapses to one commit on the base"
    );
    assert_eq!(
        loom.read_file(ns, "b.txt")?,
        b"b",
        "files survive the squash"
    );
    assert_eq!(loom.read_file(ns, "c.txt")?, b"c");
    assert_eq!(
        loom.squash(ns, &sq.to_string(), "conformance", "x", 5)
            .unwrap_err()
            .code,
        Code::InvalidArgument,
        "squashing onto the tip is rejected"
    );
    Ok(())
}

/// Execute the protected-ref suite over the local VCS policy evaluator.
pub fn run_protected_ref_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns = loom.registry_mut().create(
        FacetKind::Files,
        Some("protected-ref"),
        WorkspaceId::from_bytes([25; 16]),
    )?;
    loom.write_file(ns, "a.txt", b"a", 0o100644)?;
    let base = loom.commit(ns, "conformance", "base", 1)?;
    loom.write_file(ns, "b.txt", b"b", 0o100644)?;
    loom.commit(ns, "conformance", "second", 2)?;
    loom.set_protected_ref_policy(
        ns,
        "branch/main",
        ProtectedRefPolicy {
            fast_forward_only: true,
            ..ProtectedRefPolicy::default()
        },
    )?;
    assert!(
        loom.protected_ref_policy(ns, "branch/main")?
            .expect("policy")
            .fast_forward_only,
        "protected-ref policy is readable after configuration"
    );
    assert_eq!(
        loom.squash(ns, &base.to_string(), "conformance", "blocked", 3)
            .unwrap_err()
            .code,
        Code::PermissionDenied,
        "fast-forward-only protection blocks destructive rewrites"
    );

    loom.set_protected_ref_policy(
        ns,
        "branch/main",
        ProtectedRefPolicy {
            signed_commits_required: true,
            signed_ref_advance_required: true,
            required_review_count: 1,
            ..ProtectedRefPolicy::default()
        },
    )?;
    loom.write_file(ns, "c.txt", b"c", 0o100644)?;
    assert_eq!(
        loom.commit(ns, "conformance", "requires proof", 4)
            .unwrap_err()
            .code,
        Code::PermissionDenied,
        "signature and review policy fails closed until proof records exist"
    );

    loom.set_protected_ref_policy(
        ns,
        "branch/main",
        ProtectedRefPolicy {
            fast_forward_only: true,
            ..ProtectedRefPolicy::default()
        },
    )?;
    loom.commit(ns, "conformance", "fast-forward", 5)?;
    loom.tag_create(ns, "release", "HEAD", "conformance", "", 6)?;
    loom.set_protected_ref_policy(
        ns,
        "tag/release",
        ProtectedRefPolicy {
            retention_lock: true,
            governance_lock: true,
            ..ProtectedRefPolicy::default()
        },
    )?;
    assert_eq!(
        loom.tag_delete(ns, "release").unwrap_err().code,
        Code::PermissionDenied,
        "retention and governance locks block tag deletion"
    );
    Ok(())
}

/// Execute the cross-facet commit diff suite over the local core envelope.
pub fn run_diff_commits_behavior<S: ObjectStore>(loom: &mut Loom<S>) -> Result<()> {
    let ns = loom.registry_mut().create(
        FacetKind::Files,
        Some("diff"),
        WorkspaceId::from_bytes([24; 16]),
    )?;
    loom.registry_mut().add_facet(ns, FacetKind::Sql)?;
    loom.registry_mut().add_facet(ns, FacetKind::Kv)?;
    loom.registry_mut().add_facet(ns, FacetKind::Document)?;
    loom.registry_mut().add_facet(ns, FacetKind::Queue)?;
    loom.registry_mut().add_facet(ns, FacetKind::Cas)?;
    loom.registry_mut().add_facet(ns, FacetKind::Calendar)?;
    loom.registry_mut().add_facet(ns, FacetKind::Contacts)?;
    loom.registry_mut().add_facet(ns, FacetKind::Mail)?;
    loom.registry_mut().add_facet(ns, FacetKind::Graph)?;

    let table_path = facet_path(FacetKind::Sql, "app/tables/items");
    let schema = Schema::new(
        vec![
            ("id".to_string(), ColumnType::Int),
            ("name".to_string(), ColumnType::Text),
        ],
        vec![0],
    )?;
    let mut table = Table::new(schema);
    table.insert(vec![Value::Int(1), Value::Text("one".to_string())])?;
    loom.write_file(ns, "readme.txt", b"v1", 0o100644)?;
    loom.stage_table(ns, &table_path, &table)?;
    kv_put(
        loom,
        ns,
        "settings",
        Value::Text("theme".to_string()),
        b"light".to_vec(),
    )?;
    document_put_binary(
        loom,
        ns,
        "people",
        "ada",
        br#"{"name":"Ada"}"#.to_vec(),
        None,
    )?;
    loom.stream_append(ns, "events", b"first")?;
    cas_put(loom, ns, b"blob-one")?;
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
    contacts::create_book(
        loom,
        ns,
        "alice",
        "personal",
        &BookMeta {
            display_name: "Personal".to_string(),
        },
    )?;
    mail::create_mailbox(
        loom,
        ns,
        "alice",
        "inbox",
        &MailboxMeta {
            display_name: "Inbox".to_string(),
        },
    )?;
    mail::ingest_message(
        loom,
        ns,
        "alice",
        "inbox",
        "m1",
        b"From: alice@example.com\r\nTo: bob@example.com\r\nSubject: hello\r\n\r\nbody",
    )?;
    let mut graph_value = Graph::new();
    let mut props = Props::new();
    props.insert("label".to_string(), GraphValue::Text("one".to_string()));
    graph_value.upsert_node("n1", props)?;
    graph::put_graph(loom, ns, "pages", &graph_value)?;
    let c0 = loom.commit(ns, "conformance", "base", 1)?;

    loom.write_file(ns, "readme.txt", b"v2", 0o100644)?;
    loom.insert_row(
        ns,
        &table_path,
        vec![Value::Int(2), Value::Text("two".to_string())],
    )?;
    kv_put(
        loom,
        ns,
        "settings",
        Value::Text("theme".to_string()),
        b"dark".to_vec(),
    )?;
    kv_put(
        loom,
        ns,
        "settings",
        Value::Text("mode".to_string()),
        b"prod".to_vec(),
    )?;
    document_put_binary(
        loom,
        ns,
        "people",
        "ada",
        br#"{"name":"Ada Lovelace"}"#.to_vec(),
        None,
    )?;
    document_put_binary(
        loom,
        ns,
        "people",
        "grace",
        br#"{"name":"Grace"}"#.to_vec(),
        None,
    )?;
    loom.stream_append(ns, "events", b"second")?;
    cas_put(loom, ns, b"blob-two")?;
    calendar::put_entry(
        loom,
        ns,
        "alice",
        "work",
        &CalendarEntry::event("event-1", "Standup", "20260101T090000"),
    )?;
    contacts::put_entry(
        loom,
        ns,
        "alice",
        "personal",
        &ContactEntry::new("contact-1", "Ada Lovelace"),
    )?;
    mail::set_flags(loom, ns, "alice", "inbox", "m1", &["seen".to_string()])?;
    let mut props = Props::new();
    props.insert("label".to_string(), GraphValue::Text("two".to_string()));
    graph_value.upsert_node("n2", props)?;
    graph::put_graph(loom, ns, "pages", &graph_value)?;
    let c1 = loom.commit(ns, "conformance", "changed", 2)?;

    let diff = loom.diff_commits(ns, c0, c1)?;
    let frame = diff_array(&diff);
    assert_eq!(diff_text(&frame[0]), "LMDIFF");
    assert_eq!(diff_uint(&frame[1]), 1);
    assert_eq!(diff_bytes(&frame[2]), ns.as_bytes());
    assert_eq!(diff_bytes(&frame[3]), c0.bytes());
    assert_eq!(diff_bytes(&frame[4]), c1.bytes());
    assert_eq!(
        diff_collection_summary(&diff, "files", &[]),
        Some((0, 0, 1, 0, false, 1)),
        "file path changes are reported"
    );
    assert_eq!(
        diff_collection_summary(&diff, "sql", &["app", "items"]),
        Some((1, 0, 0, 0, false, 1)),
        "SQL row additions are reported"
    );
    assert_eq!(
        diff_collection_summary(&diff, "kv", &["settings"]),
        Some((1, 0, 1, 0, false, 2)),
        "KV key additions and changes are reported"
    );
    assert_eq!(
        diff_collection_summary(&diff, "document", &["people"]),
        Some((1, 0, 1, 0, false, 2)),
        "document id additions and changes are reported"
    );
    assert_eq!(
        diff_collection_summary(&diff, "queue", &["events"]),
        Some((0, 0, 0, 1, false, 1)),
        "queue appends are reported"
    );
    assert_eq!(
        diff_collection_summary(&diff, "cas", &[]),
        Some((1, 0, 0, 0, false, 1)),
        "CAS digest additions are reported"
    );
    assert_eq!(
        diff_collection_summary(&diff, "calendar", &["alice", "work"]),
        Some((1, 0, 0, 0, false, 1)),
        "calendar entry additions are reported"
    );
    assert_eq!(
        diff_collection_summary(&diff, "contacts", &["alice", "personal"]),
        Some((1, 0, 0, 0, false, 1)),
        "contact additions are reported"
    );
    assert_eq!(
        diff_collection_summary(&diff, "mail", &["alice", "inbox"]),
        Some((1, 0, 0, 0, false, 1)),
        "mail flag changes are reported"
    );
    assert_eq!(
        diff_collection_summary(&diff, "graph", &["pages"]),
        Some((0, 0, 1, 0, true, 0)),
        "whole-blob fallback facets are reported as coarse"
    );

    let other_ns = loom.registry_mut().create(
        FacetKind::Files,
        Some("foreign"),
        WorkspaceId::from_bytes([25; 16]),
    )?;
    loom.write_file(other_ns, "foreign.txt", b"x", 0o100644)?;
    let foreign = loom.commit(other_ns, "conformance", "foreign", 3)?;
    assert!(
        matches!(
            loom.diff_commits(ns, c0, foreign).unwrap_err().code,
            Code::CrossWorkspace | Code::PermissionDenied
        ),
        "commits outside the workspace are rejected before returning a diff"
    );
    Ok(())
}

fn diff_array(bytes: &[u8]) -> Vec<CborValue> {
    match loom_codec::decode(bytes).expect("diff envelope must decode") {
        CborValue::Array(items) => items,
        other => panic!("diff envelope must be an array, got {other:?}"),
    }
}

fn diff_text(value: &CborValue) -> &str {
    match value {
        CborValue::Text(text) => text,
        other => panic!("expected text, got {other:?}"),
    }
}

fn diff_uint(value: &CborValue) -> u64 {
    match value {
        CborValue::Uint(n) => *n,
        other => panic!("expected uint, got {other:?}"),
    }
}

fn diff_bool(value: &CborValue) -> bool {
    match value {
        CborValue::Bool(b) => *b,
        other => panic!("expected bool, got {other:?}"),
    }
}

fn diff_bytes(value: &CborValue) -> &[u8] {
    match value {
        CborValue::Bytes(bytes) => bytes,
        other => panic!("expected bytes, got {other:?}"),
    }
}

fn diff_collection_summary(
    diff: &[u8],
    wanted_facet: &str,
    wanted_collection: &[&str],
) -> Option<(u64, u64, u64, u64, bool, usize)> {
    let frame = diff_array(diff);
    let CborValue::Array(facets) = &frame[5] else {
        panic!("facet sections must be an array");
    };
    for facet_section in facets {
        let CborValue::Array(facet_fields) = facet_section else {
            panic!("facet section must be an array");
        };
        if diff_text(&facet_fields[0]) != wanted_facet {
            continue;
        }
        let CborValue::Array(collections) = &facet_fields[1] else {
            panic!("collection sections must be an array");
        };
        for collection_section in collections {
            let CborValue::Array(collection_fields) = collection_section else {
                panic!("collection section must be an array");
            };
            let CborValue::Array(path_values) = &collection_fields[0] else {
                panic!("collection path must be an array");
            };
            let path = path_values.iter().map(diff_text).collect::<Vec<_>>();
            if path != wanted_collection {
                continue;
            }
            let CborValue::Array(summary) = &collection_fields[1] else {
                panic!("summary must be an array");
            };
            let CborValue::Array(units) = &collection_fields[2] else {
                panic!("unit changes must be an array");
            };
            return Some((
                diff_uint(&summary[0]),
                diff_uint(&summary[1]),
                diff_uint(&summary[2]),
                diff_uint(&summary[3]),
                diff_bool(&summary[4]),
                units.len(),
            ));
        }
    }
    None
}
