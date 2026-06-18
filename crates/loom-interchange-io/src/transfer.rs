//! Server-side transfer staging for the byte-transfer interchange contract (`specs/0067` §17).
//!
//! [`TransferStaging`] is a bounded, cancellable, lease-expiring buffer keyed by one import
//! transfer. The server accumulates chunked writes at monotonic sequence numbers, tracks accepted
//! bytes and remaining credit for backpressure, maintains a running digest, and validates it
//! against the caller's `final_digest` before the interchange is applied. It holds no store handle
//! and applies no interchange itself: it is the staging seam the `Transfer` methods build on (task
//! 553), so both the path-shaped local/admin methods and the byte-transfer path can share the same
//! kind-keyed byte codecs (archive/car here, columnar arrow-ipc/parquet in `loom-columnar`).

use loom_types::{Algo, Code, Digest, LoomError, Result};
use std::time::{Duration, Instant};

/// A transfer payload format (`specs/0067` §17.2). A `kind` is a format, never a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransferKind {
    FsTree,
    Tar,
    TarZstd,
    TarGzip,
    Zip,
    Gzip,
    Car,
    ArrowIpc,
    Parquet,
}

impl TransferKind {
    /// The stable wire name for this kind.
    pub const fn as_str(self) -> &'static str {
        match self {
            TransferKind::FsTree => "fs-tree",
            TransferKind::Tar => "tar",
            TransferKind::TarZstd => "tar-zstd",
            TransferKind::TarGzip => "tar-gzip",
            TransferKind::Zip => "zip",
            TransferKind::Gzip => "gzip",
            TransferKind::Car => "car",
            TransferKind::ArrowIpc => "arrow-ipc",
            TransferKind::Parquet => "parquet",
        }
    }

    /// Parse the stable wire name emitted by [`TransferKind::as_str`].
    pub fn parse(name: &str) -> Result<Self> {
        Ok(match name {
            "fs-tree" => TransferKind::FsTree,
            "tar" => TransferKind::Tar,
            "tar-zstd" => TransferKind::TarZstd,
            "tar-gzip" => TransferKind::TarGzip,
            "zip" => TransferKind::Zip,
            "gzip" => TransferKind::Gzip,
            "car" => TransferKind::Car,
            "arrow-ipc" => TransferKind::ArrowIpc,
            "parquet" => TransferKind::Parquet,
            other => {
                return Err(LoomError::new(
                    Code::InvalidArgument,
                    format!("unknown transfer kind '{other}'"),
                ));
            }
        })
    }

    /// Whether the byte-transfer interchange is implemented for this kind. `fs-tree` has no
    /// dedicated manifest format yet (`specs/0067` §17.2) and rides an archive kind in practice, so
    /// it is not directly byte-transferable until that manifest is specified.
    pub const fn byte_transfer_supported(self) -> bool {
        !matches!(self, TransferKind::FsTree)
    }
}

/// Server-advertised staging limits (`specs/0067` §17.5: bounded chunks and bounded staging).
#[derive(Debug, Clone, Copy)]
pub struct StagingLimits {
    /// Largest single [`TransferStaging::write`] chunk the server accepts.
    pub max_chunk_bytes: u64,
    /// Largest total staged payload before the transfer is rejected.
    pub max_total_bytes: u64,
    /// How long the staging area lives without a touch before it expires.
    pub lease: Duration,
}

impl StagingLimits {
    pub const DEFAULT_MAX_CHUNK_BYTES: u64 = 8 * 1024 * 1024;
    pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
    pub const DEFAULT_LEASE_SECS: u64 = 300;
}

impl Default for StagingLimits {
    fn default() -> Self {
        Self {
            max_chunk_bytes: Self::DEFAULT_MAX_CHUNK_BYTES,
            max_total_bytes: Self::DEFAULT_MAX_TOTAL_BYTES,
            lease: Duration::from_secs(Self::DEFAULT_LEASE_SECS),
        }
    }
}

