//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

pub(crate) const CONTROL_MAP_MAGIC: &[u8; 8] = b"LCTLKV1\0";

pub(crate) fn encode_control_map(map: &BTreeMap<Vec<u8>, Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(CONTROL_MAP_MAGIC);
    put_uvarint(&mut out, map.len() as u64);
    for (key, value) in map {
        put_uvarint(&mut out, key.len() as u64);
        put_uvarint(&mut out, value.len() as u64);
        out.extend_from_slice(key);
        out.extend_from_slice(value);
    }
    out
}

pub(crate) fn decode_control_map(bytes: &[u8]) -> Result<BTreeMap<Vec<u8>, Vec<u8>>> {
    if bytes.len() < CONTROL_MAP_MAGIC.len()
        || &bytes[..CONTROL_MAP_MAGIC.len()] != CONTROL_MAP_MAGIC
    {
        return Err(corrupt("bad control-plane map magic"));
    }
    let mut pos = CONTROL_MAP_MAGIC.len();
    let count = get_uvarint(bytes, &mut pos).ok_or_else(|| corrupt("control-plane map count"))?;
    let mut out = BTreeMap::new();
    let mut prev: Option<Vec<u8>> = None;
    for _ in 0..count {
        let key_len =
            get_uvarint(bytes, &mut pos).ok_or_else(|| corrupt("control-plane map key length"))?;
        let value_len = get_uvarint(bytes, &mut pos)
            .ok_or_else(|| corrupt("control-plane map value length"))?;
        let key_end = pos
            .checked_add(key_len as usize)
            .ok_or_else(|| corrupt("control-plane map key length overflow"))?;
        let value_end = key_end
            .checked_add(value_len as usize)
            .ok_or_else(|| corrupt("control-plane map value length overflow"))?;
        if value_end > bytes.len() {
            return Err(corrupt("control-plane map entry truncated"));
        }
        let key = bytes[pos..key_end].to_vec();
        if prev.as_ref().is_some_and(|p| p >= &key) {
            return Err(corrupt("control-plane map keys out of order"));
        }
        let value = bytes[key_end..value_end].to_vec();
        pos = value_end;
        prev = Some(key.clone());
        out.insert(key, value);
    }
    if pos != bytes.len() {
        return Err(corrupt("control-plane map trailing bytes"));
    }
    Ok(out)
}

pub(crate) fn lock_control_key(prefix: &[u8], key: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + key.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(key);
    out
}

pub(crate) fn decode_lock_fence_value(value: &[u8]) -> Result<u64> {
    let bytes: [u8; 8] = value
        .try_into()
        .map_err(|_| corrupt("lock fence value must be 8 bytes"))?;
    Ok(u64::from_be_bytes(bytes))
}

/// Build one object record: `[REC_MAGIC][digest(32)][frame:u8][uvarint plain_len][uvarint stored_len]
/// [stored bytes][crc32c]`. `digest` is over the plaintext `plain`; the stored bytes are `plain`
/// transformed by `codec` (subject to the size and shrink guardrails). When `enc` is `Some`, the
/// transformed bytes are then AEAD-sealed (frame id `0x10`-`0x12`, a fresh per-object nonce), so an
/// unlocked encrypted store **never** writes a plaintext object frame; the digest is still over the
/// plaintext, so encryption is invisible above `get` and preserves object identity.
pub(crate) fn encode_record(
    digest: &Digest,
    plain: &[u8],
    codec: Codec,
    enc: Option<&DekSession>,
) -> Result<Vec<u8>> {
    let (mut frame_id, mut stored) = frame::encode_payload(codec, plain);
    if let Some(session) = enc {
        let nonce = fresh_nonce(session.active_suite().nonce_len())?;
        let (aead_frame_id, sealed) = frame::seal_aead_frame(
            frame_id,
            &stored,
            session,
            digest,
            plain.len() as u64,
            &nonce,
        )?;
        frame_id = aead_frame_id;
        stored = sealed;
    }
    let mut rec = Vec::with_capacity(1 + 32 + 1 + 10 + 10 + stored.len() + 4);
    rec.push(REC_MAGIC);
    rec.extend_from_slice(digest.bytes());
    rec.push(frame_id);
    put_uvarint(&mut rec, plain.len() as u64);
    put_uvarint(&mut rec, stored.len() as u64);
    rec.extend_from_slice(&stored);
    let crc = crc32c(&rec);
    rec.extend_from_slice(&crc.to_le_bytes());
    Ok(rec)
}

