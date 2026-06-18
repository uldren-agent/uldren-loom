//! `VTIMEZONE` parsing and UTC-offset resolution (0037 RD10).
//!
//! A `VTIMEZONE` is a set of `STANDARD`/`DAYLIGHT` observances, each an offset change recurring by its
//! own `RRULE`. This module reuses the same [`crate::RRule`] expander to enumerate transition instants,
//! so one recurrence engine serves both event recurrence and timezone resolution - no `chrono-tz`, no
//! embedded zone database. The caller supplies the resource's own `VTIMEZONE` text.

use crate::{Error, RRule};
use time::{Date, Duration, Month, PrimitiveDateTime, Time};

/// One `STANDARD` or `DAYLIGHT` observance: an offset change effective from `dtstart` (a wall-clock
/// time in `offset_from`) and recurring per `rrule`/`rdates`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observance {
    /// Local wall-clock start of the first transition, interpreted in `offset_from`.
    pub dtstart: PrimitiveDateTime,
    /// UTC offset in seconds in effect before the transition.
    pub offset_from: i32,
    /// UTC offset in seconds in effect after the transition.
    pub offset_to: i32,
    pub rrule: Option<RRule>,
    pub rdates: Vec<PrimitiveDateTime>,
}

/// A parsed `VTIMEZONE`: a `TZID` and its observances.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VTimeZone {
    pub tzid: String,
    pub observances: Vec<Observance>,
}

/// `(utc_instant_of_transition, offset_to_seconds)`.
type Transition = (PrimitiveDateTime, i32);

fn parse_offset(v: &str) -> Option<i32> {
    let (sign, rest) = match v.as_bytes().first()? {
        b'+' => (1, &v[1..]),
        b'-' => (-1, &v[1..]),
        _ => return None,
    };
    if rest.len() != 4 && rest.len() != 6 {
        return None;
    }
    if !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let h: i32 = rest[0..2].parse().ok()?;
    let m: i32 = rest[2..4].parse().ok()?;
    let s: i32 = if rest.len() == 6 {
        rest[4..6].parse().ok()?
    } else {
        0
    };
    Some(sign * (h * 3600 + m * 60 + s))
}

