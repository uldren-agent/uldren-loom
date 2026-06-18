//! Owned RFC 5545 recurrence engine for the calendar facet (0037).
//!
//! Expansion is a pure, deterministic function of `(rule, dtstart, window)` over proleptic-Gregorian
//! wall-clock time (the `time` crate). It embeds no timezone database: UTC resolution for zoned events
//! is the caller's job via the resource's own `VTIMEZONE` (0037 RD10). This keeps the engine wasm-clean
//! and ~zero marginal binary footprint versus pulling `chrono-tz`.
//!
//! Implemented: every RFC 5545 `FREQ` (SECONDLY/MINUTELY/HOURLY/DAILY/WEEKLY/MONTHLY/YEARLY) with
//! `INTERVAL`, `COUNT`, `UNTIL`, `WKST`, and the BY-rule set `BYSECOND`/`BYMINUTE`/`BYHOUR`/`BYDAY`
//! (incl. ordinals like `2MO`/`-1FR`)/`BYMONTHDAY` (incl. negative)/`BYYEARDAY` (incl. negative)/
//! `BYWEEKNO` (ISO weeks)/`BYMONTH`/`BYSETPOS`, applied per the RFC 5545 expand/limit matrix and
//! evaluated within a bounded `[from, to)` window. [`RecurrenceSet`] composes an `RRULE` with `RDATE`
//! and `EXDATE` to form a full component recurrence set, with `DTSTART` always the first instance.

use std::collections::BTreeSet;
use thiserror::Error;
use time::{Date, Duration, Month, PrimitiveDateTime, Time, Weekday};

mod vtimezone;
pub use vtimezone::{Observance, VTimeZone};

/// Recurrence frequency (the full RFC 5545 `FREQ` set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frequency {
    Secondly,
    Minutely,
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// One `BYDAY` term: a weekday, optionally with an ordinal (`2MO` = 2nd Monday, `-1FR` = last Friday).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByDay {
    pub ordinal: Option<i8>,
    pub weekday: Weekday,
}

/// A parsed `RRULE`. Construct with [`RRule::parse`]; expand with [`RRule::expand`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RRule {
    pub freq: Frequency,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<PrimitiveDateTime>,
    pub by_second: Vec<u8>,
    pub by_minute: Vec<u8>,
    pub by_hour: Vec<u8>,
    pub by_day: Vec<ByDay>,
    pub by_month_day: Vec<i8>,
    pub by_year_day: Vec<i16>,
    pub by_week_no: Vec<i8>,
    pub by_month: Vec<u8>,
    pub by_set_pos: Vec<i16>,
    pub wkst: Weekday,
}

/// Parse/expansion errors. Stable, caller-mappable.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("rrule: empty rule")]
    Empty,
    #[error("rrule: missing FREQ")]
    MissingFreq,
    #[error("rrule: malformed part {0:?} (expected KEY=VALUE)")]
    MalformedPart(String),
    #[error("rrule: unknown or unsupported key {0:?}")]
    UnknownKey(String),
    #[error("rrule: invalid value {value:?} for {key}")]
    InvalidValue { key: &'static str, value: String },
    #[error("rrule: unsupported FREQ {0:?}")]
    UnsupportedFreq(String),
}

fn parse_weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "MO" => Weekday::Monday,
        "TU" => Weekday::Tuesday,
        "WE" => Weekday::Wednesday,
        "TH" => Weekday::Thursday,
        "FR" => Weekday::Friday,
        "SA" => Weekday::Saturday,
        "SU" => Weekday::Sunday,
        _ => return None,
    })
}

/// Parse a `YYYYMMDD[THHMMSS[Z]]` value into a [`PrimitiveDateTime`]. A bare date with `end_of_day`
/// true maps to 23:59:59 (used for inclusive `UNTIL`); otherwise to 00:00:00.
fn parse_datetime(v: &str, end_of_day: bool) -> Option<PrimitiveDateTime> {
    let core = v.strip_suffix('Z').unwrap_or(v);
    let (date_part, time_part) = match core.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (core, None),
    };
    if date_part.len() != 8 || !date_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year: i32 = date_part[0..4].parse().ok()?;
    let month = Month::try_from(date_part[4..6].parse::<u8>().ok()?).ok()?;
    let day: u8 = date_part[6..8].parse().ok()?;
    let date = Date::from_calendar_date(year, month, day).ok()?;
    let time = match time_part {
        Some(t) if t.len() == 6 && t.bytes().all(|b| b.is_ascii_digit()) => Time::from_hms(
            t[0..2].parse().ok()?,
            t[2..4].parse().ok()?,
            t[4..6].parse().ok()?,
        )
        .ok()?,
        Some(_) => return None,
        None if end_of_day => Time::from_hms(23, 59, 59).ok()?,
        None => Time::MIDNIGHT,
    };
    Some(PrimitiveDateTime::new(date, time))
}

