//! Loom Canonical CBOR v1 - the deterministic, content-addressed codec for Uldren Loom.
//!
//! One codec serves both the canonical object form (the bytes hashed under the store identity profile)
//! and ABI result payloads. It is a strict profile of CBOR (RFC 8949): standard wire format, single
//! canonical encoding per value, strict decode. The wire is CBOR; the rules below are the profile.
//!
//! Profile rules (encode emits exactly one form; decode rejects every other):
//! - Definite lengths only; indefinite-length items are rejected.
//! - Shortest-form integer/length arguments; non-minimal encodings are rejected.
//! - Map keys in ascending canonical-encoded-key order, no duplicates.
//! - Floats are 64-bit only (no f16/f32). NaN and infinities are rejected; `-0.0` encodes as `+0.0`,
//!   and a `-0.0` bit pattern is rejected on decode as an alternate of `+0.0`.
//! - No CBOR tags. The only simple values are `false`, `true`, `null`.
//! - A single top-level item; trailing bytes are rejected.
//! - Bounded nesting depth; deeper input is rejected rather than overflowing the stack.
//!
//! `ciborium` was evaluated for decode and rejected: it normalizes non-canonical input (indefinite
//! lengths, non-minimal ints, duplicate/unsorted keys, trailing bytes all decode to `Ok`), so it
//! cannot enforce this profile. Owning the codec also keeps the bytes that define content addresses
//! free of any third-party encoder whose output could drift across versions.
//!
//! Loom objects use a positional framing: a top-level array `[epoch, type, ...fields]`. See
//! [`encode_object`] / [`decode_object`].

/// The Loom Canonical CBOR schema epoch. Bumped only when the framing or value model changes
/// incompatibly; [`decode_object`] rejects any other epoch.
pub const EPOCH: u64 = 1;

/// Maximum nesting depth accepted by [`decode`]. Deeper input is rejected (it would otherwise recurse
/// the decoder stack on attacker-controlled data).
pub const MAX_DEPTH: usize = 128;

/// The Loom Canonical CBOR value model: the CBOR data items the profile admits. `Uint`/`Nint` mirror
/// CBOR major types 0/1 (a negative integer with argument `n` is the value `-1 - n`), so decode round
/// trips the wire form exactly rather than collapsing signedness.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Uint(u64),
    Nint(u64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Float(f64),
    Bool(bool),
    Null,
}

impl Value {
    /// A signed integer as the matching canonical CBOR major type (0 for `>= 0`, 1 for negatives).
    pub fn int(v: i64) -> Value {
        if v >= 0 {
            Value::Uint(v as u64)
        } else {
            Value::Nint((-1 - v) as u64)
        }
    }
}

/// A strict-decode or encode failure. Every variant names a specific non-canonical condition the
/// profile forbids.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CodecError {
    #[error("unexpected end of input")]
    UnexpectedEof,
    #[error("trailing bytes after the top-level item")]
    TrailingBytes,
    #[error("indefinite-length item")]
    IndefiniteLength,
    #[error("non-minimal integer/length encoding")]
    NonMinimalInt,
    #[error("reserved additional-info value (28-30)")]
    ReservedAdditionalInfo,
    #[error("invalid UTF-8 in a text string")]
    InvalidUtf8,
    #[error("map keys not in ascending canonical order")]
    UnsortedMapKeys,
    #[error("duplicate map key")]
    DuplicateMapKey,
    #[error("CBOR tags are not part of the profile")]
    Tag,
    #[error("non-canonical float (NaN, infinity, -0.0, or non-64-bit width)")]
    NonCanonicalFloat,
    #[error("unsupported simple value")]
    UnsupportedSimpleValue,
    #[error("nesting deeper than MAX_DEPTH")]
    DepthExceeded,
    #[error("not a Loom object array")]
    NotAnObject,
    #[error("wrong schema epoch")]
    WrongEpoch,
}

// ---- encode -------------------------------------------------------------------------------------

/// Encode a value to its single canonical byte form. Fails only on a non-finite float.
pub fn encode(value: &Value) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::new();
    encode_into(&mut out, value)?;
    Ok(out)
}

