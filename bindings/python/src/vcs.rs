//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Whether workspace `workspace` (selected with `facet`) has a conflicted merge awaiting
/// ``merge_continue`` or ``merge_abort``.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, passphrase=None))]
pub(crate) fn merge_in_progress(
    path: &str,
    facet: &str,
    workspace: &str,
    passphrase: Option<&str>,
) -> PyResult<bool> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.merge_in_progress(ns).map_err(py_err)
}
/// The still-unresolved conflict paths of the in-progress merge, in path order; empty when none.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, passphrase=None))]
pub(crate) fn merge_conflicts(
    path: &str,
    facet: &str,
    workspace: &str,
    passphrase: Option<&str>,
) -> PyResult<Vec<String>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.merge_conflicts(ns).map_err(py_err)
}
/// Settle one conflicted ``conflict_path`` of the in-progress merge. ``resolution`` is ``"ours"``,
/// ``"theirs"``, or ``"working"`` (accept the currently staged content).
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, conflict_path, resolution, passphrase=None))]
pub(crate) fn merge_resolve(
    path: &str,
    facet: &str,
    workspace: &str,
    conflict_path: &str,
    resolution: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let res = parse_conflict_resolution(resolution)?;
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.merge_resolve(ns, conflict_path, res).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Abandon the in-progress merge, restoring the pre-merge working tree. Raises ``INVALID_ARGUMENT`` if
/// no merge is in progress.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, passphrase=None))]
pub(crate) fn merge_abort(
    path: &str,
    facet: &str,
    workspace: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.merge_abort(ns).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Finish the in-progress merge: record the two-parent merge commit and return its content address.
/// Raises ``CONFLICT`` if conflicts remain.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, author, passphrase=None))]
pub(crate) fn merge_continue(
    path: &str,
    facet: &str,
    workspace: &str,
    author: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    let digest = loom.merge_continue(ns, author, now_ms()).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(digest.to_string())
}
/// Stage `paths` into the workspace's shared index (one stage across all facets).
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, paths, passphrase=None))]
pub(crate) fn stage(
    path: &str,
    facet: &str,
    workspace: &str,
    paths: Vec<String>,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    loom.stage(ns, &refs).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Stage the entire working tree (every change across every facet) into the shared index.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, passphrase=None))]
pub(crate) fn stage_all(
    path: &str,
    facet: &str,
    workspace: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.stage_all(ns).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Unstage `paths`, reverting each index entry to its HEAD state.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, paths, passphrase=None))]
