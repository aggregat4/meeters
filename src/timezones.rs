use crate::custom_timezone::CustomTz;
use crate::custom_timezone::FixedTimespan;
use crate::custom_timezone::FixedTimespanSet;
use crate::domain::CalendarError;
use crate::ical_util::find_property;
use crate::ical_util::find_property_value;
use crate::ical_util::properties_to_string;
use crate::ical_util::unescape_string;
use crate::recurrence::Engine;
use chrono::prelude::*;
use chrono_tz::Tz;
use either::Either;
use either::Left;
use either::Right;
use ical::parser::ical::component::IcalCalendar;
use ical::parser::ical::component::IcalTimeZone;
use ical::parser::ical::component::IcalTimeZoneTransition;
use ical::property::Property;
use std::collections::HashMap;

use crate::windows_timezones::*;

fn parse_windows_tzid(tzid: &str) -> Result<Tz, String> {
    match WINDOWS_TZ_TO_CHRONO_TZ.get(tzid) {
        Some(tz) => Ok(*tz),
        None => Err(format!(
            "Timezone string {} does not represent a windows timezone",
            tzid
        )),
    }
}

/// Parses timezone of the form "(UTC+01:00) Amsterdam, Berlin, Bern, Rome, Stockholm, Vienna"
/// by identifying the UTC offset and generating a Tz from that
fn parse_explicit_tzid(tzid: &str) -> Result<Tz, String> {
    match FUCKED_WINDOWS_TZ_TO_CHRONO_TZ.get(tzid) {
        Some(tz) => Ok(*tz),
        None => Err(format!(
            "Timezone string {} does not represent a windows timezone",
            tzid
        )),
    }
}

/// Parses a TZID string as it may occur in an ical event and returns a chrono-tz timezone.
/// This supports the following formats:
/// * If you provide a custom timezone map then that is checked first
/// * If no custom timezones are provided, it will defer to standardized timezone definitions
pub fn parse_tzid<'a>(
    tzid: &str,
    custom_timezones: &'a HashMap<String, CustomTz>,
) -> Result<Either<Tz, &'a CustomTz>, String> {
    match custom_timezones.get(tzid) {
        Some(tz) => Ok(Right(tz)),
        None => Ok(Left(parse_standard_tz(tzid)?)),
    }
}

/// Formats supported:
/// * Explicit timezone strings containing a UTC offset and some cities, e.g. "(UTC+01:00) Amsterdam, Berlin, Bern, Rome, Stockholm, Vienna"
/// * Windows specific timezone identifiers like "W. Europe Standard Time", these are sourced from https://github.com/unicode-org/cldr/blob/master/common/supplemental/windowsZones.xml
/// * IANA Timezone identifiers like "Europe/Berlin" (natively supported by chrono-tz)
fn parse_standard_tz(tzid: &str) -> Result<Tz, String> {
    match tzid.parse() {
        Ok(tz) => Ok(tz),
        Err(_) => match parse_windows_tzid(tzid) {
            Ok(wtz) => Ok(wtz),
            Err(_) => match parse_explicit_tzid(tzid) {
                Ok(etz) => Ok(etz),
                Err(_) => Err(format!("Can't parse tzid {}", tzid)),
            },
        },
    }
}

/// Parses the VTIMEZONEs from the calendar and returns a map from timezone id to CustomTz
#[cfg(test)]
pub fn parse_ical_timezones(
    calendar: &IcalCalendar,
    evaluation_year: i32,
) -> Result<HashMap<String, CustomTz>, CalendarError> {
    parse_ical_timezones_with_engine(calendar, evaluation_year, Engine::Fork)
}

pub(crate) fn parse_ical_timezones_with_engine(
    calendar: &IcalCalendar,
    evaluation_year: i32,
    engine: Engine,
) -> Result<HashMap<String, CustomTz>, CalendarError> {
    calendar
        .timezones
        .iter()
        .map(|vtimezone| parse_ical_timezone(vtimezone, evaluation_year, engine))
        .collect()
}

