//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

#[cfg(test)]
thread_local! {
    static BLOB_LOCATOR_READS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_blob_locator_reads_for_test() {
    BLOB_LOCATOR_READS.with(|reads| reads.set(0));
}

#[cfg(test)]
pub(crate) fn blob_locator_reads_for_test() -> u64 {
    BLOB_LOCATOR_READS.with(|reads| reads.get())
}

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
    #[cfg(test)]
    RECORD_ENCODE_CALLS_FOR_TEST.with(|calls| calls.set(calls.get() + 1));
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

#[cfg(test)]
thread_local! {
    static RECORD_ENCODE_CALLS_FOR_TEST: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_record_encode_calls_for_test() {
    RECORD_ENCODE_CALLS_FOR_TEST.with(|calls| calls.set(0));
}

#[cfg(test)]
pub(crate) fn record_encode_calls_for_test() -> u64 {
    RECORD_ENCODE_CALLS_FOR_TEST.with(|calls| calls.get())
}

pub(crate) struct PreparedRecordFrame {
    pub(crate) digest: [u8; 32],
    pub(crate) frame: Vec<u8>,
}

impl PreparedRecordFrame {
    pub(crate) fn as_ref(&self) -> PreparedRecordFrameRef<'_> {
        PreparedRecordFrameRef {
            digest: self.digest,
            frame: &self.frame,
        }
    }
}

pub(crate) struct PreparedRecordFrameRef<'a> {
    pub(crate) digest: [u8; 32],
    pub(crate) frame: &'a [u8],
}

pub(crate) fn prepare_record_frame(
    digest: Digest,
    canonical: &[u8],
    codec: Codec,
    enc: Option<&DekSession>,
) -> Result<PreparedRecordFrame> {
    Ok(PreparedRecordFrame {
        digest: *digest.bytes(),
        frame: encode_record(&digest, canonical, codec, enc)?,
    })
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
    let prepared = fresh
        .iter()
        .map(|(digest, canonical, codec)| prepare_record_frame(*digest, canonical, *codec, enc))
        .collect::<Result<Vec<_>>>()?;
    let prepared = prepared
        .iter()
        .map(|record| record.as_ref())
        .collect::<Vec<_>>();
    write_prepared_record_pages(file, alloc, &prepared)
}

pub(crate) fn write_prepared_record_pages(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    prepared: &[PreparedRecordFrameRef<'_>],
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    let mut placements = vec![None; prepared.len()];
    let mut slab = SlabBuilder::new();
    let mut pending: Vec<(usize, [u8; 32], u32)> = Vec::new();
    for (index, record) in prepared.iter().enumerate() {
        if record::is_large(record.frame.len() as u64) {
            let dedicated =
                write_dedicated_blob_pages(file, alloc, &[(record.digest, record.frame)])?;
            placements[index] = dedicated.into_iter().next();
        } else {
            let slot = match slab.try_push(record.frame) {
                Some(slot) => slot,
                None => {
                    flush_prepared_slab(file, alloc, &slab, &pending, &mut placements)?;
                    slab = SlabBuilder::new();
                    pending.clear();
                    slab.try_push(record.frame)
                        .expect("a fresh slab page holds one small record")
                }
            };
            pending.push((index, record.digest, slot));
        }
    }
    if !slab.is_empty() {
        flush_prepared_slab(file, alloc, &slab, &pending, &mut placements)?;
    }
    placements
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| corrupt("prepared record placement missing"))
}

fn flush_prepared_slab(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    slab: &SlabBuilder,
    pending: &[(usize, [u8; 32], u32)],
    placements: &mut [Option<([u8; 32], RecordLoc)>],
) -> Result<()> {
    let page = alloc.alloc(1);
    write_at(file, page.offset(DATA_START), &slab.finish()).map_err(io_err)?;
    for (index, digest, slot) in pending {
        placements[*index] = Some((*digest, RecordLoc::from_global(page.0, *slot)));
    }
    Ok(())
}