/// Encode a Loom object: the canonical array `[epoch, type, ...fields]`.
pub fn encode_object(type_code: u16, fields: &[Value]) -> Result<Vec<u8>, CodecError> {
    let mut items = Vec::with_capacity(2 + fields.len());
    items.push(Value::Uint(EPOCH));
    items.push(Value::Uint(type_code as u64));
    items.extend(fields.iter().cloned());
    encode(&Value::Array(items))
}

fn put_head(out: &mut Vec<u8>, major: u8, arg: u64) {
    let m = major << 5;
    if arg <= 23 {
        out.push(m | arg as u8);
    } else if arg <= u8::MAX as u64 {
        out.push(m | 24);
        out.push(arg as u8);
    } else if arg <= u16::MAX as u64 {
        out.push(m | 25);
        out.extend_from_slice(&(arg as u16).to_be_bytes());
    } else if arg <= u32::MAX as u64 {
        out.push(m | 26);
        out.extend_from_slice(&(arg as u32).to_be_bytes());
    } else {
        out.push(m | 27);
        out.extend_from_slice(&arg.to_be_bytes());
    }
}

fn encode_into(out: &mut Vec<u8>, value: &Value) -> Result<(), CodecError> {
    match value {
        Value::Uint(n) => put_head(out, 0, *n),
        Value::Nint(n) => put_head(out, 1, *n),
        Value::Bytes(b) => {
            put_head(out, 2, b.len() as u64);
            out.extend_from_slice(b);
        }
        Value::Text(s) => {
            put_head(out, 3, s.len() as u64);
            out.extend_from_slice(s.as_bytes());
        }
        Value::Array(items) => {
            put_head(out, 4, items.len() as u64);
            for item in items {
                encode_into(out, item)?;
            }
        }
        Value::Map(pairs) => {
            // Canonicalize: encode each pair, order by canonical key bytes, reject duplicate keys.
            let mut encoded: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(pairs.len());
            for (k, v) in pairs {
                encoded.push((encode(k)?, encode(v)?));
            }
            encoded.sort_by(|a, b| a.0.cmp(&b.0));
            if encoded.windows(2).any(|w| w[0].0 == w[1].0) {
                return Err(CodecError::DuplicateMapKey);
            }
            put_head(out, 5, encoded.len() as u64);
            for (k, v) in encoded {
                out.extend_from_slice(&k);
                out.extend_from_slice(&v);
            }
        }
        Value::Float(f) => {
            if !f.is_finite() {
                return Err(CodecError::NonCanonicalFloat);
            }
            let normalized = if *f == 0.0 { 0.0 } else { *f }; // -0.0 -> +0.0
            out.push((7 << 5) | 27);
            out.extend_from_slice(&normalized.to_be_bytes());
        }
        Value::Bool(b) => out.push(if *b { 0xf5 } else { 0xf4 }),
        Value::Null => out.push(0xf6),
    }
    Ok(())
}

// ---- decode (strict) ----------------------------------------------------------------------------

/// Strictly decode a single canonical value, rejecting any non-canonical form (see the profile rules
/// in the crate docs) and any trailing bytes.
pub fn decode(bytes: &[u8]) -> Result<Value, CodecError> {
    let mut r = Reader { buf: bytes, pos: 0 };
    let value = r.value(0)?;
    if r.pos != bytes.len() {
        return Err(CodecError::TrailingBytes);
    }
    Ok(value)
}

