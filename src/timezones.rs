use crate::custom_timezone::CustomTz;
use crate::custom_timezone::FixedTimespan;
use crate::custom_timezone::FixedTimespanSet;
use crate::ical_util::find_property;
use crate::ical_util::find_property_value;
use crate::ical_util::properties_to_string;
use crate::CalendarError;
use chrono::prelude::*;
use chrono::DateTime;
use chrono_tz::Tz;
use either::Either;
use either::Left;
use either::Right;
use ical::parser::ical::component::IcalCalendar;
use ical::parser::ical::component::IcalTimeZone;
use ical::parser::ical::component::IcalTimeZoneTransition;
use ical::property::Property;
use rrule::RRuleSet;
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
pub fn parse_ical_timezones(
    calendar: &IcalCalendar,
    local_tz: &Tz,
) -> Result<HashMap<String, CustomTz>, CalendarError> {
    calendar
        .timezones
        .iter()
        .map(|vtimezone| parse_ical_timezone(vtimezone, local_tz))
        .collect()
}

fn parse_ical_timezone(
    vtimezone: &IcalTimeZone,
    local_tz: &Tz,
) -> Result<(String, CustomTz), CalendarError> {
    match find_property_value(&vtimezone.properties, "TZID") {
        Some(name) => {
            let timezone = CustomTz {
                name: name.to_string(),
                timespanset: parse_timespansets(vtimezone, local_tz)?, // pass on the error
            };
            println!(
                "Parsed custom timezone definition '{:?}' with '{:?}' spans",
                timezone.name,
                timezone.timespanset.rest.len()
            );
            Ok((name, timezone))
        }
        None => Err(CalendarError {
            msg: "Expecting TZID property for custom timezone".to_string(),
        }),
    }
}

/// We assume there is no TZ identifier with timespan DTSTARTS
/// This is in the spec. See https://icalendar.org/iCalendar-RFC-5545/3-6-5-time-zone-component.html
/// "DTSTART" in this usage MUST be specified as a date with a local time value."
///
/// Additionally we move the starting year forward to 2 years before now since we are not interested
/// in historical events and typical VTIMEZONE definitions start many hundreds of years in the past.
/// This allows us to generate many fewer timespansets and preserve memory and performance.
fn parse_occurrences_from_timespan(
    properties: &[Property],
    local_tz: &Tz,
) -> Result<Vec<DateTime<Tz>>, CalendarError> {
    let maybe_dtstart_prop = find_property(properties, "DTSTART");
    let maybe_rrule_prop = find_property(properties, "RRULE");
    if maybe_dtstart_prop.is_none() {
        return Err(CalendarError {
            msg: "Invalid definition for timespan, missing DTSTART".to_string(),
        });
    }
    if maybe_rrule_prop.is_some() {
        let rule_props = vec![
            maybe_rrule_prop.unwrap().clone(),
            maybe_dtstart_prop.unwrap().clone(),
        ];
        // There is also no EXDATE as far as I can tell from the spec so we don't try to parse it
        let event_as_string = properties_to_string(&rule_props);
        let current_year = Local::now().year();
        match event_as_string.parse::<RRuleSet>() {
            // We only take occurrences in a short interval around the current year since we are
            // only interested in current dates
            Ok(ruleset) => {
                let relevant_transitions: Vec<DateTime<Tz>> = ruleset
                    .clone()
                    .into_iter()
                    .skip_while(|d| d.year() < (current_year - 2))
                    .take_while(|d| d.year() < (current_year + 2))
                    .collect();
                if relevant_transitions.is_empty() {
                    // There could be a case where there are no transitions around our current year.
                    // For example for a country that dropped daylight savings at some point in the
                    // past. For this case we return simply the last 4 transitions coming right
                    // before the current year
                    let all_transitions_before_now: Vec<DateTime<Tz>> = ruleset
                        .into_iter()
                        .take_while(|d| d.year() < current_year)
                        .collect();
                    Ok(all_transitions_before_now
                        .into_iter()
                        .rev()
                        .take(4)
                        .rev()
                        .collect())
                } else {
                    Ok(relevant_transitions)
                }
            }
            Err(e) => Err(CalendarError {
                msg: format!(
                    "error for string '{:?}' in RRULE parsing: {:?}",
                    event_as_string, e
                ),
            }),
        }
    } else {
        // A timezone definition can also have no RRULE definition.
        // In Exchange this will happen for UTC or other timezones that
        // have no daylight savings. For example UTC-4, La Paz, Bolivia.
        // According to the iCalendar spec this would also be possible
        // for partial timezone definitions that are only valid within
        // a certain period of time, but I am disregarding that use case for now.
        // See also https://icalendar.org/iCalendar-RFC-5545/3-6-5-time-zone-component.html
        let date_time_str = maybe_dtstart_prop.unwrap().value.as_ref().unwrap();
        match NaiveDateTime::parse_from_str(date_time_str, "%Y%m%dT%H%M%S") {
            Ok(dt) => return Ok(vec![local_tz.from_local_datetime(&dt).unwrap()]),
            Err(e) => Err(CalendarError {
                msg: format!(
                    "Could not parse DTSTART for timezone timespan with value {:?} and error: {:?}",
                    date_time_str, e
                ),
            }),
        }
    }
}

