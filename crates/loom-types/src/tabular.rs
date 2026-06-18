//! Shared tabular scalar contracts.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::error::{LoomError, Result};
use loom_codec::Value as CborValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Int,
    Float,
    Text,
    Bool,
    Bytes,
    I8,
    I16,
    I32,
    I128,
    U8,
    U16,
    U32,
    U64,
    U128,
    F32,
    Decimal,
    Date,
    Time,
    Timestamp,
    Interval,
    Uuid,
    Inet,
    Point,
    List,
    Map,
}

impl ColumnType {
    pub fn tag(self) -> u8 {
        match self {
            ColumnType::Int => 1,
            ColumnType::Float => 2,
            ColumnType::Text => 3,
            ColumnType::Bool => 4,
            ColumnType::Bytes => 5,
            ColumnType::I8 => 6,
            ColumnType::I16 => 7,
            ColumnType::I32 => 8,
            ColumnType::I128 => 9,
            ColumnType::U8 => 10,
            ColumnType::U16 => 11,
            ColumnType::U32 => 12,
            ColumnType::U64 => 13,
            ColumnType::U128 => 14,
            ColumnType::F32 => 15,
            ColumnType::Decimal => 16,
            ColumnType::Date => 17,
            ColumnType::Time => 18,
            ColumnType::Timestamp => 19,
            ColumnType::Interval => 20,
            ColumnType::Uuid => 21,
            ColumnType::Inet => 22,
            ColumnType::Point => 23,
            ColumnType::List => 24,
            ColumnType::Map => 25,
        }
    }

    pub fn from_tag(b: u8) -> Result<Self> {
        Ok(match b {
            1 => ColumnType::Int,
            2 => ColumnType::Float,
            3 => ColumnType::Text,
            4 => ColumnType::Bool,
            5 => ColumnType::Bytes,
            6 => ColumnType::I8,
            7 => ColumnType::I16,
            8 => ColumnType::I32,
            9 => ColumnType::I128,
            10 => ColumnType::U8,
            11 => ColumnType::U16,
            12 => ColumnType::U32,
            13 => ColumnType::U64,
            14 => ColumnType::U128,
            15 => ColumnType::F32,
            16 => ColumnType::Decimal,
            17 => ColumnType::Date,
            18 => ColumnType::Time,
            19 => ColumnType::Timestamp,
            20 => ColumnType::Interval,
            21 => ColumnType::Uuid,
            22 => ColumnType::Inet,
            23 => ColumnType::Point,
            24 => ColumnType::List,
            25 => ColumnType::Map,
            other => {
                return Err(LoomError::corrupt(format!(
                    "unknown column type {other:#x}"
                )));
            }
        })
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    I8(i8),
    I16(i16),
    I32(i32),
    I128(i128),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    U128(u128),
    F32(f32),
    Decimal { mantissa: i128, scale: u32 },
    Date(i32),
    Time(u64),
    Timestamp(i64),
    Interval { months: i32, micros: i64 },
    Uuid(u128),
    Inet(std::net::IpAddr),
    Point { x: f64, y: f64 },
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
}

impl Value {
    pub const fn rank(&self) -> u8 {
        match self {
            Value::Null => 0,
            Value::Bool(_) => 1,
            Value::Int(_) => 2,
            Value::Float(_) => 3,
            Value::Text(_) => 4,
            Value::Bytes(_) => 5,
            Value::I8(_) => 6,
            Value::I16(_) => 7,
            Value::I32(_) => 8,
            Value::I128(_) => 9,
            Value::U8(_) => 10,
            Value::U16(_) => 11,
            Value::U32(_) => 12,
            Value::U64(_) => 13,
            Value::U128(_) => 14,
            Value::F32(_) => 15,
            Value::Decimal { .. } => 16,
            Value::Date(_) => 17,
            Value::Time(_) => 18,
            Value::Timestamp(_) => 19,
            Value::Interval { .. } => 20,
            Value::Uuid(_) => 21,
            Value::Inet(_) => 22,
            Value::Point { .. } => 23,
            Value::List(_) => 24,
            Value::Map(_) => 25,
        }
    }