fn parse_ical_timezone(
    vtimezone: &IcalTimeZone,
    evaluation_year: i32,
    engine: Engine,
) -> Result<(String, CustomTz), CalendarError> {
    match find_property_value(&vtimezone.properties, "TZID") {
        Some(name) => {
            // All string values in the icalendar are escaped so we need to unescape them
            let unescaped_name = unescape_string(&name.to_string());
            let timezone = CustomTz {
                name: unescaped_name.clone(),
                timespanset: parse_timespansets(vtimezone, evaluation_year, engine)?,
            };
            log::debug!(
                "Parsed custom timezone definition '{:?}' with '{:?}' spans",
                timezone.name,
                timezone.timespanset.rest.len()
            );
            Ok((unescaped_name, timezone))
        }
        None => Err(CalendarError {
            msg: "Expecting TZID property for custom timezone".to_string(),
        }),
    }
}

/// Expand local observance onsets (DTSTART, RRULE and RDATE), retaining
/// the evaluation window and the latest preceding onset. UNTIL uses UTC.
fn parse_occurrences_from_timespan(
    properties: &[Property],
    current_year: i32,
    offset_from: i32,
    engine: Engine,
) -> Result<Vec<NaiveDateTime>, CalendarError> {
    let parse_local = |value: &str| {
        NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").map_err(|e| CalendarError {
            msg: format!("Invalid observance date {value:?}: {e}"),
        })
    };
    let start = find_property(properties, "DTSTART").ok_or_else(|| CalendarError {
        msg: "Invalid definition for timespan, missing DTSTART".into(),
    })?;
    let start_value = start.value.as_deref().ok_or_else(|| CalendarError {
        msg: "Missing observance DTSTART value".into(),
    })?;
    let mut dates = vec![parse_local(start_value)?];
    for prop in properties.iter().filter(|p| p.name == "RDATE") {
        let value = prop.value.as_deref().ok_or_else(|| CalendarError {
            msg: "Missing observance RDATE value".into(),
        })?;
        for value in value.split(',') {
            dates.push(parse_local(value)?);
        }
    }
    let maybe_rrule_prop = find_property(properties, "RRULE");
    if maybe_rrule_prop.is_some() {
        // The recurrence engine expands local fields. UNTIL is UTC in a
        // VTIMEZONE, so express it using the same pre-transition offset first.
        let mut rule = maybe_rrule_prop.unwrap().clone();
        let value = rule.value.as_deref().ok_or_else(|| CalendarError {
            msg: "Missing timezone RRULE value".into(),
        })?;
        rule.value = Some(
            value
                .split(';')
                .map(|part| {
                    if let Some(until) = part
                        .strip_prefix("UNTIL=")
                        .and_then(|v| v.strip_suffix('Z'))
                    {
                        let utc =
                            NaiveDateTime::parse_from_str(until, "%Y%m%dT%H%M%S").map_err(|e| {
                                CalendarError {
                                    msg: format!("Invalid timezone UNTIL: {e}"),
                                }
                            })?;
                        let local = utc
                            .checked_add_signed(chrono::Duration::seconds(i64::from(offset_from)))
                            .ok_or_else(|| CalendarError {
                                msg: "Timezone UNTIL out of range".into(),
                            })?;
                        Ok(format!("UNTIL={}", local.format("%Y%m%dT%H%M%S")))
                    } else {
                        Ok(part.to_string())
                    }
                })
                .collect::<Result<Vec<_>, CalendarError>>()?
                .join(";"),
        );
        let rule_props = vec![rule, start.clone()];
        // There is also no EXDATE as far as I can tell from the spec so we don't try to parse it
        let event_as_string = properties_to_string(&rule_props);
        let ruleset = engine.expand(&event_as_string, current_year + 2)?;
        // Keep the last onset before the window as well as onsets inside it.
        // An ended observance still determines the offset until a later change.
        let mut previous = None;
        for dt in ruleset
            .into_iter()
            .take_while(|d| d.year() < current_year + 2)
        {
            if dt.year() < current_year - 2 {
                previous = Some(dt.naive_local());
            } else {
                dates.push(dt.naive_local());
            }
        }
        dates.extend(previous);
    }
    dates.retain(|d| d.year() < current_year + 2);
    dates.sort();
    dates.dedup();
    // Exchange often supplies synthetic DTSTART anchors in 1601 for both
    // observances. Retain only the latest onset before our window so those
    // obsolete anchors cannot conflict with each other.
    let before_window = dates.partition_point(|d| d.year() < current_year - 2);
    dates.drain(..before_window.saturating_sub(1));
    Ok(dates)
}

