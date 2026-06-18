#!/usr/bin/env bash
# Move git tags OFF a wrong (source) commit ONTO the correct commit, on the remote.
#
# You provide the SOURCE commit: the commit the misplaced tags currently sit on (for example the
# version-bump commit a release tag landed on instead of the merge commit). The script:
#   1. fetches tags and branches from the remote (works even if you have no local tags),
#   2. finds every tag pointing at <source-commit>,
#   3. picks the TARGET commit: by default the merge commit on <branch> that brought <source> in
#      (you can also pass an explicit target as the 2nd argument),
#   4. re-points those tags onto the target, force-pushes them back to the remote, and then
#      deletes the local tag copies (the tags live on the remote, not here).
#
# Usage:
#   scripts/retag-misplaced-tags.sh <source-commit> [target-commit]
# Environment (defaults): REMOTE=origin  BRANCH=main  KEEP_LOCAL_TAGS=0 (set to 1 to keep local tags)
#
# Dry-run by default (prints the plan, changes nothing). Re-run with APPLY=1 to execute:
#   bash scripts/retag-misplaced-tags.sh c3372c2            # preview, auto-detect target
#   APPLY=1 bash scripts/retag-misplaced-tags.sh c3372c2    # move + force-push
#
# Needs push access to the remote. If tags are protected by a ruleset, allow yourself to bypass it.
set -euo pipefail

source_ref="${1:-}"
if [ -z "$source_ref" ]; then
  echo "usage: $0 <source-commit> [target-commit]" >&2
  exit 2
fi
target_ref="${2:-}"
remote="${REMOTE:-origin}"
branch="${BRANCH:-main}"

# Pull remote tags + branches so this works without local tags. Non-fatal if offline.
git fetch --tags --prune --force "$remote" || echo "warning: fetch from '$remote' failed; using local refs" >&2

if ! src_sha="$(git rev-parse --verify --quiet "${source_ref}^{commit}")"; then
  echo "error: source commit '$source_ref' not found (after fetching '$remote')." >&2
  exit 1
fi

# Pick the target commit.
if [ -n "$target_ref" ]; then
  if ! target_sha="$(git rev-parse --verify --quiet "${target_ref}^{commit}")"; then
    echo "error: target commit '$target_ref' not found." >&2
    exit 1
  fi
  target_origin="given"
else
  # The commit on <remote>/<branch> whose parent list includes the source commit (the merge commit).
  target_sha="$(git rev-list --parents "refs/remotes/${remote}/${branch}" \
      | awk -v s="$src_sha" '!done { for (i = 2; i <= NF; i++) if ($i == s) { print $1; done = 1 } }')"
  if [ -z "$target_sha" ]; then
    echo "error: could not find a commit on ${remote}/${branch} that has ${src_sha:0:9} as a parent." >&2
    echo "       Is the source commit merged into ${branch}? Otherwise pass an explicit target." >&2
    exit 1
  fi
  target_origin="auto-detected merge commit on ${remote}/${branch}"
fi

if [ "$src_sha" = "$target_sha" ]; then
  echo "error: source and target are the same commit (${src_sha:0:9}); nothing to move." >&2
  exit 1
fi

# Tags currently pointing at the source commit (peeled, so annotated tags resolve to their commit).
tags=()
while IFS= read -r tag; do
  [ -n "$tag" ] || continue
  if [ "$(git rev-parse "refs/tags/${tag}^{commit}")" = "$src_sha" ]; then
    tags+=("$tag")
  fi
done < <(git tag)

apply="${APPLY:-0}"
echo "Remote:  $remote"
echo "Source:  ${src_sha:0:9}  (tags here will be moved)"
echo "Target:  ${target_sha:0:9}  (${target_origin})"
echo "Tags on source: ${#tags[@]}"
if [ "$apply" = "1" ]; then
  echo "Mode: APPLY (moves tags and force-pushes to $remote)"
else
  echo "Mode: dry-run (set APPLY=1 to execute)"
fi
echo

if [ "${#tags[@]}" -eq 0 ]; then
  echo "No tags point at ${src_sha:0:9}; nothing to do." >&2
  exit 1
fi

for tag in "${tags[@]}"; do
  echo "move   $tag : ${src_sha:0:9} -> ${target_sha:0:9}"
  if [ "$apply" = "1" ]; then
    git tag -f "$tag" "$target_sha" >/dev/null
    git push --force "$remote" "refs/tags/${tag}"
  fi
done

# The tags belong on the remote, not here. Delete the local copies that the fetch (and the move)
# created, so this repo is left without them. Set KEEP_LOCAL_TAGS=1 to keep them locally.
cleaned=0
if [ "${KEEP_LOCAL_TAGS:-0}" != "1" ]; then
  for tag in "${tags[@]}"; do
    if git tag -d "$tag" >/dev/null 2>&1; then
      cleaned=$((cleaned + 1))
    fi
  done
fi

echo
if [ "$apply" = "1" ]; then
  echo "done: moved ${#tags[@]} tag(s) from ${src_sha:0:9} to ${target_sha:0:9} on $remote"
else
  echo "dry-run complete. Re-run with APPLY=1 to move the listed tags."
fi
if [ "$cleaned" -gt 0 ]; then
  echo "removed $cleaned local tag copy(ies); the tags live on $remote, not in this repo"
fi
