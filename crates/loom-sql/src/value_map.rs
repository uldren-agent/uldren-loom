//! Lossless GlueSQL <-> tabular type bridge (A2).
//!
//! This is the single place GlueSQL's type system (`gluesql_core::data::Value` / `Key`, and the
//! `ast::DataType` schema types) meets loom-core's typed-primitive substrate
//! (`loom_core::tabular::Value` / `ColumnType`). Every GlueSQL scalar round-trips through a tabular
//! value and back. The `rust_decimal` and `chrono` conversions live here so loom-core stays free of
//! those dependencies; loom-core carries decimals as `(mantissa, scale)`, dates/times/timestamps as
//! integers, and so on.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, Timelike, Utc};
use gluesql_core::ast::DataType as GDataType;
use gluesql_core::data::{Interval as GInterval, Key, Point as GPoint, Value as GValue};
use loom_core::error::{LoomError, Result};
use loom_core::tabular::{ColumnType, Value};
use rust_decimal::Decimal;
use std::collections::BTreeMap;

/// The Unix epoch date, the zero point for [`Value::Date`].
fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch is a valid date")
}

fn date_to_days(d: NaiveDate) -> i32 {
    (d - epoch()).num_days() as i32
}
fn days_to_date(n: i32) -> Result<NaiveDate> {
    epoch()
        .checked_add_signed(TimeDelta::days(n as i64))
        .ok_or_else(|| LoomError::invalid(format!("date out of range: {n} days from epoch")))
}
fn time_to_nanos(t: NaiveTime) -> u64 {
    t.num_seconds_from_midnight() as u64 * 1_000_000_000 + t.nanosecond() as u64
}
fn nanos_to_time(n: u64) -> Result<NaiveTime> {
    let secs = (n / 1_000_000_000) as u32;
    let nanos = (n % 1_000_000_000) as u32;
    NaiveTime::from_num_seconds_from_midnight_opt(secs, nanos)
        .ok_or_else(|| LoomError::invalid(format!("time out of range: {n} ns")))
}
fn ndt_to_micros(dt: NaiveDateTime) -> i64 {
    dt.and_utc().timestamp_micros()
}
fn micros_to_ndt(us: i64) -> Result<NaiveDateTime> {
    DateTime::<Utc>::from_timestamp_micros(us)
        .map(|d| d.naive_utc())
        .ok_or_else(|| LoomError::invalid(format!("timestamp out of range: {us} us")))
}

/// A GlueSQL [`GValue`] as the equivalent tabular [`Value`] (lossless).
pub(crate) fn value_to_tabular(v: &GValue) -> Result<Value> {
    Ok(match v {
        GValue::Null => Value::Null,
        GValue::Bool(b) => Value::Bool(*b),
        GValue::I8(x) => Value::I8(*x),
        GValue::I16(x) => Value::I16(*x),
        GValue::I32(x) => Value::I32(*x),
        GValue::I64(x) => Value::Int(*x),
        GValue::I128(x) => Value::I128(*x),
        GValue::U8(x) => Value::U8(*x),
        GValue::U16(x) => Value::U16(*x),
        GValue::U32(x) => Value::U32(*x),
        GValue::U64(x) => Value::U64(*x),
        GValue::U128(x) => Value::U128(*x),
        GValue::F32(x) => Value::F32(*x),
        GValue::F64(x) => Value::Float(*x),
        GValue::Decimal(d) => Value::Decimal {
            mantissa: d.mantissa(),
            scale: d.scale(),
        },
        GValue::Str(s) => Value::Text(s.clone()),
        GValue::Bytea(b) => Value::Bytes(b.clone()),
        GValue::Inet(ip) => Value::Inet(*ip),
        GValue::Date(d) => Value::Date(date_to_days(*d)),
        GValue::Timestamp(ts) => Value::Timestamp(ndt_to_micros(*ts)),
        GValue::Time(t) => Value::Time(time_to_nanos(*t)),
        GValue::Interval(iv) => match iv {
            GInterval::Month(m) => Value::Interval {
                months: *m,
                micros: 0,
            },
            GInterval::Microsecond(us) => Value::Interval {
                months: 0,
                micros: *us,
            },
        },
        GValue::Uuid(u) => Value::Uuid(*u),
        GValue::Map(m) => Value::Map(
            m.iter()
                .map(|(k, v)| Ok((k.clone(), value_to_tabular(v)?)))
                .collect::<Result<BTreeMap<_, _>>>()?,
        ),
        GValue::List(items) => Value::List(
            items
                .iter()
                .map(value_to_tabular)
                .collect::<Result<Vec<_>>>()?,
        ),
        GValue::Point(p) => Value::Point { x: p.x, y: p.y },
    })
}

