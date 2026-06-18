//! Probabilistic Merkle B-tree (prolly tree) sharding for large key/value maps. Node boundaries are
//! content-defined (a hash of the key, not insertion order), giving two properties a plain B-tree
//! lacks:
//!
//! - History independence: the same key set always produces the same node boundaries, node digests,
//!   and root, independent of insertion order, so two peers that reach the same state converge.
//! - Structural sharing: changing one entry re-chunks only its node and the spine to the root; every
//!   other node keeps its digest, so a diff/sync transfers `O(changed)` nodes.
//!
//! Nodes here are content-addressed byte blobs in the [`ObjectStore`]; the tree's identity is its root
//! digest. This module backs the tabular facet's row map ([`crate::tabular::Table::build_rows`]).
//! Directory sharding reuses the same boundary rule (`is_boundary`) but emits `Tree`-object shard
//! nodes instead, so it reuses the existing object types and the normal Tree reachability walk.

use crate::cbor;
use crate::digest::Digest;
use crate::error::{LoomError, Result};
use crate::provider::ObjectStore;
use std::collections::{BTreeMap, BTreeSet};

const LEAF_TAG: u8 = 0;
const INTERNAL_TAG: u8 = 1;
/// Average fan-out is about 2^AVG_BITS entries/children per node (a key is a boundary with prob 2^-AVG_BITS).
const AVG_BITS: u32 = 5;
const BOUNDARY_MASK: u32 = (1 << AVG_BITS) - 1;
/// Guard against a crafted/cyclic tree during traversal.
const MAX_DEPTH: usize = 64;

/// Whether `key` ends a node at `level`: a pure function of the key and level, so the boundary set is
/// independent of insertion order. Folding `level` in stops every level from cutting at the same keys.
/// Shared with directory sharding so it cuts at the same boundaries.
pub(crate) fn is_boundary(key: &[u8], level: u8) -> bool {
    let mut buf = Vec::with_capacity(key.len() + 1);
    buf.extend_from_slice(key);
    buf.push(level);
    // This is a *structural* chunking-boundary function (it decides tree shape), not a content address
    // or an integrity hash, so it stays BLAKE3 across identity profiles: node *ids*
    // come from `store.put` and so already use the store's profile; only where a node is split is fixed
    // here, and keeping it constant keeps prolly tree shape deterministic regardless of profile.
    let h = Digest::blake3(&buf);
    let b = h.bytes();
    let lead = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    lead & BOUNDARY_MASK == 0
}

// ---- node codec --------------------------------------------------------------------------------

fn encode_leaf(entries: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    use cbor::Value::{Array, Bytes, Uint};
    let items = entries
        .iter()
        .map(|(k, v)| Array(vec![Bytes(k.clone()), Bytes(v.clone())]))
        .collect();
    cbor::encode(&Array(vec![Uint(u64::from(LEAF_TAG)), Array(items)]))
}

fn encode_internal(children: &[(Vec<u8>, Digest)]) -> Vec<u8> {
    use cbor::Value::{Array, Bytes, Uint};
    let items = children
        .iter()
        .map(|(max_key, digest)| Array(vec![Bytes(max_key.clone()), cbor::digest_value(digest)]))
        .collect();
    cbor::encode(&Array(vec![Uint(u64::from(INTERNAL_TAG)), Array(items)]))
}

enum Node {
    Leaf(Vec<(Vec<u8>, Vec<u8>)>),
    Internal(Vec<(Vec<u8>, Digest)>), // (max key in child subtree, child digest), ascending
}

fn read_node<S: ObjectStore>(store: &S, digest: &Digest) -> Result<Node> {
    let bytes = store
        .get(digest)?
        .ok_or_else(|| LoomError::not_found(format!("prolly node {digest}")))?;
    let mut f = cbor::Fields::new(cbor::decode_array(&bytes)?);
    let tag = f.uint()?;
    let items = f.array()?;
    f.end()?;
    match tag {
        t if t == u64::from(LEAF_TAG) => {
            let mut entries = Vec::with_capacity(items.len());
            for item in items {
                let mut ef = cbor::Fields::new(cbor::as_array(item)?);
                let k = ef.bytes()?;
                let v = ef.bytes()?;
                ef.end()?;
                entries.push((k, v));
            }
            Ok(Node::Leaf(entries))
        }
        t if t == u64::from(INTERNAL_TAG) => {
            let mut children = Vec::with_capacity(items.len());
            for item in items {
                let mut ef = cbor::Fields::new(cbor::as_array(item)?);
                let max_key = ef.bytes()?;
                let digest = ef.digest()?;
                ef.end()?;
                children.push((max_key, digest));
            }
            Ok(Node::Internal(children))
        }
        _ => Err(LoomError::corrupt("bad prolly node tag")),
    }
}

// ---- build / lookup ----------------------------------------------------------------------------

