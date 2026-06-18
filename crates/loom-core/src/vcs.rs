//! The version-control engine over the object model and workspace registry.
//!
//! [`Loom`] is the engine handle. It owns the content-addressed object [`ObjectStore`] (Trees,
//! Commits, Tags, and file Blobs), a [`Registry`] of workspaces, a per-workspace **working tree**
//! (staged edits), and the **content index** that maps a file's *content address*
//! (whole-content BLAKE3, what a Tree references) to the Blob *object* that holds its bytes. On
//! this it implements `commit`, `checkout`, `log`, `diff`, `branch`, and a 3-way `merge`.
//!
//! Workspace history is a DAG regardless of the active facets. This engine runs on any
//! `ObjectStore` (the in-memory `MemoryStore` in tests).

use crate::cbor::{self, Value as CborValue};
use crate::digest::Digest;
use crate::document::{
    Collection as DocumentCollection, DocumentBodyRef, DocumentCollectionManifest, DocumentId,
    DocumentRecord,
};
use crate::error::{Code, LoomError, Result};
use crate::identity::{IdentityStore, PrincipalId};
use crate::kv::{
    BackPressure, EphemeralKvMap, EphemeralPutOptions, KvMap, KvMapConfig, KvTier, OnEvict,
    kv_delete, kv_get, kv_put,
};
use crate::object::{Commit, EntryKind, Object, ObjectType, Tag, TreeEntry, content_address_with};
use crate::provider::{CompressionHint, ObjectStore};
use crate::tabular::{self, Row, RowDiff};
use crate::workspace::{Registry, WorkspaceId, default_compression_for_facets};
use crate::{
    AclDomain, AclPredicateEvaluator, AclResource, AclResourceScope, AclRight, AclScopeKind,
    AclStore, FacetKind,
};
pub use loom_types::vcs::ChangeKind;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

/// A file staged in the working tree: its content address and POSIX mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct StagedFile {
    pub(crate) content_addr: Digest,
    pub(crate) mode: u32,
}

/// One staged working-tree slot: an ordinary file or a structured facet root referenced by its root
/// `Tree` digest. Whole-facet identity for VCS is the root Tree digest, so diff/merge/dedup over
/// structured facets work the same way they do over files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum StagedEntry {
    /// A file slot (its content address + mode).
    File(StagedFile),
    /// A table slot: the digest of its `TABLE`-entry `Tree`.
    Table(Digest),
    /// An append-log stream slot: the digest of its stream-root `Tree`.
    Stream(Digest),
    /// A structured time-series collection slot: the digest of its collection-root `Tree`.
    TimeSeries(Digest),
    /// A structured graph collection slot: the digest of its graph-root `Tree`.
    Graph(Digest),
    /// A structured ledger collection slot: the digest of its ledger-root `Tree`.
    Ledger(Digest),
    /// A structured columnar dataset slot: the digest of its columnar-root `Tree`.
    Columnar(Digest),
    /// A structured document collection slot: the digest of its document-root `Tree`.
    Document(Digest),
}

/// A workspace's working tree: a flat path -> staged-slot map.
pub(crate) type WorkTree = BTreeMap<String, StagedEntry>;

/// A flattened view of a commit: leaf path -> staged slot (same shape as a working tree).
type FileMap = WorkTree;

/// One step of a history replay ([`Loom::replay_onto`]): apply the change from `base` to `theirs` onto
/// the running tree, recording a new commit with `author`/`message` and the `theirs` directory set.
struct ReplayPatch {
    base: FileMap,
    theirs: FileMap,
    theirs_dirs: BTreeSet<String>,
    author: String,
    message: String,
}

/// The kind of byte-range edit [`Loom::build_edited_content`] applies.
enum EditKind {
    /// Overwrite `[offset, offset + data.len())`, zero-filling any gap past the old end.
    Write { offset: u64, data: Vec<u8> },
    /// Resize to the plan's `new_size`, dropping bytes past it or zero-extending up to it.
    Truncate,
}

/// A planned edit of one file's content: the source content (absent for a brand-new file), its old
/// size, the resulting size, and the kind of change. Consumed by [`Loom::build_edited_content`].
pub(crate) struct EditPlan {
    src: Option<Digest>,
    old_size: u64,
    new_size: u64,
    kind: EditKind,
}

impl EditPlan {
    /// A write of `data` at `offset` over `src` (whose size is `old_size`); the result grows to fit.
    fn write(src: Option<Digest>, old_size: u64, offset: u64, data: Vec<u8>) -> Self {
        let new_size = old_size.max(offset + data.len() as u64);
        Self {
            src,
            old_size,
            new_size,
            kind: EditKind::Write { offset, data },
        }
    }
    /// A resize of `src` (size `old_size`) to `size`.
    fn truncate(src: Option<Digest>, old_size: u64, size: u64) -> Self {
        Self {
            src,
            old_size,
            new_size: size,
            kind: EditKind::Truncate,
        }
    }
}

