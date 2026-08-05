//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

#[cfg(not(target_arch = "wasm32"))]
use std::fs::File;
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Seek, SeekFrom, Write};

/// The minimal block-device surface the storage-v2 layer (superblock, journal, freemap, CoW B-tree)
/// needs from its backing: positioned read/write, size, grow/truncate, and durability. Abstracting it
/// lets the same on-disk format run over `std::fs::File` natively or an OPFS sync-access-handle in the
/// browser. `FileStore` holds `Box<dyn BackingIo>` (dynamic dispatch: the cost is dwarfed by
/// the syscall/OPFS op it precedes, and it keeps `FileStore` one concrete type - no generic ripple into
/// the C ABI or bindings). The open/lock/compaction lifecycle stays per-backend (see `FileStore`).
/// `Send` on every target except `wasm32`, where the OPFS backing wraps a `!Send` JS handle and the
/// runtime is single-threaded (so `Send` is neither available nor needed). This lets one `BackingIo`
/// definition serve both the multi-threaded native store and the single-threaded browser store.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> MaybeSend for T {}
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}

pub trait BackingIo: std::fmt::Debug + MaybeSend {
    /// Read exactly `buf.len()` bytes starting at byte offset `off`.
    fn pread(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()>;
    /// Write all of `buf` starting at byte offset `off`.
    fn pwrite(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()>;
    /// The current length in bytes.
    fn size(&self) -> std::io::Result<u64>;
    /// Set the length to `len` (grow with zeros, or truncate).
    fn grow(&mut self, len: u64) -> std::io::Result<()>;
    /// Flush all writes durably (the commit point depends on this).
    fn fsync(&mut self) -> std::io::Result<()>;
    fn is_planning(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PreparedBackingOperation {
    Write { offset: u64, bytes: Vec<u8> },
    Resize { len: u64 },
    Fsync,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PreparedBackingTransaction {
    source_len: u64,
    final_len: u64,
    operations: Vec<PreparedBackingOperation>,
}

impl PreparedBackingTransaction {
    pub(crate) fn source_len(&self) -> u64 {
        self.source_len
    }

    pub(crate) fn final_len(&self) -> u64 {
        self.final_len
    }

    pub(crate) fn write_count(&self) -> usize {
        self.operations
            .iter()
            .filter(|operation| matches!(operation, PreparedBackingOperation::Write { .. }))
            .count()
    }

    pub(crate) fn apply(
        &self,
        backing: &mut dyn BackingIo,
        mut observe_fsync: impl FnMut(std::time::Duration),
    ) -> std::io::Result<()> {
        if backing.size()? != self.source_len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "prepared backing source length changed",
            ));
        }
        for operation in &self.operations {
            match operation {
                PreparedBackingOperation::Write { offset, bytes } => {
                    backing.pwrite(*offset, bytes)?;
                }
                PreparedBackingOperation::Resize { len } => backing.grow(*len)?,
                PreparedBackingOperation::Fsync => {
                    let started = std::time::Instant::now();
                    backing.fsync()?;
                    observe_fsync(started.elapsed());
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct PlanningBacking<'a> {
    source: &'a mut dyn BackingIo,
    source_len: u64,
    len: u64,
    operations: Vec<PreparedBackingOperation>,
}

impl<'a> PlanningBacking<'a> {
    pub(crate) fn new(source: &'a mut dyn BackingIo) -> std::io::Result<Self> {
        let source_len = source.size()?;
        Ok(Self {
            source,
            source_len,
            len: source_len,
            operations: Vec::new(),
        })
    }

    pub(crate) fn finish(self) -> PreparedBackingTransaction {
        PreparedBackingTransaction {
            source_len: self.source_len,
            final_len: self.len,
            operations: self.operations,
        }
    }

    fn overlay_write(&mut self, offset: u64, bytes: &[u8]) {
        self.operations.push(PreparedBackingOperation::Write {
            offset,
            bytes: bytes.to_vec(),
        });
    }
}

impl BackingIo for PlanningBacking<'_> {
    fn pread(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
        let end = off.checked_add(buf.len() as u64).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "read offset overflow")
        })?;
        if end > self.len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "read past end of planning backing",
            ));
        }
        buf.fill(0);
        if off < self.source_len {
            let source_end = end.min(self.source_len);
            self.source
                .pread(off, &mut buf[..(source_end - off) as usize])?;
        }
        for operation in &self.operations {
            let PreparedBackingOperation::Write {
                offset: write_offset,
                bytes,
            } = operation
            else {
                continue;
            };
            let write_end = write_offset.saturating_add(bytes.len() as u64);
            let overlap_start = off.max(*write_offset);
            let overlap_end = end.min(write_end);
            if overlap_start < overlap_end {
                let destination = (overlap_start - off) as usize;
                let source = (overlap_start - *write_offset) as usize;
                let len = (overlap_end - overlap_start) as usize;
                buf[destination..destination + len].copy_from_slice(&bytes[source..source + len]);
            }
        }
        Ok(())
    }

    fn pwrite(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()> {
        let end = off.checked_add(buf.len() as u64).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "write offset overflow")
        })?;
        self.len = self.len.max(end);
        self.overlay_write(off, buf);
        Ok(())
    }

    fn size(&self) -> std::io::Result<u64> {
        Ok(self.len)
    }

    fn grow(&mut self, len: u64) -> std::io::Result<()> {
        self.len = len;
        self.operations
            .push(PreparedBackingOperation::Resize { len });
        Ok(())
    }

    fn fsync(&mut self) -> std::io::Result<()> {
        self.operations.push(PreparedBackingOperation::Fsync);
        Ok(())
    }

    fn is_planning(&self) -> bool {
        true
    }
}