pub(crate) fn unstage(
    path: &str,
    facet: &str,
    workspace: &str,
    paths: Vec<String>,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    loom.unstage(ns, &refs).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// The workspace status as a JSON string (`{ staged, unstaged, untracked, conflicts }`).
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, passphrase=None))]
pub(crate) fn status_json(
    path: &str,
    facet: &str,
    workspace: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    Ok(status_to_json(&loom.status(ns).map_err(py_err)?))
}
/// Commit only the staged index (`commit --staged`); returns the new commit's content address.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, author, message, passphrase=None))]
pub(crate) fn commit_staged(
    path: &str,
    facet: &str,
    workspace: &str,
    author: &str,
    message: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    let digest = loom
        .commit_staged(ns, author, message, now_ms())
        .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(digest.to_string())
}
/// Restore one `file_path` in the working tree to the snapshot `rev` resolves to (absent => removed).
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, rev, file_path, passphrase=None))]
pub(crate) fn restore_file(
    path: &str,
    facet: &str,
    workspace: &str,
    rev: &str,
    file_path: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.restore_file(ns, rev, file_path).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Restore the subtree under `prefix` to the snapshot `rev` resolves to (a `""` prefix restores all).
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, rev, prefix, passphrase=None))]
pub(crate) fn restore_path(
    path: &str,
    facet: &str,
    workspace: &str,
    rev: &str,
    prefix: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.restore_path(ns, rev, prefix).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Cherry-pick `commits` (digest strings) onto the current branch, preserving each author and message.
/// `dry_run` previews conflicts. Returns the outcome JSON.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, commits, dry_run=false, passphrase=None))]
pub(crate) fn cherry_pick(
    path: &str,
    facet: &str,
    workspace: &str,
    commits: Vec<String>,
    dry_run: bool,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    let list = parse_commits(&commits)?;
    let outcome = loom
        .cherry_pick(ns, &list, now_ms(), dry_run)
        .map_err(py_err)?;
    if !dry_run {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(replay_json(outcome))
}
/// Revert `commits` (digest strings) on the current branch as new commits authored by `author`.
/// `dry_run` previews conflicts. Returns the outcome JSON.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, commits, author, dry_run=false, passphrase=None))]
pub(crate) fn revert(
    path: &str,
    facet: &str,
    workspace: &str,
    commits: Vec<String>,
    author: &str,
    dry_run: bool,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    let list = parse_commits(&commits)?;
    let outcome = loom
        .revert(ns, &list, author, now_ms(), dry_run)
        .map_err(py_err)?;
    if !dry_run {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(replay_json(outcome))
}
/// Rebase the current branch onto `onto` (HEAD|branch|digest), replaying first-parent commits linearly.
/// `dry_run` previews conflicts. Returns the outcome JSON.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, onto, dry_run=false, passphrase=None))]
pub(crate) fn rebase(
    path: &str,
    facet: &str,
    workspace: &str,
    onto: &str,
    dry_run: bool,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    let outcome = loom.rebase(ns, onto, now_ms(), dry_run).map_err(py_err)?;
    if !dry_run {
        save_loom(&mut loom).map_err(py_err)?;
    }
    Ok(replay_json(outcome))
}
/// Squash the commits after `onto` up to the tip into one commit (`author`/`message`); returns the new
/// commit digest. `onto` must be an ancestor of the tip and not the tip itself.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, onto, author, message, passphrase=None))]
pub(crate) fn squash(
    path: &str,
    facet: &str,
    workspace: &str,
    onto: &str,
    author: &str,
    message: &str,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    let new = loom
        .squash(ns, onto, author, message, now_ms())
        .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(new.to_string())
}
/// Create tag `name` at the commit `rev` resolves to (`HEAD`, a branch name, or a digest). A non-empty
/// `message` makes an annotated tag (with `tagger`); empty makes a lightweight tag. Returns the ref
/// target digest (the commit, or the tag object).
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, name, rev, tagger=None, message=None, passphrase=None))]
pub(crate) fn tag_create(
    path: &str,
    facet: &str,
    workspace: &str,
    name: &str,
    rev: &str,
    tagger: Option<&str>,
    message: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    let target = loom
        .tag_create(
            ns,
            name,
            rev,
            tagger.unwrap_or(""),
            message.unwrap_or(""),
            now_ms(),
        )
        .map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(target.to_string())
}
/// All tag names in the workspace, sorted.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, passphrase=None))]
pub(crate) fn tag_list(
    path: &str,
    facet: &str,
    workspace: &str,
    passphrase: Option<&str>,
) -> PyResult<Vec<String>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.tag_list(ns).map_err(py_err)
}
/// The raw ref target digest of tag `name` (commit for lightweight, tag object for annotated), or
/// `None` if absent.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, name, passphrase=None))]
pub(crate) fn tag_target(
    path: &str,
    facet: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<Option<String>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    Ok(loom
        .tag_target(ns, name)
        .map_err(py_err)?
        .map(|d| d.to_string()))
}
/// Delete tag `name` (errors if absent).
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, name, passphrase=None))]
pub(crate) fn tag_delete(
    path: &str,
    facet: &str,
    workspace: &str,
    name: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.tag_delete(ns, name).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Rename tag `old_name` to `new_name`, preserving its target.
#[pyfunction]
#[pyo3(signature = (path, facet, workspace, old_name, new_name, passphrase=None))]
pub(crate) fn tag_rename(
    path: &str,
    facet: &str,
    workspace: &str,
    old_name: &str,
    new_name: &str,
    passphrase: Option<&str>,
) -> PyResult<()> {
    let mut loom = open_loom_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_typed_ns(&loom, facet, workspace)?;
    loom.tag_rename(ns, old_name, new_name).map_err(py_err)?;
    save_loom(&mut loom).map_err(py_err)?;
    Ok(())
}
/// Workspace/entry-level blame for `branch` in `workspace` (selected with `facet`): each current path
/// plus the commit that last set it, as canonical-CBOR (`{ "kind": "PathBlame", "paths": [...] }`).
/// Mirrors the C ABI `loom_vcs_blame`.
#[pyfunction]
#[pyo3(signature = (path, workspace, branch, passphrase=None))]
pub(crate) fn vcs_blame<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    branch: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let paths = loom.blame(ns, branch).map_err(py_err)?;
    let bytes = result_cbor::path_blame_cbor(&paths).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}
