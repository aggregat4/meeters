use std::env;
use std::fmt;
use std::path::Path;
use std::thread;
use std::time::Duration;
use gtk::prelude::*;
use libappindicator::{AppIndicator, AppIndicatorStatus};
use ical::parser::ical::component::IcalEvent;
use ical::property::Property;
use chrono::prelude::*;

// From https://doc.rust-lang.org/stable/rust-by-example/error/multiple_error_types/define_error_type.html and message added
#[derive(Debug, Clone)]
struct CalendarError {
    // Type "String" means that the struct owns and stores the string, if I would use a string reference (&str)
    // I would also need to specify a lifecycle like  "msg: &'a str,". This is less storage
    // but it means we can't just generate error messages on the fly that are not static
    // If I _would_ use a refernce we need to suffix the CalendarError type with a lifetime like
    // CalendarError<'a>
    msg: String,
}

impl fmt::Display for CalendarError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Error getting events: {}", self.msg)
    }
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

fn get_ical(url: &str) -> Result<String, reqwest::Error> {
    let body = reqwest::blocking::get(url)?;
    return body.text();
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

#[derive(Debug)]
struct Event {
    summary: String,
    description: String,
    location: String,
    meeturl: String,
    all_day: bool,
    start_timestamp: DateTime<FixedOffset>,
    end_timestamp: DateTime<FixedOffset>,
    // TODO: more things like status?
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

fn get_events(url: &str) -> Result<Vec<Event>, CalendarError> {
    // the .or() invocation converts from the custom reqwest error to our standard CalendarError
    let text = get_ical(url).or_else(|get_error| Err(CalendarError { msg: format!("Error getting calendar: {}", get_error).to_string() }))?;
    let mut reader = ical::IcalParser::new(text.as_bytes());
    match reader.next() {
        Some(result) => match result {
            Ok(calendar) => {
                println!("Number of events: {:?}", calendar.events.len());
                let mut events: Vec<Event> = Vec::new();
                for event in calendar.events {
                    // println!(
                    //     "Summary {:?}, DTSTART: {:?}",
                    //     find_property_value(&event.properties, "SUMMARY"),
                    //     find_property_value(&event.properties, "DTSTART")
                    // );
                    events.push(parse_event(&event)?);
                }
                return Ok(events);
            }
            Err(e) => Err(CalendarError {
                msg: format!("error in ical parsing: {}", e),
            }),
        },
        None => return Ok(vec![]),
    }
}

fn start_calendar_work(url: String) {
    // TODO: figure out this move crap
    thread::spawn(move || loop {
        match get_events(&url) {
            Ok(events) => {
                println!("Successfully got {:?} events", events.len());
                let today_start = Local::now().date().and_hms(0, 0, 0);
                let today_end = Local::now().date().and_hms(23, 59, 59);
                let today_events = events
                    .into_iter()
                    .filter(|e| e.start_timestamp > today_start && e.start_timestamp < today_end)
                    .collect::<Vec<_>>();
                println!("There are {} events for today: {:?}", today_events.len(), today_events);
            },
            Err(e) => println!("Error getting events: {:?}", e.msg),
        }
        thread::sleep(Duration::from_secs(10));
    });
}

fn create_indicator() -> AppIndicator {
    let mut indicator = AppIndicator::new("rs-meetings", "");
    indicator.set_status(AppIndicatorStatus::Active);
    // including resources into a package is unsolved, except perhaps for something like https://doc.rust-lang.org/std/macro.include_bytes.html
    // for our purposes this should probably be a resource in the configuration somewhere
    let icon_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    indicator.set_icon_theme_path(icon_path.to_str().unwrap());
    indicator.set_icon_full("rust-logo-64x64-blk", "icon");
    //indicator.set_label("Next meeting foobar 4 min", "8.8"); // does not get shown in XFCE
    //indicator.connect_activate
    return indicator;
}

fn create_indicator_menu() -> gtk::Menu {
    let m = gtk::Menu::new();
    let mi = gtk::MenuItem::with_label("Quit");
    mi.connect_activate(|_| {
        gtk::main_quit();
    });
    m.append(&mi);
    m.show_all();
    return m;
}

fn main() {
    // magic incantation
    gtk::init().unwrap();
    // set up our widgets
    let mut indicator = create_indicator();
    let mut menu = create_indicator_menu();
    indicator.set_menu(&mut menu);
    // start the background thread for calendar work
    let args: Vec<String> = env::args().collect();
    if args.len() == 1 {
        panic!("Need a command line argument with the url of the ICS file")
    }
    start_calendar_work(String::from(&args[1]));
    // start listening for messsages
    gtk::main();
}
