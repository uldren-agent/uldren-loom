//! Copy-on-write B-tree for fixed 32-byte keys and typed leaf values. Each node occupies one page,
//! addressed by [`PageId`]. Nodes are immutable once written; an insert appends fresh copies of the
//! root-to-leaf path and frees the pages it supersedes, and the caller swaps the root via a generation
//! bump, so a crash can never corrupt a committed index.
//!
//! On-disk node layout (little-endian), one node per page, CRC-32C over the page minus its last 4
//! bytes:
//! ```text
//!   [0]        NODE_MAGIC
//!   [1]        flags: bit0 = is_leaf, high nibble = value codec discriminator
//!   [2,4)      u16 n  (entry count)
//!   [4, ..)    n * { key[32], codec-defined value }       (sorted by key)
//!   (internal) (n+1) * { child PageId u64 }
//!   [PAGE-4, PAGE) crc32c over [0, PAGE-4)
//! ```

use crate::page::{PAGE_SIZE, PageId};
use crate::pagemap::PageAllocator;
use crate::record::RecordLoc;
use crate::{BackingIo, corrupt, crc32c, io_err, read_exact_at, write_at};
use loom_core::error::Result;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::fs::File;

#[cfg(test)]
thread_local! {
    static LOAD_ALL_CALLS_FOR_TEST: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_load_all_calls_for_test() {
    LOAD_ALL_CALLS_FOR_TEST.with(|calls| calls.set(0));
}

#[cfg(test)]
pub(crate) fn load_all_calls_for_test() -> u64 {
    LOAD_ALL_CALLS_FOR_TEST.with(|calls| calls.get())
}

const NODE_MAGIC: u8 = 0xB7;
const NODE_FLAG_LEAF: u8 = 0x01;
const NODE_FLAG_CODEC_MASK: u8 = 0xF0;
const NODE_FLAG_KNOWN_MASK: u8 = NODE_FLAG_LEAF | NODE_FLAG_CODEC_MASK;
const RECORD_LOC_CODEC_DISCRIMINATOR: u8 = 0x00;
const PACKED_RECORD_REF_CODEC_DISCRIMINATOR: u8 = 0x10;
const FREE_PAGE_EXTENT_CODEC_DISCRIMINATOR: u8 = 0x20;
const LEGACY_MIN_DEGREE: usize = 32;
const LEGACY_MAX_ENTRIES: usize = 2 * LEGACY_MIN_DEGREE - 1;
pub(crate) const MAX_DEPTH: usize = 32; // crafted-tree guard: a real order-64 tree is far shallower than this
pub(crate) const MAX_BATCH_UPSERT_ENTRIES: usize = 65_536;
const PAGE: usize = PAGE_SIZE as usize;
const CRC: usize = 4;
const BODY_END: usize = PAGE - CRC;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ValueCodecKind {
    RecordLoc,
    PackedRecordRef,
    FreePageExtent,
}

impl ValueCodecKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            ValueCodecKind::RecordLoc => "RecordLocCodec",
            ValueCodecKind::PackedRecordRef => "PackedRecordRefCodec",
            ValueCodecKind::FreePageExtent => "FreePageExtentCodec",
        }
    }

    pub(crate) fn discriminator(self) -> u8 {
        match self {
            ValueCodecKind::RecordLoc => RECORD_LOC_CODEC_DISCRIMINATOR,
            ValueCodecKind::PackedRecordRef => PACKED_RECORD_REF_CODEC_DISCRIMINATOR,
            ValueCodecKind::FreePageExtent => FREE_PAGE_EXTENT_CODEC_DISCRIMINATOR,
        }
    }

    fn from_discriminator(discriminator: u8) -> Result<Self> {
        match discriminator {
            RECORD_LOC_CODEC_DISCRIMINATOR => Ok(ValueCodecKind::RecordLoc),
            PACKED_RECORD_REF_CODEC_DISCRIMINATOR => Ok(ValueCodecKind::PackedRecordRef),
            FREE_PAGE_EXTENT_CODEC_DISCRIMINATOR => Ok(ValueCodecKind::FreePageExtent),
            _ => Err(corrupt("unknown btree node codec discriminator")),
        }
    }

    fn maximum_value_width(self) -> usize {
        match self {
            ValueCodecKind::RecordLoc | ValueCodecKind::PackedRecordRef => {
                RecordLoc::MAX_ENCODED_LEN
            }
            ValueCodecKind::FreePageExtent => FreePageExtentValue::ENCODED_LEN,
        }
    }

    fn layout_max_entries(self) -> usize {
        let value = self.maximum_value_width();
        let leaf = (BODY_END - 4) / (32 + value);
        let internal = (BODY_END - 4 - 8) / (32 + value + 8);
        leaf.min(internal)
    }

    fn max_entries(self) -> usize {
        let layout = self.layout_max_entries();
        let odd_layout = layout - usize::from(layout.is_multiple_of(2));
        match self {
            ValueCodecKind::RecordLoc | ValueCodecKind::PackedRecordRef => {
                LEGACY_MAX_ENTRIES.min(odd_layout)
            }
            ValueCodecKind::FreePageExtent => odd_layout,
        }
    }

    fn min_degree(self) -> usize {
        self.max_entries().div_ceil(2)
    }

    fn encode_value(self, value: PageTreeValue, out: &mut Vec<u8>) -> Result<()> {
        match (self, value) {
            (
                ValueCodecKind::RecordLoc | ValueCodecKind::PackedRecordRef,
                PageTreeValue::RecordLoc(value),
            ) => {
                let mut encoded = Vec::with_capacity(RecordLoc::MAX_ENCODED_LEN);
                value.encode(&mut encoded);
                if encoded.len() > RecordLoc::MAX_ENCODED_LEN {
                    return Err(corrupt("btree record locator exceeds canonical width"));
                }
                out.extend_from_slice(&encoded);
                Ok(())
            }
            (ValueCodecKind::FreePageExtent, PageTreeValue::FreePageExtent(value)) => {
                out.extend_from_slice(&value.encode());
                Ok(())
            }
            _ => Err(corrupt("btree value does not match node codec")),
        }
    }

    fn decode_value(self, bytes: &[u8], pos: &mut usize) -> Result<PageTreeValue> {
        match self {
            ValueCodecKind::RecordLoc | ValueCodecKind::PackedRecordRef => {
                let start = *pos;
                let value = RecordLoc::decode(bytes, pos)
                    .ok_or_else(|| corrupt("btree node bad locator"))?;
                if pos.saturating_sub(start) > RecordLoc::MAX_ENCODED_LEN {
                    return Err(corrupt("btree record locator exceeds canonical width"));
                }
                Ok(PageTreeValue::RecordLoc(value))
            }
            ValueCodecKind::FreePageExtent => {
                let end = pos
                    .checked_add(FreePageExtentValue::ENCODED_LEN)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(|| corrupt("btree node truncated free-page extent value"))?;
                let value = FreePageExtentValue::decode(&bytes[*pos..end])?;
                *pos = end;
                Ok(PageTreeValue::FreePageExtent(value))
            }
        }
    }

    fn validate_entry(self, key: &[u8; 32], value: PageTreeValue) -> Result<()> {
        match (self, value) {
            (
                ValueCodecKind::RecordLoc | ValueCodecKind::PackedRecordRef,
                PageTreeValue::RecordLoc(_),
            ) => Ok(()),
            (ValueCodecKind::FreePageExtent, PageTreeValue::FreePageExtent(value)) => {
                if key[..24].iter().any(|byte| *byte != 0) {
                    return Err(corrupt("free-page extent key prefix"));
                }
                let start = u64::from_be_bytes(key[24..].try_into().unwrap());
                value.validate_start(start)
            }
            _ => Err(corrupt("btree value does not match node codec")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FreePageExtentValue {
    pub(crate) len: u64,
    pub(crate) freed_gen: u64,
}

impl FreePageExtentValue {
    pub(crate) const ENCODED_LEN: usize = 16;

    pub(crate) fn encode(self) -> [u8; Self::ENCODED_LEN] {
        let mut bytes = [0u8; Self::ENCODED_LEN];
        bytes[..8].copy_from_slice(&self.len.to_le_bytes());
        bytes[8..].copy_from_slice(&self.freed_gen.to_le_bytes());
        bytes
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != Self::ENCODED_LEN {
            return Err(corrupt("bad free-page extent value length"));
        }
        let value = Self {
            len: u64::from_le_bytes(bytes[..8].try_into().unwrap()),
            freed_gen: u64::from_le_bytes(bytes[8..].try_into().unwrap()),
        };
        if value.len == 0 {
            return Err(corrupt("free-page extent value has zero length"));
        }
        Ok(value)
    }

    pub(crate) fn validate_start(self, start: u64) -> Result<()> {
        if self.len == 0 {
            return Err(corrupt("free-page extent value has zero length"));
        }
        start
            .checked_add(self.len)
            .ok_or_else(|| corrupt("free-page extent range overflow"))?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PageTreeValue {
    RecordLoc(RecordLoc),
    FreePageExtent(FreePageExtentValue),
}

impl PageTreeValue {
    fn record_loc(self) -> Result<RecordLoc> {
        match self {
            Self::RecordLoc(value) => Ok(value),
            Self::FreePageExtent(_) => Err(corrupt("btree value is not a record locator")),
        }
    }

    fn free_page_extent(self) -> Result<FreePageExtentValue> {
        match self {
            Self::FreePageExtent(value) => Ok(value),
            Self::RecordLoc(_) => Err(corrupt("btree value is not a free-page extent")),
        }
    }
}

#[derive(Clone)]
struct Node {
    codec: ValueCodecKind,
    is_leaf: bool,
    entries: Vec<([u8; 32], PageTreeValue)>, // sorted by key
    children: Vec<PageId>,                   // empty for a leaf; otherwise entries.len() + 1
}

impl Node {
    #[cfg(test)]
    fn leaf(entries: Vec<([u8; 32], RecordLoc)>) -> Self {
        Self::leaf_with_codec(
            ValueCodecKind::RecordLoc,
            entries
                .into_iter()
                .map(|(key, value)| (key, PageTreeValue::RecordLoc(value)))
                .collect(),
        )
    }

    fn leaf_with_codec(codec: ValueCodecKind, entries: Vec<([u8; 32], PageTreeValue)>) -> Self {
        Self {
            codec,
            is_leaf: true,
            entries,
            children: Vec::new(),
        }
    }

    fn requested_leaf(codec: ValueCodecKind, entries: Vec<([u8; 32], PageTreeValue)>) -> Self {
        Self::leaf_with_codec(codec, entries)
    }

    /// Lay the node out into a full page. Errors only if the entries overflow one page, which a
    /// A codec-capacity-bounded node never does; the check guards against a logic bug, not valid input.
    fn encode(&self) -> Result<[u8; PAGE]> {
        if self.entries.is_empty() || self.entries.len() > self.codec.max_entries() {
            return Err(corrupt("btree node entry count out of range"));
        }
        let mut body = Vec::with_capacity(PAGE);
        body.push(NODE_MAGIC);
        body.push(self.codec.discriminator() | u8::from(self.is_leaf));
        body.extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
        for (k, v) in &self.entries {
            self.codec.validate_entry(k, *v)?;
            body.extend_from_slice(k);
            self.codec.encode_value(*v, &mut body)?;
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BatchUpsertStats {
    pub(crate) existing_pages_replaced: u64,
    pub(crate) new_split_pages_written: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BatchUpsertResult {
    pub(crate) root: Option<PageId>,
    pub(crate) stats: BatchUpsertStats,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkingRef {
    Existing(PageId),
    Working(usize),
}

#[derive(Clone)]
struct WorkingNode {
    origin: Option<PageId>,
    is_leaf: bool,
    entries: Vec<([u8; 32], PageTreeValue)>,
    children: Vec<WorkingRef>,
    live: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreparedRef {
    Existing(PageId),
    Planned(usize),
}

#[derive(Clone)]
struct PreparedNode {
    is_leaf: bool,
    entries: Vec<([u8; 32], PageTreeValue)>,
    children: Vec<PreparedRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreparedTreeDecisionKind {
    DeleteAbsent,
    DeleteLeaf,
    DeletePredecessor,
    DeleteSuccessor,
    DeleteBorrowLeft,
    DeleteBorrowRight,
    DeleteMergeLeft,
    DeleteMergeRight,
    DeleteRootCollapse,
    UpsertUnchanged,
    UpsertInsert,
    UpsertReplace,
    UpsertSplit,
    UpsertRootConstruct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreparedTreeDecision {
    kind: PreparedTreeDecisionKind,
    key: [u8; 32],
    first_source: Option<PageId>,
    second_source: Option<PageId>,
}

impl PreparedTreeDecision {
    fn is_valid_for(self, page_count: u64) -> bool {
        self.first_source.is_none_or(|page| page.0 < page_count)
            && self.second_source.is_none_or(|page| page.0 < page_count)
            && matches!(
                self.kind,
                PreparedTreeDecisionKind::DeleteAbsent
                    | PreparedTreeDecisionKind::DeleteLeaf
                    | PreparedTreeDecisionKind::DeletePredecessor
                    | PreparedTreeDecisionKind::DeleteSuccessor
                    | PreparedTreeDecisionKind::DeleteBorrowLeft
                    | PreparedTreeDecisionKind::DeleteBorrowRight
                    | PreparedTreeDecisionKind::DeleteMergeLeft
                    | PreparedTreeDecisionKind::DeleteMergeRight
                    | PreparedTreeDecisionKind::DeleteRootCollapse
                    | PreparedTreeDecisionKind::UpsertUnchanged
                    | PreparedTreeDecisionKind::UpsertInsert
                    | PreparedTreeDecisionKind::UpsertReplace
                    | PreparedTreeDecisionKind::UpsertSplit
                    | PreparedTreeDecisionKind::UpsertRootConstruct
            )
    }
}

#[derive(Clone)]
pub(crate) struct PreparedPageTreeDelta {
    source_root: Option<PageId>,
    source_page_count: u64,
    codec: ValueCodecKind,
    deletes: Vec<[u8; 32]>,
    upserts: Vec<([u8; 32], PageTreeValue)>,
    affected_pages: Vec<PageId>,
    decisions: Vec<PreparedTreeDecision>,
    nodes: Vec<PreparedNode>,
    result_root: Option<PreparedRef>,
    allocation_calls: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PreparedPageTreeApplyResult {
    pub(crate) root: Option<PageId>,
    pub(crate) allocation_calls: u64,
    pub(crate) superseded_pages: u64,
}

impl PreparedPageTreeDelta {
    pub(crate) fn allocation_calls(&self) -> u64 {
        self.allocation_calls
    }

    pub(crate) fn affected_page_count(&self) -> u64 {
        self.affected_pages.len() as u64
    }

    pub(crate) fn affected_pages(&self) -> &[PageId] {
        &self.affected_pages
    }

    pub(crate) fn split_decision_count(&self) -> u64 {
        self.decisions
            .iter()
            .filter(|decision| decision.kind == PreparedTreeDecisionKind::UpsertSplit)
            .count() as u64
    }

    pub(crate) fn rebind_upsert_values(
        &mut self,
        replacements: &[([u8; 32], RecordLoc)],
    ) -> Result<()> {
        let replacements = replacements.iter().copied().collect::<BTreeMap<_, _>>();
        if replacements.len() != self.upserts.len()
            || self
                .upserts
                .iter()
                .any(|(key, _)| !replacements.contains_key(key))
        {
            return Err(corrupt("prepared btree upsert replacement keys mismatch"));
        }
        let mut rebound = BTreeSet::new();
        for node in &mut self.nodes {
            for (key, value) in &mut node.entries {
                if let Some(replacement) = replacements.get(key) {
                    *value = PageTreeValue::RecordLoc(*replacement);
                    rebound.insert(*key);
                }
            }
        }
        if rebound.len() != replacements.len() {
            return Err(corrupt(
                "prepared btree upsert replacement is not materialized",
            ));
        }
        for (key, value) in &mut self.upserts {
            *value = PageTreeValue::RecordLoc(replacements[key]);
        }
        Ok(())
    }
}

enum PlannedInsert {
    Done(WorkingRef),
    Split {
        separator: ([u8; 32], PageTreeValue),
        left: WorkingRef,
        right: WorkingRef,
    },
}

struct DeltaPlanner<'a> {
    file: &'a mut dyn BackingIo,
    header_len: u64,
    page_count: u64,
    codec: ValueCodecKind,
    working: Vec<WorkingNode>,
    affected_pages: BTreeSet<PageId>,
    decisions: Vec<PreparedTreeDecision>,
}

impl DeltaPlanner<'_> {
    fn source_of(&self, reference: WorkingRef) -> Option<PageId> {
        match reference {
            WorkingRef::Existing(page) => Some(page),
            WorkingRef::Working(index) => self.working.get(index).and_then(|node| node.origin),
        }
    }

    fn record(
        &mut self,
        kind: PreparedTreeDecisionKind,
        key: [u8; 32],
        first: WorkingRef,
        second: Option<WorkingRef>,
    ) {
        self.decisions.push(PreparedTreeDecision {
            kind,
            key,
            first_source: self.source_of(first),
            second_source: second.and_then(|reference| self.source_of(reference)),
        });
    }

    fn read_existing(&mut self, page: PageId) -> Result<WorkingNode> {
        if page.0 >= self.page_count {
            return Err(corrupt("btree node page out of range"));
        }
        let mut raw = [0u8; PAGE];
        read_exact_at(self.file, page.offset(self.header_len), &mut raw)
            .map_err(|_| corrupt("truncated btree node page"))?;
        let node = decode_node_page_with_codec(&raw, self.codec)?;
        Ok(WorkingNode {
            origin: Some(page),
            is_leaf: node.is_leaf,
            entries: node.entries,
            children: node
                .children
                .into_iter()
                .map(WorkingRef::Existing)
                .collect(),
            live: true,
        })
    }

    fn read(&mut self, reference: WorkingRef) -> Result<WorkingNode> {
        match reference {
            WorkingRef::Existing(page) => self.read_existing(page),
            WorkingRef::Working(index) => self
                .working
                .get(index)
                .filter(|node| node.live)
                .cloned()
                .ok_or_else(|| corrupt("prepared btree node reference is not live")),
        }
    }

    fn make_working(&mut self, reference: WorkingRef) -> Result<WorkingRef> {
        match reference {
            WorkingRef::Working(index) => {
                if self.working.get(index).is_some_and(|node| node.live) {
                    Ok(reference)
                } else {
                    Err(corrupt("prepared btree node reference is not live"))
                }
            }
            WorkingRef::Existing(page) => {
                let node = self.read_existing(page)?;
                self.affected_pages.insert(page);
                let index = self.working.len();
                self.working.push(node);
                Ok(WorkingRef::Working(index))
            }
        }
    }

    fn replace_working(&mut self, reference: WorkingRef, node: WorkingNode) -> Result<()> {
        let WorkingRef::Working(index) = reference else {
            return Err(corrupt("prepared btree replacement is not mutable"));
        };
        let slot = self
            .working
            .get_mut(index)
            .ok_or_else(|| corrupt("prepared btree node reference is out of range"))?;
        *slot = node;
        Ok(())
    }

    fn retire(&mut self, reference: WorkingRef) -> Result<()> {
        match reference {
            WorkingRef::Existing(page) => {
                self.affected_pages.insert(page);
            }
            WorkingRef::Working(index) => {
                let node = self
                    .working
                    .get_mut(index)
                    .ok_or_else(|| corrupt("prepared btree node reference is out of range"))?;
                node.live = false;
            }
        }
        Ok(())
    }

    fn lookup(
        &mut self,
        mut reference: WorkingRef,
        key: &[u8; 32],
    ) -> Result<Option<PageTreeValue>> {
        for _ in 0..=MAX_DEPTH {
            let node = self.read(reference)?;
            match node
                .entries
                .binary_search_by(|(candidate, _)| candidate.cmp(key))
            {
                Ok(index) => return Ok(Some(node.entries[index].1)),
                Err(_) if node.is_leaf => return Ok(None),
                Err(index) => reference = node.children[index],
            }
        }
        Err(corrupt("btree depth exceeds limit"))
    }

    fn extreme_entry(
        &mut self,
        mut reference: WorkingRef,
        take_maximum: bool,
    ) -> Result<([u8; 32], PageTreeValue)> {
        for _ in 0..=MAX_DEPTH {
            let node = self.read(reference)?;
            if node.is_leaf {
                return if take_maximum {
                    node.entries.last().copied()
                } else {
                    node.entries.first().copied()
                }
                .ok_or_else(|| corrupt("empty btree leaf"));
            }
            reference = if take_maximum {
                *node.children.last().unwrap()
            } else {
                node.children[0]
            };
        }
        Err(corrupt("btree depth exceeds limit"))
    }

    fn merge(
        &mut self,
        left: WorkingRef,
        separator: ([u8; 32], PageTreeValue),
        right: WorkingRef,
        kind: PreparedTreeDecisionKind,
        operation_key: [u8; 32],
    ) -> Result<WorkingRef> {
        let left = self.make_working(left)?;
        let mut left_node = self.read(left)?;
        let mut right_node = self.read(right)?;
        if left_node.is_leaf != right_node.is_leaf {
            return Err(corrupt("btree merge node kinds differ"));
        }
        left_node.entries.push(separator);
        left_node.entries.append(&mut right_node.entries);
        if !left_node.is_leaf {
            left_node.children.append(&mut right_node.children);
        }
        self.replace_working(left, left_node)?;
        self.record(kind, operation_key, left, Some(right));
        self.retire(right)?;
        Ok(left)
    }

    fn prepare_child_for_delete(
        &mut self,
        parent: &mut WorkingNode,
        index: usize,
        operation_key: [u8; 32],
    ) -> Result<(usize, WorkingRef)> {
        let child = parent.children[index];
        let min_degree = self.codec.min_degree();
        if self.read(child)?.entries.len() >= min_degree {
            return Ok((index, child));
        }
        if index > 0 {
            let left = parent.children[index - 1];
            if self.read(left)?.entries.len() >= min_degree {
                let left = self.make_working(left)?;
                let child = self.make_working(child)?;
                let mut left_node = self.read(left)?;
                let mut child_node = self.read(child)?;
                let promoted = left_node.entries.pop().unwrap();
                child_node.entries.insert(0, parent.entries[index - 1]);
                if !child_node.is_leaf {
                    child_node
                        .children
                        .insert(0, left_node.children.pop().unwrap());
                }
                parent.entries[index - 1] = promoted;
                parent.children[index - 1] = left;
                parent.children[index] = child;
                self.replace_working(left, left_node)?;
                self.replace_working(child, child_node)?;
                self.record(
                    PreparedTreeDecisionKind::DeleteBorrowLeft,
                    operation_key,
                    child,
                    Some(left),
                );
                return Ok((index, child));
            }
        }
        if index + 1 < parent.children.len() {
            let right = parent.children[index + 1];
            if self.read(right)?.entries.len() >= min_degree {
                let child = self.make_working(child)?;
                let right = self.make_working(right)?;
                let mut child_node = self.read(child)?;
                let mut right_node = self.read(right)?;
                child_node.entries.push(parent.entries[index]);
                if !child_node.is_leaf {
                    child_node.children.push(right_node.children.remove(0));
                }
                parent.entries[index] = right_node.entries.remove(0);
                parent.children[index] = child;
                parent.children[index + 1] = right;
                self.replace_working(child, child_node)?;
                self.replace_working(right, right_node)?;
                self.record(
                    PreparedTreeDecisionKind::DeleteBorrowRight,
                    operation_key,
                    child,
                    Some(right),
                );
                return Ok((index, child));
            }
        }
        if index > 0 {
            let left = parent.children[index - 1];
            let separator = parent.entries.remove(index - 1);
            parent.children.remove(index);
            let merged = self.merge(
                left,
                separator,
                child,
                PreparedTreeDecisionKind::DeleteMergeLeft,
                operation_key,
            )?;
            parent.children[index - 1] = merged;
            Ok((index - 1, merged))
        } else {
            let right = parent.children[index + 1];
            let separator = parent.entries.remove(index);
            parent.children.remove(index + 1);
            let merged = self.merge(
                child,
                separator,
                right,
                PreparedTreeDecisionKind::DeleteMergeRight,
                operation_key,
            )?;
            parent.children[index] = merged;
            Ok((index, merged))
        }
    }

    fn delete_from(
        &mut self,
        reference: WorkingRef,
        key: &[u8; 32],
        depth: usize,
    ) -> Result<WorkingRef> {
        if depth > MAX_DEPTH {
            return Err(corrupt("btree deeper than the structural maximum"));
        }
        let reference = self.make_working(reference)?;
        let mut node = self.read(reference)?;
        match node
            .entries
            .binary_search_by(|(candidate, _)| candidate.cmp(key))
        {
            Ok(index) if node.is_leaf => {
                node.entries.remove(index);
                self.record(PreparedTreeDecisionKind::DeleteLeaf, *key, reference, None);
            }
            Ok(index) => {
                let left = node.children[index];
                let right = node.children[index + 1];
                let min_degree = self.codec.min_degree();
                if self.read(left)?.entries.len() >= min_degree {
                    let predecessor = self.extreme_entry(left, true)?;
                    node.entries[index] = predecessor;
                    node.children[index] = self.delete_from(left, &predecessor.0, depth + 1)?;
                    self.record(
                        PreparedTreeDecisionKind::DeletePredecessor,
                        *key,
                        left,
                        None,
                    );
                } else if self.read(right)?.entries.len() >= min_degree {
                    let successor = self.extreme_entry(right, false)?;
                    node.entries[index] = successor;
                    node.children[index + 1] = self.delete_from(right, &successor.0, depth + 1)?;
                    self.record(PreparedTreeDecisionKind::DeleteSuccessor, *key, right, None);
                } else {
                    let separator = node.entries.remove(index);
                    node.children.remove(index + 1);
                    let merged = self.merge(
                        left,
                        separator,
                        right,
                        PreparedTreeDecisionKind::DeleteMergeRight,
                        *key,
                    )?;
                    node.children[index] = self.delete_from(merged, key, depth + 1)?;
                }
            }
            Err(_) if node.is_leaf => {
                return Err(corrupt("prepared delete key disappeared during planning"));
            }
            Err(index) => {
                let (slot, child) = self.prepare_child_for_delete(&mut node, index, *key)?;
                node.children[slot] = self.delete_from(child, key, depth + 1)?;
            }
        }
        self.replace_working(reference, node)?;
        Ok(reference)
    }

    fn delete_key(
        &mut self,
        root: Option<WorkingRef>,
        key: &[u8; 32],
    ) -> Result<Option<WorkingRef>> {
        let Some(root) = root else {
            self.decisions.push(PreparedTreeDecision {
                kind: PreparedTreeDecisionKind::DeleteAbsent,
                key: *key,
                first_source: None,
                second_source: None,
            });
            return Ok(None);
        };
        if self.lookup(root, key)?.is_none() {
            self.record(PreparedTreeDecisionKind::DeleteAbsent, *key, root, None);
            return Ok(Some(root));
        }
        let root = self.delete_from(root, key, 0)?;
        let node = self.read(root)?;
        if node.entries.is_empty() {
            let result = node.children.first().copied();
            self.record(
                PreparedTreeDecisionKind::DeleteRootCollapse,
                *key,
                root,
                result,
            );
            self.retire(root)?;
            Ok(result)
        } else {
            Ok(Some(root))
        }
    }

    fn split_insert_node(
        &mut self,
        reference: WorkingRef,
        operation_key: [u8; 32],
    ) -> Result<PlannedInsert> {
        let mut node = self.read(reference)?;
        let min_degree = self.codec.min_degree();
        if node.entries.len() <= self.codec.max_entries() {
            return Ok(PlannedInsert::Done(reference));
        }
        let right_entries = node.entries.split_off(min_degree);
        let separator = node.entries.pop().unwrap();
        let right_children = if node.is_leaf {
            Vec::new()
        } else {
            node.children.split_off(min_degree)
        };
        let right = WorkingRef::Working(self.working.len());
        self.working.push(WorkingNode {
            origin: None,
            is_leaf: node.is_leaf,
            entries: right_entries,
            children: right_children,
            live: true,
        });
        self.replace_working(reference, node)?;
        self.record(
            PreparedTreeDecisionKind::UpsertSplit,
            operation_key,
            reference,
            Some(right),
        );
        Ok(PlannedInsert::Split {
            separator,
            left: reference,
            right,
        })
    }

    fn upsert_from(
        &mut self,
        reference: WorkingRef,
        key: [u8; 32],
        value: PageTreeValue,
        depth: usize,
    ) -> Result<PlannedInsert> {
        if depth > MAX_DEPTH {
            return Err(corrupt("btree deeper than the structural maximum"));
        }
        let snapshot = self.read(reference)?;
        match snapshot
            .entries
            .binary_search_by(|(candidate, _)| candidate.cmp(&key))
        {
            Ok(index) if snapshot.entries[index].1 == value => {
                self.record(
                    PreparedTreeDecisionKind::UpsertUnchanged,
                    key,
                    reference,
                    None,
                );
                Ok(PlannedInsert::Done(reference))
            }
            Ok(index) => {
                let reference = self.make_working(reference)?;
                let mut node = self.read(reference)?;
                node.entries[index].1 = value;
                self.replace_working(reference, node)?;
                self.record(
                    PreparedTreeDecisionKind::UpsertReplace,
                    key,
                    reference,
                    None,
                );
                Ok(PlannedInsert::Done(reference))
            }
            Err(index) if snapshot.is_leaf => {
                let reference = self.make_working(reference)?;
                let mut node = self.read(reference)?;
                node.entries.insert(index, (key, value));
                self.replace_working(reference, node)?;
                self.record(PreparedTreeDecisionKind::UpsertInsert, key, reference, None);
                self.split_insert_node(reference, key)
            }
            Err(index) => {
                let child = snapshot.children[index];
                let child_result = self.upsert_from(child, key, value, depth + 1)?;
                let reference = self.make_working(reference)?;
                let mut node = self.read(reference)?;
                match child_result {
                    PlannedInsert::Done(child) => node.children[index] = child,
                    PlannedInsert::Split {
                        separator,
                        left,
                        right,
                    } => {
                        node.entries.insert(index, separator);
                        node.children[index] = left;
                        node.children.insert(index + 1, right);
                    }
                }
                self.replace_working(reference, node)?;
                self.split_insert_node(reference, key)
            }
        }
    }

    fn upsert_key(
        &mut self,
        root: Option<WorkingRef>,
        key: [u8; 32],
        value: PageTreeValue,
    ) -> Result<Option<WorkingRef>> {
        let Some(root) = root else {
            let root = WorkingRef::Working(self.working.len());
            self.working.push(WorkingNode {
                origin: None,
                is_leaf: true,
                entries: vec![(key, value)],
                children: Vec::new(),
                live: true,
            });
            self.record(PreparedTreeDecisionKind::UpsertInsert, key, root, None);
            return Ok(Some(root));
        };
        match self.upsert_from(root, key, value, 0)? {
            PlannedInsert::Done(root) => Ok(Some(root)),
            PlannedInsert::Split {
                separator,
                left,
                right,
            } => {
                let root = WorkingRef::Working(self.working.len());
                self.working.push(WorkingNode {
                    origin: None,
                    is_leaf: false,
                    entries: vec![separator],
                    children: vec![left, right],
                    live: true,
                });
                self.record(
                    PreparedTreeDecisionKind::UpsertRootConstruct,
                    key,
                    root,
                    None,
                );
                Ok(Some(root))
            }
        }
    }

    fn freeze_ref(
        &self,
        reference: WorkingRef,
        visiting: &mut BTreeSet<usize>,
        emitted: &mut BTreeSet<usize>,
        nodes: &mut Vec<PreparedNode>,
    ) -> Result<PreparedRef> {
        let index = match reference {
            WorkingRef::Existing(page) => {
                if page.0 >= self.page_count {
                    return Err(corrupt("prepared btree child page out of range"));
                }
                return Ok(PreparedRef::Existing(page));
            }
            WorkingRef::Working(index) => index,
        };
        if !visiting.insert(index) {
            return Err(corrupt("prepared btree node graph is cyclic"));
        }
        if emitted.contains(&index) {
            return Err(corrupt("prepared btree node graph has duplicate ownership"));
        }
        let node = self
            .working
            .get(index)
            .filter(|node| node.live)
            .ok_or_else(|| corrupt("prepared btree root references a retired node"))?;
        let children = node
            .children
            .iter()
            .map(|child| self.freeze_ref(*child, visiting, emitted, nodes))
            .collect::<Result<Vec<_>>>()?;
        visiting.remove(&index);
        emitted.insert(index);
        let prepared = PreparedRef::Planned(nodes.len());
        nodes.push(PreparedNode {
            is_leaf: node.is_leaf,
            entries: node.entries.clone(),
            children,
        });
        Ok(prepared)
    }

    fn finish(
        self,
        root: Option<WorkingRef>,
    ) -> Result<(
        Vec<PreparedNode>,
        Option<PreparedRef>,
        Vec<PageId>,
        Vec<PreparedTreeDecision>,
    )> {
        let mut nodes = Vec::new();
        let root = match root {
            Some(root) => Some(self.freeze_ref(
                root,
                &mut BTreeSet::new(),
                &mut BTreeSet::new(),
                &mut nodes,
            )?),
            None => None,
        };
        Ok((
            nodes,
            root,
            self.affected_pages.into_iter().collect(),
            self.decisions,
        ))
    }
}

pub(crate) fn prepare_delete_upsert_delta(
    file: &mut dyn BackingIo,
    header_len: u64,
    source_root: Option<PageId>,
    source_page_count: u64,
    codec: ValueCodecKind,
    delete_keys: &[[u8; 32]],
    upserts: &[([u8; 32], RecordLoc)],
) -> Result<PreparedPageTreeDelta> {
    if codec == ValueCodecKind::FreePageExtent {
        return Err(corrupt("record locator wrapper requires a locator codec"));
    }
    let upserts = upserts
        .iter()
        .map(|(key, value)| (*key, PageTreeValue::RecordLoc(*value)))
        .collect::<Vec<_>>();
    prepare_delete_upsert_delta_values(
        file,
        header_len,
        source_root,
        source_page_count,
        codec,
        delete_keys,
        &upserts,
    )
}

fn prepare_delete_upsert_delta_values(
    file: &mut dyn BackingIo,
    header_len: u64,
    source_root: Option<PageId>,
    source_page_count: u64,
    codec: ValueCodecKind,
    delete_keys: &[[u8; 32]],
    upserts: &[([u8; 32], PageTreeValue)],
) -> Result<PreparedPageTreeDelta> {
    if delete_keys.len().saturating_add(upserts.len()) > MAX_BATCH_UPSERT_ENTRIES {
        return Err(corrupt("btree prepared delta exceeds entry limit"));
    }
    for (key, value) in upserts {
        codec.validate_entry(key, *value)?;
    }
    let mut deletes = delete_keys.to_vec();
    deletes.sort_unstable();
    deletes.dedup();
    let mut sorted_upserts = upserts.to_vec();
    sorted_upserts.sort_by_key(|(key, _)| *key);
    let mut normalized_upserts = Vec::with_capacity(sorted_upserts.len());
    for entry in sorted_upserts {
        if normalized_upserts.last_mut().is_some_and(|(key, value)| {
            if *key == entry.0 {
                *value = entry.1;
                true
            } else {
                false
            }
        }) {
            continue;
        }
        normalized_upserts.push(entry);
    }

    let mut planner = DeltaPlanner {
        file,
        header_len,
        page_count: source_page_count,
        codec,
        working: Vec::new(),
        affected_pages: BTreeSet::new(),
        decisions: Vec::new(),
    };
    let source_reference = source_root.map(WorkingRef::Existing);
    let upsert_keys: BTreeSet<_> = normalized_upserts.iter().map(|(key, _)| *key).collect();
    let mut effective_deletes = Vec::new();
    for key in &deletes {
        if upsert_keys.contains(key) {
            continue;
        }
        match source_reference {
            Some(root) if planner.lookup(root, key)?.is_some() => effective_deletes.push(*key),
            Some(root) => planner.record(PreparedTreeDecisionKind::DeleteAbsent, *key, root, None),
            None => planner.decisions.push(PreparedTreeDecision {
                kind: PreparedTreeDecisionKind::DeleteAbsent,
                key: *key,
                first_source: None,
                second_source: None,
            }),
        }
    }
    let mut effective_upserts = Vec::new();
    for (key, value) in &normalized_upserts {
        match source_reference {
            Some(root) if planner.lookup(root, key)? == Some(*value) => {
                planner.record(PreparedTreeDecisionKind::UpsertUnchanged, *key, root, None)
            }
            _ => effective_upserts.push((*key, *value)),
        }
    }

    let mut result_root = source_reference;
    for key in effective_deletes {
        result_root = planner.delete_key(result_root, &key)?;
    }
    for (key, value) in effective_upserts {
        result_root = planner.upsert_key(result_root, key, value)?;
    }
    let (nodes, result_root, affected_pages, decisions) = planner.finish(result_root)?;
    let allocation_calls = nodes.len() as u64;
    Ok(PreparedPageTreeDelta {
        source_root,
        source_page_count,
        codec,
        deletes,
        upserts: normalized_upserts,
        affected_pages,
        decisions,
        nodes,
        result_root,
        allocation_calls,
    })
}

pub(crate) fn prepare_free_page_extent_delta(
    file: &mut dyn BackingIo,
    header_len: u64,
    source_root: Option<PageId>,
    source_page_count: u64,
    delete_keys: &[[u8; 32]],
    upserts: &[([u8; 32], FreePageExtentValue)],
) -> Result<PreparedPageTreeDelta> {
    let upserts = upserts
        .iter()
        .map(|(key, value)| (*key, PageTreeValue::FreePageExtent(*value)))
        .collect::<Vec<_>>();
    prepare_delete_upsert_delta_values(
        file,
        header_len,
        source_root,
        source_page_count,
        ValueCodecKind::FreePageExtent,
        delete_keys,
        &upserts,
    )
}

pub(crate) fn apply_prepared_delta(
    file: &mut dyn BackingIo,
    header_len: u64,
    allocator: &mut PageAllocator,
    source_root: Option<PageId>,
    source_page_count: u64,
    codec: ValueCodecKind,
    prepared: PreparedPageTreeDelta,
) -> Result<PreparedPageTreeApplyResult> {
    validate_prepared_delta(source_root, source_page_count, codec, &prepared)?;
    let allocated = (0..prepared.nodes.len())
        .map(|_| allocator.alloc(1))
        .collect::<Vec<_>>();
    apply_prepared_delta_on_pages(
        file,
        header_len,
        allocator,
        source_root,
        source_page_count,
        codec,
        prepared,
        &allocated,
    )
}

pub(crate) fn apply_prepared_delta_on_pages(
    file: &mut dyn BackingIo,
    header_len: u64,
    allocator: &mut PageAllocator,
    source_root: Option<PageId>,
    source_page_count: u64,
    codec: ValueCodecKind,
    prepared: PreparedPageTreeDelta,
    allocated: &[PageId],
) -> Result<PreparedPageTreeApplyResult> {
    validate_prepared_delta(source_root, source_page_count, codec, &prepared)?;
    if allocated.len() != prepared.nodes.len()
        || allocated
            .iter()
            .any(|page| !allocator.allocated_in_transaction(page.0))
    {
        return Err(corrupt("prepared btree page allocation mismatch"));
    }
    write_prepared_delta(file, header_len, allocator, codec, prepared, allocated)
}

pub(crate) fn apply_prepared_free_page_extent_delta_on_pages(
    file: &mut dyn BackingIo,
    header_len: u64,
    allocator: &mut PageAllocator,
    source_root: Option<PageId>,
    source_page_count: u64,
    prepared: PreparedPageTreeDelta,
    allocated: &[PageId],
) -> Result<PreparedPageTreeApplyResult> {
    apply_prepared_delta_on_pages(
        file,
        header_len,
        allocator,
        source_root,
        source_page_count,
        ValueCodecKind::FreePageExtent,
        prepared,
        allocated,
    )
}

fn validate_prepared_delta(
    source_root: Option<PageId>,
    source_page_count: u64,
    codec: ValueCodecKind,
    prepared: &PreparedPageTreeDelta,
) -> Result<()> {
    if source_root != prepared.source_root
        || source_page_count != prepared.source_page_count
        || codec != prepared.codec
    {
        return Err(corrupt("prepared btree delta source identity mismatch"));
    }
    if prepared.allocation_calls != prepared.nodes.len() as u64
        || prepared.deletes.windows(2).any(|keys| keys[0] >= keys[1])
        || prepared
            .upserts
            .windows(2)
            .any(|entries| entries[0].0 >= entries[1].0)
        || prepared
            .upserts
            .iter()
            .any(|(key, value)| codec.validate_entry(key, *value).is_err())
        || prepared
            .affected_pages
            .windows(2)
            .any(|pages| pages[0] >= pages[1])
        || prepared
            .affected_pages
            .iter()
            .any(|page| page.0 >= source_page_count)
        || prepared
            .decisions
            .iter()
            .any(|decision| !decision.is_valid_for(source_page_count))
    {
        return Err(corrupt("prepared btree delta integrity check failed"));
    }
    for (index, node) in prepared.nodes.iter().enumerate() {
        if node.entries.is_empty()
            || node.entries.len() > codec.max_entries()
            || node
                .entries
                .windows(2)
                .any(|entries| entries[0].0 >= entries[1].0)
            || node
                .entries
                .iter()
                .any(|(key, value)| codec.validate_entry(key, *value).is_err())
            || (node.is_leaf && !node.children.is_empty())
            || (!node.is_leaf && node.children.len() != node.entries.len() + 1)
            || node.children.iter().any(|child| match child {
                PreparedRef::Existing(page) => page.0 >= source_page_count,
                PreparedRef::Planned(child) => *child >= index,
            })
        {
            return Err(corrupt("prepared btree node integrity check failed"));
        }
    }
    if prepared.result_root.is_some_and(|root| match root {
        PreparedRef::Existing(page) => page.0 >= source_page_count,
        PreparedRef::Planned(index) => index >= prepared.nodes.len(),
    }) {
        return Err(corrupt("prepared btree root integrity check failed"));
    }

    Ok(())
}

fn write_prepared_delta(
    file: &mut dyn BackingIo,
    header_len: u64,
    allocator: &mut PageAllocator,
    codec: ValueCodecKind,
    prepared: PreparedPageTreeDelta,
    allocated: &[PageId],
) -> Result<PreparedPageTreeApplyResult> {
    for (index, node) in prepared.nodes.iter().enumerate() {
        let children = node
            .children
            .iter()
            .map(|child| match child {
                PreparedRef::Existing(page) => *page,
                PreparedRef::Planned(index) => allocated[*index],
            })
            .collect();
        let page = Node {
            codec,
            is_leaf: node.is_leaf,
            entries: node.entries.clone(),
            children,
        }
        .encode()?;
        let page_id = allocated[index];
        write_at(file, page_id.offset(header_len), &page).map_err(io_err)?;
    }
    for page in &prepared.affected_pages {
        allocator.free(*page, 1)?;
    }
    let root = prepared.result_root.map(|root| match root {
        PreparedRef::Existing(page) => page,
        PreparedRef::Planned(index) => allocated[index],
    });
    Ok(PreparedPageTreeApplyResult {
        root,
        allocation_calls: allocated.len() as u64,
        superseded_pages: prepared.affected_pages.len() as u64,
    })
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
    codec: ValueCodecKind,
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
        decode_node_page_with_codec(&buf, self.codec)
    }

    /// Allocate one page for `node`, write it there, and return its page id.
    fn write(&mut self, node: &Node) -> Result<PageId> {
        let node = Node {
            codec: self.codec,
            is_leaf: node.is_leaf,
            entries: node.entries.clone(),
            children: node.children.clone(),
        };
        let page = node.encode()?;
        let pid = self.cur.alloc(1);
        write_at(self.file, pid.offset(self.header_len), &page).map_err(io_err)?;
        Ok(pid)
    }

    fn get_node(
        &mut self,
        page: PageId,
        key: &[u8; 32],
        depth: usize,
    ) -> Result<Option<PageTreeValue>> {
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

    fn walk(
        &mut self,
        page: PageId,
        depth: usize,
        out: &mut Vec<([u8; 32], PageTreeValue)>,
        progress: &mut impl FnMut(u64),
    ) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(corrupt("btree deeper than the structural maximum"));
        }
        let node = self.read(page)?;
        progress(1);
        if node.is_leaf {
            out.extend(node.entries.iter().copied());
        } else {
            for i in 0..node.entries.len() {
                self.walk(node.children[i], depth + 1, out, progress)?;
                out.push(node.entries[i]);
            }
            self.walk(node.children[node.entries.len()], depth + 1, out, progress)?;
        }
        Ok(())
    }

    fn free_pages(&mut self, page: PageId, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(corrupt("btree deeper than the structural maximum"));
        }
        let node = self.read(page)?;
        self.cur.free(page, 1)?;
        if !node.is_leaf {
            for child in node.children {
                self.free_pages(child, depth + 1)?;
            }
        }
        Ok(())
    }

    fn collect_pages(&mut self, page: PageId, depth: usize, out: &mut Vec<PageId>) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(corrupt("btree deeper than the structural maximum"));
        }
        let node = self.read(page)?;
        out.push(page);
        if !node.is_leaf {
            for child in node.children {
                self.collect_pages(child, depth + 1, out)?;
            }
        }
        Ok(())
    }

    /// Build internal levels above finished child nodes until one root remains.
    fn build_up(
        &mut self,
        seps: Vec<([u8; 32], PageTreeValue)>,
        children: Vec<PageId>,
    ) -> Result<PageId> {
        let max_entries = self.codec.max_entries();
        if seps.len() <= max_entries {
            return self.write(&Node {
                codec: self.codec,
                is_leaf: false,
                entries: seps,
                children,
            });
        }
        let cap = max_entries + 1;
        let c = children.len();
        let p = c.div_ceil(cap); // >= 2 because separators exceed the codec capacity
        let base = c / p;
        let extra = c % p; // the first `extra` groups get one more child
        let mut cidx = 0usize;
        let mut sidx = 0usize;
        let mut up_children = Vec::with_capacity(p);
        let mut up_seps: Vec<([u8; 32], PageTreeValue)> = Vec::with_capacity(p - 1);
        for gi in 0..p {
            let cnt = base + usize::from(gi < extra); // children in this group (>= 2)
            let group_children = children[cidx..cidx + cnt].to_vec();
            let group_seps = seps[sidx..sidx + (cnt - 1)].to_vec();
            sidx += cnt - 1;
            cidx += cnt;
            up_children.push(self.write(&Node {
                codec: self.codec,
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
}

fn get_with_page_reader_inner(
    page_count: u64,
    read_page: &mut impl FnMut(PageId) -> Result<[u8; PAGE]>,
    expected_codec: ValueCodecKind,
    page: PageId,
    key: &[u8; 32],
    depth: usize,
) -> Result<Option<PageTreeValue>> {
    if depth > MAX_DEPTH {
        return Err(corrupt("btree depth exceeds limit"));
    }
    if page.0 >= page_count {
        return Err(corrupt("btree node page out of range"));
    }
    let raw = read_page(page)?;
    let node = decode_node_page_with_codec(&raw, expected_codec)?;
    match node.entries.binary_search_by(|(k, _)| k.cmp(key)) {
        Ok(i) => Ok(Some(node.entries[i].1)),
        Err(_) if node.is_leaf => Ok(None),
        Err(i) => get_with_page_reader_inner(
            page_count,
            read_page,
            expected_codec,
            node.children[i],
            key,
            depth + 1,
        ),
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
    if buf[1] & !NODE_FLAG_KNOWN_MASK != 0 {
        return Err(corrupt("btree node reserved flag bit set"));
    }
    let codec = ValueCodecKind::from_discriminator(buf[1] & NODE_FLAG_CODEC_MASK)?;
    let is_leaf = buf[1] & NODE_FLAG_LEAF == NODE_FLAG_LEAF;
    let n = u16::from_le_bytes([buf[2], buf[3]]) as usize;
    if n == 0 || n > codec.max_entries() {
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
        let v = codec.decode_value(&buf[..BODY_END], &mut pos)?;
        codec.validate_entry(&k, v)?;
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
        codec,
        is_leaf,
        entries,
        children,
    })
}

fn decode_node_page_with_codec(buf: &[u8; PAGE], expected: ValueCodecKind) -> Result<Node> {
    let node = decode_node_page(buf)?;
    if node.codec != expected {
        return Err(corrupt("btree node codec discriminator mismatch"));
    }
    Ok(node)
}

pub(crate) fn looks_like_node_page(buf: &[u8; PAGE]) -> bool {
    decode_node_page(buf).is_ok()
}

pub(crate) fn predecessor(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: Option<PageId>,
    key: &[u8; 32],
    page_count: u64,
) -> Result<Option<([u8; 32], RecordLoc)>> {
    predecessor_with_codec(
        file,
        header_len,
        root,
        key,
        page_count,
        ValueCodecKind::RecordLoc,
    )
}

pub(crate) fn predecessor_with_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: Option<PageId>,
    key: &[u8; 32],
    page_count: u64,
    expected_codec: ValueCodecKind,
) -> Result<Option<([u8; 32], RecordLoc)>> {
    let Some(mut page) = root else {
        return Ok(None);
    };
    let mut candidate = None;
    for _ in 0..=MAX_DEPTH {
        if page.0 >= page_count {
            return Err(corrupt("btree node page out of range"));
        }
        let mut raw = [0u8; PAGE];
        read_exact_at(file, page.offset(header_len), &mut raw)
            .map_err(|_| corrupt("truncated btree node page"))?;
        let node = decode_node_page_with_codec(&raw, expected_codec)?;
        let child = node.entries.partition_point(|(entry, _)| entry < key);
        if child > 0 {
            candidate = Some(node.entries[child - 1]);
        }
        if node.is_leaf {
            return candidate
                .map(|(candidate_key, value)| {
                    value.record_loc().map(|value| (candidate_key, value))
                })
                .transpose();
        }
        page = node.children[child];
    }
    Err(corrupt("btree depth exceeds limit"))
}

pub(crate) fn free_page_extent_predecessor(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: Option<PageId>,
    key: &[u8; 32],
    page_count: u64,
) -> Result<Option<([u8; 32], FreePageExtentValue)>> {
    let Some(mut page) = root else {
        return Ok(None);
    };
    let mut candidate = None;
    for _ in 0..=MAX_DEPTH {
        if page.0 >= page_count {
            return Err(corrupt("btree node page out of range"));
        }
        let mut raw = [0u8; PAGE];
        read_exact_at(file, page.offset(header_len), &mut raw)
            .map_err(|_| corrupt("truncated btree node page"))?;
        let node = decode_node_page_with_codec(&raw, ValueCodecKind::FreePageExtent)?;
        let child = node.entries.partition_point(|(entry, _)| entry < key);
        if child > 0 {
            candidate = Some(node.entries[child - 1]);
        }
        if node.is_leaf {
            return candidate
                .map(|(candidate_key, value)| {
                    value.free_page_extent().map(|value| (candidate_key, value))
                })
                .transpose();
        }
        page = node.children[child];
    }
    Err(corrupt("btree depth exceeds limit"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NodePageLinks {
    pub(crate) children: Vec<PageId>,
    pub(crate) values: Vec<RecordLoc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FreePageExtentNodeLinks {
    pub(crate) children: Vec<PageId>,
}

pub(crate) fn free_page_extent_node_links(
    file: &mut dyn BackingIo,
    header_len: u64,
    page: PageId,
    page_count: u64,
) -> Result<Option<FreePageExtentNodeLinks>> {
    if page.0 >= page_count {
        return Err(corrupt("btree node page out of range"));
    }
    let mut buf = [0u8; PAGE];
    read_exact_at(file, page.offset(header_len), &mut buf).map_err(io_err)?;
    if !looks_like_node_page(&buf) {
        return Ok(None);
    }
    let node = decode_node_page_with_codec(&buf, ValueCodecKind::FreePageExtent)?;
    Ok(Some(FreePageExtentNodeLinks {
        children: node.children,
    }))
}

pub(crate) fn node_page_links_with_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    page: PageId,
    page_count: u64,
    expected_codec: ValueCodecKind,
) -> Result<Option<NodePageLinks>> {
    if page.0 >= page_count {
        return Err(corrupt("btree node page out of range"));
    }
    let mut buf = [0u8; PAGE];
    read_exact_at(file, page.offset(header_len), &mut buf).map_err(io_err)?;
    if !looks_like_node_page(&buf) {
        return Ok(None);
    }
    let node = decode_node_page_with_codec(&buf, expected_codec)?;
    let values = node
        .entries
        .iter()
        .map(|(_, value)| value.record_loc())
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(NodePageLinks {
        children: node.children,
        values,
    }))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RootPageCodecInspection {
    pub(crate) in_range: bool,
    pub(crate) checksum_ok: bool,
    pub(crate) raw_magic: Option<u8>,
    pub(crate) raw_flags: Option<u8>,
    pub(crate) actual_discriminator: Option<u8>,
    pub(crate) magic_ok: bool,
    pub(crate) codec_ok: bool,
    pub(crate) failure: Option<&'static str>,
}

pub(crate) fn inspect_root_page_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
    expected: ValueCodecKind,
) -> Result<RootPageCodecInspection> {
    if root.0 >= page_count {
        return Ok(RootPageCodecInspection {
            in_range: false,
            checksum_ok: false,
            raw_magic: None,
            raw_flags: None,
            actual_discriminator: None,
            magic_ok: false,
            codec_ok: false,
            failure: Some("root_page_out_of_range"),
        });
    }
    let mut buf = [0u8; PAGE];
    read_exact_at(file, root.offset(header_len), &mut buf).map_err(io_err)?;
    let stored = u32::from_le_bytes(buf[BODY_END..].try_into().unwrap());
    let checksum_ok = crc32c(&buf[..BODY_END]) == stored;
    let raw_magic = Some(buf[0]);
    let raw_flags = Some(buf[1]);
    let actual_discriminator = Some(buf[1] & NODE_FLAG_CODEC_MASK);
    let magic_ok = buf[0] == NODE_MAGIC;
    let codec_ok = actual_discriminator == Some(expected.discriminator());
    let failure = if !checksum_ok {
        Some("btree_node_crc_mismatch")
    } else if !magic_ok {
        Some("bad_btree_node_magic")
    } else if !codec_ok {
        Some("btree_node_codec_discriminator_mismatch")
    } else {
        None
    };
    Ok(RootPageCodecInspection {
        in_range: true,
        checksum_ok,
        raw_magic,
        raw_flags,
        actual_discriminator,
        magic_ok,
        codec_ok,
        failure,
    })
}

pub(crate) fn inspect_tree_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
    expected: ValueCodecKind,
) -> Result<(PageId, RootPageCodecInspection)> {
    inspect_tree_codec_inner(file, header_len, root, page_count, expected, 0)
}

pub(crate) fn tree_depth(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
) -> Result<u64> {
    tree_depth_with_codec(
        file,
        header_len,
        root,
        page_count,
        ValueCodecKind::RecordLoc,
    )
}

pub(crate) fn tree_depth_with_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
    expected_codec: ValueCodecKind,
) -> Result<u64> {
    let mut allocator = PageAllocator::new(page_count, 0, Vec::new());
    let mut tree = Tree {
        file,
        cur: &mut allocator,
        header_len,
        page_count,
        codec: expected_codec,
    };
    let mut page = root;
    for depth in 1..=MAX_DEPTH + 1 {
        let node = tree.read(page)?;
        if node.is_leaf {
            return Ok(depth as u64);
        }
        page = *node
            .children
            .first()
            .ok_or_else(|| corrupt("btree internal node has no children"))?;
    }
    Err(corrupt("btree depth exceeds limit"))
}

pub(crate) fn free_page_extent_tree_depth(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
) -> Result<u64> {
    tree_depth_with_codec(
        file,
        header_len,
        root,
        page_count,
        ValueCodecKind::FreePageExtent,
    )
}

fn inspect_tree_codec_inner(
    file: &mut dyn BackingIo,
    header_len: u64,
    page: PageId,
    page_count: u64,
    expected: ValueCodecKind,
    depth: usize,
) -> Result<(PageId, RootPageCodecInspection)> {
    if depth > MAX_DEPTH {
        return Ok((
            page,
            RootPageCodecInspection {
                in_range: page.0 < page_count,
                checksum_ok: false,
                raw_magic: None,
                raw_flags: None,
                actual_discriminator: None,
                magic_ok: false,
                codec_ok: false,
                failure: Some("btree_depth_exceeds_limit"),
            },
        ));
    }
    let inspection = inspect_root_page_codec(file, header_len, page, page_count, expected)?;
    if inspection.failure.is_some() {
        return Ok((page, inspection));
    }
    let mut buf = [0u8; PAGE];
    read_exact_at(file, page.offset(header_len), &mut buf).map_err(io_err)?;
    let node = decode_node_page(&buf)?;
    if node.codec != expected {
        return Ok((
            page,
            RootPageCodecInspection {
                codec_ok: false,
                failure: Some("btree_node_codec_discriminator_mismatch"),
                ..inspection
            },
        ));
    }
    if !node.is_leaf {
        for child in node.children {
            let (child_page, child_inspection) =
                inspect_tree_codec_inner(file, header_len, child, page_count, expected, depth + 1)?;
            if child_inspection.failure.is_some() {
                return Ok((child_page, child_inspection));
            }
        }
    }
    Ok((page, inspection))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScanCursor {
    pub(crate) stack: Vec<ScanOp>,
}

impl ScanCursor {
    pub(crate) fn new(root: PageId) -> Self {
        Self {
            stack: vec![ScanOp::Visit {
                page: root,
                depth: 0,
            }],
        }
    }

    pub(crate) fn completed(&self) -> bool {
        self.stack.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScanOp {
    Visit { page: PageId, depth: usize },
    Emit(([u8; 32], RecordLoc)),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScanStep {
    pub(crate) entries: Vec<([u8; 32], RecordLoc)>,
    pub(crate) pages_read: usize,
    pub(crate) completed: bool,
}

pub(crate) fn scan_step_with_page_reader(
    cursor: &mut ScanCursor,
    page_count: u64,
    max_pages: usize,
    deadline: Option<std::time::Instant>,
    read_page: impl FnMut(PageId) -> Result<[u8; PAGE]>,
) -> Result<ScanStep> {
    scan_step_with_page_reader_and_codec(
        cursor,
        page_count,
        max_pages,
        deadline,
        ValueCodecKind::RecordLoc,
        read_page,
    )
}

pub(crate) fn scan_step_with_page_reader_and_codec(
    cursor: &mut ScanCursor,
    page_count: u64,
    max_pages: usize,
    deadline: Option<std::time::Instant>,
    expected_codec: ValueCodecKind,
    mut read_page: impl FnMut(PageId) -> Result<[u8; PAGE]>,
) -> Result<ScanStep> {
    let mut entries = Vec::new();
    let mut pages_read = 0usize;
    while let Some(op) = cursor.stack.pop() {
        if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
            cursor.stack.push(op);
            break;
        }
        match op {
            ScanOp::Emit(entry) => entries.push(entry),
            ScanOp::Visit { page, depth } => {
                if pages_read >= max_pages {
                    cursor.stack.push(ScanOp::Visit { page, depth });
                    break;
                }
                if depth > MAX_DEPTH {
                    return Err(corrupt("btree deeper than the structural maximum"));
                }
                if page.0 >= page_count {
                    return Err(corrupt("btree node page out of range"));
                }
                let node = decode_node_page_with_codec(&read_page(page)?, expected_codec)?;
                pages_read += 1;
                if node.is_leaf {
                    for (key, value) in node.entries.into_iter().rev() {
                        cursor.stack.push(ScanOp::Emit((key, value.record_loc()?)));
                    }
                } else {
                    cursor.stack.push(ScanOp::Visit {
                        page: node.children[node.entries.len()],
                        depth: depth + 1,
                    });
                    for i in (0..node.entries.len()).rev() {
                        cursor.stack.push(ScanOp::Emit((
                            node.entries[i].0,
                            node.entries[i].1.record_loc()?,
                        )));
                        cursor.stack.push(ScanOp::Visit {
                            page: node.children[i],
                            depth: depth + 1,
                        });
                    }
                }
            }
        }
    }
    Ok(ScanStep {
        entries,
        pages_read,
        completed: cursor.completed(),
    })
}

/// CoW-insert `(key, value)` into the tree rooted at `root` (None = empty), allocating new node pages
/// via `cur` and freeing the pages it supersedes, and return the new root page. `page_count` is the
/// allocated-page count *before* this insert: the bound for reading the existing (immutable) nodes.
#[cfg(test)]
pub(crate) fn insert(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    root: Option<PageId>,
    key: &[u8; 32],
    value: RecordLoc,
    page_count: u64,
) -> Result<PageId> {
    insert_with_codec(
        file,
        header_len,
        cur,
        root,
        key,
        value,
        page_count,
        ValueCodecKind::RecordLoc,
    )
}

#[cfg(test)]
pub(crate) fn insert_with_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    root: Option<PageId>,
    key: &[u8; 32],
    value: RecordLoc,
    page_count: u64,
    codec: ValueCodecKind,
) -> Result<PageId> {
    let prepared = prepare_delete_upsert_delta(
        file,
        header_len,
        root,
        page_count,
        codec,
        &[],
        &[(*key, value)],
    )?;
    apply_prepared_delta(file, header_len, cur, root, page_count, codec, prepared)?
        .root
        .ok_or_else(|| corrupt("prepared btree upsert produced no root"))
}

pub(crate) fn batch_upsert(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    root: Option<PageId>,
    updates: &[([u8; 32], RecordLoc)],
    page_count: u64,
) -> Result<BatchUpsertResult> {
    batch_upsert_with_codec(
        file,
        header_len,
        cur,
        root,
        updates,
        page_count,
        ValueCodecKind::RecordLoc,
    )
}

pub(crate) fn batch_upsert_with_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    root: Option<PageId>,
    updates: &[([u8; 32], RecordLoc)],
    page_count: u64,
    codec: ValueCodecKind,
) -> Result<BatchUpsertResult> {
    let had_root = root.is_some();
    let prepared =
        prepare_delete_upsert_delta(file, header_len, root, page_count, codec, &[], updates)?;
    let existing_pages_replaced = prepared.affected_pages.len() as u64;
    let planned_allocations = prepared.allocation_calls;
    let applied = apply_prepared_delta(file, header_len, cur, root, page_count, codec, prepared)?;
    Ok(BatchUpsertResult {
        root: applied.root,
        stats: BatchUpsertStats {
            existing_pages_replaced,
            new_split_pages_written: planned_allocations.saturating_sub(if had_root {
                existing_pages_replaced
            } else {
                1
            }),
        },
    })
}

/// CoW-delete `key` from the tree rooted at `root` (None = empty), allocating new node pages via `cur`
/// and freeing the pages it supersedes, and return the new root page (None if the tree became empty).
/// `page_count` is the allocated-page count *before* this delete: the bound for reading existing nodes.
/// Deleting an absent key leaves the root unchanged.
pub(crate) fn delete(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    root: Option<PageId>,
    key: &[u8; 32],
    page_count: u64,
) -> Result<Option<PageId>> {
    delete_with_codec(
        file,
        header_len,
        cur,
        root,
        key,
        page_count,
        ValueCodecKind::RecordLoc,
    )
}

pub(crate) fn delete_with_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    root: Option<PageId>,
    key: &[u8; 32],
    page_count: u64,
    codec: ValueCodecKind,
) -> Result<Option<PageId>> {
    let prepared =
        prepare_delete_upsert_delta(file, header_len, root, page_count, codec, &[*key], &[])?;
    apply_prepared_delta(file, header_len, cur, root, page_count, codec, prepared)
        .map(|result| result.root)
}

pub(crate) fn get(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: Option<PageId>,
    key: &[u8; 32],
    page_count: u64,
) -> Result<Option<RecordLoc>> {
    get_with_codec(
        file,
        header_len,
        root,
        key,
        page_count,
        ValueCodecKind::RecordLoc,
    )
}

pub(crate) fn get_with_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: Option<PageId>,
    key: &[u8; 32],
    page_count: u64,
    expected_codec: ValueCodecKind,
) -> Result<Option<RecordLoc>> {
    let Some(root) = root else {
        return Ok(None);
    };
    let mut t = Tree {
        file,
        cur: &mut PageAllocator::new(page_count, 0, Vec::new()),
        header_len,
        page_count,
        codec: expected_codec,
    };
    t.get_node(root, key, 0)?
        .map(PageTreeValue::record_loc)
        .transpose()
}

pub(crate) fn free_page_extent_get(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: Option<PageId>,
    key: &[u8; 32],
    page_count: u64,
) -> Result<Option<FreePageExtentValue>> {
    let Some(root) = root else {
        return Ok(None);
    };
    let mut t = Tree {
        file,
        cur: &mut PageAllocator::new(page_count, 0, Vec::new()),
        header_len,
        page_count,
        codec: ValueCodecKind::FreePageExtent,
    };
    t.get_node(root, key, 0)?
        .map(PageTreeValue::free_page_extent)
        .transpose()
}

pub(crate) fn range(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    low: &[u8; 32],
    high: &[u8; 32],
    page_count: u64,
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    range_with_codec(
        file,
        header_len,
        root,
        low,
        high,
        page_count,
        ValueCodecKind::RecordLoc,
    )
}

pub(crate) fn range_with_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    low: &[u8; 32],
    high: &[u8; 32],
    page_count: u64,
    expected_codec: ValueCodecKind,
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    let mut t = Tree {
        file,
        cur: &mut PageAllocator::new(page_count, 0, Vec::new()),
        header_len,
        page_count,
        codec: expected_codec,
    };
    let mut out = Vec::new();
    t.range_node(root, low, high, 0, &mut out)?;
    Ok(out)
}

impl Tree<'_> {
    fn range_node(
        &mut self,
        page: PageId,
        low: &[u8; 32],
        high: &[u8; 32],
        depth: usize,
        out: &mut Vec<([u8; 32], RecordLoc)>,
    ) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(corrupt("btree deeper than the structural maximum"));
        }
        let node = self.read(page)?;
        for (index, (key, loc)) in node.entries.iter().enumerate() {
            if !node.is_leaf && low <= key {
                self.range_node(node.children[index], low, high, depth + 1, out)?;
            }
            if key >= high {
                return Ok(());
            }
            if key >= low {
                out.push((*key, loc.record_loc()?));
            }
        }
        if !node.is_leaf {
            self.range_node(node.children[node.entries.len()], low, high, depth + 1, out)?;
        }
        Ok(())
    }

    fn range_value_node(
        &mut self,
        page: PageId,
        low: &[u8; 32],
        high: &[u8; 32],
        depth: usize,
        out: &mut Vec<([u8; 32], PageTreeValue)>,
    ) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(corrupt("btree deeper than the structural maximum"));
        }
        let node = self.read(page)?;
        for (index, (key, value)) in node.entries.iter().enumerate() {
            if !node.is_leaf && low <= key {
                self.range_value_node(node.children[index], low, high, depth + 1, out)?;
            }
            if key >= high {
                return Ok(());
            }
            if key >= low {
                out.push((*key, *value));
            }
        }
        if !node.is_leaf {
            self.range_value_node(node.children[node.entries.len()], low, high, depth + 1, out)?;
        }
        Ok(())
    }
}

pub(crate) fn free_page_extent_range(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    low: &[u8; 32],
    high: &[u8; 32],
    page_count: u64,
) -> Result<Vec<([u8; 32], FreePageExtentValue)>> {
    let mut t = Tree {
        file,
        cur: &mut PageAllocator::new(page_count, 0, Vec::new()),
        header_len,
        page_count,
        codec: ValueCodecKind::FreePageExtent,
    };
    let mut values = Vec::new();
    t.range_value_node(root, low, high, 0, &mut values)?;
    values
        .into_iter()
        .map(|(key, value)| value.free_page_extent().map(|value| (key, value)))
        .collect()
}

pub(crate) fn get_with_page_reader(
    root: Option<PageId>,
    key: &[u8; 32],
    page_count: u64,
    read_page: impl FnMut(PageId) -> Result<[u8; PAGE]>,
) -> Result<Option<RecordLoc>> {
    get_with_page_reader_and_codec(root, key, page_count, ValueCodecKind::RecordLoc, read_page)
}

pub(crate) fn get_with_page_reader_and_codec(
    root: Option<PageId>,
    key: &[u8; 32],
    page_count: u64,
    expected_codec: ValueCodecKind,
    mut read_page: impl FnMut(PageId) -> Result<[u8; PAGE]>,
) -> Result<Option<RecordLoc>> {
    let Some(root) = root else {
        return Ok(None);
    };
    get_with_page_reader_inner(page_count, &mut read_page, expected_codec, root, key, 0)?
        .map(PageTreeValue::record_loc)
        .transpose()
}

/// Walk the whole tree rooted at `root` and return every `(key, locator)` entry, in ascending key
/// order. Used on open to rebuild the in-memory index without scanning object payloads.
pub(crate) fn load_all(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    load_all_with_codec(
        file,
        header_len,
        root,
        page_count,
        ValueCodecKind::RecordLoc,
    )
}

pub(crate) fn load_all_with_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
    expected_codec: ValueCodecKind,
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    load_all_with_progress_and_codec(file, header_len, root, page_count, expected_codec, |_| {})
}

pub(crate) fn load_all_with_progress(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
    progress: impl FnMut(u64),
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    load_all_with_progress_and_codec(
        file,
        header_len,
        root,
        page_count,
        ValueCodecKind::RecordLoc,
        progress,
    )
}

pub(crate) fn load_all_with_progress_and_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
    expected_codec: ValueCodecKind,
    mut progress: impl FnMut(u64),
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    #[cfg(test)]
    LOAD_ALL_CALLS_FOR_TEST.with(|calls| calls.set(calls.get() + 1));
    let mut t = Tree {
        file,
        cur: &mut PageAllocator::new(page_count, 0, Vec::new()),
        header_len,
        page_count,
        codec: expected_codec,
    };
    let mut values = Vec::new();
    t.walk(root, 0, &mut values, &mut progress)?;
    values
        .into_iter()
        .map(|(key, value)| value.record_loc().map(|value| (key, value)))
        .collect()
}

pub(crate) fn load_all_free_page_extents_with_progress(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
    mut progress: impl FnMut(u64),
) -> Result<Vec<([u8; 32], FreePageExtentValue)>> {
    #[cfg(test)]
    LOAD_ALL_CALLS_FOR_TEST.with(|calls| calls.set(calls.get() + 1));
    let mut t = Tree {
        file,
        cur: &mut PageAllocator::new(page_count, 0, Vec::new()),
        header_len,
        page_count,
        codec: ValueCodecKind::FreePageExtent,
    };
    let mut values = Vec::new();
    t.walk(root, 0, &mut values, &mut progress)?;
    values
        .into_iter()
        .map(|(key, value)| value.free_page_extent().map(|value| (key, value)))
        .collect()
}

pub(crate) fn load_all_free_page_extents(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
) -> Result<Vec<([u8; 32], FreePageExtentValue)>> {
    load_all_free_page_extents_with_progress(file, header_len, root, page_count, |_| {})
}

pub(crate) fn free_all(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    root: PageId,
    page_count: u64,
) -> Result<()> {
    free_all_with_codec(
        file,
        header_len,
        cur,
        root,
        page_count,
        ValueCodecKind::RecordLoc,
    )
}

pub(crate) fn free_all_with_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    root: PageId,
    page_count: u64,
    expected_codec: ValueCodecKind,
) -> Result<()> {
    let mut t = Tree {
        file,
        cur,
        header_len,
        page_count,
        codec: expected_codec,
    };
    t.free_pages(root, 0)
}

pub(crate) fn collect_pages(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
) -> Result<Vec<PageId>> {
    collect_pages_with_codec(
        file,
        header_len,
        root,
        page_count,
        ValueCodecKind::RecordLoc,
    )
}

pub(crate) fn collect_pages_with_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
    expected_codec: ValueCodecKind,
) -> Result<Vec<PageId>> {
    let mut t = Tree {
        file,
        cur: &mut PageAllocator::new(page_count, 0, Vec::new()),
        header_len,
        page_count,
        codec: expected_codec,
    };
    let mut out = Vec::new();
    t.collect_pages(root, 0, &mut out)?;
    Ok(out)
}

