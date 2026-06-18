//! Per-object storage frames: a compression transform applied to a record's payload below the
//! content-address boundary. The address is over the plaintext canonical bytes, so a frame never
//! changes a digest. DEFLATE uses `miniz_oxide`, LZ4 uses `lz4_flex`; both are pure-Rust and
//! `wasm32`-clean.

use crate::corrupt;
use loom_core::CompressionHint;
use loom_core::Digest;
use loom_core::error::Result;
use loom_core::keys::{self, DekSession, Suite};

pub(crate) const FRAME_IDENTITY: u8 = 0x00;
pub(crate) const FRAME_DEFLATE: u8 = 0x01;
pub(crate) const FRAME_LZ4: u8 = 0x02;

/// AEAD object frames. A sealed frame is `0x10 + inner_codec_id`, so the inner compression
/// transform is still recorded: `0x10` = identity+encrypt, `0x11` = DEFLATE+encrypt, `0x12` =
/// LZ4+encrypt. Encryption sits below the content address, so the digest is still over the plaintext
/// canonical bytes and an encrypted Loom shares object identity with a plaintext one.
pub(crate) const FRAME_AEAD_BASE: u8 = 0x10;
/// Frame-format version bound into the AEAD associated data, so a future layout change can't be
/// reinterpreted under the v1 rules.
const FRAME_VERSION: u8 = 1;
/// Domain separator for object-frame associated data (distinct from the key-layer's own contexts).
const FRAME_AD_DOMAIN: &[u8] = b"uldren-loom/objframe/v1";

/// True if `frame` is one of the AEAD-sealed frame ids (`0x10`-`0x12`).
pub(crate) fn is_aead_frame(frame: u8) -> bool {
    matches!(frame, FRAME_AEAD_BASE..=0x12)
}

/// Payloads below this size are stored uncompressed; per-object framing of tiny payloads commonly
/// expands them.
const COMPRESS_THRESHOLD: usize = 1024;
const DEFLATE_LEVEL: u8 = 6;

/// The codec a store attempts on write. The stored frame is identity instead if compression would
/// not shrink the payload, or the payload is below `COMPRESS_THRESHOLD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// Never compress.
    None,
    /// DEFLATE via `miniz_oxide`.
    Deflate,
    /// LZ4 via `lz4_flex`.
    Lz4,
}

/// Map a [`CompressionHint`] to a concrete store codec.
pub(crate) fn codec_for_hint(hint: CompressionHint) -> Codec {
    match hint {
        CompressionHint::None => Codec::None,
        CompressionHint::Fast => Codec::Lz4,
        CompressionHint::Small => Codec::Deflate,
    }
}

/// Transform `plain` for storage using `codec`, returning `(frame_id, stored_bytes)`. Falls back to
/// identity when the payload is below the size threshold or compression does not shrink it, so a
/// stored record is never larger than its plaintext.
pub(crate) fn encode_payload(codec: Codec, plain: &[u8]) -> (u8, Vec<u8>) {
    if matches!(codec, Codec::None) || plain.len() < COMPRESS_THRESHOLD {
        return (FRAME_IDENTITY, plain.to_vec());
    }
    let (frame, compressed) = match codec {
        Codec::Deflate => (
            FRAME_DEFLATE,
            miniz_oxide::deflate::compress_to_vec(plain, DEFLATE_LEVEL),
        ),
        Codec::Lz4 => (FRAME_LZ4, lz4_flex::compress_prepend_size(plain)),
        Codec::None => unreachable!(),
    };
    if compressed.len() < plain.len() {
        (frame, compressed)
    } else {
        (FRAME_IDENTITY, plain.to_vec()) // incompressible: don't pay to store it larger
    }
}

