//! Licensed under BUSL-1.1 (see the workspace `LICENSE`). (c) Uldren Technologies LLC.

use super::*;

fn put_uvarint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            out.push(byte | 0x80);
        } else {
            out.push(byte);
            break;
        }
    }
}

/// Narrow a decoded CBOR uint to a `usize` (a column count or index), rejecting overflow.
pub(crate) fn as_usize(n: u64) -> Result<usize> {
    usize::try_from(n).map_err(|_| LoomError::corrupt("value out of usize range"))
}
/// Narrow a decoded CBOR uint to a `u8`, rejecting overflow.
pub(crate) fn as_u8(n: u64) -> Result<u8> {
    u8::try_from(n).map_err(|_| LoomError::corrupt("value out of u8 range"))
}
/// Encode a primary-key value sequence as an order-preserving prolly key: comparing two encoded keys
/// as raw bytes yields the same order as comparing the primary keys as [`Value`]s. Each column is a
/// `rank` byte then an order-preserving, self-delimiting body, so multi-column keys concatenate
/// without ambiguity.
pub(crate) fn encode_pk_values(pk: &[Value]) -> Vec<u8> {
    let mut b = Vec::new();
    for v in pk {
        encode_key_value(&mut b, v);
    }
    b
}

/// Append one value to an order-preserving key. The leading byte is the type **rank** (matching
/// [`Value::rank`], so cross-type order is correct); the body is order-preserving within the type.
pub(crate) fn encode_key_value(out: &mut Vec<u8>, v: &Value) {
    out.push(v.rank());
    match v {
        Value::Null => {}
        Value::Bool(b) => out.push(u8::from(*b)),
        // Flip the sign bit so two's-complement signed order becomes unsigned big-endian byte order.
        Value::Int(i) => out.extend_from_slice(&((*i as u64) ^ (1 << 63)).to_be_bytes()),
        // Total-order transform (matches `f64::total_cmp`): negatives flip all bits, others flip sign.
        Value::Float(f) => {
            let bits = f.to_bits();
            let key = if bits & (1 << 63) != 0 {
                !bits
            } else {
                bits | (1 << 63)
            };
            out.extend_from_slice(&key.to_be_bytes());
        }
        // Byte-stuffed, NUL-terminated: 0x00 becomes 0x00 0xFF (sorts after the 0x00 0x00 terminator), so
        // the encoding is order-preserving and self-delimiting even when the text/bytes contain NUL.
        Value::Text(s) => encode_orderpreserving_bytes(out, s.as_bytes()),
        Value::Bytes(bytes) => encode_orderpreserving_bytes(out, bytes),
        // Signed integers: flip the sign bit, then big-endian (signed order -> unsigned BE byte order).
        Value::I8(x) => out.push((*x as u8) ^ 0x80),
        Value::I16(x) => out.extend_from_slice(&((*x as u16) ^ (1 << 15)).to_be_bytes()),
        Value::I32(x) => out.extend_from_slice(&((*x as u32) ^ (1 << 31)).to_be_bytes()),
        Value::I128(x) => out.extend_from_slice(&((*x as u128) ^ (1 << 127)).to_be_bytes()),
        // Unsigned integers: big-endian is already order-preserving.
        Value::U8(x) => out.push(*x),
        Value::U16(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::U32(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::U64(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::U128(x) => out.extend_from_slice(&x.to_be_bytes()),
        // f32 total-order transform, big-endian.
        Value::F32(f) => {
            let bits = f.to_bits();
            let key = if bits & (1 << 31) != 0 {
                !bits
            } else {
                bits | (1 << 31)
            };
            out.extend_from_slice(&key.to_be_bytes());
        }
        Value::Decimal { mantissa, scale } => encode_decimal_key(out, *mantissa, *scale),
        // Temporals carry as integers: same sign-flip/BE rule as the matching integer width.
        Value::Date(d) => out.extend_from_slice(&((*d as u32) ^ (1 << 31)).to_be_bytes()),
        Value::Time(t) => out.extend_from_slice(&t.to_be_bytes()),
        Value::Timestamp(t) => out.extend_from_slice(&((*t as u64) ^ (1 << 63)).to_be_bytes()),
        Value::Interval { months, micros } => {
            out.extend_from_slice(&((*months as u32) ^ (1 << 31)).to_be_bytes());
            out.extend_from_slice(&((*micros as u64) ^ (1 << 63)).to_be_bytes());
        }
        Value::Uuid(u) => out.extend_from_slice(&u.to_be_bytes()),
        // IP: family byte (v4=0 < v6=1, matching `IpAddr` order) then the octets big-endian.
        Value::Inet(ip) => match ip {
            std::net::IpAddr::V4(a) => {
                out.push(0);
                out.extend_from_slice(&a.octets());
            }
            std::net::IpAddr::V6(a) => {
                out.push(1);
                out.extend_from_slice(&a.octets());
            }
        },
        Value::Point { x, y } => {
            for f in [x, y] {
                let bits = f.to_bits();
                let key = if bits & (1 << 63) != 0 {
                    !bits
                } else {
                    bits | (1 << 63)
                };
                out.extend_from_slice(&key.to_be_bytes());
            }
        }
        // Composites: length-prefixed then each element's key encoding. Self-delimiting and
        // equality-correct; the order is by encoding (a deterministic total order), not element-wise
        // semantic order, which is undefined for heterogeneous composites.
        Value::List(items) => {
            put_uvarint(out, items.len() as u64);
            for item in items {
                encode_key_value(out, item);
            }
        }
        Value::Map(entries) => {
            put_uvarint(out, entries.len() as u64);
            for (k, val) in entries {
                encode_orderpreserving_bytes(out, k.as_bytes());
                encode_key_value(out, val);
            }
        }
    }
}

/// Append an order-preserving, self-delimiting key encoding of the decimal `mantissa * 10^(-scale)`.
/// Scheme: a sign marker (`0x7F` negative `< 0x80` zero `< 0x81` positive), then for non-zero values a
/// big-endian biased exponent and the significant digits (trailing zeros stripped so `1.5 == 1.50`),
/// terminated so a shorter coefficient sorts before a longer one. For negatives the exponent bytes and
/// digit bytes are inverted and the terminator raised, so larger magnitude sorts smaller.
fn encode_decimal_key(out: &mut Vec<u8>, mantissa: i128, scale: u32) {
    if mantissa == 0 {
        out.push(0x80);
        return;
    }
    let negative = mantissa < 0;
    let mut digits = mantissa.unsigned_abs().to_string().into_bytes(); // ASCII, MSD first, no leading 0s
    // Exponent of the most-significant digit: value = d.ddd * 10^e.
    let e = digits.len() as i64 - scale as i64 - 1;
    while digits.len() > 1 && *digits.last().expect("non-empty") == b'0' {
        digits.pop(); // normalize: drop trailing zeros (does not move the leading digit)
    }
    let exp = ((e as u64) ^ (1 << 63)).to_be_bytes();
    if !negative {
        out.push(0x81);
        out.extend_from_slice(&exp);
        for &d in &digits {
            out.push(d - b'0' + 1); // digits map to 1..=10, all above the 0x00 terminator
        }
        out.push(0x00);
    } else {
        out.push(0x7F);
        for b in exp {
            out.push(!b); // invert: larger magnitude exponent sorts smaller
        }
        for &d in &digits {
            out.push(10 - (d - b'0')); // invert digits: larger digit sorts smaller
        }
        out.push(0xFF); // terminator above the inverted digits, so a longer coefficient sorts smaller
    }
}

fn encode_orderpreserving_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    for &byte in bytes {
        if byte == 0x00 {
            out.push(0x00);
            out.push(0xFF);
        } else {
            out.push(byte);
        }
    }
    out.push(0x00);
    out.push(0x00);
}

/// Encode a full row as a prolly value: the Loom Canonical CBOR array of its cell values.
pub(crate) fn encode_row(row: &Row) -> Vec<u8> {
    cbor::encode(&cbor::Value::Array(row.iter().map(cell_value).collect()))
}

/// Decode a row written by [`encode_row`] under `schema`, rejecting a wrong arity.
pub(crate) fn decode_row(schema: &Schema, bytes: &[u8]) -> Result<Row> {
    let items = cbor::decode_array(bytes)?;
    if items.len() != schema.arity() {
        return Err(LoomError::corrupt("row arity mismatch"));
    }
    items.into_iter().map(cell_from).collect()
}

/// A cursor over an order-preserving binary key encoding ([`encode_key_value`]); the only remaining
/// non-CBOR codec, kept because canonical CBOR cannot preserve Loom's byte-lexicographic key order.
pub(crate) struct Cur<'a> {
    buf: &'a [u8],
    pub(crate) pos: usize,
}
impl<'a> Cur<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    pub(crate) fn u8(&mut self) -> Result<u8> {
        let b = *self
            .buf
            .get(self.pos)
            .ok_or_else(|| LoomError::corrupt("unexpected end of key bytes"))?;
        self.pos += 1;
        Ok(b)
    }
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|e| *e <= self.buf.len())
            .ok_or_else(|| LoomError::corrupt("key bytes truncated"))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    pub(crate) fn uvarint(&mut self) -> Result<u64> {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift >= 64 {
                return Err(LoomError::corrupt("uvarint too long"));
            }
        }
        Ok(v)
    }
}