/// A tabular [`Value`] back as the equivalent GlueSQL [`GValue`] (inverse of [`value_to_tabular`]).
/// A zero interval round-trips as `Microsecond(0)` (`Month(0)` and `Microsecond(0)` denote the same
/// interval).
pub(crate) fn value_from_tabular(v: &Value) -> Result<GValue> {
    Ok(match v {
        Value::Null => GValue::Null,
        Value::Bool(b) => GValue::Bool(*b),
        Value::I8(x) => GValue::I8(*x),
        Value::I16(x) => GValue::I16(*x),
        Value::I32(x) => GValue::I32(*x),
        Value::Int(x) => GValue::I64(*x),
        Value::I128(x) => GValue::I128(*x),
        Value::U8(x) => GValue::U8(*x),
        Value::U16(x) => GValue::U16(*x),
        Value::U32(x) => GValue::U32(*x),
        Value::U64(x) => GValue::U64(*x),
        Value::U128(x) => GValue::U128(*x),
        Value::F32(x) => GValue::F32(*x),
        Value::Float(x) => GValue::F64(*x),
        Value::Decimal { mantissa, scale } => GValue::Decimal(
            Decimal::try_from_i128_with_scale(*mantissa, *scale)
                .map_err(|e| LoomError::invalid(format!("decimal {mantissa}e-{scale}: {e}")))?,
        ),
        Value::Text(s) => GValue::Str(s.clone()),
        Value::Bytes(b) => GValue::Bytea(b.clone()),
        Value::Inet(ip) => GValue::Inet(*ip),
        Value::Date(d) => GValue::Date(days_to_date(*d)?),
        Value::Time(t) => GValue::Time(nanos_to_time(*t)?),
        Value::Timestamp(us) => GValue::Timestamp(micros_to_ndt(*us)?),
        Value::Interval { months, micros } => GValue::Interval(if *months != 0 {
            GInterval::Month(*months)
        } else {
            GInterval::Microsecond(*micros)
        }),
        Value::Uuid(u) => GValue::Uuid(*u),
        Value::Map(m) => GValue::Map(
            m.iter()
                .map(|(k, v)| Ok((k.clone(), value_from_tabular(v)?)))
                .collect::<Result<BTreeMap<_, _>>>()?,
        ),
        Value::List(items) => GValue::List(
            items
                .iter()
                .map(value_from_tabular)
                .collect::<Result<Vec<_>>>()?,
        ),
        Value::Point { x, y } => GValue::Point(GPoint::new(*x, *y)),
    })
}

/// A GlueSQL primary-[`Key`] as a tabular [`Value`] (via GlueSQL's own `Key -> Value`).
pub(crate) fn key_to_tabular(k: &Key) -> Result<Value> {
    value_to_tabular(&GValue::from(k.clone()))
}

/// A tabular [`Value`] as a GlueSQL primary-[`Key`] (via GlueSQL's own `Value -> Key`); errors if the
/// value is not a valid key type (e.g. a `List`/`Map`).
pub(crate) fn key_from_tabular(v: &Value) -> Result<Key> {
    let gv = value_from_tabular(v)?;
    Key::try_from(&gv).map_err(|e| LoomError::invalid(format!("value is not a key: {e}")))
}