/// A fresh AEAD nonce of `len` bytes from the OS CSPRNG. Each sealed object frame gets its own nonce;
/// combined with the per-object CEK, this keeps (key, nonce) pairs unique even under the 96-bit AES-GCM
/// nonce, which is the size at which random-nonce reuse would otherwise become a concern.
pub(crate) fn fresh_nonce(len: usize) -> Result<Vec<u8>> {
    let mut nonce = vec![0u8; len];
    getrandom::fill(&mut nonce).map_err(|e| {
        LoomError::new(Code::Internal, format!("loom-store: nonce RNG failed: {e}"))
    })?;
    Ok(nonce)
}

/// Write `fresh`'s framed records onto freshly allocated record pages and return each object's
/// locator. Small records pack into shared slab pages; records over the slab threshold take their own
/// page run. Committed pages are immutable, so each commit's small records share pages only with each
/// other.
pub(crate) fn write_record_pages(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    fresh: &[(Digest, &[u8], Codec)],
    enc: Option<&DekSession>,
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    let mut placements = Vec::with_capacity(fresh.len());
    let mut slab = SlabBuilder::new();
    let mut pending: Vec<([u8; 32], u32)> = Vec::new();
    for (digest, canonical, codec) in fresh {
        let rec = encode_record(digest, canonical, *codec, enc)?;
        if record::is_large(rec.len() as u64) {
            let buf = record::encode_large(&rec);
            let page = alloc.alloc(record::large_pages(rec.len() as u64));
            write_at(file, page.offset(DATA_START), &buf).map_err(io_err)?;
            placements.push((*digest.bytes(), RecordLoc::from_global(page.0, 0)));
        } else {
            let slot = match slab.try_push(&rec) {
                Some(slot) => slot,
                None => {
                    flush_slab(file, alloc, &slab, &pending, &mut placements)?;
                    slab = SlabBuilder::new();
                    pending.clear();
                    slab.try_push(&rec)
                        .expect("a fresh slab page holds one small record")
                }
            };
            pending.push((*digest.bytes(), slot));
        }
    }
    if !slab.is_empty() {
        flush_slab(file, alloc, &slab, &pending, &mut placements)?;
    }
    Ok(placements)
}

/// Allocate a page for `slab`, write it, and record a locator for every record it packed.
pub(crate) fn flush_slab(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    slab: &SlabBuilder,
    pending: &[([u8; 32], u32)],
    placements: &mut Vec<([u8; 32], RecordLoc)>,
) -> Result<()> {
    let page = alloc.alloc(1);
    write_at(file, page.offset(DATA_START), &slab.finish()).map_err(io_err)?;
    for (digest, slot) in pending {
        placements.push((*digest, RecordLoc::from_global(page.0, *slot)));
    }
    Ok(())
}