/// Merge any number of STANDARD/DAYLIGHT observances into a UTC timeline.
/// The first retained onset's TZOFFSETFROM supplies the initial offset.
fn parse_timespansets(
    vtimezone: &IcalTimeZone,
    evaluation_year: i32,
    engine: Engine,
) -> Result<FixedTimespanSet, CalendarError> {
    // convert the ical timezone transitions into our own struct
    let transitions: Vec<TimezoneTransition> = vtimezone
        .transitions
        .iter()
        .map(parse_icaltimezonetransition)
        .collect::<Result<Vec<TimezoneTransition>, CalendarError>>()?; // collect moves the result to the outer scope and doing '?' will fail the operation at the first Err
    if transitions.is_empty() {
        return Err(CalendarError {
            msg: "VTIMEZONE has no observances".into(),
        });
    }
    // generate all timestamps of all transition points for the available timestamps starting at the provided DTSTART times
    let mut transition_points: Vec<TransitionPoint> = vec![];
    for (pos, transition) in transitions.iter().enumerate() {
        match parse_occurrences_from_timespan(&transition.properties, evaluation_year, transition.offsetfrom, engine) {
            Ok(occurrences) => {
                for dt in occurrences {
                    transition_points.push(TransitionPoint {
                        // An observance onset uses the offset in effect before the change.
                        timestamp: dt.and_utc().timestamp() - i64::from(transition.offsetfrom),
                        transition_index: pos,
                    })
                }
            }
            Err(e) => {
                return Err(CalendarError {
                    msg: format!("error in RRULE parsing for timezone transition: {}, this is for ical timezone {:?}", e, vtimezone),
                })
            }
        }
    }
    transition_points.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    // Identical onsets are common in fixed-offset Exchange definitions.
    for pair in transition_points.windows(2) {
        if pair[0].timestamp == pair[1].timestamp
            && transitions[pair[0].transition_index].offsetto
                != transitions[pair[1].transition_index].offsetto
        {
            return Err(CalendarError {
                msg: "Conflicting timezone observances at the same instant".into(),
            });
        }
    }
    transition_points.dedup_by_key(|p| p.timestamp);
    let initial_offset = transition_points
        .first()
        .map(|p| transitions[p.transition_index].offsetfrom)
        .unwrap_or(transitions[0].offsetfrom);
    Ok(FixedTimespanSet {
        first: FixedTimespan {
            utc_offset: initial_offset,
            dst_offset: 0,
            name: "",
        },
        rest: transition_points
            .iter()
            .map(|transition_point| {
                (
                    transition_point.timestamp,
                    FixedTimespan {
                        utc_offset: transitions[transition_point.transition_index].offsetto,
                        dst_offset: 0,
                        name: "", // name is irrelevant here
                    },
                )
            })
            .collect(),
    })
}

struct TransitionPoint {
    timestamp: i64,
    transition_index: usize, // index in the transitions vector to identify the concrete transition
}

struct TimezoneTransition {
    properties: Vec<Property>,
    offsetfrom: i32,
    offsetto: i32,
}