/// Recover the plaintext canonical bytes from a stored `frame` + `stored` payload. The caller still
/// verifies the store-profile digest and `len == plain_len` (this only inverts the transform).
pub(crate) fn decode_payload(frame: u8, stored: &[u8]) -> Result<Vec<u8>> {
    match frame {
        FRAME_IDENTITY => Ok(stored.to_vec()),
        FRAME_DEFLATE => miniz_oxide::inflate::decompress_to_vec(stored)
            .map_err(|_| corrupt("deflate frame failed to decompress")),
        FRAME_LZ4 => lz4_flex::decompress_size_prepended(stored)
            .map_err(|_| corrupt("lz4 frame failed to decompress")),
        other => Err(corrupt(&format!("unknown storage frame id {other:#04x}"))),
    }
}

/// Associated data bound into every sealed object frame. All of it is recoverable from the record
/// header *before* decryption, so AD verification needs no plaintext: domain tag, frame-format version,
/// the on-disk frame id (which encodes the inner codec + the encrypt bit), the AEAD suite id, the
/// plaintext digest, and the plaintext/stored lengths. This binds the frame to its content address and
/// to its declared lengths, so a relocated or length-confused frame fails authentication. Object *type*
/// is deliberately **not** bound: the record format does not carry it and it is not available before
/// decrypting (bind only pre-decrypt metadata in v1).
fn frame_aad(
    aead_frame_id: u8,
    suite_id: u8,
    digest: &Digest,
    plain_len: u64,
    stored_len: u64,
) -> Vec<u8> {
    let mut ad = Vec::with_capacity(FRAME_AD_DOMAIN.len() + 3 + 32 + 16);
    ad.extend_from_slice(FRAME_AD_DOMAIN);
    ad.push(FRAME_VERSION);
    ad.push(aead_frame_id);
    ad.push(suite_id);
    ad.extend_from_slice(digest.bytes());
    ad.extend_from_slice(&plain_len.to_le_bytes());
    ad.extend_from_slice(&stored_len.to_le_bytes());
    ad
}

/// Seal an already-compressed (or identity) inner payload into an AEAD object frame. Returns
/// `(aead_frame_id, suite_id || nonce || ciphertext || tag)`. The caller supplies a **fresh** nonce of
/// [`Suite::nonce_len`] bytes (native RNG); reusing a nonce under one CEK is a hard AEAD violation, so
/// nonce freshness is the caller's discipline and the per-object CEK further narrows reuse blast radius.
/// `plain_len` is the plaintext length (bound in the AD); the inverse is [`open_aead_frame`].
pub(crate) fn seal_aead_frame(
    inner_frame_id: u8,
    inner: &[u8],
    session: &DekSession,
    digest: &Digest,
    plain_len: u64,
    nonce: &[u8],
) -> Result<(u8, Vec<u8>)> {
    let suite = session.active_suite();
    let aead_frame_id = FRAME_AEAD_BASE + inner_frame_id; // 0x00->0x10, 0x01->0x11, 0x02->0x12
    // The on-disk stored length is deterministic from the inputs (AEAD tag is 16 bytes for both
    // suites), so it can be bound in the AD even though it describes the bytes we are about to write.
    let stored_len = (1 + nonce.len() + inner.len() + 16) as u64;
    let aad = frame_aad(aead_frame_id, suite.id(), digest, plain_len, stored_len);
    let cek = session.derive_cek(suite, digest);
    let ciphertext = keys::seal(suite, &cek, nonce, &aad, inner)?;
    let mut out = Vec::with_capacity(1 + nonce.len() + ciphertext.len());
    out.push(suite.id());
    out.extend_from_slice(nonce);
    out.extend_from_slice(&ciphertext);
    debug_assert_eq!(out.len() as u64, stored_len, "sealed frame length mismatch");
    Ok((aead_frame_id, out))
}

