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

use crate::page::{PAGE_SIZE, PageId};
use crate::{BackingIo, REUSE_SAFE_WINDOW, corrupt, crc32c, io_err, read_exact_at, write_at};
use loom_core::error::Result;
use std::collections::BTreeMap;
#[cfg(test)]
use std::fs::File;

const MAP_MAGIC: u8 = 0xB4;

/// A run of contiguous free pages, tagged with the generation that freed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FreePageRun {
    pub(crate) start: u64,
    pub(crate) len: u64,
    pub(crate) freed_gen: u64,
}

/// Hands out page-runs for one transaction. Reuses pages freed earlier in this transaction (extended
/// past the prior committed page count, so reusing them is crash-safe) and prior-generation runs aged
/// past the recoverable window, before extending the array. Collects the runs this transaction frees
/// so they enter the free-page map on commit.
pub(crate) struct PageAllocator {
    end: u64,
    start_end: u64, // page count at the start of this transaction
    txn_gen: u64,
    reuse_current_free: bool,
    reuse_before: Option<u64>,
    free: BTreeMap<u64, (u64, u64)>, // prior-generation free runs: start -> (len, freed_gen)
    txn_freed: BTreeMap<u64, u64>,   // runs freed this transaction: start -> len
}

impl PageAllocator {
    pub(crate) fn new(page_count: u64, txn_gen: u64, free: Vec<FreePageRun>) -> Self {
        let free = free
            .into_iter()
            .map(|r| (r.start, (r.len, r.freed_gen)))
            .collect();
        Self {
            end: page_count,
            start_end: page_count,
            txn_gen,
            reuse_current_free: false,
            reuse_before: None,
            free,
            txn_freed: BTreeMap::new(),
        }
    }

