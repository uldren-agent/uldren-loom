//! FastCDC content-defined chunking: split a large blob into variable-length chunks at content-derived
//! boundaries, so an edit re-chunks only the affected region and unchanged chunks keep their digests
//! (dedup across versions). The boundaries are a deterministic pure function of the bytes - the gear
//! table and masks are fixed - so every peer/platform chunks identical content identically, which is
//! required for the chunks' addresses to match.
//!
//! Normalized chunking (FastCDC, level 2): below the average size a *harder* mask makes a cut unlikely
//! (avoid tiny chunks); above it an *easier* mask makes a cut likely (avoid huge chunks); `MIN`/`MAX`
//! bound the result.

/// Minimum chunk size: no boundary is taken before this many bytes.
const MIN_SIZE: usize = 512 * 1024;
/// Target average chunk size (a power of two; `log2` drives the masks).
const AVG_SIZE: usize = 1024 * 1024;
/// Maximum chunk size: a boundary is forced here if none was found.
const MAX_SIZE: usize = 4 * 1024 * 1024;
/// Content at or below this size is stored as a single Blob (no ChunkList); above it, chunked.
pub(crate) const CHUNK_THRESHOLD: usize = MIN_SIZE;

// Mask widths in bits = log2(AVG) +/- 2. We test "the top N bits of the rolling hash are zero" (prob
// 2^-N): the wider mask (harder) applies below AVG, the narrower (easier) above it.
const AVG_BITS: u32 = AVG_SIZE.trailing_zeros();
const MASK_S_BITS: u32 = AVG_BITS + 2;
const MASK_L_BITS: u32 = AVG_BITS - 2;

/// The gear table: 256 fixed pseudo-random u64s, built at compile time from a fixed SplitMix64 seed so
/// it is identical everywhere (determinism is mandatory - a different table re-chunks all content).
const GEAR: [u64; 256] = build_gear();

const fn build_gear() -> [u64; 256] {
    let mut table = [0u64; 256];
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut i = 0;
    while i < 256 {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        table[i] = z;
        i += 1;
    }
    table
}

/// The cut point (chunk length in bytes) for the chunk starting at the front of `data`: the first
/// content-defined boundary in `[MIN_SIZE, MAX_SIZE]`, or `MAX_SIZE` if none is found, or the whole
/// slice if it is shorter than `MIN_SIZE`.
fn cut_point(data: &[u8]) -> usize {
    let len = data.len();
    if len <= MIN_SIZE {
        return len;
    }
    let max_scan = len.min(MAX_SIZE);
    let mut fp = 0u64;
    let mut i = 0usize;
    while i < max_scan {
        fp = (fp << 1).wrapping_add(GEAR[data[i] as usize]);
        i += 1;
        if i < MIN_SIZE {
            continue;
        }
        let bits = if i < AVG_SIZE {
            MASK_S_BITS
        } else {
            MASK_L_BITS
        };
        if fp >> (64 - bits) == 0 {
            return i;
        }
    }
    max_scan
}

/// Split `data` into content-defined chunks. The concatenation of the returned slices is exactly
/// `data`, every chunk but the last is in `[MIN_SIZE, MAX_SIZE]`, and the split is deterministic.
pub(crate) fn chunk(data: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let c = cut_point(rest);
        out.push(&rest[..c]);
        rest = &rest[c..];
    }
    out
}

/// Incremental form of [`chunk`] for bounded-memory streaming: content is pushed in arbitrary-sized
/// blocks and complete chunks are emitted as their boundaries settle, so a large file is never held
/// in memory at once. The cut points are identical to [`chunk`] over the same concatenated bytes,
/// because both take the first content-defined boundary in `[MIN_SIZE, MAX_SIZE]` greedily from each
/// chunk's start; the streamer only defers emitting a chunk until enough bytes have arrived to prove a
/// boundary (a real cut strictly inside the buffer, or the forced `MAX_SIZE` cut). The pending buffer
/// never exceeds `MAX_SIZE`.
pub(crate) struct StreamChunker {
    buf: Vec<u8>,
}