/// The write-side backpressure reply (`specs/0067` §17.3 `TransferAccept`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferAccept {
    /// Total bytes accepted for this transfer so far.
    pub accepted_bytes: u64,
    /// Remaining bytes the server will accept before the staging limit is reached.
    pub credit: u64,
}

/// A bounded, cancellable, lease-expiring staging buffer for one import transfer.
///
/// The buffer accumulates chunk bytes in order; the interchange itself (applying the payload to a
/// store under the write authority) is the caller's job at `finish` time (task 553). Drop the value
/// (or call [`TransferStaging::cancel`]) to release the staging area.
pub struct TransferStaging {
    kind: TransferKind,
    algo: Algo,
    limits: StagingLimits,
    buf: Vec<u8>,
    next_seq: u64,
    accepted_bytes: u64,
    last_touch: Instant,
    finished: bool,
}

impl TransferStaging {
    /// Open a staging area for `kind`, hashing with the store's `algo`, using `now` as the initial
    /// lease anchor. `open` uses [`Instant::now`]; `open_at` injects the clock for tests.
    pub fn open(kind: TransferKind, algo: Algo, limits: StagingLimits) -> Self {
        Self::open_at(kind, algo, limits, Instant::now())
    }

    pub fn open_at(kind: TransferKind, algo: Algo, limits: StagingLimits, now: Instant) -> Self {
        Self {
            kind,
            algo,
            limits,
            buf: Vec::new(),
            next_seq: 0,
            accepted_bytes: 0,
            last_touch: now,
            finished: false,
        }
    }

    pub fn kind(&self) -> TransferKind {
        self.kind
    }

    pub fn accepted_bytes(&self) -> u64 {
        self.accepted_bytes
    }

    /// Bytes the server will still accept before the staging limit is reached.
    pub fn credit(&self) -> u64 {
        self.limits
            .max_total_bytes
            .saturating_sub(self.accepted_bytes)
    }

    /// The next sequence number the staging area expects.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// The accumulated payload bytes accepted so far.
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn accept(&self) -> TransferAccept {
        TransferAccept {
            accepted_bytes: self.accepted_bytes,
            credit: self.credit(),
        }
    }