pub(crate) fn write_blob_pages(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    fresh: &[([u8; 32], &[u8])],
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    let mut placements = Vec::with_capacity(fresh.len());
    let mut slab = SlabBuilder::new();
    let mut pending: Vec<([u8; 32], u32)> = Vec::new();
    for (key, blob) in fresh {
        if record::is_large(blob.len() as u64) {
            let buf = record::encode_large(blob);
            let page = alloc.alloc(record::large_pages(blob.len() as u64));
            write_at(file, page.offset(DATA_START), &buf).map_err(io_err)?;
            placements.push((*key, RecordLoc::from_global(page.0, 0)));
        } else {
            let slot = match slab.try_push(blob) {
                Some(slot) => slot,
                None => {
                    flush_slab(file, alloc, &slab, &pending, &mut placements)?;
                    slab = SlabBuilder::new();
                    pending.clear();
                    slab.try_push(blob)
                        .expect("a fresh slab page holds one small blob")
                }
            };
            pending.push((*key, slot));
        }
    }
    if !slab.is_empty() {
        flush_slab(file, alloc, &slab, &pending, &mut placements)?;
    }
    Ok(placements)
}

pub(crate) fn write_dedicated_blob_pages(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    fresh: &[([u8; 32], &[u8])],
) -> Result<Vec<([u8; 32], RecordLoc)>> {
    let mut placements = Vec::with_capacity(fresh.len());
    for (key, blob) in fresh {
        let capacity = record::chunked_blob_payload_capacity();
        let page_count = blob.len().max(1).div_ceil(capacity);
        let pages = (0..page_count).map(|_| alloc.alloc(1)).collect::<Vec<_>>();
        for (index, page) in pages.iter().enumerate() {
            let start = index * capacity;
            let end = blob.len().min(start + capacity);
            let chunk = &blob[start.min(blob.len())..end];
            let next = pages.get(index + 1).map(|page| page.0);
            let encoded = record::encode_chunked_blob_page(chunk, next, blob.len() as u64)
                .ok_or_else(|| corrupt("mutable blob chunk exceeds page capacity"))?;
            write_at(file, page.offset(DATA_START), &encoded).map_err(io_err)?;
        }
        placements.push((*key, RecordLoc::from_global(pages[0].0, 0)));
    }
    Ok(placements)
}

fn visit_chunked_blob_pages(
    file: &mut dyn BackingIo,
    start: u64,
    page_count: u64,
    mut visit: impl FnMut(u64, u64, &[u8]) -> Result<()>,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut current = start;
    let mut expected_total = None;
    let mut accumulated = 0u64;
    loop {
        if current >= page_count || !seen.insert(current) {
            return Err(corrupt(
                "mutable blob chunk chain is cyclic or out of bounds",
            ));
        }
        let mut page = [0u8; PAGE_SIZE as usize];
        read_exact_at(file, PageId(current).offset(DATA_START), &mut page).map_err(io_err)?;
        let Some((next, total, chunk)) = record::decode_chunked_blob_page(&page) else {
            return Err(corrupt("bad mutable blob chunk page"));
        };
        if expected_total
            .replace(total)
            .is_some_and(|seen| seen != total)
        {
            return Err(corrupt(
                "mutable blob chunk total length changed within chain",
            ));
        }
        accumulated = accumulated
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| corrupt("mutable blob chunk length overflow"))?;
        if accumulated > total {
            return Err(corrupt("mutable blob chunk chain exceeds total length"));
        }
        visit(current, total, chunk)?;
        match next {
            Some(next) => current = next,
            None if accumulated == total => return Ok(()),
            None => return Err(corrupt("mutable blob chunk chain is truncated")),
        }
    }
}

pub(crate) fn chunked_blob_pages(
    file: &mut dyn BackingIo,
    start: u64,
    page_count: u64,
) -> Result<Vec<u64>> {
    let mut pages = Vec::new();
    visit_chunked_blob_pages(file, start, page_count, |page, _, _| {
        pages.push(page);
        Ok(())
    })?;
    Ok(pages)
}

