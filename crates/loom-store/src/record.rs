//! Record placement on pages: the index value [`RecordLoc`], slab-packed pages for small objects, and
//! page-run encoding for large ones.
//!
//! An object's framed record (see `encode_record` in the crate root) is stored either packed into a
//! shared slab page with other small records, addressed by an intra-page slot, or, when larger than
//! [`SLAB_THRESHOLD`], in its own run of pages. Either way the index maps the digest to a
//! [`RecordLoc`]; `get` reads the bytes back and re-verifies the digest, so page placement never
//! changes the content address.

use crate::page::{PAGE_SIZE, PAGES_PER_SEGMENT};
use crate::{crc32c, get_uvarint, put_uvarint};

pub(crate) const SLAB_MAGIC: u8 = 0xB5;
pub(crate) const LARGE_MAGIC: u8 = 0xB6;

/// Objects with a framed record this size or smaller pack into a shared slab page; larger ones get
/// their own page run.
pub(crate) const SLAB_THRESHOLD: u64 = PAGE_SIZE / 4;

const SLAB_HEADER: usize = 3; // magic(1) + slot_count(2)
const SLOT_ENTRY: usize = 4; // off(2) + len(2)
const CRC: usize = 4;
const PAGE: usize = PAGE_SIZE as usize;
const LARGE_HEADER: usize = 9; // magic(1) + blob_len(8)

/// A record's location: the segment, the page index within that segment, and the intra-page slot
/// (always 0 for a large page run). Stored in the index as three uvarints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecordLoc {
    pub(crate) segment_id: u64,
    pub(crate) page_index: u64,
    pub(crate) slot: u32,
}

impl RecordLoc {
    /// Build a locator from a global page index and intra-page slot.
    pub(crate) fn from_global(global_page: u64, slot: u32) -> Self {
        Self {
            segment_id: global_page / PAGES_PER_SEGMENT,
            page_index: global_page % PAGES_PER_SEGMENT,
            slot,
        }
    }

    /// The global page index this locator addresses.
    pub(crate) fn global_page(self) -> u64 {
        self.segment_id * PAGES_PER_SEGMENT + self.page_index
    }

    pub(crate) fn encode(self, out: &mut Vec<u8>) {
        put_uvarint(out, self.segment_id);
        put_uvarint(out, self.page_index);
        put_uvarint(out, u64::from(self.slot));
    }

    /// Decode a locator from `buf` at `*pos`, advancing past it. `None` on truncation or a slot that
    /// does not fit in `u32`.
    pub(crate) fn decode(buf: &[u8], pos: &mut usize) -> Option<Self> {
        let segment_id = get_uvarint(buf, pos)?;
        let page_index = get_uvarint(buf, pos)?;
        let slot = u32::try_from(get_uvarint(buf, pos)?).ok()?;
        Some(Self {
            segment_id,
            page_index,
            slot,
        })
    }
}

/// Whether a framed record of `len` bytes goes in its own page run rather than a slab page.
pub(crate) fn is_large(len: u64) -> bool {
    len > SLAB_THRESHOLD
}

/// Accumulates small framed records into one slab page until the page is full.
#[derive(Default)]
pub(crate) struct SlabBuilder {
    blobs: Vec<Vec<u8>>,
    used: usize,
}