fn parse_dt(v: &str) -> Option<PrimitiveDateTime> {
    // VTIMEZONE DTSTART is local (floating) time: YYYYMMDDTHHMMSS, never with a Z suffix.
    let (d, t) = v.split_once('T')?;
    if d.len() != 8 || t.len() != 6 || !d.bytes().chain(t.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let date = Date::from_calendar_date(
        d[0..4].parse().ok()?,
        Month::try_from(d[4..6].parse::<u8>().ok()?).ok()?,
        d[6..8].parse().ok()?,
    )
    .ok()?;
    let time = Time::from_hms(
        t[0..2].parse().ok()?,
        t[2..4].parse().ok()?,
        t[4..6].parse().ok()?,
    )
    .ok()?;
    Some(PrimitiveDateTime::new(date, time))
}

impl VTimeZone {
    /// Parse a `VTIMEZONE` component (an unfolded iCalendar block, one property per line).
    pub fn parse(input: &str) -> Result<Self, Error> {
        let mut tzid = String::new();
        let mut observances = Vec::new();
        let mut cur: Option<ObservanceBuilder> = None;

        for raw in input.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let (name, value) = match line.split_once(':') {
                Some((n, v)) => (n.split(';').next().unwrap_or(n), v),
                None => continue,
            };
            match name {
                "BEGIN" if value == "STANDARD" || value == "DAYLIGHT" => {
                    cur = Some(ObservanceBuilder::default());
                }
                "END" if value == "STANDARD" || value == "DAYLIGHT" => {
                    if let Some(b) = cur.take() {
                        observances.push(b.build()?);
                    }
                }
                "TZID" => tzid = value.to_string(),
                "DTSTART" => {
                    if let Some(b) = cur.as_mut() {
                        b.dtstart = parse_dt(value);
                    }
                }
                "TZOFFSETFROM" => {
                    if let Some(b) = cur.as_mut() {
                        b.offset_from = parse_offset(value);
                    }
                }
                "TZOFFSETTO" => {
                    if let Some(b) = cur.as_mut() {
                        b.offset_to = parse_offset(value);
                    }
                }
                "RRULE" => {
                    if let Some(b) = cur.as_mut() {
                        b.rrule = Some(RRule::parse(value)?);
                    }
                }
                "RDATE" => {
                    if let Some(b) = cur.as_mut() {
                        for part in value.split(',') {
                            if let Some(dt) = parse_dt(part) {
                                b.rdates.push(dt);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if observances.is_empty() {
            return Err(Error::InvalidValue {
                key: "VTIMEZONE",
                value: "no STANDARD/DAYLIGHT observances".to_string(),
            });
        }
        Ok(VTimeZone { tzid, observances })
    }

    /// All transition instants (in UTC) within `[lo, hi]` of UTC years, paired with the offset that
    /// takes effect at each. Includes each observance's own `dtstart` transition.
    fn transitions(&self, lo: PrimitiveDateTime, hi: PrimitiveDateTime) -> Vec<Transition> {
        let mut out: Vec<Transition> = Vec::new();
        for ob in &self.observances {
            // The transition's UTC instant is its local start minus the prior (from) offset.
            let to_utc =
                |local: PrimitiveDateTime| local - Duration::seconds(i64::from(ob.offset_from));
            out.push((to_utc(ob.dtstart), ob.offset_to));
            if let Some(r) = &ob.rrule {
                // Expand over a generous local window; convert each to its UTC instant.
                for occ in r.expand(ob.dtstart, lo, hi) {
                    out.push((to_utc(occ), ob.offset_to));
                }
            }
            for &rd in &ob.rdates {
                out.push((to_utc(rd), ob.offset_to));
            }
        }
        out.sort_by_key(|&(utc, _)| utc);
        out
    }

    /// The UTC offset (seconds) in effect at a given UTC instant.
    pub fn offset_at_utc(&self, utc: PrimitiveDateTime) -> i32 {
        let lo = PrimitiveDateTime::new(
            Date::from_calendar_date(utc.year() - 2, Month::January, 1).expect("valid"),
            Time::MIDNIGHT,
        );
        let hi = PrimitiveDateTime::new(
            Date::from_calendar_date(utc.year() + 2, Month::January, 1).expect("valid"),
            Time::MIDNIGHT,
        );
        let trans = self.transitions(lo, hi);
        let mut offset = self
            .observances
            .iter()
            .map(|o| o.offset_from)
            .next()
            .unwrap_or(0);
        for (instant, off) in trans {
            if instant <= utc {
                offset = off;
            } else {
                break;
            }
        }
        offset
    }

    /// Convert a local wall-clock time in this zone to its UTC instant. Iterates twice to settle the
    /// offset across a transition boundary (the common ambiguous/gap cases resolve to the post-step
    /// offset).
    pub fn to_utc(&self, local: PrimitiveDateTime) -> PrimitiveDateTime {
        let mut off = self.offset_at_utc(local);
        for _ in 0..2 {
            let utc = local - Duration::seconds(i64::from(off));
            off = self.offset_at_utc(utc);
        }
        local - Duration::seconds(i64::from(off))
    }
}

#[derive(Default)]
struct ObservanceBuilder {
    dtstart: Option<PrimitiveDateTime>,
    offset_from: Option<i32>,
    offset_to: Option<i32>,
    rrule: Option<RRule>,
    rdates: Vec<PrimitiveDateTime>,
}

impl ObservanceBuilder {
    fn build(self) -> Result<Observance, Error> {
        Ok(Observance {
            dtstart: self.dtstart.ok_or(Error::InvalidValue {
                key: "VTIMEZONE",
                value: "observance missing DTSTART".to_string(),
            })?,
            offset_from: self.offset_from.ok_or(Error::InvalidValue {
                key: "VTIMEZONE",
                value: "observance missing TZOFFSETFROM".to_string(),
            })?,
            offset_to: self.offset_to.ok_or(Error::InvalidValue {
                key: "VTIMEZONE",
                value: "observance missing TZOFFSETTO".to_string(),
            })?,
            rrule: self.rrule,
            rdates: self.rdates,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NY: &str = "BEGIN:VTIMEZONE\n\
TZID:America/New_York\n\
BEGIN:DAYLIGHT\n\
DTSTART:20070311T020000\n\
TZOFFSETFROM:-0500\n\
TZOFFSETTO:-0400\n\
RRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU\n\
END:DAYLIGHT\n\
BEGIN:STANDARD\n\
DTSTART:20071104T020000\n\
TZOFFSETFROM:-0400\n\
TZOFFSETTO:-0500\n\
RRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU\n\
END:STANDARD\n\
END:VTIMEZONE\n";

    fn dt(y: i32, m: u8, d: u8, hh: u8, mm: u8) -> PrimitiveDateTime {
        PrimitiveDateTime::new(
            Date::from_calendar_date(y, Month::try_from(m).unwrap(), d).unwrap(),
            Time::from_hms(hh, mm, 0).unwrap(),
        )
    }

    #[test]
    fn parses_two_observances() {
        let tz = VTimeZone::parse(NY).unwrap();
        assert_eq!(tz.tzid, "America/New_York");
        assert_eq!(tz.observances.len(), 2);
    }

    #[test]
    fn offset_summer_is_edt() {
        let tz = VTimeZone::parse(NY).unwrap();
        // 2024-07-01 12:00 UTC is during EDT.
        assert_eq!(tz.offset_at_utc(dt(2024, 7, 1, 12, 0)), -4 * 3600);
    }

    #[test]
    fn offset_winter_is_est() {
        let tz = VTimeZone::parse(NY).unwrap();
        assert_eq!(tz.offset_at_utc(dt(2024, 1, 15, 12, 0)), -5 * 3600);
    }

    #[test]
    fn transition_boundaries() {
        let tz = VTimeZone::parse(NY).unwrap();
        // Spring forward 2024-03-10 02:00 local EST -> the UTC instant is 07:00Z; just before is EST.
        assert_eq!(tz.offset_at_utc(dt(2024, 3, 10, 6, 59)), -5 * 3600);
        assert_eq!(tz.offset_at_utc(dt(2024, 3, 10, 7, 0)), -4 * 3600);
        // Fall back 2024-11-03 02:00 local EDT -> UTC 06:00Z.
        assert_eq!(tz.offset_at_utc(dt(2024, 11, 3, 5, 59)), -4 * 3600);
        assert_eq!(tz.offset_at_utc(dt(2024, 11, 3, 6, 0)), -5 * 3600);
    }

    #[test]
    fn local_to_utc_roundtrip() {
        let tz = VTimeZone::parse(NY).unwrap();
        // A summer 09:00 local meeting is 13:00Z (EDT -4).
        assert_eq!(tz.to_utc(dt(2024, 7, 1, 9, 0)), dt(2024, 7, 1, 13, 0));
        // A winter 09:00 local meeting is 14:00Z (EST -5).
        assert_eq!(tz.to_utc(dt(2024, 1, 15, 9, 0)), dt(2024, 1, 15, 14, 0));
    }
}