    /// Whether the lease has elapsed as of `now` (no successful write/touch within the lease).
    pub fn is_expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last_touch) > self.limits.lease
    }

    /// Append one bounded chunk at a monotonic `seq`.
    ///
    /// - Re-sending an already-accepted `seq` (`seq < next_seq`) is an idempotent no-op that returns
    ///   the current accepted/credit counters (`specs/0067` §17.3/§17.5).
    /// - A gap or rewind past the next expected `seq` is `InvalidArgument`.
    /// - An oversized chunk is `InvalidArgument`; exceeding the staging limit is `ResourceExhausted`.
    /// - An optional per-chunk `digest` is verified against the chunk bytes and rejected early with
    ///   `IntegrityFailure` on mismatch.
    pub fn write(
        &mut self,
        seq: u64,
        chunk: &[u8],
        chunk_digest: Option<&Digest>,
        now: Instant,
    ) -> Result<TransferAccept> {
        if self.finished {
            return Err(LoomError::new(
                Code::Conflict,
                "transfer is already finalized; no further writes are accepted",
            ));
        }
        if seq < self.next_seq {
            // Idempotent replay of an already-accepted chunk.
            self.last_touch = now;
            return Ok(self.accept());
        }
        if seq > self.next_seq {
            return Err(LoomError::new(
                Code::InvalidArgument,
                format!(
                    "transfer chunk seq {seq} is out of order (expected {})",
                    self.next_seq
                ),
            ));
        }
        let len = chunk.len() as u64;
        if len > self.limits.max_chunk_bytes {
            return Err(LoomError::new(
                Code::InvalidArgument,
                format!(
                    "transfer chunk of {len} bytes exceeds the {}-byte chunk limit",
                    self.limits.max_chunk_bytes
                ),
            ));
        }
        if self.accepted_bytes.saturating_add(len) > self.limits.max_total_bytes {
            return Err(LoomError::new(
                Code::ResourceExhausted,
                format!(
                    "transfer would exceed the {}-byte staging limit",
                    self.limits.max_total_bytes
                ),
            ));
        }
        if let Some(expected) = chunk_digest {
            let actual = Digest::hash(self.algo, chunk);
            if &actual != expected {
                return Err(LoomError::new(
                    Code::IntegrityFailure,
                    "transfer chunk digest mismatch",
                ));
            }
        }
        self.buf.extend_from_slice(chunk);
        self.next_seq += 1;
        self.accepted_bytes += len;
        self.last_touch = now;
        Ok(self.accept())
    }

    /// The running digest over the bytes accepted so far, using the store's algo. Computed over the
    /// accumulated (bounded) buffer, which is equivalent to an incremental digest.
    pub fn running_digest(&self) -> Digest {
        Digest::hash(self.algo, &self.buf)
    }

    /// Validate the staged bytes against `final_digest` and mark the staging finalized. The caller
    /// (task 553) then applies the interchange over these bytes under the write authority.
    ///
    /// Finalize-once: after a successful validation a repeated call with the same `final_digest`
    /// returns `Ok` (so a replayed `finish` is safe), while a mismatch is `IntegrityFailure`.
    pub fn validate_final(&mut self, final_digest: &Digest) -> Result<()> {
        let running = self.running_digest();
        if &running != final_digest {
            return Err(LoomError::new(
                Code::IntegrityFailure,
                format!(
                    "transfer final digest mismatch: staged {} != declared {}",
                    running.to_hex(),
                    final_digest.to_hex()
                ),
            ));
        }
        self.finished = true;
        Ok(())
    }

    /// Discard the staged bytes and release the buffer (`cancel`/lease expiry).
    pub fn cancel(&mut self) {
        self.buf = Vec::new();
        self.accepted_bytes = 0;
        self.next_seq = 0;
        self.finished = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(max_chunk: u64, max_total: u64, lease_ms: u64) -> StagingLimits {
        StagingLimits {
            max_chunk_bytes: max_chunk,
            max_total_bytes: max_total,
            lease: Duration::from_millis(lease_ms),
        }
    }

    #[test]
    fn transfer_kind_round_trips_names_and_flags_fs_tree() {
        for kind in [
            TransferKind::FsTree,
            TransferKind::Tar,
            TransferKind::TarZstd,
            TransferKind::TarGzip,
            TransferKind::Zip,
            TransferKind::Gzip,
            TransferKind::Car,
            TransferKind::ArrowIpc,
            TransferKind::Parquet,
        ] {
            assert_eq!(TransferKind::parse(kind.as_str()).unwrap(), kind);
        }
        assert!(!TransferKind::FsTree.byte_transfer_supported());
        assert!(TransferKind::Tar.byte_transfer_supported());
        assert!(TransferKind::Parquet.byte_transfer_supported());
        assert!(TransferKind::parse("no-such-kind").is_err());
    }

    #[test]
    fn staging_accepts_ordered_chunks_and_tracks_credit() {
        let now = Instant::now();
        let mut s =
            TransferStaging::open_at(TransferKind::Tar, Algo::Blake3, limits(4, 10, 1000), now);
        let a0 = s.write(0, b"abcd", None, now).unwrap();
        assert_eq!(
            a0,
            TransferAccept {
                accepted_bytes: 4,
                credit: 6
            }
        );
        let a1 = s.write(1, b"ef", None, now).unwrap();
        assert_eq!(
            a1,
            TransferAccept {
                accepted_bytes: 6,
                credit: 4
            }
        );
        assert_eq!(s.bytes(), b"abcdef");
        assert_eq!(s.next_seq(), 2);
    }

    #[test]
    fn staging_replay_of_accepted_seq_is_a_no_op() {
        let now = Instant::now();
        let mut s =
            TransferStaging::open_at(TransferKind::Tar, Algo::Blake3, limits(8, 100, 1000), now);
        s.write(0, b"hello", None, now).unwrap();
        s.write(1, b"world", None, now).unwrap();
        // Replay seq 0: no double-append, counters unchanged.
        let replay = s.write(0, b"hello", None, now).unwrap();
        assert_eq!(replay.accepted_bytes, 10);
        assert_eq!(s.bytes(), b"helloworld");
        assert_eq!(s.next_seq(), 2);
    }

    #[test]
    fn staging_rejects_out_of_order_seq() {
        let now = Instant::now();
        let mut s =
            TransferStaging::open_at(TransferKind::Tar, Algo::Blake3, limits(8, 100, 1000), now);
        s.write(0, b"a", None, now).unwrap();
        let err = s.write(2, b"c", None, now).unwrap_err();
        assert_eq!(err.code, Code::InvalidArgument);
    }

    #[test]
    fn staging_enforces_chunk_and_total_bounds() {
        let now = Instant::now();
        let mut s =
            TransferStaging::open_at(TransferKind::Tar, Algo::Blake3, limits(4, 6, 1000), now);
        assert_eq!(
            s.write(0, b"toolong!!", None, now).unwrap_err().code,
            Code::InvalidArgument
        );
        s.write(0, b"aaaa", None, now).unwrap();
        assert_eq!(
            s.write(1, b"bbbb", None, now).unwrap_err().code,
            Code::ResourceExhausted
        );
    }

    #[test]
    fn staging_verifies_optional_per_chunk_digest() {
        let now = Instant::now();
        let mut s =
            TransferStaging::open_at(TransferKind::Tar, Algo::Blake3, limits(8, 100, 1000), now);
        let good = Digest::hash(Algo::Blake3, b"data");
        s.write(0, b"data", Some(&good), now).unwrap();
        let wrong = Digest::hash(Algo::Blake3, b"other");
        let err = s.write(1, b"more", Some(&wrong), now).unwrap_err();
        assert_eq!(err.code, Code::IntegrityFailure);
    }

    #[test]
    fn staging_validate_final_matches_running_digest_and_is_finalize_once() {
        let now = Instant::now();
        let mut s =
            TransferStaging::open_at(TransferKind::Tar, Algo::Blake3, limits(8, 100, 1000), now);
        s.write(0, b"payload", None, now).unwrap();
        let good = Digest::hash(Algo::Blake3, b"payload");
        s.validate_final(&good).unwrap();
        assert!(s.is_finished());
        // Finalize-once: same digest replays Ok; further writes rejected.
        s.validate_final(&good).unwrap();
        assert_eq!(
            s.write(1, b"x", None, now).unwrap_err().code,
            Code::Conflict
        );
    }

    #[test]
    fn staging_rejects_bad_final_digest() {
        let now = Instant::now();
        let mut s =
            TransferStaging::open_at(TransferKind::Tar, Algo::Blake3, limits(8, 100, 1000), now);
        s.write(0, b"payload", None, now).unwrap();
        let bad = Digest::hash(Algo::Blake3, b"tampered");
        let err = s.validate_final(&bad).unwrap_err();
        assert_eq!(err.code, Code::IntegrityFailure);
        assert!(!s.is_finished());
    }

    #[test]
    fn staging_lease_expires_after_the_lease_window() {
        let now = Instant::now();
        let s = TransferStaging::open_at(TransferKind::Tar, Algo::Blake3, limits(8, 100, 50), now);
        assert!(!s.is_expired(now + Duration::from_millis(10)));
        assert!(s.is_expired(now + Duration::from_millis(60)));
    }

    #[test]
    fn staging_cancel_releases_the_buffer() {
        let now = Instant::now();
        let mut s =
            TransferStaging::open_at(TransferKind::Tar, Algo::Blake3, limits(8, 100, 1000), now);
        s.write(0, b"payload", None, now).unwrap();
        s.cancel();
        assert!(s.bytes().is_empty());
        assert_eq!(s.accepted_bytes(), 0);
        assert_eq!(s.next_seq(), 0);
    }
}