fn parse_int_list<T: std::str::FromStr>(value: &str, key: &'static str) -> Result<Vec<T>, Error> {
    value
        .split(',')
        .map(|s| {
            s.parse::<T>().map_err(|_| Error::InvalidValue {
                key,
                value: s.to_string(),
            })
        })
        .collect()
}

impl RRule {
    /// Parse an `RRULE` line. Accepts an optional `RRULE:` prefix.
    pub fn parse(input: &str) -> Result<Self, Error> {
        let body = input.trim().strip_prefix("RRULE:").unwrap_or(input.trim());
        if body.is_empty() {
            return Err(Error::Empty);
        }
        let mut r = RRule {
            freq: Frequency::Daily,
            interval: 1,
            count: None,
            until: None,
            by_second: Vec::new(),
            by_minute: Vec::new(),
            by_hour: Vec::new(),
            by_day: Vec::new(),
            by_month_day: Vec::new(),
            by_year_day: Vec::new(),
            by_week_no: Vec::new(),
            by_month: Vec::new(),
            by_set_pos: Vec::new(),
            wkst: Weekday::Monday,
        };
        let mut freq = None;

        for part in body.split(';') {
            let (key, value) = part
                .split_once('=')
                .ok_or_else(|| Error::MalformedPart(part.to_string()))?;
            match key {
                "FREQ" => {
                    freq = Some(match value {
                        "SECONDLY" => Frequency::Secondly,
                        "MINUTELY" => Frequency::Minutely,
                        "HOURLY" => Frequency::Hourly,
                        "DAILY" => Frequency::Daily,
                        "WEEKLY" => Frequency::Weekly,
                        "MONTHLY" => Frequency::Monthly,
                        "YEARLY" => Frequency::Yearly,
                        other => return Err(Error::UnsupportedFreq(other.to_string())),
                    });
                }
                "INTERVAL" => {
                    r.interval = value.parse().map_err(|_| Error::InvalidValue {
                        key: "INTERVAL",
                        value: value.to_string(),
                    })?;
                    if r.interval == 0 {
                        return Err(Error::InvalidValue {
                            key: "INTERVAL",
                            value: value.into(),
                        });
                    }
                }
                "COUNT" => {
                    r.count = Some(value.parse().map_err(|_| Error::InvalidValue {
                        key: "COUNT",
                        value: value.to_string(),
                    })?);
                }
                "UNTIL" => {
                    r.until =
                        Some(
                            parse_datetime(value, true).ok_or_else(|| Error::InvalidValue {
                                key: "UNTIL",
                                value: value.to_string(),
                            })?,
                        );
                }
                "BYSECOND" => {
                    r.by_second = parse_int_list(value, "BYSECOND")?;
                    for &s in &r.by_second {
                        if s > 60 {
                            return Err(Error::InvalidValue {
                                key: "BYSECOND",
                                value: value.into(),
                            });
                        }
                    }
                }
                "BYMINUTE" => {
                    r.by_minute = parse_int_list(value, "BYMINUTE")?;
                    for &m in &r.by_minute {
                        if m > 59 {
                            return Err(Error::InvalidValue {
                                key: "BYMINUTE",
                                value: value.into(),
                            });
                        }
                    }
                }
                "BYHOUR" => {
                    r.by_hour = parse_int_list(value, "BYHOUR")?;
                    for &h in &r.by_hour {
                        if h > 23 {
                            return Err(Error::InvalidValue {
                                key: "BYHOUR",
                                value: value.into(),
                            });
                        }
                    }
                }
                "BYDAY" => {
                    for term in value.split(',') {
                        let split = term.len().saturating_sub(2);
                        let (ord, wd) = term.split_at(split);
                        let weekday = parse_weekday(wd).ok_or_else(|| Error::InvalidValue {
                            key: "BYDAY",
                            value: term.to_string(),
                        })?;
                        let ordinal = if ord.is_empty() {
                            None
                        } else {
                            Some(ord.parse().map_err(|_| Error::InvalidValue {
                                key: "BYDAY",
                                value: term.to_string(),
                            })?)
                        };
                        r.by_day.push(ByDay { ordinal, weekday });
                    }
                }
                "BYMONTHDAY" => {
                    r.by_month_day = parse_int_list(value, "BYMONTHDAY")?;
                    for &n in &r.by_month_day {
                        if n == 0 || !(-31..=31).contains(&n) {
                            return Err(Error::InvalidValue {
                                key: "BYMONTHDAY",
                                value: value.into(),
                            });
                        }
                    }
                }
                "BYYEARDAY" => {
                    r.by_year_day = parse_int_list(value, "BYYEARDAY")?;
                    for &n in &r.by_year_day {
                        if n == 0 || !(-366..=366).contains(&n) {
                            return Err(Error::InvalidValue {
                                key: "BYYEARDAY",
                                value: value.into(),
                            });
                        }
                    }
                }
                "BYWEEKNO" => {
                    r.by_week_no = parse_int_list(value, "BYWEEKNO")?;
                    for &n in &r.by_week_no {
                        if n == 0 || !(-53..=53).contains(&n) {
                            return Err(Error::InvalidValue {
                                key: "BYWEEKNO",
                                value: value.into(),
                            });
                        }
                    }
                }
                "BYMONTH" => {
                    r.by_month = parse_int_list(value, "BYMONTH")?;
                    for &n in &r.by_month {
                        if !(1..=12).contains(&n) {
                            return Err(Error::InvalidValue {
                                key: "BYMONTH",
                                value: value.into(),
                            });
                        }
                    }
                }
                "BYSETPOS" => {
                    r.by_set_pos = parse_int_list(value, "BYSETPOS")?;
                    for &n in &r.by_set_pos {
                        if n == 0 || !(-366..=366).contains(&n) {
                            return Err(Error::InvalidValue {
                                key: "BYSETPOS",
                                value: value.into(),
                            });
                        }
                    }
                }
                "WKST" => {
                    r.wkst = parse_weekday(value).ok_or_else(|| Error::InvalidValue {
                        key: "WKST",
                        value: value.to_string(),
                    })?;
                }
                other => return Err(Error::UnknownKey(other.to_string())),
            }
        }

        r.freq = freq.ok_or(Error::MissingFreq)?;
        Ok(r)
    }