/// 3-way merge two flattened file maps against `base`, at file granularity. Returns the merged map
/// (built on `ours`) and the list of conflicting paths - paths where both sides changed the file
/// differently from the base. The caller decides what a non-empty conflict list means: the real
/// merge aborts and reports them, while the virtual-base reduction resolves them deterministically
/// and continues.
fn three_way_merge_files(
    base: &FileMap,
    ours: &FileMap,
    theirs: &FileMap,
) -> (FileMap, Vec<String>) {
    let mut paths: BTreeSet<String> = BTreeSet::new();
    paths.extend(ours.keys().cloned());
    paths.extend(theirs.keys().cloned());
    let mut merged = ours.clone();
    let mut conflicts = Vec::new();
    for path in &paths {
        let b = base.get(path);
        let o = ours.get(path);
        let t = theirs.get(path);
        if o == t {
            continue;
        }
        if b == o {
            // ours is unchanged from base; take theirs.
            match t {
                Some(v) => {
                    merged.insert(path.clone(), *v);
                }
                None => {
                    merged.remove(path);
                }
            }
        } else if b == t {
            // theirs is unchanged from base; keep ours (already in `merged`).
        } else {
            conflicts.push(path.clone());
        }
    }
    (merged, conflicts)
}

/// One path-level difference between two commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The file path (no leading `/`).
    pub path: String,
    /// What changed.
    pub kind: ChangeKind,
}

#[derive(Debug, Clone)]
struct DiffUnitChange {
    unit_kind: String,
    unit_key: Vec<u8>,
    change: String,
    before: Option<Digest>,
    after: Option<Digest>,
    detail_kind: String,
    detail: CborValue,
}

#[derive(Debug, Default)]
struct DiffCollection {
    coarse: bool,
    units: Vec<DiffUnitChange>,
    coarse_added: u64,
    coarse_removed: u64,
    coarse_changed: u64,
    coarse_appended: u64,
}

#[derive(Debug, Default)]
struct DiffEnvelope {
    collections: BTreeMap<(String, Vec<String>), DiffCollection>,
}

impl DiffEnvelope {
    fn add_unit(&mut self, facet: &str, collection: Vec<String>, unit: DiffUnitChange) {
        self.collections
            .entry((facet.to_string(), collection))
            .or_default()
            .units
            .push(unit);
    }

    fn add_coarse(&mut self, facet: &str, collection: Vec<String>, change: &str, appended: bool) {
        let section = self
            .collections
            .entry((facet.to_string(), collection))
            .or_default();
        section.coarse = true;
        match change {
            "added" => section.coarse_added += 1,
            "removed" => section.coarse_removed += 1,
            "changed" => section.coarse_changed += 1,
            "appended" if appended => section.coarse_appended += 1,
            _ => section.coarse_changed += 1,
        }
    }

    fn encode(self, ns: WorkspaceId, from: Digest, to: Digest) -> Vec<u8> {
        let mut by_facet: BTreeMap<String, Vec<CborValue>> = BTreeMap::new();
        for ((facet, collection_path), mut collection) in self.collections {
            collection.units.sort_by(|a, b| {
                (&a.unit_kind, &a.unit_key, &a.change).cmp(&(&b.unit_kind, &b.unit_key, &b.change))
            });
            let mut added = collection.coarse_added;
            let mut removed = collection.coarse_removed;
            let mut changed = collection.coarse_changed;
            let mut appended = collection.coarse_appended;
            let unit_values = collection
                .units
                .into_iter()
                .map(|unit| {
                    match unit.change.as_str() {
                        "added" => added += 1,
                        "removed" => removed += 1,
                        "changed" => changed += 1,
                        "appended" => appended += 1,
                        _ => changed += 1,
                    }
                    CborValue::Array(vec![
                        CborValue::Text(unit.unit_kind),
                        CborValue::Bytes(unit.unit_key),
                        CborValue::Text(unit.change),
                        opt_digest_value(unit.before),
                        opt_digest_value(unit.after),
                        CborValue::Text(unit.detail_kind),
                        unit.detail,
                    ])
                })
                .collect();
            let section = CborValue::Array(vec![
                CborValue::Array(collection_path.into_iter().map(CborValue::Text).collect()),
                CborValue::Array(vec![
                    CborValue::Uint(added),
                    CborValue::Uint(removed),
                    CborValue::Uint(changed),
                    CborValue::Uint(appended),
                    CborValue::Bool(collection.coarse),
                ]),
                CborValue::Array(unit_values),
            ]);
            by_facet.entry(facet).or_default().push(section);
        }
        let facets = by_facet
            .into_iter()
            .map(|(facet, collections)| {
                CborValue::Array(vec![CborValue::Text(facet), CborValue::Array(collections)])
            })
            .collect();
        cbor::encode(&CborValue::Array(vec![
            CborValue::Text("LMDIFF".to_string()),
            CborValue::Uint(1),
            CborValue::Bytes(ns.as_bytes().to_vec()),
            cbor::digest_value(&from),
            cbor::digest_value(&to),
            CborValue::Array(facets),
        ]))
    }
}

/// A workspace's working state relative to its `HEAD` tip and the shared staging index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// Index-vs-HEAD changes: what a `commit_staged` (`commit --staged`) would record.
    pub staged: Vec<Change>,
    /// Working-tree-vs-index changes to tracked paths (modified or deleted since they were staged).
    pub unstaged: Vec<Change>,
    /// Working-tree paths present in neither the index nor `HEAD`.
    pub untracked: Vec<String>,
    /// Unresolved paths of an in-progress merge, if any.
    pub conflicts: Vec<String>,
}

