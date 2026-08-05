//! Page addressing and the region table.
//!
//! The file is a fixed header followed by an array of `PAGE_SIZE`-byte pages; a [`PageId`] names one
//! page by its zero-based index in that array. The region table is the one page the superblock and
//! journal record point at: it carries the root page of each page-structured region (the object index
//! and the free-page map) plus the open segment and page size, so a single pointer locates the engine
//! state. The engine-state (reference) root is a content digest, not a page, so it rides in the
//! superblock and journal record directly rather than here.

use crate::crc32c;
use crate::pagebtree::ValueCodecKind;

/// Size in bytes of one page. Locked at 4 KiB for the major-1 file layout by the D-1 benchmark in
/// `prototypes/page-store`: smaller pages cannot hold an index node, while larger pages waste slab
/// space and amplify reads and writes on small-object workloads.
pub(crate) const PAGE_SIZE: u64 = 4096;

/// Target bytes per segment: a logical group of record pages tracked for garbage collection.
pub(crate) const SEGMENT_BYTES: u64 = 64 * 1024 * 1024;

/// Record pages per segment. A record's segment id is its global page index divided by this; its
/// in-segment page index is the remainder.
pub(crate) const PAGES_PER_SEGMENT: u64 = SEGMENT_BYTES / PAGE_SIZE;

/// On-disk size of an encoded region table: `magic(1) page_size(8) 4*root{flag(1) id(8)}
/// open_segment(8) crc32c(4)`.
pub(crate) const REGION_TABLE_LEN: usize = 1 + 8 + 4 * 9 + 8 + 4;
const LEGACY_REGION_TABLE_LEN: usize = 1 + 8 + 3 * 9 + 8 + 4;

const _: () = assert!(REGION_TABLE_LEN as u64 <= PAGE_SIZE);

const REGION_TABLE_MAGIC_V2: u8 = 0xB6;
const REGION_TABLE_MAGIC_V3: u8 = 0xB8;
pub(crate) const CANONICAL_REGION_TABLE_LEN: usize = PAGE_SIZE as usize;
pub(crate) const ROOT_CATALOG_LEN: usize = PAGE_SIZE as usize;
pub(crate) const ROOT_CATALOG_ENTRY_SIZE: usize = 32;
pub(crate) const ROOT_CATALOG_MAX_ENTRIES: usize =
    (ROOT_CATALOG_LEN - 32 - 4) / ROOT_CATALOG_ENTRY_SIZE;

const CANONICAL_REGION_TABLE_MAGIC: &[u8; 4] = b"LRT5";
const METADATA_BOOTSTRAP_HEADER_END: usize = 96;
const METADATA_BOOTSTRAP_EXTENT_SIZE: usize = 16;
const MINIMUM_RECOVERABLE_GENERATION_AT: usize = CANONICAL_REGION_TABLE_LEN - 12;
pub(crate) const METADATA_BOOTSTRAP_MAX_EXTENTS: usize = (MINIMUM_RECOVERABLE_GENERATION_AT
    - METADATA_BOOTSTRAP_HEADER_END)
    / METADATA_BOOTSTRAP_EXTENT_SIZE;