/// Build a prolly tree from `entries` (which MUST be sorted ascending by key and unique), storing its
/// nodes in `store`, and return the root digest, or `None` if `entries` is empty. The root is a pure
/// function of the entry set.
pub fn build<S: ObjectStore>(
    store: &mut S,
    entries: &[(Vec<u8>, Vec<u8>)],
) -> Result<Option<Digest>> {
    if entries.is_empty() {
        return Ok(None);
    }
    // Level 0: chunk the sorted entries into leaves at content-defined boundaries.
    let mut level_nodes: Vec<(Vec<u8>, Digest)> = Vec::new();
    let mut run: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for (k, v) in entries {
        run.push((k.clone(), v.clone()));
        if is_boundary(k, 0) {
            level_nodes.push(flush_leaf(store, &mut run)?);
        }
    }
    if !run.is_empty() {
        level_nodes.push(flush_leaf(store, &mut run)?);
    }

    // Internal levels: chunk the child (max_key, digest) list the same way until one root remains.
    let mut level: u8 = 1;
    while level_nodes.len() > 1 {
        let mut parents: Vec<(Vec<u8>, Digest)> = Vec::new();
        let mut crun: Vec<(Vec<u8>, Digest)> = Vec::new();
        for child in level_nodes {
            let boundary = is_boundary(&child.0, level);
            crun.push(child);
            if boundary {
                parents.push(flush_internal(store, &mut crun)?);
            }
        }
        if !crun.is_empty() {
            parents.push(flush_internal(store, &mut crun)?);
        }
        level_nodes = parents;
        level = level
            .checked_add(1)
            .ok_or_else(|| LoomError::corrupt("prolly tree too tall"))?;
    }
    Ok(level_nodes.first().map(|(_, d)| *d))
}

fn flush_leaf<S: ObjectStore>(
    store: &mut S,
    run: &mut Vec<(Vec<u8>, Vec<u8>)>,
) -> Result<(Vec<u8>, Digest)> {
    let max_key = run.last().expect("non-empty leaf run").0.clone();
    let digest = store.put(&encode_leaf(run))?;
    run.clear();
    Ok((max_key, digest))
}

fn flush_internal<S: ObjectStore>(
    store: &mut S,
    run: &mut Vec<(Vec<u8>, Digest)>,
) -> Result<(Vec<u8>, Digest)> {
    let max_key = run.last().expect("non-empty internal run").0.clone();
    let digest = store.put(&encode_internal(run))?;
    run.clear();
    Ok((max_key, digest))
}

// ---- incremental insert / remove ---------------------------------------------------------------

/// A run of `(max_key, node_digest)` child entries at one tree level (what a chunking pass produces).
type ChildRun = Vec<(Vec<u8>, Digest)>;

/// A change to apply to one key during [`insert`]/[`remove`].
enum Change<'a> {
    /// Insert or replace the key with this value.
    Set(&'a [u8]),
    /// Remove the key.
    Remove,
}

/// Insert or replace `key`->`value` in the tree at `root` (`None` = empty tree), returning the new
/// root. Only the affected leaf and the spine above it are re-chunked (`O(log n)` nodes touched), and
/// the result is **byte-identical** to [`build`] over the equivalent entry set. An insert never forces
/// a cross-node merge (it keeps a leaf's closing boundary or splits at a new one, always closed), so
/// the local path always applies.
pub fn insert<S: ObjectStore>(
    store: &mut S,
    root: Option<&Digest>,
    key: &[u8],
    value: &[u8],
) -> Result<Digest> {
    match root {
        None => Ok(build(store, &[(key.to_vec(), value.to_vec())])?
            .expect("a single entry builds a non-empty tree")),
        Some(r) => {
            let h = height(store, r)?;
            match mutate(store, r, h, key, &Change::Set(value), true)? {
                Some(nodes) => finish(store, nodes, h),
                // Unreachable for inserts (they never open a leaf), but fall back correctly if so.
                None => Ok(rebuild_with(store, r, key, &Change::Set(value))?
                    .expect("insert yields a non-empty tree")),
            }
        }
    }
}

/// Remove `key` from the tree at `root`, returning the new root, or `None` if the tree is now empty.
/// The local `O(log n)` path applies unless the removal deletes a leaf's boundary key and leaves that
/// (non-rightmost) leaf "open" - it must then merge with the successor leaf, which crosses subtree
/// edges, so that case falls back to a full [`build`] (correct, and rare: a leaf boundary is ~1 key in
/// 32). Either way the result equals `build` over the reduced set.
pub fn remove<S: ObjectStore>(store: &mut S, root: &Digest, key: &[u8]) -> Result<Option<Digest>> {
    let h = height(store, root)?;
    match mutate(store, root, h, key, &Change::Remove, true)? {
        Some(nodes) if nodes.is_empty() => Ok(None),
        Some(nodes) => Ok(Some(finish(store, nodes, h)?)),
        None => rebuild_with(store, root, key, &Change::Remove),
    }
}

