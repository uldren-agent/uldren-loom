//! Content addresses.
//!
//! A [`Digest`] is the cryptographic hash of an object's canonical bytes, prefixed by its
//! algorithm. Textual form is `algo:hex` (e.g. `blake3:af1349b9…`). The default and only-required
//! algorithm is BLAKE3-256.

use crate::error::{Code, LoomError, Result};
use std::fmt;

/// Number of bytes in a digest (BLAKE3-256 output).
pub const DIGEST_LEN: usize = 32;

/// Digest algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Algo {
    /// BLAKE3 with 256-bit output. The default.
    Blake3,
}

impl Algo {
    /// The stable textual tag used in `algo:hex` form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Algo::Blake3 => "blake3",
        }
    }
}

/// A content address: an algorithm plus the raw digest bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Digest {
    algo: Algo,
    bytes: [u8; DIGEST_LEN],
}

impl Digest {
    /// The algorithm this digest was produced with.
    pub const fn algo(&self) -> Algo {
        self.algo
    }

    /// The raw digest bytes.
    pub const fn bytes(&self) -> &[u8; DIGEST_LEN] {
        &self.bytes
    }

    /// Compute the BLAKE3-256 digest of `data`.
    ///
    /// The input is an object's canonical (type-tagged) bytes - see [`crate::object::Object::canonical`].
    pub fn blake3(data: &[u8]) -> Self {
        Self {
            algo: Algo::Blake3,
            bytes: *blake3::hash(data).as_bytes(),
        }
    }

    /// Lowercase hex of the digest bytes (no algorithm prefix).
    pub fn to_hex(&self) -> String {
        hex::encode(self.bytes)
    }

    /// Parse the textual `algo:hex` form.
    pub fn parse(s: &str) -> Result<Self> {
        let (algo_str, hex_str) = s
            .split_once(':')
            .ok_or_else(|| LoomError::invalid("digest missing 'algo:' prefix"))?;
        let algo = match algo_str {
            "blake3" => Algo::Blake3,
            other => {
                return Err(LoomError::new(
                    Code::Unsupported,
                    format!("unknown digest algo '{other}'"),
                ));
            }
        };
        let raw =
            hex::decode(hex_str).map_err(|e| LoomError::invalid(format!("bad digest hex: {e}")))?;
        let bytes: [u8; DIGEST_LEN] = raw
            .as_slice()
            .try_into()
            .map_err(|_| LoomError::invalid(format!("digest must be {DIGEST_LEN} bytes")))?;
        Ok(Self { algo, bytes })
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algo.as_str(), self.to_hex())
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blake3_empty_is_known_vector() {
        // Official BLAKE3 test vector for the empty input.
        assert_eq!(
            Digest::blake3(b"").to_hex(),
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
    }

    #[test]
    fn display_parse_roundtrip() {
        let d = Digest::blake3(b"hello loom");
        let s = d.to_string();
        assert!(s.starts_with("blake3:"));
        assert_eq!(Digest::parse(&s).unwrap(), d);
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