/// Path-level changes turning `from` into `to`: `Added` for a path only in `to`, `Deleted` for a path
/// only in `from`, `Modified` when both hold it with different slots. Sorted by path.
fn worktree_changes(from: &WorkTree, to: &WorkTree) -> Vec<Change> {
    let mut paths: BTreeSet<&String> = BTreeSet::new();
    paths.extend(from.keys());
    paths.extend(to.keys());
    let mut out = Vec::new();
    for path in paths {
        let kind = match (from.get(path), to.get(path)) {
            (None, Some(_)) => ChangeKind::Added,
            (Some(_), None) => ChangeKind::Deleted,
            (Some(a), Some(b)) if a != b => ChangeKind::Modified,
            _ => continue,
        };
        out.push(Change {
            path: path.clone(),
            kind,
        });
    }
    out
}

/// The result of a [`Loom::merge`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// The source is already contained in the current branch; nothing to do.
    UpToDate,
    /// The current branch fast-forwarded to the given commit (no merge commit).
    FastForward(Digest),
    /// A merge commit (two parents) was created at the given digest.
    Merged(Digest),
    /// The merge stopped with unresolved conflicts at these paths and entered an in-progress merge
    /// state (see [`Loom::merge_continue`] / [`Loom::merge_abort`]); no commit was made.
    Conflicts(Vec<String>),
}

/// The result of a history-replay op ([`Loom::cherry_pick`], [`Loom::revert`], [`Loom::rebase`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayOutcome {
    /// The replay applied cleanly; the branch tip advanced to this commit.
    Replayed(Digest),
    /// A dry run found no conflicts and made no change (no commit was created).
    Clean,
    /// The replay stopped at the first conflicting step with these unresolved paths and made no change
    /// (atomic: the branch tip and working tree are untouched). Preview with a dry run, resolve by
    /// adjusting the branch, then replay again.
    Conflicts(Vec<String>),
    /// There was nothing to replay (an empty commit list, or a rebase already based on the target).
    Empty,
}

/// How [`Loom::merge_resolve`] settles one conflicted path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    /// Keep the current branch's slot (the value at the merge's `ours` tip).
    Ours,
    /// Take the merged branch's slot (the value at the merge's `theirs` tip).
    Theirs,
    /// Accept whatever is currently staged at the path in the working tree (a hand-merged file, or
    /// edited conflict markers, or a deletion).
    Working,
}

impl ConflictResolution {
    /// The stable one-byte tag for this resolution (`Ours=0`, `Theirs=1`, `Working=2`), the shared
    /// numeric contract used by the C ABI and the API/wire codecs.
    pub const fn stable_tag(self) -> u8 {
        match self {
            ConflictResolution::Ours => 0,
            ConflictResolution::Theirs => 1,
            ConflictResolution::Working => 2,
        }
    }

    /// The resolution for a stable tag, or `None` for an unknown tag.
    pub const fn from_stable_tag(tag: u8) -> Option<Self> {
        Some(match tag {
            0 => ConflictResolution::Ours,
            1 => ConflictResolution::Theirs,
            2 => ConflictResolution::Working,
            _ => return None,
        })
    }
}

/// How a file handle ([`Loom::file_open`]) was opened. The modes mirror POSIX `open(2)`: a handle
/// binds to an *inode* (the open file's bytes), not to the path, so it keeps working after the path is
/// renamed or unlinked, and two handles opened on the same path share one inode (each with its own
/// offset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    /// Read-only. The file must already exist (`NOT_FOUND` otherwise). Writes are rejected.
    Read,
    /// Write-only, create-if-missing, truncated to empty on open (`O_WRONLY|O_CREAT|O_TRUNC`). The
    /// truncation applies to the shared inode, so any other open handle sees it. Reads are rejected.
    Write,
    /// Read-write, create-if-missing, content preserved (`O_RDWR|O_CREAT`, no truncation).
    ReadWrite,
    /// Append, create-if-missing. Every sequential write goes to the current end of file
    /// (`O_APPEND`), regardless of the handle offset. Reads are allowed.
    Append,
}

impl OpenMode {
    /// Whether this mode permits sequential/positional reads.
    fn can_read(self) -> bool {
        !matches!(self, OpenMode::Write)
    }
    /// Whether this mode permits writes.
    fn can_write(self) -> bool {
        !matches!(self, OpenMode::Read)
    }
    /// Whether opening in this mode creates the file when it is missing.
    fn creates(self) -> bool {
        !matches!(self, OpenMode::Read)
    }
    /// The stable 1-byte engine-state encoding.
    fn to_u8(self) -> u8 {
        match self {
            OpenMode::Read => 0,
            OpenMode::Write => 1,
            OpenMode::ReadWrite => 2,
            OpenMode::Append => 3,
        }
    }
    /// Decode the engine-state byte.
    fn from_u8(b: u8) -> Result<Self> {
        Ok(match b {
            0 => OpenMode::Read,
            1 => OpenMode::Write,
            2 => OpenMode::ReadWrite,
            3 => OpenMode::Append,
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown open-mode tag {other:#x}"
                )));
            }
        })
    }

    /// The canonical 1-byte API/wire tag for this mode: 0 Read, 1 Write, 2 ReadWrite, 3 Append. This
    /// is the byte carried in the generated `FileHandle.open` `mode` argument.
    pub fn to_wire_tag(self) -> u8 {
        self.to_u8()
    }

    /// Decode the canonical 1-byte API/wire tag (see [`OpenMode::to_wire_tag`]). An unknown tag is
    /// `INVALID_ARGUMENT` (caller-supplied input), unlike the engine-state decode.
    pub fn from_wire_tag(tag: u8) -> Result<Self> {
        Self::from_u8(tag)
            .map_err(|_| LoomError::invalid(format!("unknown open-mode tag {tag:#x}")))
    }
}