/// Fallback for the cross-subtree-merge case: materialize all entries, apply the change, and [`build`].
fn rebuild_with<S: ObjectStore>(
    store: &mut S,
    root: &Digest,
    key: &[u8],
    change: &Change,
) -> Result<Option<Digest>> {
    let mut entries = entries(store, root)?;
    match entries.binary_search_by(|(k, _)| k.as_slice().cmp(key)) {
        Ok(i) => match change {
            Change::Set(v) => entries[i].1 = v.to_vec(),
            Change::Remove => {
                entries.remove(i);
            }
        },
        Err(i) => {
            if let Change::Set(v) = change {
                entries.insert(i, (key.to_vec(), v.to_vec()));
            }
        }
    }
    build(store, &entries)
}

/// The height of the tree (0 for a single leaf), via the leftmost spine.
fn height<S: ObjectStore>(store: &S, root: &Digest) -> Result<u8> {
    let mut digest = *root;
    for h in 0..=(MAX_DEPTH as u8) {
        match read_node(store, &digest)? {
            Node::Leaf(_) => return Ok(h),
            Node::Internal(children) => {
                digest = children
                    .first()
                    .ok_or_else(|| LoomError::corrupt("empty internal node"))?
                    .1;
            }
        }
    }
    Err(LoomError::corrupt("prolly tree too deep"))
}

/// Apply `change` to `key` in the subtree at `digest` (at `level`, leaves at 0), returning the
/// `(max_key, digest)` nodes that replace it in its parent - or `None` if the change opens a
/// non-rightmost leaf (a cross-subtree merge the local path can't do; the caller rebuilds). `rightmost`
/// is true while the descent has only taken last children, i.e. the affected leaf is the tree's last.
fn mutate<S: ObjectStore>(
    store: &mut S,
    digest: &Digest,
    level: u8,
    key: &[u8],
    change: &Change,
    rightmost: bool,
) -> Result<Option<ChildRun>> {
    match read_node(store, digest)? {
        Node::Leaf(mut entries) => {
            match entries.binary_search_by(|(k, _)| k.as_slice().cmp(key)) {
                Ok(i) => match change {
                    Change::Set(v) => entries[i].1 = v.to_vec(),
                    Change::Remove => {
                        entries.remove(i);
                    }
                },
                Err(i) => {
                    if let Change::Set(v) = change {
                        entries.insert(i, (key.to_vec(), v.to_vec()));
                    }
                }
            }
            // A non-empty leaf whose last key is not a boundary is "open": it must merge with the
            // successor leaf. The local splice can do that only when the leaf is the tree's last
            // (no successor); otherwise the caller must rebuild.
            if let Some((last, _)) = entries.last()
                && !is_boundary(last, 0)
                && !rightmost
            {
                return Ok(None);
            }
            Ok(Some(chunk_leaves(store, &entries)?))
        }
        Node::Internal(children) => {
            if level == 0 {
                return Err(LoomError::corrupt("internal node at level 0"));
            }
            let i = children
                .iter()
                .position(|(mk, _)| mk.as_slice() >= key)
                .unwrap_or(children.len() - 1);
            let child_rightmost = rightmost && i == children.len() - 1;
            let Some(sub) = mutate(
                store,
                &children[i].1,
                level - 1,
                key,
                change,
                child_rightmost,
            )?
            else {
                return Ok(None);
            };
            let mut next = Vec::with_capacity(children.len() + sub.len());
            next.extend_from_slice(&children[..i]);
            next.extend(sub);
            next.extend_from_slice(&children[i + 1..]);
            if next.is_empty() {
                return Ok(Some(Vec::new()));
            }
            Ok(Some(chunk_internal(store, level, &next)?))
        }
    }
}

/// Build the final root from the re-chunked top-level `nodes` (at `level`): add internal levels until
/// one node remains, then collapse a single-child internal root (which [`build`] never produces).
fn finish<S: ObjectStore>(store: &mut S, mut nodes: ChildRun, mut level: u8) -> Result<Digest> {
    while nodes.len() > 1 {
        level = level
            .checked_add(1)
            .ok_or_else(|| LoomError::corrupt("prolly tree too tall"))?;
        nodes = chunk_internal(store, level, &nodes)?;
    }
    let mut root = nodes
        .first()
        .ok_or_else(|| LoomError::corrupt("empty prolly result"))?
        .1;
    for _ in 0..=MAX_DEPTH {
        match read_node(store, &root)? {
            Node::Internal(ch) if ch.len() == 1 => root = ch[0].1,
            _ => return Ok(root),
        }
    }
    Err(LoomError::corrupt("prolly collapse too deep"))
}

/// Chunk sorted leaf `entries` into leaf nodes at content-defined boundaries (the rule [`build`] uses).
fn chunk_leaves<S: ObjectStore>(store: &mut S, entries: &[(Vec<u8>, Vec<u8>)]) -> Result<ChildRun> {
    let mut out = Vec::new();
    let mut run: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for (k, v) in entries {
        run.push((k.clone(), v.clone()));
        if is_boundary(k, 0) {
            out.push(flush_leaf(store, &mut run)?);
        }
    }
    if !run.is_empty() {
        out.push(flush_leaf(store, &mut run)?);
    }
    Ok(out)
}