/// The native backing: a `std::fs::File`. (A local trait on a foreign type, so no wrapper newtype is
/// needed and a bare `&mut File` coerces to `&mut dyn BackingIo` at every call site.) Native-only;
/// the wasm32 backing is the OPFS sync handle implemented in the wasm binding.
#[cfg(not(target_arch = "wasm32"))]
impl BackingIo for File {
    fn pread(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
        self.seek(SeekFrom::Start(off))?;
        self.read_exact(buf)
    }
    fn pwrite(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()> {
        self.seek(SeekFrom::Start(off))?;
        self.write_all(buf)
    }
    fn size(&self) -> std::io::Result<u64> {
        Ok(self.metadata()?.len())
    }
    fn grow(&mut self, len: u64) -> std::io::Result<()> {
        self.set_len(len)
    }
    fn fsync(&mut self) -> std::io::Result<()> {
        self.sync_all()
    }
}

/// An in-memory [`BackingIo`] over a growable byte buffer. Useful for tests and as the substrate for a
/// non-file `FileStore` (via [`FileStore::with_backing`]); it is the simplest non-`File` backing and
/// proves the abstraction the browser OPFS backend plugs into. Not persisted - dropping it
/// loses the data.
#[derive(Debug, Default)]
pub struct MemoryBacking {
    bytes: Vec<u8>,
}

impl MemoryBacking {
    /// An empty in-memory backing.
    pub fn new() -> Self {
        Self::default()
    }
    /// Construct from existing bytes (e.g. a previously-saved `.loom` image).
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }
    /// A copy of the current bytes (the full `.loom` image).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }
}

impl BackingIo for MemoryBacking {
    fn pread(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<()> {
        let off = off as usize;
        let end = off.checked_add(buf.len()).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "read offset overflow")
        })?;
        if end > self.bytes.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "read past end of memory backing",
            ));
        }
        buf.copy_from_slice(&self.bytes[off..end]);
        Ok(())
    }
    fn pwrite(&mut self, off: u64, buf: &[u8]) -> std::io::Result<()> {
        let off = off as usize;
        let end = off.checked_add(buf.len()).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "write offset overflow")
        })?;
        if end > self.bytes.len() {
            self.bytes.resize(end, 0); // grow with zeros, like a sparse file
        }
        self.bytes[off..end].copy_from_slice(buf);
        Ok(())
    }
    fn size(&self) -> std::io::Result<u64> {
        Ok(self.bytes.len() as u64)
    }
    fn grow(&mut self, len: u64) -> std::io::Result<()> {
        self.bytes.resize(len as usize, 0); // also truncates, matching File::set_len
        Ok(())
    }
    fn fsync(&mut self) -> std::io::Result<()> {
        Ok(()) // in-memory: nothing to flush
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planning_backing_preserves_exact_overlapping_writes_until_apply() {
        let mut source = MemoryBacking::from_bytes(vec![0; 32]);
        let prepared = {
            let mut planning = PlanningBacking::new(&mut source).unwrap();
            planning.pwrite(4, &[1, 2, 3, 4]).unwrap();
            planning.pwrite(6, &[9, 8, 7]).unwrap();
            let mut observed = [0; 7];
            planning.pread(3, &mut observed).unwrap();
            assert_eq!(observed, [0, 1, 2, 9, 8, 7, 0]);
            planning.fsync().unwrap();
            planning.finish()
        };
        assert_eq!(source.to_bytes(), vec![0; 32]);
        let mut fsyncs = 0;
        prepared.apply(&mut source, |_| fsyncs += 1).unwrap();
        assert_eq!(fsyncs, 1);
        assert_eq!(&source.to_bytes()[4..9], &[1, 2, 9, 8, 7]);
    }
}

pub(crate) fn write_at(f: &mut dyn BackingIo, off: u64, buf: &[u8]) -> std::io::Result<()> {
    f.pwrite(off, buf)
}
pub(crate) fn read_exact_at(
    f: &mut dyn BackingIo,
    off: u64,
    buf: &mut [u8],
) -> std::io::Result<()> {
    f.pread(off, buf)
}