/// Metadata of an open file handle ([`Loom::file_stat`]): the live byte length and POSIX mode of the
/// inode the handle refers to. A handle has no path of its own (the inode may be unlinked), so unlike
/// [`crate::fs::Stat`] this carries no path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStat {
    /// Live byte length of the open file.
    pub size: u64,
    /// POSIX-style mode bits.
    pub mode: u32,
}

/// An open file description: the inode (shared bytes) plus per-handle cursor and mode. Multiple handles
/// on the same path share one inode but each keep their own [`OpenHandle::offset`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenHandle {
    pub(crate) inode: u64,
    pub(crate) offset: u64,
    pub(crate) mode: OpenMode,
}

/// A live inode: the bytes an open file currently holds, shared by every handle on it. While
/// [`Inode::path`] is `Some` the inode is *linked* and its bytes mirror that working-tree path; once
/// unlinked (`None`) the bytes survive only until the last handle closes (delete-on-last-close), and
/// the path is never resurrected by writes through surviving handles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Inode {
    pub(crate) ns: WorkspaceId,
    pub(crate) path: Option<String>,
    pub(crate) content_addr: Digest,
    pub(crate) size: u64,
    pub(crate) mode: u32,
    pub(crate) open_count: u32,
}

/// One unresolved path in an in-progress merge: the base, `ours`, and `theirs` slots, each absent when
/// the path did not exist on that side. The slots are the structured source of truth a merge tool
/// resolves through; conflict markers in the working tree are only a text-file presentation layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MergeConflict {
    pub(crate) path: String,
    pub(crate) base: Option<StagedEntry>,
    pub(crate) ours: Option<StagedEntry>,
    pub(crate) theirs: Option<StagedEntry>,
}

/// Persisted state of an in-progress (conflicted) merge for one workspace. Operational metadata kept
/// with the local engine state only: it is never part of commits, reachability, clone, push, bundle, or
/// ordinary sync. It records the second parent, the `ours` tip the final commit must fast-forward from,
/// the merge message, the unresolved conflicts, and a pre-merge snapshot of the working tree so
/// [`Loom::merge_abort`] can restore it exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MergeInProgress {
    pub(crate) other_parent: Digest,
    pub(crate) our_head: Digest,
    pub(crate) message: String,
    pub(crate) conflicts: Vec<MergeConflict>,
    pub(crate) pre_work: WorkTree,
    pub(crate) pre_dirs: BTreeSet<String>,
}

/// Durable policy attached to one exact VCS ref such as `branch/main` or `tag/v1`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProtectedRefPolicy {
    /// The ref may only move from an ancestor to a descendant.
    pub fast_forward_only: bool,
    /// Every commit being published must carry an accepted Loom commit signature.
    pub signed_commits_required: bool,
    /// The ref-advance operation itself must carry an accepted Loom signature.
    pub signed_ref_advance_required: bool,
    /// Minimum accepted reviews for the ref advance.
    pub required_review_count: u32,
    /// Retention policy blocks deletion or destructive rewrites of this ref.
    pub retention_lock: bool,
    /// Governance policy blocks deletion or destructive rewrites of this ref.
    pub governance_lock: bool,
}