    /// Expand occurrences in the half-open window `[from, to)`, on or after `dtstart`. `COUNT` and
    /// `UNTIL` are evaluated against the full series from `dtstart` (not the window), matching RFC 5545.
    pub fn expand(
        &self,
        dtstart: PrimitiveDateTime,
        from: PrimitiveDateTime,
        to: PrimitiveDateTime,
    ) -> Vec<PrimitiveDateTime> {
        let mut out = Vec::new();
        let mut emitted = 0u32;
        // Safety cap: every period strictly advances and `period_start_after` breaks once a period
        // begins past `to`, so this only guards a pathological rule that never matches.
        const MAX_PERIODS: u32 = 1_000_000;

        for n in 0..MAX_PERIODS {
            let mut set = self.period_set(dtstart, n);
            set.sort_unstable();
            set.dedup();
            // BYSETPOS selects over the FULL period set (RFC 5545); the dtstart lower bound is applied
            // only after selection, so the first (partial) period's set-positions are still correct.
            if !self.by_set_pos.is_empty() {
                set = select_set_pos(&set, &self.by_set_pos);
            }
            set.retain(|&dt| dt >= dtstart);
            for dt in set {
                if self.until.is_some_and(|u| dt > u) {
                    return out;
                }
                if self.count.is_some_and(|c| emitted >= c) {
                    return out;
                }
                emitted += 1;
                if dt >= to {
                    return out;
                }
                if dt >= from {
                    out.push(dt);
                }
            }
            if self.period_start_after(dtstart, n, to) {
                break;
            }
        }
        out
    }

    /// The full candidate occurrence set for period `n` (unsorted, unfiltered by window/count/until).
    fn period_set(&self, dtstart: PrimitiveDateTime, n: u32) -> Vec<PrimitiveDateTime> {
        let start = dtstart.date();
        match self.freq {
            Frequency::Secondly | Frequency::Minutely | Frequency::Hourly => {
                self.subdaily_set(dtstart, n)
            }
            _ => {
                let dates = match self.freq {
                    Frequency::Daily => self.daily_dates(start, n),
                    Frequency::Weekly => self.weekly_dates(start, n),
                    Frequency::Monthly => self.monthly_dates(start, n),
                    Frequency::Yearly => self.yearly_dates(start, n),
                    _ => unreachable!(),
                };
                let times = self.times_of_day(dtstart.time());
                let mut out = Vec::with_capacity(dates.len() * times.len());
                for d in dates {
                    for &t in &times {
                        out.push(PrimitiveDateTime::new(d, t));
                    }
                }
                out
            }
        }
    }

    /// Time-of-day set for the date frequencies: the cartesian product of `BYHOUR`/`BYMINUTE`/
    /// `BYSECOND` (each defaulting to the `dtstart` component when its BY-rule is absent).
    fn times_of_day(&self, default: Time) -> Vec<Time> {
        let hours = if self.by_hour.is_empty() {
            vec![default.hour()]
        } else {
            self.by_hour.clone()
        };
        let minutes = if self.by_minute.is_empty() {
            vec![default.minute()]
        } else {
            self.by_minute.clone()
        };
        let seconds = if self.by_second.is_empty() {
            vec![default.second()]
        } else {
            self.by_second.clone()
        };
        let mut out = Vec::new();
        for h in hours {
            for &m in &minutes {
                for &s in &seconds {
                    // BYSECOND allows 60 (leap second); clamp to 59 for wall-clock construction.
                    if let Ok(t) = Time::from_hms(h, m, s.min(59)) {
                        out.push(t);
                    }
                }
            }
        }
        out
    }

    fn daily_dates(&self, start: Date, n: u32) -> Vec<Date> {
        let date = start + Duration::days(i64::from(n) * i64::from(self.interval));
        if self.date_limits_ok(date) {
            vec![date]
        } else {
            Vec::new()
        }
    }

