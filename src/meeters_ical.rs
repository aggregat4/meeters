use crate::custom_timezone::CustomTz;
use crate::ical_util::unescape_string;
use crate::timezones::parse_ical_timezones;
use crate::timezones::parse_tzid;
use chrono::prelude::*;
use chrono::Duration;
use chrono_tz::{Tz, UTC};
use either::{Either, Left};
use ical::parser::ical::component::{IcalCalendar, IcalEvent};
use ical::property::Property;
use lazy_static::lazy_static;
use regex::Regex;
use rrule::RRuleSet;
use std::collections::HashMap;
use std::collections::HashSet;

use crate::domain::*;
use crate::ical_util::{
    find_param, find_property, find_property_value, is_ical_date, properties_to_string,
};
use multimap::MultiMap;

/// Parses datetimes of the format 'YYYYMMDDTHHMMSS'
///
/// See <https://tools.ietf.org/html/rfc5545#section-3.3.5>
fn parse_ical_datetime<T: TimeZone>(
    datetime: &str,
    tz: &Either<Tz, &CustomTz>,
    target_tz: &T,
) -> Result<DateTime<T>, CalendarError> {
    match NaiveDateTime::parse_from_str(datetime, "%Y%m%dT%H%M%S") {
        Ok(d) => {
            if tz.is_left() {
                Ok(tz
                    .left()
                    .unwrap()
                    .from_local_datetime(&d)
                    .unwrap()
                    .with_timezone(target_tz))
            } else {
                Ok(tz
                    .right()
                    .unwrap()
                    .from_local_datetime(&d)
                    .unwrap()
                    .with_timezone(target_tz))
            }
            // println!(
            //     "Converting timezones between {} and {}, which means {} to {}",
            //     tz, target_tz, datetime, converted
            // );
        }
        Err(_) => Err(CalendarError {
            msg: "Can't parse datetime string with tzid".to_string(),
        }),
    }
}

/// If a property is a timestamp it can have 3 forms:
/// - a timestamp with an explicit timezone identifier (e.g. 20201102T235401 + "Europe/Berlin")
/// - a timestamp with no timezone specified (e.g. 20201102T235401)
/// - a timestamp in zulu time (UTC) (e.g. 20201102T235401Z)
///
/// See <https://tools.ietf.org/html/rfc5545#section-3.3.5>
fn extract_ical_datetime(
    prop: &Property,
    calendar_timezones: &HashMap<String, CustomTz>,
    local_tz: &Tz,
) -> Result<DateTime<Tz>, CalendarError> {
    let date_time_str = prop.value.as_ref().unwrap();
    if prop.params.is_some() && find_param(prop.params.as_ref().unwrap(), "TZID").is_some() {
        // timestamp with an explicit timezone: YYYYMMDDTHHMMSS
        // We are assuming there is only one value in the TZID param
        let tzid = unescape_string(&find_param(prop.params.as_ref().unwrap(), "TZID").unwrap()[0]);
        // println!("We have a TZID: {}", tzid);
        match parse_tzid(&tzid, calendar_timezones) {
            Ok(timezone) => parse_ical_datetime(date_time_str, &timezone, local_tz),
            // in case we can't parse the timezone ID we just default to local, also not optimal
            Err(_) => {
                // println!("We have an error parsing the source tzid");
                parse_ical_datetime(date_time_str, &Left(*local_tz), local_tz)
            }
        }
    } else {
        // It is either
        //  - a datetime with no timezone: 20201102T235401
        //  - a datetime with in UTC:      20201102T235401Z
        if date_time_str.ends_with('Z') {
            // println!("We assume UTC because of Z");
            parse_ical_datetime(
                date_time_str.strip_suffix('Z').unwrap(),
                &Left(UTC),
                local_tz,
            )
        } else {
            // println!("We use the local timezone as the originating timezone");
            parse_ical_datetime(date_time_str, &Left(*local_tz), local_tz)
        }
    }
}