pub(crate) fn read_chunked_blob(
    file: &mut dyn BackingIo,
    start: u64,
    page_count: u64,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    visit_chunked_blob_pages(file, start, page_count, |_, total, chunk| {
        if out.is_empty() {
            let capacity = usize::try_from(total)
                .map_err(|_| corrupt("mutable blob length does not fit this platform"))?;
            out.try_reserve(capacity)
                .map_err(|_| corrupt("mutable blob length exceeds addressable memory"))?;
        }
        out.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(out)
}

pub(crate) fn read_blob_from_loc(
    file: &mut dyn BackingIo,
    loc: RecordLoc,
    page_count: u64,
) -> Result<Vec<u8>> {
    #[cfg(test)]
    BLOB_LOCATOR_READS.with(|reads| reads.set(reads.get().saturating_add(1)));
    let global = loc.global_page();
    if global >= page_count {
        return Err(corrupt("blob locator out of range"));
    }
    let mut first = [0u8; PAGE_SIZE as usize];
    read_exact_at(file, PageId(global).offset(DATA_START), &mut first).map_err(io_err)?;
    match first[0] {
        record::SLAB_MAGIC => record::read_slab_slot(&first, loc.slot)
            .map(|bytes| bytes.to_vec())
            .ok_or_else(|| corrupt("bad slab blob slot on read")),
        record::LARGE_MAGIC => {
            let blob_len =
                record::large_blob_len(&first).ok_or_else(|| corrupt("bad large blob header"))?;
            let pages = record::large_pages(blob_len);
            if global.saturating_add(pages) > page_count {
                return Err(corrupt("large blob run past the page array"));
            }
            let mut buf = vec![0u8; (pages * PAGE_SIZE) as usize];
            read_exact_at(file, PageId(global).offset(DATA_START), &mut buf).map_err(io_err)?;
            record::decode_large(&buf)
                .map(|bytes| bytes.to_vec())
                .ok_or_else(|| corrupt("large blob parse failure"))
        }
        record::CHUNKED_BLOB_MAGIC => read_chunked_blob(file, global, page_count),
        _ => Err(corrupt("bad blob page magic on read")),
    }
}

pub(crate) fn blob_pages(
    file: &mut dyn BackingIo,
    start: u64,
    page_count: u64,
) -> Result<Vec<u64>> {
    let mut header = [0u8; 9];
    read_exact_at(file, PageId(start).offset(DATA_START), &mut header).map_err(io_err)?;
    match header[0] {
        record::SLAB_MAGIC => Ok(vec![start]),
        record::LARGE_MAGIC => {
            let len =
                record::large_blob_len(&header).ok_or_else(|| corrupt("bad large blob header"))?;
            let span = record::large_pages(len);
            if start.saturating_add(span) > page_count {
                return Err(corrupt("large blob run past the page array"));
            }
            Ok((start..start + span).collect())
        }
        record::CHUNKED_BLOB_MAGIC => chunked_blob_pages(file, start, page_count),
        _ => Err(corrupt("bad blob page magic")),
    }
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
#[derive(Clone, Debug)]
pub(crate) struct TxnRoots {
    pub(crate) generation: u64,
    pub(crate) page_count: u64,
    pub(crate) object_index: Option<PageId>,
    pub(crate) free: Vec<FreePageRun>,
    pub(crate) freemap: Option<(PageId, u64)>, // (root, page span) of the persisted free-page map
    pub(crate) region_table_root: PageId,
    pub(crate) maintenance_root: PageId,
    pub(crate) legacy_overlay: Option<PageId>,
    pub(crate) current_record_root: Option<PageId>,
    pub(crate) root_catalog: TxnRootCatalog,
    pub(crate) mutable_overlay_generation_floor: u64,
    pub(crate) minimum_recoverable_generation: u64,
    pub(crate) reference: Option<[u8; 32]>,
    pub(crate) control: Option<[u8; 32]>,
    pub(crate) maintenance: MaintenanceState,
    pub(crate) metadata_bootstrap_reserve: crate::page::MetadataBootstrapReserve,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TxnRootCatalog {
    pub(crate) root: Option<PageId>,
    pub(crate) entries: Vec<RootCatalogEntry>,
}

pub(crate) struct TxnRootInputs {
    pub(crate) object_index: Option<PageId>,
    pub(crate) legacy_overlay: Option<PageId>,
    pub(crate) current_records: Option<PageId>,
    pub(crate) root_catalog: TxnRootCatalog,
    pub(crate) previous_mutable_overlay_generation_floor: u64,
    pub(crate) mutable_overlay_generation_floor: u64,
    pub(crate) reference: Option<[u8; 32]>,
    pub(crate) control: Option<[u8; 32]>,
}

/// Persist a transaction's roots and make it durable, shared by the put commit and the GC paths:
/// publish bounded metadata roots, fsync, then journal a `COMMIT` (that fsync is the commit point)
/// and checkpoint the superblock on the interval. `alloc` already holds every page this transaction
/// wrote and freed.
#[allow(clippy::too_many_arguments)] // an internal commit helper; the roots it persists are distinct
#[track_caller]
pub(crate) fn finish_txn(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    new_gen: u64,
    object_count: u64,
    root_inputs: TxnRootInputs,
    open_segment: u64,
    previous_maintenance: &MaintenanceState,
    touched_segments: &BTreeSet<u64>,
    superseded: (Option<(PageId, u64)>, Option<PageId>, Option<PageId>),
    encryption: Option<Vec<u8>>,
    digest_algo: Algo,
    // Durability diagnostics sink: `Some` on the group-commit / hot-mutable durable publish
    // path, `None` on maintenance/compaction paths. Each `fsync` below is timed once when present.
    metrics: Option<&GroupCommitMetrics>,
) -> Result<TxnRoots> {
    let caller = std::panic::Location::caller();
    finish_txn_with_pre_commit_hook(
        file,
        alloc,
        new_gen,
        object_count,
        root_inputs,
        open_segment,
        previous_maintenance,
        touched_segments,
        superseded,
        encryption,
        digest_algo,
        metrics,
        None,
    )
    .map_err(|mut error| {
        if error.code == Code::CorruptObject && error.message.contains("root") {
            error.message = format!(
                "{} (finish_txn caller {}:{})",
                error.message,
                caller.file(),
                caller.line()
            );
        }
        error
    })
}

pub(crate) struct PreparedForegroundTxnResult {
    roots: TxnRoots,
    free_map_publication_demand: pagemap::FreeMapPublicationDemand,
}

impl PreparedForegroundTxnResult {
    pub(crate) fn into_parts(self) -> (TxnRoots, pagemap::FreeMapPublicationDemand) {
        (self.roots, self.free_map_publication_demand)
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        roots: TxnRoots,
        free_map_publication_demand: pagemap::FreeMapPublicationDemand,
    ) -> Self {
        Self {
            roots,
            free_map_publication_demand,
        }
    }
}

fn allocate_free_map_publication_pages(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    previous_freemap_root: Option<PageId>,
    demand: pagemap::FreeMapPublicationDemand,
    dirty_range_count: u64,
) -> Result<Vec<PageId>> {
    #[cfg(any(test, feature = "test-hooks"))]
    if demand.allocation_pages() > pagemap::FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES {
        let free_map_depth = previous_freemap_root
            .and_then(|root| {
                pagebtree::free_page_extent_tree_depth(file, DATA_START, root, alloc.page_count())
                    .ok()
            })
            .unwrap_or_default();
        observe_rejected_free_map_publication(RejectedFreeMapPublicationDiagnostic {
            demanded_pages: demand.allocation_pages(),
            reserve_capacity_pages: pagemap::FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES,
            reserve_available_pages: alloc.metadata_bootstrap_page_count(),
            extent_deletes: demand.extent_deletes,
            extent_upserts: demand.extent_upserts,
            btree_node_pages: demand.btree_node_pages,
            affected_existing_btree_pages: demand.affected_existing_btree_pages,
            split_decisions: demand.split_decisions,
            dirty_range_count,
            free_map_depth,
        });
    }
    #[cfg(not(any(test, feature = "test-hooks")))]
    let _ = (file, previous_freemap_root, dirty_range_count);
    alloc.alloc_metadata_bootstrap_pages(demand.allocation_pages())
}

#[allow(clippy::too_many_arguments)]
#[track_caller]
pub(crate) fn finish_foreground_txn_on_planning_backing(
    file: &mut PlanningBacking<'_>,
    alloc: &mut PageAllocator,
    new_gen: u64,
    object_count: u64,
    root_inputs: TxnRootInputs,
    open_segment: u64,
    previous_maintenance: &MaintenanceState,
    touched_segments: &BTreeSet<u64>,
    superseded: (Option<(PageId, u64)>, Option<PageId>, Option<PageId>),
    encryption: Option<Vec<u8>>,
    digest_algo: Algo,
    metrics: Option<&GroupCommitMetrics>,
    free_map_publication: pagemap::PreparedFreeMapPublication,
) -> Result<PreparedForegroundTxnResult> {
    let free_map_publication_demand = free_map_publication.demand();
    let roots = finish_txn_impl(
        file,
        alloc,
        new_gen,
        object_count,
        root_inputs,
        open_segment,
        previous_maintenance,
        touched_segments,
        superseded,
        encryption,
        digest_algo,
        metrics,
        None,
        Some(free_map_publication),
    )?;
    Ok(PreparedForegroundTxnResult {
        roots,
        free_map_publication_demand,
    })
}

#[allow(clippy::too_many_arguments)] // mirrors finish_txn and adds one test hook at the commit edge
pub(crate) fn finish_txn_with_pre_commit_hook(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    new_gen: u64,
    object_count: u64,
    root_inputs: TxnRootInputs,
    open_segment: u64,
    previous_maintenance: &MaintenanceState,
    touched_segments: &BTreeSet<u64>,
    superseded: (Option<(PageId, u64)>, Option<PageId>, Option<PageId>),
    encryption: Option<Vec<u8>>,
    digest_algo: Algo,
    metrics: Option<&GroupCommitMetrics>,
    pre_commit_hook: Option<&mut dyn FnMut() -> Result<()>>,
) -> Result<TxnRoots> {
    finish_txn_impl(
        file,
        alloc,
        new_gen,
        object_count,
        root_inputs,
        open_segment,
        previous_maintenance,
        touched_segments,
        superseded,
        encryption,
        digest_algo,
        metrics,
        pre_commit_hook,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish_txn_impl(
    file: &mut dyn BackingIo,
    alloc: &mut PageAllocator,
    new_gen: u64,
    object_count: u64,
    root_inputs: TxnRootInputs,
    open_segment: u64,
    previous_maintenance: &MaintenanceState,
    touched_segments: &BTreeSet<u64>,
    superseded: (Option<(PageId, u64)>, Option<PageId>, Option<PageId>),
    encryption: Option<Vec<u8>>,
    digest_algo: Algo,
    metrics: Option<&GroupCommitMetrics>,
    mut pre_commit_hook: Option<&mut dyn FnMut() -> Result<()>>,
    free_map_publication: Option<pagemap::PreparedFreeMapPublication>,
) -> Result<TxnRoots> {
    #[cfg(target_arch = "wasm32")]
    let _ = metrics;
    if root_inputs.mutable_overlay_generation_floor
        < root_inputs.previous_mutable_overlay_generation_floor
    {
        return Err(corrupt("mutable overlay generation floor cannot decrease"));
    }
    if root_inputs.root_catalog.root.is_none() && !root_inputs.root_catalog.entries.is_empty() {
        return Err(corrupt(
            "root catalog publication cannot carry entries without root",
        ));
    }
    if root_inputs.root_catalog.root.is_some() && root_inputs.root_catalog.entries.is_empty() {
        return Err(corrupt(
            "root catalog publication cannot carry root without entries",
        ));
    }
    if !root_inputs.root_catalog.entries.is_empty() {
        RootCatalog {
            entries: root_inputs.root_catalog.entries.clone(),
        }
        .encode()
        .map_err(|_| corrupt("root catalog encode failure"))?;
    }
    if root_inputs.legacy_overlay.is_some()
        && (root_inputs.current_records.is_some() || root_inputs.root_catalog.root.is_some())
    {
        return Err(corrupt(
            "legacy overlay cannot publish with canonical mutable roots",
        ));
    }
    // The prior region-table and maintenance pages are superseded. The extent-tree writer compares
    // the prior free-map root against the new free set and publishes the replacement root.
    let (old_freemap, _old_region, _old_maintenance) = superseded;
    let previous_free = alloc.initial_free_runs();
    let previous_freemap_root = old_freemap.map(|(root, _)| root);
    if let Some(prepared) = free_map_publication.as_ref() {
        let pending_updates = alloc.pending_free_map_extent_updates();
        pagemap::validate_prepared_tree_map_publication_source(
            previous_freemap_root,
            &pending_updates,
            prepared,
        )?;
    }
    alloc.activate_publication_reserve();
    let rt_page = alloc.alloc(1);
    let maintenance_page = alloc.alloc(1);
    #[cfg(any(test, feature = "test-hooks"))]
    alloc.note_fixed_metadata_pages(2);
    alloc.ensure_metadata_bootstrap_capacity()?;
    let map_root = match free_map_publication {
        Some(prepared) => {
            let dirty_range_count = alloc.pending_free_map_extent_update_count() as u64;
            let allocated = allocate_free_map_publication_pages(
                file,
                alloc,
                previous_freemap_root,
                prepared.demand(),
                dirty_range_count,
            )?;
            let freemap_updates = alloc.take_free_map_extent_updates();
            pagemap::apply_prepared_tree_map_publication(
                file,
                DATA_START,
                alloc,
                previous_freemap_root,
                freemap_updates.clone(),
                prepared,
                &allocated,
            )?
        }
        None => {
            let dirty_range_count = alloc.pending_free_map_extent_update_count() as u64;
            let freemap_updates = alloc.take_free_map_extent_updates();
            let prepared = pagemap::prepare_tree_map_publication(
                file,
                DATA_START,
                previous_freemap_root,
                &previous_free,
                freemap_updates.clone(),
                freemap_updates.clone(),
                alloc.page_count(),
            )?;
            let allocated = allocate_free_map_publication_pages(
                file,
                alloc,
                previous_freemap_root,
                prepared.demand(),
                dirty_range_count,
            )?;
            pagemap::apply_prepared_tree_map_publication(
                file,
                DATA_START,
                alloc,
                previous_freemap_root,
                freemap_updates,
                prepared,
                &allocated,
            )?
        }
    };
    if alloc.pending_free_map_extent_update_count() != 0 {
        return Err(corrupt(
            "free-map publication left unpersisted extent updates",
        ));
    }
    // Snapshot the free set after every page referenced by this generation has been allocated. The
    // published page count must continue to bound the free-map tree written above; physical tail
    // trimming is a separate maintenance publication.
    let runs = alloc.snapshot_free();
    for (name, root) in [
        ("object index", root_inputs.object_index),
        ("legacy overlay", root_inputs.legacy_overlay),
        ("current record", root_inputs.current_records),
        ("root catalog", root_inputs.root_catalog.root),
        ("region table", Some(rt_page)),
        ("maintenance", Some(maintenance_page)),
        ("free map", map_root),
    ] {
        if let Some(page) = root
            && runs
                .iter()
                .any(|run| page.0 >= run.start && page.0 < run.start.saturating_add(run.len))
        {
            #[cfg(any(test, feature = "test-hooks"))]
            let allocator_detail = format!(" ({})", alloc.free_page_debug(page.0));
            #[cfg(not(any(test, feature = "test-hooks")))]
            let allocator_detail = String::new();
            return Err(corrupt(&format!(
                "transaction {name} root {} is listed as free{allocator_detail}",
                page.0,
            )));
        }
    }
    for entry in &root_inputs.root_catalog.entries {
        if runs.iter().any(|run| {
            entry.root.0 >= run.start && entry.root.0 < run.start.saturating_add(run.len)
        }) {
            #[cfg(any(test, feature = "test-hooks"))]
            let allocator_detail = format!(" ({})", alloc.free_page_debug(entry.root.0));
            #[cfg(not(any(test, feature = "test-hooks")))]
            let allocator_detail = String::new();
            return Err(corrupt(&format!(
                "transaction root catalog family {} root {} is listed as free{allocator_detail}",
                entry.family_id, entry.root.0,
            )));
        }
    }
    let page_count = alloc.page_count();
    if let Some(entry) = root_inputs
        .root_catalog
        .entries
        .iter()
        .find(|entry| entry.root.0 >= page_count)
    {
        return Err(corrupt(&format!(
            "transaction root catalog family {} root {} beyond truncated page count {page_count}",
            entry.family_id, entry.root.0,
        )));
    }
    let freemap = map_root.map(|root| (root, 1));
    validate_truncated_roots(
        page_count,
        root_inputs.object_index,
        root_inputs.legacy_overlay,
        root_inputs.current_records,
        root_inputs.root_catalog.root,
        rt_page,
        maintenance_page,
        freemap,
    )?;
    let maintenance = MaintenanceState::next(
        previous_maintenance,
        new_gen,
        object_count,
        page_count,
        &runs,
        touched_segments,
    );
    maintenance::write_maintenance(file, maintenance_page, &maintenance)?;
    let region = RegionTable {
        page_size: PAGE_SIZE,
        index_root: root_inputs.object_index,
        freemap_root: map_root,
        maintenance_root: Some(maintenance_page),
        overlay_root: root_inputs.legacy_overlay,
        current_record_root: root_inputs.current_records,
        root_catalog_root: root_inputs.root_catalog.root,
        open_segment,
        mutable_overlay_generation_floor: root_inputs.mutable_overlay_generation_floor,
        minimum_recoverable_generation: new_gen,
        metadata_bootstrap_reserve: alloc.metadata_bootstrap_descriptor(new_gen),
    };
    let rt_buf = region
        .encode_page(page_count)
        .map_err(|_| corrupt("canonical region table encode failure"))?;
    write_at(file, rt_page.offset(DATA_START), &rt_buf).map_err(io_err)?;
    if let Some(hook) = pre_commit_hook.as_mut() {
        hook()?;
    }
    #[cfg(not(target_arch = "wasm32"))]
    let fsync_started = std::time::Instant::now();
    file.fsync().map_err(io_err)?; // every referenced page durable before the commit point
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(metrics) = metrics {
        metrics.record_fsync(fsync_started.elapsed());
    }
    // journal ring: fsync the new root-set into this generation's ring slot. That fsync IS the commit
    // point - every referenced page is already durable above it, and the record survives in its own
    // slot until a later checkpoint, so a torn newer record cannot destroy this one.
    let jrec = journal::encode_commit(&journal::Roots {
        generation: new_gen,
        page_count,
        region_table: Some(rt_page),
        reference: root_inputs.reference,
        control: root_inputs.control,
    });
    let ring_off = JOURNAL_OFFSET + (new_gen % RING_SLOTS) * journal::RECORD_SIZE as u64;
    write_at(file, ring_off, &jrec).map_err(io_err)?;
    #[cfg(not(target_arch = "wasm32"))]
    let commit_fsync_started = std::time::Instant::now();
    file.fsync().map_err(io_err)?; // commit point: the ring record is durable
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(metrics) = metrics {
        metrics.record_fsync(commit_fsync_started.elapsed());
    }
    // Ordinary commits grow the backing but never shrink it. Physical reclamation is isolated behind
    // the cross-process reader lease, while trailing bytes remain harmless to recovery.
    let committed_len = DATA_START + page_count * PAGE_SIZE;
    if file.size().map_err(io_err)? < committed_len {
        file.grow(committed_len).map_err(io_err)?;
    }
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
            reference: root_inputs.reference,
            control: root_inputs.control,
            // Carry the immutable encryption_meta into the checkpoint so it survives the slot rewrite.
            encryption: encryption.clone(),
        }
        .encode();
        write_at(file, cp_slot, &sb).map_err(io_err)?;
        #[cfg(not(target_arch = "wasm32"))]
        let checkpoint_fsync_started = std::time::Instant::now();
        file.fsync().map_err(io_err)?;
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(metrics) = metrics {
            metrics.record_fsync(checkpoint_fsync_started.elapsed());
        }
    }
    #[cfg(any(test, feature = "test-hooks"))]
    if !file.is_planning() {
        crate::complete_btree_batch_transaction_for_test();
        crate::complete_foreground_allocator_transaction_for_test(alloc.transaction_stats());
    }
    Ok(TxnRoots {
        generation: new_gen,
        page_count,
        object_index: root_inputs.object_index,
        free: runs,
        freemap,
        region_table_root: rt_page,
        maintenance_root: maintenance_page,
        legacy_overlay: root_inputs.legacy_overlay,
        current_record_root: root_inputs.current_records,
        root_catalog: root_inputs.root_catalog,
        mutable_overlay_generation_floor: root_inputs.mutable_overlay_generation_floor,
        minimum_recoverable_generation: new_gen,
        reference: root_inputs.reference,
        control: root_inputs.control,
        maintenance,
        metadata_bootstrap_reserve: alloc.metadata_bootstrap_descriptor(new_gen),
    })
}

fn validate_truncated_roots(
    page_count: u64,
    index_root: Option<PageId>,
    overlay_root: Option<PageId>,
    current_record_root: Option<PageId>,
    root_catalog_root: Option<PageId>,
    region_table_root: PageId,
    maintenance_root: PageId,
    freemap: Option<(PageId, u64)>,
) -> Result<()> {
    if page_count == 0 {
        return Err(corrupt("transaction roots beyond truncated page count"));
    }
    for (name, root) in [
        ("object index", index_root),
        ("legacy overlay", overlay_root),
        ("current record", current_record_root),
        ("root catalog", root_catalog_root),
        ("region table", Some(region_table_root)),
        ("maintenance", Some(maintenance_root)),
    ] {
        if let Some(page) = root
            && page.0 >= page_count
        {
            return Err(corrupt(&format!(
                "transaction {name} root {} beyond truncated page count {page_count}",
                page.0
            )));
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
#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn prepared_free_map_source_mismatch_rejects_before_finish_allocations() {
        let prior_superblock = Superblock {
            generation: 7,
            page_count: 64,
            digest_algo: Algo::Blake3,
            region_table: None,
            reference: None,
            control: None,
            encryption: None,
        }
        .encode();
        let mut file = MemoryBacking::from_bytes(prior_superblock.to_vec());
        let mut allocator = PageAllocator::new(64, 8, Vec::new());
        let first = allocator.extend(1);
        allocator.free(first, 1).unwrap();
        let prepared_updates = allocator.pending_free_map_extent_updates();
        let prepared = pagemap::prepare_tree_map_publication(
            &mut file,
            DATA_START,
            None,
            &[],
            prepared_updates.clone(),
            prepared_updates,
            allocator.page_count(),
        )
        .unwrap();

        let second = allocator.extend(1);
        allocator.free(second, 1).unwrap();
        let file_before = file.to_bytes();
        let page_count_before = allocator.page_count();
        let free_before = allocator.snapshot_free();
        let reserve_before = allocator.metadata_bootstrap_descriptor(8);
        let updates_before = allocator.pending_free_map_extent_updates();
        let stats_before = allocator.transaction_stats();

        let error = finish_txn_impl(
            &mut file,
            &mut allocator,
            8,
            0,
            TxnRootInputs {
                object_index: None,
                legacy_overlay: None,
                current_records: None,
                root_catalog: TxnRootCatalog {
                    root: None,
                    entries: Vec::new(),
                },
                previous_mutable_overlay_generation_floor: 0,
                mutable_overlay_generation_floor: 0,
                reference: None,
                control: None,
            },
            0,
            &MaintenanceState::default(),
            &BTreeSet::new(),
            (None, None, None),
            None,
            Algo::Blake3,
            None,
            None,
            Some(prepared),
        )
        .unwrap_err();

        assert_eq!(error.code, Code::CorruptObject);
        assert!(error.message.contains("source mismatch"));
        assert_eq!(file.to_bytes(), file_before);
        assert_eq!(file.size().unwrap(), file_before.len() as u64);
        assert_eq!(allocator.page_count(), page_count_before);
        assert_eq!(allocator.snapshot_free(), free_before);
        assert_eq!(allocator.metadata_bootstrap_descriptor(8), reserve_before);
        assert_eq!(allocator.pending_free_map_extent_updates(), updates_before);
        assert_eq!(allocator.transaction_stats(), stats_before);
        let mut slot = [0u8; SLOT_SIZE as usize];
        slot.copy_from_slice(&file.to_bytes());
        let visible = Superblock::decode(&slot).unwrap();
        assert_eq!(visible.generation, 7);
        assert_eq!(visible.page_count, 64);
        assert_eq!(visible.region_table, None);
        assert_eq!(visible.reference, None);
        assert_eq!(visible.control, None);
    }

    #[test]
    fn rejected_free_map_publication_observer_captures_record_io_allocation_branch() {
        let mut file = MemoryBacking::new();
        let mut allocator = PageAllocator::new(64, 7, Vec::new());
        allocator.ensure_metadata_bootstrap_capacity().unwrap();
        let demand = pagemap::FreeMapPublicationDemand {
            extent_deletes: 17,
            extent_upserts: 13,
            btree_node_pages: 513,
            affected_existing_btree_pages: 41,
            split_decisions: 9,
        };
        let page_count_before = allocator.page_count();
        let reserve_before = allocator.metadata_bootstrap_descriptor(7);
        let file_size_before = file.size().unwrap();
        let observations = Arc::new(Mutex::new(Vec::new()));
        {
            let captured = Arc::clone(&observations);
            let _guard =
                install_rejected_free_map_publication_test_observer(Arc::new(move |diagnostic| {
                    captured.lock().unwrap().push(diagnostic)
                }));
            let error =
                allocate_free_map_publication_pages(&mut file, &mut allocator, None, demand, 23)
                    .unwrap_err();
            assert_eq!(error.code, Code::ResourceExhausted);
        }
        assert_eq!(allocator.page_count(), page_count_before);
        assert_eq!(allocator.metadata_bootstrap_descriptor(7), reserve_before);
        assert_eq!(file.size().unwrap(), file_size_before);
        assert_eq!(
            *observations.lock().unwrap(),
            vec![RejectedFreeMapPublicationDiagnostic {
                demanded_pages: 513,
                reserve_capacity_pages: pagemap::FOREGROUND_TRANSACTION_METADATA_LIMIT_PAGES,
                reserve_available_pages: pagemap::METADATA_BOOTSTRAP_TARGET_PAGES,
                extent_deletes: 17,
                extent_upserts: 13,
                btree_node_pages: 513,
                affected_existing_btree_pages: 41,
                split_decisions: 9,
                dirty_range_count: 23,
                free_map_depth: 0,
            }]
        );

        let error =
            allocate_free_map_publication_pages(&mut file, &mut allocator, None, demand, 24)
                .unwrap_err();
        assert_eq!(error.code, Code::ResourceExhausted);
        assert_eq!(observations.lock().unwrap().len(), 1);
    }
}