fn parse_icaltimezonetransition(
    transition: &IcalTimeZoneTransition,
) -> Result<TimezoneTransition, CalendarError> {
    Ok(TimezoneTransition {
        properties: transition.properties.to_owned(),
        offsetfrom: offset_to_seconds(
            find_property_value(&transition.properties, "TZOFFSETFROM").ok_or(CalendarError {
                msg: "no TZOFFSETFROM in timezone transition".to_string(),
            })?,
        )?,
        offsetto: offset_to_seconds(
            find_property_value(&transition.properties, "TZOFFSETTO").ok_or(CalendarError {
                msg: "no TZOFFSETTO in timezone transition".to_string(),
            })?,
        )?,
    })
}

/// Parse an iCalendar UTC-OFFSET (+/-HHMM[SS]); the sign covers all fields.
fn offset_to_seconds(offset: String) -> Result<i32, CalendarError> {
    let invalid = || CalendarError {
        msg: format!("Invalid timezone offset: {offset:?}"),
    };
    let bytes = offset.as_bytes();
    if !matches!(bytes.len(), 5 | 7)
        || !matches!(bytes[0], b'+' | b'-')
        || !bytes[1..].iter().all(u8::is_ascii_digit)
    {
        return Err(invalid());
    }
    let pair = |i| i32::from(bytes[i] - b'0') * 10 + i32::from(bytes[i + 1] - b'0');
    let (hours, minutes, seconds) = (pair(1), pair(3), if bytes.len() == 7 { pair(5) } else { 0 });
    if hours > 23 || minutes > 59 || seconds > 59 {
        return Err(invalid());
    }
    let magnitude = hours * 3600 + minutes * 60 + seconds;
    if bytes[0] == b'-' && magnitude == 0 {
        return Err(invalid());
    }
    Ok(if bytes[0] == b'-' {
        -magnitude
    } else {
        magnitude
    })
}

// Example custom timezone by Exchange for Western European Standard Time
// BEGIN:VTIMEZONE
//     TZID:W. Europe Standard Time
//     BEGIN:STANDARD
//     DTSTART:16010101T030000
//     TZOFFSETFROM:+0200
//     TZOFFSETTO:+0100
//     RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=10
//     END:STANDARD
//     BEGIN:DAYLIGHT
//     DTSTART:16010101T020000
//     TZOFFSETFROM:+0100
//     TZOFFSETTO:+0200
//     RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=3
//     END:DAYLIGHT
//     END:VTIMEZONE