/// The version-control engine handle.
#[derive(Debug)]
pub struct Loom<S: ObjectStore> {
    store: S,
    registry: Registry,
    lazy_state_sections: Option<Vec<TreeEntry>>,
    /// content address (whole-content BLAKE3) -> Blob object address.
    content: BTreeMap<Digest, Digest>,
    /// Per-workspace working tree. Accessed by the `fs` facade (`crate::fs`).
    pub(crate) work: BTreeMap<WorkspaceId, WorkTree>,
    /// Per-workspace set of directories that exist. Directories are first-class, not implied: an
    /// empty directory is tracked here and persists across commit/checkout/sync.
    pub(crate) dirs: BTreeMap<WorkspaceId, BTreeSet<String>>,
    /// Per-workspace compression-hint overrides; absent means the facet-derived default. A write
    /// policy passed to the store as a hint; it never affects object identity.
    compression: BTreeMap<WorkspaceId, CompressionHint>,
    /// Ingested ChunkList object digests whose whole-content address could not yet be rebuilt because
    /// some chunk Blob had not arrived; retried as chunks are ingested. Transient transfer state.
    pending_chunklists: BTreeSet<Digest>,
    /// Queue consumer offsets keyed by `(workspace, stream, consumer_id)`, holding each consumer's next
    /// sequence to read. Operational metadata persisted with the local engine state only; never part of
    /// commits, stream roots, reachability, clone, push, bundle, or ordinary sync.
    consumer_offsets: BTreeMap<(WorkspaceId, String, String), u64>,
    /// Queue retained low-water marks keyed by `(workspace, stream)`. A consumer cursor below the mark
    /// fails with `RETAINED_GAP` instead of silently skipping pruned history.
    stream_low_water_marks: BTreeMap<(WorkspaceId, String), u64>,
    /// In-progress (conflicted) merge state, keyed by workspace. Operational metadata persisted with
    /// the local engine state only; never part of commits, reachability, clone, push, bundle, or
    /// ordinary sync. Present for a workspace iff a merge stopped with unresolved conflicts.
    merge_state: BTreeMap<WorkspaceId, MergeInProgress>,
    /// The staging index, keyed by workspace: one shared stage across all of a workspace's facets. A
    /// `commit` snapshots the whole working tree (staged plus unstaged); a `commit_staged` snapshots
    /// only this index. `stage`/`unstage` move entries between the working tree and the index, and a
    /// clean commit or checkout resets the index to the committed tree. Operational metadata persisted
    /// with the local engine state.
    index: BTreeMap<WorkspaceId, WorkTree>,
    /// Live inodes for open file handles, keyed by inode id. The bytes an open file currently holds,
    /// shared across every handle on it. Operational metadata persisted with the local engine state
    /// only (so a handle stays valid across the stateless per-op reopen until `file_close`); never part
    /// of commits, reachability for transfer, clone, push, bundle, or ordinary sync. The objects a live
    /// inode references are kept alive by [`Loom::live_object_set`] while it is open.
    inodes: BTreeMap<u64, Inode>,
    /// Open file handles, keyed by handle id; each is an open file description (inode + cursor + mode).
    /// Operational metadata persisted with the local engine state only.
    handles: BTreeMap<u64, OpenHandle>,
    /// Reverse map from a linked open file's `(workspace, path)` to its inode id, so a second
    /// `file_open` of the same path shares the one inode. Derived from `inodes` (rebuilt on import),
    /// not separately persisted. Holds only linked inodes; an unlinked inode is absent here.
    path_to_inode: BTreeMap<(WorkspaceId, String), u64>,
    /// Monotonic inode-id allocator (persisted, so ids never collide across reopens).
    next_inode: u64,
    /// Monotonic handle-id allocator (persisted).
    next_handle: u64,
    /// Runtime-only ephemeral KV entries. The durable KV map tier config lives in a committed reserved
    /// file (so it versions and syncs); only the cache entries are coordinator-local runtime state.
    ephemeral_kv: BTreeMap<(WorkspaceId, String), EphemeralKvMap>,
    /// Durable protected-ref policy records keyed by `(workspace, "branch/name" | "tag/name")`.
    protected_refs: BTreeMap<(WorkspaceId, String), ProtectedRefPolicy>,
    identity: Option<IdentityStore>,
    acl: AclStore,
    predicate_evaluator: Option<Arc<dyn AclPredicateEvaluator>>,
    session: Option<String>,
}

mod access;
mod columnar;
mod commit;
mod diff;
mod files;
mod handles;
mod kv_config;
mod merge;
mod objects;
pub use objects::{LiveRootClassDiagnostics, LiveRootDiagnostics, LiveRootExample};
mod protected_refs;
mod replay;
mod state;
mod streams;
mod timeseries;

pub use objects::{ReachabilityMarkState, ReachabilityMarkStep};

// ---- engine-state codec helpers (for export_state/import_state) ---------------------------------

