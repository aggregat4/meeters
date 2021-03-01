use chrono::prelude::*;
use chrono::Duration;
use chrono_tz::{Tz, UTC};
use ical::parser::ical::component::{IcalCalendar, IcalEvent};
use ical::property::Property;
use lazy_static::lazy_static;
use regex::Regex;
use rrule::RRuleSet;
use std::collections::HashMap;

use crate::chrono_ical::*;
use crate::domain::*;

fn default_tz(_: dotenv::Error) -> Result<String, dotenv::Error> {
    Ok("Europe/Berlin".to_string())
}

lazy_static! {
    static ref LOCAL_TZ_IANA: String = dotenv::var("MEETERS_LOCAL_TIMEZONE")
        .or_else(default_tz)
        .unwrap();
    static ref LOCAL_TZ: Tz = LOCAL_TZ_IANA
        .parse()
        .expect("Expecting to be able to parse the local timezone, instead got an error");
}

fn find_property_value(properties: &[Property], name: &str) -> Option<String> {
    for property in properties {
        if property.name == name {
            // obviously this clone works but I don't like it, as_ref() didn't seem to do it
            // still do not understand the semantics I should be using here
            return property.value.clone();
        }
    }
    None
}

fn find_property<'a>(properties: &'a [Property], name: &str) -> Option<&'a Property> {
    for property in properties {
        if property.name == name {
            return Some(property);
        }
    }
    None
}

fn find_param<'a>(params: &'a [(String, Vec<String>)], name: &str) -> Option<&'a [String]> {
    for param in params {
        let (param_name, values) = param;
        if param_name == name {
            return Some(values);
        }
    }
    None
}

/// Parses datetimes of the format 'YYYYMMDDTHHMMSS'
///
/// See <https://tools.ietf.org/html/rfc5545#section-3.3.5>
fn parse_ical_datetime(
    datetime: &str,
    tz: &Tz,
    target_tz: &Tz,
) -> Result<DateTime<Tz>, CalendarError> {
    match NaiveDateTime::parse_from_str(&datetime, "%Y%m%dT%H%M%S") {
        Ok(d) => {
            let converted = tz.from_local_datetime(&d).unwrap().with_timezone(target_tz);
            // println!(
            //     "Converting timezones between {} and {}, which means {} to {}",
            //     tz, target_tz, datetime, converted
            // );
            Ok(converted)
        }
        Err(_) => Err(CalendarError {
            msg: "Can't parse datetime string with tzid".to_string(),
        }),
    }
}

/// If a property is a timestamp it can have 3 forms
///
/// See <https://tools.ietf.org/html/rfc5545#section-3.3.5>
fn extract_ical_datetime(prop: &Property) -> Result<DateTime<Tz>, CalendarError> {
    let date_time_str = prop.value.as_ref().unwrap();
    if prop.params.is_some() && find_param(prop.params.as_ref().unwrap(), "TZID").is_some() {
        // timestamp with an explicit timezone: YYYYMMDDTHHMMSS
        // We are assuming there is only one value in the TZID param
        let tzid = &find_param(prop.params.as_ref().unwrap(), "TZID").unwrap()[0];
        // println!("We have a TZID: {}", tzid);
        match parse_tzid(tzid) {
            Ok(timezone) => parse_ical_datetime(&date_time_str, &timezone, &LOCAL_TZ),
            // in case we can't parse the timezone ID we just default to local, also not optimal
            Err(_) => {
                // println!("We have an error parsing the source tzid");
                parse_ical_datetime(&date_time_str, &LOCAL_TZ, &LOCAL_TZ)
            }
        }
    } else {
        // It is either
        //  - a datetime with no timezone: 20201102T235401
        //  - a datetime with in UTC:      20201102T235401Z
        if date_time_str.ends_with('Z') {
            // println!("We assume UTC because of Z");
            parse_ical_datetime(&date_time_str, &UTC, &LOCAL_TZ)
        } else {
            // println!("We use the local timezone as the originating timezone");
            parse_ical_datetime(&date_time_str, &LOCAL_TZ, &LOCAL_TZ)
        }
    }
}