    fn weekly_dates(&self, start: Date, n: u32) -> Vec<Date> {
        let week0 = start_of_week(start, self.wkst);
        let week = week0 + Duration::weeks(i64::from(n) * i64::from(self.interval));
        let weekdays = if self.by_day.is_empty() {
            vec![start.weekday()]
        } else {
            self.by_day.iter().map(|b| b.weekday).collect()
        };
        weekdays
            .into_iter()
            .map(|wd| week + Duration::days(i64::from(day_offset(self.wkst, wd))))
            .filter(|d| self.month_ok(d.month()))
            .collect()
    }

    fn monthly_dates(&self, start: Date, n: u32) -> Vec<Date> {
        let (y, m) = add_months(
            start.year(),
            u32::from(u8::from(start.month())),
            n * self.interval,
        );
        let month = Month::try_from(m as u8).expect("1..=12");
        if !self.month_ok(month) {
            return Vec::new();
        }
        let mut out = Vec::new();
        if !self.by_month_day.is_empty() {
            for &md in &self.by_month_day {
                if let Some(d) = self.matching_monthday(y, month, md) {
                    out.push(d);
                }
            }
        } else if !self.by_day.is_empty() {
            for b in &self.by_day {
                let all = weekdays_in_month(y, month, b.weekday);
                match b.ordinal {
                    None => out.extend(all),
                    Some(o) => {
                        if let Some(d) = nth(&all, o) {
                            out.push(d);
                        }
                    }
                }
            }
        } else if let Ok(d) = Date::from_calendar_date(y, month, start.day()) {
            out.push(d);
        }
        out
    }

    fn yearly_dates(&self, start: Date, n: u32) -> Vec<Date> {
        let y = start.year() + (n * self.interval) as i32;
        let mut out: Vec<Date> = Vec::new();

        if !self.by_year_day.is_empty() {
            let len = days_in_year(y);
            for &yd in &self.by_year_day {
                let ord = if yd > 0 {
                    yd as u16
                } else {
                    (len as i16 + yd + 1) as u16
                };
                if let Ok(d) = Date::from_ordinal_date(y, ord) {
                    out.push(d);
                }
            }
        } else if !self.by_week_no.is_empty() {
            let max_week = iso_weeks_in_year(y);
            for &wn in &self.by_week_no {
                let week = if wn > 0 {
                    wn as u8
                } else {
                    (max_week as i8 + wn + 1) as u8
                };
                let weekdays = if self.by_day.is_empty() {
                    (0..7).map(|o| Weekday::Monday.nth_next(o)).collect()
                } else {
                    self.by_day.iter().map(|b| b.weekday).collect::<Vec<_>>()
                };
                for wd in weekdays {
                    if let Ok(d) = Date::from_iso_week_date(y, week, wd) {
                        out.push(d);
                    }
                }
            }
        } else {
            // Month-driven expansion. BYMONTH expands; otherwise the anchor month(s) follow the
            // remaining BY-rules.
            let by_month_present = !self.by_month.is_empty();
            let months: Vec<Month> = if by_month_present {
                self.by_month
                    .iter()
                    .map(|&m| Month::try_from(m).expect("1..=12"))
                    .collect()
            } else if !self.by_month_day.is_empty()
                || self.by_day.iter().any(|b| b.ordinal.is_none())
            {
                (1..=12)
                    .map(|m| Month::try_from(m).expect("1..=12"))
                    .collect()
            } else {
                vec![start.month()]
            };

            // Ordinal BYDAY with no BYMONTH means "nth weekday of the year".
            if !by_month_present && self.by_day.iter().any(|b| b.ordinal.is_some()) {
                for b in &self.by_day {
                    if let Some(o) = b.ordinal {
                        let all = weekdays_in_year(y, b.weekday);
                        if let Some(d) = nth(&all, o) {
                            out.push(d);
                        }
                    } else {
                        out.extend(weekdays_in_year(y, b.weekday));
                    }
                }
            } else {
                for month in months {
                    if !self.by_month_day.is_empty() {
                        for &md in &self.by_month_day {
                            if let Some(d) = self.matching_monthday(y, month, md) {
                                out.push(d);
                            }
                        }
                    } else if !self.by_day.is_empty() {
                        for b in &self.by_day {
                            let all = weekdays_in_month(y, month, b.weekday);
                            match b.ordinal {
                                None => out.extend(all),
                                Some(o) => {
                                    if let Some(d) = nth(&all, o) {
                                        out.push(d);
                                    }
                                }
                            }
                        }
                    } else if let Ok(d) = Date::from_calendar_date(y, month, start.day()) {
                        out.push(d);
                    }
                }
            }
        }

        out.retain(|d| self.month_ok(d.month()));
        out
    }

