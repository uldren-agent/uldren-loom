//! The append-log / durable-queue facet - a versioned, append-only stream of entries keyed by a
//! monotonic seq (the entry's index). Pure-Rust, `wasm32`-clean, deterministic. Streams persist as a
//! structured stream root (metadata blob plus a sequence-keyed entry map) that versions, branches, and
//! syncs through the engine.
//!
//! Single-writer per stream (ref CAS + retry). Consumer offsets are caller state, not log content, so
//! they are not modeled in the stream itself.

use crate::acl::AclRight;
use crate::cbor::{self, Value};
use crate::change_set::{ChangeCursor, ChangeGapState, ChangeItem, ChangeSet};
use crate::error::Result;
use crate::provider::ObjectStore;
use crate::vcs::Loom;
use crate::workspace::{FacetKind, WorkspaceId, facet_root};

/// A versioned append-only stream: entries in append order, addressed by their `seq` (0-based index).
#[derive(Debug, Clone, Default)]
pub struct Stream {
    entries: Vec<Vec<u8>>,
}

impl Stream {
    /// An empty stream.
    pub fn new() -> Self {
        Self::default()
    }
    /// Number of entries (also the seq the next append will get).
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// Whether the stream has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Append `entry`, returning its assigned seq (the previous length).
    pub fn append(&mut self, entry: Vec<u8>) -> usize {
        let seq = self.entries.len();
        self.entries.push(entry);
        seq
    }
    /// The entry at `seq`, or `None` if out of range.
    pub fn get(&self, seq: usize) -> Option<&[u8]> {
        self.entries.get(seq).map(Vec::as_slice)
    }
    /// Entries with `lo <= seq < hi` (clamped to the stream), oldest first.
    pub fn range(&self, lo: usize, hi: usize) -> &[Vec<u8>] {
        let hi = hi.min(self.entries.len());
        let lo = lo.min(hi);
        &self.entries[lo..hi]
    }
    /// All entries, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &[u8])> {
        self.entries
            .iter()
            .enumerate()
            .map(|(i, e)| (i, e.as_slice()))
    }

    /// Canonical bytes: entries in seq order. Deterministic.
    pub fn encode(&self) -> Vec<u8> {
        let items = self.entries.iter().cloned().map(Value::Bytes).collect();
        cbor::encode(&Value::Array(items))
    }
    /// Parse a stream from [`Stream::encode`] output.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut s = Stream::new();
        for item in cbor::decode_array(bytes)? {
            s.entries.push(cbor::as_bytes(item)?);
        }
        Ok(s)
    }
}

/// Stage `stream` under `name` in `ns` as a structured stream slot; `commit` snapshots it.
pub fn put_stream<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    stream: &Stream,
) -> Result<()> {
    loom.stage_stream(ns, name, stream)
}

/// Load the stream named `name` from `ns`'s current working tree, or `NOT_FOUND`.
pub fn get_stream<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId, name: &str) -> Result<Stream> {
    loom.load_stream(ns, name)
}

/// Append `entry` to the structured stream `name` in `ns`, returning the assigned zero-based sequence.
/// Stages the updated stream root; `commit` snapshots it.
pub fn append<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    name: &str,
    entry: &[u8],
) -> Result<usize> {
    loom.stream_append(ns, name, entry)
}

/// The payload at `seq` in the structured stream `name` in `ns`, or `None` if out of range.
pub fn get<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    seq: usize,
) -> Result<Option<Vec<u8>>> {
    loom.stream_get(ns, name, seq)
}

/// The payloads with `lo <= seq < hi` (clamped) in the structured stream `name`, oldest first.
pub fn range<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    name: &str,
    lo: usize,
    hi: usize,
) -> Result<Vec<Vec<u8>>> {
    loom.stream_range(ns, name, lo, hi)
}

/// The number of entries in the structured stream `name` in `ns`.
pub fn len<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId, name: &str) -> Result<usize> {
    loom.stream_len(ns, name)
}

