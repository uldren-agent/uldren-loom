//! Copy-on-write B-tree for the object-location index, resident on index pages: maps a 32-byte digest
//! to the [`RecordLoc`] of its record. Each node occupies one page, addressed by [`PageId`]. Nodes are
//! immutable once written; an insert appends fresh copies of the root-to-leaf path and frees the pages
//! it supersedes, and the caller swaps the root via a generation bump, so a crash can never corrupt a
//! committed index.
//!
//! On-disk node layout (little-endian), one node per page, CRC-32C over the page minus its last 4
//! bytes:
//! ```text
//!   [0]        NODE_MAGIC
//!   [1]        flags: bit0 = is_leaf
//!   [2,4)      u16 n  (entry count)
//!   [4, ..)    n * { key[32], RecordLoc (uvarints) }      (sorted by key)
//!   (internal) (n+1) * { child PageId u64 }
//!   [PAGE-4, PAGE) crc32c over [0, PAGE-4)
//! ```

use crate::page::{PAGE_SIZE, PageId};
use crate::pagemap::PageAllocator;
use crate::record::RecordLoc;
use crate::{BackingIo, corrupt, crc32c, io_err, read_exact_at, write_at};
use loom_core::error::Result;
#[cfg(test)]
use std::fs::File;

const NODE_MAGIC: u8 = 0xB7;
const T: usize = 32; // min degree
const MAX_ENTRIES: usize = 2 * T - 1; // 63 keys before a node splits
const MAX_DEPTH: usize = 32; // crafted-tree guard: a real order-64 tree is far shallower than this
const PAGE: usize = PAGE_SIZE as usize;
const CRC: usize = 4;
const BODY_END: usize = PAGE - CRC;

struct Node {
    is_leaf: bool,
    entries: Vec<([u8; 32], RecordLoc)>, // sorted by key
    children: Vec<PageId>,               // empty for a leaf; otherwise entries.len() + 1
}

impl Node {
    fn leaf(entries: Vec<([u8; 32], RecordLoc)>) -> Self {
        Self {
            is_leaf: true,
            entries,
            children: Vec::new(),
        }
    }

    /// Lay the node out into a full page. Errors only if the entries overflow one page, which a
    /// `MAX_ENTRIES`-bounded node never does; the check guards against a logic bug, not valid input.
    fn encode(&self) -> Result<[u8; PAGE]> {
        let mut body = Vec::with_capacity(PAGE);
        body.push(NODE_MAGIC);
        body.push(u8::from(self.is_leaf));
        body.extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
        for (k, v) in &self.entries {
            body.extend_from_slice(k);
            v.encode(&mut body);
        }
        if !self.is_leaf {
            for c in &self.children {
                body.extend_from_slice(&c.0.to_le_bytes());
            }
        }
        if body.len() > BODY_END {
            return Err(corrupt("btree node exceeds one page"));
        }
        let mut page = [0u8; PAGE];
        page[..body.len()].copy_from_slice(&body);
        let crc = crc32c(&page[..BODY_END]);
        page[BODY_END..].copy_from_slice(&crc.to_le_bytes());
        Ok(page)
    }
}

/// Merge `left`, the separator `sep`, and `right` into one node holding all their entries and
/// children. With two minimal (T-1) neighbours this yields exactly `MAX_ENTRIES` entries, so the
/// result still fits one page.
fn merge_nodes(mut left: Node, sep: ([u8; 32], RecordLoc), mut right: Node) -> Node {
    left.entries.push(sep);
    left.entries.append(&mut right.entries);
    if !left.is_leaf {
        left.children.append(&mut right.children);
    }
    left
}

/// Rotate `left`'s last entry up into the parent and the parent's `sep` down into `child`'s front,
/// returning `(new_left, entry_rotated_up, new_child)`. `left` must hold more than the minimum.
fn borrow_from_left(
    mut left: Node,
    sep: ([u8; 32], RecordLoc),
    mut child: Node,
) -> (Node, ([u8; 32], RecordLoc), Node) {
    let up = left.entries.pop().expect("left sibling has a spare entry");
    child.entries.insert(0, sep);
    if !child.is_leaf {
        let moved = left.children.pop().expect("internal sibling has a child");
        child.children.insert(0, moved);
    }
    (left, up, child)
}

