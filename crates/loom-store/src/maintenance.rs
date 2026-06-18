//! Maintenance-state records for conservative store compaction planning.

use crate::page::{PAGE_SIZE, PageId};
use crate::{BackingIo, DATA_START, FreePageRun, corrupt, crc32c, io_err, read_exact_at, write_at};
use loom_core::error::Result;
use std::collections::BTreeSet;

const MAINTENANCE_MAGIC: u8 = 0xB7;
const MAINTENANCE_VERSION_V1: u8 = 1;
const MAINTENANCE_VERSION: u8 = 2;
const FLAG_OVERFLOW: u8 = 0x01;
const MAX_SEGMENTS: usize = 220;
const HEADER_LEN_V1: usize = 1 + 1 + 1 + 8 * 5 + 2 + 2;
const HEADER_LEN: usize = 1 + 1 + 1 + 8 * 6 + 2 + 2;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MaintenanceState {
    pub(crate) generation: u64,
    pub(crate) object_count: u64,
    pub(crate) object_count_known: bool,
    pub(crate) physical_page_count: u64,
    pub(crate) reusable_free_pages: u64,
    pub(crate) candidate_dead_pages: u64,
    pub(crate) last_validated_mark_epoch: u64,
    pub(crate) touched_segments: Vec<u64>,
    pub(crate) candidate_segments: Vec<u64>,
    pub(crate) segment_overflow: bool,
}

impl MaintenanceState {
    pub(crate) fn next(
        previous: &MaintenanceState,
        generation: u64,
        object_count: u64,
        page_count: u64,
        free: &[FreePageRun],
        touched: &BTreeSet<u64>,
    ) -> MaintenanceState {
        let reusable_free_pages = free.iter().map(|run| run.len).sum();
        let mut touched_segments = previous.touched_segments.clone();
        let mut candidate_segments = previous.candidate_segments.clone();
        let mut overflow = previous.segment_overflow;

        for segment in touched {
            push_segment(&mut touched_segments, *segment, &mut overflow);
            push_segment(&mut candidate_segments, *segment, &mut overflow);
        }
        for run in free {
            let first = run.start / crate::page::PAGES_PER_SEGMENT;
            let last = (run.start + run.len.saturating_sub(1)) / crate::page::PAGES_PER_SEGMENT;
            for segment in first..=last {
                push_segment(&mut candidate_segments, segment, &mut overflow);
            }
        }

        MaintenanceState {
            generation,
            object_count,
            object_count_known: true,
            physical_page_count: page_count,
            reusable_free_pages,
            candidate_dead_pages: reusable_free_pages,
            last_validated_mark_epoch: previous.last_validated_mark_epoch,
            touched_segments,
            candidate_segments,
            segment_overflow: overflow,
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            HEADER_LEN + (self.touched_segments.len() + self.candidate_segments.len()) * 8 + 4,
        );
        out.push(MAINTENANCE_MAGIC);
        out.push(MAINTENANCE_VERSION);
        out.push(if self.segment_overflow {
            FLAG_OVERFLOW
        } else {
            0
        });
        for value in [
            self.generation,
            self.object_count,
            self.physical_page_count,
            self.reusable_free_pages,
            self.candidate_dead_pages,
            self.last_validated_mark_epoch,
        ] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&(self.touched_segments.len() as u16).to_le_bytes());
        out.extend_from_slice(&(self.candidate_segments.len() as u16).to_le_bytes());
        for segment in &self.touched_segments {
            out.extend_from_slice(&segment.to_le_bytes());
        }
        for segment in &self.candidate_segments {
            out.extend_from_slice(&segment.to_le_bytes());
        }
        let crc = crc32c(&out);
        out.extend_from_slice(&crc.to_le_bytes());
        out
    }

    fn decode(buf: &[u8]) -> Result<MaintenanceState> {
        if buf.len() < HEADER_LEN + 4 || buf[0] != MAINTENANCE_MAGIC {
            return Err(corrupt("maintenance record magic"));
        }
        let (header_len, has_object_count) = match buf[1] {
            MAINTENANCE_VERSION => (HEADER_LEN, true),
            MAINTENANCE_VERSION_V1 => (HEADER_LEN_V1, false),
            _ => return Err(corrupt("maintenance record version")),
        };
        if buf.len() < header_len + 4 {
            return Err(corrupt("maintenance record version"));
        }
        let flags = buf[2];
        if flags & !FLAG_OVERFLOW != 0 {
            return Err(corrupt("maintenance record flags"));
        }
        let mut pos = 3;
        let generation = read_u64(buf, &mut pos)?;
        let object_count = if has_object_count {
            read_u64(buf, &mut pos)?
        } else {
            0
        };
        let physical_page_count = read_u64(buf, &mut pos)?;
        let reusable_free_pages = read_u64(buf, &mut pos)?;
        let candidate_dead_pages = read_u64(buf, &mut pos)?;
        let last_validated_mark_epoch = read_u64(buf, &mut pos)?;
        let touched_len = read_u16(buf, &mut pos)? as usize;
        let candidate_len = read_u16(buf, &mut pos)? as usize;
        if touched_len > MAX_SEGMENTS || candidate_len > MAX_SEGMENTS {
            return Err(corrupt("maintenance record segment count"));
        }
        let expected = header_len
            .checked_add((touched_len + candidate_len) * 8)
            .and_then(|n| n.checked_add(4))
            .ok_or_else(|| corrupt("maintenance record length overflow"))?;
        if buf.len() < expected {
            return Err(corrupt("maintenance record truncated"));
        }
        let stored = u32::from_le_bytes(buf[expected - 4..expected].try_into().unwrap());
        if crc32c(&buf[..expected - 4]) != stored {
            return Err(corrupt("maintenance record crc"));
        }
        let mut touched_segments = Vec::with_capacity(touched_len);
        for _ in 0..touched_len {
            touched_segments.push(read_u64(buf, &mut pos)?);
        }
        let mut candidate_segments = Vec::with_capacity(candidate_len);
        for _ in 0..candidate_len {
            candidate_segments.push(read_u64(buf, &mut pos)?);
        }
        if !is_strictly_sorted(&touched_segments) || !is_strictly_sorted(&candidate_segments) {
            return Err(corrupt("maintenance record segments out of order"));
        }
        Ok(MaintenanceState {
            generation,
            object_count,
            object_count_known: has_object_count,
            physical_page_count,
            reusable_free_pages,
            candidate_dead_pages,
            last_validated_mark_epoch,
            touched_segments,
            candidate_segments,
            segment_overflow: flags & FLAG_OVERFLOW != 0,
        })
    }
}