    /// Sub-daily (`HOURLY`/`MINUTELY`/`SECONDLY`) candidate datetimes for period `n`. The period base
    /// steps by the frequency unit; date BY-rules limit, and smaller time units expand per the matrix.
    fn subdaily_set(&self, dtstart: PrimitiveDateTime, n: u32) -> Vec<PrimitiveDateTime> {
        let step = i64::from(n) * i64::from(self.interval);
        let base = match self.freq {
            Frequency::Hourly => dtstart + Duration::hours(step),
            Frequency::Minutely => dtstart + Duration::minutes(step),
            Frequency::Secondly => dtstart + Duration::seconds(step),
            _ => unreachable!(),
        };
        if !self.date_limits_ok(base.date()) {
            return Vec::new();
        }
        let bt = base.time();
        if !self.by_hour.is_empty() && !self.by_hour.contains(&bt.hour()) {
            return Vec::new();
        }
        let minutes: Vec<u8> = match self.freq {
            Frequency::Hourly => {
                if self.by_minute.is_empty() {
                    vec![bt.minute()]
                } else {
                    self.by_minute.clone()
                }
            }
            _ => {
                if !self.by_minute.is_empty() && !self.by_minute.contains(&bt.minute()) {
                    return Vec::new();
                }
                vec![bt.minute()]
            }
        };
        let seconds: Vec<u8> = match self.freq {
            Frequency::Secondly => {
                if !self.by_second.is_empty() && !self.by_second.contains(&bt.second()) {
                    return Vec::new();
                }
                vec![bt.second()]
            }
            _ => {
                if self.by_second.is_empty() {
                    vec![bt.second()]
                } else {
                    self.by_second.clone()
                }
            }
        };
        let mut out = Vec::new();
        for m in minutes {
            for &s in &seconds {
                if let Ok(t) = Time::from_hms(bt.hour(), m, s.min(59)) {
                    out.push(PrimitiveDateTime::new(base.date(), t));
                }
            }
        }
        out
    }

    /// Date-level limit rules (`BYMONTH`/`BYMONTHDAY`/`BYYEARDAY`/`BYDAY` weekday) used by the
    /// frequencies for which those rules limit rather than expand.
    fn date_limits_ok(&self, date: Date) -> bool {
        self.month_ok(date.month())
            && (self.by_month_day.is_empty()
                || self
                    .by_month_day
                    .iter()
                    .any(|&md| monthday_to_day(date.year(), date.month(), md) == Some(date.day())))
            && (self.by_year_day.is_empty() || self.year_day_ok(date))
            && (self.by_day.is_empty() || self.by_day.iter().any(|b| b.weekday == date.weekday()))
    }

    fn year_day_ok(&self, date: Date) -> bool {
        let len = days_in_year(date.year()) as i16;
        let ord = date.ordinal() as i16;
        self.by_year_day
            .iter()
            .any(|&yd| yd == ord || yd == ord - len - 1)
    }

    fn month_ok(&self, month: Month) -> bool {
        self.by_month.is_empty() || self.by_month.contains(&u8::from(month))
    }

    /// Resolve a `BYMONTHDAY` value within `(year, month)` to a concrete date, honoring the optional
    /// `BYDAY` weekday limit (when both are present, `BYDAY` limits `BYMONTHDAY`). Returns `None` if the
    /// month-day does not exist or is filtered out by `BYDAY`.
    fn matching_monthday(&self, year: i32, month: Month, md: i8) -> Option<Date> {
        let day = monthday_to_day(year, month, md)?;
        let d = Date::from_calendar_date(year, month, day).ok()?;
        (self.by_day.is_empty() || self.by_day.iter().any(|b| b.weekday == d.weekday()))
            .then_some(d)
    }

    /// Whether period `n` begins strictly after `to` (so all later periods do too).
    fn period_start_after(
        &self,
        dtstart: PrimitiveDateTime,
        n: u32,
        to: PrimitiveDateTime,
    ) -> bool {
        let start = dtstart.date();
        let to_date = to.date();
        match self.freq {
            Frequency::Secondly => {
                dtstart + Duration::seconds(i64::from(n) * i64::from(self.interval)) > to
            }
            Frequency::Minutely => {
                dtstart + Duration::minutes(i64::from(n) * i64::from(self.interval)) > to
            }
            Frequency::Hourly => {
                dtstart + Duration::hours(i64::from(n) * i64::from(self.interval)) > to
            }
            Frequency::Daily => {
                start + Duration::days(i64::from(n) * i64::from(self.interval)) > to_date
            }
            Frequency::Weekly => {
                start_of_week(start, self.wkst)
                    + Duration::weeks(i64::from(n) * i64::from(self.interval))
                    > to_date
            }
            Frequency::Monthly => {
                let (y, m) = add_months(
                    start.year(),
                    u32::from(u8::from(start.month())),
                    n * self.interval,
                );
                first_of_month(y, m as u8) > to_date
            }
            Frequency::Yearly => {
                let y = start.year() + (n * self.interval) as i32;
                Date::from_calendar_date(y, Month::January, 1)
                    .map(|d| d > to_date)
                    .unwrap_or(true)
            }
        }
    }
}

