//! Synchronization: the sync engine.
//!
//! Reconcile two Looms by transferring the immutable, content-addressed **objects** plus the
//! **ref updates** a destination lacks so it gains history a source holds. Because every object is
//! self-verifying under the store's identity profile, the hard parts are (a) discovering the minimal
//! object set to send - the "want" closure, [`Loom::reachable`] - and (b) advancing refs safely
//! (fast-forward / CAS).
//!
//! This module implements the engine-level, transport-agnostic core: direct Loom-to-Loom
//! [`clone_workspace`] and [`push_branch`], and the offline [`Bundle`] form ([`bundle_export`] /
//! [`bundle_import`]) - the basis of the canonical single-file to single-file exchange. Transport
//! adapters can wrap these primitives without changing object transfer semantics.
//!
//! Properties honored: integrity (every object re-verified on receipt by the store), minimality
//! (only objects the receiver lacks are sent), atomic ref advancement (a ref moves only after its
//! whole subgraph is present), and no partial corruption (a failed transfer leaves refs unmoved;
//! orphaned objects are GC-reclaimable).

use crate::acl::AclRight;
use crate::cbor::{self, Value};
use crate::digest::{Algo, Digest};
use crate::error::{Code, LoomError, Result};
use crate::lock::{LockCoordinator, LockMode, LockOwner};
use crate::object::{ChunkRef, Commit, EntryKind, Object, Tag, TreeEntry};
use crate::provider::ObjectStore;
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId};
use std::collections::{BTreeMap, BTreeSet};

/// Reject a sync (direct or bundle) when source and destination disagree on their identity profile:
/// object addresses are profile-specific, so a cross-profile transfer would relabel
/// every object. This is a loud protocol/profile mismatch, never a silent rehash; rehashing is a
/// separate, explicit migration path.
fn check_profiles(src: Algo, dst: Algo) -> Result<()> {
    if src == dst {
        Ok(())
    } else {
        Err(LoomError::new(
            Code::Conflict,
            format!(
                "identity-profile mismatch: source is {}, destination is {}; sync requires matching \
                 profiles (a cross-profile transfer is a separate explicit migration, never a silent \
                 rehash)",
                src.as_str(),
                dst.as_str()
            ),
        ))
    }
}

/// What a sync moved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Objects actually transferred (the receiver lacked them).
    pub objects_transferred: u64,
    /// Objects in the want-set the receiver already had (skipped).
    pub objects_skipped: u64,
    /// `(branch, tip)` refs advanced on the receiver.
    pub new_tips: Vec<(String, Digest)>,
}

/// What an explicit identity-profile migration copied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MigrationReport {
    /// Objects written to the destination profile.
    pub objects_written: u64,
    /// Content payloads written through the destination profile.
    pub content_written: u64,
    /// Raw prolly nodes written to the destination profile.
    pub prolly_nodes_written: u64,
    /// `(branch, tip)` refs created on the destination.
    pub new_tips: Vec<(String, Digest)>,
}

/// A self-contained, offline transfer set: a workspace's id/name/facets, its ref tips, and the
/// canonical object frames satisfying them - Loom's analogue of `git bundle`. The object stream is
/// order-independent on import because each object re-verifies against its own address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// The source Loom's identity profile (digest algorithm). Object addresses in the
    /// bundle are under this algorithm, so an import into a differently-profiled Loom is rejected.
    pub digest_algo: Algo,
    /// The source workspace id.
    pub ns_id: WorkspaceId,
    /// The source workspace facets.
    pub facets: Vec<FacetKind>,
    /// The source workspace name.
    pub ns_name: String,
    /// `(branch, tip)` pairs the bundle satisfies.
    pub branches: Vec<(String, Digest)>,
    /// `(tag, target)` pairs the bundle satisfies.
    pub tags: Vec<(String, Digest)>,
    /// Canonical object byte-frames (every object the refs reach).
    pub objects: Vec<Vec<u8>>,
}

impl Bundle {
    /// The self-describing leading marker (first element of the canonical frame).
    const MAGIC: &'static str = "LMBNDL";
    /// The bundle wire-format version. v4 carries the workspace id and full facet set.
    const VERSION: u64 = 4;

    /// Serialize to a self-describing byte stream for an offline `.bundle` file (the single-file to
    /// single-file exchange). The frame is one Loom Canonical CBOR array
    /// `[magic, version, digest_algo, ns_id, facets, ns_name, branches, tags, objects]`. Deterministic:
    /// refs and objects are emitted in the order the [`Bundle`] holds them ([`bundle_export`] sorts
    /// objects by digest).
    pub fn encode(&self) -> Vec<u8> {
        let objects = self
            .objects
            .iter()
            .map(|o| Value::Bytes(o.clone()))
            .collect();
        cbor::encode(&Value::Array(vec![
            Value::Text(Self::MAGIC.to_string()),
            Value::Uint(Self::VERSION),
            Value::Uint(u64::from(self.digest_algo.code())),
            workspace_id_value(self.ns_id),
            facets_value(&self.facets),
            Value::Text(self.ns_name.clone()),
            refs_value(&self.branches),
            refs_value(&self.tags),
            Value::Array(objects),
        ]))
    }

    /// Parse a [`Bundle`] from [`Bundle::encode`] output.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut f = cbor::Fields::new(cbor::decode_array(bytes)?);
        if f.text()? != Self::MAGIC {
            return Err(LoomError::corrupt("not a loom bundle (bad magic)"));
        }
        let version = f.uint()?;
        let digest_algo = match version {
            4 => Algo::from_code(
                u8::try_from(f.uint()?)
                    .map_err(|_| LoomError::corrupt("bundle digest-algo code out of range"))?,
            )?,
            _ => return Err(LoomError::corrupt("unsupported loom bundle version")),
        };
        let ns_id = workspace_id_from(f.next_field()?)?;
        let facets = facets_from(f.array()?)?;
        let ns_name = f.text()?;
        let branches = refs_from(f.array()?)?;
        let tags = refs_from(f.array()?)?;
        let objects = f
            .array()?
            .into_iter()
            .map(cbor::as_bytes)
            .collect::<Result<Vec<_>>>()?;
        f.end()?;
        Ok(Bundle {
            digest_algo,
            ns_id,
            facets,
            ns_name,
            branches,
            tags,
            objects,
        })
    }
}

