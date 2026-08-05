//! The page allocator and the free-page map.
//!
//! The allocator hands out [`PageId`]s. It reuses a page extended earlier in the same transaction
//! first (safe: no committed generation references it, so a crash that reverts to the prior
//! generation cannot observe the overwrite), then a prior-generation free page aged past the
//! crash-safe window, before extending the array. Reusing same-transaction pages bounds the cost of a
//! many-node operation (e.g. a bulk delete) to its working set rather than letting every copy-on-write
//! path extend the file. Freed pages live in the free-page map: an extent tree of free page-runs,
//! persisted sorted and CRC'd on its own pages so reuse survives a reopen; the map's pages are carved
//! out of the free set before it is written, so the map never lists its own pages.

use crate::page::{
    METADATA_BOOTSTRAP_MAX_EXTENTS, MetadataBootstrapExtent, MetadataBootstrapReserve,
};
use crate::page::{PAGE_SIZE, PageId};
use crate::pagebtree;
use crate::{BackingIo, REUSE_SAFE_WINDOW, corrupt, crc32c};
use loom_core::error::{Code, LoomError, Result};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::fs::File;

const LEGACY_MAP_MAGIC: u8 = 0xB4;
const EXTENT_MAGIC: &[u8; 8] = b"LFMEXT1\0";
pub(crate) const FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES: u64 = 512;
pub(crate) const FOREGROUND_TRANSACTION_METADATA_LIMIT_BYTES: u64 =
    FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES * PAGE_SIZE;
pub(crate) const METADATA_BOOTSTRAP_REFILL_THRESHOLD_PAGES: u64 = 8;
pub(crate) const METADATA_BOOTSTRAP_TARGET_PAGES: u64 = 16;
pub(crate) const METADATA_BOOTSTRAP_CAPACITY_PAGES: u64 =
    FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES;

