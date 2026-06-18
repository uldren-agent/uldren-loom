//! Conformance vectors vendored from RFC 5545 section 3.8.5.3 ("Recurrence Rule") worked examples and
//! the well-known python-dateutil/libical RRULE corpora. Each asserts our owned engine matches the
//! published occurrence set, so a regression in the expand/limit matrix is caught here.

use loom_rrule::RRule;
use time::PrimitiveDateTime;
use time::macros::datetime;

fn ymd(list: &[PrimitiveDateTime]) -> Vec<(i32, u8, u8)> {
    list.iter()
        .map(|t| (t.year(), u8::from(t.month()), t.day()))
        .collect()
}

const LO: PrimitiveDateTime = datetime!(1990-01-01 00:00:00);
const HI: PrimitiveDateTime = datetime!(2100-01-01 00:00:00);

fn occ(rule: &str, dtstart: PrimitiveDateTime) -> Vec<(i32, u8, u8)> {
    ymd(&RRule::parse(rule).unwrap().expand(dtstart, LO, HI))
}

#[test]
fn daily_for_10() {
    // RFC: Daily for 10 occurrences. DTSTART;TZID=...:19970902T090000
    let got = occ("FREQ=DAILY;COUNT=10", datetime!(1997-09-02 09:00:00));
    assert_eq!(got.len(), 10);
    assert_eq!(got[0], (1997, 9, 2));
    assert_eq!(got[9], (1997, 9, 11));
}

#[test]
fn every_other_day_window() {
    // RFC: Every other day, forever - clipped here to a window.
    let r = RRule::parse("FREQ=DAILY;INTERVAL=2").unwrap();
    let got = ymd(&r.expand(
        datetime!(1997-09-02 09:00:00),
        datetime!(1997-09-02 00:00:00),
        datetime!(1997-09-11 00:00:00),
    ));
    assert_eq!(
        got,
        [
            (1997, 9, 2),
            (1997, 9, 4),
            (1997, 9, 6),
            (1997, 9, 8),
            (1997, 9, 10)
        ]
    );
}

#[test]
fn weekly_tu_th_for_5_weeks_bycount() {
    // RFC: Weekly on Tuesday and Thursday for five weeks (COUNT form), WKST=SU.
    let got = occ(
        "FREQ=WEEKLY;COUNT=10;WKST=SU;BYDAY=TU,TH",
        datetime!(1997-09-02 09:00:00),
    );
    assert_eq!(
        got,
        [
            (1997, 9, 2),
            (1997, 9, 4),
            (1997, 9, 9),
            (1997, 9, 11),
            (1997, 9, 16),
            (1997, 9, 18),
            (1997, 9, 23),
            (1997, 9, 25),
            (1997, 9, 30),
            (1997, 10, 2),
        ]
    );
}

#[test]
fn monthly_first_friday_count_10() {
    // RFC: Monthly on the first Friday for 10 occurrences.
    let got = occ(
        "FREQ=MONTHLY;COUNT=10;BYDAY=1FR",
        datetime!(1997-09-05 09:00:00),
    );
    assert_eq!(
        got,
        [
            (1997, 9, 5),
            (1997, 10, 3),
            (1997, 11, 7),
            (1997, 12, 5),
            (1998, 1, 2),
            (1998, 2, 6),
            (1998, 3, 6),
            (1998, 4, 3),
            (1998, 5, 1),
            (1998, 6, 5),
        ]
    );
}

#[test]
fn monthly_every_other_month_first_and_last_sunday() {
    // RFC: Every other month on the first and last Sunday, COUNT=10.
    let got = occ(
        "FREQ=MONTHLY;INTERVAL=2;COUNT=10;BYDAY=1SU,-1SU",
        datetime!(1997-09-07 09:00:00),
    );
    assert_eq!(
        got,
        [
            (1997, 9, 7),
            (1997, 9, 28),
            (1997, 11, 2),
            (1997, 11, 30),
            (1998, 1, 4),
            (1998, 1, 25),
            (1998, 3, 1),
            (1998, 3, 29),
            (1998, 5, 3),
            (1998, 5, 31),
        ]
    );
}

#[test]
fn yearly_us_presidential_election() {
    // RFC: Every 4 years, the first Tuesday after a Monday in November (US election day).
    let got = occ(
        "FREQ=YEARLY;INTERVAL=4;BYMONTH=11;BYDAY=TU;BYMONTHDAY=2,3,4,5,6,7,8",
        datetime!(1996-11-05 09:00:00),
    );
    assert_eq!(got[0], (1996, 11, 5));
    assert_eq!(got[1], (2000, 11, 7));
    assert_eq!(got[2], (2004, 11, 2));
}

#[test]
fn monthly_third_instance_bysetpos() {
    // RFC: The third instance into the month of one of Tuesday, Wednesday, or Thursday, COUNT=3.
    let got = occ(
        "FREQ=MONTHLY;COUNT=3;BYDAY=TU,WE,TH;BYSETPOS=3",
        datetime!(1997-09-04 09:00:00),
    );
    assert_eq!(got, [(1997, 9, 4), (1997, 10, 7), (1997, 11, 6)]);
}

#[test]
fn monthly_second_to_last_weekday_bysetpos() {
    // RFC: The second-to-last weekday of the month.
    let got = occ(
        "FREQ=MONTHLY;COUNT=3;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-2",
        datetime!(1997-09-29 09:00:00),
    );
    // Sep 1997 weekdays end ...29(Mon),30(Tue) -> 2nd to last = 29.
    assert_eq!(got[0], (1997, 9, 29));
    // Oct 1997 ends ...30(Thu),31(Fri) -> 2nd to last = 30.
    assert_eq!(got[1], (1997, 10, 30));
}

#[test]
fn yearly_in_june_and_july() {
    // RFC: Yearly in June and July, COUNT=10 (BYMONTH expands).
    let got = occ(
        "FREQ=YEARLY;COUNT=10;BYMONTH=6,7",
        datetime!(1997-06-10 09:00:00),
    );
    assert_eq!(
        got,
        [
            (1997, 6, 10),
            (1997, 7, 10),
            (1998, 6, 10),
            (1998, 7, 10),
            (1999, 6, 10),
            (1999, 7, 10),
            (2000, 6, 10),
            (2000, 7, 10),
            (2001, 6, 10),
            (2001, 7, 10),
        ]
    );
}

#[test]
fn daily_until_with_interval() {
    // dateutil corpus: every 10 days, 5 occurrences via UNTIL.
    let got = occ(
        "FREQ=DAILY;INTERVAL=10;UNTIL=19971024T000000Z",
        datetime!(1997-09-02 09:00:00),
    );
    assert_eq!(
        got,
        [
            (1997, 9, 2),
            (1997, 9, 12),
            (1997, 9, 22),
            (1997, 10, 2),
            (1997, 10, 12),
            (1997, 10, 22),
        ]
    );
}