/// Parses an ical date of the form YYYYMMDD with no timezone information into a datetime
/// with the provided timezone.
///
/// See <https://tools.ietf.org/html/rfc5545#section-3.3.4>
fn parse_ical_date_notz(date: &str, tz: &Tz) -> Result<DateTime<Tz>, CalendarError> {
    // println!("Parsing {}", date);
    match NaiveDate::parse_from_str(date, "%Y%m%d") {
        // NOTE: we don't convert the datetime to the given timezone since we are talking about a
        // date that represents a particular _day_, not a time. Therefore we need to make sure that
        // we don't accidentally shift it into another day
        Ok(d) => Ok(tz
            .with_ymd_and_hms(d.year(), d.month(), d.day(), 0, 0, 0)
            .unwrap()),
        Err(chrono_err) => Err(CalendarError {
            msg: format!(
                "Can't parse date '{:?}' with cause: {:?}",
                date,
                chrono_err.to_string()
            ),
        }),
    }
}

fn extract_ical_date(prop: &Property, local_tz: &Tz) -> Result<DateTime<Tz>, CalendarError> {
    parse_ical_date_notz(prop.value.as_ref().unwrap(), local_tz)
}

/// This encapsulates the logic for parsing DTSTART and DTEND ical properties.
/// It will detect whether an event is an all day event or not and it will convert the datetime to
/// the local timezone.
fn extract_start_end_time(
    ical_event: &IcalEvent,
    calendar_timezones: &HashMap<String, CustomTz>,
    local_tz: &Tz,
) -> Result<(DateTime<Tz>, DateTime<Tz>, bool), CalendarError> {
    // we assume that DTSTART is mandatory, the spec sort of says that but also mentions something called
    // a "METHOD", ignoring that
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
        let start_time = extract_ical_date(start_property, local_tz)?;
        match end_property {
            Some(p) => extract_ical_date(p, local_tz).map(|end_time| (start_time, end_time, true)),
            None => Ok((start_time, start_time, true)),
        }
    } else {
        // println!("Have a 'real' datetime");
        // not a whole day event, so real times, there should be an end time
        match end_property {
            Some(p) => {
                let start_time =
                    extract_ical_datetime(start_property, calendar_timezones, local_tz)?;
                let end_time = extract_ical_datetime(p, calendar_timezones, local_tz)?;
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

// See https://tools.ietf.org/html/rfc5545#section-3.6.1
fn parse_event(
    ical_event: &IcalEvent,
    calendar_timezones: &HashMap<String, CustomTz>,
    local_tz: &Tz,
) -> Result<Event, CalendarError> {
    let summary = unescape_string(
        &find_property_value(&ical_event.properties, "SUMMARY").unwrap_or_default(),
    );
    let description = unescape_string(
        &find_property_value(&ical_event.properties, "DESCRIPTION").unwrap_or_default(),
    );
    let location = unescape_string(
        &find_property_value(&ical_event.properties, "LOCATION").unwrap_or_default(),
    );
    // println!("Parsing event '{}'", summary);
    let (start_timestamp, end_timestamp, all_day) =
        extract_start_end_time(ical_event, calendar_timezones, local_tz)?; // ? short circuits the error
    let meeturl = parse_zoom_url(&location)
        .or_else(|| parse_zoom_url(&summary))
        .or_else(|| parse_zoom_url(&description));
    
    // Count the number of participants by looking for email addresses in the description
    let num_participants = if !description.is_empty() {
        description.split(|c| c == ',' || c == ';' || c == '\n')
            .filter(|s| s.contains('@'))
            .count() as u32
    } else {
        1 // Default to 1 if no description
    };

    Ok(Event {
        summary,
        description,
        location,
        meeturl,
        all_day,
        start_timestamp,
        end_timestamp,
        num_participants,
    })
}

fn strip_param(p: &Property, param_name: &str) -> (Property, Option<String>) {
    let mut removed_param_value = None;
    let new_prop = Property {
        name: p.name.clone(),
        params: p.params.as_ref().map(|params| {
            params
                .iter()
                .filter(|param| {
                    if param.0 != param_name {
                        true
                    } else {
                        removed_param_value = Some(param.1[0].clone());
                        false
                    }
                })
                .cloned()
                .collect()
        }),
        value: p.value.clone(),
    };
    (new_prop, removed_param_value)
}

/// This function will parse occurrences from an ical event by using the rrule library to expand
/// the various rule definitions into concrete DateTimes representing recurring instances
///
/// There are 3 relevant properties: RRULE, DTSTART and EXDATE
/// Since the rrule lib can only deal with UTC (and maybe local dates) we need to convert any
/// local dates to UTC first and feed rrule the modified DTSTART and EXDATE properties.
/// According to https://tools.ietf.org/html/rfc5545#section-3.3.10 the UNTIL part of the RRULE
/// is either a DATE-TIME or a DATE, depending on how DTSTART is defined. The following cases can occur:
/// - If DTSTART is a DATE then rrule is fine and UNTIL is also a DATE
///   -> In case of dates we can NOT convert any dates, we need to just take them and apply the local
///      timezone to them (otherwise we might shift into a different day). We do need to strip the TZID
///      before we send to rrule.
/// - If DTSTART is a local DATE-TIME (e.g. 20200101T101010 (no Z)) then the UNTIL is also a local time
///   -> We can't correctly handle "local" datetimes yet since we don't know what the local timezone of
///      the calendar is. We should throw an error here (!?)
/// - If DTSTART is a UTC DATE-TIME (e.g. 20200101T101010Z (with a Z)) then the UNTIL is also UTC
///   -> This is easy: we just convert all generated occurrences to the local timezone from UTC
/// - If DTSTART is a DATE-TIME with a Timezone identifier, then UNTIL is in UTC (WTF people)
///   -> In this case we need to convert the EXDATE and DTSTART to UTC, feed that together with
///      the original RRULE to the library and save the timzone we identified in DTSTART.
///      We then convert all occurrences to the local timezone from UTC.
fn parse_occurrences(
    properties: &[Property],
    custom_timezones: &HashMap<String, CustomTz>,
    local_tz: &Tz,
) -> Result<Vec<DateTime<Tz>>, CalendarError> {
    // if no DTSTART or RRULE is present we can't do anything and assume we can't calculate occurrences
    let maybe_dtstart_prop = find_property(properties, "DTSTART");
    let maybe_rrule_prop = find_property(properties, "RRULE");
    if maybe_dtstart_prop.is_none() || maybe_rrule_prop.is_none() {
        return Ok(vec![]);
    }
    // some preliminary data wrangling so the actual handling of all the cases is easier afterwards
    let dtstart_prop = maybe_dtstart_prop.unwrap();
    let dtstart_time_str = dtstart_prop.value.as_ref().unwrap();
    let maybe_tzid_param = dtstart_prop
        .params
        .as_ref()
        .and_then(|params| find_param(params, "TZID"));
    let maybe_original_tz = if let Some(tzid_param) = maybe_tzid_param {
        let unescaped_tzid = unescape_string(&tzid_param[0]);
        match parse_tzid(&unescaped_tzid, custom_timezones) {
            Ok(original_tz) => Some(original_tz),
            Err(e) => {
                return Err(CalendarError {
                    msg: format!("error in timezone string parsing: {}", e),
                })
            }
        }
    } else {
        None
    };
    let rrule_prop = maybe_rrule_prop.unwrap();
    let maybe_exdate_prop = find_property(properties, "EXDATE");
    let all_day_event = is_ical_date(dtstart_prop);
    // Prepare a vec of all relevant rrule properties for rrule to work on by stripping tzid parameters
    let mut rule_props = vec![];
    let (stripped_dtstart, _) = strip_param(dtstart_prop, "TZID");
    rule_props.push(stripped_dtstart);
    let stripped_exdate; // need to define that here otherwise in the inside if scope it will go out of scope
    if let Some(exdate_prop) = maybe_exdate_prop {
        stripped_exdate = strip_param(exdate_prop, "TZID").0;
        rule_props.push(stripped_exdate);
    }
    // need to do this silly conversion as otherwise the with_timezone call below doesn't work correctly
    let current_year = Local::now().year();
    let skip_occurrence_pred = |d: &DateTime<Tz>| d.year() < (current_year - 1);
    let take_occurrence_pred =
        |d: &DateTime<Tz>| d.year() >= (current_year - 1) && d.year() <= (current_year + 1);
    //let take_occurrence_pred =
    // Case 1: DTSTART is a DATE
    if all_day_event {
        rule_props.push(rrule_prop.clone());
        let event_as_string = properties_to_string(&rule_props);
        match event_as_string.parse::<RRuleSet>() {
            Ok(ruleset) => Ok(ruleset
                .all()
                .iter()
                .skip_while(|d| skip_occurrence_pred(d))
                .take_while(|d| take_occurrence_pred(d))
                .map(|dt| {
                    local_tz
                        .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 0, 0, 0)
                        .unwrap()
                })
                .collect()),
            Err(e) => Err(CalendarError {
                msg: format!("error in RRULE parsing: {}", e),
            }),
        }
    } else if maybe_tzid_param.is_none() && !dtstart_time_str.ends_with('Z') {
        // CASE 2: we have local datetimes with no timezone information, throw error?
        Err(CalendarError {
            msg: "Found an event with a local timestamp without a timezone, this is unsupported"
                .to_string(),
        })
    } else if maybe_tzid_param.is_none() && dtstart_time_str.ends_with('Z') {
        // CASE 3: UTC datetime, let rrule do its thing, we convert all occurrences to the local TZ
        rule_props.push(rrule_prop.clone());
        let event_as_string = properties_to_string(&rule_props);
        match event_as_string.parse::<RRuleSet>() {
            Ok(ruleset) => Ok(ruleset
                .all()
                .iter()
                .skip_while(|d| skip_occurrence_pred(d))
                .take_while(|d| take_occurrence_pred(d))
                .map(|dt| dt.with_timezone(local_tz))
                .collect()),
            Err(e) => Err(CalendarError {
                msg: format!("error in RRULE parsing: {}", e),
            }),
        }
    } else if let Some(original_tz) = maybe_original_tz {
        // CASE 4: we have a timestamp with a timezone identifier
        // We strip the tz from the DTSTART and EXDATE and let rrule just regard them as naked timestamps
        // We do need to manually convert the UNTIL parameter of the RRULE to the original TZ since that
        // will always be UTC and if don't convert we can miss the last meeting of the interval
        //
        // Let rrule calculate occurrences
        // Interpret all occurrences as original TZ, then convert to local TZ
        //
        // hard assumption that there is a value always in an rrule
        let rrule_value = rrule_prop.value.as_ref().unwrap();
        // RRULE is a bit special, the parameters are not actually in the params but they are encoded in the VALUE of the property
        // we basically parse the value here and substitute the UNTIL component with a date that has a converted timestamp
        let rrule_value_modified = rrule_value
            .split(';')
            .map(|rrule_component| {
                if let Some(until_value) = rrule_component.strip_prefix("UNTIL=") {
                    if until_value.ends_with('Z') {
                        // NOTE we do not check whether maybe the parse failed, we hard assume it does
                        let until_originaltz_str = if original_tz.is_left() {
                            parse_ical_datetime(
                                until_value.to_string().strip_suffix('Z').unwrap(),
                                &Left(UTC),
                                &original_tz.left().unwrap(),
                            )
                            .unwrap()
                            .format("%Y%m%dT%H%M%S")
                            .to_string()
                        } else {
                            parse_ical_datetime(
                                until_value.to_string().strip_suffix('Z').unwrap(),
                                &Left(UTC),
                                original_tz.right().unwrap(),
                            )
                            .unwrap()
                            .format("%Y%m%dT%H%M%S")
                            .to_string()
                        };
                        format!("UNTIL={}", until_originaltz_str)
                    } else {
                        rrule_component.to_string()
                    }
                } else {
                    rrule_component.to_string()
                }
            })
            .collect::<Vec<String>>()
            .join(";");
        let new_rule_prop = Property {
            name: rrule_prop.name.clone(),
            params: rrule_prop.params.clone(),
            value: Some(rrule_value_modified),
        };
        rule_props.push(new_rule_prop);
        let event_as_string = properties_to_string(&rule_props);
        // println!("New RRULE string: {:?}", event_as_string);
        match event_as_string.parse::<RRuleSet>() {
            Ok(ruleset) => Ok(ruleset
                .all()
                .iter()
                .skip_while(|d| skip_occurrence_pred(d))
                .take_while(|d| take_occurrence_pred(d))
                .map(|dt| {
                    let original_datetime = &NaiveDateTime::new(
                        NaiveDate::from_ymd_opt(dt.year(), dt.month(), dt.day()).unwrap(),
                        NaiveTime::from_hms_opt(dt.hour(), dt.minute(), dt.second()).unwrap(),
                    );
                    // println!(
                    //     "converted occurence date from {:?} to {:?}",
                    //     original_datetime,
                    //     original_tz
                    //         .from_local_datetime(&original_datetime)
                    //         .unwrap()
                    //         .with_timezone(&local_tz)
                    // );
                    if original_tz.is_left() {
                        original_tz
                            .left()
                            .unwrap()
                            .from_local_datetime(original_datetime)
                            .unwrap()
                            .with_timezone(local_tz)
                    } else {
                        original_tz
                            .right()
                            .unwrap()
                            .from_local_datetime(original_datetime)
                            .unwrap()
                            .with_timezone(local_tz)
                    }
                })
                .collect()),
            Err(e) => Err(CalendarError {
                msg: format!("error in RRULE parsing: {}", e),
            }),
        }
    } else {
        Err(CalendarError {
            msg: "Unknown ical event date specification".to_string(),
        })
    }
}

/// Partitions the events into those that are modifying (i.e. they modify event instances of
/// recurring events) and non modifying events which are the base events we need to process without
/// all the modifying events.
fn partition_modifying_events(
    events: &[(IcalEvent, Event)],
    calendar_timezones: &HashMap<String, CustomTz>,
    local_tz: &Tz,
) -> (
    MultiMap<String, (IcalEvent, Event)>,
    Vec<(IcalEvent, Event)>,
) {
    // Create a map of all modifying events so we can correct recurring occurrences later
    let mut modifying_events: MultiMap<String, (IcalEvent, Event)> = MultiMap::new();
    let mut non_modifying_events: Vec<(IcalEvent, Event)> = Vec::new();
    let mut non_modifying_event_uids = HashSet::new();
    // find_property_value(&ical_event.properties, "UID").unwrap();
    for (ical_event, event) in events {
        // presence of a RECURRENCE-ID property is the trigger to know this is a modifying event
        if let Some(recurrence_id_property) = find_property(&ical_event.properties, "RECURRENCE-ID")
        {
            match extract_ical_datetime(recurrence_id_property, calendar_timezones, local_tz) {
                Ok(_) => {
                    if let Some(uid) = find_property_value(&ical_event.properties, "UID") {
                        // println!("+MODIFYING EVENT: {:?}", ical_event);
                        modifying_events.insert(uid, (ical_event.clone(), event.clone()));
                    }
                }
                Err(e) => eprintln!("Can't parse a recurrence id as datetime: {:?}", e),
            }
        } else {
            // println!("NON-MODIFYING EVENT: {:?}", ical_event);
            let uid = find_property_value(&ical_event.properties, "UID").unwrap();
            non_modifying_event_uids.insert(uid);
            non_modifying_events.push((ical_event.clone(), event.clone()));
        }
    }
    // We make sure that we only retain modifying events that actually modify a non-modifying event
    // if this is not the case then we assume that the modifying event is a full event on its own
    // and add it back to the modifying events collection.
    // This is something that can happen when someone gets forwarded a modified occurrence of an event
    // and _just_ that modified occurrence.
    for (modifying_uid, events) in &modifying_events {
        if !non_modifying_event_uids.contains(modifying_uid) {
            non_modifying_events.append(&mut events.clone())
        }
    }
    modifying_events
        .retain(|modifying_uid, _value| non_modifying_event_uids.contains(modifying_uid));
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

fn parse_events(
    calendar: IcalCalendar,
    calendar_timezones: &HashMap<String, CustomTz>,
    local_tz: &Tz,
) -> Result<Vec<(IcalEvent, Event)>, CalendarError> {
    calendar
        .events
        .into_iter()
        .map(
            |event| match parse_event(&event, calendar_timezones, local_tz) {
                Ok(parsed_event) => Ok((event, parsed_event)),
                Err(e) => Err(e),
            },
        )
        .collect::<Result<Vec<(IcalEvent, Event)>, CalendarError>>() // will fail on the first parse error and return an error
}

fn calculate_occurrences(
    ical_event: &IcalEvent,
    parsed_event: &Event,
    occurrences: &[DateTime<Tz>],
    modifying_events: &MultiMap<String, (IcalEvent, Event)>,
    calendar_timezones: &HashMap<String, CustomTz>,
    local_tz: &Tz,
) -> Vec<Event> {
    occurrences
        .iter()
        .map(|datetime| {
            // We need to figure out whether the occurrence can be used as such or whether it was changed by a modifying event
            // We assume that each ical_event that is a recurring event has a UID, otherwise the unwrap will fail here.
            // Needs more error handling?
            let occurrence_uid = find_property_value(&ical_event.properties, "UID").unwrap();
            if modifying_events.contains_key(&occurrence_uid) {
                let modifications = modifying_events.get_vec(&occurrence_uid).unwrap();
                for (modifying_ical_event, modifying_event) in modifications {
                    // since these modifying events are constructed before and are assumed to have a recurrence-id we just unwrap here
                    let recurrence_id_property =
                        find_property(&modifying_ical_event.properties, "RECURRENCE-ID").unwrap();
                    // println!(
                    //     "Calculating start and end for recurrence event {}",
                    //     parsed_event.summary
                    // );
                    let recurrence_datetime =
                        extract_ical_datetime(recurrence_id_property, calendar_timezones, local_tz)
                            .unwrap();
                    if *datetime == recurrence_datetime {
                        // the modifying event has the same UID as our event and it has the same timestamp, so we return the modification instead
                        return modifying_event.clone();
                    }
                }
            }
            // we need to calculate this occurrence's end time by adding the duration of the original event to this particular start time
            let end_time = *datetime
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
                start_timestamp: *datetime,
                end_timestamp: end_time,
                num_participants: parsed_event.num_participants,
            }
        })
        .collect()
}

pub fn extract_events(text: &str, local_tz: &Tz) -> Result<Vec<Event>, CalendarError> {
    match parse_calendar(text)? {
        Some(calendar) => {
            let calendar_timezones = parse_ical_timezones(&calendar, local_tz)?;
            //println!("Calendar timezones found: {:?}", calendar_timezones);
            let event_tuples = parse_events(calendar, &calendar_timezones, local_tz)?;
            // Events are either normal events (potentially recurring) or they are modifying events
            // that defines exceptions to recurrences of other events. We need to split these types out
            let (modifying_events, non_modifying_events) =
                partition_modifying_events(&event_tuples, &calendar_timezones, local_tz);
            // Calculate occurrences for recurring events
            non_modifying_events
                .into_iter()
                .map(|(ical_event, parsed_event)| {
                    match parse_occurrences(&ical_event.properties, &calendar_timezones, local_tz) {
                        Ok(occurrences) => {
                            // println!("Occurrences for {:?}: {:?}", ical_event, occurrences);
                            if occurrences.is_empty() {
                                Ok(vec![parsed_event])
                            } else {
                                Ok(calculate_occurrences(
                                    &ical_event,
                                    &parsed_event,
                                    &occurrences,
                                    &modifying_events,
                                    &calendar_timezones,
                                    local_tz,
                                ))
                            }
                        }
                        Err(e) => Err(e),
                    }
                })
                // we now have replaced each event with a list of its occurrences
                .collect::<Result<Vec<Vec<Event>>, CalendarError>>()
                .map(|event_instances| {
                    event_instances.into_iter().flatten().collect() // flatmap that shit
                })
        }
        None => Ok(vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn rrule_all_missing_final_meeting() {
        //println!("{:?}", "DTSTART;TZID=W. Europe Standard Time:20210316T113000\nRRULE:FREQ=WEEKLY;UNTIL=20210511T093000Z;INTERVAL=1;BYDAY=TU;WKST=MO\nEXDATE;TZID=W. Europe Standard Time:20210406T113000,20210504T11300\n0UID:040000008200E00074C5B7101A82E0080000000000EB5C2C7B0FD701000000000000000\n 010000000E4ADD290686A07499DF2A0FAB11D79E9".parse::<RRuleSet>().unwrap().all());
        println!("{:?}", "DTSTART:20210316T093000Z\nRRULE:FREQ=WEEKLY;UNTIL=20210511T093000Z;INTERVAL=1;BYDAY=TU;WKST=MO".parse::<RRuleSet>().unwrap());
    }

    #[test]
    fn rrule_invalid_by_month_value() {
        // This used to fail with "invalid by_month value"
        "DTSTART;VALUE=DATE:20211206\nRRULE:FREQ=YEARLY;UNTIL=20221205T230000Z;INTERVAL=1;BYMONTHDAY=6;BYMONTH=12".parse::<RRuleSet>().unwrap();
    }

    // The following test was reported as https://github.com/fmeringdal/rust_rrule/issues/13
    // but it wasn't really rrule's fault, it just can't deal with non-standard timezone identifiers
    // I am now trying to handle that myself and "protecting" rrule from this case
    // #[test]
    // fn rrule_generates_final_event_on_8_3_2021() {
    //     let dates = "DTSTART;TZID=W. Europe Standard Time:20201214T093000\nRRULE:FREQ=WEEKLY;UNTIL=20210308T083000Z;INTERVAL=2;BYDAY=MO;WKST=MO\nEXDATE;TZID=W. Europe Standard Time:20201228T093000,20210125T093000,20210208T093000".parse::<RRuleSet>().unwrap().all();
    //     // the following outputs 2021-02-22 09:30:00 UTC
    //     println!("last date: {}", dates[dates.len() - 1]);
    //     assert_eq!(8, dates[dates.len() - 1].day());
    //     assert_eq!(3, dates[dates.len() - 1].month());
    //     assert_eq!(2021, dates[dates.len() - 1].year());
    // }

    // TODO: create a test case for this apple event in outlook custom timezone example:
    /*

    BEGIN:VCALENDAR
    METHOD:PUBLISH
    PRODID:Microsoft Exchange Server 2010
    VERSION:2.0
    X-WR-CALNAME:Calendar

    BEGIN:VTIMEZONE
    TZID:W. Europe Standard Time
    BEGIN:STANDARD
    DTSTART:16010101T030000
    TZOFFSETFROM:+0200
    TZOFFSETTO:+0100
    RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=10
    END:STANDARD
    BEGIN:DAYLIGHT
    DTSTART:16010101T020000
    TZOFFSETFROM:+0100
    TZOFFSETTO:+0200
    RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=3
    END:DAYLIGHT
    END:VTIMEZONE

    BEGIN:VTIMEZONE
    TZID:(UTC) Coordinated Universal Time
    BEGIN:STANDARD
    DTSTART:16010101T000000
    TZOFFSETFROM:+0000
    TZOFFSETTO:+0000
    END:STANDARD
    BEGIN:DAYLIGHT
    DTSTART:16010101T000000
    TZOFFSETFROM:+0000
    TZOFFSETTO:+0000
    END:DAYLIGHT
    END:VTIMEZONE

    BEGIN:VTIMEZONE
    TZID:Customized Time Zone
    BEGIN:STANDARD
    DTSTART:16010101T020000
    TZOFFSETFROM:-0700
    TZOFFSETTO:-0800
    RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=1SU;BYMONTH=11
    END:STANDARD
    BEGIN:DAYLIGHT
    DTSTART:16010101T020000
    TZOFFSETFROM:-0800
    TZOFFSETTO:-0700
    RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=2SU;BYMONTH=3
    END:DAYLIGHT
    END:VTIMEZONE

    BEGIN:VEVENT
    DESCRIPTION:Join us for the WWDC21 Apple Keynote broadcasting from Apple Pa
        rk. Watch it online at apple.com.
    UID:040000008200E00074C5B7101A82E0080000000043FB88505C5BD701000000000000000
        01000000041F5952E4CF15A44A138CAFC4CA230AC
    SUMMARY:WWDC21 Apple Keynote
    DTSTART;TZID=Customized Time Zone:20210607T100000
    DTEND;TZID=Customized Time Zone:20210607T120000
    CLASS:PUBLIC
    PRIORITY:5
    DTSTAMP:20210607T064729Z
    TRANSP:OPAQUE
    STATUS:CONFIRMED
    SEQUENCE:0
    LOCATION:apple.com/apple-events/event-stream/
    X-MICROSOFT-CDO-APPT-SEQUENCE:0
    X-MICROSOFT-CDO-BUSYSTATUS:BUSY
    X-MICROSOFT-CDO-INTENDEDSTATUS:BUSY
    X-MICROSOFT-CDO-ALLDAYEVENT:FALSE
    X-MICROSOFT-CDO-IMPORTANCE:1
    X-MICROSOFT-CDO-INSTTYPE:0
    X-MICROSOFT-DONOTFORWARDMEETING:FALSE
    X-MICROSOFT-DISALLOW-COUNTER:FALSE
    END:VEVENT
    END:VCALENDAR

        */

    /*
            Outlook timezone definition example

    BEGIN:VTIMEZONE
    TZID:W. Europe Standard Time
    BEGIN:STANDARD
    DTSTART:16010101T030000
    TZOFFSETFROM:+0200
    TZOFFSETTO:+0100
    RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=10
    END:STANDARD
    BEGIN:DAYLIGHT
    DTSTART:16010101T020000
    TZOFFSETFROM:+0100
    TZOFFSETTO:+0200
    RRULE:FREQ=YEARLY;INTERVAL=1;BYDAY=-1SU;BYMONTH=3
    END:DAYLIGHT
    END:VTIMEZONE
    BEGIN:VTIMEZONE
    TZID:UTC
    BEGIN:STANDARD
    DTSTART:16010101T000000
    TZOFFSETFROM:+0000
    TZOFFSETTO:+0000
    END:STANDARD
    BEGIN:DAYLIGHT
    DTSTART:16010101T000000
    TZOFFSETFROM:+0000
    TZOFFSETTO:+0000
    END:DAYLIGHT
    END:VTIMEZONE

        */
}