/// Parse a framed record (as written by [`encode_record`]) read back from a page, verify its CRC,
/// decrypt/decompress, and verify that the recovered plaintext hashes to `digest`. The digest check
/// runs *after* decrypt-then-decompress, so a tampered ciphertext fails AEAD authentication before any
/// plaintext is produced, and a substituted plaintext fails the content-address check. `dek` supplies
/// the unlocked key for AEAD frames; an encrypted frame with no session returns `E2eLocked`.
pub(crate) fn decode_record(
    rec: &[u8],
    digest: &Digest,
    dek: Option<&DekSession>,
    store_algo: Algo,
) -> Result<Vec<u8>> {
    if rec.len() < 34 || rec[0] != REC_MAGIC {
        return Err(corrupt("bad record magic on read"));
    }
    let frame_id = rec[33];
    let mut pos = 34;
    let plain_len = get_uvarint(rec, &mut pos).ok_or_else(|| corrupt("record plain_len varint"))?;
    let stored_len =
        get_uvarint(rec, &mut pos).ok_or_else(|| corrupt("record stored_len varint"))?;
    let stored_end = pos
        .checked_add(stored_len as usize)
        .ok_or_else(|| corrupt("record stored_len overflow"))?;
    let crc_end = stored_end
        .checked_add(4)
        .ok_or_else(|| corrupt("record crc overflow"))?;
    if rec.len() < crc_end {
        return Err(corrupt("record truncated"));
    }
    let stored_crc = u32::from_le_bytes(rec[stored_end..crc_end].try_into().unwrap());
    if crc32c(&rec[..stored_end]) != stored_crc {
        return Err(corrupt("record crc mismatch"));
    }
    let stored = &rec[pos..stored_end];
    let payload = if frame::is_aead_frame(frame_id) {
        let session = dek.ok_or_else(|| {
            LoomError::new(
                Code::E2eLocked,
                "loom-store: encrypted object requires an unlocked key",
            )
        })?;
        frame::open_aead_frame(frame_id, stored, session, digest, plain_len, stored_len)?
    } else {
        frame::decode_payload(frame_id, stored)?
    };
    if payload.len() as u64 != plain_len {
        return Err(corrupt("record plain_len mismatch after unframing"));
    }
    // Verify under the store's identity profile, not the requested digest's tag: a
    // digest reconstructed during engine decode is tagged blake3 by convention even in a FIPS store, so
    // the store's own algorithm is the source of truth for re-hashing. `Digest` compares bytes-only, so
    // the recomputed address matches the requested one regardless of either side's tag.
    if Digest::hash(store_algo, &payload) != *digest {
        return Err(LoomError::integrity_failure(
            "stored bytes do not match requested digest",
        ));
    }
    Ok(payload)
}

/// The committed root-set a transaction leaves behind, for publishing into [`Inner`].
pub(crate) struct TxnRoots {
    pub(crate) page_count: u64,
    pub(crate) free: Vec<FreePageRun>,
    pub(crate) freemap: Option<(PageId, u64)>, // (root, page span) of the persisted free-page map
    pub(crate) region_table_root: PageId,
    pub(crate) maintenance_root: PageId,
    pub(crate) maintenance: MaintenanceState,
}