/// Copy the canonical bytes of every digest in `need` from `src` into `dst`, skipping any `dst`
/// already holds. Each ingested object is re-verified by the store.
fn transfer<S: ObjectStore, T: ObjectStore>(
    src: &Loom<S>,
    dst: &mut Loom<T>,
    need: &BTreeSet<Digest>,
) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    for &d in need {
        if dst.has_object(d)? {
            report.objects_skipped += 1;
            continue;
        }
        let bytes = src.object_bytes(d)?;
        let got = dst.ingest_object(&bytes)?;
        debug_assert_eq!(got, d, "ingested object re-addressed differently");
        report.objects_transferred += 1;
    }
    Ok(report)
}

/// All `(branch, tip)` pairs of a workspace that have a commit.
fn branch_tips<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId) -> Result<Vec<(String, Digest)>> {
    let mut out = Vec::new();
    for b in loom.registry().branch_list(ns)? {
        if let Some(tip) = loom.registry().branch_tip(ns, &b)? {
            out.push((b, tip));
        }
    }
    Ok(out)
}

/// All `(tag, target)` pairs of a workspace.
fn tag_targets<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId) -> Result<Vec<(String, Digest)>> {
    let mut out = Vec::new();
    for t in loom.registry().tag_list(ns)? {
        if let Some(target) = loom.registry().tag_target(ns, &t)? {
            out.push((t, target));
        }
    }
    Ok(out)
}

/// Clone one workspace from `src` into `dst` as a fresh workspace with id `new_id`: transfer every
/// object its branches and tags reach, then recreate those refs. `dst` must not already hold a
/// workspace of the same name. The clone is **bare** (no working tree is checked out);
/// the caller may `checkout_commit` a branch tip afterwards. Returns the new id and a report.
pub fn clone_workspace<S: ObjectStore, T: ObjectStore>(
    src: &Loom<S>,
    src_ns: WorkspaceId,
    dst: &mut Loom<T>,
    new_id: WorkspaceId,
) -> Result<(WorkspaceId, SyncReport)> {
    src.authorize_workspace_facets(src_ns, AclRight::Read)?;
    dst.authorize_global_admin()?;
    check_profiles(src.store().digest_algo(), dst.store().digest_algo())?;
    let name = src.registry().name(src_ns)?;
    let facets = src.registry().facets(src_ns)?;
    let branches = branch_tips(src, src_ns)?;
    let tags = tag_targets(src, src_ns)?;

    let tips: Vec<Digest> = branches
        .iter()
        .chain(tags.iter())
        .map(|(_, d)| *d)
        .collect();
    let need = src.reachable(&tips, &BTreeSet::new())?;
    let mut report = transfer(src, dst, &need)?;

    let dst_ns = dst.registry_mut().create_workspace(Some(&name), new_id)?;
    for facet in facets {
        dst.registry_mut().add_facet(dst_ns, facet)?;
    }
    for (b, tip) in &branches {
        dst.registry_mut().update_branch(dst_ns, b, None, *tip)?;
        report.new_tips.push((b.clone(), *tip));
    }
    for (t, target) in &tags {
        dst.registry_mut().tag_create(dst_ns, t, *target)?;
    }
    Ok((dst_ns, report))
}

/// Copy one workspace into `dst` while rewriting content and object labels under the destination
/// store's identity profile. This is the explicit migration path sync refuses to perform silently.
pub fn migrate_workspace_profile<S: ObjectStore, T: ObjectStore>(
    src: &Loom<S>,
    src_ns: WorkspaceId,
    dst: &mut Loom<T>,
) -> Result<(WorkspaceId, MigrationReport)> {
    src.authorize_workspace_facets(src_ns, AclRight::Read)?;
    dst.authorize_global_admin()?;
    let name = src.registry().name(src_ns)?;
    let facets = src.registry().facets(src_ns)?;
    let branches = branch_tips(src, src_ns)?;
    let tags = tag_targets(src, src_ns)?;
    let dst_ns = dst.registry_mut().create_workspace(Some(&name), src_ns)?;
    for facet in facets {
        dst.registry_mut().add_facet(dst_ns, facet)?;
    }
    let mut ctx = MigrationCtx {
        src,
        dst,
        dst_ns,
        objects: BTreeMap::new(),
        content: BTreeMap::new(),
        prolly: BTreeMap::new(),
        report: MigrationReport::default(),
    };
    for (branch, tip) in branches {
        let new_tip = ctx.migrate_object(tip, TreeMode::Normal)?;
        ctx.dst
            .registry_mut()
            .update_branch(dst_ns, &branch, None, new_tip)?;
        ctx.report.new_tips.push((branch, new_tip));
    }
    for (tag, target) in tags {
        let new_target = ctx.migrate_object(target, TreeMode::Normal)?;
        ctx.dst
            .registry_mut()
            .tag_create(dst_ns, &tag, new_target)?;
    }
    let head = ctx.src.registry().head_branch(src_ns)?;
    ctx.dst.registry_mut().set_head(dst_ns, &head)?;
    if let Some(tip) = ctx.dst.registry().branch_tip(dst_ns, &head)? {
        ctx.dst.checkout_commit(dst_ns, tip)?;
    }
    Ok((dst_ns, ctx.report))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeMode {
    Normal,
    StreamRoot,
}

struct MigrationCtx<'a, S: ObjectStore, T: ObjectStore> {
    src: &'a Loom<S>,
    dst: &'a mut Loom<T>,
    dst_ns: WorkspaceId,
    objects: BTreeMap<Digest, Digest>,
    content: BTreeMap<Digest, Digest>,
    prolly: BTreeMap<Digest, Digest>,
    report: MigrationReport,
}