/// Chunk a `children` run into internal nodes at `level`'s content-defined boundaries.
fn chunk_internal<S: ObjectStore>(
    store: &mut S,
    level: u8,
    children: &[(Vec<u8>, Digest)],
) -> Result<ChildRun> {
    let mut out = Vec::new();
    let mut run: Vec<(Vec<u8>, Digest)> = Vec::new();
    for child in children {
        run.push(child.clone());
        if is_boundary(&child.0, level) {
            out.push(flush_internal(store, &mut run)?);
        }
    }
    if !run.is_empty() {
        out.push(flush_internal(store, &mut run)?);
    }
    Ok(out)
}

/// Look up `key` in the tree rooted at `root`, returning its value or `None`.
pub fn get<S: ObjectStore>(store: &S, root: &Digest, key: &[u8]) -> Result<Option<Vec<u8>>> {
    let mut digest = *root;
    for _ in 0..MAX_DEPTH {
        match read_node(store, &digest)? {
            Node::Leaf(entries) => {
                return Ok(entries
                    .binary_search_by(|(k, _)| k.as_slice().cmp(key))
                    .ok()
                    .map(|i| entries[i].1.clone()));
            }
            Node::Internal(children) => {
                // Descend into the first child whose max key is >= the search key.
                match children
                    .iter()
                    .position(|(max_key, _)| max_key.as_slice() >= key)
                {
                    Some(i) => digest = children[i].1,
                    None => return Ok(None), // key is greater than every entry
                }
            }
        }
    }
    Err(LoomError::corrupt(
        "prolly tree deeper than the structural maximum",
    ))
}

/// All entries in the tree rooted at `root`, in ascending key order (a full scan). Used to
/// materialize a sharded directory or table back into its in-memory form. Bounded by `MAX_DEPTH`.
pub fn entries<S: ObjectStore>(store: &S, root: &Digest) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut out = Vec::new();
    collect_entries(store, root, 0, &mut out)?;
    Ok(out)
}

fn collect_entries<S: ObjectStore>(
    store: &S,
    digest: &Digest,
    depth: usize,
    out: &mut Vec<(Vec<u8>, Vec<u8>)>,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(LoomError::corrupt("prolly tree too deep"));
    }
    match read_node(store, digest)? {
        Node::Leaf(entries) => out.extend(entries),
        // Children are stored ascending by max key, so a left-to-right descent yields sorted entries.
        Node::Internal(children) => {
            for (_, child) in children {
                collect_entries(store, &child, depth + 1, out)?;
            }
        }
    }
    Ok(())
}

/// All entries whose key starts with `prefix`, in ascending key order. Internal subtrees whose whole
/// key range falls below the prefix are skipped, and the walk stops once it passes the prefix range,
/// so cost is `O(matches + height)` not `O(total)`. A `prefix` of `[]` returns every entry. Backs the
/// secondary-index point/range lookup ([`crate::tabular`]). Bounded by `MAX_DEPTH`.
pub fn scan_prefix<S: ObjectStore>(
    store: &S,
    root: &Digest,
    prefix: &[u8],
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut out = Vec::new();
    collect_prefix(store, root, prefix, &upper_bound(prefix), 0, &mut out)?;
    Ok(out)
}

/// The smallest byte string strictly greater than every string starting with `prefix` (increment the
/// last byte below `0xFF`, dropping trailing `0xFF`s). `None` means unbounded above (an empty prefix,
/// or all-`0xFF`): every key is in range.
fn upper_bound(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut u = prefix.to_vec();
    while let Some(last) = u.last_mut() {
        if *last < 0xFF {
            *last += 1;
            return Some(u);
        }
        u.pop();
    }
    None
}

fn collect_prefix<S: ObjectStore>(
    store: &S,
    digest: &Digest,
    prefix: &[u8],
    upper: &Option<Vec<u8>>,
    depth: usize,
    out: &mut Vec<(Vec<u8>, Vec<u8>)>,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(LoomError::corrupt("prolly tree too deep"));
    }
    match read_node(store, digest)? {
        Node::Leaf(entries) => {
            for (k, v) in entries {
                if k.starts_with(prefix) {
                    out.push((k, v));
                }
            }
        }
        Node::Internal(children) => {
            // Children are ascending by max key. Skip a child whose max key is below the prefix (all
            // its keys precede the range); stop after the first child whose max key reaches the upper
            // bound (every later child starts beyond the range).
            for (max_key, child) in children {
                if max_key.as_slice() < prefix {
                    continue;
                }
                collect_prefix(store, &child, prefix, upper, depth + 1, out)?;
                if let Some(u) = upper
                    && max_key.as_slice() >= u.as_slice()
                {
                    break;
                }
            }
        }
    }
    Ok(())
}

// ---- lazy cursor (streaming, larger-than-RAM) --------------------------------------------------

/// A lazy, forward, in-order cursor over a prolly tree: it walks leaves left to right, loading nodes
/// from the store **on demand**, so iterating does not materialize the whole tree. This is the
/// streaming primitive behind larger-than-RAM table scans and index range lookups (#180A.1); unlike
/// [`entries`] / [`scan_prefix`] (which return a full `Vec`), memory use is bounded by the tree height
/// plus the current leaf. Supports an optional inclusive **start** key (seek) and exclusive **upper**
/// bound (range). Read-only: it borrows the store immutably.
/// One frame of a [`ProllyCursor`]'s descent path: an internal node's children ([`ChildRun`]) and the
/// index of the next child to descend into.
type CursorFrame = (ChildRun, usize);