const ROOT_CATALOG_MAGIC: &[u8; 8] = b"LROOTC1\0";
pub(crate) const ROOT_FLAG_AUTHORITATIVE: u16 = 0x0001;
pub(crate) const ROOT_FLAG_ADVISORY: u16 = 0x0002;
pub(crate) const CURRENT_RECORDS_FAMILY_ID: u16 = 0x0001;
pub(crate) const RETAINED_HISTORY_FAMILY_ID: u16 = 0x0100;
pub(crate) const OWNER_TOKEN_FAMILY_ID: u16 = 0x0110;
pub(crate) const SECONDARY_INDEX_FAMILY_ID: u16 = 0x0120;
pub(crate) const MUTABLE_IDEMPOTENCY_FAMILY_ID: u16 = 0x0130;
pub(crate) const WORKFLOW_IDEMPOTENCY_FAMILY_ID: u16 = 0x0131;
pub(crate) const AUDIT_RETENTION_FAMILY_ID: u16 = 0x0140;
pub(crate) const MVCC_GENERATION_FAMILY_ID: u16 = 0x0200;
pub(crate) const RETENTION_INDEX_FAMILY_ID: u16 = 0x0210;
pub(crate) const CHECKPOINT_INDEX_FAMILY_ID: u16 = 0x0220;
pub(crate) const RECLAIM_INDEX_FAMILY_ID: u16 = 0x0230;
pub(crate) const DELTA_PACK_CANDIDATE_FAMILY_ID: u16 = 0x0300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootCodecError {
    WrongLength { expected: usize, actual: usize },
    BadMagic,
    BadVersion,
    BadLayoutLength,
    BadPageSize,
    CrcMismatch,
    NonZeroReserved,
    BadRootSlot,
    AbsentEntryRoot,
    PresentZeroPageId,
    EntryCountTooLarge,
    BadEntrySize,
    BadCatalogFlags,
    DuplicateOrUnsortedFamily,
    BadFamilyFlags,
    KnownFamilyFlagMismatch,
    DirectRegionTableFamilyInCatalog,
    UnknownAuthoritativeFamily,
    PageIdOutOfBounds { page_id: u64, page_count: u64 },
    LegacyOverlayRoot,
    BadMetadataBootstrapCapacity,
    BadMetadataBootstrapExtentCount,
    BadMetadataBootstrapExtent,
    MetadataBootstrapGenerationMismatch { expected: u64, actual: u64 },
    RecoveryGenerationFloorBeyondCommit { floor: u64, generation: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MetadataBootstrapExtent {
    pub(crate) start: u64,
    pub(crate) len: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MetadataBootstrapReserve {
    pub(crate) owning_generation: u64,
    pub(crate) capacity: u64,
    pub(crate) extents: Vec<MetadataBootstrapExtent>,
}

impl MetadataBootstrapReserve {
    pub(crate) fn page_count(&self) -> u64 {
        self.extents
            .iter()
            .fold(0u64, |total, extent| total.saturating_add(extent.len))
    }

    pub(crate) fn validate(&self, page_count: u64) -> Result<(), RootCodecError> {
        if self.capacity == 0 || self.page_count() > self.capacity {
            return Err(RootCodecError::BadMetadataBootstrapCapacity);
        }
        if self.extents.len() > METADATA_BOOTSTRAP_MAX_EXTENTS {
            return Err(RootCodecError::BadMetadataBootstrapExtentCount);
        }
        let mut previous_end = None;
        for extent in &self.extents {
            let end = extent
                .start
                .checked_add(extent.len)
                .ok_or(RootCodecError::BadMetadataBootstrapExtent)?;
            if extent.len == 0 || end > page_count {
                return Err(RootCodecError::BadMetadataBootstrapExtent);
            }
            if previous_end.is_some_and(|prior| extent.start <= prior) {
                return Err(RootCodecError::BadMetadataBootstrapExtent);
            }
            previous_end = Some(end);
        }
        Ok(())
    }

    pub(crate) fn contains_page(&self, page: u64) -> bool {
        self.extents
            .iter()
            .any(|extent| page >= extent.start && page < extent.start.saturating_add(extent.len))
    }

    pub(crate) fn pages(&self) -> impl Iterator<Item = u64> + '_ {
        self.extents
            .iter()
            .flat_map(|extent| extent.start..extent.start.saturating_add(extent.len))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootFamilyLocation {
    DirectRegionTable,
    RootCatalog,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootFamilyRole {
    CurrentState,
    RetainedControl,
    RebuildableAdvisory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootFamilyOpenHydration {
    DirectCurrent,
    RootOnly,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootFamilyAbsence {
    EmptyFamily,
    OptionalAdvisory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RootFamilyReachability {
    SemanticRoot,
    ControlRoot,
    PhysicalSafetyRoot,
    AdvisoryPreserveOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RootFamilyDescriptor {
    pub(crate) family_id: u16,
    pub(crate) name: &'static str,
    pub(crate) location: RootFamilyLocation,
    pub(crate) flags: u16,
    pub(crate) role: RootFamilyRole,
    pub(crate) open_hydration: RootFamilyOpenHydration,
    pub(crate) absence: RootFamilyAbsence,
    pub(crate) gc_reachability: RootFamilyReachability,
    pub(crate) value_codec: ValueCodecKind,
}

pub(crate) const ROOT_FAMILY_REGISTRY: &[RootFamilyDescriptor] = &[
    RootFamilyDescriptor {
        family_id: CURRENT_RECORDS_FAMILY_ID,
        name: "current_records",
        location: RootFamilyLocation::DirectRegionTable,
        flags: ROOT_FLAG_AUTHORITATIVE,
        role: RootFamilyRole::CurrentState,
        open_hydration: RootFamilyOpenHydration::DirectCurrent,
        absence: RootFamilyAbsence::EmptyFamily,
        gc_reachability: RootFamilyReachability::SemanticRoot,
        value_codec: ValueCodecKind::RecordLoc,
    },
    RootFamilyDescriptor {
        family_id: RETAINED_HISTORY_FAMILY_ID,
        name: "retained_history",
        location: RootFamilyLocation::RootCatalog,
        flags: ROOT_FLAG_AUTHORITATIVE,
        role: RootFamilyRole::RetainedControl,
        open_hydration: RootFamilyOpenHydration::RootOnly,
        absence: RootFamilyAbsence::EmptyFamily,
        gc_reachability: RootFamilyReachability::SemanticRoot,
        value_codec: ValueCodecKind::PackedRecordRef,
    },
    RootFamilyDescriptor {
        family_id: OWNER_TOKEN_FAMILY_ID,
        name: "owner_tokens",
        location: RootFamilyLocation::RootCatalog,
        flags: ROOT_FLAG_AUTHORITATIVE,
        role: RootFamilyRole::RetainedControl,
        open_hydration: RootFamilyOpenHydration::RootOnly,
        absence: RootFamilyAbsence::EmptyFamily,
        gc_reachability: RootFamilyReachability::ControlRoot,
        value_codec: ValueCodecKind::PackedRecordRef,
    },
    RootFamilyDescriptor {
        family_id: SECONDARY_INDEX_FAMILY_ID,
        name: "secondary_indexes",
        location: RootFamilyLocation::RootCatalog,
        flags: ROOT_FLAG_AUTHORITATIVE,
        role: RootFamilyRole::RetainedControl,
        open_hydration: RootFamilyOpenHydration::RootOnly,
        absence: RootFamilyAbsence::EmptyFamily,
        gc_reachability: RootFamilyReachability::ControlRoot,
        value_codec: ValueCodecKind::PackedRecordRef,
    },
    RootFamilyDescriptor {
        family_id: MUTABLE_IDEMPOTENCY_FAMILY_ID,
        name: "mutable_idempotency",
        location: RootFamilyLocation::RootCatalog,
        flags: ROOT_FLAG_AUTHORITATIVE,
        role: RootFamilyRole::RetainedControl,
        open_hydration: RootFamilyOpenHydration::RootOnly,
        absence: RootFamilyAbsence::EmptyFamily,
        gc_reachability: RootFamilyReachability::ControlRoot,
        value_codec: ValueCodecKind::RecordLoc,
    },
    RootFamilyDescriptor {
        family_id: WORKFLOW_IDEMPOTENCY_FAMILY_ID,
        name: "workflow_idempotency",
        location: RootFamilyLocation::RootCatalog,
        flags: ROOT_FLAG_AUTHORITATIVE,
        role: RootFamilyRole::RetainedControl,
        open_hydration: RootFamilyOpenHydration::RootOnly,
        absence: RootFamilyAbsence::EmptyFamily,
        gc_reachability: RootFamilyReachability::ControlRoot,
        value_codec: ValueCodecKind::PackedRecordRef,
    },
    RootFamilyDescriptor {
        family_id: AUDIT_RETENTION_FAMILY_ID,
        name: "audit_retention",
        location: RootFamilyLocation::RootCatalog,
        flags: ROOT_FLAG_AUTHORITATIVE,
        role: RootFamilyRole::RetainedControl,
        open_hydration: RootFamilyOpenHydration::RootOnly,
        absence: RootFamilyAbsence::EmptyFamily,
        gc_reachability: RootFamilyReachability::SemanticRoot,
        value_codec: ValueCodecKind::RecordLoc,
    },
    RootFamilyDescriptor {
        family_id: MVCC_GENERATION_FAMILY_ID,
        name: "mvcc_generations",
        location: RootFamilyLocation::RootCatalog,
        flags: ROOT_FLAG_AUTHORITATIVE,
        role: RootFamilyRole::RetainedControl,
        open_hydration: RootFamilyOpenHydration::RootOnly,
        absence: RootFamilyAbsence::EmptyFamily,
        gc_reachability: RootFamilyReachability::ControlRoot,
        value_codec: ValueCodecKind::RecordLoc,
    },
    RootFamilyDescriptor {
        family_id: RETENTION_INDEX_FAMILY_ID,
        name: "retention_index",
        location: RootFamilyLocation::RootCatalog,
        flags: ROOT_FLAG_AUTHORITATIVE,
        role: RootFamilyRole::RetainedControl,
        open_hydration: RootFamilyOpenHydration::RootOnly,
        absence: RootFamilyAbsence::EmptyFamily,
        gc_reachability: RootFamilyReachability::SemanticRoot,
        value_codec: ValueCodecKind::RecordLoc,
    },
    RootFamilyDescriptor {
        family_id: CHECKPOINT_INDEX_FAMILY_ID,
        name: "checkpoint_index",
        location: RootFamilyLocation::RootCatalog,
        flags: ROOT_FLAG_AUTHORITATIVE,
        role: RootFamilyRole::RetainedControl,
        open_hydration: RootFamilyOpenHydration::RootOnly,
        absence: RootFamilyAbsence::EmptyFamily,
        gc_reachability: RootFamilyReachability::SemanticRoot,
        value_codec: ValueCodecKind::RecordLoc,
    },
    RootFamilyDescriptor {
        family_id: RECLAIM_INDEX_FAMILY_ID,
        name: "reclaim_index",
        location: RootFamilyLocation::RootCatalog,
        flags: ROOT_FLAG_AUTHORITATIVE,
        role: RootFamilyRole::RetainedControl,
        open_hydration: RootFamilyOpenHydration::RootOnly,
        absence: RootFamilyAbsence::EmptyFamily,
        gc_reachability: RootFamilyReachability::PhysicalSafetyRoot,
        value_codec: ValueCodecKind::RecordLoc,
    },
    RootFamilyDescriptor {
        family_id: DELTA_PACK_CANDIDATE_FAMILY_ID,
        name: "delta_pack_candidates",
        location: RootFamilyLocation::RootCatalog,
        flags: ROOT_FLAG_ADVISORY,
        role: RootFamilyRole::RebuildableAdvisory,
        open_hydration: RootFamilyOpenHydration::None,
        absence: RootFamilyAbsence::OptionalAdvisory,
        gc_reachability: RootFamilyReachability::AdvisoryPreserveOnly,
        value_codec: ValueCodecKind::RecordLoc,
    },
];

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

fn encode_root_slot(out: &mut [u8], root: Option<PageId>) {
    if let Some(PageId(id)) = root {
        out[0] = 1;
        out[1..9].copy_from_slice(&id.to_le_bytes());
    }
}

fn decode_root_slot(
    buf: &[u8],
    allow_absent: bool,
    reject_zero: bool,
) -> Result<Option<PageId>, RootCodecError> {
    match buf.first().copied().ok_or(RootCodecError::BadRootSlot)? {
        0 if allow_absent && buf.get(1..9).ok_or(RootCodecError::BadRootSlot)? == [0; 8] => {
            Ok(None)
        }
        0 if allow_absent => Err(RootCodecError::BadRootSlot),
        0 => Err(RootCodecError::AbsentEntryRoot),
        1 => {
            let id = u64::from_le_bytes(
                buf.get(1..9)
                    .ok_or(RootCodecError::BadRootSlot)?
                    .try_into()
                    .map_err(|_| RootCodecError::BadRootSlot)?,
            );
            if reject_zero && id == 0 {
                return Err(RootCodecError::PresentZeroPageId);
            }
            Ok(Some(PageId(id)))
        }
        _ => Err(RootCodecError::BadRootSlot),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalRegionTable {
    pub(crate) index_root: Option<PageId>,
    pub(crate) freemap_root: Option<PageId>,
    pub(crate) maintenance_root: Option<PageId>,
    pub(crate) current_record_root: Option<PageId>,
    pub(crate) root_catalog_root: Option<PageId>,
    pub(crate) open_segment: u64,
    pub(crate) mutable_overlay_generation_floor: u64,
    pub(crate) minimum_recoverable_generation: u64,
    pub(crate) metadata_bootstrap_reserve: MetadataBootstrapReserve,
}

impl CanonicalRegionTable {
    pub(crate) fn encode(
        &self,
        page_count: u64,
    ) -> Result<[u8; CANONICAL_REGION_TABLE_LEN], RootCodecError> {
        self.validate_root_bounds(page_count)?;
        let mut r = [0u8; CANONICAL_REGION_TABLE_LEN];
        r[0..4].copy_from_slice(CANONICAL_REGION_TABLE_MAGIC);
        r[4..6].copy_from_slice(&5u16.to_le_bytes());
        r[6..8].copy_from_slice(&(PAGE_SIZE as u16).to_le_bytes());
        r[8..16].copy_from_slice(&PAGE_SIZE.to_le_bytes());
        let mut p = 16;
        for root in [
            self.index_root,
            self.freemap_root,
            self.maintenance_root,
            self.current_record_root,
            self.root_catalog_root,
        ] {
            encode_root_slot(&mut r[p..p + 9], root);
            p += 9;
        }
        r[61..69].copy_from_slice(&self.open_segment.to_le_bytes());
        r[69..77].copy_from_slice(&self.mutable_overlay_generation_floor.to_le_bytes());
        r[77..85].copy_from_slice(&self.metadata_bootstrap_reserve.capacity.to_le_bytes());
        r[85..93].copy_from_slice(
            &self
                .metadata_bootstrap_reserve
                .owning_generation
                .to_le_bytes(),
        );
        let extent_count = u16::try_from(self.metadata_bootstrap_reserve.extents.len())
            .map_err(|_| RootCodecError::BadMetadataBootstrapExtentCount)?;
        r[93..95].copy_from_slice(&extent_count.to_le_bytes());
        let mut extent_pos = METADATA_BOOTSTRAP_HEADER_END;
        for extent in &self.metadata_bootstrap_reserve.extents {
            r[extent_pos..extent_pos + 8].copy_from_slice(&extent.start.to_le_bytes());
            r[extent_pos + 8..extent_pos + 16].copy_from_slice(&extent.len.to_le_bytes());
            extent_pos += METADATA_BOOTSTRAP_EXTENT_SIZE;
        }
        r[MINIMUM_RECOVERABLE_GENERATION_AT..MINIMUM_RECOVERABLE_GENERATION_AT + 8]
            .copy_from_slice(&self.minimum_recoverable_generation.to_le_bytes());
        let crc = crc32c(&r[..CANONICAL_REGION_TABLE_LEN - 4]);
        r[CANONICAL_REGION_TABLE_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
        Ok(r)
    }

    pub(crate) fn decode(buf: &[u8]) -> Result<CanonicalRegionTable, RootCodecError> {
        if buf.len() != CANONICAL_REGION_TABLE_LEN {
            return Err(RootCodecError::WrongLength {
                expected: CANONICAL_REGION_TABLE_LEN,
                actual: buf.len(),
            });
        }
        if &buf[0..4] != CANONICAL_REGION_TABLE_MAGIC {
            return Err(RootCodecError::BadMagic);
        }
        if u16::from_le_bytes(
            buf[4..6]
                .try_into()
                .map_err(|_| RootCodecError::BadVersion)?,
        ) != 5
        {
            return Err(RootCodecError::BadVersion);
        }
        if u16::from_le_bytes(
            buf[6..8]
                .try_into()
                .map_err(|_| RootCodecError::BadLayoutLength)?,
        ) != PAGE_SIZE as u16
        {
            return Err(RootCodecError::BadLayoutLength);
        }
        if u64::from_le_bytes(
            buf[8..16]
                .try_into()
                .map_err(|_| RootCodecError::BadPageSize)?,
        ) != PAGE_SIZE
        {
            return Err(RootCodecError::BadPageSize);
        }
        let stored = u32::from_le_bytes(
            buf[CANONICAL_REGION_TABLE_LEN - 4..CANONICAL_REGION_TABLE_LEN]
                .try_into()
                .map_err(|_| RootCodecError::CrcMismatch)?,
        );
        if crc32c(&buf[..CANONICAL_REGION_TABLE_LEN - 4]) != stored {
            return Err(RootCodecError::CrcMismatch);
        }
        let extent_count = usize::from(u16::from_le_bytes(
            buf[93..95]
                .try_into()
                .map_err(|_| RootCodecError::BadMetadataBootstrapExtentCount)?,
        ));
        if extent_count > METADATA_BOOTSTRAP_MAX_EXTENTS {
            return Err(RootCodecError::BadMetadataBootstrapExtentCount);
        }
        let extent_end =
            METADATA_BOOTSTRAP_HEADER_END + extent_count * METADATA_BOOTSTRAP_EXTENT_SIZE;
        if buf[95] != 0
            || buf[extent_end..MINIMUM_RECOVERABLE_GENERATION_AT]
                .iter()
                .any(|b| *b != 0)
        {
            return Err(RootCodecError::NonZeroReserved);
        }
        let mut p = 16;
        let mut roots = [None; 5];
        for slot in &mut roots {
            *slot = decode_root_slot(&buf[p..p + 9], true, false)?;
            p += 9;
        }
        let mut extents = Vec::with_capacity(extent_count);
        let mut extent_pos = METADATA_BOOTSTRAP_HEADER_END;
        for _ in 0..extent_count {
            extents.push(MetadataBootstrapExtent {
                start: u64::from_le_bytes(
                    buf[extent_pos..extent_pos + 8]
                        .try_into()
                        .map_err(|_| RootCodecError::BadMetadataBootstrapExtent)?,
                ),
                len: u64::from_le_bytes(
                    buf[extent_pos + 8..extent_pos + 16]
                        .try_into()
                        .map_err(|_| RootCodecError::BadMetadataBootstrapExtent)?,
                ),
            });
            extent_pos += METADATA_BOOTSTRAP_EXTENT_SIZE;
        }
        Ok(CanonicalRegionTable {
            index_root: roots[0],
            freemap_root: roots[1],
            maintenance_root: roots[2],
            current_record_root: roots[3],
            root_catalog_root: roots[4],
            open_segment: u64::from_le_bytes(
                buf[61..69]
                    .try_into()
                    .map_err(|_| RootCodecError::BadRootSlot)?,
            ),
            mutable_overlay_generation_floor: u64::from_le_bytes(
                buf[69..77]
                    .try_into()
                    .map_err(|_| RootCodecError::BadRootSlot)?,
            ),
            minimum_recoverable_generation: u64::from_le_bytes(
                buf[MINIMUM_RECOVERABLE_GENERATION_AT..MINIMUM_RECOVERABLE_GENERATION_AT + 8]
                    .try_into()
                    .map_err(|_| RootCodecError::BadRootSlot)?,
            ),
            metadata_bootstrap_reserve: MetadataBootstrapReserve {
                owning_generation: u64::from_le_bytes(
                    buf[85..93]
                        .try_into()
                        .map_err(|_| RootCodecError::BadMetadataBootstrapExtent)?,
                ),
                capacity: u64::from_le_bytes(
                    buf[77..85]
                        .try_into()
                        .map_err(|_| RootCodecError::BadMetadataBootstrapCapacity)?,
                ),
                extents,
            },
        })
    }

    pub(crate) fn validate_root_bounds(&self, page_count: u64) -> Result<(), RootCodecError> {
        for root in [
            self.index_root,
            self.freemap_root,
            self.maintenance_root,
            self.current_record_root,
            self.root_catalog_root,
        ] {
            validate_page_id_bounds(root, page_count)?;
        }
        self.metadata_bootstrap_reserve.validate(page_count)?;
        Ok(())
    }

    pub(crate) fn validate_recovered_generation(
        &self,
        page_count: u64,
        expected_generation: u64,
    ) -> Result<(), RootCodecError> {
        self.validate_root_bounds(page_count)?;
        if self.minimum_recoverable_generation > expected_generation {
            return Err(RootCodecError::RecoveryGenerationFloorBeyondCommit {
                floor: self.minimum_recoverable_generation,
                generation: expected_generation,
            });
        }
        let actual = self.metadata_bootstrap_reserve.owning_generation;
        if actual != expected_generation {
            return Err(RootCodecError::MetadataBootstrapGenerationMismatch {
                expected: expected_generation,
                actual,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn decode_lrt4_for_promotion(
    buf: &[u8],
    page_count: u64,
) -> Result<CanonicalRegionTable, RootCodecError> {
    if buf.len() != CANONICAL_REGION_TABLE_LEN {
        return Err(RootCodecError::WrongLength {
            expected: CANONICAL_REGION_TABLE_LEN,
            actual: buf.len(),
        });
    }
    if &buf[0..4] != b"LRT4" {
        return Err(RootCodecError::BadMagic);
    }
    if u16::from_le_bytes(
        buf[4..6]
            .try_into()
            .map_err(|_| RootCodecError::BadVersion)?,
    ) != 4
    {
        return Err(RootCodecError::BadVersion);
    }
    if u16::from_le_bytes(
        buf[6..8]
            .try_into()
            .map_err(|_| RootCodecError::BadLayoutLength)?,
    ) != PAGE_SIZE as u16
    {
        return Err(RootCodecError::BadLayoutLength);
    }
    if u64::from_le_bytes(
        buf[8..16]
            .try_into()
            .map_err(|_| RootCodecError::BadPageSize)?,
    ) != PAGE_SIZE
    {
        return Err(RootCodecError::BadPageSize);
    }
    let stored = u32::from_le_bytes(
        buf[CANONICAL_REGION_TABLE_LEN - 4..CANONICAL_REGION_TABLE_LEN]
            .try_into()
            .map_err(|_| RootCodecError::CrcMismatch)?,
    );
    if crc32c(&buf[..CANONICAL_REGION_TABLE_LEN - 4]) != stored {
        return Err(RootCodecError::CrcMismatch);
    }
    if buf[77..CANONICAL_REGION_TABLE_LEN - 4]
        .iter()
        .any(|byte| *byte != 0)
    {
        return Err(RootCodecError::NonZeroReserved);
    }
    let mut offset = 16;
    let mut roots = [None; 5];
    for root in &mut roots {
        *root = decode_root_slot(&buf[offset..offset + 9], true, false)?;
        validate_page_id_bounds(*root, page_count)?;
        offset += 9;
    }
    Ok(CanonicalRegionTable {
        index_root: roots[0],
        freemap_root: roots[1],
        maintenance_root: roots[2],
        current_record_root: roots[3],
        root_catalog_root: roots[4],
        open_segment: u64::from_le_bytes(
            buf[61..69]
                .try_into()
                .map_err(|_| RootCodecError::BadRootSlot)?,
        ),
        mutable_overlay_generation_floor: u64::from_le_bytes(
            buf[69..77]
                .try_into()
                .map_err(|_| RootCodecError::BadRootSlot)?,
        ),
        minimum_recoverable_generation: 0,
        metadata_bootstrap_reserve: MetadataBootstrapReserve::default(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RootCatalogEntry {
    pub(crate) family_id: u16,
    pub(crate) flags: u16,
    pub(crate) root: PageId,
}

impl RootCatalogEntry {
    pub(crate) fn authoritative(family_id: u16, root: PageId) -> RootCatalogEntry {
        RootCatalogEntry {
            family_id,
            flags: ROOT_FLAG_AUTHORITATIVE,
            root,
        }
    }

    pub(crate) fn advisory(family_id: u16, root: PageId) -> RootCatalogEntry {
        RootCatalogEntry {
            family_id,
            flags: ROOT_FLAG_ADVISORY,
            root,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RootCatalog {
    pub(crate) entries: Vec<RootCatalogEntry>,
}

impl RootCatalog {
    pub(crate) fn encode(&self) -> Result<[u8; ROOT_CATALOG_LEN], RootCodecError> {
        if self.entries.len() > ROOT_CATALOG_MAX_ENTRIES {
            return Err(RootCodecError::EntryCountTooLarge);
        }
        validate_root_catalog_entries(&self.entries)?;
        let mut out = [0u8; ROOT_CATALOG_LEN];
        out[0..8].copy_from_slice(ROOT_CATALOG_MAGIC);
        out[8..10].copy_from_slice(&1u16.to_le_bytes());
        out[10..12].copy_from_slice(&(ROOT_CATALOG_ENTRY_SIZE as u16).to_le_bytes());
        out[12..14].copy_from_slice(&(self.entries.len() as u16).to_le_bytes());
        let mut p = 32;
        for entry in &self.entries {
            out[p..p + 2].copy_from_slice(&entry.family_id.to_le_bytes());
            out[p + 2..p + 4].copy_from_slice(&entry.flags.to_le_bytes());
            encode_root_slot(&mut out[p + 4..p + 13], Some(entry.root));
            p += ROOT_CATALOG_ENTRY_SIZE;
        }
        let crc = crc32c(&out[..ROOT_CATALOG_LEN - 4]);
        out[ROOT_CATALOG_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
        Ok(out)
    }

    pub(crate) fn decode(buf: &[u8]) -> Result<RootCatalog, RootCodecError> {
        if buf.len() != ROOT_CATALOG_LEN {
            return Err(RootCodecError::WrongLength {
                expected: ROOT_CATALOG_LEN,
                actual: buf.len(),
            });
        }
        if &buf[0..8] != ROOT_CATALOG_MAGIC {
            return Err(RootCodecError::BadMagic);
        }
        if u16::from_le_bytes(
            buf[8..10]
                .try_into()
                .map_err(|_| RootCodecError::BadVersion)?,
        ) != 1
        {
            return Err(RootCodecError::BadVersion);
        }
        if u16::from_le_bytes(
            buf[10..12]
                .try_into()
                .map_err(|_| RootCodecError::BadEntrySize)?,
        ) as usize
            != ROOT_CATALOG_ENTRY_SIZE
        {
            return Err(RootCodecError::BadEntrySize);
        }
        let count = u16::from_le_bytes(
            buf[12..14]
                .try_into()
                .map_err(|_| RootCodecError::EntryCountTooLarge)?,
        ) as usize;
        if u16::from_le_bytes(
            buf[14..16]
                .try_into()
                .map_err(|_| RootCodecError::BadCatalogFlags)?,
        ) != 0
        {
            return Err(RootCodecError::BadCatalogFlags);
        }
        if buf[16..32].iter().any(|b| *b != 0) {
            return Err(RootCodecError::NonZeroReserved);
        }
        if count > ROOT_CATALOG_MAX_ENTRIES {
            return Err(RootCodecError::EntryCountTooLarge);
        }
        let entries_end = 32 + count * ROOT_CATALOG_ENTRY_SIZE;
        if buf[entries_end..ROOT_CATALOG_LEN - 4]
            .iter()
            .any(|b| *b != 0)
        {
            return Err(RootCodecError::NonZeroReserved);
        }
        let stored = u32::from_le_bytes(
            buf[ROOT_CATALOG_LEN - 4..ROOT_CATALOG_LEN]
                .try_into()
                .map_err(|_| RootCodecError::CrcMismatch)?,
        );
        if crc32c(&buf[..ROOT_CATALOG_LEN - 4]) != stored {
            return Err(RootCodecError::CrcMismatch);
        }
        let mut entries = Vec::with_capacity(count);
        for chunk in buf[32..entries_end].chunks_exact(ROOT_CATALOG_ENTRY_SIZE) {
            if chunk[13..ROOT_CATALOG_ENTRY_SIZE].iter().any(|b| *b != 0) {
                return Err(RootCodecError::NonZeroReserved);
            }
            let root =
                decode_root_slot(&chunk[4..13], false, true)?.ok_or(RootCodecError::BadRootSlot)?;
            entries.push(RootCatalogEntry {
                family_id: u16::from_le_bytes(
                    chunk[0..2]
                        .try_into()
                        .map_err(|_| RootCodecError::DuplicateOrUnsortedFamily)?,
                ),
                flags: u16::from_le_bytes(
                    chunk[2..4]
                        .try_into()
                        .map_err(|_| RootCodecError::BadFamilyFlags)?,
                ),
                root,
            });
        }
        validate_root_catalog_entries(&entries)?;
        Ok(RootCatalog { entries })
    }

    pub(crate) fn validate_root_bounds(&self, page_count: u64) -> Result<(), RootCodecError> {
        for entry in &self.entries {
            validate_page_id_bounds(Some(entry.root), page_count)?;
        }
        Ok(())
    }
}

fn validate_page_id_bounds(root: Option<PageId>, page_count: u64) -> Result<(), RootCodecError> {
    if let Some(PageId(page_id)) = root
        && page_id >= page_count
    {
        return Err(RootCodecError::PageIdOutOfBounds {
            page_id,
            page_count,
        });
    }
    Ok(())
}

pub(crate) fn root_family_descriptor(family_id: u16) -> Option<&'static RootFamilyDescriptor> {
    ROOT_FAMILY_REGISTRY
        .iter()
        .find(|descriptor| descriptor.family_id == family_id)
}

fn validate_root_catalog_entries(entries: &[RootCatalogEntry]) -> Result<(), RootCodecError> {
    let mut previous = None;
    for entry in entries {
        if previous.is_some_and(|family_id| family_id >= entry.family_id) {
            return Err(RootCodecError::DuplicateOrUnsortedFamily);
        }
        previous = Some(entry.family_id);
        match entry.flags {
            ROOT_FLAG_AUTHORITATIVE | ROOT_FLAG_ADVISORY => {}
            _ => return Err(RootCodecError::BadFamilyFlags),
        }
        if entry.root.0 == 0 {
            return Err(RootCodecError::PresentZeroPageId);
        }
        if let Some(descriptor) = root_family_descriptor(entry.family_id) {
            if descriptor.location != RootFamilyLocation::RootCatalog {
                return Err(RootCodecError::DirectRegionTableFamilyInCatalog);
            }
            if descriptor.flags != entry.flags {
                return Err(RootCodecError::KnownFamilyFlagMismatch);
            }
        } else if entry.flags == ROOT_FLAG_AUTHORITATIVE {
            return Err(RootCodecError::UnknownAuthoritativeFamily);
        }
    }
    Ok(())
}

/// Roots and accounting for the page-structured regions, held on one page. The region table's own
/// page id is the single region pointer the superblock and journal record carry. A `None` root means
/// that region has no page yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegionTable {
    pub(crate) page_size: u64,
    pub(crate) index_root: Option<PageId>,
    pub(crate) freemap_root: Option<PageId>,
    pub(crate) maintenance_root: Option<PageId>,
    pub(crate) overlay_root: Option<PageId>,
    pub(crate) current_record_root: Option<PageId>,
    pub(crate) root_catalog_root: Option<PageId>,
    pub(crate) open_segment: u64,
    pub(crate) mutable_overlay_generation_floor: u64,
    pub(crate) minimum_recoverable_generation: u64,
    pub(crate) metadata_bootstrap_reserve: MetadataBootstrapReserve,
}

impl RegionTable {
    pub(crate) fn from_canonical(canonical: CanonicalRegionTable) -> RegionTable {
        RegionTable {
            page_size: PAGE_SIZE,
            index_root: canonical.index_root,
            freemap_root: canonical.freemap_root,
            maintenance_root: canonical.maintenance_root,
            overlay_root: None,
            current_record_root: canonical.current_record_root,
            root_catalog_root: canonical.root_catalog_root,
            open_segment: canonical.open_segment,
            mutable_overlay_generation_floor: canonical.mutable_overlay_generation_floor,
            minimum_recoverable_generation: canonical.minimum_recoverable_generation,
            metadata_bootstrap_reserve: canonical.metadata_bootstrap_reserve,
        }
    }

    pub(crate) fn encode_page(
        &self,
        page_count: u64,
    ) -> Result<[u8; PAGE_SIZE as usize], RootCodecError> {
        if self.overlay_root.is_some() {
            return Err(RootCodecError::LegacyOverlayRoot);
        }
        CanonicalRegionTable {
            index_root: self.index_root,
            freemap_root: self.freemap_root,
            maintenance_root: self.maintenance_root,
            current_record_root: self.current_record_root,
            root_catalog_root: self.root_catalog_root,
            open_segment: self.open_segment,
            mutable_overlay_generation_floor: self.mutable_overlay_generation_floor,
            minimum_recoverable_generation: self.minimum_recoverable_generation,
            metadata_bootstrap_reserve: self.metadata_bootstrap_reserve.clone(),
        }
        .encode(page_count)
    }

    /// Encode into a fixed-size, CRC'd blob suitable for writing into the region-table page.
    #[cfg(test)]
    pub(crate) fn encode(&self) -> [u8; REGION_TABLE_LEN] {
        let mut r = [0u8; REGION_TABLE_LEN];
        r[0] = REGION_TABLE_MAGIC_V3;
        r[1..9].copy_from_slice(&self.page_size.to_le_bytes());
        let mut p = 9;
        for root in [
            self.index_root,
            self.freemap_root,
            self.maintenance_root,
            self.overlay_root,
        ] {
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

    /// Legacy-compatible decoding for controlled migration, diagnostics, and tests. Ordinary open uses
    /// the canonical recovered-generation validator.
    pub(crate) fn decode(buf: &[u8]) -> Option<RegionTable> {
        if buf.get(0..4) == Some(CANONICAL_REGION_TABLE_MAGIC) {
            let canonical =
                CanonicalRegionTable::decode(buf.get(..CANONICAL_REGION_TABLE_LEN)?).ok()?;
            return Some(RegionTable::from_canonical(canonical));
        }
        let magic = buf.first().copied()?;
        let encoded_len = match magic {
            REGION_TABLE_MAGIC_V3 => REGION_TABLE_LEN,
            REGION_TABLE_MAGIC_V2 => LEGACY_REGION_TABLE_LEN,
            _ => return None,
        };
        if buf.len() < encoded_len {
            return None;
        }
        let stored = u32::from_le_bytes(buf[encoded_len - 4..encoded_len].try_into().ok()?);
        if crc32c(&buf[..encoded_len - 4]) != stored {
            return None;
        }
        let page_size = u64::from_le_bytes(buf[1..9].try_into().ok()?);
        let mut roots = [None; 4];
        let mut p = 9;
        let root_count = if magic == REGION_TABLE_MAGIC_V3 { 4 } else { 3 };
        for slot in roots.iter_mut().take(root_count) {
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
            overlay_root: roots[3],
            current_record_root: None,
            root_catalog_root: None,
            open_segment,
            mutable_overlay_generation_floor: 0,
            minimum_recoverable_generation: 0,
            metadata_bootstrap_reserve: MetadataBootstrapReserve::default(),
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
            overlay_root: Some(PageId(13)),
            current_record_root: None,
            root_catalog_root: None,
            open_segment: 3,
            mutable_overlay_generation_floor: 0,
            minimum_recoverable_generation: 0,
            metadata_bootstrap_reserve: MetadataBootstrapReserve::default(),
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
                overlay_root: None,
                current_record_root: None,
                root_catalog_root: None,
                open_segment: 0,
                mutable_overlay_generation_floor: 0,
                minimum_recoverable_generation: 0,
                metadata_bootstrap_reserve: MetadataBootstrapReserve::default(),
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
    fn overlay_free_region_tables_encode_as_canonical_pages() {
        for table in [
            RegionTable {
                page_size: PAGE_SIZE,
                index_root: None,
                freemap_root: None,
                maintenance_root: None,
                overlay_root: None,
                current_record_root: None,
                root_catalog_root: None,
                open_segment: 0,
                mutable_overlay_generation_floor: 0,
                minimum_recoverable_generation: 1,
                metadata_bootstrap_reserve: MetadataBootstrapReserve {
                    owning_generation: 1,
                    capacity: 8,
                    extents: Vec::new(),
                },
            },
            RegionTable {
                page_size: PAGE_SIZE,
                index_root: Some(PageId(7)),
                freemap_root: Some(PageId(8)),
                maintenance_root: Some(PageId(9)),
                overlay_root: None,
                current_record_root: None,
                root_catalog_root: None,
                open_segment: 3,
                mutable_overlay_generation_floor: 0,
                minimum_recoverable_generation: 1,
                metadata_bootstrap_reserve: MetadataBootstrapReserve {
                    owning_generation: 1,
                    capacity: 8,
                    extents: Vec::new(),
                },
            },
        ] {
            let bytes = table.encode_page(16).unwrap();
            assert_eq!(&bytes[..4], CANONICAL_REGION_TABLE_MAGIC);
            assert_eq!(RegionTable::decode(&bytes), Some(table));
        }
    }

    #[test]
    fn canonical_region_table_writer_rejects_legacy_overlay_root() {
        assert_eq!(
            sample().encode_page(16).unwrap_err(),
            RootCodecError::LegacyOverlayRoot
        );
    }

    #[test]
    fn legacy_v2_region_table_fixture_still_decodes() {
        let mut bytes = [0u8; LEGACY_REGION_TABLE_LEN];
        bytes[0] = REGION_TABLE_MAGIC_V2;
        bytes[1..9].copy_from_slice(&PAGE_SIZE.to_le_bytes());
        let roots = [Some(PageId(7)), None, Some(PageId(11))];
        let mut p = 9;
        for root in roots {
            if let Some(PageId(id)) = root {
                bytes[p] = 1;
                bytes[p + 1..p + 9].copy_from_slice(&id.to_le_bytes());
            }
            p += 9;
        }
        bytes[p..p + 8].copy_from_slice(&3u64.to_le_bytes());
        let crc = crc32c(&bytes[..LEGACY_REGION_TABLE_LEN - 4]);
        bytes[LEGACY_REGION_TABLE_LEN - 4..].copy_from_slice(&crc.to_le_bytes());

        assert_eq!(
            RegionTable::decode(&bytes),
            Some(RegionTable {
                page_size: PAGE_SIZE,
                index_root: Some(PageId(7)),
                freemap_root: None,
                maintenance_root: Some(PageId(11)),
                overlay_root: None,
                current_record_root: None,
                root_catalog_root: None,
                open_segment: 3,
                mutable_overlay_generation_floor: 0,
                minimum_recoverable_generation: 0,
                metadata_bootstrap_reserve: MetadataBootstrapReserve::default(),
            })
        );
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

    fn canonical_region_sample() -> CanonicalRegionTable {
        CanonicalRegionTable {
            index_root: Some(PageId(1)),
            freemap_root: Some(PageId(2)),
            maintenance_root: Some(PageId(3)),
            current_record_root: Some(PageId(4)),
            root_catalog_root: Some(PageId(5)),
            open_segment: 6,
            mutable_overlay_generation_floor: 9,
            minimum_recoverable_generation: 12,
            metadata_bootstrap_reserve: MetadataBootstrapReserve {
                owning_generation: 12,
                capacity: 32,
                extents: vec![MetadataBootstrapExtent { start: 7, len: 8 }],
            },
        }
    }

    fn root_catalog_sample() -> RootCatalog {
        RootCatalog {
            entries: vec![
                RootCatalogEntry::authoritative(0x0100, PageId(100)),
                RootCatalogEntry::authoritative(0x0110, PageId(101)),
                RootCatalogEntry::authoritative(0x0120, PageId(102)),
                RootCatalogEntry::authoritative(0x0130, PageId(103)),
                RootCatalogEntry::authoritative(0x0131, PageId(104)),
                RootCatalogEntry::authoritative(0x0140, PageId(105)),
                RootCatalogEntry::authoritative(0x0200, PageId(106)),
                RootCatalogEntry::authoritative(0x0210, PageId(107)),
                RootCatalogEntry::authoritative(0x0220, PageId(108)),
                RootCatalogEntry::authoritative(0x0230, PageId(109)),
                RootCatalogEntry::advisory(0x0300, PageId(110)),
            ],
        }
    }

    fn recompute_crc(bytes: &mut [u8]) {
        let crc_offset = bytes.len() - 4;
        let crc = crc32c(&bytes[..crc_offset]);
        bytes[crc_offset..].copy_from_slice(&crc.to_le_bytes());
    }

    #[test]
    fn canonical_region_table_matches_positive_vector() {
        let table = canonical_region_sample();
        let bytes = table.encode(32).unwrap();

        assert_eq!(bytes.len(), CANONICAL_REGION_TABLE_LEN);
        assert_eq!(CanonicalRegionTable::decode(&bytes).unwrap(), table);
        assert_eq!(
            &bytes[..96],
            &hex_to_vec(
                "4c5254350500001000100000000000000101000000000000000102000000000000000103000000000000000104000000000000000105000000000000000600000000000000090000000000000020000000000000000c00000000000000010000"
            )
        );
        assert_eq!(
            &bytes[96..112],
            &hex_to_vec("07000000000000000800000000000000")
        );
        assert_eq!(
            &bytes[4080..4096],
            &hex_to_vec("000000000c00000000000000e9403ed7")
        );
    }

    #[test]
    fn canonical_region_table_rejects_negative_vectors() {
        let mut bytes = canonical_region_sample().encode(32).unwrap();
        bytes[0] = 0;
        assert_eq!(
            CanonicalRegionTable::decode(&bytes).unwrap_err(),
            RootCodecError::BadMagic
        );

        let mut bytes = canonical_region_sample().encode(32).unwrap();
        bytes[95] = 1;
        recompute_crc(&mut bytes);
        assert_eq!(
            CanonicalRegionTable::decode(&bytes).unwrap_err(),
            RootCodecError::NonZeroReserved
        );

        let mut bytes = canonical_region_sample().encode(32).unwrap();
        bytes[16] = 2;
        recompute_crc(&mut bytes);
        assert_eq!(
            CanonicalRegionTable::decode(&bytes).unwrap_err(),
            RootCodecError::BadRootSlot
        );
    }

    #[test]
    fn canonical_region_table_rejects_invalid_metadata_bootstrap_descriptors() {
        let mut table = canonical_region_sample();
        table.metadata_bootstrap_reserve.capacity = 0;
        assert_eq!(
            table.encode(32).unwrap_err(),
            RootCodecError::BadMetadataBootstrapCapacity
        );

        let mut table = canonical_region_sample();
        table.metadata_bootstrap_reserve.extents = vec![
            MetadataBootstrapExtent { start: 7, len: 4 },
            MetadataBootstrapExtent { start: 10, len: 2 },
        ];
        assert_eq!(
            table.encode(32).unwrap_err(),
            RootCodecError::BadMetadataBootstrapExtent
        );

        let mut table = canonical_region_sample();
        table.metadata_bootstrap_reserve.extents[0] = MetadataBootstrapExtent { start: 30, len: 3 };
        assert_eq!(
            table.encode(32).unwrap_err(),
            RootCodecError::BadMetadataBootstrapExtent
        );

        let mut bytes = canonical_region_sample().encode(32).unwrap();
        bytes[93..95].copy_from_slice(&((METADATA_BOOTSTRAP_MAX_EXTENTS + 1) as u16).to_le_bytes());
        recompute_crc(&mut bytes);
        assert_eq!(
            CanonicalRegionTable::decode(&bytes).unwrap_err(),
            RootCodecError::BadMetadataBootstrapExtentCount
        );
    }

    #[test]
    fn canonical_region_table_requires_exact_len_and_crc() {
        let bytes = canonical_region_sample().encode(32).unwrap();
        assert_eq!(
            CanonicalRegionTable::decode(&bytes[..CANONICAL_REGION_TABLE_LEN - 1]).unwrap_err(),
            RootCodecError::WrongLength {
                expected: CANONICAL_REGION_TABLE_LEN,
                actual: CANONICAL_REGION_TABLE_LEN - 1,
            }
        );

        let mut oversized = bytes.to_vec();
        oversized.push(0);
        assert_eq!(
            CanonicalRegionTable::decode(&oversized).unwrap_err(),
            RootCodecError::WrongLength {
                expected: CANONICAL_REGION_TABLE_LEN,
                actual: CANONICAL_REGION_TABLE_LEN + 1,
            }
        );

        let mut bad_crc = bytes;
        bad_crc[4092] ^= 0xff;
        assert_eq!(
            CanonicalRegionTable::decode(&bad_crc).unwrap_err(),
            RootCodecError::CrcMismatch
        );
    }

    #[test]
    fn canonical_region_table_validates_root_bounds_from_committed_page_count() {
        let table = CanonicalRegionTable {
            index_root: Some(PageId(0)),
            freemap_root: Some(PageId(1)),
            maintenance_root: Some(PageId(2)),
            current_record_root: Some(PageId(3)),
            root_catalog_root: Some(PageId(4)),
            open_segment: 6,
            mutable_overlay_generation_floor: 7,
            minimum_recoverable_generation: 7,
            metadata_bootstrap_reserve: MetadataBootstrapReserve {
                owning_generation: 7,
                capacity: 8,
                extents: Vec::new(),
            },
        };
        table.validate_root_bounds(5).unwrap();

        let out_of_bounds = CanonicalRegionTable {
            root_catalog_root: Some(PageId(5)),
            ..table
        };
        assert_eq!(
            out_of_bounds.validate_root_bounds(5).unwrap_err(),
            RootCodecError::PageIdOutOfBounds {
                page_id: 5,
                page_count: 5,
            }
        );
    }

    #[test]
    fn canonical_region_table_validates_recovered_metadata_bootstrap_generation() {
        let table = canonical_region_sample();
        table.validate_recovered_generation(16, 12).unwrap();

        let future_floor = CanonicalRegionTable {
            minimum_recoverable_generation: 13,
            ..table.clone()
        };
        assert_eq!(
            future_floor
                .validate_recovered_generation(16, 12)
                .unwrap_err(),
            RootCodecError::RecoveryGenerationFloorBeyondCommit {
                floor: 13,
                generation: 12,
            }
        );
        assert_eq!(
            table.validate_recovered_generation(16, 13).unwrap_err(),
            RootCodecError::MetadataBootstrapGenerationMismatch {
                expected: 13,
                actual: 12,
            }
        );
    }

    #[test]
    fn root_catalog_matches_positive_vector() {
        let catalog = root_catalog_sample();
        let bytes = catalog.encode().unwrap();

        assert_eq!(bytes.len(), ROOT_CATALOG_LEN);
        assert_eq!(RootCatalog::decode(&bytes).unwrap(), catalog);
        let vector = hex_to_vec(
            "4c524f4f54433100010020000b000000000000000000000000000000000000000001010001640000000000000000000000000000000000000000000000000000",
        );
        assert_eq!(vector.len(), 64);
        assert_eq!(&bytes[..64], &vector);
        assert_eq!(
            &bytes[32..64],
            &hex_to_vec("0001010001640000000000000000000000000000000000000000000000000000")
        );
        assert_eq!(&bytes[4088..4096], &hex_to_vec("00000000bac1924c"));
    }

    #[test]
    fn root_catalog_rejects_corrupt_vectors() {
        let mut bytes = root_catalog_sample().encode().unwrap();
        bytes[12] = 0x7f;
        recompute_crc(&mut bytes);
        assert_eq!(
            RootCatalog::decode(&bytes).unwrap_err(),
            RootCodecError::EntryCountTooLarge
        );

        let mut bytes = root_catalog_sample().encode().unwrap();
        bytes[36] = 0;
        recompute_crc(&mut bytes);
        assert_eq!(
            RootCatalog::decode(&bytes).unwrap_err(),
            RootCodecError::AbsentEntryRoot
        );

        let mut bytes = root_catalog_sample().encode().unwrap();
        bytes[36..45].copy_from_slice(&hex_to_vec("010000000000000000"));
        recompute_crc(&mut bytes);
        assert_eq!(
            RootCatalog::decode(&bytes).unwrap_err(),
            RootCodecError::PresentZeroPageId
        );

        let mut bytes = root_catalog_sample().encode().unwrap();
        bytes[64..66].copy_from_slice(&hex_to_vec("0001"));
        recompute_crc(&mut bytes);
        assert_eq!(
            RootCatalog::decode(&bytes).unwrap_err(),
            RootCodecError::DuplicateOrUnsortedFamily
        );

        let mut bytes = root_catalog_sample().encode().unwrap();
        bytes[96..98].copy_from_slice(&hex_to_vec("0801"));
        recompute_crc(&mut bytes);
        assert_eq!(
            RootCatalog::decode(&bytes).unwrap_err(),
            RootCodecError::DuplicateOrUnsortedFamily
        );

        let mut bytes = root_catalog_sample().encode().unwrap();
        bytes[12] = 0x0c;
        bytes[384..397].copy_from_slice(&hex_to_vec("01400100016f00000000000000"));
        recompute_crc(&mut bytes);
        assert_eq!(
            RootCatalog::decode(&bytes).unwrap_err(),
            RootCodecError::UnknownAuthoritativeFamily
        );
    }

    #[test]
    fn root_catalog_requires_exact_len_and_crc() {
        let bytes = root_catalog_sample().encode().unwrap();
        assert_eq!(
            RootCatalog::decode(&bytes[..ROOT_CATALOG_LEN - 1]).unwrap_err(),
            RootCodecError::WrongLength {
                expected: ROOT_CATALOG_LEN,
                actual: ROOT_CATALOG_LEN - 1,
            }
        );

        let mut oversized = bytes.to_vec();
        oversized.push(0);
        assert_eq!(
            RootCatalog::decode(&oversized).unwrap_err(),
            RootCodecError::WrongLength {
                expected: ROOT_CATALOG_LEN,
                actual: ROOT_CATALOG_LEN + 1,
            }
        );

        let mut bad_crc = bytes;
        bad_crc[4092] ^= 0xff;
        assert_eq!(
            RootCatalog::decode(&bad_crc).unwrap_err(),
            RootCodecError::CrcMismatch
        );
    }

    #[test]
    fn root_catalog_validates_entry_root_bounds_from_committed_page_count() {
        let catalog = RootCatalog {
            entries: vec![
                RootCatalogEntry::authoritative(0x0100, PageId(1)),
                RootCatalogEntry::advisory(0x0300, PageId(4)),
            ],
        };
        catalog.validate_root_bounds(5).unwrap();

        let out_of_bounds = RootCatalog {
            entries: vec![
                RootCatalogEntry::authoritative(0x0100, PageId(1)),
                RootCatalogEntry::advisory(0x0300, PageId(5)),
            ],
        };
        assert_eq!(
            out_of_bounds.validate_root_bounds(5).unwrap_err(),
            RootCodecError::PageIdOutOfBounds {
                page_id: 5,
                page_count: 5,
            }
        );
    }

    #[test]
    fn root_catalog_preserves_unknown_advisory_entries() {
        let mut bytes = root_catalog_sample().encode().unwrap();
        bytes[12] = 0x0c;
        bytes[384..397].copy_from_slice(&hex_to_vec("01400200016f00000000000000"));
        recompute_crc(&mut bytes);

        let catalog = RootCatalog::decode(&bytes).unwrap();
        assert_eq!(
            catalog.entries.last(),
            Some(&RootCatalogEntry::advisory(0x4001, PageId(111)))
        );
    }

    #[test]
    fn root_catalog_rejects_direct_family_and_bad_flags() {
        let bad_direct = RootCatalog {
            entries: vec![RootCatalogEntry::authoritative(
                CURRENT_RECORDS_FAMILY_ID,
                PageId(1),
            )],
        };
        assert_eq!(
            bad_direct.encode().unwrap_err(),
            RootCodecError::DirectRegionTableFamilyInCatalog
        );

        let bad_flags = RootCatalog {
            entries: vec![RootCatalogEntry {
                family_id: 0x0100,
                flags: ROOT_FLAG_AUTHORITATIVE | ROOT_FLAG_ADVISORY,
                root: PageId(1),
            }],
        };
        assert_eq!(
            bad_flags.encode().unwrap_err(),
            RootCodecError::BadFamilyFlags
        );
    }

    fn hex_to_vec(hex: &str) -> Vec<u8> {
        assert_eq!(hex.len() % 2, 0);
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }
}
