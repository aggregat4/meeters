extern crate rrule;

use ical::parser::ical::component::IcalEvent;
use ical::property::Property;
use chrono::prelude::*;
use chrono_tz::Tz;
use std::fmt;
use rrule::RRule;
use rrule::RRuleSet;
use std::cmp::Ordering;

// From https://doc.rust-lang.org/stable/rust-by-example/error/multiple_error_types/define_error_type.html and message added
#[derive(Debug, Clone)]
pub struct CalendarError {
    // Type "String" means that the struct owns and stores the string, if I would use a string reference (&str)
    // I would also need to specify a lifecycle like  "msg: &'a str,". This is less storage
    // but it means we can't just generate error messages on the fly that are not static
    // If I _would_ use a refernce we need to suffix the CalendarError type with a lifetime like
    // CalendarError<'a>
    pub msg: String,
}

impl fmt::Display for CalendarError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Error getting events: {}", self.msg)
    }
}

#[derive(Debug)]
pub struct Event {
    pub summary: String,
    pub description: String,
    pub location: String,
    pub meeturl: String,
    pub all_day: bool,
    pub start_timestamp: DateTime<FixedOffset>,
    pub end_timestamp: DateTime<FixedOffset>,
    // TODO: more things like status?
}

fn find_property_value(properties: &Vec<Property>, name: &str) -> Option<String> {
    for property in properties {
        if property.name == name {
            // obviously this clone works but I don't like it, as_ref() didn't seem to do it
            // still do not understand the semantics I should be using here
            return property.value.clone();
        }
    }
    return None;
}

fn find_property<'a>(properties: &'a Vec<Property>, name: &str) -> Option<&'a Property> {
    for property in properties {
        if property.name == name {
            return Some(property);
        }
    }
    return None;
}

fn find_param<'a>(params: &'a Vec<(String, Vec<String>)>, name: &str) -> Option<&'a Vec<String>> {
    for param in params {
        let (param_name, values) = param;
        if param_name == name {
            return Some(values);
        }
    }
    return None;
}

/**
* Parses datetimes of the format 'YYYYMMDDTHHMMSS'
* 
* See https://tools.ietf.org/html/rfc5545#section-3.3.5
*/
fn parse_ical_datetime(datetime: &str, tz: &FixedOffset) -> Result<DateTime<FixedOffset>, CalendarError> {
    // TODO: implementation
    // this is where I left off: Plan: we get the timezone here that was determined by either having
    // a Z modifier indicating zulu time, or no timezone indicating local time or it has an explicit timzone indicator and then we can just use it
    return match NaiveDateTime::parse_from_str(&datetime, "%Y%m%dT%H%M%S") {
        Ok(d) => Ok(tz.from_local_datetime(&d).unwrap()),
        Err(_) => Err(CalendarError { msg: "Can't parse datetime string with tzid".to_string() })
    }
}

/**
* If a property is a timestamp it can have 3 forms ( see https://tools.ietf.org/html/rfc5545#section-3.3.5 )
*/
fn extract_ical_datetime(prop: &Property) -> Result<DateTime<FixedOffset>, CalendarError> {
    let date_time_str = prop.value.as_ref().unwrap();
    if prop.params.is_some() && find_param(prop.params.as_ref().unwrap(), "TZID").is_some() {
        // timestamp with an explicit timezone: YYYYMMDDTHHMMSS
        // TODO: timezone parsing at some point, for now just assume local
        return parse_ical_datetime(&date_time_str, Local::now().offset());
    } else {
        // It is either
        //  - a datetime with no timezone: 20201102T235401
        //  - a datetime with in UTC:      20201102T235401Z
        if date_time_str.ends_with("Z") {
            return parse_ical_datetime(&date_time_str, &Utc.fix());
        } else {
        // TODO: I can't find a better way to get the local timezone offset, maybe just keep this value in a static?
        return parse_ical_datetime(&date_time_str, Local::now().offset());
        }
    }
}