pub(crate) fn collect_free_page_extent_pages(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
) -> Result<Vec<PageId>> {
    collect_pages_with_codec(
        file,
        header_len,
        root,
        page_count,
        ValueCodecKind::FreePageExtent,
    )
}

/// Bulk-build a balanced B-tree from `entries`, writing each node once.
pub(crate) fn build_packed(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    entries: &[([u8; 32], RecordLoc)],
) -> Result<Option<PageId>> {
    build_packed_with_codec(file, header_len, cur, entries, ValueCodecKind::RecordLoc)
}

pub(crate) fn build_packed_with_codec(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    entries: &[([u8; 32], RecordLoc)],
    codec: ValueCodecKind,
) -> Result<Option<PageId>> {
    if codec == ValueCodecKind::FreePageExtent {
        return Err(corrupt("record locator wrapper requires a locator codec"));
    }
    let entries = entries
        .iter()
        .map(|(key, value)| (*key, PageTreeValue::RecordLoc(*value)))
        .collect::<Vec<_>>();
    build_packed_values(file, header_len, cur, &entries, codec)
}

pub(crate) fn build_packed_free_page_extents(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    entries: &[([u8; 32], FreePageExtentValue)],
) -> Result<Option<PageId>> {
    let entries = entries
        .iter()
        .map(|(key, value)| (*key, PageTreeValue::FreePageExtent(*value)))
        .collect::<Vec<_>>();
    build_packed_values(
        file,
        header_len,
        cur,
        &entries,
        ValueCodecKind::FreePageExtent,
    )
}