/// A run of contiguous free pages, tagged with the generation that freed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FreePageRun {
    pub(crate) start: u64,
    pub(crate) len: u64,
    pub(crate) freed_gen: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CapturedFreeRun {
    pub(crate) run: FreePageRun,
    pub(crate) cursor_start: u64,
    pub(crate) cursor_end: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CapturedFreeAllocationAuthority {
    pub(crate) runs: Vec<CapturedFreeRun>,
    pub(crate) consumed_through: u64,
    pub(crate) page_count: u64,
}

/// Hands out page-runs for one transaction. Reuses pages freed earlier in this transaction (extended
/// past the prior committed page count, so reusing them is crash-safe) and prior-generation runs aged
/// past the recoverable window, before extending the array. Collects the runs this transaction frees
/// so they enter the free-page map on commit.
#[derive(Clone)]
pub(crate) struct PageAllocator {
    end: u64,
    start_end: u64, // page count at the start of this transaction
    txn_gen: u64,
    reuse_current_free: bool,
    reusable_runs: Option<Vec<FreePageRun>>,
    publication_eligible_runs: Vec<FreePageRun>,
    captured_free_authority: Option<CapturedFreeAllocationAuthority>,
    publication_reserve: BTreeMap<u64, (u64, u64)>,
    publication_reserved_pages: u64,
    publication_reserve_active: bool,
    metadata_bootstrap_reserve: BTreeMap<u64, u64>,
    metadata_bootstrap_owning_generation: u64,
    allocated_runs: Vec<(u64, u64)>,
    active_allocated_pages: BTreeSet<u64>,
    reuse_before: Option<u64>,
    initial_free: Vec<FreePageRun>,
    initial_free_by_start: BTreeMap<u64, FreePageRun>,
    free: BTreeMap<u64, (u64, u64)>, // prior-generation free runs: start -> (len, freed_gen)
    txn_freed: BTreeMap<u64, u64>,   // runs freed this transaction: start -> len
    deferred_freed: BTreeMap<u64, u64>,
    dirty_free_ranges: BTreeMap<u64, u64>,
    suppress_free_map_tracking: bool,
    #[cfg(any(test, feature = "test-hooks"))]
    free_origins: BTreeMap<u64, &'static std::panic::Location<'static>>,
    #[cfg(any(test, feature = "test-hooks"))]
    transaction_stats: PageAllocatorTransactionStats,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PageAllocatorTransactionStats {
    pub(crate) publication_reserved_pages: u64,
    pub(crate) publication_reused_pages: u64,
    pub(crate) publication_unused_pages: u64,
    pub(crate) ordinary_reused_pages: u64,
    pub(crate) transaction_reused_pages: u64,
    pub(crate) extended_pages: u64,
    pub(crate) free_map_updates: u64,
    pub(crate) free_map_extent_deletes: u64,
    pub(crate) free_map_extent_upserts: u64,
    pub(crate) free_map_unique_btree_nodes_touched: u64,
    pub(crate) free_map_split_pages: u64,
    pub(crate) fixed_metadata_pages: u64,
    pub(crate) publication_reserve_exhaustions: u64,
    pub(crate) reusable_eligible_pages_left: u64,
    pub(crate) metadata_bootstrap_reused_pages: u64,
    pub(crate) metadata_bootstrap_extended_pages: u64,
    pub(crate) metadata_bootstrap_unused_pages: u64,
}

impl PageAllocator {
    pub(crate) fn new(page_count: u64, txn_gen: u64, free: Vec<FreePageRun>) -> Self {
        let initial_free = coalesce_free_runs(free);
        let initial_free_by_start = initial_free
            .iter()
            .copied()
            .map(|run| (run.start, run))
            .collect();
        let free = initial_free
            .iter()
            .copied()
            .into_iter()
            .map(|r| (r.start, (r.len, r.freed_gen)))
            .collect();
        Self {
            end: page_count,
            start_end: page_count,
            txn_gen,
            reuse_current_free: false,
            reusable_runs: None,
            publication_eligible_runs: Vec::new(),
            captured_free_authority: None,
            publication_reserve: BTreeMap::new(),
            publication_reserved_pages: 0,
            publication_reserve_active: false,
            metadata_bootstrap_reserve: BTreeMap::new(),
            metadata_bootstrap_owning_generation: 0,
            allocated_runs: Vec::new(),
            active_allocated_pages: BTreeSet::new(),
            reuse_before: None,
            initial_free,
            initial_free_by_start,
            free,
            txn_freed: BTreeMap::new(),
            deferred_freed: BTreeMap::new(),
            dirty_free_ranges: BTreeMap::new(),
            suppress_free_map_tracking: false,
            #[cfg(any(test, feature = "test-hooks"))]
            free_origins: BTreeMap::new(),
            #[cfg(any(test, feature = "test-hooks"))]
            transaction_stats: PageAllocatorTransactionStats::default(),
        }
    }

    pub(crate) fn new_with_current_free_reusable(
        page_count: u64,
        txn_gen: u64,
        free: Vec<FreePageRun>,
    ) -> Self {
        let mut allocator = Self::new(page_count, txn_gen, free);
        allocator.reuse_current_free = true;
        allocator.reusable_runs = Some(allocator.initial_free.clone());
        allocator.publication_eligible_runs = allocator.initial_free.clone();
        allocator
    }

    pub(crate) fn new_with_reusable_runs(
        page_count: u64,
        txn_gen: u64,
        current_free: Vec<FreePageRun>,
        reusable_runs: Vec<FreePageRun>,
    ) -> Self {
        Self::new_with_reusable_authorities(
            page_count,
            txn_gen,
            current_free,
            reusable_runs.clone(),
            reusable_runs,
        )
    }

    pub(crate) fn new_with_reusable_authorities(
        page_count: u64,
        txn_gen: u64,
        current_free: Vec<FreePageRun>,
        ordinary_reusable_runs: Vec<FreePageRun>,
        publication_eligible_runs: Vec<FreePageRun>,
    ) -> Self {
        let mut allocator = Self::new(page_count, txn_gen, current_free);
        allocator.reuse_current_free = true;
        allocator.reusable_runs = Some(ordinary_reusable_runs);
        allocator.publication_eligible_runs =
            coalesce_adjacent_same_generation(publication_eligible_runs);
        allocator
    }

    pub(crate) fn install_captured_free_authority(
        &mut self,
        authority: CapturedFreeAllocationAuthority,
    ) -> Result<()> {
        if authority.consumed_through > authority.page_count {
            return Err(corrupt("captured-free allocation cursor is out of range"));
        }
        let mut prior_cursor = authority.consumed_through;
        for candidate in &authority.runs {
            if candidate.run.len == 0
                || candidate.cursor_start < prior_cursor
                || candidate.cursor_end != candidate.cursor_start.saturating_add(candidate.run.len)
                || candidate.cursor_end > authority.page_count
                || candidate.run.start.saturating_add(candidate.run.len) > self.end
            {
                return Err(corrupt("captured-free allocation authority is invalid"));
            }
            prior_cursor = candidate.cursor_end;
        }
        self.captured_free_authority = Some(authority);
        Ok(())
    }

    pub(crate) fn captured_free_consumed_through(&self) -> Option<u64> {
        self.captured_free_authority
            .as_ref()
            .map(|authority| authority.consumed_through)
    }

    pub(crate) fn captured_free_page_count(&self) -> Option<u64> {
        self.captured_free_authority
            .as_ref()
            .map(|authority| authority.page_count)
    }

    #[cfg(test)]
    pub(crate) fn new_with_reusable_runs_and_publication_reserve(
        page_count: u64,
        txn_gen: u64,
        current_free: Vec<FreePageRun>,
        reusable_runs: Vec<FreePageRun>,
        publication_reserve: Vec<FreePageRun>,
    ) -> Self {
        let mut allocator = Self::new_with_reusable_authorities(
            page_count,
            txn_gen,
            current_free,
            reusable_runs,
            publication_reserve.clone(),
        );
        if allocator
            .install_publication_reserve(&publication_reserve)
            .is_err()
        {
            for reserved in publication_reserve {
                allocator.reserve_publication_run(reserved);
            }
        }
        allocator
    }

    pub(crate) fn new_reusing_before(
        page_count: u64,
        txn_gen: u64,
        free: Vec<FreePageRun>,
        before: u64,
    ) -> Self {
        let mut allocator = Self::new(page_count, txn_gen, free);
        allocator.reuse_before = Some(before);
        allocator
    }

    pub(crate) fn install_metadata_bootstrap_reserve(
        &mut self,
        reserve: &MetadataBootstrapReserve,
    ) -> Result<()> {
        if reserve.capacity == 0 && reserve.extents.is_empty() {
            self.metadata_bootstrap_reserve.clear();
            self.metadata_bootstrap_owning_generation = reserve.owning_generation;
            return Ok(());
        }
        reserve
            .validate(self.end)
            .map_err(|_| corrupt("metadata bootstrap reserve descriptor is invalid"))?;
        if reserve.capacity != METADATA_BOOTSTRAP_CAPACITY_PAGES {
            return Err(corrupt("metadata bootstrap reserve capacity mismatch"));
        }
        let mut next = BTreeMap::new();
        for extent in &reserve.extents {
            if self
                .active_allocated_pages
                .range(extent.start..extent.start.saturating_add(extent.len))
                .next()
                .is_some()
                || self
                    .free
                    .iter()
                    .any(|(start, (len, _))| ranges_overlap(*start, *len, extent.start, extent.len))
            {
                return Err(corrupt(
                    "metadata bootstrap reserve overlaps allocated or free state",
                ));
            }
            next.insert(extent.start, extent.len);
        }
        self.metadata_bootstrap_reserve = next;
        self.metadata_bootstrap_owning_generation = reserve.owning_generation;
        Ok(())
    }

    pub(crate) fn metadata_bootstrap_page_count(&self) -> u64 {
        self.metadata_bootstrap_reserve.values().copied().sum()
    }

    pub(crate) fn ensure_metadata_bootstrap_capacity(&mut self) -> Result<()> {
        let current = self.metadata_bootstrap_page_count();
        if current > METADATA_BOOTSTRAP_REFILL_THRESHOLD_PAGES {
            return Ok(());
        }
        let mut refill = METADATA_BOOTSTRAP_TARGET_PAGES.saturating_sub(current);
        if refill == 0 {
            return Ok(());
        }
        let eligible = self.publication_eligible_runs.clone();
        for allowed in eligible {
            if refill == 0
                || self.metadata_bootstrap_reserve.len()
                    >= METADATA_BOOTSTRAP_MAX_EXTENTS.saturating_sub(1)
            {
                break;
            }
            let allowed_end = allowed.start.saturating_add(allowed.len);
            let candidates =
                self.free
                    .range(..allowed_end)
                    .filter_map(|(&start, &(len, generation))| {
                        let end = start.saturating_add(len);
                        let selected_start = start.max(allowed.start);
                        let selected_end = end.min(allowed_end);
                        (selected_start < selected_end && generation == allowed.freed_gen)
                            .then_some((start, len, generation, selected_start, selected_end))
                    })
                    .collect::<Vec<_>>();
            for (start, len, generation, selected_start, selected_end) in candidates {
                if refill == 0
                    || self.metadata_bootstrap_reserve.len()
                        >= METADATA_BOOTSTRAP_MAX_EXTENTS.saturating_sub(1)
                {
                    break;
                }
                let take = refill.min(selected_end.saturating_sub(selected_start));
                self.reserve_metadata_bootstrap_run(start, len, generation, selected_start, take)?;
                refill -= take;
            }
        }
        if refill == 0 {
            return Ok(());
        }
        let start = self.end;
        self.end = self
            .end
            .checked_add(refill)
            .ok_or_else(|| corrupt("metadata bootstrap reserve page bound overflow"))?;
        insert_bootstrap_extent(&mut self.metadata_bootstrap_reserve, start, refill)?;
        #[cfg(any(test, feature = "test-hooks"))]
        {
            self.transaction_stats.metadata_bootstrap_extended_pages = self
                .transaction_stats
                .metadata_bootstrap_extended_pages
                .saturating_add(refill);
        }
        Ok(())
    }

    fn reserve_metadata_bootstrap_run(
        &mut self,
        free_start: u64,
        free_len: u64,
        generation: u64,
        selected_start: u64,
        selected_len: u64,
    ) -> Result<()> {
        let free_end = free_start.saturating_add(free_len);
        let selected_end = selected_start.saturating_add(selected_len);
        if selected_len == 0
            || selected_start < free_start
            || selected_end > free_end
            || self
                .active_allocated_pages
                .range(selected_start..selected_end)
                .next()
                .is_some()
        {
            return Err(corrupt("metadata bootstrap refill selection is invalid"));
        }
        let mut next_reserve = self.metadata_bootstrap_reserve.clone();
        insert_bootstrap_extent(&mut next_reserve, selected_start, selected_len)?;
        self.free.remove(&free_start);
        self.note_committed_extent_delete(free_start);
        if free_start < selected_start {
            let prefix = FreePageRun {
                start: free_start,
                len: selected_start - free_start,
                freed_gen: generation,
            };
            self.free
                .insert(prefix.start, (prefix.len, prefix.freed_gen));
            self.note_extent_upsert(prefix);
        }
        if selected_end < free_end {
            let suffix = FreePageRun {
                start: selected_end,
                len: free_end - selected_end,
                freed_gen: generation,
            };
            self.free
                .insert(suffix.start, (suffix.len, suffix.freed_gen));
            self.note_extent_upsert(suffix);
        }
        self.metadata_bootstrap_reserve = next_reserve;
        Ok(())
    }

    pub(crate) fn alloc_metadata_bootstrap_pages(&mut self, pages: u64) -> Result<Vec<PageId>> {
        if pages > FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES {
            return Err(LoomError::new(
                Code::ResourceExhausted,
                "loom-store: free-map publication exceeds metadata bootstrap capacity",
            ));
        }
        let current = self.metadata_bootstrap_page_count();
        if current < pages {
            let shortfall = pages - current;
            let start = self.end;
            self.end = self
                .end
                .checked_add(shortfall)
                .ok_or_else(|| corrupt("metadata bootstrap reserve page bound overflow"))?;
            insert_bootstrap_extent(&mut self.metadata_bootstrap_reserve, start, shortfall)?;
            #[cfg(any(test, feature = "test-hooks"))]
            {
                self.transaction_stats.metadata_bootstrap_extended_pages = self
                    .transaction_stats
                    .metadata_bootstrap_extended_pages
                    .saturating_add(shortfall);
            }
        }
        let mut remaining = pages;
        let mut allocated = Vec::with_capacity(pages as usize);
        while remaining > 0 {
            let Some((&start, &len)) = self.metadata_bootstrap_reserve.iter().next() else {
                return Err(corrupt("metadata bootstrap reserve accounting mismatch"));
            };
            self.metadata_bootstrap_reserve.remove(&start);
            let take = len.min(remaining);
            allocated.extend((start..start + take).map(PageId));
            self.allocated_runs.push((start, take));
            self.active_allocated_pages.extend(start..start + take);
            if take < len {
                self.metadata_bootstrap_reserve
                    .insert(start + take, len - take);
            }
            remaining -= take;
        }
        #[cfg(any(test, feature = "test-hooks"))]
        {
            self.transaction_stats.metadata_bootstrap_reused_pages = self
                .transaction_stats
                .metadata_bootstrap_reused_pages
                .saturating_add(current.min(pages));
        }
        Ok(allocated)
    }

    pub(crate) fn metadata_bootstrap_descriptor(
        &self,
        owning_generation: u64,
    ) -> MetadataBootstrapReserve {
        MetadataBootstrapReserve {
            owning_generation,
            capacity: METADATA_BOOTSTRAP_CAPACITY_PAGES,
            extents: self
                .metadata_bootstrap_reserve
                .iter()
                .map(|(start, len)| MetadataBootstrapExtent {
                    start: *start,
                    len: *len,
                })
                .collect(),
        }
    }

    /// Reserve a run of `n` pages and return its first page: reuse a run freed earlier in this
    /// transaction that was extended within it, then a prior-generation run aged past the window
    /// (splitting any remainder back), and extend the array otherwise.
    pub(crate) fn alloc(&mut self, n: u64) -> PageId {
        let page = if self.publication_reserve_active {
            if let Some(start) = self.take_publication_reserve(n) {
                #[cfg(any(test, feature = "test-hooks"))]
                {
                    self.transaction_stats.publication_reused_pages = self
                        .transaction_stats
                        .publication_reused_pages
                        .saturating_add(n);
                }
                PageId(start)
            } else {
                #[cfg(any(test, feature = "test-hooks"))]
                {
                    self.transaction_stats.publication_reserve_exhaustions = self
                        .transaction_stats
                        .publication_reserve_exhaustions
                        .saturating_add(1);
                    self.transaction_stats.extended_pages =
                        self.transaction_stats.extended_pages.saturating_add(n);
                }
                self.extend(n)
            }
        } else if let Some(start) = self.take_txn_freed(n) {
            #[cfg(any(test, feature = "test-hooks"))]
            {
                self.transaction_stats.transaction_reused_pages = self
                    .transaction_stats
                    .transaction_reused_pages
                    .saturating_add(n);
            }
            PageId(start)
        } else if let Some(start) = self.take_aged(n) {
            #[cfg(any(test, feature = "test-hooks"))]
            {
                self.transaction_stats.ordinary_reused_pages = self
                    .transaction_stats
                    .ordinary_reused_pages
                    .saturating_add(n);
            }
            PageId(start)
        } else if let Some(start) = self.take_captured_free(n) {
            #[cfg(any(test, feature = "test-hooks"))]
            {
                self.transaction_stats.ordinary_reused_pages = self
                    .transaction_stats
                    .ordinary_reused_pages
                    .saturating_add(n);
            }
            PageId(start)
        } else {
            #[cfg(any(test, feature = "test-hooks"))]
            {
                self.transaction_stats.extended_pages =
                    self.transaction_stats.extended_pages.saturating_add(n);
            }
            self.extend(n)
        };
        self.allocated_runs.push((page.0, n));
        self.active_allocated_pages
            .extend(page.0..page.0.saturating_add(n));
        page
    }

    pub(crate) fn activate_publication_reserve(&mut self) {
        self.publication_reserve_active = true;
    }

    fn take_publication_reserve(&mut self, n: u64) -> Option<u64> {
        let start = self
            .publication_reserve
            .iter()
            .find_map(|(start, (len, _))| (*len >= n).then_some(*start))?;
        let (len, freed_gen) = self.publication_reserve.remove(&start).unwrap_or((0, 0));
        self.note_committed_extent_delete(start);
        if len > n {
            let remainder = FreePageRun {
                start: start + n,
                len: len - n,
                freed_gen,
            };
            self.publication_reserve
                .insert(remainder.start, (remainder.len, remainder.freed_gen));
            self.note_extent_upsert(remainder);
        }
        Some(start)
    }

    pub(crate) fn install_publication_reserve(&mut self, selected: &[FreePageRun]) -> Result<()> {
        if self.publication_reserve_active {
            return Err(LoomError::invalid("publication reserve is already active"));
        }
        let normalized = normalize_publication_reserve_selection(selected)?;
        for run in &normalized {
            let end = run.start + run.len;
            if end > self.end {
                return Err(LoomError::invalid(
                    "publication reserve run exceeds the allocator page bound",
                ));
            }
            if self
                .active_allocated_pages
                .range(run.start..end)
                .next()
                .is_some()
            {
                return Err(LoomError::invalid(
                    "publication reserve run is already allocated in this transaction",
                ));
            }
            let Some((&free_start, &(free_len, freed_gen))) =
                self.free.range(..=run.start).next_back()
            else {
                return Err(LoomError::invalid(
                    "publication reserve run is not currently free",
                ));
            };
            let free_end = free_start.saturating_add(free_len);
            if run.start < free_start || end > free_end || run.freed_gen != freed_gen {
                return Err(LoomError::invalid(
                    "publication reserve run is not contained in one matching free extent",
                ));
            }
            let within_reuse_bound = self
                .reuse_before
                .map(|before| end <= before)
                .unwrap_or(true);
            let reusable = self.publication_eligible_runs.iter().any(|allowed| {
                let allowed_end = allowed.start.saturating_add(allowed.len);
                run.start >= allowed.start
                    && end <= allowed_end
                    && run.freed_gen == allowed.freed_gen
            });
            if !within_reuse_bound || !reusable {
                return Err(LoomError::invalid(
                    "publication reserve run is not eligible for reuse",
                ));
            }
        }
        for run in normalized {
            self.reserve_publication_run(run);
        }
        Ok(())
    }

    fn reserve_publication_run(&mut self, reserved: FreePageRun) {
        let reserved_end = reserved.start.saturating_add(reserved.len);
        let overlapping = self
            .free
            .range(..reserved_end)
            .filter_map(|(start, (len, generation))| {
                let end = start.saturating_add(*len);
                (end > reserved.start).then_some((*start, *len, *generation))
            })
            .collect::<Vec<_>>();
        for (start, len, generation) in overlapping {
            let end = start.saturating_add(len);
            let remove_start = start.max(reserved.start);
            let remove_end = end.min(reserved_end);
            if remove_start >= remove_end {
                continue;
            }
            self.free.remove(&start);
            self.note_committed_extent_delete(start);
            if start < remove_start {
                let prefix = FreePageRun {
                    start,
                    len: remove_start - start,
                    freed_gen: generation,
                };
                self.free
                    .insert(prefix.start, (prefix.len, prefix.freed_gen));
                self.note_extent_upsert(prefix);
            }
            if remove_end < end {
                let suffix = FreePageRun {
                    start: remove_end,
                    len: end - remove_end,
                    freed_gen: generation,
                };
                self.free
                    .insert(suffix.start, (suffix.len, suffix.freed_gen));
                self.note_extent_upsert(suffix);
            }
            self.publication_reserve
                .insert(remove_start, (remove_end - remove_start, generation));
            self.note_extent_upsert(FreePageRun {
                start: remove_start,
                len: remove_end - remove_start,
                freed_gen: generation,
            });
            self.publication_reserved_pages = self
                .publication_reserved_pages
                .saturating_add(remove_end - remove_start);
        }
    }

    /// Take a run of `n` pages from those freed after allocation in this transaction. A crash before
    /// commit reverts to the prior generation, which never referenced those pages, so overwriting
    /// them now is safe.
    fn take_txn_freed(&mut self, n: u64) -> Option<u64> {
        let (run_start, allocation_start) = self.txn_freed.iter().find_map(|(start, len)| {
            if *start >= self.start_end && *len >= n {
                return Some((*start, *start));
            }
            let run_end = start.saturating_add(*len);
            self.allocated_runs
                .iter()
                .find_map(|(allocated, allocated_len)| {
                    let allocation_start = (*start).max(*allocated);
                    let allocation_end = run_end.min(allocated.saturating_add(*allocated_len));
                    (allocation_start.saturating_add(n) <= allocation_end)
                        .then_some((*start, allocation_start))
                })
        })?;
        let len = self.txn_freed.remove(&run_start).unwrap_or(0);
        self.note_committed_extent_delete(run_start);
        if allocation_start > run_start {
            let prefix = FreePageRun {
                start: run_start,
                len: allocation_start - run_start,
                freed_gen: self.txn_gen,
            };
            self.txn_freed.insert(prefix.start, prefix.len);
            self.note_extent_upsert(prefix);
        }
        let run_end = run_start.saturating_add(len);
        let allocation_end = allocation_start.saturating_add(n);
        if allocation_end < run_end {
            self.txn_freed
                .insert(allocation_end, run_end - allocation_end);
            self.note_extent_upsert(FreePageRun {
                start: allocation_end,
                len: run_end - allocation_end,
                freed_gen: self.txn_gen,
            });
        }
        Some(allocation_start)
    }

    /// Take a run of `n` pages from a prior generation that is now outside the recoverable window.
    fn take_aged(&mut self, n: u64) -> Option<u64> {
        if let Some(reusable_runs) = &self.reusable_runs {
            if self.reuse_before == Some(0) {
                return None;
            }
            let (run_start, allocation_start) = reusable_runs.iter().find_map(|allowed| {
                let allowed_end = allowed.start.saturating_add(allowed.len);
                self.free.iter().find_map(|(start, (len, generation))| {
                    let end = start.saturating_add(*len);
                    let allocation_start = (*start).max(allowed.start);
                    let within_bound = self
                        .reuse_before
                        .map(|before| allocation_start.saturating_add(n) <= before)
                        .unwrap_or(true);
                    (*generation == allowed.freed_gen
                        && within_bound
                        && allocation_start.saturating_add(n) <= end
                        && allocation_start.saturating_add(n) <= allowed_end)
                        .then_some((*start, allocation_start))
                })
            })?;
            let (len, generation) = self.free.remove(&run_start).unwrap_or((0, 0));
            self.note_committed_extent_delete(run_start);
            if allocation_start > run_start {
                let prefix = FreePageRun {
                    start: run_start,
                    len: allocation_start - run_start,
                    freed_gen: generation,
                };
                self.free
                    .insert(prefix.start, (prefix.len, prefix.freed_gen));
                self.note_extent_upsert(prefix);
            }
            let run_end = run_start.saturating_add(len);
            let allocation_end = allocation_start.saturating_add(n);
            if allocation_end < run_end {
                let suffix = FreePageRun {
                    start: allocation_end,
                    len: run_end - allocation_end,
                    freed_gen: generation,
                };
                self.free
                    .insert(suffix.start, (suffix.len, suffix.freed_gen));
                self.note_extent_upsert(suffix);
            }
            return Some(allocation_start);
        }
        let start = self.free.iter().find_map(|(s, v)| {
            let end = s.saturating_add(n);
            let within_bound = self
                .reuse_before
                .map(|before| end <= before)
                .unwrap_or(true);
            (v.0 >= n
                && within_bound
                && (self.reuse_current_free || v.1 + REUSE_SAFE_WINDOW <= self.txn_gen))
                .then_some(*s)
        })?;
        let (len, g) = self.free.remove(&start).unwrap_or((0, 0));
        self.note_committed_extent_delete(start);
        if len > n {
            self.free.insert(start + n, (len - n, g));
            self.note_extent_upsert(FreePageRun {
                start: start + n,
                len: len - n,
                freed_gen: g,
            });
        }
        Some(start)
    }

    fn take_captured_free(&mut self, n: u64) -> Option<u64> {
        let authority = self.captured_free_authority.as_ref()?.clone();
        let mut cursor = authority.consumed_through;
        for candidate in &authority.runs {
            if candidate.cursor_end <= cursor {
                continue;
            }
            cursor = cursor.max(candidate.cursor_start);
            let candidate_offset = cursor.saturating_sub(candidate.cursor_start);
            let candidate_start = candidate.run.start.saturating_add(candidate_offset);
            let candidate_end = candidate.run.start.saturating_add(candidate.run.len);
            let mut page = candidate_start;
            while page < candidate_end {
                let free_index = self
                    .free
                    .range(..=page)
                    .next_back()
                    .map(|(&start, &(len, generation))| (start, len, generation));
                let Some((free_start, free_len, generation)) = free_index else {
                    cursor = candidate.cursor_end;
                    break;
                };
                let free_end = free_start.saturating_add(free_len);
                if page < free_start || page >= free_end || generation != candidate.run.freed_gen {
                    cursor = candidate.cursor_end;
                    break;
                }
                let available_end = free_end.min(candidate_end);
                let available = available_end.saturating_sub(page);
                if available < n {
                    cursor = cursor.saturating_add(available);
                    page = available_end;
                    continue;
                }
                let (len, freed_gen) = self.free.remove(&free_start).unwrap_or((0, 0));
                self.note_committed_extent_delete(free_start);
                if free_start < page {
                    let prefix = FreePageRun {
                        start: free_start,
                        len: page - free_start,
                        freed_gen,
                    };
                    self.free
                        .insert(prefix.start, (prefix.len, prefix.freed_gen));
                    self.note_extent_upsert(prefix);
                }
                let allocation_end = page.saturating_add(n);
                let original_end = free_start.saturating_add(len);
                if allocation_end < original_end {
                    let suffix = FreePageRun {
                        start: allocation_end,
                        len: original_end - allocation_end,
                        freed_gen,
                    };
                    self.free
                        .insert(suffix.start, (suffix.len, suffix.freed_gen));
                    self.note_extent_upsert(suffix);
                }
                if let Some(current) = self.captured_free_authority.as_mut() {
                    current.consumed_through = cursor.saturating_add(n);
                }
                return Some(page);
            }
            cursor = cursor.max(candidate.cursor_end);
        }
        if let Some(current) = self.captured_free_authority.as_mut() {
            current.consumed_through = authority.page_count;
        }
        None
    }

    pub(crate) fn select_captured_publication_reserve(
        &mut self,
        max_pages: u64,
    ) -> Result<Vec<FreePageRun>> {
        if max_pages == 0 {
            return Ok(Vec::new());
        }
        let Some(authority) = self.captured_free_authority.as_mut() else {
            return Ok(Vec::new());
        };
        let mut selected = Vec::new();
        let mut remaining = max_pages;
        let mut cursor = authority.consumed_through;
        for candidate in &authority.runs {
            if candidate.cursor_end <= cursor {
                continue;
            }
            cursor = cursor.max(candidate.cursor_start);
            let offset = cursor.saturating_sub(candidate.cursor_start);
            let mut page = candidate.run.start.saturating_add(offset);
            let candidate_end = candidate.run.start.saturating_add(candidate.run.len);
            while page < candidate_end && remaining > 0 {
                let current = self
                    .free
                    .range(..=page)
                    .next_back()
                    .map(|(&start, &(len, generation))| (start, len, generation));
                let Some((free_start, free_len, generation)) = current else {
                    cursor = candidate.cursor_end;
                    break;
                };
                let free_end = free_start.saturating_add(free_len);
                if page < free_start || page >= free_end || generation != candidate.run.freed_gen {
                    cursor = candidate.cursor_end;
                    break;
                }
                let take = free_end
                    .min(candidate_end)
                    .saturating_sub(page)
                    .min(remaining);
                if take == 0 {
                    break;
                }
                selected.push(FreePageRun {
                    start: page,
                    len: take,
                    freed_gen: generation,
                });
                page = page.saturating_add(take);
                cursor = cursor.saturating_add(take);
                remaining -= take;
            }
            if remaining == 0 {
                break;
            }
            cursor = cursor.max(candidate.cursor_end);
        }
        if remaining > 0 {
            cursor = authority.page_count;
        }
        authority.consumed_through = cursor;
        let normalized = normalize_publication_reserve_selection(&selected)?;
        self.install_publication_reserve(&normalized)?;
        Ok(normalized)
    }

    /// Reserve `n` pages by extending the page array, never reusing a free run.
    pub(crate) fn extend(&mut self, n: u64) -> PageId {
        let start = self.end;
        self.end += n;
        PageId(start)
    }

    /// Record that the `n`-page run starting at `start` is freed by this transaction. It becomes
    /// reusable immediately if extended within this transaction, and otherwise joins the free-page map
    /// on commit, tagged with this generation.
    #[track_caller]
    pub(crate) fn free(&mut self, start: PageId, n: u64) -> Result<()> {
        let end = self.validate_free_span(start, n)?;
        if self.metadata_bootstrap_reserve_overlaps(start, n) {
            return Err(corrupt(
                "metadata bootstrap reserve cannot enter the canonical free map",
            ));
        }
        for page in start.0..end {
            self.active_allocated_pages.remove(&page);
        }
        if self.suppress_free_map_tracking {
            return Ok(());
        }
        #[cfg(any(test, feature = "test-hooks"))]
        for page in start.0..end {
            self.free_origins
                .insert(page, std::panic::Location::caller());
        }
        let changes = insert_txn_run(&mut self.txn_freed, start.0, end)?;
        self.note_new_extent_changes(changes);
        Ok(())
    }

    pub(crate) fn defer_free(&mut self, start: PageId, n: u64) -> Result<()> {
        let end = self.validate_free_span(start, n)?;
        if self.metadata_bootstrap_reserve_overlaps(start, n) {
            return Err(corrupt(
                "metadata bootstrap reserve cannot enter deferred reclamation",
            ));
        }
        for page in start.0..end {
            self.active_allocated_pages.remove(&page);
        }
        if self.suppress_free_map_tracking {
            return Ok(());
        }
        let changes = insert_txn_run(&mut self.deferred_freed, start.0, end)?;
        self.note_new_extent_changes(changes);
        Ok(())
    }

    fn validate_free_span(&self, start: PageId, len: u64) -> Result<u64> {
        if len == 0 {
            return Err(corrupt("free-page span must be nonempty"));
        }
        let end = start
            .0
            .checked_add(len)
            .ok_or_else(|| corrupt("free-page span overflows"))?;
        if end > self.end {
            return Err(corrupt("free-page span exceeds allocator page bound"));
        }
        Ok(end)
    }

    fn metadata_bootstrap_reserve_overlaps(&self, start: PageId, n: u64) -> bool {
        self.metadata_bootstrap_reserve
            .iter()
            .any(|(reserve_start, reserve_len)| {
                ranges_overlap(start.0, n, *reserve_start, *reserve_len)
            })
    }

    /// Total pages the array spans: every page handed out so far lies below this.
    pub(crate) fn page_count(&self) -> u64 {
        self.end
    }

    pub(crate) fn allocated_in_transaction(&self, page: u64) -> bool {
        self.active_allocated_pages.contains(&page)
    }

    pub(crate) fn initial_free_runs(&self) -> Vec<FreePageRun> {
        self.initial_free.clone()
    }

    pub(crate) fn take_free_map_extent_updates(&mut self) -> Vec<FreeMapExtentUpdate> {
        let updates = self.pending_free_map_extent_updates();
        self.dirty_free_ranges.clear();
        #[cfg(any(test, feature = "test-hooks"))]
        {
            self.transaction_stats.free_map_updates = updates.len() as u64;
        }
        updates
    }

    pub(crate) fn pending_free_map_extent_updates(&self) -> Vec<FreeMapExtentUpdate> {
        self.dirty_free_map_extent_updates()
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn transaction_stats(&self) -> PageAllocatorTransactionStats {
        PageAllocatorTransactionStats {
            publication_reserved_pages: self.publication_reserved_pages,
            publication_unused_pages: self.publication_reserve.values().map(|(len, _)| *len).sum(),
            reusable_eligible_pages_left: self.reusable_eligible_pages_left(),
            metadata_bootstrap_unused_pages: self.metadata_bootstrap_page_count(),
            ..self.transaction_stats
        }
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn reusable_eligible_pages_left(&self) -> u64 {
        let ordinary = self.reusable_runs.as_ref().map_or(0, |allowed_runs| {
            allowed_runs
                .iter()
                .map(|allowed| {
                    let allowed_end = allowed.start.saturating_add(allowed.len);
                    self.free
                        .iter()
                        .map(|(start, (len, _))| {
                            let end = start.saturating_add(*len);
                            end.min(allowed_end)
                                .saturating_sub((*start).max(allowed.start))
                        })
                        .sum::<u64>()
                })
                .sum::<u64>()
        });
        ordinary.saturating_add(
            self.publication_reserve
                .values()
                .map(|(len, _)| *len)
                .sum::<u64>(),
        )
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn note_free_map_publication_stats(
        &mut self,
        extent_deletes: u64,
        extent_upserts: u64,
        unique_btree_nodes_touched: u64,
        split_pages: u64,
    ) {
        self.transaction_stats.free_map_extent_deletes = extent_deletes;
        self.transaction_stats.free_map_extent_upserts = extent_upserts;
        self.transaction_stats.free_map_unique_btree_nodes_touched = unique_btree_nodes_touched;
        self.transaction_stats.free_map_split_pages = split_pages;
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn note_fixed_metadata_pages(&mut self, pages: u64) {
        self.transaction_stats.fixed_metadata_pages = self
            .transaction_stats
            .fixed_metadata_pages
            .saturating_add(pages);
    }

    pub(crate) fn pending_free_map_extent_update_count(&self) -> usize {
        self.dirty_free_ranges.len()
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn free_page_debug(&self, page: u64) -> String {
        let initial = self
            .initial_free
            .iter()
            .any(|run| page >= run.start && page < run.start.saturating_add(run.len));
        let allocated = self
            .allocated_runs
            .iter()
            .any(|(start, len)| page >= *start && page < start.saturating_add(*len));
        let origin = self
            .free_origins
            .get(&page)
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_else(|| "none".to_string());
        format!("initial_free={initial} allocated={allocated} free_origin={origin}")
    }

    fn replace_free_map_tracking_suppression(&mut self, suppress: bool) -> bool {
        std::mem::replace(&mut self.suppress_free_map_tracking, suppress)
    }

    fn note_committed_extent_delete(&mut self, start: u64) {
        if let Some(initial) = self.initial_free_by_start.get(&start).copied() {
            self.mark_free_map_dirty(initial.start, initial.len);
        }
    }

    fn note_extent_upsert(&mut self, run: FreePageRun) {
        self.mark_free_map_dirty(run.start, run.len);
    }

    fn mark_free_map_dirty(&mut self, start: u64, len: u64) {
        if self.suppress_free_map_tracking || len == 0 {
            return;
        }
        let mut merged_start = start;
        let mut merged_end = start.saturating_add(len);
        if let Some((&prior_start, &prior_end)) = self.dirty_free_ranges.range(..=start).next_back()
            && prior_end >= start
        {
            merged_start = prior_start;
            merged_end = merged_end.max(prior_end);
            self.dirty_free_ranges.remove(&prior_start);
        }
        while let Some((&next_start, &next_end)) =
            self.dirty_free_ranges.range(merged_start..).next()
        {
            if next_start > merged_end {
                break;
            }
            merged_end = merged_end.max(next_end);
            self.dirty_free_ranges.remove(&next_start);
        }
        self.dirty_free_ranges.insert(merged_start, merged_end);
    }

    fn dirty_free_map_extent_updates(&self) -> Vec<FreeMapExtentUpdate> {
        let mut updates = Vec::new();
        for (&start, &end) in &self.dirty_free_ranges {
            let initial_runs = runs_intersecting(&self.initial_free_by_start, start, end);
            let final_runs = self.final_free_runs_intersecting(start, end);
            if initial_runs == final_runs {
                continue;
            }
            for initial in initial_runs {
                updates.push(FreeMapExtentUpdate::Delete(initial));
            }
            for final_run in final_runs {
                updates.push(FreeMapExtentUpdate::Upsert(final_run));
            }
        }
        updates
    }

    fn final_free_runs_intersecting(&self, start: u64, end: u64) -> Vec<FreePageRun> {
        let mut runs = Vec::new();
        collect_prior_free_runs(&self.free, start, end, &mut runs);
        collect_transaction_free_runs(&self.txn_freed, self.txn_gen, start, end, &mut runs);
        collect_transaction_free_runs(&self.deferred_freed, self.txn_gen, start, end, &mut runs);
        collect_prior_free_runs(&self.publication_reserve, start, end, &mut runs);
        coalesce_free_runs(runs)
    }

    fn note_new_extent_changes(&mut self, changes: TxnRunChanges) {
        for start in changes.removed_starts {
            self.mark_free_map_dirty(start, 1);
        }
        let inserted_end = changes.inserted_start.saturating_add(changes.inserted_len);
        let committed_overlaps = self
            .free
            .range(..inserted_end)
            .filter_map(|(&start, &(len, _))| {
                (start.saturating_add(len) > changes.inserted_start).then_some(start)
            })
            .collect::<Vec<_>>();
        for start in committed_overlaps {
            self.free.remove(&start);
            self.note_committed_extent_delete(start);
        }
        self.note_extent_upsert(FreePageRun {
            start: changes.inserted_start,
            len: changes.inserted_len,
            freed_gen: self.txn_gen,
        });
    }

    /// The free run list this transaction leaves behind: still-unused prior-generation runs plus the
    /// runs it freed, tagged with its generation. Computed without consuming the allocator.
    pub(crate) fn snapshot_free(&self) -> Vec<FreePageRun> {
        let mut v: Vec<FreePageRun> = self
            .free
            .iter()
            .map(|(&start, &(len, freed_gen))| FreePageRun {
                start,
                len,
                freed_gen,
            })
            .collect();
        for (&start, &len) in &self.txn_freed {
            v.push(FreePageRun {
                start,
                len,
                freed_gen: self.txn_gen,
            });
        }
        for (&start, &len) in &self.deferred_freed {
            v.push(FreePageRun {
                start,
                len,
                freed_gen: self.txn_gen,
            });
        }
        for (&start, &(len, freed_gen)) in &self.publication_reserve {
            v.push(FreePageRun {
                start,
                len,
                freed_gen,
            });
        }
        coalesce_free_runs(v)
    }
}

fn runs_intersecting(runs: &BTreeMap<u64, FreePageRun>, start: u64, end: u64) -> Vec<FreePageRun> {
    let mut selected = Vec::new();
    if let Some((_, run)) = runs.range(..start).next_back()
        && run.start.saturating_add(run.len) > start
    {
        selected.push(*run);
    }
    selected.extend(
        runs.range(start..end)
            .map(|(_, run)| *run)
            .filter(|run| run.start < end && run.start.saturating_add(run.len) > start),
    );
    selected
}

fn collect_prior_free_runs(
    runs: &BTreeMap<u64, (u64, u64)>,
    start: u64,
    end: u64,
    selected: &mut Vec<FreePageRun>,
) {
    if let Some((&run_start, &(len, freed_gen))) = runs.range(..start).next_back()
        && run_start.saturating_add(len) > start
    {
        selected.push(FreePageRun {
            start: run_start,
            len,
            freed_gen,
        });
    }
    selected.extend(
        runs.range(start..end)
            .filter_map(|(&run_start, &(len, freed_gen))| {
                (run_start < end && run_start.saturating_add(len) > start).then_some(FreePageRun {
                    start: run_start,
                    len,
                    freed_gen,
                })
            }),
    );
}

fn collect_transaction_free_runs(
    runs: &BTreeMap<u64, u64>,
    freed_gen: u64,
    start: u64,
    end: u64,
    selected: &mut Vec<FreePageRun>,
) {
    if let Some((&run_start, &len)) = runs.range(..start).next_back()
        && run_start.saturating_add(len) > start
    {
        selected.push(FreePageRun {
            start: run_start,
            len,
            freed_gen,
        });
    }
    selected.extend(runs.range(start..end).filter_map(|(&run_start, &len)| {
        (run_start < end && run_start.saturating_add(len) > start).then_some(FreePageRun {
            start: run_start,
            len,
            freed_gen,
        })
    }));
}

fn ranges_overlap(left_start: u64, left_len: u64, right_start: u64, right_len: u64) -> bool {
    left_start < right_start.saturating_add(right_len)
        && right_start < left_start.saturating_add(left_len)
}

fn insert_bootstrap_extent(extents: &mut BTreeMap<u64, u64>, start: u64, len: u64) -> Result<()> {
    if len == 0 {
        return Err(LoomError::invalid(
            "metadata bootstrap reserve extent must not be empty",
        ));
    }
    let mut merged_start = start;
    let mut merged_end = start
        .checked_add(len)
        .ok_or_else(|| corrupt("metadata bootstrap reserve extent overflow"))?;
    if let Some((&prior_start, &prior_len)) = extents.range(..=start).next_back() {
        let prior_end = prior_start.saturating_add(prior_len);
        if prior_end >= start {
            merged_start = prior_start;
            merged_end = merged_end.max(prior_end);
            extents.remove(&prior_start);
        }
    }
    while let Some((&next_start, &next_len)) = extents.range(merged_start..).next() {
        if next_start > merged_end {
            break;
        }
        merged_end = merged_end.max(next_start.saturating_add(next_len));
        extents.remove(&next_start);
    }
    extents.insert(merged_start, merged_end - merged_start);
    if extents.len() > METADATA_BOOTSTRAP_MAX_EXTENTS {
        return Err(LoomError::new(
            Code::ResourceExhausted,
            "loom-store: metadata bootstrap reserve extent limit reached",
        ));
    }
    Ok(())
}

struct TxnRunChanges {
    removed_starts: Vec<u64>,
    inserted_start: u64,
    inserted_len: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FreeMapExtentUpdate {
    Delete(FreePageRun),
    Upsert(FreePageRun),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FreeMapPublicationDemand {
    pub(crate) extent_deletes: u64,
    pub(crate) extent_upserts: u64,
    pub(crate) btree_node_pages: u64,
    pub(crate) affected_existing_btree_pages: u64,
    pub(crate) split_decisions: u64,
}

impl FreeMapPublicationDemand {
    pub(crate) fn allocation_pages(self) -> u64 {
        self.btree_node_pages
    }
}

#[derive(Clone)]
pub(crate) struct PreparedFreeMapPublication {
    previous_root: Option<PageId>,
    source_page_count: u64,
    source_updates: Vec<FreeMapExtentUpdate>,
    updates: Vec<FreeMapExtentUpdate>,
    tree_delta: pagebtree::PreparedPageTreeDelta,
    demand: FreeMapPublicationDemand,
}

impl PreparedFreeMapPublication {
    pub(crate) fn demand(&self) -> FreeMapPublicationDemand {
        self.demand
    }

    #[cfg(test)]
    fn tree_allocation_calls_for_test(&self) -> u64 {
        self.tree_delta.allocation_calls()
    }
}

fn insert_txn_run(runs: &mut BTreeMap<u64, u64>, start: u64, end: u64) -> Result<TxnRunChanges> {
    let mut merged_start = start;
    let mut merged_end = end;
    let mut removed_starts = Vec::new();
    let mut successor_scan_start = start;
    if let Some((&prev_start, &prev_len)) = runs.range(..=start).next_back() {
        let prev_end = prev_start
            .checked_add(prev_len)
            .ok_or_else(|| corrupt("existing transaction free-page span overflows"))?;
        if prev_end >= start {
            merged_start = prev_start;
            merged_end = merged_end.max(prev_end);
            removed_starts.push(prev_start);
            successor_scan_start = prev_start
                .checked_add(1)
                .ok_or_else(|| corrupt("existing transaction free-page start overflows"))?;
        }
    }
    for (&next_start, &next_len) in runs.range(successor_scan_start..) {
        if next_start > merged_end {
            break;
        }
        let next_end = next_start
            .checked_add(next_len)
            .ok_or_else(|| corrupt("existing transaction free-page span overflows"))?;
        merged_end = merged_end.max(next_end);
        removed_starts.push(next_start);
    }
    for removed_start in &removed_starts {
        runs.remove(removed_start);
    }
    runs.insert(merged_start, merged_end - merged_start);
    Ok(TxnRunChanges {
        removed_starts,
        inserted_start: merged_start,
        inserted_len: merged_end - merged_start,
    })
}

fn coalesce_free_runs(mut runs: Vec<FreePageRun>) -> Vec<FreePageRun> {
    runs.sort_by_key(|run| run.start);
    let mut normalized: Vec<FreePageRun> = Vec::with_capacity(runs.len());
    for run in runs {
        if let Some(previous) = normalized.last_mut() {
            let previous_end = previous.start.saturating_add(previous.len);
            if run.start < previous_end {
                let end = previous_end.max(run.start.saturating_add(run.len));
                previous.len = end.saturating_sub(previous.start);
                previous.freed_gen = previous.freed_gen.max(run.freed_gen);
                continue;
            }
            if run.start == previous_end && run.freed_gen == previous.freed_gen {
                previous.len = previous.len.saturating_add(run.len);
                continue;
            }
        }
        normalized.push(run);
    }
    normalized
}

fn coalesce_adjacent_same_generation(mut runs: Vec<FreePageRun>) -> Vec<FreePageRun> {
    runs.sort_by_key(|run| run.start);
    let mut normalized: Vec<FreePageRun> = Vec::with_capacity(runs.len());
    for run in runs {
        if let Some(previous) = normalized.last_mut()
            && previous.start.saturating_add(previous.len) == run.start
            && previous.freed_gen == run.freed_gen
        {
            previous.len = previous.len.saturating_add(run.len);
        } else {
            normalized.push(run);
        }
    }
    normalized
}

fn normalize_publication_reserve_selection(selected: &[FreePageRun]) -> Result<Vec<FreePageRun>> {
    let mut sorted = selected.to_vec();
    sorted.sort_by_key(|run| run.start);
    let mut normalized: Vec<FreePageRun> = Vec::with_capacity(sorted.len());
    for run in sorted {
        let end = run
            .start
            .checked_add(run.len)
            .ok_or_else(|| LoomError::invalid("publication reserve run overflows"))?;
        if run.len == 0 {
            return Err(LoomError::invalid(
                "publication reserve run must be nonempty",
            ));
        }
        if let Some(previous) = normalized.last_mut() {
            let previous_end = previous.start + previous.len;
            if run.start < previous_end {
                return Err(LoomError::invalid(
                    "publication reserve runs overlap after ordering",
                ));
            }
            if run.start == previous_end && run.freed_gen == previous.freed_gen {
                previous.len = end - previous.start;
                continue;
            }
        }
        normalized.push(run);
    }
    Ok(normalized)
}

fn extent_key(start: u64) -> [u8; 32] {
    let mut key = [0u8; 32];
    key[24..].copy_from_slice(&start.to_be_bytes());
    key
}

fn extent_start_from_key(key: &[u8; 32]) -> Result<u64> {
    if key[..24].iter().any(|byte| *byte != 0) {
        return Err(corrupt("free-page extent key prefix"));
    }
    Ok(u64::from_be_bytes(
        key[24..]
            .try_into()
            .map_err(|_| corrupt("free-page extent key"))?,
    ))
}

fn decode_extent(start: u64, bytes: &[u8]) -> Result<FreePageRun> {
    let len = EXTENT_MAGIC.len() + 16 + 4;
    if bytes.len() != len || bytes.get(..EXTENT_MAGIC.len()) != Some(EXTENT_MAGIC) {
        return Err(corrupt("bad free-page extent record"));
    }
    let stored_crc = u32::from_le_bytes(
        bytes[len - 4..len]
            .try_into()
            .map_err(|_| corrupt("free-page extent crc"))?,
    );
    if crc32c(&bytes[..len - 4]) != stored_crc {
        return Err(corrupt("free-page extent crc mismatch"));
    }
    let len_start = EXTENT_MAGIC.len();
    let run_len = u64::from_le_bytes(
        bytes[len_start..len_start + 8]
            .try_into()
            .map_err(|_| corrupt("free-page extent length"))?,
    );
    let freed_gen = u64::from_le_bytes(
        bytes[len_start + 8..len_start + 16]
            .try_into()
            .map_err(|_| corrupt("free-page extent generation"))?,
    );
    Ok(FreePageRun {
        start,
        len: run_len,
        freed_gen,
    })
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct LegacyFreeMapInventory {
    pub(crate) runs: Vec<FreePageRun>,
    pub(crate) tree_pages: BTreeSet<u64>,
    pub(crate) blob_pages: BTreeSet<u64>,
}

#[cfg(test)]
static LEGACY_PROMOTION_INVENTORY: std::sync::Mutex<Option<LegacyFreeMapInventory>> =
    std::sync::Mutex::new(None);

#[cfg(test)]
pub(crate) fn take_legacy_promotion_inventory() -> Option<LegacyFreeMapInventory> {
    LEGACY_PROMOTION_INVENTORY.lock().ok()?.take()
}

#[cfg(test)]
pub(crate) fn read_legacy_recordloc_map_for_promotion(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
) -> Result<LegacyFreeMapInventory> {
    let entries = pagebtree::load_all_with_progress(file, header_len, root, page_count, |_| {})?;
    let tree_pages = pagebtree::collect_pages(file, header_len, root, page_count)?
        .into_iter()
        .map(|page| page.0)
        .collect::<BTreeSet<_>>();
    let mut runs = Vec::with_capacity(entries.len());
    let mut blob_pages = BTreeSet::new();
    for (key, loc) in entries {
        if loc.slot != 0 {
            return Err(corrupt("legacy free-page extent locator slot"));
        }
        let start = extent_start_from_key(&key)?;
        let bytes = crate::record_io::read_blob_from_loc(file, loc, page_count)?;
        let run = decode_extent(start, &bytes)?;
        let pages = crate::record_io::blob_pages(file, loc.global_page(), page_count)?;
        if pages.is_empty() {
            return Err(corrupt("legacy free-page extent owns no blob page"));
        }
        for page in pages {
            if !blob_pages.insert(page) {
                return Err(corrupt("legacy free-page extent shares a blob page"));
            }
        }
        runs.push(run);
    }
    runs.sort_by_key(|run| run.start);
    validate_runs(&runs, page_count)?;
    if tree_pages.iter().any(|page| blob_pages.contains(page)) {
        return Err(corrupt(
            "legacy free-page extent tree overlaps its blob pages",
        ));
    }
    for run in &runs {
        let end = run.start.saturating_add(run.len);
        if tree_pages
            .iter()
            .chain(blob_pages.iter())
            .any(|page| *page >= run.start && *page < end)
        {
            return Err(corrupt(
                "legacy free-page extent metadata overlaps its logical free runs",
            ));
        }
    }
    Ok(LegacyFreeMapInventory {
        runs,
        tree_pages,
        blob_pages,
    })
}

#[cfg(test)]
pub(crate) fn record_legacy_promotion_inventory(inventory: &LegacyFreeMapInventory) -> Result<()> {
    *LEGACY_PROMOTION_INVENTORY
        .lock()
        .map_err(|_| crate::poisoned())? = Some(inventory.clone());
    Ok(())
}

pub(crate) fn decode_extent_record_for_reclaim(bytes: &[u8]) -> bool {
    decode_extent(0, bytes).is_ok()
}

pub(crate) fn decode(bytes: &[u8]) -> Option<Vec<FreePageRun>> {
    if bytes.len() < 9 || bytes[0] != LEGACY_MAP_MAGIC {
        return None;
    }
    let count = u32::from_le_bytes(bytes[1..5].try_into().ok()?) as usize;
    let total = 5usize.checked_add(count.checked_mul(24)?)?.checked_add(4)?;
    if bytes.len() < total {
        return None;
    }
    let stored = u32::from_le_bytes(bytes[total - 4..total].try_into().ok()?);
    if crc32c(&bytes[..total - 4]) != stored {
        return None;
    }
    let mut runs = Vec::with_capacity(count);
    let mut pos = 5;
    for _ in 0..count {
        runs.push(FreePageRun {
            start: u64::from_le_bytes(bytes[pos..pos + 8].try_into().ok()?),
            len: u64::from_le_bytes(bytes[pos + 8..pos + 16].try_into().ok()?),
            freed_gen: u64::from_le_bytes(bytes[pos + 16..pos + 24].try_into().ok()?),
        });
        pos += 24;
    }
    Some(runs)
}

fn extent_value(run: FreePageRun) -> pagebtree::FreePageExtentValue {
    pagebtree::FreePageExtentValue {
        len: run.len,
        freed_gen: run.freed_gen,
    }
}

fn extent_run(key: &[u8; 32], value: pagebtree::FreePageExtentValue) -> Result<FreePageRun> {
    let start = extent_start_from_key(key)?;
    value.validate_start(start)?;
    Ok(FreePageRun {
        start,
        len: value.len,
        freed_gen: value.freed_gen,
    })
}

pub(crate) fn collect_map_pages(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
) -> Result<BTreeSet<u64>> {
    Ok(
        pagebtree::collect_free_page_extent_pages(file, header_len, root, page_count)?
            .into_iter()
            .map(|page| page.0)
            .collect(),
    )
}

fn collect_intersecting_extent_keys(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    run: FreePageRun,
    page_count: u64,
    keys: &mut BTreeSet<[u8; 32]>,
) -> Result<()> {
    let end = run
        .start
        .checked_add(run.len)
        .ok_or_else(|| corrupt("free-page extent overflow"))?;
    if run.len == 0 {
        return Err(corrupt("free-page extent value has zero length"));
    }
    let low = extent_key(run.start);
    if pagebtree::free_page_extent_get(file, header_len, Some(root), &low, page_count)?.is_some() {
        keys.insert(low);
    }
    if let Some((predecessor_key, value)) =
        pagebtree::free_page_extent_predecessor(file, header_len, Some(root), &low, page_count)?
    {
        let predecessor = extent_run(&predecessor_key, value)?;
        if predecessor.start.saturating_add(predecessor.len) > run.start {
            keys.insert(predecessor_key);
        }
    }
    let successor_low = extent_key(run.start + 1);
    let high = extent_key(end);
    for (key, _) in pagebtree::free_page_extent_range(
        file,
        header_len,
        root,
        &successor_low,
        &high,
        page_count,
    )? {
        keys.insert(key);
    }
    Ok(())
}

pub(crate) fn prepare_tree_map_publication(
    file: &mut dyn BackingIo,
    header_len: u64,
    previous_root: Option<PageId>,
    previous: &[FreePageRun],
    source_updates: Vec<FreeMapExtentUpdate>,
    updates: Vec<FreeMapExtentUpdate>,
    source_page_count: u64,
) -> Result<PreparedFreeMapPublication> {
    let tree_updates = if previous_root.is_none() {
        initial_tree_updates(previous, updates.clone())
    } else {
        updates.clone()
    };
    let mut delete_keys = BTreeSet::new();
    let mut upserts = Vec::new();
    for update in &tree_updates {
        match update {
            FreeMapExtentUpdate::Delete(run) => {
                delete_keys.insert(extent_key(run.start));
            }
            FreeMapExtentUpdate::Upsert(run) => upserts.push(*run),
        }
    }
    upserts.sort_by_key(|run| run.start);

    let read_bound = source_page_count;
    if let Some(root) = previous_root {
        for update in &tree_updates {
            let run = match update {
                FreeMapExtentUpdate::Delete(run) | FreeMapExtentUpdate::Upsert(run) => *run,
            };
            collect_intersecting_extent_keys(
                file,
                header_len,
                root,
                run,
                read_bound,
                &mut delete_keys,
            )?;
        }
    }
    let typed_upserts = upserts
        .iter()
        .map(|run| (extent_key(run.start), extent_value(*run)))
        .collect::<Vec<_>>();
    let delete_keys = delete_keys.into_iter().collect::<Vec<_>>();
    let tree_delta = pagebtree::prepare_free_page_extent_delta(
        file,
        header_len,
        previous_root,
        source_page_count,
        &delete_keys,
        &typed_upserts,
    )?;
    let demand = FreeMapPublicationDemand {
        extent_deletes: delete_keys.len() as u64,
        extent_upserts: typed_upserts.len() as u64,
        btree_node_pages: tree_delta.allocation_calls(),
        affected_existing_btree_pages: tree_delta.affected_page_count(),
        split_decisions: tree_delta.split_decision_count(),
    };
    Ok(PreparedFreeMapPublication {
        previous_root,
        source_page_count,
        source_updates,
        updates,
        tree_delta,
        demand,
    })
}

fn initial_tree_updates(
    previous: &[FreePageRun],
    updates: Vec<FreeMapExtentUpdate>,
) -> Vec<FreeMapExtentUpdate> {
    let mut map = previous
        .iter()
        .copied()
        .map(|run| (run.start, run))
        .collect::<BTreeMap<_, _>>();
    for update in updates {
        match update {
            FreeMapExtentUpdate::Delete(run) => {
                let end = run.start.saturating_add(run.len);
                map.retain(|start, _| *start < run.start || *start >= end);
            }
            FreeMapExtentUpdate::Upsert(run) => {
                map.insert(run.start, run);
            }
        }
    }
    map.into_values().map(FreeMapExtentUpdate::Upsert).collect()
}

pub(crate) fn apply_prepared_tree_map_publication(
    file: &mut dyn BackingIo,
    header_len: u64,
    alloc: &mut PageAllocator,
    previous_root: Option<PageId>,
    updates: Vec<FreeMapExtentUpdate>,
    prepared: PreparedFreeMapPublication,
    allocated_pages: &[PageId],
) -> Result<Option<PageId>> {
    if previous_root != prepared.previous_root || updates != prepared.updates {
        return Err(corrupt(&format!(
            "prepared free-map publication input mismatch: previous={previous_root:?} expected_previous={:?} updates={updates:?} expected_updates={:?}",
            prepared.previous_root, prepared.updates,
        )));
    }
    validate_prepared_tree_map_publication_assigned_pages(alloc, &prepared, allocated_pages)?;
    let prior_reuse_before = alloc.reuse_before;
    let prior_suppression = alloc.replace_free_map_tracking_suppression(true);
    alloc.reuse_before = Some(0);
    let result = apply_prepared_tree_map_publication_inner(
        file,
        header_len,
        alloc,
        prepared,
        allocated_pages,
    );
    alloc.reuse_before = prior_reuse_before;
    alloc.replace_free_map_tracking_suppression(prior_suppression);
    result
}

pub(crate) fn validate_prepared_tree_map_publication_source(
    previous_root: Option<PageId>,
    pending_updates: &[FreeMapExtentUpdate],
    prepared: &PreparedFreeMapPublication,
) -> Result<()> {
    if previous_root != prepared.previous_root || pending_updates != prepared.source_updates {
        return Err(corrupt(&format!(
            "prepared free-map publication source mismatch: previous={previous_root:?} expected_previous={:?} updates={pending_updates:?} expected_updates={:?}",
            prepared.previous_root, prepared.source_updates,
        )));
    }
    Ok(())
}

fn validate_prepared_tree_map_publication_assigned_pages(
    alloc: &PageAllocator,
    prepared: &PreparedFreeMapPublication,
    allocated_pages: &[PageId],
) -> Result<()> {
    let tree_count = usize::try_from(prepared.demand.btree_node_pages)
        .map_err(|_| corrupt("prepared free-map btree page count"))?;
    if allocated_pages.len() != tree_count
        || allocated_pages
            .iter()
            .any(|page| !alloc.allocated_in_transaction(page.0))
    {
        return Err(corrupt("prepared free-map page allocation mismatch"));
    }
    Ok(())
}

fn apply_prepared_tree_map_publication_inner(
    file: &mut dyn BackingIo,
    header_len: u64,
    alloc: &mut PageAllocator,
    prepared: PreparedFreeMapPublication,
    allocated_pages: &[PageId],
) -> Result<Option<PageId>> {
    let applied = pagebtree::apply_prepared_free_page_extent_delta_on_pages(
        file,
        header_len,
        alloc,
        prepared.previous_root,
        prepared.source_page_count,
        prepared.tree_delta,
        allocated_pages,
    )?;
    #[cfg(any(test, feature = "test-hooks"))]
    alloc.note_free_map_publication_stats(
        prepared.demand.extent_deletes,
        prepared.demand.extent_upserts,
        prepared.demand.affected_existing_btree_pages,
        prepared.demand.split_decisions,
    );
    Ok(applied.root)
}

#[cfg(test)]
fn write_extent_tree_updates(
    file: &mut dyn BackingIo,
    header_len: u64,
    alloc: &mut PageAllocator,
    root: Option<PageId>,
    previous: &[FreePageRun],
    updates: Vec<FreeMapExtentUpdate>,
) -> Result<Option<PageId>> {
    let prepared = prepare_tree_map_publication(
        file,
        header_len,
        root,
        previous,
        updates.clone(),
        updates.clone(),
        alloc.page_count(),
    )?;
    let allocated = (0..prepared.demand().allocation_pages())
        .map(|_| alloc.alloc(1))
        .collect::<Vec<_>>();
    apply_prepared_tree_map_publication(
        file, header_len, alloc, root, updates, prepared, &allocated,
    )
}

#[cfg(test)]
pub(crate) fn write_tree_map(
    file: &mut dyn BackingIo,
    header_len: u64,
    alloc: &mut PageAllocator,
    previous_root: Option<PageId>,
    previous: &[FreePageRun],
    updates: Vec<FreeMapExtentUpdate>,
) -> Result<Option<PageId>> {
    write_extent_tree_updates(file, header_len, alloc, previous_root, previous, updates)
}

/// Read and decode the free-page map rooted at `root`. Reads are bounded by `page_count`, so a
/// crafted root or run count is a clean CORRUPT error.
#[cfg(test)]
pub(crate) fn read_map(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
) -> Result<Vec<FreePageRun>> {
    read_map_with_root_span(file, header_len, root, page_count).map(|(runs, _)| runs)
}

pub(crate) fn read_map_with_root_span(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
) -> Result<(Vec<FreePageRun>, u64)> {
    let mut runs = pagebtree::load_all_free_page_extents(file, header_len, root, page_count)?
        .into_iter()
        .map(|(key, value)| extent_run(&key, value))
        .collect::<Result<Vec<_>>>()?;
    runs.sort_by_key(|r| r.start);
    validate_runs(&runs, page_count)?;
    Ok((coalesce_free_runs(runs), 1))
}

fn validate_runs(runs: &[FreePageRun], page_count: u64) -> Result<()> {
    let mut prev_end = 0u64;
    for r in runs {
        let end = r
            .start
            .checked_add(r.len)
            .ok_or_else(|| corrupt("free-page run overflows"))?;
        if r.len == 0 || end > page_count || r.start < prev_end {
            return Err(LoomError::new(
                Code::CorruptObject,
                format!(
                    "free-page run out of range or overlapping: start={} len={} end={} previous_end={} page_count={page_count}",
                    r.start, r.len, end, prev_end
                ),
            ));
        }
        prev_end = end;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    const HEADER: u64 = 3 * PAGE_SIZE;
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn run(start: u64, len: u64, freed_gen: u64) -> FreePageRun {
        FreePageRun {
            start,
            len,
            freed_gen,
        }
    }

    struct Temp(PathBuf, File);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    fn temp() -> Temp {
        let mut p = std::env::temp_dir();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        p.push(format!("loom-pagemap-{}-{n}.tmp", std::process::id()));
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&p)
            .unwrap();
        Temp(p, f)
    }

    #[test]
    fn extends_when_no_reusable_run_fits() {
        let mut a = PageAllocator::new(10, 100, vec![]);
        assert_eq!(a.alloc(3), PageId(10));
        assert_eq!(a.alloc(1), PageId(13));
        assert_eq!(a.page_count(), 14);
    }

    #[test]
    fn reuses_only_runs_outside_the_window() {
        // Freed long ago (gen 1) -> reusable at gen 100; freed recently is not.
        let mut a = PageAllocator::new(50, 100, vec![run(4, 2, 1), run(20, 5, 99)]);
        assert_eq!(a.alloc(2), PageId(4)); // reuses the old run
        assert_eq!(a.alloc(5), PageId(50)); // the recent run is inside the window, so extend instead
        assert_eq!(a.page_count(), 55);
    }

    #[test]
    fn splits_a_larger_reused_run() {
        let mut a = PageAllocator::new(100, 100, vec![run(8, 5, 1)]);
        assert_eq!(a.alloc(2), PageId(8)); // takes 8..10, leaves 10..13 free
        assert_eq!(a.page_count(), 100); // no extension
        assert_eq!(a.alloc(3), PageId(10)); // reuses the remainder
        assert_eq!(a.page_count(), 100);
    }

    #[test]
    fn repeated_splits_publish_only_the_net_committed_extent_change() {
        let initial = run(10, 10, 1);
        let mut allocator =
            PageAllocator::new_with_reusable_runs(100, 100, vec![initial], vec![initial]);

        for expected in 10..15 {
            assert_eq!(allocator.alloc(1), PageId(expected));
        }

        let updates = allocator.take_free_map_extent_updates();
        assert_eq!(updates.len(), 2);
        assert!(
            updates.iter().any(
                |update| matches!(update, FreeMapExtentUpdate::Delete(run) if run.start == 10)
            )
        );
        assert!(updates.iter().any(|update| matches!(
            update,
            FreeMapExtentUpdate::Upsert(run)
                if *run == FreePageRun { start: 15, len: 5, freed_gen: 1 }
        )));
    }

    #[test]
    fn free_map_extent_updates_ignore_untouched_cardinality() {
        fn run_case(untouched_extent_count: u64) -> (Vec<FreeMapExtentUpdate>, Vec<FreePageRun>) {
            let absorbed = run(10, 1, 1);
            let untouched = (0..untouched_extent_count)
                .map(|index| run(100 + index * 3, 1, 1))
                .collect::<Vec<_>>();
            let mut initial = vec![absorbed];
            initial.extend(untouched.iter().copied());
            let mut allocator =
                PageAllocator::new_with_reusable_runs(10_000, 100, initial, vec![absorbed]);

            assert_eq!(allocator.alloc(1), PageId(10));
            allocator.free(PageId(9), 2).unwrap();

            assert_eq!(allocator.pending_free_map_extent_update_count(), 1);
            let updates = allocator.take_free_map_extent_updates();
            let snapshot = allocator.snapshot_free();
            assert_eq!(
                snapshot
                    .iter()
                    .copied()
                    .filter(|extent| extent.start < 11 && extent.start + extent.len > 9)
                    .collect::<Vec<_>>(),
                vec![run(9, 2, 100)]
            );
            assert_eq!(
                updates,
                vec![
                    FreeMapExtentUpdate::Delete(absorbed),
                    FreeMapExtentUpdate::Upsert(run(9, 2, 100)),
                ]
            );
            for extent in untouched {
                assert!(snapshot.contains(&extent));
                assert!(!updates.iter().any(|update| match update {
                    FreeMapExtentUpdate::Delete(changed) | FreeMapExtentUpdate::Upsert(changed) =>
                        changed.start == extent.start,
                }));
            }
            (updates, snapshot)
        }

        let (small_updates, small_snapshot) = run_case(4);
        let (large_updates, large_snapshot) = run_case(512);
        assert_eq!(small_updates, large_updates);
        assert_eq!(small_snapshot.len(), 5);
        assert_eq!(large_snapshot.len(), 513);
    }

    #[test]
    fn fully_consumed_original_extent_keeps_its_persisted_delete() {
        let initial = run(10, 5, 1);
        let mut allocator =
            PageAllocator::new_with_reusable_runs(100, 100, vec![initial], vec![initial]);

        for expected in 10..15 {
            assert_eq!(allocator.alloc(1), PageId(expected));
        }

        let updates = allocator.take_free_map_extent_updates();
        assert_eq!(updates.len(), 1);
        assert!(matches!(updates[0], FreeMapExtentUpdate::Delete(run) if run == initial));
        assert!(allocator.snapshot_free().is_empty());
    }

    #[test]
    fn transaction_free_reuse_preserves_the_committed_extent_delete() {
        let initial = run(10, 1, 1);
        let mut allocator =
            PageAllocator::new_with_reusable_runs(100, 100, vec![initial], vec![initial]);

        let page = allocator.alloc(1);
        assert_eq!(page, PageId(10));
        allocator.free(page, 1).unwrap();
        assert_eq!(allocator.alloc(1), page);

        let updates = allocator.take_free_map_extent_updates();
        assert_eq!(updates, vec![FreeMapExtentUpdate::Delete(initial)]);
        assert!(allocator.snapshot_free().is_empty());
    }

    #[test]
    fn publication_never_consumes_unreserved_transaction_frees() {
        let mut allocator = PageAllocator::new(20, 100, Vec::new());
        allocator.free(PageId(10), 1).unwrap();
        allocator.activate_publication_reserve();

        assert_eq!(allocator.alloc(1), PageId(20));
        assert_eq!(allocator.snapshot_free(), vec![run(10, 1, 100)]);
    }

    #[test]
    fn publication_does_not_recycle_a_superseded_reserved_page() {
        let reserved = run(4, 2, 1);
        let mut allocator = PageAllocator::new_with_reusable_runs_and_publication_reserve(
            20,
            100,
            vec![reserved],
            vec![reserved],
            vec![reserved],
        );
        allocator.activate_publication_reserve();
        let page = allocator.alloc(1);
        assert_eq!(page, PageId(4));
        allocator.free(page, 1).unwrap();

        assert_eq!(allocator.alloc(1), PageId(5));
        assert_eq!(allocator.transaction_stats().publication_unused_pages, 0);
    }

    fn assert_publication_reserve_rejected_atomically(
        mut allocator: PageAllocator,
        selected: &[FreePageRun],
    ) {
        let free = allocator.free.clone();
        let reusable_runs = allocator.reusable_runs.clone();
        let publication_eligible_runs = allocator.publication_eligible_runs.clone();
        let publication_reserve = allocator.publication_reserve.clone();
        let publication_reserved_pages = allocator.publication_reserved_pages;
        let publication_reserve_active = allocator.publication_reserve_active;
        let allocated_runs = allocator.allocated_runs.clone();
        let active_allocated_pages = allocator.active_allocated_pages.clone();
        let dirty_free_ranges = allocator.dirty_free_ranges.clone();
        let transaction_stats = allocator.transaction_stats;
        let snapshot = allocator.snapshot_free();
        let page_count = allocator.page_count();

        let error = allocator
            .install_publication_reserve(selected)
            .expect_err("invalid publication reserve must fail");

        assert_eq!(error.code, Code::InvalidArgument);
        assert_eq!(allocator.free, free);
        assert_eq!(allocator.reusable_runs, reusable_runs);
        assert_eq!(
            allocator.publication_eligible_runs,
            publication_eligible_runs
        );
        assert_eq!(allocator.publication_reserve, publication_reserve);
        assert_eq!(
            allocator.publication_reserved_pages,
            publication_reserved_pages
        );
        assert_eq!(
            allocator.publication_reserve_active,
            publication_reserve_active
        );
        assert_eq!(allocator.allocated_runs, allocated_runs);
        assert_eq!(allocator.active_allocated_pages, active_allocated_pages);
        assert_eq!(allocator.dirty_free_ranges, dirty_free_ranges);
        assert_eq!(allocator.transaction_stats, transaction_stats);
        assert_eq!(allocator.snapshot_free(), snapshot);
        assert_eq!(allocator.page_count(), page_count);
    }

    #[test]
    fn mu17j_l_g2a_late_publication_reserve_isolated_until_activation() {
        let initial = run(10, 10, 1);
        let mut allocator = PageAllocator::new_with_reusable_authorities(
            20,
            100,
            vec![initial],
            vec![run(10, 4, 1)],
            vec![run(14, 6, 1)],
        );

        assert_eq!(allocator.alloc(4), PageId(10));
        assert_eq!(allocator.alloc(1), PageId(20));
        allocator
            .install_publication_reserve(&[run(14, 4, 1)])
            .unwrap();
        assert_eq!(allocator.publication_reserve.get(&14), Some(&(4, 1)));
        assert_eq!(allocator.free.get(&18), Some(&(2, 1)));
        assert_eq!(allocator.page_count(), 21);

        allocator.activate_publication_reserve();
        assert_eq!(allocator.alloc(2), PageId(14));
        assert_eq!(allocator.page_count(), 21);
        assert_eq!(allocator.publication_reserve.get(&16), Some(&(2, 1)));
        assert_eq!(allocator.free.get(&18), Some(&(2, 1)));
        assert_eq!(allocator.snapshot_free(), vec![run(16, 4, 1)]);
        let stats = allocator.transaction_stats();
        assert_eq!(stats.publication_reserved_pages, 4);
        assert_eq!(stats.publication_reused_pages, 2);
        assert_eq!(stats.publication_unused_pages, 2);
        assert_eq!(stats.extended_pages, 1);

        let updates = allocator.take_free_map_extent_updates();
        assert!(matches!(
            updates.first(),
            Some(FreeMapExtentUpdate::Delete(run)) if *run == initial
        ));
        let final_runs = coalesce_free_runs(
            updates
                .into_iter()
                .filter_map(|update| match update {
                    FreeMapExtentUpdate::Upsert(run) => Some(run),
                    FreeMapExtentUpdate::Delete(_) => None,
                })
                .collect(),
        );
        assert_eq!(final_runs, allocator.snapshot_free());
    }

    #[test]
    fn mu17j_l_g2a_invalid_publication_reserve_is_rejected_atomically() {
        let initial = run(10, 10, 1);
        let allocator =
            || PageAllocator::new_with_reusable_runs(20, 100, vec![initial], vec![initial]);

        assert_publication_reserve_rejected_atomically(allocator(), &[run(12, 0, 1)]);
        assert_publication_reserve_rejected_atomically(allocator(), &[run(19, 2, 1)]);
        assert_publication_reserve_rejected_atomically(
            allocator(),
            &[run(12, 3, 1), run(14, 2, 1)],
        );
        assert_publication_reserve_rejected_atomically(allocator(), &[run(12, 2, 2)]);

        let ineligible =
            PageAllocator::new_with_reusable_runs(20, 100, vec![initial], vec![run(10, 5, 1)]);
        assert_publication_reserve_rejected_atomically(ineligible, &[run(16, 2, 1)]);

        let mut allocated = allocator();
        assert_eq!(allocated.alloc(1), PageId(10));
        assert_publication_reserve_rejected_atomically(allocated, &[run(10, 1, 1)]);
    }

    #[test]
    fn mu17j_l_g2a_unsorted_reserve_normalizes_and_shortfall_extends_exactly() {
        let initial = run(10, 10, 1);
        let mut allocator = PageAllocator::new_with_reusable_authorities(
            20,
            100,
            vec![initial],
            vec![run(10, 4, 1)],
            vec![run(14, 6, 1)],
        );
        allocator
            .install_publication_reserve(&[run(16, 2, 1), run(14, 2, 1)])
            .unwrap();
        assert_eq!(allocator.publication_reserve.len(), 1);
        assert_eq!(allocator.publication_reserve.get(&14), Some(&(4, 1)));
        allocator.activate_publication_reserve();

        assert_eq!(allocator.alloc(4), PageId(14));
        assert_eq!(allocator.page_count(), 20);
        assert_eq!(allocator.alloc(3), PageId(20));
        assert_eq!(allocator.page_count(), 23);
        assert_eq!(
            allocator.snapshot_free(),
            vec![run(10, 4, 1), run(18, 2, 1)]
        );
        let stats = allocator.transaction_stats();
        assert_eq!(stats.publication_reserved_pages, 4);
        assert_eq!(stats.publication_reused_pages, 4);
        assert_eq!(stats.publication_unused_pages, 0);
        assert_eq!(stats.publication_reserve_exhaustions, 1);
        assert_eq!(stats.extended_pages, 3);
    }

    #[test]
    fn mu17j_l_captured_free_ordinary_prefix_and_publication_suffix_do_not_overlap() {
        let current_free = vec![run(10, 2, 1), run(20, 1, 1), run(30, 4, 1)];
        let mut allocator = PageAllocator::new_with_reusable_authorities(
            64,
            100,
            current_free,
            Vec::new(),
            vec![run(10, 2, 1), run(20, 1, 1), run(30, 4, 1)],
        );
        allocator
            .install_captured_free_authority(CapturedFreeAllocationAuthority {
                runs: vec![
                    CapturedFreeRun {
                        run: run(10, 2, 1),
                        cursor_start: 3,
                        cursor_end: 5,
                    },
                    CapturedFreeRun {
                        run: run(20, 1, 1),
                        cursor_start: 8,
                        cursor_end: 9,
                    },
                    CapturedFreeRun {
                        run: run(30, 4, 1),
                        cursor_start: 11,
                        cursor_end: 15,
                    },
                ],
                consumed_through: 0,
                page_count: 15,
            })
            .unwrap();

        assert_eq!(allocator.alloc(3), PageId(30));
        assert_eq!(allocator.captured_free_consumed_through(), Some(14));
        let publication = allocator.select_captured_publication_reserve(1).unwrap();
        assert_eq!(publication, vec![run(33, 1, 1)]);
        assert_eq!(allocator.captured_free_consumed_through(), Some(15));
        assert_eq!(allocator.page_count(), 64);
        allocator.activate_publication_reserve();
        assert_eq!(allocator.alloc(1), PageId(33));
        assert_eq!(allocator.page_count(), 64);
    }

    #[test]
    fn mu17j_l_captured_free_short_extents_advance_cursor_before_extension() {
        let current_free = vec![run(10, 1, 1), run(20, 2, 1)];
        let mut allocator = PageAllocator::new_with_reusable_authorities(
            40,
            100,
            current_free,
            Vec::new(),
            vec![run(10, 1, 1), run(20, 2, 1)],
        );
        allocator
            .install_captured_free_authority(CapturedFreeAllocationAuthority {
                runs: vec![
                    CapturedFreeRun {
                        run: run(10, 1, 1),
                        cursor_start: 2,
                        cursor_end: 3,
                    },
                    CapturedFreeRun {
                        run: run(20, 2, 1),
                        cursor_start: 6,
                        cursor_end: 8,
                    },
                ],
                consumed_through: 0,
                page_count: 8,
            })
            .unwrap();

        assert_eq!(allocator.alloc(3), PageId(40));
        assert_eq!(allocator.captured_free_consumed_through(), Some(8));
        assert_eq!(
            allocator.snapshot_free(),
            vec![run(10, 1, 1), run(20, 2, 1)]
        );
    }

    #[test]
    fn metadata_bootstrap_reserve_consumes_and_carries_unused_pages() {
        let mut allocator = PageAllocator::new(40, 7, Vec::new());
        allocator.ensure_metadata_bootstrap_capacity().unwrap();
        assert_eq!(allocator.page_count(), 40 + METADATA_BOOTSTRAP_TARGET_PAGES);
        let before = allocator.metadata_bootstrap_descriptor(7);
        assert_eq!(before.extents.len(), 1);
        assert_eq!(before.extents[0].start, 40);
        assert_eq!(before.extents[0].len, METADATA_BOOTSTRAP_TARGET_PAGES);

        let allocated = allocator.alloc_metadata_bootstrap_pages(3).unwrap();
        assert_eq!(allocated, vec![PageId(40), PageId(41), PageId(42)]);
        let after = allocator.metadata_bootstrap_descriptor(8);
        assert_eq!(after.owning_generation, 8);
        assert_eq!(
            after.extents,
            vec![MetadataBootstrapExtent {
                start: 43,
                len: METADATA_BOOTSTRAP_TARGET_PAGES - 3,
            }]
        );
        assert!(allocator.snapshot_free().is_empty());
    }

    #[test]
    fn metadata_bootstrap_refill_prefers_eligible_free_pages_and_tracks_removal() {
        let current = METADATA_BOOTSTRAP_REFILL_THRESHOLD_PAGES;
        let reserve_start = 100;
        let free_start = reserve_start + current + 10;
        let refill = METADATA_BOOTSTRAP_TARGET_PAGES - current;
        let page_count = free_start + refill + 5;
        let eligible = run(free_start, refill + 5, 1);
        let mut allocator = PageAllocator::new_with_reusable_authorities(
            page_count,
            100,
            vec![eligible],
            Vec::new(),
            vec![eligible],
        );
        allocator
            .install_metadata_bootstrap_reserve(&MetadataBootstrapReserve {
                owning_generation: 99,
                capacity: METADATA_BOOTSTRAP_CAPACITY_PAGES,
                extents: vec![MetadataBootstrapExtent {
                    start: reserve_start,
                    len: current,
                }],
            })
            .unwrap();

        allocator.ensure_metadata_bootstrap_capacity().unwrap();
        assert_eq!(allocator.page_count(), page_count);
        assert_eq!(
            allocator.metadata_bootstrap_page_count(),
            METADATA_BOOTSTRAP_TARGET_PAGES
        );
        assert_eq!(
            allocator.snapshot_free(),
            vec![run(free_start + refill, 5, 1)]
        );
        assert_eq!(
            allocator.take_free_map_extent_updates(),
            vec![
                FreeMapExtentUpdate::Delete(eligible),
                FreeMapExtentUpdate::Upsert(run(free_start + refill, 5, 1)),
            ]
        );
    }

    #[test]
    fn metadata_bootstrap_below_refill_threshold_reuses_eligible_free_pages() {
        let current = METADATA_BOOTSTRAP_REFILL_THRESHOLD_PAGES - 1;
        let reserve_start = 100;
        let free_start = reserve_start + current + 10;
        let refill = METADATA_BOOTSTRAP_TARGET_PAGES - current;
        let page_count = free_start + refill;
        let eligible = run(free_start, refill, 1);
        let mut allocator = PageAllocator::new_with_reusable_authorities(
            page_count,
            100,
            vec![eligible],
            Vec::new(),
            vec![eligible],
        );
        allocator
            .install_metadata_bootstrap_reserve(&MetadataBootstrapReserve {
                owning_generation: 99,
                capacity: METADATA_BOOTSTRAP_CAPACITY_PAGES,
                extents: vec![MetadataBootstrapExtent {
                    start: reserve_start,
                    len: current,
                }],
            })
            .unwrap();

        allocator.ensure_metadata_bootstrap_capacity().unwrap();

        assert_eq!(allocator.page_count(), page_count);
        assert_eq!(
            allocator.metadata_bootstrap_page_count(),
            METADATA_BOOTSTRAP_TARGET_PAGES
        );
        assert!(allocator.snapshot_free().is_empty());
        assert_eq!(
            allocator.take_free_map_extent_updates(),
            vec![FreeMapExtentUpdate::Delete(eligible)]
        );
    }

    #[test]
    fn metadata_bootstrap_over_capacity_rejects_without_mutation() {
        let mut allocator = PageAllocator::new(40, 7, Vec::new());
        allocator.ensure_metadata_bootstrap_capacity().unwrap();
        let before = allocator.metadata_bootstrap_descriptor(7);
        let before_end = allocator.page_count();
        let error = allocator
            .alloc_metadata_bootstrap_pages(FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES + 1)
            .unwrap_err();
        assert_eq!(error.code, Code::ResourceExhausted);
        assert_eq!(allocator.metadata_bootstrap_descriptor(7), before);
        assert_eq!(allocator.page_count(), before_end);
    }

    #[test]
    fn metadata_bootstrap_shortfall_extends_exactly_after_demand_is_known() {
        let mut allocator = PageAllocator::new(40, 7, Vec::new());
        allocator.ensure_metadata_bootstrap_capacity().unwrap();
        let before_end = allocator.page_count();
        let demand = METADATA_BOOTSTRAP_TARGET_PAGES + 5;
        let allocated = allocator.alloc_metadata_bootstrap_pages(demand).unwrap();
        assert_eq!(allocated.len() as u64, demand);
        assert_eq!(allocator.page_count(), before_end + 5);
        assert_eq!(allocator.metadata_bootstrap_page_count(), 0);
        let stats = allocator.transaction_stats();
        assert_eq!(
            stats.metadata_bootstrap_reused_pages,
            METADATA_BOOTSTRAP_TARGET_PAGES
        );
        assert_eq!(
            stats.metadata_bootstrap_extended_pages,
            METADATA_BOOTSTRAP_TARGET_PAGES + 5
        );
    }

    #[test]
    fn metadata_bootstrap_reserve_rejects_canonical_and_deferred_free_overlap() {
        let reserve = MetadataBootstrapReserve {
            owning_generation: 7,
            capacity: METADATA_BOOTSTRAP_CAPACITY_PAGES,
            extents: vec![MetadataBootstrapExtent { start: 40, len: 4 }],
        };
        let mut allocator = PageAllocator::new(64, 8, Vec::new());
        allocator
            .install_metadata_bootstrap_reserve(&reserve)
            .unwrap();
        let before = allocator.snapshot_free();

        let free_error = allocator.free(PageId(42), 1).unwrap_err();
        assert_eq!(free_error.code, loom_core::error::Code::CorruptObject);
        let deferred_error = allocator.defer_free(PageId(39), 2).unwrap_err();
        assert_eq!(deferred_error.code, loom_core::error::Code::CorruptObject);
        assert_eq!(allocator.snapshot_free(), before);
        assert_eq!(allocator.metadata_bootstrap_descriptor(7), reserve);
    }

    fn assert_invalid_free_span_rejected_atomically(
        deferred: bool,
        start: PageId,
        len: u64,
        expected_message: &str,
    ) {
        let initial = run(10, 10, 1);
        let mut allocator =
            PageAllocator::new_with_reusable_runs(100, 100, vec![initial], vec![initial]);
        assert_eq!(allocator.alloc(1), PageId(10));
        let before = allocator.clone();

        let error = if deferred {
            allocator.defer_free(start, len).unwrap_err()
        } else {
            allocator.free(start, len).unwrap_err()
        };

        assert_eq!(error.code, Code::CorruptObject);
        assert!(error.message.contains(expected_message));
        assert_eq!(allocator.page_count(), before.page_count());
        assert_eq!(allocator.allocated_runs, before.allocated_runs);
        assert_eq!(
            allocator.active_allocated_pages,
            before.active_allocated_pages
        );
        assert_eq!(allocator.free, before.free);
        assert_eq!(allocator.txn_freed, before.txn_freed);
        assert_eq!(allocator.deferred_freed, before.deferred_freed);
        assert_eq!(allocator.publication_reserve, before.publication_reserve);
        assert_eq!(
            allocator.metadata_bootstrap_reserve,
            before.metadata_bootstrap_reserve
        );
        assert_eq!(allocator.dirty_free_ranges, before.dirty_free_ranges);
        assert_eq!(allocator.free_origins, before.free_origins);
        assert_eq!(allocator.transaction_stats, before.transaction_stats);
        assert_eq!(allocator.snapshot_free(), before.snapshot_free());
    }

    #[test]
    fn free_rejects_invalid_spans_before_mutation() {
        assert_invalid_free_span_rejected_atomically(false, PageId(10), 0, "must be nonempty");
        assert_invalid_free_span_rejected_atomically(false, PageId(u64::MAX), 1, "overflows");
        assert_invalid_free_span_rejected_atomically(
            false,
            PageId(99),
            2,
            "exceeds allocator page bound",
        );
    }

    #[test]
    fn defer_free_rejects_invalid_spans_before_mutation() {
        assert_invalid_free_span_rejected_atomically(true, PageId(10), 0, "must be nonempty");
        assert_invalid_free_span_rejected_atomically(true, PageId(u64::MAX), 1, "overflows");
        assert_invalid_free_span_rejected_atomically(
            true,
            PageId(99),
            2,
            "exceeds allocator page bound",
        );
    }

    #[test]
    fn multi_page_alloc_needs_one_contiguous_run() {
        // Two small runs cannot satisfy a 4-page request; the allocator extends.
        let mut a = PageAllocator::new(30, 100, vec![run(2, 2, 1), run(6, 2, 1)]);
        assert_eq!(a.alloc(4), PageId(30));
    }

    #[test]
    fn snapshot_carries_existing_and_freshly_freed_runs() {
        let mut a = PageAllocator::new(40, 7, vec![run(3, 1, 2)]);
        a.free(PageId(12), 4).unwrap();
        let mut snap = a.snapshot_free();
        snap.sort_by_key(|r| r.start);
        assert_eq!(snap, vec![run(3, 1, 2), run(12, 4, 7)]);
    }

    #[test]
    fn snapshot_coalesces_overlapping_allocator_sources_conservatively() {
        let mut allocator = PageAllocator::new(40, 7, vec![run(3, 4, 2)]);
        allocator.txn_freed.insert(5, 3);

        assert_eq!(allocator.snapshot_free(), vec![run(3, 5, 7)]);
    }

    #[test]
    fn deferred_free_appears_in_snapshot_but_cannot_be_allocated_by_transaction() {
        let mut a = PageAllocator::new_with_current_free_reusable(40, 7, vec![run(3, 1, 2)]);
        a.defer_free(PageId(12), 4).unwrap();

        assert_eq!(a.alloc(4), PageId(40));
        let mut snap = a.snapshot_free();
        snap.sort_by_key(|r| r.start);
        assert_eq!(snap, vec![run(3, 1, 2), run(12, 4, 7)]);
    }

    #[test]
    fn adjacent_free_runs_preserve_distinct_reuse_generations() {
        let mut a = PageAllocator::new(40, 100, vec![run(3, 2, 1)]);
        a.free(PageId(5), 2).unwrap();
        let snapshot = a.snapshot_free();

        assert_eq!(snapshot, vec![run(3, 2, 1), run(5, 2, 100)]);
        assert_eq!(a.alloc(2), PageId(3));
        assert_eq!(a.alloc(2), PageId(40));
    }

    #[test]
    fn map_survives_a_reopen() {
        let runs = vec![run(4, 2, 1), run(9, 3, 2), run(100, 1, 3)];
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut a = PageAllocator::new(200, 5, vec![]);
        let root = write_tree_map(
            &mut file,
            HEADER,
            &mut a,
            None,
            &[],
            runs.iter()
                .copied()
                .map(FreeMapExtentUpdate::Upsert)
                .collect(),
        )
        .unwrap()
        .unwrap();
        drop(file);

        let mut reopened = OpenOptions::new().read(true).open(&t.0).unwrap();
        crate::record_io::reset_blob_locator_reads_for_test();
        let back = read_map(&mut reopened, HEADER, root, a.page_count()).unwrap();
        assert_eq!(back, runs);
        assert_eq!(crate::record_io::blob_locator_reads_for_test(), 0);
    }

    #[test]
    fn read_map_requires_the_inline_extent_codec() {
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut allocator = PageAllocator::new(0, 1, vec![]);
        let root = pagebtree::build_packed(
            &mut file,
            HEADER,
            &mut allocator,
            &[(extent_key(4), crate::record::RecordLoc::from_global(12, 0))],
        )
        .unwrap()
        .unwrap();

        let error = read_map(&mut file, HEADER, root, allocator.page_count()).unwrap_err();
        assert_eq!(error.code, Code::CorruptObject);
        assert!(error.message.contains("codec"));
    }

    #[test]
    fn extent_update_replaces_an_overlapping_predecessor() {
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut initial = PageAllocator::new(100, 1, vec![]);
        let root = write_tree_map(
            &mut file,
            HEADER,
            &mut initial,
            None,
            &[],
            vec![FreeMapExtentUpdate::Upsert(run(4, 8, 1))],
        )
        .unwrap();

        let prior = read_map(&mut file, HEADER, root.unwrap(), initial.page_count()).unwrap();
        let mut replacement = PageAllocator::new(initial.page_count(), 2, prior.clone());
        let root = write_tree_map(
            &mut file,
            HEADER,
            &mut replacement,
            root,
            &prior,
            vec![FreeMapExtentUpdate::Upsert(run(8, 1, 2))],
        )
        .unwrap();

        let reopened =
            read_map(&mut file, HEADER, root.unwrap(), replacement.page_count()).unwrap();
        assert_eq!(reopened, vec![run(8, 1, 2)]);
    }

    #[test]
    fn extent_update_replaces_an_overlapping_successor() {
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut initial = PageAllocator::new(100, 1, vec![]);
        let root = write_tree_map(
            &mut file,
            HEADER,
            &mut initial,
            None,
            &[],
            vec![FreeMapExtentUpdate::Upsert(run(10, 5, 1))],
        )
        .unwrap();
        let prior = read_map(&mut file, HEADER, root.unwrap(), initial.page_count()).unwrap();
        let mut replacement = PageAllocator::new(initial.page_count(), 2, prior.clone());
        let root = write_tree_map(
            &mut file,
            HEADER,
            &mut replacement,
            root,
            &prior,
            vec![FreeMapExtentUpdate::Upsert(run(8, 4, 2))],
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            read_map(&mut file, HEADER, root, replacement.page_count()).unwrap(),
            vec![run(8, 4, 2)]
        );
    }

    #[derive(Debug)]
    struct FailFirstWrite<'a> {
        file: &'a mut File,
    }

    impl BackingIo for FailFirstWrite<'_> {
        fn pread(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
            self.file.pread(off, buf)
        }

        fn pwrite(&mut self, _off: u64, _buf: &[u8]) -> std::io::Result<()> {
            Err(std::io::Error::other("injected free-map write failure"))
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
    fn prepared_extent_publication_is_node_exact_and_rejects_before_source_mutation() {
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let prior_runs = vec![run(4, 2, 1), run(20, 2, 1)];
        let mut initial = PageAllocator::new(100, 1, vec![]);
        let prior_root = write_tree_map(
            &mut file,
            HEADER,
            &mut initial,
            None,
            &[],
            prior_runs
                .iter()
                .copied()
                .map(FreeMapExtentUpdate::Upsert)
                .collect(),
        )
        .unwrap()
        .unwrap();
        let source_page_count = initial.page_count();
        let updates = vec![FreeMapExtentUpdate::Upsert(run(8, 3, 2))];
        let prepared = prepare_tree_map_publication(
            &mut file,
            HEADER,
            Some(prior_root),
            &prior_runs,
            updates.clone(),
            updates.clone(),
            source_page_count,
        )
        .unwrap();
        assert_eq!(
            prepared.demand().allocation_pages(),
            prepared.tree_allocation_calls_for_test()
        );
        let mut applying = PageAllocator::new(source_page_count, 2, Vec::new());
        let assigned = (0..prepared.demand().allocation_pages())
            .map(|_| applying.alloc(1))
            .collect::<Vec<_>>();
        let before = std::fs::read(&t.0).unwrap();
        let allocator_before = applying.clone();

        let short = &assigned[..assigned.len().saturating_sub(1)];
        assert!(
            apply_prepared_tree_map_publication(
                &mut file,
                HEADER,
                &mut applying,
                Some(prior_root),
                updates.clone(),
                prepared.clone(),
                short,
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&t.0).unwrap(), before);
        assert_eq!(applying.reuse_before, allocator_before.reuse_before);
        assert_eq!(
            applying.suppress_free_map_tracking,
            allocator_before.suppress_free_map_tracking
        );
        assert_eq!(applying.snapshot_free(), allocator_before.snapshot_free());
        assert_eq!(
            applying.pending_free_map_extent_update_count(),
            allocator_before.pending_free_map_extent_update_count()
        );
        assert_eq!(
            applying.transaction_stats(),
            allocator_before.transaction_stats()
        );
        assert!(
            apply_prepared_tree_map_publication(
                &mut file,
                HEADER,
                &mut applying,
                None,
                updates.clone(),
                prepared.clone(),
                &assigned,
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&t.0).unwrap(), before);
        assert_eq!(applying.reuse_before, allocator_before.reuse_before);
        assert_eq!(
            applying.suppress_free_map_tracking,
            allocator_before.suppress_free_map_tracking
        );
        assert_eq!(applying.snapshot_free(), allocator_before.snapshot_free());
        assert_eq!(
            applying.pending_free_map_extent_update_count(),
            allocator_before.pending_free_map_extent_update_count()
        );
        assert_eq!(
            applying.transaction_stats(),
            allocator_before.transaction_stats()
        );

        let error = {
            let mut failing = FailFirstWrite { file: &mut file };
            apply_prepared_tree_map_publication(
                &mut failing,
                HEADER,
                &mut applying,
                Some(prior_root),
                updates,
                prepared,
                &assigned,
            )
            .unwrap_err()
        };
        assert_eq!(error.code, Code::Io);
        assert_eq!(
            read_map(&mut file, HEADER, prior_root, source_page_count).unwrap(),
            prior_runs
        );
    }

    #[test]
    fn map_spanning_many_pages_round_trips() {
        let runs: Vec<FreePageRun> = (0..400).map(|i| run(i * 2, 1, i)).collect();
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut a = PageAllocator::new(1000, 9, vec![]);
        let root = write_tree_map(
            &mut file,
            HEADER,
            &mut a,
            None,
            &[],
            runs.iter()
                .copied()
                .map(FreeMapExtentUpdate::Upsert)
                .collect(),
        )
        .unwrap()
        .unwrap();
        assert!(
            collect_map_pages(&mut file, HEADER, root, a.page_count())
                .unwrap()
                .len()
                > 1
        );
        let back = read_map(&mut file, HEADER, root, a.page_count()).unwrap();
        assert_eq!(back, runs);
    }

    #[test]
    fn read_map_rejects_a_root_past_the_page_array() {
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut a = PageAllocator::new(10, 1, vec![]);
        let root = write_tree_map(
            &mut file,
            HEADER,
            &mut a,
            None,
            &[],
            vec![FreeMapExtentUpdate::Upsert(run(1, 1, 0))],
        )
        .unwrap()
        .unwrap();
        assert!(read_map(&mut file, HEADER, root, root.0).is_err()); // page_count excludes the map page
    }

    #[test]
    fn read_map_rejects_a_run_outside_the_page_array() {
        // A CRC-valid map whose run ends past page_count must be rejected, not trusted.
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut a = PageAllocator::new(50, 1, vec![]);
        let root = write_tree_map(
            &mut file,
            HEADER,
            &mut a,
            None,
            &[],
            vec![FreeMapExtentUpdate::Upsert(run(5, 100, 0))],
        )
        .unwrap()
        .unwrap();
        assert!(read_map(&mut file, HEADER, root, a.page_count()).is_err());
    }

    #[test]
    fn read_map_rejects_overlapping_runs() {
        // Two runs that overlap ([2,5) and [4,7)) would double-hand-out a page; reject them.
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut a = PageAllocator::new(100, 1, vec![]);
        let root = write_tree_map(
            &mut file,
            HEADER,
            &mut a,
            None,
            &[],
            vec![
                FreeMapExtentUpdate::Upsert(run(2, 3, 0)),
                FreeMapExtentUpdate::Upsert(run(4, 3, 0)),
            ],
        )
        .unwrap()
        .unwrap();
        assert!(read_map(&mut file, HEADER, root, a.page_count()).is_err());
    }

    #[test]
    fn read_map_rejects_a_zero_length_run() {
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut a = PageAllocator::new(100, 1, vec![]);
        assert!(
            write_tree_map(
                &mut file,
                HEADER,
                &mut a,
                None,
                &[],
                vec![FreeMapExtentUpdate::Upsert(run(3, 0, 0))],
            )
            .is_err()
        );
    }
}