///
/// We have two spans: standard and daylight savings. They both have:
/// - a starting date
/// - an RRULE for the transition day
/// - an offset
///
/// We generate a sequence of switching dates for each span from the starting date until the year after this year
/// We figure out what sequence is the initial sequence (earlier in the year) and which is the second sequence
/// We generate a fake timespan for the beginning of time until the first switch day
/// For each element in the EARLY sequence:
///     Add a span for the early element
///     Add a span for the corresponding late element
///
fn parse_timespansets(
    vtimezone: &IcalTimeZone,
    local_tz: &Tz,
) -> Result<FixedTimespanSet, CalendarError> {
    // convert the ical timezone transitions into our own struct
    let transitions: Vec<TimezoneTransition> = vtimezone
        .transitions
        .iter()
        .map(|vtimezone_transition| parse_icaltimezonetransition(vtimezone_transition))
        .collect::<Result<Vec<TimezoneTransition>, CalendarError>>()?; // collect moves the result to the outer scope and doing '?' will fail the operation at the first Err
    assert_eq!(2, transitions.len());
    // generate all timestamps of all transition points for the available timestamps starting at the provided DTSTART times
    let mut transition_points: Vec<TransitionPoint> = vec![];
    for (pos, transition) in transitions.iter().enumerate() {
        match parse_occurrences_from_timespan(&transition.properties, local_tz) {
            Ok(occurrences) => {
                for dt in occurrences {
                    transition_points.push(TransitionPoint {
                        timestamp: dt.timestamp(),
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
    Ok(FixedTimespanSet {
        // This synthetic fake first timespan models the time before the first
        // transition point, these values are fake and should never be used
        // The assumption is that the custom TZ starts defining transitions points
        // at some distant time in the past and that we are fine with this
        first: FixedTimespan {
            utc_offset: 0,
            dst_offset: 0,
            name: "", // name is irrelevant here
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
        ),
        offsetto: offset_to_seconds(
            find_property_value(&transition.properties, "TZOFFSETTO").ok_or(CalendarError {
                msg: "no TZOFFSETTO in timezone transition".to_string(),
            })?,
        ),
    })
}

/// Converts offsets in string form like "+0200" or more generally
/// "+HHMM" to the matching number of seconds.
fn offset_to_seconds(offset: String) -> i32 {
    let mut seconds = 0;
    seconds += offset[..3].parse::<i32>().unwrap() * 3600;
    seconds += offset[3..].parse::<i32>().unwrap() * 60;
    seconds
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