/// Persist a transaction's roots and make it durable, shared by the put commit and the GC paths: free
/// the prior free-page-map and region-table pages (`superseded`), write the new free-page map and
/// region-table page, fsync, then journal a `COMMIT` (that fsync is the commit point) and checkpoint
/// the superblock on the interval. `alloc` already holds every page this transaction wrote and freed.
#[allow(clippy::too_many_arguments)] // an internal commit helper; the roots it persists are distinct
pub(crate) fn finish_txn(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    new_gen: u64,
    object_count: u64,
    index_root: Option<PageId>,
    open_segment: u64,
    reference: Option<[u8; 32]>,
    control: Option<[u8; 32]>,
    previous_maintenance: &MaintenanceState,
    touched_segments: &BTreeSet<u64>,
    superseded: (Option<(PageId, u64)>, Option<PageId>, Option<PageId>),
    encryption: Option<Vec<u8>>,
    digest_algo: Algo,
) -> Result<TxnRoots> {
    // The prior free-page map and region-table page are superseded; free them (the new map can reuse
    // them once they age out).
    let (old_freemap, old_region, old_maintenance) = superseded;
    if let Some((root, pages)) = old_freemap {
        alloc.free(root, pages);
    }
    if let Some(rt) = old_region {
        alloc.free(rt, 1);
    }
    if let Some(root) = old_maintenance {
        alloc.free(root, 1);
    }
    // Place the region-table page and the free-page-map run by reusing low aged-out pages where one
    // fits (so they do not pin the top of the file and block truncation), carving them out of the free
    // set before the map is snapshotted so the map never lists its own pages. The map is sized for the
    // run count before its own pages are removed - an upper bound on what it must hold.
    let rt_page = alloc.alloc(1);
    let maintenance_page = alloc.alloc(1);
    let (freemap, map_root, map_reserved) = {
        let pending = alloc.snapshot_free().len();
        if pending == 0 {
            (None, None, 0)
        } else {
            let reserved = pagemap::map_pages(pending);
            let root = alloc.alloc(reserved);
            (Some((root, reserved)), Some(root), reserved)
        }
    };
    // Snapshot the free set (now excluding the region and map pages) and drop a maximal trailing run
    // of free pages, so the file can shrink to just above the highest live page.
    let runs = alloc.snapshot_free();
    let (page_count, runs) = truncate_trailing(runs, alloc.page_count());
    validate_truncated_roots(page_count, index_root, rt_page, maintenance_page, freemap)?;
    let maintenance = MaintenanceState::next(
        previous_maintenance,
        new_gen,
        object_count,
        page_count,
        &runs,
        touched_segments,
    );
    if let Some(root) = map_root {
        pagemap::write_map_at(file, DATA_START, root, map_reserved, &runs)?;
    }
    maintenance::write_maintenance(file, maintenance_page, &maintenance)?;
    let region = RegionTable {
        page_size: PAGE_SIZE,
        index_root,
        freemap_root: map_root,
        maintenance_root: Some(maintenance_page),
        open_segment,
    };
    let mut rt_buf = [0u8; PAGE_SIZE as usize];
    rt_buf[..page::REGION_TABLE_LEN].copy_from_slice(&region.encode());
    write_at(file, rt_page.offset(DATA_START), &rt_buf).map_err(io_err)?;
    file.fsync().map_err(io_err)?; // every referenced page durable before the commit point
    // journal ring: fsync the new root-set into this generation's ring slot. That fsync IS the commit
    // point - every referenced page is already durable above it, and the record survives in its own
    // slot until a later checkpoint, so a torn newer record cannot destroy this one.
    let jrec = journal::encode_commit(&journal::Roots {
        generation: new_gen,
        page_count,
        region_table: Some(rt_page),
        reference,
        control,
    });
    let ring_off = JOURNAL_OFFSET + (new_gen % RING_SLOTS) * journal::RECORD_SIZE as u64;
    write_at(file, ring_off, &jrec).map_err(io_err)?;
    file.fsync().map_err(io_err)?; // commit point: the ring record is durable
    // Online shrink, strictly after the commit point: recovery always adopts this now-durable
    // generation, whose live pages are all below `page_count`, so a lost or partial truncate just
    // leaves ignorable trailing bytes - never a too-short file for the committed generation. This is
    // why no aging window is needed here (unlike page reuse): we never fall back to an older, larger
    // generation once this one is durable.
    let _ = file.grow(DATA_START + page_count * PAGE_SIZE);
    // Periodic checkpoint: every CHECKPOINT_INTERVAL commits, fold the latest root-set into a
    // superblock so the ring scan on open stays bounded and reused slots are already checkpointed.
    // Alternating slots keep a torn checkpoint recoverable from the prior one.
    if new_gen.is_multiple_of(CHECKPOINT_INTERVAL) {
        let cp_slot = ((new_gen / CHECKPOINT_INTERVAL) & 1) * SLOT_SIZE;
        let sb = Superblock {
            generation: new_gen,
            page_count,
            digest_algo,
            region_table: Some(rt_page),
            reference,
            control,
            // Carry the immutable encryption_meta into the checkpoint so it survives the slot rewrite.
            encryption: encryption.clone(),
        }
        .encode();
        write_at(file, cp_slot, &sb).map_err(io_err)?;
        file.fsync().map_err(io_err)?;
    }
    Ok(TxnRoots {
        page_count,
        free: runs,
        freemap,
        region_table_root: rt_page,
        maintenance_root: maintenance_page,
        maintenance,
    })
}

