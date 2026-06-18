//! Standalone D-1 page-size analysis for the page-allocator layout.
//!
//! Production `loom-store` keeps `PAGE_SIZE` fixed at 4096 for the major-1 file format. This prototype
//! owns the tunable `LOOM_PAGE_SIZE` model used to compare candidate sizes without making production
//! builds produce incompatible files.

use loom_core::digest::Digest;
use std::collections::BTreeMap;
use std::time::Instant;

const N: usize = 40_000;
const BATCH: usize = 500;
const HEADER_BYTES: usize = 3 * 4096;
const INDEX_MAX_ENTRIES: usize = 63;
const INDEX_MAX_CHILDREN: usize = 64;
const SLAB_HEADER: usize = 3;
const SLOT_ENTRY: usize = 4;
const CRC: usize = 4;
const LARGE_HEADER: usize = 9;

#[derive(Clone, Copy)]
struct Loc {
    page: usize,
    slot: usize,
}

enum Page {
    Slab { records: Vec<Vec<u8>>, used: usize },
    Large(Vec<u8>),
    Continuation,
}

struct PageModel {
    page_size: usize,
    slab_threshold: usize,
    pages: Vec<Page>,
    open_slab: Option<usize>,
    index: BTreeMap<[u8; 32], Loc>,
}

impl PageModel {
    fn new(page_size: usize) -> Self {
        Self {
            page_size,
            slab_threshold: page_size / 4,
            pages: Vec::new(),
            open_slab: None,
            index: BTreeMap::new(),
        }
    }

    fn put_batch(&mut self, items: &[Vec<u8>]) -> Vec<Digest> {
        items.iter().map(|item| self.put(item)).collect()
    }

    fn put(&mut self, canonical: &[u8]) -> Digest {
        let digest = Digest::blake3(canonical);
        if self.index.contains_key(digest.bytes()) {
            return digest;
        }
        let record = encode_record(&digest, canonical);
        let loc = if record.len() > self.slab_threshold {
            self.write_large(record)
        } else {
            self.write_slab(record)
        };
        self.index.insert(*digest.bytes(), loc);
        digest
    }

    fn write_slab(&mut self, record: Vec<u8>) -> Loc {
        if let Some(page) = self.open_slab
            && let Some(slot) = self.try_push_slab(page, record.clone())
        {
            return Loc { page, slot };
        }
        let page = self.pages.len();
        self.pages.push(Page::Slab {
            records: Vec::new(),
            used: SLAB_HEADER + CRC,
        });
        self.open_slab = Some(page);
        let slot = self
            .try_push_slab(page, record)
            .expect("one small record fits in an empty slab");
        Loc { page, slot }
    }

    fn try_push_slab(&mut self, page: usize, record: Vec<u8>) -> Option<usize> {
        let Page::Slab { records, used } = &mut self.pages[page] else {
            return None;
        };
        let next = *used + SLOT_ENTRY + record.len();
        if next > self.page_size {
            return None;
        }
        let slot = records.len();
        records.push(record);
        *used = next;
        Some(slot)
    }

    fn write_large(&mut self, record: Vec<u8>) -> Loc {
        let pages = (LARGE_HEADER + record.len() + CRC).div_ceil(self.page_size);
        let first = self.pages.len();
        self.pages.push(Page::Large(record));
        for _ in 1..pages {
            self.pages.push(Page::Continuation);
        }
        Loc {
            page: first,
            slot: 0,
        }
    }

    fn get(&self, digest: &Digest) -> Option<&[u8]> {
        let loc = self.index.get(digest.bytes())?;
        let record = match &self.pages[loc.page] {
            Page::Slab { records, .. } => records.get(loc.slot)?,
            Page::Large(record) => record,
            Page::Continuation => return None,
        };
        decode_record(record, digest)
    }

    fn modeled_file_bytes(&self) -> usize {
        HEADER_BYTES + (self.pages.len() + btree_pages(self.index.len()) + 1) * self.page_size
    }
}

fn main() {
    let page_size = page_size();
    let mut store = PageModel::new(page_size);

    let objs: Vec<Vec<u8>> = (0..N)
        .map(|i| format!("loom-object-{i:08}-payload").into_bytes())
        .collect();

    let mut digests = Vec::with_capacity(N);
    let t = Instant::now();
    for chunk in objs.chunks(BATCH) {
        digests.extend(store.put_batch(chunk));
    }
    let put = t.elapsed();

    let t = Instant::now();
    for d in &digests {
        assert!(store.get(d).is_some());
    }
    let get = t.elapsed();

    println!(
        "{page_size}\t{:.0}\t{:.0}\t{}",
        N as f64 / put.as_secs_f64(),
        N as f64 / get.as_secs_f64(),
        store.modeled_file_bytes() / N,
    );
}

fn page_size() -> usize {
    let raw = std::env::var("LOOM_PAGE_SIZE").unwrap_or_else(|_| "4096".into());
    let page_size = raw
        .parse::<usize>()
        .expect("LOOM_PAGE_SIZE must be decimal bytes");
    assert!(
        page_size >= 4096,
        "LOOM_PAGE_SIZE must be at least 4096 because index pages hold order-64 nodes"
    );
    page_size
}

fn encode_record(digest: &Digest, canonical: &[u8]) -> Vec<u8> {
    let mut record = Vec::with_capacity(1 + 32 + 10 + canonical.len() + CRC);
    record.push(0xB0);
    record.extend_from_slice(digest.bytes());
    put_uvarint(&mut record, canonical.len() as u64);
    record.extend_from_slice(canonical);
    record.extend_from_slice(&crc32c(&record).to_le_bytes());
    record
}

fn decode_record<'a>(record: &'a [u8], digest: &Digest) -> Option<&'a [u8]> {
    if record.len() < 1 + 32 + CRC || record[0] != 0xB0 || &record[1..33] != digest.bytes() {
        return None;
    }
    let crc_at = record.len().checked_sub(CRC)?;
    let stored = u32::from_le_bytes(record[crc_at..].try_into().ok()?);
    if crc32c(&record[..crc_at]) != stored {
        return None;
    }
    let mut pos = 33;
    let len = get_uvarint(record, &mut pos)? as usize;
    let end = pos.checked_add(len)?;
    (end == crc_at).then_some(&record[pos..end])
}

fn btree_pages(entries: usize) -> usize {
    if entries == 0 {
        return 0;
    }
    let mut level = entries.div_ceil(INDEX_MAX_ENTRIES);
    let mut total = level;
    while level > 1 {
        level = level.div_ceil(INDEX_MAX_CHILDREN);
        total += level;
    }
    total
}

fn put_uvarint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

fn get_uvarint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;
    loop {
        let byte = *buf.get(*pos)?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift >= 64 {
            return None;
        }
    }
}

fn crc32c(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0x82F6_3B78 & mask);
        }
    }
    !crc
}