    pub fn matches(&self, ty: ColumnType) -> bool {
        matches!(
            (self, ty),
            (Value::Null, _)
                | (Value::Bool(_), ColumnType::Bool)
                | (Value::Int(_), ColumnType::Int)
                | (Value::Float(_), ColumnType::Float)
                | (Value::Text(_), ColumnType::Text)
                | (Value::Bytes(_), ColumnType::Bytes)
                | (Value::I8(_), ColumnType::I8)
                | (Value::I16(_), ColumnType::I16)
                | (Value::I32(_), ColumnType::I32)
                | (Value::I128(_), ColumnType::I128)
                | (Value::U8(_), ColumnType::U8)
                | (Value::U16(_), ColumnType::U16)
                | (Value::U32(_), ColumnType::U32)
                | (Value::U64(_), ColumnType::U64)
                | (Value::U128(_), ColumnType::U128)
                | (Value::F32(_), ColumnType::F32)
                | (Value::Decimal { .. }, ColumnType::Decimal)
                | (Value::Date(_), ColumnType::Date)
                | (Value::Time(_), ColumnType::Time)
                | (Value::Timestamp(_), ColumnType::Timestamp)
                | (Value::Interval { .. }, ColumnType::Interval)
                | (Value::Uuid(_), ColumnType::Uuid)
                | (Value::Inet(_), ColumnType::Inet)
                | (Value::Point { .. }, ColumnType::Point)
                | (Value::List(_), ColumnType::List)
                | (Value::Map(_), ColumnType::Map)
        )
    }
}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        use Value::*;
        match (self, other) {
            (Bool(a), Bool(b)) => a.cmp(b),
            (Int(a), Int(b)) => a.cmp(b),
            (Float(a), Float(b)) => a.total_cmp(b),
            (Text(a), Text(b)) => a.cmp(b),
            (Bytes(a), Bytes(b)) => a.cmp(b),
            (I8(a), I8(b)) => a.cmp(b),
            (I16(a), I16(b)) => a.cmp(b),
            (I32(a), I32(b)) => a.cmp(b),
            (I128(a), I128(b)) => a.cmp(b),
            (U8(a), U8(b)) => a.cmp(b),
            (U16(a), U16(b)) => a.cmp(b),
            (U32(a), U32(b)) => a.cmp(b),
            (U64(a), U64(b)) => a.cmp(b),
            (U128(a), U128(b)) => a.cmp(b),
            (F32(a), F32(b)) => a.total_cmp(b),
            (Date(a), Date(b)) => a.cmp(b),
            (Time(a), Time(b)) => a.cmp(b),
            (Timestamp(a), Timestamp(b)) => a.cmp(b),
            (Uuid(a), Uuid(b)) => a.cmp(b),
            (Decimal { .. }, Decimal { .. })
            | (Interval { .. }, Interval { .. })
            | (Inet(_), Inet(_))
            | (Point { .. }, Point { .. })
            | (List(_), List(_))
            | (Map(_), Map(_)) => key_bytes(self).cmp(&key_bytes(other)),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Value {}

pub type Row = Vec<Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

pub fn encode_pk_values(pk: &[Value]) -> Vec<u8> {
    let mut b = Vec::new();
    for v in pk {
        encode_key_value(&mut b, v);
    }
    b
}

pub fn encode_key_value(out: &mut Vec<u8>, v: &Value) {
    out.push(v.rank());
    match v {
        Value::Null => {}
        Value::Bool(b) => out.push(u8::from(*b)),
        Value::Int(i) => out.extend_from_slice(&((*i as u64) ^ (1 << 63)).to_be_bytes()),
        Value::Float(f) => {
            let bits = f.to_bits();
            let key = if bits & (1 << 63) != 0 {
                !bits
            } else {
                bits | (1 << 63)
            };
            out.extend_from_slice(&key.to_be_bytes());
        }
        Value::Text(s) => encode_orderpreserving_bytes(out, s.as_bytes()),
        Value::Bytes(bytes) => encode_orderpreserving_bytes(out, bytes),
        Value::I8(x) => out.push((*x as u8) ^ 0x80),
        Value::I16(x) => out.extend_from_slice(&((*x as u16) ^ (1 << 15)).to_be_bytes()),
        Value::I32(x) => out.extend_from_slice(&((*x as u32) ^ (1 << 31)).to_be_bytes()),
        Value::I128(x) => out.extend_from_slice(&((*x as u128) ^ (1 << 127)).to_be_bytes()),
        Value::U8(x) => out.push(*x),
        Value::U16(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::U32(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::U64(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::U128(x) => out.extend_from_slice(&x.to_be_bytes()),
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
        Value::Date(d) => out.extend_from_slice(&((*d as u32) ^ (1 << 31)).to_be_bytes()),
        Value::Time(t) => out.extend_from_slice(&t.to_be_bytes()),
        Value::Timestamp(t) => out.extend_from_slice(&((*t as u64) ^ (1 << 63)).to_be_bytes()),
        Value::Interval { months, micros } => {
            out.extend_from_slice(&((*months as u32) ^ (1 << 31)).to_be_bytes());
            out.extend_from_slice(&((*micros as u64) ^ (1 << 63)).to_be_bytes());
        }
        Value::Uuid(u) => out.extend_from_slice(&u.to_be_bytes()),
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

pub fn key_bytes(v: &Value) -> Vec<u8> {
    let mut b = Vec::new();
    encode_key_value(&mut b, v);
    b
}

pub fn encode_cell(v: &Value) -> Vec<u8> {
    loom_codec::encode(&cell_value(v)).expect("tabular cell values are canonical")
}

pub fn encode_cells(values: &[Value]) -> Vec<u8> {
    loom_codec::encode(&CborValue::Array(values.iter().map(cell_value).collect()))
        .expect("tabular cell arrays are canonical")
}

pub fn cell_value(v: &Value) -> CborValue {
    use CborValue::{Array, Bytes, Map, Text, Uint};
    match v {
        Value::Null => Array(vec![Uint(0)]),
        Value::Bool(b) => Array(vec![Uint(1), CborValue::Bool(*b)]),
        Value::Int(i) => Array(vec![Uint(2), CborValue::int(*i)]),
        Value::Float(f) => Array(vec![Uint(3), Uint(f.to_bits())]),
        Value::Text(s) => Array(vec![Uint(4), Text(s.clone())]),
        Value::Bytes(b) => Array(vec![Uint(5), Bytes(b.clone())]),
        Value::I8(x) => Array(vec![Uint(6), CborValue::int(i64::from(*x))]),
        Value::I16(x) => Array(vec![Uint(7), CborValue::int(i64::from(*x))]),
        Value::I32(x) => Array(vec![Uint(8), CborValue::int(i64::from(*x))]),
        Value::I128(x) => Array(vec![Uint(9), Bytes(x.to_le_bytes().to_vec())]),
        Value::U8(x) => Array(vec![Uint(10), Uint(u64::from(*x))]),
        Value::U16(x) => Array(vec![Uint(11), Uint(u64::from(*x))]),
        Value::U32(x) => Array(vec![Uint(12), Uint(u64::from(*x))]),
        Value::U64(x) => Array(vec![Uint(13), Uint(*x)]),
        Value::U128(x) => Array(vec![Uint(14), Bytes(x.to_le_bytes().to_vec())]),
        Value::F32(x) => Array(vec![Uint(15), Uint(u64::from(x.to_bits()))]),
        Value::Decimal { mantissa, scale } => Array(vec![
            Uint(16),
            Bytes(mantissa.to_le_bytes().to_vec()),
            Uint(u64::from(*scale)),
        ]),
        Value::Date(d) => Array(vec![Uint(17), CborValue::int(i64::from(*d))]),
        Value::Time(t) => Array(vec![Uint(18), Uint(*t)]),
        Value::Timestamp(t) => Array(vec![Uint(19), CborValue::int(*t)]),
        Value::Interval { months, micros } => Array(vec![
            Uint(20),
            CborValue::int(i64::from(*months)),
            CborValue::int(*micros),
        ]),
        Value::Uuid(u) => Array(vec![Uint(21), Bytes(u.to_le_bytes().to_vec())]),
        Value::Inet(ip) => match ip {
            std::net::IpAddr::V4(a) => Array(vec![Uint(22), Uint(4), Bytes(a.octets().to_vec())]),
            std::net::IpAddr::V6(a) => Array(vec![Uint(22), Uint(6), Bytes(a.octets().to_vec())]),
        },
        Value::Point { x, y } => Array(vec![Uint(23), Uint(x.to_bits()), Uint(y.to_bits())]),
        Value::List(items) => Array(vec![
            Uint(24),
            Array(items.iter().map(cell_value).collect()),
        ]),
        Value::Map(entries) => Array(vec![
            Uint(25),
            Map(entries
                .iter()
                .map(|(k, val)| (Text(k.clone()), cell_value(val)))
                .collect()),
        ]),
    }
}

pub fn cell_from(item: CborValue) -> Result<Value> {
    let mut f = CellFields::new(as_array(item)?);
    let value = match f.uint()? {
        0 => Value::Null,
        1 => Value::Bool(f.bool()?),
        2 => Value::Int(f.int()?),
        3 => Value::Float(f64::from_bits(f.uint()?)),
        4 => Value::Text(f.text()?),
        5 => Value::Bytes(f.bytes()?),
        6 => Value::I8(as_i8(f.int()?)?),
        7 => Value::I16(as_i16(f.int()?)?),
        8 => Value::I32(as_i32(f.int()?)?),
        9 => Value::I128(i128::from_le_bytes(fixed::<16>(f.bytes()?)?)),
        10 => Value::U8(as_u8(f.uint()?)?),
        11 => Value::U16(as_u16(f.uint()?)?),
        12 => Value::U32(as_u32(f.uint()?)?),
        13 => Value::U64(f.uint()?),
        14 => Value::U128(u128::from_le_bytes(fixed::<16>(f.bytes()?)?)),
        15 => Value::F32(f32::from_bits(as_u32(f.uint()?)?)),
        16 => Value::Decimal {
            mantissa: i128::from_le_bytes(fixed::<16>(f.bytes()?)?),
            scale: as_u32(f.uint()?)?,
        },
        17 => Value::Date(as_i32(f.int()?)?),
        18 => Value::Time(f.uint()?),
        19 => Value::Timestamp(f.int()?),
        20 => Value::Interval {
            months: as_i32(f.int()?)?,
            micros: f.int()?,
        },
        21 => Value::Uuid(u128::from_le_bytes(fixed::<16>(f.bytes()?)?)),
        22 => {
            let family = f.uint()?;
            let octets = f.bytes()?;
            match family {
                4 => Value::Inet(std::net::IpAddr::V4(std::net::Ipv4Addr::from(fixed::<4>(
                    octets,
                )?))),
                6 => Value::Inet(std::net::IpAddr::V6(std::net::Ipv6Addr::from(fixed::<16>(
                    octets,
                )?))),
                other => return Err(LoomError::corrupt(format!("bad inet family {other}"))),
            }
        }
        23 => Value::Point {
            x: f64::from_bits(f.uint()?),
            y: f64::from_bits(f.uint()?),
        },
        24 => Value::List(
            f.array()?
                .into_iter()
                .map(cell_from)
                .collect::<Result<Vec<_>>>()?,
        ),
        25 => {
            let mut m = BTreeMap::new();
            for (k, val) in f.map()? {
                m.insert(as_text(k)?, cell_from(val)?);
            }
            Value::Map(m)
        }
        other => return Err(LoomError::corrupt(format!("unknown value tag {other}"))),
    };
    f.end()?;
    Ok(value)
}

fn as_array(value: CborValue) -> Result<Vec<CborValue>> {
    match value {
        CborValue::Array(values) => Ok(values),
        _ => Err(LoomError::corrupt("expected an array")),
    }
}

fn as_text(value: CborValue) -> Result<String> {
    match value {
        CborValue::Text(value) => Ok(value),
        _ => Err(LoomError::corrupt("expected a text string")),
    }
}

fn as_uint(value: CborValue) -> Result<u64> {
    match value {
        CborValue::Uint(value) => Ok(value),
        _ => Err(LoomError::corrupt("expected a uint")),
    }
}

fn as_int(value: CborValue) -> Result<i64> {
    match value {
        CborValue::Uint(n) => {
            i64::try_from(n).map_err(|_| LoomError::corrupt("integer out of i64 range"))
        }
        CborValue::Nint(n) => {
            let n = i64::try_from(n).map_err(|_| LoomError::corrupt("integer out of i64 range"))?;
            Ok(-1 - n)
        }
        _ => Err(LoomError::corrupt("expected an integer")),
    }
}

fn as_bool(value: CborValue) -> Result<bool> {
    match value {
        CborValue::Bool(value) => Ok(value),
        _ => Err(LoomError::corrupt("expected a bool")),
    }
}

fn as_bytes(value: CborValue) -> Result<Vec<u8>> {
    match value {
        CborValue::Bytes(value) => Ok(value),
        _ => Err(LoomError::corrupt("expected a byte string")),
    }
}

fn as_map(value: CborValue) -> Result<Vec<(CborValue, CborValue)>> {
    match value {
        CborValue::Map(value) => Ok(value),
        _ => Err(LoomError::corrupt("expected a map")),
    }
}

fn as_u8(n: u64) -> Result<u8> {
    u8::try_from(n).map_err(|_| LoomError::corrupt("value out of u8 range"))
}

fn as_u16(n: u64) -> Result<u16> {
    u16::try_from(n).map_err(|_| LoomError::corrupt("value out of u16 range"))
}

fn as_u32(n: u64) -> Result<u32> {
    u32::try_from(n).map_err(|_| LoomError::corrupt("value out of u32 range"))
}

fn as_i8(n: i64) -> Result<i8> {
    i8::try_from(n).map_err(|_| LoomError::corrupt("value out of i8 range"))
}

fn as_i16(n: i64) -> Result<i16> {
    i16::try_from(n).map_err(|_| LoomError::corrupt("value out of i16 range"))
}

fn as_i32(n: i64) -> Result<i32> {
    i32::try_from(n).map_err(|_| LoomError::corrupt("value out of i32 range"))
}

fn fixed<const N: usize>(b: Vec<u8>) -> Result<[u8; N]> {
    b.as_slice()
        .try_into()
        .map_err(|_| LoomError::corrupt("fixed-width byte field has the wrong length"))
}

struct CellFields {
    items: std::vec::IntoIter<CborValue>,
}

impl CellFields {
    fn new(items: Vec<CborValue>) -> Self {
        Self {
            items: items.into_iter(),
        }
    }

    fn next_field(&mut self) -> Result<CborValue> {
        self.items
            .next()
            .ok_or_else(|| LoomError::corrupt("missing field"))
    }

    fn uint(&mut self) -> Result<u64> {
        as_uint(self.next_field()?)
    }

    fn int(&mut self) -> Result<i64> {
        as_int(self.next_field()?)
    }

    fn bool(&mut self) -> Result<bool> {
        as_bool(self.next_field()?)
    }

    fn bytes(&mut self) -> Result<Vec<u8>> {
        as_bytes(self.next_field()?)
    }

    fn text(&mut self) -> Result<String> {
        as_text(self.next_field()?)
    }

    fn array(&mut self) -> Result<Vec<CborValue>> {
        as_array(self.next_field()?)
    }

    fn map(&mut self) -> Result<Vec<(CborValue, CborValue)>> {
        as_map(self.next_field()?)
    }

    fn end(mut self) -> Result<()> {
        if self.items.next().is_some() {
            Err(LoomError::corrupt("unexpected extra fields"))
        } else {
            Ok(())
        }
    }
}

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

fn encode_decimal_key(out: &mut Vec<u8>, mantissa: i128, scale: u32) {
    if mantissa == 0 {
        out.push(0x80);
        return;
    }
    let negative = mantissa < 0;
    let mut digits = mantissa.unsigned_abs().to_string().into_bytes();
    let e = digits.len() as i64 - scale as i64 - 1;
    while digits.len() > 1 && *digits.last().expect("non-empty") == b'0' {
        digits.pop();
    }
    let exp = ((e as u64) ^ (1 << 63)).to_be_bytes();
    if !negative {
        out.push(0x81);
        out.extend_from_slice(&exp);
        for &d in &digits {
            out.push(d - b'0' + 1);
        }
        out.push(0x00);
    } else {
        out.push(0x7F);
        for b in exp {
            out.push(!b);
        }
        for &d in &digits {
            out.push(10 - (d - b'0'));
        }
        out.push(0xFF);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_key_normalizes_trailing_zeroes() {
        assert_eq!(
            key_bytes(&Value::Decimal {
                mantissa: 15,
                scale: 1
            }),
            key_bytes(&Value::Decimal {
                mantissa: 150,
                scale: 2
            })
        );
    }

    #[test]
    fn scalar_matches_column_type() {
        assert!(Value::Text("loom".into()).matches(ColumnType::Text));
        assert!(!Value::Text("loom".into()).matches(ColumnType::Int));
        assert!(Value::Null.matches(ColumnType::Map));
    }
}