fn put_uvarint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            out.push(byte | 0x80);
        } else {
            out.push(byte);
            break;
        }
    }
}
fn put_lp(out: &mut Vec<u8>, bytes: &[u8]) {
    put_uvarint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

/// Encode one staged slot with the durable tag scheme, matching the working-tree encoding in
/// [`Loom::export_state`].
fn put_slot(out: &mut Vec<u8>, slot: &StagedEntry) {
    match slot {
        StagedEntry::File(f) => {
            out.push(0);
            out.extend_from_slice(f.content_addr.bytes());
            put_uvarint(out, u64::from(f.mode));
        }
        StagedEntry::Table(tree) => {
            out.push(1);
            out.extend_from_slice(tree.bytes());
        }
        StagedEntry::Stream(tree) => {
            out.push(2);
            out.extend_from_slice(tree.bytes());
        }
        StagedEntry::TimeSeries(tree) => {
            out.push(3);
            out.extend_from_slice(tree.bytes());
        }
        StagedEntry::Graph(tree) => {
            out.push(4);
            out.extend_from_slice(tree.bytes());
        }
        StagedEntry::Ledger(tree) => {
            out.push(5);
            out.extend_from_slice(tree.bytes());
        }
        StagedEntry::Columnar(tree) => {
            out.push(6);
            out.extend_from_slice(tree.bytes());
        }
        StagedEntry::Document(tree) => {
            out.push(7);
            out.extend_from_slice(tree.bytes());
        }
    }
}

/// Encode an optional staged slot: `0` for absent, `1` then the slot for present.
fn put_opt_slot(out: &mut Vec<u8>, slot: &Option<StagedEntry>) {
    match slot {
        None => out.push(0),
        Some(s) => {
            out.push(1);
            put_slot(out, s);
        }
    }
}

struct StateCur<'a> {
    buf: &'a [u8],
    pos: usize,
}
impl<'a> StateCur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|e| *e <= self.buf.len())
            .ok_or_else(|| LoomError::corrupt("engine-state bytes truncated"))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn bool(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(LoomError::corrupt("engine-state bool tag")),
        }
    }
    fn take16(&mut self) -> Result<[u8; 16]> {
        let mut a = [0u8; 16];
        a.copy_from_slice(self.take(16)?);
        Ok(a)
    }
    fn take32(&mut self) -> Result<[u8; 32]> {
        let mut a = [0u8; 32];
        a.copy_from_slice(self.take(32)?);
        Ok(a)
    }
    fn lp_str(&mut self) -> Result<String> {
        let n = self.uvarint()? as usize;
        let raw = self.take(n)?;
        String::from_utf8(raw.to_vec())
            .map_err(|_| LoomError::corrupt("non-utf8 engine-state string"))
    }
    /// Decode one staged slot written by [`put_slot`], tagging digests with the store's identity profile.
    fn slot(&mut self, algo: crate::Algo) -> Result<StagedEntry> {
        Ok(match self.u8()? {
            0 => {
                let content_addr = Digest::of(algo, self.take32()?);
                let mode = self.uvarint()? as u32;
                StagedEntry::File(StagedFile { content_addr, mode })
            }
            1 => StagedEntry::Table(Digest::of(algo, self.take32()?)),
            2 => StagedEntry::Stream(Digest::of(algo, self.take32()?)),
            3 => StagedEntry::TimeSeries(Digest::of(algo, self.take32()?)),
            4 => StagedEntry::Graph(Digest::of(algo, self.take32()?)),
            5 => StagedEntry::Ledger(Digest::of(algo, self.take32()?)),
            6 => StagedEntry::Columnar(Digest::of(algo, self.take32()?)),
            7 => StagedEntry::Document(Digest::of(algo, self.take32()?)),
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown staged-slot tag {other:#x}"
                )));
            }
        })
    }
    /// Decode an optional staged slot written by [`put_opt_slot`].
    fn opt_slot(&mut self, algo: crate::Algo) -> Result<Option<StagedEntry>> {
        Ok(if self.u8()? == 0 {
            None
        } else {
            Some(self.slot(algo)?)
        })
    }
    fn uvarint(&mut self) -> Result<u64> {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = *self
                .buf
                .get(self.pos)
                .ok_or_else(|| LoomError::corrupt("engine-state uvarint truncated"))?;
            self.pos += 1;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift >= 64 {
                return Err(LoomError::corrupt("engine-state uvarint too long"));
            }
        }
        Ok(v)
    }
}

/// A directory with more than this many entries is stored as a prolly-sharded tree of `Tree`-object
/// shard nodes instead of a single flat [`Object::Tree`]. The threshold, like the rolling-hash
/// parameters, is a fixed protocol constant, so two peers shard the same entry set identically and
/// converge on the same root.
pub(crate) const DIR_SHARD_THRESHOLD: usize = 256;

/// Maximum prolly depth a directory walk descends before declaring the tree corrupt; guards against
/// a crafted or cyclic shard graph.
const MAX_SHARD_DEPTH: usize = 64;

/// Version byte of the structured stream metadata record.
const STREAM_META_VERSION: u64 = 1;
/// Version byte of a structured stream entry record.
const STREAM_ENTRY_VERSION: u64 = 1;

/// The working-tree path of the stream named `name` (its queue-facet path).
fn stream_facet_path(name: &str) -> String {
    crate::workspace::facet_path(crate::workspace::FacetKind::Queue, name)
}

fn validate_queue_stream_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_control)
    {
        return Err(LoomError::invalid(format!("invalid stream name {name:?}")));
    }
    normalize_path(&stream_facet_path(name))?;
    Ok(())
}

/// Reject empty consumer ids, or ids carrying a separator or control character.
fn validate_consumer_id(id: &str) -> Result<()> {
    if id.is_empty() || id.contains('/') || id.chars().any(char::is_control) {
        return Err(LoomError::invalid(format!("invalid consumer id {id:?}")));
    }
    Ok(())
}

/// Canonical bytes of one entry record: `[1, payload_digest, payload_len]`.
fn encode_stream_record(payload_addr: Digest, payload_len: u64) -> Vec<u8> {
    crate::cbor::encode(&crate::cbor::Value::Array(vec![
        crate::cbor::Value::Uint(STREAM_ENTRY_VERSION),
        crate::cbor::digest_value(&payload_addr),
        crate::cbor::Value::Uint(payload_len),
    ]))
}

/// Stable on-the-wire byte for a compression hint (engine-state encoding).
fn hint_to_u8(hint: CompressionHint) -> u8 {
    match hint {
        CompressionHint::None => 0,
        CompressionHint::Fast => 1,
        CompressionHint::Small => 2,
    }
}
fn hint_from_u8(b: u8) -> CompressionHint {
    match b {
        0 => CompressionHint::None,
        1 => CompressionHint::Fast,
        _ => CompressionHint::Small,
    }
}

/// Parse a textual revision into a digest: either an `algo:hex` form (e.g. `blake3:...`) or a bare
/// 64-character lowercase-hex string tagged with the store's profile `algo`. Returns `None` for anything
/// that is not digest-shaped, so the caller can fall through to branch-name resolution.
fn parse_rev_digest(algo: crate::Algo, rev: &str) -> Option<Digest> {
    if rev.contains(':') {
        return Digest::parse(rev).ok();
    }
    if rev.len() == 2 * crate::digest::DIGEST_LEN && rev.bytes().all(|b| b.is_ascii_hexdigit()) {
        let mut bytes = [0u8; crate::digest::DIGEST_LEN];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&rev[i * 2..i * 2 + 2], 16).ok()?;
        }
        return Some(Digest::of(algo, bytes));
    }
    None
}