/// A full component recurrence set: an optional `RRULE` (or several) plus explicit `RDATE` additions
/// and `EXDATE` exclusions, anchored at `DTSTART`. Per RFC 5545, `DTSTART` is always the first instance
/// of the set, and `EXDATE` removes matching instances last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecurrenceSet {
    pub dtstart: PrimitiveDateTime,
    pub rrules: Vec<RRule>,
    pub rdates: Vec<PrimitiveDateTime>,
    pub exdates: Vec<PrimitiveDateTime>,
}

impl RecurrenceSet {
    /// A non-recurring single instance at `dtstart`.
    pub fn single(dtstart: PrimitiveDateTime) -> Self {
        RecurrenceSet {
            dtstart,
            rrules: Vec::new(),
            rdates: Vec::new(),
            exdates: Vec::new(),
        }
    }

    /// Expand the whole set into the half-open window `[from, to)`, sorted ascending and de-duplicated.
    pub fn expand(&self, from: PrimitiveDateTime, to: PrimitiveDateTime) -> Vec<PrimitiveDateTime> {
        let mut set: BTreeSet<PrimitiveDateTime> = BTreeSet::new();
        if self.dtstart >= from && self.dtstart < to {
            set.insert(self.dtstart);
        }
        for r in &self.rrules {
            for o in r.expand(self.dtstart, from, to) {
                set.insert(o);
            }
        }
        for &d in &self.rdates {
            if d >= from && d < to {
                set.insert(d);
            }
        }
        for &d in &self.exdates {
            set.remove(&d);
        }
        set.into_iter().collect()
    }
}

fn day_offset(wkst: Weekday, wd: Weekday) -> u8 {
    (wd.number_days_from_monday() + 7 - wkst.number_days_from_monday()) % 7
}

fn start_of_week(d: Date, wkst: Weekday) -> Date {
    d - Duration::days(i64::from(day_offset(wkst, d.weekday())))
}

fn add_months(year: i32, month1: u32, add: u32) -> (i32, u32) {
    let zero = (month1 - 1) + add;
    (year + (zero / 12) as i32, (zero % 12) + 1)
}

fn first_of_month(year: i32, month1: u8) -> Date {
    Date::from_calendar_date(year, Month::try_from(month1).expect("1..=12"), 1)
        .expect("day 1 valid")
}

fn days_in_year(year: i32) -> u16 {
    if time::util::is_leap_year(year) {
        366
    } else {
        365
    }
}

/// The number of ISO-8601 weeks in a year (52 or 53).
fn iso_weeks_in_year(year: i32) -> u8 {
    Date::from_calendar_date(year, Month::December, 28)
        .expect("dec 28 valid")
        .iso_week()
}

fn monthday_to_day(year: i32, month: Month, md: i8) -> Option<u8> {
    let len = month.length(year);
    if md > 0 {
        let d = md as u8;
        (d <= len).then_some(d)
    } else {
        let back = (-md) as u8;
        (back <= len).then(|| len - back + 1)
    }
}

fn weekdays_in_month(year: i32, month: Month, weekday: Weekday) -> Vec<Date> {
    (1..=month.length(year))
        .filter_map(|day| Date::from_calendar_date(year, month, day).ok())
        .filter(|d| d.weekday() == weekday)
        .collect()
}

fn weekdays_in_year(year: i32, weekday: Weekday) -> Vec<Date> {
    (1..=days_in_year(year))
        .filter_map(|o| Date::from_ordinal_date(year, o).ok())
        .filter(|d| d.weekday() == weekday)
        .collect()
}

/// 1-based positive index or negative-from-end index into an ordered list.
fn nth<T: Copy>(list: &[T], ordinal: i8) -> Option<T> {
    if ordinal > 0 {
        list.get((ordinal as usize).checked_sub(1)?).copied()
    } else if ordinal < 0 {
        let from_end = (-ordinal) as usize;
        list.len()
            .checked_sub(from_end)
            .and_then(|i| list.get(i))
            .copied()
    } else {
        None
    }
}