impl<S: ObjectStore, T: ObjectStore> MigrationCtx<'_, S, T> {
    fn migrate_object(&mut self, digest: Digest, mode: TreeMode) -> Result<Digest> {
        if let Some(mapped) = self.objects.get(&digest) {
            return Ok(*mapped);
        }
        let bytes = self.src.object_bytes(digest)?;
        let object = Object::decode(&bytes)?;
        let migrated = match object {
            Object::Blob(bytes) => Object::Blob(bytes),
            Object::ChunkList {
                total_size,
                entries,
            } => {
                let entries = entries
                    .into_iter()
                    .map(|entry| {
                        Ok(ChunkRef {
                            target: self.migrate_object(entry.target, TreeMode::Normal)?,
                            size: entry.size,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Object::ChunkList {
                    total_size,
                    entries,
                }
            }
            Object::Tree(entries) => {
                let entries = match mode {
                    TreeMode::Normal => self.migrate_tree_entries(entries)?,
                    TreeMode::StreamRoot => self.migrate_stream_root_entries(entries)?,
                };
                Object::tree(entries)?
            }
            Object::Commit(commit) => Object::Commit(Commit {
                tree: self.migrate_object(commit.tree, TreeMode::Normal)?,
                parents: commit
                    .parents
                    .into_iter()
                    .map(|parent| self.migrate_object(parent, TreeMode::Normal))
                    .collect::<Result<Vec<_>>>()?,
                author: commit.author,
                timestamp_ms: commit.timestamp_ms,
                message: commit.message,
                meta: commit.meta,
            }),
            Object::Tag(tag) => Object::Tag(Tag {
                target: self.migrate_object(tag.target, TreeMode::Normal)?,
                target_type: tag.target_type,
                name: tag.name,
                tagger: tag.tagger,
                timestamp_ms: tag.timestamp_ms,
                message: tag.message,
            }),
        };
        let new_digest = self.dst.ingest_object(&migrated.canonical())?;
        self.objects.insert(digest, new_digest);
        self.report.objects_written += 1;
        Ok(new_digest)
    }

    fn migrate_tree_entries(&mut self, entries: Vec<TreeEntry>) -> Result<Vec<TreeEntry>> {
        entries
            .into_iter()
            .map(|entry| {
                let target = match entry.kind {
                    EntryKind::Blob | EntryKind::Symlink => self.migrate_content(entry.target)?,
                    EntryKind::Tree | EntryKind::TreeShard | EntryKind::Subloom => {
                        self.migrate_object(entry.target, TreeMode::Normal)?
                    }
                    EntryKind::Table
                    | EntryKind::TimeSeries
                    | EntryKind::Graph
                    | EntryKind::Ledger
                    | EntryKind::Columnar
                    | EntryKind::Document => self.migrate_object(entry.target, TreeMode::Normal)?,
                    EntryKind::ProllyMap => self.migrate_prolly(entry.target, LeafMode::Raw)?,
                    EntryKind::Stream => self.migrate_object(entry.target, TreeMode::StreamRoot)?,
                };
                Ok(TreeEntry { target, ..entry })
            })
            .collect()
    }

    fn migrate_stream_root_entries(&mut self, entries: Vec<TreeEntry>) -> Result<Vec<TreeEntry>> {
        let entries_root = entries
            .iter()
            .find(|entry| entry.name == "entries")
            .map(|entry| self.migrate_prolly(entry.target, LeafMode::StreamRecord))
            .transpose()?;
        let consumers_root = entries
            .iter()
            .find(|entry| entry.name == "consumers")
            .map(|entry| self.migrate_prolly(entry.target, LeafMode::Raw))
            .transpose()?;
        entries
            .into_iter()
            .map(|entry| {
                let target = match (entry.name.as_str(), entry.kind) {
                    ("meta", EntryKind::Blob) => {
                        self.migrate_stream_meta(entry.target, entries_root, consumers_root)?
                    }
                    ("entries", EntryKind::ProllyMap) => entries_root.ok_or_else(|| {
                        LoomError::corrupt("stream entries root missing after migration")
                    })?,
                    ("consumers", EntryKind::ProllyMap) => consumers_root.ok_or_else(|| {
                        LoomError::corrupt("stream consumers root missing after migration")
                    })?,
                    _ => match entry.kind {
                        EntryKind::Blob | EntryKind::Symlink => {
                            self.migrate_content(entry.target)?
                        }
                        EntryKind::Tree | EntryKind::TreeShard | EntryKind::Subloom => {
                            self.migrate_object(entry.target, TreeMode::Normal)?
                        }
                        EntryKind::Table
                        | EntryKind::TimeSeries
                        | EntryKind::Graph
                        | EntryKind::Ledger
                        | EntryKind::Columnar
                        | EntryKind::Document => {
                            self.migrate_object(entry.target, TreeMode::Normal)?
                        }
                        EntryKind::ProllyMap => self.migrate_prolly(entry.target, LeafMode::Raw)?,
                        EntryKind::Stream => {
                            self.migrate_object(entry.target, TreeMode::StreamRoot)?
                        }
                    },
                };
                Ok(TreeEntry { target, ..entry })
            })
            .collect()
    }

    fn migrate_stream_meta(
        &mut self,
        old_addr: Digest,
        entries_root: Option<Digest>,
        consumers_root: Option<Digest>,
    ) -> Result<Digest> {
        let bytes = self.src.load_content(old_addr)?;
        let mut f = cbor::Fields::new(cbor::decode_array(&bytes)?);
        let version = f.uint()?;
        if version != 1 {
            return Err(LoomError::corrupt("unsupported stream metadata version"));
        }
        let length = f.uint()?;
        let _old_entries = f.next_field()?;
        let _old_consumers = f.next_field()?;
        f.end()?;
        let entries_value = entries_root.map_or(cbor::Value::Null, |d| cbor::digest_value(&d));
        let consumers_value = consumers_root.map_or(cbor::Value::Null, |d| cbor::digest_value(&d));
        let migrated = cbor::encode(&cbor::Value::Array(vec![
            cbor::Value::Uint(1),
            cbor::Value::Uint(length),
            entries_value,
            consumers_value,
        ]));
        let new_addr = self.dst.store_content(self.dst_ns, &migrated)?;
        self.content.insert(old_addr, new_addr);
        self.report.content_written += 1;
        Ok(new_addr)
    }

    fn migrate_content(&mut self, addr: Digest) -> Result<Digest> {
        if let Some(mapped) = self.content.get(&addr) {
            return Ok(*mapped);
        }
        let bytes = self.src.load_content(addr)?;
        let new_addr = self.dst.store_content(self.dst_ns, &bytes)?;
        self.content.insert(addr, new_addr);
        self.report.content_written += 1;
        Ok(new_addr)
    }

    fn migrate_prolly(&mut self, root: Digest, leaf: LeafMode) -> Result<Digest> {
        if leaf == LeafMode::Raw
            && let Some(mapped) = self.prolly.get(&root)
        {
            return Ok(*mapped);
        }
        let bytes = self
            .src
            .store()
            .get(&root)?
            .ok_or_else(|| LoomError::not_found(format!("prolly node {root}")))?;
        let mut f = cbor::Fields::new(cbor::decode_array(&bytes)?);
        let tag = f.uint()?;
        let items = f.array()?;
        f.end()?;
        let migrated = match tag {
            0 => {
                let mut entries = Vec::with_capacity(items.len());
                for item in items {
                    let mut entry = cbor::Fields::new(cbor::as_array(item)?);
                    let key = entry.bytes()?;
                    let value = entry.bytes()?;
                    entry.end()?;
                    entries.push(cbor::Value::Array(vec![
                        cbor::Value::Bytes(key),
                        cbor::Value::Bytes(self.migrate_leaf_value(value, leaf)?),
                    ]));
                }
                cbor::Value::Array(vec![cbor::Value::Uint(0), cbor::Value::Array(entries)])
            }
            1 => {
                let mut children = Vec::with_capacity(items.len());
                for item in items {
                    let mut child = cbor::Fields::new(cbor::as_array(item)?);
                    let key = child.bytes()?;
                    let digest = child.digest()?;
                    child.end()?;
                    children.push(cbor::Value::Array(vec![
                        cbor::Value::Bytes(key),
                        cbor::digest_value(&self.migrate_prolly(digest, leaf)?),
                    ]));
                }
                cbor::Value::Array(vec![cbor::Value::Uint(1), cbor::Value::Array(children)])
            }
            _ => return Err(LoomError::corrupt("bad prolly node tag")),
        };
        let new_digest = self.dst.ingest_object(&cbor::encode(&migrated))?;
        if leaf == LeafMode::Raw {
            self.prolly.insert(root, new_digest);
        }
        self.report.prolly_nodes_written += 1;
        Ok(new_digest)
    }

    fn migrate_leaf_value(&mut self, value: Vec<u8>, mode: LeafMode) -> Result<Vec<u8>> {
        match mode {
            LeafMode::Raw => Ok(value),
            LeafMode::StreamRecord => {
                let mut f = cbor::Fields::new(cbor::decode_array(&value)?);
                let version = f.uint()?;
                if version != 1 {
                    return Err(LoomError::corrupt(
                        "unsupported stream entry-record version",
                    ));
                }
                let payload = f.digest()?;
                let len = f.uint()?;
                f.end()?;
                Ok(cbor::encode(&cbor::Value::Array(vec![
                    cbor::Value::Uint(1),
                    cbor::digest_value(&self.migrate_content(payload)?),
                    cbor::Value::Uint(len),
                ])))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeafMode {
    Raw,
    StreamRecord,
}

/// Push one branch from `src`'s workspace into `dst`'s existing workspace, transferring only the
/// objects `dst` lacks and **fast-forwarding** the branch. A non-fast-forward (the
/// destination tip is not an ancestor of the source tip) is refused with `NOT_FAST_FORWARD`; the
/// caller must integrate (pull/merge) first. `pull` is the same call with `src`/`dst` swapped.
pub fn push_branch<S: ObjectStore, T: ObjectStore>(
    src: &Loom<S>,
    src_ns: WorkspaceId,
    branch: &str,
    dst: &mut Loom<T>,
    dst_ns: WorkspaceId,
) -> Result<SyncReport> {
    src.authorize_workspace_facets(src_ns, AclRight::Read)?;
    dst.authorize_workspace_facets(dst_ns, AclRight::Write)?;
    dst.authorize_workspace_facets(dst_ns, AclRight::Advance)?;
    check_profiles(src.store().digest_algo(), dst.store().digest_algo())?;
    let src_tip = src
        .registry()
        .branch_tip(src_ns, branch)?
        .ok_or_else(|| LoomError::not_found(format!("branch {branch:?} on the source")))?;
    let dst_tip = dst.registry().branch_tip(dst_ns, branch)?;

    // Fast-forward guard: the destination's current tip must be an ancestor of the new tip.
    if let Some(cur) = dst_tip
        && cur != src_tip
        && !src.reachable(&[src_tip], &BTreeSet::new())?.contains(&cur)
    {
        return Err(LoomError::not_fast_forward(format!(
            "branch {branch:?}: {src_tip} does not descend from {cur}; integrate first"
        )));
    }

    // "Have" = everything reachable from the destination's existing branch tips; prune it from the
    // transfer so only genuinely new objects move.
    let have_tips: Vec<Digest> = branch_tips(dst, dst_ns)?
        .into_iter()
        .map(|(_, d)| d)
        .collect();
    let have = dst.reachable(&have_tips, &BTreeSet::new())?;
    let need = src.reachable(&[src_tip], &have)?;
    let mut report = transfer(src, dst, &need)?;

    dst.registry_mut()
        .update_branch(dst_ns, branch, dst_tip, src_tip)?;
    report.new_tips.push((branch.to_string(), src_tip));
    Ok(report)
}

/// Serialize a destination branch push through an embedded coordinator lock.
#[allow(clippy::too_many_arguments)]
pub fn push_branch_locked<S: ObjectStore, T: ObjectStore>(
    src: &Loom<S>,
    src_ns: WorkspaceId,
    branch: &str,
    dst: &mut Loom<T>,
    dst_ns: WorkspaceId,
    coordinator: &mut LockCoordinator,
    owner: LockOwner,
    lease_ms: u64,
    now_ms: u64,
) -> Result<SyncReport> {
    let key = sync_destination_lock_key(dst_ns, branch);
    let token =
        coordinator.try_acquire(key.clone(), owner, LockMode::Exclusive, lease_ms, now_ms)?;
    let result = push_branch(src, src_ns, branch, dst, dst_ns);
    if result.is_ok() {
        coordinator.apply_fence(&key, token.fence)?;
    }
    let release = coordinator.release(&token, now_ms);
    match (result, release) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(err), _) => Err(err),
        (Ok(_), Err(err)) => Err(err),
    }
}

/// Workspace/branch key used by the internal sync destination lock.
pub fn sync_destination_lock_key(ns: WorkspaceId, branch: &str) -> Vec<u8> {
    let mut key = b"sync/branch/".to_vec();
    key.extend_from_slice(ns.as_bytes());
    key.push(0);
    key.extend_from_slice(branch.as_bytes());
    key
}

/// Export an offline [`Bundle`] of one workspace: all of its branches and tags plus every object
/// they reach. The objects are emitted in digest order; import is order-independent.
pub fn bundle_export<S: ObjectStore>(src: &Loom<S>, src_ns: WorkspaceId) -> Result<Bundle> {
    src.authorize_workspace_facets(src_ns, AclRight::Read)?;
    let branches = branch_tips(src, src_ns)?;
    let tags = tag_targets(src, src_ns)?;
    let tips: Vec<Digest> = branches
        .iter()
        .chain(tags.iter())
        .map(|(_, d)| *d)
        .collect();
    let need = src.reachable(&tips, &BTreeSet::new())?;
    let mut objects = Vec::with_capacity(need.len());
    for d in &need {
        objects.push(src.object_bytes(*d)?);
    }
    Ok(Bundle {
        digest_algo: src.store().digest_algo(),
        ns_id: src_ns,
        facets: src.registry().facets(src_ns)?,
        ns_name: src.registry().name(src_ns)?,
        branches,
        tags,
        objects,
    })
}

/// Import a [`Bundle`] into `dst`, preserving the source workspace id. Every object is ingested and
/// re-verified; the bundle's ref tips are recreated only after their full subgraphs are confirmed
/// present - a missing object surfaces as `NOT_FOUND` and no ref is set. `dst` must not already hold a
/// workspace with the bundle's id or name.
pub fn bundle_import<T: ObjectStore>(
    dst: &mut Loom<T>,
    bundle: &Bundle,
) -> Result<(WorkspaceId, SyncReport)> {
    dst.authorize_global_admin()?;
    // The bundle's object addresses are under its source profile; importing into a Loom of a different
    // profile would relabel every object, so reject it loudly.
    check_profiles(bundle.digest_algo, dst.store().digest_algo())?;
    let dst_algo = dst.store().digest_algo();
    let mut report = SyncReport::default();
    for frame in &bundle.objects {
        // Dedup under the destination's profile (its objects are addressed that way).
        if let Ok(true) = dst.has_object(Digest::hash(dst_algo, frame)) {
            report.objects_skipped += 1;
        } else {
            dst.ingest_object(frame)?;
            report.objects_transferred += 1;
        }
    }
    // Confirm every advertised tip's subgraph is present before creating any ref.
    for (_, tip) in bundle.branches.iter().chain(bundle.tags.iter()) {
        dst.reachable(&[*tip], &BTreeSet::new())?;
    }
    let dst_ns = dst
        .registry_mut()
        .create_workspace(Some(&bundle.ns_name), bundle.ns_id)?;
    for facet in &bundle.facets {
        dst.registry_mut().add_facet(dst_ns, *facet)?;
    }
    for (b, tip) in &bundle.branches {
        dst.registry_mut().update_branch(dst_ns, b, None, *tip)?;
        report.new_tips.push((b.clone(), *tip));
    }
    for (t, target) in &bundle.tags {
        dst.registry_mut().tag_create(dst_ns, t, *target)?;
    }
    Ok((dst_ns, report))
}

// ---- bundle codec helpers ----------------------------------------------------------------------

/// `(name, digest)` ref pairs as a canonical array of `[name, digest]` arrays, preserving order.
fn refs_value(refs: &[(String, Digest)]) -> Value {
    Value::Array(
        refs.iter()
            .map(|(name, d)| Value::Array(vec![Value::Text(name.clone()), cbor::digest_value(d)]))
            .collect(),
    )
}

/// Parse the `[name, digest]` pair arrays produced by [`refs_value`].
fn refs_from(items: Vec<Value>) -> Result<Vec<(String, Digest)>> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let mut f = cbor::Fields::new(cbor::as_array(item)?);
        let name = f.text()?;
        let d = f.digest()?;
        f.end()?;
        out.push((name, d));
    }
    Ok(out)
}

fn facets_value(facets: &[FacetKind]) -> Value {
    Value::Array(
        facets
            .iter()
            .map(|facet| Value::Text(facet.as_str().to_string()))
            .collect(),
    )
}

fn facets_from(items: Vec<Value>) -> Result<Vec<FacetKind>> {
    items
        .into_iter()
        .map(|item| FacetKind::parse(&cbor::as_text(item)?))
        .collect()
}

fn workspace_id_value(id: WorkspaceId) -> Value {
    Value::Bytes(id.as_bytes().to_vec())
}

fn workspace_id_from(value: Value) -> Result<WorkspaceId> {
    let bytes = cbor::as_bytes(value)?;
    let bytes: [u8; 16] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| LoomError::corrupt("bundle workspace id is not 16 bytes"))?;
    Ok(WorkspaceId::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::memory::MemoryStore;
    use crate::vcs::Loom;
    use crate::workspace::{DEFAULT_BRANCH, FacetKind, WorkspaceId};
    use std::sync::Mutex;

    #[derive(Debug)]
    struct ProfileStore {
        algo: Algo,
        objects: Mutex<BTreeMap<[u8; crate::digest::DIGEST_LEN], Vec<u8>>>,
    }

    impl ProfileStore {
        fn new(algo: Algo) -> Self {
            Self {
                algo,
                objects: Mutex::new(BTreeMap::new()),
            }
        }
    }

    impl ObjectStore for ProfileStore {
        fn put(&self, canonical: &[u8]) -> Result<Digest> {
            let digest = Digest::hash(self.algo, canonical);
            self.objects
                .lock()
                .expect("profile store lock")
                .insert(*digest.bytes(), canonical.to_vec());
            Ok(digest)
        }

        fn get(&self, digest: &Digest) -> Result<Option<Vec<u8>>> {
            Ok(self
                .objects
                .lock()
                .expect("profile store lock")
                .get(digest.bytes())
                .cloned())
        }

        fn has(&self, digest: &Digest) -> Result<bool> {
            Ok(self
                .objects
                .lock()
                .expect("profile store lock")
                .contains_key(digest.bytes()))
        }

        fn len(&self) -> usize {
            self.objects.lock().expect("profile store lock").len()
        }

        fn digest_algo(&self) -> Algo {
            self.algo
        }
    }

    fn nid(seed: u8) -> WorkspaceId {
        WorkspaceId::from_bytes([seed; 16])
    }

    fn files_ns<S: ObjectStore>(loom: &mut Loom<S>, seed: u8) -> WorkspaceId {
        loom.registry_mut()
            .create(FacetKind::Files, None, nid(seed))
            .unwrap()
    }

    fn owner(name: &str) -> LockOwner {
        LockOwner {
            principal: name.to_string(),
            session: "sync".to_string(),
        }
    }

    fn authenticate_root(loom: &mut Loom<MemoryStore>, root: WorkspaceId) {
        let mut identity = crate::IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);
    }

    #[test]
    fn clone_copies_a_workspace_and_materializes() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = src
            .registry_mut()
            .create(FacetKind::Files, None, nid(1))
            .unwrap();
        src.registry_mut().add_facet(ns, FacetKind::Sql).unwrap();
        src.write_file(ns, "a.txt", b"alpha", 0o100644).unwrap();
        src.create_directory(ns, "dir", false).unwrap();
        src.write_file(ns, "dir/b.txt", b"bravo", 0o100644).unwrap();
        let tip = src.commit(ns, "nas", "c0", 1).unwrap();

        let mut dst = Loom::new(MemoryStore::new());
        let (dst_ns, report) = clone_workspace(&src, ns, &mut dst, nid(2)).unwrap();

        assert!(report.objects_transferred > 0);
        assert_eq!(report.objects_skipped, 0);
        assert_eq!(
            dst.registry().branch_tip(dst_ns, DEFAULT_BRANCH).unwrap(),
            Some(tip)
        );
        assert_eq!(
            dst.registry().facets(dst_ns).unwrap(),
            vec![FacetKind::Files, FacetKind::Sql]
        );
        // The clone is bare; checking out resolves blobs via the rebuilt content index.
        dst.checkout_commit(dst_ns, tip).unwrap();
        assert_eq!(dst.read_file(dst_ns, "a.txt").unwrap(), b"alpha");
        assert_eq!(dst.read_file(dst_ns, "dir/b.txt").unwrap(), b"bravo");
    }

    #[test]
    fn migrate_workspace_profile_rewrites_committed_files_and_streams() {
        let mut src = Loom::new(ProfileStore::new(Algo::Blake3));
        let ns = src
            .registry_mut()
            .create(FacetKind::Files, None, nid(1))
            .unwrap();
        src.registry_mut().add_facet(ns, FacetKind::Queue).unwrap();
        src.write_file(ns, "a.txt", b"alpha", 0o100644).unwrap();
        src.stream_append(ns, "events", b"one").unwrap();
        let src_tip = src.commit(ns, "nas", "c0", 1).unwrap();

        let mut dst = Loom::new(ProfileStore::new(Algo::Sha256));
        let (dst_ns, report) = migrate_workspace_profile(&src, ns, &mut dst).unwrap();
        let dst_tip = dst
            .registry()
            .branch_tip(dst_ns, DEFAULT_BRANCH)
            .unwrap()
            .unwrap();

        assert_ne!(src_tip, dst_tip);
        assert_eq!(dst_tip.algo(), Algo::Sha256);
        assert!(report.objects_written > 0);
        assert!(report.content_written > 0);
        assert!(report.prolly_nodes_written > 0);
        assert_eq!(dst.registry().name(dst_ns).unwrap(), "Default");
        assert_eq!(
            dst.registry().facets(dst_ns).unwrap(),
            vec![FacetKind::Files, FacetKind::Queue]
        );
        assert_eq!(dst.read_file(dst_ns, "a.txt").unwrap(), b"alpha");
        assert_eq!(
            dst.stream_get(dst_ns, "events", 0).unwrap().unwrap(),
            b"one"
        );
    }

    #[test]
    fn migrate_workspace_profile_preserves_optional_runtime_config_without_activation() {
        let mut src = Loom::new(ProfileStore::new(Algo::Blake3));
        let ns = files_ns(&mut src, 1);
        let mut settings = BTreeMap::new();
        settings.insert("endpoint".to_string(), "https://ipfs.example".to_string());
        let config =
            crate::OptionalRuntimeConfig::new(crate::OptionalRuntimeKind::Ipfs, true, settings)
                .unwrap();
        crate::set_optional_runtime_config(&mut src, ns, &config).unwrap();
        src.commit(ns, "nas", "optional runtime config", 1).unwrap();

        let mut dst = Loom::new(ProfileStore::new(Algo::Sha256));
        let (dst_ns, report) = migrate_workspace_profile(&src, ns, &mut dst).unwrap();

        assert!(report.objects_written > 0);
        assert!(report.content_written > 0);
        assert_eq!(
            crate::get_optional_runtime_config(&dst, dst_ns, crate::OptionalRuntimeKind::Ipfs)
                .unwrap(),
            Some(config)
        );
        assert_eq!(
            crate::activate_optional_runtime(&dst, dst_ns, crate::OptionalRuntimeKind::Ipfs)
                .unwrap_err()
                .code,
            Code::Unsupported
        );
    }

    #[test]
    fn incremental_push_transfers_only_new_objects() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = files_ns(&mut src, 1);
        src.registry_mut().add_facet(ns, FacetKind::Sql).unwrap();
        src.write_file(ns, "a.txt", b"alpha", 0o100644).unwrap();
        src.commit(ns, "nas", "c0", 1).unwrap();

        // Clone the first commit over.
        let mut dst = Loom::new(MemoryStore::new());
        let (dst_ns, _) = clone_workspace(&src, ns, &mut dst, nid(2)).unwrap();

        // A second commit on the source adds exactly one new file.
        src.write_file(ns, "b.txt", b"bravo", 0o100644).unwrap();
        let c1 = src.commit(ns, "nas", "c1", 2).unwrap();

        let report = push_branch(&src, ns, DEFAULT_BRANCH, &mut dst, dst_ns).unwrap();
        // Only the new commit, its tree, and the new blob move - not c0's objects.
        assert_eq!(report.objects_transferred, 3);
        assert_eq!(report.objects_skipped, 0);
        assert_eq!(
            dst.registry().branch_tip(dst_ns, DEFAULT_BRANCH).unwrap(),
            Some(c1)
        );
        dst.checkout_commit(dst_ns, c1).unwrap();
        assert_eq!(dst.read_file(dst_ns, "b.txt").unwrap(), b"bravo");
    }

    #[test]
    fn locked_push_serializes_destination_branch() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = files_ns(&mut src, 1);
        src.write_file(ns, "a.txt", b"alpha", 0o100644).unwrap();
        src.commit(ns, "nas", "c0", 1).unwrap();

        let mut dst = Loom::new(MemoryStore::new());
        let (dst_ns, _) = clone_workspace(&src, ns, &mut dst, nid(2)).unwrap();
        src.write_file(ns, "b.txt", b"bravo", 0o100644).unwrap();
        let c1 = src.commit(ns, "nas", "c1", 2).unwrap();

        let mut coordinator = LockCoordinator::default();
        let report = push_branch_locked(
            &src,
            ns,
            DEFAULT_BRANCH,
            &mut dst,
            dst_ns,
            &mut coordinator,
            owner("a"),
            100,
            10,
        )
        .unwrap();
        assert_eq!(report.objects_transferred, 3);
        assert_eq!(
            dst.registry().branch_tip(dst_ns, DEFAULT_BRANCH).unwrap(),
            Some(c1)
        );
        assert_eq!(
            coordinator.applied_fence(&sync_destination_lock_key(dst_ns, DEFAULT_BRANCH)),
            Some(loom_types::Fence::embedded(1))
        );
    }

    #[test]
    fn locked_push_returns_locked_before_mutating_on_contention() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = files_ns(&mut src, 1);
        src.write_file(ns, "a.txt", b"alpha", 0o100644).unwrap();
        let c0 = src.commit(ns, "nas", "c0", 1).unwrap();

        let mut dst = Loom::new(MemoryStore::new());
        let (dst_ns, _) = clone_workspace(&src, ns, &mut dst, nid(2)).unwrap();
        src.write_file(ns, "b.txt", b"bravo", 0o100644).unwrap();
        src.commit(ns, "nas", "c1", 2).unwrap();

        let mut coordinator = LockCoordinator::default();
        let key = sync_destination_lock_key(dst_ns, DEFAULT_BRANCH);
        coordinator
            .try_acquire(key, owner("holder"), LockMode::Exclusive, 100, 10)
            .unwrap();
        let err = push_branch_locked(
            &src,
            ns,
            DEFAULT_BRANCH,
            &mut dst,
            dst_ns,
            &mut coordinator,
            owner("contender"),
            100,
            10,
        )
        .unwrap_err();
        assert_eq!(err.code, Code::Locked);
        assert_eq!(
            dst.registry().branch_tip(dst_ns, DEFAULT_BRANCH).unwrap(),
            Some(c0)
        );
    }

    #[test]
    fn clone_requires_source_read_and_destination_admin() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = files_ns(&mut src, 1);
        src.write_file(ns, "a.txt", b"alpha", 0o100644).unwrap();
        src.commit(ns, "nas", "c0", 1).unwrap();
        let root = nid(90);
        authenticate_root(&mut src, root);

        let mut dst = Loom::new(MemoryStore::new());
        authenticate_root(&mut dst, root);
        let err = clone_workspace(&src, ns, &mut dst, nid(2)).unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);

        src.acl_store_mut()
            .allow(
                crate::AclSubject::Principal(root),
                Some(ns),
                None,
                [AclRight::Read],
            )
            .unwrap();
        let err = clone_workspace(&src, ns, &mut dst, nid(2)).unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);

        dst.acl_store_mut()
            .allow(
                crate::AclSubject::Principal(root),
                None,
                None,
                [AclRight::Admin],
            )
            .unwrap();
        clone_workspace(&src, ns, &mut dst, nid(2)).unwrap();
    }

    #[test]
    fn push_requires_source_read_and_destination_write_and_advance() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = files_ns(&mut src, 1);
        src.write_file(ns, "a.txt", b"alpha", 0o100644).unwrap();
        src.commit(ns, "nas", "c0", 1).unwrap();

        let mut dst = Loom::new(MemoryStore::new());
        let (dst_ns, _) = clone_workspace(&src, ns, &mut dst, nid(2)).unwrap();
        src.write_file(ns, "b.txt", b"bravo", 0o100644).unwrap();
        src.commit(ns, "nas", "c1", 2).unwrap();

        let root = nid(91);
        authenticate_root(&mut src, root);
        authenticate_root(&mut dst, root);
        let err = push_branch(&src, ns, DEFAULT_BRANCH, &mut dst, dst_ns).unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);

        src.acl_store_mut()
            .allow(
                crate::AclSubject::Principal(root),
                Some(ns),
                None,
                [AclRight::Read],
            )
            .unwrap();
        dst.acl_store_mut()
            .allow(
                crate::AclSubject::Principal(root),
                Some(dst_ns),
                None,
                [AclRight::Write],
            )
            .unwrap();
        let err = push_branch(&src, ns, DEFAULT_BRANCH, &mut dst, dst_ns).unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);

        dst.acl_store_mut()
            .allow(
                crate::AclSubject::Principal(root),
                Some(dst_ns),
                None,
                [AclRight::Advance],
            )
            .unwrap();
        push_branch(&src, ns, DEFAULT_BRANCH, &mut dst, dst_ns).unwrap();
    }

    #[test]
    fn bundle_export_requires_read_on_every_facet() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = files_ns(&mut src, 1);
        src.registry_mut().add_facet(ns, FacetKind::Sql).unwrap();
        src.write_file(ns, "a.txt", b"alpha", 0o100644).unwrap();
        src.commit(ns, "nas", "c0", 1).unwrap();
        let root = nid(92);
        authenticate_root(&mut src, root);

        let err = bundle_export(&src, ns).unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);
        src.acl_store_mut()
            .allow(
                crate::AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Files),
                [AclRight::Read],
            )
            .unwrap();
        let err = bundle_export(&src, ns).unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);

        src.acl_store_mut()
            .allow(
                crate::AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Sql),
                [AclRight::Read],
            )
            .unwrap();
        bundle_export(&src, ns).unwrap();
    }

    #[test]
    fn bundle_import_requires_destination_admin() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = files_ns(&mut src, 1);
        src.write_file(ns, "a.txt", b"alpha", 0o100644).unwrap();
        src.commit(ns, "nas", "c0", 1).unwrap();
        let bundle = bundle_export(&src, ns).unwrap();

        let mut dst = Loom::new(MemoryStore::new());
        let root = nid(93);
        authenticate_root(&mut dst, root);
        let err = bundle_import(&mut dst, &bundle).unwrap_err();
        assert_eq!(err.code, Code::PermissionDenied);

        dst.acl_store_mut()
            .allow(
                crate::AclSubject::Principal(root),
                None,
                None,
                [AclRight::Admin],
            )
            .unwrap();
        bundle_import(&mut dst, &bundle).unwrap();
    }

    #[test]
    fn bundle_encode_decode_round_trips_and_imports() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = files_ns(&mut src, 1);
        src.registry_mut().add_facet(ns, FacetKind::Sql).unwrap();
        src.write_file(ns, "a.txt", b"alpha", 0o100644).unwrap();
        let tip = src.commit(ns, "nas", "c0", 1).unwrap();
        src.registry_mut().tag_create(ns, "v1", tip).unwrap();

        let bundle = bundle_export(&src, ns).unwrap();
        assert_eq!(bundle.ns_id, ns);
        assert_eq!(bundle.facets, vec![FacetKind::Files, FacetKind::Sql]);
        // Serialize to bytes and back; the decoded bundle must be identical.
        let bytes = bundle.encode();
        let decoded = Bundle::decode(&bytes).unwrap();
        assert_eq!(decoded, bundle);
        assert!(Bundle::decode(b"not a bundle").is_err());

        // Importing the decoded bundle into a fresh Loom rebuilds the workspace + refs.
        let mut dst = Loom::new(MemoryStore::new());
        let (dst_ns, report) = bundle_import(&mut dst, &decoded).unwrap();
        assert_eq!(dst_ns, ns);
        assert!(report.objects_transferred > 0);
        assert_eq!(
            dst.registry().branch_tip(dst_ns, DEFAULT_BRANCH).unwrap(),
            Some(tip)
        );
        assert_eq!(
            dst.registry().facets(dst_ns).unwrap(),
            vec![FacetKind::Files, FacetKind::Sql]
        );
        assert_eq!(dst.registry().tag_target(dst_ns, "v1").unwrap(), Some(tip));
        dst.checkout_commit(dst_ns, tip).unwrap();
        assert_eq!(dst.read_file(dst_ns, "a.txt").unwrap(), b"alpha");

        let mut collision = Loom::new(MemoryStore::new());
        collision
            .registry_mut()
            .create_workspace(Some("other"), ns)
            .unwrap();
        let err = bundle_import(&mut collision, &decoded).unwrap_err();
        assert_eq!(err.code, crate::error::Code::AlreadyExists);
    }

    #[test]
    fn bundle_and_clone_preserve_optional_runtime_config_without_activation() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = files_ns(&mut src, 1);
        let mut settings = BTreeMap::new();
        settings.insert("socks".to_string(), "127.0.0.1:9050".to_string());
        let config =
            crate::OptionalRuntimeConfig::new(crate::OptionalRuntimeKind::Tor, true, settings)
                .unwrap();
        crate::set_optional_runtime_config(&mut src, ns, &config).unwrap();
        let tip = src.commit(ns, "nas", "optional runtime config", 1).unwrap();

        let bundle = bundle_export(&src, ns).unwrap();
        let mut imported = Loom::new(MemoryStore::new());
        let (imported_ns, _) = bundle_import(&mut imported, &bundle).unwrap();
        imported.checkout_commit(imported_ns, tip).unwrap();
        assert_eq!(
            crate::get_optional_runtime_config(
                &imported,
                imported_ns,
                crate::OptionalRuntimeKind::Tor
            )
            .unwrap(),
            Some(config.clone())
        );
        assert_eq!(
            crate::activate_optional_runtime(
                &imported,
                imported_ns,
                crate::OptionalRuntimeKind::Tor
            )
            .unwrap_err()
            .code,
            Code::Unsupported
        );

        let mut cloned = Loom::new(MemoryStore::new());
        let (cloned_ns, _) = clone_workspace(&src, ns, &mut cloned, nid(2)).unwrap();
        cloned.checkout_commit(cloned_ns, tip).unwrap();
        assert_eq!(
            crate::get_optional_runtime_config(&cloned, cloned_ns, crate::OptionalRuntimeKind::Tor)
                .unwrap(),
            Some(config)
        );
        assert_eq!(
            crate::activate_optional_runtime(&cloned, cloned_ns, crate::OptionalRuntimeKind::Tor)
                .unwrap_err()
                .code,
            Code::Unsupported
        );
    }

    #[test]
    fn bundle_carries_identity_profile_and_rejects_mismatched_import() {
        // A bundle records its source identity profile. The bundle round-trips it,
        // and importing a bundle whose profile differs from the destination is rejected loudly rather
        // than silently rehashing object labels.
        let mut src = Loom::new(MemoryStore::new());
        let ns = files_ns(&mut src, 1);
        src.write_file(ns, "a.txt", b"alpha", 0o100644).unwrap();
        src.commit(ns, "nas", "c0", 1).unwrap();
        let mut bundle = bundle_export(&src, ns).unwrap();
        // MemoryStore is the default profile, so the bundle is BLAKE3 and round-trips with that tag.
        assert_eq!(bundle.digest_algo, crate::digest::Algo::Blake3);
        assert_eq!(Bundle::decode(&bundle.encode()).unwrap(), bundle);

        // Forge a FIPS-profile bundle and import it into a BLAKE3 Loom: profile mismatch -> Conflict.
        bundle.digest_algo = crate::digest::Algo::Sha256;
        let mut dst = Loom::new(MemoryStore::new());
        let err = bundle_import(&mut dst, &bundle).unwrap_err();
        assert_eq!(err.code, crate::error::Code::Conflict);
    }

    #[test]
    fn non_fast_forward_push_is_refused() {
        // src and dst diverge from a shared base, so neither tip descends from the other.
        let mut src = Loom::new(MemoryStore::new());
        let ns = files_ns(&mut src, 1);
        src.write_file(ns, "a.txt", b"base", 0o100644).unwrap();
        src.commit(ns, "nas", "c0", 1).unwrap();

        let mut dst = Loom::new(MemoryStore::new());
        let (dst_ns, _) = clone_workspace(&src, ns, &mut dst, nid(2)).unwrap();

        // Both sides commit independently on top of the shared base.
        src.write_file(ns, "a.txt", b"src-side", 0o100644).unwrap();
        src.commit(ns, "nas", "src", 2).unwrap();
        dst.write_file(dst_ns, "a.txt", b"dst-side", 0o100644)
            .unwrap();
        dst.commit(dst_ns, "nas", "dst", 2).unwrap();

        let err = push_branch(&src, ns, DEFAULT_BRANCH, &mut dst, dst_ns).unwrap_err();
        assert_eq!(err.code, crate::error::Code::NotFastForward);
    }

    #[test]
    fn bundle_round_trips_file_to_file() {
        let mut a = Loom::new(MemoryStore::new());
        let ns = files_ns(&mut a, 1);
        a.write_file(ns, "note.txt", b"hello", 0o100644).unwrap();
        let tip = a.commit(ns, "nas", "snapshot", 1).unwrap();
        a.registry_mut().tag_create(ns, "v1", tip).unwrap();

        let bundle = bundle_export(&a, ns).unwrap();
        assert!(!bundle.objects.is_empty());

        // Import into a brand-new Loom that has never seen these objects.
        let mut b = Loom::new(MemoryStore::new());
        let (b_ns, report) = bundle_import(&mut b, &bundle).unwrap();
        assert_eq!(b_ns, ns);
        assert_eq!(report.objects_transferred as usize, bundle.objects.len());
        assert_eq!(
            b.registry().branch_tip(b_ns, DEFAULT_BRANCH).unwrap(),
            Some(tip)
        );
        assert_eq!(b.registry().tag_target(b_ns, "v1").unwrap(), Some(tip));
        b.checkout_commit(b_ns, tip).unwrap();
        assert_eq!(b.read_file(b_ns, "note.txt").unwrap(), b"hello");
    }

    #[test]
    fn objects_are_deduplicated_across_workspaces() {
        // Identical content in two source commits yields identical objects; a second push of the
        // same content transfers nothing new.
        let mut src = Loom::new(MemoryStore::new());
        let ns = files_ns(&mut src, 1);
        src.write_file(ns, "x.txt", b"same", 0o100644).unwrap();
        src.commit(ns, "nas", "c0", 1).unwrap();

        let mut dst = Loom::new(MemoryStore::new());
        let (dst_ns, _) = clone_workspace(&src, ns, &mut dst, nid(2)).unwrap();

        // Re-push with no new commits: nothing to transfer, tip unchanged.
        let report = push_branch(&src, ns, DEFAULT_BRANCH, &mut dst, dst_ns).unwrap();
        assert_eq!(report.objects_transferred, 0);
    }
}