/// Decode a Loom object array, checking the schema epoch. Returns `(type_code, fields)`.
pub fn decode_object(bytes: &[u8]) -> Result<(u16, Vec<Value>), CodecError> {
    let Value::Array(mut items) = decode(bytes)? else {
        return Err(CodecError::NotAnObject);
    };
    if items.len() < 2 {
        return Err(CodecError::NotAnObject);
    }
    let Value::Uint(epoch) = items[0] else {
        return Err(CodecError::NotAnObject);
    };
    if epoch != EPOCH {
        return Err(CodecError::WrongEpoch);
    }
    let type_code = match items[1] {
        Value::Uint(t) if t <= u16::MAX as u64 => t as u16,
        _ => return Err(CodecError::NotAnObject),
    };
    let fields = items.split_off(2);
    Ok((type_code, fields))
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> Result<u8, CodecError> {
        let b = *self.buf.get(self.pos).ok_or(CodecError::UnexpectedEof)?;
        self.pos += 1;
        Ok(b)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], CodecError> {
        let end = self.pos.checked_add(n).ok_or(CodecError::UnexpectedEof)?;
        let slice = self
            .buf
            .get(self.pos..end)
            .ok_or(CodecError::UnexpectedEof)?;
        self.pos = end;
        Ok(slice)
    }

    /// Read a major type's argument with shortest-form enforcement.
    fn arg(&mut self, ai: u8) -> Result<u64, CodecError> {
        match ai {
            0..=23 => Ok(ai as u64),
            24 => {
                let v = self.byte()? as u64;
                if v < 24 {
                    return Err(CodecError::NonMinimalInt);
                }
                Ok(v)
            }
            25 => {
                let b = self.take(2)?;
                let v = u16::from_be_bytes([b[0], b[1]]) as u64;
                if v <= u8::MAX as u64 {
                    return Err(CodecError::NonMinimalInt);
                }
                Ok(v)
            }
            26 => {
                let b = self.take(4)?;
                let v = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as u64;
                if v <= u16::MAX as u64 {
                    return Err(CodecError::NonMinimalInt);
                }
                Ok(v)
            }
            27 => {
                let b = self.take(8)?;
                let v = u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
                if v <= u32::MAX as u64 {
                    return Err(CodecError::NonMinimalInt);
                }
                Ok(v)
            }
            28..=30 => Err(CodecError::ReservedAdditionalInfo),
            _ => Err(CodecError::IndefiniteLength), // ai == 31
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, CodecError> {
        if depth > MAX_DEPTH {
            return Err(CodecError::DepthExceeded);
        }
        let ib = self.byte()?;
        let major = ib >> 5;
        let ai = ib & 0x1f;
        match major {
            0 => Ok(Value::Uint(self.arg(ai)?)),
            1 => Ok(Value::Nint(self.arg(ai)?)),
            2 => {
                let n = self.arg(ai)? as usize;
                Ok(Value::Bytes(self.take(n)?.to_vec()))
            }
            3 => {
                let n = self.arg(ai)? as usize;
                let s = self.take(n)?;
                let text = core::str::from_utf8(s).map_err(|_| CodecError::InvalidUtf8)?;
                Ok(Value::Text(text.to_string()))
            }
            4 => {
                let n = self.arg(ai)?;
                let mut items = Vec::new();
                for _ in 0..n {
                    items.push(self.value(depth + 1)?);
                }
                Ok(Value::Array(items))
            }
            5 => {
                let n = self.arg(ai)?;
                let mut pairs = Vec::new();
                let mut prev_key: Option<Vec<u8>> = None;
                for _ in 0..n {
                    let key_start = self.pos;
                    let key = self.value(depth + 1)?;
                    let key_bytes = self.buf[key_start..self.pos].to_vec();
                    if let Some(prev) = &prev_key {
                        match key_bytes.cmp(prev) {
                            core::cmp::Ordering::Greater => {}
                            core::cmp::Ordering::Equal => return Err(CodecError::DuplicateMapKey),
                            core::cmp::Ordering::Less => return Err(CodecError::UnsortedMapKeys),
                        }
                    }
                    let val = self.value(depth + 1)?;
                    pairs.push((key, val));
                    prev_key = Some(key_bytes);
                }
                Ok(Value::Map(pairs))
            }
            6 => Err(CodecError::Tag),
            _ => self.simple_or_float(ai), // major == 7
        }
    }

    fn simple_or_float(&mut self, ai: u8) -> Result<Value, CodecError> {
        match ai {
            20 => Ok(Value::Bool(false)),
            21 => Ok(Value::Bool(true)),
            22 => Ok(Value::Null),
            25 | 26 => Err(CodecError::NonCanonicalFloat), // half/single float widths are not allowed
            27 => {
                let b = self.take(8)?;
                let f = f64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
                if !f.is_finite() || (f == 0.0 && f.is_sign_negative()) {
                    return Err(CodecError::NonCanonicalFloat);
                }
                Ok(Value::Float(f))
            }
            _ => Err(CodecError::UnsupportedSimpleValue),
        }
    }
}

#[cfg(test)]
mod tests;