/// The next sequence the named consumer should read from `stream`; `0` when none is stored.
pub fn consumer_position<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    stream: &str,
    consumer_id: &str,
) -> Result<u64> {
    loom.consumer_position(ns, stream, consumer_id)
}

/// Read up to `max` entries from the consumer's stored next sequence, oldest first; does not advance.
pub fn consumer_read<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    stream: &str,
    consumer_id: &str,
    max: usize,
) -> Result<Vec<Vec<u8>>> {
    loom.consumer_read(ns, stream, consumer_id, max)
}

/// Advance the named consumer's next sequence to `next_seq`; rejects backward movement.
pub fn consumer_advance<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    stream: &str,
    consumer_id: &str,
    next_seq: u64,
) -> Result<()> {
    loom.consumer_advance(ns, stream, consumer_id, next_seq)
}

/// Set the named consumer's next sequence to `next_seq`, which may move backward.
pub fn consumer_reset<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    stream: &str,
    consumer_id: &str,
    next_seq: u64,
) -> Result<()> {
    loom.consumer_reset(ns, stream, consumer_id, next_seq)
}

pub fn retained_low_water_mark<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    stream: &str,
) -> Result<u64> {
    loom.stream_retained_low_water_mark(ns, stream)
}

pub fn set_retained_low_water_mark<S: ObjectStore>(
    loom: &mut Loom<S>,
    ns: WorkspaceId,
    stream: &str,
    mark: u64,
) -> Result<()> {
    loom.stream_set_retained_low_water_mark(ns, stream, mark)
}

pub fn consumer_change_cursor<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    stream: &str,
    consumer_id: &str,
) -> Result<ChangeCursor> {
    let next = consumer_position(loom, ns, stream, consumer_id)?;
    Ok(ChangeCursor::sequence(queue_change_scope(ns, stream), next))
}

pub fn consumer_change_set<S: ObjectStore>(
    loom: &Loom<S>,
    ns: WorkspaceId,
    stream: &str,
    consumer_id: &str,
    max: usize,
) -> Result<ChangeSet> {
    let start = consumer_position(loom, ns, stream, consumer_id)?;
    let records = consumer_read(loom, ns, stream, consumer_id, max)?;
    let next = start.saturating_add(records.len() as u64);
    let items = records
        .into_iter()
        .enumerate()
        .map(|(offset, payload)| ChangeItem::sequence_record(start + offset as u64, payload))
        .collect();
    ChangeSet::new(
        queue_change_scope(ns, stream),
        ChangeGapState::Retained,
        Some(retained_low_water_mark(loom, ns, stream)?),
        ChangeCursor::sequence(queue_change_scope(ns, stream), next),
        items,
    )
}

pub fn queue_change_scope(ns: WorkspaceId, stream: &str) -> String {
    format!("queue:{}:{stream}", hex::encode(ns.as_bytes()))
}

