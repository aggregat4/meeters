use std::path::PathBuf;
use std::thread;

use chrono::prelude::*;
use chrono_tz::Tz;
use directories::ProjectDirs;
use ureq::Agent;

use crate::domain::Event;
use crate::CalendarMessages::{DayEvents, EventNotification};
use domain::CalendarError;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod binary_search;
mod custom_timezone;
mod domain;
mod gui;
mod ical_util;
mod meeters_ical;
mod timezones;
mod windows_timezones;

use std::sync::Arc;

fn get_ical(url: &str) -> Result<String, CalendarError> {
    println!("trying to fetch ical");
    let config = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .build();
    let agent: Agent = config.into();
    agent
        .get(url)
        .call()
        .map_err(|e| CalendarError {
            msg: format!("Error calling calendar URL: {}", e),
        })?
        .body_mut()
        .read_to_string()
        .map_err(|e| CalendarError {
            msg: format!("Error reading calendar response body: {}", e),
        })
}

fn get_config_directory() -> PathBuf {
    ProjectDirs::from("net", "aggregat4", "meeters")
        .expect("Project directory must be available")
        .config_dir()
        .to_path_buf()
}

fn load_config() -> std::io::Result<()> {
    let config_file = get_config_directory().join("meeters_config.env");
    if !config_file.exists() {
        panic!(
            "Require the project configuration file to be present at {}",
            config_file.to_str().unwrap()
        );
    }
    dotenvy::from_path(config_file).expect("Can not load configuration file meeters_config.env");
    Ok(())
}

fn get_events_for_interval(
    events: Vec<Event>,
    start_time: DateTime<Tz>,
    end_time: DateTime<Tz>,
) -> Vec<Event> {
    let mut filtered_events = events
        .into_iter()
        .filter(|e| {
            // We check for events that are inside the interval OR overlap with the interval in some way
            (e.start_timestamp > start_time && e.start_timestamp < end_time)
                || (e.start_timestamp < start_time && e.end_timestamp > start_time)
                || (e.start_timestamp < end_time && e.end_timestamp > end_time)
        })
        .collect::<Vec<_>>();
    filtered_events.sort_by(|a, b| Ord::cmp(&a.start_timestamp, &b.start_timestamp));
    filtered_events
}

/// Time between two ical calendar download in milliseconds
const DEFAULT_POLLING_INTERVAL_MS: u128 = 2 * 60 * 1000;
/// The amount of time in seconds we want to be warned before the meeting starts
const DEFAULT_EVENT_WARNING_TIME_SECONDS: i64 = 60;
/// Default start hour for the timeline view (8 AM)
const DEFAULT_START_HOUR: i32 = 8;
/// Default end hour for the timeline view (8 PM)
const DEFAULT_END_HOUR: i32 = 20;
/// Default number of future days to show (1 = today + tomorrow)
const DEFAULT_FUTURE_DAYS: i32 = 1;

enum CalendarMessages {
    DayEvents(Vec<Vec<Event>>), // Vector of events for each day
    EventNotification(Event),
}