/// Select `BYSETPOS` positions (1-based, negative from end) from a sorted period set.
fn select_set_pos(set: &[PrimitiveDateTime], positions: &[i16]) -> Vec<PrimitiveDateTime> {
    let mut out = Vec::new();
    for &p in positions {
        let idx = if p > 0 {
            (p as usize).checked_sub(1)
        } else {
            set.len().checked_sub((-p) as usize)
        };
        if let Some(&dt) = idx.and_then(|i| set.get(i)) {
            out.push(dt);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    fn dt(y: i32, m: u8, d: u8, hh: u8, mm: u8) -> PrimitiveDateTime {
        PrimitiveDateTime::new(
            Date::from_calendar_date(y, Month::try_from(m).unwrap(), d).unwrap(),
            Time::from_hms(hh, mm, 0).unwrap(),
        )
    }
    fn dts(y: i32, m: u8, d: u8, hh: u8, mm: u8, ss: u8) -> PrimitiveDateTime {
        PrimitiveDateTime::new(
            Date::from_calendar_date(y, Month::try_from(m).unwrap(), d).unwrap(),
            Time::from_hms(hh, mm, ss).unwrap(),
        )
    }
    fn ymd(list: &[PrimitiveDateTime]) -> Vec<(i32, u8, u8)> {
        list.iter()
            .map(|t| (t.year(), u8::from(t.month()), t.day()))
            .collect()
    }
    fn hms(list: &[PrimitiveDateTime]) -> Vec<(u8, u8, u8)> {
        list.iter()
            .map(|t| (t.hour(), t.minute(), t.second()))
            .collect()
    }
    fn wide() -> (PrimitiveDateTime, PrimitiveDateTime) {
        (dt(1970, 1, 1, 0, 0), dt(2100, 1, 1, 0, 0))
    }

    #[test]
    fn daily_count_and_interval() {
        let (f, t) = wide();
        let r = RRule::parse("RRULE:FREQ=DAILY;COUNT=3").unwrap();
        assert_eq!(
            ymd(&r.expand(dt(2024, 1, 1, 9, 0), f, t)),
            [(2024, 1, 1), (2024, 1, 2), (2024, 1, 3)]
        );
        let r = RRule::parse("FREQ=DAILY;INTERVAL=2;COUNT=3").unwrap();
        assert_eq!(
            ymd(&r.expand(dt(2024, 1, 1, 9, 0), f, t)),
            [(2024, 1, 1), (2024, 1, 3), (2024, 1, 5)]
        );
    }

    #[test]
    fn daily_until_inclusive() {
        let (f, t) = wide();
        let r = RRule::parse("FREQ=DAILY;UNTIL=20240103").unwrap();
        assert_eq!(
            ymd(&r.expand(dt(2024, 1, 1, 9, 0), f, t)),
            [(2024, 1, 1), (2024, 1, 2), (2024, 1, 3)]
        );
    }

    #[test]
    fn weekly_byday() {
        let (f, t) = wide();
        let r = RRule::parse("FREQ=WEEKLY;BYDAY=MO,WE,FR;COUNT=5").unwrap();
        assert_eq!(
            ymd(&r.expand(dt(2024, 1, 1, 9, 0), f, t)),
            [
                (2024, 1, 1),
                (2024, 1, 3),
                (2024, 1, 5),
                (2024, 1, 8),
                (2024, 1, 10)
            ]
        );
    }

    #[test]
    fn monthly_bymonthday_and_negative() {
        let (f, t) = wide();
        let r = RRule::parse("FREQ=MONTHLY;BYMONTHDAY=1;COUNT=3").unwrap();
        assert_eq!(
            ymd(&r.expand(dt(2024, 1, 1, 0, 0), f, t)),
            [(2024, 1, 1), (2024, 2, 1), (2024, 3, 1)]
        );
        let r = RRule::parse("FREQ=MONTHLY;BYMONTHDAY=-1;COUNT=3").unwrap();
        assert_eq!(
            ymd(&r.expand(dt(2024, 1, 31, 0, 0), f, t)),
            [(2024, 1, 31), (2024, 2, 29), (2024, 3, 31)]
        );
    }

    #[test]
    fn monthly_byday_ordinal() {
        let (f, t) = wide();
        let r = RRule::parse("FREQ=MONTHLY;BYDAY=2MO;COUNT=3").unwrap();
        assert_eq!(
            ymd(&r.expand(dt(2024, 1, 1, 0, 0), f, t)),
            [(2024, 1, 8), (2024, 2, 12), (2024, 3, 11)]
        );
        let r = RRule::parse("FREQ=MONTHLY;BYDAY=-1FR;COUNT=2").unwrap();
        assert_eq!(
            ymd(&r.expand(dt(2024, 1, 1, 0, 0), f, t)),
            [(2024, 1, 26), (2024, 2, 23)]
        );
    }

    #[test]
    fn yearly_and_window_clip() {
        let r = RRule::parse("FREQ=YEARLY").unwrap();
        let got = r.expand(
            dt(2024, 3, 15, 12, 0),
            dt(2025, 1, 1, 0, 0),
            dt(2027, 1, 1, 0, 0),
        );
        assert_eq!(ymd(&got), [(2025, 3, 15), (2026, 3, 15)]);
    }

    #[test]
    fn monthly_bysetpos_last_workday() {
        let (f, t) = wide();
        // Last weekday (Mon-Fri) of the month.
        let r = RRule::parse("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=3").unwrap();
        assert_eq!(
            ymd(&r.expand(dt(2024, 1, 1, 0, 0), f, t)),
            // Jan 31 (Wed), Feb 29 (Thu), Mar 29 (Fri).
            [(2024, 1, 31), (2024, 2, 29), (2024, 3, 29)]
        );
    }

    #[test]
    fn yearly_byyearday_negative() {
        let (f, t) = wide();
        // Last day of the year and first day.
        let r = RRule::parse("FREQ=YEARLY;BYYEARDAY=1,-1;COUNT=4").unwrap();
        assert_eq!(
            ymd(&r.expand(dt(2024, 1, 1, 0, 0), f, t)),
            [(2024, 1, 1), (2024, 12, 31), (2025, 1, 1), (2025, 12, 31)]
        );
    }

    #[test]
    fn yearly_byweekno_byday() {
        let (f, t) = wide();
        // Monday of ISO week 1 each year.
        let r = RRule::parse("FREQ=YEARLY;BYWEEKNO=1;BYDAY=MO;COUNT=2").unwrap();
        assert_eq!(
            ymd(&r.expand(dt(2024, 1, 1, 0, 0), f, t)),
            // ISO week 1 Monday: 2024-01-01, 2024(next yr) 2025-W01 Monday = 2024-12-30.
            [(2024, 1, 1), (2024, 12, 30)]
        );
    }

    #[test]
    fn hourly_interval() {
        let r = RRule::parse("FREQ=HOURLY;INTERVAL=6;COUNT=4").unwrap();
        let got = r.expand(
            dts(2024, 1, 1, 0, 0, 0),
            dts(1970, 1, 1, 0, 0, 0),
            dts(2100, 1, 1, 0, 0, 0),
        );
        assert_eq!(hms(&got), [(0, 0, 0), (6, 0, 0), (12, 0, 0), (18, 0, 0)]);
        assert_eq!(ymd(&got)[3], (2024, 1, 1));
    }

    #[test]
    fn minutely_byhour_limit() {
        // Every 30 minutes, but only the 9:00 hour: 9:00 and 9:30.
        let r = RRule::parse("FREQ=MINUTELY;INTERVAL=30;BYHOUR=9;COUNT=2").unwrap();
        let got = r.expand(
            dts(2024, 1, 1, 9, 0, 0),
            dts(1970, 1, 1, 0, 0, 0),
            dts(2100, 1, 1, 0, 0, 0),
        );
        assert_eq!(hms(&got), [(9, 0, 0), (9, 30, 0)]);
    }

    #[test]
    fn daily_byhour_expand() {
        let (f, t) = wide();
        // Twice a day at 09:00 and 17:00.
        let r = RRule::parse("FREQ=DAILY;BYHOUR=9,17;COUNT=4").unwrap();
        let got = r.expand(dt(2024, 1, 1, 0, 0), f, t);
        assert_eq!(hms(&got), [(9, 0, 0), (17, 0, 0), (9, 0, 0), (17, 0, 0)]);
        assert_eq!(
            ymd(&got),
            [(2024, 1, 1), (2024, 1, 1), (2024, 1, 2), (2024, 1, 2)]
        );
    }

    #[test]
    fn recurrence_set_rdate_exdate() {
        let f = dt(2024, 1, 1, 0, 0);
        let to = dt(2024, 2, 1, 0, 0);
        let set = RecurrenceSet {
            dtstart: dt(2024, 1, 1, 9, 0),
            rrules: vec![RRule::parse("FREQ=WEEKLY;BYDAY=MO").unwrap()],
            rdates: vec![dt(2024, 1, 10, 9, 0)], // an extra Wednesday
            exdates: vec![dt(2024, 1, 15, 9, 0)], // drop the 2nd Monday
        };
        // Mondays in Jan 2024: 1, 8, 15, 22, 29. Drop 15, add 10.
        assert_eq!(
            ymd(&set.expand(f, to)),
            [
                (2024, 1, 1),
                (2024, 1, 8),
                (2024, 1, 10),
                (2024, 1, 22),
                (2024, 1, 29)
            ]
        );
    }

    #[test]
    fn recurrence_set_single() {
        let set = RecurrenceSet::single(dt(2024, 6, 1, 12, 0));
        assert_eq!(
            ymd(&set.expand(dt(2024, 1, 1, 0, 0), dt(2025, 1, 1, 0, 0))),
            [(2024, 6, 1)]
        );
        // Out of window -> empty.
        assert!(
            set.expand(dt(2024, 7, 1, 0, 0), dt(2025, 1, 1, 0, 0))
                .is_empty()
        );
    }

    #[test]
    fn parse_errors() {
        assert_eq!(RRule::parse(""), Err(Error::Empty));
        assert_eq!(RRule::parse("INTERVAL=2"), Err(Error::MissingFreq));
        assert!(matches!(
            RRule::parse("FREQ=FORTNIGHTLY"),
            Err(Error::UnsupportedFreq(_))
        ));
        assert!(matches!(
            RRule::parse("FREQ=DAILY;BYMONTH=13"),
            Err(Error::InvalidValue { .. })
        ));
        assert!(matches!(
            RRule::parse("FREQ=DAILY;BYSETPOS=0"),
            Err(Error::InvalidValue { .. })
        ));
        assert!(matches!(
            RRule::parse("FREQ=YEARLY;RSCALE=CHINESE"),
            Err(Error::UnknownKey(key)) if key == "RSCALE"
        ));
        assert!(matches!(
            RRule::parse("FREQ=YEARLY;SKIP=FORWARD"),
            Err(Error::UnknownKey(key)) if key == "SKIP"
        ));
    }
}