pub struct ProllyCursor<'a, S: ObjectStore> {
    store: &'a S,
    /// Path of internal nodes from the root toward the current leaf: each frame holds that node's
    /// `(max_key, child)` pairs (ascending) and the index of the **next** child to descend.
    stack: Vec<CursorFrame>,
    leaf: Vec<(Vec<u8>, Vec<u8>)>,
    leaf_idx: usize,
    /// Exclusive upper bound; iteration stops at the first key `>= upper`.
    upper: Option<Vec<u8>>,
    /// Inclusive start key; only consulted while descending to the first leaf, then cleared.
    start: Option<Vec<u8>>,
    depth_guard: usize,
}

impl<'a, S: ObjectStore> ProllyCursor<'a, S> {
    /// A cursor over every entry of the tree rooted at `root`, ascending. `None` for an empty tree.
    pub fn open(store: &'a S, root: &Digest) -> Result<Self> {
        Self::open_range(store, root, None, None)
    }

    /// A cursor over entries with key in `[start, upper)` (start inclusive, upper exclusive; either
    /// `None` is unbounded on that side), ascending. Backs index range lookups.
    pub fn open_range(
        store: &'a S,
        root: &Digest,
        start: Option<&[u8]>,
        upper: Option<Vec<u8>>,
    ) -> Result<Self> {
        let mut cur = ProllyCursor {
            store,
            stack: Vec::new(),
            leaf: Vec::new(),
            leaf_idx: 0,
            upper,
            start: start.map(<[u8]>::to_vec),
            depth_guard: 0,
        };
        cur.descend_from(*root)?;
        Ok(cur)
    }

    /// A cursor over entries whose key starts with `prefix` (the index point/equality lookup), ascending.
    pub fn open_prefix(store: &'a S, root: &Digest, prefix: &[u8]) -> Result<Self> {
        Self::open_range(store, root, Some(prefix), upper_bound(prefix))
    }

    /// Descend from `digest` to the leftmost leaf at or after `self.start`, pushing internal frames.
    fn descend_from(&mut self, digest: Digest) -> Result<()> {
        let mut d = digest;
        loop {
            self.depth_guard += 1;
            if self.depth_guard > MAX_DEPTH * MAX_DEPTH {
                return Err(LoomError::corrupt("prolly cursor walked too deep"));
            }
            match read_node(self.store, &d)? {
                Node::Leaf(entries) => {
                    self.leaf = entries;
                    self.leaf_idx = 0;
                    if let Some(s) = &self.start {
                        while self.leaf_idx < self.leaf.len()
                            && self.leaf[self.leaf_idx].0.as_slice() < s.as_slice()
                        {
                            self.leaf_idx += 1;
                        }
                    }
                    return Ok(());
                }
                Node::Internal(children) => {
                    // Children are ascending by max key; a child whose max key is below `start` holds
                    // only keys before the range, so skip to the first child that can contain `start`.
                    let mut idx = 0;
                    if let Some(s) = &self.start {
                        while idx < children.len() && children[idx].0.as_slice() < s.as_slice() {
                            idx += 1;
                        }
                    }
                    if idx >= children.len() {
                        self.leaf = Vec::new();
                        self.leaf_idx = 0;
                        return Ok(());
                    }
                    let child = children[idx].1;
                    self.stack.push((children, idx + 1));
                    d = child;
                }
            }
        }
    }

    /// The next `(key, value)` in ascending order, or `None` at the end of the range.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
        loop {
            if self.leaf_idx < self.leaf.len() {
                let (k, v) = self.leaf[self.leaf_idx].clone();
                if let Some(u) = &self.upper
                    && k.as_slice() >= u.as_slice()
                {
                    // Past the range: nothing more can be in [.., upper). Exhaust the cursor.
                    self.stack.clear();
                    self.leaf.clear();
                    self.leaf_idx = 0;
                    return Ok(None);
                }
                self.leaf_idx += 1;
                return Ok(Some((k, v)));
            }
            // Current leaf is spent; the start bound no longer applies once we move past the first leaf.
            self.start = None;
            self.depth_guard = 0;
            let next_child = loop {
                match self.stack.last_mut() {
                    None => return Ok(None),
                    Some((children, idx)) => {
                        if *idx < children.len() {
                            let child = children[*idx].1;
                            *idx += 1;
                            break child;
                        }
                        self.stack.pop();
                    }
                }
            };
            self.descend_from(next_child)?;
        }
    }
}

/// The node digests and leaf values visited by [`reachable_with_leaves`].
pub struct ProllyReach {
    /// Visited node digests.
    pub nodes: Vec<Digest>,
    /// Values from the visited leaves, in encounter order.
    pub leaf_values: Vec<Vec<u8>>,
}