/// Rotate `right`'s first entry up into the parent and the parent's `sep` down into `child`'s back,
/// returning `(new_child, new_right, entry_rotated_up)`. `right` must hold more than the minimum.
fn borrow_from_right(
    mut child: Node,
    sep: ([u8; 32], RecordLoc),
    mut right: Node,
) -> (Node, Node, ([u8; 32], RecordLoc)) {
    child.entries.push(sep);
    if !child.is_leaf {
        let moved = right.children.remove(0);
        child.children.push(moved);
    }
    let up = right.entries.remove(0);
    (child, right, up)
}

/// Result of inserting into a subtree: either a single new subtree root, or a split that promotes a
/// separator key/value and two new children up to the parent.
enum Ins {
    Done(PageId),
    Split {
        key: [u8; 32],
        val: RecordLoc,
        left: PageId,
        right: PageId,
    },
}

/// One tree operation's working context: the file, the allocator handing out node pages, the byte
/// header preceding the page array, and `page_count` (the allocated-page count before this operation,
/// the bound for reading existing immutable nodes). Threading it as `self` keeps the recursion's
/// signatures small.
struct Tree<'a> {
    file: &'a mut dyn BackingIo,
    cur: &'a mut PageAllocator,
    header_len: u64,
    page_count: u64,
}

