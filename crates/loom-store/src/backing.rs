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