/// Open an AEAD object frame (the inverse of [`seal_aead_frame`]): authenticate + decrypt under the
/// session's DEK, then invert the inner compression frame. `stored` is `suite_id || nonce || ciphertext
/// || tag`; `plain_len`/`stored_len` come from the record header and are bound in the AD. AEAD failure
/// (tamper, wrong key, wrong suite) surfaces as `E2eKeyInvalid` *before* any plaintext is returned.
pub(crate) fn open_aead_frame(
    aead_frame_id: u8,
    stored: &[u8],
    session: &DekSession,
    digest: &Digest,
    plain_len: u64,
    stored_len: u64,
) -> Result<Vec<u8>> {
    let suite_id = *stored.first().ok_or_else(|| corrupt("empty AEAD frame"))?;
    let suite = Suite::from_id(suite_id)?;
    let nonce_len = suite.nonce_len();
    if stored.len() < 1 + nonce_len + 16 {
        return Err(corrupt("AEAD frame shorter than nonce + tag"));
    }
    let nonce = &stored[1..1 + nonce_len];
    let ciphertext = &stored[1 + nonce_len..];
    let aad = frame_aad(aead_frame_id, suite_id, digest, plain_len, stored_len);
    let cek = session.derive_cek(suite, digest);
    let inner = keys::unseal(suite, &cek, nonce, &aad, ciphertext)?;
    let inner_frame_id = aead_frame_id - FRAME_AEAD_BASE;
    decode_payload(inner_frame_id, &inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compressible(n: usize) -> Vec<u8> {
        b"the quick brown loom object commit tree branch "
            .iter()
            .copied()
            .cycle()
            .take(n)
            .collect()
    }

    #[test]
    fn round_trips_each_codec() {
        let data = compressible(8192);
        for codec in [Codec::None, Codec::Deflate, Codec::Lz4] {
            let (frame, stored) = encode_payload(codec, &data);
            assert_eq!(
                decode_payload(frame, &stored).unwrap(),
                data,
                "codec {codec:?}"
            );
        }
    }

    #[test]
    fn compressible_data_shrinks_under_a_real_codec() {
        let data = compressible(8192);
        let (frame_d, stored_d) = encode_payload(Codec::Deflate, &data);
        assert_eq!(frame_d, FRAME_DEFLATE);
        assert!(stored_d.len() < data.len());
        let (frame_l, stored_l) = encode_payload(Codec::Lz4, &data);
        assert_eq!(frame_l, FRAME_LZ4);
        assert!(stored_l.len() < data.len());
    }

    #[test]
    fn tiny_and_incompressible_fall_back_to_identity() {
        // Below the threshold -> identity regardless of codec.
        let (frame, stored) = encode_payload(Codec::Deflate, b"small");
        assert_eq!(frame, FRAME_IDENTITY);
        assert_eq!(stored, b"small");

        // Large but incompressible (high-entropy SplitMix64) -> kept identity because it won't shrink.
        let mut s = 0x1234_5678_9abc_def0u64;
        let random: Vec<u8> = (0..4096)
            .map(|_| {
                s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = s;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                ((z ^ (z >> 31)) & 0xff) as u8
            })
            .collect();
        let (frame, _) = encode_payload(Codec::Deflate, &random);
        assert_eq!(
            frame, FRAME_IDENTITY,
            "incompressible data must stay identity, not expand"
        );
    }

    #[test]
    fn unknown_frame_is_clean_error() {
        assert!(decode_payload(0x7f, b"x").is_err());
    }

    use loom_core::Code;
    use loom_core::keys::{DekSession, Suite};

    fn session(suite: Suite) -> DekSession {
        DekSession::from_raw([0x42; 32], suite)
    }

    /// A sealed frame round-trips for every inner codec under both suites, and the on-disk frame id
    /// records the inner codec (`0x10`/`0x11`/`0x12`).
    #[test]
    fn aead_frame_round_trips_each_inner_codec_and_suite() {
        let plain = compressible(8192);
        let digest = Digest::blake3(&plain);
        for suite in [Suite::XChaCha20Poly1305, Suite::Aes256Gcm] {
            let s = session(suite);
            for (codec, want_inner) in [
                (Codec::None, FRAME_IDENTITY),
                (Codec::Deflate, FRAME_DEFLATE),
                (Codec::Lz4, FRAME_LZ4),
            ] {
                let (inner_id, inner) = encode_payload(codec, &plain);
                assert_eq!(inner_id, want_inner);
                let nonce = vec![0x11u8; suite.nonce_len()];
                let (aead_id, stored) =
                    seal_aead_frame(inner_id, &inner, &s, &digest, plain.len() as u64, &nonce)
                        .unwrap();
                assert_eq!(aead_id, FRAME_AEAD_BASE + want_inner);
                assert!(is_aead_frame(aead_id));
                // The plaintext must not appear verbatim in the sealed bytes.
                assert!(!contains(&stored, &plain[..64]));
                let stored_len = stored.len() as u64;
                let opened = open_aead_frame(
                    aead_id,
                    &stored,
                    &s,
                    &digest,
                    plain.len() as u64,
                    stored_len,
                )
                .unwrap();
                assert_eq!(opened, plain, "suite {suite:?} codec {codec:?}");
            }
        }
    }

    /// Flipping any ciphertext byte fails AEAD authentication (E2eKeyInvalid) before any plaintext is
    /// returned. The same holds for a wrong key and for associated-data the frame was not sealed under.
    #[test]
    fn aead_frame_rejects_tamper_wrong_key_and_rebound_metadata() {
        let plain = b"frame-level secret bytes for the AEAD adversarial checks".to_vec();
        let digest = Digest::blake3(&plain);
        let s = session(Suite::Aes256Gcm);
        let nonce = vec![0x05u8; Suite::Aes256Gcm.nonce_len()];
        let (aead_id, stored) = seal_aead_frame(
            FRAME_IDENTITY,
            &plain,
            &s,
            &digest,
            plain.len() as u64,
            &nonce,
        )
        .unwrap();
        let stored_len = stored.len() as u64;

        // Tamper: flip the last ciphertext byte (inside the tag).
        let mut bad = stored.clone();
        *bad.last_mut().unwrap() ^= 0x01;
        let e = open_aead_frame(aead_id, &bad, &s, &digest, plain.len() as u64, stored_len)
            .unwrap_err();
        assert_eq!(e.code, Code::E2eKeyInvalid);

        // Wrong key: a different DEK cannot open it.
        let other = DekSession::from_raw([0x07; 32], Suite::Aes256Gcm);
        let e = open_aead_frame(
            aead_id,
            &stored,
            &other,
            &digest,
            plain.len() as u64,
            stored_len,
        )
        .unwrap_err();
        assert_eq!(e.code, Code::E2eKeyInvalid);

        // Rebound AD: opening under a different digest (relocation/length confusion) fails the AD bind.
        let wrong_digest = Digest::blake3(b"a different object");
        let e = open_aead_frame(
            aead_id,
            &stored,
            &s,
            &wrong_digest,
            plain.len() as u64,
            stored_len,
        )
        .unwrap_err();
        assert_eq!(e.code, Code::E2eKeyInvalid);
        // And a length-confused stored_len in the AD fails too.
        let e = open_aead_frame(
            aead_id,
            &stored,
            &s,
            &digest,
            plain.len() as u64,
            stored_len + 1,
        )
        .unwrap_err();
        assert_eq!(e.code, Code::E2eKeyInvalid);
    }

    /// A fresh nonce per object means two seals of the same plaintext under the same key produce
    /// different ciphertext - the discipline that keeps AES-GCM's 96-bit nonce safe.
    #[test]
    fn distinct_nonces_yield_distinct_ciphertext() {
        let plain = b"identical plaintext".to_vec();
        let digest = Digest::blake3(&plain);
        let s = session(Suite::Aes256Gcm);
        let n = Suite::Aes256Gcm.nonce_len();
        let (_, a) = seal_aead_frame(
            FRAME_IDENTITY,
            &plain,
            &s,
            &digest,
            plain.len() as u64,
            &vec![1u8; n],
        )
        .unwrap();
        let (_, b) = seal_aead_frame(
            FRAME_IDENTITY,
            &plain,
            &s,
            &digest,
            plain.len() as u64,
            &vec![2u8; n],
        )
        .unwrap();
        assert_ne!(a, b);
    }

    /// A frame whose recorded suite id is unknown, or whose body is too short for a nonce + tag, is a
    /// clean error rather than a panic.
    #[test]
    fn aead_frame_rejects_unknown_suite_and_short_body() {
        let s = session(Suite::Aes256Gcm);
        let digest = Digest::blake3(b"x");
        // suite id 0x03 is reserved/unknown.
        let bogus = vec![0x03u8; 1 + 12 + 16];
        assert!(open_aead_frame(0x10, &bogus, &s, &digest, 1, bogus.len() as u64).is_err());
        // A body shorter than suite_id + nonce + tag is rejected.
        let short = vec![Suite::Aes256Gcm.id(); 4];
        assert!(open_aead_frame(0x10, &short, &s, &digest, 1, short.len() as u64).is_err());
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Golden conformance vectors for the sealed-frame wire format. With a fixed DEK
    /// (`0x42` x32), a fixed object digest, and a fixed nonce (`0x24` repeated), the sealed bytes are
    /// fully deterministic for each suite: `suite_id || nonce || ciphertext || tag`. Pinning the exact
    /// bytes locks both the CEK derivation (keyed-BLAKE3 for XChaCha, HKDF-SHA-256 for AES) and the AEAD
    /// output, so any future change to the frame layout, the KDF, or the associated-data construction is
    /// caught here rather than silently breaking cross-version / cross-platform reads. To regenerate,
    /// run `emit_golden_vectors` (below) with `--ignored --nocapture`.
    #[test]
    fn sealed_frame_golden_vectors() {
        let digest = Digest::blake3(b"loom-encryption-conformance-vector-object");
        assert_eq!(
            hex(digest.bytes()),
            "203ee702f0ab62296ff9d11fe22f6772141659ef02f6b4e76d829e149d81b218"
        );
        let plain = b"conformance plaintext: the quick brown loom".to_vec();
        let cases = [
            (
                Suite::XChaCha20Poly1305,
                "0124242424242424242424242424242424242424242424242491f6b445a5411ea048a6d3939c27677b797c272ba3667229321d6f1447b92e78a61f82bf3204bde768fa96680bcc9327862ae05c8659fb6c155929",
            ),
            (
                Suite::Aes256Gcm,
                "022424242424242424242424243fdeecdfbc28ec79f3ac80351d81ed1b2d746a728aef81b001056b512109e8440476f11ae0badad52a1d1a07cb01d007c5401c89ddc56d0be3a403",
            ),
        ];
        for (suite, want_hex) in cases {
            let s = session(suite);
            let nonce = vec![0x24u8; suite.nonce_len()];
            let (fid, stored) = seal_aead_frame(
                FRAME_IDENTITY,
                &plain,
                &s,
                &digest,
                plain.len() as u64,
                &nonce,
            )
            .unwrap();
            assert_eq!(fid, FRAME_AEAD_BASE, "identity-inner frame id");
            assert_eq!(hex(&stored), want_hex, "sealed bytes for {suite:?}");
            // The pinned bytes must open back to the original plaintext.
            let want = unhex(want_hex);
            assert_eq!(stored, want);
            let opened = open_aead_frame(
                fid,
                &want,
                &s,
                &digest,
                plain.len() as u64,
                want.len() as u64,
            )
            .unwrap();
            assert_eq!(opened, plain);
        }
    }

    #[test]
    #[ignore = "run with --ignored --nocapture to regenerate the golden vectors above"]
    fn emit_golden_vectors() {
        let digest = Digest::blake3(b"loom-encryption-conformance-vector-object");
        eprintln!("DIGEST {}", hex(digest.bytes()));
        for suite in [Suite::XChaCha20Poly1305, Suite::Aes256Gcm] {
            let s = session(suite);
            let plain = b"conformance plaintext: the quick brown loom".to_vec();
            let nonce = vec![0x24u8; suite.nonce_len()];
            let (fid, stored) = seal_aead_frame(
                FRAME_IDENTITY,
                &plain,
                &s,
                &digest,
                plain.len() as u64,
                &nonce,
            )
            .unwrap();
            eprintln!(
                "SUITE {} FID {:#04x} STORED {}",
                suite.as_str(),
                fid,
                hex(&stored)
            );
        }
    }
}