impl Tree<'_> {
    /// Read and validate the node on `page`. `page_count` bounds the read so a crafted page id, an
    /// entry count beyond the structural max, or a truncated body is a clean CORRUPT error.
    fn read(&mut self, page: PageId) -> Result<Node> {
        if page.0 >= self.page_count {
            return Err(corrupt("btree node page out of range"));
        }
        let mut buf = [0u8; PAGE];
        read_exact_at(self.file, page.offset(self.header_len), &mut buf)
            .map_err(|_| corrupt("truncated btree node page"))?;
        decode_node_page(&buf)
    }

    /// Allocate one page for `node`, write it there, and return its page id.
    fn write(&mut self, node: &Node) -> Result<PageId> {
        let page = node.encode()?;
        let pid = self.cur.alloc(1);
        write_at(self.file, pid.offset(self.header_len), &page).map_err(io_err)?;
        Ok(pid)
    }

    fn insert_node(
        &mut self,
        page: PageId,
        key: &[u8; 32],
        value: RecordLoc,
        depth: usize,
    ) -> Result<Ins> {
        if depth > MAX_DEPTH {
            return Err(corrupt("btree deeper than the structural maximum"));
        }
        let mut node = self.read(page)?;
        // This node is about to be copied-on-write; its page becomes reclaimable. Record it before
        // mutating.
        self.cur.free(page, 1);
        match node
            .entries
            .binary_search_by(|(k, _)| k.as_slice().cmp(key.as_slice()))
        {
            Ok(i) => {
                node.entries[i].1 = value;
                Ok(Ins::Done(self.write(&node)?))
            }
            Err(i) => {
                if node.is_leaf {
                    node.entries.insert(i, (*key, value));
                    self.emit(node)
                } else {
                    let child = node.children[i];
                    match self.insert_node(child, key, value, depth + 1)? {
                        Ins::Done(new_child) => {
                            node.children[i] = new_child;
                            self.emit(node)
                        }
                        Ins::Split {
                            key: sk,
                            val: sv,
                            left,
                            right,
                        } => {
                            node.entries.insert(i, (sk, sv));
                            node.children[i] = left;
                            node.children.insert(i + 1, right);
                            self.emit(node)
                        }
                    }
                }
            }
        }
    }

    #[cfg(test)]
    fn get_node(
        &mut self,
        page: PageId,
        key: &[u8; 32],
        depth: usize,
    ) -> Result<Option<RecordLoc>> {
        if depth > MAX_DEPTH {
            return Err(corrupt("btree depth exceeds limit"));
        }
        let node = self.read(page)?;
        match node.entries.binary_search_by(|(k, _)| k.cmp(key)) {
            Ok(i) => Ok(Some(node.entries[i].1)),
            Err(_) if node.is_leaf => Ok(None),
            Err(i) => self.get_node(node.children[i], key, depth + 1),
        }
    }

    /// Write `node`, splitting at the median if the insert overflowed it (entries == 2T).
    fn emit(&mut self, mut node: Node) -> Result<Ins> {
        if node.entries.len() <= MAX_ENTRIES {
            return Ok(Ins::Done(self.write(&node)?));
        }
        let right_entries = node.entries.split_off(T); // entries[T..]
        let median = node.entries.pop().expect("median present"); // entries[T-1]
        let right_children = if node.is_leaf {
            Vec::new()
        } else {
            node.children.split_off(T) // children[T..], left keeps children[0, T)
        };
        let left = Node {
            is_leaf: node.is_leaf,
            entries: node.entries,
            children: node.children,
        };
        let right = Node {
            is_leaf: left.is_leaf,
            entries: right_entries,
            children: right_children,
        };
        let left_page = self.write(&left)?;
        let right_page = self.write(&right)?;
        Ok(Ins::Split {
            key: median.0,
            val: median.1,
            left: left_page,
            right: right_page,
        })
    }

    fn walk(
        &mut self,
        page: PageId,
        depth: usize,
        out: &mut Vec<([u8; 32], RecordLoc)>,
    ) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(corrupt("btree deeper than the structural maximum"));
        }
        let node = self.read(page)?;
        if node.is_leaf {
            out.extend(node.entries.iter().copied());
        } else {
            for i in 0..node.entries.len() {
                self.walk(node.children[i], depth + 1, out)?;
                out.push(node.entries[i]);
            }
            self.walk(node.children[node.entries.len()], depth + 1, out)?;
        }
        Ok(())
    }

    /// Build the internal levels above a finished level of `children` separated by `seps`
    /// (`seps.len() == children.len() - 1`), one node per group, recursing until a single root remains.
    /// Only reached via [`build_packed`] (compaction), so cfg-gated off for wasm32 with it.
    #[cfg(not(target_arch = "wasm32"))]
    fn build_up(
        &mut self,
        seps: Vec<([u8; 32], RecordLoc)>,
        children: Vec<PageId>,
    ) -> Result<PageId> {
        if seps.len() <= MAX_ENTRIES {
            return self.write(&Node {
                is_leaf: false,
                entries: seps,
                children,
            });
        }
        let cap = MAX_ENTRIES + 1; // max children per internal node
        let c = children.len();
        let p = c.div_ceil(cap); // >= 2 (c > cap when seps.len() > MAX_ENTRIES)
        let base = c / p;
        let extra = c % p; // the first `extra` groups get one more child
        let mut cidx = 0usize;
        let mut sidx = 0usize;
        let mut up_children = Vec::with_capacity(p);
        let mut up_seps: Vec<([u8; 32], RecordLoc)> = Vec::with_capacity(p - 1);
        for gi in 0..p {
            let cnt = base + usize::from(gi < extra); // children in this group (>= 2)
            let group_children = children[cidx..cidx + cnt].to_vec();
            let group_seps = seps[sidx..sidx + (cnt - 1)].to_vec();
            sidx += cnt - 1;
            cidx += cnt;
            up_children.push(self.write(&Node {
                is_leaf: false,
                entries: group_seps,
                children: group_children,
            })?);
            if gi < p - 1 {
                up_seps.push(seps[sidx]);
                sidx += 1;
            }
        }
        debug_assert_eq!(cidx, c);
        debug_assert_eq!(sidx, seps.len());
        self.build_up(up_seps, up_children)
    }

    /// The largest entry in the subtree at `page` (its rightmost leaf entry).
    fn max_entry(&mut self, page: PageId, depth: usize) -> Result<([u8; 32], RecordLoc)> {
        if depth > MAX_DEPTH {
            return Err(corrupt("btree deeper than the structural maximum"));
        }
        let node = self.read(page)?;
        match node.children.last() {
            Some(&c) => self.max_entry(c, depth + 1),
            None => node
                .entries
                .last()
                .copied()
                .ok_or_else(|| corrupt("empty btree leaf")),
        }
    }

    /// The smallest entry in the subtree at `page` (its leftmost leaf entry).
    fn min_entry(&mut self, page: PageId, depth: usize) -> Result<([u8; 32], RecordLoc)> {
        if depth > MAX_DEPTH {
            return Err(corrupt("btree deeper than the structural maximum"));
        }
        let node = self.read(page)?;
        match node.children.first() {
            Some(&c) => self.min_entry(c, depth + 1),
            None => node
                .entries
                .first()
                .copied()
                .ok_or_else(|| corrupt("empty btree leaf")),
        }
    }

    /// Delete `key` from the in-memory subtree `node`, whose own old page the caller has already freed
    /// and will rewrite. Maintains the B-tree min-degree invariant: a child is brought to at least T
    /// keys (by borrowing from or merging with a sibling) before the deletion descends into it, so the
    /// returned node never underflows below the minimum (only the root may). Children that change are
    /// read, their old pages freed, and their new pages written here; unchanged children keep their ids.
    fn delete_in(&mut self, mut node: Node, key: &[u8; 32], depth: usize) -> Result<Node> {
        if depth > MAX_DEPTH {
            return Err(corrupt("btree deeper than the structural maximum"));
        }
        match node
            .entries
            .binary_search_by(|(k, _)| k.as_slice().cmp(key.as_slice()))
        {
            Ok(i) if node.is_leaf => {
                node.entries.remove(i);
            }
            Ok(i) => {
                // Internal node holds `key` as the separator between children i and i+1.
                let left_p = node.children[i];
                let right_p = node.children[i + 1];
                let left = self.read(left_p)?;
                if left.entries.len() >= T {
                    // Replace `key` with its predecessor and delete that from the left subtree.
                    let pred = self.max_entry(left_p, depth + 1)?;
                    node.entries[i] = pred;
                    self.cur.free(left_p, 1);
                    let new_left = self.delete_in(left, &pred.0, depth + 1)?;
                    node.children[i] = self.write(&new_left)?;
                } else {
                    let right = self.read(right_p)?;
                    if right.entries.len() >= T {
                        // Replace `key` with its successor and delete that from the right subtree.
                        let succ = self.min_entry(right_p, depth + 1)?;
                        node.entries[i] = succ;
                        self.cur.free(right_p, 1);
                        let new_right = self.delete_in(right, &succ.0, depth + 1)?;
                        node.children[i + 1] = self.write(&new_right)?;
                    } else {
                        // Both neighbours are minimal: merge left + separator + right, then delete
                        // `key` from the merged child.
                        self.cur.free(left_p, 1);
                        self.cur.free(right_p, 1);
                        let sep = node.entries.remove(i);
                        node.children.remove(i + 1);
                        let merged = merge_nodes(left, sep, right);
                        let new_child = self.delete_in(merged, key, depth + 1)?;
                        node.children[i] = self.write(&new_child)?;
                    }
                }
            }
            Err(_) if node.is_leaf => {} // key absent: nothing to do
            Err(i) => {
                // Descend into child i; first bring it to at least T keys.
                let (child, slot) = self.fix_descend(&mut node, i)?;
                let new_child = self.delete_in(child, key, depth + 1)?;
                node.children[slot] = self.write(&new_child)?;
            }
        }
        Ok(node)
    }

    /// Bring the child at index `i` to at least T keys before descending into it, borrowing from a
    /// sibling that has a spare key or, failing that, merging with one. Reads the child (freeing its
    /// old page) and any sibling touched (rewriting or freeing it), updates `node`, and returns the
    /// in-memory child to descend into plus its slot in `node.children`.
    fn fix_descend(&mut self, node: &mut Node, i: usize) -> Result<(Node, usize)> {
        let child_p = node.children[i];
        let child = self.read(child_p)?;
        self.cur.free(child_p, 1);
        if child.entries.len() >= T {
            return Ok((child, i));
        }
        if i > 0 {
            let left_p = node.children[i - 1];
            let left = self.read(left_p)?;
            if left.entries.len() >= T {
                self.cur.free(left_p, 1);
                let (new_left, up, fixed) = borrow_from_left(left, node.entries[i - 1], child);
                node.entries[i - 1] = up;
                node.children[i - 1] = self.write(&new_left)?;
                return Ok((fixed, i));
            }
        }
        if i + 1 < node.children.len() {
            let right_p = node.children[i + 1];
            let right = self.read(right_p)?;
            if right.entries.len() >= T {
                self.cur.free(right_p, 1);
                let (fixed, new_right, up) = borrow_from_right(child, node.entries[i], right);
                node.entries[i] = up;
                node.children[i + 1] = self.write(&new_right)?;
                return Ok((fixed, i));
            }
        }
        // No sibling has a spare key: merge with one, collapsing a separator out of `node`.
        if i > 0 {
            let left_p = node.children[i - 1];
            let left = self.read(left_p)?;
            self.cur.free(left_p, 1);
            let sep = node.entries.remove(i - 1);
            node.children.remove(i);
            Ok((merge_nodes(left, sep, child), i - 1))
        } else {
            let right_p = node.children[i + 1];
            let right = self.read(right_p)?;
            self.cur.free(right_p, 1);
            let sep = node.entries.remove(i);
            node.children.remove(i + 1);
            Ok((merge_nodes(child, sep, right), i))
        }
    }
}