/// Parses an ical date with no timezone information into a chrono Local date, assuming that it is in the
/// local timezone. (This is probably wrong)
///
/// See <https://tools.ietf.org/html/rfc5545#section-3.3.4>
fn parse_ical_date_notz(date: &str, tz: &Tz) -> Result<DateTime<Tz>, CalendarError> {
    // TODO: What time is assumed here by chrono and does that match the ical spec?
    match NaiveDate::parse_from_str(date, "%Y%m%d") {
        Ok(d) => Ok(tz.from_local_datetime(&d.and_hms(0, 0, 0)).unwrap()),
        Err(chrono_err) => Err(CalendarError {
            msg: format!(
                "Can't parse date '{:?}' with cause: {:?}",
                date,
                chrono_err.to_string()
            ),
        }),
    }
}

fn extract_ical_date(prop: &Property) -> Result<DateTime<Tz>, CalendarError> {
    parse_ical_date_notz(&prop.value.as_ref().unwrap(), &LOCAL_TZ)
}

fn extract_start_end_time(
    ical_event: &IcalEvent,
) -> Result<(DateTime<Tz>, DateTime<Tz>, bool), CalendarError> {
    // we assume that DTSTART is mandatory, the spec sort of says that but also mentions something called
    // a "METHOD", ignoring that
    // TODO: do manual TZ detection and map to the correct one instead of defaulting to Berlin
    let start_property = find_property(&ical_event.properties, "DTSTART").unwrap();
    let end_property = find_property(&ical_event.properties, "DTEND");
    // The start property can be a "date":
    //    in this case it has a param called VALUE with the value DATE
    // The start property can also be a "date-time":
    //    in this case it is a YYYYMMDDTHHMMSS string with optionally Z at the end for zulu time
    //    in the date-time case there is an (optional?) TZID param that specific the timezone as a string
    if start_property.params.is_some()
        && find_param(start_property.params.as_ref().unwrap(), "VALUE").is_some()
    {
        // println!("Have a basic date without timezone datetime");
        // the first real value of the VALUE param should be "DATE"
        let value_param = &find_param(start_property.params.as_ref().unwrap(), "VALUE").unwrap()[0];
        if value_param != "DATE" {
            return Err(CalendarError{ msg: format!("Encountered DTSTART with a VALUE parameter that has a value different from 'DATE': {}", value_param) });
        }
        // start property is a "DATE", which indicates a whole day or multi day event
        // see https://tools.ietf.org/html/rfc5545#section-3.6.1 and specifically the discussion on DTSTART
        let start_time = extract_ical_date(start_property)?;
        match end_property {
            Some(p) => extract_ical_date(p).map(|end_time| (start_time, end_time, true)),
            None => Ok((start_time, start_time.clone(), true)),
        }
    } else {
        // println!("Have a 'real' datetime");
        // not a whole day event, so real times, there should be an end time
        match end_property {
            Some(p) => {
                let start_time = extract_ical_datetime(start_property)?;
                let end_time = extract_ical_datetime(p)?;
                Ok((start_time, end_time, false))
            }
            None => Err(CalendarError {
                msg: "missing end time for an event".to_string(),
            }),
        }
    }
}

fn parse_zoom_url(text: &str) -> Option<String> {
    lazy_static! {
        static ref ZOOM_URL_REGEX: regex::Regex =
            Regex::new(r"https?://[^\s]*zoom.us/(j|my)/[^\s\n\r<>]+").unwrap();
    }
    ZOOM_URL_REGEX
        .find(text)
        .map(|mat| mat.as_str().to_string())
}

fn sanitise_string(input: &str) -> String {
    input
        .replace("\\n", "\n")
        .replace("\\r", "\r")
        .replace("\\t", "\t")
        .replace("\\,", ",")
        .replace("\\'", "'")
}

// See https://tools.ietf.org/html/rfc5545#section-3.6.1
fn parse_event(ical_event: &IcalEvent) -> Result<Event, CalendarError> {
    let summary = sanitise_string(
        &find_property_value(&ical_event.properties, "SUMMARY").unwrap_or_else(|| "".to_string()),
    );
    let description = sanitise_string(
        &find_property_value(&ical_event.properties, "DESCRIPTION")
            .unwrap_or_else(|| "".to_string()),
    );
    let location = sanitise_string(
        &find_property_value(&ical_event.properties, "LOCATION").unwrap_or_else(|| "".to_string()),
    );
    // println!("Parsing event '{}'", summary);
    let (start_timestamp, end_timestamp, all_day) = extract_start_end_time(&ical_event)?; // ? short circuits the error
    let meeturl = parse_zoom_url(&location)
        .or_else(|| parse_zoom_url(&summary))
        .or_else(|| parse_zoom_url(&description));
    Ok(Event {
        summary,
        description,
        location,
        meeturl,
        all_day,
        start_timestamp,
        end_timestamp,
    })
}