impl StreamChunker {
    pub(crate) fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Append `data` and emit every chunk whose boundary is now settled.
    pub(crate) fn push(&mut self, data: &[u8], mut emit: impl FnMut(&[u8])) {
        self.buf.extend_from_slice(data);
        loop {
            if self.buf.is_empty() {
                break;
            }
            let c = cut_point(&self.buf);
            if c < self.buf.len() {
                // A boundary (content-defined, or forced at MAX_SIZE when the buffer is longer) lies
                // strictly inside the buffer: this chunk is final.
                emit(&self.buf[..c]);
                self.buf.drain(..c);
            } else if self.buf.len() >= MAX_SIZE {
                // Exactly MAX_SIZE with no earlier boundary: a forced cut.
                emit(&self.buf);
                self.buf.clear();
                break;
            } else {
                // No settled boundary yet; wait for more bytes.
                break;
            }
        }
    }

    /// Emit all remaining buffered bytes as final chunks.
    pub(crate) fn finish(self, mut emit: impl FnMut(&[u8])) {
        let mut rest: &[u8] = &self.buf;
        while !rest.is_empty() {
            let c = cut_point(rest);
            emit(&rest[..c]);
            rest = &rest[c..];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A deterministic pseudo-random byte stream (SplitMix64), so the test pins cross-platform behavior.
    fn bytes(n: usize, seed: u64) -> Vec<u8> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = s;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                ((z ^ (z >> 31)) & 0xff) as u8
            })
            .collect()
    }

    #[test]
    fn chunks_reassemble_and_respect_bounds() {
        let data = bytes(10 * 1024 * 1024, 42);
        let chunks = chunk(&data);
        assert!(chunks.len() > 1, "large input should split into chunks");
        // Concatenation is exact.
        let joined: Vec<u8> = chunks.iter().flat_map(|c| c.iter().copied()).collect();
        assert_eq!(joined, data);
        // Every chunk but the last is within [MIN, MAX]; the last is just <= MAX.
        for (i, c) in chunks.iter().enumerate() {
            assert!(c.len() <= MAX_SIZE, "chunk over MAX");
            if i + 1 < chunks.len() {
                assert!(c.len() >= MIN_SIZE, "non-final chunk under MIN");
            }
        }
    }

    #[test]
    fn chunking_is_deterministic() {
        let data = bytes(3 * 1024 * 1024, 7);
        let a: Vec<usize> = chunk(&data).iter().map(|c| c.len()).collect();
        let b: Vec<usize> = chunk(&data).iter().map(|c| c.len()).collect();
        assert_eq!(a, b, "identical input must chunk identically");
    }

    #[test]
    fn an_edit_re_chunks_only_locally() {
        // Insert a byte near the end; chunk boundaries before the edit must be unchanged (dedup).
        let data = bytes(6 * 1024 * 1024, 99);
        let mut edited = data.clone();
        edited.insert(5 * 1024 * 1024, 0xAB);
        let orig: Vec<usize> = chunk(&data).iter().map(|c| c.len()).collect();
        let new: Vec<usize> = chunk(&edited).iter().map(|c| c.len()).collect();
        // The first several chunks (well before the edit) are byte-identical in length.
        assert_eq!(orig[0], new[0]);
        assert_eq!(orig[1], new[1]);
        assert_eq!(orig[2], new[2]);
    }

    #[test]
    fn small_data_is_one_chunk() {
        let data = bytes(MIN_SIZE - 1, 1);
        let chunks = chunk(&data);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), MIN_SIZE - 1);
    }

    #[test]
    fn stream_chunker_matches_batch_for_any_block_size() {
        let data = bytes(3 * 1024 * 1024, 17);
        let want: Vec<Vec<u8>> = chunk(&data).iter().map(|c| c.to_vec()).collect();
        // Feed the same content in several different block sizes; the emitted chunks must be identical
        // to the batch split every time, proving streaming does not change boundaries.
        for block in [64 * 1024, MIN_SIZE, AVG_SIZE, MAX_SIZE] {
            let mut got: Vec<Vec<u8>> = Vec::new();
            let mut sc = StreamChunker::new();
            for piece in data.chunks(block) {
                sc.push(piece, |c| got.push(c.to_vec()));
            }
            sc.finish(|c| got.push(c.to_vec()));
            assert_eq!(got, want, "block size {block} changed the chunk boundaries");
            let joined: Vec<u8> = got.iter().flatten().copied().collect();
            assert_eq!(joined, data, "block size {block} lost bytes");
        }
    }
}