fn get_with_page_reader_inner(
    page_count: u64,
    read_page: &mut impl FnMut(PageId) -> Result<[u8; PAGE]>,
    page: PageId,
    key: &[u8; 32],
    depth: usize,
) -> Result<Option<RecordLoc>> {
    if depth > MAX_DEPTH {
        return Err(corrupt("btree depth exceeds limit"));
    }
    if page.0 >= page_count {
        return Err(corrupt("btree node page out of range"));
    }
    let raw = read_page(page)?;
    let node = decode_node_page(&raw)?;
    match node.entries.binary_search_by(|(k, _)| k.cmp(key)) {
        Ok(i) => Ok(Some(node.entries[i].1)),
        Err(_) if node.is_leaf => Ok(None),
        Err(i) => {
            get_with_page_reader_inner(page_count, read_page, node.children[i], key, depth + 1)
        }
    }
}

fn decode_node_page(buf: &[u8; PAGE]) -> Result<Node> {
    let stored = u32::from_le_bytes(buf[BODY_END..].try_into().unwrap());
    if crc32c(&buf[..BODY_END]) != stored {
        return Err(corrupt("btree node crc mismatch"));
    }
    if buf[0] != NODE_MAGIC {
        return Err(corrupt("bad btree node magic"));
    }
    let is_leaf = buf[1] & 1 == 1;
    let n = u16::from_le_bytes([buf[2], buf[3]]) as usize;
    if n == 0 || n > MAX_ENTRIES {
        return Err(corrupt("btree node entry count out of range"));
    }
    let mut pos = 4;
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        if pos + 32 > BODY_END {
            return Err(corrupt("btree node truncated key"));
        }
        let mut k = [0u8; 32];
        k.copy_from_slice(&buf[pos..pos + 32]);
        pos += 32;
        let v = RecordLoc::decode(&buf[..BODY_END], &mut pos)
            .ok_or_else(|| corrupt("btree node bad locator"))?;
        entries.push((k, v));
    }
    let mut children = Vec::new();
    if !is_leaf {
        children.reserve(n + 1);
        for _ in 0..n + 1 {
            if pos + 8 > BODY_END {
                return Err(corrupt("btree node truncated child"));
            }
            children.push(PageId(u64::from_le_bytes(
                buf[pos..pos + 8].try_into().unwrap(),
            )));
            pos += 8;
        }
    }
    Ok(Node {
        is_leaf,
        entries,
        children,
    })
}