/// The stream names present in `ns`'s current working tree, sorted and de-duplicated. Enumeration is
/// within the workspace, not a global index. Reserved names beginning with `.` are excluded.
pub fn list_streams<S: ObjectStore>(loom: &Loom<S>, ns: WorkspaceId) -> Result<Vec<String>> {
    loom.authorize_collection(ns, FacetKind::Queue, "", AclRight::Read)?;
    let prefix = format!("{}/", facet_root(FacetKind::Queue));
    let mut out: Vec<String> = loom
        .staged_paths(ns)
        .into_iter()
        .filter_map(|p| {
            let rest = p.strip_prefix(&prefix)?;
            if rest.contains('/') || rest.starts_with('.') {
                return None;
            }
            Some(rest.to_string())
        })
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acl::{AclRight, AclSubject};
    use crate::error::Code;
    use crate::identity::IdentityStore;
    use crate::provider::memory::MemoryStore;
    use crate::workspace::{FacetKind, WorkspaceId};

    #[test]
    fn list_streams_enumerates_stream_names_sorted() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Queue, None, WorkspaceId::from_bytes([11; 16]))
            .unwrap();
        assert!(list_streams(&loom, ns).unwrap().is_empty());
        append(&mut loom, ns, "events", b"a").unwrap();
        append(&mut loom, ns, "audit", b"b").unwrap();
        append(&mut loom, ns, "events", b"c").unwrap();
        assert_eq!(list_streams(&loom, ns).unwrap(), vec!["audit", "events"]);
    }

    #[test]
    fn append_assigns_monotonic_seqs() {
        let mut s = Stream::new();
        assert_eq!(s.append(b"a".to_vec()), 0);
        assert_eq!(s.append(b"b".to_vec()), 1);
        assert_eq!(s.append(b"c".to_vec()), 2);
        assert_eq!(s.get(1), Some(&b"b"[..]));
        assert_eq!(s.get(9), None);
        assert_eq!(s.range(1, 3).len(), 2);
        assert_eq!(s.range(1, 99).len(), 2); // clamped
    }

    #[test]
    fn encode_round_trips_and_versions() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = loom
            .registry_mut()
            .create(FacetKind::Queue, None, WorkspaceId::from_bytes([6; 16]))
            .unwrap();
        let mut s = Stream::new();
        s.append(b"e0".to_vec());
        s.append(b"e1".to_vec());
        assert_eq!(Stream::decode(&s.encode()).unwrap().len(), 2);

        put_stream(&mut loom, ns, "events", &s).unwrap();
        let c1 = loom.commit(ns, "nas", "two entries", 1).unwrap();
        s.append(b"e2".to_vec());
        put_stream(&mut loom, ns, "events", &s).unwrap();
        loom.commit(ns, "nas", "three entries", 2).unwrap();
        assert_eq!(get_stream(&loom, ns, "events").unwrap().len(), 3);
        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(get_stream(&loom, ns, "events").unwrap().len(), 2);
    }

    fn queue_ns(loom: &mut Loom<MemoryStore>, seed: u8) -> WorkspaceId {
        loom.registry_mut()
            .create(FacetKind::Queue, None, WorkspaceId::from_bytes([seed; 16]))
            .unwrap()
    }

    #[test]
    fn structured_append_get_range_len_and_versioning() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut loom, 6);

        assert_eq!(append(&mut loom, ns, "events", b"e0").unwrap(), 0);
        assert_eq!(append(&mut loom, ns, "events", b"e1").unwrap(), 1);
        assert_eq!(append(&mut loom, ns, "events", b"e2").unwrap(), 2);
        assert_eq!(len(&loom, ns, "events").unwrap(), 3);
        assert_eq!(
            get(&loom, ns, "events", 1).unwrap().as_deref(),
            Some(&b"e1"[..])
        );
        assert_eq!(get(&loom, ns, "events", 9).unwrap(), None);
        assert_eq!(
            range(&loom, ns, "events", 1, 3).unwrap(),
            vec![b"e1".to_vec(), b"e2".to_vec()]
        );
        assert_eq!(range(&loom, ns, "events", 1, 99).unwrap().len(), 2);

        let c1 = loom.commit(ns, "nas", "three entries", 1).unwrap();
        assert_eq!(append(&mut loom, ns, "events", b"e3").unwrap(), 3);
        loom.commit(ns, "nas", "four entries", 2).unwrap();
        assert_eq!(len(&loom, ns, "events").unwrap(), 4);

        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(len(&loom, ns, "events").unwrap(), 3);
        assert_eq!(
            get(&loom, ns, "events", 0).unwrap().as_deref(),
            Some(&b"e0"[..])
        );
        let all = get_stream(&loom, ns, "events").unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all.get(2), Some(&b"e2"[..]));
    }

    #[test]
    fn clone_preserves_structured_stream() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut src, 1);
        for i in 0..5u8 {
            append(&mut src, ns, "events", &[b'e', b'0' + i]).unwrap();
        }
        let tip = src.commit(ns, "nas", "five", 1).unwrap();

        let mut dst = Loom::new(MemoryStore::new());
        let (dst_ns, report) =
            crate::sync::clone_workspace(&src, ns, &mut dst, WorkspaceId::from_bytes([2; 16]))
                .unwrap();
        assert!(report.objects_transferred > 0);
        dst.checkout_commit(dst_ns, tip).unwrap();
        assert_eq!(len(&dst, dst_ns, "events").unwrap(), 5);
        assert_eq!(
            range(&dst, dst_ns, "events", 0, 5).unwrap(),
            (0..5u8).map(|i| vec![b'e', b'0' + i]).collect::<Vec<_>>()
        );
    }

    #[test]
    fn bundle_preserves_structured_stream() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut src, 1);
        for i in 0..4u8 {
            append(&mut src, ns, "events", &[b'x', b'0' + i]).unwrap();
        }
        let tip = src.commit(ns, "nas", "four", 1).unwrap();

        let bundle = crate::sync::bundle_export(&src, ns).unwrap();
        let mut dst = Loom::new(MemoryStore::new());
        let (dst_ns, _) = crate::sync::bundle_import(&mut dst, &bundle).unwrap();
        dst.checkout_commit(dst_ns, tip).unwrap();
        assert_eq!(
            get(&dst, dst_ns, "events", 3).unwrap().as_deref(),
            Some(&b"x3"[..])
        );
        assert_eq!(get_stream(&dst, dst_ns, "events").unwrap().len(), 4);
    }

    /// A payload comfortably above the chunk threshold, varied so it splits into several content-defined
    /// chunks (a ChunkList, not a single Blob).
    fn large_payload(seed: u8) -> Vec<u8> {
        let len = 5 * crate::chunk::CHUNK_THRESHOLD;
        (0..len)
            .map(|i| (i.wrapping_mul(2_654_435_761).wrapping_add(seed as usize) % 251) as u8)
            .collect()
    }

    #[test]
    fn clone_preserves_chunked_stream_payload() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut src, 1);
        let big = large_payload(7);
        append(&mut src, ns, "events", &big).unwrap();
        let tip = src.commit(ns, "nas", "big", 1).unwrap();

        let mut dst = Loom::new(MemoryStore::new());
        let (dst_ns, _) =
            crate::sync::clone_workspace(&src, ns, &mut dst, WorkspaceId::from_bytes([2; 16]))
                .unwrap();
        dst.checkout_commit(dst_ns, tip).unwrap();
        assert_eq!(get(&dst, dst_ns, "events", 0).unwrap(), Some(big));
    }

    #[test]
    fn bundle_preserves_chunked_stream_payload() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut src, 1);
        let big = large_payload(9);
        append(&mut src, ns, "events", &big).unwrap();
        let tip = src.commit(ns, "nas", "big", 1).unwrap();

        let bundle = crate::sync::bundle_export(&src, ns).unwrap();
        let mut dst = Loom::new(MemoryStore::new());
        let (dst_ns, _) = crate::sync::bundle_import(&mut dst, &bundle).unwrap();
        dst.checkout_commit(dst_ns, tip).unwrap();
        assert_eq!(get(&dst, dst_ns, "events", 0).unwrap(), Some(big));
    }

    #[test]
    fn range_reads_mixed_small_and_large_payloads_after_clone() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut src, 1);
        let big0 = large_payload(1);
        let big2 = large_payload(2);
        append(&mut src, ns, "events", &big0).unwrap();
        append(&mut src, ns, "events", b"small").unwrap();
        append(&mut src, ns, "events", &big2).unwrap();
        append(&mut src, ns, "events", b"tail").unwrap();
        let tip = src.commit(ns, "nas", "mixed", 1).unwrap();

        let mut dst = Loom::new(MemoryStore::new());
        let (dst_ns, _) =
            crate::sync::clone_workspace(&src, ns, &mut dst, WorkspaceId::from_bytes([3; 16]))
                .unwrap();
        dst.checkout_commit(dst_ns, tip).unwrap();
        assert_eq!(
            range(&dst, dst_ns, "events", 0, 3).unwrap(),
            vec![big0, b"small".to_vec(), big2]
        );
        assert_eq!(
            get(&dst, dst_ns, "events", 3).unwrap().as_deref(),
            Some(&b"tail"[..])
        );
    }

    fn seed_stream(loom: &mut Loom<MemoryStore>, ns: WorkspaceId, n: u8) {
        for i in 0..n {
            append(loom, ns, "events", &[b'e', b'0' + i]).unwrap();
        }
    }

    #[test]
    fn missing_offset_is_zero_and_read_does_not_advance() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut loom, 1);
        seed_stream(&mut loom, ns, 3);

        assert_eq!(consumer_position(&loom, ns, "events", "worker").unwrap(), 0);
        // A bounded read from the stored position does not move it; rereads redeliver the same entries.
        let first = consumer_read(&loom, ns, "events", "worker", 2).unwrap();
        assert_eq!(first, vec![b"e0".to_vec(), b"e1".to_vec()]);
        assert_eq!(consumer_position(&loom, ns, "events", "worker").unwrap(), 0);
        let again = consumer_read(&loom, ns, "events", "worker", 2).unwrap();
        assert_eq!(
            again, first,
            "read without advance must redeliver the same entries"
        );
    }

    #[test]
    fn advance_persists_after_reopen_and_rejects_backward() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut loom, 1);
        seed_stream(&mut loom, ns, 3);

        consumer_advance(&mut loom, ns, "events", "worker", 2).unwrap();
        assert_eq!(consumer_position(&loom, ns, "events", "worker").unwrap(), 2);
        assert_eq!(
            consumer_read(&loom, ns, "events", "worker", 10).unwrap(),
            vec![b"e2".to_vec()]
        );

        // Backward advance is rejected; forward past the length is rejected.
        assert!(consumer_advance(&mut loom, ns, "events", "worker", 1).is_err());
        assert!(consumer_advance(&mut loom, ns, "events", "worker", 99).is_err());

        // Progress survives an export/import round trip (the local engine state).
        let state = loom.export_state();
        let mut reopened = Loom::new(MemoryStore::new());
        reopened.import_state(&state).unwrap();
        assert_eq!(
            consumer_position(&reopened, ns, "events", "worker").unwrap(),
            2
        );
    }

    #[test]
    fn reset_can_move_backward() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut loom, 1);
        seed_stream(&mut loom, ns, 3);
        consumer_advance(&mut loom, ns, "events", "worker", 3).unwrap();
        consumer_reset(&mut loom, ns, "events", "worker", 1).unwrap();
        assert_eq!(consumer_position(&loom, ns, "events", "worker").unwrap(), 1);
        assert_eq!(
            consumer_read(&loom, ns, "events", "worker", 10).unwrap(),
            vec![b"e1".to_vec(), b"e2".to_vec()]
        );
    }

    #[test]
    fn retained_low_water_mark_reports_gap_and_changeset_anchor() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut loom, 1);
        seed_stream(&mut loom, ns, 3);

        set_retained_low_water_mark(&mut loom, ns, "events", 2).unwrap();

        assert_eq!(
            consumer_position(&loom, ns, "events", "worker")
                .unwrap_err()
                .code,
            Code::RetainedGap
        );
        assert_eq!(
            consumer_read(&loom, ns, "events", "worker", 10)
                .unwrap_err()
                .code,
            Code::RetainedGap
        );
        assert_eq!(
            consumer_reset(&mut loom, ns, "events", "worker", 1)
                .unwrap_err()
                .code,
            Code::RetainedGap
        );

        consumer_reset(&mut loom, ns, "events", "worker", 2).unwrap();
        let changes = consumer_change_set(&loom, ns, "events", "worker", 10).unwrap();
        assert_eq!(changes.gap_state, ChangeGapState::Retained);
        assert_eq!(changes.retained_low_water_mark, Some(2));
        assert_eq!(changes.items.len(), 1);
        assert_eq!(changes.items[0].sequence, Some(2));
        assert_eq!(changes.items[0].payload.as_deref(), Some(&b"e2"[..]));
    }

    #[test]
    fn checkout_does_not_mutate_offset() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut loom, 1);
        seed_stream(&mut loom, ns, 2);
        let c1 = loom.commit(ns, "nas", "two", 1).unwrap();
        consumer_advance(&mut loom, ns, "events", "worker", 2).unwrap();
        append(&mut loom, ns, "events", b"e2").unwrap();
        loom.commit(ns, "nas", "three", 2).unwrap();
        loom.checkout_commit(ns, c1).unwrap();
        assert_eq!(consumer_position(&loom, ns, "events", "worker").unwrap(), 2);
    }

    #[test]
    fn clone_and_bundle_do_not_transfer_offsets() {
        let mut src = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut src, 1);
        seed_stream(&mut src, ns, 3);
        src.commit(ns, "nas", "three", 1).unwrap();
        consumer_advance(&mut src, ns, "events", "worker", 3).unwrap();

        let mut cloned = Loom::new(MemoryStore::new());
        let (clone_ns, _) =
            crate::sync::clone_workspace(&src, ns, &mut cloned, WorkspaceId::from_bytes([2; 16]))
                .unwrap();
        assert_eq!(
            consumer_position(&cloned, clone_ns, "events", "worker").unwrap(),
            0
        );

        let bundle = crate::sync::bundle_export(&src, ns).unwrap();
        let mut imported = Loom::new(MemoryStore::new());
        let (imp_ns, _) = crate::sync::bundle_import(&mut imported, &bundle).unwrap();
        assert_eq!(
            consumer_position(&imported, imp_ns, "events", "worker").unwrap(),
            0
        );
    }

    #[test]
    fn invalid_consumer_ids_and_stream_names_are_rejected() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut loom, 1);
        seed_stream(&mut loom, ns, 1);
        assert!(consumer_position(&loom, ns, "events", "").is_err());
        assert!(consumer_position(&loom, ns, "events", "a/b").is_err());
        assert!(consumer_position(&loom, ns, "events", "a\u{0}b").is_err());
        assert!(consumer_position(&loom, ns, "", "worker").is_err());
        assert!(consumer_position(&loom, ns, "../escape", "worker").is_err());
        assert!(consumer_advance(&mut loom, ns, "events", "", 0).is_err());
    }

    #[test]
    fn authenticated_queue_operations_are_acl_checked() {
        let mut loom = Loom::new(MemoryStore::new());
        let ns = queue_ns(&mut loom, 1);
        let root = WorkspaceId::from_bytes([1; 16]);
        let mut identity = IdentityStore::new(root);
        identity.set_passphrase(root, "root", b"12345678").unwrap();
        let session = identity
            .authenticate_passphrase(root, "root", "session")
            .unwrap();
        loom.set_identity_store(identity);
        loom.set_session(session.id);

        assert_eq!(
            append(&mut loom, ns, "events", b"e0").unwrap_err().code,
            Code::PermissionDenied
        );

        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Queue),
                [AclRight::Write],
            )
            .unwrap();
        assert_eq!(append(&mut loom, ns, "events", b"e0").unwrap(), 0);
        assert_eq!(
            get(&loom, ns, "events", 0).unwrap_err().code,
            Code::PermissionDenied
        );
        assert_eq!(
            consumer_position(&loom, ns, "events", "worker")
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );

        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Queue),
                [AclRight::Read],
            )
            .unwrap();
        assert_eq!(
            get(&loom, ns, "events", 0).unwrap().as_deref(),
            Some(&b"e0"[..])
        );
        assert_eq!(
            consumer_read(&loom, ns, "events", "worker", 1).unwrap(),
            vec![b"e0".to_vec()]
        );
        assert_eq!(
            consumer_advance(&mut loom, ns, "events", "worker", 1)
                .unwrap_err()
                .code,
            Code::PermissionDenied
        );

        loom.acl_store_mut()
            .allow(
                AclSubject::Principal(root),
                Some(ns),
                Some(FacetKind::Queue),
                [AclRight::Advance],
            )
            .unwrap();
        consumer_advance(&mut loom, ns, "events", "worker", 1).unwrap();
        assert_eq!(consumer_position(&loom, ns, "events", "worker").unwrap(), 1);
        consumer_reset(&mut loom, ns, "events", "worker", 0).unwrap();
    }
}