impl DiffUnitChange {
    fn new(
        unit_kind: &str,
        unit_key: Vec<u8>,
        change: &str,
        before: Option<Digest>,
        after: Option<Digest>,
    ) -> Self {
        Self {
            unit_kind: unit_kind.to_string(),
            unit_key,
            change: change.to_string(),
            before,
            after,
            detail_kind: "none".to_string(),
            detail: CborValue::Null,
        }
    }
}

enum DiffPath {
    Ignored,
    File {
        collection: Vec<String>,
    },
    SqlTable {
        db: String,
        table: String,
    },
    QueueStream {
        stream: String,
    },
    KvCollection {
        collection: String,
    },
    DocumentCollection {
        collection: String,
    },
    SearchCollection {
        collection: String,
    },
    CasDigest {
        digest: Digest,
    },
    Calendar {
        principal: String,
        collection: String,
        uid: String,
    },
    Contacts {
        principal: String,
        book: String,
        uid: String,
    },
    Mail {
        principal: String,
        mailbox: String,
        unit_kind: &'static str,
        uid: String,
    },
    VectorEntry {
        set: Vec<String>,
        id: String,
    },
    CoarseFacet {
        facet: String,
        collection: Vec<String>,
    },
}

impl DiffPath {
    fn facet_kind(&self) -> Option<FacetKind> {
        match self {
            DiffPath::Ignored => None,
            DiffPath::File { .. } => Some(FacetKind::Files),
            DiffPath::SqlTable { .. } => Some(FacetKind::Sql),
            DiffPath::QueueStream { .. } => Some(FacetKind::Queue),
            DiffPath::KvCollection { .. } => Some(FacetKind::Kv),
            DiffPath::DocumentCollection { .. } => Some(FacetKind::Document),
            DiffPath::SearchCollection { .. } => Some(FacetKind::Search),
            DiffPath::CasDigest { .. } => Some(FacetKind::Cas),
            DiffPath::Calendar { .. } => Some(FacetKind::Calendar),
            DiffPath::Contacts { .. } => Some(FacetKind::Contacts),
            DiffPath::Mail { .. } => Some(FacetKind::Mail),
            DiffPath::VectorEntry { .. } => Some(FacetKind::Vector),
            DiffPath::CoarseFacet { facet, .. } => FacetKind::parse(facet).ok(),
        }
    }
}

fn opt_digest_value(digest: Option<Digest>) -> CborValue {
    digest
        .as_ref()
        .map(cbor::digest_value)
        .unwrap_or(CborValue::Null)
}

fn key_text(text: &str) -> Vec<u8> {
    cbor::encode(&CborValue::Text(text.to_string()))
}

fn key_bytes(bytes: &[u8]) -> Vec<u8> {
    cbor::encode(&CborValue::Bytes(bytes.to_vec()))
}

fn key_uint(n: u64) -> Vec<u8> {
    cbor::encode(&CborValue::Uint(n))
}

fn key_digest(digest: Digest) -> Vec<u8> {
    cbor::encode(&cbor::digest_value(&digest))
}

fn vector_id_from_path_segment(segment: &str) -> Option<String> {
    let bytes = hex::decode(segment).ok()?;
    cbor::as_text(cbor::decode(&bytes).ok()?).ok()
}

fn document_collection_key(collection: &str) -> String {
    hex::encode(collection.as_bytes())
}

fn document_map_diff_path(collection: &str) -> String {
    crate::workspace::facet_path(
        crate::workspace::FacetKind::Document,
        &format!(".maps/{}", document_collection_key(collection)),
    )
}

fn document_body_diff_path(collection: &str, digest: &Digest) -> String {
    crate::workspace::facet_path(
        crate::workspace::FacetKind::Document,
        &format!(
            ".bodies/{}/{}",
            document_collection_key(collection),
            digest.to_hex()
        ),
    )
}

fn row_key(schema: &tabular::Schema, row: &Row) -> Vec<u8> {
    let key = schema
        .primary_key
        .iter()
        .map(|&i| row[i].clone())
        .collect::<Row>();
    tabular::encode_row(&key)
}

fn entry_digest(entry: Option<&StagedEntry>) -> Option<Digest> {
    match entry {
        Some(StagedEntry::File(file)) => Some(file.content_addr),
        Some(
            StagedEntry::Table(root)
            | StagedEntry::Stream(root)
            | StagedEntry::TimeSeries(root)
            | StagedEntry::Graph(root)
            | StagedEntry::Ledger(root)
            | StagedEntry::Columnar(root)
            | StagedEntry::Document(root),
        ) => Some(*root),
        None => None,
    }
}

fn simple_change(lhs: Option<&StagedEntry>, rhs: Option<&StagedEntry>) -> &'static str {
    match (lhs, rhs) {
        (None, Some(_)) => "added",
        (Some(_), None) => "removed",
        (Some(_), Some(_)) => "changed",
        (None, None) => "changed",
    }
}