fn walk_with_page_reader(
    page_count: u64,
    read_page: &mut impl FnMut(PageId) -> Result<[u8; PAGE]>,
    page: PageId,
    depth: usize,
    out: &mut Vec<([u8; 32], RecordLoc)>,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(corrupt("btree deeper than the structural maximum"));
    }
    if page.0 >= page_count {
        return Err(corrupt("btree node page out of range"));
    }
    let buf = read_page(page)?;
    let node = decode_node_page(&buf)?;
    if node.is_leaf {
        out.extend(node.entries.iter().copied());
    } else {
        for i in 0..node.entries.len() {
            walk_with_page_reader(page_count, read_page, node.children[i], depth + 1, out)?;
            out.push(node.entries[i]);
        }
        walk_with_page_reader(
            page_count,
            read_page,
            node.children[node.entries.len()],
            depth + 1,
            out,
        )?;
    }
    Ok(())
}

/// CoW-insert `(key, value)` into the tree rooted at `root` (None = empty), allocating new node pages
/// via `cur` and freeing the pages it supersedes, and return the new root page. `page_count` is the
/// allocated-page count *before* this insert: the bound for reading the existing (immutable) nodes.
pub(crate) fn insert(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    root: Option<PageId>,
    key: &[u8; 32],
    value: RecordLoc,
    page_count: u64,
) -> Result<PageId> {
    let mut t = Tree {
        file,
        cur,
        header_len,
        page_count,
    };
    match root {
        None => t.write(&Node::leaf(vec![(*key, value)])),
        Some(r) => match t.insert_node(r, key, value, 0)? {
            Ins::Done(p) => Ok(p),
            Ins::Split {
                key: k,
                val,
                left,
                right,
            } => t.write(&Node {
                is_leaf: false,
                entries: vec![(k, val)],
                children: vec![left, right],
            }),
        },
    }
}