/// Walk the tree rooted at `root`, collecting node digests and leaf values, pruning any subtree whose
/// node digest is already in `have` (a held content-addressed node implies its whole subtree is held).
/// Pass an empty `have` to retain everything. Bounded by `MAX_DEPTH`.
pub fn reachable_with_leaves<S: ObjectStore>(
    store: &S,
    root: &Digest,
    have: &BTreeSet<Digest>,
) -> Result<ProllyReach> {
    let mut out = ProllyReach {
        nodes: Vec::new(),
        leaf_values: Vec::new(),
    };
    let mut stack = vec![(*root, 0usize)];
    while let Some((d, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            return Err(LoomError::corrupt("prolly tree too deep"));
        }
        if have.contains(&d) {
            continue; // peer already holds this node and everything below it
        }
        out.nodes.push(d);
        match read_node(store, &d)? {
            Node::Leaf(entries) => {
                for (_, v) in entries {
                    out.leaf_values.push(v);
                }
            }
            Node::Internal(children) => {
                for (_, child) in children {
                    stack.push((child, depth + 1));
                }
            }
        }
    }
    Ok(out)
}

/// Every node digest reachable from `root` (diff/sharing measurements and the GC walk). A thin
/// wrapper over [`reachable_with_leaves`] with an empty `have`.
pub fn reachable_nodes<S: ObjectStore>(store: &S, root: &Digest) -> Result<Vec<Digest>> {
    Ok(reachable_with_leaves(store, root, &BTreeSet::new())?.nodes)
}

/// Validate that `bytes` are a structurally well-formed prolly node (a leaf of `[key, value]` pairs or
/// an internal node of `[max_key, digest]` children). Used to admit raw node blobs on ingest while
/// rejecting any other bytes.
pub fn validate_node_bytes(bytes: &[u8]) -> Result<()> {
    let mut f = cbor::Fields::new(cbor::decode_array(bytes)?);
    let tag = f.uint()?;
    let items = f.array()?;
    f.end()?;
    for item in items {
        let mut ef = cbor::Fields::new(cbor::as_array(item)?);
        match tag {
            t if t == u64::from(LEAF_TAG) => {
                ef.bytes()?;
                ef.bytes()?;
            }
            t if t == u64::from(INTERNAL_TAG) => {
                ef.bytes()?;
                ef.digest()?;
            }
            _ => return Err(LoomError::corrupt("not a prolly node")),
        }
        ef.end()?;
    }
    Ok(())
}

/// Leaf entries of the tree at `root` whose enclosing leaf is **not** shared with `have` (a set of
/// node digests from another tree). A subtree whose node digest is in `have` is pruned: it is
/// byte-identical to the other tree, so none of its entries can differ. Bounded by `MAX_DEPTH`.
fn changed_entries<S: ObjectStore>(
    store: &S,
    root: &Digest,
    have: &BTreeSet<Digest>,
    depth: usize,
    out: &mut Vec<(Vec<u8>, Vec<u8>)>,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(LoomError::corrupt("prolly tree too deep"));
    }
    if have.contains(root) {
        return Ok(()); // shared subtree: identical in both trees, nothing changed below it
    }
    match read_node(store, root)? {
        Node::Leaf(entries) => out.extend(entries),
        Node::Internal(children) => {
            for (_, child) in children {
                changed_entries(store, &child, have, depth + 1, out)?;
            }
        }
    }
    Ok(())
}

/// One differing key from [`diff`]: `(key, value_in_a, value_in_b)`; a `None` value means the key is
/// absent on that side.
pub type DiffEntry = (Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>);