fn change_kind(before: Option<Digest>, after: Option<Digest>) -> &'static str {
    match (before, after) {
        (None, Some(_)) => "added",
        (Some(_), None) => "removed",
        (Some(_), Some(_)) => "changed",
        (None, None) => "changed",
    }
}

fn stream_seq_key(key: &[u8]) -> Result<u64> {
    let bytes: [u8; 8] = key
        .try_into()
        .map_err(|_| LoomError::corrupt("stream entry key is not u64"))?;
    Ok(u64::from_be_bytes(bytes))
}

fn classify_diff_path(path: &str, algo: crate::Algo) -> DiffPath {
    let collection = || {
        path.rsplit_once('/')
            .map(|(parent, _)| parent.split('/').map(str::to_string).collect())
            .unwrap_or_default()
    };
    let Some(rest) = path.strip_prefix(".loom/facets/") else {
        return DiffPath::File {
            collection: collection(),
        };
    };
    let segments = rest.split('/').collect::<Vec<_>>();
    match segments.as_slice() {
        ["sql", db, "tables", table] => DiffPath::SqlTable {
            db: (*db).to_string(),
            table: (*table).to_string(),
        },
        ["queue", stream] => DiffPath::QueueStream {
            stream: (*stream).to_string(),
        },
        ["kv", ".values", _, _] => DiffPath::Ignored,
        ["kv", collection] => DiffPath::KvCollection {
            collection: (*collection).to_string(),
        },
        ["document", ".maps", _] | ["document", ".bodies", _, _] => DiffPath::Ignored,
        ["document", collection] => DiffPath::DocumentCollection {
            collection: (*collection).to_string(),
        },
        ["search", ".documents", _, _] | ["search", ".aliases", _] => DiffPath::Ignored,
        ["search", collection] => DiffPath::SearchCollection {
            collection: (*collection).to_string(),
        },
        ["cas", hex] => {
            if let Some(digest) = parse_rev_digest(algo, hex) {
                DiffPath::CasDigest { digest }
            } else {
                DiffPath::CoarseFacet {
                    facet: "cas".to_string(),
                    collection: Vec::new(),
                }
            }
        }
        ["calendar", principal, collection, uid] => DiffPath::Calendar {
            principal: (*principal).to_string(),
            collection: (*collection).to_string(),
            uid: (*uid).to_string(),
        },
        ["contacts", principal, book, uid] => DiffPath::Contacts {
            principal: (*principal).to_string(),
            book: (*book).to_string(),
            uid: (*uid).to_string(),
        },
        ["mail", principal, mailbox, "msg", uid] => DiffPath::Mail {
            principal: (*principal).to_string(),
            mailbox: (*mailbox).to_string(),
            unit_kind: "message",
            uid: (*uid).to_string(),
        },
        ["mail", principal, mailbox, "flags", uid] => DiffPath::Mail {
            principal: (*principal).to_string(),
            mailbox: (*mailbox).to_string(),
            unit_kind: "flags",
            uid: (*uid).to_string(),
        },
        ["vector", set @ .., "entries", encoded_id] if !set.is_empty() => {
            if let Some(id) = vector_id_from_path_segment(encoded_id) {
                DiffPath::VectorEntry {
                    set: set.iter().map(|s| (*s).to_string()).collect(),
                    id,
                }
            } else {
                DiffPath::CoarseFacet {
                    facet: "vector".to_string(),
                    collection: set.iter().map(|s| (*s).to_string()).collect(),
                }
            }
        }
        [facet, collection @ ..] => DiffPath::CoarseFacet {
            facet: (*facet).to_string(),
            collection: collection.iter().map(|s| (*s).to_string()).collect(),
        },
        [] => DiffPath::File {
            collection: collection(),
        },
    }
}

/// POSIX file-type mask (`S_IFMT`): the high mode bits that select a slot's type.
pub(crate) const FILE_TYPE_MASK: u32 = 0o170000;
/// POSIX symbolic-link type bits (`S_IFLNK`). A symlink is a file slot whose mode carries these and
/// whose content is the (opaque) target path; this matches git's symlink representation.
pub(crate) const SYMLINK_MODE: u32 = 0o120000;

/// Whether a file mode denotes a symbolic link (its type bits are `S_IFLNK`).
pub(crate) fn is_symlink_mode(mode: u32) -> bool {
    mode & FILE_TYPE_MASK == SYMLINK_MODE
}

/// Normalize a path for the working tree: strip a leading `/`, reject empty / trailing-slash forms.
pub(crate) fn normalize_path(path: &str) -> Result<String> {
    let p = path.trim_start_matches('/');
    if p.is_empty() || p.ends_with('/') || p.contains("//") {
        return Err(LoomError::invalid(format!("invalid path {path:?}")));
    }
    Ok(p.to_string())
}

/// Reject a user write whose target (a normalized path) is in the reserved `.loom` subtree (0014a
/// baseline). The public `fs` mutators call this; the typed facet facades bypass it through the
/// `*_reserved` privileged variants, which write their own `.loom/facets/<facet>/...` storage.
pub(crate) fn guard_reserved_write(path: &str) -> Result<()> {
    if crate::workspace::is_reserved_path(path) {
        return Err(LoomError::new(
            Code::PermissionDenied,
            format!(
                "{path:?} is under the reserved {:?} subtree and is not user-writable",
                crate::workspace::LOOM_RESERVED_DIR
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