fn validate_truncated_roots(
    page_count: u64,
    index_root: Option<PageId>,
    region_table_root: PageId,
    maintenance_root: PageId,
    freemap: Option<(PageId, u64)>,
) -> Result<()> {
    if page_count == 0 {
        return Err(corrupt("transaction roots beyond truncated page count"));
    }
    for root in [index_root, Some(region_table_root), Some(maintenance_root)] {
        if root.is_some_and(|page| page.0 >= page_count) {
            return Err(corrupt("transaction root beyond truncated page count"));
        }
    }
    if let Some((root, pages)) = freemap {
        let end = root
            .0
            .checked_add(pages)
            .ok_or_else(|| corrupt("free-page map root overflow"))?;
        if pages == 0 || end > page_count {
            return Err(corrupt("free-page map root beyond truncated page count"));
        }
    }
    Ok(())
}

/// Drop a maximal run of free pages at the very top of the array, returning the reduced page count and
/// the free runs with those pages removed. Free pages reach `page_count` only when nothing live (a
/// record, index, region, or map page) sits above them, so live data at the top blocks the shrink.
pub(crate) fn truncate_trailing(
    mut runs: Vec<FreePageRun>,
    page_count: u64,
) -> (u64, Vec<FreePageRun>) {
    let by_end: std::collections::HashMap<u64, u64> =
        runs.iter().map(|r| (r.start + r.len, r.start)).collect();
    let mut cursor = page_count;
    while let Some(&start) = by_end.get(&cursor) {
        cursor = start;
    }
    if cursor < page_count {
        runs.retain(|r| r.start < cursor);
    }
    (cursor, runs)
}

/// The number of pages the record at global page `p` occupies: one for a slab page, the whole run for
/// a large record (read from its header). Lets GC free a record's full footprint.
pub(crate) fn page_span(file: &mut dyn BackingIo, p: u64) -> Result<u64> {
    let mut hdr = [0u8; 9];
    read_exact_at(file, PageId(p).offset(DATA_START), &mut hdr).map_err(io_err)?;
    match hdr[0] {
        record::SLAB_MAGIC => Ok(1),
        record::LARGE_MAGIC => {
            let blob_len =
                record::large_blob_len(&hdr).ok_or_else(|| corrupt("bad large record header"))?;
            Ok(record::large_pages(blob_len))
        }
        _ => Err(corrupt("bad record page magic during gc")),
    }
}

/// Choose the segments worth garbage-collecting from per-segment `(live_pages, total_pages)` counts:
/// those at least half dead by page count. A segment with no dead pages is skipped, keeping cost
/// proportional to the garbage.
pub(crate) fn choose_sparse_segments_bounded(
    occupancy: &BTreeMap<u64, (u64, u64)>,
    eligible: Option<&BTreeSet<u64>>,
    budget: GcSegmentBudget,
) -> Vec<u64> {
    let mut pages = 0u64;
    let mut out = Vec::new();
    for (segment, (live_pages, total_pages)) in occupancy {
        if live_pages * 2 >= *total_pages {
            continue;
        }
        if eligible.is_some_and(|eligible| !eligible.contains(segment)) {
            continue;
        }
        if out.len() as u64 >= budget.max_segments {
            break;
        }
        if pages.saturating_add(*total_pages) > budget.max_pages && !out.is_empty() {
            break;
        }
        pages = pages.saturating_add(*total_pages);
        out.push(*segment);
    }
    out
}