/// A GlueSQL [`GDataType`] as the tabular [`ColumnType`] a column of that type maps onto.
pub(crate) fn coltype_to_tabular(dt: &GDataType) -> ColumnType {
    match dt {
        GDataType::Boolean => ColumnType::Bool,
        GDataType::Int8 => ColumnType::I8,
        GDataType::Int16 => ColumnType::I16,
        GDataType::Int32 => ColumnType::I32,
        GDataType::Int => ColumnType::Int,
        GDataType::Int128 => ColumnType::I128,
        GDataType::Uint8 => ColumnType::U8,
        GDataType::Uint16 => ColumnType::U16,
        GDataType::Uint32 => ColumnType::U32,
        GDataType::Uint64 => ColumnType::U64,
        GDataType::Uint128 => ColumnType::U128,
        GDataType::Float32 => ColumnType::F32,
        GDataType::Float => ColumnType::Float,
        GDataType::Text => ColumnType::Text,
        GDataType::Bytea => ColumnType::Bytes,
        GDataType::Inet => ColumnType::Inet,
        GDataType::Date => ColumnType::Date,
        GDataType::Timestamp => ColumnType::Timestamp,
        GDataType::Time => ColumnType::Time,
        GDataType::Interval => ColumnType::Interval,
        GDataType::Uuid => ColumnType::Uuid,
        GDataType::Map => ColumnType::Map,
        GDataType::List => ColumnType::List,
        GDataType::Decimal => ColumnType::Decimal,
        GDataType::Point => ColumnType::Point,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn dec(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    #[test]
    fn every_gluesql_value_round_trips_through_tabular() {
        let samples = vec![
            GValue::Null,
            GValue::Bool(true),
            GValue::I8(-8),
            GValue::I16(-16),
            GValue::I32(-32),
            GValue::I64(-64),
            GValue::I128(-128),
            GValue::U8(8),
            GValue::U16(16),
            GValue::U32(32),
            GValue::U64(64),
            GValue::U128(128),
            GValue::F32(-1.5),
            GValue::F64(2.5),
            GValue::Decimal(dec("123.4500")),
            GValue::Str("hello".into()),
            GValue::Bytea(vec![0, 1, 2, 255]),
            GValue::Inet(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))),
            GValue::Date(NaiveDate::from_ymd_opt(2026, 6, 20).unwrap()),
            GValue::Timestamp(
                NaiveDate::from_ymd_opt(1969, 12, 31)
                    .unwrap()
                    .and_hms_micro_opt(23, 59, 59, 123_456)
                    .unwrap(),
            ),
            GValue::Time(NaiveTime::from_hms_nano_opt(13, 45, 30, 123_456_789).unwrap()),
            GValue::Interval(GInterval::Month(15)),
            GValue::Interval(GInterval::Microsecond(-987_654)),
            GValue::Uuid(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef),
            GValue::Map(BTreeMap::from([("k".to_string(), GValue::I64(9))])),
            GValue::List(vec![GValue::I64(1), GValue::Str("x".into())]),
            GValue::Point(GPoint::new(1.25, -2.5)),
        ];
        for g in &samples {
            let t = value_to_tabular(g).unwrap();
            let back = value_from_tabular(&t).unwrap();
            assert_eq!(&back, g, "round-trip for {g:?}");
        }
    }

    #[test]
    fn decimal_scale_and_negative_round_trip() {
        for s in ["0", "-0.01", "1000000", "-123456.789", "0.0000000001"] {
            let g = GValue::Decimal(dec(s));
            let back = value_from_tabular(&value_to_tabular(&g).unwrap()).unwrap();
            assert_eq!(back, g, "decimal {s}");
        }
    }

    #[test]
    fn keys_round_trip_and_datatypes_map_both_ways() {
        // Key round-trip via the tabular value bridge.
        for k in [
            Key::I64(-5),
            Key::Str("k".into()),
            Key::Uuid(7),
            Key::Bool(true),
            Key::None,
        ] {
            let v = key_to_tabular(&k).unwrap();
            assert_eq!(key_from_tabular(&v).unwrap(), k, "key {k:?}");
        }
        // DataType -> ColumnType maps each SQL type onto its tabular column type.
        for (dt, ct) in [
            (GDataType::Boolean, ColumnType::Bool),
            (GDataType::Int8, ColumnType::I8),
            (GDataType::Int, ColumnType::Int),
            (GDataType::Uint128, ColumnType::U128),
            (GDataType::Float32, ColumnType::F32),
            (GDataType::Decimal, ColumnType::Decimal),
            (GDataType::Date, ColumnType::Date),
            (GDataType::Timestamp, ColumnType::Timestamp),
            (GDataType::Uuid, ColumnType::Uuid),
            (GDataType::Point, ColumnType::Point),
            (GDataType::Map, ColumnType::Map),
        ] {
            assert_eq!(coltype_to_tabular(&dt), ct, "datatype {dt:?}");
        }
    }
}