/// Row-level diff of two prolly trees, in `O(changed)` not `O(total)`: shared subtrees (equal node
/// digests) are pruned, so only the differing leaves are read. Returns one tuple per key that differs,
/// as `(key, value_in_a, value_in_b)` where a `None` means the key is absent on that side (added in
/// `b`, or removed from `a`). A `None` root is an empty tree.
pub fn diff<S: ObjectStore>(
    store: &S,
    a: Option<&Digest>,
    b: Option<&Digest>,
) -> Result<Vec<DiffEntry>> {
    let a_nodes: BTreeSet<Digest> = match a {
        Some(r) => reachable_nodes(store, r)?.into_iter().collect(),
        None => BTreeSet::new(),
    };
    let b_nodes: BTreeSet<Digest> = match b {
        Some(r) => reachable_nodes(store, r)?.into_iter().collect(),
        None => BTreeSet::new(),
    };
    // Entries of each side that live in leaves not shared with the other side. A key in a shared
    // (pruned) leaf is identical on both sides, so it never appears here.
    let mut am: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    if let Some(r) = a {
        changed_entries(store, r, &b_nodes, 0, &mut am)?;
    }
    let mut bm: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    if let Some(r) = b {
        changed_entries(store, r, &a_nodes, 0, &mut bm)?;
    }
    let am: BTreeMap<Vec<u8>, Vec<u8>> = am.into_iter().collect();
    let bm: BTreeMap<Vec<u8>, Vec<u8>> = bm.into_iter().collect();
    let mut out = Vec::new();
    for key in am.keys().chain(bm.keys()).cloned().collect::<BTreeSet<_>>() {
        let av = am.get(&key);
        let bv = bm.get(&key);
        if av == bv {
            continue; // present in both non-shared leaves with the same value: not a real change
        }
        out.push((key, av.cloned(), bv.cloned()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::memory::MemoryStore;
    use std::collections::BTreeSet;

    // `n` deterministic, distinct (key, value) pairs.
    fn entries(n: u64) -> Vec<(Vec<u8>, Vec<u8>)> {
        (0..n)
            .map(|i| {
                let k = format!("key-{i:08}").into_bytes();
                let v = format!("val-{i}").into_bytes();
                (k, v)
            })
            .collect()
    }

    #[test]
    fn root_is_history_independent() {
        // Build from the entries in two different insertion orders (both sorted before build); the
        // root digest must be identical: the tree is a function of the set, not the order.
        let mut sorted = entries(5_000);
        sorted.sort();
        let mut store_a = MemoryStore::new();
        let root_a = build(&mut store_a, &sorted).unwrap().unwrap();

        // Insert into a set in a scrambled order, then sort -> same sequence -> same root.
        let mut shuffled = entries(5_000);
        shuffled.sort_by_key(|(k, _)| {
            let h = Digest::blake3(k);
            u64::from_le_bytes(h.bytes()[..8].try_into().unwrap())
        });
        let mut set: BTreeSet<(Vec<u8>, Vec<u8>)> = BTreeSet::new();
        for e in shuffled {
            set.insert(e);
        }
        let resorted: Vec<_> = set.into_iter().collect();
        let mut store_b = MemoryStore::new();
        let root_b = build(&mut store_b, &resorted).unwrap().unwrap();

        assert_eq!(root_a, root_b, "same set must yield the same root");
    }

    /// Drain a cursor into a Vec.
    fn drain<S: ObjectStore>(mut c: ProllyCursor<'_, S>) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut out = Vec::new();
        while let Some(kv) = c.next().unwrap() {
            out.push(kv);
        }
        out
    }

    #[test]
    fn cursor_full_scan_matches_entries() {
        // The lazy cursor yields exactly the same ascending sequence as the materializing `entries`,
        // over a tree deep enough to have many internal nodes.
        let data = entries(4_000);
        let mut store = MemoryStore::new();
        let root = build(&mut store, &data).unwrap().unwrap();
        let streamed = drain(ProllyCursor::open(&store, &root).unwrap());
        // `entries` here is the test's `(key, value)` generator; the materializing scan is `super::entries`.
        assert_eq!(streamed, super::entries(&store, &root).unwrap());
        assert_eq!(streamed, data);
    }

    #[test]
    fn cursor_range_and_prefix_match_scan_prefix() {
        let data = entries(2_000);
        let mut store = MemoryStore::new();
        let root = build(&mut store, &data).unwrap().unwrap();

        // Inclusive start / exclusive upper range == the matching slice of the sorted data.
        let start = b"key-00000500";
        let upper = b"key-00001500".to_vec();
        let ranged =
            drain(ProllyCursor::open_range(&store, &root, Some(start), Some(upper)).unwrap());
        let expected: Vec<_> = data
            .iter()
            .filter(|(k, _)| k.as_slice() >= &start[..] && k.as_slice() < &b"key-00001500"[..])
            .cloned()
            .collect();
        assert_eq!(ranged, expected);
        assert_eq!(ranged.len(), 1000);

        // Prefix cursor == scan_prefix.
        let prefix = b"key-000001";
        let by_cursor = drain(ProllyCursor::open_prefix(&store, &root, prefix).unwrap());
        assert_eq!(by_cursor, scan_prefix(&store, &root, prefix).unwrap());
        assert!(!by_cursor.is_empty());
    }

    #[test]
    fn cursor_empty_and_single_leaf() {
        let mut store = MemoryStore::new();
        // Single-leaf tree (tiny).
        let small = entries(3);
        let root = build(&mut store, &small).unwrap().unwrap();
        assert_eq!(drain(ProllyCursor::open(&store, &root).unwrap()), small);
        // A start beyond all keys yields nothing.
        let none = drain(ProllyCursor::open_range(&store, &root, Some(b"zzz"), None).unwrap());
        assert!(none.is_empty());
    }

    #[test]
    fn every_key_round_trips() {
        let data = entries(3_000);
        let mut store = MemoryStore::new();
        let root = build(&mut store, &data).unwrap().unwrap();
        for (k, v) in &data {
            assert_eq!(
                get(&store, &root, k).unwrap().as_deref(),
                Some(v.as_slice())
            );
        }
        // A key that isn't present.
        assert_eq!(get(&store, &root, b"key-99999999").unwrap(), None);
        assert_eq!(get(&store, &root, b"aaa-before-all").unwrap(), None);
    }

    #[test]
    fn insert_remove_match_full_build() {
        // Thousands of pseudo-random inserts/replaces/removes (including removing boundary keys, which
        // exercises the rebuild fallback), asserting the incrementally-mutated root is byte-identical
        // to a full `build` over the equivalent key set at every checkpoint - the history-independence
        // contract.
        let mut store = MemoryStore::new();
        let mut model: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let mut root: Option<Digest> = None;
        let mut seed = 0x1234_5678_9abc_def0u64;
        let mut rng = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for step in 0..6000u32 {
            let k = format!("k-{:05}", rng() % 1500).into_bytes();
            if rng() % 3 == 0 {
                model.remove(&k);
                root = match &root {
                    Some(r) => remove(&mut store, r, &k).unwrap(),
                    None => None,
                };
            } else {
                let v = format!("v-{}", rng() % 100).into_bytes();
                model.insert(k.clone(), v.clone());
                root = Some(insert(&mut store, root.as_ref(), &k, &v).unwrap());
            }
            if step % 20 == 0 {
                let entries: Vec<_> = model.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                let built = build(&mut store, &entries).unwrap();
                assert_eq!(
                    root,
                    built,
                    "incremental root != build at step {step} (n={})",
                    model.len()
                );
            }
        }
        let entries: Vec<_> = model.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        assert_eq!(root, build(&mut store, &entries).unwrap());
        if let Some(r) = &root {
            for (k, v) in &model {
                assert_eq!(get(&store, r, k).unwrap().as_deref(), Some(v.as_slice()));
            }
        }
    }

    #[test]
    fn incremental_insert_shares_most_nodes() {
        // Inserting one key into a large tree re-chunks only the affected leaf + spine; the vast
        // majority of node digests are unchanged.
        let mut data = entries(5_000);
        data.sort();
        let mut store = MemoryStore::new();
        let root1 = build(&mut store, &data).unwrap().unwrap();
        let nodes1: BTreeSet<Digest> = reachable_nodes(&store, &root1)
            .unwrap()
            .into_iter()
            .collect();

        let root2 = insert(&mut store, Some(&root1), b"key-00002500-x", b"inserted").unwrap();
        let nodes2: BTreeSet<Digest> = reachable_nodes(&store, &root2)
            .unwrap()
            .into_iter()
            .collect();

        assert_ne!(root1, root2);
        let shared = nodes1.intersection(&nodes2).count();
        let changed = nodes2.difference(&nodes1).count();
        assert!(
            shared > changed * 4,
            "expected most nodes shared after one insert: shared={shared}, changed={changed}"
        );
    }

    #[test]
    fn scan_prefix_returns_only_matching_keys() {
        // Keys "aa-0000".."aa-2999" and "bb-0000".."bb-0999"; a prefix scan returns exactly one group,
        // in ascending order, across many leaves/levels (so internal pruning is exercised).
        let mut data: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for i in 0..3_000u32 {
            data.push((format!("aa-{i:04}").into_bytes(), b"a".to_vec()));
        }
        for i in 0..1_000u32 {
            data.push((format!("bb-{i:04}").into_bytes(), b"b".to_vec()));
        }
        data.sort();
        let mut store = MemoryStore::new();
        let root = build(&mut store, &data).unwrap().unwrap();

        let aa = scan_prefix(&store, &root, b"aa-").unwrap();
        assert_eq!(aa.len(), 3_000);
        assert!(aa.iter().all(|(k, _)| k.starts_with(b"aa-")));
        assert!(aa.windows(2).all(|w| w[0].0 < w[1].0), "ascending");

        let bb = scan_prefix(&store, &root, b"bb-").unwrap();
        assert_eq!(bb.len(), 1_000);
        assert!(bb.iter().all(|(k, _)| k.starts_with(b"bb-")));

        // A narrower prefix and a no-match prefix.
        assert_eq!(scan_prefix(&store, &root, b"aa-00").unwrap().len(), 100);
        assert!(scan_prefix(&store, &root, b"zz-").unwrap().is_empty());
        // Empty prefix returns the whole set.
        assert_eq!(scan_prefix(&store, &root, b"").unwrap().len(), 4_000);
    }

    #[test]
    fn one_change_shares_most_nodes() {
        // Changing a single value re-chunks only its leaf + the spine to the root; the vast majority
        // of node digests are unchanged (structural sharing gives cheap diff/sync).
        let mut data = entries(5_000);
        data.sort();
        let mut store = MemoryStore::new();
        let root1 = build(&mut store, &data).unwrap().unwrap();
        let nodes1: BTreeSet<Digest> = reachable_nodes(&store, &root1)
            .unwrap()
            .into_iter()
            .collect();

        // Change one value in the middle.
        data[2_500].1 = b"CHANGED".to_vec();
        let root2 = build(&mut store, &data).unwrap().unwrap();
        let nodes2: BTreeSet<Digest> = reachable_nodes(&store, &root2)
            .unwrap()
            .into_iter()
            .collect();

        assert_ne!(root1, root2);
        let shared = nodes1.intersection(&nodes2).count();
        let changed = nodes1.symmetric_difference(&nodes2).count();
        assert!(
            shared > changed * 4,
            "expected most nodes shared after a 1-entry change: shared={shared}, changed={changed}"
        );
    }
}