pub(crate) fn write_maintenance(
    file: &mut dyn BackingIo,
    page: PageId,
    state: &MaintenanceState,
) -> Result<()> {
    let encoded = state.encode();
    if encoded.len() > PAGE_SIZE as usize {
        return Err(corrupt("maintenance record too large"));
    }
    let mut buf = [0u8; PAGE_SIZE as usize];
    buf[..encoded.len()].copy_from_slice(&encoded);
    write_at(file, page.offset(DATA_START), &buf).map_err(io_err)
}

pub(crate) fn read_maintenance(
    file: &mut dyn BackingIo,
    page: PageId,
    page_count: u64,
) -> Result<MaintenanceState> {
    if page.0 >= page_count {
        return Err(corrupt("maintenance page out of range"));
    }
    let mut buf = [0u8; PAGE_SIZE as usize];
    read_exact_at(file, page.offset(DATA_START), &mut buf).map_err(io_err)?;
    MaintenanceState::decode(&buf)
}

fn push_segment(segments: &mut Vec<u64>, segment: u64, overflow: &mut bool) {
    match segments.binary_search(&segment) {
        Ok(_) => {}
        Err(pos) if segments.len() < MAX_SEGMENTS => segments.insert(pos, segment),
        Err(_) => *overflow = true,
    }
}

fn read_u16(buf: &[u8], pos: &mut usize) -> Result<u16> {
    let end = pos
        .checked_add(2)
        .ok_or_else(|| corrupt("maintenance record offset overflow"))?;
    let bytes = buf
        .get(*pos..end)
        .ok_or_else(|| corrupt("maintenance record truncated"))?;
    *pos = end;
    Ok(u16::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_u64(buf: &[u8], pos: &mut usize) -> Result<u64> {
    let end = pos
        .checked_add(8)
        .ok_or_else(|| corrupt("maintenance record offset overflow"))?;
    let bytes = buf
        .get(*pos..end)
        .ok_or_else(|| corrupt("maintenance record truncated"))?;
    *pos = end;
    Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
}

fn is_strictly_sorted(values: &[u64]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_record_round_trips() {
        let state = MaintenanceState {
            generation: 7,
            object_count: 13,
            object_count_known: true,
            physical_page_count: 11,
            reusable_free_pages: 3,
            candidate_dead_pages: 3,
            last_validated_mark_epoch: 5,
            touched_segments: vec![1, 4],
            candidate_segments: vec![1, 3, 4],
            segment_overflow: false,
        };

        assert_eq!(MaintenanceState::decode(&state.encode()).unwrap(), state);
    }

    #[test]
    fn maintenance_record_rejects_crc_damage() {
        let mut bytes = MaintenanceState {
            generation: 1,
            physical_page_count: 1,
            ..MaintenanceState::default()
        }
        .encode();
        bytes[5] ^= 0x80;

        assert!(MaintenanceState::decode(&bytes).is_err());
    }
}
