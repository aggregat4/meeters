use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::{json, Value};

use crate::config::{CalendarSourceConfig, Config};
use crate::domain::Event;
use crate::ews::{self, EwsConfig, EwsError};
use crate::secrets::{self, StoredPassword};

pub const HELP: &str = "Usage: meeters [--calendar-until YYYY-MM-DD]\n\nWith no arguments, starts the tray application.\n--calendar-until emits EWS calendar summaries as JSON, from now through the\nrequested date (inclusive) in MEETERS_LOCAL_TIMEZONE (default Europe/Berlin).\nUses the existing configuration and saved keyring password; never saves credentials.\nUnlock the desktop keyring first to avoid a keyring unlock prompt.\n-h, --help  Show this help.";

#[derive(Debug, PartialEq)]
pub enum Mode {
    Gui,
    Help,
    Calendar(NaiveDate),
}

pub fn parse_args(args: &[String]) -> Result<Mode, String> {
    match args {
        [] => Ok(Mode::Gui),
        [flag] if flag == "--help" || flag == "-h" => Ok(Mode::Help),
        [flag, date] if flag == "--calendar-until" => {
            if date.len() != 10
                || !date.bytes().enumerate().all(|(index, byte)| match index {
                    4 | 7 => byte == b'-',
                    _ => byte.is_ascii_digit(),
                })
            {
                return Err("Expected a date in YYYY-MM-DD format".into());
            }
            let parsed = NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map_err(|_| "Expected a valid date in YYYY-MM-DD format".to_string())?;
            if parsed.format("%Y-%m-%d").to_string() != *date {
                return Err("Expected a date in YYYY-MM-DD format".into());
            }
            Ok(Mode::Calendar(parsed))
        }
        _ => Err("Expected --calendar-until YYYY-MM-DD, --help, or no arguments".into()),
    }
}

fn range_end(date: NaiveDate, now: DateTime<Tz>) -> Result<DateTime<Tz>, String> {
    if date < now.date_naive() {
        return Err("Requested date must be today or later in the configured timezone".into());
    }
    let midnight = date
        .succ_opt()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .ok_or("Requested date is out of range")?;
    now.timezone().from_local_datetime(&midnight).single()
        .ok_or_else(|| "Midnight after the requested date is ambiguous or nonexistent in the configured timezone".into())
}

fn calendar_json(events: Vec<Event>, start: DateTime<Tz>, end: DateTime<Tz>) -> Value {
    let mut events = events
        .into_iter()
        .filter(|e| e.start_timestamp < end && e.end_timestamp > start)
        .collect::<Vec<_>>();
    events.sort_by_key(|e| e.start_timestamp);
    json!({
        "timezone": start.timezone().name(),
        "start": start.to_rfc3339(),
        "end_exclusive": end.to_rfc3339(),
        "events": events.iter().map(|e| json!({
            "summary": e.summary,
            "start": e.start_timestamp.to_rfc3339(),
            "end": e.end_timestamp.to_rfc3339(),
            "all_day": e.all_day,
            "location": e.location,
        })).collect::<Vec<_>>()
    })
}

pub fn run(date: NaiveDate) -> Result<String, String> {
    // Do not echo configuration values, server responses or authentication data.
    let config = Config::load().map_err(|_| {
        "Could not load configuration; check meeters_config.env and MEETERS_* settings"
    })?;
    let start = Utc::now().with_timezone(&config.local_tz);
    let end = range_end(date, start)?;
    let CalendarSourceConfig::Ews { url, user } = config.calendar_source else {
        return Err("--calendar-until requires MEETERS_CALENDAR_SOURCE=ews".into());
    };
    let password = match secrets::get_ews_password(&user)
        .map_err(|_| "Could not read EWS password; unlock the desktop keyring and retry")?
    {
        StoredPassword::Found(password) => password,
        StoredPassword::Missing => {
            return Err(
                "No saved EWS password; configure it in the normal tray application first".into(),
            )
        }
    };
    let events = ews::fetch_event_summaries(
        &ews::http_agent(std::time::Duration::from_secs(20)),
        &EwsConfig { url, user }, &password, &config.local_tz, start, end,
    ).map_err(|error| match error {
        EwsError::AuthFailed => "EWS authentication failed; check the saved password in the tray application",
        EwsError::Other(_) => "Could not read the complete EWS calendar; check connection/settings or try a shorter date range",
    })?;
    serde_json::to_string(&calendar_json(events, start, end))
        .map_err(|_| "Could not serialize calendar JSON".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::EventMetadata;

    fn dt(month: u32, day: u32, hour: u32) -> DateTime<Tz> {
        chrono_tz::Europe::Berlin
            .with_ymd_and_hms(2026, month, day, hour, 0, 0)
            .unwrap()
    }

    #[test]
    fn modes_and_strict_dates() {
        assert_eq!(parse_args(&[]).unwrap(), Mode::Gui);
        assert_eq!(parse_args(&["--help".into()]).unwrap(), Mode::Help);
        assert_eq!(
            parse_args(&["--calendar-until".into(), "2026-10-25".into()]).unwrap(),
            Mode::Calendar(dt(10, 25, 0).date_naive())
        );
        for args in [
            vec!["--calendar-until"],
            vec!["--wat"],
            vec!["--calendar-until", "2026-02-30"],
            vec!["--calendar-until", "2026-1-1"],
            vec!["--calendar-until", "+10000-01-01"],
            vec!["--calendar-until", "-0001-01-01"],
            vec!["--calendar-until", "2026-10-25T12:00:00"],
            vec!["--help", "extra"],
        ] {
            assert!(parse_args(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).is_err());
        }
    }

    #[test]
    fn inclusive_date_handles_dst_and_past_dates() {
        for (month, day, hours) in [(3, 29, 23), (10, 25, 25)] {
            let start = dt(month, day, 0);
            let end = range_end(start.date_naive(), start).unwrap();
            assert_eq!((end - start).num_hours(), hours);
            assert_eq!(end, dt(month, day + 1, 0));
        }
        assert!(range_end(dt(9, 29, 0).date_naive(), dt(9, 30, 0)).is_err());
        assert!(range_end(NaiveDate::MAX, dt(9, 30, 0)).is_err());
    }

    #[test]
    fn json_is_escaped_sorted_body_free_and_half_open() {
        let start = dt(9, 30, 10);
        let end = dt(10, 1, 0);
        let event = |name: &str, from, to| Event {
            summary: name.into(),
            description: "private body".into(),
            location: "Room\n\"A\"".into(),
            meeturl: None,
            metadata: EventMetadata::empty(),
            all_day: false,
            start_timestamp: from,
            end_timestamp: to,
        };
        let data = calendar_json(
            vec![
                event("later", dt(9, 30, 15), dt(9, 30, 16)),
                event("ongoing ä", dt(9, 30, 9), dt(9, 30, 11)),
                event("ended", dt(9, 30, 9), start),
                event("tomorrow", end, dt(10, 1, 1)),
            ],
            start,
            end,
        );
        let text = serde_json::to_string(&data).unwrap();
        assert!(!text.contains("private body"));
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["events"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["events"][0]["summary"], "ongoing ä");
        assert_eq!(parsed["events"][0]["location"], "Room\n\"A\"");
        assert_eq!(parsed["end_exclusive"], "2026-10-01T00:00:00+02:00");
        assert_eq!(calendar_json(vec![], start, end)["events"], json!([]));
    }
}
