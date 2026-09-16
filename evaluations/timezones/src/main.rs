#![allow(dead_code)]
#[path = "../../../src/binary_search.rs"]
mod binary_search;
#[path = "../../../src/custom_timezone.rs"]
mod custom_timezone;
#[path = "../../../src/domain.rs"]
mod domain;
#[path = "../../../src/ical_util.rs"]
mod ical_util;
#[path = "../../../src/recurrence.rs"]
mod recurrence;
#[path = "../../../src/timezones.rs"]
mod timezones;
#[path = "../../../src/windows_timezones.rs"]
mod windows_timezones;

use chrono::{DateTime, Datelike, LocalResult, NaiveDateTime, Offset, TimeZone, Timelike};
use custom_timezone::{CustomTz, FixedTimespan, FixedTimespanSet};
use std::{collections::BTreeSet, hint::black_box, time::Instant};

fn offset(span: &FixedTimespan) -> i32 {
    span.utc_offset + span.dst_offset
}
fn expected_offset(z: &CustomTz, ts: i64) -> i32 {
    z.timespanset
        .rest
        .iter()
        .rev()
        .find(|(at, _)| *at <= ts)
        .map(|(_, s)| offset(s))
        .unwrap_or(offset(&z.timespanset.first))
}
fn candidates(z: &CustomTz, local: i64) -> Vec<i64> {
    let mut offsets = BTreeSet::from([offset(&z.timespanset.first)]);
    offsets.extend(z.timespanset.rest.iter().map(|(_, s)| offset(s)));
    let mut result: Vec<_> = offsets
        .into_iter()
        .map(|o| local - i64::from(o))
        .filter(|ts| *ts + i64::from(expected_offset(z, *ts)) == local)
        .collect();
    result.sort();
    result
}
fn local(ts: i64) -> NaiveDateTime {
    DateTime::from_timestamp(ts, 0).unwrap().naive_utc()
}
fn chrono_candidates<T: TimeZone>(tz: &T, dt: NaiveDateTime) -> Vec<i64> {
    match tz.from_local_datetime(&dt) {
        LocalResult::None => vec![],
        LocalResult::Single(d) => vec![d.timestamp()],
        LocalResult::Ambiguous(a, b) => {
            let mut v = vec![a.timestamp(), b.timestamp()];
            v.sort();
            v
        }
    }
}
fn jiff_date(dt: NaiveDateTime) -> jiff::civil::DateTime {
    jiff::civil::DateTime::new(
        dt.year() as i16,
        dt.month() as i8,
        dt.day() as i8,
        dt.hour() as i8,
        dt.minute() as i8,
        dt.second() as i8,
        0,
    )
    .unwrap()
}
fn jiff_candidates(z: &jiff::tz::TimeZone, ts: i64) -> Vec<i64> {
    use jiff::tz::AmbiguousOffset::*;
    let mut result = match z.to_ambiguous_timestamp(jiff_date(local(ts))).offset() {
        Unambiguous { offset } => vec![ts - i64::from(offset.seconds())],
        Gap { .. } => vec![],
        Fold { before, after } => vec![
            ts - i64::from(before.seconds()),
            ts - i64::from(after.seconds()),
        ],
    };
    result.sort();
    result
}
fn tzrs_candidates(z: &tz::TimeZone, ts: i64) -> (Vec<i64>, Option<i64>) {
    use tz::datetime::FoundDateTimeKind::*;
    let d = local(ts);
    let found = tz::DateTime::find(
        d.year(),
        d.month() as u8,
        d.day() as u8,
        d.hour() as u8,
        d.minute() as u8,
        d.second() as u8,
        0,
        z.as_ref(),
    )
    .unwrap();
    let mut values = vec![];
    let mut gap = None;
    for item in found.into_inner() {
        match item {
            Normal(d) => values.push(d.unix_time()),
            Skipped {
                before_transition, ..
            } => gap = Some(ts - i64::from(before_transition.local_time_type().ut_offset())),
        }
    }
    values.sort();
    (values, gap)
}
fn tzrs(z: &CustomTz) -> tz::TimeZone {
    let spans =
        std::iter::once(&z.timespanset.first).chain(z.timespanset.rest.iter().map(|(_, s)| s));
    let types: Vec<_> = spans
        .map(|s| tz::LocalTimeType::new(offset(s), false, Some(b"FIX")).unwrap())
        .collect();
    let transitions = z
        .timespanset
        .rest
        .iter()
        .enumerate()
        .map(|(i, (ts, _))| tz::timezone::Transition::new(*ts, i + 1))
        .collect();
    let tail = tz::timezone::TransitionRule::Fixed(*types.last().unwrap());
    tz::TimeZone::new(transitions, types, vec![], Some(tail)).unwrap()
}
// Evaluation-only TZif v2 encoder. The 64-bit block carries every transition;
// the 32-bit compatibility block has only the initial offset. No future rule.
// Do not promote this minimal encoder without a dedicated format test suite.
fn tzif(z: &CustomTz) -> Vec<u8> {
    fn header(out: &mut Vec<u8>, transitions: usize, types: usize) {
        out.extend(b"TZif2");
        out.extend([0; 15]);
        for n in [0, 0, 0, transitions, types, 4] {
            out.extend((n as u32).to_be_bytes());
        }
    }
    fn ty(out: &mut Vec<u8>, o: i32) {
        out.extend(o.to_be_bytes());
        out.extend([0, 0]);
    }
    let mut out = vec![];
    header(&mut out, 0, 1);
    ty(&mut out, offset(&z.timespanset.first));
    out.extend(b"FIX\0");
    assert!(z.timespanset.rest.len() < 255);
    header(
        &mut out,
        z.timespanset.rest.len(),
        z.timespanset.rest.len() + 1,
    );
    for (ts, _) in &z.timespanset.rest {
        out.extend(ts.to_be_bytes());
    }
    for i in 0..z.timespanset.rest.len() {
        out.push((i + 1) as u8);
    }
    ty(&mut out, offset(&z.timespanset.first));
    for (_, s) in &z.timespanset.rest {
        ty(&mut out, offset(s));
    }
    out.extend(b"FIX\0\n\n");
    out
}
fn check_custom(
    z: &CustomTz,
    calendar: Option<&ical::parser::ical::component::IcalCalendar>,
) -> (usize, usize, usize) {
    let bytes = tzif(z);
    let j = jiff::tz::TimeZone::tzif(&z.name, &bytes).unwrap();
    let t = tzrs(z);
    let f = tzfile::Tz::parse(&z.name, &bytes).unwrap();
    let mut utc = BTreeSet::new();
    let mut wall = BTreeSet::new();
    let mut prev = offset(&z.timespanset.first);
    let mut gaps = vec![];
    for (ts, s) in &z.timespanset.rest {
        let next = offset(s);
        for delta in [-3600, -1, 0, 1, 3600] {
            utc.insert(ts + delta);
        }
        for o in [prev, next] {
            for delta in [-1, 0, 1] {
                wall.insert(ts + i64::from(o) + delta);
            }
        }
        if next > prev {
            gaps.push((ts + i64::from(prev + (next - prev) / 2), prev));
        }
        wall.insert(ts + i64::from(prev + (next - prev) / 2));
        prev = next;
    }
    if let Some(calendar) = calendar {
        for event in &calendar.events {
            for prop in &event.properties {
                if !["DTSTART", "DTEND", "EXDATE", "RECURRENCE-ID", "RDATE"]
                    .contains(&prop.name.as_str())
                {
                    continue;
                }
                let matching = prop.params.as_ref().is_some_and(|params| {
                    params
                        .iter()
                        .any(|(name, values)| name == "TZID" && values.contains(&z.name))
                });
                if let Some(value) = &prop.value {
                    for value in value.split(',') {
                        if matching {
                            if let Ok(dt) = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S") {
                                wall.insert(dt.and_utc().timestamp());
                            }
                        } else if let Ok(dt) =
                            NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ")
                        {
                            utc.insert(dt.and_utc().timestamp());
                        }
                    }
                }
            }
        }
    }
    utc.insert(0);
    wall.insert(0);
    for ts in &utc {
        let expected = expected_offset(z, *ts);
        assert_eq!(
            z.offset_from_utc_datetime(&local(*ts))
                .fix()
                .local_minus_utc(),
            expected
        );
        assert_eq!(
            j.to_offset(jiff::Timestamp::from_second(*ts).unwrap())
                .seconds(),
            expected,
            "jiff {} {ts}",
            z.name
        );
        assert_eq!(
            t.find_local_time_type(*ts).unwrap().ut_offset(),
            expected,
            "tz-rs {} {ts}",
            z.name
        );
        assert_eq!(
            (&f).offset_from_utc_datetime(&local(*ts))
                .fix()
                .local_minus_utc(),
            expected,
            "tzfile {} {ts}",
            z.name
        );
    }
    for ts in &wall {
        let expected = candidates(z, *ts);
        assert_eq!(chrono_candidates(z, local(*ts)), expected);
        assert_eq!(
            jiff_candidates(&j, *ts),
            expected,
            "jiff local {} {ts}",
            z.name
        );
        assert_eq!(
            tzrs_candidates(&t, *ts).0,
            expected,
            "tz-rs local {} {ts}",
            z.name
        );
        assert_eq!(
            chrono_candidates(&&f, local(*ts)),
            expected,
            "tzfile local {} {ts}",
            z.name
        );
        if let Some(first) = expected.first() {
            assert_eq!(
                j.to_ambiguous_timestamp(jiff_date(local(*ts)))
                    .compatible()
                    .unwrap()
                    .as_second(),
                *first
            );
        }
    }
    for (ts, previous) in &gaps {
        let expected = ts - i64::from(*previous);
        assert_eq!(
            j.to_ambiguous_timestamp(jiff_date(local(*ts)))
                .compatible()
                .unwrap()
                .as_second(),
            expected
        );
        assert_eq!(tzrs_candidates(&t, *ts).1, Some(expected));
    }
    (utc.len(), wall.len(), gaps.len())
}
fn bench(z: &CustomTz) {
    let bytes = tzif(z);
    let j = jiff::tz::TimeZone::tzif("Eval", &bytes).unwrap();
    let t = tzrs(z);
    let f = tzfile::Tz::parse("Eval", &bytes).unwrap();
    let ts = 1775000000;
    let dt = local(ts);
    let jt = jiff::Timestamp::from_second(ts).unwrap();
    let n = 100000;
    for (name, run) in [
        (
            "CustomTz",
            Box::new(|| {
                z.offset_from_utc_datetime(black_box(&dt))
                    .fix()
                    .local_minus_utc()
            }) as Box<dyn Fn() -> i32>,
        ),
        ("jiff", Box::new(|| j.to_offset(black_box(jt)).seconds())),
        (
            "tz-rs",
            Box::new(|| t.find_local_time_type(black_box(ts)).unwrap().ut_offset()),
        ),
        (
            "tzfile",
            Box::new(|| {
                (&f).offset_from_utc_datetime(black_box(&dt))
                    .fix()
                    .local_minus_utc()
            }),
        ),
    ] {
        let start = Instant::now();
        for _ in 0..n {
            black_box(run());
        }
        println!(
            "offset lookup {name}: {} ns/op (single release run, {n} iterations)",
            start.elapsed().as_nanos() / n
        );
    }
}
fn iana() {
    println!(
        "chrono-tz 0.10.4 embedded tzdb: {}",
        chrono_tz_new::IANA_TZDB_VERSION
    );
    let dates = [
        "2024-01-15T12:00:00Z",
        "2024-07-15T12:00:00Z",
        "2026-07-15T12:00:00Z",
        "2050-07-15T12:00:00Z",
    ];
    let mut count = 0;
    for name in [
        "Europe/Berlin",
        "America/New_York",
        "Australia/Lord_Howe",
        "Pacific/Apia",
        "Asia/Gaza",
        "America/Mexico_City",
        "Asia/Almaty",
        "Africa/Cairo",
    ] {
        let data = std::fs::read(format!("/usr/share/zoneinfo/{name}")).unwrap();
        let j = jiff::tz::TimeZone::tzif(name, &data).unwrap();
        let t = tz::TimeZone::from_tz_data(&data).unwrap();
        let f = tzfile::Tz::parse(name, &data).unwrap();
        let old: chrono_tz::Tz = name.parse().unwrap();
        let new: chrono_tz_new::Tz = name.parse().unwrap();
        for date in dates {
            let dt = DateTime::parse_from_rfc3339(date).unwrap();
            let ts = dt.timestamp();
            let expected = j
                .to_offset(jiff::Timestamp::from_second(ts).unwrap())
                .seconds();
            assert_eq!(t.find_local_time_type(ts).unwrap().ut_offset(), expected);
            for (lib, got) in [
                (
                    "chrono-tz-0.5.3",
                    old.offset_from_utc_datetime(&dt.naive_utc())
                        .fix()
                        .local_minus_utc(),
                ),
                (
                    "chrono-tz-0.10.4",
                    new.offset_from_utc_datetime(&dt.naive_utc())
                        .fix()
                        .local_minus_utc(),
                ),
                (
                    "tzfile",
                    (&f).offset_from_utc_datetime(&dt.naive_utc())
                        .fix()
                        .local_minus_utc(),
                ),
            ] {
                if got != expected {
                    println!(
                        "IANA difference: {lib} {name} {date}: {got} vs system TZif {expected}"
                    );
                    count += 1;
                }
            }
        }
    }
    println!("IANA: 32 instants, tz-rs agrees with Jiff on identical system TZif; {count} other differences");
}

