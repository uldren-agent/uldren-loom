//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

/// Workspace/entry-level blame for `branch` in workspace `workspace`: each current path plus the
/// commit that last set it, as canonical-CBOR
/// (`{ "kind": "PathBlame", "paths": [ { "path", "commit" } ] }`). Mirrors the C ABI `loom_vcs_blame`.
#[napi]
pub fn vcs_blame(
    loom_path: String,
    workspace: String,
    branch: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let paths = loom.blame(ns, &branch).map_err(reason)?;
    let bytes = result_cbor::path_blame_cbor(&paths).map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}
/// Cross-facet structural diff between two commits as the raw `LMDIFF` canonical-CBOR envelope.
/// Mirrors the C ABI `loom_vcs_diff`.
#[napi]
pub fn vcs_diff(
    loom_path: String,
    workspace: String,
    from_commit: String,
    to_commit: String,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let from = Digest::parse(&from_commit).map_err(reason)?;
    let to = Digest::parse(&to_commit).map_err(reason)?;
    let bytes = loom.diff_commits(ns, from, to).map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}

fn parse_watch_change_kinds(kinds: Option<Vec<String>>) -> napi::Result<Vec<ChangeKind>> {
    kinds
        .unwrap_or_default()
        .into_iter()
        .map(|kind| match kind.as_str() {
            "added" => Ok(ChangeKind::Added),
            "modified" => Ok(ChangeKind::Modified),
            "deleted" => Ok(ChangeKind::Deleted),
            other => Err(napi::Error::from_reason(format!(
                "unknown watch change kind {other:?}"
            ))),
        })
        .collect()
}

/// Subscribe to workspace history changes and return an opaque watch cursor string.
#[napi]
pub fn watch_subscribe(
    loom_path: String,
    workspace: String,
    branch: String,
    facet: Option<String>,
    path_prefix: Option<String>,
    change_kinds: Option<Vec<String>>,
    from_commit: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_workspace_arg(&loom, &workspace)?;
    let mut selector = WatchSelector::new(ns, &branch).map_err(reason)?;
    if let Some(facet) = facet.as_deref().filter(|value| !value.is_empty()) {
        selector = selector.with_facet(FacetKind::parse(facet).map_err(reason)?);
    }
    if let Some(path_prefix) = path_prefix.filter(|value| !value.is_empty()) {
        selector = selector.with_path_prefix(path_prefix);
    }
    for kind in parse_watch_change_kinds(change_kinds)? {
        selector = selector.with_change_kind(kind);
    }
    let from = from_commit
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(Digest::parse)
        .transpose()
        .map_err(reason)?;
    Ok(loom
        .watch_subscribe(&selector, from)
        .map_err(reason)?
        .encode())
}

