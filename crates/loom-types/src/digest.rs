//! Content addresses.

use crate::error::{Code, LoomError, Result};
use std::fmt;

pub const DIGEST_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Algo {
    Blake3,
    Sha256,
}

impl Algo {
    pub const fn as_str(self) -> &'static str {
        match self {
            Algo::Blake3 => "blake3",
            Algo::Sha256 => "sha256",
        }
    }

    pub const fn code(self) -> u8 {
        match self {
            Algo::Blake3 => 0x1e,
            Algo::Sha256 => 0x12,
        }
    }

    pub fn from_code(code: u8) -> Result<Self> {
        match code {
            0x1e => Ok(Algo::Blake3),
            0x12 => Ok(Algo::Sha256),
            other => Err(LoomError::new(
                Code::Unsupported,
                format!("unknown digest algo code {other:#04x}"),
            )),
        }
    }

    /// Parse the stable name emitted by [`Algo::as_str`] (the `algo:` prefix of a content address).
    pub fn from_name(name: &str) -> Result<Self> {
        match name {
            "blake3" => Ok(Algo::Blake3),
            "sha256" => Ok(Algo::Sha256),
            other => Err(LoomError::new(
                Code::Unsupported,
                format!("unknown digest algo '{other}'"),
            )),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Digest {
    algo: Algo,
    bytes: [u8; DIGEST_LEN],
}

impl PartialEq for Digest {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Eq for Digest {}

impl PartialOrd for Digest {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Digest {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.bytes.cmp(&other.bytes)
    }
}

impl std::hash::Hash for Digest {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.bytes.hash(state);
    }
}

impl Digest {
    pub const fn algo(&self) -> Algo {
        self.algo
    }

    pub const fn bytes(&self) -> &[u8; DIGEST_LEN] {
        &self.bytes
    }

    pub fn hash(algo: Algo, data: &[u8]) -> Self {
        let bytes = match algo {
            Algo::Blake3 => *blake3::hash(data).as_bytes(),
            Algo::Sha256 => {
                use sha2::{Digest as _, Sha256};
                Sha256::digest(data).into()
            }
        };
        Self { algo, bytes }
    }

    pub const fn of(algo: Algo, bytes: [u8; DIGEST_LEN]) -> Self {
        Self { algo, bytes }
    }

    pub fn blake3(data: &[u8]) -> Self {
        Self::hash(Algo::Blake3, data)
    }

    pub const fn from_blake3_bytes(bytes: [u8; DIGEST_LEN]) -> Self {
        Self::of(Algo::Blake3, bytes)
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.bytes)
    }

    pub fn parse(s: &str) -> Result<Self> {
        let (algo_str, hex_str) = s
            .split_once(':')
            .ok_or_else(|| LoomError::invalid("digest missing 'algo:' prefix"))?;
        let algo = Algo::from_name(algo_str)?;
        let raw =
            hex::decode(hex_str).map_err(|e| LoomError::invalid(format!("bad digest hex: {e}")))?;
        let bytes: [u8; DIGEST_LEN] = raw
            .as_slice()
            .try_into()
            .map_err(|_| LoomError::invalid(format!("digest must be {DIGEST_LEN} bytes")))?;
        Ok(Self { algo, bytes })
    }
}

pub struct ContentHasher {
    inner: HasherInner,
}

enum HasherInner {
    Blake3(Box<blake3::Hasher>),
    Sha256(sha2::Sha256),
}

impl ContentHasher {
    pub fn new(algo: Algo) -> Self {
        use sha2::Digest as _;
        let inner = match algo {
            Algo::Blake3 => HasherInner::Blake3(Box::new(blake3::Hasher::new())),
            Algo::Sha256 => HasherInner::Sha256(sha2::Sha256::new()),
        };
        Self { inner }
    }

    pub fn update(&mut self, data: &[u8]) {
        match &mut self.inner {
            HasherInner::Blake3(h) => {
                h.update(data);
            }
            HasherInner::Sha256(h) => {
                use sha2::Digest as _;
                h.update(data);
            }
        }
    }

    pub fn finish(self) -> Digest {
        match self.inner {
            HasherInner::Blake3(h) => Digest {
                algo: Algo::Blake3,
                bytes: *h.finalize().as_bytes(),
            },
            HasherInner::Sha256(h) => {
                use sha2::Digest as _;
                Digest {
                    algo: Algo::Sha256,
                    bytes: h.finalize().into(),
                }
            }
        }
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algo.as_str(), self.to_hex())
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Digest").field(&self.to_string()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn algo_name_round_trips() {
        for algo in [Algo::Blake3, Algo::Sha256] {
            assert_eq!(Algo::from_name(algo.as_str()).unwrap(), algo);
        }
        assert_eq!(Algo::Blake3.as_str(), "blake3");
        assert_eq!(Algo::Sha256.as_str(), "sha256");
        assert!(Algo::from_name("md5").is_err());
    }

    #[test]
    fn blake3_empty_is_known_vector() {
        assert_eq!(
            Digest::blake3(b"").to_hex(),
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
    }

    #[test]
    fn sha256_empty_is_known_vector() {
        assert_eq!(
            Digest::hash(Algo::Sha256, b"").to_hex(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(Digest::hash(Algo::Sha256, b"").algo(), Algo::Sha256);
    }

    #[test]
    fn display_parse_roundtrip() {
        for algo in [Algo::Blake3, Algo::Sha256] {
            let digest = Digest::hash(algo, b"hello loom");
            assert_eq!(Digest::parse(&digest.to_string()).unwrap(), digest);
        }
    }

    #[test]
    fn content_hasher_matches_whole_hash_for_both_profiles() {
        let data: Vec<u8> = (0..50_000u32)
            .map(|i| (i.wrapping_mul(2_246_822_519)) as u8)
            .collect();
        for algo in [Algo::Blake3, Algo::Sha256] {
            let mut h = ContentHasher::new(algo);
            for span in data.chunks(7) {
                h.update(span);
            }
            assert_eq!(h.finish(), Digest::hash(algo, &data));
        }
    }

    #[test]
    fn parse_rejects_bad_input() {
        assert_eq!(
            Digest::parse("deadbeef").unwrap_err().code,
            Code::InvalidArgument
        );
        assert_eq!(
            Digest::parse("sha9:ab").unwrap_err().code,
            Code::Unsupported
        );
        assert_eq!(
            Digest::parse("blake3:zz").unwrap_err().code,
            Code::InvalidArgument
        );
    }
}