fn main() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/calendars");
    let mut paths: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "ics"))
        .collect();
    paths.sort();
    let mut totals = (0, 0, 0);
    let mut zones = 0;
    let mut benchmark = None;
    for path in paths {
        let year = if path.file_name().unwrap() == "exchange_sanitized.ics" {
            2026
        } else {
            2024
        };
        let text = std::fs::read(&path).unwrap();
        let cal = ical::IcalParser::new(text.as_slice())
            .next()
            .unwrap()
            .unwrap();
        for zone in
            timezones::parse_ical_timezones_with_engine(&cal, year, recurrence::Engine::Upstream)
                .unwrap()
                .values()
        {
            let (u, l, g) = check_custom(zone, Some(&cal));
            totals.0 += u;
            totals.1 += l;
            totals.2 += g;
            zones += 1;
            if path.file_name().unwrap() == "exchange_sanitized.ics" {
                benchmark = Some(zone.clone());
            }
        }
    }
    // Explicit half-hour gap/fold and negative fractional offsets, independent
    // of the VTIMEZONE compiler and the IANA database.
    let span = |o| FixedTimespan {
        utc_offset: o,
        dst_offset: 0,
        name: "FIX",
    };
    let synthetic = CustomTz {
        name: "Synthetic".into(),
        timespanset: FixedTimespanSet {
            first: span(-12600),
            rest: vec![(1700000000, span(-10800)), (1710000000, span(-12600))],
        },
    };
    let (u, l, g) = check_custom(&synthetic, None);
    totals.0 += u;
    totals.1 += l;
    totals.2 += g;
    zones += 1;
    println!("CUSTOM PASS: {zones} zones, {} UTC samples, {} local candidate samples, {} gap-policy samples; CustomTz/Jiff/tz-rs/tzfile agree",totals.0,totals.1,totals.2);
    iana();
    bench(&benchmark.unwrap());
}