fn build_packed_values(
    file: &mut dyn BackingIo,
    header_len: u64,
    cur: &mut PageAllocator,
    entries: &[([u8; 32], PageTreeValue)],
    codec: ValueCodecKind,
) -> Result<Option<PageId>> {
    if entries.is_empty() {
        return Ok(None);
    }
    for (key, value) in entries {
        codec.validate_entry(key, *value)?;
    }
    let page_count = cur.page_count();
    let mut t = Tree {
        file,
        cur,
        header_len,
        page_count,
        codec,
    };
    let max_entries = codec.max_entries();
    if entries.len() <= max_entries {
        return Ok(Some(
            t.write(&Node::requested_leaf(codec, entries.to_vec()))?,
        ));
    }
    let s = max_entries;
    let n = entries.len();
    let m = (n + 1).div_ceil(s + 1); // >= 2 here
    let leaf_total = n - (m - 1);
    let base = leaf_total / m;
    let extra = leaf_total % m; // the first `extra` leaves get one more entry
    let mut idx = 0usize;
    let mut children = Vec::with_capacity(m);
    let mut seps: Vec<([u8; 32], PageTreeValue)> = Vec::with_capacity(m - 1);
    for li in 0..m {
        let cnt = base + usize::from(li < extra);
        children.push(t.write(&Node::requested_leaf(
            codec,
            entries[idx..idx + cnt].to_vec(),
        ))?);
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
    use std::collections::{BTreeMap, BTreeSet};
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

    fn ordered_key(i: u64) -> [u8; 32] {
        let mut key = [0u8; 32];
        key[..8].copy_from_slice(&i.to_be_bytes());
        key
    }

    fn refresh_node_crc(page: &mut [u8; PAGE]) {
        let crc = crc32c(&page[..BODY_END]);
        page[BODY_END..].copy_from_slice(&crc.to_le_bytes());
    }

    fn extent_key_for_test(start: u64) -> [u8; 32] {
        let mut key = [0u8; 32];
        key[24..].copy_from_slice(&start.to_be_bytes());
        key
    }

    fn extent_value(len: u64, freed_gen: u64) -> PageTreeValue {
        PageTreeValue::FreePageExtent(FreePageExtentValue { len, freed_gen })
    }

    #[test]
    fn typed_node_codec_preserves_legacy_locator_bytes_and_pins_extent_bytes() {
        let key = extent_key_for_test(9);
        let locator = RecordLoc::from_global(2 * crate::page::PAGES_PER_SEGMENT + 3, 4);
        let legacy = Node::leaf(vec![(key, locator)]).encode().unwrap();
        let mut expected_legacy = [0u8; PAGE];
        expected_legacy[0] = NODE_MAGIC;
        expected_legacy[1] = NODE_FLAG_LEAF | RECORD_LOC_CODEC_DISCRIMINATOR;
        expected_legacy[2..4].copy_from_slice(&1u16.to_le_bytes());
        expected_legacy[4..36].copy_from_slice(&key);
        expected_legacy[36..39].copy_from_slice(&[2, 3, 4]);
        refresh_node_crc(&mut expected_legacy);
        assert_eq!(legacy, expected_legacy);
        assert_eq!(
            decode_node_page(&legacy).unwrap().entries[0].1,
            PageTreeValue::RecordLoc(locator)
        );
        let packed = Node::leaf_with_codec(
            ValueCodecKind::PackedRecordRef,
            vec![(key, PageTreeValue::RecordLoc(locator))],
        )
        .encode()
        .unwrap();
        let mut expected_packed = expected_legacy;
        expected_packed[1] = NODE_FLAG_LEAF | PACKED_RECORD_REF_CODEC_DISCRIMINATOR;
        refresh_node_crc(&mut expected_packed);
        assert_eq!(packed, expected_packed);
        let mut maximum_locator = Vec::new();
        RecordLoc::from_global(u64::MAX, u32::MAX).encode(&mut maximum_locator);
        assert_eq!(maximum_locator.len(), RecordLoc::MAX_ENCODED_LEN);

        let value = FreePageExtentValue {
            len: 0x0102_0304_0506_0708,
            freed_gen: 0x1112_1314_1516_1718,
        };
        assert_eq!(
            value.encode(),
            [
                0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, 0x18, 0x17, 0x16, 0x15, 0x14, 0x13,
                0x12, 0x11,
            ]
        );
        let extent = Node::leaf_with_codec(
            ValueCodecKind::FreePageExtent,
            vec![(key, PageTreeValue::FreePageExtent(value))],
        )
        .encode()
        .unwrap();
        assert_eq!(extent[0], NODE_MAGIC);
        assert_eq!(
            extent[1],
            NODE_FLAG_LEAF | FREE_PAGE_EXTENT_CODEC_DISCRIMINATOR
        );
        assert_eq!(&extent[4..36], &key);
        assert_eq!(&extent[36..52], &value.encode());
        assert_eq!(
            decode_node_page(&extent).unwrap().entries,
            vec![(key, PageTreeValue::FreePageExtent(value))]
        );
    }

    #[test]
    fn typed_node_codec_rejects_malformed_extent_values_and_wrong_codecs() {
        let value = FreePageExtentValue {
            len: 7,
            freed_gen: 11,
        };
        assert!(FreePageExtentValue::decode(&value.encode()[..15]).is_err());
        let mut trailing = value.encode().to_vec();
        trailing.push(0);
        assert!(FreePageExtentValue::decode(&trailing).is_err());
        assert!(
            FreePageExtentValue::decode(
                &FreePageExtentValue {
                    len: 0,
                    freed_gen: 11,
                }
                .encode()
            )
            .is_err()
        );
        assert!(value.validate_start(u64::MAX - 7).is_ok());
        assert!(value.validate_start(u64::MAX - 6).is_err());

        let key = extent_key_for_test(4);
        let wrong_locator = Node::leaf_with_codec(
            ValueCodecKind::FreePageExtent,
            vec![(key, PageTreeValue::RecordLoc(loc(1)))],
        )
        .encode()
        .unwrap_err();
        assert!(wrong_locator.message.contains("does not match node codec"));
        let wrong_extent = Node::leaf_with_codec(
            ValueCodecKind::RecordLoc,
            vec![(key, PageTreeValue::FreePageExtent(value))],
        )
        .encode()
        .unwrap_err();
        assert!(wrong_extent.message.contains("does not match node codec"));

        let mut unknown = Node::leaf(vec![(key, loc(1))]).encode().unwrap();
        unknown[1] = (unknown[1] & !NODE_FLAG_CODEC_MASK) | 0xF0;
        refresh_node_crc(&mut unknown);
        assert!(
            decode_node_page(&unknown)
                .err()
                .unwrap()
                .message
                .contains("unknown btree node codec discriminator")
        );
    }

    #[test]
    fn typed_node_codec_capacity_is_layout_derived_and_boundary_safe() {
        assert_eq!(ValueCodecKind::RecordLoc.maximum_value_width(), 15);
        assert_eq!(ValueCodecKind::RecordLoc.max_entries(), 63);
        assert_eq!(ValueCodecKind::PackedRecordRef.max_entries(), 63);
        assert_eq!(ValueCodecKind::FreePageExtent.maximum_value_width(), 16);
        assert_eq!(ValueCodecKind::FreePageExtent.layout_max_entries(), 72);
        assert_eq!(ValueCodecKind::FreePageExtent.max_entries(), 71);

        let boundary = (0..ValueCodecKind::FreePageExtent.max_entries() as u64)
            .map(|start| (extent_key_for_test(start), extent_value(1, 3)))
            .collect();
        Node::leaf_with_codec(ValueCodecKind::FreePageExtent, boundary)
            .encode()
            .unwrap();
        let overflow = (0..=ValueCodecKind::FreePageExtent.max_entries() as u64)
            .map(|start| (extent_key_for_test(start), extent_value(1, 3)))
            .collect();
        assert!(
            Node::leaf_with_codec(ValueCodecKind::FreePageExtent, overflow)
                .encode()
                .unwrap_err()
                .message
                .contains("entry count out of range")
        );
    }

    #[test]
    fn typed_node_codec_uses_the_shared_prepared_mutation_and_traversal() {
        let mut scratch = Scratch::new();
        let mut allocator = PageAllocator::new(0, 0, Vec::new());
        let upserts = [
            (extent_key_for_test(30), extent_value(3, 7)),
            (extent_key_for_test(10), extent_value(2, 5)),
            (extent_key_for_test(30), extent_value(4, 8)),
        ];
        let prepared = prepare_delete_upsert_delta_values(
            &mut scratch.1,
            HEADER,
            None,
            0,
            ValueCodecKind::FreePageExtent,
            &[],
            &upserts,
        )
        .unwrap();
        let applied = apply_prepared_delta(
            &mut scratch.1,
            HEADER,
            &mut allocator,
            None,
            0,
            ValueCodecKind::FreePageExtent,
            prepared,
        )
        .unwrap();
        let root = applied.root.unwrap();
        let mut read_allocator = PageAllocator::new(allocator.page_count(), 0, Vec::new());
        let mut reader = Tree {
            file: &mut scratch.1,
            cur: &mut read_allocator,
            header_len: HEADER,
            page_count: allocator.page_count(),
            codec: ValueCodecKind::FreePageExtent,
        };
        let mut entries = Vec::new();
        reader.walk(root, 0, &mut entries, &mut |_| {}).unwrap();
        assert_eq!(
            entries,
            vec![
                (extent_key_for_test(10), extent_value(2, 5)),
                (extent_key_for_test(30), extent_value(4, 8)),
            ]
        );
        assert_eq!(
            reader.get_node(root, &extent_key_for_test(30), 0).unwrap(),
            Some(extent_value(4, 8))
        );
    }

    #[test]
    fn free_page_extent_wrappers_cover_the_shared_tree_authority() {
        // Seventy-two entries exceed the extent codec's 71-entry leaf capacity.
        let entries = (0..72u64)
            .map(|index| {
                (
                    extent_key_for_test(index * 2),
                    FreePageExtentValue {
                        len: 1,
                        freed_gen: index + 10,
                    },
                )
            })
            .collect::<Vec<_>>();
        let mut scratch = Scratch::new();
        let mut allocator = PageAllocator::new(0, 0, Vec::new());
        let root = build_packed_free_page_extents(&mut scratch.1, HEADER, &mut allocator, &entries)
            .unwrap()
            .unwrap();
        let page_count = allocator.page_count();

        assert!(free_page_extent_tree_depth(&mut scratch.1, HEADER, root, page_count).unwrap() > 1);
        assert_eq!(
            free_page_extent_get(
                &mut scratch.1,
                HEADER,
                Some(root),
                &extent_key_for_test(40),
                page_count,
            )
            .unwrap(),
            Some(FreePageExtentValue {
                len: 1,
                freed_gen: 30,
            })
        );
        assert_eq!(
            free_page_extent_predecessor(
                &mut scratch.1,
                HEADER,
                Some(root),
                &extent_key_for_test(41),
                page_count,
            )
            .unwrap(),
            Some(entries[20])
        );
        assert_eq!(
            free_page_extent_range(
                &mut scratch.1,
                HEADER,
                root,
                &extent_key_for_test(40),
                &extent_key_for_test(46),
                page_count,
            )
            .unwrap(),
            entries[20..23]
        );
        let mut progress = Vec::new();
        assert_eq!(
            load_all_free_page_extents_with_progress(
                &mut scratch.1,
                HEADER,
                root,
                page_count,
                |count| progress.push(count),
            )
            .unwrap(),
            entries
        );
        assert!(!progress.is_empty());
        let pages =
            collect_free_page_extent_pages(&mut scratch.1, HEADER, root, page_count).unwrap();
        assert!(pages.len() > 1);
        assert_eq!(
            free_page_extent_node_links(&mut scratch.1, HEADER, root, page_count)
                .unwrap()
                .unwrap()
                .children
                .len(),
            2
        );

        let replacement = (
            extent_key_for_test(41),
            FreePageExtentValue {
                len: 2,
                freed_gen: 500,
            },
        );
        let prepared = prepare_free_page_extent_delta(
            &mut scratch.1,
            HEADER,
            Some(root),
            page_count,
            &[extent_key_for_test(40)],
            &[replacement],
        )
        .unwrap();
        let assigned = (0..prepared.allocation_calls())
            .map(|_| allocator.alloc(1))
            .collect::<Vec<_>>();
        let applied = apply_prepared_free_page_extent_delta_on_pages(
            &mut scratch.1,
            HEADER,
            &mut allocator,
            Some(root),
            page_count,
            prepared,
            &assigned,
        )
        .unwrap();
        let updated_root = applied.root.unwrap();
        let updated_page_count = allocator.page_count();
        let mut reopened = std::fs::OpenOptions::new()
            .read(true)
            .open(&scratch.0)
            .unwrap();
        let reopened_entries =
            load_all_free_page_extents(&mut reopened, HEADER, updated_root, updated_page_count)
                .unwrap();
        assert!(
            !reopened_entries
                .iter()
                .any(|entry| entry.0 == extent_key_for_test(40))
        );
        assert!(reopened_entries.contains(&replacement));
    }

    #[test]
    fn typed_node_codec_handles_multilevel_extent_mutations_reopen_and_apply_failure() {
        let mut scratch = Scratch::new();
        let mut allocator = PageAllocator::new(0, 0, Vec::new());
        let upserts = (0..160u64)
            .map(|start| (extent_key_for_test(start * 2), extent_value(1, 7)))
            .collect::<Vec<_>>();
        let prepared = prepare_delete_upsert_delta_values(
            &mut scratch.1,
            HEADER,
            None,
            0,
            ValueCodecKind::FreePageExtent,
            &[],
            &upserts,
        )
        .unwrap();
        assert!(prepared.split_decision_count() > 0);
        let mut root = apply_prepared_delta(
            &mut scratch.1,
            HEADER,
            &mut allocator,
            None,
            0,
            ValueCodecKind::FreePageExtent,
            prepared,
        )
        .unwrap()
        .root
        .unwrap();
        assert!(
            tree_depth_with_codec(
                &mut scratch.1,
                HEADER,
                root,
                allocator.page_count(),
                ValueCodecKind::FreePageExtent,
            )
            .unwrap()
                > 1
        );
        let pages = collect_pages_with_codec(
            &mut scratch.1,
            HEADER,
            root,
            allocator.page_count(),
            ValueCodecKind::FreePageExtent,
        )
        .unwrap();
        for page in pages {
            let mut raw = [0u8; PAGE];
            read_exact_at(&mut scratch.1, page.offset(HEADER), &mut raw).unwrap();
            assert_eq!(
                decode_node_page_with_codec(&raw, ValueCodecKind::FreePageExtent)
                    .unwrap()
                    .codec,
                ValueCodecKind::FreePageExtent
            );
        }

        let source_page_count = allocator.page_count();
        let replacement = (extent_key_for_test(80), extent_value(5, 99));
        let prepared = prepare_delete_upsert_delta_values(
            &mut scratch.1,
            HEADER,
            Some(root),
            source_page_count,
            ValueCodecKind::FreePageExtent,
            &[],
            &[replacement],
        )
        .unwrap();
        assert!(
            prepared
                .decisions
                .iter()
                .any(|decision| decision.kind == PreparedTreeDecisionKind::UpsertReplace)
        );
        root = apply_prepared_delta(
            &mut scratch.1,
            HEADER,
            &mut allocator,
            Some(root),
            source_page_count,
            ValueCodecKind::FreePageExtent,
            prepared,
        )
        .unwrap()
        .root
        .unwrap();

        let source_page_count = allocator.page_count();
        let expected_before_failure = {
            let mut read_allocator = PageAllocator::new(source_page_count, 0, Vec::new());
            let mut reader = Tree {
                file: &mut scratch.1,
                cur: &mut read_allocator,
                header_len: HEADER,
                page_count: source_page_count,
                codec: ValueCodecKind::FreePageExtent,
            };
            let mut entries = Vec::new();
            reader.walk(root, 0, &mut entries, &mut |_| {}).unwrap();
            entries
        };
        assert_eq!(
            expected_before_failure
                .iter()
                .find(|(key, _)| *key == replacement.0)
                .map(|(_, value)| *value),
            Some(replacement.1)
        );
        let failed_prepared = prepare_delete_upsert_delta_values(
            &mut scratch.1,
            HEADER,
            Some(root),
            source_page_count,
            ValueCodecKind::FreePageExtent,
            &[],
            &[(extent_key_for_test(1_000), extent_value(2, 100))],
        )
        .unwrap();
        let mut failed_allocator = PageAllocator::new(source_page_count, 1, Vec::new());
        let error = {
            let mut failing = FailNthWrite {
                file: &mut scratch.1,
                writes: 0,
                fail_on: 1,
            };
            apply_prepared_delta(
                &mut failing,
                HEADER,
                &mut failed_allocator,
                Some(root),
                source_page_count,
                ValueCodecKind::FreePageExtent,
                failed_prepared,
            )
            .unwrap_err()
        };
        assert_eq!(error.code, loom_core::error::Code::Io);
        let after_failure = {
            let mut read_allocator = PageAllocator::new(source_page_count, 0, Vec::new());
            let mut reader = Tree {
                file: &mut scratch.1,
                cur: &mut read_allocator,
                header_len: HEADER,
                page_count: source_page_count,
                codec: ValueCodecKind::FreePageExtent,
            };
            let mut entries = Vec::new();
            reader.walk(root, 0, &mut entries, &mut |_| {}).unwrap();
            entries
        };
        assert_eq!(after_failure, expected_before_failure);

        let deletes = (0..160u64)
            .map(|start| extent_key_for_test(start * 2))
            .filter(|key| *key != replacement.0)
            .collect::<Vec<_>>();
        let prepared = prepare_delete_upsert_delta_values(
            &mut scratch.1,
            HEADER,
            Some(root),
            source_page_count,
            ValueCodecKind::FreePageExtent,
            &deletes,
            &[],
        )
        .unwrap();
        assert!(prepared.decisions.iter().any(|decision| matches!(
            decision.kind,
            PreparedTreeDecisionKind::DeleteBorrowLeft
                | PreparedTreeDecisionKind::DeleteBorrowRight
                | PreparedTreeDecisionKind::DeleteMergeLeft
                | PreparedTreeDecisionKind::DeleteMergeRight
        )));
        assert!(
            prepared
                .decisions
                .iter()
                .any(|decision| decision.kind == PreparedTreeDecisionKind::DeleteRootCollapse)
        );
        root = apply_prepared_delta(
            &mut scratch.1,
            HEADER,
            &mut allocator,
            Some(root),
            source_page_count,
            ValueCodecKind::FreePageExtent,
            prepared,
        )
        .unwrap()
        .root
        .unwrap();

        let mut reopened = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&scratch.0)
            .unwrap();
        let reopened_page_count = allocator.page_count();
        let mut reopened_allocator = PageAllocator::new(reopened_page_count, 0, Vec::new());
        let mut reader = Tree {
            file: &mut reopened,
            cur: &mut reopened_allocator,
            header_len: HEADER,
            page_count: reopened_page_count,
            codec: ValueCodecKind::FreePageExtent,
        };
        let mut reopened_entries = Vec::new();
        reader
            .walk(root, 0, &mut reopened_entries, &mut |_| {})
            .unwrap();
        assert_eq!(reopened_entries, vec![replacement]);
        assert_eq!(
            tree_depth_with_codec(
                &mut reopened,
                HEADER,
                root,
                reopened_page_count,
                ValueCodecKind::FreePageExtent,
            )
            .unwrap(),
            1
        );
    }

    #[derive(Debug)]
    struct ValidatedNode {
        entries: Vec<[u8; 32]>,
        children: Vec<PageId>,
    }

    #[derive(Debug)]
    struct ValidatedTree {
        pages: BTreeMap<PageId, ValidatedNode>,
        leaf_depth: usize,
    }

    fn validate_tree_structure(
        file: &mut dyn BackingIo,
        root: PageId,
        page_count: u64,
        expected_codec: ValueCodecKind,
    ) -> ValidatedTree {
        fn visit(
            file: &mut dyn BackingIo,
            page: PageId,
            page_count: u64,
            expected_codec: ValueCodecKind,
            depth: usize,
            is_root: bool,
            pages: &mut BTreeMap<PageId, ValidatedNode>,
            leaf_depth: &mut Option<usize>,
        ) -> ([u8; 32], [u8; 32]) {
            assert!(page.0 < page_count, "page {} is out of range", page.0);
            assert!(
                !pages.contains_key(&page),
                "page {} is multiply owned or cyclic",
                page.0
            );
            let mut raw = [0u8; PAGE];
            read_exact_at(file, page.offset(HEADER), &mut raw).unwrap();
            let node = decode_node_page(&raw).unwrap();
            assert_eq!(
                node.codec, expected_codec,
                "codec mismatch at page {}",
                page.0
            );
            assert!(
                node.entries.windows(2).all(|pair| pair[0].0 < pair[1].0),
                "page {} entries are not strictly ordered",
                page.0
            );
            assert!(node.entries.len() <= expected_codec.max_entries());
            if !is_root {
                assert!(
                    node.entries.len() >= expected_codec.min_degree() - 1,
                    "non-root page {} is underfull with {} entries",
                    page.0,
                    node.entries.len()
                );
            }
            let entries: Vec<_> = node.entries.iter().map(|(key, _)| *key).collect();
            let children = node.children.clone();
            pages.insert(
                page,
                ValidatedNode {
                    entries: entries.clone(),
                    children: children.clone(),
                },
            );
            if node.is_leaf {
                assert!(children.is_empty());
                match *leaf_depth {
                    Some(expected) => {
                        assert_eq!(depth, expected, "leaf depth differs at page {}", page.0)
                    }
                    None => *leaf_depth = Some(depth),
                }
                return (*entries.first().unwrap(), *entries.last().unwrap());
            }

            assert_eq!(
                children.len(),
                entries.len() + 1,
                "internal page {} child count differs from entries + 1",
                page.0
            );
            let ranges: Vec<_> = children
                .iter()
                .map(|child| {
                    visit(
                        file,
                        *child,
                        page_count,
                        expected_codec,
                        depth + 1,
                        false,
                        pages,
                        leaf_depth,
                    )
                })
                .collect();
            for (index, separator) in entries.iter().enumerate() {
                assert!(
                    ranges[index].1 < *separator,
                    "separator {} does not exceed its left child range on page {}",
                    index,
                    page.0
                );
                assert!(
                    *separator < ranges[index + 1].0,
                    "separator {} does not precede its right child range on page {}",
                    index,
                    page.0
                );
            }
            (ranges.first().unwrap().0, ranges.last().unwrap().1)
        }

        let mut pages = BTreeMap::new();
        let mut leaf_depth = None;
        visit(
            file,
            root,
            page_count,
            expected_codec,
            0,
            true,
            &mut pages,
            &mut leaf_depth,
        );
        ValidatedTree {
            pages,
            leaf_depth: leaf_depth.unwrap(),
        }
    }

    fn tree_path_for_key(tree: &ValidatedTree, root: PageId, key: &[u8; 32]) -> Vec<PageId> {
        let mut path = Vec::new();
        let mut page = root;
        loop {
            path.push(page);
            let node = tree.pages.get(&page).unwrap();
            match node.entries.binary_search(key) {
                Ok(_) => return path,
                Err(_) if node.children.is_empty() => return path,
                Err(index) => page = node.children[index],
            }
        }
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
    fn scan_cursor_reaches_every_entry_across_bounded_steps() {
        let mut s = Scratch::new();
        let mut root: Option<PageId> = None;
        let mut cur = PageAllocator::new(0, 0, Vec::new());
        let mut expect = BTreeMap::new();
        for i in 0..512u64 {
            let k = key(i);
            let v = loc(i);
            let bound = cur.page_count();
            root = Some(insert(&mut s.1, HEADER, &mut cur, root, &k, v, bound).unwrap());
            expect.insert(k, v);
        }
        let mut cursor = ScanCursor::new(root.unwrap());
        let mut out = Vec::new();
        let mut steps = 0usize;
        while !cursor.completed() {
            let page_count = cur.page_count();
            let step = scan_step_with_page_reader(&mut cursor, page_count, 1, None, |page| {
                let mut buf = [0u8; PAGE];
                read_exact_at(&mut s.1, page.offset(HEADER), &mut buf).map_err(io_err)?;
                Ok(buf)
            })
            .unwrap();
            assert!(step.pages_read <= 1);
            out.extend(step.entries);
            steps += 1;
            assert!(steps < 2048);
        }
        assert!(steps > 1);
        assert_eq!(out.into_iter().collect::<BTreeMap<_, _>>(), expect);
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
    fn node_decode_rejects_reserved_flags_unknown_codec_and_batch_codec_mismatch() {
        let canonical = Node::leaf(vec![(ordered_key(1), loc(1))]).encode().unwrap();
        for reserved in [0x02, 0x04, 0x08] {
            let mut malformed = canonical;
            malformed[1] |= reserved;
            refresh_node_crc(&mut malformed);
            let error = decode_node_page(&malformed)
                .err()
                .expect("reserved flag rejected");
            assert_eq!(error.code, loom_core::error::Code::CorruptObject);
            assert!(error.message.contains("reserved flag bit"));
        }

        let mut unknown_codec = canonical;
        unknown_codec[1] = (unknown_codec[1] & !NODE_FLAG_CODEC_MASK) | 0xF0;
        refresh_node_crc(&mut unknown_codec);
        let error = decode_node_page(&unknown_codec)
            .err()
            .expect("unknown codec rejected");
        assert_eq!(error.code, loom_core::error::Code::CorruptObject);
        assert!(
            error
                .message
                .contains("unknown btree node codec discriminator")
        );

        let mut scratch = Scratch::new();
        let mut initial_allocator = PageAllocator::new(0, 0, Vec::new());
        let root = build_packed_with_codec(
            &mut scratch.1,
            HEADER,
            &mut initial_allocator,
            &[(ordered_key(1), loc(1))],
            ValueCodecKind::PackedRecordRef,
        )
        .unwrap()
        .unwrap();
        let page_count = initial_allocator.page_count();
        let file_len = scratch.1.metadata().unwrap().len();
        let mut allocator = PageAllocator::new(page_count, 1, Vec::new());
        let error = batch_upsert_with_codec(
            &mut scratch.1,
            HEADER,
            &mut allocator,
            Some(root),
            &[(ordered_key(1), loc(2))],
            page_count,
            ValueCodecKind::RecordLoc,
        )
        .unwrap_err();
        assert_eq!(error.code, loom_core::error::Code::CorruptObject);
        assert!(error.message.contains("codec discriminator mismatch"));
        assert_eq!(allocator.page_count(), page_count);
        assert_eq!(scratch.1.metadata().unwrap().len(), file_len);
        assert!(allocator.take_free_map_extent_updates().is_empty());
    }

    #[test]
    fn batch_upsert_handles_empty_disjoint_replacement_and_duplicate_inputs() {
        let mut s = Scratch::new();
        let mut cur = PageAllocator::new(0, 0, Vec::new());
        let empty = batch_upsert(&mut s.1, HEADER, &mut cur, None, &[], 0).unwrap();
        assert_eq!(empty.root, None);
        assert_eq!(empty.stats, BatchUpsertStats::default());

        let updates = [
            (key(3), loc(3)),
            (key(1), loc(1)),
            (key(3), loc(30)),
            (key(2), loc(2)),
        ];
        let first = batch_upsert(&mut s.1, HEADER, &mut cur, None, &updates, 0).unwrap();
        let first_root = first.root.expect("batch creates a root");
        assert_eq!(first.stats.existing_pages_replaced, 0);
        assert_eq!(first.stats.new_split_pages_written, 0);
        let mut expected = BTreeMap::from([(key(1), loc(1)), (key(2), loc(2)), (key(3), loc(30))]);
        assert_eq!(
            load_all(&mut s.1, HEADER, first_root, cur.page_count()).unwrap(),
            expected
                .iter()
                .map(|(key, value)| (*key, *value))
                .collect::<Vec<_>>()
        );

        let bound = cur.page_count();
        let second = batch_upsert(
            &mut s.1,
            HEADER,
            &mut cur,
            Some(first_root),
            &[(key(2), loc(20)), (key(4), loc(4))],
            bound,
        )
        .unwrap();
        expected.insert(key(2), loc(20));
        expected.insert(key(4), loc(4));
        assert_eq!(second.stats.existing_pages_replaced, 1);
        assert_eq!(second.stats.new_split_pages_written, 0);
        assert_eq!(
            load_all(&mut s.1, HEADER, second.root.unwrap(), cur.page_count()).unwrap(),
            expected
                .iter()
                .map(|(key, value)| (*key, *value))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn batch_upsert_partial_multilevel_paths_preserve_untouched_pages_and_structure() {
        let mut scratch = Scratch::new();
        let mut initial_allocator = PageAllocator::new(0, 0, Vec::new());
        let original: Vec<_> = (0..4_096u64)
            .map(|index| (ordered_key(index * 2), loc(index)))
            .collect();
        let root = build_packed(&mut scratch.1, HEADER, &mut initial_allocator, &original)
            .unwrap()
            .unwrap();
        let published_page_count = initial_allocator.page_count();
        let old_tree = validate_tree_structure(
            &mut scratch.1,
            root,
            published_page_count,
            ValueCodecKind::RecordLoc,
        );
        assert!(
            old_tree.leaf_depth >= 2,
            "fixture must contain multiple internal levels"
        );

        let leaves: Vec<_> = old_tree
            .pages
            .iter()
            .filter(|(_, node)| node.children.is_empty())
            .collect();
        let (replace_page, replace_leaf) = leaves.first().copied().unwrap();
        let (insert_page, insert_leaf) = leaves
            .iter()
            .rev()
            .copied()
            .find(|(page, node)| {
                **page != *replace_page
                    && node.entries.len() < ValueCodecKind::RecordLoc.max_entries()
            })
            .unwrap();
        let (untouched_page, untouched_leaf) = leaves
            .iter()
            .copied()
            .find(|(page, _)| **page != *replace_page && **page != *insert_page)
            .unwrap();
        let replaced_key = replace_leaf.entries[0];
        let left_insert_bound = insert_leaf.entries[0];
        let right_insert_bound = insert_leaf.entries[1];
        let mut left_number_bytes = [0u8; 8];
        left_number_bytes.copy_from_slice(&left_insert_bound[..8]);
        let inserted_key = ordered_key(u64::from_be_bytes(left_number_bytes) + 1);
        assert!(left_insert_bound < inserted_key && inserted_key < right_insert_bound);
        let untouched_key = untouched_leaf.entries[0];
        let untouched_value = get(
            &mut scratch.1,
            HEADER,
            Some(root),
            &untouched_key,
            published_page_count,
        )
        .unwrap()
        .unwrap();

        let affected_pages: BTreeSet<_> = [replaced_key, inserted_key]
            .into_iter()
            .flat_map(|key| tree_path_for_key(&old_tree, root, &key))
            .collect();
        assert!(affected_pages.contains(replace_page));
        assert!(affected_pages.contains(insert_page));
        assert!(!affected_pages.contains(untouched_page));
        let old_pages: BTreeSet<_> = old_tree.pages.keys().copied().collect();
        let expected_untouched: BTreeSet<_> =
            old_pages.difference(&affected_pages).copied().collect();

        let updates = [(inserted_key, loc(90_001)), (replaced_key, loc(90_002))];
        let mut allocator = PageAllocator::new(published_page_count, 1, Vec::new());
        let result = batch_upsert(
            &mut scratch.1,
            HEADER,
            &mut allocator,
            Some(root),
            &updates,
            published_page_count,
        )
        .unwrap();
        let new_root = result.root.unwrap();
        assert_eq!(
            result.stats.existing_pages_replaced,
            affected_pages.len() as u64,
            "each distinct affected path page is rewritten once"
        );
        assert_eq!(result.stats.new_split_pages_written, 0);
        assert_eq!(
            allocator.page_count() - published_page_count,
            affected_pages.len() as u64,
            "replacement-only paths plus one non-splitting insertion write one page per affected node"
        );

        let new_tree = validate_tree_structure(
            &mut scratch.1,
            new_root,
            allocator.page_count(),
            ValueCodecKind::RecordLoc,
        );
        assert_eq!(new_tree.leaf_depth, old_tree.leaf_depth);
        let new_pages: BTreeSet<_> = new_tree.pages.keys().copied().collect();
        assert_eq!(
            old_pages
                .intersection(&new_pages)
                .copied()
                .collect::<BTreeSet<_>>(),
            expected_untouched,
            "only untouched child pages retain their prior identities"
        );

        let mut reopened = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&scratch.0)
            .unwrap();
        assert_eq!(
            get(
                &mut reopened,
                HEADER,
                Some(new_root),
                &inserted_key,
                allocator.page_count()
            )
            .unwrap(),
            Some(loc(90_001))
        );
        assert_eq!(
            get(
                &mut reopened,
                HEADER,
                Some(new_root),
                &replaced_key,
                allocator.page_count()
            )
            .unwrap(),
            Some(loc(90_002))
        );
        assert_eq!(
            get(
                &mut reopened,
                HEADER,
                Some(new_root),
                &untouched_key,
                allocator.page_count()
            )
            .unwrap(),
            Some(untouched_value)
        );
    }

    #[test]
    fn batch_upsert_internal_separator_replacement_uses_last_duplicate() {
        let mut scratch = Scratch::new();
        let mut initial_allocator = PageAllocator::new(0, 0, Vec::new());
        let original: Vec<_> = (0..128u64)
            .map(|index| (ordered_key(index), loc(index)))
            .collect();
        let root = build_packed(&mut scratch.1, HEADER, &mut initial_allocator, &original)
            .unwrap()
            .unwrap();
        let page_count = initial_allocator.page_count();
        let mut root_page = [0u8; PAGE];
        read_exact_at(&mut scratch.1, root.offset(HEADER), &mut root_page).unwrap();
        let old_root = decode_node_page(&root_page).unwrap();
        assert!(!old_root.is_leaf);
        let separator = old_root.entries[0].0;
        let old_children = old_root.children;

        let mut allocator = PageAllocator::new(page_count, 1, Vec::new());
        let result = batch_upsert(
            &mut scratch.1,
            HEADER,
            &mut allocator,
            Some(root),
            &[(separator, loc(80_001)), (separator, loc(80_002))],
            page_count,
        )
        .unwrap();
        assert_eq!(result.stats.existing_pages_replaced, 1);
        assert_eq!(result.stats.new_split_pages_written, 0);
        assert_eq!(allocator.page_count() - page_count, 1);
        let new_root = result.root.unwrap();
        let mut new_root_page = [0u8; PAGE];
        read_exact_at(&mut scratch.1, new_root.offset(HEADER), &mut new_root_page).unwrap();
        let new_root_node = decode_node_page(&new_root_page).unwrap();
        assert_eq!(new_root_node.children, old_children);
        assert_eq!(
            get(
                &mut scratch.1,
                HEADER,
                Some(new_root),
                &separator,
                allocator.page_count()
            )
            .unwrap(),
            Some(loc(80_002))
        );
    }

    #[test]
    fn batch_upsert_rejects_oversized_input_before_allocation_or_write() {
        let mut scratch = Scratch::new();
        let file_len = scratch.1.metadata().unwrap().len();
        let oversized = vec![(ordered_key(1), loc(1)); MAX_BATCH_UPSERT_ENTRIES + 1];
        let mut allocator = PageAllocator::new(0, 1, Vec::new());
        let error =
            batch_upsert(&mut scratch.1, HEADER, &mut allocator, None, &oversized, 0).unwrap_err();
        assert_eq!(error.code, loom_core::error::Code::CorruptObject);
        assert!(error.message.contains("exceeds entry limit"));
        assert_eq!(allocator.page_count(), 0);
        assert_eq!(scratch.1.metadata().unwrap().len(), file_len);
        assert!(allocator.take_free_map_extent_updates().is_empty());
    }

    #[test]
    fn batch_upsert_rewrites_each_affected_path_once_across_internal_splits() {
        let mut s = Scratch::new();
        let mut cur = PageAllocator::new(0, 0, Vec::new());
        let mut original: Vec<_> = (0..4_096u64).map(|i| (key(i), loc(i))).collect();
        original.sort_by_key(|entry| entry.0);
        let root = build_packed(&mut s.1, HEADER, &mut cur, &original)
            .unwrap()
            .expect("non-empty tree");
        let published_page_count = cur.page_count();
        let old_pages = collect_pages(&mut s.1, HEADER, root, published_page_count).unwrap();
        let mut batch_allocator = PageAllocator::new(published_page_count, 1, Vec::new());
        let mut expected = original.into_iter().collect::<BTreeMap<_, _>>();
        let updates: Vec<_> = (0..2_048u64)
            .flat_map(|i| {
                [
                    (key(i), loc(i + 10_000)),
                    (key(i + 10_000), loc(i + 20_000)),
                ]
            })
            .collect();
        for (key, value) in &updates {
            expected.insert(*key, *value);
        }
        let result = batch_upsert(
            &mut s.1,
            HEADER,
            &mut batch_allocator,
            Some(root),
            &updates,
            published_page_count,
        )
        .unwrap();
        let new_root = result.root.unwrap();
        let validated = validate_tree_structure(
            &mut s.1,
            new_root,
            batch_allocator.page_count(),
            ValueCodecKind::RecordLoc,
        );
        assert!(validated.leaf_depth >= 2);
        assert_eq!(
            load_all(&mut s.1, HEADER, new_root, batch_allocator.page_count()).unwrap(),
            expected
                .iter()
                .map(|(key, value)| (*key, *value))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            result.stats.existing_pages_replaced as usize,
            old_pages.len(),
            "a full-keyspace batch replaces each old page exactly once"
        );
        assert!(result.stats.new_split_pages_written > 0);

        let mut reopened = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&s.0)
            .unwrap();
        assert_eq!(
            load_all(
                &mut reopened,
                HEADER,
                new_root,
                batch_allocator.page_count()
            )
            .unwrap(),
            expected
                .iter()
                .map(|(key, value)| (*key, *value))
                .collect::<Vec<_>>()
        );

        let reclaimed_pages: u64 = batch_allocator
            .take_free_map_extent_updates()
            .into_iter()
            .filter_map(|update| match update {
                crate::pagemap::FreeMapExtentUpdate::Upsert(run) => Some(run.len),
                crate::pagemap::FreeMapExtentUpdate::Delete(_) => None,
            })
            .sum();
        assert_eq!(reclaimed_pages, result.stats.existing_pages_replaced);
    }

    #[derive(Debug)]
    struct ObservedBacking<'a> {
        file: &'a mut File,
        reads: u64,
        writes: u64,
        grows: u64,
        syncs: u64,
    }

    impl ObservedBacking<'_> {
        fn new(file: &mut File) -> ObservedBacking<'_> {
            ObservedBacking {
                file,
                reads: 0,
                writes: 0,
                grows: 0,
                syncs: 0,
            }
        }
    }

    impl BackingIo for ObservedBacking<'_> {
        fn pread(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
            self.reads += 1;
            self.file.pread(off, buf)
        }

        fn pwrite(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()> {
            self.writes += 1;
            self.file.pwrite(off, buf)
        }

        fn size(&self) -> std::io::Result<u64> {
            self.file.size()
        }

        fn grow(&mut self, len: u64) -> std::io::Result<()> {
            self.grows += 1;
            self.file.grow(len)
        }

        fn fsync(&mut self) -> std::io::Result<()> {
            self.syncs += 1;
            self.file.fsync()
        }
    }

    #[test]
    fn prepared_delta_planning_is_read_only_and_normalizes_inputs() {
        let mut scratch = Scratch::new();
        let mut initial_allocator = PageAllocator::new(0, 0, Vec::new());
        let original: Vec<_> = (0..128u64)
            .map(|index| (ordered_key(index), loc(index)))
            .collect();
        let root = build_packed(&mut scratch.1, HEADER, &mut initial_allocator, &original)
            .unwrap()
            .unwrap();
        let page_count = initial_allocator.page_count();
        let file_len = scratch.1.metadata().unwrap().len();
        let mut untouched_allocator = PageAllocator::new(page_count, 9, Vec::new());
        let allocator_pages = untouched_allocator.page_count();
        let allocator_free = untouched_allocator.snapshot_free();
        let allocator_stats = untouched_allocator.transaction_stats();
        let absent = ordered_key(10_000);
        let inserted = ordered_key(10_001);
        let prepared = {
            let mut observed = ObservedBacking::new(&mut scratch.1);
            let prepared = prepare_delete_upsert_delta(
                &mut observed,
                HEADER,
                Some(root),
                page_count,
                ValueCodecKind::RecordLoc,
                &[absent, absent],
                &[
                    (ordered_key(7), loc(7)),
                    (inserted, loc(70_001)),
                    (inserted, loc(70_002)),
                ],
            )
            .unwrap();
            assert!(observed.reads > 0);
            assert_eq!(observed.writes, 0);
            assert_eq!(observed.grows, 0);
            assert_eq!(observed.syncs, 0);
            prepared
        };
        assert_eq!(scratch.1.metadata().unwrap().len(), file_len);
        assert_eq!(untouched_allocator.page_count(), allocator_pages);
        assert_eq!(untouched_allocator.snapshot_free(), allocator_free);
        assert_eq!(untouched_allocator.transaction_stats(), allocator_stats);
        assert_eq!(prepared.deletes, vec![absent]);
        assert_eq!(
            prepared.upserts,
            vec![
                (ordered_key(7), PageTreeValue::RecordLoc(loc(7))),
                (inserted, PageTreeValue::RecordLoc(loc(70_002)))
            ]
        );
        assert!(prepared.decisions.iter().any(|decision| {
            decision.kind == PreparedTreeDecisionKind::DeleteAbsent && decision.key == absent
        }));
        assert!(prepared.decisions.iter().any(|decision| {
            decision.kind == PreparedTreeDecisionKind::UpsertUnchanged
                && decision.key == ordered_key(7)
        }));

        let noop = prepare_delete_upsert_delta(
            &mut scratch.1,
            HEADER,
            Some(root),
            page_count,
            ValueCodecKind::RecordLoc,
            &[absent],
            &[(ordered_key(7), loc(7))],
        )
        .unwrap();
        assert_eq!(noop.allocation_calls, 0);
        assert!(noop.affected_pages.is_empty());
        assert_eq!(noop.result_root, Some(PreparedRef::Existing(root)));
        let file_len = scratch.1.metadata().unwrap().len();
        let applied = {
            let mut observed = ObservedBacking::new(&mut scratch.1);
            let applied = apply_prepared_delta(
                &mut observed,
                HEADER,
                &mut untouched_allocator,
                Some(root),
                page_count,
                ValueCodecKind::RecordLoc,
                noop,
            )
            .unwrap();
            assert_eq!(observed.writes, 0);
            assert_eq!(observed.grows, 0);
            applied
        };
        assert_eq!(applied.root, Some(root));
        assert_eq!(scratch.1.metadata().unwrap().len(), file_len);
        assert!(
            untouched_allocator
                .take_free_map_extent_updates()
                .is_empty()
        );
    }

    #[test]
    fn prepared_delta_mixed_multilevel_apply_preserves_unaffected_pages_and_reopens() {
        let mut scratch = Scratch::new();
        let mut initial_allocator = PageAllocator::new(0, 0, Vec::new());
        let original: Vec<_> = (0..4_096u64)
            .map(|index| (ordered_key(index * 2), loc(index)))
            .collect();
        let root = build_packed(&mut scratch.1, HEADER, &mut initial_allocator, &original)
            .unwrap()
            .unwrap();
        let page_count = initial_allocator.page_count();
        let old_tree =
            validate_tree_structure(&mut scratch.1, root, page_count, ValueCodecKind::RecordLoc);
        assert!(old_tree.leaf_depth >= 2);
        let leaves: Vec<_> = old_tree
            .pages
            .iter()
            .filter(|(_, node)| node.children.is_empty())
            .collect();
        let deleted = leaves.first().unwrap().1.entries[0];
        let replaced = leaves.last().unwrap().1.entries[0];
        let insert_leaf = leaves[leaves.len() / 2].1;
        let mut lower = [0u8; 8];
        lower.copy_from_slice(&insert_leaf.entries[0][..8]);
        let inserted = ordered_key(u64::from_be_bytes(lower) + 1);
        let prepared = prepare_delete_upsert_delta(
            &mut scratch.1,
            HEADER,
            Some(root),
            page_count,
            ValueCodecKind::RecordLoc,
            &[deleted, ordered_key(99_999)],
            &[(replaced, loc(80_001)), (inserted, loc(80_002))],
        )
        .unwrap();
        let affected: BTreeSet<_> = prepared.affected_pages.iter().copied().collect();
        let old_pages: BTreeSet<_> = old_tree.pages.keys().copied().collect();
        assert!(affected.is_subset(&old_pages));
        assert_eq!(affected.len(), prepared.affected_pages.len());
        let expected_untouched: BTreeSet<_> = old_pages.difference(&affected).copied().collect();
        let planned_allocations = prepared.allocation_calls;
        let mut allocator = PageAllocator::new(page_count, 1, Vec::new());
        let applied = apply_prepared_delta(
            &mut scratch.1,
            HEADER,
            &mut allocator,
            Some(root),
            page_count,
            ValueCodecKind::RecordLoc,
            prepared,
        )
        .unwrap();
        assert_eq!(applied.allocation_calls, planned_allocations);
        assert_eq!(applied.superseded_pages, affected.len() as u64);
        assert_eq!(allocator.page_count() - page_count, planned_allocations);
        let new_root = applied.root.unwrap();
        let new_tree = validate_tree_structure(
            &mut scratch.1,
            new_root,
            allocator.page_count(),
            ValueCodecKind::RecordLoc,
        );
        let new_pages: BTreeSet<_> = new_tree.pages.keys().copied().collect();
        assert_eq!(
            old_pages
                .intersection(&new_pages)
                .copied()
                .collect::<BTreeSet<_>>(),
            expected_untouched
        );
        assert_eq!(
            get(
                &mut scratch.1,
                HEADER,
                Some(new_root),
                &deleted,
                allocator.page_count()
            )
            .unwrap(),
            None
        );
        assert_eq!(
            get(
                &mut scratch.1,
                HEADER,
                Some(new_root),
                &replaced,
                allocator.page_count()
            )
            .unwrap(),
            Some(loc(80_001))
        );
        assert_eq!(
            get(
                &mut scratch.1,
                HEADER,
                Some(new_root),
                &inserted,
                allocator.page_count()
            )
            .unwrap(),
            Some(loc(80_002))
        );

        let mut reopened = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&scratch.0)
            .unwrap();
        assert_eq!(
            get(
                &mut reopened,
                HEADER,
                Some(new_root),
                &deleted,
                allocator.page_count()
            )
            .unwrap(),
            None
        );
        assert_eq!(
            get(
                &mut reopened,
                HEADER,
                Some(new_root),
                &inserted,
                allocator.page_count()
            )
            .unwrap(),
            Some(loc(80_002))
        );
    }

    #[test]
    fn prepared_delta_records_separator_borrow_merge_root_collapse_and_split_choices() {
        let mut scratch = Scratch::new();
        let mut allocator = PageAllocator::new(0, 0, Vec::new());
        let entries: Vec<_> = (0..64u64)
            .map(|index| (ordered_key(index), loc(index)))
            .collect();
        let root = build_packed(&mut scratch.1, HEADER, &mut allocator, &entries)
            .unwrap()
            .unwrap();
        let page_count = allocator.page_count();
        let separator_delete = prepare_delete_upsert_delta(
            &mut scratch.1,
            HEADER,
            Some(root),
            page_count,
            ValueCodecKind::RecordLoc,
            &[ordered_key(32)],
            &[],
        )
        .unwrap();
        assert!(separator_delete.decisions.iter().any(|decision| {
            decision.kind == PreparedTreeDecisionKind::DeletePredecessor
                && decision.key == ordered_key(32)
        }));
        let separator_replace = prepare_delete_upsert_delta(
            &mut scratch.1,
            HEADER,
            Some(root),
            page_count,
            ValueCodecKind::RecordLoc,
            &[],
            &[(ordered_key(32), loc(32_000))],
        )
        .unwrap();
        assert_eq!(separator_replace.allocation_calls, 1);
        assert!(separator_replace.decisions.iter().any(|decision| {
            decision.kind == PreparedTreeDecisionKind::UpsertReplace
                && decision.key == ordered_key(32)
        }));

        let collapse = prepare_delete_upsert_delta(
            &mut scratch.1,
            HEADER,
            Some(root),
            page_count,
            ValueCodecKind::RecordLoc,
            &[ordered_key(33), ordered_key(34)],
            &[],
        )
        .unwrap();
        for expected in [
            PreparedTreeDecisionKind::DeleteBorrowLeft,
            PreparedTreeDecisionKind::DeleteMergeLeft,
            PreparedTreeDecisionKind::DeleteRootCollapse,
        ] {
            assert!(
                collapse
                    .decisions
                    .iter()
                    .any(|decision| decision.kind == expected)
            );
        }
        let mut collapse_allocator = PageAllocator::new(page_count, 1, Vec::new());
        let collapsed = apply_prepared_delta(
            &mut scratch.1,
            HEADER,
            &mut collapse_allocator,
            Some(root),
            page_count,
            ValueCodecKind::RecordLoc,
            collapse,
        )
        .unwrap();
        let collapsed_tree = validate_tree_structure(
            &mut scratch.1,
            collapsed.root.unwrap(),
            collapse_allocator.page_count(),
            ValueCodecKind::RecordLoc,
        );
        assert_eq!(collapsed_tree.leaf_depth, 0);

        let mut split_scratch = Scratch::new();
        let mut split_allocator = PageAllocator::new(0, 0, Vec::new());
        let full_leaf: Vec<_> = (0..ValueCodecKind::RecordLoc.max_entries() as u64)
            .map(|index| (ordered_key(index * 2), loc(index)))
            .collect();
        let split_root = build_packed(
            &mut split_scratch.1,
            HEADER,
            &mut split_allocator,
            &full_leaf,
        )
        .unwrap()
        .unwrap();
        let split_bound = split_allocator.page_count();
        let split = prepare_delete_upsert_delta(
            &mut split_scratch.1,
            HEADER,
            Some(split_root),
            split_bound,
            ValueCodecKind::RecordLoc,
            &[],
            &[(ordered_key(1), loc(100_001))],
        )
        .unwrap();
        assert!(
            split
                .decisions
                .iter()
                .any(|decision| decision.kind == PreparedTreeDecisionKind::UpsertSplit)
        );
        assert!(
            split
                .decisions
                .iter()
                .any(|decision| decision.kind == PreparedTreeDecisionKind::UpsertRootConstruct)
        );
        assert_eq!(split.allocation_calls, 3);
    }

    #[test]
    fn prepared_delta_rejects_stale_identity_before_mutation_and_write_failure_preserves_source() {
        let mut scratch = Scratch::new();
        let mut initial_allocator = PageAllocator::new(0, 0, Vec::new());
        let entries: Vec<_> = (0..512u64)
            .map(|index| (ordered_key(index), loc(index)))
            .collect();
        let root = build_packed(&mut scratch.1, HEADER, &mut initial_allocator, &entries)
            .unwrap()
            .unwrap();
        let page_count = initial_allocator.page_count();
        let prepared = prepare_delete_upsert_delta(
            &mut scratch.1,
            HEADER,
            Some(root),
            page_count,
            ValueCodecKind::RecordLoc,
            &[ordered_key(10)],
            &[(ordered_key(20), loc(20_000)), (ordered_key(700), loc(700))],
        )
        .unwrap();
        assert!(prepared.allocation_calls >= 2);
        let file_len = scratch.1.metadata().unwrap().len();
        let mut rejected_allocator = PageAllocator::new(page_count, 1, Vec::new());
        {
            let mut observed = ObservedBacking::new(&mut scratch.1);
            for (source_root, bound, codec) in [
                (None, page_count, ValueCodecKind::RecordLoc),
                (Some(root), page_count + 1, ValueCodecKind::RecordLoc),
                (Some(root), page_count, ValueCodecKind::PackedRecordRef),
            ] {
                let error = apply_prepared_delta(
                    &mut observed,
                    HEADER,
                    &mut rejected_allocator,
                    source_root,
                    bound,
                    codec,
                    prepared.clone(),
                )
                .unwrap_err();
                assert_eq!(error.code, loom_core::error::Code::CorruptObject);
                assert!(error.message.contains("source identity mismatch"));
            }
            assert_eq!(observed.writes, 0);
            assert_eq!(observed.grows, 0);
        }
        assert_eq!(rejected_allocator.page_count(), page_count);
        assert!(rejected_allocator.take_free_map_extent_updates().is_empty());
        assert_eq!(scratch.1.metadata().unwrap().len(), file_len);

        let mut failed_allocator = PageAllocator::new(page_count, 1, Vec::new());
        let error = {
            let mut failing = FailNthWrite {
                file: &mut scratch.1,
                writes: 0,
                fail_on: 2,
            };
            apply_prepared_delta(
                &mut failing,
                HEADER,
                &mut failed_allocator,
                Some(root),
                page_count,
                ValueCodecKind::RecordLoc,
                prepared,
            )
            .unwrap_err()
        };
        assert_eq!(error.code, loom_core::error::Code::Io);
        assert!(failed_allocator.take_free_map_extent_updates().is_empty());
        assert_eq!(
            load_all(&mut scratch.1, HEADER, root, page_count).unwrap(),
            entries
        );
    }

    #[test]
    fn prepared_delta_rejects_oversized_input_before_read_write_or_allocator_work() {
        let mut scratch = Scratch::new();
        let mut allocator = PageAllocator::new(0, 1, Vec::new());
        let oversized = vec![[0u8; 32]; MAX_BATCH_UPSERT_ENTRIES + 1];
        let file_len = scratch.1.metadata().unwrap().len();
        let error = {
            let mut observed = ObservedBacking::new(&mut scratch.1);
            let error = prepare_delete_upsert_delta(
                &mut observed,
                HEADER,
                None,
                0,
                ValueCodecKind::RecordLoc,
                &oversized,
                &[],
            )
            .err()
            .expect("oversized prepared delta rejected");
            assert_eq!(observed.reads, 0);
            assert_eq!(observed.writes, 0);
            assert_eq!(observed.grows, 0);
            error
        };
        assert_eq!(error.code, loom_core::error::Code::CorruptObject);
        assert!(error.message.contains("exceeds entry limit"));
        assert_eq!(allocator.page_count(), 0);
        assert!(allocator.take_free_map_extent_updates().is_empty());
        assert_eq!(scratch.1.metadata().unwrap().len(), file_len);
    }

    #[derive(Debug)]
    struct FailNthWrite<'a> {
        file: &'a mut File,
        writes: usize,
        fail_on: usize,
    }

    impl BackingIo for FailNthWrite<'_> {
        fn pread(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
            self.file.pread(off, buf)
        }

        fn pwrite(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()> {
            self.writes += 1;
            if self.writes == self.fail_on {
                return Err(std::io::Error::other("injected batch write failure"));
            }
            self.file.pwrite(off, buf)
        }

        fn size(&self) -> std::io::Result<u64> {
            self.file.size()
        }

        fn grow(&mut self, len: u64) -> std::io::Result<()> {
            self.file.grow(len)
        }

        fn fsync(&mut self) -> std::io::Result<()> {
            self.file.fsync()
        }
    }

    #[test]
    fn batch_upsert_write_failure_preserves_the_published_root() {
        let mut s = Scratch::new();
        let mut cur = PageAllocator::new(0, 0, Vec::new());
        let mut original: Vec<_> = (0..512u64).map(|i| (key(i), loc(i))).collect();
        original.sort_by_key(|entry| entry.0);
        let root = build_packed(&mut s.1, HEADER, &mut cur, &original)
            .unwrap()
            .expect("non-empty tree");
        let published_page_count = cur.page_count();
        let mut failed_allocator = PageAllocator::new(published_page_count, 1, Vec::new());
        let error = {
            let mut failing = FailNthWrite {
                file: &mut s.1,
                writes: 0,
                fail_on: 2,
            };
            batch_upsert(
                &mut failing,
                HEADER,
                &mut failed_allocator,
                Some(root),
                &[(key(10), loc(100_010)), (key(600), loc(100_600))],
                published_page_count,
            )
            .unwrap_err()
        };
        assert_eq!(error.code, loom_core::error::Code::Io);
        assert_eq!(
            load_all(&mut s.1, HEADER, root, published_page_count).unwrap(),
            original
        );
    }

    #[test]
    fn batch_upsert_with_codec_preserves_the_requested_family_codec() {
        let mut s = Scratch::new();
        let mut initial_allocator = PageAllocator::new(0, 0, Vec::new());
        let mut original: Vec<_> = (0..128u64).map(|i| (key(i), loc(i))).collect();
        original.sort_by_key(|entry| entry.0);
        let root = build_packed_with_codec(
            &mut s.1,
            HEADER,
            &mut initial_allocator,
            &original,
            ValueCodecKind::PackedRecordRef,
        )
        .unwrap()
        .expect("non-empty tree");
        let page_count = initial_allocator.page_count();
        let mut allocator = PageAllocator::new(page_count, 1, Vec::new());
        let result = batch_upsert_with_codec(
            &mut s.1,
            HEADER,
            &mut allocator,
            Some(root),
            &[(key(7), loc(700)), (key(300), loc(300))],
            page_count,
            ValueCodecKind::PackedRecordRef,
        )
        .unwrap();
        assert_every_reachable_node_uses_codec(
            &mut s,
            result.root.unwrap(),
            allocator.page_count(),
            ValueCodecKind::PackedRecordRef,
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

    fn assert_every_reachable_node_uses_codec(
        s: &mut Scratch,
        root: PageId,
        page_count: u64,
        codec: ValueCodecKind,
    ) {
        let pages = collect_pages_with_codec(&mut s.1, HEADER, root, page_count, codec).unwrap();
        assert!(!pages.is_empty());
        for page in pages {
            let mut raw = [0u8; PAGE];
            read_exact_at(&mut s.1, page.offset(HEADER), &mut raw).unwrap();
            let node = decode_node_page(&raw).unwrap();
            assert_eq!(node.codec, codec, "wrong codec at page {}", page.0);
        }
        let (_page, inspection) =
            inspect_tree_codec(&mut s.1, HEADER, root, page_count, codec).unwrap();
        assert_eq!(inspection.failure, None);
    }

    #[test]
    fn build_packed_with_codec_uses_requested_codec_for_every_reachable_node() {
        for codec in [ValueCodecKind::RecordLoc, ValueCodecKind::PackedRecordRef] {
            for &n in &[1u64, 63, 64, 65, 4096, 5000] {
                let mut s = Scratch::new();
                let mut cur = PageAllocator::new(0, 0, Vec::new());
                let mut sorted: Vec<([u8; 32], RecordLoc)> =
                    (0..n).map(|i| (key(i), loc(i))).collect();
                sorted.sort_by_key(|a| a.0);
                let root = build_packed_with_codec(&mut s.1, HEADER, &mut cur, &sorted, codec)
                    .unwrap()
                    .expect("non-empty tree");

                assert_every_reachable_node_uses_codec(&mut s, root, cur.page_count(), codec);
                let all =
                    load_all_with_codec(&mut s.1, HEADER, root, cur.page_count(), codec).unwrap();
                assert_eq!(all, sorted, "bulk build changed entries at n={n}");
                if codec == ValueCodecKind::PackedRecordRef {
                    let error = load_all(&mut s.1, HEADER, root, cur.page_count()).unwrap_err();
                    assert!(error.message.contains("codec discriminator mismatch"));
                }
                for i in [0, n / 2, n - 1] {
                    let k = key(i);
                    assert_eq!(
                        get_with_codec(&mut s.1, HEADER, Some(root), &k, cur.page_count(), codec,)
                            .unwrap(),
                        Some(loc(i)),
                        "boundary lookup failed at n={n} i={i}"
                    );
                }

                let next_key = key(n + 1_000_000);
                let bound = cur.page_count();
                let next_root = insert_with_codec(
                    &mut s.1,
                    HEADER,
                    &mut cur,
                    Some(root),
                    &next_key,
                    loc(n + 1_000_000),
                    bound,
                    codec,
                )
                .unwrap();
                assert_every_reachable_node_uses_codec(&mut s, next_root, cur.page_count(), codec);
                assert_eq!(
                    get_with_codec(
                        &mut s.1,
                        HEADER,
                        Some(next_root),
                        &next_key,
                        cur.page_count(),
                        codec,
                    )
                    .unwrap(),
                    Some(loc(n + 1_000_000))
                );
            }
        }
    }

    #[test]
    fn inspect_tree_codec_rejects_correct_root_with_invalid_descendant() {
        let mut s = Scratch::new();
        let mut cur = PageAllocator::new(0, 0, Vec::new());
        let mut sorted: Vec<([u8; 32], RecordLoc)> = (0..65).map(|i| (key(i), loc(i))).collect();
        sorted.sort_by_key(|a| a.0);
        let root = build_packed_with_codec(
            &mut s.1,
            HEADER,
            &mut cur,
            &sorted,
            ValueCodecKind::PackedRecordRef,
        )
        .unwrap()
        .expect("non-empty tree");
        let descendant = collect_pages_with_codec(
            &mut s.1,
            HEADER,
            root,
            cur.page_count(),
            ValueCodecKind::PackedRecordRef,
        )
        .unwrap()
        .into_iter()
        .find(|page| *page != root)
        .expect("descendant page");
        let mut raw = [0u8; PAGE];
        read_exact_at(&mut s.1, descendant.offset(HEADER), &mut raw).unwrap();
        raw[1] = (raw[1] & !NODE_FLAG_CODEC_MASK) | ValueCodecKind::RecordLoc.discriminator();
        let crc = crc32c(&raw[..BODY_END]);
        raw[BODY_END..].copy_from_slice(&crc.to_le_bytes());
        write_at(&mut s.1, descendant.offset(HEADER), &raw).unwrap();

        let (failure_page, inspection) = inspect_tree_codec(
            &mut s.1,
            HEADER,
            root,
            cur.page_count(),
            ValueCodecKind::PackedRecordRef,
        )
        .unwrap();
        assert_eq!(failure_page, descendant);
        assert_eq!(
            inspection.failure,
            Some("btree_node_codec_discriminator_mismatch")
        );
    }

    #[test]
    fn exact_codec_traversal_rejects_mixed_locator_descendant_before_mutation() {
        let mut scratch = Scratch::new();
        let mut allocator = PageAllocator::new(0, 0, Vec::new());
        let entries = (0..128u64)
            .map(|index| (ordered_key(index), loc(index)))
            .collect::<Vec<_>>();
        let root = build_packed(&mut scratch.1, HEADER, &mut allocator, &entries)
            .unwrap()
            .unwrap();
        let page_count = allocator.page_count();
        let mut root_page = [0u8; PAGE];
        read_exact_at(&mut scratch.1, root.offset(HEADER), &mut root_page).unwrap();
        let root_node = decode_node_page_with_codec(&root_page, ValueCodecKind::RecordLoc).unwrap();
        let descendant = root_node.children[0];
        let mut descendant_page = [0u8; PAGE];
        read_exact_at(
            &mut scratch.1,
            descendant.offset(HEADER),
            &mut descendant_page,
        )
        .unwrap();
        let target = decode_node_page_with_codec(&descendant_page, ValueCodecKind::RecordLoc)
            .unwrap()
            .entries[0]
            .0;
        descendant_page[1] = (descendant_page[1] & !NODE_FLAG_CODEC_MASK)
            | ValueCodecKind::PackedRecordRef.discriminator();
        refresh_node_crc(&mut descendant_page);
        write_at(&mut scratch.1, descendant.offset(HEADER), &descendant_page).unwrap();

        let assert_mismatch = |error: loom_core::error::LoomError| {
            assert_eq!(error.code, loom_core::error::Code::CorruptObject);
            assert!(error.message.contains("codec discriminator mismatch"));
        };
        assert_mismatch(get(&mut scratch.1, HEADER, Some(root), &target, page_count).unwrap_err());
        assert_mismatch(
            predecessor(&mut scratch.1, HEADER, Some(root), &target, page_count).unwrap_err(),
        );
        assert_mismatch(load_all(&mut scratch.1, HEADER, root, page_count).unwrap_err());
        assert_mismatch(
            range(
                &mut scratch.1,
                HEADER,
                root,
                &[0u8; 32],
                &[0xFF; 32],
                page_count,
            )
            .unwrap_err(),
        );
        assert_mismatch(collect_pages(&mut scratch.1, HEADER, root, page_count).unwrap_err());
        assert_mismatch(tree_depth(&mut scratch.1, HEADER, root, page_count).unwrap_err());
        assert_mismatch(
            get_with_page_reader(Some(root), &target, page_count, |page| {
                let mut bytes = [0u8; PAGE];
                read_exact_at(&mut scratch.1, page.offset(HEADER), &mut bytes).map_err(io_err)?;
                Ok(bytes)
            })
            .unwrap_err(),
        );
        let mut cursor = ScanCursor::new(root);
        assert_mismatch(
            scan_step_with_page_reader(&mut cursor, page_count, 8, None, |page| {
                let mut bytes = [0u8; PAGE];
                read_exact_at(&mut scratch.1, page.offset(HEADER), &mut bytes).map_err(io_err)?;
                Ok(bytes)
            })
            .unwrap_err(),
        );

        let file_len = scratch.1.metadata().unwrap().len();
        let mut mutation_allocator = PageAllocator::new(page_count, 1, Vec::new());
        assert_mismatch(
            batch_upsert(
                &mut scratch.1,
                HEADER,
                &mut mutation_allocator,
                Some(root),
                &[(target, loc(1_000))],
                page_count,
            )
            .unwrap_err(),
        );
        assert_eq!(mutation_allocator.page_count(), page_count);
        assert!(mutation_allocator.take_free_map_extent_updates().is_empty());
        assert_eq!(scratch.1.metadata().unwrap().len(), file_len);
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