/// Poll an opaque watch cursor and return a canonical-CBOR `loom.watch.batch.v1` batch.
#[napi]
pub fn watch_poll(
    loom_path: String,
    cursor: String,
    max: u32,
    passphrase: Option<String>,
) -> napi::Result<Uint8Array> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let cursor = WatchCursor::decode(&cursor).map_err(reason)?;
    let batch = loom.watch_poll(&cursor, max as usize).map_err(reason)?;
    let bytes = watch_batch_to_cbor(&batch).map_err(reason)?;
    Ok(Uint8Array::from(bytes))
}
/// Whether workspace `workspace` (selected with `facet`) has a conflicted merge awaiting `mergeContinue`
/// or `mergeAbort`.
#[napi]
pub fn merge_in_progress(
    loom_path: String,
    facet: String,
    workspace: String,
    passphrase: Option<String>,
) -> napi::Result<bool> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.merge_in_progress(ns).map_err(reason)
}
/// The still-unresolved conflict paths of the in-progress merge, in path order; empty when no merge is
/// in progress.
#[napi]
pub fn merge_conflicts(
    loom_path: String,
    facet: String,
    workspace: String,
    passphrase: Option<String>,
) -> napi::Result<Vec<String>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.merge_conflicts(ns).map_err(reason)
}
/// Settle one conflicted `path` of the in-progress merge. `resolution` is `"ours"`, `"theirs"`, or
/// `"working"` (accept the currently staged content).
#[napi]
pub fn merge_resolve(
    loom_path: String,
    facet: String,
    workspace: String,
    path: String,
    resolution: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let res = parse_conflict_resolution(&resolution)?;
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.merge_resolve(ns, &path, res).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Abandon the in-progress merge, restoring the pre-merge working tree. Throws `INVALID_ARGUMENT` if no
/// merge is in progress.
#[napi]
pub fn merge_abort(
    loom_path: String,
    facet: String,
    workspace: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.merge_abort(ns).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Finish the in-progress merge: record the two-parent merge commit and return its content address.
/// Throws `CONFLICT` if conflicts remain.
#[napi]
pub fn merge_continue(
    loom_path: String,
    facet: String,
    workspace: String,
    author: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    let digest = loom.merge_continue(ns, &author, now_ms()).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(digest.to_string())
}
/// Stage `paths` into the workspace's shared index (one stage across all facets).
#[napi]
pub fn stage(
    loom_path: String,
    facet: String,
    workspace: String,
    paths: Vec<String>,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    loom.stage(ns, &refs).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Stage the entire working tree (every change across every facet) into the shared index.
#[napi]
pub fn stage_all(
    loom_path: String,
    facet: String,
    workspace: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.stage_all(ns).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Unstage `paths`, reverting each index entry to its HEAD state.
#[napi]
pub fn unstage(
    loom_path: String,
    facet: String,
    workspace: String,
    paths: Vec<String>,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    loom.unstage(ns, &refs).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// The workspace status as a JSON string (`{ staged, unstaged, untracked, conflicts }`).
#[napi]
pub fn status_json(
    loom_path: String,
    facet: String,
    workspace: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    Ok(status_to_json(&loom.status(ns).map_err(reason)?))
}
/// Commit only the staged index (`commit --staged`); returns the new commit's content address.
#[napi]
pub fn commit_staged(
    loom_path: String,
    facet: String,
    workspace: String,
    author: String,
    message: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    let digest = loom
        .commit_staged(ns, &author, &message, now_ms())
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(digest.to_string())
}
/// Restore one `path` in the working tree to the snapshot `rev` resolves to (HEAD|branch|digest);
/// absent in the snapshot removes it. Working tree only.
#[napi]
pub fn restore_file(
    loom_path: String,
    facet: String,
    workspace: String,
    rev: String,
    path: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.restore_file(ns, &rev, &path).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Restore the subtree under `prefix` to the snapshot `rev` resolves to (a `""` prefix restores the
/// whole tree). Working tree only.
#[napi]
pub fn restore_path(
    loom_path: String,
    facet: String,
    workspace: String,
    rev: String,
    prefix: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.restore_path(ns, &rev, &prefix).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Cherry-pick `commits` (digest strings) onto the current branch, preserving each author and message.
/// `dryRun` previews conflicts. Returns the outcome JSON.
#[napi]
pub fn cherry_pick(
    loom_path: String,
    facet: String,
    workspace: String,
    commits: Vec<String>,
    dry_run: bool,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    let list = parse_commits(&commits)?;
    let outcome = loom
        .cherry_pick(ns, &list, now_ms(), dry_run)
        .map_err(reason)?;
    if !dry_run {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(replay_json(outcome))
}
/// Revert `commits` (digest strings) on the current branch as new commits authored by `author`.
/// `dryRun` previews conflicts. Returns the outcome JSON.
#[napi]
pub fn revert(
    loom_path: String,
    facet: String,
    workspace: String,
    commits: Vec<String>,
    author: String,
    dry_run: bool,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    let list = parse_commits(&commits)?;
    let outcome = loom
        .revert(ns, &list, &author, now_ms(), dry_run)
        .map_err(reason)?;
    if !dry_run {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(replay_json(outcome))
}
/// Rebase the current branch onto `onto` (HEAD|branch|digest), replaying first-parent commits linearly.
/// `dryRun` previews conflicts. Returns the outcome JSON.
#[napi]
pub fn rebase(
    loom_path: String,
    facet: String,
    workspace: String,
    onto: String,
    dry_run: bool,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    let outcome = loom.rebase(ns, &onto, now_ms(), dry_run).map_err(reason)?;
    if !dry_run {
        save_loom(&mut loom).map_err(reason)?;
    }
    Ok(replay_json(outcome))
}
/// Squash the commits after `onto` up to the tip into one commit (`author`/`message`); returns the new
/// commit digest. `onto` must be an ancestor of the tip and not the tip itself.
#[napi]
pub fn squash(
    loom_path: String,
    facet: String,
    workspace: String,
    onto: String,
    author: String,
    message: String,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    let new = loom
        .squash(ns, &onto, &author, &message, now_ms())
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(new.to_string())
}
/// Create tag `name` at the commit `rev` resolves to (`HEAD`, a branch name, or a digest). A non-empty
/// `message` makes an annotated tag (with `tagger`); empty makes a lightweight tag. Returns the ref
/// target digest (the commit, or the tag object).
#[napi]
pub fn tag_create(
    loom_path: String,
    facet: String,
    workspace: String,
    name: String,
    rev: String,
    tagger: Option<String>,
    message: Option<String>,
    passphrase: Option<String>,
) -> napi::Result<String> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    let target = loom
        .tag_create(
            ns,
            &name,
            &rev,
            tagger.as_deref().unwrap_or(""),
            message.as_deref().unwrap_or(""),
            now_ms(),
        )
        .map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(target.to_string())
}
/// All tag names in the workspace, sorted.
#[napi]
pub fn tag_list(
    loom_path: String,
    facet: String,
    workspace: String,
    passphrase: Option<String>,
) -> napi::Result<Vec<String>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.tag_list(ns).map_err(reason)
}
/// The raw ref target digest of tag `name` (commit for lightweight, tag object for annotated), or null.
#[napi]
pub fn tag_target(
    loom_path: String,
    facet: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<Option<String>> {
    let loom = open_loom_read_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref())
        .map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    Ok(loom
        .tag_target(ns, &name)
        .map_err(reason)?
        .map(|d| d.to_string()))
}
/// Delete tag `name` (errors if absent).
#[napi]
pub fn tag_delete(
    loom_path: String,
    facet: String,
    workspace: String,
    name: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.tag_delete(ns, &name).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
/// Rename tag `oldName` to `newName`, preserving its target.
#[napi]
pub fn tag_rename(
    loom_path: String,
    facet: String,
    workspace: String,
    old_name: String,
    new_name: String,
    passphrase: Option<String>,
) -> napi::Result<()> {
    let mut loom =
        open_loom_unlocked(&loom_path, key_spec(passphrase.as_deref()).as_ref()).map_err(reason)?;
    let ns = resolve_typed_ns(&loom, &facet, &workspace)?;
    loom.tag_rename(ns, &old_name, &new_name).map_err(reason)?;
    save_loom(&mut loom).map_err(reason)?;
    Ok(())
}
