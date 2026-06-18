//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

// On-disk layout (page engine):
//   [0,8) magic  [8,10) format_major  [10,12) format_minor  [12,20) generation  [20,21) digest_algo
//   [21,29) page_count  [29,30) region_table_present  [30,38) region_table page id
//   [38,39) reference_present  [39,71) reference digest
//   [71,72) control_present  [72,104) control digest
//   [104,105) encryption_present  [105,107) encryption_meta length (u16 LE)
//   [107, 107+len) encryption_meta (set only at creation / rekey)
//   [107+len,4092) reserved (zeroed)  [4092,4096) crc32c over [0,4092)
#[derive(Clone)]
pub(crate) struct Superblock {
    pub(crate) generation: u64,
    pub(crate) page_count: u64,
    // The identity-profile digest algorithm, stored at offset [20,21). Chosen at
    // creation, immutable; every object address in the store uses it.
    pub(crate) digest_algo: Algo,
    pub(crate) region_table: Option<PageId>, // page holding the region roots, None = empty store
    pub(crate) reference: Option<[u8; 32]>,  // engine-state root object digest, if any
    pub(crate) control: Option<[u8; 32]>, // durable-local control-plane root object digest, if any
    // The encoded `encryption_meta`: wrapped DEK + KDF salt + active suite. Immutable
    // after creation (changed only by rekey), so it is NOT part of the per-commit journal `Roots`;
    // instead every superblock write (fresh init, checkpoint, compaction) carries it forward, and the
    // journal-recovery fold preserves it from the checkpoint slot. `None` = an unencrypted Loom.
    pub(crate) encryption: Option<Vec<u8>>,
}

impl Superblock {
    pub(crate) fn encode(&self) -> [u8; SLOT_SIZE as usize] {
        let mut s = [0u8; SLOT_SIZE as usize];
        s[0..8].copy_from_slice(MAGIC);
        s[8..10].copy_from_slice(&FORMAT_MAJOR.to_le_bytes());
        s[10..12].copy_from_slice(&FORMAT_MINOR.to_le_bytes());
        s[12..20].copy_from_slice(&self.generation.to_le_bytes());
        s[20] = self.digest_algo.code();
        s[21..29].copy_from_slice(&self.page_count.to_le_bytes());
        if let Some(PageId(id)) = self.region_table {
            s[29] = 1;
            s[30..38].copy_from_slice(&id.to_le_bytes());
        }
        if let Some(reference) = self.reference {
            s[38] = 1;
            s[39..71].copy_from_slice(&reference);
        }
        if let Some(control) = self.control {
            s[71] = 1;
            s[72..104].copy_from_slice(&control);
        }
        if let Some(enc) = &self.encryption {
            // The metadata is tiny (~100 B); it must fit the reserved span under the CRC.
            debug_assert!(
                107 + enc.len() <= CRC_OFFSET,
                "encryption_meta overflows superblock"
            );
            s[104] = 1;
            s[105..107].copy_from_slice(&(enc.len() as u16).to_le_bytes());
            s[107..107 + enc.len()].copy_from_slice(enc);
        }
        let crc = crc32c(&s[0..CRC_OFFSET]);
        s[CRC_OFFSET..CRC_OFFSET + 4].copy_from_slice(&crc.to_le_bytes());
        s
    }

    pub(crate) fn decode(s: &[u8; SLOT_SIZE as usize]) -> Option<Self> {
        if &s[0..8] != MAGIC {
            return None;
        }
        if u16::from_le_bytes([s[8], s[9]]) != FORMAT_MAJOR {
            return None; // unknown major: refuse (caller treats as "no valid slot")
        }
        // An unknown digest-algo code is a forward-version store we cannot address; treat the slot as
        // unreadable (same contract as an unknown major).
        let digest_algo = Algo::from_code(s[20]).ok()?;
        let stored_crc = u32::from_le_bytes([
            s[CRC_OFFSET],
            s[CRC_OFFSET + 1],
            s[CRC_OFFSET + 2],
            s[CRC_OFFSET + 3],
        ]);
        if crc32c(&s[0..CRC_OFFSET]) != stored_crc {
            return None; // torn / corrupt slot
        }
        let generation = u64::from_le_bytes(s[12..20].try_into().ok()?);
        let page_count = u64::from_le_bytes(s[21..29].try_into().ok()?);
        let region_table = match s[29] {
            0 => None,
            _ => {
                let id = u64::from_le_bytes(s[30..38].try_into().ok()?);
                if id >= page_count {
                    return None; // the region-table page must lie within the committed page array
                }
                Some(PageId(id))
            }
        };
        let reference = if s[38] == 1 {
            let mut d = [0u8; 32];
            d.copy_from_slice(&s[39..71]);
            Some(d)
        } else {
            None
        };
        let control = if s[71] == 1 {
            let mut d = [0u8; 32];
            d.copy_from_slice(&s[72..104]);
            Some(d)
        } else {
            None
        };
        let encryption = if s[104] == 1 {
            let len = u16::from_le_bytes([s[105], s[106]]) as usize;
            if 107 + len > CRC_OFFSET {
                return None; // length runs past the reserved span: malformed
            }
            Some(s[107..107 + len].to_vec())
        } else {
            None
        };
        Some(Self {
            generation,
            page_count,
            digest_algo,
            region_table,
            reference,
            control,
            encryption,
        })
    }
}