/// CoW-delete `key` from the tree rooted at `root` (None = empty), allocating new node pages via `cur`
/// and freeing the pages it supersedes, and return the new root page (None if the tree became empty).
/// `page_count` is the allocated-page count *before* this delete: the bound for reading existing nodes.
/// Deleting an absent key still rewrites the root-to-leaf path (a harmless no-op for the index).
pub(crate) fn delete(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    root: Option<PageId>,
    key: &[u8; 32],
    page_count: u64,
) -> Result<Option<PageId>> {
    let Some(r) = root else {
        return Ok(None);
    };
    let mut t = Tree {
        file,
        cur,
        header_len,
        page_count,
    };
    let node = t.read(r)?;
    t.cur.free(r, 1);
    let new_root = t.delete_in(node, key, 0)?;
    if new_root.entries.is_empty() {
        // The root emptied: an internal root with a single remaining child collapses to that child; an
        // empty leaf root means the tree is now empty.
        match new_root.children.first() {
            Some(&only_child) => Ok(Some(only_child)),
            None => Ok(None),
        }
    } else {
        Ok(Some(t.write(&new_root)?))
    }
}

#[cfg(test)]
pub(crate) fn get(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: Option<PageId>,
    key: &[u8; 32],
    page_count: u64,
) -> Result<Option<RecordLoc>> {
    let Some(root) = root else {
        return Ok(None);
    };
    let mut t = Tree {
        file,
        cur: &mut PageAllocator::new(page_count, 0, Vec::new()),
        header_len,
        page_count,
    };
    t.get_node(root, key, 0)
}

pub(crate) fn get_with_page_reader(
    root: Option<PageId>,
    key: &[u8; 32],
    page_count: u64,
    mut read_page: impl FnMut(PageId) -> Result<[u8; PAGE]>,
) -> Result<Option<RecordLoc>> {
    let Some(root) = root else {
        return Ok(None);
    };
    get_with_page_reader_inner(page_count, &mut read_page, root, key, 0)
}

/// Walk the whole tree rooted at `root` and return every `(key, locator)` entry, in ascending key
/// order. Used on open to rebuild the in-memory index without scanning object payloads.
pub(crate) fn load_all(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    let mut t = Tree {
        file,
        cur: &mut PageAllocator::new(page_count, 0, Vec::new()),
        header_len,
        page_count,
    };
    let mut out = Vec::new();
    t.walk(root, 0, &mut out)?;
    Ok(out)
}

pub(crate) fn load_all_with_page_reader(
    root: PageId,
    page_count: u64,
    mut read_page: impl FnMut(PageId) -> Result<[u8; PAGE]>,
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    let mut out = Vec::new();
    walk_with_page_reader(page_count, &mut read_page, root, 0, &mut out)?;
    Ok(out)
}