fn parse_occurrences(event: &IcalEvent) -> Result<Vec<DateTime<Tz>>, CalendarError> {
    // We need to compensate for some weaknesses in the rrule library
    // by sanitising some date constructs and filtering out spurious ical fields.
    let mut filtered_tzid: Option<String> = None;
    let rrule_props = event
        .properties
        .iter()
        .filter(|p| p.name == "DTSTART" || p.name == "RRULE" || p.name == "EXDATE") // || p.name == "DTEND"
        .map(|p| {
            if p.name == "DTSTART" || p.name == "EXDATE" {
                Property {
                    name: p.name.clone(),
                    params: match &p.params {
                        None => None,
                        // this is cleanup: rrule can not deal with explicit long TZID timezone
                        // identifiers and we just remove it here, this means that all the dates
                        // are in the wrong timezone though...
                        // first get the TZID parameter, map to real timezone then rewrite the date
                        // value to a local date in the correct timezone (for example if DTSTART
                        // is UTC, just read and convert to local TZ then write back that value here)
                        Some(params) => Some(
                            params
                                .iter()
                                .filter(|param| {
                                    if param.0 != "TZID" {
                                        true
                                    } else {
                                        filtered_tzid = Some(param.1[0].clone());
                                        false
                                    }
                                })
                                .cloned()
                                .collect(),
                        ),
                    },
                    value: p.value.clone(),
                }
            } else {
                p.clone()
            }
        })
        .collect::<Vec<Property>>();
    let event_as_string = properties_to_string(&rrule_props);
    // println!("Parsing event {:?}", event);
    match event_as_string.parse::<RRuleSet>() {
        Ok(ruleset) => Ok(ruleset
            .all()
            .iter()
            .map(|dt| {
                // rrule does not understand TZ strings and we strip those beforehand
                // all these dates are UTC and we hard convert them to the local timezone
                // just so we can work with this. This needs to be fixed of course.
                // Also converting by going to string and back since I can't deal with chrono correctly apparently
                let real_timezone: Tz = if let Some(original_tzid) = filtered_tzid.clone() {
                    match parse_tzid(&original_tzid) {
                        Ok(tz) => tz,
                        Err(_) => *LOCAL_TZ,
                    }
                } else {
                    *LOCAL_TZ
                };
                // need to do this silly conversion as otherwise the with_timezone call below doesn't work correctly
                let local_tz = *LOCAL_TZ;
                // println!("Real timezone for event {:?} is {}", event, real_timezone);
                let original_datetime = &NaiveDateTime::parse_from_str(
                    &*format!("{}", dt.format("%Y%m%dT%H%M%S")),
                    "%Y%m%dT%H%M%S",
                )
                .unwrap();
                real_timezone
                    .from_local_datetime(original_datetime)
                    .unwrap()
                    .with_timezone(&local_tz)
            })
            .collect()),
        Err(e) => Err(CalendarError {
            msg: format!("error in RRULE parsing: {}", e),
        }),
    }
}

/// Partitions the events into those that are modifying (i.e. they modify event instances of
/// recurring events) and non modifying events which are the base events we need to process without
/// all the modifying events.
fn find_modifying_events(
    events: &[(IcalEvent, Event)],
) -> (HashMap<String, (IcalEvent, Event)>, Vec<(IcalEvent, Event)>) {
    // Create a map of all modifying events so we can correct recurring ocurrences later
    let mut modifying_events: HashMap<String, (IcalEvent, Event)> = HashMap::new();
    let mut non_modifying_events = Vec::new();
    for (ical_event, event) in events {
        // presence of a RECURRENCE-ID property is the trigger to know this is a modifying event
        if let Some(recurrence_id_property) = find_property(&ical_event.properties, "RECURRENCE-ID")
        {
            match extract_ical_datetime(&recurrence_id_property) {
                Ok(_) => {
                    find_property_value(&ical_event.properties, "UID").and_then(|uid| {
                        modifying_events.insert(uid, (ical_event.clone(), event.clone()))
                    });
                }
                Err(e) => println!("Can't parse a recurrence id as datetime: {:?}", e),
            }
        } else {
            non_modifying_events.push((ical_event.clone(), event.clone()));
        }
    }
    (modifying_events, non_modifying_events)
}