/**
* Parses an ical date with no timezone information into a chrono Local date, assuming that it is in the
* local timezone. (This is probably wrong)
* 
* See https://tools.ietf.org/html/rfc5545#section-3.3.4
*/
fn parse_ical_date_notz(date: &str, tz: &FixedOffset) -> Result<DateTime<FixedOffset>, CalendarError> {
    // TODO: What time is assumed here by chrono and does that match the ical spec?
    return match NaiveDate::parse_from_str(date, "%Y%m%d") {
        Ok(d) => Ok(tz.from_local_datetime(&d.and_hms(0, 0, 0)).unwrap()),
        Err(chrono_err) => Err(CalendarError { msg: format!("Can't parse date '{:?}' with cause: {:?}", date, chrono_err.to_string()).to_string() })
    }
}

fn extract_ical_date(prop: &Property) -> Result<DateTime<FixedOffset>, CalendarError> {
    return parse_ical_date_notz(&prop.value.as_ref().unwrap(), Local::now().offset());
}

fn extract_start_end_time(
    ical_event: &IcalEvent,
) -> Result<(DateTime<FixedOffset>, DateTime<FixedOffset>, bool), CalendarError> {
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
        // the first real value of the VALUE param should be "DATE"
        let value_param = &find_param(start_property.params.as_ref().unwrap(), "VALUE").unwrap()[0];
        if value_param != "DATE" {
            return Err(CalendarError{ msg: format!("Encountered DTSTART with a VALUE parameter that has a value different from 'DATE': {}", value_param).to_string() })
        }
        // start property is a "DATE", which indicates a whole day or multi day event
        // see https://tools.ietf.org/html/rfc5545#section-3.6.1 and specifically the discussion on DTSTART
        let start_time = extract_ical_date(start_property)?;
        if end_property.is_some() {
            return extract_ical_date(end_property.unwrap()).and_then(|end_time| Ok((start_time, end_time, true)));
        } else {
            return Ok((start_time, start_time, true));
        }
    } else {
        // not a whole day event, so real times, there should be an end time
        if end_property.is_some() {
            let start_time = extract_ical_datetime(start_property)?;
            let end_time = extract_ical_datetime(end_property.unwrap())?;
            return Ok((start_time, end_time, false));
        } else {
            return Err(CalendarError{ msg: "missing end time for an event".to_string() })
        }
    }
}

// See https://tools.ietf.org/html/rfc5545#section-3.6.1
fn parse_event(ical_event: &IcalEvent) -> Result<Event, CalendarError> {
    let summary = find_property_value(&ical_event.properties, "SUMMARY").unwrap_or("".to_string());
    let description =
        find_property_value(&ical_event.properties, "SUMMARY").unwrap_or("".to_string());
    let location = find_property_value(&ical_event.properties, "SUMMARY").unwrap_or("".to_string());
    let (start_time, end_time, all_day) = extract_start_end_time(&ical_event)?; // ? short circuits the error
    return Ok(Event {
        summary: summary,
        description: description,
        location: location,
        meeturl: "".to_string(),
        all_day: all_day,
        start_timestamp: start_time,
        end_timestamp: end_time,
    });
    // TODO: parse meetUrl from summary, description and location and consolidate
}

fn parse_occurrences(event: &IcalEvent) -> Result<Vec<DateTime<Tz>>, CalendarError> {
    // We need to compensate for some weaknesses in the rrule library
    // by sanitising some date constructs and filtering out spurious ical fields.
    let rrule_props = event.properties
        .iter()
        .filter(|p| p.name == "DTSTART" || p.name == "RRULE") // || p.name == "DTEND"
        .map(|p| if p.name == "DTSTART" {
            return Property {
                name: p.name.clone(),
                params: match &p.params {
                    None => None,
                    Some(params) => Some(params.iter().filter(|param| param.0 != "TZID").cloned().collect()),
                },
                value: p.value.clone(),
            } 
        } else { return p.clone() })
        .collect::<Vec<Property>>();
    let event_as_string = properties_to_string(&rrule_props);
    match event_as_string.parse::<RRuleSet>() {
        Ok(mut ruleset) => Ok(ruleset.all()),
        Err(e) => Err(CalendarError { msg: format!("error in RRULE parsing: {}", e) })
    }
}