/// Bulk-build a balanced B-tree from `entries` (sorted ascending and unique), writing each node
/// exactly once via `cur`, and return the new root page (None if empty). Used by compaction, where
/// per-key [`insert`] would reproduce the copy-on-write churn it is reclaiming. Compaction is
/// native-file-only, so this (and its helper `build_up`) is cfg-gated off for wasm32.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn build_packed(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    entries: &[([u8; 32], RecordLoc)],
) -> Result<Option<PageId>> {
    if entries.is_empty() {
        return Ok(None);
    }
    let page_count = cur.page_count();
    let mut t = Tree {
        file,
        cur,
        header_len,
        page_count,
    };
    if entries.len() <= MAX_ENTRIES {
        return Ok(Some(t.write(&Node::leaf(entries.to_vec()))?));
    }
    let s = MAX_ENTRIES;
    let n = entries.len();
    let m = (n + 1).div_ceil(s + 1); // >= 2 here
    let leaf_total = n - (m - 1);
    let base = leaf_total / m;
    let extra = leaf_total % m; // the first `extra` leaves get one more entry
    let mut idx = 0usize;
    let mut children = Vec::with_capacity(m);
    let mut seps: Vec<([u8; 32], RecordLoc)> = Vec::with_capacity(m - 1);
    for li in 0..m {
        let cnt = base + usize::from(li < extra);
        children.push(t.write(&Node::leaf(entries[idx..idx + cnt].to_vec()))?);
        idx += cnt;
        if li < m - 1 {
            seps.push(entries[idx]);
            idx += 1;
        }
    }
    debug_assert_eq!(idx, n);
    t.build_up(seps, children).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    const HEADER: u64 = 3 * PAGE_SIZE;

    struct Scratch(std::path::PathBuf, File);
    impl Scratch {
        fn new() -> Self {
            static C: AtomicU64 = AtomicU64::new(0);
            let mut p = std::env::temp_dir();
            let n = C.fetch_add(1, Ordering::Relaxed);
            p.push(format!("loom-pagebtree-{}-{n}.bin", std::process::id()));
            let _ = std::fs::remove_file(&p);
            let f = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(&p)
                .unwrap();
            Self(p, f)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn key(i: u64) -> [u8; 32] {
        let mut k = [0u8; 32];
        k[..8].copy_from_slice(&i.wrapping_mul(0x9E37_79B9_7F4A_7C15).to_le_bytes());
        k[8..16].copy_from_slice(&(!i).to_le_bytes());
        k[16..24].copy_from_slice(&i.to_be_bytes());
        k
    }

    fn loc(i: u64) -> RecordLoc {
        RecordLoc::from_global(i, (i % 97) as u32)
    }

    #[test]
    fn deep_tree_inserts_and_loads_every_key() {
        let mut s = Scratch::new();
        let n = 5_000u64; // forces leaf splits, internal splits, and at least one root split
        let mut root: Option<PageId> = None;
        let mut cur = PageAllocator::new(0, 0, Vec::new());
        let mut expect = BTreeMap::new();
        for i in 0..n {
            let k = key(i);
            let v = loc(i);
            let bound = cur.page_count();
            root = Some(insert(&mut s.1, HEADER, &mut cur, root, &k, v, bound).unwrap());
            expect.insert(k, v);
        }
        let all = load_all(&mut s.1, HEADER, root.unwrap(), cur.page_count()).unwrap();
        assert_eq!(
            all.len(),
            expect.len(),
            "lost or duplicated keys across splits"
        );
        let mut prev: Option<[u8; 32]> = None;
        for (k, _) in &all {
            if let Some(p) = prev {
                assert!(p < *k, "load_all not in ascending key order");
            }
            prev = Some(*k);
        }
        assert_eq!(all.into_iter().collect::<BTreeMap<_, _>>(), expect);
        for i in [0, 1, 63, 64, 255, 1024, n - 1] {
            let k = key(i);
            assert_eq!(
                get(&mut s.1, HEADER, root, &k, cur.page_count()).unwrap(),
                Some(loc(i))
            );
        }
        let mut reads = 0u64;
        for i in [0, 64, 1024, n - 1] {
            let k = key(i);
            assert_eq!(
                get_with_page_reader(root, &k, cur.page_count(), |page| {
                    reads += 1;
                    let mut buf = [0u8; PAGE];
                    read_exact_at(&mut s.1, page.offset(HEADER), &mut buf).map_err(io_err)?;
                    Ok(buf)
                })
                .unwrap(),
                Some(loc(i))
            );
        }
        assert!(reads > 0);
        assert_eq!(
            get(&mut s.1, HEADER, root, &[0xFF; 32], cur.page_count()).unwrap(),
            None
        );
    }

    #[test]
    fn reinserting_a_key_replaces_its_locator() {
        let mut s = Scratch::new();
        let mut root: Option<PageId> = None;
        let mut cur = PageAllocator::new(0, 0, Vec::new());
        let k = key(42);
        let b = cur.page_count();
        root = Some(insert(&mut s.1, HEADER, &mut cur, root, &k, loc(100), b).unwrap());
        let b = cur.page_count();
        root = Some(insert(&mut s.1, HEADER, &mut cur, root, &k, loc(200), b).unwrap());
        assert_eq!(
            load_all(&mut s.1, HEADER, root.unwrap(), cur.page_count()).unwrap(),
            vec![(k, loc(200))]
        );
    }

    #[test]
    fn bulk_load_round_trips_at_every_size_and_stays_insertable() {
        for &n in &[0u64, 1, 62, 63, 64, 65, 127, 128, 129, 4096, 4097, 5000] {
            let mut s = Scratch::new();
            let mut cur = PageAllocator::new(0, 0, Vec::new());
            let mut sorted: Vec<([u8; 32], RecordLoc)> = (0..n).map(|i| (key(i), loc(i))).collect();
            sorted.sort_by_key(|a| a.0);
            let root = build_packed(&mut s.1, HEADER, &mut cur, &sorted).unwrap();

            match root {
                None => assert_eq!(n, 0),
                Some(r) => {
                    let all = load_all(&mut s.1, HEADER, r, cur.page_count()).unwrap();
                    assert_eq!(all, sorted, "bulk_load lost/reordered keys at n={n}");
                    let nk = key(n + 1_000_000);
                    let bound = cur.page_count();
                    let r2 =
                        insert(&mut s.1, HEADER, &mut cur, Some(r), &nk, loc(7), bound).unwrap();
                    let after = load_all(&mut s.1, HEADER, r2, cur.page_count()).unwrap();
                    assert_eq!(after.len(), sorted.len() + 1);
                    assert!(after.iter().any(|&(k, v)| k == nk && v == loc(7)));
                }
            }
        }
    }

    fn entries_of(
        s: &mut Scratch,
        root: Option<PageId>,
        cur: &PageAllocator,
    ) -> Vec<([u8; 32], RecordLoc)> {
        match root {
            Some(r) => load_all(&mut s.1, HEADER, r, cur.page_count()).unwrap(),
            None => Vec::new(),
        }
    }

    #[test]
    fn delete_tracks_a_btreemap_oracle_through_borrow_merge_and_collapse() {
        let mut s = Scratch::new();
        let mut cur = PageAllocator::new(0, 0, Vec::new());
        let n = 2_000u64;
        let mut root: Option<PageId> = None;
        let mut oracle = BTreeMap::new();
        for i in 0..n {
            let bound = cur.page_count();
            root = Some(insert(&mut s.1, HEADER, &mut cur, root, &key(i), loc(i), bound).unwrap());
            oracle.insert(key(i), loc(i));
        }
        // Delete ~three quarters of the keys in a scrambled order, so the deletions drive leaf and
        // internal borrow, merge, and root collapse rather than a tidy right-to-left peel.
        let mut order: Vec<u64> = (0..n).collect();
        order.sort_by_key(|&i| i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        for (step, &i) in order.iter().enumerate() {
            if i % 4 == 0 {
                continue; // keep a quarter of the keys
            }
            let bound = cur.page_count();
            root = delete(&mut s.1, HEADER, &mut cur, root, &key(i), bound).unwrap();
            oracle.remove(&key(i));
            if step % 200 == 0 {
                let expect: Vec<_> = oracle.iter().map(|(k, v)| (*k, *v)).collect();
                assert_eq!(entries_of(&mut s, root, &cur), expect);
            }
        }
        let expect: Vec<_> = oracle.iter().map(|(k, v)| (*k, *v)).collect();
        assert_eq!(entries_of(&mut s, root, &cur), expect);
    }

    #[test]
    fn delete_every_key_empties_the_tree() {
        let mut s = Scratch::new();
        let mut cur = PageAllocator::new(0, 0, Vec::new());
        let n = 300u64;
        let mut root: Option<PageId> = None;
        for i in 0..n {
            let bound = cur.page_count();
            root = Some(insert(&mut s.1, HEADER, &mut cur, root, &key(i), loc(i), bound).unwrap());
        }
        for i in 0..n {
            let bound = cur.page_count();
            root = delete(&mut s.1, HEADER, &mut cur, root, &key(i), bound).unwrap();
        }
        assert!(root.is_none(), "deleting every key empties the tree");
    }

    #[test]
    fn deleting_an_absent_key_leaves_the_tree_intact() {
        let mut s = Scratch::new();
        let mut cur = PageAllocator::new(0, 0, Vec::new());
        let mut root: Option<PageId> = None;
        for i in 0..100u64 {
            let bound = cur.page_count();
            root = Some(insert(&mut s.1, HEADER, &mut cur, root, &key(i), loc(i), bound).unwrap());
        }
        let bound = cur.page_count();
        root = delete(&mut s.1, HEADER, &mut cur, root, &key(999_999), bound).unwrap();
        assert_eq!(entries_of(&mut s, root, &cur).len(), 100);
    }

    #[test]
    fn read_node_never_panics_on_arbitrary_pages() {
        fn xorshift(s: &mut u64) -> u64 {
            *s ^= *s << 13;
            *s ^= *s >> 7;
            *s ^= *s << 17;
            *s
        }
        let mut s = 0xDEAD_BEEF_CAFE_1234u64;
        let mut sc = Scratch::new();
        for _ in 0..2_000 {
            let mut page = [0u8; PAGE];
            for b in &mut page {
                *b = (xorshift(&mut s) >> 33) as u8;
            }
            write_at(&mut sc.1, PageId(0).offset(HEADER), &page).unwrap();
            // A crafted node page must be a clean CORRUPT error (bad crc/magic/count/child), not a
            // panic or runaway recursion - the per-node bounds and depth guard ensure that.
            let _ = load_all(&mut sc.1, HEADER, PageId(0), 1);
        }
    }
}