fn parse_calendar(text: &str) -> Result<Option<IcalCalendar>, CalendarError> {
    let mut reader = ical::IcalParser::new(text.as_bytes());
    match reader.next() {
        Some(result) => match result {
            Ok(calendar) => Ok(Some(calendar)),
            Err(e) => Err(CalendarError {
                msg: format!("error in ical parsing: {:?}", e),
            }),
        },
        None => Ok(None),
    }
}

fn parse_events(calendar: IcalCalendar) -> Result<Vec<(IcalEvent, Event)>, CalendarError> {
    calendar
        .events
        .into_iter()
        .map(|event| match parse_event(&event) {
            Ok(parsed_event) => Ok((event, parsed_event)),
            Err(e) => Err(e),
        })
        .collect::<Result<Vec<(IcalEvent, Event)>, CalendarError>>() // will fail on the first parse error and return an error
}

pub fn extract_events(text: &str) -> Result<Vec<Event>, CalendarError> {
    let tz: String = LOCAL_TZ_IANA.clone();
    println!("Local Timezone configured as {}", tz);
    match parse_calendar(text)? {
        Some(calendar) => {
            let event_tuples = parse_events(calendar)?;
            // we need to filter out the modifying events from the non modifying events as we would
            // otherwise get duplicates
            let (modifying_events, non_modifying_events) = find_modifying_events(&event_tuples);
            // Calculate occurrences for repeating events
            non_modifying_events
                .into_iter()
                .map(
                    |(ical_event, parsed_event)| match parse_occurrences(&ical_event) {
                        Ok(occurrences) => {
                            if occurrences.is_empty() {
                                Ok(vec![parsed_event])
                            } else {
                                Ok(occurrences
                                    .into_iter()
                                    .map(|datetime| {
                                        // We need to figure out whether the occurrence can be used as such or whether it was changed by a modifying event
                                        // We assume that each ical_event that is a recurring event has a UID, otherwise the unwrap will fail here.
                                        // Needs more error handling?
                                        let occurrence_uid =
                                            find_property_value(&ical_event.properties, "UID")
                                                .unwrap();
                                        if modifying_events.contains_key(&occurrence_uid) {
                                            let (modifying_ical_event, modifying_event) =
                                                modifying_events.get(&occurrence_uid).unwrap();
                                            // since these modifying events are constructed before and are assumed to have an occurence-id we just unwrap here
                                            let recurrence_id_property = find_property(
                                                &modifying_ical_event.properties,
                                                "RECURRENCE-ID",
                                            )
                                            .unwrap();
                                            // println!(
                                            //     "Calculating start and end for recurrence event {}",
                                            //     parsed_event.summary
                                            // );
                                            let recurrence_datetime =
                                                extract_ical_datetime(recurrence_id_property)
                                                    .unwrap();
                                            if datetime == recurrence_datetime {
                                                return modifying_event.clone();
                                            }
                                        }
                                        // we need to calculate this occurrence's end time by adding the duration of the original event to this particular start time
                                        let end_time = datetime
                                            + Duration::seconds(
                                                parsed_event.end_timestamp.timestamp()
                                                    - parsed_event.start_timestamp.timestamp(),
                                            );
                                        Event {
                                            summary: parsed_event.summary.to_string(),
                                            description: parsed_event.description.to_string(),
                                            location: parsed_event.location.to_string(),
                                            meeturl: parsed_event.meeturl.clone(),
                                            all_day: parsed_event.all_day,
                                            start_timestamp: datetime,
                                            end_timestamp: end_time,
                                        }
                                    })
                                    .collect())
                            }
                        }
                        Err(e) => Err(e),
                    },
                )
                .collect::<Result<Vec<Vec<Event>>, CalendarError>>()
                .map(|event_instances| {
                    event_instances.into_iter().flatten().collect() // flatmap that shit
                })
        }
        None => Ok(vec![]),
    }
}