pub fn parse_events(text: &str) -> Result<Vec<Event>, CalendarError> {
    let mut reader = ical::IcalParser::new(text.as_bytes());
    match reader.next() {
        Some(result) => match result {
            Ok(calendar) => {
                println!("Number of events: {:?}", calendar.events.len());
                return calendar.events
                    .into_iter()
                    .map(|event| match parse_event(&event) {
                        Ok(parsed_event) => Ok((event, parsed_event)),
                        Err(e) => Err(e)
                    } )
                    .collect::<Result<Vec<(IcalEvent, Event)>, CalendarError>>() // will fail on the first parse error and return n error
                    .and_then(|event_tuples| event_tuples
                        .into_iter()
                        .map(|(event, parsed_event)| match parse_occurrences(&event) {
                            Ok(occurrences) => if occurrences.len() == 0 {
                                Ok(vec![parsed_event])
                            } else {
                                Ok(occurrences
                                    .into_iter()
                                    .map(|datetime| Event {
                                        summary: parsed_event.summary,
                                        description: parsed_event.description,
                                        location: parsed_event.location,
                                        meeturl: parsed_event.meeturl,
                                        all_day: parsed_event.all_day,
                                        // TODO: fix our DateTimes to use real TimeZones instead of fixed offsets so we can use the occurrences
                                        // start_timestamp: datetime,
                                        // end_timestamp: DateTime<FixedOffset>,                                    
                                    })
                                    .collect()
                                )
                            },
                            Err(e) => Err(e)
                        })
                        .collect::<Result<Vec<Vec<Event>>,CalendarError>>() // will fail on the first parse error and return n error
                        .and_then(|event_instances| Ok(event_instances
                            .into_iter()
                            .flatten()
                            .collect())
                        )
                    )
            }
            Err(e) => Err(CalendarError { msg: format!("error in ical parsing: {}", e) }),
        },
        None => return Ok(vec![]),
    }
}

fn format_param_values(param_values: &Vec<String>) -> String {
    return param_values
        .iter()
        .map(|param_val| if param_val.contains(" ") { format!("\"{}\"", param_val) } else { param_val.to_string() } )
        .collect::<Vec<String>>()
        .join(",");
}

fn params_to_string(params: &Vec<(String, Vec<String>)>) -> String {
    if params.is_empty() {
        return "".to_string();
    } else {
        return format!(";{}",
            params
                .into_iter()
                .map(|param| format!("{}={}", param.0, format_param_values(&param.1)))
                .collect::<Vec<String>>()
                .join(","));
    }
}

fn prop_to_string(prop: &Property) -> String {
    return format!("{}{}:{}", prop.name, params_to_string(&prop.params.as_ref().unwrap_or(&vec![])), prop.value.as_ref().unwrap_or(&"".to_string()));
}

fn properties_to_string(properties: &Vec<Property>) -> String {
    return properties
        .iter() // "interesting" note here: i was getting an E0507 when using into_iter since that apparenty takes ownership. and iter is just return refs
        .map(|p| prop_to_string(&p))
        .collect::<Vec<String>>()
        .join("\n");
}

fn ical_event_to_string(event: &IcalEvent) -> String {
    return properties_to_string(&event.properties);
}

#[cfg(test)]
mod tests {
    use ical::parser::Component;
    use super::*;

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
    
    #[test]
    fn rruleset_parsing_date() {
        "DTSTART;VALUE=DATE:20200812\nRRULE:FREQ=WEEKLY;UNTIL=20210511T220000Z;INTERVAL=1;BYDAY=WE;WKST=MO".parse::<RRuleSet>().unwrap();
    }
    
    #[test]
    fn rruleset_parsing_date_with_timezone() {
        "DTSTART;TZID=\"(UTC+01:00) Amsterdam, Berlin, Bern, Rome, Stockholm, Vienna\":20201111T160000\nRRULE:FREQ=WEEKLY;UNTIL=20210428T140000Z;INTERVAL=6;BYDAY=WE;WKST=MO".parse::<RRuleSet>().unwrap();
    }

}
