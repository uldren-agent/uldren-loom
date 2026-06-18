//! Page addressing and the region table.
//!
//! The file is a fixed header followed by an array of `PAGE_SIZE`-byte pages; a [`PageId`] names one
//! page by its zero-based index in that array. The region table is the one page the superblock and
//! journal record point at: it carries the root page of each page-structured region (the object index
//! and the free-page map) plus the open segment and page size, so a single pointer locates the engine
//! state. The engine-state (reference) root is a content digest, not a page, so it rides in the
//! superblock and journal record directly rather than here.

use crate::crc32c;

/// Size in bytes of one page. Locked at 4 KiB for the major-1 file layout by the D-1 benchmark in
/// `prototypes/page-store`: smaller pages cannot hold an index node, while larger pages waste slab
/// space and amplify reads and writes on small-object workloads.
pub(crate) const PAGE_SIZE: u64 = 4096;

/// Target bytes per segment: a logical group of record pages tracked for garbage collection.
pub(crate) const SEGMENT_BYTES: u64 = 64 * 1024 * 1024;

/// Record pages per segment. A record's segment id is its global page index divided by this; its
/// in-segment page index is the remainder.
pub(crate) const PAGES_PER_SEGMENT: u64 = SEGMENT_BYTES / PAGE_SIZE;

/// On-disk size of an encoded region table: `magic(1) page_size(8) 3*root{flag(1) id(8)}
/// open_segment(8) crc32c(4)`.
pub(crate) const REGION_TABLE_LEN: usize = 1 + 8 + 3 * 9 + 8 + 4;

const _: () = assert!(REGION_TABLE_LEN as u64 <= PAGE_SIZE);

const REGION_TABLE_MAGIC_V2: u8 = 0xB6;

/// A page's zero-based index in the file's page array.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PageId(pub(crate) u64);

impl PageId {
    /// Byte offset of this page's first byte. `header_len` is the size of the fixed header that
    /// precedes the page array.
    pub(crate) fn offset(self, header_len: u64) -> u64 {
        header_len + self.0 * PAGE_SIZE
    }
}

/// Roots and accounting for the page-structured regions, held on one page. The region table's own
/// page id is the single region pointer the superblock and journal record carry. A `None` root means
/// that region has no page yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RegionTable {
    pub(crate) page_size: u64,
    pub(crate) index_root: Option<PageId>,
    pub(crate) freemap_root: Option<PageId>,
    pub(crate) maintenance_root: Option<PageId>,
    pub(crate) open_segment: u64,
}

impl RegionTable {
    /// Encode into a fixed-size, CRC'd blob suitable for writing into the region-table page.
    pub(crate) fn encode(&self) -> [u8; REGION_TABLE_LEN] {
        let mut r = [0u8; REGION_TABLE_LEN];
        r[0] = REGION_TABLE_MAGIC_V2;
        r[1..9].copy_from_slice(&self.page_size.to_le_bytes());
        let mut p = 9;
        for root in [self.index_root, self.freemap_root, self.maintenance_root] {
            if let Some(PageId(id)) = root {
                r[p] = 1;
                r[p + 1..p + 9].copy_from_slice(&id.to_le_bytes());
            }
            p += 9;
        }
        r[p..p + 8].copy_from_slice(&self.open_segment.to_le_bytes());
        let crc = crc32c(&r[..REGION_TABLE_LEN - 4]);
        r[REGION_TABLE_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
        r
    }

    /// Decode a region table, or `None` on short buffer, bad magic, bad presence byte, or CRC mismatch.
    pub(crate) fn decode(buf: &[u8]) -> Option<RegionTable> {
        if buf.first().copied()? != REGION_TABLE_MAGIC_V2 || buf.len() < REGION_TABLE_LEN {
            return None;
        }
        let stored = u32::from_le_bytes(
            buf[REGION_TABLE_LEN - 4..REGION_TABLE_LEN]
                .try_into()
                .ok()?,
        );
        if crc32c(&buf[..REGION_TABLE_LEN - 4]) != stored {
            return None;
        }
        let page_size = u64::from_le_bytes(buf[1..9].try_into().ok()?);
        let mut roots = [None; 3];
        let mut p = 9;
        for slot in &mut roots {
            *slot = match buf[p] {
                0 => None,
                1 => Some(PageId(u64::from_le_bytes(
                    buf[p + 1..p + 9].try_into().ok()?,
                ))),
                _ => return None,
            };
            p += 9;
        }
        let open_segment = u64::from_le_bytes(buf[p..p + 8].try_into().ok()?);
        Some(RegionTable {
            page_size,
            index_root: roots[0],
            freemap_root: roots[1],
            maintenance_root: roots[2],
            open_segment,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RegionTable {
        RegionTable {
            page_size: PAGE_SIZE,
            index_root: Some(PageId(7)),
            freemap_root: None,
            maintenance_root: Some(PageId(11)),
            open_segment: 3,
        }
    }

    #[test]
    fn offset_is_header_plus_index_times_page_size() {
        assert_eq!(PageId(0).offset(DATA_HEADER), DATA_HEADER);
        assert_eq!(PageId(5).offset(DATA_HEADER), DATA_HEADER + 5 * PAGE_SIZE);
    }

    const DATA_HEADER: u64 = 3 * 4096;

    #[test]
    fn round_trips_with_mixed_and_empty_roots() {
        for table in [
            sample(),
            RegionTable {
                page_size: PAGE_SIZE,
                index_root: None,
                freemap_root: None,
                maintenance_root: None,
                open_segment: 0,
            },
            RegionTable {
                freemap_root: Some(PageId(9)),
                ..sample()
            },
        ] {
            let bytes = table.encode();
            assert_eq!(bytes.len(), REGION_TABLE_LEN);
            assert_eq!(RegionTable::decode(&bytes).unwrap(), table);
        }
    }

    #[test]
    fn crc_catches_a_flipped_bit() {
        let mut bytes = sample().encode();
        bytes[5] ^= 0xFF;
        assert!(RegionTable::decode(&bytes).is_none());
    }

    #[test]
    fn rejects_bad_magic_short_buffer_and_bad_presence_byte() {
        assert!(RegionTable::decode(&[0u8; REGION_TABLE_LEN]).is_none()); // bad magic
        assert!(RegionTable::decode(&[]).is_none()); // short
        assert!(RegionTable::decode(&sample().encode()[..REGION_TABLE_LEN - 1]).is_none()); // truncated

        let mut bytes = sample().encode(); // presence byte of the first root set to 2
        bytes[9] = 2;
        let crc = crc32c(&bytes[..REGION_TABLE_LEN - 4]);
        bytes[REGION_TABLE_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
        assert!(RegionTable::decode(&bytes).is_none());
    }

    #[test]
    fn rejects_legacy_two_root_region_table_without_maintenance_root() {
        let legacy_len = 1 + 8 + 2 * 9 + 8 + 4;
        let mut bytes = vec![0u8; legacy_len];
        bytes[0] = 0xB3;
        bytes[1..9].copy_from_slice(&PAGE_SIZE.to_le_bytes());
        bytes[9] = 1;
        bytes[10..18].copy_from_slice(&7u64.to_le_bytes());
        bytes[27..35].copy_from_slice(&3u64.to_le_bytes());
        let crc = crc32c(&bytes[..legacy_len - 4]);
        bytes[legacy_len - 4..].copy_from_slice(&crc.to_le_bytes());

        assert!(RegionTable::decode(&bytes).is_none());
    }
}
