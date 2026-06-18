//! The write-ahead journal: a redo log of the committed root-set. Each commit fsyncs its record into
//! a ring slot; that fsync is the commit point, since the referenced pages are already durable above
//! it. The superblock is only a periodic checkpoint, so recovery scans the ring for the newest valid
//! record and adopts it when it is ahead of the superblock.
//!
//! The ring keeps the newest records each in their own slot, so a torn write of one commit's record
//! cannot destroy an earlier acked commit (a single slot would), and the superblock fsync can be
//! amortized across many commits.

use crate::crc32c;
use crate::page::PageId;

const J_MAGIC: &[u8; 4] = b"JRNL";

/// Journal record kinds, tagging a record's role in a transaction's lifecycle. A put
/// commits via the region-table-page swap and records its root-set as a `COMMIT`; recovery adopts
/// only a `COMMIT`'s roots. `PREPARE`/`ABORT` bracket a transaction's page-extent intent and
/// `CHECKPOINT` marks a folded superblock generation.
pub(crate) const KIND_PREPARE: u8 = 0;
pub(crate) const KIND_COMMIT: u8 = 1;
pub(crate) const KIND_ABORT: u8 = 2;
pub(crate) const KIND_CHECKPOINT: u8 = 3;

/// One journal record on disk.
pub(crate) const RECORD_SIZE: usize = 4 + 1 + 8 + 8 + 1 + 8 + 1 + 32 + 1 + 32 + 4;
const CRC_AT: usize = RECORD_SIZE - 4;

/// The committed root-set a transaction produces: enough to reconstruct the superblock on replay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Roots {
    pub generation: u64,
    pub page_count: u64,
    pub region_table: Option<PageId>, // None = empty store (no committed pages)
    pub reference: Option<[u8; 32]>,  // engine-state root object digest, if any
    pub control: Option<[u8; 32]>,    // durable-local control-plane root, if any
}

/// Encode a `COMMIT` record for `roots`.
pub(crate) fn encode_commit(roots: &Roots) -> [u8; RECORD_SIZE] {
    let mut r = [0u8; RECORD_SIZE];
    r[0..4].copy_from_slice(J_MAGIC);
    r[4] = KIND_COMMIT;
    r[5..13].copy_from_slice(&roots.generation.to_le_bytes());
    r[13..21].copy_from_slice(&roots.page_count.to_le_bytes());
    if let Some(PageId(id)) = roots.region_table {
        r[21] = 1;
        r[22..30].copy_from_slice(&id.to_le_bytes());
    }
    if let Some(reference) = roots.reference {
        r[30] = 1;
        r[31..63].copy_from_slice(&reference);
    }
    if let Some(control) = roots.control {
        r[63] = 1;
        r[64..96].copy_from_slice(&control);
    }
    let crc = crc32c(&r[0..CRC_AT]);
    r[CRC_AT..RECORD_SIZE].copy_from_slice(&crc.to_le_bytes());
    r
}

/// Decode a journal record, or `None` if `buf` is too short, has a bad magic, or fails its CRC.
pub(crate) fn decode(buf: &[u8]) -> Option<(u8, Roots)> {
    if buf.len() < RECORD_SIZE || &buf[0..4] != J_MAGIC {
        return None;
    }
    let stored_crc = u32::from_le_bytes(buf[CRC_AT..RECORD_SIZE].try_into().ok()?);
    if crc32c(&buf[0..CRC_AT]) != stored_crc {
        return None;
    }
    let kind = buf[4];
    if !matches!(
        kind,
        KIND_PREPARE | KIND_COMMIT | KIND_ABORT | KIND_CHECKPOINT
    ) {
        return None; // unknown record kind
    }
    let generation = u64::from_le_bytes(buf[5..13].try_into().ok()?);
    let page_count = u64::from_le_bytes(buf[13..21].try_into().ok()?);
    let region_table = match buf[21] {
        0 => None,
        _ => Some(PageId(u64::from_le_bytes(buf[22..30].try_into().ok()?))),
    };
    let reference = if buf[30] == 1 {
        let mut d = [0u8; 32];
        d.copy_from_slice(&buf[31..63]);
        Some(d)
    } else {
        None
    };
    let control = if buf[63] == 1 {
        let mut d = [0u8; 32];
        d.copy_from_slice(&buf[64..96]);
        Some(d)
    } else {
        None
    };
    Some((
        kind,
        Roots {
            generation,
            page_count,
            region_table,
            reference,
            control,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(generation: u64, reference: Option<[u8; 32]>) -> Roots {
        Roots {
            generation,
            page_count: 42,
            region_table: Some(PageId(17)),
            reference,
            control: Some([9; 32]),
        }
    }

    #[test]
    fn commit_record_round_trips() {
        for reference in [None, Some([7u8; 32])] {
            let r = roots(42, reference);
            let bytes = encode_commit(&r);
            let (kind, back) = decode(&bytes).unwrap();
            assert_eq!(kind, KIND_COMMIT);
            assert_eq!(back, r);
        }
    }

    #[test]
    fn empty_region_table_round_trips() {
        let r = Roots {
            generation: 0,
            page_count: 0,
            region_table: None,
            reference: None,
            control: None,
        };
        assert_eq!(decode(&encode_commit(&r)).unwrap().1, r);
    }

    #[test]
    fn zeroed_or_short_region_has_no_record() {
        assert!(decode(&[0u8; RECORD_SIZE]).is_none()); // fresh region: no magic
        assert!(decode(&[]).is_none());
        assert!(decode(&encode_commit(&roots(1, None))[..RECORD_SIZE - 1]).is_none()); // truncated
    }

    #[test]
    fn a_flipped_bit_fails_the_crc() {
        let mut bytes = encode_commit(&roots(5, None));
        bytes[10] ^= 1;
        assert!(decode(&bytes).is_none());
    }
}