    pub(crate) fn new_with_current_free_reusable(
        page_count: u64,
        txn_gen: u64,
        free: Vec<FreePageRun>,
    ) -> Self {
        let mut allocator = Self::new(page_count, txn_gen, free);
        allocator.reuse_current_free = true;
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

    /// Reserve a run of `n` pages and return its first page: reuse a run freed earlier in this
    /// transaction that was extended within it, then a prior-generation run aged past the window
    /// (splitting any remainder back), and extend the array otherwise.
    pub(crate) fn alloc(&mut self, n: u64) -> PageId {
        if let Some(start) = self.take_txn_freed(n) {
            return PageId(start);
        }
        if let Some(start) = self.take_aged(n) {
            return PageId(start);
        }
        self.extend(n)
    }

    /// Take a run of `n` pages from those freed this transaction, but only one extended within this
    /// transaction (`start >= start_end`): a crash before commit reverts to the prior generation,
    /// which never referenced those pages, so overwriting them now is safe.
    fn take_txn_freed(&mut self, n: u64) -> Option<u64> {
        let start = self
            .txn_freed
            .iter()
            .find_map(|(s, len)| (*s >= self.start_end && *len >= n).then_some(*s))?;
        let len = self.txn_freed.remove(&start).unwrap_or(0);
        if len > n {
            self.txn_freed.insert(start + n, len - n);
        }
        Some(start)
    }

    /// Take a run of `n` pages from a prior generation that is now outside the recoverable window.
    fn take_aged(&mut self, n: u64) -> Option<u64> {
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
        if len > n {
            self.free.insert(start + n, (len - n, g));
        }
        Some(start)
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
    pub(crate) fn free(&mut self, start: PageId, n: u64) {
        self.txn_freed.insert(start.0, n);
    }

    /// Total pages the array spans: every page handed out so far lies below this.
    pub(crate) fn page_count(&self) -> u64 {
        self.end
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
        v
    }
}

/// Bytes a free-page-map blob with `count` runs occupies: `magic(1) count(4) count*{start,len,
/// freed_gen}(24) crc(4)`.
fn byte_len(count: usize) -> u64 {
    (1 + 4 + count * 24 + 4) as u64
}

fn pages_for(bytes: u64) -> u64 {
    bytes.div_ceil(PAGE_SIZE)
}

/// Pages a free-page map of `run_count` runs occupies on disk. Lets a commit free the prior map's
/// page-run before writing the new one.
pub(crate) fn map_pages(run_count: usize) -> u64 {
    pages_for(byte_len(run_count))
}

/// Encode the free run list into a self-delimiting, CRC'd blob.
pub(crate) fn encode(runs: &[FreePageRun]) -> Vec<u8> {
    let mut b = Vec::with_capacity(byte_len(runs.len()) as usize);
    b.push(MAP_MAGIC);
    b.extend_from_slice(&(runs.len() as u32).to_le_bytes());
    for r in runs {
        b.extend_from_slice(&r.start.to_le_bytes());
        b.extend_from_slice(&r.len.to_le_bytes());
        b.extend_from_slice(&r.freed_gen.to_le_bytes());
    }
    let crc = crc32c(&b);
    b.extend_from_slice(&crc.to_le_bytes());
    b
}

/// Decode a free-page-map blob, or `None` on bad magic, short buffer, or CRC mismatch.
pub(crate) fn decode(buf: &[u8]) -> Option<Vec<FreePageRun>> {
    if buf.len() < 9 || buf[0] != MAP_MAGIC {
        return None;
    }
    let count = u32::from_le_bytes(buf[1..5].try_into().ok()?) as usize;
    let total = byte_len(count) as usize;
    if buf.len() < total {
        return None;
    }
    let stored = u32::from_le_bytes(buf[total - 4..total].try_into().ok()?);
    if crc32c(&buf[..total - 4]) != stored {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    let mut p = 5;
    for _ in 0..count {
        let start = u64::from_le_bytes(buf[p..p + 8].try_into().ok()?);
        let len = u64::from_le_bytes(buf[p + 8..p + 16].try_into().ok()?);
        let freed_gen = u64::from_le_bytes(buf[p + 16..p + 24].try_into().ok()?);
        out.push(FreePageRun {
            start,
            len,
            freed_gen,
        });
        p += 24;
    }
    Some(out)
}

/// Write the free run list into the pre-allocated `reserved_pages`-page run starting at `root`,
/// zero-padding to the page boundary. `reserved_pages` must be at least `map_pages(runs.len())`; the
/// caller reserves the run (so the map's own pages can be carved out of the set it persists).
pub(crate) fn write_map_at(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    reserved_pages: u64,
    runs: &[FreePageRun],
) -> Result<()> {
    let bytes = encode(runs);
    debug_assert!(pages_for(bytes.len() as u64) <= reserved_pages);
    let mut buf = vec![0u8; (reserved_pages * PAGE_SIZE) as usize];
    buf[..bytes.len()].copy_from_slice(&bytes);
    write_at(file, root.offset(header_len), &buf).map_err(io_err)?;
    Ok(())
}

/// Read and decode the free-page map rooted at `root`. Reads are bounded by `page_count`, so a
/// crafted root or run count is a clean CORRUPT error.
pub(crate) fn read_map(
    file: &mut dyn BackingIo,
    header_len: u64,
    root: PageId,
    page_count: u64,
) -> Result<Vec<FreePageRun>> {
    let mut hdr = [0u8; 5];
    read_exact_at(file, root.offset(header_len), &mut hdr).map_err(io_err)?;
    if hdr[0] != MAP_MAGIC {
        return Err(corrupt("bad free-page-map magic"));
    }
    let count = u32::from_le_bytes(hdr[1..5].try_into().unwrap_or([0; 4])) as usize;
    let total = byte_len(count);
    if root.0 + pages_for(total) > page_count {
        return Err(corrupt("free-page map extends past allocated pages"));
    }
    let mut buf = vec![0u8; total as usize];
    read_exact_at(file, root.offset(header_len), &mut buf).map_err(io_err)?;
    let mut runs = decode(&buf).ok_or_else(|| corrupt("free-page map crc/parse failure"))?;
    // A valid CRC only proves the bytes are intact, not that the runs are meaningful. Validate them
    // before the allocator trusts them: each run must be non-empty, lie within the committed page
    // array, not overflow, and not overlap another. A run outside the array would let the allocator
    // hand out a page the committed file does not cover, corrupting the next commit.
    runs.sort_by_key(|r| r.start);
    let mut prev_end = 0u64;
    for r in &runs {
        let end = r
            .start
            .checked_add(r.len)
            .ok_or_else(|| corrupt("free-page run overflows"))?;
        if r.len == 0 || end > page_count || r.start < prev_end {
            return Err(corrupt("free-page run out of range or overlapping"));
        }
        prev_end = end;
    }
    Ok(runs)
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
    fn multi_page_alloc_needs_one_contiguous_run() {
        // Two small runs cannot satisfy a 4-page request; the allocator extends.
        let mut a = PageAllocator::new(30, 100, vec![run(2, 2, 1), run(6, 2, 1)]);
        assert_eq!(a.alloc(4), PageId(30));
    }

    #[test]
    fn snapshot_carries_existing_and_freshly_freed_runs() {
        let mut a = PageAllocator::new(40, 7, vec![run(3, 1, 2)]);
        a.free(PageId(12), 4);
        let mut snap = a.snapshot_free();
        snap.sort_by_key(|r| r.start);
        assert_eq!(snap, vec![run(3, 1, 2), run(12, 4, 7)]);
    }

    #[test]
    fn blob_round_trips_and_detects_corruption() {
        let runs = vec![run(4, 2, 3), run(20, 5, 7)];
        let bytes = encode(&runs);
        assert_eq!(bytes.len() as u64, byte_len(runs.len()));
        assert_eq!(decode(&bytes).unwrap(), runs);

        let mut torn = bytes.clone();
        torn[8] ^= 0xFF;
        assert!(decode(&torn).is_none());

        assert_eq!(decode(&encode(&[])).unwrap(), Vec::<FreePageRun>::new());
        assert!(decode(&[0u8; 9]).is_none());
    }

    #[test]
    fn map_survives_a_reopen() {
        let runs = vec![run(4, 2, 1), run(9, 3, 2), run(100, 1, 3)];
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut a = PageAllocator::new(200, 5, vec![]);
        let pages = map_pages(runs.len());
        let root = a.extend(pages);
        write_map_at(&mut file, HEADER, root, pages, &runs).unwrap();
        drop(file);

        let mut reopened = OpenOptions::new().read(true).open(&t.0).unwrap();
        let back = read_map(&mut reopened, HEADER, root, a.page_count()).unwrap();
        assert_eq!(back, runs);
    }

    #[test]
    fn map_spanning_many_pages_round_trips() {
        let runs: Vec<FreePageRun> = (0..400).map(|i| run(i * 2, 1, i)).collect();
        assert!(byte_len(runs.len()) > PAGE_SIZE); // forces a multi-page map
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut a = PageAllocator::new(1000, 9, vec![]);
        let pages = map_pages(runs.len());
        let root = a.extend(pages);
        write_map_at(&mut file, HEADER, root, pages, &runs).unwrap();
        let back = read_map(&mut file, HEADER, root, a.page_count()).unwrap();
        assert_eq!(back, runs);
    }

    #[test]
    fn read_map_rejects_a_root_past_the_page_array() {
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut a = PageAllocator::new(10, 1, vec![]);
        let pages = map_pages(1);
        let root = a.extend(pages);
        write_map_at(&mut file, HEADER, root, pages, &[run(1, 1, 0)]).unwrap();
        assert!(read_map(&mut file, HEADER, root, root.0).is_err()); // page_count excludes the map page
    }

    #[test]
    fn read_map_rejects_a_run_outside_the_page_array() {
        // A CRC-valid map whose run ends past page_count must be rejected, not trusted.
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut a = PageAllocator::new(50, 1, vec![]);
        let pages = map_pages(1);
        let root = a.extend(pages);
        write_map_at(&mut file, HEADER, root, pages, &[run(5, 100, 0)]).unwrap();
        assert!(read_map(&mut file, HEADER, root, a.page_count()).is_err());
    }

    #[test]
    fn read_map_rejects_overlapping_runs() {
        // Two runs that overlap ([2,5) and [4,7)) would double-hand-out a page; reject them.
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut a = PageAllocator::new(100, 1, vec![]);
        let pages = map_pages(2);
        let root = a.extend(pages);
        write_map_at(
            &mut file,
            HEADER,
            root,
            pages,
            &[run(2, 3, 0), run(4, 3, 0)],
        )
        .unwrap();
        assert!(read_map(&mut file, HEADER, root, a.page_count()).is_err());
    }

    #[test]
    fn read_map_rejects_a_zero_length_run() {
        let t = temp();
        let mut file = t.1.try_clone().unwrap();
        let mut a = PageAllocator::new(100, 1, vec![]);
        let pages = map_pages(1);
        let root = a.extend(pages);
        write_map_at(&mut file, HEADER, root, pages, &[run(3, 0, 0)]).unwrap();
        assert!(read_map(&mut file, HEADER, root, a.page_count()).is_err());
    }
}