/// Cross-facet structural diff between commits as the raw ``LMDIFF`` canonical-CBOR envelope.
/// Mirrors the C ABI `loom_vcs_diff`.
#[pyfunction]
#[pyo3(signature = (path, workspace, from_commit, to_commit, passphrase=None))]
pub(crate) fn vcs_diff<'py>(
    py: Python<'py>,
    path: &str,
    workspace: &str,
    from_commit: &str,
    to_commit: &str,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let from = Digest::parse(from_commit).map_err(py_err)?;
    let to = Digest::parse(to_commit).map_err(py_err)?;
    let bytes = loom.diff_commits(ns, from, to).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}

fn parse_watch_change_kinds(kinds: Option<Vec<String>>) -> PyResult<Vec<ChangeKind>> {
    kinds
        .unwrap_or_default()
        .into_iter()
        .map(|kind| match kind.as_str() {
            "added" => Ok(ChangeKind::Added),
            "modified" => Ok(ChangeKind::Modified),
            "deleted" => Ok(ChangeKind::Deleted),
            other => Err(PyRuntimeError::new_err(format!(
                "unknown watch change kind {other:?}"
            ))),
        })
        .collect()
}

/// Subscribe to workspace history changes and return an opaque watch cursor string.
#[pyfunction]
#[pyo3(signature = (path, workspace, branch, facet=None, path_prefix=None, change_kinds=None, from_commit=None, passphrase=None))]
pub(crate) fn watch_subscribe(
    path: &str,
    workspace: &str,
    branch: &str,
    facet: Option<&str>,
    path_prefix: Option<&str>,
    change_kinds: Option<Vec<String>>,
    from_commit: Option<&str>,
    passphrase: Option<&str>,
) -> PyResult<String> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let ns = resolve_workspace_arg(&loom, workspace)?;
    let mut selector = WatchSelector::new(ns, branch).map_err(py_err)?;
    if let Some(facet) = facet.filter(|value| !value.is_empty()) {
        selector = selector.with_facet(FacetKind::parse(facet).map_err(py_err)?);
    }
    if let Some(path_prefix) = path_prefix.filter(|value| !value.is_empty()) {
        selector = selector.with_path_prefix(path_prefix);
    }
    for kind in parse_watch_change_kinds(change_kinds)? {
        selector = selector.with_change_kind(kind);
    }
    let from = from_commit
        .filter(|value| !value.is_empty())
        .map(Digest::parse)
        .transpose()
        .map_err(py_err)?;
    Ok(loom
        .watch_subscribe(&selector, from)
        .map_err(py_err)?
        .encode())
}

/// Poll an opaque watch cursor and return a canonical-CBOR ``loom.watch.batch.v1`` batch.
#[pyfunction]
#[pyo3(signature = (path, cursor, max, passphrase=None))]
pub(crate) fn watch_poll<'py>(
    py: Python<'py>,
    path: &str,
    cursor: &str,
    max: u32,
    passphrase: Option<&str>,
) -> PyResult<Bound<'py, PyBytes>> {
    let loom = open_loom_read_unlocked(path, key_spec(passphrase).as_ref()).map_err(py_err)?;
    let cursor = WatchCursor::decode(cursor).map_err(py_err)?;
    let batch = loom.watch_poll(&cursor, max as usize).map_err(py_err)?;
    let bytes = watch_batch_to_cbor(&batch).map_err(py_err)?;
    Ok(PyBytes::new(py, &bytes))
}