impl SlabBuilder {
    pub(crate) fn new() -> Self {
        Self {
            blobs: Vec::new(),
            used: SLAB_HEADER + CRC,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }

    /// Append `blob`, returning its slot index, or `None` if it would overflow the page (the builder
    /// is left unchanged so the caller can flush this page and start a new one).
    pub(crate) fn try_push(&mut self, blob: &[u8]) -> Option<u32> {
        let add = SLOT_ENTRY + blob.len();
        if self.used + add > PAGE {
            return None;
        }
        let slot = self.blobs.len() as u32;
        self.blobs.push(blob.to_vec());
        self.used += add;
        Some(slot)
    }

    /// Lay the accumulated records out into a full page: header, slot directory, packed blobs, CRC.
    pub(crate) fn finish(&self) -> [u8; PAGE] {
        let mut page = [0u8; PAGE];
        page[0] = SLAB_MAGIC;
        page[1..3].copy_from_slice(&(self.blobs.len() as u16).to_le_bytes());
        let mut data_off = SLAB_HEADER + self.blobs.len() * SLOT_ENTRY;
        for (i, blob) in self.blobs.iter().enumerate() {
            let e = SLAB_HEADER + i * SLOT_ENTRY;
            page[e..e + 2].copy_from_slice(&(data_off as u16).to_le_bytes());
            page[e + 2..e + 4].copy_from_slice(&(blob.len() as u16).to_le_bytes());
            page[data_off..data_off + blob.len()].copy_from_slice(blob);
            data_off += blob.len();
        }
        let crc = crc32c(&page[..PAGE - CRC]);
        page[PAGE - CRC..].copy_from_slice(&crc.to_le_bytes());
        page
    }
}

/// Borrow the framed record in `slot` of a slab `page`, or `None` on bad magic, CRC mismatch, an
/// out-of-range slot, or a directory entry pointing outside the page body.
pub(crate) fn read_slab_slot(page: &[u8], slot: u32) -> Option<&[u8]> {
    if page.len() < PAGE || page[0] != SLAB_MAGIC {
        return None;
    }
    let stored = u32::from_le_bytes(page[PAGE - CRC..PAGE].try_into().ok()?);
    if crc32c(&page[..PAGE - CRC]) != stored {
        return None;
    }
    let n = u16::from_le_bytes(page[1..3].try_into().ok()?) as usize;
    let i = slot as usize;
    if i >= n {
        return None;
    }
    let e = SLAB_HEADER + i * SLOT_ENTRY;
    let off = u16::from_le_bytes(page[e..e + 2].try_into().ok()?) as usize;
    let len = u16::from_le_bytes(page[e + 2..e + 4].try_into().ok()?) as usize;
    let end = off.checked_add(len)?;
    if off < SLAB_HEADER + n * SLOT_ENTRY || end > PAGE - CRC {
        return None;
    }
    Some(&page[off..end])
}

/// Pages a large-object run of `blob_len` framed bytes occupies.
pub(crate) fn large_pages(blob_len: u64) -> u64 {
    (LARGE_HEADER as u64 + blob_len + CRC as u64).div_ceil(PAGE_SIZE)
}

/// Encode a large framed record as a page-run blob (header, bytes, CRC), zero-padded to a page edge.
pub(crate) fn encode_large(blob: &[u8]) -> Vec<u8> {
    let pages = large_pages(blob.len() as u64);
    let mut buf = vec![0u8; (pages * PAGE_SIZE) as usize];
    buf[0] = LARGE_MAGIC;
    buf[1..9].copy_from_slice(&(blob.len() as u64).to_le_bytes());
    let end = LARGE_HEADER + blob.len();
    buf[LARGE_HEADER..end].copy_from_slice(blob);
    let crc = crc32c(&buf[..end]);
    buf[end..end + CRC].copy_from_slice(&crc.to_le_bytes());
    buf
}

/// The framed-record length declared in a large run's header (its first [`LARGE_HEADER`] bytes), or
/// `None` on bad magic. The caller uses it to size the full read before [`decode_large`].
pub(crate) fn large_blob_len(head: &[u8]) -> Option<u64> {
    if head.len() < LARGE_HEADER || head[0] != LARGE_MAGIC {
        return None;
    }
    Some(u64::from_le_bytes(head[1..9].try_into().ok()?))
}

/// Borrow the framed record from a fully-read large run, or `None` on bad magic, short buffer, or CRC.
pub(crate) fn decode_large(buf: &[u8]) -> Option<&[u8]> {
    let len = large_blob_len(buf)? as usize;
    let end = LARGE_HEADER + len;
    if buf.len() < end + CRC {
        return None;
    }
    let stored = u32::from_le_bytes(buf[end..end + CRC].try_into().ok()?);
    if crc32c(&buf[..end]) != stored {
        return None;
    }
    Some(&buf[LARGE_HEADER..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locator_round_trips_and_maps_to_global_page() {
        let g = 3 * PAGES_PER_SEGMENT + 17;
        let loc = RecordLoc::from_global(g, 5);
        assert_eq!(loc.segment_id, 3);
        assert_eq!(loc.page_index, 17);
        assert_eq!(loc.global_page(), g);

        let mut buf = Vec::new();
        loc.encode(&mut buf);
        let mut pos = 0;
        assert_eq!(RecordLoc::decode(&buf, &mut pos).unwrap(), loc);
        assert_eq!(pos, buf.len());
    }

    #[test]
    fn locator_decode_rejects_truncation() {
        let mut buf = Vec::new();
        RecordLoc::from_global(1_000_000, 9).encode(&mut buf);
        let mut pos = 0;
        assert!(RecordLoc::decode(&buf[..buf.len() - 1], &mut pos).is_none());
    }

    #[test]
    fn slab_packs_many_small_records_and_reads_each_back() {
        let mut b = SlabBuilder::new();
        let blobs: Vec<Vec<u8>> = (0..20u8).map(|i| vec![i; 30 + i as usize]).collect();
        let mut slots = Vec::new();
        for blob in &blobs {
            slots.push(b.try_push(blob).expect("small records fit"));
        }
        let page = b.finish();
        for (slot, blob) in slots.iter().zip(&blobs) {
            assert_eq!(read_slab_slot(&page, *slot).unwrap(), blob.as_slice());
        }
        assert!(read_slab_slot(&page, blobs.len() as u32).is_none()); // out of range
    }

    #[test]
    fn slab_reports_full_without_mutating() {
        let mut b = SlabBuilder::new();
        let big = vec![7u8; PAGE - 100];
        assert_eq!(b.try_push(&big), Some(0));
        let another = vec![9u8; PAGE - 100];
        assert_eq!(b.try_push(&another), None); // would overflow
        let page = b.finish();
        assert_eq!(read_slab_slot(&page, 0).unwrap(), big.as_slice());
        assert!(read_slab_slot(&page, 1).is_none());
    }

    #[test]
    fn slab_crc_catches_a_flipped_bit() {
        let mut b = SlabBuilder::new();
        b.try_push(&[1, 2, 3, 4]).unwrap();
        let mut page = b.finish();
        page[2] ^= 0xFF;
        assert!(read_slab_slot(&page, 0).is_none());
    }

    #[test]
    fn large_run_round_trips_across_pages() {
        let blob = vec![0xABu8; 10_000]; // > one page
        let buf = encode_large(&blob);
        assert_eq!(buf.len() as u64 % PAGE_SIZE, 0);
        assert_eq!(buf.len() as u64, large_pages(blob.len() as u64) * PAGE_SIZE);
        assert_eq!(large_blob_len(&buf).unwrap(), blob.len() as u64);
        assert_eq!(decode_large(&buf).unwrap(), blob.as_slice());
    }

    #[test]
    fn large_crc_catches_a_flipped_bit() {
        let blob = vec![5u8; 5000];
        let mut buf = encode_large(&blob);
        buf[20] ^= 0xFF;
        assert!(decode_large(&buf).is_none());
    }

    #[test]
    fn threshold_routes_small_to_slab_and_big_to_run() {
        assert!(!is_large(SLAB_THRESHOLD));
        assert!(is_large(SLAB_THRESHOLD + 1));
    }
}