fn default_tz(_: dotenvy::Error) -> Result<String, dotenvy::Error> {
    Ok("Europe/Berlin".to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    load_config()?;

    // Parse config
    let local_tz_iana: String = dotenvy::var("MEETERS_LOCAL_TIMEZONE")
        .or_else(default_tz)
        .unwrap();
    let local_tz: Tz = local_tz_iana
        .parse()
        .expect("Expecting to be able to parse the local timezone, instead got an error");
    let config_ical_url = dotenvy::var("MEETERS_ICAL_URL")
        .expect("Expecting a configuration property with name MEETERS_ICAL_URL");
    let config_show_event_notification: bool = match dotenvy::var("MEETERS_EVENT_NOTIFICATION") {
        Ok(val) => val.parse::<bool>().expect(
            "Value for MEETERS_EVENT_NOTIFICATION configuration parameter must be a boolean",
        ),
        Err(_) => true,
    };
    let config_polling_interval_ms: u128 = match dotenvy::var("MEETERS_POLLING_INTERVAL_MS") {
        Ok(val) => val.parse::<u128>().expect("MEETERS_POLLING_INTERVAL_MS must be a positive integer expressing the polling interval in milliseconds"),
        Err(_) => DEFAULT_POLLING_INTERVAL_MS
    };
    let config_event_warning_time_seconds: i64 = match dotenvy::var("MEETERS_EVENT_WARNING_TIME_SECONDS") {
        Ok(val) => val.parse::<i64>().expect("MEETERS_EVENT_WARNING_TIME_SECONDS must be a positive integer expressing the polling interval in seconds"),
        Err(_) => DEFAULT_EVENT_WARNING_TIME_SECONDS
    };
    let config_start_hour: i32 = match dotenvy::var("MEETERS_TODAY_START_HOUR") {
        Ok(val) => val.parse::<i32>().expect("MEETERS_TODAY_START_HOUR defines the start hour of the today view,must be a positive integer between 0 and 23"),
        Err(_) => DEFAULT_START_HOUR
    };
    let config_end_hour: i32 = match dotenvy::var("MEETERS_TODAY_END_HOUR") {
        Ok(val) => val.parse::<i32>().expect("MEETERS_TODAY_END_HOUR defines the end hour of the today view, must be a positive integer between 0 and 23"),
        Err(_) => DEFAULT_END_HOUR
    };
    let config_future_days: i32 = match dotenvy::var("MEETERS_FUTURE_DAYS") {
        Ok(val) => val.parse::<i32>().expect("MEETERS_FUTURE_DAYS defines the number of future days to show in addition to today, must be a positive integer"),
        Err(_) => DEFAULT_FUTURE_DAYS
    };
    println!("Local Timezone configured as {}", local_tz_iana.clone());

    // Initialize GUI components
    let (mut indicator, window_manager) =
        gui::initialize_gui(config_start_hour, config_end_hour, config_future_days);

    // Create a message passing channel so we can communicate safely with the main GUI thread from our worker thread
    let (events_sender, events_receiver) =
        glib::MainContext::channel::<Result<CalendarMessages, ()>>(glib::Priority::DEFAULT);
    events_receiver.attach(None, move |event_result| {
        match event_result {
            Ok(DayEvents(day_events)) => {
                // Update window manager with new events
                let mut wm = window_manager.lock().unwrap();
                wm.update_events(day_events.clone());

                // Only show today's events in the indicator menu
                let empty_events = Vec::new();
                let today_events = day_events.first().unwrap_or(&empty_events);
                gui::create_indicator_menu(
                    today_events,
                    &mut indicator,
                    Arc::clone(&window_manager),
                );
            }
            Ok(EventNotification(event)) => {
                if config_show_event_notification {
                    gui::show_event_notification(event);
                }
            }
            Err(_) => gui::set_error_icon(&mut indicator),
        }
        glib::ControlFlow::Continue
    });

    // start the background thread for calendar work
    thread::spawn(move || {
        let mut last_download_time = 0;
        let mut last_events: Vec<Event> = vec![];
        let mut last_notification_start_time: Option<DateTime<Tz>> = None;
        loop {
            let current_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time must flow")
                .as_millis();
            if last_download_time == 0
                || current_time - last_download_time > config_polling_interval_ms
            {
                last_download_time = current_time;
                match get_ical(&config_ical_url)
                    .and_then(|t| meeters_ical::extract_events(&t, &local_tz))
                {
                    Ok(events) => {
                        println!("Successfully got {:?} events", events.len());
                        let local_date = Local::now().date_naive();

                        // Get events for each day
                        let mut day_events = Vec::new();

                        // Process each day
                        for day_offset in 0..=config_future_days {
                            let day_date = local_date + chrono::Duration::days(day_offset as i64);
                            let day_start = local_tz
                                .with_ymd_and_hms(
                                    day_date.year(),
                                    day_date.month(),
                                    day_date.day(),
                                    0,
                                    0,
                                    0,
                                )
                                .unwrap();
                            let day_end = local_tz
                                .with_ymd_and_hms(
                                    day_date.year(),
                                    day_date.month(),
                                    day_date.day(),
                                    23,
                                    59,
                                    59,
                                )
                                .unwrap();
                            let day_events_list =
                                get_events_for_interval(events.clone(), day_start, day_end);
                            println!(
                                "There are {} events for day {}: {:?}",
                                day_events_list.len(),
                                day_offset,
                                day_events_list
                            );
                            day_events.push(day_events_list);
                        }

                        // Store today's events for notifications
                        last_events = day_events[0].clone();

                        events_sender
                            .send(Ok(DayEvents(day_events)))
                            .expect("Channel should be sendable");
                    }
                    Err(e) => {
                        events_sender
                            .send(Err(()))
                            .expect("Channel should be sendable");
                        eprintln!("Error getting events: {:?}", e.msg);
                    }
                }
            }
            // Phase two of the background loop: check whether we have events that are close to occurring and trigger a notification
            // find the first event that is about to start in the next minute and if we did not notify before, send a notification
            let now = Local::now();
            let potential_next_immediate_upcoming_event = last_events.iter().find(|event| {
                let time_distance_from_now = event.start_timestamp.signed_duration_since(now);
                time_distance_from_now.num_seconds() > 0
                    && time_distance_from_now.num_seconds() <= config_event_warning_time_seconds
            });
            if let Some(next_immediate_upcoming_event) = potential_next_immediate_upcoming_event {
                if last_notification_start_time.is_none()
                    || next_immediate_upcoming_event.start_timestamp
                        != last_notification_start_time.unwrap()
                {
                    events_sender
                        .send(Ok(EventNotification(next_immediate_upcoming_event.clone())))
                        .expect("Channel should be sendable");
                    last_notification_start_time =
                        Some(next_immediate_upcoming_event.start_timestamp);
                }
            }
            thread::sleep(std::time::Duration::from_secs(5));
        }
    });

    // Run the GUI main loop
    gui::run_gui_main_loop();
    Ok(())
}