fn format_param_values(param_values: &[String]) -> String {
    param_values
        .iter()
        .map(|param_val| {
            if param_val.contains(' ') {
                format!("\"{}\"", param_val)
            } else {
                param_val.to_string()
            }
        })
        .collect::<Vec<String>>()
        .join(",")
}

fn params_to_string(params: &[(String, Vec<String>)]) -> String {
    if params.is_empty() {
        "".to_string()
    } else {
        return format!(
            ";{}",
            params
                .iter()
                .map(|param| format!("{}={}", param.0, format_param_values(&param.1)))
                .collect::<Vec<String>>()
                .join(",")
        );
    }
}

fn prop_to_string(prop: &Property) -> String {
    return format!(
        "{}{}:{}",
        prop.name,
        params_to_string(&prop.params.as_ref().unwrap_or(&vec![])),
        prop.value.as_ref().unwrap_or(&"".to_string())
    );
}

fn properties_to_string(properties: &[Property]) -> String {
    properties
        .iter() // "interesting" note here: i was getting an E0507 when using into_iter since that apparenty takes ownership. and iter is just return refs
        .map(|p| prop_to_string(&p))
        .collect::<Vec<String>>()
        .join("\n")
}

#[allow(dead_code)]
fn ical_event_to_string(event: &IcalEvent) -> String {
    properties_to_string(&event.properties)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ical::parser::Component;

    #[test]
    fn ical_to_string_empty_ical_event() {
        assert_eq!("", ical_event_to_string(&IcalEvent::new()));
    }

    #[test]
    fn ical_to_string_one_prop_with_value() {
        let mut event = IcalEvent::new();
        let mut prop = Property::new();
        prop.name = "DESCRIPTION".to_string();
        prop.value = Some("foobar".to_string());
        event.add_property(prop);
        assert_eq!("DESCRIPTION:foobar", ical_event_to_string(&event));
    }

    #[test]
    fn ical_to_string_one_prop_with_no_value() {
        let mut event = IcalEvent::new();
        let mut prop = Property::new();
        prop.name = "DESCRIPTION".to_string();
        event.add_property(prop);
        assert_eq!("DESCRIPTION:", ical_event_to_string(&event));
    }

    #[test]
    fn ical_to_string_two_props() {
        let mut event = IcalEvent::new();
        let mut prop = Property::new();
        prop.name = "FOO".to_string();
        prop.value = Some("bar".to_string());
        event.add_property(prop);

        prop = Property::new();
        prop.name = "baz".to_string();
        prop.value = Some("qux".to_string());
        event.add_property(prop);

        assert_eq!("FOO:bar\nbaz:qux", ical_event_to_string(&event));
    }

    // Fixed: https://github.com/fmeringdal/rust_rrule/issues/2
    #[test]
    fn rruleset_parsing_date() {
        "DTSTART;VALUE=DATE:20200812\nRRULE:FREQ=WEEKLY;UNTIL=20210511T220000Z;INTERVAL=1;BYDAY=WE;WKST=MO".parse::<RRuleSet>().unwrap();
    }

    // New feature request: https://github.com/fmeringdal/rust_rrule/issues/3
    // #[test]
    // fn rruleset_parsing_date_with_timezone() {
    //     "DTSTART;TZID=\"(UTC+01:00) Amsterdam, Berlin, Bern, Rome, Stockholm, Vienna\":20201111T160000\nRRULE:FREQ=WEEKLY;UNTIL=20210428T140000Z;INTERVAL=6;BYDAY=WE;WKST=MO".parse::<RRuleSet>().unwrap();
    // }

    // https://github.com/fmeringdal/rust_rrule/issues/5
    #[test]
    fn rruleset_monthly_first_wednesday() {
        println!("{:?}", "DTSTART;VALUE=DATE:20200701\nRRULE:FREQ=MONTHLY;UNTIL=20210303T090000Z;INTERVAL=1;BYDAY=1WE".parse::<RRuleSet>().unwrap().all());
    }

    #[test]
    fn rrule_all_fails_with_panic() {
        "DTSTART;VALUE=DATE:20201230T130000\nRRULE:FREQ=MONTHLY;UNTIL=20210825T120000Z;INTERVAL=1;BYDAY=-1WE".parse::<RRuleSet>().unwrap().all();
    }
}
