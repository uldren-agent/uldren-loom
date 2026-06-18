//! Canonical wire helpers for the calendar facet range/search accessors, shared by the C ABI, the
//! in-process client service impl, and the server dispatch. A range window bound crosses as a
//! `YYYYMMDDTHHMMSS` wall-clock string; a component filter as `""`/`"event"`/`"todo"`; a range result
//! as a CBOR array of `[uid, start]` pairs (each `start` a `YYYYMMDDTHHMMSS` string).

use loom_codec::{Value as CborValue, encode};
use loom_core::calendar::{Component, DateTime, IcalDate, IcalMonth, IcalTime, Occurrence};
use loom_types::LoomError;

/// Parse a `YYYYMMDDTHHMMSS` (15 bytes, `T` at index 8) wall-clock string into a [`DateTime`].
pub fn parse_window_bound(s: &str, what: &str) -> Result<DateTime, LoomError> {
    let bytes = s.as_bytes();
    let bad = || {
        LoomError::invalid(format!(
            "calendar {what} must be YYYYMMDDTHHMMSS, got {s:?}"
        ))
    };
    if bytes.len() != 15 || bytes[8] != b'T' {
        return Err(bad());
    }
    let digits = |range: std::ops::Range<usize>| -> Result<&str, LoomError> {
        let part = &s[range];
        if part.bytes().all(|b| b.is_ascii_digit()) {
            Ok(part)
        } else {
            Err(bad())
        }
    };
    let num = |part: &str| -> Result<u32, LoomError> { part.parse::<u32>().map_err(|_| bad()) };
    let year = num(digits(0..4)?)?;
    let month = num(digits(4..6)?)?;
    let day = num(digits(6..8)?)?;
    let hour = num(digits(9..11)?)?;
    let minute = num(digits(11..13)?)?;
    let second = num(digits(13..15)?)?;
    let month = IcalMonth::try_from(u8::try_from(month).map_err(|_| bad())?).map_err(|_| bad())?;
    let date = IcalDate::from_calendar_date(
        i32::try_from(year).map_err(|_| bad())?,
        month,
        u8::try_from(day).map_err(|_| bad())?,
    )
    .map_err(|_| bad())?;
    let time = IcalTime::from_hms(
        u8::try_from(hour).map_err(|_| bad())?,
        u8::try_from(minute).map_err(|_| bad())?,
        u8::try_from(second).map_err(|_| bad())?,
    )
    .map_err(|_| bad())?;
    Ok(DateTime::new(date, time))
}

/// Render a wall-clock [`DateTime`] as its `YYYYMMDDTHHMMSS` wire form.
pub fn format_window_bound(dt: &DateTime) -> String {
    let d = dt.date();
    let t = dt.time();
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}",
        d.year(),
        u8::from(d.month()),
        d.day(),
        t.hour(),
        t.minute(),
        t.second(),
    )
}

/// Map a component-filter string to an optional component: `""` -> `None`, `"event"` ->
/// `Some(Event)`, `"todo"` -> `Some(Todo)`. Any other token is an error.
pub fn parse_component_filter(component: &str) -> Result<Option<Component>, LoomError> {
    match component {
        "" => Ok(None),
        "event" => Ok(Some(Component::Event)),
        "todo" => Ok(Some(Component::Todo)),
        other => Err(LoomError::invalid(format!(
            "calendar unknown component filter {other:?}"
        ))),
    }
}

/// Encode range occurrences as a CBOR array of `[uid, start]` pairs.
pub fn occurrences_to_cbor(occurrences: Vec<Occurrence>) -> Result<Vec<u8>, LoomError> {
    let items = occurrences
        .into_iter()
        .map(|o| {
            CborValue::Array(vec![
                CborValue::Text(o.uid),
                CborValue::Text(format_window_bound(&o.start)),
            ])
        })
        .collect();
    encode(&CborValue::Array(items)).map_err(|e| LoomError::corrupt(format!("cbor: {e}")))
}

/// Decode the CBOR array of `[uid, start]` pairs produced by [`occurrences_to_cbor`] back into
/// occurrences, parsing each `start` from its `YYYYMMDDTHHMMSS` wall-clock form.
pub fn occurrences_from_cbor(bytes: &[u8]) -> Result<Vec<Occurrence>, LoomError> {
    let bad =
        || LoomError::corrupt("calendar range payload is not a CBOR array of [uid, start] pairs");
    let value = loom_codec::decode(bytes).map_err(|e| LoomError::corrupt(format!("cbor: {e}")))?;
    let CborValue::Array(items) = value else {
        return Err(bad());
    };
    items
        .into_iter()
        .map(|item| {
            let CborValue::Array(pair) = item else {
                return Err(bad());
            };
            let [CborValue::Text(uid), CborValue::Text(start)] = pair.as_slice() else {
                return Err(bad());
            };
            Ok(Occurrence {
                uid: uid.clone(),
                start: parse_window_bound(start, "start")?,
            })
        })
        .collect()
}