/// Example Timezone definition for La Paz Bolivia without daylight savings from Exchange
/// Note the missing RRULE definition
// BEGIN:VCALENDAR
// METHOD:PUBLISH
// PRODID:Microsoft Exchange Server 2010
// VERSION:2.0
// X-WR-CALNAME:Calendar
// BEGIN:VTIMEZONE
// TZID:(UTC-04:00) Georgetown\, La Paz\, Manaus\, San Juan
// BEGIN:STANDARD
// DTSTART:16010101T000000
// TZOFFSETFROM:-0400
// TZOFFSETTO:-0400
// END:STANDARD
// BEGIN:DAYLIGHT
// DTSTART:16010101T000000
// TZOFFSETFROM:-0400
// TZOFFSETTO:-0400
// END:DAYLIGHT
// END:VTIMEZONE
// BEGIN:VTIMEZONE
// TZID:W. Europe Standard Time
// BEGIN:STANDARD
// DTSTART:16010101T030000
// TZOFFSETFROM:+0200
// TZOFFSETTO:+0100
// RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=10
// END:STANDARD
// BEGIN:DAYLIGHT
// DTSTART:16010101T020000
// TZOFFSETFROM:+0100
// TZOFFSETTO:+0200
// RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=3
// END:DAYLIGHT
// END:VTIMEZONE

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Europe::{Berlin, Dublin, Vienna};

    #[test]
    fn signed_offsets_include_minutes_and_optional_seconds() {
        for (input, expected) in [
            ("-0330", -12600),
            ("-0030", -1800),
            ("+0545", 20700),
            ("-033015", -12615),
            ("+000030", 30),
            ("+0000", 0),
        ] {
            assert_eq!(
                offset_to_seconds(input.into()).unwrap(),
                expected,
                "{input}"
            );
        }
        for input in [
            "", "0330", "+2400", "+0060", "+000060", "-0000", "-000000", "+0é0", "+0x30",
        ] {
            assert!(offset_to_seconds(input.into()).is_err(), "{input}");
        }
    }

    #[test]
    fn observance_until_is_inclusive_in_pre_transition_offset() {
        let property = |name: &str, value: &str| Property {
            name: name.into(),
            value: Some(value.into()),
            params: None,
        };
        for (offset, until, before) in [
            (3600, "20240331T010000Z", "20240331T005959Z"),
            (-14400, "20240331T060000Z", "20240331T055959Z"),
        ] {
            for (bound, expected_len) in [(until, 2), (before, 1)] {
                let properties = vec![
                    property("DTSTART", "20230326T020000"),
                    property(
                        "RRULE",
                        &format!("FREQ=YEARLY;BYMONTH=3;BYDAY=-1SU;UNTIL={bound}"),
                    ),
                ];
                let dates =
                    parse_occurrences_from_timespan(&properties, 2024, offset, Engine::Fork)
                        .unwrap();
                assert_eq!(
                    dates,
                    parse_occurrences_from_timespan(&properties, 2024, offset, Engine::Upstream)
                        .unwrap(),
                    "observance UNTIL behavior differs"
                );
                assert_eq!(dates.len(), expected_len, "{offset}: {bound}");
                assert_eq!(dates[0].to_string(), "2023-03-26 02:00:00");
                if expected_len == 2 {
                    assert_eq!(dates[1].to_string(), "2024-03-31 02:00:00");
                }
            }
        }
    }

    #[test]
    fn nonrecurring_observances_use_previous_offset() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VTIMEZONE\nTZID:Test\nBEGIN:DAYLIGHT\nDTSTART:20240331T020000\nTZOFFSETFROM:+0100\nTZOFFSETTO:+0200\nEND:DAYLIGHT\nBEGIN:STANDARD\nDTSTART:20241027T030000\nTZOFFSETFROM:+0200\nTZOFFSETTO:+0100\nEND:STANDARD\nEND:VTIMEZONE\nEND:VCALENDAR\n";
        let calendar = ical::IcalParser::new(text.as_bytes())
            .next()
            .unwrap()
            .unwrap();
        let zones = parse_ical_timezones(&calendar, 2024).unwrap();
        let actual: Vec<_> = zones["Test"]
            .timespanset
            .rest
            .iter()
            .map(|(ts, _)| *ts)
            .collect();
        let expected: Vec<_> = ["2024-03-31T01:00:00Z", "2024-10-27T01:00:00Z"]
            .iter()
            .map(|s| DateTime::parse_from_rfc3339(s).unwrap().timestamp())
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn parses_iana_strings() {
        assert_eq!(Berlin, parse_standard_tz("Europe/Berlin").unwrap());
    }

    #[test]
    fn parses_windows_strings() {
        assert_eq!(
            Berlin,
            parse_standard_tz("W. Europe Standard Time").unwrap()
        );
    }

    #[test]
    fn parses_fucked_windows_strings() {
        assert_eq!(
            Vienna,
            parse_standard_tz("(UTC+01:00) Amsterdam, Berlin, Bern, Rome, Stockholm, Vienna")
                .unwrap()
        );
        assert_eq!(
            Dublin,
            parse_standard_tz("(UTC+00:00) Dublin, Edinburgh, Lisbon, London").unwrap()
        );
    }
}
